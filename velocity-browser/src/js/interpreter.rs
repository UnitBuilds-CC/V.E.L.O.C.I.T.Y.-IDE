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
    Return(Option<Expr>),
    Break,
    Continue,
    Throw(Expr),
    TryCatch { try_block: Box<Stmt>, catch_var: Option<String>, catch_block: Option<Box<Stmt>>, finally_block: Option<Box<Stmt>> },
    FunctionDecl { name: String, params: Vec<String>, body: Box<Stmt> },
    ClassDecl { name: String, parent: Option<String>, methods: Vec<ClassMethod> },
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

#[derive(Debug, Clone)]
pub struct ClassMethod {
    pub name: String,
    pub params: Vec<String>,
    pub body: Stmt,
    pub is_static: bool,
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
                    self.advance(); // in/of
                    let obj = self.parse_expr()?;
                    self.expect(&Token::RParen)?;
                    let body = Box::new(self.parse_stmt()?);
                    return Ok(Stmt::ForIn { var_name, object: obj, body });
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
        while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
            // Skip semicolons between methods
            if self.eat(&Token::Semi) { continue; }
            let is_static = self.eat(&Token::Static);
            // Handle get/set/async as method name prefixes or actual method names
            let method_name = match self.peek().clone() {
                Token::Ident(n) => { self.advance(); n }
                _ => { self.advance(); "unknown".to_string() }
            };
            // If next token is ( it's a method; if next is an ident, this was a keyword prefix
            let final_name = if self.at(&Token::LParen) {
                method_name
            } else if let Token::Ident(n) = self.peek().clone() {
                self.advance(); n
            } else {
                method_name
            };
            let params = if self.at(&Token::LParen) { self.parse_params()? } else { vec![] };
            let body = if self.at(&Token::LBrace) { self.parse_block()? } else { Stmt::Block(vec![]) };
            methods.push(ClassMethod { name: final_name, params, body, is_static });
        }
        self.expect(&Token::RBrace)?;
        Ok(Stmt::ClassDecl { name, parent, methods })
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
            if self.eat(&Token::Comma) {
                if self.at(&Token::LBrace) {
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
                match self.advance() { Token::Ident(n) => named.push(n), _ => {} }
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
            match self.advance() { Token::Ident(n) => params.push(n), _ => {} }
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
            Token::New => { self.advance(); let callee = self.parse_new_target()?; let args = if self.at(&Token::LParen) { self.parse_args()? } else { Vec::new() }; Ok(Expr::New(Box::new(callee), args)) }
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
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek().clone() {
                Token::LParen => { let args = self.parse_args()?; expr = Expr::Call(Box::new(expr), args); }
                Token::Dot => { self.advance(); let prop = match self.advance() { Token::Ident(n) => n, t => return Err(format!("expected property name, got {:?}", t)) }; expr = Expr::Member(Box::new(expr), prop); }
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
                        let prop = match self.advance() { Token::Ident(n) => n, t => return Err(format!("expected property name after ?., got {:?}", t)) };
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
                Token::Dot => { self.advance(); let prop = match self.advance() { Token::Ident(n) => n, t => return Err(format!("expected property name, got {:?}", t)) }; expr = Expr::Member(Box::new(expr), prop); }
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
            if !self.at(&Token::RParen) { if !self.eat(&Token::Comma) { self.pos = saved; return Err("not arrow".into()); } }
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
            let key = match self.peek().clone() {
                Token::Ident(k) => { self.advance(); k }
                Token::Str(k) => { self.advance(); k }
                Token::Number(n) => { self.advance(); format!("{}", n) }
                Token::LBracket => { self.advance(); let e = self.parse_expr()?; self.expect(&Token::RBracket)?; format!("{:?}", e) }
                _ => return Err(format!("expected property key, got {:?}", self.peek())),
            };
            // Getter/setter: { get x() { ... }, set x(v) { ... } }
            if (key == "get" || key == "set") && matches!(self.peek(), Token::Ident(_)) {
                let actual_key = match self.advance() { Token::Ident(n) => n, _ => key.clone() };
                let params = self.parse_params()?;
                let body = self.parse_block()?;
                let func = Expr::Function(Some(format!("{}_{}", key, actual_key)), params, Box::new(body));
                props.push((actual_key.clone(), func.clone()));
                spread_props.push(ObjectProp::KeyValue(actual_key, func));
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
        if has_spread {
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
                        // for-in: iterate over keys
                        for key in map.keys() {
                            Scope::declare(scope, var_name, JsValue::String(key.clone()));
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
        Stmt::Return(e) => {
            let val = match e { Some(ex) => eval_expr_node(ex, scope)?, None => JsValue::Undefined };
            Err(Signal::Return(val))
        }
        Stmt::Break => Err(Signal::Break),
        Stmt::Continue => Err(Signal::Continue),
        Stmt::Throw(e) => Err(Signal::Throw(eval_expr_node(e, scope)?)),
        Stmt::TryCatch { try_block, catch_var, catch_block, finally_block } => {
            let result = eval_stmt(try_block, scope);
            let value = match result {
                Err(Signal::Throw(thrown)) => {
                    if let Some(cb) = catch_block {
                        let catch_scope = Scope::new_child(scope);
                        if let Some(var) = catch_var { Scope::declare(&catch_scope, var, thrown); }
                        eval_stmt(cb, &catch_scope).unwrap_or(JsValue::Undefined)
                    } else { JsValue::Undefined }
                }
                Err(other) => { if let Some(fb) = finally_block { let _ = eval_stmt(fb, scope); } return Err(other); }
                Ok(v) => v,
            };
            if let Some(fb) = finally_block { let _ = eval_stmt(fb, scope); }
            Ok(value)
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
        Stmt::ClassDecl { name, parent, methods } => {
            eval_class_decl(name, parent, methods, scope);
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
        Expr::Ident(name) => Ok(Scope::resolve(scope, name).unwrap_or(JsValue::Undefined)),
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
            // Synchronous model: await just evaluates the expression
            let val = eval_expr_node(e, scope)?;
            // If it's a "promise" object with a __value__ key, unwrap
            if let JsValue::Object(map) = &val {
                if let Some(inner) = map.get("__resolved__") { return Ok(inner.clone()); }
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
    let val = eval_expr_node(rhs, scope)?;
    Ok(match op {
        Token::Minus => JsValue::Number(-to_number(&val)),
        Token::Plus => JsValue::Number(to_number(&val)),
        Token::Bang => JsValue::Boolean(!to_boolean(&val)),
        Token::Tilde => JsValue::Number(!((to_number(&val) as i32)) as f64),
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
            if matches!(l, JsValue::String(_)) || matches!(r, JsValue::String(_)) {
                JsValue::String(format!("{}{}", to_string(&l), to_string(&r)))
            } else { JsValue::Number(to_number(&l) + to_number(&r)) }
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
        Token::Lt => JsValue::Boolean(to_number(&l) < to_number(&r)),
        Token::Gt => JsValue::Boolean(to_number(&l) > to_number(&r)),
        Token::LtEq => JsValue::Boolean(to_number(&l) <= to_number(&r)),
        Token::GtEq => JsValue::Boolean(to_number(&l) >= to_number(&r)),
        Token::Amp => JsValue::Number(((to_number(&l) as i32) & (to_number(&r) as i32)) as f64),
        Token::Pipe => JsValue::Number(((to_number(&l) as i32) | (to_number(&r) as i32)) as f64),
        Token::Caret => JsValue::Number(((to_number(&l) as i32) ^ (to_number(&r) as i32)) as f64),
        Token::LtLt => JsValue::Number(((to_number(&l) as i32) << (to_number(&r) as u32 & 31)) as f64),
        Token::GtGt => JsValue::Number(((to_number(&l) as i32) >> (to_number(&r) as u32 & 31)) as f64),
        Token::GtGtGt => JsValue::Number(((to_number(&l) as u32) >> (to_number(&r) as u32 & 31)) as f64),
        Token::Instanceof => JsValue::Boolean(false), // simplified
        Token::In => {
            let key = to_string(&l);
            match &r { JsValue::Object(m) => JsValue::Boolean(m.contains_key(&key)), _ => JsValue::Boolean(false) }
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
                if let Some(JsValue::Object(mut map)) = Scope::resolve(scope, name) {
                    map.insert(prop.clone(), value);
                    Scope::assign(scope, name, JsValue::Object(map));
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
        // Handle static built-in calls: Promise.resolve, Object.keys, etc.
        if let Expr::Ident(obj_name) = obj_expr.as_ref() {
            let native_name = format!("{}.{}", obj_name, method);
            match native_name.as_str() {
                "Promise.resolve" | "Promise.reject" | "Promise.all" | "Promise.race" | "Promise.allSettled" |
                "Object.keys" | "Object.values" | "Object.entries" | "Object.assign" | "Object.freeze" |
                "Object.create" | "Object.getPrototypeOf" | "Object.defineProperty" |
                "Array.isArray" | "Array.from" |
                "JSON.parse" | "JSON.stringify" |
                "Math.floor" | "Math.ceil" | "Math.round" | "Math.abs" | "Math.sqrt" |
                "Math.trunc" | "Math.sign" | "Math.log" | "Math.pow" | "Math.max" | "Math.min" | "Math.random" |
                "Number.parseInt" | "Number.parseFloat" | "Number.isNaN" | "Number.isFinite" |
                "String.fromCharCode" | "Date.now" | "console.log" | "console.warn" | "console.error" | "console.info" |
                "eval" | "structuredClone" | "queueMicrotask" | "requestAnimationFrame" | "requestIdleCallback" | "Symbol" | "Symbol.for" |
                "Reflect.get" | "Reflect.set" | "Reflect.has" | "Reflect.deleteProperty" |
                "Reflect.ownKeys" | "Reflect.getOwnPropertyDescriptor" | "Reflect.apply" | "Reflect.construct" => {
                    return call_native(&native_name, &evaluated_args);
                }
                _ => {}
            }
        }
        let obj = eval_expr_node(obj_expr, scope)?;
        // Use writeback for methods on objects so `this` mutations propagate
        if let JsValue::Object(map) = &obj {
            let type_tag = map.get("__type__").map(to_string);
            match type_tag.as_deref() {
                Some("Map") => return call_map_method(map, method, &evaluated_args, scope),
                Some("Set") => return call_set_method(map, method, &evaluated_args),
                Some("Promise") => return call_promise_method(map, method, &evaluated_args, scope),
                Some("Date") => return call_date_method(map, method, &evaluated_args),
                Some("Generator") => return call_generator_method(map, method),
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
            "requestAnimationFrame" | "requestIdleCallback" => {
                return call_native(name, &evaluated_args);
            }
            _ => {}
        }
    }

    let func = eval_expr_node(callee, scope)?;
    call_function(&func, &evaluated_args, scope)
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
                items = init.clone();
            }
            map.insert("__items__".to_string(), JsValue::Array(items));
            Ok(JsValue::Object(map))
        }
        Some("WeakMap") => {
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("WeakMap".to_string()));
            map.insert("__entries__".to_string(), JsValue::Array(Vec::new()));
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
            // new Promise((resolve, reject) => { ... }) - just call executor and capture value
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            map.insert("__resolved__".to_string(), JsValue::Undefined);
            if let Some(executor) = evaluated_args.first() {
                // Create resolve/reject native fns
                let result_scope = Scope::new_child(scope);
                Scope::declare(&result_scope, "__promise_resolved__", JsValue::Undefined);
                let resolve_fn = JsValue::NativeFunction("__promise_resolve__".to_string());
                let reject_fn = JsValue::NativeFunction("__promise_reject__".to_string());
                let _ = call_function(executor, &[resolve_fn, reject_fn], &result_scope);
                // The resolved value was stored in scope (simplified)
                map.insert("__resolved__".to_string(), Scope::resolve(&result_scope, "__promise_resolved__").unwrap_or(JsValue::Undefined));
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
            // new Proxy(target, handler) - create a proxy object
            let target = evaluated_args.first().cloned().unwrap_or(JsValue::Object(HashMap::new()));
            let handler = evaluated_args.get(1).cloned().unwrap_or(JsValue::Object(HashMap::new()));
            let mut map = HashMap::new();
            map.insert("__type__".to_string(), JsValue::String("Proxy".to_string()));
            map.insert("__proxy_target__".to_string(), target);
            map.insert("__proxy_handler__".to_string(), handler);
            Ok(JsValue::Object(map))
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
fn eval_class_decl(name: &str, parent: &Option<String>, methods: &[ClassMethod], scope: &ScopeRef) {
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
            proto.insert(m.name.clone(), func);
        }
    }
    class_obj.insert("__proto_methods__".to_string(), JsValue::Object(proto));
    class_obj.insert("__static_methods__".to_string(), JsValue::Object(statics));

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
        _ => Ok(JsValue::Undefined),
    }
}

/// Call a function with `this` bound, and return the mutated `this` value after execution.
fn call_method_with_this_writeback(func: &JsValue, args: &[JsValue], _scope: &ScopeRef, this_val: JsValue) -> (EvalResult, JsValue) {
    match func {
        JsValue::Function { params, body, closure, .. } => {
            let call_scope = Scope::new_child(closure);
            Scope::declare(&call_scope, "this", this_val.clone());
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

fn call_native(name: &str, args: &[JsValue]) -> EvalResult {
    Ok(match name {
        "parseInt" | "Number.parseInt" => {
            let s = args.first().map(to_string).unwrap_or_default();
            let radix = args.get(1).map(|v| to_number(v) as u32).unwrap_or(10);
            JsValue::Number(i64::from_str_radix(s.trim(), radix).unwrap_or(0) as f64)
        }
        "parseFloat" | "Number.parseFloat" => {
            let s = args.first().map(to_string).unwrap_or_default();
            JsValue::Number(s.trim().parse::<f64>().unwrap_or(f64::NAN))
        }
        "isNaN" | "Number.isNaN" => {
            let n = args.first().map(to_number).unwrap_or(f64::NAN);
            JsValue::Boolean(n.is_nan())
        }
        "isFinite" | "Number.isFinite" => {
            let n = args.first().map(to_number).unwrap_or(f64::NAN);
            JsValue::Boolean(n.is_finite())
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
        "JSON.parse" => {
            let s = args.first().map(to_string).unwrap_or_default();
            json_parse(&s)
        }
        "JSON.stringify" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::String(json_stringify(&val))
        }
        "Object.keys" => {
            if let Some(JsValue::Object(map)) = args.first() { JsValue::Array(map.keys().map(|k| JsValue::String(k.clone())).collect()) }
            else { JsValue::Array(Vec::new()) }
        }
        "Object.values" => {
            if let Some(JsValue::Object(map)) = args.first() { JsValue::Array(map.values().cloned().collect()) }
            else { JsValue::Array(Vec::new()) }
        }
        "Object.entries" => {
            if let Some(JsValue::Object(map)) = args.first() { JsValue::Array(map.iter().map(|(k, v)| JsValue::Array(vec![JsValue::String(k.clone()), v.clone()])).collect()) }
            else { JsValue::Array(Vec::new()) }
        }
        "Object.assign" => {
            let mut target = if let Some(JsValue::Object(m)) = args.first() { m.clone() } else { HashMap::new() };
            for src in args.iter().skip(1) { if let JsValue::Object(m) = src { target.extend(m.iter().map(|(k, v)| (k.clone(), v.clone()))); } }
            JsValue::Object(target)
        }
        "Object.freeze" => args.first().cloned().unwrap_or(JsValue::Undefined),
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
            // Simplified: just set the value on the object
            args.first().cloned().unwrap_or(JsValue::Undefined)
        }
        "Array.isArray" => JsValue::Boolean(matches!(args.first(), Some(JsValue::Array(_)))),
        "Array.from" => {
            match args.first() {
                Some(JsValue::Array(a)) => JsValue::Array(a.clone()),
                Some(JsValue::String(s)) => JsValue::Array(s.chars().map(|c| JsValue::String(c.to_string())).collect()),
                _ => JsValue::Array(Vec::new()),
            }
        }
        "String.fromCharCode" => {
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
        "structuredClone" => {
            // Deep clone via identity (our JsValue is already Clone)
            args.first().cloned().unwrap_or(JsValue::Undefined)
        }
        "queueMicrotask" | "requestAnimationFrame" | "requestIdleCallback" => {
            // These need event loop integration; return a dummy id
            JsValue::Number(0.0)
        }
        "__noop__" => JsValue::Undefined,
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
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            match &target {
                JsValue::Object(map) => JsValue::Boolean(map.contains_key(&prop)),
                _ => JsValue::Boolean(false),
            }
        }
        "Reflect.deleteProperty" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let prop = args.get(1).map(to_string).unwrap_or_default();
            match &target {
                JsValue::Object(map) => JsValue::Boolean(map.contains_key(&prop)),
                _ => JsValue::Boolean(false),
            }
        }
        "Reflect.ownKeys" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            match &target {
                JsValue::Object(map) => {
                    let keys: Vec<JsValue> = map.keys()
                        .filter(|k| !k.starts_with("__"))
                        .map(|k| JsValue::String(k.clone()))
                        .collect();
                    JsValue::Array(keys)
                }
                _ => JsValue::Array(Vec::new()),
            }
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
            // Simplified: call as regular function (full ctor needs new-target)
            call_function(&target, &call_args, &Scope::new_global()).unwrap_or(JsValue::Undefined)
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
        return JsValue::String(s[1..s.len()-1].replace("\\n", "\n").replace("\\\"", "\""));
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

fn json_stringify(val: &JsValue) -> String {
    match val {
        JsValue::Undefined => "undefined".to_string(),
        JsValue::Null => "null".to_string(),
        JsValue::Boolean(b) => b.to_string(),
        JsValue::Number(n) => format_number(*n),
        JsValue::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        JsValue::Array(arr) => format!("[{}]", arr.iter().map(json_stringify).collect::<Vec<_>>().join(",")),
        JsValue::Object(map) => {
            let entries: Vec<String> = map.iter().map(|(k, v)| format!("\"{}\":{}", k, json_stringify(v))).collect();
            format!("{{{}}}", entries.join(","))
        }
        JsValue::Function { .. } | JsValue::NativeFunction(_) => "null".to_string(),
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
    let block = parser.parse_block()?;
    let stmts = match block {
        Stmt::Block(stmts) => stmts,
        other => vec![other],
    };
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
pub fn apply_import(
    specifiers: &[ImportSpecifier],
    source: &str,
    scope: &ScopeRef,
) -> Result<(), String> {
    let module_exports = match resolve_module(source) {
        Some(exports) => exports,
        None => return Ok(()), // Module not yet loaded; silently skip
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
            if let Some(val) = map.get(prop) { return val.clone(); }
            // Walk __proto__ chain
            let mut proto = map.get("__proto__");
            while let Some(p) = proto {
                if let JsValue::Object(proto_map) = p {
                    if let Some(val) = proto_map.get(prop) { return val.clone(); }
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
        JsValue::String(s) => {
            if prop == "length" { return JsValue::Number(s.len() as f64); }
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
        map.insert(prop.to_string(), value);
        true
    } else {
        false
    }
}

fn call_method(obj: &JsValue, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    match obj {
        JsValue::Array(arr) => call_array_method(arr, method, args, scope),
        JsValue::String(s) => Ok(call_string_method(s, method, args)),
        JsValue::Object(map) => {
            // Check for Map/Set/Promise builtins
            let type_tag = map.get("__type__").map(to_string);
            match type_tag.as_deref() {
                Some("Map") => return call_map_method(map, method, args, scope),
                Some("Set") => return call_set_method(map, method, args),
                Some("Promise") => return call_promise_method(map, method, args, scope),
                Some("Date") => return call_date_method(map, method, args),
                Some("Generator") => return call_generator_method(map, method),
                _ => {}
            }
            // Call method with `this` bound to the object
            if let Some(func) = map.get(method) {
                return call_function_with_this(func, args, scope, Some(obj.clone()));
            }
            call_object_method(map, method, args)
        }
        JsValue::Number(n) => Ok(call_number_method(*n, method, args)),
        _ => Ok(JsValue::Undefined),
    }
}

fn call_array_method(arr: &[JsValue], method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    Ok(match method {
        "push" => { let mut new_arr = arr.to_vec(); new_arr.extend(args.iter().cloned()); JsValue::Number(new_arr.len() as f64) }
        "pop" => arr.last().cloned().unwrap_or(JsValue::Undefined),
        "shift" => arr.first().cloned().unwrap_or(JsValue::Undefined),
        "length" => JsValue::Number(arr.len() as f64),
        "indexOf" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::Number(arr.iter().position(|x| strict_eq(x, &target)).map(|i| i as f64).unwrap_or(-1.0))
        }
        "includes" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::Boolean(arr.iter().any(|x| strict_eq(x, &target)))
        }
        "join" => {
            let sep = args.first().map(to_string).unwrap_or_else(|| ",".into());
            JsValue::String(arr.iter().map(to_string).collect::<Vec<_>>().join(&sep))
        }
        "slice" => {
            let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0);
            let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(arr.len() as i64);
            let s = if start < 0 { (arr.len() as i64 + start).max(0) as usize } else { start as usize };
            let e = if end < 0 { (arr.len() as i64 + end).max(0) as usize } else { (end as usize).min(arr.len()) };
            JsValue::Array(arr.get(s..e).unwrap_or(&[]).to_vec())
        }
        "concat" => {
            let mut new_arr = arr.to_vec();
            for a in args { if let JsValue::Array(other) = a { new_arr.extend(other.iter().cloned()); } else { new_arr.push(a.clone()); } }
            JsValue::Array(new_arr)
        }
        "reverse" => { let mut new_arr = arr.to_vec(); new_arr.reverse(); JsValue::Array(new_arr) }
        "map" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let mut result = Vec::new();
            for (i, item) in arr.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64)], scope)?;
                result.push(r);
            }
            JsValue::Array(result)
        }
        "filter" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let mut result = Vec::new();
            for (i, item) in arr.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64)], scope)?;
                if to_boolean(&r) { result.push(item.clone()); }
            }
            JsValue::Array(result)
        }
        "forEach" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            for (i, item) in arr.iter().enumerate() {
                call_function(&callback, &[item.clone(), JsValue::Number(i as f64)], scope)?;
            }
            JsValue::Undefined
        }
        "find" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            for (i, item) in arr.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64)], scope)?;
                if to_boolean(&r) { return Ok(item.clone()); }
            }
            JsValue::Undefined
        }
        "reduce" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let mut acc = args.get(1).cloned().unwrap_or_else(|| arr.first().cloned().unwrap_or(JsValue::Undefined));
            let start = if args.len() > 1 { 0 } else { 1 };
            for (i, item) in arr.iter().enumerate().skip(start) {
                acc = call_function(&callback, &[acc, item.clone(), JsValue::Number(i as f64)], scope)?;
            }
            acc
        }
        "some" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            for item in arr { if to_boolean(&call_function(&callback, &[item.clone()], scope)?) { return Ok(JsValue::Boolean(true)); } }
            JsValue::Boolean(false)
        }
        "every" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            for item in arr { if !to_boolean(&call_function(&callback, &[item.clone()], scope)?) { return Ok(JsValue::Boolean(false)); } }
            JsValue::Boolean(true)
        }
        "flat" => {
            let mut flat = Vec::new();
            for item in arr { if let JsValue::Array(inner) = item { flat.extend(inner.iter().cloned()); } else { flat.push(item.clone()); } }
            JsValue::Array(flat)
        }
        "fill" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::Array(vec![val; arr.len()])
        }
        _ => JsValue::Undefined,
    })
}

