use std::{cell::RefCell, rc::Rc};

use indexmap::IndexMap;
use rustc_hash::FxHashMap;

use crate::config::FormLogicConfig;
use crate::object::{CompiledFunctionObject, Object};
use crate::parser::parse_program_from_source;
use crate::rcompiler::RCompiler;
use crate::value::{obj_into_val, val_to_obj, Heap, Value};
use crate::vm::{ExecutionQuota, VM};

const BYTECODE_CACHE_CAPACITY: usize = 256;

#[derive(Clone)]
struct CachedBytecode {
    instructions: Rc<Vec<u8>>,
    constants: Rc<Vec<Object>>,
    num_cache_slots: u16,
    max_stack_depth: u16,
    register_count: u16,
}

pub struct FormLogicEngine {
    pub config: FormLogicConfig,
    bytecode_cache: RefCell<IndexMap<String, CachedBytecode>>,
    vm_pool: RefCell<Option<VM>>,
}

impl Default for FormLogicEngine {
    fn default() -> Self {
        Self {
            config: FormLogicConfig::default(),
            bytecode_cache: RefCell::new(IndexMap::new()),
            vm_pool: RefCell::new(None),
        }
    }
}

impl FormLogicEngine {
    pub fn with_config(config: FormLogicConfig) -> Self {
        Self {
            config,
            bytecode_cache: RefCell::new(IndexMap::new()),
            vm_pool: RefCell::new(None),
        }
    }

    pub fn eval(&self, source: &str) -> Result<Object, String> {
        let cached = { self.bytecode_cache.borrow().get(source).cloned() };
        let cached = if let Some(cached) = cached {
            cached
        } else {
            let (program, errors) = parse_program_from_source(source);
            if !errors.is_empty() {
                return Err(format!("Parser errors: {}", errors.join(", ")));
            }

            let compiled = RCompiler::new().compile_program(&program)?;
            let compiled = CachedBytecode {
                instructions: Rc::new(compiled.instructions),
                constants: Rc::new(compiled.constants),
                num_cache_slots: compiled.num_cache_slots,
                max_stack_depth: compiled.max_stack_depth,
                register_count: compiled.register_count,
            };
            {
                let mut cache = self.bytecode_cache.borrow_mut();
                if cache.len() >= BYTECODE_CACHE_CAPACITY {
                    cache.swap_remove_index(0);
                }
                cache.insert(source.to_string(), compiled.clone());
            }
            compiled
        };

        // Take VM from pool, or create fresh if pool is empty
        let mut vm = self.vm_pool.borrow_mut().take().unwrap_or_else(|| {
            VM::new_from_rc(
                Rc::clone(&cached.instructions),
                Rc::clone(&cached.constants),
                self.config.clone(),
                crate::vm::STACK_SIZE,
                cached.num_cache_slots,
                cached.max_stack_depth,
            )
        });

        // Reset VM for the new bytecode (no-op on fresh VM, resets recycled VM)
        vm.reset_for_run(
            Rc::clone(&cached.instructions),
            Rc::clone(&cached.constants),
            cached.num_cache_slots,
            cached.max_stack_depth,
            cached.register_count,
        );

        let run_result = vm.run_register();
        let last = vm.last_popped.take().unwrap_or(Value::UNDEFINED);
        let result = val_to_obj(last, &vm.heap);

        // Return VM to pool for reuse (even on error — it gets reset next call)
        *self.vm_pool.borrow_mut() = Some(vm);

        run_result.map_err(|e| format!("VM error: {:?}", e))?;
        Ok(result)
    }

    /// Evaluate source code and return the result as a JSON string.
    /// Unlike `eval()` which returns an Object (with potential `[ref]` for nested heap values),
    /// this method serializes the result while the heap is still accessible, producing
    /// a complete JSON representation with all nested values fully resolved.
    pub fn eval_to_json(&self, source: &str) -> Result<String, String> {
        let cached = { self.bytecode_cache.borrow().get(source).cloned() };
        let cached = if let Some(cached) = cached {
            cached
        } else {
            let (program, errors) = parse_program_from_source(source);
            if !errors.is_empty() {
                return Err(format!("Parser errors: {}", errors.join(", ")));
            }

            let compiled = RCompiler::new().compile_program(&program)?;
            let compiled = CachedBytecode {
                instructions: Rc::new(compiled.instructions),
                constants: Rc::new(compiled.constants),
                num_cache_slots: compiled.num_cache_slots,
                max_stack_depth: compiled.max_stack_depth,
                register_count: compiled.register_count,
            };
            {
                let mut cache = self.bytecode_cache.borrow_mut();
                if cache.len() >= BYTECODE_CACHE_CAPACITY {
                    cache.swap_remove_index(0);
                }
                cache.insert(source.to_string(), compiled.clone());
            }
            compiled
        };

        let mut vm = self.vm_pool.borrow_mut().take().unwrap_or_else(|| {
            VM::new_from_rc(
                Rc::clone(&cached.instructions),
                Rc::clone(&cached.constants),
                self.config.clone(),
                crate::vm::STACK_SIZE,
                cached.num_cache_slots,
                cached.max_stack_depth,
            )
        });

        vm.reset_for_run(
            Rc::clone(&cached.instructions),
            Rc::clone(&cached.constants),
            cached.num_cache_slots,
            cached.max_stack_depth,
            cached.register_count,
        );

        let run_result = vm.run_register();
        let last = vm.last_popped.take().unwrap_or(Value::UNDEFINED);

        // Serialize to JSON while heap is still available
        let json = Self::value_to_json(last, &vm.heap);

        *self.vm_pool.borrow_mut() = Some(vm);
        run_result.map_err(|e| format!("VM error: {:?}", e))?;
        Ok(json)
    }

    /// Evaluate source code, return JSON result + execution trace for ZK proving.
    /// The trace captures (clk, pc, opcode, val_a, val_b, val_dst, const_val, aux) per step.
    pub fn eval_with_trace(&self, source: &str) -> Result<(String, Vec<(u64, u64, u8, u64, u64, u64, u64, u64)>), String> {
        let cached = { self.bytecode_cache.borrow().get(source).cloned() };
        let cached = if let Some(cached) = cached {
            cached
        } else {
            let (program, errors) = parse_program_from_source(source);
            if !errors.is_empty() {
                return Err(format!("Parser errors: {}", errors.join(", ")));
            }
            let compiled = RCompiler::new().compile_program(&program)?;
            let compiled = CachedBytecode {
                instructions: Rc::new(compiled.instructions),
                constants: Rc::new(compiled.constants),
                num_cache_slots: compiled.num_cache_slots,
                max_stack_depth: compiled.max_stack_depth,
                register_count: compiled.register_count,
            };
            {
                let mut cache = self.bytecode_cache.borrow_mut();
                if cache.len() >= BYTECODE_CACHE_CAPACITY {
                    cache.swap_remove_index(0);
                }
                cache.insert(source.to_string(), compiled.clone());
            }
            compiled
        };

        let mut vm = self.vm_pool.borrow_mut().take().unwrap_or_else(|| {
            VM::new_from_rc(
                Rc::clone(&cached.instructions),
                Rc::clone(&cached.constants),
                self.config.clone(),
                crate::vm::STACK_SIZE,
                cached.num_cache_slots,
                cached.max_stack_depth,
            )
        });

        vm.reset_for_run(
            Rc::clone(&cached.instructions),
            Rc::clone(&cached.constants),
            cached.num_cache_slots,
            cached.max_stack_depth,
            cached.register_count,
        );

        // Enable trace capture
        vm.trace_enabled = true;
        vm.trace_steps.clear();
        vm.trace_clk = 0;

        let run_result = vm.run_register();
        let last = vm.last_popped.take().unwrap_or(Value::UNDEFINED);
        let json = Self::value_to_json(last, &vm.heap);

        // Extract trace before returning VM to pool
        let trace = std::mem::take(&mut vm.trace_steps);
        vm.trace_enabled = false;

        *self.vm_pool.borrow_mut() = Some(vm);
        run_result.map_err(|e| format!("VM error: {:?}", e))?;
        Ok((json, trace))
    }

