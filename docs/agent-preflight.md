# Agent preflight

Run preflight before starting long-running agent work, especially races,
multi-hour refactors, or unattended harness sessions.

## Before Launch

```bash
asp preflight
asp checkpoint -m "before agent run"
asp setup codex
asp setup opencode
asp setup claude
```

Run the setup command for the harness you actually use. Re-running setup is
idempotent and should not replace unmanaged config.

## Wrapper Pattern

Use this shape in local scripts that launch agents:

```bash
asp preflight
asp checkpoint -m "before $AGENT_NAME"
"$@"
asp checkpoint -m "after $AGENT_NAME"
```

Keep wrappers simple and visible. Do not hide `asp undo`, `asp restore`, or
`asp promote` inside unattended scripts.

## Race Pattern

```bash
asp preflight
asp race -n 3 --timeout 20m --retries 1 -- <agent command>
asp race compare --name race
asp forks
```

If preflight fails, stop before launching the race. Use `asp doctor --runbook`
or `asp secrets scan` to triage the failure locally.

## Harness Checklist

- `asp preflight` passes.
- A fresh checkpoint exists immediately before launch.
- MCP setup is visible in the harness config.
- Secrets scan is clean for checkpoint-scoped files.
- The operator knows the recovery command: `asp undo`.
- Long-running races have a timeout and a retry budget.
