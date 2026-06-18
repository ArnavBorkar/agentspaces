# Schema Inventory Audit

This audit compares shipped `asp --json` command surfaces against
[docs/schemas.md](schemas.md) and
[schemas/asp-result.schema.json](../schemas/asp-result.schema.json).

## Covered Surfaces

The Result Map now covers the core workspace, policy, config, readiness,
security, evidence, sync, review, race, diagnostics, and setup-Claude payloads:

- `asp config show --json` and `asp config validate --json`
- `asp quickstart --json`
- `asp completions <shell> --json`
- `asp manpage --json`
- `asp preflight --json`
- `asp evidence collect --json`
- `asp evidence collect --json --output file.json`
- raw SARIF exports for `asp preflight --sarif` and `asp secrets scan --sarif`

## Follow-Up Inventory

These shipped machine-readable surfaces still need explicit Result Map rows,
schema definitions, snapshots, or all three:

| Surface | Current state | Needed follow-up |
| --- | --- | --- |
| `asp setup codex --json` | Returns `setupReport`, but the Result Map only lists `asp setup claude --json`. | Add Result Map rows for Codex and OpenCode setup variants. |
| `asp setup opencode --json` | Returns `setupReport`, but the Result Map only lists `asp setup claude --json`. | Add Result Map rows for Codex and OpenCode setup variants. |
| `asp diff --json --patch` and `asp diff --json --stat` | `diffTextReport` exists in the shared result schema, but the Result Map omits these rows. | Add explicit Result Map rows and snapshot coverage for text diff modes. |
| `asp diff --json --html --output review.html` | `diffHtmlOutputResult` exists in the shared result schema, but the Result Map omits this row. | Add an explicit Result Map row and snapshot coverage for HTML export confirmation. |
| `asp doctor --json --runbook` | Tested in CLI/docs coverage but missing `doctorRunbookReport` in the shared result schema. | Add schema definition, Result Map row, and snapshot. |

## Audit Rule

When a new command or flag can change the shape under the CLI envelope's
`result`, the PR should update one of these states in the same commit:

- the Result Map points at an existing shared schema;
- a new `$defs` entry is added under `schemas/asp-result.schema.json`;
- raw standard-format output is documented under Raw Export Formats;
- the surface is listed in this audit with a named follow-up task.
