#![allow(dead_code)]

use velocity_browser::{BrowserSession, SwarmSessionOrchestrator, NdaTriple};
use velocity_browser::screencast::ScreencastRecorder;
use velocity_browser::vector_memory::SiteVectorStore;
use std::sync::{Arc, Mutex, LazyLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct NativeBrowserBridge {
    pub swarm: SwarmSessionOrchestrator,
    pub active_session: BrowserSession,
    pub screencast: ScreencastRecorder,
    pub vector_memory: SiteVectorStore,
}

static NATIVE_BRIDGES: LazyLock<Arc<Mutex<HashMap<String, Arc<Mutex<NativeBrowserBridge>>>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

pub fn get_or_create_native_bridge(session_id: &str) -> Arc<Mutex<NativeBrowserBridge>> {
    let mut map = NATIVE_BRIDGES.lock().unwrap();
    map.entry(session_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(NativeBrowserBridge::new(session_id))))
        .clone()
}

pub fn persist_native_nda_triples(
    workspace_root: &Path,
    session_id: &str,
    triples: &[NdaTriple],
) -> Result<PathBuf, String> {
    let artifacts_dir = workspace_root.join(".velocity").join("browser_artifacts");
    let _ = std::fs::create_dir_all(&artifacts_dir);
    let nda_path = artifacts_dir.join(format!("{}_native.nda", session_id));
    let mut encoded = Vec::new();
    for t in triples {
        encoded.extend_from_slice(&t.to_bytes());
    }
    std::fs::write(&nda_path, &encoded).map_err(|e| format!("failed to write native NDA: {e}"))?;
    Ok(nda_path)
}

impl NativeBrowserBridge {
    pub fn new(session_id: &str) -> Self {
        Self {
            swarm: SwarmSessionOrchestrator::new(),
            active_session: BrowserSession::new(session_id.to_string()),
            screencast: ScreencastRecorder::new(session_id),
            vector_memory: SiteVectorStore::new(),
        }
    }

    pub fn navigate(&mut self, url: &str) -> Result<Vec<NdaTriple>, Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.fetch_and_load(url)
    }

    pub fn click(&mut self, selector: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.click(selector)
    }

    pub fn click_ocr(&mut self, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.click_ocr_text(text)
    }

    pub fn fill(&mut self, selector: &str, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.fill(selector, text)
    }

    pub fn eval(&mut self, expr: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.active_session.eval_js(expr)
    }

    pub fn predict_action(&self) -> Option<velocity_browser::PredictedActionTarget> {
        self.active_session.predict_action()
    }

    pub fn spawn_swarm_tab(&mut self, session_id: &str) -> &mut BrowserSession {
        self.swarm.spawn_swarm_tab(session_id)
    }

    pub fn capture_nda(&self) -> Vec<NdaTriple> {
        self.active_session.capture_state_nda()
    }
}
