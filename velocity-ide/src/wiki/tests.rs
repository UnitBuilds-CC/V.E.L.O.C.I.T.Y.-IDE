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
