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
    /// Whether the file ran out at the divergence, so the quoted region is not
    /// present with one line altered but stops short there.
    at_eof: bool,
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
        at_eof: j >= file.len(),
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

impl NearMiss {
    /// Whether this near miss locates the site the `oldString` was aimed at,
    /// rather than happening on a prefix of it somewhere else in the file.
    ///
    /// The scan is a prefix run, so `matched` counts the lines before the
    /// divergence and says nothing at all about the ones after it. A needle that
    /// parts company on its second line and runs on for twenty more is a
    /// two-line coincidence; one that parts on its second-to-last line has every
    /// other line of it standing in the file at a single place. Only the second
    /// contradicts the claim that the site was rewritten away, which is what
    /// [`superseded_site`] asserts from one line of the `newString`, so only the
    /// second one gets to displace it.
    ///
    /// `lr7b_v24` needed the distinction. Turns 3 and 4 quoted a twelve-line
    /// chain whose only fault was `});` where the file says `})`, and both were
    /// answered with that line quoted from each side — the whole of the fix. Turn
    /// 5 sent the same twelve lines with a different replacement, so
    /// `superseded_site` found a unique anchor in it, and the refusal became "an
    /// earlier write of yours replaced what the `oldString` quotes" over a paste
    /// of a function forty lines above the site. Nothing had replaced anything.
    /// Turn 6 then quoted the tail of turn 5's refused `newString`, text the file
    /// had never held, and the run spent both turns it had left on a phantom.
    ///
    /// A divergence at end of file is excluded: there the region does not stand
    /// with one line altered, it stops short, and a write that shortened the file
    /// is exactly the history `superseded_site` tells.
    fn locates_the_site(&self) -> bool {
        /// Lines of the needle the divergence may leave unexamined and still be
        /// the site's own: the line that differs, and one after it.
        const TAIL: usize = 1;
        /// A prefix this short turns up anywhere by accident.
        const MIN_MATCHED: usize = 3;
        !self.at_eof
            && self.matched >= MIN_MATCHED
            && self.total.saturating_sub(self.matched + 1) <= TAIL
    }
}

/// A refused `oldString` that quotes two runs of the file and steps over the
/// lines between them.
#[derive(Debug, PartialEq)]
struct SkippedBlock {
    /// 1-based file line the head the needle matched starts on.
    head_from: usize,
    /// 1-based file line that head ends on.
    head_to: usize,
    /// How many of the needle's non-blank lines that head is.
    head: usize,
    /// How many non-blank lines the needle has.
    total: usize,
    /// The needle's own 1-based line where the head ran out.
    needle_line: usize,
    /// What the needle wrote there, which is not what the file has there.
    wrote: String,
    /// 1-based file line the needle's remaining lines resume on.
    resumes_at: usize,
    /// 1-based file line they end on, so the span to copy has a floor and a
    /// ceiling the model can hand to `read_file`.
    tail_to: usize,
    /// The file's own lines in between, numbered and quoted as they stand.
    omitted: Vec<(usize, String)>,
}

/// Where a refused `oldString` steps over the file, and what it steps over.
///
/// [`nearest_miss`] is a prefix run, so it reports the first line that does not
/// match and stops looking. That is the right answer when the needle quotes one
/// region and gets one line of it wrong, and it is the wrong answer when the
/// needle quotes *two* regions and leaves the lines between them out — because
/// then the line it names is not a mistake, it is the needle's own tail standing
/// further down the file, and the prescription that follows from reading it as a
/// mistake ("send the same edit with the file's version of that one line") builds
/// an `oldString` covering the head plus one line of the block that was skipped,
/// against a `newString` written for the whole span. The splice would delete the
/// rest of the block.
///
/// `lr7b_v32` died on exactly that. Turns 5, 6, 8 and 10 sent one byte-identical
/// `edit_file`: an `oldString` of twelve lines that were the file's 288 to 298
/// and then its 303, leaving out 299 to 302, with a `newString` that wrote all
/// sixteen back. Each attempt was answered "11 of its 12 non-blank lines are in
/// the file in that order and the next one is not. Its line 12 is where they part
/// — you wrote `}`, the file says `self.user_dict`", which is a true sentence
/// about a false shape, and the run re-sent the same call four times rather than
/// act on it. It ended at 3/3 compile-repair turns and 0 refunds with the E0499
/// it started with.
///
/// Three things have to be proved before this says anything, because a hole is
/// an inference about intent and the file alone does not carry it:
///
/// - the head has to match at exactly one place. Two places is no site, and the
///   span named below would be a guess dressed as a line number.
/// - the tail has to resume at exactly one place that leaves a block of at most
///   `MAX_OMITTED` lines between. A one-line tail like `}` stands on hundreds
///   of lines, so "the first one after the break" is not a proof; the block has
///   to be short enough that the two runs are still one region.
/// - the `newString` has to carry the skipped lines, in order and contiguously,
///   right through to the end of the tail. That is what picks the resumption out
///   of the many a short tail could have, and it is also the whole of the claim
///   the message makes: the replacement already covers the block, so widening the
///   `oldString` to match it is the only thing this edit is missing.
///
/// This refuses rather than applies. Widening a span is not the same as
/// re-indenting one or joining what a formatter broke: the needle here also says
/// nothing about whether the lines *above* the head belong in the span, and
/// `lr7b_v32`'s did not — its `newString` opened with a `let` the file already
/// had on the line its `oldString` started below, so writing it would have
/// declared that binding twice. Naming the hole is the most this can prove.
fn skipped_block(hay: &str, needle: &str, patch: &str) -> Option<SkippedBlock> {
    /// File lines a needle may step over and still be one region with a hole in
    /// it rather than two regions quoted by accident.
    const MAX_OMITTED: usize = 12;
    /// Below this there is no head *and* a tail to resume.
    const MIN_TOTAL: usize = 3;
    const MAX_LINES: usize = 60;
    let raw: Vec<&str> = hay.lines().collect();
    // Blank lines carry no text to match, so dropping them up front makes every
    // run below contiguous in one index space — the same reading `nearest_miss`
    // and `run_start` give, without their inner skip loops.
    let file: Vec<(usize, &str)> = raw
        .iter()
        .enumerate()
        .map(|(i, l)| (i, l.trim()))
        .filter(|(_, l)| !l.is_empty())
        .collect();
    let want: Vec<(usize, &str)> = needle
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty())
        .take(MAX_LINES)
        .collect();
    if want.len() < MIN_TOTAL {
        return None;
    }
    let have: Vec<&str> = patch
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    // The longest run of the needle's own leading lines that the file carries,
    // and every place it carries it. A head of zero is declined by the majority
    // floor below whatever `sites` says, so the run it records needs no floor of
    // its own here.
    let mut head = 0usize;
    let mut sites: Vec<usize> = Vec::new();
    for p in 0..file.len() {
        let mut k = 0;
        while k < want.len() && p + k < file.len() && file[p + k].1 == want[k].1 {
            k += 1;
        }
        if k > head {
            head = k;
            sites.clear();
        }
        if k == head {
            sites.push(p);
        }
    }
    // A head that is the whole needle is a match, not a hole, and one that is
    // less than half of it is a coincidence the tail says nothing about. That
    // floor is also what keeps the head at two lines or more, since the needle
    // has three at the very least — and, with the head a run of the file itself,
    // what keeps the tail no longer than the file has lines left to search.
    if head >= want.len() || head * 2 < want.len() || sites.len() != 1 {
        return None;
    }
    let p = sites[0];
    let tail: Vec<&str> = want[head..].iter().map(|&(_, l)| l).collect();
    let mut hits: Vec<usize> = Vec::new();
    for q in (p + head + 1)..=file.len() - tail.len() {
        if file[q..q + tail.len()]
            .iter()
            .map(|&(_, l)| l)
            .ne(tail.iter().copied())
        {
            continue;
        }
        let omitted = &file[p + head..q];
        if omitted.len() > MAX_OMITTED {
            break;
        }
        // The witness: the replacement carries the skipped lines and the tail
        // after them as one unbroken run, which is what makes this span the one
        // the edit means rather than a place the tail happens to stand.
        let needed: Vec<&str> = omitted
            .iter()
            .map(|&(_, l)| l)
            .chain(tail.iter().copied())
            .collect();
        if !have.windows(needed.len()).any(|w| w == needed.as_slice()) {
            continue;
        }
        hits.push(q);
        if hits.len() > 1 {
            return None;
        }
    }
    let q = *hits.first()?;
    let omitted = &file[p + head..q];
    Some(SkippedBlock {
        head_from: file[p].0 + 1,
        head_to: file[p + head - 1].0 + 1,
        head,
        total: want.len(),
        needle_line: want[head].0,
        wrote: clipped_line(want[head].1),
        resumes_at: file[q].0 + 1,
        tail_to: file[q + tail.len() - 1].0 + 1,
        omitted: omitted
            .iter()
            .map(|&(i, _)| (i + 1, clipped_line(raw[i])))
            .collect(),
    })
}

/// The refusal for an `oldString` with a hole in it: both runs it did quote, by
/// line, and the file's own lines between them, which are the part it left out
/// and the part its `newString` already writes back.
///
/// The lines are quoted with their own indentation and their own numbers rather
/// than trimmed, because the model has to put them into an `oldString` that will
/// be matched byte for byte — the one thing it cannot do is count leading
/// whitespace, which is why [`indent_rescue`] exists.
fn skipped_block_message(rel_path: &str, s: &SkippedBlock) -> String {
    let quoted = s
        .omitted
        .iter()
        .map(|(line, text)| format!("    {line}| {text}"))
        .collect::<Vec<_>>()
        .join("\n");
    let n = s.omitted.len();
    format!(
        "its `oldString` is not in {rel_path}, and it is not one line wrong either — it has a \
         hole in it. {head} of its {total} non-blank lines are the file's own, lines {from} to \
         {to}; its line {needle_line}, `{wrote}`, is not what the file has next but the file's \
         line {resumes}. Between the two the file carries {n} line(s) the `oldString` leaves \
         out:\n{quoted}\nIts `newString` writes those {n} line(s) back and the line after them, \
         which is how the span they sit in is the one this edit means. So widen the `oldString` \
         to cover it: `read_file` lines {from} to {tail_to} and copy that whole run in, the {n} \
         line(s) above included, and send the same `newString`. Do not swap your line \
         {needle_line} for the file's line {first_omitted} — nothing here is a one-line \
         difference, and that would leave the hole where it is.",
        head = s.head,
        total = s.total,
        from = s.head_from,
        to = s.head_to,
        needle_line = s.needle_line,
        wrote = s.wrote,
        resumes = s.resumes_at,
        tail_to = s.tail_to,
        first_omitted = s.omitted[0].0,
    )
}

/// A refused `oldString` whose head is absent from the file but whose tail is
/// present, complete, and in exactly one place.
#[derive(Debug, PartialEq)]
struct TailAnchor {
    /// 1-based file line the needle's first present line sits on.
    line: usize,
    /// How many of the needle's non-blank lines were absent before it.
    skipped: usize,
    /// How many non-blank lines matched from there: the whole of the rest.
    matched: usize,
    /// How many non-blank lines the needle has.
    total: usize,
    /// What the model wrote on its last absent line.
    wrote: String,
    /// The file's own lines immediately above the match, numbered, which is where
    /// the model's absent head belongs.
    file_says: Vec<(usize, String)>,
}

/// The 1-based file line the run of `tail` starts at, scanning forward from the
/// 0-based index `start` with blank lines skipped, or `None` if the run breaks
/// or the file runs out.
fn run_start(file: &[&str], start: usize, tail: &[&str]) -> Option<usize> {
    let mut at = None;
    let mut j = start;
    for want in tail {
        loop {
            if j >= file.len() {
                return None;
            }
            let line = file[j].trim();
            j += 1;
            if line.is_empty() {
                continue;
            }
            if line != *want {
                return None;
            }
            at.get_or_insert(j);
            break;
        }
    }
    at
}

/// Where the tail of a refused `needle` sits, when its head is not in the file.
///
/// [`nearest_miss`] is a prefix run, so a needle whose *first* line is wrong
/// scores zero and falls through to [`explain_refused_edit`] however exactly the
/// rest of it stands in the file. `lr7b_v26` lost turns 5, 11 and 12 to that. The
/// model quoted fourteen lines whose only fault was the first: it wrote
/// `let result_record = self.user_dict` on one line where rustfmt had broken the
/// file's into `let result_record =` and `self.user_dict`, and the other thirteen
/// were the file's own 289 to 302 byte for byte. The refusal answered that the
/// text was "wrong rather than merely mis-indented" and told the model to
/// `read_file` "that region" without naming one, so it read 289-304 three times —
/// a window opening below the only two lines it was missing — and re-sent the same
/// guess each time.
///
/// That exact needle is no longer refused: a head whose only fault is where the
/// formatter broke it is the file's own code, so [`chain_join_rescue`] applies the
/// edit and says so. What reaches here is a head that is wrong *text* — a renamed
/// field, a dropped argument — where applying the edit would write a line the file
/// has never held, so the answer has to be the line the tail starts at and the
/// head above it to copy from.
///
/// The tail must match complete and unique, and the head must be short and text
/// the file has never held. A partial tail is a coincidence, an ambiguous one
/// names nothing, a head that is somewhere in the file already has a home, and a
/// long absent head means the model was quoting a different region than the one
/// it landed in.
fn tail_anchor(hay: &str, needle: &str) -> Option<TailAnchor> {
    /// Absent head lines still worth looking past.
    const MAX_SKIPPED: usize = 2;
    /// A tail this short turns up anywhere by accident.
    const MIN_TAIL: usize = 3;
    const MAX_LINES: usize = 60;
    let hay = hay.replace("\r\n", "\n");
    let needle = needle.replace("\r\n", "\n");
    let file: Vec<&str> = hay.lines().collect();
    let want: Vec<&str> = needle
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(MAX_LINES)
        .collect();
    if want.len() < MIN_TAIL + 1 {
        return None;
    }
    // The head has to be text the file has never held. A needle whose own first
    // line is standing somewhere in the file was aimed at that place, and a tail
    // it happens to share with a second region says nothing about where its head
    // belongs — the lines above that second site would be quoted as the home of a
    // head that has one already, sending the model to a region it never quoted.
    if file.iter().any(|l| l.trim() == want[0]) {
        return None;
    }
    for skipped in 1..=MAX_SKIPPED.min(want.len() - MIN_TAIL) {
        let tail = &want[skipped..];
        let mut sites: Vec<usize> = Vec::new();
        for i in 0..file.len() {
            if let Some(at) = run_start(&file, i, tail) {
                if !sites.contains(&at) {
                    sites.push(at);
                }
            }
        }
        if sites.len() != 1 {
            continue;
        }
        let first_matched = sites[0];
        // Walk up from the line directly above the match — never the match's own
        // first line — and keep one more line than were absent, so a head the
        // formatter split across two lines is still shown whole.
        let mut above: Vec<(usize, String)> = Vec::new();
        let mut index = first_matched - 1;
        while above.len() < skipped + 1 {
            if index == 0 {
                break;
            }
            index -= 1;
            let line = file[index].trim();
            if !line.is_empty() {
                above.push((index + 1, clipped_line(line)));
            }
        }
        above.reverse();
        // A unique site at the very top of the file leaves no head room, and the
        // next `skipped` would name a line as absent that the file plainly has —
        // the line above the match, which is file line 1. A unique site settles
        // how much of the head is missing, so this one is the answer or there is
        // none.
        if above.is_empty() {
            return None;
        }
        return Some(TailAnchor {
            line: first_matched,
            skipped,
            matched: tail.len(),
            total: want.len(),
            wrote: clipped_line(want[skipped - 1]),
            file_says: above,
        });
    }
    None
}

/// The refusal for a tail-anchored `oldString`: the head is the part that is not
/// in the file, the tail is the file's own, and here is the line to read the head
/// from. Naming the line is the whole of it — "read that region" is the advice
/// `lr7b_v26` obeyed three times into a window that missed the divergence.
fn tail_anchor_message(rel_path: &str, a: &TailAnchor) -> String {
    let quoted = a
        .file_says
        .iter()
        .map(|(line, text)| format!("    {line}| {text}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "its `oldString` is not in {rel_path}, but everything after its first {skipped} line(s) \
         is: {matched} of its {total} non-blank lines are in the file in that order, at line \
         {line}, and nowhere else. Your line {skipped}, `{wrote}`, is the last one that is not \
         there. The file has this where yours has those:\n{quoted}\nRead from line {read_from} \
         and copy that head from what it returns; the {matched} lines after it are already right.",
        read_from = a.file_says[0].0,
        skipped = a.skipped,
        matched = a.matched,
        total = a.total,
        line = a.line,
        wrote = a.wrote,
    )
}

/// A refused `oldString` that differs from exactly one place in the file by
/// nothing but a uniform re-indent.
#[derive(Debug, PartialEq)]
struct IndentRescue {
    /// Byte span the matched lines occupy in the haystack, terminators excluded.
    start: usize,
    end: usize,
    /// 1-based file line the match starts at.
    line: usize,
    /// The one indentation adjustment every non-blank matched line shares.
    extra: String,
    /// Whether the file is deeper than the needle, so `extra` is added rather
    /// than removed.
    deepen: bool,
}

/// Byte offset of every line of `hay`, one entry per `split('\n')` segment, so
/// a line's terminator length never has to be guessed.
fn line_starts(hay: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut pos = 0usize;
    for segment in hay.split('\n') {
        starts.push(pos);
        pos += segment.len() + 1;
    }
    starts
}

/// Line `k` of `hay` without its terminator, by byte offset rather than by
/// re-splitting, so a rescued edit can be spliced back into the original bytes
/// of a CRLF file.
fn line_at<'a>(hay: &'a str, starts: &[usize], k: usize) -> &'a str {
    let s = starts[k];
    let e = if k + 1 < starts.len() {
        starts[k + 1]
    } else {
        hay.len()
    };
    hay[s..e].trim_end_matches(['\r', '\n'])
}

/// The one place a needle's lines occur ignoring leading whitespace, when there
/// is exactly one and the difference is a uniform re-indent.
///
/// `lr7b_v20` spent ten of its twenty-three turns, and the run, on this. The
/// model's `oldString` was `        user_record: result_record,\n    }` — eight
/// and four spaces — against a file that says twelve and eight, with a
/// `newString` that was the correct E0308 fix. The refusal already knew the
/// match was unique ("IS in 1 place(s) of the file under different leading
/// whitespace"), pasted the file's own bytes under a `[line 358]` marker, and
/// the `read_file` the advice sent it to returned the same text again. The
/// model re-emitted eight spaces every single time. A 7B cannot count leading
/// whitespace: it re-indents to the nesting level it expects, and no wording of
/// a refusal can teach it otherwise, so the tenth replay was as doomed as the
/// second.
///
/// Uniqueness is what makes applying this honest rather than a guess — one
/// candidate site is the site the model meant — and the uniform-delta rule is
/// what keeps it honest about the replacement. Blank lines are skipped because
/// they carry no indentation to compare; a needle whose lines differ by
/// *different* amounts is a model that restructured the block, and re-indenting
/// that would be inventing an edit, so it declines and falls through to the
/// sites paste.
fn indent_rescue(hay: &str, needle: &str) -> Option<IndentRescue> {
    // `}` and `});` are in every file. Uniqueness already rules out the usual
    // ones, and a residue this short carries no evidence of intent.
    const MIN_TRIMMED: usize = 8;
    let want: Vec<&str> = needle.lines().collect();
    let want_trim: Vec<&str> = want.iter().map(|l| l.trim()).collect();
    if want_trim.iter().all(|l| l.is_empty()) {
        return None;
    }
    if want_trim.iter().map(|l| l.chars().count()).sum::<usize>() < MIN_TRIMMED {
        return None;
    }
    let starts = line_starts(hay);
    let count = hay.lines().count();
    let n = want.len();
    if n == 0 || n > count {
        return None;
    }
    let mut hits: Vec<usize> = Vec::new();
    for i in 0..=(count - n) {
        if (0..n).all(|k| line_at(hay, &starts, i + k).trim() == want_trim[k]) {
            hits.push(i);
            if hits.len() > 1 {
                return None;
            }
        }
    }
    let i = *hits.first()?;

    let mut extra: Option<(String, bool)> = None;
    for k in 0..n {
        if want_trim[k].is_empty() {
            continue;
        }
        let nk = crate::editor::auto_indent::leading_whitespace(want[k]);
        let fk = crate::editor::auto_indent::leading_whitespace(line_at(hay, &starts, i + k));
        let found = if fk.starts_with(nk) {
            (fk[nk.len()..].to_string(), true)
        } else if nk.starts_with(fk) {
            (nk[fk.len()..].to_string(), false)
        } else {
            // Tabs against spaces, or an indent that is not a prefix of the
            // other: not a re-indent of the same block.
            return None;
        };
        match &extra {
            None => extra = Some(found),
            Some(prev) if *prev == found => {}
            Some(_) => return None,
        }
    }
    let (extra, deepen) = extra.unwrap_or_default();
    let tail_start = if i + n < starts.len() {
        starts[i + n]
    } else {
        hay.len()
    };
    let matched = &hay[starts[i]..tail_start];
    Some(IndentRescue {
        start: starts[i],
        end: starts[i] + matched.trim_end_matches(['\r', '\n']).len(),
        line: i + 1,
        extra,
        deepen,
    })
}

