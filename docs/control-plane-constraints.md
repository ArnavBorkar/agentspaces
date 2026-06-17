# Future Control Plane Constraints

This document defines constraints for any future hosted control plane, managed
sync service, dashboard, policy service, or enterprise admin surface around
`asp`.

The control plane must be additive. The local engine remains useful, auditable,
and recoverable without it.

## Principles

1. **Zero custody by default.** Source files, checkpoint objects, CAS blobs,
   diagnostics, and race logs do not leave the machine unless a user or
   organization explicitly enables a feature that sends them.
2. **Opt-in sync.** Sync is disabled until configured. Install, upgrade, init,
   checkpoint, restore, fork, race, doctor, and MCP workflows must not silently
   enroll a workspace.
3. **Exportability.** Users can export hosted data in documented formats and
   continue local recovery without the hosted service.
4. **Local truth wins.** `.asp/` remains the recovery source of truth for local
   checkpoints. Hosted state can mirror or coordinate, not replace it.
5. **No account-required local workflow.** The CLI and MCP server keep working
   offline for local operations.

## Data Custody Classes

| Data Class | Default | May Leave Machine When | Required Controls |
| --- | --- | --- | --- |
| Source files and checkpoint objects | Local only | User enables sync or uploads support artifact. | Explicit scope, encryption in transit, export, delete. |
| CAS blobs | Local only | User enables sync or backup for large files. | Integrity hashes, immutable object semantics, export. |
| Journal and provenance | Local only | User enables team audit or sync. | Redaction, retention controls, export. |
| Fork metadata | Local only | User enables dashboard or team coordination. | No source content by default, opt-out. |
| Diagnostics bundles | Local only | User explicitly uploads or attaches bundle. | Redacted default, full-path opt-in. |
| Usage metrics | Disabled | User or org explicitly enables telemetry. | Clear disable path, documented fields, no source content. |
| Billing and account data | Hosted | User signs up for hosted service. | Separate from local command authorization. |

Default means the behavior of a fresh `asp init` in this repository.

## Zero-Custody Requirements

Future hosted features must:

- start from no source custody;
- document every data class they send;
- keep source upload separate from account login;
- support metadata-only operation where practical;
- redact paths and secret-shaped values by default for support flows;
- never treat hosted data as the only copy of checkpoint history;
- allow users to delete hosted copies without damaging local `.asp/`;
- keep local recovery possible with stock git and local CAS files.

If a feature requires custody of source files, it needs an explicit design note,
threat model update, and user-facing opt-in language before implementation.

## Opt-In Sync Requirements

Sync is allowed only when configured by the user or organization. A sync design
must state:

- where objects are stored: user-owned bucket, managed bucket, local remote, or
  another target;
- which refs, git objects, CAS blobs, journals, race metadata, and diagnostics
  are included;
- whether sync is push-only, fetch-only, bidirectional, or backup-only;
- how conflicts are detected without overwriting newer local state;
- how partial uploads resume;
- how credentials are scoped, rotated, and revoked;
- how users verify a restore without the service;
- how to disable sync and confirm no new data leaves the machine.

The preferred first design is bring-your-own-bucket sync with user-controlled
credentials and immutable object writes. Managed sync can exist later, but it
must not make the BYO or local path worse.

## Exportability Requirements

Hosted features must provide export paths for user-owned data:

| Hosted Data | Export Format |
| --- | --- |
| Checkpoint metadata | JSON or JSONL matching documented schema versions. |
| Checkpoint objects | Git objects/refs or a documented bundle that restores to `.asp/shadow.git`. |
| CAS blobs | Files named by hash, matching `.asp/blobs/` semantics. |
| Audit events | JSONL with timestamps, actors, operation names, and object references. |
| Policy config | TOML or JSON schema-documented files. |
| Diagnostics | Same bundle shape emitted by `asp diagnostics`. |

Export must not depend on an active paid subscription for already-owned data.
Deletion must be documented separately from export.

## Offline Behavior

When offline, unauthenticated, rate-limited, or when the hosted service is down:

- `asp init`, `status`, `stats`, `checkpoint`, `log`, `undo`, `restore`,
  `fork`, `forks`, `diff`, `promote`, `discard`, `doctor`, `diagnostics`,
  `race`, `race compare`, and `mcp` keep working locally;
- hosted status warnings are non-fatal for local commands;
- queued sync or audit events remain local until the user retries or disables
  the feature;
- no local recovery command blocks on remote approval;
- errors include hints that let an agent decide whether to retry, disable sync,
  or continue offline.

## Control Plane Allowed Scope

A hosted control plane may:

- display health, fork, race, and audit metadata that users opted to share;
- coordinate promotion approvals without becoming the only approval record;
- distribute policy templates that local tools can evaluate;
- manage team membership, billing, SSO, support, and retention settings;
- orchestrate sync jobs configured by the user or organization;
- store exported diagnostics when users upload them.

It may not:

- become required for local recovery;
- silently upload source, blobs, diagnostics, race logs, or journal entries;
- replace documented local JSON schemas with a private API;
- prevent a user from exporting hosted data;
- make hosted refs the only checkpoint refs;
- make local `asp` behavior depend on remote feature flags without a local
  fallback.

## Design Review Checklist

Any future control-plane proposal must answer:

- What is the minimum useful local version of this feature?
- Which data classes leave the machine, and who opts in?
- What works when the service is offline?
- How does a user export and restore without the service?
- How are conflicts detected and resolved?
- What logs, metrics, or diagnostics are collected by default?
- What is the local disable path?
- Which JSON, config, or format schemas change?
- Which trust-model, backup, and governance docs must be updated?

No control-plane feature should ship until these answers are public.

## Related Docs

- [Open-core boundary policy](open-core-boundary.md)
- [Local engine governance](local-engine-governance.md)
- [Trust model whitepaper](trust-model.md)
- [Backup and disaster recovery](backup-recovery.md)
- [On-disk format](design/format.md)
- [JSON schemas](schemas.md)
