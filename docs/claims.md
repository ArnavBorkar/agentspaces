# README claims and evidence

This file keeps public claims traceable. When README language changes, update
the relevant row or add a new one. Claims should point to a test, reproducible
benchmark, design doc, or source file.

| Claim | Evidence | How to verify |
|---|---|---|
| `asp` is one binary containing CLI and MCP server. | `crates/asp/src/main.rs` defines CLI subcommands including `Mcp`; `crates/asp/src/mcp.rs` implements the stdio server. | `cargo build --workspace && target/debug/asp --help` |
| The engine never uses the user's `.git` for checkpoints. | `.asp/shadow.git` is documented in [format.md](design/format.md); `user_git_dir_never_captured` covers it. | `cargo test -p asp-core user_git_dir_never_captured` |
| `asp promote` creates an ordinary branch and does not move HEAD. | Promote behavior is covered by engine and CLI tests. | `cargo test promote` |
| Checkpoints are recoverable with stock git. | Recovery runbook is documented in [format.md](design/format.md) and tested. | `cargo test -p asp-core stock_git_recovery_runbook_works` |
| The journal self-heals a torn tail and rejects non-tail corruption. | Journal unit tests and property tests cover truncation and corruption behavior. | `cargo test -p asp-core journal` |
| Crash recovery is exercised with real process kills. | `crates/asp/tests/torture.rs` runs kill-9 sweeps over checkpoint, fork, and restore. | `cargo test -p asp --test torture` |
| Forks copy the whole physical directory and preserve runnable state. | Fork implementation is in `crates/asp-core/src/fork.rs`; fork and compare tests exercise independence. | `cargo test -p asp-core fork` |
| Linux reflink behavior is tested on btrfs in CI. | `.github/workflows/ci.yml` mounts btrfs and checks JSON fork output for `"method": "reflink"`. | Inspect the `linux-reflink` CI job or run the same commands on btrfs. |
| Large files use a BLAKE3 content-addressed sidecar. | `crates/asp-core/src/blobs.rs` and [format.md](design/format.md) describe pointer blobs; engine tests cover restore. | `cargo test -p asp-core big_file` |
| Gitignored files are excluded from checkpoints, while forks carry the physical tree. | FAQ and SECURITY document the behavior; tests cover derived excludes. | `cargo test -p asp-core excludes_keep_derived_state_out_of_checkpoints` |
| Every command supports machine-readable JSON. | `crates/asp/src/main.rs` defines global `--json`; CLI tests exercise JSON paths indirectly through command behavior. | `target/debug/asp --json status` in an initialized workspace. |
| Errors include corrective hints. | `asp_core::error` carries `hint`; CLI tests cover actionable errors. | `cargo test -p asp errors_are_actionable_outside_workspace` |
| Claude Code setup is idempotent and reversible. | Hook setup tests cover install/remove and provenance checkpointing. | `cargo test -p asp hook` |
| Benchmarks are reproducible from this repo. | Benchmark methodology lives in [docs/benchmarks/BENCHMARKS.md](benchmarks/BENCHMARKS.md) and `scripts/bench/run.py`. | `python3 scripts/bench/run.py` |
| Release artifacts are built for macOS and Linux targets. | `.github/workflows/release.yml` defines the release matrix; [launch-checklist.md](launch-checklist.md) records v0.1.0 smoke tests. | Inspect release workflow logs for the tagged release. |
| The project is fully local with no telemetry. | SECURITY documents the local-only model; source search should show no network clients in runtime code. | `rg -n "http|telemetry|analytics|reqwest|ureq|curl" crates` |

## Maintenance rule

Before adding a new headline claim to the README, decide which of these buckets
supports it:

- automated test;
- reproducible benchmark;
- source file reference;
- design/format documentation;
- explicitly documented caveat.

If none apply, add evidence first or soften the claim.
