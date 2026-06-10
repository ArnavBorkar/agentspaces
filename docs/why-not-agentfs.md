# Why not AgentFS or a virtual filesystem?

Turso's AgentFS and similar projects give agents a branchable filesystem by interposing a database (e.g. SQLite) between the agent and its files. It's clever engineering, and for fully-sandboxed, platform-managed agents it can be the right call. asp makes a different bet.

## asp versions your real directory

With a virtual filesystem, your project's files live *inside the substrate* — a database file, a FUSE mount, a server. Every other tool in your life now needs an adapter: your editor, your LSP, `rg`, `make`, Docker bind mounts, and git itself either go through the VFS layer or see nothing.

asp's bet: **the files stay exactly where they are, in the real filesystem.** Your editor, toolchain, and muscle memory keep working unchanged, because nothing changed. The version layer is a sidecar (`.asp/`), not a custody arrangement. Filesystem-level CoW (APFS clonefile, btrfs/XFS reflink) supplies the cheap branching that a VFS would otherwise have to simulate.

## Three consequences of that bet

1. **No lock-in by construction.** asp's checkpoint store is an ordinary git repository; the large-file store is plain content-addressed files. The [recovery runbook](design/format.md) uses stock git and `cp`. Delete `.asp/` and your project is simply... your project. A VFS, by contrast, *is* the data — if it breaks or you leave, you need an export path.
2. **Git interop is native, not a bridge.** `asp promote` lands work as a real git branch because the engine speaks git all the way down. PR-based review, CI, and the entire existing SDLC work with zero glue.
3. **The boring failure mode.** If asp crashes mid-write (we kill -9 it in CI to make sure), the worst case is a stale lock file and a torn fork directory that `asp doctor --fix` removes. If a VFS corrupts its database, your *files* are what's at stake.

## What a VFS does better

Honesty requires the other column. A database-backed filesystem can: intercept reads/writes from *any* process without hooks; enforce quotas and copy-on-write at byte granularity on filesystems without reflink support; and offer time-travel over every write, not just checkpoint boundaries. If you're building a hosted platform where agents must be fully sandboxed and metered, that shape may serve you better — that's a different product for a different buyer.

asp is for the engineer whose agents work in real repos on a real machine, who wants durable, branchable, auditable state **without moving in** to someone else's filesystem.
