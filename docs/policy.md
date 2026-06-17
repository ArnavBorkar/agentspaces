# Workspace Policy

`asp init` writes `.asp/policy.toml` as a local-first team policy file. Every
field is optional, and a missing or empty policy file means no local policy.

The parser is strict: unknown tables or keys are rejected with a
`store_corrupt` error and a hint to fix the TOML or delete the file to disable
local policy. Valid policy values are enforced locally before risky workspace
mutations.

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
| `forks.max_active` | positive integer or omitted | unset | Maximum active sibling forks allowed before new fork creation is blocked. |
| `checkpoints.max_age_hours` | positive integer or omitted | unset | Maximum acceptable age for the latest checkpoint before `fork`, `restore`, or `promote`. |
| `paths.protected` | array of workspace-relative strings | `[]` | Path patterns protected from restore and promote. |
| `promote.require_clean_status` | boolean | `false` | Whether promotion requires the main workspace to have no dirty, deleted, or untracked paths. |
| `promote.require_checkpoint` | boolean | `false` | Whether promotion requires at least one checkpoint. |
| `promote.allowed_branch_prefixes` | array of strings | `[]` | Branch prefixes promotion may create; empty means unrestricted. |

## Validation Rules

- Unknown tables or keys are rejected.
- `forks.max_active` and `checkpoints.max_age_hours` must be greater than zero
  when set.
- `paths.protected` entries must be non-empty workspace-relative patterns and
  cannot contain `..` path segments.
- `promote.allowed_branch_prefixes` entries must be non-empty and cannot contain
  whitespace.

## Enforcement Status

Policy violations fail with the stable `policy_violation` error code and a
hint describing the next action. Enforcement happens before the risky part of
the operation:

- `forks.max_active` is checked before fork-point checkpoint capture or clone
  creation.
- `checkpoints.max_age_hours` is checked before `fork`, `restore`, and
  `promote`. Run `asp checkpoint` to refresh the latest checkpoint.
- `paths.protected` blocks full or targeted restores that would write/delete a
  matching path, and blocks promotes whose fork changes a matching path.
- `promote.require_clean_status`, `promote.require_checkpoint`, and
  `promote.allowed_branch_prefixes` are checked before the promoted branch is
  created.

Protected path patterns are workspace-relative. `*` matches within one path
segment; `**` matches across path segments. For example, `src/security/**`
matches `src/security/auth.rs` and deeper files under that directory.

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
