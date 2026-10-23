//! Optional `rustfmt` pass over files the agent writes.
//!
//! Agent-authored Rust lands exactly as the model typed it, and models do not
//! type to rustfmt's line widths: the LiteLLM crucible produced a crate that
//! compiled and passed its tests but still needed a manual `cargo fmt` before
//! it was readable. Formatting on write closes that gap with no extra build
//! step, and — because a file rustfmt cannot parse is reported back to the
//! model — it doubles as a free syntax check on every Rust file produced.
//!
//! The scope is exactly one file: the one the agent just wrote. Sibling
//! modules in the same crate are left alone, and so are the file's own line
//! endings. Both fall out of how the formatter is invoked; see
//! [`format_written_file`].
//!
//! It is deliberately non-fatal. The write already succeeded by the time this
//! runs, so a missing formatter, an unparseable file or a failed subprocess
//! must never turn a good write into a tool error; the worst case is that the
//! file stays exactly as the model wrote it.
//!
//! Not the same thing as `VelocityApp::format_buffer_for_save`, and the two
//! deliberately do not overlap: that hook is LSP-driven, runs only on an
//! interactive save inside the egui app and is opt-in (off by default), so an
//! agent writing through `write_file` never reaches it. This is the headless
//! half of the same idea — a `rustfmt` subprocess on files the agent produces,
//! where there is no editor, no language server and nobody to press Ctrl+S.

use std::path::Path;
use std::sync::OnceLock;

/// Edition rustfmt is invoked with when no `Cargo.toml` declares one. Modern
/// rather than rustfmt's own 2015 default, which cannot parse `async fn` or
/// `dyn Trait` and would report a syntax error on most agent-written code.
const DEFAULT_EDITION: &str = "2021";

/// What the format pass did with a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatOutcome {
    /// Not a file this pass formats — anything but `.rs`.
    NotApplicable,
    /// Switched off with `VELOCITY_FORMAT_ON_WRITE=off`.
    Disabled,
    /// No runnable `rustfmt` on PATH.
    NoFormatter,
    /// rustfmt rewrote the file.
    Formatted,
    /// rustfmt accepted the file and changed nothing.
    Unchanged,
    /// rustfmt ran and did not succeed. In practice this is a parse error,
    /// which is worth reporting: it means the file the model just wrote is not
    /// valid Rust, and the bytes on disk are untouched.
    Failed(String),
}

impl FormatOutcome {
    /// The sentence appended to the `write_file` result.
    ///
    /// `None` when there is nothing for the model to act on. A clean format, a
    /// non-Rust file, a disabled pass and a missing formatter are all silent:
    /// a note on every write is noise the model learns to skip, which would
    /// also bury the one note that matters (a parse failure).
    pub fn note(&self) -> Option<String> {
        match self {
            FormatOutcome::Formatted => Some(
                "Note: rustfmt reformatted this file, so its bytes differ from what you sent."
                    .to_string(),
            ),
            FormatOutcome::Failed(detail) => Some(format!(
                "WARNING: rustfmt rejected this file ({detail}). It was left on disk exactly as \
                 written, but that normally means a syntax error \u{2014} re-read and fix it \
                 before building."
            )),
            _ => None,
        }
    }
}

/// Whether the pass runs at all. On by default; the usual off spellings are
/// accepted so an operator can disable it without a rebuild.
pub fn format_on_write_enabled() -> bool {
    match std::env::var("VELOCITY_FORMAT_ON_WRITE") {
        Ok(raw) => !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no" | "disabled"
        ),
        Err(_) => true,
    }
}

/// Whether this pass formats the file. Extension-only: a `.rs` file is Rust
/// regardless of where in the workspace it lives, and anything else has no
/// formatter wired up here.
pub fn is_formattable(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.eq_ignore_ascii_case("rs"))
        .unwrap_or(false)
}

