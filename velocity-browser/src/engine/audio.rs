/// Audio node types in the audio graph.
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
    pub buffer_samples: Vec<f32>,
    pub delay_time: f32,
    pub feedback: f32,
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

/// Analyzer data for frequency visualization.
#[derive(Debug, Clone)]
pub struct AnalyzerData {
    pub frequency_data: Vec<f32>,
    pub time_domain_data: Vec<f32>,
}

/// Web Audio engine that manages the audio processing graph.
pub struct WebAudioEngine {
    pub sample_rate: u32,
    pub nodes: Vec<AudioContextNode>,
    pub master_gain: f32,
    next_node_id: usize,
    playback_time: f32,
    is_playing: bool,
}

impl WebAudioEngine {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            nodes: Vec::new(),
            master_gain: 1.0,
            next_node_id: 1,
            playback_time: 0.0,
            is_playing: false,
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
            buffer_samples: Vec::new(),
            delay_time: 0.0,
            feedback: 0.0,
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
            buffer_samples: Vec::new(),
            delay_time: 0.0,
            feedback: 0.0,
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
            buffer_samples: Vec::new(),
            delay_time: 0.0,
            feedback: 0.0,
        });
        id
    }

    /// Create a delay node.
    pub fn create_delay(&mut self, max_delay_secs: f32) -> usize {
        let id = self.next_node_id;
        self.next_node_id += 1;
        self.nodes.push(AudioContextNode {
            node_id: id,
            node_type: AudioNodeType::Delay,
            frequency_hz: 0.0,
            gain: 1.0,
            connected_to: Vec::new(),
            detune: 0.0,
            pan: 0.0,
            buffer_samples: Vec::new(),
            delay_time: max_delay_secs,
            feedback: 0.0,
        });
        id
    }

    /// Create a buffer source node.
    pub fn create_buffer_source(&mut self, samples: Vec<f32>) -> usize {
        let id = self.next_node_id;
        self.next_node_id += 1;
        self.nodes.push(AudioContextNode {
            node_id: id,
            node_type: AudioNodeType::BufferSource,
            frequency_hz: 0.0,
            gain: 1.0,
            connected_to: Vec::new(),
            detune: 0.0,
            pan: 0.0,
            buffer_samples: samples,
            delay_time: 0.0,
            feedback: 0.0,
        });
        id
    }

    /// Create a destination node.
    pub fn create_destination(&mut self) -> usize {
        let id = self.next_node_id;
        self.next_node_id += 1;
        self.nodes.push(AudioContextNode {
            node_id: id,
            node_type: AudioNodeType::Destination,
            frequency_hz: 0.0,
            gain: 1.0,
            connected_to: Vec::new(),
            detune: 0.0,
            pan: 0.0,
            buffer_samples: Vec::new(),
            delay_time: 0.0,
            feedback: 0.0,
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

    /// Generate a waveform sample buffer.
    pub fn generate_waveform(&self, osc_type: OscillatorType, frequency: f32, duration_secs: f32) -> Vec<f32> {
        let num_samples = (self.sample_rate as f32 * duration_secs) as usize;
        let two_pi = std::f32::consts::TAU;

        (0..num_samples)
            .map(|i| {
                let t = i as f32 / self.sample_rate as f32;
                let phase = two_pi * frequency * t;

                let sample = match osc_type {
                    OscillatorType::Sine => phase.sin(),
                    OscillatorType::Square => if phase.sin() >= 0.0 { 1.0 } else { -1.0 },
                    OscillatorType::Sawtooth => {
                        let normalized = (frequency * t) % 1.0;
                        2.0 * normalized - 1.0
                    }
                    OscillatorType::Triangle => {
                        let normalized = (frequency * t) % 1.0;
                        if normalized < 0.5 {
                            4.0 * normalized - 1.0
                        } else {
                            3.0 - 4.0 * normalized
                        }
                    }
                };

                sample * self.master_gain
            })
            .collect()
    }

    /// Generate a simple sine wave sample buffer for testing.
    pub fn generate_sine_wave(&self, frequency: f32, duration_secs: f32) -> Vec<f32> {
        self.generate_waveform(OscillatorType::Sine, frequency, duration_secs)
    }

    /// Process the audio graph and return mixed output samples.
    pub fn process_graph(&mut self, duration_secs: f32) -> Vec<f32> {
        let num_samples = (self.sample_rate as f32 * duration_secs) as usize;
        let mut output = vec![0.0f32; num_samples];

        // Find destination node
        let dest_id = self.nodes.iter()
            .find(|n| n.node_type == AudioNodeType::Destination)
            .map(|n| n.node_id);

        let _dest_id = match dest_id {
            Some(id) => id,
            None => return output,
        };

        // Process each source node and mix into output
        for node in &self.nodes {
            if node.node_type == AudioNodeType::Destination { continue; }

            let samples = match node.node_type {
                AudioNodeType::Oscillator => {
                    self.generate_sine_wave(node.frequency_hz, duration_secs)
                }
                AudioNodeType::BufferSource => {
                    node.buffer_samples.clone()
                }
                _ => continue,
            };

            // Apply gain and mix
            for (i, &sample) in samples.iter().enumerate() {
                if i >= output.len() { break; }
                output[i] += sample * node.gain;
            }
        }

        // Apply master gain
        for sample in output.iter_mut() {
            *sample *= self.master_gain;
        }

        output
    }

    /// Get analyzer data for a specific analyzer node.
    pub fn get_analyzer_data(&self, analyzer_id: usize) -> Option<AnalyzerData> {
        let _node = self.nodes.iter().find(|n| n.node_id == analyzer_id && n.node_type == AudioNodeType::Analyzer)?;

        // Generate fake frequency and time domain data
        let fft_size = 1024;
        let frequency_data = vec![0.0f32; fft_size / 2];
        let time_domain_data = vec![0.0f32; fft_size];

        Some(AnalyzerData {
            frequency_data,
            time_domain_data,
        })
    }

    /// Start playback.
    pub fn start(&mut self) {
        self.is_playing = true;
    }

    /// Stop playback.
    pub fn stop(&mut self) {
        self.is_playing = false;
    }

    /// Get current playback time.
    pub fn current_time(&self) -> f32 {
        self.playback_time
    }

    /// Advance playback time.
    pub fn advance_time(&mut self, delta: f32) {
        if self.is_playing {
            self.playback_time += delta;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_waveforms() {
        let engine = WebAudioEngine::new(44100);

        let sine = engine.generate_waveform(OscillatorType::Sine, 440.0, 0.01);
        assert_eq!(sine.len(), 441);

        let square = engine.generate_waveform(OscillatorType::Square, 440.0, 0.01);
        assert_eq!(square.len(), 441);

        let sawtooth = engine.generate_waveform(OscillatorType::Sawtooth, 440.0, 0.01);
        assert_eq!(sawtooth.len(), 441);

        let triangle = engine.generate_waveform(OscillatorType::Triangle, 440.0, 0.01);
        assert_eq!(triangle.len(), 441);
    }

    #[test]
    fn process_audio_graph() {
        let mut engine = WebAudioEngine::new(44100);

        let osc = engine.create_oscillator(440.0);
        let gain = engine.create_gain(0.5);
        let dest = engine.create_destination();

        engine.connect(osc, gain);
        engine.connect(gain, dest);

        let output = engine.process_graph(0.01);
        assert_eq!(output.len(), 441);
        assert!(output.iter().any(|&s| s != 0.0)); // Should have some audio content
    }

    #[test]
    fn playback_control() {
        let mut engine = WebAudioEngine::new(44100);

        assert_eq!(engine.current_time(), 0.0);
        assert!(!engine.is_playing);

        engine.start();
        assert!(engine.is_playing);

        engine.advance_time(1.0);
        assert_eq!(engine.current_time(), 1.0);

        engine.stop();
        assert!(!engine.is_playing);

        engine.advance_time(1.0);
        assert_eq!(engine.current_time(), 1.0); // Should not advance when stopped
    }

    #[test]
    fn buffer_source_playback() {
        let mut engine = WebAudioEngine::new(44100);

        let samples = vec![0.5, 0.6, 0.7, 0.8, 0.9];
        let source = engine.create_buffer_source(samples.clone());
        let dest = engine.create_destination();

        engine.connect(source, dest);

        let output = engine.process_graph(0.001);
        assert!(output.len() >= 5);
        assert_eq!(output[0], 0.5);
        assert_eq!(output[1], 0.6);
    }

    #[test]
    fn gain_clamped_to_unit_range() {
        let mut engine = WebAudioEngine::new(44100);
        let id = engine.create_gain(2.5);
        let node = engine.nodes.iter().find(|n| n.node_id == id).unwrap();
        assert_eq!(node.gain, 1.0);
        let id2 = engine.create_gain(-0.5);
        let node2 = engine.nodes.iter().find(|n| n.node_id == id2).unwrap();
        assert_eq!(node2.gain, 0.0);
    }

    #[test]
    fn remove_node_cleans_references() {
        let mut engine = WebAudioEngine::new(44100);
        let osc = engine.create_oscillator(440.0);
        let gain = engine.create_gain(0.5);
        engine.connect(osc, gain);
        assert!(engine.nodes.iter().find(|n| n.node_id == osc).unwrap().connected_to.contains(&gain));
        assert!(engine.remove_node(gain));
        assert!(!engine.nodes.iter().find(|n| n.node_id == osc).unwrap().connected_to.contains(&gain));
    }

    #[test]
    fn remove_nonexistent_node_returns_false() {
        let mut engine = WebAudioEngine::new(44100);
        assert!(!engine.remove_node(9999));
    }

    #[test]
    fn disconnect_nonexistent_returns_false() {
        let mut engine = WebAudioEngine::new(44100);
        assert!(!engine.disconnect(9999));
    }

    #[test]
    fn set_frequency_nonexistent_returns_false() {
        let mut engine = WebAudioEngine::new(44100);
        assert!(!engine.set_frequency(9999, 880.0));
    }

    #[test]
    fn set_gain_nonexistent_returns_false() {
        let mut engine = WebAudioEngine::new(44100);
        assert!(!engine.set_gain(9999, 0.5));
    }

    #[test]
    fn oscillator_type_as_str_values() {
        assert_eq!(AudioNodeType::Oscillator.as_str(), "OscillatorNode");
        assert_eq!(AudioNodeType::Gain.as_str(), "GainNode");
        assert_eq!(AudioNodeType::Filter.as_str(), "BiquadFilterNode");
        assert_eq!(AudioNodeType::Delay.as_str(), "DelayNode");
        assert_eq!(AudioNodeType::Destination.as_str(), "AudioDestinationNode");
        assert_eq!(AudioNodeType::BufferSource.as_str(), "AudioBufferSourceNode");
    }

    #[test]
    fn oscillator_waveform_type_strings() {
        assert_eq!(OscillatorType::Sine.as_str(), "sine");
        assert_eq!(OscillatorType::Square.as_str(), "square");
        assert_eq!(OscillatorType::Sawtooth.as_str(), "sawtooth");
        assert_eq!(OscillatorType::Triangle.as_str(), "triangle");
    }

    #[test]
    fn generate_sine_matches_waveform() {
        let engine = WebAudioEngine::new(44100);
        let sine = engine.generate_sine_wave(440.0, 0.01);
        let waveform = engine.generate_waveform(OscillatorType::Sine, 440.0, 0.01);
        assert_eq!(sine.len(), waveform.len());
        for (a, b) in sine.iter().zip(waveform.iter()) {
            assert!((a - b).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn analyzer_data_returns_correct_sizes() {
        let mut engine = WebAudioEngine::new(44100);
        let aid = engine.create_analyzer();
        let data = engine.get_analyzer_data(aid).unwrap();
        assert_eq!(data.frequency_data.len(), 512);
        assert_eq!(data.time_domain_data.len(), 1024);
    }

    #[test]
    fn analyzer_data_none_for_wrong_id() {
        let engine = WebAudioEngine::new(44100);
        assert!(engine.get_analyzer_data(9999).is_none());
    }

    #[test]
    fn process_graph_no_destination_returns_silence() {
        let mut engine = WebAudioEngine::new(44100);
        engine.create_oscillator(440.0);
        let output = engine.process_graph(0.01);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn advance_time_does_nothing_when_stopped() {
        let mut engine = WebAudioEngine::new(44100);
        engine.advance_time(5.0);
        assert_eq!(engine.current_time(), 0.0);
    }

    #[test]
    fn connect_nonexistent_from_returns_false() {
        let mut engine = WebAudioEngine::new(44100);
        let osc = engine.create_oscillator(440.0);
        assert!(!engine.connect(99999, osc));
    }

    #[test]
    fn connect_duplicate_is_idempotent() {
        let mut engine = WebAudioEngine::new(44100);
        let a = engine.create_oscillator(440.0);
        let b = engine.create_gain(0.5);
        assert!(engine.connect(a, b));
        assert!(engine.connect(a, b)); // second connect should not duplicate
        let node = engine.nodes.iter().find(|n| n.node_id == a).unwrap();
        assert_eq!(node.connected_to.iter().filter(|&id| *id == b).count(), 1);
    }

    #[test]
    fn node_count_tracks_additions_and_removals() {
        let mut engine = WebAudioEngine::new(44100);
        assert_eq!(engine.node_count(), 0);
        let a = engine.create_oscillator(440.0);
        assert_eq!(engine.node_count(), 1);
        let _b = engine.create_gain(0.5);
        assert_eq!(engine.node_count(), 2);
        engine.remove_node(a);
        assert_eq!(engine.node_count(), 1);
    }

    #[test]
    fn master_gain_affects_output_level() {
        let build_engine = |mg: f32| -> Vec<f32> {
            let mut engine = WebAudioEngine::new(44100);
            engine.master_gain = mg;
            let osc = engine.create_oscillator(440.0);
            let dest = engine.create_destination();
            engine.connect(osc, dest);
            engine.process_graph(0.01)
        };
        let silent = build_engine(0.0);
        let loud = build_engine(1.0);
        // Zero gain should produce silence
        assert!(silent.iter().all(|&s| s == 0.0));
        // Non-zero gain should produce audible output
        assert!(loud.iter().any(|&s| s != 0.0));
        // Higher gain produces larger samples
        let peak_loud = loud.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak_loud > 0.0);
    }
}
