//! Tests for the agent empowerment layer — interactive elements, content
//! extraction, page summary, CSS selectors, and DOM state diffing.

use super::*;

// ── Interactive Elements ─────────────────────────────────────────────────────

#[test]
fn get_interactive_elements_returns_array() {
    let result = eval_full("typeof document.getInteractiveElements()");
    assert_eq!(result, JsValue::String("object".to_string()));
}

#[test]
fn get_interactive_elements_finds_button() {
    eval_full("document.body.innerHTML = '<button>Click Me</button>'");
    let result = eval_full("document.getInteractiveElements().length");
    assert!(to_number(&result) >= 1.0);
}

#[test]
fn get_interactive_elements_finds_link() {
    eval_full("document.body.innerHTML = '<a href=\"/test\">Go</a>'");
    let result = eval_full("document.getInteractiveElements().length");
    assert!(to_number(&result) >= 1.0);
}

#[test]
fn get_interactive_elements_finds_input() {
    eval_full("document.body.innerHTML = '<input type=\"text\" name=\"email\" placeholder=\"Email\">'");
    let result = eval_full("document.getInteractiveElements().length");
    assert!(to_number(&result) >= 1.0);
}

// ── Form Fill Primitives ─────────────────────────────────────────────────────

#[test]
fn fill_by_label_sets_value() {
    eval_full("document.body.innerHTML = '<input type=\"text\" placeholder=\"Email\">'");
    let ok = eval_full("document.fillByLabel('Email', 'a@b.com')");
    assert_eq!(ok, JsValue::Boolean(true));
    let value = eval_full("document.querySelector('input').value");
    assert_eq!(value, JsValue::String("a@b.com".to_string()));
}

#[test]
fn fill_by_label_matches_label_element() {
    eval_full("document.body.innerHTML = '<label for=\"u\">Username</label><input id=\"u\" type=\"text\">'");
    let ok = eval_full("document.fillByLabel('username', 'alice')");
    assert_eq!(ok, JsValue::Boolean(true));
    let value = eval_full("document.getElementById('u').value");
    assert_eq!(value, JsValue::String("alice".to_string()));
}

