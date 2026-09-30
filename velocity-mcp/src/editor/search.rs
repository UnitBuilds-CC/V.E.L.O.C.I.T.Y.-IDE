use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

/// Matching options shared by workspace search and replace, mirroring the
/// in-file find bar's toggles (match case / whole word / regular expression).
/// The default is a case-insensitive substring search — the historical
/// behavior of `project_search`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
}

/// Return the byte ranges of every match of `query` in `text` under `opts`.
/// A malformed regex yields no matches (the UI then shows "No results"),
/// matching the in-file find bar. Kept pure so the toggle matrix is unit-tested
/// independently of the filesystem walk.
pub fn find_matches(text: &str, query: &str, opts: SearchOptions) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    if query.is_empty() {
        return matches;
    }
    if opts.use_regex {
        let re = match crate::editor::regex_engine::Regex::compile(query, !opts.case_sensitive) {
            Ok(r) => r,
            Err(_) => return matches,
        };
        for (start, end) in re.find_all(text) {
            if opts.whole_word && !is_whole_word(text, start, end) {
                continue;
            }
            matches.push((start, end));
        }
        return matches;
    }
    // Literal search. Byte indices line up with `text` for ASCII (the same
    // approximation the in-file find bar makes when case-folding).
    let needle = if opts.case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let hay = if opts.case_sensitive {
        text.to_string()
    } else {
        text.to_lowercase()
    };
    let nlen = needle.len();
    let mut start = 0;
    while let Some(pos) = hay[start..].find(&needle) {
        let abs = start + pos;
        let end = abs + nlen;
        if !opts.whole_word || is_whole_word(text, abs, end) {
            matches.push((abs, end));
        }
        start = abs + 1;
        if start >= hay.len() {
            break;
        }
    }
    matches
}

fn is_whole_word(text: &str, start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !text.as_bytes()[start - 1].is_ascii_alphanumeric();
    let after_ok = end >= text.len() || !text.as_bytes()[end].is_ascii_alphanumeric();
    before_ok && after_ok
}

