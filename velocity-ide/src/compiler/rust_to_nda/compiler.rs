// RustToNda compiler implementation.

use super::*;
use crate::site_map::verifier::NdaNode;
use crate::site_map::SiteMap;

impl RustToNda {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            current_impl: None,
            diagnostics: CompileDiagnostics::default(),
        }
    }

    /// Compile a complete Rust source string.
    ///
    /// Returns the top-level NDA program node (a Scope whose children are
    /// one Scope per top-level function / impl block).
    pub fn compile_source(&mut self, source: &str) -> Result<NdaNode> {
        let file: File = syn::parse_str(source).context("Failed to parse Rust source")?;

        // Pass 1: compile every function/impl into NDA Scope nodes.
        for item in &file.items {
            self.compile_item(item);
        }

        // Pass 2: resolve Call node targets (fill in hashes from function names).
        let fn_hashes: HashMap<String, u64> = self
            .functions
            .iter()
            .map(|(name, cf)| (name.clone(), cf.hash))
            .collect();

        // Re-walk all compiled nodes and patch Call nodes whose target = 0
        // (placeholder set during pass 1 before all hashes were known).
        for cf in self.functions.values_mut() {
            cf.node = patch_calls(&cf.node, &fn_hashes);
            cf.hash = cf.node.hash();
        }

        // Build top-level Scope: one child Scope per compiled function.
        let mut sorted_fns: Vec<&CompiledFn> = self.functions.values().collect();
        sorted_fns.sort_by_key(|cf| &cf.name);

        let children: Vec<NdaNode> = sorted_fns.into_iter().map(|cf| cf.node.clone()).collect();

        Ok(NdaNode::Scope { children })
    }

    /// Compile a Rust source file on disk.
    pub fn compile_file(&mut self, path: &Path) -> Result<NdaNode> {
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("Reading source file: {path:?}"))?;
        self.compile_source(&source)
    }

    /// Store all compiled functions individually in the SiteMap, then store
    /// the top-level program node.
    ///
    /// Each function is stored separately so Call nodes can resolve them.
    /// The top-level program is stored as the root program.
    pub fn store_all(&self, site_map: &mut SiteMap, root: &NdaNode) -> Result<usize> {
        let mut count = 0;

        // Store individual functions first (so Call targets resolve).
        for cf in self.functions.values() {
            site_map
                .put_program(&cf.node)
                .with_context(|| format!("Storing function '{}'", cf.name))?;
            count += 1;
        }

        // Store the full program root.
        site_map.put_program(root).context("Storing root program")?;
        count += 1;

        site_map.flush().context("Flushing SiteMap")?;
        Ok(count)
    }

    /// How many functions were compiled.
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    /// Return all compiled function names.
    pub fn function_names(&self) -> Vec<&str> {
        self.functions.keys().map(|s| s.as_str()).collect()
    }

    /// Return the resolved call graph: caller name → [callee names].
    ///
    /// Callees are filtered to only include functions that were actually compiled
    /// (i.e. exist in the function map). Unresolved callees are dropped.
    pub fn call_graph(&self) -> HashMap<String, Vec<String>> {
        let known: std::collections::HashSet<&str> =
            self.functions.keys().map(|s| s.as_str()).collect();
        self.functions
            .iter()
            .map(|(name, cf)| {
                let resolved: Vec<String> = cf
                    .callees
                    .iter()
                    .filter(|c| known.contains(c.as_str()))
                    .cloned()
                    .collect();
                (name.clone(), resolved)
            })
            .collect()
    }

    /// Return a reference to the accumulated compilation diagnostics.
    pub fn diagnostics(&self) -> &CompileDiagnostics {
        &self.diagnostics
    }

    /// Compile all `.rs` files in a directory tree. Returns one SeedReport per file.
    pub fn compile_directory(dir: &Path, site_map: &mut SiteMap) -> Result<Vec<SeedReport>> {
        let mut reports = Vec::new();
        let mut rs_files: Vec<_> = walkdir_rs_files(dir)?;
        rs_files.sort();
        for path in rs_files {
            match seed_from_source(&path, site_map) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    eprintln!("[rust_to_nda] skipping {}: {e}", path.display());
                }
            }
        }
        Ok(reports)
    }

    // ── Internal: item-level compilation ──────────────────────────────────────

    fn compile_item(&mut self, item: &Item) {
        let kind_name = match item {
            Item::Fn(_) => "Fn",
            Item::Impl(_) => "Impl",
            Item::Mod(_) => "Mod",
            Item::Struct(_) => "Struct",
            Item::Enum(_) => "Enum",
            Item::Trait(_) => "Trait",
            Item::Use(_) => "Use",
            Item::Const(_) => "Const",
            Item::Static(_) => "Static",
            Item::Type(_) => "Type",
            _ => "Other",
        };
        *self
            .diagnostics
            .items_by_kind
            .entry(kind_name.to_string())
            .or_insert(0) += 1;
        match item {
            Item::Fn(f) => {
                self.compile_fn(f, None);
            }
            Item::Impl(i) => {
                self.compile_impl(i);
            }
            Item::Mod(m) => {
                // Recurse into inline modules.
                if let Some((_, items)) = &m.content {
                    for inner in items {
                        self.compile_item(inner);
                    }
                }
            }
            Item::Enum(e) => {
                self.diagnostics.warnings.push(format!(
                    "Enum '{}' not transpiled to NDA (no executable body)",
                    e.ident
                ));
            }
            Item::Struct(s) => {
                self.diagnostics.warnings.push(format!(
                    "Struct '{}' not transpiled to NDA (no executable body)",
                    s.ident
                ));
            }
            _ => {} // Traits, use, const, static, type — no executable body
        }
    }

    fn compile_impl(&mut self, impl_block: &ItemImpl) {
        // Extract type name for method qualification.
        let type_name = type_name_of(&impl_block.self_ty);
        let prev = self.current_impl.replace(type_name);

        for impl_item in &impl_block.items {
            if let ImplItem::Fn(method) = impl_item {
                let name = self.qualified_name(&method.sig.ident.to_string());
                let children = self.compile_stmts(&method.block.stmts);
                let node = NdaNode::Scope { children };
                let hash = node.hash();
                self.functions.insert(
                    name.clone(),
                    CompiledFn {
                        name,
                        node,
                        hash,
                        callees: vec![],
                    },
                );
            }
        }

        self.current_impl = prev;
    }

    fn compile_fn(&mut self, f: &ItemFn, qualifier: Option<&str>) {
        let base = f.sig.ident.to_string();
        let name = if let Some(q) = qualifier {
            format!("{q}::{base}")
        } else {
            self.qualified_name(&base)
        };

        let mut callees = Vec::new();
        let children = self.compile_stmts_with_calls(&f.block.stmts, &mut callees);
        let node = NdaNode::Scope { children };
        let hash = node.hash();

        self.functions.insert(
            name.clone(),
            CompiledFn {
                name,
                node,
                hash,
                callees,
            },
        );
    }

    fn qualified_name(&self, base: &str) -> String {
        if let Some(impl_type) = &self.current_impl {
            format!("{impl_type}::{base}")
        } else {
            base.to_string()
        }
    }

    // ── Internal: statement/expression compilation ────────────────────────────

    fn compile_stmts(&mut self, stmts: &[Stmt]) -> Vec<NdaNode> {
        let mut dummy = Vec::new();
        self.compile_stmts_with_calls(stmts, &mut dummy)
    }

    fn compile_stmts_with_calls(
        &mut self,
        stmts: &[Stmt],
        callees: &mut Vec<String>,
    ) -> Vec<NdaNode> {
        let mut nodes = Vec::new();
        for stmt in stmts {
            if let Some(node) = self.compile_stmt(stmt, callees) {
                nodes.push(node);
            }
        }
        nodes
    }

    fn compile_stmt(&mut self, stmt: &Stmt, callees: &mut Vec<String>) -> Option<NdaNode> {
        match stmt {
            Stmt::Local(local) => {
                // `let x: T = expr;`  — the type annotation is the key signal.
                if let Some(init) = &local.init {
                    // Check if type annotation is a 2D array → Matrix.
                    if let Some(ty) = extract_let_type(&local.pat) {
                        if let Some((rows, cols)) = matrix_dims_from_type(&ty) {
                            return Some(build_matrix_node(rows, cols));
                        }
                    }
                    // Otherwise compile the init expression.
                    return self.compile_expr(&init.expr, callees);
                }
                None
            }
            Stmt::Expr(expr, _) => self.compile_expr(expr, callees),
            Stmt::Item(item) => {
                self.compile_item(item);
                None
            }
            Stmt::Macro(_) => None,
        }
    }

    fn compile_expr(&mut self, expr: &Expr, callees: &mut Vec<String>) -> Option<NdaNode> {
        self.diagnostics.expressions_visited += 1;
        let expr_kind = match expr {
            Expr::Block(_) => "Block",
            Expr::Call(_) => "Call",
            Expr::MethodCall(_) => "MethodCall",
            Expr::ForLoop(_) => "ForLoop",
            Expr::While(_) => "While",
            Expr::Loop(_) => "Loop",
            Expr::If(_) => "If",
            Expr::Lit(_) => "Lit",
            Expr::Array(_) => "Array",
            Expr::Repeat(_) => "Repeat",
            Expr::Return(_) => "Return",
            Expr::Closure(_) => "Closure",
            Expr::Binary(_) => "Binary",
            Expr::Match(_) => "Match",
            Expr::Reference(_) => "Reference",
            Expr::Tuple(_) => "Tuple",
            Expr::Struct(_) => "Struct",
            Expr::Field(_) => "Field",
            Expr::Index(_) => "Index",
            Expr::Unary(_) => "Unary",
            Expr::Path(_) => "Path",
            Expr::Assign(_) => "Assign",
            Expr::Range(_) => "Range",
            _ => "Other",
        };
        *self
            .diagnostics
            .expr_type_coverage
            .entry(expr_kind.to_string())
            .or_insert(0) += 1;

        let result = self.compile_expr_inner(expr, callees);
        if result.is_some() {
            self.diagnostics.expressions_compiled += 1;
        } else {
            self.diagnostics.expressions_dropped += 1;
        }
        result
    }

    fn compile_expr_inner(&mut self, expr: &Expr, callees: &mut Vec<String>) -> Option<NdaNode> {
        match expr {
            // Block: recurse → Scope
            Expr::Block(b) => {
                let children = self.compile_stmts_with_calls(&b.block.stmts, callees);
                if children.is_empty() {
                    None
                } else {
                    Some(NdaNode::Scope { children })
                }
            }

            // Function call: record callee, emit Call node (hash resolved in pass 2)
            Expr::Call(ExprCall { func, args, .. }) => {
                let callee_name = expr_to_name(func);

                // Also compile each argument expression.
                let mut arg_nodes: Vec<NdaNode> = args
                    .iter()
                    .filter_map(|a| self.compile_expr(a, callees))
                    .collect();

                if !callee_name.is_empty() {
                    callees.push(callee_name.clone());
                    // target = 0 placeholder; resolved in pass 2.
                    arg_nodes.push(NdaNode::Call { target: 0 });
                }

                if arg_nodes.is_empty() {
                    None
                } else if arg_nodes.len() == 1 {
                    Some(arg_nodes.remove(0))
                } else {
                    Some(NdaNode::Scope {
                        children: arg_nodes,
                    })
                }
            }

            // Method call: same as function call.
            Expr::MethodCall(ExprMethodCall {
                receiver,
                method,
                args,
                ..
            }) => {
                let callee_name = method.to_string();
                callees.push(callee_name);

                let mut children = Vec::new();
                if let Some(n) = self.compile_expr(receiver, callees) {
                    children.push(n);
                }
                for a in args {
                    if let Some(n) = self.compile_expr(a, callees) {
                        children.push(n);
                    }
                }
                children.push(NdaNode::Call { target: 0 });

                if children.is_empty() {
                    None
                } else {
                    Some(NdaNode::Scope { children })
                }
            }

            // For / while loops: the body becomes a Scope.
            Expr::ForLoop(fl) => {
                let children = self.compile_stmts_with_calls(&fl.body.stmts, callees);
                if children.is_empty() {
                    None
                } else {
                    Some(NdaNode::Scope { children })
                }
            }
            Expr::While(w) => {
                let children = self.compile_stmts_with_calls(&w.body.stmts, callees);
                if children.is_empty() {
                    None
                } else {
                    Some(NdaNode::Scope { children })
                }
            }
            Expr::Loop(l) => {
                let children = self.compile_stmts_with_calls(&l.body.stmts, callees);
                if children.is_empty() {
                    None
                } else {
                    Some(NdaNode::Scope { children })
                }
            }

            // If/else: both branches become Scopes.
            Expr::If(i) => {
                let then_nodes = self.compile_stmts_with_calls(&i.then_branch.stmts, callees);
                let mut children = Vec::new();
                if !then_nodes.is_empty() {
                    children.push(NdaNode::Scope {
                        children: then_nodes,
                    });
                }
                if let Some((_, else_expr)) = &i.else_branch {
                    if let Some(n) = self.compile_expr(else_expr, callees) {
                        children.push(n);
                    }
                }
                if children.is_empty() {
                    None
                } else {
                    Some(NdaNode::Scope { children })
                }
            }

            // Integer / float literals → Int node.
            Expr::Lit(l) => match &l.lit {
                Lit::Int(i) => i
                    .base10_parse::<i32>()
                    .ok()
                    .map(|v| NdaNode::Int { value: v }),
                Lit::Float(f) => f
                    .base10_parse::<f32>()
                    .ok()
                    .map(|v| NdaNode::Int { value: v as i32 }),
                _ => None,
            },

            // Array literal → Matrix (rows=1, cols=len)
            Expr::Array(a) => {
                let cols = a.elems.len();
                if cols > 0 {
                    Some(build_matrix_node(1, cols))
                } else {
                    None
                }
            }

            // Repeat `[val; N]` → Matrix (rows=1, cols=N)
            Expr::Repeat(r) => {
                if let Expr::Lit(l) = r.len.as_ref() {
                    if let Lit::Int(i) = &l.lit {
                        if let Ok(cols) = i.base10_parse::<usize>() {
                            return Some(build_matrix_node(1, cols));
                        }
                    }
                }
                None
            }

            // Return value: compile the returned expression.
            Expr::Return(r) => r.expr.as_ref().and_then(|e| self.compile_expr(e, callees)),

            // Closure: body becomes a Scope.
            Expr::Closure(c) => self.compile_expr(&c.body, callees),

            // Match: each arm's body becomes a child Scope.
            Expr::Match(m) => {
                let mut arm_nodes = Vec::new();
                for arm in &m.arms {
                    if let Some(body_node) = self.compile_expr(&arm.body, callees) {
                        arm_nodes.push(body_node);
                    }
                }
                if arm_nodes.is_empty() {
                    None
                } else if arm_nodes.len() == 1 {
                    arm_nodes.into_iter().next()
                } else {
                    Some(NdaNode::Scope {
                        children: arm_nodes,
                    })
                }
            }

            // Reference: recurse into the inner expression.
            Expr::Reference(r) => self.compile_expr(&r.expr, callees),

            // Tuple: compile each element, wrap in Scope.
            Expr::Tuple(t) => {
                let children: Vec<NdaNode> = t
                    .elems
                    .iter()
                    .filter_map(|e| self.compile_expr(e, callees))
                    .collect();
                if children.is_empty() {
                    None
                } else {
                    Some(NdaNode::Scope { children })
                }
            }

            // Struct literal: compile each field expression.
            Expr::Struct(s) => {
                let children: Vec<NdaNode> = s
                    .fields
                    .iter()
                    .filter_map(|fv| self.compile_expr(&fv.expr, callees))
                    .collect();
                if children.is_empty() {
                    None
                } else {
                    Some(NdaNode::Scope { children })
                }
            }

            // Field access: compile the base expression.
            Expr::Field(f) => self.compile_expr(&f.base, callees),

            // Index: compile both base and index.
            Expr::Index(idx) => {
                let base = self.compile_expr(&idx.expr, callees);
                let index = self.compile_expr(&idx.index, callees);
                match (base, index) {
                    (Some(b), Some(i)) => Some(NdaNode::Scope {
                        children: vec![b, i],
                    }),
                    (Some(b), None) => Some(b),
                    (None, Some(i)) => Some(i),
                    _ => None,
                }
            }

            // Unary: compile the operand.
            Expr::Unary(u) => self.compile_expr(&u.expr, callees),

            // Assignment: compile both sides.
            Expr::Assign(a) => {
                let lhs = self.compile_expr(&a.left, callees);
                let rhs = self.compile_expr(&a.right, callees);
                match (lhs, rhs) {
                    (Some(l), Some(r)) => Some(NdaNode::Scope {
                        children: vec![l, r],
                    }),
                    (Some(l), None) => Some(l),
                    (None, Some(r)) => Some(r),
                    _ => None,
                }
            }

            // Range: compile start and end if present.
            Expr::Range(r) => {
                let mut children = Vec::new();
                if let Some(start) = &r.start {
                    if let Some(n) = self.compile_expr(start, callees) {
                        children.push(n);
                    }
                }
                if let Some(end) = &r.end {
                    if let Some(n) = self.compile_expr(end, callees) {
                        children.push(n);
                    }
                }
                if children.is_empty() {
                    None
                } else {
                    Some(NdaNode::Scope { children })
                }
            }

            // Binary ops: compile both sides, wrap in Scope if both produce nodes.
            Expr::Binary(b) => {
                let lhs = self.compile_expr(&b.left, callees);
                let rhs = self.compile_expr(&b.right, callees);
                match (lhs, rhs) {
                    (Some(l), Some(r)) => Some(NdaNode::Scope {
                        children: vec![l, r],
                    }),
                    (Some(l), None) => Some(l),
                    (None, Some(r)) => Some(r),
                    _ => None,
                }
            }

            // Everything else: attempt to recurse on sub-expressions.
            _ => {
                let mut collector = ExprCollector {
                    nodes: Vec::new(),
                    callees,
                };
                collector.visit_expr(expr);
                if collector.nodes.is_empty() {
                    None
                } else if collector.nodes.len() == 1 {
                    Some(collector.nodes.remove(0))
                } else {
                    Some(NdaNode::Scope {
                        children: collector.nodes,
                    })
                }
            }
        }
    }
}
