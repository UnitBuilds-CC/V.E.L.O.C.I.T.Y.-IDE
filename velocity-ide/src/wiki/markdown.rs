//! Renders a [`WikiModel`] to interlinked Markdown files.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::generate::{WikiModel, WikiPage, WikiPageKind};

/// Export the whole wiki to `dir` as Markdown. Returns the number of pages
/// written (overview + files + symbols).
pub fn export_markdown(model: &WikiModel, dir: &Path) -> Result<usize> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::create_dir_all(dir.join("files"))?;
    fs::create_dir_all(dir.join("symbols"))?;

    let mut count = 0usize;

    fs::write(dir.join("index.md"), render_index(model))?;
    count += 1;

    for page in &model.file_pages {
        let path = dir.join("files").join(format!("{}.md", page.slug));
        fs::write(&path, render_page_markdown(page, model))?;
        count += 1;
    }

    for page in &model.symbol_pages {
        let path = dir.join("symbols").join(format!("{}.md", page.slug));
        fs::write(&path, render_page_markdown(page, model))?;
        count += 1;
    }

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
    out.push_str(&model.overview.summary);
    out.push_str("\n\n");

    if !model.file_pages.is_empty() {
        out.push_str("## Files\n\n");
        for page in &model.file_pages {
            out.push_str(&format!(
                "- [{}]({}) — {}\n",
                page.title,
                format!("files/{}.md", page.slug),
                page.summary
            ));
        }
        out.push('\n');
    }

    if !model.symbol_pages.is_empty() {
        out.push_str("## Symbols\n\n");
        for page in &model.symbol_pages {
            out.push_str(&format!(
                "- [{}]({}) — {}\n",
                page.title,
                format!("symbols/{}.md", page.slug),
                page.summary
            ));
        }
        out.push('\n');
    }

    out
}

/// Render a single page to Markdown, cross-linking any relationship targets
/// that resolve to a known file or symbol page.
pub fn render_page_markdown(page: &WikiPage, model: &WikiModel) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", page.title));
    out.push_str(&format!("> {}\n\n", page.kind.label()));
    out.push_str(&generated_header(model));
    out.push('\n');
    out.push_str(&page.summary);
    out.push_str("\n\n");

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

/// Build a relative Markdown link to a target page if one exists, otherwise
/// return the plain name. Links are written relative to a page that lives one
/// directory below the wiki root (files/ or symbols/).
fn link_for(target: &str, model: &WikiModel) -> String {
    if let Some(page) = find_page(model, target) {
        let rel = match page.kind {
            WikiPageKind::File => format!("../files/{}.md", page.slug),
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
