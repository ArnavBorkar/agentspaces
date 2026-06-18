# Quickstart

Run the guided workflow from any directory:

```bash
asp quickstart
```

The command is read-only. It detects whether the current directory is already
inside an `asp` workspace and prints the next safe commands to run. Use JSON
when wiring onboarding into scripts or internal docs:

```bash
asp --json quickstart
```

## First five minutes

```bash
asp init
asp status
asp checkpoint -m "baseline"
asp setup codex
asp race -n 3 -- <agent command>
asp forks
asp diff --fork <name>
asp promote <name>
```

`asp init` creates only `.asp/` metadata. It does not capture file contents and
does not write to the user's `.git`. The first real snapshot happens when you
run `asp checkpoint`.

## Recovery muscle memory

```bash
asp undo
asp restore 1
asp doctor --explain
```

Keep these close while evaluating agent workflows. The fastest way to trust a
new workspace tool is to prove you can recover from a bad edit before anything
important depends on it.
