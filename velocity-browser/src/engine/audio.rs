/// Web Audio API node types.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioNodeType {
    Oscillator,
    Gain,
    Filter,
    Delay,
    Panner,
    Analyzer,
    Destination,
    BufferSource,
    ChannelMerger,
    ChannelSplitter,
}

impl AudioNodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Oscillator => "OscillatorNode",
            Self::Gain => "GainNode",
            Self::Filter => "BiquadFilterNode",
            Self::Delay => "DelayNode",
            Self::Panner => "PannerNode",
            Self::Analyzer => "AnalyserNode",
            Self::Destination => "AudioDestinationNode",
            Self::BufferSource => "AudioBufferSourceNode",
            Self::ChannelMerger => "ChannelMergerNode",
            Self::ChannelSplitter => "ChannelSplitterNode",
        }
    }
}

/// An audio processing node in the audio graph.
#[derive(Debug, Clone)]
pub struct AudioContextNode {
    pub node_id: usize,
    pub node_type: AudioNodeType,
    pub frequency_hz: f32,
    pub gain: f32,
    pub connected_to: Vec<usize>,
    pub detune: f32,
    pub pan: f32,
}

/// Oscillator waveform types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OscillatorType {
    Sine,
    Square,
    Sawtooth,
    Triangle,
}

impl OscillatorType {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Sine => "sine", Self::Square => "square", Self::Sawtooth => "sawtooth", Self::Triangle => "triangle" }
    }
}

/// Web Audio engine that manages the audio processing graph.
pub struct WebAudioEngine {
    pub sample_rate: u32,
    pub nodes: Vec<AudioContextNode>,
    pub master_gain: f32,
    next_node_id: usize,
}

impl WebAudioEngine {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            nodes: Vec::new(),
            master_gain: 1.0,
            next_node_id: 1,
        }
    }

    /// Create an oscillator node with the given frequency.
    pub fn create_oscillator(&mut self, frequency: f32) -> usize {
        let id = self.next_node_id;
        self.next_node_id += 1;
        self.nodes.push(AudioContextNode {
            node_id: id,
            node_type: AudioNodeType::Oscillator,
            frequency_hz: frequency,
            gain: 1.0,
            connected_to: Vec::new(),
            detune: 0.0,
            pan: 0.0,
        });
        id
    }

    /// Create a gain node.
    pub fn create_gain(&mut self, gain: f32) -> usize {
        let id = self.next_node_id;
        self.next_node_id += 1;
        self.nodes.push(AudioContextNode {
            node_id: id,
            node_type: AudioNodeType::Gain,
            frequency_hz: 0.0,
            gain: gain.clamp(0.0, 1.0),
            connected_to: Vec::new(),
            detune: 0.0,
            pan: 0.0,
        });
        id
    }

    /// Create an analyzer node for frequency visualization.
    pub fn create_analyzer(&mut self) -> usize {
        let id = self.next_node_id;
        self.next_node_id += 1;
        self.nodes.push(AudioContextNode {
            node_id: id,
            node_type: AudioNodeType::Analyzer,
            frequency_hz: 0.0,
            gain: 1.0,
            connected_to: Vec::new(),
            detune: 0.0,
            pan: 0.0,
        });
        id
    }

    /// Connect two nodes: from_id -> to_id.
    pub fn connect(&mut self, from_id: usize, to_id: usize) -> bool {
        if let Some(node) = self.nodes.iter_mut().find(|n| n.node_id == from_id) {
            if !node.connected_to.contains(&to_id) {
                node.connected_to.push(to_id);
            }
            true
        } else {
            false
        }
    }

    /// Disconnect a node from all downstream connections.
    pub fn disconnect(&mut self, node_id: usize) -> bool {
        if let Some(node) = self.nodes.iter_mut().find(|n| n.node_id == node_id) {
            node.connected_to.clear();
            true
        } else {
            false
        }
    }

    /// Set the gain of a node.
    pub fn set_gain(&mut self, node_id: usize, gain: f32) -> bool {
        if let Some(node) = self.nodes.iter_mut().find(|n| n.node_id == node_id) {
            node.gain = gain.clamp(0.0, 1.0);
            true
        } else {
            false
        }
    }

    /// Set the frequency of an oscillator node.
    pub fn set_frequency(&mut self, node_id: usize, frequency: f32) -> bool {
        if let Some(node) = self.nodes.iter_mut().find(|n| n.node_id == node_id) {
            node.frequency_hz = frequency;
            true
        } else {
            false
        }
    }

    /// Remove a node from the graph.
    pub fn remove_node(&mut self, node_id: usize) -> bool {
        if let Some(pos) = self.nodes.iter().position(|n| n.node_id == node_id) {
            self.nodes.remove(pos);
            // Remove references to this node from other nodes
            for node in &mut self.nodes {
                node.connected_to.retain(|&id| id != node_id);
            }
            true
        } else {
            false
        }
    }

    /// Get the total number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Generate a simple sine wave sample buffer for testing.
    pub fn generate_sine_wave(&self, frequency: f32, duration_secs: f32) -> Vec<f32> {
        let num_samples = (self.sample_rate as f32 * duration_secs) as usize;
        let two_pi = std::f32::consts::TAU;
        (0..num_samples)
            .map(|i| {
                let t = i as f32 / self.sample_rate as f32;
                (two_pi * frequency * t).sin() * self.master_gain
            })
            .collect()
    }
}
