# velocity-ide: Compiler & SiteMap

The `velocity-ide` crate (75 source files) provides Rust-to-NDA compilation, RDF triple store indexing, Merkle integrity verification, automated wiki generation, and sandboxed execution.

---

## Rust-to-NDA Compiler Pipeline

### Module Structure (`compiler/`)

```
compiler/
├── mod.rs                  # Module root
├── rust_to_nda.rs          # Main Rust→NDA compilation pipeline
├── lexer.rs                # Token stream generation
├── parser.rs               # Rust syntax parsing
├── ast_builder.rs          # AST construction from tokens
├── nda_encoder.rs          # NDA binary serialization
├── nda_jit/                # JIT compilation for NDA execution
│   ├── mod.rs
│   └── tests.rs
├── driver/                 # Vulkan GPU driver for LLM inference
│   ├── mod.rs
│   ├── vulkan_init.rs      # Vulkan API initialization
│   ├── bitnet_layer.rs     # 1.58-bit quantized matrix multiply
│   ├── nda_bitnet_layer.rs # NDA-encoded BitNet layer
│   ├── qwen_layer.rs       # Qwen model kernels
│   └── gemv.rs             # General matrix-vector multiply
├── jit/                    # JIT weight-inlining compiler
│   └── mod.rs
├── property_fuzzer.rs      # Property-based fuzzing engine
└── sandbox/                # (see Sandbox section below)
```

### Compilation Pipeline

```
Rust source code (.rs)
    │
    ▼
1. Lexer (lexer.rs)
   Token stream: keywords, identifiers, literals, punctuation
    │
    ▼
2. Parser (parser.rs)
   Syntax tree: functions, structs, enums, traits, impl blocks
    │
    ▼
3. AST Builder (ast_builder.rs)
   Structured AST with semantic annotations
    │
    ▼
4. NDA Encoder (nda_encoder.rs)
   Compact binary NDA format with:
   - Magic bytes (NDA1 header)
   - Entry count, string pool offset, data section offset
   - Deterministic serialization
    │
    ▼
5. Output: .nda binary file
   Sub-millisecond load times for large codebases
```

### What Gets Extracted

- Function signatures and bodies
- Struct/enum/trait definitions
- Call sites (caller→callee relationships)
- Documentation comments
- Import/export relationships
- Type annotations

---

## SiteMap RDF Triple Store

### Module Structure (`site_map/`)

```
site_map/
├── mod.rs          # SiteMap struct, open(), put_node(), flush()
├── verifier.rs     # Merkle tree hash verification
├── string_registry.rs # Deterministic string→u64 hash mapping
├── tests.rs        # SiteMap unit tests
└── types.rs        # VcTriple, NdaNode, SiteMap flags
```

### Triple Structure

```rust
pub struct VcTriple {
    pub subject_hash: u64,    // Hashed string identifier
    pub predicate_id: u16,    // Relationship type
    pub object_hash: u64,     // Hashed string identifier
}
```

### Standard Predicates

| ID | Label | Meaning |
|----|-------|---------|
| 1 | DEFINES | File/module subject defines a symbol object |
| 2 | CALLS | Function subject calls another symbol object |
| 3 | IMPORTS | File subject imports another module or symbol |

### SiteMap API

```rust
pub struct SiteMap { ... }

impl SiteMap {
    /// Open or create a SiteMap at the given directory with flags
    pub fn open(dir: &Path, flags: u32) -> Result<Self>;
    
    /// Register a string and return its deterministic u64 hash
    pub fn register_string(&mut self, s: &str) -> Result<u64>;
    
    /// Insert a node (triple or entity) into the store
    pub fn put_node(&mut self, node: &NdaNode) -> Result<()>;
    
    /// Store a file's complete triple snapshot
    pub fn put_file_snapshot(&mut self, file: &str, triples: &[VcTriple]) -> Result<()>;
    
    /// Remove all triples associated with a file
    pub fn remove_file_snapshot(&mut self, file: &str) -> Result<()>;
    
    /// Persist to disk
    pub fn flush(&mut self) -> Result<()>;
}
```

### Storage Format

- **Disk location**: `.velocity/site_map/` and `.velocity/sitemap.nda`
- **In-memory**: Full triple store loaded into memory for sub-millisecond queries
- **Persistence**: `flush()` writes accumulated changes to disk

---

## String Hash Registry & Merkle Verification