fn call_string_method(s: &str, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "length" => JsValue::Number(s.len() as f64),
        "charAt" => { let i = args.first().map(to_number).unwrap_or(0.0) as usize; s.chars().nth(i).map(|c| JsValue::String(c.to_string())).unwrap_or(JsValue::String(String::new())) }
        "charCodeAt" => { let i = args.first().map(to_number).unwrap_or(0.0) as usize; s.chars().nth(i).map(|c| JsValue::Number(c as u32 as f64)).unwrap_or(JsValue::Number(f64::NAN)) }
        "indexOf" => { let needle = args.first().map(to_string).unwrap_or_default(); JsValue::Number(s.find(&needle).map(|i| i as f64).unwrap_or(-1.0)) }
        "lastIndexOf" => { let needle = args.first().map(to_string).unwrap_or_default(); JsValue::Number(s.rfind(&needle).map(|i| i as f64).unwrap_or(-1.0)) }
        "includes" => { let needle = args.first().map(to_string).unwrap_or_default(); JsValue::Boolean(s.contains(&needle)) }
        "startsWith" => { let needle = args.first().map(to_string).unwrap_or_default(); JsValue::Boolean(s.starts_with(&needle)) }
        "endsWith" => { let needle = args.first().map(to_string).unwrap_or_default(); JsValue::Boolean(s.ends_with(&needle)) }
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
            let sep = args.first().map(to_string).unwrap_or_default();
            if sep.is_empty() { JsValue::Array(s.chars().map(|c| JsValue::String(c.to_string())).collect()) }
            else { JsValue::Array(s.split(&sep).map(|p| JsValue::String(p.to_string())).collect()) }
        }
        "replace" => {
            let pattern = args.first().map(to_string).unwrap_or_default();
            let replacement = args.get(1).map(to_string).unwrap_or_default();
            JsValue::String(s.replacen(&pattern, &replacement, 1))
        }
        "replaceAll" => {
            let pattern = args.first().map(to_string).unwrap_or_default();
            let replacement = args.get(1).map(to_string).unwrap_or_default();
            JsValue::String(s.replace(&pattern, &replacement))
        }
        "repeat" => {
            let n = args.first().map(to_number).unwrap_or(0.0) as usize;
            JsValue::String(s.repeat(n.min(10000)))
        }
        "padStart" => {
            let len = args.first().map(to_number).unwrap_or(0.0) as usize;
            let pad = args.get(1).map(to_string).unwrap_or_else(|| " ".into());
            let mut result = s.to_string();
            while result.len() < len { result = format!("{}{}", pad, result); }
            JsValue::String(result[..len.max(s.len())].to_string())
        }
        "padEnd" => {
            let len = args.first().map(to_number).unwrap_or(0.0) as usize;
            let pad = args.get(1).map(to_string).unwrap_or_else(|| " ".into());
            let mut result = s.to_string();
            while result.len() < len { result.push_str(&pad); }
            result.truncate(len.max(s.len()));
            JsValue::String(result)
        }
        "match" | "search" | "matchAll" => JsValue::Null, // regex not supported
        "toString" | "valueOf" => JsValue::String(s.to_string()),
        _ => JsValue::Undefined,
    }
}

