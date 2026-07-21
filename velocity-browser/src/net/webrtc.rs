#[derive(Debug, Clone)]
pub enum IceCandidateState {
    Gathering,
    Complete,
}

pub struct WebRtcTransport {
    pub peer_id: String,
    pub sdp_offer: Option<String>,
    pub sdp_answer: Option<String>,
    pub ice_candidates: Vec<String>,
    pub ice_state: IceCandidateState,
}

impl WebRtcTransport {
    pub fn new(peer_id: &str) -> Self {
        Self {
            peer_id: peer_id.to_string(),
            sdp_offer: None,
            sdp_answer: None,
            ice_candidates: Vec::new(),
            ice_state: IceCandidateState::Gathering,
        }
    }

    pub fn set_remote_offer(&mut self, sdp: &str) -> String {
        self.sdp_offer = Some(sdp.to_string());
        let answer = format!("v=0\r\no=- 12345 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=sendrecv\r\n");
        self.sdp_answer = Some(answer.clone());
        answer
    }

    pub fn add_ice_candidate(&mut self, candidate: &str) {
        self.ice_candidates.push(candidate.to_string());
    }
}
