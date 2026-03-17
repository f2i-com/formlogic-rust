//! Register-based VM dispatch loop.
//!
//! This module adds `run_register()` and `rdispatch_loop()` to the existing VM
//! struct. Registers are mapped into the VM's value stack: `stack[reg_base + i]`.
//! The stack, globals, heap, config, and all helper methods are shared with
//! the stack-based dispatch.

use std::rc::Rc;

use crate::object::{make_array, make_hash, Object, PromiseObject, PromiseState, SuperRefObject};
use crate::rcode::ROp;
use crate::value::{obj_into_val, val_as_obj_ref, val_inspect, val_to_obj, Value};
use crate::vm::{VMError, MAX_ARRAY_SIZE, STACK_SIZE, VM};

impl VM {
    /// Pre-convert Object constants to NaN-boxed Values, caching by raw pointer
    /// to avoid repeated heap allocation for string/function constants.
    /// On cache hit, sets `constants_values_ptr` directly — no Vec copy.
    pub(crate) fn preconvert_constants(&mut self) {
        let key = self.constants_raw as usize;
        // Fast path: same function as last lookup (e.g. add() called 1000× in a loop).
        // Single u64 comparison skips the entire cache scan.
        if key == self.last_preconvert_key {
            self.constants_values_ptr = self.last_preconvert_values_ptr;
            self.constants_syms_ptr = self.last_preconvert_syms_ptr;
            return;
        }
        // Check cache (linear scan — typically <10 unique functions)
        for (i, entry) in self.constants_values_cache.iter().enumerate() {
            if entry.0 == key {
                self.constants_values_ptr = entry.1.as_ptr();
                self.constants_syms_ptr = self.constants_syms_cache[i].1.as_ptr();
                self.last_preconvert_key = key;
                self.last_preconvert_values_ptr = self.constants_values_ptr;
                self.last_preconvert_syms_ptr = self.constants_syms_ptr;
                return;
            }
        }
        // Cache miss: convert all constants into scratch buffer
        // SAFETY: constants_raw is always set before preconvert_constants is called,
        // and the underlying Rc keeps the data alive throughout VM execution.
        let constants = unsafe { &*self.constants_raw };
        self.constants_values_buf.clear();
        self.constants_values_buf.reserve(constants.len());
        self.constants_syms_buf.clear();
        self.constants_syms_buf.reserve(constants.len());
        for obj in constants.iter() {
            // Migrate local_objects in compiler-constructed hashes to VM heap
            if let Object::Hash(hash_rc) = obj {
                let hash = hash_rc.borrow_mut();
                hash.migrate_local_objects(&mut self.heap);
            }
            let val = match obj {
                Object::Integer(v) => Value::from_i64(*v),
                Object::Float(v) => Value::from_f64(*v),
                Object::Boolean(v) => Value::from_bool(*v),
                Object::Null => Value::NULL,
                Object::Undefined => Value::UNDEFINED,
                other => obj_into_val(VM::clone_object_fast(other), &mut self.heap),
            };
            self.constants_values_buf.push(val);
            // Pre-intern string constants as symbol IDs
            let sym = match obj {
                Object::String(s) => crate::intern::intern_rc(s),
                _ => 0,
            };
            self.constants_syms_buf.push(sym);
        }
        self.constants_values_cache
            .push((key, self.constants_values_buf.clone()));
        self.constants_syms_cache
            .push((key, self.constants_syms_buf.clone()));
        // SAFETY: Cache entries are never removed during VM execution, so the
        // pointer into the last cache entry remains valid.
        let entry = self.constants_values_cache.last().unwrap();
        self.constants_values_ptr = entry.1.as_ptr();
        let sym_entry = self.constants_syms_cache.last().unwrap();
        self.constants_syms_ptr = sym_entry.1.as_ptr();
        self.last_preconvert_key = key;
        self.last_preconvert_values_ptr = self.constants_values_ptr;
        self.last_preconvert_syms_ptr = self.constants_syms_ptr;
    }

