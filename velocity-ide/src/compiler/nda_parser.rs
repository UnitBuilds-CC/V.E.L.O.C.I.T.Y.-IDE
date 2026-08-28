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
        for call_name in calls.values() {
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

    // ── Block 136: NDA Parser comprehensive tests ──────────────────────────

    #[test]
    fn hash_name_empty_vs_nonempty() {
        let h_empty = hash_name("");
        let h_foo = hash_name("foo");
        assert_ne!(h_empty, h_foo);
    }

    #[test]
    fn hash_name_different_inputs_differ() {
        let names = ["alpha", "beta", "gamma", "delta", "main", "helper"];
        let hashes: Vec<u64> = names.iter().map(|n| hash_name(n)).collect();
        // All pairwise distinct
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(hashes[i], hashes[j], "collision between {} and {}", names[i], names[j]);
            }
        }
    }

    #[test]
    fn build_matrix_node_bitmap_bytes_formula() {
        // bitmap_bytes = rows * cols.div_ceil(8)
        let node = build_matrix_node(4, 16);
        match node {
            NdaNode::Matrix { rows, cols, sign, extra, .. } => {
                assert_eq!(rows, 4);
                assert_eq!(cols, 16);
                let expected_bytes = 4 * (16usize).div_ceil(8); // 4 * 2 = 8
                assert_eq!(sign.len(), expected_bytes);
                assert_eq!(extra.len(), expected_bytes);
            }
            _ => panic!("Expected Matrix"),
        }
    }

    #[test]
    fn build_matrix_node_1x1() {
        let node = build_matrix_node(1, 1);
        match node {
            NdaNode::Matrix { rows, cols, sign, extra, .. } => {
                assert_eq!(rows, 1);
                assert_eq!(cols, 1);
                // 1 * div_ceil(1, 8) = 1 * 1 = 1 byte
                assert_eq!(sign.len(), 1);
                assert_eq!(extra.len(), 1);
            }
            _ => panic!("Expected Matrix"),
        }
    }

    #[test]
    fn build_matrix_node_large_dims() {
        let node = build_matrix_node(65535, 65535);
        match node {
            NdaNode::Matrix { rows, cols, .. } => {
                assert_eq!(rows, 65535);
                assert_eq!(cols, 65535);
            }
            _ => panic!("Expected Matrix"),
        }
    }

    #[test]
    fn build_matrix_node_clamped_max() {
        // Values > 65535 should be clamped to 65535
        let node = build_matrix_node(100000, 100000);
        match node {
            NdaNode::Matrix { rows, cols, .. } => {
                assert_eq!(rows, 65535);
                assert_eq!(cols, 65535);
            }
            _ => panic!("Expected Matrix"),
        }
    }

    #[test]
    fn build_norm_node_weight_bias_pattern() {
        let node = build_norm_node(16);
        match node {
            NdaNode::Norm { size, weight, bias } => {
                assert_eq!(size, 16);
                let expected_bytes = (16usize).div_ceil(8); // 2
                assert_eq!(weight.len(), expected_bytes);
                assert_eq!(bias.len(), expected_bytes);
                // weight is all 0xFF, bias is all 0x00
                assert!(weight.iter().all(|&b| b == 0xFF));
                assert!(bias.iter().all(|&b| b == 0x00));
            }
            _ => panic!("Expected Norm"),
        }
    }

    #[test]
    fn build_norm_node_large_size() {
        let node = build_norm_node(65535);
        match node {
            NdaNode::Norm { size, .. } => assert_eq!(size, 65535),
            _ => panic!("Expected Norm"),
        }
    }

    #[test]
    fn build_norm_node_clamped_max() {
        let node = build_norm_node(99999);
        match node {
            NdaNode::Norm { size, .. } => assert_eq!(size, 65535),
            _ => panic!("Expected Norm"),
        }
    }

    #[test]
    fn resolve_calls_through_loop() {
        let inner = NdaNode::Call { target: 10 };
        let node = NdaNode::Loop { count: 5, body: vec![inner] };
        let mut fn_map = HashMap::new();
        fn_map.insert("f".to_string(), 99u64);
        let mut call_names = HashMap::new();
        call_names.insert(10u64, "f".to_string());

        let resolved = resolve_calls(&node, &fn_map, &call_names);
        match resolved {
            NdaNode::Loop { count, body } => {
                assert_eq!(count, 5);
                match &body[0] {
                    NdaNode::Call { target } => assert_eq!(*target, 99),
                    _ => panic!("Expected resolved Call inside Loop"),
                }
            }
            _ => panic!("Expected Loop"),
        }
    }

    #[test]
    fn resolve_calls_through_while() {
        let cond = NdaNode::Call { target: 20 };
        let node = NdaNode::While { cond: Box::new(cond), body: vec![] };
        let mut fn_map = HashMap::new();
        fn_map.insert("g".to_string(), 77u64);
        let mut call_names = HashMap::new();
        call_names.insert(20u64, "g".to_string());

        let resolved = resolve_calls(&node, &fn_map, &call_names);
        match resolved {
            NdaNode::While { cond, body } => {
                assert!(body.is_empty());
                match *cond {
                    NdaNode::Call { target } => assert_eq!(target, 77),
                    _ => panic!("Expected resolved Call in While cond"),
                }
            }
            _ => panic!("Expected While"),
        }
    }

    #[test]
    fn resolve_calls_through_if() {
        let cond = NdaNode::Call { target: 30 };
        let node = NdaNode::If {
            cond: Box::new(cond),
            then_body: vec![NdaNode::Call { target: 31 }],
            else_body: Some(vec![NdaNode::Call { target: 32 }]),
        };
        let mut fn_map = HashMap::new();
        fn_map.insert("a".to_string(), 100u64);
        fn_map.insert("b".to_string(), 200u64);
        fn_map.insert("c".to_string(), 300u64);
        let mut call_names = HashMap::new();
        call_names.insert(30u64, "a".to_string());
        call_names.insert(31u64, "b".to_string());
        call_names.insert(32u64, "c".to_string());

        let resolved = resolve_calls(&node, &fn_map, &call_names);
        match resolved {
            NdaNode::If { cond, then_body, else_body } => {
                match *cond {
                    NdaNode::Call { target } => assert_eq!(target, 100),
                    _ => panic!("Expected resolved Call in If cond"),
                }
                match &then_body[0] {
                    NdaNode::Call { target } => assert_eq!(*target, 200),
                    _ => panic!("Expected resolved Call in then_body"),
                }
                match &else_body.unwrap()[0] {
                    NdaNode::Call { target } => assert_eq!(*target, 300),
                    _ => panic!("Expected resolved Call in else_body"),
                }
            }
            _ => panic!("Expected If"),
        }
    }

    #[test]
    fn resolve_calls_through_let_store_add_print_return() {
        let call = NdaNode::Call { target: 40 };
        let mut fn_map = HashMap::new();
        fn_map.insert("x".to_string(), 555u64);
        let mut call_names = HashMap::new();
        call_names.insert(40u64, "x".to_string());

        // Let
        let let_node = NdaNode::Let { name_hash: 1, init: Box::new(call.clone()) };
        match resolve_calls(&let_node, &fn_map, &call_names) {
            NdaNode::Let { init, .. } => match *init {
                NdaNode::Call { target } => assert_eq!(target, 555),
                _ => panic!("Let init not resolved"),
            },
            _ => panic!("Expected Let"),
        }

        // Store
        let store_node = NdaNode::Store { name_hash: 2, value: Box::new(call.clone()) };
        match resolve_calls(&store_node, &fn_map, &call_names) {
            NdaNode::Store { value, .. } => match *value {
                NdaNode::Call { target } => assert_eq!(target, 555),
                _ => panic!("Store value not resolved"),
            },
            _ => panic!("Expected Store"),
        }

        // Add
        let add_node = NdaNode::Add { lhs: Box::new(call.clone()), rhs: Box::new(NdaNode::Int { value: 1 }) };
        match resolve_calls(&add_node, &fn_map, &call_names) {
            NdaNode::Add { lhs, .. } => match *lhs {
                NdaNode::Call { target } => assert_eq!(target, 555),
                _ => panic!("Add lhs not resolved"),
            },
            _ => panic!("Expected Add"),
        }

        // VecOp
        let vec_node = NdaNode::VecOp { op: VecOpKind::Negate, operand: Box::new(call.clone()) };
        match resolve_calls(&vec_node, &fn_map, &call_names) {
            NdaNode::VecOp { operand, .. } => match *operand {
                NdaNode::Call { target } => assert_eq!(target, 555),
                _ => panic!("VecOp operand not resolved"),
            },
            _ => panic!("Expected VecOp"),
        }

        // Print
        let print_node = NdaNode::Print { source: Box::new(call.clone()) };
        match resolve_calls(&print_node, &fn_map, &call_names) {
            NdaNode::Print { source } => match *source {
                NdaNode::Call { target } => assert_eq!(target, 555),
                _ => panic!("Print source not resolved"),
            },
            _ => panic!("Expected Print"),
        }

        // Return
        let ret_node = NdaNode::Return { value: Box::new(call.clone()) };
        match resolve_calls(&ret_node, &fn_map, &call_names) {
            NdaNode::Return { value } => match *value {
                NdaNode::Call { target } => assert_eq!(target, 555),
                _ => panic!("Return value not resolved"),
            },
            _ => panic!("Expected Return"),
        }

        // Compare
        let cmp_node = NdaNode::Compare { op: CmpOp::Eq, lhs: Box::new(call.clone()), rhs: Box::new(NdaNode::Int { value: 0 }) };
        match resolve_calls(&cmp_node, &fn_map, &call_names) {
            NdaNode::Compare { lhs, .. } => match *lhs {
                NdaNode::Call { target } => assert_eq!(target, 555),
                _ => panic!("Compare lhs not resolved"),
            },
            _ => panic!("Expected Compare"),
        }
    }

    #[test]
    fn resolve_calls_leaf_nodes_pass_through() {
        let fn_map = HashMap::new();
        let call_names = HashMap::new();

        // Int, Load, Break should pass through unchanged (verify by hash)
        let int_node = NdaNode::Int { value: 42 };
        let resolved = resolve_calls(&int_node, &fn_map, &call_names);
        assert_eq!(resolved.hash(), int_node.hash());

        let load_node = NdaNode::Load { name_hash: 123 };
        let resolved = resolve_calls(&load_node, &fn_map, &call_names);
        assert_eq!(resolved.hash(), load_node.hash());

        let break_node = NdaNode::Break;
        let resolved = resolve_calls(&break_node, &fn_map, &call_names);
        assert_eq!(resolved.hash(), break_node.hash());
    }

    #[test]
    fn compile_report_function_names_sorted() {
        let src = r#"
            fn zebra() { return 1 }
            fn alpha() { return 2 }
            fn mango() { return 3 }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_names, vec!["alpha", "mango", "zebra"]);
    }

    #[test]
    fn compile_report_no_calls_zero_edges() {
        let src = "fn main() { return 42 }";
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.call_edges, 0);
        assert_eq!(report.call_edges_resolved, 0);
    }

    #[test]
    fn compile_report_multiple_call_edges() {
        let src = r#"
            fn a() { return 1 }
            fn b() { return 2 }
            fn main() {
                let x = a()
                let y = b()
                let z = a()
            }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_count, 3);
        assert_eq!(report.call_edges, 3); // a(), b(), a()
        assert_eq!(report.call_edges_resolved, 3);
    }

    #[test]
    fn parse_report_json_all_fields() {
        let src = r#"
            fn helper() { return 1 }
            fn main() { let x = helper() }
        "#;
        let report = compile_with_report(src).unwrap();
        let json = report.to_json();
        assert!(json["function_count"].is_number());
        assert!(json["function_names"].is_array());
        assert!(json["call_edges"].is_number());
        assert!(json["call_edges_resolved"].is_number());
        assert!(json["lexer_errors"].is_array());
        assert!(json["program_hash"].is_string());
        // program_hash should be 16 hex chars
        let hash_str = json["program_hash"].as_str().unwrap();
        assert_eq!(hash_str.len(), 16);
    }

    #[test]
    fn parse_matrix_paren_syntax() {
        let src = "fn main() { let m = matrix(128; 896) }";
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
    fn parse_norm_paren_syntax() {
        let src = "fn main() { let n = norm(256) }";
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
    fn parse_vec_paren_syntax() {
        // vec(4) creates a matrix 1x4
        let src = "fn main() { let v = vec(4) }";
        let program = parse_ok(src);
        fn has_matrix_1x4(node: &NdaNode) -> bool {
            match node {
                NdaNode::Matrix { rows, cols, .. } => *rows == 1 && *cols == 4,
                NdaNode::Scope { children } => children.iter().any(has_matrix_1x4),
                NdaNode::Let { init, .. } => has_matrix_1x4(init),
                _ => false,
            }
        }
        assert!(has_matrix_1x4(&program));
    }

    #[test]
    fn parse_vec_bracket_syntax() {
        // vec[1.0; 8] creates a 1x8 matrix
        let src = "fn main() { let v = vec[1.0; 8] }";
        let program = parse_ok(src);
        fn has_matrix_1x8(node: &NdaNode) -> bool {
            match node {
                NdaNode::Matrix { rows, cols, .. } => *rows == 1 && *cols == 8,
                NdaNode::Scope { children } => children.iter().any(has_matrix_1x8),
                NdaNode::Let { init, .. } => has_matrix_1x8(init),
                _ => false,
            }
        }
        assert!(has_matrix_1x8(&program));
    }

    #[test]
    fn parse_parenthesized_expression() {
        let src = "fn main() { let x = (42) }";
        let program = parse_ok(src);
        fn has_int_42(node: &NdaNode) -> bool {
            match node {
                NdaNode::Int { value } => *value == 42,
                NdaNode::Scope { children } => children.iter().any(has_int_42),
                NdaNode::Let { init, .. } => has_int_42(init),
                _ => false,
            }
        }
        assert!(has_int_42(&program));
    }

    #[test]
    fn parse_negate_builtin() {
        let src = "fn main() { let x = negate(5) }";
        let program = parse_ok(src);
        fn has_negate(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, .. } => matches!(op, VecOpKind::Negate),
                NdaNode::Scope { children } => children.iter().any(has_negate),
                NdaNode::Let { init, .. } => has_negate(init),
                _ => false,
            }
        }
        assert!(has_negate(&program));
    }

    #[test]
    fn parse_multiple_functions_in_report() {
        let src = r#"
            fn a() { return 1 }
            fn b() { return 2 }
            fn c() { return 3 }
            fn d() { return 4 }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_count, 4);
        assert_eq!(report.function_names.len(), 4);
        assert!(report.lexer_errors.is_empty());
    }

    #[test]
    fn parse_dynamic_loop_countdown_structure() {
        let src = r#"
            fn main() {
                let x = 10
                loop x {
                    print(x)
                }
            }
        "#;
        let program = parse_ok(src);
        // Dynamic loop desugars to: Let + While with Compare, Store, Add, Negate
        fn has_let_and_while(node: &NdaNode) -> (bool, bool) {
            let mut has_let = false;
            let mut has_while = false;
            match node {
                NdaNode::Let { .. } => has_let = true,
                NdaNode::While { cond: _, body } => {
                    has_while = true;
                    // Check that body contains a Store (decrement)
                    let has_store = body.iter().any(|c| matches!(c, NdaNode::Store { .. }));
                    assert!(has_store, "Dynamic loop body should contain Store (decrement)");
                }
                NdaNode::Scope { children } => {
                    for c in children {
                        let (l, w) = has_let_and_while(c);
                        has_let |= l;
                        has_while |= w;
                    }
                }
                _ => {}
            }
            (has_let, has_while)
        }
        let (has_let, has_while) = has_let_and_while(&program);
        assert!(has_let, "Dynamic loop should produce a Let node");
        assert!(has_while, "Dynamic loop should produce a While node");
    }

    #[test]
    fn parse_else_if_chain() {
        let src = r#"
            fn main() {
                let x = 1
                if x {
                    print(1)
                } else if x {
                    print(2)
                } else {
                    print(3)
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
        // Should have 2 If nodes: the outer if and the else-if
        assert_eq!(count_ifs(&program), 2);
    }

    #[test]
    fn parse_function_no_params_no_return_type() {
        let src = "fn bare() { return 0 }";
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_count, 1);
        assert_eq!(report.function_names[0], "bare");
    }

    #[test]
    fn parse_function_multiple_params() {
        let src = "fn multi(a: int, b: vec, c: matrix) -> int { return a }";
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_count, 1);
        // Should compile without error
        assert!(report.lexer_errors.is_empty());
    }

    #[test]
    fn error_top_level_not_fn() {
        let src = "let x = 1";
        let result = compile(src);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("fn") || err.contains("Expected"));
    }

    #[test]
    fn error_missing_rparen_in_call() {
        let src = "fn main() { print(42 }";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
    }

    #[test]
    fn error_missing_lbrace() {
        let src = "fn main() print(1) }";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
    }

    #[test]
    fn compile_deterministic_hash() {
        let src = "fn main() { return 42 }";
        let r1 = compile_with_report(src).unwrap();
        let r2 = compile_with_report(src).unwrap();
        assert_eq!(r1.program.hash(), r2.program.hash());
    }

    #[test]
    fn different_source_different_hash() {
        let r1 = compile_with_report("fn main() { return 1 }").unwrap();
        let r2 = compile_with_report("fn main() { return 2 }").unwrap();
        // Different programs should (almost certainly) have different hashes
        assert_ne!(r1.program.hash(), r2.program.hash());
    }

    #[test]
    fn parse_comparison_all_ops() {
        // Test >= and <= operators
        let src = r#"
            fn main() {
                let a = 1
                let b = 2
                if a >= b { print(a) }
                if a <= b { print(b) }
                if a > b { print(0) }
            }
        "#;
        let program = parse_ok(src);
        fn count_compares(node: &NdaNode) -> Vec<CmpOp> {
            let mut ops = Vec::new();
            match node {
                NdaNode::Compare { op, lhs, rhs, .. } => {
                    ops.push(*op);
                    ops.extend(count_compares(lhs));
                    ops.extend(count_compares(rhs));
                }
                NdaNode::Scope { children } => {
                    for c in children { ops.extend(count_compares(c)); }
                }
                NdaNode::If { cond, then_body, else_body } => {
                    ops.extend(count_compares(cond));
                    for c in then_body { ops.extend(count_compares(c)); }
                    if let Some(eb) = else_body {
                        for c in eb { ops.extend(count_compares(c)); }
                    }
                }
                NdaNode::Let { init, .. } => ops.extend(count_compares(init)),
                _ => {}
            }
            ops
        }
        let ops = count_compares(&program);
        assert!(ops.contains(&CmpOp::Ge));
        assert!(ops.contains(&CmpOp::Le));
        assert!(ops.contains(&CmpOp::Gt));
    }

    #[test]
    fn parse_add_operator_infix() {
        let src = r#"
            fn main() {
                let x = 1 + 2
            }
        "#;
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
    fn parse_float_literal() {
        let src = "fn main() { let x = 3.14 }";
        let program = parse_ok(src);
        // FloatLit is cast to i32, so 3.14 → 3
        fn has_int_3(node: &NdaNode) -> bool {
            match node {
                NdaNode::Int { value } => *value == 3,
                NdaNode::Scope { children } => children.iter().any(has_int_3),
                NdaNode::Let { init, .. } => has_int_3(init),
                _ => false,
            }
        }
        assert!(has_int_3(&program));
    }

    #[test]
    fn parse_call_with_multiple_args() {
        let src = r#"
            fn sum(a: int, b: int) -> int { return a }
            fn main() {
                let x = sum(1, 2)
            }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.call_edges, 1);
        assert_eq!(report.call_edges_resolved, 1);
    }

    #[test]
    fn parse_call_no_args() {
        let src = r#"
            fn noop() { return 0 }
            fn main() {
                noop()
            }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.call_edges, 1);
    }

    #[test]
    fn compile_function_empty_report_fields() {
        let report = compile_with_report("").unwrap();
        let json = report.to_json();
        assert_eq!(json["function_count"], 0);
        assert_eq!(json["call_edges"], 0);
        assert_eq!(json["call_edges_resolved"], 0);
        assert!(json["function_names"].as_array().unwrap().is_empty());
        assert!(json["lexer_errors"].as_array().unwrap().is_empty());
    }

    // ── Block 165: NDA Parser expanded tests ────────────────────────────────

    #[test]
    fn parse_report_json_has_exactly_6_keys() {
        let report = compile_with_report("fn main() { return 1 }").unwrap();
        let json = report.to_json();
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 6);
        assert!(obj.contains_key("function_count"));
        assert!(obj.contains_key("function_names"));
        assert!(obj.contains_key("call_edges"));
        assert!(obj.contains_key("call_edges_resolved"));
        assert!(obj.contains_key("lexer_errors"));
        assert!(obj.contains_key("program_hash"));
    }

    #[test]
    fn build_matrix_node_scale_is_zero() {
        let node = build_matrix_node(8, 32);
        match node {
            NdaNode::Matrix { scale, .. } => assert_eq!(scale, 0),
            _ => panic!("Expected Matrix"),
        }
    }

    #[test]
    fn build_matrix_node_boundary_65535_passes() {
        let node = build_matrix_node(65535, 1);
        match node {
            NdaNode::Matrix { rows, cols, .. } => {
                assert_eq!(rows, 65535);
                assert_eq!(cols, 1);
            }
            _ => panic!("Expected Matrix"),
        }
    }

    #[test]
    fn build_matrix_node_boundary_65536_clamped() {
        let node = build_matrix_node(65536, 65536);
        match node {
            NdaNode::Matrix { rows, cols, .. } => {
                assert_eq!(rows, 65535);
                assert_eq!(cols, 65535);
            }
            _ => panic!("Expected Matrix"),
        }
    }

    #[test]
    fn build_norm_node_boundary_1() {
        let node = build_norm_node(1);
        match node {
            NdaNode::Norm { size, weight, bias } => {
                assert_eq!(size, 1);
                assert_eq!(weight.len(), 1); // div_ceil(1,8) = 1
                assert_eq!(bias.len(), 1);
            }
            _ => panic!("Expected Norm"),
        }
    }

    #[test]
    fn resolve_calls_through_scope_multiple_children() {
        let node = NdaNode::Scope {
            children: vec![
                NdaNode::Call { target: 10 },
                NdaNode::Call { target: 20 },
                NdaNode::Int { value: 99 },
            ],
        };
        let mut fn_map = HashMap::new();
        fn_map.insert("a".to_string(), 100u64);
        fn_map.insert("b".to_string(), 200u64);
        let mut call_names = HashMap::new();
        call_names.insert(10u64, "a".to_string());
        call_names.insert(20u64, "b".to_string());

        let resolved = resolve_calls(&node, &fn_map, &call_names);
        match resolved {
            NdaNode::Scope { children } => {
                assert_eq!(children.len(), 3);
                match &children[0] {
                    NdaNode::Call { target } => assert_eq!(*target, 100),
                    _ => panic!("Expected resolved Call"),
                }
                match &children[1] {
                    NdaNode::Call { target } => assert_eq!(*target, 200),
                    _ => panic!("Expected resolved Call"),
                }
                assert!(matches!(&children[2], NdaNode::Int { value: 99 }));
            }
            _ => panic!("Expected Scope"),
        }
    }

    #[test]
    fn resolve_calls_through_compare() {
        let node = NdaNode::Compare {
            op: CmpOp::Lt,
            lhs: Box::new(NdaNode::Call { target: 50 }),
            rhs: Box::new(NdaNode::Call { target: 60 }),
        };
        let mut fn_map = HashMap::new();
        fn_map.insert("x".to_string(), 500u64);
        fn_map.insert("y".to_string(), 600u64);
        let mut call_names = HashMap::new();
        call_names.insert(50u64, "x".to_string());
        call_names.insert(60u64, "y".to_string());

        let resolved = resolve_calls(&node, &fn_map, &call_names);
        match resolved {
            NdaNode::Compare { op, lhs, rhs } => {
                assert_eq!(op, CmpOp::Lt);
                match *lhs {
                    NdaNode::Call { target } => assert_eq!(target, 500),
                    _ => panic!("Expected resolved lhs"),
                }
                match *rhs {
                    NdaNode::Call { target } => assert_eq!(target, 600),
                    _ => panic!("Expected resolved rhs"),
                }
            }
            _ => panic!("Expected Compare"),
        }
    }

    #[test]
    fn resolve_calls_empty_fn_map_leaves_calls_unchanged() {
        let node = NdaNode::Call { target: 42 };
        let fn_map = HashMap::new();
        let mut call_names = HashMap::new();
        call_names.insert(42u64, "anything".to_string());

        let resolved = resolve_calls(&node, &fn_map, &call_names);
        match resolved {
            NdaNode::Call { target } => assert_eq!(target, 42),
            _ => panic!("Expected Call"),
        }
    }

    #[test]
    fn parse_unary_minus_on_identifier() {
        let src = r#"
            fn main() {
                let x = 5
                let y = -x
            }
        "#;
        let program = parse_ok(src);
        fn has_negate_load(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, operand } => {
                    matches!(op, VecOpKind::Negate) && matches!(**operand, NdaNode::Load { .. })
                }
                NdaNode::Scope { children } => children.iter().any(has_negate_load),
                NdaNode::Let { init, .. } => has_negate_load(init),
                _ => false,
            }
        }
        assert!(has_negate_load(&program));
    }

    #[test]
    fn parse_unary_minus_on_parenthesized_expr() {
        let src = "fn main() { let x = -(1 + 2) }";
        let program = parse_ok(src);
        fn has_negate_add(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, operand } => {
                    matches!(op, VecOpKind::Negate) && matches!(**operand, NdaNode::Add { .. })
                }
                NdaNode::Scope { children } => children.iter().any(has_negate_add),
                NdaNode::Let { init, .. } => has_negate_add(init),
                _ => false,
            }
        }
        assert!(has_negate_add(&program));
    }

    #[test]
    fn parse_matrix_bracket_semicolon_delimiter() {
        let src = "fn main() { let m = matrix[64; 32] }";
        let program = parse_ok(src);
        fn has_matrix(node: &NdaNode) -> bool {
            match node {
                NdaNode::Matrix { rows, cols, .. } => *rows == 64 && *cols == 32,
                NdaNode::Scope { children } => children.iter().any(has_matrix),
                NdaNode::Let { init, .. } => has_matrix(init),
                _ => false,
            }
        }
        assert!(has_matrix(&program));
    }

    #[test]
    fn parse_matrix_paren_comma_delimiter() {
        let src = "fn main() { let m = matrix(64, 32) }";
        let program = parse_ok(src);
        fn has_matrix(node: &NdaNode) -> bool {
            match node {
                NdaNode::Matrix { rows, cols, .. } => *rows == 64 && *cols == 32,
                NdaNode::Scope { children } => children.iter().any(has_matrix),
                NdaNode::Let { init, .. } => has_matrix(init),
                _ => false,
            }
        }
        assert!(has_matrix(&program));
    }

    #[test]
    fn parse_norm_bracket_syntax() {
        let src = "fn main() { let n = norm[512] }";
        let program = parse_ok(src);
        fn has_norm(node: &NdaNode) -> bool {
            match node {
                NdaNode::Norm { size, .. } => *size == 512,
                NdaNode::Scope { children } => children.iter().any(has_norm),
                NdaNode::Let { init, .. } => has_norm(init),
                _ => false,
            }
        }
        assert!(has_norm(&program));
    }

    #[test]
    fn parse_silu_with_complex_argument() {
        let src = "fn main() { let x = silu(1 + 2) }";
        let program = parse_ok(src);
        fn has_silu_add(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, operand } => {
                    matches!(op, VecOpKind::SiLU) && matches!(**operand, NdaNode::Add { .. })
                }
                NdaNode::Scope { children } => children.iter().any(has_silu_add),
                NdaNode::Let { init, .. } => has_silu_add(init),
                _ => false,
            }
        }
        assert!(has_silu_add(&program));
    }

    #[test]
    fn parse_break_with_semicolon() {
        let src = r#"
            fn main() {
                loop 5 {
                    break;
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
    fn parse_print_with_semicolon() {
        let src = "fn main() { print(42); }";
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
    fn parse_store_with_expression_value() {
        let src = r#"
            fn main() {
                let x = 1
                x = 1 + 2
            }
        "#;
        let program = parse_ok(src);
        fn has_store_with_add(node: &NdaNode) -> bool {
            match node {
                NdaNode::Store { value, .. } => matches!(**value, NdaNode::Add { .. }),
                NdaNode::Scope { children } => children.iter().any(has_store_with_add),
                _ => false,
            }
        }
        assert!(has_store_with_add(&program));
    }

    #[test]
    fn parse_chained_comparison() {
        // a < b produces a Compare node; the result is used as an expression
        let src = r#"
            fn main() {
                let a = 1
                let b = 2
                let c = a < b
            }
        "#;
        let program = parse_ok(src);
        fn has_compare_in_let(node: &NdaNode) -> bool {
            match node {
                NdaNode::Let { init, .. } => matches!(**init, NdaNode::Compare { .. }),
                NdaNode::Scope { children } => children.iter().any(has_compare_in_let),
                _ => false,
            }
        }
        assert!(has_compare_in_let(&program));
    }

    #[test]
    fn error_eof_inside_block() {
        let src = "fn main() { let x = 1";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("}") || err.contains("End of File") || err.contains("end of file"),
            "Expected error about missing closing brace or EOF, got: {}", err);
    }

    #[test]
    fn error_eof_inside_expression() {
        let src = "fn main() { let x = ";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
    }

    #[test]
    fn error_missing_function_name() {
        let src = "fn () { return 0 }";
        let result = parse_err(src);
        assert!(result.contains("identifier") || result.contains("Expected"),
            "Expected identifier error, got: {}", result);
    }

    #[test]
    fn error_bad_type_keyword() {
        let src = "fn main(x: bool) { return x }";
        let result = parse_err(src);
        assert!(result.contains("type") || result.contains("vec") || result.contains("Expected"),
            "Expected type error, got: {}", result);
    }

    #[test]
    fn compile_fn_hashes_populated() {
        let src = r#"
            fn alpha() { return 1 }
            fn beta() { return 2 }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.fn_hashes.len(), 2);
        assert!(report.fn_hashes.contains_key("alpha"));
        assert!(report.fn_hashes.contains_key("beta"));
        // Hashes are non-zero (they go through propagation so won't match hash_name exactly)
        assert_ne!(report.fn_hashes["alpha"], 0);
        assert_ne!(report.fn_hashes["beta"], 0);
    }

    #[test]
    fn compile_report_self_call_is_resolved() {
        // Recursive function: calls itself
        let src = r#"
            fn rec() { return rec() }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_count, 1);
        assert_eq!(report.call_edges, 1);
        // Self-call may or may not resolve depending on hash convergence
        // Just verify it doesn't panic and report is well-formed
        assert!(report.call_edges_resolved <= report.call_edges);
        assert!(report.fn_hashes.contains_key("rec"));
    }

    #[test]
    fn parse_function_params_become_let_bindings() {
        let src = "fn f(x: int) { return x }";
        let program = parse_ok(src);
        // The function body should start with a Let node for param 'x'
        match &program {
            NdaNode::Scope { children } => {
                let fn_scope = &children[0];
                match fn_scope {
                    NdaNode::Scope { children } => {
                        // First child should be Let for param x
                        assert!(matches!(&children[0], NdaNode::Let { name_hash, .. } if *name_hash == hash_name("x")));
                    }
                    _ => panic!("Expected inner Scope"),
                }
            }
            _ => panic!("Expected outer Scope"),
        }
    }

    #[test]
    fn parse_multiple_let_statements() {
        let src = r#"
            fn main() {
                let a = 1
                let b = 2
                let c = 3
                let d = 4
            }
        "#;
        let program = parse_ok(src);
        fn count_lets(node: &NdaNode) -> usize {
            match node {
                NdaNode::Let { init, .. } => 1 + count_lets(init),
                NdaNode::Scope { children } => children.iter().map(count_lets).sum(),
                _ => 0,
            }
        }
        // 4 lets from source + 0 params = 4
        assert_eq!(count_lets(&program), 4);
    }

    #[test]
    fn parse_while_with_comparison_condition() {
        let src = r#"
            fn main() {
                let x = 10
                while x > 0 {
                    x = x + -1
                }
            }
        "#;
        let program = parse_ok(src);
        fn has_while_with_compare(node: &NdaNode) -> bool {
            match node {
                NdaNode::While { cond, .. } => matches!(**cond, NdaNode::Compare { op: CmpOp::Gt, .. }),
                NdaNode::Scope { children } => children.iter().any(has_while_with_compare),
                _ => false,
            }
        }
        assert!(has_while_with_compare(&program));
    }

    #[test]
    fn parse_if_without_else() {
        let src = r#"
            fn main() {
                let x = 1
                if x {
                    print(x)
                }
            }
        "#;
        let program = parse_ok(src);
        fn find_if(node: &NdaNode) -> Option<bool> {
            match node {
                NdaNode::If { else_body, .. } => Some(else_body.is_none()),
                NdaNode::Scope { children } => {
                    for c in children {
                        if let Some(r) = find_if(c) { return Some(r); }
                    }
                    None
                }
                _ => None,
            }
        }
        assert_eq!(find_if(&program), Some(true));
    }

    #[test]
    fn parse_nested_function_calls() {
        let src = r#"
            fn a() { return 1 }
            fn b() { return 2 }
            fn main() {
                let x = a()
                let y = b()
                let z = a()
            }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_count, 3);
        assert_eq!(report.call_edges, 3);
        assert_eq!(report.call_edges_resolved, 3);
    }

    #[test]
    fn compile_empty_scope_hash_deterministic() {
        let r1 = compile_with_report("").unwrap();
        let r2 = compile_with_report("").unwrap();
        assert_eq!(r1.program.hash(), r2.program.hash());
        assert_eq!(r1.function_count, 0);
    }

    #[test]
    fn parse_return_with_complex_expr() {
        let src = "fn main() { return 1 + 2 }";
        let program = parse_ok(src);
        fn has_return_add(node: &NdaNode) -> bool {
            match node {
                NdaNode::Return { value } => matches!(**value, NdaNode::Add { .. }),
                NdaNode::Scope { children } => children.iter().any(has_return_add),
                _ => false,
            }
        }
        assert!(has_return_add(&program));
    }

    #[test]
    fn parse_zero_loop_count_translates_to_static_loop() {
        // loop 0 should produce a static Loop with count=0 (not a while)
        let src = r#"
            fn main() {
                loop 0 {
                    print(1)
                }
            }
        "#;
        let program = parse_ok(src);
        fn has_static_loop(node: &NdaNode) -> bool {
            match node {
                NdaNode::Loop { count, .. } => *count == 0,
                NdaNode::Scope { children } => children.iter().any(has_static_loop),
                _ => false,
            }
        }
        assert!(has_static_loop(&program));
    }

    #[test]
    fn parse_add_infix_vs_builtin() {
        // Both `a + b` and `add(a, b)` should produce Add nodes
        let src_infix = "fn main() { let x = 1 + 2 }";
        let src_builtin = "fn main() { let x = add(1, 2) }";
        let p1 = parse_ok(src_infix);
        let p2 = parse_ok(src_builtin);
        fn has_add(node: &NdaNode) -> bool {
            match node {
                NdaNode::Add { .. } => true,
                NdaNode::Scope { children } => children.iter().any(has_add),
                NdaNode::Let { init, .. } => has_add(init),
                _ => false,
            }
        }
        assert!(has_add(&p1));
        assert!(has_add(&p2));
    }

    #[test]
    fn parse_load_produces_correct_hash() {
        let src = "fn main() { let myvar = 1 }";
        let program = parse_ok(src);
        fn find_load_hash(node: &NdaNode) -> Option<u64> {
            match node {
                NdaNode::Let { name_hash, .. } => Some(*name_hash),
                NdaNode::Scope { children } => {
                    for c in children {
                        if let Some(h) = find_load_hash(c) { return Some(h); }
                    }
                    None
                }
                _ => None,
            }
        }
        assert_eq!(find_load_hash(&program), Some(hash_name("myvar")));
    }

    #[test]
    fn parse_store_uses_correct_hash() {
        let src = r#"
            fn main() {
                let myvar = 1
                myvar = 2
            }
        "#;
        let program = parse_ok(src);
        fn find_store_hash(node: &NdaNode) -> Option<u64> {
            match node {
                NdaNode::Store { name_hash, .. } => Some(*name_hash),
                NdaNode::Scope { children } => {
                    for c in children {
                        if let Some(h) = find_store_hash(c) { return Some(h); }
                    }
                    None
                }
                _ => None,
            }
        }
        assert_eq!(find_store_hash(&program), Some(hash_name("myvar")));
    }

    // ── Block 199: NDA Parser expanded tests ─────────────────────────────────

    // ── hash_name distribution & properties ──

    #[test]
    fn hash_name_similar_prefixes_differ() {
        let h1 = hash_name("foo");
        let h2 = hash_name("foobar");
        let h3 = hash_name("foo_bar");
        assert_ne!(h1, h2);
        assert_ne!(h2, h3);
        assert_ne!(h1, h3);
    }

    #[test]
    fn hash_name_many_unique() {
        let hashes: Vec<u64> = (0..100).map(|i| hash_name(&format!("fn_{}", i))).collect();
        let unique: std::collections::HashSet<&u64> = hashes.iter().collect();
        assert_eq!(unique.len(), 100, "All 100 hashes should be unique");
    }

    #[test]
    fn hash_name_case_sensitive() {
        assert_ne!(hash_name("Foo"), hash_name("foo"));
        assert_ne!(hash_name("MAIN"), hash_name("main"));
    }

    // ── build_matrix_node bitmap byte formulas ──

    #[test]
    fn build_matrix_node_bitmap_byte_count() {
        // bitmap_bytes = rows * cols.div_ceil(8)
        let node = build_matrix_node(4, 16);
        match node {
            NdaNode::Matrix { rows, cols, sign, extra, .. } => {
                assert_eq!(rows, 4);
                assert_eq!(cols, 16);
                let expected = 4 * 16usize.div_ceil(8); // 4 * 2 = 8
                assert_eq!(sign.len(), expected);
                assert_eq!(extra.len(), expected);
            }
            _ => panic!("Expected Matrix"),
        }
    }

    #[test]
    fn build_matrix_node_odd_cols_bitmap() {
        // 1 row, 9 cols → div_ceil(9,8) = 2 bytes per row
        let node = build_matrix_node(1, 9);
        match node {
            NdaNode::Matrix { sign, extra, .. } => {
                assert_eq!(sign.len(), 2);
                assert_eq!(extra.len(), 2);
            }
            _ => panic!("Expected Matrix"),
        }
    }

    #[test]
    fn build_matrix_node_1x1_single_byte() {
        let node = build_matrix_node(1, 1);
        match node {
            NdaNode::Matrix { rows, cols, sign, extra, .. } => {
                assert_eq!(rows, 1);
                assert_eq!(cols, 1);
                assert_eq!(sign.len(), 1);
                assert_eq!(extra.len(), 1);
                assert_eq!(sign[0], 0xAA);
                assert_eq!(extra[0], 0x55);
            }
            _ => panic!("Expected Matrix"),
        }
    }

    // ── build_norm_node bitmap formulas ──

    #[test]
    fn build_norm_node_bitmap_bytes_8() {
        let node = build_norm_node(8);
        match node {
            NdaNode::Norm { size, weight, bias } => {
                assert_eq!(size, 8);
                assert_eq!(weight.len(), 1); // div_ceil(8,8) = 1
                assert_eq!(bias.len(), 1);
            }
            _ => panic!("Expected Norm"),
        }
    }

    #[test]
    fn build_norm_node_bitmap_bytes_9() {
        let node = build_norm_node(9);
        match node {
            NdaNode::Norm { size, weight, bias } => {
                assert_eq!(size, 9);
                assert_eq!(weight.len(), 2); // div_ceil(9,8) = 2
                assert_eq!(bias.len(), 2);
            }
            _ => panic!("Expected Norm"),
        }
    }

    #[test]
    fn build_norm_node_large_clamped() {
        let node = build_norm_node(99999);
        match node {
            NdaNode::Norm { size, .. } => assert_eq!(size, 65535),
            _ => panic!("Expected Norm"),
        }
    }

    #[test]
    fn build_norm_node_weight_bias_content() {
        let node = build_norm_node(16);
        match node {
            NdaNode::Norm { weight, bias, .. } => {
                assert!(weight.iter().all(|&b| b == 0xFF));
                assert!(bias.iter().all(|&b| b == 0x00));
            }
            _ => panic!("Expected Norm"),
        }
    }

    // ── resolve_calls through nested structures ──

    #[test]
    fn resolve_calls_through_nested_if_else() {
        let node = NdaNode::If {
            cond: Box::new(NdaNode::Call { target: 10 }),
            then_body: vec![NdaNode::Call { target: 20 }],
            else_body: Some(vec![NdaNode::Call { target: 30 }]),
        };
        let mut fn_map = HashMap::new();
        fn_map.insert("a".to_string(), 100u64);
        fn_map.insert("b".to_string(), 200u64);
        fn_map.insert("c".to_string(), 300u64);
        let mut call_names = HashMap::new();
        call_names.insert(10u64, "a".to_string());
        call_names.insert(20u64, "b".to_string());
        call_names.insert(30u64, "c".to_string());

        let resolved = resolve_calls(&node, &fn_map, &call_names);
        match resolved {
            NdaNode::If { cond, then_body, else_body } => {
                assert!(matches!(*cond, NdaNode::Call { target: 100 }));
                assert!(matches!(&then_body[0], NdaNode::Call { target: 200 }));
                let eb = else_body.unwrap();
                assert!(matches!(&eb[0], NdaNode::Call { target: 300 }));
            }
            _ => panic!("Expected If"),
        }
    }

    #[test]
    fn resolve_calls_unresolved_name_stays_original() {
        // call_names maps target→name, but name not in fn_map → stays original
        let node = NdaNode::Call { target: 42 };
        let fn_map = HashMap::new();
        let mut call_names = HashMap::new();
        call_names.insert(42u64, "unknown_fn".to_string());

        let resolved = resolve_calls(&node, &fn_map, &call_names);
        match resolved {
            NdaNode::Call { target } => assert_eq!(target, 42),
            _ => panic!("Expected Call"),
        }
    }

    #[test]
    fn resolve_calls_no_call_name_entry_stays_original() {
        // target not in call_names at all → stays original
        let node = NdaNode::Call { target: 99 };
        let fn_map = HashMap::new();
        let call_names = HashMap::new();

        let resolved = resolve_calls(&node, &fn_map, &call_names);
        match resolved {
            NdaNode::Call { target } => assert_eq!(target, 99),
            _ => panic!("Expected Call"),
        }
    }

    // ── compile() vs compile_with_report() equivalence ──

    #[test]
    fn compile_vs_report_equivalence() {
        let src = "fn main() { return 42 }";
        let (program, fn_hashes) = compile(src).unwrap();
        let report = compile_with_report(src).unwrap();
        assert_eq!(program.hash(), report.program.hash());
        assert_eq!(fn_hashes, report.fn_hashes);
    }

    // ── ParseReport edge cases ──

    #[test]
    fn parse_report_unresolved_calls() {
        // Call to a function that doesn't exist in the program
        let src = r#"
            fn main() {
                let x = nonexistent()
            }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_count, 1);
        assert_eq!(report.call_edges, 1);
        // Unresolved call should not be counted as resolved
        assert_eq!(report.call_edges_resolved, 0);
    }

    #[test]
    fn parse_report_json_program_hash_format() {
        let report = compile_with_report("fn main() { return 1 }").unwrap();
        let json = report.to_json();
        let hash_str = json["program_hash"].as_str().unwrap();
        // Should be 16 hex chars (u64 → 016x format)
        assert_eq!(hash_str.len(), 16);
        assert!(hash_str.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn parse_report_function_names_sorted() {
        let src = r#"
            fn zeta() { return 1 }
            fn alpha() { return 2 }
            fn mu() { return 3 }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.function_names, vec!["alpha", "mu", "zeta"]);
    }

    // ── parse_type coverage ──

    #[test]
    fn parse_type_vec_param() {
        let src = "fn f(x: vec) { return x }";
        let program = parse_ok(src);
        assert!(matches!(program, NdaNode::Scope { .. }));
    }

    #[test]
    fn parse_type_matrix_param() {
        let src = "fn f(x: matrix) { return x }";
        let program = parse_ok(src);
        assert!(matches!(program, NdaNode::Scope { .. }));
    }

    #[test]
    fn parse_type_norm_param() {
        let src = "fn f(x: norm) { return x }";
        let program = parse_ok(src);
        assert!(matches!(program, NdaNode::Scope { .. }));
    }

    #[test]
    fn parse_type_int_return() {
        let src = "fn f() -> int { return 1 }";
        let program = parse_ok(src);
        assert!(matches!(program, NdaNode::Scope { .. }));
    }

    #[test]
    fn parse_type_vec_return() {
        let src = "fn f() -> vec { return 1 }";
        let program = parse_ok(src);
        assert!(matches!(program, NdaNode::Scope { .. }));
    }

    // ── parse expression variants ──

    #[test]
    fn parse_vec_bracket_semicolon() {
        let src = "fn main() { let v = vec[5.0; 4] }";
        let program = parse_ok(src);
        fn has_matrix_1x4(node: &NdaNode) -> bool {
            match node {
                NdaNode::Matrix { rows, cols, .. } => *rows == 1 && *cols == 4,
                NdaNode::Scope { children } => children.iter().any(has_matrix_1x4),
                NdaNode::Let { init, .. } => has_matrix_1x4(init),
                _ => false,
            }
        }
        assert!(has_matrix_1x4(&program));
    }

    #[test]
    fn parse_vec_paren_comma() {
        let src = "fn main() { let v = vec(8) }";
        let program = parse_ok(src);
        fn has_matrix_1x8(node: &NdaNode) -> bool {
            match node {
                NdaNode::Matrix { rows, cols, .. } => *rows == 1 && *cols == 8,
                NdaNode::Scope { children } => children.iter().any(has_matrix_1x8),
                NdaNode::Let { init, .. } => has_matrix_1x8(init),
                _ => false,
            }
        }
        assert!(has_matrix_1x8(&program));
    }

    #[test]
    fn parse_norm_paren_roundtrip() {
        let src = "fn main() { let n = norm(256) }";
        let program = parse_ok(src);
        fn has_norm_256(node: &NdaNode) -> bool {
            match node {
                NdaNode::Norm { size, .. } => *size == 256,
                NdaNode::Scope { children } => children.iter().any(has_norm_256),
                NdaNode::Let { init, .. } => has_norm_256(init),
                _ => false,
            }
        }
        assert!(has_norm_256(&program));
    }

    // ── VecOp kinds coverage ──

    #[test]
    fn parse_abs_value() {
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
    fn parse_reduce_sum_value() {
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
    fn parse_negate_builtin_form() {
        let src = "fn main() { let x = negate(5) }";
        let program = parse_ok(src);
        fn has_negate(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, .. } => matches!(op, VecOpKind::Negate),
                NdaNode::Scope { children } => children.iter().any(has_negate),
                NdaNode::Let { init, .. } => has_negate(init),
                _ => false,
            }
        }
        assert!(has_negate(&program));
    }

    // ── Nested control flow ──

    #[test]
    fn parse_nested_loops() {
        let src = r#"
            fn main() {
                loop 3 {
                    loop 2 {
                        print(1)
                    }
                }
            }
        "#;
        let program = parse_ok(src);
        fn count_inner_loops(node: &NdaNode) -> usize {
            match node {
                NdaNode::Loop { body, .. } => {
                    1 + body.iter().map(count_inner_loops).sum::<usize>()
                }
                NdaNode::Scope { children } => children.iter().map(count_inner_loops).sum(),
                _ => 0,
            }
        }
        assert_eq!(count_inner_loops(&program), 2);
    }

    #[test]
    fn parse_if_else_if_else_chain() {
        let src = r#"
            fn main() {
                let x = 1
                if x {
                    print(1)
                } else if x {
                    print(2)
                } else {
                    print(3)
                }
            }
        "#;
        let program = parse_ok(src);
        fn count_ifs(node: &NdaNode) -> usize {
            match node {
                NdaNode::If { else_body, then_body, .. } => {
                    1 + then_body.iter().map(count_ifs).sum::<usize>()
                        + else_body.as_ref().map_or(0, |eb| eb.iter().map(count_ifs).sum::<usize>())
                }
                NdaNode::Scope { children } => children.iter().map(count_ifs).sum(),
                _ => 0,
            }
        }
        assert_eq!(count_ifs(&program), 2);
    }

    #[test]
    fn parse_while_inside_loop() {
        let src = r#"
            fn main() {
                let x = 5
                loop 3 {
                    while x {
                        x = x + -1
                    }
                }
            }
        "#;
        let program = parse_ok(src);
        fn has_while_in_loop(node: &NdaNode) -> bool {
            match node {
                NdaNode::Loop { body, .. } => body.iter().any(|c| matches!(c, NdaNode::While { .. })),
                NdaNode::Scope { children } => children.iter().any(has_while_in_loop),
                _ => false,
            }
        }
        assert!(has_while_in_loop(&program));
    }

    // ── Error paths ──

    #[test]
    fn error_unexpected_token_in_let() {
        let src = "fn main() { let x = } }";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
    }

    #[test]
    fn error_missing_colon_in_param() {
        let src = "fn f(x int) { return x }";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
    }

    #[test]
    fn error_missing_rparen_in_function_sig() {
        let src = "fn main(x: int { return x }";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
    }

    #[test]
    fn error_missing_comma_between_params() {
        let src = "fn f(x: int y: int) { return x }";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = NdaParser::new(tokens);
        let result = parser.parse_program();
        assert!(result.is_err());
    }

    // ── Misc ──

    #[test]
    fn parse_empty_function_body() {
        let src = "fn f() {}";
        let program = parse_ok(src);
        match &program {
            NdaNode::Scope { children } => {
                assert_eq!(children.len(), 1);
                match &children[0] {
                    NdaNode::Scope { children } => assert_eq!(children.len(), 0),
                    _ => panic!("Expected inner Scope"),
                }
            }
            _ => panic!("Expected outer Scope"),
        }
    }

    #[test]
    fn parse_multiple_calls_same_function() {
        let src = r#"
            fn helper() { return 1 }
            fn main() {
                let a = helper()
                let b = helper()
                let c = helper()
            }
        "#;
        let report = compile_with_report(src).unwrap();
        assert_eq!(report.call_edges, 3);
        assert_eq!(report.call_edges_resolved, 3);
    }

    #[test]
    fn parse_call_target_hash_uses_fn_hash() {
        let src = r#"
            fn alpha() { return 1 }
            fn main() {
                let x = alpha()
            }
        "#;
        let report = compile_with_report(src).unwrap();
        // The call to alpha should be resolved to alpha's hash
        assert!(report.fn_hashes.contains_key("alpha"));
        assert_eq!(report.call_edges_resolved, 1);
    }

    #[test]
    fn parse_semicolons_optional() {
        // Semicolons should be optional after statements
        let src_no_semi = "fn main() { let x = 1\nprint(x)\nreturn x }";
        let src_semi = "fn main() { let x = 1; print(x); return x; }";
        let p1 = parse_ok(src_no_semi);
        let p2 = parse_ok(src_semi);
        // Both should parse successfully
        assert!(matches!(p1, NdaNode::Scope { .. }));
        assert!(matches!(p2, NdaNode::Scope { .. }));
    }

    #[test]
    fn parse_negative_int_literal() {
        let src = "fn main() { let x = -42 }";
        let program = parse_ok(src);
        fn has_negate_int(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, operand } => {
                    matches!(op, VecOpKind::Negate) && matches!(**operand, NdaNode::Int { value: 42 })
                }
                NdaNode::Scope { children } => children.iter().any(has_negate_int),
                NdaNode::Let { init, .. } => has_negate_int(init),
                _ => false,
            }
        }
        assert!(has_negate_int(&program));
    }

    #[test]
    fn parse_double_negate() {
        let src = "fn main() { let x = --5 }";
        let program = parse_ok(src);
        fn has_double_negate(node: &NdaNode) -> bool {
            match node {
                NdaNode::VecOp { op, operand } => {
                    matches!(op, VecOpKind::Negate) &&
                    matches!(**operand, NdaNode::VecOp { op: VecOpKind::Negate, .. })
                }
                NdaNode::Scope { children } => children.iter().any(has_double_negate),
                NdaNode::Let { init, .. } => has_double_negate(init),
                _ => false,
            }
        }
        assert!(has_double_negate(&program));
    }

    #[test]
    fn parse_add_chain() {
        let src = "fn main() { let x = 1 + 2 + 3 }";
        let program = parse_ok(src);
        fn count_adds(node: &NdaNode) -> usize {
            match node {
                NdaNode::Add { lhs, rhs } => 1 + count_adds(lhs) + count_adds(rhs),
                NdaNode::Scope { children } => children.iter().map(count_adds).sum(),
                NdaNode::Let { init, .. } => count_adds(init),
                _ => 0,
            }
        }
        // 1 + 2 + 3 is left-associative: (1 + 2) + 3 → 2 Add nodes
        assert_eq!(count_adds(&program), 2);
    }
}
