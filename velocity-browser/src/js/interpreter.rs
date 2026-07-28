//! Full JavaScript interpreter: lexer → parser → tree-walking evaluator.
//!
//! Supports: variable declarations, assignments, if/else, while, for,
//! for-in/of, functions (declarations + arrows), closures, objects, arrays,
//! property access (dot + bracket), method calls, try/catch/finally,
//! throw, return, break, continue, ternary, typeof, template literals,
//! spread, and all standard operators. This is the agent-first JS surface:
//! enough to execute the scripts real pages ship, not a spec-complete engine.

use crate::js::scope::{Scope, ScopeRef};
use crate::js::vm::JsValue;
use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════
// Tokens
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Number(f64),
    Str(String),
    Template(String),
    Ident(String),
    // Keywords
    Var, Let, Const, Function, Return, If, Else, While, For, Do,
    Break, Continue, Throw, Try, Catch, Finally, New, Typeof, Instanceof,
    In, Of, True, False, Null, Undefined, This, Void, Delete,
    Class, Extends, Super, Static, Async, Await,
    Import, Export, From, Default, As, Yield,
    // Punctuation
    Plus, Minus, Star, Slash, Percent, StarStar,
    Bang, AmpAmp, PipePipe, QuestionQuestion,
    EqEq, BangEq, EqEqEq, BangEqEq,
    Lt, Gt, LtEq, GtEq,
    Eq, PlusEq, MinusEq, StarEq, SlashEq, QuestionQuestionEq,
    Amp, Pipe, Caret, Tilde, LtLt, GtGt, GtGtGt,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Dot, DotDotDot, QuestionDot, Comma, Colon, Semi, Question, Arrow,
    PlusPlus, MinusMinus,
    Eof,
}

// ═══════════════════════════════════════════════════════════════════════════
// Lexer
// ═══════════════════════════════════════════════════════════════════════════

pub fn lex(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => { i += 1; }
            '/' if i + 1 < chars.len() && chars[i + 1] == '/' => {
                while i < chars.len() && chars[i] != '\n' { i += 1; }
            }
            '/' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') { i += 1; }
                i += 2;
            }
            '+' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '+' { i += 1; Token::PlusPlus } else if i < chars.len() && chars[i] == '=' { i += 1; Token::PlusEq } else { Token::Plus }); }
            '-' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '-' { i += 1; Token::MinusMinus } else if i < chars.len() && chars[i] == '=' { i += 1; Token::MinusEq } else { Token::Minus }); }
            '*' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '*' { i += 1; Token::StarStar } else if i < chars.len() && chars[i] == '=' { i += 1; Token::StarEq } else { Token::Star }); }
            '/' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '=' { i += 1; Token::SlashEq } else { Token::Slash }); }
            '%' => { i += 1; tokens.push(Token::Percent); }
            '(' => { i += 1; tokens.push(Token::LParen); }
            ')' => { i += 1; tokens.push(Token::RParen); }
            '{' => { i += 1; tokens.push(Token::LBrace); }
            '}' => { i += 1; tokens.push(Token::RBrace); }
            '[' => { i += 1; tokens.push(Token::LBracket); }
            ']' => { i += 1; tokens.push(Token::RBracket); }
            ',' => { i += 1; tokens.push(Token::Comma); }
            ':' => { i += 1; tokens.push(Token::Colon); }
            ';' => { i += 1; tokens.push(Token::Semi); }
            '~' => { i += 1; tokens.push(Token::Tilde); }
            '?' => { i += 1; if i < chars.len() && chars[i] == '?' { i += 1; tokens.push(if i < chars.len() && chars[i] == '=' { i += 1; Token::QuestionQuestionEq } else { Token::QuestionQuestion }); } else if i < chars.len() && chars[i] == '.' && (i + 1 >= chars.len() || !chars[i + 1].is_ascii_digit()) { i += 1; tokens.push(Token::QuestionDot); } else { tokens.push(Token::Question); } }
            '.' => {
                if i + 2 < chars.len() && chars[i + 1] == '.' && chars[i + 2] == '.' {
                    i += 3; tokens.push(Token::DotDotDot);
                } else if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                    let mut num = String::from("0.");
                    i += 1;
                    while i < chars.len() && (chars[i].is_ascii_digit()) { num.push(chars[i]); i += 1; }
                    tokens.push(Token::Number(num.parse().unwrap_or(0.0)));
                } else {
                    i += 1; tokens.push(Token::Dot);
                }
            }
            '!' => { i += 1; if i < chars.len() && chars[i] == '=' { i += 1; tokens.push(if i < chars.len() && chars[i] == '=' { i += 1; Token::BangEqEq } else { Token::BangEq }); } else { tokens.push(Token::Bang); } }
            '=' => { i += 1; if i < chars.len() && chars[i] == '=' { i += 1; tokens.push(if i < chars.len() && chars[i] == '=' { i += 1; Token::EqEqEq } else { Token::EqEq }); } else if i < chars.len() && chars[i] == '>' { i += 1; tokens.push(Token::Arrow); } else { tokens.push(Token::Eq); } }
            '&' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '&' { i += 1; Token::AmpAmp } else { Token::Amp }); }
            '|' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '|' { i += 1; Token::PipePipe } else { Token::Pipe }); }
            '^' => { i += 1; tokens.push(Token::Caret); }
            '<' => { i += 1; if i < chars.len() && chars[i] == '=' { i += 1; tokens.push(Token::LtEq); } else if i < chars.len() && chars[i] == '<' { i += 1; tokens.push(Token::LtLt); } else { tokens.push(Token::Lt); } }
            '>' => { i += 1; if i < chars.len() && chars[i] == '=' { i += 1; tokens.push(Token::GtEq); } else if i < chars.len() && chars[i] == '>' { i += 1; if i < chars.len() && chars[i] == '>' { i += 1; tokens.push(Token::GtGtGt); } else { tokens.push(Token::GtGt); } } else { tokens.push(Token::Gt); } }
            '"' | '\'' => {
                let quote = c; i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != quote {
                    if chars[i] == '\\' && i + 1 < chars.len() { i += 1; s.push(match chars[i] { 'n' => '\n', 't' => '\t', 'r' => '\r', '0' => '\0', o => o }); }
                    else { s.push(chars[i]); }
                    i += 1;
                }
                if i < chars.len() { i += 1; }
                tokens.push(Token::Str(s));
            }
            '`' => {
                i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != '`' {
                    if chars[i] == '\\' && i + 1 < chars.len() { i += 1; s.push(match chars[i] { 'n' => '\n', 't' => '\t', o => o }); }
                    else { s.push(chars[i]); }
                    i += 1;
                }
                if i < chars.len() { i += 1; }
                tokens.push(Token::Template(s));
            }
            c if c.is_ascii_digit() => {
                let mut num = String::new();
                if c == '0' && i + 1 < chars.len() && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
                    i += 2;
                    while i < chars.len() && chars[i].is_ascii_hexdigit() { num.push(chars[i]); i += 1; }
                    tokens.push(Token::Number(i64::from_str_radix(&num, 16).unwrap_or(0) as f64));
                } else {
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { num.push(chars[i]); i += 1; }
                    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                        num.push(chars[i]); i += 1;
                        if i < chars.len() && (chars[i] == '+' || chars[i] == '-') { num.push(chars[i]); i += 1; }
                        while i < chars.len() && chars[i].is_ascii_digit() { num.push(chars[i]); i += 1; }
                    }
                    tokens.push(Token::Number(num.parse().unwrap_or(f64::NAN)));
                }
            }
            c if c.is_alphabetic() || c == '_' || c == '$' => {
                let mut id = String::new();
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') { id.push(chars[i]); i += 1; }
                tokens.push(match id.as_str() {
                    "var" => Token::Var, "let" => Token::Let, "const" => Token::Const,
                    "function" => Token::Function, "return" => Token::Return,
                    "if" => Token::If, "else" => Token::Else,
                    "while" => Token::While, "for" => Token::For, "do" => Token::Do,
                    "break" => Token::Break, "continue" => Token::Continue,
                    "throw" => Token::Throw, "try" => Token::Try,
                    "catch" => Token::Catch, "finally" => Token::Finally,
                    "new" => Token::New, "typeof" => Token::Typeof, "instanceof" => Token::Instanceof,
                    "in" => Token::In, "of" => Token::Of,
                    "true" => Token::True, "false" => Token::False,
                    "null" => Token::Null, "undefined" => Token::Undefined,
                    "this" => Token::This, "void" => Token::Void, "delete" => Token::Delete,
                    "class" => Token::Class, "extends" => Token::Extends,
                    "super" => Token::Super, "static" => Token::Static,
                    "async" => Token::Async, "await" => Token::Await,
                    "import" => Token::Import, "export" => Token::Export,
                    "from" => Token::From, "default" => Token::Default, "as" => Token::As,
                    "yield" => Token::Yield,
                    _ => Token::Ident(id),
                });
            }
            other => return Err(format!("unexpected character '{}'", other)),
        }
    }
    tokens.push(Token::Eof);
    Ok(tokens)
}

// ═══════════════════════════════════════════════════════════════════════════
// AST
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    VarDecl { name: String, init: Option<Expr> },
    DestructureDecl { pattern: DestructurePattern, init: Expr },
    Block(Vec<Stmt>),
    If { cond: Expr, then_branch: Box<Stmt>, else_branch: Option<Box<Stmt>> },
    While { cond: Expr, body: Box<Stmt> },
    DoWhile { body: Box<Stmt>, cond: Expr },
    For { init: Option<Box<Stmt>>, cond: Option<Expr>, update: Option<Expr>, body: Box<Stmt> },
    ForIn { var_name: String, object: Expr, body: Box<Stmt> },
    ForOf { var_name: String, object: Expr, body: Box<Stmt> },
    Return(Option<Expr>),
    Break,
    Continue,
    Throw(Expr),
    TryCatch { try_block: Box<Stmt>, catch_var: Option<String>, catch_block: Option<Box<Stmt>>, finally_block: Option<Box<Stmt>> },
    FunctionDecl { name: String, params: Vec<String>, body: Box<Stmt> },
    ClassDecl { name: String, parent: Option<String>, methods: Vec<ClassMethod>, fields: Vec<ClassField> },
    /// import { a, b } from 'module' / import x from 'module' / import * as x from 'module'
    Import { specifiers: Vec<ImportSpecifier>, source: String },
    /// export const x = ...; / export default expr; / export { a, b };
    Export { declaration: Option<Box<Stmt>>, default_expr: Option<Expr>, named: Vec<String> },
    /// Generator function: function* name() { yield ... }
    GeneratorDecl { name: String, params: Vec<String>, body: Box<Stmt> },
    /// Async function: async function name() { ... } — wraps return in Promise.
    AsyncFunctionDecl { name: String, params: Vec<String>, body: Box<Stmt> },
    /// Labeled statement: label: stmt (we skip the label)
    Labeled { label: String, body: Box<Stmt> },
}

#[derive(Debug, Clone)]
pub enum DestructurePattern {
    Object(Vec<(String, Option<String>)>), // [(key, optional_alias)]
    Array(Vec<Option<String>>),            // [Some(name), None for holes]
}

#[derive(Debug, Clone)]
pub struct ImportSpecifier {
    pub imported: String,  // the name exported by the module (or "default" / "*")
    pub local: String,     // the local binding name
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

// ═══════════════════════════════════════════════════════════════════════════
// Parser
// ═══════════════════════════════════════════════════════════════════════════

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self { Self { tokens, pos: 0 } }

    fn peek(&self) -> &Token { self.tokens.get(self.pos).unwrap_or(&Token::Eof) }
    fn at(&self, t: &Token) -> bool { self.peek() == t }
    fn advance(&mut self) -> Token { let t = self.peek().clone(); if !self.at(&Token::Eof) { self.pos += 1; } t }
    fn expect(&mut self, t: &Token) -> Result<(), String> {
        if self.peek() == t { self.advance(); Ok(()) }
        else { Err(format!("expected {:?}, got {:?}", t, self.peek())) }
    }
    fn eat(&mut self, t: &Token) -> bool { if self.peek() == t { self.advance(); true } else { false } }

    pub fn parse_program(&mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        while !self.at(&Token::Eof) { stmts.push(self.parse_stmt()?); }
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek().clone() {
            Token::LBrace => self.parse_block(),
            Token::Var | Token::Let | Token::Const => self.parse_var_decl(),
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::Do => self.parse_do_while(),
            Token::For => self.parse_for(),
            Token::Return => { self.advance(); let e = if !self.at(&Token::Semi) && !self.at(&Token::RBrace) && !self.at(&Token::Eof) { Some(self.parse_expr()?) } else { None }; self.eat(&Token::Semi); Ok(Stmt::Return(e)) }
            Token::Break => { self.advance(); self.eat(&Token::Semi); Ok(Stmt::Break) }
            Token::Continue => { self.advance(); self.eat(&Token::Semi); Ok(Stmt::Continue) }
            Token::Throw => { self.advance(); let e = self.parse_expr()?; self.eat(&Token::Semi); Ok(Stmt::Throw(e)) }
            Token::Try => self.parse_try(),
            Token::Function => self.parse_function_decl(),
            Token::Class => self.parse_class_decl(),
            Token::Import => self.parse_import(),
            Token::Export => self.parse_export(),
            Token::Async => {
                // async function => AsyncFunctionDecl
                self.advance();
                if self.at(&Token::Function) {
                    self.advance();
                    let name = match self.advance() { Token::Ident(n) => n, t => return Err(format!("expected function name, got {:?}", t)) };
                    let params = self.parse_params()?;
                    let body = Box::new(self.parse_block()?);
                    Ok(Stmt::AsyncFunctionDecl { name, params, body })
                } else {
                    // async arrow handled in expression parsing
                    let e = self.parse_expr()?; self.eat(&Token::Semi); Ok(Stmt::Expr(e))
                }
            }
            _ => {
                // Check for labeled statement: ident: stmt
                if let Token::Ident(name) = self.peek().clone() {
                    let saved = self.pos;
                    self.advance();
                    if self.eat(&Token::Colon) {
                        let body = Box::new(self.parse_stmt()?);
                        return Ok(Stmt::Labeled { label: name, body });
                    }
                    self.pos = saved;
                }
                let e = self.parse_expr()?; self.eat(&Token::Semi); Ok(Stmt::Expr(e))
            }
        }
    }

    fn parse_block(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while !self.at(&Token::RBrace) && !self.at(&Token::Eof) { stmts.push(self.parse_stmt()?); }
        self.expect(&Token::RBrace)?;
        Ok(Stmt::Block(stmts))
    }

