# Preflight

`asp preflight` runs a read-only readiness gate for CI, onboarding, and agent
workflows.
For copyable CI snippets, see [CI preflight examples](ci.md).
For harness launch guidance, see [agent preflight](agent-preflight.md).

```bash
asp preflight
asp --json preflight
asp preflight --deep
```

The command checks:

- `.asp/config.toml` parses and reports effective settings.
- `.asp/policy.toml` loads and reports active rule count.
- `asp doctor` has no warning or error findings that require attention.
- `asp secrets scan` finds no likely secrets in checkpoint-scoped files.

If any blocking check fails, `asp preflight` exits nonzero. JSON output still
uses the normal success envelope so CI systems and agents can inspect the full
report. Each check includes a runbook link; human output prints the link beside
failing checks.

## CI Example

```bash
asp config validate
asp preflight --json
```

Keep preflight non-mutating. Do not pair it with `asp doctor --fix`, `asp undo`,
`asp restore`, or `asp promote` in an automatic CI job.
