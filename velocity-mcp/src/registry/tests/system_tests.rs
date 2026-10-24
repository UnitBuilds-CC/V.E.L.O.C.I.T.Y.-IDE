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

/// `lr7b_v20` in full shape. The model's `oldString` is the file's own two lines
/// with four fewer spaces on each and its `newString` is the correct E0308 fix,
/// so the match is unique the moment indentation is ignored. The refusal already
/// said as much — "IS in 1 place(s) of the file under different leading
/// whitespace" — and pasted the file's own bytes under a `[line 358]` marker,
/// and the `read_file` its advice sent the model to returned the same text again.
/// It re-sent eight spaces ten times anyway. A model cannot count leading
/// whitespace, so no refusal ends this loop; applying the one site it can have
/// meant does.
#[test]
fn an_oldstring_whose_only_fault_is_indentation_is_applied_at_the_one_site_it_matches() {
    let (_temp, root) = setup_root();
    let file = "\
pub struct UpdateCostResult<'a> {
    pub user_record: &'a Record,
}

pub fn update_cost(record: &Record) -> UpdateCostResult<'_> {
    let result_record = record;
    UpdateCostResult {
        user_record: result_record,
    }
}
";
    fs::write(root.join("port.rs"), file).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    user_record: result_record,\n}",
                "newString": "    user_record: &result_record,\n}"
            }],
            "context": "fix E0308"
        }),
    )
    .unwrap_or_else(|e| panic!("a uniquely-located mis-indent is not a refusal: {e}"));

    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert!(
        after.contains("        user_record: &result_record,"),
        "the replacement lands at the FILE's eight spaces, not the four it was sent with: {after}"
    );
    assert!(
        ok.contains("line 8") && ok.contains("ignoring leading whitespace"),
        "and the success says it was re-indented, so the changed bytes are not a mystery: {ok}"
    );
    assert!(
        ok.contains("4 whitespace character(s) shallower"),
        "by how much, and which way: {ok}"
    );
    assert!(
        !after.contains("user_record: result_record,"),
        "the old text is gone — this applied, it did not merely refuse politely: {after}"
    );
}

/// The rescue is licensed by uniqueness alone, so the moment a second site
/// matches, applying either one is a guess. The shift here IS uniform — four
/// spaces on every line — so nothing but the second site keeps this from being
/// applied, and the refusal keeps the sites paste it had before, which is the
/// answer that lets the model widen instead.
#[test]
fn a_mis_indented_oldstring_matching_two_sites_is_still_refused_with_both_pasted() {
    let (_temp, root) = setup_root();
    let file = "\
fn a() {
    mgr.record(
        total_cost,
    );
}
fn b() {
    mgr.record(
        total_cost,
    );
}
";
    fs::write(root.join("port.rs"), file).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "mgr.record(\n    total_cost,\n);",
                "newString": "mgr.record(\n    total_budget,\n);"
            }],
            "context": "widen"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "two candidate sites must not be resolved by picking one".to_string());

    assert!(err.contains("IS in 2 place(s)"), "{err}");
    assert!(
        err.contains("[line 2]") && err.contains("[line 7]"),
        "both sites, numbered: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        file,
        "a refused batch leaves the file alone"
    );
}

/// Uniformity is the other half of the licence. A needle indented by four extra
/// spaces on its first line and two on its second is not the same block
/// re-indented, it is a model that restructured the region, and shifting the
/// replacement by one amount would be inventing an edit.
///
/// The two lines have to be statements that stand on their own. Wrapped inside a
/// call they are folded before this question is ever asked, and the fold drops the
/// interior indentation that is the only evidence the shifts disagreed — see
/// `a_non_uniform_shift_inside_a_wrapped_call_is_folded_and_applied`.
#[test]
fn a_mis_indented_oldstring_whose_lines_disagree_on_the_shift_is_refused() {
    let (_temp, root) = setup_root();
    let file = "fn a() {\n    mgr.record(total_cost);\n    save(total_cost);\n}\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "        mgr.record(total_cost);\n      save(total_cost);",
                "newString": "        mgr.record(total_budget);\n      save(total_budget);"
            }],
            "context": "widen"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a non-uniform re-indent must not be applied".to_string());

    assert!(
        !err.contains("ignoring leading whitespace"),
        "nothing was applied, so the success wording has no business here: {err}"
    );
    assert!(err.contains("different leading whitespace"), "{err}");
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        file,
        "a refused batch leaves the file alone"
    );
}

/// The other side of that boundary, and the reason the fixture above had to move.
/// The same disagreement wrapped inside a call is applied rather than refused:
/// both sides fold onto one line first, and a fold keeps the code and drops the
/// interior whitespace that was the only evidence of the disagreement. What is left
/// to take a shift from is the first line, which is the licence the rescue already
/// runs under — the code occurs in one place and nowhere else. Refusing this one
/// would cost a turn to a model that has just proved it cannot count spaces, and
/// `lr7b_v20` re-guessed eight spaces ten times on exactly that refusal.
#[test]
fn a_non_uniform_shift_inside_a_wrapped_call_is_folded_and_applied() {
    let (_temp, root) = setup_root();
    let file = "fn a() {\n    mgr.record(\n        total_cost,\n    );\n}\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "        mgr.record(\n      total_cost,\n    );",
                "newString": "        mgr.record(\n      total_budget,\n    );"
            }],
            "context": "widen"
        }),
    )
    .unwrap_or_else(|e| panic!("one site, its code matched once the wraps were ignored: {e}"));

    assert!(
        ok.contains("ignoring its line breaks"),
        "the answer has to say what was forgiven, or the model re-sends it: {ok}"
    );
    assert!(
        ok.contains("at line 2") && ok.contains("across 3 lines") && ok.contains("wrote it on 3"),
        "both sides were folded, and the note counts the file's lines against the model's: {ok}"
    );
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert!(after.contains("total_budget"), "{after}");
    assert!(
        !after.contains("total_cost"),
        "the site was replaced, not a flat copy left beside it: {after}"
    );
}

/// A rescued edit writes into the file's own bytes, so it has to keep their line
/// endings. Emitting LF into a CRLF file would leave it mixed and every later
/// verbatim `oldString` the model copies from `read_file` would fail to match.
/// Not a `.rs` file, so no formatter stands between the splice and the assertion.
#[test]
fn an_indentation_rescue_keeps_the_files_own_line_endings() {
    let (_temp, root) = setup_root();
    let file = "begin\r\n    mgr.record(\r\n        total_cost,\r\n    );\r\nend\r\n";
    fs::write(root.join("port.txt"), file).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.txt",
            "edits": [{
                "oldString": "mgr.record(\n    total_cost,\n);",
                "newString": "mgr.record(\n    total_budget,\n);"
            }],
            "context": "rename the field"
        }),
    )
    .unwrap_or_else(|e| panic!("a unique mis-indent in a CRLF file still applies: {e}"));

    assert!(ok.contains("ignoring leading whitespace"), "{ok}");
    let after = fs::read_to_string(root.join("port.txt")).unwrap();
    assert!(
        after.contains("        total_budget,\r\n"),
        "the re-indented replacement carries CRLF like the rest of the file: {after:?}"
    );
    assert_eq!(
        after.matches('\n').count(),
        after.matches("\r\n").count(),
        "no line is left half-converted: {after:?}"
    );
    assert!(
        after.starts_with("begin\r\n") && after.ends_with("end\r\n"),
        "only the matched span was touched: {after:?}"
    );
}

/// Ordering: `already_applied_at` is a truer answer than the rescue. A
/// `newString` that is already on disk means this change was made and the
/// compiler is asking for a different one — applying the mis-indented
/// `oldString` at the other site would write a second copy of a finished edit.
/// The shift at that other site is a uniform four spaces, so nothing but the
/// ordering keeps this from being applied.
#[test]
fn an_already_applied_change_is_not_rescued_into_a_second_site() {
    let (_temp, root) = setup_root();
    let file = "fn a() {\n    mgr.record(\n        &total_cost,\n    );\n}\n\
                fn b() {\n      mgr.record(\n        total_cost,\n      );\n}\n";
    fs::write(root.join("port.txt"), file).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.txt",
            "edits": [{
                "oldString": "  mgr.record(\n    total_cost,\n  );",
                "newString": "    mgr.record(\n        &total_cost,\n    );"
            }],
            "context": "fix E0308"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "an already-applied change is not something to redo".to_string());

    assert!(
        err.contains("already on disk"),
        "the honest answer wins over the applicable one: {err}"
    );
    assert!(
        err.contains("line 2"),
        "and names where it already is: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.txt")).unwrap(),
        file,
        "and the second site is left alone"
    );
}

