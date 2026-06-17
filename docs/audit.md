# Audit

`asp audit` reads the local `.asp/journal.jsonl` audit log and filters it
without contacting any hosted service.

```bash
asp audit --op checkpoint --tool claude --session session-1
asp audit --since 2026-06-17T00:00:00Z --until 2026-06-18T00:00:00Z
asp audit --op restore --path src/app.py --json
```

Filters are additive:

- `--session <id>` matches `session_id` recorded by hooks or agent harnesses.
- `--tool <name>` matches the recorded tool.
- `--op <operation>` can be repeated for `init`, `checkpoint`, `fork`,
  `restore`, `undo`, `promote`, or `discard`.
- `--since` and `--until` accept RFC3339 timestamps.
- `--path <path>` matches path-aware journal entries such as targeted restore
  operations. Checkpoint-to-path attribution is tracked separately in
  `BACKLOG.md`.

With `--json`, the result is the same journal-entry array used by
`asp log --json`, so existing audit scripts can branch on the stable `op`,
`session_id`, `tool`, `detail`, and `ts` fields.
