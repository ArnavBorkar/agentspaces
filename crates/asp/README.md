# asp

`asp` is the agentspaces command-line binary and MCP stdio server. It gives AI
agents durable, branchable workspaces over real project directories:

- checkpoint the source tree into a recoverable shadow git store;
- fork full working directories for parallel agent attempts;
- compare, undo, restore, and promote winning forks as ordinary git branches;
- expose the same workflow to agent harnesses through JSON output and MCP.

The full project README, trust model, install script, and release verification
docs live at <https://github.com/ArnavBorkar/agentspaces>.

Install from a published crate with:

```bash
cargo install asp
```

Until the crates.io publish is complete, use the repository install paths in the
project README.
