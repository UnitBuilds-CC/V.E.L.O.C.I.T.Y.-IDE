#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub frame_index: usize,
    pub width: usize,
    pub height: usize,
    pub timestamp_us: u64,
}

pub struct WebCodecsDecoder {
    pub codec_name: String,
    pub decoded_frames: Vec<VideoFrame>,
}

impl WebCodecsDecoder {
    pub fn new(codec_name: &str) -> Self {
        Self {
            codec_name: codec_name.to_string(),
            decoded_frames: Vec::new(),
        }
    }

    pub fn decode_chunk(&mut self, _chunk: &[u8], width: usize, height: usize) -> VideoFrame {
        let idx = self.decoded_frames.len() + 1;
        let frame = VideoFrame {
            frame_index: idx,
            width,
            height,
            timestamp_us: idx as u64 * 33333,
        };
        self.decoded_frames.push(frame.clone());
        frame
    }
}