/// `lr7b_v31` end to end. Calls 5 and 6 of that run were byte-identical and
/// *both* applied: the first inserted `let _ = self.save_data();` once and the
/// second inserted it again, because the `oldString` still matched uniquely and
/// nothing asked whether the file already went on with the rest of the
/// `newString`. Every `oldString` the model built after that was stale, so turns
/// 4 to 9 alternated a refusal with a re-read of the same fourteen lines and the
/// transcript ended at 29 identical refusals.
#[test]
fn a_replayed_edit_that_already_applied_is_refused_instead_of_written_twice() {
    let (_temp, root) = setup_root();
    let before = "fn charge(&mut self, user: &str, record: UsageRecord) -> f64 {\n    let result_record = record.clone();\n    result_record.current_cost\n}\n";
    fs::write(root.join("port.txt"), before).unwrap();
    let call = json!({
        "relativeFilePath": "port.txt",
        "edits": [{
            "oldString": "    let result_record = record.clone();",
            "newString": "    let result_record = record.clone();\n    let _ = self.save_data();"
        }],
        "context": "persist before returning"
    });
    let copies = || {
        fs::read_to_string(root.join("port.txt"))
            .unwrap()
            .matches("let _ = self.save_data();")
            .count()
    };

    let ok = call_tool_in_workspace(&root, "edit_file", &call)
        .unwrap_or_else(|e| panic!("the first call is an ordinary insert: {e}"));
    assert!(ok.starts_with("Success"), "{ok}");
    assert_eq!(copies(), 1, "the insert landed once");

    let after = fs::read_to_string(root.join("port.txt")).unwrap();
    let err = call_tool_in_workspace(&root, "edit_file", &call)
        .err()
        .map(|e| e.to_string())
        .unwrap_or_else(|| "a replay of an applied edit writes a second copy".to_string());

    assert!(
        err.contains("already on disk"),
        "the same answer its whole-file sibling gives, so `replay_advice` reads it the same way: {err}"
    );
    assert!(
        err.contains("from line 2 down"),
        "and names where the replacement already starts: {err}"
    );
    assert!(
        err.contains("would write 1 of those lines a second time"),
        "and says what applying it again would cost: {err}"
    );
    assert!(
        !err.contains("is not in the file at all"),
        "the `oldString` is in the file here; that clause belongs to the other predicate: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.txt")).unwrap(),
        after,
        "a refused batch leaves every byte alone"
    );
    assert_eq!(copies(), 1, "and the file still carries the insert once");
}

/// The other side of the same predicate: an insert the file does not yet carry is
/// the ordinary case and has to keep applying. Without this the refusal above
/// could be passing by refusing everything.
#[test]
fn an_insert_the_file_lacks_is_still_written_at_its_unique_site() {
    let (_temp, root) = setup_root();
    let before = "fn charge(&mut self, record: UsageRecord) -> f64 {\n    let result_record = record.clone();\n    result_record.current_cost\n}\n";
    fs::write(root.join("port.txt"), before).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.txt",
            "edits": [{
                "oldString": "    let result_record = record.clone();",
                "newString": "    let result_record = record.clone();\n    let _ = self.save_data();"
            }],
            "context": "persist before returning"
        }),
    )
    .unwrap_or_else(|e| panic!("nothing about this edit is a replay: {e}"));

    assert!(ok.starts_with("Success"), "{ok}");
    assert_eq!(
        fs::read_to_string(root.join("port.txt")).unwrap(),
        "fn charge(&mut self, record: UsageRecord) -> f64 {\n    let result_record = record.clone();\n    let _ = self.save_data();\n    result_record.current_cost\n}\n",
        "the insert went in where the site was"
    );
}

/// A batch is all-or-nothing, so one replayed edit in a batch of two refuses the
/// other one with it. That is the contract every other refusal here follows, and
/// it is the honest one: reporting "applied 2 edit(s)" when one of them was
/// skipped is the lie the success message exists not to tell.
#[test]
fn a_replayed_edit_refuses_its_whole_batch_and_leaves_the_other_edit_out() {
    let (_temp, root) = setup_root();
    let before = "fn a() {\n    let x = 1;\n    let _ = self.save_data();\n}\n\nfn b() {\n    let y = 2;\n}\n";
    fs::write(root.join("port.txt"), before).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.txt",
            "edits": [
                {
                    "oldString": "    let x = 1;",
                    "newString": "    let x = 1;\n    let _ = self.save_data();"
                },
                {
                    "oldString": "    let y = 2;",
                    "newString": "    let y = compute();"
                }
            ],
            "context": "two changes"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "one replayed edit has to refuse the batch".to_string());

    assert!(err.contains("no edits were applied"), "{err}");
    assert!(
        err.contains("edit 1: this change is already on disk"),
        "{err}"
    );
    assert!(
        !err.contains("edit 2:"),
        "the second edit was applicable and is not blamed: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.txt")).unwrap(),
        before,
        "a refused batch leaves every byte alone"
    );
}

/// The predicate is not gated on Rust, unlike `terminator_miss`: a duplicated
/// line is a duplicated line in every language, and a `.rs` file is where the
/// crucible actually runs. rustfmt leaves this fixture alone, so the second call
/// sees the same bytes the first one wrote.
#[test]
fn a_replayed_edit_is_refused_in_a_rust_file_too() {
    let (_temp, root) = setup_root();
    let before = "impl Manager {\n    fn charge(&mut self, record: UsageRecord) -> f64 {\n        let result_record = record.clone();\n        result_record.current_cost\n    }\n}\n";
    fs::write(root.join("port.rs"), before).unwrap();
    let call = json!({
        "relativeFilePath": "port.rs",
        "edits": [{
            "oldString": "        let result_record = record.clone();",
            "newString": "        let result_record = record.clone();\n        let _ = self.save_data();"
        }],
        "context": "persist before returning"
    });

    call_tool_in_workspace(&root, "edit_file", &call)
        .unwrap_or_else(|e| panic!("the first call is an ordinary insert: {e}"));
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert_eq!(
        after.matches("let _ = self.save_data();").count(),
        1,
        "the insert landed once: {after}"
    );

    let err = call_tool_in_workspace(&root, "edit_file", &call)
        .err()
        .map(|e| e.to_string())
        .unwrap_or_else(|| "a replay of an applied edit writes a second copy".to_string());
    assert!(err.contains("already on disk"), "{err}");
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        after,
        "and the file still carries the insert once"
    );
}

/// `lr7b_v22` in full shape. Turns 5, 7, 9 and 11 sent one byte-identical
/// `edit_file` whose `oldString` was the file's own chain with rustfmt's line
/// breaks undone, and all four were refused — burning three compile repairs and
/// two refunds on a turn counter that never saw a write. Turn 4 had been allowed
/// to `read_file` that exact region and handed back every line verbatim; the
/// model re-emitted them flattened anyway. Reproducing where a formatter chose
/// to wrap a chain is the same ability `indent_rescue` exists to substitute for,
/// one dimension over, and its absence — not the model's reasoning — is what
/// ended the run.
#[test]
fn a_chain_the_model_flattened_onto_one_line_is_applied_not_refused() {
    let (_temp, root) = setup_root();
    let file = "fn update(&mut self, k: &str) {\n    self.dict\n        .get_mut(k)\n        .unwrap_or(&mut Rec { a: 0.0 });\n}\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    self.dict.get_mut(k).unwrap_or(&mut Rec { a: 0.0 });",
                "newString": "    let r = self.dict.get_mut(k).unwrap_or(&mut Rec { a: 0.0 });\n    save(r);"
            }],
            "context": "fix E0515"
        }),
    )
    .unwrap_or_else(|e| panic!("the code is the file's own, only its wraps differ: {e}"));

    assert!(
        ok.contains("ignoring its line breaks"),
        "the answer has to say what was forgiven, or the model re-sends it: {ok}"
    );
    assert!(
        ok.contains("across 3 lines") && ok.contains("wrote it on 1"),
        "and how many: {ok}"
    );
    assert!(ok.contains("line 2"), "{ok}");
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert!(after.contains("let r = self.dict"), "{after}");
    assert!(after.contains("save(r);"), "{after}");
    assert!(
        !after.contains("        .get_mut(k)"),
        "the wrapped chain is what got replaced, not a copy of it beside the original: {after}"
    );
}

