//! Native DOM API bridge.
//!
//! Rather than execute JavaScript to manipulate the DOM, the common DOM
//! surface (`getElementById`, `querySelector`, `setAttribute`, `getAttribute`,
//! `textContent`, ...) is modeled directly in Rust. This is faster, has no
//! interpreter overhead, and keeps the agent's model of the page authoritative
//! - in line with the "prefer Rust over JS" philosophy. The JS VM delegates
//! DOM-shaped expressions here before falling back to the expression evaluator.

use crate::dom::DomTree;
use crate::js::vm::JsValue;
use crate::parser::html::NodeType;
use std::collections::HashMap;

/// Try to evaluate `expr` as a native DOM operation.
///
/// Returns `None` if the expression is not a recognized DOM access (so the
/// caller can fall through to the JS expression evaluator). Returns
/// `Some(Ok(..))`/`Some(Err(..))` when it *is* a DOM access.
pub fn eval_dom(tree: &mut DomTree, expr: &str) -> Option<Result<JsValue, String>> {
    let expr = expr.trim();

    // document.createElement('tag')
    if let Some(arg) = expr
        .strip_prefix("document.createElement(")
        .and_then(|s| s.strip_suffix(")"))
    {
        let tag = unquote(arg);
        let id = tree.create_element(&tag);
        return Some(Ok(element_handle(id)));
    }

    // document.createTextNode('text')
    if let Some(arg) = expr
        .strip_prefix("document.createTextNode(")
        .and_then(|s| s.strip_suffix(")"))
    {
        let text = unquote(arg);
        let id = tree.create_text_node(&text);
        return Some(Ok(element_handle(id)));
    }

    // document.querySelectorAll('sel')
    if let Some(arg) = expr
        .strip_prefix("document.querySelectorAll(")
        .and_then(|s| s.strip_suffix(")"))
    {
        let sel = unquote(arg);
        let ids = tree.query_selector_all(&sel);
        let arr: Vec<JsValue> = ids.into_iter().map(element_handle).collect();
        return Some(Ok(JsValue::Array(arr)));
    }

    let (node_id, rest) = parse_target(tree, expr)?;
    let rest = rest.trim();

    // Bare target: `document.getElementById('x')` returns an element handle
    // (or null). The handle carries the resolved node id for later use.
    if rest.is_empty() {
        return Some(Ok(match node_id {
            Some(id) => element_handle(id),
            None => JsValue::Null,
        }));
    }

    let Some(id) = node_id else {
        return Some(Ok(JsValue::Null));
    };

    // Method: setAttribute(name, value)
    if let Some(args) = rest
        .strip_prefix(".setAttribute(")
        .and_then(|s| s.strip_suffix(")"))
    {
        if let Some((attr, val)) = args.split_once(',') {
            let attr = unquote(attr);
            let val = unquote(val);
            if let Some(node) = tree.get_node_mut(id) {
                node.attributes.insert(attr.clone(), val.clone());
                return Some(Ok(JsValue::String(format!(
                    "Set attribute '{}'='{}'",
                    attr, val
                ))));
            }
        }
        return Some(Err("setAttribute: invalid arguments".to_string()));
    }

    // Method: getAttribute(name)
    if let Some(arg) = rest
        .strip_prefix(".getAttribute(")
        .and_then(|s| s.strip_suffix(")"))
    {
        let key = unquote(arg);
        return Some(Ok(tree
            .get_node(id)
            .and_then(|n| n.attributes.get(&key))
            .map(|v| JsValue::String(v.clone()))
            .unwrap_or(JsValue::Null)));
    }

    // Method: removeAttribute(name)
    if let Some(arg) = rest
        .strip_prefix(".removeAttribute(")
        .and_then(|s| s.strip_suffix(")"))
    {
        let key = unquote(arg);
        if let Some(node) = tree.get_node_mut(id) {
            node.attributes.remove(&key);
        }
        return Some(Ok(JsValue::Undefined));
    }

    // Method: hasAttribute(name)
    if let Some(arg) = rest
        .strip_prefix(".hasAttribute(")
        .and_then(|s| s.strip_suffix(")"))
    {
        let key = unquote(arg);
        return Some(Ok(JsValue::Boolean(
            tree.get_node(id)
                .map(|n| n.attributes.contains_key(&key))
                .unwrap_or(false),
        )));
    }

    // Method: appendChild(child_handle) — expects __node_id__ in call
    if let Some(arg) = rest
        .strip_prefix(".appendChild(")
        .and_then(|s| s.strip_suffix(")"))
    {
        if let Some(child_id) = parse_node_id_arg(arg.trim()) {
            tree.append_child(id, child_id);
            return Some(Ok(element_handle(child_id)));
        }
    }

    // Method: removeChild(child_handle)
    if let Some(arg) = rest
        .strip_prefix(".removeChild(")
        .and_then(|s| s.strip_suffix(")"))
    {
        if let Some(child_id) = parse_node_id_arg(arg.trim()) {
            tree.remove_child(id, child_id);
            return Some(Ok(element_handle(child_id)));
        }
    }

    // Method: insertBefore(new, ref)
    if let Some(args) = rest
        .strip_prefix(".insertBefore(")
        .and_then(|s| s.strip_suffix(")"))
    {
        if let Some((new_arg, ref_arg)) = args.split_once(',') {
            if let (Some(new_id), Some(ref_id)) = (
                parse_node_id_arg(new_arg.trim()),
                parse_node_id_arg(ref_arg.trim()),
            ) {
                tree.insert_before(id, new_id, ref_id);
                return Some(Ok(element_handle(new_id)));
            }
        }
    }

    // Method: remove() — self-removal
    if rest == ".remove()" {
        if let Some(parent_id) = tree.get_node(id).and_then(|n| n.parent) {
            tree.remove_child(parent_id, id);
        }
        return Some(Ok(JsValue::Undefined));
    }

    // Method: querySelectorAll('sel') on element
    if let Some(arg) = rest
        .strip_prefix(".querySelectorAll(")
        .and_then(|s| s.strip_suffix(")"))
    {
        let sel = unquote(arg);
        let all = tree.query_selector_all(&sel);
        // Filter to descendants of `id`
        let descendants: Vec<JsValue> = all
            .into_iter()
            .filter(|&nid| is_descendant_of(tree, nid, id))
            .map(element_handle)
            .collect();
        return Some(Ok(JsValue::Array(descendants)));
    }

    // Property: .innerHTML (getter)
    if rest == ".innerHTML" {
        return Some(Ok(JsValue::String(tree.get_inner_html(id))));
    }

    // Property: .children
    if rest == ".children" {
        let children = tree.element_children(id);
        return Some(Ok(JsValue::Array(
            children.into_iter().map(element_handle).collect(),
        )));
    }

    // Property: .childNodes
    if rest == ".childNodes" {
        let children = tree
            .get_node(id)
            .map(|n| n.children.clone())
            .unwrap_or_default();
        return Some(Ok(JsValue::Array(
            children.into_iter().map(element_handle).collect(),
        )));
    }

    // Property: .parentNode / .parentElement
    if rest == ".parentNode" || rest == ".parentElement" {
        return Some(Ok(tree
            .get_node(id)
            .and_then(|n| n.parent)
            .map(element_handle)
            .unwrap_or(JsValue::Null)));
    }

    // Property: .firstChild / .firstElementChild
    if rest == ".firstChild" || rest == ".firstElementChild" {
        return Some(Ok(tree
            .get_node(id)
            .and_then(|n| n.children.first().copied())
            .map(element_handle)
            .unwrap_or(JsValue::Null)));
    }

    // Property: .lastChild / .lastElementChild
    if rest == ".lastChild" || rest == ".lastElementChild" {
        return Some(Ok(tree
            .get_node(id)
            .and_then(|n| n.children.last().copied())
            .map(element_handle)
            .unwrap_or(JsValue::Null)));
    }

    // Property: .nextSibling / .nextElementSibling
    if rest == ".nextSibling" || rest == ".nextElementSibling" {
        return Some(Ok(get_sibling(tree, id, 1)
            .map(element_handle)
            .unwrap_or(JsValue::Null)));
    }

    // Property: .previousSibling / .previousElementSibling
    if rest == ".previousSibling" || rest == ".previousElementSibling" {
        return Some(Ok(get_sibling(tree, id, -1)
            .map(element_handle)
            .unwrap_or(JsValue::Null)));
    }

    // Properties (no parentheses).
    // Property setters: .innerHTML = '...'
    if let Some(val_part) = rest
        .strip_prefix(".innerHTML=")
        .or_else(|| rest.strip_prefix(".innerHTML ="))
    {
        let html = unquote(val_part.trim());
        tree.set_inner_html(id, &html);
        return Some(Ok(JsValue::Undefined));
    }

    // Property setter: .textContent = '...'
    if let Some(val_part) = rest
        .strip_prefix(".textContent=")
        .or_else(|| rest.strip_prefix(".textContent ="))
    {
        let text = unquote(val_part.trim());
        // Clear children and add text node
        if let Some(node) = tree.get_node_mut(id) {
            node.children.clear();
        }
        let text_id = tree.create_text_node(&text);
        tree.append_child(id, text_id);
        return Some(Ok(JsValue::Undefined));
    }

    // classList methods: .classList.add('x'), .classList.remove('x'), .classList.toggle('x'), .classList.contains('x')
    if let Some(class_rest) = rest.strip_prefix(".classList.") {
        if let Some(arg) = class_rest
            .strip_prefix("add(")
            .and_then(|s| s.strip_suffix(")"))
        {
            let cls = unquote(arg);
            if let Some(node) = tree.get_node_mut(id) {
                let current = node.attributes.get("class").cloned().unwrap_or_default();
                if !current.split_whitespace().any(|c| c == cls) {
                    let new_class = if current.is_empty() {
                        cls
                    } else {
                        format!("{} {}", current, cls)
                    };
                    node.attributes.insert("class".to_string(), new_class);
                }
            }
            return Some(Ok(JsValue::Undefined));
        }
        if let Some(arg) = class_rest
            .strip_prefix("remove(")
            .and_then(|s| s.strip_suffix(")"))
        {
            let cls = unquote(arg);
            if let Some(node) = tree.get_node_mut(id) {
                let current = node.attributes.get("class").cloned().unwrap_or_default();
                let new_class: String = current
                    .split_whitespace()
                    .filter(|&c| c != cls)
                    .collect::<Vec<_>>()
                    .join(" ");
                node.attributes.insert("class".to_string(), new_class);
            }
            return Some(Ok(JsValue::Undefined));
        }
        if let Some(arg) = class_rest
            .strip_prefix("toggle(")
            .and_then(|s| s.strip_suffix(")"))
        {
            let cls = unquote(arg);
            if let Some(node) = tree.get_node_mut(id) {
                let current = node.attributes.get("class").cloned().unwrap_or_default();
                let has_it = current.split_whitespace().any(|c| c == cls);
                let new_class = if has_it {
                    current
                        .split_whitespace()
                        .filter(|&c| c != cls)
                        .collect::<Vec<_>>()
                        .join(" ")
                } else if current.is_empty() {
                    cls.clone()
                } else {
                    format!("{} {}", current, cls)
                };
                node.attributes.insert("class".to_string(), new_class);
                return Some(Ok(JsValue::Boolean(!has_it)));
            }
            return Some(Ok(JsValue::Boolean(false)));
        }
        if let Some(arg) = class_rest
            .strip_prefix("contains(")
            .and_then(|s| s.strip_suffix(")"))
        {
            let cls = unquote(arg);
            let has_it = tree
                .get_node(id)
                .and_then(|n| n.attributes.get("class"))
                .map(|c| c.split_whitespace().any(|x| x == cls))
                .unwrap_or(false);
            return Some(Ok(JsValue::Boolean(has_it)));
        }
    }

    // replaceChild(new, old)
    if let Some(args) = rest
        .strip_prefix(".replaceChild(")
        .and_then(|s| s.strip_suffix(")"))
    {
        if let Some((new_arg, old_arg)) = args.split_once(',') {
            if let (Some(new_id), Some(old_id)) = (
                parse_node_id_arg(new_arg.trim()),
                parse_node_id_arg(old_arg.trim()),
            ) {
                tree.replace_child(id, new_id, old_id);
                return Some(Ok(element_handle(old_id)));
            }
        }
    }

    match rest {
        ".textContent" | ".innerText" => Some(Ok(JsValue::String(text_content(tree, id)))),
        ".value" => Some(Ok(read_attr(tree, id, "value"))),
        ".id" => Some(Ok(read_attr(tree, id, "id"))),
        ".className" => Some(Ok(read_attr(tree, id, "class"))),
        ".tagName" | ".nodeName" => Some(Ok(tree
            .get_node(id)
            .map(|n| JsValue::String(n.tag_name.to_uppercase()))
            .unwrap_or(JsValue::Null))),
        ".nodeType" => Some(Ok(JsValue::Number(1.0))), // ELEMENT_NODE
        ".outerHTML" => {
            let mut html = String::new();
            tree.serialize_node(id, &mut html);
            Some(Ok(JsValue::String(html)))
        }
        _ => None,
    }
}

