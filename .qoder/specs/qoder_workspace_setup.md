# Velocity IDE — .qoder Configuration and Knowledge Base

## Summary

Implement the `.qoder` directory structure for the Velocity IDE workspace, matching the pattern established in the Dwarven Stronghold project. This provides Qoder with project-specific knowledge, skills, and documentation context for all three primary crates (velocity-mcp, velocity-browser, velocity-ide).

---

## Structure Created

```
.qoder/
├── repowiki/
│   ├── en/
│   │   ├── content/
│   │   │   ├── Getting Started.md
│   │   │   ├── Development Guide.md
│   │   │   ├── Troubleshooting & FAQ.md
│   │   │   ├── Core Concepts & Architecture/
│   │   │   │   ├── Workspace Architecture Overview.md
│   │   │   │   └── NDA Binary Format and Security Model.md
│   │   │   ├── Agent & Orchestration/
│   │   │   │   └── 4-Provider Agent Reasoning Loop.md
│   │   │   ├── Browser Engine & Rendering/
│   │   │   │   └── Browser Engine Architecture.md
│   │   │   ├── Compiler & NDA Pipeline/
│   │   │   │   └── NDA Compiler and JIT Pipeline.md
│   │   │   ├── Editor & IDE UI/
│   │   │   │   └── Editor IDE UI Architecture.md
│   │   │   └── API Reference & Development Interface/
│   │   │       └── Tool Registry and Windows Automation.md
│   │   ├── meta/
│   │   │   └── repowiki-metadata.json
│   │   └── knowledge/
│   │       └── en/
│   │           ├── _index.yaml
│   │           ├── Velocity IDE Workspace Root Crate/
│   │           ├── Velocity MCP Server and IDE Editor/
│   │           ├── Velocity Browser Engine/
│   │           ├── Velocity IDE Compiler and NDA Pipeline/
│   │           ├── 4-Provider Agent Reasoning Loop with Automatic Failover/
│   │           ├── NDA Binary Format 18-Byte Triple with SHA-256 Merkle Integrity/
│   │           ├── Sub-1k LOC File Size Constraint Architecture Rule/
│   │           ├── egui 0.35 Native Immediate-Mode GUI with Dark HSL Theme/
│   │           ├── Windows UI Automation FFI via COM Interfaces/
│   │           └── Rust Cargo Workspace with Dual WASM Target Build/
├── skills/
│   ├── rust-code-review/
│   │   └── SKILL.md
│   └── agent-test-writer/
│       └── SKILL.md
└── specs/
    └── qoder_workspace_setup.md (this file)
```

---

## Key Decisions

1. **Content pages cover all major subsystems**: Architecture, agent loop, browser engine, compiler, editor UI, tool registry, WA
2. **Knowledge cards capture critical constraints**: Sub-1k LOC rule, NDA format, provider chain, build config
3. **Skills adapted for Velocity**: Rust code review with project-specific high-risk areas, behavior test writer with Velocity patterns
4. **Up-to-date**: All file paths, module counts, and architectural details verified against current codebase (August 2026)

---

## Maintenance

When the codebase changes significantly:
1. Update affected content pages in `repowiki/en/content/`
2. Update knowledge cards if module boundaries shift
3. Update `_index.yaml` if new modules are added
4. Update skills if new patterns emerge or conventions change
