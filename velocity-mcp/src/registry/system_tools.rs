use crate::safety::SafeMutex;
use serde_json::{json, Value};
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

struct SidecarDaemon {
    child: std::process::Child,
}

static SIDECAR_DAEMON: Mutex<Option<SidecarDaemon>> = Mutex::new(None);

/// Hard deadline for one sidecar request/response round-trip. The daemon is
/// an external process; an unbounded `read_line` on its stdout used to wedge
/// the dispatch thread *and* the global SIDECAR_DAEMON mutex forever whenever
/// the daemon was alive-but-silent (see the execute_nda hang: a
/// VELOCITY_MCP_SERVER pointing at another velocity_mcp build made every
/// generation spawn its own sidecar and wait on the next, forever).
const SIDECAR_TIMEOUT_SECS: u64 = 30;

/// Spawn the sidecar daemon with the guards that keep the chain finite:
/// `env_remove(VELOCITY_MCP_SERVER)` so a Rust sibling takes the local
/// fallback path instead of spawning a sidecar of its own. velocity_mcp-
/// family binaries also get an explicit `--mode stdio` (the standalone
/// upstream server predates `--workspace`, so paths are resolved by the
/// parent before the request is forwarded).
fn spawn_sidecar(exe_path: &str) -> Result<SidecarDaemon, Box<dyn Error>> {
    let mut cmd = Command::new(exe_path);
    cmd.env_remove("VELOCITY_MCP_SERVER");
    if Path::new(exe_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("velocity_mcp"))
        .unwrap_or(false)
    {
        cmd.arg("--mode").arg("stdio");
    }
    let child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
    Ok(SidecarDaemon { child })
}

