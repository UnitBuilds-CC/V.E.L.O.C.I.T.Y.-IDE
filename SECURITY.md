# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 1.0.x | Yes |
| < 1.0 | No |

## Reporting a Vulnerability

We take security vulnerabilities seriously. Please follow these guidelines:

### Do NOT

- Open a public GitHub issue for a security vulnerability
- Discuss the vulnerability publicly before it has been addressed
- Attempt to access or modify production data

### DO

- Email security concerns to the maintainers via [GitHub Security Advisories](https://github.com/UnitBuilds/Velocity-IDE/security/advisories/new)
- Provide a detailed description including:
  - Type of vulnerability (e.g., buffer overflow, path traversal, injection)
  - Steps to reproduce or proof-of-concept
  - Affected component(s)
  - Potential impact
  - Suggested fix if you have one

### Response Timeline

| Stage | Timeline |
|-------|----------|
| Initial acknowledgment | 48 hours |
| Severity assessment | 5 business days |
| Fix development | 14 business days (critical), 30 days (high), 60 days (medium/low) |
| Public disclosure | After fix is released |

## Security Architecture

Velocity IDE implements several security measures:

### Credential Boundary

- API keys and secrets are managed through `credential_guard` module
- Environment variable scrubbing prevents credential leakage in logs
- Audit logging tracks all credential access

### Path Traversal Protection

- All file operations use `canonicalize()` + prefix checking
- Symlink escapes are detected and blocked
- See `velocity-mcp::safety` for implementation details

### Sandboxed Execution

- NDA sandbox validates scope and prevents escape
- JIT compilation uses validated instruction sets only
- Worker threads operate within defined boundaries

### Supply Chain Security

- `cargo-deny` enforces license and advisory compliance
- `cargo-audit` runs in CI on every commit
- SBOM (CycloneDX) generated for every release
- Dependencies are pinned via `Cargo.lock`

## Security Best Practices for Users

1. **Keep updated**: Always use the latest release
2. **API keys**: Store in environment variables, never in code or config files
3. **Network**: Use HTTPS for all provider connections
4. **File access**: Run the IDE with minimum required filesystem permissions
5. **Dependencies**: Regularly audit your own project dependencies

## Known Security Boundaries

The following are considered out-of-scope for security reporting:

- Vulnerabilities in third-party dependencies (report upstream)
- Issues requiring physical access to the machine
- Social engineering attacks
- Denial of service from authorized users (rate limiting applies)

## Contact

For security concerns, use [GitHub Security Advisories](https://github.com/UnitBuilds/Velocity-IDE/security/advisories/new) or contact the maintainers directly.
