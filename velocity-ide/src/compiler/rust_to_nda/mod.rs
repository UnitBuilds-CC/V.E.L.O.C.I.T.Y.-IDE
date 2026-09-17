// compiler/rust_to_nda — Full Rust source → NDA program tree
//
// Philosophy: teach from complete programs, not fragments.
//
// A fragment-trained model learns vocabulary.
// A full-program-trained model learns grammar, causality, and intent.
//
// This compiler walks the complete Rust AST produced by syn::parse_file.
// Every structural element of the source becomes a structural element of the
// NDA tree:
//
//   Rust source concept         NDA representation
//   ─────────────────────────   ───────────────────────────────────────
//   fn foo() { ... }        →   NdaNode::Scope (children = body nodes)
//   foo(args)               →   NdaNode::Call  (target = hash of foo's Scope)
//   let x: [[f32; N]; M]]  →   NdaNode::Matrix (rows=M, cols=N)
//   let x: f32/i32/usize   →   NdaNode::Int    (value = literal if known)
//   { stmt; stmt; ... }     →   NdaNode::Scope  (children = stmts)
//   impl Struct { ... }     →   NdaNode::Scope  (one child Scope per method)
//
// The call graph is preserved: when fn A calls fn B, A's Scope contains a
// Call node whose target hash equals B's Scope hash.  The SiteMap stores each
// function's Scope individually so Call nodes can resolve transitively.
//
// This means the model sees complete programs including:
//   • Which functions call which (causality)
//   • How data flows through a pipeline (ordering)
//   • The nesting depth and hierarchy of a real algorithm
//   • The relationships between an encoder and its decoder
//
// — NOT just isolated matrix multiply patterns.

mod compiler;
#[cfg(test)]
mod tests;

