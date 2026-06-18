# Organization Policy Bundles

Use these `.asp/policy.toml` bundles as reviewed starting points for common
rollout profiles. They are examples, not universal defaults: copy one into a
pilot repository, review every path with the repo owner, then commit it like any
other safety policy.

Verify a bundle before asking agents to work in the repository:

```bash
asp policy validate
asp policy explain
asp --json policy explain > asp-policy-explain.json
asp preflight
```

Keep `asp-policy-explain.json` with the rollout ticket so reviewers can see why
each active rule exists, which commands it affects, and when it is enforced.
The bundles cover `forks.max_active`, `checkpoints.max_age_hours`,
`paths.protected`, `paths.deny_checkpoint`, `promote.require_clean_status`,
`promote.require_checkpoint`, `promote.allowed_branch_prefixes`,
`retention.keep_last`, and `retention.max_age_days`.

## Regulated Workflow

Use this for repositories with production infrastructure, compliance-controlled
code paths, or strict change windows.

```toml
[forks]
max_active = 4

[checkpoints]
max_age_hours = 12

[paths]
protected = [
  "src/security/**",
  "infra/prod/**",
  ".github/workflows/**",
  "db/migrations/**",
]
deny_checkpoint = [
  ".env",
  ".env.*",
  "**/*.pem",
  "**/*.p12",
  "secrets/**",
]

[promote]
require_clean_status = true
require_checkpoint = true
allowed_branch_prefixes = ["asp/reg/", "review/"]

[retention]
keep_last = 100
max_age_days = 90
```

Review notes:

- Confirm protected paths match CODEOWNERS or the team's approval map.
- Keep `max_active` low until the team has reviewed fork cleanup behavior.
- Pair with `asp config diff --against <file>` when the organization has a
  standard config baseline.

## Startup Workflow

Use this for small teams that want faster fan-out while still blocking obvious
secrets and production infrastructure mistakes.

```toml
[forks]
max_active = 12

[checkpoints]
max_age_hours = 48

[paths]
protected = [
  ".github/workflows/**",
  "infra/prod/**",
]
deny_checkpoint = [
  ".env",
  ".env.*",
  "**/*.pem",
  "secrets/**",
]

[promote]
require_clean_status = false
require_checkpoint = true
allowed_branch_prefixes = ["asp/", "ship/"]

[retention]
keep_last = 30
max_age_days = 30
```

Review notes:

- Raise `max_active` only when CI capacity and reviewer bandwidth can absorb the
  extra lanes.
- Keep production infrastructure protected even when most product paths stay
  flexible.
- Use `asp policy explain` in onboarding docs so new contributors understand
  why promotion still requires a checkpoint.

## OSS Maintainer Workflow

Use this for open source maintainers who run community-proposed agent work and
want conservative landing rules around repository governance files.

```toml
[forks]
max_active = 6

[checkpoints]
max_age_hours = 72

[paths]
protected = [
  ".github/workflows/**",
  "SECURITY.md",
  "CODEOWNERS",
]
deny_checkpoint = [
  ".env",
  ".env.*",
  "**/*.pem",
  "*.key",
  "secrets/**",
]

[promote]
require_clean_status = true
require_checkpoint = true
allowed_branch_prefixes = ["asp/", "contrib/"]

[retention]
keep_last = 40
max_age_days = 60
```

Review notes:

- Protect governance files that should move only through explicit maintainer
  review.
- Keep contributor branches in a predictable namespace for PR triage.
- Run `asp secrets scan` before promotion when accepting external prompts,
  patches, or generated fixtures.

## Rollout Checklist

For every bundle:

1. Review protected and denied path patterns with the repository owner.
2. Run `asp policy validate` and `asp policy explain`.
3. Save `asp --json policy explain > asp-policy-explain.json` as a rollout
   artifact.
4. Run one checkpoint, one fork, and one discard in a disposable branch.
5. Revisit `forks.max_active`, `checkpoints.max_age_hours`, and retention after
   the first week of real usage.

For multi-repository adoption, use the [fleet rollout checklist](fleet-rollout.md)
and [rollout handoff guide](rollout-handoff.md).