    /// Produce a correctly escaped JSON string literal using serde_json.
    /// Handles all control characters, unicode, backslashes, and quotes.
    fn json_string(s: &str) -> String {
        serde_json::to_string(s).unwrap_or_else(|_| "null".to_string())
    }

    /// Convert a NaN-boxed Value to a JSON string, resolving all heap references.
    fn value_to_json(val: Value, heap: &Heap) -> String {
        if val.is_i32() {
            return format!("{}", unsafe { val.as_i32_unchecked() });
        }
        if val.is_f64() {
            let f = val.as_f64();
            if f.is_nan() { return "null".to_string(); }
            if f.is_infinite() { return "null".to_string(); }
            if f.fract() == 0.0 && f.abs() < i64::MAX as f64 {
                return format!("{}", f as i64);
            }
            return format!("{}", f);
        }
        if val.is_bool() {
            return if unsafe { val.as_bool_unchecked() } { "true" } else { "false" }.to_string();
        }
        if val.is_null() || val.is_undefined() {
            return "null".to_string();
        }
        if val.is_inline_str() {
            let (buf, len) = val.inline_str_buf();
            let s = std::str::from_utf8(&buf[..len]).unwrap_or("");
            return Self::json_string(s);
        }
        if val.is_heap() {
            return Self::object_to_json(heap.get(val.heap_index()), heap);
        }
        "null".to_string()
    }

    /// Convert an Object to a JSON string, resolving all nested heap references.
    fn object_to_json(obj: &Object, heap: &Heap) -> String {
        match obj {
            Object::Integer(v) => format!("{}", v),
            Object::Float(v) => {
                if v.is_nan() || v.is_infinite() { "null".to_string() }
                else if v.fract() == 0.0 && v.abs() < i64::MAX as f64 { format!("{}", *v as i64) }
                else { format!("{}", v) }
            }
            Object::Boolean(v) => format!("{}", v),
            Object::Null | Object::Undefined => "null".to_string(),
            Object::String(v) => Self::json_string(v),
            Object::Array(items) => {
                let borrowed = items.borrow();
                let elements: Vec<String> = borrowed.iter()
                    .map(|v| Self::value_to_json(*v, heap))
                    .collect();
                format!("[{}]", elements.join(", "))
            }
            Object::Hash(h) => {
                let h = h.borrow();
                let entries: Vec<String> = h.pairs.keys().enumerate()
                    .map(|(i, k)| {
                        let v = h.values.get(i)
                            .map(|v| Self::value_to_json(*v, heap))
                            .unwrap_or_else(|| "null".to_string());
                        format!("{}: {}", Self::json_string(&k.to_string()), v)
                    })
                    .collect();
                format!("{{{}}}", entries.join(", "))
            }
            Object::Instance(inst) => {
                let entries: Vec<String> = inst.fields.iter()
                    .map(|(k, v)| format!("{}: {}", Self::json_string(k), Self::object_to_json(v, heap)))
                    .collect();
                format!("{{{}}}", entries.join(", "))
            }
            Object::Error(err) => Self::json_string(&format!("Error: {}", err.message)),
            Object::ReturnValue(v) => Self::object_to_json(v, heap),
            _ => Self::json_string(&obj.inspect()),
        }
    }

    /// Parse, compile, and execute top-level code, keeping the VM alive for
    /// subsequent `call_function` / `get_global` / `set_global` calls.
    pub fn init_script(&self, source: &str) -> Result<ScriptState, String> {
        let mut state = self.compile_script(source)?;
        state.run_init()?;
        Ok(state)
    }

    /// Parse and compile top-level code WITHOUT executing it.
    /// Returns a `ScriptState` with the VM ready to run. Call `run_init()` on the
    /// returned state after setting up bridges (db, localStorage, etc.).
    pub fn compile_script(&self, source: &str) -> Result<ScriptState, String> {
        let (program, errors) = parse_program_from_source(source);
        if !errors.is_empty() {
            return Err(format!("Parser errors: {}", errors.join(", ")));
        }

        let compiled = RCompiler::new().compile_program_persistent(&program)?;
        let globals_table = compiled.globals_table.clone();
        let register_count = compiled.register_count;

        let instructions = Rc::new(compiled.instructions);
        let constants = Rc::new(compiled.constants);

        let mut vm = VM::new_from_rc(
            Rc::clone(&instructions),
            Rc::clone(&constants),
            self.config.clone(),
            crate::vm::STACK_SIZE,
            compiled.num_cache_slots,
            compiled.max_stack_depth,
        );
        vm.reset_for_run(
            Rc::clone(&instructions),
            Rc::clone(&constants),
            compiled.num_cache_slots,
            compiled.max_stack_depth,
            register_count,
        );

        Ok(ScriptState { vm, globals_table, gc_threshold: ScriptState::GC_INITIAL_THRESHOLD })
    }
}

/// Persistent script state: a VM with its globals still alive, plus the
/// name→slot mapping so callers can look up variables and functions by name.
pub struct ScriptState {
    pub(crate) vm: VM,
    pub(crate) globals_table: FxHashMap<String, u16>,
    /// Dynamic GC threshold: scales to 2x live objects after each collection.
    /// Prevents the O(N²) GC storm where a static threshold triggers collection
    /// on every single function call once the heap has grown past it.
    gc_threshold: usize,
}

impl ScriptState {
    const GC_INITIAL_THRESHOLD: usize = 4096;

    /// Execute the compiled top-level code (the "init" phase).
    /// Call this after setting up bridges (db, localStorage, etc.) on the ScriptState.
    pub fn run_init(&mut self) -> Result<(), String> {
        self.vm.run_register().map_err(|e| format!("VM error: {:?}", e))
    }

    /// Snapshot global slot values (up to high-water mark).
    /// Used to restore globals to post-init state between calls, preventing
    /// state bleed across requests on the same worker thread.
    pub fn snapshot_globals(&self) -> Vec<Value> {
        let hwm = self.vm.globals.high_water_mark();
        let mut snapshot = Vec::with_capacity(hwm);
        for i in 0..hwm {
            snapshot.push(unsafe { self.vm.globals.get_unchecked(i) });
        }
        snapshot
    }

    /// Restore global slot values from a snapshot taken after init.
    /// Any globals written since the snapshot are reverted, ensuring each
    /// handler call starts from a clean state.
    pub fn restore_globals(&mut self, snapshot: &[Value]) {
        for (i, &val) in snapshot.iter().enumerate() {
            unsafe { self.vm.globals.set_unchecked(i, val) };
        }
    }

