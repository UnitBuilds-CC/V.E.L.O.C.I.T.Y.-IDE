use super::token::*;

/// Variable declaration kind: `var`, `let`, `const`, or `using`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    Var,
    Let,
    Const,
    Using,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    VarDecl {
        kind: VarKind,
        name: String,
        init: Option<Expr>,
    },
    DestructureDecl {
        pattern: DestructurePattern,
        init: Expr,
    },
    Block(Vec<Stmt>),
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
    },
    DoWhile {
        body: Box<Stmt>,
        cond: Expr,
    },
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    ForIn {
        var_name: String,
        object: Expr,
        body: Box<Stmt>,
    },
    ForOf {
        var_name: String,
        object: Expr,
        body: Box<Stmt>,
    },
    /// `for await (x of y) { ... }` — awaits each value before binding.
    ForAwaitOf {
        var_name: String,
        object: Expr,
        body: Box<Stmt>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Throw(Expr),
    TryCatch {
        try_block: Box<Stmt>,
        catch_var: Option<String>,
        catch_block: Option<Box<Stmt>>,
        finally_block: Option<Box<Stmt>>,
    },
    FunctionDecl {
        name: String,
        params: Vec<String>,
        body: Box<Stmt>,
    },
    ClassDecl {
        name: String,
        parent: Option<String>,
        methods: Vec<ClassMethod>,
        fields: Vec<ClassField>,
    },
    /// import { a, b } from 'module' / import x from 'module' / import * as x from 'module'
    Import {
        specifiers: Vec<ImportSpecifier>,
        source: String,
    },
    /// export const x = ...; / export default expr; / export { a, b };
    Export {
        declaration: Option<Box<Stmt>>,
        default_expr: Option<Expr>,
        named: Vec<String>,
    },
    /// Generator function: function* name() { yield ... }
    GeneratorDecl {
        name: String,
        params: Vec<String>,
        body: Box<Stmt>,
    },
    /// Async function: async function name() { ... } — wraps return in Promise.
    AsyncFunctionDecl {
        name: String,
        params: Vec<String>,
        body: Box<Stmt>,
    },
    /// Labeled statement: label: stmt (we skip the label)
    Labeled {
        label: String,
        body: Box<Stmt>,
    },
    /// Switch statement: switch (discriminant) { case ...: ... default: ... }
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub pattern: Option<Expr>, // None for default
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum DestructurePattern {
    Object(Vec<(String, Option<String>)>), // [(key, optional_alias)]
    Array(Vec<Option<String>>),            // [Some(name), None for holes]
}

#[derive(Debug, Clone)]
pub struct ImportSpecifier {
    pub imported: String, // the name exported by the module (or "default" / "*")
    pub local: String,    // the local binding name
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassMemberKind {
    Method,
    Getter,
    Setter,
}

#[derive(Debug, Clone)]
pub struct ClassMethod {
    pub name: String,
    pub params: Vec<String>,
    pub body: Stmt,
    pub is_static: bool,
    pub kind: ClassMemberKind,
}

/// A class field (property initializer): `class C { x = 5; static y = 2; z; }`.
/// `init` is `None` for a bare field (`z;`), which initializes to `undefined`.
#[derive(Debug, Clone)]
pub struct ClassField {
    pub name: String,
    pub is_static: bool,
    pub init: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    Str(String),
    Template(String),
    Bool(bool),
    Null,
    Undefined,
    This,
    Ident(String),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    ObjectWithSpread(Vec<ObjectProp>),
    Unary(Token, Box<Expr>),
    Binary(Token, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Token, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Member(Box<Expr>, String),
    OptionalMember(Box<Expr>, String),
    OptionalIndex(Box<Expr>, Box<Expr>),
    OptionalCall(Box<Expr>, Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    New(Box<Expr>, Vec<Expr>),
    Arrow(Vec<String>, Box<Stmt>),
    Function(Option<String>, Vec<String>, Box<Stmt>),
    Typeof(Box<Expr>),
    Void(Box<Expr>),
    Spread(Box<Expr>),
    Sequence(Vec<Expr>),
    Await(Box<Expr>),
    Yield(Box<Expr>),
    Super,
}

#[derive(Debug, Clone)]
pub enum ObjectProp {
    KeyValue(String, Expr),
    /// Object-literal getter: `{ get x() { ... } }`
    Getter(String, Expr),
    /// Object-literal setter: `{ set x(v) { ... } }`
    Setter(String, Expr),
    /// Computed property key: `{ [expr]: value }` (key evaluated at runtime).
    Computed(Expr, Expr),
    Spread(Expr),
}
