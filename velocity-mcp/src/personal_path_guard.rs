//! Binds every account-naming path in the shipped files to a placeholder.
//!
//! A path that spells out somebody's profile directory claims that the reader's
//! machine is the author's machine, and it is wrong in three ways at once. The
//! test that reads it runs for one person and is a dead `expect` for everyone
//! else - the two ignored crucible probes did exactly that, which is how they
//! could stay broken without anybody noticing. The script that cds into it works
//! on one checkout. And the account name, plus the personal sync tree above it,
//! is a piece of the author's filesystem that ships to every reader: a captured
//! traceback pasted into a fixture, a screenshot path, a directory nobody else
//! has.
//!
//! [`crate::doc_links_guard`] refuses this shape in the target of a markdown
//! link, and blanks code on its way there, so a tutorial is free to show a
//! command line it does not mean anybody to run. This guard reads the other
//! corpus - every tracked source, script, config and prose file - and blanks
//! nothing, because here the name itself is the offence wherever it sits.
//!
//! The shapes read, each named in the failure so a reader can see which form was
//! matched:
//!
//! - a Windows profile path: the marker `Users` between separators whose *own*
//!   preceding token is a drive letter (`C:`), so that a `/users/<name>` route in
//!   a URL and a `https:` colon do not turn prose into a filesystem;
//! - `Users` or `home` in the spelling a home directory uses, with the leading
//!   slash part of the marker, so that a lowercase route is not mistaken for one;
//! - a personal sync tree: the segment `OneDrive` wherever it sits behind a
//!   separator, account named or not, because a sync root is one machine's
//!   directory and this repository writes `/home/velocity` or a placeholder when
//!   it genuinely needs a home.
//!
//! A segment that interpolates the account (`{}`, `<user>`, `$USER`) or one of
//! the placeholder names in [`PLACEHOLDERS`] is the correct way to write all
//! three, so it is graded as such. What is *not* read: a path built at runtime
//! from `std::env::var` - that is the fix, not the bug; a path whose segments are
//! split across a concatenation, which no scan of text can see; and anything the
//! repository does not track, because a working tree carries the state this
//! machine left in it - site maps, browser profiles, captured reports - all of
//! it naming whoever owns the disk and none of it reaching a reader. The corpus
//! is therefore what `git ls-files` lists, and a checkout with no git to ask is
//! reported as a failure rather than as a sweep over nothing.

use std::path::{Path, PathBuf};

/// Extensions whose contents are read by someone other than whoever wrote them.
const GRADED_EXTENSIONS: [&str; 16] = [
    "rs", "py", "bat", "cmd", "ps1", "sh", "toml", "json", "yaml", "yml", "js", "ts", "tsx",
    "proto", "md", "txt",
];

/// Extension-less files that are shipped all the same.
const GRADED_NAMES: [&str; 3] = ["Dockerfile", "Justfile", "Makefile"];

/// Home-directory segments that mean *whoever is reading this*. Each is a
/// placeholder somebody chose on purpose, so the sweep reads it as the correct
/// form of the very path it refuses.
const PLACEHOLDERS: [&str; 19] = [
    "admin", "anyone", "anybody", "bar", "baz", "ci", "deploy", "dev", "example", "foo", "root",
    "sample", "someone", "test", "user", "velocity", "x", "y", "z",
];

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

/// Chars that cannot sit inside a path segment: the separators, and the quoting
/// or bracketing a path sits inside.
const SEGMENT_ENDS: [char; 10] = ['"', '\'', '`', ')', ']', '}', '>', ';', ',', ':'];

/// What ends a path segment inside a line of source.
fn ends_segment(c: char) -> bool {
    is_sep(c) || c.is_whitespace() || SEGMENT_ENDS.contains(&c)
}

/// A segment that interpolates the account instead of naming one.
fn is_interpolation(segment: &str) -> bool {
    segment
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '{' | '<' | '$' | '%' | '@'))
}

/// `hay` carries `needle` starting at index `at`.
fn starts_with_at(hay: &[char], at: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(k, c)| hay.get(at + k) == Some(&c))
}

/// Chars that end the token before a marker.
///
/// A drive letter reaches a `Users` directory as `C:\Users\`, and the only thing
/// on the other side of that `C:` is whatever quotes, assigns or spaces the path -
/// `r"C:\Users\…`, `"C:\\Users\\…`, `--dir=C:\Users\…`. Splitting on those is what
/// makes the preceding token a drive rather than the whole line up to it.
const TOKEN_ENDS: [char; 15] = [
    '"', '\'', '`', '(', ')', '[', ']', '{', '}', '<', '>', ';', ',', '=', '#',
];

