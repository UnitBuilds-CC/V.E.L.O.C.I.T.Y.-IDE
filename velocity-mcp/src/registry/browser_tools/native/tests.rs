//! Integration tests for the native browser tool family.
//!
//! Exercised through `handle_native_tool` end to end: every test loads the
//! shared form fixture into its own session id and asserts on the readable
//! or compact tool output.

use super::*;
use serde_json::json;

const FORM_HTML: &str = r#"<html><head><title>Signup</title></head><body>
        <form id="f">
          <input type="text" placeholder="Email" name="email" />
          <input type="checkbox" aria-label="Subscribe" />
          <select aria-label="Plan">
            <option value="free">Free</option>
            <option value="pro">Pro</option>
          </select>
          <button type="submit">Log In</button>
        </form>
    </body></html>"#;

/// Each test uses its own session id: bridges are process-global by id.
fn load(session: &str) {
    let bridge = get_or_create_native_bridge(session);
    bridge
        .lock()
        .unwrap()
        .load_html("http://local.test/form", FORM_HTML);
}

fn call(name: &str, args: serde_json::Value) -> String {
    handle_native_tool(Path::new("."), name, &args)
        .expect("tool call succeeds")
        .expect("native tool name is handled")
}

fn call_err(name: &str, args: serde_json::Value) -> Box<dyn Error> {
    handle_native_tool(Path::new("."), name, &args).expect_err("tool call fails")
}

#[test]
fn click_text_tool_acts_and_reports_observation() {
    load("t17-click");
    let out = call(
        "browser_native_click_text",
        json!({ "sessionId": "t17-click", "text": "Log In" }),
    );
    assert!(
        out.contains("clicked"),
        "status should report the click: {out}"
    );
    assert!(
        out.contains("Changes:"),
        "action output must include the delta section"
    );
    assert!(
        out.contains("URL:"),
        "action output must include the refreshed view"
    );
}

#[test]
fn fill_label_then_read_form_shows_typed_value() {
    load("t17-fill");
    let out = call(
        "browser_native_fill_label",
        json!({ "sessionId": "t17-fill", "label": "Email", "text": "a@b.c" }),
    );
    assert!(
        out.contains("node_"),
        "fill should resolve a concrete node: {out}"
    );
    let form = call(
        "browser_native_read_form",
        json!({ "sessionId": "t17-fill" }),
    );
    assert!(
        form.contains("a@b.c"),
        "read_form must show the typed value: {form}"
    );
    assert!(
        form.contains("unchecked"),
        "read_form must show checkbox state: {form}"
    );
}

#[test]
fn check_and_select_label_tools_update_form_state() {
    load("t17-check");
    let out = call(
        "browser_native_check_label",
        json!({ "sessionId": "t17-check", "label": "Subscribe" }),
    );
    assert!(out.contains("checked"), "check status: {out}");
    let out = call(
        "browser_native_select_label",
        json!({ "sessionId": "t17-check", "label": "Plan", "option": "Pro" }),
    );
    assert!(out.contains("selected 'pro'"), "select status: {out}");
    let form = call(
        "browser_native_read_form",
        json!({ "sessionId": "t17-check" }),
    );
    assert!(form.contains("checked"), "form shows checked state: {form}");
    assert!(form.contains("pro"), "form shows selected value: {form}");
}

#[test]
fn focus_label_and_press_drive_session_keyboard() {
    load("t17-press");
    let miss = call(
        "browser_native_press",
        json!({ "sessionId": "t17-press", "key": "x" }),
    );
    assert!(
        miss.contains("nothing focused"),
        "press without focus: {miss}"
    );
    let out = call(
        "browser_native_focus_label",
        json!({ "sessionId": "t17-press", "label": "Email" }),
    );
    assert!(out.contains("focused"), "focus status: {out}");
    let out = call(
        "browser_native_press",
        json!({ "sessionId": "t17-press", "key": "z" }),
    );
    assert!(out.contains("pressed"), "press status: {out}");
    let form = call(
        "browser_native_read_form",
        json!({ "sessionId": "t17-press" }),
    );
    assert!(
        form.contains('z'),
        "pressed character lands in the control: {form}"
    );
}

#[test]
fn observe_and_settle_tools_return_readable_state() {
    load("t17-observe");
    let facts = call(
        "browser_native_observe",
        json!({ "sessionId": "t17-observe" }),
    );
    assert!(
        facts.contains("http://local.test/form"),
        "observe includes url: {facts}"
    );
    assert!(
        facts.contains("button"),
        "observe includes AOM roles: {facts}"
    );
    let out = call(
        "browser_native_settle",
        json!({ "sessionId": "t17-observe" }),
    );
    assert!(out.contains("settled"), "settle status: {out}");
}

