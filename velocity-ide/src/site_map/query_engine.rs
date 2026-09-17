#![allow(dead_code)]

use std::collections::HashMap;
use std::time::Instant;

// ---------------------------------------------------------------------------
// SymbolKind
// ---------------------------------------------------------------------------

/// Classification of a symbol in the codebase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Module,
    Constant,
    TypeAlias,
    Field,
    Method,
}

impl SymbolKind {
    /// Weight used when ranking search results — higher-value kinds surface first.
    fn relevance_boost(self) -> f64 {
        match self {
            SymbolKind::Function => 1.2,
            SymbolKind::Struct => 1.15,
            SymbolKind::Enum => 1.1,
            SymbolKind::Trait => 1.1,
            SymbolKind::Module => 1.05,
            SymbolKind::Constant => 1.0,
            SymbolKind::TypeAlias => 1.0,
            SymbolKind::Method => 0.95,
            SymbolKind::Field => 0.8,
        }
    }
}

// ---------------------------------------------------------------------------
// IndexedSymbol
// ---------------------------------------------------------------------------

/// A single symbol extracted from source code and stored in the query index.
#[derive(Clone, Debug)]
pub struct IndexedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file_hash: u64,
    pub file_path: String,
    pub line: usize,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    /// Other symbol names this symbol references (dependencies).
    pub dependencies: Vec<String>,
}

// ---------------------------------------------------------------------------
// FileSymbols
// ---------------------------------------------------------------------------

/// All symbols that belong to a single source file.
#[derive(Clone, Debug)]
pub struct FileSymbols {
    pub file_hash: u64,
    pub file_path: String,
    pub symbols: Vec<IndexedSymbol>,
    pub last_updated: Instant,
}

// ---------------------------------------------------------------------------
// QueryResult
// ---------------------------------------------------------------------------

/// One entry returned by a fuzzy search.
#[derive(Clone, Debug)]
pub struct QueryResult {
    pub symbol: IndexedSymbol,
    pub relevance_score: f64,
    pub match_reason: String,
}

// ---------------------------------------------------------------------------
// QueryEngineStats
// ---------------------------------------------------------------------------

/// Summary statistics for the current state of a [`QueryEngine`].
#[derive(Clone, Debug)]
pub struct QueryEngineStats {
    pub total_symbols: usize,
    pub total_files: usize,
    pub symbols_by_kind: HashMap<SymbolKind, usize>,
}

// ---------------------------------------------------------------------------
// QueryEngine
// ---------------------------------------------------------------------------

/// In-memory semantic index that supports fast look-ups, fuzzy search, and
/// dependency/reference queries over the symbols extracted from a codebase.
pub struct QueryEngine {
    /// symbol name (lower-cased) → list of locations
    index: HashMap<String, Vec<IndexedSymbol>>,
    /// file hash → symbols belonging to that file
    file_index: HashMap<u64, FileSymbols>,
    /// Running total of indexed symbols (avoids iterating the maps).
    total_symbols: usize,
}