#[test]
fn fill_by_label_fires_input_listener() {
    let result = eval_full("
        document.body.innerHTML = '<input type=\"text\" placeholder=\"City\">';
        var events = '';
        document.querySelector('input').addEventListener('input', function() { events = events + 'i'; });
        document.querySelector('input').addEventListener('change', function() { events = events + 'c'; });
        document.fillByLabel('City', 'Lisbon');
        events
    ");
    assert_eq!(result, JsValue::String("ic".to_string()));
}

#[test]
fn fill_by_label_misses_unknown_field() {
    eval_full("document.body.innerHTML = '<input type=\"text\" placeholder=\"Email\">'");
    let ok = eval_full("document.fillByLabel('Phone', '123')");
    assert_eq!(ok, JsValue::Boolean(false));
}

#[test]
fn fill_by_label_skips_disabled_control() {
    eval_full("document.body.innerHTML = '<input type=\"text\" placeholder=\"Email\" disabled>'");
    let ok = eval_full("document.fillByLabel('Email', 'x')");
    assert_eq!(ok, JsValue::Boolean(false));
}

#[test]
fn fill_by_label_prefers_exact_match() {
    eval_full("document.body.innerHTML = '<input placeholder=\"Name of company\"><input placeholder=\"Name\">'");
    eval_full("document.fillByLabel('Name', 'exact')");
    let value = eval_full("document.querySelectorAll('input')[1].value");
    assert_eq!(value, JsValue::String("exact".to_string()));
}

#[test]
fn check_by_label_sets_checked() {
    eval_full("document.body.innerHTML = '<label for=\"t\">Accept terms</label><input id=\"t\" type=\"checkbox\">'");
    let ok = eval_full("document.checkByLabel('Accept terms')");
    assert_eq!(ok, JsValue::Boolean(true));
    let checked = eval_full("document.getElementById('t').hasAttribute('checked')");
    assert_eq!(checked, JsValue::Boolean(true));
}

#[test]
fn check_by_label_unchecks_with_false() {
    eval_full("document.body.innerHTML = '<input type=\"checkbox\" aria-label=\"News\" checked>'");
    let ok = eval_full("document.checkByLabel('News', false)");
    assert_eq!(ok, JsValue::Boolean(true));
    let checked = eval_full("document.querySelector('input').hasAttribute('checked')");
    assert_eq!(checked, JsValue::Boolean(false));
}

#[test]
fn check_by_label_ignores_text_inputs() {
    eval_full("document.body.innerHTML = '<input type=\"text\" placeholder=\"Search\">'");
    let ok = eval_full("document.checkByLabel('Search')");
    assert_eq!(ok, JsValue::Boolean(false));
}

// ── Batch Form Primitives ────────────────────────────────────────────────────

#[test]
fn fill_form_by_label_fills_multiple_fields() {
    eval_full("document.body.innerHTML = '<input placeholder=\"Email\"><input placeholder=\"City\">'");
    let filled = eval_full("document.fillFormByLabel({Email: 'a@b.com', City: 'Lisbon'}).filled");
    assert_eq!(filled, JsValue::Number(2.0));
    let value = eval_full("document.querySelectorAll('input')[1].value");
    assert_eq!(value, JsValue::String("Lisbon".to_string()));
}

#[test]
fn fill_form_by_label_routes_booleans_to_checkboxes() {
    eval_full("document.body.innerHTML = '<input placeholder=\"Email\"><input type=\"checkbox\" aria-label=\"Terms\">'");
    let filled = eval_full("document.fillFormByLabel({Email: 'x@y.z', Terms: true}).filled");
    assert_eq!(filled, JsValue::Number(2.0));
    let checked = eval_full("document.querySelectorAll('input')[1].hasAttribute('checked')");
    assert_eq!(checked, JsValue::Boolean(true));
}

#[test]
fn fill_form_by_label_reports_missed_labels() {
    eval_full("document.body.innerHTML = '<input placeholder=\"Email\">'");
    let missed = eval_full("document.fillFormByLabel({Email: 'a', Phone: 'b'}).missed");
    assert_eq!(missed, JsValue::Array(vec![JsValue::String("Phone".to_string())]));
}

#[test]
fn read_form_lists_controls_with_state() {
    eval_full("document.body.innerHTML = '<input placeholder=\"Email\" value=\"a@b.com\"><input type=\"checkbox\" aria-label=\"Terms\" checked>'");
    let count = eval_full("document.readForm().length");
    assert_eq!(count, JsValue::Number(2.0));
    let labels = eval_full("document.readForm().map(function(c) { return c.label; }).sort().join(',')");
    assert_eq!(labels, JsValue::String("Email,Terms".to_string()));
}

#[test]
fn read_form_text_is_token_cheap() {
    eval_full("document.body.innerHTML = '<input placeholder=\"Email\" value=\"a@b.com\"><input type=\"checkbox\" aria-label=\"Terms\" checked>'");
    let text = eval_full("document.readFormText()");
    if let JsValue::String(s) = &text {
        assert!(s.contains("Email [textbox] = a@b.com"), "got: {}", s);
        assert!(s.contains("Terms [checkbox] = checked"), "got: {}", s);
    } else {
        panic!("Expected string form text");
    }
}

#[test]
fn read_form_reflects_fill_by_label() {
    eval_full("
        document.body.innerHTML = '<input placeholder=\"Email\">';
        document.fillByLabel('Email', 'new@val.ue');
    ");
    let text = eval_full("document.readFormText()");
    if let JsValue::String(s) = &text {
        assert!(s.contains("Email [textbox] = new@val.ue"), "got: {}", s);
    } else {
        panic!("Expected string form text");
    }
}

#[test]
fn submit_form_fires_submit_listener() {
    let result = eval_full("
        document.body.innerHTML = '<form><input placeholder=\"Q\"></form>';
        var submitted = false;
        document.querySelector('form').addEventListener('submit', function() { submitted = true; });
        document.submitForm();
        submitted
    ");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn submit_form_without_form_returns_false() {
    eval_full("document.body.innerHTML = '<p>No form here</p>'");
    let ok = eval_full("document.submitForm()");
    assert_eq!(ok, JsValue::Boolean(false));
}

#[test]
fn interactive_element_has_role() {
    eval_full("document.body.innerHTML = '<button>Submit</button>'");
    let result = eval_full("document.getInteractiveElements()[0].role");
    assert_eq!(result, JsValue::String("button".to_string()));
}

#[test]
fn interactive_element_has_name() {
    eval_full("document.body.innerHTML = '<button>Submit</button>'");
    let result = eval_full("document.getInteractiveElements()[0].name");
    assert_eq!(result, JsValue::String("Submit".to_string()));
}

#[test]
fn interactive_element_has_selector() {
    eval_full("document.body.innerHTML = '<button id=\"btn\">Go</button>'");
    let result = eval_full("document.getInteractiveElements()[0].selector");
    assert_eq!(result, JsValue::String("#btn".to_string()));
}

#[test]
fn interactive_element_disabled_flag() {
    eval_full("document.body.innerHTML = '<button disabled>No</button>'");
    let result = eval_full("document.getInteractiveElements()[0].disabled");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn hidden_elements_excluded() {
    eval_full("document.body.innerHTML = '<button hidden>Hidden</button>'");
    let result = eval_full("document.getInteractiveElements().length");
    assert_eq!(to_number(&result), 0.0);
}

#[test]
fn aria_hidden_elements_excluded() {
    eval_full("document.body.innerHTML = '<button aria-hidden=\"true\">Hidden</button>'");
    let result = eval_full("document.getInteractiveElements().length");
    assert_eq!(to_number(&result), 0.0);
}

#[test]
fn interactive_elements_text_format() {
    eval_full("document.body.innerHTML = '<button>Click</button>'");
    let result = eval_full("typeof document.getInteractiveElementsText()");
    assert_eq!(result, JsValue::String("string".to_string()));
}

#[test]
fn multiple_interactive_elements() {
    eval_full("document.body.innerHTML = '<button>A</button><a href=\"#\">B</a><input type=\"text\">'");
    let result = eval_full("document.getInteractiveElements().length");
    assert!(to_number(&result) >= 3.0);
}

#[test]
fn select_element_is_combobox() {
    eval_full("document.body.innerHTML = '<select><option>A</option></select>'");
    let els = eval_full("document.getInteractiveElements()");
    if let JsValue::Array(arr) = &els {
        if let Some(JsValue::Object(first)) = arr.first() {
            assert_eq!(first.get("role").and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }), Some("combobox"));
        }
    }
}

#[test]
fn checkbox_role() {
    eval_full("document.body.innerHTML = '<input type=\"checkbox\">'");
    let els = eval_full("document.getInteractiveElements()");
    if let JsValue::Array(arr) = &els {
        if let Some(JsValue::Object(first)) = arr.first() {
            assert_eq!(first.get("role").and_then(|v| if let JsValue::String(s) = v { Some(s.as_str()) } else { None }), Some("checkbox"));
        }
    }
}

// ── Content Extraction ───────────────────────────────────────────────────────

#[test]
fn extract_content_returns_array() {
    let result = eval_full("typeof document.extractContent()");
    assert_eq!(result, JsValue::String("object".to_string()));
}

#[test]
fn extract_content_finds_paragraph() {
    eval_full("document.body.innerHTML = '<p>This is a long paragraph with enough text to be considered content by the extractor.</p>'");
    let result = eval_full("document.extractContent().length");
    assert!(to_number(&result) >= 1.0);
}

#[test]
fn extract_content_skips_nav() {
    eval_full("document.body.innerHTML = '<nav><a href=\"/\">Home</a></nav><p>This is the main content paragraph with enough text.</p>'");
    let result = eval_full("document.extractContent().length");
    // Should have the paragraph but not the nav content as a separate block.
    assert!(to_number(&result) >= 1.0);
}

#[test]
fn extract_content_block_has_text() {
    eval_full("document.body.innerHTML = '<p>Meaningful content here for testing.</p>'");
    let result = eval_full("document.extractContent()[0].text");
    if let JsValue::String(s) = result {
        assert!(s.contains("Meaningful content"));
    } else {
        panic!("Expected string");
    }
}

// ── Page Summary ─────────────────────────────────────────────────────────────

#[test]
fn summarize_page_returns_object() {
    let result = eval_full("typeof document.summarizePage()");
    assert_eq!(result, JsValue::String("object".to_string()));
}

#[test]
fn summarize_page_has_title() {
    eval_full("document.head.innerHTML = '<title>Test Page</title>'");
    let result = eval_full("document.summarizePage().title");
    assert_eq!(result, JsValue::String("Test Page".to_string()));
}

#[test]
fn summarize_page_counts_links() {
    eval_full("document.body.innerHTML = '<a href=\"/a\">A</a><a href=\"/b\">B</a>'");
    let result = eval_full("document.summarizePage().links");
    assert!(to_number(&result) >= 2.0);
}

#[test]
fn summarize_page_counts_forms() {
    eval_full("document.body.innerHTML = '<form><input></form>'");
    let result = eval_full("document.summarizePage().forms");
    assert!(to_number(&result) >= 1.0);
}

#[test]
fn summarize_page_counts_interactive() {
    eval_full("document.body.innerHTML = '<button>A</button><a href=\"#\">B</a><input>'");
    let result = eval_full("document.summarizePage().interactive");
    assert!(to_number(&result) >= 2.0);
}

#[test]
fn summarize_page_headings() {
    eval_full("document.body.innerHTML = '<h1>Title</h1><h2>Section</h2>'");
    let result = eval_full("document.summarizePage().headings.length");
    assert!(to_number(&result) >= 2.0);
}

#[test]
fn summarize_page_text_format() {
    eval_full("document.head.innerHTML = '<title>Test</title>'");
    let result = eval_full("typeof document.summarizePageText()");
    assert_eq!(result, JsValue::String("string".to_string()));
}

// ── DOM State / Diff ─────────────────────────────────────────────────────────

#[test]
fn capture_state_returns_object() {
    let result = eval_full("typeof document.captureState()");
    assert_eq!(result, JsValue::String("object".to_string()));
}

#[test]
fn capture_state_has_node_count() {
    eval_full("document.body.innerHTML = '<p>Hello</p>'");
    let result = eval_full("document.captureState().nodeCount");
    assert!(to_number(&result) > 0.0);
}

#[test]
fn capture_state_has_interactive_count() {
    eval_full("document.body.innerHTML = '<button>Click</button>'");
    let result = eval_full("document.captureState().interactiveCount");
    assert!(to_number(&result) >= 1.0);
}

#[test]
fn capture_state_has_text_hash() {
    eval_full("document.body.innerHTML = '<p>Content</p>'");
    let result = eval_full("typeof document.captureState().textHash");
    assert_eq!(result, JsValue::String("number".to_string()));
}

// ── CSS Selectors ────────────────────────────────────────────────────────────

#[test]
fn selector_uses_id() {
    eval_full("document.body.innerHTML = '<button id=\"submit-btn\">Go</button>'");
    let result = eval_full("document.getInteractiveElements()[0].selector");
    assert_eq!(result, JsValue::String("#submit-btn".to_string()));
}

#[test]
fn selector_uses_name_for_inputs() {
    eval_full("document.body.innerHTML = '<input name=\"email\">'");
    let result = eval_full("document.getInteractiveElements()[0].selector");
    assert_eq!(result, JsValue::String("input[name=\"email\"]".to_string()));
}

// ── Integration: Full Agent Workflow ─────────────────────────────────────────

#[test]
fn agent_workflow_summarize_then_interact() {
    eval_full(r#"
        document.head.innerHTML = '<title>Login Page</title>';
        document.body.innerHTML = '<h1>Welcome</h1><form><input name="user" placeholder="Username"><input name="pass" type="password" placeholder="Password"><button>Login</button></form>';
    "#);
    // Step 1: Summarize.
    let title = eval_full("document.summarizePage().title");
    assert_eq!(title, JsValue::String("Login Page".to_string()));
    // Step 2: Get interactive elements.
    let count = eval_full("document.getInteractiveElements().length");
    assert!(to_number(&count) >= 3.0); // 2 inputs + 1 button
    // Step 3: Capture state.
    let state = eval_full("document.captureState()");
    if let JsValue::Object(m) = &state {
        assert!(m.contains_key("nodeCount"));
    } else {
        panic!("Expected object");
    }
}

// ── Table Extraction ─────────────────────────────────────────────────────────

#[test]
fn extract_tables_finds_table() {
    eval_full("document.body.innerHTML = '<table><tr><th>Name</th><th>Age</th></tr><tr><td>Alice</td><td>30</td></tr></table>'");
    let result = eval_full("document.extractTables().length");
    assert_eq!(to_number(&result), 1.0);
}

#[test]
fn extract_tables_headers() {
    eval_full("document.body.innerHTML = '<table><tr><th>Name</th><th>Age</th></tr><tr><td>Alice</td><td>30</td></tr></table>'");
    let result = eval_full("document.extractTables()[0].headers[0]");
    assert_eq!(result, JsValue::String("Name".to_string()));
}

#[test]
fn extract_tables_rows() {
    eval_full("document.body.innerHTML = '<table><tr><th>Name</th></tr><tr><td>Alice</td></tr><tr><td>Bob</td></tr></table>'");
    let result = eval_full("document.extractTables()[0].rows.length");
    assert_eq!(to_number(&result), 2.0);
}

#[test]
fn extract_tables_cell_value() {
    eval_full("document.body.innerHTML = '<table><tr><td>CellValue</td></tr></table>'");
    let result = eval_full("document.extractTables()[0].rows[0][0]");
    assert_eq!(result, JsValue::String("CellValue".to_string()));
}

#[test]
fn extract_tables_text_is_markdown() {
    eval_full("document.body.innerHTML = '<table><tr><th>H</th></tr><tr><td>V</td></tr></table>'");
    let result = eval_full("document.extractTablesText()");
    if let JsValue::String(s) = &result {
        assert!(s.contains("| H |"), "got: {}", s);
        assert!(s.contains("| --- |"), "got: {}", s);
        assert!(s.contains("| V |"), "got: {}", s);
    } else {
        panic!("Expected string");
    }
}

#[test]
fn extract_tables_empty_page() {
    eval_full("document.body.innerHTML = '<p>No tables here</p>'");
    let result = eval_full("document.extractTables().length");
    assert_eq!(to_number(&result), 0.0);
}

// ── Page-to-Markdown ─────────────────────────────────────────────────────────

#[test]
fn to_markdown_returns_string() {
    eval_full("document.body.innerHTML = '<p>Hello world content</p>'");
    let result = eval_full("typeof document.toMarkdown()");
    assert_eq!(result, JsValue::String("string".to_string()));
}

#[test]
fn to_markdown_headings() {
    eval_full("document.body.innerHTML = '<h2>Section Title</h2><p>Body text here</p>'");
    let result = eval_full("document.toMarkdown()");
    if let JsValue::String(s) = &result {
        assert!(s.contains("## Section Title"), "got: {}", s);
        assert!(s.contains("Body text here"), "got: {}", s);
    } else {
        panic!("Expected string");
    }
}

#[test]
fn to_markdown_links() {
    eval_full("document.body.innerHTML = '<p>See <a href=\"/docs\">the docs</a> now</p>'");
    let result = eval_full("document.toMarkdown()");
    if let JsValue::String(s) = &result {
        assert!(s.contains("[the docs](/docs)"), "got: {}", s);
    } else {
        panic!("Expected string");
    }
}

#[test]
fn to_markdown_lists() {
    eval_full("document.body.innerHTML = '<ul><li>First item</li><li>Second item</li></ul>'");
    let result = eval_full("document.toMarkdown()");
    if let JsValue::String(s) = &result {
        assert!(s.contains("- First item"), "got: {}", s);
        assert!(s.contains("- Second item"), "got: {}", s);
    } else {
        panic!("Expected string");
    }
}

#[test]
fn to_markdown_ordered_lists() {
    eval_full("document.body.innerHTML = '<ol><li>Step one</li><li>Step two</li></ol>'");
    let result = eval_full("document.toMarkdown()");
    if let JsValue::String(s) = &result {
        assert!(s.contains("1. Step one"), "got: {}", s);
        assert!(s.contains("2. Step two"), "got: {}", s);
    } else {
        panic!("Expected string");
    }
}

#[test]
fn to_markdown_skips_boilerplate() {
    eval_full("document.body.innerHTML = '<nav>Navigation junk</nav><p>Real content stays</p>'");
    let result = eval_full("document.toMarkdown()");
    if let JsValue::String(s) = &result {
        assert!(!s.contains("Navigation junk"), "got: {}", s);
        assert!(s.contains("Real content stays"), "got: {}", s);
    } else {
        panic!("Expected string");
    }
}

#[test]
fn to_markdown_bold_and_code() {
    eval_full("document.body.innerHTML = '<p>Use <strong>bold</strong> and <code>inline()</code> text</p>'");
    let result = eval_full("document.toMarkdown()");
    if let JsValue::String(s) = &result {
        assert!(s.contains("**bold**"), "got: {}", s);
        assert!(s.contains("`inline()`"), "got: {}", s);
    } else {
        panic!("Expected string");
    }
}

#[test]
fn to_markdown_includes_title() {
    eval_full(r#"
        document.head.innerHTML = '<title>Doc Title</title>';
        document.body.innerHTML = '<p>Some content</p>';
    "#);
    let result = eval_full("document.toMarkdown()");
    if let JsValue::String(s) = &result {
        assert!(s.contains("# Doc Title"), "got: {}", s);
    } else {
        panic!("Expected string");
    }
}

// ── Bulk Form Fill ───────────────────────────────────────────────────────────

#[test]
fn fill_form_by_name() {
    eval_full("document.body.innerHTML = '<input name=\"email\">'");
    let result = eval_full("document.fillForm({email: 'a@b.com'})[0].ok");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn fill_form_sets_value() {
    eval_full(r#"
        document.body.innerHTML = '<input name="email">';
        document.fillForm({email: 'a@b.com'});
    "#);
    let result = eval_full("document.getInteractiveElements()[0].value");
    assert_eq!(result, JsValue::String("a@b.com".to_string()));
}

#[test]
fn fill_form_by_id() {
    eval_full("document.body.innerHTML = '<input id=\"user-field\">'");
    let result = eval_full("document.fillForm({'user-field': 'bob'})[0].ok");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn fill_form_missing_field() {
    eval_full("document.body.innerHTML = '<input name=\"email\">'");
    let result = eval_full("document.fillForm({nonexistent: 'x'})[0].ok");
    assert_eq!(result, JsValue::Boolean(false));
}

#[test]
fn fill_form_missing_field_reason() {
    eval_full("document.body.innerHTML = '<p>no inputs</p>'");
    let result = eval_full("document.fillForm({email: 'x'})[0].reason");
    assert_eq!(result, JsValue::String("not found".to_string()));
}

#[test]
fn fill_form_disabled_field() {
    eval_full("document.body.innerHTML = '<input name=\"email\" disabled>'");
    let result = eval_full("document.fillForm({email: 'x'})[0].reason");
    assert_eq!(result, JsValue::String("disabled".to_string()));
}

#[test]
fn fill_form_checkbox() {
    eval_full(r#"
        document.body.innerHTML = '<input type="checkbox" name="agree">';
        document.fillForm({agree: 'true'});
    "#);
    let result = eval_full("document.querySelector('input').hasAttribute('checked')");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn fill_form_multiple_fields() {
    eval_full("document.body.innerHTML = '<input name=\"user\"><input name=\"pass\">'");
    let result = eval_full("document.fillForm({user: 'alice', pass: 'secret'}).length");
    assert_eq!(to_number(&result), 2.0);
}

#[test]
fn fill_form_fires_input_event() {
    let result = eval_full(r#"
        document.body.innerHTML = '<input name="email">';
        var fired = false;
        document.querySelector('input').addEventListener('input', function() { fired = true; });
        document.fillForm({email: 'a@b.com'});
        fired
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn fill_form_fires_change_event() {
    let result = eval_full(r#"
        document.body.innerHTML = '<input type="checkbox" name="agree">';
        var changed = false;
        document.querySelector('input').addEventListener('change', function() { changed = true; });
        document.fillForm({agree: 'true'});
        changed
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

// ── Link Map ─────────────────────────────────────────────────────────────────

#[test]
fn get_links_finds_links() {
    eval_full("document.body.innerHTML = '<a href=\"/one\">One</a><a href=\"/two\">Two</a>'");
    let result = eval_full("document.getLinks().length");
    assert_eq!(to_number(&result), 2.0);
}

#[test]
fn get_links_has_text_and_href() {
    eval_full("document.body.innerHTML = '<a href=\"/about\">About Us</a>'");
    let text = eval_full("document.getLinks()[0].text");
    let href = eval_full("document.getLinks()[0].href");
    assert_eq!(text, JsValue::String("About Us".to_string()));
    assert_eq!(href, JsValue::String("/about".to_string()));
}

#[test]
fn get_links_deduplicates() {
    eval_full("document.body.innerHTML = '<a href=\"/same\">A</a><a href=\"/same\">B</a>'");
    let result = eval_full("document.getLinks().length");
    assert_eq!(to_number(&result), 1.0);
}

#[test]
fn get_links_skips_fragments_and_js() {
    eval_full("document.body.innerHTML = '<a href=\"#top\">Top</a><a href=\"javascript:void(0)\">JS</a><a href=\"/real\">Real</a>'");
    let result = eval_full("document.getLinks().length");
    assert_eq!(to_number(&result), 1.0);
}

#[test]
fn get_links_text_format() {
    eval_full("document.body.innerHTML = '<a href=\"/page\">My Page</a>'");
    let result = eval_full("document.getLinksText()");
    if let JsValue::String(s) = &result {
        assert!(s.contains("My Page"), "got: {}", s);
        assert!(s.contains("/page"), "got: {}", s);
    } else {
        panic!("Expected string");
    }
}

// ── Semantic Element Finding ─────────────────────────────────────────────────

#[test]
fn find_by_text_finds_button() {
    eval_full("document.body.innerHTML = '<button>Login</button><p>Other stuff</p>'");
    let result = eval_full("document.findByText('Login').length");
    assert_eq!(to_number(&result), 1.0);
}

#[test]
fn find_by_text_case_insensitive() {
    eval_full("document.body.innerHTML = '<button>Submit Order</button>'");
    let result = eval_full("document.findByText('submit order').length");
    assert_eq!(to_number(&result), 1.0);
}

#[test]
fn find_by_text_exact_flag() {
    eval_full("document.body.innerHTML = '<button>Login</button>'");
    let result = eval_full("document.findByText('Login')[0].exact");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn find_by_text_interactive_flag() {
    eval_full("document.body.innerHTML = '<button>Go</button>'");
    let result = eval_full("document.findByText('Go')[0].interactive");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn find_by_text_returns_deepest() {
    eval_full("document.body.innerHTML = '<div><p><span>Target text here</span></p></div>'");
    let result = eval_full("document.findByText('Target text')[0].tagName");
    assert_eq!(result, JsValue::String("SPAN".to_string()));
}

#[test]
fn find_by_text_no_match() {
    eval_full("document.body.innerHTML = '<p>Nothing relevant</p>'");
    let result = eval_full("document.findByText('missing').length");
    assert_eq!(to_number(&result), 0.0);
}

#[test]
fn find_by_text_has_selector() {
    eval_full("document.body.innerHTML = '<button id=\"go-btn\">Go Now</button>'");
    let result = eval_full("document.findByText('Go Now')[0].selector");
    assert_eq!(result, JsValue::String("#go-btn".to_string()));
}

// ── Click Dispatch ───────────────────────────────────────────────────────────

#[test]
fn click_fires_listener() {
    let result = eval_full(r#"
        document.body.innerHTML = '<button id="b">Go</button>';
        var clicked = false;
        document.querySelector('#b').addEventListener('click', function() { clicked = true; });
        document.querySelector('#b').click();
        clicked
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn click_bubbles_to_parent() {
    let result = eval_full(r#"
        document.body.innerHTML = '<div id="wrap"><button id="b">Go</button></div>';
        var bubbled = false;
        document.querySelector('#wrap').addEventListener('click', function() { bubbled = true; });
        document.querySelector('#b').click();
        bubbled
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn click_by_text_fires_listener() {
    let result = eval_full(r#"
        document.body.innerHTML = '<button>Save Draft</button>';
        var saved = false;
        document.querySelector('button').addEventListener('click', function() { saved = true; });
        document.clickByText('Save Draft');
        saved
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn click_by_text_returns_true_on_hit() {
    eval_full("document.body.innerHTML = '<button>Continue</button>'");
    let result = eval_full("document.clickByText('Continue')");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn click_by_text_returns_false_on_miss() {
    eval_full("document.body.innerHTML = '<p>Plain paragraph text</p>'");
    let result = eval_full("document.clickByText('Nonexistent Button')");
    assert_eq!(result, JsValue::Boolean(false));
}

#[test]
fn click_by_text_resolves_span_inside_button() {
    let result = eval_full(r#"
        document.body.innerHTML = '<button id="b"><span>Buy Now</span></button>';
        var bought = false;
        document.querySelector('#b').addEventListener('click', function() { bought = true; });
        document.clickByText('Buy Now');
        bought
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

// ── State Diff ───────────────────────────────────────────────────────────────

#[test]
fn diff_state_unchanged() {
    let result = eval_full(r#"
        document.body.innerHTML = '<p>Stable content</p>';
        var before = document.captureState();
        document.diffState(before).changed
    "#);
    assert_eq!(result, JsValue::Boolean(false));
}

#[test]
fn diff_state_detects_change() {
    let result = eval_full(r#"
        document.body.innerHTML = '<p>Before content</p>';
        var before = document.captureState();
        document.body.innerHTML = '<p>After content</p><button>New</button>';
        document.diffState(before).changed
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn diff_state_text_changed_flag() {
    let result = eval_full(r#"
        document.body.innerHTML = '<p>Original text</p>';
        var before = document.captureState();
        document.body.innerHTML = '<p>Modified text</p>';
        document.diffState(before).textChanged
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

// ── NDA Export ───────────────────────────────────────────────────────────────

#[test]
fn export_nda_has_facts() {
    eval_full(r#"
        document.head.innerHTML = '<title>NDA Page</title>';
        document.body.innerHTML = '<button>Act</button><a href="/x">Link</a>';
    "#);
    let doc = super::super::agent_layer::export_agent_state_nda();
    assert!(!doc.facts.is_empty());
}

#[test]
fn export_nda_contains_title_fact() {
    eval_full(r#"
        document.head.innerHTML = '<title>NDA Title Test</title>';
        document.body.innerHTML = '<p>content</p>';
    "#);
    let doc = super::super::agent_layer::export_agent_state_nda();
    let facts = doc.readable_facts();
    assert!(facts.iter().any(|(s, p, o)| s == "page"
        && *p == crate::predicates::SESSION_TITLE
        && o == "NDA Title Test"));
}

#[test]
fn export_nda_contains_interactive_element_facts() {
    eval_full("document.body.innerHTML = '<button id=\"go\">Go Now</button>'");
    let doc = super::super::agent_layer::export_agent_state_nda();
    let facts = doc.readable_facts();
    assert!(facts.iter().any(|(_, p, o)| *p == crate::predicates::AOM_ROLE && o == "button"));
    assert!(facts.iter().any(|(_, p, o)| *p == crate::predicates::AOM_NAME && o == "Go Now"));
    assert!(facts.iter().any(|(_, p, o)| *p == crate::predicates::AOM_SELECTOR && o == "#go"));
}

#[test]
fn export_nda_binary_roundtrip() {
    eval_full(r#"
        document.head.innerHTML = '<title>Roundtrip</title>';
        document.body.innerHTML = '<button>Press</button>';
    "#);
    let doc = super::super::agent_layer::export_agent_state_nda();
    let bytes = doc.to_binary_stream();
    let decoded = crate::nda::NdaDocument::from_binary_stream(&bytes).expect("decode");
    assert_eq!(doc.readable_facts(), decoded.readable_facts());
}

#[test]
fn export_nda_disabled_fact() {
    eval_full("document.body.innerHTML = '<button disabled>Off</button>'");
    let doc = super::super::agent_layer::export_agent_state_nda();
    let facts = doc.readable_facts();
    assert!(facts.iter().any(|(_, p, o)| *p == crate::predicates::AOM_DISABLED && o == "1"));
}

#[test]
fn export_nda_text_via_js() {
    eval_full(r#"
        document.head.innerHTML = '<title>JS NDA</title>';
        document.body.innerHTML = '<button>Do It</button>';
    "#);
    let result = eval_full("document.exportNdaText()");
    if let JsValue::String(s) = &result {
        assert!(s.contains("JS NDA"), "got: {}", s);
        assert!(s.contains("Do It"), "got: {}", s);
    } else {
        panic!("Expected string");
    }
}

#[test]
fn export_nda_bytes_via_js() {
    eval_full("document.body.innerHTML = '<button>B</button>'");
    let result = eval_full("document.exportNdaBytes().length");
    assert!(to_number(&result) > 0.0);
}

// ── Wait For Settlement ──────────────────────────────────────────────────────

#[test]
fn wait_for_settlement_settles_when_quiet() {
    let result = eval_full("var s = document.waitForSettlement(); s.settled");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn wait_for_settlement_runs_pending_timers() {
    let result = eval_full("var x = 0; setTimeout(function(){ x = 7; }, 0); document.waitForSettlement(); x");
    assert_eq!(result, JsValue::Number(7.0));
}

#[test]
fn wait_for_settlement_runs_chained_timers() {
    let result = eval_full(
        "var x = 0; setTimeout(function(){ x = 1; setTimeout(function(){ x = 2; }, 0); }, 0); document.waitForSettlement(); x"
    );
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn wait_for_settlement_reports_timer_count() {
    let result = eval_full("setTimeout(function(){}, 0); setTimeout(function(){}, 0); var s = document.waitForSettlement(); s.timersRun");
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn wait_for_settlement_never_settles_with_interval() {
    let result = eval_full("setInterval(function(){}, 1); var s = document.waitForSettlement(); s.settled");
    assert_eq!(result, JsValue::Boolean(false));
}

#[test]
fn wait_for_settlement_observes_timer_dom_mutations() {
    let result = eval_full(r#"
        document.body.innerHTML = '<div id="box"></div>';
        setTimeout(function(){ document.getElementById('box').setAttribute('data-x', '1'); }, 0);
        document.waitForSettlement();
        document.getElementById('box').getAttribute('data-x')
    "#);
    assert_eq!(result, JsValue::String("1".to_string()));
}

#[test]
fn wait_for_settlement_reports_dom_state() {
    eval_full("document.body.innerHTML = '<button>Go</button>'");
    let result = eval_full("var s = document.waitForSettlement(); s.interactiveCount");
    assert!(to_number(&result) >= 1.0);
}

#[test]
fn export_nda_includes_network_facts() {
    eval_full("document.body.innerHTML = '<p>Page</p>'; fetch('https://api.example.com/data')");
    let doc = super::super::agent_layer::export_agent_state_nda();
    let facts = doc.readable_facts();
    assert!(
        facts.iter().any(|(s, p, o)| s == "https://api.example.com/data"
            && *p == crate::predicates::NET_METHOD && o == "GET"),
        "expected net method fact, got {:?}", facts
    );
    assert!(
        facts.iter().any(|(s, p, o)| s == "https://api.example.com/data"
            && *p == crate::predicates::NET_STATUS && o == "200"),
        "expected net status fact, got {:?}", facts
    );
}


