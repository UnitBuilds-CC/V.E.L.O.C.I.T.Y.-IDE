//! Binds `USER_GUIDE.md`'s environment-variable reference to the code.
//!
//! A table of environment variables is a claim about what the program looks up,
//! what each value is accepted as, and what happens when it is unset. Nothing in
//! a document can be trusted to stay in step with source, so the section is
//! compared against it: the shipped crates are scanned for every name the
//! environment is actually read through, and the guide must name exactly those.
//!
//! The scan is the interesting part, because a name reaches `std::env::var` in
//! four shapes and a grep for `env::var("NAME")` sees only one of them:
//!
//! 1. a literal at the call site - `env::var("OLLAMA_HOSTS")`;
//! 2. a `const`/`static &str` declared elsewhere in the file and passed in -
//!    `env::var(EVENT_LOG_MAX_BYTES_ENV)`;
//! 3. a list or a closure between the literal and the call - `["A", "B"]` handed
//!    to `find_map`, or `let get = |k| env::var(k); get("VELOCITY_JEV_MODEL")`;
//! 4. a `format!` template, which is not a literal at all - the indexed account
//!    families `CF_ACCOUNT_{i}_ID`, `OPENROUTER_ACCOUNT_{i}_KEY`,
//!    `AZURE_OPENAI_ACCOUNT_{i}_ENDPOINT`.
//!
//! Shapes 1, 2 and 4 are read structurally. Shape 3 needs a window around each
//! call, which is also what drags in names the process only *writes* for a child
//! (`VELOCITY_HOOK_EVENT`, `WUIAS_SESSION_ID`, the `VELOCITY_SANDBOX_*` hints).
//! Those are listed once, each with the reason it is not a knob, and the test
//! checks that each is still in the source: an exclusion is allowed to go stale
//! only because the code moved, never because a name disappeared unnoticed.
//!
//! `#[cfg(test)]` items are skipped by brace matching rather than by cutting at
//! the first occurrence, because several files put their tests in the middle and
//! everything after them would otherwise be read as production code. The same
//! reason excludes this file from its own walk: it quotes every excluded name and
//! every bucket prefix, so a scan that read it would let the guard certify its
//! own list from inside itself, and an exclusion that had outlived its code would
//! still find its new name a few lines above the assertion meant to notice.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Crates whose `src/` is scanned. A name read outside these is out of scope for
/// this section, so a new crate has to say so here deliberately.
const CRATES: [&str; 6] = [
    "velocity-mcp",
    "velocity-ide",
    "velocity-ide-gui",
    "velocity-browser",
    "drone",
    "velocity-serve",
];

/// Prefixes this project owns. Anything wearing one is a knob the guide names.
const OWNED_PREFIXES: &[&str] = &[
    "VELOCITY_",
    "CF_",
    "AZURE_OPENAI_",
    "OPENROUTER_",
    "OLLAMA_",
    "LLAMA_CPP_",
    "LLM_PROVIDER",
    "BEDROCK_",
    "DRONE_",
    "WUIAS_",
    "NDA_",
    "JEV_",
    "SELECT_MODEL_",
];

/// Names that are not this project's but are read with effect: the OS, the
/// platform conventions, and the provider keys other tools already set. The
/// `*_API_KEY` suffix is a rule rather than a list because the failover ladder
/// in `agent::executor::loop_runner` asks for one key per provider.
const INHERITED: &[&str] = &[
    "RUST_LOG",
    "XDG_CONFIG_HOME",
    "APPDATA",
    "HOME",
    "USERPROFILE",
    "USERNAME",
    "USER",
    "LOGNAME",
    "HOSTNAME",
    "COMPUTERNAME",
    "SHELL",
    "PYTHONIOENCODING",
];