    /// Return a reference to the globals table (name → slot index).
    pub fn globals_table(&self) -> &FxHashMap<String, u16> {
        &self.globals_table
    }

    /// Check if a global slot has been written since the last `clear_dirty()`.
    #[inline]
    pub fn is_global_dirty(&self, index: u16) -> bool {
        self.vm.globals.is_dirty(index as usize)
    }

    /// Clear all dirty bits (call after syncing state to React).
    #[inline]
    pub fn clear_dirty(&self) {
        self.vm.globals.clear_dirty();
    }

    /// Read a global variable by slot index.
    pub fn get_global_by_index(&self, index: u16) -> Object {
        let val = unsafe { self.vm.globals.get_unchecked(index as usize) };
        val_to_obj(val, &self.vm.heap)
    }

    /// Write a global variable by slot index.
    pub fn set_global_by_index(&mut self, index: u16, value: Object) {
        let val = obj_into_val(value, &mut self.vm.heap);
        unsafe { self.vm.globals.set_unchecked(index as usize, val) };
    }

    /// Call a named function defined in the script.
    /// Resets the execution quota so each call gets a fresh instruction/time budget.
    pub fn call_function(&mut self, name: &str, args: &[Object]) -> Result<Object, String> {
        self.vm.quota = ExecutionQuota::default();

        let &slot = self
            .globals_table
            .get(name)
            .ok_or_else(|| format!("undefined function: {}", name))?;

        // Read the function object from the global slot
        let val = unsafe { self.vm.globals.get_unchecked(slot as usize) };
        if !val.is_heap() {
            return Err(format!("{} is not a function", name));
        }
        let func = match self.vm.heap.get(val.heap_index()) {
            Object::CompiledFunction(f) => f.clone(),
            _ => return Err(format!("{} is not a function", name)),
        };

        // Convert args to Values and place them on the stack
        let arg_start = self.vm.sp;
        // Reserve a dummy register for callee (Call opcode layout: base = callee, base+1.. = args)
        // call_register_direct expects args starting at arg_stack_start
        for arg in args {
            let v = obj_into_val(arg.clone(), &mut self.vm.heap);
            if self.vm.sp >= self.vm.stack.len() {
                self.vm.stack.push(v);
            } else {
                self.vm.stack[self.vm.sp] = v;
            }
            self.vm.sp += 1;
        }
        let nargs = args.len();

        // SAFETY: func pointers are derived from Rc-backed CompiledFunctionObject
        // fields that remain valid for the duration of the call.
        let result_val = unsafe {
            self.vm
                .call_register_direct(
                    func.instructions.as_ptr(),
                    func.instructions.len(),
                    &*func.constants as *const std::vec::Vec<Object>,
                    func.rest_parameter_index,
                    func.takes_this,
                    func.is_async,
                    func.num_cache_slots,
                    func.max_stack_depth,
                    func.register_count,
                    Rc::as_ptr(&func.inline_cache),
                    arg_start,
                    nargs,
                    None,
                )
        }
            .map_err(|e| format!("VM error: {:?}", e))?;

        // Restore sp
        self.vm.sp = arg_start;

        let result = val_to_obj(result_val, &self.vm.heap);

        // Trigger GC when live heap objects exceed dynamic threshold
        if self.vm.heap.allocated_count() > self.gc_threshold {
            self.gc_collect();
            // Scale threshold to 2x live objects, never below initial
            self.gc_threshold = std::cmp::max(
                Self::GC_INITIAL_THRESHOLD,
                self.vm.heap.allocated_count() * 2,
            );
        }

        Ok(result)
    }

    /// Call a Value (function closure / compiled function) with Object arguments.
    /// Used for dispatching event handlers stored as Values in event_listeners.
    pub fn call_value(&mut self, callee: Value, args: &[Object]) -> Result<Object, String> {
        let arg_start = self.vm.sp;
        for arg in args {
            let v = obj_into_val(arg.clone(), &mut self.vm.heap);
            if self.vm.sp >= self.vm.stack.len() {
                self.vm.stack.push(v);
            } else {
                self.vm.stack[self.vm.sp] = v;
            }
            self.vm.sp += 1;
        }

        let arg_vals: Vec<Value> = (arg_start..self.vm.sp)
            .map(|i| self.vm.stack[i])
            .collect();

        let result_val = self.vm
            .call_value_slice(callee, &arg_vals)
            .map_err(|e| format!("VM error: {:?}", e))?;

        self.vm.sp = arg_start;
        let result = val_to_obj(result_val, &self.vm.heap);

        if self.vm.heap.allocated_count() > self.gc_threshold {
            self.gc_collect();
            self.gc_threshold = std::cmp::max(
                Self::GC_INITIAL_THRESHOLD,
                self.vm.heap.allocated_count() * 2,
            );
        }

        Ok(result)
    }

    /// Call a CompiledFunctionObject directly (for functions stored in hash objects,
    /// e.g. component renderers registered via `registerComponent`).
    pub fn call_compiled_function(
        &mut self,
        func: &CompiledFunctionObject,
        args: &[Object],
    ) -> Result<Object, String> {
        let arg_start = self.vm.sp;
        for arg in args {
            let v = obj_into_val(arg.clone(), &mut self.vm.heap);
            if self.vm.sp >= self.vm.stack.len() {
                self.vm.stack.push(v);
            } else {
                self.vm.stack[self.vm.sp] = v;
            }
            self.vm.sp += 1;
        }
        let nargs = args.len();

        // SAFETY: func pointers are derived from Rc-backed CompiledFunctionObject
        // fields that remain valid for the duration of the call.
        let result_val = unsafe {
            self.vm
                .call_register_direct(
                    func.instructions.as_ptr(),
                    func.instructions.len(),
                    &*func.constants as *const std::vec::Vec<Object>,
                    func.rest_parameter_index,
                    func.takes_this,
                    func.is_async,
                    func.num_cache_slots,
                    func.max_stack_depth,
                    func.register_count,
                    Rc::as_ptr(&func.inline_cache),
                    arg_start,
                    nargs,
                    None,
                )
        }
            .map_err(|e| format!("VM error: {:?}", e))?;

        self.vm.sp = arg_start;
        let result = val_to_obj(result_val, &self.vm.heap);

        if self.vm.heap.allocated_count() > self.gc_threshold {
            self.gc_collect();
            self.gc_threshold = std::cmp::max(
                Self::GC_INITIAL_THRESHOLD,
                self.vm.heap.allocated_count() * 2,
            );
        }

        Ok(result)
    }

