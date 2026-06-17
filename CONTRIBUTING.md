# Contributing to agentspaces

Thanks for caring about agent state. Contributions of every size are welcome.

## Ground rules

1. **The trust model is non-negotiable.** Every checkpoint must remain recoverable with stock git; the worst-case failure mode must degrade to a plain git repository. PRs that trade this away for speed or features will be declined, kindly.
2. **Assume `kill -9` at any line.** Store mutations must be one of: atomic rename, append-only with CRC, or git's own tmp+rename. New mutation paths need torture-suite coverage (`crates/asp/tests/torture.rs`).
3. **The user's `.git` is sacred.** Never write to it except `promote` creating ordinary branches.
4. **Agents are first-class users.** New commands need `--json`; new errors need a corrective `hint`.

## Dev loop

```bash
cargo build --workspace
cargo test --workspace          # includes the kill -9 torture suite (~20s)
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check                # install with: cargo install --locked cargo-deny
```

CI runs all of the above on macOS and Linux, plus cargo-deny dependency policy and the FICLONE reflink path on a real btrfs volume.

Dependency policy and triage are documented in [docs/dependency-governance.md](docs/dependency-governance.md).
The contributor command map and focused test guide live in [docs/development.md](docs/development.md).

## Changing the on-disk format

`.asp/format-version` is a contract. Additive JSON fields are fine; anything else needs a version bump, a migration note in [docs/design/format.md](docs/design/format.md), and discussion in an issue first.

## Benchmarks

Performance claims in the README must be reproducible: `python3 scripts/bench/run.py`. If your change moves a headline number (fork latency, incremental checkpoint), include before/after output in the PR.

## License

By contributing, you agree your contributions are dual-licensed under MIT OR Apache-2.0, like the rest of the project.
