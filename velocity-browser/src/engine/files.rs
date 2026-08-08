use crate::dom::DomTree;
use crate::nda::NdaTriple;
use crate::parser::html::NodeType;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct FileChooserEvent {
    pub input_id: String,
    pub accept_types: Vec<String>,
    pub is_multiple: bool,
}

#[derive(Debug, Clone)]
pub struct DownloadStreamArtifact {
    pub guid: String,
    pub url: String,
    pub file_name: String,
    pub total_bytes: usize,
    pub received_bytes: usize,
    pub save_path: String,
    pub is_complete: bool,
    pub started_at_ms: u64,
}

impl DownloadStreamArtifact {
    /// Progress as a fraction (0.0..1.0).
    pub fn progress(&self) -> f64 {
        if self.total_bytes == 0 { return 0.0; }
        self.received_bytes as f64 / self.total_bytes as f64
    }

    /// Whether the download is still in progress.
    pub fn is_in_progress(&self) -> bool {
        !self.is_complete && self.received_bytes < self.total_bytes
    }
}

pub struct FileManager {
    pub file_choosers: Vec<FileChooserEvent>,
    pub downloads: Vec<DownloadStreamArtifact>,
    pub attached_files: HashMap<String, String>, // input_id -> local_file_path
}

impl Default for FileManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FileManager {
    pub fn new() -> Self {
        Self {
            file_choosers: Vec::new(),
            downloads: Vec::new(),
            attached_files: HashMap::new(),
        }
    }

    pub fn handle_file_input_click(&mut self, tree: &DomTree, _selector: &str) -> Option<FileChooserEvent> {
        for node in &tree.nodes {
            if node.node_type == NodeType::Element && node.tag_name == "input" {
                if let Some(type_attr) = node.attributes.get("type") {
                    if type_attr == "file" {
                        let id = node.attributes.get("id").cloned().unwrap_or_else(|| format!("input_{}", node.id));
                        let accept = node.attributes.get("accept").map(|s| s.split(',').map(|t| t.trim().to_string()).collect()).unwrap_or_default();
                        let multiple = node.attributes.contains_key("multiple");

                        let event = FileChooserEvent {
                            input_id: id.clone(),
                            accept_types: accept,
                            is_multiple: multiple,
                        };
                        self.file_choosers.push(event.clone());
                        return Some(event);
                    }
                }
            }
        }
        None
    }

    pub fn attach_file(&mut self, tree: &mut DomTree, selector: &str, file_path: &str) -> Result<String, String> {
        let mut attached_id = None;
        let clean_selector = selector.trim_start_matches('#');

        for node in &mut tree.nodes {
            if node.node_type == NodeType::Element && node.tag_name == "input" {
                let node_id = node.attributes.get("id").cloned().unwrap_or_default();
                if node_id == clean_selector || selector == "input[type=\"file\"]" || selector == "input" {
                    node.attributes.insert("value".to_string(), file_path.to_string());
                    attached_id = Some(if node_id.is_empty() { format!("input_{}", node.id) } else { node_id });
                    break;
                }
            }
        }

        if let Some(id) = attached_id {
            self.attached_files.insert(id, file_path.to_string());
            Ok(format!("Attached file '{}' to selector '{}'", file_path, selector))
        } else {
            Err(format!("File input matching selector '{}' not found", selector))
        }
    }

    pub fn record_download(&mut self, guid: &str, url: &str, file_name: &str, total_bytes: usize, save_path: &str) {
        self.downloads.push(DownloadStreamArtifact {
            guid: guid.to_string(),
            url: url.to_string(),
            file_name: file_name.to_string(),
            total_bytes,
            received_bytes: 0,
            save_path: save_path.to_string(),
            is_complete: true,
            started_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        });
    }

    /// Start tracking a new in-progress download.
    pub fn start_download(&mut self, guid: &str, url: &str, file_name: &str, total_bytes: usize, save_path: &str) {
        self.downloads.push(DownloadStreamArtifact {
            guid: guid.to_string(),
            url: url.to_string(),
            file_name: file_name.to_string(),
            total_bytes,
            received_bytes: 0,
            save_path: save_path.to_string(),
            is_complete: false,
            started_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        });
    }

    /// Update progress on an in-progress download.
    pub fn update_download_progress(&mut self, guid: &str, received_bytes: usize) {
        if let Some(d) = self.downloads.iter_mut().find(|d| d.guid == guid) {
            d.received_bytes = received_bytes;
            if d.total_bytes > 0 && received_bytes >= d.total_bytes {
                d.is_complete = true;
                d.received_bytes = d.total_bytes;
            }
        }
    }

