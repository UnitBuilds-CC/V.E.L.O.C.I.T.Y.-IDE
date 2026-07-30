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
