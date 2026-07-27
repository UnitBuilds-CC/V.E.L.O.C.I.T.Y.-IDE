use crate::nda::NdaTriple;

/// A shadow DOM host element.
#[derive(Debug, Clone)]
pub struct ShadowHost {
    pub host_id: String,
    pub mode: String, // "open" or "closed"
    pub shadow_root_id: String,
    pub children: Vec<ShadowNode>,
    pub slot_assignments: Vec<SlotAssignment>,
}

/// A node within a shadow tree.
#[derive(Debug, Clone)]
pub struct ShadowNode {
    pub node_id: String,
    pub tag: String,
    pub slot: Option<String>,
    pub attributes: Vec<(String, String)>,
    pub text_content: Option<String>,
    pub children: Vec<ShadowNode>,
}

/// A slot assignment mapping light DOM nodes to shadow slots.
#[derive(Debug, Clone)]
pub struct SlotAssignment {
    pub slot_name: String,
    pub assigned_node_ids: Vec<String>,
    pub fallback_content: Option<String>,
}

/// An iframe or frame target for cross-frame navigation.
#[derive(Debug, Clone)]
pub struct FrameTarget {
    pub frame_id: String,
    pub parent_id: Option<String>,
    pub url: String,
    pub security_origin: String,
    pub sandbox_flags: Vec<String>,
    pub is_sandboxed: bool,
}

/// Shadow DOM and frame extraction engine.
pub struct ShadowFrameExtractor;

impl ShadowFrameExtractor {
    /// Extract shadow hosts and their content into NDA triples.
    pub fn extract_shadow_hosts_nda(hosts: &[ShadowHost]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(hosts.len() * 4);
        for host in hosts {
            triples.push(NdaTriple::new(&host.host_id, 20, &host.mode));
            triples.push(NdaTriple::new(&host.host_id, 21, &host.shadow_root_id));
            // Extract child nodes
            for child in &host.children {
                Self::extract_node_nda(child, &host.shadow_root_id, &mut triples);
            }
            // Extract slot assignments
            for slot in &host.slot_assignments {
                triples.push(NdaTriple::new(
                    &host.host_id,
                    22,
                    &format!("slot:{}:{}", slot.slot_name, slot.assigned_node_ids.join(",")),
                ));
            }
        }
        triples
    }

    /// Recursively extract a shadow node into NDA triples.
    fn extract_node_nda(node: &ShadowNode, parent_id: &str, triples: &mut Vec<NdaTriple>) {
        triples.push(NdaTriple::new(&node.node_id, 23, &node.tag));
        triples.push(NdaTriple::new(&node.node_id, 24, parent_id));
        if let Some(slot) = &node.slot {
            triples.push(NdaTriple::new(&node.node_id, 25, slot));
        }
        if let Some(text) = &node.text_content {
            triples.push(NdaTriple::new(&node.node_id, 26, text));
        }
        for (key, value) in &node.attributes {
            triples.push(NdaTriple::new(
                &node.node_id,
                27,
                &format!("{}={}", key, value),
            ));
        }
        for child in &node.children {
            Self::extract_node_nda(child, &node.node_id, triples);
        }
    }

    /// Extract frame targets into NDA triples.
    pub fn extract_frames_nda(frames: &[FrameTarget]) -> Vec<NdaTriple> {
        let mut triples = Vec::with_capacity(frames.len() * 3);
        for frame in frames {
            triples.push(NdaTriple::new(&frame.frame_id, 30, &frame.url));
            triples.push(NdaTriple::new(&frame.frame_id, 31, &frame.security_origin));
            if let Some(parent) = &frame.parent_id {
                triples.push(NdaTriple::new(&frame.frame_id, 32, parent));
            }
            if frame.is_sandboxed {
                triples.push(NdaTriple::new(
                    &frame.frame_id,
                    33,
                    &frame.sandbox_flags.join(","),
                ));
            }
        }
        triples
    }

    /// Find all shadow hosts within a document tree (by tag name heuristic).
    pub fn find_shadow_hosts(html_content: &str) -> Vec<String> {
        let mut hosts = Vec::new();
        // Look for elements with shadow root indicators
        for line in html_content.lines() {
            let trimmed = line.trim();
            if trimmed.contains("attachShadow") || trimmed.contains("shadow-root") {
                // Extract element ID or generate one
                if let Some(id_start) = trimmed.find("id=\"") {
                    let id = &trimmed[id_start + 4..];
                    if let Some(id_end) = id.find('"') {
                        hosts.push(id[..id_end].to_string());
                    }
                }
            }
        }
        hosts
    }

