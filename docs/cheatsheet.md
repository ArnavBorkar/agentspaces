# Command cheat sheet

Use this as the daily `asp` workflow map.

## Start safely

```bash
asp init
asp status
asp checkpoint -m "baseline"
asp setup claude
asp setup codex
asp setup opencode
```

## Recover work

```bash
asp log
asp undo
asp restore 12
asp restore 12 path/to/file
asp doctor --explain
asp doctor --fix
```

## Compare changes

```bash
asp diff
asp diff --patch 3 7
asp diff --stat 3
asp diff --html --output review.html 3
asp review --json
```

## Run agent races

```bash
asp race -n 3 -- claude -p "make tests pass"
asp race -n 3 --timeout 10m --retries 1 -- npm test
asp race compare --name race
asp forks
```

## Land a winner

```bash
asp promote race-2
asp promote race-2 --push --remote origin
asp promote race-2 --push --remote origin --pr-draft
asp discard race-1
asp discard race-3 --force
```

## Audit and policy

```bash
asp audit --path src/lib.rs
asp audit --format jsonl
asp policy validate --json
asp retention plan
asp secrets scan
```

## Sync and support

```bash
asp sync push --remote /path/to/remote
asp sync fetch --remote /path/to/remote
asp stats --json
asp diagnostics --output diagnostics.json
```

## Shell and packaging

```bash
asp completions zsh > ~/.zfunc/_asp
asp manpage > asp.1
asp schema --json
```
