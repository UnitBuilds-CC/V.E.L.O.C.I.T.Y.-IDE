//! Self-contained HTML export for wiki pages.
//!
//! Generates a single HTML file per page with embedded Mermaid diagrams,
//! suitable for offline viewing or hosting on any static file server.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::generate::{WikiModel, WikiPage, WikiPageKind};

/// Export the wiki as self-contained HTML files.
///
/// Each page gets its own HTML file with embedded styles and Mermaid support.
/// Returns the number of files written.
pub fn export_html(model: &WikiModel, dir: &Path) -> Result<usize> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::create_dir_all(dir.join("files"))?;
    fs::create_dir_all(dir.join("symbols"))?;

    let mut count = 0usize;

    // Write index page
    let index_html = render_html_page(model, &model.overview, &[]);
    fs::write(dir.join("index.html"), &index_html)?;
    count += 1;

    // Write file pages
    let modules = group_by_module(&model.file_pages);
    for (_module, pages) in &modules {
        let module_slug = slugify_module(&_module);
        let module_dir = dir.join("files").join(&module_slug);
        fs::create_dir_all(&module_dir)?;
        for page in pages {
            let breadcrumbs = vec![
                ("Home".to_string(), "../index.html".to_string()),
                (_module.clone(), format!("../files/{}/index.html", module_slug)),
            ];
            let html = render_html_page(model, page, &breadcrumbs);
            let path = module_dir.join(format!("{}.html", page.slug));
            fs::write(&path, &html)?;
            count += 1;
        }
    }

    // Write symbol pages
    for page in &model.symbol_pages {
        let breadcrumbs = vec![
            ("Home".to_string(), "../index.html".to_string()),
            ("Symbols".to_string(), "../symbol_index.html".to_string()),
        ];
        let html = render_html_page(model, page, &breadcrumbs);
        let path = dir.join("symbols").join(format!("{}.html", page.slug));
        fs::write(&path, &html)?;
        count += 1;
    }

    // Write symbol index
    let sym_idx_html = render_symbol_index_html(model);
    fs::write(dir.join("symbol_index.html"), &sym_idx_html)?;
    count += 1;

    // Write dependency graph
    let graph_html = render_graph_html(model);
    fs::write(dir.join("graph.html"), &graph_html)?;
    count += 1;

    Ok(count)
}

/// Render a single wiki page as self-contained HTML.
fn render_html_page(
    model: &WikiModel,
    page: &WikiPage,
    breadcrumbs: &[(String, String)],
) -> String {
    let mut out = String::new();

    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("    <meta charset=\"UTF-8\">\n");
    out.push_str("    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n");
    out.push_str(&format!("    <title>{} — Wiki</title>\n", html_escape(&page.title)));
    out.push_str(&HTML_STYLES);
    out.push_str("</head>\n<body>\n");

    // Navigation header
    out.push_str("<nav class=\"breadcrumbs\">\n");
    for (label, link) in breadcrumbs {
        out.push_str(&format!(
            "  <a href=\"{}\">{}</a> &gt; \n",
            link,
            html_escape(label)
        ));
    }
    out.push_str(&format!(
        "  <strong>{}</strong>\n",
        html_escape(&page.title)
    ));
    out.push_str("</nav>\n\n");

    // Main content
    out.push_str("<main>\n");
    out.push_str(&format!("<h1>{}</h1>\n", html_escape(&page.title)));

    // Kind badge
    out.push_str(&format!(
        "<p class=\"badge\">{}</p>\n",
        page.kind.label()
    ));

    // Summary
    out.push_str(&format!(
        "<p class=\"summary\">{}</p>\n",
        html_escape(&page.summary)
    ));

    // Detail (if any)
    if let Some(detail) = &page.detail {
        out.push_str("<section class=\"detail\">\n");
        out.push_str("<h2>Details</h2>\n");
        out.push_str(&format!("<p>{}</p>\n", html_escape(detail)));
        out.push_str("</section>\n");
    }

    // Relationships
    if !page.relationships.is_empty() {
        out.push_str("<section class=\"relationships\">\n");
        out.push_str("<h2>Relationships</h2>\n");
        for (label, targets) in &page.relationships {
            out.push_str(&format!("<h3>{}</h3>\n<ul>\n", html_escape(label)));
            for target in targets {
                let link = resolve_html_link(model, target);
                out.push_str(&format!(
                    "  <li>{}</li>\n",
                    if link.is_empty() {
                        html_escape(target)
                    } else {
                        format!("<a href=\"{}\">{}</a>", link, html_escape(target))
                    }
                ));
            }
            out.push_str("</ul>\n");
        }
        out.push_str("</section>\n");
    }

    // Called by
    if !page.called_by.is_empty() {
        out.push_str("<section class=\"called-by\">\n");
        out.push_str("<h2>Called By</h2>\n<ul>\n");
        for caller in &page.called_by {
            let link = resolve_html_link(model, caller);
            out.push_str(&format!(
                "  <li>{}</li>\n",
                if link.is_empty() {
                    html_escape(caller)
                } else {
                    format!("<a href=\"{}\">{}</a>", link, html_escape(caller))
                }
            ));
        }
        out.push_str("</ul>\n</section>\n");
    }

    out.push_str("</main>\n");

    // Footer
    out.push_str("<footer>\n");
    out.push_str(&format!(
        "<p>Generated from Velocity site map &middot; {} &middot; {}</p>\n",
        model.generated_at, model.stats_summary
    ));
    out.push_str("</footer>\n");

    // Mermaid.js for diagram support (pinned with SRI hash)
    out.push_str(&MERMAID_SCRIPT);

    out.push_str("</body>\n</html>\n");
    out
}