/// Check if `child_id` is a descendant of `ancestor_id`.
fn is_descendant_of(tree: &DomTree, child_id: usize, ancestor_id: usize) -> bool {
    let mut current = tree.get_node(child_id).and_then(|n| n.parent);
    while let Some(pid) = current {
        if pid == ancestor_id {
            return true;
        }
        current = tree.get_node(pid).and_then(|n| n.parent);
    }
    false
}

/// Get next or previous sibling.
fn get_sibling(tree: &DomTree, node_id: usize, offset: i32) -> Option<usize> {
    let parent_id = tree.get_node(node_id)?.parent?;
    let parent = tree.get_node(parent_id)?;
    let pos = parent.children.iter().position(|&c| c == node_id)? as i32;
    let target = pos + offset;
    if target >= 0 && (target as usize) < parent.children.len() {
        Some(parent.children[target as usize])
    } else {
        None
    }
}

/// Try to parse a node id from an argument that could be a number or a handle reference.
fn parse_node_id_arg(s: &str) -> Option<usize> {
    // Direct numeric id
    if let Ok(id) = s.parse::<usize>() {
        return Some(id);
    }
    // Could be a variable reference — we can't resolve that here
    None
}

/// Resolve the leading `document.getElementById(..)` / `document.querySelector(..)`
/// call in `expr`, returning the matched node id (if any) and the remainder of
/// the expression after the call (e.g. `.textContent`).
fn parse_target<'a>(tree: &DomTree, expr: &'a str) -> Option<(Option<usize>, &'a str)> {
    for method in ["getElementById", "querySelector"] {
        let prefix = format!("document.{}(", method);
        if let Some(rel) = expr.strip_prefix(&prefix) {
            let close = rel.find(')')?;
            let arg = unquote(&rel[..close]);
            let node_id = if method == "getElementById" {
                find_by_id(tree, &arg)
            } else {
                resolve_selector(tree, &arg)
            };
            return Some((node_id, &rel[close + 1..]));
        }
    }
    None
}