fn delimits_token(c: char) -> bool {
    is_sep(c) || c.is_whitespace() || TOKEN_ENDS.contains(&c)
}

/// Is the token immediately before the marker at `at` a lone drive letter?
///
/// Requiring it to be the *preceding* token - not merely present somewhere on
/// the line - is what keeps `https://host/users/mine` out of the sweep while
/// `\\?\C:\Users\dev\x` stays in it. A `Users` directory under some other folder
/// (`D:\scratch\Users\bob`) or under a UNC root (`\\server\share\Users\bob`) is
/// read as a directory of that name, which is the shape this rule misses.
fn drive_rooted_before(lower: &[char], at: usize) -> bool {
    if at == 0 || !is_sep(lower[at - 1]) {
        return false;
    }
    let prefix: String = lower[..at].iter().collect();
    prefix
        .split(delimits_token)
        .rev()
        .find(|token| !token.is_empty())
        .is_some_and(|token| {
            token.len() == 2
                && token.as_bytes()[0].is_ascii_alphabetic()
                && token.as_bytes()[1] == b':'
        })
}

/// The segment starting just past a marker, and where to resume scanning.
fn segment_after(chars: &[char], at: usize) -> (String, usize) {
    let mut j = at;
    while j < chars.len() && is_sep(chars[j]) {
        j += 1;
    }
    let start = j;
    while j < chars.len() && !ends_segment(chars[j]) {
        j += 1;
    }
    (chars[start..j].iter().collect(), (at + 1).max(j))
}

/// One home-path mention in a line: where it sits, which shape was read, and the
/// segment that follows it.
#[derive(Debug)]
struct Mention {
    at: usize,
    shape: &'static str,
    segment: String,
}

/// Every home-path mention on `line`, in the order they appear on it.
fn mentions_in_line(line: &str) -> Vec<Mention> {
    let chars: Vec<char> = line.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_ascii_lowercase()).collect();
    let mut found = Vec::new();

    // Windows profile: the marker sits between separators behind a drive letter,
    // so both `\\?\C:\Users\dev\x` and `C:/Users/test/x` read.
    let mut pos = 0;
    while pos + 5 <= lower.len() {
        if starts_with_at(&lower, pos, "users")
            && drive_rooted_before(&lower, pos)
            && lower.get(pos + 5).is_some_and(|c| is_sep(*c))
        {
            let (segment, next) = segment_after(&chars, pos + 5);
            if !segment.is_empty() {
                found.push(Mention {
                    at: pos,
                    shape: "Windows profile path",
                    segment,
                });
            }
            pos = next;
            continue;
        }
        pos += 1;
    }

    // A home directory, leading slash included: the slash being part of the
    // marker is what keeps a lowercase route in a URL out of the sweep.
    for (marker, shape) in [
        ("/Users/", "macOS profile path"),
        ("/home/", "Unix home path"),
    ] {
        for (start, _) in line.match_indices(marker) {
            let rest = &line[start + marker.len()..];
            let segment = match rest.find(ends_segment) {
                Some(end) => &rest[..end],
                None => rest,
            };
            if !segment.is_empty() {
                found.push(Mention {
                    at: start,
                    shape,
                    segment: segment.to_string(),
                });
            }
        }
    }

    // A personal sync tree, whether or not an account is named under it: the
    // tree is the offence, so the tree's own name is what is reported.
    let mut pos = 0;
    while pos + 8 <= lower.len() {
        if starts_with_at(&lower, pos, "onedrive") && pos > 0 && is_sep(lower[pos - 1]) {
            found.push(Mention {
                at: pos,
                shape: "personal sync root",
                segment: "OneDrive".to_string(),
            });
            pos += 8;
            continue;
        }
        pos += 1;
    }

    found.sort_by_key(|m| m.at);
    found
}

/// A mention that names a person rather than a placeholder.
#[derive(Debug)]
pub struct NamedPath {
    /// Repository-relative path of the file that carries it.
    pub file: String,
    /// 1-based line number, so the reader of the failure finds the line.
    pub line: usize,
    /// Which of the read shapes matched.
    pub shape: &'static str,
    /// The segment as written.
    pub segment: String,
}