/// `superseded_site` infers a history — "an earlier write of yours replaced what
/// the `oldString` quotes" — from one line of the `newString` occurring in the
/// file exactly once. v22's fourth refusal was that claim and it was false: the
/// anchor was `self.user_dict.insert(user, record);`, a line the file had always
/// carried and the `newString` merely repeated as context, while the site itself
/// sat there untouched with its chain re-wrapped. An explanation of why text
/// cannot be located is contradicted by anything that locates it, so the two
/// answers that apply come first. `already_applied_at` still leads — a whole
/// `newString` on disk is proof rather than inference.
#[test]
fn a_rescue_that_locates_the_site_outranks_the_guess_that_it_moved() {
    let (_temp, root) = setup_root();
    let file = "fn update(&mut self, k: &str) {\n    self.dict.insert(k, rec);\n    self.dict\n        .get_mut(k)\n        .unwrap_or(&mut Rec { a: 0.0 });\n}\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    self.dict.get_mut(k).unwrap_or(&mut Rec { a: 0.0 });",
                "newString": "    let r = self.dict.get_mut(k).unwrap_or(&mut Rec { a: 0.0 });\n    self.dict.insert(k, rec);\n    save(r);"
            }],
            "context": "fix E0515"
        }),
    )
    .unwrap_or_else(|e| panic!("the anchor line is context the model copied, not a rewrite: {e}"));

    assert!(!ok.contains("The text is gone"), "{ok}");
    assert!(ok.contains("ignoring its line breaks"), "{ok}");
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert_eq!(
        after.matches("self.dict.insert(k, rec);").count(),
        2,
        "the context line is written where the model put it: {after}"
    );
}

/// `lr7b_v27`, which spent 38 identical refusals and ended at turn 48 with
/// E0499 and E0515 still standing. rustfmt had broken one binding in two places —
/// after its `=` at line 287 and again before each `.` of the chain under it, down
/// to line 291:
///
/// ```text
///         let result_record =
///             self.user_dict
///                 .values_mut()
///                 .next()
///                 .unwrap_or(&mut PersistedUserRecord {
/// ```
///
/// The model wrote all five back as the one line
/// `let result_record = self.user_dict.values_mut().next().unwrap_or(&mut
/// PersistedUserRecord {`. `chain_join_rescue` existed for exactly that and could
/// not see it, because `dechain` joined only before a `.` and so left the file's
/// first break standing: the joined file said `let result_record =` and the needle
/// said `let result_record = self.user_dict…`, which differ. The refusal that
/// answered it named a real line, 292, but quoted only 290 and 291 — two of the
/// five the absent head covered — so it was true and unusable, and the model
/// re-sent. This is a rescue rather than a better explanation because the code is
/// the file's own, the site is unique (one hit in the 754-line file), and the only
/// thing wrong is where the formatter wrapped it.
#[test]
fn a_binding_the_formatter_broke_after_its_equals_is_applied_not_refused() {
    let (_temp, root) = setup_root();
    let file = concat!(
        "    fn create_budget(&mut self) {\n",
        "        let _ = self.save_data();\n",
        "        let result_record =\n",
        "            self.user_dict\n",
        "                .values_mut()\n",
        "                .next()\n",
        "                .unwrap_or(&mut Rec { a: 0.0 });\n",
        "        result_record\n",
        "    }\n",
    );
    fs::write(root.join("port.rs"), file).unwrap();

    let flat = "        let result_record = self.user_dict.values_mut().next().unwrap_or(&mut Rec { a: 0.0 });";
    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": flat,
                "newString": format!("{flat}\n        save(result_record);")
            }],
            "context": "fix E0515"
        }),
    )
    .unwrap_or_else(|e| panic!("one binding, five file lines, one needle line: {e}"));

    assert!(
        ok.contains("ignoring its line breaks"),
        "the answer has to say what was forgiven, or the model re-sends it: {ok}"
    );
    assert!(
        ok.contains("across 5 lines") && ok.contains("wrote it on 1"),
        "and how many, counting the break after the `=`: {ok}"
    );
    assert!(ok.contains("line 3"), "{ok}");
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert!(after.contains("save(result_record);"), "{after}");
    assert!(
        !after.contains("        let result_record =\n"),
        "the broken binding is what got replaced, not a flat copy beside it: {after}"
    );
    assert!(
        !after.contains("                .values_mut()"),
        "and so is the chain under it: {after}"
    );
}

/// `lr7b_v28` call 5. The `=` break of v26 and v27 is only the first of three the
/// model has to get right here: rustfmt had also broken the chain under it and
/// opened a struct literal whose six fields each got a line, twelve lines in all,
/// and the model quoted the whole of it as one. `nearest_miss` scores a prefix run,
/// so a needle whose first line is wrong says nothing about the other eleven;
/// `tail_anchor` needs at least four lines to work with and this needle has one;
/// and so the answer fell through to the one diagnosis in the chain that names no
/// line at all.
#[test]
fn a_struct_literal_the_formatter_broke_open_is_applied_not_refused() {
    let (_temp, root) = setup_root();
    let file = concat!(
        "    fn create_budget(&mut self) {\n",
        "        let _ = self.save_data();\n",
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
        "        result_record\n",
        "    }\n",
    );
    fs::write(root.join("port.rs"), file).unwrap();

    let flat = "        let result_record = self.user_dict.values_mut().next().unwrap_or(&mut PersistedUserRecord { total_budget: 0.0, current_cost: 0.0, model_cost: HashMap::new(), duration: None, created_at: 0.0, last_updated_at: 0.0, });";
    let fixed = flat.replace("unwrap_or(", "unwrap_or_else(|| ");
    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{ "oldString": flat, "newString": fixed }],
            "context": "fix E0515"
        }),
    )
    .unwrap_or_else(|e| panic!("one binding, twelve file lines, one needle line: {e}"));

    assert!(
        ok.contains("ignoring its line breaks"),
        "the answer has to say what was forgiven, or the model re-sends it: {ok}"
    );
    assert!(
        ok.contains("across 12 lines") && ok.contains("wrote it on 1"),
        "and how many, counting the struct fields: {ok}"
    );
    assert!(ok.contains("line 3"), "{ok}");
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert!(after.contains("unwrap_or_else(||"), "{after}");
    assert_eq!(
        after.matches("unwrap_or").count(),
        1,
        "the twelve lines were replaced, not kept beside a flat copy: {after}"
    );
}