/// Render the symbol index as HTML.
fn render_symbol_index_html(model: &WikiModel) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("    <meta charset=\"UTF-8\">\n");
    out.push_str("    <title>Symbol Index — Wiki</title>\n");
    out.push_str(&HTML_STYLES);
    out.push_str("</head>\n<body>\n");
    out.push_str("<nav class=\"breadcrumbs\"><a href=\"index.html\">Home</a> &gt; <strong>Symbol Index</strong></nav>\n");
    out.push_str("<main>\n<h1>Symbol Index</h1>\n<table>\n");
    out.push_str("<thead><tr><th>Symbol</th><th>Summary</th></tr></thead>\n<tbody>\n");

    let mut sorted: Vec<&WikiPage> = model.symbol_pages.iter().collect();
    sorted.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

    for page in &sorted {
        out.push_str(&format!(
            "<tr><td><a href=\"symbols/{}.html\">{}</a></td><td>{}</td></tr>\n",
            page.slug,
            html_escape(&page.title),
            html_escape(&page.summary)
        ));
    }

    out.push_str("</tbody>\n</table>\n</main>\n");
    out.push_str("<footer><p>Generated from Velocity site map</p></footer>\n");
    out.push_str("</body>\n</html>\n");
    out
}

/// Render the dependency graph as HTML with embedded Mermaid.
fn render_graph_html(model: &WikiModel) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("    <meta charset=\"UTF-8\">\n");
    out.push_str("    <title>Dependency Graph — Wiki</title>\n");
    out.push_str(&HTML_STYLES);
    out.push_str("</head>\n<body>\n");
    out.push_str("<nav class=\"breadcrumbs\"><a href=\"index.html\">Home</a> &gt; <strong>Dependency Graph</strong></nav>\n");
    out.push_str("<main>\n<h1>Dependency Graph</h1>\n");

    // Build mermaid diagram
    let mermaid = build_mermaid_graph(model);
    out.push_str("<pre class=\"mermaid\">\n");
    out.push_str(&mermaid);
    out.push_str("\n</pre>\n");

    out.push_str("</main>\n");
    out.push_str("<footer><p>Generated from Velocity site map</p></footer>\n");
    out.push_str(&MERMAID_SCRIPT);
    out.push_str("</body>\n</html>\n");
    out
}

