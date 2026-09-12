// compiler/nda_parser — Parser for the NDA programming language
//
// Converts a stream of Located tokens into a compiled NdaNode AST.
// Resolves function call targets dynamically via Merkle hash propagation.
#![allow(dead_code)]

mod parser;
#[cfg(test)]
mod tests;

use crate::compiler::nda_lexer::{Located, NdaLexer, Token};
use crate::site_map::verifier::NdaNode;
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
                loc.line,
                loc.col,
                loc.token.display_name()
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
