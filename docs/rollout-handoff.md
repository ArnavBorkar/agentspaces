# Phased Rollout And Owner Handoff

Use this guide after a pilot repository has passed the
[fleet rollout checklist](fleet-rollout.md). It turns a working pilot into an
owned operating model: phase gates, rollback choices, and a handoff packet the
repository owner can keep current.

## Phase Gates

| Phase | Scope | Entry gate | Exit gate |
| --- | --- | --- | --- |
| 0: Pilot | 2-3 maintainer-owned repositories | Owner agrees to test `asp` and capture evidence. | `asp preflight`, one checkpoint, one fork, one discard, and `asp doctor --deep` pass locally. |
| 1: Early teams | 5-10 willing teams | Pilot evidence is reviewed and policy/config baselines are chosen. | Read-only CI runs `asp config validate`, `asp config diff`, and `asp preflight` without unresolved findings. |
| 2: Critical repos | Production or compliance-sensitive repositories | Backup and recovery owner is named. | Recovery drill, evidence manifest verification, and owner handoff are complete. |
| 3: Default recommendation | Broad internal adoption | Support channel, triage owner, and rollback notes are published. | New repositories can self-serve with templates, policy packs, CI gates, and escalation paths. |

Do not advance a repository to the next phase while any data-loss, restore,
promote, source-exposure, or secret-handling report is unresolved.

## Rollback Levels

Choose the smallest rollback that solves the problem. Keep evidence unless the
owner explicitly confirms it is no longer needed.

| Level | Use when | Commands |
| --- | --- | --- |
| Harness rollback | Agent hooks or MCP setup caused trouble, but `.asp/` evidence should remain. | `asp setup claude --remove`, `asp setup codex --remove`, `asp setup opencode --remove`, then `asp doctor --deep`. |
| Policy/config rollback | A rollout changed `.asp/config.toml` or `.asp/policy.toml` too aggressively. | Revert the config or policy commit, then run `asp config validate`, `asp policy validate`, and `asp preflight`. |
| Workspace rollback | The team wants to stop using `asp` in this repo but preserve support context. | Run `asp evidence collect --deep --output asp-evidence.json`, `asp evidence manifest`, and `asp evidence verify` before deciding whether `.asp/` can be removed. |

Do not run `asp doctor --fix`, `asp restore`, `asp promote`, or `asp discard`
as part of rollback unless the repository owner has approved that exact command.

## Owner Handoff Packet

Create a short handoff issue, ticket, or repository doc with these fields:

```markdown
# asp owner handoff

Repository:
Owner:
Escalation channel:
Rollout phase:
Selected config template:
Selected policy bundle:
Baseline config path:
CI job names:
Required local smoke test:
Rollback owner:
Evidence packet:
Evidence manifest:
Verification log:
Known exceptions:
Next review date:
```

Attach or link these artifacts:

- `asp --json config show > asp-config.json`
- `asp --json config diff --against baseline.toml > asp-config-diff.json`
- `asp --json policy explain > asp-policy-explain.json`
- `asp --json preflight > asp-preflight.json`
- `asp evidence collect --output asp-evidence.json`
- `asp evidence manifest --packet asp-evidence.json --output asp-evidence.manifest.json`
- `asp evidence verify --packet asp-evidence.json --manifest asp-evidence.manifest.json`

The owner should know which files are committed policy (`.asp/config.toml` and
`.asp/policy.toml`), which files are CI artifacts, and which local `.asp/`
contents are evidence that may need retention.

## Review Cadence

During the first week, review:

- active forks and whether `forks.max_active` is too high or too low;
- config drift from the baseline and whether exceptions are intentional;
- denied checkpoint paths that blocked useful work;
- protected paths that need CODEOWNERS alignment;
- recovery drill results and evidence verification logs;
- open support tickets or unresolved preflight findings.

After the first week, move the repo to normal ownership only when the owner can
run the smoke test, explain the rollback level to use, and identify the support
channel without help from the rollout team.

## Related Docs

- [Fleet rollout checklist](fleet-rollout.md)
- [Config templates](config-templates.md)
- [Organization policy bundles](policy-packs.md)
- [CI preflight examples](ci.md)
- [Evidence packets](evidence.md)
- [Support ticket templates](support-ticket-templates.md)