    /// Find all iframes/frames in HTML content.
    pub fn find_frames(html_content: &str, base_url: &str) -> Vec<FrameTarget> {
        let mut frames = Vec::new();
        let mut frame_idx = 0;

        for line in html_content.lines() {
            let trimmed = line.trim().to_lowercase();
            if trimmed.contains("<iframe") || trimmed.contains("<frame") {
                let url = extract_attribute_value(&trimmed, "src")
                    .unwrap_or_default();
                let sandbox = extract_attribute_value(&trimmed, "sandbox");
                let frame_id = format!("frame_{}", frame_idx);
                frame_idx += 1;

                let security_origin = if url.starts_with("http") {
                    url.split('/').take(3).collect::<Vec<_>>().join("/")
                } else {
                    base_url.to_string()
                };

                frames.push(FrameTarget {
                    frame_id,
                    parent_id: None,
                    url,
                    security_origin,
                    is_sandboxed: sandbox.is_some(),
                    sandbox_flags: sandbox.map(|s| s.split_whitespace().map(|w| w.to_string()).collect()).unwrap_or_default(),
                });
            }
        }

        frames
    }
}

/// Extract an attribute value from an HTML tag string.
fn extract_attribute_value(tag: &str, attr_name: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr_name);
    if let Some(start) = tag.find(&pattern) {
        let value_start = start + pattern.len();
        if let Some(end) = tag[value_start..].find('"') {
            return Some(tag[value_start..value_start + end].to_string());
        }
    }
    // Try single quotes
    let pattern = format!("{}='", attr_name);
    if let Some(start) = tag.find(&pattern) {
        let value_start = start + pattern.len();
        if let Some(end) = tag[value_start..].find('\'') {
            return Some(tag[value_start..value_start + end].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_shadow_hosts_nda() {
        let hosts = vec![ShadowHost {
            host_id: "host1".into(),
            mode: "open".into(),
            shadow_root_id: "shadow1".into(),
            children: vec![ShadowNode {
                node_id: "n1".into(),
                tag: "div".into(),
                slot: None,
                attributes: vec![("class".into(), "inner".into())],
                text_content: Some("Hello".into()),
                children: vec![],
            }],
            slot_assignments: vec![SlotAssignment {
                slot_name: "default".into(),
                assigned_node_ids: vec!["light1".into()],
                fallback_content: None,
            }],
        }];
        let triples = ShadowFrameExtractor::extract_shadow_hosts_nda(&hosts);
        assert!(triples.len() >= 4); // host mode + shadow root + child + slot
    }

    #[test]
    fn test_find_shadow_hosts() {
        let html = r#"
            <div id="my-component" attachShadow></div>
            <span>normal</span>
        "#;
        let hosts = ShadowFrameExtractor::find_shadow_hosts(html);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0], "my-component");
    }

    #[test]
    fn test_find_frames() {
        let html = r#"
            <iframe src="https://example.com/page" sandbox="allow-scripts"></iframe>
            <iframe src="/relative/path"></iframe>
        "#;
        let frames = ShadowFrameExtractor::find_frames(html, "https://mysite.com");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].url, "https://example.com/page");
        assert!(frames[0].is_sandboxed);
        assert_eq!(frames[1].url, "/relative/path");
        assert!(!frames[1].is_sandboxed);
    }

    #[test]
    fn test_extract_frames_nda() {
        let frames = vec![FrameTarget {
            frame_id: "f0".into(),
            parent_id: Some("parent".into()),
            url: "https://example.com".into(),
            security_origin: "https://example.com".into(),
            sandbox_flags: vec!["allow-scripts".into()],
            is_sandboxed: true,
        }];
        let triples = ShadowFrameExtractor::extract_frames_nda(&frames);
        assert!(triples.len() >= 3); // url + origin + parent + sandbox
    }

    #[test]
    fn test_extract_attribute_value() {
        assert_eq!(extract_attribute_value(r#"<div src="hello.html">"#, "src"), Some("hello.html".into()));
        assert_eq!(extract_attribute_value("<div src='single.html'>", "src"), Some("single.html".into()));
        assert_eq!(extract_attribute_value("<div class='x'>", "src"), None);
    }
}
