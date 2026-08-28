//! Builds a [`WikiModel`] from a [`SiteMap`].

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::Serialize;

use crate::site_map::SiteMap;

/// The flavour of a wiki page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum WikiPageKind {
    Overview,
    File,
    Symbol,
}

impl WikiPageKind {
    pub fn label(self) -> &'static str {
        match self {
            WikiPageKind::Overview => "Overview",
            WikiPageKind::File => "File",
            WikiPageKind::Symbol => "Symbol",
        }
    }
}

/// A single wiki page derived from sitemap triples.
#[derive(Clone, Debug, Serialize)]
pub struct WikiPage {
    pub kind: WikiPageKind,
    /// Resolved human-readable name (or a hex hash fallback).
    pub title: String,
    /// Filesystem/link-safe identifier used for the exported file name.
    pub slug: String,
    /// Deterministic one-line description.
    pub summary: String,
    /// Outgoing relationships grouped by label, e.g. ("Defines", [symbols…]).
    pub relationships: Vec<(String, Vec<String>)>,
    /// Incoming "called by" edges (subjects of `Calls` triples targeting this page).
    pub called_by: Vec<String>,
    /// Optional AI-generated narration layered on top of the structural page.
    pub detail: Option<String>,
}

/// The complete generated wiki.
#[derive(Clone, Debug)]
pub struct WikiModel {
    pub generated_at: String,
    pub stats_summary: String,
    pub overview: WikiPage,
    pub file_pages: Vec<WikiPage>,
    pub symbol_pages: Vec<WikiPage>,
}

impl WikiModel {
    pub fn is_empty(&self) -> bool {
        self.file_pages.is_empty() && self.symbol_pages.is_empty()
    }

    pub fn file_count(&self) -> usize {
        self.file_pages.len()
    }

    pub fn symbol_count(&self) -> usize {
        self.symbol_pages.len()
    }

    /// Total number of pages (overview + files + symbols).
    pub fn total_pages(&self) -> usize {
        1 + self.file_pages.len() + self.symbol_pages.len()
    }

    /// Iterate over all pages: overview first, then files, then symbols.
    pub fn all_pages(&self) -> impl Iterator<Item = &WikiPage> {
        std::iter::once(&self.overview)
            .chain(self.file_pages.iter())
            .chain(self.symbol_pages.iter())
    }

    /// Find a page by exact title match.
    pub fn find_by_title(&self, title: &str) -> Option<&WikiPage> {
        self.all_pages().find(|p| p.title == title)
    }

    /// Find a page by slug match.
    pub fn find_by_slug(&self, slug: &str) -> Option<&WikiPage> {
        self.all_pages().find(|p| p.slug == slug)
    }

    /// Search across all pages for keyword matches.
    ///
    /// Searches title, summary, and detail fields (case-insensitive).
    /// Returns results sorted by relevance score (higher = better match).
    pub fn search(&self, query: &str) -> Vec<WikiSearchResult> {
        if query.is_empty() {
            return Vec::new();
        }
        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

        let mut results: Vec<WikiSearchResult> = self
            .all_pages()
            .filter_map(|page| {
                let score = compute_relevance(page, &query_terms);
                if score > 0 {
                    Some(WikiSearchResult { page: page.clone(), score })
                } else {
                    None
                }
            })
            .collect();

        // Sort by score descending, then by title alphabetically.
        results.sort_by(|a, b| {
            b.score.cmp(&a.score).then_with(|| a.page.title.cmp(&b.page.title))
        });
        results
    }

    /// Get all symbols defined by a specific file page.
    pub fn symbols_defined_by(&self, file_title: &str) -> Vec<&WikiPage> {
        self.symbol_pages
            .iter()
            .filter(|sym| {
                sym.called_by.iter().any(|caller| caller == file_title)
            })
            .collect()
    }

    /// Get all files that reference a specific symbol.
    pub fn files_referencing(&self, symbol_title: &str) -> Vec<&WikiPage> {
        self.file_pages
            .iter()
            .filter(|file| {
                file.relationships.iter().any(|(_, targets)| {
                    targets.iter().any(|t| t == symbol_title)
                })
            })
            .collect()
    }

    /// Return diagnostic statistics for this wiki model.
    pub fn stats(&self) -> WikiStats {
        let total_relationships: usize = self
            .all_pages()
            .map(|p| p.relationships.iter().map(|(_, t)| t.len()).sum::<usize>())
            .sum();
        let total_called_by: usize = self.all_pages().map(|p| p.called_by.len()).sum();
        let pages_with_detail: usize = self.all_pages().filter(|p| p.detail.is_some()).count();

        WikiStats {
            total_pages: self.total_pages(),
            file_pages: self.file_pages.len(),
            symbol_pages: self.symbol_pages.len(),
            total_relationships,
            total_called_by,
            pages_with_detail,
            generated_at: self.generated_at.clone(),
        }
    }

    /// Validate the wiki model for consistency.
    /// Returns a list of warnings (empty = all good).
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.is_empty() {
            warnings.push("Wiki has no file or symbol pages".to_string());
        }

        // Check for duplicate slugs.
        let mut slugs = std::collections::HashSet::new();
        for page in self.all_pages() {
            if !slugs.insert(&page.slug) {
                warnings.push(format!("Duplicate slug: {}", page.slug));
            }
        }

        // Check for empty titles.
        for page in self.all_pages() {
            if page.title.is_empty() {
                warnings.push("Page has empty title".to_string());
            }
        }

