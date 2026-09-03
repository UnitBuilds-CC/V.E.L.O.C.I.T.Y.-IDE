# Delivery Boundary

## Acceptance Route

**Check:** `cargo test` (workspace-wide, all members)
**Target branch:** `main`

All tests must pass before any commit lands on `main`.

```powershell
cargo test --workspace
```

A non-zero exit code blocks the commit.

## Recovery Route

**Affected resource:** `main` branch working tree
**Postcondition:** workspace compiles and all tests pass (`cargo test --workspace` exits 0)

If a commit introduces a test failure on `main`, revert it immediately:

```powershell
git revert HEAD --no-edit
cargo test --workspace
```

After the revert, confirm the postcondition: `cargo test --workspace` exits 0 and the workspace is in a known-good state.
