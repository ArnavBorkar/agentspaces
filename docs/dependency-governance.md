# Dependency governance

Agentspaces is a storage tool, so dependency drift is a trust issue. This
runbook explains how maintainers should operate the `cargo-deny` policy in
[deny.toml](../deny.toml).

## What runs automatically

- Pull requests and pushes run the `dependency policy` CI job.
- A scheduled `Dependency audit` workflow runs every Monday.
- Maintainers can run the scheduled workflow manually from GitHub Actions.

Both workflows run:

```bash
cargo deny check
```

## Local workflow

Install the checker once:

```bash
cargo install --locked cargo-deny
```

Before changing dependencies:

```bash
cargo deny check
cargo tree -d
cargo test --workspace
```

## Policy

- **Advisories:** fail the build unless there is a documented ignore with a
  removal condition.
- **Licenses:** allow only permissive licenses already listed in `deny.toml`.
  Additions require a reason in the PR description.
- **Sources:** deny unknown registries and unknown git dependencies. New source
  locations need maintainer approval.
- **Bans:** deny wildcard dependencies, deny unused workspace dependencies, and
  warn on duplicate versions unless the duplicate is intentionally skipped with
  a narrow version and reason.

## Triage playbook

1. **RustSec advisory failure:** upgrade the affected crate first. If blocked,
   add a temporary advisory ignore with the advisory id, reason, tracking issue,
   and removal condition.
2. **License failure:** prefer an alternative crate with an allowed license. If
   the crate is necessary, document the legal rationale before changing
   `deny.toml`.
3. **Unknown source failure:** replace the dependency with a crates.io release.
   Git dependencies should be temporary and must point to a reviewed upstream.
4. **Wildcard failure:** add an explicit version requirement. Path dependencies
   between workspace crates should also carry the current package version.
5. **Unused workspace dependency:** remove it from `[workspace.dependencies]` or
   move it into the crate that actually uses it.
6. **Duplicate warning:** run `cargo tree -d`, upgrade the crate that pulls the
   older version, or add a narrow `[[bans.skip]]` with a reason if it is only
   test/dev tooling and cannot be resolved yet.

## Updating the policy

Policy changes should include:

- the `deny.toml` change;
- the `cargo deny check` output after the change;
- any related `Cargo.toml` or `Cargo.lock` cleanup;
- an update to this runbook if the operating procedure changes.