/// Names the window rule picks up that the process only writes, scrubs, or
/// splits out of a longer literal. Each is asserted to still appear in
/// production source, so this list cannot quietly outlive the code it excuses.
/// An entry ending in `_` is a prefix, not a whole name.
const NOT_A_KNOB: &[(&str, &str)] = &[
    (
        "VELOCITY_HOOK_EVENT",
        "written into a hook child's environment (registry/hooks.rs), never read",
    ),
    (
        "VELOCITY_SANDBOX_NO_NETWORK",
        "a hint handed down to a sandboxed child (security/sandbox.rs), not read here",
    ),
    (
        "VELOCITY_SANDBOX_MEM_LIMIT",
        "a hint handed down to a sandboxed child (security/sandbox.rs), not read here",
    ),
    (
        "VELOCITY_CSHARP_ENGINE",
        "removed from a sandboxed child's environment (security/sandbox.rs), not read here",
    ),
    (
        "VELOCITY_CT_",
        "the prefix custom-tool parameters are exported under (registry/custom_tools.rs); written, never read",
    ),
    (
        "WUIAS_SESSION_ID",
        "set around CreateProcessW for the sandbox DLL and removed again (registry/system_tools.rs)",
    ),
    (
        "WUIAS_REDIRECT_DIR",
        "set around CreateProcessW for the sandbox DLL and removed again (registry/system_tools.rs)",
    ),
    (
        "GOOGLE_APPLICATION_CREDENTIALS",
        "in credential_guard's scrub list: probed for existence and removed, never used to configure anything",
    ),
];

#[derive(Default)]
struct Reads {
    /// Plain names the environment is read through.
    knobs: BTreeSet<String>,
    /// `format!` families, normalised so the index placeholder reads `{n}`.
    families: BTreeSet<String>,
    /// Names read that belong to no bucket - a scan that has outgrown its rules.
    unclassified: BTreeSet<String>,
    /// Every upper-case literal seen in production source, used to keep
    /// `NOT_A_KNOB` honest.
    seen_in_source: BTreeSet<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("velocity-mcp sits at the repo root")
        .to_path_buf()
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
}

/// A comment is prose *about* a variable, not a read of one. Without this the
/// scan would find the names this file's own doc comment quotes.
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// `NAME` when the literal could be an environment variable: upper case, digits
/// and underscores, at least four characters.
fn looks_like_env_name(name: &str) -> bool {
    name.len() >= 4
        && name.starts_with(|c: char| c.is_ascii_uppercase())
        && name.chars().all(is_name_char)
}

/// Drop the contents of string literals and of line comments so that brace
/// counting cannot be fooled by `"{"` or by a `// }`.
fn code_shape(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
                out.push_str("\"\"");
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '/' if chars.peek() == Some(&'/') => break,
            other => out.push(other),
        }
    }
    out
}

/// Whether this line's code shape finishes an item at a `;` instead of opening
/// a brace body: `mod helpers;`, `const N: u32 = 3;`.
fn ends_at_semicolon(shape: &str) -> bool {
    !shape.contains('{') && shape.trim_end().ends_with(';')
}

/// The file's production lines joined with newlines, `#[cfg(test)]` items
/// removed by brace matching.
fn production_body(text: &str) -> Vec<String> {
    let mut keep = Vec::new();
    let mut skipping = false;
    let mut opened = false;
    let mut depth = 0i32;
    for line in text.lines() {
        let shape = code_shape(line);
        let delta = shape.matches('{').count() as i32 - shape.matches('}').count() as i32;
        if skipping {
            if opened {
                depth += delta;
                if depth <= 0 {
                    skipping = false;
                    opened = false;
                }
            } else if shape.contains('{') {
                opened = true;
                depth += delta;
                if depth <= 0 {
                    skipping = false;
                    opened = false;
                }
            } else if ends_at_semicolon(&shape) {
                // The attribute went on an item with no body, and this line is
                // that item. Skipping past it would drop every production line
                // after a `#[cfg(test)] mod helpers;` up to the next brace.
                skipping = false;
            }
            continue;
        }
        if line.trim_start().starts_with("#[cfg(test)]") {
            skipping = !ends_at_semicolon(&shape);
            continue;
        }
        keep.push(line.to_string());
    }
    keep
}

