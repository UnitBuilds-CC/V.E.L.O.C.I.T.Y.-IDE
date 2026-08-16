# Velocity IDE Workspace Root Crate

## Classification
- **Category**: Project Root
- **Files**: Cargo.toml, README.md, AGENTS.md, PRODUCT.md, DELIVERY.md, .gitignore
- **Criticality**: High — defines workspace structure

## Summary

The workspace root defines a Cargo workspace with `resolver = "2"` containing five member crates: `velocity-mcp`, `velocity-browser`, `velocity-ide`, `drone`, and `e2e`. Excluded directories include `archive/`, `bin/`, `scratch/`, `memory/`, `.velocity/`, `browsing/`, and `fuzz/`.

## Build Configuration

```toml
[profile.dev]
debug = 1          # Prevents MSVC linker overflow

[profile.release]
strip = true
lto = "thin"
opt-level = "s"
codegen-units = 16
```

## Key Constraints

- Sub-1,000 LOC per file (hard architectural rule)
- NDA binary format is canonical (JSON is adapter only)
- SHA-256 Merkle integrity for all persisted state
- crossbeam channels for inter-thread communication (no shared mutable state)
- egui 0.35 for native GUI (no web tech stack)

## Entry Points

| File | Purpose |
|------|---------|
| `Cargo.toml` | Workspace manifest |
| `README.md` | Project overview and getting started |
| `AGENTS.md` | Agent-facing architecture guide |
| `PRODUCT.md` | Product vision and design principles |
| `DELIVERY.md` | Delivery criteria |