    /// Run register-based bytecode. Call this instead of `run()` when the
    /// bytecode was emitted by `RCompiler`.
    pub fn run_register(&mut self) -> Result<(), VMError> {
        let entry_depth = self.frames.len();
        let reg_base = self.sp;
        let reg_window = (self.register_count as usize).max(1);

        // Ensure stack has capacity for register window
        if reg_base + reg_window > STACK_SIZE {
            return Err(VMError::StackOverflow);
        }
        // Pre-reserve full stack capacity to avoid reallocation in recursive calls
        self.stack
            .reserve(STACK_SIZE.saturating_sub(self.stack.len()));
        while self.stack.len() < reg_base + reg_window {
            self.stack.push(Value::UNDEFINED);
        }

        // Set raw constants pointer for the register dispatch loop
        self.constants_raw = &*self.constants as *const Vec<Object>;
        // Pre-convert constants to Values (cached by Rc pointer)
        self.preconvert_constants();

        // Set sp past the register window so stack-pushing helper methods
        // (get_property_fast_path, execute_index_expression, etc.) operate
        // above our register window and don't clobber registers.
        self.sp = reg_base + reg_window;

        match self.rdispatch_loop(entry_depth, reg_base) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.unwind_frames(entry_depth);
                Err(e)
            }
        }
    }

    /// Read a u8 operand at the given byte offset.
    #[inline(always)]
    fn read_u8_at(&self, offset: usize) -> u8 {
        debug_assert!(offset < self.inst_len);
        unsafe { *self.inst_ptr.add(offset) }
    }

    /// Read 3 u16 register operands at ip+1, ip+3, ip+5.
    #[inline(always)]
    fn read_3u16_operands(&self, ip: usize) -> (usize, usize, usize) {
        (
            self.read_u16(ip + 1) as usize,
            self.read_u16(ip + 3) as usize,
            self.read_u16(ip + 5) as usize,
        )
    }

    /// Register dispatch loop. `reg_base` is the stack offset of register 0.
    pub(crate) fn rdispatch_loop(
        &mut self,
        entry_depth: usize,
        reg_base: usize,
    ) -> Result<(), VMError> {
        let mut ip = self.ip;
        // SAFETY: Stack is pre-allocated to STACK_SIZE and never reallocates.
        // reg_base is fixed for this frame. Using a raw pointer lets the compiler
        // keep it in a register instead of reloading Vec metadata on every access.
        let mut regs: *mut Value = unsafe { self.stack.as_mut_ptr().add(reg_base) };
        loop {
            // SAFETY: bytecode is generated by our compiler, opcodes are always valid.
            // Skip the from_byte bounds check to save one branch per dispatch.
            let op: ROp = unsafe { std::mem::transmute(*self.inst_ptr.add(ip)) };

            // Save pre-execution state for ZK trace
            let trace_ip = ip;
            let trace_op = op as u8;

            match op {
                ROp::LoadConst => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let idx = self.read_u16(ip + 3) as usize;
                    // Pre-converted: single 8-byte copy, zero allocation
                    unsafe { *regs.add(dst) = *self.constants_values_ptr.add(idx) };
                    ip += 5;
                }
                ROp::LoadTrue => {
                    let dst = self.read_u16(ip + 1) as usize;
                    unsafe { *regs.add(dst) = Value::TRUE };
                    ip += 3;
                }
                ROp::LoadFalse => {
                    let dst = self.read_u16(ip + 1) as usize;
                    unsafe { *regs.add(dst) = Value::FALSE };
                    ip += 3;
                }
                ROp::LoadNull => {
                    let dst = self.read_u16(ip + 1) as usize;
                    unsafe { *regs.add(dst) = Value::NULL };
                    ip += 3;
                }
                ROp::LoadUndef => {
                    let dst = self.read_u16(ip + 1) as usize;
                    unsafe { *regs.add(dst) = Value::UNDEFINED };
                    ip += 3;
                }
                ROp::Move => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let src = self.read_u16(ip + 3) as usize;
                    unsafe { *regs.add(dst) = *regs.add(src) };
                    ip += 5;
                }
                ROp::GetGlobal => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let idx = self.read_u16(ip + 3) as usize;
                    unsafe { *regs.add(dst) = self.get_global_as_value(idx) };
                    ip += 5;
                }
                ROp::SetGlobal => {
                    let idx = self.read_u16(ip + 1) as usize;
                    let src = self.read_u16(ip + 3) as usize;
                    let val = unsafe { *regs.add(src) };
                    unsafe { self.globals.set_unchecked(idx, val) };
                    ip += 5;
                }

                // ── Arithmetic ──────────────────────────────────────────
                ROp::Add => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    // SAFETY: register indices from compiler are within pre-allocated window
                    let left = unsafe { *regs.add(left_r) };
                    let right = unsafe { *regs.add(right_r) };
                    if Value::both_i32(left, right) {
                        let a = unsafe { left.as_i32_unchecked() };
                        let b = unsafe { right.as_i32_unchecked() };
                        unsafe {
                            *regs.add(dst) = match a.checked_add(b) {
                                Some(sum) => Value::from_i32(sum),
                                None => Value::from_f64(a as f64 + b as f64),
                            }
                        };
                    } else if left.is_number() && right.is_number() {
                        unsafe {
                            *regs.add(dst) = Value::from_f64(left.to_number() + right.to_number())
                        };
                    } else if left.is_heap() || left.is_inline_str() {
                        unsafe { *regs.add(dst) = self.add_string_or_object(left, right)? };
                    } else {
                        unsafe { *regs.add(dst) = self.add_slow(left, right)? };
                    }
                    ip += 7;
                }
                ROp::Sub => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let left = unsafe { *regs.add(left_r) };
                    let right = unsafe { *regs.add(right_r) };
                    if Value::both_i32(left, right) {
                        let a = unsafe { left.as_i32_unchecked() };
                        let b = unsafe { right.as_i32_unchecked() };
                        unsafe {
                            *regs.add(dst) = match a.checked_sub(b) {
                                Some(diff) => Value::from_i32(diff),
                                None => Value::from_f64(a as f64 - b as f64),
                            }
                        };
                    } else if left.is_number() && right.is_number() {
                        unsafe {
                            *regs.add(dst) = Value::from_f64(left.to_number() - right.to_number())
                        };
                    } else {
                        unsafe { *regs.add(dst) = self.sub_slow(left, right)? };
                    }
                    ip += 7;
                }
                ROp::Mul => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let left = unsafe { *regs.add(left_r) };
                    let right = unsafe { *regs.add(right_r) };
                    if Value::both_i32(left, right) {
                        let a = unsafe { left.as_i32_unchecked() };
                        let b = unsafe { right.as_i32_unchecked() };
                        unsafe {
                            *regs.add(dst) = match a.checked_mul(b) {
                                Some(prod) => Value::from_i32(prod),
                                None => Value::from_f64(a as f64 * b as f64),
                            }
                        };
                    } else if left.is_number() && right.is_number() {
                        unsafe {
                            *regs.add(dst) = Value::from_f64(left.to_number() * right.to_number())
                        };
                    } else {
                        unsafe { *regs.add(dst) = self.mul_slow(left, right)? };
                    }
                    ip += 7;
                }
                ROp::Div => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let left = unsafe { *regs.add(left_r) };
                    let right = unsafe { *regs.add(right_r) };
                    if Value::both_i32(left, right) {
                        let a = unsafe { left.as_i32_unchecked() };
                        let b = unsafe { right.as_i32_unchecked() };
                        if b != 0 && a % b == 0 {
                            unsafe { *regs.add(dst) = Value::from_i32(a / b) };
                        } else {
                            unsafe { *regs.add(dst) = Value::from_f64(a as f64 / b as f64) };
                        }
                    } else if left.is_number() && right.is_number() {
                        unsafe {
                            *regs.add(dst) = Value::from_f64(left.to_number() / right.to_number())
                        };
                    } else {
                        unsafe { *regs.add(dst) = self.div_slow(left, right)? };
                    }
                    ip += 7;
                }
                ROp::Mod => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let left = unsafe { *regs.add(left_r) };
                    let right = unsafe { *regs.add(right_r) };
                    if Value::both_i32(left, right) {
                        let a = unsafe { left.as_i32_unchecked() };
                        let b = unsafe { right.as_i32_unchecked() };
                        if b != 0 {
                            unsafe { *regs.add(dst) = Value::from_i32(a % b) };
                        } else {
                            unsafe { *regs.add(dst) = Value::from_f64(f64::NAN) };
                        }
                    } else if left.is_number() && right.is_number() {
                        unsafe {
                            *regs.add(dst) = Value::from_f64(left.to_number() % right.to_number())
                        };
                    } else {
                        unsafe { *regs.add(dst) = self.mod_slow(left, right)? };
                    }
                    ip += 7;
                }
                ROp::Pow => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };
                    unsafe { *regs.add(dst) = self.pow_impl(lv, rv)? };
                    ip += 7;
                }

                // ── Strict equality with i32 + heap-pointer fast paths ──
                ROp::StrictEqual => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };
                    let result = if lv.bits() == rv.bits() {
                        !lv.is_f64() || !lv.as_f64().is_nan()
                    } else if Value::both_i32(lv, rv) {
                        false
                    } else if lv.is_f64() && rv.is_f64() {
                        lv.as_f64() == rv.as_f64()
                    } else if lv.is_number() && rv.is_number() {
                        lv.to_number() == rv.to_number()
                    } else {
                        self.strict_equality_slow(lv, rv)
                    };
                    unsafe { *regs.add(dst) = if result { Value::TRUE } else { Value::FALSE } };
                    ip += 7;
                }
                ROp::StrictNotEqual => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };
                    let result = if lv.bits() == rv.bits() {
                        !lv.is_f64() || !lv.as_f64().is_nan()
                    } else if Value::both_i32(lv, rv) {
                        false
                    } else if lv.is_f64() && rv.is_f64() {
                        lv.as_f64() == rv.as_f64()
                    } else if lv.is_number() && rv.is_number() {
                        lv.to_number() == rv.to_number()
                    } else {
                        self.strict_equality_slow(lv, rv)
                    };
                    unsafe { *regs.add(dst) = if !result { Value::TRUE } else { Value::FALSE } };
                    ip += 7;
                }

                // ── Numeric comparison with i32/f64 fast paths ──────────
                // Split into separate arms to eliminate inner match dispatch
                ROp::LessThan => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };
                    let result = if Value::both_i32(lv, rv) {
                        unsafe { lv.as_i32_unchecked() < rv.as_i32_unchecked() }
                    } else if lv.is_number() && rv.is_number() {
                        lv.to_number() < rv.to_number()
                    } else {
                        self.comparison_slow(ROp::LessThan, lv, rv)?
                    };
                    unsafe { *regs.add(dst) = if result { Value::TRUE } else { Value::FALSE } };
                    ip += 7;
                }
                ROp::LessOrEqual => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };
                    let result = if Value::both_i32(lv, rv) {
                        unsafe { lv.as_i32_unchecked() <= rv.as_i32_unchecked() }
                    } else if lv.is_number() && rv.is_number() {
                        lv.to_number() <= rv.to_number()
                    } else {
                        self.comparison_slow(ROp::LessOrEqual, lv, rv)?
                    };
                    unsafe { *regs.add(dst) = if result { Value::TRUE } else { Value::FALSE } };
                    ip += 7;
                }
                ROp::GreaterThan => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };
                    let result = if Value::both_i32(lv, rv) {
                        unsafe { lv.as_i32_unchecked() > rv.as_i32_unchecked() }
                    } else if lv.is_number() && rv.is_number() {
                        lv.to_number() > rv.to_number()
                    } else {
                        self.comparison_slow(ROp::GreaterThan, lv, rv)?
                    };
                    unsafe { *regs.add(dst) = if result { Value::TRUE } else { Value::FALSE } };
                    ip += 7;
                }
                ROp::GreaterOrEqual => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };
                    let result = if Value::both_i32(lv, rv) {
                        unsafe { lv.as_i32_unchecked() >= rv.as_i32_unchecked() }
                    } else if lv.is_number() && rv.is_number() {
                        lv.to_number() >= rv.to_number()
                    } else {
                        self.comparison_slow(ROp::GreaterOrEqual, lv, rv)?
                    };
                    unsafe { *regs.add(dst) = if result { Value::TRUE } else { Value::FALSE } };
                    ip += 7;
                }

                // ── Equality / other comparison ─────────────────────────
                ROp::Equal => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };
                    let result = if lv.bits() == rv.bits() {
                        !lv.is_f64() || !lv.as_f64().is_nan()
                    } else if Value::both_i32(lv, rv) {
                        false
                    } else if lv.is_number() && rv.is_number() {
                        lv.to_number() == rv.to_number()
                    } else {
                        self.equality_slow(lv, rv)
                    };
                    unsafe { *regs.add(dst) = if result { Value::TRUE } else { Value::FALSE } };
                    ip += 7;
                }
                ROp::NotEqual => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };
                    let result = if lv.bits() == rv.bits() {
                        !lv.is_f64() || !lv.as_f64().is_nan()
                    } else if Value::both_i32(lv, rv) {
                        false
                    } else if lv.is_number() && rv.is_number() {
                        lv.to_number() == rv.to_number()
                    } else {
                        self.equality_slow(lv, rv)
                    };
                    unsafe { *regs.add(dst) = if !result { Value::TRUE } else { Value::FALSE } };
                    ip += 7;
                }
                ROp::Instanceof | ROp::In => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };

                    // Fast path: "key" in hash — peek heap, no val_to_obj
                    if op == ROp::In && rv.is_heap() {
                        let heap_obj =
                            unsafe { &*self.heap.objects.as_ptr().add(rv.heap_index() as usize) };
                        if let Object::Hash(hash_rc) = heap_obj {
                            let hash = hash_rc.borrow();
                            let found = if lv.is_heap() {
                                let key_obj = unsafe {
                                    &*self.heap.objects.as_ptr().add(lv.heap_index() as usize)
                                };
                                if let Object::String(s) = key_obj {
                                    hash.contains_str(s)
                                } else {
                                    let k = self.hash_key_from_value(lv);
                                    hash.pairs.contains_key(&k)
                                }
                            } else if lv.is_inline_str() {
                                let (buf, len) = lv.inline_str_buf();
                                let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
                                let sym = crate::intern::intern(s);
                                hash.pairs.contains_key(&crate::object::HashKey::Sym(sym))
                            } else {
                                let k = self.hash_key_from_value(lv);
                                hash.pairs.contains_key(&k)
                            };
                            unsafe {
                                *regs.add(dst) = if found { Value::TRUE } else { Value::FALSE }
                            };
                            ip += 7;
                            continue;
                        }
                    }

                    // Slow path
                    let lo = val_to_obj(lv, &self.heap);
                    let ro = val_to_obj(rv, &self.heap);
                    let result = self.eval_comparison(op, &lo, &ro)?;
                    unsafe { *regs.add(dst) = result };
                    ip += 7;
                }

                // ── Bitwise ─────────────────────────────────────────────
                ROp::BitwiseAnd
                | ROp::BitwiseOr
                | ROp::BitwiseXor
                | ROp::LeftShift
                | ROp::RightShift
                | ROp::UnsignedRightShift => {
                    let (dst, left_r, right_r) = self.read_3u16_operands(ip);
                    let lv = unsafe { *regs.add(left_r) };
                    let rv = unsafe { *regs.add(right_r) };
                    // Fast path: i32 or f64-that-fits-i32 operands
                    if let (Some(a), Some(b)) = (lv.try_as_i32(), rv.try_as_i32()) {
                        let result = match op {
                            ROp::BitwiseAnd => Value::from_i32(a & b),
                            ROp::BitwiseOr => Value::from_i32(a | b),
                            ROp::BitwiseXor => Value::from_i32(a ^ b),
                            ROp::LeftShift => Value::from_i32(a << (b & 31)),
                            ROp::RightShift => Value::from_i32(a >> (b & 31)),
                            ROp::UnsignedRightShift => {
                                Value::from_i64(((a as u32) >> (b as u32 & 31)) as i64)
                            }
                            _ => unreachable!(),
                        };
                        unsafe { *regs.add(dst) = result };
                    } else {
                        unsafe { *regs.add(dst) = self.bitwise_slow(op, lv, rv)? };
                    }
                    ip += 7;
                }

                // ── Unary ───────────────────────────────────────────────
                ROp::Neg => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let src = self.read_u16(ip + 3) as usize;
                    let val = unsafe { *regs.add(src) };
                    if val.is_i32() {
                        let v = unsafe { val.as_i32_unchecked() };
                        let r = if v == 0 {
                            Value::from_f64(-0.0)
                        } else {
                            match v.checked_neg() {
                                Some(n) => Value::from_i32(n),
                                None => Value::from_f64(-(v as f64)),
                            }
                        };
                        unsafe { *regs.add(dst) = r };
                    } else if val.is_f64() {
                        unsafe { *regs.add(dst) = Value::from_f64(-val.as_f64()) };
                    } else {
                        let obj = val_to_obj(val, &self.heap);
                        let n = self.to_number(&obj)?;
                        unsafe { *regs.add(dst) = Value::from_f64(-n) };
                    }
                    ip += 5;
                }
                ROp::Not => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let src = self.read_u16(ip + 3) as usize;
                    let val = unsafe { *regs.add(src) };
                    // Fast inline: bool from comparison is the most common case
                    let truthy = if val.is_bool() {
                        unsafe { val.as_bool_unchecked() }
                    } else {
                        val.is_truthy_full(&self.heap)
                    };
                    unsafe { *regs.add(dst) = if truthy { Value::FALSE } else { Value::TRUE } };
                    ip += 5;
                }
                ROp::UnaryPlus => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let src = self.read_u16(ip + 3) as usize;
                    let val = unsafe { *regs.add(src) };
                    if val.is_i32() || val.is_f64() {
                        unsafe { *regs.add(dst) = val };
                    } else {
                        let n = self.to_number_val(val)?;
                        unsafe { *regs.add(dst) = Value::from_f64(n) };
                    }
                    ip += 5;
                }
                ROp::Typeof => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let src = self.read_u16(ip + 3) as usize;
                    let value = unsafe { *regs.add(src) };
                    // Lazily initialize typeof cache on first use
                    if self.typeof_undefined.is_undefined() {
                        self.typeof_undefined =
                            obj_into_val(Object::String(Rc::from("undefined")), &mut self.heap);
                        self.typeof_number =
                            obj_into_val(Object::String(Rc::from("number")), &mut self.heap);
                        self.typeof_string =
                            obj_into_val(Object::String(Rc::from("string")), &mut self.heap);
                        self.typeof_boolean =
                            obj_into_val(Object::String(Rc::from("boolean")), &mut self.heap);
                        self.typeof_function =
                            obj_into_val(Object::String(Rc::from("function")), &mut self.heap);
                        self.typeof_object =
                            obj_into_val(Object::String(Rc::from("object")), &mut self.heap);
                        self.typeof_symbol =
                            obj_into_val(Object::String(Rc::from("symbol")), &mut self.heap);
                    }
                    let result = if value.is_undefined() {
                        self.typeof_undefined
                    } else if value.is_null() {
                        self.typeof_object
                    } else if value.is_bool() {
                        self.typeof_boolean
                    } else if value.is_i32() || value.is_f64() {
                        self.typeof_number
                    } else if value.is_inline_str() {
                        self.typeof_string
                    } else if value.is_heap() {
                        let heap_obj = unsafe {
                            &*self.heap.objects.as_ptr().add(value.heap_index() as usize)
                        };
                        match heap_obj {
                            Object::String(_) => self.typeof_string,
                            Object::CompiledFunction(_) | Object::BoundMethod(_) => {
                                self.typeof_function
                            }
                            Object::Symbol(_, _) => self.typeof_symbol,
                            _ => self.typeof_object,
                        }
                    } else {
                        self.typeof_object
                    };
                    unsafe { *regs.add(dst) = result };
                    ip += 5;
                }
                ROp::IsNullish => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let src = self.read_u16(ip + 3) as usize;
                    let val = unsafe { *regs.add(src) };
                    let is_nullish = val.is_null() || val.is_undefined();
                    unsafe {
                        *regs.add(dst) = if is_nullish {
                            Value::TRUE
                        } else {
                            Value::FALSE
                        }
                    };
                    ip += 5;
                }

                // ── Control flow ────────────────────────────────────────
                ROp::Jump => {
                    let target = self.read_u16(ip + 1) as usize;
                    if self.enforce_limits && target <= ip {
                        self.check_execution_limits()?;
                    }
                    ip = target;
                }
                ROp::JumpIfNot => {
                    let cond_r = self.read_u16(ip + 1) as usize;
                    let target = self.read_u16(ip + 3) as usize;
                    let cond = unsafe { *regs.add(cond_r) };
                    // Fast inline: bool from comparison is the most common case
                    let truthy = if cond.is_bool() {
                        unsafe { cond.as_bool_unchecked() }
                    } else {
                        cond.is_truthy_full(&self.heap)
                    };
                    if truthy {
                        ip += 5;
                    } else {
                        ip = target;
                    }
                }
                ROp::JumpIfTruthy => {
                    let cond_r = self.read_u16(ip + 1) as usize;
                    let target = self.read_u16(ip + 3) as usize;
                    let cond = unsafe { *regs.add(cond_r) };
                    let truthy = if cond.is_bool() {
                        unsafe { cond.as_bool_unchecked() }
                    } else {
                        cond.is_truthy_full(&self.heap)
                    };
                    if truthy {
                        ip = target;
                    } else {
                        ip += 5;
                    }
                }

                // ── Function calls ──────────────────────────────────────
                ROp::Call => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let base = self.read_u16(ip + 3) as usize;
                    let nargs = self.read_u8_at(ip + 5) as usize;
                    ip += 6;

                    let callee_val = unsafe { *regs.add(base) };
                    let arg_stack_start = reg_base + base + 1;

                    // Fast path: register→register compiled function call.
                    // Passes Values directly without Object↔Value conversion.
                    if callee_val.is_heap() {
                        let idx = callee_val.heap_index();
                        // Extract function metadata and receiver in one pass.
                        // Use a struct-like tuple to capture everything we need
                        // before dropping the immutable borrow on self.heap.
                        let (fast, receiver, captured_vals) = {
                            let obj = self.heap.get(idx);
                            match obj {
                                Object::CompiledFunction(func) if func.register_count > 0 => {
                                    // Raw pointers — zero Rc clones. Safe because:
                                    // - Heap is append-only, function object lives for VM's lifetime
                                    // - Rc inside function keeps underlying data alive
                                    let cv: Vec<(u16, Value)> = func.captured_values.clone();
                                    (
                                        Some((
                                            func.instructions.as_ptr(),
                                            func.instructions.len(),
                                            &*func.constants as *const Vec<Object>,
                                            func.rest_parameter_index,
                                            func.takes_this,
                                            func.is_async,
                                            func.num_cache_slots,
                                            func.max_stack_depth,
                                            func.register_count,
                                            Rc::as_ptr(&func.inline_cache),
                                            func.is_generator,
                                        )),
                                        None,
                                        cv,
                                    )
                                }
                                Object::BoundMethod(bound) if bound.function.register_count > 0 => {
                                    // Clone receiver as owned Object first, convert later
                                    let receiver_obj = *bound.receiver.clone();
                                    let cv: Vec<(u16, Value)> = bound.function.captured_values.clone();
                                    (
                                        Some((
                                            bound.function.instructions.as_ptr(),
                                            bound.function.instructions.len(),
                                            &*bound.function.constants as *const Vec<Object>,
                                            bound.function.rest_parameter_index,
                                            bound.function.takes_this,
                                            bound.function.is_async,
                                            bound.function.num_cache_slots,
                                            bound.function.max_stack_depth,
                                            bound.function.register_count,
                                            Rc::as_ptr(&bound.function.inline_cache),
                                            bound.function.is_generator,
                                        )),
                                        Some(receiver_obj),
                                        cv,
                                    )
                                }
                                _ => (None, None, vec![]),
                            }
                        };
                        // Immutable borrow on self.heap is dropped here.

                        if let Some((
                            instr,
                            instr_len,
                            consts,
                            rest_idx,
                            takes_this,
                            is_async,
                            cache_slots,
                            max_depth,
                            reg_count,
                            func_cache,
                            is_generator,
                        )) = fast
                        {
                            if is_generator {
                                // Generator function: create GeneratorObject instead of executing.
                                let func_clone = val_to_obj(callee_val, &self.heap);
                                let func_obj = match func_clone {
                                    Object::CompiledFunction(f) => (*f).clone(),
                                    Object::BoundMethod(b) => b.function.clone(),
                                    _ => unreachable!(),
                                };
                                let mut gen_args = Vec::with_capacity(nargs);
                                for i in 0..nargs {
                                    gen_args.push(unsafe { *regs.add(base + 1 + i) });
                                }
                                let receiver_val =
                                    receiver.map(|r| obj_into_val(r, &mut self.heap));
                                self.ip = ip;
                                let gen_val =
                                    self.create_generator(func_obj, gen_args, receiver_val);
                                unsafe { *regs.add(dst) = gen_val };
                                continue;
                            }

                            // Convert receiver (if BoundMethod) now that borrow is released
                            let receiver_val = receiver.map(|r| obj_into_val(r, &mut self.heap));

                            // Inject captured closure values into globals, saving originals
                            let saved_closure: Vec<(u16, Value)> = captured_vals
                                .iter()
                                .map(|&(slot, val)| {
                                    let old = self.get_global_as_value(slot as usize);
                                    unsafe { self.globals.set_unchecked(slot as usize, val) };
                                    (slot, old)
                                })
                                .collect();

                            self.ip = ip;
                            // SAFETY: pointers derived from heap-allocated CompiledFunctionObject
                            let result = unsafe {
                                self.call_register_direct(
                                    instr,
                                    instr_len,
                                    consts,
                                    rest_idx,
                                    takes_this,
                                    is_async,
                                    cache_slots,
                                    max_depth,
                                    reg_count,
                                    func_cache,
                                    arg_stack_start,
                                    nargs,
                                    receiver_val,
                                )
                            };

                            // Restore original global values
                            for (slot, old_val) in saved_closure {
                                unsafe { self.globals.set_unchecked(slot as usize, old_val) };
                            }

                            let result = result?;

                            unsafe { *regs.add(dst) = result };
                            continue;
                        }
                    }

                    // Slow path: builtins, stack-based functions, etc.
                    self.ip = ip;
                    // Check if callee is a super() constructor call.  When it is,
                    // the return value is the updated `this` and must also be
                    // written back to register 0 so the derived constructor sees
                    // properties set by the parent.
                    let is_super_call = callee_val.is_heap()
                        && matches!(self.heap.get(callee_val.heap_index()), Object::SuperRef(_));
                    let result = self.call_slow(callee_val, arg_stack_start, nargs)?;
                    unsafe { *regs.add(dst) = result };
                    if is_super_call {
                        unsafe { *regs.add(0) = result };
                    }
                }
                ROp::CallSpread => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let func_r = self.read_u16(ip + 3) as usize;
                    let args_r = self.read_u16(ip + 5) as usize;
                    ip += 7;

                    let callee_val = unsafe { *regs.add(func_r) };
                    let args_val = unsafe { *regs.add(args_r) };
                    let args: Vec<Value> = if args_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(args_val.heap_index() as usize)
                        };
                        match heap_obj {
                            Object::Array(arr) => arr.borrow().to_vec(),
                            _ => vec![],
                        }
                    } else {
                        vec![]
                    };

                    self.ip = ip;
                    let result = self.call_value_slice(callee_val, &args)?;
                    unsafe { *regs.add(dst) = result };
                }
                ROp::CallGlobal => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let global_idx = self.read_u16(ip + 3) as usize;
                    let base = self.read_u16(ip + 5) as usize;
                    let nargs = self.read_u8_at(ip + 7) as usize;
                    ip += 8;

                    let arg_stack_start = reg_base + base + 1;

                    // Fast path: check if global is a register-based compiled function
                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let fast = if gval.is_heap() {
                        let heap_obj =
                            unsafe { &*self.heap.objects.as_ptr().add(gval.heap_index() as usize) };
                        match heap_obj {
                            Object::CompiledFunction(func) if func.register_count > 0 => {
                                // Raw pointers — zero Rc clones, zero val_to_obj
                                Some((
                                    func.instructions.as_ptr(),
                                    func.instructions.len(),
                                    &*func.constants as *const Vec<Object>,
                                    func.rest_parameter_index,
                                    func.takes_this,
                                    func.is_async,
                                    func.num_cache_slots,
                                    func.max_stack_depth,
                                    func.register_count,
                                    Rc::as_ptr(&func.inline_cache),
                                    func.is_generator,
                                ))
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };

                    if let Some((
                        instr,
                        instr_len,
                        consts,
                        rest_idx,
                        takes_this,
                        is_async,
                        cache_slots,
                        max_depth,
                        reg_count,
                        func_cache,
                        is_generator,
                    )) = fast
                    {
                        if is_generator {
                            let func_clone = val_to_obj(gval, &self.heap);
                            let func_obj = match func_clone {
                                Object::CompiledFunction(f) => (*f).clone(),
                                _ => unreachable!(),
                            };
                            let mut gen_args = Vec::with_capacity(nargs);
                            for i in 0..nargs {
                                gen_args.push(unsafe {
                                    *self.stack.get_unchecked(arg_stack_start + i)
                                });
                            }
                            self.ip = ip;
                            let gen_val = self.create_generator(func_obj, gen_args, None);
                            unsafe { *regs.add(dst) = gen_val };
                            continue;
                        }

                        self.ip = ip;
                        // SAFETY: pointers derived from heap-allocated CompiledFunctionObject
                        let result = unsafe {
                            self.call_register_direct(
                                instr,
                                instr_len,
                                consts,
                                rest_idx,
                                takes_this,
                                is_async,
                                cache_slots,
                                max_depth,
                                reg_count,
                                func_cache,
                                arg_stack_start,
                                nargs,
                                None,
                            )
                        }?;
                        unsafe { *regs.add(dst) = result };
                        continue;
                    }

                    // Slow path: builtins, stack-based functions, etc.
                    self.ip = ip;
                    unsafe { *regs.add(dst) = self.call_slow(gval, arg_stack_start, nargs)? };
                }
                ROp::CallMethod => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let base = self.read_u16(ip + 3) as usize;
                    let nargs = self.read_u8_at(ip + 5) as usize;
                    let prop_idx = self.read_u16(ip + 6) as usize;
                    let cache_slot = self.read_u16(ip + 8) as usize;
                    ip += 10;

                    let obj_val = unsafe { *regs.add(base) };
                    let arg_start = reg_base + base + 1;

                    // Fast path: direct Map/Hash method dispatch
                    if obj_val.is_heap() {
                        let heap_idx = obj_val.heap_index() as usize;
                        // Resolve property symbol from pre-interned constants
                        let prop_sym = unsafe { *self.constants_syms_ptr.add(prop_idx) };

                        // Peek at the heap object type
                        let heap_obj = unsafe { &*self.heap.objects.as_ptr().add(heap_idx) };

                        match heap_obj {
                            Object::Map(map_obj) => {
                                if prop_sym == self.sym_set && nargs >= 2 {
                                    // Value-native: store Value directly, no Object clone
                                    let key = self.hash_key_from_value(unsafe {
                                        *self.stack.get_unchecked(arg_start)
                                    });
                                    let value = unsafe { *self.stack.get_unchecked(arg_start + 1) };
                                    let entries = map_obj.entries.borrow_mut();
                                    let indices = map_obj.indices.borrow_mut();
                                    VM::map_insert_or_replace(entries, indices, key, value);
                                    unsafe { *regs.add(dst) = obj_val };
                                    continue;
                                } else if prop_sym == self.sym_get && nargs >= 1 {
                                    let key = self.hash_key_from_value(unsafe {
                                        *self.stack.get_unchecked(arg_start)
                                    });
                                    let result = VM::map_get(
                                        map_obj.entries.borrow(),
                                        map_obj.indices.borrow(),
                                        &key,
                                    );
                                    unsafe { *regs.add(dst) = result.unwrap_or(Value::UNDEFINED) };
                                    continue;
                                } else if prop_sym == self.sym_has && nargs >= 1 {
                                    let key = self.hash_key_from_value(unsafe {
                                        *self.stack.get_unchecked(arg_start)
                                    });
                                    let has = VM::map_contains(map_obj.indices.borrow(), &key);
                                    unsafe { *regs.add(dst) = Value::from_bool(has) };
                                    continue;
                                } else if prop_sym == self.sym_size {
                                    let len = map_obj.entries.borrow().len() as i64;
                                    unsafe { *regs.add(dst) = Value::from_i64(len) };
                                    continue;
                                }
                                // fall through to slow path
                            }
                            Object::Array(arr_rc) => {
                                if prop_sym == self.sym_push && nargs >= 1 {
                                    let items = arr_rc.borrow_mut();
                                    for i in 0..nargs {
                                        let arg =
                                            unsafe { *self.stack.get_unchecked(arg_start + i) };
                                        items.push(arg);
                                    }
                                    let len = items.len() as i64;
                                    if len as usize > MAX_ARRAY_SIZE {
                                        return Err(VMError::TypeError(
                                            "Array size limit exceeded".to_string(),
                                        ));
                                    }
                                    unsafe { *regs.add(dst) = Value::from_i64(len) };
                                    continue;
                                } else if prop_sym == self.sym_pop && nargs == 0 {
                                    let items = arr_rc.borrow_mut();
                                    match items.pop() {
                                        Some(val) => {
                                            unsafe { *regs.add(dst) = val };
                                        }
                                        None => {
                                            unsafe { *regs.add(dst) = Value::UNDEFINED };
                                        }
                                    }
                                    continue;
                                } else if prop_sym == self.sym_length {
                                    let len = arr_rc.borrow().len() as i64;
                                    unsafe { *regs.add(dst) = Value::from_i64(len) };
                                    continue;
                                } else if prop_sym == self.sym_shift && nargs == 0 {
                                    let items = arr_rc.borrow_mut();
                                    if items.is_empty() {
                                        unsafe { *regs.add(dst) = Value::UNDEFINED };
                                    } else {
                                        let first = items.remove(0);
                                        unsafe { *regs.add(dst) = first };
                                    }
                                    continue;
                                } else if prop_sym == self.sym_unshift && nargs >= 1 {
                                    let items = arr_rc.borrow_mut();
                                    for i in (0..nargs).rev() {
                                        let arg =
                                            unsafe { *self.stack.get_unchecked(arg_start + i) };
                                        items.insert(0, arg);
                                    }
                                    let len = items.len() as i64;
                                    unsafe { *regs.add(dst) = Value::from_i64(len) };
                                    continue;
                                } else if prop_sym == self.sym_splice {
                                    let items = arr_rc.borrow_mut();
                                    let len = items.len() as i64;
                                    let start_raw = if nargs >= 1 {
                                        self.to_i32_val(unsafe {
                                            *self.stack.get_unchecked(arg_start)
                                        })? as i64
                                    } else {
                                        0
                                    };
                                    let start = if start_raw < 0 {
                                        (len + start_raw).max(0) as usize
                                    } else {
                                        (start_raw as usize).min(items.len())
                                    };
                                    let delete_count = if nargs >= 2 {
                                        self.to_i32_val(unsafe {
                                            *self.stack.get_unchecked(arg_start + 1)
                                        })?
                                        .max(0) as usize
                                    } else {
                                        items.len() - start
                                    };
                                    let delete_count = delete_count.min(items.len() - start);
                                    let removed: Vec<Value> =
                                        items.drain(start..start + delete_count).collect();
                                    // Insert new items
                                    for i in 0..(nargs.saturating_sub(2)) {
                                        let arg = unsafe {
                                            *self.stack.get_unchecked(arg_start + 2 + i)
                                        };
                                        items.insert(start + i, arg);
                                    }
                                    let _ = items;
                                    unsafe {
                                        *regs.add(dst) = obj_into_val(
                                            make_array(removed),
                                            &mut self.heap,
                                        )
                                    };
                                    continue;
                                }
                                // fall through to slow path
                            }
                            Object::Hash(hash_rc) => {
                                // Try inline cache for compiled function methods on Hash
                                debug_assert!(cache_slot < self.inline_cache.len());
                                let fast = {
                                    let (cached_shape, cached_offset) =
                                        unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                                    if cached_shape != 0 {
                                        let hash = hash_rc.borrow();
                                        if cached_shape == hash.shape_version {
                                            let slot = cached_offset as usize;
                                            let prop_val =
                                                unsafe { hash.get_value_at_slot_unchecked(slot) };
                                            // Value-native: check if it's a heap ref to CompiledFunction
                                            if prop_val.is_heap() {
                                                let func_obj = self.heap.get(prop_val.heap_index());
                                                if let Object::CompiledFunction(func) = func_obj {
                                                    if func.register_count > 0 {
                                                        // Raw pointers — zero Rc clones
                                                        Some((
                                                            func.instructions.as_ptr(),
                                                            func.instructions.len(),
                                                            &*func.constants as *const Vec<Object>,
                                                            func.rest_parameter_index,
                                                            func.takes_this,
                                                            func.is_async,
                                                            func.num_cache_slots,
                                                            func.max_stack_depth,
                                                            func.register_count,
                                                            Rc::as_ptr(&func.inline_cache),
                                                            func.is_generator,
                                                        ))
                                                    } else {
                                                        None
                                                    }
                                                } else {
                                                    None
                                                }
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                };
                                // hash borrow dropped here
                                if let Some((
                                    instr,
                                    instr_len,
                                    consts,
                                    rest_idx,
                                    takes_this,
                                    is_async,
                                    cache_slots,
                                    max_depth,
                                    reg_count,
                                    func_cache,
                                    is_generator,
                                )) = fast
                                {
                                    if is_generator {
                                        // Generator methods: create GeneratorObject
                                        let prop_val_again = {
                                            let hash = hash_rc.borrow();
                                            let (cs, co) = unsafe {
                                                *self.inline_cache.get_unchecked(cache_slot)
                                            };
                                            if cs == hash.shape_version {
                                                unsafe {
                                                    hash.get_value_at_slot_unchecked(co as usize)
                                                }
                                            } else {
                                                Value::UNDEFINED
                                            }
                                        };
                                        let func_clone = val_to_obj(prop_val_again, &self.heap);
                                        let func_obj = match func_clone {
                                            Object::CompiledFunction(f) => (*f).clone(),
                                            _ => unreachable!(),
                                        };
                                        let mut gen_args = Vec::with_capacity(nargs);
                                        for i in 0..nargs {
                                            gen_args.push(unsafe {
                                                *self.stack.get_unchecked(arg_start + i)
                                            });
                                        }
                                        self.ip = ip;
                                        let gen_val = self.create_generator(
                                            func_obj,
                                            gen_args,
                                            Some(obj_val),
                                        );
                                        unsafe { *regs.add(dst) = gen_val };
                                        continue;
                                    }

                                    self.ip = ip;
                                    // SAFETY: pointers derived from heap-allocated CompiledFunctionObject
                                    let result = unsafe {
                                        self.call_register_direct(
                                            instr,
                                            instr_len,
                                            consts,
                                            rest_idx,
                                            takes_this,
                                            is_async,
                                            cache_slots,
                                            max_depth,
                                            reg_count,
                                            func_cache,
                                            arg_start,
                                            nargs,
                                            Some(obj_val),
                                        )
                                    }?;
                                    unsafe { *regs.add(dst) = result };
                                    continue;
                                }
                            }
                            _ => {} // fall through to slow path
                        }
                    }

                    // Slow path: resolve property + call
                    self.ip = ip;
                    unsafe {
                        *regs.add(dst) =
                            self.call_method_slow(obj_val, prop_idx, cache_slot, nargs, arg_start)?
                    };
                }
                ROp::Return => {
                    let src = self.read_u16(ip + 1) as usize;
                    let rv = unsafe { *regs.add(src) };
                    self.last_popped = Some(rv);
                    if self.frames.len() <= entry_depth {
                        return Ok(());
                    }
                    let _is_async = self.restore_caller_frame();
                    return Ok(());
                }
                ROp::ReturnUndef => {
                    self.last_popped = Some(Value::UNDEFINED);
                    if self.frames.len() <= entry_depth {
                        return Ok(());
                    }
                    let _is_async = self.restore_caller_frame();
                    return Ok(());
                }

                // ── Constructors ────────────────────────────────────────
                ROp::New => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let base = self.read_u16(ip + 3) as usize;
                    let nargs = self.read_u8_at(ip + 5) as usize;
                    ip += 6;

                    let callee = val_to_obj(unsafe { *regs.add(base) }, &self.heap);
                    let mut args = std::mem::take(&mut self.arg_buffer);
                    args.clear();
                    for i in 0..nargs {
                        args.push(unsafe { *regs.add(base + 1 + i) });
                    }

                    // execute_new_with_args_slice pushes result to stack
                    self.ip = ip;
                    let new_result = self.execute_new_with_args_slice(callee, &args);
                    args.clear();
                    self.arg_buffer = args;
                    new_result?;
                    let result = self.pop_val()?;
                    unsafe { *regs.add(dst) = result };
                }
                ROp::NewSpread => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let cls_r = self.read_u16(ip + 3) as usize;
                    let args_r = self.read_u16(ip + 5) as usize;
                    ip += 7;

                    let callee = val_to_obj(unsafe { *regs.add(cls_r) }, &self.heap);
                    let args_val = unsafe { *regs.add(args_r) };
                    let args: Vec<Value> = if args_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(args_val.heap_index() as usize)
                        };
                        match heap_obj {
                            Object::Array(arr) => arr.borrow().to_vec(),
                            _ => vec![],
                        }
                    } else {
                        vec![]
                    };

                    self.ip = ip;
                    self.execute_new_with_args_slice(callee, &args)?;
                    let result = self.pop_val()?;
                    unsafe { *regs.add(dst) = result };
                }
                ROp::Super => {
                    let dst = self.read_u16(ip + 1) as usize;
                    // In register VM, register 0 is "this" (first local)
                    let this_val =
                        val_to_obj(unsafe { *self.stack.get_unchecked(reg_base) }, &self.heap);
                    if let Object::Instance(instance) = this_val {
                        let result = Object::SuperRef(Box::new(SuperRefObject {
                            receiver: Box::new(Object::Instance(Box::new((*instance).clone()))),
                            methods: instance.super_methods.clone(),
                            getters: instance.super_getters.clone(),
                            setters: instance.super_setters.clone(),
                            constructor_chain: instance.super_constructor_chain.clone(),
                        }));
                        unsafe { *regs.add(dst) = obj_into_val(result, &mut self.heap) };
                    } else {
                        unsafe { *regs.add(dst) = Value::UNDEFINED };
                    }
                    ip += 3;
                }

                // ── Collections ─────────────────────────────────────────
                ROp::Array => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let base = self.read_u16(ip + 3) as usize;
                    let count = self.read_u16(ip + 5) as usize;
                    ip += 7;

                    let mut items: Vec<Value> = Vec::with_capacity(count);
                    for i in 0..count {
                        items.push(unsafe { *regs.add(base + i) });
                    }
                    let arr = make_array(items);
                    unsafe { *regs.add(dst) = obj_into_val(arr, &mut self.heap) };
                }
                ROp::Hash => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let base = self.read_u16(ip + 3) as usize;
                    let count = self.read_u16(ip + 5) as usize;
                    ip += 7;

                    let mut hash = crate::object::HashObject::default();
                    let num_pairs = count / 2;
                    // Intern the spread marker symbol once before the loop
                    let rest_sym = crate::intern::intern("__fl_rest__");
                    for i in 0..num_pairs {
                        let key_val = unsafe { *regs.add(base + i * 2) };
                        let val = unsafe { *regs.add(base + i * 2 + 1) };
                        // Value-native key: avoids val_to_obj clone per key
                        let key = self.hash_key_from_value(key_val);
                        // Handle __fl_rest__ spread marker by symbol ID
                        if matches!(&key, crate::object::HashKey::Sym(s) if *s == rest_sym) {
                            if val.is_heap() {
                                let spread_obj = self.heap.get(val.heap_index());
                                if let Object::Hash(spread_hash) = spread_obj {
                                    let spread = spread_hash.borrow_mut();
                                    spread.sync_pairs_if_dirty();
                                    for (k, v) in spread.pairs.iter() {
                                        hash.insert_pair(k.clone(), *v);
                                    }
                                }
                            }
                            continue;
                        }
                        hash.insert_pair(key, val);
                    }
                    let result = make_hash(hash);
                    unsafe { *regs.add(dst) = obj_into_val(result, &mut self.heap) };
                }
                ROp::AppendElement => {
                    let arr_r = self.read_u16(ip + 1) as usize;
                    let val_r = self.read_u16(ip + 3) as usize;
                    let arr_val = unsafe { *regs.add(arr_r) };
                    let val_v = unsafe { *regs.add(val_r) };
                    if arr_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(arr_val.heap_index() as usize)
                        };
                        if let Object::Array(arr) = heap_obj {
                            let borrowed = arr.borrow_mut();
                            borrowed.push(val_v);
                            if borrowed.len() > MAX_ARRAY_SIZE {
                                return Err(VMError::TypeError(
                                    "Array size limit exceeded".to_string(),
                                ));
                            }
                        } else {
                            return Err(VMError::TypeError(
                                "append target must be array".to_string(),
                            ));
                        }
                    } else {
                        return Err(VMError::TypeError(
                            "append target must be array".to_string(),
                        ));
                    }
                    ip += 5;
                }
                ROp::AppendSpread => {
                    let arr_r = self.read_u16(ip + 1) as usize;
                    let iter_r = self.read_u16(ip + 3) as usize;
                    let arr_val = unsafe { *regs.add(arr_r) };
                    let spread_val = unsafe { *regs.add(iter_r) };
                    if !arr_val.is_heap() {
                        return Err(VMError::TypeError(
                            "spread target must be array".to_string(),
                        ));
                    }
                    // Peek target array directly from heap
                    let arr_heap = unsafe {
                        &*self
                            .heap
                            .objects
                            .as_ptr()
                            .add(arr_val.heap_index() as usize)
                    };
                    let Object::Array(arr) = arr_heap else {
                        return Err(VMError::TypeError(
                            "spread target must be array".to_string(),
                        ));
                    };
                    // Handle inline string spread: [...str]
                    if spread_val.is_inline_str() {
                        let (buf, len) = spread_val.inline_str_buf();
                        let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
                        let borrowed = arr.borrow_mut();
                        for ch in s.chars() {
                            borrowed.push(obj_into_val(
                                Object::String(ch.to_string().into()),
                                &mut self.heap,
                            ));
                        }
                        ip += 5;
                        continue;
                    }
                    if !spread_val.is_heap() {
                        return Err(VMError::TypeError(
                            "spread source is not iterable".to_string(),
                        ));
                    }
                    // Peek spread source from heap
                    let spread_heap = unsafe {
                        &*self
                            .heap
                            .objects
                            .as_ptr()
                            .add(spread_val.heap_index() as usize)
                    };
                    match spread_heap {
                        Object::Array(items) => {
                            let borrowed = arr.borrow_mut();
                            borrowed.extend(items.borrow().iter().copied());
                        }
                        Object::String(s) => {
                            let borrowed = arr.borrow_mut();
                            for ch in s.chars() {
                                borrowed.push(obj_into_val(
                                    Object::String(ch.to_string().into()),
                                    &mut self.heap,
                                ));
                            }
                        }
                        Object::Set(set_obj) => {
                            let borrowed = arr.borrow_mut();
                            for key in set_obj.entries.borrow().iter() {
                                borrowed.push(obj_into_val(
                                    self.object_from_hash_key(key),
                                    &mut self.heap,
                                ));
                            }
                        }
                        Object::Map(map_obj) => {
                            let borrowed = arr.borrow_mut();
                            for (k, v) in map_obj.entries.borrow().iter() {
                                borrowed.push(obj_into_val(
                                    make_array(vec![
                                        obj_into_val(self.object_from_hash_key(k), &mut self.heap),
                                        *v,
                                    ]),
                                    &mut self.heap,
                                ));
                            }
                        }
                        Object::Generator(gen_rc) => {
                            // Iterate generator by calling .next() until done
                            let gen_rc = gen_rc.clone();
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
                                        if done { break; }
                                        let value = hb.get_by_str("value")
                                            .unwrap_or(Value::UNDEFINED);
                                        arr.borrow_mut().push(value);
                                    }
                                    _ => break,
                                }
                            }
                        }
                        _ => {
                            return Err(VMError::TypeError(
                                "spread value is not iterable".to_string(),
                            ));
                        }
                    }
                    ip += 5;
                }

                // ── Property access ─────────────────────────────────────
                ROp::GetProp => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let obj_r = self.read_u16(ip + 3) as usize;
                    let prop_idx = self.read_u16(ip + 5) as usize;
                    let cache_slot = self.read_u16(ip + 7) as usize;
                    ip += 9;

                    // Fast path: inline cache hit on hash with primitive value.
                    let obj_val = unsafe { *regs.add(obj_r) };
                    debug_assert!(cache_slot < self.inline_cache.len());
                    if obj_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(obj_val.heap_index() as usize)
                        };

                        // Inline cache hit on Hash properties
                        // Skip fast path when accessors exist so getters are invoked.
                        let (cached_shape, cached_offset) =
                            unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                        if cached_shape != 0 {
                            if let Object::Hash(hash_rc) = heap_obj {
                                let hash = hash_rc.borrow_mut();
                                if cached_shape == hash.shape_version && !hash.has_accessors() {
                                    let slot = cached_offset as usize;
                                    debug_assert!(slot < hash.values.len());
                                    let val = unsafe { *hash.values.get_unchecked(slot) };
                                    unsafe { *regs.add(dst) = val };
                                    continue;
                                }
                            }
                        }

                        // .length fast path: u32 symbol compare instead of string match
                        let prop_sym = unsafe { *self.constants_syms_ptr.add(prop_idx) };
                        if prop_sym == self.sym_length {
                            match heap_obj {
                                Object::String(s) => {
                                    unsafe { *regs.add(dst) = Value::from_i64(s.len() as i64) };
                                    continue;
                                }
                                Object::Array(arr) => {
                                    unsafe {
                                        *regs.add(dst) = Value::from_i64(arr.borrow().len() as i64)
                                    };
                                    continue;
                                }
                                _ => {}
                            }
                        }

                        // Slow path — Value-native (no Object conversion for Hash)
                        self.ip = ip;
                        let result = self.get_property_val(obj_val, prop_sym, cache_slot)?;
                        unsafe { *regs.add(dst) = result };
                        continue;
                    }

                    // Non-heap: handle inline strings (e.g. "abc".length)
                    if obj_val.is_inline_str() {
                        let prop_sym = unsafe { *self.constants_syms_ptr.add(prop_idx) };
                        if prop_sym == self.sym_length {
                            unsafe {
                                *regs.add(dst) = Value::from_i32(obj_val.inline_str_len() as i32)
                            };
                            continue;
                        }
                        self.ip = ip;
                        let result = self.get_property_val(obj_val, prop_sym, cache_slot)?;
                        unsafe { *regs.add(dst) = result };
                        continue;
                    }
                    // Other non-heap slow path
                    let prop_sym = unsafe { *self.constants_syms_ptr.add(prop_idx) };
                    self.ip = ip;
                    let result = self.get_property_val(obj_val, prop_sym, cache_slot)?;
                    unsafe { *regs.add(dst) = result };
                }
                ROp::SetProp => {
                    let obj_r = self.read_u16(ip + 1) as usize;
                    let prop_idx = self.read_u16(ip + 3) as usize;
                    let src = self.read_u16(ip + 5) as usize;
                    let cache_slot = self.read_u16(ip + 7) as usize;
                    ip += 9;

                    // Fast path: inline cache hit on hash object.
                    // Skip fast path when accessors exist so setters are invoked.
                    let obj_val = unsafe { *regs.add(obj_r) };
                    debug_assert!(cache_slot < self.inline_cache.len());
                    if obj_val.is_heap() {
                        let (cached_shape, cached_offset) =
                            unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                        if cached_shape != 0 {
                            // SAFETY: heap index valid, no heap reallocation in this path
                            let heap_obj = unsafe {
                                &*self
                                    .heap
                                    .objects
                                    .as_ptr()
                                    .add(obj_val.heap_index() as usize)
                            };
                            if let Object::Hash(hash_rc) = heap_obj {
                                let hash = hash_rc.borrow_mut();
                                if hash.frozen {
                                    continue;
                                }
                                if cached_shape == hash.shape_version && !hash.has_accessors() {
                                    let slot = cached_offset as usize;
                                    debug_assert!(slot < hash.values.len());
                                    // Value-native: direct 8-byte write, no conversion
                                    let src_val = unsafe { *regs.add(src) };
                                    unsafe { *hash.values.get_unchecked_mut(slot) = src_val };
                                    if !hash.pairs_dirty {
                                        hash.pairs_dirty = true;
                                    }
                                    continue;
                                }
                            }
                        }
                    }

                    // Slow path — Value-native (no Object conversion for Hash)
                    let src_val = unsafe { *regs.add(src) };
                    let prop_sym = unsafe { *self.constants_syms_ptr.add(prop_idx) };

                    self.ip = ip;
                    if let Some(updated) =
                        self.set_property_val(obj_val, prop_sym, src_val, cache_slot)?
                    {
                        unsafe { *regs.add(obj_r) = updated };
                    }
                }
                ROp::GetGlobalProp => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let global_idx = self.read_u16(ip + 3) as usize;
                    let prop_idx = self.read_u16(ip + 5) as usize;
                    let cache_slot = self.read_u16(ip + 7) as usize;
                    ip += 9;

                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let prop_sym = unsafe { *self.constants_syms_ptr.add(prop_idx) };

                    self.ip = ip;
                    let result = self.get_property_val(gval, prop_sym, cache_slot)?;
                    unsafe { *regs.add(dst) = result };
                }
                ROp::SetGlobalProp => {
                    let global_idx = self.read_u16(ip + 1) as usize;
                    let prop_idx = self.read_u16(ip + 3) as usize;
                    let src = self.read_u16(ip + 5) as usize;
                    let cache_slot = self.read_u16(ip + 7) as usize;
                    ip += 9;

                    let gval = unsafe { self.globals.get_unchecked(global_idx) };
                    let src_val = unsafe { *regs.add(src) };
                    let prop_sym = unsafe { *self.constants_syms_ptr.add(prop_idx) };

                    self.ip = ip;
                    if let Some(updated) =
                        self.set_property_val(gval, prop_sym, src_val, cache_slot)?
                    {
                        unsafe { self.globals.set_unchecked(global_idx, updated) };
                    }
                }

                // ── Index access ────────────────────────────────────────
                ROp::Index => {
                    let (dst, obj_r, key_r) = self.read_3u16_operands(ip);
                    ip += 7;

                    let obj_val = unsafe { *regs.add(obj_r) };
                    let key_val = unsafe { *regs.add(key_r) };

                    // Fast path: array[i32] or hash[string] — direct access without val_to_obj
                    if obj_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(obj_val.heap_index() as usize)
                        };
                        if key_val.is_i32() {
                            if let Object::Array(arr_rc) = heap_obj {
                                let idx = unsafe { key_val.as_i32_unchecked() };
                                if idx >= 0 {
                                    let arr = arr_rc.borrow_mut();
                                    let i = idx as usize;
                                    if i < arr.len() {
                                        let val = unsafe { *arr.get_unchecked(i) };
                                        unsafe { *regs.add(dst) = val };
                                        continue;
                                    } else {
                                        unsafe { *regs.add(dst) = Value::UNDEFINED };
                                        continue;
                                    }
                                }
                            }
                        } else if key_val.is_heap() {
                            // Hash[string] fast path
                            if let Object::Hash(hash_rc) = heap_obj {
                                let key_heap = unsafe {
                                    &*self
                                        .heap
                                        .objects
                                        .as_ptr()
                                        .add(key_val.heap_index() as usize)
                                };
                                if let Object::String(s) = key_heap {
                                    let sym = crate::intern::intern(s);
                                    let val = hash_rc
                                        .borrow()
                                        .get_by_sym(sym)
                                        .unwrap_or(Value::UNDEFINED);
                                    let result = self.maybe_bind_method_val(val, obj_val)?;
                                    unsafe { *regs.add(dst) = result };
                                    continue;
                                }
                            }
                        } else if key_val.is_inline_str() {
                            // Hash[inline_string] fast path
                            if let Object::Hash(hash_rc) = heap_obj {
                                let (buf, len) = key_val.inline_str_buf();
                                let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
                                let sym = crate::intern::intern(s);
                                let val =
                                    hash_rc.borrow().get_by_sym(sym).unwrap_or(Value::UNDEFINED);
                                let result = self.maybe_bind_method_val(val, obj_val)?;
                                unsafe { *regs.add(dst) = result };
                                continue;
                            }
                        }
                    }

                    // Slow path — pop_val avoids Value→Object→Value roundtrip
                    let obj = val_to_obj(obj_val, &self.heap);
                    let key = val_to_obj(key_val, &self.heap);
                    self.ip = ip;
                    self.execute_index_expression(obj, key)?;
                    unsafe { *regs.add(dst) = self.pop_val()? };
                }
                ROp::SetIndex => {
                    let (obj_r, key_r, val_r) = self.read_3u16_operands(ip);
                    ip += 7;

                    let obj_val = unsafe { *regs.add(obj_r) };
                    let key_val = unsafe { *regs.add(key_r) };

                    // Fast path: array[i32] or hash[string] = val — direct write
                    if obj_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(obj_val.heap_index() as usize)
                        };
                        if key_val.is_i32() {
                            if let Object::Array(arr_rc) = heap_obj {
                                let idx = unsafe { key_val.as_i32_unchecked() };
                                if idx >= 0 {
                                    let i = idx as usize;
                                    let val_v = unsafe { *regs.add(val_r) };
                                    let arr = arr_rc.borrow_mut();
                                    if i < arr.len() {
                                        unsafe { *arr.get_unchecked_mut(i) = val_v };
                                    } else {
                                        // Extend array to fit index
                                        if i > MAX_ARRAY_SIZE {
                                            return Err(VMError::TypeError(
                                                "Array size limit exceeded".to_string(),
                                            ));
                                        }
                                        arr.resize(i + 1, Value::UNDEFINED);
                                        unsafe { *arr.get_unchecked_mut(i) = val_v };
                                    }
                                    // Array is Rc<VmCell>, mutation is in-place.
                                    continue;
                                }
                            }
                        } else if key_val.is_heap() {
                            // Hash[string] = val fast path
                            if let Object::Hash(hash_rc) = heap_obj {
                                let key_heap = unsafe {
                                    &*self
                                        .heap
                                        .objects
                                        .as_ptr()
                                        .add(key_val.heap_index() as usize)
                                };
                                if let Object::String(s) = key_heap {
                                    let sym = crate::intern::intern(s);
                                    let val_v = unsafe { *regs.add(val_r) };
                                    hash_rc.borrow_mut().set_by_sym(sym, val_v);
                                    continue;
                                }
                            }
                        } else if key_val.is_inline_str() {
                            // Hash[inline_string] = val fast path
                            if let Object::Hash(hash_rc) = heap_obj {
                                let (buf, len) = key_val.inline_str_buf();
                                let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
                                let sym = crate::intern::intern(s);
                                let val_v = unsafe { *regs.add(val_r) };
                                hash_rc.borrow_mut().set_by_sym(sym, val_v);
                                continue;
                            }
                        }
                    }

                    // Instance fast path: mutate fields in-place on the heap
                    // (Instance is Box<InstanceObject>, not Rc, so we must
                    //  write directly to avoid clone-and-lose semantics.)
                    if obj_val.is_heap() {
                        let heap_idx = obj_val.heap_index() as usize;
                        let is_instance = matches!(
                            self.heap.objects.get(heap_idx),
                            Some(Object::Instance(_))
                        );
                        if is_instance {
                            // Extract the string key
                            let key_str: Option<String> = if key_val.is_heap() {
                                let key_obj = unsafe {
                                    &*self.heap.objects.as_ptr().add(key_val.heap_index() as usize)
                                };
                                if let Object::String(s) = key_obj {
                                    Some(s.to_string())
                                } else {
                                    None
                                }
                            } else if key_val.is_inline_str() {
                                let (buf, len) = key_val.inline_str_buf();
                                let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
                                Some(s.to_string())
                            } else {
                                None
                            };
                            if let Some(prop_name) = key_str {
                                let val_v = unsafe { *regs.add(val_r) };
                                let val_obj = val_to_obj(val_v, &self.heap);
                                if let Object::Instance(inst) = &mut self.heap.objects[heap_idx] {
                                    inst.fields.insert(prop_name, val_obj);
                                }
                                continue;
                            }
                        }
                    }

                    // Slow path — pop_val avoids Value→Object→Value roundtrip
                    let obj = val_to_obj(obj_val, &self.heap);
                    let key = val_to_obj(key_val, &self.heap);
                    let val = val_to_obj(unsafe { *regs.add(val_r) }, &self.heap);
                    self.ip = ip;
                    self.execute_set_index(obj, key, val)?;
                    unsafe { *regs.add(obj_r) = self.pop_val()? };
                }
                ROp::DeleteProp => {
                    let (dst, obj_r, key_r) = self.read_3u16_operands(ip);
                    ip += 7;

                    let obj_val = unsafe { *regs.add(obj_r) };
                    let key_val = unsafe { *regs.add(key_r) };

                    // Fast path: Hash deletion without val_to_obj
                    if obj_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(obj_val.heap_index() as usize)
                        };
                        if let Object::Hash(hash_rc) = heap_obj {
                            let k = self.hash_key_from_value(key_val);
                            hash_rc.borrow_mut().remove_pair(&k);
                            // Hash is Rc, mutated in-place — no store-back needed
                            unsafe { *regs.add(dst) = Value::TRUE };
                            continue;
                        }
                    }

                    // Slow path for non-Hash types — pop_val avoids roundtrip
                    let obj = val_to_obj(obj_val, &self.heap);
                    let key = val_to_obj(key_val, &self.heap);
                    self.ip = ip;
                    self.execute_delete_property(obj, key)?;
                    unsafe { *regs.add(obj_r) = self.pop_val()? };
                    unsafe { *regs.add(dst) = Value::TRUE };
                }

                // ── Iterator / destructuring ────────────────────────────
                ROp::IteratorRest => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let iter_r = self.read_u16(ip + 3) as usize;
                    let skip = self.read_u16(ip + 5) as usize;
                    ip += 7;

                    let iter_val = unsafe { *regs.add(iter_r) };
                    let rest: Vec<Value> = if iter_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(iter_val.heap_index() as usize)
                        };
                        if let Object::Array(arr_rc) = heap_obj {
                            let items = arr_rc.borrow();
                            if skip < items.len() {
                                items[skip..].to_vec()
                            } else {
                                vec![]
                            }
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    };
                    let result = make_array(rest);
                    unsafe { *regs.add(dst) = obj_into_val(result, &mut self.heap) };
                }
                ROp::GetKeysIter => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let obj_r = self.read_u16(ip + 3) as usize;
                    ip += 5;

                    let obj_val = unsafe { *regs.add(obj_r) };
                    // get_keys_array takes Object; peek heap to avoid clone for Hash
                    let keys = if obj_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(obj_val.heap_index() as usize)
                        };
                        if let Object::Hash(hash_rc) = heap_obj {
                            let hash_b = hash_rc.borrow();
                            let ordered = self.ordered_hash_keys_js(&hash_b);
                            let mut out = Vec::with_capacity(ordered.len());
                            for key in ordered {
                                out.push(obj_into_val(
                                    self.object_from_hash_key(&key),
                                    &mut self.heap,
                                ));
                            }
                            out
                        } else {
                            self.get_keys_array(val_to_obj(obj_val, &self.heap))
                        }
                    } else {
                        vec![]
                    };
                    let result = make_array(keys);
                    unsafe { *regs.add(dst) = obj_into_val(result, &mut self.heap) };
                }
                ROp::ObjectRest => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let obj_r = self.read_u16(ip + 3) as usize;
                    let keys_base = self.read_u16(ip + 5) as usize;
                    let count = self.read_u16(ip + 7) as usize;
                    ip += 9;

                    // Collect excluded keys — Value-native, no val_to_obj
                    let mut excluded = rustc_hash::FxHashSet::default();
                    excluded.reserve(count);
                    for i in 0..count {
                        let key_val = unsafe { *regs.add(keys_base + i) };
                        excluded.insert(self.hash_key_from_value(key_val));
                    }

                    // Peek heap for source hash — no val_to_obj
                    let source_val = unsafe { *regs.add(obj_r) };
                    let mut out = crate::object::HashObject::default();
                    if source_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(source_val.heap_index() as usize)
                        };
                        if let Object::Hash(h) = heap_obj {
                            let h = h.borrow_mut();
                            h.sync_pairs_if_dirty();
                            for k in h.ordered_keys_ref() {
                                if !excluded.contains(&k) {
                                    let v = *h.pairs.get(&k).expect("hash key_order out of sync");
                                    out.insert_pair(k.clone(), v);
                                }
                            }
                        }
                    }

                    unsafe { *regs.add(dst) = obj_into_val(make_hash(out), &mut self.heap) };
                }

                // ── Async ───────────────────────────────────────────────
                ROp::Await => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let src = self.read_u16(ip + 3) as usize;
                    let val = unsafe { *regs.add(src) };
                    // Heap-peek: only inspect Promise objects, pass everything else through
                    let result = if val.is_heap() {
                        let heap_obj = self.heap.get(val.heap_index());
                        if let Object::Promise(p) = heap_obj {
                            match &p.settled {
                                PromiseState::Fulfilled(v) => {
                                    obj_into_val(v.as_ref().clone(), &mut self.heap)
                                }
                                PromiseState::Rejected(v) => {
                                    return Err(VMError::TypeError(format!(
                                        "Await rejected: {}",
                                        v.inspect()
                                    )));
                                }
                            }
                        } else {
                            val // non-Promise heap object — pass through as-is
                        }
                    } else {
                        val // inline value (i32/f64/bool/null/undefined) — zero work
                    };
                    unsafe { *regs.add(dst) = result };
                    ip += 5;
                }

                // ── Error ───────────────────────────────────────────────
                ROp::Throw => {
                    let src = self.read_u16(ip + 1) as usize;
                    let val = unsafe { *regs.add(src) };
                    return Err(VMError::TypeError(format!(
                        "Uncaught throw: {}",
                        val_inspect(val, &self.heap)
                    )));
                }

                // ── Halt ────────────────────────────────────────────────
                // ── Fused opcodes ───────────────────────────────────────
                ROp::AddRegConst => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let src = self.read_u16(ip + 3) as usize;
                    let const_idx = self.read_u16(ip + 5) as usize;
                    let lv = unsafe { *regs.add(src) };
                    let cv = unsafe { *self.constants_values_ptr.add(const_idx) };
                    if Value::both_i32(lv, cv) {
                        let a = unsafe { lv.as_i32_unchecked() };
                        let b = unsafe { cv.as_i32_unchecked() };
                        unsafe {
                            *regs.add(dst) = match a.checked_add(b) {
                                Some(sum) => Value::from_i32(sum),
                                None => Value::from_f64(a as f64 + b as f64),
                            }
                        };
                    } else if lv.is_number() && cv.is_number() {
                        unsafe {
                            *regs.add(dst) = Value::from_f64(lv.to_number() + cv.to_number())
                        };
                    } else {
                        unsafe { *regs.add(dst) = self.fused_add_const_slow(lv, const_idx)? };
                    }
                    ip += 7;
                }
                ROp::TestLtConstJump => {
                    let r = self.read_u16(ip + 1) as usize;
                    let const_idx = self.read_u16(ip + 3) as usize;
                    let target = self.read_u16(ip + 5) as usize;
                    let lv = unsafe { *regs.add(r) };
                    let cv = unsafe { *self.constants_values_ptr.add(const_idx) };
                    let passes = if Value::both_i32(lv, cv) {
                        unsafe { lv.as_i32_unchecked() < cv.as_i32_unchecked() }
                    } else if lv.is_number() && cv.is_number() {
                        lv.to_number() < cv.to_number()
                    } else {
                        self.fused_test_lt_slow(lv, const_idx)?
                    };
                    if passes {
                        ip += 7;
                    } else {
                        ip = target;
                    }
                }
                ROp::TestLeConstJump => {
                    let r = self.read_u16(ip + 1) as usize;
                    let const_idx = self.read_u16(ip + 3) as usize;
                    let target = self.read_u16(ip + 5) as usize;
                    let lv = unsafe { *regs.add(r) };
                    let cv = unsafe { *self.constants_values_ptr.add(const_idx) };
                    let passes = if Value::both_i32(lv, cv) {
                        unsafe { lv.as_i32_unchecked() <= cv.as_i32_unchecked() }
                    } else if lv.is_number() && cv.is_number() {
                        lv.to_number() <= cv.to_number()
                    } else {
                        self.fused_test_le_slow(lv, const_idx)?
                    };
                    if passes {
                        ip += 7;
                    } else {
                        ip = target;
                    }
                }
                ROp::IncrementRegAndJump => {
                    let r = self.read_u16(ip + 1) as usize;
                    let const_idx = self.read_u16(ip + 3) as usize;
                    let target = self.read_u16(ip + 5) as usize;
                    let lv = unsafe { *regs.add(r) };
                    let cv = unsafe { *self.constants_values_ptr.add(const_idx) };
                    if Value::both_i32(lv, cv) {
                        let a = unsafe { lv.as_i32_unchecked() };
                        let b = unsafe { cv.as_i32_unchecked() };
                        unsafe {
                            *regs.add(r) = match a.checked_add(b) {
                                Some(sum) => Value::from_i32(sum),
                                None => Value::from_f64(a as f64 + b as f64),
                            }
                        };
                    } else if lv.is_number() && cv.is_number() {
                        unsafe { *regs.add(r) = Value::from_f64(lv.to_number() + cv.to_number()) };
                    } else {
                        unsafe { *regs.add(r) = self.fused_add_const_slow(lv, const_idx)? };
                    }
                    if self.enforce_limits {
                        self.check_execution_limits()?;
                    }
                    ip = target;

                    // ── Opcode threading: inline the condition test at loop start ──
                    // After backward jump, the next opcode is almost always
                    // TestLe/LtConstJump. Inlining it saves one match dispatch per
                    // loop iteration.
                    let next_op = unsafe { *self.inst_ptr.add(ip) };
                    if next_op == ROp::TestLeConstJump as u8
                        || next_op == ROp::TestLtConstJump as u8
                    {
                        let cmp_r = self.read_u16(ip + 1) as usize;
                        let cmp_const = self.read_u16(ip + 3) as usize;
                        let cmp_target = self.read_u16(ip + 5) as usize;
                        let cmp_lv = unsafe { *regs.add(cmp_r) };
                        let cmp_cv = unsafe { *self.constants_values_ptr.add(cmp_const) };
                        if Value::both_i32(cmp_lv, cmp_cv) {
                            let a = unsafe { cmp_lv.as_i32_unchecked() };
                            let b = unsafe { cmp_cv.as_i32_unchecked() };
                            let passes = if next_op == ROp::TestLeConstJump as u8 {
                                a <= b
                            } else {
                                a < b
                            };
                            if passes {
                                ip += 7;
                            } else {
                                ip = cmp_target;
                            }
                            continue;
                        }
                        // Non-i32: fall through to normal dispatch at ip
                    }
                }
                ROp::ModRegConstStrictEqConstJump => {
                    let r = self.read_u16(ip + 1) as usize;
                    let mod_const_idx = self.read_u16(ip + 3) as usize;
                    let cmp_const_idx = self.read_u16(ip + 5) as usize;
                    let target = self.read_u16(ip + 7) as usize;
                    let lv = unsafe { *regs.add(r) };
                    let mod_cv = unsafe { *self.constants_values_ptr.add(mod_const_idx) };
                    let cmp_cv = unsafe { *self.constants_values_ptr.add(cmp_const_idx) };
                    let passes = if lv.is_i32() && mod_cv.is_i32() && cmp_cv.is_i32() {
                        let a = unsafe { lv.as_i32_unchecked() };
                        let b = unsafe { mod_cv.as_i32_unchecked() };
                        let c = unsafe { cmp_cv.as_i32_unchecked() };
                        b != 0 && (a % b) == c
                    } else {
                        self.fused_mod_strict_eq_slow(lv, mod_const_idx, cmp_const_idx)?
                    };
                    if passes {
                        ip += 9;
                    } else {
                        ip = target;
                    }
                }
                ROp::AddConstToRegProp => {
                    let obj_r = self.read_u16(ip + 1) as usize;
                    let prop_const_idx = self.read_u16(ip + 3) as usize;
                    let val_const_idx = self.read_u16(ip + 5) as usize;
                    let cache_slot = self.read_u16(ip + 7) as usize;

                    let obj_val = unsafe { *regs.add(obj_r) };
                    if obj_val.is_heap() {
                        // SAFETY: heap index is valid; we only read the hash through
                        // its VmCell and don't reallocate the heap objects Vec.
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(obj_val.heap_index() as usize)
                        };
                        if let Object::Hash(hash_rc) = heap_obj {
                            let hash = hash_rc.borrow_mut();
                            debug_assert!(cache_slot < self.inline_cache.len());
                            let (cached_shape, cached_offset) =
                                unsafe { *self.inline_cache.get_unchecked(cache_slot) };
                            if cached_shape == hash.shape_version {
                                let slot = cached_offset as usize;
                                debug_assert!(slot < hash.values.len());
                                let prop_val = unsafe { *hash.values.get_unchecked(slot) };
                                let add_cv =
                                    unsafe { *self.constants_values_ptr.add(val_const_idx) };
                                // Value-native arithmetic on hash slot
                                let result = if Value::both_i32(prop_val, add_cv) {
                                    let a = unsafe { prop_val.as_i32_unchecked() };
                                    let b = unsafe { add_cv.as_i32_unchecked() };
                                    match a.checked_add(b) {
                                        Some(sum) => Value::from_i32(sum),
                                        None => Value::from_f64(a as f64 + b as f64),
                                    }
                                } else if prop_val.is_number() && add_cv.is_number() {
                                    Value::from_f64(prop_val.to_number() + add_cv.to_number())
                                } else {
                                    let lo = val_as_obj_ref(prop_val, &self.heap);
                                    let ro = val_as_obj_ref(add_cv, &self.heap);
                                    obj_into_val(self.add_objects(&lo, &ro)?, &mut self.heap)
                                };
                                unsafe { *hash.values.get_unchecked_mut(slot) = result };
                                hash.pairs_dirty = true;
                                ip += 9;
                                continue;
                            }
                        }
                    }

                    // Cache miss: cold slow path
                    self.ip = ip;
                    self.fused_add_const_to_prop_slow(
                        obj_val,
                        obj_r,
                        prop_const_idx,
                        val_const_idx,
                        cache_slot,
                        regs,
                    )?;
                    ip += 9;
                }
                ROp::AddRegPropsToRegProp => {
                    let obj_r = self.read_u16(ip + 1) as usize;
                    let s1_cache = self.read_u16(ip + 5) as usize;
                    let s2_cache = self.read_u16(ip + 9) as usize;
                    let dst_cache = self.read_u16(ip + 13) as usize;

                    let obj_val = unsafe { *regs.add(obj_r) };
                    if obj_val.is_heap() {
                        let heap_obj = unsafe {
                            &*self
                                .heap
                                .objects
                                .as_ptr()
                                .add(obj_val.heap_index() as usize)
                        };
                        if let Object::Hash(hash_rc) = heap_obj {
                            let hash = hash_rc.borrow_mut();
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
                                let val1 = unsafe { *hash.values.get_unchecked(s1) };
                                let val2 = unsafe { *hash.values.get_unchecked(s2) };
                                // Value-native arithmetic
                                let result = if Value::both_i32(val1, val2) {
                                    let a = unsafe { val1.as_i32_unchecked() };
                                    let b = unsafe { val2.as_i32_unchecked() };
                                    match a.checked_add(b) {
                                        Some(sum) => Value::from_i32(sum),
                                        None => Value::from_f64(a as f64 + b as f64),
                                    }
                                } else if val1.is_number() && val2.is_number() {
                                    Value::from_f64(val1.to_number() + val2.to_number())
                                } else {
                                    let lo = val_as_obj_ref(val1, &self.heap);
                                    let ro = val_as_obj_ref(val2, &self.heap);
                                    obj_into_val(self.add_objects(&lo, &ro)?, &mut self.heap)
                                };
                                unsafe { *hash.values.get_unchecked_mut(d) = result };
                                hash.pairs_dirty = true;
                                ip += 15;
                                continue;
                            }
                        }
                    }

                    // Cache miss: cold slow path
                    let s1_prop_idx = self.read_u16(ip + 3) as usize;
                    let s2_prop_idx = self.read_u16(ip + 7) as usize;
                    let dst_prop_idx = self.read_u16(ip + 11) as usize;
                    self.ip = ip;
                    self.fused_add_reg_props_slow(
                        obj_val,
                        obj_r,
                        s1_prop_idx,
                        s2_prop_idx,
                        dst_prop_idx,
                        s1_cache,
                        s2_cache,
                        dst_cache,
                        regs,
                    )?;
                    ip += 15;
                }

                ROp::DefineAccessor => {
                    // Operands: [hash_r: u16, func_r: u16, prop_const_idx: u16, kind: u8]
                    let hash_r = self.read_u16(ip + 1) as usize;
                    let func_r = self.read_u16(ip + 3) as usize;
                    let prop_idx = self.read_u16(ip + 5) as usize;
                    let kind = self.read_u8_at(ip + 7);

                    let func_val = unsafe { *regs.add(func_r) };
                    let hash_val = unsafe { *regs.add(hash_r) };

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
                    ip += 8;
                }

                ROp::InitClass => {
                    // Operands: [dst:2] — class register to init in-place
                    let dst = self.read_u16(ip + 1) as usize;
                    let class_val = unsafe { *regs.add(dst) };
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
                            // Write updated class back to register and heap
                            let updated_val =
                                obj_into_val(Object::Class(class_box), &mut self.heap);
                            unsafe { *regs.add(dst) = updated_val };
                            // Also need to refresh regs pointer since
                            // execute_compiled_function_slice may have extended the stack
                            regs = unsafe { self.stack.as_mut_ptr().add(reg_base) };
                        }
                        other => {
                            return Err(VMError::TypeError(format!(
                                "InitClass: expected class, got {:?}",
                                other.object_type()
                            )));
                        }
                    }
                    ip += 3;
                }

                ROp::NewTarget => {
                    // Operands: [dst:2]
                    let dst = self.read_u16(ip + 1) as usize;
                    unsafe { *regs.add(dst) = self.new_target };
                    ip += 3;
                }
                ROp::ImportMeta => {
                    // Operands: [dst:2]
                    let dst = self.read_u16(ip + 1) as usize;
                    let empty_hash = Object::Hash(Rc::new(crate::object::VmCell::new(
                        crate::object::HashObject::with_capacity(0),
                    )));
                    unsafe { *regs.add(dst) = obj_into_val(empty_hash, &mut self.heap) };
                    ip += 3;
                }

                ROp::Yield => {
                    // Operands: [dst:2, src:2]
                    // dst = register where the resume value will go (on next .next() call)
                    // src = register containing the yielded value
                    let _dst = self.read_u16(ip + 1) as usize;
                    let src = self.read_u16(ip + 3) as usize;
                    let yielded = unsafe { *regs.add(src) };
                    ip += 5;
                    // Save ip AFTER the yield instruction so resume continues past it.
                    // The dst register index can be recovered from instruction bytes at
                    // saved_ip - 2 when resuming.
                    self.ip = ip;
                    return Err(VMError::Yield(yielded));
                }

                ROp::MakeClosure => {
                    let dst = self.read_u16(ip + 1) as usize;
                    let const_idx = self.read_u16(ip + 3) as usize;
                    let count = self.read_u8_at(ip + 5) as usize;
                    ip += 6; // past fixed part

                    // Read the function from constants and clone it
                    let func_obj = unsafe { &*self.constants_raw };
                    let mut func = match &func_obj[const_idx] {
                        Object::CompiledFunction(f) => (**f).clone(),
                        _ => {
                            return Err(VMError::TypeError(
                                "MakeClosure: not a function".to_string(),
                            ))
                        }
                    };

                    // Snapshot captured global slot values
                    let mut captured = Vec::with_capacity(count);
                    for _ in 0..count {
                        let slot = self.read_u16(ip) as usize;
                        ip += 2;
                        let val = self.get_global_as_value(slot);
                        captured.push((slot as u16, val));
                    }
                    func.captured_values = captured;

                    // Allocate on heap and store in register
                    let val = obj_into_val(
                        Object::CompiledFunction(Box::new(func)),
                        &mut self.heap,
                    );
                    unsafe { *regs.add(dst) = val };
                }

                ROp::Halt => {
                    if self.trace_enabled {
                        self.trace_steps.push((self.trace_clk, trace_ip as u64, 76, 0, 0, 0, 0, 0));
                        self.trace_clk += 1;
                    }
                    return Ok(());
                }
                ROp::HaltValue => {
                    let src = self.read_u16(ip + 1) as usize;
                    let val = unsafe { *regs.add(src) };
                    self.last_popped = Some(val);
                    if self.trace_enabled {
                        let vd = if val.is_i32() { (unsafe { val.as_i32_unchecked() }) as u64 } else { val.bits() };
                        self.trace_steps.push((self.trace_clk, trace_ip as u64, 77, 0, 0, vd, 0, 0));
                        self.trace_clk += 1;
                    }
                    return Ok(());
                }
            }

            // Post-execution ZK trace capture — records the state AFTER the instruction ran
            if self.trace_enabled {
                let (va, vb, vd, cv, ax) = match trace_op {
                    // Binary arithmetic: store SEMANTIC numeric values (not NaN-boxed bits)
                    // so field element arithmetic in the STARK constraint matches.
                    // Excludes 12 (MOD) and 13 (POW) which have specialized handling below.
                    8..=11 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let left_r = self.read_u16(trace_ip + 3) as usize;
                        let right_r = self.read_u16(trace_ip + 5) as usize;
                        unsafe {
                            let lv = *regs.add(left_r);
                            let rv = *regs.add(right_r);
                            let dv = *regs.add(dst_r);
                            // Extract semantic numeric values for ZK constraints
                            let la = if lv.is_i32() { lv.as_i32_unchecked() as u64 } else { lv.as_f64().to_bits() };
                            let ra = if rv.is_i32() { rv.as_i32_unchecked() as u64 } else { rv.as_f64().to_bits() };
                            let da = if dv.is_i32() { dv.as_i32_unchecked() as u64 } else { dv.as_f64().to_bits() };
                            (la, ra, da, 0u64, 0u64)
                        }
                    }
                    // Comparisons: result is 1 (true) or 0 (false) in AUX
                    14..=21 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let left_r = self.read_u16(trace_ip + 3) as usize;
                        let right_r = self.read_u16(trace_ip + 5) as usize;
                        let dst_val = unsafe { (*regs.add(dst_r)).bits() };
                        let cmp_bool = if dst_val == Value::TRUE.bits() { 1u64 } else { 0u64 };
                        unsafe {
                            let lv = *regs.add(left_r);
                            let rv = *regs.add(right_r);
                            let la = if lv.is_i32() { lv.as_i32_unchecked() as u64 } else { lv.bits() };
                            let ra = if rv.is_i32() { rv.as_i32_unchecked() as u64 } else { rv.bits() };
                            (la, ra, dst_val, 0u64, cmp_bool)
                        }
                    }
                    // LoadConst: dst = const (semantic value for integers)
                    0 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let dv = unsafe { *regs.add(dst_r) };
                        let da = if dv.is_i32() { unsafe { dv.as_i32_unchecked() as u64 } } else { dv.bits() };
                        (0, 0, da, da, 0)
                    }
                    // LoadTrue/False/Null/Undef: dst = literal (use NaN-boxed since constraints match)
                    1..=4 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let dv = unsafe { *regs.add(dst_r) };
                        let da = if dv.is_i32() { unsafe { dv.as_i32_unchecked() as u64 } } else { dv.bits() };
                        (0, 0, da, da, 0)
                    }
                    // Move: dst = src (semantic)
                    5 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let src_r = self.read_u16(trace_ip + 3) as usize;
                        let dv = unsafe { *regs.add(dst_r) };
                        let sv = unsafe { *regs.add(src_r) };
                        let da = if dv.is_i32() { unsafe { dv.as_i32_unchecked() as u64 } } else { dv.bits() };
                        let sa = if sv.is_i32() { unsafe { sv.as_i32_unchecked() as u64 } } else { sv.bits() };
                        (sa, 0, da, 0, 0)
                    }
                    // Jump
                    35 => {
                        (0, 0, 0, 0, ip as u64) // aux = actual jump target (new ip)
                    }
                    // JumpIfNot, JumpIfTruthy
                    36 | 37 => {
                        let cond_r = self.read_u16(trace_ip + 1) as usize;
                        let cv = unsafe { (*regs.add(cond_r)).bits() };
                        (cv, 0, 0, 0, ip as u64)
                    }
                    // Unary ops: NEG (30), NOT (31)
                    30 | 31 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let src_r = self.read_u16(trace_ip + 3) as usize;
                        let dv = unsafe { *regs.add(dst_r) };
                        let sv = unsafe { *regs.add(src_r) };
                        let da = if dv.is_i32() { unsafe { dv.as_i32_unchecked() as u64 } } else { dv.bits() };
                        let sa = if sv.is_i32() { unsafe { sv.as_i32_unchecked() as u64 } } else { sv.bits() };
                        // For NOT: dst is boolean (0 or 1)
                        let da = if trace_op == 31 {
                            if dv == Value::TRUE { 1u64 } else { 0u64 }
                        } else { da };
                        (sa, 0, da, 0, 0)
                    }
                    // GetGlobal (6): dst = loaded value, const = global index
                    6 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let idx = self.read_u16(trace_ip + 3) as u64;
                        let dv = unsafe { *regs.add(dst_r) };
                        let da = if dv.is_i32() { unsafe { dv.as_i32_unchecked() as u64 } } else { dv.bits() };
                        (da, 0, da, idx, 0) // val_a = val_dst = loaded value, const = index
                    }
                    // SetGlobal (7): val_a = stored value, const = global index
                    7 => {
                        let idx = self.read_u16(trace_ip + 1) as u64;
                        let src_r = self.read_u16(trace_ip + 3) as usize;
                        let sv = unsafe { *regs.add(src_r) };
                        let sa = if sv.is_i32() { unsafe { sv.as_i32_unchecked() as u64 } } else { sv.bits() };
                        (sa, 0, 0, idx, 0)
                    }
                    // MOD (12): a % b = dst, quotient in AUX
                    12 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let left_r = self.read_u16(trace_ip + 3) as usize;
                        let right_r = self.read_u16(trace_ip + 5) as usize;
                        unsafe {
                            let lv = *regs.add(left_r);
                            let rv = *regs.add(right_r);
                            let dv = *regs.add(dst_r);
                            let la = if lv.is_i32() { lv.as_i32_unchecked() as u64 } else { lv.bits() };
                            let ra = if rv.is_i32() { rv.as_i32_unchecked() as u64 } else { rv.bits() };
                            let da = if dv.is_i32() { dv.as_i32_unchecked() as u64 } else { dv.bits() };
                            // Compute quotient for the constraint: a = b * quotient + dst
                            let quotient = if ra != 0 { la / ra } else { 0 };
                            (la, ra, da, 0, quotient)
                        }
                    }
                    // Call (38), CallGlobal (62): capture func ref + nargs
                    38 | 62 => {
                        let nargs = self.read_u8_at(trace_ip + 5) as u64;
                        (0, nargs, 0, 0, 0)
                    }
                    // Return (41): capture return value
                    41 => {
                        let src_r = self.read_u16(trace_ip + 1) as usize;
                        let rv = unsafe { *regs.add(src_r) };
                        let ra = if rv.is_i32() { (unsafe { rv.as_i32_unchecked() }) as u64 } else { rv.bits() };
                        (0, 0, ra, 0, 0)
                    }
                    // ReturnUndef (42)
                    42 => (0, 0, 0, 0, 0),
                    // POW (13): binary op like MUL
                    13 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let left_r = self.read_u16(trace_ip + 3) as usize;
                        let right_r = self.read_u16(trace_ip + 5) as usize;
                        unsafe {
                            let lv = *regs.add(left_r);
                            let rv = *regs.add(right_r);
                            let dv = *regs.add(dst_r);
                            let la = if lv.is_i32() { lv.as_i32_unchecked() as u64 } else { lv.bits() };
                            let ra = if rv.is_i32() { rv.as_i32_unchecked() as u64 } else { rv.bits() };
                            let da = if dv.is_i32() { dv.as_i32_unchecked() as u64 } else { dv.bits() };
                            (la, ra, da, 0, 0)
                        }
                    }
                    // Bitwise ops (24-29): capture operands + result
                    24..=29 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let left_r = self.read_u16(trace_ip + 3) as usize;
                        let right_r = self.read_u16(trace_ip + 5) as usize;
                        unsafe {
                            let lv = *regs.add(left_r);
                            let rv = *regs.add(right_r);
                            let dv = *regs.add(dst_r);
                            let la = if lv.is_i32() { lv.as_i32_unchecked() as u64 } else { lv.bits() };
                            let ra = if rv.is_i32() { rv.as_i32_unchecked() as u64 } else { rv.bits() };
                            let da = if dv.is_i32() { dv.as_i32_unchecked() as u64 } else { dv.bits() };
                            (la, ra, da, 0, 0)
                        }
                    }
                    // Typeof (33): result in dst
                    33 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let src_r = self.read_u16(trace_ip + 3) as usize;
                        unsafe {
                            ((*regs.add(src_r)).bits(), 0, (*regs.add(dst_r)).bits(), 0, 0)
                        }
                    }
                    // GetProp (50): capture object, result
                    50 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let obj_r = self.read_u16(trace_ip + 3) as usize;
                        unsafe {
                            ((*regs.add(obj_r)).bits(), 0, (*regs.add(dst_r)).bits(), 0, 0)
                        }
                    }
                    // SetProp (51): capture object, value
                    51 => {
                        let obj_r = self.read_u16(trace_ip + 1) as usize;
                        let src_r = self.read_u16(trace_ip + 5) as usize;
                        unsafe {
                            ((*regs.add(obj_r)).bits(), (*regs.add(src_r)).bits(), 0, 0, 0)
                        }
                    }
                    // Index read (54): obj[key] -> dst
                    54 => {
                        let dst_r = self.read_u16(trace_ip + 1) as usize;
                        let obj_r = self.read_u16(trace_ip + 3) as usize;
                        let key_r = self.read_u16(trace_ip + 5) as usize;
                        unsafe {
                            ((*regs.add(obj_r)).bits(), (*regs.add(key_r)).bits(),
                             (*regs.add(dst_r)).bits(), 0, 0)
                        }
                    }
                    // Array (46): capture count
                    46 => {
                        let count = self.read_u16(trace_ip + 5) as u64;
                        (0, count, 0, 0, 0)
                    }
                    // Hash (47): capture count
                    47 => {
                        let count = self.read_u16(trace_ip + 5) as u64;
                        (0, count, 0, 0, 0)
                    }
                    // Fused: TestLtConstJump (64), TestLeConstJump (65)
                    64 | 65 => {
                        let r = self.read_u16(trace_ip + 1) as usize;
                        let rv = unsafe { *regs.add(r) };
                        let ra = if rv.is_i32() { (unsafe { rv.as_i32_unchecked() }) as u64 } else { rv.bits() };
                        (ra, 0, 0, 0, ip as u64)
                    }
                    // All other opcodes
                    _ => (0, 0, 0, 0, 0),
                };
                self.trace_steps.push((self.trace_clk, trace_ip as u64, trace_op, va, vb, vd, cv, ax));
                self.trace_clk += 1;
            }
        }
    }

    // ── Inline helpers ────────────────────────────────────────────────

    #[inline(always)]
    fn get_global_as_value(&self, idx: usize) -> Value {
        unsafe { self.globals.get_unchecked(idx) }
    }

    // ── Cold slow-path helpers ────────────────────────────────────────
    // These are marked #[cold] / #[inline(never)] to keep the dispatch
    // loop compact and improve icache utilization. The hot i32/f64 fast
    // paths stay inline in the match arms.

    #[cold]
    #[inline(never)]
    fn add_slow(&mut self, left: Value, right: Value) -> Result<Value, VMError> {
        // Check for Instance with valueOf/toString before falling to immutable path
        let left = self.coerce_instance_for_arithmetic(left)?;
        let right = self.coerce_instance_for_arithmetic(right)?;
        let lo = val_as_obj_ref(left, &self.heap);
        let ro = val_as_obj_ref(right, &self.heap);
        let result = self.add_objects(&lo, &ro)?;
        Ok(obj_into_val(result, &mut self.heap))
    }

    /// Handle Add when left is a heap ref or inline string.
    /// Checks if left is a string (heap or inline) and dispatches to
    /// string concat with scratch buffer, or falls back to add_slow.
    #[inline(never)]
    fn add_string_or_object(&mut self, left: Value, right: Value) -> Result<Value, VMError> {
        // Extract left string into scratch buffer
        self.string_concat_buf.clear();
        if left.is_heap() {
            let lo_ref = unsafe { &*self.heap.objects.as_ptr().add(left.heap_index() as usize) };
            if let Object::String(a) = lo_ref {
                self.string_concat_buf.push_str(a);
            } else {
                // Non-string heap object (array + number, etc.)
                return self.add_slow(left, right);
            }
        } else {
            // Inline string
            let (lbuf, llen) = left.inline_str_buf();
            let a = unsafe { std::str::from_utf8_unchecked(&lbuf[..llen]) };
            self.string_concat_buf.push_str(a);
        }

        // Append right side
        if right.is_heap() {
            let ro_ref = unsafe { &*self.heap.objects.as_ptr().add(right.heap_index() as usize) };
            if let Object::String(b) = ro_ref {
                self.string_concat_buf.push_str(b);
            } else if matches!(ro_ref, Object::Instance(_)) {
                // Instance: call toString() if available.
                // Save buf since coerce may re-enter string concat.
                let saved_buf = std::mem::take(&mut self.string_concat_buf);
                let coerced = self.coerce_to_string_val(right)?;
                self.string_concat_buf = saved_buf;
                self.string_concat_buf.push_str(&coerced);
            } else {
                // string + non-string object → full add
                let lo = Object::String(Rc::from(self.string_concat_buf.as_str()));
                let result = self.add_objects(&lo, ro_ref)?;
                return Ok(obj_into_val(result, &mut self.heap));
            }
        } else if right.is_inline_str() {
            let (rbuf, rlen) = right.inline_str_buf();
            let b = unsafe { std::str::from_utf8_unchecked(&rbuf[..rlen]) };
            self.string_concat_buf.push_str(b);
        } else if right.is_i32() {
            let b = unsafe { right.as_i32_unchecked() };
            let mut buf = itoa::Buffer::new();
            self.string_concat_buf.push_str(buf.format(b));
        } else {
            // string + other → full add
            let lo = Object::String(Rc::from(self.string_concat_buf.as_str()));
            let ro = val_as_obj_ref(right, &self.heap);
            let result = self.add_objects(&lo, &ro)?;
            return Ok(obj_into_val(result, &mut self.heap));
        }

        if self.string_concat_buf.len() > crate::vm::MAX_STRING_LENGTH {
            return Err(VMError::TypeError("String length exceeded".to_string()));
        }
        Ok(obj_into_val(
            Object::String(Rc::from(self.string_concat_buf.as_str())),
            &mut self.heap,
        ))
    }

    #[cold]
    #[inline(never)]
    fn sub_slow(&mut self, left: Value, right: Value) -> Result<Value, VMError> {
        let a = self.coerce_to_number_val(left)?;
        let b = self.coerce_to_number_val(right)?;
        let out = a - b;
        if out.is_finite() && out.fract() == 0.0 {
            Ok(Value::from_i64(out as i64))
        } else {
            Ok(Value::from_f64(out))
        }
    }

    #[cold]
    #[inline(never)]
    fn mul_slow(&mut self, left: Value, right: Value) -> Result<Value, VMError> {
        let a = self.coerce_to_number_val(left)?;
        let b = self.coerce_to_number_val(right)?;
        let out = a * b;
        if out.is_finite() && out.fract() == 0.0 {
            Ok(Value::from_i64(out as i64))
        } else {
            Ok(Value::from_f64(out))
        }
    }

    #[cold]
    #[inline(never)]
    fn div_slow(&mut self, left: Value, right: Value) -> Result<Value, VMError> {
        let a = self.coerce_to_number_val(left)?;
        let b = self.coerce_to_number_val(right)?;
        Ok(Value::from_f64(a / b))
    }

    #[cold]
    #[inline(never)]
    fn mod_slow(&mut self, left: Value, right: Value) -> Result<Value, VMError> {
        let a = self.coerce_to_number_val(left)?;
        let b = self.coerce_to_number_val(right)?;
        let out = a % b;
        if out.is_finite() && out.fract() == 0.0 {
            Ok(Value::from_i64(out as i64))
        } else {
            Ok(Value::from_f64(out))
        }
    }

    /// Coerce an Instance to a primitive via valueOf()/toString() for arithmetic.
    /// For `+` operator which can result in either string concat or numeric add,
    /// we need to return the coerced Value (not just f64).
    fn coerce_instance_for_arithmetic(&mut self, val: Value) -> Result<Value, VMError> {
        if val.is_heap() {
            let heap_idx = val.heap_index() as usize;
            let heap_obj = unsafe { &*self.heap.objects.as_ptr().add(heap_idx) };
            if let Object::Instance(inst) = heap_obj {
                // Try valueOf first (default hint for +)
                if let Some(value_of_func) = inst.methods.get("valueOf").cloned() {
                    let (result, _) = self.execute_compiled_function_slice(
                        value_of_func,
                        &[],
                        Some(val),
                    )?;
                    return Ok(result);
                }
                // Fall back to toString
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

    #[cold]
    #[inline(never)]
    fn comparison_slow(&mut self, op: ROp, left: Value, right: Value) -> Result<bool, VMError> {
        // Coerce Instance objects via valueOf before comparison
        let left = self.coerce_instance_for_arithmetic(left)?;
        let right = self.coerce_instance_for_arithmetic(right)?;
        let lo = val_to_obj(left, &self.heap);
        let ro = val_to_obj(right, &self.heap);
        let result = self.eval_comparison(op, &lo, &ro)?;
        Ok(if result.is_bool() {
            unsafe { result.as_bool_unchecked() }
        } else {
            false
        })
    }

    #[cold]
    #[inline(never)]
    fn equality_slow(&mut self, left: Value, right: Value) -> bool {
        // Coerce Instance objects via valueOf for loose equality
        let left = match self.coerce_instance_for_arithmetic(left) {
            Ok(v) => v,
            Err(_) => left,
        };
        let right = match self.coerce_instance_for_arithmetic(right) {
            Ok(v) => v,
            Err(_) => right,
        };
        let lo = val_as_obj_ref(left, &self.heap);
        let ro = val_as_obj_ref(right, &self.heap);
        self.equals(&lo, &ro)
    }

    #[cold]
    #[inline(never)]
    fn strict_equality_slow(&mut self, left: Value, right: Value) -> bool {
        let lo = val_as_obj_ref(left, &self.heap);
        let ro = val_as_obj_ref(right, &self.heap);
        Self::strict_equal(&lo, &ro)
    }

    #[cold]
    #[inline(never)]
    fn bitwise_slow(&mut self, op: ROp, left: Value, right: Value) -> Result<Value, VMError> {
        let lo = val_as_obj_ref(left, &self.heap);
        let ro = val_as_obj_ref(right, &self.heap);
        let result = self.eval_bitwise(op, &lo, &ro)?;
        Ok(obj_into_val(result, &mut self.heap))
    }

    #[inline(always)]
    fn pow_impl(&mut self, lv: Value, rv: Value) -> Result<Value, VMError> {
        let (base_n, exp_n) = if lv.is_number() && rv.is_number() {
            (lv.to_number(), rv.to_number())
        } else {
            let lo = val_as_obj_ref(lv, &self.heap);
            let ro = val_as_obj_ref(rv, &self.heap);
            (self.to_number(&lo)?, self.to_number(&ro)?)
        };
        let out = base_n.powf(exp_n);
        Ok(if out.is_finite() && out.fract() == 0.0 {
            Value::from_i64(out as i64)
        } else {
            Value::from_f64(out)
        })
    }

    #[cold]
    #[inline(never)]
    fn fused_add_const_slow(&mut self, lv: Value, const_idx: usize) -> Result<Value, VMError> {
        let lo = val_as_obj_ref(lv, &self.heap);
        let cv = unsafe { (&*self.constants_raw).get_unchecked(const_idx) };
        let result = self.add_objects(&lo, cv)?;
        Ok(obj_into_val(result, &mut self.heap))
    }

    #[cold]
    #[inline(never)]
    fn fused_test_lt_slow(&mut self, lv: Value, const_idx: usize) -> Result<bool, VMError> {
        let lo = val_as_obj_ref(lv, &self.heap);
        let cv = unsafe { (&*self.constants_raw).get_unchecked(const_idx) };
        self.compare_numeric(&lo, cv, crate::code::Opcode::OpLessThan)
    }

    #[cold]
    #[inline(never)]
    fn fused_test_le_slow(&mut self, lv: Value, const_idx: usize) -> Result<bool, VMError> {
        let lo = val_as_obj_ref(lv, &self.heap);
        let cv = unsafe { (&*self.constants_raw).get_unchecked(const_idx) };
        self.compare_numeric(&lo, cv, crate::code::Opcode::OpLessOrEqual)
    }

    #[cold]
    #[inline(never)]
    fn fused_mod_strict_eq_slow(
        &mut self,
        lv: Value,
        mod_idx: usize,
        cmp_idx: usize,
    ) -> Result<bool, VMError> {
        let lo = val_as_obj_ref(lv, &self.heap);
        let mod_const = unsafe { (&*self.constants_raw).get_unchecked(mod_idx) };
        let mod_result = self.mod_objects(&lo, mod_const)?;
        let cmp_const = unsafe { (&*self.constants_raw).get_unchecked(cmp_idx) };
        Ok(self.strict_equals(&mod_result, cmp_const))
    }

    // ── Cold dispatch-arm slow paths ─────────────────────────────────────
    // These extract the cold/fallback bodies from large match arms (Call,
    // CallMethod, AddConstToRegProp, AddRegPropsToRegProp) so the hot fast
    // paths stay tight in the L1 i-cache.

    #[cold]
    #[inline(never)]
    fn call_slow(
        &mut self,
        callee_val: Value,
        arg_start: usize,
        nargs: usize,
    ) -> Result<Value, VMError> {
        let arg_slice =
            unsafe { std::slice::from_raw_parts(self.stack.as_ptr().add(arg_start), nargs) };
        self.call_value_slice(callee_val, arg_slice)
    }

    #[cold]
    #[inline(never)]
    fn call_method_slow(
        &mut self,
        obj_val: Value,
        prop_idx: usize,
        cache_slot: usize,
        nargs: usize,
        arg_start: usize,
    ) -> Result<Value, VMError> {
        let prop_sym = unsafe { *self.constants_syms_ptr.add(prop_idx) };

        // Instance: look up method directly and call with the original heap
        // reference as receiver so that `this` mutations persist in-place.
        if obj_val.is_heap() {
            let heap_idx = obj_val.heap_index() as usize;
            let method_opt = match self.heap.objects.get(heap_idx) {
                Some(Object::Instance(inst)) => {
                    let prop_name = crate::intern::resolve(prop_sym);
                    inst.methods.get(&*prop_name).cloned()
                }
                _ => None,
            };
            if let Some(method) = method_opt {
                let arg_slice = unsafe {
                    std::slice::from_raw_parts(self.stack.as_ptr().add(arg_start), nargs)
                };
                let (result, _) =
                    self.execute_compiled_function_slice(method, arg_slice, Some(obj_val))?;
                return Ok(result);
            }
        }

        // Built-in methods on Hash objects (hasOwnProperty)
        if prop_sym == self.sym_has_own_property && obj_val.is_heap() {
            let heap_idx = obj_val.heap_index() as usize;
            if let Some(Object::Hash(h)) = self.heap.objects.get(heap_idx) {
                let key_val = if nargs >= 1 {
                    unsafe { *self.stack.get_unchecked(arg_start) }
                } else {
                    Value::UNDEFINED
                };
                let key_str = {
                    let obj = val_to_obj(key_val, &self.heap);
                    match obj {
                        Object::String(s) => s.to_string(),
                        _ => obj.inspect(),
                    }
                };
                let has = h.borrow().contains_str(&key_str);
                return Ok(Value::from_bool(has));
            }
        }

        // Promise.then() / .catch() — synchronous settlement
        if obj_val.is_heap() {
            let heap_obj = self.heap.get(obj_val.heap_index());
            if let Object::Promise(p) = heap_obj {
                if prop_sym == self.sym_then && nargs >= 1 {
                    let callback = unsafe { *self.stack.get_unchecked(arg_start) };
                    match &p.settled {
                        PromiseState::Fulfilled(v) => {
                            let arg = obj_into_val(v.as_ref().clone(), &mut self.heap);
                            let result = self.call_value_slice(callback, &[arg])?;
                            let promise = Object::Promise(Box::new(PromiseObject {
                                settled: PromiseState::Fulfilled(Box::new(val_to_obj(
                                    result, &self.heap,
                                ))),
                            }));
                            return Ok(obj_into_val(promise, &mut self.heap));
                        }
                        PromiseState::Rejected(_) => {
                            // .then() on rejected: skip callback, return same rejected promise
                            return Ok(obj_val);
                        }
                    }
                } else if prop_sym == self.sym_catch && nargs >= 1 {
                    let callback = unsafe { *self.stack.get_unchecked(arg_start) };
                    match &p.settled {
                        PromiseState::Rejected(v) => {
                            let arg = obj_into_val(v.as_ref().clone(), &mut self.heap);
                            let result = self.call_value_slice(callback, &[arg])?;
                            let promise = Object::Promise(Box::new(PromiseObject {
                                settled: PromiseState::Fulfilled(Box::new(val_to_obj(
                                    result, &self.heap,
                                ))),
                            }));
                            return Ok(obj_into_val(promise, &mut self.heap));
                        }
                        PromiseState::Fulfilled(_) => {
                            // .catch() on fulfilled: skip callback, return same fulfilled promise
                            return Ok(obj_val);
                        }
                    }
                }
            }
        }

        let callee_val = self.get_property_val(obj_val, prop_sym, cache_slot)?;
        let arg_slice =
            unsafe { std::slice::from_raw_parts(self.stack.as_ptr().add(arg_start), nargs) };
        self.call_value_slice(callee_val, arg_slice)
    }

    #[cold]
    #[inline(never)]
    fn fused_add_const_to_prop_slow(
        &mut self,
        obj_val: Value,
        obj_r: usize,
        prop_const_idx: usize,
        val_const_idx: usize,
        cache_slot: usize,
        regs: *mut Value,
    ) -> Result<(), VMError> {
        let add_val = unsafe { &*(&*self.constants_raw).as_ptr().add(val_const_idx) };
        let prop_sym = unsafe { *self.constants_syms_ptr.add(prop_const_idx) };
        let prop_val = self.get_property_val(obj_val, prop_sym, cache_slot)?;
        let prop_ref = val_as_obj_ref(prop_val, &self.heap);
        let result = self.add_objects(&prop_ref, add_val)?;
        let result_val = obj_into_val(result, &mut self.heap);
        if let Some(updated) = self.set_property_val(obj_val, prop_sym, result_val, cache_slot)? {
            unsafe { *regs.add(obj_r) = updated };
        }
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn fused_add_reg_props_slow(
        &mut self,
        obj_val: Value,
        obj_r: usize,
        s1_prop_idx: usize,
        s2_prop_idx: usize,
        dst_prop_idx: usize,
        s1_cache: usize,
        s2_cache: usize,
        dst_cache: usize,
        regs: *mut Value,
    ) -> Result<(), VMError> {
        let s1_sym = unsafe { *self.constants_syms_ptr.add(s1_prop_idx) };
        let s2_sym = unsafe { *self.constants_syms_ptr.add(s2_prop_idx) };
        let v1 = self.get_property_val(obj_val, s1_sym, s1_cache)?;
        let v2 = self.get_property_val(obj_val, s2_sym, s2_cache)?;
        let v1_ref = val_as_obj_ref(v1, &self.heap);
        let v2_ref = val_as_obj_ref(v2, &self.heap);
        let result = self.add_objects(&v1_ref, &v2_ref)?;
        let result_val = obj_into_val(result, &mut self.heap);
        let dst_sym = unsafe { *self.constants_syms_ptr.add(dst_prop_idx) };
        if let Some(updated) = self.set_property_val(obj_val, dst_sym, result_val, dst_cache)? {
            unsafe { *regs.add(obj_r) = updated };
        }
        Ok(())
    }

    // ── Helper methods needed by register dispatch ──────────────────────

    fn eval_comparison(
        &mut self,
        op: ROp,
        left: &Object,
        right: &Object,
    ) -> Result<Value, VMError> {
        use ROp::*;
        let result = match op {
            Equal => self.equals(left, right),
            NotEqual => !self.equals(left, right),
            StrictEqual => Self::strict_equal(left, right),
            StrictNotEqual => !Self::strict_equal(left, right),
            GreaterThan => self.compare_numeric(left, right, crate::code::Opcode::OpGreaterThan)?,
            GreaterOrEqual => {
                self.compare_numeric(left, right, crate::code::Opcode::OpGreaterOrEqual)?
            }
            LessThan => self.compare_numeric(left, right, crate::code::Opcode::OpLessThan)?,
            LessOrEqual => self.compare_numeric(left, right, crate::code::Opcode::OpLessOrEqual)?,
            Instanceof => self.op_instanceof(left, right),
            In => self.op_in(left, right),
            _ => false,
        };
        Ok(if result { Value::TRUE } else { Value::FALSE })
    }

    fn eval_bitwise(&self, op: ROp, left: &Object, right: &Object) -> Result<Object, VMError> {
        use ROp::*;
        let a = self.to_i32(left)?;
        let b = self.to_i32(right)?;
        if matches!(op, UnsignedRightShift) {
            // Unsigned right shift: result is u32 (always non-negative)
            let result = ((a as u32) >> (b as u32 & 31)) as u32;
            return Ok(Object::Integer(result as i64));
        }
        let result = match op {
            BitwiseAnd => a & b,
            BitwiseOr => a | b,
            BitwiseXor => a ^ b,
            LeftShift => a << (b & 31),
            RightShift => a >> (b & 31),
            _ => return Err(VMError::TypeError("invalid bitwise op".to_string())),
        };
        Ok(Object::Integer(result as i64))
    }
}
