use velocity_browser::{BrowserSession, SwarmSessionOrchestrator, NdaTriple};

pub struct NativeBrowserBridge {
    pub swarm: SwarmSessionOrchestrator,
    pub active_session: BrowserSession,
}

impl NativeBrowserBridge {
    pub fn new(session_id: &str) -> Self {
        Self {
            swarm: SwarmSessionOrchestrator::new(),
            active_session: BrowserSession::new(session_id.to_string()),
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
