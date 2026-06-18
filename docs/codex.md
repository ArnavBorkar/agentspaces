# Codex integration

`asp setup codex` registers the local `asp mcp` server in Codex's documented
configuration file shape. Project-scoped setup writes `.codex/config.toml`;
user-scoped setup writes `~/.codex/config.toml`.

```bash
asp setup codex
```

The managed block is marker-delimited so existing config, comments, and other
MCP servers stay in place:

```toml
# agentspaces: begin asp setup codex
[mcp_servers.agentspaces]
command = "asp"
args = ["mcp"]
startup_timeout_sec = 10
tool_timeout_sec = 60
# agentspaces: end asp setup codex
```

Codex loads project `.codex/` configuration only after the project is trusted.
After setup, restart Codex or open a new session, then use `/mcp` to inspect the
registered server.

## Options

```bash
asp setup codex --user     # write ~/.codex/config.toml
asp setup codex --remove   # remove only the asp-managed block
```

If `[mcp_servers.agentspaces]` already exists outside the managed block, setup
stops with a hint instead of overwriting it. Rename or remove the existing entry
and rerun setup.

`asp setup codex` registers MCP only. Automatic checkpoint hooks remain an
explicit manual opt-in because Codex hooks require trust review inside Codex.

## Optional checkpoint hooks

Codex hooks are enabled by default, but project-local hooks run only after the
project is trusted and the hook definition is reviewed in Codex. Use `/hooks` in
Codex to inspect and trust new hooks.

Add these inline hooks to `.codex/config.toml` if you want Codex shell and file
tool activity to create `asp` checkpoints:

```toml
[[hooks.PreToolUse]]
matcher = "^Bash$"

[[hooks.PreToolUse.hooks]]
type = "command"
command = 'asp checkpoint -m "codex: before Bash" --source hook --tool Bash >/dev/null 2>&1 || true'
timeout = 60
statusMessage = "Checkpointing before shell command"

[[hooks.PostToolUse]]
matcher = "Bash|apply_patch|Edit|Write"

[[hooks.PostToolUse.hooks]]
type = "command"
command = 'asp checkpoint -m "codex: after tool" --source hook --tool Codex >/dev/null 2>&1 || true'
timeout = 60
statusMessage = "Checkpointing workspace changes"
```

The commands are deliberately payload-independent: they call `asp checkpoint`
directly instead of `asp hook-event`, whose parser is currently shaped for
Claude Code hook payloads. They also swallow errors so a missing workspace or
transient checkpoint issue does not break the Codex session.
