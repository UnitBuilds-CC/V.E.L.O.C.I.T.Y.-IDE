---
name: Pull Request
about: Submit changes for review
title: ""
labels: ''
assignees: ''
---

## Summary

Brief description of what this PR does.

## Type of Change

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to change)
- [ ] Documentation update
- [ ] Refactoring (no functional changes)
- [ ] Performance improvement
- [ ] CI/CD or build changes

## Checklist

- [ ] `cargo fmt --all` — formatting passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings
- [ ] `cargo test --workspace` — all tests pass
- [ ] Added/updated tests for new code
- [ ] Updated documentation (doc comments, README, or docs/)
- [ ] Followed [CONTRIBUTING.md](../CONTRIBUTING.md) guidelines

## Related Issues

Closes #(issue number)

## Testing

How was this tested? (unit tests, integration tests, manual testing)