/// Expand `$0`–`$99` and `${0}`–`${99}` references in a regex replacement
/// template from the whole match and its capture groups (group 1 is
/// `groups[0]`). A `$` not followed by a digit or `{` stays literal, and
/// references the pattern doesn't define expand to the empty string — the
/// same semantics the big competitors use in regex Replace.
pub fn expand_replacement(replacement: &str, whole: &str, groups: &[Option<&str>]) -> String {
    let chars: Vec<char> = replacement.chars().collect();
    let mut out = String::with_capacity(replacement.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '$'
            && i + 1 < chars.len()
            && (chars[i + 1].is_ascii_digit() || chars[i + 1] == '{')
        {
            let braced = chars[i + 1] == '{';
            // Unbraced refs start their digits right at the `$` follower;
            // braced refs skip past the `{`.
            let start = if braced { i + 2 } else { i + 1 };
            let mut j = start;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            let closed = !braced || (j < chars.len() && chars[j] == '}' && j > start);
            if j > start && closed {
                let digits: String = chars[start..j].iter().collect();
                let n: usize = digits.parse().unwrap_or(usize::MAX);
                let text: &str = if n == 0 {
                    whole
                } else {
                    groups.get(n - 1).and_then(|g| *g).unwrap_or("")
                };
                out.push_str(text);
                // Skip past the digits and the closing brace, if any.
                i = if braced { j + 1 } else { j };
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Replace every match of `find` in `text` with `replacement` under `opts`,
/// returning the rewritten text and the number of replacements. In regex mode
/// the replacement template supports `$0`–`$99` / `${n}` capture-group
/// expansion (see [`expand_replacement`]); in literal mode it is inserted
/// verbatim.
pub fn replace_matches(
    text: &str,
    find: &str,
    replacement: &str,
    opts: SearchOptions,
) -> (String, usize) {
    if opts.use_regex {
        if find.is_empty() {
            return (text.to_string(), 0);
        }
        let re = match crate::editor::regex_engine::Regex::compile(find, !opts.case_sensitive) {
            Ok(r) => r,
            Err(_) => return (text.to_string(), 0),
        };
        let mut out = String::with_capacity(text.len());
        let mut last = 0usize;
        let mut count = 0usize;
        for m in re.find_all_caps(text) {
            if opts.whole_word && !is_whole_word(text, m.start, m.end) {
                continue;
            }
            let groups: Vec<Option<&str>> = m
                .groups
                .iter()
                .map(|g| g.map(|(a, b)| &text[a..b]))
                .collect();
            out.push_str(&text[last..m.start]);
            out.push_str(&expand_replacement(
                replacement,
                &text[m.start..m.end],
                &groups,
            ));
            last = m.end;
            count += 1;
        }
        if count == 0 {
            return (text.to_string(), 0);
        }
        out.push_str(&text[last..]);
        return (out, count);
    }
    let matches = find_matches(text, find, opts);
    if matches.is_empty() {
        return (text.to_string(), 0);
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for &(start, end) in &matches {
        out.push_str(&text[last..start]);
        out.push_str(replacement);
        last = end;
    }
    out.push_str(&text[last..]);
    (out, matches.len())
}

/// Match a `/`-separated relative path against a comma-separated glob list
/// ("files to include" filter, same semantics competitors ship): `*` and `?`
/// match within one path segment, `**` spans segments, and a pattern without
/// `/` is matched against the file name. Empty input matches everything.
pub fn path_matches_glob(path: &str, patterns: &str) -> bool {
    let trimmed = patterns.trim();
    if trimmed.is_empty() {
        return true;
    }
    trimmed
        .split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .any(|p| {
            let p = p.replace('\\', "/");
            if !p.contains('/') {
                match_segment(path.rsplit('/').next().unwrap_or(path), &p)
            } else {
                let path_segs: Vec<&str> = path.split('/').collect();
                let pat_segs: Vec<&str> = p.split('/').collect();
                match_globs(&path_segs, &pat_segs)
            }
        })
}

/// Segment-list matcher with `**` (zero or more segments) support.
fn match_globs(path: &[&str], pat: &[&str]) -> bool {
    match pat.first() {
        None => path.is_empty(),
        Some(&"**") => (0..=path.len()).any(|i| match_globs(&path[i..], &pat[1..])),
        Some(p) => {
            !path.is_empty() && match_segment(path[0], p) && match_globs(&path[1..], &pat[1..])
        }
    }
}

/// Single-segment wildcard match (`*`, `?`), case-insensitive like search UIs.
fn match_segment(text: &str, pat: &str) -> bool {
    let t: Vec<char> = text.chars().flat_map(|c| c.to_lowercase()).collect();
    let p: Vec<char> = pat.chars().flat_map(|c| c.to_lowercase()).collect();
    let (mut ti, mut pi) = (0usize, 0usize);
    let (mut star, mut star_ti) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            ti += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Workspace-wide search honoring `opts`. Each hit is the first line in a file
/// containing at least one match. `include` is an optional glob list filtering
/// which files participate (empty = all files).
pub fn project_search(
    root: &Path,
    query: &str,
    max_results: usize,
    opts: SearchOptions,
    include: &str,
) -> Vec<SearchHit> {
    let mut results = Vec::new();
    if query.is_empty() {
        return results;
    }
    walk(root, root, query, max_results, opts, include, &mut results);
    results
}

fn walk(
    root: &Path,
    dir: &Path,
    query: &str,
    max_results: usize,
    opts: SearchOptions,
    include: &str,
    results: &mut Vec<SearchHit>,
) {
    if results.len() >= max_results {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if results.len() >= max_results {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, query, max_results, opts, include, results);
        } else if path.is_file() {
            search_file(root, &path, query, max_results, opts, include, results);
        }
    }
}

fn search_file(
    root: &Path,
    path: &Path,
    query: &str,
    max_results: usize,
    opts: SearchOptions,
    include: &str,
    results: &mut Vec<SearchHit>,
) {
    const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    if !path_matches_glob(&rel, include) {
        return;
    }
    let Ok(meta) = fs::metadata(path) else { return };
    if meta.len() > MAX_FILE_BYTES {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for (idx, line) in text.lines().enumerate() {
        // A single file can hold far more matches than the budget: honor the
        // cap per line, not just between files, so "N results" stays truthful.
        if results.len() >= max_results {
            break;
        }
        if !find_matches(line, query, opts).is_empty() {
            results.push(SearchHit {
                path: path.strip_prefix(root).unwrap_or(path).to_path_buf(),
                line: idx + 1,
                text: line.trim().to_string(),
            });
        }
    }
}

/// Outcome of a workspace-wide replace operation.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReplaceSummary {
    pub files_changed: usize,
    pub replacements: usize,
}

/// Replace every match of `find` with `replace` across the workspace, honoring
/// the same `opts` and `include` glob filter as `project_search` (so what the
/// panel shows is what gets replaced). Returns how many files changed and how
/// many occurrences did.
pub fn project_replace(
    root: &Path,
    find: &str,
    replace: &str,
    opts: SearchOptions,
    include: &str,
) -> ReplaceSummary {
    let mut summary = ReplaceSummary::default();
    if find.is_empty() {
        return summary;
    }
    replace_walk(root, root, find, replace, opts, include, &mut summary);
    summary
}

fn replace_walk(
    root: &Path,
    dir: &Path,
    find: &str,
    replace: &str,
    opts: SearchOptions,
    include: &str,
    summary: &mut ReplaceSummary,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.')
            || name == "target"
            || name == "node_modules"
            || name == "__pycache__"
        {
            continue;
        }
        if path.is_dir() {
            replace_walk(root, &path, find, replace, opts, include, summary);
        } else if path.is_file() {
            replace_file(&path, root, find, replace, opts, include, summary);
        }
    }
}

fn replace_file(
    path: &Path,
    root: &Path,
    find: &str,
    replace: &str,
    opts: SearchOptions,
    include: &str,
    summary: &mut ReplaceSummary,
) {
    const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    if !path_matches_glob(&rel, include) {
        return;
    }
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() > MAX_FILE_BYTES {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let (updated, count) = replace_matches(&text, find, replace, opts);
    if count == 0 {
        return;
    }
    if fs::write(path, updated).is_ok() {
        summary.files_changed += 1;
        summary.replacements += count;
    }
}

/// Replace every match inside a single workspace file (`rel` is the hit path
/// as stored by `project_search`, relative to `root`). Returns the number of
/// replacements made, or 0 if nothing matched or the file could not be
/// rewritten. Backs the per-file "replace in file" action in the search panel.
pub fn replace_in_file(
    root: &Path,
    rel: &Path,
    find: &str,
    replace: &str,
    opts: SearchOptions,
) -> usize {
    let path = root.join(rel);
    let Ok(text) = fs::read_to_string(&path) else {
        return 0;
    };
    let (updated, count) = replace_matches(&text, find, replace, opts);
    if count == 0 {
        return 0;
    }
    if fs::write(&path, updated).is_ok() {
        count
    } else {
        0
    }
}

/// Collect every indexable file in the workspace as a relative path string,
/// for the quick-open switcher. Skips hidden dirs, build output and
/// dependencies, and caps the total so huge trees stay responsive.
pub fn list_workspace_files(root: &Path, max_results: usize) -> Vec<String> {
    let mut results = Vec::new();
    walk_files(root, root, max_results, &mut results);
    results.sort();
    results
}

fn walk_files(root: &Path, dir: &Path, max_results: usize, results: &mut Vec<String>) {
    if results.len() >= max_results {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if results.len() >= max_results {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.')
            || name == "target"
            || name == "node_modules"
            || name == "__pycache__"
        {
            continue;
        }
        if path.is_dir() {
            walk_files(root, &path, max_results, results);
        } else if path.is_file() {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            results.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// A symbol indexed by the site map: a name plus the relative file that
/// defines it (predicate 1 = "file defines symbol").
#[derive(Clone, Debug)]
pub struct SymbolEntry {
    pub name: String,
    pub file: String,
    /// Exact 1-based definition line, when a language server reported it.
    /// `None` for sitemap-backed entries, which fall back to a text scan.
    pub line: Option<usize>,
}

/// Collect every symbol the site map knows about, paired with the relative
/// path of the file that defines it. Symbols whose resolved name looks like a
/// path are skipped.
pub fn collect_workspace_symbols(workspace_root: &Path) -> Vec<SymbolEntry> {
    let Ok(sm) = crate::automation::open_workspace_site_map(workspace_root) else {
        return Vec::new();
    };
    let mut out: Vec<SymbolEntry> = Vec::new();
    // predicate 1 = "file defines/contains symbol".
    for triple in sm.find_live_triples(None, Some(1), None) {
        let file = sm.resolve_string(triple.subject_hash).unwrap_or_default();
        let name = sm.resolve_string(triple.object_hash).unwrap_or_default();
        if file.is_empty() || name.is_empty() {
            continue;
        }
        if !file.contains('/') && !file.contains('\\') {
            continue;
        }
        if name.contains('/') || name.contains('\\') {
            continue;
        }
        out.push(SymbolEntry {
            name,
            file: file.replace('\\', "/"),
            line: None,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.file.cmp(&b.file)));
    out.dedup_by(|a, b| a.name == b.name && a.file == b.file);
    out
}

/// Merge language-server `workspace/symbol` hits into the sitemap-backed
/// list: server results first (they carry exact definition lines), then
/// local entries the server did not already report, keyed on (name, file).
/// Paths under the workspace root are stored relative with forward slashes
/// so the jump resolves through the same `root.join(file)` path as local
/// entries; anything outside keeps its absolute spelling.
pub fn merge_workspace_symbols(
    local: &[SymbolEntry],
    lsp: &[crate::editor::lsp_client::LspWorkspaceSymbol],
    workspace_root: &Path,
) -> Vec<SymbolEntry> {
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut out: Vec<SymbolEntry> = Vec::with_capacity(local.len() + lsp.len());
    for s in lsp {
        let rel = s.path.strip_prefix(workspace_root).unwrap_or(&s.path);
        let file = rel.to_string_lossy().replace('\\', "/");
        if seen.insert((s.name.clone(), file.clone())) {
            out.push(SymbolEntry {
                name: s.name.clone(),
                file,
                // Server lines are 0-based; the jump consumes 1-based.
                line: s.line.map(|l| l + 1),
            });
        }
    }
    for e in local {
        if seen.insert((e.name.clone(), e.file.clone())) {
            out.push(e.clone());
        }
    }
    out
}

/// Next go-to-symbol dispatch delay for a given streak of empty answers:
/// the base debounce, doubling per retry up to a cap. A language server
/// still loading its crate graph answers `workspace/symbol` with `[]`
/// immediately, so an empty answer means "ask again later" — with backoff,
/// not a request storm.
pub fn symbol_query_retry_delay(retries: u32) -> std::time::Duration {
    std::time::Duration::from_millis((180u64 << retries.min(5)).min(3000))
}

/// Best-effort 1-based line number where `name` is defined inside `content`.
/// Prefers a definition keyword (`fn`/`struct`/…) on the line, then falls back
/// to the first line mentioning the name.
pub fn find_definition_line(content: &str, name: &str) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    let keywords = [
        "fn ", "struct ", "enum ", "trait ", "impl ", "type ", "const ", "static ", "mod ",
        "class ", "def ",
    ];
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if keywords.iter().any(|k| trimmed.starts_with(k)) && trimmed.contains(name) {
            return Some(idx + 1);
        }
    }
    content
        .lines()
        .position(|line| line.contains(name))
        .map(|idx| idx + 1)
}

/// A top-level symbol extracted from a source file's text, for the Outline view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSymbol {
    pub name: String,
    /// 1-based line number.
    pub line: usize,
    /// The definition keyword that produced this entry (`"fn"`, `"struct"`, …),
    /// trailing space trimmed. The Outline shows it as a per-row badge so a
    /// function reads differently from a type at a glance.
    pub kind: &'static str,
}

/// Extract top-level (column-0) definitions from source text for the Outline
/// view. Keyword-based and language-light: works for Rust, Python, JS/TS, etc.
/// Entries come back in source order (ascending line), which is what the
/// Outline renders — no separate sort.
pub fn extract_file_symbols(content: &str) -> Vec<FileSymbol> {
    const KEYWORDS: &[&str] = &[
        "fn ",
        "struct ",
        "enum ",
        "trait ",
        "impl ",
        "type ",
        "const ",
        "static ",
        "mod ",
        "class ",
        "def ",
        "interface ",
        "function ",
    ];
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        // Only top-level items: the definition keyword starts at column 0.
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let trimmed = line.trim_start();
        for kw in KEYWORDS {
            if let Some(rest) = trimmed.strip_prefix(kw) {
                if let Some(name) = extract_ident(rest) {
                    out.push(FileSymbol {
                        name,
                        line: idx + 1,
                        kind: kw.trim_end(),
                    });
                }
                break;
            }
        }
    }
    out
}

/// Read the leading identifier (plus generics like `Foo<T>`) from `s`.
fn extract_ident(s: &str) -> Option<String> {
    let s = s.trim_start();
    let mut name = String::new();
    for ch in s.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            name.push(ch);
        } else {
            break;
        }
    }
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub fn icon_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rs",
        Some("toml") => "cf",
        Some("md") => "md",
        Some("json") => "{}",
        Some("py") => "py",
        Some("js" | "ts") => "js",
        Some("html" | "css") => "<>",
        Some("cpp" | "c" | "h") => "c",
        _ => "f",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_query_retry_delay_grows_and_caps() {
        // Fresh query: the plain debounce interval.
        assert_eq!(symbol_query_retry_delay(0).as_millis(), 180);
        // Each empty answer doubles the wait…
        assert_eq!(symbol_query_retry_delay(2).as_millis(), 720);
        // …up to a fixed ceiling, so a loading server is polled at most
        // every few seconds instead of on every frame.
        assert_eq!(symbol_query_retry_delay(5).as_millis(), 3000);
        assert_eq!(symbol_query_retry_delay(99).as_millis(), 3000);
    }

    #[test]
    fn extract_file_symbols_captures_kind_line_in_source_order() {
        let src = "struct Foo {\n    x: u8,\n}\n\nfn bar() {\n    baz();\n}\nimpl Foo {}\n";
        let syms = extract_file_symbols(src);
        // Only column-0 definitions: the indented `x: u8,` and `baz();` are skipped.
        assert_eq!(
            syms,
            vec![
                FileSymbol {
                    name: "Foo".into(),
                    line: 1,
                    kind: "struct"
                },
                FileSymbol {
                    name: "bar".into(),
                    line: 5,
                    kind: "fn"
                },
                FileSymbol {
                    name: "Foo".into(),
                    line: 8,
                    kind: "impl"
                },
            ]
        );
    }

    #[test]
    fn extract_file_symbols_ignores_prefixed_keywords() {
        // `effect` and `transfer` start with a keyword's letters but not the
        // `"fn "`/`"trait"` token-with-space, so nothing is captured.
        let src = "effect run() {}\ntraitish\n";
        assert!(extract_file_symbols(src).is_empty());
    }

    #[test]
    fn project_search_finds_literal_in_own_source_tree() {
        // The crate's own src/ tree is full of "fn " — the synchronous walk
        // must surface hits for it. Guards against regressions in `walk` /
        // `search_file` (e.g. over-eager skipping or unreadable files).
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let hits = project_search(&root, "fn ", 100, SearchOptions::default(), "");
        assert!(
            !hits.is_empty(),
            "project_search found nothing for \"fn \" under {}",
            root.display()
        );
    }

    #[test]
    fn find_matches_honors_case_toggle() {
        let ci = SearchOptions::default();
        let cs = SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        // Case-insensitive finds both spellings; case-sensitive only exact.
        assert_eq!(find_matches("Foo foo FOO", "foo", ci).len(), 3);
        assert_eq!(find_matches("Foo foo FOO", "foo", cs).len(), 1);
    }

    #[test]
    fn find_matches_honors_whole_word() {
        let ww = SearchOptions {
            whole_word: true,
            ..Default::default()
        };
        // "cat" matches only the standalone word, not "category"/"concat".
        let m = find_matches("cat category concat the cat.", "cat", ww);
        assert_eq!(m.len(), 2);
        // Without the flag, substrings count too.
        assert_eq!(
            find_matches(
                "cat category concat the cat.",
                "cat",
                SearchOptions::default()
            )
            .len(),
            4
        );
    }

    #[test]
    fn find_matches_regex_mode() {
        let rx = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        // \\d+ finds the two runs of digits, case-insensitivity is moot here.
        assert_eq!(find_matches("a12 b3 c", r"\d+", rx).len(), 2);
        // A malformed pattern yields no matches rather than panicking.
        assert!(find_matches("abc", r"(", rx).is_empty());
    }

    #[test]
    fn replace_matches_rewrites_all_and_counts() {
        let ci = SearchOptions::default();
        let (out, n) = replace_matches("Foo foo FOO", "foo", "X", ci);
        assert_eq!(n, 3);
        assert_eq!(out, "X X X");
        // Case-sensitive replaces only the exact spelling.
        let (out2, n2) = replace_matches(
            "Foo foo FOO",
            "foo",
            "X",
            SearchOptions {
                case_sensitive: true,
                ..Default::default()
            },
        );
        assert_eq!((out2.as_str(), n2), ("Foo X FOO", 1));
        // No matches leaves the text untouched with a zero count.
        let (out3, n3) = replace_matches("hello", "zzz", "X", ci);
        assert_eq!((out3.as_str(), n3), ("hello", 0));
    }

    #[test]
    fn regex_replace_rewrites_matched_spans() {
        let rx = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let (out, n) = replace_matches("id=12, id=34", r"\d+", "#", rx);
        assert_eq!((out.as_str(), n), ("id=#, id=#", 2));
    }

    #[test]
    fn regex_replace_expands_capture_groups() {
        let rx = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        // Group swap between two spellings of the same pattern.
        let (out, n) = replace_matches("hi there bob joe", r"(\w+) (\w+)", "$2 $1", rx);
        assert_eq!((out.as_str(), n), ("there hi joe bob", 2));
        // Braced form, $0 whole match, and undefined groups → empty.
        let (out2, _) = replace_matches("ab", r"(a)(b)?", "[$0]{${1}}{${9}}", rx);
        assert_eq!(out2, "[ab]{a}{}");
        // A lone `$` (not followed by a digit or `{`) stays literal.
        let (out3, _) = replace_matches("cost 12", r"\d+", "$ cost", rx);
        assert_eq!(out3, "cost $ cost");
    }

    #[test]
    fn expand_replacement_semantics() {
        let g: Vec<Option<&str>> = vec![Some("one"), None];
        assert_eq!(expand_replacement("$1/$2/$0", "whole", &g), "one//whole");
        assert_eq!(expand_replacement("${1}x", "w", &g), "onex");
        // Unclosed brace and non-digit `$` stay literal.
        assert_eq!(expand_replacement("${1", "w", &g), "${1");
        assert_eq!(expand_replacement("a$b", "w", &g), "a$b");
        // Group numbers beyond what the pattern defines expand to nothing.
        assert_eq!(expand_replacement("$7!", "w", &g), "!");
    }

    #[test]
    fn literal_replace_does_not_expand_dollars() {
        // Only regex mode treats `$n` as a group reference.
        let (out, n) = replace_matches("foo foo", "foo", "$1", SearchOptions::default());
        assert_eq!((out.as_str(), n), ("$1 $1", 2));
    }

    #[test]
    fn path_matches_glob_matrix() {
        // Empty filter matches everything.
        assert!(path_matches_glob("src/editor/main.rs", ""));
        // Bare extension pattern matches the file name at any depth.
        assert!(path_matches_glob("src/editor/main.rs", "*.rs"));
        assert!(path_matches_glob("main.rs", "*.rs"));
        assert!(!path_matches_glob("src/main.py", "*.rs"));
        // ** spans segments; a directory prefix scopes the search.
        assert!(path_matches_glob("src/editor/app/x.rs", "src/**/*.rs"));
        assert!(path_matches_glob("src/x.rs", "src/**/*.rs"));
        assert!(!path_matches_glob("tests/x.rs", "src/**/*.rs"));
        // Single-segment * does not cross '/'; ? matches one char.
        assert!(path_matches_glob("src/a.rs", "src/?.rs"));
        assert!(!path_matches_glob("src/sub/a.rs", "src/a.rs"));
        assert!(path_matches_glob("src/sub/a.rs", "src/*/a.rs"));
        // Comma-separated list: any alternative matches; case-insensitive.
        assert!(path_matches_glob("README.md", "*.rs, *.md"));
        assert!(path_matches_glob("SRC/Main.RS", "*.rs"));
        assert!(!path_matches_glob("src/main.rs", "docs/**, *.toml"));
    }

    #[test]
    fn project_search_honors_include_filter() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // Restricting to Rust files still finds "fn " and every hit is a .rs.
        let rs = project_search(&root, "fn ", 50, SearchOptions::default(), "*.rs");
        assert!(!rs.is_empty());
        assert!(rs
            .iter()
            .all(|h| h.path.extension().is_some_and(|e| e == "rs")));
        // A filter matching nothing yields no hits even though the term exists.
        let none = project_search(&root, "fn ", 50, SearchOptions::default(), "*.zzz");
        assert!(none.is_empty());
    }

    #[test]
    fn replace_in_file_rewrites_only_that_file() {
        // Two temp files both containing the needle; only the targeted one
        // should change, and the reported count must match the occurrences.
        let dir =
            std::env::temp_dir().join(format!("velocity_replace_in_file_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("a.txt"), "foo bar foo").unwrap();
        fs::write(dir.join("b.txt"), "foo").unwrap();
        let n = replace_in_file(
            &dir,
            Path::new("a.txt"),
            "foo",
            "X",
            SearchOptions::default(),
        );
        assert_eq!(n, 2);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "X bar X");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).unwrap(), "foo");
        // A no-match replace leaves the file untouched and reports 0.
        let n0 = replace_in_file(
            &dir,
            Path::new("a.txt"),
            "zzz",
            "X",
            SearchOptions::default(),
        );
        assert_eq!(n0, 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
