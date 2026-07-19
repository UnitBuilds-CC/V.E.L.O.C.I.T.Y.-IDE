use std::path::Path;

use velocity_ide::site_map::SiteMap;

pub fn resolve_weight_root(workspace_root: &Path) -> u64 {
    let sitemap_dir = workspace_root.join(".velocity").join("site_map");
    SiteMap::read_persisted_weight_root(&sitemap_dir).unwrap_or(0)
}

pub fn open_workspace_site_map(workspace_root: &Path) -> Result<SiteMap, String> {
    let sitemap_dir = workspace_root.join(".velocity").join("site_map");
    let weight_root = resolve_weight_root(workspace_root);
    SiteMap::open(&sitemap_dir, weight_root).map_err(|err| err.to_string())
}
