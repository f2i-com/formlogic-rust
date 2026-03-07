/// Register-based opcode set.
///
/// Each instruction explicitly names source/destination registers (u16 indices)
/// within the current call frame's register window. Operand widths are 1 (u8,
/// small count or flag) or 2 (u16, register index / constant index / global
/// index / jump target / cache slot).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ROp {
    // ── Loads ─────────────────────────────────────────────────────────────
    /// Load constant into register.  [dst:2, const_idx:2]
    LoadConst,
    /// Load `true`.  [dst:2]
    LoadTrue,
    /// Load `false`.  [dst:2]
    LoadFalse,
    /// Load `null`.  [dst:2]
    LoadNull,
    /// Load `undefined`.  [dst:2]
    LoadUndef,

    // ── Register ops ──────────────────────────────────────────────────────
    /// Copy register.  [dst:2, src:2]
    Move,

    // ── Global access ─────────────────────────────────────────────────────
    /// Load global into register.  [dst:2, global_idx:2]
    GetGlobal,
    /// Store register into global.  [global_idx:2, src:2]
    SetGlobal,

    // ── Arithmetic ────────────────────────────────────────────────────────
    /// [dst:2, left:2, right:2]
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,

    // ── Comparison ────────────────────────────────────────────────────────
    /// [dst:2, left:2, right:2]
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    GreaterThan,
    GreaterOrEqual,
    LessThan,
    LessOrEqual,
    Instanceof,
    In,

    // ── Bitwise ───────────────────────────────────────────────────────────
    /// [dst:2, left:2, right:2]
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    LeftShift,
    RightShift,
    UnsignedRightShift,

    // ── Unary ─────────────────────────────────────────────────────────────
    /// [dst:2, src:2]
    Neg,
    Not,
    UnaryPlus,
    Typeof,
    IsNullish,

    // ── Control flow ──────────────────────────────────────────────────────
    /// Unconditional jump.  [target:2]
    Jump,
    /// Jump if register is falsy.  [cond:2, target:2]
    JumpIfNot,
    /// Jump if register is truthy.  [cond:2, target:2]
    JumpIfTruthy,

    // ── Function calls ────────────────────────────────────────────────────
    /// Call function in `base` register with args in base+1..base+nargs.
    /// Result goes into `dst`.  [dst:2, base:2, nargs:1]
    Call,
    /// Fused method call: obj.method(args).  Object in `base` register,
    /// args in base+1..base+nargs.  [dst:2, base:2, nargs:1, prop_const:2, cache:2]
    CallMethod,
    /// Call with spread args array.  [dst:2, func:2, args_arr:2]
    CallSpread,
    /// Return value from function.  [src:2]
    Return,
    /// Return undefined from function.  []
    ReturnUndef,

    // ── Constructors ──────────────────────────────────────────────────────
    /// `new` with args in base+1..base+nargs.  [dst:2, base:2, nargs:1]
    New,
    /// `new` with spread args.  [dst:2, class:2, args_arr:2]
    NewSpread,
    /// Load super reference.  [dst:2]
    Super,

    // ── Collections ───────────────────────────────────────────────────────
    /// Create array from contiguous regs base..base+count-1.  [dst:2, base:2, count:2]
    Array,
    /// Create hash from contiguous regs (key,val pairs).  [dst:2, base:2, count:2]
    Hash,
    /// Append element to array.  [arr:2, val:2]
    AppendElement,
    /// Spread-append iterable to array.  [arr:2, iterable:2]
    AppendSpread,

    // ── Property access ───────────────────────────────────────────────────
    /// Get named property (inline-cached).  [dst:2, obj:2, prop_const:2, cache:2]
    GetProp,
    /// Set named property (inline-cached). Keeps value.  [obj:2, prop_const:2, src:2, cache:2]
    SetProp,
    /// Get property on global object.  [dst:2, global_idx:2, prop_const:2, cache:2]
    GetGlobalProp,
    /// Set property on global object.  [global_idx:2, prop_const:2, src:2, cache:2]
    SetGlobalProp,

    // ── Index access ──────────────────────────────────────────────────────
    /// Dynamic index read.  [dst:2, obj:2, key:2]
    Index,
    /// Dynamic index write.  [obj:2, key:2, val:2]
    SetIndex,
    /// Delete property.  [dst:2, obj:2, key:2]
    DeleteProp,

    // ── Iterator / destructuring ──────────────────────────────────────────
    /// Slice array from index.  [dst:2, iterable:2, skip:2]
    IteratorRest,
    /// Get object keys as array.  [dst:2, obj:2]
    GetKeysIter,
    /// Object rest (exclude keys in keys_base..keys_base+count-1).
    /// [dst:2, obj:2, keys_base:2, count:2]
    ObjectRest,

    // ── Async ─────────────────────────────────────────────────────────────
    /// Await a value.  [dst:2, src:2]
    Await,

    // ── Error ─────────────────────────────────────────────────────────────
    /// Throw value.  [src:2]
    Throw,

    // ── Fused ─────────────────────────────────────────────────────────────
    /// Fused: load global, call with args.  [dst:2, global_idx:2, base:2, nargs:1]
    CallGlobal,
    /// Fused: regs[dst] = regs[src] + constants[idx].  [dst:2, src:2, const_idx:2]
    AddRegConst,
    /// Fused: if !(regs[r] < constants[idx]) jump target.  [r:2, const_idx:2, target:2]
    TestLtConstJump,
    /// Fused: if !(regs[r] <= constants[idx]) jump target.  [r:2, const_idx:2, target:2]
    TestLeConstJump,
    /// Fused: regs[r] += constants[idx]; jump target.  [r:2, const_idx:2, target:2]
    IncrementRegAndJump,
    /// Fused: if !((regs[r] % const_a) === const_b) jump target.
    /// [r:2, mod_const:2, cmp_const:2, target:2]
    ModRegConstStrictEqConstJump,
    /// Fused: obj.prop += const (with inline cache).
    /// [obj:2, prop_const:2, val_const:2, cache:2]
    AddConstToRegProp,
    /// Fused: obj.dst_prop = obj.src1_prop + obj.src2_prop (with inline cache).
    /// [obj:2, s1_prop:2, s1_cache:2, s2_prop:2, s2_cache:2, dst_prop:2, dst_cache:2]
    AddRegPropsToRegProp,

    /// Define a getter or setter on a hash object.
    /// [hash:2, func:2, prop_const:2, kind:1] where kind 0 = getter, 1 = setter.
    DefineAccessor,

    /// Run static initializers on a class object (in-place).
    /// [dst:2] — reads class from dst register, runs static initializers, writes back.
    InitClass,

    /// Load `new.target` into register.  [dst:2]
    NewTarget,
    /// Load `import.meta` into register.  [dst:2]
    ImportMeta,
    /// Yield a value from a generator.  [dst:2, src:2]
    /// `src` is the yielded value; on resume, the value passed to `.next(v)`
    /// is written to `dst`.
    Yield,

    // ── Closures ──────────────────────────────────────────────────────────
    /// Create a closure by snapshotting captured global slots.
    /// [dst:2, const_idx:2, count:1]
    /// Followed by `count` pairs of [slot:2] — the global slot indices to capture.
    MakeClosure,

    // ── Halt ──────────────────────────────────────────────────────────────
    /// Halt execution (no result).  []
    Halt,
    /// Halt execution with result.  [src:2]
    HaltValue,
}