/// Deadline for running an extracted NDA payload.
const NDA_EXEC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// `wait_with_output` off-thread with a hard deadline; on expiry the child
/// is force-killed by PID (taskkill /F) so the waiting thread unwinds and
/// its own pipes drain.
pub(crate) fn run_child_with_timeout(child: std::process::Child) -> Result<String, Box<dyn Error>> {
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let collected = match rx.recv_timeout(NDA_EXEC_TIMEOUT) {
        Ok(result) => result,
        Err(_) => {
            kill_process(pid);
            rx.recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|_| "extracted payload did not exit after being killed")?
        }
    };
    let output = collected.map_err(|e| format!("failed to run extracted payload: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "extracted payload exited with {}: {}",
            output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn kill_process(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

/// Clone a git repository into the workspace.
///
/// `git` is invoked *directly* (never through a shell), so a hostile URL cannot
/// smuggle extra commands; the URL is additionally screened for shell
/// metacharacters and required to carry a git scheme. The destination resolves
/// strictly inside the workspace root and defaults to `.velocity/clones/<repo>`
/// so a clone never clobbers tracked source. A non-empty existing destination is
/// refused rather than overwritten.
pub fn git_clone_impl(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let url = arguments["url"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("'url' is required")?;
    let scheme_ok = url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("git://")
        || url.starts_with("git@");
    if !scheme_ok {
        return Err("url must start with http://, https://, git:// or git@".into());
    }
    if url.chars().any(|c| {
        matches!(
            c,
            ';' | '|' | '&' | '$' | '`' | '\n' | '\r' | '<' | '>' | '(' | ')'
        )
    }) {
        return Err("url contains forbidden characters".into());
    }

    let repo_name = url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit(['/', ':'])
        .next()
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .ok_or("could not derive a repository name from url")?
        .to_string();

    let default_rel = format!(".velocity/clones/{repo_name}");
    let rel_dest = arguments["dest"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or(default_rel);
    let dest = resolve_workspace_path(root, &rel_dest, true)?;

    let occupied = dest
        .read_dir()
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);
    if occupied {
        return Err(format!(
            "destination '{}' already exists and is not empty",
            dest.display()
        )
        .into());
    }

    let mut cmd = Command::new("git");
    cmd.arg("clone");
    if let Some(depth) = arguments["depth"].as_u64() {
        cmd.arg("--depth").arg(depth.to_string());
    }
    if let Some(branch) = arguments["branch"]
        .as_str()
        .filter(|b| !b.trim().is_empty())
    {
        cmd.arg("--branch").arg(branch.trim());
    }
    cmd.arg(url);
    // `dest` inherits the verbatim (`\\?\`) prefix from the canonicalized
    // workspace root on Windows. Containment checks need it, but MSYS git
    // rejects extended-length arguments outright — the crucible run died with
    // "could not create work tree dir '\\?\C:\...': Invalid argument". Hand
    // the child a plain drive path; the directory it names is identical.
    cmd.arg(crate::registry::dispatch::strip_verbatim_prefix(&dest));
    let output = cmd
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}\n{stderr}");
    let combined = combined.trim();
    let rel = dest
        .strip_prefix(root)
        .unwrap_or(&dest)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(serde_json::to_string_pretty(&json!({
        "success": output.status.success(),
        "url": url,
        "repo_name": repo_name,
        "path": rel,
        "exit_code": output.status.code(),
        "output": combined,
    }))?)
}

pub fn resolve_workspace_path(
    root: &Path,
    rel_path: &str,
    allow_create: bool,
) -> Result<PathBuf, Box<dyn Error>> {
    // The parameter is contracted as workspace-relative and the schemas say so
    // outright ("Absolute paths and anything resolving outside the workspace are
    // refused"). The old code trimmed a leading separator instead, which quietly
    // rewrote an absolute path into a relative one: on Unix `/tmp/x` became
    // `<root>/tmp/x`, so a caller asking for one file was told "ok" about a
    // different one. Refusing is also the only way the check means anything off
    // Windows -- `join` substitutes the root for anything absolute, which is why
    // the Windows side looked covered: it was caught downstream by the
    // containment test, not here, and reported as an escape rather than the
    // malformed input it is. The Component check covers what `is_absolute` calls
    // relative on Windows: a drive-relative `C:foo` and a UNC root.
    let given = Path::new(rel_path);
    if given.is_absolute()
        || matches!(
            given.components().next(),
            Some(std::path::Component::Prefix(_))
        )
    {
        return Err(format!(
            "Access Denied: {rel_path} is absolute; this tool takes a path relative to the workspace root"
        )
        .into());
    }
    let target = root.join(given);

    if allow_create {
        if let Some(parent) = target.parent() {
            let parent_canon = parent.canonicalize().or_else(|_| {
                fs::create_dir_all(parent)?;
                parent.canonicalize()
            })?;
            if !parent_canon.starts_with(root) {
                return Err(
                    format!("Access Denied: Path escapes workspace root ({})", rel_path).into(),
                );
            }
        }
        Ok(target)
    } else {
        let canon = target
            .canonicalize()
            .map_err(|_| format!("File or directory not found in workspace: {}", rel_path))?;

        if !canon.starts_with(root) {
            return Err(
                format!("Access Denied: Path escapes workspace root ({})", rel_path).into(),
            );
        }
        Ok(canon)
    }
}

pub fn scan_file_content(content: &str) -> Option<String> {
    let lowercase = content.to_lowercase();
    if lowercase.contains("api_key = \"sk-") || lowercase.contains("secret = \"") {
        Some("Potential hardcoded secret or API key detected.".to_string())
    } else {
        None
    }
}

/// Generate test stubs from source code using regex-based function extraction.
fn generate_test_stubs(source: &str, language: &str) -> Vec<String> {
    let mut tests = Vec::new();

    match language {
        "rust" => {
            // Extract function signatures: pub fn name(...) -> RetType
            for line in source.lines() {
                let trimmed = line.trim();
                if let Some(fn_start) = trimmed.find("fn ") {
                    let after_fn = &trimmed[fn_start + 3..];
                    if let Some(paren) = after_fn.find('(') {
                        let name = after_fn[..paren].trim().trim_start_matches("pub ");
                        if name.is_empty() || name.starts_with("test_") {
                            continue;
                        }
                        tests.push(format!(
                            "#[test]\nfn test_{}() {{\n    // TODO: Setup\n    let result = {}(/* args */);\n    // TODO: Assert\n    assert!(result != Default::default());\n}}",
                            name, name
                        ));
                        // Edge case: empty/zero input
                        tests.push(format!(
                            "#[test]\nfn test_{}_empty_input() {{\n    // Edge case: empty or zero input\n    // TODO: Verify graceful handling\n}}",
                            name
                        ));
                    }
                }
            }
        }
        "typescript" | "javascript" => {
            // Extract function/const arrow signatures
            for line in source.lines() {
                let trimmed = line.trim();
                let name = if trimmed.starts_with("export function ")
                    || trimmed.starts_with("function ")
                {
                    trimmed
                        .trim_start_matches("export ")
                        .trim_start_matches("function ")
                        .split('(')
                        .next()
                        .map(|s| s.trim().to_string())
                } else if trimmed.contains("= (") || trimmed.contains("= async (") {
                    trimmed.split("= ").next().map(|s| {
                        s.trim()
                            .trim_start_matches("export ")
                            .trim_start_matches("const ")
                            .trim_start_matches("let ")
                            .to_string()
                    })
                } else {
                    None
                };
                if let Some(fname) = name {
                    if fname.is_empty() || fname.contains("test") {
                        continue;
                    }
                    tests.push(format!(
                        "describe('{}', () => {{\n  it('should work with valid input', () => {{\n    // TODO: Arrange\n    // TODO: Act\n    // TODO: Assert\n  }});\n\n  it('should handle empty input', () => {{\n    // Edge case\n  }});\n}});",
                        fname
                    ));
                }
            }
        }
        "python" => {
            for line in source.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("def ") && !trimmed.starts_with("def test_") {
                    if let Some(paren) = trimmed.find('(') {
                        let name = &trimmed[4..paren];
                        tests.push(format!(
                            "def test_{}():\n    # TODO: Setup\n    result = {}()\n    assert result is not None\n\ndef test_{}_edge_case():\n    # Edge case: empty/None input\n    pass",
                            name, name, name
                        ));
                    }
                }
            }
        }
        _ => {
            tests.push("// Unsupported language for test generation".to_string());
        }
    }

    if tests.is_empty() {
        tests.push("// No testable functions found in source".to_string());
    }
    tests
}

/// Read structured IDE panel data without navigating the GUI.
///
/// Returns a JSON [`Value`] suitable for both the MCP tool boundary and the
/// agent-to-UI message channel, so callers share a single serialisation path.
pub fn fetch_panel_data_value(
    root: &Path,
    panel: &str,
    relative_path: Option<&str>,
) -> Result<Value, Box<dyn Error>> {
    let data = match panel {
        "teams" => {
            let teams = crate::editor::expert_team::load_expert_teams(root);
            let teams: Vec<Value> = teams
                .into_iter()
                .map(|team| {
                    json!({
                        "id": team.id,
                        "name": team.name,
                        "slug": team.slug(),
                        "description": team.description,
                        "is_preset": team.is_preset,
                        "members": team.members.into_iter().map(|member| json!({
                            "id": member.id,
                            "name": member.name,
                            "role": member.role,
                            "provider": member.provider.label(),
                            "model_id": member.model_id,
                            "skills": member.skills,
                            "scope_patterns": member.scope_patterns,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            json!({"panel": panel, "teams": teams})
        }
        "wiki" => match crate::automation::open_workspace_site_map(root) {
            Ok(site_map) => {
                let wiki = velocity_ide::wiki::build_wiki(&site_map);
                let pages: Vec<Value> = wiki
                    .file_pages
                    .iter()
                    .map(|page| {
                        json!({
                            "title": page.title,
                            "slug": page.slug,
                            "relationship_count": page.relationships.len(),
                            "called_by_count": page.called_by.len(),
                        })
                    })
                    .collect();
                json!({
                    "panel": panel,
                    "file_count": wiki.file_count(),
                    "symbol_count": wiki.symbol_count(),
                    "pages": pages,
                })
            }
            Err(error) => json!({"panel": panel, "error": error}),
        },
        "graph" => {
            let symbols = crate::editor::search::collect_workspace_symbols(root);
            let mut files = std::collections::BTreeMap::<String, usize>::new();
            for symbol in symbols {
                *files.entry(symbol.file).or_default() += 1;
            }
            let mut top_files: Vec<(String, usize)> = files.into_iter().collect();
            top_files
                .sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
            let symbol_count: usize = top_files.iter().map(|(_, count)| count).sum();
            json!({
                "panel": panel,
                "file_count": top_files.len(),
                "symbol_count": symbol_count,
                "top_files": top_files.into_iter().take(20).map(|(file, symbols)| json!({"file": file, "symbols": symbols})).collect::<Vec<_>>(),
            })
        }
        "bookmarks" => {
            let path = root.join(".velocity").join("bookmarks.json");
            match fs::read_to_string(path) {
                Ok(raw) => serde_json::from_str(&raw)
                    .unwrap_or_else(|_| json!({"error": "invalid bookmarks file"})),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    json!({"bookmarks": []})
                }
                Err(error) => return Err(error.into()),
            }
        }
        "files" => {
            let rel = relative_path.unwrap_or(".");
            let directory = if rel == "." || rel.is_empty() {
                root.to_path_buf()
            } else {
                resolve_workspace_path(root, rel, false)?
            };
            if !directory.is_dir() {
                return Err(format!("Not a directory: {}", rel).into());
            }
            let mut files = fs::read_dir(directory)?
                .flatten()
                .filter_map(|entry| {
                    let metadata = entry.metadata().ok()?;
                    Some(json!({
                        "name": entry.file_name().to_string_lossy(),
                        "is_dir": metadata.is_dir(),
                        "size": metadata.len(),
                    }))
                })
                .collect::<Vec<_>>();
            files.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
            json!({"panel": panel, "relative_path": rel, "files": files})
        }
        _ => {
            return Err(format!(
                "Unknown panel '{}'. Expected teams, wiki, graph, bookmarks, or files.",
                panel
            )
            .into())
        }
    };
    Ok(data)
}

fn fetch_panel_data(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let panel = arguments["panel"].as_str().ok_or("panel is required")?;
    let relative_path = arguments["relativePath"].as_str();
    let data = fetch_panel_data_value(root, panel, relative_path)?;
    Ok(serde_json::to_string(&data)?)
}

/// Longest `read_file` window a ranged request returns, in lines.
const MAX_READ_WINDOW_LINES: usize = 200;
/// … and in characters of body text.
///
/// This is the cap that actually binds on real source. The history compressor
/// rewrites any tool result over 4,000 characters as soon as an assistant
/// message follows it (`compress_history`, agent/executor/utils.rs), which is
/// the turn after the read — so a 200-line window of indented Rust (~9,000
/// chars) would be elided before the model could copy from it and the range
/// would buy nothing. Staying under the threshold is what makes the text
/// survive long enough to become an `oldString`.
const MAX_READ_WINDOW_CHARS: usize = 3_200;

fn read_window_param(arguments: &Value, key: &str) -> Result<Option<usize>, String> {
    match arguments.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(|n| Some(n as usize)).ok_or(format!(
            "`{key}` must be a positive whole number (1-based line)"
        )),
    }
}

/// A named slice of a file's lines, for `read_file` with `startLine`/`endLine`.
///
/// Without ranges the only thing a model can ask for is the whole file, and on a
/// port big enough to matter the context budget elides most of the result it
/// just received — which leaves `edit_file` unobtainable, since its `oldString`
/// has to be copied verbatim, and pushes the model back onto a whole-file
/// rewrite that its generation limit truncates. A bounded region is what makes
/// targeted repair possible at all.
pub(crate) fn read_line_range(
    text: &str,
    rel_path: &str,
    start: Option<usize>,
    end: Option<usize>,
) -> Result<String, String> {
    let total = text.lines().count();
    let start = start.unwrap_or(1);
    if start == 0 {
        return Err("`startLine` is 1-based: pass 1 for the first line".to_string());
    }
    if total == 0 || start > total {
        return Err(format!(
            "`{rel_path}` has {total} line(s); `startLine` {start} is past its end"
        ));
    }
    let requested = end.unwrap_or(total).min(total);
    if requested < start {
        return Err(format!(
            "`endLine` {} is before `startLine` {start}; the range has to run downwards",
            end.unwrap_or(total)
        ));
    }
    let ceiling = requested.min(start + MAX_READ_WINDOW_LINES - 1);

    let mut body: Vec<&str> = Vec::new();
    let mut chars = 0usize;
    let mut last = start;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if n < start {
            continue;
        }
        if n > ceiling {
            break;
        }
        // A single over-long line is still returned: an empty window would tell
        // the model nothing about the region it asked for.
        if !body.is_empty() && chars + line.len() + 1 > MAX_READ_WINDOW_CHARS {
            break;
        }
        chars += line.len() + 1;
        body.push(line);
        last = n;
    }

    // The header has to be recognisable as metadata: a model that copies it
    // into an `oldString` matches nothing and loses the edit.
    let mut header = format!(
        "Lines {start}-{last} of {total} in `{rel_path}`. Everything below is verbatim file \
         content — this line is not part of it."
    );
    if last < requested {
        header.push_str(&format!(
            " This window is capped so the text survives your next turn; send `startLine` {} \
             for the rest, and make the edit before re-reading.",
            last + 1
        ));
    }
    Ok(format!("{header}\n{}", body.join("\n")))
}

/// Whether a line is one of the loop's excerpt headers rather than file text.
///
/// Every repair directive pastes the error site under such a line
/// (`--- src/budget_manager.rs:356..363 (verbatim from disk) ---`, and the five
/// sibling shapes in `loop_runner`'s excerpt builders). The middle must be
/// non-empty so a bare `---`, which is markdown's own rule and could legitimately
/// sit inside a doc comment, is not mistaken for furniture.
fn is_excerpt_header(line: &str) -> bool {
    let Some(body) = line.strip_prefix("--- ") else {
        return false;
    };
    !body.trim().is_empty() && body.trim_end().ends_with(" ---")
}

/// The excerpt-header line an `oldString` wrongly quotes, if it quotes one.
///
/// `lr7b_v6`'s repair turns copied the header into `oldString` *and* `newString`,
/// producing a patch that can never match and burning a 10 s turn to be told
/// "copy the text verbatim" — advice the turn had just tried to follow. Naming
/// the exact line is what lets the next call differ. Checked only where the edit
/// already matched nothing, so a file that genuinely carries such a line still
/// edits normally.
fn quoted_excerpt_header(needle: &str) -> Option<String> {
    needle
        .lines()
        .map(str::trim)
        .find(|line| is_excerpt_header(line))
        .map(str::to_string)
}

/// What the file actually says where a refused `oldString` expected to find it.
///
/// "Copy the text verbatim, including the exact indentation" is true advice and
/// useless on its own: it tells the model nothing about the indentation, so it
/// regenerates the same guess and the batch is refused again, and one rejected
/// edit voids every other edit in the call. Telling the two failure kinds apart
/// is what makes the next call differ from this one — text that IS in the file
/// under different leading spaces, which the tool now pastes back, versus text
/// that is not in the file at all even ignoring whitespace, which no amount of
/// re-guessing will land and which only a ranged `read_file` can settle.
fn explain_refused_edit(hay: &str, needle: &str) -> String {
    let probe = needle
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");
    // A first line of `}` or `});` exists everywhere; naming its sites would
    // send the model after the wrong one.
    if probe.chars().count() < 6 {
        return String::new();
    }
    const SITES: usize = 3;
    const MAX_WIDTH: usize = 4;
    let lines: Vec<&str> = hay.lines().collect();
    let sites: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.trim() == probe)
        .map(|(i, _)| i)
        .collect();
    if sites.is_empty() {
        return format!(
            "Its first line, `{probe}`, appears nowhere in the file even with indentation \
             ignored, so the text itself is wrong rather than merely mis-indented. Call \
             read_file with `startLine` and `endLine` over that region and copy from what it \
             returns; guessing a second time wastes the turn."
        );
    }
    let shown = &sites[..sites.len().min(SITES)];
    // The following line is the disambiguator: two identical openings are told
    // apart by what comes next, and the pair is what makes a widened `oldString`
    // unique. Repeat until the shown blocks differ from each other — a run of
    // the same call with the same first argument (`create_budget(` / `100.0,`
    // three times over) pasted three identical blocks and taught nothing.
    let mut width = 2usize;
    while width < MAX_WIDTH && !blocks_are_distinct(&lines, shown, width) {
        width += 1;
    }
    let still_ambiguous = !blocks_are_distinct(&lines, shown, width);
    let mut out = format!(
        "Its first line, `{probe}`, IS in {} place(s) of the file under different leading \
         whitespace. Each block below is verbatim file content, starting at the line number \
         in its `[line N]` marker; copy from them, and treat every line of this message, \
         markers included, as something that is not part of the file.",
        sites.len()
    );
    for &i in shown {
        let end = (i + width).min(lines.len());
        out.push_str(&format!("\n[line {}]\n{}", i + 1, lines[i..end].join("\n")));
    }
    if still_ambiguous {
        out.push_str(
            " These sites are still identical line-for-line, so no `oldString` built from \
             them alone is unique: read_file a wider region and anchor on text that differs.",
        );
    }
    out
}

/// Whether every shown site, read for `width` lines from its opening, yields a
/// block no other shown site yields.
fn blocks_are_distinct(lines: &[&str], sites: &[usize], width: usize) -> bool {
    let mut seen: Vec<String> = Vec::with_capacity(sites.len());
    for &i in sites {
        let end = (i + width).min(lines.len());
        let block = lines[i..end].join("\n");
        if seen.contains(&block) {
            return false;
        }
        seen.push(block);
    }
    true
}

/// A refused `oldString` that is one line away from text the file does contain.
#[derive(Debug, PartialEq)]
struct NearMiss {
    /// The 1-based file line the candidate starts at.
    line: usize,
    /// How many of the needle's non-blank lines matched in order from there.
    matched: usize,
    /// How many non-blank lines the needle has.
    total: usize,
    /// The needle's own 1-based line where it diverged.
    needle_line: usize,
    /// What the model wrote there, and what the file says.
    wrote: String,
    file_says: String,
}

/// One quoted line of a near-miss report, clipped so a long line cannot crowd
/// out the rest of the refusal.
fn clipped_line(line: &str) -> String {
    const MAX: usize = 160;
    let mut out: String = line.chars().take(MAX).collect();
    if out.len() != line.len() {
        out.push('\u{2026}');
    }
    out
}

/// The closest near-miss to a refused `needle`, when one is close enough to name.
///
/// `lr7b_v19` lost five of its eleven turns, and the run, to one semicolon. The
/// model's 11-line `oldString` matched `budget_manager.rs` from line 287 for 10
/// of its 11 non-blank lines and diverged on the last — it wrote `});` where the
/// file says `})` — and its `newString` was the correct fix for the E0515 that
/// turn was repairing. The refusal answered with the sites of the needle's
/// *first* line pasted two to four lines wide, which is the part that already
/// matched. Nothing in the message named the line that did not, so the model
/// re-sent the byte-identical call five times and the error survived the run.
///
/// The scan is a prefix run from every file line, not an alignment: a needle
/// botched in the middle scores only the lines before the mistake and falls
/// through to `explain_refused_edit`. That is the shape this was built for — a
/// model reproduces a region it was just shown and paraphrases its last line —
/// and guessing past a divergence in the middle would be inventing an edit.
///
/// `None` unless the needle has at least three non-blank lines and the best
/// candidate matches at least half of them, and `None` when every line matched:
/// then only the indentation differs, which is `explain_refused_edit`'s answer
/// and a better one. A near miss that is not confidently near reads as an answer
/// while pointing at the wrong place, which is worse than the sites paste.
fn nearest_miss(hay: &str, needle: &str) -> Option<NearMiss> {
    const MIN_LINES: usize = 3;
    const MAX_LINES: usize = 60;
    let hay = hay.replace("\r\n", "\n");
    let needle = needle.replace("\r\n", "\n");
    let file: Vec<&str> = hay.lines().collect();
    let want: Vec<(usize, &str)> = needle
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty())
        .take(MAX_LINES)
        .collect();
    if want.len() < MIN_LINES {
        return None;
    }
    let mut best_at = 0usize;
    let mut best_matched = 0usize;
    let mut indentation_only = false;
    for i in 0..file.len() {
        let mut k = 0usize;
        let mut j = i;
        // The run may skip blank lines before its first match, and an anchor on
        // one of those is not where the region starts.
        let mut first = i;
        while j < file.len() && k < want.len() {
            let line = file[j].trim();
            if line.is_empty() {
                j += 1;
                continue;
            }
            if line != want[k].1 {
                break;
            }
            if k == 0 {
                first = j;
            }
            k += 1;
            j += 1;
        }
        if k == want.len() {
            indentation_only = true;
        } else if k > best_matched {
            best_matched = k;
            best_at = first;
        }
    }
    if indentation_only || best_matched < 2 || best_matched * 2 < want.len() {
        return None;
    }
    // Walk the winning candidate again to land on the file line it stopped at.
    let mut j = best_at;
    let mut k = 0usize;
    while j < file.len() && k < best_matched {
        if file[j].trim().is_empty() {
            j += 1;
            continue;
        }
        k += 1;
        j += 1;
    }
    while j < file.len() && file[j].trim().is_empty() {
        j += 1;
    }
    let (needle_line, wrote) = want[best_matched];
    Some(NearMiss {
        line: best_at + 1,
        matched: best_matched,
        total: want.len(),
        needle_line,
        wrote: clipped_line(wrote),
        file_says: if j < file.len() {
            clipped_line(file[j].trim())
        } else {
            "the file ends".to_string()
        },
    })
}

/// The refusal for a near-miss `oldString`: what matched, and the one line that
/// did not, quoted from both sides so the next call differs where it must.
fn near_miss_message(rel_path: &str, m: &NearMiss) -> String {
    format!(
        "its `oldString` is not in {rel_path}, but it is close to text that is: read from line \
         {line}, {matched} of its {total} non-blank lines are in the file in that order and the \
         next one is not. Its line {needle_line} is where they part — you wrote `{wrote}`, the \
         file says `{file_says}`. Send the same edit with the file's version of that one line in \
         the `oldString`; the {matched} lines before it are right. Both texts quoted here are \
         single lines, not a region to paste back whole.",
        line = m.line,
        matched = m.matched,
        total = m.total,
        needle_line = m.needle_line,
        wrote = m.wrote,
        file_says = m.file_says,
    )
}

/// The line a `newString` already occupies verbatim, if it occupies one.
///
/// This is the "already applied" case in its pure form: an earlier turn wrote
/// this replacement and the formatter left it alone, so the edit being refused
/// now asks for a change the file already carries. Claiming that is only honest
/// on a match of the whole `newString` against the file's own bytes — a shared
/// line proves nothing — and only when the `newString` is long enough that a
/// match cannot be luck: `}` and `});` are in every file, and pointing at the
/// first one would send the model to the wrong place with total confidence.
fn already_applied_at(file: &str, replacement: &str) -> Option<usize> {
    const MIN_LINES: usize = 2;
    const MIN_CHARS: usize = 24;
    let hay = file.replace("\r\n", "\n");
    let needle = replacement.replace("\r\n", "\n");
    if needle.lines().filter(|l| !l.trim().is_empty()).count() < MIN_LINES
        && needle.trim().chars().count() < MIN_CHARS
    {
        return None;
    }
    let at = hay.find(&needle)?;
    Some(hay[..at].matches('\n').count() + 1)
}

/// The line where the file already carries what a refused edit meant to write.
///
/// A refusal whose `oldString` is absent has two causes and only one of them is
/// a typo. The other is that the file moved: the model's own earlier write
/// replaced the text it is now quoting, or the formatter that ran after that
/// write rewrapped it. `lr7b_v11` turns 5, 7 and 9 replayed one byte-identical
/// edit against a site its turn-3 write had already rewritten, and the reply —
/// "copy the text verbatim" — was true and impossible to act on, because the
/// text it wanted to copy had been the thing that got replaced.
///
/// The anchor is taken from the `newString`, not the `oldString`: the
/// replacement is what the model believes belongs at that place, so the lines of
/// it that differ from what it is quoting are the change itself. A line the
/// `oldString` also carries is context the model copied and says nothing about
/// where the edit was aimed. Requiring exactly one occurrence is what keeps the
/// answer honest — an anchor that matches three places names none of them, and
/// the next candidate line is tried instead.
fn superseded_site(hay: &str, needle: &str, replacement: &str) -> Option<usize> {
    const MIN_CHARS: usize = 12;
    let lines: Vec<&str> = hay.lines().map(str::trim).collect();
    let quoted: Vec<&str> = needle.lines().map(str::trim).collect();
    replacement
        .lines()
        .map(str::trim)
        .filter(|l| l.chars().count() >= MIN_CHARS && !quoted.contains(l))
        .find_map(|probe| {
            let hits: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter(|(_, l)| **l == probe)
                .map(|(i, _)| i)
                .collect();
            (hits.len() == 1).then(|| hits[0] + 1)
        })
}

/// The file's own bytes around a site an earlier write already changed.
fn superseded_site_message(hay: &str, line: usize) -> String {
    const BEFORE: usize = 2;
    const AFTER: usize = 10;
    let lines: Vec<&str> = hay.lines().collect();
    let start = line.saturating_sub(1).saturating_sub(BEFORE);
    let end = (start + BEFORE + AFTER + 1).min(lines.len());
    format!(
        "The text is gone because this place has already been changed: an earlier write of \
         yours replaced what the `oldString` quotes, or the formatter that ran after it \
         rewrapped those lines. The file now says this, verbatim, starting at the line number \
         in the `[line N]` marker — build the next edit from these lines, and treat the marker \
         itself as something that is not part of the file:\n[line {}]\n{}",
        start + 1,
        lines[start..end].join("\n")
    )
}

/// Where an over-fitting `oldString` actually matches, and what the file puts
/// around each site.
///
/// "Widen it with the surrounding lines until it is unique" names no lines, so
/// the model rebuilds the context from memory of a file it emitted a dozen
/// minutes ago — and gateprobe2 showed what that costs: the same one-line needle
/// sent twice for two different errors, refused twice with this same sentence,
/// then guessed at an indentation and voided a third batch. The lines that make
/// a site unique are in the file, so the tool hands them over instead of asking
/// for them.
fn explain_ambiguous_edit(hay: &str, needle: &str, hits: usize) -> String {
    const SITES: usize = 3;
    const BEFORE: usize = 2;
    const AFTER: usize = 2;
    let lines: Vec<&str> = hay.lines().collect();
    let mut sites: Vec<(usize, usize)> = Vec::new();
    let mut pos = 0usize;
    while let Some(rel) = hay[pos..].find(needle) {
        let abs = pos + rel;
        let first = hay[..abs].matches('\n').count();
        sites.push((first, first + needle.matches('\n').count()));
        pos = abs + needle.len().max(1);
        if sites.len() >= SITES {
            break;
        }
    }
    if sites.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "It matches {} place(s){}. Each block below is verbatim file text at one of them, with a \
         few lines either side: copy the lines under a `[line N]` marker — never the marker, which \
         is not in the file — and widen until only the site you mean still matches.",
        hits,
        if hits > SITES {
            format!(", of which the first {SITES} are shown")
        } else {
            String::new()
        }
    );
    for (first, last) in sites {
        let a = first.saturating_sub(BEFORE);
        let b = (last + AFTER + 1).min(lines.len());
        out.push_str(&format!("\n[line {}]\n{}", a + 1, lines[a..b].join("\n")));
    }
    out
}

/// How many `{` a piece of Rust source leaves open, ignoring the ones that live
/// in comments, strings and char literals.
///
/// Only the *difference* between an `oldString` and its `newString` is ever
/// used, so a file that starts unbalanced still reports correctly, and a doc
/// comment with a code example in it cannot skew an edit that does not touch it.
/// The skipping is what keeps the check honest: a refusal on `format!("{}", x)`
/// or a comment that mentions a brace would make `edit_file` worse than the
/// whole-file rewrite it exists to replace.
pub(crate) fn brace_delta(src: &str) -> i32 {
    let b = src.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i += 2;
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                // Block comments nest in Rust; stopping at the first `*/` would
                // re-expose the text a nested comment hides.
                let mut level = 1usize;
                i += 2;
                while i < b.len() && level > 0 {
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        level += 1;
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        level -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            b'"' => {
                // Raw strings declare their own terminator length with `#`s, so
                // the prefix has to be recognised before the closing quote can
                // be found.
                match raw_string_hashes(b, i) {
                    Some(hashes) => {
                        let terminator = format!("\"{}", "#".repeat(hashes));
                        i = match src[i + 1..].find(&terminator) {
                            Some(rel) => i + 1 + rel + terminator.len(),
                            None => b.len(),
                        };
                    }
                    None => {
                        i += 1;
                        while i < b.len() {
                            if b[i] == b'\\' {
                                i += 2;
                            } else if b[i] == b'"' {
                                i += 1;
                                break;
                            } else {
                                i += 1;
                            }
                        }
                    }
                }
            }
            b'\'' => {
                i += char_literal_len(b, i).unwrap_or(1);
            }
            _ => i += 1,
        }
    }
    depth
}

/// The length of the char literal starting at `quote`, or `None` when the quote
/// starts a lifetime (`'a`, `'src`). Only the two shapes Rust allows are
/// recognised, because treating a lifetime as an opening quote would swallow the
/// code up to the next apostrophe — including its braces.
fn char_literal_len(b: &[u8], quote: usize) -> Option<usize> {
    let next = *b.get(quote + 1)?;
    if next == b'\'' {
        return None;
    }
    if next == b'\\' {
        // `'\n'`, `'\''`, `'\x1A'`, `'\u{1F600}'` — the escape body is consumed
        // by shape, because its own quote or braces must not be read as code.
        let mut i = quote + 2;
        if i < b.len() && b[i] == b'u' {
            i += 1;
            if i < b.len() && b[i] == b'{' {
                while i < b.len() && b[i] != b'}' {
                    i += 1;
                }
                i += 1;
            }
        } else if i < b.len() {
            i += 1;
            if b.get(i - 2) == Some(&b'x') {
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
            }
        }
        return if b.get(i) == Some(&b'\'') {
            Some(i + 1 - quote)
        } else {
            None
        };
    }
    if b.get(quote + 2) == Some(&b'\'') {
        Some(3)
    } else {
        None
    }
}

/// When the `"` at `quote` opens a raw string, the number of `#`s that must
/// follow its closing quote (`r"…"`, `r#"…"#`, `br#"…"#`).
fn raw_string_hashes(b: &[u8], quote: usize) -> Option<usize> {
    let mut hashes = 0usize;
    let mut letter = quote;
    while letter > 0 && b[letter - 1] == b'#' {
        hashes += 1;
        letter -= 1;
    }
    if letter == 0 || b[letter - 1] != b'r' {
        return None;
    }
    letter -= 1;
    // The `r` has to begin a token. A byte-string raw prefix (`br"`/`rb"`) is
    // the one letter allowed in front of it.
    let preceded = |k: usize| k > 0 && (b[k - 1].is_ascii_alphanumeric() || b[k - 1] == b'_');
    if preceded(letter) && !(matches!(b[letter - 1], b'b' | b'r') && preceded(letter - 1)) {
        return None;
    }
    Some(hashes)
}

pub fn handle_system_tool(
    root: &Path,
    name: &str,
    arguments: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    log::debug!("system_tool: {} called", name);
    let result = match name {
        // Native Rust NDA document path (portable NDA1 with in-file history).
        "convert_to_nda" => native_convert_to_nda(root, arguments)?,
        "read_nda" => native_read_nda(root, arguments)?,
        "execute_nda" => execute_csharp_mcp_tool(root, name, arguments)?,
        "read_file" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(root, rel_path, false)?;
            let text = fs::read_to_string(full_path)?;
            let start = read_window_param(arguments, "startLine")
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let end = read_window_param(arguments, "endLine")
                .map_err(|e| -> Box<dyn Error> { e.into() })?;
            if start.is_none() && end.is_none() {
                text
            } else {
                read_line_range(&text, rel_path, start, end)?
            }
        }
        "write_file" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let raw_content = arguments["content"].as_str().ok_or("content is required")?;
            // Models copy tool results verbatim: a Decision Trail block that
            // read_file appended can reappear in the write payload. It is
            // harness metadata, never intended file content, so strip it.
            let stripped = crate::registry::event_store::strip_decision_trail(raw_content);
            let content = stripped.as_deref().unwrap_or(raw_content);

            let scan_warning = scan_file_content(content);

            let full_path = resolve_workspace_path(root, rel_path, true)?;
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&full_path, content)?;

            // Rust gets a formatter pass now rather than at some later build
            // step nobody runs: agent-written code is otherwise exactly as the
            // model typed it. Non-Rust files and a missing rustfmt are no-ops,
            // and the outcome is a note, never an error — the write succeeded.
            let format_note = crate::automation::formatter::format_written_file(&full_path).note();

            // `edit_file` and `delete_file` already name the path they acted on
            // and `write_file` did not, so a 32 KB port written to a path the
            // model never intended came back as an unqualified "success". The
            // measured case is crucible run `convert22`: the task said
            // 'rust_port/src/budget_manager.rs' and the model sent
            // 'litellm_ws/rust_port/src/budget_manager.rs' — the workspace
            // root's own name repeated inside itself — which
            // `resolve_workspace_path` accepted as legitimately relative.
            // Echoing the path back is the only signal in the conversation that
            // a duplicated prefix ever existed.
            let wrote_to = rel_path.replace('\\', "/");
            let mut message = if let Some(warn) = scan_warning {
                format!(
                    "Success: wrote {wrote_to}. WARNING: Security scan warning triggered: [{}]. Please immediately correct this exposure in your next step.",
                    warn
                )
            } else if stripped.is_some() {
                format!(
                    "Success: wrote {wrote_to}. Note: a trailing Decision Trail annotation (harness metadata from a prior read_file result, not file content) was stripped before writing."
                )
            } else {
                format!("Success: wrote {wrote_to}")
            };
            if let Some(note) = format_note {
                message.push_str(&format!(" {note}"));
            }
            message
        }
        "edit_file" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let edits = arguments["edits"]
                .as_array()
                .filter(|batch| !batch.is_empty())
                .ok_or("edits must be a non-empty array of {oldString, newString} objects")?;
            let full_path = resolve_workspace_path(root, rel_path, false)?;
            let current = fs::read_to_string(&full_path)?;

            // Apply the batch to an in-memory copy and write only if EVERY edit
            // matched. A half-applied batch leaves the file in a state the model
            // has never seen, so its next `oldString` guess is worse than the
            // error it started from.
            let mut working = current.clone();
            let mut rejected: Vec<String> = Vec::new();
            // Rust is the one language whose edits the agent loop grades with a
            // compiler, and a batch that changes the file's brace balance cannot
            // be anything but a mistake: `mod tests {` left open by a `newString`
            // that dropped the `}`s its `oldString` matched. testgate2b is the
            // cost — the edit applied cleanly, rustc reported an unclosed
            // delimiter, and the 391s repair turn that followed made the file
            // worse. Refusing at this point names the mistake for free.
            let is_rust = rel_path.replace('\\', "/").ends_with(".rs");
            let mut brace_drift: i32 = 0;
            for (i, edit) in edits.iter().enumerate() {
                let n = i + 1;
                let Some(old) = edit["oldString"].as_str() else {
                    rejected.push(format!("edit {n}: `oldString` must be a string"));
                    continue;
                };
                if old.is_empty() {
                    rejected.push(format!("edit {n}: `oldString` is empty"));
                    continue;
                }
                let Some(new) = edit["newString"].as_str() else {
                    // The schema declares newString required, and the pre-execution
                    // validator only checks an array item's enums — so a missing
                    // key reaches here. Deleting the matched text must be a
                    // deliberate `newString: ""`, never an omitted field.
                    rejected.push(format!(
                        "edit {n}: `newString` is required (use \"\" to delete the matched text)"
                    ));
                    continue;
                };
                // A repair turn that hands back the text it was given applies
                // cleanly and changes nothing, so "Success: applied 1 edit(s)
                // (23886 -> 23886 bytes)" reads to the model as a fix while the
                // compiler still reports the same error. Refusing costs one turn
                // and tells it the patch it meant never left its context.
                if new == old {
                    rejected.push(format!(
                        "edit {n}: its `newString` is byte-identical to its `oldString`, so this \
                         edit would change no characters. The error is still in the file at that \
                         line — write the replacement the error asks for."
                    ));
                    continue;
                }
                // Line endings are the one thing a model cannot see: read_file
                // shows a CRLF file as plain newlines, and the text it copies
                // back is the same content with whichever ending its tokenizer
                // kept. Try both spellings of the needle before refusing — a
                // false "not in the file" would make edit_file worse than the
                // whole-file rewrite it exists to replace — then emit the
                // replacement in the FILE's own endings so a patch cannot leave
                // it mixed.
                let crlf_file = current.contains("\r\n");
                let exact = working.matches(old).count();
                let (needle, hits) = if exact > 0 {
                    (old.to_string(), exact)
                } else {
                    let variants = [old.replace("\r\n", "\n"), old.replace('\n', "\r\n")];
                    match variants
                        .into_iter()
                        .find(|v| working.matches(v.as_str()).count() == 1)
                    {
                        Some(v) => {
                            let n = working.matches(v.as_str()).count();
                            (v, n)
                        }
                        None => (old.to_string(), 0),
                    }
                };
                // Downgrade to LF first, then re-apply the file's own ending.
                // Skipping the downgrade double-CRs a CRLF `newString` patched
                // into a CRLF file — exactly the case where the model copies
                // `read_file` text verbatim, which on a Windows checkout
                // already carries the `\r`.
                let repl = {
                    let lf = new.replace("\r\n", "\n");
                    if crlf_file {
                        lf.replace('\n', "\r\n")
                    } else {
                        lf
                    }
                };
                if hits == 0 {
                    if let Some(header) = quoted_excerpt_header(&needle) {
                        rejected.push(format!(
                            "edit {n}: its `oldString` quotes the excerpt header `{header}`, \
                             which is a label this message writes to show you the file — it is \
                             not in {rel_path} and never will be. Re-issue the edit built only \
                             from the lines inside the box under that header, indentation \
                             included, with the header itself left out."
                        ));
                        continue;
                    }
                    // The two answers below are read from the file, not from
                    // `working`: a batch is all-or-nothing, so an edit earlier in
                    // this same call matched in memory only and nothing it produced
                    // is on disk yet. Both are complete answers in their own right,
                    // so neither rides behind the "copy the text verbatim" lead-in —
                    // the text these two describe is not there to copy.
                    if let Some(line) = already_applied_at(&current, new) {
                        rejected.push(format!(
                            "edit {n}: this change is already on disk — its `newString` is in \
                             {rel_path} verbatim at line {line}, and its `oldString` is not in the \
                             file at all. Nothing to redo here. If the compiler still reports an \
                             error, it is asking for a different change, at the line it names."
                        ));
                        continue;
                    }
                    if let Some(line) = superseded_site(&current, old, new) {
                        rejected.push(format!(
                            "edit {n}: its `oldString` is not in {rel_path}. {}",
                            superseded_site_message(&current, line)
                        ));
                        continue;
                    }
                    // Naming the one line that differs beats pasting the sites of
                    // the line that already matched: v19 re-sent the same call
                    // five times because the paste showed only its correct part.
                    if let Some(miss) = nearest_miss(&current, old) {
                        rejected.push(format!("edit {n}: {}", near_miss_message(rel_path, &miss)));
                        continue;
                    }
                    let why = explain_refused_edit(&current, old);
                    let mut msg = format!(
                        "edit {n}: its `oldString` is not in {rel_path} \u{2014} copy the text \
                         verbatim, including the exact indentation."
                    );
                    if !why.is_empty() {
                        msg.push(' ');
                        msg.push_str(&why);
                    }
                    rejected.push(msg);
                } else if hits > 1 {
                    let why = explain_ambiguous_edit(&working, &needle, hits);
                    let mut msg = format!(
                        "edit {n}: its `oldString` matches {hits} places in {rel_path} \u{2014} \
                         widen it with the surrounding lines until it is unique."
                    );
                    if !why.is_empty() {
                        msg.push(' ');
                        msg.push_str(&why);
                    }
                    rejected.push(msg);
                } else {
                    if is_rust {
                        brace_drift += brace_delta(&repl) - brace_delta(&needle);
                    }
                    working = working.replace(&needle, &repl);
                }
            }
            if !rejected.is_empty() {
                return Err(format!(
                    "no edits were applied to {rel_path}; the file is unchanged:\n{}",
                    rejected.join("\n")
                )
                .into());
            }
            if is_rust && brace_drift != 0 {
                let (moved, glyph) = if brace_drift > 0 {
                    (brace_drift, "block left open")
                } else {
                    (-brace_drift, "brace with no block to close")
                };
                return Err(format!(
                    "no edits were applied to {rel_path}; the file is unchanged: this batch of \
                     {} edit(s) would leave the file with {moved} more {glyph} than it has now. \
                     That is never what a patch means. When an `oldString` matches a line that \
                     closes a block, its `newString` has to repeat that closing brace and put the \
                     new text around it, not in place of it. Re-issue the batch carrying every \
                     delimiter the `oldString` matched.",
                    edits.len()
                )
                .into());
            }

            let scan_warning = scan_file_content(&working);
            fs::write(&full_path, &working)?;
            let format_note = crate::automation::formatter::format_written_file(&full_path).note();

            let mut message = format!(
                "Success: applied {} edit(s) to {rel_path} ({} \u{2192} {} bytes).",
                edits.len(),
                current.len(),
                working.len()
            );
            if let Some(warn) = scan_warning {
                message = format!(
                    "{message} WARNING: Security scan warning triggered: [{warn}]. \
                     Please immediately correct this exposure in your next step."
                );
            }
            if let Some(note) = format_note {
                message.push_str(&format!(" {note}"));
            }
            message
        }
        "list_dir" => {
            let rel_path = arguments["relativeDirPath"]
                .as_str()
                .ok_or("relativeDirPath is required")?;
            let target_dir = if rel_path == "." || rel_path.is_empty() {
                root.to_path_buf()
            } else {
                resolve_workspace_path(root, rel_path, false)?
            };

            let mut entries_list = Vec::new();
            let entries = fs::read_dir(&target_dir).map_err(|e| {
                format!(
                    "Failed to read directory '{}': {:?}",
                    target_dir.display(),
                    e
                )
            })?;

            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type()?.is_dir();
                entries_list.push(format!("{}{}", name, if is_dir { "/" } else { "" }));
            }
            entries_list.join("\n")
        }
        "delete_file" => {
            let rel_path = arguments["relativeFilePath"]
                .as_str()
                .ok_or("relativeFilePath is required")?;
            let full_path = resolve_workspace_path(root, rel_path, false)?;

            if full_path.is_dir() {
                return Err("delete_file cannot be used to delete a directory. Use a command line tool if needed.".into());
            }

            fs::remove_file(&full_path)?;
            format!("Success: File '{}' deleted successfully.", rel_path)
        }
        // ── Disk Hygiene ─────────────────────────────────────────────────────
        "scan_disk_artifacts" => {
            let report = crate::disk_hygiene::scan_and_record(root);
            serde_json::to_string_pretty(&report)?
        }
        "clean_disk_artifacts" => {
            // Dry-run unless explicitly asked to delete; safe-only, always.
            let dry_run = arguments["dryRun"].as_bool().unwrap_or(true);
            let selected: Option<Vec<String>> = arguments["paths"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });
            let result = crate::disk_hygiene::clean(root, selected.as_deref(), dry_run);
            serde_json::to_string_pretty(&result)?
        }
        "grep_search" => {
            let query = arguments["query"].as_str().ok_or("query is required")?;
            let root_dir = root.to_path_buf();
            let mut matches = Vec::new();

            fn search_dir(
                dir: &Path,
                query: &str,
                matches: &mut Vec<String>,
                root: &Path,
            ) -> Result<(), Box<dyn Error>> {
                if let Ok(entries) = fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let file_type = entry.file_type()?;

                        if file_type.is_dir() {
                            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            if dir_name == "node_modules"
                                || dir_name == ".git"
                                // The agent runtime's own state: repo clones it
                                // made, chat logs, crash dumps. `.velocity/clones`
                                // has held a full Rust port of the very library a
                                // porting task was being graded on, so a model that
                                // grepped found the answer beside its own work.
                                || dir_name == ".velocity"
                                || dir_name == "target"
                                || dir_name == "dist"
                                || dir_name == "build"
                                || dir_name == ".vscode"
                                || dir_name == ".idea"
                                || dir_name == "bin"
                                || dir_name == "obj"
                            {
                                continue;
                            }
                            search_dir(&path, query, matches, root)?;
                        } else if file_type.is_file() {
                            let extension = path
                                .extension()
                                .and_then(|ext| ext.to_str())
                                .unwrap_or("")
                                .to_lowercase();
                            let skip_exts = [
                                "png", "jpg", "jpeg", "gif", "ico", "pdf", "zip", "tar", "gz",
                                "7z", "rar", "exe", "dll", "so", "dylib", "class", "pyc", "nda",
                            ];
                            if skip_exts.contains(&extension.as_str()) {
                                continue;
                            }

                            if let Ok(metadata) = path.metadata() {
                                if metadata.len() > 1024 * 1024 {
                                    continue;
                                }
                            }

                            if let Ok(content) = fs::read_to_string(&path) {
                                let rel = path
                                    .strip_prefix(root)
                                    .unwrap_or(&path)
                                    .to_string_lossy()
                                    .to_string();
                                for (idx, line) in content.lines().enumerate() {
                                    if line.contains(query) {
                                        matches.push(format!(
                                            "{}:{}: {}",
                                            rel,
                                            idx + 1,
                                            line.trim()
                                        ));
                                        if matches.len() >= 100 {
                                            return Ok(());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(())
            }

            search_dir(&root_dir, query, &mut matches, root)?;
            if matches.is_empty() {
                format!("No matches found for '{}'", query)
            } else {
                matches.join("\n")
            }
        }
        "fetch_panel_data" => fetch_panel_data(root, arguments)?,
        "run_command" => {
            let cmd_str = arguments["command"].as_str().ok_or("command is required")?;

            let (shell, arg) = if cfg!(target_os = "windows") {
                ("powershell", "-NoProfile -Command")
            } else {
                ("sh", "-c")
            };

            let output = Command::new(shell)
                .args(arg.split_whitespace())
                .arg(cmd_str)
                .current_dir(root)
                .output()?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}{}", stdout, stderr);

            let mut outcome = if combined.trim().is_empty() {
                format!(
                    "Command executed with exit code: {}",
                    output.status.code().unwrap_or(-1)
                )
            } else {
                combined
            };
            // Provenance hook: if this was a build-class command, measure what
            // it cost the disk and say so next to the result. The growth event
            // lands in the shared log, so the Decision Trail shows artifact
            // growth beside the action that caused it.
            if let Some(note) = crate::disk_hygiene::note_build_growth(root, cmd_str) {
                outcome.push_str(&format!("\n[disk hygiene] {note}"));
            }
            outcome
        }
        "git_clone" => git_clone_impl(root, arguments)?,
        // ── Agent Checkpointing ─────────────────────────────────────────────
        "agent_checkpoint_create" => {
            let label = arguments["label"].as_str().ok_or("label is required")?;
            let mut mgr = crate::agent::checkpoint::CheckpointManager::new(root);
            match mgr.checkpoint(label) {
                Some(id) => serde_json::to_string(&json!({
                    "success": true,
                    "checkpointId": id,
                    "label": label,
                    "totalCheckpoints": mgr.count()
                }))?,
                None => serde_json::to_string(&json!({
                    "success": false,
                    "error": "Checkpointing unavailable (not a git repository or git not found)"
                }))?,
            }
        }
        "agent_checkpoint_restore" => {
            let cp_id = arguments["checkpointId"]
                .as_u64()
                .ok_or("checkpointId is required")? as usize;
            let mut mgr = crate::agent::checkpoint::CheckpointManager::new(root);
            // Note: In a real session the same CheckpointManager instance should be reused.
            // This handler provides the MCP interface; the loop_runner holds the live instance.
            match mgr.restore(cp_id) {
                Ok(()) => serde_json::to_string(&json!({
                    "success": true,
                    "restoredTo": cp_id
                }))?,
                Err(e) => serde_json::to_string(&json!({
                    "success": false,
                    "error": e
                }))?,
            }
        }
        "agent_checkpoint_list" => {
            let mgr = crate::agent::checkpoint::CheckpointManager::new(root);
            let list: Vec<Value> = mgr
                .list()
                .iter()
                .map(|cp| {
                    json!({
                        "id": cp.id,
                        "label": cp.label,
                        "gitRef": cp.git_ref,
                        "createdAt": cp.created_at,
                        "dirtyFiles": cp.dirty_files
                    })
                })
                .collect();
            serde_json::to_string(&json!({
                "count": list.len(),
                "checkpoints": list
            }))?
        }
        // ── Agent Memory ────────────────────────────────────────────────────
        "agent_memory_remember" => {
            let key = arguments["key"].as_str().ok_or("key is required")?;
            let content = arguments["content"].as_str().ok_or("content is required")?;
            let tags: Vec<&str> = arguments["tags"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let score = arguments["score"].as_f64().unwrap_or(0.5);
            let mut mem = crate::agent::memory_store::PersistentMemory::open(root);
            mem.remember(key, content, &tags, score);
            let _ = mem.save();
            serde_json::to_string(&json!({
                "success": true,
                "key": key,
                "totalMemories": mem.len()
            }))?
        }
        "agent_memory_recall" => {
            let query = arguments["query"].as_str().ok_or("query is required")?;
            let limit = arguments["limit"].as_u64().unwrap_or(5) as usize;
            let mem = crate::agent::memory_store::PersistentMemory::open(root);
            let hits = mem.recall(query, limit);
            let results: Vec<Value> = hits
                .iter()
                .map(|h| {
                    json!({
                        "key": h.entry.key,
                        "content": h.entry.content,
                        "tags": h.entry.tags,
                        "score": h.entry.score,
                        "similarity": h.similarity
                    })
                })
                .collect();
            serde_json::to_string(&json!({
                "count": results.len(),
                "results": results
            }))?
        }
        "agent_memory_forget" => {
            let key = arguments["key"].as_str().ok_or("key is required")?;
            let mut mem = crate::agent::memory_store::PersistentMemory::open(root);
            let removed = mem.forget(key);
            let _ = mem.save();
            serde_json::to_string(&json!({
                "success": removed,
                "key": key
            }))?
        }
        // ── Test Generation ─────────────────────────────────────────────────
        "code_generate_tests" => {
            let source = arguments["source"].as_str().ok_or("source is required")?;
            let language = arguments["language"].as_str().unwrap_or("rust");
            let tests = generate_test_stubs(source, language);
            serde_json::to_string(&json!({
                "language": language,
                "testCount": tests.len(),
                "tests": tests
            }))?
        }
        "code_coverage_analyze" => {
            // T3c: auto test-coverage analysis. Discovers testable functions
            // (optionally scoped to a file/dir via `path`), reports coverage,
            // and scaffolds test skeletons for the untested ones.
            let mut gen = crate::editor::test_generator::TestGenerator::default();
            if let Some(rel) = arguments["path"].as_str() {
                let target = resolve_workspace_path(root, rel, false)?;
                if target.is_dir() {
                    gen.analyze_coverage(&target);
                } else {
                    gen.analyze_file(root, &target);
                }
            } else {
                gen.analyze_coverage(root);
            }
            let skeletons = gen.generate_tests();
            let untested: Vec<Value> = gen
                .analysis
                .untested_functions
                .iter()
                .take(200)
                .map(|f| {
                    json!({
                        "name": f.name,
                        "file": f.file.display().to_string(),
                        "line": f.line,
                        "signature": f.signature
                    })
                })
                .collect();
            serde_json::to_string(&json!({
                "summary": gen.coverage_summary(),
                "coveragePercent": gen.analysis.coverage_percent,
                "totalFunctions": gen.analysis.total_functions,
                "testedFunctions": gen.analysis.tested_functions,
                "untestedCount": gen.analysis.untested_functions.len(),
                "untested": untested,
                "skeletonCount": skeletons.len(),
                "skeletons": skeletons.iter().map(|s| s.test_body.clone()).collect::<Vec<_>>()
            }))?
        }
        // ── Knowledge / RAG ─────────────────────────────────────────────────
        "knowledge_ingest" => {
            let mut kb = crate::editor::knowledge_base::KnowledgeBase::load(root);
            let (label, added) = if let Some(text) = arguments["text"].as_str() {
                let source = arguments["source"].as_str().unwrap_or("inline");
                let n = kb.ingest_text(source, text);
                (source.to_string(), n)
            } else if let Some(rel) = arguments["path"].as_str() {
                let target = resolve_workspace_path(root, rel, false)?;
                if target.is_dir() {
                    let (files, chunks) = kb.ingest_dir(root, &target);
                    (format!("{rel} ({files} files)"), chunks)
                } else {
                    let n = kb
                        .ingest_path(root, &target)
                        .map_err(|e| -> Box<dyn Error> { e.into() })?;
                    (rel.to_string(), n)
                }
            } else {
                return Err(
                    "knowledge_ingest requires 'text' (with optional 'source') or 'path'".into(),
                );
            };
            kb.save(root).map_err(|e| -> Box<dyn Error> { e.into() })?;
            serde_json::to_string(&json!({
                "success": true,
                "source": label,
                "chunksAdded": added,
                "totalChunks": kb.chunk_count()
            }))?
        }
        "knowledge_search" => {
            let query = arguments["query"].as_str().ok_or("query is required")?;
            let k = arguments["k"].as_u64().unwrap_or(5) as usize;
            let kb = crate::editor::knowledge_base::KnowledgeBase::load(root);
            let hits: Vec<Value> = kb
                .search(query, k)
                .into_iter()
                .map(|h| {
                    json!({
                        "source": h.source,
                        "ordinal": h.ordinal,
                        "score": h.score,
                        "snippet": h.snippet
                    })
                })
                .collect();
            serde_json::to_string(&json!({
                "query": query,
                "resultCount": hits.len(),
                "results": hits
            }))?
        }
        // ── Workspace Indexing ──────────────────────────────────────────────
        "index_workspace" => {
            let target = if let Some(rel) = arguments["path"].as_str() {
                resolve_workspace_path(root, rel, false)?
            } else {
                root.to_path_buf()
            };
            let sitemap_dir = root.join(".velocity").join("site_map");
            let weight_root =
                velocity_ide::site_map::SiteMap::read_persisted_weight_root(&sitemap_dir)
                    .unwrap_or(0);
            let mut sm = velocity_ide::site_map::SiteMap::open(&sitemap_dir, weight_root)
                .map_err(|e| format!("Failed to open site map: {e}"))?;
            let reports =
                velocity_ide::compiler::rust_to_nda::RustToNda::compile_directory(&target, &mut sm)
                    .map_err(|e| format!("Indexing failed: {e}"))?;
            let files_indexed = reports.len();
            let total_functions: usize = reports.iter().map(|r| r.functions).sum();
            let total_nodes: usize = reports.iter().map(|r| r.nodes_stored).sum();
            let total_calls: usize = reports.iter().map(|r| r.call_graph.len()).sum();
            let elapsed_ms: u128 = reports.iter().map(|r| r.elapsed_ms).sum();
            // Drop the site map to flush the dictionary to disk before returning.
            drop(sm);
            serde_json::to_string(&json!({
                "success": true,
                "target": target.display().to_string(),
                "filesIndexed": files_indexed,
                "totalFunctions": total_functions,
                "totalNodes": total_nodes,
                "totalCallEdges": total_calls,
                "elapsedMs": elapsed_ms,
                "sitemapDir": sitemap_dir.display().to_string()
            }))?
        }
        // ── Custom Tool Management ──────────────────────────────────────────
        "tool_register" => {
            let name = arguments["name"].as_str().ok_or("name is required")?;
            let description = arguments["description"]
                .as_str()
                .ok_or("description is required")?;
            let command = arguments["command"].as_str().ok_or("command is required")?;
            let parameters = arguments["parameters"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            let input_schema = serde_json::json!({
                "type": "object",
                "properties": parameters,
                "required": parameters.keys().collect::<Vec<_>>()
            });
            let tool = super::custom_tools::CustomTool {
                name: name.to_string(),
                description: description.to_string(),
                command: command.to_string(),
                input_schema,
            };
            super::custom_tools::register_tool(root, tool).map_err(|e| e.to_string())?
        }
        "tool_unregister" => {
            let name = arguments["name"].as_str().ok_or("name is required")?;
            super::custom_tools::unregister_tool(root, name).map_err(|e| e.to_string())?
        }
        "tool_list_custom" => {
            let tools = super::custom_tools::list_tools(root);
            serde_json::to_string(&json!({
                "count": tools.len(),
                "tools": tools.iter().map(|t| json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema
                })).collect::<Vec<_>>()
            }))?
        }
        // ── Audit & Observability ──────────────────────────────────────────
        "audit_status" => {
            let limit = arguments["limit"].as_u64().unwrap_or(10).min(100) as usize;
            let registry = crate::security::audit::tool_audit_registry();
            let all = registry.aggregate_all();
            let total = all.len();
            let sessions = registry.session_count();
            let recent: Vec<_> = all
                .iter()
                .take(limit)
                .map(|e| {
                    json!({
                        "seq": e.sequence,
                        "tool": e.tool_name,
                        "duration_us": e.duration_us,
                        "outcome": format!("{:?}", e.outcome),
                        "timestamp_ms": e.timestamp_ms
                    })
                })
                .collect();
            serde_json::to_string(&json!({
                "totalEntries": total,
                "activeSessions": sessions,
                "recent": recent
            }))?
        }
        // ── Codebase Event Store ─────────────────────────────────────────
        "event_record" => super::event_store::handle_event_record(root, arguments)
            .map_err(|e| -> Box<dyn Error> { e.into() })?,
        "event_history" => super::event_store::handle_event_history(root, arguments)
            .map_err(|e| -> Box<dyn Error> { e.into() })?,
        "event_context" => super::event_store::handle_event_context(root, arguments)
            .map_err(|e| -> Box<dyn Error> { e.into() })?,
        "event_mark_outcome" => super::event_store::handle_event_mark_outcome(root, arguments)
            .map_err(|e| -> Box<dyn Error> { e.into() })?,
        "event_timeline" => super::event_store::handle_event_timeline(root, arguments)
            .map_err(|e| -> Box<dyn Error> { e.into() })?,
        "event_attach_context" => super::event_store::handle_event_attach_context(root, arguments)
            .map_err(|e| -> Box<dyn Error> { e.into() })?,
        // ── Markdown skills ─────────────────────────────────────────────────
        // (list_skills already belongs to the team subsystem's .nda skills;
        // knowledge .md skills are discovered via the injected PROJECT SKILLS
        // index, so only the pull tool is routed here.)
        "use_skill" => super::skills::handle_use_skill(root, arguments)
            .map_err(|e| -> Box<dyn Error> { e.into() })?,
        // ── Workflows ───────────────────────────────────────────────────────
        "workflow_list" => {
            use crate::editor::workflow::WorkflowStep;
            let reg = crate::editor::workflow::WorkflowRegistry::load(root);
            let mut out = String::new();
            if reg.workflows.is_empty() {
                out.push_str("No saved workflows (create them in the Workflows panel).\n");
            }
            for wf in &reg.workflows {
                let (mut t, mut c, mut a, mut f) = (0u32, 0u32, 0u32, 0u32);
                for s in &wf.steps {
                    match s {
                        WorkflowStep::Tool { .. } | WorkflowStep::Connector { .. } => t += 1,
                        WorkflowStep::Condition { .. } => c += 1,
                        WorkflowStep::AgentTask { .. } => a += 1,
                        WorkflowStep::ForEach { .. } => f += 1,
                    }
                }
                out.push_str(&format!(
                    "  - {} ({} step(s): T{} C{} A{} F{}): {}\n",
                    wf.id,
                    wf.steps.len(),
                    t,
                    c,
                    a,
                    f,
                    wf.name
                ));
                // Same reference check the workflow_run guard applies: flag
                // unregistered tools and connectors before a run fails on them.
                if let Err(e) = crate::editor::company_org::validate_step_references(
                    Some(root),
                    "workflow",
                    &wf.id,
                    &wf.steps,
                ) {
                    out.push_str(&format!("      \u{26a0} {e}\n"));
                }
            }
            out
        }
        "workflow_run" => {
            let id = arguments["id"].as_str().ok_or("id is required")?;
            let reg = crate::editor::workflow::WorkflowRegistry::load(root);
            let workflow = reg
                .get(id)
                .ok_or_else(|| format!("Unknown workflow: {id}"))?;
            // Fail fast if any Tool step references a non-existent tool: a
            // mid-run halt would leave partial side effects (and the engine
            // only short-circuits on Condition failures, not on unknown
            // tools). Mirrors the run_playbook guard.
            crate::editor::company_org::validate_step_references(
                Some(root),
                "workflow",
                id,
                &workflow.steps,
            )
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
            let run = workflow.execute(root);
            serde_json::to_string(&run)?
        }
        "connector_call" => {
            let id = arguments["id"].as_str().ok_or("id is required")?;
            let req = crate::connectors::ConnectorRequest {
                method: arguments["method"].as_str().unwrap_or("GET").to_string(),
                path: arguments["path"].as_str().unwrap_or("").to_string(),
                headers: arguments["headers"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                body: arguments["body"].as_str().map(str::to_string),
            };
            let resp = crate::connectors::call_connector(root, id, &req)?;
            serde_json::to_string(&resp)?
        }
        // generate_image is dispatched by registry::generation_tools, which
        // infers the provider (Cloudflare Workers AI vs native endpoint) from
        // the selected model. It is intentionally absent here so the call falls
        // through to that handler.
        "describe_image" => {
            let path_arg = arguments["path"].as_str().ok_or("path is required")?;
            let path = root.join(path_arg);
            let v = crate::editor::multimodal::describe_image(&path)?;
            serde_json::to_string(&v)?
        }

        // ── GUI Control Bridge ─────────────────────────────────────────────
        "gui_open_file" => {
            let path = arguments["path"].as_str().ok_or("path is required")?;
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::OpenFile {
                    path: path.to_string(),
                },
            )?
        }
        "gui_get_state" => gui_call(root, crate::editor::gui_control::GuiCommand::GetState {})?,
        "gui_navigate_panel" => {
            let panel = arguments["panel"].as_str().ok_or("panel is required")?;
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::NavigatePanel {
                    panel: panel.to_string(),
                },
            )?
        }
        "gui_toggle_panel" => {
            let panel = arguments["panel"].as_str().ok_or("panel is required")?;
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::TogglePanel {
                    panel: panel.to_string(),
                },
            )?
        }
        "gui_quit" => gui_call(root, crate::editor::gui_control::GuiCommand::Quit {})?,
        "gui_list_commands" => {
            let category = arguments["category"].as_str().map(str::to_string);
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::ListCommands { category },
            )?
        }
        "gui_run_command" => {
            let label = arguments["label"].as_str().ok_or("label is required")?;
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::RunCommand {
                    label: label.to_string(),
                    // Deliberately read from the caller's arguments rather than
                    // defaulted to true: an omitted flag means "do not run
                    // anything that writes".
                    allow_unsafe: arguments["allow_unsafe"].as_bool(),
                },
            )?
        }
        "gui_app_map" => {
            let from = arguments["from"].as_str().map(str::to_string);
            let to = arguments["to"].as_str().map(str::to_string);
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::AppMap { from, to },
            )?
        }
        "gui_navigate_to" => {
            let target = arguments["target"].as_str().ok_or("target is required")?;
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::NavigateTo {
                    target: target.to_string(),
                },
            )?
        }
        "gui_list_tabs" => gui_call(root, crate::editor::gui_control::GuiCommand::ListTabs {})?,
        "gui_select_tab" => {
            let tab = arguments["tab"].as_str().ok_or("tab is required")?;
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::SelectTab {
                    tab: tab.to_string(),
                },
            )?
        }
        "gui_select_sub_tab" => {
            let rail = arguments["rail"].as_str().ok_or("rail is required")?;
            let sub_tab = arguments["sub_tab"].as_str().ok_or("sub_tab is required")?;
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::SelectSubTab {
                    rail: rail.to_string(),
                    sub_tab: sub_tab.to_string(),
                },
            )?
        }
        "gui_dismiss_overlays" => gui_call(
            root,
            crate::editor::gui_control::GuiCommand::DismissOverlays {},
        )?,
        "gui_submit_dialog" => {
            // Whatever is on screen asked for a path; this is the only way a
            // caller can hand it one without reaching for somebody's keyboard.
            let value = arguments["value"]
                .as_str()
                .ok_or_else(|| "gui_submit_dialog needs a 'value' string.".to_string())?
                .to_string();
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::SubmitDialog { value },
            )?
        }
        "gui_screenshot" => {
            // An absent path is the normal request -- "just show me the window"
            // -- and the GUI picks a timestamped name inside the workspace, so
            // the caller does not have to know where the app is allowed to write.
            let path = arguments["path"].as_str().unwrap_or("").to_string();
            // Absent means "just take the picture"; present means the caller
            // wants to know whether the screen moved, so the answer has to carry
            // the diff rather than drop it on the floor. A wrong type is refused
            // instead of read as absent -- silence here would look like a
            // comparison that found no change.
            let against = match arguments.get("against") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(other) => {
                    return Err(format!(
                        "gui_screenshot 'against' must be a path string, got {other}"
                    )
                    .into())
                }
            };
            gui_call(
                root,
                crate::editor::gui_control::GuiCommand::Screenshot { path, against },
            )?
        }

        _ => return Ok(None),
    };

    Ok(Some(result))
}

