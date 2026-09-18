use crate::dom::DomTree;
use crate::nda::{NdaDocument, NdaTriple};
use crate::parser::html::{DomNode, NodeType};
use crate::predicates::{
    AOM_ACTIONABILITY, AOM_CHECKED, AOM_EXPANDED, AOM_FOCUSED, AOM_NAME, AOM_ROLE, AOM_VALUE,
};

/// Recursively collect the visible text content of a node and its descendants,
/// mirroring DOM `textContent` semantics. Used as a fallback accessible name
/// when no explicit attribute (`aria-label`, `placeholder`, etc.) is present.
fn collect_inner_text(tree: &DomTree, node_id: usize) -> String {
    let mut buf = String::new();
    inner_text_walk(tree, node_id, &mut buf);
    let trimmed = buf.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed
}

fn inner_text_walk(tree: &DomTree, id: usize, out: &mut String) {
    if let Some(node) = tree.get_node(id) {
        if node.node_type == NodeType::Text {
            out.push_str(&node.text_content);
        }
        for &child in &node.children {
            inner_text_walk(tree, child, out);
        }
    }
}

/// Tags whose accessible name comes from their surroundings rather than from
/// their own contents, and which therefore honour a bound `<label>`.
fn is_form_control(tag: &str) -> bool {
    matches!(tag, "input" | "select" | "textarea" | "button")
}

