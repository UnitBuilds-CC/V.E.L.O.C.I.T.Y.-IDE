//! Voice-to-Task: processes speech commands into actionable IDE tasks.
//! Provides intent parsing, command mapping, and a voice command registry
//! that bridges natural language to IDE actions.
//!
//! T3a: Windows Speech API wiring — continuous recognition via System.Speech.Recognition
//! running in a background process, feeding transcriptions into the command parser.

use std::collections::HashMap;
use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// A parsed voice command with intent and parameters.
#[derive(Debug, Clone)]
pub struct VoiceCommand {
    pub raw_text: String,
    pub intent: VoiceIntent,
    pub parameters: HashMap<String, String>,
    pub confidence: f32,
    pub timestamp: Instant,
}

/// High-level intent categories for voice commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoiceIntent {
    /// "Open file X", "Go to file X"
    OpenFile,
    /// "Search for X", "Find X"
    Search,
    /// "Run tests", "Test the project"
    RunTests,
    /// "Build the project", "Compile"
    Build,
    /// "Deploy to production"
    Deploy,
    /// "Fix the error", "Debug this"
    FixError,
    /// "Refactor X", "Rename X to Y"
    Refactor,
    /// "Create a new file", "Add function X"
    Create,
    /// "Undo", "Redo"
    UndoRedo,
    /// "Save", "Save all"
    Save,
    /// "Ask the AI to X", "Agent do X"
    AgentTask,
    /// "Show diagnostics", "Show problems"
    ShowPanel,
    /// "Navigate to line X", "Go to definition"
    Navigate,
    /// Unknown/unrecognized command.
    Unknown,
}

impl VoiceIntent {
    pub fn label(&self) -> &'static str {
        match self {
            Self::OpenFile => "Open File",
            Self::Search => "Search",
            Self::RunTests => "Run Tests",
            Self::Build => "Build",
            Self::Deploy => "Deploy",
            Self::FixError => "Fix Error",
            Self::Refactor => "Refactor",
            Self::Create => "Create",
            Self::UndoRedo => "Undo/Redo",
            Self::Save => "Save",
            Self::AgentTask => "Agent Task",
            Self::ShowPanel => "Show Panel",
            Self::Navigate => "Navigate",
            Self::Unknown => "Unknown",
        }
    }
}

/// Voice command registry: maps trigger phrases to intents.
#[derive(Debug, Clone)]
pub struct VoiceCommandRegistry {
    patterns: Vec<CommandPattern>,
}

#[derive(Debug, Clone)]
struct CommandPattern {
    triggers: Vec<String>,
    intent: VoiceIntent,
    param_extractor: ParamExtractor,
}

#[derive(Debug, Clone, Copy)]
enum ParamExtractor {
    /// Everything after the trigger phrase becomes the "target" parameter.
    RestAsTarget,
    /// No parameters extracted.
    None,
    /// First word after trigger is "from", second is "to" for rename.
    RenamePattern,
}

