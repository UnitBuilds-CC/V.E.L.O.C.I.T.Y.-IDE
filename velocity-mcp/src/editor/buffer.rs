use std::path::PathBuf;

#[derive(Default, Debug, Clone)]
pub struct EditorBuffer {
    pub path: Option<PathBuf>,
    pub content: String,
}

impl EditorBuffer {
    pub fn new(path: Option<PathBuf>, content: String) -> Self {
        Self { path, content }
    }

    pub fn update_content(&mut self, content: String) {
        self.content = content;
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        if let Some(path) = &self.path {
            std::fs::write(path, &self.content)?;
        }
        Ok(())
    }
}
