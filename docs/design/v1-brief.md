# agentspaces v1 — Product Brief

*Decided June 10, 2026. Derived from the multi-lens analysis in [lens-analysis.json](lens-analysis.json), grounded in [the agent-infra research report](../agent-infra-research-archil-mesa-code.md). This is the working source of truth for the build.*

## North star

> Agentspaces is the open-source, local-first state engine that turns every agent session into an instant, disposable, fully-reviewable fork of your real working directory — fork is control flow, the checkpoint journal is the audit log, and promote is the only way work lands.

State is the noun. Compute is a verb. Branches are control flow. Commits are the audit log.

## Founder decisions (locked)

| Fork | Decision | Consequence |
|---|---|---|
| Endgame | **Venture path** | Raise on the wedge thesis within ~3 months of launch. Instrument 90-day metrics from day one (installs, weekly actives, forks/week, retention, hosted waitlist) — they are both the traction story and the pitch. Hosted-tier design starts on paper early; benchmarks and the race demo double as pitch material. |
| Runway | **12+ months** | Pure OSS through 2026. BYO-bucket sync ships as a free feature. Monetize from strength in 2027 (or post-raise). No premature paid tier. |
| License | **MIT/Apache open core** | Engine and on-disk format permissively licensed forever. The open-core boundary is declared in the README at launch: hosted sync/control plane is proprietary. No rug-pulls. |
| Harness bet | **Claude-first, hedged** | Deep Claude Code integration (hooks + MCP) in v1; the CLI/MCP contract stays strictly harness-neutral underneath; Codex/OpenCode port is a dated milestone (~week 6). |
| Custody | **Zero custody until revenue funds ops** (default) | v1 and the sync tier custody no bytes (BYO S3/R2, user's keys, thin stateless control plane at most). Capability-token fields exist in the API from day one, unenforced. A true hosted tier waits for a team. |

## The wedge

Archil, Mesa, and Pierre sell cloud infrastructure to platforms. Nobody serves **the engineer running 3–8 parallel Claude Code/Codex sessions** who duct-tapes `cp -r`, shell scripts, and git worktrees today:

- Harness checkpoints (`/rewind`) cover only the model's own edits and die with the session.
- Git worktrees cover only git-tracked files.
- **Unserved: durable cross-session timelines, whole-directory forks (untracked files, `.env`, build artifacts included), and undo of agent bash side-effects.**

Distribution is the channel no incumbent occupies: `claude mcp add agentspaces`, hooks, the MCP registry. Open source is simultaneously the distribution strategy, the trust strategy, and the defense against bundling.

Nearest collision: **Turso AgentFS** — a local SQLite-file substrate. We differentiate as the git-native workflow/version layer over *real directories and real repos*, sync-ready to the user's own cloud. The seam they have announced intentions toward but not shipped is exactly our target: speed is a strategy.

## The product

A single static binary, **`asp`**, simultaneously a CLI and an MCP stdio server. macOS (APFS clonefile) and Linux (btrfs/XFS reflink; degraded fallback elsewhere). No FUSE, no daemon, no account, no telemetry, no server.

**Core loop:**

```
asp init                  # adopt any directory or git repo without disturbing it (sidecar store)
asp fork -n 3             # O(1) CoW forks of the WHOLE working directory, sub-second on multi-GB trees
                          # auto-checkpoint around every agent change (Claude Code hooks make
                          # bash side-effects rewindable — the documented gap in /rewind)
asp diff                  # cross-fork side-by-side comparison table
asp promote <fork>        # winner lands as an ordinary git branch/PR
asp discard               # losers vanish
asp race "<prompt>" -n 3  # the whole loop packaged: the killer demo
asp undo / asp log        # durable cross-session timeline
```

**Engine:** shadow-git capture layer — plain git plumbing behind a `Workspace` trait, FastCDC/BLAKE3 sidecar for large blobs, write-batching, periodic repack. Everything recoverable with stock git by design: **worst-case failure degrades to a plain git repo.** That sentence is the trust model. The on-disk format is content-addressed and sync-ready from day one. An append-only journal maps each checkpoint to the tool call, session, and prompt that caused it.

**MCP surface:** `workspace_fork / checkpoint / diff / promote / undo / log` with agent-legible descriptions, `--json` on every CLI command, error messages that state the corrective next action. The agent is a first-class user.

**Quality gates (non-negotiable, ship with v1):**
- `kill -9` crash-safety torture suite running in CI — data loss is a one-strike kill for a storage tool.
- Published honest benchmarks: fork latency + disk overhead on a 5 GB dirty 100k-file monorepo vs `cp -r` and `git worktree`. Fork + status must be sub-second or the pitch dies in the first demo.
- 90-second README demo; "why not git worktrees / why not AgentFS" positioning docs.

## Explicitly OUT of v1

Hosted service or custody of any bytes · BYO-bucket sync (first post-v1 milestone, weeks 5–8) · FUSE/NFS/mounts · sandbox exec adapters (E2B/Modal/Daytona) · token enforcement · Windows · web UI · versioned-memory feature · session-manager UI · non-coding verticals. A minimal run-wrapper (record command, exit code, duration into the journal + auto-checkpoint) is the only exec footprint in v1.

## Milestones

- **Week 1 — kill the existential risks first.** Benchmark clonefile/reflink whole-directory fork + mtime-indexed status on a real 5 GB/100k-file dirty monorepo (target sub-second, published numbers). Spike the shadow-git capture layer (untracked files, write-batching, large-blob sidecar) proving stock-git recoverability. Scaffold `asp init/fork/checkpoint/diff/discard` behind the `Workspace` trait. One-page format doc locking in content-addressed, sync-ready objects.
- **Week 2 — demoable alpha inside Claude Code, dogfooded on agentspaces itself.** Hooks auto-checkpointing around every Bash/Edit call; MCP server with `workspace_*` verbs; `--json` everywhere; corrective-action errors. From here on, agentspaces is built using agentspaces.
- **Week 3 — complete the loop, earn the trust claims.** Promote-to-git, durable cross-session timeline + undo, `asp race -n N` end-to-end, torture suite in CI. Recruit 5–10 Claude Code power users + 2–3 eval/fleet engineers as weekly design partners; co-design the diff table with the latter.
- **Week 4 — polish the first five minutes, soft-launch.** brew/npx/curl install, README demo, published benchmarks, positioning docs, declared open-core boundary. Ship to the cohort, fix what they hit, prep public HN/X launch for weeks 5–6.
- **Post-v1 (named, dated):** Codex/OpenCode port (~week 6 — an open Codex feature request for exactly this exists). Zero-server BYO-S3/R2 sync via conditional-write ref CAS (weeks 5–8). Fundraising motion from ~month 2 on the wedge thesis + 90-day metrics.

## Top risks

1. **Anthropic absorption** — one release note covering bash side-effects + untracked files compresses the wedge. Mitigation: the hedge (cross-harness, cross-machine, git-interop layers are what a harness vendor won't build); Codex port on a date, not a vibe.
2. **Performance claim fails** — if fork+status isn't sub-second on big dirty trees, reposition before building further. That's why it's week 1.
3. **Trust** — any data-loss incident is fatal. Stock-git recoverability + torture suite + zero custody are the mitigations; never use the words "production-grade" publicly until the suite, benchmarks, and 4+ weeks of dogfooding exist (~weeks 8–10).
4. **Turso ships first** — AgentFS moves toward durability/git-interop. Mitigation: pace, and owning the git-native seam they architecturally lack.