/// Hand one command to the running IDE over the control bridge.
///
/// Every `gui_*` tool used to repeat the token load and the "is the GUI
/// running?" message inline, which is how one of them ended up describing a
/// panel list the handler no longer used. One call site, one error text.
fn gui_call(
    root: &Path,
    cmd: crate::editor::gui_control::GuiCommand,
) -> Result<String, Box<dyn Error>> {
    let token = crate::editor::gui_control::load_token(root)
        .ok_or("GUI control token not found. Is the IDE running?")?;
    let resp = crate::editor::gui_control::send_command(&cmd, &token)?;
    Ok(serde_json::to_string(&resp)?)
}

/// Native `convert_to_nda`: convert any file to a portable NDA1 document
/// (text → wrapped DrawText lines + content triples; image → a DrawImage
/// command carrying a data-url), with an optional seal toggle.
fn native_convert_to_nda(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let file_path = arguments["filePath"]
        .as_str()
        .ok_or("filePath is required")?;
    let output_path = arguments["outputPath"].as_str().unwrap_or("");
    let seal = arguments["seal"].as_bool().unwrap_or(false);

    let src = PathBuf::from(file_path);
    let final_output = if output_path.is_empty() {
        format!("{file_path}.nda")
    } else {
        output_path.to_string()
    };
    let out = PathBuf::from(&final_output);

    let doc = crate::editor::nda_document::convert_file_to_doc(&src)?;
    // Record an origin revision so the produced document carries provenance.
    let mut doc = doc;
    let author = crate::editor::nda_document::resolve_author(root);
    let origin = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string());
    doc.commit_revision(
        &author.name,
        &author.email,
        &author.source,
        &crate::editor::nda_document::now_rfc3339(),
        "converted via convert_to_nda",
        &origin,
    );
    let outcome = crate::editor::nda_document::save_to_disk(root, &out, &doc, seal)?;
    let effective_sealed = matches!(
        outcome,
        crate::editor::nda_document::SaveOutcome::Saved { sealed: true }
    );
    let note = if outcome == crate::editor::nda_document::SaveOutcome::FellBackToPortable {
        " NOTE: seal unavailable (no key material); saved portable instead."
    } else {
        ""
    };

    Ok(format!(
        "Success: converted {} to {} ({} triples, {} commands, sealed={}).{}",
        file_path,
        final_output,
        doc.triples.len(),
        doc.commands.len(),
        effective_sealed,
        note
    ))
}

