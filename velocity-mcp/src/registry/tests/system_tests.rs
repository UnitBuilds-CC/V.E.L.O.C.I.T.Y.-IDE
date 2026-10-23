use crate::registry::call_tool_in_workspace;
use serde_json::json;
use std::fs;

fn setup_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    (temp, root)
}

#[test]
fn file_tools_use_explicit_workspace_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    call_tool_in_workspace(
        &root,
        "write_file",
        &json!({"relativeFilePath": "src/main.rs", "content": "fn main() {}"}),
    )
    .unwrap();

    // `.rs` files are passed through rustfmt on write, which normalises the
    // trailing newline — and, for a file with no line endings at all, the
    // platform's line ending. This test is about *where* the file landed, so
    // compare trimmed rather than pinning bytes the formatter owns.
    assert_eq!(
        fs::read_to_string(root.join("src/main.rs")).unwrap().trim(),
        "fn main() {}"
    );

    // Anything that is not Rust is written byte-for-byte, formatter or not.
    call_tool_in_workspace(
        &root,
        "write_file",
        &json!({"relativeFilePath": "notes.md", "content": "#   Title"}),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(root.join("notes.md")).unwrap(),
        "#   Title"
    );
}

/// The formatter pass has to be visible to the model. If a Rust file comes back
/// reformatted and the tool result says nothing, the model keeps working from
/// the bytes it sent and its next `read_file` disagrees with its own memory.
#[test]
fn writing_unformatted_rust_reports_the_formatter_pass() {
    let (_temp, root) = setup_root();
    let result = call_tool_in_workspace(
        &root,
        "write_file",
        &json!({"relativeFilePath": "src/lib.rs", "content": "pub fn  add(a:u8,b:u8)->u8{a+b}"}),
    )
    .unwrap();

    let on_disk = fs::read_to_string(root.join("src/lib.rs")).unwrap();
    if on_disk.contains("pub fn add(a: u8, b: u8) -> u8 {") {
        assert!(
            result.contains("rustfmt"),
            "a reformatted file must be reported to the model: {result}"
        );
    } else {
        // No rustfmt on this machine: the write is untouched *and* silent, so
        // the model is never told about a formatting pass that did not happen.
        assert_eq!(on_disk, "pub fn  add(a:u8,b:u8)->u8{a+b}");
        assert!(!result.contains("rustfmt"), "{result}");
    }
    assert!(result.starts_with("Success:"), "{result}");
}

/// A file rustfmt cannot parse is reported but the write still succeeds: the
/// bytes on disk are what the model asked for, and "this is not valid Rust" is
/// information worth having, not a tool failure.
#[test]
fn writing_unparseable_rust_warns_without_failing_the_write() {
    let (_temp, root) = setup_root();
    let broken = "pub fn main( {";
    let result = call_tool_in_workspace(
        &root,
        "write_file",
        &json!({"relativeFilePath": "src/broken.rs", "content": broken}),
    )
    .expect("an unparseable file is not a failed write");

    assert_eq!(
        fs::read_to_string(root.join("src/broken.rs")).unwrap(),
        broken,
        "a rejected file must be left exactly as written"
    );
    assert!(result.starts_with("Success:"), "{result}");
    if result.contains("WARNING") {
        assert!(result.contains("rustfmt"), "{result}");
    }
}

/// A write of a 32 KB port is the biggest single thing a model can do to a
/// workspace, and `convert22` showed it cannot be trusted to know where it
/// landed: the task named 'rust_port/src/budget_manager.rs' and the model sent
/// 'litellm_ws/rust_port/src/budget_manager.rs', a legal relative path that
/// quietly created a second copy of the workspace inside itself. The tool said
/// only "File written successfully", so nothing in the conversation ever
/// displayed the wrong path.
#[test]
fn writing_a_file_names_the_path_it_was_given() {
    let (_temp, root) = setup_root();
    let result = call_tool_in_workspace(
        &root,
        "write_file",
        &json!({"relativeFilePath": "litellm_ws/rust_port/src/budget_manager.rs", "content": "fn a() {}\n"}),
    )
    .unwrap();

    assert!(result.starts_with("Success:"), "{result}");
    assert!(
        result.contains("litellm_ws/rust_port/src/budget_manager.rs"),
        "the model must see the path a write landed at: {result}"
    );
    assert!(root
        .join("litellm_ws/rust_port/src/budget_manager.rs")
        .exists());
}

#[test]
fn file_tools_reject_parent_traversal() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let result = call_tool_in_workspace(
        &root,
        "write_file",
        &json!({"relativeFilePath": "../outside.txt", "content": "nope"}),
    );

    assert!(result.is_err());
    assert!(!temp.path().join("outside.txt").exists());
}

#[test]
fn command_runs_in_explicit_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    // `pwd` works cross-shell: PowerShell aliases it to `Get-Location`
    // (whose ToString renders the full path), and `sh -c pwd` prints the
    // working directory on Unix. The previous version used `cd`, which is
    // a no-op with no stdout in PowerShell, so the assertion never fired
    // on Windows.
    let output = call_tool_in_workspace(&root, "run_command", &json!({"command": "pwd"})).unwrap();
    assert!(
        output.to_lowercase().contains("project"),
        "expected current-directory output to include the workspace name, got: {output}"
    );
}

#[test]
fn file_tools_delete_file_success() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    fs::write(root.join("temp.txt"), "hello").unwrap();
    assert!(root.join("temp.txt").exists());

    call_tool_in_workspace(
        &root,
        "delete_file",
        &json!({"relativeFilePath": "temp.txt"}),
    )
    .unwrap();

    assert!(!root.join("temp.txt").exists());
}

#[test]
fn file_tools_delete_file_rejects_parent_traversal() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let result = call_tool_in_workspace(
        &root,
        "delete_file",
        &json!({"relativeFilePath": "../outside.txt"}),
    );

    assert!(result.is_err());
}

#[test]
fn list_dir_returns_error_on_missing_dir() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let result = call_tool_in_workspace(
        &root,
        "list_dir",
        &json!({"relativeDirPath": "missing_folder"}),
    );

    assert!(result.is_err());
}

#[test]
fn code_coverage_analyze_reports_gaps_and_scaffolds() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/math.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\npub fn sub(a: i32, b: i32) -> i32 { a - b }\n",
    )
    .unwrap();

    let out = call_tool_in_workspace(&root, "code_coverage_analyze", &json!({})).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    // Both functions are discovered and neither has a test yet.
    assert_eq!(v["totalFunctions"].as_u64().unwrap(), 2);
    assert_eq!(v["testedFunctions"].as_u64().unwrap(), 0);
    assert_eq!(v["untestedCount"].as_u64().unwrap(), 2);
    assert!(v["skeletonCount"].as_u64().unwrap() >= 1);
    assert!(v["summary"].as_str().unwrap().contains("Coverage"));
}

#[test]
fn code_coverage_analyze_is_advertised_in_definitions() {
    let tools = crate::registry::get_tools();
    assert!(
        tools.iter().any(|t| t.name == "code_coverage_analyze"),
        "missing tool definition for code_coverage_analyze"
    );
}

#[test]
fn fetch_panel_data_lists_files_and_is_advertised() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("README.md"), "hello").unwrap();

    let output =
        call_tool_in_workspace(&root, "fetch_panel_data", &json!({"panel": "files"})).unwrap();
    let data: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(data["panel"], "files");
    assert!(data["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["name"] == "README.md"));

    let tools = crate::registry::get_tools();
    assert!(tools.iter().any(|tool| tool.name == "fetch_panel_data"));
}

#[test]
fn fetch_panel_data_rejects_unknown_panel() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    assert!(
        call_tool_in_workspace(&root, "fetch_panel_data", &json!({"panel": "run_build"}),).is_err()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — read_file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn read_file_returns_file_content() {
    let (_temp, root) = setup_root();
    fs::write(root.join("hello.txt"), "hello world").unwrap();

    let output = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "hello.txt"}),
    )
    .unwrap();

    assert!(output.contains("hello world"));
}

