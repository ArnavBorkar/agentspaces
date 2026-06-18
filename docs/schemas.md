# JSON Schemas

`asp` publishes JSON Schemas for the automation-facing surfaces that scripts and
agent harnesses depend on:

- CLI envelopes from `asp --json ...`: [schemas/cli-json-envelope.schema.json](../schemas/cli-json-envelope.schema.json)
- Shared result payloads: [schemas/asp-result.schema.json](../schemas/asp-result.schema.json)
- MCP `tools/call` result objects: [schemas/mcp-tool-result.schema.json](../schemas/mcp-tool-result.schema.json)

The schemas use JSON Schema Draft 2020-12. They describe the current v1 CLI and
MCP payload contract; the on-disk format is separately versioned by
`.asp/format-version`.

The TOML schemas for `.asp/config.toml` and `.asp/policy.toml` are documented
in [docs/config.md](config.md) and [docs/policy.md](policy.md).

## CLI Envelope

Every user-facing command accepts `--json`. Successful commands emit:

```json
{
  "ok": true,
  "result": {}
}
```

Errors emit:

```json
{
  "ok": false,
  "error": {
    "code": "not_a_workspace",
    "message": "this directory is not an asp workspace",
    "hint": "run `asp init` in your project root to create one"
  }
}
```

`error.code` is a stable machine-readable enum. `error.hint` is either a
corrective next step or `null` for unexpected infrastructure failures.

## Result Map

| CLI command | Result schema |
| --- | --- |
| `asp init --json` | `#/$defs/initResult` |
| `asp status --json` | `#/$defs/statusReport` |
| `asp stats --json` | `#/$defs/statsReport` |
| `asp bench self --json` | `#/$defs/benchSelfReport` |
| `asp schema --json` | `#/$defs/schemaReport` |
| `asp audit --json` | `#/$defs/journalEntries` |
| `asp policy validate --json` | `#/$defs/policyValidateReport` |
| `asp preflight --json` | `#/$defs/preflightReport` |
| `asp secrets scan --json` | `#/$defs/secretScanReport` |
| `asp evidence collect --json` | `#/$defs/evidenceReport` |
| `asp evidence collect --json --output file.json` | `#/$defs/evidenceOutputResult` |
| `asp retention plan --json` | `#/$defs/retentionPlan` |
| `asp sync push --json --remote <dir>` | `#/$defs/syncPushReport` |
| `asp sync fetch --json --remote <dir>` | `#/$defs/syncFetchReport` |
| `asp checkpoint --json` | `#/$defs/checkpointInfo` or `#/$defs/noChanges` |
| `asp log --json` | `#/$defs/journalEntries` |
| `asp undo --json` | `#/$defs/restoreReport` |
| `asp restore --json` | `#/$defs/restoreReport` |
| `asp fork --json` | `#/$defs/forkInfos` |
| `asp forks --json` | `#/$defs/forkCompareRows` |
| `asp review --json` | `#/$defs/reviewReport` |
| `asp diff --json` | `#/$defs/diffReport` |
| `asp promote --json` | `#/$defs/promoteReport` |
| `asp discard --json` | `#/$defs/discardResult` |
| `asp race --json` | `#/$defs/raceLaneResults` |
| `asp race compare --json` | `#/$defs/raceLaneResults` |
| `asp setup claude --json` | `#/$defs/setupReport` |
| `asp doctor --json` | `#/$defs/doctorFindings` |
| `asp diagnostics --json` | `#/$defs/diagnosticBundle` |
| `asp diagnostics --json --output file.json` | `#/$defs/diagnosticsOutputResult` |

`asp evidence collect --json` emits the evidence packet directly; when
`--output file.json` is also used, the JSON result is a write confirmation
containing `path`, `redacted`, and the same packet under `packet`.
Checkpoint journal entries may include `detail.paths` with workspace-relative
changed paths; clients should treat unknown `detail` fields as operation-specific
metadata.

`asp preflight --json` returns stable `checks[].id` values (`preflight.config`,
`preflight.policy`, `preflight.doctor`, and `preflight.secrets`) plus runbook
links for failed readiness gates. `asp evidence collect --json` summarizes
preflight results, includes the installed schema inventory, and sanitizes recent
audit events by omitting free-form `message` and `detail` fields.

## Raw Export Formats

Some commands produce raw artifacts for tools that expect a standard format
instead of an `asp --json` envelope:

| CLI command | Raw format contract | Compatibility notes |
| --- | --- | --- |
| `asp audit --format jsonl` | Newline-delimited `#/$defs/journalEntry` objects. | New journal fields are additive; clients should ignore unknown `detail` fields. |
| `asp audit --format csv` | Fixed CSV columns documented in [docs/audit.md](audit.md). | Adding columns is additive; removing or renaming columns is breaking. |
| `asp preflight --sarif` | SARIF 2.1.0 with failed readiness checks as results. | `ruleId` values are stable preflight check IDs such as `preflight.secrets`; secret locations stay redacted. |
| `asp secrets scan --sarif` | SARIF 2.1.0 with redacted secret findings as results. | `ruleId` values use stable `secrets.<kind>` names; locations are workspace-relative file and line references. |