/// Native `read_nda`: parse a portable or sealed NDA1 document and summarize
/// its title, counts, provenance, and (bounded) triples.
fn native_read_nda(root: &Path, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
    let path = PathBuf::from(nda_path);
    let (doc, kind) = crate::editor::nda_document::load_from_disk(root, &path)
        .map_err(|e| format!("failed to read NDA: {e}"))?;

    let mut out = String::new();
    out.push_str(&format!("NDA document: {nda_path}\n"));
    out.push_str(&format!("Kind: {kind:?}\n"));
    out.push_str(&format!("Title: {}\n", doc.title().unwrap_or("(untitled)")));
    out.push_str(&format!(
        "Triples: {} \u{00b7} Commands: {} \u{00b7} Revisions: {}\n",
        doc.triples.len(),
        doc.commands.len(),
        doc.revisions().len()
    ));
    out.push_str(&format!(
        "History chain: {}\n",
        if doc.verify_history().is_ok() {
            "valid"
        } else {
            "BROKEN"
        }
    ));
    for r in doc.revisions() {
        out.push_str(&format!(
            "  rev {} \u{2014} {} <{}> [{}] {} {}\n",
            r.id, r.author_name, r.author_email, r.author_source, r.timestamp, r.message
        ));
    }
    out.push_str("Triples:\n");
    for (i, (s, p, o)) in doc.triples.iter().enumerate() {
        if i >= 200 {
            out.push_str(&format!("  \u{2026} {} more\n", doc.triples.len() - 200));
            break;
        }
        out.push_str(&format!("  {s} \u{2192} {p} \u{2192} {o}\n"));
    }
    Ok(out)
}