#[test]
fn read_file_errors_on_missing_file() {
    let (_temp, root) = setup_root();

    let result = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "nonexistent.txt"}),
    );

    assert!(result.is_err());
}

/// A model repairing one line of a 25 KB port currently has to ask for the
/// whole file, and the context budget elides most of what comes back — so the
/// verbatim `oldString` `edit_file` demands is unobtainable and the only move
/// left is a full rewrite the generation limit truncates. Ranges are what make
/// targeted repair reachable.
#[test]
fn read_file_returns_a_named_line_range_when_asked() {
    let (_temp, root) = setup_root();
    let mut body = String::new();
    for i in 1..=300 {
        body.push_str(&format!("let line_{i} = {i};\n"));
    }
    fs::write(root.join("big.rs"), body).unwrap();

    let output = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "big.rs", "startLine": 149, "endLine": 152}),
    )
    .unwrap();

    assert!(
        output.contains("Lines 149-152 of 300 in `big.rs`"),
        "the header must name the range and the total: {output}"
    );
    assert!(output.contains("let line_149 = 149;"), "{output}");
    assert!(output.contains("let line_152 = 152;"), "{output}");
    assert!(
        !output.contains("let line_148"),
        "the window starts where it was told to: {output}"
    );
    assert!(
        !output.contains("let line_153"),
        "and ends where it was told to: {output}"
    );
}

#[test]
fn read_file_without_a_range_stays_the_bare_file() {
    // The dispatch layer appends a Decision Trail trailer to every read, and
    // consumers account for it. What must NOT appear is a range header: models
    // and tests that call read_file today have to keep getting the file itself.
    let (_temp, root) = setup_root();
    fs::write(root.join("bare.txt"), "alpha\nbeta\n").unwrap();

    let output =
        call_tool_in_workspace(&root, "read_file", &json!({"relativeFilePath": "bare.txt"}))
            .unwrap();
    assert!(
        output.starts_with("alpha\nbeta\n"),
        "un-ranged reads must begin with the file: {output}"
    );
    assert!(
        !output.contains("Lines 1-"),
        "no header may be invented for a read that asked for none: {output}"
    );
}

#[test]
fn read_file_caps_an_oversized_window_and_shows_the_way_on() {
    let (_temp, root) = setup_root();
    let mut body = String::new();
    for i in 1..=1000 {
        body.push_str(&format!("l{i}\n"));
    }
    fs::write(root.join("huge.rs"), body).unwrap();

    let output = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "huge.rs", "startLine": 1, "endLine": 10_000}),
    )
    .unwrap();

    assert!(
        !output.contains("l201\n"),
        "the cap has to bind or the elision problem returns:\n{}",
        &output[..output.len().min(300)]
    );
    assert!(
        output.contains("send `startLine` 201"),
        "a clamped window must say how to continue: {}",
        &output[..output.len().min(300)]
    );
}

/// The character cap is the one that actually protects a repair: history
/// compression rewrites any tool result over 4,000 characters once an assistant
/// message follows it, so a window that clears the line cap but not this one is
/// elided before the model can copy from it — which is the exact dead end
/// `startLine` exists to remove.
#[test]
fn a_ranged_read_stays_small_enough_to_survive_context_compression() {
    let (_temp, root) = setup_root();
    // 600 lines of realistic indented Rust: ~75 chars each, so the line cap
    // alone would return ~15 KB.
    let line = "        let model_cost = model_costs.get(model).copied().unwrap_or(0.0);";
    let body = vec![line; 600].join("\n");
    fs::write(root.join("dense.rs"), body).unwrap();

    let output = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "dense.rs", "startLine": 10, "endLine": 600}),
    )
    .unwrap();

    assert!(
        output.len() < 4_000,
        "a ranged read must fit under the 4,000-char compression threshold, got {}",
        output.len()
    );
    // It still has to be a real window, not an empty apology.
    assert!(output.contains("Lines 10-"), "{output}");
    assert!(
        output.contains(line),
        "the window must carry the lines: {output}"
    );
    assert!(
        output.contains("send `startLine`"),
        "a char-capped window must tell the model where to continue: {output}"
    );
}

#[test]
fn read_file_refuses_a_range_it_cannot_satisfy_instead_of_eyeballing_it() {
    let (_temp, root) = setup_root();
    fs::write(root.join("short.txt"), "one\ntwo\nthree\n").unwrap();

    // Past the end: the reply has to carry the real line count, because the
    // model's only alternative is to guess again.
    let past = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "short.txt", "startLine": 90}),
    );
    let msg = past
        .err()
        .map(|e| e.to_string())
        .unwrap_or_else(|| "must fail".to_string());
    assert!(
        msg.contains("3 line") && msg.contains("90"),
        "must state the true length: {msg}"
    );

    // Reversed range.
    let reversed = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "short.txt", "startLine": 3, "endLine": 1}),
    );
    assert!(
        reversed.is_err(),
        "an inverted window must not return a file"
    );

    // Zero is the classic off-by-one, and 1-based here.
    let zero = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "short.txt", "startLine": 0}),
    );
    assert!(zero.is_err(), "`startLine` 0 is not addressable");

    // A non-integer must be refused, NOT ignored: ignoring it would hand back
    // the whole file the model was trying to avoid requesting.
    let junk = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "short.txt", "startLine": "first"}),
    );
    let msg = junk
        .err()
        .map(|e| e.to_string())
        .unwrap_or_else(|| "must fail".to_string());
    assert!(msg.contains("startLine"), "must name the bad field: {msg}");
}

/// The whole point of the range: the bytes it returns are usable as an
/// `edit_file` needle. A header the model copies, or a body whose indentation
/// differs from the file, makes the feature decorative.
#[test]
fn a_ranged_read_feeds_an_edit_file_call_that_lands() {
    let (_temp, root) = setup_root();
    let mut body = String::new();
    for i in 1..=250 {
        if i == 120 {
            body.push_str("        let entry = self.user_dict.entry(k);\n");
        } else {
            body.push_str(&format!("        filler_{i}();\n"));
        }
    }
    fs::write(root.join("port.rs"), &body).unwrap();

    let read = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "port.rs", "startLine": 120, "endLine": 120}),
    )
    .unwrap();
    // The range header opens the body and the harness's own "End of file
    // content" marker closes it; between them is the file, untouched.
    let body = read
        .split('\n')
        .skip(1)
        .take_while(|l| !l.starts_with("── End of file content"))
        .collect::<Vec<_>>()
        .join("\n");
    let needle = body.trim_end();
    assert_eq!(needle, "        let entry = self.user_dict.entry(k);");

    let edited = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": needle,
                "newString": "        let entry = self.user_dict.entry(k.to_string());"
            }],
            "context": "fix the key type"
        }),
    )
    .unwrap();
    assert!(edited.contains("Success"), "{edited}");
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert!(after.contains("entry(k.to_string())"), "{after}");
}

