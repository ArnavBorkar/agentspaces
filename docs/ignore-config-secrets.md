# Ignore, config, and secrets coordination

Teams should review `.gitignore`, `.asp/config.toml`, `.asp/policy.toml`, and
secret-scanning rules together. Each file answers a different question:

| File or command | Scope | Use it for |
| --- | --- | --- |
| `.gitignore` | User git and shadow git checkpoints | Source-control ignores and generated state every git command should ignore. |
| `.asp/config.toml` | asp checkpoint capture | Additional asp-only checkpoint excludes and large-file sidecar thresholds. |
| `.asp/policy.toml` | asp mutations | Hard safety rules for checkpoint age, fork count, protected paths, and promote branches. |
| `asp secrets scan` | Pre-checkpoint review | Detect likely secrets before they enter checkpoint history. |

## Coordination Rules

1. Put broadly generated state in `.gitignore` first.
2. Use `capture.extra_excludes` for asp-specific generated paths that should
   remain visible to normal git workflows.
3. Avoid broad source-tree excludes in `.asp/config.toml`; they reduce recovery
   coverage.
4. Put must-not-touch paths in `.asp/policy.toml`, not in ignore files.
5. Run `asp secrets scan` before a baseline checkpoint and before promoting
   agent-generated work.

## Review Flow

```bash
git check-ignore -v path/to/generated-file
asp config validate
asp --json config show
asp policy validate --json
asp secrets scan
asp checkpoint -m "ignore/config/secrets review"
```

If a file is ignored by git but should be recoverable through asp, remove it
from `.gitignore` and use a narrower `capture.extra_excludes` only when it is
safe for asp to omit it too. If a file must never be restored or promoted by an
agent, add it to `.asp/policy.toml` protected paths.

## Common Patterns

| Pattern | Recommended owner |
| --- | --- |
| Package manager caches | `.gitignore` |
| Build output directories | `.gitignore`, plus `capture.extra_excludes` only for asp-specific output |
| Large source-adjacent binaries | `capture.blob_threshold_mb` |
| Credentials, env files, private keys | `.gitignore`, `asp secrets scan`, and `.asp/policy.toml` protected paths |
| Compliance-controlled files | `.asp/policy.toml` protected paths |
| Reviewed generated clients | Keep checkpointed; review with `asp diff --stat` |

## CI Gate

For repositories that standardize asp, add a CI job that runs:

```bash
asp config validate
asp preflight --json
asp policy validate --json
asp secrets scan
```

Keep the job non-mutating. Do not run `asp doctor --fix`, `asp restore`,
`asp undo`, or `asp promote` from an automatic CI gate.
