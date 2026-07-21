#[derive(Debug, Clone)]
pub struct AudioContextNode {
    pub node_id: usize,
    pub node_type: String,
    pub frequency_hz: f32,
    pub gain: f32,
}

pub struct WebAudioEngine {
    pub sample_rate: u32,
    pub nodes: Vec<AudioContextNode>,
}

impl WebAudioEngine {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            nodes: Vec::new(),
        }
    }

    pub fn create_oscillator(&mut self, frequency: f32) -> usize {
        let id = self.nodes.len() + 1;
        self.nodes.push(AudioContextNode {
            node_id: id,
            node_type: "OscillatorNode".to_string(),
            frequency_hz: frequency,
            gain: 1.0,
        });
        id
    }
}
