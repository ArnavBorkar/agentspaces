# asp on-disk format (v1)

*The contract between the engine and the world. Changes require a format-version bump and a migration note. Design constraints: every checkpoint recoverable with stock git; assume `kill -9` at any line; content-addressed and sync-ready (S3 conditional-write friendly) for the post-v1 BYO-bucket sync.*

## Layout

```
<workspace root>/                  # the user's real directory
  .asp/
    format-version                 # ASCII integer, currently "1"
    workspace.json                 # identity: {id, created_at, parent: {workspace_id, fork_point}?, label?}
    config.toml                    # user-editable settings (excludes, blob threshold)
    lock                           # advisory exclusive lock for mutations (fs2)
    shadow.git/                    # bare git dir — the checkpoint store
    shadow.index                   # git index for the shadow repo
    journal.jsonl                  # append-only operation journal, CRC-prefixed lines
    blobs/                         # large-file CAS: files named <blake3-hex> (CoW'd in)
    forks.json                     # fork registry (atomic-rename updates)
```

## The two primitives: fork vs checkpoint (different scopes, by design)

- **Fork** = `clonefile(2)` (macOS) / reflink (Linux) of the **whole physical tree** — untracked
  files, `node_modules`, build artifacts, `.env`, everything. O(1)-ish (inode-count-bound),
  ~32 MB overhead for a 3.3 GiB / 100k-file tree. A fork is instantly runnable.
- **Checkpoint** = shadow-git capture of **source-of-truth files** (respects `.gitignore` +
  default excludes for derived state). Commit-grained, diffable, recoverable with stock git.

Forks land at `<parent dir>/<dirname>@<fork-name>` (same volume — required for CoW). Each fork
is a complete workspace: the cloned `.asp` shares shadow objects via CoW and diverges
independently; `workspace.json` is rewritten post-clone with a fresh id + parent pointer.
See [filesystem detection](../filesystems.md) for the platform/filesystem matrix and probe
commands.

## Shadow repo

- Bare repo at `.asp/shadow.git`, worktree forced to the workspace root via env
  (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`) — never a `.git` in the user's tree.
- Config: `core.compression=0`, `gc.auto=0`, `core.untrackedCache=true`.
- `info/exclude` holds: `/.asp/`, the default derived-state excludes
  (`node_modules/`, `target/`, `.venv/`, `__pycache__/`, `build/`, `dist/`, `.next/`),
  user-configured extras from `config.toml`, and one generated line per known large blob.
- The user's `.git` is ignored automatically by git itself; we never read or write it except
  during `promote` (which only ever creates ordinary new branches via local fetch).
- Checkpoints are commits on `refs/asp/checkpoints/<seq>` (one ref per checkpoint,
  monotonically increasing decimal `seq`). No branch is ever checked out; `HEAD` is unused.
  A no-change capture (identical tree to parent) is skipped — no empty checkpoints.

### Stock-git recovery runbook (the trust model)

```bash
GIT_DIR=.asp/shadow.git git log --all              # see every checkpoint
GIT_DIR=.asp/shadow.git GIT_WORK_TREE=out \
GIT_INDEX_FILE=/tmp/i git read-tree <sha> && \
GIT_DIR=.asp/shadow.git GIT_WORK_TREE=out \
GIT_INDEX_FILE=/tmp/i git checkout-index -a -f     # restore any checkpoint, stock git only
# large blobs: pointer files (see below) name .asp/blobs/<hash>; plain cp restores them
```

## Large-blob sidecar

Files larger than `capture.blob_threshold_mb` (default 50) are not stored as git blobs:

1. engine BLAKE3-hashes the file, `clonefile`s it into `.asp/blobs/<hash>` (zero-copy, instant);
2. a generated exclude line hides the real path from `add -A`;
3. a pointer blob is committed at the real path via `hash-object` + `update-index --cacheinfo`:
   `{"asp_ptr":1,"blake3":"<hash>","size":<bytes>,"mode":"100644"}`.

Restore reverses it: `checkout-index`, then clonefile CAS → real path for each pointer.
CAS entries are immutable; garbage collection only via `asp doctor --gc` (post-v1).

## Journal

Append-only `journal.jsonl`. Each line: `<crc32-hex-8> <json>\n` where crc32 covers the JSON
bytes. Entries are written with `O_APPEND` + fsync-on-checkpoint. On open, the engine validates
tail lines and truncates a torn final line (crash recovery); any non-tail corruption = surface
in `asp doctor`, never silently drop.

```json
{"v":1,"ts":"2026-06-11T09:30:00Z","op":"checkpoint","seq":42,"commit":"<sha>",
 "source":"hook|manual|mcp|race","session_id":"...","tool":"Bash","message":"...",
 "files_changed":11,"duration_ms":462}
```

Ops: `init`, `checkpoint`, `fork`, `restore`, `undo`, `promote`, `discard`. The journal is the
audit log: every entry answers *which session/tool/prompt caused this change*.

## forks.json

`{"v":1,"forks":[{"name":"attempt-1","path":"../myrepo@attempt-1","created_at":...,
"fork_point_seq":42,"label":"...","status":"active|promoted|discarded"}]}` — updated via
write-to-temp + atomic rename, under the lock.

## Concurrency & crash safety

- All mutations take the exclusive advisory lock on `.asp/lock` (fs2); reads are lock-free.
- Atomicity: git's own tmp+rename for objects/refs; atomic rename for `forks.json` /
  `workspace.json`; append-only for the journal. Every mutation is one of those three shapes.
- `kill -9` invariants (enforced by the EPIC 6 torture suite):
  1. checkpointed data is never lost;
  2. the store always opens (self-heals torn journal tail);
  3. a torn fork (clone died mid-flight) is detectable (`workspace.json` not yet rewritten)
     and removable by `asp doctor`.

## Sync-readiness (post-v1, designed-in now)

Loose objects, CAS blobs, and refs-as-files are all content-addressed or tiny; a future sync
engine maps them onto S3/R2 with conditional writes (refs) + immutable puts (objects/blobs).
Capability-token fields (`scope`, `ttl`) are reserved in workspace.json but unenforced in v1.

## Versioning

`format-version` is read before anything else. Unknown major ⇒ refuse with corrective hint
("upgrade asp"). Additive JSON fields are non-breaking; journal `v` field versions entries.
