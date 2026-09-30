//! Markdown skills — workspace-defined knowledge files that turn project
//! conventions into agent context without anyone prompting for them.
//!
//! A skill is a plain markdown file under `<workspace>/.velocity/skills/`
//! with optional YAML-style frontmatter:
//!
//! ```markdown
//! ---
//! name: migration-hygiene
//! description: How to add schema migrations safely
//! triggers: migration, schema, sqlite
//! globs: src/db/*.rs, **/migrations/**
//! ---
//! Always add a new migration file instead of editing a shipped one...
//! ```
//!
//! Two consumption paths, mirroring how external products make skills work
//! but with one structural difference (auto-activation):
//! * **Progressive disclosure**: dispatched workers receive a PROJECT SKILLS
//!   index (name + description only) and pull any full body on demand via the
//!   `use_skill` tool — so browsing costs almost nothing in context.
//!   (The team subsystem's `list_skills` tool covers the `.nda` executable
//!   skills kept beside these `.md` files; this module is the knowledge half.)
//! * **Task-conditioned auto-activation** (the part the competition does via
//!   manual invocation): when a worker is dispatched, skills whose `triggers`
//!   appear in the task text or whose `globs` match a scope file are inlined
//!   into the worker's instructions *before it starts*, bounded by a char
//!   budget. Skills with no triggers/globs are always active.
//!
//! Like [`hooks`](super::hooks) and [`custom_tools`](super::custom_tools),
//! skills are disk-backed with no process-global cache: every load reads the
//! directory, so editing a skill file takes effect on the next dispatch and
//! parallel tests stay isolated.

use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

/// One parsed skill file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Display name from frontmatter; defaults to the file stem.
    pub name: String,
    /// One-line summary from frontmatter; defaults to the first body line.
    pub description: String,
    /// Lowercased keywords that activate the skill when found in task text.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// Path globs that activate the skill when they match a scope file.
    #[serde(default)]
    pub globs: Vec<String>,
    /// Markdown body after the frontmatter.
    #[serde(skip)]
    pub content: String,
    /// Path of the file this skill was loaded from.
    pub path: String,
}

impl Skill {
    /// Whether this skill should speak for a given task. No triggers and no
    /// globs means always-active general guidance.
    pub fn is_active_for(&self, task_text_lower: &str, scope_files: &[String]) -> bool {
        if self.triggers.is_empty() && self.globs.is_empty() {
            return true;
        }
        self.triggers
            .iter()
            .any(|t| !t.is_empty() && task_text_lower.contains(t.as_str()))
            || scope_files
                .iter()
                .any(|f| self.globs.iter().any(|g| path_matches_glob(g, f)))
    }
}

/// Split an optional leading frontmatter block from a markdown document.
/// Returns (frontmatter lines, body). Documents without a `---` first line
/// yield an empty frontmatter and the whole text as body.
fn split_frontmatter(raw: &str) -> (Vec<&str>, String) {
    let mut lines = raw.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (Vec::new(), raw.to_string());
    }
    let mut fm = Vec::new();
    let mut body_start = None;
    for (i, line) in raw.lines().enumerate().skip(1) {
        if line.trim() == "---" {
            body_start = Some(i + 1);
            break;
        }
        fm.push(line);
    }
    let body = match body_start {
        Some(start) => raw.lines().skip(start).collect::<Vec<_>>().join("\n"),
        // Unterminated frontmatter: treat everything as body (no silent loss).
        None => raw.to_string(),
    };
    (fm, body)
}