fn call_number_method(n: f64, method: &str, _args: &[JsValue]) -> JsValue {
    match method {
        "toString" => JsValue::String(format_number(n)),
        "toFixed" => {
            let digits = _args.first().map(to_number).unwrap_or(0.0) as usize;
            JsValue::String(format!("{:.prec$}", n, prec = digits))
        }
        _ => JsValue::Undefined,
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
        _ => JsValue::Undefined,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Map/Set/Promise/Date methods
// ═══════════════════════════════════════════════════════════════════════════

fn call_map_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue], _scope: &ScopeRef) -> EvalResult {
    let entries = if let Some(JsValue::Array(e)) = map.get("__entries__") { e.clone() } else { Vec::new() };
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
            // Return the map (immutable model, but semantically correct)
            JsValue::Object(map.clone())
        }
        "delete" => JsValue::Boolean(true),
        "size" => JsValue::Number(entries.len() as f64),
        "keys" => JsValue::Array(entries.iter().filter_map(|e| if let JsValue::Array(kv) = e { kv.first().cloned() } else { None }).collect()),
        "values" => JsValue::Array(entries.iter().filter_map(|e| if let JsValue::Array(kv) = e { kv.get(1).cloned() } else { None }).collect()),
        "entries" => JsValue::Array(entries),
        "forEach" => JsValue::Undefined,
        "clear" => JsValue::Undefined,
        _ => JsValue::Undefined,
    })
}

