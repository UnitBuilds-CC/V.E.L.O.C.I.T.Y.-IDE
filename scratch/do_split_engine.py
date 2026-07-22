import os, sys

engine_file = r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-mcp\src\editor\browser\engine.rs"
engine_dir = r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-mcp\src\editor\browser\engine"

os.makedirs(engine_dir, exist_ok=True)

with open(engine_file, 'r', encoding='utf-8') as f:
    lines = f.readlines()

TYPES_HEADER = """use crate::editor::browser::models::*;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sha2::{Digest, Sha256};
use velocity_ide::site_map::verifier::NdaNode;
use velocity_ide::site_map::{SiteMap, VcTriple};
"""

SUBMODULE_HEADER = """use super::*;
"""

modules = {
    "types.rs": lines[0:878],
    "url_helpers.rs": lines[878:1208],
    "helpers.rs": lines[1208:1595],
    "reports.rs": lines[1595:2181],
    "snapshots.rs": lines[2181:2695],
    "sessions.rs": lines[2695:3145],
    "auth.rs": lines[3145:3398],
    "render_reports.rs": lines[3398:3807],
    "auth_diagnostics.rs": lines[3807:4100],
    "health.rs": lines[4100:4690],
    "auth_profiles.rs": lines[4690:5013],
    "checkpoints.rs": lines[5013:5551],
    "session_reports.rs": lines[5551:6036],
    "runtime.rs": lines[6036:6725],
    "session_actions.rs": lines[6725:7062],
    "snapshot_diff.rs": lines[7062:7816],
    "waits.rs": lines[7816:8481],
    "workflows.rs": lines[8481:9201],
    "workflow_runner.rs": lines[9201:10117],
}

for filename, content in modules.items():
    path = os.path.join(engine_dir, filename)
    header = TYPES_HEADER if filename == "types.rs" else SUBMODULE_HEADER
    with open(path, 'w', encoding='utf-8') as out:
        out.write(header + "\n")
        out.writelines(content)
    print(f"Wrote {filename}: {len(content)} lines")

# Create mod.rs in engine/
mod_rs_path = os.path.join(engine_dir, "mod.rs")
with open(mod_rs_path, 'w', encoding='utf-8') as out:
    mod_exports = "\n".join([f"pub mod {fname[:-3]};" for fname in modules.keys()])
    use_exports = "\n".join([f"pub use {fname[:-3]}::*;" for fname in modules.keys()])
    out.write(f"{mod_exports}\n\n{use_exports}\n")

print("Done generating engine submodules!")
