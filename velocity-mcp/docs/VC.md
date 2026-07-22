# V.E.L.O.C.I.T.Y. Custom Version Control System (VC)

## What is VC?

**VC** in V.E.L.O.C.I.T.Y. stands for **Version Control**, specifically referring to our custom, zero-external-dependency **Merkle AST Version Control System** implemented in elocity-ide/src/site_map/.

Instead of relying on heavy third-party graph databases (like Neo4j) or standard line-based diff tools alone, V.E.L.O.C.I.T.Y. uses a **semantic binary Merkle tree** stored in .velocity/agentic/ as .nda binary files.

---

## Core Components of the VC System

### 1. VcTriple (Semantic Triples)
An 18-byte compact binary triple that records relationships between code symbols (functions, structs, variables, method calls):

`ust
pub struct VcTriple {
    pub subject_hash: u64,  // Hash of declaring scope or caller symbol
    pub predicate_id: u16,  // Relationship (0 = Declare, 1 = Define, 2 = Call)
    pub object_hash: u64,   // Hash of declared symbol or target callee
}
`

### 2. SiteMap (Merkle Tree Storage)
SiteMap manages the local Version Control database:
- **Kv**: Key-Value token mappings.
- **Node**: AST program nodes.
- **Snapshot**: File-scoped live semantic snapshots.
- **MerkleVerifier**: Computes deterministic root hashes (oot and weight_root) to verify code integrity and detect corruption without parsing raw files.

### 3. Workspace File Tree & Symbol History Explorer
Integrated into elocity-mcp/src/editor/graph_view.rs:
- **Left Pane**: File hierarchy and declared symbols.
- **Right Pane**: Change history, timestamp, action type, and context rationale for any selected method or variable.

---

## Why Custom VC over Git alone or Neo4j?

1. **Sub-millisecond Symbol Lookups**: Binary NDA triples enable fast symbol dependency lookup without spawning external processes.
2. **Zero External Dependency**: Runs natively in Rust inside elocity-ide and elocity-mcp.
3. **AST-Level Granularity**: Tracks changes per-function and per-symbol, preserving context rationale across agent edits.