/// The contents of every `"..."` on the line.
fn quoted(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for c in line.chars() {
        if in_string {
            if escaped {
                current.push(c);
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                out.push(std::mem::take(&mut current));
                in_string = false;
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_string = true;
        }
    }
    out
}

/// `CF_ACCOUNT_{i}_ID` and `CF_ACCOUNT_{n}_ID` are one family, so the index
/// placeholder is normalised before either side is compared. A template whose
/// fragments are not variable-name-shaped - a URL, a sentence - is rejected.
fn normalize_template(raw: &str) -> Option<String> {
    let mut out = String::new();
    let mut bare = String::new();
    let mut rest = raw;
    let mut placeholders = 0usize;
    while let Some(start) = rest.find('{') {
        let chunk = &rest[..start];
        if !chunk.is_empty() && !chunk.chars().all(is_name_char) {
            return None;
        }
        let end = start + rest[start..].find('}')?;
        if end == start {
            return None;
        }
        out.push_str(chunk);
        bare.push_str(chunk);
        out.push_str("{n}");
        placeholders += 1;
        rest = &rest[end + 1..];
    }
    if placeholders == 0 {
        return None;
    }
    if !rest.is_empty() && !rest.chars().all(is_name_char) {
        return None;
    }
    out.push_str(rest);
    bare.push_str(rest);
    if !looks_like_env_name(&bare) {
        return None;
    }
    Some(out)
}

fn is_excluded(name: &str) -> bool {
    NOT_A_KNOB.iter().any(|(skip, _)| {
        if skip.ends_with('_') {
            name.starts_with(skip)
        } else {
            name == *skip
        }
    })
}

fn classify(name: &str) -> bool {
    OWNED_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
        || name.ends_with("_API_KEY")
        || INHERITED.contains(&name)
}

fn record(name: String, reads: &mut Reads) {
    if is_excluded(&name) {
        return;
    }
    if classify(&name) {
        reads.knobs.insert(name);
    } else {
        reads.unclassified.insert(name);
    }
}

/// Bare all-upper-case identifiers on the line: the ones that might name a const
/// holding the variable's name.
fn uppercase_idents(line: &str) -> Vec<String> {
    line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|token| {
            token.len() >= 3
                && token.starts_with(|c: char| c.is_ascii_uppercase())
                && token
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        })
        .map(|token| token.to_string())
        .collect()
}

/// `NAME: &str = "VALUE"` from a const or static declaration, whatever
/// visibility the owner put in front of it. `pub const` is how a knob shared
/// across modules is usually declared, so ignoring it would hide the name.
fn const_decl_value_part(line: &str) -> Option<&str> {
    if is_comment(line) {
        return None;
    }
    let mut rest = line.trim_start();
    if let Some(after_pub) = rest.strip_prefix("pub") {
        rest = after_pub
            .strip_prefix("(crate)")
            .or_else(|| after_pub.strip_prefix("(super)"))
            .unwrap_or(after_pub);
        rest = rest.trim_start();
    }
    rest.strip_prefix("const ")
        .or_else(|| rest.strip_prefix("static "))
}