impl ROp {
    pub fn from_byte(value: u8) -> Option<Self> {
        if value <= ROp::HaltValue as u8 {
            Some(unsafe { std::mem::transmute::<u8, ROp>(value) })
        } else {
            None
        }
    }

    /// Total instruction size in bytes (opcode + operands).
    pub fn size(self) -> usize {
        1 + self.operand_bytes()
    }

    /// Total operand bytes (not including the opcode byte itself).
    pub fn operand_bytes(self) -> usize {
        use ROp::*;
        match self {
            // []
            ReturnUndef | Halt => 0,
            // [dst:2] or [src:2]
            LoadTrue | LoadFalse | LoadNull | LoadUndef | Super | HaltValue | Return | Throw
            | InitClass | NewTarget | ImportMeta => 2,
            // [dst:2, src:2]
            Move | Neg | Not | UnaryPlus | Typeof | IsNullish | AppendElement | AppendSpread
            | GetKeysIter | Await | Yield => 4,
            // [target:2]
            Jump => 2,
            // [dst:2, const:2] or [dst:2, global:2]
            LoadConst | GetGlobal => 4,
            // [global:2, src:2]
            SetGlobal => 4,
            // [cond:2, target:2]
            JumpIfNot | JumpIfTruthy => 4,
            // [dst:2, left:2, right:2] — all binary ops, index ops
            Add | Sub | Mul | Div | Mod | Pow | Equal | NotEqual | StrictEqual | StrictNotEqual
            | GreaterThan | GreaterOrEqual | LessThan | LessOrEqual | Instanceof | In
            | BitwiseAnd | BitwiseOr | BitwiseXor | LeftShift | RightShift | UnsignedRightShift
            | CallSpread | NewSpread | Index | SetIndex | DeleteProp => 6,
            // [dst:2, base:2, nargs:1]
            Call | New => 5,
            // [dst:2, base:2, count:2]
            Array | Hash | IteratorRest => 6,
            // [dst:2, obj:2, keys_base:2, count:2]
            ObjectRest => 8,
            // [dst:2, global:2, base:2, nargs:1]
            CallGlobal => 7,
            // [dst:2, base:2, nargs:1, prop_const:2, cache:2]
            CallMethod => 9,
            // [dst:2, src:2, const:2]
            AddRegConst => 6,
            // [r:2, const:2, target:2]
            TestLtConstJump | TestLeConstJump | IncrementRegAndJump => 6,
            // [r:2, mod_const:2, cmp_const:2, target:2]
            ModRegConstStrictEqConstJump => 8,
            // [obj:2, prop_const:2, val_const:2, cache:2]
            AddConstToRegProp => 8,
            // [obj:2, s1_prop:2, s1_cache:2, s2_prop:2, s2_cache:2, dst_prop:2, dst_cache:2]
            AddRegPropsToRegProp => 14,
            // [hash:2, func:2, prop_const:2, kind:1]
            DefineAccessor => 7,
            // [dst:2, const_idx:2, count:1] + count*2 variable bytes
            // Note: operand_bytes returns just the fixed part; callers must add count*2.
            MakeClosure => 5,
            // [dst:2, obj:2, prop:2, cache:2]
            GetProp => 8,
            // [obj:2, prop:2, src:2, cache:2]
            SetProp => 8,
            // [dst:2, global:2, prop:2, cache:2]
            GetGlobalProp => 8,
            // [global:2, prop:2, src:2, cache:2]
            SetGlobalProp => 8,
        }
    }
}