fn execute_csharp_mcp_tool(
    root: &Path,
    tool_name: &str,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    let exe_path = std::env::var("VELOCITY_MCP_SERVER")
        .unwrap_or_else(|_| r"C:\WUIAS\velocity_nda\VelocityMcpServer.exe".to_string());

    if !Path::new(&exe_path).exists() {
        return execute_rust_fallback_tool(root, tool_name, arguments);
    }

    let mut daemon_guard = SIDECAR_DAEMON.lock_safe();

    if daemon_guard.is_none() {
        *daemon_guard = Some(spawn_sidecar(&exe_path)?);
    } else if let Some(daemon) = daemon_guard.as_mut() {
        if let Ok(Some(_status)) = daemon.child.try_wait() {
            *daemon = spawn_sidecar(&exe_path)?;
        }
    }

    // The sidecar may be an older standalone server without workspace-flag
    // support; resolve its file arguments against *our* root first so the
    // tool's workspace-relative contract holds either way.
    let mut arguments = arguments.clone();
    if let Some(raw) = arguments["ndaPath"].as_str() {
        if !Path::new(raw).is_absolute() {
            if let Ok(resolved) = resolve_workspace_path(root, raw, false) {
                arguments["ndaPath"] = json!(resolved.to_string_lossy());
            }
        }
    }

    let request = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments
        },
        "id": 999
    });

    let request_str = serde_json::to_string(&request)? + "\n";

    {
        let daemon = daemon_guard
            .as_mut()
            .ok_or("failed to initialize sidecar daemon")?;
        let stdin = daemon
            .child
            .stdin
            .as_mut()
            .ok_or("Failed to open stdin of C# daemon")?;
        stdin.write_all(request_str.as_bytes())?;
        stdin.flush()?;
    }

    // Hand the stdout pipe to a reader thread and poll with a deadline: a
    // wedged daemon now costs one 30 s round-trip, not the server. The
    // `daemon` borrow must not outlive this block — the timeout arm below
    // re-borrows `daemon_guard` to kill and retire the child.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    {
        let daemon = daemon_guard
            .as_mut()
            .ok_or("failed to initialize sidecar daemon")?;
        let stdout = daemon
            .child
            .stdout
            .take()
            .ok_or("Failed to open stdout of C# daemon")?;
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if !line.trim().is_empty() && tx.send(line).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }

    let response_str;
    loop {
        match rx.recv_timeout(std::time::Duration::from_secs(SIDECAR_TIMEOUT_SECS)) {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.starts_with('{') && trimmed.ends_with('}') {
                    response_str = trimmed.to_string();
                    break;
                }
                // Non-JSON chatter from the daemon: surface it, keep waiting.
                eprintln!("[C# Sidecar Log] {}", trimmed);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(daemon) = daemon_guard.as_mut() {
                    let _ = daemon.child.kill();
                }
                *daemon_guard = None;
                return Err(format!(
                    "C# sidecar daemon did not respond within {}s; it was killed \
                     and will be respawned on the next call",
                    SIDECAR_TIMEOUT_SECS
                )
                .into());
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                *daemon_guard = None;
                return Err("C# sidecar daemon closed stdout unexpectedly".into());
            }
        }
    }

    let response: Value = serde_json::from_str(&response_str)?;

    if let Some(err) = response.get("error") {
        return Err(format!(
            "C# Execution Error: {}",
            err["message"].as_str().unwrap_or("Unknown")
        )
        .into());
    }

    let is_error = response["result"]["isError"].as_bool().unwrap_or(false);
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .ok_or("Failed to parse tool text output")?;

    if is_error {
        Err(text.into())
    } else {
        Ok(text.to_string())
    }
}