fn scan_file(path: &Path, reads: &mut Reads) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let lines = production_body(&text);
    if lines.is_empty() {
        return;
    }
    for line in &lines {
        if is_comment(line) {
            continue;
        }
        for name in quoted(line) {
            if looks_like_env_name(&name) {
                reads.seen_in_source.insert(name);
            }
        }
    }
    let body = lines.join("\n");

    // Shape 2: `const NAME: &str = "VALUE"` in this file, passed to env::var.
    let mut consts: Vec<(String, String)> = Vec::new();
    for line in &lines {
        let Some(after) = const_decl_value_part(line) else {
            continue;
        };
        if !after.contains("&str") || !after.contains('=') {
            continue;
        }
        let ident = after
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or_default()
            .to_string();
        for value in quoted(after) {
            if looks_like_env_name(&value) {
                consts.push((ident.clone(), value));
            }
        }
    }

    // Shape 4 through a binding: `let key = format!("FAMILY_{i}_ID")` and later
    // `env::var(&key)` on a different line.
    for line in &lines {
        if is_comment(line) || !line.contains("let ") || !line.contains("= format!(") {
            continue;
        }
        let Some(assign) = line.find("= format!") else {
            continue;
        };
        let ident = line[..assign]
            .trim_end()
            .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or_default()
            .to_string();
        if ident.is_empty() {
            continue;
        }
        if !body.contains(&format!("env::var(&{ident})"))
            && !body.contains(&format!("env::var({ident})"))
        {
            continue;
        }
        for template in quoted(line) {
            if let Some(family) = normalize_template(&template) {
                if classify(&family) || classify(&family.replace("{n}", "")) {
                    reads.families.insert(family);
                }
            }
        }
    }

    // Shapes 1, 2 and 3, from the call site outward.
    for position in 0..lines.len() {
        let line = &lines[position];
        if is_comment(line) || !line.contains("env::var") {
            continue;
        }
        for name in quoted(line) {
            if let Some(family) = normalize_template(&name) {
                if classify(&family) || classify(&family.replace("{n}", "")) {
                    reads.families.insert(family);
                }
                continue;
            }
            if looks_like_env_name(&name) {
                record(name, reads);
            }
        }
        for ident in uppercase_idents(line) {
            if let Some((_, value)) = consts.iter().find(|(key, _)| *key == ident) {
                record(value.clone(), reads);
            }
        }
        let from = position.saturating_sub(45);
        let to = (position + 46).min(lines.len());
        for window_line in &lines[from..to] {
            if is_comment(window_line) {
                continue;
            }
            for name in quoted(window_line) {
                if looks_like_env_name(&name) && classify(&name) {
                    record(name, reads);
                }
            }
        }
    }
}

/// Whether this is the file the scan is running out of.
///
/// The guard's own text quotes every `NOT_A_KNOB` name and every bucket prefix,
/// so letting it into the corpus lets the file certify its own exclusions from
/// inside itself: renaming an exclusion that had outlived its code still found
/// the new name quoted a few lines above the assertion that was supposed to
/// notice. A test-only module cannot be evidence about production source.
fn is_guard_source(file_name: &str) -> bool {
    file!().rsplit(['/', '\\']).next().unwrap_or_default() == file_name
}

fn walk(dir: &Path, reads: &mut Reads) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name != "target" {
                walk(&path, reads);
            }
        } else if name.ends_with(".rs") && !is_guard_source(&name) {
            scan_file(&path, reads);
        }
    }
}

fn scan_all_reads() -> Reads {
    let root = repo_root();
    let mut reads = Reads::default();
    let mut scanned = 0usize;
    for crate_name in CRATES {
        let src = root.join(crate_name).join("src");
        if src.is_dir() {
            scanned += 1;
            walk(&src, &mut reads);
        }
    }
    assert_eq!(
        scanned,
        CRATES.len(),
        "only {scanned} of the {} crates this guard expects to scan are on disk, so the set \
         it derived is smaller than the set it claims to cover",
        CRATES.len()
    );
    reads
}

/// The lines of `USER_GUIDE.md`'s `## Environment Variables` section.
fn guide_section() -> Vec<String> {
    let guide = std::fs::read_to_string(repo_root().join("USER_GUIDE.md"))
        .expect("USER_GUIDE.md is part of the repository");
    let mut section = Vec::new();
    let mut inside = false;
    for line in guide.lines() {
        let heading = line.trim_start();
        if !inside && heading.starts_with("## ") && heading.contains("Environment Variables") {
            inside = true;
            continue;
        }
        if inside && heading.starts_with("## ") {
            break;
        }
        if inside {
            section.push(line.to_string());
        }
    }
    assert!(
        inside,
        "USER_GUIDE.md has no `## Environment Variables` section for the scan to be compared with"
    );
    section
}