### String Hash Registry (`string_registry.rs`)

Maps human-readable identifiers to deterministic 64-bit integers:

```
"src/lib.rs"         → 0xA3F1B2C4D5E6F708
"my_function"        → 0x1234567890ABCDEF
"velocity-mcp"       → 0xFEDCBA0987654321
```

- Uses SHA-256 truncated to 8 bytes (u64::from_le_bytes)
- Deterministic: same string always produces same hash
- Enables instant graph lookup without string comparison overhead

### Merkle Verification (`verifier.rs`)

Calculates root Merkle tree hashes across scope nodes:

```
Root Hash
├── Directory Hash (src/)
│   ├── File Hash (lib.rs)
│   │   └── Triple hashes
│   └── File Hash (main.rs)
│       └── Triple hashes
└── Directory Hash (tests/)
    └── File Hash (integration.rs)
        └── Triple hashes
```

**Purpose**: Detect file corruption or stale caches instantly. If any triple changes, the root hash changes, invalidating the cache.

---

## Automated Wiki Generator

### Module Structure (`wiki/`)

```
wiki/
├── mod.rs          # Module root
├── generate.rs     # build_wiki(): SiteMap → WikiModel
├── markdown.rs     # export_markdown(): WikiModel → .md files
└── tests.rs        # Wiki generation tests
```

### Wiki Generation Pipeline

```
1. build_wiki(&SiteMap) → WikiModel
   │
   ├── Traverse all DEFINES triples → Symbol pages
   ├── Traverse all CALLS triples → Caller/Callee relationships
   ├── Traverse all IMPORTS triples → Import relationships
   └── Generate Overview page with file/symbol counts
   │
2. export_markdown(&WikiModel, path) → usize (pages written)
   │
   ├── index.md          — Overview with links to all pages
   ├── files/*.md        — Per-file pages with defined symbols
   └── symbols/*.md      — Per-symbol pages with callers/callees
```

### Cross-Linking

All generated pages include relative Markdown links:
- `../symbols/my_func.md` — from file page to symbol page
- `../files/src/lib.rs.md` — from symbol page to defining file
- Caller/callee lists link to both symbol and file pages

---

## Sandbox & JIT Compiler

### Module Structure (`sandbox/`)

```
sandbox/
├── mod.rs              # Sandbox entry point
├── jit_sandbox.rs      # JIT compilation sandbox
└── wasm_runner.rs      # WasmPluginRunner
```

### JIT Sandbox

Provides isolated execution environment for compiled NDA code:
- Memory-bounded execution
- Instruction count limits
- Deterministic output verification

### WasmPluginRunner

Executes WebAssembly plugins within the sandbox:
- Wasm module loading and validation
- Host function imports
- Resource limit enforcement

---

## NDA Interpreter (`nda_int/`)

```
nda_int/
├── mod.rs              # NDA interpreter entry
├── runtime.rs          # NDA execution runtime
├── decoder.rs          # NDA binary decoding
├── evaluator.rs        # NDA expression evaluation
└── types.rs            # Interpreter types
```

The NDA interpreter executes NDA binary programs:
- Decodes NDA binary format into executable instructions
- Evaluates expressions with NDA-native types
- Provides runtime environment for NDA programs

---

## Pipeline Bridge (`pipeline_bridge.rs` & `pipeline_nda.rs`)

Bridges the compilation pipeline with NDA representation:
- `pipeline_bridge.rs`: Connects lexer→parser→AST→NDA stages
- `pipeline_nda.rs`: NDA-specific pipeline adaptations

---

## Model Types (`model/`)

```
model/
├── mod.rs              # Model module root
└── types.rs            # Data model definitions
```

Shared data model types used across the crate.

---

## Binary Entry Points (`bin/`)

| Binary | Purpose |
|--------|---------|
| `velocity_ide` | Main library binary |
| `bench_nda_vs_rust` | Benchmark: NDA performance vs Rust native |
| `run_nda` | NDA program runner |
| `test_tok` | Tokenizer test harness |

---

## Tokenizer (`tokenizer.rs`)

NDA-embedded tokenizer implementation:
- Bit-compressed embedding tables
- 3200-dimension active/pos bitmaps
- 10x memory savings vs FP16 lookup tables
- Each token embedding: 750 bits (2 bits per parameter × 3200 dimensions)

Used by `velocity_mcp --tokenize <text>` demo command.
