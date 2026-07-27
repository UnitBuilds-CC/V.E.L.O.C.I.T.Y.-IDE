// compiler/nda_parser.rs — Parser for the NDA programming language
//
// Converts a stream of Located tokens into a compiled NdaNode AST.
// Resolves function call targets dynamically via Merkle hash propagation.
#![allow(dead_code)]

use std::collections::HashMap;
use sha2::{Digest, Sha256};
use crate::site_map::verifier::{NdaNode, CmpOp, VecOpKind};
use crate::compiler::nda_lexer::{Token, Located, NdaLexer};

/// Helper: compute 8-byte SHA-256 hash of a string name.
pub fn hash_name(name: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes(digest[..8].try_into().unwrap())
}

/// Helper: build synthetic matrix node of shape rows x cols.
pub fn build_matrix_node(rows: usize, cols: usize) -> NdaNode {
    let rows = rows.min(65535).max(1) as u16;
    let cols = cols.min(65535).max(1) as u16;
    let bitmap_bytes = rows as usize * (cols as usize).div_ceil(8);
    let sign: Vec<u8> = (0..bitmap_bytes).map(|i| if i % 2 == 0 { 0xAA } else { 0x55 }).collect();
    let extra: Vec<u8> = (0..bitmap_bytes).map(|i| if i % 2 == 0 { 0x55 } else { 0xAA }).collect();
    NdaNode::Matrix { rows, cols, scale: 0, sign, extra }
}

/// Helper: build synthetic norm node of shape size.
pub fn build_norm_node(size: usize) -> NdaNode {
    let size = size.min(65535).max(1) as u16;
    let bitmap_bytes = (size as usize).div_ceil(8);
    let weight = vec![0xFF; bitmap_bytes];
    let bias = vec![0x00; bitmap_bytes];
    NdaNode::Norm { size, weight, bias }
}

/// Recursively replace Call nodes whose targets match the temporary call keys.
pub fn resolve_calls(
    node: &NdaNode,
    fn_map: &HashMap<String, u64>,
    call_names: &HashMap<u64, String>,
) -> NdaNode {
    match node {
        NdaNode::Scope { children } => NdaNode::Scope {
            children: children.iter().map(|c| resolve_calls(c, fn_map, call_names)).collect(),
        },
        NdaNode::Loop { count, body } => NdaNode::Loop {
            count: *count,
            body: body.iter().map(|c| resolve_calls(c, fn_map, call_names)).collect(),
        },
        NdaNode::While { cond, body } => NdaNode::While {
            cond: Box::new(resolve_calls(cond, fn_map, call_names)),
            body: body.iter().map(|c| resolve_calls(c, fn_map, call_names)).collect(),
        },
        NdaNode::If { cond, then_body, else_body } => NdaNode::If {
            cond: Box::new(resolve_calls(cond, fn_map, call_names)),
            then_body: then_body.iter().map(|c| resolve_calls(c, fn_map, call_names)).collect(),
            else_body: else_body.as_ref().map(|eb| eb.iter().map(|c| resolve_calls(c, fn_map, call_names)).collect()),
        },
        NdaNode::Compare { op, lhs, rhs } => NdaNode::Compare {
            op: *op,
            lhs: Box::new(resolve_calls(lhs, fn_map, call_names)),
            rhs: Box::new(resolve_calls(rhs, fn_map, call_names)),
        },
        NdaNode::Let { name_hash, init } => NdaNode::Let {
            name_hash: *name_hash,
            init: Box::new(resolve_calls(init, fn_map, call_names)),
        },
        NdaNode::Store { name_hash, value } => NdaNode::Store {
            name_hash: *name_hash,
            value: Box::new(resolve_calls(value, fn_map, call_names)),
        },
        NdaNode::Add { lhs, rhs } => NdaNode::Add {
            lhs: Box::new(resolve_calls(lhs, fn_map, call_names)),
            rhs: Box::new(resolve_calls(rhs, fn_map, call_names)),
        },
        NdaNode::VecOp { op, operand } => NdaNode::VecOp {
            op: *op,
            operand: Box::new(resolve_calls(operand, fn_map, call_names)),
        },
        NdaNode::Print { source } => NdaNode::Print {
            source: Box::new(resolve_calls(source, fn_map, call_names)),
        },
        NdaNode::Return { value } => NdaNode::Return {
            value: Box::new(resolve_calls(value, fn_map, call_names)),
        },
        NdaNode::Call { target } => {
            if let Some(name) = call_names.get(target) {
                if let Some(&hash) = fn_map.get(name) {
                    return NdaNode::Call { target: hash };
                }
            }
            NdaNode::Call { target: *target }
        }
        other => other.clone(),
    }
}

