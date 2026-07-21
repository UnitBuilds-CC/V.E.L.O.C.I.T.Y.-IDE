use crate::cdp::{CdpEventLoop, NativeCdpClient};
use crate::engine::{CanvasElement, CanvasExtractor, FrameTarget, InterstitialClassifier, InterstitialKind, NetworkTracker, ShadowFrameExtractor, ShadowHost};
use crate::nda::NdaTriple;
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
    pub client: Option<NativeCdpClient>,
    pub event_loop: CdpEventLoop,
    pub network_tracker: NetworkTracker,
    pub current_url: String,
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
            client: None,
            event_loop: CdpEventLoop::new(),
            network_tracker: NetworkTracker::new(),
            current_url: String::new(),
            cookies: Vec::new(),
            storage: HashMap::new(),
            downloads: Vec::new(),
            trace_logs: Vec::new(),
            shadow_hosts: Vec::new(),
            frames: Vec::new(),
            canvases: Vec::new(),
        }
    }

    pub fn connect(&mut self, host: &str, port: u16, path: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = NativeCdpClient::connect(host, port, path)?;
        self.client = Some(client);
        Ok(())
    }

    pub fn navigate(&mut self, url: &str) -> Result<Vec<NdaTriple>, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(client) = self.client.as_mut() {
            let params = format!("{{\"url\":\"{}\"}}", url);
            let _ = client.send_command("Page.navigate", &params)?;
            self.current_url = url.to_string();
            self.trace_logs.push(format!("Navigated to {}", url));
            let triples = client.page_to_nda_triples(url, "Loaded");
            return Ok(triples);
        }
        Err("Client not connected".into())
    }

    pub fn click(&mut self, selector: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(client) = self.client.as_mut() {
            let eval_script = format!(
                "{{\"expression\":\"document.querySelector('{}').click()\"}}",
                selector
            );
            let _ = client.send_command("Runtime.evaluate", &eval_script)?;
            self.trace_logs.push(format!("Clicked selector '{}'", selector));
            return Ok(());
        }
        Err("Client not connected".into())
    }

    pub fn fill(&mut self, selector: &str, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(client) = self.client.as_mut() {
            let eval_script = format!(
                "{{\"expression\":\"let el = document.querySelector('{}'); el.value = '{}'; el.dispatchEvent(new Event('input', {{bubbles:true}}));\"}}",
                selector, text
            );
            let _ = client.send_command("Runtime.evaluate", &eval_script)?;
            self.trace_logs.push(format!("Filled selector '{}'", selector));
            return Ok(());
        }
        Err("Client not connected".into())
    }

    pub fn scroll(&mut self, delta_x: i32, delta_y: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(client) = self.client.as_mut() {
            let eval_script = format!(
                "{{\"expression\":\"window.scrollBy({}, {})\"}}",
                delta_x, delta_y
            );
            let _ = client.send_command("Runtime.evaluate", &eval_script)?;
            self.trace_logs.push(format!("Scrolled window by ({}, {})", delta_x, delta_y));
            return Ok(());
        }
        Err("Client not connected".into())
    }

    pub fn hover(&mut self, selector: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(client) = self.client.as_mut() {
            let eval_script = format!(
                "{{\"expression\":\"let el = document.querySelector('{}'); if (el) el.dispatchEvent(new MouseEvent('mouseover', {{bubbles:true}}));\"}}",
                selector
            );
            let _ = client.send_command("Runtime.evaluate", &eval_script)?;
            self.trace_logs.push(format!("Hovered selector '{}'", selector));
            return Ok(());
        }
        Err("Client not connected".into())
    }

    pub fn press_key(&mut self, key: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(client) = self.client.as_mut() {
            let eval_script = format!(
                "{{\"expression\":\"document.activeElement.dispatchEvent(new KeyboardEvent('keydown', {{key:'{}', bubbles:true}}));\"}}",
                key
            );
            let _ = client.send_command("Runtime.evaluate", &eval_script)?;
            self.trace_logs.push(format!("Pressed key '{}'", key));
            return Ok(());
        }
        Err("Client not connected".into())
    }

    pub fn classify_interstitial(&self, title: &str, html_snippet: &str) -> InterstitialKind {
        InterstitialClassifier::classify_page(title, html_snippet)
    }

    pub fn capture_state_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        triples.push(NdaTriple::new(&self.session_id, 100, &self.current_url));
        for cookie in &self.cookies {
            triples.push(NdaTriple::new(&cookie.name, 101, &cookie.value));
        }
        for (k, v) in &self.storage {
            triples.push(NdaTriple::new(k, 102, v));
        }

        // Add Shadow DOM, frame, canvas, and network triples
        triples.extend(ShadowFrameExtractor::extract_shadow_hosts_nda(&self.shadow_hosts));
        triples.extend(ShadowFrameExtractor::extract_frames_nda(&self.frames));
        triples.extend(CanvasExtractor::extract_canvases_nda(&self.canvases));
        triples.extend(self.network_tracker.export_triples_nda());

        triples
    }
}
