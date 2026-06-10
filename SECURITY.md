# Security Policy

## Reporting

Please report suspected vulnerabilities privately via [GitHub Security Advisories](https://github.com/ArnavBorkar/agentspaces/security/advisories/new). You'll get an acknowledgment within 72 hours.

## Scope & model

`asp` is a fully local tool: no network calls, no telemetry, no accounts. Its security-relevant surface is:

- **File handling**: asp reads/writes within the workspace root and creates sibling fork directories (store-supplied paths are validated against traversal). Checkpoints capture untracked-but-not-gitignored files into the local `.asp/` store by design; gitignored secrets (`.env`) stay out of checkpoints but ARE physically present in forks (they're clones). `.asp/` inherits your filesystem permissions. Add patterns to `capture.extra_excludes` to keep more out of checkpoints.
- **Subprocess execution**: asp shells out to `git` (pinned environment) and, for `asp race`, runs *the command you pass* in each fork. asp never executes workspace content on its own.
- **Hooks**: `asp setup claude` installs hooks that run `asp hook-event`; the handler parses untrusted JSON from stdin and always exits 0.

Issues we'd consider vulnerabilities: path traversal escaping the workspace root, the shadow repo or journal being usable to execute code, checkpoint capture following symlinks outside the root, lock-file symlink attacks.