pub fn compile(source: &str) -> Result<(NdaNode, HashMap<String, u64>), String> {
    let mut lexer = NdaLexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = NdaParser::new(tokens);
    
    let mut functions = HashMap::new();
    let mut all_calls = HashMap::new();

    while let Some(tok) = parser.peek() {
        if *tok == Token::Eof {
            break;
        }
        if *tok == Token::Fn {
            let (name, node, calls) = parser.parse_function()?;
            functions.insert(name.clone(), node);
            all_calls.insert(name, calls);
        } else {
            let loc = parser.peek_loc().unwrap();
            return Err(format!("{}:{}: Expected 'fn' keyword at top level", loc.line, loc.col));
        }
    }

    let mut fn_hashes: HashMap<String, u64> = functions.keys()
        .map(|name| (name.clone(), hash_name(name)))
        .collect();

    for _ in 0..5 {
        let mut next_hashes = fn_hashes.clone();
        for (name, node) in &functions {
            let calls = all_calls.get(name).unwrap();
            let resolved = resolve_calls(node, &fn_hashes, calls);
            next_hashes.insert(name.clone(), resolved.hash());
        }
        fn_hashes = next_hashes;
    }

    let mut sorted_names: Vec<String> = functions.keys().cloned().collect();
    sorted_names.sort();

    let mut children = Vec::new();
    let mut final_hashes = HashMap::new();
    for name in sorted_names {
        let node = functions.get(&name).unwrap();
        let calls = all_calls.get(&name).unwrap();
        let resolved = resolve_calls(node, &fn_hashes, calls);
        final_hashes.insert(name.clone(), resolved.hash());
        children.push(resolved);
    }

    Ok((NdaNode::Scope { children }, final_hashes))
}

pub struct NdaParser {
    tokens: Vec<Located>,
    pos: usize,
}

