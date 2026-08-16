# NDA Binary Format Specification

<cite>
**Referenced Files in This Document**
- [velocity-ide/src/nda.rs](file://velocity-ide/src/nda.rs)
- [velocity-ide/src/nda_int/mod.rs](file://velocity-ide/src/nda_int/mod.rs)
- [velocity-ide/src/nda_int/ops.rs](file://velocity-ide/src/nda_int/ops.rs)
- [velocity-ide/src/nda_int/tables.rs](file://velocity-ide/src/nda_int/tables.rs)
- [velocity-browser/src/nda.rs](file://velocity-browser/src/nda.rs)
- [velocity-browser/src/nda_portable.rs](file://velocity-browser/src/nda_portable.rs)
- [velocity-mcp/src/protocol/nmcp_binary.rs](file://velocity-mcp/src/protocol/nmcp_binary.rs)
- [velocity-mcp/docs/NDA_FORMAT.md](file://velocity-mcp/docs/NDA_FORMAT.md)
- [velocity-mcp/docs/NDA_BOUNDARIES.md](file://velocity-mcp/docs/NDA_BOUNDARIES.md)
</cite>

## Overview

NDA (Non-Deterministic Automata) is the canonical binary persistence format for Velocity. It stores state as compact 18-byte triples (`NdaTriple`) under `.velocity/` directories. JSON is supported only as an import/export adapter.

## Record Format

Each NDA triple is exactly 18 bytes:

| Field | Size | Description |
|-------|------|-------------|
| `subject` | 4 bytes (u32) | String hash registry index |
| `predicate` | 2 bytes (u16) | Relation type identifier |
| `object_kind` | 1 byte (u8) | Inline value type tag |
| `object_value` | 11 bytes | Inline value or hash reference |

## Security Model

- **Integrity-based, not encryption-based**: SHA-256 Merkle chain verifies file integrity
- Each NDA file contains a Merkle hash header linking to previous state
- Tampering is detectable; content is not hidden
- JSON export strips Merkle proofs (adapter-only format)

## String Hash Registry

Strings are stored as u32 hashes in a global registry (`velocity-ide/src/site_map/`):
- Deterministic: same string always produces same hash
- Registry is serialized alongside triples for deserialization
- Collision resistance via 32-bit FNV-1a variant

## Key Files

| File | Role |
|------|------|
| `velocity-ide/src/nda.rs` | Core NDA triple definition and serialization |
| `velocity-ide/src/nda_int/ops.rs` | NDA interpreter operations |
| `velocity-ide/src/nda_int/tables.rs` | Lookup tables for NDA ops |
| `velocity-ide/src/nda_int/gemv.rs` | GEMV kernel over NDA vectors |
| `velocity-browser/src/nda.rs` | Browser-side NDA persistence |
| `velocity-browser/src/nda_portable.rs` | Cross-platform NDA serialization |
| `velocity-browser/src/agentic/nda_encoder.rs` | Agentic NDA encoding |
| `velocity-mcp/src/protocol/nmcp_binary.rs` | NMCP binary protocol over NDA |
| `velocity-mcp/src/agent/nda.rs` | Agent-side NDA state |

## NDA Compiler Pipeline

The NDA compiler (`velocity-ide/src/compiler/`) transforms Rust source into NDA bytecode:

1. **Tokenize** (`nda_lexer.rs`) → token stream
2. **Parse** (`nda_parser.rs`) → AST
3. **Lower** (`rust_to_nda.rs`) → NDA IR
4. **JIT** (`nda_jit/`) → native x86 code
5. **Execute** (`sandbox/`) → sandboxed validation

## Rules

1. NDA is canonical. JSON is import/export only.
2. All persisted agent state uses NDA format.
3. Schema changes require migration logic — format drift corrupts state.
4. Merkle chain must be maintained across writes.
5. Never encrypt NDA files — integrity, not secrecy.

**Section sources**
- [velocity-ide/src/nda.rs](file://velocity-ide/src/nda.rs)
- [velocity-mcp/docs/NDA_FORMAT.md](file://velocity-mcp/docs/NDA_FORMAT.md)
- [velocity-mcp/docs/NDA_BOUNDARIES.md](file://velocity-mcp/docs/NDA_BOUNDARIES.md)
