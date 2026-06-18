# asp-core

`asp-core` is the engine library for agentspaces. It owns the local store,
shadow-git checkpointing, journal recovery, full-directory fork mechanics,
large-file sidecar storage, policy enforcement, diff/review data, and sync
primitives.

This crate intentionally contains no CLI presentation or MCP formatting. Those
surfaces live in the `asp` crate.

The full project README, trust model, install script, and release verification
docs live at <https://github.com/ArnavBorkar/agentspaces>.
