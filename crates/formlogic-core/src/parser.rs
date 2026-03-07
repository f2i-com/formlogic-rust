use crate::ast::{
    ArrayBindingItem, BindingPattern, BindingTarget, ClassMember, ClassMethod, Expression,
    ForBinding, HashEntry, ObjectBindingItem, Program, Statement, SwitchCase, VariableKind,
};
use crate::config::FormLogicConfig;
use crate::lexer::Lexer;
use crate::token::{Token, TokenType};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Precedence {
    Lowest,
    Assign,
    Conditional,
    LogicalOr,
    Nullish,
    LogicalAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseAnd,
    Equals,
    LessGreater,
    Shift,
    Sum,
    Product,
    Prefix,
    Exponent,
    Call,
    Index,
}

#[derive(Clone)]
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    cur_token: Token,
    peek_token: Token,
    pub errors: Vec<String>,
}

struct ParsedParameters {
    names: Vec<String>,
    prologue: Vec<Statement>,
}

impl<'a> Parser<'a> {
    fn clone_for_probe(&self) -> Self {
        Self {
            lexer: self.lexer.clone(),
            cur_token: self.cur_token.clone(),
            peek_token: self.peek_token.clone(),
            errors: Vec::new(),
        }
    }

    fn parse_int_literal(lit: &str) -> Option<i64> {
        if let Some(body) = lit.strip_prefix("0x").or_else(|| lit.strip_prefix("0X")) {
            return i64::from_str_radix(body, 16).ok();
        }
        if let Some(body) = lit.strip_prefix("0b").or_else(|| lit.strip_prefix("0B")) {
            return i64::from_str_radix(body, 2).ok();
        }
        if let Some(body) = lit.strip_prefix("0o").or_else(|| lit.strip_prefix("0O")) {
            return i64::from_str_radix(body, 8).ok();
        }
        lit.parse::<i64>().ok()
    }

    pub fn new(mut lexer: Lexer<'a>) -> Self {
        let cur = lexer.next_token();
        let peek = lexer.next_token();
        Self {
            lexer,
            cur_token: cur,
            peek_token: peek,
            errors: vec![],
        }
    }

    fn next_token(&mut self) {
        let next = self.lexer.next_token();
        self.cur_token = std::mem::replace(&mut self.peek_token, next);
    }

    pub fn parse_program(&mut self) -> Program {
        let mut program = Program::new();
        while self.cur_token.token_type != TokenType::Eof {
            if let Some(stmt) = self.parse_statement() {
                program.statements.push(stmt);
            }
            self.next_token();
        }
        program
    }

