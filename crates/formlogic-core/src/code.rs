#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Opcode {
    OpConstant,
    OpAdd,
    OpSub,
    OpMul,
    OpDiv,
    OpMod,
    OpPop,
    OpTrue,
    OpFalse,
    OpNull,
    OpUndefined,
    OpEqual,
    OpNotEqual,
    OpGreaterThan,
    OpMinus,
    OpBang,
    OpJumpNotTruthy,
    OpJump,
    OpGetGlobal,
    OpSetGlobal,
    OpGetLocal,
    OpSetLocal,
    OpArray,
    OpHash,
    OpIndex,
    OpCall,
    OpReturnValue,
    OpReturn,
    OpGetProperty,
    OpGetLocalProperty,
    OpGetGlobalProperty,
    OpSetLocalProperty,
    OpSetGlobalProperty,
    OpAwait,
    OpAppendElement,
    OpAppendSpread,
    OpThrow,
    OpSetIndex,
    OpNew,
    OpSuper,
    OpTypeof,
    OpPow,
    OpIsNullish,
    OpStrictEqual,
    OpStrictNotEqual,
    OpInstanceof,
    OpIn,
    OpDeleteProperty,
    OpUnsignedRightShift,
    OpBitwiseAnd,
    OpBitwiseOr,
    OpBitwiseXor,
    OpLeftShift,
    OpRightShift,
    OpLessThan,
    OpLessOrEqual,
    OpGreaterOrEqual,
    OpIteratorRest,
    OpGetKeysIterator,
    OpObjectRest,
    OpUnaryPlus,
    OpCallSpread,
    OpNewSpread,
    OpSetLocalPropertyPop,
    OpSetGlobalPropertyPop,
    /// Fused: local[idx] += constants[const_idx] (numeric add), no stack effect.
    /// Replaces: OpGetLocal + OpConstant + OpAdd + OpSetLocal + OpGetLocal + OpPop
    /// Operands: [local_idx: u16, const_idx: u16]
    OpIncrementLocal,
    /// Fused: if !(local[idx] < constants[const_idx]) jump target.
    /// Replaces: OpGetLocal + OpConstant + OpLessThan + OpJumpNotTruthy
    /// Operands: [local_idx: u16, const_idx: u16, jump_target: u16]
    OpTestLocalLtConstJump,
    /// Fused: if !(local[idx] <= constants[const_idx]) jump target.
    /// Replaces: OpGetLocal + OpConstant + OpLessOrEqual + OpJumpNotTruthy
    /// Operands: [local_idx: u16, const_idx: u16, jump_target: u16]
    OpTestLocalLeConstJump,
    /// Fused: globals[idx] += constants[const_idx] (numeric add), no stack effect.
    /// Replaces: OpGetGlobal + OpConstant + OpAdd + OpSetGlobal + OpGetGlobal + OpPop
    /// Operands: [global_idx: u16, const_idx: u16]
    OpIncrementGlobal,
    /// Fused: if !(globals[idx] < constants[const_idx]) jump target.
    /// Replaces: OpGetGlobal + OpConstant + OpLessThan + OpJumpNotTruthy
    /// Operands: [global_idx: u16, const_idx: u16, jump_target: u16]
    OpTestGlobalLtConstJump,
    /// Fused: if !(globals[idx] <= constants[const_idx]) jump target.
    /// Replaces: OpGetGlobal + OpConstant + OpLessOrEqual + OpJumpNotTruthy
    /// Operands: [global_idx: u16, const_idx: u16, jump_target: u16]
    OpTestGlobalLeConstJump,
    /// Fused: globals[target] = globals[target] + globals[source] (numeric add), no stack effect.
    /// Replaces: OpGetGlobal + OpGetGlobal + OpAdd + OpSetGlobal (+ elided OpGetGlobal + OpPop)
    /// Operands: [target_idx: u16, source_idx: u16]
    OpAccumulateGlobal,
    /// Fused: globals[idx] += constants[const_idx]; jump target (always backward).
    /// Replaces: OpIncrementGlobal + OpJump at end of for-loop update.
    /// Operands: [global_idx: u16, const_idx: u16, jump_target: u16]
    OpIncrementGlobalAndJump,
    /// Fused: call globals[global_idx] with num_args args from stack.
    /// Replaces: OpGetGlobal + OpCall when calling a named global function.
    /// Eliminates Box allocation for CompiledFunctionObject clone.
    /// Operands: [global_idx: u16, num_args: u16]
    OpCallGlobal,
    /// Fused: if !((globals[idx] % constants[mod_const]) === constants[cmp_const]) jump target.
    /// Replaces: OpGetGlobal + OpConstant + OpMod + OpConstant + OpStrictEqual + OpJumpNotTruthy
    /// Operands: [global_idx: u16, mod_const_idx: u16, cmp_const_idx: u16, jump_target: u16]
    OpModGlobalConstStrictEqConstJump,
    /// Fused: globals[global_idx].prop += constants[val_const_idx] (numeric add in-place).
    /// Replaces: OpGetGlobalProperty + OpConstant + OpAdd + OpSetGlobalPropertyPop
    /// Operands: [global_idx: u16, prop_const_idx: u16, val_const_idx: u16, cache_slot: u16]
    OpAddConstToGlobalProperty,
    /// Fused: globals[obj].dst_prop = globals[obj].src_prop1 + globals[obj].src_prop2 (in-place).
    /// Replaces: OpGetGlobalProperty(obj,src1) + OpGetGlobalProperty(obj,src2) + OpAdd + OpSetGlobalPropertyPop(obj,dst)
    /// Operands: [global_idx: u16, src1_prop_const: u16, src1_cache: u16, src2_prop_const: u16, src2_cache: u16, dst_prop_const: u16, dst_cache: u16]
    OpAddGlobalPropsToGlobalProp,
    /// Define a getter or setter on a hash object.
    /// Stack: [hash, compiled_function] → [hash].
    /// Operands: [prop_const_idx: u16, kind: u16] where kind 0 = getter, 1 = setter.
    OpDefineAccessor,
    /// Run static initializers (fields and blocks) on a class.
    /// Stack: [class] → [class] (class is mutated in place).
    /// Operands: [] (no operands).
    OpInitClass,
    /// Push `new.target` value onto the stack.
    /// Operands: [] (no operands).
    OpNewTarget,
    /// Push `import.meta` object onto the stack.
    /// Operands: [] (no operands).
    OpImportMeta,
    /// Yield a value from a generator function.
    /// Stack: [value] → [received_value].
    /// The dispatch loop breaks with the yielded value; on resume, the
    /// value passed to `.next(v)` is pushed.
    /// Operands: [] (no operands).
    OpYield,
    OpHalt,
}

