# agentspaces v1 Product Brief

*Updated June 17, 2026. This brief is the public source of truth for the
project's product direction. It is written from first principles for
agentspaces, not as a clone or wrapper of any other system.*

## North Star

Agentspaces is the open-source, local-first state engine that turns every
agent session into an instant, disposable, fully-reviewable fork of your real
working directory.

Fork is control flow. The checkpoint journal is the audit log. Promote is the
only way work lands.

## The Problem

Coding agents mutate more than tracked source files. They run shell commands,
regenerate assets, create untracked files, delete local state, and modify
artifacts that never appear in a normal git diff. Existing workflows leave
teams with an awkward choice:

- plain git, which is excellent for reviewed source history but blind to much
  of the live workspace;
- git worktrees, which are useful for tracked code but do not copy the whole
  runnable directory;
- ad hoc `cp -r` backups, which are slow, hard to review, and easy to lose;
- harness-local rewind features, which can disappear with the session and do
  not cover every file-system side effect.

Agents need a state layer that is local, inspectable, crash-resilient,
agent-friendly, and recoverable with boring tools.

## Product Principles

1. **Local first.** The user's directory is the source of truth. No daemon,
   account, telemetry, hosted dependency, or custody is required.
2. **Real directories.** Editors, language servers, package managers, test
   runners, and git keep working unchanged.
3. **Whole-workspace forks.** A fork includes the runnable physical tree:
   untracked files, build outputs, dependencies, local config, and secrets.
4. **Checkpointed source state.** The journal captures tracked and untracked
   non-ignored source-of-truth files into a shadow git repo.
5. **Stock-git recovery.** Every checkpoint must be restorable with ordinary
   git commands. Worst-case failure degrades to a plain git repo.
6. **Crash safety by construction.** Store mutations use atomic rename,
   append-only records with CRC, or git's own object/ref writes.
7. **Agents are first-class users.** Every command supports JSON output, and
   every actionable error includes the next corrective step.
8. **Human approval remains explicit.** Agent work lands through `promote`,
   which creates normal reviewable git branches.

## The Product

`asp` is a single static binary containing both a CLI and an MCP stdio server.
It creates durable branchable workspaces over ordinary directories on macOS and
Linux.

Core loop:

```bash
asp init
asp checkpoint -m "baseline"
asp fork -n 3
asp race -n 3 -- claude -p "make the tests pass"
asp diff
asp promote race-2
asp undo
asp doctor --fix
```

Primary surfaces:

- CLI commands for humans and scripts;
- JSON output for automation and agents;
- MCP tools for model-facing workspace operations;
- Claude Code hooks for automatic checkpoints;
- a documented on-disk format for recovery, review, and future sync.

## Technical Strategy

The engine uses two deliberately separate primitives:

- **Forks** use copy-on-write directory cloning (`clonefile(2)` on macOS,
  reflink on Linux when available, copy fallback elsewhere) to create runnable
  sibling workspaces quickly.
- **Checkpoints** use a shadow git repository under `.asp/` to capture
  source-of-truth file state without touching the user's `.git`.

Large files are stored in a BLAKE3 content-addressed sidecar and represented in
the shadow repo by pointer blobs. The journal records provenance for each
operation so teams can answer which session, tool, or command caused a change.

## Quality Gates

Storage tools get one strike. v1 quality is defined by evidence, not claims:

- `cargo build --workspace` works from a fresh checkout;
- fmt, clippy, and tests pass on macOS and Linux;
- a btrfs CI job verifies the Linux reflink path;
- kill-9 torture tests exercise checkpoint, fork, and restore recovery;
- property tests cover journal truncation/corruption behavior;
- benchmark methodology and numbers are reproducible from this repo;
- the stock-git recovery runbook is documented and tested;
- user-facing errors carry corrective hints;
- docs are honest about gitignored files, secrets, unsupported platforms, and
  external side effects.

## v1 Scope

In scope:

- local CLI and MCP server;
- init, status, stats, checkpoint, log, undo, restore, fork, forks, diff,
  promote, discard, doctor, race;
- Claude Code setup and hook integration;
- public docs, install script, release artifacts, and trust tests;
- permissive MIT/Apache-2.0 licensing for the engine, CLI, MCP server, and
  on-disk format.

Out of scope for v1:

- hosted custody of user files;
- a web control plane;
- Windows support;
- FUSE/NFS mounts;
- sandbox-provider adapters;
- built-in agent execution beyond running the command passed to `asp race`;
- enforced capability tokens;
- multi-device sync.

## Post-v1 Direction

The next phase is about enterprise-grade adoption without compromising the
local-first trust model:

- multi-harness integrations for Codex, OpenCode, Cursor, and generic MCP
  clients;
- bring-your-own-bucket sync with user-controlled credentials;
- team policy, audit, retention, and approval workflows;
- richer diff and review ergonomics for multi-agent work;
- signed releases, package managers, SBOMs, and supply-chain hardening;
- large-repo performance work guided by public benchmarks;
- issue templates, design docs, and contribution paths that make outside
  contributors successful.

## Strategic Boundary

The open-source repository remains the complete local engine and on-disk
format. If a hosted service ever exists, it must be additive and optional:
sync, coordination, policy, or observability for teams that want it. It must
not make the local project less capable, less recoverable, or less trustworthy.
