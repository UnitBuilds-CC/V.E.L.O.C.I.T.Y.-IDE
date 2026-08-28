//! Integration tests for wiki generation and Markdown export.

use crate::site_map::{SiteMap, VcTriple};
use crate::wiki::markdown::slugify_module;
use crate::wiki::{build_wiki, export_markdown, WikiPageKind, WikiPage, render_page_markdown};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("velocity_wiki_test_{}_{}", name, nanos))
}

#[test]
fn builds_file_and_symbol_pages_with_relationships() {
    let dir = temp_dir("build");
    let mut sm = SiteMap::open(&dir, 0).expect("open site map");

    let file = "src/lib.rs";
    let caller = "caller_fn";
    let callee = "callee_fn";

    let file_hash = sm.register_string(file).unwrap();
    let caller_hash = sm.register_string(caller).unwrap();
    let callee_hash = sm.register_string(callee).unwrap();

    // File defines both functions (predicate 1).
    sm.put_file_snapshot(
        file,
        &[
            VcTriple {
                subject_hash: file_hash,
                predicate_id: 1,
                object_hash: caller_hash,
            },
            VcTriple {
                subject_hash: file_hash,
                predicate_id: 1,
                object_hash: callee_hash,
            },
        ],
    )
    .unwrap();

    // caller calls callee (predicate 2).
    sm.put_file_snapshot(
        "calls.snapshot",
        &[VcTriple {
            subject_hash: caller_hash,
            predicate_id: 2,
            object_hash: callee_hash,
        }],
    )
    .unwrap();

    let model = build_wiki(&sm);
    assert!(!model.is_empty());
    assert_eq!(model.file_count(), 1, "expected one file page");
    assert_eq!(model.symbol_count(), 2, "expected two symbol pages");

    let file_page = model
        .file_pages
        .iter()
        .find(|p| p.title == file)
        .expect("file page");
    let defines = file_page
        .relationships
        .iter()
        .find(|(label, _)| label == "Defines")
        .map(|(_, targets): &(String, Vec<String>)| targets.clone())
        .unwrap_or_default();
    assert!(defines.contains(&caller.to_string()));
    assert!(defines.contains(&callee.to_string()));

    let callee_page = model
        .symbol_pages
        .iter()
        .find(|p| p.title == callee)
        .expect("callee page");
    assert!(
        callee_page.called_by.contains(&caller.to_string()),
        "callee should record an incoming call from caller"
    );

    let caller_page = model
        .symbol_pages
        .iter()
        .find(|p| p.title == caller)
        .expect("caller page");
    let calls = caller_page
        .relationships
        .iter()
        .find(|(label, _)| label == "Calls")
        .map(|(_, targets): &(String, Vec<String>)| targets.clone())
        .unwrap_or_default();
    assert!(calls.contains(&callee.to_string()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn empty_site_map_yields_overview_only() {
    let dir = temp_dir("empty");
    let sm = SiteMap::open(&dir, 0).expect("open site map");
    let model = build_wiki(&sm);
    assert!(model.is_empty());
    assert_eq!(model.overview.title, "Overview");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exports_interlinked_markdown() {
    let dir = temp_dir("export_sm");
    let out = temp_dir("export_out");
    let mut sm = SiteMap::open(&dir, 0).expect("open site map");

    let file = "src/lib.rs";
    let func = "do_thing";
    let file_hash = sm.register_string(file).unwrap();
    let func_hash = sm.register_string(func).unwrap();
    sm.put_file_snapshot(
        file,
        &[VcTriple {
            subject_hash: file_hash,
            predicate_id: 1,
            object_hash: func_hash,
        }],
    )
    .unwrap();

    let model = build_wiki(&sm);
    let count = export_markdown(&model, &out).expect("export");
    // index + 1 file page + 1 symbol page + symbol_index + graph
    assert_eq!(count, 5);

    let index = std::fs::read_to_string(out.join("index.md")).expect("index.md");
    assert!(index.contains("# Project Wiki"));
    assert!(index.contains(file));
    assert!(index.contains(func));

    let file_slug = model
        .file_pages
        .iter()
        .find(|p| p.title == file)
        .map(|p| p.slug.clone())
        .unwrap();
    // Files are now grouped by module directory (e.g., "src" from "src/lib.rs")
    let module = file.split('/').next().unwrap_or("root");
    let module = if module.contains('.') || module.is_empty() {
        "root"
    } else {
        module
    };
    let module_slug = slugify_module(module);
    let file_md = std::fs::read_to_string(
        out.join("files")
            .join(&module_slug)
            .join(format!("{}.md", file_slug)),
    )
    .expect("file page markdown");
    assert!(file_md.contains("## Defines"));
    // The defined symbol should be cross-linked to its own page.
    assert!(file_md.contains("../symbols/"));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn wiki_search_finds_pages_by_title() {
    let dir = temp_dir("search_title");
    let mut sm = SiteMap::open(&dir, 0).expect("open site map");

    let file = "src/lib.rs";
    let func = "do_thing";
    let file_hash = sm.register_string(file).unwrap();
    let func_hash = sm.register_string(func).unwrap();
    sm.put_file_snapshot(
        file,
        &[VcTriple {
            subject_hash: file_hash,
            predicate_id: 1,
            object_hash: func_hash,
        }],
    )
    .unwrap();

    let model = build_wiki(&sm);

    // Search by title.
    let results = model.search("do_thing");
    assert!(!results.is_empty(), "should find symbol page by title");
    assert_eq!(results[0].page.title, "do_thing");
    assert!(results[0].score > 0);

    // Search by partial title.
    let results = model.search("do");
    assert!(!results.is_empty(), "should find by partial title");

    // Case-insensitive search.
    let results = model.search("DO_THING");
    assert!(!results.is_empty(), "search should be case-insensitive");

    // Empty query returns nothing.
    let results = model.search("");
    assert!(results.is_empty(), "empty query should return no results");

    // Non-matching query returns nothing.
    let results = model.search("nonexistent_xyz");
    assert!(results.is_empty(), "non-matching query should return no results");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_search_ranks_title_matches_higher() {
    let dir = temp_dir("search_rank");
    let mut sm = SiteMap::open(&dir, 0).expect("open site map");

    // Create a file that defines a symbol with a distinctive name.
    let file = "src/lib.rs";
    let func = "merkle_verify";
    let file_hash = sm.register_string(file).unwrap();
    let func_hash = sm.register_string(func).unwrap();
    sm.put_file_snapshot(
        file,
        &[VcTriple {
            subject_hash: file_hash,
            predicate_id: 1,
            object_hash: func_hash,
        }],
    )
    .unwrap();

    let model = build_wiki(&sm);
    let results = model.search("merkle");

    // The symbol "merkle_verify" should rank higher than the file page
    // because its title contains the search term (title match bonus).
    assert!(results.len() >= 2, "should find both file and symbol");
    assert_eq!(results[0].page.title, "merkle_verify", "symbol should rank first (title match)");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_find_by_title_and_slug() {
    let dir = temp_dir("find_by");
    let mut sm = SiteMap::open(&dir, 0).expect("open site map");

    let file = "src/lib.rs";
    let func = "my_function";
    let file_hash = sm.register_string(file).unwrap();
    let func_hash = sm.register_string(func).unwrap();
    sm.put_file_snapshot(
        file,
        &[VcTriple {
            subject_hash: file_hash,
            predicate_id: 1,
            object_hash: func_hash,
        }],
    )
    .unwrap();

    let model = build_wiki(&sm);

    // Find by title.
    assert!(model.find_by_title("src/lib.rs").is_some());
    assert!(model.find_by_title("my_function").is_some());
    assert!(model.find_by_title("nonexistent").is_none());

    // Find by slug.
    assert!(model.find_by_slug("my_function").is_some());
    assert!(model.find_by_slug("nonexistent").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_total_pages_count() {
    let dir = temp_dir("total_pages");
    let mut sm = SiteMap::open(&dir, 0).expect("open site map");

    let file = "src/lib.rs";
    let func = "helper";
    let file_hash = sm.register_string(file).unwrap();
    let func_hash = sm.register_string(func).unwrap();
    sm.put_file_snapshot(
        file,
        &[VcTriple {
            subject_hash: file_hash,
            predicate_id: 1,
            object_hash: func_hash,
        }],
    )
    .unwrap();

    let model = build_wiki(&sm);
    // 1 overview + 1 file + 1 symbol = 3
    assert_eq!(model.total_pages(), 3);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_symbols_defined_by_and_files_referencing() {
    let dir = temp_dir("symbols_by");
    let mut sm = SiteMap::open(&dir, 0).expect("open site map");

    let file = "src/lib.rs";
    let func_a = "func_a";
    let func_b = "func_b";
    let file_hash = sm.register_string(file).unwrap();
    let a_hash = sm.register_string(func_a).unwrap();
    let b_hash = sm.register_string(func_b).unwrap();
    sm.put_file_snapshot(
        file,
        &[
            VcTriple {
                subject_hash: file_hash,
                predicate_id: 1,
                object_hash: a_hash,
            },
            VcTriple {
                subject_hash: file_hash,
                predicate_id: 1,
                object_hash: b_hash,
            },
        ],
    )
    .unwrap();

    let model = build_wiki(&sm);

    let symbols = model.symbols_defined_by(file);
    assert_eq!(symbols.len(), 2, "file defines 2 symbols");

    let files = model.files_referencing(func_a);
    assert_eq!(files.len(), 1, "func_a referenced by 1 file");
    assert_eq!(files[0].title, file);

    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiPageKind tests ──────────────────────────────────────────────────────

#[test]
fn wiki_page_kind_labels() {
    assert_eq!(WikiPageKind::Overview.label(), "Overview");
    assert_eq!(WikiPageKind::File.label(), "File");
    assert_eq!(WikiPageKind::Symbol.label(), "Symbol");
}

#[test]
fn wiki_page_kind_derives() {
    // Clone + Copy
    let kind = WikiPageKind::File;
    let copied = kind;
    assert_eq!(kind, copied);
    let cloned = kind.clone();
    assert_eq!(kind, cloned);
    // Debug
    let debug = format!("{:?}", kind);
    assert_eq!(debug, "File");
    // PartialEq / Eq
    assert_eq!(WikiPageKind::Overview, WikiPageKind::Overview);
    assert_ne!(WikiPageKind::File, WikiPageKind::Symbol);
}

#[test]
fn wiki_page_kind_serializes() {
    let kind = WikiPageKind::Symbol;
    let json = serde_json::to_string(&kind).unwrap();
    assert_eq!(json, "\"Symbol\"");
}

// ── WikiModel structural tests ──────────────────────────────────────────────

#[test]
fn wiki_model_clone() {
    let dir = temp_dir("wiki_clone");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/main.rs").unwrap();
    let sh = sm.register_string("main").unwrap();
    sm.put_file_snapshot("src/main.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let cloned = model.clone();
    assert_eq!(cloned.file_count(), model.file_count());
    assert_eq!(cloned.symbol_count(), model.symbol_count());
    assert_eq!(cloned.total_pages(), model.total_pages());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_model_debug_format() {
    let dir = temp_dir("wiki_debug");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let debug = format!("{:?}", model);
    assert!(debug.contains("WikiModel"));
    assert!(debug.contains("overview"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_all_pages_includes_overview() {
    let dir = temp_dir("wiki_all");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let all: Vec<_> = model.all_pages().collect();
    // At minimum: overview page
    assert!(all.len() >= 1);
    assert_eq!(all[0].kind, WikiPageKind::Overview);
    assert_eq!(all[0].title, "Overview");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_all_pages_ordering() {
    let dir = temp_dir("wiki_order");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let sh = sm.register_string("my_func").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let all: Vec<_> = model.all_pages().collect();
    // Overview first, then file pages, then symbol pages
    assert_eq!(all[0].kind, WikiPageKind::Overview);
    // Find file and symbol pages
    let file_idx = all.iter().position(|p| p.kind == WikiPageKind::File).unwrap();
    let sym_idx = all.iter().position(|p| p.kind == WikiPageKind::Symbol).unwrap();
    assert!(file_idx < sym_idx, "file pages should come before symbol pages");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel validation tests ──────────────────────────────────────────────

#[test]
fn wiki_validate_empty_model() {
    let dir = temp_dir("wiki_val_empty");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let warnings = model.validate();
    assert!(warnings.iter().any(|w| w.contains("no file or symbol")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_validate_non_empty_no_warnings() {
    let dir = temp_dir("wiki_val_ok");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/test.rs").unwrap();
    let sh = sm.register_string("test_fn").unwrap();
    sm.put_file_snapshot("src/test.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let warnings = model.validate();
    assert!(warnings.is_empty(), "non-empty wiki should have no warnings, got: {:?}", warnings);
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel stats tests ───────────────────────────────────────────────────

#[test]
fn wiki_stats_empty_model() {
    let dir = temp_dir("wiki_stats_empty");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let stats = model.stats();
    assert_eq!(stats.total_pages, 1); // just overview
    assert_eq!(stats.file_pages, 0);
    assert_eq!(stats.symbol_pages, 0);
    assert_eq!(stats.total_relationships, 0);
    assert_eq!(stats.total_called_by, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_stats_with_content() {
    let dir = temp_dir("wiki_stats_content");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let sh = sm.register_string("my_fn").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let stats = model.stats();
    assert_eq!(stats.total_pages, 3); // overview + 1 file + 1 symbol
    assert_eq!(stats.file_pages, 1);
    assert_eq!(stats.symbol_pages, 1);
    assert!(stats.total_relationships > 0, "should have at least one relationship");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_stats_serializes() {
    let dir = temp_dir("wiki_stats_ser");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let stats = model.stats();
    let json = serde_json::to_string(&stats).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["total_pages"], 1);
    assert!(parsed["generated_at"].is_string());
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel info tests ────────────────────────────────────────────────────

#[test]
fn wiki_model_info_empty() {
    let dir = temp_dir("wiki_info_empty");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let info = model.info();
    assert_eq!(info.total_pages, 1);
    assert_eq!(info.file_pages, 0);
    assert_eq!(info.symbol_pages, 0);
    assert_eq!(info.orphan_pages, 0);
    assert!(info.top_symbols.is_empty());
    assert!(!info.validation_issues.is_empty()); // empty wiki has warnings
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_model_info_with_content() {
    let dir = temp_dir("wiki_info_content");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let sh = sm.register_string("helper").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let info = model.info();
    assert_eq!(info.total_pages, 3);
    assert_eq!(info.file_pages, 1);
    assert_eq!(info.symbol_pages, 1);
    assert!(info.validation_issues.is_empty(), "non-empty wiki should be valid");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_model_info_serializes() {
    let dir = temp_dir("wiki_info_ser");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let info = model.info();
    let json = serde_json::to_string(&info).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed["total_pages"].is_number());
    assert!(parsed["validation_issues"].is_array());
    assert!(parsed["top_symbols"].is_array());
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel orphan & top symbols tests ────────────────────────────────────

#[test]
fn wiki_orphan_pages_none_when_connected() {
    let dir = temp_dir("wiki_orphan_conn");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let sh = sm.register_string("func").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let orphans = model.orphan_pages();
    // All pages have relationships or are overview
    assert!(orphans.is_empty(), "connected wiki should have no orphans, got: {:?}",
        orphans.iter().map(|p| &p.title).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_top_symbols_limit() {
    let dir = temp_dir("wiki_top_sym");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let s1 = sm.register_string("alpha").unwrap();
    let s2 = sm.register_string("beta").unwrap();
    let s3 = sm.register_string("gamma").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[
        VcTriple { subject_hash: fh, predicate_id: 1, object_hash: s1 },
        VcTriple { subject_hash: fh, predicate_id: 1, object_hash: s2 },
        VcTriple { subject_hash: fh, predicate_id: 1, object_hash: s3 },
    ]).unwrap();
    let model = build_wiki(&sm);
    let top2 = model.top_symbols(2);
    assert_eq!(top2.len(), 2, "should limit to 2");
    let top100 = model.top_symbols(100);
    assert_eq!(top100.len(), 3, "only 3 symbols exist");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_top_symbols_zero_limit() {
    let dir = temp_dir("wiki_top_zero");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let sh = sm.register_string("func").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let top0 = model.top_symbols(0);
    assert!(top0.is_empty(), "limit=0 should return empty");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel relationship edges tests ──────────────────────────────────────

#[test]
fn wiki_relationship_edges_empty() {
    let dir = temp_dir("wiki_edges_empty");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let edges = model.relationship_edges();
    assert!(edges.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_relationship_edges_with_content() {
    let dir = temp_dir("wiki_edges");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let file = "src/lib.rs";
    let func = "my_func";
    let fh = sm.register_string(file).unwrap();
    let sh = sm.register_string(func).unwrap();
    sm.put_file_snapshot(file, &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let edges = model.relationship_edges();
    assert!(!edges.is_empty(), "should have at least one edge");
    // Check edge structure
    let edge = &edges[0];
    assert!(!edge.source.is_empty());
    assert!(!edge.label.is_empty());
    assert!(!edge.target.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel batch_search tests ────────────────────────────────────────────

#[test]
fn wiki_batch_search_deduplicates() {
    let dir = temp_dir("wiki_batch_dedup");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("lib.rs").unwrap();
    let sh = sm.register_string("merkle_verify").unwrap();
    sm.put_file_snapshot("lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    // Search for the same thing with two different queries
    let results = model.batch_search(&["merkle", "verify"]);
    // Each page should appear only once
    let slugs: Vec<_> = results.iter().map(|r| r.page.slug.as_str()).collect();
    let unique: std::collections::HashSet<_> = slugs.iter().copied().collect();
    assert_eq!(slugs.len(), unique.len(), "batch_search should deduplicate");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_batch_search_empty_queries() {
    let dir = temp_dir("wiki_batch_empty");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let results = model.batch_search(&[]);
    assert!(results.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel search_by_kind tests ──────────────────────────────────────────

#[test]
fn wiki_search_by_kind_file_only() {
    let dir = temp_dir("wiki_kind_file");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let sh = sm.register_string("my_func").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    // Search for "lib" but only in File pages
    let results = model.search_by_kind("lib", WikiPageKind::File);
    assert!(!results.is_empty());
    assert!(results.iter().all(|r| r.page.kind == WikiPageKind::File));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_search_by_kind_symbol_only() {
    let dir = temp_dir("wiki_kind_sym");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let sh = sm.register_string("my_func").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let results = model.search_by_kind("func", WikiPageKind::Symbol);
    assert!(!results.is_empty());
    assert!(results.iter().all(|r| r.page.kind == WikiPageKind::Symbol));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_search_by_kind_empty_query() {
    let dir = temp_dir("wiki_kind_empty");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let results = model.search_by_kind("", WikiPageKind::File);
    assert!(results.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel paginated search tests ────────────────────────────────────────

#[test]
fn wiki_search_paginated_first_page() {
    let dir = temp_dir("wiki_pag_first");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("lib.rs").unwrap();
    let s1 = sm.register_string("alpha_fn").unwrap();
    let s2 = sm.register_string("beta_fn").unwrap();
    sm.put_file_snapshot("lib.rs", &[
        VcTriple { subject_hash: fh, predicate_id: 1, object_hash: s1 },
        VcTriple { subject_hash: fh, predicate_id: 1, object_hash: s2 },
    ]).unwrap();
    let model = build_wiki(&sm);
    let page = model.search_paginated("fn", 1, 0);
    assert_eq!(page.results.len(), 1);
    assert_eq!(page.offset, 0);
    assert_eq!(page.limit, 1);
    assert!(page.total_matches >= 2);
    assert!(page.has_more);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_search_paginated_last_page() {
    let dir = temp_dir("wiki_pag_last");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("lib.rs").unwrap();
    let s1 = sm.register_string("alpha_fn").unwrap();
    let s2 = sm.register_string("beta_fn").unwrap();
    sm.put_file_snapshot("lib.rs", &[
        VcTriple { subject_hash: fh, predicate_id: 1, object_hash: s1 },
        VcTriple { subject_hash: fh, predicate_id: 1, object_hash: s2 },
    ]).unwrap();
    let model = build_wiki(&sm);
    let page = model.search_paginated("fn", 10, 0);
    assert!(!page.has_more, "all results fit in one page");
    assert_eq!(page.results.len(), page.total_matches);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_search_paginated_beyond_results() {
    let dir = temp_dir("wiki_pag_beyond");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("lib.rs").unwrap();
    let sh = sm.register_string("my_func").unwrap();
    sm.put_file_snapshot("lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let page = model.search_paginated("func", 10, 100);
    assert!(page.results.is_empty(), "offset beyond results");
    assert!(!page.has_more);
    assert!(page.total_matches > 0);
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel autocomplete tests ────────────────────────────────────────────

#[test]
fn wiki_autocomplete_prefix_match() {
    let dir = temp_dir("wiki_auto_prefix");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("lib.rs").unwrap();
    let sh = sm.register_string("merkle_verify").unwrap();
    sm.put_file_snapshot("lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let suggestions = model.autocomplete("merkle", 10);
    assert!(!suggestions.is_empty());
    assert!(suggestions.iter().any(|s| s.title == "merkle_verify"));
    // Check match_type
    let m = suggestions.iter().find(|s| s.title == "merkle_verify").unwrap();
    assert_eq!(m.match_type, "prefix");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_autocomplete_contains_match() {
    let dir = temp_dir("wiki_auto_contains");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("lib.rs").unwrap();
    let sh = sm.register_string("merkle_verify").unwrap();
    sm.put_file_snapshot("lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    // "verify" is not a prefix but is contained in "merkle_verify"
    let suggestions = model.autocomplete("verify", 10);
    assert!(!suggestions.is_empty());
    let m = suggestions.iter().find(|s| s.title == "merkle_verify").unwrap();
    assert_eq!(m.match_type, "contains");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_autocomplete_empty_prefix() {
    let dir = temp_dir("wiki_auto_empty");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let suggestions = model.autocomplete("", 10);
    assert!(suggestions.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_autocomplete_limit() {
    let dir = temp_dir("wiki_auto_limit");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("lib.rs").unwrap();
    let s1 = sm.register_string("alpha_fn").unwrap();
    let s2 = sm.register_string("alpha_test").unwrap();
    let s3 = sm.register_string("alpha_main").unwrap();
    sm.put_file_snapshot("lib.rs", &[
        VcTriple { subject_hash: fh, predicate_id: 1, object_hash: s1 },
        VcTriple { subject_hash: fh, predicate_id: 1, object_hash: s2 },
        VcTriple { subject_hash: fh, predicate_id: 1, object_hash: s3 },
    ]).unwrap();
    let model = build_wiki(&sm);
    let suggestions = model.autocomplete("alpha", 2);
    assert_eq!(suggestions.len(), 2, "should limit to 2");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel fuzzy_search tests ────────────────────────────────────────────

#[test]
fn wiki_fuzzy_search_subsequence() {
    let dir = temp_dir("wiki_fuzzy_sub");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("lib.rs").unwrap();
    let sh = sm.register_string("merkle_verify").unwrap();
    sm.put_file_snapshot("lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    // "mkv" is a subsequence of "merkle_verify" (m...k...v...)
    let results = model.fuzzy_search("mkv");
    assert!(!results.is_empty(), "fuzzy search should find subsequence match");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_fuzzy_search_empty() {
    let dir = temp_dir("wiki_fuzzy_empty");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let results = model.fuzzy_search("");
    assert!(results.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_fuzzy_search_no_match() {
    let dir = temp_dir("wiki_fuzzy_nomatch");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("lib.rs").unwrap();
    let sh = sm.register_string("abc").unwrap();
    sm.put_file_snapshot("lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    // "xyz" is not a subsequence of any page title
    let results = model.fuzzy_search("xyz");
    assert!(results.is_empty(), "no page should match 'xyz' subsequence");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiModel search_report tests ───────────────────────────────────────────

#[test]
fn wiki_search_report_structure() {
    let dir = temp_dir("wiki_report");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("lib.rs").unwrap();
    let sh = sm.register_string("test_func").unwrap();
    sm.put_file_snapshot("lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let report = model.search_report("test");
    assert_eq!(report.query, "test");
    assert!(report.total_matches > 0);
    assert!(report.top_score > 0);
    assert!(report.average_score > 0.0);
    assert!(!report.results_by_kind.is_empty());
    assert!(!report.top_results.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_search_report_no_matches() {
    let dir = temp_dir("wiki_report_empty");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let report = model.search_report("nonexistent");
    assert_eq!(report.total_matches, 0);
    assert_eq!(report.top_score, 0);
    assert_eq!(report.average_score, 0.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_search_report_serializes() {
    let dir = temp_dir("wiki_report_ser");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let report = model.search_report("test");
    let json = serde_json::to_string(&report).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["query"], "test");
    assert!(parsed["total_matches"].is_number());
    assert!(parsed["elapsed_us"].is_number());
    let _ = std::fs::remove_dir_all(&dir);
}

// ── WikiPage struct tests ───────────────────────────────────────────────────

#[test]
fn wiki_page_clone_and_debug() {
    let page = WikiPage {
        kind: WikiPageKind::File,
        title: "test.rs".into(),
        slug: "test-rs".into(),
        summary: "A test file".into(),
        relationships: vec![("Defines".into(), vec!["func".into()])],
        called_by: vec![],
        detail: Some("Detailed info".into()),
    };
    let cloned = page.clone();
    assert_eq!(cloned.title, "test.rs");
    assert_eq!(cloned.kind, WikiPageKind::File);
    let debug = format!("{:?}", page);
    assert!(debug.contains("test.rs"));
    assert!(debug.contains("WikiPage"));
}

#[test]
fn wiki_page_serializes() {
    let page = WikiPage {
        kind: WikiPageKind::Symbol,
        title: "my_fn".into(),
        slug: "my_fn".into(),
        summary: "A function".into(),
        relationships: vec![],
        called_by: vec!["other_fn".into()],
        detail: None,
    };
    let json = serde_json::to_string(&page).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["title"], "my_fn");
    assert_eq!(parsed["kind"], "Symbol");
    assert!(parsed["called_by"].as_array().unwrap().len() == 1);
    assert!(parsed["detail"].is_null());
}

// ── render_page_markdown tests ──────────────────────────────────────────────

#[test]
fn render_page_markdown_overview() {
    let dir = temp_dir("wiki_render_overview");
    let sm = SiteMap::open(&dir, 0).unwrap();
    let model = build_wiki(&sm);
    let page = WikiPage {
        kind: WikiPageKind::Overview,
        title: "Overview".into(),
        slug: "overview".into(),
        summary: "Project overview".into(),
        relationships: vec![],
        called_by: vec![],
        detail: Some("This is the project overview.".into()),
    };
    let md = render_page_markdown(&page, &model);
    assert!(md.contains("# Overview"));
    assert!(md.contains("overview"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_page_markdown_with_relationships() {
    let dir = temp_dir("wiki_render_rels");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let sh = sm.register_string("func_a").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let page = model.find_by_title("src/lib.rs").unwrap().clone();
    let md = render_page_markdown(&page, &model);
    assert!(md.contains("src/lib.rs"));
    assert!(md.contains("Defines"));
    assert!(md.contains("func_a"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Multi-file wiki tests ───────────────────────────────────────────────────

#[test]
fn wiki_multiple_files_and_symbols() {
    let dir = temp_dir("wiki_multi");
    let mut sm = SiteMap::open(&dir, 0).unwrap();

    let f1 = "src/lib.rs";
    let f2 = "src/main.rs";
    let s1 = "helper";
    let s2 = "run_app";
    let f1h = sm.register_string(f1).unwrap();
    let f2h = sm.register_string(f2).unwrap();
    let s1h = sm.register_string(s1).unwrap();
    let s2h = sm.register_string(s2).unwrap();

    sm.put_file_snapshot(f1, &[VcTriple {
        subject_hash: f1h, predicate_id: 1, object_hash: s1h,
    }]).unwrap();
    sm.put_file_snapshot(f2, &[VcTriple {
        subject_hash: f2h, predicate_id: 1, object_hash: s2h,
    }]).unwrap();

    let model = build_wiki(&sm);
    assert_eq!(model.file_count(), 2);
    assert_eq!(model.symbol_count(), 2);
    assert_eq!(model.total_pages(), 5); // overview + 2 files + 2 symbols

    let warnings = model.validate();
    assert!(warnings.is_empty(), "multi-file wiki should be valid: {:?}", warnings);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_files_referencing_nonexistent_symbol() {
    let dir = temp_dir("wiki_ref_nonexist");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let sh = sm.register_string("func").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let files = model.files_referencing("nonexistent_symbol");
    assert!(files.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiki_symbols_defined_by_nonexistent_file() {
    let dir = temp_dir("wiki_symdef_nonexist");
    let mut sm = SiteMap::open(&dir, 0).unwrap();
    let fh = sm.register_string("src/lib.rs").unwrap();
    let sh = sm.register_string("func").unwrap();
    sm.put_file_snapshot("src/lib.rs", &[VcTriple {
        subject_hash: fh, predicate_id: 1, object_hash: sh,
    }]).unwrap();
    let model = build_wiki(&sm);
    let syms = model.symbols_defined_by("nonexistent.rs");
    assert!(syms.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
