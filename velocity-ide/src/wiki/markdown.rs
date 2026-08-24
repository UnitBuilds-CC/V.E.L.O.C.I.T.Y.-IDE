//! Renders a [`WikiModel`] to interlinked Markdown files.
//!
//! Qodo-style features:
//! - Table of contents on the index page
//! - Mermaid dependency graph for cross-references
//! - Module-level grouping (files grouped by directory)
//! - Navigation breadcrumbs on each page
//! - Alphabetical symbol index

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Serialize;

use super::generate::{WikiModel, WikiPage, WikiPageKind};

// ─── Markdown export diagnostics ──────────────────────────────────────────

/// Report from a markdown export operation.
#[derive(Debug, Clone, Serialize)]
pub struct MarkdownExportReport {
    pub elapsed_us: u64,
    pub pages_written: usize,
    pub total_bytes: usize,
    pub index_bytes: usize,
    pub symbol_index_bytes: usize,
    pub graph_bytes: usize,
    pub modules_exported: usize,
    pub validation_issues: Vec<String>,
}

/// Diagnostic info about the markdown rendering configuration.
#[derive(Debug, Clone, Serialize)]
pub struct MarkdownRenderInfo {
    pub max_graph_nodes: usize,
    pub has_files: bool,
    pub has_symbols: bool,
    pub module_count: usize,
    pub total_pages: usize,
}

/// Get diagnostic info about the rendering configuration for a model.
pub fn render_info(model: &WikiModel) -> MarkdownRenderInfo {
    let modules = group_by_module(&model.file_pages);
    MarkdownRenderInfo {
        max_graph_nodes: 50,
        has_files: !model.file_pages.is_empty(),
        has_symbols: !model.symbol_pages.is_empty(),
        module_count: modules.len(),
        total_pages: model.total_pages(),
    }
}

/// Export wiki to markdown with a detailed timing report.
pub fn export_markdown_reported(
    model: &WikiModel, dir: &Path,
) -> Result<(usize, MarkdownExportReport)> {
    let start = Instant::now();
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::create_dir_all(dir.join("files"))?;
    fs::create_dir_all(dir.join("symbols"))?;

    let mut count = 0usize;
    let mut total_bytes = 0usize;

    let index_md = render_index(model);
    fs::write(dir.join("index.md"), &index_md)?;
    let index_bytes = index_md.len();
    total_bytes += index_bytes;
    count += 1;

    let modules = group_by_module(&model.file_pages);
    let module_count = modules.len();
    for (module, pages) in &modules {
        let module_slug = slugify_module(module);
        let module_dir = dir.join("files").join(&module_slug);
        fs::create_dir_all(&module_dir)?;
        for page in pages {
            let path = module_dir.join(format!("{}.md", page.slug));
            let md = render_page_markdown(page, model);
            fs::write(&path, &md)?;
            total_bytes += md.len();
            count += 1;
        }
    }

    for page in &model.symbol_pages {
        let path = dir.join("symbols").join(format!("{}.md", page.slug));
        let md = render_page_markdown(page, model);
        fs::write(&path, &md)?;
        total_bytes += md.len();
        count += 1;
    }

    let sym_idx = render_symbol_index(model);
    fs::write(dir.join("symbol_index.md"), &sym_idx)?;
    let symbol_index_bytes = sym_idx.len();
    total_bytes += symbol_index_bytes;
    count += 1;

    let graph = render_dependency_graph(model);
    fs::write(dir.join("graph.md"), &graph)?;
    let graph_bytes = graph.len();
    total_bytes += graph_bytes;
    count += 1;

    let elapsed = start.elapsed().as_micros() as u64;
    let mut issues = Vec::new();
    if model.is_empty() {
        issues.push("Wiki model is empty".to_string());
    }

    let report = MarkdownExportReport {
        elapsed_us: elapsed,
        pages_written: count,
        total_bytes,
        index_bytes,
        symbol_index_bytes,
        graph_bytes,
        modules_exported: module_count,
        validation_issues: issues,
    };
    Ok((count, report))
}

/// Export the whole wiki to `dir` as Markdown. Returns the number of pages
/// written (overview + files + symbols + index pages).
pub fn export_markdown(model: &WikiModel, dir: &Path) -> Result<usize> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::create_dir_all(dir.join("files"))?;
    fs::create_dir_all(dir.join("symbols"))?;

    let mut count = 0usize;

    fs::write(dir.join("index.md"), render_index(model))?;
    count += 1;

    // Write module-grouped file index pages
    let modules = group_by_module(&model.file_pages);
    for (module, pages) in &modules {
        let module_slug = slugify_module(module);
        let module_dir = dir.join("files").join(&module_slug);
        fs::create_dir_all(&module_dir)?;
        for page in pages {
            let path = module_dir.join(format!("{}.md", page.slug));
            fs::write(&path, render_page_markdown(page, model))?;
            count += 1;
        }
    }

    for page in &model.symbol_pages {
        let path = dir.join("symbols").join(format!("{}.md", page.slug));
        fs::write(&path, render_page_markdown(page, model))?;
        count += 1;
    }

    // Write alphabetical symbol index
    fs::write(dir.join("symbol_index.md"), render_symbol_index(model))?;
    count += 1;

    // Write dependency graph
    fs::write(dir.join("graph.md"), render_dependency_graph(model))?;
    count += 1;

    Ok(count)
}

fn generated_header(model: &WikiModel) -> String {
    format!(
        "_Generated from the Velocity site map \u{00b7} {} \u{00b7} {}_\n",
        model.generated_at, model.stats_summary
    )
}

