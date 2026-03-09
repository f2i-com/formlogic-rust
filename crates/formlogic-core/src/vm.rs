use regex::RegexBuilder;
use serde_json::Value as JsonValue;
use std::borrow::Cow;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use std::{cell::UnsafeCell, rc::Rc};

use crate::bytecode::Bytecode;
use crate::code::Opcode;
use crate::config::FormLogicConfig;
use crate::object::{
    make_array, make_hash, undefined_object, unwrap_array, BuiltinFunction, BuiltinFunctionObject,
    CompiledFunctionObject, HashKey, HashObject, Object, PromiseObject, PromiseState,
    SuperRefObject,
};
use crate::value::{obj_into_val, obj_to_val, val_inspect, val_to_obj, Heap, Value};

// ── Platform-safe time helpers ────────────────────────────────────────────
// On WASM, use js_sys for Date.now() and Math.random() for real values.
// On native, use std::time.

/// Get the current epoch time in milliseconds (platform-safe).
#[cfg(not(target_arch = "wasm32"))]
fn epoch_millis_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

#[cfg(target_arch = "wasm32")]
fn epoch_millis_now() -> f64 {
    js_sys::Date::now()
}

/// Generate a seed for the xorshift64 RNG (platform-safe).
#[cfg(not(target_arch = "wasm32"))]
fn rng_seed_now() -> u64 {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x12345678_9abcdef0);
    if seed == 0 { 0x12345678_9abcdef0 } else { seed }
}

#[cfg(target_arch = "wasm32")]
fn rng_seed_now() -> u64 {
    // On WASM, use js_sys::Date::now() as seed for better entropy
    let seed = js_sys::Date::now() as u64;
    if seed == 0 { 0x12345678_9abcdef0 } else { seed }
}

pub const STACK_SIZE: usize = 2048;
pub const GLOBALS_SIZE: usize = 65_536;
pub const MAX_FRAMES: usize = 1024;

// Pre-allocated typeof result strings — zero allocation on every typeof call.
thread_local! {
    static TYPEOF_UNDEFINED: Rc<str> = Rc::from("undefined");
    static TYPEOF_OBJECT: Rc<str> = Rc::from("object");
    static TYPEOF_BOOLEAN: Rc<str> = Rc::from("boolean");
    static TYPEOF_NUMBER: Rc<str> = Rc::from("number");
    static TYPEOF_STRING: Rc<str> = Rc::from("string");
    static TYPEOF_FUNCTION: Rc<str> = Rc::from("function");
}
pub const MAX_ARRAY_SIZE: usize = 1_000_000;
pub const MAX_STRING_LENGTH: usize = 10_000_000;

#[derive(Clone, Debug, Default)]
pub struct ExecutionQuota {
    pub instructions: u64,
    #[cfg(not(target_arch = "wasm32"))]
    pub started_at: Option<Instant>,
    #[cfg(target_arch = "wasm32")]
    pub started_at_ms: Option<f64>,
}

#[derive(Debug)]
pub enum VMError {
    StackOverflow,
    StackUnderflow,
    InstructionOutOfBounds(usize),
    ExecutionTimeout(String),
    TypeError(String),
    InvalidOpcode(u8),
    /// Internal sentinel: a `yield` expression suspended the generator.
    /// Carries the yielded NaN-boxed Value.
    Yield(Value),
}

#[derive(Clone)]
pub struct SharedGlobals {
    inner: Rc<UnsafeCell<Vec<Value>>>,
    /// One past the highest global index that has been written.
    /// GC only needs to scan 0..high_water_mark instead of all 65536 slots.
    high_water_mark: Rc<std::cell::Cell<usize>>,
    /// Dirty bitset: tracks which global slots were written during VM execution.
    /// Used by the WASM bridge to return only mutated indices, eliminating the
    /// need for deepEqual on unchanged state variables.
    dirty: Rc<UnsafeCell<Vec<u64>>>,
}

impl SharedGlobals {
    fn new() -> Self {
        // 65536 globals / 64 bits per u64 = 1024 u64s
        Self {
            inner: Rc::new(UnsafeCell::new(vec![Value::UNDEFINED; GLOBALS_SIZE])),
            high_water_mark: Rc::new(std::cell::Cell::new(0)),
            dirty: Rc::new(UnsafeCell::new(vec![0u64; GLOBALS_SIZE / 64])),
        }
    }

    /// Returns one past the highest global index that has been written.
    /// GC and other scanning loops can use this to avoid iterating all 65536 slots.
    #[inline(always)]
    pub fn high_water_mark(&self) -> usize {
        self.high_water_mark.get()
    }

    /// Unchecked global set — caller must ensure `idx < GLOBALS_SIZE`.
    #[inline(always)]
    pub(crate) unsafe fn set_unchecked(&self, idx: usize, value: Value) {
        let globals = &mut *self.inner.get();
        debug_assert!(idx < globals.len(), "global index {} out of bounds", idx);
        *globals.get_unchecked_mut(idx) = value;
        // Update high-water mark if this is a new highest index
        let next = idx + 1;
        if next > self.high_water_mark.get() {
            self.high_water_mark.set(next);
        }
        // Mark this slot as dirty for VM→React sync optimization
        let dirty = &mut *self.dirty.get();
        *dirty.get_unchecked_mut(idx / 64) |= 1u64 << (idx % 64);
    }

    /// Unchecked global get — caller must ensure `idx < GLOBALS_SIZE`.
    #[inline(always)]
    pub(crate) unsafe fn get_unchecked(&self, idx: usize) -> Value {
        let globals = &*self.inner.get();
        debug_assert!(idx < globals.len(), "global index {} out of bounds", idx);
        *globals.get_unchecked(idx)
    }

    /// Check if a specific global slot is dirty.
    #[inline(always)]
    pub fn is_dirty(&self, idx: usize) -> bool {
        let dirty = unsafe { &*self.dirty.get() };
        let word = idx / 64;
        let bit = idx % 64;
        word < dirty.len() && (dirty[word] & (1u64 << bit)) != 0
    }

    /// Clear all dirty bits.
    #[inline]
    pub fn clear_dirty(&self) {
        let dirty = unsafe { &mut *self.dirty.get() };
        for word in dirty.iter_mut() {
            *word = 0;
        }
    }
}

/// Saved state of the caller when entering a compiled function via OpCall.
/// Restored by OpReturn/OpReturnValue without recursing into a nested `run()`.
pub(crate) struct CallFrame {
    ip: usize,
    instructions: Rc<Vec<u8>>,
    pub(crate) constants: Rc<Vec<Object>>,
    pub(crate) locals: Vec<Object>,
    sp: usize,
    inline_cache: Vec<(u32, u32)>,
    max_stack_depth: usize,
    /// The function's persistent cache handle — written back on return.
    /// `None` for functions with no property accesses (num_cache_slots == 0).
    func_cache: Option<Rc<crate::object::VmCell<Vec<(u32, u32)>>>>,
    /// True if the called function is async (return value wrapped in Promise).
    is_async: bool,
}

pub struct VM {
    pub constants: Rc<Vec<Object>>,
    pub instructions: Rc<Vec<u8>>,
    /// NaN-boxed value stack. Each element is 8 bytes (Copy).
    pub stack: Vec<Value>,
    pub sp: usize,
    pub ip: usize,
    /// Cached raw pointer to `instructions` data.
    /// Eliminates one indirection per bytecode read (Rc→Vec→data becomes ptr→data).
    /// SAFETY: Must be updated whenever `self.instructions` changes.
    pub(crate) inst_ptr: *const u8,
    /// Length of current instruction buffer (only used in debug_assert checks).
    pub(crate) inst_len: usize,
    pub globals: SharedGlobals,
    pub locals: Vec<Object>,
    pub config: FormLogicConfig,
    pub(crate) enforce_limits: bool,
    pub quota: ExecutionQuota,
    pub last_popped: Option<Value>,
    pub(crate) arg_buffer: Vec<Value>,
    pub(crate) string_concat_buf: String,
    pub(crate) locals_pool: Vec<Vec<Object>>,
    /// Inline property cache: indexed by cache_slot, stores (shape_version, pair_index).
    /// shape_version 0 means "uncached".
    pub(crate) inline_cache: Vec<(u32, u32)>,
    pub(crate) max_stack_depth: usize,
    /// Call-frame stack for non-recursive function dispatch.
    pub(crate) frames: Vec<CallFrame>,
    /// Heap for NaN-boxed Value objects (strings, arrays, hashes, etc.).
    pub heap: Heap,
    /// Number of registers used by the top-level program (register VM only).
    pub(crate) register_count: u16,
    /// Raw pointer to pre-converted constants as NaN-boxed Values (register VM only).
    /// Points into the active `constants_values_cache` entry. Set by `preconvert_constants()`.
    pub(crate) constants_values_ptr: *const Value,
    /// Scratch buffer for building constants on cache miss.
    pub(crate) constants_values_buf: Vec<Value>,
    /// Cache of pre-converted constants keyed by `Rc::as_ptr` of the
    /// `Rc<Vec<Object>>` constants. Avoids repeated heap allocation for
    /// string/function constants across recursive calls to the same function.
    pub(crate) constants_values_cache: Vec<(usize, Vec<Value>)>,
    /// Raw pointer to current function's constants (register VM only).
    /// Avoids Rc::clone on every function call. Safe because the Rc in
    /// the CompiledFunctionObject keeps the data alive, and the heap is
    /// append-only during VM execution.
    pub(crate) constants_raw: *const Vec<Object>,
    /// Pre-interned symbol IDs for string constants (register VM only).
    /// `constants_syms_ptr[i]` is the interned symbol ID if constant `i` is a string, else 0.
    /// Eliminates `intern_rc()` hash lookups on property access slow paths.
    pub(crate) constants_syms_buf: Vec<u32>,
    pub(crate) constants_syms_ptr: *const u32,
    pub(crate) constants_syms_cache: Vec<(usize, Vec<u32>)>,
    /// Cached `typeof` result Values — lazily initialized on first use.
    /// Avoids allocating `Rc<str>` on every `typeof` call.
    pub(crate) typeof_undefined: Value,
    pub(crate) typeof_number: Value,
    pub(crate) typeof_string: Value,
    pub(crate) typeof_boolean: Value,
    pub(crate) typeof_function: Value,
    pub(crate) typeof_object: Value,
    pub(crate) typeof_symbol: Value,
    /// Cached method symbol IDs for fast-path dispatch (lazily initialized).
    /// u32::MAX = uninitialized. Symbol IDs are stable across VM lifetime.
    pub(crate) sym_push: u32,
    pub(crate) sym_pop: u32,
    pub(crate) sym_length: u32,
    pub(crate) sym_set: u32,
    pub(crate) sym_get: u32,
    pub(crate) sym_has: u32,
    pub(crate) sym_size: u32,
    pub(crate) sym_shift: u32,
    pub(crate) sym_unshift: u32,
    pub(crate) sym_splice: u32,
    pub(crate) sym_has_own_property: u32,
    pub(crate) sym_then: u32,
    pub(crate) sym_catch: u32,
    /// Fast path for `preconvert_constants`: remembers the last-used constants_raw
    /// key and its resolved pointers. Skips the linear cache scan when the same
    /// function is called repeatedly (e.g. `add()` called 1000× from a loop).
    pub(crate) last_preconvert_key: usize,
    pub(crate) last_preconvert_values_ptr: *const Value,
    pub(crate) last_preconvert_syms_ptr: *const u32,
    /// The `new.target` value for the current constructor call.
    /// Set in `execute_new_with_args_slice` before running the constructor,
    /// saved/restored across nested `new` calls.
    pub(crate) new_target: Value,
    /// Xorshift64 PRNG state for Math.random().
    pub(crate) rng_state: u64,
    /// Pluggable localStorage backend (e.g. SQLite on native).
    pub local_storage: Option<Box<dyn crate::local_storage::LocalStorageBridge>>,
    /// Pluggable XDB database backend (e.g. SQLite on native).
    pub db: Option<Box<dyn crate::db_bridge::DbBridge>>,
    /// Pluggable 2D drawing backend (e.g. vello on native).
    pub draw: Option<Box<dyn crate::draw_bridge::DrawBridge>>,
    /// Pluggable CSS layout backend (e.g. taffy on native).
    pub layout: Option<Box<dyn crate::layout_bridge::LayoutBridge>>,
    /// Pluggable input/event state (e.g. winit on native).
    pub input: Option<Box<dyn crate::input_bridge::InputBridge>>,
    /// Pluggable HTTP backend (server-side).
    pub http: Option<Box<dyn crate::http_bridge::HttpBridge>>,
    /// Pluggable file system backend (server-side, scoped).
    pub fs: Option<Box<dyn crate::fs_bridge::FsBridge>>,
    /// Pluggable environment variable backend (server-side).
    pub env: Option<Box<dyn crate::env_bridge::EnvBridge>>,
    /// Event listeners registered via `window.addEventListener(type, handler)`.
    /// Maps event type (e.g. "keydown") to a list of handler Values (heap refs).
    pub event_listeners: std::collections::HashMap<String, Vec<Value>>,
    /// Pending async host calls queued by `softn.*` builtins.
    pub pending_host_calls: Vec<crate::host_bridge::PendingHostCall>,
    /// Callbacks stored by `softn.*` builtins, keyed by host call ID.
    pub host_callbacks: std::collections::HashMap<u32, Value>,
    /// Auto-incrementing ID for host calls.
    pub next_host_call_id: u32,
}

impl VM {
    #[inline(always)]
    fn is_terminator_byte(byte: u8) -> bool {
        byte == Opcode::OpReturn as u8
            || byte == Opcode::OpReturnValue as u8
            || byte == Opcode::OpHalt as u8
    }

    fn ensure_terminated_instructions(mut instructions: Vec<u8>) -> Vec<u8> {
        if instructions
            .last()
            .copied()
            .is_none_or(|b| !Self::is_terminator_byte(b))
        {
            instructions.push(Opcode::OpHalt as u8);
        }
        instructions
    }

    fn ensure_terminated_instructions_rc(instructions: Rc<Vec<u8>>) -> Rc<Vec<u8>> {
        if instructions
            .last()
            .copied()
            .is_some_and(Self::is_terminator_byte)
        {
            return instructions;
        }

        let mut owned = (*instructions).clone();
        owned.push(Opcode::OpHalt as u8);
        Rc::new(owned)
    }

    #[inline(always)]
    pub(crate) fn clone_object_fast(value: &Object) -> Object {
        match value {
            Object::Integer(v) => Object::Integer(*v),
            Object::Float(v) => Object::Float(*v),
            Object::Boolean(v) => Object::Boolean(*v),
            Object::Null => Object::Null,
            Object::Undefined => Object::Undefined,
            other => other.clone(),
        }
    }

    pub fn new(bytecode: Bytecode, config: FormLogicConfig) -> Self {
        let enforce_limits = config.max_instructions.is_some() || config.max_wall_time_ms.is_some();
        let mut stack = Vec::with_capacity(STACK_SIZE);
        stack.reserve(STACK_SIZE);
        let num_cache_slots = bytecode.num_cache_slots;
        let max_stack_depth = bytecode.max_stack_depth as usize;
        let instructions = Rc::new(Self::ensure_terminated_instructions(bytecode.instructions));
        let inst_ptr = instructions.as_ptr();
        let inst_len = instructions.len();
        Self {
            constants: Rc::new(bytecode.constants),
            instructions,
            stack,
            sp: 0,
            ip: 0,
            inst_ptr,
            inst_len,
            globals: SharedGlobals::new(),
            locals: vec![],
            config,
            enforce_limits,
            quota: ExecutionQuota::default(),
            last_popped: None,
            arg_buffer: Vec::with_capacity(8),
            string_concat_buf: String::new(),
            locals_pool: Vec::with_capacity(8),
            inline_cache: vec![(0, 0); num_cache_slots as usize],
            max_stack_depth,
            frames: Vec::with_capacity(16),
            heap: Heap::new(),
            register_count: bytecode.register_count,
            constants_values_ptr: std::ptr::null(),
            constants_values_buf: Vec::new(),
            constants_values_cache: Vec::new(),
            constants_raw: std::ptr::null(),
            constants_syms_buf: Vec::new(),
            constants_syms_ptr: std::ptr::null(),
            constants_syms_cache: Vec::new(),
            typeof_undefined: Value::UNDEFINED,
            typeof_number: Value::UNDEFINED,
            typeof_string: Value::UNDEFINED,
            typeof_boolean: Value::UNDEFINED,
            typeof_function: Value::UNDEFINED,
            typeof_object: Value::UNDEFINED,
            typeof_symbol: Value::UNDEFINED,
            sym_push: crate::intern::intern("push"),
            sym_pop: crate::intern::intern("pop"),
            sym_length: crate::intern::intern("length"),
            sym_set: crate::intern::intern("set"),
            sym_get: crate::intern::intern("get"),
            sym_has: crate::intern::intern("has"),
            sym_size: crate::intern::intern("size"),
            sym_shift: crate::intern::intern("shift"),
            sym_unshift: crate::intern::intern("unshift"),
            sym_splice: crate::intern::intern("splice"),
            sym_has_own_property: crate::intern::intern("hasOwnProperty"),
            sym_then: crate::intern::intern("then"),
            sym_catch: crate::intern::intern("catch"),
            last_preconvert_key: 0,
            last_preconvert_values_ptr: std::ptr::null(),
            last_preconvert_syms_ptr: std::ptr::null(),
            new_target: Value::UNDEFINED,
            rng_state: rng_seed_now(),
            local_storage: None,
            db: None,
            draw: None,
            layout: None,
            input: None,
            http: None,
            fs: None,
            env: None,
            event_listeners: std::collections::HashMap::new(),
            pending_host_calls: Vec::new(),
            host_callbacks: std::collections::HashMap::new(),
            next_host_call_id: 1,
        }
    }

    pub fn new_from_rc(
        instructions: Rc<Vec<u8>>,
        constants: Rc<Vec<Object>>,
        config: FormLogicConfig,
        initial_stack_capacity: usize,
        num_cache_slots: u16,
        max_stack_depth: u16,
    ) -> Self {
        Self::new_from_rc_with_globals(
            instructions,
            constants,
            config,
            initial_stack_capacity,
            SharedGlobals::new(),
            num_cache_slots,
            max_stack_depth,
        )
    }

    pub(crate) fn new_from_rc_with_globals(
        instructions: Rc<Vec<u8>>,
        constants: Rc<Vec<Object>>,
        config: FormLogicConfig,
        _initial_stack_capacity: usize,
        globals: SharedGlobals,
        num_cache_slots: u16,
        max_stack_depth: u16,
    ) -> Self {
        let enforce_limits = config.max_instructions.is_some() || config.max_wall_time_ms.is_some();
        let mut stack = Vec::with_capacity(STACK_SIZE);
        stack.reserve(STACK_SIZE);
        let instructions = Self::ensure_terminated_instructions_rc(instructions);
        let inst_ptr = instructions.as_ptr();
        let inst_len = instructions.len();
        Self {
            constants,
            instructions,
            stack,
            sp: 0,
            ip: 0,
            inst_ptr,
            inst_len,
            globals,
            locals: vec![],
            config,
            enforce_limits,
            quota: ExecutionQuota::default(),
            last_popped: None,
            arg_buffer: Vec::with_capacity(8),
            string_concat_buf: String::new(),
            locals_pool: Vec::with_capacity(8),
            inline_cache: vec![(0, 0); num_cache_slots as usize],
            max_stack_depth: max_stack_depth as usize,
            frames: Vec::with_capacity(16),
            heap: Heap::new(),
            register_count: 0,
            constants_values_ptr: std::ptr::null(),
            constants_values_buf: Vec::new(),
            constants_values_cache: Vec::new(),
            constants_raw: std::ptr::null(),
            constants_syms_buf: Vec::new(),
            constants_syms_ptr: std::ptr::null(),
            constants_syms_cache: Vec::new(),
            typeof_undefined: Value::UNDEFINED,
            typeof_number: Value::UNDEFINED,
            typeof_string: Value::UNDEFINED,
            typeof_boolean: Value::UNDEFINED,
            typeof_function: Value::UNDEFINED,
            typeof_object: Value::UNDEFINED,
            typeof_symbol: Value::UNDEFINED,
            sym_push: crate::intern::intern("push"),
            sym_pop: crate::intern::intern("pop"),
            sym_length: crate::intern::intern("length"),
            sym_set: crate::intern::intern("set"),
            sym_get: crate::intern::intern("get"),
            sym_has: crate::intern::intern("has"),
            sym_size: crate::intern::intern("size"),
            sym_shift: crate::intern::intern("shift"),
            sym_unshift: crate::intern::intern("unshift"),
            sym_splice: crate::intern::intern("splice"),
            sym_has_own_property: crate::intern::intern("hasOwnProperty"),
            sym_then: crate::intern::intern("then"),
            sym_catch: crate::intern::intern("catch"),
            last_preconvert_key: 0,
            last_preconvert_values_ptr: std::ptr::null(),
            last_preconvert_syms_ptr: std::ptr::null(),
            new_target: Value::UNDEFINED,
            rng_state: rng_seed_now(),
            local_storage: None,
            db: None,
            draw: None,
            layout: None,
            input: None,
            http: None,
            fs: None,
            env: None,
            event_listeners: std::collections::HashMap::new(),
            pending_host_calls: Vec::new(),
            host_callbacks: std::collections::HashMap::new(),
            next_host_call_id: 1,
        }
    }

    /// Reset VM state for a new execution, reusing allocated buffers.
    /// Loads new bytecode and resets all execution state.
    /// Allocated buffers (stack, arg_buffer, locals_pool, frames) keep their
    /// capacity across calls. The Heap drops objects but keeps Vec capacity.
    pub fn reset_for_run(
        &mut self,
        instructions: Rc<Vec<u8>>,
        constants: Rc<Vec<Object>>,
        num_cache_slots: u16,
        max_stack_depth: u16,
        register_count: u16,
    ) {
        let instructions = Self::ensure_terminated_instructions_rc(instructions);
        self.inst_ptr = instructions.as_ptr();
        self.inst_len = instructions.len();
        self.instructions = instructions;
        self.constants = constants;
        self.ip = 0;
        self.sp = 0;
        self.stack.clear();
        self.last_popped = None;
        self.locals.clear();
        self.arg_buffer.clear();
        // locals_pool: keep pooled vecs for reuse (don't clear)
        self.frames.clear();
        self.heap.reset();
        // Migrate local_objects in compiler-constructed hashes to VM heap
        for obj in self.constants.iter() {
            if let Object::Hash(hash_rc) = obj {
                hash_rc.borrow_mut().migrate_local_objects(&mut self.heap);
            }
        }
        self.globals = SharedGlobals::new();
        self.quota = ExecutionQuota::default();
        self.inline_cache.clear();
        self.inline_cache.resize(num_cache_slots as usize, (0, 0));
        self.max_stack_depth = max_stack_depth as usize;
        self.register_count = register_count;
        self.constants_values_ptr = std::ptr::null();
        self.constants_values_buf.clear();
        self.constants_values_cache.clear();
        self.constants_raw = std::ptr::null();
        self.constants_syms_ptr = std::ptr::null();
        self.constants_syms_buf.clear();
        self.constants_syms_cache.clear();
        // Reset preconvert cache — old pointers are dangling after cache clear
        self.last_preconvert_key = 0;
        self.last_preconvert_values_ptr = std::ptr::null();
        self.last_preconvert_syms_ptr = std::ptr::null();
        // Reset typeof cache — heap was cleared, old Values are invalid
        self.typeof_undefined = Value::UNDEFINED;
        self.typeof_number = Value::UNDEFINED;
        self.typeof_string = Value::UNDEFINED;
        self.typeof_boolean = Value::UNDEFINED;
        self.typeof_function = Value::UNDEFINED;
        self.typeof_object = Value::UNDEFINED;
    }

    /// Push an Object onto the Value stack (converts Object → Value).
    #[inline(always)]
    pub fn push(&mut self, obj: Object) -> Result<(), VMError> {
        if self.sp >= STACK_SIZE {
            return Err(VMError::StackOverflow);
        }
        let val = obj_into_val(obj, &mut self.heap);
        unsafe {
            let ptr = self.stack.as_mut_ptr().add(self.sp);
            std::ptr::write(ptr, val);
            self.stack.set_len(self.sp + 1);
        }
        self.sp += 1;
        Ok(())
    }

    /// Push a Value directly (no conversion needed).
    #[inline(always)]
    pub fn push_val(&mut self, val: Value) -> Result<(), VMError> {
        if self.sp >= STACK_SIZE {
            return Err(VMError::StackOverflow);
        }
        unsafe {
            let ptr = self.stack.as_mut_ptr().add(self.sp);
            std::ptr::write(ptr, val);
            self.stack.set_len(self.sp + 1);
        }
        self.sp += 1;
        Ok(())
    }

    /// Push a Value without bounds checking. Caller must have verified stack capacity.
    #[inline(always)]
    unsafe fn push_unchecked(&mut self, val: Value) {
        debug_assert!(self.sp < STACK_SIZE, "push_unchecked: stack overflow");
        let ptr = self.stack.as_mut_ptr().add(self.sp);
        std::ptr::write(ptr, val);
        self.stack.set_len(self.sp + 1);
        self.sp += 1;
    }

    /// Push an Object without bounds checking (converts Object → Value).
    #[inline(always)]
    unsafe fn push_obj_unchecked(&mut self, obj: Object) {
        let val = obj_into_val(obj, &mut self.heap);
        self.push_unchecked(val);
    }

    /// Pop a Value from the stack.
    #[inline(always)]
    pub fn pop_val(&mut self) -> Result<Value, VMError> {
        if self.sp == 0 {
            return Err(VMError::StackUnderflow);
        }
        self.sp -= 1;
        unsafe {
            let val = *self.stack.as_ptr().add(self.sp);
            self.stack.set_len(self.sp);
            Ok(val)
        }
    }

    /// Pop a Value and convert to Object.
    #[inline(always)]
    pub fn pop(&mut self) -> Result<Object, VMError> {
        let val = self.pop_val()?;
        Ok(val_to_obj(val, &self.heap))
    }

    /// Pop a Value without bounds checking.
    #[inline(always)]
    unsafe fn pop_unchecked(&mut self) -> Value {
        debug_assert!(self.sp > 0, "pop_unchecked: stack underflow");
        self.sp -= 1;
        // Read from the raw pointer before truncating, since Value is Copy
        // and get_unchecked would fail after set_len shrinks the Vec.
        let val = *self.stack.as_ptr().add(self.sp);
        self.stack.set_len(self.sp);
        val
    }

    /// Pop a Value and convert to Object without bounds checking.
    #[inline(always)]
    unsafe fn pop_obj(&mut self) -> Object {
        let val = self.pop_unchecked();
        val_to_obj(val, &self.heap)
    }

    /// Push an Object onto the stack (converts to Value). No bounds checking.
    #[inline(always)]
    #[allow(dead_code)]
    fn push_obj(&mut self, obj: Object) {
        let val = obj_into_val(obj, &mut self.heap);
        unsafe { self.push_unchecked(val) };
    }

    /// Get an original Object constant by index (for property name strings, etc.)
    #[inline(always)]
    #[allow(dead_code)]
    fn const_obj(&self, idx: usize) -> &Object {
        debug_assert!(
            idx < self.constants.len(),
            "const_obj index {} out of bounds",
            idx
        );
        unsafe { self.constants.get_unchecked(idx) }
    }

    #[inline(always)]
    pub(crate) fn read_u16(&self, offset: usize) -> u16 {
        // SAFETY: The compiler guarantees valid bytecode with correct operand lengths.
        // All callers pass self.ip + N where self.ip points to a valid multi-byte opcode.
        // inst_ptr is kept in sync with self.instructions by all code paths that
        // reassign self.instructions.
        debug_assert!(offset + 1 < self.inst_len);
        unsafe { u16::from_be((self.inst_ptr.add(offset) as *const u16).read_unaligned()) }
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn check_execution_limits(&mut self) -> Result<(), VMError> {
        self.quota.instructions += 1;
        #[cfg(not(target_arch = "wasm32"))]
        if self.quota.started_at.is_none() {
            self.quota.started_at = Some(Instant::now());
        }

        let max_instructions = self.config.max_instructions;
        let must_check_now = (self.quota.instructions & 0x3fff) == 0
            || max_instructions
                .map(|m| self.quota.instructions >= m)
                .unwrap_or(false);

        if !must_check_now {
            return Ok(());
        }

        if let Some(max) = max_instructions {
            if self.quota.instructions > max {
                return Err(VMError::ExecutionTimeout(format!(
                    "Execution exceeded maximum instruction count: {}",
                    max
                )));
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(max_ms) = self.config.max_wall_time_ms {
            let elapsed = self
                .quota
                .started_at
                .map(|x| x.elapsed())
                .unwrap_or_else(|| Duration::from_millis(0));
            if elapsed > Duration::from_millis(max_ms) {
                return Err(VMError::ExecutionTimeout(format!(
                    "Execution exceeded maximum wall time: {}ms",
                    max_ms
                )));
            }
        }

        #[cfg(target_arch = "wasm32")]
        if let Some(max_ms) = self.config.max_wall_time_ms {
            let now = epoch_millis_now();
            let started = *self.quota.started_at_ms.get_or_insert(now);
            if now - started > max_ms as f64 {
                return Err(VMError::ExecutionTimeout(format!(
                    "Execution exceeded maximum wall time: {}ms",
                    max_ms
                )));
            }
        }
        Ok(())
    }

    pub fn run(&mut self) -> Result<(), VMError> {
        if self.max_stack_depth > 0 && self.sp + self.max_stack_depth > STACK_SIZE {
            return Err(VMError::StackOverflow);
        }

        let entry_depth = self.frames.len();

        match self.dispatch_loop(entry_depth) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Unwind any frames pushed during this run() invocation.
                self.unwind_frames(entry_depth);
                Err(e)
            }
        }
    }

    /// Unwind call frames back to the given depth, restoring state.
    pub(crate) fn unwind_frames(&mut self, target_depth: usize) {
        while self.frames.len() > target_depth {
            self.restore_caller_frame();
        }
    }

    /// Pop one call frame and restore the caller's state. Returns whether
    /// the popped frame was async (needed to decide if the return value
    /// should be wrapped in a Promise).
    #[inline(never)]
    pub(crate) fn restore_caller_frame(&mut self) -> bool {
        let frame = self.frames.pop().unwrap();
        // Write the function's warm cache back to its persistent storage,
        // but only if the function actually uses inline caching.
        if let Some(ref func_cache) = frame.func_cache {
            *func_cache.borrow_mut() =
                std::mem::replace(&mut self.inline_cache, frame.inline_cache);
        }
        // Return used locals to pool for reuse.
        let mut used_locals = std::mem::replace(&mut self.locals, frame.locals);
        used_locals.clear();
        self.locals_pool.push(used_locals);
        self.instructions = frame.instructions;
        self.inst_ptr = self.instructions.as_ptr();
        self.inst_len = self.instructions.len();
        self.constants = frame.constants;
        self.ip = frame.ip;
        // The stack pointer should already be at the correct depth after
        // OpReturnValue pops the return value. Force sp to frame.sp to
        // handle any unbalanced stack from expression temporaries.
        self.sp = frame.sp;
        self.max_stack_depth = frame.max_stack_depth;
        frame.is_async
    }

    #[inline(always)]
    fn dispatch_loop(&mut self, entry_depth: usize) -> Result<(), VMError> {
        loop {
            // SAFETY: Compiler emits explicit OpHalt/OpReturn terminators.
            // inst_ptr is kept in sync with self.instructions.
            let byte = unsafe { *self.inst_ptr.add(self.ip) };
            // SAFETY: Compiler only emits valid opcode bytes in range 0..=OpHalt.
            // Opcode is #[repr(u8)] with contiguous variants.
            debug_assert!(byte <= Opcode::OpHalt as u8, "Invalid opcode byte: {byte}");
            let op: Opcode = unsafe { std::mem::transmute(byte) };

            match op {
                Opcode::OpConstant => {
                    let idx = self.read_u16(self.ip + 1) as usize;
                    // SAFETY: compiler guarantees valid constant indices.
                    debug_assert!(
                        idx < self.constants.len(),
                        "constant index {} out of bounds",
                        idx
                    );
                    let obj = unsafe { Self::clone_object_fast(self.constants.get_unchecked(idx)) };
                    unsafe { self.push_obj_unchecked(obj) };
                    self.ip += 3;
                }
                Opcode::OpAdd => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    if left.is_i32() && right.is_i32() {
                        let a = unsafe { left.as_i32_unchecked() };
                        let b = unsafe { right.as_i32_unchecked() };
                        let result = match a.checked_add(b) {
                            Some(sum) => Value::from_i32(sum),
                            None => Value::from_f64(a as f64 + b as f64),
                        };
                        unsafe { self.push_unchecked(result) };
                    } else if left.is_number() && right.is_number() {
                        unsafe {
                            self.push_unchecked(Value::from_f64(
                                left.to_number() + right.to_number(),
                            ))
                        };
                    } else {
                        // Coerce Instance objects via valueOf/toString before add
                        let left = self.coerce_instance_for_add(left)?;
                        let right = self.coerce_instance_for_add(right)?;
                        let lo = val_to_obj(left, &self.heap);
                        let ro = val_to_obj(right, &self.heap);
                        let result = self.add_objects_buffered(lo, ro)?;
                        self.push_obj(result);
                    }
                    self.ip += 1;
                }
                Opcode::OpSub => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    if left.is_i32() && right.is_i32() {
                        let a = unsafe { left.as_i32_unchecked() };
                        let b = unsafe { right.as_i32_unchecked() };
                        let result = match a.checked_sub(b) {
                            Some(diff) => Value::from_i32(diff),
                            None => Value::from_f64(a as f64 - b as f64),
                        };
                        unsafe { self.push_unchecked(result) };
                    } else if left.is_number() && right.is_number() {
                        unsafe {
                            self.push_unchecked(Value::from_f64(
                                left.to_number() - right.to_number(),
                            ))
                        };
                    } else {
                        let a = self.coerce_to_number_val(left)?;
                        let b = self.coerce_to_number_val(right)?;
                        let out = a - b;
                        if out.is_finite() && out.fract() == 0.0 {
                            self.push_val(Value::from_i64(out as i64))?;
                        } else {
                            self.push_val(Value::from_f64(out))?;
                        }
                    }
                    self.ip += 1;
                }
                Opcode::OpMul => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    if left.is_i32() && right.is_i32() {
                        let a = unsafe { left.as_i32_unchecked() };
                        let b = unsafe { right.as_i32_unchecked() };
                        let result = match a.checked_mul(b) {
                            Some(prod) => Value::from_i32(prod),
                            None => Value::from_f64(a as f64 * b as f64),
                        };
                        unsafe { self.push_unchecked(result) };
                    } else if left.is_number() && right.is_number() {
                        unsafe {
                            self.push_unchecked(Value::from_f64(
                                left.to_number() * right.to_number(),
                            ))
                        };
                    } else {
                        let a = self.coerce_to_number_val(left)?;
                        let b = self.coerce_to_number_val(right)?;
                        let out = a * b;
                        if out.is_finite() && out.fract() == 0.0 {
                            self.push_val(Value::from_i64(out as i64))?;
                        } else {
                            self.push_val(Value::from_f64(out))?;
                        }
                    }
                    self.ip += 1;
                }
                Opcode::OpDiv => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    if left.is_i32() && right.is_i32() {
                        let a = unsafe { left.as_i32_unchecked() };
                        let b = unsafe { right.as_i32_unchecked() };
                        if b != 0 && a % b == 0 {
                            unsafe { self.push_unchecked(Value::from_i32(a / b)) };
                        } else {
                            unsafe { self.push_unchecked(Value::from_f64(a as f64 / b as f64)) };
                        }
                    } else if left.is_number() && right.is_number() {
                        unsafe {
                            self.push_unchecked(Value::from_f64(
                                left.to_number() / right.to_number(),
                            ))
                        };
                    } else {
                        let a = self.coerce_to_number_val(left)?;
                        let b = self.coerce_to_number_val(right)?;
                        self.push_val(Value::from_f64(a / b))?;
                    }
                    self.ip += 1;
                }
                Opcode::OpMod => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    if left.is_i32() && right.is_i32() {
                        let a = unsafe { left.as_i32_unchecked() };
                        let b = unsafe { right.as_i32_unchecked() };
                        if b != 0 {
                            unsafe { self.push_unchecked(Value::from_i32(a % b)) };
                        } else {
                            unsafe { self.push_unchecked(Value::from_f64(f64::NAN)) };
                        }
                    } else if left.is_number() && right.is_number() {
                        unsafe {
                            self.push_unchecked(Value::from_f64(
                                left.to_number() % right.to_number(),
                            ))
                        };
                    } else {
                        let a = self.coerce_to_number_val(left)?;
                        let b = self.coerce_to_number_val(right)?;
                        let out = a % b;
                        if out.is_finite() && out.fract() == 0.0 {
                            self.push_val(Value::from_i64(out as i64))?;
                        } else {
                            self.push_val(Value::from_f64(out))?;
                        }
                    }
                    self.ip += 1;
                }
                Opcode::OpPow => {
                    let right = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    let base = self.to_number(&left)?;
                    let exp = self.to_number(&right)?;
                    let out = base.powf(exp);
                    if out.is_finite() && out.fract() == 0.0 {
                        unsafe { self.push_unchecked(Value::from_i64(out as i64)) };
                    } else {
                        unsafe { self.push_unchecked(Value::from_f64(out)) };
                    }
                    self.ip += 1;
                }
                Opcode::OpPop => {
                    let popped = unsafe { self.pop_unchecked() };
                    self.last_popped = Some(popped);
                    self.ip += 1;
                }
                Opcode::OpJump => {
                    let pos = self.read_u16(self.ip + 1) as usize;
                    // Check execution limits only on backward jumps (loop iterations).
                    if self.enforce_limits && pos <= self.ip {
                        self.check_execution_limits()?;
                    }
                    self.ip = pos;
                }
                Opcode::OpJumpNotTruthy => {
                    let pos = self.read_u16(self.ip + 1) as usize;
                    let condition = unsafe { self.pop_unchecked() };
                    if condition.is_truthy_full(&self.heap) {
                        self.ip += 3;
                    } else {
                        self.ip = pos;
                    }
                }
                // Fused: if !(local[idx] < constants[const_idx]) jump target.
                // Operands: [local_idx: u16, const_idx: u16, jump_target: u16]
                Opcode::OpTestLocalLtConstJump => {
                    let local_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let jump_target = self.read_u16(self.ip + 5) as usize;
                    let local_val = unsafe { self.locals.get_unchecked(local_idx) };
                    let const_val = unsafe { self.constants.get_unchecked(const_idx) };
                    let passes = match (local_val, const_val) {
                        (Object::Integer(a), Object::Integer(b)) => *a < *b,
                        _ => self.compare_numeric(local_val, const_val, Opcode::OpLessThan)?,
                    };
                    if passes {
                        self.ip += 7;
                    } else {
                        self.ip = jump_target;
                    }
                }
                // Fused: if !(local[idx] <= constants[const_idx]) jump target.
                Opcode::OpTestLocalLeConstJump => {
                    let local_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let jump_target = self.read_u16(self.ip + 5) as usize;
                    let local_val = unsafe { self.locals.get_unchecked(local_idx) };
                    let const_val = unsafe { self.constants.get_unchecked(const_idx) };
                    let passes = match (local_val, const_val) {
                        (Object::Integer(a), Object::Integer(b)) => *a <= *b,
                        _ => self.compare_numeric(local_val, const_val, Opcode::OpLessOrEqual)?,
                    };
                    if passes {
                        self.ip += 7;
                    } else {
                        self.ip = jump_target;
                    }
                }
                // Fused: local[idx] += constants[const_idx] (numeric add), no stack effect.
                // Operands: [local_idx: u16, const_idx: u16]
                Opcode::OpIncrementLocal => {
                    let local_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let local_val = unsafe { self.locals.get_unchecked(local_idx) };
                    let const_val = unsafe { self.constants.get_unchecked(const_idx) };
                    let result = match (local_val, const_val) {
                        (Object::Integer(a), Object::Integer(b)) => Object::Integer(*a + *b),
                        (Object::Float(a), Object::Float(b)) => Object::Float(*a + *b),
                        _ => self.add_objects(local_val, const_val)?,
                    };
                    unsafe { *self.locals.get_unchecked_mut(local_idx) = result };
                    self.ip += 5;
                }
                // Fused: globals[idx] += constants[const_idx] (numeric add), no stack effect.
                // Operands: [global_idx: u16, const_idx: u16]
                Opcode::OpIncrementGlobal => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let const_obj = unsafe { self.constants.get_unchecked(const_idx) };
                    let result = if gval.is_i32() {
                        if let Object::Integer(b) = const_obj {
                            Value::from_i64(unsafe { gval.as_i32_unchecked() } as i64 + *b)
                        } else if let Object::Float(b) = const_obj {
                            Value::from_f64(gval.to_number() + *b)
                        } else {
                            let go = val_to_obj(gval, &self.heap);
                            obj_into_val(self.add_objects(&go, const_obj)?, &mut self.heap)
                        }
                    } else if gval.is_f64() {
                        if let Object::Integer(b) = const_obj {
                            Value::from_f64(gval.to_number() + *b as f64)
                        } else if let Object::Float(b) = const_obj {
                            Value::from_f64(gval.to_number() + *b)
                        } else {
                            let go = val_to_obj(gval, &self.heap);
                            obj_into_val(self.add_objects(&go, const_obj)?, &mut self.heap)
                        }
                    } else {
                        let go = val_to_obj(gval, &self.heap);
                        obj_into_val(self.add_objects(&go, const_obj)?, &mut self.heap)
                    };
                    unsafe { self.globals.set_unchecked(global_idx, result) };
                    self.ip += 5;
                }
                // Fused: if !(globals[idx] < constants[const_idx]) jump target.
                // Operands: [global_idx: u16, const_idx: u16, jump_target: u16]
                Opcode::OpTestGlobalLtConstJump => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let jump_target = self.read_u16(self.ip + 5) as usize;
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let const_obj = unsafe { self.constants.get_unchecked(const_idx) };
                    let passes = if gval.is_i32() {
                        if let Object::Integer(b) = const_obj {
                            (unsafe { gval.as_i32_unchecked() } as i64) < *b
                        } else {
                            let go = val_to_obj(gval, &self.heap);
                            self.compare_numeric(&go, const_obj, Opcode::OpLessThan)?
                        }
                    } else {
                        let go = val_to_obj(gval, &self.heap);
                        self.compare_numeric(&go, const_obj, Opcode::OpLessThan)?
                    };
                    if passes {
                        self.ip += 7;
                    } else {
                        self.ip = jump_target;
                    }
                }
                // Fused: if !(globals[idx] <= constants[const_idx]) jump target.
                // Operands: [global_idx: u16, const_idx: u16, jump_target: u16]
                Opcode::OpTestGlobalLeConstJump => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let jump_target = self.read_u16(self.ip + 5) as usize;
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let const_obj = unsafe { self.constants.get_unchecked(const_idx) };
                    let passes = if gval.is_i32() {
                        if let Object::Integer(b) = const_obj {
                            (unsafe { gval.as_i32_unchecked() } as i64) <= *b
                        } else {
                            let go = val_to_obj(gval, &self.heap);
                            self.compare_numeric(&go, const_obj, Opcode::OpLessOrEqual)?
                        }
                    } else {
                        let go = val_to_obj(gval, &self.heap);
                        self.compare_numeric(&go, const_obj, Opcode::OpLessOrEqual)?
                    };
                    if passes {
                        self.ip += 7;
                    } else {
                        self.ip = jump_target;
                    }
                }
                // Fused: if !((globals[idx] % constants[mod_const]) === constants[cmp_const]) jump target.
                // Replaces 6 opcodes: OpGetGlobal + OpConstant + OpMod + OpConstant + OpStrictEqual + OpJumpNotTruthy
                // Operands: [global_idx: u16, mod_const_idx: u16, cmp_const_idx: u16, jump_target: u16]
                Opcode::OpModGlobalConstStrictEqConstJump => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let mod_const_idx = self.read_u16(self.ip + 3) as usize;
                    let cmp_const_idx = self.read_u16(self.ip + 5) as usize;
                    let jump_target = self.read_u16(self.ip + 7) as usize;
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let mod_const = unsafe { self.constants.get_unchecked(mod_const_idx) };
                    let cmp_const = unsafe { self.constants.get_unchecked(cmp_const_idx) };
                    // Fast path: all integers (the common case for `i % 3 === 0`).
                    let passes = if gval.is_i32() {
                        if let (Object::Integer(b), Object::Integer(c)) = (mod_const, cmp_const) {
                            if *b != 0 {
                                (unsafe { gval.as_i32_unchecked() } as i64 % *b) == *c
                            } else {
                                let go = val_to_obj(gval, &self.heap);
                                let mod_result = self.mod_objects(&go, mod_const)?;
                                self.strict_equals(&mod_result, cmp_const)
                            }
                        } else {
                            let go = val_to_obj(gval, &self.heap);
                            let mod_result = match (&go, mod_const) {
                                (Object::Integer(a), Object::Integer(b)) if *b != 0 => {
                                    Object::Integer(*a % *b)
                                }
                                (Object::Float(a), Object::Float(b)) => Object::Float(a % b),
                                (Object::Integer(a), Object::Float(b)) => {
                                    Object::Float(*a as f64 % b)
                                }
                                (Object::Float(a), Object::Integer(b)) => {
                                    Object::Float(a % *b as f64)
                                }
                                _ => self.mod_objects(&go, mod_const)?,
                            };
                            self.strict_equals(&mod_result, cmp_const)
                        }
                    } else {
                        // Slow path: compute mod, then strict-equal.
                        let go = val_to_obj(gval, &self.heap);
                        let mod_result = match (&go, mod_const) {
                            (Object::Integer(a), Object::Integer(b)) if *b != 0 => {
                                Object::Integer(*a % *b)
                            }
                            (Object::Float(a), Object::Float(b)) => Object::Float(a % b),
                            (Object::Integer(a), Object::Float(b)) => Object::Float(*a as f64 % b),
                            (Object::Float(a), Object::Integer(b)) => Object::Float(a % *b as f64),
                            _ => self.mod_objects(&go, mod_const)?,
                        };
                        self.strict_equals(&mod_result, cmp_const)
                    };
                    if passes {
                        self.ip += 9;
                    } else {
                        self.ip = jump_target;
                    }
                }
                // Fused: globals[target] = globals[target] + globals[source] (numeric add).
                // Operands: [target_idx: u16, source_idx: u16]
                Opcode::OpAccumulateGlobal => {
                    let target_idx = self.read_u16(self.ip + 1) as usize;
                    let source_idx = self.read_u16(self.ip + 3) as usize;
                    let tval = unsafe { self.globals.get_unchecked(target_idx) };
                    let sval = unsafe { self.globals.get_unchecked(source_idx) };
                    let result = if tval.is_i32() && sval.is_i32() {
                        Value::from_i64(
                            unsafe { tval.as_i32_unchecked() } as i64
                                + unsafe { sval.as_i32_unchecked() } as i64,
                        )
                    } else if tval.is_number() && sval.is_number() {
                        Value::from_f64(tval.to_number() + sval.to_number())
                    } else {
                        let to = val_to_obj(tval, &self.heap);
                        let so = val_to_obj(sval, &self.heap);
                        obj_into_val(self.add_objects(&to, &so)?, &mut self.heap)
                    };
                    unsafe { self.globals.set_unchecked(target_idx, result) };
                    self.ip += 5;
                }
                // Fused: globals[idx] += constants[const_idx]; jump to target.
                // Operands: [global_idx: u16, const_idx: u16, jump_target: u16]
                Opcode::OpIncrementGlobalAndJump => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let jump_target = self.read_u16(self.ip + 5) as usize;
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let const_obj = unsafe { self.constants.get_unchecked(const_idx) };
                    let result = if gval.is_i32() {
                        if let Object::Integer(b) = const_obj {
                            Value::from_i64(unsafe { gval.as_i32_unchecked() } as i64 + *b)
                        } else if let Object::Float(b) = const_obj {
                            Value::from_f64(gval.to_number() + *b)
                        } else {
                            let go = val_to_obj(gval, &self.heap);
                            obj_into_val(self.add_objects(&go, const_obj)?, &mut self.heap)
                        }
                    } else if gval.is_f64() {
                        if let Object::Integer(b) = const_obj {
                            Value::from_f64(gval.to_number() + *b as f64)
                        } else if let Object::Float(b) = const_obj {
                            Value::from_f64(gval.to_number() + *b)
                        } else {
                            let go = val_to_obj(gval, &self.heap);
                            obj_into_val(self.add_objects(&go, const_obj)?, &mut self.heap)
                        }
                    } else {
                        let go = val_to_obj(gval, &self.heap);
                        obj_into_val(self.add_objects(&go, const_obj)?, &mut self.heap)
                    };
                    unsafe { self.globals.set_unchecked(global_idx, result) };
                    // Backward jump — check execution limits.
                    if self.enforce_limits {
                        self.check_execution_limits()?;
                    }
                    self.ip = jump_target;
                }
                Opcode::OpSetGlobal => {
                    let idx = self.read_u16(self.ip + 1) as usize;
                    let val = unsafe { self.pop_unchecked() };
                    // SAFETY: compiler guarantees valid global indices < GLOBALS_SIZE.
                    unsafe { self.globals.set_unchecked(idx, val) };
                    self.ip += 3;
                }
                Opcode::OpSetLocal => {
                    let idx = self.read_u16(self.ip + 1) as usize;
                    let value = unsafe { self.pop_obj() };
                    // SAFETY: locals are pre-sized to num_locals in execute_compiled_function_slice.
                    debug_assert!(
                        idx < self.locals.len(),
                        "OpSetLocal index {} out of bounds (len {})",
                        idx,
                        self.locals.len()
                    );
                    unsafe {
                        *self.locals.get_unchecked_mut(idx) = value;
                    }
                    self.ip += 3;
                }
                Opcode::OpGetGlobal => {
                    let idx = self.read_u16(self.ip + 1) as usize;
                    // SAFETY: compiler guarantees valid global indices < GLOBALS_SIZE.
                    let val = unsafe { self.globals.get_unchecked(idx) };
                    unsafe { self.push_unchecked(val) };
                    self.ip += 3;
                }
                Opcode::OpGetGlobalProperty => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let cache_slot = self.read_u16(self.ip + 5) as usize;
                    // SAFETY: compiler guarantees valid global indices < GLOBALS_SIZE.
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let receiver = val_to_obj(gval, &self.heap);
                    // Inline cache fast path: avoid Rc::clone of property string entirely.
                    // Skip fast path when accessors exist so getters are invoked.
                    if let Object::Hash(hash_rc) = &receiver {
                        let hash = hash_rc.borrow();
                        if !hash.has_accessors() {
                            debug_assert!(cache_slot < self.inline_cache.len());
                            let (cached_shape, cached_offset) =
                                unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                            if cached_shape == hash.shape_version {
                                let slot = cached_offset as usize;
                                debug_assert!(slot < hash.values.len());
                                let value = unsafe { hash.get_value_at_slot_unchecked(slot) };
                                let out = val_to_obj(value, &self.heap);
                                unsafe { self.push_obj_unchecked(out) };
                                self.ip += 7;
                                continue;
                            }
                        }
                    }
                    // Cache miss or has accessors: fall through to full path.
                    let prop_str = match &self.constants[const_idx] {
                        Object::String(s) => crate::intern::intern_rc(s),
                        _ => {
                            return Err(VMError::TypeError(
                                "OpGetGlobalProperty constant must be a string".to_string(),
                            ));
                        }
                    };
                    self.get_property_fast_path_ref(&receiver, prop_str, cache_slot)?;
                    self.ip += 7;
                }
                Opcode::OpGetLocal => {
                    let idx = self.read_u16(self.ip + 1) as usize;
                    // SAFETY: compiler guarantees valid local indices;
                    // locals are pre-sized to num_locals in execute_compiled_function_slice.
                    debug_assert!(
                        idx < self.locals.len(),
                        "OpGetLocal index {} out of bounds (len {})",
                        idx,
                        self.locals.len()
                    );
                    let value = unsafe { Self::clone_object_fast(self.locals.get_unchecked(idx)) };
                    unsafe { self.push_obj_unchecked(value) };
                    self.ip += 3;
                }
                Opcode::OpGetLocalProperty => {
                    let local_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let cache_slot = self.read_u16(self.ip + 5) as usize;
                    if local_idx >= self.locals.len() {
                        return Err(VMError::InstructionOutOfBounds(self.ip + 1));
                    }
                    // SAFETY: bounds checked above; locals length is unchanged during opcode.
                    let receiver = unsafe { &*self.locals.as_ptr().add(local_idx) };
                    // Inline cache fast path: avoid Rc::clone of property string entirely.
                    // Skip fast path when accessors exist so getters are invoked.
                    if let Object::Hash(hash_rc) = receiver {
                        let hash = hash_rc.borrow();
                        if !hash.has_accessors() {
                            debug_assert!(cache_slot < self.inline_cache.len());
                            let (cached_shape, cached_offset) =
                                unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                            if cached_shape == hash.shape_version {
                                let slot = cached_offset as usize;
                                debug_assert!(slot < hash.values.len());
                                let value = unsafe { hash.get_value_at_slot_unchecked(slot) };
                                let out = val_to_obj(value, &self.heap);
                                unsafe { self.push_obj_unchecked(out) };
                                self.ip += 7;
                                continue;
                            }
                        }
                    }
                    // Cache miss or has accessors: fall through to full path (needs Rc::clone of prop string).
                    let prop_str = match &self.constants[const_idx] {
                        Object::String(s) => crate::intern::intern_rc(s),
                        _ => {
                            return Err(VMError::TypeError(
                                "OpGetLocalProperty constant must be a string".to_string(),
                            ));
                        }
                    };
                    let receiver = unsafe { &*self.locals.as_ptr().add(local_idx) };
                    self.get_property_fast_path_ref(receiver, prop_str, cache_slot)?;
                    self.ip += 7;
                }
                Opcode::OpSetLocalProperty => {
                    let local_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let cache_slot = self.read_u16(self.ip + 5) as usize;
                    if local_idx >= self.locals.len() {
                        return Err(VMError::InstructionOutOfBounds(self.ip + 1));
                    }
                    let value = unsafe { self.pop_obj() };
                    // Inline cache fast path: avoid Rc::clone of property string entirely.
                    // Skip fast path when accessors exist so setters are invoked.
                    let receiver = unsafe { &*self.locals.as_ptr().add(local_idx) };
                    if let Object::Hash(hash_rc) = receiver {
                        let hash = hash_rc.borrow_mut();
                        if !hash.has_accessors() {
                            debug_assert!(cache_slot < self.inline_cache.len());
                            let (cached_shape, cached_offset) =
                                unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                            if cached_shape == hash.shape_version {
                                let slot = cached_offset as usize;
                                debug_assert!(slot < hash.values.len());
                                let val = obj_to_val(&value, &mut self.heap);
                                unsafe { hash.set_value_at_slot_unchecked(slot, val) };
                                unsafe { self.push_obj_unchecked(value) };
                                self.ip += 7;
                                continue;
                            }
                        }
                    }
                    // Cache miss or has accessors: fall through to full path (needs Rc::clone of prop string).
                    let prop_str = match &self.constants[const_idx] {
                        Object::String(s) => crate::intern::intern_rc(s),
                        _ => {
                            return Err(VMError::TypeError(
                                "OpSetLocalProperty constant must be a string".to_string(),
                            ));
                        }
                    };
                    // SAFETY: bounds checked above; with Rc<VmCell<HashObject>>,
                    // mutation happens in-place so no need to store back for Hash.
                    let receiver = unsafe { &*self.locals.as_ptr().add(local_idx) };
                    let (updated, result) =
                        self.set_property_fast_path(receiver, prop_str, value, cache_slot)?;
                    if let Some(obj) = updated {
                        self.locals[local_idx] = obj;
                    }
                    unsafe { self.push_obj_unchecked(result) };
                    self.ip += 7;
                }
                Opcode::OpSetGlobalProperty => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let cache_slot = self.read_u16(self.ip + 5) as usize;
                    let value = unsafe { self.pop_obj() };
                    // SAFETY: compiler guarantees valid global indices < GLOBALS_SIZE.
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let receiver = val_to_obj(gval, &self.heap);
                    // Inline cache fast path: avoid Rc::clone of property string entirely.
                    // Skip fast path when accessors exist so setters are invoked.
                    if let Object::Hash(hash_rc) = &receiver {
                        let hash = hash_rc.borrow_mut();
                        if !hash.has_accessors() {
                            debug_assert!(cache_slot < self.inline_cache.len());
                            let (cached_shape, cached_offset) =
                                unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                            if cached_shape == hash.shape_version {
                                let slot = cached_offset as usize;
                                debug_assert!(slot < hash.values.len());
                                let val = obj_to_val(&value, &mut self.heap);
                                unsafe { hash.set_value_at_slot_unchecked(slot, val) };
                                unsafe { self.push_obj_unchecked(value) };
                                self.ip += 7;
                                continue;
                            }
                        }
                    }
                    // Cache miss or has accessors: fall through to full path.
                    let prop_str = match &self.constants[const_idx] {
                        Object::String(s) => crate::intern::intern_rc(s),
                        _ => {
                            return Err(VMError::TypeError(
                                "OpSetGlobalProperty constant must be a string".to_string(),
                            ));
                        }
                    };
                    let (updated, result) =
                        self.set_property_fast_path(&receiver, prop_str, value, cache_slot)?;
                    if let Some(obj) = updated {
                        // SAFETY: global_idx already validated above.
                        let updated_val = obj_into_val(obj, &mut self.heap);
                        unsafe { self.globals.set_unchecked(global_idx, updated_val) };
                    }
                    unsafe { self.push_obj_unchecked(result) };
                    self.ip += 7;
                }
                // Fused set+pop: identical to OpSetLocalProperty but discards the result
                // (no push), saving one clone and the push+pop cycle.
                Opcode::OpSetLocalPropertyPop => {
                    let local_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let cache_slot = self.read_u16(self.ip + 5) as usize;
                    if local_idx >= self.locals.len() {
                        return Err(VMError::InstructionOutOfBounds(self.ip + 1));
                    }
                    let value = unsafe { self.pop_obj() };
                    // Inline cache fast path: avoid Rc::clone of property string entirely.
                    // Skip fast path when accessors exist so setters are invoked.
                    let receiver = unsafe { &*self.locals.as_ptr().add(local_idx) };
                    if let Object::Hash(hash_rc) = receiver {
                        let hash = hash_rc.borrow_mut();
                        if !hash.has_accessors() {
                            debug_assert!(cache_slot < self.inline_cache.len());
                            let (cached_shape, cached_offset) =
                                unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                            if cached_shape == hash.shape_version {
                                let slot = cached_offset as usize;
                                debug_assert!(slot < hash.values.len());
                                let val = obj_into_val(value, &mut self.heap);
                                unsafe { hash.set_value_at_slot_unchecked(slot, val) };
                                self.ip += 7;
                                continue;
                            }
                        }
                    }
                    // Cache miss or has accessors: fall through to full path.
                    let prop_str = match &self.constants[const_idx] {
                        Object::String(s) => crate::intern::intern_rc(s),
                        _ => {
                            return Err(VMError::TypeError(
                                "OpSetLocalPropertyPop constant must be a string".to_string(),
                            ));
                        }
                    };
                    let receiver = unsafe { &*self.locals.as_ptr().add(local_idx) };
                    let updated =
                        self.set_property_no_result(receiver, prop_str, value, cache_slot)?;
                    if let Some(obj) = updated {
                        self.locals[local_idx] = obj;
                    }
                    self.ip += 7;
                }
                Opcode::OpSetGlobalPropertyPop => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let const_idx = self.read_u16(self.ip + 3) as usize;
                    let cache_slot = self.read_u16(self.ip + 5) as usize;
                    let value = unsafe { self.pop_obj() };
                    // SAFETY: compiler guarantees valid global indices < GLOBALS_SIZE.
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let receiver = val_to_obj(gval, &self.heap);
                    // Inline cache fast path: avoid Rc::clone of property string entirely.
                    // Skip fast path when accessors exist so setters are invoked.
                    if let Object::Hash(hash_rc) = &receiver {
                        let hash = hash_rc.borrow_mut();
                        if !hash.has_accessors() {
                            debug_assert!(cache_slot < self.inline_cache.len());
                            let (cached_shape, cached_offset) =
                                unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                            if cached_shape == hash.shape_version {
                                let slot = cached_offset as usize;
                                debug_assert!(slot < hash.values.len());
                                let val = obj_into_val(value, &mut self.heap);
                                unsafe { hash.set_value_at_slot_unchecked(slot, val) };
                                self.ip += 7;
                                continue;
                            }
                        }
                    }
                    // Cache miss or has accessors: fall through to full path.
                    let prop_str = match &self.constants[const_idx] {
                        Object::String(s) => crate::intern::intern_rc(s),
                        _ => {
                            return Err(VMError::TypeError(
                                "OpSetGlobalPropertyPop constant must be a string".to_string(),
                            ));
                        }
                    };
                    let updated =
                        self.set_property_no_result(&receiver, prop_str, value, cache_slot)?;
                    if let Some(obj) = updated {
                        // SAFETY: global_idx already validated above.
                        let updated_val = obj_into_val(obj, &mut self.heap);
                        unsafe { self.globals.set_unchecked(global_idx, updated_val) };
                    }
                    self.ip += 7;
                }
                // Fused: globals[idx].prop += constants[val_const_idx] in-place.
                // Collapses OpGetGlobalProperty + OpConstant + OpAdd + OpSetGlobalPropertyPop
                // into a single dispatch with one inline cache check.
                Opcode::OpAddConstToGlobalProperty => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let prop_const_idx = self.read_u16(self.ip + 3) as usize;
                    let val_const_idx = self.read_u16(self.ip + 5) as usize;
                    let cache_slot = self.read_u16(self.ip + 7) as usize;
                    let add_val = unsafe { &*self.constants.as_ptr().add(val_const_idx) };
                    // SAFETY: compiler guarantees valid global indices < GLOBALS_SIZE.
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let receiver = val_to_obj(gval, &self.heap);
                    if let Object::Hash(hash_rc) = &receiver {
                        let hash = hash_rc.borrow_mut();
                        // Skip fast path when accessors exist so getters/setters are invoked.
                        if !hash.has_accessors() {
                            debug_assert!(cache_slot < self.inline_cache.len());
                            let (cached_shape, cached_offset) =
                                unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                            if cached_shape == hash.shape_version {
                                let slot = cached_offset as usize;
                                debug_assert!(slot < hash.values.len());
                                let current = unsafe { hash.get_value_at_slot_unchecked(slot) };
                                let current_obj = val_to_obj(current, &self.heap);
                                // Inline numeric add (Int+Int, Int+Float, Float+Int, Float+Float).
                                let new_val = match (&current_obj, add_val) {
                                    (Object::Integer(a), Object::Integer(b)) => {
                                        Object::Integer(a + b)
                                    }
                                    (Object::Float(a), Object::Float(b)) => Object::Float(*a + *b),
                                    (Object::Integer(a), Object::Float(b)) => {
                                        Object::Float(*a as f64 + *b)
                                    }
                                    (Object::Float(a), Object::Integer(b)) => {
                                        Object::Float(*a + *b as f64)
                                    }
                                    _ => {
                                        // Non-numeric: fall through to slow path.
                                        let result = self.add_objects(&current_obj, add_val)?;
                                        let result_val = obj_into_val(result, &mut self.heap);
                                        // Re-read receiver from globals (receiver was moved)
                                        let gval2 =
                                            unsafe { self.globals.get_unchecked(global_idx) };
                                        let receiver2 = val_to_obj(gval2, &self.heap);
                                        if let Object::Hash(hash_rc2) = &receiver2 {
                                            let hash2 = hash_rc2.borrow_mut();
                                            unsafe {
                                                hash2.set_value_at_slot_unchecked(slot, result_val);
                                            }
                                        }
                                        self.ip += 9;
                                        continue;
                                    }
                                };
                                let new_val_v = obj_into_val(new_val, &mut self.heap);
                                unsafe { hash.set_value_at_slot_unchecked(slot, new_val_v) };
                                self.ip += 9;
                                continue;
                            }
                        }
                    }
                    // Cache miss or has accessors: fall through to full path.
                    // Read property, add constant, write back.
                    let prop_str = match unsafe { &*self.constants.as_ptr().add(prop_const_idx) } {
                        Object::String(s) => crate::intern::intern_rc(s),
                        _ => {
                            return Err(VMError::TypeError(
                                "OpAddConstToGlobalProperty prop constant must be a string"
                                    .to_string(),
                            ));
                        }
                    };
                    // Read the property value.
                    self.get_property_fast_path_ref(&receiver, prop_str, cache_slot)?;
                    // Stack now has the property value on top.
                    let prop_val = unsafe { self.pop_obj() };
                    let result = self.add_objects(&prop_val, add_val)?;
                    // Write back.
                    let updated =
                        self.set_property_no_result(&receiver, prop_str, result, cache_slot)?;
                    if let Some(obj) = updated {
                        let updated_val = obj_into_val(obj, &mut self.heap);
                        unsafe { self.globals.set_unchecked(global_idx, updated_val) };
                    }
                    self.ip += 9;
                }
                // Fused: globals[obj].dst = globals[obj].src1 + globals[obj].src2 in-place.
                // 15-byte instruction: [op(1)] [global(2)] [s1_prop(2)] [s1_cache(2)]
                //                      [s2_prop(2)] [s2_cache(2)] [dst_prop(2)] [dst_cache(2)]
                Opcode::OpAddGlobalPropsToGlobalProp => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let s1_cache = self.read_u16(self.ip + 5) as usize;
                    let s2_cache = self.read_u16(self.ip + 9) as usize;
                    let dst_cache = self.read_u16(self.ip + 13) as usize;
                    // SAFETY: compiler guarantees valid global indices.
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let receiver = val_to_obj(gval, &self.heap);
                    if let Object::Hash(hash_rc) = &receiver {
                        let hash = hash_rc.borrow_mut();
                        // Try inline-cache fast path: all 3 cache slots hit same shape.
                        // Skip fast path when accessors exist so getters/setters are invoked.
                        if !hash.has_accessors() {
                            debug_assert!(s1_cache < self.inline_cache.len());
                            debug_assert!(s2_cache < self.inline_cache.len());
                            debug_assert!(dst_cache < self.inline_cache.len());
                            let (s1_shape, s1_slot) =
                                unsafe { *self.inline_cache.get_unchecked(s1_cache) };
                            let (s2_shape, s2_slot) =
                                unsafe { *self.inline_cache.get_unchecked(s2_cache) };
                            let (dst_shape, dst_slot) =
                                unsafe { *self.inline_cache.get_unchecked(dst_cache) };
                            let shape = hash.shape_version;
                            if s1_shape == shape && s2_shape == shape && dst_shape == shape {
                                let s1 = s1_slot as usize;
                                let s2 = s2_slot as usize;
                                let d = dst_slot as usize;
                                debug_assert!(s1 < hash.values.len());
                                debug_assert!(s2 < hash.values.len());
                                debug_assert!(d < hash.values.len());
                                let val1 = unsafe { hash.get_value_at_slot_unchecked(s1) };
                                let val2 = unsafe { hash.get_value_at_slot_unchecked(s2) };
                                let val1_obj = val_to_obj(val1, &self.heap);
                                let val2_obj = val_to_obj(val2, &self.heap);
                                let result = match (&val1_obj, &val2_obj) {
                                    (Object::Integer(a), Object::Integer(b)) => {
                                        Object::Integer(a + b)
                                    }
                                    (Object::Float(a), Object::Float(b)) => Object::Float(*a + *b),
                                    (Object::Integer(a), Object::Float(b)) => {
                                        Object::Float(*a as f64 + *b)
                                    }
                                    (Object::Float(a), Object::Integer(b)) => {
                                        Object::Float(*a + *b as f64)
                                    }
                                    _ => {
                                        // Non-numeric: fall through to slow path.
                                        let res = self.add_objects(&val1_obj, &val2_obj)?;
                                        let res_val = obj_into_val(res, &mut self.heap);
                                        // Re-read receiver from globals
                                        let gval2 =
                                            unsafe { self.globals.get_unchecked(global_idx) };
                                        let receiver2 = val_to_obj(gval2, &self.heap);
                                        if let Object::Hash(hash_rc2) = &receiver2 {
                                            let hash2 = hash_rc2.borrow_mut();
                                            unsafe {
                                                hash2.set_value_at_slot_unchecked(d, res_val);
                                            }
                                        }
                                        self.ip += 15;
                                        continue;
                                    }
                                };
                                let result_val = obj_into_val(result, &mut self.heap);
                                unsafe { hash.set_value_at_slot_unchecked(d, result_val) };
                                self.ip += 15;
                                continue;
                            }
                        }
                    }
                    // Cache miss or has accessors: fall through to full path using stack operations.
                    let s1_prop_const = self.read_u16(self.ip + 3) as usize;
                    let s2_prop_const = self.read_u16(self.ip + 7) as usize;
                    let dst_prop_const = self.read_u16(self.ip + 11) as usize;
                    let get_prop_sym = |idx: usize| -> Result<u32, VMError> {
                        match unsafe { &*self.constants.as_ptr().add(idx) } {
                            Object::String(s) => Ok(crate::intern::intern_rc(s)),
                            _ => Err(VMError::TypeError(
                                "OpAddGlobalPropsToGlobalProp prop constant must be a string"
                                    .to_string(),
                            )),
                        }
                    };
                    let s1_sym = get_prop_sym(s1_prop_const)?;
                    let s2_sym = get_prop_sym(s2_prop_const)?;
                    let dst_sym = get_prop_sym(dst_prop_const)?;
                    // Read src1.
                    self.get_property_fast_path_ref(&receiver, s1_sym, s1_cache)?;
                    let v1 = unsafe { self.pop_obj() };
                    // Read src2.
                    self.get_property_fast_path_ref(&receiver, s2_sym, s2_cache)?;
                    let v2 = unsafe { self.pop_obj() };
                    // Add.
                    let result = self.add_objects(&v1, &v2)?;
                    // Write dst.
                    let updated =
                        self.set_property_no_result(&receiver, dst_sym, result, dst_cache)?;
                    if let Some(obj) = updated {
                        let updated_val = obj_into_val(obj, &mut self.heap);
                        unsafe { self.globals.set_unchecked(global_idx, updated_val) };
                    }
                    self.ip += 15;
                }
                Opcode::OpArray => {
                    let count = self.read_u16(self.ip + 1) as usize;
                    if count > self.sp {
                        return Err(VMError::StackUnderflow);
                    }
                    let start = self.sp - count;
                    let items: Vec<Value> = self.stack.drain(start..).collect();
                    self.sp -= count;
                    if items.len() > MAX_ARRAY_SIZE {
                        return Err(VMError::TypeError("Array size limit exceeded".to_string()));
                    }
                    unsafe { self.push_obj_unchecked(make_array(items)) };
                    self.ip += 3;
                }
                Opcode::OpHash => {
                    let count = self.read_u16(self.ip + 1) as usize;
                    if count > self.sp || count % 2 != 0 {
                        return Err(VMError::StackUnderflow);
                    }
                    let start = self.sp - count;
                    let key_values: Vec<Object> = self
                        .stack
                        .split_off(start)
                        .into_iter()
                        .map(|v| val_to_obj(v, &self.heap))
                        .collect();
                    self.sp -= count;

                    let mut hash = crate::object::HashObject::with_capacity(count / 2);
                    let mut iter = key_values.into_iter();
                    while let Some(key) = iter.next() {
                        let Some(value) = iter.next() else {
                            break;
                        };

                        match key {
                            Object::String(s) if &*s == "__fl_rest__" => {
                                if let Object::Hash(spread_hash) = value {
                                    let spread = spread_hash.borrow_mut();
                                    spread.sync_pairs_if_dirty();
                                    for (k, v) in spread.pairs.iter() {
                                        hash.insert_pair(k.clone(), *v);
                                    }
                                    continue;
                                }
                                let val = obj_into_val(value, &mut self.heap);
                                hash.insert_pair(HashKey::Sym(crate::intern::intern_rc(&s)), val);
                            }
                            other_key => {
                                let val = obj_into_val(value, &mut self.heap);
                                hash.insert_pair(self.hash_key_from_object(&other_key), val);
                            }
                        }
                    }
                    unsafe { self.push_obj_unchecked(make_hash(hash)) };
                    self.ip += 3;
                }
                Opcode::OpGetKeysIterator => {
                    let source = unsafe { self.pop_obj() };
                    let keys = self.get_keys_array(source);
                    unsafe { self.push_obj_unchecked(make_array(keys)) };
                    self.ip += 1;
                }
                Opcode::OpIteratorRest => {
                    let start = unsafe { self.pop_obj() };
                    let source = unsafe { self.pop_obj() };
                    let start_idx = match start {
                        Object::Integer(v) => v.max(0) as usize,
                        Object::Float(v) => {
                            if v.is_finite() {
                                (v as i64).max(0) as usize
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    };

                    let items: Vec<Value> = match source {
                        Object::Array(arr) => {
                            let borrowed = arr.borrow();
                            if start_idx >= borrowed.len() {
                                vec![]
                            } else {
                                borrowed[start_idx..].to_vec()
                            }
                        }
                        Object::String(s) => {
                            let chars: Vec<char> = s.chars().collect();
                            if start_idx >= chars.len() {
                                vec![]
                            } else {
                                chars[start_idx..]
                                    .iter()
                                    .map(|c| {
                                        obj_into_val(
                                            Object::String(c.to_string().into()),
                                            &mut self.heap,
                                        )
                                    })
                                    .collect()
                            }
                        }
                        _ => vec![],
                    };

                    unsafe { self.push_obj_unchecked(make_array(items)) };
                    self.ip += 1;
                }
                Opcode::OpObjectRest => {
                    let count = self.read_u16(self.ip + 1) as usize;
                    let mut excluded = {
                        let mut s = rustc_hash::FxHashSet::default();
                        s.reserve(count);
                        s
                    };
                    for _ in 0..count {
                        let key_obj = unsafe { self.pop_obj() };
                        excluded.insert(self.hash_key_from_object(&key_obj));
                    }
                    let source = unsafe { self.pop_obj() };

                    let mut out = crate::object::HashObject::default();
                    if let Object::Hash(h) = source {
                        let h = h.borrow_mut();
                        h.sync_pairs_if_dirty();
                        for k in h.ordered_keys_ref() {
                            let v = h.pairs.get(&k).expect("hash key_order out of sync").clone();
                            if !excluded.contains(&k) {
                                out.insert_pair(k.clone(), v);
                            }
                        }
                    }

                    unsafe { self.push_obj_unchecked(make_hash(out)) };
                    self.ip += 3;
                }
                Opcode::OpGetProperty => {
                    let const_idx = self.read_u16(self.ip + 1) as usize;
                    let cache_slot = self.read_u16(self.ip + 3) as usize;
                    let receiver = unsafe { self.pop_obj() };
                    let prop_str = match &self.constants[const_idx] {
                        Object::String(s) => crate::intern::intern_rc(s),
                        _ => {
                            return Err(VMError::TypeError(
                                "OpGetProperty constant must be a string".to_string(),
                            ));
                        }
                    };
                    self.get_property_fast_path(receiver, prop_str, cache_slot)?;
                    self.ip += 5; // opcode(1) + const_idx(2) + cache_slot(2)
                }
                Opcode::OpIndex => {
                    let index = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    self.execute_index_expression(left, index)?;
                    self.ip += 1;
                }
                Opcode::OpSetIndex => {
                    let value = unsafe { self.pop_obj() };
                    let index = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    self.execute_set_index(left, index, value)?;
                    self.ip += 1;
                }
                Opcode::OpAppendElement => {
                    let value = unsafe { self.pop_obj() };
                    let target = unsafe { self.pop_obj() };
                    match target {
                        Object::Array(arr) => {
                            let borrowed = arr.borrow_mut();
                            borrowed.push(obj_into_val(value, &mut self.heap));
                            if borrowed.len() > MAX_ARRAY_SIZE {
                                return Err(VMError::TypeError(
                                    "Array size limit exceeded".to_string(),
                                ));
                            }
                            unsafe { self.push_obj_unchecked(Object::Array(arr)) };
                        }
                        other => {
                            return Err(VMError::TypeError(format!(
                                "append target must be array, got {:?}",
                                other.object_type()
                            )))
                        }
                    }
                    self.ip += 1;
                }
                Opcode::OpAppendSpread => {
                    let spread_value = unsafe { self.pop_obj() };
                    let target = unsafe { self.pop_obj() };
                    match target {
                        Object::Array(arr) => {
                            {
                                let borrowed = arr.borrow_mut();
                                match spread_value {
                                    Object::Array(items) => {
                                        borrowed.extend(items.borrow().iter().copied())
                                    }
                                    Object::String(s) => {
                                        for ch in s.chars() {
                                            borrowed.push(obj_into_val(
                                                Object::String(ch.to_string().into()),
                                                &mut self.heap,
                                            ));
                                        }
                                    }
                                    Object::Set(set_obj) => {
                                        for key in set_obj.entries.borrow().iter() {
                                            borrowed.push(obj_into_val(
                                                self.object_from_hash_key(key),
                                                &mut self.heap,
                                            ));
                                        }
                                    }
                                    Object::Map(map_obj) => {
                                        for (k, v) in map_obj.entries.borrow().iter() {
                                            let key_val = obj_into_val(
                                                self.object_from_hash_key(k),
                                                &mut self.heap,
                                            );
                                            let entry = make_array(vec![key_val, *v]);
                                            borrowed.push(obj_into_val(entry, &mut self.heap));
                                        }
                                    }
                                    other => {
                                        return Err(VMError::TypeError(format!(
                                            "spread value is not iterable: {:?}",
                                            other.object_type()
                                        )))
                                    }
                                }
                                if borrowed.len() > MAX_ARRAY_SIZE {
                                    return Err(VMError::TypeError(
                                        "Array size limit exceeded".to_string(),
                                    ));
                                }
                            }
                            unsafe { self.push_obj_unchecked(Object::Array(arr)) };
                        }
                        other => {
                            return Err(VMError::TypeError(format!(
                                "append target must be array, got {:?}",
                                other.object_type()
                            )))
                        }
                    }
                    self.ip += 1;
                }
                Opcode::OpCall => {
                    debug_assert!(self.ip + 1 < self.instructions.len());
                    let num_args = unsafe { *self.inst_ptr.add(self.ip + 1) as usize };
                    let callee_val = self.stage_call_args(num_args)?;
                    let callee = val_to_obj(callee_val, &self.heap);
                    match callee {
                        Object::CompiledFunction(func) if !func.is_async && !func.is_generator => {
                            self.push_call_frame(*func, None)?;
                            continue;
                        }
                        Object::BoundMethod(bound)
                            if !bound.function.is_async && !bound.function.is_generator =>
                        {
                            self.push_call_frame(bound.function, Some(*bound.receiver))?;
                            continue;
                        }
                        _ => {
                            // Async functions, builtins, SuperRef — use the old recursive path.
                            let is_super_call = matches!(callee, Object::SuperRef(_));
                            let mut args = std::mem::take(&mut self.arg_buffer);
                            let out = self.call_value_slice(callee_val, &args);
                            args.clear();
                            self.arg_buffer = args;
                            let result = out?;
                            // When super() is called, the result is the updated
                            // `this` — write it back to locals[0] so the derived
                            // constructor sees properties set by the parent.
                            if is_super_call {
                                if let Some(slot) = self.locals.get_mut(0) {
                                    *slot = val_to_obj(result, &self.heap);
                                }
                            }
                            self.push_val(result)?;
                            self.ip += 2;
                        }
                    }
                }
                Opcode::OpCallGlobal => {
                    let global_idx = self.read_u16(self.ip + 1) as usize;
                    let num_args = self.read_u16(self.ip + 3) as usize;
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let callee = val_to_obj(gval, &self.heap);
                    match &callee {
                        Object::CompiledFunction(func)
                            if !func.is_async
                                && !func.is_generator
                                && !func.takes_this
                                && func.rest_parameter_index.is_none() =>
                        {
                            self.ip += 3;
                            self.push_call_frame_direct(func.as_ref(), num_args)?;
                            continue;
                        }
                        Object::CompiledFunction(func) if !func.is_async && !func.is_generator => {
                            self.stage_call_args_no_callee(num_args);
                            self.ip += 3;
                            self.push_call_frame(func.as_ref().clone(), None)?;
                            continue;
                        }
                        Object::BoundMethod(bound)
                            if !bound.function.is_async && !bound.function.is_generator =>
                        {
                            self.stage_call_args_no_callee(num_args);
                            let func_clone = bound.function.clone();
                            let receiver = Self::clone_object_fast(&bound.receiver);
                            self.ip += 3;
                            self.push_call_frame(func_clone, Some(receiver))?;
                            continue;
                        }
                        _ => {
                            // Fallback: use old recursive path.
                            self.stage_call_args_no_callee(num_args);
                            let mut args = std::mem::take(&mut self.arg_buffer);
                            let out = self.call_value_slice(gval, &args);
                            args.clear();
                            self.arg_buffer = args;
                            self.push_val(out?)?;
                            self.ip += 5;
                        }
                    }
                }
                Opcode::OpCallSpread => {
                    let spread_args_val = unsafe { self.pop_unchecked() };
                    let callee_val = unsafe { self.pop_unchecked() };
                    let spread_args = val_to_obj(spread_args_val, &self.heap);
                    let vals = match spread_args {
                        Object::Array(items) => match Rc::try_unwrap(items) {
                            Ok(cell) => cell.into_inner(),
                            Err(rc) => rc.borrow().clone(),
                        },
                        other => {
                            return Err(VMError::TypeError(format!(
                                "spread call arguments must be array, got {:?}",
                                other.object_type()
                            )))
                        }
                    };
                    self.execute_call_with_args(callee_val, vals)?;
                    self.ip += 1;
                }
                Opcode::OpNew => {
                    debug_assert!(self.ip + 1 < self.instructions.len());
                    let num_args = unsafe { *self.inst_ptr.add(self.ip + 1) as usize };
                    self.execute_new(num_args)?;
                    self.ip += 2;
                }
                Opcode::OpNewSpread => {
                    let spread_args_val = unsafe { self.pop_unchecked() };
                    let callee_val = unsafe { self.pop_unchecked() };
                    let spread_args = val_to_obj(spread_args_val, &self.heap);
                    let callee = val_to_obj(callee_val, &self.heap);
                    let vals = match spread_args {
                        Object::Array(items) => match Rc::try_unwrap(items) {
                            Ok(cell) => cell.into_inner(),
                            Err(rc) => rc.borrow().clone(),
                        },
                        other => {
                            return Err(VMError::TypeError(format!(
                                "spread constructor arguments must be array, got {:?}",
                                other.object_type()
                            )))
                        }
                    };
                    self.execute_new_with_args(callee, vals)?;
                    self.ip += 1;
                }
                Opcode::OpAwait => {
                    let value = unsafe { self.pop_obj() };
                    match value {
                        Object::Promise(p) => match p.settled {
                            PromiseState::Fulfilled(v) => unsafe { self.push_obj_unchecked(*v) },
                            PromiseState::Rejected(v) => {
                                return Err(VMError::TypeError(format!(
                                    "Await rejected: {}",
                                    v.inspect()
                                )))
                            }
                        },
                        other => unsafe { self.push_obj_unchecked(other) },
                    }
                    self.ip += 1;
                }
                Opcode::OpSuper => {
                    if let Some(Object::Instance(instance)) = self.locals.get(0) {
                        unsafe {
                            self.push_obj_unchecked(Object::SuperRef(Box::new(SuperRefObject {
                                receiver: Box::new(Object::Instance(Box::new(
                                    (**instance).clone(),
                                ))),
                                methods: instance.super_methods.clone(),
                                getters: instance.super_getters.clone(),
                                setters: instance.super_setters.clone(),
                                constructor_chain: instance.super_constructor_chain.clone(),
                            })))
                        };
                    } else {
                        unsafe { self.push_unchecked(Value::UNDEFINED) };
                    }
                    self.ip += 1;
                }
                Opcode::OpTrue => {
                    unsafe { self.push_unchecked(Value::TRUE) };
                    self.ip += 1;
                }
                Opcode::OpFalse => {
                    unsafe { self.push_unchecked(Value::FALSE) };
                    self.ip += 1;
                }
                Opcode::OpNull => {
                    unsafe { self.push_unchecked(Value::NULL) };
                    self.ip += 1;
                }
                Opcode::OpUndefined => {
                    unsafe { self.push_unchecked(Value::UNDEFINED) };
                    self.ip += 1;
                }
                Opcode::OpMinus => {
                    let value = unsafe { self.pop_unchecked() };
                    if value.is_i32() {
                        let v = unsafe { value.as_i32_unchecked() };
                        if v == 0 {
                            unsafe { self.push_unchecked(Value::from_f64(-0.0)) }
                        } else {
                            unsafe { self.push_unchecked(Value::from_i32(-v)) }
                        }
                    } else if value.is_f64() {
                        unsafe { self.push_unchecked(Value::from_f64(-value.as_f64())) }
                    } else {
                        let obj = val_to_obj(value, &self.heap);
                        return Err(VMError::TypeError(format!(
                            "Unsupported type for unary minus: {:?}",
                            obj.object_type()
                        )));
                    }
                    self.ip += 1;
                }
                Opcode::OpUnaryPlus => {
                    let value = unsafe { self.pop_unchecked() };
                    if value.is_i32() || value.is_f64() {
                        unsafe { self.push_unchecked(value) };
                    } else {
                        let obj = val_to_obj(value, &self.heap);
                        let n = self.to_number(&obj)?;
                        if n == 0.0 && n.is_sign_negative() {
                            unsafe { self.push_unchecked(Value::from_f64(-0.0)) };
                        } else if n.is_finite() && n.fract() == 0.0 {
                            unsafe { self.push_unchecked(Value::from_i64(n as i64)) };
                        } else {
                            unsafe { self.push_unchecked(Value::from_f64(n)) };
                        }
                    }
                    self.ip += 1;
                }
                Opcode::OpBang => {
                    let value = unsafe { self.pop_unchecked() };
                    let truthy = value.is_truthy_full(&self.heap);
                    unsafe { self.push_unchecked(Value::from_bool(!truthy)) };
                    self.ip += 1;
                }
                Opcode::OpEqual => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    let result = if left.is_i32() && right.is_i32() {
                        unsafe { left.as_i32_unchecked() == right.as_i32_unchecked() }
                    } else if left.is_bool() && right.is_bool() {
                        unsafe { left.as_bool_unchecked() == right.as_bool_unchecked() }
                    } else if left.is_number() && right.is_number() {
                        left.to_number() == right.to_number()
                    } else {
                        // Coerce Instance objects via valueOf for loose equality
                        let left = self.coerce_instance_for_add(left).unwrap_or(left);
                        let right = self.coerce_instance_for_add(right).unwrap_or(right);
                        let lo = val_to_obj(left, &self.heap);
                        let ro = val_to_obj(right, &self.heap);
                        self.equals(&lo, &ro)
                    };
                    unsafe { self.push_unchecked(Value::from_bool(result)) };
                    self.ip += 1;
                }
                Opcode::OpStrictEqual => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    let result = if left.is_i32() && right.is_i32() {
                        unsafe { left.as_i32_unchecked() == right.as_i32_unchecked() }
                    } else if left.is_bool() && right.is_bool() {
                        unsafe { left.as_bool_unchecked() == right.as_bool_unchecked() }
                    } else if left.is_f64() && right.is_f64() {
                        left.as_f64() == right.as_f64()
                    } else {
                        let lo = val_to_obj(left, &self.heap);
                        let ro = val_to_obj(right, &self.heap);
                        self.strict_equals(&lo, &ro)
                    };
                    unsafe { self.push_unchecked(Value::from_bool(result)) };
                    self.ip += 1;
                }
                Opcode::OpNotEqual => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    let result = if left.is_i32() && right.is_i32() {
                        unsafe { left.as_i32_unchecked() != right.as_i32_unchecked() }
                    } else if left.is_bool() && right.is_bool() {
                        unsafe { left.as_bool_unchecked() != right.as_bool_unchecked() }
                    } else if left.is_number() && right.is_number() {
                        left.to_number() != right.to_number()
                    } else {
                        let left = self.coerce_instance_for_add(left).unwrap_or(left);
                        let right = self.coerce_instance_for_add(right).unwrap_or(right);
                        let lo = val_to_obj(left, &self.heap);
                        let ro = val_to_obj(right, &self.heap);
                        !self.equals(&lo, &ro)
                    };
                    unsafe { self.push_unchecked(Value::from_bool(result)) };
                    self.ip += 1;
                }
                Opcode::OpStrictNotEqual => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    let result = if left.is_i32() && right.is_i32() {
                        unsafe { left.as_i32_unchecked() != right.as_i32_unchecked() }
                    } else if left.is_bool() && right.is_bool() {
                        unsafe { left.as_bool_unchecked() != right.as_bool_unchecked() }
                    } else if left.is_f64() && right.is_f64() {
                        left.as_f64() != right.as_f64()
                    } else {
                        let lo = val_to_obj(left, &self.heap);
                        let ro = val_to_obj(right, &self.heap);
                        !self.strict_equals(&lo, &ro)
                    };
                    unsafe { self.push_unchecked(Value::from_bool(result)) };
                    self.ip += 1;
                }
                Opcode::OpGreaterThan => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    let result = if left.is_i32() && right.is_i32() {
                        unsafe { left.as_i32_unchecked() > right.as_i32_unchecked() }
                    } else if left.is_number() && right.is_number() {
                        left.to_number() > right.to_number()
                    } else {
                        let left = self.coerce_instance_for_add(left)?;
                        let right = self.coerce_instance_for_add(right)?;
                        let lo = val_to_obj(left, &self.heap);
                        let ro = val_to_obj(right, &self.heap);
                        self.compare_numeric(&lo, &ro, Opcode::OpGreaterThan)?
                    };
                    unsafe { self.push_unchecked(Value::from_bool(result)) };
                    self.ip += 1;
                }
                Opcode::OpLessThan => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    let result = if left.is_i32() && right.is_i32() {
                        unsafe { left.as_i32_unchecked() < right.as_i32_unchecked() }
                    } else if left.is_number() && right.is_number() {
                        left.to_number() < right.to_number()
                    } else {
                        let left = self.coerce_instance_for_add(left)?;
                        let right = self.coerce_instance_for_add(right)?;
                        let lo = val_to_obj(left, &self.heap);
                        let ro = val_to_obj(right, &self.heap);
                        self.compare_numeric(&lo, &ro, Opcode::OpLessThan)?
                    };
                    unsafe { self.push_unchecked(Value::from_bool(result)) };
                    self.ip += 1;
                }
                Opcode::OpLessOrEqual => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    let result = if left.is_i32() && right.is_i32() {
                        unsafe { left.as_i32_unchecked() <= right.as_i32_unchecked() }
                    } else if left.is_number() && right.is_number() {
                        left.to_number() <= right.to_number()
                    } else {
                        let left = self.coerce_instance_for_add(left)?;
                        let right = self.coerce_instance_for_add(right)?;
                        let lo = val_to_obj(left, &self.heap);
                        let ro = val_to_obj(right, &self.heap);
                        self.compare_numeric(&lo, &ro, Opcode::OpLessOrEqual)?
                    };
                    unsafe { self.push_unchecked(Value::from_bool(result)) };
                    self.ip += 1;
                }
                Opcode::OpGreaterOrEqual => {
                    let right = unsafe { self.pop_unchecked() };
                    let left = unsafe { self.pop_unchecked() };
                    let result = if left.is_i32() && right.is_i32() {
                        unsafe { left.as_i32_unchecked() >= right.as_i32_unchecked() }
                    } else if left.is_number() && right.is_number() {
                        left.to_number() >= right.to_number()
                    } else {
                        let left = self.coerce_instance_for_add(left)?;
                        let right = self.coerce_instance_for_add(right)?;
                        let lo = val_to_obj(left, &self.heap);
                        let ro = val_to_obj(right, &self.heap);
                        self.compare_numeric(&lo, &ro, Opcode::OpGreaterOrEqual)?
                    };
                    unsafe { self.push_unchecked(Value::from_bool(result)) };
                    self.ip += 1;
                }
                Opcode::OpIn => {
                    let right = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    unsafe { self.push_unchecked(Value::from_bool(self.op_in(&left, &right))) };
                    self.ip += 1;
                }
                Opcode::OpInstanceof => {
                    let right = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    unsafe {
                        self.push_unchecked(Value::from_bool(self.op_instanceof(&left, &right)))
                    };
                    self.ip += 1;
                }
                Opcode::OpIsNullish => {
                    let value = unsafe { self.pop_unchecked() };
                    let is_nullish = value.is_null() || value.is_undefined();
                    unsafe { self.push_unchecked(Value::from_bool(is_nullish)) };
                    self.ip += 1;
                }
                Opcode::OpBitwiseAnd => {
                    let right = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    unsafe {
                        self.push_unchecked(Value::from_i32(
                            self.to_i32(&left)? & self.to_i32(&right)?,
                        ))
                    };
                    self.ip += 1;
                }
                Opcode::OpBitwiseOr => {
                    let right = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    unsafe {
                        self.push_unchecked(Value::from_i32(
                            self.to_i32(&left)? | self.to_i32(&right)?,
                        ))
                    };
                    self.ip += 1;
                }
                Opcode::OpBitwiseXor => {
                    let right = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    unsafe {
                        self.push_unchecked(Value::from_i32(
                            self.to_i32(&left)? ^ self.to_i32(&right)?,
                        ))
                    };
                    self.ip += 1;
                }
                Opcode::OpLeftShift => {
                    let right = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    let r = (self.to_u32(&right)? & 0x1f) as i32;
                    unsafe { self.push_unchecked(Value::from_i32(self.to_i32(&left)? << r)) };
                    self.ip += 1;
                }
                Opcode::OpRightShift => {
                    let right = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    let r = (self.to_u32(&right)? & 0x1f) as i32;
                    unsafe { self.push_unchecked(Value::from_i32(self.to_i32(&left)? >> r)) };
                    self.ip += 1;
                }
                Opcode::OpUnsignedRightShift => {
                    let right = unsafe { self.pop_obj() };
                    let left = unsafe { self.pop_obj() };
                    let r = self.to_u32(&right)? & 0x1f;
                    let l = self.to_u32(&left)?;
                    // Must use i64 to preserve unsigned u32 results (e.g. -1 >>> 0 = 4294967295)
                    unsafe { self.push_unchecked(Value::from_i64((l >> r) as i64)) };
                    self.ip += 1;
                }
                Opcode::OpReturnValue => {
                    let value = unsafe { self.pop_unchecked() };
                    if self.frames.len() > entry_depth {
                        let was_async = self.restore_caller_frame();
                        if was_async {
                            let obj_val = val_to_obj(value, &self.heap);
                            let promise = PromiseObject {
                                settled: PromiseState::Fulfilled(Box::new(obj_val)),
                            };
                            unsafe { self.push_obj_unchecked(Object::Promise(Box::new(promise))) };
                        } else {
                            unsafe { self.push_unchecked(value) };
                        }
                        continue;
                    }
                    self.last_popped = Some(value);
                    break;
                }
                Opcode::OpReturn => {
                    if self.frames.len() > entry_depth {
                        let was_async = self.restore_caller_frame();
                        if was_async {
                            let promise = PromiseObject {
                                settled: PromiseState::Fulfilled(Box::new(undefined_object())),
                            };
                            unsafe { self.push_obj_unchecked(Object::Promise(Box::new(promise))) };
                        } else {
                            unsafe { self.push_unchecked(Value::UNDEFINED) };
                        }
                        continue;
                    }
                    self.last_popped = Some(Value::UNDEFINED);
                    break;
                }
                Opcode::OpDefineAccessor => {
                    // Operands: [prop_const_idx: u16, kind: u16]
                    // Stack: ..., hash, function => ..., hash
                    // kind: 0 = getter, 1 = setter
                    let prop_idx = self.read_u16(self.ip + 1) as usize;
                    let kind = self.read_u16(self.ip + 3);
                    let func_val = unsafe { self.pop_unchecked() };
                    // hash is still on the stack (peek)
                    let hash_val = unsafe { *self.stack.get_unchecked(self.sp - 1) };
                    let prop_name = match &self.constants[prop_idx] {
                        Object::String(s) => s.to_string(),
                        _ => {
                            return Err(VMError::TypeError(
                                "DefineAccessor: expected string constant for property name"
                                    .to_string(),
                            ))
                        }
                    };
                    let func_obj = val_to_obj(func_val, &self.heap);
                    let compiled_fn = match func_obj {
                        Object::CompiledFunction(f) => (*f).clone(),
                        _ => {
                            return Err(VMError::TypeError(
                                "DefineAccessor: expected function".to_string(),
                            ))
                        }
                    };
                    // Mutate the hash object on the stack
                    let hash_obj = val_to_obj(hash_val, &self.heap);
                    match hash_obj {
                        Object::Hash(h) => {
                            let ho = h.borrow_mut();
                            if kind == 0 {
                                ho.define_getter(prop_name, compiled_fn);
                            } else {
                                ho.define_setter(prop_name, compiled_fn);
                            }
                        }
                        _ => {
                            return Err(VMError::TypeError(
                                "DefineAccessor: expected hash object".to_string(),
                            ))
                        }
                    }
                    self.ip += 5;
                }
                Opcode::OpInitClass => {
                    // Stack: [class] → [class] (with static_fields populated)
                    // No operands.
                    let class_val = unsafe { self.pop_unchecked() };
                    let class_obj = val_to_obj(class_val, &self.heap);
                    match class_obj {
                        Object::Class(mut class_box) => {
                            let inits = std::mem::take(&mut class_box.static_initializers);
                            for init in &inits {
                                match init {
                                    crate::object::StaticInitializer::Field { name, thunk } => {
                                        let receiver_val = obj_into_val(
                                            Object::Class(class_box.clone()),
                                            &mut self.heap,
                                        );
                                        let (result, _) = self.execute_compiled_function_slice(
                                            thunk.clone(),
                                            &[],
                                            Some(receiver_val),
                                        )?;
                                        let result_obj = val_to_obj(result, &self.heap);
                                        class_box.static_fields.insert(name.clone(), result_obj);
                                    }
                                    crate::object::StaticInitializer::Block { thunk } => {
                                        let receiver_val = obj_into_val(
                                            Object::Class(class_box.clone()),
                                            &mut self.heap,
                                        );
                                        self.execute_compiled_function_slice(
                                            thunk.clone(),
                                            &[],
                                            Some(receiver_val),
                                        )?;
                                    }
                                }
                            }
                            class_box.static_initializers = inits;
                            self.push(Object::Class(class_box))?;
                        }
                        other => {
                            return Err(VMError::TypeError(format!(
                                "OpInitClass: expected class, got {:?}",
                                other.object_type()
                            )));
                        }
                    }
                    self.ip += 1;
                }
                Opcode::OpNewTarget => {
                    // Push the current new.target value onto the stack.
                    self.push_val(self.new_target)?;
                    self.ip += 1;
                }
                Opcode::OpImportMeta => {
                    // Push an empty object as import.meta stub.
                    self.push(Object::Hash(Rc::new(crate::object::VmCell::new(
                        crate::object::HashObject::with_capacity(0),
                    ))))?;
                    self.ip += 1;
                }
                Opcode::OpYield => {
                    // Pop the yielded value and signal suspension via error sentinel.
                    // The generator .next() catches Yield, saves state, and returns
                    // the iterator result.  ip is advanced past OpYield (1 byte) so
                    // that resumption starts at the next instruction.
                    let yielded = unsafe { self.pop_unchecked() };
                    self.ip += 1; // advance past OpYield
                    return Err(VMError::Yield(yielded));
                }
                Opcode::OpHalt => {
                    break;
                }
                Opcode::OpThrow => {
                    let thrown = unsafe { self.pop_unchecked() };
                    return Err(VMError::TypeError(format!(
                        "Uncaught throw: {}",
                        val_inspect(thrown, &self.heap)
                    )));
                }
                Opcode::OpTypeof => {
                    let value = unsafe { self.pop_unchecked() };
                    let s = if value.is_undefined() {
                        TYPEOF_UNDEFINED.with(|s| Rc::clone(s))
                    } else if value.is_null() {
                        TYPEOF_OBJECT.with(|s| Rc::clone(s))
                    } else if value.is_bool() {
                        TYPEOF_BOOLEAN.with(|s| Rc::clone(s))
                    } else if value.is_i32() || value.is_f64() {
                        TYPEOF_NUMBER.with(|s| Rc::clone(s))
                    } else if value.is_heap() {
                        let obj = val_to_obj(value, &self.heap);
                        match obj {
                            Object::String(_) => TYPEOF_STRING.with(|s| Rc::clone(s)),
                            Object::CompiledFunction(_) | Object::BoundMethod(_) => {
                                TYPEOF_FUNCTION.with(|s| Rc::clone(s))
                            }
                            _ => TYPEOF_OBJECT.with(|s| Rc::clone(s)),
                        }
                    } else {
                        TYPEOF_OBJECT.with(|s| Rc::clone(s))
                    };
                    unsafe { self.push_obj_unchecked(Object::String(s)) };
                    self.ip += 1;
                }
                Opcode::OpDeleteProperty => {
                    let key = unsafe { self.pop_obj() };
                    let target = unsafe { self.pop_obj() };
                    self.execute_delete_property(target, key)?;
                    self.ip += 1;
                }
            }
        }

        Ok(())
    }

    #[inline(always)]
    fn push_property_value(&mut self, receiver: &Object, value: &Object) -> Result<(), VMError> {
        let out = match value {
            Object::CompiledFunction(func) => {
                Object::BoundMethod(Box::new(crate::object::BoundMethodObject {
                    function: (**func).clone(),
                    receiver: Box::new(receiver.clone()),
                }))
            }
            Object::Integer(v) => Object::Integer(*v),
            Object::Float(v) => Object::Float(*v),
            Object::Boolean(v) => Object::Boolean(*v),
            Object::Null => Object::Null,
            Object::Undefined => Object::Undefined,
            other => other.clone(),
        };
        unsafe { self.push_obj_unchecked(out) };
        Ok(())
    }

    /// Value-native property get.  For HashObject receivers the value is
    /// returned directly from `hash.values` (which already stores `Value`)
    /// without any Object conversion or stack push/pop.
    #[inline(always)]
    pub(crate) fn get_property_val(
        &mut self,
        receiver_val: Value,
        prop_sym: u32,
        cache_slot: usize,
    ) -> Result<Value, VMError> {
        if receiver_val.is_heap() {
            let heap_obj = unsafe {
                &*self
                    .heap
                    .objects
                    .as_ptr()
                    .add(receiver_val.heap_index() as usize)
            };
            if let Object::Hash(hash_rc) = heap_obj {
                let hash = hash_rc.borrow();

                // Check for getter accessor before data property path.
                if hash.has_accessors() {
                    let prop_name = crate::intern::resolve(prop_sym);
                    if let Some(getter) = hash.get_getter(&prop_name) {
                        let getter_func = getter.clone();
                        let _ = hash; // end borrow before calling accessor
                        let (result, _) = self.execute_compiled_function_slice(
                            getter_func,
                            &[],
                            Some(receiver_val),
                        )?;
                        return Ok(result);
                    }
                }

                // Inline cache hit
                debug_assert!(cache_slot < self.inline_cache.len());
                let (cached_shape, cached_offset) =
                    unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                if cached_shape == hash.shape_version {
                    let slot = cached_offset as usize;
                    debug_assert!(slot < hash.values.len());
                    let val = unsafe { *hash.values.get_unchecked(slot) };
                    return self.maybe_bind_method_val(val, receiver_val);
                }
                // Cache miss: symbol lookup + cache update
                if let Some(&pair_index) = hash.str_slots.get(&prop_sym) {
                    self.inline_cache[cache_slot] = (hash.shape_version, pair_index as u32);
                    let val = unsafe { hash.get_value_at_slot_unchecked(pair_index) };
                    return self.maybe_bind_method_val(val, receiver_val);
                }
                return Ok(Value::UNDEFINED);
            }
            // Instance: read fields/methods directly from the heap.
            if matches!(heap_obj, Object::Instance(_)) {
                let heap_idx = receiver_val.heap_index() as usize;
                let prop_name = crate::intern::resolve(prop_sym);

                // Check getter first
                let getter = match &self.heap.objects[heap_idx] {
                    Object::Instance(inst) => inst.getters.get(&*prop_name).cloned(),
                    _ => None,
                };
                if let Some(getter_func) = getter {
                    let (result, _) = self.execute_compiled_function_slice(
                        getter_func,
                        &[],
                        Some(receiver_val),
                    )?;
                    return Ok(result);
                }

                // Read field or method
                return match &self.heap.objects[heap_idx] {
                    Object::Instance(inst) => {
                        if let Some(field_val) = inst.fields.get(&*prop_name) {
                            Ok(obj_into_val(field_val.clone(), &mut self.heap))
                        } else if let Some(method) = inst.methods.get(&*prop_name) {
                            let func_val = obj_into_val(
                                Object::CompiledFunction(Box::new(method.clone())),
                                &mut self.heap,
                            );
                            self.maybe_bind_method_val(func_val, receiver_val)
                        } else {
                            Ok(Value::UNDEFINED)
                        }
                    }
                    _ => Ok(Value::UNDEFINED),
                };
            }
        }
        // Non-Hash/Instance fallback: use existing stack-based path
        let obj = val_to_obj(receiver_val, &self.heap);
        let index_obj = Object::String(crate::intern::resolve(prop_sym));
        self.execute_index_expression(obj, index_obj)?;
        self.pop_val()
    }

    /// Value-native property set.  For HashObject receivers the value is
    /// written directly into `hash.values` (already `Vec<Value>`) without
    /// any Object conversion.  Returns `None` when mutated in-place (Hash),
    /// `Some(updated_receiver)` for non-Hash types needing store-back.
    #[inline(always)]
    pub(crate) fn set_property_val(
        &mut self,
        receiver_val: Value,
        prop_sym: u32,
        value: Value,
        cache_slot: usize,
    ) -> Result<Option<Value>, VMError> {
        if receiver_val.is_heap() {
            let heap_obj = unsafe {
                &*self
                    .heap
                    .objects
                    .as_ptr()
                    .add(receiver_val.heap_index() as usize)
            };
            if let Object::Hash(hash_rc) = heap_obj {
                // Check for setter accessor before data property path.
                {
                    let hash = hash_rc.borrow();
                    if hash.has_accessors() {
                        let prop_name = crate::intern::resolve(prop_sym);
                        if let Some(setter) = hash.get_setter(&prop_name) {
                            let setter_func = setter.clone();
                            let _ = hash; // end borrow before calling accessor
                            self.execute_compiled_function_slice(
                                setter_func,
                                std::slice::from_ref(&value),
                                Some(receiver_val),
                            )?;
                            return Ok(None);
                        }
                    }
                }

                let hash = hash_rc.borrow_mut();
                // Frozen check: silently ignore writes to frozen objects
                if hash.frozen {
                    return Ok(None);
                }
                // Inline cache hit
                debug_assert!(cache_slot < self.inline_cache.len());
                let (cached_shape, cached_offset) =
                    unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                if cached_shape == hash.shape_version {
                    let slot = cached_offset as usize;
                    debug_assert!(slot < hash.values.len());
                    unsafe { hash.set_value_at_slot_unchecked(slot, value) };
                    return Ok(None);
                }
                // Cache miss: full insert + cache update
                hash.set_by_sym(prop_sym, value);
                if let Some(&slot) = hash.str_slots.get(&prop_sym) {
                    self.inline_cache[cache_slot] = (hash.shape_version, slot as u32);
                }
                return Ok(None);
            }
            // Instance: modify fields directly on the heap (in-place).
            if matches!(heap_obj, Object::Instance(_)) {
                let heap_idx = receiver_val.heap_index() as usize;
                let prop_name = crate::intern::resolve(prop_sym);

                // Check setter first
                let setter = match &self.heap.objects[heap_idx] {
                    Object::Instance(inst) => inst.setters.get(&*prop_name).cloned(),
                    _ => None,
                };
                if let Some(setter_func) = setter {
                    self.execute_compiled_function_slice(
                        setter_func,
                        std::slice::from_ref(&value),
                        Some(receiver_val),
                    )?;
                    return Ok(None);
                }

                let val_obj = val_to_obj(value, &self.heap);
                if let Object::Instance(inst) = &mut self.heap.objects[heap_idx] {
                    inst.fields.insert(prop_name.to_string(), val_obj);
                }
                return Ok(None);
            }
            // Class: modify static fields directly on the heap (in-place).
            if matches!(heap_obj, Object::Class(_)) {
                let heap_idx = receiver_val.heap_index() as usize;
                let prop_name = crate::intern::resolve(prop_sym);
                let val_obj = val_to_obj(value, &self.heap);
                if let Object::Class(class_obj) = &mut self.heap.objects[heap_idx] {
                    class_obj.static_fields.insert(prop_name.to_string(), val_obj);
                }
                return Ok(None);
            }
        }
        // Non-Hash/Instance/Class fallback
        let obj = val_to_obj(receiver_val, &self.heap);
        let val_obj = val_to_obj(value, &self.heap);
        self.execute_set_index(
            obj,
            Object::String(crate::intern::resolve(prop_sym)),
            val_obj,
        )?;
        let updated = self.pop_val()?;
        Ok(Some(updated))
    }

    /// Coerce a Value to its string representation for string concatenation.
    /// For Instance objects, calls `toString()` if defined.
    /// Returns the string, or falls back to `inspect()`.
    pub(crate) fn coerce_to_string_val(&mut self, val: Value) -> Result<Rc<str>, VMError> {
        if val.is_heap() {
            let heap_idx = val.heap_index() as usize;
            let heap_obj = unsafe { &*self.heap.objects.as_ptr().add(heap_idx) };
            if let Object::Instance(inst) = heap_obj {
                if let Some(to_str_func) = inst.methods.get("toString").cloned() {
                    let (result, _) = self.execute_compiled_function_slice(
                        to_str_func,
                        &[],
                        Some(val),
                    )?;
                    let result_obj = val_to_obj(result, &self.heap);
                    return Ok(Rc::from(result_obj.inspect()));
                }
                return Ok(Rc::from(format!("[Instance {}]", inst.class_name).as_str()));
            }
        }
        Ok(Rc::from(val_to_obj(val, &self.heap).inspect().as_str()))
    }

    /// If `val` is a heap CompiledFunction, wrap it as a BoundMethod with
    /// `receiver_val` as the receiver.  For non-function values (the common
    /// case for data properties), returns `val` unchanged.
    #[inline(always)]
    pub(crate) fn maybe_bind_method_val(
        &mut self,
        val: Value,
        receiver_val: Value,
    ) -> Result<Value, VMError> {
        if val.is_heap() {
            let heap_obj = unsafe { &*self.heap.objects.as_ptr().add(val.heap_index() as usize) };
            if let Object::CompiledFunction(func) = heap_obj {
                let bound = Object::BoundMethod(Box::new(crate::object::BoundMethodObject {
                    function: (**func).clone(),
                    receiver: Box::new(val_to_obj(receiver_val, &self.heap)),
                }));
                return Ok(obj_into_val(bound, &mut self.heap));
            }
        }
        Ok(val)
    }

    #[inline(always)]
    pub(crate) fn get_property_fast_path(
        &mut self,
        receiver: Object,
        prop_sym: u32,
        cache_slot: usize,
    ) -> Result<(), VMError> {
        self.get_property_fast_path_ref(&receiver, prop_sym, cache_slot)
    }

    #[inline(always)]
    fn get_property_fast_path_ref(
        &mut self,
        receiver: &Object,
        prop_sym: u32,
        cache_slot: usize,
    ) -> Result<(), VMError> {
        match receiver {
            Object::Hash(hash_rc) => {
                let hash = hash_rc.borrow();

                // Check for getter accessor before using data property path.
                if hash.has_accessors() {
                    let prop_name = crate::intern::resolve(prop_sym);
                    if let Some(getter) = hash.get_getter(&prop_name) {
                        let getter_func = getter.clone();
                        let _ = hash; // end borrow before calling accessor
                        let receiver_val =
                            obj_into_val(Object::Hash(Rc::clone(hash_rc)), &mut self.heap);
                        let (result, _) = self.execute_compiled_function_slice(
                            getter_func,
                            &[],
                            Some(receiver_val),
                        )?;
                        self.push_val(result)?;
                        return Ok(());
                    }
                }

                debug_assert!(cache_slot < self.inline_cache.len());
                let (cached_shape, cached_offset) =
                    unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                if cached_shape == hash.shape_version {
                    let slot = cached_offset as usize;
                    debug_assert!(slot < hash.values.len());
                    let value = unsafe { hash.get_value_at_slot_unchecked(slot) };
                    let value_obj = val_to_obj(value, &self.heap);
                    self.push_property_value(receiver, &value_obj)?;
                    return Ok(());
                }

                if let Some(&pair_index) = hash.str_slots.get(&prop_sym) {
                    self.inline_cache[cache_slot] = (hash.shape_version, pair_index as u32);
                    let value = unsafe { hash.get_value_at_slot_unchecked(pair_index) };
                    let value_obj = val_to_obj(value, &self.heap);
                    self.push_property_value(receiver, &value_obj)?;
                } else {
                    self.push(Object::Undefined)?;
                }
            }
            other => {
                let index_obj = Object::String(crate::intern::resolve(prop_sym));
                self.execute_index_expression(other.clone(), index_obj)?;
            }
        }

        Ok(())
    }

    #[inline(always)]
    pub(crate) fn set_property_fast_path(
        &mut self,
        receiver: &Object,
        prop_sym: u32,
        value: Object,
        cache_slot: usize,
    ) -> Result<(Option<Object>, Object), VMError> {
        if let Object::Hash(hash_rc) = receiver {
            // Check for setter accessor before data property path.
            {
                let hash = hash_rc.borrow();
                if hash.has_accessors() {
                    let prop_name = crate::intern::resolve(prop_sym);
                    if let Some(setter) = hash.get_setter(&prop_name) {
                        let setter_func = setter.clone();
                        let _ = hash; // end borrow before calling accessor
                        let value_val = obj_into_val(value.clone(), &mut self.heap);
                        let receiver_val =
                            obj_into_val(Object::Hash(Rc::clone(hash_rc)), &mut self.heap);
                        self.execute_compiled_function_slice(
                            setter_func,
                            std::slice::from_ref(&value_val),
                            Some(receiver_val),
                        )?;
                        return Ok((None, value));
                    }
                }
            }

            let val = obj_to_val(&value, &mut self.heap);
            let hash = hash_rc.borrow_mut();

            // Fast path: inline cache hit — write directly to slot, no HashMap lookup.
            debug_assert!(cache_slot < self.inline_cache.len());
            let (cached_shape, cached_offset) =
                unsafe { *self.inline_cache.get_unchecked(cache_slot) };
            if cached_shape == hash.shape_version {
                let slot = cached_offset as usize;
                debug_assert!(slot < hash.values.len());
                unsafe { hash.set_value_at_slot_unchecked(slot, val) };
                return Ok((None, value));
            }

            // Slow path: full insert + cache update.
            hash.set_by_sym(prop_sym, val);
            if let Some(&slot) = hash.str_slots.get(&prop_sym) {
                self.inline_cache[cache_slot] = (hash.shape_version, slot as u32);
            }
            return Ok((None, value));
        }

        let rhs = value.clone();
        self.execute_set_index(
            receiver.clone(),
            Object::String(crate::intern::resolve(prop_sym)),
            value,
        )?;
        let updated = self.pop()?;
        Ok((Some(updated), rhs))
    }

    /// Like `set_property_fast_path` but discards the result value.
    /// Used by OpSetLocalPropertyPop / OpSetGlobalPropertyPop to avoid
    /// cloning the value just to push+pop it immediately.
    /// Returns `Some(updated_receiver)` for non-Hash types that need store-back,
    /// or `None` for Hash (mutated in-place).
    #[inline(always)]
    pub(crate) fn set_property_no_result(
        &mut self,
        receiver: &Object,
        prop_sym: u32,
        value: Object,
        cache_slot: usize,
    ) -> Result<Option<Object>, VMError> {
        if let Object::Hash(hash_rc) = receiver {
            // Check for setter accessor before data property path.
            {
                let hash = hash_rc.borrow();
                if hash.has_accessors() {
                    let prop_name = crate::intern::resolve(prop_sym);
                    if let Some(setter) = hash.get_setter(&prop_name) {
                        let setter_func = setter.clone();
                        let _ = hash; // end borrow before calling accessor
                        let value_val = obj_into_val(value, &mut self.heap);
                        let receiver_val =
                            obj_into_val(Object::Hash(Rc::clone(hash_rc)), &mut self.heap);
                        self.execute_compiled_function_slice(
                            setter_func,
                            std::slice::from_ref(&value_val),
                            Some(receiver_val),
                        )?;
                        return Ok(None);
                    }
                }
            }

            let val = obj_into_val(value, &mut self.heap);
            let hash = hash_rc.borrow_mut();

            // Fast path: inline cache hit — write directly to slot, no HashMap lookup.
            // No clone needed since the result is discarded.
            debug_assert!(cache_slot < self.inline_cache.len());
            let (cached_shape, cached_offset) =
                unsafe { *self.inline_cache.get_unchecked(cache_slot) };
            if cached_shape == hash.shape_version {
                let slot = cached_offset as usize;
                debug_assert!(slot < hash.values.len());
                unsafe { hash.set_value_at_slot_unchecked(slot, val) };
                return Ok(None);
            }

            // Slow path: full insert + cache update.
            hash.set_by_sym(prop_sym, val);
            if let Some(&slot) = hash.str_slots.get(&prop_sym) {
                self.inline_cache[cache_slot] = (hash.shape_version, slot as u32);
            }
            return Ok(None);
        }

        self.execute_set_index(
            receiver.clone(),
            Object::String(crate::intern::resolve(prop_sym)),
            value,
        )?;
        let updated = self.pop()?;
        Ok(Some(updated))
    }

    #[inline(always)]
    pub(crate) fn add_objects(&self, left: &Object, right: &Object) -> Result<Object, VMError> {
        match (left, right) {
            (Object::Integer(a), Object::Integer(b)) => Ok(Object::Integer(a + b)),
            (Object::Float(a), Object::Float(b)) => Ok(Object::Float(a + b)),
            (Object::Integer(a), Object::Float(b)) => Ok(Object::Float(*a as f64 + b)),
            (Object::Float(a), Object::Integer(b)) => Ok(Object::Float(a + *b as f64)),
            (Object::String(a), Object::String(b)) => {
                let mut s = String::with_capacity(a.len() + b.len());
                s.push_str(a);
                s.push_str(b);
                if s.len() > MAX_STRING_LENGTH {
                    return Err(VMError::TypeError(
                        "String length limit exceeded".to_string(),
                    ));
                }
                Ok(Object::String(s.into()))
            }
            (Object::String(a), b) => {
                let b_str: Cow<str> = match b {
                    Object::Integer(v) => {
                        let mut buf = itoa::Buffer::new();
                        Cow::Owned(buf.format(*v).to_string())
                    }
                    Object::Float(v) => {
                        let v = *v;
                        if v.is_finite() && v.fract() == 0.0 && v.abs() < i64::MAX as f64 {
                            let mut buf = itoa::Buffer::new();
                            Cow::Owned(buf.format(v as i64).to_string())
                        } else {
                            Cow::Owned(b.inspect())
                        }
                    }
                    Object::Array(items) => {
                        Cow::Owned(self.array_to_js_string(&items.borrow()))
                    }
                    Object::Hash(_) | Object::Instance(_) => {
                        Cow::Borrowed("[object Object]")
                    }
                    _ => Cow::Owned(b.to_js_string()),
                };
                let mut s = String::with_capacity(a.len() + b_str.len());
                s.push_str(a);
                s.push_str(&b_str);
                if s.len() > MAX_STRING_LENGTH {
                    return Err(VMError::TypeError(
                        "String length limit exceeded".to_string(),
                    ));
                }
                Ok(Object::String(s.into()))
            }
            (a, Object::String(b)) => {
                let a_str: Cow<str> = match a {
                    Object::Integer(v) => {
                        let mut buf = itoa::Buffer::new();
                        Cow::Owned(buf.format(*v).to_string())
                    }
                    Object::Float(v) => {
                        let v = *v;
                        if v.is_finite() && v.fract() == 0.0 && v.abs() < i64::MAX as f64 {
                            let mut buf = itoa::Buffer::new();
                            Cow::Owned(buf.format(v as i64).to_string())
                        } else {
                            Cow::Owned(a.inspect())
                        }
                    }
                    Object::Array(items) => {
                        Cow::Owned(self.array_to_js_string(&items.borrow()))
                    }
                    Object::Hash(_) | Object::Instance(_) => {
                        Cow::Borrowed("[object Object]")
                    }
                    _ => Cow::Owned(a.to_js_string()),
                };
                let mut s = String::with_capacity(a_str.len() + b.len());
                s.push_str(&a_str);
                s.push_str(b);
                if s.len() > MAX_STRING_LENGTH {
                    return Err(VMError::TypeError(
                        "String length limit exceeded".to_string(),
                    ));
                }
                Ok(Object::String(s.into()))
            }
            // JS behavior: coerce to numbers and add
            (a, b) => {
                let x = self.to_number(a)?;
                let y = self.to_number(b)?;
                Ok(Object::Float(x + y))
            }
        }
    }

    /// String concatenation using the VM's reusable scratch buffer.
    /// Only called from OpAdd where operands are owned (not borrowed from self).
    /// Avoids allocating a new String on every `+` — the buffer is cleared
    /// and reused, only the final `Rc<str>` is heap-allocated.
    #[inline(never)]
    pub(crate) fn add_objects_buffered(&mut self, left: Object, right: Object) -> Result<Object, VMError> {
        match (&left, &right) {
            (Object::String(a), Object::String(b)) => {
                self.string_concat_buf.clear();
                self.string_concat_buf.push_str(a);
                self.string_concat_buf.push_str(b);
                if self.string_concat_buf.len() > MAX_STRING_LENGTH {
                    return Err(VMError::TypeError(
                        "String length limit exceeded".to_string(),
                    ));
                }
                Ok(Object::String(Rc::from(self.string_concat_buf.as_str())))
            }
            (Object::String(a), _) => {
                self.string_concat_buf.clear();
                self.string_concat_buf.push_str(a);
                match &right {
                    Object::Integer(v) => {
                        let mut buf = itoa::Buffer::new();
                        self.string_concat_buf.push_str(buf.format(*v));
                    }
                    Object::Float(v) => {
                        let v = *v;
                        if v.is_finite() && v.fract() == 0.0 && v.abs() < i64::MAX as f64 {
                            let mut buf = itoa::Buffer::new();
                            self.string_concat_buf.push_str(buf.format(v as i64));
                        } else {
                            self.string_concat_buf.push_str(&right.inspect());
                        }
                    }
                    Object::Array(items) => {
                        self.string_concat_buf.push_str(&self.array_to_js_string(&items.borrow()));
                    }
                    Object::Hash(_) | Object::Instance(_) => {
                        self.string_concat_buf.push_str("[object Object]");
                    }
                    _ => self.string_concat_buf.push_str(&right.to_js_string()),
                };
                if self.string_concat_buf.len() > MAX_STRING_LENGTH {
                    return Err(VMError::TypeError(
                        "String length limit exceeded".to_string(),
                    ));
                }
                Ok(Object::String(Rc::from(self.string_concat_buf.as_str())))
            }
            (_, Object::String(b)) => {
                self.string_concat_buf.clear();
                match &left {
                    Object::Integer(v) => {
                        let mut buf = itoa::Buffer::new();
                        self.string_concat_buf.push_str(buf.format(*v));
                    }
                    Object::Float(v) => {
                        let v = *v;
                        if v.is_finite() && v.fract() == 0.0 && v.abs() < i64::MAX as f64 {
                            let mut buf = itoa::Buffer::new();
                            self.string_concat_buf.push_str(buf.format(v as i64));
                        } else {
                            self.string_concat_buf.push_str(&left.inspect());
                        }
                    }
                    Object::Array(items) => {
                        self.string_concat_buf.push_str(&self.array_to_js_string(&items.borrow()));
                    }
                    Object::Hash(_) | Object::Instance(_) => {
                        self.string_concat_buf.push_str("[object Object]");
                    }
                    _ => self.string_concat_buf.push_str(&left.to_js_string()),
                };
                self.string_concat_buf.push_str(b);
                if self.string_concat_buf.len() > MAX_STRING_LENGTH {
                    return Err(VMError::TypeError(
                        "String length limit exceeded".to_string(),
                    ));
                }
                Ok(Object::String(Rc::from(self.string_concat_buf.as_str())))
            }
            _ => self.add_objects(&left, &right),
        }
    }

    #[inline(always)]
    pub(crate) fn mod_objects(&self, left: &Object, right: &Object) -> Result<Object, VMError> {
        let (a, b) = match (left, right) {
            (Object::Integer(a), Object::Integer(b)) => (*a as f64, *b as f64),
            (Object::Float(a), Object::Float(b)) => (*a, *b),
            (Object::Integer(a), Object::Float(b)) => (*a as f64, *b),
            (Object::Float(a), Object::Integer(b)) => (*a, *b as f64),
            (x, y) => (self.to_number(x)?, self.to_number(y)?),
        };
        Ok(Object::Float(a % b))
    }

    #[inline(always)]
    pub(crate) fn is_truthy(&self, value: &Object) -> bool {
        match value {
            Object::Boolean(v) => *v,
            Object::Null | Object::Undefined => false,
            Object::Integer(v) => *v != 0,
            Object::Float(v) => *v != 0.0 && !v.is_nan(),
            Object::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    #[inline(always)]
    pub(crate) fn equals(&self, a: &Object, b: &Object) -> bool {
        match (a, b) {
            (Object::Integer(x), Object::Integer(y)) => x == y,
            (Object::Float(x), Object::Float(y)) => x == y,
            (Object::Integer(x), Object::Float(y)) => (*x as f64) == *y,
            (Object::Float(x), Object::Integer(y)) => *x == (*y as f64),
            (Object::Boolean(x), Object::Boolean(y)) => x == y,
            (Object::String(x), Object::String(y)) => Rc::ptr_eq(x, y) || x == y,
            (Object::Null, Object::Null) => true,
            (Object::Null, Object::Undefined) | (Object::Undefined, Object::Null) => true,
            (Object::Undefined, Object::Undefined) => true,
            (Object::Boolean(v), other) => {
                let n = Object::Integer(if *v { 1 } else { 0 });
                self.equals(&n, other)
            }
            (other, Object::Boolean(v)) => {
                let n = Object::Integer(if *v { 1 } else { 0 });
                self.equals(other, &n)
            }
            (Object::String(s), Object::Integer(n)) => {
                let parsed = Self::js_string_to_number(s);
                !parsed.is_nan() && parsed == (*n as f64)
            }
            (Object::String(s), Object::Float(n)) => {
                let parsed = Self::js_string_to_number(s);
                !parsed.is_nan() && parsed == *n
            }
            (Object::Integer(n), Object::String(s)) => {
                let parsed = Self::js_string_to_number(s);
                !parsed.is_nan() && (*n as f64) == parsed
            }
            (Object::Float(n), Object::String(s)) => {
                let parsed = Self::js_string_to_number(s);
                !parsed.is_nan() && *n == parsed
            }
            (Object::Array(_), _)
            | (Object::Hash(_), _)
            | (_, Object::Array(_))
            | (_, Object::Hash(_)) => {
                let left = self.to_primitive_for_loose_eq(a);
                let right = self.to_primitive_for_loose_eq(b);
                match (left, right) {
                    (Some(l), Some(r)) => self.equals(&l, &r),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn to_primitive_for_loose_eq(&self, value: &Object) -> Option<Object> {
        match value {
            Object::Array(items) => {
                let borrowed = items.borrow();
                Some(Object::String(self.array_to_js_string(&borrowed).into()))
            }
            Object::Hash(_) => Some(Object::String("[object Object]".to_string().into())),
            other => Some(other.clone()),
        }
    }

    fn array_to_js_string(&self, items: &[Value]) -> String {
        let mut parts = Vec::with_capacity(items.len());
        for item in items {
            let obj = val_to_obj(*item, &self.heap);
            let piece = match &obj {
                Object::Undefined | Object::Null => String::new(),
                Object::String(s) => s.to_string(),
                Object::Integer(v) => v.to_string(),
                Object::Float(v) => v.to_string(),
                Object::Boolean(v) => {
                    if *v {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    }
                }
                Object::Array(nested) => self.array_to_js_string(&nested.borrow()),
                Object::Hash(_) => "[object Object]".to_string(),
                other => other.inspect(),
            };
            parts.push(piece);
        }
        parts.join(",")
    }

    fn to_js_string(&self, value: &Object) -> String {
        match value {
            Object::String(s) => s.to_string(),
            Object::Integer(v) => v.to_string(),
            Object::Float(v) => v.to_string(),
            Object::Boolean(v) => {
                if *v {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            Object::Null => "null".to_string(),
            Object::Undefined => "undefined".to_string(),
            Object::Array(items) => {
                let borrowed = items.borrow();
                self.array_to_js_string(&borrowed)
            }
            Object::Hash(_) => "[object Object]".to_string(),
            other => other.inspect(),
        }
    }

    #[inline(always)]
    pub(crate) fn strict_equals(&self, a: &Object, b: &Object) -> bool {
        match (a, b) {
            (Object::Integer(x), Object::Integer(y)) => x == y,
            (Object::Float(x), Object::Float(y)) => x == y,
            (Object::Integer(x), Object::Float(y)) => (*x as f64) == *y,
            (Object::Float(x), Object::Integer(y)) => *x == (*y as f64),
            (Object::Array(xs), Object::Array(ys)) => {
                let xs = xs.borrow();
                let ys = ys.borrow();
                xs.len() == ys.len()
                    && xs.iter().zip(ys.iter()).all(|(x, y)| {
                        let xo = val_to_obj(*x, &self.heap);
                        let yo = val_to_obj(*y, &self.heap);
                        self.strict_equals(&xo, &yo)
                    })
            }
            (Object::Hash(xh), Object::Hash(yh)) => {
                let xh = xh.borrow_mut();
                let yh = yh.borrow_mut();
                if xh.pairs.len() != yh.pairs.len() {
                    return false;
                }
                xh.sync_pairs_if_dirty();
                yh.sync_pairs_if_dirty();
                xh.pairs.iter().all(|(k, xv)| {
                    yh.pairs
                        .get(k)
                        .map(|yv| {
                            let xo = val_to_obj(*xv, &self.heap);
                            let yo = val_to_obj(*yv, &self.heap);
                            self.strict_equals(&xo, &yo)
                        })
                        .unwrap_or(false)
                })
            }
            (Object::Boolean(x), Object::Boolean(y)) => x == y,
            (Object::String(x), Object::String(y)) => Rc::ptr_eq(x, y) || x == y,
            (Object::Null, Object::Null) => true,
            (Object::Undefined, Object::Undefined) => true,
            _ => false,
        }
    }

    fn same_value(&self, a: &Object, b: &Object) -> bool {
        match (a, b) {
            (Object::Integer(x), Object::Integer(y)) => x == y,
            (Object::Float(x), Object::Float(y)) => {
                if x.is_nan() && y.is_nan() {
                    true
                } else if *x == 0.0 && *y == 0.0 {
                    x.is_sign_negative() == y.is_sign_negative()
                } else {
                    x == y
                }
            }
            (Object::Integer(x), Object::Float(y)) | (Object::Float(y), Object::Integer(x)) => {
                let xf = *x as f64;
                if xf == 0.0 && *y == 0.0 {
                    !y.is_sign_negative()
                } else {
                    xf == *y
                }
            }
            (Object::Boolean(x), Object::Boolean(y)) => x == y,
            (Object::String(x), Object::String(y)) => x == y,
            (Object::Null, Object::Null) => true,
            (Object::Undefined, Object::Undefined) => true,
            _ => false,
        }
    }

    pub(crate) fn to_number(&self, value: &Object) -> Result<f64, VMError> {
        match value {
            Object::Integer(v) => Ok(*v as f64),
            Object::Float(v) => Ok(*v),
            Object::Boolean(v) => Ok(if *v { 1.0 } else { 0.0 }),
            Object::Null => Ok(0.0),
            Object::Undefined => Ok(f64::NAN),
            Object::String(s) => Ok(Self::js_string_to_number(s)),
            Object::Array(items) => {
                let borrowed = items.borrow();
                Ok(Self::js_string_to_number(
                    &self.array_to_js_string(&borrowed),
                ))
            }
            Object::Hash(_) => Ok(f64::NAN),
            // Match JavaScript behavior: Number(function) → NaN, Number(anything) → NaN
            _ => Ok(f64::NAN),
        }
    }

    /// Convert days since Unix epoch to (year, month, day).
    fn days_to_ymd(days: i64) -> (i64, i64, i64) {
        // Algorithm from Howard Hinnant's civil_from_days
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = (z - era * 146097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = if m <= 2 { y + 1 } else { y };
        (year, m as i64, d as i64)
    }

    /// Extract epoch ms from a Date method's receiver (stored as Object::Float).
    fn extract_date_ms(receiver: &Option<Object>) -> f64 {
        match receiver {
            Some(Object::Float(ms)) => *ms,
            Some(Object::Integer(ms)) => *ms as f64,
            _ => epoch_millis_now(),
        }
    }

    fn js_string_to_number(s: &str) -> f64 {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return 0.0;
        }

        if trimmed == "Infinity" || trimmed == "+Infinity" {
            return f64::INFINITY;
        }
        if trimmed == "-Infinity" {
            return f64::NEG_INFINITY;
        }

        let (sign, body) = if let Some(rest) = trimmed.strip_prefix('-') {
            (-1.0, rest)
        } else if let Some(rest) = trimmed.strip_prefix('+') {
            (1.0, rest)
        } else {
            (1.0, trimmed)
        };

        if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
            if hex.is_empty() {
                return f64::NAN;
            }
            if let Ok(v) = i64::from_str_radix(hex, 16) {
                return sign * (v as f64);
            }
            return f64::NAN;
        }

        if let Some(bin) = body.strip_prefix("0b").or_else(|| body.strip_prefix("0B")) {
            if bin.is_empty() {
                return f64::NAN;
            }
            if let Ok(v) = i64::from_str_radix(bin, 2) {
                return sign * (v as f64);
            }
            return f64::NAN;
        }

        if let Some(oct) = body.strip_prefix("0o").or_else(|| body.strip_prefix("0O")) {
            if oct.is_empty() {
                return f64::NAN;
            }
            if let Ok(v) = i64::from_str_radix(oct, 8) {
                return sign * (v as f64);
            }
            return f64::NAN;
        }

        trimmed.parse::<f64>().unwrap_or(f64::NAN)
    }

    pub(crate) fn to_i32(&self, value: &Object) -> Result<i32, VMError> {
        let n = self.to_number(value)?;
        if n.is_nan() || n.is_infinite() {
            return Ok(0);
        }
        Ok((n as i64 as u32) as i32)
    }

    fn to_u32(&self, value: &Object) -> Result<u32, VMError> {
        let n = self.to_number(value)?;
        if n.is_nan() || n.is_infinite() {
            return Ok(0);
        }
        Ok(n as i64 as u32)
    }

    pub(crate) fn to_number_val(&self, val: Value) -> Result<f64, VMError> {
        if val.is_i32() {
            return Ok(unsafe { val.as_i32_unchecked() } as f64);
        }
        if val.is_f64() {
            return Ok(val.as_f64());
        }
        self.to_number(&val_to_obj(val, &self.heap))
    }

    /// Like to_number_val but calls valueOf() on Instance objects if available.
    pub(crate) fn coerce_to_number_val(&mut self, val: Value) -> Result<f64, VMError> {
        if val.is_i32() {
            return Ok(unsafe { val.as_i32_unchecked() } as f64);
        }
        if val.is_f64() {
            return Ok(val.as_f64());
        }
        if val.is_heap() {
            let heap_idx = val.heap_index() as usize;
            let heap_obj = unsafe { &*self.heap.objects.as_ptr().add(heap_idx) };
            if let Object::Instance(inst) = heap_obj {
                if let Some(value_of_func) = inst.methods.get("valueOf").cloned() {
                    let (result, _) = self.execute_compiled_function_slice(
                        value_of_func,
                        &[],
                        Some(val),
                    )?;
                    return self.to_number_val(result);
                }
            }
        }
        self.to_number(&val_to_obj(val, &self.heap))
    }

    /// Coerce an Instance to a primitive via valueOf()/toString() for the + operator.
    /// Returns the coerced Value (which may be a number or string).
    fn coerce_instance_for_add(&mut self, val: Value) -> Result<Value, VMError> {
        if val.is_heap() {
            let heap_idx = val.heap_index() as usize;
            let heap_obj = unsafe { &*self.heap.objects.as_ptr().add(heap_idx) };
            if let Object::Instance(inst) = heap_obj {
                if let Some(value_of_func) = inst.methods.get("valueOf").cloned() {
                    let (result, _) = self.execute_compiled_function_slice(
                        value_of_func,
                        &[],
                        Some(val),
                    )?;
                    return Ok(result);
                }
                if let Some(to_str_func) = inst.methods.get("toString").cloned() {
                    let (result, _) = self.execute_compiled_function_slice(
                        to_str_func,
                        &[],
                        Some(val),
                    )?;
                    return Ok(result);
                }
            }
        }
        Ok(val)
    }

    pub(crate) fn to_i32_val(&self, val: Value) -> Result<i32, VMError> {
        if val.is_i32() {
            return Ok(unsafe { val.as_i32_unchecked() });
        }
        self.to_i32(&val_to_obj(val, &self.heap))
    }

    fn to_u32_val(&self, val: Value) -> Result<u32, VMError> {
        if val.is_i32() {
            let i = unsafe { val.as_i32_unchecked() };
            return Ok(i as u32);
        }
        self.to_u32(&val_to_obj(val, &self.heap))
    }

    #[inline(always)]
    pub(crate) fn compare_numeric(
        &self,
        a: &Object,
        b: &Object,
        op: Opcode,
    ) -> Result<bool, VMError> {
        let (x, y) = match (a, b) {
            (Object::Integer(x), Object::Integer(y)) => (*x as f64, *y as f64),
            (Object::Float(x), Object::Float(y)) => (*x, *y),
            (Object::Integer(x), Object::Float(y)) => (*x as f64, *y),
            (Object::Float(x), Object::Integer(y)) => (*x, *y as f64),
            _ => (self.to_number(a)?, self.to_number(b)?),
        };

        Ok(match op {
            Opcode::OpGreaterThan => x > y,
            Opcode::OpLessThan => x < y,
            Opcode::OpGreaterOrEqual => x >= y,
            Opcode::OpLessOrEqual => x <= y,
            _ => false,
        })
    }

    pub(crate) fn hash_key_from_object(&self, obj: &Object) -> HashKey {
        match obj {
            Object::String(s) => HashKey::Sym(crate::intern::intern_rc(s)),
            Object::Integer(v) => HashKey::from_int(*v),
            Object::Float(v) => HashKey::from_float(*v),
            Object::Boolean(v) => HashKey::from_bool(*v),
            Object::Null => HashKey::Null,
            Object::Undefined => HashKey::Undefined,
            _ => HashKey::Other(obj.inspect()),
        }
    }

    /// Convert a NaN-boxed Value to a HashKey without full Object conversion.
    #[inline(always)]
    pub(crate) fn hash_key_from_value(&self, val: Value) -> HashKey {
        if val.is_i32() {
            return HashKey::from_int(unsafe { val.as_i32_unchecked() } as i64);
        }
        if val.is_f64() {
            return HashKey::from_float(val.as_f64());
        }
        if val.is_bool() {
            return HashKey::from_bool(unsafe { val.as_bool_unchecked() });
        }
        if val.is_null() {
            return HashKey::Null;
        }
        if val.is_undefined() {
            return HashKey::Undefined;
        }
        if val.is_inline_str() {
            let (buf, len) = val.inline_str_buf();
            let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
            return HashKey::Sym(crate::intern::intern(s));
        }
        if val.is_heap() {
            let obj = self.heap.get(val.heap_index());
            match obj {
                Object::String(s) => HashKey::Sym(crate::intern::intern_rc(s)),
                Object::Symbol(id, _) => HashKey::Other(format!("@@sym:{}", id)),
                other => HashKey::Other(other.inspect()),
            }
        } else {
            HashKey::Undefined
        }
    }

    pub(crate) fn object_from_hash_key(&self, key: &HashKey) -> Object {
        match key {
            HashKey::Sym(id) => Object::String(crate::intern::resolve(*id)),
            HashKey::Int(v) => Object::Integer(*v),
            HashKey::Float(bits) => Object::Float(f64::from_bits(*bits)),
            HashKey::Bool(v) => Object::Boolean(*v),
            HashKey::Null => Object::Null,
            HashKey::Undefined => Object::Undefined,
            HashKey::Other(s) => Object::String(s.clone().into()),
        }
    }

    pub(crate) fn map_insert_or_replace(
        entries: &mut Vec<(HashKey, Value)>,
        indices: &mut rustc_hash::FxHashMap<HashKey, usize>,
        key: HashKey,
        value: Value,
    ) {
        // Single hash computation via entry() API instead of get() + insert()
        match indices.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => {
                let idx = *e.get();
                if let Some((_, existing_value)) = entries.get_mut(idx) {
                    *existing_value = value;
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                let idx = entries.len();
                entries.push((e.key().clone(), value));
                e.insert(idx);
            }
        }
    }

    pub(crate) fn map_get(
        entries: &[(HashKey, Value)],
        indices: &rustc_hash::FxHashMap<HashKey, usize>,
        key: &HashKey,
    ) -> Option<Value> {
        let idx = indices.get(key).copied()?;
        entries.get(idx).map(|(_, v)| *v)
    }

    pub(crate) fn map_contains(
        indices: &rustc_hash::FxHashMap<HashKey, usize>,
        key: &HashKey,
    ) -> bool {
        indices.contains_key(key)
    }

    fn map_remove(
        entries: &mut Vec<(HashKey, Value)>,
        indices: &mut rustc_hash::FxHashMap<HashKey, usize>,
        key: &HashKey,
    ) -> Option<Value> {
        let idx = indices.remove(key)?;
        let removed = entries.remove(idx).1;
        for i in idx..entries.len() {
            if let Some((k, _)) = entries.get(i) {
                indices.insert(k.clone(), i);
            }
        }
        Some(removed)
    }

    fn set_insert_unique(
        entries: &mut Vec<HashKey>,
        indices: &mut rustc_hash::FxHashMap<HashKey, usize>,
        key: HashKey,
    ) {
        if indices.contains_key(&key) {
            return;
        }
        let idx = entries.len();
        entries.push(key.clone());
        indices.insert(key, idx);
    }

    fn set_contains(indices: &rustc_hash::FxHashMap<HashKey, usize>, key: &HashKey) -> bool {
        indices.contains_key(key)
    }

    fn set_remove(
        entries: &mut Vec<HashKey>,
        indices: &mut rustc_hash::FxHashMap<HashKey, usize>,
        key: &HashKey,
    ) -> bool {
        let Some(idx) = indices.remove(key) else {
            return false;
        };
        entries.remove(idx);
        for i in idx..entries.len() {
            if let Some(k) = entries.get(i) {
                indices.insert(k.clone(), i);
            }
        }
        true
    }

    fn same_value_zero(a: &Object, b: &Object) -> bool {
        match (a, b) {
            (Object::Float(x), Object::Float(y)) => (x.is_nan() && y.is_nan()) || (*x == *y),
            (Object::Integer(x), Object::Integer(y)) => x == y,
            (Object::Integer(x), Object::Float(y)) | (Object::Float(y), Object::Integer(x)) => {
                !y.is_nan() && (*x as f64 == *y)
            }
            (Object::String(x), Object::String(y)) => x == y,
            (Object::Boolean(x), Object::Boolean(y)) => x == y,
            (Object::Null, Object::Null) => true,
            (Object::Undefined, Object::Undefined) => true,
            _ => false,
        }
    }

    pub(crate) fn strict_equal(a: &Object, b: &Object) -> bool {
        match (a, b) {
            (Object::Float(x), Object::Float(y)) => !x.is_nan() && !y.is_nan() && (*x == *y),
            (Object::Integer(x), Object::Integer(y)) => x == y,
            (Object::Integer(x), Object::Float(y)) | (Object::Float(y), Object::Integer(x)) => {
                !y.is_nan() && (*x as f64 == *y)
            }
            (Object::String(x), Object::String(y)) => x == y,
            (Object::Boolean(x), Object::Boolean(y)) => x == y,
            (Object::Null, Object::Null) => true,
            (Object::Undefined, Object::Undefined) => true,
            // Reference identity for Rc-backed heap types (JS === semantics)
            (Object::Array(x), Object::Array(y)) => Rc::ptr_eq(x, y),
            (Object::Hash(x), Object::Hash(y)) => Rc::ptr_eq(x, y),
            (Object::Set(x), Object::Set(y)) => Rc::ptr_eq(&x.entries, &y.entries),
            (Object::Map(x), Object::Map(y)) => Rc::ptr_eq(&x.entries, &y.entries),
            _ => false,
        }
    }

    fn slice_bounds(start: i32, end: i32, len: i32) -> (i32, i32) {
        let norm = |idx: i32| {
            if idx < 0 {
                (len + idx).max(0)
            } else {
                idx.min(len)
            }
        };
        let s = norm(start);
        let e = norm(end);
        if e < s {
            (s, s)
        } else {
            (s, e)
        }
    }

    fn expand_js_replacement(
        template: &str,
        full_match: &str,
        captures: &[Option<String>],
        prefix: &str,
        suffix: &str,
    ) -> String {
        let chars: Vec<char> = template.chars().collect();
        let mut out = String::new();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] != '$' {
                out.push(chars[i]);
                i += 1;
                continue;
            }

            if i + 1 >= chars.len() {
                out.push('$');
                i += 1;
                continue;
            }

            let next = chars[i + 1];
            match next {
                '$' => {
                    out.push('$');
                    i += 2;
                }
                '&' => {
                    out.push_str(full_match);
                    i += 2;
                }
                '`' => {
                    out.push_str(prefix);
                    i += 2;
                }
                '\'' => {
                    out.push_str(suffix);
                    i += 2;
                }
                '0'..='9' => {
                    if next == '0' {
                        out.push('$');
                        out.push('0');
                        i += 2;
                        continue;
                    }

                    let d1 = (next as u8 - b'0') as usize;
                    if i + 2 < chars.len() && chars[i + 2].is_ascii_digit() {
                        let d2 = (chars[i + 2] as u8 - b'0') as usize;
                        let idx2 = d1 * 10 + d2;
                        if idx2 > 0 && idx2 <= captures.len() {
                            if let Some(group) = &captures[idx2 - 1] {
                                out.push_str(group);
                            }
                            i += 3;
                            continue;
                        }
                    }

                    if d1 <= captures.len() {
                        if let Some(group) = &captures[d1 - 1] {
                            out.push_str(group);
                        }
                        i += 2;
                    } else {
                        out.push('$');
                        out.push(next);
                        i += 2;
                    }
                }
                _ => {
                    out.push('$');
                    out.push(next);
                    i += 2;
                }
            }
        }

        out
    }

    fn int_to_radix_string(value: i64, radix: u32) -> String {
        const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
        if value == 0 {
            return "0".to_string();
        }

        let negative = value < 0;
        let mut n = value.unsigned_abs() as u128;
        let mut buf: Vec<char> = Vec::new();
        while n > 0 {
            let d = (n % radix as u128) as usize;
            buf.push(DIGITS[d] as char);
            n /= radix as u128;
        }
        if negative {
            buf.push('-');
        }
        buf.iter().rev().collect()
    }

    pub(crate) fn get_keys_array(&mut self, source: Object) -> Vec<Value> {
        match source {
            Object::Array(items) => {
                let items = items.borrow();
                let mut out = Vec::with_capacity(items.len());
                for i in 0..items.len() {
                    out.push(obj_into_val(
                        Object::String(i.to_string().into()),
                        &mut self.heap,
                    ));
                }
                out
            }
            Object::String(s) => {
                let mut out = Vec::with_capacity(Self::string_char_len(&s));
                for (i, _) in s.chars().enumerate() {
                    out.push(obj_into_val(
                        Object::String(i.to_string().into()),
                        &mut self.heap,
                    ));
                }
                out
            }
            Object::Hash(hash) => {
                let hash_b = hash.borrow();
                let mut out = Vec::with_capacity(hash_b.pairs.len());
                for key in self.ordered_hash_keys_js(&hash_b) {
                    out.push(obj_into_val(
                        Object::String(key.display_key().into()),
                        &mut self.heap,
                    ));
                }
                out
            }
            _ => vec![],
        }
    }

    pub(crate) fn ordered_hash_keys_js(&self, hash: &crate::object::HashObject) -> Vec<HashKey> {
        let mut numeric = Vec::<(u32, HashKey)>::new();
        let mut others = Vec::<HashKey>::new();

        for key in hash.ordered_keys_ref() {
            if let Some(v) = key.is_numeric_index() {
                numeric.push((v, key.clone()));
            } else {
                others.push(key.clone());
            }
        }

        numeric.sort_by_key(|(v, _)| *v);
        let mut out = Vec::with_capacity(numeric.len() + others.len());
        out.extend(numeric.into_iter().map(|(_, k)| k));
        out.extend(others);
        out
    }

    fn object_key_cow<'a>(&self, obj: &'a Object) -> Cow<'a, str> {
        match obj {
            Object::String(s) => Cow::Borrowed(s),
            Object::Integer(v) => Cow::Owned(v.to_string()),
            Object::Float(v) if v.fract() == 0.0 => Cow::Owned((*v as i64).to_string()),
            Object::Float(v) => Cow::Owned(v.to_string()),
            Object::Boolean(v) => Cow::Owned(v.to_string()),
            _ => Cow::Owned(obj.inspect()),
        }
    }

    fn object_to_array_index(obj: &Object) -> Option<usize> {
        match obj {
            Object::Integer(v) if *v >= 0 => Some(*v as usize),
            Object::Float(v) if v.is_finite() && v.fract() == 0.0 && *v >= 0.0 => Some(*v as usize),
            Object::String(s) => Self::parse_non_negative_usize(s),
            _ => None,
        }
    }

    fn numeric_array_index(obj: &Object) -> Option<usize> {
        match obj {
            Object::Integer(v) if *v >= 0 => Some(*v as usize),
            Object::Float(v) if v.is_finite() && v.fract() == 0.0 && *v >= 0.0 => Some(*v as usize),
            _ => None,
        }
    }

    #[inline(always)]
    fn parse_non_negative_usize(s: &str) -> Option<usize> {
        if s.is_empty() {
            return None;
        }
        let bytes = s.as_bytes();
        let mut out: usize = 0;
        for b in bytes {
            if !b.is_ascii_digit() {
                return None;
            }
            out = out.checked_mul(10)?;
            out = out.checked_add((b - b'0') as usize)?;
        }
        Some(out)
    }

    #[inline(always)]
    fn string_char_len(s: &str) -> usize {
        if s.is_ascii() {
            s.len()
        } else {
            s.chars().count()
        }
    }

    #[inline(always)]
    fn string_nth_char(s: &str, idx: usize) -> Option<char> {
        if s.is_ascii() {
            s.as_bytes().get(idx).map(|b| *b as char)
        } else {
            s.chars().nth(idx)
        }
    }

    pub(crate) fn op_in(&self, left: &Object, right: &Object) -> bool {
        match right {
            Object::Array(items) => {
                let items = items.borrow();
                match left {
                    Object::String(s) => {
                        if &**s == "length" {
                            return true;
                        }
                        if let Some(idx) = Self::parse_non_negative_usize(s) {
                            return idx < items.len();
                        }
                    }
                    _ => {
                        if let Some(idx) = Self::numeric_array_index(left) {
                            return idx < items.len();
                        }
                    }
                }
                false
            }
            Object::Hash(hash) => {
                let hash_b = hash.borrow();
                match left {
                    Object::String(s) => hash_b.contains_str(s),
                    _ => {
                        let key = self.hash_key_from_object(left);
                        hash_b.pairs.contains_key(&key)
                    }
                }
            }
            Object::Instance(instance) => {
                let k = self.object_key_cow(left);
                instance.fields.contains_key(k.as_ref())
                    || instance.methods.contains_key(k.as_ref())
                    || instance.getters.contains_key(k.as_ref())
                    || instance.setters.contains_key(k.as_ref())
            }
            Object::Class(class_obj) => {
                let k = self.object_key_cow(left);
                class_obj.methods.contains_key(k.as_ref())
                    || class_obj.static_methods.contains_key(k.as_ref())
                    || class_obj.static_fields.contains_key(k.as_ref())
                    || class_obj.getters.contains_key(k.as_ref())
                    || class_obj.setters.contains_key(k.as_ref())
                    || class_obj.super_methods.contains_key(k.as_ref())
                    || class_obj.super_getters.contains_key(k.as_ref())
                    || class_obj.super_setters.contains_key(k.as_ref())
            }
            Object::SuperRef(super_ref) => {
                let k = self.object_key_cow(left);
                super_ref.methods.contains_key(k.as_ref())
                    || super_ref.getters.contains_key(k.as_ref())
                    || super_ref.setters.contains_key(k.as_ref())
            }
            _ => false,
        }
    }

    pub(crate) fn op_instanceof(&self, left: &Object, right: &Object) -> bool {
        match (left, right) {
            (Object::Instance(inst), Object::Class(class_obj)) => {
                inst.class_name == class_obj.name
                    || inst.parent_chain.iter().any(|n| n == &class_obj.name)
            }
            (_, Object::Hash(hash)) if hash.borrow().contains_str("from") => {
                matches!(left, Object::Array(_))
            }
            (_, Object::Hash(hash)) if hash.borrow().contains_str("keys") => {
                matches!(
                    left,
                    Object::Hash(_) | Object::Array(_) | Object::Instance(_)
                )
            }
            _ => false,
        }
    }

    pub(crate) fn execute_index_expression(
        &mut self,
        left: Object,
        index: Object,
    ) -> Result<(), VMError> {
        match left {
            Object::Array(items) => {
                if let Some(idx) = Self::numeric_array_index(&index) {
                    let borrowed = items.borrow();
                    if idx >= borrowed.len() {
                        self.push_val(Value::UNDEFINED)?;
                    } else {
                        // Array stores Values — push directly, no conversion
                        self.push_val(borrowed[idx])?;
                    }
                    return Ok(());
                }

                if let Object::String(s) = &index {
                    if &**s == "length" {
                        self.push(Object::Integer(items.borrow().len() as i64))?;
                        return Ok(());
                    }

                    let builtin = match &**s {
                        "map" => Some(BuiltinFunction::ArrayMap),
                        "forEach" => Some(BuiltinFunction::ArrayForEach),
                        "flatMap" => Some(BuiltinFunction::ArrayFlatMap),
                        "flat" => Some(BuiltinFunction::ArrayFlat),
                        "reverse" => Some(BuiltinFunction::ArrayReverse),
                        "some" => Some(BuiltinFunction::ArraySome),
                        "every" => Some(BuiltinFunction::ArrayEvery),
                        "findIndex" => Some(BuiltinFunction::ArrayFindIndex),
                        "indexOf" => Some(BuiltinFunction::ArrayIndexOf),
                        "lastIndexOf" => Some(BuiltinFunction::ArrayLastIndexOf),
                        "pop" => Some(BuiltinFunction::ArrayPop),
                        "push" => Some(BuiltinFunction::ArrayPush),
                        "sort" => Some(BuiltinFunction::ArraySort),
                        "filter" => Some(BuiltinFunction::ArrayFilter),
                        "reduce" => Some(BuiltinFunction::ArrayReduce),
                        "reduceRight" => Some(BuiltinFunction::ArrayReduceRight),
                        "find" => Some(BuiltinFunction::ArrayFind),
                        "includes" => Some(BuiltinFunction::ArrayIncludes),
                        "join" => Some(BuiltinFunction::ArrayJoin),
                        "toString" => Some(BuiltinFunction::ArrayToString),
                        "valueOf" => Some(BuiltinFunction::ArrayValueOf),
                        "slice" => Some(BuiltinFunction::ArraySlice),
                        "at" => Some(BuiltinFunction::ArrayAt),
                        "toSorted" => Some(BuiltinFunction::ArrayToSorted),
                        "with" => Some(BuiltinFunction::ArrayWith),
                        "keys" => Some(BuiltinFunction::ArrayKeys),
                        "values" => Some(BuiltinFunction::ArrayValues),
                        "entries" => Some(BuiltinFunction::ArrayEntries),
                        "shift" => Some(BuiltinFunction::ArrayShift),
                        "unshift" => Some(BuiltinFunction::ArrayUnshift),
                        "splice" => Some(BuiltinFunction::ArraySplice),
                        "concat" => Some(BuiltinFunction::ArrayConcat),
                        "fill" => Some(BuiltinFunction::ArrayFill),
                        "copyWithin" => Some(BuiltinFunction::ArrayCopyWithin),
                        "findLast" => Some(BuiltinFunction::ArrayFindLast),
                        "findLastIndex" => Some(BuiltinFunction::ArrayFindLastIndex),
                        "toReversed" => Some(BuiltinFunction::ArrayToReversed),
                        _ => None,
                    };
                    if let Some(function) = builtin {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function,
                            receiver: Some(Object::Array(items)),
                        })))?;
                        return Ok(());
                    }

                    if let Some(idx) = Self::parse_non_negative_usize(s) {
                        let borrowed = items.borrow();
                        if idx >= borrowed.len() {
                            self.push_val(Value::UNDEFINED)?;
                        } else {
                            self.push_val(borrowed[idx])?;
                        }
                        return Ok(());
                    }
                }
                self.push(undefined_object())?;
                Ok(())
            }
            Object::Hash(hash) => {
                let hash_b = hash.borrow();

                // Check for getter accessor first (if the hash has any accessors).
                if hash_b.has_accessors() {
                    if let Object::String(s) = &index {
                        if let Some(getter) = hash_b.get_getter(s) {
                            let getter_func = getter.clone();
                            let _ = hash_b; // end borrow before calling accessor
                            let receiver_val =
                                obj_into_val(Object::Hash(Rc::clone(&hash)), &mut self.heap);
                            let (result, _) = self.execute_compiled_function_slice(
                                getter_func,
                                &[],
                                Some(receiver_val),
                            )?;
                            self.push_val(result)?;
                            return Ok(());
                        }
                    }
                }

                let value_ref = match &index {
                    Object::String(s) => hash_b.get_value_by_str(s),
                    _ => {
                        let key = self.hash_key_from_object(&index);
                        hash_b
                            .pairs
                            .get_index_of(&key)
                            .and_then(|slot| hash_b.get_value_at_slot(slot))
                    }
                };
                match value_ref {
                    Some(val) => {
                        let obj = val_to_obj(val, &self.heap);
                        if let Object::CompiledFunction(func) = &obj {
                            self.push(Object::BoundMethod(Box::new(
                                crate::object::BoundMethodObject {
                                    function: (**func).clone(),
                                    receiver: Box::new(Object::Hash(Rc::clone(&hash))),
                                },
                            )))?;
                        } else {
                            self.push(obj)?;
                        }
                    }
                    None => {
                        // Built-in methods on hash/object literals
                        if let Object::String(s) = &index {
                            if &**s == "hasOwnProperty" {
                                self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                    function: BuiltinFunction::HashHasOwnProperty,
                                    receiver: Some(Object::Hash(Rc::clone(&hash))),
                                })))?;
                                return Ok(());
                            }
                        }
                        self.push(Object::Undefined)?;
                    }
                }
                Ok(())
            }
            Object::Instance(instance) => {
                let prop: Cow<str> = match &index {
                    Object::String(s) => Cow::Borrowed(s),
                    Object::Integer(v) => Cow::Owned(v.to_string()),
                    Object::Float(v) => Cow::Owned(v.to_string()),
                    Object::Boolean(v) => Cow::Owned(v.to_string()),
                    _ => {
                        self.push(undefined_object())?;
                        return Ok(());
                    }
                };

                if let Some(value) = instance.fields.get(prop.as_ref()) {
                    self.push(value.clone())?;
                    return Ok(());
                }

                if let Some(getter) = instance.getters.get(prop.as_ref()) {
                    let receiver_val = obj_into_val(
                        Object::Instance(Box::new((*instance).clone())),
                        &mut self.heap,
                    );
                    let (result, _) = self.execute_compiled_function_slice(
                        getter.clone(),
                        &[],
                        Some(receiver_val),
                    )?;
                    self.push_val(result)?;
                    return Ok(());
                }

                if let Some(method) = instance.methods.get(prop.as_ref()) {
                    self.push(Object::BoundMethod(Box::new(
                        crate::object::BoundMethodObject {
                            function: method.clone(),
                            receiver: Box::new(Object::Instance(Box::new((*instance).clone()))),
                        },
                    )))?;
                    return Ok(());
                }

                self.push(undefined_object())?;
                Ok(())
            }
            Object::Error(err) => {
                if let Object::String(s) = &index {
                    match &**s {
                        "message" => {
                            self.push(Object::String(err.message.clone()))?;
                        }
                        "name" => {
                            self.push(Object::String(err.name.clone()))?;
                        }
                        "stack" => {
                            self.push(Object::String(Rc::from("")))?;
                        }
                        _ => {
                            self.push(undefined_object())?;
                        }
                    }
                } else {
                    self.push(undefined_object())?;
                }
                Ok(())
            }
            Object::Class(class_obj) => {
                let prop: Cow<str> = match &index {
                    Object::String(s) => Cow::Borrowed(s),
                    Object::Integer(v) => Cow::Owned(v.to_string()),
                    Object::Float(v) => Cow::Owned(v.to_string()),
                    Object::Boolean(v) => Cow::Owned(v.to_string()),
                    _ => {
                        self.push(undefined_object())?;
                        return Ok(());
                    }
                };

                if let Some(method) = class_obj.static_methods.get(prop.as_ref()) {
                    self.push(Object::BoundMethod(Box::new(
                        crate::object::BoundMethodObject {
                            function: method.clone(),
                            receiver: Box::new(Object::Class(Box::new((*class_obj).clone()))),
                        },
                    )))?;
                    return Ok(());
                }

                if let Some(getter) = class_obj.getters.get(prop.as_ref()) {
                    let receiver_val = obj_into_val(
                        Object::Class(Box::new((*class_obj).clone())),
                        &mut self.heap,
                    );
                    let (result, _) = self.execute_compiled_function_slice(
                        getter.clone(),
                        &[],
                        Some(receiver_val),
                    )?;
                    self.push_val(result)?;
                    return Ok(());
                }

                if let Some(field_val) = class_obj.static_fields.get(prop.as_ref()) {
                    self.push(field_val.clone())?;
                    return Ok(());
                }

                self.push(undefined_object())?;
                Ok(())
            }
            Object::SuperRef(super_ref) => {
                let prop: Cow<str> = match &index {
                    Object::String(s) => Cow::Borrowed(s),
                    Object::Integer(v) => Cow::Owned(v.to_string()),
                    Object::Float(v) => Cow::Owned(v.to_string()),
                    Object::Boolean(v) => Cow::Owned(v.to_string()),
                    _ => {
                        self.push(undefined_object())?;
                        return Ok(());
                    }
                };

                // Helper: shift the receiver's super_methods to the next
                // ancestor level so that super.X() inside the parent method
                // correctly resolves to the grandparent's methods.
                let shift_receiver = |recv: &Object, chain: &[crate::object::SuperLevel]| -> Object {
                    let mut r = recv.clone();
                    if let Object::Instance(inst) = &mut r {
                        if let Some((next_m, next_g, next_s)) = chain.first() {
                            inst.super_methods = next_m.clone();
                            inst.super_getters = next_g.clone();
                            inst.super_setters = next_s.clone();
                            inst.super_constructor_chain = chain[1..].to_vec();
                        } else {
                            inst.super_methods.clear();
                            inst.super_getters.clear();
                            inst.super_setters.clear();
                            inst.super_constructor_chain.clear();
                        }
                    }
                    r
                };

                if let Some(getter) = super_ref.getters.get(prop.as_ref()) {
                    let shifted = shift_receiver(&super_ref.receiver, &super_ref.constructor_chain);
                    let receiver_val = obj_into_val(shifted, &mut self.heap);
                    let (result, _) = self.execute_compiled_function_slice(
                        getter.clone(),
                        &[],
                        Some(receiver_val),
                    )?;
                    self.push_val(result)?;
                    return Ok(());
                }

                if let Some(method) = super_ref.methods.get(prop.as_ref()) {
                    let shifted = shift_receiver(&super_ref.receiver, &super_ref.constructor_chain);
                    self.push(Object::BoundMethod(Box::new(
                        crate::object::BoundMethodObject {
                            function: method.clone(),
                            receiver: Box::new(shifted),
                        },
                    )))?;
                    return Ok(());
                }

                self.push(undefined_object())?;
                Ok(())
            }
            Object::String(text) => {
                match index {
                    Object::String(s) => match &*s {
                        "length" => {
                            self.push(Object::Integer(Self::string_char_len(&text) as i64))?;
                        }
                        "charAt" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringCharAt,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "split" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringSplit,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "includes" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringIncludes,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "slice" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringSlice,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "toUpperCase" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringToUpperCase,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "toLowerCase" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringToLowerCase,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "trim" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringTrim,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "startsWith" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringStartsWith,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "endsWith" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringEndsWith,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "indexOf" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringIndexOf,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "lastIndexOf" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringLastIndexOf,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "substring" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringSubstring,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "repeat" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringRepeat,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "padStart" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringPadStart,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "padEnd" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringPadEnd,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "charCodeAt" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringCharCodeAt,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "replace" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringReplace,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "replaceAll" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringReplaceAll,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "match" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringMatch,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "matchAll" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringMatchAll,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "search" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringSearch,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "concat" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringConcat,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "trimStart" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringTrimStart,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "trimEnd" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringTrimEnd,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "at" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringAt,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "codePointAt" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringCodePointAt,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        "normalize" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringNormalize,
                                receiver: Some(Object::String(text)),
                            })))?;
                        }
                        _ => {
                            self.push(undefined_object())?;
                        }
                    },
                    Object::Integer(v) => {
                        if v < 0 {
                            self.push(undefined_object())?;
                        } else {
                            let idx = v as usize;
                            let ch = Self::string_nth_char(&text, idx)
                                .map(|c| Object::String(c.to_string().into()));
                            self.push(ch.unwrap_or_else(undefined_object))?;
                        }
                    }
                    _ => {
                        self.push(undefined_object())?;
                    }
                }
                Ok(())
            }
            Object::Integer(v) => {
                let key = self.object_key_cow(&index);
                match key.as_ref() {
                    "toFixed" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::NumberToFixed,
                            receiver: Some(Object::Integer(v)),
                        })))?
                    }
                    "toPrecision" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::NumberToPrecision,
                            receiver: Some(Object::Integer(v)),
                        })))?
                    }
                    "toString" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::NumberToString,
                            receiver: Some(Object::Integer(v)),
                        })))?
                    }
                    _ => self.push(undefined_object())?,
                }
                Ok(())
            }
            Object::Float(v) => {
                let key = self.object_key_cow(&index);
                match key.as_ref() {
                    "toFixed" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::NumberToFixed,
                            receiver: Some(Object::Float(v)),
                        })))?
                    }
                    "toPrecision" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::NumberToPrecision,
                            receiver: Some(Object::Float(v)),
                        })))?
                    }
                    "toString" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::NumberToString,
                            receiver: Some(Object::Float(v)),
                        })))?
                    }
                    _ => self.push(undefined_object())?,
                }
                Ok(())
            }
            Object::BuiltinFunction(builtin) => {
                let key = self.object_key_cow(&index);
                match builtin.function {
                    BuiltinFunction::StringCtor => match key.as_ref() {
                        "fromCharCode" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringFromCharCode,
                                receiver: None,
                            })))?;
                        }
                        "fromCodePoint" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringFromCodePoint,
                                receiver: None,
                            })))?;
                        }
                        "raw" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::StringRaw,
                                receiver: None,
                            })))?;
                        }
                        _ => self.push(undefined_object())?,
                    },
                    BuiltinFunction::NumberCtor => match key.as_ref() {
                        "isNaN" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::NumberIsNaN,
                                receiver: None,
                            })))?
                        }
                        "isFinite" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::NumberIsFinite,
                                receiver: None,
                            })))?
                        }
                        "parseInt" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::ParseInt,
                                receiver: None,
                            })))?
                        }
                        "parseFloat" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::ParseFloat,
                                receiver: None,
                            })))?
                        }
                        "isInteger" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::NumberIsInteger,
                                receiver: None,
                            })))?
                        }
                        "isSafeInteger" => {
                            self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                                function: BuiltinFunction::NumberIsSafeInteger,
                                receiver: None,
                            })))?
                        }
                        "MAX_SAFE_INTEGER" => self.push(Object::Integer(9007199254740991))?,
                        "MIN_SAFE_INTEGER" => self.push(Object::Integer(-9007199254740991))?,
                        "EPSILON" => self.push(Object::Float(2f64.powi(-52)))?,
                        "MAX_VALUE" => self.push(Object::Float(f64::MAX))?,
                        "MIN_VALUE" => self.push(Object::Float(5e-324_f64))?,
                        "POSITIVE_INFINITY" => self.push(Object::Float(f64::INFINITY))?,
                        "NEGATIVE_INFINITY" => self.push(Object::Float(f64::NEG_INFINITY))?,
                        _ => self.push(undefined_object())?,
                    },
                    _ => self.push(undefined_object())?,
                }
                Ok(())
            }
            Object::RegExp(re) => {
                let key = self.object_key_cow(&index);
                match key.as_ref() {
                    "test" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::RegExpTest,
                            receiver: Some(Object::RegExp(re)),
                        })))?
                    }
                    "exec" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::RegExpExec,
                            receiver: Some(Object::RegExp(re)),
                        })))?
                    }
                    _ => self.push(undefined_object())?,
                }
                Ok(())
            }
            Object::Map(map_obj) => {
                let key = self.object_key_cow(&index);
                match key.as_ref() {
                    "size" => self.push(Object::Integer(map_obj.entries.borrow().len() as i64))?,
                    "set" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::MapSet,
                            receiver: Some(Object::Map(map_obj)),
                        })))?
                    }
                    "get" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::MapGet,
                            receiver: Some(Object::Map(map_obj)),
                        })))?
                    }
                    "has" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::MapHas,
                            receiver: Some(Object::Map(map_obj)),
                        })))?
                    }
                    "delete" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::MapDelete,
                            receiver: Some(Object::Map(map_obj)),
                        })))?
                    }
                    "clear" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::MapClear,
                            receiver: Some(Object::Map(map_obj)),
                        })))?
                    }
                    "keys" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::MapKeys,
                            receiver: Some(Object::Map(map_obj)),
                        })))?
                    }
                    "values" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::MapValues,
                            receiver: Some(Object::Map(map_obj)),
                        })))?
                    }
                    "entries" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::MapEntries,
                            receiver: Some(Object::Map(map_obj)),
                        })))?
                    }
                    "forEach" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::MapForEach,
                            receiver: Some(Object::Map(map_obj)),
                        })))?
                    }
                    _ => self.push(undefined_object())?,
                }
                Ok(())
            }
            Object::Set(set_obj) => {
                let key = self.object_key_cow(&index);
                match key.as_ref() {
                    "size" => self.push(Object::Integer(set_obj.entries.borrow().len() as i64))?,
                    "add" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::SetAdd,
                            receiver: Some(Object::Set(set_obj)),
                        })))?
                    }
                    "has" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::SetHas,
                            receiver: Some(Object::Set(set_obj)),
                        })))?
                    }
                    "delete" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::SetDelete,
                            receiver: Some(Object::Set(set_obj)),
                        })))?
                    }
                    "clear" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::SetClear,
                            receiver: Some(Object::Set(set_obj)),
                        })))?
                    }
                    "keys" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::SetKeys,
                            receiver: Some(Object::Set(set_obj)),
                        })))?
                    }
                    "values" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::SetValues,
                            receiver: Some(Object::Set(set_obj)),
                        })))?
                    }
                    "entries" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::SetEntries,
                            receiver: Some(Object::Set(set_obj)),
                        })))?
                    }
                    "forEach" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::SetForEach,
                            receiver: Some(Object::Set(set_obj)),
                        })))?
                    }
                    _ => self.push(undefined_object())?,
                }
                Ok(())
            }
            Object::Generator(gen_rc) => {
                let key = self.object_key_cow(&index);
                match key.as_ref() {
                    "next" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::GeneratorNext,
                            receiver: Some(Object::Generator(gen_rc)),
                        })))?
                    }
                    "return" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::GeneratorReturn,
                            receiver: Some(Object::Generator(gen_rc)),
                        })))?
                    }
                    "throw" => {
                        self.push(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                            function: BuiltinFunction::GeneratorThrow,
                            receiver: Some(Object::Generator(gen_rc)),
                        })))?
                    }
                    _ => self.push(undefined_object())?,
                }
                Ok(())
            }
            _ => {
                self.push(undefined_object())?;
                Ok(())
            }
        }
    }

    #[allow(dead_code)]
    fn execute_call(&mut self, num_args: usize) -> Result<(), VMError> {
        let callee_val = self.stage_call_args(num_args)?;
        let mut args = std::mem::take(&mut self.arg_buffer);
        let out = self.call_value_slice(callee_val, &args);
        args.clear();
        self.arg_buffer = args;
        self.push_val(out?)?;
        Ok(())
    }

    fn execute_call_with_args(&mut self, callee: Value, args: Vec<Value>) -> Result<(), VMError> {
        let out = self.call_value_slice(callee, &args)?;
        self.push_val(out)?;
        Ok(())
    }

    fn stage_call_args(&mut self, num_args: usize) -> Result<Value, VMError> {
        if self.sp < num_args + 1 {
            return Err(VMError::StackUnderflow);
        }

        let callee_index = self.sp - num_args - 1;
        self.arg_buffer.clear();
        if self.arg_buffer.capacity() < num_args {
            self.arg_buffer
                .reserve(num_args - self.arg_buffer.capacity());
        }
        for idx in (callee_index + 1)..self.sp {
            let val = unsafe { *self.stack.as_ptr().add(idx) };
            self.arg_buffer.push(val);
        }

        let callee_val = unsafe { *self.stack.as_ptr().add(callee_index) };
        debug_assert!(callee_index <= self.stack.len());
        unsafe { self.stack.set_len(callee_index) };
        self.sp = callee_index;
        Ok(callee_val)
    }

    /// Stage call arguments from the stack into `arg_buffer` without popping a
    /// callee.  Used by `OpCallGlobal` where the function is read directly from
    /// the globals array instead of from the stack.
    #[inline(always)]
    fn stage_call_args_no_callee(&mut self, num_args: usize) {
        let args_start = self.sp - num_args;
        self.arg_buffer.clear();
        if self.arg_buffer.capacity() < num_args {
            self.arg_buffer
                .reserve(num_args - self.arg_buffer.capacity());
        }
        for idx in args_start..self.sp {
            let val = unsafe { *self.stack.as_ptr().add(idx) };
            self.arg_buffer.push(val);
        }
        debug_assert!(args_start <= self.stack.len());
        unsafe { self.stack.set_len(args_start) };
        self.sp = args_start;
    }

    pub(crate) fn call_value_slice(
        &mut self,
        callee: Value,
        args: &[Value],
    ) -> Result<Value, VMError> {
        // Peek heap to extract lightweight data without full val_to_obj clone
        if callee.is_heap() {
            let heap_obj = self.heap.get(callee.heap_index());
            match heap_obj {
                Object::BuiltinFunction(b) => {
                    // Clone just the fields, skip Box allocation
                    let builtin = BuiltinFunctionObject {
                        function: b.function.clone(),
                        receiver: b.receiver.clone(),
                    };
                    return self.execute_builtin_function_slice(builtin, args);
                }
                Object::CompiledFunction(f) => {
                    if f.is_generator {
                        let func = (**f).clone();
                        return Ok(self.create_generator(func, args.to_vec(), None));
                    }
                    // Clone CompiledFunctionObject without Box allocation
                    let func = (**f).clone();
                    let (result, _) = self.execute_compiled_function_slice(func, args, None)?;
                    return Ok(result);
                }
                // Hash with __call__: invoke the __call__ builtin (e.g. Symbol())
                Object::Hash(hash_rc) => {
                    let call_sym = crate::intern::intern("__call__");
                    if let Some(call_val) = hash_rc.borrow().get_by_sym(call_sym) {
                        return self.call_value_slice(call_val, args);
                    }
                }
                _ => {}
            }
        }
        // Fall through for BoundMethod, SuperRef (need ownership for destructure)
        let callee_obj = val_to_obj(callee, &self.heap);
        match callee_obj {
            Object::BoundMethod(bound) => {
                if bound.function.is_generator {
                    let receiver_val = obj_into_val(*bound.receiver, &mut self.heap);
                    return Ok(self.create_generator(
                        bound.function,
                        args.to_vec(),
                        Some(receiver_val),
                    ));
                }
                let receiver_val = obj_into_val(*bound.receiver, &mut self.heap);
                let (result, _) =
                    self.execute_compiled_function_slice(bound.function, args, Some(receiver_val))?;
                Ok(result)
            }
            Object::SuperRef(super_ref) => {
                let SuperRefObject {
                    mut receiver,
                    mut methods,
                    constructor_chain,
                    ..
                } = *super_ref;
                if let Some(ctor) = methods.remove("constructor") {
                    // Save original super info so we can restore after the parent
                    // constructor returns (needed for super.method() calls later).
                    let (saved_sm, saved_sg, saved_ss, saved_chain) =
                        if let Object::Instance(inst) = &*receiver {
                            (
                                inst.super_methods.clone(),
                                inst.super_getters.clone(),
                                inst.super_setters.clone(),
                                inst.super_constructor_chain.clone(),
                            )
                        } else {
                            (
                                rustc_hash::FxHashMap::default(),
                                rustc_hash::FxHashMap::default(),
                                rustc_hash::FxHashMap::default(),
                                vec![],
                            )
                        };

                    // Shift the super chain so that nested super() calls inside
                    // the parent constructor resolve to the next ancestor.
                    if let Object::Instance(inst) = &mut *receiver {
                        if let Some((next_methods, next_getters, next_setters)) =
                            constructor_chain.first()
                        {
                            inst.super_methods = next_methods.clone();
                            inst.super_getters = next_getters.clone();
                            inst.super_setters = next_setters.clone();
                            inst.super_constructor_chain =
                                constructor_chain[1..].to_vec();
                        } else {
                            inst.super_methods.clear();
                            inst.super_getters.clear();
                            inst.super_setters.clear();
                            inst.super_constructor_chain.clear();
                        }
                    }
                    let receiver_val = obj_into_val(*receiver, &mut self.heap);
                    let (result, receiver_after) =
                        self.execute_compiled_function_slice(ctor, args, Some(receiver_val))?;

                    // Restore original super info on the returned instance so that
                    // super.method() calls in the derived class work correctly.
                    let final_val = receiver_after.unwrap_or(result);
                    if final_val.is_heap() {
                        let heap_idx = final_val.heap_index() as usize;
                        if let Some(Object::Instance(inst)) =
                            self.heap.objects.get_mut(heap_idx)
                        {
                            inst.super_methods = saved_sm;
                            inst.super_getters = saved_sg;
                            inst.super_setters = saved_ss;
                            inst.super_constructor_chain = saved_chain;
                        }
                    }
                    Ok(final_val)
                } else {
                    Err(VMError::TypeError(
                        "super constructor not found".to_string(),
                    ))
                }
            }
            other => Err(VMError::TypeError(format!(
                "not callable: {:?}",
                other.object_type()
            ))),
        }
    }

    /// Returns the maximum number of positional arguments the callback will access.
    /// For compiled functions without rest params, this is num_parameters.
    /// For everything else, returns a conservative MAX to ensure all args are passed.
    fn callback_max_used_args(callback: &Object) -> usize {
        match callback {
            Object::CompiledFunction(f) if f.rest_parameter_index.is_none() => f.num_parameters,
            Object::BoundMethod(b) if b.function.rest_parameter_index.is_none() => {
                b.function.num_parameters
            }
            _ => usize::MAX,
        }
    }

    fn callback_max_used_args_val(callback: Value, heap: &Heap) -> usize {
        if callback.is_heap() {
            return Self::callback_max_used_args(heap.get(callback.heap_index()));
        }
        usize::MAX
    }

    fn call_value2(&mut self, callee: Value, a: Value, b: Value) -> Result<Value, VMError> {
        let args = [a, b];
        self.call_value_slice(callee, &args)
    }

    fn call_value3(
        &mut self,
        callee: Value,
        a: Value,
        b: Value,
        c: Value,
    ) -> Result<Value, VMError> {
        let args = [a, b, c];
        self.call_value_slice(callee, &args)
    }

    fn call_value4(
        &mut self,
        callee: Value,
        a: Value,
        b: Value,
        c: Value,
        d: Value,
    ) -> Result<Value, VMError> {
        let args = [a, b, c, d];
        self.call_value_slice(callee, &args)
    }

    fn execute_builtin_function_slice(
        &mut self,
        builtin: BuiltinFunctionObject,
        args: &[Value],
    ) -> Result<Value, VMError> {
        match builtin.function {
            BuiltinFunction::MathAbs => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(0.0);
                if n.is_finite() && n.fract() == 0.0 {
                    Ok(Value::from_i64(n.abs() as i64))
                } else {
                    Ok(Value::from_f64(n.abs()))
                }
            }
            BuiltinFunction::MathFloor => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.floor();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathCeil => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.ceil();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathRound => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.round();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathMin => {
                if args.is_empty() {
                    return Ok(Value::from_f64(f64::INFINITY));
                }
                let mut min = f64::INFINITY;
                for arg in args {
                    let n = self.to_number_val(*arg)?;
                    if n.is_nan() {
                        return Ok(Value::from_f64(f64::NAN));
                    }
                    if n < min {
                        min = n;
                    }
                }
                if min.is_finite() && min.fract() == 0.0 {
                    Ok(Value::from_i64(min as i64))
                } else {
                    Ok(Value::from_f64(min))
                }
            }
            BuiltinFunction::MathMax => {
                if args.is_empty() {
                    return Ok(Value::from_f64(f64::NEG_INFINITY));
                }
                let mut max = f64::NEG_INFINITY;
                for arg in args {
                    let n = self.to_number_val(*arg)?;
                    if n.is_nan() {
                        return Ok(Value::from_f64(f64::NAN));
                    }
                    if n > max {
                        max = n;
                    }
                }
                if max.is_finite() && max.fract() == 0.0 {
                    Ok(Value::from_i64(max as i64))
                } else {
                    Ok(Value::from_f64(max))
                }
            }
            BuiltinFunction::MathPow => {
                let base = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let exp = args
                    .get(1)
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = base.powf(exp);
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathSqrt => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.sqrt();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathTrunc => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.trunc();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathSign => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = if n.is_nan() {
                    f64::NAN
                } else if n == 0.0 {
                    // JS Math.sign(0) === 0, Math.sign(-0) === -0
                    n
                } else {
                    n.signum()
                };
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathRandom => {
                #[cfg(target_arch = "wasm32")]
                {
                    Ok(Value::from_f64(js_sys::Math::random()))
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Xorshift64 PRNG — fast, stateful, uniform distribution
                    let mut s = self.rng_state;
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    self.rng_state = s;
                    // Convert to [0, 1) by taking upper 53 bits as f64
                    let n = (s >> 11) as f64 / (1u64 << 53) as f64;
                    Ok(Value::from_f64(n))
                }
            }
            BuiltinFunction::MathLog => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.ln();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathLog2 => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.log2();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathCbrt => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.cbrt();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathSin => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.sin();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathCos => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.cos();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathTan => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.tan();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathExp => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.exp();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathLog10 => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = n.log10();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathAtan2 => {
                let y = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let x = args
                    .get(1)
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = y.atan2(x);
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathHypot => {
                if args.is_empty() {
                    return Ok(Value::from_i64(0));
                }
                let mut sum_sq = 0.0;
                for arg in args {
                    let n = self.to_number_val(*arg)?;
                    if n.is_nan() {
                        return Ok(Value::from_f64(f64::NAN));
                    }
                    if n.is_infinite() {
                        return Ok(Value::from_f64(f64::INFINITY));
                    }
                    sum_sq += n * n;
                }
                let out = sum_sq.sqrt();
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::MathImul => {
                let a = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                let b = args
                    .get(1)
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                let out = a.wrapping_mul(b);
                Ok(Value::from_i64(out as i64))
            }
            BuiltinFunction::MathClz32 => {
                let n = args
                    .first()
                    .map(|v| self.to_u32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                Ok(Value::from_i64(n.leading_zeros() as i64))
            }
            BuiltinFunction::MathFround => {
                let n = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(f64::NAN);
                let out = (n as f32) as f64;
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::NumberCtor => {
                let out = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(0.0);
                if out.is_finite() && out.fract() == 0.0 {
                    Ok(Value::from_i64(out as i64))
                } else {
                    Ok(Value::from_f64(out))
                }
            }
            BuiltinFunction::StringCtor => {
                let value = args
                    .first()
                    .map(|v| {
                        let obj = val_to_obj(*v, &self.heap);
                        self.to_js_string(&obj)
                    })
                    .unwrap_or_else(String::new);
                Ok(obj_into_val(Object::String(value.into()), &mut self.heap))
            }
            BuiltinFunction::StringFromCharCode => {
                let mut out = String::new();
                for arg in args {
                    let code = self.to_u32_val(*arg)?;
                    let ch = char::from_u32(code).unwrap_or('\u{FFFD}');
                    out.push(ch);
                }
                Ok(obj_into_val(Object::String(out.into()), &mut self.heap))
            }
            BuiltinFunction::StringCharAt => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.charAt missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(Value::UNDEFINED),
                };
                let idx = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                if idx < 0 {
                    return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap));
                }
                let ch = Self::string_nth_char(&text, idx as usize);
                Ok(obj_into_val(
                    Object::String(ch.map(|c| c.to_string()).unwrap_or_default().into()),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::StringSplit => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.split missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(Value::UNDEFINED),
                };
                let limit: Option<usize> = if args.len() >= 2 {
                    Some(self.to_i32_val(args[1])?.max(0) as usize)
                } else {
                    None
                };

                // Check if separator is a RegExp
                let sep_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                let sep_obj = val_to_obj(sep_val, &self.heap);
                if let Object::RegExp(re) = &sep_obj {
                    let regex = self.build_regex(&re.pattern, &re.flags)?;
                    let mut items: Vec<Value> = Vec::new();
                    let mut last = 0;
                    let max = limit.unwrap_or(usize::MAX);
                    for m in regex.find_iter(&text) {
                        if items.len() >= max { break; }
                        items.push(obj_into_val(
                            Object::String(text[last..m.start()].to_string().into()),
                            &mut self.heap,
                        ));
                        last = m.end();
                    }
                    if items.len() < max {
                        items.push(obj_into_val(
                            Object::String(text[last..].to_string().into()),
                            &mut self.heap,
                        ));
                    }
                    return Ok(obj_into_val(make_array(items), &mut self.heap));
                }

                let sep = match sep_obj {
                    Object::String(s) => s.to_string(),
                    _ => sep_obj.inspect(),
                };
                if sep.is_empty() {
                    let items: Vec<Value> = text
                        .chars()
                        .take(limit.unwrap_or(usize::MAX))
                        .map(|c| {
                            obj_into_val(
                                Object::String(c.to_string().into()),
                                &mut self.heap,
                            )
                        })
                        .collect();
                    return Ok(obj_into_val(make_array(items), &mut self.heap));
                }
                let items: Vec<Value> = text
                    .split(&sep)
                    .take(limit.unwrap_or(usize::MAX))
                    .map(|s| {
                        obj_into_val(Object::String(s.to_string().into()), &mut self.heap)
                    })
                    .collect();
                Ok(obj_into_val(make_array(items), &mut self.heap))
            }
            BuiltinFunction::StringIncludes => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.includes missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(Value::from_bool(false)),
                };
                let needle = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(String::new);
                Ok(Value::from_bool(text.contains(&needle)))
            }
            BuiltinFunction::StringSlice => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.slice missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                let chars: Vec<char> = text.chars().collect();
                let len = chars.len() as i32;
                let start = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                let end = args
                    .get(1)
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(len);
                let (sidx, eidx) = Self::slice_bounds(start, end, len);
                let out: String = chars[sidx as usize..eidx as usize].iter().collect();
                Ok(obj_into_val(Object::String(out.into()), &mut self.heap))
            }
            BuiltinFunction::StringToUpperCase => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.toUpperCase missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                Ok(obj_into_val(
                    Object::String(text.to_uppercase().into()),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::StringToLowerCase => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.toLowerCase missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                Ok(obj_into_val(
                    Object::String(text.to_lowercase().into()),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::StringTrim => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.trim missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                Ok(obj_into_val(
                    Object::String(text.trim().to_string().into()),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::StringStartsWith => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.startsWith missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(Value::from_bool(false)),
                };
                let needle = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(String::new);
                let pos = args
                    .get(1)
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0)
                    .max(0) as usize;
                let slice: String = text.chars().skip(pos).collect();
                Ok(Value::from_bool(slice.starts_with(&needle)))
            }
            BuiltinFunction::StringEndsWith => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.endsWith missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(Value::from_bool(false)),
                };
                let needle = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(String::new);
                if let Some(end_pos_obj) = args.get(1) {
                    let end_pos = self.to_i32_val(*end_pos_obj)?.max(0) as usize;
                    let truncated: String = text.chars().take(end_pos).collect();
                    Ok(Value::from_bool(truncated.ends_with(&needle)))
                } else {
                    Ok(Value::from_bool(text.ends_with(&needle)))
                }
            }
            BuiltinFunction::StringIndexOf => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.indexOf missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(Value::from_i64(-1)),
                };
                let needle = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(String::new);
                let from = args
                    .get(1)
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0)
                    .max(0) as usize;
                if from >= text.len() {
                    return Ok(Value::from_i64(-1));
                }
                let sliced = &text[from..];
                if let Some(pos) = sliced.find(&needle) {
                    Ok(Value::from_i64((from + pos) as i64))
                } else {
                    Ok(Value::from_i64(-1))
                }
            }
            BuiltinFunction::StringLastIndexOf => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.lastIndexOf missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(Value::from_i64(-1)),
                };
                let needle = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(String::new);
                if needle.is_empty() {
                    let pos = args
                        .get(1)
                        .map(|v| self.to_i32_val(*v))
                        .transpose()?
                        .unwrap_or(text.len() as i32)
                        .max(0) as usize;
                    return Ok(Value::from_i64(pos.min(text.len()) as i64));
                }

                let default_pos = text.len().saturating_sub(needle.len()) as i32;
                let mut pos = args
                    .get(1)
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(default_pos)
                    .max(0) as usize;
                pos = pos.min(text.len().saturating_sub(needle.len()));

                for i in (0..=pos).rev() {
                    if text[i..].starts_with(&needle) {
                        return Ok(Value::from_i64(i as i64));
                    }
                }
                Ok(Value::from_i64(-1))
            }
            BuiltinFunction::StringSubstring => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.substring missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                let len = Self::string_char_len(&text) as i32;
                let mut start = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0)
                    .max(0)
                    .min(len);
                let mut end = args
                    .get(1)
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(len)
                    .max(0)
                    .min(len);
                if start > end {
                    std::mem::swap(&mut start, &mut end);
                }
                let chars: Vec<char> = text.chars().collect();
                let out: String = chars[start as usize..end as usize].iter().collect();
                Ok(obj_into_val(Object::String(out.into()), &mut self.heap))
            }
            BuiltinFunction::StringRepeat => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.repeat missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                let count = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                if count < 0 {
                    return Err(VMError::TypeError(
                        "String.repeat count must be non-negative".to_string(),
                    ));
                }
                Ok(obj_into_val(
                    Object::String(text.repeat(count as usize).into()),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::StringPadStart | BuiltinFunction::StringPadEnd => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.padStart/padEnd missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                let target_len = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0)
                    .max(0) as usize;
                let text_len = Self::string_char_len(&text);
                if text_len >= target_len {
                    return Ok(obj_into_val(Object::String(text), &mut self.heap));
                }
                let fill = args
                    .get(1)
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(|| " ".to_string());
                if fill.is_empty() {
                    return Ok(obj_into_val(Object::String(text), &mut self.heap));
                }

                let needed = target_len - text_len;
                let mut pad = String::new();
                while Self::string_char_len(&pad) < needed {
                    pad.push_str(&fill);
                }
                let pad: String = pad.chars().take(needed).collect();
                if matches!(builtin.function, BuiltinFunction::StringPadStart) {
                    Ok(obj_into_val(
                        Object::String(format!("{}{}", pad, text).into()),
                        &mut self.heap,
                    ))
                } else {
                    Ok(obj_into_val(
                        Object::String(format!("{}{}", text, pad).into()),
                        &mut self.heap,
                    ))
                }
            }
            BuiltinFunction::StringCharCodeAt => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.charCodeAt missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(Value::from_f64(f64::NAN)),
                };
                let idx = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                if idx < 0 {
                    return Ok(Value::from_f64(f64::NAN));
                }
                match Self::string_nth_char(&text, idx as usize) {
                    Some(ch) => Ok(Value::from_i64(ch as i64)),
                    None => Ok(Value::from_f64(f64::NAN)),
                }
            }
            BuiltinFunction::StringReplace => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.replace missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                let replacement_val = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let replacement_value = val_to_obj(replacement_val, &self.heap);
                let replacement_is_fn = matches!(
                    replacement_value,
                    Object::CompiledFunction(_)
                        | Object::BoundMethod(_)
                        | Object::BuiltinFunction(_)
                        | Object::SuperRef(_)
                );
                let arg0 = args.first().map(|v| val_to_obj(*v, &self.heap));
                let out = match arg0.as_ref() {
                    Some(Object::RegExp(re)) => {
                        let regex = self.build_regex(&re.pattern, &re.flags)?;
                        if replacement_is_fn {
                            let mut out = String::new();
                            let mut last = 0usize;
                            let mut cb_args: Vec<Value> = Vec::with_capacity(4);
                            for caps in regex.captures_iter(&text) {
                                let Some(m) = caps.get(0) else {
                                    continue;
                                };
                                out.push_str(&text[last..m.start()]);
                                cb_args.clear();
                                cb_args.push(obj_into_val(
                                    Object::String(m.as_str().to_string().into()),
                                    &mut self.heap,
                                ));
                                for i in 1..caps.len() {
                                    if let Some(g) = caps.get(i) {
                                        cb_args.push(obj_into_val(
                                            Object::String(g.as_str().to_string().into()),
                                            &mut self.heap,
                                        ));
                                    } else {
                                        cb_args.push(Value::UNDEFINED);
                                    }
                                }
                                cb_args.push(Value::from_i64(m.start() as i64));
                                cb_args.push(obj_into_val(
                                    Object::String(text.clone()),
                                    &mut self.heap,
                                ));
                                let replace_result =
                                    self.call_value_slice(replacement_val, &cb_args)?;
                                let replace_obj = val_to_obj(replace_result, &self.heap);
                                out.push_str(&replace_obj.inspect());
                                last = m.end();
                                if !re.flags.contains('g') {
                                    break;
                                }
                            }
                            out.push_str(&text[last..]);
                            out
                        } else {
                            let replacement_template = replacement_value.inspect();
                            let mut out = String::new();
                            let mut last = 0usize;
                            for caps in regex.captures_iter(&text) {
                                let Some(m) = caps.get(0) else {
                                    continue;
                                };
                                out.push_str(&text[last..m.start()]);
                                let mut groups = Vec::with_capacity(caps.len().saturating_sub(1));
                                for i in 1..caps.len() {
                                    groups.push(caps.get(i).map(|g| g.as_str().to_string()));
                                }
                                let expanded = Self::expand_js_replacement(
                                    &replacement_template,
                                    m.as_str(),
                                    &groups,
                                    &text[..m.start()],
                                    &text[m.end()..],
                                );
                                out.push_str(&expanded);
                                last = m.end();
                                if !re.flags.contains('g') {
                                    break;
                                }
                            }
                            out.push_str(&text[last..]);
                            out
                        }
                    }
                    Some(pattern) => {
                        let p = pattern.inspect();
                        if replacement_is_fn {
                            if let Some(start) = text.find(&p) {
                                let end = start + p.len();
                                let match_val =
                                    obj_into_val(Object::String(p.clone().into()), &mut self.heap);
                                let text_val =
                                    obj_into_val(Object::String(text.clone()), &mut self.heap);
                                let replacement_result = self.call_value3(
                                    replacement_val,
                                    match_val,
                                    Value::from_i64(start as i64),
                                    text_val,
                                )?;
                                let mut out = String::new();
                                out.push_str(&text[..start]);
                                out.push_str(&val_inspect(replacement_result, &self.heap));
                                out.push_str(&text[end..]);
                                out
                            } else {
                                text.to_string()
                            }
                        } else {
                            let replacement_template = replacement_value.inspect();
                            if let Some(start) = text.find(&p) {
                                let end = start + p.len();
                                let mut out = String::new();
                                out.push_str(&text[..start]);
                                out.push_str(&Self::expand_js_replacement(
                                    &replacement_template,
                                    &p,
                                    &[],
                                    &text[..start],
                                    &text[end..],
                                ));
                                out.push_str(&text[end..]);
                                out
                            } else {
                                text.to_string()
                            }
                        }
                    }
                    None => text.to_string(),
                };
                Ok(obj_into_val(Object::String(out.into()), &mut self.heap))
            }
            BuiltinFunction::StringReplaceAll => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.replaceAll missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                let replacement_val = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let replacement_value = val_to_obj(replacement_val, &self.heap);
                let replacement_is_fn = matches!(
                    replacement_value,
                    Object::CompiledFunction(_)
                        | Object::BoundMethod(_)
                        | Object::BuiltinFunction(_)
                        | Object::SuperRef(_)
                );

                let arg0 = args.first().map(|v| val_to_obj(*v, &self.heap));
                let out = match arg0.as_ref() {
                    Some(Object::RegExp(re)) => {
                        if !re.flags.contains('g') {
                            return Err(VMError::TypeError(
                                "String.prototype.replaceAll called with a non-global RegExp"
                                    .to_string(),
                            ));
                        }
                        let regex = self.build_regex(&re.pattern, &re.flags)?;
                        if replacement_is_fn {
                            let mut out = String::new();
                            let mut last = 0usize;
                            let mut cb_args: Vec<Value> = Vec::with_capacity(4);
                            for caps in regex.captures_iter(&text) {
                                let Some(m) = caps.get(0) else {
                                    continue;
                                };
                                out.push_str(&text[last..m.start()]);
                                cb_args.clear();
                                cb_args.push(obj_into_val(
                                    Object::String(m.as_str().to_string().into()),
                                    &mut self.heap,
                                ));
                                for i in 1..caps.len() {
                                    if let Some(g) = caps.get(i) {
                                        cb_args.push(obj_into_val(
                                            Object::String(g.as_str().to_string().into()),
                                            &mut self.heap,
                                        ));
                                    } else {
                                        cb_args.push(Value::UNDEFINED);
                                    }
                                }
                                cb_args.push(Value::from_i64(m.start() as i64));
                                cb_args.push(obj_into_val(
                                    Object::String(text.clone()),
                                    &mut self.heap,
                                ));
                                let replace_result =
                                    self.call_value_slice(replacement_val, &cb_args)?;
                                let replace_obj = val_to_obj(replace_result, &self.heap);
                                out.push_str(&replace_obj.inspect());
                                last = m.end();
                            }
                            out.push_str(&text[last..]);
                            out
                        } else {
                            let replacement_template = replacement_value.inspect();
                            let mut out = String::new();
                            let mut last = 0usize;
                            for caps in regex.captures_iter(&text) {
                                let Some(m) = caps.get(0) else {
                                    continue;
                                };
                                out.push_str(&text[last..m.start()]);
                                let mut groups = Vec::with_capacity(caps.len().saturating_sub(1));
                                for i in 1..caps.len() {
                                    groups.push(caps.get(i).map(|g| g.as_str().to_string()));
                                }
                                let expanded = Self::expand_js_replacement(
                                    &replacement_template,
                                    m.as_str(),
                                    &groups,
                                    &text[..m.start()],
                                    &text[m.end()..],
                                );
                                out.push_str(&expanded);
                                last = m.end();
                            }
                            out.push_str(&text[last..]);
                            out
                        }
                    }
                    Some(pattern) => {
                        let p = pattern.inspect();
                        if p.is_empty() {
                            return Ok(obj_into_val(Object::String(text), &mut self.heap));
                        }
                        if replacement_is_fn {
                            let mut out = String::new();
                            let mut cursor = 0usize;
                            while let Some(rel) = text[cursor..].find(&p) {
                                let start = cursor + rel;
                                let end = start + p.len();
                                out.push_str(&text[cursor..start]);
                                let match_val =
                                    obj_into_val(Object::String(p.clone().into()), &mut self.heap);
                                let text_val =
                                    obj_into_val(Object::String(text.clone()), &mut self.heap);
                                let repl = self.call_value3(
                                    replacement_val,
                                    match_val,
                                    Value::from_i64(start as i64),
                                    text_val,
                                )?;
                                out.push_str(&val_inspect(repl, &self.heap));
                                cursor = end;
                            }
                            out.push_str(&text[cursor..]);
                            out
                        } else {
                            let replacement_template = replacement_value.inspect();
                            let mut out = String::new();
                            let mut cursor = 0usize;
                            while let Some(rel) = text[cursor..].find(&p) {
                                let start = cursor + rel;
                                let end = start + p.len();
                                out.push_str(&text[cursor..start]);
                                out.push_str(&Self::expand_js_replacement(
                                    &replacement_template,
                                    &p,
                                    &[],
                                    &text[..start],
                                    &text[end..],
                                ));
                                cursor = end;
                            }
                            out.push_str(&text[cursor..]);
                            out
                        }
                    }
                    None => text.to_string(),
                };
                Ok(obj_into_val(Object::String(out.into()), &mut self.heap))
            }
            BuiltinFunction::NumberToFixed => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Number.toFixed missing receiver".to_string())
                })?;
                let n = self.to_number(&receiver)?;
                let digits = args
                    .first()
                    .map(|v| self.to_u32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                Ok(obj_into_val(
                    Object::String(format!("{:.*}", digits as usize, n).into()),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::NumberToString => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Number.toString missing receiver".to_string())
                })?;
                let n = self.to_number(&receiver)?;
                let radix = args
                    .first()
                    .map(|v| self.to_u32_val(*v))
                    .transpose()?
                    .unwrap_or(10);
                if !n.is_finite() {
                    return Ok(obj_into_val(
                        Object::String(if n.is_nan() {
                            "NaN".into()
                        } else if n.is_sign_negative() {
                            "-Infinity".into()
                        } else {
                            "Infinity".into()
                        }),
                        &mut self.heap,
                    ));
                }

                if radix == 10 {
                    if n.fract() == 0.0 {
                        Ok(obj_into_val(
                            Object::String((n as i64).to_string().into()),
                            &mut self.heap,
                        ))
                    } else {
                        Ok(obj_into_val(
                            Object::String(n.to_string().into()),
                            &mut self.heap,
                        ))
                    }
                } else if (2..=36).contains(&radix) {
                    Ok(obj_into_val(
                        Object::String(Self::int_to_radix_string(n as i64, radix).into()),
                        &mut self.heap,
                    ))
                } else {
                    Err(VMError::TypeError(
                        "Number.toString radix must be between 2 and 36".to_string(),
                    ))
                }
            }
            BuiltinFunction::ParseInt => {
                let input = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(String::new);
                let radix_opt = args.get(1).map(|v| self.to_i32_val(*v)).transpose()?;
                let trimmed = input.trim_start();
                let sign = if trimmed.starts_with('-') {
                    -1i64
                } else {
                    1i64
                };
                let mut body = if trimmed.starts_with('-') || trimmed.starts_with('+') {
                    &trimmed[1..]
                } else {
                    trimmed
                };
                let mut base = radix_opt.unwrap_or(0);
                if base == 0 {
                    if body.starts_with("0x") || body.starts_with("0X") {
                        base = 16;
                        body = &body[2..];
                    } else {
                        base = 10;
                    }
                } else if base == 16 && (body.starts_with("0x") || body.starts_with("0X")) {
                    body = &body[2..];
                }
                if !(2..=36).contains(&base) {
                    return Ok(Value::from_f64(f64::NAN));
                }
                let digits: String = body
                    .chars()
                    .take_while(|c| c.is_digit(base as u32))
                    .collect();
                if digits.is_empty() {
                    return Ok(Value::from_f64(f64::NAN));
                }
                match i64::from_str_radix(&digits, base as u32) {
                    Ok(v) => Ok(Value::from_i64(v * sign)),
                    Err(_) => Ok(Value::from_f64(f64::NAN)),
                }
            }
            BuiltinFunction::ParseFloat => {
                let input = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(String::new);
                let trimmed = input.trim_start();
                let mut buf = String::new();
                let mut has_digit = false;
                let mut has_dot = false;
                let mut has_exp = false;
                let mut prev_was_exp = false;
                for ch in trimmed.chars() {
                    if (ch == '+' || ch == '-') && (buf.is_empty() || prev_was_exp) {
                        buf.push(ch);
                        prev_was_exp = false;
                        continue;
                    }
                    if ch.is_ascii_digit() {
                        buf.push(ch);
                        has_digit = true;
                        prev_was_exp = false;
                        continue;
                    }
                    if ch == '.' && !has_dot && !has_exp {
                        buf.push(ch);
                        has_dot = true;
                        prev_was_exp = false;
                        continue;
                    }
                    if (ch == 'e' || ch == 'E') && !has_exp && has_digit {
                        buf.push(ch);
                        has_exp = true;
                        prev_was_exp = true;
                        continue;
                    }
                    break;
                }
                if !has_digit {
                    return Ok(Value::from_f64(f64::NAN));
                }
                match buf.parse::<f64>() {
                    Ok(v) => {
                        if v.fract() == 0.0 {
                            Ok(Value::from_i64(v as i64))
                        } else {
                            Ok(Value::from_f64(v))
                        }
                    }
                    Err(_) => Ok(Value::from_f64(f64::NAN)),
                }
            }
            BuiltinFunction::IsNaN => {
                if args.is_empty() {
                    return Ok(Value::from_bool(true));
                }
                let n = self.to_number_val(args[0]).unwrap_or(f64::NAN);
                Ok(Value::from_bool(n.is_nan()))
            }
            BuiltinFunction::IsFinite => {
                if args.is_empty() {
                    return Ok(Value::from_bool(false));
                }
                let n = self.to_number_val(args[0]).unwrap_or(f64::NAN);
                Ok(Value::from_bool(n.is_finite()))
            }
            BuiltinFunction::NumberIsNaN => {
                let obj = args.first().map(|v| val_to_obj(*v, &self.heap));
                match obj.as_ref() {
                    Some(Object::Float(v)) => Ok(Value::from_bool(v.is_nan())),
                    Some(Object::Integer(_)) => Ok(Value::from_bool(false)),
                    _ => Ok(Value::from_bool(false)),
                }
            }
            BuiltinFunction::NumberIsFinite => {
                let obj = args.first().map(|v| val_to_obj(*v, &self.heap));
                match obj.as_ref() {
                    Some(Object::Integer(_)) => Ok(Value::from_bool(true)),
                    Some(Object::Float(v)) => Ok(Value::from_bool(v.is_finite())),
                    _ => Ok(Value::from_bool(false)),
                }
            }
            BuiltinFunction::NumberIsInteger => {
                let obj = args.first().map(|v| val_to_obj(*v, &self.heap));
                match obj.as_ref() {
                    Some(Object::Integer(_)) => Ok(Value::from_bool(true)),
                    Some(Object::Float(v)) => {
                        Ok(Value::from_bool(v.is_finite() && v.fract() == 0.0))
                    }
                    _ => Ok(Value::from_bool(false)),
                }
            }
            BuiltinFunction::NumberIsSafeInteger => {
                let obj = args.first().map(|v| val_to_obj(*v, &self.heap));
                match obj.as_ref() {
                    Some(Object::Integer(v)) => {
                        Ok(Value::from_bool((*v as f64).abs() <= 9007199254740991.0))
                    }
                    Some(Object::Float(v)) => Ok(Value::from_bool(
                        v.is_finite() && v.fract() == 0.0 && v.abs() <= 9007199254740991.0,
                    )),
                    _ => Ok(Value::from_bool(false)),
                }
            }
            BuiltinFunction::ObjectKeys => {
                let source_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                let source = val_to_obj(source_val, &self.heap);
                Ok(obj_into_val(
                    make_array(self.get_keys_array(source)),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::ObjectValues => {
                let source_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                let source = val_to_obj(source_val, &self.heap);
                let out: Vec<Value> = match source {
                    Object::Hash(hash) => {
                        let hash_b = hash.borrow_mut();
                        hash_b.sync_pairs_if_dirty();
                        self.ordered_hash_keys_js(&hash_b)
                            .into_iter()
                            .filter_map(|k| hash_b.pairs.get(&k).copied())
                            .collect()
                    }
                    Object::String(s) => s
                        .chars()
                        .map(|c| obj_into_val(Object::String(c.to_string().into()), &mut self.heap))
                        .collect(),
                    Object::Array(items) => unwrap_array(items),
                    _ => vec![],
                };
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::ObjectEntries => {
                let source_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                let source = val_to_obj(source_val, &self.heap);
                let mut out: Vec<Value> = vec![];
                match source {
                    Object::Hash(hash) => {
                        let hash_b = hash.borrow_mut();
                        hash_b.sync_pairs_if_dirty();
                        for k in self.ordered_hash_keys_js(&hash_b) {
                            let Some(v) = hash_b.pairs.get(&k).copied() else {
                                continue;
                            };
                            let key_val = obj_into_val(
                                Object::String(k.display_key().into()),
                                &mut self.heap,
                            );
                            let entry = make_array(vec![key_val, v]);
                            out.push(obj_into_val(entry, &mut self.heap));
                        }
                    }
                    Object::Array(items) => {
                        for (i, v) in unwrap_array(items).into_iter().enumerate() {
                            let idx_val =
                                obj_into_val(Object::String(i.to_string().into()), &mut self.heap);
                            let entry = make_array(vec![idx_val, v]);
                            out.push(obj_into_val(entry, &mut self.heap));
                        }
                    }
                    Object::String(s) => {
                        for (i, c) in s.chars().enumerate() {
                            let idx_val =
                                obj_into_val(Object::String(i.to_string().into()), &mut self.heap);
                            let ch_val =
                                obj_into_val(Object::String(c.to_string().into()), &mut self.heap);
                            let entry = make_array(vec![idx_val, ch_val]);
                            out.push(obj_into_val(entry, &mut self.heap));
                        }
                    }
                    _ => {}
                }
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::ObjectFromEntries => {
                let source_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                let source = val_to_obj(source_val, &self.heap);
                let mut out = crate::object::HashObject::default();
                if let Object::Array(items) = source {
                    for item in unwrap_array(items).into_iter() {
                        let item_obj = val_to_obj(item, &self.heap);
                        if let Object::Array(entry) = item_obj {
                            let entry = entry.borrow();
                            if entry.len() < 2 {
                                continue;
                            }
                            let key_obj = val_to_obj(entry[0], &self.heap);
                            let key = self.hash_key_from_object(&key_obj);
                            let val = entry[1];
                            out.insert_pair(key, val);
                        }
                    }
                }
                Ok(obj_into_val(make_hash(out), &mut self.heap))
            }
            BuiltinFunction::ObjectHasOwn => {
                if args.len() < 2 {
                    return Ok(Value::from_bool(false));
                }
                let arg0 = val_to_obj(args[0], &self.heap);
                let arg1 = val_to_obj(args[1], &self.heap);
                let has = match &arg0 {
                    Object::Hash(hash) => {
                        let k = self.hash_key_from_object(&arg1);
                        hash.borrow().pairs.contains_key(&k)
                    }
                    Object::Array(items) => {
                        let items = items.borrow();
                        match &arg1 {
                            Object::String(key_str) => {
                                if &**key_str == "length" {
                                    true
                                } else if let Some(idx) = Self::parse_non_negative_usize(key_str) {
                                    idx < items.len()
                                } else {
                                    false
                                }
                            }
                            other => Self::numeric_array_index(other)
                                .map(|idx| idx < items.len())
                                .unwrap_or(false),
                        }
                    }
                    Object::String(s) => {
                        let s_len = Self::string_char_len(s);
                        match &arg1 {
                            Object::String(key_str) => {
                                if &**key_str == "length" {
                                    true
                                } else if let Some(idx) = Self::parse_non_negative_usize(key_str) {
                                    idx < s_len
                                } else {
                                    false
                                }
                            }
                            other => Self::numeric_array_index(other)
                                .map(|idx| idx < s_len)
                                .unwrap_or(false),
                        }
                    }
                    _ => false,
                };
                Ok(Value::from_bool(has))
            }
            BuiltinFunction::ObjectIs => {
                if args.len() < 2 {
                    return Ok(Value::from_bool(true));
                }
                let a = val_to_obj(args[0], &self.heap);
                let b = val_to_obj(args[1], &self.heap);
                Ok(Value::from_bool(self.same_value(&a, &b)))
            }
            BuiltinFunction::ObjectAssign => {
                if args.is_empty() {
                    return Ok(Value::UNDEFINED);
                }

                let target_val = args[0];

                // Collect source entries first (avoids borrow conflicts)
                let mut source_entries: Vec<Vec<(HashKey, Value)>> = Vec::new();
                for source_val in args.iter().skip(1) {
                    let source = val_to_obj(*source_val, &self.heap);
                    let mut entries = Vec::new();
                    match &source {
                        Object::Hash(hash) => {
                            let hash_b = hash.borrow_mut();
                            hash_b.sync_pairs_if_dirty();
                            for (k, v) in hash_b.pairs.iter() {
                                entries.push((k.clone(), *v));
                            }
                        }
                        Object::Array(items) => {
                            for (i, v) in items.borrow().iter().enumerate() {
                                entries.push((HashKey::from_string(&i.to_string()), *v));
                            }
                        }
                        Object::String(s) => {
                            for (i, ch) in s.chars().enumerate() {
                                let val = obj_into_val(
                                    Object::String(ch.to_string().into()),
                                    &mut self.heap,
                                );
                                entries.push((HashKey::from_string(&i.to_string()), val));
                            }
                        }
                        _ => {}
                    }
                    source_entries.push(entries);
                }

                // Mutate target in-place (or create new if not a hash)
                if target_val.is_heap() {
                    let heap_obj = unsafe {
                        &*self
                            .heap
                            .objects
                            .as_ptr()
                            .add(target_val.heap_index() as usize)
                    };
                    if let Object::Hash(hash_rc) = heap_obj {
                        let target = hash_rc.borrow_mut();
                        for entries in source_entries {
                            for (k, v) in entries {
                                target.insert_pair(k, v);
                            }
                        }
                        return Ok(target_val);
                    }
                }

                // Fallback: create new hash
                let mut target = crate::object::HashObject::default();
                for entries in source_entries {
                    for (k, v) in entries {
                        target.insert_pair(k, v);
                    }
                }
                Ok(obj_into_val(make_hash(target), &mut self.heap))
            }
            BuiltinFunction::ObjectFreeze => {
                let target_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                if target_val.is_heap() {
                    let heap_obj = unsafe {
                        &*self
                            .heap
                            .objects
                            .as_ptr()
                            .add(target_val.heap_index() as usize)
                    };
                    if let Object::Hash(hash_rc) = heap_obj {
                        hash_rc.borrow_mut().frozen = true;
                    }
                }
                Ok(target_val)
            }
            BuiltinFunction::ObjectCreate => {
                let proto_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                if proto_val.is_null() {
                    return Ok(obj_into_val(make_hash(HashObject::default()), &mut self.heap));
                }
                if proto_val.is_heap() {
                    let heap_idx = proto_val.heap_index() as usize;
                    let proto_obj = unsafe { &*self.heap.objects.as_ptr().add(heap_idx) };
                    if let Object::Hash(h) = proto_obj {
                        let mut new_hash = HashObject::default();
                        let h = h.borrow();
                        for (key, &val) in h.pairs.iter() {
                            new_hash.pairs.insert(key.clone(), val);
                        }
                        new_hash.values = h.values.clone();
                        new_hash.str_slots = h.str_slots.clone();
                        if let Some(ref getters) = h.getters {
                            new_hash.getters = Some(getters.clone());
                        }
                        if let Some(ref setters) = h.setters {
                            new_hash.setters = Some(setters.clone());
                        }
                        return Ok(obj_into_val(make_hash(new_hash), &mut self.heap));
                    }
                }
                Ok(obj_into_val(make_hash(HashObject::default()), &mut self.heap))
            }
            BuiltinFunction::ArrayOf => {
                Ok(obj_into_val(make_array(args.to_vec()), &mut self.heap))
            }
            BuiltinFunction::HashHasOwnProperty => {
                let key_str = args
                    .first()
                    .map(|v| {
                        let obj = val_to_obj(*v, &self.heap);
                        match obj {
                            Object::String(s) => s.to_string(),
                            _ => obj.inspect(),
                        }
                    })
                    .unwrap_or_default();
                if let Some(Object::Hash(h)) = &builtin.receiver {
                    let has = h.borrow().contains_str(&key_str);
                    Ok(Value::from_bool(has))
                } else {
                    Ok(Value::FALSE)
                }
            }
            BuiltinFunction::JsonStringify => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let value_obj = val_to_obj(value, &self.heap);
                let json = self.object_to_json_value(&value_obj);
                Ok(obj_into_val(
                    Object::String(json.to_string().into()),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::JsonParse => {
                let source = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(String::new);
                let parsed = self.json_parse(&source)?;
                Ok(obj_into_val(parsed, &mut self.heap))
            }
            BuiltinFunction::SymbolCtor => {
                static NEXT_SYMBOL_ID: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(100); // IDs < 100 reserved for well-known symbols
                let id = NEXT_SYMBOL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let desc = args.first().and_then(|v| {
                    if v.is_undefined() {
                        None
                    } else {
                        let obj = val_to_obj(*v, &self.heap);
                        Some(Rc::from(obj.to_js_string().as_str()))
                    }
                });
                let sym = Object::Symbol(id, desc);
                Ok(obj_into_val(sym, &mut self.heap))
            }
            BuiltinFunction::PromiseResolve => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let value_obj = val_to_obj(value, &self.heap);
                let promise = Object::Promise(Box::new(PromiseObject {
                    settled: PromiseState::Fulfilled(Box::new(value_obj)),
                }));
                Ok(obj_into_val(promise, &mut self.heap))
            }
            BuiltinFunction::PromiseReject => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let value_obj = val_to_obj(value, &self.heap);
                let promise = Object::Promise(Box::new(PromiseObject {
                    settled: PromiseState::Rejected(Box::new(value_obj)),
                }));
                Ok(obj_into_val(promise, &mut self.heap))
            }
            BuiltinFunction::ArrayPop => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.pop missing receiver".to_string()))?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                Ok(items.last().copied().unwrap_or(Value::UNDEFINED))
            }
            BuiltinFunction::ArrayPush => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.push missing receiver".to_string()))?;
                let mut items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                for arg in args {
                    items.push(*arg);
                }
                if items.len() > MAX_ARRAY_SIZE {
                    return Err(VMError::TypeError("Array size limit exceeded".to_string()));
                }
                Ok(Value::from_i64(items.len() as i64))
            }
            BuiltinFunction::ArrayAt => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.at missing receiver".to_string()))?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                let idx = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                let real = if idx < 0 {
                    items.len() as i32 + idx
                } else {
                    idx
                };
                if real < 0 || real as usize >= items.len() {
                    Ok(Value::UNDEFINED)
                } else {
                    Ok(items[real as usize])
                }
            }
            BuiltinFunction::ArrayToSorted => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.toSorted missing receiver".to_string())
                })?;
                let mut items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                items.sort_by_key(|v| val_inspect(*v, &self.heap));
                Ok(obj_into_val(make_array(items), &mut self.heap))
            }
            BuiltinFunction::ArrayWith => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.with missing receiver".to_string()))?;
                let mut items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                if args.len() < 2 {
                    return Ok(obj_into_val(make_array(items), &mut self.heap));
                }
                let idx = self.to_i32_val(args[0])?;
                if idx < 0 || idx as usize >= items.len() {
                    return Ok(obj_into_val(make_array(items), &mut self.heap));
                }
                items[idx as usize] = args[1];
                Ok(obj_into_val(make_array(items), &mut self.heap))
            }
            BuiltinFunction::ArrayMap => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.map missing receiver".to_string()))?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                let callback = args
                    .first()
                    .cloned()
                    .ok_or_else(|| VMError::TypeError("Array.map requires callback".to_string()))?;
                let source_for_cb = if Self::callback_max_used_args_val(callback, &self.heap) >= 3 {
                    Some(obj_into_val(make_array(items.clone()), &mut self.heap))
                } else {
                    None
                };
                let mut out: Vec<Value> = Vec::with_capacity(items.len());
                for (i, item) in items.into_iter().enumerate() {
                    let mapped = if let Some(src) = source_for_cb {
                        self.call_value3(callback, item, Value::from_i64(i as i64), src)?
                    } else {
                        self.call_value2(callback, item, Value::from_i64(i as i64))?
                    };
                    out.push(mapped);
                }
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::ArrayForEach => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.forEach missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.forEach requires callback".to_string())
                })?;
                let source_for_cb = if Self::callback_max_used_args_val(callback, &self.heap) >= 3 {
                    Some(obj_into_val(make_array(items.clone()), &mut self.heap))
                } else {
                    None
                };
                for (i, item) in items.into_iter().enumerate() {
                    if let Some(src) = source_for_cb {
                        let _ = self.call_value3(callback, item, Value::from_i64(i as i64), src)?;
                    } else {
                        let _ = self.call_value2(callback, item, Value::from_i64(i as i64))?;
                    }
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::ArrayFlatMap => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.flatMap missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.flatMap requires callback".to_string())
                })?;
                let source_for_cb = if Self::callback_max_used_args_val(callback, &self.heap) >= 3 {
                    Some(obj_into_val(make_array(items.clone()), &mut self.heap))
                } else {
                    None
                };
                let mut out: Vec<Value> = vec![];
                for (i, item) in items.into_iter().enumerate() {
                    let mapped = if let Some(src) = source_for_cb {
                        self.call_value3(callback, item, Value::from_i64(i as i64), src)?
                    } else {
                        self.call_value2(callback, item, Value::from_i64(i as i64))?
                    };
                    let mapped_obj = val_to_obj(mapped, &self.heap);
                    match mapped_obj {
                        Object::Array(inner) => out.extend(unwrap_array(inner)),
                        _ => out.push(mapped),
                    }
                }
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::ArrayFlat => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.flat missing receiver".to_string()))?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let depth = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(1)
                    .max(0);

                fn flatten(items: Vec<Value>, depth: i32, heap: &Heap) -> Vec<Value> {
                    if depth == 0 {
                        return items;
                    }
                    let mut out = vec![];
                    for item in items {
                        let obj = val_to_obj(item, heap);
                        match obj {
                            Object::Array(inner) => {
                                out.extend(flatten(unwrap_array(inner), depth - 1, heap))
                            }
                            _ => out.push(item),
                        }
                    }
                    out
                }

                Ok(obj_into_val(
                    make_array(flatten(items, depth, &self.heap)),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::ArrayReverse => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.reverse missing receiver".to_string())
                })?;
                let mut items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                items.reverse();
                Ok(obj_into_val(make_array(items), &mut self.heap))
            }
            BuiltinFunction::ArraySort => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.sort missing receiver".to_string()))?;
                let mut items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };

                if let Some(compare_fn) = args.first().copied() {
                    items.sort_by(|a, b| {
                        let out = self.call_value2(compare_fn, *a, *b);
                        match out {
                            Ok(v) => {
                                let n = self.to_number_val(v).unwrap_or(0.0);
                                if n < 0.0 {
                                    std::cmp::Ordering::Less
                                } else if n > 0.0 {
                                    std::cmp::Ordering::Greater
                                } else {
                                    std::cmp::Ordering::Equal
                                }
                            }
                            Err(_) => std::cmp::Ordering::Equal,
                        }
                    });
                } else {
                    items.sort_by_key(|v| val_inspect(*v, &self.heap));
                }

                Ok(obj_into_val(make_array(items), &mut self.heap))
            }
            BuiltinFunction::ArrayFilter => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.filter missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.filter requires callback".to_string())
                })?;
                let source_for_cb = if Self::callback_max_used_args_val(callback, &self.heap) >= 3 {
                    Some(obj_into_val(make_array(items.clone()), &mut self.heap))
                } else {
                    None
                };
                let mut out: Vec<Value> = vec![];
                for (i, item) in items.into_iter().enumerate() {
                    let keep = if let Some(ref src) = source_for_cb {
                        self.call_value3(callback, item, Value::from_i64(i as i64), *src)?
                    } else {
                        self.call_value2(callback, item, Value::from_i64(i as i64))?
                    };
                    if self.is_truthy(&val_to_obj(keep, &self.heap)) {
                        out.push(item);
                    }
                }
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::ArraySome => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.some missing receiver".to_string()))?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::from_bool(false)),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.some requires callback".to_string())
                })?;
                let source_for_cb = if Self::callback_max_used_args_val(callback, &self.heap) >= 3 {
                    Some(obj_into_val(make_array(items.clone()), &mut self.heap))
                } else {
                    None
                };
                for (i, item) in items.into_iter().enumerate() {
                    let ok = if let Some(ref src) = source_for_cb {
                        self.call_value3(callback, item, Value::from_i64(i as i64), *src)?
                    } else {
                        self.call_value2(callback, item, Value::from_i64(i as i64))?
                    };
                    if self.is_truthy(&val_to_obj(ok, &self.heap)) {
                        return Ok(Value::from_bool(true));
                    }
                }
                Ok(Value::from_bool(false))
            }
            BuiltinFunction::ArrayEvery => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.every missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::from_bool(false)),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.every requires callback".to_string())
                })?;
                let source_for_cb = if Self::callback_max_used_args_val(callback, &self.heap) >= 3 {
                    Some(obj_into_val(make_array(items.clone()), &mut self.heap))
                } else {
                    None
                };
                for (i, item) in items.into_iter().enumerate() {
                    let ok = if let Some(ref src) = source_for_cb {
                        self.call_value3(callback, item, Value::from_i64(i as i64), *src)?
                    } else {
                        self.call_value2(callback, item, Value::from_i64(i as i64))?
                    };
                    if !self.is_truthy(&val_to_obj(ok, &self.heap)) {
                        return Ok(Value::from_bool(false));
                    }
                }
                Ok(Value::from_bool(true))
            }
            BuiltinFunction::ArrayFind => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.find missing receiver".to_string()))?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.find requires callback".to_string())
                })?;
                let source_for_cb = if Self::callback_max_used_args_val(callback, &self.heap) >= 3 {
                    Some(obj_into_val(make_array(items.clone()), &mut self.heap))
                } else {
                    None
                };
                for (i, item) in items.into_iter().enumerate() {
                    let found = if let Some(ref src) = source_for_cb {
                        self.call_value3(callback, item, Value::from_i64(i as i64), *src)?
                    } else {
                        self.call_value2(callback, item, Value::from_i64(i as i64))?
                    };
                    if self.is_truthy(&val_to_obj(found, &self.heap)) {
                        return Ok(item);
                    }
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::ArrayFindIndex => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.findIndex missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::from_i64(-1)),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.findIndex requires callback".to_string())
                })?;
                let source_for_cb = if Self::callback_max_used_args_val(callback, &self.heap) >= 3 {
                    Some(obj_into_val(make_array(items.clone()), &mut self.heap))
                } else {
                    None
                };
                for (i, item) in items.into_iter().enumerate() {
                    let found = if let Some(ref src) = source_for_cb {
                        self.call_value3(callback, item, Value::from_i64(i as i64), *src)?
                    } else {
                        self.call_value2(callback, item, Value::from_i64(i as i64))?
                    };
                    if self.is_truthy(&val_to_obj(found, &self.heap)) {
                        return Ok(Value::from_i64(i as i64));
                    }
                }
                Ok(Value::from_i64(-1))
            }
            BuiltinFunction::ArrayFindLast => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.findLast missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.findLast requires callback".to_string())
                })?;
                for i in (0..items.len()).rev() {
                    let item = items[i];
                    let found = self.call_value2(callback, item, Value::from_i64(i as i64))?;
                    if self.is_truthy(&val_to_obj(found, &self.heap)) {
                        return Ok(item);
                    }
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::ArrayFindLastIndex => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.findLastIndex missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::from_i64(-1)),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.findLastIndex requires callback".to_string())
                })?;
                for i in (0..items.len()).rev() {
                    let item = items[i];
                    let found = self.call_value2(callback, item, Value::from_i64(i as i64))?;
                    if self.is_truthy(&val_to_obj(found, &self.heap)) {
                        return Ok(Value::from_i64(i as i64));
                    }
                }
                Ok(Value::from_i64(-1))
            }
            BuiltinFunction::ArrayToReversed => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.toReversed missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                let mut reversed = items;
                reversed.reverse();
                Ok(obj_into_val(make_array(reversed), &mut self.heap))
            }
            BuiltinFunction::ArrayIncludes => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.includes missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::from_bool(false)),
                };
                let needle = args.first().copied().unwrap_or(Value::UNDEFINED);
                let needle_obj = val_to_obj(needle, &self.heap);
                let has = items.iter().any(|item| {
                    let item_obj = val_to_obj(*item, &self.heap);
                    Self::same_value_zero(&item_obj, &needle_obj)
                });
                Ok(Value::from_bool(has))
            }
            BuiltinFunction::ArrayIndexOf => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.indexOf missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::from_i64(-1)),
                };
                let needle = args.first().copied().unwrap_or(Value::UNDEFINED);
                let needle_obj = val_to_obj(needle, &self.heap);
                let mut from = args
                    .get(1)
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                if from < 0 {
                    from = (items.len() as i32 + from).max(0);
                }
                for (i, item) in items.iter().enumerate().skip(from as usize) {
                    let item_obj = val_to_obj(*item, &self.heap);
                    if Self::strict_equal(&item_obj, &needle_obj) {
                        return Ok(Value::from_i64(i as i64));
                    }
                }
                Ok(Value::from_i64(-1))
            }
            BuiltinFunction::ArrayLastIndexOf => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.lastIndexOf missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::from_i64(-1)),
                };
                let needle = args.first().copied().unwrap_or(Value::UNDEFINED);
                let needle_obj = val_to_obj(needle, &self.heap);
                let mut from = args
                    .get(1)
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(items.len() as i32 - 1);
                if from < 0 {
                    from = items.len() as i32 + from;
                }
                if items.is_empty() {
                    return Ok(Value::from_i64(-1));
                }
                let from = from.clamp(0, items.len() as i32 - 1) as usize;
                for i in (0..=from).rev() {
                    let item_obj = val_to_obj(items[i], &self.heap);
                    if Self::strict_equal(&item_obj, &needle_obj) {
                        return Ok(Value::from_i64(i as i64));
                    }
                }
                Ok(Value::from_i64(-1))
            }
            BuiltinFunction::ArrayJoin => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.join missing receiver".to_string()))?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                let sep = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(|| ",".to_string());
                let parts: Vec<String> = items
                    .iter()
                    .map(|v| {
                        let obj = val_to_obj(*v, &self.heap);
                        match &obj {
                            Object::Undefined | Object::Null => String::new(),
                            Object::Array(nested) => {
                                self.array_to_js_string(&nested.borrow())
                            }
                            Object::Hash(_) => "[object Object]".to_string(),
                            _ => obj.inspect(),
                        }
                    })
                    .collect();
                Ok(obj_into_val(
                    Object::String(parts.join(&sep).into()),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::ArrayToString => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.toString missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                Ok(obj_into_val(
                    Object::String(self.array_to_js_string(&items[..]).into()),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::ArrayValueOf => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.valueOf missing receiver".to_string())
                })?;
                match receiver {
                    Object::Array(items) => Ok(obj_into_val(Object::Array(items), &mut self.heap)),
                    _ => Ok(Value::UNDEFINED),
                }
            }
            BuiltinFunction::ArraySlice => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.slice missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let len = items.len() as i32;
                let start = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0);
                let end = args
                    .get(1)
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(len);
                let (sidx, eidx) = Self::slice_bounds(start, end, len);
                Ok(obj_into_val(
                    make_array(items[sidx as usize..eidx as usize].to_vec()),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::ArrayReduce => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.reduce missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.reduce requires callback".to_string())
                })?;
                let source_for_cb = if Self::callback_max_used_args_val(callback, &self.heap) >= 4 {
                    Some(obj_into_val(make_array(items.clone()), &mut self.heap))
                } else {
                    None
                };

                if items.is_empty() && args.get(1).is_none() {
                    return Err(VMError::TypeError(
                        "Reduce of empty array with no initial value".to_string(),
                    ));
                }

                let mut idx = 0usize;
                let mut acc: Value = if let Some(init) = args.get(1) {
                    *init
                } else {
                    idx = 1;
                    items[0]
                };

                while idx < items.len() {
                    acc = if let Some(src) = source_for_cb {
                        self.call_value4(
                            callback,
                            acc,
                            items[idx],
                            Value::from_i64(idx as i64),
                            src,
                        )?
                    } else {
                        self.call_value3(callback, acc, items[idx], Value::from_i64(idx as i64))?
                    };
                    idx += 1;
                }

                Ok(acc)
            }
            BuiltinFunction::ArrayReduceRight => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.reduceRight missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Array.reduceRight requires callback".to_string())
                })?;

                if items.is_empty() && args.get(1).is_none() {
                    return Err(VMError::TypeError(
                        "Reduce of empty array with no initial value".to_string(),
                    ));
                }

                let len = items.len();
                let mut idx = len.wrapping_sub(1);
                let mut acc: Value = if let Some(init) = args.get(1) {
                    *init
                } else {
                    idx = len.wrapping_sub(2);
                    items[len - 1]
                };

                loop {
                    if idx >= len {
                        break;
                    }
                    acc =
                        self.call_value3(callback, acc, items[idx], Value::from_i64(idx as i64))?;
                    idx = idx.wrapping_sub(1);
                }

                Ok(acc)
            }
            BuiltinFunction::ArrayFrom => {
                let source_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                let source = val_to_obj(source_val, &self.heap);
                let mut out: Vec<Object> = match source {
                    Object::Array(items) => unwrap_array(items)
                        .into_iter()
                        .map(|v| val_to_obj(v, &self.heap))
                        .collect(),
                    Object::String(s) => s
                        .chars()
                        .map(|c| Object::String(c.to_string().into()))
                        .collect(),
                    Object::Hash(hash) => {
                        // Check for Symbol.iterator protocol (@@sym:1)
                        let iter_fn_opt = {
                            let hash_b = hash.borrow_mut();
                            hash_b.sync_pairs_if_dirty();
                            let sym_iter_key = HashKey::Other("@@sym:1".to_string());
                            hash_b.pairs.get(&sym_iter_key).copied()
                        };
                        if let Some(iter_fn_val) = iter_fn_opt {
                            // Call [Symbol.iterator]() to get the iterator object
                            let iterator_val =
                                self.call_value_slice(iter_fn_val, &[source_val])?;
                            // Iterate: call .next() until done
                            let mut items = Vec::new();
                            loop {
                                let next_sym = crate::intern::intern("next");
                                let next_fn =
                                    self.get_property_val(iterator_val, next_sym, 0)?;
                                let result =
                                    self.call_value_slice(next_fn, &[iterator_val])?;
                                let result_obj = val_to_obj(result, &self.heap);
                                match result_obj {
                                    Object::Hash(h) => {
                                        let hb = h.borrow();
                                        let done = hb
                                            .get_by_str("done")
                                            .map(|v| {
                                                let obj = val_to_obj(v, &self.heap);
                                                self.is_truthy(&obj)
                                            })
                                            .unwrap_or(false);
                                        if done {
                                            break;
                                        }
                                        let value = hb
                                            .get_by_str("value")
                                            .map(|v| val_to_obj(v, &self.heap))
                                            .unwrap_or_else(undefined_object);
                                        items.push(value);
                                    }
                                    _ => break,
                                }
                                if items.len() > MAX_ARRAY_SIZE {
                                    return Err(VMError::TypeError(
                                        "Array.from: iterator too large"
                                            .to_string(),
                                    ));
                                }
                            }
                            items
                        } else {
                            let hash_b = hash.borrow_mut();
                            if let Some(length_val) = hash_b.get_by_str("length") {
                                let length_obj = val_to_obj(length_val, &self.heap);
                                let len = self.to_u32(&length_obj).unwrap_or(0) as usize;
                                let mut arr = Vec::with_capacity(len);
                                for i in 0..len {
                                    let key = HashKey::from_string(&i.to_string());
                                    arr.push(
                                        hash_b
                                            .pairs
                                            .get(&key)
                                            .map(|v| val_to_obj(*v, &self.heap))
                                            .unwrap_or_else(undefined_object),
                                    );
                                }
                                arr
                            } else {
                                vec![]
                            }
                        }
                    }
                    Object::Set(set_obj) => set_obj
                        .entries
                        .borrow()
                        .iter()
                        .map(|k| self.object_from_hash_key(k))
                        .collect(),
                    Object::Map(map_obj) => map_obj
                        .entries
                        .borrow()
                        .iter()
                        .map(|(k, v)| {
                            let key_val =
                                obj_into_val(self.object_from_hash_key(k), &mut self.heap);
                            make_array(vec![key_val, *v])
                        })
                        .collect(),
                    Object::Generator(gen_rc) => {
                        // Iterate generator by calling .next() until done
                        let mut items = Vec::new();
                        loop {
                            let result = self.execute_generator_next(&gen_rc, Value::UNDEFINED)?;
                            let result_obj = val_to_obj(result, &self.heap);
                            match result_obj {
                                Object::Hash(h) => {
                                    let hb = h.borrow();
                                    let done = hb.get_by_str("done")
                                        .map(|v| {
                                            let obj = val_to_obj(v, &self.heap);
                                            self.is_truthy(&obj)
                                        })
                                        .unwrap_or(false);
                                    if done {
                                        break;
                                    }
                                    let value = hb.get_by_str("value")
                                        .map(|v| val_to_obj(v, &self.heap))
                                        .unwrap_or_else(undefined_object);
                                    items.push(value);
                                }
                                _ => break,
                            }
                        }
                        items
                    }
                    _ => vec![],
                };

                if let Some(callback) = args.get(1) {
                    let cb = *callback;
                    let source_arr_vals: Vec<Value> =
                        out.iter().map(|o| obj_to_val(o, &mut self.heap)).collect();
                    let source_arr = obj_into_val(make_array(source_arr_vals), &mut self.heap);
                    let mut mapped = Vec::with_capacity(out.len());
                    for (i, item) in out.into_iter().enumerate() {
                        let item_val = obj_into_val(item, &mut self.heap);
                        let value =
                            self.call_value3(cb, item_val, Value::from_i64(i as i64), source_arr)?;
                        mapped.push(val_to_obj(value, &self.heap));
                    }
                    out = mapped;
                }

                let result: Vec<Value> = out
                    .into_iter()
                    .map(|o| obj_into_val(o, &mut self.heap))
                    .collect();
                Ok(obj_into_val(make_array(result), &mut self.heap))
            }
            BuiltinFunction::ArrayIsArray => {
                let val = args.first().copied().unwrap_or(Value::UNDEFINED);
                let is_arr = if val.is_heap() {
                    matches!(
                        self.heap.objects.get(val.heap_index() as usize),
                        Some(Object::Array(_))
                    )
                } else {
                    false
                };
                Ok(Value::from_bool(is_arr))
            }
            BuiltinFunction::ArrayFill => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("fill requires receiver".to_string()))?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Err(VMError::TypeError("fill called on non-array".to_string())),
                };
                let fill_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                let len = items.len() as i64;
                let start = if let Some(v) = args.get(1) {
                    let s = self.to_i32_val(*v)? as i64;
                    if s < 0 { (len + s).max(0) as usize } else { (s as usize).min(len as usize) }
                } else {
                    0
                };
                let end = if let Some(v) = args.get(2) {
                    let e = self.to_i32_val(*v)? as i64;
                    if e < 0 { (len + e).max(0) as usize } else { (e as usize).min(len as usize) }
                } else {
                    len as usize
                };
                let mut new_items = items;
                for i in start..end {
                    new_items[i] = fill_val;
                }
                Ok(obj_into_val(make_array(new_items), &mut self.heap))
            }
            BuiltinFunction::ArrayCopyWithin => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("copyWithin requires receiver".to_string()))?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => {
                        return Err(VMError::TypeError(
                            "copyWithin called on non-array".to_string(),
                        ))
                    }
                };
                let len = items.len() as i64;
                let target = {
                    let t = self.to_i32_val(args.first().copied().unwrap_or(Value::UNDEFINED))? as i64;
                    if t < 0 { (len + t).max(0) as usize } else { (t as usize).min(len as usize) }
                };
                let start = if let Some(v) = args.get(1) {
                    let s = self.to_i32_val(*v)? as i64;
                    if s < 0 { (len + s).max(0) as usize } else { (s as usize).min(len as usize) }
                } else {
                    0
                };
                let end = if let Some(v) = args.get(2) {
                    let e = self.to_i32_val(*v)? as i64;
                    if e < 0 { (len + e).max(0) as usize } else { (e as usize).min(len as usize) }
                } else {
                    len as usize
                };
                // Copy elements from [start..end) to [target..)
                let count = (end - start).min(len as usize - target);
                let mut new_items = items;
                // Use a temporary buffer to handle overlapping regions
                let source: Vec<Value> = new_items[start..start + count].to_vec();
                for (i, val) in source.into_iter().enumerate() {
                    new_items[target + i] = val;
                }
                Ok(obj_into_val(make_array(new_items), &mut self.heap))
            }
            BuiltinFunction::ArrayKeys => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Array.keys missing receiver".to_string()))?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let out: Vec<Value> = (0..items.len())
                    .map(|i| Value::from_i64(i as i64))
                    .collect();
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::ArrayValues => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.values missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => items,
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                Ok(obj_into_val(Object::Array(items), &mut self.heap))
            }
            BuiltinFunction::ArrayEntries => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.entries missing receiver".to_string())
                })?;
                let items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let out: Vec<Value> = items
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let entry = make_array(vec![Value::from_i64(i as i64), v]);
                        obj_into_val(entry, &mut self.heap)
                    })
                    .collect();
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::ArrayShift => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.shift missing receiver".to_string())
                })?;
                let mut items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                if items.is_empty() {
                    Ok(Value::UNDEFINED)
                } else {
                    let first = items.remove(0);
                    Ok(first)
                }
            }
            BuiltinFunction::ArrayUnshift => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.unshift missing receiver".to_string())
                })?;
                let mut items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                for (i, arg) in args.iter().enumerate() {
                    items.insert(i, *arg);
                }
                if items.len() > MAX_ARRAY_SIZE {
                    return Err(VMError::TypeError("Array size limit exceeded".to_string()));
                }
                Ok(Value::from_i64(items.len() as i64))
            }
            BuiltinFunction::ArraySplice => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.splice missing receiver".to_string())
                })?;
                let mut items = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                let len = items.len() as i64;
                let start_raw = args
                    .first()
                    .map(|v| self.to_i32_val(*v))
                    .transpose()?
                    .unwrap_or(0) as i64;
                let start = if start_raw < 0 {
                    (len + start_raw).max(0) as usize
                } else {
                    (start_raw as usize).min(items.len())
                };
                let delete_count = if args.len() >= 2 {
                    self.to_i32_val(args[1])?.max(0) as usize
                } else {
                    items.len() - start
                };
                let delete_count = delete_count.min(items.len() - start);
                let removed: Vec<Value> = items.drain(start..start + delete_count).collect();
                // Insert new items
                for (i, arg) in args[2..].iter().enumerate() {
                    items.insert(start + i, *arg);
                }
                Ok(obj_into_val(make_array(removed), &mut self.heap))
            }
            BuiltinFunction::ArrayConcat => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Array.concat missing receiver".to_string())
                })?;
                let mut result = match receiver {
                    Object::Array(items) => unwrap_array(items),
                    _ => return Ok(Value::UNDEFINED),
                };
                for arg in args {
                    let obj = val_to_obj(*arg, &self.heap);
                    match obj {
                        Object::Array(items) => result.extend(unwrap_array(items)),
                        _ => result.push(*arg),
                    }
                }
                Ok(obj_into_val(make_array(result), &mut self.heap))
            }
            BuiltinFunction::RegExpCtor => {
                let arg0 = args.first().map(|v| val_to_obj(*v, &self.heap));
                let (pattern, inferred_flags) = match arg0.as_ref() {
                    Some(Object::RegExp(re)) => (re.pattern.clone(), re.flags.clone()),
                    Some(v) => (v.inspect(), String::new()),
                    None => (String::new(), String::new()),
                };
                let flags = args
                    .get(1)
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or(inferred_flags);
                let regexp =
                    Object::RegExp(Box::new(crate::object::RegExpObject { pattern, flags }));
                Ok(obj_into_val(regexp, &mut self.heap))
            }
            BuiltinFunction::RegExpTest => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("RegExp.test missing receiver".to_string())
                })?;
                let re = match receiver {
                    Object::RegExp(re) => *re,
                    _ => return Ok(Value::from_bool(false)),
                };
                let text = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(String::new);
                let regex = self.build_regex(&re.pattern, &re.flags)?;
                Ok(Value::from_bool(regex.is_match(&text)))
            }
            BuiltinFunction::RegExpExec => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("RegExp.exec missing receiver".to_string())
                })?;
                let re = match receiver {
                    Object::RegExp(re) => *re,
                    _ => return Ok(Value::NULL),
                };
                let text = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_else(String::new);
                let regex = self.build_regex(&re.pattern, &re.flags)?;
                if let Some(captures) = regex.captures(&text) {
                    let mut result = Vec::new();
                    for i in 0..captures.len() {
                        if let Some(m) = captures.get(i) {
                            result.push(obj_into_val(
                                Object::String(m.as_str().to_string().into()),
                                &mut self.heap,
                            ));
                        } else {
                            result.push(Value::UNDEFINED);
                        }
                    }
                    Ok(obj_into_val(make_array(result), &mut self.heap))
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::StringMatch => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.match missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(Value::NULL),
                };
                let arg0 = args.first().map(|v| val_to_obj(*v, &self.heap));
                let (pattern, flags, is_regex_input) = match arg0.as_ref() {
                    Some(Object::RegExp(re)) => (re.pattern.clone(), re.flags.clone(), true),
                    Some(other) => (other.inspect(), String::new(), false),
                    None => (String::new(), String::new(), false),
                };
                if pattern.is_empty() {
                    let empty_str_val = obj_into_val(Object::String(Rc::from("")), &mut self.heap);
                    return Ok(obj_into_val(
                        make_array(vec![empty_str_val]),
                        &mut self.heap,
                    ));
                }

                let regex = self.build_regex(&pattern, &flags)?;
                if is_regex_input && flags.contains('g') {
                    let matches: Vec<Value> = regex
                        .find_iter(&text)
                        .map(|m| {
                            obj_into_val(
                                Object::String(m.as_str().to_string().into()),
                                &mut self.heap,
                            )
                        })
                        .collect();
                    if matches.is_empty() {
                        Ok(Value::NULL)
                    } else {
                        Ok(obj_into_val(make_array(matches), &mut self.heap))
                    }
                } else if let Some(captures) = regex.captures(&text) {
                    let mut result = Vec::new();
                    for i in 0..captures.len() {
                        if let Some(m) = captures.get(i) {
                            result.push(obj_into_val(
                                Object::String(m.as_str().to_string().into()),
                                &mut self.heap,
                            ));
                        } else {
                            result.push(Value::UNDEFINED);
                        }
                    }
                    Ok(obj_into_val(make_array(result), &mut self.heap))
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::StringMatchAll => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.matchAll missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };

                let arg0 = args.first().map(|v| val_to_obj(*v, &self.heap));
                let (pattern, flags) = match arg0.as_ref() {
                    Some(Object::RegExp(re)) => {
                        if !re.flags.contains('g') {
                            return Err(VMError::TypeError(
                                "String.prototype.matchAll called with a non-global RegExp"
                                    .to_string(),
                            ));
                        }
                        (re.pattern.clone(), re.flags.clone())
                    }
                    Some(other) => (regex::escape(&other.inspect()), "g".to_string()),
                    None => (String::new(), "g".to_string()),
                };

                let regex = self.build_regex(&pattern, &flags)?;
                let mut out: Vec<Value> = Vec::new();
                for caps in regex.captures_iter(&text) {
                    let mut m: Vec<Value> = Vec::new();
                    for i in 0..caps.len() {
                        match caps.get(i) {
                            Some(g) => m.push(obj_into_val(
                                Object::String(g.as_str().to_string().into()),
                                &mut self.heap,
                            )),
                            None => m.push(Value::UNDEFINED),
                        }
                    }
                    out.push(obj_into_val(make_array(m), &mut self.heap));
                }
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::StringSearch => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.search missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(Value::from_i64(-1)),
                };
                let arg0 = args.first().map(|v| val_to_obj(*v, &self.heap));
                let (pattern, flags) = match arg0.as_ref() {
                    Some(Object::RegExp(re)) => (re.pattern.clone(), re.flags.clone()),
                    Some(other) => (other.inspect(), String::new()),
                    None => (String::new(), String::new()),
                };
                let regex = self.build_regex(&pattern, &flags)?;
                if let Some(m) = regex.find(&text) {
                    Ok(Value::from_i64(m.start() as i64))
                } else {
                    Ok(Value::from_i64(-1))
                }
            }
            BuiltinFunction::StringConcat => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.concat missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                let mut result = text.to_string();
                for arg in args {
                    let s = val_inspect(*arg, &self.heap);
                    result.push_str(&s);
                }
                Ok(obj_into_val(
                    Object::String(Rc::from(result.as_str())),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::StringTrimStart => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.trimStart missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                Ok(obj_into_val(
                    Object::String(Rc::from(text.trim_start())),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::StringTrimEnd => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("String.trimEnd missing receiver".to_string())
                })?;
                let text = match receiver {
                    Object::String(s) => s,
                    _ => return Ok(obj_into_val(Object::String(Rc::from("")), &mut self.heap)),
                };
                Ok(obj_into_val(
                    Object::String(Rc::from(text.trim_end())),
                    &mut self.heap,
                ))
            }
            BuiltinFunction::StringFromCodePoint => {
                let mut out = String::new();
                for arg in args {
                    let code = self.to_u32_val(*arg)?;
                    let ch = char::from_u32(code).ok_or_else(|| {
                        VMError::TypeError(format!(
                            "Invalid code point: {}",
                            code
                        ))
                    })?;
                    out.push(ch);
                }
                Ok(obj_into_val(Object::String(out.into()), &mut self.heap))
            }
            BuiltinFunction::MapCtor => {
                let map_obj = crate::object::MapObject::default();
                if let Some(source_val) = args.first() {
                    let source = val_to_obj(*source_val, &self.heap);
                    match &source {
                        Object::Array(entries) => {
                            let mut target_entries = map_obj.entries.borrow_mut();
                            let mut target_indices = map_obj.indices.borrow_mut();
                            for entry in entries.borrow().iter() {
                                let entry_obj = val_to_obj(*entry, &self.heap);
                                if let Object::Array(pair) = entry_obj {
                                    let pair = pair.borrow();
                                    if pair.len() >= 2 {
                                        let key_obj = val_to_obj(pair[0], &self.heap);
                                        let key = self.hash_key_from_object(&key_obj);
                                        Self::map_insert_or_replace(
                                            &mut target_entries,
                                            &mut target_indices,
                                            key,
                                            pair[1],
                                        );
                                    }
                                }
                            }
                        }
                        Object::Map(existing) => {
                            let mut target_entries = map_obj.entries.borrow_mut();
                            let mut target_indices = map_obj.indices.borrow_mut();
                            for (k, v) in existing.entries.borrow().iter() {
                                Self::map_insert_or_replace(
                                    &mut target_entries,
                                    &mut target_indices,
                                    k.clone(),
                                    *v,
                                );
                            }
                        }
                        _ => {}
                    }
                }
                Ok(obj_into_val(Object::Map(Box::new(map_obj)), &mut self.heap))
            }
            BuiltinFunction::MapSet => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Map.set missing receiver".to_string()))?;
                let map_obj = match receiver {
                    Object::Map(m) => m,
                    _ => return Ok(Value::UNDEFINED),
                };
                if args.len() >= 2 {
                    let arg0 = val_to_obj(args[0], &self.heap);
                    let key = self.hash_key_from_object(&arg0);
                    let mut entries = map_obj.entries.borrow_mut();
                    let mut indices = map_obj.indices.borrow_mut();
                    Self::map_insert_or_replace(&mut entries, &mut indices, key, args[1]);
                }
                Ok(obj_into_val(Object::Map(map_obj), &mut self.heap))
            }
            BuiltinFunction::MapGet => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Map.get missing receiver".to_string()))?;
                let map_obj = match receiver {
                    Object::Map(m) => m,
                    _ => return Ok(Value::UNDEFINED),
                };
                let key = args.first().map(|v| {
                    let obj = val_to_obj(*v, &self.heap);
                    self.hash_key_from_object(&obj)
                });
                if let Some(k) = key {
                    let entries = map_obj.entries.borrow();
                    let indices = map_obj.indices.borrow();
                    Ok(Self::map_get(&entries, &indices, &k).unwrap_or(Value::UNDEFINED))
                } else {
                    Ok(Value::UNDEFINED)
                }
            }
            BuiltinFunction::MapHas => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Map.has missing receiver".to_string()))?;
                let map_obj = match receiver {
                    Object::Map(m) => m,
                    _ => return Ok(Value::from_bool(false)),
                };
                let key = args.first().map(|v| {
                    let obj = val_to_obj(*v, &self.heap);
                    self.hash_key_from_object(&obj)
                });
                Ok(Value::from_bool(
                    key.map(|k| Self::map_contains(&map_obj.indices.borrow(), &k))
                        .unwrap_or(false),
                ))
            }
            BuiltinFunction::MapDelete => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Map.delete missing receiver".to_string()))?;
                let map_obj = match receiver {
                    Object::Map(m) => m,
                    _ => return Ok(Value::from_bool(false)),
                };
                let key = args.first().map(|v| {
                    let obj = val_to_obj(*v, &self.heap);
                    self.hash_key_from_object(&obj)
                });
                Ok(Value::from_bool(
                    key.map(|k| {
                        let mut entries = map_obj.entries.borrow_mut();
                        let mut indices = map_obj.indices.borrow_mut();
                        Self::map_remove(&mut entries, &mut indices, &k).is_some()
                    })
                    .unwrap_or(false),
                ))
            }
            BuiltinFunction::MapClear => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Map.clear missing receiver".to_string()))?;
                let map_obj = match receiver {
                    Object::Map(m) => m,
                    _ => return Ok(Value::UNDEFINED),
                };
                map_obj.entries.borrow_mut().clear();
                map_obj.indices.borrow_mut().clear();
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::MapKeys => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Map.keys missing receiver".to_string()))?;
                let map_obj = match receiver {
                    Object::Map(m) => m,
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let out: Vec<Value> = map_obj
                    .entries
                    .borrow()
                    .iter()
                    .map(|(k, _)| obj_into_val(self.object_from_hash_key(k), &mut self.heap))
                    .collect();
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::MapValues => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Map.values missing receiver".to_string()))?;
                let map_obj = match receiver {
                    Object::Map(m) => m,
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let out: Vec<Value> = map_obj.entries.borrow().iter().map(|(_, v)| *v).collect();
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::MapEntries => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Map.entries missing receiver".to_string())
                })?;
                let map_obj = match receiver {
                    Object::Map(m) => m,
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let out: Vec<Value> = map_obj
                    .entries
                    .borrow()
                    .iter()
                    .map(|(k, v)| {
                        let key_val = obj_into_val(self.object_from_hash_key(k), &mut self.heap);
                        let entry = make_array(vec![key_val, *v]);
                        obj_into_val(entry, &mut self.heap)
                    })
                    .collect();
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::MapForEach => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Map.forEach missing receiver".to_string())
                })?;
                let map_obj = match receiver {
                    Object::Map(m) => m,
                    _ => return Ok(Value::UNDEFINED),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Map.forEach requires callback".to_string())
                })?;
                let snapshot = map_obj.entries.borrow().clone();
                for (k, v) in snapshot {
                    let key_val = obj_into_val(self.object_from_hash_key(&k), &mut self.heap);
                    let map_val = obj_into_val(Object::Map(map_obj.clone()), &mut self.heap);
                    let _ = self.call_value3(callback, v, key_val, map_val)?;
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::SetCtor => {
                let set_obj = crate::object::SetObject::default();
                if let Some(source_val) = args.first() {
                    let source = val_to_obj(*source_val, &self.heap);
                    match &source {
                        Object::Array(entries) => {
                            let mut target_entries = set_obj.entries.borrow_mut();
                            let mut target_indices = set_obj.indices.borrow_mut();
                            for entry in entries.borrow().iter() {
                                let entry_obj = val_to_obj(*entry, &self.heap);
                                Self::set_insert_unique(
                                    &mut target_entries,
                                    &mut target_indices,
                                    self.hash_key_from_object(&entry_obj),
                                );
                            }
                        }
                        Object::Set(existing) => {
                            let mut target_entries = set_obj.entries.borrow_mut();
                            let mut target_indices = set_obj.indices.borrow_mut();
                            for key in existing.entries.borrow().iter() {
                                Self::set_insert_unique(
                                    &mut target_entries,
                                    &mut target_indices,
                                    key.clone(),
                                );
                            }
                        }
                        Object::String(text) => {
                            let mut target_entries = set_obj.entries.borrow_mut();
                            let mut target_indices = set_obj.indices.borrow_mut();
                            for ch in text.chars() {
                                Self::set_insert_unique(
                                    &mut target_entries,
                                    &mut target_indices,
                                    self.hash_key_from_object(&Object::String(
                                        ch.to_string().into(),
                                    )),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                Ok(obj_into_val(Object::Set(Box::new(set_obj)), &mut self.heap))
            }
            BuiltinFunction::SetAdd => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Set.add missing receiver".to_string()))?;
                let set_obj = match receiver {
                    Object::Set(s) => s,
                    _ => return Ok(Value::UNDEFINED),
                };
                if let Some(v) = args.first() {
                    let obj = val_to_obj(*v, &self.heap);
                    let mut entries = set_obj.entries.borrow_mut();
                    let mut indices = set_obj.indices.borrow_mut();
                    Self::set_insert_unique(
                        &mut entries,
                        &mut indices,
                        self.hash_key_from_object(&obj),
                    );
                }
                Ok(obj_into_val(Object::Set(set_obj), &mut self.heap))
            }
            BuiltinFunction::SetHas => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Set.has missing receiver".to_string()))?;
                let set_obj = match receiver {
                    Object::Set(s) => s,
                    _ => return Ok(Value::from_bool(false)),
                };
                let key = args.first().map(|v| {
                    let obj = val_to_obj(*v, &self.heap);
                    self.hash_key_from_object(&obj)
                });
                Ok(Value::from_bool(
                    key.map(|k| Self::set_contains(&set_obj.indices.borrow(), &k))
                        .unwrap_or(false),
                ))
            }
            BuiltinFunction::SetDelete => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Set.delete missing receiver".to_string()))?;
                let set_obj = match receiver {
                    Object::Set(s) => s,
                    _ => return Ok(Value::from_bool(false)),
                };
                let key = args.first().map(|v| {
                    let obj = val_to_obj(*v, &self.heap);
                    self.hash_key_from_object(&obj)
                });
                Ok(Value::from_bool(
                    key.map(|k| {
                        let mut entries = set_obj.entries.borrow_mut();
                        let mut indices = set_obj.indices.borrow_mut();
                        Self::set_remove(&mut entries, &mut indices, &k)
                    })
                    .unwrap_or(false),
                ))
            }
            BuiltinFunction::SetClear => {
                let receiver = builtin
                    .receiver
                    .ok_or_else(|| VMError::TypeError("Set.clear missing receiver".to_string()))?;
                let set_obj = match receiver {
                    Object::Set(s) => s,
                    _ => return Ok(Value::UNDEFINED),
                };
                set_obj.entries.borrow_mut().clear();
                set_obj.indices.borrow_mut().clear();
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::SetKeys | BuiltinFunction::SetValues => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Set.keys/values missing receiver".to_string())
                })?;
                let set_obj = match receiver {
                    Object::Set(s) => s,
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let out: Vec<Value> = set_obj
                    .entries
                    .borrow()
                    .iter()
                    .map(|k| obj_into_val(self.object_from_hash_key(k), &mut self.heap))
                    .collect();
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::SetEntries => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Set.entries missing receiver".to_string())
                })?;
                let set_obj = match receiver {
                    Object::Set(s) => s,
                    _ => return Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                };
                let out: Vec<Value> = set_obj
                    .entries
                    .borrow()
                    .iter()
                    .map(|k| {
                        let v = obj_into_val(self.object_from_hash_key(k), &mut self.heap);
                        let entry = make_array(vec![v, v]);
                        obj_into_val(entry, &mut self.heap)
                    })
                    .collect();
                Ok(obj_into_val(make_array(out), &mut self.heap))
            }
            BuiltinFunction::SetForEach => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Set.forEach missing receiver".to_string())
                })?;
                let set_obj = match receiver {
                    Object::Set(s) => s,
                    _ => return Ok(Value::UNDEFINED),
                };
                let callback = args.first().copied().ok_or_else(|| {
                    VMError::TypeError("Set.forEach requires callback".to_string())
                })?;
                let snapshot = set_obj.entries.borrow().clone();
                for k in snapshot {
                    let v_val = obj_into_val(self.object_from_hash_key(&k), &mut self.heap);
                    let set_val = obj_into_val(Object::Set(set_obj.clone()), &mut self.heap);
                    let _ = self.call_value3(callback, v_val, v_val, set_val)?;
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::GeneratorNext => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Generator.next missing receiver".to_string())
                })?;
                let gen_rc = match receiver {
                    Object::Generator(g) => g,
                    _ => return Ok(Value::UNDEFINED),
                };
                let next_arg = args.first().copied().unwrap_or(Value::UNDEFINED);
                self.execute_generator_next(&gen_rc, next_arg)
            }
            BuiltinFunction::GeneratorReturn => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Generator.return missing receiver".to_string())
                })?;
                let gen_rc = match receiver {
                    Object::Generator(g) => g,
                    _ => return Ok(Value::UNDEFINED),
                };
                let return_value = args.first().copied().unwrap_or(Value::UNDEFINED);
                self.execute_generator_return(&gen_rc, return_value)
            }
            BuiltinFunction::GeneratorThrow => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Generator.throw missing receiver".to_string())
                })?;
                let gen_rc = match receiver {
                    Object::Generator(g) => g,
                    _ => return Ok(Value::UNDEFINED),
                };
                // For now, just mark the generator as completed and propagate
                // the error.  Full throw-into-generator semantics would require
                // resuming inside a try/catch inside the generator body.
                gen_rc.borrow_mut().state = crate::object::GeneratorState::Completed;
                let err_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                let err_obj = val_to_obj(err_val, &self.heap);
                let msg = match &err_obj {
                    Object::String(s) => s.to_string(),
                    Object::Error(e) => e.message.to_string(),
                    _ => format!("{:?}", err_obj),
                };
                Err(VMError::TypeError(msg))
            }
            BuiltinFunction::DateNow => {
                let ms = epoch_millis_now();
                Ok(Value::from_f64(ms))
            }
            BuiltinFunction::DateToISOString => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                let secs = (ms / 1000.0) as i64;
                let millis = (ms % 1000.0) as u32;
                let (year, month, day) = Self::days_to_ymd(secs / 86400);
                let tod = secs % 86400;
                let iso = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                    year, month, day, tod / 3600, (tod % 3600) / 60, tod % 60, millis);
                Ok(obj_into_val(Object::String(iso.into()), &mut self.heap))
            }
            BuiltinFunction::DateGetTime | BuiltinFunction::DateValueOf => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                Ok(Value::from_f64(ms))
            }
            BuiltinFunction::DateGetHours => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                let secs = (ms / 1000.0) as i64;
                let hours = ((secs % 86400) + 86400) % 86400 / 3600;
                Ok(Value::from_i64(hours))
            }
            BuiltinFunction::DateGetMinutes => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                let secs = (ms / 1000.0) as i64;
                let minutes = ((secs % 3600) + 3600) % 3600 / 60;
                Ok(Value::from_i64(minutes))
            }
            BuiltinFunction::DateGetSeconds => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                let secs = (ms / 1000.0) as i64;
                Ok(Value::from_i64(((secs % 60) + 60) % 60))
            }
            BuiltinFunction::DateGetMilliseconds => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                Ok(Value::from_i64(((ms % 1000.0) + 1000.0) as i64 % 1000))
            }
            BuiltinFunction::DateGetFullYear => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                let secs = (ms / 1000.0) as i64;
                let (year, _, _) = Self::days_to_ymd(secs / 86400);
                Ok(Value::from_i64(year))
            }
            BuiltinFunction::DateGetMonth => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                let secs = (ms / 1000.0) as i64;
                let (_, month, _) = Self::days_to_ymd(secs / 86400);
                // JS months are 0-indexed
                Ok(Value::from_i64(month - 1))
            }
            BuiltinFunction::DateGetDate => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                let secs = (ms / 1000.0) as i64;
                let (_, _, day) = Self::days_to_ymd(secs / 86400);
                Ok(Value::from_i64(day))
            }
            BuiltinFunction::DateGetDay => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                let secs = (ms / 1000.0) as i64;
                // Day of week: 0=Sunday. Unix epoch (1970-01-01) was a Thursday (4).
                let days = secs / 86400;
                let dow = ((days % 7 + 4) % 7 + 7) % 7;
                Ok(Value::from_i64(dow))
            }
            BuiltinFunction::DateToLocaleDateString
            | BuiltinFunction::DateToLocaleTimeString
            | BuiltinFunction::DateToLocaleString
            | BuiltinFunction::DateToString => {
                let ms = Self::extract_date_ms(&builtin.receiver);
                let secs = (ms / 1000.0) as i64;
                let millis = (ms % 1000.0) as u32;
                let (year, month, day) = Self::days_to_ymd(secs / 86400);
                let tod = ((secs % 86400) + 86400) % 86400;
                let hours = tod / 3600;
                let minutes = (tod % 3600) / 60;
                let seconds = tod % 60;
                let s = match builtin.function {
                    BuiltinFunction::DateToLocaleDateString => {
                        format!("{}/{}/{}", month, day, year)
                    }
                    BuiltinFunction::DateToLocaleTimeString => {
                        let ampm = if hours < 12 { "AM" } else { "PM" };
                        let h12 = if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours };
                        format!("{}:{:02}:{:02} {}", h12, minutes, seconds, ampm)
                    }
                    BuiltinFunction::DateToLocaleString => {
                        let ampm = if hours < 12 { "AM" } else { "PM" };
                        let h12 = if hours == 0 { 12 } else if hours > 12 { hours - 12 } else { hours };
                        format!("{}/{}/{}, {}:{:02}:{:02} {}", month, day, year, h12, minutes, seconds, ampm)
                    }
                    _ => {
                        // DateToString
                        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                            year, month, day, hours, minutes, seconds, millis)
                    }
                };
                Ok(obj_into_val(Object::String(s.into()), &mut self.heap))
            }
            BuiltinFunction::LocalStorageGetItem => {
                if let Some(ref storage) = self.local_storage {
                    let key = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    match storage.get_item(&key) {
                        Some(val) => {
                            let s = Object::String(val.into());
                            Ok(obj_into_val(s, &mut self.heap))
                        }
                        None => Ok(Value::NULL),
                    }
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::LocalStorageSetItem => {
                if let Some(ref mut storage) = self.local_storage {
                    let key = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let value = args
                        .get(1)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    storage.set_item(&key, &value);
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::LocalStorageRemoveItem => {
                if let Some(ref mut storage) = self.local_storage {
                    let key = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    storage.remove_item(&key);
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::LocalStorageClear => {
                if let Some(ref mut storage) = self.local_storage {
                    storage.clear();
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::DbQuery => {
                if let Some(ref db) = self.db {
                    let collection = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    match db.query(&collection) {
                        Ok(records) => {
                            let items: Vec<Value> = records
                                .into_iter()
                                .map(|r| {
                                    let obj = self.db_record_to_object(r);
                                    obj_into_val(obj, &mut self.heap)
                                })
                                .collect();
                            Ok(obj_into_val(make_array(items), &mut self.heap))
                        }
                        Err(e) => Err(VMError::TypeError(format!("db.query error: {}", e))),
                    }
                } else {
                    Ok(obj_into_val(make_array(vec![]), &mut self.heap))
                }
            }
            BuiltinFunction::DbCreate => {
                let collection = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_default();
                let data_json = if let Some(&v) = args.get(1) {
                    let obj = val_to_obj(v, &self.heap);
                    let jv = self.object_to_json_value(&obj);
                    serde_json::to_string(&jv).unwrap_or_else(|_| "{}".to_string())
                } else {
                    "{}".to_string()
                };
                if let Some(ref mut db) = self.db {
                    match db.create(&collection, &data_json) {
                        Ok(record) => {
                            let obj = self.db_record_to_object(record);
                            Ok(obj_into_val(obj, &mut self.heap))
                        }
                        Err(e) => Err(VMError::TypeError(format!("db.create error: {}", e))),
                    }
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::DbUpdate => {
                let id = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_default();
                let data_json = if let Some(&v) = args.get(1) {
                    let obj = val_to_obj(v, &self.heap);
                    let jv = self.object_to_json_value(&obj);
                    serde_json::to_string(&jv).unwrap_or_else(|_| "{}".to_string())
                } else {
                    "{}".to_string()
                };
                if let Some(ref mut db) = self.db {
                    match db.update(&id, &data_json) {
                        Ok(Some(record)) => {
                            let obj = self.db_record_to_object(record);
                            Ok(obj_into_val(obj, &mut self.heap))
                        }
                        Ok(None) => Ok(Value::NULL),
                        Err(e) => Err(VMError::TypeError(format!("db.update error: {}", e))),
                    }
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::DbDelete => {
                if let Some(ref mut db) = self.db {
                    let id = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    if let Err(e) = db.delete(&id) {
                        return Err(VMError::TypeError(format!("db.delete error: {}", e)));
                    }
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::DbHardDelete => {
                if let Some(ref mut db) = self.db {
                    let collection = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let id = args
                        .get(1)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    if let Err(e) = db.hard_delete(&collection, &id) {
                        return Err(VMError::TypeError(format!("db.hardDelete error: {}", e)));
                    }
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::DbGet => {
                if let Some(ref db) = self.db {
                    let collection = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let id = args
                        .get(1)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    match db.get(&collection, &id) {
                        Ok(Some(record)) => {
                            let obj = self.db_record_to_object(record);
                            Ok(obj_into_val(obj, &mut self.heap))
                        }
                        Ok(None) => Ok(Value::NULL),
                        Err(e) => Err(VMError::TypeError(format!("db.get error: {}", e))),
                    }
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::DbStartSync => {
                if let Some(ref mut db) = self.db {
                    let room = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    db.start_sync(&room);
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::DbStopSync => {
                if let Some(ref mut db) = self.db {
                    let room = args
                        .first()
                        .map(|v| {
                            let s = val_inspect(*v, &self.heap);
                            if s == "undefined" || s == "null" || s.is_empty() {
                                None
                            } else {
                                Some(s)
                            }
                        })
                        .unwrap_or(None);
                    db.stop_sync(room.as_deref());
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::DbGetSyncStatus => {
                if let Some(ref db) = self.db {
                    let room = args
                        .first()
                        .map(|v| {
                            let s = val_inspect(*v, &self.heap);
                            if s == "undefined" || s == "null" || s.is_empty() {
                                None
                            } else {
                                Some(s)
                            }
                        })
                        .unwrap_or(None);
                    let status = db.get_sync_status(room.as_deref());
                    let mut hash = crate::object::HashObject::default();
                    hash.insert_pair(
                        HashKey::from_string("connected"),
                        Value::from_bool(status.connected),
                    );
                    hash.insert_pair(
                        HashKey::from_string("peers"),
                        Value::from_i64(status.peers as i64),
                    );
                    let room_val = obj_into_val(Object::String(status.room.into()), &mut self.heap);
                    hash.insert_pair(HashKey::from_string("room"), room_val);
                    Ok(obj_into_val(make_hash(hash), &mut self.heap))
                } else {
                    let mut hash = crate::object::HashObject::default();
                    hash.insert_pair(HashKey::from_string("connected"), Value::from_bool(false));
                    hash.insert_pair(HashKey::from_string("peers"), Value::from_i64(0));
                    let room_val = obj_into_val(Object::String("".into()), &mut self.heap);
                    hash.insert_pair(HashKey::from_string("room"), room_val);
                    Ok(obj_into_val(make_hash(hash), &mut self.heap))
                }
            }
            BuiltinFunction::DbGetSavedSyncRoom => {
                if let Some(ref db) = self.db {
                    match db.get_saved_sync_room() {
                        Some(room) => {
                            let s = Object::String(room.into());
                            Ok(obj_into_val(s, &mut self.heap))
                        }
                        None => Ok(Value::NULL),
                    }
                } else {
                    Ok(Value::NULL)
                }
            }

            // ════════════════════════════════════════════════════════════════
            // HTTP bridge
            // ════════════════════════════════════════════════════════════════
            BuiltinFunction::HttpGet => {
                if let Some(ref http) = self.http {
                    let url = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    match http.get(&url) {
                        Ok(resp) => {
                            let mut hash = crate::object::HashObject::default();
                            hash.insert_pair(HashKey::from_string("status"), Value::from_i64(resp.status as i64));
                            hash.insert_pair(HashKey::from_string("ok"), Value::from_bool(resp.ok));
                            let body_val = obj_into_val(Object::String(resp.body.into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("body"), body_val);
                            Ok(obj_into_val(make_hash(hash), &mut self.heap))
                        }
                        Err(e) => {
                            let mut hash = crate::object::HashObject::default();
                            hash.insert_pair(HashKey::from_string("ok"), Value::from_bool(false));
                            hash.insert_pair(HashKey::from_string("status"), Value::from_i64(0));
                            let err_val = obj_into_val(Object::String(e.into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("error"), err_val);
                            let body_val = obj_into_val(Object::String("".into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("body"), body_val);
                            Ok(obj_into_val(make_hash(hash), &mut self.heap))
                        }
                    }
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::HttpPost => {
                if let Some(ref http) = self.http {
                    let url = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    let body = args.get(1).map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    let ct = args.get(2).map(|v| val_inspect(*v, &self.heap)).unwrap_or_else(|| "application/json".into());
                    match http.post(&url, &body, &ct) {
                        Ok(resp) => {
                            let mut hash = crate::object::HashObject::default();
                            hash.insert_pair(HashKey::from_string("status"), Value::from_i64(resp.status as i64));
                            hash.insert_pair(HashKey::from_string("ok"), Value::from_bool(resp.ok));
                            let body_val = obj_into_val(Object::String(resp.body.into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("body"), body_val);
                            Ok(obj_into_val(make_hash(hash), &mut self.heap))
                        }
                        Err(e) => {
                            let mut hash = crate::object::HashObject::default();
                            hash.insert_pair(HashKey::from_string("ok"), Value::from_bool(false));
                            hash.insert_pair(HashKey::from_string("status"), Value::from_i64(0));
                            let err_val = obj_into_val(Object::String(e.into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("error"), err_val);
                            let body_val = obj_into_val(Object::String("".into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("body"), body_val);
                            Ok(obj_into_val(make_hash(hash), &mut self.heap))
                        }
                    }
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::HttpPut => {
                if let Some(ref http) = self.http {
                    let url = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    let body = args.get(1).map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    let ct = args.get(2).map(|v| val_inspect(*v, &self.heap)).unwrap_or_else(|| "application/json".into());
                    match http.put(&url, &body, &ct) {
                        Ok(resp) => {
                            let mut hash = crate::object::HashObject::default();
                            hash.insert_pair(HashKey::from_string("status"), Value::from_i64(resp.status as i64));
                            hash.insert_pair(HashKey::from_string("ok"), Value::from_bool(resp.ok));
                            let body_val = obj_into_val(Object::String(resp.body.into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("body"), body_val);
                            Ok(obj_into_val(make_hash(hash), &mut self.heap))
                        }
                        Err(e) => {
                            let mut hash = crate::object::HashObject::default();
                            hash.insert_pair(HashKey::from_string("ok"), Value::from_bool(false));
                            hash.insert_pair(HashKey::from_string("status"), Value::from_i64(0));
                            let err_val = obj_into_val(Object::String(e.into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("error"), err_val);
                            let body_val = obj_into_val(Object::String("".into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("body"), body_val);
                            Ok(obj_into_val(make_hash(hash), &mut self.heap))
                        }
                    }
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::HttpDelete => {
                if let Some(ref http) = self.http {
                    let url = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    match http.delete(&url) {
                        Ok(resp) => {
                            let mut hash = crate::object::HashObject::default();
                            hash.insert_pair(HashKey::from_string("status"), Value::from_i64(resp.status as i64));
                            hash.insert_pair(HashKey::from_string("ok"), Value::from_bool(resp.ok));
                            let body_val = obj_into_val(Object::String(resp.body.into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("body"), body_val);
                            Ok(obj_into_val(make_hash(hash), &mut self.heap))
                        }
                        Err(e) => {
                            let mut hash = crate::object::HashObject::default();
                            hash.insert_pair(HashKey::from_string("ok"), Value::from_bool(false));
                            hash.insert_pair(HashKey::from_string("status"), Value::from_i64(0));
                            let err_val = obj_into_val(Object::String(e.into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("error"), err_val);
                            let body_val = obj_into_val(Object::String("".into()), &mut self.heap);
                            hash.insert_pair(HashKey::from_string("body"), body_val);
                            Ok(obj_into_val(make_hash(hash), &mut self.heap))
                        }
                    }
                } else {
                    Ok(Value::NULL)
                }
            }

            // ════════════════════════════════════════════════════════════════
            // FS bridge
            // ════════════════════════════════════════════════════════════════
            BuiltinFunction::FsReadFile => {
                if let Some(ref fs) = self.fs {
                    let path = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    match fs.read_file(&path) {
                        Ok(content) => Ok(obj_into_val(Object::String(content.into()), &mut self.heap)),
                        Err(_) => Ok(Value::NULL),
                    }
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::FsWriteFile => {
                if let Some(ref mut fs) = self.fs {
                    let path = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    let content = args.get(1).map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    match fs.write_file(&path, &content) {
                        Ok(()) => Ok(Value::from_bool(true)),
                        Err(_) => Ok(Value::from_bool(false)),
                    }
                } else {
                    Ok(Value::from_bool(false))
                }
            }
            BuiltinFunction::FsAppendFile => {
                if let Some(ref mut fs) = self.fs {
                    let path = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    let content = args.get(1).map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    match fs.append_file(&path, &content) {
                        Ok(()) => Ok(Value::from_bool(true)),
                        Err(_) => Ok(Value::from_bool(false)),
                    }
                } else {
                    Ok(Value::from_bool(false))
                }
            }
            BuiltinFunction::FsExists => {
                if let Some(ref fs) = self.fs {
                    let path = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    Ok(Value::from_bool(fs.exists(&path)))
                } else {
                    Ok(Value::from_bool(false))
                }
            }
            BuiltinFunction::FsListDir => {
                if let Some(ref fs) = self.fs {
                    let path = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    match fs.list_dir(&path) {
                        Ok(entries) => {
                            let items: Vec<Value> = entries
                                .into_iter()
                                .map(|e| obj_into_val(Object::String(e.into()), &mut self.heap))
                                .collect();
                            Ok(obj_into_val(make_array(items), &mut self.heap))
                        }
                        Err(_) => Ok(obj_into_val(make_array(vec![]), &mut self.heap)),
                    }
                } else {
                    Ok(obj_into_val(make_array(vec![]), &mut self.heap))
                }
            }
            BuiltinFunction::FsDeleteFile => {
                if let Some(ref mut fs) = self.fs {
                    let path = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    match fs.delete_file(&path) {
                        Ok(()) => Ok(Value::from_bool(true)),
                        Err(_) => Ok(Value::from_bool(false)),
                    }
                } else {
                    Ok(Value::from_bool(false))
                }
            }
            BuiltinFunction::FsMkdir => {
                if let Some(ref mut fs) = self.fs {
                    let path = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    match fs.mkdir(&path) {
                        Ok(()) => Ok(Value::from_bool(true)),
                        Err(_) => Ok(Value::from_bool(false)),
                    }
                } else {
                    Ok(Value::from_bool(false))
                }
            }

            // ════════════════════════════════════════════════════════════════
            // Env bridge
            // ════════════════════════════════════════════════════════════════
            BuiltinFunction::EnvGet => {
                if let Some(ref env) = self.env {
                    let name = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    match env.get(&name) {
                        Some(val) => Ok(obj_into_val(Object::String(val.into()), &mut self.heap)),
                        None => Ok(Value::NULL),
                    }
                } else {
                    Ok(Value::NULL)
                }
            }
            BuiltinFunction::EnvKeys => {
                if let Some(ref env) = self.env {
                    let keys = env.keys();
                    let items: Vec<Value> = keys
                        .into_iter()
                        .map(|k| obj_into_val(Object::String(k.into()), &mut self.heap))
                        .collect();
                    Ok(obj_into_val(make_array(items), &mut self.heap))
                } else {
                    Ok(obj_into_val(make_array(vec![]), &mut self.heap))
                }
            }
            BuiltinFunction::EnvLog => {
                if let Some(ref env) = self.env {
                    let level = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    let message = args.get(1).map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                    env.log(&level, &message);
                }
                Ok(Value::UNDEFINED)
            }

            // ════════════════════════════════════════════════════════════════
            // Draw bridge
            // ════════════════════════════════════════════════════════════════
            BuiltinFunction::DrawRect => {
                if let Some(ref mut d) = self.draw {
                    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let w = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let h = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let fill = args
                        .get(4)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let border_radius = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
                    let border_width = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
                    let border_color = args
                        .get(7)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let opacity = args.get(8).map(|v| v.to_number()).unwrap_or(1.0);
                    d.draw_rect(
                        x,
                        y,
                        w,
                        h,
                        &fill,
                        border_radius,
                        border_width,
                        &border_color,
                        opacity,
                    );
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawRoundedRect => {
                // Calling convention (formlogic):
                //   draw.roundedRect(x, y, w, h, fill, borderRadius, borderWidth, borderColor, opacity)
                // This is the same as draw.rect but always uses rounded corners.
                if let Some(ref mut d) = self.draw {
                    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let w = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let h = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let fill = args
                        .get(4)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let border_radius = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
                    let border_width = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
                    let border_color = args
                        .get(7)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let opacity = args.get(8).map(|v| v.to_number()).unwrap_or(1.0);
                    d.draw_rect(
                        x,
                        y,
                        w,
                        h,
                        &fill,
                        border_radius,
                        border_width,
                        &border_color,
                        opacity,
                    );
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawCircle => {
                if let Some(ref mut d) = self.draw {
                    let cx = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let cy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let fill = args
                        .get(3)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let opacity = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
                    d.draw_circle(cx, cy, r, &fill, opacity);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawEllipse => {
                if let Some(ref mut d) = self.draw {
                    let cx = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let cy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let rx = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let ry = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let fill = args
                        .get(4)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let opacity = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
                    d.draw_ellipse(cx, cy, rx, ry, &fill, opacity);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawLine => {
                if let Some(ref mut d) = self.draw {
                    let x1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let y1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let x2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let y2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let color = args
                        .get(4)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let width = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
                    d.draw_line(x1, y1, x2, y2, &color, width);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawPath => {
                if let Some(ref mut d) = self.draw {
                    let commands = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let fill = args
                        .get(1)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let stroke = args
                        .get(2)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let stroke_width = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let opacity = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
                    d.draw_path(&commands, &fill, &stroke, stroke_width, opacity);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawText => {
                // Calling convention (formlogic):
                //   draw.text(text, x, y, fontSize, fontWeight, color,
                //             fontFamily, maxWidth, opacity)
                if let Some(ref mut d) = self.draw {
                    let text = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let y = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let font_size = args.get(3).map(|v| v.to_number()).unwrap_or(14.0);
                    let font_weight = args.get(4).map(|v| v.to_number() as u32).unwrap_or(400);
                    let color = args
                        .get(5)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let font_family = args
                        .get(6)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let max_width = args.get(7).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
                    let opacity = args.get(8).map(|v| v.to_number()).unwrap_or(1.0);

                    // Apply opacity layer if needed
                    if opacity < 1.0 && opacity > 0.0 {
                        d.push_opacity(opacity);
                    }

                    let (w, h) = d.draw_text(
                        &text,
                        x,
                        y,
                        font_size,
                        &color,
                        font_weight,
                        &font_family,
                        max_width,
                        0.0, // letter_spacing (not used by formlogic stdlib)
                    );

                    if opacity < 1.0 && opacity > 0.0 {
                        d.pop_opacity();
                    }

                    // Return {width, height} object
                    let mut hash = HashObject::default();
                    hash.insert_pair(HashKey::from_string("width"), Value::from_f64(w));
                    hash.insert_pair(HashKey::from_string("height"), Value::from_f64(h));
                    Ok(obj_into_val(make_hash(hash), &mut self.heap))
                } else {
                    Ok(Value::UNDEFINED)
                }
            }

            BuiltinFunction::DrawImage => {
                if let Some(ref mut d) = self.draw {
                    let src = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let y = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let w = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let h = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
                    let opacity = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
                    d.draw_image(&src, x, y, w, h, opacity);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawLinearGradient => {
                let stops = self.extract_f64_vec(args.get(5).copied());
                if let Some(ref mut d) = self.draw {
                    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let w = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let h = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let angle_deg = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
                    let border_radius = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
                    d.draw_linear_gradient(x, y, w, h, angle_deg, &stops, border_radius);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawRadialGradient => {
                let stops = self.extract_f64_vec(args.get(4).copied());
                if let Some(ref mut d) = self.draw {
                    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let w = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let h = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let border_radius = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
                    d.draw_radial_gradient(x, y, w, h, &stops, border_radius);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawShadow => {
                if let Some(ref mut d) = self.draw {
                    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let w = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let h = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let blur = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
                    let spread = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
                    let color = args
                        .get(6)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let offset_x = args.get(7).map(|v| v.to_number()).unwrap_or(0.0);
                    let offset_y = args.get(8).map(|v| v.to_number()).unwrap_or(0.0);
                    let border_radius = args.get(9).map(|v| v.to_number()).unwrap_or(0.0);
                    d.draw_shadow(
                        x,
                        y,
                        w,
                        h,
                        blur,
                        spread,
                        &color,
                        offset_x,
                        offset_y,
                        border_radius,
                    );
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawPushClip => {
                if let Some(ref mut d) = self.draw {
                    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let w = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let h = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let border_radius = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
                    d.push_clip(x, y, w, h, border_radius);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawPopClip => {
                if let Some(ref mut d) = self.draw {
                    d.pop_clip();
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawPushTransform => {
                if let Some(ref mut d) = self.draw {
                    let translate_x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let translate_y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let rotate_deg = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let scale_x = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
                    let scale_y = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
                    d.push_transform(translate_x, translate_y, rotate_deg, scale_x, scale_y);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawPopTransform => {
                if let Some(ref mut d) = self.draw {
                    d.pop_transform();
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawPushOpacity => {
                if let Some(ref mut d) = self.draw {
                    let opacity = args.first().map(|v| v.to_number()).unwrap_or(1.0);
                    d.push_opacity(opacity);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawPopOpacity => {
                if let Some(ref mut d) = self.draw {
                    d.pop_opacity();
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawArc => {
                if let Some(ref mut d) = self.draw {
                    let cx = args.first().map(|v| v.to_number()).unwrap_or(0.0);
                    let cy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let radius = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    let thickness = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
                    let start_angle = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
                    let end_angle = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
                    let color = args
                        .get(6)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    d.draw_arc(cx, cy, radius, thickness, start_angle, end_angle, &color);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::DrawMeasureText => {
                if let Some(ref d) = self.draw {
                    let text = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let font_size = args.get(1).map(|v| v.to_number()).unwrap_or(14.0);
                    let font_weight = args.get(2).map(|v| v.to_number() as u32).unwrap_or(400);
                    let font_family = args
                        .get(3)
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    let max_width = args.get(4).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
                    let (w, h) =
                        d.measure_text(&text, font_size, font_weight, &font_family, max_width);
                    let mut hash = HashObject::default();
                    hash.insert_pair(HashKey::from_string("width"), Value::from_f64(w));
                    hash.insert_pair(HashKey::from_string("height"), Value::from_f64(h));
                    Ok(obj_into_val(make_hash(hash), &mut self.heap))
                } else {
                    Ok(Value::UNDEFINED)
                }
            }

            BuiltinFunction::DrawGetViewportWidth => {
                if let Some(ref d) = self.draw {
                    Ok(Value::from_f64(d.get_viewport_width()))
                } else {
                    Ok(Value::from_f64(0.0))
                }
            }

            BuiltinFunction::DrawGetViewportHeight => {
                if let Some(ref d) = self.draw {
                    Ok(Value::from_f64(d.get_viewport_height()))
                } else {
                    Ok(Value::from_f64(0.0))
                }
            }

            // ════════════════════════════════════════════════════════════════
            // Layout bridge
            // ════════════════════════════════════════════════════════════════
            BuiltinFunction::LayoutCreateNode => {
                let style = self.extract_layout_style(args.first().copied());
                if let Some(ref mut lay) = self.layout {
                    let id = lay.create_node(style);
                    Ok(Value::from_f64(id as f64))
                } else {
                    Ok(Value::from_f64(0.0))
                }
            }

            BuiltinFunction::LayoutSetChildren => {
                let children = self.extract_u64_vec(args.get(1).copied());
                if let Some(ref mut lay) = self.layout {
                    let parent = args.first().map(|v| v.to_number() as u64).unwrap_or(0);
                    lay.set_children(parent, &children);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::LayoutComputeLayout => {
                if let Some(ref mut lay) = self.layout {
                    let root = args.first().map(|v| v.to_number() as u64).unwrap_or(0);
                    let avail_w = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
                    let avail_h = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
                    lay.compute_layout(root, avail_w, avail_h);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::LayoutGetLayout => {
                if let Some(ref lay) = self.layout {
                    let node = args.first().map(|v| v.to_number() as u64).unwrap_or(0);
                    let (x, y, w, h) = lay.get_layout(node);
                    let mut hash = HashObject::default();
                    hash.insert_pair(HashKey::from_string("x"), Value::from_f64(x));
                    hash.insert_pair(HashKey::from_string("y"), Value::from_f64(y));
                    hash.insert_pair(HashKey::from_string("width"), Value::from_f64(w));
                    hash.insert_pair(HashKey::from_string("height"), Value::from_f64(h));
                    Ok(obj_into_val(make_hash(hash), &mut self.heap))
                } else {
                    Ok(Value::UNDEFINED)
                }
            }

            BuiltinFunction::LayoutRemoveNode => {
                if let Some(ref mut lay) = self.layout {
                    let node = args.first().map(|v| v.to_number() as u64).unwrap_or(0);
                    lay.remove_node(node);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::LayoutClear => {
                if let Some(ref mut lay) = self.layout {
                    lay.clear();
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::LayoutUpdateStyle => {
                // args[0] = node_id (f64), args[1] = style object
                let node_id = args
                    .first()
                    .copied()
                    .unwrap_or(Value::from_f64(0.0))
                    .to_number() as u64;
                let style = self.extract_layout_style(args.get(1).copied());
                if let Some(ref mut lay) = self.layout {
                    lay.update_style(node_id, style);
                }
                Ok(Value::UNDEFINED)
            }

            // ════════════════════════════════════════════════════════════════
            // Input bridge
            // ════════════════════════════════════════════════════════════════
            BuiltinFunction::InputGetMouseX => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_f64(inp.get_mouse_x()))
                } else {
                    Ok(Value::from_f64(0.0))
                }
            }

            BuiltinFunction::InputGetMouseY => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_f64(inp.get_mouse_y()))
                } else {
                    Ok(Value::from_f64(0.0))
                }
            }

            BuiltinFunction::InputIsMouseDown => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_bool(inp.is_mouse_down()))
                } else {
                    Ok(Value::FALSE)
                }
            }

            BuiltinFunction::InputIsMousePressed => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_bool(inp.is_mouse_pressed()))
                } else {
                    Ok(Value::FALSE)
                }
            }

            BuiltinFunction::InputIsMouseReleased => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_bool(inp.is_mouse_released()))
                } else {
                    Ok(Value::FALSE)
                }
            }

            BuiltinFunction::InputGetScrollY => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_f64(inp.get_scroll_y()))
                } else {
                    Ok(Value::from_f64(0.0))
                }
            }

            BuiltinFunction::InputSetCursor => {
                if let Some(ref mut inp) = self.input {
                    let cursor = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    inp.set_cursor(&cursor);
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::InputGetTextInput => {
                if let Some(ref inp) = self.input {
                    let text = inp.get_text_input();
                    if text.is_empty() {
                        Ok(obj_into_val(Object::String("".into()), &mut self.heap))
                    } else {
                        Ok(obj_into_val(Object::String(text.into()), &mut self.heap))
                    }
                } else {
                    Ok(obj_into_val(Object::String("".into()), &mut self.heap))
                }
            }

            BuiltinFunction::InputIsBackspacePressed => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_bool(inp.is_backspace_pressed()))
                } else {
                    Ok(Value::FALSE)
                }
            }

            BuiltinFunction::InputIsEscapePressed => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_bool(inp.is_escape_pressed()))
                } else {
                    Ok(Value::FALSE)
                }
            }

            BuiltinFunction::InputRequestRedraw => {
                if let Some(ref mut inp) = self.input {
                    inp.request_redraw();
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::InputGetElapsedSecs => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_f64(inp.get_elapsed_secs()))
                } else {
                    Ok(Value::from_f64(0.0))
                }
            }

            BuiltinFunction::InputGetPageElapsedSecs => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_f64(inp.get_page_elapsed_secs()))
                } else {
                    Ok(Value::from_f64(0.0))
                }
            }

            BuiltinFunction::InputGetDeltaTime => {
                if let Some(ref inp) = self.input {
                    Ok(Value::from_f64(inp.get_delta_time()))
                } else {
                    Ok(Value::from_f64(0.0))
                }
            }

            BuiltinFunction::InputGetFocusedInput => {
                if let Some(ref inp) = self.input {
                    match inp.get_focused_input() {
                        Some(name) => {
                            let s = Object::String(name.into());
                            Ok(obj_into_val(s, &mut self.heap))
                        }
                        None => Ok(Value::NULL),
                    }
                } else {
                    Ok(Value::NULL)
                }
            }

            BuiltinFunction::InputSetFocusedInput => {
                if let Some(ref mut inp) = self.input {
                    let val = args.first().copied();
                    if let Some(v) = val {
                        if v.is_null() || v.is_undefined() {
                            inp.set_focused_input(None);
                        } else {
                            let name = val_inspect(v, &self.heap);
                            inp.set_focused_input(Some(&name));
                        }
                    } else {
                        inp.set_focused_input(None);
                    }
                }
                Ok(Value::UNDEFINED)
            }

            BuiltinFunction::InputIsKeyDown => {
                if let Some(ref inp) = self.input {
                    let key = args
                        .first()
                        .map(|v| val_inspect(*v, &self.heap))
                        .unwrap_or_default();
                    Ok(Value::from_bool(inp.is_key_down(&key)))
                } else {
                    Ok(Value::FALSE)
                }
            }

            // ── Window event bridge ──
            BuiltinFunction::WindowAddEventListener => {
                // args: (event_type: string, handler: function)
                let event_type = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                if let Some(&handler_val) = args.get(1) {
                    self.event_listeners
                        .entry(event_type)
                        .or_insert_with(Vec::new)
                        .push(handler_val);
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::WindowRemoveEventListener => {
                // args: (event_type: string, handler: function)
                let event_type = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                if let Some(&handler_val) = args.get(1) {
                    if let Some(listeners) = self.event_listeners.get_mut(&event_type) {
                        listeners.retain(|v| *v != handler_val);
                    }
                }
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::EventPreventDefault | BuiltinFunction::EventStopPropagation => {
                // No-ops in native runtime
                Ok(Value::UNDEFINED)
            }

            // ── URI encoding ──
            BuiltinFunction::EncodeURIComponent => {
                let input = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_default();
                let encoded = uri_encode(&input, false);
                Ok(obj_into_val(Object::String(encoded.into()), &mut self.heap))
            }
            BuiltinFunction::DecodeURIComponent => {
                let input = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_default();
                let decoded = uri_decode(&input);
                Ok(obj_into_val(Object::String(decoded.into()), &mut self.heap))
            }
            BuiltinFunction::EncodeURI => {
                let input = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_default();
                let encoded = uri_encode(&input, true);
                Ok(obj_into_val(Object::String(encoded.into()), &mut self.heap))
            }
            BuiltinFunction::DecodeURI => {
                let input = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_default();
                let decoded = uri_decode(&input);
                Ok(obj_into_val(Object::String(decoded.into()), &mut self.heap))
            }

            // ── Generic host call bridge ──
            BuiltinFunction::HostCall => {
                // host.call(kind, argsArray, callback)
                let kind = args.first().map(|v| val_inspect(*v, &self.heap)).unwrap_or_default();
                // Second arg is an array of string arguments
                let mut call_args = Vec::new();
                if let Some(&arr_val) = args.get(1) {
                    if arr_val.is_heap() {
                        if let Object::Array(ref items) = self.heap.get(arr_val.heap_index()) {
                            for item in items.borrow().iter() {
                                call_args.push(val_inspect(*item, &self.heap));
                            }
                        }
                    }
                }
                // Third arg is the callback function
                let callback = args.get(2).copied().unwrap_or(Value::UNDEFINED);
                self.queue_host_call(&kind, call_args, callback);
                Ok(Value::UNDEFINED)
            }
            BuiltinFunction::ErrorConstructor => {
                let message = args
                    .first()
                    .map(|v| val_inspect(*v, &self.heap))
                    .unwrap_or_default();
                let err = Object::Error(Box::new(crate::object::ErrorObject {
                    name: Rc::from("Error"),
                    message: Rc::from(message.as_str()),
                }));
                Ok(obj_into_val(err, &mut self.heap))
            }
            BuiltinFunction::StringAt => {
                let receiver = builtin.receiver.as_ref();
                let s = match receiver {
                    Some(Object::String(s)) => s.clone(),
                    _ => return Ok(Value::UNDEFINED),
                };
                let idx = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(0.0) as i64;
                let len = s.chars().count() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len {
                    return Ok(Value::UNDEFINED);
                }
                if let Some(ch) = s.chars().nth(actual as usize) {
                    Ok(obj_into_val(
                        Object::String(ch.to_string().into()),
                        &mut self.heap,
                    ))
                } else {
                    Ok(Value::UNDEFINED)
                }
            }
            BuiltinFunction::StringCodePointAt => {
                let receiver = builtin.receiver.as_ref();
                let s = match receiver {
                    Some(Object::String(s)) => s.clone(),
                    _ => return Ok(Value::UNDEFINED),
                };
                let idx = args
                    .first()
                    .map(|v| self.to_number_val(*v))
                    .transpose()?
                    .unwrap_or(0.0) as usize;
                if let Some(ch) = s.chars().nth(idx) {
                    Ok(Value::from_i64(ch as i64))
                } else {
                    Ok(Value::UNDEFINED)
                }
            }
            BuiltinFunction::StringRaw => {
                // String.raw(strings, ...values)
                // strings is an object with a .raw property (array of raw strings)
                let strings_val = args.first().copied().unwrap_or(Value::UNDEFINED);
                let strings_obj = val_to_obj(strings_val, &self.heap);
                let raw_parts: Vec<String> = match strings_obj {
                    Object::Hash(h) => {
                        let hb = h.borrow();
                        if let Some(raw_val) = hb.get_by_str("raw") {
                            let raw_obj = val_to_obj(raw_val, &self.heap);
                            match raw_obj {
                                Object::Array(items) => {
                                    items.borrow().iter().map(|v| {
                                        let o = val_to_obj(*v, &self.heap);
                                        match o {
                                            Object::String(s) => s.to_string(),
                                            _ => o.inspect(),
                                        }
                                    }).collect()
                                }
                                _ => vec![],
                            }
                        } else {
                            vec![]
                        }
                    }
                    _ => vec![],
                };
                let mut result = String::new();
                for (i, part) in raw_parts.iter().enumerate() {
                    result.push_str(part);
                    if i + 1 < raw_parts.len() {
                        if let Some(&val) = args.get(i + 1) {
                            let obj = val_to_obj(val, &self.heap);
                            match obj {
                                Object::String(s) => result.push_str(&s),
                                _ => result.push_str(&obj.inspect()),
                            }
                        }
                    }
                }
                Ok(obj_into_val(Object::String(result.into()), &mut self.heap))
            }
            BuiltinFunction::StringNormalize => {
                let receiver = builtin.receiver.as_ref();
                match receiver {
                    Some(Object::String(s)) => {
                        // Basic NFC normalization — for ASCII strings this is identity
                        // Full Unicode normalization would require the `unicode-normalization` crate
                        Ok(obj_into_val(Object::String(s.clone()), &mut self.heap))
                    }
                    _ => Ok(Value::UNDEFINED),
                }
            }
            BuiltinFunction::NumberToPrecision => {
                let receiver = builtin.receiver.ok_or_else(|| {
                    VMError::TypeError("Number.toPrecision missing receiver".to_string())
                })?;
                let n = self.to_number(&receiver)?;
                let precision = args
                    .first()
                    .map(|v| self.to_u32_val(*v))
                    .transpose()?
                    .unwrap_or(0) as usize;
                if precision == 0 {
                    // No argument: just toString
                    return Ok(obj_into_val(
                        Object::String(format!("{}", n).into()),
                        &mut self.heap,
                    ));
                }
                let result = Self::format_to_precision(n, precision);
                Ok(obj_into_val(Object::String(result.into()), &mut self.heap))
            }
            BuiltinFunction::StructuredClone => {
                if args.is_empty() {
                    return Ok(Value::UNDEFINED);
                }
                let val = args[0];
                let cloned = self.deep_clone_value(val)?;
                Ok(cloned)
            }
        }
    }

    fn format_to_precision(n: f64, precision: usize) -> String {
        if !n.is_finite() {
            return if n.is_nan() {
                "NaN".to_string()
            } else if n > 0.0 {
                "Infinity".to_string()
            } else {
                "-Infinity".to_string()
            };
        }
        if n == 0.0 {
            if precision <= 1 {
                return "0".to_string();
            }
            return format!("0.{}", "0".repeat(precision - 1));
        }
        let abs = n.abs();
        let exp = abs.log10().floor() as i32;
        // If exponent is within reasonable range, use fixed notation
        if exp >= 0 && (exp as usize) < precision {
            let decimal_places = precision - 1 - exp as usize;
            let formatted = format!("{:.*}", decimal_places, n);
            return formatted;
        }
        if exp < 0 && exp >= -4 {
            let decimal_places = precision as i32 - 1 - exp;
            let formatted = format!("{:.*}", decimal_places as usize, n);
            return formatted;
        }
        // Use exponential notation
        let mantissa_digits = precision - 1;
        let formatted = format!("{:.*e}", mantissa_digits, n);
        // JavaScript uses e+N format
        if let Some(pos) = formatted.find('e') {
            let (mantissa, exp_part) = formatted.split_at(pos);
            let exp_str = &exp_part[1..];
            let exp_val: i32 = exp_str.parse().unwrap_or(0);
            if exp_val >= 0 {
                format!("{}e+{}", mantissa, exp_val)
            } else {
                format!("{}e{}", mantissa, exp_val)
            }
        } else {
            formatted
        }
    }

    fn deep_clone_value(&mut self, val: Value) -> Result<Value, VMError> {
        if !val.is_heap() {
            return Ok(val); // primitives are already cloned by value
        }
        let obj = val_to_obj(val, &self.heap);
        let cloned_obj = self.deep_clone_object(obj)?;
        Ok(obj_into_val(cloned_obj, &mut self.heap))
    }

    fn deep_clone_object(&mut self, obj: Object) -> Result<Object, VMError> {
        match obj {
            Object::Array(items) => {
                let borrowed = items.borrow();
                let mut new_items = Vec::with_capacity(borrowed.len());
                for &v in borrowed.iter() {
                    new_items.push(self.deep_clone_value(v)?);
                }
                Ok(make_array(new_items))
            }
            Object::Hash(hash) => {
                let h = hash.borrow();
                let mut new_hash = HashObject::default();
                for (k, &v) in h.pairs.iter() {
                    let cloned_v = self.deep_clone_value(v)?;
                    new_hash.insert_pair(k.clone(), cloned_v);
                }
                Ok(make_hash(new_hash))
            }
            // Primitives and other types: return as-is
            other => Ok(other),
        }
    }

    // ── Host call queue (softn.* bridge) ──────────────────────────────

    /// Queue an async host call and store the callback for later resolution.
    fn queue_host_call(&mut self, kind: &str, args: Vec<String>, callback: Value) {
        let id = self.next_host_call_id;
        self.next_host_call_id += 1;
        if callback != Value::UNDEFINED {
            self.host_callbacks.insert(id, callback);
        }
        self.pending_host_calls.push(crate::host_bridge::PendingHostCall {
            id,
            kind: kind.to_string(),
            args,
        });
    }

    #[allow(dead_code)]
    fn extract_builtin_array(
        &self,
        builtin: &BuiltinFunctionObject,
    ) -> Result<Rc<crate::object::VmCell<Vec<Value>>>, VMError> {
        if let Some(Object::Array(arr)) = &builtin.receiver {
            return Ok(arr.clone());
        }
        Err(VMError::TypeError("expected array receiver".to_string()))
    }

    #[allow(dead_code)]
    fn flatten_array(&self, items: &[Value], depth: usize, result: &mut Vec<Value>) {
        for &v in items {
            if depth > 0 && v.is_heap() {
                let heap_obj = self.heap.get(v.heap_index());
                if let Object::Array(arr) = heap_obj {
                    self.flatten_array(&arr.borrow(), depth - 1, result);
                    continue;
                }
            }
            result.push(v);
        }
    }

    // ── Bridge helper methods ────────────────────────────────────────

    /// Extract a `[f64; 4]` from a Value that should be an array.
    #[allow(dead_code)]
    fn extract_f64_array_4(&self, val: Option<Value>) -> [f64; 4] {
        let mut out = [0.0; 4];
        if let Some(v) = val {
            if v.is_heap() {
                let obj = self.heap.get(v.heap_index());
                if let Object::Array(ref arr) = obj {
                    let items = arr.borrow();
                    for (i, item) in items.iter().enumerate().take(4) {
                        out[i] = item.to_number();
                    }
                }
            }
        }
        out
    }

    /// Extract a `[f64; 3]` from a Value that should be an array.
    #[allow(dead_code)]
    fn extract_f64_array_3(&self, val: Option<Value>) -> [f64; 3] {
        let mut out = [0.0; 3];
        if let Some(v) = val {
            if v.is_heap() {
                let obj = self.heap.get(v.heap_index());
                if let Object::Array(ref arr) = obj {
                    let items = arr.borrow();
                    for (i, item) in items.iter().enumerate().take(3) {
                        out[i] = item.to_number();
                    }
                }
            }
        }
        out
    }

    /// Extract a `Vec<f64>` from a Value that should be an array.
    fn extract_f64_vec(&self, val: Option<Value>) -> Vec<f64> {
        if let Some(v) = val {
            if v.is_heap() {
                let obj = self.heap.get(v.heap_index());
                if let Object::Array(ref arr) = obj {
                    let items = arr.borrow();
                    return items.iter().map(|item| item.to_number()).collect();
                }
            }
        }
        Vec::new()
    }

    /// Extract a `Vec<u64>` from a Value that should be an array of numbers.
    fn extract_u64_vec(&self, val: Option<Value>) -> Vec<u64> {
        if let Some(v) = val {
            if v.is_heap() {
                let obj = self.heap.get(v.heap_index());
                if let Object::Array(ref arr) = obj {
                    let items = arr.borrow();
                    return items.iter().map(|item| item.to_number() as u64).collect();
                }
            }
        }
        Vec::new()
    }

    /// Extract a `LayoutStyle` from a Value that should be a JS object (hash).
    /// Properties map to the LayoutStyle struct fields.
    fn extract_layout_style(&self, val: Option<Value>) -> crate::layout_bridge::LayoutStyle {
        use crate::layout_bridge::*;
        let mut style = LayoutStyle::default();

        let hash = match val {
            Some(v) if v.is_heap() => {
                let obj = self.heap.get(v.heap_index());
                if let Object::Hash(ref h) = obj {
                    h.borrow()
                } else {
                    return style;
                }
            }
            _ => return style,
        };

        // Helper closures to read properties from the hash
        let get_str = |key: &str| -> Option<String> {
            hash.get_by_str(key).map(|v| val_inspect(v, &self.heap))
        };
        let get_f64 = |key: &str| -> Option<f64> { hash.get_by_str(key).map(|v| v.to_number()) };
        let get_f64_or = |key: &str, default: f64| -> f64 {
            hash.get_by_str(key)
                .map(|v| v.to_number())
                .unwrap_or(default)
        };

        // Display
        if let Some(d) = get_str("display") {
            style.display = match d.as_str() {
                "flex" => LayoutDisplay::Flex,
                "grid" => LayoutDisplay::Grid,
                "none" => LayoutDisplay::None,
                _ => LayoutDisplay::Flex,
            };
        }

        // Position
        if let Some(p) = get_str("position") {
            style.position = match p.as_str() {
                "relative" => LayoutPosition::Relative,
                "absolute" => LayoutPosition::Absolute,
                "fixed" => LayoutPosition::Fixed,
                "sticky" => LayoutPosition::Sticky,
                _ => LayoutPosition::Relative,
            };
        }

        // Overflow
        if let Some(o) = get_str("overflow") {
            style.overflow = match o.as_str() {
                "visible" => LayoutOverflow::Visible,
                "hidden" => LayoutOverflow::Hidden,
                "scroll" => LayoutOverflow::Scroll,
                _ => LayoutOverflow::Visible,
            };
        }

        // Flex container
        if let Some(fd) = get_str("flexDirection") {
            style.flex_direction = match fd.as_str() {
                "row" => FlexDirection::Row,
                "column" => FlexDirection::Column,
                "row-reverse" => FlexDirection::RowReverse,
                "column-reverse" => FlexDirection::ColumnReverse,
                _ => FlexDirection::Column,
            };
        }
        if let Some(fw) = get_str("flexWrap") {
            style.flex_wrap = match fw.as_str() {
                "nowrap" => FlexWrap::NoWrap,
                "wrap" => FlexWrap::Wrap,
                "wrap-reverse" => FlexWrap::WrapReverse,
                _ => FlexWrap::NoWrap,
            };
        }
        if let Some(jc) = get_str("justifyContent") {
            style.justify_content = match jc.as_str() {
                "flex-start" | "start" => JustifyContent::FlexStart,
                "flex-end" | "end" => JustifyContent::FlexEnd,
                "center" => JustifyContent::Center,
                "space-between" => JustifyContent::SpaceBetween,
                "space-around" => JustifyContent::SpaceAround,
                "space-evenly" => JustifyContent::SpaceEvenly,
                _ => JustifyContent::FlexStart,
            };
        }
        if let Some(ai) = get_str("alignItems") {
            style.align_items = match ai.as_str() {
                "flex-start" | "start" => AlignItems::FlexStart,
                "flex-end" | "end" => AlignItems::FlexEnd,
                "center" => AlignItems::Center,
                "baseline" => AlignItems::Baseline,
                "stretch" => AlignItems::Stretch,
                _ => AlignItems::Stretch,
            };
        }
        if let Some(ac) = get_str("alignContent") {
            style.align_content = match ac.as_str() {
                "flex-start" | "start" => AlignContent::FlexStart,
                "flex-end" | "end" => AlignContent::FlexEnd,
                "center" => AlignContent::Center,
                "stretch" => AlignContent::Stretch,
                "space-between" => AlignContent::SpaceBetween,
                "space-around" => AlignContent::SpaceAround,
                _ => AlignContent::Stretch,
            };
        }
        style.gap_row = get_f64_or("rowGap", 0.0);
        style.gap_column = get_f64_or("columnGap", 0.0);
        // shorthand: gap sets both
        if let Some(gap) = get_f64("gap") {
            if style.gap_row == 0.0 {
                style.gap_row = gap;
            }
            if style.gap_column == 0.0 {
                style.gap_column = gap;
            }
        }

        // Flex item
        style.flex_grow = get_f64_or("flexGrow", 0.0);
        style.flex_shrink = get_f64_or("flexShrink", 1.0);
        if let Some(fb) = hash.get_by_str("flexBasis") {
            style.flex_basis = self.parse_dimension(fb);
        }
        if let Some(als) = get_str("alignSelf") {
            style.align_self = match als.as_str() {
                "auto" => AlignSelf::Auto,
                "flex-start" | "start" => AlignSelf::FlexStart,
                "flex-end" | "end" => AlignSelf::FlexEnd,
                "center" => AlignSelf::Center,
                "baseline" => AlignSelf::Baseline,
                "stretch" => AlignSelf::Stretch,
                _ => AlignSelf::Auto,
            };
        }

        // Size
        if let Some(v) = hash.get_by_str("width") {
            style.width = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("height") {
            style.height = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("minWidth") {
            style.min_width = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("minHeight") {
            style.min_height = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("maxWidth") {
            style.max_width = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("maxHeight") {
            style.max_height = self.parse_dimension(v);
        }

        // Padding (number or [top, right, bottom, left])
        if let Some(v) = hash.get_by_str("padding") {
            style.padding = self.extract_spacing_f64(v);
        }
        if let Some(v) = hash.get_by_str("paddingTop") {
            style.padding[0] = v.to_number();
        }
        if let Some(v) = hash.get_by_str("paddingRight") {
            style.padding[1] = v.to_number();
        }
        if let Some(v) = hash.get_by_str("paddingBottom") {
            style.padding[2] = v.to_number();
        }
        if let Some(v) = hash.get_by_str("paddingLeft") {
            style.padding[3] = v.to_number();
        }

        // Margin (dimension or [top, right, bottom, left])
        if let Some(v) = hash.get_by_str("margin") {
            style.margin = self.extract_spacing_dim(v);
        }
        if let Some(v) = hash.get_by_str("marginTop") {
            style.margin[0] = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("marginRight") {
            style.margin[1] = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("marginBottom") {
            style.margin[2] = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("marginLeft") {
            style.margin[3] = self.parse_dimension(v);
        }

        // Border widths
        if let Some(v) = hash.get_by_str("borderWidth") {
            style.border = self.extract_spacing_f64(v);
        }
        if let Some(v) = hash.get_by_str("borderTopWidth") {
            style.border[0] = v.to_number();
        }
        if let Some(v) = hash.get_by_str("borderRightWidth") {
            style.border[1] = v.to_number();
        }
        if let Some(v) = hash.get_by_str("borderBottomWidth") {
            style.border[2] = v.to_number();
        }
        if let Some(v) = hash.get_by_str("borderLeftWidth") {
            style.border[3] = v.to_number();
        }

        // Inset (for absolute positioning)
        if let Some(v) = hash.get_by_str("top") {
            style.inset[0] = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("right") {
            style.inset[1] = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("bottom") {
            style.inset[2] = self.parse_dimension(v);
        }
        if let Some(v) = hash.get_by_str("left") {
            style.inset[3] = self.parse_dimension(v);
        }

        // Aspect ratio
        if let Some(v) = get_f64("aspectRatio") {
            style.aspect_ratio = Some(v);
        }

        // z-index
        if let Some(v) = get_f64("zIndex") {
            style.z_index = v as i32;
        }

        // order
        if let Some(v) = get_f64("order") {
            style.order = v as i32;
        }

        // Grid (basic) — grid_template_columns/rows as arrays of track strings
        // TODO: grid support can be expanded later

        style
    }

    /// Parse a Value as a Dimension: number → Points, "50%" → Percent, "auto" → Auto.
    fn parse_dimension(&self, val: Value) -> crate::layout_bridge::Dimension {
        use crate::layout_bridge::Dimension;
        if val.is_i32() || val.is_f64() {
            return Dimension::Points(val.to_number());
        }
        let s = val_inspect(val, &self.heap);
        if s == "auto" {
            Dimension::Auto
        } else if let Some(pct) = s.strip_suffix('%') {
            pct.parse::<f64>()
                .map(|v| Dimension::Percent(v / 100.0))
                .unwrap_or(Dimension::Auto)
        } else if let Ok(v) = s.parse::<f64>() {
            Dimension::Points(v)
        } else {
            Dimension::Auto
        }
    }

    /// Extract [top, right, bottom, left] f64 from a Value.
    /// If it's a single number, applies to all 4 sides.
    /// If it's an array, extracts up to 4 elements (CSS shorthand style).
    fn extract_spacing_f64(&self, val: Value) -> [f64; 4] {
        if val.is_i32() || val.is_f64() {
            let v = val.to_number();
            return [v, v, v, v];
        }
        if val.is_heap() {
            let obj = self.heap.get(val.heap_index());
            if let Object::Array(ref arr) = obj {
                let items = arr.borrow();
                return match items.len() {
                    0 => [0.0; 4],
                    1 => {
                        let v = items[0].to_number();
                        [v, v, v, v]
                    }
                    2 => {
                        let vert = items[0].to_number();
                        let horiz = items[1].to_number();
                        [vert, horiz, vert, horiz]
                    }
                    3 => {
                        let top = items[0].to_number();
                        let horiz = items[1].to_number();
                        let bottom = items[2].to_number();
                        [top, horiz, bottom, horiz]
                    }
                    _ => [
                        items[0].to_number(),
                        items[1].to_number(),
                        items[2].to_number(),
                        items[3].to_number(),
                    ],
                };
            }
        }
        [0.0; 4]
    }

    /// Extract [top, right, bottom, left] Dimension from a Value.
    fn extract_spacing_dim(&self, val: Value) -> [crate::layout_bridge::Dimension; 4] {
        use crate::layout_bridge::Dimension;
        if val.is_i32() || val.is_f64() {
            let d = Dimension::Points(val.to_number());
            return [d.clone(), d.clone(), d.clone(), d];
        }
        if val.is_heap() {
            let obj = self.heap.get(val.heap_index());
            if let Object::Array(ref arr) = obj {
                let items = arr.borrow();
                let parse = |i: usize| -> Dimension {
                    if let Some(&v) = items.get(i) {
                        self.parse_dimension(v)
                    } else {
                        Dimension::Auto
                    }
                };
                return match items.len() {
                    0 => [
                        Dimension::Auto,
                        Dimension::Auto,
                        Dimension::Auto,
                        Dimension::Auto,
                    ],
                    1 => {
                        let d = parse(0);
                        [d.clone(), d.clone(), d.clone(), d]
                    }
                    2 => {
                        let vert = parse(0);
                        let horiz = parse(1);
                        [vert.clone(), horiz.clone(), vert, horiz]
                    }
                    3 => {
                        let top = parse(0);
                        let horiz = parse(1);
                        let bottom = parse(2);
                        [top, horiz.clone(), bottom, horiz]
                    }
                    _ => [parse(0), parse(1), parse(2), parse(3)],
                };
            }
        }
        [
            Dimension::Auto,
            Dimension::Auto,
            Dimension::Auto,
            Dimension::Auto,
        ]
    }

    fn json_parse(&mut self, source: &str) -> Result<Object, VMError> {
        let parsed: JsonValue = serde_json::from_str(source)
            .map_err(|e| VMError::TypeError(format!("JSON.parse error: {}", e)))?;
        Ok(self.json_value_to_object(parsed))
    }

    fn object_to_json_value(&self, value: &Object) -> JsonValue {
        match value {
            Object::Null | Object::Undefined => JsonValue::Null,
            Object::Boolean(v) => JsonValue::Bool(*v),
            Object::Integer(v) => JsonValue::from(*v),
            Object::Float(v) => {
                serde_json::Number::from_f64(*v).map_or(JsonValue::Null, JsonValue::Number)
            }
            Object::String(s) => JsonValue::String(s.to_string()),
            Object::Array(items) => JsonValue::Array(
                items
                    .borrow()
                    .iter()
                    .map(|item| {
                        let obj = val_to_obj(*item, &self.heap);
                        self.object_to_json_value(&obj)
                    })
                    .collect(),
            ),
            Object::Hash(hash) => {
                let hash_b = hash.borrow_mut();
                hash_b.sync_pairs_if_dirty();
                let mut map = serde_json::Map::new();
                for k in hash_b.ordered_keys_ref() {
                    let v = hash_b.pairs.get(&k).expect("hash key_order out of sync");
                    let obj = val_to_obj(*v, &self.heap);
                    let key = k.display_key();
                    map.insert(key, self.object_to_json_value(&obj));
                }
                JsonValue::Object(map)
            }
            _ => JsonValue::Null,
        }
    }

    fn json_value_to_object(&mut self, value: JsonValue) -> Object {
        match value {
            JsonValue::Null => Object::Null,
            JsonValue::Bool(v) => Object::Boolean(v),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Object::Integer(i)
                } else {
                    Object::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            JsonValue::String(s) => Object::String(s.into()),
            JsonValue::Array(arr) => make_array(
                arr.into_iter()
                    .map(|v| {
                        let obj = self.json_value_to_object(v);
                        obj_into_val(obj, &mut self.heap)
                    })
                    .collect(),
            ),
            JsonValue::Object(obj) => {
                let mut hash = crate::object::HashObject::default();
                for (k, v) in obj {
                    let child = self.json_value_to_object(v);
                    let val = obj_into_val(child, &mut self.heap);
                    hash.insert_pair(HashKey::from_owned_string(k), val);
                }
                make_hash(hash)
            }
        }
    }

    /// Convert a DbRecord into a VM HashObject matching the web XDB format:
    /// `{ id, collection, data: { ...fields }, created_at, updated_at }`
    fn db_record_to_object(&mut self, record: crate::db_bridge::DbRecord) -> Object {
        let mut hash = crate::object::HashObject::default();
        let id_val = obj_into_val(Object::String(record.id.into()), &mut self.heap);
        hash.insert_pair(HashKey::from_string("id"), id_val);
        let coll_val = obj_into_val(Object::String(record.collection.into()), &mut self.heap);
        hash.insert_pair(HashKey::from_string("collection"), coll_val);
        let created_val = obj_into_val(Object::String(record.created_at.into()), &mut self.heap);
        hash.insert_pair(HashKey::from_string("created_at"), created_val);
        let updated_val = obj_into_val(Object::String(record.updated_at.into()), &mut self.heap);
        hash.insert_pair(HashKey::from_string("updated_at"), updated_val);
        // Parse data and wrap in a `.data` property (matching web XDB record format).
        // Prefer pre-parsed data_parsed (avoids redundant JSON string round-trip).
        let data_json_val = if let Some(parsed) = record.data_parsed {
            Some(parsed)
        } else if !record.data.is_empty() {
            serde_json::from_str::<JsonValue>(&record.data).ok()
        } else {
            None
        };
        if let Some(parsed) = data_json_val {
            let data_obj = self.json_value_to_object(parsed);
            let data_val = obj_into_val(data_obj, &mut self.heap);
            hash.insert_pair(HashKey::from_string("data"), data_val);
        } else {
            let empty = make_hash(crate::object::HashObject::default());
            let data_val = obj_into_val(empty, &mut self.heap);
            hash.insert_pair(HashKey::from_string("data"), data_val);
        }
        make_hash(hash)
    }

    fn build_regex(&self, pattern: &str, flags: &str) -> Result<regex::Regex, VMError> {
        let mut builder = RegexBuilder::new(pattern);
        for flag in flags.chars() {
            match flag {
                'i' => {
                    builder.case_insensitive(true);
                }
                'm' => {
                    builder.multi_line(true);
                }
                's' => {
                    builder.dot_matches_new_line(true);
                }
                'u' | 'g' => {}
                _ => {
                    return Err(VMError::TypeError(format!(
                        "unsupported regex flag '{}'",
                        flag
                    )))
                }
            }
        }

        builder
            .build()
            .map_err(|e| VMError::TypeError(format!("invalid regex: {}", e)))
    }

    /// Push a call frame for a compiled function, set up the new function's
    /// state, and return. The caller should `continue` the dispatch loop.
    /// Args must be pre-staged in `self.arg_buffer` via `stage_call_args`.
    /// The return address (ip after OpCall) is saved in the frame.
    #[inline(never)]
    fn push_call_frame(
        &mut self,
        func: CompiledFunctionObject,
        receiver: Option<Object>,
    ) -> Result<(), VMError> {
        if self.frames.len() >= MAX_FRAMES {
            return Err(VMError::StackOverflow);
        }
        // Check execution limits on function calls (catches infinite recursion).
        if self.enforce_limits {
            self.check_execution_limits()?;
        }

        let CompiledFunctionObject {
            instructions,
            constants,
            num_locals,
            num_parameters,
            rest_parameter_index,
            takes_this,
            is_async,
            is_generator: _,
            num_cache_slots,
            max_stack_depth,
            register_count: _,
            inline_cache: func_cache,
            closure_captures: _,
            captured_values: _,
        } = func;

        // Set up locals for the new function.
        // Args are in self.arg_buffer, populated by stage_call_args.
        let arg_offset = if takes_this { 1 } else { 0 };
        let init_local_count = num_parameters + arg_offset;
        let needed = num_locals.max(init_local_count);
        let mut new_locals = self
            .locals_pool
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(needed));
        new_locals.resize(needed, Object::Undefined);

        if let Some(rest_i) = rest_parameter_index {
            let need = rest_i + arg_offset + 1;
            if new_locals.len() < need {
                new_locals.resize(need, Object::Undefined);
            }
        }

        if takes_this {
            new_locals[0] = receiver.unwrap_or(Object::Undefined);
        }

        // Populate locals from arg_buffer. We do this before the state save
        // because arg_buffer is consumed and cleared at the end.
        if let Some(rest_i) = rest_parameter_index {
            let rest_local = rest_i + arg_offset;
            let mut rest_values: Vec<Value> =
                Vec::with_capacity(self.arg_buffer.len().saturating_sub(rest_i));
            for arg in self.arg_buffer.iter().skip(rest_i) {
                rest_values.push(*arg);
            }
            new_locals[rest_local] = make_array(rest_values);
            // Rest parameter path: convert Value args to Object for locals.
            let positional_count = self.arg_buffer.len().min(rest_i);
            let positional_count =
                positional_count.min(new_locals.len().saturating_sub(arg_offset));
            for i in 0..positional_count {
                let target = i + arg_offset;
                unsafe {
                    *new_locals.get_unchecked_mut(target) =
                        val_to_obj(*self.arg_buffer.get_unchecked(i), &self.heap)
                };
            }
        } else {
            // No rest parameter: convert Value args to Object for locals.
            let positional_count = self
                .arg_buffer
                .len()
                .min(new_locals.len().saturating_sub(arg_offset));
            for i in 0..positional_count {
                let target = i + arg_offset;
                unsafe {
                    *new_locals.get_unchecked_mut(target) =
                        val_to_obj(*self.arg_buffer.get_unchecked(i), &self.heap);
                };
            }
            self.arg_buffer.clear();
        }

        // Stack bounds check for the new function.
        if max_stack_depth > 0 && self.sp + max_stack_depth as usize > STACK_SIZE {
            // Return locals to pool before failing.
            new_locals.clear();
            self.locals_pool.push(new_locals);
            return Err(VMError::StackOverflow);
        }

        // Save current state into a CallFrame.
        // Return address is ip + 2 (past the OpCall [opcode + 1-byte num_args]).
        let frame = if num_cache_slots > 0 {
            // Function uses inline property cache: swap in the warm cache.
            CallFrame {
                ip: self.ip + 2,
                instructions: std::mem::replace(&mut self.instructions, instructions),
                constants: std::mem::replace(&mut self.constants, constants),
                locals: std::mem::replace(&mut self.locals, new_locals),
                sp: self.sp,
                inline_cache: std::mem::replace(
                    &mut self.inline_cache,
                    std::mem::take(func_cache.borrow_mut()),
                ),
                max_stack_depth: self.max_stack_depth,
                func_cache: Some(func_cache),
                is_async,
            }
        } else {
            // Function has no property accesses: skip the cache swap entirely.
            // The VM's current inline_cache is left as-is (callee won't index it).
            CallFrame {
                ip: self.ip + 2,
                instructions: std::mem::replace(&mut self.instructions, instructions),
                constants: std::mem::replace(&mut self.constants, constants),
                locals: std::mem::replace(&mut self.locals, new_locals),
                sp: self.sp,
                inline_cache: Vec::new(),
                max_stack_depth: self.max_stack_depth,
                func_cache: None,
                is_async,
            }
        };
        self.frames.push(frame);

        // Set up new function state.
        self.ip = 0;
        self.inst_ptr = self.instructions.as_ptr();
        self.inst_len = self.instructions.len();
        self.max_stack_depth = max_stack_depth as usize;

        // Clear arg_buffer since args have been consumed.
        self.arg_buffer.clear();

        Ok(())
    }

    /// Like `push_call_frame`, but reads args directly from the VM stack
    /// instead of from `self.arg_buffer`. This eliminates the intermediate
    /// staging step (stack → arg_buffer → locals becomes stack → locals).
    /// Used by `OpCallGlobal` for non-async, non-rest-parameter calls.
    ///
    /// Takes the function by reference to avoid cloning the entire
    /// `CompiledFunctionObject` upfront. Only clones the `Rc` fields that
    /// actually need separate ownership (instructions, constants, and
    /// optionally inline_cache).
    #[inline(never)]
    fn push_call_frame_direct(
        &mut self,
        func: &CompiledFunctionObject,
        num_args: usize,
    ) -> Result<(), VMError> {
        if self.frames.len() >= MAX_FRAMES {
            return Err(VMError::StackOverflow);
        }
        if self.enforce_limits {
            self.check_execution_limits()?;
        }

        let num_locals = func.num_locals;
        let num_parameters = func.num_parameters;
        let num_cache_slots = func.num_cache_slots;
        let max_stack_depth = func.max_stack_depth;

        let needed = num_locals.max(num_parameters);
        let mut new_locals = self
            .locals_pool
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(needed));

        // Pre-size the locals Vec, then directly move args from the stack.
        // We avoid resize() + overwrite by using set_len + ptr::write for
        // the arg slots, then filling the remaining slots with Undefined.
        if new_locals.capacity() < needed {
            new_locals.reserve(needed - new_locals.capacity());
        }
        unsafe { new_locals.set_len(0) };

        // Move args directly from the stack into locals (no intermediate buffer).
        // Stack holds Value; locals hold Object — convert at boundary.
        let args_start = self.sp - num_args;
        let positional_count = num_args.min(needed);
        for i in 0..positional_count {
            let val = unsafe { *self.stack.as_ptr().add(args_start + i) };
            new_locals.push(val_to_obj(val, &self.heap));
        }
        // Fill remaining locals with Undefined.
        for _ in positional_count..needed {
            new_locals.push(Object::Undefined);
        }
        // Shrink the stack (args consumed).
        unsafe { self.stack.set_len(args_start) };

        // Stack bounds check for the new function.
        if max_stack_depth > 0 && self.sp + max_stack_depth as usize > STACK_SIZE {
            new_locals.clear();
            self.locals_pool.push(new_locals);
            return Err(VMError::StackOverflow);
        }

        let frame = if num_cache_slots > 0 {
            let func_cache = Rc::clone(&func.inline_cache);
            CallFrame {
                ip: self.ip + 2,
                instructions: std::mem::replace(
                    &mut self.instructions,
                    Rc::clone(&func.instructions),
                ),
                constants: std::mem::replace(&mut self.constants, Rc::clone(&func.constants)),
                locals: std::mem::replace(&mut self.locals, new_locals),
                sp: args_start,
                inline_cache: std::mem::replace(
                    &mut self.inline_cache,
                    std::mem::take(func_cache.borrow_mut()),
                ),
                max_stack_depth: self.max_stack_depth,
                func_cache: Some(func_cache),
                is_async: false,
            }
        } else {
            CallFrame {
                ip: self.ip + 2,
                instructions: std::mem::replace(
                    &mut self.instructions,
                    Rc::clone(&func.instructions),
                ),
                constants: std::mem::replace(&mut self.constants, Rc::clone(&func.constants)),
                locals: std::mem::replace(&mut self.locals, new_locals),
                sp: args_start,
                inline_cache: Vec::new(),
                max_stack_depth: self.max_stack_depth,
                func_cache: None,
                is_async: false,
            }
        };
        self.frames.push(frame);

        self.ip = 0;
        self.inst_ptr = self.instructions.as_ptr();
        self.inst_len = self.instructions.len();
        self.sp = args_start;
        self.max_stack_depth = max_stack_depth as usize;

        Ok(())
    }

    /// Fast path for register→register function calls. Copies Values directly
    /// from caller registers into callee register window without any Object
    /// conversion. Returns the function result as a Value.
    ///
    /// `arg_stack_start` — absolute stack index of the first arg Value.
    /// `nargs` — number of arguments.
    /// `receiver_val` — optional `this` value (already a Value).
    ///
    /// # Safety
    /// - `instr_ptr` must point to valid bytecode of length `instr_len`.
    /// - `constants_raw` must point to a valid `Vec<Object>`.
    /// - `func_cache` must point to a valid `VmCell<Vec<(u32,u32)>>` on the heap.
    pub unsafe fn call_register_direct(
        &mut self,
        instr_ptr: *const u8,
        instr_len: usize,
        constants_raw: *const Vec<Object>,
        rest_parameter_index: Option<usize>,
        takes_this: bool,
        is_async: bool,
        num_cache_slots: u16,
        max_stack_depth: u16,
        register_count: u16,
        func_cache: *const crate::object::VmCell<Vec<(u32, u32)>>,
        arg_stack_start: usize,
        nargs: usize,
        receiver_val: Option<Value>,
    ) -> Result<Value, VMError> {
        // Set up register window (callee registers are above the caller's sp)
        let reg_base = self.sp;
        let reg_window = (register_count as usize).max(1);

        if reg_base + reg_window > STACK_SIZE {
            return Err(VMError::StackOverflow);
        }

        // ── Self-recursion fast path ─────────────────────────────────
        // When a function calls itself, inst_ptr/inst_len/constants/cache
        // are all identical — skip their save/restore entirely.
        // Also shares the inline cache with the recursive call for better
        // hit rates (the function's VmCell cache was already emptied on
        // the initial call entry).
        let is_self_call =
            instr_ptr == self.inst_ptr && !is_async && rest_parameter_index.is_none();
        if is_self_call {
            let saved_ip = self.ip;
            let saved_sp = self.sp;

            self.ip = 0;

            // Ensure stack fits
            let needed = reg_base + reg_window;
            if self.stack.len() < needed {
                self.stack.resize(needed, Value::UNDEFINED);
            }

            let arg_offset = if takes_this { 1 } else { 0 };
            if takes_this {
                unsafe {
                    *self.stack.get_unchecked_mut(reg_base) =
                        receiver_val.unwrap_or(Value::UNDEFINED)
                };
            }

            // Copy args directly via ptr::copy_nonoverlapping. Safe because
            // reg_base = self.sp >= arg_stack_start + nargs (non-overlapping).
            if nargs > 0 {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.stack.as_ptr().add(arg_stack_start),
                        self.stack.as_mut_ptr().add(reg_base + arg_offset),
                        nargs,
                    );
                }
            }
            let first_uninit = nargs + arg_offset;
            for i in first_uninit..reg_window {
                unsafe { *self.stack.get_unchecked_mut(reg_base + i) = Value::UNDEFINED };
            }

            self.sp = reg_base + reg_window;
            let entry_depth = self.frames.len();
            let run_result = self.rdispatch_loop(entry_depth, reg_base);
            let rv = self.last_popped.take().unwrap_or(Value::UNDEFINED);

            self.ip = saved_ip;
            self.sp = saved_sp;

            run_result?;
            return Ok(rv);
        }

        // ── Normal call path ─────────────────────────────────────────
        // Args stay in place at arg_stack_start — we'll copy directly to the
        // callee's register window after resize (non-overlapping regions).

        // Save current VM state — raw pointer swaps, zero Rc clones
        let saved_ip = self.ip;
        let saved_inst_ptr = self.inst_ptr;
        let saved_inst_len = self.inst_len;
        self.inst_ptr = instr_ptr;
        self.inst_len = instr_len;
        let saved_constants_raw = self.constants_raw;
        self.constants_raw = constants_raw;
        let saved_cv_ptr = self.constants_values_ptr;
        let saved_cs_ptr = self.constants_syms_ptr;
        self.preconvert_constants();
        // Register functions don't use self.locals — skip save/restore for perf.
        // The parent (rdispatch_loop) also doesn't use locals.
        let saved_sp = self.sp;
        // Skip last_popped save — in register→register calls, it's always None
        // (the result of the previous call was already stored in a register).
        let saved_max_stack_depth = self.max_stack_depth;
        let saved_inline_cache = if num_cache_slots > 0 {
            // SAFETY: func_cache points to a VmCell in a CompiledFunctionObject on the heap.
            // The heap is append-only so the pointer remains valid.
            let taken = std::mem::take(unsafe { &*func_cache }.borrow_mut());
            if taken.is_empty() && num_cache_slots > 0 {
                // Self-recursive call via normal path (e.g. async or rest-param
                // function): the VmCell was already emptied by an outer
                // activation. Allocate a fresh cache for this frame.
                std::mem::replace(
                    &mut self.inline_cache,
                    vec![(0, 0); num_cache_slots as usize],
                )
            } else {
                std::mem::replace(&mut self.inline_cache, taken)
            }
        } else {
            Vec::new()
        };

        self.ip = 0;
        self.max_stack_depth = max_stack_depth as usize;

        // Ensure stack fits register window.
        let needed = reg_base + reg_window;
        if self.stack.len() < needed {
            self.stack.resize(needed, Value::UNDEFINED);
        }

        let arg_offset = if takes_this { 1 } else { 0 };

        // Copy 'this' into register 0
        if takes_this {
            unsafe {
                *self.stack.get_unchecked_mut(reg_base) = receiver_val.unwrap_or(Value::UNDEFINED)
            };
        }

        // Copy positional args directly from caller's stack region to callee's
        // register window via ptr::copy_nonoverlapping. Safe because:
        // - reg_base = saved_sp >= arg_stack_start + nargs (non-overlapping)
        // - stack.resize() above ensures both regions are valid
        let positional_count = rest_parameter_index.map_or(nargs, |ri| nargs.min(ri));
        if positional_count > 0 {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.stack.as_ptr().add(arg_stack_start),
                    self.stack.as_mut_ptr().add(reg_base + arg_offset),
                    positional_count,
                );
            }
        }

        // Only init remaining registers to UNDEFINED (skip those already set by args/this)
        let first_uninit = positional_count + arg_offset;
        for i in first_uninit..reg_window {
            unsafe { *self.stack.get_unchecked_mut(reg_base + i) = Value::UNDEFINED };
        }

        // Handle rest parameter — read from original stack position
        if let Some(rest_i) = rest_parameter_index {
            let rest_reg = rest_i + arg_offset;
            let mut rest_values: Vec<Value> = Vec::with_capacity(nargs.saturating_sub(rest_i));
            for i in rest_i..nargs {
                rest_values.push(unsafe { *self.stack.get_unchecked(arg_stack_start + i) });
            }
            unsafe {
                *self.stack.get_unchecked_mut(reg_base + rest_reg) =
                    obj_into_val(make_array(rest_values), &mut self.heap)
            };
        }

        // Set sp past register window
        self.sp = reg_base + reg_window;

        // Execute register dispatch
        let entry_depth = self.frames.len();
        let run_result = self.rdispatch_loop(entry_depth, reg_base);

        // Extract return value directly as Value (no Object conversion!)
        let rv = self.last_popped.take().unwrap_or(Value::UNDEFINED);

        // Restore parent state — raw pointer swaps, zero Rc clones
        self.ip = saved_ip;
        self.inst_ptr = saved_inst_ptr;
        self.inst_len = saved_inst_len;
        self.constants_raw = saved_constants_raw;
        self.constants_values_ptr = saved_cv_ptr;
        self.constants_syms_ptr = saved_cs_ptr;
        self.max_stack_depth = saved_max_stack_depth;
        if num_cache_slots > 0 {
            // SAFETY: func_cache points to a VmCell in a CompiledFunctionObject on the heap.
            // The heap is append-only so the pointer remains valid.
            let our_cache = std::mem::replace(&mut self.inline_cache, saved_inline_cache);
            let fc = unsafe { &*func_cache }.borrow_mut();
            // Only write back if the VmCell is still empty (i.e. we're the
            // outermost activation that originally took the cache). For
            // inner self-recursive activations that allocated a fresh cache,
            // the VmCell was already restored by the outer activation — skip.
            if fc.is_empty() {
                *fc = our_cache;
            }
        }
        // Skip locals restore — register functions don't use locals.
        self.sp = saved_sp;
        // last_popped was consumed by the rv extraction above; no restore needed.

        // Handle async
        if is_async {
            let rv_obj = val_to_obj(rv, &self.heap);
            let promise = match run_result {
                Ok(()) => PromiseObject {
                    settled: PromiseState::Fulfilled(Box::new(rv_obj)),
                },
                Err(err) => PromiseObject {
                    settled: PromiseState::Rejected(Box::new(Object::Error(Box::new(
                        crate::object::ErrorObject {
                            name: Rc::from("Error"),
                            message: Rc::from(format!("{:?}", err)),
                        },
                    )))),
                },
            };
            return Ok(obj_into_val(
                Object::Promise(Box::new(promise)),
                &mut self.heap,
            ));
        }

        run_result?;
        Ok(rv)
    }

    pub(crate) fn execute_compiled_function_slice(
        &mut self,
        func: CompiledFunctionObject,
        args: &[Value],
        receiver: Option<Value>,
    ) -> Result<(Value, Option<Value>), VMError> {
        let CompiledFunctionObject {
            instructions,
            constants,
            num_locals,
            num_parameters,
            rest_parameter_index,
            takes_this,
            is_async,
            is_generator: _,
            num_cache_slots,
            max_stack_depth,
            register_count,
            inline_cache: func_cache,
            closure_captures: _,
            captured_values: _,
        } = func;

        let is_register = register_count > 0;

        // Save current VM state
        let saved_ip = self.ip;
        let saved_inst_ptr = self.inst_ptr;
        let saved_inst_len = self.inst_len;
        let saved_instructions = std::mem::replace(&mut self.instructions, instructions);
        self.inst_ptr = self.instructions.as_ptr();
        self.inst_len = self.instructions.len();
        let saved_constants = std::mem::replace(&mut self.constants, constants);
        let saved_constants_raw = self.constants_raw;
        let saved_cv_ptr = self.constants_values_ptr;
        let saved_cs_ptr = self.constants_syms_ptr;
        if is_register {
            self.constants_raw = &*self.constants as *const Vec<Object>;
            self.preconvert_constants();
        }
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_sp = self.sp;
        let saved_last_popped = self.last_popped.take();
        let saved_max_stack_depth = self.max_stack_depth;
        let saved_inline_cache = if num_cache_slots > 0 {
            let mut taken = std::mem::take(func_cache.borrow_mut());
            // If the cache was already taken (recursive call), allocate fresh
            if taken.is_empty() {
                taken = vec![(0, 0); num_cache_slots as usize];
            }
            std::mem::replace(&mut self.inline_cache, taken)
        } else {
            Vec::new()
        };

        self.ip = 0;
        self.max_stack_depth = max_stack_depth as usize;
        let arg_offset = if takes_this { 1 } else { 0 };
        let rest_index = rest_parameter_index;

        let (run_result, return_value, receiver_after) = if is_register {
            // ── Register-based function ──────────────────────────────
            let reg_base = self.sp;
            // Ensure register window is large enough for both the function's
            // registers AND the argument slots (this + positional args + rest).
            let positional_count = rest_index.map_or(args.len(), |ri| args.len().min(ri));
            let arg_slots = positional_count + arg_offset;
            let rest_slots = rest_index.map_or(0, |ri| ri + arg_offset + 1);
            let reg_window = (register_count as usize).max(1).max(arg_slots).max(rest_slots);

            // Stack bounds check
            if reg_base + reg_window > STACK_SIZE {
                self.ip = saved_ip;
                self.instructions = saved_instructions;
                self.inst_ptr = saved_inst_ptr;
                self.inst_len = saved_inst_len;
                self.constants = saved_constants;
                self.constants_raw = saved_constants_raw;
                self.constants_values_ptr = saved_cv_ptr;
                self.constants_syms_ptr = saved_cs_ptr;
                self.max_stack_depth = saved_max_stack_depth;
                if num_cache_slots > 0 {
                    *func_cache.borrow_mut() =
                        std::mem::replace(&mut self.inline_cache, saved_inline_cache);
                }
                self.locals = saved_locals;
                self.last_popped = saved_last_popped;
                return Err(VMError::StackOverflow);
            }

            // Extend stack to fit register window
            while self.stack.len() < reg_base + reg_window {
                self.stack.push(Value::UNDEFINED);
            }

            // Initialize register window to UNDEFINED
            for i in 0..reg_window {
                self.stack[reg_base + i] = Value::UNDEFINED;
            }

            // Copy 'this' into register 0
            if takes_this {
                self.stack[reg_base] = receiver.unwrap_or(Value::UNDEFINED);
            }

            // Copy positional args into registers (window already sized to fit)
            for (i, arg) in args.iter().take(positional_count).enumerate() {
                self.stack[reg_base + i + arg_offset] = *arg;
            }

            // Handle rest parameter
            if let Some(rest_i) = rest_index {
                let rest_reg = rest_i + arg_offset;
                let rest_values: Vec<Value> = args.iter().skip(rest_i).copied().collect();
                self.stack[reg_base + rest_reg] =
                    obj_into_val(make_array(rest_values), &mut self.heap);
            }

            // No locals needed for register VM
            self.locals = Vec::new();

            // Set sp past register window for scratch use
            self.sp = reg_base + reg_window;

            // Execute register dispatch
            let entry_depth = self.frames.len();
            let rr = self.rdispatch_loop(entry_depth, reg_base);

            // Extract results � return Value directly
            let rv = self.last_popped.take().unwrap_or(Value::UNDEFINED);
            let ra = if takes_this {
                Some(self.stack[reg_base])
            } else {
                None
            };

            (rr, rv, ra)
        } else {
            // ── Stack-based function ─────────────────────────────────
            let init_local_count = num_parameters + arg_offset;
            let needed = num_locals.max(init_local_count);
            let mut new_locals = self
                .locals_pool
                .pop()
                .unwrap_or_else(|| Vec::with_capacity(needed));
            new_locals.resize(needed, Object::Undefined);

            if let Some(rest_i) = rest_parameter_index {
                let need = rest_i + arg_offset + 1;
                if new_locals.len() < need {
                    new_locals.resize(need, Object::Undefined);
                }
            }

            if takes_this {
                new_locals[0] = val_to_obj(receiver.unwrap_or(Value::UNDEFINED), &self.heap);
            }

            if let Some(rest_i) = rest_index {
                let rest_local = rest_i + arg_offset;
                let rest_values: Vec<Value> = args.iter().skip(rest_i).copied().collect();
                new_locals[rest_local] = make_array(rest_values);
            }

            let positional_count = rest_index.map_or(args.len(), |rest_i| args.len().min(rest_i));
            let positional_count =
                positional_count.min(new_locals.len().saturating_sub(arg_offset));
            for (i, arg) in args.iter().take(positional_count).enumerate() {
                let target = i + arg_offset;
                unsafe { *new_locals.get_unchecked_mut(target) = val_to_obj(*arg, &self.heap) };
            }

            self.locals = new_locals;

            if max_stack_depth > 0 && self.sp + max_stack_depth as usize > STACK_SIZE {
                self.ip = saved_ip;
                self.instructions = saved_instructions;
                self.inst_ptr = saved_inst_ptr;
                self.inst_len = saved_inst_len;
                self.constants = saved_constants;
                self.constants_raw = saved_constants_raw;
                self.max_stack_depth = saved_max_stack_depth;
                if num_cache_slots > 0 {
                    *func_cache.borrow_mut() =
                        std::mem::replace(&mut self.inline_cache, saved_inline_cache);
                }
                let mut used_locals = std::mem::replace(&mut self.locals, saved_locals);
                used_locals.clear();
                self.locals_pool.push(used_locals);
                self.stack.truncate(saved_sp);
                self.sp = saved_sp;
                self.last_popped = saved_last_popped;
                return Err(VMError::StackOverflow);
            }

            let rr = self.run();

            let rv = self.last_popped.take().unwrap_or(Value::UNDEFINED);
            let ra = if takes_this {
                if self.locals.is_empty() {
                    None
                } else {
                    let obj = std::mem::replace(&mut self.locals[0], Object::Undefined);
                    Some(obj_into_val(obj, &mut self.heap))
                }
            } else {
                None
            };

            (rr, rv, ra)
        };

        // Restore parent state
        self.ip = saved_ip;
        self.instructions = saved_instructions;
        // Restore inst_ptr/inst_len from saved values, NOT from self.instructions.
        // When the caller was entered via call_register_direct (register-based),
        // self.inst_ptr points to the caller's Rc<Vec<u8>> data (not self.instructions).
        // Restoring from self.instructions.as_ptr() would point to the wrong bytecode.
        self.inst_ptr = saved_inst_ptr;
        self.inst_len = saved_inst_len;
        self.constants = saved_constants;
        self.constants_raw = saved_constants_raw;
        self.constants_values_ptr = saved_cv_ptr;
        self.constants_syms_ptr = saved_cs_ptr;
        self.max_stack_depth = saved_max_stack_depth;
        if num_cache_slots > 0 {
            *func_cache.borrow_mut() =
                std::mem::replace(&mut self.inline_cache, saved_inline_cache);
        }
        let mut used_locals = std::mem::replace(&mut self.locals, saved_locals);
        used_locals.clear();
        self.locals_pool.push(used_locals);
        self.stack.truncate(saved_sp);
        self.sp = saved_sp;
        self.last_popped = saved_last_popped;

        // Handle async/errors
        if is_async {
            let return_obj = val_to_obj(return_value, &self.heap);
            let promise = match run_result {
                Ok(()) => PromiseObject {
                    settled: PromiseState::Fulfilled(Box::new(return_obj)),
                },
                Err(err) => PromiseObject {
                    settled: PromiseState::Rejected(Box::new(Object::Error(Box::new(
                        crate::object::ErrorObject {
                            name: Rc::from("Error"),
                            message: Rc::from(format!("{:?}", err)),
                        },
                    )))),
                },
            };
            let promise_val = obj_into_val(Object::Promise(Box::new(promise)), &mut self.heap);
            return Ok((promise_val, receiver_after));
        }

        run_result?;
        Ok((return_value, receiver_after))
    }

    fn execute_new(&mut self, num_args: usize) -> Result<(), VMError> {
        let callee_val = self.stage_call_args(num_args)?;
        let callee = val_to_obj(callee_val, &self.heap);
        let mut args = std::mem::take(&mut self.arg_buffer);
        let result = self.execute_new_with_args_slice(callee, &args);
        args.clear();
        self.arg_buffer = args;
        result
    }

    fn execute_new_with_args(&mut self, callee: Object, args: Vec<Value>) -> Result<(), VMError> {
        self.execute_new_with_args_slice(callee, &args)
    }

    pub(crate) fn execute_new_with_args_slice(
        &mut self,
        callee: Object,
        args: &[Value],
    ) -> Result<(), VMError> {
        // Save and set new.target for the duration of this constructor call.
        let saved_new_target = self.new_target;

        match callee {
            Object::Class(class_obj) => {
                // Set new.target to the class itself (clone before destructuring).
                self.new_target = obj_into_val(Object::Class(class_obj.clone()), &mut self.heap);

                let crate::object::ClassObject {
                    name,
                    parent_chain,
                    constructor,
                    methods,
                    getters,
                    setters,
                    super_methods,
                    super_getters,
                    super_setters,
                    super_constructor_chain,
                    field_initializers,
                    ..
                } = *class_obj;

                let mut instance = crate::object::InstanceObject {
                    class_name: name,
                    parent_chain,
                    fields: rustc_hash::FxHashMap::default(),
                    methods,
                    getters,
                    setters,
                    super_methods,
                    super_getters,
                    super_setters,
                    super_constructor_chain,
                };

                // Run instance field initializers (parent fields first, then own)
                for (field_name, thunk) in &field_initializers {
                    let receiver_val =
                        obj_into_val(Object::Instance(Box::new(instance.clone())), &mut self.heap);
                    let (result, receiver_after) = self.execute_compiled_function_slice(
                        thunk.clone(),
                        &[],
                        Some(receiver_val),
                    )?;
                    // Update instance from receiver_after (in case thunk mutated this)
                    if let Some(ra) = receiver_after {
                        if let Object::Instance(updated) = val_to_obj(ra, &self.heap) {
                            instance = *updated;
                        }
                    }
                    let result_obj = val_to_obj(result, &self.heap);
                    instance.fields.insert(field_name.clone(), result_obj);
                }

                if let Some(ctor) = constructor {
                    let receiver_val =
                        obj_into_val(Object::Instance(Box::new(instance.clone())), &mut self.heap);
                    let (_, receiver_after) =
                        self.execute_compiled_function_slice(ctor, args, Some(receiver_val))?;
                    if let Some(ra) = receiver_after {
                        if let Object::Instance(updated) = val_to_obj(ra, &self.heap) {
                            instance = *updated;
                        }
                    }
                }

                self.push(Object::Instance(Box::new(instance)))?;
                self.new_target = saved_new_target;
                Ok(())
            }
            Object::BuiltinFunction(builtin) => {
                self.new_target = saved_new_target;
                let out = self.execute_builtin_function_slice(*builtin, args)?;
                self.push_val(out)?;
                Ok(())
            }
            // Handle `new Date()` / `new Array()` — return appropriate objects
            Object::Hash(ref hash) => {
                let h = hash.borrow();
                let is_date = h.pairs.iter().any(|(k, _)| {
                    if let HashKey::Sym(sym) = k {
                        &*crate::intern::resolve(*sym) == "now"
                    } else { false }
                });
                let is_array = h.pairs.iter().any(|(k, _)| {
                    if let HashKey::Sym(sym) = k {
                        &*crate::intern::resolve(*sym) == "isArray"
                    } else { false }
                });
                let _ = h;
                if is_array {
                    // new Array() / new Array(n) / new Array(a, b, c)
                    let arr = if args.len() == 1 {
                        let arg = args[0];
                        let len = if arg.is_i32() {
                            (unsafe { arg.as_i32_unchecked() }) as usize
                        } else if arg.is_f64() {
                            arg.as_f64() as usize
                        } else {
                            // single non-numeric arg → array with that element
                            let items = vec![arg];
                            self.push(make_array(items))?;
                            self.new_target = saved_new_target;
                            return Ok(());
                        };
                        let items = vec![Value::UNDEFINED; len];
                        make_array(items)
                    } else if args.is_empty() {
                        make_array(vec![])
                    } else {
                        make_array(args.to_vec())
                    };
                    self.push(arr)?;
                    self.new_target = saved_new_target;
                    Ok(())
                } else if is_date {
                    // Support `new Date()`, `new Date(ms)`, `new Date(string)`
                    let ms = if args.is_empty() {
                        epoch_millis_now()
                    } else {
                        let arg = args[0];
                        if arg.is_f64() {
                            arg.as_f64()
                        } else if arg.is_i32() {
                            (unsafe { arg.as_i32_unchecked() }) as f64
                        } else if arg.is_heap() {
                            match self.heap.get(arg.heap_index()) {
                                Object::String(s) => {
                                    // Very basic ISO 8601 parse
                                    s.parse::<f64>().unwrap_or_else(|_| epoch_millis_now())
                                }
                                _ => epoch_millis_now(),
                            }
                        } else {
                            epoch_millis_now()
                        }
                    };
                    let time_obj = Object::Float(ms);
                    let mut date_hash = crate::object::HashObject::default();
                    date_hash.insert_pair(
                        HashKey::from_string("__time_ms"),
                        obj_into_val(Object::Float(ms), &mut self.heap),
                    );

                    // Helper macro for date methods: each gets receiver=Float(ms)
                    macro_rules! date_method {
                        ($name:expr, $func:ident) => {
                            date_hash.insert_pair(
                                HashKey::from_string($name),
                                obj_into_val(Object::BuiltinFunction(Box::new(
                                    crate::object::BuiltinFunctionObject {
                                        function: BuiltinFunction::$func,
                                        receiver: Some(time_obj.clone()),
                                    },
                                )), &mut self.heap),
                            );
                        }
                    }

                    date_method!("getTime", DateGetTime);
                    date_method!("getHours", DateGetHours);
                    date_method!("getMinutes", DateGetMinutes);
                    date_method!("getSeconds", DateGetSeconds);
                    date_method!("getMilliseconds", DateGetMilliseconds);
                    date_method!("getFullYear", DateGetFullYear);
                    date_method!("getMonth", DateGetMonth);
                    date_method!("getDate", DateGetDate);
                    date_method!("getDay", DateGetDay);
                    date_method!("toISOString", DateToISOString);
                    date_method!("toLocaleDateString", DateToLocaleDateString);
                    date_method!("toLocaleTimeString", DateToLocaleTimeString);
                    date_method!("toLocaleString", DateToLocaleString);
                    date_method!("toString", DateToString);
                    date_method!("valueOf", DateValueOf);

                    self.push(make_hash(date_hash))?;
                    self.new_target = saved_new_target;
                    Ok(())
                } else {
                    self.new_target = saved_new_target;
                    Err(VMError::TypeError("not a constructor: Hash".to_string()))
                }
            }
            other => {
                self.new_target = saved_new_target;
                Err(VMError::TypeError(format!(
                    "not a constructor: {:?}",
                    other.object_type()
                )))
            }
        }
    }

    pub(crate) fn execute_set_index(
        &mut self,
        left: Object,
        index: Object,
        value: Object,
    ) -> Result<(), VMError> {
        match left {
            Object::Array(items) => {
                if let Some(uidx) = Self::object_to_array_index(&index) {
                    let borrowed = items.borrow_mut();
                    if uidx >= borrowed.len() {
                        borrowed.resize(uidx + 1, Value::UNDEFINED);
                    }
                    borrowed[uidx] = obj_into_val(value, &mut self.heap);
                }

                self.push(Object::Array(items))?;
                Ok(())
            }
            Object::Hash(hash) => {
                // Check for setter accessor first (if the hash has any accessors).
                {
                    let hash_b = hash.borrow();
                    if hash_b.has_accessors() {
                        if let Object::String(s) = &index {
                            if let Some(setter) = hash_b.get_setter(s) {
                                let setter_func = setter.clone();
                                let _ = hash_b; // end borrow before calling accessor
                                let value_val = obj_into_val(value, &mut self.heap);
                                let receiver_val =
                                    obj_into_val(Object::Hash(Rc::clone(&hash)), &mut self.heap);
                                self.execute_compiled_function_slice(
                                    setter_func,
                                    std::slice::from_ref(&value_val),
                                    Some(receiver_val),
                                )?;
                                self.push(Object::Hash(hash))?;
                                return Ok(());
                            }
                        }
                    }
                }
                {
                    let hash_m = hash.borrow_mut();
                    let val = obj_into_val(value, &mut self.heap);
                    match &index {
                        Object::String(s) => {
                            hash_m.set_by_str(Rc::clone(s), val);
                        }
                        _ => {
                            let key = self.hash_key_from_object(&index);
                            hash_m.insert_pair(key, val);
                        }
                    }
                }
                self.push(Object::Hash(hash))?;
                Ok(())
            }
            Object::Instance(mut instance) => {
                let prop: Cow<str> = match &index {
                    Object::String(s) => Cow::Borrowed(s),
                    Object::Integer(v) => Cow::Owned(v.to_string()),
                    Object::Float(v) => Cow::Owned(v.to_string()),
                    Object::Boolean(v) => Cow::Owned(v.to_string()),
                    _ => {
                        self.push(Object::Instance(instance))?;
                        return Ok(());
                    }
                };
                if let Some(setter) = instance.setters.get(prop.as_ref()).cloned() {
                    let value_val = obj_into_val(value, &mut self.heap);
                    let receiver_val = obj_into_val(
                        Object::Instance(Box::new((*instance).clone())),
                        &mut self.heap,
                    );
                    let (_, receiver_after) = self.execute_compiled_function_slice(
                        setter,
                        std::slice::from_ref(&value_val),
                        Some(receiver_val),
                    )?;
                    if let Some(ra) = receiver_after {
                        if let Object::Instance(updated) = val_to_obj(ra, &self.heap) {
                            self.push(Object::Instance(updated))?;
                        } else {
                            self.push(Object::Instance(instance))?;
                        }
                    } else {
                        self.push(Object::Instance(instance))?;
                    }
                } else {
                    instance.fields.insert(prop.into_owned(), value);
                    self.push(Object::Instance(instance))?;
                }
                Ok(())
            }
            Object::Class(mut class_obj) => {
                let prop: Cow<str> = match &index {
                    Object::String(s) => Cow::Borrowed(s),
                    Object::Integer(v) => Cow::Owned(v.to_string()),
                    Object::Float(v) => Cow::Owned(v.to_string()),
                    Object::Boolean(v) => Cow::Owned(v.to_string()),
                    _ => {
                        self.push(Object::Class(class_obj))?;
                        return Ok(());
                    }
                };
                // Check for setter accessor first
                if let Some(setter) = class_obj.setters.get(prop.as_ref()).cloned() {
                    let value_val = obj_into_val(value, &mut self.heap);
                    let receiver_val = obj_into_val(
                        Object::Class(Box::new((*class_obj).clone())),
                        &mut self.heap,
                    );
                    self.execute_compiled_function_slice(
                        setter,
                        std::slice::from_ref(&value_val),
                        Some(receiver_val),
                    )?;
                    self.push(Object::Class(class_obj))?;
                } else {
                    class_obj.static_fields.insert(prop.into_owned(), value);
                    self.push(Object::Class(class_obj))?;
                }
                Ok(())
            }
            Object::SuperRef(mut super_ref) => {
                let prop: Cow<str> = match &index {
                    Object::String(s) => Cow::Borrowed(s),
                    Object::Integer(v) => Cow::Owned(v.to_string()),
                    Object::Float(v) => Cow::Owned(v.to_string()),
                    Object::Boolean(v) => Cow::Owned(v.to_string()),
                    _ => {
                        self.push(Object::SuperRef(super_ref))?;
                        return Ok(());
                    }
                };

                if let Some(setter) = super_ref.setters.get(prop.as_ref()).cloned() {
                    let value_val = obj_into_val(value, &mut self.heap);
                    let receiver_val = obj_into_val((*super_ref.receiver).clone(), &mut self.heap);
                    let (_, receiver_after) = self.execute_compiled_function_slice(
                        setter,
                        std::slice::from_ref(&value_val),
                        Some(receiver_val),
                    )?;
                    if let Some(ra) = receiver_after {
                        super_ref.receiver = Box::new(val_to_obj(ra, &self.heap));
                    }
                    self.push(Object::SuperRef(super_ref))?;
                    Ok(())
                } else {
                    self.push(Object::SuperRef(super_ref))?;
                    Ok(())
                }
            }
            _ => {
                self.push(left)?;
                Ok(())
            }
        }
    }

    pub(crate) fn execute_delete_property(
        &mut self,
        target: Object,
        key: Object,
    ) -> Result<(), VMError> {
        match target {
            Object::Hash(hash) => {
                let k = self.hash_key_from_object(&key);
                hash.borrow_mut().remove_pair(&k);
                self.push(Object::Hash(hash))?;
                Ok(())
            }
            Object::Array(arr) => {
                let idx = match key {
                    Object::Integer(v) => v,
                    Object::Float(v) if v.fract() == 0.0 => v as i64,
                    _ => {
                        self.push(Object::Array(arr))?;
                        return Ok(());
                    }
                };
                if idx >= 0 {
                    let uidx = idx as usize;
                    let arr_ref = arr.borrow_mut();
                    if uidx < arr_ref.len() {
                        arr_ref[uidx] = Value::UNDEFINED;
                    }
                }
                self.push(Object::Array(arr))?;
                Ok(())
            }
            Object::Instance(mut instance) => {
                let prop = self.object_key_cow(&key);
                instance.fields.remove(prop.as_ref());
                self.push(Object::Instance(instance))?;
                Ok(())
            }
            _ => {
                self.push(target)?;
                Ok(())
            }
        }
    }

    // ── Generator support ───────────────────────────────────────────────

    /// Create a `{value, done}` iterator result hash object.
    fn make_iterator_result(&mut self, value: Value, done: bool) -> Value {
        let mut hash = crate::object::HashObject::with_capacity(2);
        let sym_value = crate::intern::intern("value");
        let sym_done = crate::intern::intern("done");
        hash.insert_pair(crate::object::HashKey::Sym(sym_value), value);
        hash.insert_pair(
            crate::object::HashKey::Sym(sym_done),
            Value::from_bool(done),
        );
        let obj = Object::Hash(Rc::new(crate::object::VmCell::new(hash)));
        obj_into_val(obj, &mut self.heap)
    }

    /// Create a GeneratorObject from a generator function and its arguments.
    /// Returns the generator as a NaN-boxed Value.
    pub(crate) fn create_generator(
        &mut self,
        func: CompiledFunctionObject,
        args: Vec<Value>,
        receiver: Option<Value>,
    ) -> Value {
        use crate::object::{GeneratorObject, GeneratorState, VmCell};
        let gen = GeneratorObject {
            function: func,
            locals: Vec::new(),
            saved_ip: 0,
            args,
            receiver,
            state: GeneratorState::Created,
        };
        let obj = Object::Generator(Rc::new(VmCell::new(gen)));
        obj_into_val(obj, &mut self.heap)
    }

    /// Execute a generator `.next(arg)` call.
    ///
    /// On the first call (`Created` state), the generator function is set up
    /// and executed until the first `yield` or `return`.
    ///
    /// On subsequent calls (`Suspended` state), the VM state is restored and
    /// the value passed to `.next()` is pushed onto the stack (as the result
    /// of the `yield` expression), then execution continues.
    ///
    /// Returns a `{value, done}` iterator result.
    pub(crate) fn execute_generator_next(
        &mut self,
        gen_rc: &Rc<crate::object::VmCell<crate::object::GeneratorObject>>,
        next_arg: Value,
    ) -> Result<Value, VMError> {
        use crate::object::GeneratorState;

        let state = gen_rc.borrow().state.clone();
        match state {
            GeneratorState::Completed => Ok(self.make_iterator_result(Value::UNDEFINED, true)),
            GeneratorState::Created => {
                // First call: set up the function and run until yield/return.
                let func = gen_rc.borrow().function.clone();
                let args = gen_rc.borrow().args.clone();
                let receiver = gen_rc.borrow().receiver;

                gen_rc.borrow_mut().state = GeneratorState::Suspended;

                // Run the function body.  If it yields, we get Err(Yield(v)).
                let result = self.execute_generator_body(
                    gen_rc, func, &args, receiver, None, // no saved_ip — start from 0
                    None, // no resume value on first call
                );
                self.finalize_generator_result(gen_rc, result)
            }
            GeneratorState::Suspended => {
                // Resume: restore state and push the .next() argument.
                let func = gen_rc.borrow().function.clone();
                let saved_ip = gen_rc.borrow().saved_ip;
                let receiver = gen_rc.borrow().receiver;

                let result = self.execute_generator_body(
                    gen_rc,
                    func,
                    &[],
                    receiver,
                    Some(saved_ip),
                    Some(next_arg),
                );
                self.finalize_generator_result(gen_rc, result)
            }
        }
    }

    /// Execute a generator `.return(value)` call.
    /// Forces the generator to complete with the given value.
    fn execute_generator_return(
        &mut self,
        gen_rc: &Rc<crate::object::VmCell<crate::object::GeneratorObject>>,
        return_value: Value,
    ) -> Result<Value, VMError> {
        use crate::object::GeneratorState;

        let state = gen_rc.borrow().state.clone();
        match state {
            GeneratorState::Completed => Ok(self.make_iterator_result(return_value, true)),
            _ => {
                gen_rc.borrow_mut().state = GeneratorState::Completed;
                Ok(self.make_iterator_result(return_value, true))
            }
        }
    }

    /// Execute the generator function body (either from the beginning or from
    /// a saved instruction pointer).
    ///
    /// This uses `execute_compiled_function_slice` with the full save/restore
    /// machinery. On `Yield`, the VM state (ip, locals) is saved into the
    /// GeneratorObject so it can be resumed later.
    fn execute_generator_body(
        &mut self,
        gen_rc: &Rc<crate::object::VmCell<crate::object::GeneratorObject>>,
        func: CompiledFunctionObject,
        args: &[Value],
        receiver: Option<Value>,
        resume_ip: Option<usize>,
        resume_value: Option<Value>,
    ) -> Result<Value, VMError> {
        // Destructure the function
        let CompiledFunctionObject {
            instructions,
            constants,
            num_locals,
            num_parameters,
            rest_parameter_index,
            takes_this,
            is_async: _,
            is_generator: _,
            num_cache_slots,
            max_stack_depth,
            register_count,
            inline_cache: func_cache,
            closure_captures: _,
            captured_values: _,
        } = func;

        let is_register = register_count > 0;

        // Save current VM state
        let saved_ip = self.ip;
        let saved_inst_ptr = self.inst_ptr;
        let saved_inst_len = self.inst_len;
        let saved_instructions = std::mem::replace(&mut self.instructions, instructions);
        self.inst_ptr = self.instructions.as_ptr();
        self.inst_len = self.instructions.len();
        let saved_constants = std::mem::replace(&mut self.constants, constants);
        let saved_constants_raw = self.constants_raw;
        let saved_cv_ptr = self.constants_values_ptr;
        let saved_cs_ptr = self.constants_syms_ptr;
        if is_register {
            self.constants_raw = &*self.constants as *const Vec<Object>;
            self.preconvert_constants();
        }
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_sp = self.sp;
        let saved_last_popped = self.last_popped.take();
        let saved_max_stack_depth = self.max_stack_depth;
        let saved_inline_cache = if num_cache_slots > 0 {
            let mut taken = std::mem::take(func_cache.borrow_mut());
            // If the cache was already taken (recursive call), allocate fresh
            if taken.is_empty() {
                taken = vec![(0, 0); num_cache_slots as usize];
            }
            std::mem::replace(&mut self.inline_cache, taken)
        } else {
            Vec::new()
        };

        self.max_stack_depth = max_stack_depth as usize;
        let arg_offset = if takes_this { 1 } else { 0 };

        let (run_result, return_value) = if is_register {
            // ── Register-based generator ──────────────────────────────
            let reg_base = self.sp;
            let reg_window = (register_count as usize).max(1);

            if reg_base + reg_window > STACK_SIZE {
                // Restore on error
                self.ip = saved_ip;
                self.instructions = saved_instructions;
                self.inst_ptr = saved_inst_ptr;
                self.inst_len = saved_inst_len;
                self.constants = saved_constants;
                self.constants_raw = saved_constants_raw;
                self.constants_values_ptr = saved_cv_ptr;
                self.constants_syms_ptr = saved_cs_ptr;
                self.max_stack_depth = saved_max_stack_depth;
                if num_cache_slots > 0 {
                    *func_cache.borrow_mut() =
                        std::mem::replace(&mut self.inline_cache, saved_inline_cache);
                }
                self.locals = saved_locals;
                self.last_popped = saved_last_popped;
                return Err(VMError::StackOverflow);
            }

            while self.stack.len() < reg_base + reg_window {
                self.stack.push(Value::UNDEFINED);
            }

            if let Some(ip) = resume_ip {
                // Resuming: restore saved registers from GeneratorObject.locals
                self.ip = ip;
                {
                    let gen = gen_rc.borrow();
                    let saved_regs = &gen.locals;
                    for (i, obj) in saved_regs.iter().enumerate() {
                        if i < reg_window {
                            self.stack[reg_base + i] = obj_into_val(obj.clone(), &mut self.heap);
                        }
                    }
                }

                // Push the resume value (the arg to .next()) into the register
                // that was the dst of the ROp::Yield instruction.
                // saved_ip points past the 5-byte Yield instruction [opcode, dst_hi, dst_lo, src_hi, src_lo],
                // so the dst register is the big-endian u16 at instructions[ip-4..ip-2].
                if let Some(rv) = resume_value {
                    let dst_reg = ((self.instructions[ip - 4] as usize) << 8)
                        | (self.instructions[ip - 3] as usize);
                    self.stack[reg_base + dst_reg] = rv;
                }
            } else {
                // First call: initialize registers
                self.ip = 0;
                for i in 0..reg_window {
                    self.stack[reg_base + i] = Value::UNDEFINED;
                }
                if takes_this {
                    self.stack[reg_base] = receiver.unwrap_or(Value::UNDEFINED);
                }
                let rest_index = rest_parameter_index;
                let positional_count = rest_index.map_or(args.len(), |ri| args.len().min(ri));
                for (i, arg) in args.iter().take(positional_count).enumerate() {
                    self.stack[reg_base + i + arg_offset] = *arg;
                }
                if let Some(rest_i) = rest_index {
                    let rest_reg = rest_i + arg_offset;
                    let rest_values: Vec<Value> = args.iter().skip(rest_i).copied().collect();
                    self.stack[reg_base + rest_reg] =
                        obj_into_val(make_array(rest_values), &mut self.heap);
                }
            }

            self.locals = Vec::new();
            self.sp = reg_base + reg_window;

            let entry_depth = self.frames.len();
            let rr = self.rdispatch_loop(entry_depth, reg_base);

            // Save register state if yielding
            if let Err(VMError::Yield(_)) = &rr {
                let mut regs = Vec::with_capacity(reg_window);
                for i in 0..reg_window {
                    regs.push(val_to_obj(self.stack[reg_base + i], &self.heap));
                }
                let gen = gen_rc.borrow_mut();
                gen.locals = regs;
                gen.saved_ip = self.ip;
            }

            let rv = self.last_popped.take().unwrap_or(Value::UNDEFINED);
            (rr, rv)
        } else {
            // ── Stack-based generator ─────────────────────────────────
            let init_local_count = num_parameters + arg_offset;
            let needed = num_locals.max(init_local_count);

            if let Some(ip) = resume_ip {
                // Resuming: restore locals from GeneratorObject
                self.ip = ip;
                {
                    let gen = gen_rc.borrow();
                    self.locals = gen.locals.clone();
                }

                // Push resume value onto the stack for OpYield to "receive"
                if let Some(rv) = resume_value {
                    unsafe { self.push_unchecked(rv) };
                }
            } else {
                // First call: set up locals
                self.ip = 0;
                let mut new_locals = self
                    .locals_pool
                    .pop()
                    .unwrap_or_else(|| Vec::with_capacity(needed));
                new_locals.resize(needed, Object::Undefined);

                if let Some(rest_i) = rest_parameter_index {
                    let need = rest_i + arg_offset + 1;
                    if new_locals.len() < need {
                        new_locals.resize(need, Object::Undefined);
                    }
                }

                if takes_this {
                    new_locals[0] = val_to_obj(receiver.unwrap_or(Value::UNDEFINED), &self.heap);
                }

                let rest_index = rest_parameter_index;
                if let Some(rest_i) = rest_index {
                    let rest_local = rest_i + arg_offset;
                    let rest_values: Vec<Value> = args.iter().skip(rest_i).copied().collect();
                    new_locals[rest_local] = make_array(rest_values);
                }

                let positional_count =
                    rest_parameter_index.map_or(args.len(), |rest_i| args.len().min(rest_i));
                let positional_count =
                    positional_count.min(new_locals.len().saturating_sub(arg_offset));
                for (i, arg) in args.iter().take(positional_count).enumerate() {
                    let target = i + arg_offset;
                    unsafe { *new_locals.get_unchecked_mut(target) = val_to_obj(*arg, &self.heap) };
                }

                self.locals = new_locals;
            }

            if max_stack_depth > 0 && self.sp + max_stack_depth as usize > STACK_SIZE {
                self.ip = saved_ip;
                self.instructions = saved_instructions;
                self.inst_ptr = saved_inst_ptr;
                self.inst_len = saved_inst_len;
                self.constants = saved_constants;
                self.constants_raw = saved_constants_raw;
                self.max_stack_depth = saved_max_stack_depth;
                if num_cache_slots > 0 {
                    *func_cache.borrow_mut() =
                        std::mem::replace(&mut self.inline_cache, saved_inline_cache);
                }
                let mut used_locals = std::mem::replace(&mut self.locals, saved_locals);
                used_locals.clear();
                self.locals_pool.push(used_locals);
                self.stack.truncate(saved_sp);
                self.sp = saved_sp;
                self.last_popped = saved_last_popped;
                return Err(VMError::StackOverflow);
            }

            let rr = self.run();

            // Save locals if yielding
            if let Err(VMError::Yield(_)) = &rr {
                let gen = gen_rc.borrow_mut();
                gen.locals = self.locals.clone();
                gen.saved_ip = self.ip;
            }

            let rv = self.last_popped.take().unwrap_or(Value::UNDEFINED);
            (rr, rv)
        };

        // Restore parent VM state
        self.ip = saved_ip;
        self.instructions = saved_instructions;
        // Restore inst_ptr/inst_len from saved values (see execute_compiled_function_slice).
        self.inst_ptr = saved_inst_ptr;
        self.inst_len = saved_inst_len;
        self.constants = saved_constants;
        self.constants_raw = saved_constants_raw;
        self.constants_values_ptr = saved_cv_ptr;
        self.constants_syms_ptr = saved_cs_ptr;
        self.max_stack_depth = saved_max_stack_depth;
        if num_cache_slots > 0 {
            *func_cache.borrow_mut() =
                std::mem::replace(&mut self.inline_cache, saved_inline_cache);
        }
        let mut used_locals = std::mem::replace(&mut self.locals, saved_locals);
        used_locals.clear();
        self.locals_pool.push(used_locals);
        self.stack.truncate(saved_sp);
        self.sp = saved_sp;
        self.last_popped = saved_last_popped;

        match run_result {
            Err(VMError::Yield(yielded)) => Err(VMError::Yield(yielded)),
            Err(e) => Err(e),
            Ok(()) => Ok(return_value),
        }
    }

    /// Convert the result of `execute_generator_body` into an iterator result.
    /// On yield: `{value: yielded, done: false}`.
    /// On return/completion: `{value: returned, done: true}` + mark completed.
    fn finalize_generator_result(
        &mut self,
        gen_rc: &Rc<crate::object::VmCell<crate::object::GeneratorObject>>,
        result: Result<Value, VMError>,
    ) -> Result<Value, VMError> {
        use crate::object::GeneratorState;
        match result {
            Ok(return_value) => {
                // Normal completion (return or end of function)
                gen_rc.borrow_mut().state = GeneratorState::Completed;
                Ok(self.make_iterator_result(return_value, true))
            }
            Err(VMError::Yield(yielded_val)) => {
                // Suspension: state already set to Suspended, ip/locals saved
                // by execute_generator_body
                Ok(self.make_iterator_result(yielded_val, false))
            }
            Err(e) => {
                gen_rc.borrow_mut().state = GeneratorState::Completed;
                Err(e)
            }
        }
    }
}

// ── URI encoding helpers ──────────────────────────────────────────

/// Characters that `encodeURIComponent` does NOT encode.
const URI_COMPONENT_UNESCAPED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";

/// Extra characters that `encodeURI` (but not `encodeURIComponent`) preserves.
const URI_EXTRA_UNESCAPED: &[u8] = b";,/?:@&=+$#";

fn uri_encode(input: &str, is_full_uri: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        if URI_COMPONENT_UNESCAPED.contains(byte)
            || (is_full_uri && URI_EXTRA_UNESCAPED.contains(byte))
        {
            out.push(*byte as char);
        } else {
            // Percent-encode each byte of multi-byte UTF-8 chars too
            out.push('%');
            out.push(HEX_UPPER[(*byte >> 4) as usize] as char);
            out.push(HEX_UPPER[(*byte & 0x0F) as usize] as char);
        }
    }
    out
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

fn uri_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::bytecode::Bytecode;
    use crate::code::{make, Opcode};
    use crate::config::FormLogicConfig;
    use crate::object::Object;
    use crate::vm::VM;

    #[test]
    fn executes_constant_and_add() {
        let mut ins = vec![];
        ins.extend(make(Opcode::OpConstant, &[0]));
        ins.extend(make(Opcode::OpConstant, &[1]));
        ins.extend(make(Opcode::OpAdd, &[]));

        let bytecode = Bytecode::new(ins, vec![Object::Integer(2), Object::Integer(3)], vec![]);
        let mut vm = VM::new(bytecode, FormLogicConfig::default());
        vm.run().expect("vm run should pass");
        assert!(matches!(vm.pop().expect("pop"), Object::Integer(5)));
    }
}
