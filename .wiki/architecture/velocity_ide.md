# Velocity IDE & SiteMap Architecture

The `velocity-ide` crate provides semantic indexing, RDF site map storage, NDA AST compilation, and automated Markdown wiki generation for the Velocity workspace.

---

## 🏛️ Component Overview

```
                      ┌─────────────────────────────────┐
                      │          velocity-ide           │
                      └────────────────┬────────────────┘
                                       │
            ┌──────────────────────────┼──────────────────────────┐
            │                          │                          │
            ▼                          ▼                          ▼
   ┌─────────────────┐        ┌─────────────────┐        ┌─────────────────┐
   │    site_map     │        │    compiler     │        │      wiki       │
   │ (RDF Triples &  │        │ (Rust to NDA    │        │ (Model Builder  │
   │ String Index)   │        │ AST & Symbols)  │        │  & Markdown)    │
   └─────────────────┘        └─────────────────┘        └─────────────────┘
```

---

## 🔧 Subsystem Breakdown

### 1. SiteMap Triple Store (`src/site_map/`)
The `SiteMap` is an in-memory and disk-backed RDF binary database that indexes symbol relationships across the codebase.
- **Triple Format**: Represented as `VcTriple { subject_hash, predicate_id, object_hash }`.
- **Predicates**:
  - `1`: `DEFINES` (e.g., `file.rs` defines `symbol_a`)
  - `2`: `CALLS` (e.g., `function_a` calls `function_b`)
  - `3`: `IMPORTS` (e.g., `file_a` imports `file_b`)
- **Deterministic String Index**: Hashes strings to unique 64-bit integer identifiers, enabling instant graph lookup and verification (`verifier.rs`).

### 2. Rust to NDA Compiler (`src/compiler/`)
- **`rust_to_nda.rs`**: Parses Rust source code files into compact binary NDA (No-Delay Binary AST) formats.
- Extracts functions, structs, enums, traits, call sites, and documentation comments.
- Enables sub-millisecond symbol resolution for IDE autocompletion and agent context building.

### 3. Automated Wiki Generator (`src/wiki/`)
- **`generate.rs`**: Traverses the `SiteMap` triple store to build a `WikiModel` containing Overview, File, and Symbol pages.
- **`markdown.rs`**: Renders the `WikiModel` into interlinked Markdown files (`index.md`, `files/*.md`, `symbols/*.md`).
- Cross-links all caller/callee relationships with relative Markdown links (`../symbols/my_func.md`).