fn render_index(model: &WikiModel) -> String {
    let mut out = String::new();
    out.push_str("# Project Wiki\n\n");
    out.push_str(&generated_header(model));
    out.push('\n');

    // Table of Contents
    out.push_str("## Table of Contents\n\n");
    out.push_str("- [Overview](#overview)\n");
    if !model.file_pages.is_empty() {
        out.push_str("- [Files](#files)\n");
        let modules = group_by_module(&model.file_pages);
        for module in modules.keys() {
            out.push_str(&format!(
                "  - [{}](#module-{})\n",
                module,
                slugify_module(module)
            ));
        }
    }
    if !model.symbol_pages.is_empty() {
        out.push_str("- [Symbols](#symbols)\n");
    }
    out.push_str("- [Dependency Graph](graph.md)\n");
    out.push_str("- [Symbol Index](symbol_index.md)\n");
    out.push('\n');

    // Overview
    out.push_str("## Overview\n\n");
    out.push_str(&model.overview.summary);
    out.push_str("\n\n");

    // Files grouped by module
    if !model.file_pages.is_empty() {
        out.push_str("## Files\n\n");
        let modules = group_by_module(&model.file_pages);
        for (module, pages) in &modules {
            out.push_str(&format!("### Module: {}\n\n", module));
            for page in pages {
                out.push_str(&format!(
                    "- [{}](files/{}/{}.md) \u{2014} {}\n",
                    page.title,
                    slugify_module(module),
                    page.slug,
                    page.summary
                ));
            }
            out.push('\n');
        }
    }

    // Symbols
    if !model.symbol_pages.is_empty() {
        out.push_str("## Symbols\n\n");
        for page in &model.symbol_pages {
            out.push_str(&format!(
                "- [{}](symbols/{}.md) \u{2014} {}\n",
                page.title, page.slug, page.summary
            ));
        }
        out.push('\n');
    }

    out
}

/// Render a single page to Markdown with breadcrumbs and cross-links.
pub fn render_page_markdown(page: &WikiPage, model: &WikiModel) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", page.title));

    // Breadcrumb navigation
    let kind_label = page.kind.label();
    out.push_str(&format!(
        "> [Wiki](../index.md) > {} > {}\n\n",
        kind_label, page.title
    ));
    out.push_str(&generated_header(model));
    out.push('\n');
    out.push_str(&page.summary);
    out.push_str("\n\n");

    // Page-level TOC
    out.push_str("## Contents\n\n");
    if !page.relationships.is_empty() {
        for (label, _) in &page.relationships {
            out.push_str(&format!("- [{}](#-)\n", label));
        }
    }
    if !page.called_by.is_empty() {
        out.push_str("- [Called By](#called-by)\n");
    }
    out.push('\n');

    if let Some(detail) = page.detail.as_deref() {
        out.push_str("## Details\n\n");
        out.push_str(detail.trim_end());
        out.push_str("\n\n");
    }

    for (label, targets) in &page.relationships {
        if targets.is_empty() {
            continue;
        }
        out.push_str(&format!("## {}\n\n", label));
        for target in targets {
            out.push_str(&format!("- {}\n", link_for(target, model)));
        }
        out.push('\n');
    }

    if !page.called_by.is_empty() {
        out.push_str("## Called By\n\n");
        for caller in &page.called_by {
            out.push_str(&format!("- {}\n", link_for(caller, model)));
        }
        out.push('\n');
    }

    out
}

/// Render an alphabetical symbol index page.
fn render_symbol_index(model: &WikiModel) -> String {
    let mut out = String::new();
    out.push_str("# Symbol Index\n\n");
    out.push_str(&generated_header(model));
    out.push('\n');

    // Group by first letter
    let mut by_letter: BTreeMap<char, Vec<&WikiPage>> = BTreeMap::new();
    for page in &model.symbol_pages {
        let first = page
            .title
            .chars()
            .next()
            .unwrap_or('#')
            .to_ascii_uppercase();
        by_letter.entry(first).or_default().push(page);
    }

    // Letter navigation bar
    out.push_str("**Letters:** ");
    let letters: Vec<char> = by_letter.keys().cloned().collect();
    for (i, letter) in letters.iter().enumerate() {
        if i > 0 {
            out.push_str(" \u{00b7} ");
        }
        out.push_str(&format!("[{}](#{})", letter, letter.to_ascii_lowercase()));
    }
    out.push_str("\n\n");

    for (letter, pages) in &by_letter {
        out.push_str(&format!("## {}\n\n", letter));
        for page in pages {
            out.push_str(&format!(
                "- [{}](symbols/{}.md) \u{2014} {}\n",
                page.title, page.slug, page.summary
            ));
        }
        out.push('\n');
    }

    out
}

/// Render a Mermaid dependency graph of all pages with relationships.
fn render_dependency_graph(model: &WikiModel) -> String {
    let mut out = String::new();
    out.push_str("# Dependency Graph\n\n");
    out.push_str(&generated_header(model));
    out.push('\n');
    out.push_str("```mermaid\ngraph LR\n");

    // Add nodes for files and symbols (limit to first 50 to keep graph readable)
    let max_nodes = 50;
    let mut node_count = 0;
    let mut node_ids: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for page in model.file_pages.iter().chain(model.symbol_pages.iter()) {
        if node_count >= max_nodes {
            break;
        }
        let id = format!("N{}", node_count);
        let label = if page.title.len() > 20 {
            format!("{}...", &page.title[..17])
        } else {
            page.title.clone()
        };
        node_ids.insert(page.title.clone(), id.clone());
        let shape = match page.kind {
            WikiPageKind::File => format!("{}[\"{}\"]", id, label),
            WikiPageKind::Symbol => format!("{}({})", id, label),
            WikiPageKind::Overview => continue,
        };
        out.push_str(&format!("    {}\n", shape));
        node_count += 1;
    }

    // Add edges for relationships
    for page in model.file_pages.iter().chain(model.symbol_pages.iter()) {
        if let Some(from_id) = node_ids.get(&page.title) {
            for (label, targets) in &page.relationships {
                for target in targets {
                    if let Some(to_id) = node_ids.get(target) {
                        let edge_label = if label == "Calls" { "calls" } else { "defines" };
                        out.push_str(&format!("    {} -->|{}| {}\n", from_id, edge_label, to_id));
                    }
                }
            }
        }
    }

    out.push_str("```\n");
    out
}

