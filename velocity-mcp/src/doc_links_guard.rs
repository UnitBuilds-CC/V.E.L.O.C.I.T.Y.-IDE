//! Binds every relative link in the shipped prose to a file in this repository.
//!
//! A link is the only part of a document a reader is meant to click, and it is
//! the part that rots silently: renaming a page, moving a directory or deleting
//! a screenshot leaves the sentence around it reading perfectly. Nothing in CI
//! notices, because nothing in a `.md` file is compiled. So the claim each link
//! makes - *there is something at this path* - is checked against the tree:
//! every markdown file in the repo is scanned for inline links and images, and
//! each target that names a file has to resolve to one.
//!
//! The exclusions, each with a reason:
//!
//! - build, cache and downloaded directories (`target`, `node_modules`, `dist`,
//!   `build`, `models`, `.velocity_testing`): their `.md` files are vendored
//!   from dependencies or fetched with model weights, so they are nobody's prose
//!   and nobody repairs them from here;
//! - `.qoder`: the generated wiki. It is excluded because a generated tree is
//!   rewritten under names no author controls - which is how the one dead link
//!   this guard found came to be dead. `docs/TEST_SUITE_SPECIFICATION.md`
//!   pointed into `.qoder/repowiki/en/content/Architecture/…` after that layout
//!   was regenerated away. Shipped docs reference the hand-maintained `.wiki/`
//!   instead, and grading the generated tree would turn a green suite red on a
//!   commit that changed no document;
//! - off-site targets, same-page anchors and `#section` jumps: not claims about
//!   this repository. A `file:` or a `C:\…` target *is* kept, because a document
//!   that names one machine's filesystem is making a claim this guard can
//!   falsify, and did.
//!
//! Reference-style links (`[text][ref]` plus a `[ref]: url` definition) are not
//! graded: none exists in the corpus today, and a parser that silently skips a
//! form is worse than one that says which forms it reads.

use std::path::{Path, PathBuf};

/// Directories whose markdown files are not this project's prose.
const NOT_PROSE: [&str; 7] = [
    "target",
    "node_modules",
    "dist",
    "build",
    ".git",
    ".qoder",
    ".velocity_testing",
];

/// Downloaded weights arrive with a card, and the card belongs to whoever
/// published the weights.
const NOT_PROSE_DOWNLOADED: [&str; 1] = ["models"];

/// Targets that point somewhere other than this repository. `file:` is
/// deliberately absent: a document that links to a local filesystem is naming
/// one machine, which is exactly the claim this guard can falsify.
const OFF_SITE: [&str; 7] = [
    "http://", "https://", "ftp://", "ftps://", "mailto:", "tel:", "data:",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("velocity-mcp sits at the repo root")
        .to_path_buf()
}

/// The absolute path of a repository-relative `/`-form path.
fn at(root: &Path, repo_relative: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for segment in repo_relative.split('/').filter(|s| !s.is_empty()) {
        path.push(segment);
    }
    path
}

/// `text` with the parts that are code rather than prose removed: fenced blocks
/// by line, inline spans within a line. A tutorial has to be allowed to show a
/// link it does not mean the reader to follow.
fn prose_only(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<char> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let marker = fence_marker(trimmed);
        match fence {
            None => {
                if let Some(c) = marker {
                    fence = Some(c);
                } else {
                    out.push_str(&blank_inline_code(line));
                }
                out.push('\n');
            }
            Some(opened) => {
                if marker == Some(opened) && trimmed.chars().all(|c| c == opened) {
                    fence = None;
                }
                out.push('\n');
            }
        }
    }
    out
}

/// The fence character when a line opens or closes a code fence.
fn fence_marker(trimmed: &str) -> Option<char> {
    let first = trimmed.chars().next()?;
    if (first == '`' || first == '~') && trimmed.chars().take_while(|c| *c == first).count() >= 3 {
        Some(first)
    } else {
        None
    }
}

