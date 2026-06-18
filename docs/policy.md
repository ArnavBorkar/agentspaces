# Workspace Policy

`asp init` writes `.asp/policy.toml` as a local-first team policy file. Every
field is optional, and a missing or empty policy file means no local policy.
For copyable organization starting points, see
[organization policy bundles](policy-packs.md).

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
deny_checkpoint = [".env", "**/*.pem"]

[promote]
require_clean_status = true
require_checkpoint = true
allowed_branch_prefixes = ["asp/"]

[retention]
keep_last = 50
max_age_days = 30
```

| TOML path | Type | Default | Meaning |
| --- | --- | --- | --- |
| `forks.max_active` | positive integer or omitted | unset | Maximum active sibling forks allowed before new fork creation is blocked. |
| `checkpoints.max_age_hours` | positive integer or omitted | unset | Maximum acceptable age for the latest checkpoint before `fork`, `restore`, or `promote`. |
| `paths.protected` | array of workspace-relative strings | `[]` | Path patterns protected from restore and promote. |
| `paths.deny_checkpoint` | array of workspace-relative strings | `[]` | Path patterns that checkpoint is not allowed to capture. |
| `promote.require_clean_status` | boolean | `false` | Whether promotion requires the main workspace to have no dirty, deleted, or untracked paths. |
| `promote.require_checkpoint` | boolean | `false` | Whether promotion requires at least one checkpoint. |
| `promote.allowed_branch_prefixes` | array of strings | `[]` | Branch prefixes promotion may create; empty means unrestricted. |
| `retention.keep_last` | positive integer or omitted | unset | Minimum newest checkpoints retained by retention plans. |
| `retention.max_age_days` | positive integer or omitted | unset | Checkpoints older than this many days are eligible in dry-run retention plans. |

## Validation Rules

Run `asp policy validate` to check the policy without attempting a workspace
mutation. Add `--json` for CI or agent harnesses; a valid policy returns
`#/$defs/policyValidateReport` from [docs/schemas.md](schemas.md).

Run `asp policy explain` when reviewers need to understand why each active rule
exists and which commands it affects:

```bash
asp policy explain
asp policy explain --json
```

The JSON form returns `#/$defs/policyExplainReport` with `rules[]` entries that
include `field`, `value`, `reason`, `affects`, and `enforced`. Path and branch
prefix arrays are explained one entry at a time so owners can review each
pattern independently.

- Unknown tables or keys are rejected.
- `forks.max_active` and `checkpoints.max_age_hours` must be greater than zero
  when set.
- `retention.keep_last` and `retention.max_age_days` must be greater than zero
  when set.
- `paths.protected` and `paths.deny_checkpoint` entries must be non-empty
  workspace-relative patterns and cannot contain `..` path segments.
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
- `paths.deny_checkpoint` blocks checkpoints that would capture a matching file.
  Deleting a denied file can still be checkpointed so teams can remove
  accidental inclusions.
- `promote.require_clean_status`, `promote.require_checkpoint`, and
  `promote.allowed_branch_prefixes` are checked before the promoted branch is
  created.

Protected path patterns are workspace-relative. `*` matches within one path
segment; `**` matches across path segments. For example, `src/security/**`
matches `src/security/auth.rs` and deeper files under that directory.

## Config Pairing

Promotion policy is easier to review with the effective config beside it:

```bash
asp --json config show > asp-config.json
asp policy validate --json > asp-policy.json
```

Compare `asp-config.json.result.config.promote.branch_template` with
`asp-policy.json.result.policy.promote.allowed_branch_prefixes`. For example,
`"branch_template": "review/{workspace}/{fork}"` pairs with
`"allowed_branch_prefixes": ["review/"]`; a template such as `asp/{fork}` would
fail that policy before `asp promote` creates a branch.

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

Block common local secret files from checkpoint capture:

```toml
[paths]
deny_checkpoint = [".env", ".env.*", "**/*.pem"]
```

Reserve promotion branches for reviewable asp output:

```toml
[promote]
require_clean_status = true
require_checkpoint = true
allowed_branch_prefixes = ["asp/", "review/"]
```

Plan local checkpoint retention without deleting anything:

```toml
[retention]
keep_last = 50
max_age_days = 30
```

## Recovery

If `.asp/policy.toml` is invalid, commands that open the workspace fail before
mutating the store. Fix the TOML syntax or invalid value, or delete
`.asp/policy.toml` to disable local policy.
