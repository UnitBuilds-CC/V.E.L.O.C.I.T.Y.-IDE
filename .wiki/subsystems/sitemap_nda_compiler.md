# SiteMap & NDA Binary Compiler

The `velocity-ide` crate provides symbol indexing, RDF triple graph storage, AST parsing, and binary serialization for the Velocity workspace.

---

## 🗺️ SiteMap RDF Triple Store (`site_map/`)

The `SiteMap` is an in-memory and disk-persisted triple database stored under `.velocity/site_map/` and `.velocity/sitemap.nda`.

### 1. Triple Structure (`VcTriple`)
```rust
pub struct VcTriple {
    pub subject_hash: u64,
    pub predicate_id: u16,
    pub object_hash: u64,
}
```

### 2. Standard Predicates
| Predicate ID | Label | Description |
| :--- | :--- | :--- |
| `1` | `DEFINES` | A file or module subject defines a symbol object |
| `2` | `CALLS` | A function or method subject calls another symbol object |
| `3` | `IMPORTS` | A file subject imports another module or symbol object |

### 3. String Hash Registry & Verifier (`verifier.rs`)
- Maps human-readable identifiers (`src/lib.rs`, `my_function`) to deterministic 64-bit FNV/XxHash integers.
- Calculates root merkle tree hashes across scope nodes to detect file corruption or invalid stale caches instantly.

---

## ⚡ NDA (No-Delay Binary AST) Format

NDA is a custom binary serialization specification used across Velocity to achieve sub-millisecond load times for massive codebases.

- **Header Specification**: Magic bytes `NDA1`, total entry count, string pool offset, and data section offset.
- **Used For**:
  - `sitemap.nda`: Codebase symbol relationships.
  - `changelog.nda`: Workspace git history & incremental file edits.
  - `transcript.nda`: Agent conversation transcript logs.
  - `execution_facts.nda`: Multi-agent worktree execution contracts.

---

## 📖 Automated Wiki Builder (`wiki/`)

The `wiki` module consumes the `SiteMap` triple graph and automatically synthesizes a full cross-linked Markdown documentation site:
- **`build_wiki(&SiteMap) -> WikiModel`**: Constructs Overview, File, and Symbol pages with `Defines`, `Calls`, and `Called By` relationships.
- **`export_markdown(&WikiModel, path)`**: Writes formatted Markdown files (`index.md`, `files/*.md`, `symbols/*.md`) directly to disk.