/// Every back-ticked variable name the section uses, families normalised.
fn documented_names() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in guide_section() {
        let mut rest = line.as_str();
        while let Some(start) = rest.find('`') {
            let after = &rest[start + 1..];
            let Some(end) = after.find('`') else {
                break;
            };
            let token = after[..end].trim();
            if let Some(family) = normalize_template(token) {
                out.insert(family);
            } else if looks_like_env_name(token) {
                out.insert(token.to_string());
            }
            rest = &after[end + 1..];
        }
    }
    out
}

fn sorted(set: &BTreeSet<String>) -> Vec<String> {
    set.iter().cloned().collect()
}

#[test]
fn the_guide_names_every_environment_variable_velocity_reads() {
    let reads = scan_all_reads();
    assert!(
        reads.unclassified.is_empty(),
        "the scan found environment reads its rules do not classify: {:?} - a new name is \
         either a knob that belongs in USER_GUIDE.md and in a bucket here, or a write that \
         belongs in NOT_A_KNOB with the reason it is not a knob",
        sorted(&reads.unclassified)
    );
    // A scan that silently found nothing would let an empty guide pass.
    assert!(
        reads.knobs.len() + reads.families.len() >= 80,
        "the scan read only {} names and {} families from {} crates; a rule that stopped \
         matching, or a crate that stopped being walked, would otherwise be indistinguishable \
         from a guide that is up to date",
        reads.knobs.len(),
        reads.families.len(),
        CRATES.len()
    );

    let documented = documented_names();
    let missing: Vec<String> = reads
        .knobs
        .iter()
        .chain(reads.families.iter())
        .filter(|name| !documented.contains(*name))
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "USER_GUIDE.md's environment section omits {} variable(s) the code reads: {missing:?} - \
         this section is where a reader learns these exist, so each needs a row naming what it \
         accepts and what it defaults to",
        missing.len()
    );

    for (name, reason) in NOT_A_KNOB {
        assert!(
            reads
                .seen_in_source
                .iter()
                .any(|seen| seen.contains(*name) || seen.starts_with(*name)),
            "{name} is excluded from the knob set as \"{reason}\", but no production line \
             carries it any more - either the code moved and this exclusion should go, or the \
             name is back and needs documenting"
        );
    }
}

#[test]
fn the_guide_invents_no_environment_variable() {
    let reads = scan_all_reads();
    let documented = documented_names();
    let invented: Vec<String> = documented
        .iter()
        .filter(|name| {
            !reads.knobs.contains(*name) && !reads.families.contains(*name) && !is_excluded(name)
        })
        .cloned()
        .collect();
    assert!(
        invented.is_empty(),
        "USER_GUIDE.md documents {} environment variable(s) no scanned crate reads: {invented:?} \
         - a reference a reader can set and nothing honours is worse than no reference at all",
        invented.len()
    );
}

