# Spike results — EPIC 1 de-risk benchmarks

*2026-06-11 · machine: Apple M3 Pro, macOS 15.5, APFS, 460GB SSD · synthetic monorepo: 100,026 files, 3.28 GiB (200 packages of Rust-like source 1–20KB, 20k-file node_modules-style noise dir, 24 incompressible binary assets totaling 3 GiB) · generator: `scripts/spike/gen_tree.py --files 100000 --blob-gb 3 --seed 42`*

## Verdict: **GO.** Both existential claims hold.

## 1. Whole-directory CoW fork (`scripts/spike/bench_fork.py`)

| Strategy | Wall time | Extra disk |
|---|---:|---:|
| **`clonefile(2)` on dir root (asp fast path)** | **906–943 ms** | **32 MB** |
| `cp -cR` (userspace walk, per-file clone) | 10,617 ms | 31 MB |
| `cp -R` (full copy) | 26,967 ms | 3,665 MB |
| `git worktree add` (tracked files only) | 13,828 ms | n/a |

- Kernel-recursive `clonefile` is **11x faster than per-file cloning and 15x faster than `git worktree add`** — and unlike worktrees it carries untracked files, `.env`, and build artifacts.
- Cost scales with **inode count, not bytes** (3.3 GiB forked for 32 MB of metadata). Typical repos (1k–50k files) will fork in **10–500 ms**.
- 100k files ≈ 0.9s on this machine (Apple M3 Pro). "Sub-second fork on a 100k-file tree" is honest on this hardware.

## 2. Change-detection scan (`crates/asp-core/examples/walk_bench.rs`, release build)

| Scan | Time |
|---|---:|
| Cold-ish first walk, 100k files | 408 ms |
| Warm walk (steady state) | **263 ms** |

Single-threaded `walkdir` + lstat. No parallelism needed for v1.

## 3. Shadow-git capture (`scripts/spike/bench_shadow_git.py`, compression=0)

| Operation | Time |
|---|---:|
| Initial capture, full tree incl. 3 GiB blobs | 65.9 s |
| Initial capture, blobs excluded | 38.1 s |
| **Incremental checkpoint (11 changes) — the auto-checkpoint hot path** | **462 ms** |
| No-op rescan (nothing changed) | 252 ms |
| Shadow rescan with user `.git` present | 663 ms |
| Full-tree restore to fresh dir (3.3 GiB materialized) | 24.0 s |

- **Byte-identical restore via stock git plumbing: PASS** (`git read-tree` + `checkout-index` into a fresh dir, `diff -r` clean). The trust model — *everything recoverable with plain git* — is demonstrated, not claimed.
- **User-repo coexistence: PASS** — sidecar `GIT_DIR` under `.asp/` with `GIT_WORK_TREE` pointed at the user dir; git auto-ignores the user's `.git`; user history untouched.
- Untracked files are captured (a brand-new untracked file appeared in the checkpoint and survived restore).

## Decisions locked by this data (→ format doc)

1. **Fork = `clonefile`/reflink of the whole physical tree** (deps, artifacts, everything — that's what makes forks instantly runnable). **Checkpoint = shadow-git capture of source-of-truth files.** Two different operations with two different scopes, by design.
2. **Default capture excludes** (configurable): `node_modules/`, `target/`, `.venv/`, `build/`, `dist/`, `__pycache__/` — derived state, rebuildable from lockfiles; forks still carry them physically. This collapses the 38s initial capture to seconds on real repos (our noise dir was 20k of the 100k files).
3. **Large-blob sidecar**: files over a threshold (default 50 MB) are BLAKE3-hashed into a CAS dir (`.asp/blobs/<hash>`) via `clonefile` (instant, zero-copy), with a small pointer file committed to shadow git. Cuts the blob share of initial capture (~30s for 3 GiB) to hash time only, keeps `asp` stores compact, and keeps a documented plain-`cp` recovery runbook.
4. **Initial capture runs on first `checkpoint`, not on `init`** — `asp init` must feel instant; first checkpoint shows a progress bar on big trees.
5. **Restore default is file-level and targeted** (restore what changed, not the whole tree); full-tree materialization (24s) is the worst case, and fork-from-checkpoint covers the "give me a clean copy" case via clonefile instead.
6. Shadow repo config: `core.compression=0` (objects are mostly already-compressed or cheap; CPU is the scarce resource), `gc.auto=0` (asp schedules its own maintenance), `core.untrackedCache=true`.

## Honest caveats

- Numbers are from one Apple M3 Pro Mac; Linux reflink (btrfs/XFS) path not yet measured (EPIC 6 CI matrix).
- `clonefile` requires same-volume destination; cross-volume falls back to copy with a warning (engine handles this).
- The 100k-file synthetic tree is a stress case, deliberately harsher than typical repos.
