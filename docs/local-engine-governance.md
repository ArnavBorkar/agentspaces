# Local Engine Governance

This note explains how maintainers should decide whether a feature belongs in
the open-source local engine, an optional local integration, or a future hosted
service.

Use it with the [open-core boundary policy](open-core-boundary.md). The boundary
policy says what cannot be crossed. This note describes how to make feature
decisions before implementation starts.

## Default Rule

If a capability is required to create, inspect, recover, verify, or promote
local agent work, it belongs in the open-source local engine.

Hosted services may coordinate around that capability, but they should not be
the only way to use it.

## Feature Classes

| Class | Definition | Default Location |
| --- | --- | --- |
| Core local engine | Required for local workspace state, recovery, review, or automation. | This repo, MIT OR Apache-2.0. |
| Optional local integration | Adapts the engine to an editor, harness, CI, or agent without taking custody. | This repo when broadly useful, plugin when specialized. |
| Hosted companion | Adds team coordination, dashboards, approvals, managed sync, support, or billing. | Separate service, optional and additive. |
| Enterprise service | SLA, onboarding, migration, training, policy consulting, or support operations. | Commercial service, no local engine dependency. |

When a feature spans classes, split it so the local primitive remains open and
the hosted layer only adds coordination or convenience.

## Must Remain Local

These feature families are part of the local engine contract:

- `.asp/` layout, migrations, config, shadow git, journal, CAS, race metadata,
  and fork registry handling;
- checkpoint, restore, undo, fork, diff, promote, discard, doctor, diagnostics,
  stats, schema, and log behavior;
- MCP stdio server tools that expose local workspace operations to agents;
- hook and harness integrations needed to checkpoint agent edits locally;
- `asp race` execution, lane logs, saved race comparison, and local result
  ingestion;
- JSON output, error codes, corrective hints, and automation schemas;
- stock-git recovery docs and tests;
- crash-safety, path-containment, dependency policy, and release-verification
  gates.

Changing any item above requires normal code review, tests, and docs in this
repository. It cannot be implemented only in a proprietary service.

## Review Questions

Before accepting a feature proposal, answer:

- Does the feature read or mutate `.asp/`, the workspace tree, or user `.git/`?
- Is it needed to recover from agent damage or machine failure?
- Is it needed to review, compare, promote, or discard agent work?
- Would an MCP client or script need this to operate without a human UI?
- Would removing the hosted service make the local workflow incomplete?
- Does the feature introduce telemetry, account state, remote policy, or hosted
  custody?
- Can the feature be split into an open local primitive plus an optional hosted
  workflow?

If any of the first four answers is yes, start with an open local primitive.
If either of the last two answers is yes, document the boundary before writing
code.

## Required Proposal Metadata

Issues or design docs for boundary-sensitive features should include:

- **Class:** core local engine, optional local integration, hosted companion, or
  enterprise service.
- **Local primitive:** command, API, schema, or file format that remains usable
  without a service.
- **Hosted value:** what the service adds beyond the local primitive.
- **Failure mode:** what still works when offline, unauthenticated, or after the
  hosted service disappears.
- **Data custody:** whether source, metadata, diagnostics, or race results leave
  the machine, and how users opt in.
- **Tests/docs:** which trust, JSON, or recovery tests and docs must change.

Small changes can include this in the PR description. Larger changes should get
a design note under `docs/design/`.

## Decision Outcomes

Use these outcomes during triage:

| Outcome | Meaning |
| --- | --- |
| Local-first | Build the full primitive in the OSS engine before any hosted layer. |
| Split | Build an OSS primitive plus an optional hosted coordinator. |
| Hosted-only | Allowed only when the feature is accounts, billing, support, or team convenience with no local recovery dependency. |
| Defer | Boundary is unclear; write a design note before code. |
| Reject | Feature would make local workflows less capable, less recoverable, or hosted-dependent. |

## Examples

| Feature Idea | Governance Decision |
| --- | --- |
| Retention policy that prunes old checkpoints. | Local-first. It mutates recovery state and needs tests proving safety. |
| Organization dashboard for active forks. | Split. Local fork metadata remains available; hosted view is optional. |
| SAML login for a hosted admin console. | Hosted-only. It does not gate local commands. |
| Remote approval required before `asp promote`. | Split or reject. Local promote must still work; hosted approvals may annotate or advise. |
| Sync protocol for user-owned object storage. | Local-first. Sync affects recovery and should remain user-custodied. |
| Fleet health report that phones home by default. | Reject. Telemetry must be explicit opt-in. |

## Documentation And Audit Trail

Boundary decisions should be discoverable:

- update [BACKLOG.md](../BACKLOG.md) when a planned feature changes class;
- update [CHANGELOG.md](../CHANGELOG.md) when a released feature touches the
  boundary;
- link new design notes from the relevant public docs;
- keep automation schema changes in [docs/schemas.md](schemas.md);
- update the [trust model whitepaper](trust-model.md) if the trust boundary
  changes.

The goal is not bureaucracy. The goal is to keep the local engine boring,
recoverable, and safe to adopt even if commercial services grow around it.

## Related Docs

- [Open-core boundary policy](open-core-boundary.md)
- [Future control plane constraints](control-plane-constraints.md)
- [Trust model whitepaper](trust-model.md)
- [Development guide](development.md)
- [JSON schemas](schemas.md)
- [Backup and disaster recovery](backup-recovery.md)