/// `lr7b_v24` turns 3 to 6. One semicolon cost the run four of its ten turns:
/// the `oldString` said `});` where the file says `})`, and turns 3 and 4 were
/// both answered with that line quoted from each side, which is the whole of the
/// fix. Turn 5 sent the same twelve lines behind a different replacement, and
/// because that replacement happened to repeat one unique line of the file,
/// `superseded_site` took the refusal away from `nearest_miss` and asserted "an
/// earlier write of yours replaced what the `oldString` quotes" over a paste of
/// a function forty lines above the site. Nothing had replaced anything — the
/// site was standing with one character altered, which the two earlier refusals
/// had proved by quoting it. Turn 6 then quoted the tail of turn 5's refused
/// `newString`, text the file had never held, and the run ended two errors deep.
///
/// A located site outranks an inferred history for the same reason the two
/// rescues do: an explanation of why text cannot be located is contradicted by
/// anything that locates it.
#[test]
fn a_tight_near_miss_outranks_the_guess_that_the_site_was_rewritten() {
    let (_temp, root) = setup_root();
    let file = "fn update(&mut self, user: &str) -> &mut Rec {\n    let _ = self.save_data();\n    self.dict\n        .values_mut()\n        .next()\n        .unwrap_or(&mut Rec { a: 0.0 })\n}\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    self.dict\n        .values_mut()\n        .next()\n        .unwrap_or(&mut Rec { a: 0.0 });\n}",
                "newString": "    let r = self.dict\n        .values_mut()\n        .next()\n        .unwrap_or(&mut Rec { a: 0.0 });\n    let _ = self.save_data();\n    r"
            }],
            "context": "fix E0515"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "one semicolon is not a match".to_string());

    assert!(
        !err.contains("The text is gone"),
        "the replacement's one unique file line is context, not a rewrite, and the site is \
         standing: {err}"
    );
    assert!(
        err.contains("is where they part"),
        "the answer that names the line has to be the one sent: {err}"
    );
    assert!(
        err.contains("you wrote `.unwrap_or(&mut Rec { a: 0.0 });`"),
        "{err}"
    );
    assert!(
        err.contains("the file says `.unwrap_or(&mut Rec { a: 0.0 })`"),
        "{err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        file,
        "a refused batch leaves the file alone"
    );
}

/// The other side of the confidence gate. Three lines matched and two the scan
/// never reached is a prefix that happened, not a located site, so the guess
/// keeps its turn: it is the only answer left that says anything about where the
/// text went, and pasting the file's own bytes around the anchor beats naming a
/// divergence at a place the edit may never have been aimed at.
#[test]
fn a_loose_near_miss_still_defers_to_the_guess_that_the_site_moved() {
    let (_temp, root) = setup_root();
    let file = "fn update(&mut self, user: &str) -> &mut Rec {\n    let _ = self.save_data();\n    self.dict\n        .values_mut()\n        .next()\n        .unwrap_or(&mut Rec { a: 0.0 })\n}\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    self.dict\n        .values_mut()\n        .next()\n        .unwrpx(&mut Rec { a: 0.0 })\n        .other()\n}",
                "newString": "    let r = self.dict\n        .values_mut();\n    let _ = self.save_data();\n    r"
            }],
            "context": "fix E0515"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a line the file has never held is not a match".to_string());

    assert!(err.contains("The text is gone"), "{err}");
    assert!(!err.contains("is where they part"), "{err}");
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        file,
        "a refused batch leaves the file alone"
    );
}

/// `lr7b_v26` turns 5, 11 and 12. The model quoted eleven lines whose only fault
/// was the first, and every line after it was the file's own byte for byte.
/// [`nearest_miss`] is a prefix run, so it scored zero, and the refusal fell
/// through to "the text itself is wrong rather than merely mis-indented ... over
/// that region" with no line number anywhere in it. The model obeyed by reading
/// 289-304 three times — a window opening *below* the only two lines it was
/// missing — and re-sent the same guess each time.
///
/// v26's first line was the file's own two lines joined, and that shape is now
/// applied rather than explained: see
/// `a_binding_the_formatter_broke_after_its_equals_is_applied_not_refused`. What
/// is left for the tail anchor is a head that is wrong *text* — here a field the
/// model renamed — where applying the edit would write a line the file has never
/// held, so the only useful answer is the line the tail starts at and the lines
/// above it to copy the head from.
///
/// The `newString` carries one unique file line, so `superseded_site` has an
/// anchor and would otherwise have answered first with a history nothing had
/// caused.
#[test]
fn a_refused_edit_whose_head_is_wrong_but_whose_tail_stands_names_the_line() {
    let (_temp, root) = setup_root();
    let file = concat!(
        "    fn create_budget(&mut self) {\n",
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
    fs::write(root.join("port.rs"), file).unwrap();
    let old = concat!(
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

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": old,
                "newString": "        let _ = self.save_data();\n        let result_record =\n            self.user_dict\n                .values_mut()\n                .next()\n                .unwrap_or(&mut PersistedUserRecord {\n                    total_budget: 0.0,\n                });\n    }"
            }],
            "context": "fix E0499"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a line the file has never held is not a match".to_string());

    assert!(
        !err.contains("The text is gone"),
        "the tail standing at one place is a reading of the file, and that outranks the guess \
         about its history: {err}"
    );
    assert!(
        err.contains("at line 5") && err.contains("Read from line 3"),
        "the refusal has to carry a line number and not a region: {err}"
    );
    assert!(
        err.contains("3| let result_record =") && err.contains("4| self.user_dict"),
        "and quote the two lines the head has to be copied from: {err}"
    );
    assert!(
        !err.contains("appears nowhere in the file"),
        "the fallback that named no location is the whole of the gap: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        file,
        "a refused batch leaves the file alone"
    );
}

/// The join is licensed by Rust's grammar: a line whose first non-whitespace
/// character is `.` cannot begin a statement there, so it continues the one
/// above. In prose or markup a line can begin with a full stop and mean it, so
/// the rescue stays off and the ordinary refusal stands.
#[test]
fn a_flattened_chain_in_a_file_with_no_such_grammar_is_not_rescued() {
    let (_temp, root) = setup_root();
    let file = "step one\n    .values_mut()\n    .next()\n    .unwrap_or(the default value)\n";
    fs::write(root.join("notes.txt"), file).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "notes.txt",
            "edits": [{
                "oldString": "step one.values_mut().next().unwrap_or(the default value)",
                "newString": "step one joined"
            }]
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a `.` line in prose is not a continuation".to_string());

    assert!(!err.contains("ignoring its line breaks"), "{err}");
    assert_eq!(
        fs::read_to_string(root.join("notes.txt")).unwrap(),
        file,
        "a refused batch leaves the file alone"
    );
}

/// `lr7b_v23` turn 4 in full shape. The `oldString` matched thirteen lines and
/// the `newString` opened with two the file already carried immediately above
/// them, so the splice wrote those two twice and rustc answered with
/// `E0382: use of moved value: user` and `use of moved value: record` — two
/// errors the file did not have before the edit. The tool said "Success: applied
/// 1 edit(s)", so nothing sent the model looking for them. Its next two turns
/// tried to fix the pair and were refused because the duplicated line by then
/// matched two places, its next three were byte-identical `read_file` calls, and
/// its last produced no tool call at all.
#[test]
fn an_edit_whose_replacement_copies_the_lines_above_its_site_writes_them_once() {
    let (_temp, root) = setup_root();
    let file = "pub struct M;\n\nimpl M {\n    pub fn update(&mut self, user: &str, record: Record) {\n        self.user_dict.insert(user, record);\n        let _ = self.save_data();\n        let total = self.total_for(user);\n        save(total)\n    }\n}\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "        let total = self.total_for(user);\n        save(total)",
                "newString": "        self.user_dict.insert(user, record);\n        let _ = self.save_data();\n        let total = self.total_for(user);\n        let logged = save(total);\n        logged"
            }],
            "context": "fix E0308"
        }),
    )
    .unwrap_or_else(|e| panic!("the site is unique and the change is in the payload: {e}"));

    assert!(
        ok.contains("2 line(s) the file already has above the text it replaced"),
        "the payload it sent is not the text that was written, so it has to be told: {ok}"
    );
    assert!(
        ok.contains("do not re-send it"),
        "or it will send the two lines back: {ok}"
    );
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert_eq!(
        after
            .matches("self.user_dict.insert(user, record);")
            .count(),
        1,
        "v23 wrote this line twice and the run died on the E0382 that followed: {after}"
    );
    assert_eq!(
        after.matches("let _ = self.save_data();").count(),
        1,
        "{after}"
    );
    assert!(after.contains("let logged = save(total);"), "{after}");
    assert!(
        !after.contains("\n        save(total)\n"),
        "the change still went in: {after}"
    );
}