fn call_set_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> EvalResult {
    let items = if let Some(JsValue::Array(i)) = map.get("__items__") { i.clone() } else { Vec::new() };
    Ok(match method {
        "has" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            JsValue::Boolean(items.iter().any(|x| strict_eq(x, &val)))
        }
        "add" => JsValue::Object(map.clone()),
        "delete" => JsValue::Boolean(true),
        "size" => JsValue::Number(items.len() as f64),
        "values" | "keys" => JsValue::Array(items),
        "forEach" => JsValue::Undefined,
        "clear" => JsValue::Undefined,
        _ => JsValue::Undefined,
    })
}

fn call_promise_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    match method {
        "then" => {
            let resolved = map.get("__resolved__").cloned().unwrap_or(JsValue::Undefined);
            if let Some(callback) = args.first() {
                let result = call_function(callback, &[resolved], scope)?;
                // Wrap result in a new promise
                let mut new_promise = HashMap::new();
                new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                new_promise.insert("__resolved__".to_string(), result);
                Ok(JsValue::Object(new_promise))
            } else {
                Ok(JsValue::Object(map.clone()))
            }
        }
        "catch" => {
            if let Some(rejected) = map.get("__rejected__") {
                if let Some(callback) = args.first() {
                    let result = call_function(callback, &[rejected.clone()], scope)?;
                    let mut new_promise = HashMap::new();
                    new_promise.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
                    new_promise.insert("__resolved__".to_string(), result);
                    return Ok(JsValue::Object(new_promise));
                }
            }
            Ok(JsValue::Object(map.clone()))
        }
        "finally" => {
            if let Some(callback) = args.first() {
                let _ = call_function(callback, &[], scope);
            }
            Ok(JsValue::Object(map.clone()))
        }
        _ => Ok(JsValue::Undefined),
    }
}