#[test]
fn compact_flag_returns_json_action_report() {
    load("t17-compact");
    let out = call(
        "browser_native_click_text",
        json!({ "sessionId": "t17-compact", "text": "Log In", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("compact output is valid JSON");
    assert!(report["status"].as_str().unwrap().contains("clicked"));
    assert!(report.get("delta").is_some(), "report carries the delta");
    assert!(report["view"]["url"]
        .as_str()
        .unwrap()
        .contains("local.test"));
}

/// Export tests write real artifacts, so they root themselves in the OS
/// temp dir instead of the workspace.
fn call_rooted(root: &Path, name: &str, args: serde_json::Value) -> String {
    handle_native_tool(root, name, &args)
        .expect("tool call succeeds")
        .expect("native tool name is handled")
}

fn temp_root(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("velocity_export_{tag}"));
    let _ = std::fs::create_dir_all(&root);
    root
}

#[test]
fn export_nda_binary_writes_triple_stream_artifact() {
    load("t18-bin");
    let root = temp_root("bin");
    let out = call_rooted(
        &root,
        "browser_native_export_nda",
        json!({ "sessionId": "t18-bin" }),
    );
    assert!(out.contains("binary"), "default format is binary: {out}");
    assert!(
        out.contains("t18-bin_native.nda"),
        "output names the artifact: {out}"
    );
    let path = root
        .join(".velocity")
        .join("browser_artifacts")
        .join("t18-bin_native.nda");
    let bytes = std::fs::read(&path).expect("binary artifact exists");
    assert!(!bytes.is_empty(), "a loaded page produces state triples");
    assert_eq!(
        bytes.len() % 18,
        0,
        "stream is whole 18-byte triple records"
    );
}

#[test]
fn export_nda_readable_returns_and_persists_fact_text() {
    load("t18-read");
    let root = temp_root("read");
    let out = call_rooted(
        &root,
        "browser_native_export_nda",
        json!({ "sessionId": "t18-read", "format": "readable" }),
    );
    assert!(
        out.contains("http://local.test/form"),
        "readable export returns the fact text inline: {out}"
    );
    let path = root
        .join(".velocity")
        .join("browser_artifacts")
        .join("t18-read_facts.txt");
    let persisted = std::fs::read_to_string(&path).expect("facts artifact exists");
    assert!(
        persisted.contains("http://local.test/form"),
        "persisted facts match: {persisted}"
    );
}

#[test]
fn export_nda_trace_persists_trace_stream() {
    load("t18-trace");
    // Act first so the trace collector has something to export.
    call(
        "browser_native_fill_label",
        json!({ "sessionId": "t18-trace", "label": "Email", "text": "t@e.st" }),
    );
    let root = temp_root("trace");
    let out = call_rooted(
        &root,
        "browser_native_export_nda",
        json!({ "sessionId": "t18-trace", "format": "trace" }),
    );
    assert!(
        out.contains("t18-trace_trace.nda"),
        "output names the artifact: {out}"
    );
    let path = root
        .join(".velocity")
        .join("browser_artifacts")
        .join("t18-trace_trace.nda");
    let bytes = std::fs::read(&path).expect("trace artifact exists");
    assert_eq!(bytes.len() % 18, 0, "trace stream is whole triple records");
}

#[test]
fn export_nda_compact_reports_path_and_fact_count() {
    load("t18-compact");
    let root = temp_root("compact");
    let out = call_rooted(
        &root,
        "browser_native_export_nda",
        json!({ "sessionId": "t18-compact", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("compact export output is valid JSON");
    assert_eq!(report["format"], "binary");
    assert!(
        report["factCount"].as_u64().unwrap() > 0,
        "fact count reported"
    );
    assert!(report["path"]
        .as_str()
        .unwrap()
        .contains("t18-compact_native.nda"));
}

#[test]
fn export_nda_rejects_unknown_format() {
    load("t18-badfmt");
    let root = temp_root("badfmt");
    let err = handle_native_tool(
        &root,
        "browser_native_export_nda",
        &json!({ "sessionId": "t18-badfmt", "format": "yaml" }),
    )
    .expect_err("unknown format must be rejected");
    assert!(err.to_string().contains("unknown export format"), "{err}");
}

#[test]
fn tab_tools_open_switch_and_close_with_observed_state() {
    load("t19-tabs");
    let out = call(
        "browser_native_tab_open",
        json!({ "sessionId": "t19-tabs", "tabId": "t19-tabs-bg" }),
    );
    assert!(out.contains("opened background tab 't19-tabs-bg'"), "{out}");
    assert!(
        out.contains("* t19-tabs \""),
        "original tab stays active: {out}"
    );

    let out = call(
        "browser_native_tab_switch",
        json!({ "sessionId": "t19-tabs", "tabId": "t19-tabs-bg" }),
    );
    assert!(out.contains("switched to tab 't19-tabs-bg'"), "{out}");
    assert!(
        out.contains("* t19-tabs-bg \""),
        "new tab becomes active: {out}"
    );
    assert!(
        out.contains("URL:"),
        "switch returns the newly active view: {out}"
    );

    // Switching back must restore the parked tab with its page intact.
    let out = call(
        "browser_native_tab_switch",
        json!({ "sessionId": "t19-tabs", "tabId": "t19-tabs" }),
    );
    assert!(
        out.contains("http://local.test/form"),
        "foreground state survives parking: {out}"
    );

    let out = call(
        "browser_native_tab_close",
        json!({ "sessionId": "t19-tabs", "tabId": "t19-tabs-bg" }),
    );
    assert!(out.contains("closed tab 't19-tabs-bg'"), "{out}");
    assert!(
        out.contains("Tabs (1):"),
        "closed tab leaves the list: {out}"
    );
}

#[test]
fn tab_close_active_and_duplicate_open_are_rejected() {
    load("t19-taberr");
    let err = handle_native_tool(
        Path::new("."),
        "browser_native_tab_close",
        &json!({ "sessionId": "t19-taberr", "tabId": "t19-taberr" }),
    )
    .expect_err("closing the active tab must fail");
    assert!(
        err.to_string().contains("cannot close the active tab"),
        "{err}"
    );

    call(
        "browser_native_tab_open",
        json!({ "sessionId": "t19-taberr", "tabId": "t19-taberr-bg" }),
    );
    let err = handle_native_tool(
        Path::new("."),
        "browser_native_tab_open",
        &json!({ "sessionId": "t19-taberr", "tabId": "t19-taberr-bg" }),
    )
    .expect_err("duplicate tab id must be rejected");
    assert!(err.to_string().contains("already exists"), "{err}");
}

#[test]
fn tab_list_compact_reports_active_flag() {
    load("t19-tablist");
    call(
        "browser_native_tab_open",
        json!({ "sessionId": "t19-tablist", "tabId": "t19-tablist-bg" }),
    );
    let out = call(
        "browser_native_tab_list",
        json!({ "sessionId": "t19-tablist", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("compact tab list is valid JSON");
    let tabs = report["tabs"].as_array().expect("tabs array");
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0]["tabId"], "t19-tablist");
    assert_eq!(tabs[0]["active"], true);
    assert_eq!(tabs[1]["tabId"], "t19-tablist-bg");
    assert_eq!(tabs[1]["active"], false);
}

#[test]
fn scroll_tool_reports_offset_and_scroll_fact_delta() {
    load("t20-scroll");
    let out = call(
        "browser_native_scroll",
        json!({ "sessionId": "t20-scroll", "deltaY": 120 }),
    );
    assert!(
        out.contains("to offset (0, 120)"),
        "status carries the new offset: {out}"
    );
    assert!(
        out.contains("scroll : 0,0 -> 0,120"),
        "delta shows the scroll fact moving: {out}"
    );
}

#[test]
fn scroll_into_view_tool_resolves_element_by_label() {
    load("t20-inview");
    // The default 1920x1080 viewport already shows the whole form.
    let out = call(
        "browser_native_scroll_into_view",
        json!({ "sessionId": "t20-inview", "label": "Log In" }),
    );
    assert!(out.contains("already in view"), "{out}");
    assert!(
        out.contains("URL:"),
        "action output includes the refreshed view: {out}"
    );

    // Shrink the viewport so the submit button starts below the fold.
    get_or_create_native_bridge("t20-inview")
        .lock()
        .unwrap()
        .active_session
        .viewport_height = 10.0;
    let out = call(
        "browser_native_scroll_into_view",
        json!({ "sessionId": "t20-inview", "label": "Log In" }),
    );
    assert!(out.contains("into view (offset"), "{out}");
    assert!(
        out.contains("inViewport"),
        "delta shows in-viewport facts flipping: {out}"
    );
}

#[test]
fn scroll_into_view_tool_reports_missing_label() {
    load("t20-miss");
    let out = call(
        "browser_native_scroll_into_view",
        json!({ "sessionId": "t20-miss", "label": "Nonexistent Widget" }),
    );
    assert!(out.contains("no element matching"), "{out}");
    assert!(
        out.contains("(no state change)"),
        "miss produces an empty delta: {out}"
    );
}

#[test]
fn remember_tool_indexes_page_and_recall_finds_it_semantically() {
    load("t21-mem");
    let out = call(
        "browser_native_remember",
        json!({ "sessionId": "t21-mem", "tags": ["signup"], "outcome": 0.9 }),
    );
    assert!(out.contains("remembered page as 't21-mem:0'"), "{out}");
    assert!(
        out.contains("http://local.test/form"),
        "report carries the url: {out}"
    );
    assert!(out.contains("1 memory stored"), "{out}");

    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t21-mem", "query": "signup" }),
    );
    assert!(
        out.contains("1 memory matched 'signup' (semantic):"),
        "{out}"
    );
    assert!(out.contains("t21-mem:0"), "hit lists the memory id: {out}");
    assert!(
        out.contains("http://local.test/form"),
        "hit lists the url: {out}"
    );

    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t21-mem", "query": "signup", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("compact recall is valid JSON");
    let hits = report["hits"].as_array().expect("hits array");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["memoryId"], "t21-mem:0");
    assert!(hits[0]["similarity"].as_f64().expect("semantic score") > 0.0);
}

#[test]
fn remember_indexes_distilled_markdown_not_raw_text() {
    let bridge = get_or_create_native_bridge("t41-mem");
    bridge.lock().unwrap().load_html(
        "http://local.test/article",
        "<html><head><title>Article</title></head><body>\
             <nav><a href=\"/home\">Home link chrome</a></nav>\
             <div class=\"cookie-banner\"><p>We use cookies here.</p></div>\
             <main><h1>Story Headline</h1><p>Body of the story.</p></main>\
             </body></html>",
    );
    // Raw page text carries the chrome noise.
    let raw = call(
        "browser_native_page_text",
        json!({ "sessionId": "t41-mem" }),
    );
    assert!(raw.contains("Home link chrome"), "{raw}");

    let remember = call(
        "browser_native_remember",
        json!({ "sessionId": "t41-mem", "tags": ["article"], "outcome": 0.8, "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&remember).expect("compact remember is valid JSON");
    let indexed = report["indexedChars"].as_u64().expect("indexedChars") as usize;
    assert!(indexed > 0, "{remember}");
    assert!(
        indexed < raw.len(),
        "boilerplate stripped before indexing: {indexed} vs {} raw chars",
        raw.len()
    );

    // Content words are recallable...
    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t41-mem", "query": "story", "mode": "keyword" }),
    );
    assert!(out.contains("1 memory matched 'story'"), "{out}");
    // ...but chrome words never made it into memory.
    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t41-mem", "query": "cookies", "mode": "keyword" }),
    );
    assert!(
        out.contains("no memories matched"),
        "boilerplate not indexed: {out}"
    );
}

#[test]
fn recall_tool_supports_keyword_tag_and_empty_results() {
    load("t21-modes");
    call(
        "browser_native_remember",
        json!({
            "sessionId": "t21-modes",
            "tags": ["checkout"],
            "outcome": 0.7,
            "note": "special discount pricing page"
        }),
    );

    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t21-modes", "query": "discount", "mode": "keyword" }),
    );
    assert!(out.contains("(keyword)"), "{out}");
    assert!(
        out.contains("discount"),
        "note text is indexed and recallable: {out}"
    );

    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t21-modes", "query": "checkout", "mode": "tag" }),
    );
    assert!(out.contains("(tag)"), "{out}");
    assert!(out.contains("tags [checkout]"), "{out}");
    assert!(out.contains("outcome 0.70"), "{out}");

    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t21-modes", "query": "quantum blockchain", "mode": "semantic" }),
    );
    assert!(
        out.contains("no memories matched 'quantum blockchain'"),
        "{out}"
    );

    let err = handle_native_tool(
        Path::new("."),
        "browser_native_recall",
        &json!({ "sessionId": "t21-modes", "query": "x", "mode": "psychic" }),
    )
    .expect_err("unknown recall mode must be rejected");
    assert!(err.to_string().contains("unknown recall mode"), "{err}");
}

#[test]
fn recall_tool_finds_similar_memories_and_filters_by_outcome() {
    load("t22-sim");
    call(
        "browser_native_remember",
        json!({ "sessionId": "t22-sim", "tags": ["attempt"], "outcome": 0.9, "note": "first pass" }),
    );
    call(
        "browser_native_remember",
        json!({ "sessionId": "t22-sim", "tags": ["attempt"], "outcome": 0.2, "note": "second pass" }),
    );

    // Same page indexed twice: similar mode on the first memory id must
    // surface the second one with a high embedding score.
    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t22-sim", "query": "t22-sim:0", "mode": "similar" }),
    );
    assert!(out.contains("(similar)"), "{out}");
    assert!(out.contains("t22-sim:1"), "sibling memory is found: {out}");
    assert!(
        !out.contains("t22-sim:0 http"),
        "source memory excludes itself: {out}"
    );

    // Unknown memory id is a miss, not an error.
    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t22-sim", "query": "t22-sim:99", "mode": "similar" }),
    );
    assert!(
        out.contains("no memories matched 't22-sim:99' (similar)"),
        "{out}"
    );

    // minOutcome keeps only the successful attempt across any mode.
    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t22-sim", "query": "attempt", "mode": "tag", "minOutcome": 0.8 }),
    );
    assert!(
        out.contains("1 memory matched 'attempt' (tag, outcome >= 0.80):"),
        "{out}"
    );
    assert!(out.contains("t22-sim:0"), "{out}");
    assert!(
        !out.contains("t22-sim:1"),
        "low-outcome memory filtered: {out}"
    );

    // Filter that excludes everything reports the threshold in the miss.
    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t22-sim", "query": "attempt", "mode": "tag", "minOutcome": 0.95 }),
    );
    assert!(
        out.contains("no memories matched 'attempt' (tag, outcome >= 0.95)"),
        "{out}"
    );

    // Compact report carries the filter value.
    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t22-sim", "query": "attempt", "mode": "tag", "minOutcome": 0.8, "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("compact recall is valid JSON");
    assert_eq!(report["minOutcome"], 0.8);
    assert_eq!(report["hits"].as_array().expect("hits array").len(), 1);
}

