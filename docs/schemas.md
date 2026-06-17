# JSON Schemas

`asp` publishes JSON Schemas for the automation-facing surfaces that scripts and
agent harnesses depend on:

- CLI envelopes from `asp --json ...`: [schemas/cli-json-envelope.schema.json](../schemas/cli-json-envelope.schema.json)
- Shared result payloads: [schemas/asp-result.schema.json](../schemas/asp-result.schema.json)
- MCP `tools/call` result objects: [schemas/mcp-tool-result.schema.json](../schemas/mcp-tool-result.schema.json)

The schemas use JSON Schema Draft 2020-12. They describe the current v1 CLI and
MCP payload contract; the on-disk format is separately versioned by
`.asp/format-version`.

The TOML schema for `.asp/config.toml` is documented in
[docs/config.md](config.md).

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
| `asp schema --json` | `#/$defs/schemaReport` |
| `asp checkpoint --json` | `#/$defs/checkpointInfo` or `#/$defs/noChanges` |
| `asp log --json` | `#/$defs/journalEntries` |
| `asp undo --json` | `#/$defs/restoreReport` |
| `asp restore --json` | `#/$defs/restoreReport` |
| `asp fork --json` | `#/$defs/forkInfos` |
| `asp forks --json` | `#/$defs/forkCompareRows` |
| `asp diff --json` | `#/$defs/diffReport` |
| `asp promote --json` | `#/$defs/promoteReport` |
| `asp discard --json` | `#/$defs/discardResult` |
| `asp race --json` | `#/$defs/raceLaneResults` |
| `asp race compare --json` | `#/$defs/raceLaneResults` |
| `asp setup claude --json` | `#/$defs/setupReport` |
| `asp doctor --json` | `#/$defs/doctorFindings` |
| `asp diagnostics --json` | `#/$defs/diagnosticBundle` |
| `asp diagnostics --json --output file.json` | `#/$defs/diagnosticsOutputResult` |

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

Tool-level MCP errors still return a JSON-RPC success response containing
`{ "isError": true, "content": [...] }`, which is covered by
`mcp-tool-result.schema.json`.

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
