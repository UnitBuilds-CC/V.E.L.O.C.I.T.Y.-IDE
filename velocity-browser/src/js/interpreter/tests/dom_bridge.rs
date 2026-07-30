//! Tests for the in-memory DOM bridge: document methods, element manipulation,
//! selectors, events, and tree navigation.

use super::*;

fn reset() {
    super::super::dom_bridge::reset_dom();
}

// ── Document Methods ─────────────────────────────────────────────────────────

#[test]
fn document_create_element() {
    reset();
    let result = eval_full("var el = document.createElement('div'); el.tagName");
    assert_eq!(result, JsValue::String("DIV".to_string()));
}

#[test]
fn document_create_text_node() {
    reset();
    let result = eval_full("var t = document.createTextNode('hello'); t.textContent");
    assert_eq!(result, JsValue::String("hello".to_string()));
}

#[test]
fn document_get_element_by_id() {
    reset();
    let result = eval_full("var el = document.createElement('p'); el.setAttribute('id', 'main'); document.body.appendChild(el); var found = document.getElementById('main'); found.tagName");
    assert_eq!(result, JsValue::String("P".to_string()));
}

#[test]
fn document_get_element_by_id_missing() {
    reset();
    let result = eval_full("document.getElementById('nope')");
    assert_eq!(result, JsValue::Null);
}

#[test]
fn document_query_selector_by_tag() {
    reset();
    let result = eval_full("var el = document.createElement('span'); document.body.appendChild(el); var found = document.querySelector('span'); found.tagName");
    assert_eq!(result, JsValue::String("SPAN".to_string()));
}

