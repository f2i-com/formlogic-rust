use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;

use crate::intern::intern_str;
use crate::object::VmCell;

use crate::ast::ClassMember;
use crate::ast::{
    ArrayBindingItem, BindingPattern, BindingTarget, ForBinding, HashEntry, ObjectBindingItem,
};
use crate::ast::{Expression, Program, Statement, VariableKind};
use crate::bytecode::Bytecode;
use crate::code::{make, Opcode};
use crate::object::{
    make_hash, BuiltinFunction, BuiltinFunctionObject, ClassObject, CompiledFunctionObject,
    HashKey, HashObject, Object, RegExpObject, StaticInitializer,
};

const GLOBALS_SIZE: usize = 65_536;

pub struct Compiler {
    instructions: Vec<u8>,
    constants: Vec<Object>,
    globals: FxHashMap<String, u16>,
    next_global: u16,
    locals: FxHashMap<String, u16>,
    class_defs: FxHashMap<String, ClassObject>,
    next_local: u16,
    is_function_scope: bool,
    last_opcode: Option<Opcode>,
    last_position: usize,
    loop_stack: Vec<LoopContext>,
    temp_counter: usize,
    try_stack: Vec<TryContext>,
    // Constant deduplication maps
    constant_strings: FxHashMap<Rc<str>, u16>,
    constant_ints: FxHashMap<i64, u16>,
    constant_floats: FxHashMap<u64, u16>,
    // Inline cache slot counter for OpGetProperty
    next_cache_slot: u16,
    // Stack depth tracking for bounds check hoisting
    current_stack_depth: i32,
    max_stack_depth: i32,
    // Records an elided trailing OpGet{Local,Global} so that remove_last_pop()
    // can re-emit it when the expression value is actually needed (e.g. last
    // statement in an if-body).
    elided_trailing_get: Option<(Opcode, u16)>,
    // Names declared with `const` — assignment to these is a compile error.
    const_bindings: FxHashSet<String>,
}

struct LoopContext {
    label: Option<String>,
    continue_target: usize,
    break_positions: Vec<usize>,
    continue_positions: Vec<usize>,
}