fn find_by_id(tree: &DomTree, id: &str) -> Option<usize> {
    tree.nodes
        .iter()
        .find(|n| {
            n.node_type == NodeType::Element
                && n.attributes.get("id").map(|s| s.as_str()) == Some(id)
        })
        .map(|n| n.id)
}

/// Minimal selector support: `#id`, `.class`, or a bare tag name.
fn resolve_selector(tree: &DomTree, sel: &str) -> Option<usize> {
    let sel = sel.trim();
    if let Some(id) = sel.strip_prefix('#') {
        return find_by_id(tree, id);
    }
    if let Some(class) = sel.strip_prefix('.') {
        return tree
            .nodes
            .iter()
            .find(|n| {
                n.node_type == NodeType::Element
                    && n.attributes
                        .get("class")
                        .map(|c| c.split_whitespace().any(|x| x == class))
                        .unwrap_or(false)
            })
            .map(|n| n.id);
    }
    tree.nodes
        .iter()
        .find(|n| n.node_type == NodeType::Element && n.tag_name == sel)
        .map(|n| n.id)
}

/// Concatenated text of a node and all its descendants (like DOM textContent).
fn text_content(tree: &DomTree, id: usize) -> String {
    let mut out = String::new();
    collect_text(tree, id, &mut out);
    out
}

