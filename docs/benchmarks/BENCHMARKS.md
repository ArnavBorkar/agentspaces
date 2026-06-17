# asp benchmarks

*2026-06-11 · macOS-15.5-arm64-arm-64bit · arm*

Tree: generated 100026 files, 3.28 GiB in 14.8s at /Users/apple/Projects/agentspaces/bench-data/bench-tree

| Operation | Time |
|---|---:|
| init | 528 ms |
| first checkpoint (sidecar active) | 43.6 s |
| incremental checkpoint (10 edits) | 666 ms |
| no-op checkpoint | 265 ms |
| status | 168 ms |
| fork (whole tree, CoW) | 1224 ms |
| forks compare | 4.6 s |
| cp -R baseline | 26.0 s |
| git worktree add baseline (tracked only) | 9.8 s |

`.asp` store size after benchmarks: 3.0G

Methodology: every number is one cold run of the release binary via subprocess (includes process startup, ~5ms). Generator and harness are in `scripts/`; reproduce with `python3 scripts/bench/run.py`.

## CI Trend Artifact

The CI workflow also runs a lightweight benchmark baseline with a much smaller
synthetic tree and uploads `bench-baseline.md` as a non-blocking artifact. These
CI numbers are useful for spotting rough trends across commits, but they are not
the source for README performance claims.