/// A refused edit that only says "copy verbatim" sends the model back to the
/// same guess of the indentation, and one refusal voids the whole batch. The
/// file's own line has to be in the message for the next call to differ.
#[test]
fn a_refused_edit_shows_the_files_own_indentation_of_the_line_it_missed() {
    let (_temp, root) = setup_root();
    fs::write(
        root.join("port.rs"),
        "fn f() {\n        let entry = self.user_dict.entry(k);\n        drop(entry);\n}\n",
    )
    .unwrap();

    // Over-indenting is the failure a model actually hits: a needle with fewer
    // leading spaces than the file is still a substring of the real line and
    // edits fine, while one with more matches nothing and reads as "not in the
    // file" even though the text is sitting right there.
    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "                let entry = self.user_dict.entry(k);",
                "newString": "                let entry = self.user_dict.entry(k.to_string());"
            }],
            "context": "fix the key type"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "the mis-indented edit must be refused".to_string());

    assert!(
        err.contains("        let entry = self.user_dict.entry(k);"),
        "the refusal must hand back the file's own line, indentation and all: {err}"
    );
    assert!(
        err.contains("different leading whitespace"),
        "and name the failure the model can actually fix: {err}"
    );
    // The pair that follows is what turns a matched line into a UNIQUE one.
    assert!(
        err.contains("        drop(entry);"),
        "the next line belongs in the window so the widened edit lands: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        "fn f() {\n        let entry = self.user_dict.entry(k);\n        drop(entry);\n}\n",
        "a refused batch leaves the file alone"
    );
}

/// The other half: text that is not in the file at all. Pasting back a random
/// near-match would be worse than useless, so the refusal has to say the
/// content differs and point at the one tool that can settle it.
#[test]
fn a_refused_edit_for_text_the_file_never_had_says_so_and_offers_a_ranged_read() {
    let (_temp, root) = setup_root();
    fs::write(root.join("port.rs"), "fn f() {\n    keep_me();\n}\n").unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    remove_me();",
                "newString": "    keep_forever();"
            }],
            "context": "delete a call"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "an absent line must be refused".to_string());

    assert!(
        err.contains("appears nowhere in the file"),
        "the refusal must distinguish content from indentation: {err}"
    );
    assert!(
        err.contains("startLine") && err.contains("endLine"),
        "and name the read that can find the real text: {err}"
    );
    assert!(
        !err.contains("keep_me();"),
        "it must not dress an unrelated line up as the match: {err}"
    );
}

/// A first line of `});` matches half the file; naming its sites would send the
/// model after the wrong one, so a probe too short to identify anything is left
/// unexplained rather than guessed at.
#[test]
fn a_refused_edit_with_a_probe_too_generic_to_locate_stays_silent_about_sites() {
    let (_temp, root) = setup_root();
    fs::write(root.join("port.rs"), "fn a() {\n    x();\n});\n}\n").unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{ "oldString": "  });\n  z();", "newString": "  });\n  y();" }],
            "context": "widen"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "must be refused".to_string());

    assert!(
        !err.contains("IS in"),
        "a two-character probe is not evidence about a site: {err}"
    );
}

/// Three call sites that open identically — same method, same first argument —
/// are the case where pasting back "that line and the one after it" produces
/// three copies of the same block and tells the model nothing. The window has to
/// widen until the sites differ, and say so when even then they do not.
#[test]
fn a_refused_edit_widens_identical_sites_until_they_differ_and_numbers_them() {
    let (_temp, root) = setup_root();
    fs::write(
        root.join("port.rs"),
        "fn f() {\n    mgr.create_budget(\n        100.0,\n        \"alpha\",\n    );\n    \
         mgr.create_budget(\n        100.0,\n        \"beta\",\n    );\n}\n",
    )
    .unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "        mgr.create_budget(\n            100.0,",
                "newString": "        mgr.create_budget(\n            250.0,"
            }],
            "context": "raise the floor"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a mis-indented batch must be refused".to_string());

    // The run of identical two-line openings was the bug: both sites had to be
    // shown, so the third line is what separates them.
    assert!(err.contains("\"alpha\""), "site 1 window: {err}");
    assert!(err.contains("\"beta\""), "site 2 window: {err}");
    assert!(
        err.contains("[line 2]") && err.contains("[line 6]"),
        "numbered so the model can read the region it wants: {err}"
    );
    assert!(
        err.contains("exact indentation. Its first line"),
        "the two halves of the refusal must stay two sentences: {err}"
    );
    assert!(
        !err.contains("still identical"),
        "widening worked, so the ambiguity note does not belong: {err}"
    );
}

/// The honest limit: a repeated block that never differs within the window can
/// not be made unique from what the refusal shows, and saying "copy verbatim"
/// again would send the model back to the same doomed guess.
#[test]
fn a_refused_edit_says_so_when_no_widening_inside_the_window_separates_the_sites() {
    let (_temp, root) = setup_root();
    fs::write(
        root.join("port.rs"),
        "fn a() {\n    mgr.create_budget(\n        100.0,\n    );\n}\nfn b() {\n    \
         mgr.create_budget(\n        100.0,\n    );\n}\n",
    )
    .unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "        mgr.create_budget(\n            100.0,",
                "newString": "        mgr.create_budget(\n            250.0,"
            }],
            "context": "raise the floor"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "an ambiguous batch must be refused".to_string());

    assert!(
        err.contains("still identical"),
        "the refusal must admit its own window cannot disambiguate: {err}"
    );
    assert!(
        err.contains("read_file"),
        "and name the tool that can widen further: {err}"
    );
}

/// An edit whose `newString` equals its `oldString` matches, applies and
/// changes nothing — and a green "applied 1 edit(s) (23886 -> 23886 bytes)"
/// tells a repair turn it fixed the error it was just shown.
#[test]
fn an_edit_that_would_write_back_the_text_it_matched_is_refused_as_a_no_op() {
    let (_temp, root) = setup_root();
    let src = "fn f() {\n    let x = 1;\n}\n";
    fs::write(root.join("port.rs"), src).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    let x = 1;",
                "newString": "    let x = 1;"
            }],
            "context": "signal an intent with no patch"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a no-op edit must be refused".to_string());

    assert!(
        err.contains("byte-identical"),
        "the refusal must name the mistake the model can see: {err}"
    );
    assert!(
        err.contains("change no characters"),
        "and say why it is not progress: {err}"
    );
    assert_eq!(fs::read_to_string(root.join("port.rs")).unwrap(), src);

    // A deliberate deletion is the opposite case and must stay allowed:
    // `newString: ""` removes text, which is a real change.
    let deleted = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{ "oldString": "    let x = 1;\n", "newString": "" }],
            "context": "delete a line"
        }),
    )
    .unwrap();
    assert!(deleted.contains("Success"), "{deleted}");
    // `write_file`/`edit_file` run rustfmt on `.rs`, which collapses the now
    // empty block onto one line — the deletion is the change, the shape is his.
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        "fn f() {}\n"
    );
}

/// Gateprobe2 spent three turns on this refusal. The model sent a one-line
/// `oldString` that matched two call sites, got back "widen it with the
/// surrounding lines until it is unique" — a sentence naming no lines — and
/// rebuilt the context from memory of a file it had emitted a dozen minutes
/// earlier. The lines that separate the sites are sitting in the file, so the
/// refusal has to show them instead of asking for them.
#[test]
fn an_ambiguous_edit_shows_the_files_own_text_around_each_site_it_matched() {
    let (_temp, root) = setup_root();
    fs::write(
        root.join("port.rs"),
        "fn a() {\n    let cache = HashMap::new();\n    fill_a();\n}\n\nfn b() {\n    \
         let cache = HashMap::new();\n    fill_b();\n}\n",
    )
    .unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    let cache = HashMap::new();",
                "newString": "    let cache: HashMap<String, u32> = HashMap::new();"
            }],
            "context": "annotate the map type"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "the two-site edit must be refused".to_string());

    assert!(err.contains("matches 2 places"), "got: {err}");
    assert!(
        err.contains("widen until only the site you mean still matches"),
        "and say what to do with the blocks: {err}"
    );
    assert!(
        err.contains("never the marker"),
        "a `[line N]` marker looks like file content unless it is disclaimed: {err}"
    );
    // The distinguishing neighbour is the whole point: without it the widened
    // edit is still a guess.
    assert!(
        err.contains("[line 1]\nfn a() {\n    let cache = HashMap::new();\n    fill_a();"),
        "site one must be verbatim file text with its own neighbour:\n{err}"
    );
    assert!(
        err.contains("fill_b();"),
        "site two must be shown too, or the model edits the wrong one blind:\n{err}"
    );
    assert!(
        !err.contains("appears nowhere"),
        "this needle IS in the file; that refusal would send it back to read_file:\n{err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        "fn a() {\n    let cache = HashMap::new();\n    fill_a();\n}\n\nfn b() {\n    \
         let cache = HashMap::new();\n    fill_b();\n}\n",
        "a refused batch leaves the file alone"
    );
}