/// Empty out `` `code` `` runs, keeping the rest of the line. A run of N
/// backticks opens a span that only a run of exactly N closes, so a stray tick
/// inside prose does not erase the sentence after it.
fn blank_inline_code(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    let mut span = 0usize;
    while i < chars.len() {
        if chars[i] == '`' {
            let start = i;
            while i < chars.len() && chars[i] == '`' {
                i += 1;
            }
            let run = i - start;
            if span == 0 {
                span = run;
            } else if span == run {
                span = 0;
            }
            continue;
        }
        if span == 0 {
            out.push(chars[i]);
        }
        i += 1;
    }
    out
}

/// The raw `(...)` payload of every inline link and image in `text`.
///
/// Scanning a `Vec<char>` rather than slicing keeps a non-ASCII document from
/// panicking the guard on a multi-byte boundary - a confusing way to report that
/// a link is fine.
fn link_targets(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != ']' || i + 1 >= chars.len() || chars[i + 1] != '(' {
            i += 1;
            continue;
        }
        let Some(start) = chars[..i].iter().rposition(|c| *c == '[') else {
            i += 1;
            continue;
        };
        let escaped = start > 0 && chars[start - 1] == '\\';
        if escaped || chars[start + 1..i].contains(&']') {
            i += 1;
            continue;
        }
        let Some(offset) = chars[i + 2..].iter().position(|c| *c == ')') else {
            break;
        };
        found.push(chars[i + 2..i + 2 + offset].iter().collect());
        i = i + 2 + offset + 1;
    }
    found
}

/// What a target claims, after the `<…>` and `"title"` forms are taken apart.
enum Reading {
    /// Another site, or a `#section` jump inside the same page: not this repo.
    NotAFile,
    /// One machine's filesystem - a `\`-separated or drive-letter path. Legal in
    /// a terminal, meaningless in a rendered document.
    OneMachine,
    /// A path within the repository, in `/` form and percent-decoded.
    InRepo(String),
}

fn read_target(raw: &str) -> Reading {
    let trimmed = raw.trim();
    let body = if let Some(rest) = trimmed.strip_prefix('<') {
        rest.split('>').next().unwrap_or("")
    } else {
        trimmed.split_whitespace().next().unwrap_or("")
    };
    if body.is_empty() {
        return Reading::NotAFile;
    }
    let lower = body.to_lowercase();
    if OFF_SITE.iter().any(|scheme| lower.starts_with(scheme)) {
        return Reading::NotAFile;
    }
    let drive = body.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && body.chars().nth(1) == Some(':');
    if body.contains('\\') || drive {
        return Reading::OneMachine;
    }
    let path = body.split('#').next().unwrap_or("");
    if path.is_empty() {
        return Reading::NotAFile;
    }
    Reading::InRepo(percent_decode(path))
}

/// `%20` and friends, since a path with a space in it is spelled that way in a
/// link. An invalid escape is left as written rather than dropped, so the
/// resolution below reports what the reader would actually be sent to.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The repository-relative `/`-form path a target resolves to, or the reason it
/// resolves to nothing at all.
fn resolve(doc: &str, path: &str) -> Result<String, String> {
    let mut segments: Vec<&str> = Vec::new();
    if let Some(rooted) = path.strip_prefix('/') {
        segments.extend(rooted.split('/').filter(|s| !s.is_empty() && *s != "."));
    } else {
        if let Some((directory, _)) = doc.rsplit_once('/') {
            segments.extend(directory.split('/').filter(|s| !s.is_empty() && *s != "."));
        }
        for segment in path.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    if segments.pop().is_none() {
                        return Err("climbs out of the repository".to_string());
                    }
                }
                other => segments.push(other),
            }
        }
    }
    if segments.is_empty() {
        return Err("resolves to the repository directory itself".to_string());
    }
    Ok(segments.join("/"))
}

/// A link that does not land.
#[derive(Debug)]
pub struct DeadLink {
    /// The document that carries it, repository-relative.
    pub doc: String,
    /// The target exactly as written, so the reader of the failure finds the line.
    pub target: String,
    /// Where it was looked for, or why there was nowhere to look.
    pub detail: String,
}

