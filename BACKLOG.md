# agentspaces — Product Backlog

*Living document. Updated continuously as tasks complete and learning arrives. Source of truth for scope is [docs/design/v1-brief.md](docs/design/v1-brief.md).*

**Status legend:** `[ ]` pending · `[~]` in progress · `[x]` done · `[-]` dropped (reason noted)

---

## EPIC 0 — Foundation

**PM intent:** A repo a stranger (or a future Claude session) can pick up cold: builds in one command, CI green on every push, conventions written down.
**Done when:** `cargo build` works from a fresh clone, CI runs fmt+clippy+tests on macOS+Linux, licenses and contributor docs exist.

- **S0.1 Toolchain & scaffold**
  - [x] T0.1.1 Install Rust stable toolchain (1.96.0)
  - [x] T0.1.2 Cargo workspace: `crates/asp-core` (engine lib), `crates/asp` (binary: CLI + `asp mcp` stdio server in one static binary)
  - [x] T0.1.3 rustfmt + clippy config, .gitignore
  - [x] T0.1.4 Dual-license MIT/Apache-2.0 files + per-crate license fields
  - [x] T0.1.5 CLAUDE.md: build/test commands, conventions, gh-account note
- **S0.2 CI**
  - [~] T0.2.1 GitHub Actions: fmt + clippy -D warnings + test on macos-latest & ubuntu-latest (pushed; verifying first run)
  - [ ] T0.2.2 CI badge in README (with README rewrite, EPIC 7)

## EPIC 1 — De-risk spikes (existential — nothing else proceeds if these fail)

**PM intent:** The two claims the whole pitch rests on, proven with published numbers before we invest in features: (1) whole-directory CoW fork + status is sub-second on a big dirty tree; (2) every byte is recoverable with stock git.
**Done when:** docs/benchmarks/spike-results.md has honest numbers from this machine, and a written go/reposition decision.

- **S1.1 CoW fork benchmark** ✅
  - [x] T1.1.1 Generator: synthetic monorepo (100,026 files, 3.28 GiB)
  - [x] T1.1.2 clonefile(dir) **919ms/32MB** vs cp -R 27s/3.7GB vs worktree 13.8s — 15x win
  - [x] T1.1.3 Change-detection scan: 263ms warm on 100k files
  - [x] T1.1.4 docs/benchmarks/spike-results.md — **verdict: GO**
- **S1.2 Shadow-git capture spike** ✅
  - [x] T1.2.1 Sidecar GIT_DIR captures untracked files; user .git untouched (PASS)
  - [x] T1.2.2 Incremental checkpoint **462ms**; initial 66s → mitigations decided (excludes, blob sidecar, capture-on-first-checkpoint)
  - [x] T1.2.3 Stock-git restore byte-identical (PASS); runbook = read-tree + checkout-index
  - [x] T1.2.4 Blob policy: >50MB → BLAKE3 CAS sidecar via clonefile + pointer in shadow git (6 format decisions recorded in spike-results.md)

## EPIC 2 — Core engine (`asp-core`)

**PM intent:** The trust-bearing layer. Boring, correct, crash-safe. Everything recoverable with stock git is the product's one-sentence trust model.
**Done when:** all engine ops have unit+integration tests, crash-recovery test passes, format doc matches implementation.

