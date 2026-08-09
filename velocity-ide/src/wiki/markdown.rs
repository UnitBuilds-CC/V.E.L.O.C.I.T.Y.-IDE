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

use anyhow::{Context, Result};

use super::generate::{WikiModel, WikiPage, WikiPageKind};

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
        "_Generated from the Velocity site map · {} · {}_\n",
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
                    "- [{}](files/{}/{}.md) — {}\n",
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
                "- [{}](symbols/{}.md) — {}\n",
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
            out.push_str(" · ");
        }
        out.push_str(&format!("[{}](#{})", letter, letter.to_ascii_lowercase()));
    }
    out.push_str("\n\n");

    for (letter, pages) in &by_letter {
        out.push_str(&format!("## {}\n\n", letter));
        for page in pages {
            out.push_str(&format!(
                "- [{}](symbols/{}.md) — {}\n",
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