/// A replacement re-indented by the one adjustment the file's copy of the
/// needle carries, so the rescued edit lands at the nesting level the file
/// actually uses instead of the one the model guessed.
fn reindent(new_lf: &str, extra: &str, deepen: bool) -> String {
    let strip = extra.chars().count();
    let out: Vec<String> = new_lf
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                return String::new();
            }
            if deepen {
                return format!("{extra}{line}");
            }
            let ws = line.chars().take_while(|c| c.is_whitespace()).count();
            let keep = ws.saturating_sub(strip);
            let head: String = line.chars().take(keep).collect();
            let tail: String = line.chars().skip(ws).collect();
            format!("{head}{tail}")
        })
        .collect();
    let mut joined = out.join("\n");
    if new_lf.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// A refused `oldString` that names exactly one place in the file once the
/// file's chain-continuation lines are joined back onto the lines they
/// continue.
#[derive(Debug, PartialEq)]
struct ChainJoin {
    /// Byte span the matched lines occupy in the unjoined file, terminators
    /// excluded — the bytes a rescued edit replaces.
    start: usize,
    end: usize,
    /// 1-based line of the unjoined file the match starts at.
    line: usize,
    /// Indentation adjustment taken from the first matched line alone, which is
    /// the only one whose shift means anything: the lines under it were joined
    /// out of deeper nesting, so their shift is an artefact of the formatter.
    extra: String,
    deepen: bool,
    /// Unjoined file lines the match covers, always more than the needle has.
    file_lines: usize,
}

/// A refused `oldString` that names exactly one place in the file once its chain
/// breaks are undone, and differs from it by nothing but a statement terminator.
#[derive(Debug, PartialEq)]
struct TerminatorMiss {
    /// 1-based line of the unjoined file the site starts at.
    line: usize,
    /// Unjoined file lines the site covers.
    file_lines: usize,
    /// Raw lines the `oldString` covers, so the refusal can say what the fold
    /// undid on that side too.
    needle_lines: usize,
    /// Whether the file's own last line carries the terminator the needle lacks.
    file_has_the_terminator: bool,
    /// The last line of each side, trimmed: the file's own, and what was sent.
    file_tail: String,
    wrote_tail: String,
}

/// `hay` with every line that continues the one above it joined onto it, plus the
/// byte span each joined line covers in the original.
///
/// Three shapes continue, and each is licensed by Rust's grammar rather than by
/// appearance. A line whose first non-whitespace character is `.` cannot begin a
/// statement, so it belongs to the one before it. A line whose last non-whitespace
/// character is `=` cannot end one, so the line after it is its right-hand side —
/// rustfmt breaks there whenever the whole binding will not fit, and that is the
/// break both `lr7b_v26` and `lr7b_v27` reproduced wrong: the model wrote
/// `let result_record = self.user_dict…` back on one line where the file had two.
/// And a line that leaves a `(` or `[` unclosed cannot end one either, so
/// everything down to the closer is still that one expression — which is the break
/// `lr7b_v28` reproduced wrong, quoting a twelve-line binding and the six struct
/// fields inside it as a single line.
///
/// Joining changes the line breaks and nothing else, which is what makes the
/// joined text safe to match against, and the spans are what make the match
/// reversible: they say which original bytes a joined line stands for, so a splice
/// can replace them rather than the joined copy. A blank line above a continuation
/// ends the chain rather than joining through it, and a comment neither opens nor
/// joins one: its text is not grammar, so a bracket at the end of it is inside the
/// comment, and joining the line below a comment onto it would put that line
/// inside the comment too.
fn dechain(hay: &str) -> (String, Vec<(usize, usize)>) {
    /// Lines one run may cover. A bracket the scanner cannot see — one under a
    /// `/* */` comment that spans lines — would otherwise leave the depth open
    /// for the rest of the file and fold all of it onto one line.
    const MAX_RUN: usize = 60;
    let starts = line_starts(hay);
    let mut text = String::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    // A continuation needs something above it to continue, so a blank line above
    // one means there is nothing to join onto rather than that the blank line is
    // the head of a chain.
    let mut prev_blank = true;
    // Whether the line being accumulated is still waiting for the expression
    // that completes it, so the line after it continues rather than starts.
    let mut prev_open = false;
    // The brackets the run so far has left unclosed. Reset at every head, which
    // is where the line above proved it had closed them all.
    let mut depth = 0usize;
    let mut run = 0usize;
    for (k, &s) in starts.iter().enumerate() {
        let raw = line_at(hay, &starts, k);
        // The segment after a trailing newline is not a line of the file.
        if raw.is_empty() && s == hay.len() {
            break;
        }
        let body = raw.trim_start();
        let dotted = body.starts_with('.');
        let comment = body.starts_with("//");
        let blank = raw.trim().is_empty();
        let joins = !blank
            && !comment
            && run < MAX_RUN
            && !spans.is_empty()
            && !prev_blank
            && (dotted || prev_open);
        if joins {
            // The break after `=` or inside a bracket stands where the source had
            // a space and the model puts the space back; a break before a `.` had
            // none to lose, and neither had one that only wrapped a bracket.
            if !dotted && !joins_without_a_space(&text, body) {
                while text.ends_with(' ') || text.ends_with('\t') {
                    text.pop();
                }
                text.push(' ');
            }
            text.push_str(body);
            let last = spans.len() - 1;
            spans[last].1 = s + raw.len();
            depth = bracket_depth(body, depth);
            prev_blank = false;
            prev_open = depth > 0 || ends_with_eq(body);
            run += 1;
            continue;
        }
        if !spans.is_empty() {
            text.push('\n');
        }
        text.push_str(raw);
        spans.push((s, s + raw.len()));
        prev_blank = blank;
        run = 1;
        depth = 0;
        prev_open = false;
        if !blank && !comment {
            depth = bracket_depth(raw, 0);
            prev_open = depth > 0 || ends_with_eq(raw);
        }
    }
    (text, spans)
}

/// Whether a line is left waiting for the expression that completes it.
fn ends_with_eq(line: &str) -> bool {
    line.trim_end().ends_with('=')
}

/// `depth` plus the brackets `line` opens and does not close.
///
/// Only `(` and `[` are counted. A `{` at the outermost level is a block's — a
/// function's, an `if`'s, a `match`'s — and rustfmt keeps one statement per line
/// inside one, so joining there would fold a whole body onto a line nobody quoted.
/// A struct literal's `{` is inside a `(` or `[` by the time rustfmt has wrapped
/// it, so the parenthesis already says the expression is unfinished.
///
/// Brackets that are text rather than code are skipped: a `//` comment runs to the
/// end of the line, a `/* */` one to its closer or to the end of the line when it
/// has none on it, and a string or character literal holds whatever it holds — one
/// holding `'('` would otherwise open a parenthesis nothing in the file closes.
fn bracket_depth(line: &str, mut depth: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '"' => {
                i = past_literal(&chars, i, '"');
                continue;
            }
            '\'' if starts_a_char_literal(&chars, i) => {
                i = past_literal(&chars, i, '\'');
                continue;
            }
            '/' if chars.get(i + 1) == Some(&'/') => break,
            '/' if chars.get(i + 1) == Some(&'*') => {
                match chars[i + 2..].windows(2).position(|w| w == ['*', '/']) {
                    Some(at) => {
                        i += 4 + at;
                        continue;
                    }
                    // The comment outlives this line, and no scan of one line can
                    // follow it. Counting the brackets under it is the worse
                    // guess, so the line stops here.
                    None => break,
                }
            }
            _ => {}
        }
        i += 1;
    }
    depth
}

/// The index just past a quoted run opening at `at`, honouring backslash escapes.
/// An unterminated one ends with the line, because that is where it stops being
/// this line's problem.
fn past_literal(chars: &[char], at: usize, quote: char) -> usize {
    let mut i = at + 1;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    chars.len()
}

/// Whether a `'` opens a character literal rather than a lifetime. Only a literal
/// can hold a bracket, and one holding `'('` would otherwise open a parenthesis
/// that nothing in the file ever closes.
fn starts_a_char_literal(chars: &[char], at: usize) -> bool {
    match chars.get(at + 1) {
        Some('\\') => chars.get(at + 2).is_some(),
        Some(_) => chars.get(at + 2) == Some(&'\''),
        None => false,
    }
}

/// Whether a formatter's break between two pieces of one expression stood for
/// nothing at all, so joining them must not put a space back. Rust writes none
/// after an opening bracket and none before a closing one or a separator; the
/// struct literal is the case that matters, because `Rec { a: 0.0, });` is what a
/// model writes for the file's `Rec {` / fields / `});`.
fn joins_without_a_space(left: &str, right: &str) -> bool {
    if matches!(left.trim_end().chars().next_back(), Some('(') | Some('[')) {
        return true;
    }
    matches!(
        right.chars().next(),
        Some(')') | Some(']') | Some(',') | Some(';')
    )
}

/// The one place a needle's lines occur once the file's chain breaks are
/// undone, when there is exactly one and a break really was undone.
///
/// `lr7b_v22` spent turns 5 through 11 — four identical calls, three compile
/// repairs and two refunds — on an `oldString` whose only fault was its line
/// breaks. The file said
///
/// ```text
///         self.user_dict
///             .values_mut()
///             .next()
///             .unwrap_or(&mut PersistedUserRecord {
/// ```
///
/// and the model wrote `self.user_dict.values_mut().next().unwrap_or(&mut
/// PersistedUserRecord {`, one line, with the struct body under it four spaces
/// shallower than the file has it. Turn 4 had been allowed to `read_file` that
/// very region and returned all eleven lines verbatim; the model re-emitted
/// eight anyway. It is the same inability [`indent_rescue`] exists for, one
/// dimension over: a small model reproduces the code it means, not the
/// formatter's decisions about where that code wraps.
///
/// Uniqueness is the licence, as it is there. The needle's shift cannot be used
/// the way `indent_rescue` uses one — the first line keeps the file's indent
/// while every line under it loses the nesting the chain was broken into, so no
/// single delta describes it — which is why this answer is narrower: it only
/// fires when joining recovered the match, and only on the first line's shift.
/// Both sides are joined, so a needle that breaks a chain the file keeps on one
/// line is answered the same way.
fn chain_join_rescue(hay: &str, needle: &str) -> Option<ChainJoin> {
    // A chain flattened onto one line still has to say something: `x.y()` is in
    // every source file, and uniqueness alone cannot carry a residue this short.
    const MIN_TRIMMED: usize = 24;
    let (joined_needle, _) = dechain(needle);
    let want: Vec<&str> = joined_needle.lines().collect();
    let n = want.len();
    if n == 0 {
        return None;
    }
    let want_trim: Vec<&str> = want.iter().map(|l| l.trim()).collect();
    if want_trim[0].is_empty() {
        return None;
    }
    if want_trim.iter().map(|l| l.chars().count()).sum::<usize>() < MIN_TRIMMED {
        return None;
    }
    let (joined_hay, spans) = dechain(hay);
    let got: Vec<&str> = joined_hay.lines().collect();
    if n > got.len() {
        return None;
    }
    let mut hits: Vec<usize> = Vec::new();
    for i in 0..=(got.len() - n) {
        if (0..n).all(|k| got[i + k].trim() == want_trim[k]) {
            hits.push(i);
            if hits.len() > 1 {
                return None;
            }
        }
    }
    let i = *hits.first()?;
    let start = spans[i].0;
    let end = spans[i + n - 1].1;
    // If no break was undone on either side then the needle's lines already
    // occur in the file, and `indent_rescue` — with its stricter one-delta rule
    // — is the truer answer. This one earns its looser footing only by
    // recovering a match the file's line breaks were hiding.
    let file_lines = hay[start..end].matches('\n').count() + 1;
    if file_lines == n && needle.lines().count() == n {
        return None;
    }
    let fk = crate::editor::auto_indent::leading_whitespace(&hay[start..]);
    let nk = crate::editor::auto_indent::leading_whitespace(want[0]);
    let (extra, deepen) = if fk.starts_with(nk) {
        (fk[nk.len()..].to_string(), true)
    } else if nk.starts_with(fk) {
        (nk[fk.len()..].to_string(), false)
    } else {
        // Tabs against spaces: not a re-wrap of the same code.
        return None;
    };
    Some(ChainJoin {
        start,
        end,
        line: hay[..start].matches('\n').count() + 1,
        extra,
        deepen,
        file_lines,
    })
}

/// Whether two trimmed lines are the same code with exactly one statement
/// terminator between them, in either direction.
///
/// One and only one, and only at the end: `foo();;` against `foo()` is a
/// different fault, and `foo(); // why` against `foo()` is not a terminator the
/// file lacks but a comment it does.
fn one_terminator_apart(a: &str, b: &str) -> bool {
    if b.len() == a.len() + 1 && b.ends_with(';') && &b[..b.len() - 1] == a {
        return true;
    }
    a.len() == b.len() + 1 && a.ends_with(';') && &a[..a.len() - 1] == b
}

/// The one place a refused `oldString` differs from the file by a single
/// statement terminator, found by folding both sides first.
///
/// `lr7b_v30` spent turns 3 through 9 — two refused edits alternating with
/// two `read_file` calls over the same fourteen lines — on an `oldString`
/// whose only fault was its last character. The file held eleven raw lines from
/// 287 to 297 whose last was `            })`, and the model sent the same code
/// flattened onto one line ending `});`. Both answers the chain had were wrong.
/// [`nearest_miss`] scored it zero, because its run is over raw lines and the
/// needle's first raw line matches nothing; [`explain_refused_edit`] then said
/// the text "appears nowhere in the file even with indentation ignored" and
/// sent the model to `read_file` a region it had just read twice. The tool
/// could fold that site — [`chain_join_rescue`] had done exactly it two
/// turns earlier, for the same code with the `;` in place — so the claim
/// that the text appears nowhere was false, and no line was named to read.
///
/// This is a refusal and not a rescue. A trailing `;` is Rust semantics: it is
/// what tells a statement from the value a block returns, so a needle that adds
/// one to a tail expression, or drops one from a statement, asks for a change
/// the tool cannot tell apart from the patch around it. Terminators stay out of
/// every rescuer and inside the diagnosers; this is the diagnosis that was
/// missing.
///
/// Uniqueness carries it, as it does in [`chain_join_rescue`], and so does that
/// function's residue floor, for the same reason: folded text still has to say
/// enough to name one place.
///
/// It also finds a needle that needed no fold, and that is not overreach but
/// the caller's to rank. [`nearest_miss`] says more when it can align the two
/// sides line by line -- how many of the needle's lines are already right, and
/// that only the last is not -- so the chain consults this one only after that
/// one and [`tail_anchor`] have both declined, which is how it ranks
/// [`tail_anchor`] too. `lr7b_v19` is the case that decided it: a needle broken
/// exactly like the file with one `;` on its last line, answered since that run
/// by "6 of its 7 non-blank lines".
fn terminator_miss(hay: &str, needle: &str) -> Option<TerminatorMiss> {
    // Shared with `chain_join_rescue` because both ask the same question of the
    // same folded text, and a residue this short turns up anywhere.
    const MIN_TRIMMED: usize = 24;
    let (joined_needle, _) = dechain(needle);
    let want: Vec<&str> = joined_needle.lines().collect();
    let n = want.len();
    if n == 0 {
        return None;
    }
    let want_trim: Vec<&str> = want.iter().map(|l| l.trim()).collect();
    if want_trim.iter().map(|l| l.chars().count()).sum::<usize>() < MIN_TRIMMED {
        return None;
    }
    let (joined_hay, spans) = dechain(hay);
    let got: Vec<&str> = joined_hay.lines().collect();
    if n > got.len() {
        return None;
    }
    let mut hits: Vec<usize> = Vec::new();
    for i in 0..=(got.len() - n) {
        // Every line but the last has to match exactly; the last one has to
        // match all the way to its terminator.
        if !(0..n - 1).all(|k| got[i + k].trim() == want_trim[k]) {
            continue;
        }
        let says = got[i + n - 1].trim();
        let wrote = want_trim[n - 1];
        if says != wrote && one_terminator_apart(says, wrote) {
            hits.push(i);
            if hits.len() > 1 {
                return None;
            }
        }
    }
    let i = *hits.first()?;
    let start = spans[i].0;
    let end = spans[i + n - 1].1;
    let site = &hay[start..end];
    let file_tail = site.lines().last().unwrap_or("").trim();
    Some(TerminatorMiss {
        line: hay[..start].matches('\n').count() + 1,
        file_lines: site.matches('\n').count() + 1,
        needle_lines: needle.lines().count(),
        file_has_the_terminator: file_tail.ends_with(';'),
        file_tail: clipped_line(file_tail),
        wrote_tail: clipped_line(needle.lines().last().unwrap_or("").trim()),
    })
}

/// The refusal for a `;`-apart site: the span is named, the file's own last line
/// is quoted against the one that was sent, and the reason no rescue is offered
/// is stated rather than left to be inferred from a bare refusal.
fn terminator_miss_message(rel_path: &str, m: &TerminatorMiss) -> String {
    let last = m.line + m.file_lines - 1;
    let side = if m.file_has_the_terminator {
        "the file's line ends with a `;` and yours does not"
    } else {
        "yours ends with a `;` and the file's does not"
    };
    // Only worth saying when the two sides really do wrap differently; "across
    // 7 line(s) where your `oldString` wrote 7" is noise that buries the `;`.
    let wrapped = if m.file_lines == m.needle_lines {
        String::new()
    } else {
        format!(
            ", which the file wraps across {} line(s) where your `oldString` wrote {}",
            m.file_lines, m.needle_lines
        )
    };
    format!(
        "its `oldString` is not in {rel_path}, but the code it quotes is, in exactly one place: \
         lines {line}-{last}{wrapped}. The whole difference is a statement terminator on the \
         last of them \u{2014} {side}. You wrote `{wrote_tail}`, the file says `{file_tail}`. A \
         trailing `;` is Rust semantics and not formatting: it is what tells a statement from the \
         value a block returns, so this tool will neither add one nor drop one for you. \
         `read_file` lines {line} to {last}, copy them into the `oldString` verbatim, and settle \
         the terminator in the `newString`, where the change you are making lives.",
        line = m.line,
        last = last,
        wrapped = wrapped,
        side = side,
        wrote_tail = m.wrote_tail,
        file_tail = m.file_tail,
    )
}

/// A replacement with the lines it only copied from around its site removed.
#[derive(Debug)]
struct CopiedContext {
    /// The replacement to write, in the file's own line endings.
    text: String,
    /// Lines dropped from its head, which the file already has above the site.
    above: usize,
    /// Lines dropped from its tail, which the file already has below the site.
    below: usize,
}

/// What a replacement turned out to hold once the file's neighbours were read
/// out of it.
enum ContextTrim {
    /// It repeated nothing from outside the span it replaces.
    Keep,
    /// It did, and this is what is left to write.
    Trimmed(CopiedContext),
    /// What is left cannot be written. `everything` says the replacement held
    /// only lines the file carries around the site, so trimming it would delete
    /// the matched text; otherwise what is left is the matched text itself, so
    /// writing it would change nothing.
    Unwritable { dropped: usize, everything: bool },
}

