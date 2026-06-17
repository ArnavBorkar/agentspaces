# Open-Core Boundary Policy

This policy defines what must remain open source in `agentspaces` and what may
be built as optional hosted or commercial services later.

The short version: the local engine is the product's trust anchor. It must not
be weakened to create a hosted upsell.

## Non-Negotiable OSS Guarantees

These guarantees apply to this repository:

1. The `asp` CLI, MCP stdio server, engine library, and on-disk format remain
   dual-licensed MIT OR Apache-2.0.
2. Existing open-source code is never relicensed into a more restrictive
   license.
3. Local checkpoint, restore, undo, fork, diff, promote, discard, race, doctor,
   diagnostics, config, and MCP workflows do not require an account, network
   call, telemetry opt-in, or hosted control plane.
4. Every checkpoint remains recoverable with stock git and local `.asp/` files.
5. The user's own `.git/` remains outside `asp` ownership except for `promote`
   creating ordinary branches.
6. Public automation contracts, including CLI JSON envelopes and MCP tool
   shapes, remain documented and testable in the open repo.
7. Release verification, dependency policy, CI gates, and trust tests remain
   visible to users and contributors.
8. Hosted features must be additive. They must not remove local functionality,
   gate local commands, or make the local engine less recoverable.

If a proposed change violates any guarantee above, it does not belong in this
repository.

## What Must Stay In The Open Repo

The following capabilities are core, not premium gates:

| Area | Must Remain OSS |
| --- | --- |
| Storage | `.asp/` layout, shadow-git backend, journal, CAS blobs, config schema, migrations. |
| Recovery | Stock-git recovery runbook, restore, undo, doctor, diagnostics, deep integrity checks. |
| Workspace control | init, status, stats, checkpoint, log, fork, forks, diff, promote, discard. |
| Agent workflow | MCP stdio server, hook integration, `asp race`, lane logs, saved race metadata, compare. |
| Automation | `--json` output, published schemas, actionable error codes and hints. |
| Supply chain | Build/test commands, CI workflows, dependency policy, release checksums, signatures, SBOMs. |
| Documentation | Trust model, on-disk format, configuration, evaluation, operations, and recovery docs. |

Commercial services may improve coordination around these capabilities, but the
local versions above remain usable without those services.

## Allowed Hosted Or Commercial Surfaces

A future paid or hosted product may provide:

- managed team dashboards over user-approved metadata;
- hosted approval workflows for promotion or policy review;
- managed sync or backup for teams that explicitly opt in;
- organization policy distribution;
- SSO, billing, audit export, retention management, and support workflows;
- fleet reporting about installed versions and health when explicitly enabled;
- hosted issue triage, CI integration, or race-result visualization;
- enterprise support, SLAs, training, and migration assistance.

These services may charge money. They may not become prerequisites for local
use.

## Disallowed Boundary Crossings

Do not merge changes that:

- require a server token to run local checkpoint, restore, fork, promote, race,
  doctor, diagnostics, or MCP workflows;
- hide the on-disk format or make `.asp/` unreadable without proprietary code;
- move the only implementation of local recovery, integrity checking, or JSON
  automation behind a hosted service;
- add mandatory telemetry, crash upload, usage tracking, or remote policy
  checks to local commands;
- remove stock-git recovery in favor of a proprietary export path;
- make a hosted service the source of truth for local checkpoint history;
- weaken CI, torture tests, dependency policy, or release verification to speed
  up commercial development;
- add an open-source feature that is intentionally incomplete so a hosted
  service can make it usable.

## Boundary Review Checklist

Every feature that touches sync, policy, telemetry, accounts, hosting,
licensing, or custody needs an explicit boundary review:

- Can a user still run the local workflow with no account and no network?
- Can a user recover checkpointed files using stock git and `.asp/` only?
- Does the change preserve `--json` output and documented schemas for agents?
- Does any new server-side concept have a local no-op or local-first fallback?
- Does the change keep user source files out of hosted custody by default?
- Are telemetry, sync, and backup opt-in with clear disable paths?
- Does documentation state what remains open source and what is hosted?
- Would an enterprise security reviewer still be able to audit the recovery
  path from this repository alone?

If the answer is unclear, the default decision is to keep the capability local
and open until the boundary is documented.

## Examples

| Proposal | Boundary Decision |
| --- | --- |
| Add `asp sync push` to a user-owned S3 bucket. | OSS candidate if credentials stay user-owned and local recovery remains. |
| Hosted dashboard that reads uploaded diagnostics bundles. | Allowed if upload is explicit and local diagnostics remain useful. |
| Require account login before `asp race`. | Disallowed. Race is core local agent workflow. |
| Hosted promotion approvals that annotate branches. | Allowed if `asp promote` still creates local branches without the service. |
| Proprietary `.asp` compactor needed for restore. | Disallowed. Recovery must remain open and stock-git compatible. |
| Enterprise SLA and support portal. | Allowed. Support is a service, not a local-engine dependency. |

## Maintainer Commitments

Maintainers should keep this policy current when product direction changes.
When in doubt:

- preserve the local engine first;
- document hosted boundaries before implementation;
- prefer user-owned sync and backup paths over custody;
- keep trust tests in the open repo;
- reject convenience features that make failure recovery less boring.

The business can sell convenience, coordination, support, and hosted operations.
It cannot sell back the local safety layer that made users trust `asp` in the
first place.

## Related Docs

- [Trust model whitepaper](trust-model.md)
- [On-disk format](design/format.md)
- [Backup and disaster recovery](backup-recovery.md)
- [JSON schemas](schemas.md)
- [Release verification](release-verification.md)
- [Dependency governance](dependency-governance.md)
