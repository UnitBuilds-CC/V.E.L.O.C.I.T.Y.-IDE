use super::ast::*;
use super::token::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }
    fn at(&self, t: &Token) -> bool {
        self.peek() == t
    }
    fn advance(&mut self) -> Token {
        let t = self.peek().clone();
        if !self.at(&Token::Eof) {
            self.pos += 1;
        }
        t
    }
    fn expect(&mut self, t: &Token) -> Result<(), String> {
        if self.peek() == t {
            self.advance();
            Ok(())
        } else {
            Err(format!("expected {:?}, got {:?}", t, self.peek()))
        }
    }
    fn eat(&mut self, t: &Token) -> bool {
        if self.peek() == t {
            self.advance();
            true
        } else {
            false
        }
    }

    pub fn parse_program(&mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        while !self.at(&Token::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek().clone() {
            Token::LBrace => self.parse_block(),
            Token::Var | Token::Let | Token::Const => self.parse_var_decl(),
            Token::Switch => self.parse_switch(),
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::Do => self.parse_do_while(),
            Token::For => self.parse_for(),
            Token::Return => {
                self.advance();
                let e = if !self.at(&Token::Semi)
                    && !self.at(&Token::RBrace)
                    && !self.at(&Token::Eof)
                {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.eat(&Token::Semi);
                Ok(Stmt::Return(e))
            }
            Token::Break => {
                self.advance();
                self.eat(&Token::Semi);
                Ok(Stmt::Break)
            }
            Token::Continue => {
                self.advance();
                self.eat(&Token::Semi);
                Ok(Stmt::Continue)
            }
            Token::Throw => {
                self.advance();
                let e = self.parse_expr()?;
                self.eat(&Token::Semi);
                Ok(Stmt::Throw(e))
            }
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
                    let name = match self.advance() {
                        Token::Ident(n) => n,
                        t => return Err(format!("expected function name, got {:?}", t)),
                    };
                    let params = self.parse_params()?;
                    let body = Box::new(self.parse_block()?);
                    Ok(Stmt::AsyncFunctionDecl { name, params, body })
                } else {
                    // async arrow handled in expression parsing
                    let e = self.parse_expr()?;
                    self.eat(&Token::Semi);
                    Ok(Stmt::Expr(e))
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
                let e = self.parse_expr()?;
                self.eat(&Token::Semi);
                Ok(Stmt::Expr(e))
            }
        }
    }

    pub(super) fn parse_block(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(Stmt::Block(stmts))
    }

    fn parse_var_decl(&mut self) -> Result<Stmt, String> {
        let kind = match self.peek() {
            Token::Var => VarKind::Var,
            Token::Let => VarKind::Let,
            Token::Const => VarKind::Const,
            _ => VarKind::Var,
        };
        self.advance(); // var/let/const
                        // Destructuring: let { a, b } = expr or let [a, b] = expr
        if self.at(&Token::LBrace) {
            self.advance();
            let mut props = Vec::new();
            while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
                let key = match self.advance() {
                    Token::Ident(n) => n,
                    t => return Err(format!("expected ident in destructure, got {:?}", t)),
                };
                let alias = if self.eat(&Token::Colon) {
                    match self.advance() {
                        Token::Ident(n) => Some(n),
                        _ => None,
                    }
                } else {
                    None
                };
                // Skip default value: = expr
                if self.eat(&Token::Eq) {
                    let _ = self.parse_assign()?;
                }
                props.push((key, alias));
                if !self.at(&Token::RBrace) {
                    self.eat(&Token::Comma);
                }
            }
            self.expect(&Token::RBrace)?;
            self.expect(&Token::Eq)?;
            let init = self.parse_expr()?;
            self.eat(&Token::Semi);
            return Ok(Stmt::DestructureDecl {
                pattern: DestructurePattern::Object(props),
                init,
            });
        }
        if self.at(&Token::LBracket) {
            self.advance();
            let mut items = Vec::new();
            while !self.at(&Token::RBracket) && !self.at(&Token::Eof) {
                if self.at(&Token::Comma) {
                    items.push(None);
                } else {
                    match self.advance() {
                        Token::Ident(n) => items.push(Some(n)),
                        _ => items.push(None),
                    }
                }
                if !self.at(&Token::RBracket) {
                    self.eat(&Token::Comma);
                }
            }
            self.expect(&Token::RBracket)?;
            self.expect(&Token::Eq)?;
            let init = self.parse_expr()?;
            self.eat(&Token::Semi);
            return Ok(Stmt::DestructureDecl {
                pattern: DestructurePattern::Array(items),
                init,
            });
        }
        let name = match self.advance() {
            Token::Ident(n) => n,
            t => return Err(format!("expected identifier, got {:?}", t)),
        };
        let init = if self.eat(&Token::Eq) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        // Handle multiple declarators
        while self.eat(&Token::Comma) {
            let _extra = match self.advance() {
                Token::Ident(_n) => _n,
                _ => break,
            };
            if self.eat(&Token::Eq) {
                let _ = self.parse_expr()?;
            }
        }
        self.eat(&Token::Semi);
        Ok(Stmt::VarDecl { kind, name, init })
    }

    fn parse_switch(&mut self) -> Result<Stmt, String> {
        self.advance(); // switch
        self.expect(&Token::LParen)?;
        let discriminant = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::LBrace)?;
        let mut cases = Vec::new();
        while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
            if self.at(&Token::Case) {
                self.advance();
                let pattern = self.parse_expr()?;
                self.expect(&Token::Colon)?;
                let mut body = Vec::new();
                while !self.at(&Token::Case)
                    && !self.at(&Token::Default)
                    && !self.at(&Token::RBrace)
                    && !self.at(&Token::Eof)
                {
                    body.push(self.parse_stmt()?);
                }
                cases.push(SwitchCase {
                    pattern: Some(pattern),
                    body,
                });
            } else if self.at(&Token::Default) {
                self.advance();
                self.expect(&Token::Colon)?;
                let mut body = Vec::new();
                while !self.at(&Token::Case) && !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
                    body.push(self.parse_stmt()?);
                }
                cases.push(SwitchCase {
                    pattern: None,
                    body,
                });
            } else {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Stmt::Switch {
            discriminant,
            cases,
        })
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.advance(); // if
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        let then_branch = Box::new(self.parse_stmt()?);
        let else_branch = if self.eat(&Token::Else) {
            Some(Box::new(self.parse_stmt()?))
        } else {
            None
        };
        Ok(Stmt::If {
            cond,
            then_branch,
            else_branch,
        })
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
                        // Detect `for await (...)` — the await keyword sits between for and (.
        let is_await = self.at(&Token::Await);
        if is_await {
            self.advance();
        }
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
                        if is_await {
                            Stmt::ForAwaitOf {
                                var_name,
                                object: obj,
                                body,
                            }
                        } else {
                            Stmt::ForOf {
                                var_name,
                                object: obj,
                                body,
                            }
                        }
                    } else {
                        Stmt::ForIn {
                            var_name,
                            object: obj,
                            body,
                        }
                    });
                }
            }
            self.pos = saved;
        }
        let init = if self.at(&Token::Semi) {
            None
        } else {
            Some(Box::new(self.parse_stmt()?))
        };
        if !matches!(
            init.as_deref(),
            Some(Stmt::VarDecl { .. }) | Some(Stmt::Expr(_))
        ) {
            // stmt already consumed semicolon
        } else if init.is_none() {
            self.eat(&Token::Semi);
        }
        let cond = if self.at(&Token::Semi) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.eat(&Token::Semi);
        let update = if self.at(&Token::RParen) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(&Token::RParen)?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::For {
            init,
            cond,
            update,
            body,
        })
    }

    fn parse_try(&mut self) -> Result<Stmt, String> {
        self.advance(); // try
        let try_block = Box::new(self.parse_block()?);
        let (catch_var, catch_block) = if self.eat(&Token::Catch) {
            let var = if self.eat(&Token::LParen) {
                let n = match self.advance() {
                    Token::Ident(n) => n,
                    _ => "e".into(),
                };
                self.expect(&Token::RParen)?;
                Some(n)
            } else {
                None
            };
            (var, Some(Box::new(self.parse_block()?)))
        } else {
            (None, None)
        };
        let finally_block = if self.eat(&Token::Finally) {
            Some(Box::new(self.parse_block()?))
        } else {
            None
        };
        Ok(Stmt::TryCatch {
            try_block,
            catch_var,
            catch_block,
            finally_block,
        })
    }

    fn parse_function_decl(&mut self) -> Result<Stmt, String> {
        self.advance(); // function
                        // function* generator
        let is_generator = self.eat(&Token::Star);
        let name = match self.advance() {
            Token::Ident(n) => n,
            t => return Err(format!("expected function name, got {:?}", t)),
        };
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
        let name = match self.advance() {
            Token::Ident(n) => n,
            t => return Err(format!("expected class name, got {:?}", t)),
        };
        let parent = if self.eat(&Token::Extends) {
            match self.advance() {
                Token::Ident(n) => Some(n),
                _ => None,
            }
        } else {
            None
        };
        self.expect(&Token::LBrace)?;
        let mut methods = Vec::new();
        let mut fields = Vec::new();
        while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
            // Skip semicolons between members
            if self.eat(&Token::Semi) {
                continue;
            }
            let is_static = self.eat(&Token::Static);
            // Handle get/set/async as method name prefixes or actual method names
            let method_name = match self.peek().clone() {
                Token::Ident(n) => {
                    self.advance();
                    n
                }
                _ => {
                    self.advance();
                    "unknown".to_string()
                }
            };
            // Detect accessor members: `get x() {}` / `set x(v) {}`. The keyword is only a
            // prefix when followed by another identifier (the real property name).
            let mut kind = ClassMemberKind::Method;
            // If next token is ( it's a method; if next is an ident, this was a keyword prefix
            let final_name = if self.at(&Token::LParen) {
                method_name
            } else if (method_name == "get" || method_name == "set")
                && matches!(self.peek(), Token::Ident(_))
            {
                let Token::Ident(n) = self.advance() else {
                    unreachable!()
                };
                kind = if method_name == "get" {
                    ClassMemberKind::Getter
                } else {
                    ClassMemberKind::Setter
                };
                n
            } else if let Token::Ident(n) = self.peek().clone() {
                self.advance();
                n
            } else {
                method_name
            };
            if self.at(&Token::LParen) {
                // Method / getter / setter member.
                let params = self.parse_params()?;
                let body = self.parse_block()?;
                methods.push(ClassMethod {
                    name: final_name,
                    params,
                    body,
                    is_static,
                    kind,
                });
            } else {
                // Class field: `name = expr;` or bare `name;`.
                let init = if self.eat(&Token::Eq) {
                    Some(self.parse_assign()?)
                } else {
                    None
                };
                self.eat(&Token::Semi);
                fields.push(ClassField {
                    name: final_name,
                    is_static,
                    init,
                });
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Stmt::ClassDecl {
            name,
            parent,
            methods,
            fields,
        })
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
            let local = match self.advance() {
                Token::Ident(n) => n,
                _ => "_".into(),
            };
            specifiers.push(ImportSpecifier {
                imported: "*".into(),
                local,
            });
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
                    match self.advance() {
                        Token::Ident(n) => n,
                        _ => imported.clone(),
                    }
                } else {
                    imported.clone()
                };
                specifiers.push(ImportSpecifier { imported, local });
                if !self.at(&Token::RBrace) {
                    self.eat(&Token::Comma);
                }
            }
            self.expect(&Token::RBrace)?;
        } else {
            // import defaultExport from 'module'
            let local = match self.advance() {
                Token::Ident(n) => n,
                _ => "_".into(),
            };
            specifiers.push(ImportSpecifier {
                imported: "default".into(),
                local,
            });
            // Could also have: import x, { a, b } from '...'
            if self.eat(&Token::Comma) && self.at(&Token::LBrace) {
                self.advance();
                while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
                    let imported = match self.advance() {
                        Token::Ident(n) => n,
                        _ => "_".into(),
                    };
                    let local2 = if self.eat(&Token::As) {
                        match self.advance() {
                            Token::Ident(n) => n,
                            _ => imported.clone(),
                        }
                    } else {
                        imported.clone()
                    };
                    specifiers.push(ImportSpecifier {
                        imported,
                        local: local2,
                    });
                    if !self.at(&Token::RBrace) {
                        self.eat(&Token::Comma);
                    }
                }
                self.expect(&Token::RBrace)?;
            }
        }
        // from 'source'
        self.expect(&Token::From)?;
        let source = match self.advance() {
            Token::Str(s) => s,
            _ => String::new(),
        };
        self.eat(&Token::Semi);
        Ok(Stmt::Import { specifiers, source })
    }

    fn parse_export(&mut self) -> Result<Stmt, String> {
        self.advance(); // export
                        // export default expr
        if self.eat(&Token::Default) {
            let expr = self.parse_expr()?;
            self.eat(&Token::Semi);
            return Ok(Stmt::Export {
                declaration: None,
                default_expr: Some(expr),
                named: vec![],
            });
        }
        // export { a, b, c }
        if self.at(&Token::LBrace) {
            self.advance();
            let mut named = Vec::new();
            while !self.at(&Token::RBrace) && !self.at(&Token::Eof) {
                if let Token::Ident(n) = self.advance() {
                    named.push(n)
                }
                if self.eat(&Token::As) {
                    self.advance();
                } // skip alias for now
                if !self.at(&Token::RBrace) {
                    self.eat(&Token::Comma);
                }
            }
            self.expect(&Token::RBrace)?;
            // Optional: from 'source'
            if self.eat(&Token::From) {
                self.advance();
            }
            self.eat(&Token::Semi);
            return Ok(Stmt::Export {
                declaration: None,
                default_expr: None,
                named,
            });
        }
        // export const/let/var/function/class
        let decl = self.parse_stmt()?;
        Ok(Stmt::Export {
            declaration: Some(Box::new(decl)),
            default_expr: None,
            named: vec![],
        })
    }

    fn parse_params(&mut self) -> Result<Vec<String>, String> {
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        while !self.at(&Token::RParen) && !self.at(&Token::Eof) {
            self.eat(&Token::DotDotDot); // rest param
            if let Token::Ident(n) = self.advance() {
                params.push(n)
            }
            if self.at(&Token::Eq) {
                self.advance();
                let _ = self.parse_expr()?;
            } // default value
            if !self.at(&Token::RParen) {
                self.expect(&Token::Comma)?;
            }
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
            Token::Eq
            | Token::PlusEq
            | Token::MinusEq
            | Token::StarEq
            | Token::SlashEq
            | Token::QuestionQuestionEq => {
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
            Ok(Expr::Ternary(
                Box::new(cond),
                Box::new(then_expr),
                Box::new(else_expr),
            ))
        } else {
            Ok(cond)
        }
    }

    fn parse_nullish(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_or()?;
        while self.eat(&Token::QuestionQuestion) {
            let rhs = self.parse_or()?;
            lhs = Expr::Binary(Token::QuestionQuestion, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_and()?;
        while self.eat(&Token::PipePipe) {
            let rhs = self.parse_and()?;
            lhs = Expr::Binary(Token::PipePipe, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_bitwise_or()?;
        while self.eat(&Token::AmpAmp) {
            let rhs = self.parse_bitwise_or()?;
            lhs = Expr::Binary(Token::AmpAmp, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_bitwise_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_bitwise_xor()?;
        while self.at(&Token::Pipe) && !self.at(&Token::PipePipe) {
            self.advance();
            let rhs = self.parse_bitwise_xor()?;
            lhs = Expr::Binary(Token::Pipe, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_bitwise_and()?;
        while self.eat(&Token::Caret) {
            let rhs = self.parse_bitwise_and()?;
            lhs = Expr::Binary(Token::Caret, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_equality()?;
        while self.at(&Token::Amp) && !self.at(&Token::AmpAmp) {
            self.advance();
            let rhs = self.parse_equality()?;
            lhs = Expr::Binary(Token::Amp, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_comparison()?;
        loop {
            match self.peek().clone() {
                Token::EqEq | Token::BangEq | Token::EqEqEq | Token::BangEqEq => {
                    let op = self.advance();
                    let rhs = self.parse_comparison()?;
                    lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_shift()?;
        loop {
            match self.peek().clone() {
                Token::Lt
                | Token::Gt
                | Token::LtEq
                | Token::GtEq
                | Token::Instanceof
                | Token::In => {
                    let op = self.advance();
                    let rhs = self.parse_shift()?;
                    lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_additive()?;
        loop {
            match self.peek().clone() {
                Token::LtLt | Token::GtGt | Token::GtGtGt => {
                    let op = self.advance();
                    let rhs = self.parse_additive()?;
                    lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            match self.peek().clone() {
                Token::Plus | Token::Minus => {
                    let op = self.advance();
                    let rhs = self.parse_multiplicative()?;
                    lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_exponent()?;
        loop {
            match self.peek().clone() {
                Token::Star | Token::Slash | Token::Percent => {
                    let op = self.advance();
                    let rhs = self.parse_exponent()?;
                    lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_exponent(&mut self) -> Result<Expr, String> {
        let base = self.parse_unary()?;
        if self.eat(&Token::StarStar) {
            let exp = self.parse_exponent()?;
            Ok(Expr::Binary(Token::StarStar, Box::new(base), Box::new(exp)))
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::Bang | Token::Minus | Token::Plus | Token::Tilde => {
                let op = self.advance();
                let rhs = self.parse_unary()?;
                Ok(Expr::Unary(op, Box::new(rhs)))
            }
            Token::Typeof => {
                self.advance();
                let rhs = self.parse_unary()?;
                Ok(Expr::Typeof(Box::new(rhs)))
            }
            Token::Void => {
                self.advance();
                let rhs = self.parse_unary()?;
                Ok(Expr::Void(Box::new(rhs)))
            }
            Token::Delete => {
                self.advance();
                let rhs = self.parse_unary()?;
                Ok(Expr::Unary(Token::Delete, Box::new(rhs)))
            }
            Token::PlusPlus | Token::MinusMinus => {
                let op = self.advance();
                let rhs = self.parse_unary()?;
                Ok(Expr::Unary(op, Box::new(rhs)))
            }
            Token::New => {
                self.advance();
                let callee = self.parse_new_target()?;
                let args = if self.at(&Token::LParen) {
                    self.parse_args()?
                } else {
                    Vec::new()
                };
                self.parse_member_chain(Expr::New(Box::new(callee), args))
            }
            Token::DotDotDot => {
                self.advance();
                let e = self.parse_assign()?;
                Ok(Expr::Spread(Box::new(e)))
            }
            Token::Await => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::Await(Box::new(e)))
            }
            Token::Yield => {
                self.advance();
                let e = if self.at(&Token::Semi)
                    || self.at(&Token::RBrace)
                    || self.at(&Token::RParen)
                    || self.at(&Token::Comma)
                {
                    Expr::Undefined
                } else {
                    self.parse_assign()?
                };
                Ok(Expr::Yield(Box::new(e)))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_call_expr()?;
        match self.peek() {
            Token::PlusPlus => {
                self.advance();
                expr = Expr::Unary(Token::PlusPlus, Box::new(expr));
            }
            Token::MinusMinus => {
                self.advance();
                expr = Expr::Unary(Token::MinusMinus, Box::new(expr));
            }
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
                Token::LParen => {
                    let args = self.parse_args()?;
                    expr = Expr::Call(Box::new(expr), args);
                }
                Token::Dot => {
                    self.advance();
                    let prop = self.parse_prop_name()?;
                    expr = Expr::Member(Box::new(expr), prop);
                }
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
                Token::LBracket => {
                    self.advance();
                    let idx = self.parse_expr()?;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::Index(Box::new(expr), Box::new(idx));
                }
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
                Token::Dot => {
                    self.advance();
                    let prop = self.parse_prop_name()?;
                    expr = Expr::Member(Box::new(expr), prop);
                }
                Token::LBracket => {
                    self.advance();
                    let idx = self.parse_expr()?;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::Index(Box::new(expr), Box::new(idx));
                }
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
            if !self.at(&Token::RParen) {
                self.expect(&Token::Comma)?;
            }
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
            Token::Number(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Token::Template(s) => {
                self.advance();
                Ok(Expr::Template(s))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Null)
            }
            Token::Undefined => {
                self.advance();
                Ok(Expr::Undefined)
            }
            Token::This => {
                self.advance();
                Ok(Expr::This)
            }
            Token::Super => {
                self.advance();
                Ok(Expr::Super)
            }
            Token::Async => {
                // async () => ... or async function
                self.advance();
                if self.at(&Token::Function) {
                    self.advance();
                    let name = if let Token::Ident(n) = self.peek().clone() {
                        self.advance();
                        Some(n)
                    } else {
                        None
                    };
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
                            let body = if self.at(&Token::LBrace) {
                                self.parse_block()?
                            } else {
                                Stmt::Return(Some(self.parse_assign()?))
                            };
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
                let Token::Ident(name) = self.advance() else {
                    unreachable!()
                };
                // Arrow function: x => expr  or (x, y) => expr
                if self.at(&Token::Arrow) {
                    self.advance();
                    let body = if self.at(&Token::LBrace) {
                        self.parse_block()?
                    } else {
                        Stmt::Return(Some(self.parse_assign()?))
                    };
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
                    if self.at(&Token::Arrow) {
                        self.advance();
                        let body = if self.at(&Token::LBrace) {
                            self.parse_block()?
                        } else {
                            Stmt::Return(Some(self.parse_assign()?))
                        };
                        return Ok(Expr::Arrow(vec![], Box::new(body)));
                    }
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
                let name = if let Token::Ident(n) = self.peek().clone() {
                    self.advance();
                    Some(n)
                } else {
                    None
                };
                let params = self.parse_params()?;
                let body = Box::new(self.parse_block()?);
                let func_name = if is_generator {
                    Some(format!(
                        "__generator__{}",
                        name.unwrap_or_else(|| "anon".to_string())
                    ))
                } else {
                    name
                };
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
            match self.advance() {
                Token::Ident(n) => params.push(n),
                _ => {
                    self.pos = saved;
                    return Err("not arrow".into());
                }
            }
            if self.at(&Token::Eq) {
                self.advance();
                let _ = self.parse_assign()?;
            }
            if !self.at(&Token::RParen) && !self.eat(&Token::Comma) {
                self.pos = saved;
                return Err("not arrow".into());
            }
        }
        if !self.eat(&Token::RParen) {
            self.pos = saved;
            return Err("not arrow".into());
        }
        if !self.eat(&Token::Arrow) {
            self.pos = saved;
            return Err("not arrow".into());
        }
        let body = if self.at(&Token::LBrace) {
            self.parse_block()?
        } else {
            Stmt::Return(Some(self.parse_assign()?))
        };
        Ok(Expr::Arrow(params, Box::new(body)))
    }

    fn parse_array_literal(&mut self) -> Result<Expr, String> {
        self.expect(&Token::LBracket)?;
        let mut elems = Vec::new();
        while !self.at(&Token::RBracket) && !self.at(&Token::Eof) {
            elems.push(self.parse_assign()?);
            if !self.at(&Token::RBracket) {
                self.expect(&Token::Comma)?;
            }
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
                if !self.at(&Token::RBrace) {
                    self.expect(&Token::Comma)?;
                }
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
                if !self.at(&Token::RBrace) {
                    self.expect(&Token::Comma)?;
                }
                continue;
            }
            let key = match self.peek().clone() {
                Token::Ident(k) => {
                    self.advance();
                    k
                }
                Token::Str(k) => {
                    self.advance();
                    k
                }
                Token::Number(n) => {
                    self.advance();
                    format!("{}", n)
                }
                _ => return Err(format!("expected property key, got {:?}", self.peek())),
            };
            // Getter/setter: { get x() { ... }, set x(v) { ... } }
            if (key == "get" || key == "set") && matches!(self.peek(), Token::Ident(_)) {
                let actual_key = match self.advance() {
                    Token::Ident(n) => n,
                    _ => key.clone(),
                };
                let params = self.parse_params()?;
                let body = self.parse_block()?;
                let func = Expr::Function(
                    Some(format!("{}_{}", key, actual_key)),
                    params,
                    Box::new(body),
                );
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
            if !self.at(&Token::RBrace) {
                self.expect(&Token::Comma)?;
            }
        }
        self.expect(&Token::RBrace)?;
        if has_spread || has_accessor || has_computed {
            Ok(Expr::ObjectWithSpread(spread_props))
        } else {
            Ok(Expr::Object(props))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::lexer::lex;
    use super::*;

    fn parse(src: &str) -> Result<Vec<Stmt>, String> {
        let tokens = lex(src).map_err(|e| e.to_string())?;
        let mut parser = Parser::new(tokens);
        parser.parse_program()
    }

    fn parse_one(src: &str) -> Stmt {
        parse(src).unwrap().into_iter().next().unwrap()
    }

    fn parse_expr_node(src: &str) -> Expr {
        match parse_one(src) {
            Stmt::Expr(e) => e,
            other => panic!("expected Expr stmt, got {:?}", other),
        }
    }

    // ── empty program ──────────────────────────────────────────────────

    #[test]
    fn empty_program() {
        let stmts = parse("").unwrap();
        assert!(stmts.is_empty());
    }

    #[test]
    fn whitespace_only() {
        let stmts = parse("   \n\t  ").unwrap();
        assert!(stmts.is_empty());
    }

    // ── number literal ─────────────────────────────────────────────────

    #[test]
    fn number_literal_expr() {
        match parse_expr_node("42") {
            Expr::Number(n) => assert_eq!(n, 42.0),
            other => panic!("expected Number, got {:?}", other),
        }
    }

    // ── string literal ─────────────────────────────────────────────────

    #[test]
    fn string_literal_expr() {
        match parse_expr_node("'hello'") {
            Expr::Str(s) => assert_eq!(s, "hello"),
            other => panic!("expected Str, got {:?}", other),
        }
    }

    // ── boolean literals ───────────────────────────────────────────────

    #[test]
    fn bool_true() {
        match parse_expr_node("true") {
            Expr::Bool(b) => assert!(b),
            other => panic!("expected Bool, got {:?}", other),
        }
    }

    #[test]
    fn bool_false() {
        match parse_expr_node("false") {
            Expr::Bool(b) => assert!(!b),
            other => panic!("expected Bool, got {:?}", other),
        }
    }

    // ── null and undefined ─────────────────────────────────────────────

    #[test]
    fn null_literal() {
        assert!(matches!(parse_expr_node("null"), Expr::Null));
    }

    #[test]
    fn undefined_literal() {
        assert!(matches!(parse_expr_node("undefined"), Expr::Undefined));
    }

    // ── identifier ─────────────────────────────────────────────────────

    #[test]
    fn identifier_expr() {
        match parse_expr_node("foo") {
            Expr::Ident(s) => assert_eq!(s, "foo"),
            other => panic!("expected Ident, got {:?}", other),
        }
    }

    // ── var declarations ───────────────────────────────────────────────

    #[test]
    fn var_decl_no_init() {
        match parse_one("var x;") {
            Stmt::VarDecl { kind, name, init } => {
                assert_eq!(kind, VarKind::Var);
                assert_eq!(name, "x");
                assert!(init.is_none());
            }
            other => panic!("expected VarDecl, got {:?}", other),
        }
    }

    #[test]
    fn let_decl_with_init() {
        match parse_one("let y = 5;") {
            Stmt::VarDecl { kind, name, init } => {
                assert_eq!(kind, VarKind::Let);
                assert_eq!(name, "y");
                assert!(init.is_some());
            }
            other => panic!("expected VarDecl, got {:?}", other),
        }
    }

    #[test]
    fn const_decl() {
        match parse_one("const Z = 10;") {
            Stmt::VarDecl { kind, name, .. } => {
                assert_eq!(kind, VarKind::Const);
                assert_eq!(name, "Z");
            }
            other => panic!("expected VarDecl, got {:?}", other),
        }
    }

    // ── return statement ───────────────────────────────────────────────

    #[test]
    fn return_with_value() {
        match parse_one("return 42;") {
            Stmt::Return(Some(Expr::Number(n))) => assert_eq!(n, 42.0),
            other => panic!("expected Return(42), got {:?}", other),
        }
    }

    #[test]
    fn return_bare() {
        match parse_one("return;") {
            Stmt::Return(None) => {}
            other => panic!("expected Return(None), got {:?}", other),
        }
    }

    // ── break and continue ─────────────────────────────────────────────

    #[test]
    fn break_stmt() {
        assert!(matches!(parse_one("break;"), Stmt::Break));
    }

    #[test]
    fn continue_stmt() {
        assert!(matches!(parse_one("continue;"), Stmt::Continue));
    }

    // ── block statement ────────────────────────────────────────────────

    #[test]
    fn block_empty() {
        match parse_one("{}") {
            Stmt::Block(stmts) => assert!(stmts.is_empty()),
            other => panic!("expected Block, got {:?}", other),
        }
    }

    #[test]
    fn block_with_stmts() {
        let stmts = parse("{ var x; var y; }").unwrap();
        assert_eq!(stmts.len(), 1); // one block stmt
    }

    // ── binary expressions ─────────────────────────────────────────────

    #[test]
    fn addition() {
        match parse_expr_node("1 + 2") {
            Expr::Binary(Token::Plus, _, _) => {}
            other => panic!("expected Binary(+), got {:?}", other),
        }
    }

    #[test]
    fn multiplication() {
        match parse_expr_node("3 * 4") {
            Expr::Binary(Token::Star, _, _) => {}
            other => panic!("expected Binary(*), got {:?}", other),
        }
    }

    // ── unary expressions ──────────────────────────────────────────────

    #[test]
    fn typeof_expr() {
        match parse_expr_node("typeof x") {
            Expr::Typeof(_) => {}
            other => panic!("expected Typeof, got {:?}", other),
        }
    }

    // ── array literal ──────────────────────────────────────────────────

    #[test]
    fn array_literal() {
        match parse_expr_node("[1, 2, 3]") {
            Expr::Array(elems) => assert_eq!(elems.len(), 3),
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn array_empty() {
        match parse_expr_node("[]") {
            Expr::Array(elems) => assert!(elems.is_empty()),
            other => panic!("expected Array, got {:?}", other),
        }
    }

    // ── object literal ─────────────────────────────────────────────────

    #[test]
    fn object_literal() {
        // Wrap in parens to force expression context (avoids block ambiguity)
        match parse_expr_node("({a: 1, b: 2})") {
            Expr::Object(props) => assert_eq!(props.len(), 2),
            other => panic!("expected Object, got {:?}", other),
        }
    }

    // ── function declaration ───────────────────────────────────────────

    #[test]
    fn function_decl() {
        match parse_one("function foo(a, b) { return a; }") {
            Stmt::FunctionDecl { name, params, .. } => {
                assert_eq!(name, "foo");
                assert_eq!(params.len(), 2);
            }
            other => panic!("expected FunctionDecl, got {:?}", other),
        }
    }

    // ── if statement ───────────────────────────────────────────────────

    #[test]
    fn if_stmt_no_else() {
        match parse_one("if (true) { }") {
            Stmt::If {
                cond, else_branch, ..
            } => {
                assert!(matches!(cond, Expr::Bool(true)));
                assert!(else_branch.is_none());
            }
            other => panic!("expected If, got {:?}", other),
        }
    }

    // ── while loop ─────────────────────────────────────────────────────

    #[test]
    fn while_loop() {
        match parse_one("while (true) { }") {
            Stmt::While { .. } => {}
            other => panic!("expected While, got {:?}", other),
        }
    }

    // ── throw statement ────────────────────────────────────────────────

    #[test]
    fn throw_stmt() {
        match parse_one("throw 42;") {
            Stmt::Throw(Expr::Number(n)) => assert_eq!(n, 42.0),
            other => panic!("expected Throw(42), got {:?}", other),
        }
    }

    // ── try/catch ──────────────────────────────────────────────────────

    #[test]
    fn try_catch() {
        match parse_one("try { } catch (e) { }") {
            Stmt::TryCatch { catch_var, .. } => {
                assert_eq!(catch_var, Some("e".to_string()));
            }
            other => panic!("expected TryCatch, got {:?}", other),
        }
    }

    // ── member access ──────────────────────────────────────────────────

    #[test]
    fn member_access() {
        match parse_expr_node("a.b") {
            Expr::Member(_, prop) => assert_eq!(prop, "b"),
            other => panic!("expected Member, got {:?}", other),
        }
    }

    // ── call expression ────────────────────────────────────────────────

    #[test]
    fn call_expr() {
        match parse_expr_node("foo(1, 2)") {
            Expr::Call(_, args) => assert_eq!(args.len(), 2),
            other => panic!("expected Call, got {:?}", other),
        }
    }

    // ── new expression ─────────────────────────────────────────────────

    #[test]
    fn new_expr() {
        match parse_expr_node("new Foo()") {
            Expr::New(_, _) => {}
            other => panic!("expected New, got {:?}", other),
        }
    }

    // ── multiple statements ────────────────────────────────────────────

    #[test]
    fn multiple_statements() {
        let stmts = parse("var x; var y; var z;").unwrap();
        assert_eq!(stmts.len(), 3);
    }

    // ── parse error ────────────────────────────────────────────────────

    #[test]
    fn parse_error_unclosed_brace() {
        let result = parse("{ ");
        assert!(result.is_err());
    }
}