impl QueryEngine {
    /// Create a new, empty engine.
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
            file_index: HashMap::new(),
            total_symbols: 0,
        }
    }

    // -- indexing -----------------------------------------------------------

    /// Add a single symbol to both the name index and the file index.
    pub fn index_symbol(&mut self, symbol: IndexedSymbol) {
        let key = symbol.name.to_lowercase();
        self.index.entry(key).or_default().push(symbol.clone());

        let file_entry = self
            .file_index
            .entry(symbol.file_hash)
            .or_insert_with(|| FileSymbols {
                file_hash: symbol.file_hash,
                file_path: symbol.file_path.clone(),
                symbols: Vec::new(),
                last_updated: Instant::now(),
            });
        file_entry.symbols.push(symbol);
        file_entry.last_updated = Instant::now();

        self.total_symbols += 1;
    }

    /// Bulk-index all symbols that belong to a single file.
    pub fn index_file(&mut self, file_hash: u64, file_path: &str, symbols: Vec<IndexedSymbol>) {
        let now = Instant::now();
        for symbol in &symbols {
            let key = symbol.name.to_lowercase();
            self.index.entry(key).or_default().push(symbol.clone());
            self.total_symbols += 1;
        }
        self.file_index.insert(
            file_hash,
            FileSymbols {
                file_hash,
                file_path: file_path.to_string(),
                symbols,
                last_updated: now,
            },
        );
    }

    /// Remove every symbol that was indexed under `file_hash`.
    pub fn remove_file(&mut self, file_hash: u64) {
        let Some(file_symbols) = self.file_index.remove(&file_hash) else {
            return;
        };
        for sym in &file_symbols.symbols {
            let key = sym.name.to_lowercase();
            if let Some(entries) = self.index.get_mut(&key) {
                entries.retain(|s| s.file_hash != file_hash);
                if entries.is_empty() {
                    self.index.remove(&key);
                }
            }
            self.total_symbols = self.total_symbols.saturating_sub(1);
        }
    }

    // -- querying -----------------------------------------------------------

    /// Return all symbols whose name matches `name` exactly (case-insensitive).
    pub fn find_symbol(&self, name: &str) -> Vec<&IndexedSymbol> {
        let key = name.to_lowercase();
        self.index
            .get(&key)
            .map_or_else(Vec::new, |v| v.iter().collect())
    }

    /// Fuzzy search across all indexed symbols.
    ///
    /// Scoring rules:
    /// - exact match (case-insensitive) → base 1.0
    /// - prefix match                   → base 0.8
    /// - substring match                → base 0.5
    ///
    /// The base score is multiplied by the kind-specific relevance boost so
    /// that functions and structs rank above fields, for example.
    ///
    /// Results are sorted descending by score and capped at `max_results`.
    pub fn search(&self, query: &str, max_results: usize) -> Vec<QueryResult> {
        if query.is_empty() || max_results == 0 {
            return Vec::new();
        }
        let q_lower = query.to_lowercase();
        let mut results: Vec<QueryResult> = Vec::new();

        for (key, symbols) in &self.index {
            let (base_score, reason) = if key == &q_lower {
                (1.0, "exact match".to_string())
            } else if key.starts_with(&q_lower) {
                (0.8, "prefix match".to_string())
            } else if key.contains(&q_lower) {
                (0.5, "substring match".to_string())
            } else {
                continue;
            };

            for sym in symbols {
                let boosted = base_score * sym.kind.relevance_boost();
                results.push(QueryResult {
                    symbol: sym.clone(),
                    relevance_score: boosted,
                    match_reason: reason.clone(),
                });
            }
        }

        results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());
        results.truncate(max_results);
        results
    }

    /// Find all indexed symbols that list `symbol_name` in their `dependencies`.
    pub fn find_references(&self, symbol_name: &str) -> Vec<&IndexedSymbol> {
        let target = symbol_name.to_lowercase();
        let mut refs = Vec::new();
        for symbols in self.index.values() {
            for sym in symbols {
                if sym.dependencies.iter().any(|d| d.to_lowercase() == target) {
                    refs.push(sym);
                }
            }
        }
        refs
    }

    /// Return the [`FileSymbols`] record for a given file hash, if present.
    pub fn file_symbols(&self, file_hash: u64) -> Option<&FileSymbols> {
        self.file_index.get(&file_hash)
    }

    /// Return every distinct symbol name currently in the index (useful for
    /// autocomplete / type-ahead).
    pub fn all_symbol_names(&self) -> Vec<&str> {
        // The keys in `self.index` are lower-cased; return them as-is so that
        // callers can filter / prefix-match on them.
        self.index.keys().map(|k| k.as_str()).collect()
    }

    /// Compute summary statistics.
    pub fn stats(&self) -> QueryEngineStats {
        let mut by_kind: HashMap<SymbolKind, usize> = HashMap::new();
        for symbols in self.index.values() {
            for sym in symbols {
                *by_kind.entry(sym.kind).or_insert(0) += 1;
            }
        }
        QueryEngineStats {
            total_symbols: self.total_symbols,
            total_files: self.file_index.len(),
            symbols_by_kind: by_kind,
        }
    }
}

