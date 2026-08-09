use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScreencastFrame {
    pub frame_idx: u32,
    pub timestamp_ms: u64,
    pub width: u32,
    pub height: u32,
    pub element_count: usize,
    pub frame_hash: u64,
}

#[derive(Debug, Clone)]
pub struct ScreencastRecorder {
    pub session_id: String,
    pub frames: Vec<ScreencastFrame>,
    pub recording: bool,
}

impl ScreencastRecorder {
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            frames: Vec::new(),
            recording: true,
        }
    }

    pub fn capture_frame(
        &mut self,
        width: u32,
        height: u32,
        element_count: usize,
    ) -> ScreencastFrame {
        let frame_idx = self.frames.len() as u32;
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        use std::hash::Hash;
        use std::hash::Hasher;
        (self.session_id.as_str(), frame_idx, element_count).hash(&mut hasher);
        let frame_hash = hasher.finish();

        let frame = ScreencastFrame {
            frame_idx,
            timestamp_ms,
            width,
            height,
            element_count,
            frame_hash,
        };
        if self.recording {
            self.frames.push(frame.clone());
        }
        frame
    }

    pub fn save_metadata(&self, workspace_root: &Path) -> Result<PathBuf, String> {
        let dir = workspace_root
            .join(".velocity")
            .join("browser_artifacts")
            .join("screencasts");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}_screencast.json", self.session_id));
        let json = serde_json::to_string_pretty(&self.frames)
            .map_err(|e| format!("failed to serialize screencast metadata: {e}"))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("failed to write screencast metadata: {e}"))?;
        Ok(path)
    }
}
