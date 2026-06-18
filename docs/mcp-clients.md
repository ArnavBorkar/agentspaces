# Generic MCP clients

`asp mcp` is a stdio MCP server. Any MCP client that can launch a local command
can use it.

The server should start in an `asp` workspace, or the client should pass a
workspace directory to tools that support a directory argument.

## Generic JSON shape

Many clients use a Claude-style JSON object:

```json
{
  "mcpServers": {
    "agentspaces": {
      "command": "asp",
      "args": ["mcp"]
    }
  }
}
```

If the client supports `cwd`, set it to the workspace root:

```json
{
  "mcpServers": {
    "agentspaces": {
      "command": "asp",
      "args": ["mcp"],
      "cwd": "/path/to/project"
    }
  }
}
```

## Codex TOML shape

Codex stores MCP servers in `config.toml`:

```toml
[mcp_servers.agentspaces]
command = "asp"
args = ["mcp"]
startup_timeout_sec = 10
tool_timeout_sec = 60
```

Use `asp setup codex` for managed project or user configuration.

## OpenCode JSON shape

OpenCode stores MCP servers under `mcp`:

```json
{
  "mcp": {
    "agentspaces": {
      "type": "local",
      "command": ["asp", "mcp"],
      "enabled": true
    }
  }
}
```

Use `asp setup opencode` for managed project or user configuration.

## Tool safety

The MCP server exposes both read-only inspection tools and state-changing tools
such as checkpoint, undo, restore, promote, and discard. Configure your client
approval policy so destructive tools require a human prompt unless your
workflow has another review gate.

Every tool error returns a structured code, message, and hint. See
[docs/mcp-error-codes.md](mcp-error-codes.md) for recovery guidance.

## Quick smoke check

After registering the server, use your client's MCP inspector or tool list UI
and confirm that tools such as `workspace_status`, `workspace_checkpoint`, and
`workspace_diff` are visible.

If the client cannot find `asp`, install it first or use an absolute command
path in the MCP config.