impl Default for VoiceCommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceCommandRegistry {
    pub fn new() -> Self {
        let patterns = vec![
            CommandPattern {
                triggers: vec!["open file".into(), "go to file".into(), "open".into()],
                intent: VoiceIntent::OpenFile,
                param_extractor: ParamExtractor::RestAsTarget,
            },
            CommandPattern {
                triggers: vec!["search for".into(), "find".into(), "grep".into()],
                intent: VoiceIntent::Search,
                param_extractor: ParamExtractor::RestAsTarget,
            },
            CommandPattern {
                triggers: vec!["run tests".into(), "test".into(), "run the tests".into()],
                intent: VoiceIntent::RunTests,
                param_extractor: ParamExtractor::None,
            },
            CommandPattern {
                triggers: vec!["build".into(), "compile".into(), "cargo build".into()],
                intent: VoiceIntent::Build,
                param_extractor: ParamExtractor::None,
            },
            CommandPattern {
                triggers: vec![
                    "deploy".into(),
                    "ship it".into(),
                    "push to production".into(),
                ],
                intent: VoiceIntent::Deploy,
                param_extractor: ParamExtractor::None,
            },
            CommandPattern {
                triggers: vec!["fix".into(), "debug".into(), "fix the error".into()],
                intent: VoiceIntent::FixError,
                param_extractor: ParamExtractor::RestAsTarget,
            },
            CommandPattern {
                triggers: vec!["refactor".into(), "rename".into()],
                intent: VoiceIntent::Refactor,
                param_extractor: ParamExtractor::RenamePattern,
            },
            CommandPattern {
                triggers: vec!["create".into(), "new file".into(), "add".into()],
                intent: VoiceIntent::Create,
                param_extractor: ParamExtractor::RestAsTarget,
            },
            CommandPattern {
                triggers: vec!["undo".into()],
                intent: VoiceIntent::UndoRedo,
                param_extractor: ParamExtractor::None,
            },
            CommandPattern {
                triggers: vec!["redo".into()],
                intent: VoiceIntent::UndoRedo,
                param_extractor: ParamExtractor::None,
            },
            CommandPattern {
                triggers: vec!["save".into(), "save all".into()],
                intent: VoiceIntent::Save,
                param_extractor: ParamExtractor::None,
            },
            CommandPattern {
                triggers: vec![
                    "ask the agent".into(),
                    "agent".into(),
                    "ai".into(),
                    "hey velocity".into(),
                ],
                intent: VoiceIntent::AgentTask,
                param_extractor: ParamExtractor::RestAsTarget,
            },
            CommandPattern {
                triggers: vec![
                    "show diagnostics".into(),
                    "show problems".into(),
                    "show terminal".into(),
                    "show panel".into(),
                ],
                intent: VoiceIntent::ShowPanel,
                param_extractor: ParamExtractor::RestAsTarget,
            },
            CommandPattern {
                triggers: vec![
                    "go to line".into(),
                    "navigate to".into(),
                    "go to definition".into(),
                ],
                intent: VoiceIntent::Navigate,
                param_extractor: ParamExtractor::RestAsTarget,
            },
        ];

        Self { patterns }
    }

    /// Parse a transcribed voice text into a structured command.
    pub fn parse(&self, text: &str) -> VoiceCommand {
        let lower = text.to_lowercase();
        let trimmed = lower.trim();

        for pattern in &self.patterns {
            for trigger in &pattern.triggers {
                if trimmed.starts_with(trigger.as_str()) {
                    let rest = trimmed[trigger.len()..].trim().to_string();
                    let parameters = extract_params(&rest, pattern.param_extractor);
                    return VoiceCommand {
                        raw_text: text.to_string(),
                        intent: pattern.intent,
                        parameters,
                        confidence: 0.9,
                        timestamp: Instant::now(),
                    };
                }
            }
        }

        // Fuzzy matching: check if any trigger word appears in the text
        for pattern in &self.patterns {
            for trigger in &pattern.triggers {
                let words: Vec<&str> = trigger.split_whitespace().collect();
                if words.len() == 1 && trimmed.contains(words[0]) {
                    let rest = trimmed.replace(words[0], "").trim().to_string();
                    let parameters = extract_params(&rest, pattern.param_extractor);
                    return VoiceCommand {
                        raw_text: text.to_string(),
                        intent: pattern.intent,
                        parameters,
                        confidence: 0.6,
                        timestamp: Instant::now(),
                    };
                }
            }
        }

        VoiceCommand {
            raw_text: text.to_string(),
            intent: VoiceIntent::Unknown,
            parameters: HashMap::new(),
            confidence: 0.0,
            timestamp: Instant::now(),
        }
    }
}

