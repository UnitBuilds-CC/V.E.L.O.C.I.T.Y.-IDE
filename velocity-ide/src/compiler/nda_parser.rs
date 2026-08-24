// compiler/nda_parser.rs — Parser for the NDA programming language
//
// Converts a stream of Located tokens into a compiled NdaNode AST.
// Resolves function call targets dynamically via Merkle hash propagation.
#![allow(dead_code)]

use crate::compiler::nda_lexer::{Located, NdaLexer, Token};
use crate::site_map::verifier::{CmpOp, NdaNode, VecOpKind};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Helper: compute 8-byte SHA-256 hash of a string name.
pub fn hash_name(name: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes(digest[..8].try_into().unwrap())
}

/// Helper: build synthetic matrix node of shape rows x cols.
pub fn build_matrix_node(rows: usize, cols: usize) -> NdaNode {
    let rows = rows.clamp(1, 65535) as u16;
    let cols = cols.clamp(1, 65535) as u16;
    let bitmap_bytes = rows as usize * (cols as usize).div_ceil(8);
    let sign: Vec<u8> = (0..bitmap_bytes)
        .map(|i| if i % 2 == 0 { 0xAA } else { 0x55 })
        .collect();
    let extra: Vec<u8> = (0..bitmap_bytes)
        .map(|i| if i % 2 == 0 { 0x55 } else { 0xAA })
        .collect();
    NdaNode::Matrix {
        rows,
        cols,
        scale: 0,
        sign,
        extra,
    }
}

/// Helper: build synthetic norm node of shape size.
pub fn build_norm_node(size: usize) -> NdaNode {
    let size = size.clamp(1, 65535) as u16;
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
            children: children
                .iter()
                .map(|c| resolve_calls(c, fn_map, call_names))
                .collect(),
        },
        NdaNode::Loop { count, body } => NdaNode::Loop {
            count: *count,
            body: body
                .iter()
                .map(|c| resolve_calls(c, fn_map, call_names))
                .collect(),
        },
        NdaNode::While { cond, body } => NdaNode::While {
            cond: Box::new(resolve_calls(cond, fn_map, call_names)),
            body: body
                .iter()
                .map(|c| resolve_calls(c, fn_map, call_names))
                .collect(),
        },
        NdaNode::If {
            cond,
            then_body,
            else_body,
        } => NdaNode::If {
            cond: Box::new(resolve_calls(cond, fn_map, call_names)),
            then_body: then_body
                .iter()
                .map(|c| resolve_calls(c, fn_map, call_names))
                .collect(),
            else_body: else_body.as_ref().map(|eb| {
                eb.iter()
                    .map(|c| resolve_calls(c, fn_map, call_names))
                    .collect()
            }),
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
    let report = compile_with_report(source)?;
    Ok((report.program, report.fn_hashes))
}

/// Compile NDA source with full diagnostics.
#[derive(Debug)]
pub struct ParseReport {
    /// The compiled program AST.
    pub program: NdaNode,
    /// Function name → final hash.
    pub fn_hashes: HashMap<String, u64>,
    /// Number of functions compiled.
    pub function_count: usize,
    /// Total call edges found.
    pub call_edges: usize,
    /// Total call edges resolved to known functions.
    pub call_edges_resolved: usize,
    /// Lexer errors encountered (if any).
    pub lexer_errors: Vec<String>,
    /// Names of compiled functions.
    pub function_names: Vec<String>,
}

impl ParseReport {
    /// Serialize to JSON-friendly struct.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "function_count": self.function_count,
            "function_names": self.function_names,
            "call_edges": self.call_edges,
            "call_edges_resolved": self.call_edges_resolved,
            "lexer_errors": self.lexer_errors,
            "program_hash": format!("{:016x}", self.program.hash()),
        })
    }
}

