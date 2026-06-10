# agentspaces — development notes

`asp` is a single static Rust binary: a CLI + MCP stdio server giving AI agents durable,
branchable, fully-reviewable workspaces over real directories. Source of truth for product
scope: [docs/design/v1-brief.md](docs/design/v1-brief.md). Live plan: [BACKLOG.md](BACKLOG.md).

## Build & test

```bash
export PATH="$HOME/.cargo/bin:$PATH"   # rustup-installed toolchain
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Layout

- `crates/asp-core` — engine library: store, shadow-git backend, journal, fork, diff, promote.
  No CLI/IO-formatting concerns here.
- `crates/asp` — the `asp` binary: clap CLI + `asp mcp` stdio server. Human and `--json` output.

## Conventions

- Trust model is non-negotiable: every checkpoint recoverable with stock git; worst-case
  failure degrades to a plain git repo. Never design that away.
- All store mutations: atomic rename or append-only with CRC; assume `kill -9` at any line.
- User-facing errors must state the corrective next action ("hint:" line). Agents are
  first-class users: every command supports `--json`; error text should let an LLM self-correct.
- The user's own `.git` is sacred — never write to it except `promote` creating ordinary
  branches; never force-push, never rewrite user history.

## GitHub

Use the **ArnavBorkar** account (`gh auth switch --user ArnavBorkar` if needed — the machine's
default is the work account). Commit identity is configured repo-locally. Push to
https://github.com/ArnavBorkar/agentspaces (private until launch flip).