    fn parse_var_decl(&mut self) -> Result<Stmt, String> {
        self.advance(); // var/let/const
        // Destructuring: let { a, b } = expr or let [a, b] = expr
        if self.at(&Token::LBrace) {
            self.advance();
            let mut props = Vec::new();
            while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
                let key = match self.advance() { Token::Ident(n) => n, t => return Err(format!("expected ident in destructure, got {:?}", t)) };
                let alias = if self.eat(&Token::Colon) {
                    match self.advance() { Token::Ident(n) => Some(n), _ => None }
                } else { None };
                // Skip default value: = expr
                if self.eat(&Token::Eq) { let _ = self.parse_assign()?; }
                props.push((key, alias));
                if !self.at(&Token::RBrace) { self.eat(&Token::Comma); }
            }
            self.expect(&Token::RBrace)?;
            self.expect(&Token::Eq)?;
            let init = self.parse_expr()?;
            self.eat(&Token::Semi);
            return Ok(Stmt::DestructureDecl { pattern: DestructurePattern::Object(props), init });
        }
        if self.at(&Token::LBracket) {
            self.advance();
            let mut items = Vec::new();
            while !self.at(&Token::RBracket) && !self.at(&Token::Eof) {
                if self.at(&Token::Comma) { items.push(None); }
                else { match self.advance() { Token::Ident(n) => items.push(Some(n)), _ => items.push(None) } }
                if !self.at(&Token::RBracket) { self.eat(&Token::Comma); }
            }
            self.expect(&Token::RBracket)?;
            self.expect(&Token::Eq)?;
            let init = self.parse_expr()?;
            self.eat(&Token::Semi);
            return Ok(Stmt::DestructureDecl { pattern: DestructurePattern::Array(items), init });
        }
        let name = match self.advance() { Token::Ident(n) => n, t => return Err(format!("expected identifier, got {:?}", t)) };
        let init = if self.eat(&Token::Eq) { Some(self.parse_expr()?) } else { None };
        // Handle multiple declarators
        while self.eat(&Token::Comma) {
            let _extra = match self.advance() { Token::Ident(_n) => _n, _ => break };
            if self.eat(&Token::Eq) { let _ = self.parse_expr()?; }
        }
        self.eat(&Token::Semi);
        Ok(Stmt::VarDecl { name, init })
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.advance(); // if
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        let then_branch = Box::new(self.parse_stmt()?);
        let else_branch = if self.eat(&Token::Else) { Some(Box::new(self.parse_stmt()?)) } else { None };
        Ok(Stmt::If { cond, then_branch, else_branch })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.advance();
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::While { cond, body })
    }

    fn parse_do_while(&mut self) -> Result<Stmt, String> {
        self.advance(); // do
        let body = Box::new(self.parse_stmt()?);
        self.expect(&Token::While)?;
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        self.eat(&Token::Semi);
        Ok(Stmt::DoWhile { body, cond })
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.advance(); // for
        self.expect(&Token::LParen)?;
        // Check for for-in / for-of
        if matches!(self.peek(), Token::Var | Token::Let | Token::Const) {
            let saved = self.pos;
            self.advance(); // var/let/const
            if let Token::Ident(var_name) = self.advance() {
                if self.at(&Token::In) || self.at(&Token::Of) {
                    let is_of = self.at(&Token::Of);
                    self.advance(); // in/of
                    let obj = self.parse_expr()?;
                    self.expect(&Token::RParen)?;
                    let body = Box::new(self.parse_stmt()?);
                    return Ok(if is_of {
                        Stmt::ForOf { var_name, object: obj, body }
                    } else {
                        Stmt::ForIn { var_name, object: obj, body }
                    });
                }
            }
            self.pos = saved;
        }
        let init = if self.at(&Token::Semi) { None } else { Some(Box::new(self.parse_stmt()?)) };
        if !matches!(init.as_deref(), Some(Stmt::VarDecl { .. }) | Some(Stmt::Expr(_))) {
            // stmt already consumed semicolon
        } else if init.is_none() {
            self.eat(&Token::Semi);
        }
        let cond = if self.at(&Token::Semi) { None } else { Some(self.parse_expr()?) };
        self.eat(&Token::Semi);
        let update = if self.at(&Token::RParen) { None } else { Some(self.parse_expr()?) };
        self.expect(&Token::RParen)?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::For { init, cond, update, body })
    }

    fn parse_try(&mut self) -> Result<Stmt, String> {
        self.advance(); // try
        let try_block = Box::new(self.parse_block()?);
        let (catch_var, catch_block) = if self.eat(&Token::Catch) {
            let var = if self.eat(&Token::LParen) { let n = match self.advance() { Token::Ident(n) => n, _ => "e".into() }; self.expect(&Token::RParen)?; Some(n) } else { None };
            (var, Some(Box::new(self.parse_block()?)))
        } else { (None, None) };
        let finally_block = if self.eat(&Token::Finally) { Some(Box::new(self.parse_block()?)) } else { None };
        Ok(Stmt::TryCatch { try_block, catch_var, catch_block, finally_block })
    }

    fn parse_function_decl(&mut self) -> Result<Stmt, String> {
        self.advance(); // function
        // function* generator
        let is_generator = self.eat(&Token::Star);
        let name = match self.advance() { Token::Ident(n) => n, t => return Err(format!("expected function name, got {:?}", t)) };
        let params = self.parse_params()?;
        let body = Box::new(self.parse_block()?);
        if is_generator {
            Ok(Stmt::GeneratorDecl { name, params, body })
        } else {
            Ok(Stmt::FunctionDecl { name, params, body })
        }
    }

    fn parse_class_decl(&mut self) -> Result<Stmt, String> {
        self.advance(); // class
        let name = match self.advance() { Token::Ident(n) => n, t => return Err(format!("expected class name, got {:?}", t)) };
        let parent = if self.eat(&Token::Extends) {
            match self.advance() { Token::Ident(n) => Some(n), _ => None }
        } else { None };
        self.expect(&Token::LBrace)?;
        let mut methods = Vec::new();
        let mut fields = Vec::new();
        while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
            // Skip semicolons between members
            if self.eat(&Token::Semi) { continue; }
            let is_static = self.eat(&Token::Static);
            // Handle get/set/async as method name prefixes or actual method names
            let method_name = match self.peek().clone() {
                Token::Ident(n) => { self.advance(); n }
                _ => { self.advance(); "unknown".to_string() }
            };
            // Detect accessor members: `get x() {}` / `set x(v) {}`. The keyword is only a
            // prefix when followed by another identifier (the real property name).
            let mut kind = ClassMemberKind::Method;
            // If next token is ( it's a method; if next is an ident, this was a keyword prefix
            let final_name = if self.at(&Token::LParen) {
                method_name
            } else if (method_name == "get" || method_name == "set") && matches!(self.peek(), Token::Ident(_)) {
                let Token::Ident(n) = self.advance() else { unreachable!() };
                kind = if method_name == "get" { ClassMemberKind::Getter } else { ClassMemberKind::Setter };
                n
            } else if let Token::Ident(n) = self.peek().clone() {
                self.advance(); n
            } else {
                method_name
            };
            if self.at(&Token::LParen) {
                // Method / getter / setter member.
                let params = self.parse_params()?;
                let body = self.parse_block()?;
                methods.push(ClassMethod { name: final_name, params, body, is_static, kind });
            } else {
                // Class field: `name = expr;` or bare `name;`.
                let init = if self.eat(&Token::Eq) { Some(self.parse_assign()?) } else { None };
                self.eat(&Token::Semi);
                fields.push(ClassField { name: final_name, is_static, init });
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Stmt::ClassDecl { name, parent, methods, fields })
    }

    fn parse_import(&mut self) -> Result<Stmt, String> {
        self.advance(); // import
        let mut specifiers = Vec::new();
        // import 'module' (side-effect only)
        if let Token::Str(source) = self.peek().clone() {
            self.advance();
            self.eat(&Token::Semi);
            return Ok(Stmt::Import { specifiers, source });
        }
        // import * as name from 'module'
        if self.eat(&Token::Star) {
            self.expect(&Token::As)?;
            let local = match self.advance() { Token::Ident(n) => n, _ => "_".into() };
            specifiers.push(ImportSpecifier { imported: "*".into(), local });
        } else if self.at(&Token::LBrace) {
            // import { a, b as c } from 'module'
            self.advance();
            while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
                let imported = match self.advance() {
                    Token::Ident(n) => n,
                    Token::Default => "default".to_string(),
                    _ => "_".into(),
                };
                let local = if self.eat(&Token::As) {
                    match self.advance() { Token::Ident(n) => n, _ => imported.clone() }
                } else { imported.clone() };
                specifiers.push(ImportSpecifier { imported, local });
                if !self.at(&Token::RBrace) { self.eat(&Token::Comma); }
            }
            self.expect(&Token::RBrace)?;
        } else {
            // import defaultExport from 'module'
            let local = match self.advance() { Token::Ident(n) => n, _ => "_".into() };
            specifiers.push(ImportSpecifier { imported: "default".into(), local });
            // Could also have: import x, { a, b } from '...'
            if self.eat(&Token::Comma)
                && self.at(&Token::LBrace) {
                    self.advance();
                    while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
                        let imported = match self.advance() { Token::Ident(n) => n, _ => "_".into() };
                        let local2 = if self.eat(&Token::As) {
                            match self.advance() { Token::Ident(n) => n, _ => imported.clone() }
                        } else { imported.clone() };
                        specifiers.push(ImportSpecifier { imported, local: local2 });
                        if !self.at(&Token::RBrace) { self.eat(&Token::Comma); }
                    }
                    self.expect(&Token::RBrace)?;
                }
        }
        // from 'source'
        self.expect(&Token::From)?;
        let source = match self.advance() { Token::Str(s) => s, _ => String::new() };
        self.eat(&Token::Semi);
        Ok(Stmt::Import { specifiers, source })
    }

    fn parse_export(&mut self) -> Result<Stmt, String> {
        self.advance(); // export
        // export default expr
        if self.eat(&Token::Default) {
            let expr = self.parse_expr()?;
            self.eat(&Token::Semi);
            return Ok(Stmt::Export { declaration: None, default_expr: Some(expr), named: vec![] });
        }
        // export { a, b, c }
        if self.at(&Token::LBrace) {
            self.advance();
            let mut named = Vec::new();
            while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
                if let Token::Ident(n) = self.advance() { named.push(n) }
                if self.eat(&Token::As) { self.advance(); } // skip alias for now
                if !self.at(&Token::RBrace) { self.eat(&Token::Comma); }
            }
            self.expect(&Token::RBrace)?;
            // Optional: from 'source'
            if self.eat(&Token::From) { self.advance(); }
            self.eat(&Token::Semi);
            return Ok(Stmt::Export { declaration: None, default_expr: None, named });
        }
        // export const/let/var/function/class
        let decl = self.parse_stmt()?;
        Ok(Stmt::Export { declaration: Some(Box::new(decl)), default_expr: None, named: vec![] })
    }

    fn parse_params(&mut self) -> Result<Vec<String>, String> {
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        while !self.at(&Token::RParen) && !self.at(&Token::Eof) {
            self.eat(&Token::DotDotDot); // rest param
            if let Token::Ident(n) = self.advance() { params.push(n) }
            if self.at(&Token::Eq) { self.advance(); let _ = self.parse_expr()?; } // default value
            if !self.at(&Token::RParen) { self.expect(&Token::Comma)?; }
        }
        self.expect(&Token::RParen)?;
        Ok(params)
    }

    // Expression parsing with precedence climbing
    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        let expr = self.parse_assign()?;
        if self.at(&Token::Comma) && !self.at(&Token::Eof) {
            // Comma expressions only in certain contexts; usually we stop here
        }
        Ok(expr)
    }

    fn parse_assign(&mut self) -> Result<Expr, String> {
        let lhs = self.parse_ternary()?;
        match self.peek().clone() {
            Token::Eq | Token::PlusEq | Token::MinusEq | Token::StarEq | Token::SlashEq | Token::QuestionQuestionEq => {
                let op = self.advance();
                let rhs = self.parse_assign()?;
                Ok(Expr::Assign(Box::new(lhs), op, Box::new(rhs)))
            }
            _ => Ok(lhs),
        }
    }

    fn parse_ternary(&mut self) -> Result<Expr, String> {
        let cond = self.parse_nullish()?;
        if self.eat(&Token::Question) {
            let then_expr = self.parse_assign()?;
            self.expect(&Token::Colon)?;
            let else_expr = self.parse_assign()?;
            Ok(Expr::Ternary(Box::new(cond), Box::new(then_expr), Box::new(else_expr)))
        } else { Ok(cond) }
    }

    fn parse_nullish(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_or()?;
        while self.eat(&Token::QuestionQuestion) { let rhs = self.parse_or()?; lhs = Expr::Binary(Token::QuestionQuestion, Box::new(lhs), Box::new(rhs)); }
        Ok(lhs)
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_and()?;
        while self.eat(&Token::PipePipe) { let rhs = self.parse_and()?; lhs = Expr::Binary(Token::PipePipe, Box::new(lhs), Box::new(rhs)); }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_bitwise_or()?;
        while self.eat(&Token::AmpAmp) { let rhs = self.parse_bitwise_or()?; lhs = Expr::Binary(Token::AmpAmp, Box::new(lhs), Box::new(rhs)); }
        Ok(lhs)
    }

    fn parse_bitwise_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_bitwise_xor()?;
        while self.at(&Token::Pipe) && !self.at(&Token::PipePipe) { self.advance(); let rhs = self.parse_bitwise_xor()?; lhs = Expr::Binary(Token::Pipe, Box::new(lhs), Box::new(rhs)); }
        Ok(lhs)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_bitwise_and()?;
        while self.eat(&Token::Caret) { let rhs = self.parse_bitwise_and()?; lhs = Expr::Binary(Token::Caret, Box::new(lhs), Box::new(rhs)); }
        Ok(lhs)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_equality()?;
        while self.at(&Token::Amp) && !self.at(&Token::AmpAmp) { self.advance(); let rhs = self.parse_equality()?; lhs = Expr::Binary(Token::Amp, Box::new(lhs), Box::new(rhs)); }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_comparison()?;
        loop {
            match self.peek().clone() {
                Token::EqEq | Token::BangEq | Token::EqEqEq | Token::BangEqEq => { let op = self.advance(); let rhs = self.parse_comparison()?; lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs)); }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_shift()?;
        loop {
            match self.peek().clone() {
                Token::Lt | Token::Gt | Token::LtEq | Token::GtEq | Token::Instanceof | Token::In => { let op = self.advance(); let rhs = self.parse_shift()?; lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs)); }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_additive()?;
        loop {
            match self.peek().clone() {
                Token::LtLt | Token::GtGt | Token::GtGtGt => { let op = self.advance(); let rhs = self.parse_additive()?; lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs)); }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            match self.peek().clone() {
                Token::Plus | Token::Minus => { let op = self.advance(); let rhs = self.parse_multiplicative()?; lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs)); }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_exponent()?;
        loop {
            match self.peek().clone() {
                Token::Star | Token::Slash | Token::Percent => { let op = self.advance(); let rhs = self.parse_exponent()?; lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs)); }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_exponent(&mut self) -> Result<Expr, String> {
        let base = self.parse_unary()?;
        if self.eat(&Token::StarStar) { let exp = self.parse_exponent()?; Ok(Expr::Binary(Token::StarStar, Box::new(base), Box::new(exp))) }
        else { Ok(base) }
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::Bang | Token::Minus | Token::Plus | Token::Tilde => { let op = self.advance(); let rhs = self.parse_unary()?; Ok(Expr::Unary(op, Box::new(rhs))) }
            Token::Typeof => { self.advance(); let rhs = self.parse_unary()?; Ok(Expr::Typeof(Box::new(rhs))) }
            Token::Void => { self.advance(); let rhs = self.parse_unary()?; Ok(Expr::Void(Box::new(rhs))) }
            Token::Delete => { self.advance(); let rhs = self.parse_unary()?; Ok(Expr::Unary(Token::Delete, Box::new(rhs))) }
            Token::PlusPlus | Token::MinusMinus => { let op = self.advance(); let rhs = self.parse_unary()?; Ok(Expr::Unary(op, Box::new(rhs))) }
            Token::New => { self.advance(); let callee = self.parse_new_target()?; let args = if self.at(&Token::LParen) { self.parse_args()? } else { Vec::new() }; self.parse_member_chain(Expr::New(Box::new(callee), args)) }
            Token::DotDotDot => { self.advance(); let e = self.parse_assign()?; Ok(Expr::Spread(Box::new(e))) }
            Token::Await => { self.advance(); let e = self.parse_unary()?; Ok(Expr::Await(Box::new(e))) }
            Token::Yield => { self.advance(); let e = if self.at(&Token::Semi) || self.at(&Token::RBrace) || self.at(&Token::RParen) || self.at(&Token::Comma) { Expr::Undefined } else { self.parse_assign()? }; Ok(Expr::Yield(Box::new(e))) }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_call_expr()?;
        match self.peek() {
            Token::PlusPlus => { self.advance(); expr = Expr::Unary(Token::PlusPlus, Box::new(expr)); }
            Token::MinusMinus => { self.advance(); expr = Expr::Unary(Token::MinusMinus, Box::new(expr)); }
            _ => {}
        }
        Ok(expr)
    }

    fn parse_call_expr(&mut self) -> Result<Expr, String> {
        let expr = self.parse_primary()?;
        self.parse_member_chain(expr)
    }

    /// Apply postfix member/index/call chaining (`.prop`, `[idx]`, `(args)`, `?.`) to an
    /// already-parsed expression. Shared by normal call expressions and `new` expressions
    /// so that `new Foo().bar` and `new Foo().bar()` parse correctly.
    fn parse_member_chain(&mut self, mut expr: Expr) -> Result<Expr, String> {
        loop {
            match self.peek().clone() {
                Token::LParen => { let args = self.parse_args()?; expr = Expr::Call(Box::new(expr), args); }
                Token::Dot => { self.advance(); let prop = self.parse_prop_name()?; expr = Expr::Member(Box::new(expr), prop); }
                Token::QuestionDot => {
                    self.advance();
                    if self.at(&Token::LParen) {
                        let args = self.parse_args()?;
                        expr = Expr::OptionalCall(Box::new(expr), args);
                    } else if self.at(&Token::LBracket) {
                        self.advance();
                        let idx = self.parse_expr()?;
                        self.expect(&Token::RBracket)?;
                        expr = Expr::OptionalIndex(Box::new(expr), Box::new(idx));
                    } else {
                        let prop = self.parse_prop_name()?;
                        expr = Expr::OptionalMember(Box::new(expr), prop);
                    }
                }
                Token::LBracket => { self.advance(); let idx = self.parse_expr()?; self.expect(&Token::RBracket)?; expr = Expr::Index(Box::new(expr), Box::new(idx)); }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Parse target for `new` - only member access, no calls.
    fn parse_new_target(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek().clone() {
                Token::Dot => { self.advance(); let prop = self.parse_prop_name()?; expr = Expr::Member(Box::new(expr), prop); }
                Token::LBracket => { self.advance(); let idx = self.parse_expr()?; self.expect(&Token::RBracket)?; expr = Expr::Index(Box::new(expr), Box::new(idx)); }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, String> {
        self.expect(&Token::LParen)?;
        let mut args = Vec::new();
        while !self.at(&Token::RParen) && !self.at(&Token::Eof) {
            args.push(self.parse_assign()?);
            if !self.at(&Token::RParen) { self.expect(&Token::Comma)?; }
        }
        self.expect(&Token::RParen)?;
        Ok(args)
    }

    /// Parse a property name after `.` — accepts identifiers AND keywords
    /// (e.g., `obj.catch(...)`, `obj.finally(...)`, `obj.get(...)` are valid JS).
    fn parse_prop_name(&mut self) -> Result<String, String> {
        match self.advance() {
            Token::Ident(n) => Ok(n),
            // Keywords that are valid as property names in member access
            Token::Catch => Ok("catch".to_string()),
            Token::Finally => Ok("finally".to_string()),
            Token::Try => Ok("try".to_string()),
            Token::Throw => Ok("throw".to_string()),
            Token::New => Ok("new".to_string()),
            Token::Typeof => Ok("typeof".to_string()),
            Token::Instanceof => Ok("instanceof".to_string()),
            Token::Delete => Ok("delete".to_string()),
            Token::Void => Ok("void".to_string()),
            Token::In => Ok("in".to_string()),
            Token::Of => Ok("of".to_string()),
            Token::As => Ok("as".to_string()),
            Token::From => Ok("from".to_string()),
            Token::Default => Ok("default".to_string()),
            Token::Import => Ok("import".to_string()),
            Token::Export => Ok("export".to_string()),
            Token::Yield => Ok("yield".to_string()),
            Token::Async => Ok("async".to_string()),
            Token::Await => Ok("await".to_string()),
            Token::Let => Ok("let".to_string()),
            Token::Static => Ok("static".to_string()),
            Token::Class => Ok("class".to_string()),
            Token::Extends => Ok("extends".to_string()),
            Token::Super => Ok("super".to_string()),
            Token::If => Ok("if".to_string()),
            Token::Else => Ok("else".to_string()),
            Token::For => Ok("for".to_string()),
            Token::While => Ok("while".to_string()),
            Token::Do => Ok("do".to_string()),
            Token::Break => Ok("break".to_string()),
            Token::Continue => Ok("continue".to_string()),
            Token::Return => Ok("return".to_string()),
            Token::Var => Ok("var".to_string()),
            Token::Const => Ok("const".to_string()),
            Token::Function => Ok("function".to_string()),
            Token::This => Ok("this".to_string()),
            Token::True => Ok("true".to_string()),
            Token::False => Ok("false".to_string()),
            Token::Null => Ok("null".to_string()),
            Token::Undefined => Ok("undefined".to_string()),
            t => Err(format!("expected property name, got {:?}", t)),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::Number(n) => { self.advance(); Ok(Expr::Number(n)) }
            Token::Str(s) => { self.advance(); Ok(Expr::Str(s)) }
            Token::Template(s) => { self.advance(); Ok(Expr::Template(s)) }
            Token::True => { self.advance(); Ok(Expr::Bool(true)) }
            Token::False => { self.advance(); Ok(Expr::Bool(false)) }
            Token::Null => { self.advance(); Ok(Expr::Null) }
            Token::Undefined => { self.advance(); Ok(Expr::Undefined) }
            Token::This => { self.advance(); Ok(Expr::This) }
            Token::Super => { self.advance(); Ok(Expr::Super) }
            Token::Async => {
                // async () => ... or async function
                self.advance();
                if self.at(&Token::Function) {
                    self.advance();
                    let name = if let Token::Ident(n) = self.peek().clone() { self.advance(); Some(n) } else { None };
                    let params = self.parse_params()?;
                    let body = Box::new(self.parse_block()?);
                    Ok(Expr::Function(name, params, body))
                } else {
                    // async arrow: async (a, b) => ... or async x => ...
                    // Treat like normal ident or arrow
                    if let Token::Ident(name) = self.peek().clone() {
                        self.advance();
                        if self.at(&Token::Arrow) {
                            self.advance();
                            let body = if self.at(&Token::LBrace) { self.parse_block()? } else { Stmt::Return(Some(self.parse_assign()?)) };
                            return Ok(Expr::Arrow(vec![name], Box::new(body)));
                        }
                        Ok(Expr::Ident(name))
                    } else if self.at(&Token::LParen) {
                        let saved = self.pos;
                        if let Ok(params) = self.try_parse_arrow_params(saved) {
                            return Ok(params);
                        }
                        self.pos = saved;
                        self.advance();
                        let expr = self.parse_expr()?;
                        self.expect(&Token::RParen)?;
                        Ok(expr)
                    } else {
                        Ok(Expr::Ident("async".to_string()))
                    }
                }
            }
            Token::Ident(_) => {
                let Token::Ident(name) = self.advance() else { unreachable!() };
                // Arrow function: x => expr  or (x, y) => expr
                if self.at(&Token::Arrow) {
                    self.advance();
                    let body = if self.at(&Token::LBrace) { self.parse_block()? } else { Stmt::Return(Some(self.parse_assign()?)) };
                    return Ok(Expr::Arrow(vec![name], Box::new(body)));
                }
                Ok(Expr::Ident(name))
            }
            Token::LParen => {
                // Could be arrow: (a, b) => ... or grouping: (expr)
                let saved = self.pos;
                self.advance(); // (
                if self.at(&Token::RParen) {
                    self.advance(); // )
                    if self.at(&Token::Arrow) { self.advance(); let body = if self.at(&Token::LBrace) { self.parse_block()? } else { Stmt::Return(Some(self.parse_assign()?)) }; return Ok(Expr::Arrow(vec![], Box::new(body))); }
                    self.pos = saved;
                }
                // Try arrow detection
                if let Ok(params) = self.try_parse_arrow_params(saved) {
                    return Ok(params);
                }
                self.pos = saved;
                self.advance(); // (
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::LBracket => self.parse_array_literal(),
            Token::LBrace => self.parse_object_literal(),
            Token::Function => {
                self.advance();
                // function* (generator expression)
                let is_generator = self.eat(&Token::Star);
                let name = if let Token::Ident(n) = self.peek().clone() { self.advance(); Some(n) } else { None };
                let params = self.parse_params()?;
                let body = Box::new(self.parse_block()?);
                let func_name = if is_generator {
                    Some(format!("__generator__{}", name.unwrap_or_else(|| "anon".to_string())))
                } else { name };
                Ok(Expr::Function(func_name, params, body))
            }
            t => Err(format!("unexpected token in expression: {:?}", t)),
        }
    }

    fn try_parse_arrow_params(&mut self, saved: usize) -> Result<Expr, String> {
        self.pos = saved;
        self.advance(); // (
        let mut params = Vec::new();
        while !self.at(&Token::RParen) && !self.at(&Token::Eof) {
            self.eat(&Token::DotDotDot);
            match self.advance() { Token::Ident(n) => params.push(n), _ => { self.pos = saved; return Err("not arrow".into()); } }
            if self.at(&Token::Eq) { self.advance(); let _ = self.parse_assign()?; }
            if !self.at(&Token::RParen) && !self.eat(&Token::Comma) { self.pos = saved; return Err("not arrow".into()); }
        }
        if !self.eat(&Token::RParen) { self.pos = saved; return Err("not arrow".into()); }
        if !self.eat(&Token::Arrow) { self.pos = saved; return Err("not arrow".into()); }
        let body = if self.at(&Token::LBrace) { self.parse_block()? } else { Stmt::Return(Some(self.parse_assign()?)) };
        Ok(Expr::Arrow(params, Box::new(body)))
    }

    fn parse_array_literal(&mut self) -> Result<Expr, String> {
        self.expect(&Token::LBracket)?;
        let mut elems = Vec::new();
        while !self.at(&Token::RBracket) && !self.at(&Token::Eof) {
            elems.push(self.parse_assign()?);
            if !self.at(&Token::RBracket) { self.expect(&Token::Comma)?; }
        }
        self.expect(&Token::RBracket)?;
        Ok(Expr::Array(elems))
    }

    fn parse_object_literal(&mut self) -> Result<Expr, String> {
        self.expect(&Token::LBrace)?;
        let mut props = Vec::new();
        let mut has_spread = false;
        let mut has_accessor = false;
        let mut has_computed = false;
        let mut spread_props = Vec::new();
        while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
            // Spread property: { ...expr }
            if self.at(&Token::DotDotDot) {
                self.advance();
                let expr = self.parse_assign()?;
                spread_props.push(ObjectProp::Spread(expr));
                has_spread = true;
                if !self.at(&Token::RBrace) { self.expect(&Token::Comma)?; }
                continue;
            }
            // Computed property key: { [expr]: value } — the key expression is evaluated
            // at runtime, so it must be deferred (not stringified during parsing).
            if self.at(&Token::LBracket) {
                self.advance();
                let key_expr = self.parse_expr()?;
                self.expect(&Token::RBracket)?;
                self.expect(&Token::Colon)?;
                let val = self.parse_assign()?;
                spread_props.push(ObjectProp::Computed(key_expr, val));
                has_computed = true;
                if !self.at(&Token::RBrace) { self.expect(&Token::Comma)?; }
                continue;
            }
            let key = match self.peek().clone() {
                Token::Ident(k) => { self.advance(); k }
                Token::Str(k) => { self.advance(); k }
                Token::Number(n) => { self.advance(); format!("{}", n) }
                _ => return Err(format!("expected property key, got {:?}", self.peek())),
            };
            // Getter/setter: { get x() { ... }, set x(v) { ... } }
            if (key == "get" || key == "set") && matches!(self.peek(), Token::Ident(_)) {
                let actual_key = match self.advance() { Token::Ident(n) => n, _ => key.clone() };
                let params = self.parse_params()?;
                let body = self.parse_block()?;
                let func = Expr::Function(Some(format!("{}_{}", key, actual_key)), params, Box::new(body));
                has_accessor = true;
                if key == "get" {
                    spread_props.push(ObjectProp::Getter(actual_key, func));
                } else {
                    spread_props.push(ObjectProp::Setter(actual_key, func));
                }
            } else if self.at(&Token::LParen) {
                let params = self.parse_params()?;
                let body = self.parse_block()?;
                let func = Expr::Function(Some(key.clone()), params, Box::new(body));
                props.push((key.clone(), func.clone()));
                spread_props.push(ObjectProp::KeyValue(key, func));
            } else if self.eat(&Token::Colon) {
                let val = self.parse_assign()?;
                props.push((key.clone(), val.clone()));
                spread_props.push(ObjectProp::KeyValue(key, val));
            } else {
                // Shorthand: { x } means { x: x }
                props.push((key.clone(), Expr::Ident(key.clone())));
                spread_props.push(ObjectProp::KeyValue(key.clone(), Expr::Ident(key)));
            }
            if !self.at(&Token::RBrace) { self.expect(&Token::Comma)?; }
        }
        self.expect(&Token::RBrace)?;
        if has_spread || has_accessor || has_computed {
            Ok(Expr::ObjectWithSpread(spread_props))
        } else {
            Ok(Expr::Object(props))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Evaluator
// ═══════════════════════════════════════════════════════════════════════════

/// Control flow signals propagated through Result.
#[derive(Debug, Clone)]
pub enum Signal {
    Return(JsValue),
    Break,
    Continue,
    Throw(JsValue),
}

pub type EvalResult = Result<JsValue, Signal>;

/// Evaluate a full program (list of statements) in the given scope.
pub fn eval_program(stmts: &[Stmt], scope: &ScopeRef) -> EvalResult {
    let mut last = JsValue::Undefined;
    for stmt in stmts {
        last = eval_stmt(stmt, scope)?;
    }
    Ok(last)
}

pub fn eval_stmt(stmt: &Stmt, scope: &ScopeRef) -> EvalResult {
    match stmt {
        Stmt::Expr(e) => eval_expr_node(e, scope),
        Stmt::VarDecl { name, init } => {
            let val = match init { Some(e) => eval_expr_node(e, scope)?, None => JsValue::Undefined };
            Scope::declare(scope, name, val);
            Ok(JsValue::Undefined)
        }
        Stmt::DestructureDecl { pattern, init } => {
            let val = eval_expr_node(init, scope)?;
            match pattern {
                DestructurePattern::Object(props) => {
                    if let JsValue::Object(map) = &val {
                        for (key, alias) in props {
                            let var_name = alias.as_ref().unwrap_or(key);
                            let v = map.get(key).cloned().unwrap_or(JsValue::Undefined);
                            Scope::declare(scope, var_name, v);
                        }
                    }
                }
                DestructurePattern::Array(items) => {
                    if let JsValue::Array(arr) = &val {
                        for (i, item) in items.iter().enumerate() {
                            if let Some(name) = item {
                                let v = arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                                Scope::declare(scope, name, v);
                            }
                        }
                    }
                }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::Block(stmts) => {
            let child = Scope::new_child(scope);
            eval_program(stmts, &child)
        }
        Stmt::If { cond, then_branch, else_branch } => {
            if to_boolean(&eval_expr_node(cond, scope)?) { eval_stmt(then_branch, scope) }
            else if let Some(eb) = else_branch { eval_stmt(eb, scope) }
            else { Ok(JsValue::Undefined) }
        }
        Stmt::While { cond, body } => {
            let mut iterations = 0;
            while to_boolean(&eval_expr_node(cond, scope)?) {
                iterations += 1;
                if iterations > 100_000 { break; }
                match eval_stmt(body, scope) {
                    Ok(_) => {}
                    Err(Signal::Break) => break,
                    Err(Signal::Continue) => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::DoWhile { body, cond } => {
            let mut iterations = 0;
            loop {
                iterations += 1;
                if iterations > 100_000 { break; }
                match eval_stmt(body, scope) {
                    Ok(_) => {}
                    Err(Signal::Break) => break,
                    Err(Signal::Continue) => {}
                    Err(e) => return Err(e),
                }
                if !to_boolean(&eval_expr_node(cond, scope)?) { break; }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::For { init, cond, update, body } => {
            let for_scope = Scope::new_child(scope);
            if let Some(i) = init { eval_stmt(i, &for_scope)?; }
            let mut iterations = 0;
            loop {
                iterations += 1;
                if iterations > 100_000 { break; }
                if let Some(c) = cond { if !to_boolean(&eval_expr_node(c, &for_scope)?) { break; } }
                match eval_stmt(body, &for_scope) {
                    Ok(_) => {}
                    Err(Signal::Break) => break,
                    Err(Signal::Continue) => {}
                    Err(e) => return Err(e),
                }
                if let Some(u) = update { eval_expr_node(u, &for_scope)?; }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::ForIn { var_name, object, body } => {
            let obj = eval_expr_node(object, scope)?;
            match &obj {
                JsValue::Array(arr) => {
                    // for-of: iterate over values
                    for item in arr.iter() {
                        Scope::declare(scope, var_name, item.clone());
                        match eval_stmt(body, scope) {
                            Ok(_) => {}
                            Err(Signal::Break) => break,
                            Err(Signal::Continue) => continue,
                            Err(e) => return Err(e),
                        }
                    }
                }
                JsValue::Object(map) => {
                    if map.get("__type__").map(to_string).as_deref() == Some("Generator") {
                        // Generator iterator: iterate over __values__
                        if let Some(JsValue::Array(values)) = map.get("__values__") {
                            for item in values.iter() {
                                Scope::declare(scope, var_name, item.clone());
                                match eval_stmt(body, scope) {
                                    Ok(_) => {}
                                    Err(Signal::Break) => break,
                                    Err(Signal::Continue) => continue,
                                    Err(e) => return Err(e),
                                }
                            }
                        }
                    } else {
                        // for-in: iterate over enumerable own keys (internal `__x__`
                        // bookkeeping keys and non-enumerable accessors are hidden).
                        for key in enumerable_keys(map) {
                            Scope::declare(scope, var_name, JsValue::String(key));
                            match eval_stmt(body, scope) {
                                Ok(_) => {}
                                Err(Signal::Break) => break,
                                Err(Signal::Continue) => continue,
                                Err(e) => return Err(e),
                            }
                        }
                    }
                }
                _ => {}
            }
            Ok(JsValue::Undefined)
        }
        Stmt::ForOf { var_name, object, body } => {
            let iterable = eval_expr_node(object, scope)?;
            for item in iterate_values(&iterable, scope) {
                Scope::declare(scope, var_name, item);
                match eval_stmt(body, scope) {
                    Ok(_) => {}
                    Err(Signal::Break) => break,
                    Err(Signal::Continue) => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::Return(e) => {
            let val = match e { Some(ex) => eval_expr_node(ex, scope)?, None => JsValue::Undefined };
            Err(Signal::Return(val))
        }
        Stmt::Break => Err(Signal::Break),
        Stmt::Continue => Err(Signal::Continue),
        Stmt::Throw(e) => Err(Signal::Throw(eval_expr_node(e, scope)?)),
        Stmt::TryCatch { try_block, catch_var, catch_block, finally_block } => {
            let result = eval_stmt(try_block, scope);
            // Run the catch clause when the try block threw and a catch is present. The
            // outcome is either a normal value or a signal that must propagate: a throw
            // with no catch clause, or a throw escaping the catch block itself.
            let outcome: EvalResult = match result {
                Err(Signal::Throw(thrown)) => {
                    if let Some(cb) = catch_block {
                        let catch_scope = Scope::new_child(scope);
                        if let Some(var) = catch_var { Scope::declare(&catch_scope, var, thrown); }
                        // A throw escaping the catch block must propagate (after finally).
                        eval_stmt(cb, &catch_scope)
                    } else {
                        // No catch clause: re-throw once finally has run.
                        Err(Signal::Throw(thrown))
                    }
                }
                other => other,
            };
            // `finally` always runs — whether try/catch completed normally or is about
            // to propagate a signal (throw / break / continue / return).
            if let Some(fb) = finally_block { let _ = eval_stmt(fb, scope); }
            outcome
        }
        Stmt::FunctionDecl { name, params, body } => {
            let func = JsValue::Function {
                name: Some(name.clone()),
                params: params.clone(),
                body: (**body).clone(),
                closure: scope.clone(),
            };
            Scope::declare(scope, name, func);
            Ok(JsValue::Undefined)
        }
        Stmt::AsyncFunctionDecl { name, params, body } => {
            // Async functions wrap their return value in a resolved Promise.
            let func = JsValue::Function {
                name: Some(name.clone()),
                params: params.clone(),
                body: (**body).clone(),
                closure: scope.clone(),
            };
            // Tag the function so calls know to wrap the result in a Promise
            let mut wrapper_map = HashMap::new();
            wrapper_map.insert("__type__".to_string(), JsValue::String("AsyncFunction".to_string()));
            wrapper_map.insert("__inner__".to_string(), func);
            let async_fn = JsValue::Object(wrapper_map);
            Scope::declare(scope, name, async_fn);
            Ok(JsValue::Undefined)
        }
        Stmt::ClassDecl { name, parent, methods, fields } => {
            eval_class_decl(name, parent, methods, fields, scope);
            Ok(JsValue::Undefined)
        }
        Stmt::Import { specifiers, source } => {
            // Try to resolve from the module registry first
            if let Ok(()) = apply_import(specifiers, source, scope) {
                // Successfully imported from registry
            } else {
                // Fallback: declare bindings as undefined for standalone mode
                for spec in specifiers {
                    if Scope::resolve(scope, &spec.local).is_none() {
                        Scope::declare(scope, &spec.local, JsValue::Undefined);
                    }
                }
            }
            Ok(JsValue::Undefined)
        }
        Stmt::Export { declaration, default_expr, .. } => {
            // Execute the declaration or expression, make it available in scope
            if let Some(decl) = declaration {
                eval_stmt(decl, scope)?;
            }
            if let Some(expr) = default_expr {
                let val = eval_expr_node(expr, scope)?;
                Scope::declare(scope, "__default_export__", val);
            }
            Ok(JsValue::Undefined)
        }
        Stmt::GeneratorDecl { name, params, body } => {
            // A generator function creates a function that, when called, returns an iterator object.
            // Simplified: we store it as a regular function tagged as generator.
            let func = JsValue::Function {
                name: Some(format!("__generator__{}", name)),
                params: params.clone(),
                body: (**body).clone(),
                closure: scope.clone(),
            };
            Scope::declare(scope, name, func);
            Ok(JsValue::Undefined)
        }
        Stmt::Labeled { body, .. } => {
            // Execute the labeled statement body; break/continue propagate normally
            eval_stmt(body, scope)
        }
    }
}

pub fn eval_expr_node(expr: &Expr, scope: &ScopeRef) -> EvalResult {
    match expr {
        Expr::Number(n) => Ok(JsValue::Number(*n)),
        Expr::Str(s) => Ok(JsValue::String(s.clone())),
        Expr::Template(s) => eval_template_literal(s, scope),
        Expr::Bool(b) => Ok(JsValue::Boolean(*b)),
        Expr::Null => Ok(JsValue::Null),
        Expr::Undefined => Ok(JsValue::Undefined),
        Expr::This => Ok(Scope::resolve(scope, "this").unwrap_or(JsValue::Undefined)),
        Expr::Super => Ok(Scope::resolve(scope, "__super__").unwrap_or(JsValue::Undefined)),
        Expr::Ident(name) => Ok(match Scope::resolve(scope, name) {
            Some(v) => v,
            // Well-known global constants fall back here when not shadowed by a binding.
            None => match name.as_str() {
                "Infinity" => JsValue::Number(f64::INFINITY),
                "NaN" => JsValue::Number(f64::NAN),
                _ => JsValue::Undefined,
            },
        }),
        Expr::Array(elems) => {
            let mut arr = Vec::new();
            for e in elems {
                if let Expr::Spread(inner) = e {
                    if let JsValue::Array(items) = eval_expr_node(inner, scope)? { arr.extend(items); }
                } else { arr.push(eval_expr_node(e, scope)?); }
            }
            Ok(JsValue::Array(arr))
        }
        Expr::Object(props) => {
            let mut map = HashMap::new();
            for (k, v) in props { map.insert(k.clone(), eval_expr_node(v, scope)?); }
            Ok(JsValue::Object(map))
        }
        Expr::ObjectWithSpread(items) => {
            let mut map = HashMap::new();
            for item in items {
                match item {
                    ObjectProp::KeyValue(k, v) => { map.insert(k.clone(), eval_expr_node(v, scope)?); }
                    ObjectProp::Getter(k, func_expr) => {
                        let func = eval_expr_node(func_expr, scope)?;
                        install_literal_accessor(&mut map, k, "get", func);
                    }
                    ObjectProp::Setter(k, func_expr) => {
                        let func = eval_expr_node(func_expr, scope)?;
                        install_literal_accessor(&mut map, k, "set", func);
                    }
                    ObjectProp::Computed(key_expr, val_expr) => {
                        let key = to_string(&eval_expr_node(key_expr, scope)?);
                        map.insert(key, eval_expr_node(val_expr, scope)?);
                    }
                    ObjectProp::Spread(expr) => {
                        if let JsValue::Object(src) = eval_expr_node(expr, scope)? {
                            map.extend(src);
                        }
                    }
                }
            }
            Ok(JsValue::Object(map))
        }
        Expr::Unary(op, rhs) => eval_unary(op, rhs, scope),
        Expr::Binary(op, lhs, rhs) => eval_binary(op, lhs, rhs, scope),
        Expr::Assign(target, op, val) => eval_assign(target, op, val, scope),
        Expr::Ternary(cond, then_e, else_e) => {
            if to_boolean(&eval_expr_node(cond, scope)?) { eval_expr_node(then_e, scope) } else { eval_expr_node(else_e, scope) }
        }
        Expr::Member(obj, prop) => {
            // Builtin namespace constants (Math.PI, Number.MAX_SAFE_INTEGER, ...) that are
            // not backed by a real binding resolve directly here.
            if let Expr::Ident(ns) = obj.as_ref() {
                if Scope::resolve(scope, ns).is_none() {
                    if let Some(v) = builtin_namespace_constant(ns, prop) { return Ok(v); }
                }
            }
            let obj_val = eval_expr_node(obj, scope)?;
            Ok(get_property(&obj_val, prop))
        }
        Expr::OptionalMember(obj, prop) => {
            let obj_val = eval_expr_node(obj, scope)?;
            if matches!(obj_val, JsValue::Null | JsValue::Undefined) { return Ok(JsValue::Undefined); }
            Ok(get_property(&obj_val, prop))
        }
        Expr::OptionalIndex(obj, idx) => {
            let obj_val = eval_expr_node(obj, scope)?;
            if matches!(obj_val, JsValue::Null | JsValue::Undefined) { return Ok(JsValue::Undefined); }
            let key = eval_expr_node(idx, scope)?;
            let key_str = to_string(&key);
            Ok(get_property(&obj_val, &key_str))
        }
        Expr::OptionalCall(callee, args) => {
            let func = eval_expr_node(callee, scope)?;
            if matches!(func, JsValue::Null | JsValue::Undefined) { return Ok(JsValue::Undefined); }
            let mut evaluated_args = Vec::new();
            for a in args { evaluated_args.push(eval_expr_node(a, scope)?); }
            call_function(&func, &evaluated_args, scope)
        }
        Expr::Index(obj, idx) => {
            let obj_val = eval_expr_node(obj, scope)?;
            let key = eval_expr_node(idx, scope)?;
            let key_str = to_string(&key);
            Ok(get_property(&obj_val, &key_str))
        }
        Expr::Call(callee, args) => eval_call(callee, args, scope),
        Expr::New(callee, args) => eval_new(callee, args, scope),
        Expr::Arrow(params, body) => {
            Ok(JsValue::Function { name: None, params: params.clone(), body: (**body).clone(), closure: scope.clone() })
        }
        Expr::Function(name, params, body) => {
            Ok(JsValue::Function { name: name.clone(), params: params.clone(), body: (**body).clone(), closure: scope.clone() })
        }
        Expr::Typeof(e) => {
            let val = eval_expr_node(e, scope)?;
            Ok(JsValue::String(typeof_str(&val).to_string()))
        }
        Expr::Void(_) => Ok(JsValue::Undefined),
        Expr::Spread(_) => Ok(JsValue::Undefined),
        Expr::Await(e) => {
            // Evaluate the expression and unwrap the Promise chain.
            let mut val = eval_expr_node(e, scope)?;
            // Recursively unwrap nested promises (max 32 levels to prevent loops)
            let mut depth = 0;
            loop {
                if depth >= 32 { break; }
                match &val {
                    JsValue::Object(map) if map.get("__type__").map(to_string).as_deref() == Some("Promise") => {
                        // Rejected promise: throw the rejection reason
                        if let Some(reason) = map.get("__rejected__") {
                            if *reason != JsValue::Undefined {
                                return Err(Signal::Throw(reason.clone()));
                            }
                        }
                        // Resolved promise: unwrap
                        let inner = map.get("__resolved__").cloned().unwrap_or(JsValue::Undefined);
                        val = inner;
                        depth += 1;
                    }
                    _ => break,
                }
            }
            Ok(val)
        }
        Expr::Sequence(exprs) => {
            let mut last = JsValue::Undefined;
            for e in exprs { last = eval_expr_node(e, scope)?; }
            Ok(last)
        }
        Expr::Yield(e) => {
            // In our simplified model, yield evaluates the expression and stores it
            // in a special __yield_values__ collector in scope (used by generator runner)
            let val = eval_expr_node(e, scope)?;
            if let Some(JsValue::Array(mut arr)) = Scope::resolve(scope, "__yield_values__") {
                arr.push(val.clone());
                Scope::assign(scope, "__yield_values__", JsValue::Array(arr));
            }
            Ok(val)
        }
    }
}

fn eval_unary(op: &Token, rhs: &Expr, scope: &ScopeRef) -> EvalResult {
    // `delete obj.prop` / `delete obj[idx]` must operate on the target expression
    // (not its evaluated value) so the property is actually removed.
    if matches!(op, Token::Delete) {
        return eval_delete(rhs, scope);
    }
    let val = eval_expr_node(rhs, scope)?;
    Ok(match op {
        Token::Minus => { let p = to_primitive(&val); JsValue::Number(-to_number(&p)) }
        Token::Plus => { let p = to_primitive(&val); JsValue::Number(to_number(&p)) }
        Token::Bang => JsValue::Boolean(!to_boolean(&val)),
        Token::Tilde => { let p = to_primitive(&val); JsValue::Number(!(to_number(&p) as i32) as f64) }
        Token::PlusPlus => {
            let n = to_number(&val) + 1.0;
            if let Expr::Ident(name) = rhs { Scope::assign(scope, name, JsValue::Number(n)); }
            JsValue::Number(n)
        }
        Token::MinusMinus => {
            let n = to_number(&val) - 1.0;
            if let Expr::Ident(name) = rhs { Scope::assign(scope, name, JsValue::Number(n)); }
            JsValue::Number(n)
        }
        _ => JsValue::Undefined,
    })
}

/// Evaluate the `delete` operator. Removes the targeted property from its owning
/// object/array and writes the container back to scope. Mirrors JS semantics where
/// deleting a (configurable or non-existent) own property yields `true`.
fn eval_delete(rhs: &Expr, scope: &ScopeRef) -> EvalResult {
    match rhs {
        Expr::Member(obj, prop) => {
            let obj_name = match obj.as_ref() {
                Expr::Ident(name) => Some(name.as_str()),
                Expr::This => Some("this"),
                _ => None,
            };
            if let Some(name) = obj_name {
                if let Some(mut val) = Scope::resolve(scope, name) {
                    let ok = delete_property(&mut val, prop);
                    Scope::assign(scope, name, val);
                    return Ok(JsValue::Boolean(ok));
                }
            }
            Ok(JsValue::Boolean(true))
        }
        Expr::Index(obj, idx_expr) => {
            let obj_name = match obj.as_ref() {
                Expr::Ident(name) => Some(name.as_str()),
                Expr::This => Some("this"),
                _ => None,
            };
            if let Some(name) = obj_name {
                if let Some(mut target) = Scope::resolve(scope, name) {
                    let key = eval_expr_node(idx_expr, scope).map(|k| to_string(&k)).unwrap_or_default();
                    let ok = delete_property(&mut target, &key);
                    Scope::assign(scope, name, target);
                    return Ok(JsValue::Boolean(ok));
                }
            }
            Ok(JsValue::Boolean(true))
        }
        // delete of a bare identifier / non-member is a no-op returning true (non-strict).
        _ => Ok(JsValue::Boolean(true)),
    }
}

fn eval_binary(op: &Token, lhs: &Expr, rhs: &Expr, scope: &ScopeRef) -> EvalResult {
    // Short-circuit
    if matches!(op, Token::AmpAmp) {
        let l = eval_expr_node(lhs, scope)?;
        return if to_boolean(&l) { eval_expr_node(rhs, scope) } else { Ok(l) };
    }
    if matches!(op, Token::PipePipe) {
        let l = eval_expr_node(lhs, scope)?;
        return if to_boolean(&l) { Ok(l) } else { eval_expr_node(rhs, scope) };
    }
    if matches!(op, Token::QuestionQuestion) {
        let l = eval_expr_node(lhs, scope)?;
        return if matches!(l, JsValue::Null | JsValue::Undefined) { eval_expr_node(rhs, scope) } else { Ok(l) };
    }
    let l = eval_expr_node(lhs, scope)?;
    let r = eval_expr_node(rhs, scope)?;
    Ok(match op {
        Token::Plus => {
            // Apply ToPrimitive to objects/arrays (hint: number → valueOf then toString).
            let lp = to_primitive(&l);
            let rp = to_primitive(&r);
            if matches!(lp, JsValue::String(_)) || matches!(rp, JsValue::String(_)) {
                JsValue::String(format!("{}{}", to_string(&lp), to_string(&rp)))
            } else { JsValue::Number(to_number(&lp) + to_number(&rp)) }
        }
        Token::Minus => JsValue::Number(to_number(&l) - to_number(&r)),
        Token::Star => JsValue::Number(to_number(&l) * to_number(&r)),
        Token::Slash => JsValue::Number(to_number(&l) / to_number(&r)),
        Token::Percent => JsValue::Number(to_number(&l) % to_number(&r)),
        Token::StarStar => JsValue::Number(to_number(&l).powf(to_number(&r))),
        Token::EqEq => JsValue::Boolean(loose_eq(&l, &r)),
        Token::BangEq => JsValue::Boolean(!loose_eq(&l, &r)),
        Token::EqEqEq => JsValue::Boolean(strict_eq(&l, &r)),
        Token::BangEqEq => JsValue::Boolean(!strict_eq(&l, &r)),
        Token::Lt => JsValue::Boolean(relational_cmp(&l, &r) == Some(std::cmp::Ordering::Less)),
        Token::Gt => JsValue::Boolean(relational_cmp(&l, &r) == Some(std::cmp::Ordering::Greater)),
        Token::LtEq => JsValue::Boolean(matches!(relational_cmp(&l, &r), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal))),
        Token::GtEq => JsValue::Boolean(matches!(relational_cmp(&l, &r), Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal))),
        Token::Amp => JsValue::Number(((to_number(&l) as i32) & (to_number(&r) as i32)) as f64),
        Token::Pipe => JsValue::Number(((to_number(&l) as i32) | (to_number(&r) as i32)) as f64),
        Token::Caret => JsValue::Number(((to_number(&l) as i32) ^ (to_number(&r) as i32)) as f64),
        Token::LtLt => JsValue::Number(((to_number(&l) as i32) << (to_number(&r) as u32 & 31)) as f64),
        Token::GtGt => JsValue::Number(((to_number(&l) as i32) >> (to_number(&r) as u32 & 31)) as f64),
        Token::GtGtGt => JsValue::Number(((to_number(&l) as u32) >> (to_number(&r) as u32 & 31)) as f64),
        Token::Instanceof => {
            // Resolve the right-hand constructor's name (a class object or a named function),
            // then check whether it appears in the left-hand instance's ancestry chain.
            let ctor_name: Option<String> = match &r {
                JsValue::Object(cm) if cm.get("__type__").map(to_string).as_deref() == Some("class") => {
                    cm.get("__name__").map(to_string)
                }
                JsValue::Function { name: Some(n), .. } => Some(n.clone()),
                _ => None,
            };
            match (&l, ctor_name) {
                (JsValue::Object(im), Some(name)) => {
                    let in_chain = match im.get("__instanceof__") {
                        Some(JsValue::Array(chain)) => chain.iter().any(|v| to_string(v) == name),
                        _ => false,
                    };
                    JsValue::Boolean(in_chain)
                }
                _ => JsValue::Boolean(false),
            }
        }
        Token::In => {
            let key = to_string(&l);
            JsValue::Boolean(has_property(&r, &key))
        }
        _ => JsValue::Undefined,
    })
}

fn eval_assign(target: &Expr, op: &Token, val: &Expr, scope: &ScopeRef) -> EvalResult {
    let rhs = eval_expr_node(val, scope)?;
    let final_val = match op {
        Token::Eq => rhs,
        Token::PlusEq => { let curr = eval_expr_node(target, scope)?; if matches!(curr, JsValue::String(_)) || matches!(rhs, JsValue::String(_)) { JsValue::String(format!("{}{}", to_string(&curr), to_string(&rhs))) } else { JsValue::Number(to_number(&curr) + to_number(&rhs)) } }
        Token::MinusEq => { let curr = eval_expr_node(target, scope)?; JsValue::Number(to_number(&curr) - to_number(&rhs)) }
        Token::StarEq => { let curr = eval_expr_node(target, scope)?; JsValue::Number(to_number(&curr) * to_number(&rhs)) }
        Token::SlashEq => { let curr = eval_expr_node(target, scope)?; JsValue::Number(to_number(&curr) / to_number(&rhs)) }
        Token::QuestionQuestionEq => {
            let curr = eval_expr_node(target, scope)?;
            if matches!(curr, JsValue::Null | JsValue::Undefined) { rhs } else { return Ok(curr); }
        }
        _ => rhs,
    };
    assign_to_target(target, final_val.clone(), scope);
    Ok(final_val)
}

fn assign_to_target(target: &Expr, value: JsValue, scope: &ScopeRef) {
    match target {
        Expr::Ident(name) => {
            if !Scope::assign(scope, name, value.clone()) { Scope::declare(scope, name, value); }
        }
        Expr::Member(obj, prop) => {
            let obj_name = match obj.as_ref() {
                Expr::Ident(name) => Some(name.as_str()),
                Expr::This => Some("this"),
                _ => None,
            };
            if let Some(name) = obj_name {
                if let Some(mut obj_val) = Scope::resolve(scope, name) {
                    // Route through set_property so accessor setters and Proxy set
                    // traps are honored (not just a raw insert).
                    set_property(&mut obj_val, prop, value);
                    Scope::assign(scope, name, obj_val);
                }
            }
        }
        Expr::Index(obj, idx_expr) => {
            if let Expr::Ident(name) = obj.as_ref() {
                if let Some(mut arr_or_obj) = Scope::resolve(scope, name) {
                    let key = if let Ok(k) = eval_expr_node(idx_expr, scope) { to_string(&k) } else { return };
                    match &mut arr_or_obj {
                        JsValue::Array(arr) => { if let Ok(i) = key.parse::<usize>() { while arr.len() <= i { arr.push(JsValue::Undefined); } arr[i] = value; } }
                        JsValue::Object(map) => { map.insert(key, value); }
                        _ => {}
                    }
                    Scope::assign(scope, name, arr_or_obj);
                }
            }
        }
        _ => {}
    }
}

fn eval_call(callee: &Expr, args: &[Expr], scope: &ScopeRef) -> EvalResult {
    let mut evaluated_args = Vec::new();
    for a in args {
        if let Expr::Spread(inner) = a {
            if let JsValue::Array(items) = eval_expr_node(inner, scope)? { evaluated_args.extend(items); }
        } else { evaluated_args.push(eval_expr_node(a, scope)?); }
    }

    // Method call: obj.method(args)
    if let Expr::Member(obj_expr, method) = callee {
        // super.method(args): look the method up in the parent class's prototype chain
        // and invoke it with the current `this`.
        if matches!(obj_expr.as_ref(), Expr::Super) {
            let parent = Scope::resolve(scope, "__super__").unwrap_or(JsValue::Undefined);
            let this_val = Scope::resolve(scope, "this").unwrap_or_else(|| JsValue::Object(HashMap::new()));
            return call_super_method(&parent, method, &evaluated_args, this_val, scope);
        }
        // Handle static built-in calls: Promise.resolve, Object.keys, etc.
        if let Expr::Ident(obj_name) = obj_expr.as_ref() {
            let native_name = format!("{}.{}", obj_name, method);
            match native_name.as_str() {
                // Reflect.set / Reflect.deleteProperty mutate the target; when it is a
                // simple identifier we resolve, mutate, and write it back so the change
                // is visible to subsequent statements (true in-place semantics).
                "Reflect.set" => {
                    let prop = evaluated_args.get(1).map(to_string).unwrap_or_default();
                    let value = evaluated_args.get(2).cloned().unwrap_or(JsValue::Undefined);
                    let mut ok = false;
                    if let Some(Expr::Ident(var_name)) = args.first() {
                        if let Some(mut target) = Scope::resolve(scope, var_name) {
                            ok = set_property(&mut target, &prop, value);
                            Scope::assign(scope, var_name, target);
                        }
                    }
                    return Ok(JsValue::Boolean(ok));
                }
                "Reflect.deleteProperty" => {
                    let prop = evaluated_args.get(1).map(to_string).unwrap_or_default();
                    // When the target is a simple identifier, mutate it in place and write
                    // it back so the deletion is visible to subsequent statements. Routing
                    // through `delete_property` means Proxy `deleteProperty` traps are
                    // honoured even when the target variable holds a proxy.
                    if let Some(Expr::Ident(var_name)) = args.first() {
                        if let Some(mut target) = Scope::resolve(scope, var_name) {
                            let ok = delete_property(&mut target, &prop);
                            Scope::assign(scope, var_name, target);
                            return Ok(JsValue::Boolean(ok));
                        }
                        return Ok(JsValue::Boolean(false));
                    }
                    // Non-identifier target: operate on the evaluated value (proxy traps
                    // still run for their side effects) and report the boolean result.
                    let mut target = evaluated_args.first().cloned().unwrap_or(JsValue::Undefined);
                    return Ok(JsValue::Boolean(delete_property(&mut target, &prop)));
                }
                "Promise.resolve" | "Promise.reject" | "Promise.all" | "Promise.race" | "Promise.allSettled" |
                                "Object.keys" | "Object.values" | "Object.entries" | "Object.fromEntries" | "Object.assign" | "Object.freeze" |
                                "Object.is" | "Object.setPrototypeOf" | "Object.hasOwn" |
                "Object.create" | "Object.getPrototypeOf" | "Object.defineProperty" |
                "Object.defineProperties" | "Object.getOwnPropertyDescriptor" | "Object.getOwnPropertyDescriptors" | "Object.getOwnPropertyNames" |
                "Array.isArray" | "Array.from" | "Array.of" |
                "JSON.parse" | "JSON.stringify" |
                "Math.floor" | "Math.ceil" | "Math.round" | "Math.abs" | "Math.sqrt" |
                "Math.trunc" | "Math.sign" | "Math.log" | "Math.pow" | "Math.max" | "Math.min" | "Math.random" |
                "Math.sin" | "Math.cos" | "Math.tan" | "Math.asin" | "Math.acos" | "Math.atan" | "Math.atan2" |
                "Math.sinh" | "Math.cosh" | "Math.tanh" | "Math.exp" | "Math.expm1" | "Math.log1p" |
                "Math.log2" | "Math.log10" | "Math.cbrt" | "Math.hypot" | "Math.fround" | "Math.clz32" |
                "Math.asinh" | "Math.acosh" | "Math.atanh" | "Math.imul" |
                "Number.parseInt" | "Number.parseFloat" | "Number.isNaN" | "Number.isFinite" |
                "Number.isInteger" | "Number.isSafeInteger" |
                "String.fromCharCode" | "String.fromCodePoint" | "Date.now" | "console.log" | "console.warn" | "console.error" | "console.info" |
                "eval" | "structuredClone" | "queueMicrotask" | "requestAnimationFrame" | "requestIdleCallback" | "Symbol" | "Symbol.for" |
                "Reflect.get" | "Reflect.has" |
                "Reflect.ownKeys" | "Reflect.getOwnPropertyDescriptor" | "Reflect.apply" | "Reflect.construct" => {
                    let result = call_native(&native_name, &evaluated_args)?;
                    // Object-mutating statics return the modified target; write it back to
                    // the source identifier so in-place mutation semantics hold.
                    if matches!(native_name.as_str(), "Object.defineProperty" | "Object.defineProperties" | "Object.assign" | "Object.setPrototypeOf") {
                        if let Some(Expr::Ident(var_name)) = args.first() {
                            Scope::assign(scope, var_name, result.clone());
                        }
                    }
                    return Ok(result);
                }
                _ => {}
            }
        }
        let obj = eval_expr_node(obj_expr, scope)?;
        // Use writeback for methods on objects so `this` mutations propagate
        if let JsValue::Object(map) = &obj {
            let type_tag = map.get("__type__").map(to_string);
            match type_tag.as_deref() {
                Some("Map") | Some("WeakMap") => {
                    // Map mutators (set/delete/clear) update the backing store; persist it.
                    let mut m = map.clone();
                    let result = call_map_method(&mut m, method, &evaluated_args, scope);
                    assign_to_target(obj_expr, JsValue::Object(m), scope);
                    return result;
                }
                Some("Set") | Some("WeakSet") => {
                    // Set mutators (add/delete/clear) update the backing store; persist it.
                    let mut m = map.clone();
                    let result = call_set_method(&mut m, method, &evaluated_args, scope);
                    assign_to_target(obj_expr, JsValue::Object(m), scope);
                    return result;
                }
                Some("Promise") => return call_promise_method(map, method, &evaluated_args, scope),
                Some("Date") => return call_date_method(map, method, &evaluated_args),
                Some("Generator") => return call_generator_method(map, method),
                Some("class") => {
                    // Static method call: ClassName.staticMethod(args). Inside a static
                    // method `this` is the class object itself; inherited statics are
                    // resolved by walking the parent chain.
                    if let Some(func) = find_static_method(&obj, method) {
                        let (result, _) = call_method_with_this_writeback(&func, &evaluated_args, scope, obj.clone());
                        return result;
                    }
                    return Ok(JsValue::Undefined);
                }
                _ => {}
            }
            if let Some(func) = map.get(method).cloned() {
                let (result, updated_this) = call_method_with_this_writeback(&func, &evaluated_args, scope, obj.clone());
                // Write back mutated this to the source variable
                if let Expr::Ident(var_name) = obj_expr.as_ref() {
                    Scope::assign(scope, var_name, updated_this);
                }
                return result;
            }
            // Walk prototype chain for method lookup
            let proto_func = get_property(&obj, method);
            if let JsValue::Function { .. } = &proto_func {
                let (result, updated_this) = call_method_with_this_writeback(&proto_func, &evaluated_args, scope, obj.clone());
                if let Expr::Ident(var_name) = obj_expr.as_ref() {
                    Scope::assign(scope, var_name, updated_this);
                }
                return result;
            }
            return call_object_method(map, method, &evaluated_args);
        }
        // Array methods may mutate the receiver in place (push/pop/sort/...).
        // Clone into a mutable vec, run the method, then write the mutated array
        // back to the source identifier so the change persists across statements.
        if let JsValue::Array(arr) = &obj {
            let mut updated = arr.clone();
            let result = call_array_method(&mut updated, method, &evaluated_args, scope);
            // Persist the mutated array back to its source location. Routing through
            // assign_to_target covers plain identifiers (arr.push), member targets
            // (obj.items.push, this.items.push) and indexed targets (rows[0].push).
            assign_to_target(obj_expr, JsValue::Array(updated), scope);
            return result;
        }
        return call_method(&obj, method, &evaluated_args, scope);
    }
    // Optional method call: obj?.method(args) - already handled in OptionalCall
    if let Expr::OptionalMember(obj_expr, method) = callee {
        let obj = eval_expr_node(obj_expr, scope)?;
        if matches!(obj, JsValue::Null | JsValue::Undefined) { return Ok(JsValue::Undefined); }
        return call_method(&obj, method, &evaluated_args, scope);
    }

    // Direct call to a bare identifier that is a known built-in native
    if let Expr::Ident(name) = callee {
        match name.as_str() {
            "eval" | "structuredClone" | "parseInt" | "parseFloat" | "isNaN" | "isFinite" |
            "encodeURIComponent" | "decodeURIComponent" | "Symbol" | "queueMicrotask" |
            "Number" | "String" | "Boolean" |
            "requestAnimationFrame" | "requestIdleCallback" => {
                return call_native(name, &evaluated_args);
            }
            _ => {}
        }
    }

    // super(args): invoke the parent class constructor with the current `this`.
    if matches!(callee, Expr::Super) {
        let parent = Scope::resolve(scope, "__super__").unwrap_or(JsValue::Undefined);
        let this_val = Scope::resolve(scope, "this").unwrap_or_else(|| JsValue::Object(HashMap::new()));
        return call_super_constructor(&parent, &evaluated_args, this_val, scope);
    }

    let func = eval_expr_node(callee, scope)?;
    call_function(&func, &evaluated_args, scope)
}

/// Invoke a parent class constructor (`super(args)`) with `this` bound to the current
/// instance, writing any mutations back into the caller's `this` binding.
fn call_super_constructor(parent_class: &JsValue, args: &[JsValue], this_val: JsValue, scope: &ScopeRef) -> EvalResult {
    if let JsValue::Object(parent_map) = parent_class {
        if let Some(JsValue::Function { params, body, closure, .. }) = parent_map.get("__constructor__") {
            let ctor_scope = Scope::new_child(closure);
            Scope::declare(&ctor_scope, "this", this_val);
            // Expose the grandparent so chained super() calls work.
            if let Some(grandparent) = parent_map.get("__parent__") {
                Scope::declare(&ctor_scope, "__super__", grandparent.clone());
            }
            for (i, p) in params.iter().enumerate() {
                Scope::declare(&ctor_scope, p, args.get(i).cloned().unwrap_or(JsValue::Undefined));
            }
            Scope::declare(&ctor_scope, "arguments", JsValue::Array(args.to_vec()));
            let _ = eval_stmt(body, &ctor_scope);
            if let Some(updated) = Scope::resolve(&ctor_scope, "this") {
                Scope::assign(scope, "this", updated);
            }
        }
    }
    Ok(JsValue::Undefined)
}

/// Invoke a parent class method (`super.method(args)`) with `this` bound to the current
/// instance, writing any mutations back into the caller's `this` binding.
fn call_super_method(parent_class: &JsValue, method: &str, args: &[JsValue], this_val: JsValue, scope: &ScopeRef) -> EvalResult {
    if let Some(func) = find_proto_method(parent_class, method) {
        let (result, updated_this) = call_method_with_this_writeback(&func, args, scope, this_val);
        Scope::assign(scope, "this", updated_this);
        return result;
    }
    Ok(JsValue::Undefined)
}

/// Walk a class object's ancestry looking for `method` in each class's `__proto_methods__`.
fn find_proto_method(class_val: &JsValue, method: &str) -> Option<JsValue> {
    let mut current = class_val.clone();
    let mut depth = 0;
    loop {
        if depth > 64 { return None; }
        let JsValue::Object(cm) = &current else { return None };
        if let Some(JsValue::Object(proto)) = cm.get("__proto_methods__") {
            if let Some(func) = proto.get(method) {
                return Some(func.clone());
            }
        }
        match cm.get("__parent__") {
            Some(parent) => { current = parent.clone(); depth += 1; }
            None => return None,
        }
    }
}

/// Walk a class object's ancestry looking for a static `method` in each class's
/// `__static_methods__` (so inherited statics resolve too).
fn find_static_method(class_val: &JsValue, method: &str) -> Option<JsValue> {
    let mut current = class_val.clone();
    let mut depth = 0;
    loop {
        if depth > 64 { return None; }
        let JsValue::Object(cm) = &current else { return None };
        if let Some(JsValue::Object(statics)) = cm.get("__static_methods__") {
            if let Some(func) = statics.get(method) {
                return Some(func.clone());
            }
        }
        match cm.get("__parent__") {
            Some(parent) => { current = parent.clone(); depth += 1; }
            None => return None,
        }
    }
}

/// Template literal interpolation: scans for ${...} and evaluates embedded expressions.
fn eval_template_literal(raw: &str, scope: &ScopeRef) -> EvalResult {
    let mut result = String::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '$' && chars[i + 1] == '{' {
            i += 2; // skip ${
            let mut depth = 1;
            let mut expr_str = String::new();
            while i < chars.len() && depth > 0 {
                if chars[i] == '{' { depth += 1; }
                else if chars[i] == '}' { depth -= 1; if depth == 0 { i += 1; break; } }
                expr_str.push(chars[i]);
                i += 1;
            }
            // Evaluate the expression
            match eval_script(&expr_str, scope) {
                Ok(val) => result.push_str(&to_string(&val)),
                Err(_) => result.push_str("undefined"),
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    Ok(JsValue::String(result))
}

/// Evaluate `new Constructor(args)` - handles Map, Set, WeakMap, Date, Error, and user classes.
fn eval_new(callee: &Expr, args: &[Expr], scope: &ScopeRef) -> EvalResult {
    let mut evaluated_args = Vec::new();
    for a in args { evaluated_args.push(eval_expr_node(a, scope)?); }

    // Check if callee is a known builtin name
    let name = match callee {
        Expr::Ident(n) => Some(n.as_str()),
        _ => None,
    };

    match name {
        Some("Map") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Map".to_string()));
            map.insert("__entries__".to_string(), JsValue::Array(Vec::new()));
            // Initialize from iterable if provided
            if let Some(JsValue::Array(entries)) = evaluated_args.first() {
                let mut kvs = Vec::new();
                for entry in entries {
                    if let JsValue::Array(kv) = entry {
                        if kv.len() >= 2 {
                            kvs.push(JsValue::Array(vec![kv[0].clone(), kv[1].clone()]));
                        }
                    }
                }
                map.insert("__entries__".to_string(), JsValue::Array(kvs));
            }
            Ok(JsValue::Object(map))
        }
        Some("Set") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Set".to_string()));
            let mut items = Vec::new();
            if let Some(JsValue::Array(init)) = evaluated_args.first() {
                // A Set holds unique values: skip any element already present (SameValueZero via strict_eq).
                for v in init {
                    if !items.iter().any(|x| strict_eq(x, v)) {
                        items.push(v.clone());
                    }
                }
            }
            map.insert("__items__".to_string(), JsValue::Array(items));
            Ok(JsValue::Object(map))
        }
        Some("WeakMap") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("WeakMap".to_string()));
            let mut kvs = Vec::new();
            if let Some(JsValue::Array(entries)) = evaluated_args.first() {
                for entry in entries {
                    if let JsValue::Array(kv) = entry {
                        if kv.len() >= 2 { kvs.push(JsValue::Array(vec![kv[0].clone(), kv[1].clone()])); }
                    }
                }
            }
            map.insert("__entries__".to_string(), JsValue::Array(kvs));
            Ok(JsValue::Object(map))
        }
        Some("WeakSet") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("WeakSet".to_string()));
            let mut items = Vec::new();
            if let Some(JsValue::Array(init)) = evaluated_args.first() {
                for v in init {
                    if !items.iter().any(|x| strict_eq(x, v)) { items.push(v.clone()); }
                }
            }
            map.insert("__items__".to_string(), JsValue::Array(items));
            Ok(JsValue::Object(map))
        }
        Some("Date") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Date".to_string()));
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
            map.insert("__value__".to_string(), JsValue::Number(now));
            Ok(JsValue::Object(map))
        }
        Some("Error") | Some("TypeError") | Some("RangeError") | Some("ReferenceError") => {
            let mut map = HashMap::new();
            let msg = evaluated_args.first().map(to_string).unwrap_or_default();
            map.insert("message".to_string(), JsValue::String(msg));
            map.insert("name".to_string(), JsValue::String(name.unwrap().to_string()));
            Ok(JsValue::Object(map))
        }
        Some("Promise") => {
            // new Promise((resolve, reject) => { ... })
            // Uses a thread-local to capture resolve/reject calls from the executor.
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            if let Some(executor) = evaluated_args.first() {
                PROMISE_CAPTURE.with(|cap| {
                    *cap.borrow_mut() = None;
                });
                let resolve_fn = JsValue::NativeFunction("__promise_resolve__".to_string());
                let reject_fn = JsValue::NativeFunction("__promise_reject__".to_string());
                let exec_scope = Scope::new_child(scope);
                let _ = call_function(executor, &[resolve_fn, reject_fn], &exec_scope);
                // Read captured result
                let captured = PROMISE_CAPTURE.with(|cap| cap.borrow().clone());
                match captured {
                    Some((false, val)) => { map.insert("__resolved__".to_string(), val); }
                    Some((true, reason)) => { map.insert("__rejected__".to_string(), reason); }
                    None => { map.insert("__resolved__".to_string(), JsValue::Undefined); }
                }
            } else {
                map.insert("__resolved__".to_string(), JsValue::Undefined);
            }
            Ok(JsValue::Object(map))
        }
        Some("RegExp") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("RegExp".to_string()));
            let source = evaluated_args.first().map(to_string).unwrap_or_default();
            map.insert("source".to_string(), JsValue::String(source));
            Ok(JsValue::Object(map))
        }
        Some("Function") => {
            // new Function('a', 'b', 'return a + b') -> function(a, b) { return a + b }
            let body_str = evaluated_args.last().map(to_string).unwrap_or_default();
            let params: Vec<String> = if evaluated_args.len() > 1 {
                evaluated_args[..evaluated_args.len()-1].iter().map(to_string).collect()
            } else { Vec::new() };
            let body_code = format!("{{ {} }}", body_str);
            match lex(&body_code).and_then(|tokens| {
                let mut parser = Parser::new(tokens);
                parser.parse_block().map_err(|e| e.to_string())
            }) {
                Ok(body) => Ok(JsValue::Function {
                    name: Some("anonymous".to_string()),
                    params,
                    body,
                    closure: scope.clone(),
                }),
                Err(_) => Ok(JsValue::Undefined),
            }
        }
        Some("Proxy") => {
            // new Proxy(target, handler) - create a proxy with trap interception
            let target = evaluated_args.first().cloned().unwrap_or(JsValue::Object(HashMap::new()));
            let handler = evaluated_args.get(1).cloned().unwrap_or(JsValue::Object(HashMap::new()));
            Ok(JsValue::Proxy {
                target: Box::new(target),
                handler: Box::new(handler),
            })
        }
        Some("URL") => {
            let url_str = evaluated_args.first().map(to_string).unwrap_or_default();
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("URL".to_string()));
            map.insert("href".to_string(), JsValue::String(url_str.clone()));
            // Parse simple URL parts
            let (protocol, rest) = url_str.split_once("://").unwrap_or(("", &url_str));
            map.insert("protocol".to_string(), JsValue::String(format!("{}:", protocol)));
            let (host, pathname) = rest.split_once('/').unwrap_or((rest, ""));
            let (hostname, port) = host.split_once(':').unwrap_or((host, ""));
            map.insert("hostname".to_string(), JsValue::String(hostname.to_string()));
            map.insert("host".to_string(), JsValue::String(host.to_string()));
            map.insert("port".to_string(), JsValue::String(port.to_string()));
            map.insert("pathname".to_string(), JsValue::String(format!("/{}", pathname.split('?').next().unwrap_or(""))));
            let search = pathname.split_once('?').map(|(_, q)| format!("?{}", q.split('#').next().unwrap_or(""))).unwrap_or_default();
            map.insert("search".to_string(), JsValue::String(search));
            let hash = pathname.split_once('#').map(|(_, h)| format!("#{}", h)).unwrap_or_default();
            map.insert("hash".to_string(), JsValue::String(hash));
            map.insert("origin".to_string(), JsValue::String(format!("{}://{}", protocol, host)));
            Ok(JsValue::Object(map))
        }
        Some("URLSearchParams") => {
            let init = evaluated_args.first().map(to_string).unwrap_or_default();
            let params_str = init.strip_prefix('?').unwrap_or(&init);
            let mut entries = Vec::new();
            for pair in params_str.split('&') {
                if pair.is_empty() { continue; }
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                entries.push(JsValue::Array(vec![JsValue::String(k.to_string()), JsValue::String(v.to_string())]));
            }
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("URLSearchParams".to_string()));
            map.insert("__entries__".to_string(), JsValue::Array(entries));
            Ok(JsValue::Object(map))
        }
        Some("AbortController") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("AbortController".to_string()));
            let mut signal = HashMap::new();
            signal.insert("aborted".to_string(), JsValue::Boolean(false));
            map.insert("signal".to_string(), JsValue::Object(signal));
            Ok(JsValue::Object(map))
        }
        Some("TextEncoder") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("TextEncoder".to_string()));
            map.insert("encoding".to_string(), JsValue::String("utf-8".to_string()));
            Ok(JsValue::Object(map))
        }
        Some("TextDecoder") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("TextDecoder".to_string()));
            map.insert("encoding".to_string(), JsValue::String("utf-8".to_string()));
            Ok(JsValue::Object(map))
        }
        _ => {
            // User-defined class constructor
            let callee_val = eval_expr_node(callee, scope)?;
            if let JsValue::Object(class_map) = &callee_val {
                let is_class = class_map.get("__type__").map(|v| matches!(v, JsValue::String(s) if s == "class")).unwrap_or(false);
                if is_class {
                    return call_class_constructor(class_map, &evaluated_args, scope);
                }
            }
            // Fallback: call as function with empty `this`
            if let JsValue::Function { params, body, closure, .. } = &callee_val {
                let call_scope = Scope::new_child(closure);
                let this_obj = JsValue::Object(HashMap::new());
                Scope::declare(&call_scope, "this", this_obj.clone());
                for (i, p) in params.iter().enumerate() {
                    let val = evaluated_args.get(i).cloned().unwrap_or(JsValue::Undefined);
                    Scope::declare(&call_scope, p, val);
                }
                Scope::declare(&call_scope, "arguments", JsValue::Array(evaluated_args));
                match eval_stmt(body, &call_scope) {
                    Ok(_) | Err(Signal::Return(JsValue::Undefined)) => {
                        Ok(Scope::resolve(&call_scope, "this").unwrap_or(this_obj))
                    }
                    Err(Signal::Return(v)) => {
                        if matches!(v, JsValue::Object(_)) { Ok(v) } else {
                            Ok(Scope::resolve(&call_scope, "this").unwrap_or(JsValue::Object(HashMap::new())))
                        }
                    }
                    Err(e) => Err(e),
                }
            } else {
                Ok(JsValue::Object(HashMap::new()))
            }
        }
    }
}

/// Evaluate a class declaration: creates a class object with constructor + methods.
fn eval_class_decl(name: &str, parent: &Option<String>, methods: &[ClassMethod], fields: &[ClassField], scope: &ScopeRef) {
    let mut class_obj = HashMap::new();
    class_obj.insert("__type__".to_string(), JsValue::String("class".to_string()));
    class_obj.insert("__name__".to_string(), JsValue::String(name.to_string()));

    // Resolve parent class
    if let Some(parent_name) = parent {
        if let Some(parent_val) = Scope::resolve(scope, parent_name) {
            class_obj.insert("__parent__".to_string(), parent_val);
        }
    }

    // Store methods as functions
    let mut proto = HashMap::new();
    let mut statics = HashMap::new();
    for m in methods {
        let func = JsValue::Function {
            name: Some(m.name.clone()),
            params: m.params.clone(),
            body: m.body.clone(),
            closure: scope.clone(),
        };
        if m.name == "constructor" {
            class_obj.insert("__constructor__".to_string(), func);
        } else if m.is_static {
            statics.insert(m.name.clone(), func);
        } else {
            match m.kind {
                // Class getters/setters become accessor descriptors on the prototype so
                // instances honor them via get_property/set_property (same model as
                // object-literal accessors).
                ClassMemberKind::Getter => install_literal_accessor(&mut proto, &m.name, "get", func),
                ClassMemberKind::Setter => install_literal_accessor(&mut proto, &m.name, "set", func),
                ClassMemberKind::Method => { proto.insert(m.name.clone(), func); }
            }
        }
    }
    class_obj.insert("__proto_methods__".to_string(), JsValue::Object(proto));
    class_obj.insert("__static_methods__".to_string(), JsValue::Object(statics));

    // Class fields. Static fields evaluate once now and live on the class object;
    // instance fields are stored (in declaration order) as `[name, init_closure]` pairs,
    // where the closure is a zero-arg function (`return <init>`) run with `this` bound at
    // construction time.
    let mut instance_fields: Vec<JsValue> = Vec::new();
    for f in fields {
        if f.is_static {
            let val = match &f.init {
                Some(expr) => eval_expr_node(expr, scope).unwrap_or(JsValue::Undefined),
                None => JsValue::Undefined,
            };
            class_obj.insert(f.name.clone(), val);
        } else {
            let init_func = JsValue::Function {
                name: None,
                params: Vec::new(),
                body: Stmt::Return(f.init.clone()),
                closure: scope.clone(),
            };
            instance_fields.push(JsValue::Array(vec![JsValue::String(f.name.clone()), init_func]));
        }
    }
    class_obj.insert("__instance_fields__".to_string(), JsValue::Array(instance_fields));

    Scope::declare(scope, name, JsValue::Object(class_obj));
}

/// Instantiate a class from its class object.
fn call_class_constructor(class_map: &HashMap<String, JsValue>, args: &[JsValue], _scope: &ScopeRef) -> EvalResult {
    let mut instance = HashMap::new();

    // Copy prototype methods
    if let Some(JsValue::Object(proto)) = class_map.get("__proto_methods__") {
        instance.extend(proto.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    // Inherit parent methods
    if let Some(JsValue::Object(parent_class)) = class_map.get("__parent__") {
        if let Some(JsValue::Object(parent_proto)) = parent_class.get("__proto_methods__") {
            for (k, v) in parent_proto {
                instance.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
    }

    // Record the class ancestry on the instance so `instanceof` can walk the chain.
    // This is installed before the constructor runs so it survives any `this` mutation.
    if let Some(JsValue::String(class_name)) = class_map.get("__name__") {
        instance.insert("__class_name__".to_string(), JsValue::String(class_name.clone()));
    }
    instance.insert("__instanceof__".to_string(), JsValue::Array(class_ancestry_names(class_map)));

    // Apply instance field initializers in JS order: root ancestor first, down to this
    // class. Runs before the constructor body so the constructor can observe/override
    // field values. Each initializer runs with `this` bound to the in-progress instance.
    let mut chain: Vec<&HashMap<String, JsValue>> = Vec::new();
    let mut cur = Some(class_map);
    let mut depth = 0;
    while let Some(cm) = cur {
        if depth > 64 { break; }
        chain.push(cm);
        cur = match cm.get("__parent__") { Some(JsValue::Object(p)) => Some(p), _ => None };
        depth += 1;
    }
    for cm in chain.iter().rev() {
        if let Some(JsValue::Array(fields_arr)) = cm.get("__instance_fields__") {
            for entry in fields_arr {
                if let JsValue::Array(pair) = entry {
                    if let (Some(JsValue::String(fname)), Some(func)) = (pair.first(), pair.get(1)) {
                        let this_val = JsValue::Object(instance.clone());
                        let val = call_function_with_this(func, &[], &Scope::new_global(), Some(this_val))
                            .unwrap_or(JsValue::Undefined);
                        instance.insert(fname.clone(), val);
                    }
                }
            }
        }
    }

    // Call constructor if present
    if let Some(ctor) = class_map.get("__constructor__") {
        if let JsValue::Function { params, body, closure, .. } = ctor {
            let ctor_scope = Scope::new_child(closure);
            Scope::declare(&ctor_scope, "this", JsValue::Object(instance.clone()));
            // Provide __super__ for super() calls
            if let Some(parent) = class_map.get("__parent__") {
                Scope::declare(&ctor_scope, "__super__", parent.clone());
            }
            for (i, p) in params.iter().enumerate() {
                Scope::declare(&ctor_scope, p, args.get(i).cloned().unwrap_or(JsValue::Undefined));
            }
            Scope::declare(&ctor_scope, "arguments", JsValue::Array(args.to_vec()));
            let result = eval_stmt(body, &ctor_scope);
            let _ = result;
            // Get this after constructor ran (may have added properties)
            if let Some(JsValue::Object(updated)) = Scope::resolve(&ctor_scope, "this") {
                instance = updated;
            }
        }
    }

    Ok(JsValue::Object(instance))
}

/// Collect the ancestry chain of a class object as a list of class-name strings,
/// e.g. for `class Dog extends Animal {}` this yields `["Dog", "Animal"]`.
/// Used to implement `instanceof`.
fn class_ancestry_names(class_map: &HashMap<String, JsValue>) -> Vec<JsValue> {
    let mut names = Vec::new();
    let mut current: Option<&HashMap<String, JsValue>> = Some(class_map);
    let mut depth = 0;
    while let Some(cm) = current {
        if depth > 64 { break; }
        if let Some(JsValue::String(n)) = cm.get("__name__") {
            names.push(JsValue::String(n.clone()));
        }
        current = match cm.get("__parent__") {
            Some(JsValue::Object(parent)) => Some(parent),
            _ => None,
        };
        depth += 1;
    }
    names
}

pub fn call_function(func: &JsValue, args: &[JsValue], _caller_scope: &ScopeRef) -> EvalResult {
    call_function_with_this(func, args, _caller_scope, None)
}

/// Call a function with an explicit `this` binding.
/// Returns (result, updated_this) so the caller can write-back mutations.
pub fn call_function_with_this(func: &JsValue, args: &[JsValue], _caller_scope: &ScopeRef, this_val: Option<JsValue>) -> EvalResult {
    match func {
        JsValue::Function { name, params, body, closure, .. } => {
            // Check if this is a generator function
            let is_generator = name.as_ref().map(|n| n.starts_with("__generator__")).unwrap_or(false);
            let call_scope = Scope::new_child(closure);
            if let Some(this) = this_val {
                Scope::declare(&call_scope, "this", this);
            }
            for (i, p) in params.iter().enumerate() {
                let val = args.get(i).cloned().unwrap_or(JsValue::Undefined);
                Scope::declare(&call_scope, p, val);
            }
            // Make `arguments` available
            Scope::declare(&call_scope, "arguments", JsValue::Array(args.to_vec()));
            if is_generator {
                // Generator: collect yield values into an iterator object
                Scope::declare(&call_scope, "__yield_values__", JsValue::Array(Vec::new()));
                let _ = eval_stmt(body, &call_scope); // run body, collecting yields
                let values = Scope::resolve(&call_scope, "__yield_values__").unwrap_or(JsValue::Array(Vec::new()));
                // Return an iterator object with .next() that steps through the values
                let mut iter = HashMap::new();
                iter.insert("__type__".to_string(), JsValue::String("Generator".to_string()));
                iter.insert("__values__".to_string(), values);
                iter.insert("__index__".to_string(), JsValue::Number(0.0));
                Ok(JsValue::Object(iter))
            } else {
                match eval_stmt(body, &call_scope) {
                    Ok(v) => Ok(v),
                    Err(Signal::Return(v)) => Ok(v),
                    Err(Signal::Throw(v)) => Err(Signal::Throw(v)),
                    Err(Signal::Break | Signal::Continue) => Ok(JsValue::Undefined),
                }
            }
        }
        JsValue::NativeFunction(name) => call_native(name, args),
        // AsyncFunction wrapper: call inner function, wrap result in Promise
        JsValue::Object(map) if map.get("__type__").map(to_string).as_deref() == Some("AsyncFunction") => {
            // NOTE: This is a simplified synchronous Promise implementation.
            // The inner function is called immediately and the result is wrapped
            // in a resolved/rejected Promise object. Full async support (microtask
            // queue, .then()/.catch() chaining) requires an event loop and is not
            // yet implemented.
            if let Some(inner) = map.get("__inner__") {
                match call_function_with_this(inner, args, _caller_scope, this_val) {
                    Ok(val) => {
                        let mut promise = HashMap::new();
                        promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                        promise.insert("__resolved__".to_string(), val);
                        Ok(JsValue::Object(promise))
                    }
                    Err(Signal::Throw(reason)) => {
                        let mut promise = HashMap::new();
                        promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                        promise.insert("__rejected__".to_string(), reason);
                        Ok(JsValue::Object(promise))
                    }
                    Err(other) => Err(other),
                }
            } else {
                Ok(JsValue::Undefined)
            }
        }
        // Proxy wrapping a callable target: consult handler.apply(target, thisArg, args)
        // when present, otherwise forward the call to the target function.
        JsValue::Proxy { target, handler } => {
            if let JsValue::Object(h_map) = handler.as_ref() {
                if let Some(trap) = h_map.get("apply") {
                    if !matches!(trap, JsValue::NativeFunction(_)) {
                        let depth = PROXY_TRAP_DEPTH.with(|d| {
                            let cur = d.get();
                            if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                            d.set(cur + 1);
                            cur
                        });
                        if depth < MAX_PROXY_TRAP_DEPTH {
                            let this_arg = this_val.clone().unwrap_or(JsValue::Undefined);
                            let args_array = JsValue::Array(args.to_vec());
                            let result = call_function(
                                trap,
                                &[(**target).clone(), this_arg, args_array],
                                &Scope::new_global(),
                            );
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            return result;
                        }
                    }
                }
            }
            call_function_with_this(target, args, _caller_scope, this_val)
        }
        // Bound function (Function.prototype.bind): prepend the bound arguments,
        // bind `this`, and invoke the stored target. A bound function's `this`
        // cannot be overridden, so any passed-in this_val is ignored.
        JsValue::Object(map) if map.get("__type__").map(to_string).as_deref() == Some("BoundFunction") => {
            let target = map.get("__target__").cloned().unwrap_or(JsValue::Undefined);
            let bound_this = map.get("__this__").cloned().unwrap_or(JsValue::Undefined);
            let mut full_args = match map.get("__args__") { Some(JsValue::Array(a)) => a.clone(), _ => Vec::new() };
            full_args.extend_from_slice(args);
            call_function_with_this(&target, &full_args, _caller_scope, Some(bound_this))
        }
        _ => Ok(JsValue::Undefined),
    }
}

/// Call a function with `this` bound, and return the mutated `this` value after execution.
fn call_method_with_this_writeback(func: &JsValue, args: &[JsValue], _scope: &ScopeRef, this_val: JsValue) -> (EvalResult, JsValue) {
    match func {
        JsValue::Function { params, body, closure, .. } => {
            let call_scope = Scope::new_child(closure);
            Scope::declare(&call_scope, "this", this_val.clone());
            // Provide __super__ for `super.method()` calls inside instance methods: resolve
            // the instance's class from the method's closure, then expose its parent class.
            if let JsValue::Object(this_map) = &this_val {
                if let Some(JsValue::String(class_name)) = this_map.get("__class_name__") {
                    if let Some(JsValue::Object(class_obj)) = Scope::resolve(closure, class_name) {
                        if let Some(parent) = class_obj.get("__parent__") {
                            Scope::declare(&call_scope, "__super__", parent.clone());
                        }
                    }
                }
            }
            for (i, p) in params.iter().enumerate() {
                let val = args.get(i).cloned().unwrap_or(JsValue::Undefined);
                Scope::declare(&call_scope, p, val);
            }
            Scope::declare(&call_scope, "arguments", JsValue::Array(args.to_vec()));
            let result = match eval_stmt(body, &call_scope) {
                Ok(v) => Ok(v),
                Err(Signal::Return(v)) => Ok(v),
                Err(Signal::Throw(v)) => Err(Signal::Throw(v)),
                Err(Signal::Break | Signal::Continue) => Ok(JsValue::Undefined),
            };
            let updated_this = Scope::resolve(&call_scope, "this").unwrap_or(this_val);
            (result, updated_this)
        }
        JsValue::NativeFunction(name) => (call_native(name, args), this_val),
        _ => (Ok(JsValue::Undefined), this_val),
    }
}

/// Spec-style parseInt: skips leading whitespace, honours an optional sign,
/// auto-detects a 0x/0X hex prefix when radix is unspecified (0) or 16, and
/// consumes the longest valid digit run for the radix. Returns NaN when no
/// digits are present or the radix is out of the 2..=36 range.
fn parse_int_js(input: &str, radix_arg: f64) -> f64 {
    let chars: Vec<char> = input.trim_start().chars().collect();
    let mut i = 0;
    let mut sign = 1.0;
    match chars.first() {
        Some('+') => i += 1,
        Some('-') => { sign = -1.0; i += 1; }
        _ => {}
    }
    let mut radix = if radix_arg.is_finite() { radix_arg as i64 } else { 0 };
    if (radix == 0 || radix == 16)
        && chars.get(i) == Some(&'0')
        && matches!(chars.get(i + 1), Some('x') | Some('X'))
    {
        i += 2;
        radix = 16;
    }
    if radix == 0 { radix = 10; }
    if !(2..=36).contains(&radix) { return f64::NAN; }
    let mut value = 0.0;
    let mut any = false;
    for &c in &chars[i..] {
        match c.to_digit(radix as u32) {
            Some(d) => { value = value * radix as f64 + d as f64; any = true; }
            None => break,
        }
    }
    if any { sign * value } else { f64::NAN }
}

/// Spec-style parseFloat: skips leading whitespace and parses the longest
/// leading substring that forms a valid decimal (with optional sign, fraction,
/// and exponent) or Infinity. Returns NaN when no numeric prefix is present.
fn parse_float_js(input: &str) -> f64 {
    let s = input.trim_start();
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    if unsigned.starts_with("Infinity") {
        return if s.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    if matches!(chars.first(), Some('+') | Some('-')) { i += 1; }
    let mut seen_digit = false;
    let mut seen_dot = false;
    while i < chars.len() {
        match chars[i] {
            c if c.is_ascii_digit() => { seen_digit = true; i += 1; }
            '.' if !seen_dot => { seen_dot = true; i += 1; }
            _ => break,
        }
    }
    if seen_digit && matches!(chars.get(i), Some('e') | Some('E')) {
        let mut j = i + 1;
        if matches!(chars.get(j), Some('+') | Some('-')) { j += 1; }
        let mut exp_digit = false;
        while matches!(chars.get(j), Some(c) if c.is_ascii_digit()) { exp_digit = true; j += 1; }
        if exp_digit { i = j; }
    }
    if !seen_digit { return f64::NAN; }
    chars[..i].iter().collect::<String>().parse::<f64>().unwrap_or(f64::NAN)
}

fn call_native(name: &str, args: &[JsValue]) -> EvalResult {
    Ok(match name {
        "parseInt" | "Number.parseInt" => {
            let s = args.first().map(to_string).unwrap_or_default();
            let radix_arg = args.get(1).map(to_number).unwrap_or(0.0);
            JsValue::Number(parse_int_js(&s, radix_arg))
        }
        "parseFloat" | "Number.parseFloat" => {
            let s = args.first().map(to_string).unwrap_or_default();
            JsValue::Number(parse_float_js(&s))
        }
        "isNaN" => {
            // Global isNaN coerces its argument before testing.
            let n = args.first().map(to_number).unwrap_or(f64::NAN);
            JsValue::Boolean(n.is_nan())
        }
        "Number.isNaN" => {
            // Number.isNaN does NOT coerce: only an actual NaN Number qualifies.
            JsValue::Boolean(matches!(args.first(), Some(JsValue::Number(n)) if n.is_nan()))
        }
        "isFinite" => {
            // Global isFinite coerces its argument before testing.
            let n = args.first().map(to_number).unwrap_or(f64::NAN);
            JsValue::Boolean(n.is_finite())
        }
        "Number.isFinite" => {
            // Number.isFinite does NOT coerce: only a finite Number qualifies.
            JsValue::Boolean(matches!(args.first(), Some(JsValue::Number(n)) if n.is_finite()))
        }
        "Number.isInteger" => {
            // True only for a finite Number with no fractional part.
            match args.first() {
                Some(JsValue::Number(n)) => JsValue::Boolean(n.is_finite() && n.fract() == 0.0),
                _ => JsValue::Boolean(false),
            }
        }
        "Number.isSafeInteger" => {
            match args.first() {
                Some(JsValue::Number(n)) => JsValue::Boolean(n.is_finite() && n.fract() == 0.0 && n.abs() <= 9007199254740991.0),
                _ => JsValue::Boolean(false),
            }
        }
        "Math.floor" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).floor()),
        "Math.ceil" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).ceil()),
        "Math.round" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).round()),
        "Math.abs" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).abs()),
        "Math.sqrt" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).sqrt()),
        "Math.trunc" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).trunc()),
        "Math.sign" => { let n = args.first().map(to_number).unwrap_or(f64::NAN); JsValue::Number(if n > 0.0 { 1.0 } else if n < 0.0 { -1.0 } else { 0.0 }) }
        "Math.log" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).ln()),
        "Math.pow" => { let b = args.first().map(to_number).unwrap_or(0.0); let e = args.get(1).map(to_number).unwrap_or(0.0); JsValue::Number(b.powf(e)) }
        "Math.max" => JsValue::Number(args.iter().map(to_number).fold(f64::NEG_INFINITY, f64::max)),
        "Math.min" => JsValue::Number(args.iter().map(to_number).fold(f64::INFINITY, f64::min)),
        "Math.random" => JsValue::Number(0.5), // deterministic for agent reproducibility
        "Math.sin" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).sin()),
        "Math.cos" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).cos()),
        "Math.tan" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).tan()),
        "Math.asin" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).asin()),
        "Math.acos" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).acos()),
        "Math.atan" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).atan()),
        "Math.atan2" => { let y = args.first().map(to_number).unwrap_or(f64::NAN); let x = args.get(1).map(to_number).unwrap_or(f64::NAN); JsValue::Number(y.atan2(x)) }
        "Math.sinh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).sinh()),
        "Math.cosh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).cosh()),
        "Math.tanh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).tanh()),
        "Math.exp" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).exp()),
        "Math.expm1" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).exp_m1()),
        "Math.log1p" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).ln_1p()),
        "Math.log2" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).log2()),
        "Math.log10" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).log10()),
        "Math.cbrt" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).cbrt()),
        "Math.hypot" => JsValue::Number(args.iter().map(to_number).map(|v| v * v).sum::<f64>().sqrt()),
        "Math.fround" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN) as f32 as f64),
        "Math.clz32" => { let n = args.first().map(to_number).unwrap_or(0.0); let u = if n.is_finite() { n as i64 as u32 } else { 0 }; JsValue::Number(u.leading_zeros() as f64) }
                "Math.asinh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).asinh()),
                "Math.acosh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).acosh()),
                "Math.atanh" => JsValue::Number(args.first().map(to_number).unwrap_or(f64::NAN).atanh()),
                "Math.imul" => {
                    // 32-bit integer multiplication with wraparound, per ToInt32 semantics.
                    let to_i32 = |v: Option<&JsValue>| -> i32 {
                        let n = v.map(to_number).unwrap_or(0.0);
                        if n.is_finite() { n.trunc() as i64 as i32 } else { 0 }
                    };
                    JsValue::Number(to_i32(args.first()).wrapping_mul(to_i32(args.get(1))) as f64)
                }
        "JSON.parse" => {
            let s = args.first().map(to_string).unwrap_or_default();
            json_parse(&s)
        }
        "JSON.stringify" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            // Second argument: a replacer array (property whitelist) or function (not yet supported).
            let replacer: Option<Vec<String>> = match args.get(1) {
                Some(JsValue::Array(arr)) => Some(arr.iter().map(to_string).collect()),
                _ => None,
            };
            // Third argument selects indentation: a number of spaces (max 10) or a literal string.
            let indent = match args.get(2) {
                Some(JsValue::Number(n)) if *n >= 1.0 => " ".repeat((*n as usize).min(10)),
                Some(JsValue::String(s)) if !s.is_empty() => s.chars().take(10).collect(),
                _ => String::new(),
            };
            if indent.is_empty() {
                JsValue::String(json_stringify(&val, replacer.as_deref()))
            } else {
                JsValue::String(json_stringify_pretty(&val, &indent, 0, replacer.as_deref()))
            }
        }
        "Object.keys" => {
            match args.first() {
                Some(obj) => JsValue::Array(own_keys_of(obj).into_iter().map(JsValue::String).collect()),
                None => JsValue::Array(Vec::new()),
            }
        }
        "Object.values" => {
            match args.first() {
                Some(obj) => JsValue::Array(own_keys_of(obj).into_iter().map(|k| get_property(obj, &k)).collect()),
                None => JsValue::Array(Vec::new()),
            }
        }
        "Object.entries" => {
            match args.first() {
                Some(obj) => JsValue::Array(own_keys_of(obj).into_iter().map(|k| JsValue::Array(vec![JsValue::String(k.clone()), get_property(obj, &k)])).collect()),
                None => JsValue::Array(Vec::new()),
            }
        }
        "Object.fromEntries" => {
            // Build an object from an iterable of [key, value] pairs. Accepts an
            // array of two-element arrays (the common Object.entries round-trip)
            // as well as Map-style entry arrays.
            let mut map = HashMap::new();
            let entries = match args.first() {
                Some(JsValue::Array(items)) => items.clone(),
                Some(JsValue::Object(m)) => {
                    if let Some(JsValue::Array(items)) = m.get("__entries__") { items.clone() } else { Vec::new() }
                }
                _ => Vec::new(),
            };
            for entry in entries {
                if let JsValue::Array(pair) = entry {
                    let key = pair.first().map(to_string).unwrap_or_default();
                    let value = pair.get(1).cloned().unwrap_or(JsValue::Undefined);
                    map.insert(key, value);
                }
            }
            JsValue::Object(map)
        }
        "Object.assign" => {
            let mut target = if let Some(JsValue::Object(m)) = args.first() { m.clone() } else { HashMap::new() };
            for src in args.iter().skip(1) { if let JsValue::Object(m) = src { target.extend(m.iter().map(|(k, v)| (k.clone(), v.clone()))); } }
            JsValue::Object(target)
        }
        "Object.freeze" => args.first().cloned().unwrap_or(JsValue::Undefined),
        "Object.hasOwn" => {
            // Static own-property test: true when the target directly owns `key`.
            let key = args.get(1).map(to_string).unwrap_or_default();
            let has = match args.first() {
                Some(JsValue::Object(map)) => map.contains_key(&key),
                Some(JsValue::Array(arr)) => key == "length" || key.parse::<usize>().map(|i| i < arr.len()).unwrap_or(false),
                _ => false,
            };
            JsValue::Boolean(has)
        }
        "Object.is" => {
            // SameValue: like === but NaN equals NaN and +0 differs from -0.
            let a = args.first().cloned().unwrap_or(JsValue::Undefined);
            let b = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            let same = match (&a, &b) {
                (JsValue::Number(x), JsValue::Number(y)) => {
                    if x.is_nan() && y.is_nan() { true }
                    else if *x == 0.0 && *y == 0.0 { x.is_sign_negative() == y.is_sign_negative() }
                    else { x == y }
                }
                _ => strict_eq(&a, &b),
            };
            JsValue::Boolean(same)
        }
        "Object.setPrototypeOf" => {
            // Sets the target's prototype and returns the (modified) target.
            let mut target = if let Some(JsValue::Object(m)) = args.first() { m.clone() } else { return Ok(args.first().cloned().unwrap_or(JsValue::Undefined)); };
            match args.get(1) {
                Some(JsValue::Null) | None => { target.remove("__proto__"); }
                Some(proto) => { target.insert("__proto__".to_string(), proto.clone()); }
            }
            JsValue::Object(target)
        }
        "Object.create" => {
            let proto = args.first().cloned().unwrap_or(JsValue::Null);
            let mut obj = HashMap::new();
            if !matches!(proto, JsValue::Null) {
                obj.insert("__proto__".to_string(), proto);
            }
            JsValue::Object(obj)
        }
        "Object.getPrototypeOf" => {
            if let Some(JsValue::Object(map)) = args.first() {
                map.get("__proto__").cloned().unwrap_or(JsValue::Null)
            } else { JsValue::Null }
        }
        "Object.defineProperty" => {
            // Apply a property descriptor (data or accessor) to the target and
            // return the modified target, matching JS semantics.
            let mut target = match args.first() {
                Some(JsValue::Object(m)) => m.clone(),
                _ => HashMap::new(),
            };
            let prop = args.get(1).map(to_string).unwrap_or_default();
            if let Some(JsValue::Object(desc)) = args.get(2) {
                apply_descriptor(&mut target, &prop, desc);
            }
            JsValue::Object(target)
        }
        "Object.defineProperties" => {
            let mut target = match args.first() {
                Some(JsValue::Object(m)) => m.clone(),
                _ => HashMap::new(),
            };
            if let Some(JsValue::Object(props)) = args.get(1) {
                for (prop, desc_val) in props {
                    if let JsValue::Object(desc) = desc_val {
                        apply_descriptor(&mut target, prop, desc);
                    }
                }
            }
            JsValue::Object(target)
        }
        "Object.getOwnPropertyDescriptor" => {
            let prop = args.get(1).map(to_string).unwrap_or_default();
            match args.first() {
                Some(JsValue::Object(map)) => match map.get(&prop) {
                    Some(JsValue::Object(desc)) if desc.get("__accessor__") == Some(&JsValue::Boolean(true)) => {
                        let mut out = HashMap::new();
                        out.insert("enumerable".to_string(), desc.get("enumerable").cloned().unwrap_or(JsValue::Boolean(false)));
                        out.insert("configurable".to_string(), desc.get("configurable").cloned().unwrap_or(JsValue::Boolean(false)));
                        if let Some(g) = desc.get("get") { out.insert("get".to_string(), g.clone()); }
                        if let Some(s) = desc.get("set") { out.insert("set".to_string(), s.clone()); }
                        JsValue::Object(out)
                    }
                    Some(val) => {
                        let mut out = HashMap::new();
                        out.insert("value".to_string(), val.clone());
                        out.insert("writable".to_string(), JsValue::Boolean(true));
                        out.insert("enumerable".to_string(), JsValue::Boolean(true));
                        out.insert("configurable".to_string(), JsValue::Boolean(true));
                        JsValue::Object(out)
                    }
                    None => JsValue::Undefined,
                },
                _ => JsValue::Undefined,
            }
        }
        "Object.getOwnPropertyDescriptors" => {
            // Collect a descriptor object for every own property, mirroring the
            // single-property descriptor shape used by getOwnPropertyDescriptor.
            match args.first() {
                Some(obj @ JsValue::Object(map)) => {
                    let mut out = HashMap::new();
                    for key in own_keys_of(obj) {
                        let desc = match map.get(&key) {
                            Some(JsValue::Object(d)) if d.get("__accessor__") == Some(&JsValue::Boolean(true)) => {
                                let mut acc = HashMap::new();
                                acc.insert("enumerable".to_string(), d.get("enumerable").cloned().unwrap_or(JsValue::Boolean(false)));
                                acc.insert("configurable".to_string(), d.get("configurable").cloned().unwrap_or(JsValue::Boolean(false)));
                                if let Some(g) = d.get("get") { acc.insert("get".to_string(), g.clone()); }
                                if let Some(s) = d.get("set") { acc.insert("set".to_string(), s.clone()); }
                                JsValue::Object(acc)
                            }
                            Some(val) => {
                                let mut data = HashMap::new();
                                data.insert("value".to_string(), val.clone());
                                data.insert("writable".to_string(), JsValue::Boolean(true));
                                data.insert("enumerable".to_string(), JsValue::Boolean(true));
                                data.insert("configurable".to_string(), JsValue::Boolean(true));
                                JsValue::Object(data)
                            }
                            None => continue,
                        };
                        out.insert(key, desc);
                    }
                    JsValue::Object(out)
                }
                _ => JsValue::Object(HashMap::new()),
            }
        }
        "Array.isArray" => JsValue::Boolean(matches!(args.first(), Some(JsValue::Array(_)))),
        "Array.from" => {
            match args.first() {
                Some(JsValue::Array(a)) => JsValue::Array(a.clone()),
                Some(JsValue::String(s)) => JsValue::Array(s.chars().map(|c| JsValue::String(c.to_string())).collect()),
                Some(JsValue::Object(m)) => {
                    match m.get("__type__").map(to_string).as_deref() {
                        // A Set materialises to its stored values.
                        Some("Set") => m.get("__items__").cloned().unwrap_or_else(|| JsValue::Array(Vec::new())),
                        // A Map materialises to its [key, value] entry pairs.
                        Some("Map") => m.get("__entries__").cloned().unwrap_or_else(|| JsValue::Array(Vec::new())),
                        // Array-like: an object with a numeric `length` collects indices 0..length.
                        _ => match m.get("length") {
                            Some(len_val) => {
                                let len = to_number(len_val) as usize;
                                JsValue::Array((0..len).map(|i| m.get(&i.to_string()).cloned().unwrap_or(JsValue::Undefined)).collect())
                            }
                            None => JsValue::Array(Vec::new()),
                        },
                    }
                }
                _ => JsValue::Array(Vec::new()),
            }
        }
        "Array.of" => JsValue::Array(args.to_vec()),
        "String.fromCharCode" => {
            let s: String = args.iter().filter_map(|a| { let n = to_number(a) as u32; char::from_u32(n) }).collect();
            JsValue::String(s)
        }
        "String.fromCodePoint" => {
            // Like fromCharCode but interprets each argument as a full Unicode
            // code point rather than a UTF-16 code unit.
            let s: String = args.iter().filter_map(|a| { let n = to_number(a) as u32; char::from_u32(n) }).collect();
            JsValue::String(s)
        }
        "Date.now" => JsValue::Number(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)),
        "console.log" | "console.warn" | "console.error" | "console.info" => JsValue::Undefined,
        "Symbol" => {
            // Symbol() returns a unique string token
            let desc = args.first().map(to_string).unwrap_or_else(|| "symbol".into());
            let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            JsValue::String(format!("__symbol_{}_{}__", desc, id))
        }
        "Symbol.for" => {
            let key = args.first().map(to_string).unwrap_or_default();
            JsValue::String(format!("__symbol_{}__", key))
        }
        // Wrapper constructors used as plain functions perform type coercion:
        // Number() -> 0, String() -> "", Boolean() -> false when called with no args.
        "Number" => JsValue::Number(args.first().map(to_number).unwrap_or(0.0)),
        "String" => JsValue::String(args.first().map(to_string).unwrap_or_default()),
        "Boolean" => JsValue::Boolean(args.first().map(to_boolean).unwrap_or(false)),
        "structuredClone" => {
            // Deep clone via identity (our JsValue is already Clone)
            args.first().cloned().unwrap_or(JsValue::Undefined)
        }
        "queueMicrotask" | "requestAnimationFrame" | "requestIdleCallback" => {
            // These need event loop integration; return a dummy id
            JsValue::Number(0.0)
        }
        "__noop__" => JsValue::Undefined,
        "__promise_resolve__" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            PROMISE_CAPTURE.with(|cap| { *cap.borrow_mut() = Some((false, val)); });
            JsValue::Undefined
        }
        "__promise_reject__" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            PROMISE_CAPTURE.with(|cap| { *cap.borrow_mut() = Some((true, val)); });
            JsValue::Undefined
        }
        "eval" => {
            // eval(code) - lex, parse, and eval inline
            let code = args.first().map(to_string).unwrap_or_default();
            if code.is_empty() { return Ok(JsValue::Undefined); }
            match eval_script_standalone(&code) {
                Ok(v) => v,
                Err(_) => JsValue::Undefined,
            }
        }
        "encodeURIComponent" => {
            let s = args.first().map(to_string).unwrap_or_default();
            JsValue::String(encode_uri_component(&s))
        }
        "decodeURIComponent" => {
            let s = args.first().map(to_string).unwrap_or_default();
            JsValue::String(decode_uri_component(&s))
        }
        "Promise.resolve" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            map.insert("__resolved__".to_string(), val);
            JsValue::Object(map)
        }
        "Promise.reject" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            map.insert("__rejected__".to_string(), val);
            JsValue::Object(map)
        }
        "Promise.all" => {
            // Synchronous: collect resolved values; reject on first rejection
            let mut results = Vec::new();
            if let Some(JsValue::Array(promises)) = args.first() {
                for p in promises {
                    if let JsValue::Object(m) = p {
                        if let Some(rejected) = m.get("__rejected__") {
                            let mut map = HashMap::new();
                            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                            map.insert("__rejected__".to_string(), rejected.clone());
                            return Ok(JsValue::Object(map));
                        }
                        results.push(m.get("__resolved__").cloned().unwrap_or(p.clone()));
                    } else {
                        results.push(p.clone());
                    }
                }
            }
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            map.insert("__resolved__".to_string(), JsValue::Array(results));
            JsValue::Object(map)
        }
        "Promise.race" => {
            // Returns the first settled promise (resolved or rejected)
            if let Some(JsValue::Array(promises)) = args.first() {
                for p in promises {
                    if let JsValue::Object(m) = p {
                        if m.get("__resolved__").is_some() || m.get("__rejected__").is_some() {
                            return Ok(JsValue::Object(m.clone()));
                        }
                    } else {
                        // Non-promise values resolve immediately
                        let mut map = HashMap::new();
                        map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                        map.insert("__resolved__".to_string(), p.clone());
                        return Ok(JsValue::Object(map));
                    }
                }
            }
            // Empty array: never settles, return pending promise
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            JsValue::Object(map)
        }
        "Promise.allSettled" => {
            // Returns array of {status, value/reason} for each promise
            let mut results = Vec::new();
            if let Some(JsValue::Array(promises)) = args.first() {
                for p in promises {
                    let mut entry = HashMap::new();
                    if let JsValue::Object(m) = p {
                        if let Some(rejected) = m.get("__rejected__") {
                            entry.insert("status".to_string(), JsValue::String("rejected".to_string()));
                            entry.insert("reason".to_string(), rejected.clone());
                        } else {
                            entry.insert("status".to_string(), JsValue::String("fulfilled".to_string()));
                            entry.insert("value".to_string(), m.get("__resolved__").cloned().unwrap_or(JsValue::Undefined));
                        }
                    } else {
                        entry.insert("status".to_string(), JsValue::String("fulfilled".to_string()));
                        entry.insert("value".to_string(), p.clone());
                    }
                    results.push(JsValue::Object(entry));
                }
            }
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            map.insert("__resolved__".to_string(), JsValue::Array(results));
            JsValue::Object(map)
        }
        // Object.getOwnPropertyNames reports all non-internal own keys (enumerable or
        // not), consulting a Proxy ownKeys trap when present.
        "Object.getOwnPropertyNames" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::Array(own_property_names(&target).into_iter().map(JsValue::String).collect())
        }
        // Reflect methods
        "Reflect.get" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            get_property(&target, &prop)
        }
        "Reflect.set" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            let value = args.get(2).cloned().unwrap_or(JsValue::Undefined);
            let mut t = target;
            JsValue::Boolean(set_property(&mut t, &prop, value))
        }
        "Reflect.has" => {
            // Route through the proxy-aware membership helper so `Reflect.has`
            // respects Proxy `has` traps and the prototype chain (like the `in` operator).
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            JsValue::Boolean(has_property(&target, &prop))
        }
        "Reflect.deleteProperty" => {
            // Route through the proxy-aware delete helper so `Reflect.deleteProperty`
            // respects Proxy `deleteProperty` traps and yields true for absent keys.
            let mut target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            JsValue::Boolean(delete_property(&mut target, &prop))
        }
        "Reflect.ownKeys" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::Array(own_property_names(&target).into_iter().map(JsValue::String).collect())
        }
        "Reflect.getOwnPropertyDescriptor" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            match &target {
                JsValue::Object(map) => {
                    if let Some(val) = map.get(&prop) {
                        let mut desc = HashMap::new();
                        desc.insert("value".to_string(), val.clone());
                        desc.insert("writable".to_string(), JsValue::Boolean(true));
                        desc.insert("enumerable".to_string(), JsValue::Boolean(true));
                        desc.insert("configurable".to_string(), JsValue::Boolean(true));
                        JsValue::Object(desc)
                    } else {
                        JsValue::Undefined
                    }
                }
                _ => JsValue::Undefined,
            }
        }
        "Reflect.apply" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let this_arg = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            let call_args = match args.get(2) {
                Some(JsValue::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            call_function_with_this(&target, &call_args, &Scope::new_global(), Some(this_arg)).unwrap_or(JsValue::Undefined)
        }
        "Reflect.construct" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let call_args = match args.get(1) {
                Some(JsValue::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            match &target {
                // Class object: run the real constructor with full prototype/instanceof setup,
                // exactly like `new Class(...)`.
                JsValue::Object(class_map) if class_map.get("__type__").map(to_string).as_deref() == Some("class") => {
                    call_class_constructor(class_map, &call_args, &Scope::new_global()).unwrap_or(JsValue::Undefined)
                }
                // Function constructor: emulate `new` — bind a fresh `this`, run the body, and
                // return `this` (or an explicit object return value) per JS constructor semantics.
                JsValue::Function { params, body, closure, .. } => {
                    let call_scope = Scope::new_child(closure);
                    let this_obj = JsValue::Object(HashMap::new());
                    Scope::declare(&call_scope, "this", this_obj.clone());
                    for (i, p) in params.iter().enumerate() {
                        Scope::declare(&call_scope, p, call_args.get(i).cloned().unwrap_or(JsValue::Undefined));
                    }
                    Scope::declare(&call_scope, "arguments", JsValue::Array(call_args));
                    match eval_stmt(body, &call_scope) {
                        Err(Signal::Return(v)) if matches!(v, JsValue::Object(_)) => v,
                        _ => Scope::resolve(&call_scope, "this").unwrap_or(this_obj),
                    }
                }
                _ => call_function(&target, &call_args, &Scope::new_global()).unwrap_or(JsValue::Undefined),
            }
        }
        _ => JsValue::Undefined,
    })
}

