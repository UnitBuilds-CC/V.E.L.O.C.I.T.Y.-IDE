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

    /// Encode an audio frame into a packet (simplified: wraps PCM samples with header).
    pub fn encode_audio_packet(&mut self, sample_rate: u32, channels: u16, timestamp_us: u64, pcm_data: &[u8]) -> EncodedPacket {
        let mut packet_data = Vec::with_capacity(8 + pcm_data.len());
        // Header: sample_rate(2) + channels(1) + flags(1) + reserved(4)
        packet_data.extend_from_slice(&(sample_rate as u16).to_le_bytes());
        packet_data.push(channels as u8);
        packet_data.push(0x01); // flags: raw PCM marker
        packet_data.extend_from_slice(&[0u8; 4]);
        packet_data.extend_from_slice(pcm_data);
        let packet = EncodedPacket {
            data: packet_data,
            timestamp_us,
            is_keyframe: true, // audio packets are always independently decodable
            codec: self.codec,
        };
        self.encoded_packets.push(packet.clone());
        packet
    }

    /// Flush all buffered encoded packets, returning them and clearing the buffer.
    pub fn flush(&mut self) -> Vec<EncodedPacket> {
        let packets = std::mem::take(&mut self.encoded_packets);
        self.encoding_active = false;
        packets
    }

    /// Get encoding stats.
    pub fn stats(&self) -> CodecStats {
        CodecStats {
            video_frames_decoded: self.ring_buffer.frames.len(),
            audio_frames_decoded: self.audio_ring.frames.len(),
            packets_encoded: self.encoded_packets.len(),
            frames_since_keyframe: self.ring_buffer.frames_since_keyframe(),
            audio_packets_encoded: self.encoded_packets.iter().filter(|p| p.codec.is_audio()).count(),
            video_packets_encoded: self.encoded_packets.iter().filter(|p| p.codec.is_video()).count(),
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
    pub video_packets_encoded: usize,
    pub audio_packets_encoded: usize,
    pub frames_since_keyframe: usize,
    pub codec: CodecKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_audio_packet() {
        let mut engine = VelocityCodecsEngine::new("opus");
        let pcm_data = vec![0u8; 960]; // 20ms @ 48kHz, 16-bit mono
        let packet = engine.encode_audio_packet(48000, 1, 0, &pcm_data);
        assert_eq!(packet.data.len(), 8 + 960);
        assert!(packet.is_keyframe);
        assert_eq!(packet.codec, CodecKind::Opus);
        assert_eq!(engine.encoded_packets.len(), 1);
    }

    #[test]
    fn test_flush_packets() {
        let mut engine = VelocityCodecsEngine::new("h264");
        engine.encoding_active = true;
        engine.encode_video_frame(1920, 1080, 0, &[0u8; 100]);
        engine.encode_video_frame(1920, 1080, 16666, &[0u8; 100]);
        assert_eq!(engine.encoded_packets.len(), 2);
        let packets = engine.flush();
        assert_eq!(packets.len(), 2);
        assert_eq!(engine.encoded_packets.len(), 0);
        assert!(!engine.encoding_active);
    }

    #[test]
    fn test_stats_breakdown() {
        let mut engine = VelocityCodecsEngine::new("h264");
        engine.encode_video_frame(1920, 1080, 0, &[0u8; 100]);
        // Switch to audio codec for audio packet
        engine.codec = CodecKind::Opus;
        engine.encode_audio_packet(48000, 2, 0, &[0u8; 1920]);
        let stats = engine.stats();
        assert_eq!(stats.video_packets_encoded, 1);
        assert_eq!(stats.audio_packets_encoded, 1);
        assert_eq!(stats.packets_encoded, 2);
    }

    #[test]
    fn test_codec_from_name_variants() {
        assert_eq!(CodecKind::from_name("av1"), CodecKind::AV1);
        assert_eq!(CodecKind::from_name("HEVC"), CodecKind::H265);
        assert_eq!(CodecKind::from_name("h264"), CodecKind::H264);
        assert_eq!(CodecKind::from_name("avc1"), CodecKind::H264);
        assert_eq!(CodecKind::from_name("vp9"), CodecKind::VP9);
        assert_eq!(CodecKind::from_name("vp8"), CodecKind::VP8);
        assert_eq!(CodecKind::from_name("opus"), CodecKind::Opus);
        assert_eq!(CodecKind::from_name("aac"), CodecKind::AAC);
        assert_eq!(CodecKind::from_name("unknown_codec"), CodecKind::Unknown);
    }

    #[test]
    fn test_codec_is_video() {
        assert!(CodecKind::H264.is_video());
        assert!(CodecKind::H265.is_video());
        assert!(CodecKind::VP8.is_video());
        assert!(CodecKind::VP9.is_video());
        assert!(CodecKind::AV1.is_video());
        assert!(!CodecKind::Opus.is_video());
        assert!(!CodecKind::AAC.is_video());
    }

    #[test]
    fn test_codec_is_audio() {
        assert!(CodecKind::Opus.is_audio());
        assert!(CodecKind::AAC.is_audio());
        assert!(!CodecKind::H264.is_audio());
        assert!(!CodecKind::VP9.is_audio());
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let mut buf = VelocityFrameRingBuffer::new(3);
        for i in 0..5 {
            buf.push_frame(VideoFrame {
                frame_index: i,
                width: 640,
                height: 480,
                timestamp_us: i as u64 * 16666,
                is_keyframe: i == 0,
                codec: CodecKind::H264,
            });
        }
        // Only last 3 frames remain
        assert_eq!(buf.frames.len(), 3);
        assert_eq!(buf.frames[0].frame_index, 2);
    }

    #[test]
    fn test_frames_since_keyframe() {
        let mut buf = VelocityFrameRingBuffer::new(10);
        buf.push_frame(VideoFrame {
            frame_index: 0, width: 640, height: 480, timestamp_us: 0,
            is_keyframe: true, codec: CodecKind::H264,
        });
        buf.push_frame(VideoFrame {
            frame_index: 1, width: 640, height: 480, timestamp_us: 16666,
            is_keyframe: false, codec: CodecKind::H264,
        });
        buf.push_frame(VideoFrame {
            frame_index: 2, width: 640, height: 480, timestamp_us: 33332,
            is_keyframe: false, codec: CodecKind::H264,
        });
        assert_eq!(buf.frames_since_keyframe(), 2);
    }

    #[test]
    fn test_frames_since_keyframe_no_keyframe() {
        let mut buf = VelocityFrameRingBuffer::new(10);
        buf.push_frame(VideoFrame {
            frame_index: 0, width: 640, height: 480, timestamp_us: 0,
            is_keyframe: false, codec: CodecKind::H264,
        });
        assert_eq!(buf.frames_since_keyframe(), 1); // all frames since no keyframe
    }

    #[test]
    fn test_export_codecs_nda() {
        let mut engine = VelocityCodecsEngine::new("vp9");
        engine.decode_stream_packet(&[0x00]);
        let triples = engine.export_codecs_nda("sess1");
        assert_eq!(triples.len(), 1);
        assert_eq!(triples[0].predicate_id, 253);
        assert!(triples[0].object_hash != 0);
    }
}

