use crate::cdp::NativeCdpClient;
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
    pub current_url: String,
    pub cookies: Vec<Cookie>,
    pub storage: HashMap<String, String>,
    pub downloads: Vec<DownloadArtifact>,
    pub trace_logs: Vec<String>,
}

impl BrowserSession {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            client: None,
            current_url: String::new(),
            cookies: Vec::new(),
            storage: HashMap::new(),
            downloads: Vec::new(),
            trace_logs: Vec::new(),
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

    pub fn set_files(&mut self, selector: &str, files: &[&str]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(client) = self.client.as_mut() {
            let files_str = files.iter().map(|f| format!("\"{}\"", f)).collect::<Vec<_>>().join(",");
            let params = format!("{{\"selector\":\"{}\",\"files\":[{}]}}", selector, files_str);
            let _ = client.send_command("DOM.setFileInputFiles", &params)?;
            self.trace_logs.push(format!("Attached files to selector '{}'", selector));
            return Ok(());
        }
        Err("Client not connected".into())
    }

    pub fn set_download_behavior(&mut self, download_dir: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(client) = self.client.as_mut() {
            let params = format!("{{\"behavior\":\"allow\",\"downloadPath\":\"{}\"}}", download_dir);
            let _ = client.send_command("Page.setDownloadBehavior", &params)?;
            self.trace_logs.push(format!("Set download directory to '{}'", download_dir));
            return Ok(());
        }
        Err("Client not connected".into())
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
        triples
    }
}
