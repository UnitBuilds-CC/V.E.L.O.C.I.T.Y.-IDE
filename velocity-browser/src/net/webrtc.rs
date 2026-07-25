#[derive(Debug, Clone, PartialEq)]
pub enum IceCandidateState {
    New,
    Gathering,
    Complete,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SignalingState {
    Stable,
    HaveLocalOffer,
    HaveRemoteOffer,
    HaveLocalPranswer,
    HaveRemotePranswer,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    New,
    Connecting,
    Connected,
    Disconnected,
    Failed,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IceConnectionState {
    New,
    Checking,
    Connected,
    Completed,
    Failed,
    Disconnected,
    Closed,
}

/// SDP type (offer or answer).
#[derive(Debug, Clone, PartialEq)]
pub enum SdpType {
    Offer,
    Answer,
    Pranswer,
}

/// An SDP session description.
#[derive(Debug, Clone)]
pub struct SessionDescription {
    pub sdp_type: SdpType,
    pub sdp: String,
}

/// A WebRTC data channel for bidirectional data transfer.
#[derive(Debug, Clone)]
pub struct DataChannel {
    pub label: String,
    pub ordered: bool,
    pub max_packet_life_time: Option<u16>,
    pub max_retransmits: Option<u16>,
    pub protocol: String,
    pub negotiated: bool,
    pub id: Option<u16>,
    pub ready_state: DataChannelState,
    pub buffered_amount: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DataChannelState {
    Connecting,
    Open,
    Closing,
    Closed,
}

impl DataChannel {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            ordered: true,
            max_packet_life_time: None,
            max_retransmits: None,
            protocol: String::new(),
            negotiated: false,
            id: None,
            ready_state: DataChannelState::Connecting,
            buffered_amount: 0,
        }
    }

    pub fn send(&mut self, data: &[u8]) -> Result<(), String> {
        if self.ready_state != DataChannelState::Open {
            return Err("DataChannel is not open".to_string());
        }
        self.buffered_amount += data.len();
        Ok(())
    }

    pub fn close(&mut self) {
        self.ready_state = DataChannelState::Closed;
    }
}

/// A media stream track (audio or video).
#[derive(Debug, Clone)]
pub struct MediaStreamTrack {
    pub id: String,
    pub kind: TrackKind,
    pub enabled: bool,
    pub muted: bool,
    pub ready_state: TrackState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackKind {
    Audio,
    Video,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackState {
    Live,
    Ended,
}

impl MediaStreamTrack {
    pub fn new_audio(id: &str) -> Self {
        Self { id: id.to_string(), kind: TrackKind::Audio, enabled: true, muted: false, ready_state: TrackState::Live }
    }
    pub fn new_video(id: &str) -> Self {
        Self { id: id.to_string(), kind: TrackKind::Video, enabled: true, muted: false, ready_state: TrackState::Live }
    }
    pub fn stop(&mut self) {
        self.ready_state = TrackState::Ended;
    }
}

/// ICE server configuration for STUN/TURN.
#[derive(Debug, Clone)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

impl IceServer {
    pub fn stun(url: &str) -> Self {
        Self { urls: vec![url.to_string()], username: None, credential: None }
    }
    pub fn turn(url: &str, username: &str, credential: &str) -> Self {
        Self {
            urls: vec![url.to_string()],
            username: Some(username.to_string()),
            credential: Some(credential.to_string()),
        }
    }
}

/// Configuration for a peer connection.
#[derive(Debug, Clone)]
pub struct RtcConfiguration {
    pub ice_servers: Vec<IceServer>,
    pub ice_candidate_pool_size: u32,
    pub bundle_policy: BundlePolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BundlePolicy {
    Balanced,
    MaxBundle,
    MaxCompat,
}

impl Default for RtcConfiguration {
    fn default() -> Self {
        Self {
            ice_servers: vec![IceServer::stun("stun:stun.l.google.com:19302")],
            ice_candidate_pool_size: 0,
            bundle_policy: BundlePolicy::Balanced,
        }
    }
}

pub struct WebRtcTransport {
    pub peer_id: String,
    pub config: RtcConfiguration,
    pub signaling_state: SignalingState,
    pub connection_state: ConnectionState,
    pub ice_connection_state: IceConnectionState,
    pub ice_gathering_state: IceCandidateState,
    pub sdp_offer: Option<SessionDescription>,
    pub sdp_answer: Option<SessionDescription>,
    pub ice_candidates: Vec<String>,
    pub local_tracks: Vec<MediaStreamTrack>,
    pub remote_tracks: Vec<MediaStreamTrack>,
    pub data_channels: Vec<DataChannel>,
}

impl WebRtcTransport {
    pub fn new(peer_id: &str) -> Self {
        Self {
            peer_id: peer_id.to_string(),
            config: RtcConfiguration::default(),
            signaling_state: SignalingState::Stable,
            connection_state: ConnectionState::New,
            ice_connection_state: IceConnectionState::New,
            ice_gathering_state: IceCandidateState::New,
            sdp_offer: None,
            sdp_answer: None,
            ice_candidates: Vec::new(),
            local_tracks: Vec::new(),
            remote_tracks: Vec::new(),
            data_channels: Vec::new(),
        }
    }

    pub fn with_config(peer_id: &str, config: RtcConfiguration) -> Self {
        let mut t = Self::new(peer_id);
        t.config = config;
        t
    }

    /// Create an SDP offer.
    pub fn create_offer(&mut self) -> SessionDescription {
        let sdp = format!(
            "v=0\r\no=- {} 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=sendrecv\r\na=group:BUNDLE 0\r\n",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs()).unwrap_or(0)
        );
        let desc = SessionDescription { sdp_type: SdpType::Offer, sdp };
        self.sdp_offer = Some(desc.clone());
        self.signaling_state = SignalingState::HaveLocalOffer;
        self.ice_gathering_state = IceCandidateState::Gathering;
        desc
    }

    /// Create an SDP answer in response to a remote offer.
    pub fn create_answer(&mut self) -> SessionDescription {
        let sdp = "v=0\r\no=- 12345 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=sendrecv\r\n".to_string();
        let desc = SessionDescription { sdp_type: SdpType::Answer, sdp };
        self.sdp_answer = Some(desc.clone());
        if self.signaling_state == SignalingState::HaveRemoteOffer {
            self.signaling_state = SignalingState::Stable;
        }
        desc
    }

    /// Set the local description (offer or answer).
    pub fn set_local_description(&mut self, desc: SessionDescription) {
        match desc.sdp_type {
            SdpType::Offer => {
                self.sdp_offer = Some(desc);
                self.signaling_state = SignalingState::HaveLocalOffer;
            }
            SdpType::Answer | SdpType::Pranswer => {
                self.sdp_answer = Some(desc);
                self.signaling_state = SignalingState::Stable;
            }
        }
    }

    /// Set the remote description (offer or answer).
    pub fn set_remote_description(&mut self, desc: SessionDescription) {
        match desc.sdp_type {
            SdpType::Offer => {
                self.sdp_offer = Some(desc);
                self.signaling_state = SignalingState::HaveRemoteOffer;
            }
            SdpType::Answer | SdpType::Pranswer => {
                self.sdp_answer = Some(desc);
                self.signaling_state = SignalingState::Stable;
                self.connection_state = ConnectionState::Connected;
                self.ice_connection_state = IceConnectionState::Connected;
            }
        }
    }

    pub fn add_ice_candidate(&mut self, candidate: &str) {
        self.ice_candidates.push(candidate.to_string());
    }

    /// Create a new data channel.
    pub fn create_data_channel(&mut self, label: &str) -> &mut DataChannel {
        self.data_channels.push(DataChannel::new(label));
        self.data_channels.last_mut().unwrap()
    }

    /// Add a local media track.
    pub fn add_track(&mut self, track: MediaStreamTrack) {
        self.local_tracks.push(track);
    }

    /// Close the peer connection.
    pub fn close(&mut self) {
        self.signaling_state = SignalingState::Closed;
        self.connection_state = ConnectionState::Closed;
        self.ice_connection_state = IceConnectionState::Closed;
        for ch in &mut self.data_channels {
            ch.close();
        }
        for track in &mut self.local_tracks {
            track.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_answer_flow() {
        let mut peer = WebRtcTransport::new("peer-1");
        let offer = peer.create_offer();
        assert_eq!(offer.sdp_type, SdpType::Offer);
        assert_eq!(peer.signaling_state, SignalingState::HaveLocalOffer);

        let answer = peer.create_answer();
        assert_eq!(answer.sdp_type, SdpType::Answer);
    }

    #[test]
    fn data_channel_lifecycle() {
        let mut peer = WebRtcTransport::new("peer-1");
        let ch = peer.create_data_channel("chat");
        assert_eq!(ch.label, "chat");
        assert_eq!(ch.ready_state, DataChannelState::Connecting);
        ch.ready_state = DataChannelState::Open;
        assert!(ch.send(b"hello").is_ok());
        assert_eq!(ch.buffered_amount, 5);
        ch.close();
        assert_eq!(ch.ready_state, DataChannelState::Closed);
    }

    #[test]
    fn media_track_lifecycle() {
        let mut track = MediaStreamTrack::new_video("v1");
        assert_eq!(track.kind, TrackKind::Video);
        assert_eq!(track.ready_state, TrackState::Live);
        track.stop();
        assert_eq!(track.ready_state, TrackState::Ended);
    }

    #[test]
    fn close_cleans_up() {
        let mut peer = WebRtcTransport::new("peer-1");
        peer.create_data_channel("dc");
        peer.add_track(MediaStreamTrack::new_audio("a1"));
        peer.close();
        assert_eq!(peer.connection_state, ConnectionState::Closed);
        assert_eq!(peer.data_channels[0].ready_state, DataChannelState::Closed);
        assert_eq!(peer.local_tracks[0].ready_state, TrackState::Ended);
    }

    #[test]
    fn ice_server_constructors() {
        let stun = IceServer::stun("stun:stun.example.com");
        assert_eq!(stun.urls, vec!["stun:stun.example.com"]);
        assert!(stun.username.is_none());

        let turn = IceServer::turn("turn:turn.example.com", "user", "pass");
        assert_eq!(turn.username.as_deref(), Some("user"));
    }
}
