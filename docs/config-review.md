# Config review guide

Use this guide when a platform, security, or enablement team reviews changes to
`.asp/config.toml`.

For cross-file ownership between `.gitignore`, `.asp/config.toml`, policy, and
secret scanning, see [ignore/config/secrets coordination](ignore-config-secrets.md).

## Review commands

```bash
asp config validate
asp --json config show
asp policy validate --json
asp secrets scan
asp doctor --runbook
```

`asp config validate` intentionally reads only `.asp/config.toml`, so it is a
good first CI check. `asp config show` prints the effective defaults and
project-specific overrides reviewers should compare against the proposed diff.

## What To Check

| Area | Review question | Risk if wrong |
| --- | --- | --- |
| `capture.excludes` | Is the full default exclude list intentionally replaced? | Checkpoints may capture derived state or miss generated files a team expects to preserve. |
| `capture.extra_excludes` | Are project-specific generated paths excluded without hiding source, fixtures, or migration files? | Important work can be omitted from recovery checkpoints. |
| `capture.blob_threshold_mb` | Does the threshold match repository media and binary-file behavior? | Too high bloats shadow git; too low moves normal source-adjacent files into sidecar storage. |
| `promote.branch_template` | Does the template include `{fork}` and match team branch rules? | Repeated promotions collide or produce branches policy later rejects. |
| `.gitignore` alignment | Do `.gitignore` and asp excludes agree on generated state and secrets? | Users may assume one tool captures or ignores files differently from the other. |
| `.asp/policy.toml` alignment | Do protected paths, checkpoint age, and branch prefixes match the config? | Config changes can pass review but fail during restore, promote, or checkpoint. |

## Rollout Pattern

1. Put `.asp/config.toml` changes in their own PR when possible.
2. Include `asp --json config show` output in the PR or CI artifact.
3. Run `asp checkpoint -m "before config rollout"` before testing new settings.
4. Exercise one checkpoint, one fork, and one promote dry path in a disposable
   branch or sample repo.
5. Keep the previous config in git history; config changes do not rewrite old
   checkpoints.

## Red Flags

- `capture.excludes = []` without a clear reason.
- Broad path patterns such as `src/`, `apps/`, `packages/`, or `**/*`.
- Blob thresholds above the repository's normal binary artifact size.
- Branch templates without a stable team prefix.
- Config changes bundled with large source edits, making review intent hard to
  separate.
