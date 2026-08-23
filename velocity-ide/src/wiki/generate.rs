//! Builds a [`WikiModel`] from a [`SiteMap`].

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::site_map::SiteMap;

/// The flavour of a wiki page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug)]
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
}

/// A single search result with relevance score.
#[derive(Clone, Debug)]
pub struct WikiSearchResult {
    pub page: WikiPage,
    pub score: u32,
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
}