impl Default for QueryEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers ------------------------------------------------------------

    fn make_sym(name: &str, kind: SymbolKind, file_hash: u64, file_path: &str) -> IndexedSymbol {
        IndexedSymbol {
            name: name.to_string(),
            kind,
            file_hash,
            file_path: file_path.to_string(),
            line: 1,
            signature: None,
            doc_comment: None,
            dependencies: Vec::new(),
        }
    }

    fn make_sym_with_deps(
        name: &str,
        kind: SymbolKind,
        file_hash: u64,
        file_path: &str,
        deps: Vec<&str>,
    ) -> IndexedSymbol {
        IndexedSymbol {
            name: name.to_string(),
            kind,
            file_hash,
            file_path: file_path.to_string(),
            line: 1,
            signature: None,
            doc_comment: None,
            dependencies: deps.into_iter().map(String::from).collect(),
        }
    }

    // -- basic indexing & lookup -------------------------------------------

    #[test]
    fn test_index_and_find_symbol() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("foo", SymbolKind::Function, 1, "a.rs"));
        let found = engine.find_symbol("foo");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "foo");
    }

    #[test]
    fn test_find_symbol_case_insensitive() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("MyStruct", SymbolKind::Struct, 1, "a.rs"));
        assert_eq!(engine.find_symbol("mystruct").len(), 1);
        assert_eq!(engine.find_symbol("MYSTRUCT").len(), 1);
        assert_eq!(engine.find_symbol("MyStruct").len(), 1);
    }

    #[test]
    fn test_find_symbol_not_found() {
        let engine = QueryEngine::new();
        assert!(engine.find_symbol("nope").is_empty());
    }

    #[test]
    fn test_multiple_symbols_same_name() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("run", SymbolKind::Function, 1, "a.rs"));
        engine.index_symbol(make_sym("run", SymbolKind::Method, 2, "b.rs"));
        assert_eq!(engine.find_symbol("run").len(), 2);
    }

    // -- file indexing & removal -------------------------------------------

    #[test]
    fn test_index_file_bulk() {
        let mut engine = QueryEngine::new();
        let syms = vec![
            make_sym("alpha", SymbolKind::Function, 42, "alpha.rs"),
            make_sym("beta", SymbolKind::Struct, 42, "alpha.rs"),
        ];
        engine.index_file(42, "alpha.rs", syms);
        assert_eq!(engine.find_symbol("alpha").len(), 1);
        assert_eq!(engine.find_symbol("beta").len(), 1);
        assert_eq!(engine.stats().total_files, 1);
    }

    #[test]
    fn test_file_symbols_accessor() {
        let mut engine = QueryEngine::new();
        engine.index_file(
            7,
            "main.rs",
            vec![make_sym("main", SymbolKind::Function, 7, "main.rs")],
        );
        let fs = engine.file_symbols(7).expect("should exist");
        assert_eq!(fs.file_path, "main.rs");
        assert_eq!(fs.symbols.len(), 1);
    }

    #[test]
    fn test_remove_file() {
        let mut engine = QueryEngine::new();
        engine.index_file(
            10,
            "removeme.rs",
            vec![
                make_sym("x", SymbolKind::Constant, 10, "removeme.rs"),
                make_sym("y", SymbolKind::Constant, 10, "removeme.rs"),
            ],
        );
        assert_eq!(engine.stats().total_symbols, 2);
        engine.remove_file(10);
        assert_eq!(engine.stats().total_symbols, 0);
        assert!(engine.file_symbols(10).is_none());
        assert!(engine.find_symbol("x").is_empty());
    }

    #[test]
    fn test_remove_file_nonexistent_is_noop() {
        let mut engine = QueryEngine::new();
        engine.remove_file(999); // should not panic
        assert_eq!(engine.stats().total_symbols, 0);
    }

    #[test]
    fn test_remove_file_preserves_other_files() {
        let mut engine = QueryEngine::new();
        engine.index_file(
            1,
            "keep.rs",
            vec![make_sym("a", SymbolKind::Function, 1, "keep.rs")],
        );
        engine.index_file(
            2,
            "drop.rs",
            vec![make_sym("b", SymbolKind::Function, 2, "drop.rs")],
        );
        engine.remove_file(2);
        assert_eq!(engine.find_symbol("a").len(), 1);
        assert!(engine.find_symbol("b").is_empty());
    }

    // -- fuzzy search -------------------------------------------------------

    #[test]
    fn test_search_exact_match_highest_score() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("parse", SymbolKind::Function, 1, "a.rs"));
        engine.index_symbol(make_sym("parse_all", SymbolKind::Function, 1, "a.rs"));
        engine.index_symbol(make_sym("quickparse", SymbolKind::Function, 1, "a.rs"));

        let results = engine.search("parse", 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].symbol.name, "parse");
        assert_eq!(results[0].match_reason, "exact match");
    }

    #[test]
    fn test_search_prefix_beats_substring() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("quickparse", SymbolKind::Function, 1, "a.rs"));
        engine.index_symbol(make_sym("parse_all", SymbolKind::Function, 1, "a.rs"));

        let results = engine.search("parse", 10);
        // prefix match should outrank substring
        assert_eq!(results[0].symbol.name, "parse_all");
        assert_eq!(results[0].match_reason, "prefix match");
    }

    #[test]
    fn test_search_kind_boost() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("data", SymbolKind::Field, 1, "a.rs"));
        engine.index_symbol(make_sym("data", SymbolKind::Function, 1, "a.rs"));

        let results = engine.search("data", 10);
        // Function has a higher boost than Field
        assert_eq!(results[0].symbol.kind, SymbolKind::Function);
    }

    #[test]
    fn test_search_max_results_respected() {
        let mut engine = QueryEngine::new();
        for i in 0..20 {
            engine.index_symbol(make_sym(
                &format!("sym_{i}"),
                SymbolKind::Function,
                1,
                "a.rs",
            ));
        }
        let results = engine.search("sym", 5);
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_search_empty_query_returns_nothing() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("foo", SymbolKind::Function, 1, "a.rs"));
        assert!(engine.search("", 10).is_empty());
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("MyFunction", SymbolKind::Function, 1, "a.rs"));
        let results = engine.search("myfunction", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].match_reason, "exact match");
    }

    #[test]
    fn test_search_no_match() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("hello", SymbolKind::Function, 1, "a.rs"));
        assert!(engine.search("zzz", 10).is_empty());
    }

    // -- references ---------------------------------------------------------

    #[test]
    fn test_find_references_basic() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("Config", SymbolKind::Struct, 1, "a.rs"));
        engine.index_symbol(make_sym_with_deps(
            "load_config",
            SymbolKind::Function,
            1,
            "a.rs",
            vec!["Config"],
        ));
        let refs = engine.find_references("Config");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "load_config");
    }

    #[test]
    fn test_find_references_case_insensitive() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym_with_deps(
            "use_it",
            SymbolKind::Function,
            1,
            "a.rs",
            vec!["MyType"],
        ));
        assert_eq!(engine.find_references("mytype").len(), 1);
        assert_eq!(engine.find_references("MYTYPE").len(), 1);
    }

    #[test]
    fn test_find_references_none() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("lonely", SymbolKind::Function, 1, "a.rs"));
        assert!(engine.find_references("lonely").is_empty());
    }

    // -- autocomplete / all_symbol_names ------------------------------------

    #[test]
    fn test_all_symbol_names_basic() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("Alpha", SymbolKind::Function, 1, "a.rs"));
        engine.index_symbol(make_sym("Beta", SymbolKind::Struct, 1, "a.rs"));
        let mut names = engine.all_symbol_names();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_all_symbol_names_empty_engine() {
        let engine = QueryEngine::new();
        assert!(engine.all_symbol_names().is_empty());
    }

    #[test]
    fn test_all_symbol_names_deduplicated_per_key() {
        let mut engine = QueryEngine::new();
        // Two symbols with the same name → only one key in the index.
        engine.index_symbol(make_sym("dup", SymbolKind::Function, 1, "a.rs"));
        engine.index_symbol(make_sym("dup", SymbolKind::Method, 2, "b.rs"));
        assert_eq!(engine.all_symbol_names().len(), 1);
    }

    // -- stats --------------------------------------------------------------

    #[test]
    fn test_stats_empty_engine() {
        let engine = QueryEngine::new();
        let s = engine.stats();
        assert_eq!(s.total_symbols, 0);
        assert_eq!(s.total_files, 0);
        assert!(s.symbols_by_kind.is_empty());
    }

    #[test]
    fn test_stats_counts() {
        let mut engine = QueryEngine::new();
        engine.index_file(
            1,
            "a.rs",
            vec![
                make_sym("f1", SymbolKind::Function, 1, "a.rs"),
                make_sym("f2", SymbolKind::Function, 1, "a.rs"),
                make_sym("S1", SymbolKind::Struct, 1, "a.rs"),
            ],
        );
        engine.index_file(2, "b.rs", vec![make_sym("E1", SymbolKind::Enum, 2, "b.rs")]);
        let s = engine.stats();
        assert_eq!(s.total_symbols, 4);
        assert_eq!(s.total_files, 2);
        assert_eq!(*s.symbols_by_kind.get(&SymbolKind::Function).unwrap(), 2);
        assert_eq!(*s.symbols_by_kind.get(&SymbolKind::Struct).unwrap(), 1);
        assert_eq!(*s.symbols_by_kind.get(&SymbolKind::Enum).unwrap(), 1);
    }

    // -- multi-file indexing ------------------------------------------------

    #[test]
    fn test_multi_file_same_symbol_name() {
        let mut engine = QueryEngine::new();
        engine.index_file(
            1,
            "a.rs",
            vec![make_sym("process", SymbolKind::Function, 1, "a.rs")],
        );
        engine.index_file(
            2,
            "b.rs",
            vec![make_sym("process", SymbolKind::Function, 2, "b.rs")],
        );
        let found = engine.find_symbol("process");
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|s| s.file_hash == 1));
        assert!(found.iter().any(|s| s.file_hash == 2));
    }

    #[test]
    fn test_multi_file_remove_only_one() {
        let mut engine = QueryEngine::new();
        engine.index_file(
            1,
            "a.rs",
            vec![make_sym("shared", SymbolKind::Function, 1, "a.rs")],
        );
        engine.index_file(
            2,
            "b.rs",
            vec![make_sym("shared", SymbolKind::Function, 2, "b.rs")],
        );
        engine.remove_file(1);
        let found = engine.find_symbol("shared");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_hash, 2);
    }

    // -- edge cases ---------------------------------------------------------

    #[test]
    fn test_default_trait_creates_empty_engine() {
        let engine = QueryEngine::default();
        assert_eq!(engine.stats().total_symbols, 0);
    }

    #[test]
    fn test_search_max_results_zero() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("foo", SymbolKind::Function, 1, "a.rs"));
        assert!(engine.search("foo", 0).is_empty());
    }

    #[test]
    fn test_index_symbol_updates_file_index() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("sym1", SymbolKind::Function, 5, "x.rs"));
        engine.index_symbol(make_sym("sym2", SymbolKind::Struct, 5, "x.rs"));
        let fs = engine.file_symbols(5).unwrap();
        assert_eq!(fs.symbols.len(), 2);
        assert_eq!(fs.file_path, "x.rs");
    }

    #[test]
    fn test_find_references_multiple_dependents() {
        let mut engine = QueryEngine::new();
        engine.index_symbol(make_sym("Base", SymbolKind::Struct, 1, "a.rs"));
        engine.index_symbol(make_sym_with_deps(
            "user_a",
            SymbolKind::Function,
            1,
            "a.rs",
            vec!["Base"],
        ));
        engine.index_symbol(make_sym_with_deps(
            "user_b",
            SymbolKind::Function,
            2,
            "b.rs",
            vec!["Base", "Other"],
        ));
        let refs = engine.find_references("Base");
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_symbol_kind_relevance_boost_ordering() {
        // Sanity-check the boost values directly.
        assert!(SymbolKind::Function.relevance_boost() > SymbolKind::Field.relevance_boost());
        assert!(SymbolKind::Struct.relevance_boost() > SymbolKind::Method.relevance_boost());
    }
}
