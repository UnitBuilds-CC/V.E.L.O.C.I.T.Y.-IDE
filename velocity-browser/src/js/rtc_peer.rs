//! JavaScript-facing `RTCPeerConnection` facade.
//!
//! Exposes the WebRTC transport ([`crate::net::webrtc::WebRtcTransport`]) to the
//! JS runtime as a spec-shaped `RTCPeerConnection` object: string-valued state
//! attributes, `createOffer`/`createAnswer`, local/remote description handling,
//! ICE candidate trickle, and data-channel send/receive. Peers are held in a
//! process-wide registry keyed by an opaque id so the string-level JS pipeline
//! can refer to a connection without threading Rust objects through the
//! interpreter.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::net::webrtc::{
    ConnectionState, DataChannelState, IceConnectionState, SessionDescription, SignalingState,
    SdpType, WebRtcTransport,
};

/// Process-wide registry of live peer connections keyed by opaque id.
static PEERS: Mutex<Option<HashMap<u32, WebRtcTransport>>> = Mutex::new(None);
/// Monotonic peer-id source.
static NEXT_ID: Mutex<u32> = Mutex::new(1);

fn with_peers<R>(f: impl FnOnce(&mut HashMap<u32, WebRtcTransport>) -> R) -> R {
    let mut guard = PEERS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// A JavaScript `RTCRtpTransceiver`-free peer connection facade.
#[derive(Debug, Clone, Copy)]
pub struct JsRtcPeerConnection {
    pub id: u32,
}

impl JsRtcPeerConnection {
    /// Construct a new peer connection (`new RTCPeerConnection(config)`).
    pub fn new() -> Self {
        let id = {
            let mut n = NEXT_ID.lock().unwrap();
            let id = *n;
            *n += 1;
            id
        };
        with_peers(|map| map.insert(id, WebRtcTransport::new(&format!("peer-{}", id))));
        Self { id }
    }

    fn with<R>(&self, f: impl FnOnce(&mut WebRtcTransport) -> R) -> Option<R> {
        with_peers(|map| map.get_mut(&self.id).map(f))
    }

    /// `pc.createOffer()` → `{ type: 'offer', sdp }`.
    pub fn create_offer(&self) -> Option<HashMap<String, String>> {
        self.with(|t| session_to_js(&t.create_offer()))
    }

    /// `pc.createAnswer()` → `{ type: 'answer', sdp }`.
    pub fn create_answer(&self) -> Option<HashMap<String, String>> {
        self.with(|t| session_to_js(&t.create_answer()))
    }

    /// `pc.setLocalDescription(desc)`.
    pub fn set_local_description(&self, desc: HashMap<String, String>) -> Result<(), String> {
        let sd = js_to_session(&desc)?;
        self.with(|t| t.set_local_description(sd)).unwrap_or(Err("peer not found".into()))
    }

    /// `pc.setRemoteDescription(desc)`.
    pub fn set_remote_description(&self, desc: HashMap<String, String>) -> Result<(), String> {
        let sd = js_to_session(&desc)?;
        self.with(|t| t.set_remote_description(sd)).unwrap_or(Err("peer not found".into()))
    }

    /// `pc.addIceCandidate(candidate)`.
    pub fn add_ice_candidate(&self, candidate: &str) {
        let _ = self.with(|t| t.add_ice_candidate(candidate));
    }

    /// `pc.createDataChannel(label)` → the channel label.
    pub fn create_data_channel(&self, label: &str) -> Option<String> {
        self.with(|t| t.create_data_channel(label).label.clone())
    }

    /// Open a previously created data channel (models the handshake completing).
    pub fn open_data_channel(&self, label: &str) -> bool {
        self.with(|t| {
            if let Some(ch) = t.data_channels.iter_mut().find(|c| c.label == label) {
                ch.open();
                true
            } else {
                false
            }
        })
        .unwrap_or(false)
    }

    /// `channel.readyState` for the channel with the given label.
    pub fn data_channel_state(&self, label: &str) -> Option<&'static str> {
        self.with(|t| {
            t.data_channels
                .iter()
                .find(|c| c.label == label)
                .map(|c| data_channel_state_str(&c.ready_state))
        })
        .flatten()
    }

    /// `channel.send(data)` for the channel with the given label.
    pub fn send(&self, label: &str, data: &[u8]) -> Result<(), String> {
        self.with(|t| {
            t.data_channels
                .iter_mut()
                .find(|c| c.label == label)
                .map(|c| c.send(data))
                .unwrap_or(Err("no such channel".into()))
        })
        .unwrap_or(Err("peer not found".into()))
    }

    /// `pc.signalingState`.
    pub fn signaling_state(&self) -> Option<&'static str> {
        self.with(|t| signaling_state_str(&t.signaling_state))
    }

    /// `pc.connectionState`.
    pub fn connection_state(&self) -> Option<&'static str> {
        self.with(|t| connection_state_str(&t.connection_state))
    }

    /// `pc.iceConnectionState`.
    pub fn ice_connection_state(&self) -> Option<&'static str> {
        self.with(|t| ice_connection_state_str(&t.ice_connection_state))
    }

    /// `pc.close()`.
    pub fn close(&self) {
        let _ = self.with(|t| t.close());
    }
}

impl Default for JsRtcPeerConnection {
    fn default() -> Self {
        Self::new()
    }
}

/// Drop all peer connections. Call between navigations to avoid stale state.
pub fn clear_peers() {
    with_peers(|map| map.clear());
}