/// Compile NDA source with full diagnostics.
pub fn compile_with_report(source: &str) -> Result<ParseReport, String> {
    let mut lexer = NdaLexer::new(source);
    let (tokens, lexer_errors) = lexer.tokenize_with_errors();
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
            return Err(format!(
                "{}:{}: Expected 'fn' keyword at top level, found {}",
                loc.line, loc.col, loc.token.display_name()
            ));
        }
    }

    let mut fn_hashes: HashMap<String, u64> = functions
        .keys()
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
    let mut total_edges = 0;
    let mut resolved_edges = 0;
    for name in &sorted_names {
        let node = functions.get(name).unwrap();
        let calls = all_calls.get(name).unwrap();
        total_edges += calls.len();
        let resolved = resolve_calls(node, &fn_hashes, calls);
        final_hashes.insert(name.clone(), resolved.hash());
        children.push(resolved);
    }

    // Count resolved edges (calls whose target matches a known function hash)
    let known_hashes: std::collections::HashSet<u64> = final_hashes.values().cloned().collect();
    for calls in all_calls.values() {
        for (target, call_name) in calls {
            if let Some(&hash) = fn_hashes.get(call_name) {
                if known_hashes.contains(&hash) {
                    resolved_edges += 1;
                }
            }
        }
    }

    Ok(ParseReport {
        program: NdaNode::Scope { children },
        fn_hashes: final_hashes,
        function_count: functions.len(),
        call_edges: total_edges,
        call_edges_resolved: resolved_edges,
        lexer_errors,
        function_names: sorted_names,
    })
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
                Err(format!(
                    "{}:{}: Expected {}, found {}",
                    loc.line, loc.col, expected.display_name(), loc.token.display_name()
                ))
            }
        } else {
            Err(format!(
                "Expected {}, reached end of file",
                expected.display_name()
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        if let Some(Token::Ident(name)) = self.peek() {
            let name = name.clone();
            self.advance();
            Ok(name)
        } else {
            let loc = self.peek_loc().unwrap_or(&Located {
                token: Token::Eof,
                line: 0,
                col: 0,
            });
            Err(format!("{}:{}: Expected identifier", loc.line, loc.col))
        }
    }

    fn expect_int_lit(&mut self) -> Result<i64, String> {
        if let Some(Token::IntLit(val)) = self.peek() {
            let val = *val;
            self.advance();
            Ok(val)
        } else {
            let loc = self.peek_loc().unwrap_or(&Located {
                token: Token::Eof,
                line: 0,
                col: 0,
            });
            Err(format!(
                "{}:{}: Expected integer literal",
                loc.line, loc.col
            ))
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
                return Err(format!(
                    "{}:{}: Expected 'fn' keyword at top level",
                    loc.line, loc.col
                ));
            }
        }

        // Resolve call targets iteratively
        let mut fn_hashes: HashMap<String, u64> = functions
            .keys()
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
                    Err(format!(
                        "{}:{}: Expected type keyword (vec, matrix, norm, int)",
                        loc.line, loc.col
                    ))
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

    fn parse_statement(
        &mut self,
        calls: &mut HashMap<u64, String>,
    ) -> Result<Option<NdaNode>, String> {
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
                if self.peek() == Some(&Token::Semi) {
                    self.advance();
                }
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
                    NdaNode::Int { value } if value >= 0 => Some(NdaNode::Loop {
                        count: value as u32,
                        body,
                    }),
                    _ => {
                        // Count-down translation for dynamic loop
                        let temp_name = format!("_loop_cnt_{}", self.pos);
                        let temp_hash = hash_name(&temp_name);

                        let cond = NdaNode::Compare {
                            op: CmpOp::Gt,
                            lhs: Box::new(NdaNode::Load {
                                name_hash: temp_hash,
                            }),
                            rhs: Box::new(NdaNode::Int { value: 0 }),
                        };

                        let decrement = NdaNode::Store {
                            name_hash: temp_hash,
                            value: Box::new(NdaNode::Add {
                                lhs: Box::new(NdaNode::Load {
                                    name_hash: temp_hash,
                                }),
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
                if self.peek() == Some(&Token::Semi) {
                    self.advance();
                }
                Some(NdaNode::Return {
                    value: Box::new(val),
                })
            }
            Token::Break => {
                self.advance();
                if self.peek() == Some(&Token::Semi) {
                    self.advance();
                }
                Some(NdaNode::Break)
            }
            Token::Print => {
                self.advance();
                self.match_token(Token::LParen)?;
                let expr = self.parse_expr(calls)?;
                self.match_token(Token::RParen)?;
                if self.peek() == Some(&Token::Semi) {
                    self.advance();
                }
                Some(NdaNode::Print {
                    source: Box::new(expr),
                })
            }
            Token::Ident(name)
                if self.tokens.get(self.pos + 1).map(|l| &l.token) == Some(&Token::Assign) =>
            {
                let name = name.clone();
                self.advance(); // consume ident
                self.advance(); // consume Assign '='
                let val = self.parse_expr(calls)?;
                if self.peek() == Some(&Token::Semi) {
                    self.advance();
                }
                Some(NdaNode::Store {
                    name_hash: hash_name(&name),
                    value: Box::new(val),
                })
            }
            _ => {
                let expr = self.parse_expr(calls)?;
                if self.peek() == Some(&Token::Semi) {
                    self.advance();
                }
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
                    self.advance();
                    true
                } else {
                    self.match_token(Token::LParen)?;
                    false
                };
                let rows = self.expect_int_lit()? as usize;
                let delimiter = if self.peek() == Some(&Token::Semi) {
                    Token::Semi
                } else {
                    Token::Comma
                };
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
                    self.advance();
                    true
                } else {
                    self.match_token(Token::LParen)?;
                    false
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
                    self.advance();
                    true
                } else {
                    self.match_token(Token::LParen)?;
                    false
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
                    children.push(NdaNode::Call {
                        target: temp_call_target,
                    });

                    Ok(NdaNode::Scope { children })
                } else {
                    Ok(NdaNode::Load {
                        name_hash: hash_name(&name),
                    })
                }
            }
            _ => {
                let loc = self.peek_loc().unwrap_or(&Located {
                    token: Token::Eof,
                    line: 0,
                    col: 0,
                });
                Err(format!(
                    "{}:{}: Unexpected {} in expression",
                    loc.line, loc.col, loc.token.display_name()
                ))
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
                NdaNode::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    then_body.iter().any(has_while)
                        || else_body
                            .as_ref()
                            .is_some_and(|eb| eb.iter().any(has_while))
                }
                _ => false,
            }
        }

        assert!(
            has_while(&program),
            "Dynamic loop should have been translated to a While loop"
        );
    }

    #[test]
    fn test_compile_with_report() {
        let src = r#"
            fn helper(x: int) -> int {
                return x
            }
            fn main() {
                let x = helper(1)
                print(x)
            }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_count, 2);
        assert!(report.function_names.contains(&"helper".to_string()));
        assert!(report.function_names.contains(&"main".to_string()));
        assert!(report.call_edges > 0);
        assert!(report.lexer_errors.is_empty());
    }

    #[test]
    fn test_parse_report_json() {
        let src = "fn main() { print(42) }";
        let report = compile_with_report(src).unwrap();
        let json = report.to_json();
        assert_eq!(json["function_count"], 1);
        assert!(json["program_hash"].as_str().unwrap().len() > 0);
    }

    #[test]
    fn test_improved_error_messages() {
        // Trailing incomplete expression — parser sees '}' where it expects an expression
        let src = "fn main() {\n  let x = \n}";
        let result = compile(src);
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Should use display_name() format (contains quotes around token)
        // and NOT contain Debug format like "Token::..."
        assert!(!err.contains("Token::"), "error should not contain Debug format, got: {}", err);
        // Should contain a line:col prefix
        assert!(err.contains(':'), "error should contain line:col, got: {}", err);
    }

    // ─── Helper function tests ─────────────────────────────────────────────

    #[test]
    fn hash_name_deterministic() {
        let h1 = hash_name("foo");
        let h2 = hash_name("foo");
        assert_eq!(h1, h2);
        let h3 = hash_name("bar");
        assert_ne!(h1, h3);
    }

    #[test]
    fn hash_name_nonzero() {
        // Extremely unlikely to be zero for any input
        assert_ne!(hash_name("test"), 0);
        assert_ne!(hash_name(""), 0);
    }

    #[test]
    fn build_matrix_node_dimensions() {
        let node = build_matrix_node(128, 896);
        match node {
            NdaNode::Matrix { rows, cols, scale, .. } => {
                assert_eq!(rows, 128);
                assert_eq!(cols, 896);
                assert_eq!(scale, 0);
            }
            _ => panic!("Expected Matrix node"),
        }
    }

    #[test]
    fn build_matrix_node_clamping() {
        // 0 should be clamped to 1
        let node = build_matrix_node(0, 0);
        match node {
            NdaNode::Matrix { rows, cols, .. } => {
                assert_eq!(rows, 1);
                assert_eq!(cols, 1);
            }
            _ => panic!("Expected Matrix node"),
        }
    }

    #[test]
    fn build_matrix_node_sign_extra_pattern() {
        let node = build_matrix_node(2, 8);
        match node {
            NdaNode::Matrix { sign, extra, .. } => {
                // sign alternates 0xAA, 0x55; extra alternates 0x55, 0xAA
                assert_eq!(sign[0], 0xAA);
                assert_eq!(sign[1], 0x55);
                assert_eq!(extra[0], 0x55);
                assert_eq!(extra[1], 0xAA);
            }
            _ => panic!("Expected Matrix node"),
        }
    }

    #[test]
    fn build_norm_node_size() {
        let node = build_norm_node(128);
        match node {
            NdaNode::Norm { size, weight, bias } => {
                assert_eq!(size, 128);
                assert!(!weight.is_empty());
                assert!(!bias.is_empty());
            }
            _ => panic!("Expected Norm node"),
        }
    }

    #[test]
    fn build_norm_node_clamping() {
        let node = build_norm_node(0);
        match node {
            NdaNode::Norm { size, .. } => assert_eq!(size, 1),
            _ => panic!("Expected Norm node"),
        }
    }

    // ─── Parse construct tests ─────────────────────────────────────────────

    fn parse_ok(src: &str) -> NdaNode {
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        parser.parse_program().unwrap()
    }

    fn parse_err(src: &str) -> String {
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        parser.parse_program().unwrap_err()
    }

    #[test]
    fn parse_if_else() {
        let src = r#"
            fn main() {
                let x = 1
                if x {
                    print(x)
                } else {
                    print(0)
                }
            }
        "#;
        let program = parse_ok(src);
        // Should contain an If node somewhere
        fn has_if(node: &NdaNode) -> bool {
            match node {
                NdaNode::If { .. } => true,
                NdaNode::Scope { children } => children.iter().any(has_if),
                _ => false,
            }
        }
        assert!(has_if(&program), "Expected If node in AST");
    }

    #[test]
    fn parse_while_loop() {
        let src = r#"
            fn main() {
                let x = 10
                while x {
                    x = add(x, -1)
                }
            }
        "#;
        let program = parse_ok(src);
        fn has_while(node: &NdaNode) -> bool {
            match node {
                NdaNode::While { .. } => true,
                NdaNode::Scope { children } => children.iter().any(has_while),
                _ => false,
            }
        }
        assert!(has_while(&program));
    }

    #[test]
    fn parse_static_loop() {
        let src = r#"
            fn main() {
                loop 5 {
                    print(1)
                }
            }
        "#;
        let program = parse_ok(src);
        fn has_loop(node: &NdaNode) -> bool {
            match node {
                NdaNode::Loop { count, .. } => *count == 5,
                NdaNode::Scope { children } => children.iter().any(has_loop),
                _ => false,
            }
        }
        assert!(has_loop(&program));
    }

    #[test]
    fn parse_comparison_operators() {
        let src = r#"
            fn main() {
                let a = 1
                let b = 2
                if a < b {
                    print(a)
                }
                if a == b {
                    print(b)
                }
                if a != b {
                    print(0)
                }
            }
        "#;
        let program = parse_ok(src);
        fn count_compares(node: &NdaNode) -> usize {
            let mut count = 0;
            match node {
                NdaNode::Compare { lhs, rhs, .. } => {
                    count += 1;
                    count += count_compares(lhs);
                    count += count_compares(rhs);
                }
                NdaNode::Scope { children } => {
                    for c in children { count += count_compares(c); }
                }
                NdaNode::If { cond, then_body, else_body } => {
                    count += count_compares(cond);
                    for c in then_body { count += count_compares(c); }
                    if let Some(eb) = else_body {
                        for c in eb { count += count_compares(c); }
                    }
                }
                NdaNode::Let { init, .. } => count += count_compares(init),
                _ => {}
            }
            count
        }
        assert_eq!(count_compares(&program), 3);
    }

    #[test]
    fn parse_unary_negation() {
        let src = r#"
            fn main() {
                let x = -1
                print(x)
            }
        "#;
        let program = parse_ok(src);
        fn has_negate(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, .. } => matches!(op, VecOpKind::Negate),
                NdaNode::Scope { children } => children.iter().any(has_negate),
                NdaNode::Let { init, .. } => has_negate(init),
                NdaNode::Print { source } => has_negate(source),
                _ => false,
            }
        }
        assert!(has_negate(&program));
    }

    #[test]
    fn parse_silu_builtin() {
        let src = "fn main() { let x = silu(42) }";
        let program = parse_ok(src);
        fn has_silu(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, .. } => matches!(op, VecOpKind::SiLU),
                NdaNode::Scope { children } => children.iter().any(has_silu),
                NdaNode::Let { init, .. } => has_silu(init),
                _ => false,
            }
        }
        assert!(has_silu(&program));
    }

    #[test]
    fn parse_abs_builtin() {
        let src = "fn main() { let x = abs(42) }";
        let program = parse_ok(src);
        fn has_abs(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, .. } => matches!(op, VecOpKind::Abs),
                NdaNode::Scope { children } => children.iter().any(has_abs),
                NdaNode::Let { init, .. } => has_abs(init),
                _ => false,
            }
        }
        assert!(has_abs(&program));
    }

    #[test]
    fn parse_reduce_sum_builtin() {
        let src = "fn main() { let x = reduce_sum(42) }";
        let program = parse_ok(src);
        fn has_reduce_sum(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, .. } => matches!(op, VecOpKind::ReduceSum),
                NdaNode::Scope { children } => children.iter().any(has_reduce_sum),
                NdaNode::Let { init, .. } => has_reduce_sum(init),
                _ => false,
            }
        }
        assert!(has_reduce_sum(&program));
    }

    #[test]
    fn parse_matrix_literal() {
        let src = "fn main() { let m = matrix[128, 896] }";
        let program = parse_ok(src);
        fn has_matrix(node: &NdaNode) -> bool {
            match node {
                NdaNode::Matrix { rows, cols, .. } => *rows == 128 && *cols == 896,
                NdaNode::Scope { children } => children.iter().any(has_matrix),
                NdaNode::Let { init, .. } => has_matrix(init),
                _ => false,
            }
        }
        assert!(has_matrix(&program));
    }

    #[test]
    fn parse_norm_literal() {
        let src = "fn main() { let n = norm[256] }";
        let program = parse_ok(src);
        fn has_norm(node: &NdaNode) -> bool {
            match node {
                NdaNode::Norm { size, .. } => *size == 256,
                NdaNode::Scope { children } => children.iter().any(has_norm),
                NdaNode::Let { init, .. } => has_norm(init),
                _ => false,
            }
        }
        assert!(has_norm(&program));
    }

    #[test]
    fn parse_break_statement() {
        let src = r#"
            fn main() {
                loop 10 {
                    break
                }
            }
        "#;
        let program = parse_ok(src);
        fn has_break(node: &NdaNode) -> bool {
            match node {
                NdaNode::Break => true,
                NdaNode::Scope { children } => children.iter().any(has_break),
                NdaNode::Loop { body, .. } => body.iter().any(has_break),
                _ => false,
            }
        }
        assert!(has_break(&program));
    }

    #[test]
    fn parse_return_statement() {
        let src = "fn main() { return 42 }";
        let program = parse_ok(src);
        fn has_return(node: &NdaNode) -> bool {
            match node {
                NdaNode::Return { value } => matches!(**value, NdaNode::Int { value: 42 }),
                NdaNode::Scope { children } => children.iter().any(has_return),
                _ => false,
            }
        }
        assert!(has_return(&program));
    }

    #[test]
    fn parse_print_statement() {
        let src = "fn main() { print(42) }";
        let program = parse_ok(src);
        fn has_print(node: &NdaNode) -> bool {
            match node {
                NdaNode::Print { .. } => true,
                NdaNode::Scope { children } => children.iter().any(has_print),
                _ => false,
            }
        }
        assert!(has_print(&program));
    }

    #[test]
    fn parse_empty_function() {
        let src = "fn empty() {}";
        let program = parse_ok(src);
        match program {
            NdaNode::Scope { children } => {
                assert_eq!(children.len(), 1);
            }
            _ => panic!("Expected Scope"),
        }
    }

    #[test]
    fn parse_function_with_params() {
        let src = "fn myfunc(a: int, b: int) -> int { return a }";
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_count, 1);
        assert_eq!(report.function_names, vec!["myfunc"]);
    }

    #[test]
    fn parse_function_with_return_type() {
        let src = "fn main() -> vec { return 42 }";
        let program = parse_ok(src);
        assert!(matches!(program, NdaNode::Scope { .. }));
    }

    // ─── Error case tests ──────────────────────────────────────────────────

    #[test]
    fn error_missing_fn_keyword() {
        let err = parse_err("main() { }");
        assert!(err.contains("fn"), "Expected 'fn' in error, got: {}", err);
    }

    #[test]
    fn error_missing_closing_brace() {
        let src = "fn main() { print(1)";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
    }

    #[test]
    fn error_missing_identifier() {
        let src = "fn () { }";
        let result = parse_err(src);
        assert!(result.contains("identifier") || result.contains("Expected"));
    }

    #[test]
    fn error_unexpected_token_in_expr() {
        let src = "fn main() { let x = } }";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
    }

    // ─── Call resolution tests ─────────────────────────────────────────────

    #[test]
    fn resolve_calls_replaces_targets() {
        let call_node = NdaNode::Call { target: 42 };
        let mut fn_map = HashMap::new();
        fn_map.insert("helper".to_string(), 99u64);
        let mut call_names = HashMap::new();
        call_names.insert(42u64, "helper".to_string());

        let resolved = resolve_calls(&call_node, &fn_map, &call_names);
        match resolved {
            NdaNode::Call { target } => assert_eq!(target, 99),
            _ => panic!("Expected Call node"),
        }
    }

    #[test]
    fn resolve_calls_unresolved_stays() {
        let call_node = NdaNode::Call { target: 42 };
        let fn_map = HashMap::new();
        let mut call_names = HashMap::new();
        call_names.insert(42u64, "unknown".to_string());

        let resolved = resolve_calls(&call_node, &fn_map, &call_names);
        match resolved {
            NdaNode::Call { target } => assert_eq!(target, 42), // unchanged
            _ => panic!("Expected Call node"),
        }
    }

    #[test]
    fn multi_function_call_resolution() {
        let src = r#"
            fn helper() -> int {
                return 42
            }
            fn main() {
                let x = helper()
                print(x)
            }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_count, 2);
        assert!(report.call_edges >= 1);
        // The call from main -> helper should be resolved
        assert!(report.call_edges_resolved >= 1);
    }

    // ─── Compile with report tests ─────────────────────────────────────────

    #[test]
    fn compile_with_report_empty_source() {
        let report = compile_with_report("").unwrap();
        assert_eq!(report.function_count, 0);
        assert!(report.function_names.is_empty());
    }

    #[test]
    fn compile_with_report_error() {
        let result = compile_with_report("not_a_function");
        assert!(result.is_err());
    }

    #[test]
    fn parse_report_to_json_structure() {
        let src = r#"
            fn a() { return 1 }
            fn b() { return a() }
        "#;
        let report = compile_with_report(src).unwrap();
        let json = report.to_json();
        assert_eq!(json["function_count"], 2);
        assert!(json["function_names"].as_array().unwrap().len() == 2);
        assert!(json["program_hash"].as_str().unwrap().len() == 16);
    }

    #[test]
    fn parse_add_expression() {
        let src = "fn main() { let x = add(1, 2) }";
        let program = parse_ok(src);
        fn has_add(node: &NdaNode) -> bool {
            match node {
                NdaNode::Add { .. } => true,
                NdaNode::Scope { children } => children.iter().any(has_add),
                NdaNode::Let { init, .. } => has_add(init),
                _ => false,
            }
        }
        assert!(has_add(&program));
    }

    #[test]
    fn parse_store_assignment() {
        let src = r#"
            fn main() {
                let x = 1
                x = 2
            }
        "#;
        let program = parse_ok(src);
        fn has_store(node: &NdaNode) -> bool {
            match node {
                NdaNode::Store { .. } => true,
                NdaNode::Scope { children } => children.iter().any(has_store),
                _ => false,
            }
        }
        assert!(has_store(&program));
    }

    #[test]
    fn parse_nested_if_else() {
        let src = r#"
            fn main() {
                let x = 1
                if x {
                    if x {
                        print(1)
                    } else {
                        print(2)
                    }
                }
            }
        "#;
        let program = parse_ok(src);
        fn count_ifs(node: &NdaNode) -> usize {
            let mut count = 0;
            match node {
                NdaNode::If { cond, then_body, else_body } => {
                    count += 1;
                    count += count_ifs(cond);
                    for c in then_body { count += count_ifs(c); }
                    if let Some(eb) = else_body {
                        for c in eb { count += count_ifs(c); }
                    }
                }
                NdaNode::Scope { children } => {
                    for c in children { count += count_ifs(c); }
                }
                NdaNode::Let { init, .. } => count += count_ifs(init),
                _ => {}
            }
            count
        }
        assert_eq!(count_ifs(&program), 2);
    }
}
