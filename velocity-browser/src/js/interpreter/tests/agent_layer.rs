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

