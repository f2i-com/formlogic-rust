#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub statements: Vec<Statement>,
}

impl Program {
    pub fn new() -> Self {
        Self { statements: vec![] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariableKind {
    Let,
    Const,
    Var,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Let {
        name: String,
        value: Expression,
        kind: VariableKind,
    },
    LetPattern {
        pattern: BindingPattern,
        value: Expression,
        kind: VariableKind,
    },
    Return {
        value: Expression,
    },
    ReturnVoid,
    Expression(Expression),
    Block(Vec<Statement>),
    /// Multiple let/const/var declarations from `let a = 1, b = 2;`.
    /// Unlike Block, this does NOT introduce a new lexical scope.
    MultiLet(Vec<Statement>),
    While {
        condition: Expression,
        body: Vec<Statement>,
    },
    For {
        init: Option<Box<Statement>>,
        condition: Option<Expression>,
        update: Option<Expression>,
        body: Vec<Statement>,
    },
    ForOf {
        binding: ForBinding,
        iterable: Expression,
        body: Vec<Statement>,
    },
    ForIn {
        var_name: String,
        iterable: Expression,
        body: Vec<Statement>,
    },
    FunctionDecl {
        name: String,
        parameters: Vec<String>,
        body: Vec<Statement>,
        is_async: bool,
        is_generator: bool,
    },
    ClassDecl {
        name: Option<String>,
        extends: Option<Box<Expression>>,
        members: Vec<ClassMember>,
    },
    Throw {
        value: Expression,
    },
    Try {
        try_block: Vec<Statement>,
        catch_param: Option<String>,
        catch_block: Option<Vec<Statement>>,
        finally_block: Option<Vec<Statement>>,
    },
    Labeled {
        label: String,
        statement: Box<Statement>,
    },
    Break {
        label: Option<String>,
    },
    Continue {
        label: Option<String>,
    },
    DoWhile {
        body: Vec<Statement>,
        condition: Expression,
    },
    Switch {
        discriminant: Expression,
        cases: Vec<SwitchCase>,
    },
    Debugger,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BindingPattern {
    Array(Vec<ArrayBindingItem>),
    Object(Vec<ObjectBindingItem>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum BindingTarget {
    Identifier(String),
    Pattern(Box<BindingPattern>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ForBinding {
    Identifier(String),
    Pattern(BindingPattern),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArrayBindingItem {
    Hole,
    Binding {
        target: BindingTarget,
        default_value: Option<Expression>,
    },
    Rest {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectBindingItem {
    pub key: Expression,
    pub target: BindingTarget,
    pub default_value: Option<Expression>,
    pub is_rest: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassMethod {
    pub name: String,
    pub parameters: Vec<String>,
    pub body: Vec<Statement>,
    pub is_static: bool,
    pub is_getter: bool,
    pub is_setter: bool,
}

/// A member inside a class body — method, field, or static block.
#[derive(Clone, Debug, PartialEq)]
pub enum ClassMember {
    /// A method, constructor, getter, or setter.
    Method(ClassMethod),
    /// A field declaration: `name = expr;` or `static name = expr;` or `#name = expr;`
    Field {
        name: String,
        initializer: Option<Expression>,
        is_static: bool,
    },
    /// A static initialization block: `static { ... }`
    StaticBlock { body: Vec<Statement> },
}

/// An entry inside an object literal `{ ... }`.
#[derive(Clone, Debug, PartialEq)]
pub enum HashEntry {
    /// `key: value` or shorthand `name` (stored as key=String(name), value=Ident(name))
    KeyValue { key: Expression, value: Expression },
    /// Method shorthand: `name(params) { body }` (including computed `[expr](params) { body }`)
    Method {
        key: Expression,
        parameters: Vec<String>,
        body: Vec<Statement>,
        is_async: bool,
        is_generator: bool,
    },
    /// `get name() { body }` (including computed `get [expr]() { body }`)
    Getter {
        key: Expression,
        body: Vec<Statement>,
    },
    /// `set name(param) { body }` (including computed `set [expr](param) { body }`)
    Setter {
        key: Expression,
        parameter: String,
        body: Vec<Statement>,
    },
    /// `...expr`
    Spread(Expression),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwitchCase {
    /// `None` means `default:` case.
    pub test: Option<Expression>,
    pub consequent: Vec<Statement>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expression {
    Identifier(String),
    Integer(i64),
    Float(f64),
    String(String),
    RegExp {
        pattern: String,
        flags: String,
    },
    Boolean(bool),
    Null,
    Array(Vec<Expression>),
    Hash(Vec<HashEntry>),
    Prefix {
        operator: String,
        right: Box<Expression>,
    },
    Typeof {
        value: Box<Expression>,
    },
    Void {
        value: Box<Expression>,
    },
    Delete {
        value: Box<Expression>,
    },
    Infix {
        left: Box<Expression>,
        operator: String,
        right: Box<Expression>,
    },
    If {
        condition: Box<Expression>,
        consequence: Vec<Statement>,
        alternative: Option<Vec<Statement>>,
    },
    Function {
        parameters: Vec<String>,
        body: Vec<Statement>,
        is_async: bool,
        is_generator: bool,
        is_arrow: bool,
    },
    This,
    Super,
    Await {
        value: Box<Expression>,
    },
    New {
        callee: Box<Expression>,
        arguments: Vec<Expression>,
    },
    Call {
        function: Box<Expression>,
        arguments: Vec<Expression>,
    },
    OptionalIndex {
        left: Box<Expression>,
        index: Box<Expression>,
    },
    OptionalCall {
        function: Box<Expression>,
        arguments: Vec<Expression>,
    },
    Assign {
        left: Box<Expression>,
        operator: String,
        right: Box<Expression>,
    },
    Update {
        target: Box<Expression>,
        operator: String,
        prefix: bool,
    },
    Spread {
        value: Box<Expression>,
    },
    Index {
        left: Box<Expression>,
        index: Box<Expression>,
    },
    /// Class expression: `let C = class [Name] [extends Expr] { ... }`
    Class {
        name: Option<String>,
        extends: Option<Box<Expression>>,
        members: Vec<ClassMember>,
    },
    /// `new.target` meta-property — reference to the constructor that was
    /// invoked with `new`.  `undefined` when called outside a constructor.
    NewTarget,
    /// `import.meta` meta-property — stub (returns empty object).
    ImportMeta,
    /// `yield expr` or `yield* expr` inside a generator function.
    Yield {
        value: Box<Expression>,
        delegate: bool,
    },
    /// Comma/sequence expression: `(a, b, c)` evaluates all, returns last.
    Sequence(Vec<Expression>),
}