    /// Get all in-progress downloads.
    pub fn active_downloads(&self) -> Vec<&DownloadStreamArtifact> {
        self.downloads.iter().filter(|d| d.is_in_progress()).collect()
    }

    /// Find a download by guid.
    pub fn get_download(&self, guid: &str) -> Option<&DownloadStreamArtifact> {
        self.downloads.iter().find(|d| d.guid == guid)
    }

    pub fn export_files_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for (input_id, path) in &self.attached_files {
            triples.push(NdaTriple::new(input_id, 90, path));
        }
        for d in &self.downloads {
            triples.push(NdaTriple::new(&d.guid, 91, &d.url));
            triples.push(NdaTriple::new(&d.guid, 92, &d.save_path));
        }
        triples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_progress() {
        let d = DownloadStreamArtifact {
            guid: "d1".into(), url: "http://x.com/f".into(), file_name: "f.zip".into(),
            total_bytes: 1000, received_bytes: 500, save_path: "/tmp/f.zip".into(),
            is_complete: false, started_at_ms: 0,
        };
        assert!((d.progress() - 0.5).abs() < 1e-6);
        assert!(d.is_in_progress());
    }

    #[test]
    fn test_download_complete() {
        let d = DownloadStreamArtifact {
            guid: "d2".into(), url: "http://x.com/f".into(), file_name: "f.zip".into(),
            total_bytes: 100, received_bytes: 100, save_path: "/tmp/f.zip".into(),
            is_complete: true, started_at_ms: 0,
        };
        assert!(!d.is_in_progress());
        assert!((d.progress() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_start_and_update_download() {
        let mut fm = FileManager::new();
        fm.start_download("g1", "http://x.com/big", "big.zip", 2000, "/tmp/big.zip");
        assert_eq!(fm.active_downloads().len(), 1);
        fm.update_download_progress("g1", 2000);
        assert_eq!(fm.active_downloads().len(), 0);
        assert!(fm.get_download("g1").unwrap().is_complete);
    }

    #[test]
    fn test_record_download() {
        let mut fm = FileManager::new();
        fm.record_download("r1", "http://x.com/small", "small.txt", 100, "/tmp/small.txt");
        assert_eq!(fm.downloads.len(), 1);
        assert!(fm.downloads[0].is_complete);
    }

    #[test]
    fn test_export_files_nda() {
        let mut fm = FileManager::new();
        fm.attached_files.insert("input1".into(), "/path/to/file.txt".into());
        fm.record_download("dl1", "http://x.com/f", "f.txt", 100, "/tmp/f.txt");
        let triples = fm.export_files_nda();
        assert_eq!(triples.len(), 3); // 1 file + 2 download
    }

    #[test]
    fn progress_zero_total_returns_zero() {
        let d = DownloadStreamArtifact {
            guid: "z".into(), url: "u".into(), file_name: "f".into(),
            total_bytes: 0, received_bytes: 0, save_path: "/f".into(),
            is_complete: false, started_at_ms: 0,
        };
        assert!((d.progress() - 0.0).abs() < 1e-9);
        assert!(!d.is_in_progress()); // 0 < 0 is false
    }

    #[test]
    fn get_download_nonexistent_returns_none() {
        let fm = FileManager::new();
        assert!(fm.get_download("nope").is_none());
    }

    #[test]
    fn active_downloads_empty_initially() {
        let fm = FileManager::new();
        assert!(fm.active_downloads().is_empty());
    }

    #[test]
    fn default_file_manager_is_empty() {
        let fm = FileManager::default();
        assert!(fm.file_choosers.is_empty());
        assert!(fm.downloads.is_empty());
        assert!(fm.attached_files.is_empty());
    }

    #[test]
    fn update_nonexistent_guid_is_noop() {
        let mut fm = FileManager::new();
        fm.start_download("g1", "http://x.com/f", "f.zip", 1000, "/tmp/f.zip");
        fm.update_download_progress("nonexistent", 500);
        // g1 should still be at 0 bytes
        assert_eq!(fm.get_download("g1").unwrap().received_bytes, 0);
    }

    #[test]
    fn download_completes_when_received_exceeds_total() {
        let mut fm = FileManager::new();
        fm.start_download("g1", "http://x.com/f", "f.zip", 100, "/tmp/f.zip");
        fm.update_download_progress("g1", 200);
        let d = fm.get_download("g1").unwrap();
        assert!(d.is_complete);
        assert_eq!(d.received_bytes, 100); // clamped to total
    }
}
