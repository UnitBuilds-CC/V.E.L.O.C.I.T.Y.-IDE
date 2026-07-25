# NDA Format & Security Model

NDA (No-Delay Binary AST) is Velocity's proprietary binary serialization format. It is the canonical internal representation for all state and semantic content — not an encryption scheme, but a compact, deterministic, integrity-verified binary format.

---

## NDA Binary Format Specification

### Header Layout

```
┌──────────────────────────────────────────────────────────┐
│  Magic: "NDA1" (4 bytes)                                │
│  Entry count (variable-length integer)                   │
│  String pool offset (byte position)                      │
│  Data section offset (byte position)                     │
├──────────────────────────────────────────────────────────┤
│  String Pool                                             │
│  (Deduplicated, null-terminated strings)                 │
├──────────────────────────────────────────────────────────┤
│  Data Section                                            │
│  (Triples, nodes, entities in deterministic order)       │
├──────────────────────────────────────────────────────────┤
│  Merkle Hash Chain                                       │
│  (SHA-256 root hash for integrity verification)          │
└──────────────────────────────────────────────────────────┘
```

### NDA Node Types

```rust
// velocity-ide/src/site_map/types.rs
pub enum NdaNode {
    Triple {
        subject_hash: u64,
        predicate_id: u16,
        object_hash: u64,
    },
    // Additional node types for entities, metadata
}
```

### Key Properties

- **Deterministic**: Same input always produces identical binary output
- **Compact**: Sub-millisecond load times for large codebases
- **Self-describing**: Header contains offsets for all sections
- **Diff-friendly**: Stable field ordering enables binary diffing

---

## SHA-256 Merkle Integrity Chain

### How It Works

Every NDA file includes a Merkle tree hash computed over its contents:

```
Root Hash (stored in NDA header)
├── Block 0 Hash
│   ├── Entry 0
│   ├── Entry 1
│   └── Entry 2
├── Block 1 Hash
│   ├── Entry 3
│   ├── Entry 4
│   └── Entry 5
└── ...
```

### Verification Process

```rust
// velocity-ide/src/site_map/verifier.rs
// 1. Read NDA file from disk
// 2. Recompute Merkle root from data blocks
// 3. Compare with stored root hash
// 4. Match = integrity verified, Mismatch = corruption detected
```

### What Gets Verified

| File | Verified Content |
|------|-----------------|
| `sitemap.nda` | All symbol relationship triples |
| `changelog.nda` | Workspace git history and file edits |
| `transcript.nda` | Agent conversation logs |
| `execution_facts.nda` | Multi-agent worktree edit contracts |
| `wa_snapshots/` | Windows automation state captures |

### Purpose

The Merkle chain provides:
- **Tamper detection**: Any modification to NDA content changes the root hash
- **Stale cache detection**: If a file changes on disk, its hash no longer matches
- **Incremental verification**: Only changed blocks need re-hashing
- **No external dependencies**: SHA-256 via the `sha2` crate (already a workspace dependency)

---

## At-Rest Security Model

### What NDA Security IS

NDA security is **integrity-based**, not confidentiality-based:
- SHA-256 Merkle hashes detect tampering and corruption
- Deterministic serialization enables reproducible verification
- Binary format provides obscurity through non-human-readability

### What NDA Security IS NOT

- NDA is **not encrypted** — it is a binary format, not a cipher
- The `.nda` extension does not imply AES or other encryption
- Keys in `.velocity/nda.key` are for DPAPI protection of workspace metadata, not NDA content encryption

### DPAPI Key Protection (Windows)

On Windows, `.velocity/nda.key` is protected via Windows DPAPI (Data Protection API) through FFI:
- The key is encrypted using the user's Windows credentials
- Only the same user account on the same machine can decrypt it
- Prevents other users or processes from accessing workspace keys

### Security Boundaries

| Layer | Protection | Mechanism |
|-------|-----------|-----------|
| NDA content | Integrity | SHA-256 Merkle hashes |
| Workspace key | Confidentiality | Windows DPAPI (FFI) |
| File access | OS-level | Filesystem permissions |
| Agent sandbox | Scope isolation | Workspace path sandboxing |
| Worktree locks | Conflict prevention | MediatorArena with TTL |

---

## NDA vs JSON Boundary

### When to Use NDA

Per `docs/NDA_FORMAT.md` and `docs/NDA_BOUNDARIES.md`:

- **Canonical storage**: All persistent state (sitemap, changelog, transcript, execution facts)
- **Agent output**: Prefer NDA directly instead of JSON
- **Semantic representation**: Triples, entities, relationships
- **Compact serialization**: When load time matters

### When to Use JSON

- **Import/export adapter**: JSON as an interchange format at system boundaries
- **External API communication**: MCP protocol messages
- **Human-readable configuration**: `workspace-preferences.json`, `build_diagnostics.json`
- **Tool argument passing**: Tool call arguments are JSON Value

### Conversion Rules

```
NDA → JSON: Allowed for interoperability
JSON → NDA: Must normalize back to canonical NDA
NDA is always the source of truth
```

---

## Agent Authoring Rules

When producing NDA output, agents must follow these rules:

1. **Prefer NDA directly** — do not wrap NDA in JSON unless interop requires it
2. **Preserve semantics exactly** — do not add commentary inside the payload
3. **Keep output deterministic** — same meaning → same binary
4. **Use stable identifiers** — reuse the same hash for the same entity
5. **Do not mix prose and NDA** — separate content blocks cleanly
6. **Smallest valid form** — if uncertain, produce the minimal NDA that preserves meaning

### Common Mistakes to Avoid

- Adding explanatory prose inside the NDA payload
- Renaming the same entity across lines (changes the hash)
- Representing the same fact in multiple different shapes
- Emitting JSON-like wrappers around NDA content
- Using non-deterministic ordering

### NDA Triple Example

```text
user:alice knows user:bob
user:alice works_on project:velocity
project:velocity uses format:nda
```

### Canonical Entity Reuse

```text
task:routing kind refactor
task:routing status planned
task:routing target file:velocity-mcp/src/automation/task_router.rs
```

---

## NDA Files in the Workspace

| File | Location | Content |
|------|----------|---------|
| `sitemap.nda` | `.velocity/sitemap.nda` | Symbol relationship triples |
| `changelog.nda` | `.velocity/changelog.nda` | Git history, incremental edits |
| `transcript.nda` | `.velocity/transcript.nda` | Agent conversation logs |
| `execution_facts.nda` | `.velocity/execution_facts.nda` | Worktree edit contracts |
| `nda.key` | `.velocity/nda.key` | DPAPI-protected workspace key |
| `wa_snapshots/` | `.velocity/wa_snapshots/` | Windows automation state |
| `agentic/` | `.velocity/agentic/` | Agentic run data, task snapshots |

---

## Performance Characteristics

| Operation | Latency | Notes |
|-----------|---------|-------|
| Load sitemap.nda | <1ms | Memory-mapped, pre-indexed |
| Write single triple | <10μs | Amortized batch writes |
| Merkle verification | <1ms | Incremental re-hash of changed blocks |
| NDA encode (1000 triples) | <5ms | Zero-allocation writer |
| String hash lookup | O(1) | Deterministic u64 hash |