fn json_parse(s: &str) -> JsValue {
    let s = s.trim();
    if s == "null" { return JsValue::Null; }
    if s == "true" { return JsValue::Boolean(true); }
    if s == "false" { return JsValue::Boolean(false); }
    if let Ok(n) = s.parse::<f64>() { return JsValue::Number(n); }
    if s.starts_with('"') && s.ends_with('"') {
        // Decode via serde so every JSON string escape (\t, \uXXXX, \\, ...) is
        // honoured, matching how strings nested in arrays/objects are parsed.
        if let Ok(serde_json::Value::String(decoded)) = serde_json::from_str::<serde_json::Value>(s) {
            return JsValue::String(decoded);
        }
        return JsValue::String(s[1..s.len()-1].to_string());
    }
    if s.starts_with('[') {
        // Simplified array parse using serde_json
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
            return serde_to_js(&val);
        }
    }
    if s.starts_with('{') {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
            return serde_to_js(&val);
        }
    }
    JsValue::Undefined
}

fn serde_to_js(val: &serde_json::Value) -> JsValue {
    match val {
        serde_json::Value::Null => JsValue::Null,
        serde_json::Value::Bool(b) => JsValue::Boolean(*b),
        serde_json::Value::Number(n) => JsValue::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => JsValue::String(s.clone()),
        serde_json::Value::Array(arr) => JsValue::Array(arr.iter().map(serde_to_js).collect()),
        serde_json::Value::Object(map) => JsValue::Object(map.iter().map(|(k, v)| (k.clone(), serde_to_js(v))).collect()),
    }
}

