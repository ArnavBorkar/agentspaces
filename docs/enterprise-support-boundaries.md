# Enterprise Support And SLA Boundaries

This draft defines how enterprise support, response targets, and paid services
can exist around `asp` without adding mandatory accounts, telemetry, or source
custody to the local engine.

Support is a relationship and process layer. It must not become a requirement
for local checkpoint, fork, restore, promote, doctor, diagnostics, race, or MCP
workflows.

## Principles

- Local `asp` works without an account.
- Telemetry is disabled unless a user or organization explicitly enables it.
- Diagnostics are customer-controlled artifacts, not automatic uploads.
- Support should ask for the smallest evidence bundle that can answer the
  question.
- Source upload, full paths, race logs, shell history, and remote access are
  explicit opt-ins.
- SLA language covers support response and hosted-service availability, not a
  promise that local filesystems, agents, or user commands cannot fail.

## Support Surfaces

| Surface | Allowed | Not Allowed |
| --- | --- | --- |
| Community issues | Public bug reports with redacted diagnostics. | Requesting secrets, private source, or full paths by default. |
| Enterprise support | Private tickets, response targets, onboarding, migration help. | Requiring local CLI login for support eligibility. |
| Security response | Private vulnerability handling and coordinated disclosure. | Publicly requesting exploit details or customer source. |
| Hosted services | Optional dashboards, support portal, managed sync, admin console. | Making local recovery depend on hosted uptime. |
| Professional services | Training, rollout design, policy templates, incident review. | Installing always-on telemetry without opt-in. |

## Evidence Collection

Default support packet:

```bash
asp --version
asp doctor --deep
asp diagnostics --output asp-diagnostics.json
asp --json status > asp-status.json
asp --json stats > asp-stats.json
```

Add only when relevant:

- `asp --json log -n 50` for timeline questions;
- `asp forks` or `asp race compare --name <race>` for race/fork questions;
- filesystem probe output from `findmnt` or `diskutil`;
- minimal reproduction repository;
- failing command output with secrets removed;
- redacted test logs.

Avoid by default:

- unredacted diagnostics with full paths;
- source archives;
- fork directories;
- race logs containing prompts or model output;
- environment variables;
- shell history;
- credentials or tokens;
- remote-desktop access.

If support needs any avoided artifact, the request must say why, who can access
it, how long it will be retained, and how the customer can delete it.

## SLA Boundaries

Response targets can apply to support process:

| Severity | Example | Support Target |
| --- | --- | --- |
| Sev 0 | Credible data loss, path traversal, source exposure, or remote-code execution vector. | Immediate triage target in paid plan; private security path for everyone. |
| Sev 1 | Store will not open, restore blocked, promoted work unrecoverable, release installer broken. | Same-business-day target in paid plan. |
| Sev 2 | CI failure, performance regression, confusing doctor finding, docs gap blocking pilot. | Next-business-day target in paid plan. |
| Sev 3 | General usage question, migration advice, workflow tuning. | Best-effort or plan-defined target. |

SLA targets do not override the open-source support path. Security reports are
accepted privately regardless of paid status.

Local engine behavior remains governed by tests, releases, and documented
trust boundaries. A paid SLA should not require the local binary to phone home.

## Hosted-Service SLA Split

If a hosted service exists later, separate its SLA from local-engine support:

- Hosted uptime applies to hosted dashboards, account pages, sync coordinator,
  support portal, or managed metadata services.
- Local command availability applies to the installed `asp` binary and local
  filesystem. It does not depend on hosted uptime.
- Hosted incidents may delay sync, dashboards, or support-ticket workflows, but
  should not block local restore, doctor, promote, or MCP operations.
- Export and disable paths must remain available during account closure and
  plan downgrade windows.

## No Mandatory Telemetry

Enterprise support may offer optional telemetry or fleet reporting, but it must:

- be disabled by default;
- document every collected field;
- exclude source content by default;
- provide local configuration to disable collection;
- continue to allow support with manually attached diagnostics;
- avoid changing local command success or failure based on telemetry status.

Lack of telemetry may limit how quickly support can diagnose a problem, but it
must not make local workflows unsupported or unusable.

## Remote Access Rules

Remote support sessions are high-trust exceptions:

- customer initiates the session;
- customer controls screen sharing and shell access;
- session scope and time limit are agreed in advance;
- no credentials are copied into chat or tickets;
- commands that mutate `.asp/`, user files, or `.git/` are explained before
  running;
- the customer receives a short command summary afterward.

Prefer artifacts and reproduction steps over remote access whenever possible.

## Support Commitments For Maintainers

Maintainers should:

- keep redacted diagnostics useful enough for first response;
- preserve actionable hints in user-facing errors;
- keep recovery runbooks public;
- document known unsupported platforms and filesystem caveats;
- avoid support-only commands that are unavailable to open-source users;
- publish fixes through normal releases rather than private binary drops.

Enterprise support can sell response time, expertise, rollout help, and hosted
convenience. It cannot sell back the right to run `asp` locally without an
account.

## Related Docs

- [Diagnostics bundles](diagnostics.md)
- [Trust model whitepaper](trust-model.md)
- [Future control plane constraints](control-plane-constraints.md)
- [Open-core boundary policy](open-core-boundary.md)
- [Backup and disaster recovery](backup-recovery.md)
- [Maintainer triage](triage.md)
