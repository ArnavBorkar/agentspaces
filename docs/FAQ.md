# FAQ

**Does asp touch my `.git`?**
Only `asp promote`, and only to create a new branch via a local fetch. Never HEAD, never your worktree, never history rewrites, never hooks. The checkpoint store is a separate shadow repo under `.asp/`.

**What happens if asp crashes mid-operation?**
The store self-heals: journal writes are CRC-checked and torn tails truncate on next open; all other mutations are atomic renames or git's own tmp+rename. A CI torture suite SIGKILLs asp mid-checkpoint/fork/restore and verifies nothing checkpointed is ever lost. `asp doctor --fix` cleans up anything cosmetic (e.g. a half-cloned fork directory).

**How much disk do checkpoints use?**
Source files are stored once per unique content (git objects, compression off for speed). Files over 50 MB are stored once per unique content in a CoW sidecar — on APFS/btrfs/XFS that costs almost nothing until the original changes. Derived state (`node_modules/`, `target/`, `build/`…) is excluded from checkpoints by default (configurable in `.asp/config.toml`) — forks still carry it physically.

**Are forks free?**
Nearly. A fork shares all file bytes with its parent via copy-on-write; you pay for inode metadata (~32 MB for 100k files) and only for bytes that subsequently diverge. Forks must live on the same volume as the workspace.

**Can I use asp without the Claude Code integration?**
Yes — `asp` is a plain CLI; `checkpoint`/`fork`/`undo`/`race` work with any agent (or no agent). The hooks just automate checkpointing; the MCP server (`asp mcp`) works with any MCP-capable harness.

**Does `asp undo` undo my database / external side-effects?**
No. asp versions the file tree under the workspace root. Side-effects outside it (databases, network calls, global installs) are out of scope — that honesty matters.

**What's the difference between `asp undo` and Claude Code's `/rewind`?**
`/rewind` tracks the model's own file edits within a session. asp checkpoints the *whole tree* (untracked files and bash side-effects included), persists across sessions and crashes, and adds forks/diff/promote on top. They compose fine — keep both.

**Symlinks, permissions, empty dirs?**
Symlinks are preserved in forks and checkpoints (as symlinks, like git). Execute bits are preserved. Empty directories aren't checkpointed (git semantics) but survive in forks.

**Windows?**
Not yet. macOS (APFS) and Linux (btrfs/XFS reflink; other filesystems fall back to copy with a warning) ship first.

**What if two asp processes run at once?**
Mutations take an exclusive advisory lock per workspace; concurrent readers are lock-free. A crashed process's lock clears automatically.

**Is my code sent anywhere?**
No. asp is fully local: no network calls, no telemetry, no account. The format is designed for optional bring-your-own-bucket sync later — that will be opt-in and documented when it exists.