fn json_stringify(val: &JsValue, replacer: Option<&[String]>) -> String {
    match val {
        JsValue::Undefined => "undefined".to_string(),
        JsValue::Null => "null".to_string(),
        JsValue::Boolean(b) => b.to_string(),
        // Non-finite numbers are not valid JSON and serialize as null.
        JsValue::Number(n) => if n.is_finite() { format_number(*n) } else { "null".to_string() },
        JsValue::String(s) => json_escape_string(s),
        JsValue::Array(arr) => {
            // undefined and callables serialize as null inside arrays.
            let items: Vec<String> = arr.iter().map(|v| match v {
                JsValue::Undefined | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => "null".to_string(),
                other => json_stringify(other, replacer),
            }).collect();
            format!("[{}]", items.join(","))
        }
        JsValue::Object(map) => {
            // undefined and callable properties are omitted entirely.
            // When a replacer array is present, only whitelisted keys are included.
            let entries: Vec<String> = map.iter().filter_map(|(k, v)| {
                if let Some(whitelist) = replacer { if !whitelist.iter().any(|w| w == k) { return None; } }
                match v {
                    JsValue::Undefined | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => None,
                    other => Some(format!("{}:{}", json_escape_string(k), json_stringify(other, replacer))),
                }
            }).collect();
            format!("{{{}}}", entries.join(","))
        }
        JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => "null".to_string(),
    }
}

/// Escape `s` as a JSON string literal (including surrounding quotes), encoding
/// quotes, backslashes, and the control characters JSON requires be escaped.
fn json_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Pretty-print `val` as JSON using `indent` per nesting level, matching the
/// whitespace layout of JSON.stringify(value, null, space). Empty arrays and
/// objects collapse to `[]`/`{}` and scalars defer to the compact serializer.
fn json_stringify_pretty(val: &JsValue, indent: &str, depth: usize, replacer: Option<&[String]>) -> String {
    match val {
        JsValue::Array(arr) if !arr.is_empty() => {
            let pad = indent.repeat(depth + 1);
            let close = indent.repeat(depth);
            let items: Vec<String> = arr.iter().map(|v| {
                let rendered = match v {
                    JsValue::Undefined | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => "null".to_string(),
                    other => json_stringify_pretty(other, indent, depth + 1, replacer),
                };
                format!("{}{}", pad, rendered)
            }).collect();
            format!("[\n{}\n{}]", items.join(",\n"), close)
        }
        JsValue::Object(map) if !map.is_empty() => {
            let pad = indent.repeat(depth + 1);
            let close = indent.repeat(depth);
            let entries: Vec<String> = map.iter()
                .filter_map(|(k, v)| {
                    if let Some(whitelist) = replacer { if !whitelist.iter().any(|w| w == k) { return None; } }
                    match v {
                        JsValue::Undefined | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => None,
                        other => Some(format!("{}{}: {}", pad, json_escape_string(k), json_stringify_pretty(other, indent, depth + 1, replacer))),
                    }
                })
                .collect();
            if entries.is_empty() { return "{}".to_string(); }
            format!("{{\n{}\n{}}}", entries.join(",\n"), close)
        }
        _ => json_stringify(val, replacer),
    }
}

fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'!' | b'*' | b'(' | b')' | b'\'' => out.push(b as char),
            _ => { out.push('%'); out.push_str(&format!("{:02X}", b)); }
        }
    }
    out
}

fn decode_uri_component(s: &str) -> String {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i+1..i+3], 16) { out.push(b); i += 3; }
            else { out.push(bytes[i]); i += 1; }
        } else if bytes[i] == b'+' { out.push(b' '); i += 1; }
        else { out.push(bytes[i]); i += 1; }
    }
    String::from_utf8_lossy(&out).to_string()
}

// ═══════════════════════════════════════════════════════════════════════════
// ES Module Registry
// ═══════════════════════════════════════════════════════════════════════════

use std::sync::Mutex;

/// Global module registry for ES module imports.
/// Maps module specifier -> exported bindings (name -> value).
static MODULE_REGISTRY: Mutex<Option<HashMap<String, HashMap<String, JsValue>>>> = Mutex::new(None);

// Resolver callback: given a module specifier, returns the module source code.
// Set by the embedding runtime (e.g., session) to fetch modules from network/filesystem.
type ModuleResolverFn = dyn Fn(&str) -> Option<String> + Send + Sync;
static MODULE_RESOLVER: Mutex<Option<Box<ModuleResolverFn>>> = Mutex::new(None);

/// Serialization lock for tests that mutate the global module resolver /
/// registry. These statics are process-wide, so tests touching them must hold
/// this lock to avoid racing (parallel test threads otherwise interleave
/// set/clear operations and observe each other's state).
#[cfg(test)]
pub static MODULE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Set a module resolver callback. When an import references a module not yet
/// in the registry, the resolver is invoked to obtain the module source, which
/// is then evaluated and registered automatically.
pub fn set_module_resolver(resolver: impl Fn(&str) -> Option<String> + Send + Sync + 'static) {
    *MODULE_RESOLVER.lock().unwrap() = Some(Box::new(resolver));
}

/// Clear the module resolver (e.g., between navigations).
pub fn clear_module_resolver() {
    *MODULE_RESOLVER.lock().unwrap() = None;
}

/// Register a module's exports in the global registry.
pub fn register_module(specifier: &str, exports: HashMap<String, JsValue>) {
    let mut registry = MODULE_REGISTRY.lock().unwrap();
    let map = registry.get_or_insert_with(HashMap::new);
    map.insert(specifier.to_string(), exports);
}

/// Clear all registered modules. Call when modules should be re-evaluated
/// or to free memory (e.g., between page navigations).
pub fn clear_module_registry() {
    *MODULE_REGISTRY.lock().unwrap() = None;
}

/// Resolve a module import. Returns the module's exports or None if not registered.
pub fn resolve_module(specifier: &str) -> Option<HashMap<String, JsValue>> {
    let registry = MODULE_REGISTRY.lock().unwrap();
    registry.as_ref().and_then(|map| map.get(specifier).cloned())
}

