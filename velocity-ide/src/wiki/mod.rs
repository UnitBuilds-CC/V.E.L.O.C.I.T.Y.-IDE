//! Sitemap-powered wiki generation.
//!
//! Builds a navigable model of Overview / File / Symbol pages from the
//! workspace [`SiteMap`](crate::site_map::SiteMap) and can export that model
//! as interlinked Markdown (Qodo-style) suitable for committing to git.
//!
//! # Architecture
//!
//! The wiki generation pipeline has three phases:
//!
//! 1. **Index** ([`index`]) — Deterministic pre-indexing of symbols, imports,
//!    and call graphs. Zero LLM calls. Produces compact summaries ~85% smaller
//!    than raw source.
//!
//! 2. **Cache** ([`cache`]) — Content-keyed cache that stores LLM-generated
//!    details. Re-scans skip unchanged files entirely.
//!
//! 3. **Generate** ([`generate`]) — Builds the wiki model from site map triples
//!    and index data, with optional LLM-powered detail generation.

pub mod cache;
pub mod generate;
pub mod html_export;
pub mod import_resolver;
pub mod index;
pub mod markdown;
pub mod pagerank;

#[cfg(test)]
mod tests;

pub use cache::{RegenerationState, WikiCache};
pub use generate::{build_wiki, build_wiki_enhanced, build_wiki_incremental};
pub use generate::{enrich_with_details, enrich_with_structural_details, sitemap_coverage_report};
pub use generate::{AutocompleteSuggestion, PaginatedSearchResult, SearchReport, WikiSearchResult};
pub use generate::{EnhancedWikiResult, SitemapCoverageReport, WikiModel, WikiPage, WikiPageKind};
pub use html_export::export_html;
pub use import_resolver::ImportResolver;
pub use index::{index_workspace, FileIndex, SourceLanguage};
pub use markdown::{export_github_pages, export_markdown, render_page_markdown};
pub use pagerank::PageRankScores;
