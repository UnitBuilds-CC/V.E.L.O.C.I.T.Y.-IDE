use std::path::{Path, PathBuf};

use velocity_ide::site_map::SiteMap;

pub fn resolve_weight_root(workspace_root: &Path) -> u64 {
    let mut root = 0u64;
    let mut found = false;

    for (idx, dir) in candidate_weight_dirs(workspace_root).into_iter().enumerate() {
        if dir.is_dir() {
            found = true;
            let hash = SiteMap::hash_weight_dir(&dir);
            root ^= hash.rotate_left((idx as u32) & 31);
        }
    }

    if found { root } else { 0 }
}

pub fn open_workspace_site_map(workspace_root: &Path) -> Result<SiteMap, String> {
    let sitemap_dir = workspace_root.join(".velocity").join("site_map");
    let weight_root = resolve_weight_root(workspace_root);
    SiteMap::open(&sitemap_dir, weight_root).map_err(|err| err.to_string())
}

fn candidate_weight_dirs(workspace_root: &Path) -> Vec<PathBuf> {
    vec![
        workspace_root.join("weights"),
        workspace_root.join("weight"),
        workspace_root.join("model"),
        workspace_root.join("models"),
        workspace_root.join("velocity-ide").join("weights"),
        workspace_root.join("velocity-ide").join("weight"),
        workspace_root.join("velocity-ide").join("model"),
        workspace_root.join("velocity-ide").join("models"),
    ]
}
