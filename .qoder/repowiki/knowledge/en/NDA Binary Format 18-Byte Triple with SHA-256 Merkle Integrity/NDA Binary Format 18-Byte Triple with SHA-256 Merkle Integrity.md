# NDA Binary Format 18-Byte Triple with SHA-256 Merkle Integrity

## Classification
- **Category**: Data Format / Security
- **Files**: velocity-ide/src/nda.rs, velocity-ide/src/nda_int/, velocity-browser/src/nda.rs, velocity-mcp/src/protocol/nmcp_binary.rs
- **Criticality**: Critical — all persisted state uses this format

## Summary

NDA (Non-Deterministic Automata) is the canonical binary persistence format. Each record is an 18-byte triple: subject (u32) + predicate (u16) + object_kind (u8) + object_value (11 bytes). Security is integrity-based via SHA-256 Merkle chains, not encryption.

## Record Layout

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | subject (string hash) |
| 4 | 2 | predicate (relation type) |
| 6 | 1 | object_kind (value type tag) |
| 7 | 11 | object_value (inline or hash) |

## Rules

1. NDA is canonical — JSON is import/export adapter only
2. Schema changes require migration logic
3. Merkle chain must be maintained across writes
4. Never encrypt NDA — integrity, not secrecy
5. String hashes use FNV-1a variant in global registry

## Cross-Crate Usage

- `velocity-ide`: Core definition, interpreter, compiler output
- `velocity-browser`: Browser state persistence, agentic encoding
- `velocity-mcp`: NMCP binary protocol, agent state
