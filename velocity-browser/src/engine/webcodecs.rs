use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub frame_index: usize,
    pub width: usize,
    pub height: usize,
    pub timestamp_us: u64,
    pub is_keyframe: bool,
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
        }
    }
}

pub struct VelocityCodecsEngine {
    pub codec_name: String,
    pub ring_buffer: VelocityFrameRingBuffer,
}

impl VelocityCodecsEngine {
    pub fn new(codec_name: &str) -> Self {
        Self {
            codec_name: codec_name.to_string(),
            ring_buffer: VelocityFrameRingBuffer::new(120),
        }
    }

    pub fn decode_stream_packet(&mut self, packet_bytes: &[u8]) -> VideoFrame {
        let idx = self.ring_buffer.frames.len() + 1;
        let frame = VelocityRemotePacketStreamer::demux_packet(packet_bytes, idx);
        self.ring_buffer.push_frame(frame.clone());
        frame
    }

    pub fn export_codecs_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        vec![NdaTriple::new(
            session_id,
            253,
            &format!("codecs:{}:buffered_{}", self.codec_name, self.ring_buffer.frames.len()),
        )]
    }
}