use std::{collections::HashMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;
use syn::{
    visit::Visit, Expr, ExprCall, ExprMethodCall, File, ImplItem, Item, ItemFn, ItemImpl, Lit, Pat,
    Stmt, Type,
};

use crate::site_map::{
    verifier::{MerkleVerifier, NdaNode},
    SiteMap,
};

// ─── Compiled function ────────────────────────────────────────────────────────

/// One compiled function: its NDA Scope node and metadata.
#[derive(Clone, Debug)]
pub struct CompiledFn {
    /// The fully-qualified function name (e.g. "TcpEncoder::encode").
    pub name: String,
    /// The NDA Scope representing this function's full body.
    pub node: NdaNode,
    /// Hash of the Scope node (for Call node targets).
    pub hash: u64,
    /// Names of all functions this function calls (resolved to hashes after
    /// the full compilation pass).
    pub callees: Vec<String>,
}

// ─── RustToNda compiler ───────────────────────────────────────────────────────

/// Compiles a complete Rust source file into a set of NDA Scope nodes.
///
/// Usage:
/// ```rust,ignore
/// let source = std::fs::read_to_string("src/tcp_encoder.rs")?;
/// let mut compiler = RustToNda::new();
/// let program = compiler.compile_source(&source)?;
/// compiler.store_all(&mut site_map)?;
/// ```
pub struct RustToNda {
    /// All compiled functions keyed by qualified name.
    functions: HashMap<String, CompiledFn>,
    /// Current impl block type name (for method qualification).
    current_impl: Option<String>,
    /// Accumulated compilation diagnostics.
    diagnostics: CompileDiagnostics,
}

impl Default for RustToNda {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Pass 2: patch Call node targets ──────────────────────────────────────────

/// Recursively replace Call { target: 0 } placeholders with real hashes.
fn patch_calls(node: &NdaNode, fn_hashes: &HashMap<String, u64>) -> NdaNode {
    match node {
        NdaNode::Scope { children } => NdaNode::Scope {
            children: children.iter().map(|c| patch_calls(c, fn_hashes)).collect(),
        },
        // Call with target=0 is a placeholder: we can't know the exact function
        // without a name-resolver, so we hash-combine all known function hashes
        // as a stable fingerprint of "some call to a known function".
        NdaNode::Call { target: 0 } => {
            let combined: u64 = fn_hashes.values().fold(0u64, |acc, &h| acc ^ h);
            NdaNode::Call { target: combined }
        }
        other => other.clone(),
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Extract a type annotation from a `let` binding pattern.
fn extract_let_type(pat: &Pat) -> Option<Type> {
    if let Pat::Type(pt) = pat {
        return Some(*pt.ty.clone());
    }
    None
}

/// Attempt to read 2D array dimensions from a type like `[[f32; 64]; 32]`.
fn matrix_dims_from_type(ty: &Type) -> Option<(usize, usize)> {
    if let Type::Array(outer) = ty {
        if let Expr::Lit(l) = &outer.len {
            if let Lit::Int(rows_lit) = &l.lit {
                let rows = rows_lit.base10_parse::<usize>().ok()?;
                if let Type::Array(inner) = outer.elem.as_ref() {
                    if let Expr::Lit(l2) = &inner.len {
                        if let Lit::Int(cols_lit) = &l2.lit {
                            let cols = cols_lit.base10_parse::<usize>().ok()?;
                            return Some((rows, cols));
                        }
                    }
                }
            }
        }
    }
    None
}

/// Build an NdaNode::Matrix with synthetic (uniform) weights.
///
/// Real weights come from training or from the SiteMap. During source
/// compilation we produce structurally correct nodes with uniform bitmaps;
/// the model learns the *structure and connectivity*, not specific weights.
fn build_matrix_node(rows: usize, cols: usize) -> NdaNode {
    // Clamp dimensions to u16 range (65535 max) and require non-zero.
    let rows = rows.clamp(1, 65535) as u16;
    let cols = cols.clamp(1, 65535) as u16;
    let bitmap_bytes = rows as usize * (cols as usize).div_ceil(8);
    // Alternating 0xAA / 0x55 gives a balanced {+2,+1,-1,-2} distribution.
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

/// Extract a dotted name string from an expression (e.g. `foo::bar` → "foo::bar").
fn expr_to_name(expr: &Expr) -> String {
    match expr {
        Expr::Path(p) => p
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
        _ => String::new(),
    }
}

/// Extract a human-readable name from a type (for impl block qualification).
fn type_name_of(ty: &Type) -> String {
    match ty {
        Type::Path(p) => p
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
        _ => "Unknown".to_string(),
    }
}

// ─── Fallback visitor ─────────────────────────────────────────────────────────

/// A simple syn visitor that collects any Int/Array nodes — and any call edges —
/// from sub-expressions that the main match arm doesn't explicitly handle.
struct ExprCollector<'a> {
    nodes: Vec<NdaNode>,
    /// Call targets found inside sub-expressions. Feeds `CompiledFn::callees`,
    /// and therefore `RustToNda::call_graph`.
    callees: &'a mut Vec<String>,
}

impl<'a> Visit<'_> for ExprCollector<'a> {
    fn visit_expr_lit(&mut self, lit: &syn::ExprLit) {
        match &lit.lit {
            Lit::Int(i) => {
                if let Ok(v) = i.base10_parse::<i32>() {
                    self.nodes.push(NdaNode::Int { value: v });
                }
            }
            Lit::Float(f) => {
                if let Ok(v) = f.base10_parse::<f32>() {
                    self.nodes.push(NdaNode::Int { value: v as i32 });
                }
            }
            _ => {}
        }
    }

    fn visit_expr_call(&mut self, call: &ExprCall) {
        // The main match arm only records the calls it handles directly; a call
        // nested inside a cast, index, tuple, match arm, … lands here instead.
        // Record the edge so `call_graph` still sees it, then keep descending.
        let name = expr_to_name(&call.func);
        if !name.is_empty() {
            self.callees.push(name);
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &ExprMethodCall) {
        self.callees.push(call.method.to_string());
        syn::visit::visit_expr_method_call(self, call);
    }
}

/// Recursively find all `.rs` files under `dir`.
fn walkdir_rs_files(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        anyhow::bail!("Not a directory: {}", dir.display());
    }
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // Skip target/, .git/, node_modules/
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name == "target" || name == ".git" || name == "node_modules" {
                    continue;
                }
                walk(&path, out)?;
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                out.push(path);
            }
        }
        Ok(())
    }
    walk(dir, &mut files)?;
    Ok(files)
}

// ─── CLI entry point ──────────────────────────────────────────────────────────

