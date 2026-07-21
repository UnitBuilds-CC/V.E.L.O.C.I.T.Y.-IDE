use crate::nda::NdaTriple;

pub struct InspectorServer {
    pub port: u16,
    pub is_listening: bool,
}

impl InspectorServer {
    pub fn new(port: u16) -> Self {
        Self { port, is_listening: true }
    }

    pub fn handle_agent_inspection(&self, session_id: &str) -> Vec<NdaTriple> {
        vec![
            NdaTriple::new(session_id, 200, &format!("inspector_port:{}", self.port)),
            NdaTriple::new(session_id, 201, "devtools_attached"),
        ]
    }
}
