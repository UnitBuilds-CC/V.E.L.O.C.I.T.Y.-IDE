use std::fs;
use std::path::Path;

use crate::agent::nda::{decode_nda_text, encode_nda_text};

/// A reusable skill definition. The `body` is injected into a member's system
/// prompt when a task is routed to that member, giving the model specialized
/// domain instructions/tooling knowledge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillFile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub body: String,
}

impl SkillFile {
    pub fn new(id: &str, name: &str, description: &str, body: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            body: body.to_string(),
        }
    }
}

/// `system_tools` is a built-in capability tag, not a real skill file.
pub const BUILTIN_SKILLS: &[&str] = &["system_tools"];

pub fn is_builtin_skill(id: &str) -> bool {
    BUILTIN_SKILLS.contains(&id)
}

fn skills_dir(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root.join(".velocity").join("skills")
}

fn skill_path(workspace_root: &Path, id: &str) -> std::path::PathBuf {
    skills_dir(workspace_root).join(format!("{}.nda", id))
}

pub fn serialize_skill_nda(skill: &SkillFile) -> String {
    let lines = ["skill version 1".to_string(),
        format!("field\tid\t{}", encode_nda_text(&skill.id)),
        format!("field\tname\t{}", encode_nda_text(&skill.name)),
        format!("field\tdescription\t{}", encode_nda_text(&skill.description)),
        format!("field\tbody\t{}", encode_nda_text(&skill.body))];
    lines.join("\n") + "\n"
}

pub fn parse_skill_nda(text: &str, fallback_id: &str) -> Option<SkillFile> {
    if !text.trim_start().starts_with("skill version 1") {
        return None;
    }
    let mut skill = SkillFile {
        id: fallback_id.to_string(),
        ..Default::default()
    };
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("field\t") else {
            continue;
        };
        let parts: Vec<&str> = rest.splitn(2, '\t').collect();
        if parts.len() != 2 {
            continue;
        }
        let value = decode_nda_text(parts[1]);
        match parts[0] {
            "id" => skill.id = value,
            "name" => skill.name = value,
            "description" => skill.description = value,
            "body" => skill.body = value,
            _ => {}
        }
    }
    if skill.name.is_empty() {
        skill.name = skill.id.clone();
    }
    Some(skill)
}

/// Load a single skill file by id. Returns `None` if it does not exist.
pub fn load_skill_file(workspace_root: &Path, id: &str) -> Option<SkillFile> {
    let path = skill_path(workspace_root, id);
    let bytes = fs::read(&path).ok()?;
    let plain = crate::agent::crypto::open(workspace_root, b"skill", &bytes);
    let content = String::from_utf8_lossy(&plain);
    parse_skill_nda(&content, id)
}

/// Persist a skill file, creating the skills directory if needed.
pub fn save_skill_file(workspace_root: &Path, skill: &SkillFile) -> bool {
    let dir = skills_dir(workspace_root);
    let _ = fs::create_dir_all(&dir);
    let serialized = serialize_skill_nda(skill);
    let bytes = crate::agent::crypto::seal(workspace_root, b"skill", serialized.as_bytes())
        .unwrap_or_else(|| serialized.into_bytes());
    fs::write(skill_path(workspace_root, &skill.id), bytes).is_ok()
}

/// List every skill file discovered under `.velocity/skills`, sorted by id.
pub fn list_skill_files(workspace_root: &Path) -> Vec<SkillFile> {
    let mut skills = Vec::new();
    if let Ok(read_dir) = fs::read_dir(skills_dir(workspace_root)) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("nda") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if id.is_empty() {
                continue;
            }
            if let Ok(bytes) = fs::read(&path) {
                let plain = crate::agent::crypto::open(workspace_root, b"skill", &bytes);
                let content = String::from_utf8_lossy(&plain);
                if let Some(skill) = parse_skill_nda(&content, &id) {
                    skills.push(skill);
                }
            }
        }
    }
    skills.sort_by(|a, b| a.id.cmp(&b.id));
    skills
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_round_trip_preserves_fields() {
        let skill = SkillFile::new(
            "netcode",
            "Netcode Expert",
            "Low-level networking",
            "Line one\tTabbed\nLine two with \\ backslash",
        );
        let text = serialize_skill_nda(&skill);
        let parsed = parse_skill_nda(&text, "netcode").expect("parse");
        assert_eq!(parsed, skill);
    }

    #[test]
    fn parse_rejects_unknown_header() {
        assert!(parse_skill_nda("not a skill", "x").is_none());
    }
}