#[test]
fn page_text_tool_reads_visible_text_with_truncation() {
    load("t24-text");
    let out = call(
        "browser_native_page_text",
        json!({ "sessionId": "t24-text" }),
    );
    assert!(out.starts_with("Signup"), "title leads the text: {out}");
    assert!(out.contains("Log In"), "button text is visible: {out}");

    let out = call(
        "browser_native_page_text",
        json!({ "sessionId": "t24-text", "maxChars": 6 }),
    );
    assert!(out.starts_with("Signup…"), "{out}");
    assert!(out.contains("(truncated to 6 of"), "{out}");
}

#[test]
fn screencast_tool_captures_lists_and_saves_frames() {
    load("t24-cast");
    let out = call(
        "browser_native_screencast",
        json!({ "sessionId": "t24-cast", "action": "capture" }),
    );
    assert!(out.contains("captured frame 0"), "{out}");
    assert!(out.contains("1 frame in timeline"), "{out}");

    // Default action is capture.
    let out = call(
        "browser_native_screencast",
        json!({ "sessionId": "t24-cast" }),
    );
    assert!(out.contains("captured frame 1"), "{out}");

    let out = call(
        "browser_native_screencast",
        json!({ "sessionId": "t24-cast", "action": "list" }),
    );
    assert!(out.contains("2 frames in timeline:"), "{out}");
    assert!(out.contains("frame 0: 1920x1080"), "{out}");
    assert!(out.contains("frame 1: 1920x1080"), "{out}");

    let tmp = std::env::temp_dir();
    let out = handle_native_tool(
        &tmp,
        "browser_native_screencast",
        &json!({ "sessionId": "t24-cast", "action": "save" }),
    )
    .expect("save succeeds")
    .expect("screencast tool is handled");
    assert!(out.contains("saved 2 frame(s) to"), "{out}");
    assert!(out.contains("t24-cast_screencast.json"), "{out}");

    let err = handle_native_tool(
        Path::new("."),
        "browser_native_screencast",
        &json!({ "sessionId": "t24-cast", "action": "explode" }),
    )
    .expect_err("unknown screencast action must be rejected");
    assert!(
        err.to_string().contains("unknown screencast action"),
        "{err}"
    );
}

#[test]
fn find_tool_filters_aom_by_role_and_text() {
    load("t25-find");
    let out = call(
        "browser_native_find",
        json!({ "sessionId": "t25-find", "role": "button" }),
    );
    assert!(out.contains("Log In"), "button hit is listed: {out}");
    assert!(out.contains("elements matched role=button"), "{out}");

    let out = call(
        "browser_native_find",
        json!({ "sessionId": "t25-find", "text": "plan" }),
    );
    assert!(
        out.contains("\"Plan\""),
        "select matched by label text: {out}"
    );

    let out = call(
        "browser_native_find",
        json!({ "sessionId": "t25-find", "text": "zzz-nope" }),
    );
    assert!(out.contains("no elements matched"), "{out}");

    let err = handle_native_tool(
        Path::new("."),
        "browser_native_find",
        &json!({ "sessionId": "t25-find" }),
    )
    .expect_err("find without role or text must be rejected");
    assert!(
        err.to_string().contains("at least one of role or text"),
        "{err}"
    );
}

#[test]
fn validate_tool_reports_constraint_failures_then_valid() {
    let html = r#"<html><head><title>Join</title></head><body>
            <form id="j">
              <input type="email" placeholder="Email" name="email" required />
              <input type="text" name="nick" value="ok" />
              <button type="submit">Join</button>
            </form>
        </body></html>"#;
    get_or_create_native_bridge("t25-valid")
        .lock()
        .unwrap()
        .load_html("http://local.test/join", html);

    let out = call(
        "browser_native_validate",
        json!({ "sessionId": "t25-valid" }),
    );
    assert!(out.contains("1 of 2 control(s) invalid"), "{out}");
    assert!(out.contains("valueMissing"), "empty required email: {out}");

    call(
        "browser_native_fill_label",
        json!({ "sessionId": "t25-valid", "label": "Email", "text": "not-an-email" }),
    );
    let out = call(
        "browser_native_validate",
        json!({ "sessionId": "t25-valid" }),
    );
    assert!(out.contains("typeMismatch"), "bad email flagged: {out}");

    call(
        "browser_native_fill_label",
        json!({ "sessionId": "t25-valid", "label": "Email", "text": "a@b.com" }),
    );
    let out = call(
        "browser_native_validate",
        json!({ "sessionId": "t25-valid" }),
    );
    assert!(
        out.contains("form is valid (2 control(s) checked)"),
        "{out}"
    );

    let compact = call(
        "browser_native_validate",
        json!({ "sessionId": "t25-valid", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact validate is valid JSON");
    assert_eq!(report["controls"], 2);
    assert_eq!(report["invalid"], 0);
}

#[test]
fn links_tool_lists_navigation_map_with_filter_and_limit() {
    let html = r#"<html><head><title>Nav</title></head><body>
            <a href="/pricing">Pricing</a>
            <a href="/docs">Docs</a>
            <a href="https://ext.example/x">External <b>Deal</b></a>
            <a name="top">Bare anchor</a>
        </body></html>"#;
    get_or_create_native_bridge("t26-links")
        .lock()
        .unwrap()
        .load_html("http://local.test/nav", html);

    let out = call("browser_native_links", json!({ "sessionId": "t26-links" }));
    assert!(
        out.starts_with("3 links:"),
        "bare anchor is excluded: {out}"
    );
    assert!(out.contains("\"Pricing\" -> /pricing"), "{out}");
    assert!(
        out.contains("\"ExternalDeal\" -> https://ext.example/x"),
        "nested text is included: {out}"
    );

    let out = call(
        "browser_native_links",
        json!({ "sessionId": "t26-links", "filter": "docs" }),
    );
    assert!(out.starts_with("1 link matching \"docs\":"), "{out}");
    assert!(out.contains("-> /docs"), "{out}");

    let out = call(
        "browser_native_links",
        json!({ "sessionId": "t26-links", "filter": "zzz-nope" }),
    );
    assert!(out.contains("no links matched"), "{out}");

    let out = call(
        "browser_native_links",
        json!({ "sessionId": "t26-links", "limit": 1 }),
    );
    assert!(out.contains("… 2 more"), "truncation is reported: {out}");

    let compact = call(
        "browser_native_links",
        json!({ "sessionId": "t26-links", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact links is valid JSON");
    assert_eq!(report["matched"], 3);
    assert_eq!(report["links"].as_array().expect("links array").len(), 3);
}

#[test]
fn history_tool_lists_stack_and_traversal_keeps_forward_entries() {
    load("t27-hist");
    let two = r#"<html><head><title>Two</title></head><body><p>second</p></body></html>"#;
    get_or_create_native_bridge("t27-hist")
        .lock()
        .unwrap()
        .load_html("http://local.test/two", two);

    let out = call("browser_native_history", json!({ "sessionId": "t27-hist" }));
    assert!(out.starts_with("3 history entries (at #2):"), "{out}");
    assert!(out.contains("> #2 http://local.test/two \"Two\""), "{out}");
    assert!(
        out.contains("#1 http://local.test/form \"Signup\""),
        "titles are backfilled: {out}"
    );
    assert!(
        out.contains("#0 about:blank\n"),
        "seed entry has no title: {out}"
    );

    // Reloading the current entry must not grow the stack.
    get_or_create_native_bridge("t27-hist")
        .lock()
        .unwrap()
        .load_html("http://local.test/two", two);
    let out = call("browser_native_history", json!({ "sessionId": "t27-hist" }));
    assert!(
        out.starts_with("3 history entries (at #2):"),
        "reload does not duplicate: {out}"
    );

    // Going back then re-loading that entry (what agent_back does after
    // a successful fetch) must keep the forward entry intact.
    {
        let bridge = get_or_create_native_bridge("t27-hist");
        let mut b = bridge.lock().unwrap();
        let url = b
            .active_session
            .history_stack
            .back()
            .expect("has a previous entry")
            .url
            .clone();
        b.load_html(&url, FORM_HTML);
    }
    let out = call("browser_native_history", json!({ "sessionId": "t27-hist" }));
    assert!(
        out.starts_with("3 history entries (at #1):"),
        "forward entry survives: {out}"
    );
    assert!(out.contains("> #1 http://local.test/form"), "{out}");
    assert!(out.contains("  #2 http://local.test/two"), "{out}");

    let compact = call(
        "browser_native_history",
        json!({ "sessionId": "t27-hist", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact history is valid JSON");
    assert_eq!(report["entries"], 3);
    assert_eq!(report["current"], 1);
    assert_eq!(report["history"][1]["current"], true);
}

#[test]
fn checkpoint_tool_saves_diffs_lists_and_drops() {
    load("t28-ckpt");
    let out = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t28-ckpt", "action": "save", "name": "start" }),
    );
    assert!(out.contains("checkpoint 'start' saved"), "{out}");

    // Nothing happened yet: the diff is empty.
    let out = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t28-ckpt", "action": "diff", "name": "start" }),
    );
    assert!(out.contains("changes since checkpoint 'start':"), "{out}");
    assert!(out.contains("(no state change)"), "{out}");

    // Two actions later, one diff reports the accumulated change.
    call(
        "browser_native_fill_label",
        json!({ "sessionId": "t28-ckpt", "label": "Email", "text": "x@y.example" }),
    );
    call(
        "browser_native_check_label",
        json!({ "sessionId": "t28-ckpt", "label": "Subscribe", "checked": true }),
    );
    let out = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t28-ckpt", "action": "diff", "name": "start" }),
    );
    assert!(
        out.contains("x@y.example"),
        "fill shows in the delta: {out}"
    );
    assert!(!out.contains("(no state change)"), "{out}");

    // Saving under the same name replaces the snapshot.
    let out = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t28-ckpt", "action": "save", "name": "start" }),
    );
    assert!(out.contains("checkpoint 'start' replaced"), "{out}");

    // list shows the snapshots (the rolling `_pre` auto-checkpoint from
    // the actions above is always present), drop removes the named one.
    let out = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t28-ckpt", "action": "list" }),
    );
    assert!(out.starts_with("2 checkpoints:"), "{out}");
    assert!(out.contains("start ("), "{out}");
    assert!(
        out.contains("_pre ("),
        "rolling auto-checkpoint is listed: {out}"
    );
    let out = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t28-ckpt", "action": "drop", "name": "start" }),
    );
    assert!(out.contains("checkpoint 'start' dropped"), "{out}");
    let out = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t28-ckpt", "action": "list" }),
    );
    assert!(
        out.starts_with("1 checkpoint:"),
        "only `_pre` remains: {out}"
    );
    assert!(out.contains("_pre ("), "{out}");

    // Missing checkpoint and unknown action are errors.
    let err = handle_native_tool(
        Path::new("."),
        "browser_native_checkpoint",
        &json!({ "sessionId": "t28-ckpt", "action": "diff", "name": "gone" }),
    )
    .expect_err("diff against a missing checkpoint must fail");
    assert!(err.to_string().contains("no checkpoint 'gone'"), "{err}");
    let err = handle_native_tool(
        Path::new("."),
        "browser_native_checkpoint",
        &json!({ "sessionId": "t28-ckpt", "action": "teleport" }),
    )
    .expect_err("unknown checkpoint action must be rejected");
    assert!(
        err.to_string().contains("unknown checkpoint action"),
        "{err}"
    );

    // Compact save carries the fact count.
    let compact = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t28-ckpt", "action": "save", "name": "s2", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact checkpoint is valid JSON");
    assert_eq!(report["action"], "save");
    assert_eq!(report["replaced"], false);
    assert!(report["facts"].as_u64().expect("facts count") > 0);
}

#[test]
fn checkpoint_diff_snippets_long_content_facts() {
    load("t43-diff");
    call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t43-diff", "action": "save", "name": "before" }),
    );

    // Navigate to a long article: the content fact changes by thousands
    // of characters, but the diff stays a summary.
    let long_para = "The quick brown fox jumps over the lazy dog. ".repeat(40);
    let bridge = get_or_create_native_bridge("t43-diff");
    bridge.lock().unwrap().load_html(
            "http://local.test/article",
            &format!("<html><head><title>Article</title></head><body><main><p>{long_para}</p></main></body></html>"),
        );

    let out = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t43-diff", "action": "diff", "name": "before" }),
    );
    assert!(out.contains("content"), "content change surfaces: {out}");
    assert!(
        out.contains("…(+"),
        "long values collapse to snippets: {out}"
    );
    assert!(
        out.chars().count() < 4000,
        "diff stays bounded, got {} chars",
        out.chars().count()
    );

    let compact = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t43-diff", "action": "diff", "name": "before", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact diff is valid JSON");
    let changed = report["delta"]["changed"]
        .as_array()
        .expect("changed array");
    let content_change = changed
        .iter()
        .find(|c| c["predicate"] == "content")
        .expect("content change present");
    let new_value = content_change["new"].as_str().expect("new value");
    assert!(
        new_value.contains("…(+"),
        "compact diff is snippeted too: {compact}"
    );
    assert!(new_value.chars().count() < 250, "{new_value}");
}