/// More sites than the message can show: a count that silently covered up the
/// rest would let the model widen to a unique-looking block that still matches
/// an unshown site and think the edit landed everywhere.
#[test]
fn an_ambiguous_edit_says_how_many_sites_it_hid_when_the_list_is_cut_short() {
    let (_temp, root) = setup_root();
    let src = "fn main() {\n".to_string() + &"    reset();\n".repeat(5) + "}\n";
    fs::write(root.join("port.rs"), &src).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{ "oldString": "    reset();", "newString": "    reset_all();" }],
            "context": "rename the reset call"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a five-site edit must be refused".to_string());

    assert!(err.contains("matches 5 place(s)"), "got: {err}");
    assert!(
        err.contains("the first 3 are shown"),
        "a truncated list must admit it: {err}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — write_file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn write_file_creates_parent_directories() {
    let (_temp, root) = setup_root();

    call_tool_in_workspace(
        &root,
        "write_file",
        &json!({
            "relativeFilePath": "deep/nested/dir/file.txt",
            "content": "nested content"
        }),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(root.join("deep/nested/dir/file.txt")).unwrap(),
        "nested content"
    );
}

#[test]
fn write_file_overwrites_existing() {
    let (_temp, root) = setup_root();
    fs::write(root.join("existing.txt"), "old content").unwrap();

    call_tool_in_workspace(
        &root,
        "write_file",
        &json!({
            "relativeFilePath": "existing.txt",
            "content": "new content"
        }),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(root.join("existing.txt")).unwrap(),
        "new content"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — edit_file
//
// edit_file exists because a compile-repair turn otherwise has to re-emit the
// whole file. A 30 KB port cannot fit the model's output budget, so the call is
// cut off mid-stream and the repair silently makes zero progress. These tests
// lock the two properties that make targeted edits safe: uniqueness (a vague
// `oldString` must not guess) and atomicity (a half-applied batch must not
// leave the file in a state the model has never seen).
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edit_file_applies_a_unique_replacement() {
    let (_temp, root) = setup_root();
    fs::write(root.join("lib.txt"), "line one\nline two\n").unwrap();

    let out = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.txt",
            "edits": [{ "oldString": "line two", "newString": "line 2, corrected" }]
        }),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(root.join("lib.txt")).unwrap(),
        "line one\nline 2, corrected\n"
    );
    assert!(out.contains("applied 1 edit(s)"), "message: {out}");
}

#[test]
fn edit_file_applies_the_batch_in_order() {
    let (_temp, root) = setup_root();
    fs::write(root.join("lib.txt"), "alpha\n").unwrap();

    // The second oldString only exists once the first edit has landed, so a
    // batch validated against the pre-edit file would reject a correct call.
    call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.txt",
            "edits": [
                { "oldString": "alpha", "newString": "beta" },
                { "oldString": "beta", "newString": "gamma" }
            ]
        }),
    )
    .unwrap();

    assert_eq!(fs::read_to_string(root.join("lib.txt")).unwrap(), "gamma\n");
}

#[test]
fn edit_file_rejects_an_ambiguous_oldstring_and_writes_nothing() {
    let (_temp, root) = setup_root();
    let original = "count += 1;\ncount += 1;\n";
    fs::write(root.join("lib.txt"), original).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.txt",
            "edits": [{ "oldString": "count += 1;", "newString": "count += 2;" }]
        }),
    )
    .expect_err("a two-place match must be refused, not guessed");
    let msg = err.to_string();

    assert!(msg.contains("2 places"), "must report the hit count: {msg}");
    assert_eq!(fs::read_to_string(root.join("lib.txt")).unwrap(), original);
}

#[test]
fn edit_file_is_atomic_when_one_edit_of_a_batch_is_wrong() {
    let (_temp, root) = setup_root();
    let original = "fn a() {}\nfn b() {}\n";
    fs::write(root.join("lib.txt"), original).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.txt",
            "edits": [
                { "oldString": "fn a() {}", "newString": "fn a() { /* ok */ }" },
                { "oldString": "fn zzz() {}", "newString": "fn b() {}" }
            ]
        }),
    )
    .expect_err("an edit absent from the file must refuse the whole batch");

    // The good edit must not survive on its own: the model re-tries against the
    // file it already knows, so a half-applied batch corrupts its next guess.
    let msg = err.to_string();
    assert!(
        msg.contains("edit 2") && msg.contains("not in lib.txt") && msg.contains("unchanged"),
        "refusal must name the bad edit: {msg}"
    );
    assert_eq!(fs::read_to_string(root.join("lib.txt")).unwrap(), original);
}

#[test]
fn edit_file_requires_newstring_instead_of_silently_deleting() {
    let (_temp, root) = setup_root();
    let original = "keep this line\ndrop this line\n";
    fs::write(root.join("lib.txt"), original).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.txt",
            "edits": [{ "oldString": "drop this line" }]
        }),
    )
    .expect_err("an omitted newString must not be read as a deletion");
    let msg = err.to_string();

    assert!(
        msg.contains("newString"),
        "must name the missing key: {msg}"
    );
    assert_eq!(fs::read_to_string(root.join("lib.txt")).unwrap(), original);
}

#[test]
fn edit_file_deletes_when_newstring_is_explicitly_empty() {
    let (_temp, root) = setup_root();
    fs::write(root.join("lib.txt"), "keep\ndrop\n").unwrap();

    call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.txt",
            "edits": [{ "oldString": "drop\n", "newString": "" }]
        }),
    )
    .unwrap();

    assert_eq!(fs::read_to_string(root.join("lib.txt")).unwrap(), "keep\n");
}

#[test]
fn edit_file_matches_a_crlf_oldstring_and_keeps_the_endings() {
    let (_temp, root) = setup_root();
    // The invisible-bytes trap: a Windows checkout renders as plain newlines in
    // the model's context, so the text it copies back carries CRLF while the
    // agent-written file it is patching uses LF. Refusing that patch would make
    // edit_file worse than the whole-file rewrite it exists to replace.
    fs::write(root.join("lib.txt"), "one\ntwo\n").unwrap();

    call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.txt",
            "edits": [{ "oldString": "one\r\ntwo", "newString": "1\r\n2" }]
        }),
    )
    .unwrap();

    // The LF file keeps LF endings rather than adopting the call's CRLF.
    assert_eq!(fs::read_to_string(root.join("lib.txt")).unwrap(), "1\n2\n");
}

#[test]
fn edit_file_applies_an_lf_oldstring_to_a_crlf_file_without_mixing_endings() {
    let (_temp, root) = setup_root();
    fs::write(root.join("lib.txt"), "line one\r\nline two\r\n").unwrap();

    call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.txt",
            "edits": [{ "oldString": "line one\nline two", "newString": "line 1\nline 2" }]
        }),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(root.join("lib.txt")).unwrap(),
        "line 1\r\nline 2\r\n"
    );
}