- **S2.1 Store layout & format doc** ✅ (format.md authoritative; ops behind Workspace API — backend swap = internal refactor)
- **S2.2 init / adopt** ✅ (guarded, never touches user .git; default derived-state excludes + config overrides)
- **S2.3 Checkpoint engine** ✅ (no-op skip handles hook storms; provenance metadata: source/session/tool; large-blob CAS sidecar w/ pointer manifests at refs/asp/meta/<seq>)
- **S2.4 Journal** ✅ (CRC lines, fsync, torn-tail self-heal; mid-file corruption surfaced via doctor, never dropped)
- **S2.5 Fork** ✅ (clonefile/FICLONE/copy-fallback; registry; fork-of-fork works; CoW independence tested)
- **S2.6 Timeline** ✅ (linear undo-stack semantics: restore appends safety + post checkpoints; dirty-undo vs clean-undo)
- **S2.7 Diff** ✅ (checkpoint↔checkpoint, checkpoint↔worktree, N-way fork compare; duration/test markers land with `asp race`, EPIC 3)
- **S2.8 Promote / discard** ✅ (plumbing-only commit + local fetch; no HEAD moves, no user hooks, no force-push; unpromoted-work guard)
- **S2.9 Crash safety** ✅ (advisory lock, atomic renames everywhere, doctor detects/repairs torn forks, tampered head, missing CAS blobs — kill -9 torture matrix lands in EPIC 6)

## EPIC 3 — CLI (`asp`)

**PM intent:** The first five minutes ARE the product. Every command: fast, beautiful output, `--json` for agents, errors that state the corrective next action.
**Done when:** the demo loop (`init → fork -n 3 → ... → diff → promote → discard`) feels great, help text teaches the model, all commands have --json.

- **S3.1 CLI scaffold**
  - [ ] T3.1.1 clap derive scaffold; global `--json`; exit-code contract; error type with `hint:` corrective actions
  - [ ] T3.1.2 Human output polish: tables, colors (respect NO_COLOR), progress for long ops
- **S3.2 Core commands**
  - [ ] T3.2.1 `init`, `status`, `checkpoint`, `log`, `undo`, `restore`
  - [ ] T3.2.2 `fork [-n N] [--label]`, `forks` (list), `diff [forks…|checkpoints…]`, `promote`, `discard`
- **S3.3 `asp race`**
  - [ ] T3.3.1 `race "<cmd>" -n N`: fork N ways, run command in each fork (parallel), capture exit/duration, render comparison table, prompt promote/discard
  - [ ] T3.3.2 Works headlessly (`--json`, no TTY) for agent use
- **S3.4 Help & completions**
  - [ ] T3.4.1 Rich `--help` with examples per command; shell completions; man page

## EPIC 4 — MCP server

**PM intent:** The agent is a first-class user; MCP is the distribution channel (`claude mcp add agentspaces`). Tool descriptions are prompts — write them like product copy for models.
**Done when:** all workspace tools callable from Claude Code, descriptions tested with a real agent session, errors self-correcting.

- **S4.1 Server**
  - [ ] T4.1.1 `asp mcp` stdio server (official Rust MCP SDK if mature, else minimal JSON-RPC impl) — same binary
- **S4.2 Tools**
  - [ ] T4.2.1 `workspace_status/checkpoint/log/undo/diff/fork/promote/discard` with agent-legible descriptions + corrective-action errors
  - [ ] T4.2.2 Real-session test: drive every tool from Claude Code; fix description ambiguities the model trips on

## EPIC 5 — Claude Code integration (hedged: contract stays harness-neutral)

**PM intent:** Auto-checkpoint around every agent change — the `/rewind`-for-everything experience, zero config after one command.
**Done when:** `asp hooks install` wires PostToolUse auto-checkpoints with session/prompt correlation; verified in a real session; uninstall clean.

- **S5.1 Hooks**
  - [ ] T5.1.1 `asp hooks install [--scope project|user]`: writes Claude Code PostToolUse (Edit/Write/Bash) hook → `asp checkpoint --auto` with session id; debounced; `hooks uninstall`
  - [ ] T5.1.2 Journal correlation: session id, tool name, prompt summary captured per auto-checkpoint
- **S5.2 Packaging**
  - [ ] T5.2.1 `.mcp.json` template + `claude mcp add` one-liner docs; quickstart for Claude Code users
  - [ ] T5.2.2 Codex/OpenCode port notes (named milestone post-v1; contract kept neutral now)