    /// Evaluate an expression in the script's global context.
    /// The expression has access to all script variables and functions via
    /// the existing globals table. Uses the script's own VM (heap, globals, etc.)
    /// with a temporary bytecode swap.
    pub fn eval_in_context(&mut self, source: &str) -> Result<Object, String> {
        let (program, errors) = parse_program_from_source(source);
        if !errors.is_empty() {
            return Err(format!("Parser errors: {}", errors.join(", ")));
        }

        // Compile with the script's globals table so GetGlobal uses correct indices
        let compiled = RCompiler::with_globals(&self.globals_table)
            .compile_program(&program)?;

        let expr_instructions = Rc::new(compiled.instructions);
        let expr_constants = Rc::new(compiled.constants);

        // Save VM state
        let saved_instructions =
            std::mem::replace(&mut self.vm.instructions, Rc::clone(&expr_instructions));
        let saved_constants =
            std::mem::replace(&mut self.vm.constants, Rc::clone(&expr_constants));
        let saved_ip = self.vm.ip;
        let saved_sp = self.vm.sp;
        let saved_register_count = self.vm.register_count;
        let saved_max_stack_depth = self.vm.max_stack_depth;
        let saved_inline_cache = std::mem::replace(
            &mut self.vm.inline_cache,
            vec![(0, 0); compiled.num_cache_slots as usize],
        );

        // Reset for expression evaluation
        self.vm.ip = 0;
        self.vm.inst_ptr = self.vm.instructions.as_ptr();
        self.vm.inst_len = self.vm.instructions.len();
        self.vm.register_count = compiled.register_count;
        self.vm.max_stack_depth = compiled.max_stack_depth as usize;

        // Run the expression
        let run_result = self.vm.run_register();

        // Get result
        let last = self.vm.last_popped.take().unwrap_or(Value::UNDEFINED);
        let result = val_to_obj(last, &self.vm.heap);

        // Restore VM state
        self.vm.instructions = saved_instructions;
        self.vm.constants = saved_constants;
        self.vm.ip = saved_ip;
        self.vm.sp = saved_sp;
        self.vm.register_count = saved_register_count;
        self.vm.max_stack_depth = saved_max_stack_depth;
        self.vm.inline_cache = saved_inline_cache;
        self.vm.inst_ptr = self.vm.instructions.as_ptr();
        self.vm.inst_len = self.vm.instructions.len();
        // Invalidate constants pointers — they'll be re-set on next run_register
        self.vm.constants_values_ptr = std::ptr::null();
        self.vm.constants_raw = &*self.vm.constants as *const Vec<Object>;

        run_result.map_err(|e| format!("VM error: {:?}", e))?;
        Ok(result)
    }

    /// Read a global variable by name.
    pub fn get_global(&self, name: &str) -> Result<Object, String> {
        let &slot = self
            .globals_table
            .get(name)
            .ok_or_else(|| format!("undefined variable: {}", name))?;
        let val = unsafe { self.vm.globals.get_unchecked(slot as usize) };
        Ok(val_to_obj(val, &self.vm.heap))
    }

    /// Get a reference to the VM heap (for converting Values to Objects).
    pub fn heap(&self) -> &crate::value::Heap {
        &self.vm.heap
    }

    /// Get a mutable reference to the VM heap (for allocating Objects as Values).
    pub fn heap_mut(&mut self) -> &mut crate::value::Heap {
        &mut self.vm.heap
    }

    /// Write a global variable by name.
    /// If the variable does not exist, it is created as a new runtime global.
    pub fn set_global(&mut self, name: &str, value: Object) -> Result<(), String> {
        let slot = if let Some(&s) = self.globals_table.get(name) {
            s
        } else {
            // Define a new runtime global — slot indices are contiguous starting from 0,
            // so the next available slot equals the current table size.
            let next = self.globals_table.len() as u16;
            if (next as usize) >= crate::vm::GLOBALS_SIZE {
                return Err("too many global variables".to_string());
            }
            self.globals_table.insert(name.to_string(), next);
            next
        };
        let val = obj_into_val(value, &mut self.vm.heap);
        unsafe { self.vm.globals.set_unchecked(slot as usize, val) };
        Ok(())
    }

    /// Attach a localStorage backend to the VM.
    pub fn set_local_storage(
        &mut self,
        storage: Box<dyn crate::local_storage::LocalStorageBridge>,
    ) {
        self.vm.local_storage = Some(storage);
    }

    /// Attach a database backend to the VM.
    pub fn set_db(&mut self, db: Box<dyn crate::db_bridge::DbBridge>) {
        self.vm.db = Some(db);
    }

    /// Attach a 2D drawing backend to the VM.
    pub fn set_draw(&mut self, draw: Box<dyn crate::draw_bridge::DrawBridge>) {
        self.vm.draw = Some(draw);
    }

    /// Attach a layout engine backend to the VM.
    pub fn set_layout(&mut self, layout: Box<dyn crate::layout_bridge::LayoutBridge>) {
        self.vm.layout = Some(layout);
    }

    /// Attach an input/event state backend to the VM.
    pub fn set_input(&mut self, input: Box<dyn crate::input_bridge::InputBridge>) {
        self.vm.input = Some(input);
    }

    /// Attach an HTTP backend to the VM (server-side).
    pub fn set_http(&mut self, http: Box<dyn crate::http_bridge::HttpBridge>) {
        self.vm.http = Some(http);
    }

    /// Attach a file system backend to the VM (server-side, scoped).
    pub fn set_fs(&mut self, fs: Box<dyn crate::fs_bridge::FsBridge>) {
        self.vm.fs = Some(fs);
    }

    /// Attach an environment variable backend to the VM (server-side).
    pub fn set_env(&mut self, env: Box<dyn crate::env_bridge::EnvBridge>) {
        self.vm.env = Some(env);
    }

    /// Set execution limits for script calls. Useful for server environments
    /// where untrusted scripts must be bounded.
    pub fn set_execution_limits(
        &mut self,
        max_instructions: Option<u64>,
        max_wall_time_ms: Option<u64>,
    ) {
        self.vm.config.max_instructions = max_instructions;
        self.vm.config.max_wall_time_ms = max_wall_time_ms;
        self.vm.enforce_limits =
            max_instructions.is_some() || max_wall_time_ms.is_some();
    }

    /// Read-only access to the VM (for inspecting localStorage etc.).
    pub fn vm(&self) -> &VM {
        &self.vm
    }

    /// Mutable access to the VM (for localStorage mutations etc.).
    pub fn vm_mut(&mut self) -> &mut VM {
        &mut self.vm
    }