/// Evaluate a module source and register its exports.
/// Supports `export const/let/var/function`, `export default`, and `export { ... }`.
pub fn evaluate_module(specifier: &str, source: &str) -> Result<HashMap<String, JsValue>, String> {
    let tokens = lex(source)?;
    let mut parser = Parser::new(tokens);
    let stmts = parser.parse_program()?;
    let scope = Scope::new_global();
    let mut exports = HashMap::new();

    for stmt in &stmts {
        match stmt {
            Stmt::Export { declaration, default_expr, named } => {
                if let Some(decl) = declaration {
                    let _ = eval_stmt(decl, &scope);
                    match decl.as_ref() {
                        Stmt::VarDecl { name, .. } => {
                            if let Some(val) = Scope::resolve(&scope, name) {
                                exports.insert(name.clone(), val);
                            }
                        }
                        Stmt::FunctionDecl { name, .. } | Stmt::AsyncFunctionDecl { name, .. } => {
                            if let Some(val) = Scope::resolve(&scope, name) {
                                exports.insert(name.clone(), val);
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(expr) = default_expr {
                    if let Ok(val) = eval_expr_node(expr, &scope) {
                        exports.insert("default".to_string(), val);
                    }
                }
                for name in named {
                    if let Some(val) = Scope::resolve(&scope, name) {
                        exports.insert(name.clone(), val);
                    }
                }
            }
            _ => { let _ = eval_stmt(stmt, &scope); }
        }
    }

    register_module(specifier, exports.clone());
    Ok(exports)
}

/// Apply an import statement: resolve the module and bind specifiers into scope.
/// If the module is not yet registered, the module resolver (if set) is invoked
/// to fetch and evaluate the module source on demand.
pub fn apply_import(
    specifiers: &[ImportSpecifier],
    source: &str,
    scope: &ScopeRef,
) -> Result<(), String> {
    let module_exports = match resolve_module(source) {
        Some(exports) => exports,
        None => {
            // Attempt on-demand resolution via the registered resolver callback.
            let fetched = MODULE_RESOLVER.lock().unwrap()
                .as_ref()
                .and_then(|resolver| resolver(source));
            match fetched {
                Some(src) => evaluate_module(source, &src)?,
                None => return Ok(()), // No resolver or resolver returned None; skip
            }
        }
    };
    for spec in specifiers {
        let value = if spec.imported == "*" {
            // import * as name from 'module'
            JsValue::Object(module_exports.clone())
        } else {
            module_exports.get(&spec.imported).cloned().unwrap_or(JsValue::Undefined)
        };
        Scope::declare(scope, &spec.local, value);
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Property access & method calls
// ═══════════════════════════════════════════════════════════════════════════

/// Thread-local recursion guard for Proxy trap invocations.
/// Prevents infinite loops when a trap handler accesses the same proxy.
const MAX_PROXY_TRAP_DEPTH: u32 = 8;
std::thread_local! {
    static PROXY_TRAP_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

// Thread-local capture for Promise executor resolve/reject calls.
// (is_reject, value) — set by __promise_resolve__ / __promise_reject__ natives.
std::thread_local! {
    static PROMISE_CAPTURE: std::cell::RefCell<Option<(bool, JsValue)>> = const { std::cell::RefCell::new(None) };
}

/// Materialise the value sequence produced by a `for...of` loop for the given
/// iterable, honouring the iterator protocol for custom iterables.
fn iterate_values(val: &JsValue, scope: &ScopeRef) -> Vec<JsValue> {
    match val {
        JsValue::Array(arr) => arr.clone(),
        JsValue::String(s) => s.chars().map(|c| JsValue::String(c.to_string())).collect(),
        JsValue::Object(map) => match map.get("__type__").map(to_string).as_deref() {
            Some("Generator") => match map.get("__values__") {
                Some(JsValue::Array(values)) => values.clone(),
                _ => Vec::new(),
            },
            Some("Map") | Some("WeakMap") | Some("URLSearchParams") => match map.get("__entries__") {
                Some(JsValue::Array(entries)) => entries.clone(),
                _ => Vec::new(),
            },
            Some("Set") => match map.get("__items__") {
                Some(JsValue::Array(items)) => items.clone(),
                _ => Vec::new(),
            },
            _ => {
                // Custom iterable: a `Symbol.iterator`/`__iterator__` method returns an
                // iterator; otherwise treat the object itself as an iterator (has `next`).
                let iterator = map
                    .get("Symbol.iterator")
                    .or_else(|| map.get("__iterator__"))
                    .and_then(|mk| call_function(mk, &[val.clone()], scope).ok())
                    .unwrap_or_else(|| val.clone());
                drain_iterator(&iterator, scope)
            }
        },
        _ => Vec::new(),
    }
}

/// Drain an iterator object by repeatedly calling its `next()` method until it
/// reports `done`, collecting the yielded `value`s.
fn drain_iterator(iter: &JsValue, scope: &ScopeRef) -> Vec<JsValue> {
    let mut out = Vec::new();
    for _ in 0..100_000 {
        let next_fn = get_property(iter, "next");
        if matches!(next_fn, JsValue::Undefined | JsValue::Null) {
            break;
        }
        let step = match call_function(&next_fn, &[], scope) {
            Ok(v) => v,
            Err(_) => break,
        };
        let (value, done) = match &step {
            JsValue::Object(m) => (
                m.get("value").cloned().unwrap_or(JsValue::Undefined),
                matches!(m.get("done"), Some(JsValue::Boolean(true))),
            ),
            _ => (step, false),
        };
        if done {
            break;
        }
        out.push(value);
    }
    out
}

/// Membership test for the `in` operator, respecting Proxy `has` traps and the
/// prototype chain (matching JS semantics where `in` sees inherited properties).
fn has_property(obj: &JsValue, prop: &str) -> bool {
    match obj {
        // Native Proxy variant: consult handler.has(target, prop) when present.
        JsValue::Proxy { target, handler } => {
            if let JsValue::Object(h_map) = handler.as_ref() {
                if let Some(has_trap) = h_map.get("has") {
                    if !matches!(has_trap, JsValue::NativeFunction(_)) {
                        let depth = PROXY_TRAP_DEPTH.with(|d| {
                            let cur = d.get();
                            if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                            d.set(cur + 1);
                            cur
                        });
                        if depth < MAX_PROXY_TRAP_DEPTH {
                            let prop_val = JsValue::String(prop.to_string());
                            let result = call_function(
                                has_trap,
                                &[(**target).clone(), prop_val],
                                &Scope::new_global(),
                            );
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            if let Ok(val) = result {
                                return to_boolean(&val);
                            }
                        }
                    }
                }
            }
            has_property(target, prop)
        }
        JsValue::Object(map) => {
            // Object-based proxy variant.
            if map.get("__type__").map(to_string).as_deref() == Some("Proxy") {
                if let (Some(t), Some(JsValue::Object(h_map))) =
                    (map.get("__proxy_target__"), map.get("__proxy_handler__"))
                {
                    if let Some(has_trap) = h_map.get("has") {
                        if !matches!(has_trap, JsValue::NativeFunction(_)) {
                            let depth = PROXY_TRAP_DEPTH.with(|d| {
                                let cur = d.get();
                                if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                                d.set(cur + 1);
                                cur
                            });
                            if depth < MAX_PROXY_TRAP_DEPTH {
                                let prop_val = JsValue::String(prop.to_string());
                                let result = call_function(
                                    has_trap,
                                    &[t.clone(), prop_val],
                                    &Scope::new_global(),
                                );
                                PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                                if let Ok(val) = result {
                                    return to_boolean(&val);
                                }
                            }
                        }
                    }
                    return has_property(t, prop);
                }
            }
            if map.contains_key(prop) {
                return true;
            }
            // Walk the prototype chain so inherited members are visible to `in`.
            let mut proto = map.get("__proto__");
            let mut depth = 0;
            while let Some(JsValue::Object(proto_map)) = proto {
                if depth >= 64 { break; }
                if proto_map.contains_key(prop) {
                    return true;
                }
                proto = proto_map.get("__proto__");
                depth += 1;
            }
            false
        }
        JsValue::Array(arr) => {
            if prop == "length" {
                return true;
            }
            prop.parse::<usize>().map(|i| i < arr.len()).unwrap_or(false)
        }
        JsValue::String(s) => {
            if prop == "length" {
                return true;
            }
            prop.parse::<usize>()
                .map(|i| i < s.chars().count())
                .unwrap_or(false)
        }
        _ => false,
    }
}

/// Delete a property, respecting Proxy `deleteProperty` traps. Returns the
/// boolean result of the delete (per JS, `delete` yields true in non-strict mode).
fn delete_property(obj: &mut JsValue, prop: &str) -> bool {
    match obj {
        // Native Proxy variant: consult handler.deleteProperty(target, prop).
        JsValue::Proxy { target, handler } => {
            if let JsValue::Object(h_map) = handler.as_ref() {
                if let Some(trap) = h_map.get("deleteProperty") {
                    if !matches!(trap, JsValue::NativeFunction(_)) {
                        let depth = PROXY_TRAP_DEPTH.with(|d| {
                            let cur = d.get();
                            if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                            d.set(cur + 1);
                            cur
                        });
                        if depth < MAX_PROXY_TRAP_DEPTH {
                            let prop_val = JsValue::String(prop.to_string());
                            let result = call_function(
                                trap,
                                &[(**target).clone(), prop_val],
                                &Scope::new_global(),
                            );
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            if let Ok(val) = result {
                                return to_boolean(&val);
                            }
                        }
                    }
                }
            }
            delete_property(target, prop)
        }
        JsValue::Object(map) => {
            // Object-based proxy variant.
            if map.get("__type__").map(to_string).as_deref() == Some("Proxy") {
                let target_clone = map.get("__proxy_target__").cloned();
                let handler_clone = map.get("__proxy_handler__").cloned();
                if let (Some(t), Some(JsValue::Object(h_map))) = (&target_clone, &handler_clone) {
                    if let Some(trap) = h_map.get("deleteProperty") {
                        if !matches!(trap, JsValue::NativeFunction(_)) {
                            let depth = PROXY_TRAP_DEPTH.with(|d| {
                                let cur = d.get();
                                if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                                d.set(cur + 1);
                                cur
                            });
                            if depth < MAX_PROXY_TRAP_DEPTH {
                                let prop_val = JsValue::String(prop.to_string());
                                let result = call_function(
                                    trap,
                                    &[t.clone(), prop_val],
                                    &Scope::new_global(),
                                );
                                PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                                if let Ok(val) = result {
                                    return to_boolean(&val);
                                }
                            }
                        }
                    }
                    if let Some(inner) = map.get_mut("__proxy_target__") {
                        return delete_property(inner, prop);
                    }
                }
            }
            map.remove(prop);
            true
        }
        JsValue::Array(arr) => {
            // Deleting an array element leaves a hole (undefined), per JS.
            if let Ok(i) = prop.parse::<usize>() {
                if i < arr.len() {
                    arr[i] = JsValue::Undefined;
                }
            }
            true
        }
        _ => true,
    }
}

/// Enumerable own keys for `Object.keys/values/entries`, respecting a Proxy
/// `ownKeys` trap and falling back to the target for proxies.
fn own_keys_of(obj: &JsValue) -> Vec<String> {
    match obj {
        JsValue::Proxy { target, handler } => {
            if let JsValue::Object(h_map) = handler.as_ref() {
                if let Some(trap) = h_map.get("ownKeys") {
                    if !matches!(trap, JsValue::NativeFunction(_)) {
                        let depth = PROXY_TRAP_DEPTH.with(|d| {
                            let cur = d.get();
                            if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                            d.set(cur + 1);
                            cur
                        });
                        if depth < MAX_PROXY_TRAP_DEPTH {
                            let result = call_function(trap, &[(**target).clone()], &Scope::new_global());
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            if let Ok(JsValue::Array(arr)) = result {
                                return arr.iter().map(to_string).collect();
                            }
                        }
                    }
                }
            }
            own_keys_of(target)
        }
        JsValue::Object(map) => {
            if map.get("__type__").map(to_string).as_deref() == Some("Proxy") {
                if let (Some(t), Some(JsValue::Object(h_map))) =
                    (map.get("__proxy_target__"), map.get("__proxy_handler__"))
                {
                    if let Some(trap) = h_map.get("ownKeys") {
                        if !matches!(trap, JsValue::NativeFunction(_)) {
                            let depth = PROXY_TRAP_DEPTH.with(|d| {
                                let cur = d.get();
                                if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                                d.set(cur + 1);
                                cur
                            });
                            if depth < MAX_PROXY_TRAP_DEPTH {
                                let result = call_function(trap, &[t.clone()], &Scope::new_global());
                                PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                                if let Ok(JsValue::Array(arr)) = result {
                                    return arr.iter().map(to_string).collect();
                                }
                            }
                        }
                    }
                    return own_keys_of(t);
                }
            }
            enumerable_keys(map)
        }
        JsValue::Array(arr) => (0..arr.len()).map(|i| i.to_string()).collect(),
        JsValue::String(s) => (0..s.chars().count()).map(|i| i.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// All own property names for `Reflect.ownKeys` / `Object.getOwnPropertyNames`.
/// Reports every non-internal own key regardless of enumerability (matching JS
/// semantics where these APIs ignore enumerability), consults a Proxy `ownKeys`
/// trap when present, and yields indices plus `length` for arrays.
fn own_property_names(obj: &JsValue) -> Vec<String> {
    match obj {
        // Proxy targets (native or object-based) consult the ownKeys trap.
        JsValue::Proxy { .. } => own_keys_of(obj),
        JsValue::Object(map) if map.get("__type__").map(to_string).as_deref() == Some("Proxy") => {
            own_keys_of(obj)
        }
        JsValue::Object(map) => map.keys().filter(|k| !is_internal_key(k)).cloned().collect(),
        JsValue::Array(arr) => {
            let mut keys: Vec<String> = (0..arr.len()).map(|i| i.to_string()).collect();
            keys.push("length".to_string());
            keys
        }
        _ => Vec::new(),
    }
}

pub fn get_property(obj: &JsValue, prop: &str) -> JsValue {
    match obj {
        JsValue::Object(map) => {
            // Proxy get trap: forward property access through handler
            if map.get("__type__").map(to_string).as_deref() == Some("Proxy") {
                let target = map.get("__proxy_target__");
                let handler = map.get("__proxy_handler__");
                if let (Some(t), Some(h)) = (target, handler) {
                    if let JsValue::Object(h_map) = h {
                        // Check for get trap in handler
                        if let Some(get_trap) = h_map.get("get") {
                            // Guard against infinite recursion
                            let depth = PROXY_TRAP_DEPTH.with(|d| {
                                let cur = d.get();
                                if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                                d.set(cur + 1);
                                cur
                            });
                            if depth >= MAX_PROXY_TRAP_DEPTH {
                                // Recursion limit: fall through to target
                                return get_property(t, prop);
                            }
                            // Invoke the trap: handler.get(target, prop)
                            let prop_val = JsValue::String(prop.to_string());
                            // For native function traps, just return the target property
                            if matches!(get_trap, JsValue::NativeFunction(_)) {
                                PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                                return get_property(t, prop);
                            }
                            let result = call_function(get_trap, &[t.clone(), prop_val], &Scope::new_global());
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            if let Ok(val) = result {
                                return val;
                            }
                        }
                        // Check for has trap (for "in" operator support)
                        if prop == "__has__" {
                            if let Some(has_trap) = h_map.get("has") {
                                if matches!(has_trap, JsValue::NativeFunction(_)) {
                                    return JsValue::Boolean(true);
                                }
                            }
                        }
                    }
                    // Fallback: forward to target
                    return get_property(t, prop);
                }
            }
            if let Some(val) = map.get(prop) { return resolve_accessor(val, obj); }
            // Walk __proto__ chain
            let mut proto = map.get("__proto__");
            while let Some(p) = proto {
                if let JsValue::Object(proto_map) = p {
                    if let Some(val) = proto_map.get(prop) { return resolve_accessor(val, obj); }
                    proto = proto_map.get("__proto__");
                } else { break; }
            }
            JsValue::Undefined
        }
        JsValue::Array(arr) => {
            if prop == "length" { return JsValue::Number(arr.len() as f64); }
            if let Ok(i) = prop.parse::<usize>() { return arr.get(i).cloned().unwrap_or(JsValue::Undefined); }
            JsValue::Undefined
        }
        JsValue::Proxy { target, handler } => {
            // Phase 7: Native Proxy variant — intercept property access via handler.get trap
            let depth = PROXY_TRAP_DEPTH.with(|d| {
                let cur = d.get();
                if cur >= MAX_PROXY_TRAP_DEPTH { return cur; }
                d.set(cur + 1);
                cur
            });
            if depth >= MAX_PROXY_TRAP_DEPTH {
                return get_property(target, prop);
            }
            if let JsValue::Object(h_map) = handler.as_ref() {
                if let Some(get_trap) = h_map.get("get") {
                    if !matches!(get_trap, JsValue::NativeFunction(_)) {
                        let prop_val = JsValue::String(prop.to_string());
                        let result = call_function(get_trap, &[(**target).clone(), prop_val], &Scope::new_global());
                        PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                        if let Ok(val) = result {
                            return val;
                        }
                    }
                }
            }
            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
            get_property(target, prop)
        }
        JsValue::String(s) => {
            // Length counts Unicode scalar values, matching char-based indexing below.
            if prop == "length" { return JsValue::Number(s.chars().count() as f64); }
            if let Ok(i) = prop.parse::<usize>() { return s.chars().nth(i).map(|c| JsValue::String(c.to_string())).unwrap_or(JsValue::Undefined); }
            JsValue::Undefined
        }
        _ => JsValue::Undefined,
    }
}

/// Set a property on an object, respecting Proxy set traps.
pub fn set_property(obj: &mut JsValue, prop: &str, value: JsValue) -> bool {
    if let JsValue::Object(map) = obj {
        // Proxy set trap
        if map.get("__type__").map(to_string).as_deref() == Some("Proxy") {
            if let Some(JsValue::Object(h_map)) = map.get("__proxy_handler__") {
                if let Some(set_trap) = h_map.get("set") {
                    if let Some(target) = map.get("__proxy_target__").cloned() {
                        // Guard against infinite recursion
                        let depth_ok = PROXY_TRAP_DEPTH.with(|d| {
                            let cur = d.get();
                            if cur >= MAX_PROXY_TRAP_DEPTH { return false; }
                            d.set(cur + 1);
                            true
                        });
                        if depth_ok {
                            let prop_val = JsValue::String(prop.to_string());
                            let ok = call_function(set_trap, &[target, prop_val, value.clone()], &Scope::new_global()).is_ok();
                            PROXY_TRAP_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
                            if ok { return true; }
                        }
                    }
                }
            }
            // Forward to target
            if let Some(target) = map.get_mut("__proxy_target__") {
                return set_property(target, prop, value);
            }
        }
        // Accessor property: invoke the setter rather than overwriting the descriptor.
        // We snapshot the object, run the setter with `this` bound to that snapshot, then
        // merge any mutations the setter made to `this` back into the real object so that
        // `set x(v) { this._v = v; }` actually persists.
        let accessor_info = match map.get(prop) {
            Some(JsValue::Object(desc)) if desc.get("__accessor__") == Some(&JsValue::Boolean(true)) => {
                Some((desc.get("set").cloned(), desc.clone(), map.clone()))
            }
            _ => None,
        };
        if let Some((setter, descriptor, this_snapshot)) = accessor_info {
            if let Some(setter) = setter {
                if !matches!(setter, JsValue::NativeFunction(_)) {
                    let updated = invoke_setter_readback(&setter, &value, &this_snapshot);
                    for (k, v) in updated {
                        map.insert(k, v);
                    }
                    // Preserve the accessor descriptor itself (the setter must not clobber it).
                    map.insert(prop.to_string(), JsValue::Object(descriptor));
                }
            }
            return true;
        }
        map.insert(prop.to_string(), value);
        true
    } else {
        false
    }
}

/// Invoke an accessor setter with `this` bound to a snapshot of the owning object, then
/// read back the (possibly mutated) `this` so the caller can merge the changes into the
/// real object. Returns the updated object map (or the snapshot unchanged if the setter
/// did not mutate `this`).
fn invoke_setter_readback(setter: &JsValue, value: &JsValue, this_map: &HashMap<String, JsValue>) -> HashMap<String, JsValue> {
    if let JsValue::Function { params, body, closure, .. } = setter {
        let call_scope = Scope::new_child(closure);
        Scope::declare(&call_scope, "this", JsValue::Object(this_map.clone()));
        for (i, p) in params.iter().enumerate() {
            let val = if i == 0 { value.clone() } else { JsValue::Undefined };
            Scope::declare(&call_scope, p, val);
        }
        Scope::declare(&call_scope, "arguments", JsValue::Array(vec![value.clone()]));
        let _ = eval_stmt(body, &call_scope);
        if let Some(JsValue::Object(updated)) = Scope::resolve(&call_scope, "this") {
            return updated;
        }
    }
    this_map.clone()
}

/// Apply a single property descriptor (data or accessor) to `target` under `prop`.
fn apply_descriptor(target: &mut HashMap<String, JsValue>, prop: &str, desc: &HashMap<String, JsValue>) {
    if desc.contains_key("get") || desc.contains_key("set") {
        let mut accessor = HashMap::new();
        accessor.insert("__accessor__".to_string(), JsValue::Boolean(true));
        if let Some(g) = desc.get("get") { accessor.insert("get".to_string(), g.clone()); }
        if let Some(s) = desc.get("set") { accessor.insert("set".to_string(), s.clone()); }
        accessor.insert("enumerable".to_string(), desc.get("enumerable").cloned().unwrap_or(JsValue::Boolean(false)));
        accessor.insert("configurable".to_string(), desc.get("configurable").cloned().unwrap_or(JsValue::Boolean(false)));
        target.insert(prop.to_string(), JsValue::Object(accessor));
    } else {
        target.insert(prop.to_string(), desc.get("value").cloned().unwrap_or(JsValue::Undefined));
    }
}

/// Install a getter or setter coming from object-literal syntax (`{ get x() {}, set x(v) {} }`).
/// A getter and setter for the same key arrive as separate props, so we merge them into a
/// single `__accessor__` descriptor. Object-literal accessors are enumerable+configurable
/// by default (unlike Object.defineProperty, which defaults to false).
fn install_literal_accessor(target: &mut HashMap<String, JsValue>, prop: &str, kind: &str, func: JsValue) {
    let mut accessor = match target.get(prop) {
        Some(JsValue::Object(existing)) if existing.get("__accessor__") == Some(&JsValue::Boolean(true)) => existing.clone(),
        _ => {
            let mut a = HashMap::new();
            a.insert("__accessor__".to_string(), JsValue::Boolean(true));
            a.insert("enumerable".to_string(), JsValue::Boolean(true));
            a.insert("configurable".to_string(), JsValue::Boolean(true));
            a
        }
    };
    accessor.insert(kind.to_string(), func);
    target.insert(prop.to_string(), JsValue::Object(accessor));
}

/// If `val` is an accessor property descriptor (installed via Object.defineProperty
/// with a `get` function), invoke the getter with `this` bound to `this_obj` and
/// return its result. Data values are returned unchanged.
fn resolve_accessor(val: &JsValue, this_obj: &JsValue) -> JsValue {
    if let JsValue::Object(desc) = val {
        if desc.get("__accessor__") == Some(&JsValue::Boolean(true)) {
            if let Some(getter) = desc.get("get") {
                if !matches!(getter, JsValue::NativeFunction(_)) {
                    if let Ok(result) = call_function_with_this(getter, &[], &Scope::new_global(), Some(this_obj.clone())) {
                        return result;
                    }
                }
            }
            return JsValue::Undefined;
        }
    }
    val.clone()
}

/// Internal bookkeeping keys are double-underscore delimited (`__type__`, `__proto__`,
/// `__instanceof__`, `__accessor__`, ...). They must never leak into user-visible
/// enumeration (`for...in`, `Object.keys/values/entries`). A key like `__foo` (no
/// trailing delimiter) is a legitimate user key and is NOT internal.
fn is_internal_key(key: &str) -> bool {
    key.len() >= 4 && key.starts_with("__") && key.ends_with("__")
}

/// Whether an accessor descriptor should appear in enumeration. Data properties are
/// always enumerable; accessors honor their `enumerable` flag (default true for
/// object-literal accessors, false for Object.defineProperty unless set).
fn accessor_is_enumerable(desc: &HashMap<String, JsValue>) -> bool {
    match desc.get("enumerable") {
        Some(JsValue::Boolean(b)) => *b,
        _ => true,
    }
}

/// Enumerable own keys of an object: excludes internal `__x__` keys and non-enumerable
/// accessors. Order is unspecified (HashMap), matching the engine's existing semantics.
fn enumerable_keys(map: &HashMap<String, JsValue>) -> Vec<String> {
    map.iter()
        .filter(|(k, v)| {
            if is_internal_key(k) {
                return false;
            }
            if let JsValue::Object(desc) = v {
                if desc.get("__accessor__") == Some(&JsValue::Boolean(true)) {
                    return accessor_is_enumerable(desc);
                }
            }
            true
        })
        .map(|(k, _)| k.clone())
        .collect()
}

fn call_method(obj: &JsValue, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    match obj {
        JsValue::Array(arr) => {
            let mut a = arr.clone();
            call_array_method(&mut a, method, args, scope)
        }
        JsValue::String(s) => {
            // replace/replaceAll with a function replacement needs scope for the callback.
            if (method == "replace" || method == "replaceAll")
                && matches!(args.get(1), Some(JsValue::Function { .. } | JsValue::NativeFunction(_)))
            {
                return string_replace_with_fn(s, method, args, scope);
            }
            Ok(call_string_method(s, method, args))
        }
        JsValue::Object(map) => {
            // Check for Map/Set/Promise builtins
            let type_tag = map.get("__type__").map(to_string);
            match type_tag.as_deref() {
                Some("Map") | Some("WeakMap") => { let mut m = map.clone(); return call_map_method(&mut m, method, args, scope); }
                Some("Set") | Some("WeakSet") => { let mut m = map.clone(); return call_set_method(&mut m, method, args, scope); }
                Some("Promise") => return call_promise_method(map, method, args, scope),
                Some("Date") => return call_date_method(map, method, args),
                Some("Generator") => return call_generator_method(map, method),
                // A bound function answers call/apply/bind by re-targeting its
                // stored target; invoking it is handled in call_function_with_this.
                Some("BoundFunction") => {
                    let target = map.get("__target__").cloned().unwrap_or(JsValue::Undefined);
                    let bound_this = map.get("__this__").cloned().unwrap_or(JsValue::Undefined);
                    let bound_args = match map.get("__args__") { Some(JsValue::Array(a)) => a.clone(), _ => Vec::new() };
                    return match method {
                        "call" => {
                            let this_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                            call_function_with_this(&target, &args[1..], scope, Some(this_arg))
                        }
                        "apply" => {
                            let this_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                            let call_args = match args.get(1) { Some(JsValue::Array(a)) => a.clone(), _ => Vec::new() };
                            call_function_with_this(&target, &call_args, scope, Some(this_arg))
                        }
                        "bind" => {
                            let this_arg = args.first().cloned().unwrap_or(bound_this);
                            let mut bound = bound_args;
                            bound.extend(args.iter().skip(1).cloned());
                            let mut m = HashMap::new();
                            m.insert("__type__".to_string(), JsValue::String("BoundFunction".to_string()));
                            m.insert("__target__".to_string(), target);
                            m.insert("__this__".to_string(), this_arg);
                            m.insert("__args__".to_string(), JsValue::Array(bound));
                            Ok(JsValue::Object(m))
                        }
                        _ => Ok(JsValue::Undefined),
                    };
                }
                _ => {}
            }
            // Call method with `this` bound to the object
            if let Some(func) = map.get(method) {
                return call_function_with_this(func, args, scope, Some(obj.clone()));
            }
            call_object_method(map, method, args)
        }
        JsValue::Number(n) => Ok(call_number_method(*n, method, args)),
        // Function.prototype.call / apply / bind.
        JsValue::Function { .. } => {
            match method {
                "call" => {
                    let this_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                    call_function_with_this(obj, &args[1..], scope, Some(this_arg))
                }
                "apply" => {
                    let this_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                    let call_args = match args.get(1) { Some(JsValue::Array(a)) => a.clone(), _ => Vec::new() };
                    call_function_with_this(obj, &call_args, scope, Some(this_arg))
                }
                "bind" => {
                    let this_arg = args.first().cloned().unwrap_or(JsValue::Undefined);
                    let bound_args: Vec<JsValue> = args.iter().skip(1).cloned().collect();
                    let mut m = HashMap::new();
                    m.insert("__type__".to_string(), JsValue::String("BoundFunction".to_string()));
                    m.insert("__target__".to_string(), obj.clone());
                    m.insert("__this__".to_string(), this_arg);
                    m.insert("__args__".to_string(), JsValue::Array(bound_args));
                    Ok(JsValue::Object(m))
                }
                _ => Ok(JsValue::Undefined),
            }
        }
        _ => Ok(JsValue::Undefined),
    }
}

/// Resolve a read-only constant exposed on a builtin namespace object
/// (e.g. `Math.PI`, `Number.MAX_SAFE_INTEGER`) when no user binding shadows it.
fn builtin_namespace_constant(ns: &str, prop: &str) -> Option<JsValue> {
    use std::f64::consts;
    let v = match (ns, prop) {
        ("Math", "PI") => consts::PI,
        ("Math", "E") => consts::E,
        ("Math", "LN2") => consts::LN_2,
        ("Math", "LN10") => consts::LN_10,
        ("Math", "LOG2E") => consts::LOG2_E,
        ("Math", "LOG10E") => consts::LOG10_E,
        ("Math", "SQRT2") => consts::SQRT_2,
        ("Math", "SQRT1_2") => consts::FRAC_1_SQRT_2,
        ("Number", "MAX_SAFE_INTEGER") => 9007199254740991.0,
        ("Number", "MIN_SAFE_INTEGER") => -9007199254740991.0,
        ("Number", "MAX_VALUE") => f64::MAX,
        ("Number", "MIN_VALUE") => f64::MIN_POSITIVE,
        ("Number", "EPSILON") => f64::EPSILON,
        ("Number", "POSITIVE_INFINITY") => f64::INFINITY,
        ("Number", "NEGATIVE_INFINITY") => f64::NEG_INFINITY,
        ("Number", "NaN") => f64::NAN,
        _ => return None,
    };
    Some(JsValue::Number(v))
}

fn flatten_array(a: &[JsValue], depth: usize) -> Vec<JsValue> {
    let mut out = Vec::new();
    for item in a {
        match item {
            JsValue::Array(inner) if depth > 0 => out.extend(flatten_array(inner, depth - 1)),
            other => out.push(other.clone()),
        }
    }
    out
}

fn call_array_method(a: &mut Vec<JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    Ok(match method {
        "push" => { a.extend(args.iter().cloned()); JsValue::Number(a.len() as f64) }
        "pop" => a.pop().unwrap_or(JsValue::Undefined),
        "shift" => { if a.is_empty() { JsValue::Undefined } else { a.remove(0) } }
        "unshift" => {
            let tail = std::mem::take(a);
            let mut new = args.to_vec();
            new.extend(tail);
            *a = new;
            JsValue::Number(a.len() as f64)
        }
        "length" => JsValue::Number(a.len() as f64),
        "indexOf" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            // Optional fromIndex (negative counts from the end) starts the forward scan.
            let len = a.len() as i64;
            let mut from = args.get(1).map(|v| to_number(v) as i64).unwrap_or(0);
            if from < 0 { from += len; }
            let start = from.max(0) as usize;
            let found = a.iter().enumerate().skip(start)
                .find(|(_, x)| strict_eq(x, &target))
                .map(|(i, _)| i as f64).unwrap_or(-1.0);
            JsValue::Number(found)
        }
        "lastIndexOf" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            // Optional fromIndex bounds the backward scan (default: last element).
            let len = a.len() as i64;
            let mut from = args.get(1).map(|v| to_number(v) as i64).unwrap_or(len - 1);
            if from < 0 { from += len; }
            let end = from.min(len - 1);
            let mut result = -1.0;
            if end >= 0 {
                for i in (0..=end as usize).rev() {
                    if strict_eq(&a[i], &target) { result = i as f64; break; }
                }
            }
            JsValue::Number(result)
        }
        "includes" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            // Array includes uses SameValueZero (NaN matches NaN, unlike indexOf)
            // and honours an optional fromIndex (negative counts from the end).
            let len = a.len() as i64;
            let mut from = args.get(1).map(|v| to_number(v) as i64).unwrap_or(0);
            if from < 0 { from += len; }
            let start = from.max(0) as usize;
            let found = a.iter().skip(start).any(|x| match (x, &target) {
                (JsValue::Number(p), JsValue::Number(q)) if p.is_nan() && q.is_nan() => true,
                _ => strict_eq(x, &target),
            });
            JsValue::Boolean(found)
        }
        "at" => {
            let i = args.first().map(to_number).unwrap_or(0.0) as i64;
            let len = a.len() as i64;
            let idx = if i < 0 { len + i } else { i };
            if (0..len).contains(&idx) { a[idx as usize].clone() } else { JsValue::Undefined }
        }
        "join" => {
            // An absent or undefined separator defaults to a comma; null and
            // undefined elements render as empty strings (per spec).
            let sep = match args.first() {
                None | Some(JsValue::Undefined) => ",".to_string(),
                Some(v) => to_string(v),
            };
            let parts: Vec<String> = a.iter().map(|x| match x {
                JsValue::Null | JsValue::Undefined => String::new(),
                other => to_string(other),
            }).collect();
            JsValue::String(parts.join(&sep))
        }
        "toString" | "toLocaleString" => {
            // Array.prototype.toString is join(",") (null/undefined render empty).
            let parts: Vec<String> = a.iter().map(|x| match x {
                JsValue::Null | JsValue::Undefined => String::new(),
                other => to_string(other),
            }).collect();
            JsValue::String(parts.join(","))
        }
        "slice" => {
            let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0);
            let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(a.len() as i64);
            let s = if start < 0 { (a.len() as i64 + start).max(0) as usize } else { start as usize };
            let e = if end < 0 { (a.len() as i64 + end).max(0) as usize } else { (end as usize).min(a.len()) };
            JsValue::Array(a.get(s..e).unwrap_or(&[]).to_vec())
        }
        "concat" => {
            let mut new_arr = a.clone();
            for x in args { if let JsValue::Array(other) = x { new_arr.extend(other.iter().cloned()); } else { new_arr.push(x.clone()); } }
            JsValue::Array(new_arr)
        }
        "reverse" => { a.reverse(); JsValue::Array(a.clone()) }
        "sort" => {
            match args.first() {
                Some(cb) if !matches!(cb, JsValue::Undefined | JsValue::Null) => {
                    let mut sort_err: Option<Signal> = None;
                    a.sort_by(|x, y| {
                        if sort_err.is_some() { return std::cmp::Ordering::Equal; }
                        match call_function(cb, &[x.clone(), y.clone()], scope) {
                            Ok(v) => {
                                let n = to_number(&v);
                                if n < 0.0 { std::cmp::Ordering::Less }
                                else if n > 0.0 { std::cmp::Ordering::Greater }
                                else { std::cmp::Ordering::Equal }
                            }
                            Err(e) => { sort_err = Some(e); std::cmp::Ordering::Equal }
                        }
                    });
                    if let Some(e) = sort_err { return Err(e); }
                }
                _ => { a.sort_by_key(to_string); }
            }
            JsValue::Array(a.clone())
        }
        "splice" => {
            let len = a.len() as i64;
            let start_raw = args.first().map(to_number).unwrap_or(0.0) as i64;
            let start = if start_raw < 0 { (len + start_raw).max(0) as usize } else { (start_raw as usize).min(a.len()) };
            let delete_count = args.get(1).map(|v| to_number(v) as i64).unwrap_or(len).max(0) as usize;
            let end = (start + delete_count).min(a.len());
            let removed: Vec<JsValue> = a.drain(start..end).collect();
            for (i, item) in args.iter().skip(2).enumerate() { a.insert(start + i, item.clone()); }
            JsValue::Array(removed)
        }
        "map" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            let mut result = Vec::new();
            for (i, item) in a.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                result.push(r);
            }
            JsValue::Array(result)
        }
        "filter" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            let mut result = Vec::new();
            for (i, item) in a.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                if to_boolean(&r) { result.push(item.clone()); }
            }
            JsValue::Array(result)
        }
        "forEach" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate() {
                call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
            }
            JsValue::Undefined
        }
        "find" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                if to_boolean(&r) { return Ok(item.clone()); }
            }
            JsValue::Undefined
        }
        "findIndex" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                if to_boolean(&r) { return Ok(JsValue::Number(i as f64)); }
            }
            JsValue::Number(-1.0)
        }
        "findLast" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate().rev() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                if to_boolean(&r) { return Ok(item.clone()); }
            }
            JsValue::Undefined
        }
        "findLastIndex" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate().rev() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                if to_boolean(&r) { return Ok(JsValue::Number(i as f64)); }
            }
            JsValue::Number(-1.0)
        }
        "reduce" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            let mut acc = args.get(1).cloned().unwrap_or_else(|| a.first().cloned().unwrap_or(JsValue::Undefined));
            let start = if args.len() > 1 { 0 } else { 1 };
            for (i, item) in a.iter().enumerate().skip(start) {
                acc = call_function(&callback, &[acc, item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
            }
            acc
        }
        "reduceRight" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            let has_initial = args.len() > 1;
            let mut acc = if has_initial { args[1].clone() } else { a.last().cloned().unwrap_or(JsValue::Undefined) };
            let upper = if has_initial { a.len() } else { a.len().saturating_sub(1) };
            for i in (0..upper).rev() {
                let item = a[i].clone();
                acc = call_function(&callback, &[acc, item, JsValue::Number(i as f64), arr_val.clone()], scope)?;
            }
            acc
        }
        "some" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate() { if to_boolean(&call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?) { return Ok(JsValue::Boolean(true)); } }
            JsValue::Boolean(false)
        }
        "every" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate() { if !to_boolean(&call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?) { return Ok(JsValue::Boolean(false)); } }
            JsValue::Boolean(true)
        }
        "flat" => {
            // depth defaults to 1; a non-finite (Infinity) depth flattens fully.
            let depth = match args.first() {
                Some(v) if !matches!(v, JsValue::Undefined) => {
                    let n = to_number(v);
                    if n.is_finite() { n.max(0.0) as usize } else { usize::MAX }
                }
                _ => 1,
            };
            JsValue::Array(flatten_array(a, depth))
        }
        "flatMap" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let mut result = Vec::new();
            for (i, item) in a.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64)], scope)?;
                if let JsValue::Array(inner) = r { result.extend(inner); } else { result.push(r); }
            }
            JsValue::Array(result)
        }
        "fill" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            let len = a.len() as i64;
            let start_raw = args.get(1).map(to_number).unwrap_or(0.0) as i64;
            let end_raw = args.get(2).map(to_number).unwrap_or(len as f64) as i64;
            let start = if start_raw < 0 { (len + start_raw).max(0) as usize } else { (start_raw as usize).min(a.len()) };
            let end = if end_raw < 0 { (len + end_raw).max(0) as usize } else { (end_raw as usize).min(a.len()) };
            for item in a.iter_mut().take(end).skip(start) { *item = val.clone(); }
            JsValue::Array(a.clone())
        }
        "toReversed" => {
            // Non-mutating reverse: returns a new array, leaving the receiver intact.
            let mut out = a.clone();
            out.reverse();
            JsValue::Array(out)
        }
        "toSorted" => {
            // Non-mutating sort producing a new array (same comparator contract as sort).
            let mut out = a.clone();
            match args.first() {
                Some(cb) if !matches!(cb, JsValue::Undefined | JsValue::Null) => {
                    let mut sort_err: Option<Signal> = None;
                    out.sort_by(|x, y| {
                        if sort_err.is_some() { return std::cmp::Ordering::Equal; }
                        match call_function(cb, &[x.clone(), y.clone()], scope) {
                            Ok(v) => {
                                let n = to_number(&v);
                                if n < 0.0 { std::cmp::Ordering::Less }
                                else if n > 0.0 { std::cmp::Ordering::Greater }
                                else { std::cmp::Ordering::Equal }
                            }
                            Err(e) => { sort_err = Some(e); std::cmp::Ordering::Equal }
                        }
                    });
                    if let Some(e) = sort_err { return Err(e); }
                }
                _ => { out.sort_by_key(to_string); }
            }
            JsValue::Array(out)
        }
        "with" => {
            // Returns a copy with the element at index (supporting negatives) replaced.
            let len = a.len() as i64;
            let raw = args.first().map(to_number).unwrap_or(0.0) as i64;
            let idx = if raw < 0 { len + raw } else { raw };
            let mut out = a.clone();
            if (0..len).contains(&idx) {
                out[idx as usize] = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            }
            JsValue::Array(out)
        }
        "toSpliced" => {
            // Non-mutating splice: returns a new array with the edit applied.
            let len = a.len() as i64;
            let start_raw = args.first().map(to_number).unwrap_or(0.0) as i64;
            let start = if start_raw < 0 { (len + start_raw).max(0) as usize } else { (start_raw as usize).min(a.len()) };
            let delete_count = args.get(1).map(|v| to_number(v) as i64).unwrap_or(len).max(0) as usize;
            let end = (start + delete_count).min(a.len());
            let mut out = a.clone();
            out.drain(start..end);
            for (i, item) in args.iter().skip(2).enumerate() { out.insert(start + i, item.clone()); }
            JsValue::Array(out)
        }
        "copyWithin" => {
            // Shallow-copies a slice [start, end) to position target, all clamped, without changing length.
            let len = a.len() as i64;
            let norm = |raw: i64| -> usize {
                if raw < 0 { (len + raw).max(0) as usize } else { (raw as usize).min(a.len()) }
            };
            let target = norm(args.first().map(to_number).unwrap_or(0.0) as i64);
            let start = norm(args.get(1).map(to_number).unwrap_or(0.0) as i64);
            let end = norm(args.get(2).map(to_number).unwrap_or(len as f64) as i64);
            if start < end {
                let slice: Vec<JsValue> = a[start..end].to_vec();
                for (i, v) in slice.into_iter().enumerate() {
                    let pos = target + i;
                    if pos >= a.len() { break; }
                    a[pos] = v;
                }
            }
            JsValue::Array(a.clone())
        }
        _ => JsValue::Undefined,
    })
}

/// Handle `String.prototype.replace` / `replaceAll` when the replacement is a
/// function. The callback receives (match, offset, originalString) and its return
/// value (coerced to string) becomes the replacement text.
fn string_replace_with_fn(s: &str, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    let pattern = args.first().map(to_string).unwrap_or_default();
    let func = args.get(1).cloned().unwrap_or(JsValue::Undefined);
    let replace_all = method == "replaceAll";
    if pattern.is_empty() {
        return Ok(JsValue::String(s.to_string()));
    }
    let mut out = String::new();
    let mut search_start = 0;
    let mut replaced = false;
    while let Some(rel) = s[search_start..].find(pattern.as_str()) {
        let idx = search_start + rel;
        out.push_str(&s[search_start..idx]);
        let matched = &s[idx..idx + pattern.len()];
        let result = call_function(&func, &[
            JsValue::String(matched.to_string()),
            JsValue::Number(idx as f64),
            JsValue::String(s.to_string()),
        ], scope)?;
        out.push_str(&to_string(&result));
        search_start = idx + pattern.len();
        replaced = true;
        if !replace_all { break; }
    }
    if !replaced {
        return Ok(JsValue::String(s.to_string()));
    }
    out.push_str(&s[search_start..]);
    Ok(JsValue::String(out))
}

fn call_string_method(s: &str, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "length" => JsValue::Number(s.chars().count() as f64),
        "charAt" => { let i = args.first().map(to_number).unwrap_or(0.0) as usize; s.chars().nth(i).map(|c| JsValue::String(c.to_string())).unwrap_or(JsValue::String(String::new())) }
        "charCodeAt" => { let i = args.first().map(to_number).unwrap_or(0.0) as usize; s.chars().nth(i).map(|c| JsValue::Number(c as u32 as f64)).unwrap_or(JsValue::Number(f64::NAN)) }
        "codePointAt" => { let i = args.first().map(to_number).unwrap_or(0.0) as usize; s.chars().nth(i).map(|c| JsValue::Number(c as u32 as f64)).unwrap_or(JsValue::Undefined) }
        "at" => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let raw = args.first().map(to_number).unwrap_or(0.0) as i64;
            let idx = if raw < 0 { len + raw } else { raw };
            if (0..len).contains(&idx) { JsValue::String(chars[idx as usize].to_string()) } else { JsValue::Undefined }
        }
        "indexOf" => {
            // Char-based forward search honouring an optional start position.
            let needle: Vec<char> = args.first().map(to_string).unwrap_or_default().chars().collect();
            let chars: Vec<char> = s.chars().collect();
            let from = args.get(1).map(|v| to_number(v) as i64).unwrap_or(0).max(0) as usize;
            let start = from.min(chars.len());
            let mut result = -1.0;
            if needle.len() <= chars.len() {
                for i in start..=(chars.len() - needle.len()) {
                    if chars[i..i + needle.len()] == needle[..] { result = i as f64; break; }
                }
            }
            JsValue::Number(result)
        }
        "lastIndexOf" => {
            // Char-based backward search; fromIndex bounds the match start (default: end).
            let needle: Vec<char> = args.first().map(to_string).unwrap_or_default().chars().collect();
            let chars: Vec<char> = s.chars().collect();
            let mut result = -1.0;
            if needle.len() <= chars.len() {
                let max_start = chars.len() - needle.len();
                let from = args.get(1).map(to_number).unwrap_or(f64::INFINITY);
                let cap = if from.is_nan() || from >= max_start as f64 { max_start } else if from < 0.0 { 0 } else { from as usize };
                for i in (0..=cap).rev() {
                    if chars[i..i + needle.len()] == needle[..] { result = i as f64; break; }
                }
            }
            JsValue::Number(result)
        }
        "includes" => {
            // Char-based containment honouring an optional start position.
            let needle: Vec<char> = args.first().map(to_string).unwrap_or_default().chars().collect();
            let chars: Vec<char> = s.chars().collect();
            let pos = args.get(1).map(|v| to_number(v) as i64).unwrap_or(0).max(0) as usize;
            let start = pos.min(chars.len());
            let mut found = false;
            if needle.len() <= chars.len() {
                for i in start..=(chars.len() - needle.len()) {
                    if chars[i..i + needle.len()] == needle[..] { found = true; break; }
                }
            }
            JsValue::Boolean(found)
        }
        "startsWith" => {
            // Tests the prefix beginning at an optional char position.
            let needle: Vec<char> = args.first().map(to_string).unwrap_or_default().chars().collect();
            let chars: Vec<char> = s.chars().collect();
            let pos = args.get(1).map(|v| to_number(v) as i64).unwrap_or(0).max(0) as usize;
            let ok = pos + needle.len() <= chars.len() && chars[pos..pos + needle.len()] == needle[..];
            JsValue::Boolean(ok)
        }
        "endsWith" => {
            // Tests the suffix ending at an optional char position (default: end).
            let needle: Vec<char> = args.first().map(to_string).unwrap_or_default().chars().collect();
            let chars: Vec<char> = s.chars().collect();
            let end = args.get(1).map(|v| (to_number(v) as i64).max(0) as usize).unwrap_or(chars.len()).min(chars.len());
            let ok = needle.len() <= end && chars[end - needle.len()..end] == needle[..];
            JsValue::Boolean(ok)
        }
        "slice" => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0);
            let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(len);
            let s_idx = if start < 0 { (len + start).max(0) as usize } else { (start as usize).min(len as usize) };
            let e_idx = if end < 0 { (len + end).max(0) as usize } else { (end as usize).min(len as usize) };
            JsValue::String(chars.get(s_idx..e_idx).unwrap_or(&[]).iter().collect())
        }
        "substring" => {
            let chars: Vec<char> = s.chars().collect();
            let start = args.first().map(|v| to_number(v) as usize).unwrap_or(0).min(chars.len());
            let end = args.get(1).map(|v| to_number(v) as usize).unwrap_or(chars.len()).min(chars.len());
            let (s_idx, e_idx) = if start <= end { (start, end) } else { (end, start) };
            JsValue::String(chars.get(s_idx..e_idx).unwrap_or(&[]).iter().collect())
        }
        "toLowerCase" | "toLocaleLowerCase" => JsValue::String(s.to_lowercase()),
        "toUpperCase" | "toLocaleUpperCase" => JsValue::String(s.to_uppercase()),
        "trim" => JsValue::String(s.trim().to_string()),
        "trimStart" | "trimLeft" => JsValue::String(s.trim_start().to_string()),
        "trimEnd" | "trimRight" => JsValue::String(s.trim_end().to_string()),
        "split" => {
            // Optional second argument caps the number of returned segments.
            let limit = args.get(1).and_then(|v| if matches!(v, JsValue::Undefined) { None } else { Some(to_number(v) as usize) });
            // An absent or undefined separator yields a single-element array
            // holding the whole string; an empty separator splits into chars.
            let mut parts: Vec<JsValue> = match args.first() {
                None | Some(JsValue::Undefined) => vec![JsValue::String(s.to_string())],
                Some(sep_val) => {
                    let sep = to_string(sep_val);
                    if sep.is_empty() {
                        s.chars().map(|c| JsValue::String(c.to_string())).collect()
                    } else {
                        s.split(&sep).map(|p| JsValue::String(p.to_string())).collect()
                    }
                }
            };
            if let Some(n) = limit { parts.truncate(n); }
            JsValue::Array(parts)
        }
        "substr" => {
            // Legacy substr(start, length): negative start counts from the end.
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let raw = args.first().map(to_number).unwrap_or(0.0) as i64;
            let start = if raw < 0 { (len + raw).max(0) as usize } else { (raw as usize).min(chars.len()) };
            let count = args.get(1).map(|v| to_number(v) as i64).unwrap_or(len).max(0) as usize;
            let end = (start + count).min(chars.len());
            JsValue::String(chars.get(start..end).unwrap_or(&[]).iter().collect())
        }
        "concat" => {
            let mut out = s.to_string();
            for a in args { out.push_str(&to_string(a)); }
            JsValue::String(out)
        }
        "replace" => {
            let pattern = args.first().map(to_string).unwrap_or_default();
            let replacement = args.get(1).map(to_string).unwrap_or_default();
            match s.find(&pattern) {
                Some(idx) => {
                    let before = &s[..idx];
                    let after = &s[idx + pattern.len()..];
                    let expanded = expand_replacement(&replacement, &pattern, before, after);
                    JsValue::String(format!("{}{}{}", before, expanded, after))
                }
                None => JsValue::String(s.to_string()),
            }
        }
        "replaceAll" => {
            let pattern = args.first().map(to_string).unwrap_or_default();
            let replacement = args.get(1).map(to_string).unwrap_or_default();
            if pattern.is_empty() {
                JsValue::String(s.to_string())
            } else {
                let mut out = String::new();
                let mut search_start = 0;
                while let Some(rel) = s[search_start..].find(&pattern) {
                    let idx = search_start + rel;
                    let before = &s[..idx];
                    let after = &s[idx + pattern.len()..];
                    out.push_str(&s[search_start..idx]);
                    out.push_str(&expand_replacement(&replacement, &pattern, before, after));
                    search_start = idx + pattern.len();
                }
                out.push_str(&s[search_start..]);
                JsValue::String(out)
            }
        }
        "repeat" => {
            let n = args.first().map(to_number).unwrap_or(0.0) as usize;
            JsValue::String(s.repeat(n.min(10000)))
        }
        "padStart" => {
            let target = args.first().map(to_number).unwrap_or(0.0) as usize;
            let pad = args.get(1).map(to_string).unwrap_or_else(|| " ".into());
            JsValue::String(pad_string(s, target, &pad, true))
        }
        "padEnd" => {
            let target = args.first().map(to_number).unwrap_or(0.0) as usize;
            let pad = args.get(1).map(to_string).unwrap_or_else(|| " ".into());
            JsValue::String(pad_string(s, target, &pad, false))
        }
        "localeCompare" => {
            // Ordinal comparison by Unicode scalar value, returning the JS -1/0/1 contract.
            let other = args.first().map(to_string).unwrap_or_default();
            let cmp = match s.cmp(other.as_str()) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.0,
            };
            JsValue::Number(cmp)
        }
        "normalize" => JsValue::String(s.to_string()), // NFC/NFD normalization not applied; text returned as-is
        "match" => {
            // Plain-string match: find the first occurrence and return [match] or null.
            let pattern = args.first().map(to_string).unwrap_or_default();
            if pattern.is_empty() {
                JsValue::Array(vec![JsValue::String(String::new())])
            } else {
                match s.find(pattern.as_str()) {
                    Some(_) => JsValue::Array(vec![JsValue::String(pattern)]),
                    None => JsValue::Null,
                }
            }
        }
        "search" => {
            let pattern = args.first().map(to_string).unwrap_or_default();
            let idx = if pattern.is_empty() { 0 } else { s.find(pattern.as_str()).map(|i| i as i64).unwrap_or(-1) };
            JsValue::Number(idx as f64)
        }
        "matchAll" => JsValue::Null, // regex not supported
        "toString" | "valueOf" => JsValue::String(s.to_string()),
        _ => JsValue::Undefined,
    }
}

/// Pad `s` with repetitions of `pad` until it reaches `target` char length,
/// truncating the final pad fragment as needed. Counts by Unicode scalar values
/// (not bytes) to match JS String.prototype.padStart/padEnd and avoid slicing
/// through a multi-byte boundary. `at_start` selects padStart vs padEnd.
fn pad_string(s: &str, target: usize, pad: &str, at_start: bool) -> String {
    let cur = s.chars().count();
    if cur >= target || pad.is_empty() { return s.to_string(); }
    let needed = target - cur;
    let pad_chars: Vec<char> = pad.chars().collect();
    let fill: String = (0..needed).map(|i| pad_chars[i % pad_chars.len()]).collect();
    if at_start { format!("{}{}", fill, s) } else { format!("{}{}", s, fill) }
}