#[test]
fn the_guide_states_the_credential_order_the_code_keeps() {
    // The provider tables used to promise that environment variables "take
    // precedence over the workspace settings file". `usage.rs` does the opposite:
    // it reads the settings file, returns it when that provider is configured
    // there, and only then asks the environment.
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("velocity-mcp/src/usage.rs")).expect("usage.rs");
    let body = production_body(&text).join("\n");

    let mut checked = 0usize;
    for loader in [
        "pub fn load_accounts(",
        "pub fn load_openrouter_accounts(",
        "pub fn load_azure_accounts(",
    ] {
        let Some(start) = body.find(loader) else {
            panic!("{loader} is gone, so the order it proves is no longer proven");
        };
        let end = body[start..]
            .find("\npub fn ")
            .map(|offset| start + offset)
            .unwrap_or(body.len());
        let function = &body[start..end];
        let settings = function
            .find("is_configured()")
            .unwrap_or_else(|| panic!("{loader} no longer consults the settings file"));
        let environment = function
            .find("_from_env()")
            .unwrap_or_else(|| panic!("{loader} no longer falls back to the environment"));
        assert!(
            settings < environment,
            "{loader} now asks the environment before the settings file; the guide says the \
             settings file wins, so one of them has to change"
        );
        checked += 1;
    }
    assert_eq!(
        checked, 3,
        "not every credential loader was checked for order"
    );

    let guide = std::fs::read_to_string(root.join("USER_GUIDE.md")).expect("USER_GUIDE.md");
    for sentence in guide.split(['.', '\n']) {
        let lower = sentence.to_lowercase();
        let names_environment = lower.contains("environment variable");
        let claims_it_wins = names_environment
            && (lower.contains("take precedence")
                || lower.contains("takes precedence")
                || lower.contains("override")
                || lower.contains("instead of"));
        assert!(
            !claims_it_wins,
            "the guide lets the environment outrank the settings file, which is what `usage.rs` \
             does not do: {sentence}"
        );
    }

    let section = guide_section().join("\n").to_lowercase();
    assert!(
        section.contains("provider-settings.json") && section.contains("environment"),
        "the environment section never says where a configured provider is looked for first"
    );
}

#[test]
fn a_test_module_declaration_leaves_the_code_after_it_visible() {
    let text = r#"const A: u32 = 1;
#[cfg(test)]
mod helpers;
const HOST: &str = "OLLAMA_HOST";
fn main() {}
"#;
    let body = production_body(text).join("\n");
    assert!(
        body.contains("OLLAMA_HOST"),
        "the skip ran past the `#[cfg(test)] mod` declaration it belonged to, so every line \
         between it and the next brace is invisible to the scan: {body:?}"
    );
}

#[test]
fn a_one_line_test_module_declaration_is_the_only_line_dropped() {
    let text = r#"#[cfg(test)] mod helpers;
const HOST: &str = "OLLAMA_HOST";
"#;
    let body = production_body(text).join("\n");
    assert!(
        body.contains("OLLAMA_HOST") && !body.contains("mod helpers"),
        "a declaration written on one line has to cost exactly that line: {body:?}"
    );
}

#[test]
fn a_test_module_body_is_not_read_as_production_code() {
    let text = r#"fn real() { lookup("OLLAMA_HOST"); }
#[cfg(test)]
mod tests {
    fn fake() { lookup("VELOCITY_TEST_ONLY"); }
}
fn after() {}
"#;
    let body = production_body(text).join("\n");
    assert!(
        body.contains("OLLAMA_HOST") && body.contains("fn after() {}"),
        "the brace match stopped finding production code: {body:?}"
    );
    assert!(
        !body.contains("VELOCITY_TEST_ONLY"),
        "the test module's own body was read as production code: {body:?}"
    );
}

/// Quoted nowhere in the repository except in this file.
const SELF_SCAN_PROBE: &str = "VELOCITY_GUARD_SELFSCAN_PROBE";

#[test]
fn the_guard_is_not_evidence_about_itself() {
    assert!(
        looks_like_env_name(SELF_SCAN_PROBE),
        "the probe name stopped looking like an environment variable, so the check below proves \
         nothing"
    );
    let reads = scan_all_reads();
    assert!(
        !reads.seen_in_source.contains(SELF_SCAN_PROBE),
        "the walk read this file, which means an exclusion's liveness is certified by the very \
         list it is supposed to audit: renaming a `NOT_A_KNOB` entry would still find its new \
         name quoted a few lines above the assertion meant to notice"
    );
}
