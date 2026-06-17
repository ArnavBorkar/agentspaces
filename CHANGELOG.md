# Changelog

## Unreleased

### Added

- `asp race` accepts repeated `--label` flags and templated `--env KEY=VALUE`
  variables for per-lane agent configuration.
- `asp race` accepts `--timeout`, `--retries`, and `--cancel-on-success` runner
  controls for bounded best-of-N agent work.
- `asp race --resume --name <race>` records and resumes interrupted races from
  `.asp/races/<race>.json`.
- `asp race --junit <path>` ingests per-lane JUnit XML reports and summarizes
  test outcomes in JSON results.
- `asp race compare --name <race>` re-ranks saved race lanes without rerunning
  lane commands.
- `docs/evaluation.md` gives teams a 30-minute pilot guide with explicit
  success criteria and go/no-go signals.
- `docs/playbooks.md` documents repeatable agent workflows for bug-fix fleets,
  test generation, docs generation, and CI repair.
- `docs/trust-model.md` gives security reviewers a whitepaper for local trust
  boundaries, command write behavior, residual risk, and evidence links.
- `docs/backup-recovery.md` gives operators a `.asp/` backup, restore drill,
  and disaster-recovery runbook.
- `docs/monorepo-tuning.md` documents large-repository tuning for checkpoint
  excludes, blob thresholds, and filesystem choice.
- `docs/open-core-boundary.md` defines non-negotiable OSS guarantees and the
  allowed boundary for optional hosted services.
- `docs/local-engine-governance.md` gives maintainers a review workflow for
  deciding which future features must remain in the local engine.
- `docs/control-plane-constraints.md` sets zero-custody, opt-in sync, offline
  behavior, and exportability requirements for any future hosted control plane.
- `docs/local-first-team-workflows.md` drafts local audit, policy, and approval
  workflows that do not require a hosted control plane.
- `docs/enterprise-support-boundaries.md` defines support and SLA boundaries
  that avoid mandatory telemetry, accounts, or source custody.
- MCP `initialize` now reports asp capability metadata, and `tools/list` entries
  include model-facing safety annotations guarded by snapshots.
- MCP JSON-RPC handling now rejects malformed request ids and bad `tools/call`
  params with protocol errors while continuing to serve later requests.
- MCP tool errors now include structured `code`, `message`, and `hint` fields,
  with stable recovery guidance documented in `docs/mcp-error-codes.md`.
- MCP tool descriptions now tell models when not to call risky state-changing
  tools such as undo, restore, promote, and discard.
- MCP now has a replayable transcript snapshot that guards concise tool
  descriptions and actionable error responses.
- New workspaces now include `.asp/policy.toml` with strict parsing, schema
  docs, helpful errors, and enforcement for active fork limits, checkpoint age,
  protected paths, and promote requirements.
- `asp policy validate --json` validates workspace policy and returns the
  resolved policy in a stable automation-facing shape.
- `asp audit` shows filtered local journal events by session, tool, operation,
  path, and RFC3339 time range.
- `asp audit --format jsonl` and `asp audit --format csv` export filtered audit
  events for compliance evidence and downstream review tools.
- Checkpoint journal entries now include changed-path attribution under
  `detail.paths`, so `asp audit --path <path>` can find checkpoint events.
- `asp retention plan` reads local retention policy and prints a dry-run
  checkpoint retention plan without deleting refs or objects.
- Retention planning retains the latest checkpoint and active fork-point
  checkpoints even under aggressive `keep_last` policy.
- CI now publishes a non-blocking lightweight benchmark baseline artifact on
  every run for performance trend review.

### Fixed

- Checkpoint staging preserves case-only path renames on case-insensitive
  filesystems by removing stale index spellings before adding the real path.
- The fork kill-9 torture sweep is less timing-fragile on very fast APFS
  runners while still requiring a real killed fork process.
- Shadow-git commands retry transient index-lock collisions left by killed git
  subprocesses during crash recovery.

### Automation contract

- Additive: `asp race --json` lane result objects include `label` when emitted
  by this version. Existing lane fields are unchanged.
- Additive: `asp race --json` lane result objects include `attempts`,
  `timed_out`, and `canceled` runner metadata. Existing lane fields are
  unchanged.
- Additive: `asp race --json` lane result objects may include `tests` when
  JUnit report ingestion is configured. Existing lane fields are unchanged.
- Additive: `asp race compare --json` lane result objects include `rank` after
  saved lanes are re-sorted for review. Existing lane fields are unchanged.
- Additive: `asp audit --json` returns the existing journal-entry array shape
  used by `asp log --json`; clients can opt in by calling the new command.
- Additive: `asp audit --format jsonl` emits one journal-entry JSON object per
  line, and `asp audit --format csv` emits a fixed-column CSV export.
- Additive: checkpoint journal entries may include `detail.paths`, an array of
  workspace-relative changed paths for audit filtering and exports.
- Additive: `asp retention plan --json` returns `retentionPlan`, and
  `asp policy validate --json` includes `policy.retention`.

## Automation Contract Rules

CLI `--json` envelopes, CLI result payloads, MCP `structuredContent`, MCP tool
errors, and `error.code` values are automation contracts. Any release that
changes them must include an **Automation contract** note.

Classify changes this way:

- **Additive:** new optional fields, new commands or tools, new result variants,
  new schema entries, or additional diagnostic detail that existing clients can
  ignore. Changelog note: list the field/tool/schema and whether clients need to
  opt in.
- **Breaking:** removed or renamed fields, changed field types, changed
  nullability, changed envelope shape, changed closed enum values, changed
  meaning of an existing value, or different MCP tool error wrapping. Changelog
  note: put it under **Breaking**, name the old and new shapes, include a
  migration step, and bump the affected schema version reported by
  `asp schema --json`.

Every automation-contract change must update [docs/schemas.md](docs/schemas.md),
the files under [schemas/](schemas/), and the JSON snapshot test baselines in
`crates/asp/tests/snapshots/`.

## v0.1.1 — 2026-06-11

**Fix (restore correctness on case-insensitive filesystems):** full restore now deletes
outgoing paths *before* materializing the target. Previously, restoring across a
case-only rename (checkpoint has `L/a`, working tree has `l/a` — the same file on
default macOS APFS) could let the deletion pass remove the freshly restored file.
Found by our own property tests in CI within hours of v0.1.0; deterministic
regression test added. Also hardened the property-test strategy to model
case-insensitive filesystem semantics correctly.

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