/// Whether `rustfmt` can be run, probed once per process.
///
/// Cached because `write_file` fires dozens of times in a mission, and spawning
/// a process just to rediscover "still not installed" would cost more than the
/// formatting it decides whether to do.
fn rustfmt_available() -> bool {
    static PROBE: OnceLock<bool> = OnceLock::new();
    *PROBE.get_or_init(|| {
        std::process::Command::new("rustfmt")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

/// The edition declared by one `Cargo.toml` body, or `None` when it says
/// nothing usable — including when it defers to the workspace with
/// `edition.workspace = true`, which is an inheritance marker and not an
/// edition.
fn edition_in_manifest(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        // A commented-out edition line is not a declaration.
        if trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key != "edition" && key != "edition.workspace" {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'').trim();
        // Inherited or empty: keep looking, in the next table or the next
        // manifest up the tree.
        if key == "edition.workspace" || value.is_empty() || value == "true" {
            continue;
        }
        return Some(value.to_string());
    }
    None
}

/// The edition to format `path` with: the nearest `Cargo.toml` that declares
/// one, walking up, else [`DEFAULT_EDITION`].
///
/// Worth the walk because formatting a 2015 or 2018 crate as 2021 (or the
/// reverse) can produce a diff that has nothing to do with layout.
pub fn edition_for(path: &Path) -> String {
    for dir in path.ancestors() {
        if let Ok(body) = std::fs::read_to_string(dir.join("Cargo.toml")) {
            if let Some(edition) = edition_in_manifest(&body) {
                return edition;
            }
        }
    }
    DEFAULT_EDITION.to_string()
}

/// Where rustfmt should look for its config: the nearest ancestor holding a
/// `rustfmt.toml`, mirroring the upward search rustfmt does on its own when
/// given a file path.
///
/// That search is keyed off the input path, and this pass formats from stdin
/// (see [`format_written_file`]), so without this the project's own config is
/// silently ignored and the result disagrees with `cargo fmt` on the same
/// file.
pub fn config_dir_for(path: &Path) -> Option<std::path::PathBuf> {
    path.ancestors()
        .find(|dir| dir.join("rustfmt.toml").is_file() || dir.join(".rustfmt.toml").is_file())
        .map(std::path::PathBuf::from)
}

/// Whether `bytes` is a CRLF file: it has CRLF endings and no bare LF.
///
/// A file mixing both is treated as LF, which is what rustfmt's own `Auto`
/// detection does, so the two cannot disagree about the same input.
fn uses_crlf(bytes: &[u8]) -> bool {
    let (mut crlf, mut bare_lf, mut prev) = (0usize, 0usize, 0u8);
    for &byte in bytes {
        if byte == b'\n' {
            if prev == b'\r' {
                crlf += 1;
            } else {
                bare_lf += 1;
            }
        }
        prev = byte;
    }
    crlf > 0 && bare_lf == 0
}

/// Put the file's own line endings back on the formatted text.
///
/// rustfmt reads the newline style from the file it is given; fed from stdin
/// it has no file and always emits LF. On a CRLF checkout that turns a write
/// which reformatted three lines into a diff where every line changed, so the
/// input's convention wins.
fn restore_newline_style(formatted: &[u8], original: &[u8]) -> Vec<u8> {
    if !uses_crlf(original) {
        return formatted.to_vec();
    }
    let mut out = Vec::with_capacity(formatted.len() + 64);
    for &byte in formatted {
        if byte == b'\n' && out.last() != Some(&b'\r') {
            out.push(b'\r');
        }
        out.push(byte);
    }
    out
}

/// Format a file the agent has just written, in place.
///
/// Never returns an error, and the file on disk is only ever replaced by text
/// rustfmt actually produced — a parse failure, an empty result or a failed
/// write all leave the original bytes where they were.
pub fn format_written_file(path: &Path) -> FormatOutcome {
    if !is_formattable(path) {
        return FormatOutcome::NotApplicable;
    }
    if !format_on_write_enabled() {
        return FormatOutcome::Disabled;
    }
    if !rustfmt_available() {
        return FormatOutcome::NoFormatter;
    }

    let before = std::fs::read(path).unwrap_or_default();
    if before.is_empty() {
        // Nothing to lay out, and an empty file is not a syntax error worth
        // interrupting the model over.
        return FormatOutcome::Unchanged;
    }

    let mut command = std::process::Command::new("rustfmt");
    command
        .arg("--edition")
        .arg(edition_for(path))
        // Formatting from stdin instead of passing the path is deliberate.
        // Given a file, rustfmt walks its `mod` declarations and rewrites
        // every child module it can resolve, so an agent appending one
        // `pub mod` line to `lib.rs` would reformat the entire crate —
        // including files this write never mentioned and pre-existing code
        // the model has no reason to expect changed — while the tool result
        // reported that "this file" was reformatted.
        .arg("--emit")
        .arg("stdout")
        // Without this rustfmt prints "Skipped N files" chatter that would end
        // up in a tool result.
        .arg("--quiet")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(dir) = config_dir_for(path) {
        command.arg("--config-path").arg(dir);
    }

    let Ok(mut child) = command.spawn() else {
        // Probe said yes, spawn said no (PATH changed, permissions). Formatting
        // is optional; the write is already done.
        return FormatOutcome::NoFormatter;
    };

    // The input goes in from another thread: if rustfmt fills the stdout pipe
    // while this one is still blocked writing a large file, both ends wait on
    // each other forever. A broken pipe is not an error here — it just means
    // rustfmt gave up early, which the exit status reports properly.
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let input = before.clone();
    std::thread::spawn(move || {
        use std::io::Write;
        let _ = stdin.write_all(&input);
        let _ = stdin.flush();
    });

    let Ok(output) = child.wait_with_output() else {
        return FormatOutcome::Failed("the formatter could not be waited on".to_string());
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_default();
        let detail = detail.chars().take(200).collect::<String>();
        return FormatOutcome::Failed(if detail.is_empty() {
            format!("exit status {}", output.status)
        } else {
            detail
        });
    }

    if output.stdout.is_empty() {
        // Success with no text would mean replacing a real source file with an
        // empty one. Trust nothing that comes back that cheaply.
        return FormatOutcome::Failed(
            "the formatter returned no text, so the file was left as written".to_string(),
        );
    }

    let formatted = restore_newline_style(&output.stdout, &before);
    if formatted == before {
        return FormatOutcome::Unchanged;
    }
    match std::fs::write(path, &formatted) {
        Ok(()) => FormatOutcome::Formatted,
        Err(err) => FormatOutcome::Failed(format!(
            "the formatted text could not be written back ({err}), so the file was left as written"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_rust_files_are_formattable() {
        assert!(is_formattable(Path::new("src/lib.rs")));
        // Windows paths are what the agent actually writes through.
        assert!(is_formattable(Path::new(r"C:\ws\src\main.rs")));
        assert!(!is_formattable(Path::new("Cargo.toml")));
        assert!(!is_formattable(Path::new("notes.md")));
        assert!(!is_formattable(Path::new("script.py")));
        // No extension at all must not panic or be treated as Rust.
        assert!(!is_formattable(Path::new("LICENSE")));
    }

    #[test]
    fn the_edition_comes_from_the_manifest_not_from_a_guess() {
        assert_eq!(
            edition_in_manifest("[package]\nname = \"x\"\nedition = \"2018\"\n"),
            Some("2018".to_string())
        );
        assert_eq!(
            edition_in_manifest("[package]\nedition='2015'\n"),
            Some("2015".to_string())
        );
        // A commented-out line is not a declaration.
        assert_eq!(
            edition_in_manifest("[package]\n# edition = \"2015\"\nedition = \"2021\"\n"),
            Some("2021".to_string())
        );
        // Inheritance from `[workspace.package]` is a marker, not an edition:
        // reporting "true" would hand rustfmt a bogus `--edition`.
        assert_eq!(
            edition_in_manifest("[package]\nedition.workspace = true\n"),
            None
        );
        assert_eq!(edition_in_manifest("[package]\nname = \"x\"\n"), None);
        // Unrelated keys that merely contain the word must not match.
        assert_eq!(edition_in_manifest("[deps]\npre-edition = \"1\"\n"), None);
    }

    #[test]
    fn a_workspace_inheriting_crate_resolves_to_the_root_edition() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace.package]\nedition = \"2018\"\n",
        )
        .unwrap();
        let member = tmp.path().join("crates").join("inner");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::write(
            member.join("Cargo.toml"),
            "[package]\nedition.workspace = true\n",
        )
        .unwrap();
        let file = member.join("src").join("lib.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "fn main() {}\n").unwrap();

        assert_eq!(edition_for(&file), "2018");
    }

    #[test]
    fn a_file_outside_any_crate_gets_the_modern_default() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("loose.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        // rustfmt's own default is 2015, which cannot parse most modern Rust.
        assert_eq!(edition_for(&file), DEFAULT_EDITION);
    }

    /// Silence is the default: only the two outcomes the model can act on
    /// produce a note, so a note always means something.
    #[test]
    fn only_actionable_outcomes_speak_up() {
        assert_eq!(FormatOutcome::NotApplicable.note(), None);
        assert_eq!(FormatOutcome::Disabled.note(), None);
        assert_eq!(FormatOutcome::NoFormatter.note(), None);
        assert_eq!(FormatOutcome::Unchanged.note(), None);

        let formatted = FormatOutcome::Formatted
            .note()
            .expect("formatted is reported");
        assert!(formatted.contains("rustfmt"), "{formatted}");

        let failed = FormatOutcome::Failed("expected one of `(` or `::`".to_string())
            .note()
            .expect("a parse failure is reported");
        assert!(failed.contains("WARNING"), "{failed}");
        // The real diagnostic travels with the warning, or it is useless.
        assert!(failed.contains("expected one of"), "{failed}");
    }

    #[test]
    fn non_rust_files_are_left_completely_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("notes.md");
        std::fs::write(&file, "#   unformatted   heading\n").unwrap();
        assert_eq!(format_written_file(&file), FormatOutcome::NotApplicable);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "#   unformatted   heading\n"
        );
    }

    /// End-to-end through the real formatter. Skipped, not failed, when the
    /// toolchain has no rustfmt: the pass is optional by design.
    #[test]
    fn badly_formatted_rust_is_reformatted_in_place() {
        if !rustfmt_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("lib.rs");
        std::fs::write(&file, "pub fn  add(a:u8,b:u8)->u8{a+b}\n").unwrap();

        let outcome = format_written_file(&file);
        assert_eq!(outcome, FormatOutcome::Formatted);
        let formatted = std::fs::read_to_string(&file).unwrap();
        assert!(
            formatted.contains("pub fn add(a: u8, b: u8) -> u8 {"),
            "{formatted}"
        );

        // A second pass has nothing left to do, which is what keeps the note
        // off files that were already tidy.
        assert_eq!(format_written_file(&file), FormatOutcome::Unchanged);
    }

    /// The parse-error path is the reason this pass is worth its cost: it tells
    /// the model the Rust it just wrote does not compile, without failing the
    /// write that already succeeded.
    #[test]
    fn unparseable_rust_is_reported_and_left_untouched() {
        if !rustfmt_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("broken.rs");
        let broken = "pub fn main( {\n";
        std::fs::write(&file, broken).unwrap();

        match format_written_file(&file) {
            FormatOutcome::Failed(detail) => assert!(!detail.is_empty()),
            other => panic!("expected a parse failure, got {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&file).unwrap(), broken);
    }

    /// Formatting is scoped to the file that was written. Handing rustfmt the
    /// path instead of the contents makes it walk `mod` declarations and
    /// rewrite every child module it can resolve, so one `write_file` to a
    /// crate root would silently reformat the whole crate — including
    /// pre-existing code the model never asked to touch — while the tool
    /// result claimed only that file changed.
    #[test]
    fn formatting_a_crate_root_leaves_its_child_modules_alone() {
        if !rustfmt_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"p\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let root = src.join("lib.rs");
        let child = src.join("child.rs");
        std::fs::write(&root, "pub mod child;\npub fn  root_fn()->u8{1}\n").unwrap();
        let child_before = "pub fn  child_fn()->u8{2}\n";
        std::fs::write(&child, child_before).unwrap();

        assert_eq!(format_written_file(&root), FormatOutcome::Formatted);
        assert!(
            std::fs::read_to_string(&root)
                .unwrap()
                .contains("pub fn root_fn() -> u8 {"),
            "the file that was written is the one that gets formatted"
        );
        assert_eq!(
            std::fs::read_to_string(&child).unwrap(),
            child_before,
            "a sibling module is not this write's business"
        );
    }

    /// rustfmt fed from stdin has no file to read the newline style from and
    /// always emits LF, so the pass puts the original convention back. Without
    /// that, a CRLF checkout turns every formatted write into a whole-file
    /// diff.
    #[test]
    fn a_crlf_file_keeps_its_line_endings() {
        if !rustfmt_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("lib.rs");
        std::fs::write(&file, "pub fn  add(a:u8)->u8{\r\n    a\r\n}\r\n").unwrap();

        assert_eq!(format_written_file(&file), FormatOutcome::Formatted);
        let bytes = std::fs::read(&file).unwrap();
        assert!(uses_crlf(&bytes), "line endings survived: {bytes:?}");
        assert!(
            !bytes.windows(3).any(|w| w == b"\r\r\n"),
            "carriage returns must not be doubled: {bytes:?}"
        );
    }

    /// The project's `rustfmt.toml` still applies. Config discovery is keyed
    /// off the input path, which stdin mode does not give rustfmt, so the pass
    /// locates it and passes it explicitly.
    #[test]
    fn the_project_rustfmt_config_is_honoured() {
        if !rustfmt_available() {
            return;
        }
        // Unformatted, and long enough that a narrow `max_width` has to break
        // the signature while the default 100 columns keeps it on one line.
        // It must not already be tidy, or the assertion below proves nothing.
        let body = "pub fn compute(aaa:u64,bbb:u64,ccc:u64,ddd:u64)->u64{aaa+bbb+ccc+ddd}\n";
        let manifest = "[package]\nname = \"p\"\nedition = \"2021\"\n";

        let plain = tempfile::tempdir().unwrap();
        std::fs::write(plain.path().join("Cargo.toml"), manifest).unwrap();
        std::fs::create_dir_all(plain.path().join("src")).unwrap();
        let plain_file = plain.path().join("src").join("lib.rs");
        std::fs::write(&plain_file, body).unwrap();

        let configured = tempfile::tempdir().unwrap();
        std::fs::write(configured.path().join("Cargo.toml"), manifest).unwrap();
        std::fs::write(configured.path().join("rustfmt.toml"), "max_width = 40\n").unwrap();
        std::fs::create_dir_all(configured.path().join("src")).unwrap();
        let configured_file = configured.path().join("src").join("lib.rs");
        std::fs::write(&configured_file, body).unwrap();

        assert_eq!(
            config_dir_for(&configured_file).as_deref(),
            Some(configured.path())
        );
        assert_eq!(config_dir_for(&plain_file), None);

        assert_eq!(format_written_file(&plain_file), FormatOutcome::Formatted);
        assert_eq!(
            format_written_file(&configured_file),
            FormatOutcome::Formatted
        );
        assert_ne!(
            std::fs::read(&plain_file).unwrap(),
            std::fs::read(&configured_file).unwrap(),
            "max_width = 40 has to reach the formatter"
        );
    }

    #[test]
    fn newline_style_detection_only_calls_a_file_crlf_when_it_is() {
        assert!(uses_crlf(b"a\r\nb\r\n"));
        assert!(!uses_crlf(b"a\nb\n"), "plain LF");
        assert!(!uses_crlf(b"a\r\nb\n"), "mixed is treated as LF");
        assert!(!uses_crlf(b"no line endings at all"));
        assert!(!uses_crlf(b""));
    }

    #[test]
    fn restoring_crlf_is_a_nothing_to_do_for_an_lf_file() {
        let formatted = b"fn main() {}\n".to_vec();
        assert_eq!(restore_newline_style(&formatted, b"other\n"), formatted);
        assert_eq!(
            restore_newline_style(&formatted, b"other\r\n"),
            b"fn main() {}\r\n".to_vec()
        );
    }
}