#[test]
fn document_query_selector_by_class() {
    reset();
    let result = eval_full("var el = document.createElement('div'); el.setAttribute('class', 'active'); document.body.appendChild(el); var found = document.querySelector('.active'); found !== null");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn document_query_selector_all() {
    reset();
    let result = eval_full("var a = document.createElement('li'); var b = document.createElement('li'); document.body.appendChild(a); document.body.appendChild(b); document.querySelectorAll('li').length");
    assert_eq!(result, JsValue::Number(2.0));
}

// ── Element Methods ──────────────────────────────────────────────────────────

#[test]
fn element_set_get_attribute() {
    reset();
    let result = eval_full("var el = document.createElement('input'); el.setAttribute('type', 'text'); el.getAttribute('type')");
    assert_eq!(result, JsValue::String("text".to_string()));
}

#[test]
fn element_has_attribute() {
    reset();
    let result = eval_full("var el = document.createElement('div'); el.setAttribute('data-x', '1'); el.hasAttribute('data-x')");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn element_remove_attribute() {
    reset();
    let result = eval_full("var el = document.createElement('div'); el.setAttribute('id', 'x'); el.removeAttribute('id'); el.hasAttribute('id')");
    assert_eq!(result, JsValue::Boolean(false));
}

#[test]
fn element_append_child_and_children() {
    reset();
    let result = eval_full("var parent = document.createElement('ul'); var child = document.createElement('li'); parent.appendChild(child); parent.children.length");
    assert_eq!(result, JsValue::Number(1.0));
}

#[test]
fn element_remove_child() {
    reset();
    let result = eval_full("var p = document.createElement('div'); var c = document.createElement('span'); p.appendChild(c); p.removeChild(c); p.children.length");
    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn element_text_content_set_get() {
    reset();
    let result = eval_full("var el = document.createElement('p'); el.textContent = 'Hello World'; el.textContent");
    assert_eq!(result, JsValue::String("Hello World".to_string()));
}

#[test]
fn element_remove() {
    reset();
    let result = eval_full("var p = document.createElement('div'); var c = document.createElement('span'); p.appendChild(c); c.remove(); p.children.length");
    assert_eq!(result, JsValue::Number(0.0));
}

// ── Tree Navigation ──────────────────────────────────────────────────────────

#[test]
fn element_parent_node() {
    reset();
    let result = eval_full("var p = document.createElement('div'); var c = document.createElement('span'); p.appendChild(c); c.parentNode.tagName");
    assert_eq!(result, JsValue::String("DIV".to_string()));
}

#[test]
fn element_first_last_child() {
    reset();
    let result = eval_full("var p = document.createElement('ul'); var a = document.createElement('li'); var b = document.createElement('li'); p.appendChild(a); p.appendChild(b); p.firstChild.tagName + ',' + p.lastChild.tagName");
    assert_eq!(result, JsValue::String("LI,LI".to_string()));
}

// ── Selectors on Elements ────────────────────────────────────────────────────

#[test]
fn element_query_selector() {
    reset();
    let result = eval_full("var div = document.createElement('div'); var span = document.createElement('span'); div.appendChild(span); div.querySelector('span').tagName");
    assert_eq!(result, JsValue::String("SPAN".to_string()));
}

#[test]
fn element_matches() {
    reset();
    let result = eval_full("var el = document.createElement('div'); el.setAttribute('class', 'foo'); el.matches('.foo')");
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn element_closest() {
    reset();
    let result = eval_full("var div = document.createElement('div'); div.setAttribute('id', 'wrap'); var p = document.createElement('p'); div.appendChild(p); p.closest('#wrap').tagName");
    assert_eq!(result, JsValue::String("DIV".to_string()));
}

// ── Events ───────────────────────────────────────────────────────────────────

#[test]
fn element_add_dispatch_event() {
    reset();
    let result = eval_full("
        var el = document.createElement('button');
        var clicked = false;
        el.addEventListener('click', function() { clicked = true; });
        el.dispatchEvent(new Event('click'));
        clicked
    ");
    assert_eq!(result, JsValue::Boolean(true));
}

// ── Clone ────────────────────────────────────────────────────────────────────

#[test]
fn element_clone_node_shallow() {
    reset();
    let result = eval_full("var el = document.createElement('div'); el.setAttribute('id', 'orig'); var clone = el.cloneNode(false); clone.getAttribute('id')");
    assert_eq!(result, JsValue::String("orig".to_string()));
}

#[test]
fn element_clone_node_deep() {
    reset();
    let result = eval_full("var el = document.createElement('ul'); var li = document.createElement('li'); el.appendChild(li); var clone = el.cloneNode(true); clone.children.length");
    assert_eq!(result, JsValue::Number(1.0));
}

// ── Document Properties ──────────────────────────────────────────────────────

#[test]
fn document_body_exists() {
    reset();
    let result = eval_full("document.body.tagName");
    assert_eq!(result, JsValue::String("BODY".to_string()));
}

#[test]
fn document_head_exists() {
    reset();
    let result = eval_full("document.head.tagName");
    assert_eq!(result, JsValue::String("HEAD".to_string()));
}

#[test]
fn document_document_element() {
    reset();
    let result = eval_full("document.documentElement.tagName");
    assert_eq!(result, JsValue::String("HTML".to_string()));
}

// ── classList (DOMTokenList) ─────────────────────────────────────────────────

#[test]
fn class_list_add_contains() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.classList.add('foo', 'bar');
        el.classList.contains('foo')
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn class_list_remove() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.classList.add('a', 'b');
        el.classList.remove('a');
        el.classList.contains('a')
    "#);
    assert_eq!(result, JsValue::Boolean(false));
}

#[test]
fn class_list_toggle() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.classList.toggle('active');
        var first = el.classList.contains('active');
        el.classList.toggle('active');
        var second = el.classList.contains('active');
        [first, second].join(',')
    "#);
    assert_eq!(result, JsValue::String("true,false".to_string()));
}

#[test]
fn class_list_length() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.classList.add('x', 'y', 'z');
        el.classList.length
    "#);
    assert_eq!(result, JsValue::Number(3.0));
}

#[test]
fn class_list_replace() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.classList.add('old');
        el.classList.replace('old', 'new');
        el.classList.contains('new')
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn class_list_item() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.classList.add('first', 'second');
        el.classList.item(1)
    "#);
    assert_eq!(result, JsValue::String("second".to_string()));
}

// ── dataset (DOMStringMap) ───────────────────────────────────────────────────

#[test]
fn dataset_set_get() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.dataset.userId = '42';
        el.dataset.userId
    "#);
    assert_eq!(result, JsValue::String("42".to_string()));
}

#[test]
fn dataset_camel_to_kebab() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.dataset.myAttr = 'val';
        el.getAttribute('data-my-attr')
    "#);
    assert_eq!(result, JsValue::String("val".to_string()));
}

// ── innerHTML parsing ────────────────────────────────────────────────────────

#[test]
fn inner_html_parses_children() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.innerHTML = '<span>hello</span><b>world</b>';
        el.children.length
    "#);
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn inner_html_child_tag() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.innerHTML = '<p>text</p>';
        el.children[0].tagName
    "#);
    assert_eq!(result, JsValue::String("P".to_string()));
}

