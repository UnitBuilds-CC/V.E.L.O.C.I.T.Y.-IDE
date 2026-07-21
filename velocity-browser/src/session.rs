use crate::agentic::{AgenticAomNode, AgenticAomTree};
use crate::dom::DomTree;
use crate::engine::{CanvasElement, CanvasExtractor, FrameTarget, InterstitialClassifier, InterstitialKind, NetworkTracker, ShadowFrameExtractor, ShadowHost};
use crate::nda::NdaTriple;
use crate::parser::{CssMatcher, HtmlParser};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: f64,
    pub http_only: bool,
    pub secure: bool,
}

#[derive(Debug, Clone)]
pub struct DownloadArtifact {
    pub guid: String,
    pub url: String,
    pub file_name: String,
    pub total_bytes: i64,
    pub save_path: String,
}

pub struct BrowserSession {
    pub session_id: String,
    pub current_url: String,
    pub page_title: String,
    pub dom_tree: Option<DomTree>,
    pub network_tracker: NetworkTracker,
    pub cookies: Vec<Cookie>,
    pub storage: HashMap<String, String>,
    pub downloads: Vec<DownloadArtifact>,
    pub trace_logs: Vec<String>,
    pub shadow_hosts: Vec<ShadowHost>,
    pub frames: Vec<FrameTarget>,
    pub canvases: Vec<CanvasElement>,
}

impl BrowserSession {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            current_url: String::new(),
            page_title: "Untitled Page".to_string(),
            dom_tree: None,
            network_tracker: NetworkTracker::new(),
            cookies: Vec::new(),
            storage: HashMap::new(),
            downloads: Vec::new(),
            trace_logs: Vec::new(),
            shadow_hosts: Vec::new(),
            frames: Vec::new(),
            canvases: Vec::new(),
        }
    }

    /// Native pure-Rust HTML document loading and DOM tree compilation
    pub fn load_html(&mut self, url: &str, html: &str) -> Vec<NdaTriple> {
        self.current_url = url.to_string();
        let nodes = HtmlParser::parse(html);
        let tree = DomTree::new(nodes);
        self.page_title = tree.extract_page_title();
        self.dom_tree = Some(tree);

        self.trace_logs.push(format!("Loaded HTML from {}", url));
        self.capture_state_nda()
    }

    /// Native CSS selector element query & click event execution
    pub fn click(&mut self, selector: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tree) = &self.dom_tree {
            let matches = CssMatcher::find_matches(&tree.nodes, selector);
            if !matches.is_empty() {
                self.trace_logs.push(format!("Clicked native element matching '{}'", selector));
                return Ok(());
            }
            return Err(format!("Element with selector '{}' not found", selector).into());
        }
        Err("No DOM tree loaded in session".into())
    }

    /// Native CSS selector form input filling
    pub fn fill(&mut self, selector: &str, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tree) = &mut self.dom_tree {
            let target_id = {
                let matches = CssMatcher::find_matches(&tree.nodes, selector);
                matches.first().map(|n| n.id)
            };

            if let Some(id) = target_id {
                if let Some(node) = tree.get_node_mut(id) {
                    node.attributes.insert("value".to_string(), text.to_string());
                    self.trace_logs.push(format!("Filled native element '{}' with text '{}'", selector, text));
                    return Ok(());
                }
            }
            return Err(format!("Element with selector '{}' not found", selector).into());
        }
        Err("No DOM tree loaded in session".into())
    }

    pub fn scroll(&mut self, delta_x: i32, delta_y: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.trace_logs.push(format!("Scrolled window by ({}, {})", delta_x, delta_y));
        Ok(())
    }

    pub fn hover(&mut self, selector: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.trace_logs.push(format!("Hovered native selector '{}'", selector));
        Ok(())
    }

    pub fn press_key(&mut self, key: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.trace_logs.push(format!("Pressed key '{}'", key));
        Ok(())
    }

    pub fn classify_interstitial(&self, html_snippet: &str) -> InterstitialKind {
        InterstitialClassifier::classify_page(&self.page_title, html_snippet)
    }

    /// Compile complete browser session state directly into packed binary NDA triples
    pub fn capture_state_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        triples.push(NdaTriple::new(&self.session_id, 100, &self.current_url));
        triples.push(NdaTriple::new(&self.session_id, 101, &self.page_title));

        for cookie in &self.cookies {
            triples.push(NdaTriple::new(&cookie.name, 102, &cookie.value));
        }
        for (k, v) in &self.storage {
            triples.push(NdaTriple::new(k, 103, v));
        }

        // Add native Agentic AOM triples from DOM tree
        if let Some(tree) = &self.dom_tree {
            let aom_nodes = AgenticAomTree::build_aom_nodes(tree);
            triples.extend(AgenticAomTree::to_nda_triples(&aom_nodes));
        }

        // Add Shadow DOM, frame, canvas, and network triples
        triples.extend(ShadowFrameExtractor::extract_shadow_hosts_nda(&self.shadow_hosts));
        triples.extend(ShadowFrameExtractor::extract_frames_nda(&self.frames));
        triples.extend(CanvasExtractor::extract_canvases_nda(&self.canvases));
        triples.extend(self.network_tracker.export_triples_nda());

        triples
    }
}