/// Encode a register-based instruction into a byte vector.
pub fn rmake(op: ROp, operands: &[u16]) -> Vec<u8> {
    use ROp::*;
    // Get operand widths for this opcode
    let widths: &[usize] = match op {
        LoadConst | GetGlobal => &[2, 2],
        SetGlobal => &[2, 2],
        LoadTrue | LoadFalse | LoadNull | LoadUndef | Super | HaltValue | InitClass | NewTarget
        | ImportMeta => &[2],
        Move | Neg | Not | UnaryPlus | Typeof | IsNullish | AppendElement | AppendSpread
        | GetKeysIter | Await | Yield => &[2, 2],
        Add | Sub | Mul | Div | Mod | Pow | Equal | NotEqual | StrictEqual | StrictNotEqual
        | GreaterThan | GreaterOrEqual | LessThan | LessOrEqual | Instanceof | In | BitwiseAnd
        | BitwiseOr | BitwiseXor | LeftShift | RightShift | UnsignedRightShift | CallSpread
        | NewSpread | Index | SetIndex | DeleteProp => &[2, 2, 2],
        Jump => &[2],
        JumpIfNot | JumpIfTruthy => &[2, 2],
        Return | Throw => &[2],
        ReturnUndef | Halt => &[],
        Call | New => &[2, 2, 1],
        Array | Hash | IteratorRest => &[2, 2, 2],
        GetProp => &[2, 2, 2, 2],
        SetProp => &[2, 2, 2, 2],
        GetGlobalProp => &[2, 2, 2, 2],
        SetGlobalProp => &[2, 2, 2, 2],
        ObjectRest => &[2, 2, 2, 2],
        CallGlobal => &[2, 2, 2, 1],
        CallMethod => &[2, 2, 1, 2, 2],
        AddRegConst => &[2, 2, 2],
        TestLtConstJump | TestLeConstJump | IncrementRegAndJump => &[2, 2, 2],
        ModRegConstStrictEqConstJump => &[2, 2, 2, 2],
        AddConstToRegProp => &[2, 2, 2, 2],
        AddRegPropsToRegProp => &[2, 2, 2, 2, 2, 2, 2],
        DefineAccessor => &[2, 2, 2, 1],
        // Variable-length: [dst:2, const_idx:2, count:1, slot0:2, slot1:2, ...]
        // rmake handles this specially below
        MakeClosure => &[2, 2, 1],
    };

    let mut len = 1usize;
    for w in widths {
        len += w;
    }
    let mut out = vec![0u8; len];
    out[0] = op as u8;
    let mut offset = 1usize;
    for (i, operand) in operands.iter().enumerate() {
        let width = *widths.get(i).unwrap_or(&0);
        match width {
            2 => {
                out[offset] = ((operand >> 8) & 0xff) as u8;
                out[offset + 1] = (operand & 0xff) as u8;
            }
            1 => {
                out[offset] = (operand & 0xff) as u8;
            }
            _ => {}
        }
        offset += width;
    }

    // Variable-length tail for MakeClosure: append slot indices as u16 big-endian
    if op == MakeClosure && operands.len() > 3 {
        for &slot in &operands[3..] {
            out.push(((slot >> 8) & 0xff) as u8);
            out.push((slot & 0xff) as u8);
        }
    }

    out
}