fn extract_params(rest: &str, extractor: ParamExtractor) -> HashMap<String, String> {
    let mut params = HashMap::new();
    match extractor {
        ParamExtractor::RestAsTarget => {
            if !rest.is_empty() {
                params.insert("target".into(), rest.to_string());
            }
        }
        ParamExtractor::RenamePattern => {
            // "X to Y" pattern
            if let Some(idx) = rest.find(" to ") {
                params.insert("from".into(), rest[..idx].trim().to_string());
                params.insert("to".into(), rest[idx + 4..].trim().to_string());
            } else if !rest.is_empty() {
                params.insert("target".into(), rest.to_string());
            }
        }
        ParamExtractor::None => {}
    }
    params
}

/// Voice input state for the IDE.
#[derive(Debug, Default)]
pub struct VoiceInputState {
    pub registry: VoiceCommandRegistry,
    pub listening: bool,
    pub last_transcription: String,
    pub last_command: Option<VoiceCommand>,
    pub command_history: Vec<VoiceCommand>,
    pub total_commands: usize,
    pub successful_commands: usize,
}

impl VoiceInputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a transcribed text into a command.
    pub fn process_transcription(&mut self, text: &str) -> &VoiceCommand {
        self.last_transcription = text.to_string();
        let command = self.registry.parse(text);
        self.total_commands += 1;
        if command.intent != VoiceIntent::Unknown {
            self.successful_commands += 1;
        }
        self.command_history.push(command.clone());
        if self.command_history.len() > 50 {
            self.command_history.remove(0);
        }
        self.last_command = Some(command);
        self.last_command.as_ref().unwrap()
    }

    /// Recognition accuracy percentage.
    pub fn accuracy(&self) -> f32 {
        if self.total_commands == 0 {
            0.0
        } else {
            (self.successful_commands as f32 / self.total_commands as f32) * 100.0
        }
    }

    /// Toggle listening state.
    pub fn toggle_listening(&mut self) {
        self.listening = !self.listening;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// T3a: Windows Speech API Recognition Engine
// ═══════════════════════════════════════════════════════════════════════════

/// Background speech recognition engine using Windows System.Speech.Recognition.
/// Spawns a PowerShell process that continuously recognizes speech and emits
/// transcriptions line-by-line to stdout, which we read via a background thread.
pub struct VoiceRecognitionEngine {
    running: Arc<AtomicBool>,
    /// Channel receiver for recognized phrases from the background reader.
    phrase_rx: crossbeam_channel::Receiver<String>,
    /// Handle to the reader thread.
    reader_handle: Option<std::thread::JoinHandle<()>>,
    /// The child process handle (PowerShell running SAPI).
    child: Option<std::process::Child>,
}

impl VoiceRecognitionEngine {
    /// Create a new engine (does not start recognition).
    pub fn new() -> Self {
        let (_tx, rx) = crossbeam_channel::unbounded();
        Self {
            running: Arc::new(AtomicBool::new(false)),
            phrase_rx: rx,
            reader_handle: None,
            child: None,
        }
    }

    /// Start continuous speech recognition.
    /// Spawns a PowerShell process using System.Speech.Recognition for dictation.
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(()); // Already running
        }

        let (tx, rx) = crossbeam_channel::unbounded::<String>();
        self.phrase_rx = rx;

        // PowerShell script that uses System.Speech.Recognition for continuous dictation
        let ps_script = r#"
Add-Type -AssemblyName System.Speech
$recognizer = New-Object System.Speech.Recognition.SpeechRecognitionEngine
$recognizer.SetInputToDefaultAudioDevice()
$grammar = New-Object System.Speech.Recognition.DictationGrammar
$recognizer.LoadGrammar($grammar)
$recognizer.SpeechRecognized += { param($s, $e) Write-Output $e.Result.Text }
$recognizer.RecognizeAsync([System.Speech.Recognition.RecognizeMode]::Multiple)
while ($true) { Start-Sleep -Milliseconds 100 }
"#;

        let child = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to start speech recognizer: {}", e))?;

        self.child = Some(child);
        self.running.store(true, Ordering::SeqCst);

        // Spawn reader thread that reads stdout lines from the PowerShell process
        let running_flag = self.running.clone();
        if let Some(ref mut child) = self.child {
            if let Some(stdout) = child.stdout.take() {
                let handle = std::thread::spawn(move || {
                    let reader = std::io::BufReader::new(stdout);
                    for line in reader.lines() {
                        if !running_flag.load(Ordering::SeqCst) {
                            break;
                        }
                        if let Ok(text) = line {
                            let trimmed = text.trim().to_string();
                            if !trimmed.is_empty() {
                                let _ = tx.send(trimmed);
                            }
                        }
                    }
                });
                self.reader_handle = Some(handle);
            }
        }

        Ok(())
    }

    /// Stop speech recognition and clean up.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        if let Some(handle) = self.reader_handle.take() {
            let _ = handle.join();
        }
    }

    /// Check if recognition is active.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Drain all pending recognized phrases (non-blocking).
    pub fn drain_phrases(&self) -> Vec<String> {
        let mut phrases = Vec::new();
        while let Ok(phrase) = self.phrase_rx.try_recv() {
            phrases.push(phrase);
        }
        phrases
    }

    /// Poll for a single recognized phrase (non-blocking).
    pub fn try_recv_phrase(&self) -> Option<String> {
        self.phrase_rx.try_recv().ok()
    }
}