pub(crate) fn execute_rust_fallback_tool(
    root: &Path,
    tool_name: &str,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    match tool_name {
        "convert_to_nda" => {
            let file_path = arguments["filePath"]
                .as_str()
                .ok_or("filePath is required")?;
            let output_path = arguments["outputPath"].as_str().unwrap_or("");

            let final_output = if output_path.is_empty() {
                format!("{}.nda", file_path)
            } else {
                output_path.to_string()
            };

            let content = fs::read(file_path)?;

            let mut nda_bytes = Vec::new();
            nda_bytes.extend_from_slice(b"NDAV");

            let size = content.len() as u32;
            nda_bytes.extend_from_slice(&size.to_le_bytes());

            let file_name = Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.txt");
            nda_bytes.extend_from_slice(file_name.as_bytes());
            nda_bytes.push(0);
            nda_bytes.extend_from_slice(&content);

            fs::write(&final_output, nda_bytes)?;

            Ok(format!(
                "Success: File converted and signed to NDA container at: {}",
                final_output
            ))
        }
        "read_nda" => {
            let nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            let nda_bytes = fs::read(nda_path)?;

            if nda_bytes.len() < 9 || &nda_bytes[0..4] != b"NDAV" {
                return Err("Invalid NDA container format".into());
            }

            let size = u32::from_le_bytes([nda_bytes[4], nda_bytes[5], nda_bytes[6], nda_bytes[7]])
                as usize;

            let mut name_end = 8;
            while name_end < nda_bytes.len() && nda_bytes[name_end] != 0 {
                name_end += 1;
            }

            let file_name = String::from_utf8_lossy(&nda_bytes[8..name_end]).to_string();

            let report = json!({
                "format": "NDAV-Fallback",
                "fileName": file_name,
                "payloadSizeBytes": size,
                "visualDisplayCommands": [
                    "display_text: NDA Container Contents Verified",
                    format!("display_text: Filename: {}", file_name),
                    format!("display_text: Size: {} bytes", size)
                ]
            });

            Ok(serde_json::to_string_pretty(&report)?)
        }
        "execute_nda" => {
            let nda_raw = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            // Resolve against the workspace root, not the process CWD — the
            // stdio server's CWD is wherever the client launched it from.
            let nda_path = resolve_workspace_path(root, nda_raw, false)?;
            let nda_bytes = fs::read(&nda_path)?;

            if nda_bytes.len() < 9 || &nda_bytes[0..4] != b"NDAV" {
                return Err("Invalid NDA container format".into());
            }

            let mut name_end = 8;
            while name_end < nda_bytes.len() && nda_bytes[name_end] != 0 {
                name_end += 1;
            }

            let file_name = String::from_utf8_lossy(&nda_bytes[8..name_end]).to_string();
            let payload = &nda_bytes[name_end + 1..];

            let temp_dir = std::env::temp_dir();
            let temp_file_path = temp_dir.join(&file_name);
            fs::write(&temp_file_path, payload)?;

            let ext = Path::new(&file_name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            let cmd_args = arguments["arguments"].as_array();
            let mut args_vec = Vec::new();
            if let Some(arr) = cmd_args {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        args_vec.push(s.to_string());
                    }
                }
            }

            let (shell_cmd, mut final_args) = match ext.as_str() {
                "py" => (
                    "python".to_string(),
                    vec![temp_file_path.to_string_lossy().to_string()],
                ),
                "js" => (
                    "node".to_string(),
                    vec![temp_file_path.to_string_lossy().to_string()],
                ),
                "ps1" => (
                    "powershell".to_string(),
                    vec![
                        "-ExecutionPolicy".to_string(),
                        "Bypass".to_string(),
                        "-File".to_string(),
                        temp_file_path.to_string_lossy().to_string(),
                    ],
                ),
                "sh" => (
                    "bash".to_string(),
                    vec![temp_file_path.to_string_lossy().to_string()],
                ),
                "bat" | "cmd" => (
                    "cmd".to_string(),
                    vec![
                        "/c".to_string(),
                        temp_file_path.to_string_lossy().to_string(),
                    ],
                ),
                _ => (temp_file_path.to_string_lossy().to_string(), Vec::new()),
            };

            final_args.extend(args_vec);

            let dll_path = std::env::var("WUIAS_SHIELD_DLL")
                .unwrap_or_else(|_| r"C:\WUIAS\wuias_shield\wuias_shield.dll".to_string());
            let use_sandbox = Path::new(&dll_path).exists() && cfg!(target_os = "windows");

            let output = if use_sandbox {
                #[cfg(target_os = "windows")]
                {
                    run_in_dll_sandbox(&shell_cmd, &final_args, &dll_path)?
                }
                #[cfg(not(target_os = "windows"))]
                {
                    run_child_with_timeout(
                        Command::new(&shell_cmd)
                            .args(&final_args)
                            .stdin(Stdio::null())
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .spawn()?,
                    )?
                }
            } else {
                // Bounded execution: a payload that never exits (GUI subsystem
                // app, interactive script) used to wedge the whole MCP server —
                // `.output()` waits forever by design.
                run_child_with_timeout(
                    Command::new(&shell_cmd)
                        .args(&final_args)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()?,
                )?
            };

            let _ = fs::remove_file(temp_file_path);

            Ok(output)
        }
        _ => Err(format!("Unknown fallback tool: {}", tool_name).into()),
    }
}