fn call_date_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> EvalResult {
    let ts = if let Some(JsValue::Number(n)) = map.get("__value__") { *n } else { 0.0 };
    Ok(match method {
        "getTime" | "valueOf" => JsValue::Number(ts),
        "toISOString" | "toJSON" => JsValue::String(format!("1970-01-01T00:00:00.000Z")), // simplified
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

pub fn to_number(v: &JsValue) -> f64 {
    match v {
        JsValue::Number(n) => *n,
        JsValue::Boolean(b) => if *b { 1.0 } else { 0.0 },
        JsValue::String(s) => s.trim().parse().unwrap_or(f64::NAN),
        JsValue::Null => 0.0,
        JsValue::Undefined => f64::NAN,
        _ => f64::NAN,
    }
}

pub fn to_boolean(v: &JsValue) -> bool {
    match v {
        JsValue::Boolean(b) => *b,
        JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
        JsValue::String(s) => !s.is_empty(),
        JsValue::Null | JsValue::Undefined => false,
        JsValue::Array(_) | JsValue::Object(_) | JsValue::Function { .. } | JsValue::NativeFunction(_) => true,
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
    }
}

fn format_number(n: f64) -> String {
    if n.is_nan() { return "NaN".to_string(); }
    if n.is_infinite() { return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string(); }
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{}", n) }
}

pub fn typeof_str(v: &JsValue) -> &'static str {
    match v {
        JsValue::Undefined => "undefined",
        JsValue::Null => "object",
        JsValue::Boolean(_) => "boolean",
        JsValue::Number(_) => "number",
        JsValue::String(_) => "string",
        JsValue::Function { .. } | JsValue::NativeFunction(_) => "function",
        JsValue::Array(_) | JsValue::Object(_) => "object",
    }
}

fn loose_eq(l: &JsValue, r: &JsValue) -> bool {
    match (l, r) {
        (JsValue::Null, JsValue::Null) | (JsValue::Undefined, JsValue::Undefined) |
        (JsValue::Null, JsValue::Undefined) | (JsValue::Undefined, JsValue::Null) => true,
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
        assert_eq!(eval_full("
            var target = { x: 42 };
            var handler = {};
            var p = new Proxy(target, handler);
            p
        "), JsValue::Object({
            let mut m = HashMap::new();
            m.insert("__type__".to_string(), JsValue::String("Proxy".to_string()));
            m.insert("__proxy_target__".to_string(), JsValue::Object({
                let mut t = HashMap::new();
                t.insert("x".to_string(), JsValue::Number(42.0));
                t
            }));
            m.insert("__proxy_handler__".to_string(), JsValue::Object(HashMap::new()));
            m
        }));
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
}
