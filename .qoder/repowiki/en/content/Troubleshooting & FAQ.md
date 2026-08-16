# Troubleshooting & FAQ

## Build Issues

### MSVC Linker Errors (LNK1189 / LNK1201)
**Cause**: Debug symbols too large for MSVC section limits.
**Fix**: The workspace uses `debug = 1` in `[profile.dev]` to prevent this. Do not override to `debug = true` or `debug = 2`. If you see linker errors, verify your `Cargo.toml` profile settings.

### "cannot find -lwindows" or UIA FFI Errors
**Cause**: Missing Windows SDK.
**Fix**: Install the Windows 10/11 SDK via Visual Studio Installer. The `wa/` module requires UI Automation COM interfaces.

### Workspace Check Fails with "unresolved import"
**Cause**: Missing crate dependency or incorrect feature flag.
**Fix**: Run `cargo check --workspace` from the workspace root. Verify `Cargo.toml` dependency declarations.

### Slow Compile Times
**Tips**:
- Use `cargo check` instead of `cargo build` for fast typechecking
- The release profile uses `codegen-units = 16` for parallel codegen
- Consider `sccache` for build caching

## Runtime Issues

### Agent Loop Fails with "no provider available"
**Cause**: No LLM provider configured or all providers are down.
**Fix**: Check `.env` file has at least one provider configured. Verify API keys are valid.

### Browser Engine Crashes on Navigation
**Cause**: Network connectivity or TLS handshake failure.
**Fix**: The custom TLS 1.3 stack is an engineering artifact. Check `velocity-browser/src/net/` for TLS diagnostics. Verify network connectivity.

### egui Window Not Rendering
**Cause**: GPU driver incompatibility with `egui` renderer.
**Fix**: Update GPU drivers. Try running with software rendering if available.

### NDA File Corruption
**Cause**: Schema mismatch between writer and reader versions.
**Fix**: NDA format uses Merkle integrity chains. If corruption is detected, the file should be regenerated. Never manually edit `.nda` files.

## Test Issues

### Tests Fail on First Run
**Cause**: Some integration tests require network access or specific environment setup.
**Fix**: Run `cargo test --workspace` from workspace root. Check individual crate tests with `cargo test -p <crate>`.

### Fuzz Targets Won't Compile
**Cause**: Fuzz targets are in a separate Cargo workspace (`fuzz/Cargo.toml`).
**Fix**: Run from the `fuzz/` directory: `cargo fuzz run <target>`.

## FAQ

**Q: Why is the browser engine pure Rust instead of using Chromium?**
A: The pure-Rust browser engine is an engineering artifact demonstrating that a full browser engine can be implemented without CDP or Chromium dependency. It enables agent-first browser control with native performance.

**Q: Why NDA format instead of JSON?**
A: NDA provides compact 18-byte records with SHA-256 Merkle integrity. JSON is supported as an import/export adapter only. The binary format is ~10x more compact and includes built-in tamper detection.

**Q: Why is every file under 1,000 LOC?**
A: The sub-1k LOC rule enforces clean module isolation. It prevents god-files, makes code review manageable, and ensures each module has a single clear responsibility.

**Q: Can I use a different LLM provider?**
A: The 4-provider chain (Cloudflare → OpenRouter → Azure → Ollama) supports automatic failover. Adding a new provider requires implementing the provider trait in `velocity-mcp/src/agent/provider.rs`.

**Q: How do I run the IDE without an LLM?**
A: The IDE can run without an LLM for code editing and browsing. Agent features require at least one configured provider. Local Ollama provides offline capability.

**Q: What is the drone crate?**
A: `drone` is a safety monitor process that watches for unsafe operations and provides a safety boundary for agent actions.
