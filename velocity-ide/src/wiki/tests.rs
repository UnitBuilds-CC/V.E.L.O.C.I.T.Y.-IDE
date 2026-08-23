//! Integration tests for wiki generation and Markdown export.

use crate::site_map::{SiteMap, VcTriple};
use crate::wiki::markdown::slugify_module;
use crate::wiki::{build_wiki, export_markdown};

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