/// What is left of a replacement once the file's own lines around the site are
/// taken out of it can be nothing, and then there is no honest way to write it:
/// untrimmed it duplicates every line it carries, trimmed it deletes the text it
/// matched. Both are silent, and a silent write is what v23's turn 4 was.
#[test]
fn a_replacement_of_nothing_but_the_file_around_its_site_is_refused() {
    let (_temp, root) = setup_root();
    let file = "pub struct M;\n\nimpl M {\n    pub fn total(&self) -> f64 {\n        let a = self.a();\n        let b = self.b();\n        let c = self.c();\n        a + b + c\n    }\n}\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "        let c = self.c();",
                "newString": "        let a = self.a();\n        let b = self.b();"
            }]
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "every line of it is the file's own".to_string());

    assert!(
        err.contains("nothing but 2 line(s) the file already carries"),
        "{err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        file,
        "a refused batch leaves the file alone"
    );
}

/// Unlike the rescues that forgive a flattened chain, this one needs no grammar:
/// a line the file already has beside the span is a duplicate in any language.
#[test]
fn copied_context_is_dropped_from_a_file_with_no_rust_grammar_too() {
    let (_temp, root) = setup_root();
    let file = "def update(user, record):\n    user_dict[user] = record\n    save_data()\n    total = total_for(user)\n    return total\n";
    fs::write(root.join("port.py"), file).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.py",
            "edits": [{
                "oldString": "    total = total_for(user)",
                "newString": "    user_dict[user] = record\n    save_data()\n    total = total_for(user)\n    logged = save(total)"
            }]
        }),
    )
    .unwrap_or_else(|e| panic!("the site is unique: {e}"));

    assert!(
        ok.contains("2 line(s) the file already has above the text it replaced"),
        "{ok}"
    );
    let after = fs::read_to_string(root.join("port.py")).unwrap();
    assert_eq!(
        after.matches("user_dict[user] = record").count(),
        1,
        "{after}"
    );
    assert!(after.contains("logged = save(total)"), "{after}");
}

/// A batch that changes the file's brace balance is refused, because that is
/// never what a patch means. What has to be counted is the text the splice
/// writes, not the text the model sent: the two lines this `newString` copies
/// from above its site carry a closing brace, and they are exactly the part that
/// does not get written.
#[test]
fn a_batch_is_not_refused_for_a_brace_only_the_copied_context_carried() {
    let (_temp, root) = setup_root();
    let file = "fn f() -> i32 {\n    let a = {\n        1\n    };\n    let c = 3;\n    a + c\n}\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    let c = 3;",
                "newString": "        1\n    };\n    let c = compute();"
            }]
        }),
    )
    .unwrap_or_else(|e| panic!("the brace that unbalances the payload is never written: {e}"));

    assert!(
        !ok.contains("no block to close"),
        "the sent payload reads as one brace short, the file is not: {ok}"
    );
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert!(after.contains("let c = compute();"), "{after}");
    assert!(
        after.matches("let a = ").count() == 1 && after.matches("a + c").count() == 1,
        "counting glyphs in source that carries them as text proves nothing: {after}"
    );
}

/// Both rescues write through the same splice, so both have to drop the copied
/// lines too — a flattened chain is the shape most likely to arrive with its
/// context still attached, because the model is copying a region it just read.
#[test]
fn a_rescued_edit_drops_the_context_it_copied_as_well() {
    let (_temp, root) = setup_root();
    let file = "    pub fn update(&mut self, user: &str, record: Record) {\n        self.user_dict.insert(user, record);\n        let _ = self.save_data();\n        self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut Rec { a: 0.0 })\n    }\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "        self.user_dict.values_mut().next().unwrap_or(&mut Rec { a: 0.0 })",
                "newString": "        self.user_dict.insert(user, record);\n        let _ = self.save_data();\n        let r = self.user_dict.values_mut().next().unwrap_or(&mut Rec { a: 0.0 });\n        save(r)"
            }]
        }),
    )
    .unwrap_or_else(|e| panic!("the chain and the copy are both forgiven: {e}"));

    assert!(ok.contains("ignoring its line breaks"), "{ok}");
    assert!(
        ok.contains("2 line(s) the file already has above the text it replaced"),
        "the join is not the only thing this payload got wrong: {ok}"
    );
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert_eq!(
        after
            .matches("self.user_dict.insert(user, record);")
            .count(),
        1,
        "{after}"
    );
    assert!(after.contains("let r = self"), "{after}");
    assert!(after.contains("save(r)"), "{after}");
}

/// The same through the indent rescue, where the replacement is re-indented
/// before the copy is looked for — so the comparison has to be on the code and
/// not on the whitespace, or the re-indented copy would slip past it.
#[test]
fn a_reindented_edit_drops_the_context_it_copied_as_well() {
    let (_temp, root) = setup_root();
    let file = "fn update(&mut self, k: &str) {\n        self.dict.insert(k, rec);\n        let _ = self.save();\n        let total = self.total(k);\n        save(total)\n}\n";
    fs::write(root.join("port.rs"), file).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    let total = self.total(k);\n    save(total)",
                "newString": "    self.dict.insert(k, rec);\n    let _ = self.save();\n    let total = self.total(k);\n    let logged = save(total);\n    logged"
            }]
        }),
    )
    .unwrap_or_else(|e| panic!("the shift and the copy are both forgiven: {e}"));

    assert!(ok.contains("ignoring leading whitespace"), "{ok}");
    assert!(
        ok.contains("2 line(s) the file already has above the text it replaced"),
        "{ok}"
    );
    let after = fs::read_to_string(root.join("port.rs")).unwrap();
    assert_eq!(
        after.matches("self.dict.insert(k, rec);").count(),
        1,
        "{after}"
    );
    assert_eq!(after.matches("let _ = self.save();").count(), 1, "{after}");
    assert!(after.contains("let logged = save(total);"), "{after}");
}

/// `lr7b_v19` in full shape: the `oldString` is the file's own region with one
/// extra character on its last line, and the `newString` is the correct fix for
/// the error under repair. Refusing it as "not in the file" and pasting the
/// sites of its FIRST line shows only the part that already matched, so the
/// model re-sent the byte-identical call five times and the run ended with the
/// error still standing. The line that differs has to be named, from both sides.
#[test]
fn a_refused_edit_one_character_off_names_the_line_that_differs() {
    let (_temp, root) = setup_root();
    let file = r#"    pub fn update(&mut self, user: &str, record: Record) {
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
    fs::write(root.join("port.rs"), file).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "        self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut PersistedUserRecord {\n                total_budget: 0.0,\n                current_cost: 0.0,\n            });\n",
                "newString": "        let result_record = self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut PersistedUserRecord {\n                total_budget: 0.0,\n                current_cost: 0.0,\n            });\n"
            }],
            "context": "fix E0515"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "the one-character-off edit must be refused".to_string());

    assert!(err.contains("not in port.rs"), "{err}");
    assert!(
        err.contains("6 of its 7 non-blank lines"),
        "the refusal must say how much of the edit was right: {err}"
    );
    assert!(
        err.contains("you wrote `});`") && err.contains("the file says `})`"),
        "and name the divergent line from both sides: {err}"
    );
    assert!(
        err.contains("read from line 4"),
        "and where the matching region starts: {err}"
    );
    // The sites paste answered this call before and taught nothing: its first
    // line is in the file, so the paste was all correct text and no divergence.
    assert!(
        !err.contains("different leading whitespace"),
        "the near-miss answer must take the place of the sites paste: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        file,
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

/// `lr7b_v34` sent one two-edit batch four times. Edit 1 replaced a real line;
/// edit 2 handed back the text it matched. The no-op refused the batch, so the
/// real edit was never evaluated in any of the four turns, two refunds went on
/// re-sending it, and the run reached 3/3 compile-repair turns with both of the
/// errors it started with still standing. A no-op writes no bytes, so it cannot
/// be the reason the edit beside it did not land.
#[test]
fn a_no_op_edit_does_not_veto_the_real_edit_beside_it() {
    let (_temp, root) = setup_root();
    let before = "fn charge(&mut self, user: String, record: UsageRecord) -> f64 {\n    \
                  let result_record = record.clone();\n    self.user_dict.insert(user, record);\n    \
                  &result_record\n}\n";
    fs::write(root.join("port.txt"), before).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.txt",
            "edits": [
                {
                    "oldString": "    self.user_dict.insert(user, record);",
                    "newString": "    self.user_dict.entry(user).or_insert_with(|| record);"
                },
                {
                    "oldString": "    &result_record",
                    "newString": "    &result_record"
                }
            ],
            "context": "one real change and one that is not a change"
        }),
    )
    .unwrap_or_else(|e| panic!("the no-op was the only thing wrong with this batch: {e}"));

    assert!(ok.starts_with("Success"), "{ok}");
    assert!(
        ok.contains("applied 1 edit(s)"),
        "one edit wrote bytes and one did not: {ok}"
    );
    assert!(
        ok.contains("Left out as changing no characters: edit 2"),
        "the model has to be told which of its own edits was dropped: {ok}"
    );
    assert!(
        ok.contains("still has to be written"),
        "and that dropping it did not do it: {ok}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.txt")).unwrap(),
        before.replace(
            "    self.user_dict.insert(user, record);",
            "    self.user_dict.entry(user).or_insert_with(|| record);"
        ),
        "every byte of the surviving edit lands and nothing else moves"
    );
}