/// Compile a Rust source file and populate the SiteMap.
///
/// Called from main.rs via `velocity_ide seed --source <path>`.
pub fn seed_from_source(source_path: &Path, site_map: &mut SiteMap) -> Result<SeedReport> {
    let t0 = std::time::Instant::now();

    let mut compiler = RustToNda::new();
    let root = compiler.compile_file(source_path)?;

    // Verify the root is Merkle-consistent before storing.
    let mut verifier = MerkleVerifier::new();
    if let NdaNode::Scope { ref children } = root {
        for child in children {
            verifier.push_leaf(child);
        }
    }
    let root_hash = root.hash();
    verifier.record_root(root_hash);

    let n_stored = compiler.store_all(site_map, &root)?;

    // Register file path and function names as strings, then store triples
    // so the wiki can classify entities as files vs symbols.
    // Strip the Windows extended-length path prefix (\\?\) from canonicalized
    // paths so the dictionary stores clean, portable path strings.
    let file_path_str = source_path.display().to_string();
    let file_path_str = file_path_str
        .strip_prefix("\\\\?\\")
        .unwrap_or(&file_path_str);
    let file_hash = site_map.register_string(file_path_str)?;
    let fn_names: Vec<String> = compiler.functions.keys().cloned().collect();
    for name in &fn_names {
        site_map.register_string(name)?;
    }
    let mut triples = Vec::new();
    // File defines each function (predicate 1 = Defines).
    for name in &fn_names {
        let fn_hash = site_map.hash_string(name);
        triples.push(crate::site_map::VcTriple {
            subject_hash: file_hash,
            predicate_id: 1,
            object_hash: fn_hash,
        });
    }
    // Caller calls callee (predicate 2 = Calls).
    for cf in compiler.functions.values() {
        let caller_hash = site_map.hash_string(&cf.name);
        for callee in &cf.callees {
            if compiler.functions.contains_key(callee) {
                let callee_hash = site_map.hash_string(callee);
                triples.push(crate::site_map::VcTriple {
                    subject_hash: caller_hash,
                    predicate_id: 2,
                    object_hash: callee_hash,
                });
            }
        }
    }
    if !triples.is_empty() {
        site_map.put_file_snapshot(file_path_str, &triples)?;
    }

    // Build resolved call graph and count resolved edges.
    let call_graph = compiler.call_graph();
    let total_edges: usize = compiler.functions.values().map(|cf| cf.callees.len()).sum();
    let resolved_edges: usize = call_graph.values().map(|v| v.len()).sum();
    let mut diagnostics = compiler.diagnostics().clone();
    diagnostics.call_edges = total_edges;
    diagnostics.call_edges_resolved = resolved_edges;

    Ok(SeedReport {
        source_path: source_path.to_path_buf(),
        functions: compiler.function_count(),
        nodes_stored: n_stored,
        root_hash,
        elapsed_ms: t0.elapsed().as_millis(),
        call_graph,
        diagnostics,
    })
}

/// Summary returned by `seed_from_source`.
#[derive(Debug, Serialize)]
pub struct SeedReport {
    pub source_path: std::path::PathBuf,
    pub functions: usize,
    pub nodes_stored: usize,
    pub root_hash: u64,
    pub elapsed_ms: u128,
    /// Resolved call graph: caller → callees.
    pub call_graph: HashMap<String, Vec<String>>,
    /// Compilation diagnostics.
    pub diagnostics: CompileDiagnostics,
}

/// Diagnostics from a compilation pass — tracks what was handled and what wasn't.
#[derive(Debug, Default, Clone, Serialize)]
pub struct CompileDiagnostics {
    /// Total expression nodes visited.
    pub expressions_visited: usize,
    /// Expressions that produced NDA nodes.
    pub expressions_compiled: usize,
    /// Expressions that were silently dropped (no NDA node produced).
    pub expressions_dropped: usize,
    /// Number of distinct expression types encountered (e.g. Call, If, Binary).
    pub expr_type_coverage: HashMap<String, usize>,
    /// Top-level items by kind (Fn, Impl, Struct, Enum, etc.).
    pub items_by_kind: HashMap<String, usize>,
    /// Total call edges (before resolution).
    pub call_edges: usize,
    /// Call edges resolved to known function hashes.
    pub call_edges_resolved: usize,
    /// Warnings for unsupported constructs.
    pub warnings: Vec<String>,
}

impl std::fmt::Display for SeedReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Seeded '{}': {} functions → {} NDA nodes stored | root={:016x} | {}ms",
            self.source_path.display(),
            self.functions,
            self.nodes_stored,
            self.root_hash,
            self.elapsed_ms,
        )
    }
}
