//! Register-based compiler.
//!
//! Emits register opcodes (ROp) instead of stack opcodes.
//! Locals ARE registers (0..num_locals-1). Temporaries are allocated above.
//! `compile_expression` returns a register index. `compile_expression_into`
//! writes into a specific register.

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
use crate::object::{ClassObject, CompiledFunctionObject, Object, RegExpObject, StaticInitializer};
use crate::rcode::{rmake, ROp};

const GLOBALS_SIZE: usize = 65_536;

pub struct RCompiler {
    instructions: Vec<u8>,
    constants: Vec<Object>,
    globals: FxHashMap<String, u16>,
    next_global: u16,
    /// Maps local variable names to register indices.
    locals: FxHashMap<String, u16>,
    class_defs: FxHashMap<String, ClassObject>,
    /// Number of registers allocated for named locals.
    num_locals: u16,
    /// Next temporary register index. Always >= num_locals.
    next_temp: u16,
    /// Maximum register index used (for register_count in bytecode).
    max_reg: u16,
    is_function_scope: bool,
    loop_stack: Vec<LoopContext>,
    temp_counter: usize,
    try_stack: Vec<TryContext>,
    // Constant deduplication maps
    constant_strings: FxHashMap<Rc<str>, u16>,
    constant_ints: FxHashMap<i64, u16>,
    constant_floats: FxHashMap<u64, u16>,
    // Inline cache slot counter
    next_cache_slot: u16,
    /// Names that are referenced inside nested function bodies.
    /// When Some, only these names get global slots + mirroring at top level.
    /// None means all locals get globals (used inside inner function scopes).
    captured_names: Option<FxHashSet<String>>,
    // Names declared with `const` — assignment to these is a compile error.
    const_bindings: FxHashSet<String>,
    /// Global slot indices that were freshly allocated because a parameter
    /// shadows a captured name from the outer scope (IIFE pattern).
    /// Inner closures created in this scope that reference these slots need
    /// `MakeClosure` to snapshot the values at creation time.
    param_shadow_slots: FxHashSet<u16>,
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
    /// When a try block has a finally, returns inside try/catch are deferred:
    /// the return value is stored in `return_temp` and control jumps to the
    /// finally block. After finally executes, if `return_flag_temp` is true,
    /// the actual return happens.
    has_finally: bool,
    return_temp: Option<String>,
    return_flag_temp: Option<String>,
    return_jumps: Vec<usize>,
}

impl RCompiler {
    pub fn new() -> Self {
        Self {
            instructions: vec![],
            constants: vec![],
            globals: FxHashMap::default(),
            next_global: 0,
            locals: FxHashMap::default(),
            class_defs: FxHashMap::default(),
            num_locals: 0,
            next_temp: 0,
            max_reg: 0,
            is_function_scope: false,
            loop_stack: vec![],
            temp_counter: 0,
            try_stack: vec![],
            constant_strings: FxHashMap::default(),
            constant_ints: FxHashMap::default(),
            constant_floats: FxHashMap::default(),
            next_cache_slot: 0,
            captured_names: None,
            const_bindings: FxHashSet::default(),
            param_shadow_slots: FxHashSet::default(),
        }
    }

    /// Create a compiler pre-populated with an existing globals table.
    /// Used by `eval_in_context` to compile expressions that can access
    /// the script's global variables and functions.
    pub fn with_globals(globals: &FxHashMap<String, u16>) -> Self {
        let mut compiler = Self::new();
        for (name, &slot) in globals {
            compiler.globals.insert(name.clone(), slot);
        }
        compiler.next_global = globals
            .values()
            .copied()
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        compiler
    }

    fn new_function_scope(
        mut globals: FxHashMap<String, u16>,
        next_global: u16,
        parameters: &[String],
        captured_names: FxHashSet<String>,
    ) -> Self {
        let mut locals = FxHashMap::default();
        for (i, param) in parameters.iter().enumerate() {
            locals.insert(param.clone(), i as u16);
            // If a parameter shadows a captured variable from the outer scope,
            // remove the old global slot so that ensure_global_slot allocates a
            // fresh one.  This is necessary for IIFE patterns like
            //   (i => () => i)(i)
            // where the inner closure must capture from the IIFE's own scope,
            // not the outer loop variable.
            if captured_names.contains(param) {
                globals.remove(param);
            }
        }
        let num_locals = parameters.len() as u16;

        Self {
            instructions: vec![],
            constants: vec![],
            globals,
            next_global,
            locals,
            class_defs: FxHashMap::default(),
            num_locals,
            next_temp: num_locals,
            max_reg: if num_locals > 0 { num_locals - 1 } else { 0 },
            is_function_scope: true,
            loop_stack: vec![],
            temp_counter: 0,
            try_stack: vec![],
            constant_strings: FxHashMap::default(),
            constant_ints: FxHashMap::default(),
            constant_floats: FxHashMap::default(),
            next_cache_slot: 0,
            captured_names: Some(captured_names),
            const_bindings: FxHashSet::default(),
            param_shadow_slots: FxHashSet::default(),
        }
    }

    /// Returns true if a variable needs a global slot for inner function access.
    fn needs_global(&self, name: &str) -> bool {
        match &self.captured_names {
            Some(set) => set.contains(name),
            None => true, // inner function scopes always mirror
        }
    }

    // ── Register allocation ──────────────────────────────────────────────

    /// Allocate a temporary register.
    fn alloc_temp(&mut self) -> u16 {
        let r = self.next_temp;
        self.next_temp += 1;
        if r > self.max_reg {
            self.max_reg = r;
        }
        r
    }

    /// Save temp state; call before compiling a sub-expression to scope temps.
    fn save_temps(&self) -> u16 {
        self.next_temp
    }

    /// Restore temp state; frees temps allocated after save point.
    fn restore_temps(&mut self, saved: u16) {
        self.next_temp = saved;
    }

    /// Ensure a named local has a register. Returns its register index.
    fn ensure_local(&mut self, name: &str) -> u16 {
        if let Some(&r) = self.locals.get(name) {
            return r;
        }
        let r = self.num_locals;
        self.locals.insert(name.to_string(), r);
        self.num_locals += 1;
        // Keep next_temp above locals
        if self.next_temp < self.num_locals {
            self.next_temp = self.num_locals;
        }
        if r > self.max_reg {
            self.max_reg = r;
        }
        r
    }

    /// Ensure a binding slot exists (local in function scope, global otherwise).
    fn ensure_binding_slot(&mut self, name: &str) -> Result<BindingSlot, String> {
        if self.is_function_scope {
            Ok(BindingSlot::Local(self.ensure_local(name)))
        } else {
            Ok(BindingSlot::Global(self.ensure_global_slot(name)?))
        }
    }

    fn ensure_global_slot(&mut self, name: &str) -> Result<u16, String> {
        if let Some(&idx) = self.globals.get(name) {
            return Ok(idx);
        }
        if self.next_global as usize >= GLOBALS_SIZE {
            return Err("global symbol table overflow".to_string());
        }
        let idx = self.next_global;
        self.globals.insert(name.to_string(), idx);
        self.next_global += 1;
        Ok(idx)
    }

    // ── Top-level entry ──────────────────────────────────────────────────

    /// Compile for persistent execution: all top-level bindings are mirrored
    /// to global slots so they remain accessible after run_register() completes.
    pub fn compile_program_persistent(mut self, program: &Program) -> Result<Bytecode, String> {
        self.is_function_scope = true;
        self.captured_names = None; // None → needs_global() returns true for all names
        self.compile_program_inner(program)
    }

    pub fn compile_program(mut self, program: &Program) -> Result<Bytecode, String> {
        // Use function scope at top level so all `let` bindings become register
        // locals. This enables fused opcodes (TestLtConstJump, AddRegConst, etc.)
        // Only mirror variables to globals that are actually referenced by nested
        // function bodies (captured_names scan). This avoids the overhead of
        // SetGlobal on every assignment and GetGlobal reload after every call.
        self.is_function_scope = true;
        self.captured_names = Some(scan_captured_names(&program.statements));
        self.compile_program_inner(program)
    }

    fn compile_program_inner(mut self, program: &Program) -> Result<Bytecode, String> {
        self.hoist_var_declarations(&program.statements)?;
        let mut last_reg = None;
        for stmt in &program.statements {
            last_reg = self.compile_statement(stmt)?;
            // Free temps after each top-level statement
            self.next_temp = self.num_locals;
        }

        // If there's a last expression value, emit HaltValue; otherwise Halt.
        if let Some(r) = last_reg {
            self.emit(ROp::HaltValue, &[r as u16]);
        } else {
            self.emit(ROp::Halt, &[]);
        }

        let mut bytecode = Bytecode::with_cache_slots(
            self.instructions,
            self.constants,
            vec![],
            self.next_cache_slot,
            0,
            self.max_reg + 1,
        );
        bytecode.globals_table = self.globals.iter().map(|(k, &v)| (k.clone(), v)).collect();
        Ok(bytecode)
    }

    // ── Statement compilation ────────────────────────────────────────────
    // Returns Some(reg) if the statement produced a value (expression stmt).