/// Dropping a no-op is not the same as allowing a batch that would write the
/// file back to itself. There is nothing to apply in that case, so the file is
/// left alone and the refusal keeps the wording `replay_advice` branches on.
#[test]
fn a_batch_of_nothing_but_no_ops_is_still_refused_and_names_each_one() {
    let (_temp, root) = setup_root();
    let src = "fn a() {\n    let x = 1;\n}\n\nfn b() {\n    let y = 2;\n}\n";
    fs::write(root.join("port.txt"), src).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.txt",
            "edits": [
                { "oldString": "    let x = 1;", "newString": "    let x = 1;" },
                { "oldString": "    let y = 2;", "newString": "    let y = 2;" }
            ],
            "context": "two intents with no patch between them"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a batch that writes the file back to itself is not a success".to_string());

    assert!(err.contains("no edits were applied"), "{err}");
    assert!(
        err.contains("byte-identical"),
        "the phrase `replay_advice` keys on: {err}"
    );
    assert!(
        err.contains("change no characters"),
        "and why that is not progress: {err}"
    );
    assert!(err.contains("edit 1:"), "{err}");
    assert!(
        err.contains("edit 2:"),
        "a second no-op is a second line, not a summary: {err}"
    );
    assert!(
        !err.contains("Left out"),
        "the note belongs to a batch that wrote something: {err}"
    );
    assert_eq!(fs::read_to_string(root.join("port.txt")).unwrap(), src);
}

/// The count in the success message is the number of edits that reached the
/// file. `edits.len()` would claim three and hide the drop; the note would then
/// be the only thing contradicting the headline.
#[test]
fn the_applied_count_leaves_out_the_edits_that_wrote_nothing() {
    let (_temp, root) = setup_root();
    let src = "fn f() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n}\n";
    fs::write(root.join("port.txt"), src).unwrap();

    let ok = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.txt",
            "edits": [
                { "oldString": "    let a = 1;", "newString": "    let a = 9;" },
                { "oldString": "    let b = 2;", "newString": "    let b = 2;" },
                { "oldString": "    let c = 3;", "newString": "    let c = 9;" }
            ],
            "context": "two changes with a nothing between them"
        }),
    )
    .unwrap_or_else(|e| panic!("{e}"));

    assert!(
        ok.contains("applied 2 edit(s)"),
        "not 3, which is what was sent, and not 1, which is what was dropped: {ok}"
    );
    assert!(
        ok.contains("Left out as changing no characters: edit 2"),
        "{ok}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.txt")).unwrap(),
        "fn f() {\n    let a = 9;\n    let b = 2;\n    let c = 9;\n}\n"
    );
}

/// The brace-drift refusal counts the batch that reached the gate. A no-op
/// contributes no braces, so counting it would blame the drift on a batch of two
/// when one edit caused it and the other wrote nothing.
#[test]
fn a_no_op_is_not_counted_in_the_brace_drift_it_did_not_cause() {
    let (_temp, root) = setup_root();
    let src = "fn a() {\n    let x = 1;\n}\n\nfn b() {\n    let y = 2;\n}\n";
    fs::write(root.join("port.rs"), src).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [
                { "oldString": "    let y = 2;", "newString": "    let y = 2;" },
                { "oldString": "fn a() {\n    let x = 1;\n}", "newString": "fn a() {\n    let x = 2;" }
            ],
            "context": "one nothing and one dropped closer"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a dropped closer has to be refused".to_string());

    assert!(
        err.contains("batch of 1 edit(s)"),
        "the drift is one edit's, so the count is one: {err}"
    );
    assert!(
        !err.contains("batch of 2 edit(s)"),
        "and the no-op is not folded into it: {err}"
    );
    assert!(err.contains("block left open"), "{err}");
    assert_eq!(fs::read_to_string(root.join("port.rs")).unwrap(), src);
}

/// A batch is still all-or-nothing for anything actually refused. Dropping a
/// no-op is a licence to write the edits that meant something, not to write half
/// of a batch that contains a mistake.
#[test]
fn a_refused_edit_still_refuses_the_batch_a_no_op_sits_in() {
    let (_temp, root) = setup_root();
    let src = "fn a() {\n    let x = 1;\n}\n\nfn b() {\n    let x = 2;\n}\n";
    fs::write(root.join("port.txt"), src).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.txt",
            "edits": [
                { "oldString": "    let x = 1;", "newString": "    let x = 1;" },
                { "oldString": "    let x = ", "newString": "    let q = " }
            ],
            "context": "one nothing and one that cannot be located"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "an edit that matches two places still refuses its batch".to_string());

    assert!(err.contains("no edits were applied"), "{err}");
    assert!(
        err.contains("edit 2:"),
        "the refused edit is named by its own number: {err}"
    );
    assert!(
        err.contains("matches 2 places"),
        "and by its own reason: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.txt")).unwrap(),
        src,
        "a refused batch leaves every byte alone, no-op or not"
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

/// The same gate drifted the other way, which is where `lr7b_v29` spent its
/// first five turns. The `oldString` stops above the lines that close the block
/// it opens and the `newString` closes it anyway, so the file's own `} else {`
/// and `};` are left standing with nothing to close. The refusal has to send the
/// model after the `oldString`: the note both directions used to share sent it
/// after more closers, and the count came back 1, 2, 2, 3.
#[test]
fn an_edit_that_closes_a_block_it_never_covered_is_told_to_widen() {
    let (_temp, root) = setup_root();
    let before = "fn f() -> i32 {\n    let a = if c {\n        1\n    } else {\n        2\n    };\n    a\n}\n";
    fs::write(root.join("port.rs"), before).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    let a = if c {\n        1",
                "newString": "    let a = if c {\n        1\n    } else {\n        9\n    };"
            }],
            "context": "change the branch"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "the closers below the site are still in the file".to_string());

    assert!(err.contains("no block to close"), "{err}");
    assert!(
        err.contains("Widen the `oldString`"),
        "and say which side of the edit to change: {err}"
    );
    assert!(
        !err.contains("carrying every delimiter"),
        "that is the advice the model obeyed four times: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        before,
        "a refused batch leaves every byte alone"
    );
}

/// v30's turn 3: the `oldString` is the file's own site with rustfmt's line
/// breaks undone and a `;` added to its last line. The tool can fold that site
/// and did, two turns earlier, for the same needle without the `;`, so the
/// answer it gave instead -- "appears nowhere in the file even with indentation
/// ignored", with no line named -- was both false and unusable. The run spent
/// turns 3 through 9 on it: two refusals alternating with two `read_file` calls
/// over the same fourteen lines.
///
/// It has to name the span and still refuse. A trailing `;` is the difference
/// between a statement and the value a block returns, so an `oldString` that
/// adds one asks for a change no tool can tell apart from the patch around it.
#[test]
fn an_oldstring_one_semicolon_off_a_folded_site_is_refused_at_the_line() {
    let (_temp, root) = setup_root();
    let before = "impl M {\n    fn f(&mut self) {\n        self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut PersistedUserRecord {\n                total_budget: 0.0,\n                current_cost: 0.0,\n            })\n    }\n}\n";
    fs::write(root.join("port.rs"), before).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "        self.user_dict.values_mut().next().unwrap_or(&mut PersistedUserRecord {\n            total_budget: 0.0,\n            current_cost: 0.0,\n        });",
                "newString": "        self.user_dict.insert(user, record);"
            }],
            "context": "insert instead of defaulting"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a `;` no rescuer may move has to be refused".to_string());

    assert!(err.contains("lines 3-9"), "the span it was aimed at: {err}");
    assert!(err.contains("the file says `})`"), "{err}");
    assert!(
        !err.contains("appears nowhere"),
        "the fold located it, so that claim is false: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        before,
        "a refused batch leaves every byte alone"
    );
}

