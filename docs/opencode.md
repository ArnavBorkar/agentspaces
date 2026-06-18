# OpenCode integration

`asp setup opencode` registers the local `asp mcp` server in OpenCode's
documented MCP config shape.

```bash
asp setup opencode
```

Project-scoped setup writes `opencode.json` in the workspace root:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "agentspaces": {
      "type": "local",
      "command": ["asp", "mcp"],
      "enabled": true
    }
  }
}
```

OpenCode also supports global config at
`~/.config/opencode/opencode.json`:

```bash
asp setup opencode --user
```

Remove only the `asp`-managed entry with:

```bash
asp setup opencode --remove
```

Setup preserves unrelated keys and existing MCP servers. If `mcp.agentspaces`
already exists and was not created by `asp`, setup stops with a hint rather than
overwriting it.

The setup command edits strict JSON files. If your OpenCode config uses JSONC
comments or trailing commas, either convert it to JSON before running setup or
add the MCP entry manually using the shape above.