impl Default for VoiceRecognitionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for VoiceRecognitionEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_open_file_command() {
        let registry = VoiceCommandRegistry::new();
        let cmd = registry.parse("open file main.rs");
        assert_eq!(cmd.intent, VoiceIntent::OpenFile);
        assert_eq!(cmd.parameters.get("target"), Some(&"main.rs".to_string()));
        assert!(cmd.confidence > 0.8);
    }

    #[test]
    fn parse_search_command() {
        let registry = VoiceCommandRegistry::new();
        let cmd = registry.parse("search for authentication handler");
        assert_eq!(cmd.intent, VoiceIntent::Search);
        assert_eq!(
            cmd.parameters.get("target"),
            Some(&"authentication handler".to_string())
        );
    }

    #[test]
    fn parse_run_tests() {
        let registry = VoiceCommandRegistry::new();
        let cmd = registry.parse("run tests");
        assert_eq!(cmd.intent, VoiceIntent::RunTests);
    }

    #[test]
    fn parse_rename_refactor() {
        let registry = VoiceCommandRegistry::new();
        let cmd = registry.parse("rename foo to bar");
        assert_eq!(cmd.intent, VoiceIntent::Refactor);
        assert_eq!(cmd.parameters.get("from"), Some(&"foo".to_string()));
        assert_eq!(cmd.parameters.get("to"), Some(&"bar".to_string()));
    }

    #[test]
    fn parse_agent_task() {
        let registry = VoiceCommandRegistry::new();
        let cmd = registry.parse("hey velocity fix all the compiler warnings");
        assert_eq!(cmd.intent, VoiceIntent::AgentTask);
        assert!(cmd.parameters.get("target").unwrap().contains("fix all"));
    }

    #[test]
    fn parse_unknown_command() {
        let registry = VoiceCommandRegistry::new();
        let cmd = registry.parse("what is the meaning of life");
        assert_eq!(cmd.intent, VoiceIntent::Unknown);
        assert_eq!(cmd.confidence, 0.0);
    }

    #[test]
    fn voice_input_state_tracking() {
        let mut state = VoiceInputState::new();
        state.process_transcription("open file test.rs");
        state.process_transcription("gibberish nonsense");

        assert_eq!(state.total_commands, 2);
        assert_eq!(state.successful_commands, 1);
        assert!((state.accuracy() - 50.0).abs() < 0.01);
    }

    #[test]
    fn fuzzy_match_single_word() {
        let registry = VoiceCommandRegistry::new();
        let cmd = registry.parse("please save my work");
        assert_eq!(cmd.intent, VoiceIntent::Save);
        assert!(cmd.confidence < 0.9); // Lower confidence for fuzzy
    }
}