/// Parse one skill file. Unparseable files are skipped by the caller with a
/// warning — one broken skill must never hide the others.
pub fn parse_skill(path: &Path, raw: &str) -> Skill {
    let (fm_lines, content) = split_frontmatter(raw);
    let mut name = None;
    let mut description = None;
    let mut triggers = Vec::new();
    let mut globs = Vec::new();
    for line in fm_lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match key.trim() {
            "name" => name = Some(value.to_string()),
            "description" => description = Some(value.to_string()),
            "triggers" => triggers = split_list(value),
            "globs" => globs = split_list(value),
            _ => {}
        }
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    // Body fallback for description: first non-empty, non-heading line.
    let description = description.unwrap_or_else(|| {
        content
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))
            .unwrap_or("(no description)")
            .chars()
            .take(120)
            .collect()
    });
    Skill {
        name: name.unwrap_or(stem),
        description,
        triggers,
        globs,
        content,
        path: path.display().to_string(),
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Load every skill from `<workspace>/.velocity/skills/*.md`, sorted by name
/// for deterministic ordering. Missing directory means no skills.
pub fn load_skills(workspace_root: &Path) -> Vec<Skill> {
    let dir = workspace_root.join(".velocity").join("skills");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut skills: Vec<Skill> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("md") {
                return None;
            }
            let raw = match fs::read_to_string(&path) {
                Ok(r) => r,
                Err(err) => {
                    log::warn!("skills: cannot read {}: {err}", path.display());
                    return None;
                }
            };
            Some(parse_skill(&path, &raw))
        })
        .collect();
    skills.sort_by_key(|s| s.name.to_lowercase());
    skills
}

/// Char-level glob match for a single path segment: `*` and `?` wildcards,
/// case-insensitive (these are file paths, not identifiers).
fn seg_matches(pattern: &str, segment: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = segment.to_lowercase().chars().collect();
    let (mut i, mut j) = (0usize, 0usize);
    let (mut star, mut star_match) = (usize::MAX, 0usize);
    while i < t.len() {
        if j < p.len() && (p[j] == t[i] || p[j] == '?') {
            i += 1;
            j += 1;
        } else if j < p.len() && p[j] == '*' {
            star = j;
            star_match = i;
            j += 1;
        } else if star != usize::MAX {
            j = star + 1;
            star_match += 1;
            i = star_match;
        } else {
            return false;
        }
    }
    while j < p.len() && p[j] == '*' {
        j += 1;
    }
    j == p.len()
}

/// Segment-level match with `**` (zero or more whole segments).
fn glob_segments(pat: &[&str], path: &[&str]) -> bool {
    match (pat.first(), path.first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        // Pattern has content but the path is exhausted: only `**` segments
        // can still match (they may stand for zero segments).
        (Some(_), None) => pat.iter().all(|p| *p == "**"),
        (Some(&"**"), _) => {
            glob_segments(&pat[1..], path) || (!path.is_empty() && glob_segments(pat, &path[1..]))
        }
        (Some(&p), Some(&s)) => seg_matches(p, s) && glob_segments(&pat[1..], &path[1..]),
    }
}

/// Match a file path against a skill glob. Patterns without `/` match the
/// file name anywhere in the tree (`*.rs` covers `src/main.rs`); patterns
/// with `/` match the full relative path with `**` cross-directory support.
pub fn path_matches_glob(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim().trim_start_matches("./");
    if pattern.is_empty() || path.is_empty() {
        return false;
    }
    let norm = path.replace('\\', "/");
    if !pattern.contains('/') {
        let file = norm.rsplit('/').next().unwrap_or("");
        return seg_matches(pattern, file);
    }
    let pat: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let segs: Vec<&str> = norm.split('/').filter(|s| !s.is_empty()).collect();
    glob_segments(&pat, &segs)
}

/// Build the worker-injection block for skills.
///
/// Active skills are inlined verbatim until `max_inline_chars` is spent;
/// everything after that (and everything inactive) is demoted to a one-line
/// index the agent can pull from with `use_skill`. Returns an empty string
/// when the workspace has no skills, so the caller can leave instructions
/// untouched.
pub fn skills_brief(
    workspace_root: &Path,
    task_text: &str,
    scope_files: &[String],
    max_inline_chars: usize,
) -> String {
    let skills = load_skills(workspace_root);
    if skills.is_empty() {
        return String::new();
    }
    let task_lower = task_text.to_lowercase();
    let mut out = String::from(
        "PROJECT SKILLS (workspace conventions from .velocity/skills — follow them):\n",
    );
    let mut index_lines: Vec<String> = Vec::new();
    let mut inlined = 0usize;
    for skill in &skills {
        let is_active = skill.is_active_for(&task_lower, scope_files);
        if is_active && skill.content.trim().len() + inlined <= max_inline_chars {
            out.push_str(&format!(
                "## Skill: {} ({})\n{}\n\n",
                skill.name,
                skill.path,
                skill.content.trim()
            ));
            inlined += skill.content.trim().len();
        } else {
            let mode = if is_active {
                "active, over inline budget"
            } else {
                "on demand"
            };
            index_lines.push(format!(
                "- {} — {} ({}; call use_skill(\"{}\") for the full text)",
                skill.name, skill.description, mode, skill.name
            ));
        }
    }
    if !index_lines.is_empty() {
        out.push_str("Other skills:\n");
        out.push_str(&index_lines.join("\n"));
        out.push('\n');
    }
    out
}

