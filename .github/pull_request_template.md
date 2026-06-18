## Summary

<!-- What changed, why, and what should reviewers focus on? -->

## Validation

- [ ] `cargo deny check`
- [ ] `cargo build --workspace`
- [ ] `cargo test --workspace`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Other focused checks: <!-- command/output summary -->

## Trust Checklist

- [ ] Checkpoints remain recoverable with stock git.
- [ ] New or changed mutations are atomic rename, append-only with CRC, or git's own tmp-and-rename behavior.
- [ ] The user's `.git` is untouched except for `asp promote` creating ordinary branches.
- [ ] Store paths read from `.asp/` are validated before joining onto the workspace root.
- [ ] Crash-sensitive behavior has regression, property, or torture coverage.
- [ ] User-facing errors include a corrective `hint`.
- [ ] User-facing commands support `--json` or the PR explains why no command surface changed.
- [ ] Serialized CLI/MCP output changes update schemas, docs, and JSON snapshots.
- [ ] Docs, README claims, and benchmark numbers were updated when behavior or claims changed.

## Risk Notes

<!-- Call out migration, crash-safety, platform, performance, or docs risk. -->