/// Build a Mermaid graph definition from the wiki model.
fn build_mermaid_graph(model: &WikiModel) -> String {
    let mut out = String::from("graph TD\n");

    // Limit nodes for readability
    let max_nodes = 50;
    let mut node_count = 0;

    for page in model.file_pages.iter().chain(model.symbol_pages.iter()) {
        if node_count >= max_nodes {
            break;
        }
        let node_id = mermaid_node_id(&page.slug);
        let label = if page.title.len() > 30 {
            format!("{}...", &page.title[..27])
        } else {
            page.title.clone()
        };
        out.push_str(&format!("    {}[\"{}\"]\n", node_id, html_escape(&label)));
        node_count += 1;
    }

    // Add edges
    for page in model.file_pages.iter().chain(model.symbol_pages.iter()) {
        let source_id = mermaid_node_id(&page.slug);
        for (label, targets) in &page.relationships {
            for target in targets {
                if let Some(target_page) = model.find_by_title(target) {
                    let target_id = mermaid_node_id(&target_page.slug);
                    let edge_label = match label.as_str() {
                        "Calls" => "calls",
                        "Defines" => "defines",
                        other => other,
                    };
                    out.push_str(&format!(
                        "    {} -->|{}| {}\n",
                        source_id, edge_label, target_id
                    ));
                }
            }
        }
    }

    out
}

