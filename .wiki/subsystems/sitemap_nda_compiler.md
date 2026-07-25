# SiteMap & NDA Binary Compiler

The `velocity-ide` crate provides symbol indexing, RDF triple graph storage, Rust-to-NDA compilation, Merkle integrity verification, and automated wiki generation.

> For full module inventory and pipeline details, see [velocity-ide: Compiler & SiteMap](../architecture/velocity_ide.md).

---

## Quick Reference

### Module Structure

```
site_map/
├── mod.rs              # SiteMap: open(), put_node(), flush()
├── verifier.rs         # Merkle tree hash verification
├── string_registry.rs  # Deterministic string→u64 hash mapping
├── tests.rs            # Unit tests
└── types.rs            # VcTriple, NdaNode

compiler/
├── rust_to_nda.rs      # Main Rust→NDA pipeline
├── lexer.rs            # Token stream generation
├── parser.rs           # Rust syntax parsing
├── ast_builder.rs      # AST construction
├── nda_encoder.rs      # NDA binary serialization
├── nda_jit/            # JIT compilation
├── driver/             # Vulkan GPU driver (BitNet, Qwen)
├── jit/                # JIT weight-inlining compiler
├── property_fuzzer.rs  # Property-based fuzzing
└── sandbox/            # JIT sandbox, Wasm plugin runner

wiki/
├── generate.rs         # build_wiki(): SiteMap → WikiModel
├── markdown.rs         # export_markdown(): WikiModel → .md files
└── tests.rs            # Wiki generation tests
```

---

## SiteMap RDF Triple Store

### Triple Structure

```rust
pub struct VcTriple {
    pub subject_hash: u64,
    pub predicate_id: u16,
    pub object_hash: u64,
}
```

### Standard Predicates

| ID | Label | Meaning |
|----|-------|---------|
| 1 | DEFINES | File/module defines a symbol |
| 2 | CALLS | Function calls another symbol |
| 3 | IMPORTS | File imports a module or symbol |

### SiteMap API

```rust
pub fn open(dir: &Path, flags: u32) -> Result<Self>;
pub fn register_string(&mut self, s: &str) -> Result<u64>;
pub fn put_node(&mut self, node: &NdaNode) -> Result<()>;
pub fn put_file_snapshot(&mut self, file: &str, triples: &[VcTriple]) -> Result<()>;
pub fn remove_file_snapshot(&mut self, file: &str) -> Result<()>;
pub fn flush(&mut self) -> Result<()>;
```

---

## NDA Binary Format

NDA (No-Delay Binary AST) is Velocity's proprietary binary serialization:

- **Header**: Magic bytes `NDA1`, entry count, string pool offset, data section offset
- **String Pool**: Deduplicated, null-terminated strings
- **Data Section**: Triples and entities in deterministic order
- **Merkle Hash**: SHA-256 root hash for integrity verification

### Files Using NDA

| File | Content |
|------|---------|
| `sitemap.nda` | Symbol relationships |
| `changelog.nda` | Git history and edits |
| `transcript.nda` | Agent conversation logs |
| `execution_facts.nda` | Worktree edit contracts |

---

## String Hash Registry & Merkle Verification

- **String Hash Registry**: Maps identifiers to deterministic 64-bit SHA-256 truncated hashes
- **Merkle Verifier**: Computes root hash over all triples; detects corruption or stale caches

---

## Automated Wiki Builder

```rust
let sm = SiteMap::open(Path::new(".velocity/site_map"), 0)?;
let model = build_wiki(&sm);
let written = export_markdown(&model, Path::new(".wiki"))?;
```

Generates interlinked Markdown:
- `index.md` — Overview with file/symbol counts
- `files/*.md` — Per-file pages with defined symbols
- `symbols/*.md` — Per-symbol pages with caller/callee relationships

---

## See Also

- [velocity-ide: Compiler & SiteMap Architecture](../architecture/velocity_ide.md) — Full pipeline details
- [NDA Format & Security Model](nda_security.md) — Format spec, Merkle integrity, agent rules
- [Build & Development Workflow](../references/build_workflow.md) — How to build and test