    /// Run garbage collection on the VM heap.
    ///
    /// The heap is append-only during normal execution — temporary objects from
    /// function calls accumulate and are never freed. This method performs a
    /// mark-sweep to identify heap objects still referenced by globals, then
    /// either nulls out unreachable slots or compacts the heap.
    ///
    /// Called automatically by `call_function` when the heap exceeds a
    /// threshold, or can be called manually.
    pub fn gc_collect(&mut self) {
        let heap_len = self.vm.heap.objects.len();
        if heap_len == 0 {
            return;
        }

        // Phase 1: Mark — find all heap indices reachable from globals.
        let mut reachable = vec![false; heap_len];

        // Scan only initialized global slots (up to high-water mark instead of all 65536).
        let globals_limit = self.vm.globals.high_water_mark();
        for i in 0..globals_limit {
            let val = unsafe { self.vm.globals.get_unchecked(i) };
            if val.is_heap() {
                let idx = val.heap_index() as usize;
                if idx < heap_len {
                    reachable[idx] = true;
                    // Recursively mark objects reachable from this heap object
                    mark_object_refs(&self.vm.heap.objects[idx], &mut reachable, &self.vm.heap);
                }
            }
        }

        // Also scan the VM stack (values below sp may still hold heap refs
        // from the just-completed function call, e.g. closures pushed as args).
        for i in 0..self.vm.sp {
            mark_value(&self.vm.stack[i], &mut reachable, &self.vm.heap);
        }

        // Scan current locals (may contain heap-referencing Objects).
        for obj in &self.vm.locals {
            mark_nested_object(obj, &mut reachable, &self.vm.heap);
        }

        // Scan call frames (each has locals and constants).
        for frame in &self.vm.frames {
            for obj in &frame.locals {
                mark_nested_object(obj, &mut reachable, &self.vm.heap);
            }
            for obj in frame.constants.iter() {
                mark_nested_object(obj, &mut reachable, &self.vm.heap);
            }
        }

        // Scan current constants pool.
        for obj in self.vm.constants.iter() {
            mark_nested_object(obj, &mut reachable, &self.vm.heap);
        }

        // Scan last_popped and arg_buffer for stale heap refs.
        if let Some(ref val) = self.vm.last_popped {
            mark_value(val, &mut reachable, &self.vm.heap);
        }
        for val in &self.vm.arg_buffer {
            mark_value(val, &mut reachable, &self.vm.heap);
        }

        // Scan event listener handler Values (closures registered via addEventListener).
        for handlers in self.vm.event_listeners.values() {
            for val in handlers {
                mark_value(val, &mut reachable, &self.vm.heap);
            }
        }

        // Scan host call callback Values (closures pending async resolution).
        for val in self.vm.host_callbacks.values() {
            mark_value(val, &mut reachable, &self.vm.heap);
        }

        // Scan pre-converted constants cache — contains Values pointing to
        // cloned constants on the heap. Must be marked BEFORE clearing so that
        // any heap objects shared between the cache and globals/constants are
        // not incorrectly freed.
        for (_key, values) in &self.vm.constants_values_cache {
            for val in values {
                mark_value(val, &mut reachable, &self.vm.heap);
            }
        }
        // Also scan the scratch buffer (may contain stale Values from last build)
        for val in &self.vm.constants_values_buf {
            mark_value(val, &mut reachable, &self.vm.heap);
        }

        // Scan pooled locals — returned-to-pool Vec<Object> entries may hold
        // Rc-based types (Hash, Array) that share heap identity.
        for pool_entry in &self.vm.locals_pool {
            for obj in pool_entry {
                mark_nested_object(obj, &mut reachable, &self.vm.heap);
            }
        }

        // Scan new_target (constructor target, may be a heap reference).
        mark_value(&self.vm.new_target, &mut reachable, &self.vm.heap);

        // Scan cached typeof Values (lazily-initialized heap-allocated strings).
        mark_value(&self.vm.typeof_undefined, &mut reachable, &self.vm.heap);
        mark_value(&self.vm.typeof_number, &mut reachable, &self.vm.heap);
        mark_value(&self.vm.typeof_string, &mut reachable, &self.vm.heap);
        mark_value(&self.vm.typeof_boolean, &mut reachable, &self.vm.heap);
        mark_value(&self.vm.typeof_function, &mut reachable, &self.vm.heap);
        mark_value(&self.vm.typeof_object, &mut reachable, &self.vm.heap);

        // Clear pre-converted constants caches — they're rebuilt lazily on
        // next function call. The heap objects they reference are now marked
        // as reachable so they survive this GC cycle; they'll be freed on the
        // next cycle when they're no longer in the cache.
        self.vm.constants_values_cache.clear();
        self.vm.constants_values_ptr = std::ptr::null();
        self.vm.constants_syms_cache.clear();
        self.vm.constants_syms_ptr = std::ptr::null();
        self.vm.last_preconvert_key = usize::MAX;
        self.vm.last_preconvert_values_ptr = std::ptr::null();
        self.vm.last_preconvert_syms_ptr = std::ptr::null();

        // Phase 2: Sweep — null out unreachable heap slots, add to free list.
        // Clear existing free list since we're rebuilding it from scratch.
        self.vm.heap.clear_free_list();
        let mut freed = 0usize;
        for i in 0..heap_len {
            if !reachable[i] {
                let obj = &self.vm.heap.objects[i];
                // Don't null out cheap inline values (they cost nothing to keep)
                if matches!(obj, Object::Null | Object::Undefined
                    | Object::Integer(_) | Object::Float(_) | Object::Boolean(_))
                {
                    continue;
                }
                // Remove Rc pointer from index before nulling
                self.vm.heap.unregister_rc(i as u32);
                self.vm.heap.objects[i] = Object::Null;
                self.vm.heap.add_free(i as u32);
                freed += 1;
            }
        }

        // Phase 3: Truncate trailing nulls to reclaim Vec capacity from the end.
        while self.vm.heap.objects.last().map_or(false, |o| matches!(o, Object::Null)) {
            self.vm.heap.objects.pop();
        }
        // Remove any free-list entries that are now beyond the truncated length
        let new_len = self.vm.heap.objects.len();
        self.vm.heap.trim_free_list(new_len);

        if freed > 0 {
            // Shrink the backing Vec if we freed a lot (> 50% unreachable)
            if new_len < heap_len / 2 {
                self.vm.heap.objects.shrink_to(new_len + 256);
            }
        }
    }

    /// Drain all pending host calls queued by `host.call()` builtins.
    /// Returns the calls and clears the queue.
    pub fn drain_pending_host_calls(&mut self) -> Vec<crate::host_bridge::PendingHostCall> {
        std::mem::take(&mut self.vm.pending_host_calls)
    }

    /// Resolve a pending host callback: looks up the stored callback Value
    /// by call ID and invokes it with the given result object.
    pub fn resolve_host_callback(&mut self, id: u32, result: Object) -> Result<Object, String> {
        let callback = self.vm.host_callbacks.remove(&id)
            .ok_or_else(|| format!("no pending callback for host call id {}", id))?;
        self.call_value(callback, &[result])
    }
}