/// Grade `text` as if it were the document at repository path `doc`.
fn dead_links_in(doc: &str, text: &str, root: &Path) -> Vec<DeadLink> {
    let mut dead = Vec::new();
    for raw in link_targets(&prose_only(text)) {
        match read_target(raw.trim()) {
            Reading::NotAFile => {}
            Reading::OneMachine => dead.push(DeadLink {
                doc: doc.to_string(),
                target: raw,
                detail: "names one machine's filesystem, not a file in this repository".to_string(),
            }),
            Reading::InRepo(path) => match resolve(doc, &path) {
                Err(why) => dead.push(DeadLink {
                    doc: doc.to_string(),
                    target: raw,
                    detail: why,
                }),
                Ok(resolved) => {
                    if !at(root, &resolved).exists() {
                        dead.push(DeadLink {
                            doc: doc.to_string(),
                            target: raw,
                            detail: format!("no {resolved} in the repository"),
                        });
                    }
                }
            },
        }
    }
    dead
}

/// Every prose markdown file, as repository-relative `/`-form paths. A
/// directory that will not open is named rather than passed over - see
/// [`corpus`].
fn prose_files(root: &Path, dir: &Path, out: &mut Vec<String>, stuck: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        if let Ok(relative) = dir.strip_prefix(root) {
            stuck.push(format!(
                "{}: the directory will not open",
                relative.to_string_lossy().replace('\\', "/")
            ));
        }
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if NOT_PROSE.contains(&name) || NOT_PROSE_DOWNLOADED.contains(&name) {
            continue;
        }
        if path.is_dir() {
            prose_files(root, &path, out, stuck);
        } else if name.to_lowercase().ends_with(".md") {
            if let Ok(relative) = path.strip_prefix(root) {
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

/// The documents this project ships, and the ones the walk could not read.
struct Corpus {
    /// `(repository-relative path, text)`, sorted by path.
    docs: Vec<(String, String)>,
    /// Anything that was in the way of the sweep: a directory that would not
    /// open, a file that would not read, a file that is not valid UTF-8.
    stuck: Vec<String>,
}

/// Read the corpus.
///
/// This repository lives under a sync root, where a file whose contents have
/// not been fetched and a folder that is still enumerating both fail to open,
/// and a markdown file saved in somebody's legacy encoding is not valid UTF-8.
/// A sweep that dropped those would report the same green as a corpus with no
/// rot, so they are carried out separately and a test refuses them.
fn corpus() -> Corpus {
    let root = repo_root();
    let mut files = Vec::new();
    let mut stuck = Vec::new();
    prose_files(&root, &root, &mut files, &mut stuck);
    files.sort();
    let mut docs = Vec::with_capacity(files.len());
    for file in files {
        match std::fs::read_to_string(at(&root, &file)) {
            Ok(text) => docs.push((file, text)),
            Err(why) => stuck.push(format!("{file}: {why}")),
        }
    }
    Corpus { docs, stuck }
}

/// Every dead link in the corpus.
fn dead_links() -> Vec<DeadLink> {
    let root = repo_root();
    corpus()
        .docs
        .iter()
        .flat_map(|(doc, text)| dead_links_in(doc, text, &root))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_shipped_doc_links_a_file_that_is_not_there() {
        let dead = dead_links();
        assert!(
            dead.is_empty(),
            "{} relative link(s) in the shipped markdown lead nowhere: {} - a renamed page, a \
             deleted screenshot or a path into a generated tree that has since been regenerated \
             under other names all read fine until a reader clicks, so either repair the link or \
             delete the sentence that depends on it",
            dead.len(),
            dead.iter()
                .map(|d| format!("{} -> {} ({})", d.doc, d.target, d.detail))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    /// A clean sweep is only evidence if there was something to sweep. Without
    /// this, a walker that stopped finding files or a link parser that stopped
    /// matching would report the same green as a corpus with no rot.
    #[test]
    fn the_corpus_being_graded_is_the_shipped_docs() {
        let read = corpus();
        assert!(
            read.stuck.is_empty(),
            "the sweep skipped {} document(s) it could not read: {} - a placeholder that has not \
             fetched, a folder still enumerating or a file saved in a legacy encoding all leave a \
             link inside them ungraded, and a green that means did not look is not a green",
            read.stuck.len(),
            read.stuck.join("; ")
        );
        let docs = read.docs;
        let names: Vec<&str> = docs.iter().map(|(name, _)| name.as_str()).collect();
        for expected in [
            "README.md",
            "USER_GUIDE.md",
            "CHANGELOG.md",
            ".wiki/index.md",
            "docs/TEST_SUITE_SPECIFICATION.md",
        ] {
            assert!(names.contains(&expected), "{expected} was not walked");
        }
        let graded: usize = docs
            .iter()
            .map(|(_, text)| {
                link_targets(&prose_only(text))
                    .iter()
                    .filter(|raw| !matches!(read_target(raw.trim()), Reading::NotAFile))
                    .count()
            })
            .sum();
        assert!(
            docs.len() >= 50,
            "the walk read {} markdown files; the shipped corpus is larger, so the walker - not \
             the corpus - has changed",
            docs.len()
        );
        assert!(
            graded >= 150,
            "the parser read {graded} file-level links across {} documents; the corpus carries \
             hundreds, so a parsing regression is being mistaken for a clean sweep",
            docs.len()
        );
    }

    /// The positive control: the same grader, on text where the answer is known.
    /// Every shape the shipped docs use is here, including the two the corpus no
    /// longer contains - percent-encoding and a `<…>`-wrapped target - so a
    /// regression in either is caught before an author has to write it.
    #[test]
    fn a_dead_link_is_found_and_prose_is_not() {
        let root = repo_root();
        let text = r#"
See [the guide](USER_GUIDE.md) and [the manifest](Cargo.toml).
[Dead](docs/definitely-not-a-real-page.md), and [missing, encoded](docs/nope%20here.md).
Wrapped in angles: [angles](<docs/a file with spaces.md>).
Off this repository: [site](https://example.com/x), [mail](mailto:a@b.c), [here](#section).
A host path: ![shot](C:\Users\someone\AppData\Local\Temp\x.png)
Escaped and inert: \[not a link](docs/inert.md)
```text
[fence is prose](docs/inside-a-fence.md)
```
Neither is code: `[span is prose](docs/inside-a-span.md)` is how you write one.
"#;
        let dead: Vec<String> = dead_links_in("README.md", text, &root)
            .into_iter()
            .map(|d| format!("{} ({})", d.target, d.detail))
            .collect();
        assert_eq!(
            dead,
            vec![
                "docs/definitely-not-a-real-page.md (no docs/definitely-not-a-real-page.md in the repository)",
                "docs/nope%20here.md (no docs/nope here.md in the repository)",
                "<docs/a file with spaces.md> (no docs/a file with spaces.md in the repository)",
                "C:\\Users\\someone\\AppData\\Local\\Temp\\x.png (names one machine's filesystem, not a file in this repository)",
            ],
            "the grader read the wrong set of links"
        );
    }

    /// The other half of the control: the links that must survive parsing.
    #[test]
    fn a_live_link_survives_every_legal_spelling() {
        let root = repo_root();
        for (doc, text) in [
            ("README.md", "[manifest](Cargo.toml \"the build manifest\")"),
            ("README.md", "[here](./Cargo.toml)"),
            ("README.md", "[a directory](docs/)"),
            ("docs/x.md", "[up one](../Cargo.toml)"),
            ("README.md", "[rooted](/Cargo.toml)"),
            ("README.md", "[angles](<Cargo.toml>)"),
            ("README.md", "[encoded](Cargo%2Etoml)"),
        ] {
            let dead = dead_links_in(doc, text, &root);
            assert!(dead.is_empty(), "{doc}: {text} -> {dead:?}");
        }
    }

    #[test]
    fn a_link_that_climbs_out_is_named_rather_than_looked_up() {
        let root = repo_root();
        let dead = dead_links_in("docs/x.md", "[outside](../../Windows/win.ini)", &root);
        assert_eq!(dead.len(), 1, "{dead:#?}");
        assert_eq!(dead[0].detail, "climbs out of the repository");
    }
}