/// The same refusal with a `newString` that hands `superseded_site` an anchor:
/// its first line is one the file already carries, exactly once. That answer is
/// a guess about history -- "an earlier write of yours replaced what the
/// `oldString` quotes" -- and nothing was replaced. The locator read the site
/// out of the file as it stands, so it wins, which is what the `located` gate is
/// for; v24 spent two turns on a phantom of exactly this shape.
#[test]
fn a_located_terminator_outranks_the_history_superseded_site_infers() {
    let (_temp, root) = setup_root();
    let before = "impl M {\n    fn f(&mut self) {\n        let _ = self.save_data();\n        self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut PersistedUserRecord {\n                total_budget: 0.0,\n                current_cost: 0.0,\n            })\n    }\n}\n";
    fs::write(root.join("port.rs"), before).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "        self.user_dict.values_mut().next().unwrap_or(&mut PersistedUserRecord {\n            total_budget: 0.0,\n            current_cost: 0.0,\n        });",
                "newString": "        let _ = self.save_data();\n        self.user_dict.insert(user, record);"
            }],
            "context": "insert instead of defaulting"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a `;` no rescuer may move has to be refused".to_string());

    assert!(err.contains("statement terminator"), "{err}");
    assert!(err.contains("lines 4-10"), "{err}");
    assert!(
        !err.contains("already been changed"),
        "the site is standing in the file; nothing superseded it: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        before,
        "a refused batch leaves every byte alone"
    );
}

/// Both locators find this site: nothing had to be folded, and the needle's last
/// line is one `;` off the file's. `nearest_miss` also says how much of the edit
/// was already right, which the terminator diagnosis cannot, so the chain pushes
/// that one and this one stays out of the refusal. The ranking is the push order
/// and nothing else -- both are computed, because a fold that found the site is
/// proof for `located` even when the near miss matched too few lines to be.
#[test]
fn a_near_miss_that_found_the_line_outranks_the_terminator_diagnosis() {
    let (_temp, root) = setup_root();
    let before = "fn a() {\n    let alpha = compute_one();\n    let beta = compute_two();\n    gamma(beta)\n}\n";
    fs::write(root.join("port.rs"), before).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    let alpha = compute_one();\n    let beta = compute_two();\n    gamma(beta);",
                "newString": "    let alpha = compute_one();\n    let beta = compute_two();\n    gamma(beta + alpha);"
            }],
            "context": "fold the two values into the call"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a `;` no rescuer may move has to be refused".to_string());

    assert!(
        err.contains("2 of its 3 non-blank lines"),
        "the near miss says how much of the edit was already right: {err}"
    );
    assert!(
        !err.contains("statement terminator"),
        "and the diagnosis that says less must not displace it: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        before,
        "a refused batch leaves every byte alone"
    );
}

/// A near miss this loose does not locate its own site -- two of three lines is
/// under the three `locates_the_site` asks for -- so alone it would hand the
/// refusal to `superseded_site`, which infers a rewrite from the line this
/// `newString` copies out of `flush`. The terminator fold did read the file and
/// did find the site, so it is the proof that keeps the inference out, and it
/// does that work without taking over the answer: the near miss says more and is
/// pushed first. v24 is the run that made `located` a gate at all.
#[test]
fn a_loose_near_miss_does_not_let_history_outrank_a_located_terminator() {
    let (_temp, root) = setup_root();
    let before = "fn a() {\n    let alpha = compute_one();\n    let beta = compute_two();\n    emit_the_total(beta)\n}\n\nfn flush(store: &mut Store) {\n    let _ = store.save_data();\n}\n";
    fs::write(root.join("port.rs"), before).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    let alpha = compute_one();\n    let beta = compute_two();\n    emit_the_total(beta);",
                "newString": "    let alpha = compute_one();\n    let beta = compute_two();\n    let _ = store.save_data();\n    emit_the_total(alpha);"
            }],
            "context": "persist before emitting"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a `;` no rescuer may move has to be refused".to_string());

    assert!(
        err.contains("2 of its 3 non-blank lines"),
        "the near miss answers, and it answers out of the file: {err}"
    );
    assert!(
        !err.contains("already been changed"),
        "the fold found the site, so the inferred history stays out: {err}"
    );
    assert!(
        !err.contains("statement terminator"),
        "and proving the site is not the same as taking over the answer: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        before,
        "a refused batch leaves every byte alone"
    );
}

/// The fold is Rust's grammar and the refusal cites Rust's rule about a trailing
/// `;`, so neither belongs to a file in another language. The same needle
/// against the same shape of JavaScript has to get the ordinary answer, not a
/// diagnosis quoting semantics that file does not have.
#[test]
fn a_terminator_diagnosis_is_not_offered_to_a_language_it_cites_rules_of() {
    let (_temp, root) = setup_root();
    let before = "function f(records) {\n    const rec = records\n        .values()\n        .next()\n        .orElseGet(() => ({\n            total_budget: 0.0,\n            current_cost: 0.0,\n        }))\n}\n";
    fs::write(root.join("port.js"), before).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.js",
            "edits": [{
                "oldString": "    const rec = records.values().next().orElseGet(() => ({\n        total_budget: 0.0,\n        current_cost: 0.0,\n    }));",
                "newString": "    const rec = records.insert(user);"
            }],
            "context": "insert instead of defaulting"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "the flattened needle is not in the file".to_string());

    assert!(
        !err.contains("statement terminator"),
        "Rust's rule, cited at a file that does not have it: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.js")).unwrap(),
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

/// The formatter runs after the write, so the buffer the splice left behind is
/// not the file. `lr7b_v31` was told "(137 → 163 bytes)" about a file that was
/// 171 — eight bytes out, which is exactly what rustfmt added — and it was told
/// that in the same message that said "rustfmt reformatted this file, so its
/// bytes differ from what you sent". A model reads the count to check its write
/// landed; a count that disagrees with the disk teaches it to ignore the count.
///
/// This test needs `rustfmt` on PATH and format-on-write enabled (the default) to
/// mean anything, and says so out loud rather than passing on a no-op format.
#[test]
fn the_byte_count_in_the_success_message_is_the_file_rustfmt_left_behind() {
    let (_temp, root) = setup_root();
    let before = "pub fn  add(a:u8,b:u8)->u8{a+b}\n";
    fs::write(root.join("port.rs"), before).unwrap();

    let out = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "pub fn  add(a:u8,b:u8)->u8{a+b}",
                "newString": "pub fn  add(a:u8,b:u8)->u16{(a+b) as u16}"
            }],
            "context": "widen the result"
        }),
    )
    .unwrap();

    let on_disk = fs::read_to_string(root.join("port.rs")).unwrap();
    assert!(
        on_disk.contains("pub fn add(a: u8, b: u8) -> u16 {"),
        "the formatter has to have run for this to test anything; the file is {on_disk:?}"
    );
    assert!(
        out.contains("rustfmt reformatted this file"),
        "and the message has to be the one that admits the bytes moved: {out}"
    );
    assert!(
        out.contains(&format!(
            "({} \u{2192} {} bytes)",
            before.len(),
            on_disk.len()
        )),
        "the count has to be the file's, not the buffer's: {out}"
    );
}