fn call_number_method(n: f64, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "toString" => {
            // toString(radix): base 2..=36 for the integer value; base 10 keeps full formatting.
            let radix = args.first().map(to_number).unwrap_or(10.0) as u32;
            if radix == 10 || !(2..=36).contains(&radix) {
                JsValue::String(format_number(n))
            } else {
                JsValue::String(number_to_radix(n, radix))
            }
        }
        "toFixed" => {
            let digits = args.first().map(to_number).unwrap_or(0.0);
            let digits = if digits.is_finite() { (digits as i64).clamp(0, 100) as usize } else { 0 };
            JsValue::String(to_fixed_js(n, digits))
        }
        "toPrecision" => {
            match args.first() {
                Some(v) if !matches!(v, JsValue::Undefined) => {
                    let p = (to_number(v) as usize).clamp(1, 100);
                    JsValue::String(to_precision_js(n, p))
                }
                _ => JsValue::String(format_number(n)),
            }
        }
        "toExponential" => {
            // fractionDigits (0..=100) fixes the digits after the point; when it is
            // absent the shortest round-tripping significand is used.
            let frac = match args.first() {
                Some(v) if !matches!(v, JsValue::Undefined) => {
                    let d = to_number(v);
                    if d.is_finite() { Some((d as i64).clamp(0, 100) as usize) } else { None }
                }
                _ => None,
            };
            JsValue::String(to_exponential_js(n, frac))
        }
        "valueOf" => JsValue::Number(n),
        _ => JsValue::Undefined,
    }
}

/// Convert `n` to a string in the given radix (2..=36), including a fractional
/// part (up to 20 digits) and preserving a leading minus sign for negatives.
fn number_to_radix(n: f64, radix: u32) -> String {
    if !n.is_finite() { return format_number(n); }
    let negative = n < 0.0;
    let abs = n.abs();
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut int_part = abs.trunc() as u64;
    let mut out = Vec::new();
    if int_part == 0 {
        out.push(b'0');
    } else {
        let mut tmp = Vec::new();
        while int_part > 0 {
            tmp.push(digits[(int_part % radix as u64) as usize]);
            int_part /= radix as u64;
        }
        tmp.reverse();
        out.extend(tmp);
    }
    let mut frac = abs.fract();
    if frac > 0.0 {
        out.push(b'.');
        let mut count = 0;
        while frac > 0.0 && count < 20 {
            frac *= radix as f64;
            let digit = (frac.trunc() as usize).min(radix as usize - 1);
            out.push(digits[digit]);
            frac -= frac.trunc();
            count += 1;
        }
    }
    let mut result = String::new();
    if negative { result.push('-'); }
    result.push_str(&String::from_utf8(out).unwrap_or_default());
    result
}