/// Group file pages by their top-level directory module.
fn group_by_module(pages: &[WikiPage]) -> BTreeMap<String, Vec<&WikiPage>> {
    let mut modules: BTreeMap<String, Vec<&WikiPage>> = BTreeMap::new();
    for page in pages {
        let module = page.title.split('/').next().unwrap_or("root").to_string();
        let module = if module.contains('.') || module.is_empty() {
            "root".to_string()
        } else {
            module
        };
        modules.entry(module).or_default().push(page);
    }
    modules
}

pub fn slugify_module(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Build a relative Markdown link to a target page if one exists, otherwise
/// return the plain name.
fn link_for(target: &str, model: &WikiModel) -> String {
    if let Some(page) = find_page(model, target) {
        let rel = match page.kind {
            WikiPageKind::File => {
                // Use module-aware path for file pages
                let module = page.title.split('/').next().unwrap_or("root");
                let module = if module.contains('.') || module.is_empty() {
                    "root"
                } else {
                    module
                };
                format!("../files/{}/{}.md", slugify_module(module), page.slug)
            }
            WikiPageKind::Symbol => format!("../symbols/{}.md", page.slug),
            WikiPageKind::Overview => "../index.md".to_string(),
        };
        format!("[{}]({})", target, rel)
    } else {
        target.to_string()
    }
}

fn find_page<'a>(model: &'a WikiModel, title: &str) -> Option<&'a WikiPage> {
    model
        .file_pages
        .iter()
        .find(|p| p.title == title)
        .or_else(|| model.symbol_pages.iter().find(|p| p.title == title))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::generate::*;

    fn make_page(kind: WikiPageKind, title: &str, slug: &str) -> WikiPage {
        WikiPage {
            kind,
            title: title.to_string(),
            slug: slug.to_string(),
            summary: format!("Summary for {title}"),
            relationships: vec![],
            called_by: vec![],
            detail: None,
        }
    }

    fn make_model() -> WikiModel {
        let overview = make_page(WikiPageKind::Overview, "Overview", "index");
        let mut file1 = make_page(WikiPageKind::File, "src/main.rs", "src-main-rs");
        file1.relationships = vec![("Defines".to_string(), vec!["main_fn".to_string()])];

        let mut file2 = make_page(WikiPageKind::File, "src/lib.rs", "src-lib-rs");
        file2.relationships = vec![
            ("Defines".to_string(), vec!["helper_fn".to_string()]),
            ("Calls".to_string(), vec!["main_fn".to_string()]),
        ];

        let mut sym1 = make_page(WikiPageKind::Symbol, "main_fn", "main_fn");
        sym1.called_by = vec!["src/main.rs".to_string()];
        sym1.detail = Some("The main entry point.".to_string());

        let mut sym2 = make_page(WikiPageKind::Symbol, "helper_fn", "helper_fn");
        sym2.called_by = vec!["src/lib.rs".to_string()];

        let orphan = make_page(WikiPageKind::Symbol, "unused_fn", "unused_fn");

        WikiModel {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            stats_summary: "5 pages, 3 relationships".to_string(),
            overview,
            file_pages: vec![file1, file2],
            symbol_pages: vec![sym1, sym2, orphan],
        }
    }

    fn empty_model() -> WikiModel {
        WikiModel {
            generated_at: "2024-01-01T00:00:00Z".to_string(),
            stats_summary: "0 pages".to_string(),
            overview: make_page(WikiPageKind::Overview, "Overview", "index"),
            file_pages: vec![],
            symbol_pages: vec![],
        }
    }

    // ── render_info ──────────────────────────────────────────────────────

    #[test]
    fn render_info_empty_model() {
        let model = empty_model();
        let info = render_info(&model);
        assert_eq!(info.total_pages, 1); // overview only
        assert_eq!(info.module_count, 0);
        assert!(!info.has_files);
        assert!(!info.has_symbols);
        assert_eq!(info.max_graph_nodes, 50);
    }

    #[test]
    fn render_info_populated_model() {
        let model = make_model();
        let info = render_info(&model);
        assert_eq!(info.total_pages, 6); // 1 overview + 2 files + 3 symbols
        assert!(info.has_files);
        assert!(info.has_symbols);
        assert_eq!(info.module_count, 1); // all files under "src"
    }

    #[test]
    fn render_info_serializes() {
        let model = make_model();
        let info = render_info(&model);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"has_files\":true"));
        assert!(json.contains("\"has_symbols\":true"));
        assert!(json.contains("\"max_graph_nodes\":50"));
    }

    // ── slugify_module ───────────────────────────────────────────────────

    #[test]
    fn slugify_module_basic() {
        assert_eq!(slugify_module("src"), "src");
        assert_eq!(slugify_module("compiler"), "compiler");
    }

    #[test]
    fn slugify_module_special_chars() {
        assert_eq!(slugify_module("my-module"), "my-module");
        assert_eq!(slugify_module("my_module"), "my-module");
        assert_eq!(slugify_module("path/to"), "path-to");
    }

    #[test]
    fn slugify_module_trims_dashes() {
        assert_eq!(slugify_module("-leading-"), "leading");
        assert_eq!(slugify_module("--double--"), "double");
    }

    #[test]
    fn slugify_module_uppercase() {
        assert_eq!(slugify_module("SRC"), "src");
        assert_eq!(slugify_module("MyModule"), "mymodule");
    }

    // ── group_by_module ──────────────────────────────────────────────────

    #[test]
    fn group_by_module_empty() {
        let groups = group_by_module(&[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn group_by_module_single_module() {
        let pages = vec![
            make_page(WikiPageKind::File, "src/main.rs", "a"),
            make_page(WikiPageKind::File, "src/lib.rs", "b"),
        ];
        let groups = group_by_module(&pages);
        assert_eq!(groups.len(), 1);
        assert!(groups.contains_key("src"));
        assert_eq!(groups["src"].len(), 2);
    }

    #[test]
    fn group_by_module_multiple_modules() {
        let pages = vec![
            make_page(WikiPageKind::File, "src/main.rs", "a"),
            make_page(WikiPageKind::File, "compiler/driver.rs", "b"),
            make_page(WikiPageKind::File, "compiler/jit.rs", "c"),
        ];
        let groups = group_by_module(&pages);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups["src"].len(), 1);
        assert_eq!(groups["compiler"].len(), 2);
    }

    #[test]
    fn group_by_module_bare_files_go_to_root() {
        let pages = vec![
            make_page(WikiPageKind::File, "main.rs", "a"),
            make_page(WikiPageKind::File, "lib.rs", "b"),
        ];
        let groups = group_by_module(&pages);
        // "main.rs" contains '.', so goes to "root"
        assert!(groups.contains_key("root"));
    }

    // ── render_page_markdown ─────────────────────────────────────────────

    #[test]
    fn render_page_has_title() {
        let model = make_model();
        let page = &model.file_pages[0];
        let md = render_page_markdown(page, &model);
        assert!(md.starts_with("# src/main.rs\n"));
    }

    #[test]
    fn render_page_has_breadcrumbs() {
        let model = make_model();
        let page = &model.file_pages[0];
        let md = render_page_markdown(page, &model);
        assert!(md.contains("> [Wiki](../index.md) > File > src/main.rs"));
    }

    #[test]
    fn render_page_has_generated_header() {
        let model = make_model();
        let page = &model.file_pages[0];
        let md = render_page_markdown(page, &model);
        assert!(md.contains("Generated from the Velocity site map"));
        assert!(md.contains("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn render_page_has_summary() {
        let model = make_model();
        let page = &model.file_pages[0];
        let md = render_page_markdown(page, &model);
        assert!(md.contains("Summary for src/main.rs"));
    }

    #[test]
    fn render_page_has_relationships() {
        let model = make_model();
        let page = &model.file_pages[0];
        let md = render_page_markdown(page, &model);
        assert!(md.contains("## Defines"));
        // main_fn is a known symbol, so it should be a link
        assert!(md.contains("[main_fn]("));
    }

    #[test]
    fn render_page_has_called_by() {
        let model = make_model();
        let page = &model.symbol_pages[0]; // main_fn
        let md = render_page_markdown(page, &model);
        assert!(md.contains("## Called By"));
        assert!(md.contains("src/main.rs"));
    }

    #[test]
    fn render_page_has_detail_section() {
        let model = make_model();
        let page = &model.symbol_pages[0]; // main_fn has detail
        let md = render_page_markdown(page, &model);
        assert!(md.contains("## Details"));
        assert!(md.contains("The main entry point."));
    }

    #[test]
    fn render_page_no_detail_when_absent() {
        let model = make_model();
        let page = &model.symbol_pages[1]; // helper_fn has no detail
        let md = render_page_markdown(page, &model);
        assert!(!md.contains("## Details"));
    }

    #[test]
    fn render_page_cross_links_to_symbols() {
        let model = make_model();
        let page = &model.file_pages[0]; // src/main.rs defines main_fn
        let md = render_page_markdown(page, &model);
        // main_fn should be linked to its symbol page
        assert!(md.contains("[main_fn](../symbols/main_fn.md)"));
    }

    #[test]
    fn render_page_cross_links_to_files() {
        let model = make_model();
        let page = &model.symbol_pages[0]; // main_fn called by src/main.rs
        let md = render_page_markdown(page, &model);
        // src/main.rs should be linked to its file page
        assert!(md.contains("[src/main.rs]("));
    }

    #[test]
    fn render_page_unknown_target_is_plain_text() {
        let model = make_model();
        let mut page = make_page(WikiPageKind::File, "test.rs", "test-rs");
        page.relationships = vec![("Calls".to_string(), vec!["nonexistent_fn".to_string()])];
        let md = render_page_markdown(&page, &model);
        // nonexistent_fn is not in the model, so just plain text
        assert!(md.contains("- nonexistent_fn\n"));
        assert!(!md.contains("[nonexistent_fn]"));
    }

    // ── export_markdown ──────────────────────────────────────────────────

    #[test]
    fn export_markdown_creates_files() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        let count = export_markdown(&model, dir.path()).unwrap();
        // 1 index + 2 file pages + 3 symbol pages + 1 symbol_index + 1 graph = 8
        assert_eq!(count, 8);
        assert!(dir.path().join("index.md").exists());
        assert!(dir.path().join("symbol_index.md").exists());
        assert!(dir.path().join("graph.md").exists());
    }

    #[test]
    fn export_markdown_creates_module_dirs() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&model, dir.path()).unwrap();
        // Files should be under files/src/
        assert!(dir.path().join("files").join("src").exists());
    }

    #[test]
    fn export_markdown_symbol_files() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&model, dir.path()).unwrap();
        assert!(dir.path().join("symbols").join("main_fn.md").exists());
        assert!(dir.path().join("symbols").join("helper_fn.md").exists());
        assert!(dir.path().join("symbols").join("unused_fn.md").exists());
    }

    // ── export_markdown_reported ─────────────────────────────────────────

    #[test]
    fn export_reported_returns_correct_count() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        let (count, report) = export_markdown_reported(&model, dir.path()).unwrap();
        assert_eq!(count, 8);
        assert_eq!(report.pages_written, 8);
    }

    #[test]
    fn export_reported_has_timing() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        let (_, report) = export_markdown_reported(&model, dir.path()).unwrap();
        // elapsed_us should be non-negative (can be 0 on fast machines)
        assert!(report.elapsed_us < 10_000_000); // sanity: under 10 seconds
    }

    #[test]
    fn export_reported_tracks_bytes() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        let (_, report) = export_markdown_reported(&model, dir.path()).unwrap();
        assert!(report.total_bytes > 0);
        assert!(report.index_bytes > 0);
        assert!(report.symbol_index_bytes > 0);
        assert!(report.graph_bytes > 0);
    }

    #[test]
    fn export_reported_tracks_modules() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        let (_, report) = export_markdown_reported(&model, dir.path()).unwrap();
        assert_eq!(report.modules_exported, 1); // only "src"
    }

    #[test]
    fn export_reported_no_validation_issues_for_populated_model() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        let (_, report) = export_markdown_reported(&model, dir.path()).unwrap();
        assert!(report.validation_issues.is_empty());
    }

    #[test]
    fn export_reported_flags_empty_model() {
        let model = empty_model();
        let dir = tempfile::tempdir().unwrap();
        let (count, report) = export_markdown_reported(&model, dir.path()).unwrap();
        // 1 index + 1 symbol_index + 1 graph = 3
        assert_eq!(count, 3);
        assert_eq!(report.pages_written, 3);
        assert!(!report.validation_issues.is_empty());
        assert!(report.validation_issues[0].contains("empty"));
    }

    #[test]
    fn export_reported_serializes() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        let (_, report) = export_markdown_reported(&model, dir.path()).unwrap();
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"pages_written\":8"));
        assert!(json.contains("\"modules_exported\":1"));
        assert!(json.contains("\"elapsed_us\""));
        assert!(json.contains("\"total_bytes\""));
    }

    // ── render_index (via export) ────────────────────────────────────────

    #[test]
    fn index_contains_table_of_contents() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&model, dir.path()).unwrap();
        let index = fs::read_to_string(dir.path().join("index.md")).unwrap();
        assert!(index.contains("## Table of Contents"));
        assert!(index.contains("- [Overview](#overview)"));
        assert!(index.contains("- [Files](#files)"));
        assert!(index.contains("- [Symbols](#symbols)"));
    }

    #[test]
    fn index_contains_module_links() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&model, dir.path()).unwrap();
        let index = fs::read_to_string(dir.path().join("index.md")).unwrap();
        assert!(index.contains("### Module: src"));
    }

    #[test]
    fn index_contains_overview_summary() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&model, dir.path()).unwrap();
        let index = fs::read_to_string(dir.path().join("index.md")).unwrap();
        assert!(index.contains("Summary for Overview"));
    }

    // ── render_symbol_index (via export) ─────────────────────────────────

    #[test]
    fn symbol_index_groups_by_letter() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&model, dir.path()).unwrap();
        let sym_idx = fs::read_to_string(dir.path().join("symbol_index.md")).unwrap();
        assert!(sym_idx.contains("## H")); // helper_fn
        assert!(sym_idx.contains("## M")); // main_fn
        assert!(sym_idx.contains("## U")); // unused_fn
    }

    #[test]
    fn symbol_index_has_letter_navigation() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&model, dir.path()).unwrap();
        let sym_idx = fs::read_to_string(dir.path().join("symbol_index.md")).unwrap();
        assert!(sym_idx.contains("**Letters:**"));
        assert!(sym_idx.contains("[H](#h)"));
        assert!(sym_idx.contains("[M](#m)"));
    }

    // ── render_dependency_graph (via export) ─────────────────────────────

    #[test]
    fn graph_contains_mermaid_block() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&model, dir.path()).unwrap();
        let graph = fs::read_to_string(dir.path().join("graph.md")).unwrap();
        assert!(graph.contains("```mermaid"));
        assert!(graph.contains("graph LR"));
    }

    #[test]
    fn graph_contains_edges() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&model, dir.path()).unwrap();
        let graph = fs::read_to_string(dir.path().join("graph.md")).unwrap();
        // src/main.rs defines main_fn
        assert!(graph.contains("-->|defines|"));
    }

    #[test]
    fn graph_truncates_large_models() {
        // Create a model with many pages to test the 50-node limit
        let overview = make_page(WikiPageKind::Overview, "Overview", "index");
        let mut files = Vec::new();
        for i in 0..60 {
            files.push(make_page(
                WikiPageKind::File,
                &format!("module/file_{}.rs", i),
                &format!("file-{}", i),
            ));
        }
        let model = WikiModel {
            generated_at: "test".to_string(),
            stats_summary: "test".to_string(),
            overview,
            file_pages: files,
            symbol_pages: vec![],
        };
        let dir = tempfile::tempdir().unwrap();
        export_markdown(&model, dir.path()).unwrap();
        let graph = fs::read_to_string(dir.path().join("graph.md")).unwrap();
        // Count node definitions (lines with N##[ or N##()
        let node_lines: Vec<&str> = graph
            .lines()
            .filter(|l| l.contains("[\"") || l.contains("(\""))
            .collect();
        assert!(node_lines.len() <= 50);
    }

    // ── MarkdownExportReport ─────────────────────────────────────────────

    #[test]
    fn export_report_struct_fields() {
        let report = MarkdownExportReport {
            elapsed_us: 1234,
            pages_written: 10,
            total_bytes: 5000,
            index_bytes: 500,
            symbol_index_bytes: 300,
            graph_bytes: 200,
            modules_exported: 3,
            validation_issues: vec!["test issue".to_string()],
        };
        assert_eq!(report.elapsed_us, 1234);
        assert_eq!(report.pages_written, 10);
        assert_eq!(report.total_bytes, 5000);
        assert_eq!(report.modules_exported, 3);
        assert_eq!(report.validation_issues.len(), 1);
    }

    // ── MarkdownRenderInfo ───────────────────────────────────────────────

    #[test]
    fn render_info_struct_fields() {
        let info = MarkdownRenderInfo {
            max_graph_nodes: 25,
            has_files: true,
            has_symbols: false,
            module_count: 4,
            total_pages: 100,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"max_graph_nodes\":25"));
        assert!(json.contains("\"has_files\":true"));
        assert!(json.contains("\"has_symbols\":false"));
        assert!(json.contains("\"module_count\":4"));
        assert!(json.contains("\"total_pages\":100"));
    }

    // ── Edge cases ───────────────────────────────────────────────────────

    #[test]
    fn render_page_empty_relationships() {
        let model = make_model();
        let page = &model.symbol_pages[2]; // unused_fn, no relationships
        let md = render_page_markdown(page, &model);
        assert!(!md.contains("## Called By"));
        // Should still have title and summary
        assert!(md.contains("# unused_fn"));
    }

    #[test]
    fn render_page_empty_targets_skipped() {
        let mut page = make_page(WikiPageKind::File, "test.rs", "test-rs");
        page.relationships = vec![("Calls".to_string(), vec![])]; // empty targets
        let model = make_model();
        let md = render_page_markdown(&page, &model);
        // Empty target list should be skipped
        assert!(!md.contains("## Calls"));
    }

    #[test]
    fn export_reported_creates_directories() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("deep").join("nested").join("wiki");
        assert!(!nested.exists());
        export_markdown_reported(&model, &nested).unwrap();
        assert!(nested.join("index.md").exists());
        assert!(nested.join("files").exists());
        assert!(nested.join("symbols").exists());
    }

    // ── Block 138: Extended tests ─────────────────────────────────────────

    // --- render_page_markdown: breadcrumb variants ---

    #[test]
    fn render_page_symbol_breadcrumb() {
        let model = make_model();
        let page = &model.symbol_pages[0]; // main_fn
        let md = render_page_markdown(page, &model);
        assert!(md.contains("> [Wiki](../index.md) > Symbol > main_fn"));
    }

    #[test]
    fn render_page_overview_breadcrumb() {
        let model = make_model();
        let md = render_page_markdown(&model.overview, &model);
        assert!(md.contains("> [Wiki](../index.md) > Overview > Overview"));
    }

    // --- render_page_markdown: page-level TOC ---

    #[test]
    fn render_page_toc_lists_relationship_labels() {
        let model = make_model();
        let page = &model.file_pages[1]; // src/lib.rs has Defines + Calls
        let md = render_page_markdown(page, &model);
        assert!(md.contains("## Contents"));
        assert!(md.contains("- [Defines](#-)"));
        assert!(md.contains("- [Calls](#-)"));
    }

    #[test]
    fn render_page_toc_includes_called_by_when_present() {
        let model = make_model();
        let page = &model.symbol_pages[0]; // main_fn has called_by
        let md = render_page_markdown(page, &model);
        assert!(md.contains("- [Called By](#called-by)"));
    }

    #[test]
    fn render_page_toc_empty_when_no_relationships_or_calls() {
        let model = make_model();
        let page = &model.symbol_pages[2]; // unused_fn
        let md = render_page_markdown(page, &model);
        // Contents section exists but has no relationship entries
        assert!(md.contains("## Contents"));
        assert!(!md.contains("- [Called By]"));
    }

    // --- render_page_markdown: multiple relationship types ---

    #[test]
    fn render_page_multiple_relationship_types() {
        let mut page = make_page(WikiPageKind::File, "multi.rs", "multi");
        page.relationships = vec![
            ("Defines".to_string(), vec!["alpha".to_string(), "beta".to_string()]),
            ("Calls".to_string(), vec!["gamma".to_string()]),
        ];
        let model = make_model();
        let md = render_page_markdown(&page, &model);
        assert!(md.contains("## Defines"));
        assert!(md.contains("- alpha"));
        assert!(md.contains("- beta"));
        assert!(md.contains("## Calls"));
        assert!(md.contains("- gamma"));
    }

    // --- render_page_markdown: detail trimming ---

    #[test]
    fn render_page_detail_trims_trailing_whitespace() {
        let mut page = make_page(WikiPageKind::Symbol, "trim_fn", "trim_fn");
        page.detail = Some("Some detail text.   \n\n".to_string());
        let model = make_model();
        let md = render_page_markdown(&page, &model);
        // trim_end() should remove trailing whitespace
        assert!(md.contains("## Details"));
        assert!(md.contains("Some detail text."));
    }

    // --- render_symbol_index ---

    #[test]
    fn symbol_index_empty_model() {
        let model = empty_model();
        let out = render_symbol_index(&model);
        assert!(out.contains("# Symbol Index"));
        assert!(out.contains("**Letters:**"));
        // No letter sections when no symbols
        assert!(!out.contains("## A"));
    }

    #[test]
    fn symbol_index_letters_sorted_order() {
        let model = make_model();
        let out = render_symbol_index(&model);
        // H (helper_fn), M (main_fn), U (unused_fn) — should be alphabetical
        let h_pos = out.find("## H").unwrap();
        let m_pos = out.find("## M").unwrap();
        let u_pos = out.find("## U").unwrap();
        assert!(h_pos < m_pos);
        assert!(m_pos < u_pos);
    }

    #[test]
    fn symbol_index_links_to_symbol_pages() {
        let model = make_model();
        let out = render_symbol_index(&model);
        assert!(out.contains("[main_fn](symbols/main_fn.md)"));
        assert!(out.contains("[helper_fn](symbols/helper_fn.md)"));
    }

    #[test]
    fn symbol_index_non_ascii_first_char() {
        let overview = make_page(WikiPageKind::Overview, "Overview", "index");
        let sym = make_page(WikiPageKind::Symbol, "über_fn", "uber_fn");
        let model = WikiModel {
            generated_at: "test".to_string(),
            stats_summary: "test".to_string(),
            overview,
            file_pages: vec![],
            symbol_pages: vec![sym],
        };
        let out = render_symbol_index(&model);
        // to_ascii_uppercase() only converts ASCII a-z, so 'ü' stays 'ü'
        assert!(out.contains("## ü"));
    }

    // --- render_dependency_graph ---

    #[test]
    fn graph_empty_model_has_mermaid_block() {
        let model = empty_model();
        let out = render_dependency_graph(&model);
        assert!(out.contains("```mermaid"));
        assert!(out.contains("graph LR"));
        assert!(out.contains("```"));
    }

    #[test]
    fn graph_file_nodes_use_rect_shape() {
        let model = make_model();
        let out = render_dependency_graph(&model);
        // File nodes use ["label"] shape
        assert!(out.contains("[\""));
    }

    #[test]
    fn graph_symbol_nodes_use_circle_shape() {
        let model = make_model();
        let out = render_dependency_graph(&model);
        // Symbol nodes use (label) shape
        assert!(out.contains("("));
    }

    #[test]
    fn graph_long_title_truncated_at_20() {
        let overview = make_page(WikiPageKind::Overview, "Overview", "index");
        let long_title = "a".repeat(25);
        let file = make_page(WikiPageKind::File, &long_title, "long");
        let model = WikiModel {
            generated_at: "test".to_string(),
            stats_summary: "test".to_string(),
            overview,
            file_pages: vec![file],
            symbol_pages: vec![],
        };
        let out = render_dependency_graph(&model);
        // Title > 20 chars gets truncated to first 17 + "..."
        assert!(out.contains("aaa..."));
    }

    #[test]
    fn graph_short_title_not_truncated() {
        let model = make_model();
        let out = render_dependency_graph(&model);
        // "src/main.rs" is 11 chars, should appear untruncated
        assert!(out.contains("src/main.rs"));
    }

    #[test]
    fn graph_calls_edge_label() {
        // Create a model where a file Calls a symbol that exists
        let overview = make_page(WikiPageKind::Overview, "Overview", "index");
        let mut file = make_page(WikiPageKind::File, "src/caller.rs", "caller");
        let sym = make_page(WikiPageKind::Symbol, "target_fn", "target_fn");
        file.relationships = vec![("Calls".to_string(), vec!["target_fn".to_string()])];
        let model = WikiModel {
            generated_at: "test".to_string(),
            stats_summary: "test".to_string(),
            overview,
            file_pages: vec![file],
            symbol_pages: vec![sym],
        };
        let out = render_dependency_graph(&model);
        assert!(out.contains("-->|calls|"));
    }

    #[test]
    fn graph_defines_edge_label() {
        let model = make_model();
        let out = render_dependency_graph(&model);
        assert!(out.contains("-->|defines|"));
    }

    // --- render_index ---

    #[test]
    fn index_no_files_omits_files_section() {
        let model = empty_model();
        let out = render_index(&model);
        assert!(!out.contains("## Files"));
    }

    #[test]
    fn index_no_symbols_omits_symbols_section() {
        let overview = make_page(WikiPageKind::Overview, "Overview", "index");
        let file = make_page(WikiPageKind::File, "src/main.rs", "main");
        let model = WikiModel {
            generated_at: "test".to_string(),
            stats_summary: "test".to_string(),
            overview,
            file_pages: vec![file],
            symbol_pages: vec![],
        };
        let out = render_index(&model);
        assert!(out.contains("## Files"));
        assert!(!out.contains("## Symbols"));
    }

    #[test]
    fn index_has_project_wiki_heading() {
        let model = make_model();
        let out = render_index(&model);
        assert!(out.starts_with("# Project Wiki\n"));
    }

    #[test]
    fn index_has_generated_header() {
        let model = make_model();
        let out = render_index(&model);
        assert!(out.contains("Generated from the Velocity site map"));
        assert!(out.contains("2024-01-01T00:00:00Z"));
        assert!(out.contains("5 pages, 3 relationships"));
    }

    #[test]
    fn index_has_dependency_graph_link() {
        let model = make_model();
        let out = render_index(&model);
        assert!(out.contains("- [Dependency Graph](graph.md)"));
    }

    #[test]
    fn index_has_symbol_index_link() {
        let model = make_model();
        let out = render_index(&model);
        assert!(out.contains("- [Symbol Index](symbol_index.md)"));
    }

    // --- link_for / find_page ---

    #[test]
    fn link_for_file_returns_module_aware_path() {
        let model = make_model();
        let link = link_for("src/main.rs", &model);
        assert!(link.contains("../files/src/src-main-rs.md"));
        assert!(link.starts_with("[src/main.rs]("));
    }

    #[test]
    fn link_for_symbol_returns_symbols_path() {
        let model = make_model();
        let link = link_for("main_fn", &model);
        assert_eq!(link, "[main_fn](../symbols/main_fn.md)");
    }

    #[test]
    fn link_for_unknown_returns_plain_text() {
        let model = make_model();
        let link = link_for("does_not_exist", &model);
        assert_eq!(link, "does_not_exist");
    }

    #[test]
    fn find_page_finds_file() {
        let model = make_model();
        let found = find_page(&model, "src/main.rs");
        assert!(found.is_some());
        assert_eq!(found.unwrap().slug, "src-main-rs");
    }

    #[test]
    fn find_page_finds_symbol() {
        let model = make_model();
        let found = find_page(&model, "helper_fn");
        assert!(found.is_some());
        assert_eq!(found.unwrap().slug, "helper_fn");
    }

    #[test]
    fn find_page_returns_none_for_unknown() {
        let model = make_model();
        assert!(find_page(&model, "nonexistent").is_none());
    }

    // --- group_by_module edge cases ---

    #[test]
    fn group_by_module_empty_title_goes_to_root() {
        let pages = vec![make_page(WikiPageKind::File, "", "empty")];
        let groups = group_by_module(&pages);
        assert!(groups.contains_key("root"));
    }

    #[test]
    fn group_by_module_single_char_module() {
        let pages = vec![make_page(WikiPageKind::File, "x/main.rs", "a")];
        let groups = group_by_module(&pages);
        assert!(groups.contains_key("x"));
    }

    // --- slugify_module edge cases ---

    #[test]
    fn slugify_module_empty_string() {
        assert_eq!(slugify_module(""), "");
    }

    #[test]
    fn slugify_module_all_special_chars() {
        let result = slugify_module("!!!@@@");
        assert_eq!(result, "");
    }

    #[test]
    fn slugify_module_numbers_preserved() {
        assert_eq!(slugify_module("src123"), "src123");
    }

    #[test]
    fn slugify_module_mixed_case_and_special() {
        assert_eq!(slugify_module("My-Module_v2"), "my-module-v2");
    }

    // --- Struct derives ---

    #[test]
    fn export_report_clone() {
        let report = MarkdownExportReport {
            elapsed_us: 100,
            pages_written: 5,
            total_bytes: 1000,
            index_bytes: 200,
            symbol_index_bytes: 150,
            graph_bytes: 100,
            modules_exported: 2,
            validation_issues: vec!["issue".to_string()],
        };
        let cloned = report.clone();
        assert_eq!(cloned.elapsed_us, 100);
        assert_eq!(cloned.pages_written, 5);
        assert_eq!(cloned.validation_issues.len(), 1);
    }

    #[test]
    fn export_report_debug() {
        let report = MarkdownExportReport {
            elapsed_us: 0,
            pages_written: 0,
            total_bytes: 0,
            index_bytes: 0,
            symbol_index_bytes: 0,
            graph_bytes: 0,
            modules_exported: 0,
            validation_issues: vec![],
        };
        let debug_str = format!("{:?}", report);
        assert!(debug_str.contains("MarkdownExportReport"));
    }

    #[test]
    fn render_info_clone() {
        let info = MarkdownRenderInfo {
            max_graph_nodes: 100,
            has_files: true,
            has_symbols: true,
            module_count: 5,
            total_pages: 50,
        };
        let cloned = info.clone();
        assert_eq!(cloned.max_graph_nodes, 100);
        assert_eq!(cloned.total_pages, 50);
    }

    #[test]
    fn render_info_debug() {
        let info = MarkdownRenderInfo {
            max_graph_nodes: 50,
            has_files: false,
            has_symbols: false,
            module_count: 0,
            total_pages: 1,
        };
        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("MarkdownRenderInfo"));
    }

    // --- export_markdown_reported edge cases ---

    #[test]
    fn export_reported_empty_model_modules_zero() {
        let model = empty_model();
        let dir = tempfile::tempdir().unwrap();
        let (_, report) = export_markdown_reported(&model, dir.path()).unwrap();
        assert_eq!(report.modules_exported, 0);
    }

    #[test]
    fn export_reported_total_bytes_is_sum_of_parts() {
        let model = make_model();
        let dir = tempfile::tempdir().unwrap();
        let (_, report) = export_markdown_reported(&model, dir.path()).unwrap();
        // total_bytes should account for index + symbols + graph + all pages
        assert!(report.total_bytes >= report.index_bytes
            + report.symbol_index_bytes
            + report.graph_bytes);
    }

    #[test]
    fn export_reported_multiple_modules() {
        let overview = make_page(WikiPageKind::Overview, "Overview", "index");
        let files = vec![
            make_page(WikiPageKind::File, "compiler/driver.rs", "driver"),
            make_page(WikiPageKind::File, "compiler/jit.rs", "jit"),
            make_page(WikiPageKind::File, "wiki/generate.rs", "generate"),
        ];
        let model = WikiModel {
            generated_at: "test".to_string(),
            stats_summary: "test".to_string(),
            overview,
            file_pages: files,
            symbol_pages: vec![],
        };
        let dir = tempfile::tempdir().unwrap();
        let (_, report) = export_markdown_reported(&model, dir.path()).unwrap();
        assert_eq!(report.modules_exported, 2); // "compiler" and "wiki"
    }

    // --- generated_header ---

    #[test]
    fn generated_header_contains_timestamp_and_stats() {
        let model = make_model();
        let header = generated_header(&model);
        assert!(header.contains("2024-01-01T00:00:00Z"));
        assert!(header.contains("5 pages, 3 relationships"));
        assert!(header.contains("Generated from the Velocity site map"));
    }

    #[test]
    fn generated_header_ends_with_newline() {
        let model = make_model();
        let header = generated_header(&model);
        assert!(header.ends_with('\n'));
    }

    // --- render_page_markdown: cross-link paths ---

    #[test]
    fn render_page_file_cross_link_uses_module_path() {
        let model = make_model();
        let page = &model.symbol_pages[0]; // main_fn, called_by src/main.rs
        let md = render_page_markdown(page, &model);
        // src/main.rs is a file, so link should include module path
        assert!(md.contains("[src/main.rs](../files/src/src-main-rs.md)"));
    }

    #[test]
    fn render_page_symbol_cross_link_uses_symbols_path() {
        let model = make_model();
        let page = &model.file_pages[0]; // src/main.rs, defines main_fn
        let md = render_page_markdown(page, &model);
        assert!(md.contains("[main_fn](../symbols/main_fn.md)"));
    }

    // --- render_page: file page for bare file (root module) ---

    #[test]
    fn render_page_bare_file_link_for_root_module() {
        let overview = make_page(WikiPageKind::Overview, "Overview", "index");
        let mut file = make_page(WikiPageKind::File, "main.rs", "main-rs");
        file.relationships = vec![("Defines".to_string(), vec!["entry".to_string()])];
        let mut sym = make_page(WikiPageKind::Symbol, "entry", "entry");
        sym.called_by = vec!["main.rs".to_string()]; // main.rs calls entry
        let model = WikiModel {
            generated_at: "test".to_string(),
            stats_summary: "test".to_string(),
            overview,
            file_pages: vec![file],
            symbol_pages: vec![sym],
        };
        let md = render_page_markdown(&model.symbol_pages[0], &model);
        // main.rs has '.', so module = "root"
        assert!(md.contains("[main.rs](../files/root/main-rs.md)"));
    }
}
