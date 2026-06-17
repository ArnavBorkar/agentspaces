<div align="center">

# asp · agentspaces

**Instant, disposable, fully-reviewable forks of your real working directory — built for AI agents.**

[![CI](https://github.com/ArnavBorkar/agentspaces/actions/workflows/ci.yml/badge.svg)](https://github.com/ArnavBorkar/agentspaces/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

*Fork is control flow. The checkpoint journal is the audit log. Promote is the only way work lands.*

![asp demo: undo agent damage, race 3 forks, promote the winner](docs/assets/demo.gif)

</div>

---

Coding agents change files faster than you can review them — and not just tracked files. They run bash, regenerate artifacts, delete things, scribble over files you never committed. Harness checkpoints (like `/rewind`) only cover the model's own edits and die with the session. Git worktrees only carry tracked files. **The state your agents actually produce has no safety net.**

`asp` is that safety net, as one static binary:

- **`asp fork -n 3`** — three copy-on-write clones of your *entire* directory (untracked files, `.env`, `node_modules`, build artifacts — literally everything), each instantly runnable. Typical repos fork in well under a second; a 100k-file, 3.3 GiB monorepo takes ~1.2s end-to-end and ~32 MB of disk.
- **Auto-checkpoints** — with the Claude Code integration, every file edit and bash command is captured with session/tool provenance. `asp undo` reverts agent damage that `/rewind` can't see.
- **`asp race -n 3 -- claude -p "fix the failing test"`** — run the same task in N parallel lanes, get an exit/time/diff comparison table, promote the winner.
- **`asp promote <fork>`** — the winner lands as an ordinary git branch. No force-pushes, no HEAD moves, no hooks run. Review it like any PR.

## Quickstart (90 seconds)

```bash
# install (macOS / Linux)
curl -fsSL https://raw.githubusercontent.com/ArnavBorkar/agentspaces/main/install.sh | sh

cd your-project
asp init                      # instant; touches nothing
asp checkpoint -m "baseline"  # capture everything except gitignored files

# wire up Claude Code (hooks + MCP) — one command
asp setup claude

# now run agent sessions; every change is checkpointed automatically
asp log                       # the timeline: which session/tool caused what
asp undo                      # step back, including bash side-effects

# the fan-out workflow
asp race -n 3 -- claude -p "make the test suite pass"
asp forks                     # side-by-side comparison
asp promote race-2            # winner → git branch asp/race-2
asp discard race-1 && asp discard race-3
```

## How it works

Two primitives with deliberately different scopes:

| | **Fork** | **Checkpoint** |
|---|---|---|
| What | whole physical tree (literally everything) | source files: tracked + untracked, minus `.gitignore`d and derived-state excludes |
| How | `clonefile(2)` on macOS, reflink on Linux | shadow git repo in `.asp/` (your `.git` is never touched) |
| Cost | ~O(inode count), bytes are shared CoW | sub-second incremental; no-op when nothing changed |
| For | running agents in parallel, instant safety copies | timeline, undo, diff, audit, promote |

Files larger than 50 MB skip git entirely: they're BLAKE3-hashed into a content-addressed sidecar via CoW clone (instant, zero-copy) with small pointer files in their place.

### The trust model

Storage tools get one strike, so `asp` is built to be boring:

1. **Everything is recoverable with stock git.** Checkpoints are ordinary commits in an ordinary (shadow) git repo. If `asp` vanished tomorrow: `GIT_DIR=.asp/shadow.git git log --all` and restore with `read-tree` + `checkout-index`. The recovery runbook is [documented](docs/design/format.md) and executed literally by a test in CI.
2. **A kill -9 torture suite runs in CI** — it SIGKILLs real `asp` processes mid-checkpoint/fork/restore across a sweep of delays and then proves: checkpointed data is never lost, the store always opens, and `asp doctor --fix` repairs anything torn.
3. **Property tests** guarantee the journal recovers the longest valid prefix from truncation at *any* byte and never fabricates entries from corruption.
4. **Your repo is sacred.** `asp` never writes to your `.git` except `promote`, which only ever creates a new branch via a local fetch.

## Agents are first-class users

- Every command takes `--json` and returns a stable `{ok, result|error}` envelope; errors carry a machine-readable `code` and a `hint` stating the corrective next action.
- `asp mcp` is a built-in MCP server (`claude mcp add agentspaces -- asp mcp`, or let `asp setup claude` wire it): `workspace_fork`, `workspace_checkpoint`, `workspace_undo`, `workspace_diff`, `workspace_promote`, and friends — with descriptions written for models.
- The journal records *which session and tool caused every change* — provenance is the audit log.

## Benchmarks

Honest numbers from the stress tree (100k files / 3.3 GiB, including 3 GiB of binary assets), reproducible with `python3 scripts/bench/run.py` — see [docs/benchmarks/BENCHMARKS.md](docs/benchmarks/BENCHMARKS.md) and the original [spike results](docs/benchmarks/spike-results.md). Highlights (Apple M3 Pro): whole-tree fork **~1.2s / 32 MB extra disk** vs `git worktree add` ~10s (tracked files only) vs `cp -R` 26s / 3.7 GB; incremental checkpoint **0.7s on the 100k-file stress tree, ~0.3s on typical repos**; no-op checkpoint (the hook idle path) **~0.25s**.

Public claims are mapped to tests, docs, and verification commands in [docs/claims.md](docs/claims.md).

## Why not …?

- **[Why not git worktrees?](docs/why-not-git-worktrees.md)** — worktrees carry tracked files only, need a clean index dance, and give you no cross-session timeline of agent changes.
- **[Why not AgentFS / a virtual filesystem?](docs/why-not-agentfs.md)** — asp versions your *real* directory: your editor, your toolchain, and `git` keep working unchanged, and nothing is trapped in a database file.
- **FAQ** — [docs/FAQ.md](docs/FAQ.md)
- **Architecture** — [docs/architecture.md](docs/architecture.md)

## Install

```bash
# script (macOS arm64/x86_64, Linux x86_64/aarch64 — static musl builds on Linux)
curl -fsSL https://raw.githubusercontent.com/ArnavBorkar/agentspaces/main/install.sh | sh

# from source
cargo install --git https://github.com/ArnavBorkar/agentspaces asp
```

Requires `git` ≥ 2.32 on PATH (asp uses it as its storage engine — that's the trust model, not a shortcut).

Release checksum signatures can be verified with Sigstore; see [docs/release-verification.md](docs/release-verification.md).

## Project status & open-core boundary

`asp` is young software under active development; the format is versioned (`.asp/format-version`) and the trust suite runs on every commit. **The engine, CLI, MCP server, and on-disk format in this repository are MIT/Apache-2.0 forever — they will never be relicensed.** If a hosted offering ever exists (e.g. managed sync/control plane), it will be a separate proprietary service; nothing in this repo will be moved behind it.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
