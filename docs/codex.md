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

This integration currently registers MCP only. Hook guidance for automatic
Codex checkpoints is tracked separately because Codex hooks require explicit
trust review inside Codex.
