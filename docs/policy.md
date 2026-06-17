# Workspace Policy

`asp init` writes `.asp/policy.toml` as a local-first team policy file. Every
field is optional, and a missing or empty policy file means no local policy.

The parser is strict: unknown tables or keys are rejected with a
`store_corrupt` error and a hint to fix the TOML or delete the file to disable
local policy. This release validates the file and records the schema; later
policy tasks will enforce the controls below.

## Schema

```toml
[forks]
max_active = 8

[checkpoints]
max_age_hours = 24

[paths]
protected = ["src/security/**", ".github/workflows/**"]

[promote]
require_clean_status = true
require_checkpoint = true
allowed_branch_prefixes = ["asp/"]
```

| TOML path | Type | Default | Meaning |
| --- | --- | --- | --- |
| `forks.max_active` | positive integer or omitted | unset | Maximum active sibling forks future enforcement should allow. |
| `checkpoints.max_age_hours` | positive integer or omitted | unset | Maximum acceptable age for the latest checkpoint before risky work. |
| `paths.protected` | array of workspace-relative strings | `[]` | Path patterns future enforcement should protect from restore, discard, or promote flows without approval. |
| `promote.require_clean_status` | boolean | `false` | Whether future promotion policy should require the main workspace to be clean. |
| `promote.require_checkpoint` | boolean | `false` | Whether future promotion policy should require a current checkpoint before landing fork work. |
| `promote.allowed_branch_prefixes` | array of strings | `[]` | Branch prefixes future promotion policy should allow; empty means unrestricted. |

## Validation Rules

- Unknown tables or keys are rejected.
- `forks.max_active` and `checkpoints.max_age_hours` must be greater than zero
  when set.
- `paths.protected` entries must be non-empty workspace-relative patterns and
  cannot contain `..` path segments.
- `promote.allowed_branch_prefixes` entries must be non-empty and cannot contain
  whitespace.

## Enforcement Status

`asp` validates `.asp/policy.toml` on workspace open so broken policy cannot sit
silently beside agent work. The current release does not block operations based
on the policy values yet. Enforcement for fork count, checkpoint age, protected
paths, and promote requirements is tracked in `BACKLOG.md` under EPIC 14.

## Examples

Keep a small agent fan-out during an enterprise pilot:

```toml
[forks]
max_active = 4
```

Protect sensitive paths from accidental broad operations:

```toml
[paths]
protected = ["src/security/**", "infra/prod/**", ".github/workflows/**"]
```

Reserve promotion branches for reviewable asp output:

```toml
[promote]
require_clean_status = true
require_checkpoint = true
allowed_branch_prefixes = ["asp/", "review/"]
```

## Recovery

If `.asp/policy.toml` is invalid, commands that open the workspace fail before
mutating the store. Fix the TOML syntax or invalid value, or delete
`.asp/policy.toml` to disable local policy.