/// The text of every node referenced by `aria-labelledby`, space-joined in
/// author order (the idref-list order is significant per ARIA).
fn labelledby_text(tree: &DomTree, node: &DomNode) -> Option<String> {
    let refs = node.attributes.get("aria-labelledby")?;
    let parts: Vec<String> = refs
        .split_whitespace()
        .filter_map(|id| {
            tree.nodes
                .iter()
                .find(|n| n.attributes.get("id").map(|s| s.as_str()) == Some(id))
                .map(|n| collect_inner_text(tree, n.id))
        })
        .filter(|s| !s.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// The `<label>` bound to a control: either `<label for="the-id">` anywhere in
/// the document, or a `<label>` the control sits inside.
///
/// This was missing entirely, so the AOM reported `name="custname"` for
/// `<label>Customer name:</label><input name="custname">`. Every tool that
/// promises resolution "by accessible name" - the whole `browser_native_*label`
/// family and the `role` + `name` arguments on the rest - therefore could not
/// address a field by the words printed next to it, which is the only thing an
/// agent reading the screen has.
fn associated_label_text(tree: &DomTree, node: &DomNode) -> Option<String> {
    if let Some(id) = node.attributes.get("id") {
        for candidate in &tree.nodes {
            if candidate.node_type != NodeType::Element || candidate.tag_name != "label" {
                continue;
            }
            if candidate.attributes.get("for").map(|s| s.as_str()) == Some(id.as_str()) {
                let text = collect_inner_text(tree, candidate.id);
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }
    let mut current = node.parent;
    while let Some(pid) = current {
        let Some(parent) = tree.get_node(pid) else {
            break;
        };
        if parent.tag_name == "label" {
            // A wrapping label's own text includes the control's inner text;
            // for input/select/textarea that is empty, so the caption comes
            // through on its own.
            let text = collect_inner_text(tree, pid);
            if !text.is_empty() {
                return Some(text);
            }
        }
        current = parent.parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::{DomNode, NodeType};
    use std::collections::HashMap;

    fn make_node(id: usize, tag: &str, attrs: &[(&str, &str)]) -> DomNode {
        let mut attributes = HashMap::new();
        for (k, v) in attrs {
            attributes.insert(k.to_string(), v.to_string());
        }
        DomNode {
            id,
            node_type: NodeType::Element,
            tag_name: tag.to_string(),
            attributes,
            text_content: String::new(),
            children: Vec::new(),
            parent: None,
        }
    }

    #[test]
    fn button_role_and_actionability() {
        let tree = DomTree::new(vec![make_node(0, "button", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, "button");
        assert_eq!(nodes[0].actionability_score, 100);
    }

    #[test]
    fn link_role_and_actionability() {
        let tree = DomTree::new(vec![make_node(0, "a", &[("href", "/page")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, "link");
        assert_eq!(nodes[0].actionability_score, 100);
    }

    #[test]
    fn input_text_role() {
        let tree = DomTree::new(vec![make_node(0, "input", &[("type", "text")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "textbox");
        assert_eq!(nodes[0].actionability_score, 90);
    }

    #[test]
    fn input_checkbox_role() {
        let tree = DomTree::new(vec![make_node(0, "input", &[("type", "checkbox")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "checkbox");
        assert_eq!(nodes[0].actionability_score, 90);
    }

    #[test]
    fn heading_role() {
        let tree = DomTree::new(vec![make_node(0, "h1", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "heading");
        assert_eq!(nodes[0].actionability_score, 40);
    }

    #[test]
    fn generic_div_without_label_skipped() {
        let tree = DomTree::new(vec![make_node(0, "div", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert!(nodes.is_empty());
    }

    #[test]
    fn generic_div_with_aria_label_included() {
        let tree = DomTree::new(vec![make_node(0, "div", &[("aria-label", "sidebar")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "sidebar");
    }

    #[test]
    fn aria_label_overrides_visible_text() {
        let tree = DomTree::new(vec![make_node(
            0,
            "button",
            &[("aria-label", "Close dialog")],
        )]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].name, "Close dialog");
    }

    #[test]
    fn to_nda_triples_includes_role_and_actionability() {
        let nodes = vec![AgenticAomNode {
            id: "n0".into(),
            role: "button".into(),
            name: "Submit".into(),
            value: String::new(),
            actionability_score: 100,
            is_focused: false,
            is_expanded: false,
            is_checked: false,
        }];
        let triples = AgenticAomTree::to_nda_triples(&nodes);
        // role + name + actionability = 3
        assert_eq!(triples.len(), 3);
        let role_triple = triples.iter().find(|t| t.predicate_id == AOM_ROLE).unwrap();
        assert_eq!(role_triple.object_hash, crate::nda::hash_str("button"));
    }

    #[test]
    fn to_nda_triples_includes_focused() {
        let nodes = vec![AgenticAomNode {
            id: "n0".into(),
            role: "textbox".into(),
            name: "email".into(),
            value: String::new(),
            actionability_score: 90,
            is_focused: true,
            is_expanded: false,
            is_checked: false,
        }];
        let triples = AgenticAomTree::to_nda_triples(&nodes);
        let focused = triples.iter().find(|t| t.predicate_id == AOM_FOCUSED);
        assert!(focused.is_some());
    }

    #[test]
    fn to_nda_triples_includes_expanded() {
        let nodes = vec![AgenticAomNode {
            id: "n0".into(),
            role: "combobox".into(),
            name: "select".into(),
            value: String::new(),
            actionability_score: 90,
            is_focused: false,
            is_expanded: true,
            is_checked: false,
        }];
        let triples = AgenticAomTree::to_nda_triples(&nodes);
        let expanded = triples.iter().find(|t| t.predicate_id == AOM_EXPANDED);
        assert!(expanded.is_some());
    }

    #[test]
    fn to_nda_document_nonempty() {
        let nodes = vec![AgenticAomNode {
            id: "n0".into(),
            role: "button".into(),
            name: "OK".into(),
            value: "val".into(),
            actionability_score: 100,
            is_focused: false,
            is_expanded: false,
            is_checked: false,
        }];
        let doc = AgenticAomTree::to_nda_document(&nodes);
        assert!(!doc.facts.is_empty());
    }

    #[test]
    fn input_submit_is_button_role() {
        let tree = DomTree::new(vec![make_node(0, "input", &[("type", "submit")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "button");
    }

    #[test]
    fn select_is_combobox() {
        let tree = DomTree::new(vec![make_node(0, "select", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "combobox");
    }

    #[test]
    fn value_attribute_captured() {
        let tree = DomTree::new(vec![make_node(
            0,
            "input",
            &[("type", "text"), ("value", "hello")],
        )]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].value, "hello");
    }

    #[test]
    fn textarea_is_textbox() {
        let tree = DomTree::new(vec![make_node(0, "textarea", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "textbox");
        assert_eq!(nodes[0].actionability_score, 90);
    }

    #[test]
    fn form_role_and_actionability() {
        let tree = DomTree::new(vec![make_node(0, "form", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "form");
        assert_eq!(nodes[0].actionability_score, 75);
    }

    #[test]
    fn nav_is_navigation() {
        let tree = DomTree::new(vec![make_node(0, "nav", &[])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "navigation");
        assert_eq!(nodes[0].actionability_score, 40);
    }

    #[test]
    fn explicit_role_attribute_overrides() {
        let tree = DomTree::new(vec![make_node(0, "div", &[("role", "tab")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "tab");
    }

    #[test]
    fn autofocus_sets_focused() {
        let tree = DomTree::new(vec![make_node(
            0,
            "input",
            &[("type", "text"), ("autofocus", "")],
        )]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert!(nodes[0].is_focused);
    }

    #[test]
    fn aria_expanded_true() {
        let tree = DomTree::new(vec![make_node(0, "button", &[("aria-expanded", "true")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert!(nodes[0].is_expanded);
    }

    #[test]
    fn aria_expanded_false() {
        let tree = DomTree::new(vec![make_node(0, "button", &[("aria-expanded", "false")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert!(!nodes[0].is_expanded);
    }

    #[test]
    fn empty_tree_no_aom_nodes() {
        let tree = DomTree::new(vec![]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert!(nodes.is_empty());
    }

    #[test]
    fn input_radio_is_radio_role() {
        let tree = DomTree::new(vec![make_node(0, "input", &[("type", "radio")])]);
        let nodes = AgenticAomTree::build_aom_nodes(&tree);
        assert_eq!(nodes[0].role, "radio");
    }

    #[test]
    fn to_nda_triples_with_value() {
        let nodes = vec![AgenticAomNode {
            id: "n0".into(),
            role: "textbox".into(),
            name: "email".into(),
            value: "user@test.com".into(),
            actionability_score: 90,
            is_focused: false,
            is_expanded: false,
            is_checked: false,
        }];
        let triples = AgenticAomTree::to_nda_triples(&nodes);
        let val = triples.iter().find(|t| t.predicate_id == AOM_VALUE);
        assert!(val.is_some(), "Should emit value triple");
    }

    #[test]
    fn to_nda_triples_no_name_omits_name_triple() {
        let nodes = vec![AgenticAomNode {
            id: "n0".into(),
            role: "generic".into(),
            name: String::new(),
            value: String::new(),
            actionability_score: 10,
            is_focused: false,
            is_expanded: false,
            is_checked: false,
        }];
        let triples = AgenticAomTree::to_nda_triples(&nodes);
        let name_triple = triples.iter().find(|t| t.predicate_id == AOM_NAME);
        assert!(
            name_triple.is_none(),
            "Empty name should not emit AOM_NAME triple"
        );
    }

    #[test]
    fn multiple_nodes_in_tree() {
        let mut nodes_vec = vec![];
        nodes_vec.push(make_node(0, "button", &[]));
        nodes_vec.push(make_node(1, "input", &[("type", "text")]));
        nodes_vec.push(make_node(2, "div", &[]));
        let tree = DomTree::new(nodes_vec);
        let aom = AgenticAomTree::build_aom_nodes(&tree);
        // button + input, div without label is skipped
        assert_eq!(aom.len(), 2);
    }

    // -- Accessible-name computation (bug #45) --------------------------------
    // Parsed from real markup, so the parent/child wiring the label lookup
    // depends on is the same one production uses.

    use crate::parser::html::HtmlParser;

    fn aom_from(html: &str) -> Vec<AgenticAomNode> {
        AgenticAomTree::build_aom_nodes(&DomTree::new(HtmlParser::parse(html)))
    }

    /// The first node with `role`, dumping the whole set if there is none.
    fn named<'a>(nodes: &'a [AgenticAomNode], role: &str) -> &'a AgenticAomNode {
        nodes
            .iter()
            .find(|n| n.role == role)
            .unwrap_or_else(|| panic!("no node with role {role} in {nodes:?}"))
    }

    #[test]
    fn label_for_becomes_the_accessible_name() {
        let nodes = aom_from(
            r#"<label for="cust">Customer name:</label><input id="cust" name="custname" type="text">"#,
        );
        assert_eq!(named(&nodes, "textbox").name, "Customer name:");
    }

    #[test]
    fn wrapping_label_names_the_control_inside_it() {
        let nodes = aom_from(r#"<label>E-mail<input type="email" name="custemail"></label>"#);
        assert_eq!(named(&nodes, "textbox").name, "E-mail");
    }

    #[test]
    fn bound_label_beats_placeholder() {
        let nodes =
            aom_from(r#"<label for="a">Visible caption</label><input id="a" placeholder="ph">"#);
        assert_eq!(named(&nodes, "textbox").name, "Visible caption");
    }

    #[test]
    fn aria_label_beats_the_bound_label() {
        let nodes =
            aom_from(r#"<label for="b">Ignored</label><input id="b" aria-label="Explicit">"#);
        assert_eq!(named(&nodes, "textbox").name, "Explicit");
    }

    #[test]
    fn aria_labelledby_wins_and_joins_its_idref_list() {
        let nodes = aom_from(
            r#"<span id="first">Preferred</span><span id="second">delivery time</span><input type="text" aria-labelledby="first second" name="delivery">"#,
        );
        assert_eq!(named(&nodes, "textbox").name, "Preferred delivery time");
    }

    #[test]
    fn unlabelled_control_still_resolves_by_name_attribute() {
        // The fallback chain has to survive: agents drive headless pages that
        // carry no labels at all by their `name` attribute.
        let nodes = aom_from(r#"<input type="text" name="custtel">"#);
        assert_eq!(named(&nodes, "textbox").name, "custtel");
    }

    #[test]
    fn labelledby_promotes_an_otherwise_generic_container() {
        let nodes = aom_from(r#"<span id="cap">Sidebar</span><div aria-labelledby="cap"></div>"#);
        let has_named_region = nodes
            .iter()
            .any(|n| n.role == "generic" && n.name == "Sidebar");
        assert!(has_named_region, "got {nodes:?}");
    }

    // -- Selected state (bug #52) ------------------------------------------
    // A checkbox that a tool really checked used to produce an empty delta,
    // because nothing in the AOM could carry "checked".

    #[test]
    fn checked_attribute_reaches_the_aom_node() {
        let nodes = aom_from(
            r#"<input type="checkbox" name="t" value="bacon" checked="checked"><input type="checkbox" name="t" value="onion">"#,
        );
        let boxes: Vec<&AgenticAomNode> = nodes.iter().filter(|n| n.role == "checkbox").collect();
        assert_eq!(boxes.len(), 2, "got {nodes:?}");
        assert!(boxes[0].is_checked, "the checked box must report it");
        assert!(!boxes[1].is_checked, "the empty box must not");
    }

    #[test]
    fn valueless_boolean_attribute_is_still_a_checked_box() {
        // Real markup writes `<input checked>` with no value at all; if the
        // parser drops it every selected control looks unselected.
        let nodes = aom_from(r#"<input type="radio" name="size" value="large" checked>"#);
        assert!(named(&nodes, "radio").is_checked);
    }

    #[test]
    fn checked_survives_into_the_readable_fact_and_triple_streams() {
        let nodes = aom_from(r#"<input type="checkbox" name="t" value="bacon" checked>"#);
        let box_node = named(&nodes, "checkbox");
        assert_eq!(
            box_node.value, "bacon",
            "value stays the submission value; checked is its own fact"
        );
        let doc = AgenticAomTree::to_nda_document(&nodes);
        let checked: Vec<(String, String)> = doc
            .readable_facts()
            .iter()
            .filter(|(_, p, _)| *p == AOM_CHECKED)
            .map(|(s, _, o)| (s.clone(), o.clone()))
            .collect();
        assert_eq!(
            checked,
            vec![(box_node.id.clone(), "checked".to_string())],
            "document facts: {:?}",
            doc.facts
        );
        let triples = AgenticAomTree::to_nda_triples(&nodes);
        assert!(
            triples.iter().any(|t| t.predicate_id == AOM_CHECKED
                && t.object_hash == crate::nda::hash_str("checked")),
            "checked triple missing"
        );
    }

    #[test]
    fn unchecked_control_emits_no_checked_fact() {
        // Otherwise a delta would show a spurious removal on every page.
        let nodes = aom_from(r#"<input type="checkbox" name="t" value="bacon">"#);
        let doc = AgenticAomTree::to_nda_document(&nodes);
        assert!(!doc.facts.iter().any(|f| f.predicate == AOM_CHECKED));
    }
}
#[derive(Debug, Clone)]
pub struct AgenticAomNode {
    pub id: String,
    pub role: String,
    pub name: String,
    pub value: String,
    pub actionability_score: u8,
    pub is_focused: bool,
    pub is_expanded: bool,
    /// Checkbox/radio currently checked. Kept out of `value` (which carries
    /// the control's submission value) so both facts survive independently.
    pub is_checked: bool,
}

pub struct AgenticAomTree;

impl AgenticAomTree {
    pub fn build_aom_nodes(tree: &DomTree) -> Vec<AgenticAomNode> {
        let mut aom_nodes = Vec::new();

        for node in &tree.nodes {
            if node.node_type != NodeType::Element {
                continue;
            }

            let explicit_role = node.attributes.get("role").map(|s| s.as_str());
            let role = explicit_role.unwrap_or_else(|| match node.tag_name.as_str() {
                "button" => "button",
                "a" => "link",
                "input" => {
                    let type_attr = node
                        .attributes
                        .get("type")
                        .map(|s| s.as_str())
                        .unwrap_or("text");
                    match type_attr {
                        "button" | "submit" | "reset" => "button",
                        "checkbox" => "checkbox",
                        "radio" => "radio",
                        _ => "textbox",
                    }
                }
                "select" => "combobox",
                "textarea" => "textbox",
                "form" => "form",
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
                "nav" => "navigation",
                "main" => "main",
                "article" => "article",
                "section" => "region",
                _ => "generic",
            });

            if role == "generic"
                && !node.attributes.contains_key("aria-label")
                && !node.attributes.contains_key("aria-labelledby")
                && !node.attributes.contains_key("id")
            {
                continue;
            }

            // Buttons and links are known by what they show: visible text
            // beats name/id attributes (which are developer plumbing, not
            // what an agent reads on screen). aria-label still wins overall.
            let content_named = matches!(role, "button" | "link");
            let attr_name = if content_named {
                labelledby_text(tree, node)
                    .or_else(|| {
                        node.attributes
                            .get("aria-label")
                            .cloned()
                            .filter(|s| !s.is_empty())
                    })
                    .or_else(|| {
                        node.attributes
                            .get("title")
                            .cloned()
                            .filter(|s| !s.is_empty())
                    })
                    .or_else(|| Some(collect_inner_text(tree, node.id)).filter(|s| !s.is_empty()))
                    .or_else(|| node.attributes.get("name").cloned())
                    .or_else(|| node.attributes.get("id").cloned())
                    .unwrap_or_default()
            } else {
                // HTML-AAM order: aria-labelledby, then aria-label, then the
                // bound <label>, then the fallbacks the engine already used.
                // `name`/`id` stay in the chain so unlabelled controls keep
                // resolving by the same string they used to.
                labelledby_text(tree, node)
                    .or_else(|| {
                        node.attributes
                            .get("aria-label")
                            .cloned()
                            .filter(|s| !s.is_empty())
                    })
                    .or_else(|| {
                        if is_form_control(&node.tag_name) {
                            associated_label_text(tree, node)
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        node.attributes
                            .get("placeholder")
                            .cloned()
                            .filter(|s| !s.is_empty())
                    })
                    .or_else(|| node.attributes.get("name").cloned())
                    .or_else(|| node.attributes.get("id").cloned())
                    .or_else(|| node.attributes.get("title").cloned())
                    .unwrap_or_default()
            };
            let name = if attr_name.is_empty() {
                collect_inner_text(tree, node.id)
            } else {
                attr_name
            };

            let value = node.attributes.get("value").cloned().unwrap_or_default();
            let is_focused = node.attributes.contains_key("autofocus");
            let is_expanded = node
                .attributes
                .get("aria-expanded")
                .map(|s| s == "true")
                .unwrap_or(false);
            // Selected state used to be invisible here, so `read`, the NDA
            // export and every action delta could not tell a checked box from
            // an empty one (bug #52).
            let is_checked =
                matches!(role, "checkbox" | "radio") && node.attributes.contains_key("checked");

            let actionability_score = match role {
                "button" | "link" => 100,
                "textbox" | "checkbox" | "radio" | "combobox" => 90,
                "form" => 75,
                "navigation" | "heading" => 40,
                _ => 10,
            };

            aom_nodes.push(AgenticAomNode {
                id: format!("node_{}", node.id),
                role: role.to_string(),
                name,
                value,
                actionability_score,
                is_focused,
                is_expanded,
                is_checked,
            });
        }

        aom_nodes
    }

    pub fn to_nda_triples(aom_nodes: &[AgenticAomNode]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(aom_nodes.len() * 4);
        for node in aom_nodes {
            triples.push(NdaTriple::new(&node.id, AOM_ROLE, &node.role));
            if !node.name.is_empty() {
                triples.push(NdaTriple::new(&node.id, AOM_NAME, &node.name));
            }
            if !node.value.is_empty() {
                triples.push(NdaTriple::new(&node.id, AOM_VALUE, &node.value));
            }
            triples.push(NdaTriple::new(
                &node.id,
                AOM_ACTIONABILITY,
                &node.actionability_score.to_string(),
            ));
            if node.is_focused {
                triples.push(NdaTriple::new(&node.id, AOM_FOCUSED, "focused"));
            }
            if node.is_expanded {
                triples.push(NdaTriple::new(&node.id, AOM_EXPANDED, "expanded"));
            }
            if node.is_checked {
                triples.push(NdaTriple::new(&node.id, AOM_CHECKED, "checked"));
            }
        }
        triples
    }

    /// Export the AOM as a lossless [`NdaDocument`] the agent can actually read:
    /// roles, names, and values survive as recoverable strings (not hashes).
    /// Facts are emitted in stable node/predicate order for easy diffing.
    pub fn to_nda_document(aom_nodes: &[AgenticAomNode]) -> NdaDocument {
        let mut doc = NdaDocument::new();
        for node in aom_nodes {
            doc.push_str(&node.id, AOM_ROLE, &node.role);
            if !node.name.is_empty() {
                doc.push_str(&node.id, AOM_NAME, &node.name);
            }
            if !node.value.is_empty() {
                doc.push_str(&node.id, AOM_VALUE, &node.value);
            }
            doc.push_int(&node.id, AOM_ACTIONABILITY, node.actionability_score as i64);
            if node.is_focused {
                doc.push_str(&node.id, AOM_FOCUSED, "focused");
            }
            if node.is_expanded {
                doc.push_str(&node.id, AOM_EXPANDED, "expanded");
            }
            if node.is_checked {
                doc.push_str(&node.id, AOM_CHECKED, "checked");
            }
        }
        doc
    }
}
