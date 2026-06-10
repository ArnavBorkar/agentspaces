# Changelog

## v0.1.0 — 2026-06-11

First release. `asp` is a single static binary giving AI agents durable, branchable, fully-reviewable workspaces over real directories.

### Core

- **Instant whole-tree forks**: `clonefile(2)` on macOS APFS, FICLONE reflink on Linux btrfs/XFS, copy fallback elsewhere. A 100k-file / 3.3 GiB monorepo forks in ~1.2s with ~32 MB of extra disk — untracked files, deps, and build artifacts included.
- **Checkpoint timeline**: shadow-git capture of the source tree (commit-grained, provenance-stamped), incremental in well under a second, no-op-free. `undo`/`restore` with automatic safety checkpoints; targeted path restore; `diff` between any two points; N-way fork comparison.
- **Large-blob sidecar**: files over 50 MB (configurable) live once in a BLAKE3 content-addressed store via CoW clone, with pointer blobs in git — multi-GB assets don't bloat checkpoints.
- **Promote**: a fork's work lands as an ordinary git branch via plumbing-only commit + local fetch. No HEAD moves, no user hooks, no force-pushes, and the `.asp` store is never staged.
- **`asp race -n N -- <cmd>`**: fork N lanes, run the command in each in parallel, compare exit/time/diff, promote the winner.

### Agent integration

- **MCP server built in** (`asp mcp`): 11 `workspace_*` tools with model-facing descriptions and self-correcting errors.
- **Claude Code hooks** (`asp setup claude`): every file edit and bash command auto-checkpointed with session/tool provenance; `--remove` reverses cleanly; hook handler never breaks a session.
- `--json` on every command with a stable `{ok, result|error}` envelope; error `code` + corrective `hint` on every failure.

### Trust artifacts

- Every checkpoint recoverable with stock git (runbook documented in `docs/design/format.md` and executed literally by a CI test).
- kill -9 torture suite in CI: SIGKILL sweeps across checkpoint/fork/restore; checkpointed data is never lost, the store always opens, `doctor --fix` repairs torn state.
- Property tests: journal recovers the longest valid prefix from truncation at any byte; corruption never fabricates entries; checkpoint/restore round-trips arbitrary trees.
- Fork creation uses intent journaling (Pending registry entries): `asp doctor` never deletes a directory it cannot prove asp created.
- Store-supplied paths are validated against traversal; a corrupt or malicious `.asp` store cannot write or delete outside the workspace.
- Pre-release adversarial review: 28 agents across 5 dimensions; all confirmed critical/major findings fixed with regression tests ([findings archive](docs/design/review-findings-v0.1.json)).

### Platforms

macOS (arm64, x86_64) and Linux (x86_64, aarch64 — static musl builds). Requires git ≥ 2.32. Windows not yet supported.