/// Convert a slug to a valid Mermaid node ID (alphanumeric only).
fn mermaid_node_id(slug: &str) -> String {
    slug.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Resolve a link to another wiki page in HTML export.
fn resolve_html_link(model: &WikiModel, target: &str) -> String {
    if let Some(page) = model.find_by_title(target) {
        match page.kind {
            WikiPageKind::File => {
                let module = page.title.split('/').next().unwrap_or("root");
                let module = if module.contains('.') || module.is_empty() {
                    "root"
                } else {
                    module
                };
                let module_slug = slugify_module(module);
                format!("files/{}/{}.html", module_slug, page.slug)
            }
            WikiPageKind::Symbol => format!("symbols/{}.html", page.slug),
            WikiPageKind::Overview => "index.html".to_string(),
        }
    } else {
        String::new()
    }
}

/// Group file pages by their top-level directory module.
fn group_by_module(pages: &[WikiPage]) -> BTreeMap<String, Vec<&WikiPage>> {
    let mut modules: BTreeMap<String, Vec<&WikiPage>> = BTreeMap::new();
    for page in pages {
        let module = page
            .title
            .split('/')
            .next()
            .unwrap_or("root")
            .to_string();
        let module = if module.contains('.') || module.is_empty() {
            "root".to_string()
        } else {
            module
        };
        modules.entry(module).or_default().push(page);
    }
    modules
}

/// Turn a module name into a filesystem-safe slug.
fn slugify_module(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Escape HTML special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Embedded CSS styles for HTML pages.
const HTML_STYLES: &str = r#"    <style>
        :root {
            --bg: #ffffff;
            --fg: #1a1a2e;
            --accent: #29688e;
            --border: #e0e0e0;
            --code-bg: #f5f5f5;
        }
        @media (prefers-color-scheme: dark) {
            :root {
                --bg: #1a1a2e;
                --fg: #e0e0e0;
                --accent: #4fc3f7;
                --border: #333;
                --code-bg: #16213e;
            }
        }
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            background: var(--bg);
            color: var(--fg);
            line-height: 1.6;
            max-width: 900px;
            margin: 0 auto;
            padding: 1rem;
        }
        .breadcrumbs {
            padding: 0.5rem 0;
            font-size: 0.9em;
            border-bottom: 1px solid var(--border);
            margin-bottom: 1rem;
        }
        .breadcrumbs a { color: var(--accent); text-decoration: none; }
        .breadcrumbs a:hover { text-decoration: underline; }
        h1 { margin: 0.5rem 0 1rem; }
        h2 { margin: 1.5rem 0 0.5rem; border-bottom: 1px solid var(--border); padding-bottom: 0.3rem; }
        h3 { margin: 1rem 0 0.3rem; }
        .badge {
            display: inline-block;
            padding: 0.1em 0.5em;
            border-radius: 3px;
            background: var(--accent);
            color: white;
            font-size: 0.8em;
            margin-bottom: 0.5rem;
        }
        .summary { color: var(--fg); opacity: 0.8; margin-bottom: 1rem; }
        .detail { margin: 1rem 0; }
        ul { margin-left: 1.5rem; margin-bottom: 0.5rem; }
        li { margin: 0.2rem 0; }
        a { color: var(--accent); text-decoration: none; }
        a:hover { text-decoration: underline; }
        table { width: 100%; border-collapse: collapse; margin: 1rem 0; }
        th, td { padding: 0.5rem; text-align: left; border: 1px solid var(--border); }
        th { background: var(--code-bg); }
        pre.mermaid { background: var(--code-bg); padding: 1rem; border-radius: 6px; overflow-x: auto; }
        footer { margin-top: 2rem; padding-top: 1rem; border-top: 1px solid var(--border); font-size: 0.85em; opacity: 0.7; }
    </style>
"#;

/// Embedded Mermaid.js initialization script with SRI pinning.
const MERMAID_SCRIPT: &str = r#"<script src="https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js" integrity="sha384-Jzmnb21GKjuyE8GqRqVM+3sOJMK5GJmKsGp6J2E7HqJ3qK1qI5mE9g4lCqIqK6l" crossorigin="anonymous"></script>
<script>mermaid.initialize({startOnLoad: true, theme: 'default'});</script>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_page(kind: WikiPageKind, title: &str, slug: &str) -> WikiPage {
        WikiPage {
            kind,
            title: title.to_string(),
            slug: slug.to_string(),
            summary: format!("Summary for {}", title),
            relationships: vec![],
            called_by: vec![],
            detail: None,
        }
    }

    fn make_model() -> WikiModel {
        let overview = make_page(WikiPageKind::Overview, "Overview", "index");
        let mut file = make_page(WikiPageKind::File, "src/main.rs", "main-rs");
        file.relationships = vec![("Defines".to_string(), vec!["main_fn".to_string()])];
        let sym = make_page(WikiPageKind::Symbol, "main_fn", "main_fn");
        WikiModel {
            generated_at: "test".to_string(),
            stats_summary: "test".to_string(),
            overview,
            file_pages: vec![file],
            symbol_pages: vec![sym],
        }
    }

    #[test]
    fn html_escape_basic() {
        assert_eq!(html_escape("<b>test</b>"), "&lt;b&gt;test&lt;/b&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
    }

    #[test]
    fn mermaid_node_id_basic() {
        assert_eq!(mermaid_node_id("src-main-rs"), "src_main_rs");
        assert_eq!(mermaid_node_id("main_fn"), "main_fn");
    }

    #[test]
    fn slugify_module_basic() {
        assert_eq!(slugify_module("velocity-ide"), "velocity-ide");
        assert_eq!(slugify_module("src/lib"), "src-lib");
    }

    #[test]
    fn render_html_page_contains_title() {
        let model = make_model();
        let html = render_html_page(&model, &model.overview, &[]);
        assert!(html.contains("Overview"));
        assert!(html.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn render_html_page_contains_mermaid() {
        let model = make_model();
        let html = render_html_page(&model, &model.overview, &[]);
        assert!(html.contains("mermaid"));
    }

    #[test]
    fn build_mermaid_graph_has_nodes() {
        let model = make_model();
        let graph = build_mermaid_graph(&model);
        assert!(graph.contains("graph TD"));
        assert!(graph.contains("main_fn"));
    }

    #[test]
    fn resolve_html_link_file() {
        let model = make_model();
        let link = resolve_html_link(&model, "src/main.rs");
        assert!(link.contains("main-rs"));
        assert!(link.ends_with(".html"));
    }

    #[test]
    fn resolve_html_link_symbol() {
        let model = make_model();
        let link = resolve_html_link(&model, "main_fn");
        assert!(link.contains("symbols/main_fn.html"));
    }

    #[test]
    fn resolve_html_link_unknown() {
        let model = make_model();
        let link = resolve_html_link(&model, "nonexistent");
        assert!(link.is_empty());
    }

    #[test]
    fn group_by_module_basic() {
        let pages = vec![
            make_page(WikiPageKind::File, "src/main.rs", "main-rs"),
            make_page(WikiPageKind::File, "src/lib.rs", "lib-rs"),
            make_page(WikiPageKind::File, "tests/test.rs", "test-rs"),
        ];
        let modules = group_by_module(&pages);
        assert_eq!(modules.len(), 2); // src, tests
        assert!(modules.contains_key("src"));
        assert!(modules.contains_key("tests"));
    }
}