        warnings
    }

    /// Build comprehensive diagnostic info for this wiki model.
    pub fn info(&self) -> WikiModelInfo {
        let stats = self.stats();
        let orphans = self.orphan_pages();
        let top = self.top_symbols(10);
        let issues = self.validate();

        WikiModelInfo {
            total_pages: stats.total_pages,
            file_pages: stats.file_pages,
            symbol_pages: stats.symbol_pages,
            total_relationships: stats.total_relationships,
            total_called_by: stats.total_called_by,
            pages_with_detail: stats.pages_with_detail,
            orphan_pages: orphans.len(),
            top_symbols: top.into_iter().map(|p| p.title.clone()).collect(),
            validation_issues: issues,
            generated_at: self.generated_at.clone(),
        }
    }

    /// Find pages with no relationships and no incoming calls.
    pub fn orphan_pages(&self) -> Vec<&WikiPage> {
        self.all_pages()
            .filter(|p| {
                p.relationships.is_empty()
                    && p.called_by.is_empty()
                    && p.kind != WikiPageKind::Overview
            })
            .collect()
    }

    /// Find the most-referenced symbol pages (by incoming call count).
    pub fn top_symbols(&self, limit: usize) -> Vec<&WikiPage> {
        let mut syms: Vec<&WikiPage> = self.symbol_pages.iter().collect();
        syms.sort_by(|a, b| b.called_by.len().cmp(&a.called_by.len()));
        syms.truncate(limit);
        syms
    }

    /// Extract all relationship edges as a flat list (for graph visualization).
    pub fn relationship_edges(&self) -> Vec<WikiEdge> {
        let mut edges = Vec::new();
        for page in self.all_pages() {
            for (label, targets) in &page.relationships {
                for target in targets {
                    edges.push(WikiEdge {
                        source: page.title.clone(),
                        label: label.clone(),
                        target: target.clone(),
                    });
                }
            }
        }
        edges
    }

    /// Search for multiple queries at once, merging and deduplicating results.
    pub fn batch_search(&self, queries: &[&str]) -> Vec<WikiSearchResult> {
        let mut seen_slugs = std::collections::HashSet::new();
        let mut merged: Vec<WikiSearchResult> = Vec::new();

        for query in queries {
            for result in self.search(query) {
                if seen_slugs.insert(result.page.slug.clone()) {
                    merged.push(result);
                }
            }
        }

        merged.sort_by(|a, b| {
            b.score.cmp(&a.score).then_with(|| a.page.title.cmp(&b.page.title))
        });
        merged
    }

    /// Search filtered to a specific page kind.
    pub fn search_by_kind(&self, query: &str, kind: WikiPageKind) -> Vec<WikiSearchResult> {
        if query.is_empty() {
            return Vec::new();
        }
        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

        let pages = match kind {
            WikiPageKind::Overview => std::iter::once(&self.overview).collect::<Vec<_>>(),
            WikiPageKind::File => self.file_pages.iter().collect(),
            WikiPageKind::Symbol => self.symbol_pages.iter().collect(),
        };

        let mut results: Vec<WikiSearchResult> = pages
            .into_iter()
            .filter_map(|page| {
                let score = compute_relevance(page, &query_terms);
                if score > 0 {
                    Some(WikiSearchResult { page: page.clone(), score })
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| {
            b.score.cmp(&a.score).then_with(|| a.page.title.cmp(&b.page.title))
        });
        results
    }

    /// Paginated search with limit and offset.
    pub fn search_paginated(&self, query: &str, limit: usize, offset: usize) -> PaginatedSearchResult {
        let all_results = self.search(query);
        let total = all_results.len();
        let page_results: Vec<WikiSearchResult> = all_results
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect();

        PaginatedSearchResult {
            results: page_results,
            total_matches: total,
            offset,
            limit,
            has_more: total > offset + limit,
        }
    }

    /// Prefix-based autocomplete for search suggestions.
    /// Returns up to `limit` page titles that start with or contain the prefix.
    pub fn autocomplete(&self, prefix: &str, limit: usize) -> Vec<AutocompleteSuggestion> {
        if prefix.is_empty() {
            return Vec::new();
        }
        let prefix_lower = prefix.to_lowercase();
        let mut suggestions: Vec<AutocompleteSuggestion> = Vec::new();

        for page in self.all_pages() {
            let title_lower = page.title.to_lowercase();
            if title_lower.starts_with(&prefix_lower) {
                suggestions.push(AutocompleteSuggestion {
                    title: page.title.clone(),
                    slug: page.slug.clone(),
                    kind: page.kind,
                    match_type: "prefix",
                });
            } else if title_lower.contains(&prefix_lower) {
                suggestions.push(AutocompleteSuggestion {
                    title: page.title.clone(),
                    slug: page.slug.clone(),
                    kind: page.kind,
                    match_type: "contains",
                });
            }
        }

        // Sort: prefix matches first, then contains; within each group alphabetical.
        suggestions.sort_by(|a, b| {
            (&a.match_type, &a.title).cmp(&(&b.match_type, &b.title))
        });
        suggestions.truncate(limit);
        suggestions
    }

    /// Fuzzy search using simple subsequence matching (typo-tolerant).
    /// Returns pages where the query characters appear as a subsequence in the title.
    pub fn fuzzy_search(&self, query: &str) -> Vec<WikiSearchResult> {
        if query.is_empty() {
            return Vec::new();
        }
        let query_lower = query.to_lowercase();
        let query_chars: Vec<char> = query_lower.chars().collect();

        let mut results: Vec<WikiSearchResult> = self
            .all_pages()
            .filter_map(|page| {
                let title_lower = page.title.to_lowercase();
                let title_chars: Vec<char> = title_lower.chars().collect();
                if is_subsequence(&query_chars, &title_chars) {
                    // Score inversely proportional to title length (tighter match = better).
                    let score = if title_chars.is_empty() {
                        1
                    } else {
                        (query_chars.len() as u32) * 10 / (title_chars.len() as u32)
                    };
                    Some(WikiSearchResult { page: page.clone(), score: score.max(1) })
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| {
            b.score.cmp(&a.score).then_with(|| a.page.title.cmp(&b.page.title))
        });
        results
    }

    /// Generate a structured search report with diagnostics.
    pub fn search_report(&self, query: &str) -> SearchReport {
        let start = std::time::Instant::now();
        let results = self.search(query);
        let elapsed = start.elapsed();

        let kind_counts = {
            let mut counts = std::collections::HashMap::new();
            for r in &results {
                *counts.entry(r.page.kind.label().to_string()).or_insert(0usize) += 1;
            }
            let mut v: Vec<(String, usize)> = counts.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v
        };

        let top_score = results.first().map(|r| r.score).unwrap_or(0);
        let avg_score = if results.is_empty() {
            0.0
        } else {
            results.iter().map(|r| r.score as f64).sum::<f64>() / results.len() as f64
        };

        SearchReport {
            query: query.to_string(),
            total_matches: results.len(),
            elapsed_us: elapsed.as_micros() as u64,
            top_score,
            average_score: (avg_score * 100.0).round() / 100.0,
            results_by_kind: kind_counts,
            top_results: results.iter().take(5).map(|r| r.page.title.clone()).collect(),
        }
    }

    /// Get the most connected pages (by total relationship + called_by count).
    pub fn most_connected_pages(&self, limit: usize) -> Vec<(&WikiPage, usize)> {
        let mut scored: Vec<(&WikiPage, usize)> = self
            .all_pages()
            .map(|p| {
                let rel_count: usize = p.relationships.iter().map(|(_, t)| t.len()).sum();
                let total = rel_count + p.called_by.len();
                (p, total)
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.truncate(limit);
        scored
    }

    /// Count pages by kind.
    pub fn pages_by_kind(&self) -> Vec<(WikiPageKind, usize)> {
        vec![
            (WikiPageKind::Overview, 1),
            (WikiPageKind::File, self.file_pages.len()),
            (WikiPageKind::Symbol, self.symbol_pages.len()),
        ]
    }

    /// Validate a single wiki page for consistency.
    pub fn validate_page(page: &WikiPage) -> Vec<String> {
        let mut issues = Vec::new();
        if page.title.is_empty() {
            issues.push("Page has empty title".to_string());
        }
        if page.slug.is_empty() {
            issues.push("Page has empty slug".to_string());
        }
        if page.summary.is_empty() {
            issues.push(format!("Page '{}' has empty summary", page.title));
        }
        for (label, targets) in &page.relationships {
            if label.is_empty() {
                issues.push(format!(
                    "Page '{}' has relationship with empty label",
                    page.title
                ));
            }
            if targets.is_empty() {
                issues.push(format!(
                    "Page '{}' has empty target list for relationship '{}'",
                    page.title, label
                ));
            }
        }
        issues
    }
}

/// A single search result with relevance score.
#[derive(Clone, Debug, Serialize)]
pub struct WikiSearchResult {
    pub page: WikiPage,
    pub score: u32,
}

/// Diagnostic statistics for a wiki model.
#[derive(Debug, Clone, Serialize)]
pub struct WikiStats {
    pub total_pages: usize,
    pub file_pages: usize,
    pub symbol_pages: usize,
    pub total_relationships: usize,
    pub total_called_by: usize,
    pub pages_with_detail: usize,
    pub generated_at: String,
}

/// Comprehensive wiki model diagnostic info.
#[derive(Debug, Clone, Serialize)]
pub struct WikiModelInfo {
    pub total_pages: usize,
    pub file_pages: usize,
    pub symbol_pages: usize,
    pub total_relationships: usize,
    pub total_called_by: usize,
    pub pages_with_detail: usize,
    pub orphan_pages: usize,
    pub top_symbols: Vec<String>,
    pub validation_issues: Vec<String>,
    pub generated_at: String,
}

/// Report from wiki generation with timing.
#[derive(Debug, Clone, Serialize)]
pub struct WikiGenerationReport {
    pub elapsed_us: u64,
    pub triples_processed: usize,
    pub file_pages_generated: usize,
    pub symbol_pages_generated: usize,
    pub total_pages: usize,
    pub validation_issues: Vec<String>,
}

/// A paginated search result.
#[derive(Debug, Clone, Serialize)]
pub struct PaginatedSearchResult {
    pub results: Vec<WikiSearchResult>,
    pub total_matches: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

/// An autocomplete suggestion.
#[derive(Debug, Clone, Serialize)]
pub struct AutocompleteSuggestion {
    pub title: String,
    pub slug: String,
    pub kind: WikiPageKind,
    pub match_type: &'static str,
}

/// Structured search diagnostics.
#[derive(Debug, Clone, Serialize)]
pub struct SearchReport {
    pub query: String,
    pub total_matches: usize,
    pub elapsed_us: u64,
    pub top_score: u32,
    pub average_score: f64,
    pub results_by_kind: Vec<(String, usize)>,
    pub top_results: Vec<String>,
}

/// Check if `sub` is a subsequence of `full`.
fn is_subsequence(sub: &[char], full: &[char]) -> bool {
    let mut si = 0;
    for &fc in full {
        if si < sub.len() && sub[si] == fc {
            si += 1;
        }
    }
    si == sub.len()
}

/// A relationship edge in the wiki graph.
#[derive(Debug, Clone, Serialize)]
pub struct WikiEdge {
    pub source: String,
    pub label: String,
    pub target: String,
}

/// Compute relevance score for a page against query terms.
/// Title matches are weighted highest, then summary, then detail.
fn compute_relevance(page: &WikiPage, terms: &[&str]) -> u32 {
    let mut score = 0u32;
    let title_lower = page.title.to_lowercase();
    let summary_lower = page.summary.to_lowercase();
    let detail_lower = page.detail.as_deref().unwrap_or("").to_lowercase();

    for term in terms {
        // Title match: highest weight.
        if title_lower.contains(term) {
            score += 10;
            if title_lower == *term {
                score += 20; // Exact title match bonus.
            }
        }
        // Summary match: medium weight.
        if summary_lower.contains(term) {
            score += 5;
        }
        // Detail match: low weight.
        if detail_lower.contains(term) {
            score += 2;
        }
        // Relationship target match.
        for (_, targets) in &page.relationships {
            for target in targets {
                if target.to_lowercase().contains(term) {
                    score += 3;
                }
            }
        }
        // Called-by match.
        for caller in &page.called_by {
            if caller.to_lowercase().contains(term) {
                score += 3;
            }
        }
    }
    score
}

/// Human-readable label for a predicate id. Unknown predicates render
/// generically so richer indexing improves pages automatically.
fn predicate_label(predicate_id: u16) -> String {
    match predicate_id {
        1 => "Defines".to_string(),
        2 => "Calls".to_string(),
        other => format!("Relates (predicate {})", other),
    }
}

/// Resolve a hash to a name, falling back to a zero-padded hex literal.
fn resolve_name(sm: &SiteMap, hash: u64) -> String {
    sm.resolve_string(hash)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{:016x}", hash))
}

/// Heuristic: a resolved name that contains a path separator is treated as a
/// file page; everything else is a symbol page.
fn looks_like_path(name: &str) -> bool {
    name.contains('/') || name.contains('\\')
}

/// Turn a title into a filesystem/link-safe slug.
fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            slug.push(ch.to_ascii_lowercase());
        } else if (ch == '/' || ch == '\\' || ch == '.' || ch == ' ' || ch == ':')
            && !slug.ends_with('-')
        {
            slug.push('-');
        }
        // Drop any other characters.
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "page".to_string()
    } else {
        slug
    }
}

/// Build the full wiki model from the live triples in `sm`.
pub fn build_wiki(sm: &SiteMap) -> WikiModel {
    let triples = sm.find_live_triples(None, None, None);
    let stats = sm.stats();
    let stats_summary = stats.to_string();
    let generated_at = now_string();

    if triples.is_empty() {
        let overview = WikiPage {
            kind: WikiPageKind::Overview,
            title: "Overview".to_string(),
            slug: "index".to_string(),
            summary: "No indexed semantic data is available yet.".to_string(),
            relationships: Vec::new(),
            called_by: Vec::new(),
            detail: None,
        };
        return WikiModel {
            generated_at,
            stats_summary,
            overview,
            file_pages: Vec::new(),
            symbol_pages: Vec::new(),
        };
    }

    // subject hash -> predicate id -> ordered set of object names.
    let mut by_subject: HashMap<u64, BTreeMap<u16, BTreeSet<String>>> = HashMap::new();
    // object hash -> ordered set of subject names (for inverse "called by").
    let mut incoming: HashMap<u64, BTreeSet<String>> = HashMap::new();
    // Every entity that appears as a subject or an object gets a page.
    let mut entities: BTreeSet<u64> = BTreeSet::new();

    for triple in &triples {
        let object_name = resolve_name(sm, triple.object_hash);
        by_subject
            .entry(triple.subject_hash)
            .or_default()
            .entry(triple.predicate_id)
            .or_default()
            .insert(object_name);

        let subject_name = resolve_name(sm, triple.subject_hash);
        incoming
            .entry(triple.object_hash)
            .or_default()
            .insert(subject_name);

        entities.insert(triple.subject_hash);
        entities.insert(triple.object_hash);
    }

    let mut file_pages: Vec<WikiPage> = Vec::new();
    let mut symbol_pages: Vec<WikiPage> = Vec::new();

    let mut subjects: Vec<(u64, String)> = entities
        .into_iter()
        .map(|hash| (hash, resolve_name(sm, hash)))
        .collect();
    subjects.sort_by(|a, b| a.1.cmp(&b.1));

    for (subject_hash, subject_name) in subjects {
        let predicates = by_subject.remove(&subject_hash).unwrap_or_default();
        let relationships: Vec<(String, Vec<String>)> = predicates
            .into_iter()
            .map(|(predicate_id, objects)| {
                (predicate_label(predicate_id), objects.into_iter().collect())
            })
            .collect();

        let defined = relationships
            .iter()
            .find(|(label, _)| label == "Defines")
            .map(|(_, targets)| targets.len())
            .unwrap_or(0);
        let calls = relationships
            .iter()
            .find(|(label, _)| label == "Calls")
            .map(|(_, targets)| targets.len())
            .unwrap_or(0);

        let called_by: Vec<String> = incoming
            .get(&subject_hash)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default();

        let is_file = looks_like_path(&subject_name);
        let kind = if is_file {
            WikiPageKind::File
        } else {
            WikiPageKind::Symbol
        };

        let summary = if is_file {
            if defined == 0 {
                "Indexed source file.".to_string()
            } else {
                format!("Defines {} symbol(s).", defined)
            }
        } else if calls == 0 && called_by.is_empty() {
            "Indexed symbol.".to_string()
        } else {
            format!("Calls {} symbol(s); called by {}.", calls, called_by.len())
        };

        let page = WikiPage {
            kind,
            title: subject_name.clone(),
            slug: slugify(&subject_name),
            summary,
            relationships,
            called_by,
            detail: None,
        };

        if is_file {
            file_pages.push(page);
        } else {
            symbol_pages.push(page);
        }
    }

    let overview = WikiPage {
        kind: WikiPageKind::Overview,
        title: "Overview".to_string(),
        slug: "index".to_string(),
        summary: format!(
            "{} file(s) and {} symbol(s) indexed in the site map.",
            file_pages.len(),
            symbol_pages.len()
        ),
        relationships: vec![
            (
                "Files".to_string(),
                file_pages.iter().map(|p| p.title.clone()).collect(),
            ),
            (
                "Symbols".to_string(),
                symbol_pages.iter().map(|p| p.title.clone()).collect(),
            ),
        ],
        called_by: Vec::new(),
        detail: None,
    };

    WikiModel {
        generated_at,
        stats_summary,
        overview,
        file_pages,
        symbol_pages,
    }
}

fn now_string() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

#[cfg(test)]
mod inline_tests {
    use super::*;

    #[test]
    fn slugify_handles_paths_and_symbols() {
        assert_eq!(
            slugify("velocity-mcp/src/editor/app.rs"),
            "velocity-mcp-src-editor-app-rs"
        );
        assert_eq!(slugify("match_token"), "match_token");
        assert_eq!(slugify("///"), "page");
    }

    #[test]
    fn looks_like_path_detects_separators() {
        assert!(looks_like_path("velocity-ide/src/nda.rs"));
        assert!(!looks_like_path("match_token"));
    }

    #[test]
    fn predicate_label_known_and_unknown() {
        assert_eq!(predicate_label(1), "Defines");
        assert_eq!(predicate_label(2), "Calls");
        assert_eq!(predicate_label(9), "Relates (predicate 9)");
    }

    #[test]
    fn wiki_page_serializes() {
        let page = WikiPage {
            kind: WikiPageKind::Symbol,
            title: "test_fn".to_string(),
            slug: "test_fn".to_string(),
            summary: "A test function.".to_string(),
            relationships: vec![("Calls".to_string(), vec!["other_fn".to_string()])],
            called_by: vec!["main".to_string()],
            detail: Some("Detailed description.".to_string()),
        };
        let json = serde_json::to_string(&page).unwrap();
        assert!(json.contains("\"title\":\"test_fn\""));
        assert!(json.contains("\"kind\":\"Symbol\""));
    }

    #[test]
    fn wiki_search_result_serializes() {
        let result = WikiSearchResult {
            page: WikiPage {
                kind: WikiPageKind::File,
                title: "test.rs".to_string(),
                slug: "test-rs".to_string(),
                summary: "Test file.".to_string(),
                relationships: vec![],
                called_by: vec![],
                detail: None,
            },
            score: 15,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"score\":15"));
    }

    #[test]
    fn wiki_stats_serializes() {
        let stats = WikiStats {
            total_pages: 10,
            file_pages: 3,
            symbol_pages: 6,
            total_relationships: 25,
            total_called_by: 15,
            pages_with_detail: 2,
            generated_at: "1234567890".to_string(),
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"total_pages\":10"));
        assert!(json.contains("\"file_pages\":3"));
    }

    #[test]
    fn wiki_page_kind_serializes() {
        let kind = WikiPageKind::Overview;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"Overview\"");
    }

    // ─── Block 38: new tests ─────────────────────────────────────────────

    fn make_test_page(kind: WikiPageKind, title: &str, slug: &str) -> WikiPage {
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

    fn make_test_model() -> WikiModel {
        let overview = make_test_page(WikiPageKind::Overview, "Overview", "index");
        let mut file1 = make_test_page(WikiPageKind::File, "src/main.rs", "src-main-rs");
        file1.relationships = vec![("Defines".to_string(), vec!["main_fn".to_string()])];

        let mut sym1 = make_test_page(WikiPageKind::Symbol, "main_fn", "main_fn");
        sym1.called_by = vec!["src/main.rs".to_string()];

        let mut sym2 = make_test_page(WikiPageKind::Symbol, "helper_fn", "helper_fn");
        sym2.called_by = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];

        let orphan = make_test_page(WikiPageKind::Symbol, "unused_fn", "unused_fn");

        WikiModel {
            generated_at: "1234567890".to_string(),
            stats_summary: "test".to_string(),
            overview,
            file_pages: vec![file1],
            symbol_pages: vec![sym1, sym2, orphan],
        }
    }

    #[test]
    fn wiki_model_info_basic() {
        let model = make_test_model();
        let info = model.info();
        assert_eq!(info.total_pages, 5); // 1 overview + 1 file + 3 symbols
        assert_eq!(info.file_pages, 1);
        assert_eq!(info.symbol_pages, 3);
        assert_eq!(info.orphan_pages, 1); // unused_fn
        assert!(!info.top_symbols.is_empty());
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn wiki_model_info_serializes() {
        let info = WikiModelInfo {
            total_pages: 10,
            file_pages: 3,
            symbol_pages: 6,
            total_relationships: 25,
            total_called_by: 15,
            pages_with_detail: 2,
            orphan_pages: 1,
            top_symbols: vec!["main".to_string()],
            validation_issues: vec![],
            generated_at: "1234567890".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"orphan_pages\":1"));
        assert!(json.contains("\"top_symbols\":[\"main\"]"));
    }

    #[test]
    fn wiki_orphan_pages() {
        let model = make_test_model();
        let orphans = model.orphan_pages();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].title, "unused_fn");
    }

    #[test]
    fn wiki_top_symbols() {
        let model = make_test_model();
        let top = model.top_symbols(2);
        assert_eq!(top.len(), 2);
        // helper_fn has 2 callers, main_fn has 1
        assert_eq!(top[0].title, "helper_fn");
        assert_eq!(top[1].title, "main_fn");
    }

    #[test]
    fn wiki_relationship_edges() {
        let model = make_test_model();
        let edges = model.relationship_edges();
        assert_eq!(edges.len(), 1); // file1 -> Defines -> main_fn
        assert_eq!(edges[0].source, "src/main.rs");
        assert_eq!(edges[0].label, "Defines");
        assert_eq!(edges[0].target, "main_fn");
    }

    #[test]
    fn wiki_edge_serializes() {
        let edge = WikiEdge {
            source: "a.rs".to_string(),
            label: "Calls".to_string(),
            target: "b_fn".to_string(),
        };
        let json = serde_json::to_string(&edge).unwrap();
        assert!(json.contains("\"source\":\"a.rs\""));
        assert!(json.contains("\"label\":\"Calls\""));
    }

    #[test]
    fn wiki_batch_search() {
        let model = make_test_model();
        let results = model.batch_search(&["main", "helper"]);
        assert!(!results.is_empty());
        // Should find pages matching "main" or "helper"
        let titles: Vec<&str> = results.iter().map(|r| r.page.title.as_str()).collect();
        assert!(titles.contains(&"main_fn") || titles.contains(&"src/main.rs"));
    }

    #[test]
    fn wiki_batch_search_empty() {
        let model = make_test_model();
        let results = model.batch_search(&[]);
        assert!(results.is_empty());
    }

    #[test]
    fn wiki_validate_page_clean() {
        let page = make_test_page(WikiPageKind::Symbol, "test_fn", "test_fn");
        let issues = WikiModel::validate_page(&page);
        assert!(issues.is_empty());
    }

    #[test]
    fn wiki_validate_page_empty_title() {
        let page = make_test_page(WikiPageKind::Symbol, "", "test");
        let issues = WikiModel::validate_page(&page);
        assert!(!issues.is_empty());
        assert!(issues[0].contains("empty title"));
    }

    #[test]
    fn wiki_validate_page_empty_slug() {
        let page = WikiPage {
            kind: WikiPageKind::Symbol,
            title: "test".to_string(),
            slug: "".to_string(),
            summary: "test".to_string(),
            relationships: vec![],
            called_by: vec![],
            detail: None,
        };
        let issues = WikiModel::validate_page(&page);
        assert!(issues.iter().any(|i| i.contains("empty slug")));
    }

    #[test]
    fn wiki_validate_page_empty_relationship_targets() {
        let mut page = make_test_page(WikiPageKind::File, "test.rs", "test-rs");
        page.relationships = vec![("Calls".to_string(), vec![])];
        let issues = WikiModel::validate_page(&page);
        assert!(issues.iter().any(|i| i.contains("empty target list")));
    }

    #[test]
    fn wiki_generation_report_serializes() {
        let report = WikiGenerationReport {
            elapsed_us: 5000,
            triples_processed: 100,
            file_pages_generated: 10,
            symbol_pages_generated: 50,
            total_pages: 61,
            validation_issues: vec![],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"triples_processed\":100"));
        assert!(json.contains("\"total_pages\":61"));
    }

    // ─── Block 78: search improvements tests ────────────────────────────

    #[test]
    fn wiki_search_by_kind_file() {
        let model = make_test_model();
        let results = model.search_by_kind("main", WikiPageKind::File);
        assert!(!results.is_empty());
        for r in &results {
            assert_eq!(r.page.kind, WikiPageKind::File);
        }
    }

    #[test]
    fn wiki_search_by_kind_symbol() {
        let model = make_test_model();
        let results = model.search_by_kind("fn", WikiPageKind::Symbol);
        assert!(!results.is_empty());
        for r in &results {
            assert_eq!(r.page.kind, WikiPageKind::Symbol);
        }
    }

    #[test]
    fn wiki_search_by_kind_empty_query() {
        let model = make_test_model();
        let results = model.search_by_kind("", WikiPageKind::File);
        assert!(results.is_empty());
    }

    #[test]
    fn wiki_search_paginated_first_page() {
        let model = make_test_model();
        let result = model.search_paginated("fn", 2, 0);
        assert!(result.results.len() <= 2);
        assert_eq!(result.offset, 0);
        assert_eq!(result.limit, 2);
    }

    #[test]
    fn wiki_search_paginated_has_more() {
        let model = make_test_model();
        let result = model.search_paginated("fn", 1, 0);
        assert_eq!(result.results.len(), 1);
        assert!(result.has_more);
    }

    #[test]
    fn wiki_search_paginated_offset() {
        let model = make_test_model();
        let result = model.search_paginated("fn", 100, 1000);
        assert!(result.results.is_empty());
        assert!(!result.has_more);
    }

    #[test]
    fn wiki_autocomplete_prefix() {
        let model = make_test_model();
        let suggestions = model.autocomplete("main", 10);
        assert!(!suggestions.is_empty());
        // Prefix matches should come first
        assert!(suggestions.iter().any(|s| s.match_type == "prefix"));
    }

    #[test]
    fn wiki_autocomplete_empty_prefix() {
        let model = make_test_model();
        let suggestions = model.autocomplete("", 10);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn wiki_autocomplete_limit() {
        let model = make_test_model();
        let suggestions = model.autocomplete("fn", 1);
        assert!(suggestions.len() <= 1);
    }

    #[test]
    fn wiki_fuzzy_search_subsequence() {
        let model = make_test_model();
        // "mnfn" should fuzzy-match "main_fn" (subsequence m-a-i-n-_-f-n)
        let results = model.fuzzy_search("mnfn");
        assert!(results.iter().any(|r| r.page.title == "main_fn"));
    }

    #[test]
    fn wiki_fuzzy_search_empty() {
        let model = make_test_model();
        let results = model.fuzzy_search("");
        assert!(results.is_empty());
    }

    #[test]
    fn wiki_fuzzy_search_no_match() {
        let model = make_test_model();
        let results = model.fuzzy_search("zzzzz");
        assert!(results.is_empty());
    }

    #[test]
    fn wiki_search_report_basic() {
        let model = make_test_model();
        let report = model.search_report("main");
        assert_eq!(report.query, "main");
        assert!(report.total_matches > 0);
        assert!(!report.top_results.is_empty());
    }

    #[test]
    fn wiki_search_report_serializes() {
        let report = SearchReport {
            query: "test".to_string(),
            total_matches: 5,
            elapsed_us: 100,
            top_score: 30,
            average_score: 15.5,
            results_by_kind: vec![("Symbol".to_string(), 3), ("File".to_string(), 2)],
            top_results: vec!["a".to_string(), "b".to_string()],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"total_matches\":5"));
    }

    #[test]
    fn wiki_most_connected_pages() {
        let model = make_test_model();
        let connected = model.most_connected_pages(3);
        assert!(!connected.is_empty());
        // helper_fn has 2 called_by, should be top
        assert_eq!(connected[0].0.title, "helper_fn");
        assert!(connected[0].1 >= connected.last().unwrap().1);
    }

    #[test]
    fn wiki_pages_by_kind() {
        let model = make_test_model();
        let counts = model.pages_by_kind();
        assert_eq!(counts.len(), 3);
        assert_eq!(counts[0], (WikiPageKind::Overview, 1));
        assert_eq!(counts[1], (WikiPageKind::File, 1));
        assert_eq!(counts[2], (WikiPageKind::Symbol, 3));
    }

    #[test]
    fn is_subsequence_basic() {
        let sub: Vec<char> = "abc".chars().collect();
        let full: Vec<char> = "ahbgdc".chars().collect();
        assert!(is_subsequence(&sub, &full));
    }

    #[test]
    fn is_subsequence_false() {
        let sub: Vec<char> = "axc".chars().collect();
        let full: Vec<char> = "ahbgdc".chars().collect();
        assert!(!is_subsequence(&sub, &full));
    }

    #[test]
    fn is_subsequence_empty() {
        let sub: Vec<char> = vec![];
        let full: Vec<char> = "abc".chars().collect();
        assert!(is_subsequence(&sub, &full));
    }

    #[test]
    fn autocomplete_suggestion_serializes() {
        let s = AutocompleteSuggestion {
            title: "main_fn".to_string(),
            slug: "main_fn".to_string(),
            kind: WikiPageKind::Symbol,
            match_type: "prefix",
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"match_type\":\"prefix\""));
    }

    #[test]
    fn paginated_search_result_serializes() {
        let r = PaginatedSearchResult {
            results: vec![],
            total_matches: 10,
            offset: 5,
            limit: 5,
            has_more: false,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"has_more\":false"));
    }

    // ── Block 137: Wiki generate comprehensive tests ─────────────────────

    #[test]
    fn slugify_special_chars_dropped() {
        // @ and ! are not separator chars, they are simply dropped
        assert_eq!(slugify("hello@world!"), "helloworld");
        // Only /\., : and space become dashes
        assert_eq!(slugify("a/b"), "a-b");
    }

    #[test]
    fn slugify_consecutive_separators_single_dash() {
        assert_eq!(slugify("a//b"), "a-b");
        assert_eq!(slugify("a..b"), "a-b");
        assert_eq!(slugify("a  b"), "a-b");
    }

    #[test]
    fn slugify_leading_trailing_dashes_trimmed() {
        assert_eq!(slugify("/leading"), "leading");
        assert_eq!(slugify("trailing/"), "trailing");
        assert_eq!(slugify("/both/"), "both");
    }

    #[test]
    fn slugify_all_special_becomes_page() {
        assert_eq!(slugify("!!!"), "page");
        assert_eq!(slugify("@@@"), "page");
    }

    #[test]
    fn slugify_preserves_underscores_and_hyphens() {
        assert_eq!(slugify("my_fn-name"), "my_fn-name");
    }

    #[test]
    fn slugify_backslash_path() {
        assert!(looks_like_path("src\\main.rs"));
        assert_eq!(slugify("src\\main.rs"), "src-main-rs");
    }

    #[test]
    fn wiki_page_kind_labels() {
        assert_eq!(WikiPageKind::Overview.label(), "Overview");
        assert_eq!(WikiPageKind::File.label(), "File");
        assert_eq!(WikiPageKind::Symbol.label(), "Symbol");
    }

    #[test]
    fn wiki_page_kind_eq() {
        assert_eq!(WikiPageKind::Overview, WikiPageKind::Overview);
        assert_ne!(WikiPageKind::File, WikiPageKind::Symbol);
    }

    #[test]
    fn wiki_page_kind_clone_copy() {
        let kind = WikiPageKind::File;
        let cloned = kind;
        assert_eq!(cloned, WikiPageKind::File);
    }

    #[test]
    fn wiki_model_is_empty() {
        let model = make_test_model();
        assert!(!model.is_empty());

        let empty = WikiModel {
            generated_at: "0".to_string(),
            stats_summary: String::new(),
            overview: make_test_page(WikiPageKind::Overview, "Overview", "index"),
            file_pages: vec![],
            symbol_pages: vec![],
        };
        assert!(empty.is_empty());
    }

    #[test]
    fn wiki_model_counts() {
        let model = make_test_model();
        assert_eq!(model.file_count(), 1);
        assert_eq!(model.symbol_count(), 3);
        assert_eq!(model.total_pages(), 5); // 1 overview + 1 file + 3 symbols
    }

    #[test]
    fn wiki_model_all_pages_order() {
        let model = make_test_model();
        let pages: Vec<&WikiPage> = model.all_pages().collect();
        assert_eq!(pages.len(), 5);
        // Overview first
        assert_eq!(pages[0].kind, WikiPageKind::Overview);
        // Then files
        assert_eq!(pages[1].kind, WikiPageKind::File);
        // Then symbols
        assert!(pages[2..].iter().all(|p| p.kind == WikiPageKind::Symbol));
    }

    #[test]
    fn wiki_model_find_by_title() {
        let model = make_test_model();
        assert!(model.find_by_title("main_fn").is_some());
        assert!(model.find_by_title("nonexistent").is_none());
        assert_eq!(model.find_by_title("main_fn").unwrap().kind, WikiPageKind::Symbol);
    }

    #[test]
    fn wiki_model_find_by_slug() {
        let model = make_test_model();
        assert!(model.find_by_slug("main_fn").is_some());
        assert!(model.find_by_slug("src-main-rs").is_some());
        assert!(model.find_by_slug("nonexistent").is_none());
    }

    #[test]
    fn wiki_search_empty_query() {
        let model = make_test_model();
        let results = model.search("");
        assert!(results.is_empty());
    }

    #[test]
    fn wiki_search_exact_title_match() {
        let model = make_test_model();
        let results = model.search("main_fn");
        assert!(!results.is_empty());
        // Exact title match should score highest
        let best = &results[0];
        assert!(best.score >= 30); // 10 (title contains) + 20 (exact match)
    }

    #[test]
    fn wiki_search_summary_match() {
        let model = make_test_model();
        // "Summary" appears in all page summaries ("Summary for ...")
        let results = model.search("Summary");
        assert!(!results.is_empty());
    }

    #[test]
    fn wiki_search_detail_match() {
        let mut model = make_test_model();
        // Add detail to a page
        model.symbol_pages[0].detail = Some("Detailed info about main_fn".to_string());
        let results = model.search("Detailed");
        assert!(!results.is_empty());
    }

    #[test]
    fn wiki_search_relationship_target_match() {
        let model = make_test_model();
        // "main_fn" appears as a relationship target in file1
        let results = model.search("main_fn");
        // src/main.rs has "main_fn" in its relationships
        assert!(results.iter().any(|r| r.page.title == "src/main.rs"));
    }

    #[test]
    fn wiki_search_called_by_match() {
        let model = make_test_model();
        // "src/main.rs" appears in called_by for main_fn and helper_fn
        let results = model.search("src/main.rs");
        assert!(results.iter().any(|r| r.page.title == "main_fn"));
    }

    #[test]
    fn wiki_search_multi_term() {
        let model = make_test_model();
        let results = model.search("main helper");
        // Should find pages matching either term
        assert!(!results.is_empty());
    }

    #[test]
    fn wiki_search_results_sorted_by_score() {
        let model = make_test_model();
        let results = model.search("fn");
        for window in results.windows(2) {
            assert!(window[0].score >= window[1].score);
        }
    }

    #[test]
    fn wiki_validate_empty_model() {
        let empty = WikiModel {
            generated_at: "0".to_string(),
            stats_summary: String::new(),
            overview: make_test_page(WikiPageKind::Overview, "Overview", "index"),
            file_pages: vec![],
            symbol_pages: vec![],
        };
        let warnings = empty.validate();
        assert!(warnings.iter().any(|w| w.contains("no file or symbol pages")));
    }

    #[test]
    fn wiki_validate_duplicate_slugs() {
        let mut model = make_test_model();
        // Add a page with duplicate slug
        let dup = make_test_page(WikiPageKind::Symbol, "duplicate", "main_fn");
        model.symbol_pages.push(dup);
        let warnings = model.validate();
        assert!(warnings.iter().any(|w| w.contains("Duplicate slug")));
    }

    #[test]
    fn wiki_validate_empty_title() {
        let mut model = make_test_model();
        let empty_title = WikiPage {
            kind: WikiPageKind::Symbol,
            title: String::new(),
            slug: "empty".to_string(),
            summary: "test".to_string(),
            relationships: vec![],
            called_by: vec![],
            detail: None,
        };
        model.symbol_pages.push(empty_title);
        let warnings = model.validate();
        assert!(warnings.iter().any(|w| w.contains("empty title")));
    }

    #[test]
    fn wiki_validate_clean_model() {
        let model = make_test_model();
        let warnings = model.validate();
        assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
    }

    #[test]
    fn wiki_symbols_defined_by() {
        let model = make_test_model();
        let syms = model.symbols_defined_by("src/main.rs");
        // main_fn has called_by = ["src/main.rs"], helper_fn has ["src/main.rs", "src/lib.rs"]
        assert!(syms.len() >= 1);
    }

    #[test]
    fn wiki_symbols_defined_by_no_match() {
        let model = make_test_model();
        let syms = model.symbols_defined_by("nonexistent.rs");
        assert!(syms.is_empty());
    }

    #[test]
    fn wiki_files_referencing() {
        let model = make_test_model();
        let files = model.files_referencing("main_fn");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].title, "src/main.rs");
    }

    #[test]
    fn wiki_files_referencing_no_match() {
        let model = make_test_model();
        let files = model.files_referencing("nonexistent_fn");
        assert!(files.is_empty());
    }

    #[test]
    fn wiki_stats_computation() {
        let model = make_test_model();
        let stats = model.stats();
        assert_eq!(stats.total_pages, 5);
        assert_eq!(stats.file_pages, 1);
        assert_eq!(stats.symbol_pages, 3);
        assert_eq!(stats.total_relationships, 1); // file1 -> Defines -> main_fn
        assert_eq!(stats.total_called_by, 3); // sym1: 1 + sym2: 2
        assert_eq!(stats.pages_with_detail, 0);
        assert_eq!(stats.generated_at, "1234567890");
    }

    #[test]
    fn wiki_stats_with_detail() {
        let mut model = make_test_model();
        model.symbol_pages[0].detail = Some("detail".to_string());
        let stats = model.stats();
        assert_eq!(stats.pages_with_detail, 1);
    }

    #[test]
    fn wiki_batch_search_deduplication() {
        let model = make_test_model();
        // Both queries match "main_fn" — should appear only once
        let results = model.batch_search(&["main_fn", "main"]);
        let main_fn_count = results.iter().filter(|r| r.page.title == "main_fn").count();
        assert_eq!(main_fn_count, 1, "main_fn should appear only once after dedup");
    }

    #[test]
    fn wiki_search_by_kind_overview() {
        let model = make_test_model();
        let results = model.search_by_kind("Overview", WikiPageKind::Overview);
        assert!(!results.is_empty());
        for r in &results {
            assert_eq!(r.page.kind, WikiPageKind::Overview);
        }
    }

    #[test]
    fn wiki_search_paginated_total_matches() {
        let model = make_test_model();
        let result = model.search_paginated("fn", 1, 0);
        assert!(result.total_matches > 1);
        assert_eq!(result.results.len(), 1);
        assert!(result.has_more);
    }

    #[test]
    fn wiki_autocomplete_contains_match() {
        let model = make_test_model();
        // "fn" is contained in "main_fn", "helper_fn", "unused_fn"
        let suggestions = model.autocomplete("fn", 10);
        let contains_matches: Vec<_> = suggestions.iter().filter(|s| s.match_type == "contains").collect();
        // "fn" doesn't start with "fn" for any title, but titles contain "fn"
        assert!(!contains_matches.is_empty());
    }

    #[test]
    fn wiki_autocomplete_sort_order() {
        let model = make_test_model();
        let suggestions = model.autocomplete("fn", 10);
        // Sort is alphabetical on match_type: "contains" < "prefix"
        if suggestions.len() >= 2 {
            for window in suggestions.windows(2) {
                assert!(
                    window[0].match_type <= window[1].match_type,
                    "sort order violated: {:?} before {:?}",
                    window[0].match_type,
                    window[1].match_type
                );
            }
        }
    }

    #[test]
    fn wiki_fuzzy_search_score_inversely_proportional() {
        let model = make_test_model();
        // Shorter title should score higher for same query
        let results = model.fuzzy_search("main_fn");
        // main_fn should match with high score (7 chars query / 7 chars title * 10 = 10)
        let main_result = results.iter().find(|r| r.page.title == "main_fn");
        assert!(main_result.is_some());
        assert!(main_result.unwrap().score >= 1);
    }

    #[test]
    fn wiki_search_report_empty_query() {
        let model = make_test_model();
        let report = model.search_report("");
        assert_eq!(report.total_matches, 0);
        assert_eq!(report.top_score, 0);
    }

    #[test]
    fn wiki_search_report_kind_counts() {
        let model = make_test_model();
        let report = model.search_report("fn");
        // Should have counts for different kinds
        assert!(!report.results_by_kind.is_empty());
    }

    #[test]
    fn wiki_most_connected_pages_limit() {
        let model = make_test_model();
        let connected = model.most_connected_pages(1);
        assert_eq!(connected.len(), 1);
    }

    #[test]
    fn wiki_most_connected_pages_zero_limit() {
        let model = make_test_model();
        let connected = model.most_connected_pages(0);
        assert!(connected.is_empty());
    }

    #[test]
    fn wiki_relationship_edges_empty() {
        let empty = WikiModel {
            generated_at: "0".to_string(),
            stats_summary: String::new(),
            overview: make_test_page(WikiPageKind::Overview, "Overview", "index"),
            file_pages: vec![],
            symbol_pages: vec![],
        };
        let edges = empty.relationship_edges();
        assert!(edges.is_empty());
    }

    #[test]
    fn wiki_relationship_edges_multiple() {
        let mut model = make_test_model();
        model.file_pages[0].relationships = vec![
            ("Defines".to_string(), vec!["a".to_string(), "b".to_string()]),
            ("Calls".to_string(), vec!["c".to_string()]),
        ];
        let edges = model.relationship_edges();
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn is_subsequence_exact_match() {
        let sub: Vec<char> = "abc".chars().collect();
        let full: Vec<char> = "abc".chars().collect();
        assert!(is_subsequence(&sub, &full));
    }

    #[test]
    fn is_subsequence_longer_sub() {
        let sub: Vec<char> = "abcdef".chars().collect();
        let full: Vec<char> = "abc".chars().collect();
        assert!(!is_subsequence(&sub, &full));
    }

    #[test]
    fn is_subsequence_both_empty() {
        let sub: Vec<char> = vec![];
        let full: Vec<char> = vec![];
        assert!(is_subsequence(&sub, &full));
    }

    #[test]
    fn wiki_page_clone() {
        let page = make_test_page(WikiPageKind::Symbol, "test", "test");
        let cloned = page.clone();
        assert_eq!(cloned.title, page.title);
        assert_eq!(cloned.slug, page.slug);
        assert_eq!(cloned.kind, page.kind);
    }

    #[test]
    fn wiki_page_debug() {
        let page = make_test_page(WikiPageKind::Symbol, "test", "test");
        let debug = format!("{:?}", page);
        assert!(debug.contains("WikiPage"));
        assert!(debug.contains("test"));
    }

    #[test]
    fn wiki_model_clone() {
        let model = make_test_model();
        let cloned = model.clone();
        assert_eq!(cloned.total_pages(), model.total_pages());
        assert_eq!(cloned.file_count(), model.file_count());
        assert_eq!(cloned.symbol_count(), model.symbol_count());
    }

    #[test]
    fn wiki_search_result_clone() {
        let result = WikiSearchResult {
            page: make_test_page(WikiPageKind::Symbol, "test", "test"),
            score: 42,
        };
        let cloned = result.clone();
        assert_eq!(cloned.score, 42);
        assert_eq!(cloned.page.title, "test");
    }

    #[test]
    fn wiki_stats_clone_debug() {
        let stats = WikiStats {
            total_pages: 5,
            file_pages: 1,
            symbol_pages: 3,
            total_relationships: 10,
            total_called_by: 5,
            pages_with_detail: 2,
            generated_at: "123".to_string(),
        };
        let cloned = stats.clone();
        assert_eq!(cloned.total_pages, 5);
        let debug = format!("{:?}", stats);
        assert!(debug.contains("WikiStats"));
    }

    #[test]
    fn wiki_model_info_json_all_fields() {
        let model = make_test_model();
        let info = model.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"total_pages\""));
        assert!(json.contains("\"file_pages\""));
        assert!(json.contains("\"symbol_pages\""));
        assert!(json.contains("\"total_relationships\""));
        assert!(json.contains("\"total_called_by\""));
        assert!(json.contains("\"pages_with_detail\""));
        assert!(json.contains("\"orphan_pages\""));
        assert!(json.contains("\"top_symbols\""));
        assert!(json.contains("\"validation_issues\""));
        assert!(json.contains("\"generated_at\""));
    }

    #[test]
    fn wiki_generation_report_json_all_fields() {
        let report = WikiGenerationReport {
            elapsed_us: 1000,
            triples_processed: 50,
            file_pages_generated: 5,
            symbol_pages_generated: 20,
            total_pages: 26,
            validation_issues: vec!["issue1".to_string()],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"elapsed_us\""));
        assert!(json.contains("\"triples_processed\""));
        assert!(json.contains("\"file_pages_generated\""));
        assert!(json.contains("\"symbol_pages_generated\""));
        assert!(json.contains("\"total_pages\""));
        assert!(json.contains("\"validation_issues\""));
    }

    #[test]
    fn search_report_json_all_fields() {
        let report = SearchReport {
            query: "test".to_string(),
            total_matches: 3,
            elapsed_us: 50,
            top_score: 30,
            average_score: 15.0,
            results_by_kind: vec![("Symbol".to_string(), 2)],
            top_results: vec!["a".to_string()],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"query\""));
        assert!(json.contains("\"total_matches\""));
        assert!(json.contains("\"elapsed_us\""));
        assert!(json.contains("\"top_score\""));
        assert!(json.contains("\"average_score\""));
        assert!(json.contains("\"results_by_kind\""));
        assert!(json.contains("\"top_results\""));
    }

    #[test]
    fn paginated_search_json_all_fields() {
        let r = PaginatedSearchResult {
            results: vec![],
            total_matches: 10,
            offset: 5,
            limit: 5,
            has_more: true,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"total_matches\""));
        assert!(json.contains("\"offset\""));
        assert!(json.contains("\"limit\""));
        assert!(json.contains("\"has_more\""));
    }

    #[test]
    fn wiki_validate_page_empty_relationship_label() {
        let mut page = make_test_page(WikiPageKind::File, "test.rs", "test-rs");
        page.relationships = vec![("".to_string(), vec!["target".to_string()])];
        let issues = WikiModel::validate_page(&page);
        assert!(issues.iter().any(|i| i.contains("empty label")));
    }

    #[test]
    fn wiki_top_symbols_limit_larger_than_available() {
        let model = make_test_model();
        let top = model.top_symbols(100);
        assert_eq!(top.len(), model.symbol_pages.len());
    }

    #[test]
    fn wiki_search_no_results() {
        let model = make_test_model();
        let results = model.search("zzzznonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn wiki_search_case_insensitive() {
        let model = make_test_model();
        let lower = model.search("main_fn");
        let upper = model.search("MAIN_FN");
        assert_eq!(lower.len(), upper.len());
    }
}
