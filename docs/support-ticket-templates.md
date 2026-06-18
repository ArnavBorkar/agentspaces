# Support Ticket Templates

Use these templates when an `asp` evidence packet leaves a workstation, CI
runner, or private incident channel. They keep support requests useful without
normalizing source uploads, full local paths, or unverified attachments.

Prefer the smallest template that answers the question. Attach only the listed
artifacts unless the reviewer asks for more and explains the retention and
access boundary.

## Public Issue

Use this for public GitHub issues where the report can be understood with
redacted local evidence.

````md
## Summary

- affected command or workflow:
- expected behavior:
- observed behavior:
- first failing version, if known:

## Environment

- asp version:
- git version:
- OS and filesystem:
- workspace type: parent workspace / fork / CI checkout
- install method:

## Reproduction

```bash
# Minimal command sequence. Remove secrets and private paths.
```

## Evidence Artifacts

- evidence packet: `asp-evidence.json`
- manifest: `asp-evidence.manifest.json`
- manifest verification: `asp evidence verify --packet asp-evidence.json --manifest asp-evidence.manifest.json`
- signature: not attached publicly / Sigstore bundle / minisign signature
- SARIF, if relevant: `asp-preflight.sarif` / `asp-secrets.sarif`

## Redaction Statement

- `redaction.paths_redacted`: true / false
- `redaction.secrets_redacted`: true / false
- `audit_messages_included`: true / false
- `audit_details_included`: true / false
- I did not attach source archives, fork directories, `.asp/` backups, tokens,
  shell history, or full local paths.

## Impact

- data loss risk: yes / no / unknown
- restore blocked: yes / no
- `.git` touched unexpectedly: yes / no
- workaround:
````

## Private Support Incident

Use this for a private vendor, internal platform, or enterprise support ticket.
It can include richer context, but every escalation should still be deliberate.

````md
## Incident

- customer/team:
- severity:
- incident start time:
- business impact:
- affected repository or service:
- support contact:

## Current State

- failing command:
- last successful command:
- workspace currently openable with `asp status`: yes / no
- `asp doctor --deep` result:
- recovery attempted:

## Evidence Bundle

- packet: `asp-evidence.json`
- manifest: `asp-evidence.manifest.json`
- verification result: pass / fail
- verification command:
  `asp evidence verify --packet asp-evidence.json --manifest asp-evidence.manifest.json`
- signature type: Sigstore / minisign / none
- signature artifact: `asp-evidence.manifest.sigstore.json` / `asp-evidence.manifest.minisig`
- signer identity or public key:
- SARIF artifacts:
- related PR, branch, run, or ticket:

## Redaction And Sharing Boundary

- packet collected with `--include-paths`: yes / no
- packet collected with `--deep`: yes / no
- audit messages included: yes / no
- audit details included: yes / no
- source, fork directories, or `.asp/` backup attached: yes / no
- approved support audience:
- retention expectation:
- delete request path:

## Requested Help

- diagnose only / recovery guidance / root-cause analysis / rollout advice
- commands support is allowed to suggest:
- commands support is allowed to run during a remote session:
- commands support must not run:
````

## Security Or Sensitive Data Report

Use a private security advisory or internal security system for vulnerability
reports, suspected source exposure, path traversal, credential exposure, or
reports that require exploit details.

````md
## Security Summary

- vulnerability class:
- affected version or commit:
- affected command or integration:
- attacker capabilities required:
- suspected exposure: source / path / credential / local store / hosted service

## Minimal Private Reproduction

```bash
# Keep this minimal. Use a synthetic repository whenever possible.
```

## Evidence Chain

- evidence packet: `asp-evidence.json`
- manifest: `asp-evidence.manifest.json`
- verification command:
  `asp evidence verify --packet asp-evidence.json --manifest asp-evidence.manifest.json`
- verification result: pass / fail
- signature type: Sigstore / minisign / none
- signature artifact:
- signer identity or public key:

## Redaction And Disclosure

- packet redacts paths: true / false
- packet redacts secrets: true / false
- exploit details included: yes / no
- private source included: yes / no
- credentials included: no
- coordinated disclosure constraints:

## Remediation Need

- requested embargo:
- affected downstream systems:
- suggested fix, if known:
- safe workaround:
````

## CI Evidence Handoff

Use this when a failing CI run needs support review and the job can upload
artifacts. Keep the artifact set explicit so reviewers can validate integrity
before opening the packet.

````md
## CI Context

- repository:
- branch or pull request:
- workflow and run URL:
- runner OS:
- asp version:
- failing job:

## Uploaded Artifacts

- `asp-evidence.json`
- `asp-evidence.manifest.json`
- signature: `asp-evidence.manifest.sigstore.json` / `asp-evidence.manifest.minisig`
- `asp-preflight.sarif`
- `asp-secrets.sarif`, if a secrets scan is part of the gate
- normal test logs:

## Integrity Check

```bash
asp evidence verify \
  --packet asp-evidence.json \
  --manifest asp-evidence.manifest.json

cosign verify-blob \
  --bundle asp-evidence.manifest.sigstore.json \
  --certificate-identity-regexp "<expected signer identity>" \
  asp-evidence.manifest.json

# Or, for offline teams:
minisign -Vm asp-evidence.manifest.json \
  -x asp-evidence.manifest.minisig \
  -P "<public key>"
```

## Redaction Expectations

- public CI artifacts must keep paths redacted unless the repository owner has
  approved full local paths;
- secrets scan output must be redacted and SARIF-only;
- CI should never upload source archives, fork directories, `.asp/` backups,
  environment dumps, or shell history as part of the default evidence bundle.
````

## Collection Commands

Start with the default redacted packet:

```bash
asp evidence collect --output asp-evidence.json
asp evidence manifest \
  --packet asp-evidence.json \
  --output asp-evidence.manifest.json
asp evidence verify \
  --packet asp-evidence.json \
  --manifest asp-evidence.manifest.json
```

Add deep store checks when support needs CAS or doctor evidence:

```bash
asp evidence collect --deep --output asp-evidence.json
```

Use full paths only in a private channel with an explicit retention expectation:

```bash
asp evidence collect --include-paths --output asp-evidence.json
```

## Maintainer Intake

When a ticket arrives, ask for missing evidence in this order:

1. redacted evidence packet;
2. manifest and passing `asp evidence verify` result;
3. signature bundle or minisign signature, when the packet crossed a trust
   boundary;
4. SARIF artifacts for CI, preflight, or secrets questions;
5. narrowly scoped private artifacts with a stated reason and retention window.

Do not ask for source archives, fork directories, `.asp/` backups, full local
paths, or remote shell access until the redacted packet cannot answer the
question.
