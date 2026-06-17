# Audit

`asp audit` reads the local `.asp/journal.jsonl` audit log and filters it
without contacting any hosted service.

```bash
asp audit --op checkpoint --tool claude --session session-1
asp audit --since 2026-06-17T00:00:00Z --until 2026-06-18T00:00:00Z
asp audit --op restore --path src/app.py --json
asp audit --op checkpoint --format jsonl > audit.jsonl
asp audit --op restore --format csv > restore-audit.csv
```

Filters are additive:

- `--session <id>` matches `session_id` recorded by hooks or agent harnesses.
- `--tool <name>` matches the recorded tool.
- `--op <operation>` can be repeated for `init`, `checkpoint`, `fork`,
  `restore`, `undo`, `promote`, or `discard`.
- `--since` and `--until` accept RFC3339 timestamps.
- `--path <path>` matches path-aware journal entries, including checkpoints and
  targeted restore operations.

With global `--json`, the result is the same journal-entry array used by
`asp log --json`, so existing audit scripts can branch on the stable `op`,
`session_id`, `tool`, `detail`, and `ts` fields.

Use `--format jsonl` for evidence pipelines that expect one journal-entry JSON
object per line. Use `--format csv` for spreadsheet and SIEM imports. CSV uses
these fixed columns:

```text
v,ts,op,seq,commit,source,session_id,tool,message,files_changed,duration_ms,detail
```

The `detail` column is compact JSON when an operation has additional structured
metadata. Checkpoint entries include `detail.paths`, an array of
workspace-relative paths changed by that checkpoint.
