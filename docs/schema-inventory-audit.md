# Schema Inventory Audit

This audit compares shipped `asp --json` command surfaces against
[docs/schemas.md](schemas.md) and
[schemas/asp-result.schema.json](../schemas/asp-result.schema.json).

## Covered Surfaces

The Result Map now covers the core workspace, policy, config, readiness,
security, evidence, drills, sync, review, diff, race, diagnostics, and setup
payloads:

- `asp config show --json`, `asp config validate --json`, and
  `asp config diff --against <file> --json`
- `asp policy validate --json` (`policyValidateReport`) and
  `asp policy explain --json` (`policyExplainReport`)
- `asp quickstart --json`
- `asp completions <shell> --json`
- `asp manpage --json`
- `asp preflight --json`
- `asp evidence collect --json`
- `asp evidence collect --json --output file.json`
- `asp evidence manifest --packet file.json --output manifest.json --json`
  (`evidenceManifestOutputResult`)
- `asp evidence verify --packet file.json --manifest manifest.json --json`
  (`evidenceVerifyReport`)
- `asp setup claude --json` (`setupReport`), `asp setup codex --json`
  (`codexSetupReport`), and `asp setup opencode --json`
  (`opencodeSetupReport`)
- `asp diff --json` (`diffReport`), `asp diff --json --patch`, and
  `asp diff --json --stat` (`diffTextReport`), plus
  `asp diff --json --html --output review.html` (`diffHtmlOutputResult`)
- `asp doctor --json` (`doctorFindings`) and
  `asp doctor --json --runbook` (`doctorRunbookReport`)
- raw SARIF exports for `asp preflight --sarif` and `asp secrets scan --sarif`
- `asp drill recovery --json` (`drillRecoveryReport`)
- `asp drill fork --json` (`drillForkReport`)

## Follow-Up Inventory

These shipped machine-readable surfaces still need explicit Result Map rows,
schema definitions, snapshots, or all three:

| Surface | Current state | Needed follow-up |
| --- | --- | --- |
| _None currently._ | All known shipped `--json` result shapes are mapped, schema-backed, or documented as raw standard formats. | Keep this audit updated with every new machine-readable surface. |

## Audit Rule

When a new command or flag can change the shape under the CLI envelope's
`result`, the PR should update one of these states in the same commit:

- the Result Map points at an existing shared schema;
- a new `$defs` entry is added under `schemas/asp-result.schema.json`;
- raw standard-format output is documented under Raw Export Formats;
- the surface is listed in this audit with a named follow-up task.