#[test]
fn delta_output_folds_repeated_predicate_lines() {
    let bridge = get_or_create_native_bridge("t44-fold");
    bridge.lock().unwrap().load_html(
        "http://local.test/list",
        "<html><body><ul><li>alpha</li><li>bravo</li><li>charlie</li>\
             <li>delta</li><li>echo</li><li>foxtrot</li></ul></body></html>",
    );
    call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t44-fold", "action": "save", "name": "a" }),
    );
    // Same structure, every row renamed: six name changes at once.
    bridge.lock().unwrap().load_html(
        "http://local.test/list",
        "<html><body><ul><li>one</li><li>two</li><li>three</li>\
             <li>four</li><li>five</li><li>six</li></ul></body></html>",
    );

    let out = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t44-fold", "action": "diff", "name": "a" }),
    );
    assert!(out.contains("~ node_"), "explicit lines stay: {out}");
    assert!(
        out.contains("(1 more bounds change(s))"),
        "overflowing predicate folds into a count: {out}"
    );
    let bounds_lines = out
        .lines()
        .filter(|l| l.starts_with("  ~") && l.contains(" bounds "))
        .count();
    assert!(bounds_lines <= 4, "at most 4 explicit bounds lines: {out}");
    assert!(
        out.contains("content"),
        "summary facts still present: {out}"
    );
}

#[test]
fn content_change_signal_detects_fact_changes() {
    use velocity_browser::predicates::SESSION_CONTENT;
    use velocity_browser::{FactChange, NdaDelta};

    // A changed content fact reports before/after char counts.
    let mut delta = NdaDelta::default();
    delta.changed.push(FactChange {
        subject: "s1".to_string(),
        predicate: SESSION_CONTENT,
        old: "ab".to_string(),
        new: "abcdef".to_string(),
    });
    assert_eq!(content_change_signal(&delta), Some((2, 6)));
    assert_eq!(
        content_change_note(&delta),
        "Content changed: 2 -> 6 chars\n"
    );

    // Add/remove pairs (page transitions) also count.
    let mut delta = NdaDelta::default();
    delta
        .removed
        .push(("s1".to_string(), SESSION_CONTENT, "old".to_string()));
    delta
        .added
        .push(("s1".to_string(), SESSION_CONTENT, "newtext".to_string()));
    assert_eq!(content_change_signal(&delta), Some((3, 7)));

    // No content fact touched: no signal, no note.
    let empty = NdaDelta::default();
    assert_eq!(content_change_signal(&empty), None);
    assert_eq!(content_change_note(&empty), "");
}

