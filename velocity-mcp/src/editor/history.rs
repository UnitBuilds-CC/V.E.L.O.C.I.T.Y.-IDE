use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAX_SNAPSHOTS_PER_FILE: usize = 20;

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub timestamp: u64,
    pub content: String,
    pub size: usize,
}

impl Snapshot {
    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn short_id(&self) -> String {
        format!("{}", self.timestamp)
    }

    pub fn age_label(&self) -> String {
        let now = Self::now_millis();
        let ms = now.saturating_sub(self.timestamp);
        if ms < 60_000 {
            format!("{}s ago", ms / 1000)
        } else if ms < 3_600_000 {
            format!("{}m ago", ms / 60_000)
        } else if ms < 86_400_000 {
            format!("{}h ago", ms / 3_600_000)
        } else {
            format!("{}d ago", ms / 86_400_000)
        }
    }
}

pub struct FileHistory {
    base: PathBuf,
    index_path: PathBuf,
    index: HashMap<PathBuf, Vec<Snapshot>>,
}

impl FileHistory {
    pub fn new(workspace_root: &Path) -> Self {
        let base = workspace_root.join(".velocity").join("history");
        let index_path = base.join("index.nda");
        let _ = fs::create_dir_all(&base);
        let mut index = Self::load_index_from_nda(&base, &index_path)
            .unwrap_or_else(|| Self::load_index_from_snapshots(&base));
        Self::sort_index(&mut index);
        Self { base, index_path, index }
    }

