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
    pub save_path: String,
    pub is_complete: bool,
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
            save_path: save_path.to_string(),
            is_complete: true,
        });
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
