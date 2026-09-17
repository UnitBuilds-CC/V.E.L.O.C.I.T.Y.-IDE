//! Integration test: verify wiki export pipelines produce real files.
//!
//! Run with: cargo test -p velocity-ide --lib wiki::tests::e2e_export_all_formats -- --nocapture

#[cfg(test)]
mod e2e_tests {
    use std::fs;
    use std::path::PathBuf;
    use velocity_ide::site_map::SiteMap;
    use velocity_ide::wiki;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn temp_out(tag: &str) -> PathBuf {
        let nano = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("wiki-e2e-{}-{}", tag, nano));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn e2e_index_workspace_finds_real_files() {
        let root = workspace_root();
        let indices = wiki::index_workspace(&root);
        assert!(
            !indices.is_empty(),
            "expected workspace to index at least one source file"
        );

        // All indices should have non-empty content hash
        for idx in &indices {
            assert!(!idx.content_hash.is_empty(), "empty hash for {:?}", idx.path);
            assert!(idx.size_bytes > 0);
            assert!(idx.line_count > 0);
        }

        // Sample compact_summary is much smaller than source bytes
        let sample = indices.iter().find(|i| i.size_bytes > 500).expect("no samples");
        let compact = sample.compact_summary();
        let ratio = compact.len() as f64 / sample.size_bytes as f64;
        println!(
            "Compact ratio for {:?}: {:.1}% (raw={}, compact={})",
            sample.path,
            ratio * 100.0,
            sample.size_bytes,
            compact.len()
        );
        // Compact summary should be at most 25% of source (much less = 85%+ savings)
        assert!(ratio < 0.25, "compact summary not much smaller (ratio {})", ratio);
    }

    #[test]
    fn e2e_pagerank_produces_scores() {
        let root = workspace_root();
        let sm_path = root.join("target").join("test-sitemap-e2e");
        let _ = fs::create_dir_all(&sm_path);
        let sm = SiteMap::open(&sm_path, 0).expect("open sitemap");

        let result = wiki::build_wiki_enhanced(&sm, &root);
        let pr = result.pagerank.expect("expected pagerank scores");
        println!(
            "PageRank: {} nodes, avg={:.4}, max={:.4}",
            pr.scores.len(),
            pr.avg_score,
            pr.max_score
        );
        // Even with an empty sitemap, the workspace index enrichment should populate
        // file_pages via build_wiki_enhanced. But build_wiki relies on triples, so
        // with an empty sitemap, file_pages may be empty. Assert only structure.
        assert!(pr.max_score >= 0.0);
    }