#[test]
fn action_reports_omit_content_change_when_unchanged() {
    load("t45-sig");
    // Filling an input changes a value fact but never the content fact.
    let out = call(
        "browser_native_fill_label",
        json!({ "sessionId": "t45-sig", "label": "Email", "text": "a@b.example" }),
    );
    assert!(!out.contains("Content changed:"), "{out}");

    let compact = call(
        "browser_native_fill_label",
        json!({ "sessionId": "t45-sig", "label": "Email", "text": "c@d.example", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact action report is valid JSON");
    assert!(
        report.get("contentChange").is_none(),
        "field absent when content is unchanged: {compact}"
    );
}

#[test]
fn wait_tool_matches_element_and_times_out() {
    load("t47-wait");

    // An element that already exists matches on the first poll.
    let out = call(
        "browser_native_wait",
        json!({ "sessionId": "t47-wait", "mode": "element", "label": "log in", "timeout": 2000 }),
    );
    assert!(out.starts_with("matched after "), "{out}");
    assert!(out.contains("\"Log In\""), "{out}");

    let compact = call(
        "browser_native_wait",
        json!({ "sessionId": "t47-wait", "mode": "element", "label": "log in", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact wait report is valid JSON");
    assert_eq!(report["status"], "matched", "{compact}");
    assert!(
        report["matched"].as_str().unwrap().contains("Log In"),
        "{compact}"
    );

    // Nothing named "Nonexistent" appears: the wait times out.
    let out = call(
        "browser_native_wait",
        json!({ "sessionId": "t47-wait", "mode": "element", "label": "Nonexistent", "timeout": 250, "poll": 50 }),
    );
    assert!(out.starts_with("timeout after "), "{out}");

    // Static content never changes: content mode times out too.
    let out = call(
        "browser_native_wait",
        json!({ "sessionId": "t47-wait", "mode": "content", "timeout": 250, "poll": 50 }),
    );
    assert!(out.contains("no 'content' change observed"), "{out}");
}

#[test]
fn wait_tool_rejects_bad_arguments() {
    load("t47-wait-err");
    let err = call_err(
        "browser_native_wait",
        json!({ "sessionId": "t47-wait-err", "mode": "element" }),
    );
    assert!(err.to_string().contains("label is required"), "{err}");
    let err = call_err(
        "browser_native_wait",
        json!({ "sessionId": "t47-wait-err", "mode": "idle" }),
    );
    assert!(
        err.to_string().contains("unknown wait mode 'idle'"),
        "{err}"
    );
}

#[test]
fn actions_keep_a_rolling_pre_checkpoint() {
    load("t48-auto");

    // No explicit save: the first action still leaves a `_pre` snapshot.
    let out = call(
        "browser_native_fill_label",
        json!({ "sessionId": "t48-auto", "label": "Email", "text": "x@y.example" }),
    );
    assert!(out.contains("Changes:"), "{out}");

    let list = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t48-auto", "action": "list" }),
    );
    assert!(list.contains("_pre"), "auto-checkpoint is listed: {list}");

    // Diff against `_pre` shows exactly what the last action changed.
    let diff = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t48-auto", "action": "diff", "name": "_pre" }),
    );
    assert!(
        diff.contains("x@y.example"),
        "filled value surfaces in the diff: {diff}"
    );

    // A second action rolls the checkpoint forward: `_pre` now holds the
    // state right before that action, not before the first one.
    call(
        "browser_native_fill_label",
        json!({ "sessionId": "t48-auto", "label": "Email", "text": "z@w.example" }),
    );
    let diff = call(
        "browser_native_checkpoint",
        json!({ "sessionId": "t48-auto", "action": "diff", "name": "_pre" }),
    );
    assert!(
        diff.contains("z@w.example"),
        "second fill is the new value: {diff}"
    );
    assert!(
        diff.contains("x@y.example -> z@w.example"),
        "changed fact spans exactly the last action, old -> new: {diff}"
    );
}

#[test]
fn brief_reports_last_action_change_summary() {
    load("t49-last");

    // Before any action there is no `_pre`: no summary line.
    let out = call("browser_native_brief", json!({ "sessionId": "t49-last" }));
    assert!(!out.contains("Last action:"), "{out}");

    call(
        "browser_native_fill_label",
        json!({ "sessionId": "t49-last", "label": "Email", "text": "q@r.example" }),
    );
    let out = call("browser_native_brief", json!({ "sessionId": "t49-last" }));
    assert!(
        out.contains("Last action: "),
        "summary surfaces after an action: {out}"
    );
    assert!(out.contains("changed fact(s)"), "{out}");

    let compact = call(
        "browser_native_brief",
        json!({ "sessionId": "t49-last", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact brief is valid JSON");
    let last = &report["lastChange"];
    assert!(
        last.is_object(),
        "lastChange present after an action: {compact}"
    );
    let touched = last["added"].as_u64().unwrap()
        + last["removed"].as_u64().unwrap()
        + last["changed"].as_u64().unwrap();
    assert!(
        touched >= 1,
        "the fill touched at least one fact: {compact}"
    );
}

#[test]
fn content_delta_threshold_ignores_small_moves() {
    // Growth and shrinkage both count, symmetric around the baseline.
    assert!(content_delta_matches(100, 260, 150));
    assert!(content_delta_matches(260, 100, 150));
    // Exact threshold matches; one char under does not.
    assert!(content_delta_matches(100, 250, 150));
    assert!(!content_delta_matches(100, 249, 150));
    // Default threshold (1) still ignores a static page.
    assert!(!content_delta_matches(100, 100, 1));
}

#[test]
fn wait_tool_honours_min_delta_argument() {
    load("t50-mindelta");
    // A static page with a large threshold times out just like the
    // default â€” but the argument path is exercised end to end.
    let out = call(
        "browser_native_wait",
        json!({ "sessionId": "t50-mindelta", "mode": "content", "minDelta": 500, "timeout": 250, "poll": 50 }),
    );
    assert!(out.starts_with("timeout after "), "{out}");
    assert!(out.contains("no 'content' change observed"), "{out}");
}

#[test]
fn wait_tool_url_mode_matches_and_times_out() {
    load("t51-url");

    // With a label matching the current URL: matches on the first poll.
    let out = call(
        "browser_native_wait",
        json!({ "sessionId": "t51-url", "mode": "url", "label": "local.test/form", "timeout": 2000 }),
    );
    assert!(out.starts_with("matched after "), "{out}");
    assert!(out.contains("url http://local.test/form"), "{out}");

    // Without a label, no navigation means timeout.
    let out = call(
        "browser_native_wait",
        json!({ "sessionId": "t51-url", "mode": "url", "timeout": 250, "poll": 50 }),
    );
    assert!(out.starts_with("timeout after "), "{out}");

    // A label that never appears also times out.
    let compact = call(
        "browser_native_wait",
        json!({ "sessionId": "t51-url", "mode": "url", "label": "nowhere.example", "timeout": 250, "poll": 50, "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact wait report is valid JSON");
    assert_eq!(report["status"], "timeout", "{compact}");
    assert_eq!(report["mode"], "url", "{compact}");
}

#[test]
fn wait_tool_gone_flag_inverts_element_predicate() {
    load("t52-gone");

    // An element that never existed is already gone: matches fast.
    let out = call(
        "browser_native_wait",
        json!({ "sessionId": "t52-gone", "mode": "element", "label": "Nonexistent", "gone": true, "timeout": 2000 }),
    );
    assert!(out.starts_with("matched after "), "{out}");
    assert!(out.contains("\"Nonexistent\" gone"), "{out}");

    // An element that IS on the page never becomes gone: timeout.
    let out = call(
        "browser_native_wait",
        json!({ "sessionId": "t52-gone", "mode": "element", "label": "log in", "gone": true, "timeout": 250, "poll": 50 }),
    );
    assert!(out.starts_with("timeout after "), "{out}");

    // Compact report carries the inverted match.
    let compact = call(
        "browser_native_wait",
        json!({ "sessionId": "t52-gone", "mode": "element", "label": "Nonexistent", "gone": true, "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact wait report is valid JSON");
    assert_eq!(report["status"], "matched", "{compact}");
    assert!(
        report["matched"].as_str().unwrap_or("").contains("gone"),
        "{compact}"
    );
}

#[test]
fn wait_tool_stable_mode_detects_settlement() {
    load("t53-stable");

    // A static page is already settling: N consecutive quiet polls match.
    let out = call(
        "browser_native_wait",
        json!({ "sessionId": "t53-stable", "mode": "stable", "stable": 3, "timeout": 2000, "poll": 40 }),
    );
    assert!(out.starts_with("matched after "), "{out}");
    assert!(out.contains("content stable at "), "{out}");
    assert!(out.contains("3 quiet polls"), "{out}");

    // Compact report carries the stable match.
    let compact = call(
        "browser_native_wait",
        json!({ "sessionId": "t53-stable", "mode": "stable", "stable": 2, "poll": 40, "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact wait report is valid JSON");
    assert_eq!(report["status"], "matched", "{compact}");
    assert_eq!(report["mode"], "stable", "{compact}");
}

#[test]
fn assert_tool_checks_content_and_elements() {
    load("t54-assert");

    // Both conditions hold on the fixture page (the distilled content
    // is the readable core - here the title heading - and the AOM
    // carries the form controls).
    let out = call(
        "browser_native_assert",
        json!({ "sessionId": "t54-assert", "text": "Signup", "label": "log in" }),
    );
    assert!(out.starts_with("assert ok: "), "{out}");
    assert!(out.contains("text \"Signup\""), "{out}");
    assert!(out.contains("element \"log in\""), "{out}");

    // A missing text fragment fails in-band with diagnostic detail.
    let out = call(
        "browser_native_assert",
        json!({ "sessionId": "t54-assert", "text": "zebra" }),
    );
    assert!(out.starts_with("assert FAILED:"), "{out}");
    assert!(out.contains("text \"zebra\": FAILED"), "{out}");
    assert!(out.contains("content is"), "{out}");

    // Mixed pass/fail reports both verdicts.
    let out = call(
        "browser_native_assert",
        json!({ "sessionId": "t54-assert", "text": "Signup", "label": "Checkout" }),
    );
    assert!(out.starts_with("assert FAILED:"), "{out}");
    assert!(out.contains("text \"Signup\": ok"), "{out}");
    assert!(out.contains("element \"Checkout\": FAILED"), "{out}");

    // Compact report carries per-check results.
    let compact = call(
        "browser_native_assert",
        json!({ "sessionId": "t54-assert", "label": "subscribe", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact assert report is valid JSON");
    assert_eq!(report["ok"], true, "{compact}");
    assert_eq!(report["checks"][0]["what"], "element", "{compact}");

    // No conditions at all is a usage error, not a silent pass.
    let err = call_err(
        "browser_native_assert",
        json!({ "sessionId": "t54-assert" }),
    );
    assert!(
        err.to_string().contains("assert needs at least one of"),
        "{err}"
    );
}

#[test]
fn assert_tool_wait_ms_grace_period_polls() {
    load("t55-waitassert");

    // A condition that already holds matches immediately even with a
    // grace period, and the report carries the elapsed time.
    let out = call(
        "browser_native_assert",
        json!({ "sessionId": "t55-waitassert", "label": "log in", "waitMs": 2000, "poll": 50 }),
    );
    assert!(out.starts_with("after "), "{out}");
    assert!(out.contains("assert ok: "), "{out}");

    // A condition that never holds burns the whole grace period and
    // then reports failure with the elapsed time.
    let compact = call(
        "browser_native_assert",
        json!({ "sessionId": "t55-waitassert", "label": "Checkout", "waitMs": 250, "poll": 50, "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact assert report is valid JSON");
    assert_eq!(report["ok"], false, "{compact}");
    let elapsed = report["elapsedMs"]
        .as_u64()
        .expect("elapsedMs present when waiting");
    assert!(elapsed >= 200, "grace period should be spent: {compact}");

    // Without waitMs the report stays free of timing noise.
    let out = call(
        "browser_native_assert",
        json!({ "sessionId": "t55-waitassert", "label": "log in" }),
    );
    assert!(out.starts_with("assert ok: "), "{out}");
}

#[test]
fn failed_asserts_feed_the_reflection_loop() {
    load("t56-reflect");

    // Two failed guards on the same missing element: reflect must spot
    // the repeated assertion failure like any other repeated miss.
    for _ in 0..2 {
        let out = call(
            "browser_native_assert",
            json!({ "sessionId": "t56-reflect", "label": "Checkout" }),
        );
        assert!(out.starts_with("assert FAILED:"), "{out}");
    }
    let out = call(
        "browser_native_reflect",
        json!({ "sessionId": "t56-reflect" }),
    );
    assert!(out.contains("[SELF-REFLECTION]"), "{out}");
    assert!(out.contains("failed 2 times"), "{out}");
    assert!(out.contains("assert on [element]"), "{out}");

    // Passing guards record nothing: a session with only successful
    // asserts has no outcome context to show.
    load("t56-clean");
    let out = call(
        "browser_native_assert",
        json!({ "sessionId": "t56-clean", "label": "log in" }),
    );
    assert!(out.starts_with("assert ok: "), "{out}");
    let out = call(
        "browser_native_reflect",
        json!({ "sessionId": "t56-clean" }),
    );
    assert!(!out.contains("Recent action outcomes"), "{out}");
}

#[test]
fn brief_reports_guard_health() {
    load("t57-guards");

    // No asserts yet: the brief carries no guards line or key.
    let out = call("browser_native_brief", json!({ "sessionId": "t57-guards" }));
    assert!(!out.contains("Guards:"), "{out}");
    let compact = call(
        "browser_native_brief",
        json!({ "sessionId": "t57-guards", "compact": true }),
    );
    assert!(compact.contains("\"guards\": null"), "{compact}");

    // Two failed guards enter the outcome history and surface in the
    // brief as a summary. Passing asserts record nothing (Batch 56),
    // so the failed count is exactly the number of failed checks.
    call(
        "browser_native_assert",
        json!({ "sessionId": "t57-guards", "label": "zebra" }),
    );
    call(
        "browser_native_assert",
        json!({ "sessionId": "t57-guards", "label": "unicorn" }),
    );
    let out = call("browser_native_brief", json!({ "sessionId": "t57-guards" }));
    assert!(out.contains("Guards: 0 passed, 2 failed"), "{out}");
    let compact = call(
        "browser_native_brief",
        json!({ "sessionId": "t57-guards", "compact": true }),
    );
    assert!(compact.contains("\"failed\": 2"), "{compact}");
    assert!(compact.contains("\"passed\": 0"), "{compact}");
}

#[test]
fn brief_guards_name_most_missed_target() {
    load("t58-most");

    // Two misses on one label, one on another: the breakdown points
    // at the repeater instead of forcing a read of the outcome list.
    call(
        "browser_native_assert",
        json!({ "sessionId": "t58-most", "label": "zebra" }),
    );
    call(
        "browser_native_assert",
        json!({ "sessionId": "t58-most", "label": "zebra" }),
    );
    call(
        "browser_native_assert",
        json!({ "sessionId": "t58-most", "label": "unicorn" }),
    );
    let out = call("browser_native_brief", json!({ "sessionId": "t58-most" }));
    assert!(out.contains("most missed: \"zebra\" (2x)"), "{out}");
    let compact = call(
        "browser_native_brief",
        json!({ "sessionId": "t58-most", "compact": true }),
    );
    assert!(compact.contains("\"target\": \"zebra\""), "{compact}");
    assert!(compact.contains("\"count\": 2"), "{compact}");
}

#[test]
fn reflect_tool_surfaces_repeated_failure_lessons() {
    load("t29-reflect");

    // Nothing recorded yet: no patterns, no outcome context.
    let out = call(
        "browser_native_reflect",
        json!({ "sessionId": "t29-reflect" }),
    );
    assert!(out.contains("(no failure patterns detected)"), "{out}");
    assert!(!out.contains("Recent action outcomes"), "{out}");

    // Two clicks on a target that does not exist: observed delta is empty
    // and the status reports the miss, so both score as failures.
    for _ in 0..2 {
        let out = call(
            "browser_native_click_text",
            json!({ "sessionId": "t29-reflect", "text": "Launch Rocket" }),
        );
        assert!(out.contains("no clickable element"), "{out}");
    }
    let out = call(
        "browser_native_reflect",
        json!({ "sessionId": "t29-reflect" }),
    );
    assert!(out.contains("[SELF-REFLECTION]"), "{out}");
    assert!(out.contains("failed 2 times"), "{out}");
    assert!(out.contains("clickable"), "{out}");
    assert!(out.contains("Recent action outcomes:"), "{out}");
    assert!(out.contains("click on [clickable]"), "{out}");

    // A successful fill scores high and shows up in the outcome context.
    call(
        "browser_native_fill_label",
        json!({ "sessionId": "t29-reflect", "label": "Email", "text": "x@y.example" }),
    );
    let compact = call(
        "browser_native_reflect",
        json!({ "sessionId": "t29-reflect", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact reflect is valid JSON");
    assert!(
        !report["reflections"]
            .as_array()
            .expect("reflections")
            .is_empty(),
        "{compact}"
    );
    let outcomes = report["outcomes"].as_array().expect("outcomes");
    assert_eq!(outcomes.len(), 3, "{compact}");
    let fill = &outcomes[2];
    assert_eq!(fill["action"], "fill");
    assert_eq!(fill["role"], "textbox");
    assert_eq!(fill["target"], "Email");
    assert_eq!(fill["error"], false);
    assert!(fill["score"].as_f64().expect("score") > 0.5, "{compact}");
    assert_eq!(outcomes[0]["error"], true, "{compact}");
}

#[test]
fn predict_tool_ranks_targets_by_learned_confidence() {
    load("t30-predict");

    // No history yet: prediction falls back to the conservative default
    // and there are no learned patterns to report.
    let out = call(
        "browser_native_predict",
        json!({ "sessionId": "t30-predict" }),
    );
    assert!(out.contains("suggested next action:"), "{out}");
    assert!(
        out.contains("0.70"),
        "default confidence before learning: {out}"
    );
    assert!(!out.contains("learned patterns"), "{out}");

    // Three observed successful fills teach the store that textboxes work
    // on this domain (min_observations = 3 before learned scores count).
    for text in ["a@x.example", "b@x.example", "c@x.example"] {
        call(
            "browser_native_fill_label",
            json!({ "sessionId": "t30-predict", "label": "Email", "text": text }),
        );
    }
    let out = call(
        "browser_native_predict",
        json!({ "sessionId": "t30-predict" }),
    );
    assert!(out.contains("suggested next action: fill"), "{out}");
    assert!(out.contains("[textbox]"), "{out}");
    assert!(out.contains("learned patterns on this domain:"), "{out}");
    assert!(out.contains("fill on textbox:"), "{out}");
    assert!(out.contains("(3 obs)"), "{out}");

    let compact = call(
        "browser_native_predict",
        json!({ "sessionId": "t30-predict", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact predict is valid JSON");
    assert_eq!(report["suggestion"]["action"], "fill", "{compact}");
    assert!(
        report["suggestion"]["confidence"]
            .as_f64()
            .expect("confidence")
            > 0.8,
        "{compact}"
    );
    assert_eq!(report["patterns"][0]["role"], "textbox", "{compact}");
    assert_eq!(report["patterns"][0]["observations"], 3, "{compact}");
}

#[test]
fn learn_tool_persists_confidence_across_sessions() {
    load("t31-learn-a");
    let root = temp_root("learn31");

    // Teach session A that fills on textboxes succeed on this domain.
    for text in ["a@y.example", "b@y.example", "c@y.example"] {
        call(
            "browser_native_fill_label",
            json!({ "sessionId": "t31-learn-a", "label": "Email", "text": text }),
        );
    }
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({ "sessionId": "t31-learn-a", "action": "save" }),
    );
    assert!(out.contains("saved"), "{out}");
    assert!(
        out.contains("t31-learn-a_confidence.nda"),
        "output names the artifact: {out}"
    );
    let path = root
        .join(".velocity")
        .join("browser_artifacts")
        .join("t31-learn-a_confidence.nda");
    assert!(path.exists(), "confidence artifact persisted");

    // A brand-new session starts from the conservative default...
    load("t31-learn-b");
    let out = call(
        "browser_native_predict",
        json!({ "sessionId": "t31-learn-b" }),
    );
    assert!(
        out.contains("0.70"),
        "fresh session has no experience: {out}"
    );

    // ...until it loads the experience session A recorded.
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({
            "sessionId": "t31-learn-b",
            "action": "load",
            "file": "t31-learn-a_confidence.nda",
        }),
    );
    assert!(
        out.contains("restored 2 learned pattern(s)"),
        "site + generic: {out}"
    );
    assert!(out.contains("learned patterns on this domain:"), "{out}");
    assert!(out.contains("fill on textbox:"), "{out}");

    let compact = call(
        "browser_native_predict",
        json!({ "sessionId": "t31-learn-b", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact predict is valid JSON");
    assert_eq!(report["suggestion"]["action"], "fill", "{compact}");
    assert!(
        report["suggestion"]["confidence"]
            .as_f64()
            .expect("confidence")
            > 0.8,
        "restored experience drives prediction: {compact}"
    );
}

#[test]
fn learn_tool_rejects_bad_action_and_missing_artifact() {
    load("t31-learn-err");
    let root = temp_root("learn31err");
    let err = handle_native_tool(
        &root,
        "browser_native_learn",
        &json!({ "sessionId": "t31-learn-err", "action": "forget" }),
    )
    .expect_err("unknown action must be rejected");
    assert!(err.to_string().contains("unknown learn action"), "{err}");

    let err = handle_native_tool(
        &root,
        "browser_native_learn",
        &json!({ "sessionId": "t31-learn-err", "action": "load", "file": "nope.nda" }),
    )
    .expect_err("missing artifact must be reported");
    assert!(
        err.to_string().contains("failed to read learned patterns"),
        "{err}"
    );
}

#[test]
fn learn_tool_persists_page_memory_across_sessions() {
    load("t32-mem-a");
    let root = temp_root("learn32");

    // Remember the page with a distinctive note so recall can find it.
    call(
        "browser_native_remember",
        json!({
            "sessionId": "t32-mem-a",
            "note": "alpha-bravo-memo signup page",
            "tags": ["signup"],
            "outcome": 0.9,
        }),
    );
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({ "sessionId": "t32-mem-a", "action": "save", "what": "memory" }),
    );
    assert!(out.contains("saved 1 page memory(ies)"), "{out}");
    assert!(
        out.contains("t32-mem-a_memory.nda"),
        "output names the artifact: {out}"
    );

    // A brand-new session remembers nothing...
    load("t32-mem-b");
    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t32-mem-b", "query": "alpha-bravo-memo", "mode": "keyword" }),
    );
    assert!(
        out.contains("no memories matched"),
        "fresh session is empty: {out}"
    );

    // ...until it loads what session A stored.
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({
            "sessionId": "t32-mem-b",
            "action": "load",
            "what": "memory",
            "file": "t32-mem-a_memory.nda",
        }),
    );
    assert!(out.contains("restored 1 page memory(ies)"), "{out}");
    assert!(out.contains("1 memory(ies) now stored"), "{out}");

    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t32-mem-b", "query": "alpha-bravo-memo", "mode": "keyword" }),
    );
    assert!(
        out.contains("local.test/form"),
        "restored memory is searchable: {out}"
    );
    assert!(out.contains("signup"), "tags survive the round-trip: {out}");
    assert!(
        out.contains("0.90"),
        "outcome survives the round-trip: {out}"
    );

    // Reloading the same artifact must not duplicate memories.
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({
            "sessionId": "t32-mem-b",
            "action": "load",
            "what": "memory",
            "file": "t32-mem-a_memory.nda",
        }),
    );
    assert!(
        out.contains("restored 0 page memory(ies)"),
        "reload is idempotent: {out}"
    );
    assert!(out.contains("1 memory(ies) now stored"), "{out}");
}

#[test]
fn learn_tool_rejects_unknown_store() {
    load("t32-mem-err");
    let err = handle_native_tool(
        Path::new("."),
        "browser_native_learn",
        &json!({ "sessionId": "t32-mem-err", "what": "cookies" }),
    )
    .expect_err("unknown store must be rejected");
    assert!(err.to_string().contains("unknown learn store"), "{err}");
}

#[test]
fn learn_tool_persists_outcome_history_across_sessions() {
    load("t33-out-a");
    let root = temp_root("learn33");

    // Two clicks on a missing target record two scored failures.
    for _ in 0..2 {
        let out = call(
            "browser_native_click_text",
            json!({ "sessionId": "t33-out-a", "text": "Launch Rocket" }),
        );
        assert!(out.contains("no clickable element"), "{out}");
    }
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({ "sessionId": "t33-out-a", "action": "save", "what": "outcomes" }),
    );
    assert!(out.contains("saved 2 action outcome(s)"), "{out}");
    assert!(
        out.contains("t33-out-a_outcomes.nda"),
        "output names the artifact: {out}"
    );

    // A brand-new session has no experience to reflect on...
    load("t33-out-b");
    let out = call(
        "browser_native_reflect",
        json!({ "sessionId": "t33-out-b" }),
    );
    assert!(out.contains("(no failure patterns detected)"), "{out}");

    // ...until it inherits session A's outcome history.
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({
            "sessionId": "t33-out-b",
            "action": "load",
            "what": "outcomes",
            "file": "t33-out-a_outcomes.nda",
        }),
    );
    assert!(out.contains("restored 2 action outcome(s)"), "{out}");
    assert!(out.contains("2 outcome(s) now recorded"), "{out}");

    let out = call(
        "browser_native_reflect",
        json!({ "sessionId": "t33-out-b" }),
    );
    assert!(
        out.contains("[SELF-REFLECTION]"),
        "inherited failures reflect: {out}"
    );
    assert!(out.contains("failed 2 times"), "{out}");
    assert!(out.contains("Recent action outcomes:"), "{out}");
    assert!(out.contains("click on [clickable]"), "{out}");

    // Reloading the same artifact must not duplicate history.
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({
            "sessionId": "t33-out-b",
            "action": "load",
            "what": "outcomes",
            "file": "t33-out-a_outcomes.nda",
        }),
    );
    assert!(
        out.contains("restored 0 action outcome(s)"),
        "reload is idempotent: {out}"
    );
    assert!(out.contains("2 outcome(s) now recorded"), "{out}");
}

#[test]
fn learn_tool_bundles_all_experience_stores() {
    load("t34-all-a");
    let root = temp_root("learn34");

    // Build experience in all three stores: a successful fill records
    // confidence + an outcome, and remember stores a page memory.
    call(
        "browser_native_fill_label",
        json!({ "sessionId": "t34-all-a", "label": "Email", "text": "a@b.example" }),
    );
    call(
        "browser_native_remember",
        json!({
            "sessionId": "t34-all-a",
            "note": "charlie-delta-memo pricing page",
            "tags": ["pricing"],
            "outcome": 0.8,
        }),
    );

    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({ "sessionId": "t34-all-a", "action": "save", "what": "all" }),
    );
    assert!(out.contains("1 page memory(ies)"), "{out}");
    assert!(out.contains("1 action outcome(s)"), "{out}");
    assert!(
        out.contains("t34-all-a_all.nda"),
        "output names the artifact: {out}"
    );

    // A fresh session inherits all three stores from the one bundle.
    load("t34-all-b");
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({
            "sessionId": "t34-all-b",
            "action": "load",
            "what": "all",
            "file": "t34-all-a_all.nda",
        }),
    );
    assert!(out.contains("restored"), "{out}");
    assert!(out.contains("1 page memory(ies)"), "{out}");
    assert!(out.contains("1 action outcome(s)"), "{out}");

    let out = call(
        "browser_native_recall",
        json!({ "sessionId": "t34-all-b", "query": "charlie-delta-memo", "mode": "keyword" }),
    );
    assert!(
        out.contains("pricing"),
        "bundled memory is searchable: {out}"
    );

    let out = call(
        "browser_native_reflect",
        json!({ "sessionId": "t34-all-b" }),
    );
    assert!(
        out.contains("Recent action outcomes:"),
        "bundled outcomes feed reflection: {out}"
    );
    assert!(out.contains("fill on [textbox]"), "{out}");

    // Reloading the bundle must not duplicate memories or outcomes.
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({
            "sessionId": "t34-all-b",
            "action": "load",
            "what": "all",
            "file": "t34-all-a_all.nda",
        }),
    );
    assert!(
        out.contains("0 page memory(ies)"),
        "reload is idempotent: {out}"
    );
    assert!(out.contains("0 action outcome(s)"), "{out}");
}

#[test]
fn learn_tool_lists_saved_artifacts() {
    let root = temp_root("t36list");
    // Start from a clean artifact directory so the listing is exact.
    let _ = std::fs::remove_dir_all(root.join(".velocity").join("browser_artifacts"));

    load("t36-list");
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({ "sessionId": "t36-list", "action": "list" }),
    );
    assert!(
        out.contains("(no browser artifacts saved yet)"),
        "empty directory reported: {out}"
    );

    // Save two different stores, then list must surface both with kinds.
    call_rooted(
        &root,
        "browser_native_learn",
        json!({ "sessionId": "t36-list", "action": "save", "what": "confidence" }),
    );
    call_rooted(
        &root,
        "browser_native_learn",
        json!({ "sessionId": "t36-list", "action": "save", "what": "all" }),
    );
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({ "sessionId": "t36-list", "action": "list" }),
    );
    assert!(out.contains("2 artifact(s) in"), "{out}");
    assert!(out.contains("t36-list_all.nda (all,"), "{out}");
    assert!(
        out.contains("t36-list_confidence.nda (confidence,"),
        "{out}"
    );
    assert!(
        out.contains("load one with action=load file=<name>"),
        "{out}"
    );

    // Compact mode returns the same inventory as JSON.
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({ "sessionId": "t36-list", "action": "list", "compact": true }),
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&out).expect("compact list report is valid JSON");
    assert_eq!(parsed["action"], "list");
    let artifacts = parsed["artifacts"].as_array().expect("artifacts array");
    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0]["file"], "t36-list_all.nda");
    assert_eq!(artifacts[0]["kind"], "all");
    assert!(artifacts[0]["bytes"].as_u64().unwrap() > 0);
    assert_eq!(artifacts[1]["kind"], "confidence");
}

#[test]
fn default_experience_bundle_seeds_new_sessions() {
    let root = temp_root("t37seed");
    let _ = std::fs::remove_dir_all(root.join(".velocity").join("browser_artifacts"));

    // Session A builds experience and publishes it as the workspace
    // default bundle.
    load("t37-seed-a");
    call_rooted(
        &root,
        "browser_native_fill_label",
        json!({ "sessionId": "t37-seed-a", "label": "Email", "text": "seed@b.example" }),
    );
    call_rooted(
        &root,
        "browser_native_remember",
        json!({
            "sessionId": "t37-seed-a",
            "note": "golf-hotel-memo checkout page",
            "tags": ["checkout"],
            "outcome": 0.9,
        }),
    );
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({
            "sessionId": "t37-seed-a",
            "action": "save",
            "what": "all",
            "file": "default_all.nda",
        }),
    );
    assert!(out.contains("default_all.nda"), "{out}");
    assert!(out.contains("1 page memory(ies)"), "{out}");

    // A brand-new session inherits everything on its first rooted call —
    // no explicit load needed.
    load("t37-seed-b");
    let out = call_rooted(
        &root,
        "browser_native_recall",
        json!({ "sessionId": "t37-seed-b", "query": "golf-hotel-memo", "mode": "keyword" }),
    );
    assert!(
        out.contains("checkout"),
        "auto-seeded memory is searchable: {out}"
    );

    let out = call_rooted(
        &root,
        "browser_native_reflect",
        json!({ "sessionId": "t37-seed-b" }),
    );
    assert!(
        out.contains("Recent action outcomes:"),
        "auto-seeded outcomes feed reflection: {out}"
    );
    assert!(out.contains("fill on [textbox]"), "{out}");

    // Seeding already applied the bundle, so an explicit load restores 0.
    let out = call_rooted(
        &root,
        "browser_native_learn",
        json!({
            "sessionId": "t37-seed-b",
            "action": "load",
            "what": "all",
            "file": "default_all.nda",
        }),
    );
    assert!(
        out.contains("0 page memory(ies)"),
        "seed already applied: {out}"
    );
    assert!(out.contains("0 action outcome(s)"), "{out}");

    // A session rooted elsewhere (no bundle) stays empty.
    let bare = temp_root("t37bare");
    let _ = std::fs::remove_dir_all(bare.join(".velocity").join("browser_artifacts"));
    load("t37-seed-c");
    let out = call_rooted(
        &bare,
        "browser_native_recall",
        json!({ "sessionId": "t37-seed-c", "query": "golf-hotel-memo", "mode": "keyword" }),
    );
    assert!(
        !out.contains("checkout"),
        "no bundle means no inheritance: {out}"
    );
}

#[test]
fn page_text_formats_render_markdown_tables_and_summary() {
    let html = r#"<html><head><title>Prices</title></head><body>
            <h1>Plan Prices</h1>
            <p>Pick the plan that fits.</p>
            <table>
              <caption>Plans</caption>
              <tr><th>Plan</th><th>Price</th></tr>
              <tr><td>Free</td><td>$0</td></tr>
              <tr><td>Pro</td><td>$9</td></tr>
            </table>
        </body></html>"#;
    get_or_create_native_bridge("t38-fmt")
        .lock()
        .unwrap()
        .load_html("http://local.test/prices", html);

    // Default stays the plain visible-text read.
    let out = call(
        "browser_native_page_text",
        json!({ "sessionId": "t38-fmt" }),
    );
    assert!(out.contains("Plan Prices"), "{out}");
    assert!(out.contains("Pick the plan that fits."), "{out}");

    let out = call(
        "browser_native_page_text",
        json!({ "sessionId": "t38-fmt", "format": "markdown" }),
    );
    assert!(
        out.contains("# Plan Prices"),
        "heading survives as markdown: {out}"
    );

    let out = call(
        "browser_native_page_text",
        json!({ "sessionId": "t38-fmt", "format": "content" }),
    );
    assert!(
        out.contains("# Plan Prices"),
        "content mode reads the body: {out}"
    );

    let out = call(
        "browser_native_page_text",
        json!({ "sessionId": "t38-fmt", "format": "tables" }),
    );
    assert!(out.contains("Plan"), "{out}");
    assert!(
        out.contains("| Free | $0 |"),
        "rows render as markdown cells: {out}"
    );
    assert!(out.contains("| Pro | $9 |"), "{out}");

    let out = call(
        "browser_native_page_text",
        json!({ "sessionId": "t38-fmt", "format": "summary" }),
    );
    assert!(out.contains("Prices"), "summary names the page: {out}");

    // maxChars still bounds every format.
    let out = call(
        "browser_native_page_text",
        json!({ "sessionId": "t38-fmt", "format": "markdown", "maxChars": 10 }),
    );
    assert!(out.contains("(truncated to 10 of"), "{out}");

    let err = handle_native_tool(
        Path::new("."),
        "browser_native_page_text",
        &json!({ "sessionId": "t38-fmt", "format": "csv" }),
    )
    .expect_err("unknown format is rejected");
    assert!(
        err.to_string().contains("unknown page_text format 'csv'"),
        "{err}"
    );
}

#[test]
fn brief_includes_page_structure_digest() {
    let bridge = get_or_create_native_bridge("t39-digest");
    bridge.lock().unwrap().load_html(
        "http://local.test/prices",
        "<html><head><title>Prices</title></head><body>\
             <h1>Plan Prices</h1><h2>Monthly</h2>\
             <a href=\"/signup\">Sign up</a>\
             <table><tr><td>x</td></tr></table></body></html>",
    );
    let out = call("browser_native_brief", json!({ "sessionId": "t39-digest" }));
    assert!(out.contains("brief for http://local.test/prices"), "{out}");
    assert!(
        out.contains("1 link(s)"),
        "counts surface in the brief: {out}"
    );
    assert!(out.contains("1 table(s)"), "{out}");
    assert!(out.contains("Headings:"), "{out}");
    assert!(out.contains("# Plan Prices"), "{out}");
    assert!(out.contains("## Monthly"), "{out}");
    assert!(
        out.contains("Content: ") && out.contains(" chars\n"),
        "distilled content size surfaces in the brief: {out}"
    );

    let compact = call(
        "browser_native_brief",
        json!({ "sessionId": "t39-digest", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact brief is valid JSON");
    let digest = report["digest"].as_str().expect("digest present");
    assert!(digest.contains("1 link(s)"), "{compact}");
    assert!(digest.contains("## Monthly"), "{compact}");
    assert!(
        !digest.contains("Page: Prices"),
        "identity line stays out of digest: {compact}"
    );
    let content_chars = report["contentChars"]
        .as_u64()
        .expect("contentChars present when a page is loaded");
    assert!(content_chars > 0, "content size is non-zero: {compact}");
}

#[test]
fn brief_tool_bundles_pre_action_context() {
    load("t35-brief");

    // A fresh session's brief is just the page identity.
    let out = call("browser_native_brief", json!({ "sessionId": "t35-brief" }));
    assert!(out.contains("brief for http://local.test/form"), "{out}");
    assert!(out.contains("\"Signup\""), "{out}");
    assert!(!out.contains("learned patterns"), "{out}");
    assert!(!out.contains("similar remembered pages"), "{out}");

    // Build experience: a confident fill, a remembered page and two
    // repeated failures for the reflector to chew on.
    call(
        "browser_native_fill_label",
        json!({ "sessionId": "t35-brief", "label": "Email", "text": "a@b.example" }),
    );
    call(
        "browser_native_remember",
        json!({
            "sessionId": "t35-brief",
            "note": "signup form with email subscribe plan",
            "tags": ["signup"],
            "outcome": 0.9,
        }),
    );
    for _ in 0..2 {
        call(
            "browser_native_click_text",
            json!({ "sessionId": "t35-brief", "text": "Launch Rocket" }),
        );
    }

    let out = call("browser_native_brief", json!({ "sessionId": "t35-brief" }));
    assert!(out.contains("learned patterns on this domain:"), "{out}");
    assert!(out.contains("fill on textbox:"), "{out}");
    assert!(out.contains("similar remembered pages:"), "{out}");
    assert!(out.contains("outcome 0.90"), "{out}");
    assert!(
        out.contains("[SELF-REFLECTION]"),
        "failures surface as lessons: {out}"
    );
    assert!(out.contains("Recent action outcomes:"), "{out}");

    let compact = call(
        "browser_native_brief",
        json!({ "sessionId": "t35-brief", "compact": true }),
    );
    let report: serde_json::Value =
        serde_json::from_str(&compact).expect("compact brief is valid JSON");
    assert_eq!(report["url"], "http://local.test/form", "{compact}");
    assert_eq!(report["title"], "Signup", "{compact}");
    assert!(
        report["elements"].as_u64().expect("elements") > 0,
        "{compact}"
    );
    assert!(
        !report["patterns"].as_array().expect("patterns").is_empty(),
        "{compact}"
    );
    assert!(
        !report["memories"].as_array().expect("memories").is_empty(),
        "{compact}"
    );
    assert_eq!(
        report["outcomes"].as_array().expect("outcomes").len(),
        3,
        "{compact}"
    );
}
