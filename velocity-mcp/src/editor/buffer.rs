use std::path::PathBuf;

/// A simple in-memory document.
#[derive(Default, Debug, Clone)]
pub struct EditorBuffer {
    pub path: Option<PathBuf>,
    pub content: String,
}

impl EditorBuffer {
    pub fn new(path: Option<PathBuf>, content: String) -> Self {
        Self { path, content }
    }

    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "untitled".to_string())
    }

    pub fn update_content(&mut self, content: String) {
        self.content = content;
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn content_mut(&mut self) -> &mut String {
        &mut self.content
    }

    pub fn load_text(&mut self, text: &str) {
        self.content = text.to_string();
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        if let Some(path) = &self.path {
            std::fs::write(path, &self.content)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_from_path() {
        let b = EditorBuffer::new(Some(PathBuf::from("src/main.rs")), String::new());
        assert_eq!(b.title(), "main.rs");
    }

    #[test]
    fn title_untitled() {
        let b = EditorBuffer::new(None, String::new());
        assert_eq!(b.title(), "untitled");
    }
}