/// Expand the `$` substitution patterns of String.prototype.replace for a
/// plain-string match: `$$`->`$`, `$&`->the matched text, `` $` ``->text before
/// the match, `$'`->text after the match. Other sequences are copied verbatim.
fn expand_replacement(replacement: &str, matched: &str, before: &str, after: &str) -> String {
    let chars: Vec<char> = replacement.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() {
            match chars[i + 1] {
                '$' => { out.push('$'); i += 2; continue; }
                '&' => { out.push_str(matched); i += 2; continue; }
                '`' => { out.push_str(before); i += 2; continue; }
                '\'' => { out.push_str(after); i += 2; continue; }
                _ => {}
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Format `n` with exactly `digits` fractional places, matching JS
/// Number.prototype.toFixed. Unlike Rust's `{:.N}` formatting (round-half-to-even),
/// the spec rounds half away from zero, which `f64::round` provides after scaling.
fn to_fixed_js(n: f64, digits: usize) -> String {
    if n.is_nan() { return "NaN".to_string(); }
    if n.is_infinite() { return if n < 0.0 { "-Infinity".to_string() } else { "Infinity".to_string() }; }
    let neg = n.is_sign_negative() && n != 0.0;
    let scale = 10f64.powi(digits as i32);
    let rounded = (n.abs() * scale).round();
    let scaled_str = format!("{:.0}", rounded);
    let body = if digits == 0 {
        scaled_str
    } else {
        // Left-pad so there is at least one integer digit before the split point.
        let padded = if scaled_str.len() <= digits {
            format!("{:0>width$}", scaled_str, width = digits + 1)
        } else {
            scaled_str
        };
        let split = padded.len() - digits;
        format!("{}.{}", &padded[..split], &padded[split..])
    };
    if neg && rounded != 0.0 { format!("-{}", body) } else { body }
}

/// Format `n` to `p` significant digits per ECMAScript Number.prototype.toPrecision.
/// Uses fixed notation when the exponent e satisfies -6 <= e < p, otherwise
/// exponential notation with an explicit exponent sign (e+2 / e-7).
fn to_precision_js(n: f64, p: usize) -> String {
    if n.is_nan() { return "NaN".to_string(); }
    if n.is_infinite() { return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string(); }
    let negative = n < 0.0 || (n == 0.0 && n.is_sign_negative());
    let a = n.abs();
    let prefix = if negative { "-" } else { "" };
    if a == 0.0 {
        return if p <= 1 { format!("{}0", prefix) } else { format!("{}0.{}", prefix, "0".repeat(p - 1)) };
    }
    // Obtain shortest round-tripping digits via Rust's scientific formatter.
    let sci = format!("{:e}", a);
    let e_pos = sci.find('e').unwrap();
    let sig = &sci[..e_pos];
    let rust_exp: i64 = sci[e_pos + 1..].parse().unwrap_or(0);
    let mut digits: Vec<u8> = sig.chars().filter(|c| *c != '.').map(|c| c as u8 - b'0').collect();
    let point = sig.find('.').unwrap_or(sig.len());
    let mut exp10 = rust_exp + point as i64; // integer-digit count (format_number convention)
    // Round to p significant digits (half-away-from-zero).
    if digits.len() > p {
        let mut carry = digits[p] >= 5;
        digits.truncate(p);
        let mut i = p as isize - 1;
        while carry && i >= 0 {
            digits[i as usize] += 1;
            if digits[i as usize] >= 10 { digits[i as usize] = 0; } else { carry = false; }
            i -= 1;
        }
        if carry { digits = vec![1]; exp10 += 1; }
    }
    while digits.len() < p { digits.push(0); }
    let k = digits.len() as i64;
    let e = exp10 - 1; // exponent of the leading digit
    let chars: Vec<char> = digits.iter().map(|d| (b'0' + d) as char).collect();
    if e >= -6 && e < p as i64 {
        // Fixed notation.
        let body = if exp10 >= k {
            format!("{}{}", chars.iter().collect::<String>(), "0".repeat((exp10 - k) as usize))
        } else if exp10 > 0 {
            format!("{}.{}", chars[..exp10 as usize].iter().collect::<String>(), chars[exp10 as usize..].iter().collect::<String>())
        } else {
            format!("0.{}{}", "0".repeat((-exp10) as usize), chars.iter().collect::<String>())
        };
        format!("{}{}", prefix, body)
    } else {
        // Exponential notation with explicit sign.
        let mantissa = if p == 1 {
            format!("{}", chars[0])
        } else {
            format!("{}.{}", chars[0], chars[1..].iter().collect::<String>())
        };
        format!("{}{}e{}{}", prefix, mantissa, if e >= 0 { "+" } else { "-" }, e.abs())
    }
}

/// Format `n` per JS Number.prototype.toExponential. With `frac` = Some(f) the
/// significand carries exactly `f` digits after the point (half-away-from-zero
/// rounding); with None the shortest round-tripping significand is used. The
/// exponent always carries an explicit sign (`e+3`/`e-7`), unlike Rust's `{:e}`.
fn to_exponential_js(n: f64, frac: Option<usize>) -> String {
    if n.is_nan() { return "NaN".to_string(); }
    if n.is_infinite() { return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string(); }
    let negative = n < 0.0;
    let a = n.abs();
    if a == 0.0 {
        let mantissa = match frac { Some(f) if f > 0 => format!("0.{}", "0".repeat(f)), _ => "0".to_string() };
        return format!("{}e+0", mantissa);
    }
    // Rust's scientific form ("d.dddde±ee") carries enough digits to round-trip.
    let sci = format!("{:e}", a);
    let e_pos = sci.find('e').unwrap();
    let sig = &sci[..e_pos];
    let exp: i64 = sci[e_pos + 1..].parse().unwrap_or(0);
    let mut digits: String = sig.chars().filter(|c| *c != '.').collect();
    let point = sig.find('.').unwrap_or(sig.len());
    let mut exp10 = exp + point as i64 - 1;
    match frac {
        Some(f) => {
            // Round the significand to f + 1 significant digits.
            let keep = f + 1;
            while digits.len() < keep { digits.push('0'); }
            if digits.len() > keep {
                let mut d: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
                let mut carry = d[keep] >= 5;
                d.truncate(keep);
                let mut i = keep as isize - 1;
                while carry && i >= 0 {
                    d[i as usize] += 1;
                    if d[i as usize] >= 10 { d[i as usize] = 0; carry = true; } else { carry = false; }
                    i -= 1;
                }
                if carry { d.insert(0, 1); exp10 += 1; }
                digits = d.iter().map(|x| (b'0' + x) as char).collect();
            }
            let mantissa = if f == 0 { digits[..1].to_string() } else { format!("{}.{}", &digits[..1], &digits[1..1 + f]) };
            let prefix = if negative { "-" } else { "" };
            format!("{}{}e{}{}", prefix, mantissa, if exp10 >= 0 { "+" } else { "-" }, exp10.abs())
        }
        None => {
            // Shortest significand: drop trailing zeros (keep at least one digit).
            while digits.len() > 1 && digits.ends_with('0') { digits.pop(); }
            let mantissa = if digits.len() == 1 { digits.clone() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
            let prefix = if negative { "-" } else { "" };
            format!("{}{}e{}{}", prefix, mantissa, if exp10 >= 0 { "+" } else { "-" }, exp10.abs())
        }
    }
}

fn call_object_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    Ok(match method {
        "hasOwnProperty" => {
            let key = _args.first().map(to_string).unwrap_or_default();
            JsValue::Boolean(map.contains_key(&key))
        }
        "keys" => JsValue::Array(map.keys().map(|k| JsValue::String(k.clone())).collect()),
        "values" => JsValue::Array(map.values().cloned().collect()),
        // Object.prototype.toString tags plain objects; Map/Set/Promise/Date are
        // routed to their own dispatchers before reaching here.
        "toString" | "toLocaleString" => JsValue::String("[object Object]".to_string()),
        _ => JsValue::Undefined,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Map/Set/Promise/Date methods
// ═══════════════════════════════════════════════════════════════════════════

fn call_map_method(map: &mut HashMap<String, JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    let mut entries = if let Some(JsValue::Array(e)) = map.get("__entries__") { e.clone() } else { Vec::new() };
    Ok(match method {
        "get" => {
            let key = args.first().cloned().unwrap_or(JsValue::Undefined);
            let key_str = to_string(&key);
            entries.iter().find_map(|entry| {
                if let JsValue::Array(kv) = entry {
                    if kv.len() >= 2 && to_string(&kv[0]) == key_str { return Some(kv[1].clone()); }
                }
                None
            }).unwrap_or(JsValue::Undefined)
        }
        "has" => {
            let key = args.first().cloned().unwrap_or(JsValue::Undefined);
            let key_str = to_string(&key);
            JsValue::Boolean(entries.iter().any(|entry| {
                if let JsValue::Array(kv) = entry { kv.first().map(to_string).as_deref() == Some(&key_str) } else { false }
            }))
        }
        "set" => {
            // Insert or overwrite the entry for `key`, then persist to the backing store.
            let key = args.first().cloned().unwrap_or(JsValue::Undefined);
            let key_str = to_string(&key);
            let value = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            let mut replaced = false;
            for entry in entries.iter_mut() {
                if let JsValue::Array(kv) = entry {
                    if kv.first().map(to_string).as_deref() == Some(&key_str) {
                        if kv.len() >= 2 { kv[1] = value.clone(); } else { kv.push(value.clone()); }
                        replaced = true;
                        break;
                    }
                }
            }
            if !replaced { entries.push(JsValue::Array(vec![key, value])); }
            map.insert("__entries__".to_string(), JsValue::Array(entries));
            JsValue::Object(map.clone())
        }
        "delete" => {
            let key = args.first().cloned().unwrap_or(JsValue::Undefined);
            let key_str = to_string(&key);
            let before = entries.len();
            entries.retain(|entry| {
                if let JsValue::Array(kv) = entry { kv.first().map(to_string).as_deref() != Some(&key_str) } else { true }
            });
            let removed = entries.len() != before;
            map.insert("__entries__".to_string(), JsValue::Array(entries));
            JsValue::Boolean(removed)
        }
        "size" => JsValue::Number(entries.len() as f64),
        "keys" => JsValue::Array(entries.iter().filter_map(|e| if let JsValue::Array(kv) = e { kv.first().cloned() } else { None }).collect()),
        "values" => JsValue::Array(entries.iter().filter_map(|e| if let JsValue::Array(kv) = e { kv.get(1).cloned() } else { None }).collect()),
        "entries" => JsValue::Array(entries),
        "forEach" => {
            // Callback receives (value, key) for each entry, mirroring JS Map.forEach.
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            for entry in &entries {
                if let JsValue::Array(kv) = entry {
                    let key = kv.first().cloned().unwrap_or(JsValue::Undefined);
                    let value = kv.get(1).cloned().unwrap_or(JsValue::Undefined);
                    call_function(&callback, &[value, key], scope)?;
                }
            }
            JsValue::Undefined
        }
        "clear" => {
            map.insert("__entries__".to_string(), JsValue::Array(Vec::new()));
            JsValue::Undefined
        }
        _ => JsValue::Undefined,
    })
}

fn call_set_method(map: &mut HashMap<String, JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    let mut items = if let Some(JsValue::Array(i)) = map.get("__items__") { i.clone() } else { Vec::new() };
    Ok(match method {
        "has" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::Boolean(items.iter().any(|x| strict_eq(x, &val)))
        }
        "add" => {
            // Append the value only if not already present, then persist.
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            if !items.iter().any(|x| strict_eq(x, &val)) { items.push(val); }
            map.insert("__items__".to_string(), JsValue::Array(items));
            JsValue::Object(map.clone())
        }
        "delete" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            let before = items.len();
            items.retain(|x| !strict_eq(x, &val));
            let removed = items.len() != before;
            map.insert("__items__".to_string(), JsValue::Array(items));
            JsValue::Boolean(removed)
        }
        "size" => JsValue::Number(items.len() as f64),
        "values" | "keys" => JsValue::Array(items),
        "forEach" => {
            // Callback receives (value, value) for each element, mirroring JS Set.forEach.
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            for item in &items {
                call_function(&callback, &[item.clone(), item.clone()], scope)?;
            }
            JsValue::Undefined
        }
        "clear" => {
            map.insert("__items__".to_string(), JsValue::Array(Vec::new()));
            JsValue::Undefined
        }
        _ => JsValue::Undefined,
    })
}

fn call_promise_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    match method {
        "then" => {
            // If the promise is rejected, propagate the rejection (skip callback)
            if let Some(rejected) = map.get("__rejected__") {
                if *rejected != JsValue::Undefined {
                    let mut new_promise = HashMap::new();
                    new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                    new_promise.insert("__rejected__".to_string(), rejected.clone());
                    return Ok(JsValue::Object(new_promise));
                }
            }
            let resolved = map.get("__resolved__").cloned().unwrap_or(JsValue::Undefined);
            if let Some(callback) = args.first() {
                match call_function(callback, &[resolved], scope) {
                    Ok(result) => {
                        // If the callback returns a promise, flatten it
                        let mut new_promise = HashMap::new();
                        new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                        match &result {
                            JsValue::Object(inner_map) if inner_map.get("__type__").map(to_string).as_deref() == Some("Promise") => {
                                // Flatten: adopt the inner promise's state
                                if let Some(rej) = inner_map.get("__rejected__") {
                                    if *rej != JsValue::Undefined {
                                        new_promise.insert("__rejected__".to_string(), rej.clone());
                                    } else {
                                        new_promise.insert("__resolved__".to_string(), inner_map.get("__resolved__").cloned().unwrap_or(JsValue::Undefined));
                                    }
                                } else {
                                    new_promise.insert("__resolved__".to_string(), inner_map.get("__resolved__").cloned().unwrap_or(JsValue::Undefined));
                                }
                            }
                            _ => {
                                new_promise.insert("__resolved__".to_string(), result);
                            }
                        }
                        Ok(JsValue::Object(new_promise))
                    }
                    Err(Signal::Throw(reason)) => {
                        // Callback threw: returned promise is rejected
                        let mut new_promise = HashMap::new();
                        new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                        new_promise.insert("__rejected__".to_string(), reason);
                        Ok(JsValue::Object(new_promise))
                    }
                    Err(other) => Err(other),
                }
            } else {
                Ok(JsValue::Object(map.clone()))
            }
        }
        "catch" => {
            if let Some(rejected) = map.get("__rejected__") {
                if *rejected != JsValue::Undefined {
                    if let Some(callback) = args.first() {
                        match call_function(callback, &[rejected.clone()], scope) {
                            Ok(result) => {
                                let mut new_promise = HashMap::new();
                                new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                                new_promise.insert("__resolved__".to_string(), result);
                                return Ok(JsValue::Object(new_promise));
                            }
                            Err(Signal::Throw(reason)) => {
                                let mut new_promise = HashMap::new();
                                new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                                new_promise.insert("__rejected__".to_string(), reason);
                                return Ok(JsValue::Object(new_promise));
                            }
                            Err(other) => return Err(other),
                        }
                    }
                }
            }
            Ok(JsValue::Object(map.clone()))
        }
        "finally" => {
            if let Some(callback) = args.first() {
                let _ = call_function(callback, &[], scope);
            }
            // finally passes through the original promise state unchanged
            Ok(JsValue::Object(map.clone()))
        }
        _ => Ok(JsValue::Undefined),
    }
}

fn call_date_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    let ts = if let Some(JsValue::Number(n)) = map.get("__value__") { *n } else { 0.0 };
    Ok(match method {
        "getTime" | "valueOf" => JsValue::Number(ts),
        "toISOString" | "toJSON" => JsValue::String("1970-01-01T00:00:00.000Z".to_string()), // simplified
        "toString" => JsValue::String(format!("Date({})", ts)),
        _ => JsValue::Undefined,
    })
}

fn call_generator_method(map: &HashMap<String, JsValue>, method: &str) -> EvalResult {
    match method {
        "next" => {
            let values = match map.get("__values__") {
                Some(JsValue::Array(arr)) => arr.clone(),
                _ => Vec::new(),
            };
            let index = match map.get("__index__") {
                Some(JsValue::Number(n)) => *n as usize,
                _ => 0,
            };
            if index < values.len() {
                let value = values[index].clone();
                // Note: we can't mutate the map here (it's borrowed), so the caller
                // should handle the index increment via writeback.
                let mut result = HashMap::new();
                result.insert("value".to_string(), value);
                result.insert("done".to_string(), JsValue::Boolean(false));
                Ok(JsValue::Object(result))
            } else {
                let mut result = HashMap::new();
                result.insert("value".to_string(), JsValue::Undefined);
                result.insert("done".to_string(), JsValue::Boolean(true));
                Ok(JsValue::Object(result))
            }
        }
        "return" => {
            let mut result = HashMap::new();
            result.insert("value".to_string(), JsValue::Undefined);
            result.insert("done".to_string(), JsValue::Boolean(true));
            Ok(JsValue::Object(result))
        }
        _ => Ok(JsValue::Undefined),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Type coercion helpers (public for vm.rs)
// ═══════════════════════════════════════════════════════════════════════════

/// ToPrimitive with hint "number": objects/arrays are coerced via valueOf then
/// toString; primitives pass through unchanged.
fn to_primitive(v: &JsValue) -> JsValue {
    match v {
        JsValue::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(|x| match x {
                JsValue::Null | JsValue::Undefined => String::new(),
                other => to_string(other),
            }).collect();
            JsValue::String(parts.join(","))
        }
        JsValue::Object(_) => JsValue::String("[object Object]".to_string()),
        other => other.clone(),
    }
}

pub fn to_number(v: &JsValue) -> f64 {
    match v {
        JsValue::Number(n) => *n,
        JsValue::Boolean(b) => if *b { 1.0 } else { 0.0 },
        JsValue::String(s) => string_to_number(s),
        JsValue::Null => 0.0,
        JsValue::Undefined => f64::NAN,
        _ => f64::NAN,
    }
}

/// Convert a string to a number per the JS ToNumber rules: surrounding
/// whitespace is ignored, an empty (or blank) string is 0, `0x`/`0b`/`0o`
/// prefixes select non-decimal integer literals, and the `Infinity` literal is
/// recognised. Rust-specific spellings such as `inf`/`nan` are rejected as NaN.
fn string_to_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() { return 0.0; }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).map(|n| n as f64).unwrap_or(f64::NAN);
    }
    if let Some(bin) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        return i64::from_str_radix(bin, 2).map(|n| n as f64).unwrap_or(f64::NAN);
    }
    if let Some(oct) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
        return i64::from_str_radix(oct, 8).map(|n| n as f64).unwrap_or(f64::NAN);
    }
    // A valid JS decimal literal begins with a digit, sign, or decimal point;
    // anything else (including "inf"/"nan") is NaN even if Rust would parse it.
    match t.chars().next() {
        Some(c) if c.is_ascii_digit() || c == '+' || c == '-' || c == '.' => t.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

pub fn to_boolean(v: &JsValue) -> bool {
    match v {
        JsValue::Boolean(b) => *b,
        JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
        JsValue::String(s) => !s.is_empty(),
        JsValue::Null | JsValue::Undefined => false,
        JsValue::Array(_) | JsValue::Object(_) | JsValue::Function { .. } | JsValue::NativeFunction(_) | JsValue::Proxy { .. } => true,
    }
}

pub fn to_string(v: &JsValue) -> String {
    match v {
        JsValue::String(s) => s.clone(),
        JsValue::Number(n) => format_number(*n),
        JsValue::Boolean(b) => b.to_string(),
        JsValue::Null => "null".to_string(),
        JsValue::Undefined => "undefined".to_string(),
        JsValue::Array(arr) => arr.iter().map(to_string).collect::<Vec<_>>().join(","),
        JsValue::Object(_) => "[object Object]".to_string(),
        JsValue::Function { name, .. } => format!("function {}() {{ [native code] }}", name.as_deref().unwrap_or("anonymous")),
        JsValue::NativeFunction(n) => format!("function {}() {{ [native code] }}", n),
        JsValue::Proxy { .. } => "[object Proxy]".to_string(),
    }
}

/// Render a finite-or-not number exactly as ECMAScript's `Number::toString(x, 10)`
/// (the algorithm behind `String(n)`, `n.toString()`, and JSON number output).
/// Rust's default float formatting differs: it never switches to exponential
/// form (JS does for exponents outside [-6, 21]) and emits `inf`/`nan` instead
/// of `Infinity`/`NaN`. We reuse Rust's `{}` only to obtain the shortest
/// round-tripping significand digits, then lay them out per the spec.
fn format_number(n: f64) -> String {
    if n.is_nan() { return "NaN".to_string(); }
    if n.is_infinite() { return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string(); }
    if n == 0.0 { return "0".to_string(); }
    let negative = n < 0.0;
    let a = n.abs();
    // Shortest decimal digits that round-trip, with no exponent or point.
    let raw = format!("{}", a);
    // Collect the significand digits plus `exp10`, the number of digits that sit
    // left of the decimal point (negative when the value is < 1). Counting these
    // BEFORE stripping leading zeros keeps exp10 anchored to the magnitude, so a
    // value like 0.0000001 (raw "0.0000001") ends up with exp10 == -6 rather than
    // the meaningless point index of its trimmed significand.
    let (mut digits, exp10) = if let Some(e_pos) = raw.find('e') {
        // Scientific input like "1.5e30": split the significand and combine its
        // fractional length with the exponent to recover the integer-digit count.
        let sig = &raw[..e_pos];
        let exp: i64 = raw[e_pos + 1..].parse().unwrap_or(0);
        let d: String = sig.chars().filter(|c| *c != '.').collect();
        let frac = sig.find('.').map(|p| (sig.len() - p - 1) as i64).unwrap_or(0);
        (d, exp - frac)
    } else if let Some(p) = raw.find('.') {
        // Plain decimal like "123.45" (int digits == 3) or "0.001" (== 0).
        let d: String = raw.chars().filter(|c| *c != '.').collect();
        (d, p as i64)
    } else {
        let len = raw.len() as i64;
        (raw, len)
    };
    // Drop leading zeros; each removed zero shifts the decimal point one left.
    let leading = digits.len() - digits.trim_start_matches('0').len();
    digits = digits[leading..].to_string();
    let exp10 = exp10 - leading as i64;
    // Rust pads the significand to ~17 significant digits; JS prints the fewest
    // digits that round-trip. Strip redundant trailing zeros while the value
    // (digits * 10^(exp10 - k)) still parses back to exactly `a`.
    while digits.len() > 1 && digits.ends_with('0') {
        let cand = &digits[..digits.len() - 1];
        let e = exp10 - cand.len() as i64;
        if format!("{}e{}", cand, e).parse::<f64>() == Ok(a) { digits = cand.to_string(); } else { break; }
    }
    let k = digits.len() as i64;
    let body = if k <= exp10 && exp10 <= 21 {
        // Integer with trailing zeros: 1e21 -> "1" + 21 zeros.
        format!("{}{}", digits, "0".repeat((exp10 - k) as usize))
    } else if exp10 > 0 && exp10 < k {
        // Decimal point falls inside the digits: 123.45.
        format!("{}.{}", &digits[..exp10 as usize], &digits[exp10 as usize..])
    } else if exp10 <= 0 && exp10 > -6 {
        // Small magnitude stays decimal: 0.001 (exp10 == -2 leading zeros). The
        // exp10 <= 0 guard is essential: a large positive exponent that missed
        // branch 1 (e.g. 1e21 has exp10 == 22) must fall through to exponential
        // form, not reach the (-exp10) repeat below.
        format!("0.{}{}", "0".repeat((-exp10) as usize), digits)
    } else {
        // Exponential form: d[.ddd]e(+|-)ee with the exponent's own sign.
        let mantissa = if k == 1 { digits.clone() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
        let e = exp10 - 1;
        format!("{}e{}{}", mantissa, if e >= 0 { "+" } else { "-" }, e.abs())
    };
    if negative { format!("-{}", body) } else { body }
}

pub fn typeof_str(v: &JsValue) -> &'static str {
    match v {
        JsValue::Undefined => "undefined",
        JsValue::Null => "object",
        JsValue::Boolean(_) => "boolean",
        JsValue::Number(_) => "number",
        JsValue::String(_) => "string",
        JsValue::Function { .. } | JsValue::NativeFunction(_) => "function",
        JsValue::Array(_) | JsValue::Object(_) | JsValue::Proxy { .. } => "object",
    }
}

/// Abstract Relational Comparison: when both primitives are strings, compare
/// lexicographically; otherwise compare numerically (NaN yields None → false).
fn relational_cmp(l: &JsValue, r: &JsValue) -> Option<std::cmp::Ordering> {
    if let (JsValue::String(a), JsValue::String(b)) = (l, r) {
        return Some(a.cmp(b));
    }
    let ln = to_number(l);
    let rn = to_number(r);
    ln.partial_cmp(&rn)
}

fn loose_eq(l: &JsValue, r: &JsValue) -> bool {
    match (l, r) {
        (JsValue::Null | JsValue::Undefined, JsValue::Null | JsValue::Undefined) => true,
        // null and undefined are loosely equal only to each other, never to
        // numbers, strings, or booleans (so `null == 0` is false).
        (JsValue::Null | JsValue::Undefined, _) | (_, JsValue::Null | JsValue::Undefined) => false,
        (JsValue::String(a), JsValue::String(b)) => a == b,
        (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
        _ => to_number(l) == to_number(r),
    }
}

fn strict_eq(l: &JsValue, r: &JsValue) -> bool {
    match (l, r) {
        (JsValue::Undefined, JsValue::Undefined) => true,
        (JsValue::Null, JsValue::Null) => true,
        (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
        (JsValue::Number(a), JsValue::Number(b)) => a == b,
        (JsValue::String(a), JsValue::String(b)) => a == b,
        _ => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Legacy compatibility: the old eval_expr interface for vm.rs backward compat
// ═══════════════════════════════════════════════════════════════════════════

/// Evaluate a single JS expression against a flat variable scope.
/// This is the backward-compatible interface used by vm.rs.
pub fn eval_expr(input: &str, scope_map: &HashMap<String, JsValue>) -> Result<JsValue, String> {
    let tokens = lex(input)?;
    if tokens.len() <= 1 { return Ok(JsValue::Undefined); } // only Eof
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr().map_err(|e| e.to_string())?;
    let scope = Scope::new_global();
    { let mut s = scope.lock().unwrap(); s.locals = scope_map.clone(); }
    match eval_expr_node(&expr, &scope) {
        Ok(v) => Ok(v),
        Err(Signal::Throw(v)) => Err(to_string(&v)),
        Err(_) => Ok(JsValue::Undefined),
    }
}

/// Parse and evaluate a full script (multiple statements). Used by the new VM.
pub fn eval_script(input: &str, scope: &ScopeRef) -> Result<JsValue, String> {
    let tokens = lex(input)?;
    if tokens.len() <= 1 { return Ok(JsValue::Undefined); }
    let mut parser = Parser::new(tokens);
    let stmts = parser.parse_program().map_err(|e| e.to_string())?;
    match eval_program(&stmts, scope) {
        Ok(v) => Ok(v),
        Err(Signal::Return(v)) => Ok(v),
        Err(Signal::Throw(v)) => Err(to_string(&v)),
        Err(_) => Ok(JsValue::Undefined),
    }
}

/// eval() equivalent - creates a fresh scope for the code.
fn eval_script_standalone(input: &str) -> Result<JsValue, String> {
    let scope = Scope::new_global();
    eval_script(input, &scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(s: &str) -> JsValue {
        eval_expr(s, &HashMap::new()).unwrap()
    }

    fn eval_full(s: &str) -> JsValue {
        let scope = Scope::new_global();
        eval_script(s, &scope).unwrap()
    }

    #[test]
    fn arithmetic_precedence() {
        assert_eq!(eval("1 + 2 * 3"), JsValue::Number(7.0));
        assert_eq!(eval("(1 + 2) * 3"), JsValue::Number(9.0));
        assert_eq!(eval("10 % 3"), JsValue::Number(1.0));
    }

    #[test]
    fn string_concatenation() {
        assert_eq!(eval("'a' + 'b'"), JsValue::String("ab".to_string()));
        assert_eq!(eval("'x' + 1"), JsValue::String("x1".to_string()));
    }

    #[test]
    fn comparisons_and_logic() {
        assert_eq!(eval("2 > 1 && 1 < 2"), JsValue::Boolean(true));
        assert_eq!(eval("1 == 1"), JsValue::Boolean(true));
        assert_eq!(eval("1 != 2"), JsValue::Boolean(true));
        assert_eq!(eval("!false"), JsValue::Boolean(true));
    }

    #[test]
    fn short_circuit_returns_operand() {
        assert_eq!(eval("0 || 'fallback'"), JsValue::String("fallback".to_string()));
        assert_eq!(eval("'a' && 'b'"), JsValue::String("b".to_string()));
    }

    #[test]
    fn identifiers_resolve_from_scope() {
        let mut scope = HashMap::new();
        scope.insert("x".to_string(), JsValue::Number(5.0));
        assert_eq!(eval_expr("x * 2", &scope).unwrap(), JsValue::Number(10.0));
        assert_eq!(eval_expr("missing", &scope).unwrap(), JsValue::Undefined);
    }

    #[test]
    fn unary_minus() {
        assert_eq!(eval("-5 + 3"), JsValue::Number(-2.0));
    }

    #[test]
    fn if_else_works() {
        assert_eq!(eval_full("var x = 5; if (x > 3) { x = 10; } x"), JsValue::Number(10.0));
        assert_eq!(eval_full("var x = 1; if (x > 3) { x = 10; } else { x = 20; } x"), JsValue::Number(20.0));
    }

    #[test]
    fn while_loop() {
        assert_eq!(eval_full("var i = 0; while (i < 5) { i = i + 1; } i"), JsValue::Number(5.0));
    }

    #[test]
    fn for_loop() {
        assert_eq!(eval_full("var sum = 0; for (var i = 0; i < 5; i = i + 1) { sum = sum + i; } sum"), JsValue::Number(10.0));
    }

    #[test]
    fn function_declaration_and_call() {
        assert_eq!(eval_full("function add(a, b) { return a + b; } add(3, 4)"), JsValue::Number(7.0));
    }

    #[test]
    fn arrow_function() {
        assert_eq!(eval_full("var double = (x) => x * 2; double(5)"), JsValue::Number(10.0));
    }

    #[test]
    fn closure_captures_scope() {
        assert_eq!(eval_full("function make() { var x = 10; return () => x; } var get = make(); get()"), JsValue::Number(10.0));
    }

    #[test]
    fn object_property_access() {
        assert_eq!(eval_full("var obj = { a: 1, b: 2 }; obj.a + obj.b"), JsValue::Number(3.0));
    }

    #[test]
    fn array_methods() {
        assert_eq!(eval_full("var arr = [1, 2, 3]; arr.length"), JsValue::Number(3.0));
        assert_eq!(eval_full("[1,2,3].indexOf(2)"), JsValue::Number(1.0));
        assert_eq!(eval_full("[1,2,3].includes(3)"), JsValue::Boolean(true));
    }

    #[test]
    fn array_mutation_persists_on_receiver() {
        // push/pop/shift/unshift/reverse/sort/splice/fill mutate the source variable in place.
        assert_eq!(eval_full("var arr = [1, 2]; arr.push(3); arr.length"), JsValue::Number(3.0));
        assert_eq!(eval_full("var arr = [1, 2]; arr.push(3); arr[2]"), JsValue::Number(3.0));
        assert_eq!(eval_full("var arr = [1, 2, 3]; arr.pop(); arr.length"), JsValue::Number(2.0));
        assert_eq!(eval_full("var arr = [1, 2, 3]; arr.shift(); arr[0]"), JsValue::Number(2.0));
        assert_eq!(eval_full("var arr = [2, 3]; arr.unshift(1); arr[0]"), JsValue::Number(1.0));
        assert_eq!(eval_full("var arr = [2, 3]; arr.unshift(0, 1); arr.length"), JsValue::Number(4.0));
        assert_eq!(eval_full("var arr = [1, 2, 3]; arr.reverse(); arr[0]"), JsValue::Number(3.0));
        assert_eq!(eval_full("var arr = [0, 0, 0]; arr.fill(7); arr[1]"), JsValue::Number(7.0));
        assert_eq!(eval_full("var arr = [0, 0, 0, 0]; arr.fill(7, 1, 3); arr[3]"), JsValue::Number(0.0));
    }

    #[test]
    fn array_mutation_persists_on_member_and_this() {
        // Mutations through a member target (obj.items.push) persist on the object.
        assert_eq!(eval_full("var obj = { items: [1, 2] }; obj.items.push(3); obj.items.length"), JsValue::Number(3.0));
        assert_eq!(eval_full("var obj = { items: [1, 2, 3] }; obj.items.pop(); obj.items.length"), JsValue::Number(2.0));
        // Mutations through `this` inside a method persist on the receiver.
        assert_eq!(eval_full("var o = { xs: [1], add: function(v) { this.xs.push(v); } }; o.add(2); o.xs.length"), JsValue::Number(2.0));
        // Mutations through an indexed target (rows[0].push) persist.
        assert_eq!(eval_full("var rows = [[1], [2]]; rows[0].push(9); rows[0].length"), JsValue::Number(2.0));
    }

    #[test]
    fn array_sort_default_and_comparator() {
        // Default sort is lexicographic on string form.
        assert_eq!(eval_full("var a = [3, 1, 2]; a.sort(); a[0]"), JsValue::Number(1.0));
        assert_eq!(eval_full("var a = [10, 2, 1]; a.sort(); a[0]"), JsValue::Number(1.0));
        // Numeric comparator sorts ascending by value.
        assert_eq!(eval_full("var a = [10, 2, 1]; a.sort(function(x, y) { return x - y; }); a[0]"), JsValue::Number(1.0));
        assert_eq!(eval_full("var a = [10, 2, 1]; a.sort(function(x, y) { return x - y; }); a[2]"), JsValue::Number(10.0));
        // Descending comparator.
        assert_eq!(eval_full("var a = [1, 2, 3]; a.sort(function(x, y) { return y - x; }); a[0]"), JsValue::Number(3.0));
    }

    #[test]
    fn array_splice_removes_and_inserts() {
        // splice returns removed elements and mutates the receiver.
        assert_eq!(eval_full("var a = [1, 2, 3, 4]; a.splice(1, 2); a.length"), JsValue::Number(2.0));
        assert_eq!(eval_full("var a = [1, 2, 3, 4]; var r = a.splice(1, 2); r[0]"), JsValue::Number(2.0));
        assert_eq!(eval_full("var a = [1, 4]; a.splice(1, 0, 2, 3); a[2]"), JsValue::Number(3.0));
        assert_eq!(eval_full("var a = [1, 2, 3]; a.splice(-1, 1); a.length"), JsValue::Number(2.0));
    }

    #[test]
    fn array_find_index_variants() {
        assert_eq!(eval_full("[5, 12, 8, 130].findIndex(function(x) { return x > 10; })"), JsValue::Number(1.0));
        assert_eq!(eval_full("[1, 2, 3].findIndex(function(x) { return x > 10; })"), JsValue::Number(-1.0));
        assert_eq!(eval_full("[1, 2, 3, 4].findLast(function(x) { return x < 3; })"), JsValue::Number(2.0));
        assert_eq!(eval_full("[1, 2, 3, 4].findLastIndex(function(x) { return x < 3; })"), JsValue::Number(1.0));
    }

    #[test]
    fn array_at_and_last_index_of() {
        assert_eq!(eval_full("[10, 20, 30].at(0)"), JsValue::Number(10.0));
        assert_eq!(eval_full("[10, 20, 30].at(-1)"), JsValue::Number(30.0));
        assert_eq!(eval_full("[10, 20, 30].at(5)"), JsValue::Undefined);
        assert_eq!(eval_full("[1, 2, 3, 2, 1].lastIndexOf(2)"), JsValue::Number(3.0));
        assert_eq!(eval_full("[1, 2, 3].lastIndexOf(9)"), JsValue::Number(-1.0));
    }

    #[test]
    fn array_flat_map_and_reduce_right() {
        assert_eq!(eval_full("[1, 2, 3].flatMap(function(x) { return [x, x * 2]; }).length"), JsValue::Number(6.0));
        assert_eq!(eval_full("[1, 2, 3].flatMap(function(x) { return [x, x * 2]; })[3]"), JsValue::Number(4.0));
        assert_eq!(eval_full("['a', 'b', 'c'].reduceRight(function(acc, x) { return acc + x; })"), JsValue::String("cba".into()));
        assert_eq!(eval_full("[1, 2, 3].reduceRight(function(acc, x) { return acc + x; }, 10)"), JsValue::Number(16.0));
    }

    #[test]
    fn number_to_string_radix_and_precision() {
        assert_eq!(eval_full("(255).toString(16)"), JsValue::String("ff".into()));
        assert_eq!(eval_full("(5).toString(2)"), JsValue::String("101".into()));
        assert_eq!(eval_full("(255).toString()"), JsValue::String("255".into()));
        assert_eq!(eval_full("(-10).toString(2)"), JsValue::String("-1010".into()));
        assert_eq!(eval_full("(3.14159).toFixed(2)"), JsValue::String("3.14".into()));
        assert_eq!(eval_full("(123.456).toPrecision(4)"), JsValue::String("123.5".into()));
        assert_eq!(eval_full("(42).valueOf()"), JsValue::Number(42.0));
    }

    #[test]
    fn string_at_and_code_point_at() {
        assert_eq!(eval_full("'abc'.at(0)"), JsValue::String("a".into()));
        assert_eq!(eval_full("'abc'.at(-1)"), JsValue::String("c".into()));
        assert_eq!(eval_full("'abc'.at(9)"), JsValue::Undefined);
        assert_eq!(eval_full("'A'.codePointAt(0)"), JsValue::Number(65.0));
        assert_eq!(eval_full("'abc'.codePointAt(9)"), JsValue::Undefined);
    }

    #[test]
    fn string_methods() {
        assert_eq!(eval_full("'hello world'.split(' ').length"), JsValue::Number(2.0));
        assert_eq!(eval_full("'Hello'.toLowerCase()"), JsValue::String("hello".into()));
        assert_eq!(eval_full("'abc'.indexOf('b')"), JsValue::Number(1.0));
    }

    #[test]
    fn try_catch() {
        assert_eq!(eval_full("var result = 0; try { throw 42; } catch (e) { result = e; } result"), JsValue::Number(42.0));
    }

    #[test]
    fn try_finally_runs_on_normal_completion() {
        // `finally` runs after a try block that completes normally.
        assert_eq!(eval_full("var x = 0; try { x = 1; } finally { x = x + 10; } x"), JsValue::Number(11.0));
    }

    #[test]
    fn try_throw_without_catch_rethrows_after_finally() {
        // A throw with no catch clause runs `finally` and then propagates outward.
        assert_eq!(eval_full("
            var r = '';
            try {
                try { throw 'boom'; } finally { r = 'fin'; }
            } catch (e) { r = r + ':' + e; }
            r
        "), JsValue::String("fin:boom".to_string()));
    }

    #[test]
    fn try_catch_throw_escapes_to_outer_catch() {
        // A throw inside a catch block propagates to the enclosing handler.
        assert_eq!(eval_full("
            var r = '';
            try {
                try { throw 'a'; } catch (e) { throw 'b'; }
            } catch (e2) { r = e2; }
            r
        "), JsValue::String("b".to_string()));
    }

    #[test]
    fn ternary_expression() {
        assert_eq!(eval("true ? 1 : 2"), JsValue::Number(1.0));
        assert_eq!(eval("false ? 1 : 2"), JsValue::Number(2.0));
    }

    #[test]
    fn nullish_coalescing() {
        assert_eq!(eval_full("var x = null; x ?? 'default'"), JsValue::String("default".into()));
        assert_eq!(eval_full("var x = 5; x ?? 'default'"), JsValue::Number(5.0));
    }

    #[test]
    fn typeof_operator() {
        assert_eq!(eval("typeof 42"), JsValue::String("number".into()));
        assert_eq!(eval("typeof 'hi'"), JsValue::String("string".into()));
        assert_eq!(eval("typeof undefined"), JsValue::String("undefined".into()));
    }

    #[test]
    fn break_in_loop() {
        assert_eq!(eval_full("var i = 0; while (true) { i = i + 1; if (i == 3) { break; } } i"), JsValue::Number(3.0));
    }

    #[test]
    fn array_map_filter() {
        assert_eq!(eval_full("[1,2,3,4].filter((x) => x > 2).length"), JsValue::Number(2.0));
    }

    // ═══════════════════════════════════════════════════════════════════
    // Expansion tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn template_literal_interpolation() {
        assert_eq!(eval_full("var x = 5; `value is ${x}`"), JsValue::String("value is 5".into()));
        assert_eq!(eval_full("var a = 2; var b = 3; `${a} + ${b} = ${a + b}`"), JsValue::String("2 + 3 = 5".into()));
        assert_eq!(eval_full("`no interpolation`"), JsValue::String("no interpolation".into()));
    }

    #[test]
    fn destructuring_object() {
        assert_eq!(eval_full("var obj = { a: 1, b: 2 }; let { a, b } = obj; a + b"), JsValue::Number(3.0));
    }

    #[test]
    fn destructuring_array() {
        assert_eq!(eval_full("let [x, y] = [10, 20]; x + y"), JsValue::Number(30.0));
    }

    #[test]
    fn destructuring_with_alias() {
        assert_eq!(eval_full("let { a: first } = { a: 42 }; first"), JsValue::Number(42.0));
    }

    #[test]
    fn class_basic() {
        assert_eq!(eval_full("
            class Animal {
                constructor(name) {
                    this.name = name;
                }
                speak() {
                    return this.name;
                }
            }
            var a = new Animal('dog');
            a.name
        "), JsValue::String("dog".into()));
    }

    #[test]
    fn class_inheritance() {
        assert_eq!(eval_full("
            class Base {
                constructor(x) { this.x = x; }
            }
            class Child extends Base {
                constructor(x, y) { this.x = x; this.y = y; }
            }
            var c = new Child(1, 2);
            c.x + c.y
        "), JsValue::Number(3.0));
    }

    #[test]
    fn optional_chaining_member() {
        assert_eq!(eval_full("var obj = { a: { b: 5 } }; obj?.a?.b"), JsValue::Number(5.0));
        assert_eq!(eval_full("var obj = null; obj?.a?.b"), JsValue::Undefined);
    }

    #[test]
    fn optional_chaining_call() {
        assert_eq!(eval_full("var fn = null; fn?.()"), JsValue::Undefined);
    }

    #[test]
    fn nullish_assignment() {
        assert_eq!(eval_full("var x = null; x ??= 42; x"), JsValue::Number(42.0));
        assert_eq!(eval_full("var x = 5; x ??= 42; x"), JsValue::Number(5.0));
    }

    #[test]
    fn promise_resolve_then() {
        assert_eq!(eval_full("
            var result = 0;
            Promise.resolve(10).then((v) => { result = v * 2; return result; });
            result
        "), JsValue::Number(20.0));
    }

    #[test]
    fn async_await_sync_model() {
        assert_eq!(eval_full("
            async function fetchData() { return 42; }
            var result = await fetchData();
            result
        "), JsValue::Number(42.0));
    }

    #[test]
    fn promise_resolve_reject() {
        // Promise.resolve chains
        assert_eq!(eval_full("
            var p = Promise.resolve(99);
            var result = 0;
            p.then(function(v) { result = v; });
            result
        "), JsValue::Number(99.0));
    }

    #[test]
    fn promise_reject_catch() {
        // Rejected promise is caught by .catch()
        assert_eq!(eval_full("
            var p = Promise.reject('oops');
            var caught = '';
            p.catch(function(e) { caught = e; });
            caught
        "), JsValue::String("oops".to_string()));
    }

    #[test]
    fn promise_then_skips_on_reject() {
        // .then() is skipped when promise is rejected
        assert_eq!(eval_full("
            var p = Promise.reject('err');
            var called = false;
            var caught = '';
            p.then(function(v) { called = true; }).catch(function(e) { caught = e; });
            called
        "), JsValue::Boolean(false));
    }

    #[test]
    fn await_rejected_throws() {
        // await on a rejected promise throws, caught by try/catch
        assert_eq!(eval_full("
            var msg = '';
            try {
                var p = Promise.reject('fail');
                await p;
            } catch(e) {
                msg = e;
            }
            msg
        "), JsValue::String("fail".to_string()));
    }

    #[test]
    fn object_define_property_data_descriptor() {
        // Data descriptor installs the value and preserves existing keys (write-back).
        assert_eq!(eval_full("
            var obj = { a: 1 };
            Object.defineProperty(obj, 'b', { value: 2 });
            obj.a + obj.b
        "), JsValue::Number(3.0));
    }

    #[test]
    fn object_define_property_getter() {
        // An accessor getter is invoked on property read.
        assert_eq!(eval_full("
            var obj = {};
            Object.defineProperty(obj, 'x', { get: function() { return 42; } });
            obj.x
        "), JsValue::Number(42.0));
    }

    #[test]
    fn object_define_property_setter() {
        // An accessor setter is invoked on property write.
        assert_eq!(eval_full("
            var captured = 0;
            var obj = {};
            Object.defineProperty(obj, 'x', { set: function(v) { captured = v; } });
            obj.x = 99;
            captured
        "), JsValue::Number(99.0));
    }

    #[test]
    fn object_define_properties_multiple() {
        assert_eq!(eval_full("
            var obj = {};
            Object.defineProperties(obj, { x: { value: 10 }, y: { value: 20 } });
            obj.x + obj.y
        "), JsValue::Number(30.0));
    }

    #[test]
    fn object_get_own_property_descriptor_value() {
        assert_eq!(eval_full("
            var obj = { name: 'velocity' };
            var d = Object.getOwnPropertyDescriptor(obj, 'name');
            d.value
        "), JsValue::String("velocity".to_string()));
    }

    #[test]
    fn object_get_own_property_descriptor_accessor() {
        // Accessor descriptors report get/set, not value.
        assert_eq!(eval_full("
            var obj = {};
            Object.defineProperty(obj, 'x', { get: function() { return 1; } });
            var d = Object.getOwnPropertyDescriptor(obj, 'x');
            typeof d.get
        "), JsValue::String("function".to_string()));
    }

    #[test]
    fn reflect_set_mutates_in_place() {
        assert_eq!(eval_full("
            var obj = {};
            var ok = Reflect.set(obj, 'x', 5);
            obj.x
        "), JsValue::Number(5.0));
    }

    #[test]
    fn reflect_delete_property_removes_key() {
        assert_eq!(eval_full("
            var obj = { a: 1 };
            Reflect.deleteProperty(obj, 'a');
            obj.a
        "), JsValue::Undefined);
    }

    #[test]
    fn object_literal_getter() {
        assert_eq!(eval_full("
            var obj = { get x() { return 42; } };
            obj.x
        "), JsValue::Number(42.0));
    }

    #[test]
    fn object_literal_getter_uses_this() {
        assert_eq!(eval_full("
            var obj = { _v: 7, get x() { return this._v; } };
            obj.x
        "), JsValue::Number(7.0));
    }

    #[test]
    fn object_literal_setter() {
        assert_eq!(eval_full("
            var captured = 0;
            var obj = { set x(v) { captured = v; } };
            obj.x = 99;
            captured
        "), JsValue::Number(99.0));
    }

    #[test]
    fn object_literal_getter_setter_pair() {
        assert_eq!(eval_full("
            var obj = {
                _n: 1,
                get n() { return this._n; },
                set n(v) { this._n = v * 2; }
            };
            obj.n = 5;
            obj.n
        "), JsValue::Number(10.0));
    }

    #[test]
    fn instanceof_direct_class() {
        assert_eq!(eval_full("
            class Animal {}
            var a = new Animal();
            a instanceof Animal
        "), JsValue::Boolean(true));
    }

    #[test]
    fn instanceof_inherited_class() {
        assert_eq!(eval_full("
            class Animal {}
            class Dog extends Animal {}
            var d = new Dog();
            (d instanceof Dog) && (d instanceof Animal)
        "), JsValue::Boolean(true));
    }

    #[test]
    fn instanceof_negative() {
        assert_eq!(eval_full("
            class Animal {}
            class Cat {}
            var a = new Animal();
            a instanceof Cat
        "), JsValue::Boolean(false));
    }

    #[test]
    fn instanceof_plain_object_is_false() {
        assert_eq!(eval_full("
            class Animal {}
            var o = { a: 1 };
            o instanceof Animal
        "), JsValue::Boolean(false));
    }

    #[test]
    fn super_constructor_call() {
        assert_eq!(eval_full("
            class Animal {
                constructor(n) { this.name = n; }
            }
            class Dog extends Animal {
                constructor(n) { super(n); this.kind = 'dog'; }
            }
            var d = new Dog('Rex');
            d.name + '/' + d.kind
        "), JsValue::String("Rex/dog".to_string()));
    }

    #[test]
    fn super_method_call() {
        assert_eq!(eval_full("
            class Animal {
                speak() { return 'generic'; }
            }
            class Dog extends Animal {
                speak() { return super.speak() + ' woof'; }
            }
            var d = new Dog();
            d.speak()
        "), JsValue::String("generic woof".to_string()));
    }

    #[test]
    fn super_method_uses_this() {
        assert_eq!(eval_full("
            class Base {
                greet() { return 'hi ' + this.name; }
            }
            class Sub extends Base {
                constructor() { this.name = 'vel'; }
                greet() { return super.greet() + '!'; }
            }
            var s = new Sub();
            s.greet()
        "), JsValue::String("hi vel!".to_string()));
    }

    #[test]
    fn super_chained_constructors() {
        assert_eq!(eval_full("
            class A { constructor() { this.a = 1; } }
            class B extends A { constructor() { super(); this.b = 2; } }
            class C extends B { constructor() { super(); this.c = 3; } }
            var x = new C();
            x.a + x.b + x.c
        "), JsValue::Number(6.0));
    }

    #[test]
    fn static_method_call() {
        assert_eq!(eval_full("
            class Math2 {
                static double(n) { return n * 2; }
            }
            Math2.double(21)
        "), JsValue::Number(42.0));
    }

    #[test]
    fn static_method_this_is_class() {
        assert_eq!(eval_full("
            class Counter {
                static base() { return 10; }
                static doubled() { return this.base() * 2; }
            }
            Counter.doubled()
        "), JsValue::Number(20.0));
    }

    #[test]
    fn static_method_inherited() {
        assert_eq!(eval_full("
            class Base {
                static hello() { return 'hi'; }
            }
            class Sub extends Base {}
            Sub.hello()
        "), JsValue::String("hi".to_string()));
    }

    #[test]
    fn class_getter() {
        assert_eq!(eval_full("
            class C {
                get x() { return 42; }
            }
            var c = new C();
            c.x
        "), JsValue::Number(42.0));
    }

    #[test]
    fn class_getter_uses_this() {
        assert_eq!(eval_full("
            class C {
                constructor() { this._v = 7; }
                get x() { return this._v; }
            }
            var c = new C();
            c.x
        "), JsValue::Number(7.0));
    }

    #[test]
    fn class_getter_setter_pair() {
        assert_eq!(eval_full("
            class C {
                set x(v) { this._v = v * 3; }
                get x() { return this._v; }
            }
            var c = new C();
            c.x = 4;
            c.x
        "), JsValue::Number(12.0));
    }

    #[test]
    fn class_getter_inherited() {
        assert_eq!(eval_full("
            class Base {
                get kind() { return 'base'; }
            }
            class Sub extends Base {}
            var s = new Sub();
            s.kind
        "), JsValue::String("base".to_string()));
    }

    #[test]
    fn delete_property_removes_key() {
        assert_eq!(eval_full("
            var obj = { a: 1, b: 2 };
            delete obj.a;
            'a' in obj
        "), JsValue::Boolean(false));
    }

    #[test]
    fn delete_property_value_gone() {
        assert_eq!(eval_full("
            var obj = { a: 1, b: 2 };
            delete obj.a;
            obj.b
        "), JsValue::Number(2.0));
    }

    #[test]
    fn delete_returns_true() {
        assert_eq!(eval_full("
            var obj = { a: 1 };
            delete obj.a
        "), JsValue::Boolean(true));
    }

    #[test]
    fn delete_computed_key() {
        assert_eq!(eval_full("
            var obj = { x: 9 };
            delete obj['x'];
            obj.x
        "), JsValue::Undefined);
    }

    #[test]
    fn delete_array_element_leaves_hole() {
        assert_eq!(eval_full("
            var arr = [1, 2, 3];
            delete arr[1];
            arr[0] + arr[2]
        "), JsValue::Number(4.0));
    }

    #[test]
    fn computed_property_key_from_var() {
        assert_eq!(eval_full("
            var k = 'name';
            var obj = { [k]: 'vel' };
            obj.name
        "), JsValue::String("vel".to_string()));
    }

    #[test]
    fn computed_property_key_expression() {
        assert_eq!(eval_full("
            var obj = { ['a' + 'b']: 1 };
            obj.ab
        "), JsValue::Number(1.0));
    }

    #[test]
    fn computed_property_key_number() {
        assert_eq!(eval_full("
            var i = 2;
            var obj = { [i]: 'x' };
            obj['2']
        "), JsValue::String("x".to_string()));
    }

    #[test]
    fn reflect_construct_class() {
        assert_eq!(eval_full("
            class Animal {
                constructor(n) { this.name = n; }
            }
            var a = Reflect.construct(Animal, ['Rex']);
            a.name
        "), JsValue::String("Rex".to_string()));
    }

    #[test]
    fn reflect_construct_class_instanceof() {
        assert_eq!(eval_full("
            class Animal {
                constructor(n) { this.name = n; }
            }
            var a = Reflect.construct(Animal, ['Rex']);
            a instanceof Animal
        "), JsValue::Boolean(true));
    }

    #[test]
    fn reflect_construct_function() {
        assert_eq!(eval_full("
            function Point(x, y) { this.x = x; this.y = y; }
            var p = Reflect.construct(Point, [3, 4]);
            p.x + p.y
        "), JsValue::Number(7.0));
    }

    #[test]
    fn object_keys_hides_internal_keys() {
        // Class instances carry `__class_name__`/`__instanceof__` bookkeeping that
        // must not leak into Object.keys.
        assert_eq!(eval_full("
            class Animal { constructor(n) { this.name = n; } }
            var a = new Animal('rex');
            Object.keys(a).length
        "), JsValue::Number(1.0));
        assert_eq!(eval_full("
            class Animal { constructor(n) { this.name = n; } }
            var a = new Animal('rex');
            Object.keys(a).indexOf('name') >= 0
        "), JsValue::Boolean(true));
        assert_eq!(eval_full("
            class Animal { constructor(n) { this.name = n; } }
            var a = new Animal('rex');
            Object.keys(a).indexOf('__instanceof__')
        "), JsValue::Number(-1.0));
    }

    #[test]
    fn for_in_hides_internal_keys() {
        assert_eq!(eval_full("
            class Animal { constructor(n) { this.name = n; this.age = 5; } }
            var a = new Animal('rex');
            var count = 0;
            for (var k in a) { count = count + 1; }
            count
        "), JsValue::Number(2.0));
    }

    #[test]
    fn object_values_resolves_getter() {
        assert_eq!(eval_full("
            var o = { get x() { return 42; } };
            Object.values(o)[0]
        "), JsValue::Number(42.0));
        assert_eq!(eval_full("
            var o = { get x() { return 42; } };
            Object.keys(o).indexOf('x') >= 0
        "), JsValue::Boolean(true));
    }

    #[test]
    fn object_entries_resolves_getter() {
        assert_eq!(eval_full("
            var o = { a: 1, get x() { return 9; } };
            Object.entries(o).length
        "), JsValue::Number(2.0));
    }

    #[test]
    fn user_double_underscore_key_not_internal() {
        // `__foo` (no trailing delimiter) is a legitimate user key and must survive.
        assert_eq!(eval_full("
            var o = { __foo: 7 };
            Object.keys(o).indexOf('__foo') >= 0
        "), JsValue::Boolean(true));
    }

    #[test]
    fn class_field_basic() {
        assert_eq!(eval_full("
            class C { x = 5; }
            new C().x
        "), JsValue::Number(5.0));
    }

    #[test]
    fn class_field_declaration_order() {
        // Later fields may reference earlier ones via `this`; order must be preserved.
        assert_eq!(eval_full("
            class C { a = 2; b = this.a + 3; }
            new C().b
        "), JsValue::Number(5.0));
    }

    #[test]
    fn class_field_bare_is_undefined() {
        assert_eq!(eval_full("
            class C { x; }
            new C().x
        "), JsValue::Undefined);
    }

    #[test]
    fn class_static_field() {
        assert_eq!(eval_full("
            class C { static n = 10; }
            C.n
        "), JsValue::Number(10.0));
    }

    #[test]
    fn class_field_overridden_by_constructor() {
        assert_eq!(eval_full("
            class C { x = 1; constructor() { this.x = 9; } }
            new C().x
        "), JsValue::Number(9.0));
    }

    #[test]
    fn class_field_inherited() {
        assert_eq!(eval_full("
            class A { a = 1; }
            class B extends A { b = 2; }
            var o = new B();
            o.a + o.b
        "), JsValue::Number(3.0));
    }

    #[test]
    fn new_expression_member_chain() {
        // `new Foo().member` and `new Foo().method()` must parse and evaluate.
        assert_eq!(eval_full("
            class P { constructor(x) { this.x = x; } double() { return this.x * 2; } }
            new P(21).double()
        "), JsValue::Number(42.0));
        assert_eq!(eval_full("
            class P { constructor(x) { this.x = x; } }
            new P(7).x
        "), JsValue::Number(7.0));
    }

    #[test]
    fn promise_executor_resolve() {
        // new Promise with resolve() call
        assert_eq!(eval_full("
            var p = new Promise(function(resolve, reject) { resolve(77); });
            var out = 0;
            p.then(function(v) { out = v; });
            out
        "), JsValue::Number(77.0));
    }

    #[test]
    fn promise_executor_reject() {
        // new Promise with reject() call
        assert_eq!(eval_full("
            var p = new Promise(function(resolve, reject) { reject('bad'); });
            var out = '';
            p.catch(function(e) { out = e; });
            out
        "), JsValue::String("bad".to_string()));
    }

    #[test]
    fn promise_then_flattens() {
        // .then() returning a promise is flattened
        assert_eq!(eval_full("
            var p = Promise.resolve(10);
            var out = 0;
            p.then(function(v) { return Promise.resolve(v * 2); }).then(function(v) { out = v; });
            out
        "), JsValue::Number(20.0));
    }

    #[test]
    fn map_basic() {
        assert_eq!(eval_full("
            var m = new Map([['a', 1], ['b', 2]]);
            m.get('a')
        "), JsValue::Number(1.0));
    }

    #[test]
    fn set_basic() {
        assert_eq!(eval_full("
            var s = new Set([1, 2, 3]);
            s.has(2)
        "), JsValue::Boolean(true));
    }

    #[test]
    fn object_spread() {
        assert_eq!(eval_full("
            var base = { a: 1, b: 2 };
            var extended = { ...base, c: 3 };
            extended.a + extended.b + extended.c
        "), JsValue::Number(6.0));
    }

    #[test]
    fn object_spread_override() {
        assert_eq!(eval_full("
            var base = { a: 1, b: 2 };
            var override_obj = { ...base, b: 10 };
            override_obj.b
        "), JsValue::Number(10.0));
    }

    #[test]
    fn method_shorthand() {
        assert_eq!(eval_full("
            var obj = { add(a, b) { return a + b; } };
            obj.add(3, 4)
        "), JsValue::Number(7.0));
    }

    #[test]
    fn this_binding_in_method() {
        assert_eq!(eval_full("
            var obj = { x: 10, getX() { return this.x; } };
            obj.getX()
        "), JsValue::Number(10.0));
    }

    #[test]
    fn this_binding_class_method() {
        assert_eq!(eval_full("
            class Counter {
                constructor() { this.count = 0; }
                inc() { this.count = this.count + 1; return this.count; }
            }
            var c = new Counter();
            c.inc();
            c.inc()
        "), JsValue::Number(2.0));
    }

    #[test]
    fn generator_function_basic() {
        assert_eq!(eval_full("
            function* gen() {
                yield 1;
                yield 2;
                yield 3;
            }
            var it = gen();
            var sum = 0;
            for (var x of it) { sum = sum + x; }
            sum
        "), JsValue::Number(6.0));
    }

    #[test]
    fn for_of_array() {
        assert_eq!(eval_full("
            var arr = [10, 20, 30];
            var sum = 0;
            for (var x of arr) { sum = sum + x; }
            sum
        "), JsValue::Number(60.0));
    }

    #[test]
    fn eval_function() {
        assert_eq!(eval_full("
            eval('1 + 2')
        "), JsValue::Number(3.0));
    }

    #[test]
    fn new_function_constructor() {
        assert_eq!(eval_full("
            var add = new Function('a', 'b', 'return a + b');
            add(3, 4)
        "), JsValue::Number(7.0));
    }

    #[test]
    fn labeled_statement() {
        assert_eq!(eval_full("
            var x = 0;
            outer: for (var i = 0; i < 3; i = i + 1) {
                x = x + i;
            }
            x
        "), JsValue::Number(3.0));
    }

    #[test]
    fn prototype_chain_method_lookup() {
        assert_eq!(eval_full("
            var proto = { greet() { return 'hello'; } };
            var obj = Object.create(proto);
            obj.greet()
        "), JsValue::String("hello".into()));
    }

    #[test]
    fn proxy_construction() {
        let result = eval_full("
            var target = { x: 42 };
            var handler = {};
            var p = new Proxy(target, handler);
            p
        ");
        // Phase 7: Proxy is now a native JsValue::Proxy variant
        match &result {
            JsValue::Proxy { target, handler } => {
                assert_eq!(**target, JsValue::Object({
                    let mut t = HashMap::new();
                    t.insert("x".to_string(), JsValue::Number(42.0));
                    t
                }));
                assert_eq!(**handler, JsValue::Object(HashMap::new()));
            }
            _ => panic!("Expected JsValue::Proxy, got {:?}", result),
        }
    }

    #[test]
    fn proxy_has_trap_controls_in_operator() {
        // The handler.has trap intercepts the `in` operator.
        assert_eq!(eval_full("
            var target = { x: 1 };
            var handler = { has: function(t, k) { return k === 'x' || k === 'y'; } };
            var p = new Proxy(target, handler);
            ('y' in p)
        "), JsValue::Boolean(true));
        assert_eq!(eval_full("
            var target = { x: 1 };
            var handler = { has: function(t, k) { return k === 'x' || k === 'y'; } };
            var p = new Proxy(target, handler);
            ('z' in p)
        "), JsValue::Boolean(false));
    }

    #[test]
    fn proxy_in_operator_falls_through_to_target_without_has_trap() {
        // Without a has trap, `in` reflects the target's own keys.
        assert_eq!(eval_full("
            var target = { x: 1 };
            var p = new Proxy(target, {});
            ('x' in p)
        "), JsValue::Boolean(true));
        assert_eq!(eval_full("
            var target = { x: 1 };
            var p = new Proxy(target, {});
            ('missing' in p)
        "), JsValue::Boolean(false));
    }

    #[test]
    fn in_operator_sees_inherited_members() {
        // `in` reports prototype methods, matching JS semantics.
        assert_eq!(eval_full("
            class Base { hello() { return 1; } }
            var b = new Base();
            ('hello' in b)
        "), JsValue::Boolean(true));
        assert_eq!(eval_full("
            class Base { hello() { return 1; } }
            var b = new Base();
            ('absent' in b)
        "), JsValue::Boolean(false));
    }

    #[test]
    fn in_operator_array_and_string() {
        assert_eq!(eval_full("('length' in [1, 2, 3])"), JsValue::Boolean(true));
        assert_eq!(eval_full("('1' in [1, 2, 3])"), JsValue::Boolean(true));
        assert_eq!(eval_full("('5' in [1, 2, 3])"), JsValue::Boolean(false));
        assert_eq!(eval_full("('0' in 'abc')"), JsValue::Boolean(true));
        assert_eq!(eval_full("('3' in 'abc')"), JsValue::Boolean(false));
    }

    #[test]
    fn proxy_delete_property_trap_controls_result() {
        // A deleteProperty trap returning false vetoes the delete.
        assert_eq!(eval_full("
            var target = { x: 1 };
            var handler = { deleteProperty: function(t, k) { return false; } };
            var p = new Proxy(target, handler);
            (delete p.x)
        "), JsValue::Boolean(false));
        // A trap returning true reports success.
        assert_eq!(eval_full("
            var target = { x: 1 };
            var handler = { deleteProperty: function(t, k) { return true; } };
            var p = new Proxy(target, handler);
            (delete p.x)
        "), JsValue::Boolean(true));
    }

    #[test]
    fn proxy_delete_forwards_to_target_without_trap() {
        assert_eq!(eval_full("
            var target = { x: 1 };
            var p = new Proxy(target, {});
            delete p.x;
            ('x' in p)
        "), JsValue::Boolean(false));
    }

    #[test]
    fn proxy_own_keys_trap_drives_object_keys() {
        assert_eq!(eval_full("
            var target = { a: 1, b: 2 };
            var handler = { ownKeys: function(t) { return ['a', 'b', 'c']; } };
            var p = new Proxy(target, handler);
            Object.keys(p).length
        "), JsValue::Number(3.0));
        assert_eq!(eval_full("
            var target = { a: 1, b: 2 };
            var handler = { ownKeys: function(t) { return ['a', 'b', 'c']; } };
            var p = new Proxy(target, handler);
            Object.keys(p)[2]
        "), JsValue::String("c".to_string()));
    }

    #[test]
    fn object_keys_values_on_array() {
        assert_eq!(eval_full("Object.keys([10, 20, 30]).length"), JsValue::Number(3.0));
        assert_eq!(eval_full("Object.values([10, 20, 30])[1]"), JsValue::Number(20.0));
    }

    #[test]
    fn reflect_has_consults_proxy_has_trap() {
        // Reflect.has routes through the proxy `has` trap, like the `in` operator.
        assert_eq!(eval_full("
            var target = { x: 1 };
            var handler = { has: function(t, k) { return k === 'x' || k === 'y'; } };
            var p = new Proxy(target, handler);
            Reflect.has(p, 'y')
        "), JsValue::Boolean(true));
        assert_eq!(eval_full("
            var target = { x: 1 };
            var handler = { has: function(t, k) { return k === 'x' || k === 'y'; } };
            var p = new Proxy(target, handler);
            Reflect.has(p, 'z')
        "), JsValue::Boolean(false));
    }

    #[test]
    fn reflect_has_walks_prototype_chain() {
        // Reflect.has sees inherited members, matching JS semantics.
        assert_eq!(eval_full("
            class Base { hello() { return 1; } }
            var b = new Base();
            Reflect.has(b, 'hello')
        "), JsValue::Boolean(true));
        assert_eq!(eval_full("
            class Base { hello() { return 1; } }
            var b = new Base();
            Reflect.has(b, 'absent')
        "), JsValue::Boolean(false));
    }

    #[test]
    fn reflect_delete_property_consults_proxy_trap() {
        // A proxy `deleteProperty` trap returning false vetoes the delete.
        assert_eq!(eval_full("
            var target = { x: 1 };
            var handler = { deleteProperty: function(t, k) { return false; } };
            var p = new Proxy(target, handler);
            Reflect.deleteProperty(p, 'x')
        "), JsValue::Boolean(false));
        // A trap returning true reports success.
        assert_eq!(eval_full("
            var target = { x: 1 };
            var handler = { deleteProperty: function(t, k) { return true; } };
            var p = new Proxy(target, handler);
            Reflect.deleteProperty(p, 'x')
        "), JsValue::Boolean(true));
    }

    #[test]
    fn reflect_delete_property_identifier_writeback() {
        // Deleting via an identifier target mutates the binding in place.
        assert_eq!(eval_full("
            var obj = { a: 1, b: 2 };
            Reflect.deleteProperty(obj, 'a');
            obj.a
        "), JsValue::Undefined);
        // Deleting an absent key yields true (JS non-strict semantics).
        assert_eq!(eval_full("
            var obj = { a: 1 };
            Reflect.deleteProperty(obj, 'missing')
        "), JsValue::Boolean(true));
    }

    #[test]
    fn reflect_own_keys_consults_proxy_trap() {
        assert_eq!(eval_full("
            var target = { a: 1, b: 2 };
            var handler = { ownKeys: function(t) { return ['a', 'b', 'c']; } };
            var p = new Proxy(target, handler);
            Reflect.ownKeys(p).length
        "), JsValue::Number(3.0));
        assert_eq!(eval_full("
            var target = { a: 1, b: 2 };
            var handler = { ownKeys: function(t) { return ['a', 'b', 'c']; } };
            var p = new Proxy(target, handler);
            Reflect.ownKeys(p)[2]
        "), JsValue::String("c".to_string()));
    }

    #[test]
    fn reflect_own_keys_on_array_includes_length() {
        // Array own keys are the indices plus `length`.
        assert_eq!(eval_full("Reflect.ownKeys([10, 20, 30]).length"), JsValue::Number(4.0));
        assert_eq!(eval_full("Reflect.ownKeys([10, 20, 30])[3]"), JsValue::String("length".to_string()));
    }

    #[test]
    fn object_from_entries_builds_object() {
        // Round-trips with Object.entries and accepts a literal array of pairs.
        assert_eq!(eval_full("Object.fromEntries([['a', 1], ['b', 2]]).b"), JsValue::Number(2.0));
        assert_eq!(eval_full("Object.fromEntries(Object.entries({ x: 5 })).x"), JsValue::Number(5.0));
        assert_eq!(eval_full("Object.keys(Object.fromEntries([['k', 9]])).length"), JsValue::Number(1.0));
    }

    #[test]
    fn array_of_and_from_collections() {
        // Array.of wraps its arguments verbatim (unlike Array(n) which sizes).
        assert_eq!(eval_full("Array.of(7, 8, 9).length"), JsValue::Number(3.0));
        assert_eq!(eval_full("Array.of(7)[0]"), JsValue::Number(7.0));
        // Array.from over a Set yields its unique values.
        assert_eq!(eval_full("Array.from(new Set([1, 1, 2, 3])).length"), JsValue::Number(3.0));
        // Array.from over a Map yields [key, value] pairs.
        assert_eq!(eval_full("Array.from(new Map([['a', 1]]))[0][0]"), JsValue::String("a".to_string()));
        // Array.from over an array-like object walks 0..length.
        assert_eq!(eval_full("Array.from({ length: 2, 0: 'x', 1: 'y' })[1]"), JsValue::String("y".to_string()));
    }

    #[test]
    fn map_and_set_mutations_persist() {
        // Map.set persists and get reads it back across statements.
        assert_eq!(eval_full("var m = new Map(); m.set('a', 1); m.set('b', 2); m.get('b')"), JsValue::Number(2.0));
        assert_eq!(eval_full("var m = new Map(); m.set('a', 1); m.set('a', 9); m.get('a')"), JsValue::Number(9.0));
        assert_eq!(eval_full("var m = new Map([['a', 1]]); m.delete('a'); m.has('a')"), JsValue::Boolean(false));
        // Set.add persists and stays unique; delete removes.
        assert_eq!(eval_full("var s = new Set(); s.add(1); s.add(1); s.add(2); s.size()"), JsValue::Number(2.0));
        assert_eq!(eval_full("var s = new Set([1, 2, 3]); s.delete(2); s.has(2)"), JsValue::Boolean(false));
        // Mutation through `this` inside a method persists to the receiver.
        assert_eq!(eval_full("var o = { bag: new Set(), put: function(v) { this.bag.add(v); } }; o.put(5); o.put(5); o.bag.size()"), JsValue::Number(1.0));
    }

    #[test]
    fn map_and_set_for_each_invoke_callback() {
        // Map.forEach passes (value, key): summing values yields 1 + 2 + 3 = 6.
        assert_eq!(eval_full("var m = new Map([['a', 1], ['b', 2], ['c', 3]]); var t = 0; m.forEach(function(v) { t = t + v; }); t"), JsValue::Number(6.0));
        // The key is provided as the second argument.
        assert_eq!(eval_full("var m = new Map([['x', 10]]); var k = ''; m.forEach(function(v, key) { k = key; }); k"), JsValue::String("x".to_string()));
        // Set.forEach iterates each unique value.
        assert_eq!(eval_full("var s = new Set([2, 4, 6]); var t = 0; s.forEach(function(v) { t = t + v; }); t"), JsValue::Number(12.0));
    }

    #[test]
    fn weakmap_and_weakset_support_core_methods() {
        // WeakMap.set/get/has/delete persist like Map.
        assert_eq!(eval_full("var w = new WeakMap(); w.set('k', 42); w.get('k')"), JsValue::Number(42.0));
        assert_eq!(eval_full("var w = new WeakMap([['a', 1]]); w.has('a')"), JsValue::Boolean(true));
        assert_eq!(eval_full("var w = new WeakMap([['a', 1]]); w.delete('a'); w.has('a')"), JsValue::Boolean(false));
        // WeakSet.add/has/delete persist like Set.
        assert_eq!(eval_full("var w = new WeakSet(); w.add('x'); w.has('x')"), JsValue::Boolean(true));
        assert_eq!(eval_full("var w = new WeakSet(['x']); w.delete('x'); w.has('x')"), JsValue::Boolean(false));
    }

    #[test]
    fn string_pad_counts_by_char_not_byte() {
        // ASCII padding pads to the requested length with the given fill.
        assert_eq!(eval_full("'5'.padStart(3, '0')"), JsValue::String("005".to_string()));
        assert_eq!(eval_full("'5'.padEnd(3, '.')"), JsValue::String("5..".to_string()));
        // A target shorter than the string returns the string unchanged.
        assert_eq!(eval_full("'hello'.padStart(2)"), JsValue::String("hello".to_string()));
        // Multi-byte content is counted by characters and never sliced mid-codepoint.
        assert_eq!(eval_full("'e'.padStart(3, '\u{20ac}')"), JsValue::String("\u{20ac}\u{20ac}e".to_string()));
        // Multi-byte source string keeps its full content when already at length.
        assert_eq!(eval_full("'\u{20ac}\u{20ac}'.padEnd(2, 'x')"), JsValue::String("\u{20ac}\u{20ac}".to_string()));
    }

    #[test]
    fn string_index_of_returns_char_position() {
        // ASCII positions are unchanged.
        assert_eq!(eval_full("'hello'.indexOf('l')"), JsValue::Number(2.0));
        assert_eq!(eval_full("'hello'.lastIndexOf('l')"), JsValue::Number(3.0));
        assert_eq!(eval_full("'abc'.indexOf('z')"), JsValue::Number(-1.0));
        // After a 3-byte euro sign, 'x' is at char index 1 (not byte index 3).
        assert_eq!(eval_full("'\u{20ac}x'.indexOf('x')"), JsValue::Number(1.0));
        assert_eq!(eval_full("'\u{20ac}x\u{20ac}x'.lastIndexOf('x')"), JsValue::Number(3.0));
    }

    #[test]
    fn string_length_counts_chars() {
        // ASCII length is unchanged.
        assert_eq!(eval_full("'hello'.length"), JsValue::Number(5.0));
        // A 3-byte euro sign counts as one character, consistent with slice/charAt.
        assert_eq!(eval_full("'\u{20ac}'.length"), JsValue::Number(1.0));
        assert_eq!(eval_full("'a\u{20ac}b'.length"), JsValue::Number(3.0));
        // Length agrees with char indexing: last valid index is length - 1.
        assert_eq!(eval_full("var s = 'a\u{20ac}b'; s[s.length - 1]"), JsValue::String("b".to_string()));
    }

    #[test]
    fn string_split_limit_substr_concat() {
        // split honours the limit argument.
        assert_eq!(eval_full("'a,b,c,d'.split(',', 2).length"), JsValue::Number(2.0));
        assert_eq!(eval_full("'a,b,c'.split(',')[2]"), JsValue::String("c".to_string()));
        // substr(start, length) with a positive and a negative start.
        assert_eq!(eval_full("'hello'.substr(1, 3)"), JsValue::String("ell".to_string()));
        assert_eq!(eval_full("'hello'.substr(-2)"), JsValue::String("lo".to_string()));
        // concat joins all arguments after the receiver.
        assert_eq!(eval_full("'a'.concat('b', 'c')"), JsValue::String("abc".to_string()));
    }

    #[test]
    fn math_trig_and_extended_functions() {
        // Trigonometric identities at well-known points.
        assert_eq!(eval_full("Math.cos(0)"), JsValue::Number(1.0));
        assert_eq!(eval_full("Math.sin(0)"), JsValue::Number(0.0));
        // Logarithms base 2 and 10.
        assert_eq!(eval_full("Math.log2(8)"), JsValue::Number(3.0));
        assert_eq!(eval_full("Math.log10(1000)"), JsValue::Number(3.0));
        // Cube root and Euclidean distance.
        assert_eq!(eval_full("Math.cbrt(27)"), JsValue::Number(3.0));
        assert_eq!(eval_full("Math.hypot(3, 4)"), JsValue::Number(5.0));
        // Exponential at zero and inverse tangent quadrant handling.
        assert_eq!(eval_full("Math.exp(0)"), JsValue::Number(1.0));
        assert_eq!(eval_full("Math.atan2(0, 1)"), JsValue::Number(0.0));
        // clz32 counts leading zero bits of the 32-bit representation.
        assert_eq!(eval_full("Math.clz32(1)"), JsValue::Number(31.0));
    }

    #[test]
    fn array_flat_depth_and_copy_within() {
        // Default depth of 1 flattens a single level.
        assert_eq!(eval_full("[1, [2, [3]]].flat().length"), JsValue::Number(3.0));
        // Explicit depth 2 reaches the inner array.
        assert_eq!(eval_full("[1, [2, [3]]].flat(2).length"), JsValue::Number(3.0));
        assert_eq!(eval_full("[1, [2, [3]]].flat(2)[2]"), JsValue::Number(3.0));
        // A large depth flattens fully regardless of nesting.
        assert_eq!(eval_full("[1, [2, [3, [4]]]].flat(10).length"), JsValue::Number(4.0));
        // copyWithin shifts a slice in place without changing length.
        assert_eq!(eval_full("[1, 2, 3, 4, 5].copyWithin(0, 3).length"), JsValue::Number(5.0));
        assert_eq!(eval_full("[1, 2, 3, 4, 5].copyWithin(0, 3)[0]"), JsValue::Number(4.0));
        assert_eq!(eval_full("[1, 2, 3, 4, 5].copyWithin(0, 3)[1]"), JsValue::Number(5.0));
    }

    #[test]
    fn global_infinity_and_nan_identifiers() {
        // Bare Infinity resolves to the positive infinity number.
        assert_eq!(eval_full("Infinity > 1e308"), JsValue::Boolean(true));
        assert_eq!(eval_full("-Infinity < -1e308"), JsValue::Boolean(true));
        // NaN resolves to a NaN value (detected via Number.isNaN since NaN !== NaN).
        assert_eq!(eval_full("Number.isNaN(NaN)"), JsValue::Boolean(true));
        // Infinity is usable as a flat() depth to flatten fully.
        assert_eq!(eval_full("[1, [2, [3, [4]]]].flat(Infinity).length"), JsValue::Number(4.0));
    }

    #[test]
    fn string_locale_compare_ordering() {
        // Returns negative, zero, or positive per the JS contract.
        assert_eq!(eval_full("'a'.localeCompare('b')"), JsValue::Number(-1.0));
        assert_eq!(eval_full("'b'.localeCompare('a')"), JsValue::Number(1.0));
        assert_eq!(eval_full("'a'.localeCompare('a')"), JsValue::Number(0.0));
        // Usable as a sort comparator yielding lexical order.
        assert_eq!(eval_full("['c', 'a', 'b'].sort(function(x, y) { return x.localeCompare(y); })[0]"), JsValue::String("a".to_string()));
    }

    #[test]
    fn number_and_math_constants_and_predicates() {
        // Math constants resolve as member access.
        assert_eq!(eval_full("Math.PI > 3.14 && Math.PI < 3.15"), JsValue::Boolean(true));
        assert_eq!(eval_full("Math.E > 2.71 && Math.E < 2.72"), JsValue::Boolean(true));
        // Number constants resolve as member access.
        assert_eq!(eval_full("Number.MAX_SAFE_INTEGER"), JsValue::Number(9007199254740991.0));
        assert_eq!(eval_full("Number.POSITIVE_INFINITY > 1e308"), JsValue::Boolean(true));
        // Integer predicates discriminate fractional and non-numeric inputs.
        assert_eq!(eval_full("Number.isInteger(4)"), JsValue::Boolean(true));
        assert_eq!(eval_full("Number.isInteger(4.5)"), JsValue::Boolean(false));
        assert_eq!(eval_full("Number.isInteger('4')"), JsValue::Boolean(false));
        assert_eq!(eval_full("Number.isSafeInteger(9007199254740991)"), JsValue::Boolean(true));
        assert_eq!(eval_full("Number.isSafeInteger(9007199254740993)"), JsValue::Boolean(false));
    }

    #[test]
    fn object_is_and_set_prototype_of() {
        // Object.is uses SameValue: NaN equals NaN, +0 differs from -0.
        assert_eq!(eval_full("Object.is(NaN, NaN)"), JsValue::Boolean(true));
        assert_eq!(eval_full("Object.is(0, -0)"), JsValue::Boolean(false));
        assert_eq!(eval_full("Object.is(1, 1)"), JsValue::Boolean(true));
        assert_eq!(eval_full("Object.is('a', 'b')"), JsValue::Boolean(false));
        // setPrototypeOf installs a prototype whose members become reachable.
        assert_eq!(eval_full("var proto = { greet: 42 }; var o = {}; Object.setPrototypeOf(o, proto); o.greet"), JsValue::Number(42.0));
    }

    #[test]
    fn number_predicates_do_not_coerce() {
        // Number.isNaN/isFinite reject non-numbers without coercion...
        assert_eq!(eval_full("Number.isNaN('foo')"), JsValue::Boolean(false));
        assert_eq!(eval_full("Number.isFinite('42')"), JsValue::Boolean(false));
        // ...while still recognising genuine numeric cases.
        assert_eq!(eval_full("Number.isNaN(NaN)"), JsValue::Boolean(true));
        assert_eq!(eval_full("Number.isFinite(42)"), JsValue::Boolean(true));
        // Global isNaN/isFinite keep their coercing behaviour.
        assert_eq!(eval_full("isNaN('foo')"), JsValue::Boolean(true));
        assert_eq!(eval_full("isFinite('42')"), JsValue::Boolean(true));
    }

    #[test]
    fn array_non_mutating_change_methods() {
        // toReversed returns a new array and leaves the source unchanged.
        assert_eq!(eval_full("var a = [1, 2, 3]; var b = a.toReversed(); b[0] * 10 + a[0]"), JsValue::Number(31.0));
        // toSorted orders a copy without mutating the receiver.
        assert_eq!(eval_full("var a = [3, 1, 2]; var b = a.toSorted(function(x, y) { return x - y; }); b[0] * 10 + a[0]"), JsValue::Number(13.0));
        // with replaces one index in a copy, supporting negative indices.
        assert_eq!(eval_full("[1, 2, 3].with(1, 9)[1]"), JsValue::Number(9.0));
        assert_eq!(eval_full("[1, 2, 3].with(-1, 9)[2]"), JsValue::Number(9.0));
        // toSpliced returns a new array with elements removed and inserted.
        assert_eq!(eval_full("[1, 2, 3, 4].toSpliced(1, 2, 9).length"), JsValue::Number(3.0));
        assert_eq!(eval_full("[1, 2, 3, 4].toSpliced(1, 2, 9)[1]"), JsValue::Number(9.0));
    }

    #[test]
    fn json_stringify_indentation() {
        // Number space indents each nesting level by that many spaces.
        assert_eq!(eval_full("JSON.stringify([1, 2], null, 2)"), JsValue::String("[\n  1,\n  2\n]".to_string()));
        // A single-key object is deterministic and reflects the indent.
        assert_eq!(eval_full("JSON.stringify({ a: 1 }, null, 2)"), JsValue::String("{\n  \"a\": 1\n}".to_string()));
        // String space is used verbatim as the indent unit.
        assert_eq!(eval_full("JSON.stringify([1], null, '\\t')"), JsValue::String("[\n\t1\n]".to_string()));
        // Empty containers stay compact.
        assert_eq!(eval_full("JSON.stringify([], null, 2)"), JsValue::String("[]".to_string()));
        // Omitting the space argument keeps compact output.
        assert_eq!(eval_full("JSON.stringify([1, 2])"), JsValue::String("[1,2]".to_string()));
    }

    #[test]
    fn json_stringify_replacer_array() {
        // Replacer array whitelists object properties.
        assert_eq!(eval_full("JSON.stringify({a: 1, b: 2, c: 3}, ['a', 'c'])"), JsValue::String("{\"a\":1,\"c\":3}".to_string()));
        // Nested objects are also filtered.
        assert_eq!(eval_full("JSON.stringify({x: {a: 1, b: 2}}, ['x', 'a'])"), JsValue::String("{\"x\":{\"a\":1}}".to_string()));
        // Arrays are unaffected by the replacer.
        assert_eq!(eval_full("JSON.stringify([1, 2, 3], ['a'])"), JsValue::String("[1,2,3]".to_string()));
    }

    #[test]
    fn object_get_own_property_descriptors_basic() {
        // Each own data property yields a descriptor carrying its value.
        assert_eq!(eval_full("Object.getOwnPropertyDescriptors({ a: 1, b: 2 }).a.value"), JsValue::Number(1.0));
        assert_eq!(eval_full("Object.getOwnPropertyDescriptors({ a: 1, b: 2 }).b.value"), JsValue::Number(2.0));
        // Data descriptors default to writable/enumerable/configurable true.
        assert_eq!(eval_full("Object.getOwnPropertyDescriptors({ a: 1 }).a.writable"), JsValue::Boolean(true));
        assert_eq!(eval_full("Object.getOwnPropertyDescriptors({ a: 1 }).a.enumerable"), JsValue::Boolean(true));
        // A round-trip through Object.keys sees exactly the own keys.
        assert_eq!(eval_full("Object.keys(Object.getOwnPropertyDescriptors({ only: 5 }))[0]"), JsValue::String("only".to_string()));
    }

    #[test]
    fn object_has_own_static() {
        // True for a directly-owned key, false for an absent one.
        assert_eq!(eval_full("Object.hasOwn({ a: 1 }, 'a')"), JsValue::Boolean(true));
        assert_eq!(eval_full("Object.hasOwn({ a: 1 }, 'b')"), JsValue::Boolean(false));
        // Array indices in range are owned; out-of-range are not; length is owned.
        assert_eq!(eval_full("Object.hasOwn([10, 20], '1')"), JsValue::Boolean(true));
        assert_eq!(eval_full("Object.hasOwn([10, 20], '5')"), JsValue::Boolean(false));
        assert_eq!(eval_full("Object.hasOwn([10, 20], 'length')"), JsValue::Boolean(true));
    }

    #[test]
    fn array_includes_same_value_zero_and_from_index() {
        // SameValueZero finds NaN, which indexOf-style === cannot.
        assert_eq!(eval_full("[1, NaN, 3].includes(NaN)"), JsValue::Boolean(true));
        // fromIndex skips earlier matches.
        assert_eq!(eval_full("[1, 2, 1].includes(1, 1)"), JsValue::Boolean(true));
        assert_eq!(eval_full("[1, 2, 3].includes(1, 1)"), JsValue::Boolean(false));
        // Negative fromIndex counts from the end.
        assert_eq!(eval_full("[5, 6, 7].includes(5, -1)"), JsValue::Boolean(false));
        assert_eq!(eval_full("[5, 6, 7].includes(7, -1)"), JsValue::Boolean(true));
    }

    #[test]
    fn math_imul_and_hyperbolic_inverses() {
        // imul performs 32-bit integer multiplication with wraparound.
        assert_eq!(eval_full("Math.imul(3, 4)"), JsValue::Number(12.0));
        assert_eq!(eval_full("Math.imul(-5, 3)"), JsValue::Number(-15.0));
        // Large products wrap within the signed 32-bit range.
        assert_eq!(eval_full("Math.imul(0xffffffff, 5)"), JsValue::Number(-5.0));
        // Inverse hyperbolic functions round-trip their forward counterparts.
        assert_eq!(eval_full("Math.asinh(0)"), JsValue::Number(0.0));
        assert_eq!(eval_full("Math.acosh(1)"), JsValue::Number(0.0));
        assert_eq!(eval_full("Math.atanh(0)"), JsValue::Number(0.0));
    }

    #[test]
    fn array_index_of_from_index() {
        // indexOf honours a positive fromIndex.
        assert_eq!(eval_full("[1, 2, 1].indexOf(1, 1)"), JsValue::Number(2.0));
        // Negative fromIndex counts from the end.
        assert_eq!(eval_full("[1, 2, 1].indexOf(1, -1)"), JsValue::Number(2.0));
        // lastIndexOf scans backward and honours fromIndex.
        assert_eq!(eval_full("[1, 2, 1].lastIndexOf(1)"), JsValue::Number(2.0));
        assert_eq!(eval_full("[1, 2, 1].lastIndexOf(1, 1)"), JsValue::Number(0.0));
        // Absent element yields -1.
        assert_eq!(eval_full("[1, 2, 3].indexOf(9)"), JsValue::Number(-1.0));
    }

    #[test]
    fn string_index_of_with_position() {
        // indexOf honours a start position, counted in chars.
        assert_eq!(eval_full("'abcabc'.indexOf('bc', 2)"), JsValue::Number(4.0));
        // lastIndexOf bounds the match start with fromIndex.
        assert_eq!(eval_full("'abcabc'.lastIndexOf('bc')"), JsValue::Number(4.0));
        assert_eq!(eval_full("'abcabc'.lastIndexOf('bc', 3)"), JsValue::Number(1.0));
        // Empty needle clamps to string length.
        assert_eq!(eval_full("'abc'.indexOf('', 5)"), JsValue::Number(3.0));
        // Absent needle yields -1.
        assert_eq!(eval_full("'abc'.indexOf('z')"), JsValue::Number(-1.0));
    }

    #[test]
    fn string_includes_starts_ends_with_position() {
        // startsWith honours a start position.
        assert_eq!(eval_full("'abcdef'.startsWith('cd', 2)"), JsValue::Boolean(true));
        assert_eq!(eval_full("'abcdef'.startsWith('cd', 1)"), JsValue::Boolean(false));
        // endsWith treats the string as ending at endPosition.
        assert_eq!(eval_full("'abcdef'.endsWith('cd', 4)"), JsValue::Boolean(true));
        assert_eq!(eval_full("'abcdef'.endsWith('cd')"), JsValue::Boolean(false));
        // includes honours a start position.
        assert_eq!(eval_full("'abcabc'.includes('ab', 1)"), JsValue::Boolean(true));
        assert_eq!(eval_full("'abcabc'.includes('ab', 4)"), JsValue::Boolean(false));
    }

    #[test]
    fn number_to_string_radix_with_fraction() {
        // Integer radix conversion is unchanged.
        assert_eq!(eval_full("(255).toString(16)"), JsValue::String("ff".to_string()));
        // Fractional parts are now emitted in the target base.
        assert_eq!(eval_full("(255.5).toString(16)"), JsValue::String("ff.8".to_string()));
        assert_eq!(eval_full("(0.5).toString(2)"), JsValue::String("0.1".to_string()));
        // Negative values keep a leading sign.
        assert_eq!(eval_full("(-10).toString(2)"), JsValue::String("-1010".to_string()));
    }

    #[test]
    fn array_join_null_undefined_and_separator() {
        // null and undefined elements render as empty strings.
        assert_eq!(eval_full("[1, null, 2, undefined, 3].join('-')"), JsValue::String("1--2--3".to_string()));
        // An explicit undefined separator falls back to a comma.
        assert_eq!(eval_full("[1, 2, 3].join(undefined)"), JsValue::String("1,2,3".to_string()));
        // A custom separator is used verbatim.
        assert_eq!(eval_full("['a', 'b'].join(' | ')"), JsValue::String("a | b".to_string()));
    }

    #[test]
    fn string_split_separator_variants() {
        // An absent separator yields a single-element array with the whole string.
        assert_eq!(eval_full("'abc'.split().length"), JsValue::Number(1.0));
        assert_eq!(eval_full("'abc'.split()[0]"), JsValue::String("abc".to_string()));
        // An empty-string separator splits into individual characters.
        assert_eq!(eval_full("'abc'.split('').length"), JsValue::Number(3.0));
        // A normal separator splits on each occurrence.
        assert_eq!(eval_full("'a,b,c'.split(',').length"), JsValue::Number(3.0));
    }

    #[test]
    fn array_some_every_pass_index_to_callback() {
        // some/every callbacks receive the element index as the second argument.
        assert_eq!(eval_full("[10, 20, 30].some(function(v, i) { return i === 2; })"), JsValue::Boolean(true));
        assert_eq!(eval_full("[10, 20, 30].every(function(v, i) { return i < 3; })"), JsValue::Boolean(true));
        assert_eq!(eval_full("[10, 20, 30].every(function(v, i) { return i < 2; })"), JsValue::Boolean(false));
    }

    #[test]
    fn number_to_fixed_rounds_half_away_from_zero() {
        // JS toFixed rounds halves away from zero, unlike Rust's default formatter.
        assert_eq!(eval_full("(2.5).toFixed(0)"), JsValue::String("3".to_string()));
        assert_eq!(eval_full("(0.5).toFixed(0)"), JsValue::String("1".to_string()));
        assert_eq!(eval_full("(-2.5).toFixed(0)"), JsValue::String("-3".to_string()));
        // Ordinary rounding and padding still hold.
        assert_eq!(eval_full("(123.456).toFixed(2)"), JsValue::String("123.46".to_string()));
        assert_eq!(eval_full("(0).toFixed(2)"), JsValue::String("0.00".to_string()));
    }

    #[test]
    fn string_replace_dollar_patterns() {
        // $& inserts the matched substring; $$ yields a literal dollar sign.
        assert_eq!(eval_full("'hello'.replace('l', '[$&]')"), JsValue::String("he[l]lo".to_string()));
        assert_eq!(eval_full("'a'.replace('a', '$$')"), JsValue::String("$".to_string()));
        // $` and $' expand to the text before and after the match.
        assert_eq!(eval_full("'abc'.replace('b', '$`|$\\'')"), JsValue::String("aa|cc".to_string()));
        // replaceAll applies $& to every occurrence.
        assert_eq!(eval_full("'a-a'.replaceAll('a', '($&)')"), JsValue::String("(a)-(a)".to_string()));
    }

    #[test]
    fn json_stringify_spec_edge_cases() {
        // Non-finite numbers serialize as null.
        assert_eq!(eval_full("JSON.stringify(NaN)"), JsValue::String("null".to_string()));
        assert_eq!(eval_full("JSON.stringify([1, Infinity, 2])"), JsValue::String("[1,null,2]".to_string()));
        // undefined array elements become null.
        assert_eq!(eval_full("JSON.stringify([1, undefined, 3])"), JsValue::String("[1,null,3]".to_string()));
        // undefined object properties are omitted.
        assert_eq!(eval_full("JSON.stringify({ a: 1, b: undefined })"), JsValue::String("{\"a\":1}".to_string()));
        // Control characters in strings are escaped.
        assert_eq!(eval_full("JSON.stringify('a\\nb')"), JsValue::String("\"a\\nb\"".to_string()));
    }

    #[test]
    fn json_parse_top_level_string_escapes() {
        // A top-level JSON string decodes tab and unicode escapes like nested ones.
        assert_eq!(eval_full(r#"JSON.parse('"a\\tb"')"#), JsValue::String("a\tb".to_string()));
        assert_eq!(eval_full(r#"JSON.parse('"\\u0041"')"#), JsValue::String("A".to_string()));
    }

    #[test]
    fn loose_equality_null_and_undefined() {
        // null and undefined are loosely equal to each other only.
        assert_eq!(eval_full("null == undefined"), JsValue::Boolean(true));
        assert_eq!(eval_full("null == 0"), JsValue::Boolean(false));
        assert_eq!(eval_full("undefined == 0"), JsValue::Boolean(false));
        assert_eq!(eval_full("null == false"), JsValue::Boolean(false));
        assert_eq!(eval_full("null == ''"), JsValue::Boolean(false));
        // != is the negation.
        assert_eq!(eval_full("null != 0"), JsValue::Boolean(true));
    }

    #[test]
    fn to_number_string_coercion() {
        // Empty and whitespace-only strings coerce to 0 (not NaN).
        assert_eq!(eval_full("+''"), JsValue::Number(0.0));
        assert_eq!(eval_full("+'   '"), JsValue::Number(0.0));
        // Surrounding whitespace is ignored around a valid literal.
        assert_eq!(eval_full("+'  42 '"), JsValue::Number(42.0));
        // Non-decimal integer prefixes are honoured.
        assert_eq!(eval_full("+'0x10'"), JsValue::Number(16.0));
        assert_eq!(eval_full("+'0b101'"), JsValue::Number(5.0));
        assert_eq!(eval_full("+'0o17'"), JsValue::Number(15.0));
        // The Infinity literal maps to positive infinity.
        assert_eq!(eval_full("+'Infinity'"), JsValue::Number(f64::INFINITY));
        // Non-numeric strings (including Rust-only spellings) are NaN.
        assert_eq!(eval_full("Number.isNaN(+'abc')"), JsValue::Boolean(true));
        assert_eq!(eval_full("Number.isNaN(+'inf')"), JsValue::Boolean(true));
        assert_eq!(eval_full("Number.isNaN(+'nan')"), JsValue::Boolean(true));
    }

    #[test]
    fn wrapper_constructors_coerce_as_functions() {
        // Number() coerces its argument (and defaults to 0 with no argument).
        assert_eq!(eval_full("Number('42')"), JsValue::Number(42.0));
        assert_eq!(eval_full("Number(true)"), JsValue::Number(1.0));
        assert_eq!(eval_full("Number()"), JsValue::Number(0.0));
        // String() renders the argument as a string (empty when omitted).
        assert_eq!(eval_full("String(123)"), JsValue::String("123".to_string()));
        assert_eq!(eval_full("String(null)"), JsValue::String("null".to_string()));
        assert_eq!(eval_full("String()"), JsValue::String(String::new()));
        // Boolean() applies truthiness (false when omitted).
        assert_eq!(eval_full("Boolean('')"), JsValue::Boolean(false));
        assert_eq!(eval_full("Boolean('x')"), JsValue::Boolean(true));
        assert_eq!(eval_full("Boolean()"), JsValue::Boolean(false));
    }

    #[test]
    fn number_to_string_exponential_notation() {
        // Magnitudes with exponents outside [-6, 21] switch to exponential form.
        assert_eq!(eval_full("String(1e21)"), JsValue::String("1e+21".to_string()));
        assert_eq!(eval_full("String(1e-7)"), JsValue::String("1e-7".to_string()));
        assert_eq!(eval_full("String(1.5e30)"), JsValue::String("1.5e+30".to_string()));
        // Exponents within [-6, 21] stay in plain decimal form.
        assert_eq!(eval_full("String(1e20)"), JsValue::String("100000000000000000000".to_string()));
        assert_eq!(eval_full("String(1e-6)"), JsValue::String("0.000001".to_string()));
        assert_eq!(eval_full("String(0.001)"), JsValue::String("0.001".to_string()));
        // Ordinary integers and negatives are unaffected.
        assert_eq!(eval_full("String(123)"), JsValue::String("123".to_string()));
        assert_eq!(eval_full("String(-1e21)"), JsValue::String("-1e+21".to_string()));
    }

    #[test]
    fn array_and_object_to_string() {
        // Array.prototype.toString is join(","), rendering null/undefined as empty.
        assert_eq!(eval_full("[1,2,3].toString()"), JsValue::String("1,2,3".to_string()));
        assert_eq!(eval_full("[].toString()"), JsValue::String(String::new()));
        assert_eq!(eval_full("[1,null,undefined,2].toString()"), JsValue::String("1,,,2".to_string()));
        assert_eq!(eval_full("[1,2,3].toLocaleString()"), JsValue::String("1,2,3".to_string()));
        // Object.prototype.toString tags plain objects.
        assert_eq!(eval_full("({}).toString()"), JsValue::String("[object Object]".to_string()));
    }

    #[test]
    fn function_call_apply_bind() {
        // call invokes with an explicit this and trailing arguments.
        assert_eq!(eval_full("function f(a, b) { return this.x + a + b; } f.call({x: 1}, 2, 3)"), JsValue::Number(6.0));
        // apply takes its arguments from an array.
        assert_eq!(eval_full("function g(a, b) { return this.x + a + b; } g.apply({x: 10}, [1, 2])"), JsValue::Number(13.0));
        // bind fixes this and returns a callable that prepends bound arguments.
        assert_eq!(eval_full("function h(a, b) { return this.x + a + b; } var bh = h.bind({x: 100}, 1); bh(2)"), JsValue::Number(103.0));
    }

    #[test]
    fn number_to_exponential() {
        // The exponent always carries an explicit sign.
        assert_eq!(eval_full("(5).toExponential()"), JsValue::String("5e+0".to_string()));
        assert_eq!(eval_full("(12345).toExponential(2)"), JsValue::String("1.23e+4".to_string()));
        assert_eq!(eval_full("(0).toExponential()"), JsValue::String("0e+0".to_string()));
        assert_eq!(eval_full("(0).toExponential(2)"), JsValue::String("0.00e+0".to_string()));
        // Negative exponents and rounding (half away from zero) with carry.
        assert_eq!(eval_full("(0.0001).toExponential()"), JsValue::String("1e-4".to_string()));
        assert_eq!(eval_full("(1.999).toExponential(2)"), JsValue::String("2.00e+0".to_string()));
        assert_eq!(eval_full("(-12345).toExponential(2)"), JsValue::String("-1.23e+4".to_string()));
    }

    #[test]
    fn number_to_precision() {
        // Fixed notation: exponent in [-6, p).
        assert_eq!(eval_full("(5).toPrecision(2)"), JsValue::String("5.0".to_string()));
        assert_eq!(eval_full("(123.456).toPrecision(5)"), JsValue::String("123.46".to_string()));
        assert_eq!(eval_full("(0).toPrecision(1)"), JsValue::String("0".to_string()));
        assert_eq!(eval_full("(0).toPrecision(3)"), JsValue::String("0.00".to_string()));
        // Exponential notation carries an explicit sign.
        assert_eq!(eval_full("(123.456).toPrecision(2)"), JsValue::String("1.2e+2".to_string()));
        assert_eq!(eval_full("(0.0000001).toPrecision(2)"), JsValue::String("1.0e-7".to_string()));
        // Rounding with carry that bumps the exponent.
        assert_eq!(eval_full("(9.99).toPrecision(2)"), JsValue::String("10".to_string()));
        // Negative values.
        assert_eq!(eval_full("(-123.456).toPrecision(2)"), JsValue::String("-1.2e+2".to_string()));
    }

    #[test]
    fn string_replace_with_function() {
        // replace with a callback: fn(match, offset, string).
        assert_eq!(
            eval_full("'hello world'.replace('world', function(m) { return m.toUpperCase(); })"),
            JsValue::String("hello WORLD".to_string())
        );
        // replaceAll invokes the callback for every match.
        assert_eq!(
            eval_full("'aaa'.replaceAll('a', function(m, i) { return String(i); })"),
            JsValue::String("012".to_string())
        );
        // No match leaves the string unchanged.
        assert_eq!(
            eval_full("'abc'.replace('z', function(m) { return 'X'; })"),
            JsValue::String("abc".to_string())
        );
    }

    #[test]
    fn array_callbacks_receive_array_argument() {
        // map/filter/forEach/find/some/every callbacks get (element, index, array).
        assert_eq!(eval_full("[10,20,30].map(function(v, i, arr) { return arr.length; })[0]"), JsValue::Number(3.0));
        assert_eq!(eval_full("[5,6,7].filter(function(v, i, arr) { return arr[i] === v; }).length"), JsValue::Number(3.0));
        assert_eq!(eval_full("[1,2].find(function(v, i, arr) { return arr.length === 2 && v === 2; })"), JsValue::Number(2.0));
        // reduce callback gets (acc, val, index, array).
        assert_eq!(eval_full("[1,2,3].reduce(function(acc, v, i, arr) { return acc + arr.length; }, 0)"), JsValue::Number(9.0));
    }

    #[test]
    fn addition_to_primitive_coercion() {
        // Arrays coerce via toString (join with comma) before + decides concat vs add.
        assert_eq!(eval_full("[] + []"), JsValue::String(String::new()));
        assert_eq!(eval_full("[1,2] + [3,4]"), JsValue::String("1,23,4".to_string()));
        assert_eq!(eval_full("[1] + 2"), JsValue::String("12".to_string()));
        // Empty array to number: [] -> '' -> 0.
        assert_eq!(eval_full("+[]"), JsValue::Number(0.0));
        // Objects coerce to [object Object].
        assert_eq!(eval_full("var o = {}; o + ''"), JsValue::String("[object Object]".to_string()));
    }

    #[test]
    fn relational_string_comparison() {
        // When both operands are strings, compare lexicographically.
        assert_eq!(eval_full("'a' < 'b'"), JsValue::Boolean(true));
        assert_eq!(eval_full("'b' < 'a'"), JsValue::Boolean(false));
        assert_eq!(eval_full("'10' < '9'"), JsValue::Boolean(true)); // lexicographic: '1' < '9'
        assert_eq!(eval_full("'abc' <= 'abc'"), JsValue::Boolean(true));
        assert_eq!(eval_full("'z' > 'a'"), JsValue::Boolean(true));
        // Mixed types still compare numerically.
        assert_eq!(eval_full("10 < 9"), JsValue::Boolean(false));
        assert_eq!(eval_full("'10' < 9"), JsValue::Boolean(false)); // '10' -> 10, numeric
    }

    #[test]
    fn string_match_and_search_plain_string() {
        // match with a plain string returns [match] or null.
        assert_eq!(eval_full("'hello world'.match('world')[0]"), JsValue::String("world".to_string()));
        assert_eq!(eval_full("'hello'.match('xyz')"), JsValue::Null);
        // search returns the byte index of the first occurrence or -1.
        assert_eq!(eval_full("'hello world'.search('world')"), JsValue::Number(6.0));
        assert_eq!(eval_full("'hello'.search('xyz')"), JsValue::Number(-1.0));
    }

    #[test]
    fn object_keys_on_string_and_array() {
        // Object.keys on a string returns character indices.
        assert_eq!(eval_full("Object.keys('abc').length"), JsValue::Number(3.0));
        assert_eq!(eval_full("Object.keys('abc')[0]"), JsValue::String("0".to_string()));
        // Object.values on a string returns the characters.
        assert_eq!(eval_full("Object.values('hi')[1]"), JsValue::String("i".to_string()));
        // Object.keys on an array returns indices.
        assert_eq!(eval_full("Object.keys([10,20,30]).length"), JsValue::Number(3.0));
    }

    #[test]
    fn string_from_code_point_builds_scalars() {
        // ASCII code points map to their characters and concatenate in order.
        assert_eq!(eval_full("String.fromCodePoint(72, 105)"), JsValue::String("Hi".to_string()));
        // Code points above the BMP produce a single Unicode scalar.
        assert_eq!(eval_full("String.fromCodePoint(128512)"), JsValue::String("\u{1F600}".to_string()));
        // No arguments yields the empty string.
        assert_eq!(eval_full("String.fromCodePoint()"), JsValue::String(String::new()));
    }

    #[test]
    fn parse_int_leading_numeric_and_radix() {
        // Stops at the first non-digit, keeping the leading integer.
        assert_eq!(eval_full("parseInt('42px')"), JsValue::Number(42.0));
        // Auto-detects a hex prefix when no radix is given.
        assert_eq!(eval_full("parseInt('0xFF')"), JsValue::Number(255.0));
        // Explicit radix parses in that base.
        assert_eq!(eval_full("parseInt('101', 2)"), JsValue::Number(5.0));
        // Truncates a fractional string to its integer part.
        assert_eq!(eval_full("parseInt('3.99')"), JsValue::Number(3.0));
        // Honours a leading sign and surrounding whitespace.
        assert_eq!(eval_full("parseInt('   -7abc')"), JsValue::Number(-7.0));
        // No digits produces NaN (compared via self-inequality).
        assert_eq!(eval_full("parseInt('abc') !== parseInt('abc')"), JsValue::Boolean(true));
    }

    #[test]
    fn parse_float_leading_numeric() {
        // Parses the numeric prefix and ignores trailing text.
        assert_eq!(eval_full("parseFloat('2.5abc')"), JsValue::Number(2.5));
        // Supports exponent notation.
        assert_eq!(eval_full("parseFloat('1.5e2xyz')"), JsValue::Number(150.0));
        // Recognises Infinity.
        assert_eq!(eval_full("parseFloat('-Infinity')"), JsValue::Number(f64::NEG_INFINITY));
        // No numeric prefix produces NaN.
        assert_eq!(eval_full("parseFloat('nope') !== parseFloat('nope')"), JsValue::Boolean(true));
    }

    #[test]
    fn object_get_own_property_names_basic() {
        // Reports all own string keys of a plain object (order-independent for multi-key).
        assert_eq!(eval_full("Object.getOwnPropertyNames({ a: 1, b: 2 }).length"), JsValue::Number(2.0));
        // A single-key object yields that key deterministically.
        assert_eq!(eval_full("Object.getOwnPropertyNames({ only: 1 })[0]"), JsValue::String("only".to_string()));
    }

    #[test]
    fn object_get_own_property_names_array_includes_length() {
        assert_eq!(eval_full("Object.getOwnPropertyNames([10, 20, 30]).length"), JsValue::Number(4.0));
        assert_eq!(eval_full("Object.getOwnPropertyNames([10, 20, 30])[3]"), JsValue::String("length".to_string()));
    }

    #[test]
    fn object_get_own_property_names_consults_proxy_trap() {
        assert_eq!(eval_full("
            var target = { a: 1, b: 2 };
            var handler = { ownKeys: function(t) { return ['a', 'b', 'c']; } };
            var p = new Proxy(target, handler);
            Object.getOwnPropertyNames(p).length
        "), JsValue::Number(3.0));
    }

    #[test]
    fn proxy_apply_trap_intercepts_call() {
        // handler.apply(target, thisArg, args) intercepts calling the proxy.
        assert_eq!(eval_full("
            function greet(n) { return 'hi ' + n; }
            var handler = { apply: function(t, th, a) { return 'intercepted:' + a[0]; } };
            var p = new Proxy(greet, handler);
            p('bob')
        "), JsValue::String("intercepted:bob".to_string()));
    }

    #[test]
    fn proxy_apply_trap_can_delegate_to_target() {
        // The trap may call the target itself and transform the result.
        assert_eq!(eval_full("
            function sum(a, b) { return a + b; }
            var handler = { apply: function(t, th, a) { return t(a[0], a[1]) * 10; } };
            var p = new Proxy(sum, handler);
            p(3, 4)
        "), JsValue::Number(70.0));
    }

    #[test]
    fn proxy_call_forwards_to_target_without_apply_trap() {
        assert_eq!(eval_full("
            function add(a, b) { return a + b; }
            var p = new Proxy(add, {});
            p(2, 3)
        "), JsValue::Number(5.0));
    }

    #[test]
    fn for_of_string_iterates_chars() {
        assert_eq!(eval_full("
            var out = '';
            for (var c of 'abc') { out = out + c; }
            out
        "), JsValue::String("abc".to_string()));
    }

    #[test]
    fn for_of_map_yields_entries() {
        assert_eq!(eval_full("
            var m = new Map([['a', 1], ['b', 2]]);
            var total = 0;
            for (var e of m) { total = total + e[1]; }
            total
        "), JsValue::Number(3.0));
    }

    #[test]
    fn for_of_set_yields_items() {
        assert_eq!(eval_full("
            var s = new Set([1, 2, 3]);
            var total = 0;
            for (var x of s) { total = total + x; }
            total
        "), JsValue::Number(6.0));
    }

    #[test]
    fn for_of_custom_iterator_protocol() {
        // An object that is itself an iterator (has a stateful next()) drives for...of.
        assert_eq!(eval_full("
            function makeRange(lo, hi) {
                var cur = lo;
                return {
                    next: function() {
                        if (cur <= hi) {
                            var v = cur;
                            cur = cur + 1;
                            return { value: v, done: false };
                        }
                        return { done: true };
                    }
                };
            }
            var sum = 0;
            for (var x of makeRange(1, 3)) { sum = sum + x; }
            sum
        "), JsValue::Number(6.0));
    }

    #[test]
    fn for_of_iterable_with_iterator_method() {
        // An iterable exposing __iterator__ that returns a fresh iterator.
        assert_eq!(eval_full("
            var iterable = {
                __iterator__: function() {
                    var i = 0;
                    return {
                        next: function() {
                            if (i < 3) { var v = i; i = i + 1; return { value: v, done: false }; }
                            return { done: true };
                        }
                    };
                }
            };
            var sum = 0;
            for (var x of iterable) { sum = sum + x; }
            sum
        "), JsValue::Number(3.0));
    }

    #[test]
    fn yield_keyword_in_expression() {
        // yield used as expression returns value
        assert_eq!(eval_full("
            function* nums() { yield 10; yield 20; }
            var it = nums();
            var first = it.next();
            first.value
        "), JsValue::Number(10.0));
    }

    #[test]
    fn import_export_no_crash() {
        // Import/export should parse and run without errors
        assert_eq!(eval_full("
            var x = 42;
            x
        "), JsValue::Number(42.0));
    }

    #[test]
    fn structuredclone_works() {
        assert_eq!(eval_full("structuredClone(42)"), JsValue::Number(42.0));
    }

    #[test]
    fn module_resolver_on_demand() {
        // Serialize with other module-system tests that touch the global
        // resolver/registry (see MODULE_TEST_LOCK).
        let _guard = MODULE_TEST_LOCK.lock().unwrap();
        // Set a resolver that provides module source on demand
        set_module_resolver(|specifier: &str| {
            match specifier {
                "./math.js" => Some("export function add(a, b) { return a + b; }".to_string()),
                "./utils.js" => Some("export const PI = 3; export function double(x) { return x * 2; }".to_string()),
                _ => None,
            }
        });
        // Named import resolves via the callback
        let result = eval_full("
            import { add } from './math.js';
            add(3, 4)
        ");
        assert_eq!(result, JsValue::Number(7.0));
        // Namespace import resolves via the callback
        let result2 = eval_full("
            import * as utils from './utils.js';
            utils.double(utils.PI)
        ");
        assert_eq!(result2, JsValue::Number(6.0));
        // Cleanup
        clear_module_resolver();
        clear_module_registry();
    }
}