impl NdaParser {
    pub fn new(tokens: Vec<Located>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|l| &l.token)
    }

    fn peek_loc(&self) -> Option<&Located> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos).map(|l| &l.token);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn match_token(&mut self, expected: Token) -> Result<(), String> {
        if let Some(tok) = self.peek() {
            if *tok == expected {
                self.advance();
                Ok(())
            } else {
                let loc = self.peek_loc().unwrap();
                Err(format!("{}:{}: Expected token {:?}, found {:?}", loc.line, loc.col, expected, tok))
            }
        } else {
            Err(format!("Expected token {:?}, reached End of File", expected))
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        if let Some(Token::Ident(name)) = self.peek() {
            let name = name.clone();
            self.advance();
            Ok(name)
        } else {
            let loc = self.peek_loc().unwrap_or(&Located { token: Token::Eof, line: 0, col: 0 });
            Err(format!("{}:{}: Expected identifier", loc.line, loc.col))
        }
    }

    fn expect_int_lit(&mut self) -> Result<i64, String> {
        if let Some(Token::IntLit(val)) = self.peek() {
            let val = *val;
            self.advance();
            Ok(val)
        } else {
            let loc = self.peek_loc().unwrap_or(&Located { token: Token::Eof, line: 0, col: 0 });
            Err(format!("{}:{}: Expected integer literal", loc.line, loc.col))
        }
    }

    pub fn parse_program(&mut self) -> Result<NdaNode, String> {
        let mut functions = HashMap::new();
        let mut all_calls = HashMap::new(); // function name -> calls map

        while let Some(tok) = self.peek() {
            if *tok == Token::Eof {
                break;
            }
            if *tok == Token::Fn {
                let (name, node, calls) = self.parse_function()?;
                functions.insert(name.clone(), node);
                all_calls.insert(name, calls);
            } else {
                let loc = self.peek_loc().unwrap();
                return Err(format!("{}:{}: Expected 'fn' keyword at top level", loc.line, loc.col));
            }
        }

        // Resolve call targets iteratively
        let mut fn_hashes: HashMap<String, u64> = functions.keys()
            .map(|name| (name.clone(), hash_name(name)))
            .collect();

        for _ in 0..5 {
            let mut next_hashes = fn_hashes.clone();
            for (name, node) in &functions {
                let calls = all_calls.get(name).unwrap();
                let resolved = resolve_calls(node, &fn_hashes, calls);
                next_hashes.insert(name.clone(), resolved.hash());
            }
            fn_hashes = next_hashes;
        }

        // Final compilation
        let mut sorted_names: Vec<String> = functions.keys().cloned().collect();
        sorted_names.sort();

        let mut children = Vec::new();
        for name in sorted_names {
            let node = functions.get(&name).unwrap();
            let calls = all_calls.get(&name).unwrap();
            let resolved = resolve_calls(node, &fn_hashes, calls);
            children.push(resolved);
        }

        Ok(NdaNode::Scope { children })
    }

    fn parse_function(&mut self) -> Result<(String, NdaNode, HashMap<u64, String>), String> {
        self.match_token(Token::Fn)?;
        let name = self.expect_ident()?;
        self.match_token(Token::LParen)?;

        let mut params = Vec::new();
        if self.peek() != Some(&Token::RParen) {
            loop {
                let param_name = self.expect_ident()?;
                self.match_token(Token::Colon)?;
                let _param_type = self.parse_type()?;
                params.push(param_name);
                if self.peek() == Some(&Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.match_token(Token::RParen)?;

        if self.peek() == Some(&Token::Arrow) {
            self.advance();
            let _ret_type = self.parse_type()?;
        }

        let mut calls = HashMap::new();
        let body = self.parse_block(&mut calls)?;

        // Prepend parameter bindings
        let mut children = Vec::new();
        for param in params {
            let name_hash = hash_name(&param);
            children.push(NdaNode::Let {
                name_hash,
                init: Box::new(NdaNode::Scope { children: vec![] }),
            });
        }
        children.extend(body);

        Ok((name, NdaNode::Scope { children }, calls))
    }

    fn parse_type(&mut self) -> Result<Token, String> {
        if let Some(tok) = self.peek() {
            match tok {
                Token::Vec | Token::Matrix | Token::Norm | Token::Int => {
                    let t = tok.clone();
                    self.advance();
                    Ok(t)
                }
                _ => {
                    let loc = self.peek_loc().unwrap();
                    Err(format!("{}:{}: Expected type keyword (vec, matrix, norm, int)", loc.line, loc.col))
                }
            }
        } else {
            Err("Expected type, reached End of File".to_string())
        }
    }

    fn parse_block(&mut self, calls: &mut HashMap<u64, String>) -> Result<Vec<NdaNode>, String> {
        self.match_token(Token::LBrace)?;
        let mut stmts = Vec::new();
        while let Some(tok) = self.peek() {
            if *tok == Token::RBrace {
                break;
            }
            if let Some(stmt) = self.parse_statement(calls)? {
                stmts.push(stmt);
            }
        }
        self.match_token(Token::RBrace)?;
        Ok(stmts)
    }

    fn parse_statement(&mut self, calls: &mut HashMap<u64, String>) -> Result<Option<NdaNode>, String> {
        let tok = match self.peek() {
            Some(t) => t,
            None => return Err("Unexpected End of File inside statement".to_string()),
        };

        let node = match tok {
            Token::Let => {
                self.advance();
                let name = self.expect_ident()?;
                self.match_token(Token::Assign)?;
                let init = self.parse_expr(calls)?;
                if self.peek() == Some(&Token::Semi) { self.advance(); }
                Some(NdaNode::Let {
                    name_hash: hash_name(&name),
                    init: Box::new(init),
                })
            }
            Token::While => {
                self.advance();
                let cond = self.parse_expr(calls)?;
                let body = self.parse_block(calls)?;
                Some(NdaNode::While {
                    cond: Box::new(cond),
                    body,
                })
            }
            Token::Loop => {
                self.advance();
                let expr = self.parse_expr(calls)?;
                let body = self.parse_block(calls)?;

                match expr {
                    NdaNode::Int { value } if value >= 0 => {
                        Some(NdaNode::Loop {
                            count: value as u32,
                            body,
                        })
                    }
                    _ => {
                        // Count-down translation for dynamic loop
                        let temp_name = format!("_loop_cnt_{}", self.pos);
                        let temp_hash = hash_name(&temp_name);

                        let cond = NdaNode::Compare {
                            op: CmpOp::Gt,
                            lhs: Box::new(NdaNode::Load { name_hash: temp_hash }),
                            rhs: Box::new(NdaNode::Int { value: 0 }),
                        };

                        let decrement = NdaNode::Store {
                            name_hash: temp_hash,
                            value: Box::new(NdaNode::Add {
                                lhs: Box::new(NdaNode::Load { name_hash: temp_hash }),
                                rhs: Box::new(NdaNode::VecOp {
                                    op: VecOpKind::Negate,
                                    operand: Box::new(NdaNode::Int { value: 1 }),
                                }),
                            }),
                        };

                        let mut full_body = body;
                        full_body.push(decrement);

                        Some(NdaNode::Scope {
                            children: vec![
                                NdaNode::Let {
                                    name_hash: temp_hash,
                                    init: Box::new(expr),
                                },
                                NdaNode::While {
                                    cond: Box::new(cond),
                                    body: full_body,
                                },
                            ],
                        })
                    }
                }
            }
            Token::If => {
                self.advance();
                let cond = self.parse_expr(calls)?;
                let then_body = self.parse_block(calls)?;
                let mut else_body = None;
                if self.peek() == Some(&Token::Else) {
                    self.advance();
                    if self.peek() == Some(&Token::If) {
                        if let Some(nested_if) = self.parse_statement(calls)? {
                            else_body = Some(vec![nested_if]);
                        }
                    } else {
                        else_body = Some(self.parse_block(calls)?);
                    }
                }
                Some(NdaNode::If {
                    cond: Box::new(cond),
                    then_body,
                    else_body,
                })
            }
            Token::Return => {
                self.advance();
                let val = self.parse_expr(calls)?;
                if self.peek() == Some(&Token::Semi) { self.advance(); }
                Some(NdaNode::Return { value: Box::new(val) })
            }
            Token::Break => {
                self.advance();
                if self.peek() == Some(&Token::Semi) { self.advance(); }
                Some(NdaNode::Break)
            }
            Token::Print => {
                self.advance();
                self.match_token(Token::LParen)?;
                let expr = self.parse_expr(calls)?;
                self.match_token(Token::RParen)?;
                if self.peek() == Some(&Token::Semi) { self.advance(); }
                Some(NdaNode::Print { source: Box::new(expr) })
            }
            Token::Ident(name) if self.tokens.get(self.pos + 1).map(|l| &l.token) == Some(&Token::Assign) => {
                let name = name.clone();
                self.advance(); // consume ident
                self.advance(); // consume Assign '='
                let val = self.parse_expr(calls)?;
                if self.peek() == Some(&Token::Semi) { self.advance(); }
                Some(NdaNode::Store {
                    name_hash: hash_name(&name),
                    value: Box::new(val),
                })
            }
            _ => {
                let expr = self.parse_expr(calls)?;
                if self.peek() == Some(&Token::Semi) { self.advance(); }
                Some(expr)
            }
        };

        Ok(node)
    }

    fn parse_expr(&mut self, calls: &mut HashMap<u64, String>) -> Result<NdaNode, String> {
        self.parse_comparison(calls)
    }

    fn parse_comparison(&mut self, calls: &mut HashMap<u64, String>) -> Result<NdaNode, String> {
        let mut lhs = self.parse_additive(calls)?;
        while let Some(tok) = self.peek() {
            let op = match tok {
                Token::Eq => CmpOp::Eq,
                Token::Ne => CmpOp::Ne,
                Token::Lt => CmpOp::Lt,
                Token::Gt => CmpOp::Gt,
                Token::Le => CmpOp::Le,
                Token::Ge => CmpOp::Ge,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_additive(calls)?;
            lhs = NdaNode::Compare {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self, calls: &mut HashMap<u64, String>) -> Result<NdaNode, String> {
        let mut lhs = self.parse_unary(calls)?;
        while let Some(tok) = self.peek() {
            match tok {
                Token::Plus => {
                    self.advance();
                    let rhs = self.parse_unary(calls)?;
                    lhs = NdaNode::Add {
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self, calls: &mut HashMap<u64, String>) -> Result<NdaNode, String> {
        if self.peek() == Some(&Token::Minus) {
            self.advance();
            let operand = self.parse_unary(calls)?;
            Ok(NdaNode::VecOp {
                op: VecOpKind::Negate,
                operand: Box::new(operand),
            })
        } else {
            self.parse_primary(calls)
        }
    }

    fn parse_primary(&mut self, calls: &mut HashMap<u64, String>) -> Result<NdaNode, String> {
        let tok = match self.peek() {
            Some(t) => t.clone(),
            None => return Err("Unexpected End of File inside expression".to_string()),
        };

        match tok {
            Token::IntLit(val) => {
                self.advance();
                Ok(NdaNode::Int { value: val as i32 })
            }
            Token::FloatLit(val) => {
                self.advance();
                Ok(NdaNode::Int { value: val as i32 })
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr(calls)?;
                self.match_token(Token::RParen)?;
                Ok(expr)
            }
            Token::Add => {
                self.advance();
                self.match_token(Token::LParen)?;
                let lhs = self.parse_expr(calls)?;
                self.match_token(Token::Comma)?;
                let rhs = self.parse_expr(calls)?;
                self.match_token(Token::RParen)?;
                Ok(NdaNode::Add {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                })
            }
            Token::Silu => {
                self.advance();
                self.match_token(Token::LParen)?;
                let arg = self.parse_expr(calls)?;
                self.match_token(Token::RParen)?;
                Ok(NdaNode::VecOp {
                    op: VecOpKind::SiLU,
                    operand: Box::new(arg),
                })
            }
            Token::Negate => {
                self.advance();
                self.match_token(Token::LParen)?;
                let arg = self.parse_expr(calls)?;
                self.match_token(Token::RParen)?;
                Ok(NdaNode::VecOp {
                    op: VecOpKind::Negate,
                    operand: Box::new(arg),
                })
            }
            Token::Abs => {
                self.advance();
                self.match_token(Token::LParen)?;
                let arg = self.parse_expr(calls)?;
                self.match_token(Token::RParen)?;
                Ok(NdaNode::VecOp {
                    op: VecOpKind::Abs,
                    operand: Box::new(arg),
                })
            }
            Token::ReduceSum => {
                self.advance();
                self.match_token(Token::LParen)?;
                let arg = self.parse_expr(calls)?;
                self.match_token(Token::RParen)?;
                Ok(NdaNode::VecOp {
                    op: VecOpKind::ReduceSum,
                    operand: Box::new(arg),
                })
            }
            Token::Matrix => {
                self.advance();
                let use_brackets = if self.peek() == Some(&Token::LBracket) {
                    self.advance(); true
                } else {
                    self.match_token(Token::LParen)?; false
                };
                let rows = self.expect_int_lit()? as usize;
                let delimiter = if self.peek() == Some(&Token::Semi) { Token::Semi } else { Token::Comma };
                self.match_token(delimiter)?;
                let cols = self.expect_int_lit()? as usize;
                if use_brackets {
                    self.match_token(Token::RBracket)?;
                } else {
                    self.match_token(Token::RParen)?;
                }
                Ok(build_matrix_node(rows, cols))
            }
            Token::Norm => {
                self.advance();
                let use_brackets = if self.peek() == Some(&Token::LBracket) {
                    self.advance(); true
                } else {
                    self.match_token(Token::LParen)?; false
                };
                let size = self.expect_int_lit()? as usize;
                if use_brackets {
                    self.match_token(Token::RBracket)?;
                } else {
                    self.match_token(Token::RParen)?;
                }
                Ok(build_norm_node(size))
            }
            Token::Vec => {
                self.advance();
                let use_brackets = if self.peek() == Some(&Token::LBracket) {
                    self.advance(); true
                } else {
                    self.match_token(Token::LParen)?; false
                };
                let first_val = self.parse_expr(calls)?;
                let mut len = 1;
                if use_brackets && self.peek() == Some(&Token::Semi) {
                    self.advance();
                    len = self.expect_int_lit()? as usize;
                } else if !use_brackets {
                    if let NdaNode::Int { value } = first_val {
                        len = value as usize;
                    }
                }
                if use_brackets {
                    self.match_token(Token::RBracket)?;
                } else {
                    self.match_token(Token::RParen)?;
                }
                Ok(build_matrix_node(1, len))
            }
            Token::Ident(name) => {
                self.advance();
                if self.peek() == Some(&Token::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if self.peek() != Some(&Token::RParen) {
                        loop {
                            args.push(self.parse_expr(calls)?);
                            if self.peek() == Some(&Token::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.match_token(Token::RParen)?;

                    let temp_call_target = self.pos as u64;
                    calls.insert(temp_call_target, name);

                    let mut children = args;
                    children.push(NdaNode::Call { target: temp_call_target });

                    Ok(NdaNode::Scope { children })
                } else {
                    Ok(NdaNode::Load {
                        name_hash: hash_name(&name),
                    })
                }
            }
            _ => {
                let loc = self.peek_loc().unwrap_or(&Located { token: Token::Eof, line: 0, col: 0 });
                Err(format!("{}:{}: Unexpected token in expression: {:?}", loc.line, loc.col, tok))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::nda_lexer::NdaLexer;

    #[test]
    fn test_parse_fibonacci() {
        let src = r#"
            fn fibonacci(n: int) -> vec {
                let a = vec[1.0; 1]
                let b = vec[1.0; 1]
                loop n {
                    let temp = a
                    a = add(a, b)
                    b = temp
                }
                return a
            }

            fn main() {
                let result = fibonacci(20)
                print(result)
            }
        "#;
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let program = parser.parse_program().unwrap();

        // Should compile to a Scope node containing the functions
        assert!(matches!(program, NdaNode::Scope { .. }));
        if let NdaNode::Scope { children } = program {
            assert_eq!(children.len(), 2);
            let fib = &children[0];
            assert!(matches!(fib, NdaNode::Scope { .. }));
            
            let main_fn = &children[1];
            assert!(matches!(main_fn, NdaNode::Scope { .. }));
        }
    }

    #[test]
    fn test_parse_dynamic_loop() {
        let src = r#"
            fn main() {
                let x = 10
                loop x {
                    print(x)
                }
            }
        "#;
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let program = parser.parse_program().unwrap();

        fn has_while(node: &NdaNode) -> bool {
            match node {
                NdaNode::While { .. } => true,
                NdaNode::Scope { children } => children.iter().any(has_while),
                NdaNode::Loop { body, .. } => body.iter().any(has_while),
                NdaNode::If { then_body, else_body, .. } => {
                    then_body.iter().any(has_while) || else_body.as_ref().map_or(false, |eb| eb.iter().any(has_while))
                }
                _ => false,
            }
        }

        assert!(has_while(&program), "Dynamic loop should have been translated to a While loop");
    }
}