## EPIC 6 — Quality gates (trust artifacts)

**PM intent:** Storage tools get one strike. The torture suite and honest benchmarks ARE marketing.
**Done when:** kill-9 suite green in CI; BENCHMARKS.md published with methodology + this-machine numbers; property tests on journal/store.

- **S6.1 Torture suite**
  - [ ] T6.1.1 kill -9 matrix: kill asp mid-checkpoint/fork/promote at random points (incl. SIGKILL storms); workspace must recover with zero loss of *checkpointed* data; runs in CI
- **S6.2 Benchmarks**
  - [ ] T6.2.1 Reproducible bench harness (`asp bench` or scripts/): fork latency, checkpoint latency, status scan, disk overhead vs cp -R/worktree; BENCHMARKS.md with methodology
- **S6.3 Cross-platform matrix**
  - [ ] T6.3.1 Linux CI: btrfs/XFS reflink path + ext4 fallback; macOS APFS in CI
- **S6.4 Property tests**
  - [ ] T6.4.1 proptest: journal recovery (arbitrary truncation/corruption), checkpoint/restore round-trip invariants

## EPIC 7 — Docs, packaging, launch readiness

**PM intent:** Out-of-the-box: one install command, 90 seconds to wow. Positioning docs preempt the two obvious objections.
**Done when:** fresh-machine install → demo works following README only; release pipeline produces signed-ish artifacts; open-core boundary declared.

- **S7.1 README & demo**
  - [ ] T7.1.1 README: hero pitch, 90-second quickstart, demo GIF/asciinema, architecture sketch, trust section (stock-git recovery)
- **S7.2 Positioning**
  - [ ] T7.2.1 docs/why-not-git-worktrees.md · docs/why-not-agentfs.md · FAQ
- **S7.3 Install paths**
  - [ ] T7.3.1 curl installer script (install.sh, checksummed), `cargo install asp-cli`
  - [ ] T7.3.2 Homebrew formula (tap) + npx wrapper package (downloads platform binary)
- **S7.4 Release automation**
  - [ ] T7.4.1 GitHub Actions release workflow: tag → build macOS(arm64,x86_64)+Linux binaries → GitHub Release with checksums
- **S7.5 OSS hygiene**
  - [ ] T7.5.1 CONTRIBUTING.md, SECURITY.md, CODE_OF_CONDUCT.md, issue templates; open-core boundary declared in README
  - [ ] T7.5.2 Launch checklist (repo public flip = Arnav's call; HN/X draft)

## EPIC 8 — Dogfood & final review

**PM intent:** We feel every rough edge before users do. agentspaces is built with agentspaces from the moment the alpha exists.
**Done when:** asp manages this repo's own development; first-five-minutes walkthrough passes on a clean simulated setup; adversarial multi-agent review finds no release-blocking issues.

- [ ] T8.1 Dogfood: asp init on this repo; use checkpoints/forks during remaining development
- [ ] T8.2 Fresh-machine first-five-minutes walkthrough (clean clone, README only)
- [ ] T8.3 Multi-agent adversarial review (correctness, data-safety, UX, docs) + fix wave
- [ ] T8.4 Final benchmark + torture run; tag v0.1.0

---

## Decision log (newest first)

- 2026-06-11 · Single binary `asp` contains CLI + MCP server (`asp mcp`) — one artifact to install, per first-five-minutes PM goal.
- 2026-06-11 · Journal = append-only JSONL w/ per-line CRC, not SQLite — simpler crash-safety story, greppable, zero deps. Revisit if query needs grow.
- 2026-06-11 · Engine backend = git plumbing subprocess behind `Workspace` trait for v1 — stock-git recoverability is the trust model; gitoxide/custom CAS only when profiling demands.
- 2026-06-10 · Locked founder forks: venture path · 12+ mo runway · MIT/Apache open core · Claude-first hedged · zero custody (see v1-brief).