/// Take out of a replacement the lines that only repeat the file's own lines
/// immediately outside the span it replaces.
///
/// `lr7b_v23` turn 4 matched thirteen lines — the `self.user_dict` chain and the
/// `}` under it — and replaced them with sixteen whose first two were
///
/// ```text
///         self.user_dict.insert(user, record);
///         let _ = self.save_data();
/// ```
///
/// which are lines 285 and 286 of the specimen, the two the file already carried
/// immediately above the match. The splice wrote all four, rustc answered with
/// `E0382: use of moved value: user` at 287:31 and `use of moved value: record`
/// at 287:37, and the tool's own answer to the turn that caused them was
/// "Success: applied 1 edit(s)" — nothing told the model to look. Turns 5 and 6
/// tried to fix the pair and were refused, correctly, because
/// `self.user_dict.insert(user, record);` by then matched two places and the
/// ambiguity guard will not choose between them. Turns 7 to 9 were three
/// byte-identical `read_file` calls of lines 285 to 287 and turn 10 produced no
/// tool call, so one write took the run from two standing errors to four and then
/// to nothing.
///
/// A model that copies its context into a replacement means the copy as context:
/// it is saying where the change goes, because its `oldString` was too short to
/// say that on its own. Dropping the copied part makes the short `oldString`
/// behave like the longer one it should have sent, which is the change it asked
/// for and not a second guess at it.
///
/// Two lines is the floor because one is a pattern a patch legitimately writes —
/// a second `push`, a second closing brace — where two consecutive lines
/// reproducing, in order, the two lines beside the span is a copy. A blank line
/// fails a run rather than extending it: spacing between items is formatting the
/// model may be choosing, not context it is quoting.
fn trim_copied_context(hay: &str, start: usize, end: usize, patch: &str) -> ContextTrim {
    const MIN_RUN: usize = 2;
    let starts = line_starts(patch);
    // `line_starts` counts the segment after a trailing newline, which is not a
    // line of the replacement.
    let total = starts.len() - usize::from(patch.ends_with('\n'));
    if total < MIN_RUN {
        return ContextTrim::Keep;
    }
    // The segment above `start` and the one below `end` are the partial lines the
    // span begins and ends inside, not neighbours of it.
    let above: Vec<&str> = hay[..start].split('\n').collect();
    let above = &above[..above.len() - 1];
    let below: Vec<&str> = hay[end..].split('\n').collect();
    let below = &below[1..];

    // The head run is the replacement's first lines against the file's last
    // lines above the span, both in file order, so a run of `b` pairs
    // `patch[j]` with `above[above.len() - b + j]`. Lengths are searched longest
    // first: a run of three is a copy of three lines and not a copy of two that
    // happens to be followed by a third.
    let mut a = 0usize;
    'head: for b in (MIN_RUN..=total.min(above.len())).rev() {
        for j in 0..b {
            let got = line_at(patch, &starts, j).trim();
            if got.is_empty() || got != above[above.len() - b + j].trim() {
                continue 'head;
            }
        }
        a = b;
        break;
    }
    // The tail run meets the file at the other end of the span, so it pairs the
    // replacement's last lines with the file's first lines below the span.
    let mut b = 0usize;
    'tail: for k in (MIN_RUN..=(total - a).min(below.len())).rev() {
        for j in 0..k {
            let got = line_at(patch, &starts, total - k + j).trim();
            if got.is_empty() || got != below[j].trim() {
                continue 'tail;
            }
        }
        b = k;
        break;
    }
    if a + b == 0 {
        return ContextTrim::Keep;
    }
    if a + b >= total {
        return ContextTrim::Unwritable {
            dropped: a + b,
            everything: true,
        };
    }
    let text = patch[starts[a]..tail_cut(&starts, total - b, patch.len())].to_string();
    if text.trim() == hay[start..end].trim() {
        return ContextTrim::Unwritable {
            dropped: a + b,
            everything: false,
        };
    }
    ContextTrim::Trimmed(CopiedContext {
        text,
        above: a,
        below: b,
    })
}

/// Where to cut a replacement that gives up its last `keep_from` line, which is
/// `patch.len()` when it has no trailing newline to cut at.
fn tail_cut(starts: &[usize], keep_from: usize, len: usize) -> usize {
    if keep_from < starts.len() {
        starts[keep_from]
    } else {
        len
    }
}

/// What dropping the copied lines tells the model, in the success message.
///
/// It has to be told. The replacement it sent is not the text that was written,
/// and a model that believes its own payload went in verbatim will read the
/// file back, see two lines missing from what it wrote, and send them again.
fn copied_context_note(n: usize, c: &CopiedContext) -> String {
    let what = match (c.above, c.below) {
        (above, 0) => format!("{above} line(s) the file already has above the text it replaced"),
        (0, below) => format!("{below} line(s) the file already has below the text it replaced"),
        (above, below) => format!(
            "{above} line(s) the file already has above the text it replaced and {below} below it"
        ),
    };
    format!(
        "Edit {n} was applied with {what} dropped from its `newString`: those lines sit \
         immediately outside the text the edit replaced, so writing them again would have put \
         them in the file twice. The change is in the file — do not re-send it."
    )
}

/// The file a patch was written into, and what the patch turned out to be.
#[derive(Debug)]
struct Spliced {
    /// The whole file with the patch spliced over the span.
    working: String,
    /// The patch as written. A brace-balance check has to read this rather than
    /// the patch as sent: the lines dropped from it are braces too when they are
    /// braces at all.
    wrote: String,
    /// What to tell the model about the drop, if there was one.
    note: Option<String>,
}