SARIF outputs intentionally reference the SARIF 2.1.0 standard instead of
vendoring a local schema. For v1, `version` stays `"2.1.0"` and clients should
treat new SARIF rules, results, properties, help text, or extra locations as
additive. Changing a SARIF `ruleId`, removing required redaction, changing
location semantics, or switching to a different SARIF version is a breaking
automation-contract change and needs the same changelog and compatibility notes
as a breaking CLI JSON schema change.

`asp bench self --json` can run outside an initialized workspace. It creates a
short-lived probe directory under the selected `-C` path, reports the observed
filesystem capabilities, and removes the probe before exiting.

`asp sync push --json --remote <dir>` returns `syncPushReport` with checkpoint,
git-object, CAS-blob, and ref counts split into uploaded/present/updated
buckets. It is an explicit opt-in command; no other asp command starts sync.
`asp sync fetch --json --remote <dir>` returns `syncFetchReport` with imported
ref counts, downloaded/present object counts, head update status, and explicit
`conflicts` entries. Conflicting refs are reported without overwriting local
state.

`asp diff --json` result objects include `summary` totals plus grouped buckets
by top-level path, language, and change type. The existing `rows` array remains
the per-path detail for exact review tooling. `asp diff --json --patch` and
`asp diff --json --stat` return `diffTextReport` with the same summary plus
the rendered patch or stat text. `asp diff --fork <name>` compares an active
fork against its fork point. `asp diff --json --html --output review.html`
returns `diffHtmlOutputResult` after writing an offline HTML review artifact.

`asp doctor --json` finding objects include `severity`, `message`, `cause`,
`next_action`, and `fixed`. Findings that `asp doctor --fix` can repair also
include `repair_plan` with a stable `operation`, `description`, exact `command`,
and conservative `destructive` flag, so automation can preview repairs before
applying them. Human output stays compact by default; pass `asp doctor --explain`
to print the same cause and next-action text.

`asp promote --json` returns `promoteReport` with the created branch and
retained-fork cleanup metadata: `fork_path`, `fork_retained: true`, and
`cleanup_command` (`asp discard <fork>`). When `asp promote --push --remote
<remote>` succeeds, `promoteReport.push` includes `remote`, `branch`, `refspec`,
and the exact `git push` command used. When `--pr-draft` is also used,
`promoteReport.pr` records whether `gh pr create --draft` created a PR; on
failure it includes a `fallback_command` and explanatory `message` instead of
failing the completed promote/push.

`asp forks --json` rows and `asp race --json` lane results may include `review`
signals with `tests_passed`, touched-file and line-churn counts, a numeric
`risk_score`, and explicit `risk_markers` for review dashboards.

`asp review --json` returns a dashboard-oriented review packet containing the
current workspace status, active fork comparison rows, and a Markdown summary
that can be posted as a CI comment without embedding source code.

`asp race --json` lane results include additive runner metadata: `label` is the
explicit `--label` for that lane or the fork name when no label was provided,
`attempts` is the number of attempts actually started, and `timed_out` /
`canceled` report whether the final lane state was killed by a timeout or
cancel-on-success. When JUnit ingestion is configured, lane results may also
include `tests` with aggregate report/test/failure/error/skipped counts and
reported test runtime. `asp race compare --json` additionally includes `rank`
after re-sorting saved lanes for review.

MCP tools return the same payloads in `structuredContent`:

| MCP tool | `structuredContent` schema |
| --- | --- |
| `workspace_init` | `#/$defs/initResult` |
| `workspace_status` | `#/$defs/statusReport` |
| `workspace_checkpoint` | `#/$defs/checkpointInfo` or `#/$defs/noChanges` |
| `workspace_log` | `#/$defs/journalEntries` |
| `workspace_undo` | `#/$defs/restoreReport` |
| `workspace_restore` | `#/$defs/restoreReport` |
| `workspace_fork` | `#/$defs/forkInfos` |
| `workspace_forks` | `#/$defs/forkCompareRows` |
| `workspace_diff` | `#/$defs/diffReport` |
| `workspace_promote` | `#/$defs/promoteReport` |
| `workspace_discard` | `#/$defs/discardResult` |

Tool-level MCP errors still return a JSON-RPC success response. Their result
contains `{ "isError": true, "content": [...], "structuredContent": { "error":
... } }`, with the same stable error enum used by CLI `--json` envelopes.
The full protocol/tool code table is documented in
[docs/mcp-error-codes.md](mcp-error-codes.md).

The MCP `initialize` response includes asp-specific capability metadata under
`capabilities.experimental.asp`, including server version, protocol version,
format version, schema paths, and whether tool annotations are present.
`tools/list` entries include MCP tool annotations such as `readOnlyHint` and
`destructiveHint`; snapshot tests guard these model-facing schemas.

## Change Rules

When a PR changes a serialized field, command result, MCP tool result, or error
code, update the schemas in the same PR. Additive fields should be documented as
schema updates. Removing or renaming fields is a breaking automation change and
needs a changelog entry plus a compatibility note.

Changelog classification rules live in
[CHANGELOG.md](../CHANGELOG.md#automation-contract-rules). Every release that
changes these surfaces needs an **Automation contract** note.

The CI snapshot guard for these shapes is:

```bash
cargo test -p asp --test json_snapshots
```

Use `asp schema --json` to ask an installed binary which schema and on-disk
format versions it supports.