fn session_to_js(desc: &SessionDescription) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        "type".to_string(),
        match desc.sdp_type {
            SdpType::Offer => "offer",
            SdpType::Answer => "answer",
            SdpType::Pranswer => "pranswer",
        }
        .to_string(),
    );
    m.insert("sdp".to_string(), desc.sdp.clone());
    m
}

fn js_to_session(desc: &HashMap<String, String>) -> Result<SessionDescription, String> {
    let sdp_type = match desc.get("type").map(|s| s.as_str()) {
        Some("offer") => SdpType::Offer,
        Some("answer") => SdpType::Answer,
        Some("pranswer") => SdpType::Pranswer,
        other => return Err(format!("invalid SDP type: {:?}", other)),
    };
    Ok(SessionDescription { sdp_type, sdp: desc.get("sdp").cloned().unwrap_or_default() })
}

fn signaling_state_str(s: &SignalingState) -> &'static str {
    match s {
        SignalingState::Stable => "stable",
        SignalingState::HaveLocalOffer => "have-local-offer",
        SignalingState::HaveRemoteOffer => "have-remote-offer",
        SignalingState::HaveLocalPranswer => "have-local-pranswer",
        SignalingState::HaveRemotePranswer => "have-remote-pranswer",
        SignalingState::Closed => "closed",
    }
}

fn connection_state_str(s: &ConnectionState) -> &'static str {
    match s {
        ConnectionState::New => "new",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Connected => "connected",
        ConnectionState::Disconnected => "disconnected",
        ConnectionState::Failed => "failed",
        ConnectionState::Closed => "closed",
    }
}

fn ice_connection_state_str(s: &IceConnectionState) -> &'static str {
    match s {
        IceConnectionState::New => "new",
        IceConnectionState::Checking => "checking",
        IceConnectionState::Connected => "connected",
        IceConnectionState::Completed => "completed",
        IceConnectionState::Failed => "failed",
        IceConnectionState::Disconnected => "disconnected",
        IceConnectionState::Closed => "closed",
    }
}

fn data_channel_state_str(s: &DataChannelState) -> &'static str {
    match s {
        DataChannelState::Connecting => "connecting",
        DataChannelState::Open => "open",
        DataChannelState::Closing => "closing",
        DataChannelState::Closed => "closed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_peer_starts_in_new_states() {
        let pc = JsRtcPeerConnection::new();
        assert_eq!(pc.signaling_state(), Some("stable"));
        assert_eq!(pc.connection_state(), Some("new"));
        assert_eq!(pc.ice_connection_state(), Some("new"));
        pc.close();
    }

    #[test]
    fn offer_answer_handshake_reaches_connected() {
        let caller = JsRtcPeerConnection::new();
        let callee = JsRtcPeerConnection::new();

        let offer = caller.create_offer().expect("offer");
        assert_eq!(offer.get("type").map(|s| s.as_str()), Some("offer"));
        // create_offer() models offer creation + local application, so the
        // signaling state advances to have-local-offer.
        assert_eq!(caller.signaling_state(), Some("have-local-offer"));

        callee.set_remote_description(offer).unwrap();
        assert_eq!(callee.signaling_state(), Some("have-remote-offer"));

        let answer = callee.create_answer().expect("answer");
        assert_eq!(answer.get("type").map(|s| s.as_str()), Some("answer"));
        // create_answer() models answer creation + local application while in
        // have-remote-offer, returning the callee to stable.
        assert_eq!(callee.signaling_state(), Some("stable"));

        caller.set_remote_description(answer).unwrap();
        assert_eq!(caller.signaling_state(), Some("stable"));
        assert_eq!(caller.connection_state(), Some("connected"));

        caller.close();
        callee.close();
    }

    #[test]
    fn data_channel_lifecycle_and_send() {
        let pc = JsRtcPeerConnection::new();
        assert_eq!(pc.create_data_channel("chat").as_deref(), Some("chat"));
        assert_eq!(pc.data_channel_state("chat"), Some("connecting"));
        // Sending before open fails.
        assert!(pc.send("chat", b"hi").is_err());
        // Once open, send succeeds.
        assert!(pc.open_data_channel("chat"));
        assert_eq!(pc.data_channel_state("chat"), Some("open"));
        assert!(pc.send("chat", b"hi").is_ok());
        pc.close();
    }

    #[test]
    fn close_transitions_all_states() {
        let pc = JsRtcPeerConnection::new();
        pc.create_data_channel("chan");
        pc.close();
        assert_eq!(pc.signaling_state(), Some("closed"));
        assert_eq!(pc.connection_state(), Some("closed"));
        assert_eq!(pc.ice_connection_state(), Some("closed"));
        assert_eq!(pc.data_channel_state("chan"), Some("closed"));
    }

    #[test]
    fn invalid_sdp_type_is_rejected() {
        let pc = JsRtcPeerConnection::new();
        let mut bad = HashMap::new();
        bad.insert("type".to_string(), "bogus".to_string());
        assert!(pc.set_local_description(bad).is_err());
        pc.close();
    }

    #[test]
    fn set_local_offer_advances_signaling_state() {
        let pc = JsRtcPeerConnection::new();
        let mut desc = HashMap::new();
        desc.insert("type".to_string(), "offer".to_string());
        desc.insert("sdp".to_string(), "v=0".to_string());
        pc.set_local_description(desc).unwrap();
        assert_eq!(pc.signaling_state(), Some("have-local-offer"));
        pc.close();
    }
}