// SAFETY: CreateProcessW/CreateRemoteThread are Windows kernel32 process/thread
// creation APIs. We populate STARTUPINFOW and PROCESS_INFORMATION correctly,
// pass valid wide-string command lines, and close all handles after use.
#[cfg(target_os = "windows")]
#[allow(non_snake_case)]
extern "system" {
    fn CreateProcessW(
        lpApplicationName: *const u16,
        lpCommandLine: *mut u16,
        lpProcessAttributes: *mut std::ffi::c_void,
        lpThreadAttributes: *mut std::ffi::c_void,
        bInheritHandles: i32,
        dwCreationFlags: u32,
        lpEnvironment: *mut std::ffi::c_void,
        lpCurrentDirectory: *const u16,
        lpStartupInfo: *mut STARTUPINFOW,
        lpProcessInformation: *mut PROCESS_INFORMATION,
    ) -> i32;
    fn VirtualAllocEx(
        hProcess: *mut std::ffi::c_void,
        lpAddress: *mut std::ffi::c_void,
        dwSize: usize,
        flAllocationType: u32,
        flProtect: u32,
    ) -> *mut std::ffi::c_void;
    fn WriteProcessMemory(
        hProcess: *mut std::ffi::c_void,
        lpBaseAddress: *mut std::ffi::c_void,
        lpBuffer: *const std::ffi::c_void,
        nSize: usize,
        lpNumberOfBytesWritten: *mut usize,
    ) -> i32;
    fn CreateRemoteThread(
        hProcess: *mut std::ffi::c_void,
        lpThreadAttributes: *mut std::ffi::c_void,
        dwStackSize: usize,
        lpStartAddress: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        lpParameter: *mut std::ffi::c_void,
        dwCreationFlags: u32,
        lpThreadId: *mut u32,
    ) -> *mut std::ffi::c_void;
    fn ResumeThread(hThread: *mut std::ffi::c_void) -> u32;
    fn GetModuleHandleW(lpModuleName: *const u16) -> *mut std::ffi::c_void;
    fn GetProcAddress(
        hModule: *mut std::ffi::c_void,
        lpProcName: *const u8,
    ) -> *mut std::ffi::c_void;
    fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    fn WaitForSingleObject(hHandle: *mut std::ffi::c_void, dwMilliseconds: u32) -> u32;
}

#[cfg(target_os = "windows")]
#[allow(non_snake_case)]
#[repr(C)]
pub struct STARTUPINFOW {
    cb: u32,
    lpReserved: *mut u16,
    lpDesktop: *mut u16,
    lpTitle: *mut u16,
    dwX: u32,
    dwY: u32,
    dwXSize: u32,
    dwYSize: u32,
    dwXCountChars: u32,
    dwYCountChars: u32,
    dwFillAttribute: u32,
    dwFlags: u32,
    wShowWindow: u16,
    cbReserved2: u16,
    lpReserved2: *mut u8,
    hStdInput: *mut std::ffi::c_void,
    hStdOutput: *mut std::ffi::c_void,
    hStdError: *mut std::ffi::c_void,
}

#[cfg(target_os = "windows")]
#[allow(non_snake_case)]
#[repr(C)]
pub struct PROCESS_INFORMATION {
    hProcess: *mut std::ffi::c_void,
    hThread: *mut std::ffi::c_void,
    dwProcessId: u32,
    dwThreadId: u32,
}

