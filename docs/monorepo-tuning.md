# Monorepo Tuning

This guide helps teams tune `asp` for large repositories with many packages,
generated outputs, build caches, binary assets, or agent race lanes.

The most important distinction:

- `asp fork` copies the whole physical tree so each lane is runnable.
- `asp checkpoint` captures source-of-truth files into `.asp/shadow.git`.

Tuning is mostly about deciding which local files are source of truth, which
files are derived, which large files need CAS storage, and which filesystem will
make forks cheap.

## Quick Checklist

Before the first serious agent workflow:

```bash
asp init
# Edit .asp/config.toml if defaults miss local derived state.
asp --json status
asp checkpoint -m "baseline after monorepo tuning"
asp --json stats
asp doctor --deep
```

Then answer:

- Are generated directories excluded from checkpoints?
- Are large binary assets going to `.asp/blobs/` instead of shadow-git blobs?
- Does `asp --json fork --name fs-probe` report `clonefile` or `reflink` on
  this filesystem?
- Can a recent checkpoint be restored in a scratch directory?
- Does endpoint backup include `.asp/` before pilots begin?

## Exclude Policy

Checkpoints respect the repo's normal `.gitignore` plus `.asp/config.toml`.
Forks still carry excluded files physically.

Use `.gitignore` for files that should be ignored by every tool:

- dependency directories such as `node_modules/`;
- build outputs such as `target/`, `dist/`, `bazel-bin/`, or `build/`;
- local secrets such as `.env`;
- editor, OS, and cache files.

Use `.asp/config.toml` for files that should be omitted from `asp`
checkpoints, while leaving the repo's normal ignore policy unchanged.

Defaults already exclude:

```text
node_modules/
target/
.venv/
venv/
__pycache__/
build/
dist/
.next/
.cache/
```

Prefer `capture.extra_excludes` so defaults remain active:

```toml
[capture]
extra_excludes = [
  "bazel-bin/",
  "bazel-out/",
  "bazel-testlogs/",
  ".gradle/",
  ".turbo/",
  ".nx/cache/",
  "coverage/",
  "tmp/",
]
```

Use `capture.excludes` only when the team wants to own the full list:

```toml
[capture]
excludes = [
  "node_modules/",
  "target/",
  "bazel-bin/",
  "bazel-out/",
  ".gradle/",
]
```

Changing config affects future checkpoints only. It does not rewrite old
checkpoints.

## What Not To Exclude

Do not exclude files just because they are large or noisy. Exclude only when the
team agrees the file is derived or intentionally outside checkpoint scope.

Keep these in scope unless another system is the source of truth:

- generated code that is reviewed and committed;
- migration files;
- API schemas and lockfiles;
- test fixtures needed to reproduce failures;
- small binary fixtures that are part of tests;
- build scripts and toolchain manifests.

If a file is required for code review, promotion, or rollback, it should be in a
checkpoint or backed up somewhere with an explicit recovery path.

## Blob Threshold

Files larger than `capture.blob_threshold_mb` are stored once in
`.asp/blobs/` and represented in checkpoint commits by small pointer blobs. The
default is 50 MB.

Lower the threshold when the repo has many assets, model files, screenshots, or
datasets that change during agent work:

```toml
[capture]
blob_threshold_mb = 10
```

Keep the default for mostly-source repositories where large files are rare.
Raise the threshold only when a team deliberately wants medium-size files stored
as ordinary git blobs in the shadow repo.

Tradeoffs:

| Threshold | Best For | Cost |
| --- | --- | --- |
| 5-10 MB | media, ML, design fixtures, large snapshots | More CAS entries to back up and deep-check. |
| 50 MB | general monorepos | Balanced default. |
| 100+ MB | rare large files, simpler shadow-git inspection | Larger shadow-git objects and slower first capture. |

Run `asp doctor --deep` after large-file policy changes. Deep doctor verifies
CAS blob existence and hashes known blobs.

## Filesystem Choice

Fork performance depends on the filesystem under the workspace root:

| Filesystem | Expected Fork Method | Guidance |
| --- | --- | --- |
| macOS APFS | `clonefile` | Best current path for local pilots. |
| Linux btrfs | `reflink` | CI exercises this path. Good for Linux pilots. |
| Linux XFS with reflink | `reflink` | Good when formatted with reflink support. |
| Linux ext4 | `copy` | Correct but slower and more disk-heavy for large trees. |
| Network or cloud-sync folders | `copy` or error | Avoid for primary agent work unless a probe proves acceptable behavior. |

Probe before a pilot:

```bash
asp --json fork --name fs-probe
asp discard fs-probe
```

Look at `result[0].method` in JSON output. If it says `copy`, forks still work,
but large monorepo lanes may take more time and disk.

Keep the workspace and its sibling forks on the same local volume. Cross-volume
copies cannot use copy-on-write.

## Common Presets

### JavaScript Or TypeScript Monorepo

```toml
[capture]
extra_excludes = [
  ".turbo/",
  ".nx/cache/",
  "coverage/",
  "storybook-static/",
]
blob_threshold_mb = 50
```

Keep lockfiles and generated clients in scope if they are reviewed.

### Rust Or Polyglot Monorepo

```toml
[capture]
extra_excludes = [
  "coverage/",
  "tmp/",
  ".pytest_cache/",
  ".mypy_cache/",
]
blob_threshold_mb = 50
```

The default `target/`, `.venv/`, `venv/`, and `__pycache__/` excludes already
cover common derived state.

### Bazel, Buck, Or Pants

```toml
[capture]
extra_excludes = [
  "bazel-bin/",
  "bazel-out/",
  "bazel-testlogs/",
  "buck-out/",
  ".pants.d/",
]
blob_threshold_mb = 50
```

Keep BUILD files, lockfiles, generated source manifests, and toolchain configs
in scope.

### Media, ML, Or Fixture-Heavy Repo

```toml
[capture]
extra_excludes = [
  "runs/",
  "checkpoints/",
  "wandb/",
  "data/raw/",
]
blob_threshold_mb = 10
```

Do not exclude fixtures that tests require unless the test harness can fetch
them deterministically.

## Diagnose Slow Or Large Workspaces

| Symptom | Check | Likely Fix |
| --- | --- | --- |
| First checkpoint takes minutes | Derived directories are in scope. | Add `extra_excludes`, then checkpoint again. |
| Incremental checkpoint is slow | Agent changed many files or generated outputs. | Exclude regenerated outputs, or narrow the agent task. |
| Forks consume full disk copies | `asp --json fork` reports `copy`. | Move repo to APFS, btrfs, or reflink-enabled XFS. |
| `.asp/blobs/` grows quickly | Large files change often. | Exclude derived assets, reduce asset churn, or document the storage cost. |
| Restore misses a file | File was ignored or excluded. | Move it out of ignore scope or document another recovery source. |
| Promotion branch is noisy | Generated code or outputs were edited. | Re-run generation deterministically, then exclude derived output when appropriate. |

Useful commands:

```bash
asp --json status
asp --json stats
asp doctor --deep
du -sh . .asp
find . -path ./.git -prune -o -path ./.asp -prune -o -type f | wc -l
```

## Rollout Pattern

1. Add `.asp/config.toml` tuning in a small PR.
2. Run a baseline checkpoint and `asp doctor --deep`.
3. Create one probe fork and confirm the filesystem method.
4. Run one agent task against a low-risk package.
5. Restore a recent checkpoint in a scratch directory.
6. Record the chosen excludes, blob threshold, filesystem, and backup location.

Revisit tuning after the first week of pilot data. The right policy is the one
that keeps checkpoints reviewable while still recovering every file the team
expects `asp` to protect.

## Related Docs

- [Workspace config](config.md)
- [Filesystem feature detection](filesystems.md)
- [Backup and disaster recovery](backup-recovery.md)
- [Benchmarks](benchmarks/BENCHMARKS.md)
- [On-disk format](design/format.md)
