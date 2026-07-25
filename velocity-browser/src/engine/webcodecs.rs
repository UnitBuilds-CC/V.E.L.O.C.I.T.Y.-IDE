use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub frame_index: usize,
    pub width: usize,
    pub height: usize,
    pub timestamp_us: u64,
    pub is_keyframe: bool,
    pub codec: CodecKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CodecKind {
    H264,
    H265,
    VP8,
    VP9,
    AV1,
    Opus,
    AAC,
    Unknown,
}

impl CodecKind {
    pub fn from_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.contains("av1") { CodecKind::AV1 }
        else if lower.contains("h265") || lower.contains("hevc") { CodecKind::H265 }
        else if lower.contains("h264") || lower.contains("avc") { CodecKind::H264 }
        else if lower.contains("vp9") { CodecKind::VP9 }
        else if lower.contains("vp8") { CodecKind::VP8 }
        else if lower.contains("opus") { CodecKind::Opus }
        else if lower.contains("aac") { CodecKind::AAC }
        else { CodecKind::Unknown }
    }

    pub fn is_video(&self) -> bool {
        matches!(self, CodecKind::H264 | CodecKind::H265 | CodecKind::VP8 | CodecKind::VP9 | CodecKind::AV1)
    }

    pub fn is_audio(&self) -> bool {
        matches!(self, CodecKind::Opus | CodecKind::AAC)
    }
}

/// Audio frame for audio codec processing.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub frame_index: usize,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_count: usize,
    pub timestamp_us: u64,
    pub codec: CodecKind,
}

/// Encoded packet ready for muxing.
#[derive(Debug, Clone)]
pub struct EncodedPacket {
    pub data: Vec<u8>,
    pub timestamp_us: u64,
    pub is_keyframe: bool,
    pub codec: CodecKind,
}

pub struct VelocityFrameRingBuffer {
    pub capacity: usize,
    pub frames: Vec<VideoFrame>,
}

impl VelocityFrameRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            frames: Vec::with_capacity(capacity),
        }
    }

    pub fn push_frame(&mut self, frame: VideoFrame) {
        if self.frames.len() >= self.capacity {
            self.frames.remove(0);
        }
        self.frames.push(frame);
    }

    pub fn latest_keyframe_index(&self) -> Option<usize> {
        self.frames.iter().rposition(|f| f.is_keyframe)
    }

    pub fn frames_since_keyframe(&self) -> usize {
        self.latest_keyframe_index()
            .map(|idx| self.frames.len() - 1 - idx)
            .unwrap_or(self.frames.len())
    }
}

/// Audio frame ring buffer.
pub struct AudioFrameRingBuffer {
    pub capacity: usize,
    pub frames: Vec<AudioFrame>,
}

impl AudioFrameRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            frames: Vec::with_capacity(capacity),
        }
    }

    pub fn push_frame(&mut self, frame: AudioFrame) {
        if self.frames.len() >= self.capacity {
            self.frames.remove(0);
        }
        self.frames.push(frame);
    }
}

pub struct VelocityRemotePacketStreamer;

impl VelocityRemotePacketStreamer {
    pub fn demux_packet(packet_bytes: &[u8], frame_idx: usize) -> VideoFrame {
        let is_keyframe = packet_bytes.iter().any(|&b| b == 0x25 || (b & 0x1F == 5));
        VideoFrame {
            frame_index: frame_idx,
            width: 1920,
            height: 1080,
            timestamp_us: frame_idx as u64 * 16666, // 60 FPS
            is_keyframe,
            codec: CodecKind::H264,
        }
    }

    /// Demux an audio packet.
    pub fn demux_audio_packet(packet_bytes: &[u8], frame_idx: usize, codec: CodecKind) -> AudioFrame {
        AudioFrame {
            frame_index: frame_idx,
            sample_rate: 48000,
            channels: 2,
            sample_count: packet_bytes.len() / 2, // 16-bit samples
            timestamp_us: frame_idx as u64 * 20000, // 50 FPS audio
            codec,
        }
    }
}

pub struct VelocityCodecsEngine {
    pub codec_name: String,
    pub codec: CodecKind,
    pub ring_buffer: VelocityFrameRingBuffer,
    pub audio_ring: AudioFrameRingBuffer,
    pub encoded_packets: Vec<EncodedPacket>,
    pub encoding_active: bool,
}

impl VelocityCodecsEngine {
    pub fn new(codec_name: &str) -> Self {
        let codec = CodecKind::from_name(codec_name);
        Self {
            codec_name: codec_name.to_string(),
            codec,
            ring_buffer: VelocityFrameRingBuffer::new(120),
            audio_ring: AudioFrameRingBuffer::new(200),
            encoded_packets: Vec::new(),
            encoding_active: false,
        }
    }

    pub fn decode_stream_packet(&mut self, packet_bytes: &[u8]) -> VideoFrame {
        let idx = self.ring_buffer.frames.len() + 1;
        let frame = VelocityRemotePacketStreamer::demux_packet(packet_bytes, idx);
        self.ring_buffer.push_frame(frame.clone());
        frame
    }

    /// Decode an audio packet.
    pub fn decode_audio_packet(&mut self, packet_bytes: &[u8]) -> AudioFrame {
        let idx = self.audio_ring.frames.len() + 1;
        let frame = VelocityRemotePacketStreamer::demux_audio_packet(packet_bytes, idx, self.codec);
        self.audio_ring.push_frame(frame.clone());
        frame
    }

    /// Encode a video frame into a packet (simplified: wraps raw bytes with header).
    pub fn encode_video_frame(&mut self, width: usize, height: usize, timestamp_us: u64, raw_data: &[u8]) -> EncodedPacket {
        let is_keyframe = self.ring_buffer.frames_since_keyframe() >= 30; // Keyframe every 30 frames
        let mut packet_data = Vec::with_capacity(8 + raw_data.len());
        // Simple header: width(2) + height(2) + flags(1) + reserved(3)
        packet_data.extend_from_slice(&(width as u16).to_le_bytes());
        packet_data.extend_from_slice(&(height as u16).to_le_bytes());
        packet_data.push(if is_keyframe { 0x05 } else { 0x01 });
        packet_data.extend_from_slice(&[0u8; 3]);
        packet_data.extend_from_slice(raw_data);
        let packet = EncodedPacket {
            data: packet_data,
            timestamp_us,
            is_keyframe,
            codec: self.codec,
        };
        self.encoded_packets.push(packet.clone());
        packet
    }

    /// Get encoding stats.
    pub fn stats(&self) -> CodecStats {
        CodecStats {
            video_frames_decoded: self.ring_buffer.frames.len(),
            audio_frames_decoded: self.audio_ring.frames.len(),
            packets_encoded: self.encoded_packets.len(),
            frames_since_keyframe: self.ring_buffer.frames_since_keyframe(),
            codec: self.codec,
        }
    }

    pub fn export_codecs_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        vec![NdaTriple::new(
            session_id,
            253,
            &format!("codecs:{}:buffered_{}", self.codec_name, self.ring_buffer.frames.len()),
        )]
    }
}

#[derive(Debug, Clone)]
pub struct CodecStats {
    pub video_frames_decoded: usize,
    pub audio_frames_decoded: usize,
    pub packets_encoded: usize,
    pub frames_since_keyframe: usize,
    pub codec: CodecKind,
}