#[test]
fn inner_html_nested() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.innerHTML = '<ul><li>one</li><li>two</li></ul>';
        el.children[0].children.length
    "#);
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn inner_html_query_selector() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.innerHTML = '<span class="x">found</span>';
        el.querySelector('.x').textContent
    "#);
    assert_eq!(result, JsValue::String("found".to_string()));
}

// ── Element Interaction ──────────────────────────────────────────────────────

#[test]
fn element_get_bounding_client_rect() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        var rect = el.getBoundingClientRect();
        rect.__type__
    "#);
    assert_eq!(result, JsValue::String("DOMRect".to_string()));
}

#[test]
fn element_scroll_into_view() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.scrollIntoView();
        'ok'
    "#);
    assert_eq!(result, JsValue::String("ok".to_string()));
}

#[test]
fn element_focus_blur_click() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('input');
        el.focus();
        el.blur();
        el.click();
        'done'
    "#);
    assert_eq!(result, JsValue::String("done".to_string()));
}

#[test]
fn element_insert_adjacent_html() {
    reset();
    let result = eval_full(r#"
        var parent = document.createElement('div');
        var child = document.createElement('p');
        parent.appendChild(child);
        child.insertAdjacentHTML('afterend', '<span>hi</span>');
        parent.children.length
    "#);
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn element_before_after() {
    reset();
    let result = eval_full(r#"
        var parent = document.createElement('ul');
        var li = document.createElement('li');
        parent.appendChild(li);
        var li2 = document.createElement('li');
        li.before(li2);
        parent.children.length
    "#);
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn element_append_prepend() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        var a = document.createElement('span');
        var b = document.createElement('p');
        el.append(a);
        el.prepend(b);
        el.children.length
    "#);
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn element_toggle_attribute() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('input');
        el.toggleAttribute('disabled');
        el.hasAttribute('disabled')
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn element_get_attribute_names() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.setAttribute('id', 'x');
        el.setAttribute('class', 'y');
        el.getAttributeNames().length
    "#);
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn element_animate() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        var anim = el.animate([], {});
        anim.playState
    "#);
    assert_eq!(result, JsValue::String("running".to_string()));
}

// ── Shadow DOM ───────────────────────────────────────────────────────────────

#[test]
fn element_attach_shadow() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        var shadow = el.attachShadow({mode: 'open'});
        shadow.__type__
    "#);
    assert_eq!(result, JsValue::String("Element".to_string()));
}

#[test]
fn element_shadow_root_property() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.attachShadow({mode: 'open'});
        el.shadowRoot !== null
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}

// ── Document Cookie ──────────────────────────────────────────────────────────

#[test]
fn document_cookie_set_get() {
    reset();
    let result = eval_full(r#"
        document.cookie = 'name=value';
        document.cookie
    "#);
    assert_eq!(result, JsValue::String("name=value".to_string()));
}

// ── Window Methods ───────────────────────────────────────────────────────────

#[test]
fn window_post_message() {
    let result = eval_full(r#"
        window.postMessage('hello', '*');
        'ok'
    "#);
    assert_eq!(result, JsValue::String("ok".to_string()));
}

#[test]
fn window_open_close() {
    let result = eval_full(r#"
        var w = window.open('about:blank');
        w.closed
    "#);
    assert_eq!(result, JsValue::Boolean(false));
}

#[test]
fn window_get_selection() {
    let result = eval_full(r#"
        var sel = window.getSelection();
        sel.__type__
    "#);
    assert_eq!(result, JsValue::String("Selection".to_string()));
}

// ── Custom Elements ──────────────────────────────────────────────────────────

#[test]
fn custom_elements_define() {
    let result = eval_full(r#"
        customElements.define('my-el', function() {});
        'ok'
    "#);
    assert_eq!(result, JsValue::String("ok".to_string()));
}

#[test]
fn custom_elements_when_defined() {
    let result = eval_full(r#"
        var p = customElements.whenDefined('my-el');
        p.__type__
    "#);
    assert_eq!(result, JsValue::String("Promise".to_string()));
}

// ── Element Layout Properties ────────────────────────────────────────────────

#[test]
fn element_offset_properties() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.offsetWidth
    "#);
    assert_eq!(result, JsValue::Number(0.0));
}

#[test]
fn element_is_connected() {
    reset();
    let result = eval_full(r#"
        var el = document.createElement('div');
        el.isConnected
    "#);
    assert_eq!(result, JsValue::Boolean(true));
}