fn collect_text(tree: &DomTree, id: usize, out: &mut String) {
    if let Some(node) = tree.get_node(id) {
        if node.node_type == NodeType::Text {
            out.push_str(&node.text_content);
        }
        for &child in &node.children {
            collect_text(tree, child, out);
        }
    }
}

fn read_attr(tree: &DomTree, id: usize, key: &str) -> JsValue {
    tree.get_node(id)
        .and_then(|n| n.attributes.get(key))
        .map(|v| JsValue::String(v.clone()))
        .unwrap_or(JsValue::String(String::new()))
}

fn element_handle(id: usize) -> JsValue {
    let mut obj = HashMap::new();
    obj.insert("__node_id__".to_string(), JsValue::Number(id as f64));
    JsValue::Object(obj)
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim_matches('\'').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::HtmlParser;

    fn tree(html: &str) -> DomTree {
        DomTree::new(HtmlParser::parse_html5(html))
    }

    #[test]
    fn reads_text_content_by_id() {
        let mut t = tree("<div id=\"greeting\">Hello<b>world</b></div>");
        let res = eval_dom(&mut t, "document.getElementById('greeting').textContent").unwrap();
        assert_eq!(res.unwrap(), JsValue::String("Helloworld".to_string()));
    }

    #[test]
    fn set_and_get_attribute() {
        let mut t = tree("<input id=\"n\">");
        eval_dom(
            &mut t,
            "document.getElementById('n').setAttribute('value','42')",
        )
        .unwrap()
        .unwrap();
        let got = eval_dom(&mut t, "document.getElementById('n').getAttribute('value')")
            .unwrap()
            .unwrap();
        assert_eq!(got, JsValue::String("42".to_string()));
    }

    #[test]
    fn query_selector_by_class_and_tag() {
        let mut t = tree("<p class=\"lead\">Intro</p>");
        let by_class = eval_dom(&mut t, "document.querySelector('.lead').textContent")
            .unwrap()
            .unwrap();
        assert_eq!(by_class, JsValue::String("Intro".to_string()));
        let by_tag = eval_dom(&mut t, "document.querySelector('p').tagName")
            .unwrap()
            .unwrap();
        assert_eq!(by_tag, JsValue::String("P".to_string()));
    }

    #[test]
    fn missing_element_is_null() {
        let mut t = tree("<div></div>");
        let res = eval_dom(&mut t, "document.getElementById('nope').textContent")
            .unwrap()
            .unwrap();
        assert_eq!(res, JsValue::Null);
    }

    #[test]
    fn non_dom_expression_returns_none() {
        let mut t = tree("<div></div>");
        assert!(eval_dom(&mut t, "1 + 2").is_none());
    }

    #[test]
    fn create_element_returns_handle() {
        let mut t = tree("<div></div>");
        let res = eval_dom(&mut t, "document.createElement('span')")
            .unwrap()
            .unwrap();
        match res {
            JsValue::Object(ref obj) => {
                assert!(obj.contains_key("__node_id__"));
            }
            _ => panic!("expected object handle"),
        }
    }

    #[test]
    fn classlist_add_remove_toggle_contains() {
        let mut t = tree("<div id=\"el\" class=\"foo\"></div>");
        // add
        eval_dom(&mut t, "document.getElementById('el').classList.add('bar')")
            .unwrap()
            .unwrap();
        // contains
        let has = eval_dom(
            &mut t,
            "document.getElementById('el').classList.contains('bar')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(has, JsValue::Boolean(true));
        // toggle (remove)
        let toggled = eval_dom(
            &mut t,
            "document.getElementById('el').classList.toggle('bar')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(toggled, JsValue::Boolean(false));
        // contains after toggle-off
        let has2 = eval_dom(
            &mut t,
            "document.getElementById('el').classList.contains('bar')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(has2, JsValue::Boolean(false));
        // remove foo
        eval_dom(
            &mut t,
            "document.getElementById('el').classList.remove('foo')",
        )
        .unwrap()
        .unwrap();
        let has3 = eval_dom(
            &mut t,
            "document.getElementById('el').classList.contains('foo')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(has3, JsValue::Boolean(false));
    }

    #[test]
    fn has_and_remove_attribute() {
        let mut t = tree("<div id=\"x\" data-val=\"123\"></div>");
        let has = eval_dom(
            &mut t,
            "document.getElementById('x').hasAttribute('data-val')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(has, JsValue::Boolean(true));
        eval_dom(
            &mut t,
            "document.getElementById('x').removeAttribute('data-val')",
        )
        .unwrap()
        .unwrap();
        let has2 = eval_dom(
            &mut t,
            "document.getElementById('x').hasAttribute('data-val')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(has2, JsValue::Boolean(false));
    }

    #[test]
    fn innerhtml_getter_and_setter() {
        let mut t = tree("<div id=\"box\"><span>old</span></div>");
        let html = eval_dom(&mut t, "document.getElementById('box').innerHTML")
            .unwrap()
            .unwrap();
        assert!(matches!(html, JsValue::String(ref s) if s.contains("old")));
        eval_dom(
            &mut t,
            "document.getElementById('box').innerHTML='<b>new</b>'",
        )
        .unwrap()
        .unwrap();
        let html2 = eval_dom(&mut t, "document.getElementById('box').innerHTML")
            .unwrap()
            .unwrap();
        assert!(matches!(html2, JsValue::String(ref s) if s.contains("new")));
    }

    #[test]
    fn parent_and_children_properties() {
        let mut t = tree("<div id=\"parent\"><p id=\"child\">text</p></div>");
        let parent = eval_dom(&mut t, "document.getElementById('child').parentNode")
            .unwrap()
            .unwrap();
        assert!(matches!(parent, JsValue::Object(_)));
        let children = eval_dom(&mut t, "document.getElementById('parent').children")
            .unwrap()
            .unwrap();
        assert!(matches!(children, JsValue::Array(ref arr) if !arr.is_empty()));
    }

    #[test]
    fn tagname_and_classname_properties() {
        let mut t = tree("<div id=\"el\" class=\"active\"></div>");
        let tag = eval_dom(&mut t, "document.getElementById('el').tagName")
            .unwrap()
            .unwrap();
        assert_eq!(tag, JsValue::String("DIV".to_string()));
        let cls = eval_dom(&mut t, "document.getElementById('el').className")
            .unwrap()
            .unwrap();
        assert_eq!(cls, JsValue::String("active".to_string()));
    }
}