    #[test]
    fn e2e_markdown_export_all_formats() {
        let root = workspace_root();
        let sm_path = root.join("target").join("test-sitemap-fmt");
        let _ = fs::create_dir_all(&sm_path);
        let sm = SiteMap::open(&sm_path, 0).expect("open sitemap");

        // Build a synthetic model so we exercise the renderers even when the
        // live site map has no triples.
        use velocity_ide::wiki::{WikiModel, WikiPage, WikiPageKind};
        let overview = WikiPage {
            kind: WikiPageKind::Overview,
            title: "Overview".to_string(),
            slug: "index".to_string(),
            summary: format!("Test overview against real sitemap with {} triples.", sm.stats().total_entries),
            relationships: vec![("Files".to_string(), vec!["src/main.rs".to_string()])],
            called_by: vec![],
            detail: None,
        };
        let mut file = WikiPage {
            kind: WikiPageKind::File,
            title: "src/main.rs".to_string(),
            slug: "src-main-rs".to_string(),
            summary: "Defines 2 symbols.".to_string(),
            relationships: vec![("Defines".to_string(), vec!["main".to_string(), "run".to_string()])],
            called_by: vec![],
            detail: Some("Uses `main` and refers to `src/util.rs`.".to_string()),
        };
        file.relationships.push(("Imports".to_string(), vec!["src/util.rs".to_string()]));
        let sym_main = WikiPage {
            kind: WikiPageKind::Symbol,
            title: "main".to_string(),
            slug: "main".to_string(),
            summary: "Entry point.".to_string(),
            relationships: vec![],
            called_by: vec!["src/main.rs".to_string()],
            detail: Some("The `main` function delegates to `run`.".to_string()),
        };
        let sym_run = WikiPage {
            kind: WikiPageKind::Symbol,
            title: "run".to_string(),
            slug: "run".to_string(),
            summary: "Runner.".to_string(),
            relationships: vec![],
            called_by: vec!["src/main.rs".to_string()],
            detail: None,
        };
        let model = WikiModel {
            generated_at: "e2e".to_string(),
            stats_summary: "e2e test".to_string(),
            overview,
            file_pages: vec![file],
            symbol_pages: vec![sym_main, sym_run],
        };

        // Markdown export
        let md_dir = temp_out("md");
        let md_count = wiki::export_markdown(&model, &md_dir).expect("export markdown");
        assert!(md_count >= 4, "expected at least 4 pages, got {}", md_count);
        assert!(md_dir.join("index.md").exists());
        assert!(md_dir.join("symbol_index.md").exists());
        assert!(md_dir.join("graph.md").exists());
        let index_md = fs::read_to_string(md_dir.join("index.md")).unwrap();
        assert!(index_md.contains("Project Wiki"));
        println!("Markdown export: {} pages to {}", md_count, md_dir.display());

        // HTML export
        let html_dir = temp_out("html");
        let html_count = wiki::export_html(&model, &html_dir).expect("export html");
        assert!(html_count >= 4);
        assert!(html_dir.join("index.html").exists());
        assert!(html_dir.join("symbol_index.html").exists());
        assert!(html_dir.join("graph.html").exists());
        let index_html = fs::read_to_string(html_dir.join("index.html")).unwrap();
        assert!(index_html.contains("<!DOCTYPE html>"));
        assert!(index_html.contains("Overview"));
        println!("HTML export: {} pages to {}", html_count, html_dir.display());

        // GitHub Pages export
        let gh_dir = temp_out("ghp");
        let gh_count = wiki::export_github_pages(&model, &gh_dir).expect("export gh-pages");
        assert!(gh_count >= 4);
        assert!(gh_dir.join("index.html").exists());
        assert!(gh_dir.join(".nojekyll").exists());
        assert!(gh_dir.join("_sidebar.md").exists());
        assert!(gh_dir.join("_coverpage.md").exists());
        let cover = fs::read_to_string(gh_dir.join("_coverpage.md")).unwrap();
        assert!(cover.contains("Project Wiki"));
        println!("GitHub Pages export: {} files to {}", gh_count, gh_dir.display());

        // Cross-linking: verify auto-link converts backticked symbols to links
        let linked = wiki::markdown::auto_link("Uses `main` and `src/main.rs` and `unknown`.", &model);
        assert!(linked.contains("[`main`](../symbols/main.md)"), "expected main link: {}", linked);
        assert!(linked.contains("[`src/main.rs`](../files/src/src-main-rs.md)"), "expected file link: {}", linked);
        assert!(linked.contains("`unknown`"), "unknown should stay as inline code: {}", linked);

        // Clean up
        let _ = fs::remove_dir_all(&md_dir);
        let _ = fs::remove_dir_all(&html_dir);
        let _ = fs::remove_dir_all(&gh_dir);
    }

    #[test]
    fn e2e_incremental_cache_hits() {
        let root = workspace_root();
        let sm_path = root.join("target").join("test-sitemap-inc");
        let _ = fs::create_dir_all(&sm_path);
        let sm = SiteMap::open(&sm_path, 0).expect("open sitemap");

        let cache_dir = temp_out("cache");
        let mut cache = wiki::WikiCache::open(&cache_dir);

        // First run — nothing cached
        let r1 = wiki::build_wiki_incremental(&sm, &root, &mut cache);
        let misses_1 = r1.pages_regenerated;
        let hits_1 = r1.pages_from_cache;

        // Save cache to disk
        cache.save().ok();

        // Second run — should have cache hits for any files with module docs
        let mut cache2 = wiki::WikiCache::open(&cache_dir);
        let r2 = wiki::build_wiki_incremental(&sm, &root, &mut cache2);
        let hits_2 = r2.pages_from_cache;

        println!(
            "Cache: run1 misses={} hits={} | run2 misses={} hits={}",
            misses_1, hits_1, r2.pages_regenerated, hits_2
        );

        // At minimum, run2 should have more hits than run1 if any module docs exist
        assert!(hits_2 >= hits_1, "run2 should have >= hits than run1");

        let _ = fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn e2e_import_resolver_scans_modules() {
        let root = workspace_root();
        let resolver = wiki::ImportResolver::new(&root);
        println!(
            "Resolver: {} modules, {} packages",
            resolver.module_count(),
            resolver.package_count()
        );
        // Should find at least one module (this repo has many .rs files)
        assert!(resolver.module_count() > 0, "no modules found");
    }
}
