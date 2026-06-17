# Maintainer Triage

Use this guide to turn incoming issues into actionable work without weakening
the trust model.

## First Pass

Within the first maintainer pass:

1. Confirm the report belongs in public issues. Move security or sensitive-data
   reports to private security advisories.
2. Add one type label: `bug`, `enhancement`, `documentation`, `performance`,
   `integration`, or `crash-safety`.
3. Add `needs-info` if the issue is missing the reproduction, environment,
   `asp doctor` output, or expected behavior.
4. Add one priority label once the impact is clear: `priority-high`,
   `priority-normal`, or `priority-low`.
5. Add area labels only when they help route work: `area-filesystem`,
   `area-mcp`, `area-hooks`, `area-windows`, or `area-release`.
6. Remove `needs-triage` after the issue has type, priority, and next action.

## Priority Rules

Use `priority-high` for:

- possible checkpoint data loss or byte-incorrect restore;
- `asp doctor --fix` unable to repair state that asp created;
- `.git` writes outside the documented `promote` path;
- release artifact, checksum, provenance, or install failures;
- CI red on `main`.

Use `priority-normal` for valid bugs, integration requests, and documented
roadmap work that does not block a release.

Use `priority-low` for polish, wording, nice-to-have integrations, and issues
with acceptable workarounds.

## Crash-Safety Reports

For `crash-safety` issues, ask for:

- exact command or agent action sequence;
- whether the failure happened in the parent workspace or a fork;
- `asp --version`, `git --version`, OS, and filesystem;
- `asp doctor --fix` output, and `asp doctor --deep` output for CAS concerns;
- whether the reporter can preserve the affected `.asp/` directory.

Do not ask reporters to run destructive cleanup before collecting enough
diagnostics to understand the failure.

## Performance Reports

For `performance` issues, ask for:

- operation (`checkpoint`, `fork`, `restore`, `status`, `doctor`, hooks, MCP,
  install);
- elapsed time, repository size, file count, and large-file count;
- filesystem and whether it is local, networked, virtualized, or synced;
- comparison against a previous asp version or `scripts/bench/run.py` when
  possible.

Performance claims in README files must keep matching benchmark docs.

## Integration Requests

For `integration` issues, classify the requested surface:

- `area-mcp` for MCP tool behavior or client registration;
- `area-hooks` for event hooks around file edits or shell commands;
- `area-release` for package managers or install surfaces;
- `area-windows` when the requested tool only runs natively on Windows.

Integration work must preserve existing user settings and include an uninstall
or rollback path when it writes config.

## Label Maintenance

The tracked label catalog lives in [.github/labels.yml](../.github/labels.yml).
Apply missing labels with GitHub CLI:

```bash
gh label create needs-triage --repo ArnavBorkar/agentspaces --color d4c5f9 --description "New issue waiting for maintainer classification."
gh label edit needs-triage --repo ArnavBorkar/agentspaces --color d4c5f9 --description "New issue waiting for maintainer classification."
```

For bulk updates, iterate over `.github/labels.yml` with a small script, but
review colors and descriptions before applying them to the public repository.