#[test]
fn edit_file_never_doubles_the_carriage_return_on_a_crlf_file() {
    let (_temp, root) = setup_root();
    // Both the file and the patch are CRLF — the shape a Windows checkout
    // produces when the model copies `read_file` output verbatim. Re-ending the
    // replacement without first downgrading it to LF turned every pair into
    // `\r\r\n`, which rustfmt cannot parse and no editor displays honestly.
    fs::write(root.join("lib.txt"), "alpha\r\nbeta\r\n").unwrap();

    call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.txt",
            "edits": [{ "oldString": "alpha\r\nbeta", "newString": "gamma\r\ndelta" }]
        }),
    )
    .unwrap();

    let written = fs::read_to_string(root.join("lib.txt")).unwrap();
    assert!(!written.contains("\r\r"), "stray CR in {written:?}");
    assert_eq!(written, "gamma\r\ndelta\r\n");
}

#[test]
fn edit_file_rejects_an_empty_batch() {
    let (_temp, root) = setup_root();
    fs::write(root.join("lib.txt"), "content\n").unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({ "relativeFilePath": "lib.txt", "edits": [] }),
    )
    .expect_err("an empty edits array edits nothing");
    assert!(err.to_string().contains("non-empty"), "{err}");
}

#[test]
fn edit_file_requires_an_existing_file() {
    let (_temp, root) = setup_root();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "never-written.txt",
            "edits": [{ "oldString": "a", "newString": "b" }]
        }),
    )
    .expect_err("edit_file must not create files; that is write_file's job");
    assert!(
        err.to_string().contains("not found in workspace"),
        "must say why: {err}"
    );
}

#[test]
fn edit_file_refuses_an_oldstring_that_quotes_the_excerpt_header() {
    let (_temp, root) = setup_root();
    let original = "fn budget() {\n    cap = 10;\n}\n";
    fs::write(root.join("lib.rs"), original).unwrap();

    // The shape lr7b_v6's repair turns produced: the directive's own `--- ...
    // (verbatim from disk) ---` label pasted into both sides of the patch, which
    // matches nothing and earned the model another "copy it verbatim" lecture.
    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.rs",
            "edits": [{
                "oldString": "--- lib.rs:1..3 (verbatim from disk) ---\nfn budget() {\n    cap = 10;\n}",
                "newString": "--- lib.rs:1..3 (verbatim from disk) ---\nfn budget() {\n    cap = 20;\n}"
            }]
        }),
    )
    .expect_err("a patch that carries the directive's furniture can never match the file");
    let msg = err.to_string();

    assert!(
        msg.contains("excerpt header"),
        "must name the furniture: {msg}"
    );
    assert!(
        msg.contains("verbatim from disk"),
        "must quote the header line it objected to: {msg}"
    );
    assert_eq!(fs::read_to_string(root.join("lib.rs")).unwrap(), original);
}

#[test]
fn the_same_edit_lands_once_the_excerpt_header_is_left_out() {
    let (_temp, root) = setup_root();
    fs::write(root.join("lib.rs"), "fn budget() {\n    cap = 10;\n}\n").unwrap();

    // Non-vacuity for the test above: the file text itself was always patchable,
    // so the refusal there came from the header line and nothing else.
    call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.rs",
            "edits": [{ "oldString": "    cap = 10;", "newString": "    cap = 20;" }]
        }),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(root.join("lib.rs")).unwrap(),
        "fn budget() {\n    cap = 20;\n}\n"
    );
}

#[test]
fn a_file_that_really_does_carry_an_excerpt_header_line_still_edits() {
    let (_temp, root) = setup_root();
    // Docs and transcripts quote this furniture back as content. The guard sits
    // on the no-match path precisely so such a file is patchable like any other.
    let original = "--- lib.rs:1..3 (verbatim from disk) ---\ntaken\n";
    fs::write(root.join("notes.md"), original).unwrap();

    call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "notes.md",
            "edits": [{
                "oldString": "--- lib.rs:1..3 (verbatim from disk) ---\ntaken",
                "newString": "kept"
            }]
        }),
    )
    .unwrap();

    assert_eq!(fs::read_to_string(root.join("notes.md")).unwrap(), "kept\n");
}

// The three refusals below are the shape lr7b_v11 spent its last turns in: the
// model quotes the text its own earlier write replaced, so the `oldString` is
// absent, and the reply it got — "copy the text verbatim ... call read_file" —
// pointed at a tool the same repair turn had locked.
const SUPERSEDED_OLD: &str = "    let user_record: &mut PersistedUserRecord = self\n        .user_dict\n        .get(user)\n        .unwrap_or(&mut PersistedUserRecord::default());";

#[test]
fn an_edit_whose_replacement_already_landed_is_told_the_line_it_landed_on() {
    let (_temp, root) = setup_root();
    let original = "fn create_budget(&mut self, user: &str) {\n    let user_record = self.user_dict.get(user).unwrap_or_else(|| {\n        let new_record = PersistedUserRecord::default();\n        self.user_dict.insert(user.clone(), new_record);\n        self.user_dict.get(user).unwrap()\n    });\n}\n";
    fs::write(root.join("lib.rs"), original).unwrap();

    // The `newString` is the file's own bytes: an earlier turn applied this edit
    // and the formatter left it alone, so the `oldString` is pre-repair text.
    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.rs",
            "edits": [{
                "oldString": SUPERSEDED_OLD,
                "newString": "    let user_record = self.user_dict.get(user).unwrap_or_else(|| {\n        let new_record = PersistedUserRecord::default();\n        self.user_dict.insert(user.clone(), new_record);\n        self.user_dict.get(user).unwrap()\n    });"
            }]
        }),
    )
    .expect_err("the oldString is gone, so nothing can match");
    let msg = err.to_string();

    assert!(
        msg.contains("this change is already on disk"),
        "must say the change landed rather than asking for it again: {msg}"
    );
    assert!(
        msg.contains("at line 2"),
        "must name where it landed: {msg}"
    );
    assert!(
        !msg.contains("copy the text verbatim"),
        "the lead-in contradicts an answer that says the text is already there: {msg}"
    );
    assert_eq!(fs::read_to_string(root.join("lib.rs")).unwrap(), original);
}

#[test]
fn a_refused_edit_is_answered_with_the_bytes_that_replaced_its_target() {
    let (_temp, root) = setup_root();
    // A different repair than the one being replayed landed here, so the
    // `newString` is not verbatim in the file — only its anchor line is.
    let original = "fn create_budget(&mut self, user: &str) {\n    let user_record = self.user_dict.get(user).unwrap_or_else(|| {\n        let record = PersistedUserRecord::with_budget(input.total_budget);\n        self.user_dict.insert(user, record);\n        self.user_dict.get(user).unwrap()\n    });\n}\n";
    fs::write(root.join("lib.rs"), original).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.rs",
            "edits": [{
                "oldString": SUPERSEDED_OLD,
                "newString": "    let user_record = self.user_dict.get(user).unwrap_or_else(|| {\n        let new_record = PersistedUserRecord::default();\n        self.user_dict.insert(user.clone(), new_record);\n        self.user_dict.get(user).unwrap()\n    });"
            }]
        }),
    )
    .expect_err("the oldString is gone, so nothing can match");
    let msg = err.to_string();

    assert!(
        msg.contains("this place has already been changed"),
        "must say why the text is missing: {msg}"
    );
    assert!(
        msg.contains("PersistedUserRecord::with_budget(input.total_budget);"),
        "must paste what the file says now: {msg}"
    );
    assert!(
        msg.contains("[line 1]"),
        "the pasted block must carry its line number: {msg}"
    );
    assert!(
        !msg.contains("appears nowhere in the file"),
        "the dead-end reading must not survive a site the tool can locate: {msg}"
    );
    assert!(
        !msg.contains("read_file"),
        "a repair turn locks exploration, so pointing at read_file is a dead end: {msg}"
    );
}