/// Splice `patch` over `hay[start..end]`, dropping from it first the lines that
/// only repeat the file's neighbours of that span.
///
/// The `Err` is the refusal for the batch's `rejected` list, worded as a reason
/// rather than as a whole sentence so the caller can decide what it means to
/// refuse one edit of many.
fn splice_trimmed(
    hay: &str,
    start: usize,
    end: usize,
    patch: String,
    n: usize,
) -> Result<Spliced, String> {
    let (patch, note) = match trim_copied_context(hay, start, end, &patch) {
        ContextTrim::Keep => (patch, None),
        ContextTrim::Trimmed(c) => {
            let note = copied_context_note(n, &c);
            (c.text, Some(note))
        }
        ContextTrim::Unwritable {
            dropped,
            everything,
        } => {
            return Err(if everything {
                format!(
                    "edit {n}: its `newString` is nothing but {dropped} line(s) the file already \
                     carries immediately around the text this edit replaces, so writing it would \
                     put those lines in the file twice and trimming them would delete the matched \
                     text instead of changing it. Re-issue the edit carrying the change you mean \
                     and no lines copied from around the site."
                )
            } else {
                format!(
                    "edit {n}: once the {dropped} line(s) its `newString` copies from the file \
                     around the matched text are dropped, what is left is that text unchanged, so \
                     this edit would change no characters. The error is still in the file at that \
                     line — write the replacement the error asks for."
                )
            })
        }
    };
    let working = [&hay[..start], patch.as_str(), &hay[end..]].concat();
    Ok(Spliced {
        working,
        wrote: patch,
        note,
    })
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

/// A replacement the file already carries at the site its own match found.
#[derive(Debug, PartialEq, Eq)]
struct SiteRepeat {
    /// The line the replacement starts on, 1-based.
    line: usize,
    /// How many of its lines the write would put in the file a second time.
    extra: usize,
}

/// Whether the file already carries a replacement at the site its own match
/// found, and where.
///
/// `already_applied_at` answers that question from the whole file, so it has to
/// demand a replacement long enough that a match cannot be luck. Here the site is
/// one the `oldString` was proved to reach exactly once, so the only question left
/// is whether the file goes on from there with the rest of the replacement — and
/// proving that needs no length floor.
///
/// This is what a replayed write leaves behind. `lr7b_v31` sent one `edit_file`
/// twice, byte for byte: `oldString` a single line, `newString` that line plus
/// `let _ = self.save_data();`. The first call applied it. The second found the
/// same unique site, spliced the same two lines over the same one, and left the
/// file carrying the insert twice — after which every `oldString` the model built
/// was stale, so turns 4 to 9 alternated a refusal with a re-read of the same
/// fourteen lines and the transcript ended at 29 identical refusals.
/// `trim_copied_context` could not have caught it: it needs a run of two
/// reproduced lines, and the copy here is one.
///
/// Two limits keep the claim provable. The replacement has to be *longer* than
/// the span it covers, because one of the same length is a rewrite, and a rewrite
/// is where a model legitimately changes indentation — which this comparison,
/// being blind to whitespace on purpose, would read as nothing to do. And the
/// span has to cover whole lines, because a fragment's own line in the file holds
/// more than the replacement's first line does, so the run below would be
/// comparing two different things.
fn already_written_at_site(hay: &str, start: usize, end: usize, patch: &str) -> Option<SiteRepeat> {
    // An `oldString` that ends on a newline ends at the start of the next line,
    // which is not a line of the span. Backing off one byte keeps the whole-line
    // test below reading the span's own last line rather than its successor's.
    let last = if end > start && hay.as_bytes()[end - 1] == b'\n' {
        end - 1
    } else {
        end
    };
    let line_start = hay[..start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = hay[last..].find('\n').map_or(hay.len(), |i| last + i);
    if !hay[line_start..start].trim().is_empty() || !hay[last..line_end].trim().is_empty() {
        return None;
    }
    let want: Vec<&str> = patch.lines().map(str::trim).collect();
    let covered = hay[start..end].lines().count();
    if want.len() <= covered {
        return None;
    }
    let got: Vec<&str> = hay[start..]
        .lines()
        .map(str::trim)
        .take(want.len())
        .collect();
    (got == want).then(|| SiteRepeat {
        line: hay[..start].matches('\n').count() + 1,
        extra: want.len() - covered,
    })
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
    brace_depths(src).last().map(|(_, d)| *d).unwrap_or(0)
}

/// Every brace in `src` that is code, as the byte offset it sits at and the
/// depth the file is at after it. `brace_delta` is the last of these, so the two
/// cannot disagree about what counts as a brace.
///
/// The offsets are what makes a drift *quotable*: a count on its own can say a
/// patch is wrong but not which of the file's own lines it is wrong about, and
/// `lr7b_v35` spent four turns on that — see `drift_report`.
fn brace_depths(src: &str) -> Vec<(usize, i32)> {
    let b = src.as_bytes();
    let mut depth: i32 = 0;
    let mut out: Vec<(usize, i32)> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'{' => {
                depth += 1;
                out.push((i, depth));
                i += 1;
            }
            b'}' => {
                depth -= 1;
                out.push((i, depth));
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
    out
}

/// The lines of `hay` inside `range` at which a code `}` takes the file's brace
/// depth below what it had at `range.start`, plus how many such lines exist in
/// `range` in all.
///
/// Two guards the fragment scan did not have:
///   * the scan runs over the whole file, not just the fragment, so a comment or
///     string that straddles `range`'s boundary keeps its true lexical role and
///     a brace hidden inside it is not mistaken for a closer of real code;
///   * the character at the event has to be `}`, because a `{` that merely lifts
///     a still-below-baseline depth opens a block, it does not close one, and
///     quoting it would send the model after a line the drift is not about.
///
/// One entry per line however many closers on it drop: `} else {` closes a
/// block and opens one on the same line, and `}}` closes two, but each is a
/// single fact about one line for the purpose of widening.
///
/// `from_end` picks which of them are quoted once there are more than `limit`,
/// and the two drift directions want opposite ends -- see `drift_site`.
fn depth_dropping_lines(
    hay: &str,
    range: std::ops::Range<usize>,
    limit: usize,
    from_end: bool,
) -> (Vec<(usize, String)>, usize) {
    let starts = line_starts(hay);
    let lines: Vec<&str> = hay.lines().collect();
    let bytes = hay.as_bytes();
    let events = brace_depths(hay);
    let baseline_idx = events.partition_point(|e| e.0 < range.start);
    let baseline = if baseline_idx > 0 {
        events[baseline_idx - 1].1
    } else {
        0
    };
    let mut drops: Vec<usize> = Vec::new();
    for (off, depth) in events {
        if off < range.start || off >= range.end {
            continue;
        }
        if bytes.get(off) != Some(&b'}') || depth >= baseline {
            continue;
        }
        let idx = starts.partition_point(|s| *s <= off).saturating_sub(1);
        if !drops.contains(&idx) {
            drops.push(idx);
        }
    }
    let total = drops.len();
    let from = if from_end {
        total.saturating_sub(limit)
    } else {
        0
    };
    let to = if from_end { total } else { limit.min(total) };
    let out = drops[from..to]
        .iter()
        .map(|&idx| (idx + 1, clipped_line(lines.get(idx).copied().unwrap_or(""))))
        .collect();
    (out, total)
}

/// Where a drifting edit matched in the file on disk, and which of the file's
/// own lines the drift is about.
struct DriftSite {
    /// First line the matched text is on, 1-based.
    from: usize,
    /// Last line it reaches, 1-based.
    to: usize,
    /// The lines that carry the drift, with their numbers.
    closers: Vec<(usize, String)>,
    /// How many such lines were left off the end of `closers`.
    more: usize,
}

/// The most lines one drifting edit may quote. A batch is refused and nothing is
/// written, so the model has to re-read the region anyway; past eight lines the
/// report is a paste of the file rather than an answer about it.
const MAX_DRIFT_LINES: usize = 8;

/// Locate the text an edit matched in `hay` and pick out the lines its drift is
/// about.
///
/// `None` whenever the site cannot be proved, and the report then names no line:
/// the text has to appear in `hay` exactly once, because the refusal prints a
/// number and a number that points at the wrong site sends the model to edit
/// there. An empty `matched` is refused by the same test -- every position
/// contains the empty string, so it is never "exactly once".
///
/// `hay` is the file as it stands on disk, not the batch's in-memory copy: a
/// refused batch writes nothing, so the disk is what the model reads next.
fn drift_site(hay: &str, matched: &str, drift: i32) -> Option<DriftSite> {
    let hay = hay.replace("\r\n", "\n");
    let needle = matched.replace("\r\n", "\n");
    let at = hay.find(&needle)?;
    let end = at + needle.len();
    if hay[end..].contains(&needle) {
        return None;
    }
    let from = hay[..at].matches('\n').count() + 1;
    let to = hay[..end].matches('\n').count() + usize::from(!needle.ends_with('\n'));
    let want = (drift.unsigned_abs() as usize).clamp(1, MAX_DRIFT_LINES);
    // The two directions quote different text, and the difference is the whole
    // answer. A negative drift means the `newString` closed blocks early, so the
    // lines that have lost their block are the file's own, below the match, and
    // the ones nearest it are the ones it got to first. A positive drift means it
    // dropped closers it matched, so those closers are inside the match and the
    // file below it is innocent -- and a replacement that loses closers loses the
    // ones at its end, so the last of them are the ones to quote.
    let (closers, total) = if drift < 0 {
        depth_dropping_lines(&hay, end..hay.len(), want, false)
    } else {
        depth_dropping_lines(&hay, at..end, want, true)
    };
    Some(DriftSite {
        from,
        to,
        more: total.saturating_sub(closers.len()),
        closers,
    })
}

/// One edit of a refused batch whose `newString` changed the file's brace
/// balance, with the lines the change is about.
struct DriftedEdit {
    n: usize,
    drift: i32,
    site: Option<DriftSite>,
}

/// Record an edit that drifted, or nothing when it kept the file's balance. The
/// batch total is the sum of what this returns, which is the same number the
/// single accumulator used to hold: an edit that drifts by zero adds nothing to
/// either.
fn drifted_edit(current: &str, matched: &str, n: usize, drift: i32) -> Option<DriftedEdit> {
    (drift != 0).then(|| DriftedEdit {
        n,
        drift,
        site: drift_site(current, matched, drift),
    })
}

/// The per-edit part of a brace-drift refusal: the lines the edit matched, the
/// file's own lines the drift is about, quoted, and the advice for *this* edit's
/// direction.
///
/// Per edit rather than per batch because a batch can drift both ways at once
/// and the two directions are opposite instructions -- `brace_drift_advice`
/// exists because one paragraph for both cost `lr7b_v29` four turns.
fn drift_report(path: &str, d: &DriftedEdit) -> String {
    let k = d.drift.unsigned_abs();
    let fault = if d.drift < 0 {
        format!("carries {k} more closing brace(s) than the text it replaces")
    } else {
        format!("leaves {k} block(s) open that the text it replaces closed")
    };
    let mut out = match &d.site {
        Some(s) => {
            let span = if s.from == s.to {
                format!("line {}", s.from)
            } else {
                format!("lines {}-{}", s.from, s.to)
            };
            let mut t = format!(
                "edit {}: its `oldString` matched {span} of {path} and its `newString` {fault}.",
                d.n
            );
            // The span is a fact about the file even when the closer list is
            // empty (an enclosing block whose only closer is outside the match,
            // e.g. a whole-file match on `fn f(){{}}`), so it is quoted here
            // first; the "These are..." paragraph that walks the model through
            // individual closer lines only runs when there is one to walk to.
            if !s.closers.is_empty() {
                let below = if d.drift < 0 {
                    "These are the first of the file's own lines below that match at which its \
                     brace depth drops, quoted with the file's own numbers \u{2014} once your \
                     `newString` has closed those blocks early there is nothing left for them to \
                     close:"
                } else {
                    "These are the last of the lines inside that match at which the file's brace \
                     depth drops, quoted with the file's own numbers \u{2014} the closers your \
                     `oldString` covered:"
                };
                t.push_str(&format!(" {below}"));
                for (line, text) in &s.closers {
                    t.push_str(&format!("\n[line {line}] {text}"));
                }
                if s.more > 0 {
                    t.push_str(&format!("\n[\u{2026} and {} more such line(s)]", s.more));
                }
            }
            t
        }
        _ => format!(
            "edit {}: its `oldString` matched and its `newString` {fault}.",
            d.n
        ),
    };
    out.push('\n');
    out.push_str(brace_drift_advice(d.drift));
    out
}

/// What the brace-drift refusal tells the model to do next, which is not the
/// same advice in both directions and was one paragraph for both until
/// `lr7b_v29`.
///
/// A positive drift means the `newString` dropped a closer the `oldString`
/// matched, so the fix is on the `newString` side and the old paragraph names
/// it correctly. A negative drift means the opposite: the `newString` closes
/// blocks whose closing lines the `oldString` never covered, and those file
/// lines are still standing. Telling that model to carry every delimiter it
/// matched sends it after more closers, which is the one change guaranteed to
/// raise the count -- v29 obeyed it four times and read back 1, 2, 2, 3.
fn brace_drift_advice(drift: i32) -> &'static str {
    if drift > 0 {
        "When an `oldString` matches a line that closes a block, its `newString` has to \
         repeat that closing brace and put the new text around it, not in place of it. \
         Re-issue the batch carrying every delimiter the `oldString` matched."
    } else {
        "Your `newString` closes blocks its `oldString` never covered, so the lines below \
         the site that already close them are left standing with nothing to close. Widen the \
         `oldString` down to the last of those lines and carry it into the `newString`, or \
         take the extra closing braces back out. More closers raise this count."
    }
}

/// The per-edit wording for a batch in which *every* edit writes back the text it
/// matched. The sentence is the one a single no-op edit has always been answered
/// with, because that is the case the model can act on: nothing else in the batch
/// was going to change the file either, so there is no partial success to report
/// and no line number to point at beyond the ones it quoted.
fn noop_refusal(noop: &[usize]) -> String {
    noop.iter()
        .map(|n| {
            format!(
                "edit {n}: its `newString` is byte-identical to its `oldString`, so this \
                 edit would change no characters. The error is still in the file at that \
                 line — write the replacement the error asks for."
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What a *successful* batch says about the edits it dropped for changing no
/// characters. Written without grammatical agreement on purpose: the count varies
/// from one dropped edit to four, and the sentence has to read the same for both,
/// so it says "Each such edit" rather than naming a number the list contradicts.
fn noop_edit_note(noop: &[usize]) -> String {
    let list = noop
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Left out as changing no characters: edit {list}. Each such edit has a `newString` \
         byte-identical to its `oldString`, so the text it quotes is still in the file exactly \
         as it was and no error at any of those lines has moved. Where the diagnostics name one, \
         the replacement still has to be written."
    )
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

/// How many bytes a file holds once the formatter has had it.
///
/// The buffer a write leaves behind is not the file. `format_written_file` runs
/// *after* `fs::write` and can rewrite the whole thing, so a count taken from the
/// buffer is the count of a file that no longer exists. `lr7b_v31` was told
/// "(137 \u{2192} 163 bytes)" about a file that was 171 — eight bytes out, which is
/// exactly what rustfmt added — and it was told that in the same message that said
/// "rustfmt reformatted this file, so its bytes differ from what you sent". A
/// model reads the count to check its write landed, so the count has to come off
/// the disk.
///
/// `sent` is the fallback, not the answer: a file that cannot be statted is a
/// file that cannot be read back either, and reporting what was written is the
/// honest thing left to say.
fn bytes_on_disk(path: &Path, sent: usize) -> usize {
    fs::metadata(path).map(|m| m.len() as usize).unwrap_or(sent)
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
            let mut rescued: Vec<String> = Vec::new();
            let mut noop: Vec<usize> = Vec::new();
            // Rust is the one language whose edits the agent loop grades with a
            // compiler, and a batch that changes the file's brace balance cannot
            // be anything but a mistake: `mod tests {` left open by a `newString`
            // that dropped the `}`s its `oldString` matched. testgate2b is the
            // cost — the edit applied cleanly, rustc reported an unclosed
            // delimiter, and the 391s repair turn that followed made the file
            // worse. Refusing at this point names the mistake for free.
            //
            // Per edit, not as one running total: the refusal is written after
            // the loop, by which time the site each edit matched is gone, and a
            // total with no site behind it can only say "somewhere in this batch"
            // -- which is what `lr7b_v35` was told and could not act on.
            let is_rust = rel_path.replace('\\', "/").ends_with(".rs");
            let mut drifted: Vec<DriftedEdit> = Vec::new();
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
                // An edit that hands back the text it matched writes no bytes, so
                // it cannot be the reason a batch is refused: dropping it leaves
                // the file exactly where the surviving edits put it, which is a
                // state the model did describe. lr7b_v34 is the cost of the other
                // reading — one no-op beside one real edit vetoed the real edit
                // four turns running, and the run ended at 3/3 repair turns with
                // both original errors still standing. A batch that is *only*
                // no-ops is still refused, because there the file would be
                // written and nothing in it would move, which reads to a repair
                // turn as a fix that was not one.
                //
                // This is the literal case and the only one dropped. The no-op
                // `splice_trimmed` derives by taking copied context lines back out
                // still refuses its batch: that one rests on an inference about
                // what the model meant, and silently discarding an edit on an
                // inference is not the same as discarding one that is proved to
                // write nothing.
                if new == old {
                    noop.push(n);
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
                    // Everything below is read from the file. The two that apply
                    // come before the three that explain, because an explanation
                    // of why text cannot be located is false the moment something
                    // locates it: `lr7b_v22` was told "the text is gone because
                    // this place has already been changed" about a site that was
                    // sitting there untouched, its chain merely re-wrapped.
                    // `already_applied_at` stays ahead of them all — a
                    // `newString` already on disk means the change was made, and
                    // applying the `oldString` somewhere else would write it
                    // twice. A batch is all-or-nothing, so these read `current`
                    // for the file as it stands on disk and `working` for the
                    // spans they splice.
                    if let Some(line) = already_applied_at(&current, new) {
                        rejected.push(format!(
                            "edit {n}: this change is already on disk — its `newString` is in \
                             {rel_path} verbatim at line {line}, and its `oldString` is not in the \
                             file at all. Nothing to redo here. If the compiler still reports an \
                             error, it is asking for a different change, at the line it names."
                        ));
                        continue;
                    }
                    // A model cannot count leading whitespace: `lr7b_v20` sent the
                    // same eight-space `oldString` against a twelve-space file ten
                    // times, and each refusal told it the truth — its lines were
                    // in exactly one place, here they are verbatim — and it
                    // re-guessed eight spaces anyway. One candidate site is the
                    // site it meant, so apply it and say so.
                    if let Some(r) = indent_rescue(&working, old) {
                        let lf = reindent(&new.replace("\r\n", "\n"), &r.extra, r.deepen);
                        let patch = if crlf_file {
                            lf.replace('\n', "\r\n")
                        } else {
                            lf
                        };
                        let matched = if is_rust {
                            working[r.start..r.end].to_string()
                        } else {
                            String::new()
                        };
                        let before = if is_rust { brace_delta(&matched) } else { 0 };
                        match splice_trimmed(&working, r.start, r.end, patch, n) {
                            Ok(s) => {
                                let rec = if is_rust {
                                    drifted_edit(
                                        &current,
                                        &matched,
                                        n,
                                        brace_delta(&s.wrote) - before,
                                    )
                                } else {
                                    None
                                };
                                if let Some(rec) = rec {
                                    drifted.push(rec);
                                }
                                working = s.working;
                                let how = if r.extra.is_empty() {
                                    "only its trailing whitespace differed".to_string()
                                } else {
                                    format!(
                                        "your `oldString` was {} whitespace character(s) {} than \
                                         the file",
                                        r.extra.chars().count(),
                                        if r.deepen { "shallower" } else { "deeper" }
                                    )
                                };
                                rescued.push(format!(
                                    "Edit {n} was applied at line {} after ignoring leading \
                                     whitespace: {how}, so its replacement was re-indented to \
                                     match. The change is in the file — do not re-send it.",
                                    r.line
                                ));
                                if let Some(note) = s.note {
                                    rescued.push(note);
                                }
                            }
                            Err(why) => rejected.push(why),
                        }
                        continue;
                    }
                    // Nor can it reproduce where a formatter chose to wrap: v22
                    // read the eleven lines back from `read_file` and re-sent
                    // them as eight, four times. Joining the file's chain
                    // continuations is the same answer one dimension over, and
                    // it is Rust-only because that is where a leading `.` is
                    // provably a continuation.
                    let joined = if is_rust {
                        chain_join_rescue(&working, old)
                    } else {
                        None
                    };
                    if let Some(r) = joined {
                        let lf = reindent(&new.replace("\r\n", "\n"), &r.extra, r.deepen);
                        let patch = if crlf_file {
                            lf.replace('\n', "\r\n")
                        } else {
                            lf
                        };
                        let matched = if is_rust {
                            working[r.start..r.end].to_string()
                        } else {
                            String::new()
                        };
                        let before = if is_rust { brace_delta(&matched) } else { 0 };
                        match splice_trimmed(&working, r.start, r.end, patch, n) {
                            Ok(s) => {
                                let rec = if is_rust {
                                    drifted_edit(
                                        &current,
                                        &matched,
                                        n,
                                        brace_delta(&s.wrote) - before,
                                    )
                                } else {
                                    None
                                };
                                if let Some(rec) = rec {
                                    drifted.push(rec);
                                }
                                working = s.working;
                                rescued.push(format!(
                                    "Edit {n} was applied at line {} after ignoring its line \
                                     breaks: the file wraps that code across {} lines and your \
                                     `oldString` wrote it on {}, so the match was made on the code \
                                     rather than on where the formatter wrapped it. The change is \
                                     in the file — do not re-send it.",
                                    r.line,
                                    r.file_lines,
                                    old.lines().count()
                                ));
                                if let Some(note) = s.note {
                                    rescued.push(note);
                                }
                            }
                            Err(why) => rejected.push(why),
                        }
                        continue;
                    }
                    // Naming the one line that differs beats pasting the sites of
                    // the line that already matched: v19 re-sent the same call
                    // five times because the paste showed only its correct part.
                    // Both it and the tail anchor beat `superseded_site`'s
                    // history, when either has found the site: a place still
                    // standing with the quoted text in it is not a place an
                    // earlier write replaced, and that is a reading of the file
                    // while the history is a guess about it. See
                    // `NearMiss::locates_the_site`.
                    let miss = nearest_miss(&current, old);
                    // The hole answers the case the prefix run misreads: a needle
                    // that quotes two runs of the file and steps over the lines
                    // between them scores `matched` up to the break and then
                    // reports the needle's own tail as the line that differs,
                    // which prescribes a substitution that would delete the rest
                    // of the block. It ranks above that one because it says
                    // strictly more — the same head, plus where the tail really
                    // is and what the file holds in between — and above the fold
                    // for the same reason. It needs the `newString` as its
                    // witness, so it reads `current` for the file and `new` for
                    // the patch. See `skipped_block`.
                    let skipped = skipped_block(&current, old, new);
                    // The tail anchor answers the case the prefix run cannot: a
                    // needle whose first line is wrong scores zero on every site
                    // however exactly the rest of it stands in the file.
                    let anchor = if miss.is_some() {
                        None
                    } else {
                        tail_anchor(&current, old)
                    };
                    // The fold answers the case `nearest_miss` cannot: a needle
                    // that had to be folded to be found at all. That one reads
                    // raw lines, so v30's eleven-line site quoted as one line
                    // ending `});` scored zero on every one of them however
                    // exactly the code stood in the file, and the answer became
                    // "appears nowhere in the file" about a site the tool had
                    // folded and matched two turns earlier. It ranks below that
                    // one because it says less: v19's shape, a needle broken
                    // like the file with one `;` on its last line, is answered
                    // better by "6 of its 7 non-blank lines", and that rank
                    // lives in the order the two are pushed below, which is
                    // also what settles `tail_anchor` -- that one requires the
                    // needle's last line to stand in the file and this one
                    // requires it not to. It is still computed once the near
                    // miss has fired, because the only thing left for it to
                    // change is `located`, and a fold that found the site is a
                    // reading of the file while `superseded_site` is a guess
                    // about its history: gating the computation on the near
                    // miss would hand a near miss too loose to locate the site
                    // back to that guess. It refuses rather than applies,
                    // because a `;` is semantics and not wrapping, and it is
                    // Rust-only because both the fold and the rule it cites are
                    // Rust's.
                    let terminator = if !is_rust {
                        None
                    } else {
                        terminator_miss(&current, old)
                    };
                    let located = skipped.is_some()
                        || miss.as_ref().is_some_and(NearMiss::locates_the_site)
                        || anchor.is_some()
                        || terminator.is_some();
                    if !located {
                        if let Some(line) = superseded_site(&current, old, new) {
                            rejected.push(format!(
                                "edit {n}: its `oldString` is not in {rel_path}. {}",
                                superseded_site_message(&current, line)
                            ));
                            continue;
                        }
                    }
                    if let Some(s) = skipped {
                        rejected.push(format!("edit {n}: {}", skipped_block_message(rel_path, &s)));
                        continue;
                    }
                    if let Some(miss) = miss {
                        rejected.push(format!("edit {n}: {}", near_miss_message(rel_path, &miss)));
                        continue;
                    }
                    if let Some(anchor) = anchor {
                        rejected.push(format!(
                            "edit {n}: {}",
                            tail_anchor_message(rel_path, &anchor)
                        ));
                        continue;
                    }
                    if let Some(t) = terminator {
                        rejected.push(format!(
                            "edit {n}: {}",
                            terminator_miss_message(rel_path, &t)
                        ));
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
                    // A `newString` that opens or closes by repeating the file's
                    // own lines beside the site writes them a second time: v23's
                    // turn 4 duplicated two lines and left the run with two E0382s
                    // it never had, then died trying to undo them. The copy is
                    // context, so it goes.
                    let at = working.find(&needle).unwrap_or(0);
                    let end = at + needle.len();
                    // Refuse before writing, not after: the copy lands in the
                    // file the moment the splice does, and the model's next
                    // `oldString` is built from a picture of the file that no
                    // longer matches it. The message has to carry "already on
                    // disk" — that phrase is what `replay_advice` keys on to say
                    // no version of this call can do anything, which is the one
                    // thing a model replaying it needs to hear. See
                    // `already_written_at_site`.
                    if let Some(rep) = already_written_at_site(&working, at, end, &repl) {
                        rejected.push(format!(
                            "edit {n}: this change is already on disk \u{2014} {rel_path} carries \
                             every line of its `newString` from line {} down, and its `oldString` \
                             still matches inside that run. Applying it again would write {} of \
                             those lines a second time. Nothing to redo here. If the compiler \
                             still reports an error, it is asking for a different change, at the \
                             line it names.",
                            rep.line, rep.extra
                        ));
                        continue;
                    }
                    let before = if is_rust { brace_delta(&needle) } else { 0 };
                    match splice_trimmed(&working, at, end, repl, n) {
                        Ok(s) => {
                            let rec = if is_rust {
                                drifted_edit(&current, &needle, n, brace_delta(&s.wrote) - before)
                            } else {
                                None
                            };
                            if let Some(rec) = rec {
                                drifted.push(rec);
                            }
                            working = s.working;
                            if let Some(note) = s.note {
                                rescued.push(note);
                            }
                        }
                        Err(why) => {
                            rejected.push(why);
                            continue;
                        }
                    }
                }
            }
            if !rejected.is_empty() {
                return Err(format!(
                    "no edits were applied to {rel_path}; the file is unchanged:\n{}",
                    rejected.join("\n")
                )
                .into());
            }
            if noop.len() == edits.len() {
                return Err(format!(
                    "no edits were applied to {rel_path}; the file is unchanged:\n{}",
                    noop_refusal(&noop)
                )
                .into());
            }
            // Only the edits that wrote bytes are counted from here on: a dropped
            // no-op is not part of the batch that reached the file, and saying it
            // was would tell a repair turn that more of its patch landed than did.
            let applied = edits.len() - noop.len();
            // The sum of the per-edit drifts, which is the number the single
            // accumulator used to hold: an edit that changes nothing adds zero to
            // either, and the batch is refused on the total while the report below
            // is written per edit.
            let brace_drift: i32 = drifted.iter().map(|d| d.drift).sum();
            if is_rust && brace_drift != 0 {
                let (moved, glyph) = if brace_drift > 0 {
                    (brace_drift, "block left open")
                } else {
                    (-brace_drift, "brace with no block to close")
                };
                let mut why = format!(
                    "no edits were applied to {rel_path}; the file is unchanged: this batch of \
                     {applied} edit(s) would leave the file with {moved} more {glyph} than it has now. \
                     That is never what a patch means."
                );
                // A batch that drifts in both directions has no one instruction,
                // and the count above is the sum of two opposite mistakes. Say so
                // before the per-edit answers -- "drifts both ways" is also what
                // `replay_advice` keys on, so that a replay note cannot repeat one
                // direction's advice over a batch that needs both.
                if drifted.iter().any(|d| d.drift > 0) && drifted.iter().any(|d| d.drift < 0) {
                    why.push_str(
                        " This batch drifts both ways, so the count above is the sum of two \
                         opposite mistakes and acting on it in either direction makes one of them \
                         worse: each edit is answered below in its own direction.",
                    );
                }
                for d in &drifted {
                    why.push('\n');
                    why.push_str(&drift_report(rel_path, d));
                }
                return Err(why.into());
            }

            let scan_warning = scan_file_content(&working);
            fs::write(&full_path, &working)?;
            let format_note = crate::automation::formatter::format_written_file(&full_path).note();

            let mut message = format!(
                "Success: applied {applied} edit(s) to {rel_path} ({} \u{2192} {} bytes).",
                current.len(),
                bytes_on_disk(&full_path, working.len())
            );
            for note in &rescued {
                message.push(' ');
                message.push_str(note);
            }
            if !noop.is_empty() {
                message.push(' ');
                message.push_str(&noop_edit_note(&noop));
            }
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

    /// The file a tightness case is built against: six statements, so a needle
    /// quoting them can part company at any depth and score differently.
    const SIX_STATEMENTS: &str =
        "    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    save(a);\n}\n";

    #[test]
    fn a_near_miss_one_line_from_its_end_locates_the_site() {
        // `lr7b_v24`'s twelve-line chain in miniature: everything but the line
        // that differs, and the one after it, is standing at a single place.
        let needle =
            "    let a = 1;\n    let b = 2;\n    let c = 3;\n    let WRONG = 9;\n    save(a);\n";
        let miss = nearest_miss(SIX_STATEMENTS, needle).expect("four of five lines are the file's");
        assert_eq!((miss.matched, miss.total), (3, 5), "{miss:?}");
        assert!(
            miss.locates_the_site(),
            "one unexamined line after the divergence is still the site: {miss:?}"
        );
        // And the shape the run actually sent, where the divergence is the last
        // line but one of a longer quote.
        let needle = "    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let WRONG = 9;\n    save(a);\n";
        let miss = nearest_miss(SIX_STATEMENTS, needle).expect("five of six are the file's");
        assert_eq!((miss.matched, miss.total), (4, 6), "{miss:?}");
        assert!(miss.locates_the_site(), "{miss:?}");
    }

    #[test]
    fn a_near_miss_with_a_long_unexamined_tail_does_not_locate_the_site() {
        // Three lines matched and two the scan never got to: the prefix could be
        // anywhere, so this may not contradict a claim about where the site went.
        let needle = "    let a = 1;\n    let b = 2;\n    let c = 3;\n    let WRONG = 9;\n    let e = 5;\n    let f = 6;\n";
        let miss =
            nearest_miss(SIX_STATEMENTS, needle).expect("three of six is half, so it scores");
        assert_eq!((miss.matched, miss.total), (3, 6), "{miss:?}");
        assert!(
            !miss.locates_the_site(),
            "half a needle is not the site, it is a prefix that happened: {miss:?}"
        );
    }

    #[test]
    fn a_two_line_prefix_never_locates_the_site_however_short_the_quote() {
        // The tail rule is satisfied exactly, so what turns this away is the
        // floor on the matched prefix: two lines are in every file.
        let needle = "    let a = 1;\n    let b = 2;\n    let WRONG = 9;\n";
        let miss = nearest_miss(SIX_STATEMENTS, needle).expect("two of three is above half");
        assert_eq!((miss.matched, miss.total), (2, 3), "{miss:?}");
        assert!(!miss.locates_the_site(), "{miss:?}");
    }

    #[test]
    fn a_near_miss_at_the_end_of_the_file_does_not_locate_the_site() {
        // The region does not stand with one line altered here, it stops short —
        // and a write that shortened the file is the history `superseded_site`
        // tells, so the two claims are not in contradiction.
        let hay = "    let a = 1;\n    let b = 2;\n    let c = 3;\n";
        let needle =
            "    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n";
        let miss = nearest_miss(hay, needle).expect("three of five still anchors it");
        assert!(miss.at_eof, "{miss:?}");
        assert_eq!(miss.file_says, "the file ends", "{miss:?}");
        assert!(!miss.locates_the_site(), "{miss:?}");
    }

    /// The file the tail-anchor cases are built against: `lr7b_v26`'s
    /// `create_budget` tail in miniature, where rustfmt broke
    /// `let result_record = self.user_dict` across two lines.
    const SPLIT_HEAD: &str = concat!(
        "        let _ = self.save_data();\n",
        "        let result_record =\n",
        "            self.user_dict\n",
        "                .values_mut()\n",
        "                .next()\n",
        "                .unwrap_or(&mut PersistedUserRecord {\n",
        "                    total_budget: 0.0,\n",
        "                });\n",
        "        self.user_dict\n",
        "            .values_mut()\n",
        "            .next()\n",
        "            .unwrap_or(&mut *result_record)\n",
        "    }\n",
    );

    /// The `oldString` shape the tail anchor is left with: its first line is a
    /// field the model renamed, and every line after it is the file's byte for
    /// byte. v26's own first line was the file's two lines joined, but that shape
    /// is applied by [`super::chain_join_rescue`] before a refusal is ever
    /// drafted, so it never reaches here.
    const WRONG_HEAD: &str = concat!(
        "        let result_record = self.user_map\n",
        "                .values_mut()\n",
        "                .next()\n",
        "                .unwrap_or(&mut PersistedUserRecord {\n",
        "                    total_budget: 0.0,\n",
        "                });\n",
        "        self.user_dict\n",
        "            .values_mut()\n",
        "            .next()\n",
        "            .unwrap_or(&mut *result_record)\n",
        "    }\n",
    );

    #[test]
    fn a_head_no_file_line_matches_is_named_by_the_tail_that_still_stands() {
        // The prefix run scores nothing here — the needle's first line is in no
        // file — which is why the refusal fell all the way through to
        // `explain_refused_edit` and named no line at all.
        assert!(nearest_miss(SPLIT_HEAD, WRONG_HEAD).is_none());

        let a = tail_anchor(SPLIT_HEAD, WRONG_HEAD).expect("ten of eleven lines are the file's");
        assert_eq!(a.line, 4, "the tail starts at the file's own line 4: {a:?}");
        assert_eq!(a.skipped, 1, "{a:?}");
        assert_eq!((a.matched, a.total), (10, 11), "{a:?}");
        assert_eq!(a.wrote, "let result_record = self.user_map", "{a:?}");
        assert_eq!(
            a.file_says,
            vec![
                (2, "let result_record =".to_string()),
                (3, "self.user_dict".to_string())
            ],
            "the head the model got wrong, and none of the match itself: {a:?}"
        );
    }

    #[test]
    fn the_tail_anchor_message_names_a_line_and_not_a_region() {
        let a = tail_anchor(SPLIT_HEAD, WRONG_HEAD).expect("as above");
        let msg = tail_anchor_message("src/port.rs", &a);
        assert!(msg.contains("at line 4"), "{msg}");
        assert!(msg.contains("Read from line 2"), "{msg}");
        assert!(msg.contains("2| let result_record ="), "{msg}");
        assert!(msg.contains("3| self.user_dict"), "{msg}");
        assert!(
            msg.contains("`let result_record = self.user_map`"),
            "the model has to see its own line to know which one to replace: {msg}"
        );
        // v26 read 289-304 three times on the strength of "over that region". A
        // line number is the whole of the fix, so the vague form may not survive
        // in the message that replaced it.
        assert!(!msg.contains("that region"), "{msg}");
    }

    #[test]
    fn the_tail_anchor_looks_past_blank_lines_and_crlf() {
        let hay = "fn a() {\r\n\r\n    let head =\r\n        self.dict\r\n\r\n            .step();\r\n            .then();\r\n            .done();\r\n}\r\n";
        let needle = "    let head = self.dict\n            .step();\n            .then();\n            .done();\n}\n";
        let a = tail_anchor(hay, needle).expect("four of five lines are the file's");
        assert_eq!(a.line, 6, "a blank line is not a line of the run: {a:?}");
        assert_eq!(
            a.file_says,
            vec![(3, "let head =".to_string()), (4, "self.dict".to_string())],
            "numbered as the file numbers them, blanks skipped: {a:?}"
        );
    }

    #[test]
    fn a_tail_that_stands_in_two_places_anchors_nothing() {
        // Two sites name neither. Guessing between them sends the model to the
        // wrong one with total confidence, which reads as an answer.
        let hay = concat!(
            "fn a() {\n",
            "    head_one();\n",
            "    .step()\n",
            "    .then()\n",
            "    .done()\n",
            "}\n",
            "fn b() {\n",
            "    head_two();\n",
            "    .step()\n",
            "    .then()\n",
            "    .done()\n",
            "}\n",
        );
        let needle = "    WRONG();\n    .step()\n    .then()\n    .done()\n";
        let found = tail_anchor(hay, needle);
        assert!(found.is_none(), "{found:?}");
    }

    #[test]
    fn a_tail_too_short_to_be_believed_anchors_nothing() {
        // Three non-blank lines leave at most two standing, and two lines are in
        // every file: not enough to say where the third belonged.
        let hay = "fn a() {\n    let x = 1;\n    let y = 2;\n}\n";
        let needle = "    let WRONG = 9;\n    let x = 1;\n    let y = 2;\n";
        assert!(tail_anchor(hay, needle).is_none());
    }

    #[test]
    fn a_head_too_long_to_have_been_mistaken_anchors_nothing() {
        // Three absent lines above a tail that stands: the model was quoting a
        // different region than the one it landed in, and the two lines the
        // report would quote are not the three it is missing.
        let hay = concat!(
            "fn a() {\n",
            "    let x = 1;\n",
            "    let y = 2;\n",
            "    .step()\n",
            "    .then()\n",
            "    .done()\n",
            "}\n",
        );
        let needle =
            "    one();\n    two();\n    three();\n    .step()\n    .then()\n    .done()\n";
        assert!(tail_anchor(hay, needle).is_none());
    }

    #[test]
    fn a_unique_tail_at_the_top_of_the_file_anchors_nothing() {
        // There is no head room above line 1. Taking the next `skipped` instead
        // would name the file's own first line as one the model invented, and a
        // unique site already settles how much of the head is missing.
        let hay = "    .step()\n    .then()\n    .done()\n}\n";
        let needle = "    WRONG();\n    .step()\n    .then()\n    .done()\n}\n";
        let found = tail_anchor(hay, needle);
        assert!(found.is_none(), "{found:?}");
    }

    #[test]
    fn a_head_that_stands_somewhere_else_gets_no_home_invented() {
        // The needle's first line is in the file at line 2, so the edit was aimed
        // there. Its tail also stands, uniquely, in a different function — and
        // quoting the lines above *that* site as the head's home would send the
        // model to a region it never quoted.
        let hay = concat!(
            "fn a() {\n",
            "    let head = 1;\n",
            "    save(head);\n",
            "}\n",
            "fn b() {\n",
            "    .step()\n",
            "    .then()\n",
            "    .done()\n",
            "}\n",
        );
        let needle = "    let head = 1;\n    WRONG();\n    .step()\n    .then()\n    .done()\n";
        assert!(
            nearest_miss(hay, needle).is_none(),
            "one matched line of five is under the confidence floor"
        );
        let found = tail_anchor(hay, needle);
        assert!(found.is_none(), "{found:?}");
    }

    #[test]
    fn an_indent_rescue_needs_one_site_and_one_shift() {
        let hay = "begin\n    mgr.record(\n        total_cost,\n    );\nend\n";
        let r =
            indent_rescue(hay, "mgr.record(\n    total_cost,\n);").expect("one site, one shift");
        assert_eq!(r.line, 2, "{r:?}");
        assert_eq!(r.extra, "    ", "{r:?}");
        assert!(r.deepen, "{r:?}");
        assert_eq!(
            &hay[r.start..r.end],
            "    mgr.record(\n        total_cost,\n    );",
            "the span is the matched lines and nothing else, so it splices cleanly: {r:?}"
        );

        // Two sites: applying either is a guess.
        let twice = "    mgr.record(\n        total_cost,\n    );\n\
                     other\n    mgr.record(\n        total_cost,\n    );\n";
        assert_eq!(
            indent_rescue(twice, "mgr.record(\n    total_cost,\n);"),
            None
        );

        // One site, but the lines disagree about the shift: that is a
        // restructured block, not the same block re-indented.
        assert_eq!(
            indent_rescue(hay, "    mgr.record(\n      total_cost,\n    );"),
            None
        );

        // Tabs against spaces is not a shift either; one is not a prefix of the
        // other, so there is no amount to move the replacement by.
        let tabbed = "begin\n\t\tmgr.record(\n\t\t\ttotal_cost,\n\t\t);\nend\n";
        assert_eq!(
            indent_rescue(
                tabbed,
                "        mgr.record(\n            total_cost,\n        );"
            ),
            None
        );

        // Trivia is not evidence of intent, however unique it is.
        assert_eq!(indent_rescue(hay, "    )\n    ;"), None);
        assert_eq!(indent_rescue(hay, "\n\n"), None);
        // A needle longer than the file cannot occur in it.
        assert_eq!(indent_rescue("a\n", "a\nb\nc\nd\n"), None);
    }

    #[test]
    fn an_indent_rescue_reports_a_shift_of_zero_when_only_trailing_space_differs() {
        let hay = "begin\n    mgr.record(\n        total_cost,\n    );\nend\n";
        let r = indent_rescue(hay, "    mgr.record(\n        total_cost,\n    );   ")
            .expect("same indentation, trailing space only");
        assert_eq!(r.extra, "", "{r:?}");
        assert_eq!(
            reindent("mgr.record(\n    total_budget,\n);", &r.extra, r.deepen),
            "mgr.record(\n    total_budget,\n);",
            "a zero shift leaves the replacement exactly as sent"
        );
    }

    #[test]
    fn an_indent_rescue_locates_a_span_in_a_crlf_file_by_byte() {
        let hay = "begin\r\n    mgr.record(\r\n        total_cost,\r\n    );\r\nend\r\n";
        let r =
            indent_rescue(hay, "mgr.record(\n    total_cost,\n);").expect("endings are not text");
        assert_eq!(
            &hay[r.start..r.end],
            "    mgr.record(\r\n        total_cost,\r\n    );",
            "terminators stay outside the span so the splice cannot drop or double one: {r:?}"
        );
    }

    #[test]
    fn a_rescued_replacement_moves_by_the_files_indent_and_no_further() {
        assert_eq!(
            reindent("mgr.record(\n    total_budget,\n);", "    ", true),
            "    mgr.record(\n        total_budget,\n    );"
        );
        assert_eq!(
            reindent(
                "    mgr.record(\n        total_budget,\n    );",
                "    ",
                false
            ),
            "mgr.record(\n    total_budget,\n);"
        );
        // A replacement line shallower than the shift cannot go negative.
        assert_eq!(reindent("  mgr.record(", "        ", false), "mgr.record(");
        // Blank lines carry no indentation, so they stay blank rather than
        // collecting a stray run of spaces.
        assert_eq!(reindent("a\n\n   \nb", "  ", true), "  a\n\n\n  b");
        // A trailing newline is the model's, not the file's, and survives.
        assert_eq!(reindent("a\n", "  ", true), "  a\n");
    }

    #[test]
    fn dechain_joins_continuation_lines_and_remembers_their_bytes() {
        let hay = "one\n    .two\n    .three\nfour\n";
        let (text, spans) = super::dechain(hay);
        assert_eq!(text, "one.two.three\nfour");
        assert_eq!(spans, vec![(0, 23), (24, 28)]);
        // The span is the point: it has to name the bytes the joined line stands
        // for, terminators and all, or a splice replaces the wrong region.
        assert_eq!(&hay[spans[0].0..spans[0].1], "one\n    .two\n    .three");
        assert_eq!(&hay[spans[1].0..spans[1].1], "four");
    }

    #[test]
    fn dechain_joins_nothing_when_there_is_nothing_above_the_dot() {
        // A `.` line with a blank line above it is not a continuation, and
        // joining it onto the blank would move the span onto a line the model
        // never quoted.
        let (text, spans) = super::dechain("a\n\n    .b\n");
        assert_eq!(text, "a\n\n    .b");
        assert_eq!(spans, vec![(0, 1), (2, 2), (3, 9)]);
        // Nor is one with no line above it at all.
        let (text, spans) = super::dechain("    .orphan()\nrest\n");
        assert_eq!(text, "    .orphan()\nrest");
        assert_eq!(spans, vec![(0, 13), (14, 18)]);
        assert_eq!(super::dechain("").1, Vec::<(usize, usize)>::new());
    }

    /// `lr7b_v26` and `lr7b_v27` in one fixture: rustfmt broke the binding after
    /// its `=` and broke the chain under it, and the model wrote the whole thing
    /// back on one line. Both breaks have to be undone for that line to be
    /// findable, and the one after `=` has to put back the space it stood for —
    /// plain concatenation gives `let x =self.dict`, which matches nothing.
    #[test]
    fn dechain_joins_a_right_hand_side_the_formatter_broke_off() {
        let hay = "    let x =\n        self.dict\n            .values_mut();\n    rest\n";
        let (text, spans) = super::dechain(hay);
        assert_eq!(text, "    let x = self.dict.values_mut();\n    rest");
        assert_eq!(spans, vec![(0, 56), (57, 65)]);
        assert_eq!(
            &hay[spans[0].0..spans[0].1],
            "    let x =\n        self.dict\n            .values_mut();"
        );
    }

    #[test]
    fn dechain_collapses_the_whitespace_a_break_after_equals_stood_for() {
        let (text, spans) = super::dechain("let x =  \n    y;\n");
        assert_eq!(text, "let x = y;");
        assert_eq!(spans, vec![(0, 16)]);
        // The run keeps going while the lines say it has not ended, so a break
        // after `=` and a break before a `.` in the same binding both go.
        let (text, spans) = super::dechain("    let a =\n        b\n        .c();\n");
        assert_eq!(text, "    let a = b.c();");
        assert_eq!(spans, vec![(0, 35)]);
    }

    #[test]
    fn dechain_does_not_join_through_a_blank_line_below_an_open_binding() {
        // The blank line ends the chain, so the span stays on the line the model
        // quoted rather than reaching past a gap it did not.
        let (text, spans) = super::dechain("    let x =\n\n        y;\n");
        assert_eq!(text, "    let x =\n\n        y;");
        assert_eq!(spans, vec![(0, 11), (12, 12), (13, 23)]);
        // That case is decided by the blank line being a head, which resets the
        // depth and the open binding, so the gap check is not what proves it.
        // A `.` line is the one shape that continues with nothing open above
        // it, and only the gap check stops it reaching across the blank one.
        let (text, spans) = super::dechain("    let x = foo\n\n        .bar();\n");
        assert_eq!(text, "    let x = foo\n\n        .bar();");
        assert_eq!(spans, vec![(0, 15), (16, 16), (17, 32)]);
    }

    #[test]
    fn dechain_treats_a_comment_as_prose_and_not_as_an_open_binding() {
        // A `=` at the end of a comment is inside the comment. Joining the next
        // line onto it would write that line into the comment too, so a rescue
        // would silently delete code while reporting that it applied an edit.
        let (text, spans) = super::dechain("    // default =\n    let y = 1;\n");
        assert_eq!(text, "    // default =\n    let y = 1;");
        assert_eq!(spans, vec![(0, 16), (17, 31)]);
        // The same holds for a comment standing between an open binding and the
        // expression that completes it: the run is broken, not joined through.
        let (text, spans) = super::dechain("    let x =\n    // why\n        y;\n");
        assert_eq!(text, "    let x =\n    // why\n        y;");
        assert_eq!(spans, vec![(0, 11), (12, 22), (23, 33)]);
    }

    /// `lr7b_v28`'s shape in miniature: rustfmt broke a binding after its `=` and
    /// then opened a struct literal it did not close on that line, and the model
    /// wrote all of it back as one line.
    #[test]
    fn dechain_joins_a_struct_literal_the_formatter_broke_open() {
        let hay = "    let r = foo(Bar {\n        a: 1.0,\n    });\n    rest\n";
        let (text, spans) = super::dechain(hay);
        assert_eq!(text, "    let r = foo(Bar { a: 1.0, });\n    rest");
        assert_eq!(spans, vec![(0, 45), (46, 54)]);
        assert_eq!(
            &hay[spans[0].0..spans[0].1],
            "    let r = foo(Bar {\n        a: 1.0,\n    });",
            "the span is the unjoined bytes, so the splice replaces them and not a copy"
        );
    }

    #[test]
    fn dechain_does_not_join_into_a_block_because_a_block_holds_statements() {
        // A brace at the outermost level is a block's, and rustfmt keeps one
        // statement per line inside one. Folding it would put the body on a line
        // the model never quoted and take the match of any needle that copied it
        // faithfully.
        let (text, spans) = super::dechain("fn f() {\n    if c {\n        g();\n    }\n}\n");
        assert_eq!(text, "fn f() {\n    if c {\n        g();\n    }\n}");
        assert_eq!(spans.len(), 5);
        // Nor does a `match` open one, whose arms are statements too.
        let (text, spans) = super::dechain("    let x = match v {\n        1 => a,\n    };\n");
        assert_eq!(text, "    let x = match v {\n        1 => a,\n    };");
        assert_eq!(spans.len(), 3);
        // A struct literal at the outermost level is the one this gives up: it
        // cannot be told from a block without a parser, and folding a block is
        // the worse mistake.
        let (_text, spans) = super::dechain("    let r = Rec {\n        a: 1.0,\n    };\n");
        assert_eq!(spans.len(), 3, "left refused rather than guessed at");
    }

    #[test]
    fn dechain_skips_the_brackets_that_are_text_and_not_code() {
        // Each of these holds a bracket that nothing closes. Counted as code it
        // would leave the depth open and fold every line under it onto one.
        let (_t, spans) = super::dechain("    let s = \"unbalanced (\";\n    next();\n");
        assert_eq!(spans.len(), 2, "a string literal holds what it holds");
        let (_t, spans) = super::dechain("    if c == '(' {\n        x();\n    }\n");
        assert_eq!(
            spans.len(),
            3,
            "and so does a character literal, unlike a lifetime"
        );
        let (_t, spans) = super::dechain("    let a = 1; // (\n    let b = 2;\n");
        assert_eq!(spans.len(), 2, "a line comment runs to the end of the line");
        let (_t, spans) = super::dechain("    foo(a, /* ( */ b);\n    next();\n");
        assert_eq!(spans.len(), 2, "a block comment runs to its closer");
        let (_t, spans) = super::dechain("    /* (\n    still in it\n");
        assert_eq!(
            spans.len(),
            2,
            "and one that outlives the line stops the scan there rather than guessing"
        );
        // An escape inside a string is not the end of it, so the bracket after
        // the escaped quote is still text rather than code.
        let escaped = r#"    let s = "a \" (";
    next();
"#;
        let (_t, spans) = super::dechain(escaped);
        assert_eq!(
            spans.len(),
            2,
            "an escape inside a string is not the end of it"
        );
        // Nor is a lifetime a character literal. Treating one as a literal would
        // swallow the rest of the line and the bracket standing on it, so the
        // lifetime has to be reached at a depth of 0 and the bracket that opens
        // the fold has to come after it.
        let (_t, spans) = super::dechain("    let rows: &'a [&str] = take(\n        1,\n    );\n");
        assert_eq!(
            spans.len(),
            1,
            "so the bracket after a lifetime is code, and opens the fold"
        );
    }

    #[test]
    fn dechain_puts_no_space_where_rust_writes_none() {
        // The break after `=` stood for a space, but the one that only wrapped a
        // bracket did not: `foo( a, );` is in nobody's source.
        let (text, spans) = super::dechain("    foo(\n        a,\n    );\n");
        assert_eq!(text, "    foo(a,);");
        assert_eq!(spans, vec![(0, 26)]);
        let (text, _spans) = super::dechain("    let v = [\n        1,\n        2,\n    ];\n");
        assert_eq!(text, "    let v = [1, 2,];");
        // And the closer has to end the run, or the line under it is swallowed.
        let (text, spans) =
            super::dechain("    let v = [\n        1,\n        2,\n    ];\n    rest\n");
        assert_eq!(text, "    let v = [1, 2,];\n    rest");
        assert_eq!(
            spans.len(),
            2,
            "the `]` closes the run, so what follows is its own line"
        );
    }

    #[test]
    fn dechain_stops_a_run_a_bracket_it_cannot_see_would_never_end() {
        let hay = format!("foo(\n{});\n", "a,\n".repeat(70));
        let (text, spans) = super::dechain(&hay);
        assert_eq!(
            &hay[spans[0].0..spans[0].1].matches('\n').count() + 1,
            60,
            "the fold is capped, so a bracket lost under a spanning block comment \
             costs one region and not the rest of the file"
        );
        assert_eq!(
            spans.len(),
            13,
            "the fold, the eleven lines under the cap, and the closer"
        );
        assert_eq!(text.lines().count(), spans.len());
    }

    /// `lr7b_v28` call 5, byte for byte: the model quoted a twelve-line binding —
    /// the `=` break, the chain under it and the struct literal rustfmt had opened
    /// — as one line. The refusal said the text "appears nowhere in the file" and
    /// told it to `read_file` "over that region" without naming one, so there was
    /// nothing to act on.
    #[test]
    fn a_struct_literal_broken_open_folds_onto_the_one_line_the_model_wrote() {
        let hay = concat!(
            "        let result_record =\n",
            "            self.user_dict\n",
            "                .values_mut()\n",
            "                .next()\n",
            "                .unwrap_or(&mut PersistedUserRecord {\n",
            "                    total_budget: 0.0,\n",
            "                    current_cost: 0.0,\n",
            "                    model_cost: HashMap::new(),\n",
            "                    duration: None,\n",
            "                    created_at: 0.0,\n",
            "                    last_updated_at: 0.0,\n",
            "                });\n",
            "    }\n",
        );
        let needle = "let result_record = self.user_dict.values_mut().next().unwrap_or(&mut PersistedUserRecord { total_budget: 0.0, current_cost: 0.0, model_cost: HashMap::new(), duration: None, created_at: 0.0, last_updated_at: 0.0, });";
        let (joined, spans) = super::dechain(hay);
        assert_eq!(
            joined.lines().count(),
            2,
            "the binding folds to one line and the brace under it stays its own"
        );
        assert_eq!(joined.lines().next().unwrap().trim(), needle);
        assert_eq!(hay[spans[0].0..spans[0].1].lines().count(), 12);
        let r = super::chain_join_rescue(hay, needle)
            .expect("the code is the file's own, only its wraps differ");
        assert_eq!(r.line, 1);
        assert_eq!(
            r.file_lines, 12,
            "and covers every line the formatter broke it into, the struct fields included"
        );
        assert_eq!(
            r.extra, "        ",
            "the needle carries no indent at all, so the file's own eight spaces go back"
        );
        assert!(r.deepen, "{r:?}");
    }

    /// `lr7b_v22`'s file: rustfmt broke the chain, and the model wrote it back
    /// as one line with the struct body four spaces shallower than the file
    /// nests it. The first line keeps the file's own indent while every line
    /// under it does not, so no single delta describes the difference and
    /// [`super::indent_rescue`] declines it.
    const CHAINED_FILE: &str = r#"    pub fn update(&mut self, user: &str, record: Record) {
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

    /// v22's `oldString`, byte for byte: the chain on one line, the body under
    /// it at the nesting the model expected rather than the file's.
    const FLATTENED_CHAIN: &str = r#"        self.user_dict.values_mut().next().unwrap_or(&mut PersistedUserRecord {
            total_budget: 0.0,
            current_cost: 0.0,
        })"#;

    #[test]
    fn a_chain_flattened_onto_one_line_still_names_exactly_one_site() {
        let r = super::chain_join_rescue(CHAINED_FILE, FLATTENED_CHAIN)
            .expect("the code is the file's own, only its wraps differ");
        assert_eq!(r.line, 4, "the match starts where the chain starts");
        assert_eq!(
            r.file_lines, 7,
            "and covers every line the formatter broke it into"
        );
        assert_eq!(
            &CHAINED_FILE[r.start..r.end],
            "        self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut PersistedUserRecord {\n                total_budget: 0.0,\n                current_cost: 0.0,\n            })",
            "the span is the unjoined bytes, so the splice replaces them and not a copy"
        );
        // The first line is at the file's own indent, so the replacement goes in
        // as the model wrote it and rustfmt re-wraps it.
        assert!(
            r.extra.is_empty(),
            "the one shift that is taken is the first line's, and here it is none: {r:?}"
        );
    }

    #[test]
    fn a_chain_join_declines_when_the_code_is_not_the_files() {
        // One character on the last line: the region is nearly right, which is
        // `nearest_miss`'s answer, not something to apply.
        assert_eq!(
            super::chain_join_rescue(CHAINED_FILE, &format!("{FLATTENED_CHAIN};")),
            None
        );
        // Two sites is no site: uniqueness is the whole licence.
        let twice = format!("fn a() {{\n{CHAINED_FILE}\nfn b() {{\n{CHAINED_FILE}");
        assert_eq!(super::chain_join_rescue(&twice, FLATTENED_CHAIN), None);
        // A residue this short is in every file, so uniqueness cannot carry it.
        assert_eq!(
            super::chain_join_rescue("fn f() {\n    a.b\n        .c(x);\n}\n", "a.b.c(x);\n}"),
            None
        );
        assert_eq!(super::chain_join_rescue(CHAINED_FILE, ""), None);
    }

    #[test]
    fn a_chain_join_needs_a_first_line_to_take_its_indent_from() {
        // The one delta it reports comes from the first matched line, so a
        // needle opening on a blank one would take its shift from a line that
        // carries no code — here the file's own whitespace-only line, which
        // would push the whole replacement four spaces right.
        let hay = "fn f() {\n    \n        self.dict\n            .get_mut(k)\n            .unwrap_or(&mut Rec { a: 0.0 });\n}\n";
        let needle = "\n        self.dict.get_mut(k).unwrap_or(&mut Rec { a: 0.0 });";
        assert_eq!(super::chain_join_rescue(hay, needle), None);
    }

    #[test]
    fn a_chain_join_leaves_a_pure_reindent_to_the_indent_rescue() {
        let hay = "fn f() {\n    let total = mgr.record(total_cost);\n    save(total);\n}\n";
        let needle = "  let total = mgr.record(total_cost);\n  save(total);";
        // No break to undo on either side, so this is a shift and nothing else —
        // and the rescue that answers a shift has a stricter rule about it.
        assert_eq!(super::chain_join_rescue(hay, needle), None);
        assert!(super::indent_rescue(hay, needle).is_some());
    }

    #[test]
    fn a_chain_the_model_broke_is_joined_back_too() {
        let hay = "fn f() {\n    self.user_dict.values_mut().next().unwrap_or(&mut Record {\n        total_budget: 0.0,\n    });\n}\n";
        let needle = "    self.user_dict\n        .values_mut()\n        .next()\n        \
                      .unwrap_or(&mut Record {\n        total_budget: 0.0,\n    });";
        let r = super::chain_join_rescue(hay, needle)
            .expect("the mirror image of a flattened chain is the same difference");
        assert_eq!(r.line, 2);
        assert_eq!(r.file_lines, 3);
        assert_eq!(r.extra, "");
    }

    #[test]
    fn a_chain_join_locates_a_span_in_a_crlf_file_by_byte() {
        let hay = CHAINED_FILE.replace('\n', "\r\n");
        let needle = FLATTENED_CHAIN.replace('\n', "\r\n");
        let r = super::chain_join_rescue(&hay, &needle).expect("endings are not text");
        assert_eq!(r.line, 4);
        assert_eq!(r.file_lines, 7);
        assert_eq!(
            &hay[r.start..r.end],
            "        self.user_dict\r\n            .values_mut()\r\n            .next()\r\n            .unwrap_or(&mut PersistedUserRecord {\r\n                total_budget: 0.0,\r\n                current_cost: 0.0,\r\n            })",
            "terminators stay outside the span, so a splice cannot drop or double one"
        );
    }

    /// v30's `oldString`: v22's, byte for byte, with the `;` the model put on
    /// the end of it. The fold that located the one without it locates this one
    /// too; only the terminator stands between them.
    fn flattened_with_a_terminator() -> String {
        format!("{FLATTENED_CHAIN};")
    }

    #[test]
    fn one_terminator_apart_takes_the_terminator_at_the_end_and_only_one() {
        assert!(super::one_terminator_apart("gamma(beta)", "gamma(beta);"));
        assert!(super::one_terminator_apart("gamma(beta);", "gamma(beta)"));
        assert!(!super::one_terminator_apart("gamma(beta)", "gamma(beta)"));
        assert!(
            !super::one_terminator_apart("gamma(beta)", "gamma(beta);;"),
            "two terminators are a different fault"
        );
        assert!(
            !super::one_terminator_apart("gamma(beta)", "gamma(beta); // why"),
            "a comment is not a terminator the file lacks"
        );
        assert!(
            !super::one_terminator_apart("a; b", "a b"),
            "and a `;` in the middle is not this difference"
        );
    }

    #[test]
    fn a_needle_one_terminator_off_a_folded_site_is_located() {
        let needle = flattened_with_a_terminator();
        // The same needle without the `;` is `chain_join_rescue`'s, and it
        // declines this one: the terminator is exactly what it may not move.
        assert!(super::chain_join_rescue(CHAINED_FILE, FLATTENED_CHAIN).is_some());
        assert!(super::chain_join_rescue(CHAINED_FILE, &needle).is_none());

        let m = super::terminator_miss(CHAINED_FILE, &needle).expect("v30's shape");
        assert_eq!(m.line, 4);
        assert_eq!(m.file_lines, 7);
        assert_eq!(
            m.needle_lines, 4,
            "so the refusal can say what the fold undid"
        );
        assert!(!m.file_has_the_terminator, "{m:?}");
        assert_eq!(m.file_tail, "})");
        assert_eq!(m.wrote_tail, "});");
    }

    #[test]
    fn the_terminator_message_names_the_span_and_quotes_both_sides() {
        let needle = flattened_with_a_terminator();
        let m = super::terminator_miss(CHAINED_FILE, &needle).expect("v30's shape");
        let msg = super::terminator_miss_message("port.rs", &m);
        assert!(
            msg.contains("lines 4-10"),
            "the span, and not a region to go hunting for: {msg}"
        );
        assert!(msg.contains("You wrote `});`"), "{msg}");
        assert!(msg.contains("the file says `})`"), "{msg}");
        assert!(
            msg.contains("will neither add one nor drop one"),
            "say why no rescue is offered, or the refusal reads as a shrug: {msg}"
        );
        assert!(
            msg.contains("yours ends with a `;` and the file's does not"),
            "and which side holds it: {msg}"
        );
    }

    #[test]
    fn a_terminator_miss_names_the_side_the_file_has_it_on() {
        let hay = CHAINED_FILE.replace("            })\n", "            });\n");
        assert_ne!(hay, CHAINED_FILE, "the fixture holds the line this changes");
        let m = super::terminator_miss(&hay, FLATTENED_CHAIN).expect("one `;` the other way");
        assert!(m.file_has_the_terminator, "{m:?}");
        assert_eq!(m.file_tail, "});");
        assert_eq!(m.wrote_tail, "})");
        let msg = super::terminator_miss_message("port.rs", &m);
        assert!(msg.contains("the file's line ends with a `;`"), "{msg}");
        assert!(
            !msg.contains("yours ends with a `;` and"),
            "the other direction's wording would be a lie here: {msg}"
        );
    }

    #[test]
    fn an_unfolded_terminator_is_found_by_both_locators_so_the_chain_must_rank_them() {
        // Nothing was folded on either side here, and both answers find it.
        // `nearest_miss` says more -- how many of the needle's lines are already
        // right -- so the chain pushes this one only once that one has
        // declined. That ranking is what this test exists to hold: the pure
        // function has no way to know it lost, and pushed first it would put
        // v19's "6 of its 7 non-blank lines" quiet. Both are computed, since a
        // located terminator is still proof for `located` when the near miss is
        // too loose to be.
        let hay = "fn a() {\n    let alpha = compute_one();\n    let beta = compute_two();\n    gamma(beta)\n}\n";
        let needle =
            "    let alpha = compute_one();\n    let beta = compute_two();\n    gamma(beta);";
        let m = super::nearest_miss(hay, needle).expect("v19's answer");
        assert_eq!(m.matched, 2, "{m:?}");
        let t = super::terminator_miss(hay, needle).expect("the same site, folded or not");
        assert_eq!(t.line, 2);
        assert_eq!(t.file_lines, 3);
        assert_eq!(t.needle_lines, 3);
        let msg = super::terminator_miss_message("port.rs", &t);
        assert!(
            !msg.contains("wraps across"),
            "the two sides wrap alike, so saying so is noise: {msg}"
        );
    }

    #[test]
    fn a_terminator_miss_needs_one_site_and_one_terminator() {
        let needle = flattened_with_a_terminator();
        let twice = format!("{CHAINED_FILE}{FLATTENED_CHAIN}\n");
        assert!(
            super::terminator_miss(&twice, &needle).is_none(),
            "two sites that both differ by a `;` name neither"
        );
        assert!(
            super::terminator_miss(CHAINED_FILE, &format!("{FLATTENED_CHAIN};;")).is_none(),
            "two terminators are not this difference"
        );
        assert!(
            super::terminator_miss(
                "fn a() {\n    take_the_value();\n}\n",
                "    take_the_value()"
            )
            .is_none(),
            "folded text this short turns up anywhere"
        );
    }

    #[test]
    fn a_terminator_miss_also_reads_a_chain_the_model_broke() {
        // The other direction: the file holds the site on one line and the
        // needle broke it, which is the shape `dechain` folds from the other
        // side. Only the needle's fold was undone, so the raw-line counts still
        // differ and the locator still earns its footing.
        let hay = "fn a() {\n    let total = self.records.iter_mut().next().unwrap_or(&mut blank_record)\n}\n";
        let needle = "    let total = self\n        .records\n        .iter_mut()\n        .next()\n        .unwrap_or(&mut blank_record);";
        let m = super::terminator_miss(hay, needle).expect("one line against five");
        assert_eq!(m.line, 2);
        assert_eq!(m.file_lines, 1);
        assert_eq!(m.needle_lines, 5);
        assert!(!m.file_has_the_terminator, "{m:?}");
    }

    #[test]
    fn a_terminator_miss_locates_a_span_in_a_crlf_file_by_line() {
        let hay = CHAINED_FILE.replace('\n', "\r\n");
        let needle = flattened_with_a_terminator().replace('\n', "\r\n");
        let m = super::terminator_miss(&hay, &needle).expect("endings are not text");
        assert_eq!(m.line, 4);
        assert_eq!(m.file_lines, 7);
        assert_eq!(m.file_tail, "})", "no stray carriage return in the quote");
        assert_eq!(m.wrote_tail, "});");
    }

    /// `lr7b_v32`'s file at the E0499, reduced to the region that matters: a
    /// binding whose initializer borrows `self.user_dict` mutably and holds the
    /// borrow, and a tail expression that borrows it again.
    const HOLED_FILE: &str = "\
impl BudgetManager {
    fn update_cost(&mut self, user: &str) -> f64 {
        let record = self.input(user);
        let _ = self.save_data();
        let result_record =
            self.user_dict
                .values_mut()
                .next()
                .unwrap_or(&mut PersistedUserRecord {
                    total_budget: 0.0,
                    current_cost: 0.0,
                });
        self.user_dict
            .values_mut()
            .next()
            .unwrap_or(&mut *result_record)
    }
}
";

    /// v32's `oldString`: the file's lines 6 to 12, then its line 17. Lines 13
    /// to 16 are the hole.
    const HOLED_NEEDLE: &str = "\
            self.user_dict
                .values_mut()
                .next()
                .unwrap_or(&mut PersistedUserRecord {
                    total_budget: 0.0,
                    current_cost: 0.0,
                });
    }";

    /// v32's `newString`: the binding folded onto one line, then every line of
    /// the hole written back and the `}` under it.
    const HOLED_PATCH: &str = "\
        let result_record = self.user_dict.values_mut().next().unwrap_or(&mut PersistedUserRecord {
            total_budget: 0.0,
            current_cost: 0.0,
        });
        self.user_dict
            .values_mut()
            .next()
            .unwrap_or(&mut *result_record)
    }";

    #[test]
    fn a_needle_that_steps_over_the_file_names_the_lines_it_stepped_over() {
        let s = super::skipped_block(HOLED_FILE, HOLED_NEEDLE, HOLED_PATCH)
            .expect("seven of its eight lines are the file's and the eighth is four lines on");
        assert_eq!((s.head_from, s.head_to), (6, 12), "{s:?}");
        assert_eq!((s.head, s.total), (7, 8), "{s:?}");
        assert_eq!(
            s.needle_line, 8,
            "the needle's own line where the head ran out"
        );
        assert_eq!(s.wrote, "}");
        assert_eq!(s.resumes_at, 17, "where that line really stands");
        assert_eq!(s.tail_to, 17);
        assert_eq!(
            s.omitted,
            vec![
                (13, "        self.user_dict".to_string()),
                (14, "            .values_mut()".to_string()),
                (15, "            .next()".to_string()),
                (
                    16,
                    "            .unwrap_or(&mut *result_record)".to_string()
                ),
            ],
            "quoted as the file holds them, indentation and all: the model has to paste these \
             into an `oldString` that is matched byte for byte, and counting whitespace is the \
             one thing it cannot do"
        );
    }

    #[test]
    fn the_hole_is_what_the_prefix_run_reads_as_one_wrong_line() {
        // Both locators fire on this needle, which is why the rank between them
        // is what the model reads. Left to the prefix run it is told "its line 8
        // is where they part — you wrote `}`, the file says `self.user_dict`.
        // Send the same edit with the file's version of that one line", and
        // obeying that builds an `oldString` covering lines 6 to 13 against a
        // `newString` written for 6 to 17 — the splice then deletes 14 to 17.
        let m = super::nearest_miss(HOLED_FILE, HOLED_NEEDLE).expect("the same prefix run");
        assert_eq!((m.line, m.matched, m.total), (6, 7, 8), "{m:?}");
        assert_eq!(m.needle_line, 8);
        assert_eq!(m.file_says, "self.user_dict");
        assert!(
            super::skipped_block(HOLED_FILE, HOLED_NEEDLE, HOLED_PATCH).is_some(),
            "so the hole has to be pushed first, and the integration test says which one wins"
        );
    }

    #[test]
    fn a_needle_whose_last_line_is_nowhere_in_the_file_has_no_hole() {
        // v19's shape, and the one `nearest_miss` was built for: one line
        // genuinely wrong. There is nothing further down for the needle to
        // resume at, so the substitution it prescribes is the fix.
        let mut lines: Vec<&str> = HOLED_NEEDLE.lines().collect();
        lines.pop();
        lines.push("        save(result_record)");
        let needle = lines.join("\n");
        assert!(
            super::nearest_miss(HOLED_FILE, &needle).is_some(),
            "the near miss still reads it as one line off"
        );
        assert_eq!(
            super::skipped_block(HOLED_FILE, &needle, HOLED_PATCH),
            None,
            "a line the file has never held is a mistake, not a hole"
        );
    }

    #[test]
    fn a_hole_the_replacement_does_not_write_back_is_not_named() {
        // The witness is the whole of the claim. Without those lines in the
        // `newString`, widening the span would delete them — and with nothing
        // tying the tail to the head, the two runs the needle quotes could be
        // anywhere in the file relative to each other.
        //
        // The tail here is three lines and resumes in exactly one place, so the
        // witness is the only thing declining it. `HOLED_NEEDLE`'s tail is a
        // bare `}`, which a file this size holds twice, and there the
        // two-resumption guard answers first — the integration test covers that
        // shape and this one has to cover the witness on its own.
        let hay = "\
fn a() {
    let kept = self.take();
    let used = self.give();
    let total = self.sum();
    log(kept);
    save(kept);
    done(used);
}
";
        let needle = "    let kept = self.take();\n    let used = self.give();\n    let total = self.sum();\n    save(kept);\n    done(used);\n}";
        let witnessed = "    log(kept);\n    save(kept);\n    done(used);\n}";
        assert!(
            super::skipped_block(hay, needle, witnessed).is_some(),
            "the same shape with the hole written back is named"
        );
        let unwitnessed = "    save(kept);\n    done(used);\n}";
        assert_eq!(
            super::skipped_block(hay, needle, unwitnessed),
            None,
            "a span the replacement does not carry is a span that widening would delete"
        );
    }

    #[test]
    fn a_needle_the_file_already_carries_whole_is_not_a_hole() {
        // The head run is the whole needle, so no line of it is wrong and there
        // is nothing for a tail to resume at. This shape belongs to the `already
        // on disk` refusal, and it is the `head >= want.len()` clause that keeps
        // the hole out of it: past that clause there is no needle line `head` to
        // quote, and the head ends one line short of the file's end here, which
        // is what leaves the search exactly one place to resume and so reaches
        // the quote rather than declining on the way.
        let hay = "\
fn a() {
    let kept = self.take();
    let used = self.give();
    log(kept);
}
";
        let needle = "    let kept = self.take();\n    let used = self.give();\n    log(kept);";
        let patch = "    log(kept);\n}";
        assert_eq!(
            super::skipped_block(hay, needle, patch),
            None,
            "a needle the file holds whole has no hole in it"
        );
    }

    #[test]
    fn a_two_line_needle_leaves_no_head_to_anchor_a_hole() {
        // One line of head against one line of tail. `head * 2 < want.len()` is
        // `2 < 2` here, so the majority floor waves this through and `MIN_TOTAL`
        // is the only thing declining it — and it has to, because the message
        // would read "1 of its 2 non-blank lines are the file's own, lines 2 to
        // 2", which names a line and not a span. Naming the line that differs is
        // `nearest_miss`'s answer, and it gives the one thing this cannot: the
        // text to put there.
        let hay = "\
fn a() {
    let kept = self.take();
    let used = self.give();
    log(kept);
    save(kept);
    done(used);
}
";
        let needle = "    let kept = self.take();\n    done(used);";
        let patch = "    let used = self.give();\n    log(kept);\n    save(kept);\n    done(used);";
        assert_eq!(
            super::skipped_block(hay, needle, patch),
            None,
            "one matched line is a line, not a head"
        );
    }

    #[test]
    fn a_head_that_stands_in_two_places_names_no_site() {
        // Two heads is two spans, and quoting either names a region the needle
        // never quoted. The two stands sit four lines apart and the tail resumes
        // once beyond the second of them, so this declines on the site count and
        // not on how far the head is from the tail: doubling `HOLED_FILE`'s block
        // instead puts thirteen lines between the first head and the `}`, and
        // the distance cap answers before the count ever gets asked.
        let hay = "\
fn a() {
    let kept = self.take();
    let used = self.give();
    log(kept);
    let kept = self.take();
    let used = self.give();
    mid(kept);
    save(kept);
    done(used);
}
";
        let needle = "    let kept = self.take();\n    let used = self.give();\n    save(kept);\n    done(used);";
        let patch = "    log(kept);\n    let kept = self.take();\n    let used = self.give();\n    mid(kept);\n    save(kept);\n    done(used);";
        assert_eq!(
            super::skipped_block(hay, needle, patch),
            None,
            "the head the needle quoted is one of two, and nothing says which"
        );
    }

    #[test]
    fn a_tail_that_resumes_in_two_witnessed_places_names_neither() {
        let hay = "\
fn a() {
    let kept = self.take();
    let used = self.give();
    log(kept);
    save(kept);
    done(used);
    log(kept);
    save(kept);
    done(used);
}
";
        // The head is lines 2 to 3, and the tail `save(kept); done(used);`
        // resumes at 5 and again at 8, with the `newString` carrying both
        // blocks. Two resumptions is no resumption: naming the first would put
        // a line number on a guess.
        let needle = "    let kept = self.take();\n    let used = self.give();\n    save(kept);\n    done(used);";
        let patch = "    log(kept);\n    save(kept);\n    done(used);\n    log(kept);\n    save(kept);\n    done(used);";
        assert_eq!(super::skipped_block(hay, needle, patch), None);
    }

    #[test]
    fn a_block_too_big_to_be_one_region_is_not_named() {
        let middle: String = (0..13).map(|i| format!("        step_{i}();\n")).collect();
        let hay = format!(
            "fn a() {{\n        let kept = self.take();\n        let used = self.give();\n\
             {middle}        save(kept);\n    }}\n"
        );
        let needle = "        let kept = self.take();\n        let used = self.give();\n        save(kept);\n    }";
        // The `newString` writes every skipped line back, so the cap is the only
        // thing declining this one. Thirteen lines apart, the two runs the
        // needle quotes are two regions and the span between them is not what
        // the edit means.
        let patch = format!("{middle}        save(kept);\n    }}");
        assert_eq!(super::skipped_block(&hay, needle, &patch), None);
    }

    #[test]
    fn a_head_shorter_than_half_the_needle_is_a_coincidence() {
        let hay = "\
fn a() {
    let kept = self.take();
    let used = self.give();
    log(kept);
    save(kept);
    done(used);
    end(kept);
}
";
        // Two of five lines matched, uniquely, with the tail resuming at 5 and
        // the `newString` carrying the block. The same floor `nearest_miss`
        // applies still declines it: a head that short is a prefix the file
        // happens to share, not evidence of which region the needle meant.
        let needle = "    let kept = self.take();\n    let used = self.give();\n    save(kept);\n    done(used);\n    end(kept);";
        let patch = "    log(kept);\n    save(kept);\n    done(used);\n    end(kept);";
        assert_eq!(super::skipped_block(hay, needle, patch), None);
    }

    #[test]
    fn a_hole_whose_tail_is_several_lines_is_still_one_hole() {
        let hay = "\
fn a() {
    let kept = self.take();
    let used = self.give();
    let total = self.sum();
    log(kept);
    save(kept);
    done(used);
}
";
        let needle = "    let kept = self.take();\n    let used = self.give();\n    let total = self.sum();\n    save(kept);\n    done(used);\n}";
        let patch = "    log(kept);\n    save(kept);\n    done(used);\n}";
        let s = super::skipped_block(hay, needle, patch).expect("one line skipped, three resumed");
        assert_eq!((s.head_from, s.head_to), (2, 4), "{s:?}");
        assert_eq!((s.head, s.total, s.needle_line), (3, 6, 4), "{s:?}");
        assert_eq!((s.resumes_at, s.tail_to), (6, 8), "{s:?}");
        assert_eq!(s.omitted, vec![(5, "    log(kept);".to_string())]);
        // And a near miss this loose does not locate the site: three of six
        // lines matched leaves two of the needle unexamined past the divergence,
        // which is more than `NearMiss::locates_the_site` allows. The hole does
        // locate it, so `located` has to count the one and not only the other.
        let m = super::nearest_miss(hay, needle).expect("the same prefix run");
        assert!(!m.locates_the_site(), "{m:?}");
    }

    #[test]
    fn a_hole_is_read_by_line_in_a_crlf_file() {
        let hay = HOLED_FILE.replace('\n', "\r\n");
        let needle = HOLED_NEEDLE.replace('\n', "\r\n");
        let patch = HOLED_PATCH.replace('\n', "\r\n");
        let s = super::skipped_block(&hay, &needle, &patch).expect("endings are not text");
        assert_eq!(
            s,
            super::skipped_block(HOLED_FILE, HOLED_NEEDLE, HOLED_PATCH).unwrap(),
            "same lines, same numbers, same quotes"
        );
        assert!(
            s.omitted.iter().all(|(_, t)| !t.contains('\r')),
            "no stray carriage return in a quote the model has to paste back: {:?}",
            s.omitted
        );
    }

    #[test]
    fn a_hole_spanning_blank_lines_names_the_lines_it_really_skips() {
        let hay = HOLED_FILE.replace(
            "        self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut *result_record)\n",
            "\n        self.user_dict\n\n            .values_mut()\n            .next()\n            .unwrap_or(&mut *result_record)\n",
        );
        assert_ne!(hay, HOLED_FILE, "the fixture holds the lines this splits");
        let s = super::skipped_block(&hay, HOLED_NEEDLE, HOLED_PATCH)
            .expect("a blank line carries no text to match, so it is stepped over like any other");
        assert_eq!(
            s.omitted
                .iter()
                .map(|(line, _)| *line)
                .collect::<Vec<usize>>(),
            vec![14, 16, 17, 18],
            "the file's own numbering, blanks included: {s:?}"
        );
        assert_eq!(s.resumes_at, 19, "{s:?}");
        assert_eq!((s.head_from, s.head_to), (6, 12), "{s:?}");
    }

    #[test]
    fn the_hole_quotes_a_skipped_line_with_its_trailing_space() {
        // The quote is what the model pastes into an `oldString` that is matched
        // byte for byte, so a space the file carries and the message drops is a
        // space the next call gets wrong. Leading whitespace is kept for exactly
        // that reason, and trailing whitespace is kept for the same one — the
        // lines are matched trimmed, so padding them changes nothing but the
        // quote.
        let hay = HOLED_FILE.replace(
            "        self.user_dict\n            .values_mut()\n",
            "        self.user_dict\n            .values_mut()  \n",
        );
        assert_ne!(hay, HOLED_FILE, "the fixture holds the lines this pads");
        let s = super::skipped_block(&hay, HOLED_NEEDLE, HOLED_PATCH).expect("padding is not text");
        assert_eq!(
            s.omitted[1],
            (14, "            .values_mut()  ".to_string()),
            "the file's own bytes, so an `oldString` built from them matches: {s:?}"
        );
    }

    #[test]
    fn the_hole_message_names_the_span_and_forbids_the_one_line_swap() {
        let s = super::skipped_block(HOLED_FILE, HOLED_NEEDLE, HOLED_PATCH).expect("v32's shape");
        let msg = super::skipped_block_message("port.rs", &s);
        assert!(
            msg.contains("hole in it"),
            "the phrase `replay_advice` keys on to give this refusal its own lever: {msg}"
        );
        assert!(msg.contains("7 of its 8 non-blank lines"), "{msg}");
        assert!(msg.contains("lines 6 to 12"), "the head's own span: {msg}");
        assert!(msg.contains("its line 8, `}`"), "{msg}");
        assert!(
            msg.contains("the file's line 17"),
            "where that line really stands, so it is not read as a mistake: {msg}"
        );
        assert!(
            msg.contains("    13|         self.user_dict"),
            "quoted with its number and its own indentation: {msg}"
        );
        assert!(
            msg.contains("    16|             .unwrap_or(&mut *result_record)"),
            "and all of them, not a summary: {msg}"
        );
        assert!(
            msg.contains("lines 6 to 17 and copy that whole run in"),
            "a span to copy, not a region to hunt for: {msg}"
        );
        assert!(
            msg.contains("Do not swap your line 8 for the file's line 13"),
            "the near miss prescribes exactly that swap, and here it deletes lines 14 to 17: {msg}"
        );
        assert!(
            !msg.contains("is where they part"),
            "and it must not carry the near miss's own key, or a replay of this call is answered \
             for the wrong diagnosis: {msg}"
        );
    }

    /// v23 turn 4's `oldString`: the chain as the file wraps it, and the `}`
    /// under it so the site is unique.
    const SITE_AND_BRACE: &str = "        self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut PersistedUserRecord {\n                total_budget: 0.0,\n                current_cost: 0.0,\n            })\n    }";

    /// v23 turn 4's `newString`: the fix, opened by the two lines the file
    /// already carries immediately above the site. Written verbatim, it puts
    /// both in the file twice.
    const PADDED_FIX: &str = "        self.user_dict.insert(user, record);\n        let _ = self.save_data();\n        let r = self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut PersistedUserRecord {\n                total_budget: 0.0,\n                current_cost: 0.0,\n            });\n        save(r)\n    }";

    /// The same fix with no padding, which is what the splice has to leave
    /// behind.
    const THE_FIX: &str = "        let r = self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut PersistedUserRecord {\n                total_budget: 0.0,\n                current_cost: 0.0,\n            });\n        save(r)\n    }";

    /// The one place `needle` occurs in `hay`, as the byte span a splice needs.
    fn site_of(hay: &str, needle: &str) -> (usize, usize) {
        let at = hay.find(needle).expect("a fixture holds its own site");
        (at, at + needle.len())
    }

    #[test]
    fn a_replacement_opening_with_the_files_own_lines_above_the_site_loses_them() {
        let (start, end) = site_of(CHAINED_FILE, SITE_AND_BRACE);
        let super::ContextTrim::Trimmed(c) =
            super::trim_copied_context(CHAINED_FILE, start, end, PADDED_FIX)
        else {
            panic!("the two lines the file has above the site are context, not text to add");
        };
        assert_eq!(c.above, 2, "both of them, and no more: {c:?}");
        assert_eq!(c.below, 0, "the `}}` under the site is the span's own");
        assert_eq!(c.text, THE_FIX, "what is left is the change it meant");

        // The point of dropping them is the file, not the message.
        let (start, end) = site_of(CHAINED_FILE, SITE_AND_BRACE);
        let s = super::splice_trimmed(CHAINED_FILE, start, end, PADDED_FIX.to_string(), 1)
            .expect("the fix is still there to write");
        assert_eq!(
            s.working
                .matches("self.user_dict.insert(user, record);")
                .count(),
            1,
            "v23 wrote it twice and rustc answered with two E0382s: {}",
            s.working
        );
        assert_eq!(s.wrote, THE_FIX, "the brace count reads what was written");
        assert_eq!(
            s.working,
            format!(
                "{}{THE_FIX}{}",
                &CHAINED_FILE[..start],
                &CHAINED_FILE[end..]
            ),
            "{}",
            s.working
        );
    }

    #[test]
    fn a_replacement_closing_with_the_files_own_lines_below_the_site_loses_them() {
        let hay = "fn f() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n}\n";
        let (start, end) = site_of(hay, "    let c = 3;");
        let patch = "    let c = compute();\n    let d = 4;\n}";
        let super::ContextTrim::Trimmed(c) = super::trim_copied_context(hay, start, end, patch)
        else {
            panic!("the two lines under the site are context too");
        };
        assert_eq!((c.above, c.below), (0, 2), "{c:?}");
        assert_eq!(c.text, "    let c = compute();\n");
    }

    #[test]
    fn one_repeated_line_is_the_models_to_write() {
        // A second `push`, a second closing brace: one line repeated beside a
        // site is a pattern a patch legitimately carries, so the floor is two.
        let hay = "fn f() {\n    let a = 1;\n    let b = 2;\n}\n";
        let (start, end) = site_of(hay, "    let b = 2;");
        assert!(matches!(
            super::trim_copied_context(hay, start, end, "    let a = 1;\n    let b = compute();"),
            super::ContextTrim::Keep
        ));
        // Nor is a copy that is not at an edge: the run has to meet the span.
        let hay = "fn f() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n}\n";
        let (start, end) = site_of(hay, "    let c = 3;");
        assert!(matches!(
            super::trim_copied_context(
                hay,
                start,
                end,
                "    let c = compute();\n    let a = 1;\n    let b = 2;"
            ),
            super::ContextTrim::Keep
        ));
    }

    /// `lr7b_v31`'s replay, as the file stood after the first call applied it:
    /// the one-line `oldString` still matches uniquely, and the line its
    /// `newString` adds is already the file's next one.
    const REPLAYED_FILE: &str = "    fn charge(&mut self, user: &str, record: UsageRecord) -> f64 {\n        let result_record = record.clone();\n        let _ = self.save_data();\n        result_record.current_cost\n    }\n";
    const REPLAYED_SITE: &str = "        let result_record = record.clone();";
    const REPLAYED_PATCH: &str =
        "        let result_record = record.clone();\n        let _ = self.save_data();";

    #[test]
    fn a_replayed_insert_one_line_long_is_a_replay_and_not_a_copied_run() {
        let (start, end) = site_of(REPLAYED_FILE, REPLAYED_SITE);
        assert_eq!(
            super::already_written_at_site(REPLAYED_FILE, start, end, REPLAYED_PATCH),
            Some(super::SiteRepeat { line: 2, extra: 1 }),
            "the file carries both lines of the replacement from line 2 down"
        );
        // And the guard below it cannot: one reproduced line is under
        // `trim_copied_context`'s floor of two, which is the hole v31 fell
        // through. See `one_repeated_line_is_the_models_to_write`.
        assert!(
            matches!(
                super::trim_copied_context(REPLAYED_FILE, start, end, REPLAYED_PATCH),
                super::ContextTrim::Keep
            ),
            "the copy here is a single line, so the run floor waves it through"
        );
    }

    #[test]
    fn an_insert_the_file_does_not_yet_carry_is_not_a_replay() {
        let hay = "    fn charge(&mut self, record: UsageRecord) -> f64 {\n        let result_record = record.clone();\n        result_record.current_cost\n    }\n";
        let (start, end) = site_of(hay, "        let result_record = record.clone();");
        assert_eq!(
            super::already_written_at_site(hay, start, end, REPLAYED_PATCH),
            None,
            "the second line is the change, and it is not there yet"
        );
    }

    #[test]
    fn a_replacement_the_same_length_as_its_site_is_a_rewrite_not_a_replay() {
        // A rewrite is where a model legitimately changes indentation, and this
        // comparison is blind to whitespace on purpose, so it has to stay out.
        let hay = "fn f() {\n    let a = 1;\n    let b = 2;\n}\n";
        let (start, end) = site_of(hay, "    let a = 1;");
        assert_eq!(
            super::already_written_at_site(hay, start, end, "        let a = 1;"),
            None,
            "one line over one line is the same length"
        );
        assert_eq!(
            super::already_written_at_site(hay, start, end, "    let a = compute();"),
            None,
            "and a different line of the same count is the ordinary case"
        );
    }

    #[test]
    fn a_site_that_is_a_fragment_of_its_line_is_not_a_replay() {
        // Both of these would match the run below if the whole-line test were
        // lifted, and both would then name a line that does not hold what the
        // refusal says it holds: line 2 of the first file is `let a = 1;`, not
        // the replacement's first line `a = 1;`. A diagnosis that points at the
        // wrong line is worse than one that points at none, because the model
        // reads it and finds something else there.
        let hay = "fn f() {\n    let a = 1;\n    let z = 9;\n}\n";
        let (start, end) = site_of(hay, "a = 1;");
        assert_eq!(
            super::already_written_at_site(hay, start, end, "a = 1;\n    let z = 9;"),
            None,
            "the site begins inside its line, so the file holds more there"
        );
        let hay = "fn f() {\n    let _ = self.save_data();\n}\n";
        let (start, end) = site_of(hay, "let _ = self.save_data");
        assert_eq!(
            super::already_written_at_site(hay, start, end, "let _ = self.save_data();\n}"),
            None,
            "and so does one that ends inside it"
        );
    }

    #[test]
    fn a_replay_names_the_line_the_replacement_starts_on_however_deep() {
        let hay = "use std::collections::HashMap;\n\npub struct M {\n    user_dict: HashMap<String, UsageRecord>,\n}\n\nimpl M {\n    fn charge(&mut self, user: &str) -> f64 {\n        let record = self.user_dict.get(user).unwrap();\n        let result_record = record.clone();\n        let _ = self.save_data();\n        let _ = self.flush();\n        result_record.current_cost\n    }\n}\n";
        let (start, end) = site_of(hay, "        let result_record = record.clone();");
        let patch = "        let result_record = record.clone();\n        let _ = self.save_data();\n        let _ = self.flush();";
        assert_eq!(
            super::already_written_at_site(hay, start, end, patch),
            Some(super::SiteRepeat { line: 10, extra: 2 }),
            "line 10 is where the replacement starts, and two of its lines are the copy"
        );
    }

    #[test]
    fn a_replay_is_read_by_line_in_a_crlf_file() {
        let hay = REPLAYED_FILE.replace('\n', "\r\n");
        let patch = REPLAYED_PATCH.replace('\n', "\r\n");
        let site = REPLAYED_SITE.to_string();
        let (start, end) = site_of(&hay, &site);
        assert_eq!(
            super::already_written_at_site(&hay, start, end, &patch),
            Some(super::SiteRepeat { line: 2, extra: 1 }),
            "a trailing `\\r` is whitespace, and the line count is the same"
        );
    }

    #[test]
    fn a_replay_the_formatter_reindented_is_still_a_replay() {
        let hay = "fn charge(&mut self, record: UsageRecord) -> f64 {\n    let result_record = record.clone();\n    let _ = self.save_data();\n    result_record.current_cost\n}\n";
        // The model replays the eight spaces it wrote the first time; rustfmt
        // took them down to four on the way to disk, and a model cannot count
        // indentation — which is why `indent_rescue` exists. Blindness here is
        // the same fact, and the site is proved, so there is nothing to guess.
        let patch =
            "        let result_record = record.clone();\n        let _ = self.save_data();";
        let (start, end) = site_of(hay, "    let result_record = record.clone();");
        assert_eq!(
            super::already_written_at_site(hay, start, end, patch),
            Some(super::SiteRepeat { line: 2, extra: 1 }),
            "the two sides differ only in leading whitespace"
        );
    }

    #[test]
    fn an_old_string_that_ends_on_a_newline_still_proves_the_replay() {
        let hay = "fn f() {\n    let a = 1;\n    let _ = save();\n    done();\n}\n";
        // The needle carries its own line break, so `end` lands at the start of
        // the line after the site. Without backing off a byte the whole-line test
        // reads that successor — `let _ = save();`, the very line the replacement
        // adds — and calls the site a fragment.
        let (start, end) = site_of(hay, "    let a = 1;\n");
        assert_eq!(
            super::already_written_at_site(hay, start, end, "    let a = 1;\n    let _ = save();"),
            Some(super::SiteRepeat { line: 2, extra: 1 }),
            "the span still covers exactly one whole line"
        );
    }

    #[test]
    fn a_blank_line_ends_a_copied_run_instead_of_joining_it() {
        let hay = "fn f() {\n    let a = 1;\n\n    let b = 2;\n    let c = 3;\n}\n";
        let (start, end) = site_of(hay, "    let c = 3;");
        // The file's two lines above the site are a blank one and `let b = 2;`.
        // Spacing between items is formatting a model may be choosing, so a run
        // that has to cross a blank line is not a copy.
        assert!(matches!(
            super::trim_copied_context(hay, start, end, "\n    let b = 2;\n    let c = compute();"),
            super::ContextTrim::Keep
        ));
    }

    #[test]
    fn a_replacement_of_nothing_but_context_is_refused_rather_than_written() {
        let hay = "fn f() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n}\n";
        let (start, end) = site_of(hay, "    let c = 3;");
        // Everything it says, the file says around the site. Writing it would
        // duplicate two lines and trimming it would delete the matched one, and
        // neither is a change the model asked for.
        let super::ContextTrim::Unwritable {
            dropped,
            everything,
        } = super::trim_copied_context(hay, start, end, "    let a = 1;\n    let b = 2;")
        else {
            panic!("there is nothing in it but the file's own lines");
        };
        assert_eq!((dropped, everything), (2, true));
    }

    #[test]
    fn a_replacement_that_is_the_site_plus_its_context_changes_nothing() {
        let hay = "fn f() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n}\n";
        let (start, end) = site_of(hay, "    let c = 3;");
        let patch = "    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n}";
        let super::ContextTrim::Unwritable {
            dropped,
            everything,
        } = super::trim_copied_context(hay, start, end, patch)
        else {
            panic!("once the four copied lines go, the site is left as it was");
        };
        assert_eq!((dropped, everything), (4, false));
        // The refusal is the byte-identical one's, so a model replaying it gets
        // the same answer it gets for sending its `oldString` back.
        let why = super::splice_trimmed(hay, start, end, patch.to_string(), 1)
            .expect_err("writing it would report a fix and change nothing");
        assert!(
            why.contains("would change no characters"),
            "and it says so in the words the replay advice keys on: {why}"
        );
    }

    #[test]
    fn copied_context_is_dropped_in_a_crlf_file_without_touching_its_endings() {
        let hay = CHAINED_FILE.replace('\n', "\r\n");
        let needle = SITE_AND_BRACE.replace('\n', "\r\n");
        let patch = PADDED_FIX.replace('\n', "\r\n");
        let (start, end) = site_of(&hay, &needle);
        let super::ContextTrim::Trimmed(c) = super::trim_copied_context(&hay, start, end, &patch)
        else {
            panic!("endings are not text, and the copy is the same either way");
        };
        assert_eq!((c.above, c.below), (2, 0), "{c:?}");
        assert_eq!(c.text, THE_FIX.replace('\n', "\r\n"), "{:?}", c.text);
        assert!(
            !c.text.contains("\r\r\n"),
            "a trimmed replacement is spliced into a file whose endings it already carries"
        );
    }

    #[test]
    fn a_replacement_that_dropped_nothing_is_not_reported_as_a_drop() {
        // Nothing was copied, so there is nothing to say: a `Trimmed` with two zero
        // counts would tell the model the tool rewrote its payload when it did
        // not, and the no-op check under it would judge a replacement the guard
        // never touched.
        let hay = "fn f() {\n    let a = 1;\n    let c = 3;\n}\n";
        let (start, end) = site_of(hay, "    let c = 3;");
        assert!(matches!(
            super::trim_copied_context(hay, start, end, "        let c = 3;\n        let d = 4;"),
            super::ContextTrim::Keep
        ));
    }

    #[test]
    fn the_brace_count_reads_the_patch_as_written_not_as_sent() {
        // The two lines this replacement copies from above its site carry a
        // closing brace. Counted as sent, the batch looks like it drops a brace
        // and is refused; counted as written, it is balanced.
        let hay = "fn f() {\n    let a = {\n        1\n    };\n    let c = 3;\n}\n";
        let (start, end) = site_of(hay, "    let c = 3;");
        let sent = "        1\n    };\n    let c = compute();".to_string();
        assert_eq!(
            super::brace_delta(&sent),
            -1,
            "the copied `}}` is the whole reason the sent patch reads as unbalanced"
        );
        let s = super::splice_trimmed(hay, start, end, sent, 1)
            .expect("the change is under the two copied lines");
        assert_eq!(s.wrote, "    let c = compute();");
        assert_eq!(
            super::brace_delta(&s.wrote),
            0,
            "and the file keeps every brace it had: {}",
            s.working
        );
        assert_eq!(
            s.working,
            format!("{}    let c = compute();{}", &hay[..start], &hay[end..])
        );
    }

    /// `lr7b_v29` turns 1 to 5 were four refusals of one edit whose
    /// `newString` closed blocks its `oldString` never covered. Both drift
    /// directions shared one paragraph, it was written for the other one, and
    /// the model obeyed it: a closer added each round, and the count it read
    /// back went 1, 2, 2, 3.
    #[test]
    fn the_brace_drift_advice_names_the_side_of_the_edit_that_is_wrong() {
        let opened = super::brace_drift_advice(2);
        assert!(
            opened.contains("repeat that closing brace"),
            "a dropped closer is the `newString`'s fault: {opened}"
        );
        assert!(
            opened.contains("carrying every delimiter"),
            "and carrying it back is the fix: {opened}"
        );
        let closed = super::brace_drift_advice(-1);
        assert_ne!(opened, closed, "the two directions are not one paragraph");
        assert!(
            !closed.contains("carrying every delimiter"),
            "the advice that raised the count four turns running: {closed}"
        );
        assert!(
            closed.contains("Widen the `oldString`"),
            "the site ends above the lines that close it: {closed}"
        );
        assert!(
            closed.contains("More closers raise this count"),
            "and say what the wrong move does: {closed}"
        );
    }

    /// The shape `lr7b_v35`'s offending edit sat in: an `oldString` that stops
    /// three lines above the closers its `newString` went on to write.
    const DRIFT_FILE: &str = "pub fn f(input: &In) -> Out {\n    let record = if input.flag {\n        let d = match input.kind {\n            K::A => 1,\n            _ => 2,\n        };\n        Rec::new(input.total, d)\n    } else {\n        Rec::new(input.total)\n    };\n    self.dict.insert(input.user, record);\n    self.dict.values_mut().next()\n}\n";

    /// The site is lines 2-4; the file's own closers below it are 6, 8, 10, 13.
    const DRIFT_SITE: &str =
        "    let record = if input.flag {\n        let d = match input.kind {\n            K::A => 1,";

    #[test]
    fn the_position_scan_and_the_count_agree_about_what_is_code() {
        let src = "fn f() { let s = \"}\"; /* } { */ let c = '}'; }";
        let depths = super::brace_depths(src);
        assert_eq!(
            depths.len(),
            2,
            "the string, the block comment and the char literal hold no code brace: {depths:?}"
        );
        assert_eq!(
            depths.last().map(|(_, d)| *d),
            Some(0),
            "and the file is balanced: {depths:?}"
        );
        assert_eq!(
            super::brace_delta(src),
            0,
            "the count is still the last depth the scan reached"
        );
    }

    /// A negative drift means the `newString` closed blocks early, so the lines
    /// that have lost their block are the file's own, below the match. The
    /// refusal used to say "the lines below the site that already close them"
    /// and quote none of them, and v35 answered by re-sending the call, then by
    /// trying a whole-file `write_file`, then by asking a human for the file.
    #[test]
    fn a_negative_drift_quotes_the_files_own_closers_below_the_site() {
        let site = super::drift_site(DRIFT_FILE, DRIFT_SITE, -3).expect("the site is unique");
        assert_eq!((site.from, site.to), (2, 4), "the lines the edit matched");
        assert_eq!(
            site.closers.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec![6, 8, 10],
            "one per closing brace the `newString` got to first"
        );
        assert_eq!(
            site.closers[0].1, "        };",
            "quoted as the file writes it"
        );
        assert_eq!(
            site.closers[2].1, "    };",
            "and the last of them is the widen target"
        );
        assert_eq!(
            site.more, 1,
            "the file has one more closer below, and says so"
        );
    }

    /// A positive drift is the other fault: the `newString` dropped closers its
    /// `oldString` matched, so the lines to quote are inside the match and the
    /// file below it is innocent. A replacement that loses closers loses the ones
    /// at its end, so the *last* of them are quoted -- quoting the first would
    /// name a closer the `newString` did carry and send the model to add it twice.
    #[test]
    fn a_positive_drift_quotes_the_closers_inside_the_match() {
        let matched = "        };\n        Rec::new(input.total, d)\n    } else {\n        \
                       Rec::new(input.total)\n    };";
        let site = super::drift_site(DRIFT_FILE, matched, 2).expect("the site is unique");
        assert_eq!((site.from, site.to), (6, 10));
        assert_eq!(
            site.closers.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec![8, 10],
            "the last two closers the `newString` has to carry back"
        );
        assert_eq!(site.closers[1].1, "    };", "quoted as the file writes it");
        assert!(
            site.closers.iter().all(|(l, _)| (6..=10).contains(l)),
            "and never a line outside the match: {:?}",
            site.closers
        );
        assert_eq!(site.more, 1, "line 6 drops too, and the report says so");
    }

    /// The quote is read through the same scanner as the count, so a brace that
    /// is not code cannot be named as a closer the model has to widen down to.
    /// The noise sits *above* those closers on purpose: a scanner that counted it
    /// would quote it first, and a fixture with the noise below them would read
    /// the same either way and prove nothing.
    #[test]
    fn a_brace_in_a_comment_or_a_string_is_not_quoted_as_a_closer() {
        let hay =
            "fn f() {\n    let a = if c {\n        1\n    // }\n    let s = \"}\";\n    } else \
                   {\n        2\n    };\n}\n";
        let site = super::drift_site(hay, "    let a = if c {\n        1", -2)
            .expect("the site is unique");
        let quoted = site
            .closers
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            site.closers.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec![6, 8],
            "the file's two real closers below the match, not the noise above them: {:?}",
            site.closers
        );
        assert_eq!(quoted, vec!["    } else {", "    };"], "{quoted:?}");
        assert!(
            !quoted.iter().any(|t| t.contains("//") || t.contains('"')),
            "neither the comment's brace nor the string's is a closer: {quoted:?}"
        );
        assert_eq!(site.more, 1, "line 9 closes too, and the report says so");
    }

    /// `} else {` closes a block and opens one on the same line. It is one fact
    /// about one line, and quoting it twice would read as two lines to widen
    /// down to.
    #[test]
    fn a_line_that_closes_and_reopens_is_quoted_once() {
        let site = super::drift_site(DRIFT_FILE, DRIFT_SITE, -4).expect("the site is unique");
        let eights = site.closers.iter().filter(|(l, _)| *l == 8).count();
        assert_eq!(eights, 1, "line 8 drops and reopens: {:?}", site.closers);
        assert_eq!(site.closers.len(), 4, "every closer the file has below");
        assert_eq!(site.more, 0);
    }

    #[test]
    fn drift_evidence_excludes_opening_only_lines() {
        let hay = "fn f(){{0;\n}\n}\nfn g(){\n}\n";
        let site = super::drift_site(hay, "0;", -3).expect("unique site");
        assert_eq!(
            site.closers,
            vec![(2, "}".into()), (3, "}".into()), (5, "}".into())]
        );
        assert_eq!(site.more, 0);
    }

    #[test]
    fn drift_evidence_preserves_lexical_context_across_match_boundaries() {
        for (hay, matched, drift, expected) in [
            ("fn f(){\n0; /* }\n*/\n}\n", "0; /*", -1, 4),
            ("fn f(){\nlet s = \"}\";\n}\n", "let s = \"", -1, 3),
            ("fn f(){\n/* }\n*/\n}\n", "}\n*/\n}", 1, 4),
            ("fn f(){\nlet s = \"}\";\n}\n", "}\";\n}", 1, 3),
            // A drift inside one function has to stay inside it: the closer of a
            // sibling function below the match is not one the `newString` dropped,
            // and quoting it sends the model to widen into code that never
            // belonged to the edit.
            ("fn a(){\n0;\n}\nfn b(){\n}\n", "0;\n}", 1, 3),
        ] {
            let site = super::drift_site(hay, matched, drift).expect("unique site");
            assert_eq!(site.closers, vec![(expected, "}".into())], "{hay}");
            assert_eq!(site.more, 0, "{hay}");
        }
    }

    #[test]
    fn drift_evidence_retains_proven_spans_without_closer_quotes() {
        for (hay, drift, span) in [
            ("fn f(){}", 1, "line 1"),
            ("fn f(){\n}", 1, "lines 1-2"),
            ("fn f(){}", -1, "line 1"),
        ] {
            let d = super::drifted_edit(hay, hay, 1, drift).expect("nonzero drift");
            assert!(d.site.as_ref().unwrap().closers.is_empty());
            let report = super::drift_report("port.rs", &d);
            assert!(
                report.contains(&format!("matched {span} of port.rs")),
                "{report}"
            );
            assert!(!report.contains("These are"), "{report}");
            assert!(!report.contains("[line"), "{report}");
            assert!(report.ends_with(super::brace_drift_advice(drift)));
        }
    }

    #[test]
    fn drift_evidence_quotes_multiple_closers_on_one_line_once() {
        let site = super::drift_site("fn f(){ {\n0;\n}}\n", "0;", -2).expect("unique site");
        assert_eq!(site.closers, vec![(3, "}}".into())]);
        assert_eq!(site.more, 0);
    }

    #[test]
    fn a_drift_span_can_end_in_a_multibyte_character_or_a_newline() {
        let hay = "fn f() {\n    let 名 = 1;\n}\n";
        for matched in ["    let 名", "    let 名 = 1;\n"] {
            let site = super::drift_site(hay, matched, -1).expect("the site is unique");
            assert_eq!((site.from, site.to), (2, 2));
            assert_eq!(site.closers, vec![(3, "}".to_string())]);
        }
    }

    #[test]
    fn the_quote_stops_at_the_cap_and_says_how_many_it_left() {
        let hay = format!("fn f() {{\n{}", "}\n".repeat(10));
        let site = super::drift_site(&hay, "fn f() {", -20).expect("the site is unique");
        assert_eq!(
            site.closers.len(),
            super::MAX_DRIFT_LINES,
            "a refusal is an answer about the file, not a paste of it"
        );
        assert_eq!(site.more, 10 - super::MAX_DRIFT_LINES);
    }

    /// The report prints a line number, so it may only print one it has proved.
    /// A matched text that sits in two places has no single number, and naming
    /// either would send the model to edit the wrong one.
    #[test]
    fn no_line_is_named_when_the_matched_text_sits_in_two_places() {
        let hay = "fn a() {\n    let x = 1;\n}\nfn b() {\n    let x = 1;\n}\n";
        assert!(
            super::drift_site(hay, "    let x = 1;", -1).is_none(),
            "two sites, so no number"
        );
        assert!(
            super::drift_site(hay, "", -1).is_none(),
            "and an empty match has no site at all"
        );
    }

    /// The refusal carries one paragraph per drifting edit, and each paragraph
    /// ends in the advice for *its* direction: a batch that drifts both ways has
    /// no single instruction, and the wrong one is what v29 obeyed four times.
    #[test]
    fn each_directions_report_ends_in_its_own_advice() {
        let opened = super::DriftedEdit {
            n: 1,
            drift: 2,
            site: super::drift_site(
                DRIFT_FILE,
                "        };\n        Rec::new(input.total, d)",
                2,
            ),
        };
        let report = super::drift_report("port.rs", &opened);
        assert!(
            report.starts_with("edit 1: its `oldString` matched lines 6-7"),
            "{report}"
        );
        assert!(report.contains("[line 6]"), "and quotes the file: {report}");
        assert!(report.contains("repeat that closing brace"), "{report}");
        assert!(
            !report.contains("Widen the `oldString`"),
            "the other direction's advice would send it after more closers: {report}"
        );
        assert!(
            report.contains("inside that match"),
            "the lines it quotes are its own, and the prose has to say so: {report}"
        );
        assert!(
            !report.contains("below that match"),
            "pointing below a positive drift sends the model at innocent lines: {report}"
        );

        let closed = super::DriftedEdit {
            n: 2,
            drift: -3,
            site: super::drift_site(DRIFT_FILE, DRIFT_SITE, -3),
        };
        let report = super::drift_report("port.rs", &closed);
        assert!(
            report.starts_with("edit 2: its `oldString` matched lines 2-4"),
            "{report}"
        );
        assert!(
            report.contains("carries 3 more closing brace(s)"),
            "{report}"
        );
        assert!(report.contains("[line 10]     };"), "{report}");
        assert!(report.contains("Widen the `oldString`"), "{report}");
        assert!(!report.contains("repeat that closing brace"), "{report}");
        assert!(
            report.contains("below that match"),
            "and the other way round: {report}"
        );
        assert!(!report.contains("inside that match"), "{report}");
    }

    /// A drifting edit whose site cannot be proved still gets its own direction's
    /// advice -- the count is a fact about the two strings and does not depend on
    /// locating them -- but names no line, because a wrong number is worse than
    /// none.
    #[test]
    fn a_report_without_a_provable_site_names_no_line() {
        let d = super::DriftedEdit {
            n: 3,
            drift: -1,
            site: None,
        };
        let report = super::drift_report("port.rs", &d);
        assert!(report.contains("edit 3:"), "{report}");
        assert!(
            report.contains("carries 1 more closing brace(s)"),
            "{report}"
        );
        assert!(!report.contains("[line"), "nothing to point at: {report}");
        assert!(report.contains("Widen the `oldString`"), "{report}");
    }

    #[test]
    fn the_note_names_every_side_it_dropped() {
        let hay = "fn f() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n}\n";
        let (start, end) = site_of(hay, "    let c = 3;");
        let patch = "    let a = 1;\n    let b = 2;\n    let c = compute();\n    let d = 4;\n    let e = 5;";
        let s = super::splice_trimmed(hay, start, end, patch.to_string(), 2)
            .expect("the change is between the two copies");
        assert_eq!(s.wrote, "    let c = compute();\n");
        let note = s.note.expect("a drop it does not report gets re-sent");
        assert!(
            note.contains(
                "Edit 2 was applied with 2 line(s) the file already has above the text \
                            it replaced and 2 below it"
            ),
            "{note}"
        );
        assert!(
            note.contains("do not re-send it"),
            "the model has to know the payload it sent is not what was written: {note}"
        );
    }

    #[test]
    fn the_byte_count_comes_off_the_disk_and_not_out_of_the_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("port.rs");
        fs::write(&path, "0123456789").unwrap();
        assert_eq!(
            super::bytes_on_disk(&path, 999),
            10,
            "the file is the authority, whatever the caller believes it wrote"
        );
        // A path that cannot be statted falls back to what was sent rather than
        // to zero or a panic: the write already succeeded by the time this runs,
        // and a wrong count is a bad message where a missing one is a tool error.
        assert_eq!(
            super::bytes_on_disk(&dir.path().join("absent.rs"), 42),
            42,
            "the fallback is the count of the bytes that were written"
        );
    }

    /// The note has to name the edit by the number it was sent under, because
    /// that is the only handle the model has on which of its own edits was
    /// dropped — it cannot see the file move under the other one.
    #[test]
    fn the_dropped_edits_are_named_by_the_number_they_were_sent_under() {
        let one = super::noop_edit_note(&[2]);
        assert!(one.contains("edit 2"), "{one}");
        let two = super::noop_edit_note(&[2, 4]);
        assert!(two.contains("edit 2, 4"), "{two}");
        assert!(
            !two.contains("edit 2, 4, "),
            "the list ends at the last number it names: {two}"
        );
    }

    /// One dropped edit and four have to read the same way. A note that agreed
    /// with the count would have to say "1 edit" or "2 edits", and the sentence
    /// that follows it is about each of them separately, so it says "Each such
    /// edit" and the count never enters the grammar.
    #[test]
    fn the_note_reads_the_same_for_one_dropped_edit_as_for_four() {
        for dropped in [&[3][..], &[1, 2, 3, 4][..]] {
            let note = super::noop_edit_note(dropped);
            assert!(
                note.contains("Each such edit"),
                "the sentence after the list is about all of them: {note}"
            );
            assert!(
                note.contains("byte-identical"),
                "and it says what made them alike: {note}"
            );
            assert!(
                note.contains("still has to be written"),
                "dropping an edit is not the same as doing it: {note}"
            );
        }
        // The list is the only part that changes with the count. A sentence that
        // agreed with it -- "the 4 edits were", "this edit was" -- would have to
        // change too, and that is the wording this pins down.
        let after_list = |n: &[usize]| {
            super::noop_edit_note(n)
                .split_once(". Each such edit")
                .map(|(_, rest)| rest.to_string())
                .expect("the sentence after the list is the one that has to be count-free")
        };
        assert_eq!(after_list(&[3]), after_list(&[1, 2, 3, 4]));
    }

    /// A batch of nothing but no-ops is refused in the words a replay keys on.
    /// `replay_advice` in the agent loop branches on "byte-identical", and a
    /// refusal that lost that phrase would fall through to the generic advice and
    /// tell the model to re-read a region it has already quoted exactly.
    #[test]
    fn the_all_no_op_refusal_keeps_the_words_the_replay_advice_keys_on() {
        let why = super::noop_refusal(&[1, 2]);
        assert!(why.contains("byte-identical"), "{why}");
        assert!(why.contains("change no characters"), "{why}");
        assert_eq!(
            why.lines().count(),
            2,
            "each dropped edit gets its own line, as it did when it was refused alone: {why}"
        );
        assert!(why.contains("edit 1:"), "{why}");
        assert!(why.contains("edit 2:"), "{why}");
        assert!(
            why.contains("write the replacement the error asks for"),
            "and the way out of it: {why}"
        );
    }
}
