//! Sitemap-powered wiki generation.
//!
//! Builds a navigable model of Overview / File / Symbol pages from the
//! workspace [`SiteMap`](crate::site_map::SiteMap) and can export that model
//! as interlinked Markdown (Qodo-style) suitable for committing to git.

pub mod generate;
pub mod markdown;

#[cfg(test)]
mod tests;

pub use generate::{build_wiki, WikiModel, WikiPage, WikiPageKind};
pub use markdown::{export_markdown, render_page_markdown};