impl Opcode {
    pub fn from_byte(value: u8) -> Option<Self> {
        if value <= Opcode::OpHalt as u8 {
            // SAFETY: Opcode is repr(u8) with contiguous variants from 0..=OpHalt
            Some(unsafe { std::mem::transmute(value) })
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct Definition {
    pub name: &'static str,
    pub operand_widths: &'static [usize],
}

pub fn lookup(op: Opcode) -> Definition {
    use Opcode::*;
    match op {
        OpConstant => Definition {
            name: "OpConstant",
            operand_widths: &[2],
        },
        OpJumpNotTruthy | OpJump | OpGetGlobal | OpSetGlobal | OpGetLocal | OpSetLocal
        | OpArray | OpHash | OpObjectRest => Definition {
            name: op_name(op),
            operand_widths: &[2],
        },
        OpCall | OpNew => Definition {
            name: op_name(op),
            operand_widths: &[1],
        },
        OpCallSpread | OpNewSpread => Definition {
            name: op_name(op),
            operand_widths: &[],
        },
        OpGetProperty => Definition {
            name: op_name(op),
            operand_widths: &[2, 2],
        },
        OpGetLocalProperty => Definition {
            name: op_name(op),
            operand_widths: &[2, 2, 2],
        },
        OpGetGlobalProperty => Definition {
            name: op_name(op),
            operand_widths: &[2, 2, 2],
        },
        OpSetLocalProperty
        | OpSetGlobalProperty
        | OpSetLocalPropertyPop
        | OpSetGlobalPropertyPop => Definition {
            name: op_name(op),
            operand_widths: &[2, 2, 2],
        },
        OpTestLocalLtConstJump
        | OpTestLocalLeConstJump
        | OpTestGlobalLtConstJump
        | OpTestGlobalLeConstJump
        | OpIncrementGlobalAndJump => Definition {
            name: op_name(op),
            operand_widths: &[2, 2, 2],
        },
        OpModGlobalConstStrictEqConstJump | OpAddConstToGlobalProperty => Definition {
            name: op_name(op),
            operand_widths: &[2, 2, 2, 2],
        },
        OpAddGlobalPropsToGlobalProp => Definition {
            name: op_name(op),
            operand_widths: &[2, 2, 2, 2, 2, 2, 2],
        },
        OpIncrementLocal | OpIncrementGlobal | OpAccumulateGlobal | OpCallGlobal => Definition {
            name: op_name(op),
            operand_widths: &[2, 2],
        },
        OpDefineAccessor => Definition {
            name: "OpDefineAccessor",
            operand_widths: &[2, 2],
        },
        OpInitClass => Definition {
            name: "OpInitClass",
            operand_widths: &[],
        },
        _ => Definition {
            name: op_name(op),
            operand_widths: &[],
        },
    }
}

pub fn op_name(op: Opcode) -> &'static str {
    use Opcode::*;
    match op {
        OpConstant => "OpConstant",
        OpAdd => "OpAdd",
        OpSub => "OpSub",
        OpMul => "OpMul",
        OpDiv => "OpDiv",
        OpMod => "OpMod",
        OpPop => "OpPop",
        OpTrue => "OpTrue",
        OpFalse => "OpFalse",
        OpNull => "OpNull",
        OpUndefined => "OpUndefined",
        OpEqual => "OpEqual",
        OpNotEqual => "OpNotEqual",
        OpGreaterThan => "OpGreaterThan",
        OpMinus => "OpMinus",
        OpBang => "OpBang",
        OpJumpNotTruthy => "OpJumpNotTruthy",
        OpJump => "OpJump",
        OpGetGlobal => "OpGetGlobal",
        OpSetGlobal => "OpSetGlobal",
        OpGetLocal => "OpGetLocal",
        OpSetLocal => "OpSetLocal",
        OpArray => "OpArray",
        OpHash => "OpHash",
        OpIndex => "OpIndex",
        OpCall => "OpCall",
        OpReturnValue => "OpReturnValue",
        OpReturn => "OpReturn",
        OpGetProperty => "OpGetProperty",
        OpGetLocalProperty => "OpGetLocalProperty",
        OpGetGlobalProperty => "OpGetGlobalProperty",
        OpSetLocalProperty => "OpSetLocalProperty",
        OpSetGlobalProperty => "OpSetGlobalProperty",
        OpSetLocalPropertyPop => "OpSetLocalPropertyPop",
        OpSetGlobalPropertyPop => "OpSetGlobalPropertyPop",
        OpAwait => "OpAwait",
        OpAppendElement => "OpAppendElement",
        OpAppendSpread => "OpAppendSpread",
        OpThrow => "OpThrow",
        OpSetIndex => "OpSetIndex",
        OpNew => "OpNew",
        OpSuper => "OpSuper",
        OpTypeof => "OpTypeof",
        OpPow => "OpPow",
        OpIsNullish => "OpIsNullish",
        OpStrictEqual => "OpStrictEqual",
        OpStrictNotEqual => "OpStrictNotEqual",
        OpInstanceof => "OpInstanceof",
        OpIn => "OpIn",
        OpDeleteProperty => "OpDeleteProperty",
        OpUnsignedRightShift => "OpUnsignedRightShift",
        OpBitwiseAnd => "OpBitwiseAnd",
        OpBitwiseOr => "OpBitwiseOr",
        OpBitwiseXor => "OpBitwiseXor",
        OpLeftShift => "OpLeftShift",
        OpRightShift => "OpRightShift",
        OpLessThan => "OpLessThan",
        OpLessOrEqual => "OpLessOrEqual",
        OpGreaterOrEqual => "OpGreaterOrEqual",
        OpIteratorRest => "OpIteratorRest",
        OpGetKeysIterator => "OpGetKeysIterator",
        OpObjectRest => "OpObjectRest",
        OpUnaryPlus => "OpUnaryPlus",
        OpCallSpread => "OpCallSpread",
        OpNewSpread => "OpNewSpread",
        OpIncrementLocal => "OpIncrementLocal",
        OpTestLocalLtConstJump => "OpTestLocalLtConstJump",
        OpTestLocalLeConstJump => "OpTestLocalLeConstJump",
        OpIncrementGlobal => "OpIncrementGlobal",
        OpTestGlobalLtConstJump => "OpTestGlobalLtConstJump",
        OpTestGlobalLeConstJump => "OpTestGlobalLeConstJump",
        OpAccumulateGlobal => "OpAccumulateGlobal",
        OpIncrementGlobalAndJump => "OpIncrementGlobalAndJump",
        OpCallGlobal => "OpCallGlobal",
        OpModGlobalConstStrictEqConstJump => "OpModGlobalConstStrictEqConstJump",
        OpAddConstToGlobalProperty => "OpAddConstToGlobalProperty",
        OpAddGlobalPropsToGlobalProp => "OpAddGlobalPropsToGlobalProp",
        OpDefineAccessor => "OpDefineAccessor",
        OpInitClass => "OpInitClass",
        OpNewTarget => "OpNewTarget",
        OpImportMeta => "OpImportMeta",
        OpYield => "OpYield",
        OpHalt => "OpHalt",
    }
}

pub fn make(op: Opcode, operands: &[u16]) -> Vec<u8> {
    let def = lookup(op);
    let mut len = 1usize;
    for width in def.operand_widths {
        len += width;
    }

    let mut out = vec![0u8; len];
    out[0] = op as u8;
    let mut offset = 1usize;
    for (i, operand) in operands.iter().enumerate() {
        let width = *def.operand_widths.get(i).unwrap_or(&0);
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

    out
}

pub fn read_operands(def: &Definition, ins: &[u8]) -> (Vec<u16>, usize) {
    let mut operands = vec![0u16; def.operand_widths.len()];
    let mut offset = 0usize;
    for (i, width) in def.operand_widths.iter().enumerate() {
        match *width {
            2 => {
                if offset + 1 < ins.len() {
                    operands[i] = ((ins[offset] as u16) << 8) | (ins[offset + 1] as u16);
                }
            }
            1 => {
                if offset < ins.len() {
                    operands[i] = ins[offset] as u16;
                }
            }
            _ => {}
        }
        offset += *width;
    }
    (operands, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_and_read_operands_round_trip() {
        let bytes = make(Opcode::OpConstant, &[655]);
        assert_eq!(bytes[0], Opcode::OpConstant as u8);
        let def = lookup(Opcode::OpConstant);
        let (ops, used) = read_operands(&def, &bytes[1..]);
        assert_eq!(ops, vec![655]);
        assert_eq!(used, 2);
    }
}
