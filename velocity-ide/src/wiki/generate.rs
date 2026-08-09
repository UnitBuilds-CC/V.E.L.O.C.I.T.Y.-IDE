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