#[test]
fn a_refused_edit_that_landed_nowhere_still_gets_the_read_file_advice() {
    let (_temp, root) = setup_root();
    // Non-vacuity for the two above: with neither side of the patch anywhere in
    // the file there is no site to point at, and the old answer is the honest one.
    let original = "fn budget() {\n    cap = 10;\n}\n";
    fs::write(root.join("lib.rs"), original).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.rs",
            "edits": [{
                "oldString": "fn absent_before() {\n    nothing_here = 1;\n}",
                "newString": "fn absent_after() {\n    something_else = 2;\n}"
            }]
        }),
    )
    .expect_err("neither side of this patch exists in the file");
    let msg = err.to_string();

    assert!(
        msg.contains("appears nowhere in the file"),
        "must still say the text itself is wrong: {msg}"
    );
    assert!(
        msg.contains("read_file"),
        "with no site to paste, a ranged read is the only way forward: {msg}"
    );
    assert!(!msg.contains("already on disk"), "{msg}");
}

#[test]
fn a_short_replacement_is_never_claimed_to_be_already_applied() {
    let (_temp, root) = setup_root();
    // `}` is in every file. Naming its first occurrence would point the model at
    // the wrong place with total confidence, so the claim needs a real match.
    let original = "fn budget() {\n    cap = 10;\n}\n";
    fs::write(root.join("lib.rs"), original).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "lib.rs",
            "edits": [{ "oldString": "    total = 99;", "newString": "}" }]
        }),
    )
    .expect_err("the oldString is not in the file");
    let msg = err.to_string();

    assert!(!msg.contains("already on disk"), "{msg}");
    assert!(
        msg.contains("appears nowhere in the file"),
        "must fall through to the ordinary explanation: {msg}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — list_dir
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn list_dir_lists_workspace_root() {
    let (_temp, root) = setup_root();
    fs::write(root.join("a.txt"), "").unwrap();
    fs::write(root.join("b.txt"), "").unwrap();
    fs::create_dir(root.join("subdir")).unwrap();

    let output =
        call_tool_in_workspace(&root, "list_dir", &json!({"relativeDirPath": "."})).unwrap();

    assert!(output.contains("a.txt"));
    assert!(output.contains("b.txt"));
    assert!(output.contains("subdir"));
}

#[test]
fn list_dir_lists_subdirectory() {
    let (_temp, root) = setup_root();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

    let output =
        call_tool_in_workspace(&root, "list_dir", &json!({"relativeDirPath": "src"})).unwrap();

    assert!(output.contains("main.rs"));
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — grep_search
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn grep_search_finds_matching_lines() {
    let (_temp, root) = setup_root();
    fs::write(
        root.join("code.rs"),
        "fn hello() {}\nfn world() {}\nfn hello_world() {}",
    )
    .unwrap();

    let output = call_tool_in_workspace(&root, "grep_search", &json!({"query": "hello"})).unwrap();

    assert!(output.contains("hello"));
    assert!(output.contains("code.rs"));
}

#[test]
fn grep_search_does_not_read_the_agent_runtime_s_own_state_dir() {
    let (_temp, root) = setup_root();
    fs::write(root.join("code.rs"), "fn budget_manager() {}").unwrap();
    // `.velocity` is where the runtime keeps its own data, including repository
    // clones it made on request. A clone of the library being ported therefore
    // sits inside the workspace under test, and a model that greps for its own
    // task finds the upstream implementation instead of its own work.
    let cached = root.join(".velocity").join("clones").join("litellm-rust");
    fs::create_dir_all(&cached).unwrap();
    fs::write(
        cached.join("budget_manager.rs"),
        "fn budget_manager_upstream_clone() {}",
    )
    .unwrap();

    let output =
        call_tool_in_workspace(&root, "grep_search", &json!({"query": "budget_manager"})).unwrap();

    assert!(output.contains("code.rs"));
    assert!(
        !output.contains("litellm-rust"),
        "grep walked into the runtime's clone cache: {output}"
    );
}

#[test]
fn grep_search_returns_empty_for_no_matches() {
    let (_temp, root) = setup_root();
    fs::write(root.join("code.rs"), "fn hello() {}").unwrap();

    let output = call_tool_in_workspace(
        &root,
        "grep_search",
        &json!({"query": "nonexistent_pattern_xyz"}),
    )
    .unwrap();

    // Should return empty results or indicate no matches
    assert!(
        !output.contains("nonexistent_pattern_xyz")
            || output.contains("0 match")
            || output.contains("no match")
            || output.is_empty()
            || !output.contains("code.rs")
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — run_command
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn run_command_captures_stdout() {
    let (_temp, root) = setup_root();

    let output = call_tool_in_workspace(
        &root,
        "run_command",
        &json!({"command": "echo hello_velocity"}),
    )
    .unwrap();

    assert!(output.contains("hello_velocity"));
}

#[test]
fn run_command_returns_exit_code_info() {
    let (_temp, root) = setup_root();

    // Run a command that will fail
    let result = call_tool_in_workspace(&root, "run_command", &json!({"command": "exit 42"}));

    // Should still return output (with error info), not panic
    let _ = result;
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — delete_file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn delete_file_removes_file() {
    let (_temp, root) = setup_root();
    fs::write(root.join("to_delete.txt"), "delete me").unwrap();
    assert!(root.join("to_delete.txt").exists());

    call_tool_in_workspace(
        &root,
        "delete_file",
        &json!({"relativeFilePath": "to_delete.txt"}),
    )
    .unwrap();

    assert!(!root.join("to_delete.txt").exists());
}

#[test]
fn delete_file_errors_on_missing_file() {
    let (_temp, root) = setup_root();

    let result = call_tool_in_workspace(
        &root,
        "delete_file",
        &json!({"relativeFilePath": "nonexistent.txt"}),
    );

    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — fetch_panel_data
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fetch_panel_data_teams_returns_json() {
    let (_temp, root) = setup_root();

    let output =
        call_tool_in_workspace(&root, "fetch_panel_data", &json!({"panel": "teams"})).unwrap();

    let data: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(data["panel"], "teams");
}

#[test]
fn fetch_panel_data_wiki_returns_json() {
    let (_temp, root) = setup_root();

    let output =
        call_tool_in_workspace(&root, "fetch_panel_data", &json!({"panel": "wiki"})).unwrap();

    let data: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(data["panel"], "wiki");
}

#[test]
fn fetch_panel_data_graph_returns_json() {
    let (_temp, root) = setup_root();

    let output =
        call_tool_in_workspace(&root, "fetch_panel_data", &json!({"panel": "graph"})).unwrap();

    let data: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(data["panel"], "graph");
}

#[test]
fn fetch_panel_data_bookmarks_returns_json() {
    let (_temp, root) = setup_root();

    let output =
        call_tool_in_workspace(&root, "fetch_panel_data", &json!({"panel": "bookmarks"})).unwrap();

    let data: serde_json::Value = serde_json::from_str(&output).unwrap();
    // Bookmarks panel returns {"bookmarks": []} when no bookmarks exist
    assert!(
        data.get("bookmarks").is_some() || data.get("panel").is_some(),
        "bookmarks panel should return bookmarks array or panel field"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — agent checkpoint
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn agent_checkpoint_list_returns_array() {
    let (_temp, root) = setup_root();

    let output = call_tool_in_workspace(&root, "agent_checkpoint_list", &json!({})).unwrap();

    // Should return a valid response (may be empty list)
    assert!(!output.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — agent memory
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn agent_memory_remember_and_recall() {
    let (_temp, root) = setup_root();

    // Store a memory
    let output = call_tool_in_workspace(
        &root,
        "agent_memory_remember",
        &json!({
            "key": "test_fact",
            "content": "The sky is blue",
            "tags": ["test", "fact"],
            "score": 0.8
        }),
    )
    .unwrap();

    assert!(!output.is_empty());

    // Recall the memory
    let output = call_tool_in_workspace(
        &root,
        "agent_memory_recall",
        &json!({
            "query": "sky color",
            "limit": 5
        }),
    )
    .unwrap();

    assert!(!output.is_empty());
}

#[test]
fn agent_memory_forget_removes_memory() {
    let (_temp, root) = setup_root();

    // Store a memory
    call_tool_in_workspace(
        &root,
        "agent_memory_remember",
        &json!({
            "key": "to_forget",
            "content": "temporary fact"
        }),
    )
    .unwrap();

    // Forget it
    let output =
        call_tool_in_workspace(&root, "agent_memory_forget", &json!({"key": "to_forget"})).unwrap();

    assert!(!output.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// System Tool Contract Tests — knowledge base
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn knowledge_ingest_and_search() {
    let (_temp, root) = setup_root();

    // Ingest some text
    let output = call_tool_in_workspace(
        &root,
        "knowledge_ingest",
        &json!({
            "text": "Velocity is a Rust-based IDE with MCP support.",
            "source": "test_doc"
        }),
    )
    .unwrap();

    assert!(!output.is_empty());

    // Search for it
    let output = call_tool_in_workspace(
        &root,
        "knowledge_search",
        &json!({
            "query": "Rust IDE",
            "k": 3
        }),
    )
    .unwrap();

    assert!(!output.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// execute_nda fallback — Bug #11 (workspace-relative path + no hang)
// ═══════════════════════════════════════════════════════════════════════════

/// The fallback must resolve `ndaPath` against the workspace root, not the
/// process CWD, and reject a non-NDAV container with a clean error rather
/// than spawning a runaway child (the hang that killed batch7a).
#[test]
fn execute_nda_fallback_resolves_workspace_root_and_rejects_bad_magic() {
    let (_temp, root) = setup_root();
    let root = root.canonicalize().unwrap();
    fs::write(root.join("bogus.nda"), b"NDA1-not-a-real-container").unwrap();

    let result = crate::registry::system_tools::execute_rust_fallback_tool(
        &root,
        "execute_nda",
        &json!({ "ndaPath": "bogus.nda" }),
    );
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Invalid NDA container"), "got: {err}");
}

/// A missing file must report "not found in workspace" (proving the path was
/// resolved against the root) instead of reading from the launch directory.
#[test]
fn execute_nda_fallback_missing_path_is_workspace_scoped() {
    let (_temp, root) = setup_root();
    let root = root.canonicalize().unwrap();

    let result = crate::registry::system_tools::execute_rust_fallback_tool(
        &root,
        "execute_nda",
        &json!({ "ndaPath": "nowhere.nda" }),
    );
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not found in workspace"), "got: {err}");
}

/// A well-behaved payload (echo) runs to completion under the deadline and
/// its stdout is captured.
#[cfg(windows)]
#[test]
fn run_child_with_timeout_captures_quick_child() {
    use std::process::{Command, Stdio};
    let child = Command::new("cmd")
        .args(["/C", "echo sidecar-ok"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = crate::registry::system_tools::run_child_with_timeout(child).unwrap();
    assert!(out.contains("sidecar-ok"), "got: {out}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug #13: dispatch records must resolve outcome immediately (not stay Pending)
// ═══════════════════════════════════════════════════════════════════════════

/// A successful tool call must be recorded with outcome="success", not the
/// old behavior of writing "pending" forever.
#[test]
fn dispatch_records_success_outcome() {
    let (_temp, root) = setup_root();
    call_tool_in_workspace(
        &root,
        "write_file",
        &json!({"relativeFilePath": "hello.txt", "content": "hi"}),
    )
    .unwrap();

    let hist = call_tool_in_workspace(&root, "event_history", &json!({"limit": 5})).unwrap();
    let v: serde_json::Value = serde_json::from_str(&hist).unwrap();
    let events = v["events"].as_array().unwrap();
    assert!(
        events.iter().any(|e| e["outcome"] == "success"),
        "Expected at least one success event, got: {hist}"
    );
}

/// A failed tool call must be recorded with outcome="failure" and a
/// non-empty failure_reason so the decision trail is useful.
#[test]
fn dispatch_records_failure_with_reason() {
    let (_temp, root) = setup_root();
    // read_file on a missing path will fail.
    let _ = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "nonexistent_9x3.txt"}),
    );

    let hist = call_tool_in_workspace(
        &root,
        "event_history",
        &json!({"outcome": "failure", "limit": 5}),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&hist).unwrap();
    let events = v["events"].as_array().unwrap();
    let failure = events
        .iter()
        .find(|e| e["tool"] == "read_file")
        .unwrap_or_else(|| panic!("Expected read_file failure event in: {hist}"));
    assert_eq!(failure["outcome"], "failure");
    assert!(
        failure["failure_reason"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "failure_reason should be populated: {failure}"
    );
}

/// No automatic dispatch event should linger as "pending" — that status is
/// reserved for manual `event_record` entries the agent will resolve later.
#[test]
fn dispatch_no_auto_pending_events() {
    let (_temp, root) = setup_root();
    call_tool_in_workspace(
        &root,
        "write_file",
        &json!({"relativeFilePath": "abc.txt", "content": "data"}),
    )
    .unwrap();
    let _ = call_tool_in_workspace(
        &root,
        "read_file",
        &json!({"relativeFilePath": "missing_xyz.txt"}),
    );

    let hist = call_tool_in_workspace(
        &root,
        "event_history",
        &json!({"outcome": "pending", "limit": 10}),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&hist).unwrap();
    let events = v["events"].as_array().unwrap();
    // No write_file or read_file should appear as "pending".
    for e in events {
        assert!(
            e["tool"] != "write_file" && e["tool"] != "read_file",
            "Automatic dispatch event left pending: {e:?}"
        );
    }
}

// ── workflow_list MCP tool ────────────────────────────────────────────────

use crate::editor::workflow::{Workflow, WorkflowRegistry, WorkflowStep};

/// `workflow_list` must show per-step-type counts matching the dashboard
/// format and flag workflows whose Tool steps reference unknown tools.
#[test]
fn workflow_list_reports_step_counts_and_validation() {
    let (_temp, root) = setup_root();

    let mut wf = Workflow::new("wf-good", "Good Flow");
    wf.steps.push(WorkflowStep::Tool {
        name: "read_file".into(),
        args: json!({}),
    });
    wf.steps.push(WorkflowStep::AgentTask {
        prompt: "think".into(),
        team: None,
    });
    wf.steps.push(WorkflowStep::Condition {
        require: crate::editor::workflow::StepOutcome::Ok,
        on_text: None,
        recheck: None,
    });
    wf.steps.push(WorkflowStep::ForEach {
        source: "lastOutput".into(),
        as_var: "item".into(),
        body: vec![WorkflowStep::Tool {
            name: "read_file".into(),
            args: json!({}),
        }],
    });

    let mut bad = Workflow::new("wf-bad", "Bad Flow");
    bad.steps.push(WorkflowStep::Tool {
        name: "nonexistent_tool_xyz".into(),
        args: json!({}),
    });

    let mut reg = WorkflowRegistry::default();
    reg.add(wf);
    reg.add(bad);
    reg.save(&root).unwrap();

    let out = call_tool_in_workspace(&root, "workflow_list", &json!({})).unwrap();
    // Totals + breakdown, same "(N step(s): T# C# A# F{})" shape as playbooks.
    // Counts are top-level only (a ForEach body contributes its F marker, not
    // its nested tool steps) — matching the org dashboard.
    assert!(out.contains("wf-good (4 step(s): T1 C1 A1 F1)"), "{out}");
    // The valid workflow carries no warning line.
    assert!(!out.contains("wf-good\n      \u{26a0}"), "{out}");
    // The broken one is flagged with the offending tool name.
    assert!(out.contains("wf-bad (1 step(s): T1 C0 A0 F0)"), "{out}");
    assert!(out.contains("unknown tool 'nonexistent_tool_xyz'"), "{out}");
}

/// An empty registry reports gracefully rather than returning nothing.
#[test]
fn workflow_list_handles_empty_registry() {
    let (_temp, root) = setup_root();
    let out = call_tool_in_workspace(&root, "workflow_list", &json!({})).unwrap();
    assert!(out.contains("No saved workflows"), "{out}");
}

/// `workflow_run` must refuse to start a workflow whose Tool steps reference
/// unknown tools — a mid-run halt would leave partial side effects, so the
/// check is a pre-flight guard (same contract as run_playbook).
#[test]
fn workflow_run_rejects_unknown_tool_before_executing() {
    let (_temp, root) = setup_root();

    let mut wf = Workflow::new("wf-broken", "Broken Flow");
    // A side-effecting step FIRST would run and persist its effect if the
    // validator only checked lazily during execution.
    wf.steps.push(WorkflowStep::Tool {
        name: "write_file".into(),
        args: json!({"relativeFilePath": "should_not_exist.txt", "content": "x"}),
    });
    wf.steps.push(WorkflowStep::Tool {
        name: "nonexistent_tool_xyz".into(),
        args: json!({}),
    });
    let mut reg = WorkflowRegistry::default();
    reg.add(wf);
    reg.save(&root).unwrap();

    let err = call_tool_in_workspace(&root, "workflow_run", &json!({"id": "wf-broken"}))
        .expect_err("unknown tool must be rejected before the run starts")
        .to_string();
    assert!(err.contains("nonexistent_tool_xyz"), "{err}");
    assert!(err.contains("step 1"), "{err}");
    // The preceding write_file step never executed — nothing was written.
    assert!(!root.join("should_not_exist.txt").exists());
}

/// Connector steps live in a separate namespace (`.velocity/connectors.json`);
/// the pre-flight guard must reject unknown connector ids too, before any
/// side-effecting step runs.
#[test]
fn workflow_run_rejects_unknown_connector_before_executing() {
    let (_temp, root) = setup_root();

    let mut wf = Workflow::new("wf-conn", "Connector Flow");
    wf.steps.push(WorkflowStep::Tool {
        name: "write_file".into(),
        args: json!({"relativeFilePath": "should_not_exist2.txt", "content": "x"}),
    });
    wf.steps.push(WorkflowStep::Connector {
        id: "ghost_svc".into(),
        req: json!({"method": "GET", "path": "/"}),
    });
    let mut reg = WorkflowRegistry::default();
    reg.add(wf);
    reg.save(&root).unwrap();

    let err = call_tool_in_workspace(&root, "workflow_run", &json!({"id": "wf-conn"}))
        .expect_err("unknown connector id must be rejected before the run starts")
        .to_string();
    assert!(err.contains("unknown connector 'ghost_svc'"), "{err}");
    assert!(!root.join("should_not_exist2.txt").exists());
}

/// `workflow_list` surfaces connector issues alongside tool issues.
#[test]
fn workflow_list_flags_unknown_connector() {
    let (_temp, root) = setup_root();

    let mut wf = Workflow::new("wf-conn2", "Listed Connector Flow");
    wf.steps.push(WorkflowStep::Connector {
        id: "ghost_svc2".into(),
        req: json!({}),
    });
    let mut reg = WorkflowRegistry::default();
    reg.add(wf);
    reg.save(&root).unwrap();

    let out = call_tool_in_workspace(&root, "workflow_list", &json!({})).unwrap();
    assert!(out.contains("unknown connector 'ghost_svc2'"), "{out}");
}

/// A `newString` that forgets the braces its `oldString` matched does not fail
/// to match — it applies, and leaves a block open. testgate2b showed what that
/// costs: a repair edit whose needle was `"    }\n}\n"` and whose replacement
/// repeated neither brace wrote cleanly, rustc reported `unclosed delimiter`,
/// and the 391-second turn that followed made the file worse than the first
/// mistake. The tool cannot know what the model meant, but it can refuse to make
/// the file less balanced than it found it.
#[test]
fn an_edit_that_leaves_a_brace_open_is_refused_before_the_file_changes() {
    let (_temp, root) = setup_root();
    let before = "mod tests {\n    #[test]\n    fn a() {\n        assert!(true);\n    }\n}\n";
    fs::write(root.join("port.rs"), before).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    }\n}\n",
                "newString": "\n        assert_eq!(mgr.get_total_budget(\"unknown\"), None);\n"
            }],
            "context": "add an assertion"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a batch that drops two closing braces must be refused".to_string());

    assert!(err.contains("the file is unchanged"), "{err}");
    assert!(err.contains("left open"), "{err}");
    assert!(err.contains("repeat"), "and say what to do about it: {err}");
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        before,
        "a refused batch leaves every byte alone"
    );
}

/// The other side of the same check: a restructure that adds a scope has to keep
/// both delimiters in the batch, and when it does the edit must apply. A guard
/// that refused this too would push the model back onto the whole-file rewrite
/// `edit_file` exists to replace.
#[test]
fn a_restructure_that_carries_its_braces_in_the_batch_applies() {
    let (_temp, root) = setup_root();
    fs::write(root.join("port.rs"), "fn f() {\n    run();\n}\n").unwrap();

    let out = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [
                { "oldString": "fn f() {\n    run();", "newString": "fn f() {\n    unsafe {\n        run();" },
                { "oldString": "        run();\n}\n", "newString": "        run();\n    }\n}\n" }
            ],
            "context": "wrap the call"
        }),
    )
    .unwrap();
    assert!(out.contains("Success"), "{out}");
    // Compared line by line because a `.rs` write goes through rustfmt, which
    // owns the indentation but not the braces the edit put there.
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    let kept: Vec<&str> = after
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(
        kept,
        vec!["fn f() {", "unsafe {", "run();", "}", "}"],
        "{after}"
    );
}

/// The counting only holds if it ignores the braces that are not structure: a
/// `{` inside a format string, a doc comment or a char literal, and a lifetime's
/// apostrophe, which is not a quote at all. Refusing an edit because of one of
/// those would be a new way to lose a turn.
#[test]
fn brace_delta_counts_only_the_braces_that_are_rust_structure() {
    use crate::registry::system_tools::brace_delta;

    assert_eq!(brace_delta("fn f<'a>(x: &'a str) { x.len() }"), 0);
    assert_eq!(brace_delta("let msg = \"a { bracket\";"), 0);
    assert_eq!(brace_delta("// a block opens with {\nlet c = '{';"), 0);
    assert_eq!(brace_delta("let raw = r#\"{ \"unbalanced\" }\"#;"), 0);
    assert_eq!(brace_delta("let e = '\\''; let u = '\\u{7B}';"), 0);
    // Block comments nest in Rust, so the inner `*/` must not re-expose the text
    // that follows it as code.
    assert_eq!(brace_delta("/* { /* { */ } */ fn g() {"), 1);
    assert_eq!(brace_delta("    }\n}\n"), -2);
}

/// A file the compiler already dislikes is exactly the file a repair turn edits,
/// so the guard compares the batch's own drift rather than the file's absolute
/// balance — an edit that changes nothing about the shape must not be refused.
#[test]
fn a_fix_inside_an_already_unbalanced_file_is_not_refused_for_being_unbalanced() {
    let (_temp, root) = setup_root();
    fs::write(
        root.join("port.rs"),
        "mod tests {\n    fn a() {\n        assert!(tru);\n",
    )
    .unwrap();

    let out = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{ "oldString": "        assert!(tru);", "newString": "        assert!(true);" }],
            "context": "fix the name"
        }),
    )
    .unwrap();
    assert!(out.contains("Success"), "{out}");
}