// ── Tool handlers ─────────────────────────────────────────────────────────

/// Index of every skill without the bodies. Not routed as its own tool
/// (`list_skills` is taken by the team subsystem's `.nda` skills); exposed
/// for the GUI panels and tests that want the structured list.
pub fn handle_list_skills(root: &Path) -> Result<String, String> {
    let skills = load_skills(root);
    let items: Vec<Value> = skills
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "description": s.description,
                "triggers": s.triggers,
                "globs": s.globs,
                "path": s.path,
                "contentChars": s.content.chars().count(),
            })
        })
        .collect();
    Ok(json!({ "success": true, "count": items.len(), "skills": items }).to_string())
}

/// `use_skill` — full markdown body of one skill by name (case-insensitive).
pub fn handle_use_skill(root: &Path, arguments: &Value) -> Result<String, String> {
    let want = arguments["name"].as_str().ok_or("name is required")?;
    let skills = load_skills(root);
    let lowered = want.to_lowercase();
    match skills.iter().find(|s| s.name.to_lowercase() == lowered) {
        Some(skill) => Ok(json!({
            "success": true,
            "name": skill.name,
            "description": skill.description,
            "content": skill.content,
        })
        .to_string()),
        None => Ok(json!({
            "success": false,
            "error": format!("no skill named '{want}' (see the PROJECT SKILLS index or .velocity/skills/)"),
        })
        .to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill(root: &Path, file: &str, content: &str) {
        let dir = root.join(".velocity").join("skills");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(file), content).unwrap();
    }

    #[test]
    fn frontmatter_parses_and_defaults_from_file_and_body() {
        let dir = TempDir::new().unwrap();
        write_skill(
            dir.path(),
            "migration-hygiene.md",
            "---\nname: Migrations\ndescription: Safe schema changes\ntriggers: migration, schema\nglobs: src/db/*.rs\n---\nNever edit shipped migrations.\n",
        );
        write_skill(
            dir.path(),
            "plain-skill.md",
            "No frontmatter here.\nSecond line.\n",
        );
        let skills = load_skills(dir.path());
        assert_eq!(skills.len(), 2);
        let mig = skills.iter().find(|s| s.name == "Migrations").unwrap();
        assert_eq!(
            mig.triggers,
            vec!["migration".to_string(), "schema".to_string()]
        );
        assert_eq!(mig.globs, vec!["src/db/*.rs".to_string()]);
        assert!(mig.content.starts_with("Never edit shipped migrations."));
        // Defaults: name from file stem, description from first body line.
        let plain = skills.iter().find(|s| s.name == "plain-skill").unwrap();
        assert_eq!(plain.description, "No frontmatter here.");
        assert!(plain.triggers.is_empty() && plain.globs.is_empty());
    }

    #[test]
    fn activation_by_trigger_keyword_glob_or_always_on() {
        let dir = TempDir::new().unwrap();
        write_skill(
            dir.path(),
            "a.md",
            "---\ntriggers: auth, token\n---\nbody a\n",
        );
        write_skill(dir.path(), "b.md", "---\nglobs: **/db/*.rs\n---\nbody b\n");
        write_skill(dir.path(), "c.md", "general guidance, no frontmatter\n");
        let skills = load_skills(dir.path());
        let scope = vec!["src/store/db/session.rs".to_string()];
        let task = "Fix login REFRESH token expiry".to_lowercase();
        let by = |name: &str| {
            skills
                .iter()
                .find(|s| s.name == name)
                .unwrap()
                .is_active_for(&task, &scope)
        };
        assert!(by("a"), "trigger 'token' appears in task text");
        assert!(by("b"), "glob **/db/*.rs matches scope file");
        assert!(by("c"), "no triggers/globs means always active");
        assert!(!skills[0].is_active_for("nothing relevant".to_lowercase().as_str(), &[]));
    }

    #[test]
    fn glob_matcher_covers_star_double_star_and_filename_only() {
        assert!(path_matches_glob("*.rs", "src/main.rs"));
        assert!(path_matches_glob("src/db/*.rs", "src/db/session.rs"));
        assert!(!path_matches_glob("src/db/*.rs", "src/net/session.rs"));
        assert!(path_matches_glob("**/db/*.rs", "a/b/db/c.rs"));
        assert!(path_matches_glob("docs/**", "docs/guide/intro.md"));
        assert!(!path_matches_glob("docs/**", "readme.md"));
        assert!(path_matches_glob("README.md", "any/where/README.md"));
    }

    #[test]
    fn skills_brief_inlines_active_and_indexes_rest() {
        let dir = TempDir::new().unwrap();
        write_skill(
            dir.path(),
            "small.md",
            "---\ntriggers: widget\n---\nINLINE ME\n",
        );
        write_skill(
            dir.path(),
            "big.md",
            format!(
                "---\ntriggers: widget\ndescription: too big\n---\n{}",
                "X".repeat(1500)
            )
            .as_str(),
        );
        write_skill(
            dir.path(),
            "off.md",
            "---\nname: dormant\ndescription: not triggered\ntriggers: zebra\n---\nnope\n",
        );
        let brief = skills_brief(dir.path(), "build a widget", &[], 1000);
        assert!(brief.contains("INLINE ME"), "active small skill is inlined");
        assert!(
            brief.contains("too big") && brief.contains("over inline budget"),
            "active oversize skill demoted to index"
        );
        assert!(
            brief.contains("not triggered"),
            "inactive skill still discoverable"
        );
        assert!(brief.contains("use_skill(\"dormant\")"));
        // No skills directory -> empty brief so callers skip injection.
        assert!(skills_brief(TempDir::new().unwrap().path(), "x", &[], 1000).is_empty());
    }

    #[test]
    fn list_and_use_skill_tools_round_trip() {
        let dir = TempDir::new().unwrap();
        write_skill(
            dir.path(),
            "style.md",
            "---\ndescription: Code style rules\n---\nUse tracing.\n",
        );
        let listed: Value = serde_json::from_str(&handle_list_skills(dir.path()).unwrap()).unwrap();
        assert_eq!(listed["count"], json!(1));
        assert_eq!(listed["skills"][0]["name"], json!("style"));
        // Case-insensitive lookup, full body returned.
        let used: Value =
            serde_json::from_str(&handle_use_skill(dir.path(), &json!({"name": "STYLE"})).unwrap())
                .unwrap();
        assert_eq!(used["success"], json!(true));
        assert!(used["content"].as_str().unwrap().contains("Use tracing."));
        let missing: Value =
            serde_json::from_str(&handle_use_skill(dir.path(), &json!({"name": "ghost"})).unwrap())
                .unwrap();
        assert_eq!(missing["success"], json!(false));
    }

    #[test]
    fn broken_frontmatter_never_hides_sibling_skills() {
        let dir = TempDir::new().unwrap();
        // Unterminated frontmatter: whole file treated as body.
        write_skill(
            dir.path(),
            "broken.md",
            "---\nname: unterminated\nno closing fence\n",
        );
        write_skill(dir.path(), "fine.md", "---\nname: fine\n---\nok\n");
        let skills = load_skills(dir.path());
        assert_eq!(skills.len(), 2);
        assert!(skills.iter().any(|s| s.name == "fine"));
    }
}