/// Recursively mark heap slots reachable from an Object's nested contents.
///
/// This must comprehensively handle ALL Object variants that can contain
/// Values (NaN-boxed heap references) or nested Objects (which may themselves
/// contain Values or share Rc identity with heap entries).
fn mark_object_refs(obj: &Object, reachable: &mut [bool], heap: &Heap) {
    match obj {
        Object::Hash(hash_rc) => {
            let h = hash_rc.borrow();
            for val in h.pairs.values() {
                mark_value(val, reachable, heap);
            }
            for val in &h.values {
                mark_value(val, reachable, heap);
            }
            // local_objects: backing store for heap-type Values created outside VM heap
            for obj in &h.local_objects {
                mark_nested_object(obj, reachable, heap);
            }
            // Getter/setter accessor functions may reference heap via constants
            if let Some(getters) = &h.getters {
                for func in getters.values() {
                    mark_compiled_fn_refs(func, reachable, heap);
                }
            }
            if let Some(setters) = &h.setters {
                for func in setters.values() {
                    mark_compiled_fn_refs(func, reachable, heap);
                }
            }
        }
        Object::Array(arr_rc) => {
            let arr = arr_rc.borrow();
            for val in arr.iter() {
                mark_value(val, reachable, heap);
            }
        }
        Object::CompiledFunction(func) => {
            mark_compiled_fn_refs(func, reachable, heap);
        }
        Object::Map(map) => {
            let entries = map.entries.borrow();
            for (_, val) in entries.iter() {
                mark_value(val, reachable, heap);
            }
        }
        Object::Generator(gen_rc) => {
            let gen = gen_rc.borrow();
            for obj in &gen.locals {
                mark_nested_object(obj, reachable, heap);
            }
            for val in &gen.args {
                mark_value(val, reachable, heap);
            }
            if let Some(recv) = &gen.receiver {
                mark_value(recv, reachable, heap);
            }
            mark_compiled_fn_refs(&gen.function, reachable, heap);
        }
        Object::Class(cls) => {
            if let Some(ctor) = &cls.constructor {
                mark_compiled_fn_refs(ctor, reachable, heap);
            }
            for func in cls.methods.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in cls.static_methods.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in cls.getters.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in cls.setters.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in cls.super_methods.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in cls.super_getters.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in cls.super_setters.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for (_, func) in &cls.field_initializers {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for init in &cls.static_initializers {
                match init {
                    crate::object::StaticInitializer::Field { thunk, .. }
                    | crate::object::StaticInitializer::Block { thunk } => {
                        mark_compiled_fn_refs(thunk, reachable, heap);
                    }
                }
            }
            for obj in cls.static_fields.values() {
                mark_nested_object(obj, reachable, heap);
            }
        }
        Object::Instance(inst) => {
            for obj in inst.fields.values() {
                mark_nested_object(obj, reachable, heap);
            }
            for func in inst.methods.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in inst.getters.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in inst.setters.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in inst.super_methods.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in inst.super_getters.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in inst.super_setters.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
        }
        Object::BoundMethod(bm) => {
            mark_compiled_fn_refs(&bm.function, reachable, heap);
            mark_nested_object(&bm.receiver, reachable, heap);
        }
        Object::SuperRef(sr) => {
            mark_nested_object(&sr.receiver, reachable, heap);
            for func in sr.methods.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in sr.getters.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
            for func in sr.setters.values() {
                mark_compiled_fn_refs(func, reachable, heap);
            }
        }
        Object::BuiltinFunction(bf) => {
            if let Some(recv) = &bf.receiver {
                mark_nested_object(recv, reachable, heap);
            }
        }
        Object::ReturnValue(inner) => {
            mark_nested_object(inner, reachable, heap);
        }
        Object::Promise(p) => {
            match &p.settled {
                crate::object::PromiseState::Fulfilled(inner)
                | crate::object::PromiseState::Rejected(inner) => {
                    mark_nested_object(inner, reachable, heap);
                }
            }
        }
        // Integer, Float, Boolean, Null, Undefined, String, RegExp, Set,
        // Error — no heap references.
        _ => {}
    }
}

/// Mark a NaN-boxed Value if it references a heap slot.
fn mark_value(val: &Value, reachable: &mut [bool], heap: &Heap) {
    if val.is_heap() {
        let idx = val.heap_index() as usize;
        if idx < reachable.len() && !reachable[idx] {
            reachable[idx] = true;
            mark_object_refs(&heap.objects[idx], reachable, heap);
        }
    }
}

/// Mark heap references from a CompiledFunctionObject's constant pool.
/// Constants may share Rc identity with heap entries (Hash, Array, Generator).
fn mark_compiled_fn_refs(
    func: &crate::object::CompiledFunctionObject,
    reachable: &mut [bool],
    heap: &Heap,
) {
    for c in func.constants.iter() {
        mark_nested_object(c, reachable, heap);
    }
}

/// Mark an Object that is NOT a direct heap entry (e.g., a field value,
/// local_objects entry, or constant pool entry). If it's an Rc-based type
/// (Hash, Array, Generator), find and mark the matching heap slot. Then
/// recursively scan its contents for further heap references.
///
/// Uses Heap::rc_index for O(1) pointer-to-index lookup instead of a linear
/// heap scan, eliminating the O(N²) GC bottleneck for large heaps.
fn mark_nested_object(obj: &Object, reachable: &mut [bool], heap: &Heap) {
    let ptr = match obj {
        Object::Hash(rc) => Rc::as_ptr(rc) as usize,
        Object::Array(rc) => Rc::as_ptr(rc) as usize,
        Object::Generator(rc) => Rc::as_ptr(rc) as usize,
        _ => {
            // Non-Rc type: just scan contents for heap refs
            mark_object_refs(obj, reachable, heap);
            return;
        }
    };

    if let Some(idx) = heap.rc_lookup(ptr) {
        let i = idx as usize;
        if i < reachable.len() && !reachable[i] {
            reachable[i] = true;
            mark_object_refs(&heap.objects[i], reachable, heap);
        }
    } else {
        // Not on heap but its Values may still reference heap objects
        mark_object_refs(obj, reachable, heap);
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::FormLogicEngine;
    use crate::object::Object;

    #[test]
    fn evaluates_basic_arithmetic() {
        let engine = FormLogicEngine::default();
        let out = engine.eval("1 + 2;").expect("eval");
        match out {
            Object::Integer(v) => assert_eq!(v, 3),
            Object::Float(v) => assert!((v - 3.0).abs() < 1e-9),
            _ => panic!("expected numeric output"),
        }
    }

    #[test]
    fn evaluates_let_binding_and_reference() {
        let engine = FormLogicEngine::default();
        let out = engine.eval("let x = 7; x + 5;").expect("eval");
        match out {
            Object::Integer(v) => assert_eq!(v, 12),
            Object::Float(v) => assert!((v - 12.0).abs() < 1e-9),
            _ => panic!("expected numeric output"),
        }
    }

    fn assert_int(obj: Object, expected: i64) {
        match obj {
            Object::Integer(v) => assert_eq!(v, expected),
            Object::Float(v) => assert_eq!(v as i64, expected),
            other => panic!("expected Integer({}), got {:?}", expected, other),
        }
    }

    #[test]
    fn test_init_script_and_call() {
        let engine = FormLogicEngine::default();
        let mut state = engine
            .init_script("let count = 0; function inc() { count = count + 1; }")
            .unwrap();
        assert_int(state.get_global("count").unwrap(), 0);
        state.call_function("inc", &[]).unwrap();
        assert_int(state.get_global("count").unwrap(), 1);
        state.call_function("inc", &[]).unwrap();
        assert_int(state.get_global("count").unwrap(), 2);
    }

    #[test]
    fn test_init_script_function_with_args() {
        let engine = FormLogicEngine::default();
        let mut state = engine
            .init_script("let total = 0; function add(x) { total = total + x; }")
            .unwrap();
        state.call_function("add", &[Object::Integer(5)]).unwrap();
        assert_int(state.get_global("total").unwrap(), 5);
        state.call_function("add", &[Object::Integer(3)]).unwrap();
        assert_int(state.get_global("total").unwrap(), 8);
    }

    #[test]
    fn test_init_script_set_global() {
        let engine = FormLogicEngine::default();
        let mut state = engine
            .init_script("let count = 0; function read() { return count; }")
            .unwrap();
        state.set_global("count", Object::Integer(42)).unwrap();
        assert_int(state.get_global("count").unwrap(), 42);
    }

    #[test]
    fn test_init_script_function_return_value() {
        let engine = FormLogicEngine::default();
        let mut state = engine
            .init_script("let x = 10; function double() { return x * 2; }")
            .unwrap();
        let result = state.call_function("double", &[]).unwrap();
        assert_int(result, 20);
    }

    #[test]
    fn test_init_script_undefined_var_error() {
        let engine = FormLogicEngine::default();
        let state = engine.init_script("let x = 1;").unwrap();
        assert!(state.get_global("nonexistent").is_err());
    }

    /// Regression test: self-recursive function with a for-loop should
    /// correctly process all loop iterations at every recursion depth.
    /// The bug was that the self-recursion fast path in call_register_direct
    /// corrupted for-loop state, causing inner loops to terminate early.
    #[test]
    fn test_self_recursion_for_loop() {
        let engine = FormLogicEngine::default();
        // recurse(depth): at depth 0, returns 1. At depth > 0, iterates
        // over a 3-element array, recursively calling recurse(depth-1)
        // for each element, summing the results.
        // Expected: recurse(0) = 1
        //           recurse(1) = 3  (3 elements × 1)
        //           recurse(2) = 9  (3 elements × 3)
        //           recurse(3) = 27 (3 elements × 9)
        let out = engine
            .eval(
                r#"
                function recurse(depth) {
                    if (depth <= 0) return 1;
                    let items = [10, 20, 30];
                    let sum = 0;
                    for (let i = 0; i < items.length; i++) {
                        sum = sum + recurse(depth - 1);
                    }
                    return sum;
                }
                recurse(3);
                "#,
            )
            .expect("eval");
        assert_int(out, 27);
    }

    /// Variant using for-of loop (another common pattern).
    #[test]
    fn test_self_recursion_for_of_loop() {
        let engine = FormLogicEngine::default();
        let out = engine
            .eval(
                r#"
                function recurse(depth) {
                    if (depth <= 0) return 1;
                    let items = ["a", "b", "c"];
                    let sum = 0;
                    for (let item of items) {
                        sum = sum + recurse(depth - 1);
                    }
                    return sum;
                }
                recurse(3);
                "#,
            )
            .expect("eval");
        assert_int(out, 27);
    }

    /// Test with depth=1 to isolate minimal recursion case.
    #[test]
    fn test_self_recursion_depth_1() {
        let engine = FormLogicEngine::default();
        let out = engine
            .eval(
                r#"
                function recurse(depth) {
                    if (depth <= 0) return 1;
                    let items = [10, 20, 30];
                    let sum = 0;
                    for (let i = 0; i < items.length; i++) {
                        sum = sum + recurse(depth - 1);
                    }
                    return sum;
                }
                recurse(1);
                "#,
            )
            .expect("eval");
        assert_int(out, 3);
    }

    /// Test with while loop to see if it's for-specific.
    #[test]
    fn test_self_recursion_while_loop() {
        let engine = FormLogicEngine::default();
        let out = engine
            .eval(
                r#"
                function recurse(depth) {
                    if (depth <= 0) return 1;
                    let items = [10, 20, 30];
                    let sum = 0;
                    let i = 0;
                    while (i < items.length) {
                        sum = sum + recurse(depth - 1);
                        i = i + 1;
                    }
                    return sum;
                }
                recurse(3);
                "#,
            )
            .expect("eval");
        assert_int(out, 27);
    }

    /// Test with depth=2 to see what second level returns.
    #[test]
    fn test_self_recursion_depth_2() {
        let engine = FormLogicEngine::default();
        let out = engine
            .eval(
                r#"
                function recurse(depth) {
                    if (depth <= 0) return 1;
                    let sum = 0;
                    for (let i = 0; i < 3; i++) {
                        sum = sum + recurse(depth - 1);
                    }
                    return sum;
                }
                recurse(2);
                "#,
            )
            .expect("eval");
        assert_int(out, 9);
    }

    /// Test: is the issue the length property access or the numeric constant?
    #[test]
    fn test_self_recursion_hardcoded_bound() {
        let engine = FormLogicEngine::default();
        let out = engine
            .eval(
                r#"
                function recurse(depth) {
                    if (depth <= 0) return 1;
                    let sum = 0;
                    for (let i = 0; i < 3; i++) {
                        sum = sum + recurse(depth - 1);
                    }
                    return sum;
                }
                recurse(3);
                "#,
            )
            .expect("eval");
        assert_int(out, 27);
    }

    /// Test: init_script + call_function where A calls B, B defined after A.
    /// This reproduces the _render / _dispatchEvents pattern.
    #[test]
    fn test_init_script_forward_call() {
        let engine = FormLogicEngine::default();
        let mut state = engine
            .init_script(
                r#"
                function A() {
                    return B();
                }
                function B() {
                    return 42;
                }
                "#,
            )
            .expect("init_script");
        let result = state.call_function("A", &[]).expect("call A");
        assert_int(result, 42);
    }

    /// Test: init_script + call_function where A calls B and reads a top-level let.
    #[test]
    fn test_init_script_forward_call_with_let() {
        let engine = FormLogicEngine::default();
        let mut state = engine
            .init_script(
                r#"
                let counter = 10;
                function A() {
                    return B() + counter;
                }
                function B() {
                    return 5;
                }
                "#,
            )
            .expect("init_script");
        let result = state.call_function("A", &[]).expect("call A");
        assert_int(result, 15);
    }

    /// Test: init_script + call_function with multiple forward references
    /// (mimics runtime.logic with _render calling _dispatchEvents, buildLayout, etc.)
    #[test]
    fn test_init_script_multiple_forward_refs() {
        let engine = FormLogicEngine::default();
        let mut state = engine
            .init_script(
                r#"
                let state = 0;
                function render() {
                    dispatchEvents();
                    let result = buildLayout();
                    return result + state;
                }
                function setBuilderFn(fn_ref) {
                    state = 100;
                }
                function dispatchEvents() {
                    state = state + 1;
                }
                function buildLayout() {
                    return 7;
                }
                setBuilderFn(null);
                "#,
            )
            .expect("init_script");
        let result = state.call_function("render", &[]).expect("call render");
        // dispatchEvents increments state from 100 to 101, buildLayout returns 7
        // result = 7 + 101 = 108
        assert_int(result, 108);
    }

    /// Test: mimics _render() calling _builderFn() which is a callback stored in a let.
    /// This is the actual pattern in runtime.logic.
    #[test]
    fn test_init_script_callback_in_let() {
        let engine = FormLogicEngine::default();
        let mut state = engine
            .init_script(
                r#"
                let _builderFn = null;
                let _rootNode = null;
                
                function _render() {
                    if (_builderFn) {
                        _rootNode = _builderFn();
                    }
                    if (!_rootNode) return 0;
                    return _rootNode;
                }
                
                function setBuilderFn(fn) {
                    _builderFn = fn;
                }
                
                function buildApp() {
                    return 42;
                }
                
                setBuilderFn(buildApp);
                "#,
            )
            .expect("init_script");
        let result = state.call_function("_render", &[]).expect("call _render");
        assert_int(result, 42);
    }

    /// Test: mimics the pattern where imports inject code between function definitions.
    /// In runtime.logic, _render is defined before the imports, and _dispatchEvents
    /// is defined after the imports. The imports define lots of functions/variables.
    #[test]
    fn test_init_script_code_between_functions() {
        let engine = FormLogicEngine::default();
        let mut state = engine
            .init_script(
                r#"
                let _needsRebuild = true;
                let _rootNode = null;
                let _builderFn = null;
                
                function buildLayout(node) {
                    return 1;
                }
                
                function computeLayout(node, w, h) {
                    return 2;
                }
                
                function renderTree(node) {
                    return 3;
                }
                
                function _render() {
                    if (_builderFn) {
                        _rootNode = _builderFn();
                        _needsRebuild = true;
                    }
                    if (!_rootNode) return 0;
                    _dispatchEvents();
                    if (_needsRebuild) {
                        buildLayout(_rootNode);
                        _needsRebuild = false;
                    }
                    computeLayout(_rootNode, 100, 100);
                    renderTree(_rootNode);
                    return 99;
                }
                
                // Simulated imported code (goes between _render and _dispatchEvents)
                let importedVar1 = "hello";
                let importedVar2 = "world";
                function importedFn1() { return 10; }
                function importedFn2() { return 20; }
                let importedVar3 = importedFn1() + importedFn2();
                
                function _dispatchEvents() {
                    // does nothing for this test
                }
                
                function buildApp() {
                    return { type: "Box" };
                }
                
                function setBuilderFn(fn) {
                    _builderFn = fn;
                }
                
                setBuilderFn(buildApp);
                "#,
            )
            .expect("init_script");
        let result = state.call_function("_render", &[]).expect("call _render");
        assert_int(result, 99);
    }

    /// Integration test: compile and run the actual counter.logic resolved source.
    /// This tests the real-world scenario with ~4800 lines of code.
    #[test]
    fn test_counter_resolved_source() {
        use crate::draw_bridge::DrawBridge;
        use crate::input_bridge::InputBridge;
        use crate::layout_bridge::{LayoutBridge, LayoutStyle};

        // Mock draw bridge that returns non-zero viewport dimensions
        struct MockDraw;
        impl DrawBridge for MockDraw {
            fn draw_rect(
                &mut self,
                _: f64,
                _: f64,
                _: f64,
                _: f64,
                _: &str,
                _: f64,
                _: f64,
                _: &str,
                _: f64,
            ) {
            }
            fn draw_rounded_rect(
                &mut self,
                _: f64,
                _: f64,
                _: f64,
                _: f64,
                _: [f64; 4],
                _: &str,
                _: f64,
            ) {
            }
            fn draw_circle(&mut self, _: f64, _: f64, _: f64, _: &str, _: f64) {}
            fn draw_ellipse(&mut self, _: f64, _: f64, _: f64, _: f64, _: &str, _: f64) {}
            fn draw_line(&mut self, _: f64, _: f64, _: f64, _: f64, _: &str, _: f64) {}
            fn draw_path(&mut self, _: &str, _: &str, _: &str, _: f64, _: f64) {}
            fn draw_text(
                &mut self,
                _: &str,
                _: f64,
                _: f64,
                _: f64,
                _: &str,
                _: u32,
                _: &str,
                _: f64,
                _: f64,
            ) -> (f64, f64) {
                (0.0, 0.0)
            }
            fn draw_image(&mut self, _: &str, _: f64, _: f64, _: f64, _: f64, _: f64) {}
            fn draw_linear_gradient(
                &mut self,
                _: f64,
                _: f64,
                _: f64,
                _: f64,
                _: f64,
                _: &[f64],
                _: f64,
            ) {
            }
            fn draw_radial_gradient(&mut self, _: f64, _: f64, _: f64, _: f64, _: &[f64], _: f64) {}
            fn draw_shadow(
                &mut self,
                _: f64,
                _: f64,
                _: f64,
                _: f64,
                _: f64,
                _: f64,
                _: &str,
                _: f64,
                _: f64,
                _: f64,
            ) {
            }
            fn push_clip(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64) {}
            fn pop_clip(&mut self) {}
            fn push_transform(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64) {}
            fn pop_transform(&mut self) {}
            fn push_opacity(&mut self, _: f64) {}
            fn pop_opacity(&mut self) {}
            fn draw_arc(&mut self, _: f64, _: f64, _: f64, _: f64, _: f64, _: f64, _: &str) {}
            fn measure_text(&self, _: &str, _: f64, _: u32, _: &str, _: f64) -> (f64, f64) {
                (0.0, 0.0)
            }
            fn get_viewport_width(&self) -> f64 {
                800.0
            }
            fn get_viewport_height(&self) -> f64 {
                600.0
            }
        }

        // Mock layout bridge
        struct MockLayout;
        impl LayoutBridge for MockLayout {
            fn create_node(&mut self, _: LayoutStyle) -> u64 {
                0
            }
            fn update_style(&mut self, _: u64, _: LayoutStyle) {}
            fn set_children(&mut self, _: u64, _: &[u64]) {}
            fn compute_layout(&mut self, _: u64, _: f64, _: f64) {}
            fn get_layout(&self, _: u64) -> (f64, f64, f64, f64) {
                (0.0, 0.0, 100.0, 50.0)
            }
            fn remove_node(&mut self, _: u64) {}
            fn clear(&mut self) {}
        }

        // Mock input bridge
        struct MockInput;
        impl InputBridge for MockInput {
            fn get_mouse_x(&self) -> f64 {
                0.0
            }
            fn get_mouse_y(&self) -> f64 {
                0.0
            }
            fn is_mouse_down(&self) -> bool {
                false
            }
            fn is_mouse_pressed(&self) -> bool {
                false
            }
            fn is_mouse_released(&self) -> bool {
                false
            }
            fn get_scroll_y(&self) -> f64 {
                0.0
            }
            fn set_cursor(&mut self, _: &str) {}
            fn get_text_input(&self) -> String {
                String::new()
            }
            fn is_backspace_pressed(&self) -> bool {
                false
            }
            fn is_escape_pressed(&self) -> bool {
                false
            }
            fn request_redraw(&mut self) {}
            fn get_elapsed_secs(&self) -> f64 {
                0.0
            }
            fn get_page_elapsed_secs(&self) -> f64 {
                0.0
            }
            fn get_delta_time(&self) -> f64 {
                0.016
            }
            fn get_focused_input(&self) -> Option<String> {
                None
            }
            fn set_focused_input(&mut self, _: Option<&str>) {}
            fn is_key_down(&self, _: &str) -> bool {
                false
            }
        }

        let source = include_str!("../tests/counter_resolved.logic");
        let engine = FormLogicEngine::default();
        let mut state = engine.init_script(source).expect("init_script failed");

        // Attach mock bridges
        state.set_draw(Box::new(MockDraw));
        state.set_layout(Box::new(MockLayout));
        state.set_input(Box::new(MockInput));

        // Now call _render — this should exercise the full rendering pipeline
        match state.call_function("_render", &[]) {
            Ok(_) => eprintln!("_render succeeded!"),
            Err(e) => {
                // Print some diagnostics
                eprintln!("_render FAILED: {}", e);

                // Dump all global slots that are Undefined
                let mut undefined_globals = vec![];
                for (name, &slot) in &state.globals_table {
                    let val = unsafe { state.vm.globals.get_unchecked(slot as usize) };
                    if val.is_undefined() {
                        undefined_globals.push((name.clone(), slot));
                    }
                }
                undefined_globals.sort_by_key(|&(_, s)| s);
                eprintln!("Global slots that are Undefined:");
                for (name, slot) in &undefined_globals {
                    eprintln!("  slot {}: {}", slot, name);
                }

                panic!("_render failed: {}", e);
            }
        }
    }

}