    fn parse_statement(&mut self) -> Option<Statement> {
        if self.cur_token.token_type == TokenType::Ident
            && self.peek_token.token_type == TokenType::Colon
        {
            return self.parse_labeled_statement();
        }

        match self.cur_token.token_type {
            TokenType::Let | TokenType::Const | TokenType::Var => self.parse_let_statement(),
            TokenType::Return => self.parse_return_statement(),
            TokenType::While => self.parse_while_statement(),
            TokenType::For => self.parse_for_statement(),
            TokenType::Class => self.parse_class_declaration(),
            TokenType::Try => self.parse_try_statement(),
            TokenType::Throw => self.parse_throw_statement(),
            TokenType::Switch => self.parse_switch_statement(),
            TokenType::Do => self.parse_do_while_statement(),
            TokenType::Debugger => {
                if self.peek_token.token_type == TokenType::Semicolon {
                    self.next_token();
                }
                Some(Statement::Debugger)
            }
            TokenType::Function => {
                if self.peek_token.token_type == TokenType::Asterisk {
                    // function* name() { ... } — generator declaration
                    self.next_token(); // consume '*'
                    self.parse_generator_declaration()
                } else if self.peek_token.token_type == TokenType::Ident {
                    self.parse_function_declaration(false)
                } else {
                    self.parse_expression_statement()
                }
            }
            TokenType::Async => self.parse_async_statement(),
            TokenType::Break => self.parse_break_statement(),
            TokenType::Continue => self.parse_continue_statement(),
            TokenType::LeftBrace => {
                // Disambiguate block vs object literal at statement level.
                // Treat as block if next token is a statement keyword or `}` (empty block).
                let is_block = matches!(
                    self.peek_token.token_type,
                    TokenType::Let
                        | TokenType::Const
                        | TokenType::Var
                        | TokenType::For
                        | TokenType::While
                        | TokenType::Do
                        | TokenType::If
                        | TokenType::Return
                        | TokenType::Function
                        | TokenType::Class
                        | TokenType::Try
                        | TokenType::Throw
                        | TokenType::Switch
                        | TokenType::Break
                        | TokenType::Continue
                        | TokenType::RightBrace
                        | TokenType::LeftBrace
                );
                if is_block {
                    let stmts = self.parse_block_statement();
                    Some(Statement::Block(stmts))
                } else {
                    self.parse_expression_statement()
                }
            }
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_labeled_statement(&mut self) -> Option<Statement> {
        let label = self.cur_token.literal.clone();
        self.next_token(); // ':'
        self.next_token(); // start of labeled statement
        let statement = self.parse_statement()?;
        Some(Statement::Labeled {
            label,
            statement: Box::new(statement),
        })
    }

    fn parse_async_statement(&mut self) -> Option<Statement> {
        if self.peek_token.token_type == TokenType::Function {
            self.next_token(); // now current token is 'function'
            if self.peek_token.token_type == TokenType::Asterisk {
                // async function* name() { ... } — async generator
                self.next_token(); // consume '*'
                return self.parse_async_generator_declaration();
            }
            if self.peek_token.token_type == TokenType::Ident {
                return self.parse_function_declaration(true);
            }
            let expr = self.parse_function_literal(true)?;
            if self.peek_token.token_type == TokenType::Semicolon {
                self.next_token();
            }
            return Some(Statement::Expression(expr));
        }
        self.parse_expression_statement()
    }

    fn parse_break_statement(&mut self) -> Option<Statement> {
        // ASI: a newline after `break` terminates the statement (no label).
        // Only consume an identifier as a label if it's on the same line.
        let label = if self.peek_token.token_type == TokenType::Ident
            && !self.peek_token.had_newline_before
        {
            self.next_token();
            Some(self.cur_token.literal.clone())
        } else {
            None
        };
        if self.peek_token.token_type == TokenType::Semicolon {
            self.next_token();
        }
        Some(Statement::Break { label })
    }

    fn parse_continue_statement(&mut self) -> Option<Statement> {
        // ASI: a newline after `continue` terminates the statement (no label).
        let label = if self.peek_token.token_type == TokenType::Ident
            && !self.peek_token.had_newline_before
        {
            self.next_token();
            Some(self.cur_token.literal.clone())
        } else {
            None
        };
        if self.peek_token.token_type == TokenType::Semicolon {
            self.next_token();
        }
        Some(Statement::Continue { label })
    }

    fn parse_throw_statement(&mut self) -> Option<Statement> {
        self.next_token();
        let value = self.parse_expression(Precedence::Lowest)?;
        if self.peek_token.token_type == TokenType::Semicolon {
            self.next_token();
        }
        Some(Statement::Throw { value })
    }

    fn parse_try_statement(&mut self) -> Option<Statement> {
        if !self.expect_peek(TokenType::LeftBrace) {
            return None;
        }
        let try_block = self.parse_block_statement();

        let mut catch_param: Option<String> = None;
        let mut catch_block: Option<Vec<Statement>> = None;
        let mut finally_block: Option<Vec<Statement>> = None;

        if self.peek_token.token_type == TokenType::Catch {
            self.next_token(); // catch
            // Optional catch binding: `catch { ... }` (no parameter)
            if self.peek_token.token_type == TokenType::LeftBrace {
                // No parameter — catch_param stays None
                self.next_token(); // consume `{`
                catch_block = Some(self.parse_block_statement());
            } else {
                if !self.expect_peek(TokenType::LeftParen) {
                    return None;
                }
                if !self.expect_peek(TokenType::Ident) {
                    return None;
                }
                catch_param = Some(self.cur_token.literal.clone());
                if !self.expect_peek(TokenType::RightParen) {
                    return None;
                }
                if !self.expect_peek(TokenType::LeftBrace) {
                    return None;
                }
                catch_block = Some(self.parse_block_statement());
            }
        }

        if self.peek_token.token_type == TokenType::Finally {
            self.next_token(); // finally
            if !self.expect_peek(TokenType::LeftBrace) {
                return None;
            }
            finally_block = Some(self.parse_block_statement());
        }

        if catch_block.is_none() && finally_block.is_none() {
            self.errors
                .push("try statement requires catch or finally".to_string());
            return None;
        }

        Some(Statement::Try {
            try_block,
            catch_param,
            catch_block,
            finally_block,
        })
    }

    fn parse_while_statement(&mut self) -> Option<Statement> {
        if !self.expect_peek(TokenType::LeftParen) {
            return None;
        }
        self.next_token();
        let condition = self.parse_expression(Precedence::Lowest)?;
        if !self.expect_peek(TokenType::RightParen) {
            return None;
        }
        let body = if self.peek_token.token_type == TokenType::LeftBrace {
            self.next_token();
            self.parse_block_statement()
        } else {
            self.next_token();
            match self.parse_statement() {
                Some(s) => vec![s],
                None => vec![],
            }
        };
        Some(Statement::While { condition, body })
    }

    fn parse_do_while_statement(&mut self) -> Option<Statement> {
        // do { ... } while (condition);  OR  do stmt; while (condition);
        let body = if self.peek_token.token_type == TokenType::LeftBrace {
            self.next_token();
            self.parse_block_statement()
        } else {
            self.next_token();
            match self.parse_statement() {
                Some(s) => vec![s],
                None => vec![],
            }
        };
        if !self.expect_peek(TokenType::While) {
            return None;
        }
        if !self.expect_peek(TokenType::LeftParen) {
            return None;
        }
        self.next_token();
        let condition = self.parse_expression(Precedence::Lowest)?;
        if !self.expect_peek(TokenType::RightParen) {
            return None;
        }
        if self.peek_token.token_type == TokenType::Semicolon {
            self.next_token();
        }
        Some(Statement::DoWhile { body, condition })
    }

    fn parse_switch_statement(&mut self) -> Option<Statement> {
        // switch (expr) { case expr: ... default: ... }
        if !self.expect_peek(TokenType::LeftParen) {
            return None;
        }
        self.next_token();
        let discriminant = self.parse_expression(Precedence::Lowest)?;
        if !self.expect_peek(TokenType::RightParen) {
            return None;
        }
        if !self.expect_peek(TokenType::LeftBrace) {
            return None;
        }

        let mut cases = Vec::new();
        self.next_token(); // move past '{'

        while self.cur_token.token_type != TokenType::RightBrace
            && self.cur_token.token_type != TokenType::Eof
        {
            let test = if self.cur_token.token_type == TokenType::Case {
                self.next_token();
                let expr = self.parse_expression(Precedence::Lowest)?;
                if !self.expect_peek(TokenType::Colon) {
                    return None;
                }
                Some(expr)
            } else if self.cur_token.token_type == TokenType::Default {
                if !self.expect_peek(TokenType::Colon) {
                    return None;
                }
                None
            } else {
                self.errors.push(format!(
                    "expected 'case' or 'default', got {:?}",
                    self.cur_token.token_type
                ));
                return None;
            };

            let mut consequent = Vec::new();
            self.next_token();
            while self.cur_token.token_type != TokenType::Case
                && self.cur_token.token_type != TokenType::Default
                && self.cur_token.token_type != TokenType::RightBrace
                && self.cur_token.token_type != TokenType::Eof
            {
                if let Some(stmt) = self.parse_statement() {
                    consequent.push(stmt);
                }
                self.next_token();
            }

            cases.push(SwitchCase { test, consequent });
        }

        Some(Statement::Switch {
            discriminant,
            cases,
        })
    }

    fn parse_for_statement(&mut self) -> Option<Statement> {
        if !self.expect_peek(TokenType::LeftParen) {
            return None;
        }

        self.next_token();

        // for (let x of iterable) { ... }
        if self.cur_token.token_type == TokenType::Let
            || self.cur_token.token_type == TokenType::Const
            || self.cur_token.token_type == TokenType::Var
        {
            let kind = match self.cur_token.token_type {
                TokenType::Const => VariableKind::Const,
                TokenType::Var => VariableKind::Var,
                _ => VariableKind::Let,
            };

            let binding = match self.peek_token.token_type {
                TokenType::Ident => {
                    self.next_token();
                    ForBinding::Identifier(self.cur_token.literal.clone())
                }
                TokenType::LeftBracket | TokenType::LeftBrace => {
                    self.next_token();
                    ForBinding::Pattern(self.parse_binding_pattern()?)
                }
                _ => {
                    self.errors.push(format!(
                        "expected loop binding after for(let/const/var ...), got {:?}",
                        self.peek_token.token_type
                    ));
                    return None;
                }
            };

            if self.peek_token.token_type == TokenType::Of {
                self.next_token(); // 'of'
                self.next_token(); // start iterable expression
                let iterable = self.parse_expression(Precedence::Lowest)?;
                if !self.expect_peek(TokenType::RightParen) {
                    return None;
                }
                let body = if self.peek_token.token_type == TokenType::LeftBrace {
                    self.next_token();
                    self.parse_block_statement()
                } else {
                    self.next_token();
                    match self.parse_statement() {
                        Some(s) => vec![s],
                        None => vec![],
                    }
                };
                return Some(Statement::ForOf {
                    binding,
                    iterable,
                    body,
                });
            }

            if self.peek_token.token_type == TokenType::In {
                let var_name = match binding {
                    ForBinding::Identifier(name) => name,
                    _ => {
                        self.errors
                            .push("for-in currently supports identifier binding only".to_string());
                        return None;
                    }
                };
                self.next_token(); // 'in'
                self.next_token(); // start iterable expression
                let iterable = self.parse_expression(Precedence::Lowest)?;
                if !self.expect_peek(TokenType::RightParen) {
                    return None;
                }
                let body = if self.peek_token.token_type == TokenType::LeftBrace {
                    self.next_token();
                    self.parse_block_statement()
                } else {
                    self.next_token();
                    match self.parse_statement() {
                        Some(s) => vec![s],
                        None => vec![],
                    }
                };
                return Some(Statement::ForIn {
                    var_name,
                    iterable,
                    body,
                });
            }

            // classic for initializer with let/const/var
            let var_name = match binding {
                ForBinding::Identifier(name) => name,
                _ => {
                    self.errors.push(
                        "classic for initializer currently supports identifier binding only"
                            .to_string(),
                    );
                    return None;
                }
            };
            let first_value = if self.peek_token.token_type == TokenType::Assign {
                self.next_token(); // consume =
                self.next_token(); // start of value expr
                self.parse_expression(Precedence::Lowest)?
            } else {
                Expression::Identifier("undefined".to_string())
            };
            let mut decls: Vec<Statement> = vec![Statement::Let {
                name: var_name,
                value: first_value,
                kind: kind.clone(),
            }];

            // Handle comma-separated declarations: `let i = 0, j = 10`
            while self.peek_token.token_type == TokenType::Comma {
                self.next_token(); // consume comma
                if self.peek_token.token_type != TokenType::Ident {
                    self.errors.push(format!(
                        "expected identifier after ',' in for-loop initializer, got {:?}",
                        self.peek_token.token_type
                    ));
                    return None;
                }
                self.next_token(); // consume ident
                let next_name = self.cur_token.literal.clone();
                let next_value = if self.peek_token.token_type == TokenType::Assign {
                    self.next_token(); // consume =
                    self.next_token(); // start of value expr
                    self.parse_expression(Precedence::Lowest)?
                } else {
                    Expression::Identifier("undefined".to_string())
                };
                decls.push(Statement::Let {
                    name: next_name,
                    value: next_value,
                    kind: kind.clone(),
                });
            }

            let init = if decls.len() == 1 {
                Some(Box::new(decls.remove(0)))
            } else {
                Some(Box::new(Statement::MultiLet(decls)))
            };

            if self.peek_token.token_type == TokenType::Semicolon {
                self.next_token();
            } else {
                self.errors
                    .push("expected ';' after for-loop initializer".to_string());
                return None;
            }

            self.next_token();
            let condition = if self.cur_token.token_type == TokenType::Semicolon {
                None
            } else {
                let cond = self.parse_expression(Precedence::Lowest)?;
                if self.peek_token.token_type == TokenType::Semicolon {
                    self.next_token();
                } else {
                    self.errors
                        .push("expected ';' after for-loop condition".to_string());
                    return None;
                }
                Some(cond)
            };

            self.next_token();
            let update = if self.cur_token.token_type == TokenType::RightParen {
                None
            } else {
                let upd = self.parse_sequence_expression()?;
                if !self.expect_peek(TokenType::RightParen) {
                    return None;
                }
                Some(upd)
            };

            let body = if self.peek_token.token_type == TokenType::LeftBrace {
                self.next_token();
                self.parse_block_statement()
            } else {
                self.next_token();
                match self.parse_statement() {
                    Some(s) => vec![s],
                    None => vec![],
                }
            };

            return Some(Statement::For {
                init,
                condition,
                update,
                body,
            });
        }

        let init = if self.cur_token.token_type == TokenType::Semicolon {
            None
        } else {
            let expr = self.parse_expression(Precedence::Lowest)?;
            if self.peek_token.token_type == TokenType::Semicolon {
                self.next_token();
            } else {
                self.errors
                    .push("expected ';' after for-loop initializer".to_string());
                return None;
            }
            Some(Box::new(Statement::Expression(expr)))
        };

        self.next_token();

        let condition = if self.cur_token.token_type == TokenType::Semicolon {
            None
        } else {
            let cond = self.parse_expression(Precedence::Lowest)?;
            if self.peek_token.token_type == TokenType::Semicolon {
                self.next_token();
            } else {
                self.errors
                    .push("expected ';' after for-loop condition".to_string());
                return None;
            }
            Some(cond)
        };

        self.next_token();

        let update = if self.cur_token.token_type == TokenType::RightParen {
            None
        } else {
            let upd = self.parse_sequence_expression()?;
            if !self.expect_peek(TokenType::RightParen) {
                return None;
            }
            Some(upd)
        };

        let body = if self.peek_token.token_type == TokenType::LeftBrace {
            self.next_token();
            self.parse_block_statement()
        } else {
            self.next_token();
            match self.parse_statement() {
                Some(s) => vec![s],
                None => vec![],
            }
        };

        Some(Statement::For {
            init,
            condition,
            update,
            body,
        })
    }

    fn parse_function_declaration(&mut self, is_async: bool) -> Option<Statement> {
        if !self.expect_peek(TokenType::Ident) {
            return None;
        }
        let name = self.cur_token.literal.clone();

        if !self.expect_peek(TokenType::LeftParen) {
            return None;
        }
        let parsed_params = self.parse_function_parameters()?;

        if !self.expect_peek(TokenType::LeftBrace) {
            return None;
        }
        let raw_body = self.parse_block_statement();
        let body = Self::prepend_param_prologue(raw_body, &parsed_params.prologue);

        Some(Statement::FunctionDecl {
            name,
            parameters: parsed_params.names,
            body,
            is_async,
            is_generator: false,
        })
    }

    fn parse_generator_declaration(&mut self) -> Option<Statement> {
        if !self.expect_peek(TokenType::Ident) {
            return None;
        }
        let name = self.cur_token.literal.clone();

        if !self.expect_peek(TokenType::LeftParen) {
            return None;
        }
        let parsed_params = self.parse_function_parameters()?;

        if !self.expect_peek(TokenType::LeftBrace) {
            return None;
        }
        let raw_body = self.parse_block_statement();
        let body = Self::prepend_param_prologue(raw_body, &parsed_params.prologue);

        Some(Statement::FunctionDecl {
            name,
            parameters: parsed_params.names,
            body,
            is_async: false,
            is_generator: true,
        })
    }

    fn parse_async_generator_declaration(&mut self) -> Option<Statement> {
        if !self.expect_peek(TokenType::Ident) {
            return None;
        }
        let name = self.cur_token.literal.clone();

        if !self.expect_peek(TokenType::LeftParen) {
            return None;
        }
        let parsed_params = self.parse_function_parameters()?;

        if !self.expect_peek(TokenType::LeftBrace) {
            return None;
        }
        let raw_body = self.parse_block_statement();
        let body = Self::prepend_param_prologue(raw_body, &parsed_params.prologue);

        Some(Statement::FunctionDecl {
            name,
            parameters: parsed_params.names,
            body,
            is_async: true,
            is_generator: true,
        })
    }

    fn parse_class_declaration(&mut self) -> Option<Statement> {
        if !self.expect_peek(TokenType::Ident) {
            return None;
        }
        let name = self.cur_token.literal.clone();

        let extends = self.parse_class_extends()?;

        let members = self.parse_class_body()?;

        Some(Statement::ClassDecl {
            name: Some(name),
            extends,
            members,
        })
    }

    /// Parse `extends <expression>` if present. Returns `Option<Box<Expression>>`.
    fn parse_class_extends(&mut self) -> Option<Option<Box<Expression>>> {
        if self.peek_token.token_type == TokenType::Extends {
            self.next_token(); // consume `extends`
            self.next_token(); // advance to the expression
            let expr = self.parse_expression(Precedence::Lowest)?;
            Some(Some(Box::new(expr)))
        } else {
            Some(None)
        }
    }

    /// Parse `{ ... }` class body. Returns `Vec<ClassMember>`.
    fn parse_class_body(&mut self) -> Option<Vec<ClassMember>> {
        if !self.expect_peek(TokenType::LeftBrace) {
            return None;
        }

        let mut members = vec![];
        self.next_token();
        while self.cur_token.token_type != TokenType::RightBrace
            && self.cur_token.token_type != TokenType::Eof
        {
            // Skip semicolons in class body (empty statements)
            if self.cur_token.token_type == TokenType::Semicolon {
                self.next_token();
                continue;
            }

            let mut is_static = false;

            if self.cur_token.token_type == TokenType::Static {
                // Could be: static method, static field, static getter/setter, or static block
                if self.peek_token.token_type == TokenType::LeftBrace {
                    // static { ... } — static initialization block
                    self.next_token(); // consume `{`
                    let body = self.parse_block_statement();
                    members.push(ClassMember::StaticBlock { body });
                    self.next_token();
                    continue;
                }
                is_static = true;
                self.next_token();
            }

            // Handle private names: #name
            let is_private = self.cur_token.token_type == TokenType::Hash;
            if is_private {
                self.next_token(); // consume `#`, cur_token is now the name
            }

            let mut is_getter = false;
            let mut is_setter = false;

            // Determine if this is get/set accessor
            let member_name = if self.cur_token.token_type == TokenType::Get
                && self.peek_token.token_type != TokenType::LeftParen
                && self.peek_token.token_type != TokenType::Assign
                && self.peek_token.token_type != TokenType::Semicolon
                && self.peek_token.token_type != TokenType::RightBrace
            {
                is_getter = true;
                self.next_token();
                if self.cur_token.token_type == TokenType::Hash {
                    self.next_token(); // skip `#`
                    format!("#{}", self.cur_token.literal)
                } else {
                    self.cur_token.literal.clone()
                }
            } else if self.cur_token.token_type == TokenType::Set
                && self.peek_token.token_type != TokenType::LeftParen
                && self.peek_token.token_type != TokenType::Assign
                && self.peek_token.token_type != TokenType::Semicolon
                && self.peek_token.token_type != TokenType::RightBrace
            {
                is_setter = true;
                self.next_token();
                if self.cur_token.token_type == TokenType::Hash {
                    self.next_token(); // skip `#`
                    format!("#{}", self.cur_token.literal)
                } else {
                    self.cur_token.literal.clone()
                }
            } else if self.cur_token.token_type == TokenType::Ident
                || self.cur_token.token_type == TokenType::Get
                || self.cur_token.token_type == TokenType::Set
            {
                let name = self.cur_token.literal.clone();
                if is_private {
                    format!("#{}", name)
                } else {
                    name
                }
            } else {
                self.errors.push("expected class member name".to_string());
                return None;
            };

            // Decide: is this a method (followed by `(`) or a field?
            if self.peek_token.token_type == TokenType::LeftParen {
                // Method (or constructor)
                self.next_token(); // move to `(`
                let parsed_params = self.parse_function_parameters()?;

                if !self.expect_peek(TokenType::LeftBrace) {
                    return None;
                }
                let raw_body = self.parse_block_statement();
                let body = Self::prepend_param_prologue(raw_body, &parsed_params.prologue);
                members.push(ClassMember::Method(ClassMethod {
                    name: member_name,
                    parameters: parsed_params.names,
                    body,
                    is_static,
                    is_getter,
                    is_setter,
                }));
            } else {
                // Field declaration: `name;` or `name = expr;`
                let initializer = if self.peek_token.token_type == TokenType::Assign {
                    self.next_token(); // consume `=`
                    self.next_token(); // advance to expression
                    Some(self.parse_expression(Precedence::Lowest)?)
                } else {
                    None
                };
                // Consume optional semicolon or auto-semicolon (newline)
                if self.peek_token.token_type == TokenType::Semicolon {
                    self.next_token();
                }
                members.push(ClassMember::Field {
                    name: member_name,
                    initializer,
                    is_static,
                });
            }

            self.next_token();
        }

        Some(members)
    }

    /// Parse a class expression: `class [Name] [extends Expr] { ... }`
    fn parse_class_expression(&mut self) -> Option<Expression> {
        // Optional name
        let name = if self.peek_token.token_type == TokenType::Ident {
            self.next_token();
            Some(self.cur_token.literal.clone())
        } else {
            None
        };

        let extends = self.parse_class_extends()?;
        let members = self.parse_class_body()?;

        Some(Expression::Class {
            name,
            extends,
            members,
        })
    }

    fn parse_let_statement(&mut self) -> Option<Statement> {
        let kind = match self.cur_token.token_type {
            TokenType::Const => VariableKind::Const,
            TokenType::Var => VariableKind::Var,
            _ => VariableKind::Let,
        };

        let pattern_or_name = match self.peek_token.token_type {
            TokenType::Ident => {
                self.next_token();
                Some((None, Some(self.cur_token.literal.clone())))
            }
            TokenType::LeftBracket | TokenType::LeftBrace => {
                self.next_token();
                let pattern = self.parse_binding_pattern()?;
                Some((Some(pattern), None))
            }
            _ => {
                self.errors.push(format!(
                    "expected binding identifier or pattern after let/const, got {:?}",
                    self.peek_token.token_type
                ));
                None
            }
        }?;

        // Support `let x;` and `let x, y, z;` (no initializer = undefined)
        if let Some(ref name) = pattern_or_name.1 {
            // Check for semicolon (let x;) or comma (let x, y;)
            if self.peek_token.token_type == TokenType::Semicolon {
                self.next_token();
                return Some(Statement::Let {
                    name: name.clone(),
                    value: Expression::Identifier("undefined".to_string()),
                    kind,
                });
            }
            if self.peek_token.token_type == TokenType::Comma {
                // Multi-declaration: let a, b, c; or let a, b = 1, c;
                let mut stmts: Vec<Statement> = vec![Statement::Let {
                    name: name.clone(),
                    value: Expression::Identifier("undefined".to_string()),
                    kind: kind.clone(),
                }];
                while self.peek_token.token_type == TokenType::Comma {
                    self.next_token(); // consume comma
                    if self.peek_token.token_type != TokenType::Ident {
                        self.errors.push(format!(
                            "expected identifier after comma in declaration, got {:?}",
                            self.peek_token.token_type
                        ));
                        return None;
                    }
                    self.next_token(); // consume ident
                    let var_name = self.cur_token.literal.clone();
                    if self.peek_token.token_type == TokenType::Assign {
                        self.next_token(); // consume =
                        self.next_token(); // start of value expr
                        let val = self.parse_expression(Precedence::Lowest)?;
                        stmts.push(Statement::Let {
                            name: var_name,
                            value: val,
                            kind: kind.clone(),
                        });
                    } else {
                        stmts.push(Statement::Let {
                            name: var_name,
                            value: Expression::Identifier("undefined".to_string()),
                            kind: kind.clone(),
                        });
                    }
                }
                if self.peek_token.token_type == TokenType::Semicolon {
                    self.next_token();
                }
                return Some(Statement::MultiLet(stmts));
            }
        }

        if !self.expect_peek(TokenType::Assign) {
            return None;
        }
        self.next_token();
        let value = self.parse_expression(Precedence::Lowest)?;

        // Multi-declaration with initializers: let a = 1, b = 2;
        if self.peek_token.token_type == TokenType::Comma && pattern_or_name.1.is_some() {
            let first_stmt = Statement::Let {
                name: pattern_or_name.1.expect("name exists"),
                value,
                kind: kind.clone(),
            };
            let mut stmts = vec![first_stmt];
            while self.peek_token.token_type == TokenType::Comma {
                self.next_token(); // consume comma
                if self.peek_token.token_type != TokenType::Ident {
                    self.errors.push(format!(
                        "expected identifier after comma in declaration, got {:?}",
                        self.peek_token.token_type
                    ));
                    return None;
                }
                self.next_token(); // consume ident
                let var_name = self.cur_token.literal.clone();
                if self.peek_token.token_type == TokenType::Assign {
                    self.next_token(); // consume =
                    self.next_token(); // start of value expr
                    let val = self.parse_expression(Precedence::Lowest)?;
                    stmts.push(Statement::Let {
                        name: var_name,
                        value: val,
                        kind: kind.clone(),
                    });
                } else {
                    stmts.push(Statement::Let {
                        name: var_name,
                        value: Expression::Identifier("undefined".to_string()),
                        kind: kind.clone(),
                    });
                }
            }
            if self.peek_token.token_type == TokenType::Semicolon {
                self.next_token();
            }
            return Some(Statement::MultiLet(stmts));
        }

        if self.peek_token.token_type == TokenType::Semicolon {
            self.next_token();
        }

        if let Some(pattern) = pattern_or_name.0 {
            Some(Statement::LetPattern {
                pattern,
                value,
                kind,
            })
        } else {
            Some(Statement::Let {
                name: pattern_or_name.1.expect("name exists"),
                value,
                kind,
            })
        }
    }

    fn parse_binding_pattern(&mut self) -> Option<BindingPattern> {
        match self.cur_token.token_type {
            TokenType::LeftBracket => self.parse_array_binding_pattern(),
            TokenType::LeftBrace => self.parse_object_binding_pattern(),
            _ => {
                self.errors
                    .push("expected binding pattern ([...] or {...})".to_string());
                None
            }
        }
    }

    fn parse_array_binding_pattern(&mut self) -> Option<BindingPattern> {
        let mut items: Vec<ArrayBindingItem> = vec![];
        if self.peek_token.token_type == TokenType::RightBracket {
            self.next_token();
            return Some(BindingPattern::Array(items));
        }

        let mut expect_element = true;
        loop {
            if self.peek_token.token_type == TokenType::RightBracket {
                self.next_token();
                break;
            }

            self.next_token();
            if self.cur_token.token_type == TokenType::Comma {
                if expect_element {
                    items.push(ArrayBindingItem::Hole);
                }
                expect_element = true;
                continue;
            }

            if self.cur_token.token_type == TokenType::Spread {
                if !self.expect_peek(TokenType::Ident) {
                    return None;
                }
                if self.peek_token.token_type == TokenType::Assign {
                    self.errors
                        .push("rest element in array binding cannot have default".to_string());
                    return None;
                }
                items.push(ArrayBindingItem::Rest {
                    name: self.cur_token.literal.clone(),
                });
                if self.peek_token.token_type == TokenType::Comma {
                    self.errors
                        .push("rest element in array binding must be last".to_string());
                    return None;
                }
                expect_element = false;
                continue;
            }

            if self.cur_token.token_type != TokenType::Ident {
                if self.cur_token.token_type != TokenType::LeftBracket
                    && self.cur_token.token_type != TokenType::LeftBrace
                {
                    self.errors.push(
                        "array binding pattern supports identifiers, nested patterns, defaults, and holes"
                            .to_string(),
                    );
                    return None;
                }
            }

            let target = self.parse_binding_target_from_current()?;
            let default_value = if self.peek_token.token_type == TokenType::Assign {
                self.next_token();
                self.next_token();
                Some(self.parse_expression(Precedence::Lowest)?)
            } else {
                None
            };
            items.push(ArrayBindingItem::Binding {
                target,
                default_value,
            });
            expect_element = false;

            if self.peek_token.token_type == TokenType::Comma {
                self.next_token();
                expect_element = true;
                continue;
            }

            if self.peek_token.token_type != TokenType::RightBracket {
                self.errors
                    .push("expected ',' or ']' in array binding pattern".to_string());
                return None;
            }
        }
        Some(BindingPattern::Array(items))
    }

    fn parse_object_binding_pattern(&mut self) -> Option<BindingPattern> {
        let mut pairs: Vec<ObjectBindingItem> = vec![];
        if self.peek_token.token_type == TokenType::RightBrace {
            self.next_token();
            return Some(BindingPattern::Object(pairs));
        }

        loop {
            if self.peek_token.token_type == TokenType::RightBrace {
                break;
            }

            self.next_token();

            if self.cur_token.token_type == TokenType::Spread {
                if !self.expect_peek(TokenType::Ident) {
                    return None;
                }
                if self.peek_token.token_type == TokenType::Assign {
                    self.errors
                        .push("rest property in object binding cannot have default".to_string());
                    return None;
                }
                pairs.push(ObjectBindingItem {
                    key: Expression::String(String::new()),
                    target: BindingTarget::Identifier(self.cur_token.literal.clone()),
                    default_value: None,
                    is_rest: true,
                });
                if self.peek_token.token_type == TokenType::Comma {
                    self.errors
                        .push("rest property in object binding must be last".to_string());
                    return None;
                }
                continue;
            }

            let (key, mut target, require_alias) = match self.cur_token.token_type {
                TokenType::Ident => {
                    let n = self.cur_token.literal.clone();
                    (
                        Expression::String(n.clone()),
                        BindingTarget::Identifier(n),
                        false,
                    )
                }
                TokenType::String => (
                    Expression::String(self.cur_token.literal.clone()),
                    BindingTarget::Identifier(String::new()),
                    true,
                ),
                TokenType::LeftBracket => {
                    self.next_token();
                    let key_expr = self.parse_expression(Precedence::Lowest)?;
                    if !self.expect_peek(TokenType::RightBracket) {
                        return None;
                    }
                    (key_expr, BindingTarget::Identifier(String::new()), true)
                }
                _ => {
                    self.errors.push(
                        "expected identifier/string/[expr] key in object binding".to_string(),
                    );
                    return None;
                }
            };

            if self.peek_token.token_type == TokenType::Colon {
                self.next_token();
                self.next_token();
                target = self.parse_binding_target_from_current()?;
            } else if require_alias {
                self.errors.push(
                    "object binding key requires alias, e.g. {\"x\": v} or {[k]: v}".to_string(),
                );
                return None;
            }

            let default_value = if self.peek_token.token_type == TokenType::Assign {
                self.next_token();
                self.next_token();
                Some(self.parse_expression(Precedence::Lowest)?)
            } else {
                None
            };

            pairs.push(ObjectBindingItem {
                key,
                target,
                default_value,
                is_rest: false,
            });

            if self.peek_token.token_type != TokenType::Comma {
                break;
            }
            self.next_token();
        }

        if !self.expect_peek(TokenType::RightBrace) {
            return None;
        }
        Some(BindingPattern::Object(pairs))
    }

    fn parse_return_statement(&mut self) -> Option<Statement> {
        let return_line = self.cur_token.line;
        if self.peek_token.token_type == TokenType::Semicolon {
            self.next_token();
            return Some(Statement::ReturnVoid);
        }
        // ASI: if the next token is on a different line, treat as `return;`
        // This handles patterns like `if (cond) return\nlet x = ...`
        if self.peek_token.line != return_line {
            return Some(Statement::ReturnVoid);
        }
        self.next_token();
        if self.cur_token.token_type == TokenType::RightBrace
            || self.cur_token.token_type == TokenType::Eof
        {
            return Some(Statement::ReturnVoid);
        }
        let value = self.parse_expression(Precedence::Lowest)?;
        if self.peek_token.token_type == TokenType::Semicolon {
            self.next_token();
        }
        Some(Statement::Return { value })
    }

    fn parse_binding_target_from_current(&mut self) -> Option<BindingTarget> {
        match self.cur_token.token_type {
            TokenType::Ident => Some(BindingTarget::Identifier(self.cur_token.literal.clone())),
            TokenType::LeftBracket | TokenType::LeftBrace => Some(BindingTarget::Pattern(
                Box::new(self.parse_binding_pattern()?),
            )),
            _ => {
                self.errors.push(format!(
                    "expected binding target (identifier or nested pattern), got {:?}",
                    self.cur_token.token_type
                ));
                None
            }
        }
    }

    fn parse_expression_statement(&mut self) -> Option<Statement> {
        let expr = self.parse_expression(Precedence::Lowest)?;
        if self.peek_token.token_type == TokenType::Semicolon {
            self.next_token();
        }
        Some(Statement::Expression(expr))
    }

    fn parse_expression(&mut self, precedence: Precedence) -> Option<Expression> {
        let mut left = match self.cur_token.token_type {
            TokenType::Ident => {
                if self.peek_token.token_type == TokenType::Arrow {
                    self.parse_arrow_function_single_param(false)
                } else {
                    Some(Expression::Identifier(self.cur_token.literal.clone()))
                }
            }
            TokenType::Int => {
                Self::parse_int_literal(&self.cur_token.literal).map(Expression::Integer)
            }
            TokenType::Float => self
                .cur_token
                .literal
                .parse::<f64>()
                .ok()
                .map(Expression::Float),
            TokenType::String => Some(Expression::String(self.cur_token.literal.clone())),
            TokenType::Regex => Some(Expression::RegExp {
                pattern: self.cur_token.literal.clone(),
                flags: self.cur_token.raw_literal.clone().unwrap_or_default(),
            }),
            TokenType::Template => self.parse_template_literal_expression(),
            TokenType::True => Some(Expression::Boolean(true)),
            TokenType::False => Some(Expression::Boolean(false)),
            TokenType::Null => Some(Expression::Null),
            TokenType::Undefined => Some(Expression::Identifier("undefined".to_string())),
            TokenType::Bang | TokenType::Minus | TokenType::Plus | TokenType::BitwiseNot => {
                self.parse_prefix_expression()
            }
            TokenType::Increment | TokenType::Decrement => self.parse_update_prefix_expression(),
            TokenType::LeftParen => self.parse_grouped_expression(),
            TokenType::If => self.parse_if_expression(),
            TokenType::Function => self.parse_function_literal(false),
            TokenType::This => Some(Expression::This),
            TokenType::Super => Some(Expression::Super),
            TokenType::Await => self.parse_await_expression(),
            TokenType::Yield => self.parse_yield_expression(),
            TokenType::Typeof => self.parse_typeof_expression(),
            TokenType::Void => self.parse_void_expression(),
            TokenType::Delete => self.parse_delete_expression(),
            TokenType::New => self.parse_new_expression(),
            TokenType::Import => self.parse_import_meta_expression(),
            TokenType::Class => self.parse_class_expression(),
            TokenType::Async => {
                if self.peek_token.token_type == TokenType::Function {
                    self.next_token();
                    self.parse_function_literal(true)
                } else if self.peek_token.token_type == TokenType::LeftParen {
                    self.next_token();
                    self.parse_arrow_function_from_paren(true)
                } else if self.peek_token.token_type == TokenType::Ident {
                    self.next_token();
                    if self.peek_token.token_type == TokenType::Arrow {
                        self.parse_arrow_function_single_param(true)
                    } else {
                        self.errors
                            .push("expected function or arrow after async".to_string());
                        None
                    }
                } else {
                    self.errors
                        .push("expected function or arrow after async".to_string());
                    None
                }
            }
            TokenType::LeftBracket => self.parse_array_literal(),
            TokenType::LeftBrace => self.parse_hash_literal(),
            _ => {
                self.errors.push(format!(
                    "no prefix parse function for {:?}",
                    self.cur_token.token_type
                ));
                None
            }
        }?;

        while self.peek_token.token_type != TokenType::Semicolon
            && precedence < self.peek_precedence()
        {
            match self.peek_token.token_type {
                TokenType::Plus
                | TokenType::Minus
                | TokenType::Slash
                | TokenType::Asterisk
                | TokenType::Equal
                | TokenType::NotEqual
                | TokenType::LessThan
                | TokenType::GreaterThan
                | TokenType::LessThanOrEqual
                | TokenType::GreaterThanOrEqual
                | TokenType::In
                | TokenType::Instanceof
                | TokenType::StrictEqual
                | TokenType::StrictNotEqual
                | TokenType::Percent
                | TokenType::Exponent
                | TokenType::BitwiseAnd
                | TokenType::BitwiseOr
                | TokenType::BitwiseXor
                | TokenType::LeftShift
                | TokenType::RightShift
                | TokenType::UnsignedRightShift
                | TokenType::And
                | TokenType::Or
                | TokenType::NullishCoalescing => {
                    self.next_token();
                    left = self.parse_infix_expression(left)?;
                }
                TokenType::Question => {
                    self.next_token();
                    left = self.parse_ternary_expression(left)?;
                }
                TokenType::Increment | TokenType::Decrement => {
                    self.next_token();
                    left = self.parse_update_postfix_expression(left)?;
                }
                TokenType::Assign
                | TokenType::PlusAssign
                | TokenType::MinusAssign
                | TokenType::MultiplyAssign
                | TokenType::DivideAssign
                | TokenType::PercentAssign
                | TokenType::ExponentAssign
                | TokenType::BitwiseAndAssign
                | TokenType::BitwiseOrAssign
                | TokenType::BitwiseXorAssign
                | TokenType::LeftShiftAssign
                | TokenType::RightShiftAssign
                | TokenType::UnsignedRightShiftAssign
                | TokenType::AndAssign
                | TokenType::OrAssign
                | TokenType::NullishAssign => {
                    self.next_token();
                    left = self.parse_assign_expression(left)?;
                }
                TokenType::LeftParen => {
                    self.next_token();
                    left = self.parse_call_expression(left)?;
                }
                TokenType::OptionalChain => {
                    self.next_token();
                    left = self.parse_optional_chain_expression(left)?;
                }
                TokenType::LeftBracket => {
                    self.next_token();
                    left = self.parse_index_expression(left)?;
                }
                TokenType::Dot => {
                    self.next_token();
                    left = self.parse_dot_expression(left)?;
                }
                TokenType::Template => {
                    self.next_token();
                    left = self.parse_tagged_template(left)?;
                }
                _ => return Some(left),
            }
        }

        Some(left)
    }

    fn parse_grouped_expression(&mut self) -> Option<Expression> {
        if self.is_arrow_parameter_list_from_current_paren() {
            return self.parse_arrow_function_from_paren(false);
        }

        self.next_token();
        let mut expr = self.parse_expression(Precedence::Lowest)?;

        while self.peek_token.token_type == TokenType::Comma {
            self.next_token();
            self.next_token();
            let right = self.parse_expression(Precedence::Lowest)?;
            expr = Expression::Infix {
                left: Box::new(expr),
                operator: ",".to_string(),
                right: Box::new(right),
            };
        }

        if !self.expect_peek(TokenType::RightParen) {
            return None;
        }
        Some(expr)
    }

    fn parse_new_expression(&mut self) -> Option<Expression> {
        // Check for `new.target` meta-property
        if self.peek_token.token_type == TokenType::Dot {
            self.next_token(); // consume '.'
            if self.cur_token.token_type == TokenType::Dot && self.peek_token.literal == "target" {
                self.next_token(); // consume 'target'
                return Some(Expression::NewTarget);
            }
            // Not `new.target` — this is a syntax error (new. without target)
            self.errors
                .push("expected 'target' after 'new.'".to_string());
            return None;
        }
        self.next_token();
        let callee = self.parse_expression(Precedence::Call)?;
        if self.peek_token.token_type != TokenType::LeftParen {
            return Some(Expression::New {
                callee: Box::new(callee),
                arguments: vec![],
            });
        }
        self.next_token();
        let args = self.parse_expression_list(TokenType::RightParen)?;
        Some(Expression::New {
            callee: Box::new(callee),
            arguments: args,
        })
    }

    fn parse_import_meta_expression(&mut self) -> Option<Expression> {
        // cur_token is `import`; check for `import.meta`
        if self.peek_token.token_type == TokenType::Dot {
            self.next_token(); // consume '.'
            if self.peek_token.literal == "meta" {
                self.next_token(); // consume 'meta'
                return Some(Expression::ImportMeta);
            }
            self.errors
                .push("expected 'meta' after 'import.'".to_string());
            return None;
        }
        self.errors
            .push("unexpected 'import' (modules not supported)".to_string());
        None
    }

    fn parse_await_expression(&mut self) -> Option<Expression> {
        self.next_token();
        let expr = self.parse_expression(Precedence::Prefix)?;
        Some(Expression::Await {
            value: Box::new(expr),
        })
    }

    fn parse_yield_expression(&mut self) -> Option<Expression> {
        // Check for `yield*` (delegate)
        let delegate = if self.peek_token.token_type == TokenType::Asterisk {
            self.next_token(); // consume '*'
            true
        } else {
            false
        };
        // yield with no value: `yield;` or `yield }` or `yield)`
        if !delegate
            && matches!(
                self.peek_token.token_type,
                TokenType::Semicolon
                    | TokenType::RightBrace
                    | TokenType::RightParen
                    | TokenType::RightBracket
                    | TokenType::Comma
                    | TokenType::Eof
            )
        {
            return Some(Expression::Yield {
                value: Box::new(Expression::Identifier("undefined".to_string())),
                delegate: false,
            });
        }
        self.next_token();
        let expr = self.parse_expression(Precedence::Assign)?;
        Some(Expression::Yield {
            value: Box::new(expr),
            delegate,
        })
    }

    fn parse_typeof_expression(&mut self) -> Option<Expression> {
        self.next_token();
        let expr = self.parse_expression(Precedence::Prefix)?;
        Some(Expression::Typeof {
            value: Box::new(expr),
        })
    }

    fn parse_void_expression(&mut self) -> Option<Expression> {
        self.next_token();
        let expr = self.parse_expression(Precedence::Prefix)?;
        Some(Expression::Void {
            value: Box::new(expr),
        })
    }

    fn parse_delete_expression(&mut self) -> Option<Expression> {
        self.next_token();
        let expr = self.parse_expression(Precedence::Prefix)?;
        Some(Expression::Delete {
            value: Box::new(expr),
        })
    }

    fn parse_prefix_expression(&mut self) -> Option<Expression> {
        let op = self.cur_token.literal.clone();
        self.next_token();
        let right = self.parse_expression(Precedence::Prefix)?;
        Some(Expression::Prefix {
            operator: op,
            right: Box::new(right),
        })
    }

    fn parse_update_prefix_expression(&mut self) -> Option<Expression> {
        let op = self.cur_token.literal.clone();
        self.next_token();
        let target = self.parse_expression(Precedence::Prefix)?;
        if !self.is_update_target(&target) {
            self.errors.push(
                "increment/decrement target must be identifier or property access".to_string(),
            );
            return None;
        }
        Some(Expression::Update {
            target: Box::new(target),
            operator: op,
            prefix: true,
        })
    }

    fn parse_update_postfix_expression(&mut self, target: Expression) -> Option<Expression> {
        if !self.is_update_target(&target) {
            self.errors.push(
                "increment/decrement target must be identifier or property access".to_string(),
            );
            return None;
        }
        Some(Expression::Update {
            target: Box::new(target),
            operator: self.cur_token.literal.clone(),
            prefix: false,
        })
    }

    fn is_update_target(&self, expr: &Expression) -> bool {
        matches!(expr, Expression::Identifier(_) | Expression::Index { .. })
    }

    fn parse_arrow_function_single_param(&mut self, is_async: bool) -> Option<Expression> {
        let param = self.cur_token.literal.clone();
        if !self.expect_peek(TokenType::Arrow) {
            return None;
        }

        self.next_token();
        let body = if self.cur_token.token_type == TokenType::LeftBrace {
            self.parse_block_statement()
        } else {
            let expr = self.parse_expression(Precedence::Lowest)?;
            vec![Statement::Expression(expr)]
        };

        Some(Expression::Function {
            parameters: vec![param],
            body,
            is_async,
            is_generator: false,
            is_arrow: true,
        })
    }

    fn is_arrow_parameter_list_from_current_paren(&self) -> bool {
        if self.cur_token.token_type != TokenType::LeftParen {
            return false;
        }

        let mut probe = self.clone_for_probe();
        if probe.parse_function_parameters().is_none() {
            return false;
        }

        probe.peek_token.token_type == TokenType::Arrow
    }

    fn parse_arrow_function_from_paren(&mut self, is_async: bool) -> Option<Expression> {
        if self.cur_token.token_type != TokenType::LeftParen {
            self.errors
                .push("expected '(' before arrow function parameters".to_string());
            return None;
        }

        let parsed_params = self.parse_function_parameters()?;

        if self.peek_token.token_type != TokenType::Arrow {
            self.errors
                .push("expected '=>' after arrow function parameters".to_string());
            return None;
        }

        self.next_token(); // '=>'
        self.next_token(); // body start

        let raw_body = if self.cur_token.token_type == TokenType::LeftBrace {
            self.parse_block_statement()
        } else {
            let expr = self.parse_expression(Precedence::Lowest)?;
            vec![Statement::Expression(expr)]
        };

        let body = Self::prepend_param_prologue(raw_body, &parsed_params.prologue);

        Some(Expression::Function {
            parameters: parsed_params.names,
            body,
            is_async,
            is_generator: false,
            is_arrow: true,
        })
    }

    fn parse_infix_expression(&mut self, left: Expression) -> Option<Expression> {
        let op = self.cur_token.literal.clone();
        let precedence = self.cur_precedence();
        self.next_token();
        let right_precedence = if op == "**" {
            Precedence::Product
        } else {
            precedence
        };
        let right = self.parse_expression(right_precedence)?;
        Some(Expression::Infix {
            left: Box::new(left),
            operator: op,
            right: Box::new(right),
        })
    }

    fn parse_assign_expression(&mut self, left: Expression) -> Option<Expression> {
        let op = self.cur_token.literal.clone();
        if op == "=" {
            if let Err(err) = Self::validate_assignment_pattern_target(&left) {
                self.errors.push(err);
                return None;
            }
        }
        self.next_token();
        // Use Lowest so that assignment is right-associative:
        // `a = b = c = 5` parses as `a = (b = (c = 5))`
        let right = self.parse_expression(Precedence::Lowest)?;
        Some(Expression::Assign {
            left: Box::new(left),
            operator: op,
            right: Box::new(right),
        })
    }

    fn parse_ternary_expression(&mut self, condition: Expression) -> Option<Expression> {
        self.next_token();
        let consequence_expr = self.parse_expression(Precedence::Assign)?;

        if !self.expect_peek(TokenType::Colon) {
            return None;
        }

        self.next_token();
        let alternative_expr = self.parse_expression(Precedence::Assign)?;

        Some(Expression::If {
            condition: Box::new(condition),
            consequence: vec![Statement::Expression(consequence_expr)],
            alternative: Some(vec![Statement::Expression(alternative_expr)]),
        })
    }

    fn parse_if_expression(&mut self) -> Option<Expression> {
        if !self.expect_peek(TokenType::LeftParen) {
            return None;
        }
        self.next_token();
        let condition = self.parse_expression(Precedence::Lowest)?;
        if !self.expect_peek(TokenType::RightParen) {
            return None;
        }

        // Support both braced `if (...) { ... }` and braceless `if (...) stmt;`
        let consequence = if self.peek_token.token_type == TokenType::LeftBrace {
            self.next_token(); // consume {
            self.parse_block_statement()
        } else {
            self.next_token(); // move to the single statement
            let stmt = self.parse_statement();
            match stmt {
                Some(s) => vec![s],
                None => vec![],
            }
        };

        let alternative = if self.peek_token.token_type == TokenType::Else {
            self.next_token();
            if self.peek_token.token_type == TokenType::LeftBrace {
                self.next_token(); // consume {
                Some(self.parse_block_statement())
            } else if self.peek_token.token_type == TokenType::If {
                // else if chain
                self.next_token();
                let elif = self.parse_if_expression()?;
                Some(vec![Statement::Expression(elif)])
            } else {
                self.next_token();
                let stmt = self.parse_statement();
                Some(match stmt {
                    Some(s) => vec![s],
                    None => vec![],
                })
            }
        } else {
            None
        };

        Some(Expression::If {
            condition: Box::new(condition),
            consequence,
            alternative,
        })
    }

    fn parse_block_statement(&mut self) -> Vec<Statement> {
        let mut out = vec![];
        self.next_token();
        while self.cur_token.token_type != TokenType::RightBrace
            && self.cur_token.token_type != TokenType::Eof
        {
            if let Some(stmt) = self.parse_statement() {
                out.push(stmt);
            }
            self.next_token();
        }
        out
    }

    fn parse_function_literal(&mut self, is_async: bool) -> Option<Expression> {
        // Check for function* (generator expression)
        let is_generator = if self.cur_token.token_type == TokenType::Function
            && self.peek_token.token_type == TokenType::Asterisk
        {
            self.next_token(); // consume '*'
            true
        } else {
            false
        };
        if !self.expect_peek(TokenType::LeftParen) {
            return None;
        }
        let parsed_params = self.parse_function_parameters()?;
        if !self.expect_peek(TokenType::LeftBrace) {
            return None;
        }
        let raw_body = self.parse_block_statement();
        let body = Self::prepend_param_prologue(raw_body, &parsed_params.prologue);
        Some(Expression::Function {
            parameters: parsed_params.names,
            body,
            is_async,
            is_generator,
            is_arrow: false,
        })
    }

    fn parse_function_parameters(&mut self) -> Option<ParsedParameters> {
        let mut params = vec![];
        let mut prologue = vec![];
        let mut pattern_index = 0usize;
        if self.peek_token.token_type == TokenType::RightParen {
            self.next_token();
            return Some(ParsedParameters {
                names: params,
                prologue,
            });
        }

        self.next_token();
        loop {
            if self.cur_token.token_type == TokenType::Spread {
                self.next_token();
                if self.cur_token.token_type != TokenType::Ident {
                    self.errors
                        .push("rest parameter must be an identifier".to_string());
                    return None;
                }
                if self.peek_token.token_type == TokenType::Assign {
                    self.errors
                        .push("rest parameter cannot have default".to_string());
                    return None;
                }
                params.push(format!("...{}", self.cur_token.literal));
                if self.peek_token.token_type != TokenType::RightParen {
                    self.errors.push("rest parameter must be last".to_string());
                    return None;
                }
                break;
            }

            if self.cur_token.token_type == TokenType::Ident {
                let param_name = self.cur_token.literal.clone();
                params.push(param_name.clone());
                if self.peek_token.token_type == TokenType::Assign {
                    self.next_token();
                    self.next_token();
                    let default_expr = self.parse_expression(Precedence::Assign)?;
                    prologue.push(Self::param_default_assignment(&param_name, default_expr));
                }
            } else if self.cur_token.token_type == TokenType::LeftBracket
                || self.cur_token.token_type == TokenType::LeftBrace
            {
                let pattern = self.parse_binding_pattern()?;
                let param_name = format!("__fl_param_{}", pattern_index);
                pattern_index += 1;
                params.push(param_name.clone());

                if self.peek_token.token_type == TokenType::Assign {
                    self.next_token();
                    self.next_token();
                    let default_expr = self.parse_expression(Precedence::Assign)?;
                    prologue.push(Self::param_default_assignment(&param_name, default_expr));
                }

                prologue.push(Statement::LetPattern {
                    pattern,
                    value: Expression::Identifier(param_name),
                    kind: VariableKind::Let,
                });
            } else {
                self.errors
                    .push("expected function parameter identifier or pattern".to_string());
                return None;
            }

            if self.peek_token.token_type == TokenType::Comma {
                self.next_token();
                self.next_token();
                continue;
            }
            break;
        }

        if !self.expect_peek(TokenType::RightParen) {
            return None;
        }

        Some(ParsedParameters {
            names: params,
            prologue,
        })
    }

    fn prepend_param_prologue(mut body: Vec<Statement>, prologue: &[Statement]) -> Vec<Statement> {
        if prologue.is_empty() {
            return body;
        }

        let mut prefix = prologue.to_vec();

        prefix.append(&mut body);
        prefix
    }

    fn param_default_assignment(name: &str, default_expr: Expression) -> Statement {
        Statement::Expression(Expression::Assign {
            left: Box::new(Expression::Identifier(name.to_string())),
            operator: "=".to_string(),
            right: Box::new(Expression::If {
                condition: Box::new(Expression::Infix {
                    left: Box::new(Expression::Typeof {
                        value: Box::new(Expression::Identifier(name.to_string())),
                    }),
                    operator: "===".to_string(),
                    right: Box::new(Expression::String("undefined".to_string())),
                }),
                consequence: vec![Statement::Expression(default_expr)],
                alternative: Some(vec![Statement::Expression(Expression::Identifier(
                    name.to_string(),
                ))]),
            }),
        })
    }

    fn parse_call_expression(&mut self, function: Expression) -> Option<Expression> {
        let args = self.parse_expression_list(TokenType::RightParen)?;
        Some(Expression::Call {
            function: Box::new(function),
            arguments: args,
        })
    }

    fn parse_optional_chain_expression(&mut self, left: Expression) -> Option<Expression> {
        if self.peek_token.token_type == TokenType::LeftParen {
            self.next_token();
            let args = self.parse_expression_list(TokenType::RightParen)?;
            return Some(Expression::OptionalCall {
                function: Box::new(left),
                arguments: args,
            });
        }

        if self.peek_token.token_type == TokenType::LeftBracket {
            self.next_token();
            self.next_token();
            let index = self.parse_expression(Precedence::Lowest)?;
            if !self.expect_peek(TokenType::RightBracket) {
                return None;
            }
            return Some(Expression::OptionalIndex {
                left: Box::new(left),
                index: Box::new(index),
            });
        }

        if Self::is_identifier_name_token(self.peek_token.token_type) {
            self.next_token();
            let index = Expression::String(self.cur_token.literal.clone());
            return Some(Expression::OptionalIndex {
                left: Box::new(left),
                index: Box::new(index),
            });
        }

        self.errors
            .push("expected property/index/call after optional chain".to_string());
        None
    }

    fn parse_index_expression(&mut self, left: Expression) -> Option<Expression> {
        self.next_token();
        let index = self.parse_expression(Precedence::Lowest)?;
        if !self.expect_peek(TokenType::RightBracket) {
            return None;
        }
        Some(Expression::Index {
            left: Box::new(left),
            index: Box::new(index),
        })
    }

    fn parse_dot_expression(&mut self, left: Expression) -> Option<Expression> {
        // Handle private field access: obj.#name
        if self.peek_token.token_type == TokenType::Hash {
            self.next_token(); // consume `#`
            if self.peek_token.token_type != TokenType::Ident {
                self.errors.push(format!(
                    "expected identifier after '.#', got {:?}",
                    self.peek_token.token_type
                ));
                return None;
            }
            self.next_token(); // consume the identifier
            let property = Expression::String(format!("#{}", self.cur_token.literal));
            return Some(Expression::Index {
                left: Box::new(left),
                index: Box::new(property),
            });
        }

        if !Self::is_identifier_name_token(self.peek_token.token_type) {
            self.errors.push(format!(
                "expected property name after '.', got {:?}",
                self.peek_token.token_type
            ));
            return None;
        }
        self.next_token();
        let property = Expression::String(self.cur_token.literal.clone());
        Some(Expression::Index {
            left: Box::new(left),
            index: Box::new(property),
        })
    }

    fn is_identifier_name_token(token_type: TokenType) -> bool {
        matches!(
            token_type,
            TokenType::Ident
                | TokenType::Function
                | TokenType::Let
                | TokenType::Var
                | TokenType::Const
                | TokenType::True
                | TokenType::False
                | TokenType::If
                | TokenType::Else
                | TokenType::Return
                | TokenType::Null
                | TokenType::Undefined
                | TokenType::While
                | TokenType::For
                | TokenType::Break
                | TokenType::Continue
                | TokenType::Throw
                | TokenType::Try
                | TokenType::Catch
                | TokenType::Finally
                | TokenType::Class
                | TokenType::Extends
                | TokenType::New
                | TokenType::This
                | TokenType::Super
                | TokenType::Static
                | TokenType::Async
                | TokenType::Await
                | TokenType::Of
                | TokenType::In
                | TokenType::Do
                | TokenType::Switch
                | TokenType::Case
                | TokenType::Default
                | TokenType::Get
                | TokenType::Set
                | TokenType::Typeof
                | TokenType::Instanceof
                | TokenType::Void
                | TokenType::Delete
                | TokenType::Yield
                | TokenType::Import
                | TokenType::Debugger
        )
    }

    fn parse_array_literal(&mut self) -> Option<Expression> {
        let elements = self.parse_expression_list(TokenType::RightBracket)?;
        Some(Expression::Array(elements))
    }

    fn parse_hash_literal(&mut self) -> Option<Expression> {
        let mut entries = vec![];
        if self.peek_token.token_type == TokenType::RightBrace {
            self.next_token();
            return Some(Expression::Hash(entries));
        }

        loop {
            if self.peek_token.token_type == TokenType::RightBrace {
                break;
            }

            self.next_token();

            // ── Spread ──────────────────────────────────────────────
            if self.cur_token.token_type == TokenType::Spread {
                self.next_token();
                let spread_value = self.parse_expression(Precedence::Lowest)?;
                entries.push(HashEntry::Spread(spread_value));

                if self.peek_token.token_type == TokenType::Comma {
                    self.next_token();
                    continue;
                }
                if self.peek_token.token_type == TokenType::RightBrace {
                    break;
                }

                self.errors
                    .push("expected ',' or '}' after object spread".to_string());
                return None;
            }

            // ── Getter / Setter ─────────────────────────────────────
            // `get name() { ... }` or `set name(param) { ... }`
            // But NOT `get: value` (identifier shorthand or key-value).
            if (self.cur_token.token_type == TokenType::Get
                || self.cur_token.token_type == TokenType::Set)
                && self.peek_token.token_type != TokenType::Colon
                && self.peek_token.token_type != TokenType::Comma
                && self.peek_token.token_type != TokenType::RightBrace
                && self.peek_token.token_type != TokenType::LeftParen
            {
                let is_getter = self.cur_token.token_type == TokenType::Get;
                self.next_token();

                // Parse the key (identifier or computed)
                let is_computed = self.cur_token.token_type == TokenType::LeftBracket;
                let key = if is_computed {
                    self.next_token();
                    let k = self.parse_expression(Precedence::Lowest)?;
                    if !self.expect_peek(TokenType::RightBracket) {
                        return None;
                    }
                    k
                } else {
                    Expression::String(self.cur_token.literal.clone())
                };

                if !self.expect_peek(TokenType::LeftParen) {
                    return None;
                }

                if is_getter {
                    // get name() { ... }
                    if !self.expect_peek(TokenType::RightParen) {
                        return None;
                    }
                    if !self.expect_peek(TokenType::LeftBrace) {
                        return None;
                    }
                    let body = self.parse_block_statement();
                    entries.push(HashEntry::Getter { key, body });
                } else {
                    // set name(param) { ... }
                    if !self.expect_peek(TokenType::Ident) {
                        return None;
                    }
                    let parameter = self.cur_token.literal.clone();
                    if !self.expect_peek(TokenType::RightParen) {
                        return None;
                    }
                    if !self.expect_peek(TokenType::LeftBrace) {
                        return None;
                    }
                    let body = self.parse_block_statement();
                    entries.push(HashEntry::Setter {
                        key,
                        parameter,
                        body,
                    });
                }

                if self.peek_token.token_type == TokenType::Comma {
                    self.next_token();
                }
                continue;
            }

            // ── Generator method shorthand: *name() { ... } ────────
            if self.cur_token.token_type == TokenType::Asterisk {
                self.next_token(); // consume '*', now on method name
                let is_computed_key = self.cur_token.token_type == TokenType::LeftBracket;
                let key = if is_computed_key {
                    self.next_token();
                    let computed = self.parse_expression(Precedence::Lowest)?;
                    if !self.expect_peek(TokenType::RightBracket) {
                        return None;
                    }
                    computed
                } else if let TokenType::Ident = self.cur_token.token_type {
                    Expression::String(self.cur_token.literal.clone())
                } else {
                    self.parse_expression(Precedence::Index)?
                };
                if !self.expect_peek(TokenType::LeftParen) {
                    return None;
                }
                let parsed_params = self.parse_function_parameters()?;
                if !self.expect_peek(TokenType::LeftBrace) {
                    return None;
                }
                let raw_body = self.parse_block_statement();
                let body = Self::prepend_param_prologue(raw_body, &parsed_params.prologue);
                entries.push(HashEntry::Method {
                    key,
                    parameters: parsed_params.names,
                    body,
                    is_async: false,
                    is_generator: true,
                });
                if self.peek_token.token_type == TokenType::Comma {
                    self.next_token();
                }
                continue;
            }

            // ── Computed key or regular key ──────────────────────────
            let is_computed_key = self.cur_token.token_type == TokenType::LeftBracket;
            let key = if is_computed_key {
                self.next_token();
                let computed = self.parse_expression(Precedence::Lowest)?;
                if !self.expect_peek(TokenType::RightBracket) {
                    return None;
                }
                computed
            } else {
                // Parse key at Index precedence (higher than Call) to avoid
                // consuming `(` in method shorthand `name() { ... }`.
                self.parse_expression(Precedence::Index)?
            };

            // ── Method shorthand ────────────────────────────────────
            // `name(params) { body }` or `[expr](params) { body }`
            if self.peek_token.token_type == TokenType::LeftParen {
                let method_key = if !is_computed_key {
                    if let Expression::Identifier(name) = &key {
                        Expression::String(name.clone())
                    } else {
                        key.clone()
                    }
                } else {
                    key.clone()
                };

                self.next_token(); // consume (
                let parsed_params = self.parse_function_parameters()?;
                if !self.expect_peek(TokenType::LeftBrace) {
                    return None;
                }
                let raw_body = self.parse_block_statement();
                let body = Self::prepend_param_prologue(raw_body, &parsed_params.prologue);
                entries.push(HashEntry::Method {
                    key: method_key,
                    parameters: parsed_params.names,
                    body,
                    is_async: false,
                    is_generator: false,
                });

                if self.peek_token.token_type == TokenType::Comma {
                    self.next_token();
                }
                continue;
            }

            // ── Key-value pair ──────────────────────────────────────
            let value = if self.peek_token.token_type == TokenType::Colon {
                self.next_token();
                self.next_token();
                self.parse_expression(Precedence::Lowest)?
            } else if !is_computed_key
                && (self.peek_token.token_type == TokenType::Comma
                    || self.peek_token.token_type == TokenType::RightBrace)
            {
                match &key {
                    Expression::Identifier(name) => {
                        entries.push(HashEntry::KeyValue {
                            key: Expression::String(name.clone()),
                            value: Expression::Identifier(name.clone()),
                        });
                        if self.peek_token.token_type != TokenType::Comma {
                            break;
                        }
                        self.next_token();
                        continue;
                    }
                    _ => {
                        self.errors
                            .push("object literal shorthand requires identifier key".to_string());
                        return None;
                    }
                }
            } else if self.peek_token.token_type == TokenType::Comma
                || self.peek_token.token_type == TokenType::RightBrace
            {
                self.errors
                    .push("computed object key requires ':', e.g. {[k]: value}".to_string());
                return None;
            } else {
                self.errors.push(
                    "expected ':' in object literal (or shorthand identifier property)".to_string(),
                );
                return None;
            };
            let stored_key = if !is_computed_key {
                if let Expression::Identifier(name) = &key {
                    Expression::String(name.clone())
                } else {
                    key
                }
            } else {
                key
            };
            entries.push(HashEntry::KeyValue {
                key: stored_key,
                value,
            });

            if self.peek_token.token_type != TokenType::Comma {
                break;
            }
            self.next_token();
        }

        if !self.expect_peek(TokenType::RightBrace) {
            return None;
        }
        Some(Expression::Hash(entries))
    }

    fn parse_expression_list(&mut self, end: TokenType) -> Option<Vec<Expression>> {
        let mut list = vec![];
        if self.peek_token.token_type == end {
            self.next_token();
            return Some(list);
        }

        self.next_token();
        list.push(self.parse_expression_list_item()?);
        while self.peek_token.token_type == TokenType::Comma {
            self.next_token();
            // Trailing comma: if next token is the closing delimiter, stop
            if self.peek_token.token_type == end {
                self.next_token();
                return Some(list);
            }
            self.next_token();
            list.push(self.parse_expression_list_item()?);
        }

        if !self.expect_peek(end) {
            return None;
        }
        Some(list)
    }

    fn parse_expression_list_item(&mut self) -> Option<Expression> {
        if self.cur_token.token_type != TokenType::Spread {
            return self.parse_expression(Precedence::Lowest);
        }

        self.next_token();
        let value = self.parse_expression(Precedence::Lowest)?;

        Some(Expression::Spread {
            value: Box::new(value),
        })
    }

    fn validate_assignment_pattern_target(expr: &Expression) -> Result<(), String> {
        match expr {
            Expression::Identifier(_) | Expression::Index { .. } => Ok(()),
            Expression::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    match item {
                        Expression::Spread { value } => {
                            if i + 1 != items.len() {
                                return Err("rest element in array binding must be last".to_string());
                            }
                            match &**value {
                                Expression::Identifier(_) => {}
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
                        }
                        Expression::Assign { left, operator, .. } => {
                            if operator != "=" {
                                return Err(
                                    "array destructuring defaults require '='".to_string(),
                                );
                            }
                            Self::validate_assignment_pattern_target(left)?;
                        }
                        Expression::Array(_) | Expression::Hash(_) => {
                            Self::validate_assignment_pattern_target(item)?;
                        }
                        Expression::Identifier(_) => {}
                        _ => {
                            return Err(
                                "array destructuring assignment supports identifier or nested pattern targets only"
                                    .to_string(),
                            )
                        }
                    }
                }
                Ok(())
            }
            Expression::Hash(pairs) => {
                for (i, entry) in pairs.iter().enumerate() {
                    match entry {
                        HashEntry::Spread(target_expr) => {
                            if i + 1 != pairs.len() {
                                return Err(
                                    "rest property in object pattern must be last".to_string(),
                                );
                            }
                            match target_expr {
                                Expression::Identifier(_) => {}
                                Expression::Assign { .. } => {
                                    return Err(
                                        "rest property in object pattern cannot have default"
                                            .to_string(),
                                    )
                                }
                                _ => {
                                    return Err(
                                        "object rest destructuring assignment requires identifier target"
                                            .to_string(),
                                    )
                                }
                            }
                        }
                        HashEntry::KeyValue {
                            key: _,
                            value: target_expr,
                        } => {
                            match target_expr {
                                Expression::Identifier(_) => {}
                                Expression::Assign { left, operator, .. } => {
                                    if operator != "=" {
                                        return Err(
                                            "object destructuring defaults require '='"
                                                .to_string(),
                                        );
                                    }
                                    Self::validate_assignment_pattern_target(left)?;
                                }
                                Expression::Array(_) | Expression::Hash(_) => {
                                    Self::validate_assignment_pattern_target(target_expr)?;
                                }
                                _ => {
                                    return Err(
                                        "object destructuring assignment supports identifier or nested pattern targets only"
                                            .to_string(),
                                    )
                                }
                            }
                        }
                        HashEntry::Getter { .. } | HashEntry::Setter { .. } | HashEntry::Method { .. } => {
                            return Err(
                                "methods/getters/setters not valid in destructuring patterns".to_string(),
                            );
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn parse_template_literal_expression(&mut self) -> Option<Expression> {
        let cooked = self.cur_token.literal.clone();
        if !cooked.contains("${") {
            return Some(Expression::String(cooked));
        }

        let parts = match Self::split_template_parts(&cooked) {
            Ok(parts) => parts,
            Err(err) => {
                self.errors.push(err);
                return None;
            }
        };

        let mut out: Option<Expression> = None;
        for (is_expr, segment) in parts {
            let next = if is_expr {
                match Self::parse_template_expression_source(&segment) {
                    Ok(expr) => expr,
                    Err(err) => {
                        self.errors.push(err);
                        return None;
                    }
                }
            } else {
                Expression::String(segment)
            };

            out = Some(match out {
                None => next,
                Some(prev) => Expression::Infix {
                    left: Box::new(prev),
                    operator: "+".to_string(),
                    right: Box::new(next),
                },
            });
        }

        Some(out.unwrap_or_else(|| Expression::String(String::new())))
    }

    /// Parse a tagged template: `tag\`hello ${expr}\``
    /// `cur_token` is the Template token; `tag` is the already-parsed tag expression.
    /// Desugars to: Call { function: tag, arguments: [Array(cooked_strings), ...expressions] }
    fn parse_tagged_template(&mut self, tag: Expression) -> Option<Expression> {
        let cooked = self.cur_token.literal.clone();
        let raw_str = self
            .cur_token
            .raw_literal
            .clone()
            .unwrap_or_else(|| cooked.clone());

        // No interpolations: tag`plain string`
        if !cooked.contains("${") {
            let raw_array = Expression::Array(vec![Expression::String(raw_str)]);
            let strings_obj = Expression::Hash(vec![
                HashEntry::KeyValue {
                    key: Expression::Integer(0),
                    value: Expression::String(cooked),
                },
                HashEntry::KeyValue {
                    key: Expression::String("length".to_string()),
                    value: Expression::Integer(1),
                },
                HashEntry::KeyValue {
                    key: Expression::String("raw".to_string()),
                    value: raw_array,
                },
            ]);
            return Some(Expression::Call {
                function: Box::new(tag),
                arguments: vec![strings_obj],
            });
        }

        // Split the cooked template to get string parts and expression parts.
        let cooked_parts = match Self::split_template_parts(&cooked) {
            Ok(p) => p,
            Err(e) => {
                self.errors.push(e);
                return None;
            }
        };
        let raw_parts = match Self::split_template_parts(&raw_str) {
            Ok(p) => p,
            Err(e) => {
                self.errors.push(e);
                return None;
            }
        };

        let mut quasis: Vec<String> = vec![];
        let mut raw_quasis: Vec<String> = vec![];
        let mut expressions: Vec<Expression> = vec![];

        for (is_expr, segment) in &cooked_parts {
            if *is_expr {
                match Self::parse_template_expression_source(segment) {
                    Ok(expr) => expressions.push(expr),
                    Err(err) => {
                        self.errors.push(err);
                        return None;
                    }
                }
            } else {
                quasis.push(segment.clone());
            }
        }
        for (is_expr, segment) in &raw_parts {
            if !is_expr {
                raw_quasis.push(segment.clone());
            }
        }

        // Build the strings object as a Hash with numeric keys + length + raw property.
        // This makes it array-like: strings[0], strings[1], ..., strings.length, strings.raw
        let mut entries: Vec<HashEntry> = vec![];
        for (i, q) in quasis.iter().enumerate() {
            entries.push(HashEntry::KeyValue {
                key: Expression::Integer(i as i64),
                value: Expression::String(q.clone()),
            });
        }
        entries.push(HashEntry::KeyValue {
            key: Expression::String("length".to_string()),
            value: Expression::Integer(quasis.len() as i64),
        });
        entries.push(HashEntry::KeyValue {
            key: Expression::String("raw".to_string()),
            value: Expression::Array(raw_quasis.into_iter().map(Expression::String).collect()),
        });

        let mut arguments = vec![Expression::Hash(entries)];
        arguments.extend(expressions);

        Some(Expression::Call {
            function: Box::new(tag),
            arguments,
        })
    }

    fn split_template_parts(raw: &str) -> Result<Vec<(bool, String)>, String> {
        let bytes = raw.as_bytes();
        let mut parts: Vec<(bool, String)> = vec![];
        let mut i = 0usize;
        let mut text_start = 0usize;

        while i + 1 < bytes.len() {
            if bytes[i] == b'$' && bytes[i + 1] == b'{' {
                // Always push the text segment (even if empty) to maintain
                // the invariant that strings.length == expressions.length + 1
                parts.push((false, raw[text_start..i].to_string()));

                i += 2;
                let expr_start = i;
                let mut depth = 1i32;

                while i < bytes.len() {
                    let ch = bytes[i];
                    if ch == b'\\' {
                        i += 2;
                        continue;
                    }

                    if ch == b'\'' || ch == b'"' || ch == b'`' {
                        let quote = ch;
                        i += 1;
                        while i < bytes.len() {
                            if bytes[i] == b'\\' {
                                i += 2;
                                continue;
                            }
                            if bytes[i] == quote {
                                i += 1;
                                break;
                            }
                            i += 1;
                        }
                        continue;
                    }

                    if ch == b'{' {
                        depth += 1;
                        i += 1;
                        continue;
                    }

                    if ch == b'}' {
                        depth -= 1;
                        if depth == 0 {
                            parts.push((true, raw[expr_start..i].to_string()));
                            i += 1;
                            text_start = i;
                            break;
                        }
                        i += 1;
                        continue;
                    }

                    i += 1;
                }

                if depth != 0 {
                    return Err("unterminated template interpolation".to_string());
                }

                continue;
            }

            i += 1;
        }

        // Always push the trailing text segment (even if empty) to maintain
        // the invariant that strings.length == expressions.length + 1
        if text_start <= raw.len() {
            parts.push((false, raw[text_start..].to_string()));
        }

        Ok(parts)
    }

    fn parse_template_expression_source(source: &str) -> Result<Expression, String> {
        let mut parser = Parser::new(Lexer::new(source, FormLogicConfig::default()));
        let program = parser.parse_program();
        if !parser.errors.is_empty() {
            return Err(format!(
                "template interpolation parse error: {}",
                parser.errors.join(", ")
            ));
        }

        if program.statements.len() != 1 {
            return Err("template interpolation must contain exactly one expression".to_string());
        }

        match &program.statements[0] {
            Statement::Expression(expr) => Ok(expr.clone()),
            _ => Err("template interpolation must contain an expression".to_string()),
        }
    }

    /// Parse a potentially comma-separated sequence of expressions.
    /// Returns a single expression if no commas, or `Expression::Sequence` for `a, b, c`.
    fn parse_sequence_expression(&mut self) -> Option<Expression> {
        let first = self.parse_expression(Precedence::Lowest)?;
        if self.peek_token.token_type != TokenType::Comma {
            return Some(first);
        }
        let mut exprs = vec![first];
        while self.peek_token.token_type == TokenType::Comma {
            self.next_token(); // consume comma
            self.next_token(); // start of next expr
            exprs.push(self.parse_expression(Precedence::Lowest)?);
        }
        Some(Expression::Sequence(exprs))
    }

    fn expect_peek(&mut self, token_type: TokenType) -> bool {
        if self.peek_token.token_type == token_type {
            self.next_token();
            true
        } else {
            self.errors.push(format!(
                "expected next token to be {:?}, got {:?} instead (line {}, col {})",
                token_type,
                self.peek_token.token_type,
                self.peek_token.line,
                self.peek_token.column
            ));
            false
        }
    }

    fn peek_precedence(&self) -> Precedence {
        token_precedence(self.peek_token.token_type)
    }

    fn cur_precedence(&self) -> Precedence {
        token_precedence(self.cur_token.token_type)
    }
}

fn token_precedence(token_type: TokenType) -> Precedence {
    match token_type {
        TokenType::Assign
        | TokenType::PlusAssign
        | TokenType::MinusAssign
        | TokenType::MultiplyAssign
        | TokenType::DivideAssign
        | TokenType::PercentAssign
        | TokenType::ExponentAssign
        | TokenType::BitwiseAndAssign
        | TokenType::BitwiseOrAssign
        | TokenType::BitwiseXorAssign
        | TokenType::LeftShiftAssign
        | TokenType::RightShiftAssign
        | TokenType::UnsignedRightShiftAssign
        | TokenType::AndAssign
        | TokenType::OrAssign
        | TokenType::NullishAssign => Precedence::Assign,
        TokenType::Question => Precedence::Conditional,
        TokenType::Or => Precedence::LogicalOr,
        TokenType::NullishCoalescing => Precedence::Nullish,
        TokenType::And => Precedence::LogicalAnd,
        TokenType::BitwiseOr => Precedence::BitwiseOr,
        TokenType::BitwiseXor => Precedence::BitwiseXor,
        TokenType::BitwiseAnd => Precedence::BitwiseAnd,
        TokenType::Equal | TokenType::NotEqual => Precedence::Equals,
        TokenType::StrictEqual | TokenType::StrictNotEqual => Precedence::Equals,
        TokenType::LessThan
        | TokenType::GreaterThan
        | TokenType::LessThanOrEqual
        | TokenType::GreaterThanOrEqual
        | TokenType::In
        | TokenType::Instanceof => Precedence::LessGreater,
        TokenType::LeftShift | TokenType::RightShift | TokenType::UnsignedRightShift => {
            Precedence::Shift
        }
        TokenType::Plus | TokenType::Minus => Precedence::Sum,
        TokenType::Slash | TokenType::Asterisk | TokenType::Percent => Precedence::Product,
        TokenType::Exponent => Precedence::Exponent,
        TokenType::Increment | TokenType::Decrement => Precedence::Index,
        TokenType::LeftParen => Precedence::Call,
        TokenType::Template => Precedence::Call,
        TokenType::LeftBracket | TokenType::Dot | TokenType::OptionalChain => Precedence::Index,
        _ => Precedence::Lowest,
    }
}

pub fn parse_program_from_source(input: &str) -> (Program, Vec<String>) {
    let lexer = Lexer::new(input, FormLogicConfig::default());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    (program, parser.errors)
}

#[cfg(test)]
mod tests {
    use crate::ast::{Expression, Statement};
    use crate::config::FormLogicConfig;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    #[test]
    fn parses_let_and_return_statements() {
        let input = "let x = 5; return x;";
        let lexer = Lexer::new(input, FormLogicConfig::default());
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program();
        assert!(
            parser.errors.is_empty(),
            "parser errors: {:?}",
            parser.errors
        );
        assert_eq!(program.statements.len(), 2);

        match &program.statements[0] {
            Statement::Let { name, value, .. } => {
                assert_eq!(name, "x");
                assert!(matches!(value, Expression::Integer(5)));
            }
            _ => panic!("expected let statement"),
        }

        match &program.statements[1] {
            Statement::Return { value } => {
                assert!(matches!(value, Expression::Identifier(s) if s == "x"));
            }
            _ => panic!("expected return statement"),
        }
    }

    #[test]
    fn parses_infix_precedence() {
        let input = "1 + 2 * 3";
        let lexer = Lexer::new(input, FormLogicConfig::default());
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program();
        assert!(
            parser.errors.is_empty(),
            "parser errors: {:?}",
            parser.errors
        );

        match &program.statements[0] {
            Statement::Expression(Expression::Infix {
                left,
                operator,
                right,
            }) => {
                assert_eq!(operator, "+");
                assert!(matches!(**left, Expression::Integer(1)));
                match &**right {
                    Expression::Infix {
                        left: r_left,
                        operator: r_op,
                        right: r_right,
                    } => {
                        assert_eq!(r_op, "*");
                        assert!(matches!(**r_left, Expression::Integer(2)));
                        assert!(matches!(**r_right, Expression::Integer(3)));
                    }
                    _ => panic!("expected nested infix"),
                }
            }
            _ => panic!("expected expression statement"),
        }
    }
}