/// How many home-path mentions `file` carries - the evidence that the sweep read
/// it - and which of those mentions name somebody.
fn grade(file: &str, text: &str) -> (usize, Vec<NamedPath>) {
    let mut mentions = 0;
    let mut named = Vec::new();
    for (number, line) in text.lines().enumerate() {
        for m in mentions_in_line(line) {
            mentions += 1;
            let written_as_placeholder = is_interpolation(&m.segment)
                || PLACEHOLDERS.contains(&m.segment.to_lowercase().as_str());
            if !written_as_placeholder {
                named.push(NamedPath {
                    file: file.to_string(),
                    line: number + 1,
                    shape: m.shape,
                    segment: m.segment,
                });
            }
        }
    }
    (mentions, named)
}

/// The repository the sweep reads, located from the crate rather than from
/// anywhere a caller has to be told about.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("velocity-mcp sits at the repo root")
        .to_path_buf()
}

/// The files the repository ships, as repository-relative `/`-form paths, or the
/// reason the sweep cannot say.
fn tracked_files(root: &Path) -> Result<Vec<String>, String> {
    let run = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map_err(|why| format!("`git ls-files` could not run: {why}"))?;
    if !run.status.success() {
        return Err(format!(
            "`git ls-files` exited with {}: {}",
            run.status,
            String::from_utf8_lossy(&run.stderr).trim()
        ));
    }
    let mut names = Vec::new();
    for name in String::from_utf8_lossy(&run.stdout).split('\0') {
        if name.is_empty() {
            continue;
        }
        let file = name.rsplit('/').next().unwrap_or(name);
        let graded = match file.rsplit_once('.') {
            Some((_, ext)) => GRADED_EXTENSIONS.contains(&ext.to_lowercase().as_str()),
            None => GRADED_NAMES.contains(&file),
        };
        if graded {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

/// The files this project ships, and whatever stood in the way of reading them.
struct Corpus {
    /// `(repository-relative path, text)`, sorted by path.
    files: Vec<(String, String)>,
    /// Why the corpus could not be named at all, and every file that was in the
    /// way of reading it: one that would not open, or one that is not valid
    /// UTF-8.
    stuck: Vec<String>,
}

/// Read the corpus.
///
/// This repository lives under a sync root, where a placeholder whose contents
/// have not been fetched and a file saved in somebody's legacy encoding both
/// defeat a read. A sweep that dropped those would report the same green as a
/// corpus with nothing in it, so they are carried out separately and a test
/// refuses them.
fn corpus() -> Corpus {
    let root = repo_root();
    let (paths, mut stuck) = match tracked_files(&root) {
        Ok(paths) => (paths, Vec::new()),
        Err(why) => {
            return Corpus {
                files: Vec::new(),
                stuck: vec![why],
            }
        }
    };
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let mut absolute = root.to_path_buf();
        for segment in path.split('/').filter(|s| !s.is_empty()) {
            absolute.push(segment);
        }
        match std::fs::read_to_string(&absolute) {
            Ok(text) => files.push((path, text)),
            Err(why) => stuck.push(format!("{path}: {why}")),
        }
    }
    Corpus { files, stuck }
}

/// Every named-account path in the corpus, with the sweep's own tally of the
/// mentions it read on the way and the files it opened.
fn sweep() -> (Vec<NamedPath>, usize, usize) {
    let corpus = corpus();
    let mut named = Vec::new();
    let mut mentions = 0;
    for (file, text) in &corpus.files {
        let (count, offenders) = grade(file, text);
        mentions += count;
        named.extend(offenders);
    }
    (named, mentions, corpus.files.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two tokens this guard exists to refuse, assembled rather than spelled
    /// out: this file is itself in the corpus the sweep grades, and a literal
    /// example of the shape it forbids would make the suite red on the guard that
    /// enforces it.
    fn invented_account() -> String {
        concat!("mi", "ra", "t").to_string()
    }

    fn invented_sync_root() -> String {
        concat!("One", "Drive").to_string()
    }

    #[test]
    fn no_shipped_file_names_an_account_or_a_sync_root() {
        let (named, _mentions, _files) = sweep();
        assert!(
            named.is_empty(),
            "{} path(s) in the shipped files name one person's filesystem: {} - a test that \
             reads a profile directory runs for its author alone, and an account name or a \
             personal sync root is that author's own tree published to every reader. Write a \
             placeholder (`dev`, `test`), an interpolation (`{{dir}}`, `<user>`) or a path \
             resolved from the crate instead",
            named.len(),
            named
                .iter()
                .map(|n| format!("{}:{}: {} naming {}", n.file, n.line, n.shape, n.segment))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    /// A clean sweep is only evidence if there was a corpus to sweep and the
    /// scanner matched shapes inside it.
    #[test]
    fn the_sweep_reads_the_files_and_the_shapes_it_claims_to() {
        let read = corpus();
        assert!(
            read.stuck.is_empty(),
            "the sweep could not read its way through the corpus: {} - an unfetched placeholder \
             under a sync root and a file in somebody's legacy encoding both defeat a read, and \
             a green that means did not look is not a green",
            read.stuck.join("; ")
        );
        let (named, mentions, files) = sweep();
        assert!(
            files >= 600,
            "only {files} files were graded: the walk has stopped reaching the sources it \
             exists to read, and the repository tracks hundreds of Rust files alone"
        );
        assert!(
            mentions >= 12,
            "the corpus carries {mentions} home-path mention(s) across {files} files; a scanner \
             that matches nothing reports the same green as a corpus with nothing wrong in it"
        );
        assert!(
            named.is_empty(),
            "the floors passed but the sweep is red: {named:?}"
        );
    }

    #[test]
    fn a_windows_profile_that_names_an_account_is_the_offence() {
        let account = invented_account();
        let text = format!(r#"let dir = r"C:\Users\{account}\Documents\scratch";"#);
        let (mentions, named) = grade("probe.rs", &text);
        assert_eq!(mentions, 1, "one profile path in: {text}");
        assert_eq!(named.len(), 1, "{named:?}");
        assert_eq!(named[0].shape, "Windows profile path");
        assert_eq!(named[0].segment, account);
        assert_eq!(named[0].line, 1);
    }

    #[test]
    fn an_escaped_account_path_in_a_string_literal_reads_like_the_raw_form() {
        let account = invented_account();
        let text = format!(r#"    model_dir: "C:\\Users\\{account}\\models".to_string(),"#);
        let (mentions, named) = grade("bridge.rs", &text);
        assert_eq!(mentions, 1, "the escape does not hide the path: {text}");
        assert_eq!(named.len(), 1, "{named:?}");
        assert_eq!(named[0].segment, account);
    }

    #[test]
    fn the_posix_shapes_are_read_in_line_order_and_a_route_in_a_url_is_not() {
        let account = invented_account();
        let text = format!(
            "the volume mounts /home/{account}/workspace, and /Users/{account}/proj on a mac\n\
             the route stays https://host/users/{account}/list\n"
        );
        let (mentions, named) = grade("ops.md", &text);
        assert_eq!(
            mentions, 2,
            "two home shapes, the route not among them: {named:?}"
        );
        assert_eq!(named.len(), 2, "{named:?}");
        assert_eq!(
            named.iter().map(|n| n.shape).collect::<Vec<_>>(),
            vec!["Unix home path", "macOS profile path"],
            "one line is reported in the order its paths appear on it"
        );
    }

    #[test]
    fn a_placeholder_or_an_interpolation_is_the_correct_form_of_the_same_path() {
        let (mentions, named) = grade(
            "guide.md",
            "run it from C:\\Users\\dev\\proj or C:/Users/<user>/AppData/Roaming/Velocity, \
             bind the volume at /home/velocity/workspace, and hand `format!(\"C:\\\\Users\\\\{}\\\\x\", who)` \
             to the repair turn - while a sentence about the users of the system and a \
             https://host/users/mine route are no path at all",
        );
        assert!(named.is_empty(), "nothing here names a person: {named:?}");
        assert!(
            mentions >= 4,
            "the placeholders were not counted, so the sweep matched nothing: {mentions}"
        );
    }

    #[test]
    fn a_sync_root_is_refused_even_when_no_account_is_named_under_it() {
        let tree = invented_sync_root();
        let (mentions, named) = grade(
            "probe.rs",
            &format!(r#"let out = r"D:\{tree}\scratch.txt";"#),
        );
        assert_eq!(
            mentions, 1,
            "the bare sync root is still one machine's tree"
        );
        assert_eq!(named.len(), 1, "{named:?}");
        assert_eq!(named[0].shape, "personal sync root");
    }

    #[test]
    fn a_sync_root_under_a_placeholder_account_is_still_named() {
        let tree = invented_sync_root();
        let (mentions, named) = grade(
            "app.rs",
            &format!(r#"let base = r"C:\Users\test\{tree}\repo";"#),
        );
        assert_eq!(mentions, 2, "the account and the tree: {named:?}");
        assert_eq!(
            named.len(),
            1,
            "the placeholder account is fine, the tree is not: {named:?}"
        );
        assert_eq!(named[0].shape, "personal sync root");
    }
}
