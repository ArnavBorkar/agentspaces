# Retention

`asp retention plan` is a dry-run report for checkpoint retention. It reads
`.asp/policy.toml`, shows which checkpoint refs would be retained or considered
eligible for deletion, and does not mutate `.asp/`.

```bash
asp retention plan
asp retention plan --json
```

Configure retention locally:

```toml
[retention]
keep_last = 50
max_age_days = 30
```

- `keep_last` keeps at least the newest N checkpoints.
- `max_age_days` marks checkpoints older than N days as delete-eligible.
- The latest checkpoint is always retained in the plan.
- Active and pending fork points are retained in the plan.

The JSON result is `#/$defs/retentionPlan` from [docs/schemas.md](schemas.md).
This command is intentionally non-destructive; retention deletion safety is
tracked separately in `BACKLOG.md`.