/// `lr7b_v32`'s file at the E0499, reduced to the region that matters.
const HOLED_FILE: &str = "impl BudgetManager {\n    fn update_cost(&mut self, user: &str) -> f64 {\n        let record = self.input(user);\n        let _ = self.save_data();\n        let result_record =\n            self.user_dict\n                .values_mut()\n                .next()\n                .unwrap_or(&mut PersistedUserRecord {\n                    total_budget: 0.0,\n                    current_cost: 0.0,\n                });\n        self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut *result_record)\n    }\n}\n";

/// v32's `oldString`: the file's lines 6 to 12, then its line 17. Lines 13 to
/// 16 are the hole.
const HOLED_NEEDLE: &str = "            self.user_dict\n                .values_mut()\n                .next()\n                .unwrap_or(&mut PersistedUserRecord {\n                    total_budget: 0.0,\n                    current_cost: 0.0,\n                });\n    }";

/// v32's `newString`: the binding folded onto one line, then every line of the
/// hole written back and the `}` under it.
const HOLED_PATCH: &str = "        let result_record = self.user_dict.values_mut().next().unwrap_or(&mut PersistedUserRecord {\n            total_budget: 0.0,\n            current_cost: 0.0,\n        });\n        self.user_dict\n            .values_mut()\n            .next()\n            .unwrap_or(&mut *result_record)\n    }";

/// Turns 5, 6, 8 and 10 of `lr7b_v32` sent this one call, byte for byte, and
/// each attempt was answered "11 of its 12 non-blank lines are in the file in
/// that order and the next one is not. Its line 12 is where they part — you wrote
/// `}`, the file says `self.user_dict`. Send the same edit with the file's
/// version of that one line." A true sentence about a false shape: the needle's
/// last line is not wrong, it is four lines further down, and the swap that
/// sentence prescribes builds an `oldString` covering 288 to 299 against a
/// `newString` written for 288 to 303. The run re-sent the call instead of
/// acting on it, hit 3/3 compile-repair turns with 0 refunds left, and ended with
/// the E0499 it started with.
#[test]
fn a_needle_with_a_hole_is_refused_at_the_block_it_stepped_over() {
    let (_temp, root) = setup_root();
    fs::write(root.join("port.rs"), HOLED_FILE).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": HOLED_NEEDLE,
                "newString": HOLED_PATCH
            }],
            "context": "bind the record before borrowing the map again"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a needle with a hole in it cannot match, so it has to be refused".into());

    assert!(err.contains("hole in it"), "{err}");
    assert!(
        err.contains("7 of its 8 non-blank lines are the file's own, lines 6 to 12"),
        "the run it did quote, by line: {err}"
    );
    assert!(
        err.contains("    13|         self.user_dict"),
        "and the lines it stepped over, quoted with their own indentation, because the model has \
         to paste them into an `oldString` that is matched byte for byte: {err}"
    );
    assert!(
        err.contains("    16|             .unwrap_or(&mut *result_record)"),
        "all of them, not a summary: {err}"
    );
    assert!(
        err.contains("lines 6 to 17 and copy that whole run in"),
        "a span with a floor and a ceiling, not a region to hunt for: {err}"
    );
    assert!(
        !err.contains("is where they part"),
        "the prefix run's answer has to stay out: it is the one that prescribes the swap that \
         would delete lines 14 to 17: {err}"
    );
    assert!(
        !err.contains("already on disk"),
        "and neither does the already-applied refusal, which is about a `newString` the file \
         carries and this one does not: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        HOLED_FILE,
        "a refused batch leaves every byte alone"
    );
}

/// The hole locates the site — one head, one witnessed resumption, both by line
/// — so it is proof, and proof keeps `superseded_site`'s inferred history out.
/// This fixture is the shape where that matters: three of six lines matched,
/// which is under the three `NearMiss::locates_the_site` asks for *and* leaves
/// two of the needle unexamined past the divergence, so the near miss alone does
/// not locate anything. Without the hole in `located`, this refusal belongs to a
/// guess about a write that never happened.
#[test]
fn a_located_hole_keeps_history_from_outranking_the_span_it_found() {
    let (_temp, root) = setup_root();
    let before = "fn a() {\n    let kept = self.take();\n    let used = self.give();\n    let total = self.sum();\n    log(kept);\n    save(kept);\n    done(used);\n}\n\nfn flush(store: &mut Store) {\n    let _ = store.save_data();\n}\n";
    fs::write(root.join("port.rs"), before).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": "    let kept = self.take();\n    let used = self.give();\n    let total = self.sum();\n    save(kept);\n    done(used);\n}",
                "newString": "    let _ = store.save_data();\n    log(kept);\n    save(kept);\n    done(used);\n}"
            }],
            "context": "persist before saving"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "the `oldString` steps over line 5, so it cannot match".into());

    assert!(err.contains("hole in it"), "read out of the file: {err}");
    assert!(
        err.contains("    5|     log(kept);"),
        "and naming the one line it stepped over: {err}"
    );
    assert!(
        !err.contains("already been changed"),
        "nothing was changed; the `newString` merely copies a line out of `flush`: {err}"
    );
    assert!(!err.contains("is where they part"), "{err}");
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        before,
        "every byte alone"
    );
}

/// The witness is what makes the span a proof rather than a guess, so without it
/// the diagnosis has to fall back to the one that is proved: how many lines
/// matched, and which one differs. Naming a hole the `newString` does not write
/// back would tell the model to widen into a span its own replacement deletes.
#[test]
fn a_hole_the_replacement_does_not_write_back_falls_through_to_the_line_that_differs() {
    let (_temp, root) = setup_root();
    fs::write(root.join("port.rs"), HOLED_FILE).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [{
                "oldString": HOLED_NEEDLE,
                "newString": "        let result_record = self.user_dict.values_mut().next().unwrap_or_default();\n        save(result_record)"
            }],
            "context": "drop the literal"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "the `oldString` is not in the file".into());

    assert!(
        !err.contains("hole in it"),
        "with nothing tying the tail to the head, a span would be a guess: {err}"
    );
    assert!(
        err.contains("7 of its 8 non-blank lines"),
        "so the prefix run answers, which is the most that is proved here: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        HOLED_FILE,
        "every byte alone"
    );
}

/// A batch is all-or-nothing, and the hole is a refusal like any other: the
/// second edit is never reached and nothing is written.
#[test]
fn a_holed_edit_refuses_its_whole_batch_and_leaves_the_other_edit_out() {
    let (_temp, root) = setup_root();
    fs::write(root.join("port.rs"), HOLED_FILE).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.rs",
            "edits": [
                { "oldString": HOLED_NEEDLE, "newString": HOLED_PATCH },
                {
                    "oldString": "        let record = self.input(user);",
                    "newString": "        let record = self.input(user).clone();"
                }
            ],
            "context": "two changes, one of them holed"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "one refused edit refuses the batch".into());

    assert!(err.contains("no edits were applied"), "{err}");
    assert!(
        err.contains("edit 1: its `oldString` is not in port.rs, and it is not one line wrong"),
        "{err}"
    );
    assert!(
        !err.contains("edit 2:"),
        "the second edit is never reached: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.rs")).unwrap(),
        HOLED_FILE,
        "every byte alone"
    );
}

/// A hole is a fact about line numbers, not about a grammar. The refusal quotes
/// no language's rules — unlike the terminator fold, which cites Rust's — so it
/// is not gated on the file being Rust, and the same shape in another language
/// has to get the same answer.
#[test]
fn a_hole_is_named_in_a_file_whose_grammar_the_diagnosis_does_not_cite() {
    let (_temp, root) = setup_root();
    fs::write(root.join("port.js"), HOLED_FILE).unwrap();

    let err = call_tool_in_workspace(
        &root,
        "edit_file",
        &json!({
            "relativeFilePath": "port.js",
            "edits": [{
                "oldString": HOLED_NEEDLE,
                "newString": HOLED_PATCH
            }],
            "context": "the same shape in another language"
        }),
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_else(|| "a hole is a hole in any language".into());

    assert!(err.contains("hole in it"), "{err}");
    assert!(
        err.contains("    13|         self.user_dict"),
        "with the same lines quoted: {err}"
    );
    assert_eq!(
        fs::read_to_string(root.join("port.js")).unwrap(),
        HOLED_FILE,
        "every byte alone"
    );
}