    fn compile_statement(&mut self, stmt: &Statement) -> Result<Option<u16>, String> {
        match stmt {
            Statement::Let { name, value, kind } => {
                if *kind == VariableKind::Const {
                    self.const_bindings.insert(name.clone());
                }
                let slot = self.ensure_binding_slot(name)?;
                match slot {
                    BindingSlot::Local(r) => {
                        self.compile_expression_into(value, r)?;
                        // Mirror to global only if an inner function references this name
                        if self.is_function_scope && self.needs_global(name) {
                            let g = self.ensure_global_slot(name)?;
                            self.emit(ROp::SetGlobal, &[g, r as u16]);
                        }
                    }
                    BindingSlot::Global(g) => {
                        let r = self.compile_expression(value)?;
                        self.emit(ROp::SetGlobal, &[g, r as u16]);
                    }
                }
                Ok(None)
            }
            Statement::LetPattern {
                pattern,
                value,
                kind,
            } => {
                if *kind == VariableKind::Const {
                    Self::collect_pattern_names(pattern, &mut self.const_bindings);
                }
                // Pre-declare all binding names so their local registers are
                // allocated before the source temp, preventing ensure_local
                // from claiming the register holding the source object.
                self.pre_declare_pattern_locals(pattern);
                let src = self.compile_expression(value)?;
                self.assign_pattern(pattern, src)?;
                Ok(None)
            }
            Statement::Return { value } => {
                let r = self.compile_expression(value)?;
                // If inside a try-with-finally, defer the return
                if let Some(ctx) = self.try_stack.last() {
                    if ctx.has_finally {
                        if let (Some(ref rt), Some(ref rf)) = (ctx.return_temp.clone(), ctx.return_flag_temp.clone()) {
                            self.store_identifier(&rt, r)?;
                            let true_r = self.alloc_temp();
                            self.emit(ROp::LoadTrue, &[true_r as u16]);
                            self.store_identifier(&rf, true_r)?;
                            let jmp = self.emit(ROp::Jump, &[9999]);
                            // Store the jump position on the try context
                            let ctx_mut = self.try_stack.last_mut().unwrap();
                            ctx_mut.return_jumps.push(jmp);
                            return Ok(None);
                        }
                    }
                }
                self.emit(ROp::Return, &[r as u16]);
                Ok(None)
            }
            Statement::ReturnVoid => {
                // If inside a try-with-finally, defer the return
                if let Some(ctx) = self.try_stack.last() {
                    if ctx.has_finally {
                        if let (Some(ref rt), Some(ref rf)) = (ctx.return_temp.clone(), ctx.return_flag_temp.clone()) {
                            let undef_r = self.alloc_temp();
                            self.emit(ROp::LoadUndef, &[undef_r as u16]);
                            self.store_identifier(&rt, undef_r)?;
                            let true_r = self.alloc_temp();
                            self.emit(ROp::LoadTrue, &[true_r as u16]);
                            self.store_identifier(&rf, true_r)?;
                            let jmp = self.emit(ROp::Jump, &[9999]);
                            let ctx_mut = self.try_stack.last_mut().unwrap();
                            ctx_mut.return_jumps.push(jmp);
                            return Ok(None);
                        }
                    }
                }
                self.emit(ROp::ReturnUndef, &[]);
                Ok(None)
            }
            Statement::Expression(expr) => {
                let r = self.compile_expression(expr)?;
                Ok(Some(r))
            }
            Statement::Block(statements) => {
                let shadowed = self.enter_block_scope(statements);
                let mut last = None;
                for s in statements {
                    last = self.compile_statement(s)?;
                }
                self.exit_block_scope(shadowed);
                Ok(last)
            }
            Statement::MultiLet(statements) => {
                // Multi-declaration: let a = 1, b = 2; — no block scoping
                let mut last = None;
                for s in statements {
                    last = self.compile_statement(s)?;
                }
                Ok(last)
            }
            Statement::While { condition, body } => {
                self.compile_while_statement(condition, body, None)?;
                Ok(None)
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
                Ok(None)
            }
            Statement::ForOf {
                binding,
                iterable,
                body,
            } => {
                self.compile_for_of_statement(binding, iterable, body, None)?;
                Ok(None)
            }
            Statement::ForIn {
                var_name,
                iterable,
                body,
            } => {
                self.compile_for_in_statement(var_name, iterable, body, None)?;
                Ok(None)
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
                    is_arrow: false,
                };
                let r = self.compile_expression(&function_expr)?;
                self.store_binding(name, r)?;
                Ok(None)
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
                // Pre-register class name in globals so methods can reference
                // the class via the same global slot (e.g., static methods that
                // reference the class by name like `Counter.count`).
                // Only do this for non-function scope where store_binding uses
                // globals directly, OR ensure we also pre-register the local.
                if let Some(n) = name {
                    if self.is_function_scope {
                        // In function scope, store_binding will create a local AND
                        // mirror to global. Pre-register both so child compilers
                        // see the same slot for the class name.
                        self.ensure_local(n);
                        if self.needs_global(n) {
                            let _ = self.ensure_global_slot(n);
                        }
                    } else {
                        let _ = self.ensure_global_slot(n);
                    }
                }
                let class_obj = self.compile_class_literal(class_name, extends_name, members)?;
                if let Some(n) = name {
                    self.class_defs.insert(n.clone(), class_obj.clone());
                }
                let has_static_init = !class_obj.static_initializers.is_empty();
                let idx = self.add_constant(Object::Class(Box::new(class_obj)));
                let r = self.alloc_temp();
                self.emit(ROp::LoadConst, &[r as u16, idx]);
                if has_static_init {
                    self.emit(ROp::InitClass, &[r as u16]);
                    self.reload_locals_from_globals();
                }
                if let Some(n) = name {
                    self.store_binding(n, r)?;
                }
                Ok(None)
            }
            Statement::Throw { value } => {
                self.compile_throw_statement(value)?;
                Ok(None)
            }
            Statement::Try {
                try_block,
                catch_param,
                catch_block,
                finally_block,
            } => {
                let r = self.compile_try_statement(
                    try_block,
                    catch_param.as_deref(),
                    catch_block.as_deref(),
                    finally_block.as_deref(),
                )?;
                Ok(r)
            }
            Statement::Labeled { label, statement } => match statement.as_ref() {
                Statement::While { condition, body } => {
                    self.compile_while_statement(condition, body, Some(label.as_str()))?;
                    Ok(None)
                }
                Statement::DoWhile { body, condition } => {
                    self.compile_do_while_statement(body, condition, Some(label.as_str()))?;
                    Ok(None)
                }
                Statement::Switch {
                    discriminant,
                    cases,
                } => {
                    self.compile_switch_statement(discriminant, cases, Some(label.as_str()))?;
                    Ok(None)
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
                        Some(label.as_str()),
                    )?;
                    Ok(None)
                }
                Statement::ForOf {
                    binding,
                    iterable,
                    body,
                } => {
                    self.compile_for_of_statement(binding, iterable, body, Some(label.as_str()))?;
                    Ok(None)
                }
                Statement::ForIn {
                    var_name,
                    iterable,
                    body,
                } => {
                    self.compile_for_in_statement(var_name, iterable, body, Some(label.as_str()))?;
                    Ok(None)
                }
                _ => Err("only loop statements can be labeled in current Rust port".to_string()),
            },
            Statement::Break { label } => {
                let pos = self.emit(ROp::Jump, &[9999]);
                let loop_ctx = self.find_loop_ctx_mut(label.as_deref())?;
                loop_ctx.break_positions.push(pos);
                Ok(None)
            }
            Statement::Continue { label } => {
                let pos = self.emit(ROp::Jump, &[9999]);
                let loop_ctx = self.find_loop_ctx_mut(label.as_deref())?;
                loop_ctx.continue_positions.push(pos);
                Ok(None)
            }
            Statement::DoWhile { body, condition } => {
                self.compile_do_while_statement(body, condition, None)?;
                Ok(None)
            }
            Statement::Switch {
                discriminant,
                cases,
            } => {
                self.compile_switch_statement(discriminant, cases, None)?;
                Ok(None)
            }
            Statement::Debugger => {
                // No-op in sandboxed interpreter
                Ok(None)
            }
        }
    }

    /// Store a value register into the appropriate binding (local or global).
    fn store_binding(&mut self, name: &str, src: u16) -> Result<(), String> {
        if self.is_function_scope {
            let r = self.ensure_local(name);
            if r != src {
                self.emit(ROp::Move, &[r as u16, src as u16]);
            }
            // Mirror to global only if an inner function references this name
            if self.needs_global(name) {
                let g = self.ensure_global_slot(name)?;
                self.emit(ROp::SetGlobal, &[g, r as u16]);
            }
        } else {
            let g = self.ensure_global_slot(name)?;
            self.emit(ROp::SetGlobal, &[g, src as u16]);
        }
        Ok(())
    }

    // ── Expression compilation ───────────────────────────────────────────
    // Returns the register index holding the result.

    fn compile_expression(&mut self, expr: &Expression) -> Result<u16, String> {
        let dst = self.alloc_temp();
        self.compile_expression_into(expr, dst)?;
        Ok(dst)
    }

    /// Compile expression into a specific destination register.
    fn compile_expression_into(&mut self, expr: &Expression, dst: u16) -> Result<(), String> {
        match expr {
            Expression::Integer(v) => {
                let idx = self.add_constant_int(*v);
                self.emit(ROp::LoadConst, &[dst as u16, idx]);
            }
            Expression::Float(v) => {
                let idx = self.add_constant_float(*v);
                self.emit(ROp::LoadConst, &[dst as u16, idx]);
            }
            Expression::String(v) => {
                let idx = self.add_constant_string(Rc::from(v.as_str()));
                self.emit(ROp::LoadConst, &[dst as u16, idx]);
            }
            Expression::RegExp { pattern, flags } => {
                let idx = self.add_constant(Object::RegExp(Box::new(RegExpObject {
                    pattern: pattern.clone(),
                    flags: flags.clone(),
                })));
                self.emit(ROp::LoadConst, &[dst as u16, idx]);
            }
            Expression::Boolean(v) => {
                self.emit(
                    if *v { ROp::LoadTrue } else { ROp::LoadFalse },
                    &[dst as u16],
                );
            }
            Expression::Null => {
                self.emit(ROp::LoadNull, &[dst as u16]);
            }
            Expression::Identifier(name) => {
                self.load_identifier_into(name, dst)?;
            }
            Expression::This => {
                self.load_identifier_into("this", dst)?;
            }
            Expression::Super => {
                self.emit(ROp::Super, &[dst as u16]);
            }
            Expression::NewTarget => {
                self.emit(ROp::NewTarget, &[dst as u16]);
            }
            Expression::ImportMeta => {
                self.emit(ROp::ImportMeta, &[dst as u16]);
            }
            Expression::Array(items) => {
                self.compile_array_into(items, dst)?;
            }
            Expression::Hash(pairs) => {
                self.compile_hash_into(pairs, dst)?;
            }
            Expression::Prefix { operator, right } => {
                let saved = self.save_temps();
                let src = self.compile_expression(right)?;
                match operator.as_str() {
                    "!" => self.emit(ROp::Not, &[dst as u16, src as u16]),
                    "-" => self.emit(ROp::Neg, &[dst as u16, src as u16]),
                    "+" => self.emit(ROp::UnaryPlus, &[dst as u16, src as u16]),
                    "~" => {
                        // ~x = x ^ -1
                        let neg1_idx = self.add_constant_int(-1);
                        let neg1 = self.alloc_temp();
                        self.emit(ROp::LoadConst, &[neg1 as u16, neg1_idx]);
                        self.emit(ROp::BitwiseXor, &[dst as u16, src as u16, neg1 as u16]);
                        0 // dummy
                    }
                    _ => return Err(format!("unsupported prefix operator {}", operator)),
                };
                self.restore_temps(saved);
            }
            Expression::Typeof { value } => {
                let saved = self.save_temps();
                if let Expression::Identifier(name) = &**value {
                    if self.locals.contains_key(name)
                        || self.globals.contains_key(name)
                        || Self::builtin_global_object(name).is_some()
                        || self.is_function_scope
                    {
                        let src = self.compile_expression(value)?;
                        self.emit(ROp::Typeof, &[dst as u16, src as u16]);
                    } else {
                        let undef = self.alloc_temp();
                        self.emit(ROp::LoadUndef, &[undef as u16]);
                        self.emit(ROp::Typeof, &[dst as u16, undef as u16]);
                    }
                } else {
                    let src = self.compile_expression(value)?;
                    self.emit(ROp::Typeof, &[dst as u16, src as u16]);
                }
                self.restore_temps(saved);
            }
            Expression::Void { value } => {
                // Evaluate for side effects, then load undefined
                let saved = self.save_temps();
                let _ = self.compile_expression(value)?;
                self.restore_temps(saved);
                self.emit(ROp::LoadUndef, &[dst as u16]);
            }
            Expression::Delete { value } => {
                self.compile_delete_into(value, dst)?;
            }
            Expression::Infix {
                left,
                operator,
                right,
            } => {
                if operator == "," {
                    // Evaluate left for side effects, result is right
                    let saved = self.save_temps();
                    let _ = self.compile_expression(left)?;
                    self.restore_temps(saved);
                    self.compile_expression_into(right, dst)?;
                    return Ok(());
                }
                if operator == "&&" || operator == "||" || operator == "??" {
                    self.compile_logical_into(left, operator, right, dst)?;
                    return Ok(());
                }
                let saved = self.save_temps();
                let l = self.compile_expression(left)?;
                let r = self.compile_expression(right)?;
                let op = match operator.as_str() {
                    "+" => ROp::Add,
                    "-" => ROp::Sub,
                    "*" => ROp::Mul,
                    "/" => ROp::Div,
                    "%" => ROp::Mod,
                    "**" => ROp::Pow,
                    "==" => ROp::Equal,
                    "!=" => ROp::NotEqual,
                    "===" => ROp::StrictEqual,
                    "!==" => ROp::StrictNotEqual,
                    ">" => ROp::GreaterThan,
                    "<" => ROp::LessThan,
                    ">=" => ROp::GreaterOrEqual,
                    "<=" => ROp::LessOrEqual,
                    "&" => ROp::BitwiseAnd,
                    "|" => ROp::BitwiseOr,
                    "^" => ROp::BitwiseXor,
                    "<<" => ROp::LeftShift,
                    ">>" => ROp::RightShift,
                    ">>>" => ROp::UnsignedRightShift,
                    "in" => ROp::In,
                    "instanceof" => ROp::Instanceof,
                    _ => return Err(format!("unsupported infix operator {}", operator)),
                };
                self.emit(op, &[dst as u16, l as u16, r as u16]);
                self.restore_temps(saved);
            }
            Expression::If {
                condition,
                consequence,
                alternative,
            } => {
                self.compile_if_expr_into(condition, consequence, alternative.as_deref(), dst)?;
            }
            Expression::Function {
                parameters,
                body,
                is_async,
                is_generator,
                is_arrow,
            } => {
                let takes_this = !is_arrow;
                let func_obj = self.compile_function_literal(
                    parameters,
                    body,
                    takes_this,
                    *is_async,
                    *is_generator,
                )?;
                let captures = func_obj.closure_captures.clone();
                let idx = self.add_constant(Object::CompiledFunction(Box::new(func_obj)));
                if captures.is_empty() {
                    self.emit(ROp::LoadConst, &[dst as u16, idx]);
                } else {
                    // Emit MakeClosure: [dst, const_idx, count, slot0, slot1, ...]
                    let mut operands = vec![dst as u16, idx, captures.len() as u16];
                    operands.extend_from_slice(&captures);
                    self.emit(ROp::MakeClosure, &operands);
                }
            }
            Expression::Await { value } => {
                let src = self.compile_expression(value)?;
                self.emit(ROp::Await, &[dst as u16, src as u16]);
            }
            Expression::Yield { value, delegate: _ } => {
                let src = self.compile_expression(value)?;
                self.emit(ROp::Yield, &[dst as u16, src as u16]);
            }
            Expression::Sequence(exprs) => {
                // Evaluate all expressions, result of last goes into dst
                for (i, expr) in exprs.iter().enumerate() {
                    if i == exprs.len() - 1 {
                        self.compile_expression_into(expr, dst)?;
                    } else {
                        let tmp = self.compile_expression(expr)?;
                        let _ = tmp; // discard
                    }
                }
            }
            Expression::New { callee, arguments } => {
                self.compile_new_into(callee, arguments, dst)?;
            }
            Expression::Call {
                function,
                arguments,
            } => {
                self.compile_call_into(function, arguments, dst)?;
            }
            Expression::OptionalIndex { left, index } => {
                self.compile_optional_index_into(left, index, dst)?;
            }
            Expression::OptionalCall {
                function,
                arguments,
            } => {
                self.compile_optional_call_into(function, arguments, dst)?;
            }
            Expression::Assign {
                left,
                operator,
                right,
            } => {
                self.compile_assignment_into(left, operator, right, dst)?;
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
                    self.compile_assignment_into(target, assign_op, &Expression::Integer(1), dst)?;
                } else {
                    // Post-fix: save old value, do assignment, return old value
                    let old = self.compile_expression(target)?;
                    if dst != old {
                        self.emit(ROp::Move, &[dst as u16, old as u16]);
                    }
                    let tmp = self.alloc_temp();
                    self.compile_assignment_into(target, assign_op, &Expression::Integer(1), tmp)?;
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
                self.emit(ROp::LoadConst, &[dst as u16, idx]);
                if has_static_init {
                    self.emit(ROp::InitClass, &[dst as u16]);
                    self.reload_locals_from_globals();
                }
            }
            Expression::Index { left, index } => {
                self.compile_index_into(left, index, dst)?;
            }
        }
        Ok(())
    }

    // ── Helper: load identifier into register ────────────────────────────

    fn load_identifier_into(&mut self, name: &str, dst: u16) -> Result<(), String> {
        if let Some(&r) = self.locals.get(name) {
            if r != dst {
                self.emit(ROp::Move, &[dst as u16, r as u16]);
            }
            return Ok(());
        }
        if let Some(&g) = self.globals.get(name) {
            self.emit(ROp::GetGlobal, &[dst as u16, g]);
            return Ok(());
        }
        if let Some(builtin_obj) = Self::builtin_global_object(name) {
            let idx = self.add_constant(builtin_obj);
            self.emit(ROp::LoadConst, &[dst as u16, idx]);
            return Ok(());
        }
        if self.is_function_scope {
            let g = self.ensure_global_slot(name)?;
            self.emit(ROp::GetGlobal, &[dst as u16, g]);
            return Ok(());
        }
        Err(format!("undefined identifier {}", name))
    }

    // ── Array / Hash ─────────────────────────────────────────────────────

    fn compile_array_into(&mut self, items: &[Expression], dst: u16) -> Result<(), String> {
        let has_spread = items
            .iter()
            .any(|item| matches!(item, Expression::Spread { .. }));
        if has_spread {
            // Start with empty array, then append elements
            self.emit(ROp::Array, &[dst as u16, 0, 0]);
            for item in items {
                match item {
                    Expression::Spread { value } => {
                        let v = self.compile_expression(value)?;
                        self.emit(ROp::AppendSpread, &[dst as u16, v as u16]);
                    }
                    _ => {
                        let v = self.compile_expression(item)?;
                        self.emit(ROp::AppendElement, &[dst as u16, v as u16]);
                    }
                }
            }
        } else {
            // Pack elements into contiguous registers
            let base = self.next_temp;
            for (i, item) in items.iter().enumerate() {
                self.next_temp = (base as usize + i) as u16;
                let r = self.alloc_temp();
                self.compile_expression_into(item, r)?;
            }
            self.next_temp = (base as usize + items.len()) as u16;
            self.emit(ROp::Array, &[dst as u16, base as u16, items.len() as u16]);
        }
        Ok(())
    }

    fn compile_hash_into(&mut self, pairs: &[HashEntry], dst: u16) -> Result<(), String> {
        // We need to compile method bodies first (before emitting register code)
        // because compile_function_literal modifies compiler state.

        // Collect info about what to emit for the Hash opcode
        enum KvSource<'a> {
            /// Regular key-value pair
            KeyValue {
                key: &'a Expression,
                value: &'a Expression,
            },
            /// Method: key + pre-compiled function constant index
            Method { key: &'a Expression, const_idx: u16 },
            /// Spread: sentinel key + spread expr
            Spread { expr: &'a Expression },
        }

        let mut kv_sources: Vec<KvSource> = Vec::new();
        // We also need to track getter/setter entries for the second pass
        let mut accessor_const_indices: Vec<(u16, u16, bool)> = Vec::new(); // (prop_const_idx, func_const_idx, is_setter)

        // First: pre-compile all methods, getters, setters (modifies self)
        for entry in pairs {
            match entry {
                HashEntry::KeyValue { key, value } => {
                    kv_sources.push(KvSource::KeyValue { key, value });
                }
                HashEntry::Method {
                    key,
                    parameters,
                    body,
                    is_async,
                    is_generator,
                } => {
                    let func_obj = self.compile_function_literal(
                        parameters,
                        body,
                        true,
                        *is_async,
                        *is_generator,
                    )?;
                    let func_idx = self.add_constant(Object::CompiledFunction(Box::new(func_obj)));
                    kv_sources.push(KvSource::Method {
                        key,
                        const_idx: func_idx,
                    });
                }
                HashEntry::Spread(expr) => {
                    kv_sources.push(KvSource::Spread { expr });
                }
                HashEntry::Getter { key, body } => {
                    let func_obj = self.compile_function_literal(&[], body, true, false, false)?;
                    let func_idx = self.add_constant(Object::CompiledFunction(Box::new(func_obj)));
                    let prop_name = match key {
                        Expression::String(s) => s.as_str(),
                        _ => return Err("computed getter keys not yet supported".to_string()),
                    };
                    let prop_idx = self.add_constant_string(Rc::from(prop_name));
                    accessor_const_indices.push((prop_idx, func_idx, false));
                }
                HashEntry::Setter {
                    key,
                    parameter,
                    body,
                } => {
                    let params = vec![parameter.clone()];
                    let func_obj =
                        self.compile_function_literal(&params, body, true, false, false)?;
                    let func_idx = self.add_constant(Object::CompiledFunction(Box::new(func_obj)));
                    let prop_name = match key {
                        Expression::String(s) => s.as_str(),
                        _ => return Err("computed setter keys not yet supported".to_string()),
                    };
                    let prop_idx = self.add_constant_string(Rc::from(prop_name));
                    accessor_const_indices.push((prop_idx, func_idx, true));
                }
            }
        }

        // Now emit register code for key-value pairs
        let rest_key = Expression::String("__fl_rest__".to_string());
        let base = self.next_temp;
        let num_kv = kv_sources.len();
        for (i, src) in kv_sources.iter().enumerate() {
            self.next_temp = (base as usize + i * 2) as u16;
            let kr = self.alloc_temp();
            self.next_temp = (base as usize + i * 2 + 1) as u16;
            let vr = self.alloc_temp();
            match src {
                KvSource::KeyValue { key, value } => {
                    self.compile_expression_into(key, kr)?;
                    self.compile_expression_into(value, vr)?;
                }
                KvSource::Method { key, const_idx } => {
                    self.compile_expression_into(key, kr)?;
                    self.emit(ROp::LoadConst, &[vr as u16, *const_idx]);
                }
                KvSource::Spread { expr } => {
                    self.compile_expression_into(&rest_key, kr)?;
                    self.compile_expression_into(expr, vr)?;
                }
            }
        }
        self.next_temp = (base as usize + num_kv * 2) as u16;
        self.emit(ROp::Hash, &[dst as u16, base as u16, (num_kv * 2) as u16]);

        // Second pass: emit DefineAccessor for getter/setter entries
        for (prop_idx, func_idx, is_setter) in &accessor_const_indices {
            let func_r = self.alloc_temp();
            self.emit(ROp::LoadConst, &[func_r as u16, *func_idx]);
            let kind = if *is_setter { 1u16 } else { 0u16 };
            self.emit(
                ROp::DefineAccessor,
                &[dst as u16, func_r as u16, *prop_idx, kind],
            );
        }

        Ok(())
    }

    // ── Index access ─────────────────────────────────────────────────────

    fn compile_index_into(
        &mut self,
        left: &Expression,
        index: &Expression,
        dst: u16,
    ) -> Result<(), String> {
        // Named property with inline cache
        if let Expression::String(prop) = index {
            let obj_name: Option<&str> = match left {
                Expression::Identifier(name) => Some(name.as_str()),
                Expression::This => Some("this"),
                _ => None,
            };
            if let Some(name) = obj_name {
                // Local property access
                if let Some(&local_r) = self.locals.get(name) {
                    let const_idx = self.add_constant_string(Rc::from(prop.as_str()));
                    let cache_slot = self.next_cache_slot;
                    self.next_cache_slot += 1;
                    self.emit(
                        ROp::GetProp,
                        &[dst as u16, local_r as u16, const_idx, cache_slot],
                    );
                    return Ok(());
                }
                // Global property access
                if let Some(&global_idx) = self.globals.get(name) {
                    let const_idx = self.add_constant_string(Rc::from(prop.as_str()));
                    let cache_slot = self.next_cache_slot;
                    self.next_cache_slot += 1;
                    self.emit(
                        ROp::GetGlobalProp,
                        &[dst as u16, global_idx, const_idx, cache_slot],
                    );
                    return Ok(());
                }
            }
            // General object property access
            let obj = self.compile_expression(left)?;
            let const_idx = self.add_constant_string(Rc::from(prop.as_str()));
            let cache_slot = self.next_cache_slot;
            self.next_cache_slot += 1;
            self.emit(
                ROp::GetProp,
                &[dst as u16, obj as u16, const_idx, cache_slot],
            );
        } else {
            // Dynamic index
            let obj = self.compile_expression(left)?;
            let key = self.compile_expression(index)?;
            self.emit(ROp::Index, &[dst as u16, obj as u16, key as u16]);
        }
        Ok(())
    }

    // ── Calls ────────────────────────────────────────────────────────────

    /// Check if a register is a named local (would be reloaded by reload_locals_from_globals).
    fn is_local_register(&self, reg: u16) -> bool {
        self.locals.values().any(|&r| r == reg)
    }

    /// Emit reload_locals_from_globals, then move `temp` → `dst` if they differ.
    /// Used after Call/New to protect the call result from being overwritten by reload.
    fn reload_and_move_result(&mut self, call_dst: u16, final_dst: u16) {
        self.reload_locals_from_globals();
        if call_dst != final_dst {
            self.emit(ROp::Move, &[final_dst as u16, call_dst as u16]);
        }
    }

    fn compile_call_into(
        &mut self,
        function: &Expression,
        arguments: &[Expression],
        dst: u16,
    ) -> Result<(), String> {
        // If dst is a local register, the reload after the call would overwrite it.
        // Use a temp register for the call result, reload, then move to dst.
        let call_dst = if self.is_local_register(dst) {
            self.alloc_temp()
        } else {
            dst
        };

        if self.arguments_have_spread(arguments) {
            let func = self.compile_expression(function)?;
            let args_arr = self.compile_spread_args_array(arguments)?;
            self.emit(
                ROp::CallSpread,
                &[call_dst as u16, func as u16, args_arr as u16],
            );
            self.reload_and_move_result(call_dst, dst);
            return Ok(());
        }

        // Try fused OpCallGlobal
        if let Some(global_idx) = self.try_resolve_global_function(function) {
            let base = self.next_temp;
            // Reserve slot for callee (not actually used, but keeps base consistent)
            let _ = self.alloc_temp();
            for (i, arg) in arguments.iter().enumerate() {
                self.next_temp = (base as usize + 1 + i) as u16;
                let r = self.alloc_temp();
                self.compile_expression_into(arg, r)?;
            }
            self.next_temp = (base as usize + 1 + arguments.len()) as u16;
            self.emit(
                ROp::CallGlobal,
                &[
                    call_dst as u16,
                    global_idx,
                    base as u16,
                    arguments.len() as u16,
                ],
            );
            self.reload_and_move_result(call_dst, dst);
            return Ok(());
        }

        // Try fused CallMethod for obj.method(args) pattern
        if let Expression::Index { left, index } = function {
            if let Expression::String(prop_name) = index.as_ref() {
                let base = self.next_temp;
                let obj_r = self.alloc_temp();
                self.compile_expression_into(left, obj_r)?;
                for (i, arg) in arguments.iter().enumerate() {
                    self.next_temp = (base as usize + 1 + i) as u16;
                    let r = self.alloc_temp();
                    self.compile_expression_into(arg, r)?;
                }
                self.next_temp = (base as usize + 1 + arguments.len()) as u16;
                let const_idx = self.add_constant_string(Rc::from(prop_name.as_str()));
                let cache_slot = self.next_cache_slot;
                self.next_cache_slot += 1;
                self.emit(
                    ROp::CallMethod,
                    &[
                        call_dst as u16,
                        base as u16,
                        arguments.len() as u16,
                        const_idx,
                        cache_slot,
                    ],
                );
                self.reload_and_move_result(call_dst, dst);
                return Ok(());
            }
        }

        // Optional method call: obj?.method(args) — short-circuit to undefined when nullish
        if let Expression::OptionalIndex { left, index } = function {
            if let Expression::String(prop_name) = index.as_ref() {
                let base = self.next_temp;
                let obj_r = self.alloc_temp();
                self.compile_expression_into(left, obj_r)?;

                // Check nullish
                let nullish = self.alloc_temp();
                self.emit(ROp::IsNullish, &[nullish as u16, obj_r as u16]);
                let not_null_pos = self.emit(ROp::JumpIfNot, &[nullish as u16, 9999]);
                self.emit(ROp::LoadUndef, &[dst]);
                let end_pos = self.emit(ROp::Jump, &[9999]);

                let call_pos = self.instructions.len();
                self.patch_jump(not_null_pos, call_pos);

                // Not nullish — do the method call
                for (i, arg) in arguments.iter().enumerate() {
                    self.next_temp = (base as usize + 1 + i) as u16;
                    let r = self.alloc_temp();
                    self.compile_expression_into(arg, r)?;
                }
                self.next_temp = (base as usize + 1 + arguments.len()) as u16;
                let const_idx = self.add_constant_string(Rc::from(prop_name.as_str()));
                let cache_slot = self.next_cache_slot;
                self.next_cache_slot += 1;
                self.emit(
                    ROp::CallMethod,
                    &[
                        call_dst as u16,
                        base as u16,
                        arguments.len() as u16,
                        const_idx,
                        cache_slot,
                    ],
                );
                self.reload_and_move_result(call_dst, dst);

                let end = self.instructions.len();
                self.patch_jump(end_pos, end);
                return Ok(());
            }
        }

        // General call: pack func + args in contiguous registers
        let base = self.next_temp;
        let func_r = self.alloc_temp();
        self.compile_expression_into(function, func_r)?;
        // Reset next_temp: func result is in base, internal temps are dead
        for (i, arg) in arguments.iter().enumerate() {
            self.next_temp = (base as usize + 1 + i) as u16;
            let r = self.alloc_temp();
            self.compile_expression_into(arg, r)?;
        }
        self.next_temp = (base as usize + 1 + arguments.len()) as u16;
        self.emit(
            ROp::Call,
            &[call_dst as u16, base as u16, arguments.len() as u16],
        );
        self.reload_and_move_result(call_dst, dst);
        Ok(())
    }

    fn compile_new_into(
        &mut self,
        callee: &Expression,
        arguments: &[Expression],
        dst: u16,
    ) -> Result<(), String> {
        let call_dst = if self.is_local_register(dst) {
            self.alloc_temp()
        } else {
            dst
        };

        if self.arguments_have_spread(arguments) {
            let cls = self.compile_expression(callee)?;
            let args_arr = self.compile_spread_args_array(arguments)?;
            self.emit(
                ROp::NewSpread,
                &[call_dst as u16, cls as u16, args_arr as u16],
            );
            self.reload_and_move_result(call_dst, dst);
            return Ok(());
        }
        let base = self.next_temp;
        let cls_r = self.alloc_temp();
        self.compile_expression_into(callee, cls_r)?;
        // Reset next_temp: callee result is in base, internal temps are dead
        for (i, arg) in arguments.iter().enumerate() {
            self.next_temp = (base as usize + 1 + i) as u16;
            let r = self.alloc_temp();
            self.compile_expression_into(arg, r)?;
        }
        self.next_temp = (base as usize + 1 + arguments.len()) as u16;
        self.emit(
            ROp::New,
            &[call_dst as u16, base as u16, arguments.len() as u16],
        );
        self.reload_and_move_result(call_dst, dst);
        Ok(())
    }

    fn arguments_have_spread(&self, arguments: &[Expression]) -> bool {
        arguments
            .iter()
            .any(|arg| matches!(arg, Expression::Spread { .. }))
    }

    fn compile_spread_args_array(&mut self, arguments: &[Expression]) -> Result<u16, String> {
        let arr = self.alloc_temp();
        self.emit(ROp::Array, &[arr as u16, 0, 0]);
        for arg in arguments {
            match arg {
                Expression::Spread { value } => {
                    let v = self.compile_expression(value)?;
                    self.emit(ROp::AppendSpread, &[arr as u16, v as u16]);
                }
                _ => {
                    let v = self.compile_expression(arg)?;
                    self.emit(ROp::AppendElement, &[arr as u16, v as u16]);
                }
            }
        }
        Ok(arr)
    }

    // ── Control flow ─────────────────────────────────────────────────────

    fn compile_if_expr_into(
        &mut self,
        condition: &Expression,
        consequence: &[Statement],
        alternative: Option<&[Statement]>,
        dst: u16,
    ) -> Result<(), String> {
        // Try fused condition opcodes before falling back to generic path.
        // These combine condition evaluation + conditional jump in one opcode,
        // eliminating 2 extra dispatches per branch check.
        let jump_pos = if let Some((reg, const_idx, is_le)) = self.try_fused_cmp_const(condition) {
            let op = if is_le {
                ROp::TestLeConstJump
            } else {
                ROp::TestLtConstJump
            };
            self.emit(op, &[reg as u16, const_idx, 9999])
        } else if let Some((reg, mod_const, cmp_const)) = self.try_fused_mod_strict_eq(condition) {
            self.emit(
                ROp::ModRegConstStrictEqConstJump,
                &[reg as u16, mod_const, cmp_const, 9999],
            )
        } else {
            let cond = self.compile_expression(condition)?;
            self.emit(ROp::JumpIfNot, &[cond as u16, 9999])
        };

        // Consequence: compile statements with block scoping
        let shadowed = self.enter_block_scope(consequence);
        let mut last = None;
        for stmt in consequence {
            last = self.compile_statement(stmt)?;
        }
        self.exit_block_scope(shadowed);
        if let Some(r) = last {
            if r != dst {
                self.emit(ROp::Move, &[dst as u16, r as u16]);
            }
        }

        let jump_over = self.emit(ROp::Jump, &[9999]);
        let after_cons = self.instructions.len();
        self.patch_jump(jump_pos, after_cons);

        if let Some(alt_block) = alternative {
            let shadowed = self.enter_block_scope(alt_block);
            let mut last = None;
            for stmt in alt_block {
                last = self.compile_statement(stmt)?;
            }
            self.exit_block_scope(shadowed);
            if let Some(r) = last {
                if r != dst {
                    self.emit(ROp::Move, &[dst as u16, r as u16]);
                }
            }
        } else {
            self.emit(ROp::LoadNull, &[dst as u16]);
        }

        let after_alt = self.instructions.len();
        self.patch_jump(jump_over, after_alt);
        Ok(())
    }

    fn compile_while_statement(
        &mut self,
        condition: &Expression,
        body: &[Statement],
        label: Option<&str>,
    ) -> Result<(), String> {
        let loop_start = self.instructions.len();
        let jump_pos = self.compile_loop_condition(condition)?;

        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: loop_start,
            break_positions: vec![],
            continue_positions: vec![],
        });

        // Try to detect `x = x + CONST` or `x += CONST` as last body statement
        // and fuse into IncrementRegAndJump, saving one dispatch per iteration.
        let fused_increment = if let Some(Statement::Expression(expr)) = body.last() {
            self.try_fused_increment(expr)
                .filter(|(_, _, name)| !self.globals.contains_key(*name))
        } else {
            None
        };

        let body_to_compile = if fused_increment.is_some() {
            &body[..body.len() - 1]
        } else {
            body
        };

        let shadowed = self.enter_block_scope(body);
        for stmt in body_to_compile {
            self.compile_statement(stmt)?;
            self.next_temp = self.num_locals; // free temps each iteration
        }
        self.exit_block_scope(shadowed);

        if let Some((reg, const_idx, _)) = fused_increment {
            self.emit(
                ROp::IncrementRegAndJump,
                &[reg as u16, const_idx, loop_start as u16],
            );
        } else {
            self.emit(ROp::Jump, &[loop_start as u16]);
        }
        let loop_end = self.instructions.len();
        self.patch_jump(jump_pos, loop_end);

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.patch_jump(pos, loop_end);
            }
            for pos in ctx.continue_positions {
                self.patch_jump(pos, ctx.continue_target);
            }
        }
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
            continue_target: loop_start,
            break_positions: vec![],
            continue_positions: vec![],
        });

        let shadowed = self.enter_block_scope(body);
        for stmt in body {
            self.compile_statement(stmt)?;
            self.next_temp = self.num_locals;
        }
        self.exit_block_scope(shadowed);

        // continue jumps to the condition
        let condition_start = self.instructions.len();
        if let Some(loop_ctx) = self.loop_stack.last_mut() {
            loop_ctx.continue_target = condition_start;
        }

        let cond_reg = self.compile_expression(condition)?;
        let exit_jump = self.emit(ROp::JumpIfNot, &[cond_reg as u16, 9999]);
        self.emit(ROp::Jump, &[loop_start as u16]);
        let loop_end = self.instructions.len();
        self.patch_jump(exit_jump, loop_end);

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.patch_jump(pos, loop_end);
            }
            for pos in ctx.continue_positions {
                self.patch_jump(pos, ctx.continue_target);
            }
        }
        Ok(())
    }

    fn compile_switch_statement(
        &mut self,
        discriminant: &Expression,
        cases: &[crate::ast::SwitchCase],
        label: Option<&str>,
    ) -> Result<(), String> {
        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: 0,
            break_positions: vec![],
            continue_positions: vec![],
        });

        let disc_reg = self.compile_expression(discriminant)?;

        // Phase 1: Emit case comparisons and jumps
        let mut case_body_jumps: Vec<usize> = Vec::new();
        let mut default_body_idx: Option<usize> = None;

        let saved = self.save_temps();

        for (i, case) in cases.iter().enumerate() {
            if let Some(test) = &case.test {
                let test_reg = self.compile_expression(test)?;
                let cmp_reg = self.alloc_temp();
                self.emit(
                    ROp::StrictEqual,
                    &[cmp_reg as u16, disc_reg as u16, test_reg as u16],
                );
                // Jump to body if equal
                let body_jump = self.emit(ROp::JumpIfTruthy, &[cmp_reg as u16, 9999]);
                case_body_jumps.push(body_jump);
                self.restore_temps(saved);
            } else {
                default_body_idx = Some(i);
            }
        }

        // Jump to default or end
        let default_or_end_jump = self.emit(ROp::Jump, &[9999]);

        // Phase 2: Emit bodies with fall-through
        let mut body_starts: Vec<usize> = Vec::new();
        for case in cases {
            body_starts.push(self.instructions.len());
            for stmt in &case.consequent {
                self.compile_statement(stmt)?;
                self.next_temp = self.num_locals;
            }
        }
        let switch_end = self.instructions.len();

        // Phase 3: Patch
        let mut case_jump_idx = 0;
        for (i, case) in cases.iter().enumerate() {
            if case.test.is_some() {
                self.patch_jump(case_body_jumps[case_jump_idx], body_starts[i]);
                case_jump_idx += 1;
            }
        }

        if let Some(def_idx) = default_body_idx {
            self.patch_jump(default_or_end_jump, body_starts[def_idx]);
        } else {
            self.patch_jump(default_or_end_jump, switch_end);
        }

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.patch_jump(pos, switch_end);
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
        }

        let loop_start = self.instructions.len();

        let jump_pos = if let Some(cond) = condition {
            self.compile_loop_condition(cond)?
        } else {
            // No condition = infinite loop (like `for(;;)`)
            let r = self.alloc_temp();
            self.emit(ROp::LoadTrue, &[r as u16]);
            self.emit(ROp::JumpIfNot, &[r as u16, 9999])
        };

        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: loop_start,
            break_positions: vec![],
            continue_positions: vec![],
        });

        let shadowed = self.enter_block_scope(body);
        for stmt in body {
            self.compile_statement(stmt)?;
            self.next_temp = self.num_locals;
        }
        self.exit_block_scope(shadowed);

        let update_start = self.instructions.len();
        if let Some(loop_ctx) = self.loop_stack.last_mut() {
            loop_ctx.continue_target = update_start;
        }

        // Try fused update+jump: `local += CONST; jump loop_start` → IncrementRegAndJump
        let used_fused_update = if let Some(upd) = update {
            if let Some((reg, const_idx, name)) = self.try_fused_increment(upd) {
                if self.globals.contains_key(name) {
                    // Variable has a global slot — can't use fully-fused IncrementRegAndJump
                    // because reload_locals_from_globals would reset it from the stale global.
                    // Use AddRegConst + SetGlobal + Jump instead.
                    self.emit(ROp::AddRegConst, &[reg as u16, reg as u16, const_idx]);
                    let name_owned = name.to_string();
                    self.mirror_local_to_global(&name_owned, reg);
                    false // fall through to emit Jump below
                } else {
                    self.emit(
                        ROp::IncrementRegAndJump,
                        &[reg as u16, const_idx, loop_start as u16],
                    );
                    true
                }
            } else {
                let _ = self.compile_expression(upd)?;
                self.next_temp = self.num_locals;
                false
            }
        } else {
            false
        };

        if !used_fused_update {
            self.emit(ROp::Jump, &[loop_start as u16]);
        }
        let loop_end = self.instructions.len();
        self.patch_jump(jump_pos, loop_end);

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.patch_jump(pos, loop_end);
            }
            for pos in ctx.continue_positions {
                self.patch_jump(pos, ctx.continue_target);
            }
        }
        Ok(())
    }

    fn compile_for_of_statement(
        &mut self,
        binding: &ForBinding,
        iterable: &Expression,
        body: &[Statement],
        label: Option<&str>,
    ) -> Result<(), String> {
        let iter_name = self.make_temp_name("iter");
        let idx_name = self.make_temp_name("i");

        // iter = Array.from(iterable) — ensures Set, Map, String all become arrays
        let array_from_expr = Expression::Call {
            function: Box::new(Expression::Index {
                left: Box::new(Expression::Identifier("Array".to_string())),
                index: Box::new(Expression::String("from".to_string())),
            }),
            arguments: vec![iterable.clone()],
        };
        let iter_val = self.compile_expression(&array_from_expr)?;
        self.store_identifier(&iter_name, iter_val)?;

        // i = 0
        let idx_r = self.ensure_binding_register(&idx_name)?;
        let zero_idx = self.add_constant_int(0);
        self.emit(ROp::LoadConst, &[idx_r as u16, zero_idx]);
        self.write_binding(&idx_name, idx_r)?;

        let loop_start = self.instructions.len();

        // condition: i < iter.length
        let cond_expr = Expression::Infix {
            left: Box::new(Expression::Identifier(idx_name.clone())),
            operator: "<".to_string(),
            right: Box::new(Expression::Index {
                left: Box::new(Expression::Identifier(iter_name.clone())),
                index: Box::new(Expression::String("length".to_string())),
            }),
        };
        let cond = self.compile_expression(&cond_expr)?;
        let jump_pos = self.emit(ROp::JumpIfNot, &[cond as u16, 9999]);

        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: loop_start,
            break_positions: vec![],
            continue_positions: vec![],
        });

        // item = iter[i]
        let item_expr = Expression::Index {
            left: Box::new(Expression::Identifier(iter_name.clone())),
            index: Box::new(Expression::Identifier(idx_name.clone())),
        };
        let item = self.compile_expression(&item_expr)?;

        match binding {
            ForBinding::Identifier(var_name) => {
                self.store_identifier(var_name, item)?;
            }
            ForBinding::Pattern(pattern) => {
                self.assign_pattern(pattern, item)?;
            }
        }

        let shadowed = self.enter_block_scope(body);
        for stmt in body {
            self.compile_statement(stmt)?;
        }
        self.exit_block_scope(shadowed);

        let update_start = self.instructions.len();
        if let Some(loop_ctx) = self.loop_stack.last_mut() {
            loop_ctx.continue_target = update_start;
        }

        // i += 1
        let update_expr = Expression::Assign {
            left: Box::new(Expression::Identifier(idx_name.clone())),
            operator: "+=".to_string(),
            right: Box::new(Expression::Integer(1)),
        };
        let _ = self.compile_expression(&update_expr)?;

        self.emit(ROp::Jump, &[loop_start as u16]);
        let loop_end = self.instructions.len();
        self.patch_jump(jump_pos, loop_end);

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.patch_jump(pos, loop_end);
            }
            for pos in ctx.continue_positions {
                self.patch_jump(pos, ctx.continue_target);
            }
        }
        Ok(())
    }

    fn compile_for_in_statement(
        &mut self,
        var_name: &str,
        iterable: &Expression,
        body: &[Statement],
        label: Option<&str>,
    ) -> Result<(), String> {
        let keys_name = self.make_temp_name("keys");
        let idx_name = self.make_temp_name("ki");

        // keys = Object.keys(iterable)
        let iter_r = self.compile_expression(iterable)?;
        let keys_r = self.alloc_temp();
        self.emit(ROp::GetKeysIter, &[keys_r as u16, iter_r as u16]);
        self.store_identifier(&keys_name, keys_r)?;

        // i = 0
        let idx_r = self.ensure_binding_register(&idx_name)?;
        let zero_idx = self.add_constant_int(0);
        self.emit(ROp::LoadConst, &[idx_r as u16, zero_idx]);
        self.write_binding(&idx_name, idx_r)?;

        let loop_start = self.instructions.len();

        let cond_expr = Expression::Infix {
            left: Box::new(Expression::Identifier(idx_name.clone())),
            operator: "<".to_string(),
            right: Box::new(Expression::Index {
                left: Box::new(Expression::Identifier(keys_name.clone())),
                index: Box::new(Expression::String("length".to_string())),
            }),
        };
        let cond = self.compile_expression(&cond_expr)?;
        let jump_pos = self.emit(ROp::JumpIfNot, &[cond as u16, 9999]);

        self.loop_stack.push(LoopContext {
            label: label.map(|s| s.to_string()),
            continue_target: loop_start,
            break_positions: vec![],
            continue_positions: vec![],
        });

        let key_expr = Expression::Index {
            left: Box::new(Expression::Identifier(keys_name.clone())),
            index: Box::new(Expression::Identifier(idx_name.clone())),
        };
        let key = self.compile_expression(&key_expr)?;
        self.store_identifier(var_name, key)?;

        let shadowed = self.enter_block_scope(body);
        for stmt in body {
            self.compile_statement(stmt)?;
        }
        self.exit_block_scope(shadowed);

        let update_start = self.instructions.len();
        if let Some(loop_ctx) = self.loop_stack.last_mut() {
            loop_ctx.continue_target = update_start;
        }

        let update_expr = Expression::Assign {
            left: Box::new(Expression::Identifier(idx_name.clone())),
            operator: "+=".to_string(),
            right: Box::new(Expression::Integer(1)),
        };
        let _ = self.compile_expression(&update_expr)?;

        self.emit(ROp::Jump, &[loop_start as u16]);
        let loop_end = self.instructions.len();
        self.patch_jump(jump_pos, loop_end);

        if let Some(ctx) = self.loop_stack.pop() {
            for pos in ctx.break_positions {
                self.patch_jump(pos, loop_end);
            }
            for pos in ctx.continue_positions {
                self.patch_jump(pos, ctx.continue_target);
            }
        }
        Ok(())
    }

    // ── Logical operators ────────────────────────────────────────────────

    fn compile_logical_into(
        &mut self,
        left: &Expression,
        operator: &str,
        right: &Expression,
        dst: u16,
    ) -> Result<(), String> {
        let l = self.compile_expression(left)?;

        match operator {
            "||" => {
                // If left is truthy, result is left; otherwise evaluate right
                if l != dst {
                    self.emit(ROp::Move, &[dst as u16, l as u16]);
                }
                let end_pos = self.emit(ROp::JumpIfTruthy, &[dst as u16, 9999]);
                self.compile_expression_into(right, dst)?;
                let end = self.instructions.len();
                self.patch_jump(end_pos, end);
            }
            "&&" => {
                // If left is falsy, result is left; otherwise evaluate right
                if l != dst {
                    self.emit(ROp::Move, &[dst as u16, l as u16]);
                }
                let end_pos = self.emit(ROp::JumpIfNot, &[dst as u16, 9999]);
                self.compile_expression_into(right, dst)?;
                let end = self.instructions.len();
                self.patch_jump(end_pos, end);
            }
            "??" => {
                // If left is nullish, evaluate right; otherwise result is left
                if l != dst {
                    self.emit(ROp::Move, &[dst as u16, l as u16]);
                }
                let nullish = self.alloc_temp();
                self.emit(ROp::IsNullish, &[nullish as u16, dst as u16]);
                let use_left_pos = self.emit(ROp::JumpIfNot, &[nullish as u16, 9999]);
                self.compile_expression_into(right, dst)?;
                let end = self.instructions.len();
                self.patch_jump(use_left_pos, end);
            }
            _ => return Err(format!("unsupported logical operator {}", operator)),
        }
        Ok(())
    }

    // ── Optional chaining ────────────────────────────────────────────────

    fn compile_optional_index_into(
        &mut self,
        left: &Expression,
        index: &Expression,
        dst: u16,
    ) -> Result<(), String> {
        let obj = self.compile_expression(left)?;
        let nullish = self.alloc_temp();
        self.emit(ROp::IsNullish, &[nullish as u16, obj as u16]);
        let not_null_pos = self.emit(ROp::JumpIfNot, &[nullish as u16, 9999]);
        self.emit(ROp::LoadUndef, &[dst as u16]);
        let end_pos = self.emit(ROp::Jump, &[9999]);

        let access_pos = self.instructions.len();
        self.patch_jump(not_null_pos, access_pos);

        // Do the index access from obj
        if let Expression::String(prop) = index {
            let const_idx = self.add_constant_string(Rc::from(prop.as_str()));
            let cache_slot = self.next_cache_slot;
            self.next_cache_slot += 1;
            self.emit(
                ROp::GetProp,
                &[dst as u16, obj as u16, const_idx, cache_slot],
            );
        } else {
            let key = self.compile_expression(index)?;
            self.emit(ROp::Index, &[dst as u16, obj as u16, key as u16]);
        }

        let end = self.instructions.len();
        self.patch_jump(end_pos, end);
        Ok(())
    }

    fn compile_optional_call_into(
        &mut self,
        function: &Expression,
        arguments: &[Expression],
        dst: u16,
    ) -> Result<(), String> {
        let call_dst = if self.is_local_register(dst) {
            self.alloc_temp()
        } else {
            dst
        };

        let func = self.compile_expression(function)?;
        let nullish = self.alloc_temp();
        self.emit(ROp::IsNullish, &[nullish as u16, func as u16]);
        let not_null_pos = self.emit(ROp::JumpIfNot, &[nullish as u16, 9999]);
        // Null case: write undefined directly to final dst (not call_dst)
        // so the result is correct when we jump past the call+move path.
        self.emit(ROp::LoadUndef, &[dst]);
        let end_pos = self.emit(ROp::Jump, &[9999]);

        let call_pos = self.instructions.len();
        self.patch_jump(not_null_pos, call_pos);

        if self.arguments_have_spread(arguments) {
            let args_arr = self.compile_spread_args_array(arguments)?;
            self.emit(
                ROp::CallSpread,
                &[call_dst as u16, func as u16, args_arr as u16],
            );
        } else {
            // We already have func in a register. Put it at base, args after.
            let base = func;
            let saved = self.save_temps();
            for (i, arg) in arguments.iter().enumerate() {
                self.next_temp = (base as usize + 1 + i) as u16;
                let r = self.alloc_temp();
                self.compile_expression_into(arg, r)?;
            }
            self.next_temp = (base as usize + 1 + arguments.len()) as u16;
            self.emit(
                ROp::Call,
                &[call_dst as u16, base as u16, arguments.len() as u16],
            );
            self.restore_temps(saved);
        }
        self.reload_and_move_result(call_dst, dst);

        let end = self.instructions.len();
        self.patch_jump(end_pos, end);
        Ok(())
    }

    // ── Assignment ───────────────────────────────────────────────────────

    fn compile_assignment_into(
        &mut self,
        left: &Expression,
        operator: &str,
        right: &Expression,
        dst: u16,
    ) -> Result<(), String> {
        match left {
            Expression::Identifier(name) => {
                self.compile_ident_assignment_into(name, operator, right, dst)
            }
            Expression::Index {
                left: object_expr,
                index,
            } => self.compile_index_assignment_into(object_expr, index, operator, right, dst),
            Expression::Array(items) => {
                if operator != "=" {
                    return Err("only '=' supported for array destructuring assignment".to_string());
                }
                let src = self.compile_expression(right)?;
                self.destructure_array_assignment(items, src)?;
                // Skip Move if dst was claimed by a new local during destructuring
                // (ensure_local can allocate the same register as an earlier temp).
                if dst != src && !self.locals.values().any(|&r| r == dst) {
                    self.emit(ROp::Move, &[dst as u16, src as u16]);
                }
                Ok(())
            }
            Expression::Hash(pairs) => {
                if operator != "=" {
                    return Err(
                        "only '=' supported for object destructuring assignment".to_string()
                    );
                }
                let src = self.compile_expression(right)?;
                self.destructure_object_assignment(pairs, src)?;
                // Skip Move if dst was claimed by a new local during destructuring.
                if dst != src && !self.locals.values().any(|&r| r == dst) {
                    self.emit(ROp::Move, &[dst as u16, src as u16]);
                }
                Ok(())
            }
            _ => Err("invalid assignment target".to_string()),
        }
    }

    fn compile_ident_assignment_into(
        &mut self,
        name: &str,
        operator: &str,
        right: &Expression,
        dst: u16,
    ) -> Result<(), String> {
        if self.const_bindings.contains(name) {
            return Err(format!("Assignment to constant variable '{}'", name));
        }
        if operator == "&&=" || operator == "||=" || operator == "??=" {
            return self.compile_logical_assignment_ident(name, operator, right, dst);
        }

        if operator == "=" {
            // Fused: local = local + CONST → AddRegConst
            if let Some(&r) = self.locals.get(name) {
                if let Expression::Infix {
                    left: inner_left,
                    operator: inner_op,
                    right: inner_right,
                } = right
                {
                    if inner_op == "+" {
                        if let Expression::Identifier(inner_name) = inner_left.as_ref() {
                            if inner_name == name {
                                if let Some(const_idx) = self.try_numeric_const(inner_right) {
                                    self.emit(ROp::AddRegConst, &[r as u16, r as u16, const_idx]);
                                    self.mirror_local_to_global(name, r);
                                    if dst != r {
                                        self.emit(ROp::Move, &[dst as u16, r as u16]);
                                    }
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }

            if let Some(&r) = self.locals.get(name) {
                // Compile directly into target register to avoid extra Move
                self.compile_expression_into(right, r)?;
                self.mirror_local_to_global(name, r);
                if dst != r {
                    self.emit(ROp::Move, &[dst as u16, r as u16]);
                }
            } else {
                let val = self.compile_expression(right)?;
                let g = self.ensure_global_slot(name)?;
                self.emit(ROp::SetGlobal, &[g, val as u16]);
                if dst != val {
                    self.emit(ROp::Move, &[dst as u16, val as u16]);
                }
            }
            return Ok(());
        }

        // Fused: local += CONST → AddRegConst
        if operator == "+=" {
            if let Some(&r) = self.locals.get(name) {
                if let Some(const_idx) = self.try_numeric_const(right) {
                    self.emit(ROp::AddRegConst, &[r as u16, r as u16, const_idx]);
                    self.mirror_local_to_global(name, r);
                    if dst != r {
                        self.emit(ROp::Move, &[dst as u16, r as u16]);
                    }
                    return Ok(());
                }
            }
        }

        // Compound assignment: ident op= right
        let base_op = match operator {
            "+=" => ROp::Add,
            "-=" => ROp::Sub,
            "*=" => ROp::Mul,
            "/=" => ROp::Div,
            "%=" => ROp::Mod,
            "**=" => ROp::Pow,
            "&=" => ROp::BitwiseAnd,
            "|=" => ROp::BitwiseOr,
            "^=" => ROp::BitwiseXor,
            "<<=" => ROp::LeftShift,
            ">>=" => ROp::RightShift,
            ">>>=" => ROp::UnsignedRightShift,
            _ => return Err(format!("unsupported assignment operator {}", operator)),
        };

        // Load current value
        let cur = self.alloc_temp();
        self.load_identifier_into(name, cur)?;

        // Compute right side
        let rhs = self.compile_expression(right)?;

        // Compute result
        let result = self.alloc_temp();
        self.emit(base_op, &[result as u16, cur as u16, rhs as u16]);

        // Store back
        if let Some(&r) = self.locals.get(name) {
            if r != result {
                self.emit(ROp::Move, &[r as u16, result as u16]);
            }
            self.mirror_local_to_global(name, r);
            if dst != r {
                self.emit(ROp::Move, &[dst as u16, r as u16]);
            }
        } else {
            let g = self.ensure_global_slot(name)?;
            self.emit(ROp::SetGlobal, &[g, result as u16]);
            if dst != result {
                self.emit(ROp::Move, &[dst as u16, result as u16]);
            }
        }
        Ok(())
    }

    fn compile_index_assignment_into(
        &mut self,
        object_expr: &Expression,
        index: &Expression,
        operator: &str,
        right: &Expression,
        dst: u16,
    ) -> Result<(), String> {
        // Fused: obj.prop = obj.prop + CONST → AddConstToRegProp
        // Recognizes the non-compound form and emits the same fused opcode.
        // Must be checked BEFORE generic SetProp (more specific pattern).
        if operator == "=" {
            if let Expression::String(prop) = index {
                if let Expression::Identifier(obj_name) = object_expr {
                    if let Some(&obj_r) = self.locals.get(obj_name.as_str()) {
                        if let Expression::Infix {
                            left: inner_left,
                            operator: ref inner_op,
                            right: inner_right,
                        } = right
                        {
                            if inner_op == "+" {
                                // Check: inner_left is same_obj.same_prop
                                let left_matches = if let Expression::Index {
                                    left: src_obj,
                                    index: src_idx,
                                } = inner_left.as_ref()
                                {
                                    matches!(src_obj.as_ref(), Expression::Identifier(n) if n == obj_name)
                                        && matches!(src_idx.as_ref(), Expression::String(p) if p == prop)
                                } else {
                                    false
                                };
                                if left_matches {
                                    if let Some(val_const) = self.try_numeric_const(inner_right) {
                                        let prop_const =
                                            self.add_constant_string(Rc::from(prop.as_str()));
                                        let cache_slot = self.next_cache_slot;
                                        self.next_cache_slot += 1;
                                        self.emit(
                                            ROp::AddConstToRegProp,
                                            &[obj_r as u16, prop_const, val_const, cache_slot],
                                        );
                                        if dst != obj_r {
                                            let prop_c2 =
                                                self.add_constant_string(Rc::from(prop.as_str()));
                                            let cache2 = self.next_cache_slot;
                                            self.next_cache_slot += 1;
                                            self.emit(
                                                ROp::GetProp,
                                                &[dst as u16, obj_r as u16, prop_c2, cache2],
                                            );
                                        }
                                        return Ok(());
                                    }
                                }
                                // Also check: inner_right is same_obj.same_prop (CONST + obj.prop)
                                let right_matches = if let Expression::Index {
                                    left: src_obj,
                                    index: src_idx,
                                } = inner_right.as_ref()
                                {
                                    matches!(src_obj.as_ref(), Expression::Identifier(n) if n == obj_name)
                                        && matches!(src_idx.as_ref(), Expression::String(p) if p == prop)
                                } else {
                                    false
                                };
                                if right_matches {
                                    if let Some(val_const) = self.try_numeric_const(inner_left) {
                                        let prop_const =
                                            self.add_constant_string(Rc::from(prop.as_str()));
                                        let cache_slot = self.next_cache_slot;
                                        self.next_cache_slot += 1;
                                        self.emit(
                                            ROp::AddConstToRegProp,
                                            &[obj_r as u16, prop_const, val_const, cache_slot],
                                        );
                                        if dst != obj_r {
                                            let prop_c2 =
                                                self.add_constant_string(Rc::from(prop.as_str()));
                                            let cache2 = self.next_cache_slot;
                                            self.next_cache_slot += 1;
                                            self.emit(
                                                ROp::GetProp,
                                                &[dst as u16, obj_r as u16, prop_c2, cache2],
                                            );
                                        }
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fused: obj.prop += CONST → AddConstToRegProp
        if operator == "+=" {
            if let Expression::String(prop) = index {
                let obj_name: Option<&str> = match object_expr {
                    Expression::Identifier(name) => Some(name.as_str()),
                    Expression::This => Some("this"),
                    _ => None,
                };
                if let Some(name) = obj_name {
                    if let Some(&obj_r) = self.locals.get(name) {
                        if let Some(val_const) = self.try_numeric_const(right) {
                            let prop_const = self.add_constant_string(Rc::from(prop.as_str()));
                            let cache_slot = self.next_cache_slot;
                            self.next_cache_slot += 1;
                            self.emit(
                                ROp::AddConstToRegProp,
                                &[obj_r as u16, prop_const, val_const, cache_slot],
                            );
                            // Result is the new property value; fetch it for dst
                            if dst != obj_r {
                                let prop_c2 = self.add_constant_string(Rc::from(prop.as_str()));
                                let cache2 = self.next_cache_slot;
                                self.next_cache_slot += 1;
                                self.emit(
                                    ROp::GetProp,
                                    &[dst as u16, obj_r as u16, prop_c2, cache2],
                                );
                            }
                            return Ok(());
                        }
                    }
                }
            }
        }

        // Fused: obj.z = obj.x + obj.y → AddRegPropsToRegProp
        // Must be checked BEFORE generic SetProp (more specific pattern).
        if operator == "=" {
            if let Expression::String(dst_prop) = index {
                if let Expression::Identifier(obj_name) = object_expr {
                    if let Some(&obj_r) = self.locals.get(obj_name.as_str()) {
                        if let Expression::Infix {
                            left: add_left,
                            operator: add_op,
                            right: add_right,
                        } = right
                        {
                            if add_op == "+" {
                                if let (
                                    Expression::Index {
                                        left: s1_obj,
                                        index: s1_idx,
                                    },
                                    Expression::Index {
                                        left: s2_obj,
                                        index: s2_idx,
                                    },
                                ) = (add_left.as_ref(), add_right.as_ref())
                                {
                                    let s1_same = matches!(s1_obj.as_ref(),
                                        Expression::Identifier(n) if n == obj_name);
                                    let s2_same = matches!(s2_obj.as_ref(),
                                        Expression::Identifier(n) if n == obj_name);
                                    if s1_same && s2_same {
                                        if let (
                                            Expression::String(s1_prop),
                                            Expression::String(s2_prop),
                                        ) = (s1_idx.as_ref(), s2_idx.as_ref())
                                        {
                                            let s1_const = self
                                                .add_constant_string(Rc::from(s1_prop.as_str()));
                                            let s1_cache = self.next_cache_slot;
                                            self.next_cache_slot += 1;
                                            let s2_const = self
                                                .add_constant_string(Rc::from(s2_prop.as_str()));
                                            let s2_cache = self.next_cache_slot;
                                            self.next_cache_slot += 1;
                                            let dst_const = self
                                                .add_constant_string(Rc::from(dst_prop.as_str()));
                                            let dst_cache = self.next_cache_slot;
                                            self.next_cache_slot += 1;
                                            self.emit(
                                                ROp::AddRegPropsToRegProp,
                                                &[
                                                    obj_r as u16,
                                                    s1_const,
                                                    s1_cache,
                                                    s2_const,
                                                    s2_cache,
                                                    dst_const,
                                                    dst_cache,
                                                ],
                                            );
                                            return Ok(());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Generic: obj.prop = value with inline cache (fallback after fused checks)
        if operator == "=" {
            if let Expression::String(prop) = index {
                let obj_name: Option<&str> = match object_expr {
                    Expression::Identifier(name) => Some(name.as_str()),
                    Expression::This => Some("this"),
                    _ => None,
                };
                if let Some(name) = obj_name {
                    let is_local = self.locals.contains_key(name);
                    if is_local {
                        let obj_r = *self.locals.get(name).unwrap();
                        let const_idx = self.add_constant_string(Rc::from(prop.as_str()));
                        let cache_slot = self.next_cache_slot;
                        self.next_cache_slot += 1;
                        self.compile_expression_into(right, dst)?;
                        self.emit(
                            ROp::SetProp,
                            &[obj_r as u16, const_idx, dst as u16, cache_slot],
                        );
                        return Ok(());
                    }
                    if let Some(&global_idx) = self.globals.get(name) {
                        let const_idx = self.add_constant_string(Rc::from(prop.as_str()));
                        let cache_slot = self.next_cache_slot;
                        self.next_cache_slot += 1;
                        self.compile_expression_into(right, dst)?;
                        self.emit(
                            ROp::SetGlobalProp,
                            &[global_idx, const_idx, dst as u16, cache_slot],
                        );
                        return Ok(());
                    }
                }
            }
        }

        // Handle short-circuit logical assignment operators for index targets
        if operator == "||=" || operator == "&&=" || operator == "??=" {
            let obj = self.compile_expression(object_expr)?;
            let key = self.compile_expression(index)?;
            let old = self.alloc_temp();
            self.emit(ROp::Index, &[old as u16, obj as u16, key as u16]);

            let skip_jump = match operator {
                "||=" => self.emit(ROp::JumpIfTruthy, &[old as u16, 9999]),
                "&&=" => self.emit(ROp::JumpIfNot, &[old as u16, 9999]),
                "??=" => {
                    // IsNullish then JumpIfNot (skip if NOT nullish, i.e. keep existing value)
                    let is_null = self.alloc_temp();
                    self.emit(ROp::IsNullish, &[is_null as u16, old as u16]);
                    self.emit(ROp::JumpIfNot, &[is_null as u16, 9999])
                }
                _ => unreachable!(),
            };
            let rhs = self.compile_expression(right)?;
            self.emit(ROp::SetIndex, &[obj as u16, key as u16, rhs as u16]);
            // Write back to original local if needed
            let orig_local = match object_expr {
                Expression::Identifier(name) => self.locals.get(name.as_str()).copied(),
                Expression::This => self.locals.get("this").copied(),
                _ => None,
            };
            if let Some(local_r) = orig_local {
                if local_r as u16 != obj as u16 {
                    self.emit(ROp::Move, &[local_r as u16, obj as u16]);
                }
            }
            self.emit(ROp::Move, &[dst, rhs as u16]);
            let end = self.instructions.len();
            self.patch_jump(skip_jump, end);
            // If we skipped, dst should be old value
            self.emit(ROp::Move, &[dst, old as u16]);
            return Ok(());
        }

        // General case: obj[key] op= right
        let base_op = match operator {
            "=" => None,
            "+=" => Some(ROp::Add),
            "-=" => Some(ROp::Sub),
            "*=" => Some(ROp::Mul),
            "/=" => Some(ROp::Div),
            "%=" => Some(ROp::Mod),
            "**=" => Some(ROp::Pow),
            "&=" => Some(ROp::BitwiseAnd),
            "|=" => Some(ROp::BitwiseOr),
            "^=" => Some(ROp::BitwiseXor),
            "<<=" => Some(ROp::LeftShift),
            ">>=" => Some(ROp::RightShift),
            ">>>=" => Some(ROp::UnsignedRightShift),
            _ => {
                return Err(format!(
                    "unsupported assignment operator {} for index target",
                    operator
                ))
            }
        };

        let obj = self.compile_expression(object_expr)?;
        let key = self.compile_expression(index)?;

        let val = if let Some(op) = base_op {
            let old = self.alloc_temp();
            self.emit(ROp::Index, &[old as u16, obj as u16, key as u16]);
            let rhs = self.compile_expression(right)?;
            let result = self.alloc_temp();
            self.emit(op, &[result as u16, old as u16, rhs as u16]);
            result
        } else {
            self.compile_expression(right)?
        };

        self.emit(ROp::SetIndex, &[obj as u16, key as u16, val as u16]);

        // Write updated object back to its original local register.
        // SetIndex stores the updated object in register `obj` (a temp),
        // but the original local (e.g., `this`) isn't updated unless we copy back.
        let orig_local = match object_expr {
            Expression::Identifier(name) => self.locals.get(name.as_str()).copied(),
            Expression::This => self.locals.get("this").copied(),
            _ => None,
        };
        if let Some(local_r) = orig_local {
            if obj != local_r {
                self.emit(ROp::Move, &[local_r as u16, obj as u16]);
            }
        }

        if dst != val {
            self.emit(ROp::Move, &[dst as u16, val as u16]);
        }
        Ok(())
    }

    fn compile_logical_assignment_ident(
        &mut self,
        name: &str,
        operator: &str,
        right: &Expression,
        dst: u16,
    ) -> Result<(), String> {
        let cur = self.alloc_temp();
        self.load_identifier_into(name, cur)?;

        match operator {
            "&&=" => {
                if cur != dst {
                    self.emit(ROp::Move, &[dst as u16, cur as u16]);
                }
                let keep_pos = self.emit(ROp::JumpIfNot, &[dst as u16, 9999]);
                self.compile_expression_into(right, dst)?;
                // Store back
                if let Some(&r) = self.locals.get(name) {
                    if r != dst {
                        self.emit(ROp::Move, &[r as u16, dst as u16]);
                    }
                    self.mirror_local_to_global(name, r);
                } else {
                    let g = self.ensure_global_slot(name)?;
                    self.emit(ROp::SetGlobal, &[g, dst as u16]);
                }
                let end = self.instructions.len();
                self.patch_jump(keep_pos, end);
            }
            "||=" => {
                if cur != dst {
                    self.emit(ROp::Move, &[dst as u16, cur as u16]);
                }
                let keep_pos = self.emit(ROp::JumpIfTruthy, &[dst as u16, 9999]);
                self.compile_expression_into(right, dst)?;
                if let Some(&r) = self.locals.get(name) {
                    if r != dst {
                        self.emit(ROp::Move, &[r as u16, dst as u16]);
                    }
                    self.mirror_local_to_global(name, r);
                } else {
                    let g = self.ensure_global_slot(name)?;
                    self.emit(ROp::SetGlobal, &[g, dst as u16]);
                }
                let end = self.instructions.len();
                self.patch_jump(keep_pos, end);
            }
            "??=" => {
                let nullish = self.alloc_temp();
                self.emit(ROp::IsNullish, &[nullish as u16, cur as u16]);
                let keep_pos = self.emit(ROp::JumpIfNot, &[nullish as u16, 9999]);
                self.compile_expression_into(right, dst)?;
                if let Some(&r) = self.locals.get(name) {
                    if r != dst {
                        self.emit(ROp::Move, &[r as u16, dst as u16]);
                    }
                    self.mirror_local_to_global(name, r);
                } else {
                    let g = self.ensure_global_slot(name)?;
                    self.emit(ROp::SetGlobal, &[g, dst as u16]);
                }
                let end_pos = self.emit(ROp::Jump, &[9999]);
                let keep = self.instructions.len();
                self.patch_jump(keep_pos, keep);
                if cur != dst {
                    self.emit(ROp::Move, &[dst as u16, cur as u16]);
                }
                let end = self.instructions.len();
                self.patch_jump(end_pos, end);
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

    // ── Destructuring ────────────────────────────────────────────────────

    fn assign_pattern(&mut self, pattern: &BindingPattern, src: u16) -> Result<(), String> {
        match pattern {
            BindingPattern::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    match item {
                        ArrayBindingItem::Hole => continue,
                        ArrayBindingItem::Binding {
                            target,
                            default_value,
                        } => {
                            let key = self.alloc_temp();
                            let key_idx = self.add_constant_int(i as i64);
                            self.emit(ROp::LoadConst, &[key as u16, key_idx]);
                            let val = self.alloc_temp();
                            self.emit(ROp::Index, &[val as u16, src as u16, key as u16]);
                            let val = self.apply_default(val, default_value.as_ref())?;
                            self.assign_binding_target(target, val)?;
                        }
                        ArrayBindingItem::Rest { name } => {
                            let rest = self.alloc_temp();
                            self.emit(ROp::IteratorRest, &[rest as u16, src as u16, i as u16]);
                            self.store_identifier(name, rest)?;
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
                        let rest = self.compile_object_rest(src, &excluded_keys)?;
                        let BindingTarget::Identifier(name) = target else {
                            return Err("object rest target must be identifier".to_string());
                        };
                        self.store_identifier(name, rest)?;
                        continue;
                    }
                    let key_r = self.compile_expression(key)?;
                    let val = self.alloc_temp();
                    self.emit(ROp::Index, &[val as u16, src as u16, key_r as u16]);
                    let val = self.apply_default(val, default_value.as_ref())?;
                    self.assign_binding_target(target, val)?;
                }
            }
        }
        Ok(())
    }

    fn assign_binding_target(&mut self, target: &BindingTarget, src: u16) -> Result<(), String> {
        match target {
            BindingTarget::Identifier(name) => self.store_identifier(name, src),
            BindingTarget::Pattern(pattern) => self.assign_pattern(pattern, src),
        }
    }

    /// Pre-declare all identifiers in a destructuring pattern as locals.
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
                Statement::Block(body)
                | Statement::MultiLet(body)
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
                _ => {}
            }
        }
    }

    /// This ensures their registers are allocated before any temp registers,
    /// preventing ensure_local from claiming a temp that holds the source object.
    fn pre_declare_pattern_locals(&mut self, pattern: &BindingPattern) {
        if !self.is_function_scope {
            return;
        }
        match pattern {
            BindingPattern::Array(items) => {
                for item in items {
                    match item {
                        ArrayBindingItem::Binding { target, .. } => {
                            self.pre_declare_target_locals(target);
                        }
                        ArrayBindingItem::Rest { name } => {
                            self.ensure_local(name);
                        }
                        ArrayBindingItem::Hole => {}
                    }
                }
            }
            BindingPattern::Object(pairs) => {
                for pair in pairs {
                    self.pre_declare_target_locals(&pair.target);
                }
            }
        }
    }

    fn pre_declare_target_locals(&mut self, target: &BindingTarget) {
        match target {
            BindingTarget::Identifier(name) => {
                self.ensure_local(name);
            }
            BindingTarget::Pattern(pattern) => {
                self.pre_declare_pattern_locals(pattern);
            }
        }
    }

    fn apply_default(
        &mut self,
        val: u16,
        default_value: Option<&Expression>,
    ) -> Result<u16, String> {
        let Some(default_expr) = default_value else {
            return Ok(val);
        };
        // Check if val is undefined
        let typeof_r = self.alloc_temp();
        self.emit(ROp::Typeof, &[typeof_r as u16, val as u16]);
        let undef_str = self.add_constant_string(Rc::from("undefined"));
        let undef_r = self.alloc_temp();
        self.emit(ROp::LoadConst, &[undef_r as u16, undef_str]);
        let cmp = self.alloc_temp();
        self.emit(ROp::Equal, &[cmp as u16, typeof_r as u16, undef_r as u16]);
        let skip_pos = self.emit(ROp::JumpIfNot, &[cmp as u16, 9999]);

        // val is undefined, use default
        self.compile_expression_into(default_expr, val)?;

        let end = self.instructions.len();
        self.patch_jump(skip_pos, end);
        Ok(val)
    }

    fn compile_object_rest(
        &mut self,
        src: u16,
        excluded_keys: &[Expression],
    ) -> Result<u16, String> {
        let keys_base = self.next_temp;
        for key in excluded_keys {
            let r = self.alloc_temp();
            self.compile_expression_into(key, r)?;
        }
        let dst = self.alloc_temp();
        self.emit(
            ROp::ObjectRest,
            &[
                dst as u16,
                src as u16,
                keys_base as u16,
                excluded_keys.len() as u16,
            ],
        );
        Ok(dst)
    }

    fn destructure_array_assignment(
        &mut self,
        items: &[Expression],
        src: u16,
    ) -> Result<(), String> {
        for (i, item) in items.iter().enumerate() {
            match item {
                Expression::Spread { value } => {
                    if i + 1 != items.len() {
                        return Err("array rest element in assignment must be last".to_string());
                    }
                    if let Expression::Identifier(name) = &**value {
                        let rest = self.alloc_temp();
                        self.emit(ROp::IteratorRest, &[rest as u16, src as u16, i as u16]);
                        self.store_identifier(name, rest)?;
                    } else {
                        return Err("array rest assignment requires identifier target".to_string());
                    }
                }
                Expression::Identifier(name) => {
                    let key = self.alloc_temp();
                    let key_idx = self.add_constant_int(i as i64);
                    self.emit(ROp::LoadConst, &[key as u16, key_idx]);
                    let val = self.alloc_temp();
                    self.emit(ROp::Index, &[val as u16, src as u16, key as u16]);
                    self.store_identifier(name, val)?;
                }
                Expression::Assign {
                    left,
                    operator,
                    right,
                } if operator == "=" => {
                    let key = self.alloc_temp();
                    let key_idx = self.add_constant_int(i as i64);
                    self.emit(ROp::LoadConst, &[key as u16, key_idx]);
                    let val = self.alloc_temp();
                    self.emit(ROp::Index, &[val as u16, src as u16, key as u16]);
                    let val = self.apply_default(val, Some(right.as_ref()))?;
                    match &**left {
                        Expression::Identifier(name) => {
                            self.store_identifier(name, val)?;
                        }
                        Expression::Array(_) | Expression::Hash(_) => {
                            let tmp = self.alloc_temp();
                            if val != tmp {
                                self.emit(ROp::Move, &[tmp as u16, val as u16]);
                            }
                            self.compile_assignment_into(
                                left,
                                "=",
                                &Expression::Null, // placeholder, won't be used
                                tmp,
                            )?;
                        }
                        _ => {
                            return Err(
                                "array destructuring supports identifier or nested pattern targets"
                                    .to_string(),
                            );
                        }
                    }
                }
                Expression::Array(_) | Expression::Hash(_) => {
                    let key = self.alloc_temp();
                    let key_idx = self.add_constant_int(i as i64);
                    self.emit(ROp::LoadConst, &[key as u16, key_idx]);
                    let val = self.alloc_temp();
                    self.emit(ROp::Index, &[val as u16, src as u16, key as u16]);
                    // Store to temp, then destructure
                    let tmp_name = self.make_temp_name("arr_nested");
                    self.store_identifier(&tmp_name, val)?;
                    let _r = self.compile_assignment_into(
                        item,
                        "=",
                        &Expression::Identifier(tmp_name),
                        val,
                    )?;
                }
                _ => {
                    return Err(
                        "array destructuring assignment supports identifier targets".to_string()
                    );
                }
            }
        }
        Ok(())
    }

    fn destructure_object_assignment(
        &mut self,
        pairs: &[HashEntry],
        src: u16,
    ) -> Result<(), String> {
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
                        _ => {
                            return Err(
                                "object rest destructuring requires identifier target".to_string()
                            )
                        }
                    };
                    let rest = self.compile_object_rest(src, &excluded_keys)?;
                    self.store_identifier(name, rest)?;
                }
                HashEntry::KeyValue {
                    key: key_expr,
                    value: target_expr,
                } => {
                    let key_r = self.compile_expression(key_expr)?;
                    let val = self.alloc_temp();
                    self.emit(ROp::Index, &[val as u16, src as u16, key_r as u16]);

                    match target_expr {
                        Expression::Identifier(name) => {
                            self.store_identifier(name, val)?;
                        }
                        Expression::Assign {
                            left,
                            operator,
                            right,
                        } if operator == "=" => {
                            let val = self.apply_default(val, Some(right.as_ref()))?;
                            match &**left {
                                Expression::Identifier(name) => {
                                    self.store_identifier(name, val)?;
                                }
                                _ => {
                                    let tmp_name = self.make_temp_name("obj_nested");
                                    self.store_identifier(&tmp_name, val)?;
                                    let tmp = self.alloc_temp();
                                    self.compile_assignment_into(
                                        left,
                                        "=",
                                        &Expression::Identifier(tmp_name),
                                        tmp,
                                    )?;
                                }
                            }
                        }
                        Expression::Array(_) | Expression::Hash(_) => {
                            let tmp_name = self.make_temp_name("obj_nested");
                            self.store_identifier(&tmp_name, val)?;
                            let tmp = self.alloc_temp();
                            self.compile_assignment_into(
                                target_expr,
                                "=",
                                &Expression::Identifier(tmp_name),
                                tmp,
                            )?;
                        }
                        _ => {
                            return Err(
                                "object destructuring supports identifier targets".to_string()
                            );
                        }
                    }
                }
                HashEntry::Method { .. } | HashEntry::Getter { .. } | HashEntry::Setter { .. } => {
                    return Err(
                        "methods/getters/setters not valid in destructuring assignment".to_string(),
                    );
                }
            }
        }
        Ok(())
    }

    // ── Try/Throw ────────────────────────────────────────────────────────

    fn compile_throw_statement(&mut self, value: &Expression) -> Result<(), String> {
        if self.try_stack.is_empty() {
            let r = self.compile_expression(value)?;
            self.emit(ROp::Throw, &[r as u16]);
            return Ok(());
        }

        let r = self.compile_expression(value)?;
        let exception_temp = self
            .try_stack
            .last()
            .map(|ctx| ctx.exception_temp.clone())
            .unwrap_or_else(|| self.make_temp_name("exc_fallback"));
        self.store_identifier(&exception_temp, r)?;

        let jump_pos = self.emit(ROp::Jump, &[9999]);
        if let Some(ctx) = self.try_stack.last_mut() {
            ctx.throw_jumps.push(jump_pos);
        }
        Ok(())
    }

    fn compile_try_statement(
        &mut self,
        try_block: &[Statement],
        catch_param: Option<&str>,
        catch_block: Option<&[Statement]>,
        finally_block: Option<&[Statement]>,
    ) -> Result<Option<u16>, String> {
        let exception_temp = self.make_temp_name("exc");
        // Initialize exception temp to null
        let null_r = self.alloc_temp();
        self.emit(ROp::LoadNull, &[null_r as u16]);
        self.store_identifier(&exception_temp, null_r)?;

        let has_finally = finally_block.is_some();
        let (return_temp, return_flag_temp) = if has_finally {
            let rt = self.make_temp_name("ret_val");
            let rf = self.make_temp_name("ret_flag");
            // Initialize return flag to false
            let false_r = self.alloc_temp();
            self.emit(ROp::LoadFalse, &[false_r as u16]);
            self.store_identifier(&rf, false_r)?;
            (Some(rt), Some(rf))
        } else {
            (None, None)
        };

        self.try_stack.push(TryContext {
            exception_temp: exception_temp.clone(),
            throw_jumps: vec![],
            has_finally,
            return_temp: return_temp.clone(),
            return_flag_temp: return_flag_temp.clone(),
            return_jumps: vec![],
        });

        let mut last_reg: Option<u16> = None;
        for stmt in try_block {
            if let Some(r) = self.compile_statement(stmt)? {
                last_reg = Some(r);
            }
        }

        let jump_after_try = self.emit(ROp::Jump, &[9999]);

        let ctx = self
            .try_stack
            .pop()
            .ok_or_else(|| "internal error: missing try context".to_string())?;

        let catch_start = self.instructions.len();
        for pos in ctx.throw_jumps {
            self.patch_jump(pos, catch_start);
        }

        // Keep return_jumps from the try block
        let mut all_return_jumps = ctx.return_jumps;

        if let Some(catch_stmts) = catch_block {
            // Push a context for the catch block so returns inside catch are
            // also deferred to the finally block.
            if has_finally {
                self.try_stack.push(TryContext {
                    exception_temp: exception_temp.clone(),
                    throw_jumps: vec![],
                    has_finally: true,
                    return_temp: return_temp.clone(),
                    return_flag_temp: return_flag_temp.clone(),
                    return_jumps: vec![],
                });
            }
            if let Some(param) = catch_param {
                let exc =
                    self.compile_expression(&Expression::Identifier(exception_temp.clone()))?;
                self.store_identifier(param, exc)?;
            }
            for stmt in catch_stmts {
                if let Some(r) = self.compile_statement(stmt)? {
                    last_reg = Some(r);
                }
            }
            // Pop the catch context and collect return jumps
            if has_finally {
                if let Some(catch_ctx) = self.try_stack.pop() {
                    all_return_jumps.extend(catch_ctx.return_jumps);
                }
            }
        }

        let finally_start = self.instructions.len();
        if let Some(finally_stmts) = finally_block {
            for stmt in finally_stmts {
                self.compile_statement(stmt)?;
            }
            // After finally block: check if a deferred return is pending
            if let (Some(ref rf), Some(ref rt)) = (&return_flag_temp, &return_temp) {
                let flag_r = self.compile_expression(&Expression::Identifier(rf.clone()))?;
                let skip_return = self.emit(ROp::JumpIfNot, &[flag_r as u16, 9999]);
                let val_r = self.compile_expression(&Expression::Identifier(rt.clone()))?;
                self.emit(ROp::Return, &[val_r as u16]);
                let end = self.instructions.len();
                self.patch_jump(skip_return, end);
            }
        }

        // Patch return_jumps from try/catch to finally start
        for pos in all_return_jumps {
            self.patch_jump(pos, finally_start);
        }

        let end = self.instructions.len();
        if finally_block.is_some() {
            self.patch_jump(jump_after_try, finally_start);
        } else {
            self.patch_jump(jump_after_try, end);
        }
        Ok(last_reg)
    }

    // ── Delete ───────────────────────────────────────────────────────────

    fn compile_delete_into(&mut self, value: &Expression, dst: u16) -> Result<(), String> {
        match value {
            Expression::Index { left, index } => {
                let obj = self.compile_expression(left)?;
                let key = self.compile_expression(index)?;
                self.emit(ROp::DeleteProp, &[dst as u16, obj as u16, key as u16]);

                // Store mutated object back if it's an identifier
                if let Expression::Identifier(name) = &**left {
                    if let Some(&r) = self.locals.get(name.as_str()) {
                        self.emit(ROp::Move, &[r as u16, obj as u16]);
                    } else if let Some(&g) = self.globals.get(name.as_str()) {
                        self.emit(ROp::SetGlobal, &[g, obj as u16]);
                    }
                }
                // Result is true
                self.emit(ROp::LoadTrue, &[dst as u16]);
            }
            Expression::Identifier(_) => {
                self.emit(ROp::LoadFalse, &[dst as u16]);
            }
            _ => {
                let _ = self.compile_expression(value)?;
                self.emit(ROp::LoadTrue, &[dst as u16]);
            }
        }
        Ok(())
    }

    // ── Function / Class compilation ─────────────────────────────────────

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
        // Scan the function body for names referenced by nested function
        // literals. Only those names need global mirrors (for closure capture).
        // All other locals remain purely in registers, which is correct for
        // recursion: each activation gets its own register frame.
        let captured = scan_captured_names(body);
        let mut fn_compiler = RCompiler::new_function_scope(
            self.globals.clone(),
            self.next_global,
            &effective_params,
            captured,
        );

        // Mirror captured parameters to global slots so inner functions can
        // read them via GetGlobal.  Without this, parameters stay in registers
        // only and nested closures see uninitialised globals.
        {
            let params_to_mirror: Vec<(u16, String)> = effective_params
                .iter()
                .enumerate()
                .filter(|(_, p)| fn_compiler.needs_global(p))
                .map(|(i, p)| (i as u16, p.clone()))
                .collect();
            for (reg, name) in params_to_mirror {
                let is_new = !fn_compiler.globals.contains_key(&name);
                let g = fn_compiler.ensure_global_slot(&name)?;
                fn_compiler.emit(ROp::SetGlobal, &[g, reg]);
                // If this is a freshly allocated slot (parameter shadowed an
                // outer variable), mark it so inner closures use MakeClosure.
                if is_new {
                    fn_compiler.param_shadow_slots.insert(g);
                }
            }
        }

        fn_compiler.hoist_var_declarations(body)?;

        // Hoist function declarations to top of function body (JS semantics)
        for stmt in body.iter() {
            if matches!(stmt, Statement::FunctionDecl { .. }) {
                fn_compiler.compile_statement(stmt)?;
                fn_compiler.next_temp = fn_compiler.num_locals;
            }
        }

        let mut last_reg = None;
        for stmt in body {
            if matches!(stmt, Statement::FunctionDecl { .. }) {
                continue; // already compiled above
            }
            last_reg = fn_compiler.compile_statement(stmt)?;
            fn_compiler.next_temp = fn_compiler.num_locals;
        }

        // If the last statement was an expression, emit return with its value
        if let Some(r) = last_reg {
            fn_compiler.emit(ROp::Return, &[r as u16]);
        }

        // Ensure function ends with return
        let last_byte = fn_compiler.instructions.last().copied();
        if last_byte != Some(ROp::Return as u8) && last_byte != Some(ROp::ReturnUndef as u8) {
            fn_compiler.emit(ROp::ReturnUndef, &[]);
        }
        fn_compiler.emit(ROp::Halt, &[]);

        if fn_compiler.next_global > self.next_global {
            self.next_global = fn_compiler.next_global;
        }

        // Determine which global slots from the parent's param_shadow_slots are
        // referenced by this function (or its nested closures). These need to be
        // captured at closure creation time via MakeClosure.
        let closure_captures: Vec<u16> = if !self.param_shadow_slots.is_empty() {
            fn_compiler
                .globals
                .values()
                .copied()
                .filter(|slot| self.param_shadow_slots.contains(slot))
                .collect()
        } else {
            vec![]
        };

        for (name, idx) in &fn_compiler.globals {
            self.globals.entry(name.clone()).or_insert(*idx);
        }

        Ok(CompiledFunctionObject {
            instructions: Rc::new(fn_compiler.instructions),
            constants: Rc::new(fn_compiler.constants),
            num_locals: fn_compiler.num_locals as usize,
            num_parameters: rest_parameter_index.unwrap_or(normalized_params.len()),
            rest_parameter_index,
            takes_this,
            is_async,
            is_generator,
            num_cache_slots: fn_compiler.next_cache_slot,
            max_stack_depth: 0,
            register_count: fn_compiler.max_reg + 1,
            inline_cache: Rc::new(VmCell::new(vec![
                (0, 0);
                fn_compiler.next_cache_slot as usize
            ])),
            closure_captures,
            captured_values: vec![],
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
            super_constructor_chain: vec![],
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
                // Build the constructor chain for multi-level super() calls.
                // Chain[0] = grandparent (parent's super), chain[1] = great-grandparent, etc.
                if !parent.super_methods.is_empty() {
                    class_obj.super_constructor_chain.push((
                        parent.super_methods.clone(),
                        parent.super_getters.clone(),
                        parent.super_setters.clone(),
                    ));
                    class_obj
                        .super_constructor_chain
                        .extend(parent.super_constructor_chain.clone());
                }
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
                    let compiled = self.compile_function_literal(&[], body, true, false, false)?;
                    class_obj
                        .static_initializers
                        .push(StaticInitializer::Block { thunk: compiled });
                }
            }
        }

        Ok(class_obj)
    }

    // ── Builtins (reused from stack compiler) ────────────────────────────

    fn builtin_global_object(name: &str) -> Option<Object> {
        // Delegate to the stack compiler's builtin_global_object
        crate::compiler::Compiler::builtin_global_object_static(name)
    }

    // ── Utilities ────────────────────────────────────────────────────────

    fn emit(&mut self, op: ROp, operands: &[u16]) -> usize {
        let pos = self.instructions.len();
        self.instructions.extend(rmake(op, operands));
        pos
    }

    /// Patch a jump instruction's target. The target u16 is at specific
    /// position depending on the opcode.
    fn patch_jump(&mut self, op_pos: usize, target: usize) {
        let op_byte = self.instructions[op_pos];
        let op = ROp::from_byte(op_byte).expect("valid opcode");
        // Find the position of the jump target operand
        let target_offset = match op {
            ROp::Jump => 1,         // [target:2] — offset 1
            ROp::JumpIfNot => 3,    // [cond:2, target:2] — offset 3
            ROp::JumpIfTruthy => 3, // [cond:2, target:2] — offset 3
            // Fused opcodes with jump targets:
            ROp::TestLtConstJump | ROp::TestLeConstJump | ROp::IncrementRegAndJump => 5,
            // [r:2, const:2, target:2] — target at offset 5
            ROp::ModRegConstStrictEqConstJump => 7,
            // [r:2, mod_const:2, cmp_const:2, target:2] — target at offset 7
            _ => panic!("patch_jump on non-jump opcode {:?}", op),
        };
        let pos = op_pos + target_offset;
        let target = target as u16;
        self.instructions[pos] = ((target >> 8) & 0xff) as u8;
        self.instructions[pos + 1] = (target & 0xff) as u8;
    }

    fn make_temp_name(&mut self, prefix: &str) -> String {
        let name = format!("__fl_{}_{}", prefix, self.temp_counter);
        self.temp_counter += 1;
        name
    }

    fn store_identifier(&mut self, name: &str, src: u16) -> Result<(), String> {
        if self.is_function_scope {
            let r = self.ensure_local(name);
            if r != src {
                self.emit(ROp::Move, &[r as u16, src as u16]);
            }
            self.mirror_local_to_global(name, r);
        } else {
            let g = self.ensure_global_slot(name)?;
            self.emit(ROp::SetGlobal, &[g, src as u16]);
        }
        Ok(())
    }

    /// Prepare block scoping: remove let/const names from locals so they get
    /// fresh registers inside the block.  Returns the saved (name, old_reg)
    /// pairs that must be restored after the block.
    fn enter_block_scope(&mut self, stmts: &[Statement]) -> Vec<(String, Option<u16>)> {
        let mut shadowed = Vec::new();
        for s in stmts {
            match s {
                Statement::Let { name, kind, .. } if *kind != VariableKind::Var => {
                    let old = self.locals.remove(name);
                    shadowed.push((name.clone(), old));
                }
                _ => {}
            }
        }
        shadowed
    }

    /// Restore bindings saved by `enter_block_scope`.
    fn exit_block_scope(&mut self, shadowed: Vec<(String, Option<u16>)>) {
        for (name, old_reg) in shadowed {
            if let Some(r) = old_reg {
                self.locals.insert(name, r);
            } else {
                self.locals.remove(&name);
            }
        }
    }

    fn ensure_binding_register(&mut self, name: &str) -> Result<u16, String> {
        if self.is_function_scope {
            Ok(self.ensure_local(name))
        } else {
            // For globals, use a temp register
            Ok(self.alloc_temp())
        }
    }

    fn write_binding(&mut self, name: &str, src: u16) -> Result<(), String> {
        if !self.is_function_scope {
            let g = self.ensure_global_slot(name)?;
            self.emit(ROp::SetGlobal, &[g, src as u16]);
        }
        Ok(())
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

    // ── Fused opcode pattern detection ──────────────────────────────────

    /// Try to extract a numeric constant index from an expression.
    fn try_numeric_const(&mut self, expr: &Expression) -> Option<u16> {
        match expr {
            Expression::Integer(v) => Some(self.add_constant_int(*v)),
            Expression::Float(v) => Some(self.add_constant_float(*v)),
            _ => None,
        }
    }

    /// Try to detect `local < CONST` or `local <= CONST` pattern.
    /// Returns (register, const_index, is_le).
    fn try_fused_cmp_const(&mut self, expr: &Expression) -> Option<(u16, u16, bool)> {
        if let Expression::Infix {
            left,
            operator,
            right,
        } = expr
        {
            let is_lt = operator == "<";
            let is_le = operator == "<=";
            if !is_lt && !is_le {
                return None;
            }
            if let Expression::Identifier(name) = left.as_ref() {
                if let Some(&r) = self.locals.get(name) {
                    if let Some(const_idx) = self.try_numeric_const(right) {
                        return Some((r, const_idx, is_le));
                    }
                }
            }
        }
        None
    }

    /// Try to detect `(local % CONST_A) === CONST_B` pattern.
    /// Returns (register, mod_const_idx, cmp_const_idx).
    fn try_fused_mod_strict_eq(&mut self, expr: &Expression) -> Option<(u16, u16, u16)> {
        if let Expression::Infix {
            left,
            operator,
            right,
        } = expr
        {
            if operator != "===" {
                return None;
            }
            if let Expression::Infix {
                left: mod_left,
                operator: mod_op,
                right: mod_right,
            } = left.as_ref()
            {
                if mod_op != "%" {
                    return None;
                }
                if let Expression::Identifier(name) = mod_left.as_ref() {
                    if let Some(&r) = self.locals.get(name) {
                        if let Some(mod_const) = self.try_numeric_const(mod_right) {
                            if let Some(cmp_const) = self.try_numeric_const(right) {
                                return Some((r, mod_const, cmp_const));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Try to detect `local = local + CONST` or `local += CONST` pattern.
    /// Returns (register, const_index).
    fn try_fused_increment<'a>(&mut self, expr: &'a Expression) -> Option<(u16, u16, &'a str)> {
        if let Expression::Assign {
            left,
            operator,
            right,
        } = expr
        {
            if let Expression::Identifier(name) = left.as_ref() {
                if let Some(&r) = self.locals.get(name.as_str()) {
                    if operator == "+=" {
                        if let Some(const_idx) = self.try_numeric_const(right) {
                            return Some((r, const_idx, name.as_str()));
                        }
                    } else if operator == "=" {
                        if let Expression::Infix {
                            left: inner_left,
                            operator: inner_op,
                            right: inner_right,
                        } = right.as_ref()
                        {
                            if inner_op == "+" {
                                if let Expression::Identifier(inner_name) = inner_left.as_ref() {
                                    if inner_name == name {
                                        if let Some(const_idx) = self.try_numeric_const(inner_right)
                                        {
                                            return Some((r, const_idx, name.as_str()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Compile a loop condition, trying fused opcodes first.
    /// Returns the position of the jump instruction to patch.
    fn compile_loop_condition(&mut self, cond: &Expression) -> Result<usize, String> {
        // Try fused: local < CONST or local <= CONST
        if let Some((reg, const_idx, is_le)) = self.try_fused_cmp_const(cond) {
            let op = if is_le {
                ROp::TestLeConstJump
            } else {
                ROp::TestLtConstJump
            };
            return Ok(self.emit(op, &[reg as u16, const_idx, 9999]));
        }
        // Try fused: (local % CONST) === CONST
        if let Some((reg, mod_const, cmp_const)) = self.try_fused_mod_strict_eq(cond) {
            return Ok(self.emit(
                ROp::ModRegConstStrictEqConstJump,
                &[reg as u16, mod_const, cmp_const, 9999],
            ));
        }
        // Fallback: compile condition normally + JumpIfNot
        let cond_r = self.compile_expression(cond)?;
        Ok(self.emit(ROp::JumpIfNot, &[cond_r as u16, 9999]))
    }

    /// If the local variable also has a global slot, emit SetGlobal to keep
    /// the global in sync. This is required for closure capture correctness:
    /// inner function scopes access outer variables via GetGlobal.
    fn mirror_local_to_global(&mut self, name: &str, reg: u16) {
        if let Some(&g) = self.globals.get(name) {
            self.emit(ROp::SetGlobal, &[g, reg as u16]);
        }
    }

    /// After a function call returns, reload local registers from their global
    /// counterparts to handle the case where the callee modified a global.
    fn reload_locals_from_globals(&mut self) {
        let pairs: Vec<(u16, u16)> = self
            .locals
            .iter()
            .filter_map(|(name, &r)| self.globals.get(name).map(|&g| (r, g)))
            .collect();
        for (r, g) in pairs {
            self.emit(ROp::GetGlobal, &[r as u16, g]);
        }
    }

    fn try_resolve_global_function(&self, expr: &Expression) -> Option<u16> {
        if let Expression::Identifier(name) = expr {
            if self.locals.contains_key(name) {
                return None;
            }
            if let Some(&idx) = self.globals.get(name) {
                return Some(idx);
            }
        }
        None
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
}

enum BindingSlot {
    Local(u16),
    Global(u16),
}

// ── Captured-name scanner ─────────────────────────────────────────────────
// Walks top-level statements to find identifiers referenced inside nested
// function bodies. Only these names need global slots at top level.

fn scan_captured_names(stmts: &[Statement]) -> FxHashSet<String> {
    let mut captured = FxHashSet::default();
    for stmt in stmts {
        scan_stmt_captures(stmt, &mut captured, false);
    }
    captured
}

/// Check if a function body uses `this` (directly, not inside nested non-arrow functions).
/// Uses the existing scan_expr_captures infrastructure with a sentinel set.
fn scan_body_uses_this(stmts: &[Statement]) -> bool {
    let mut out = FxHashSet::default();
    for stmt in stmts {
        scan_stmt_captures(stmt, &mut out, true);
        // Also check for Expression::This directly in statements
        scan_stmt_for_this(stmt, &mut out);
    }
    out.contains("this")
}

fn scan_stmt_for_this(stmt: &Statement, out: &mut FxHashSet<String>) {
    match stmt {
        Statement::Expression(expr) => scan_expr_for_this(expr, out),
        Statement::Let { value, .. } | Statement::LetPattern { value, .. } => {
            scan_expr_for_this(value, out)
        }
        Statement::Return { value } => scan_expr_for_this(value, out),
        Statement::Block(stmts) | Statement::MultiLet(stmts) => {
            for s in stmts {
                scan_stmt_for_this(s, out);
            }
        }
        _ => {
            // For other statement types, walk children
            // We rely on the fact that Expression::This in expression statements
            // is the most common case
        }
    }
}

fn scan_expr_for_this(expr: &Expression, out: &mut FxHashSet<String>) {
    match expr {
        Expression::This => {
            out.insert("this".to_string());
        }
        Expression::Function { is_arrow, body, .. } => {
            // Arrow functions inherit `this`, so keep scanning.
            // Regular functions have their own `this`, stop.
            if *is_arrow {
                for s in body {
                    scan_stmt_for_this(s, out);
                }
            }
        }
        Expression::Prefix { right, .. } => scan_expr_for_this(right, out),
        Expression::Infix { left, right, .. } | Expression::Assign { left, right, .. } => {
            scan_expr_for_this(left, out);
            scan_expr_for_this(right, out);
        }
        Expression::Index { left, index, .. } => {
            scan_expr_for_this(left, out);
            scan_expr_for_this(index, out);
        }
        Expression::Call { function, arguments, .. }
        | Expression::OptionalCall { function, arguments, .. } => {
            scan_expr_for_this(function, out);
            for arg in arguments {
                scan_expr_for_this(arg, out);
            }
        }
        Expression::Array(items) => {
            for item in items {
                scan_expr_for_this(item, out);
            }
        }
        Expression::If { condition, consequence, alternative } => {
            scan_expr_for_this(condition, out);
            for s in consequence {
                scan_stmt_for_this(s, out);
            }
            if let Some(alt) = alternative {
                for s in alt {
                    scan_stmt_for_this(s, out);
                }
            }
        }
        Expression::Spread { value } => scan_expr_for_this(value, out),
        Expression::Update { target, .. } => scan_expr_for_this(target, out),
        _ => {}
    }
}

fn scan_stmt_captures(stmt: &Statement, out: &mut FxHashSet<String>, in_func: bool) {
    match stmt {
        Statement::Let { value, .. } => {
            scan_expr_captures(value, out, in_func);
        }
        Statement::LetPattern { value, .. } => {
            scan_expr_captures(value, out, in_func);
        }
        Statement::Return { value } => {
            scan_expr_captures(value, out, in_func);
        }
        Statement::ReturnVoid => {}
        Statement::Expression(expr) => {
            scan_expr_captures(expr, out, in_func);
        }
        Statement::Block(stmts) | Statement::MultiLet(stmts) => {
            for s in stmts {
                scan_stmt_captures(s, out, in_func);
            }
        }
        Statement::While { condition, body } => {
            scan_expr_captures(condition, out, in_func);
            for s in body {
                scan_stmt_captures(s, out, in_func);
            }
        }
        Statement::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                scan_stmt_captures(init, out, in_func);
            }
            if let Some(cond) = condition {
                scan_expr_captures(cond, out, in_func);
            }
            if let Some(upd) = update {
                scan_expr_captures(upd, out, in_func);
            }
            for s in body {
                scan_stmt_captures(s, out, in_func);
            }
        }
        Statement::ForOf { iterable, body, .. } => {
            scan_expr_captures(iterable, out, in_func);
            for s in body {
                scan_stmt_captures(s, out, in_func);
            }
        }
        Statement::ForIn { iterable, body, .. } => {
            scan_expr_captures(iterable, out, in_func);
            for s in body {
                scan_stmt_captures(s, out, in_func);
            }
        }
        Statement::FunctionDecl { body, .. } => {
            // Everything inside the function body is "inside a function"
            for s in body {
                scan_stmt_captures(s, out, true);
            }
        }
        Statement::ClassDecl { members, .. } => {
            for member in members {
                match member {
                    ClassMember::Method(method) => {
                        for s in &method.body {
                            scan_stmt_captures(s, out, true);
                        }
                    }
                    ClassMember::Field { initializer, .. } => {
                        if let Some(init) = initializer {
                            scan_expr_captures(init, out, in_func);
                        }
                    }
                    ClassMember::StaticBlock { body } => {
                        for s in body {
                            scan_stmt_captures(s, out, true);
                        }
                    }
                }
            }
        }
        Statement::Throw { value } => {
            scan_expr_captures(value, out, in_func);
        }
        Statement::Try {
            try_block,
            catch_block,
            finally_block,
            ..
        } => {
            for s in try_block {
                scan_stmt_captures(s, out, in_func);
            }
            if let Some(cb) = catch_block {
                for s in cb {
                    scan_stmt_captures(s, out, in_func);
                }
            }
            if let Some(fb) = finally_block {
                for s in fb {
                    scan_stmt_captures(s, out, in_func);
                }
            }
        }
        Statement::Labeled { statement, .. } => {
            scan_stmt_captures(statement, out, in_func);
        }
        Statement::Break { .. } | Statement::Continue { .. } | Statement::Debugger => {}
        Statement::DoWhile { body, condition } => {
            for s in body {
                scan_stmt_captures(s, out, in_func);
            }
            scan_expr_captures(condition, out, in_func);
        }
        Statement::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_captures(discriminant, out, in_func);
            for case in cases {
                if let Some(test) = &case.test {
                    scan_expr_captures(test, out, in_func);
                }
                for s in &case.consequent {
                    scan_stmt_captures(s, out, in_func);
                }
            }
        }
    }
}

fn scan_expr_captures(expr: &Expression, out: &mut FxHashSet<String>, in_func: bool) {
    match expr {
        Expression::Identifier(name) => {
            if in_func {
                out.insert(name.clone());
            }
        }
        Expression::Integer(_)
        | Expression::Float(_)
        | Expression::String(_)
        | Expression::Boolean(_)
        | Expression::Null
        | Expression::This
        | Expression::Super
        | Expression::NewTarget
        | Expression::ImportMeta
        | Expression::RegExp { .. } => {}
        Expression::Array(items) => {
            for item in items {
                scan_expr_captures(item, out, in_func);
            }
        }
        Expression::Hash(pairs) => {
            for entry in pairs {
                match entry {
                    HashEntry::KeyValue { key, value } => {
                        scan_expr_captures(key, out, in_func);
                        scan_expr_captures(value, out, in_func);
                    }
                    HashEntry::Method {
                        key,
                        parameters: _,
                        body,
                        ..
                    } => {
                        scan_expr_captures(key, out, in_func);
                        for s in body {
                            scan_stmt_captures(s, out, true);
                        }
                    }
                    HashEntry::Spread(expr) => {
                        scan_expr_captures(expr, out, in_func);
                    }
                    HashEntry::Getter { body, .. } | HashEntry::Setter { body, .. } => {
                        for s in body {
                            scan_stmt_captures(s, out, true);
                        }
                    }
                }
            }
        }
        Expression::Prefix { right, .. } => {
            scan_expr_captures(right, out, in_func);
        }
        Expression::Typeof { value }
        | Expression::Void { value }
        | Expression::Delete { value }
        | Expression::Await { value }
        | Expression::Spread { value } => {
            scan_expr_captures(value, out, in_func);
        }
        Expression::Yield { value, .. } => {
            scan_expr_captures(value, out, in_func);
        }
        Expression::Sequence(exprs) => {
            for e in exprs {
                scan_expr_captures(e, out, in_func);
            }
        }
        Expression::Infix { left, right, .. } => {
            scan_expr_captures(left, out, in_func);
            scan_expr_captures(right, out, in_func);
        }
        Expression::If {
            condition,
            consequence,
            alternative,
        } => {
            scan_expr_captures(condition, out, in_func);
            for s in consequence {
                scan_stmt_captures(s, out, in_func);
            }
            if let Some(alt) = alternative {
                for s in alt {
                    scan_stmt_captures(s, out, in_func);
                }
            }
        }
        Expression::Function { body, is_arrow, .. } => {
            if *is_arrow {
                // Arrow functions capture `this` from the enclosing scope.
                // Scan body for `this` references and add them as captures.
                if scan_body_uses_this(body) {
                    out.insert("this".to_string());
                }
            }
            // Everything inside a function body is "inside a function"
            for s in body {
                scan_stmt_captures(s, out, true);
            }
        }
        Expression::Call {
            function,
            arguments,
        }
        | Expression::OptionalCall {
            function,
            arguments,
        } => {
            scan_expr_captures(function, out, in_func);
            for arg in arguments {
                scan_expr_captures(arg, out, in_func);
            }
        }
        Expression::New { callee, arguments } => {
            scan_expr_captures(callee, out, in_func);
            for arg in arguments {
                scan_expr_captures(arg, out, in_func);
            }
        }
        Expression::OptionalIndex { left, index } | Expression::Index { left, index } => {
            scan_expr_captures(left, out, in_func);
            scan_expr_captures(index, out, in_func);
        }
        Expression::Assign { left, right, .. } => {
            scan_expr_captures(left, out, in_func);
            scan_expr_captures(right, out, in_func);
        }
        Expression::Update { target, .. } => {
            scan_expr_captures(target, out, in_func);
        }
        Expression::Class {
            extends, members, ..
        } => {
            if let Some(ext) = extends {
                scan_expr_captures(ext, out, in_func);
            }
            for member in members {
                match member {
                    ClassMember::Method(method) => {
                        for s in &method.body {
                            scan_stmt_captures(s, out, true);
                        }
                    }
                    ClassMember::Field { initializer, .. } => {
                        if let Some(init) = initializer {
                            scan_expr_captures(init, out, in_func);
                        }
                    }
                    ClassMember::StaticBlock { body } => {
                        for s in body {
                            scan_stmt_captures(s, out, true);
                        }
                    }
                }
            }
        }
    }
}
