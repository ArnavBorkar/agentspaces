# Development Guide

This guide is for contributors changing `asp`. For the on-disk contract, read
[docs/design/format.md](design/format.md). For the shorter system map, read
[docs/architecture.md](architecture.md).

## Architecture Map

The workspace has two crates:

| Area | Path | Owns |
| --- | --- | --- |
| Engine | `crates/asp-core` | `.asp/` store, checkpoints, forks, restore, diff, promote, discard, doctor, diagnostics, config, journal, and shadow-git calls. |
| Binary | `crates/asp` | CLI, MCP stdio server, Claude Code hooks, human output, JSON envelopes, and `asp race`. |

Keep state-changing behavior in `asp-core`. The CLI and MCP layer should parse
inputs, call `Workspace`, and format results.

Important engine modules:

| Module | Use it for |
| --- | --- |
| `workspace.rs` | Public engine API and command orchestration. Start here for most features. |
| `store.rs` | Store layout, workspace identity, fork registry, locks, atomic writes, and path containment. |
| `gitx.rs` | Shadow-git subprocess wrapper and git environment isolation. |
| `journal.rs` | Append-only CRC journal and torn-tail recovery. |
| `fork.rs` | Platform-specific whole-tree clone/reflink/copy behavior. |
| `blobs.rs` | Large-file content-addressed sidecar and restore helpers. |
| `config.rs` | Capture excludes and `.asp/config.toml` defaults. |

Important binary modules:

| Module | Use it for |
| --- | --- |
| `main.rs` | Clap command definitions, JSON envelope output, and CLI formatting dispatch. |
| `mcp.rs` | MCP tool definitions and tool-call routing. |
| `hooks.rs` | Claude Code hook install/remove and hook-event handling. |
| `race.rs` | Fork fan-out, per-lane command execution, and race summaries. |
| `ui.rs` | Human table and terminal formatting helpers. |

## Command Map

Every user-facing command must support `--json` through the global envelope.

| Command | Core path | MCP tool | Typical tests |
| --- | --- | --- | --- |
| `asp init` | `Workspace::init` | `workspace_init` | engine init tests, CLI happy path |
| `asp status` | `Workspace::status` | `workspace_status` | engine status assertions |
| `asp stats` | `Workspace::stats` | none yet | engine stats tests, CLI JSON shape |
| `asp checkpoint` | `Workspace::checkpoint` | `workspace_checkpoint` | engine capture tests, hook/MCP provenance tests |
| `asp log` | `Workspace::log` | `workspace_log` | engine journal/log tests |
| `asp undo` | `Workspace::undo` | `workspace_undo` | engine undo tests and CLI loop |
| `asp restore` | `Workspace::restore` | `workspace_restore` | targeted/full restore tests, path-safety tests |
| `asp fork` | `Workspace::fork` | `workspace_fork` | fork independence tests, torture fork tests |
| `asp forks` | `Workspace::fork_compare` | `workspace_forks` | fork comparison tests |
| `asp diff` | `Workspace::diff` | `workspace_diff` | checkpoint/worktree diff tests |
| `asp promote` | `Workspace::promote` | `workspace_promote` | user-git isolation and branch tests |
| `asp discard` | `Workspace::discard` | `workspace_discard` | unpromoted-work guard tests |
| `asp race` | CLI `race.rs` plus fork/diff/promote primitives | none | CLI race tests |
| `asp doctor` | `Workspace::doctor` | none yet | repair/deep-check tests |
| `asp diagnostics` | `Workspace::diagnostics` | none yet | redaction and output-shape tests |
| `asp mcp` | `mcp.rs` | server entrypoint | MCP session tests |
| `asp setup claude` | `hooks.rs` | none | hook install/remove tests |

When adding a new command, also decide whether agents need it as an MCP tool. If
they do, add both the tool definition and a protocol-level test.

## Standard Dev Loop

Use rustup's cargo first if your shell has another Rust toolchain ahead of it:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Run the full gate before pushing:

```bash
cargo deny check
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Install `cargo-deny` if needed:

```bash
cargo install --locked cargo-deny
```

## Focused Test Guide

Use focused tests while iterating, then finish with the full gate.

| Change type | Start with | Finish with |
| --- | --- | --- |
| Store layout, path validation, or locks | `cargo test -p asp-core --test engine` | full gate |
| Journal parsing or corruption recovery | `cargo test -p asp-core journal` and `cargo test -p asp-core --test properties` | full gate |
| Fork/reflink/copy behavior | `cargo test -p asp-core fork` and `cargo test -p asp --test torture` | full gate |
| CLI output or JSON envelope | `cargo test -p asp --test cli` | full gate |
| MCP tools | `cargo test -p asp --test mcp` | full gate |
| Claude hooks | `cargo test -p asp --test hooks` | full gate |
| Release/dependency policy | `cargo deny check` | full gate |

The torture suite intentionally spawns real `asp` processes and kills them. It
is slower than ordinary unit tests because it protects the product's trust
model.

## Change Checklist

Before opening a PR or pushing to `main`, verify:

- The change preserves stock-git recovery for checkpoints.
- Any new mutation is atomic rename, append-only with CRC, or git's own
  tmp-and-rename behavior.
- User-facing errors include a corrective `hint`.
- Any new command has JSON output and tests for both success and failure.
- Storage changes include regression or torture coverage.
- README performance claims still match measured benchmark docs.

## Useful Local Commands

```bash
# See the command surface
cargo run -p asp -- --help

# Exercise a tiny workspace by hand
tmp="$(mktemp -d)"
cd "$tmp"
git init
echo hello > app.txt
git add app.txt
git -c user.name=asp-dev -c user.email=asp-dev@example.com commit -m init
asp init
asp checkpoint -m baseline
asp fork --name experiment
asp forks --json
asp doctor --fix

# Reproduce benchmark claims
python3 scripts/bench/run.py
```
