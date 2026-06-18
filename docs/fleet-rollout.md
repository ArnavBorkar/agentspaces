# Fleet Rollout Checklist

Use this checklist when introducing `asp` to many repositories. The goal is to
make rollout boring: reviewed config, reversible setup, CI evidence, and a clear
support path before broad adoption.

The checklist is optimized for 10+ repos where manual memory and one-off setup
stop scaling.

## Phase 0: Pick The Pilot Shape

Start with 2-3 repositories that represent the fleet:

- one normal service repository;
- one large monorepo or multi-package repository;
- one repository with generated code, media assets, or large binaries.

For each pilot, record:

- repository owner and escalation channel;
- default branch and required CI checks;
- agent harnesses in use;
- expected test command;
- generated directories and caches;
- paths that require human review before promote;
- closest [organization policy bundle](policy-packs.md), if the team wants
  reviewed fork, path, promote, and retention defaults;
- filesystem type for common developer machines and CI runners.

## Phase 1: Review The Template

Print the candidate template before creating `.asp/`:

```bash
asp init --print-template service
asp init --print-template monorepo
asp init --print-template generated-code
asp init --print-template media-heavy
```

Review that the template only uses `capture.extra_excludes`, not
`capture.excludes`, so the built-in derived-state exclusions remain active.
Then initialize with the selected template:

```bash
asp init --template monorepo
asp config validate
asp --json config show > asp-config.json
asp --json config diff --against baseline.toml > asp-config-diff.json
```

Keep `asp-config.json` and, when a fleet baseline exists,
`asp-config-diff.json` with the rollout ticket for review.

## Phase 2: Smoke Test Locally

Run this sequence in a disposable branch or throwaway clone:

```bash
asp preflight
asp checkpoint -m "rollout: baseline"
asp fork --name rollout-smoke
asp -C ../<repo>@rollout-smoke checkpoint -m "rollout: fork checkpoint"
asp discard rollout-smoke --force
asp doctor --deep
```

Record:

- whether `asp preflight` is ready;
- checkpoint latency on first and second checkpoint;
- fork method from `asp --json fork --name rollout-method`;
- any doctor findings;
- any paths that were unexpectedly excluded or included.

## Phase 3: Add Read-Only CI

Start with non-mutating checks:

```bash
asp config validate
asp --json preflight > asp-preflight.json
asp preflight --sarif > asp-preflight.sarif || true
asp evidence collect --output asp-evidence.json
asp evidence manifest \
  --packet asp-evidence.json \
  --output asp-evidence.manifest.json
asp evidence verify \
  --packet asp-evidence.json \
  --manifest asp-evidence.manifest.json
```

Upload the JSON, SARIF, packet, manifest, and verification log as CI artifacts
for pilots. Do not run `asp doctor --fix`, `asp restore`, `asp promote`, or
`asp discard` in CI rollout gates.

## Phase 4: Harness Setup

Install harness integrations only after the read-only checks are green:

```bash
asp setup claude
asp setup codex
asp setup opencode
```

For each harness:

- capture the before and after config diff with `asp config diff --against <file>`;
- verify the remove path in a temporary HOME when possible;
- run `asp status` after a small agent edit;
- confirm auto-checkpoints include session or tool provenance where supported.

## Phase 5: Roll Out In Rings

Use rings instead of a single fleet-wide change:

| Ring | Scope | Gate |
| --- | --- | --- |
| 0 | Maintainer-owned pilot repos | Manual smoke test plus read-only CI. |
| 1 | 5-10 willing teams | Preflight ready, support channel staffed. |
| 2 | Business-critical repos | Backup plan for `.asp/`, recovery drill passed. |
| 3 | Default recommendation | Docs, issue templates, and rollback notes published. |

Do not advance a ring while any crash-safety, restore, promote, or source
exposure report is unresolved.

## Rollback

Rollback must not delete evidence by default:

```bash
asp setup claude --remove
asp setup codex --remove
asp setup opencode --remove
asp doctor --deep
```

If the team wants to remove `.asp/`, first preserve or explicitly discard the
local audit trail:

```bash
asp evidence collect --deep --output asp-evidence.json
asp evidence manifest \
  --packet asp-evidence.json \
  --output asp-evidence.manifest.json
asp evidence verify \
  --packet asp-evidence.json \
  --manifest asp-evidence.manifest.json
```

Only delete `.asp/` after repository owners confirm no checkpoints, forks,
audit events, or support evidence need to be retained.

## Done When

A repository is considered rolled out when:

- `asp config validate` passes in CI;
- `asp preflight` is ready or has documented exceptions;
- the selected template and any local edits have an owner;
- at least one checkpoint, fork, discard, and doctor smoke test has passed;
- the support ticket template has an evidence packet, manifest, and verification
  log for the pilot;
- rollback instructions are linked from the repository's internal docs.

## Related Docs

- [Config templates](config-templates.md)
- [CI preflight examples](ci.md)
- [Evidence packets](evidence.md)
- [Support ticket templates](support-ticket-templates.md)
- [Backup and disaster recovery](backup-recovery.md)