    pub fn record(&mut self, file_path: &Path, content: &str) {
        let snapshot = Snapshot {
            timestamp: Snapshot::now_millis(),
            size: content.len(),
            content: content.to_string(),
        };
        let snap_path = self.snapshot_path(file_path, snapshot.timestamp);
        if let Some(parent) = snap_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&snap_path, content);
        let list = self.index.entry(file_path.to_path_buf()).or_default();
        list.push(snapshot);
        if list.len() > MAX_SNAPSHOTS_PER_FILE {
            list.sort_by_key(|s| s.timestamp);
            let removed = list.drain(0..=list.len() - MAX_SNAPSHOTS_PER_FILE - 1).collect::<Vec<_>>();
            for snap in removed {
                let _ = fs::remove_file(self.snapshot_path(file_path, snap.timestamp));
            }
        }
        Self::sort_index(&mut self.index);
        let _ = self.persist_index();
    }

    pub fn snapshots(&self, file_path: &Path) -> Vec<Snapshot> {
        self.index.get(file_path).cloned().unwrap_or_default()
    }

    pub fn latest_snapshot(&self, file_path: &Path) -> Option<Snapshot> {
        self.snapshots(file_path).first().cloned()
    }

    pub fn diff_strings(&self, _file_path: &Path, old: &str, new: &str) -> String {
        // Simple line-based diff good enough for small files.
        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();
        let mut out = String::new();
        let mut o = 0usize;
        let mut n = 0usize;
        while o < old_lines.len() || n < new_lines.len() {
            if o < old_lines.len() && n < new_lines.len() && old_lines[o] == new_lines[n] {
                out.push_str("  ");
                out.push_str(old_lines[o]);
                out.push('\n');
                o += 1;
                n += 1;
            } else if n < new_lines.len()
                && (o >= old_lines.len() || !old_lines[o..].contains(&new_lines[n]))
            {
                out.push_str("+ ");
                out.push_str(new_lines[n]);
                out.push('\n');
                n += 1;
            } else if o < old_lines.len() {
                out.push_str("- ");
                out.push_str(old_lines[o]);
                out.push('\n');
                o += 1;
            } else {
                break;
            }
        }
        if out.is_empty() {
            out.push_str("(no differences)");
        }
        out
    }

    fn snapshot_path(&self, file_path: &Path, timestamp: u64) -> PathBuf {
        // Store under a file-hash derived directory to avoid collisions.
        let encoded = Self::encode_name(file_path);
        self.base.join(format!("{}-{}.snap", encoded, timestamp))
    }

    fn encode_name(file_path: &Path) -> String {
        file_path
            .to_string_lossy()
            .replace(['/', '\\', ':'], "_")
            .replace(' ', "%20")
    }

    fn decode_name(name: &str) -> PathBuf {
        PathBuf::from(name.replace("%20", " ").replace('_', "/"))
    }

    fn load_snapshot(path: &Path, timestamp: u64) -> Snapshot {
        let content = fs::read_to_string(path).unwrap_or_default();
        let size = content.len();
        Snapshot {
            timestamp,
            content,
            size,
        }
    }

    fn load_index_from_nda(base: &Path, index_path: &Path) -> Option<HashMap<PathBuf, Vec<Snapshot>>> {
        let content = fs::read_to_string(index_path).ok()?;
        let mut lines = content.lines();
        let header = lines.find(|line| !line.trim().is_empty())?.trim().to_string();
        let mut index: HashMap<PathBuf, Vec<Snapshot>> = HashMap::new();

        if header == "history version 2" {
            for line in lines {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line.starts_with("file_count ") || line.starts_with("snapshot_count ") {
                    continue;
                }
                let parts = line.split('\t').collect::<Vec<_>>();
                if parts.len() != 5 || parts[0] != "snapshot" {
                    continue;
                }
                let file_path = PathBuf::from(parts[1]);
                let timestamp = parts[2].parse::<u64>().ok()?;
                let size = parts[3].parse::<usize>().ok()?;
                let snap_path = base.join(parts[4]);
                let mut snapshot = Self::load_snapshot(&snap_path, timestamp);
                snapshot.size = size;
                index.entry(file_path).or_default().push(snapshot);
            }
            return Some(index);
        }

        if header != "history version 1" {
            return None;
        }

        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.len() != 4 || parts[0] != "snapshot" {
                continue;
            }
            let file_path = PathBuf::from(parts[1]);
            let timestamp = parts[2].parse::<u64>().ok()?;
            let snap_path = base.join(parts[3]);
            let snapshot = Self::load_snapshot(&snap_path, timestamp);
            index.entry(file_path).or_default().push(snapshot);
        }
        Some(index)
    }

    fn load_index_from_snapshots(base: &Path) -> HashMap<PathBuf, Vec<Snapshot>> {
        let mut index = HashMap::new();
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("snap") {
                    let name = path.file_stem().unwrap_or_default().to_string_lossy();
                    let parts: Vec<&str> = name.rsplitn(2, '-').collect();
                    if parts.len() == 2 {
                        let ts = parts[0].parse::<u64>().unwrap_or(0);
                        let file_name = parts[1];
                        let snapshot = Self::load_snapshot(&path, ts);
                        let file_path = Self::decode_name(file_name);
                        index.entry(file_path).or_insert_with(Vec::new).push(snapshot);
                    }
                }
            }
        }
        index
    }

    fn sort_index(index: &mut HashMap<PathBuf, Vec<Snapshot>>) {
        for snapshots in index.values_mut() {
            snapshots.sort_by_key(|s| s.timestamp);
            snapshots.reverse();
        }
    }

    fn persist_index(&self) -> Result<(), String> {
        let mut file_paths = self.index.keys().cloned().collect::<Vec<_>>();
        file_paths.sort();
        let snapshot_total = self.index.values().map(|snapshots| snapshots.len()).sum::<usize>();

        let mut entries = file_paths
            .iter()
            .flat_map(|file_path| {
                self.index
                    .get(file_path)
                    .into_iter()
                    .flat_map(move |snapshots| snapshots.iter().map(move |snapshot| {
                        let snap_name = self
                            .snapshot_path(file_path, snapshot.timestamp)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        format!(
                            "snapshot\t{}\t{}\t{}\t{}",
                            file_path.display(),
                            snapshot.timestamp,
                            snapshot.size,
                            snap_name
                        )
                    }))
            })
            .collect::<Vec<_>>();
        entries.sort();
        let mut content = format!(
            "history version 2\nfile_count {}\nsnapshot_count {}\n",
            file_paths.len(),
            snapshot_total
        );
        if !entries.is_empty() {
            content.push_str(&entries.join("\n"));
            content.push('\n');
        }
        fs::write(&self.index_path, content).map_err(|err| format!("write history index: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn records_and_lists_snapshots() {
        let tmp = tempfile::tempdir().unwrap();
        let mut hist = FileHistory::new(tmp.path());
        hist.record(Path::new("src/main.rs"), "fn main() {}");
        let snaps = hist.snapshots(Path::new("src/main.rs"));
        assert_eq!(snaps.len(), 1);
        assert!(snaps[0].age_label().contains("s ago") || snaps[0].age_label().contains("now"));
    }

    #[test]
    fn writes_nda_history_index() {
        let tmp = tempfile::tempdir().unwrap();
        let mut hist = FileHistory::new(tmp.path());
        hist.record(Path::new("src/main.rs"), "fn main() {}");

        let index = fs::read_to_string(tmp.path().join(".velocity").join("history").join("index.nda")).unwrap();
        assert!(index.starts_with("history version 2\n"));
        assert!(index.contains("file_count 1\n"));
        assert!(index.contains("snapshot_count 1\n"));
        assert!(index.contains("snapshot\tsrc/main.rs\t"));
        assert!(index.contains("\t12\tsrc_main.rs-"));
    }

    #[test]
    fn loads_from_nda_history_index() {
        let tmp = tempfile::tempdir().unwrap();
        let history_dir = tmp.path().join(".velocity").join("history");
        fs::create_dir_all(&history_dir).unwrap();
        fs::write(history_dir.join("src_main.rs-123.snap"), "snapshot-body").unwrap();
        fs::write(
            history_dir.join("index.nda"),
            "history version 2\nfile_count 1\nsnapshot_count 1\nsnapshot\tsrc/main.rs\t123\t13\tsrc_main.rs-123.snap\n",
        )
        .unwrap();

        let hist = FileHistory::new(tmp.path());
        let snaps = hist.snapshots(Path::new("src/main.rs"));
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].timestamp, 123);
        assert_eq!(snaps[0].content, "snapshot-body");
    }

    #[test]
    fn diff_detects_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let hist = FileHistory::new(tmp.path());
        let out = hist.diff_strings(Path::new("x.rs"), "line1\nline2\n", "line1\nline3\n");
        assert!(out.contains("- line2"));
        assert!(out.contains("+ line3"));
    }
}