struct TryContext {
    exception_temp: String,
    throw_jumps: Vec<usize>,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            instructions: vec![],
            constants: vec![],
            globals: FxHashMap::default(),
            next_global: 0,
            locals: FxHashMap::default(),
            class_defs: FxHashMap::default(),
            next_local: 0,
            is_function_scope: false,
            last_opcode: None,
            last_position: 0,
            loop_stack: vec![],
            temp_counter: 0,
            try_stack: vec![],
            constant_strings: FxHashMap::default(),
            constant_ints: FxHashMap::default(),
            constant_floats: FxHashMap::default(),
            next_cache_slot: 0,
            current_stack_depth: 0,
            max_stack_depth: 0,
            elided_trailing_get: None,
            const_bindings: FxHashSet::default(),
        }
    }

    fn new_function_scope(
        globals: FxHashMap<String, u16>,
        next_global: u16,
        parameters: &[String],
    ) -> Self {
        let mut locals = FxHashMap::default();
        for (i, param) in parameters.iter().enumerate() {
            locals.insert(param.clone(), i as u16);
        }

        Self {
            instructions: vec![],
            constants: vec![],
            globals,
            next_global,
            locals,
            class_defs: FxHashMap::default(),
            next_local: parameters.len() as u16,
            is_function_scope: true,
            last_opcode: None,
            last_position: 0,
            loop_stack: vec![],
            temp_counter: 0,
            try_stack: vec![],
            constant_strings: FxHashMap::default(),
            constant_ints: FxHashMap::default(),
            constant_floats: FxHashMap::default(),
            next_cache_slot: 0,
            current_stack_depth: 0,
            max_stack_depth: 0,
            elided_trailing_get: None,
            const_bindings: FxHashSet::default(),
        }
    }

    pub fn compile_program(mut self, program: &Program) -> Result<Bytecode, String> {
        self.hoist_var_declarations(&program.statements)?;
        for stmt in &program.statements {
            self.compile_statement(stmt)?;
        }

        self.emit(Opcode::OpHalt, &[]);

        Ok(Bytecode::with_cache_slots(
            self.instructions,
            self.constants,
            vec![],
            self.next_cache_slot,
            self.max_stack_depth.max(0) as u16,
            0,
        ))
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), String> {
        // Clear stale elision info — only the immediately preceding
        // Statement::Expression's elision should be visible to remove_last_pop().
        self.elided_trailing_get = None;

        match stmt {
            Statement::Let { name, value, kind } => {
                if *kind == VariableKind::Const {
                    self.const_bindings.insert(name.clone());
                }
                let idx = self.ensure_binding_slot(name)?;
                self.compile_expression(value)?;
                if self.is_function_scope {
                    let global_idx = self.ensure_global_slot(name)?;
                    self.emit(Opcode::OpSetLocal, &[idx]);
                    self.emit(Opcode::OpGetLocal, &[idx]);
                    self.emit(Opcode::OpSetGlobal, &[global_idx]);
                } else {
                    self.emit(Opcode::OpSetGlobal, &[idx]);
                }
            }
            Statement::LetPattern {
                pattern,
                value,
                kind,
            } => {
                if *kind == VariableKind::Const {
                    Self::collect_pattern_names(pattern, &mut self.const_bindings);
                }
                self.compile_expression(value)?;
                self.assign_pattern_from_top(pattern)?;
            }
            Statement::Return { value } => {
                self.compile_expression(value)?;
                self.emit(Opcode::OpReturnValue, &[]);
            }
            Statement::ReturnVoid => {
                self.emit(Opcode::OpReturn, &[]);
            }
            Statement::Expression(expr) => {
                // Peephole: compile if-expression in statement position as a
                // statement-level if. This avoids the remove_last_pop() /
                // OpNull / outer OpPop overhead that expression-level if requires.
                if let Expression::If {
                    condition,
                    consequence,
                    alternative,
                } = expr
                {
                    self.compile_if_statement(condition, consequence, alternative.as_deref())?;
                    return Ok(());
                }

                // Record the start of this statement's bytecode so that peephole
                // optimizations below can verify they don't match across statement
                // boundaries.
                let stmt_start = self.instructions.len();

                // Check if this expression is a direct identifier assignment
                // (not nested inside &&, ||, ternary, etc.). Only direct assignments
                // produce guaranteed straight-line OpSet+OpGet bytecode that is safe
                // to elide. Nested assignments inside branching expressions (&&, ||,
                // ternary) produce the same pattern but in a conditional branch, making
                // elision unsafe.
                let is_direct_ident_assign = matches!(
                    expr,
                    Expression::Assign {
                        left,
                        ..
                    } if matches!(&**left, Expression::Identifier(_))
                );

                // Peephole: fuse `obj.prop = obj.prop + const` or `obj.prop += const`
                // (where obj is a global) into OpAddConstToGlobalProperty.
                // Collapses 4 dispatches into 1 with in-place numeric update.
                if let Some((global_idx, prop_const_idx, val_const_idx, cache_slot)) =
                    self.try_fuse_add_const_to_global_property(expr)
                {
                    self.emit(
                        Opcode::OpAddConstToGlobalProperty,
                        &[global_idx, prop_const_idx, val_const_idx, cache_slot],
                    );
                // Peephole: fuse `obj.dst = obj.src1 + obj.src2` where obj is a global
                // into OpAddGlobalPropsToGlobalProp. Collapses 4 dispatches into 1.
                } else if let Some((
                    global_idx,
                    s1_prop,
                    s1_cache,
                    s2_prop,
                    s2_cache,
                    d_prop,
                    d_cache,
                )) = self.try_fuse_add_global_props_to_global_prop(expr)
                {
                    self.emit(
                        Opcode::OpAddGlobalPropsToGlobalProp,
                        &[
                            global_idx, s1_prop, s1_cache, s2_prop, s2_cache, d_prop, d_cache,
                        ],
                    );
                // Peephole: fuse `ident = ident + const` in statement position into
                // OpIncrementLocal or OpIncrementGlobal.
                // The fused increment has no net stack effect (it reads and writes
                // the variable in place). In statement position the expression value
                // is discarded, so we elide the trailing OpGet + OpPop entirely.
                // If the value turns out to be needed (e.g. last statement in an
                // if-body), remove_last_pop() will restore the OpGet via
                // elided_trailing_get.
                } else if let Some((slot_idx, const_idx, fused_op)) = self.try_fuse_increment(expr)
                {
                    self.emit(fused_op, &[slot_idx, const_idx]);
                    let get_op = if fused_op == Opcode::OpIncrementLocal {
                        Opcode::OpGetLocal
                    } else {
                        Opcode::OpGetGlobal
                    };
                    self.elided_trailing_get = Some((get_op, slot_idx));
                } else if let Some((target_idx, source_idx)) = self.try_fuse_accumulate_global(expr)
                {
                    // Peephole: fuse `target = target + source` where both are globals
                    // into OpAccumulateGlobal. No stack effect; value is elided.
                    self.emit(Opcode::OpAccumulateGlobal, &[target_idx, source_idx]);
                    self.elided_trailing_get = Some((Opcode::OpGetGlobal, target_idx));
                } else {
                    self.compile_expression(expr)?;
                    // Peephole: fuse OpSetLocalProperty + OpPop -> OpSetLocalPropertyPop
                    // and OpSetGlobalProperty + OpPop -> OpSetGlobalPropertyPop.
                    // The fused variant does the set but skips pushing the result, eliminating
                    // the push+pop cycle on every property write in expression-statement position.
                    //
                    // Also: fuse OpSet{Local,Global} + OpGet{Local,Global} (same index) + OpPop
                    // into just OpSet{Local,Global}. Assignment expressions emit Set + Get to
                    // leave the value on the stack, but in statement position the value is
                    // discarded. This is restricted to direct identifier assignments
                    // (no branching expressions like &&, ||, ternary) to ensure the
                    // OpSet+OpGet pattern is in straight-line code.
                    let last = self.last_opcode;
                    if last == Some(Opcode::OpSetLocalProperty) {
                        self.instructions[self.last_position] = Opcode::OpSetLocalPropertyPop as u8;
                        self.last_opcode = Some(Opcode::OpSetLocalPropertyPop);
                        self.current_stack_depth -= 1;
                    } else if last == Some(Opcode::OpSetGlobalProperty) {
                        self.instructions[self.last_position] =
                            Opcode::OpSetGlobalPropertyPop as u8;
                        self.last_opcode = Some(Opcode::OpSetGlobalPropertyPop);
                        self.current_stack_depth -= 1;
                    } else if is_direct_ident_assign
                        && (last == Some(Opcode::OpGetLocal) || last == Some(Opcode::OpGetGlobal))
                        && self.try_elide_get_before_pop(stmt_start)
                    {
                        // Removed the trailing OpGet{Local,Global}; no OpPop needed.
                    } else {
                        self.emit(Opcode::OpPop, &[]);
                    }
                }
            }
            Statement::Block(statements) => {
                for s in statements {
                    self.compile_statement(s)?;
                }
            }
            Statement::While { condition, body } => {
                self.compile_while_statement(condition, body, None)?;
            }
            Statement::For {
                init,
                condition,
                update,
                body,
            } => {
                self.compile_for_statement(
                    init.as_deref(),
                    condition.as_ref(),
                    update.as_ref(),
                    body,
                    None,
                )?;
            }
            Statement::ForOf {
                binding,
                iterable,
                body,
            } => {
                self.compile_for_of_statement(binding, iterable, body, None)?;
            }
            Statement::ForIn {
                var_name,
                iterable,
                body,
            } => {
                self.compile_for_in_statement(var_name, iterable, body, None)?;
            }
            Statement::FunctionDecl {
                name,
                parameters,
                body,
                is_async,
                is_generator,
            } => {
                let function_expr = Expression::Function {
                    parameters: parameters.clone(),
                    body: body.clone(),
                    is_async: *is_async,
                    is_generator: *is_generator,
                };
                self.compile_expression(&function_expr)?;

                let idx = if self.is_function_scope {
                    if let Some(idx) = self.locals.get(name) {
                        *idx
                    } else {
                        let idx = self.next_local;
                        self.locals.insert(name.clone(), idx);
                        self.next_local += 1;
                        idx
                    }
                } else if let Some(idx) = self.globals.get(name) {
                    *idx
                } else {
                    if self.next_global as usize >= GLOBALS_SIZE {
                        return Err("global symbol table overflow".to_string());
                    }
                    let idx = self.next_global;
                    self.globals.insert(name.clone(), idx);
                    self.next_global += 1;
                    idx
                };

                if self.is_function_scope {
                    self.emit(Opcode::OpSetLocal, &[idx]);
                } else {
                    self.emit(Opcode::OpSetGlobal, &[idx]);
                }
            }
            Statement::ClassDecl {
                name,
                extends,
                members,
            } => {
                let class_name = name.as_deref().unwrap_or("");
                let extends_name = match extends.as_deref() {
                    Some(Expression::Identifier(s)) => Some(s.as_str()),
                    _ => None,
                };
                let class_obj = self.compile_class_literal(class_name, extends_name, members)?;
                if let Some(n) = name {
                    self.class_defs.insert(n.clone(), class_obj.clone());
                }
                let has_static_init = !class_obj.static_initializers.is_empty();
                let idx = self.add_constant(Object::Class(Box::new(class_obj)));
                self.emit(Opcode::OpConstant, &[idx]);
                if has_static_init {
                    self.emit(Opcode::OpInitClass, &[]);
                }

                if let Some(n) = name {
                    let sym = if self.is_function_scope {
                        if let Some(idx) = self.locals.get(n) {
                            *idx
                        } else {
                            let idx = self.next_local;
                            self.locals.insert(n.clone(), idx);
                            self.next_local += 1;
                            idx
                        }
                    } else if let Some(idx) = self.globals.get(n) {
                        *idx
                    } else {
                        if self.next_global as usize >= GLOBALS_SIZE {
                            return Err("global symbol table overflow".to_string());
                        }
                        let idx = self.next_global;
                        self.globals.insert(n.clone(), idx);
                        self.next_global += 1;
                        idx
                    };

                    if self.is_function_scope {
                        self.emit(Opcode::OpSetLocal, &[sym]);
                    } else {
                        self.emit(Opcode::OpSetGlobal, &[sym]);
                    }
                }
            }
            Statement::Throw { value } => {
                self.compile_throw_statement(value)?;
            }
            Statement::Try {
                try_block,
                catch_param,
                catch_block,
                finally_block,
            } => {
                self.compile_try_statement(
                    try_block,
                    catch_param.as_deref(),
                    catch_block.as_deref(),
                    finally_block.as_deref(),
                )?;
            }
            Statement::Labeled { label, statement } => match statement.as_ref() {
                Statement::While { condition, body } => {
                    self.compile_while_statement(condition, body, Some(label.as_str()))?
                }
                Statement::DoWhile { body, condition } => {
                    self.compile_do_while_statement(body, condition, Some(label.as_str()))?
                }
                Statement::Switch {
                    discriminant,
                    cases,
                } => self.compile_switch_statement(discriminant, cases, Some(label.as_str()))?,
                Statement::For {
                    init,
                    condition,
                    update,
                    body,
                } => self.compile_for_statement(
                    init.as_deref(),
                    condition.as_ref(),
                    update.as_ref(),
                    body,
                    Some(label.as_str()),
                )?,
                Statement::ForOf {
                    binding,
                    iterable,
                    body,
                } => {
                    self.compile_for_of_statement(binding, iterable, body, Some(label.as_str()))?
                }
                Statement::ForIn {
                    var_name,
                    iterable,
                    body,
                } => {
                    self.compile_for_in_statement(var_name, iterable, body, Some(label.as_str()))?
                }
                _ => {
                    return Err(
                        "only loop statements can be labeled in current Rust port".to_string()
                    )
                }
            },
            Statement::Break { label } => {
                let pos = self.emit(Opcode::OpJump, &[9999]);
                let loop_ctx = self.find_loop_ctx_mut(label.as_deref())?;
                loop_ctx.break_positions.push(pos);
            }
            Statement::Continue { label } => {
                let pos = self.emit(Opcode::OpJump, &[9999]);
                let loop_ctx = self.find_loop_ctx_mut(label.as_deref())?;
                loop_ctx.continue_positions.push(pos);
            }
            Statement::DoWhile { body, condition } => {
                self.compile_do_while_statement(body, condition, None)?;
            }
            Statement::Switch {
                discriminant,
                cases,
            } => {
                self.compile_switch_statement(discriminant, cases, None)?;
            }
            Statement::Debugger => {
                // No-op in sandboxed interpreter
            }
        }
        Ok(())
    }

    fn ensure_binding_slot(&mut self, name: &str) -> Result<u16, String> {
        if self.is_function_scope {
            if let Some(idx) = self.locals.get(name) {
                return Ok(*idx);
            }
            let idx = self.next_local;
            self.locals.insert(name.to_string(), idx);
            self.next_local += 1;
            Ok(idx)
        } else {
            if let Some(idx) = self.globals.get(name) {
                return Ok(*idx);
            }
            if self.next_global as usize >= GLOBALS_SIZE {
                return Err("global symbol table overflow".to_string());
            }
            let idx = self.next_global;
            self.globals.insert(name.to_string(), idx);
            self.next_global += 1;
            Ok(idx)
        }
    }

    fn ensure_global_slot(&mut self, name: &str) -> Result<u16, String> {
        if let Some(idx) = self.globals.get(name) {
            return Ok(*idx);
        }
        if self.next_global as usize >= GLOBALS_SIZE {
            return Err("global symbol table overflow".to_string());
        }
        let idx = self.next_global;
        self.globals.insert(name.to_string(), idx);
        self.next_global += 1;
        Ok(idx)
    }

    fn find_loop_ctx_mut(&mut self, label: Option<&str>) -> Result<&mut LoopContext, String> {
        match label {
            Some(target) => self
                .loop_stack
                .iter_mut()
                .rev()
                .find(|ctx| ctx.label.as_deref() == Some(target))
                .ok_or_else(|| format!("unknown loop label '{}'", target)),
            None => self
                .loop_stack
                .last_mut()
                .ok_or_else(|| "loop control outside loop".to_string()),
        }
    }

    fn compile_throw_statement(&mut self, value: &Expression) -> Result<(), String> {
        if self.try_stack.is_empty() {
            self.compile_expression(value)?;
            self.emit(Opcode::OpThrow, &[]);
            return Ok(());
        }

        self.compile_expression(value)?;
        let exception_temp = self
            .try_stack
            .last()
            .map(|ctx| ctx.exception_temp.clone())
            .unwrap_or_else(|| self.make_temp_name("exc_fallback"));
        self.assign_identifier_from_top(&exception_temp)?;

        let jump_pos = self.emit(Opcode::OpJump, &[9999]);
        if let Some(ctx) = self.try_stack.last_mut() {
            ctx.throw_jumps.push(jump_pos);
        }

        self.emit(Opcode::OpNull, &[]);
        Ok(())
    }

    fn compile_try_statement(
        &mut self,
        try_block: &[Statement],
        catch_param: Option<&str>,
        catch_block: Option<&[Statement]>,
        finally_block: Option<&[Statement]>,
    ) -> Result<(), String> {
        let exception_temp = self.make_temp_name("exc");
        self.compile_expression(&Expression::Null)?;
        self.assign_identifier_from_top(&exception_temp)?;
        self.try_stack.push(TryContext {
            exception_temp: exception_temp.clone(),
            throw_jumps: vec![],
        });

        for stmt in try_block {
            self.compile_statement(stmt)?;
        }
        self.remove_last_pop();

        let jump_after_try = self.emit(Opcode::OpJump, &[9999]);

        let ctx = self
            .try_stack
            .pop()
            .ok_or_else(|| "internal error: missing try context".to_string())?;

        let catch_start = self.instructions.len();
        for pos in ctx.throw_jumps {
            self.change_operand(pos, catch_start as u16);
        }

        if let Some(catch_stmts) = catch_block {
            if let Some(param) = catch_param {
                self.compile_expression(&Expression::Identifier(exception_temp.clone()))?;
                self.assign_identifier_from_top(param)?;
            }
            for stmt in catch_stmts {
                self.compile_statement(stmt)?;
            }
            self.remove_last_pop();
        }

        let finally_start = self.instructions.len();
        if let Some(finally_stmts) = finally_block {
            for stmt in finally_stmts {
                self.compile_statement(stmt)?;
            }
            self.remove_last_pop();
        }

        let end = self.instructions.len();
        if finally_block.is_some() {
            self.change_operand(jump_after_try, finally_start as u16);
        } else {
            self.change_operand(jump_after_try, end as u16);
        }

        self.emit(Opcode::OpNull, &[]);
        Ok(())
    }

    /// Compile an if-expression in statement position. Unlike the expression-level
    /// compilation in `Expression::If`, this does NOT leave a value on the stack.
    /// Each branch's body is compiled as statements (with their own pops). No
    /// `remove_last_pop()` is called, no `OpNull` is emitted for missing else,
    /// and no outer `OpPop` is needed. This saves 2 dispatches per iteration
    /// in hot loops like while+conditionals.
    fn compile_if_statement(
        &mut self,
        condition: &Expression,
        consequence: &[Statement],
        alternative: Option<&[Statement]>,
    ) -> Result<(), String> {
        // Try fused conditions in order of specificity:
        // 1. (ident % const) === const  -> OpModGlobalConstStrictEqConstJump (jump target at byte offset 6)
        // 2. ident < const / ident <= const -> OpTestLocal/GlobalLt/LeConstJump (jump target at byte offset 4)
        // 3. Fallback: compile_expression + OpJumpNotTruthy (jump target at byte offset 0)
        //
        // jump_target_byte_offset: byte offset from first operand byte to the jump target operand.
        let fused_mod_eq = self.try_fuse_mod_strict_eq_const(condition);
        let fused_cmp = if fused_mod_eq.is_none() {
            self.try_fuse_cmp_const(condition)
        } else {
            None
        };

        let (jump_pos, jump_target_byte_offset) =
            if let Some((global_idx, mod_const_idx, cmp_const_idx)) = fused_mod_eq {
                let pos = self.emit(
                    Opcode::OpModGlobalConstStrictEqConstJump,
                    &[global_idx, mod_const_idx, cmp_const_idx, 9999],
                );
                (pos, 6usize)
            } else if let Some((slot_idx, const_idx, fused_op)) = fused_cmp {
                let pos = self.emit(fused_op, &[slot_idx, const_idx, 9999]);
                (pos, 4usize)
            } else {
                self.compile_expression(condition)?;
                let pos = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
                (pos, 0usize)
            };

        // Compile consequence body as statements — each statement handles its
        // own stack cleanup. No value is left on the stack.
        for stmt in consequence {
            self.compile_statement(stmt)?;
        }

        if let Some(alt_block) = alternative {
            // There is an else branch — emit jump to skip it.
            let jump_over_pos = self.emit(Opcode::OpJump, &[9999]);
            let after_consequence = self.instructions.len();

            if jump_target_byte_offset > 0 {
                self.change_operand_at(jump_pos, jump_target_byte_offset, after_consequence as u16);
            } else {
                self.change_operand(jump_pos, after_consequence as u16);
            }

            // Compile alternative body as statements.
            for stmt in alt_block {
                self.compile_statement(stmt)?;
            }

            let after_alternative = self.instructions.len();
            self.change_operand(jump_over_pos, after_alternative as u16);
        } else {
            // No else branch — just patch the conditional jump to here.
            let after_consequence = self.instructions.len();
            if jump_target_byte_offset > 0 {
                self.change_operand_at(jump_pos, jump_target_byte_offset, after_consequence as u16);
            } else {
                self.change_operand(jump_pos, after_consequence as u16);
            }
        }

        Ok(())
    }

    fn compile_while_statement(
        &mut self,
        condition: &Expression,
        body: &[Statement],
        label: Option<&str>,
    ) -> Result<(), String> {
        let loop_start = self.instructions.len();

        // Try to fuse condition into a single test+jump opcode.
        let fused_cond = self.try_fuse_cmp_const(condition);

        let jump_not_truthy_pos = if let Some((slot_idx, const_idx, fused_op)) = fused_cond {
            self.emit(fused_op, &[slot_idx, const_idx, 9999])
        } else {
            self.compile_expression(condition)?;
            self.emit(Opcode::OpJumpNotTruthy, &[9999])
        };

        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: loop_start,
            break_positions: vec![],
            continue_positions: vec![],
        });

        for stmt in body {
            self.compile_statement(stmt)?;
        }

        // Peephole: if the last emitted opcode was OpIncrementGlobal, fuse it
        // with the backward jump into OpIncrementGlobalAndJump (saves 1 dispatch
        // per while-loop iteration when the loop body ends with `i = i + 1`).
        if self.last_opcode == Some(Opcode::OpIncrementGlobal) {
            // Replace OpIncrementGlobal(slot, const) with
            // OpIncrementGlobalAndJump(slot, const, loop_start).
            let inc_pos = self.last_position;
            self.instructions[inc_pos] = Opcode::OpIncrementGlobalAndJump as u8;
            // Append the jump target as a 3rd u16 operand.
            let target = loop_start as u16;
            self.instructions.push(((target >> 8) & 0xff) as u8);
            self.instructions.push((target & 0xff) as u8);
            self.last_opcode = Some(Opcode::OpIncrementGlobalAndJump);
            // Clear the elided trailing get since the increment is now fused
            // with the jump and we won't need to restore any get.
            self.elided_trailing_get = None;
        } else {
            self.emit(Opcode::OpJump, &[loop_start as u16]);
        }
        let loop_end = self.instructions.len();

        if fused_cond.is_some() {
            self.change_operand_at(jump_not_truthy_pos, 4, loop_end as u16);
        } else {
            self.change_operand(jump_not_truthy_pos, loop_end as u16);
        }

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.change_operand(pos, loop_end as u16);
            }
            for pos in ctx.continue_positions {
                self.change_operand(pos, ctx.continue_target as u16);
            }
        }

        self.emit(Opcode::OpNull, &[]);
        Ok(())
    }

    fn compile_do_while_statement(
        &mut self,
        body: &[Statement],
        condition: &Expression,
        label: Option<&str>,
    ) -> Result<(), String> {
        let loop_start = self.instructions.len();

        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: loop_start, // will be updated to condition start
            break_positions: vec![],
            continue_positions: vec![],
        });

        for stmt in body {
            self.compile_statement(stmt)?;
        }

        // continue jumps to the condition, not the body start
        let condition_start = self.instructions.len();
        if let Some(loop_ctx) = self.loop_stack.last_mut() {
            loop_ctx.continue_target = condition_start;
        }

        self.compile_expression(condition)?;
        // Jump back to body start if condition is truthy
        let exit_jump = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
        self.emit(Opcode::OpJump, &[loop_start as u16]);
        let loop_end = self.instructions.len();
        self.change_operand(exit_jump, loop_end as u16);

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.change_operand(pos, loop_end as u16);
            }
            for pos in ctx.continue_positions {
                self.change_operand(pos, ctx.continue_target as u16);
            }
        }

        self.emit(Opcode::OpNull, &[]);
        Ok(())
    }

    fn compile_switch_statement(
        &mut self,
        discriminant: &Expression,
        cases: &[crate::ast::SwitchCase],
        label: Option<&str>,
    ) -> Result<(), String> {
        // Switch uses the loop/break infrastructure for `break` statements.
        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: 0, // not used for switch
            break_positions: vec![],
            continue_positions: vec![],
        });

        // Compile discriminant and store in a temp variable
        let temp_name = format!("__fl_switch_{}", self.temp_counter);
        self.temp_counter += 1;
        self.compile_expression(discriminant)?;
        let disc_idx = self.ensure_binding_slot(&temp_name)?;
        if self.is_function_scope {
            self.emit(Opcode::OpSetLocal, &[disc_idx]);
        } else {
            self.emit(Opcode::OpSetGlobal, &[disc_idx]);
        }

        // Phase 1: Emit all case test comparisons and jumps to bodies
        let mut case_body_positions: Vec<usize> = Vec::new();
        let mut default_body_idx: Option<usize> = None;

        for (i, case) in cases.iter().enumerate() {
            if let Some(test) = &case.test {
                // Load discriminant
                if self.is_function_scope {
                    self.emit(Opcode::OpGetLocal, &[disc_idx]);
                } else {
                    self.emit(Opcode::OpGetGlobal, &[disc_idx]);
                }
                self.compile_expression(test)?;
                self.emit(Opcode::OpStrictEqual, &[]);
                // Jump to body if equal — we use JumpNotTruthy to skip a body-jump
                let jump_pos = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
                let body_jump = self.emit(Opcode::OpJump, &[9999]);
                let after_body_jump = self.instructions.len();
                self.change_operand(jump_pos, after_body_jump as u16);
                case_body_positions.push(body_jump);
            } else {
                default_body_idx = Some(i);
            }
        }

        // If no case matched, jump to default (if any) or to end
        let default_or_end_jump = self.emit(Opcode::OpJump, &[9999]);

        // Phase 2: Emit case bodies with fall-through
        let mut body_starts: Vec<usize> = Vec::new();
        for case in cases {
            body_starts.push(self.instructions.len());
            for stmt in &case.consequent {
                self.compile_statement(stmt)?;
            }
        }
        let switch_end = self.instructions.len();

        // Phase 3: Patch all jumps
        let mut case_jump_idx = 0;
        for (i, case) in cases.iter().enumerate() {
            if case.test.is_some() {
                self.change_operand(case_body_positions[case_jump_idx], body_starts[i] as u16);
                case_jump_idx += 1;
            }
        }

        // Patch default/end jump
        if let Some(def_idx) = default_body_idx {
            self.change_operand(default_or_end_jump, body_starts[def_idx] as u16);
        } else {
            self.change_operand(default_or_end_jump, switch_end as u16);
        }

        // Patch break jumps
        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.change_operand(pos, switch_end as u16);
            }
        }

        Ok(())
    }

    fn compile_for_statement(
        &mut self,
        init: Option<&Statement>,
        condition: Option<&Expression>,
        update: Option<&Expression>,
        body: &[Statement],
        label: Option<&str>,
    ) -> Result<(), String> {
        if let Some(init_stmt) = init {
            self.compile_statement(init_stmt)?;
            self.remove_last_pop();
        }

        let loop_start = self.instructions.len();

        // Try to fuse condition into a single test+jump opcode.
        // Pattern: local < const  => OpTestLocalLtConstJump
        //          local <= const => OpTestLocalLeConstJump
        let fused_cond = condition.and_then(|cond| self.try_fuse_cmp_const(cond));

        let jump_not_truthy_pos = if let Some((slot_idx, const_idx, fused_op)) = fused_cond {
            // Emit fused test+jump with placeholder target (operand index 2 = jump target).
            self.emit(fused_op, &[slot_idx, const_idx, 9999])
        } else {
            if let Some(cond) = condition {
                self.compile_expression(cond)?;
            } else {
                self.emit(Opcode::OpTrue, &[]);
            }
            self.emit(Opcode::OpJumpNotTruthy, &[9999])
        };

        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: loop_start,
            break_positions: vec![],
            continue_positions: vec![],
        });

        for stmt in body {
            self.compile_statement(stmt)?;
        }

        let update_start = self.instructions.len();
        if let Some(loop_ctx) = self.loop_stack.last_mut() {
            loop_ctx.continue_target = update_start;
        }

        // Try to fuse update into OpIncrementLocal or OpIncrementGlobal.
        // Pattern: ident = ident + const (where both identifiers are the same slot)
        let fused_upd = update.and_then(|upd| self.try_fuse_increment(upd));

        if let Some((slot_idx, const_idx, fused_op)) = fused_upd {
            if fused_op == Opcode::OpIncrementGlobal {
                // Fuse increment + backward jump into a single dispatch.
                self.emit(
                    Opcode::OpIncrementGlobalAndJump,
                    &[slot_idx, const_idx, loop_start as u16],
                );
            } else {
                self.emit(fused_op, &[slot_idx, const_idx]);
                self.emit(Opcode::OpJump, &[loop_start as u16]);
            }
        } else if let Some(upd) = update {
            self.compile_expression(upd)?;
            self.emit(Opcode::OpPop, &[]);
            self.emit(Opcode::OpJump, &[loop_start as u16]);
        } else {
            self.emit(Opcode::OpJump, &[loop_start as u16]);
        }

        let loop_end = self.instructions.len();
        // Patch the jump target: for fused opcodes the target is the 3rd operand (offset +5),
        // for OpJumpNotTruthy the target is the 1st operand (offset +1).
        if fused_cond.is_some() {
            // Fused opcode: OpTestLocal{Lt,Le}ConstJump [local:2, const:2, target:2]
            // The jump target is at op_pos + 5 (bytes 5..6).
            self.change_operand_at(jump_not_truthy_pos, 4, loop_end as u16);
        } else {
            self.change_operand(jump_not_truthy_pos, loop_end as u16);
        }

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.change_operand(pos, loop_end as u16);
            }
            for pos in ctx.continue_positions {
                self.change_operand(pos, ctx.continue_target as u16);
            }
        }

        self.emit(Opcode::OpNull, &[]);
        Ok(())
    }

    fn compile_for_of_statement(
        &mut self,
        binding: &ForBinding,
        iterable: &Expression,
        body: &[Statement],
        label: Option<&str>,
    ) -> Result<(), String> {
        let iter_tmp = self.make_temp_name("iter");
        let idx_tmp = self.make_temp_name("i");

        self.compile_expression(iterable)?;
        self.compile_statement(&Statement::Let {
            name: iter_tmp.clone(),
            value: Expression::Null,
            kind: VariableKind::Let,
        })?;
        self.remove_last_pop();
        self.assign_identifier_from_top(&iter_tmp)?;

        self.compile_expression(&Expression::Integer(0))?;
        self.compile_statement(&Statement::Let {
            name: idx_tmp.clone(),
            value: Expression::Null,
            kind: VariableKind::Let,
        })?;
        self.remove_last_pop();
        self.assign_identifier_from_top(&idx_tmp)?;

        let loop_start = self.instructions.len();

        let cond_expr = Expression::Infix {
            left: Box::new(Expression::Identifier(idx_tmp.clone())),
            operator: "<".to_string(),
            right: Box::new(Expression::Index {
                left: Box::new(Expression::Identifier(iter_tmp.clone())),
                index: Box::new(Expression::String("length".to_string())),
            }),
        };
        self.compile_expression(&cond_expr)?;
        let jump_not_truthy_pos = self.emit(Opcode::OpJumpNotTruthy, &[9999]);

        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: loop_start,
            break_positions: vec![],
            continue_positions: vec![],
        });

        let item_expr = Expression::Index {
            left: Box::new(Expression::Identifier(iter_tmp.clone())),
            index: Box::new(Expression::Identifier(idx_tmp.clone())),
        };
        self.compile_expression(&item_expr)?;
        match binding {
            ForBinding::Identifier(var_name) => {
                self.compile_statement(&Statement::Let {
                    name: var_name.to_string(),
                    value: Expression::Null,
                    kind: VariableKind::Let,
                })?;
                self.remove_last_pop();
                self.assign_identifier_from_top(var_name)?;
            }
            ForBinding::Pattern(pattern) => {
                self.assign_pattern_from_top(pattern)?;
            }
        }

        for stmt in body {
            self.compile_statement(stmt)?;
        }

        let update_start = self.instructions.len();
        if let Some(loop_ctx) = self.loop_stack.last_mut() {
            loop_ctx.continue_target = update_start;
        }

        let update_expr = Expression::Assign {
            left: Box::new(Expression::Identifier(idx_tmp.clone())),
            operator: "+=".to_string(),
            right: Box::new(Expression::Integer(1)),
        };
        self.compile_expression(&update_expr)?;
        self.emit(Opcode::OpPop, &[]);

        self.emit(Opcode::OpJump, &[loop_start as u16]);

        let loop_end = self.instructions.len();
        self.change_operand(jump_not_truthy_pos, loop_end as u16);

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.change_operand(pos, loop_end as u16);
            }
            for pos in ctx.continue_positions {
                self.change_operand(pos, ctx.continue_target as u16);
            }
        }

        self.emit(Opcode::OpNull, &[]);
        Ok(())
    }

    fn compile_for_in_statement(
        &mut self,
        var_name: &str,
        iterable: &Expression,
        body: &[Statement],
        label: Option<&str>,
    ) -> Result<(), String> {
        let keys_tmp = self.make_temp_name("keys");
        let idx_tmp = self.make_temp_name("ki");

        self.compile_expression(iterable)?;
        self.emit(Opcode::OpGetKeysIterator, &[]);
        self.assign_identifier_from_top(&keys_tmp)?;

        self.compile_expression(&Expression::Integer(0))?;
        self.assign_identifier_from_top(&idx_tmp)?;

        let loop_start = self.instructions.len();
        let cond_expr = Expression::Infix {
            left: Box::new(Expression::Identifier(idx_tmp.clone())),
            operator: "<".to_string(),
            right: Box::new(Expression::Index {
                left: Box::new(Expression::Identifier(keys_tmp.clone())),
                index: Box::new(Expression::String("length".to_string())),
            }),
        };
        self.compile_expression(&cond_expr)?;
        let jump_not_truthy_pos = self.emit(Opcode::OpJumpNotTruthy, &[9999]);

        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: loop_start,
            break_positions: vec![],
            continue_positions: vec![],
        });

        let key_expr = Expression::Index {
            left: Box::new(Expression::Identifier(keys_tmp.clone())),
            index: Box::new(Expression::Identifier(idx_tmp.clone())),
        };
        self.compile_expression(&key_expr)?;
        self.assign_identifier_from_top(var_name)?;

        for stmt in body {
            self.compile_statement(stmt)?;
        }
        self.remove_last_pop();

        let update_start = self.instructions.len();
        if let Some(loop_ctx) = self.loop_stack.last_mut() {
            loop_ctx.continue_target = update_start;
        }

        let update_expr = Expression::Assign {
            left: Box::new(Expression::Identifier(idx_tmp.clone())),
            operator: "+=".to_string(),
            right: Box::new(Expression::Integer(1)),
        };
        self.compile_expression(&update_expr)?;
        self.emit(Opcode::OpPop, &[]);

        self.emit(Opcode::OpJump, &[loop_start as u16]);

        let loop_end = self.instructions.len();
        self.change_operand(jump_not_truthy_pos, loop_end as u16);

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.change_operand(pos, loop_end as u16);
            }
            for pos in ctx.continue_positions {
                self.change_operand(pos, ctx.continue_target as u16);
            }
        }

        self.emit(Opcode::OpNull, &[]);
        Ok(())
    }

    fn make_temp_name(&mut self, prefix: &str) -> String {
        let name = format!("__fl_{}_{}", prefix, self.temp_counter);
        self.temp_counter += 1;
        name
    }

    fn assign_identifier_from_top(&mut self, name: &str) -> Result<(), String> {
        let idx = if self.is_function_scope {
            if let Some(idx) = self.locals.get(name) {
                *idx
            } else {
                let idx = self.next_local;
                self.locals.insert(name.to_string(), idx);
                self.next_local += 1;
                idx
            }
        } else if let Some(idx) = self.globals.get(name) {
            *idx
        } else {
            if self.next_global as usize >= GLOBALS_SIZE {
                return Err("global symbol table overflow".to_string());
            }
            let idx = self.next_global;
            self.globals.insert(name.to_string(), idx);
            self.next_global += 1;
            idx
        };

        if self.is_function_scope {
            self.emit(Opcode::OpSetLocal, &[idx]);
        } else {
            self.emit(Opcode::OpSetGlobal, &[idx]);
        }
        Ok(())
    }

    /// Collects all binding names from a pattern into the given set.
    fn collect_pattern_names(pattern: &BindingPattern, out: &mut FxHashSet<String>) {
        match pattern {
            BindingPattern::Array(items) => {
                for item in items {
                    match item {
                        ArrayBindingItem::Binding { target, .. } => match target {
                            BindingTarget::Identifier(name) => {
                                out.insert(name.clone());
                            }
                            BindingTarget::Pattern(inner) => {
                                Self::collect_pattern_names(inner, out);
                            }
                        },
                        ArrayBindingItem::Rest { name } => {
                            out.insert(name.clone());
                        }
                        ArrayBindingItem::Hole => {}
                    }
                }
            }
            BindingPattern::Object(items) => {
                for item in items {
                    match &item.target {
                        BindingTarget::Identifier(name) => {
                            out.insert(name.clone());
                        }
                        BindingTarget::Pattern(inner) => {
                            Self::collect_pattern_names(inner, out);
                        }
                    }
                }
            }
        }
    }

    /// Scans statement list for `var` declarations and pre-allocates their
    /// binding slots with `undefined`. This implements JS var hoisting.
    fn hoist_var_declarations(&mut self, stmts: &[Statement]) -> Result<(), String> {
        let mut var_names = Vec::new();
        Self::collect_var_names(stmts, &mut var_names);
        for name in var_names {
            // ensure_binding_slot is idempotent — if slot already exists, it reuses it
            self.ensure_binding_slot(&name)?;
        }
        Ok(())
    }

    /// Recursively collects all `var`-declared names from a statement list.
    /// Does NOT descend into function bodies (var is function-scoped, not
    /// hoisted across function boundaries).
    fn collect_var_names(stmts: &[Statement], out: &mut Vec<String>) {
        for stmt in stmts {
            match stmt {
                Statement::Let {
                    name,
                    kind: VariableKind::Var,
                    ..
                } => {
                    out.push(name.clone());
                }
                Statement::LetPattern {
                    pattern,
                    kind: VariableKind::Var,
                    ..
                } => {
                    let mut names = FxHashSet::default();
                    Self::collect_pattern_names(pattern, &mut names);
                    out.extend(names);
                }
                // Recurse into blocks, loops, if-bodies, etc.
                Statement::Block(body)
                | Statement::While { body, .. }
                | Statement::DoWhile { body, .. } => {
                    Self::collect_var_names(body, out);
                }
                Statement::For { init, body, .. } => {
                    if let Some(init_stmt) = init {
                        Self::collect_var_names(std::slice::from_ref(init_stmt), out);
                    }
                    Self::collect_var_names(body, out);
                }
                Statement::ForOf { body, .. } | Statement::ForIn { body, .. } => {
                    Self::collect_var_names(body, out);
                }
                Statement::Labeled { statement, .. } => {
                    Self::collect_var_names(std::slice::from_ref(statement), out);
                }
                Statement::Try {
                    try_block,
                    catch_block,
                    finally_block,
                    ..
                } => {
                    Self::collect_var_names(try_block, out);
                    if let Some(cb) = catch_block {
                        Self::collect_var_names(cb, out);
                    }
                    if let Some(fb) = finally_block {
                        Self::collect_var_names(fb, out);
                    }
                }
                Statement::Switch { cases, .. } => {
                    for case in cases {
                        Self::collect_var_names(&case.consequent, out);
                    }
                }
                Statement::Expression(Expression::If {
                    consequence,
                    alternative,
                    ..
                }) => {
                    Self::collect_var_names(consequence, out);
                    if let Some(alt) = alternative {
                        Self::collect_var_names(alt, out);
                    }
                }
                // FunctionDecl, ClassDecl, Return, etc. — don't recurse
                // (var is not hoisted out of function bodies)
                _ => {}
            }
        }
    }

    fn assign_pattern_from_top(&mut self, pattern: &BindingPattern) -> Result<(), String> {
        let source_tmp = self.make_temp_name("destr");
        self.assign_identifier_from_top(&source_tmp)?;

        match pattern {
            BindingPattern::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    match item {
                        ArrayBindingItem::Hole => continue,
                        ArrayBindingItem::Binding {
                            target,
                            default_value,
                        } => {
                            self.compile_expression(&Expression::Identifier(source_tmp.clone()))?;
                            self.compile_expression(&Expression::Integer(i as i64))?;
                            self.emit(Opcode::OpIndex, &[]);
                            self.apply_destructuring_default(default_value.as_ref())?;
                            self.assign_binding_target_from_top(target)?;
                        }
                        ArrayBindingItem::Rest { name } => {
                            self.compile_array_rest_from(&source_tmp, i as i64)?;
                            self.assign_identifier_from_top(name)?;
                        }
                    }
                }
            }
            BindingPattern::Object(pairs) => {
                let excluded_keys: Vec<Expression> = pairs
                    .iter()
                    .filter(|p| !p.is_rest)
                    .map(|p| p.key.clone())
                    .collect();

                for ObjectBindingItem {
                    key,
                    target,
                    default_value,
                    is_rest,
                } in pairs
                {
                    if *is_rest {
                        self.compile_object_rest_from(&source_tmp, &excluded_keys)?;
                        let BindingTarget::Identifier(name) = target else {
                            return Err("object rest target must be identifier".to_string());
                        };
                        self.assign_identifier_from_top(name)?;
                        continue;
                    }
                    self.compile_expression(&Expression::Identifier(source_tmp.clone()))?;
                    self.compile_expression(key)?;
                    self.emit(Opcode::OpIndex, &[]);
                    self.apply_destructuring_default(default_value.as_ref())?;
                    self.assign_binding_target_from_top(target)?;
                }
            }
        }

        Ok(())
    }

    fn apply_destructuring_default(
        &mut self,
        default_value: Option<&Expression>,
    ) -> Result<(), String> {
        let Some(default_expr) = default_value else {
            return Ok(());
        };

        let temp_name = self.make_temp_name("destr_val");
        self.assign_identifier_from_top(&temp_name)?;

        self.compile_expression(&Expression::Identifier(temp_name.clone()))?;
        self.emit(Opcode::OpTypeof, &[]);
        let undef_idx = self.add_constant_string(Rc::from("undefined"));
        self.emit(Opcode::OpConstant, &[undef_idx]);
        self.emit(Opcode::OpEqual, &[]);

        let jump_not_undef = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
        self.compile_expression(default_expr)?;
        self.assign_identifier_from_top(&temp_name)?;

        let after_default = self.instructions.len();
        self.change_operand(jump_not_undef, after_default as u16);
        self.compile_expression(&Expression::Identifier(temp_name))?;
        Ok(())
    }

    fn assign_binding_target_from_top(&mut self, target: &BindingTarget) -> Result<(), String> {
        match target {
            BindingTarget::Identifier(name) => self.assign_identifier_from_top(name),
            BindingTarget::Pattern(pattern) => self.assign_pattern_from_top(pattern),
        }
    }

    fn compile_array_rest_from(
        &mut self,
        source_tmp: &str,
        start_index: i64,
    ) -> Result<(), String> {
        self.compile_expression(&Expression::Identifier(source_tmp.to_string()))?;
        self.compile_expression(&Expression::Integer(start_index))?;
        self.emit(Opcode::OpIteratorRest, &[]);
        Ok(())
    }

    fn compile_object_rest_from(
        &mut self,
        source_tmp: &str,
        excluded_keys: &[Expression],
    ) -> Result<(), String> {
        self.compile_expression(&Expression::Identifier(source_tmp.to_string()))?;
        for key in excluded_keys {
            self.compile_expression(key)?;
        }
        self.emit(Opcode::OpObjectRest, &[excluded_keys.len() as u16]);
        Ok(())
    }

    fn compile_expression(&mut self, expr: &Expression) -> Result<(), String> {
        match expr {
            Expression::Integer(v) => {
                let idx = self.add_constant_int(*v);
                self.emit(Opcode::OpConstant, &[idx]);
            }
            Expression::Float(v) => {
                let idx = self.add_constant_float(*v);
                self.emit(Opcode::OpConstant, &[idx]);
            }
            Expression::String(v) => {
                let idx = self.add_constant_string(Rc::from(v.as_str()));
                self.emit(Opcode::OpConstant, &[idx]);
            }
            Expression::RegExp { pattern, flags } => {
                let idx = self.add_constant(Object::RegExp(Box::new(RegExpObject {
                    pattern: pattern.clone(),
                    flags: flags.clone(),
                })));
                self.emit(Opcode::OpConstant, &[idx]);
            }
            Expression::Boolean(v) => {
                self.emit(if *v { Opcode::OpTrue } else { Opcode::OpFalse }, &[]);
            }
            Expression::Null => {
                self.emit(Opcode::OpNull, &[]);
            }
            Expression::Identifier(name) => {
                if let Some(idx) = self.locals.get(name) {
                    self.emit(Opcode::OpGetLocal, &[*idx]);
                } else {
                    if let Some(idx) = self.globals.get(name) {
                        self.emit(Opcode::OpGetGlobal, &[*idx]);
                    } else if let Some(builtin_obj) = Self::builtin_global_object(name) {
                        let idx = self.add_constant(builtin_obj);
                        self.emit(Opcode::OpConstant, &[idx]);
                    } else if self.is_function_scope {
                        let idx = self.ensure_global_slot(name)?;
                        self.emit(Opcode::OpGetGlobal, &[idx]);
                    } else {
                        return Err(format!("undefined identifier {}", name));
                    }
                }
            }
            Expression::Array(items) => {
                let has_spread = items
                    .iter()
                    .any(|item| matches!(item, Expression::Spread { .. }));
                if has_spread {
                    self.emit(Opcode::OpArray, &[0]);
                    for item in items {
                        match item {
                            Expression::Spread { value } => {
                                self.compile_expression(value)?;
                                self.emit(Opcode::OpAppendSpread, &[]);
                            }
                            _ => {
                                self.compile_expression(item)?;
                                self.emit(Opcode::OpAppendElement, &[]);
                            }
                        }
                    }
                } else {
                    for item in items {
                        self.compile_expression(item)?;
                    }
                    self.emit(Opcode::OpArray, &[items.len() as u16]);
                }
            }
            Expression::Hash(entries) => {
                // Count only KeyValue and Spread entries for OpHash (methods/getters/setters
                // are compiled as functions and attached via OpDefineAccessor afterwards).
                let mut kv_count = 0usize;
                let mut accessors: Vec<(&Expression, &[Statement], Option<&str>, bool)> = vec![];

                for entry in entries {
                    match entry {
                        HashEntry::KeyValue { key, value } => {
                            self.compile_expression(key)?;
                            self.compile_expression(value)?;
                            kv_count += 1;
                        }
                        HashEntry::Spread(expr) => {
                            // Sentinel key for spread
                            self.compile_expression(&Expression::String(
                                "__fl_rest__".to_string(),
                            ))?;
                            self.compile_expression(expr)?;
                            kv_count += 1;
                        }
                        HashEntry::Method {
                            key,
                            parameters,
                            body,
                            is_async,
                            is_generator,
                        } => {
                            // Compile method as a key-value pair where value is the function
                            self.compile_expression(key)?;
                            let func = self.compile_function_literal(
                                parameters,
                                body,
                                false,
                                *is_async,
                                *is_generator,
                            )?;
                            let idx = self.add_constant(Object::CompiledFunction(Box::new(func)));
                            self.emit(Opcode::OpConstant, &[idx]);
                            kv_count += 1;
                        }
                        HashEntry::Getter { key, body } => {
                            accessors.push((key, body, None, true));
                        }
                        HashEntry::Setter {
                            key,
                            parameter,
                            body,
                        } => {
                            accessors.push((key, body, Some(parameter.as_str()), false));
                        }
                    }
                }

                self.emit(Opcode::OpHash, &[(kv_count * 2) as u16]);

                // Now emit DefineAccessor for each getter/setter
                for (key, body, param, is_getter) in accessors {
                    let prop_name = match key {
                        Expression::String(s) => s.clone(),
                        Expression::Identifier(s) => s.clone(),
                        _ => {
                            return Err(
                                "computed getter/setter keys not yet supported in stack compiler"
                                    .to_string(),
                            );
                        }
                    };
                    let params = if is_getter {
                        vec![]
                    } else {
                        vec![param.unwrap().to_string()]
                    };
                    let func = self.compile_function_literal(&params, body, true, false, false)?;
                    let func_idx = self.add_constant(Object::CompiledFunction(Box::new(func)));
                    self.emit(Opcode::OpConstant, &[func_idx]);
                    let name_idx = self.add_constant(Object::String(prop_name.into()));
                    let kind = if is_getter { 0u16 } else { 1u16 };
                    self.emit(Opcode::OpDefineAccessor, &[name_idx, kind]);
                }
            }
            Expression::Prefix { operator, right } => {
                self.compile_expression(right)?;
                match operator.as_str() {
                    "!" => self.emit(Opcode::OpBang, &[]),
                    "-" => self.emit(Opcode::OpMinus, &[]),
                    "+" => self.emit(Opcode::OpUnaryPlus, &[]),
                    "~" => {
                        let idx = self.add_constant_int(-1);
                        self.emit(Opcode::OpConstant, &[idx]);
                        self.emit(Opcode::OpBitwiseXor, &[])
                    }
                    _ => return Err(format!("unsupported prefix operator {}", operator)),
                };
            }
            Expression::Typeof { value } => {
                if let Expression::Identifier(name) = &**value {
                    if self.locals.contains_key(name)
                        || self.globals.contains_key(name)
                        || Self::builtin_global_object(name).is_some()
                        || self.is_function_scope
                    {
                        self.compile_expression(value)?;
                    } else {
                        self.emit(Opcode::OpUndefined, &[]);
                    }
                } else {
                    self.compile_expression(value)?;
                }
                self.emit(Opcode::OpTypeof, &[]);
            }
            Expression::Void { value } => {
                self.compile_expression(value)?;
                self.emit(Opcode::OpPop, &[]);
                self.emit(Opcode::OpUndefined, &[]);
            }
            Expression::Delete { value } => {
                self.compile_delete_expression(value)?;
            }
            Expression::Infix {
                left,
                operator,
                right,
            } => {
                if operator == "," {
                    self.compile_expression(left)?;
                    self.emit(Opcode::OpPop, &[]);
                    self.compile_expression(right)?;
                    return Ok(());
                }

                if operator == "&&" || operator == "||" || operator == "??" {
                    self.compile_logical_expression(left, operator, right)?;
                    return Ok(());
                }

                self.compile_expression(left)?;
                self.compile_expression(right)?;
                let op = match operator.as_str() {
                    "+" => Opcode::OpAdd,
                    "-" => Opcode::OpSub,
                    "*" => Opcode::OpMul,
                    "/" => Opcode::OpDiv,
                    "%" => Opcode::OpMod,
                    "**" => Opcode::OpPow,
                    "==" => Opcode::OpEqual,
                    "!=" => Opcode::OpNotEqual,
                    "===" => Opcode::OpStrictEqual,
                    "!==" => Opcode::OpStrictNotEqual,
                    ">" => Opcode::OpGreaterThan,
                    "<" => Opcode::OpLessThan,
                    ">=" => Opcode::OpGreaterOrEqual,
                    "<=" => Opcode::OpLessOrEqual,
                    "&" => Opcode::OpBitwiseAnd,
                    "|" => Opcode::OpBitwiseOr,
                    "^" => Opcode::OpBitwiseXor,
                    "<<" => Opcode::OpLeftShift,
                    ">>" => Opcode::OpRightShift,
                    ">>>" => Opcode::OpUnsignedRightShift,
                    "in" => Opcode::OpIn,
                    "instanceof" => Opcode::OpInstanceof,
                    _ => return Err(format!("unsupported infix operator {}", operator)),
                };
                self.emit(op, &[]);
            }
            Expression::If {
                condition,
                consequence,
                alternative,
            } => {
                // Try fused conditions in order of specificity:
                // 1. (ident % const) === const  -> OpModGlobalConstStrictEqConstJump
                // 2. ident < const / ident <= const -> fused test+jump
                // 3. Fallback: compile_expression + OpJumpNotTruthy
                let fused_mod_eq = self.try_fuse_mod_strict_eq_const(condition);
                let fused_cmp = if fused_mod_eq.is_none() {
                    self.try_fuse_cmp_const(condition)
                } else {
                    None
                };

                let (cond_jump_pos, jump_target_byte_offset) =
                    if let Some((global_idx, mod_const_idx, cmp_const_idx)) = fused_mod_eq {
                        let pos = self.emit(
                            Opcode::OpModGlobalConstStrictEqConstJump,
                            &[global_idx, mod_const_idx, cmp_const_idx, 9999],
                        );
                        (pos, 6usize)
                    } else if let Some((slot_idx, const_idx, fused_op)) = fused_cmp {
                        let pos = self.emit(fused_op, &[slot_idx, const_idx, 9999]);
                        (pos, 4usize)
                    } else {
                        self.compile_expression(condition)?;
                        let pos = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
                        (pos, 0usize)
                    };

                for stmt in consequence {
                    self.compile_statement(stmt)?;
                }
                self.remove_last_pop();

                let jump_pos = self.emit(Opcode::OpJump, &[9999]);
                let after_consequence = self.instructions.len();

                if jump_target_byte_offset > 0 {
                    self.change_operand_at(
                        cond_jump_pos,
                        jump_target_byte_offset,
                        after_consequence as u16,
                    );
                } else {
                    self.change_operand(cond_jump_pos, after_consequence as u16);
                }

                if let Some(alt_block) = alternative {
                    for stmt in alt_block {
                        self.compile_statement(stmt)?;
                    }
                    self.remove_last_pop();
                } else {
                    self.emit(Opcode::OpNull, &[]);
                }

                let after_alternative = self.instructions.len();
                self.change_operand(jump_pos, after_alternative as u16);
            }
            Expression::Function {
                parameters,
                body,
                is_async,
                is_generator,
            } => {
                let function_obj = self.compile_function_literal(
                    parameters,
                    body,
                    true,
                    *is_async,
                    *is_generator,
                )?;
                let idx = self.add_constant(Object::CompiledFunction(Box::new(function_obj)));
                self.emit(Opcode::OpConstant, &[idx]);
            }
            Expression::This => {
                self.emit(Opcode::OpGetLocal, &[0]);
            }
            Expression::Super => {
                self.emit(Opcode::OpSuper, &[]);
            }
            Expression::NewTarget => {
                self.emit(Opcode::OpNewTarget, &[]);
            }
            Expression::ImportMeta => {
                self.emit(Opcode::OpImportMeta, &[]);
            }
            Expression::Await { value } => {
                self.compile_expression(value)?;
                self.emit(Opcode::OpAwait, &[]);
            }
            Expression::Yield { value, delegate: _ } => {
                // Compile the value expression, then emit OpYield.
                // OpYield pops the yielded value, suspends, and on resume
                // pushes the value passed to .next().
                self.compile_expression(value)?;
                self.emit(Opcode::OpYield, &[]);
            }
            Expression::New { callee, arguments } => {
                self.compile_expression(callee)?;
                if self.arguments_have_spread(arguments) {
                    self.compile_arguments_array(arguments)?;
                    self.emit(Opcode::OpNewSpread, &[]);
                } else {
                    for arg in arguments {
                        self.compile_expression(arg)?;
                    }
                    self.emit(Opcode::OpNew, &[arguments.len() as u16]);
                }
            }
            Expression::Call {
                function,
                arguments,
            } => {
                if self.arguments_have_spread(arguments) {
                    self.compile_expression(function)?;
                    self.compile_arguments_array(arguments)?;
                    self.emit(Opcode::OpCallSpread, &[]);
                } else if let Some(global_idx) = self.try_resolve_global_function(function) {
                    // Fused OpCallGlobal: skip pushing callee to stack entirely.
                    // Avoids Box allocation for CompiledFunctionObject clone.
                    for arg in arguments {
                        self.compile_expression(arg)?;
                    }
                    self.emit(Opcode::OpCallGlobal, &[global_idx, arguments.len() as u16]);
                } else {
                    self.compile_expression(function)?;
                    for arg in arguments {
                        self.compile_expression(arg)?;
                    }
                    self.emit(Opcode::OpCall, &[arguments.len() as u16]);
                }
            }
            Expression::OptionalIndex { left, index } => {
                self.compile_optional_index_expression(left, index)?;
            }
            Expression::OptionalCall {
                function,
                arguments,
            } => {
                self.compile_optional_call_expression(function, arguments)?;
            }
            Expression::Assign {
                left,
                operator,
                right,
            } => {
                self.compile_assignment_expression(left, operator, right)?;
            }
            Expression::Update {
                target,
                operator,
                prefix,
            } => {
                let assign_op = match operator.as_str() {
                    "++" => "+=",
                    "--" => "-=",
                    _ => return Err(format!("unsupported update operator {}", operator)),
                };

                if *prefix {
                    self.compile_assignment_expression(target, assign_op, &Expression::Integer(1))?;
                } else {
                    self.compile_expression(target)?;
                    let temp_name = self.make_temp_name("postfix_old");
                    self.assign_identifier_from_top(&temp_name)?;

                    self.compile_assignment_expression(target, assign_op, &Expression::Integer(1))?;
                    self.emit(Opcode::OpPop, &[]);
                    self.compile_expression(&Expression::Identifier(temp_name))?;
                }
            }
            Expression::Spread { .. } => {
                return Err("spread expression is only valid in array literals".to_string());
            }
            Expression::Class {
                name,
                extends,
                members,
            } => {
                let class_name = name.as_deref().unwrap_or("");
                let extends_name = match extends.as_deref() {
                    Some(Expression::Identifier(s)) => Some(s.as_str()),
                    _ => None,
                };
                let class_obj = self.compile_class_literal(class_name, extends_name, members)?;
                if let Some(n) = name {
                    self.class_defs.insert(n.clone(), class_obj.clone());
                }
                let has_static_init = !class_obj.static_initializers.is_empty();
                let idx = self.add_constant(Object::Class(Box::new(class_obj)));
                self.emit(Opcode::OpConstant, &[idx]);
                if has_static_init {
                    self.emit(Opcode::OpInitClass, &[]);
                }
            }
            Expression::Index { left, index } => {
                if let Expression::String(prop) = &**index {
                    if let Expression::Identifier(name) = &**left {
                        if let Some(&local_idx) = self.locals.get(name) {
                            let const_idx = self.add_constant_string(Rc::from(prop.as_str()));
                            let cache_slot = self.next_cache_slot;
                            self.next_cache_slot += 1;
                            self.emit(
                                Opcode::OpGetLocalProperty,
                                &[local_idx, const_idx, cache_slot],
                            );
                            return Ok(());
                        }
                        if let Some(&global_idx) = self.globals.get(name) {
                            let const_idx = self.add_constant_string(Rc::from(prop.as_str()));
                            let cache_slot = self.next_cache_slot;
                            self.next_cache_slot += 1;
                            self.emit(
                                Opcode::OpGetGlobalProperty,
                                &[global_idx, const_idx, cache_slot],
                            );
                            return Ok(());
                        }
                    }

                    self.compile_expression(left)?;
                    let const_idx = self.add_constant_string(Rc::from(prop.as_str()));
                    let cache_slot = self.next_cache_slot;
                    self.next_cache_slot += 1;
                    self.emit(Opcode::OpGetProperty, &[const_idx, cache_slot]);
                } else {
                    self.compile_expression(left)?;
                    self.compile_expression(index)?;
                    self.emit(Opcode::OpIndex, &[]);
                }
            }
        }
        Ok(())
    }

    fn compile_logical_expression(
        &mut self,
        left: &Expression,
        operator: &str,
        right: &Expression,
    ) -> Result<(), String> {
        let temp_name = self.make_temp_name("logic");

        self.compile_expression(left)?;
        self.assign_identifier_from_top(&temp_name)?;

        match operator {
            "||" => {
                self.compile_expression(&Expression::Identifier(temp_name.clone()))?;
                let eval_right_pos = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
                self.compile_expression(&Expression::Identifier(temp_name.clone()))?;
                let end_pos = self.emit(Opcode::OpJump, &[9999]);

                let right_start = self.instructions.len();
                self.change_operand(eval_right_pos, right_start as u16);
                self.compile_expression(right)?;

                let end = self.instructions.len();
                self.change_operand(end_pos, end as u16);
            }
            "&&" => {
                self.compile_expression(&Expression::Identifier(temp_name.clone()))?;
                let use_left_pos = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
                self.compile_expression(right)?;
                let end_pos = self.emit(Opcode::OpJump, &[9999]);

                let use_left = self.instructions.len();
                self.change_operand(use_left_pos, use_left as u16);
                self.compile_expression(&Expression::Identifier(temp_name.clone()))?;

                let end = self.instructions.len();
                self.change_operand(end_pos, end as u16);
            }
            "??" => {
                self.compile_expression(&Expression::Identifier(temp_name.clone()))?;
                self.emit(Opcode::OpIsNullish, &[]);
                let use_left_pos = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
                self.compile_expression(right)?;
                let end_pos = self.emit(Opcode::OpJump, &[9999]);

                let use_left = self.instructions.len();
                self.change_operand(use_left_pos, use_left as u16);
                self.compile_expression(&Expression::Identifier(temp_name.clone()))?;

                let end = self.instructions.len();
                self.change_operand(end_pos, end as u16);
            }
            _ => return Err(format!("unsupported logical operator {}", operator)),
        }

        Ok(())
    }

    fn compile_optional_index_expression(
        &mut self,
        left: &Expression,
        index: &Expression,
    ) -> Result<(), String> {
        let temp_name = self.make_temp_name("opt");

        self.compile_expression(left)?;
        self.assign_identifier_from_top(&temp_name)?;

        self.compile_expression(&Expression::Identifier(temp_name.clone()))?;
        self.emit(Opcode::OpIsNullish, &[]);
        let nullish_pos = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
        self.emit(Opcode::OpUndefined, &[]);
        let end_pos = self.emit(Opcode::OpJump, &[9999]);

        let access_pos = self.instructions.len();
        self.change_operand(nullish_pos, access_pos as u16);
        self.compile_expression(&Expression::Identifier(temp_name))?;
        self.compile_expression(index)?;
        self.emit(Opcode::OpIndex, &[]);

        let end = self.instructions.len();
        self.change_operand(end_pos, end as u16);
        Ok(())
    }

    fn compile_optional_call_expression(
        &mut self,
        function: &Expression,
        arguments: &[Expression],
    ) -> Result<(), String> {
        let temp_name = self.make_temp_name("optcall");

        self.compile_expression(function)?;
        self.assign_identifier_from_top(&temp_name)?;

        self.compile_expression(&Expression::Identifier(temp_name.clone()))?;
        self.emit(Opcode::OpIsNullish, &[]);
        let nullish_pos = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
        self.emit(Opcode::OpUndefined, &[]);
        let end_pos = self.emit(Opcode::OpJump, &[9999]);

        let call_pos = self.instructions.len();
        self.change_operand(nullish_pos, call_pos as u16);
        self.compile_expression(&Expression::Identifier(temp_name))?;
        if self.arguments_have_spread(arguments) {
            self.compile_arguments_array(arguments)?;
            self.emit(Opcode::OpCallSpread, &[]);
        } else {
            for arg in arguments {
                self.compile_expression(arg)?;
            }
            self.emit(Opcode::OpCall, &[arguments.len() as u16]);
        }

        let end = self.instructions.len();
        self.change_operand(end_pos, end as u16);
        Ok(())
    }

    fn arguments_have_spread(&self, arguments: &[Expression]) -> bool {
        arguments
            .iter()
            .any(|arg| matches!(arg, Expression::Spread { .. }))
    }

    fn compile_arguments_array(&mut self, arguments: &[Expression]) -> Result<(), String> {
        self.emit(Opcode::OpArray, &[0]);
        for arg in arguments {
            match arg {
                Expression::Spread { value } => {
                    self.compile_expression(value)?;
                    self.emit(Opcode::OpAppendSpread, &[]);
                }
                _ => {
                    self.compile_expression(arg)?;
                    self.emit(Opcode::OpAppendElement, &[]);
                }
            }
        }
        Ok(())
    }

    fn hash_key_string(name: &str) -> HashKey {
        HashKey::from_string(name)
    }

    /// Public accessor for register compiler to reuse builtins.
    pub fn builtin_global_object_static(name: &str) -> Option<Object> {
        Self::builtin_global_object(name)
    }

    fn builtin_global_object(name: &str) -> Option<Object> {
        match name {
            "Math" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("abs"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathAbs,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("floor"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathFloor,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("ceil"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathCeil,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("round"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathRound,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("min"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathMin,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("max"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathMax,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("pow"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathPow,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("sqrt"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathSqrt,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("trunc"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathTrunc,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("sign"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathSign,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("random"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathRandom,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("log"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathLog,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("log2"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathLog2,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("cbrt"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathCbrt,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("sin"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathSin,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("cos"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathCos,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("tan"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathTan,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("exp"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathExp,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("log10"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathLog10,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("atan2"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathAtan2,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("hypot"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathHypot,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("imul"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathImul,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("clz32"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathClz32,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("fround"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathFround,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("PI"),
                    Object::Float(std::f64::consts::PI),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("E"),
                    Object::Float(std::f64::consts::E),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("LN2"),
                    Object::Float(std::f64::consts::LN_2),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("LN10"),
                    Object::Float(std::f64::consts::LN_10),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("SQRT2"),
                    Object::Float(std::f64::consts::SQRT_2),
                );
                Some(make_hash(hash))
            }
            "String" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::StringCtor,
                receiver: None,
            }))),
            "parseInt" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::ParseInt,
                receiver: None,
            }))),
            "parseFloat" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::ParseFloat,
                receiver: None,
            }))),
            "isNaN" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::IsNaN,
                receiver: None,
            }))),
            "isFinite" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::IsFinite,
                receiver: None,
            }))),
            "encodeURIComponent" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::EncodeURIComponent,
                receiver: None,
            }))),
            "decodeURIComponent" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::DecodeURIComponent,
                receiver: None,
            }))),
            "encodeURI" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::EncodeURI,
                receiver: None,
            }))),
            "decodeURI" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::DecodeURI,
                receiver: None,
            }))),
            "Infinity" => Some(Object::Float(f64::INFINITY)),
            "NaN" => Some(Object::Float(f64::NAN)),
            "undefined" => Some(Object::Undefined),
            "Number" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::NumberCtor,
                receiver: None,
            }))),
            "Array" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("from"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ArrayFrom,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }
            "RegExp" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::RegExpCtor,
                receiver: None,
            }))),
            "Map" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::MapCtor,
                receiver: None,
            }))),
            "Set" => Some(Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::SetCtor,
                receiver: None,
            }))),
            "globalThis" => {
                let mut hash = HashObject::default();

                let mut math = HashObject::default();
                math.insert_pair_obj(
                    Self::hash_key_string("abs"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathAbs,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("floor"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathFloor,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("ceil"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathCeil,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("round"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathRound,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("min"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathMin,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("max"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathMax,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("pow"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathPow,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("sqrt"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathSqrt,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("trunc"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathTrunc,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("sign"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathSign,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("random"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathRandom,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("log"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathLog,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("log2"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathLog2,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("cbrt"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathCbrt,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("sin"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathSin,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("cos"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathCos,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("tan"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathTan,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("exp"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathExp,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("log10"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathLog10,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("atan2"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathAtan2,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("hypot"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathHypot,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("imul"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathImul,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("clz32"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathClz32,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("fround"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MathFround,
                        receiver: None,
                    })),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("PI"),
                    Object::Float(std::f64::consts::PI),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("E"),
                    Object::Float(std::f64::consts::E),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("LN2"),
                    Object::Float(std::f64::consts::LN_2),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("LN10"),
                    Object::Float(std::f64::consts::LN_10),
                );
                math.insert_pair_obj(
                    Self::hash_key_string("SQRT2"),
                    Object::Float(std::f64::consts::SQRT_2),
                );

                hash.insert_pair_obj(Self::hash_key_string("Math"), make_hash(math));
                hash.insert_pair_obj(
                    Self::hash_key_string("String"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::StringCtor,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("parseInt"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ParseInt,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("parseFloat"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ParseFloat,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("isNaN"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::IsNaN,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("isFinite"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::IsFinite,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("encodeURIComponent"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::EncodeURIComponent,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("decodeURIComponent"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DecodeURIComponent,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("encodeURI"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::EncodeURI,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("decodeURI"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DecodeURI,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("Infinity"),
                    Object::Float(f64::INFINITY),
                );
                hash.insert_pair_obj(Self::hash_key_string("NaN"), Object::Float(f64::NAN));
                hash.insert_pair_obj(Self::hash_key_string("undefined"), Object::Undefined);

                hash.insert_pair_obj(
                    Self::hash_key_string("Number"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::NumberCtor,
                        receiver: None,
                    })),
                );
                let mut array_ns = HashObject::default();
                array_ns.insert_pair_obj(
                    Self::hash_key_string("from"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ArrayFrom,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(Self::hash_key_string("Array"), make_hash(array_ns));
                hash.insert_pair_obj(
                    Self::hash_key_string("RegExp"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::RegExpCtor,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("Map"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::MapCtor,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("Set"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::SetCtor,
                        receiver: None,
                    })),
                );

                let mut json_ns = HashObject::default();
                json_ns.insert_pair_obj(
                    Self::hash_key_string("stringify"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::JsonStringify,
                        receiver: None,
                    })),
                );
                json_ns.insert_pair_obj(
                    Self::hash_key_string("parse"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::JsonParse,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(Self::hash_key_string("JSON"), make_hash(json_ns));

                let mut promise_ns = HashObject::default();
                promise_ns.insert_pair_obj(
                    Self::hash_key_string("resolve"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::PromiseResolve,
                        receiver: None,
                    })),
                );
                promise_ns.insert_pair_obj(
                    Self::hash_key_string("reject"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::PromiseReject,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(Self::hash_key_string("Promise"), make_hash(promise_ns));

                let mut object_ns = HashObject::default();
                object_ns.insert_pair_obj(
                    Self::hash_key_string("keys"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectKeys,
                        receiver: None,
                    })),
                );
                object_ns.insert_pair_obj(
                    Self::hash_key_string("values"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectValues,
                        receiver: None,
                    })),
                );
                object_ns.insert_pair_obj(
                    Self::hash_key_string("entries"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectEntries,
                        receiver: None,
                    })),
                );
                object_ns.insert_pair_obj(
                    Self::hash_key_string("fromEntries"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectFromEntries,
                        receiver: None,
                    })),
                );
                object_ns.insert_pair_obj(
                    Self::hash_key_string("hasOwn"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectHasOwn,
                        receiver: None,
                    })),
                );
                object_ns.insert_pair_obj(
                    Self::hash_key_string("is"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectIs,
                        receiver: None,
                    })),
                );
                object_ns.insert_pair_obj(
                    Self::hash_key_string("assign"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectAssign,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(Self::hash_key_string("Object"), make_hash(object_ns));

                Some(make_hash(hash))
            }
            "Object" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("keys"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectKeys,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("values"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectValues,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("entries"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectEntries,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("fromEntries"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectFromEntries,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("hasOwn"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectHasOwn,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("is"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectIs,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("assign"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::ObjectAssign,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }
            "JSON" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("stringify"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::JsonStringify,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("parse"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::JsonParse,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }
            "Promise" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("resolve"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::PromiseResolve,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("reject"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::PromiseReject,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }
            "Date" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("now"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DateNow,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }
            "localStorage" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("getItem"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LocalStorageGetItem,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("setItem"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LocalStorageSetItem,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("removeItem"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LocalStorageRemoveItem,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("clear"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LocalStorageClear,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }
            "db" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("query"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DbQuery,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("create"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DbCreate,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("update"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DbUpdate,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("delete"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DbDelete,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("hardDelete"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DbHardDelete,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("get"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DbGet,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("startSync"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DbStartSync,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("stopSync"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DbStopSync,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getSyncStatus"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DbGetSyncStatus,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getSavedSyncRoom"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DbGetSavedSyncRoom,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }
            // ── draw ──
            "draw" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("rect"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawRect,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("roundedRect"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawRoundedRect,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("circle"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawCircle,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("ellipse"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawEllipse,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("line"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawLine,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("path"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawPath,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("text"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawText,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("image"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawImage,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("linearGradient"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawLinearGradient,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("radialGradient"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawRadialGradient,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("shadow"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawShadow,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("pushClip"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawPushClip,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("popClip"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawPopClip,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("pushTransform"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawPushTransform,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("popTransform"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawPopTransform,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("pushOpacity"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawPushOpacity,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("popOpacity"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawPopOpacity,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("arc"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawArc,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("measureText"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawMeasureText,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getViewportWidth"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawGetViewportWidth,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getViewportHeight"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::DrawGetViewportHeight,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }

            // ── layout ──
            "layout" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("createNode"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LayoutCreateNode,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("updateStyle"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LayoutUpdateStyle,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("setChildren"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LayoutSetChildren,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("computeLayout"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LayoutComputeLayout,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getLayout"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LayoutGetLayout,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("removeNode"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LayoutRemoveNode,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("clear"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::LayoutClear,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }

            // ── input ──
            "input" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("getMouseX"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputGetMouseX,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getMouseY"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputGetMouseY,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("isMouseDown"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputIsMouseDown,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("isMousePressed"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputIsMousePressed,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("isMouseReleased"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputIsMouseReleased,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getScrollY"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputGetScrollY,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("setCursor"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputSetCursor,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getTextInput"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputGetTextInput,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("isBackspacePressed"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputIsBackspacePressed,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("isEscapePressed"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputIsEscapePressed,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("requestRedraw"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputRequestRedraw,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getElapsedSecs"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputGetElapsedSecs,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getPageElapsedSecs"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputGetPageElapsedSecs,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getDeltaTime"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputGetDeltaTime,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("getFocusedInput"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputGetFocusedInput,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("setFocusedInput"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputSetFocusedInput,
                        receiver: None,
                    })),
                );
                hash.insert_pair_obj(
                    Self::hash_key_string("isKeyDown"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::InputIsKeyDown,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }

            "host" => {
                let mut hash = HashObject::default();
                hash.insert_pair_obj(
                    Self::hash_key_string("call"),
                    Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                        function: BuiltinFunction::HostCall,
                        receiver: None,
                    })),
                );
                Some(make_hash(hash))
            }

            _ => None,
        }
    }

    fn compile_function_literal(
        &mut self,
        parameters: &[String],
        body: &[Statement],
        takes_this: bool,
        is_async: bool,
        is_generator: bool,
    ) -> Result<CompiledFunctionObject, String> {
        let mut normalized_params: Vec<String> = vec![];
        let mut rest_parameter_index: Option<usize> = None;
        for (i, p) in parameters.iter().enumerate() {
            if let Some(rest_name) = p.strip_prefix("...") {
                if rest_name.is_empty() {
                    return Err("invalid rest parameter name".to_string());
                }
                if i + 1 != parameters.len() {
                    return Err("rest parameter must be last".to_string());
                }
                rest_parameter_index = Some(i);
                normalized_params.push(rest_name.to_string());
            } else {
                if rest_parameter_index.is_some() {
                    return Err("rest parameter must be last".to_string());
                }
                normalized_params.push(p.clone());
            }
        }

        let mut effective_params = vec![];
        if takes_this {
            effective_params.push("this".to_string());
        }
        effective_params.extend(normalized_params.iter().cloned());
        let mut fn_compiler =
            Compiler::new_function_scope(self.globals.clone(), self.next_global, &effective_params);

        fn_compiler.hoist_var_declarations(body)?;

        for stmt in body {
            fn_compiler.compile_statement(stmt)?;
        }

        if fn_compiler.last_opcode == Some(Opcode::OpPop) {
            fn_compiler.remove_last_pop();
            fn_compiler.emit(Opcode::OpReturnValue, &[]);
        }

        if fn_compiler.last_opcode != Some(Opcode::OpReturnValue)
            && fn_compiler.last_opcode != Some(Opcode::OpReturn)
        {
            fn_compiler.emit(Opcode::OpReturn, &[]);
        }
        fn_compiler.emit(Opcode::OpHalt, &[]);

        if fn_compiler.next_global > self.next_global {
            self.next_global = fn_compiler.next_global;
        }
        for (name, idx) in &fn_compiler.globals {
            self.globals.entry(name.clone()).or_insert(*idx);
        }

        Ok(CompiledFunctionObject {
            instructions: Rc::new(fn_compiler.instructions),
            constants: Rc::new(fn_compiler.constants),
            num_locals: fn_compiler.next_local as usize,
            num_parameters: rest_parameter_index.unwrap_or(normalized_params.len()),
            rest_parameter_index,
            takes_this,
            is_async,
            is_generator,
            num_cache_slots: fn_compiler.next_cache_slot,
            max_stack_depth: fn_compiler.max_stack_depth.max(0) as u16,
            register_count: 0,
            inline_cache: Rc::new(VmCell::new(vec![
                (0, 0);
                fn_compiler.next_cache_slot as usize
            ])),
        })
    }

    fn compile_class_literal(
        &mut self,
        name: &str,
        extends: Option<&str>,
        members: &[ClassMember],
    ) -> Result<ClassObject, String> {
        let mut class_obj = ClassObject {
            name: name.to_string(),
            parent_chain: vec![],
            constructor: None,
            methods: FxHashMap::default(),
            static_methods: FxHashMap::default(),
            getters: FxHashMap::default(),
            setters: FxHashMap::default(),
            super_methods: FxHashMap::default(),
            super_getters: FxHashMap::default(),
            super_setters: FxHashMap::default(),
            field_initializers: vec![],
            static_initializers: vec![],
            static_fields: FxHashMap::default(),
        };

        if let Some(parent_name) = extends {
            if let Some(parent) = self.class_defs.get(parent_name) {
                class_obj.parent_chain.push(parent_name.to_string());
                class_obj.parent_chain.extend(parent.parent_chain.clone());
                class_obj.super_methods = parent.methods.clone();
                if let Some(parent_ctor) = parent.constructor.clone() {
                    class_obj
                        .super_methods
                        .insert("constructor".to_string(), parent_ctor);
                }
                class_obj.super_getters = parent.getters.clone();
                class_obj.super_setters = parent.setters.clone();
                for (k, v) in &parent.methods {
                    class_obj.methods.entry(k.clone()).or_insert(v.clone());
                }
                for (k, v) in &parent.getters {
                    class_obj.getters.entry(k.clone()).or_insert(v.clone());
                }
                for (k, v) in &parent.setters {
                    class_obj.setters.entry(k.clone()).or_insert(v.clone());
                }
                // Inherit parent instance field initializers
                class_obj
                    .field_initializers
                    .extend(parent.field_initializers.clone());
            }
        }

        for member in members {
            match member {
                ClassMember::Method(method) => {
                    let compiled = self.compile_function_literal(
                        &method.parameters,
                        &method.body,
                        true,
                        false,
                        false,
                    )?;
                    if method.name == "constructor" {
                        class_obj.constructor = Some(compiled);
                    } else if method.is_static {
                        class_obj
                            .static_methods
                            .insert(method.name.clone(), compiled);
                    } else if method.is_getter {
                        class_obj.getters.insert(method.name.clone(), compiled);
                    } else if method.is_setter {
                        class_obj.setters.insert(method.name.clone(), compiled);
                    } else {
                        class_obj.methods.insert(method.name.clone(), compiled);
                    }
                }
                ClassMember::Field {
                    name: field_name,
                    initializer,
                    is_static,
                } => {
                    // Compile the initializer as a zero-arg, takes_this function
                    // that returns the field value.
                    let init_body = if let Some(expr) = initializer {
                        vec![Statement::Return {
                            value: expr.clone(),
                        }]
                    } else {
                        vec![Statement::Return {
                            value: Expression::Identifier("undefined".to_string()),
                        }]
                    };
                    let compiled =
                        self.compile_function_literal(&[], &init_body, true, false, false)?;
                    if *is_static {
                        class_obj
                            .static_initializers
                            .push(StaticInitializer::Field {
                                name: field_name.clone(),
                                thunk: compiled,
                            });
                    } else {
                        class_obj
                            .field_initializers
                            .push((field_name.clone(), compiled));
                    }
                }
                ClassMember::StaticBlock { body } => {
                    // Static blocks: compile as a zero-arg takes_this function.
                    // VM evaluates during class initialization.
                    let compiled = self.compile_function_literal(&[], body, true, false, false)?;
                    class_obj
                        .static_initializers
                        .push(StaticInitializer::Block { thunk: compiled });
                }
            }
        }

        Ok(class_obj)
    }

    fn compile_assignment_expression(
        &mut self,
        left: &Expression,
        operator: &str,
        right: &Expression,
    ) -> Result<(), String> {
        match left {
            Expression::Identifier(name) => {
                if self.const_bindings.contains(name) {
                    return Err(format!("Assignment to constant variable '{}'", name));
                }
                if operator == "=" {
                    self.compile_expression(right)?;
                    if let Some(idx) = self.locals.get(name).copied() {
                        self.emit(Opcode::OpSetLocal, &[idx]);
                        self.emit(Opcode::OpGetLocal, &[idx]);
                    } else {
                        let idx = if let Some(idx) = self.globals.get(name) {
                            *idx
                        } else {
                            if self.next_global as usize >= GLOBALS_SIZE {
                                return Err("global symbol table overflow".to_string());
                            }
                            let idx = self.next_global;
                            self.globals.insert(name.clone(), idx);
                            self.next_global += 1;
                            idx
                        };
                        self.emit(Opcode::OpSetGlobal, &[idx]);
                        self.emit(Opcode::OpGetGlobal, &[idx]);
                    }
                    return Ok(());
                }

                let base_op = match operator {
                    "+=" => Opcode::OpAdd,
                    "-=" => Opcode::OpSub,
                    "*=" => Opcode::OpMul,
                    "/=" => Opcode::OpDiv,
                    "%=" => Opcode::OpMod,
                    "**=" => Opcode::OpPow,
                    "&=" => Opcode::OpBitwiseAnd,
                    "|=" => Opcode::OpBitwiseOr,
                    "^=" => Opcode::OpBitwiseXor,
                    "<<=" => Opcode::OpLeftShift,
                    ">>=" => Opcode::OpRightShift,
                    ">>>=" => Opcode::OpUnsignedRightShift,
                    "&&=" | "||=" | "??=" => Opcode::OpAdd,
                    _ => return Err(format!("unsupported assignment operator {}", operator)),
                };

                if operator == "&&=" || operator == "||=" || operator == "??=" {
                    return self.compile_logical_assignment_identifier(name, operator, right);
                }

                if let Some(idx) = self.locals.get(name).copied() {
                    self.emit(Opcode::OpGetLocal, &[idx]);
                    self.compile_expression(right)?;
                    self.emit(base_op, &[]);
                    self.emit(Opcode::OpSetLocal, &[idx]);
                    self.emit(Opcode::OpGetLocal, &[idx]);
                } else {
                    let idx = if let Some(idx) = self.globals.get(name) {
                        *idx
                    } else {
                        if self.next_global as usize >= GLOBALS_SIZE {
                            return Err("global symbol table overflow".to_string());
                        }
                        let idx = self.next_global;
                        self.globals.insert(name.clone(), idx);
                        self.next_global += 1;
                        idx
                    };
                    self.emit(Opcode::OpGetGlobal, &[idx]);
                    self.compile_expression(right)?;
                    self.emit(base_op, &[]);
                    self.emit(Opcode::OpSetGlobal, &[idx]);
                    self.emit(Opcode::OpGetGlobal, &[idx]);
                }
                Ok(())
            }
            Expression::Index {
                left: object_expr,
                index,
            } => {
                let base_op = match operator {
                    "=" => None,
                    "+=" => Some(Opcode::OpAdd),
                    "-=" => Some(Opcode::OpSub),
                    "*=" => Some(Opcode::OpMul),
                    "/=" => Some(Opcode::OpDiv),
                    "%=" => Some(Opcode::OpMod),
                    "**=" => Some(Opcode::OpPow),
                    "&=" => Some(Opcode::OpBitwiseAnd),
                    "|=" => Some(Opcode::OpBitwiseOr),
                    "^=" => Some(Opcode::OpBitwiseXor),
                    "<<=" => Some(Opcode::OpLeftShift),
                    ">>=" => Some(Opcode::OpRightShift),
                    ">>>=" => Some(Opcode::OpUnsignedRightShift),
                    _ => {
                        return Err(format!(
                            "unsupported assignment operator {} for index target",
                            operator
                        ))
                    }
                };
                match &**object_expr {
                    Expression::Identifier(name) => {
                        let is_local = self.locals.contains_key(name);
                        let idx_symbol = if is_local {
                            *self.locals.get(name).expect("local exists")
                        } else if let Some(idx) = self.globals.get(name) {
                            *idx
                        } else {
                            if self.next_global as usize >= GLOBALS_SIZE {
                                return Err("global symbol table overflow".to_string());
                            }
                            let idx = self.next_global;
                            self.globals.insert(name.clone(), idx);
                            self.next_global += 1;
                            idx
                        };

                        if operator == "=" {
                            if let Expression::String(prop) = &**index {
                                let const_idx = self.add_constant_string(Rc::from(prop.as_str()));
                                let cache_slot = self.next_cache_slot;
                                self.next_cache_slot += 1;
                                self.compile_expression(right)?;
                                if is_local {
                                    self.emit(
                                        Opcode::OpSetLocalProperty,
                                        &[idx_symbol, const_idx, cache_slot],
                                    );
                                } else {
                                    self.emit(
                                        Opcode::OpSetGlobalProperty,
                                        &[idx_symbol, const_idx, cache_slot],
                                    );
                                }
                                return Ok(());
                            }
                        }

                        if let Some(op) = base_op {
                            if is_local {
                                self.emit(Opcode::OpGetLocal, &[idx_symbol]);
                            } else {
                                self.emit(Opcode::OpGetGlobal, &[idx_symbol]);
                            }
                            self.compile_expression(index)?;
                            self.emit(Opcode::OpIndex, &[]);
                            self.compile_expression(right)?;
                            self.emit(op, &[]);
                        } else {
                            self.compile_expression(right)?;
                        }

                        let temp_value = self.make_temp_name("idx_assign");
                        self.assign_identifier_from_top(&temp_value)?;

                        if is_local {
                            self.emit(Opcode::OpGetLocal, &[idx_symbol]);
                        } else {
                            self.emit(Opcode::OpGetGlobal, &[idx_symbol]);
                        }
                        self.compile_expression(index)?;
                        self.compile_expression(&Expression::Identifier(temp_value.clone()))?;
                        self.emit(Opcode::OpSetIndex, &[]);

                        if is_local {
                            self.emit(Opcode::OpSetLocal, &[idx_symbol]);
                            self.emit(Opcode::OpGetLocal, &[idx_symbol]);
                        } else {
                            self.emit(Opcode::OpSetGlobal, &[idx_symbol]);
                            self.emit(Opcode::OpGetGlobal, &[idx_symbol]);
                        }

                        self.compile_expression(index)?;
                        self.emit(Opcode::OpIndex, &[]);
                    }
                    Expression::This => {
                        if let Some(op) = base_op {
                            self.emit(Opcode::OpGetLocal, &[0]);
                            self.compile_expression(index)?;
                            self.emit(Opcode::OpIndex, &[]);
                            self.compile_expression(right)?;
                            self.emit(op, &[]);
                        } else {
                            self.compile_expression(right)?;
                        }

                        let temp_value = self.make_temp_name("this_assign");
                        self.assign_identifier_from_top(&temp_value)?;

                        self.emit(Opcode::OpGetLocal, &[0]);
                        self.compile_expression(index)?;
                        self.compile_expression(&Expression::Identifier(temp_value))?;
                        self.emit(Opcode::OpSetIndex, &[]);
                        self.emit(Opcode::OpSetLocal, &[0]);
                        self.emit(Opcode::OpGetLocal, &[0]);
                        self.compile_expression(index)?;
                        self.emit(Opcode::OpIndex, &[]);
                    }
                    _ => {
                        self.compile_expression(object_expr)?;
                        self.compile_expression(index)?;
                        self.compile_expression(right)?;
                        self.emit(Opcode::OpSetIndex, &[]);
                    }
                }
                Ok(())
            }
            Expression::Array(items) => {
                if operator != "=" {
                    return Err("only '=' supported for array destructuring assignment".to_string());
                }

                self.compile_expression(right)?;
                let source_tmp = self.make_temp_name("arr_assign");
                self.assign_identifier_from_top(&source_tmp)?;

                for (i, item) in items.iter().enumerate() {
                    match item {
                        Expression::Spread { value } => {
                            if i + 1 != items.len() {
                                return Err(
                                    "array rest element in assignment must be last".to_string(),
                                );
                            }
                            match &**value {
                                Expression::Identifier(name) => {
                                    self.compile_array_rest_from(&source_tmp, i as i64)?;
                                    self.assign_identifier_from_top(name)?;
                                }
                                Expression::Assign { .. } => {
                                    return Err(
                                        "rest element in array binding cannot have default"
                                            .to_string(),
                                    )
                                }
                                _ => {
                                    return Err(
                                        "array rest assignment requires identifier target"
                                            .to_string(),
                                    )
                                }
                            }
                            continue;
                        }
                        Expression::Identifier(name) => {
                            self.compile_expression(&Expression::Identifier(source_tmp.clone()))?;
                            self.compile_expression(&Expression::Integer(i as i64))?;
                            self.emit(Opcode::OpIndex, &[]);
                            self.assign_identifier_from_top(name)?;
                        }
                        Expression::Assign {
                            left,
                            operator,
                            right,
                        } if operator == "=" => match &**left {
                            Expression::Identifier(name) => {
                                self.compile_expression(&Expression::Identifier(source_tmp.clone()))?;
                                self.compile_expression(&Expression::Integer(i as i64))?;
                                self.emit(Opcode::OpIndex, &[]);
                                self.apply_destructuring_default(Some(right.as_ref()))?;
                                self.assign_identifier_from_top(name)?;
                            }
                            Expression::Array(_) | Expression::Hash(_) => {
                                self.compile_expression(&Expression::Identifier(source_tmp.clone()))?;
                                self.compile_expression(&Expression::Integer(i as i64))?;
                                self.emit(Opcode::OpIndex, &[]);
                                self.apply_destructuring_default(Some(right.as_ref()))?;
                                let nested_tmp = self.make_temp_name("arr_nested");
                                self.assign_identifier_from_top(&nested_tmp)?;
                                self.compile_assignment_expression(
                                    left,
                                    "=",
                                    &Expression::Identifier(nested_tmp),
                                )?;
                                self.emit(Opcode::OpPop, &[]);
                            }
                            _ => {
                                return Err(
                                    "array destructuring default supports identifier or nested pattern targets only"
                                        .to_string(),
                                )
                            }
                        },
                        Expression::Array(_) | Expression::Hash(_) => {
                            self.compile_expression(&Expression::Identifier(source_tmp.clone()))?;
                            self.compile_expression(&Expression::Integer(i as i64))?;
                            self.emit(Opcode::OpIndex, &[]);
                            let nested_tmp = self.make_temp_name("arr_nested");
                            self.assign_identifier_from_top(&nested_tmp)?;
                            self.compile_assignment_expression(
                                item,
                                "=",
                                &Expression::Identifier(nested_tmp),
                            )?;
                            self.emit(Opcode::OpPop, &[]);
                        }
                        _ => {
                            return Err(
                                "array destructuring assignment supports identifier or nested pattern targets only"
                                    .to_string(),
                            )
                        }
                    }
                }

                self.compile_expression(&Expression::Identifier(source_tmp))?;
                Ok(())
            }
            Expression::Hash(pairs) => {
                if operator != "=" {
                    return Err(
                        "only '=' supported for object destructuring assignment".to_string()
                    );
                }

                self.compile_expression(right)?;
                let source_tmp = self.make_temp_name("obj_assign");
                self.assign_identifier_from_top(&source_tmp)?;

                let excluded_keys: Vec<Expression> = pairs
                    .iter()
                    .filter_map(|entry| match entry {
                        HashEntry::Spread(_) => None,
                        HashEntry::KeyValue { key, .. } => Some(key.clone()),
                        HashEntry::Method { key, .. } => Some(key.clone()),
                        HashEntry::Getter { key, .. } => Some(key.clone()),
                        HashEntry::Setter { key, .. } => Some(key.clone()),
                    })
                    .collect();

                for entry in pairs {
                    match entry {
                        HashEntry::Spread(target_expr) => {
                            let name = match target_expr {
                                Expression::Identifier(name) => name,
                                _ => return Err(
                                    "object rest destructuring assignment requires identifier target"
                                        .to_string(),
                                ),
                            };
                            self.compile_object_rest_from(&source_tmp, &excluded_keys)?;
                            self.assign_identifier_from_top(name)?;
                        }
                        HashEntry::KeyValue {
                            key: key_expr,
                            value: target_expr,
                        } => {
                            self.compile_expression(&Expression::Identifier(source_tmp.clone()))?;
                            self.compile_expression(key_expr)?;
                            self.emit(Opcode::OpIndex, &[]);

                            match target_expr {
                                Expression::Identifier(name) => {
                                    self.assign_identifier_from_top(name)?;
                                }
                                Expression::Assign {
                                    left,
                                    operator,
                                    right,
                                } if operator == "=" => match &**left {
                                    Expression::Identifier(name) => {
                                        self.apply_destructuring_default(Some(right.as_ref()))?;
                                        self.assign_identifier_from_top(name)?;
                                    }
                                    Expression::Array(_) | Expression::Hash(_) => {
                                        self.apply_destructuring_default(Some(right.as_ref()))?;
                                        let nested_tmp = self.make_temp_name("obj_nested");
                                        self.assign_identifier_from_top(&nested_tmp)?;
                                        self.compile_assignment_expression(
                                            left,
                                            "=",
                                            &Expression::Identifier(nested_tmp),
                                        )?;
                                        self.emit(Opcode::OpPop, &[]);
                                    }
                                    _ => {
                                        return Err(
                                            "object destructuring default supports identifier or nested pattern targets only"
                                                .to_string(),
                                        )
                                    }
                                },
                                Expression::Array(_) | Expression::Hash(_) => {
                                    let nested_tmp = self.make_temp_name("obj_nested");
                                    self.assign_identifier_from_top(&nested_tmp)?;
                                    self.compile_assignment_expression(
                                        target_expr,
                                        "=",
                                        &Expression::Identifier(nested_tmp),
                                    )?;
                                    self.emit(Opcode::OpPop, &[]);
                                }
                                _ => {
                                    return Err(
                                        "object destructuring assignment supports identifier or nested pattern targets only"
                                            .to_string(),
                                    )
                                }
                            }
                        }
                        HashEntry::Method { .. }
                        | HashEntry::Getter { .. }
                        | HashEntry::Setter { .. } => {
                            return Err(
                                "methods/getters/setters not valid in destructuring assignment"
                                    .to_string(),
                            );
                        }
                    }
                }

                self.compile_expression(&Expression::Identifier(source_tmp))?;
                Ok(())
            }
            _ => Err("invalid assignment target".to_string()),
        }
    }

    fn emit(&mut self, op: Opcode, operands: &[u16]) -> usize {
        let pos = self.instructions.len();
        self.instructions.extend(make(op, operands));
        self.last_opcode = Some(op);
        self.last_position = pos;
        // Track stack depth for bounds check hoisting
        let effect = Self::stack_effect(op, operands);
        self.current_stack_depth += effect;
        if self.current_stack_depth > self.max_stack_depth {
            self.max_stack_depth = self.current_stack_depth;
        }
        pos
    }

    /// Returns the net stack effect of an opcode (positive = pushes, negative = pops).
    fn stack_effect(op: Opcode, operands: &[u16]) -> i32 {
        use Opcode::*;
        match op {
            // Push one value
            OpConstant | OpTrue | OpFalse | OpNull | OpUndefined | OpGetGlobal | OpGetLocal
            | OpGetKeysIterator | OpGetLocalProperty | OpGetGlobalProperty | OpNewTarget
            | OpImportMeta => 1,
            // Pop one, push one (net 0)
            OpMinus | OpBang | OpTypeof | OpUnaryPlus | OpIsNullish => 0,
            // Pop two, push one (net -1)
            OpAdd | OpSub | OpMul | OpDiv | OpMod | OpPow | OpEqual | OpNotEqual
            | OpGreaterThan | OpLessThan | OpLessOrEqual | OpGreaterOrEqual | OpStrictEqual
            | OpStrictNotEqual | OpIndex | OpInstanceof | OpIn | OpBitwiseAnd | OpBitwiseOr
            | OpBitwiseXor | OpLeftShift | OpRightShift | OpUnsignedRightShift
            | OpDeleteProperty | OpIteratorRest | OpAppendElement | OpAppendSpread => -1,
            // Pop one
            OpPop | OpSetGlobal | OpSetLocal | OpThrow | OpReturnValue => -1,
            // Complex stack effects
            OpGetProperty | OpSetLocalProperty | OpSetGlobalProperty => 0, // pop+push or pop obj+push
            OpSetLocalPropertyPop | OpSetGlobalPropertyPop => -1,          // pop value, no push
            OpSetIndex => -2, // pop obj+key+value, push obj
            OpJumpNotTruthy => -1,
            OpJump | OpReturn | OpHalt => 0,
            // Fused loop opcodes: no net stack effect.
            OpIncrementLocal
            | OpTestLocalLtConstJump
            | OpTestLocalLeConstJump
            | OpIncrementGlobal
            | OpTestGlobalLtConstJump
            | OpTestGlobalLeConstJump
            | OpAccumulateGlobal
            | OpIncrementGlobalAndJump
            | OpModGlobalConstStrictEqConstJump
            | OpAddConstToGlobalProperty
            | OpAddGlobalPropsToGlobalProp => 0,
            // Variable-argument opcodes: conservative estimate
            OpArray => 1i32.saturating_sub(operands.first().copied().unwrap_or(0) as i32),
            OpHash => 1i32.saturating_sub(operands.first().copied().unwrap_or(0) as i32),
            OpCall => {
                // pops function + N args, pushes result
                0i32.saturating_sub(operands.first().copied().unwrap_or(0) as i32)
            }
            OpCallGlobal => {
                // pops N args (no callee on stack), pushes result
                // operands[1] = num_args
                1i32.saturating_sub(operands.get(1).copied().unwrap_or(0) as i32)
            }
            OpNew => 0i32.saturating_sub(operands.first().copied().unwrap_or(0) as i32),
            OpObjectRest => 1i32.saturating_sub(operands.first().copied().unwrap_or(0) as i32),
            OpAwait => 0,
            OpCallSpread | OpNewSpread => 0,
            OpSuper => 1,
            // DefineAccessor pops a function from stack, hash stays on stack (net -1)
            OpDefineAccessor => -1,
            // InitClass: pops class, pushes class back (net 0)
            OpInitClass => 0,
            // OpYield: pops yielded value, pushes received value (net 0)
            OpYield => 0,
        }
    }

    fn change_operand(&mut self, op_pos: usize, operand: u16) {
        if op_pos + 2 >= self.instructions.len() {
            return;
        }
        self.instructions[op_pos + 1] = ((operand >> 8) & 0xff) as u8;
        self.instructions[op_pos + 2] = (operand & 0xff) as u8;
    }

    /// Patch a u16 operand at `op_pos + 1 + byte_offset` (byte_offset is 0-indexed from
    /// the first operand byte). Used for fused opcodes where the target is not the first operand.
    fn change_operand_at(&mut self, op_pos: usize, byte_offset: usize, operand: u16) {
        let pos = op_pos + 1 + byte_offset;
        if pos + 1 >= self.instructions.len() {
            return;
        }
        self.instructions[pos] = ((operand >> 8) & 0xff) as u8;
        self.instructions[pos + 1] = (operand & 0xff) as u8;
    }

    /// Try to match `ident < const` or `ident <= const` for fused test+jump.
    /// Works for both locals and globals.
    /// Returns (slot_idx, const_idx, fused_opcode) if the pattern matches.
    fn try_fuse_cmp_const(&mut self, cond: &Expression) -> Option<(u16, u16, Opcode)> {
        if let Expression::Infix {
            left,
            operator,
            right,
        } = cond
        {
            if operator != "<" && operator != "<=" {
                return None;
            }
            // Left must be an identifier.
            let name = if let Expression::Identifier(n) = &**left {
                n
            } else {
                return None;
            };
            // Right must be a compile-time numeric constant.
            let const_idx = match &**right {
                Expression::Integer(v) => self.add_constant_int(*v),
                Expression::Float(v) => self.add_constant_float(*v),
                _ => return None,
            };
            // Try local first, then global.
            if let Some(&local_idx) = self.locals.get(name.as_str()) {
                let fused_op = if operator == "<" {
                    Opcode::OpTestLocalLtConstJump
                } else {
                    Opcode::OpTestLocalLeConstJump
                };
                Some((local_idx, const_idx, fused_op))
            } else if let Some(&global_idx) = self.globals.get(name.as_str()) {
                let fused_op = if operator == "<" {
                    Opcode::OpTestGlobalLtConstJump
                } else {
                    Opcode::OpTestGlobalLeConstJump
                };
                Some((global_idx, const_idx, fused_op))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Try to match `(ident % const_a) === const_b` for fused
    /// OpModGlobalConstStrictEqConstJump. Only matches globals.
    /// Returns (global_idx, mod_const_idx, cmp_const_idx) if the pattern matches.
    fn try_fuse_mod_strict_eq_const(&mut self, cond: &Expression) -> Option<(u16, u16, u16)> {
        // Pattern: Infix { left: Infix { left: Identifier, op: "%", right: const },
        //                   op: "===",
        //                   right: const }
        if let Expression::Infix {
            left: outer_left,
            operator: outer_op,
            right: outer_right,
        } = cond
        {
            if outer_op != "===" {
                return None;
            }
            // outer_left must be Infix { left: Identifier, op: "%", right: const }
            if let Expression::Infix {
                left: mod_left,
                operator: mod_op,
                right: mod_right,
            } = &**outer_left
            {
                if mod_op != "%" {
                    return None;
                }
                let name = if let Expression::Identifier(n) = &**mod_left {
                    n
                } else {
                    return None;
                };
                // Must be a global (not local).
                if self.locals.contains_key(name.as_str()) {
                    return None;
                }
                let global_idx = *self.globals.get(name.as_str())?;
                // mod_right must be a compile-time constant.
                let mod_const_idx = match &**mod_right {
                    Expression::Integer(v) => self.add_constant_int(*v),
                    Expression::Float(v) => self.add_constant_float(*v),
                    _ => return None,
                };
                // outer_right must be a compile-time constant.
                let cmp_const_idx = match &**outer_right {
                    Expression::Integer(v) => self.add_constant_int(*v),
                    Expression::Float(v) => self.add_constant_float(*v),
                    _ => return None,
                };
                Some((global_idx, mod_const_idx, cmp_const_idx))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Try to match `ident = ident + const_expr` or `ident += const_expr` for fused increment.
    /// Works for both locals (OpIncrementLocal) and globals (OpIncrementGlobal).
    /// Returns (slot_idx, const_idx, opcode) if the pattern matches.
    fn try_fuse_increment(&mut self, upd: &Expression) -> Option<(u16, u16, Opcode)> {
        if let Expression::Assign {
            left,
            operator,
            right,
        } = upd
        {
            // Left must be an identifier.
            let name = if let Expression::Identifier(n) = &**left {
                n
            } else {
                return None;
            };

            if operator == "+=" {
                // Direct compound assignment: ident += const_expr
                let const_idx = match &**right {
                    Expression::Integer(v) => self.add_constant_int(*v),
                    Expression::Float(v) => self.add_constant_float(*v),
                    _ => return None,
                };
                if let Some(&local_idx) = self.locals.get(name.as_str()) {
                    return Some((local_idx, const_idx, Opcode::OpIncrementLocal));
                } else if let Some(&global_idx) = self.globals.get(name.as_str()) {
                    return Some((global_idx, const_idx, Opcode::OpIncrementGlobal));
                }
                return None;
            }

            if operator != "=" {
                return None;
            }

            // Right must be Infix { left: Identifier(same_name), op: "+", right: const_expr }
            if let Expression::Infix {
                left: infix_left,
                operator: infix_op,
                right: infix_right,
            } = &**right
            {
                if infix_op != "+" {
                    return None;
                }
                // Check that the infix left is the same identifier.
                if let Expression::Identifier(infix_name) = &**infix_left {
                    if infix_name != name {
                        return None;
                    }
                } else {
                    return None;
                }
                // The increment value must be a compile-time numeric constant.
                let const_idx = match &**infix_right {
                    Expression::Integer(v) => self.add_constant_int(*v),
                    Expression::Float(v) => self.add_constant_float(*v),
                    _ => return None,
                };
                // Try local first, then global.
                if let Some(&local_idx) = self.locals.get(name.as_str()) {
                    Some((local_idx, const_idx, Opcode::OpIncrementLocal))
                } else if let Some(&global_idx) = self.globals.get(name.as_str()) {
                    Some((global_idx, const_idx, Opcode::OpIncrementGlobal))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Try to match `target = target + source` (or `target += source`) where both
    /// `target` and `source` are globals, for fused OpAccumulateGlobal.
    /// Returns (target_global_idx, source_global_idx) if the pattern matches.
    fn try_fuse_accumulate_global(&self, expr: &Expression) -> Option<(u16, u16)> {
        if let Expression::Assign {
            left,
            operator,
            right,
        } = expr
        {
            let target_name = if let Expression::Identifier(n) = &**left {
                n
            } else {
                return None;
            };
            // Must be a global.
            let target_idx = *self.globals.get(target_name.as_str())?;
            // Also skip if it shadows a local.
            if self.locals.contains_key(target_name.as_str()) {
                return None;
            }

            // Match `target = target + source` or `target += source`.
            let source_name = if operator == "=" {
                // Right side must be `target + source` (Infix with "+").
                if let Expression::Infix {
                    left: infix_left,
                    operator: infix_op,
                    right: infix_right,
                } = &**right
                {
                    if infix_op != "+" {
                        return None;
                    }
                    // Left of the add must be the same identifier as the target.
                    let lhs_name = if let Expression::Identifier(n) = &**infix_left {
                        n
                    } else {
                        return None;
                    };
                    if lhs_name != target_name {
                        return None;
                    }
                    // Right of the add must be a different global identifier.
                    if let Expression::Identifier(n) = &**infix_right {
                        n
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            } else if operator == "+=" {
                // `target += source` — right side must be a global identifier.
                if let Expression::Identifier(n) = &**right {
                    n
                } else {
                    return None;
                }
            } else {
                return None;
            };

            // Source must be a global (and not a local shadow).
            if self.locals.contains_key(source_name.as_str()) {
                return None;
            }
            let source_idx = *self.globals.get(source_name.as_str())?;

            Some((target_idx, source_idx))
        } else {
            None
        }
    }

    /// Try to match `obj.prop = obj.prop + const`, `obj.prop = obj.prop - const`,
    /// `obj.prop += const`, or `obj.prop -= const` where `obj` is a global
    /// identifier, `prop` is a string literal property, and `const` is a
    /// compile-time numeric constant. Returns the fused opcode operands if matched.
    ///
    /// This collapses 4 dispatches (GetGlobalProperty + Constant + Add + SetGlobalPropertyPop)
    /// into a single OpAddConstToGlobalProperty dispatch.
    fn try_fuse_add_const_to_global_property(
        &mut self,
        expr: &Expression,
    ) -> Option<(u16, u16, u16, u16)> {
        let (left, operator, right) = if let Expression::Assign {
            left,
            operator,
            right,
        } = expr
        {
            (left, operator.as_str(), right)
        } else {
            return None;
        };

        // Left side must be `obj.prop` (Expression::Index { Identifier(obj), String(prop) })
        let (obj_name, prop_str) = if let Expression::Index {
            left: obj_expr,
            index: prop_expr,
        } = &**left
        {
            if let (Expression::Identifier(name), Expression::String(prop)) =
                (&**obj_expr, &**prop_expr)
            {
                (name, prop)
            } else {
                return None;
            }
        } else {
            return None;
        };

        // obj must be a global (not a local).
        if self.locals.contains_key(obj_name.as_str()) {
            return None;
        }
        let global_idx = *self.globals.get(obj_name.as_str())?;

        // Determine the constant value to add.
        let val_const_idx = if operator == "+=" || operator == "-=" {
            // Direct compound: obj.prop += const or obj.prop -= const
            match &**right {
                Expression::Integer(v) => {
                    let actual = if operator == "-=" { -*v } else { *v };
                    self.add_constant_int(actual)
                }
                Expression::Float(v) => {
                    let actual = if operator == "-=" { -*v } else { *v };
                    self.add_constant_float(actual)
                }
                _ => return None,
            }
        } else if operator == "=" {
            // obj.prop = obj.prop + const (or obj.prop - const)
            if let Expression::Infix {
                left: infix_left,
                operator: infix_op,
                right: infix_right,
            } = &**right
            {
                if infix_op != "+" && infix_op != "-" {
                    return None;
                }
                // The infix left must be `obj.prop` with the same obj and prop.
                if let Expression::Index {
                    left: il,
                    index: ir,
                } = &**infix_left
                {
                    if let (Expression::Identifier(n2), Expression::String(p2)) = (&**il, &**ir) {
                        if n2 != obj_name || p2 != prop_str {
                            return None;
                        }
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
                // The infix right must be a compile-time numeric constant.
                match &**infix_right {
                    Expression::Integer(v) => {
                        let actual = if infix_op == "-" { -*v } else { *v };
                        self.add_constant_int(actual)
                    }
                    Expression::Float(v) => {
                        let actual = if infix_op == "-" { -*v } else { *v };
                        self.add_constant_float(actual)
                    }
                    _ => return None,
                }
            } else {
                return None;
            }
        } else {
            return None;
        };

        let prop_const_idx = self.add_constant_string(Rc::from(prop_str.as_str()));
        let cache_slot = self.next_cache_slot;
        self.next_cache_slot += 1;

        Some((global_idx, prop_const_idx, val_const_idx, cache_slot))
    }

    /// Try to match `obj.dst = obj.src1 + obj.src2` where `obj` is a global
    /// identifier and all three properties are string literals. Returns the
    /// fused opcode operands: (global_idx, src1_prop_const, src1_cache,
    /// src2_prop_const, src2_cache, dst_prop_const, dst_cache).
    ///
    /// This collapses 4 dispatches (GetGlobalProperty×2 + Add + SetGlobalPropertyPop)
    /// into a single OpAddGlobalPropsToGlobalProp dispatch.
    fn try_fuse_add_global_props_to_global_prop(
        &mut self,
        expr: &Expression,
    ) -> Option<(u16, u16, u16, u16, u16, u16, u16)> {
        let (left, operator, right) = if let Expression::Assign {
            left,
            operator,
            right,
        } = expr
        {
            (left, operator.as_str(), right)
        } else {
            return None;
        };

        // Only plain assignment `=`.
        if operator != "=" {
            return None;
        }

        // Left side must be `obj.prop` (Expression::Index { Identifier(obj), String(prop) })
        let (obj_name, dst_prop) = if let Expression::Index {
            left: obj_expr,
            index: prop_expr,
        } = &**left
        {
            if let (Expression::Identifier(name), Expression::String(prop)) =
                (&**obj_expr, &**prop_expr)
            {
                (name, prop)
            } else {
                return None;
            }
        } else {
            return None;
        };

        // obj must be a global (not a local).
        if self.locals.contains_key(obj_name.as_str()) {
            return None;
        }
        let global_idx = *self.globals.get(obj_name.as_str())?;

        // Right side must be `obj.src1 + obj.src2`.
        let (src1_prop, src2_prop) = if let Expression::Infix {
            left: infix_left,
            operator: infix_op,
            right: infix_right,
        } = &**right
        {
            if infix_op != "+" {
                return None;
            }
            // Both sides must be `obj.prop` with the same obj identifier.
            let s1 = if let Expression::Index { left: l, index: r } = &**infix_left {
                if let (Expression::Identifier(n), Expression::String(p)) = (&**l, &**r) {
                    if n != obj_name {
                        return None;
                    }
                    p
                } else {
                    return None;
                }
            } else {
                return None;
            };
            let s2 = if let Expression::Index { left: l, index: r } = &**infix_right {
                if let (Expression::Identifier(n), Expression::String(p)) = (&**l, &**r) {
                    if n != obj_name {
                        return None;
                    }
                    p
                } else {
                    return None;
                }
            } else {
                return None;
            };
            (s1, s2)
        } else {
            return None;
        };

        let src1_prop_const = self.add_constant_string(Rc::from(src1_prop.as_str()));
        let src1_cache = self.next_cache_slot;
        self.next_cache_slot += 1;
        let src2_prop_const = self.add_constant_string(Rc::from(src2_prop.as_str()));
        let src2_cache = self.next_cache_slot;
        self.next_cache_slot += 1;
        let dst_prop_const = self.add_constant_string(Rc::from(dst_prop.as_str()));
        let dst_cache = self.next_cache_slot;
        self.next_cache_slot += 1;

        Some((
            global_idx,
            src1_prop_const,
            src1_cache,
            src2_prop_const,
            src2_cache,
            dst_prop_const,
            dst_cache,
        ))
    }

    fn remove_last_pop(&mut self) {
        if self.last_opcode == Some(Opcode::OpPop) {
            self.instructions.truncate(self.last_position);
            self.last_opcode = None;
            self.elided_trailing_get = None;
            return;
        }
        // If the last statement elided a trailing OpGet{Local,Global} + OpPop
        // (via try_elide_get_before_pop), we need the value on the stack after all.
        // Re-emit the OpGet to restore it.
        if let Some((get_op, operand)) = self.elided_trailing_get.take() {
            self.emit(get_op, &[operand]);
        }
    }

    /// Peephole: if the instruction stream ends with OpSet{Local,Global}(idx) + OpGet{Local,Global}(idx),
    /// remove the trailing OpGet. This eliminates the redundant re-read that
    /// assignment expressions emit when used in statement position (where the value
    /// is immediately discarded by OpPop).
    ///
    /// `stmt_start` is the bytecode offset at the beginning of the current
    /// `Statement::Expression`. For globals, we require that the `OpSetGlobal`
    /// was emitted within this same statement (`set_pos >= stmt_start`) to
    /// prevent cross-statement false matches where a prior statement's
    /// `OpSetGlobal(idx)` is adjacent to this statement's `OpGetGlobal(idx)`.
    /// Returns true if the trailing OpGet was elided.
    fn try_elide_get_before_pop(&mut self, stmt_start: usize) -> bool {
        // OpGet{Local,Global} is 3 bytes: [opcode, idx_hi, idx_lo]
        // OpSet{Local,Global} is also 3 bytes: [opcode, idx_hi, idx_lo]
        // We need at least 6 bytes: Set(3) + Get(3)
        let len = self.instructions.len();
        if len < 6 {
            return false;
        }
        // last_position points to the OpGet we're about to check
        let get_pos = self.last_position;
        if get_pos < 3 {
            return false;
        }
        let set_pos = get_pos - 3;
        // Determine which pair: Local or Global
        let set_opcode = self.instructions[set_pos];
        let get_opcode = self.instructions[get_pos];
        let (expected_set, set_op_enum) = if get_opcode == Opcode::OpGetLocal as u8 {
            (Opcode::OpSetLocal as u8, Opcode::OpSetLocal)
        } else if get_opcode == Opcode::OpGetGlobal as u8 {
            (Opcode::OpSetGlobal as u8, Opcode::OpSetGlobal)
        } else {
            return false;
        };
        // For globals, ensure the OpSetGlobal was emitted within this statement
        // to prevent cross-statement false matches.
        if get_opcode == Opcode::OpGetGlobal as u8 && set_pos < stmt_start {
            return false;
        }
        // Verify the preceding instruction is the matching Set
        if set_opcode != expected_set {
            return false;
        }
        // Verify both operands match (same slot index)
        if self.instructions[set_pos + 1] != self.instructions[get_pos + 1]
            || self.instructions[set_pos + 2] != self.instructions[get_pos + 2]
        {
            return false;
        }
        // Remove the trailing OpGet (3 bytes)
        let get_operand = ((self.instructions[get_pos + 1] as u16) << 8)
            | (self.instructions[get_pos + 2] as u16);
        let get_op_enum = if get_opcode == Opcode::OpGetLocal as u8 {
            Opcode::OpGetLocal
        } else {
            Opcode::OpGetGlobal
        };
        self.instructions.truncate(get_pos);
        self.last_position = set_pos;
        self.last_opcode = Some(set_op_enum);
        // Record the elision so remove_last_pop() can restore it if needed.
        self.elided_trailing_get = Some((get_op_enum, get_operand));
        // Stack effect: OpGet pushes +1, and we're removing it + skipping OpPop(-1).
        // Net adjustment: instead of Get(+1) + Pop(-1) = 0, we have nothing = 0. Correct.
        self.current_stack_depth -= 1;
        true
    }

    fn add_constant(&mut self, obj: Object) -> u16 {
        self.constants.push(obj);
        (self.constants.len() - 1) as u16
    }

    fn add_constant_string(&mut self, s: Rc<str>) -> u16 {
        if let Some(&idx) = self.constant_strings.get(&s) {
            return idx;
        }
        let interned = intern_str(&s);
        let idx = self.add_constant(Object::String(Rc::clone(&interned)));
        self.constant_strings.insert(interned, idx);
        idx
    }

    fn add_constant_int(&mut self, v: i64) -> u16 {
        if let Some(&idx) = self.constant_ints.get(&v) {
            return idx;
        }
        let idx = self.add_constant(Object::Integer(v));
        self.constant_ints.insert(v, idx);
        idx
    }

    fn add_constant_float(&mut self, v: f64) -> u16 {
        let bits = v.to_bits();
        if let Some(&idx) = self.constant_floats.get(&bits) {
            return idx;
        }
        let idx = self.add_constant(Object::Float(v));
        self.constant_floats.insert(bits, idx);
        idx
    }

    /// If `expr` is an `Identifier` that resolves to a global (not a local, not
    /// a builtin), return the global slot index.  Used by OpCallGlobal fusion.
    fn try_resolve_global_function(&self, expr: &Expression) -> Option<u16> {
        if let Expression::Identifier(name) = expr {
            // Must NOT be a local — locals use OpGetLocal, not OpGetGlobal.
            if self.locals.contains_key(name) {
                return None;
            }
            // Must be an existing global slot.
            if let Some(&idx) = self.globals.get(name) {
                return Some(idx);
            }
            // In function scope, ensure_global_slot would create one, but
            // we only fuse when the slot already exists (avoids side effects).
        }
        None
    }

    fn compile_delete_expression(&mut self, value: &Expression) -> Result<(), String> {
        match value {
            Expression::Index { left, index } => match &**left {
                Expression::Identifier(name) => {
                    let is_local = self.locals.contains_key(name);
                    let idx = if is_local {
                        *self.locals.get(name).expect("local exists")
                    } else if let Some(idx) = self.globals.get(name) {
                        *idx
                    } else {
                        self.emit(Opcode::OpTrue, &[]);
                        return Ok(());
                    };

                    if is_local {
                        self.emit(Opcode::OpGetLocal, &[idx]);
                    } else {
                        self.emit(Opcode::OpGetGlobal, &[idx]);
                    }
                    self.compile_expression(index)?;
                    self.emit(Opcode::OpDeleteProperty, &[]); // mutated container

                    if is_local {
                        self.emit(Opcode::OpSetLocal, &[idx]);
                    } else {
                        self.emit(Opcode::OpSetGlobal, &[idx]);
                    }
                    self.emit(Opcode::OpTrue, &[]);
                    Ok(())
                }
                _ => {
                    self.compile_expression(left)?;
                    self.compile_expression(index)?;
                    self.emit(Opcode::OpDeleteProperty, &[]);
                    self.emit(Opcode::OpPop, &[]);
                    self.emit(Opcode::OpTrue, &[]);
                    Ok(())
                }
            },
            Expression::Identifier(_) => {
                self.emit(Opcode::OpFalse, &[]);
                Ok(())
            }
            _ => {
                self.compile_expression(value)?;
                self.emit(Opcode::OpPop, &[]);
                self.emit(Opcode::OpTrue, &[]);
                Ok(())
            }
        }
    }

    fn compile_logical_assignment_identifier(
        &mut self,
        name: &str,
        operator: &str,
        right: &Expression,
    ) -> Result<(), String> {
        let is_local = self.locals.contains_key(name);
        let idx = if is_local {
            *self.locals.get(name).expect("local exists")
        } else if let Some(idx) = self.globals.get(name) {
            *idx
        } else {
            if self.next_global as usize >= GLOBALS_SIZE {
                return Err("global symbol table overflow".to_string());
            }
            let idx = self.next_global;
            self.globals.insert(name.to_string(), idx);
            self.next_global += 1;
            idx
        };

        if is_local {
            self.emit(Opcode::OpGetLocal, &[idx]);
        } else {
            self.emit(Opcode::OpGetGlobal, &[idx]);
        }

        match operator {
            "&&=" => {
                let keep_left = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
                self.compile_expression(right)?;
                if is_local {
                    self.emit(Opcode::OpSetLocal, &[idx]);
                    self.emit(Opcode::OpGetLocal, &[idx]);
                } else {
                    self.emit(Opcode::OpSetGlobal, &[idx]);
                    self.emit(Opcode::OpGetGlobal, &[idx]);
                }
                let end_pos = self.emit(Opcode::OpJump, &[9999]);

                let left_pos = self.instructions.len();
                self.change_operand(keep_left, left_pos as u16);
                if is_local {
                    self.emit(Opcode::OpGetLocal, &[idx]);
                } else {
                    self.emit(Opcode::OpGetGlobal, &[idx]);
                }

                let end = self.instructions.len();
                self.change_operand(end_pos, end as u16);
            }
            "||=" => {
                let eval_right = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
                if is_local {
                    self.emit(Opcode::OpGetLocal, &[idx]);
                } else {
                    self.emit(Opcode::OpGetGlobal, &[idx]);
                }
                let end_pos = self.emit(Opcode::OpJump, &[9999]);

                let right_pos = self.instructions.len();
                self.change_operand(eval_right, right_pos as u16);
                self.compile_expression(right)?;
                if is_local {
                    self.emit(Opcode::OpSetLocal, &[idx]);
                    self.emit(Opcode::OpGetLocal, &[idx]);
                } else {
                    self.emit(Opcode::OpSetGlobal, &[idx]);
                    self.emit(Opcode::OpGetGlobal, &[idx]);
                }

                let end = self.instructions.len();
                self.change_operand(end_pos, end as u16);
            }
            "??=" => {
                self.emit(Opcode::OpIsNullish, &[]);
                self.emit(Opcode::OpBang, &[]);
                let eval_right = self.emit(Opcode::OpJumpNotTruthy, &[9999]);
                if is_local {
                    self.emit(Opcode::OpGetLocal, &[idx]);
                } else {
                    self.emit(Opcode::OpGetGlobal, &[idx]);
                }
                let end_pos = self.emit(Opcode::OpJump, &[9999]);

                let right_pos = self.instructions.len();
                self.change_operand(eval_right, right_pos as u16);
                self.compile_expression(right)?;
                if is_local {
                    self.emit(Opcode::OpSetLocal, &[idx]);
                    self.emit(Opcode::OpGetLocal, &[idx]);
                } else {
                    self.emit(Opcode::OpSetGlobal, &[idx]);
                    self.emit(Opcode::OpGetGlobal, &[idx]);
                }

                let end = self.instructions.len();
                self.change_operand(end_pos, end as u16);
            }
            _ => {
                return Err(format!(
                    "unsupported logical assignment operator {}",
                    operator
                ))
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::compiler::Compiler;
    use crate::object::Object;
    use crate::parser::parse_program_from_source;

    #[test]
    fn compiles_simple_let_and_expr() {
        let (program, errors) = parse_program_from_source("let x = 5; x + 2;");
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(!bytecode.instructions.is_empty());
        assert_eq!(bytecode.constants.len(), 2);
    }

    #[test]
    fn compiles_if_expression() {
        let (program, errors) = parse_program_from_source("if (1 < 2) { 10 } else { 20 }");
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(!bytecode.instructions.is_empty());
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpJump as u8));
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpJumpNotTruthy as u8));
    }

    #[test]
    fn compiles_while_expression() {
        let (program, errors) =
            parse_program_from_source("let i = 0; while (i < 3) { i = i + 1; }");
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(!bytecode.instructions.is_empty());
        // The backward jump may be fused into OpIncrementGlobalAndJump when
        // the last body statement is a fused global increment.
        let has_backward_jump = bytecode.instructions.iter().any(|b| {
            *b == crate::code::Opcode::OpJump as u8
                || *b == crate::code::Opcode::OpIncrementGlobalAndJump as u8
        });
        assert!(
            has_backward_jump,
            "expected OpJump or OpIncrementGlobalAndJump"
        );
        // Condition may be fused into OpTestGlobalLtConstJump or OpTestLocalLtConstJump
        // instead of OpJumpNotTruthy.
        let has_jump_not_truthy = bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpJumpNotTruthy as u8);
        let has_fused_cond = bytecode.instructions.iter().any(|b| {
            *b == crate::code::Opcode::OpTestLocalLtConstJump as u8
                || *b == crate::code::Opcode::OpTestGlobalLtConstJump as u8
                || *b == crate::code::Opcode::OpTestLocalLeConstJump as u8
                || *b == crate::code::Opcode::OpTestGlobalLeConstJump as u8
        });
        assert!(
            has_jump_not_truthy || has_fused_cond,
            "expected OpJumpNotTruthy or a fused test+jump opcode"
        );
    }

    #[test]
    fn compiles_for_expression() {
        let (program, errors) = parse_program_from_source(
            "let x = 0; for (let i = 0; i < 3; i = i + 1) { x = x + i; }",
        );
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(!bytecode.instructions.is_empty());
        // The backward jump may be fused into OpIncrementGlobalAndJump when
        // the update is a fused global increment.
        let has_backward_jump = bytecode.instructions.iter().any(|b| {
            *b == crate::code::Opcode::OpJump as u8
                || *b == crate::code::Opcode::OpIncrementGlobalAndJump as u8
        });
        assert!(
            has_backward_jump,
            "expected OpJump or OpIncrementGlobalAndJump"
        );
        // Condition may be fused into OpTestGlobalLtConstJump or OpTestLocalLtConstJump
        // instead of OpJumpNotTruthy.
        let has_jump_not_truthy = bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpJumpNotTruthy as u8);
        let has_fused_cond = bytecode.instructions.iter().any(|b| {
            *b == crate::code::Opcode::OpTestLocalLtConstJump as u8
                || *b == crate::code::Opcode::OpTestGlobalLtConstJump as u8
                || *b == crate::code::Opcode::OpTestLocalLeConstJump as u8
                || *b == crate::code::Opcode::OpTestGlobalLeConstJump as u8
        });
        assert!(
            has_jump_not_truthy || has_fused_cond,
            "expected OpJumpNotTruthy or a fused test+jump opcode"
        );
    }

    #[test]
    fn compiles_function_declaration() {
        let (program, errors) =
            parse_program_from_source("function add(a, b) { return a + b; } add(1, 2);");
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(!bytecode.instructions.is_empty());
        assert!(bytecode
            .constants
            .iter()
            .any(|c| matches!(c, Object::CompiledFunction(_))));
    }

    #[test]
    fn compiles_for_of_statement() {
        let (program, errors) =
            parse_program_from_source("let sum = 0; for (let x of [1,2,3]) { sum = sum + x; }");
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(!bytecode.instructions.is_empty());
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpArray as u8));
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpJump as u8));
    }

    #[test]
    fn compiles_for_in_statement() {
        let (program, errors) = parse_program_from_source(
            "let out = \"\"; for (let k in {\"a\":1,\"b\":2}) { out = out + k; }",
        );
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(!bytecode.instructions.is_empty());
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpGetKeysIterator as u8));
    }

    #[test]
    fn compiles_class_declaration() {
        let (program, errors) = parse_program_from_source(
            "class Point { constructor(x, y) { this.x = x; this.y = y; } sum() { return this.x + this.y; } }",
        );
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(bytecode
            .constants
            .iter()
            .any(|c| matches!(c, Object::Class(_))));
    }

    #[test]
    fn compiles_try_catch_finally_statement() {
        let (program, errors) = parse_program_from_source(
            "let x = 0; try { throw 1; } catch (e) { x = e; } finally { x = x + 1; }",
        );
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(!bytecode.instructions.is_empty());
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpJump as u8));
    }

    #[test]
    fn compiles_in_and_instanceof_expression() {
        let (program, errors) = parse_program_from_source(
            "\"a\" in {\"a\":1}; class P { constructor() {} } let p = new P(); p instanceof P;",
        );
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpIn as u8));
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpInstanceof as u8));
    }

    #[test]
    fn compiles_class_method_kinds() {
        let (program, errors) = parse_program_from_source(
            "class C { static s() { return 1; } get x() { return this._x; } set x(v) { this._x = v; } m() { return this._x; } }",
        );
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");

        let class_obj = bytecode
            .constants
            .iter()
            .find_map(|c| {
                if let Object::Class(cls) = c {
                    Some(cls)
                } else {
                    None
                }
            })
            .expect("class constant");

        assert!(class_obj.static_methods.contains_key("s"));
        assert!(class_obj.getters.contains_key("x"));
        assert!(class_obj.setters.contains_key("x"));
        assert!(class_obj.methods.contains_key("m"));
    }

    #[test]
    fn compiles_class_extends_super_links() {
        let (program, errors) = parse_program_from_source(
            "class A { value() { return 1; } } class B extends A { value() { return super.value() + 1; } }",
        );
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");

        let mut classes = bytecode.constants.iter().filter_map(|c| {
            if let Object::Class(cls) = c {
                Some(cls)
            } else {
                None
            }
        });

        let a = classes.next().expect("class A");
        let b = classes.next().expect("class B");
        assert!(a.methods.contains_key("value"));
        assert!(b.methods.contains_key("value"));
        assert!(b.super_methods.contains_key("value"));
    }

    #[test]
    fn compiles_await_expression() {
        let (program, errors) =
            parse_program_from_source("await 1; async function id(x) { return await x; } id(2);");
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpAwait as u8));
    }

    #[test]
    fn compiles_typeof_and_delete_expression() {
        let (program, errors) =
            parse_program_from_source("let obj = {\"a\":1}; typeof 1; delete obj.a;");
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpTypeof as u8));
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpDeleteProperty as u8));
    }

    #[test]
    fn compiles_void_and_bitwise_expression() {
        let (program, errors) = parse_program_from_source("void 1; 5 & 3; 8 >>> 1; 1 === 1;");
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpUndefined as u8));
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpBitwiseAnd as u8));
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpUnsignedRightShift as u8));
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpStrictEqual as u8));
    }

    #[test]
    fn compiles_logical_and_nullish_expression() {
        let (program, errors) =
            parse_program_from_source("let a = 1; let b = 2; a && b; a || b; a ?? b;");
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpIsNullish as u8));
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpJumpNotTruthy as u8));
    }

    #[test]
    fn compiles_optional_chain_and_logical_assign_expression() {
        let (program, errors) = parse_program_from_source(
            "let obj = null; obj?.name; let f = null; f?.(1); let x = 0; x &&= 1; x ||= 2; x ??= 3;",
        );
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        let bytecode = Compiler::new().compile_program(&program).expect("compile");
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpIsNullish as u8));
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpCall as u8));
        assert!(bytecode
            .instructions
            .iter()
            .any(|b| *b == crate::code::Opcode::OpJump as u8));
    }
}