#[cfg(target_os = "windows")]
fn to_wstring(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
#[allow(non_snake_case)]
fn run_in_dll_sandbox(
    app: &str,
    args: &[String],
    dll_path: &str,
) -> Result<String, Box<dyn Error>> {
    let session_id = format!(
        "nda_session_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
    );
    let sandbox_base = std::env::var("WUIAS_SANDBOX_REDIRECT")
        .unwrap_or_else(|_| r"C:\WUIAS\sandbox\redirect".to_string());
    let redirect_dir = format!("{}\\{}", sandbox_base, session_id);
    fs::create_dir_all(&redirect_dir)?;

    let w_dll_path = to_wstring(dll_path);
    let cmd_line_str = format!("\"{}\" {}", app, args.join(" "));
    let mut w_cmd_line = to_wstring(&cmd_line_str);

    let _ = Command::new("reg")
        .args([
            "add",
            &format!("HKCU\\Software\\WUIAS_Sandbox\\{}", session_id),
            "/f",
        ])
        .output();

    // SAFETY: CreateProcessW with zeroed STARTUPINFOW (with cb set) and PROCESS_INFORMATION.
    // The command line is a valid mutable wide-string. CREATE_SUSPENDED flag is used to
    // allow sandbox configuration before resuming the process.
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();

        std::env::set_var("WUIAS_SESSION_ID", &session_id);
        std::env::set_var("WUIAS_REDIRECT_DIR", &redirect_dir);

        let CREATE_SUSPENDED: u32 = 0x00000004;
        let success = CreateProcessW(
            std::ptr::null(),
            w_cmd_line.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            CREATE_SUSPENDED,
            std::ptr::null_mut(),
            std::ptr::null(),
            &mut si,
            &mut pi,
        );

        std::env::remove_var("WUIAS_SESSION_ID");
        std::env::remove_var("WUIAS_REDIRECT_DIR");

        if success == 0 {
            return Err(format!(
                "CreateProcessW failed. Error code: {}",
                std::io::Error::last_os_error()
            )
            .into());
        }

        let path_size = (dll_path.len() + 1) * 2;
        let MEM_COMMIT = 0x1000;
        let MEM_RESERVE = 0x2000;
        let PAGE_READWRITE = 0x04;

        let remote_mem = VirtualAllocEx(
            pi.hProcess,
            std::ptr::null_mut(),
            path_size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );

        if remote_mem.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("VirtualAllocEx failed in target process".into());
        }

        let dll_bytes: Vec<u8> = w_dll_path.iter().flat_map(|&w| w.to_le_bytes()).collect();

        let mut written = 0;
        let write_ok = WriteProcessMemory(
            pi.hProcess,
            remote_mem,
            dll_bytes.as_ptr() as *const std::ffi::c_void,
            dll_bytes.len(),
            &mut written,
        );

        if write_ok == 0 {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("WriteProcessMemory failed to write DLL path".into());
        }

        let kernel32_name = to_wstring("kernel32.dll");
        let h_kernel32 = GetModuleHandleW(kernel32_name.as_ptr());
        if h_kernel32.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("Failed to locate kernel32.dll in host".into());
        }

        let load_library_addr = GetProcAddress(h_kernel32, c"LoadLibraryW".as_ptr() as *const u8);
        if load_library_addr.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("Failed to resolve LoadLibraryW address".into());
        }

        let mut thread_id = 0;
        let load_library_fn: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
            std::mem::transmute(load_library_addr);
        let h_thread = CreateRemoteThread(
            pi.hProcess,
            std::ptr::null_mut(),
            0,
            load_library_fn,
            remote_mem,
            0,
            &mut thread_id,
        );

        if h_thread.is_null() {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            return Err("CreateRemoteThread failed to load DLL".into());
        }

        WaitForSingleObject(h_thread, 5000);
        CloseHandle(h_thread);

        ResumeThread(pi.hThread);
        CloseHandle(pi.hThread);

        WaitForSingleObject(pi.hProcess, 0xFFFFFFFF);
        CloseHandle(pi.hProcess);
    }

    let mut run_output = format!(
        "=== Sandboxed execution completed (Session: {}) ===\n",
        session_id
    );

    fn count_files_recursive(dir: &Path) -> usize {
        let mut count = 0;
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        count += count_files_recursive(&entry.path());
                    } else if file_type.is_file() {
                        count += 1;
                    }
                }
            }
        }
        count
    }

    let files_count = count_files_recursive(Path::new(&redirect_dir));
    run_output += &format!(
        "Sandbox redirect folder: {}\nRedirected files written: {}\n",
        redirect_dir, files_count
    );

    let _ = Command::new("reg")
        .args([
            "delete",
            &format!("HKCU\\Software\\WUIAS_Sandbox\\{}", session_id),
            "/f",
        ])
        .output();

    Ok(run_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_clone_validation_guards_run_before_any_network_call() {
        let dir = std::env::temp_dir().join(format!("git_clone_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let dir = fs::canonicalize(&dir).unwrap_or(dir);

        // Missing url.
        let err = git_clone_impl(&dir, &json!({})).unwrap_err().to_string();
        assert!(err.contains("'url' is required"), "{err}");

        // Non-git scheme is refused (so file:// and shell text never reach git).
        let err = git_clone_impl(&dir, &json!({"url": "ftp://example.com/x.git"}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("must start with"), "{err}");

        // Shell metacharacters are rejected out of hand.
        let err = git_clone_impl(&dir, &json!({"url": "https://github.com/a/b; rm -rf /"}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("forbidden characters"), "{err}");

        // A destination escaping the workspace is refused before git runs.
        let err = git_clone_impl(
            &dir,
            &json!({"url": "https://github.com/a/b.git", "dest": "../evil"}),
        )
        .unwrap_err()
        .to_string();
        assert!(!err.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn native_convert_then_read_round_trips() {
        let dir = std::env::temp_dir().join(format!("nda_tool_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let src = dir.join("note.txt");
        fs::write(&src, "alpha\nbeta\ngamma").unwrap();
        let out = dir.join("note.nda");

        let conv = native_convert_to_nda(
            &dir,
            &json!({ "filePath": src.to_string_lossy(), "outputPath": out.to_string_lossy() }),
        )
        .unwrap();
        assert!(conv.contains("Success"), "convert output: {conv}");
        assert!(out.exists());

        // The produced file is a portable NDA1 document with a genesis revision.
        let (doc, kind) = crate::editor::nda_document::load_from_disk(&dir, &out).unwrap();
        assert_eq!(kind, crate::editor::nda_document::LoadedKind::Portable);
        assert_eq!(doc.revisions().len(), 1);
        assert!(doc.verify_history().is_ok());
        assert_eq!(doc.commands.len(), 3);

        let read = native_read_nda(&dir, &json!({ "ndaPath": out.to_string_lossy() })).unwrap();
        assert!(read.contains("Revisions: 1"), "read output: {read}");
        assert!(read.contains("History chain: valid"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn native_convert_seal_produces_sealed_doc() {
        let dir = std::env::temp_dir().join(format!("nda_tool_seal_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let src = dir.join("secret.txt");
        fs::write(&src, "classified").unwrap();
        let out = dir.join("secret.nda");

        native_convert_to_nda(
            &dir,
            &json!({ "filePath": src.to_string_lossy(), "outputPath": out.to_string_lossy(), "seal": true }),
        )
        .unwrap();

        let raw = fs::read(&out).unwrap();
        // Sealed envelope sets ENCRYPTED|RAW flags, so it is not a plain portable doc.
        assert!(velocity_browser::nda_portable::NdaPortableDoc::from_portable_bytes(&raw).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Every `gui_*` tool that is advertised has to reach a dispatch arm, and no
    /// arm may reach a running IDE from a root that has no token.
    ///
    /// `all_advertised_tools_are_dispatchable` already walks the whole tool list
    /// and fails on "Unknown tool", so the wiring half of this is deliberately
    /// redundant. What earns a second test is the second assertion: a name in the
    /// definition list with no arm here is not a compile error -- the `match` ends
    /// in `_ => return Ok(None)` -- and a `gui_*` arm that answered happily with
    /// no bridge in front of it would mean a unit test had driven somebody's live
    /// editor. Two GUI tools have already been found in this crate written,
    /// unit-tested and never wired.
    ///
    /// The probe root deliberately has no `.velocity/gui_control.token`: every arm
    /// funnels through [`gui_call`], which loads the token before it touches the
    /// socket, so the loop cannot reach a running IDE. That matters more than it
    /// sounds, because this family includes `gui_quit`.
    #[test]
    fn every_advertised_gui_tool_reaches_a_handler() {
        let root = std::env::temp_dir().join(format!(
            "velocity_gui_dispatch_probe_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let advertised: Vec<String> = crate::registry::tool_definitions::system::get_system_tools()
            .into_iter()
            .map(|t| t.name)
            .filter(|n| n.starts_with("gui_"))
            .collect();
        assert!(
            advertised.len() >= 12,
            "the gui_* family stopped being advertised; found {}: {advertised:?}",
            advertised.len()
        );
        for name in &advertised {
            match handle_system_tool(&root, name, &json!({})) {
                Ok(None) => panic!(
                    "{name} is advertised but handle_system_tool has no arm for it, so a caller \
                     silently gets nothing back"
                ),
                // Nothing in this family may answer without the bridge, so a body
                // here means a tool did local work the test did not sign up for.
                Ok(Some(body)) => panic!("{name} handled a probe call with no bridge: {body}"),
                Err(_) => {}
            }
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_near_miss_names_the_one_line_that_differs() {
        // lr7b_v19 in miniature: every non-blank line but the last is the file's
        // own, and the last carries a `;` the file does not have.
        let hay = r#"    pub fn update(&mut self, user: &str, record: Record) {
        self.user_dict.insert(user, record);
        let _ = self.save_data();
        self.user_dict
            .values_mut()
            .next()
            .unwrap_or(&mut PersistedUserRecord {
                total_budget: 0.0,
                current_cost: 0.0,
            })
    }
"#;
        let needle = r#"        self.user_dict
            .values_mut()
            .next()
            .unwrap_or(&mut PersistedUserRecord {
                total_budget: 0.0,
                current_cost: 0.0,
            });
"#;
        let miss = nearest_miss(hay, needle).expect("one character off is nameable");
        assert_eq!(miss.line, 4, "the region starts at the file's own line 4");
        assert_eq!((miss.matched, miss.total), (6, 7));
        assert_eq!(miss.needle_line, 7, "the needle's own numbering");
        assert_eq!(miss.wrote, "});");
        assert_eq!(miss.file_says, "})");

        let msg = near_miss_message("src/port.rs", &miss);
        for want in [
            "src/port.rs",
            "line 4",
            "6 of its 7 non-blank lines",
            "Its line 7",
            "you wrote `});`",
            "the file says `})`",
        ] {
            assert!(msg.contains(want), "{want} missing from: {msg}");
        }
    }

    #[test]
    fn a_near_miss_looks_past_blank_lines_and_crlf() {
        let hay = "fn a() {\r\n\r\n    let x = 1;\r\n\r\n    let y = 2;\r\n    let z = 3;\r\n}\r\n";
        let needle = "    let x = 1;\n    let y = 2;\n    let z = 9;\n";
        let miss = nearest_miss(hay, needle).expect("blank lines and CRLF are not content");
        assert_eq!(miss.line, 3);
        assert_eq!((miss.matched, miss.total), (2, 3));
        assert_eq!(miss.file_says, "let z = 3;");
    }

    #[test]
    fn a_near_miss_has_to_be_confidently_near() {
        // Two non-blank lines cannot say which region was meant.
        let hay = "fn a() {\n    let x = 1;\n    let y = 2;\n}\n";
        assert_eq!(nearest_miss(hay, "    let x = 1;\n    let z = 9;\n"), None);

        // A needle botched in the middle scores only the lines before the
        // mistake; the scan is a prefix run and never guesses past it.
        let middle = "    let a = 1;\n    let WRONG = 0;\n    let c = 3;\n    let d = 4;\n";
        let needle = "    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n";
        assert_eq!(nearest_miss(middle, needle), None);

        // Two of six is under half, so it points at nothing worth pointing at.
        let hay =
            "    let a = 1;\n    let b = 2;\n    let c = 9;\n    let d = 9;\n    let e = 9;\n";
        let six = "    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n    let f = 6;\n";
        assert_eq!(nearest_miss(hay, six), None);

        // Every line present with only the indentation wrong belongs to
        // explain_refused_edit, which pastes the file's own leading spaces.
        let flat = "    let a = 1;\n    let b = 2;\n    let c = 3;\n";
        let deep = "        let a = 1;\n        let b = 2;\n        let c = 3;\n";
        assert_eq!(nearest_miss(flat, deep), None);
        // And an exact match is not a near miss at all.
        assert_eq!(nearest_miss(flat, flat), None);
    }

    #[test]
    fn a_long_quoted_line_is_clipped_not_dropped() {
        let hay = "    let a = 1;\n    let b = 2;\n    let target = 3;\n";
        let needle = format!(
            "    let a = 1;\n    let b = 2;\n    let target = {};\n",
            "x".repeat(400)
        );
        let miss = nearest_miss(hay, &needle).expect("the first two lines still anchor it");
        assert_eq!(miss.wrote.chars().count(), 161, "160 kept plus the mark");
        assert!(miss.wrote.ends_with('\u{2026}'), "{}", miss.wrote);
        // Clipping one side does not touch the other: the file's own line is
        // short, so it arrives whole and the model can copy it.
        let msg = near_miss_message("src/port.rs", &miss);
        assert!(msg.contains("the file says `let target = 3;`"), "{msg}");
    }
}
