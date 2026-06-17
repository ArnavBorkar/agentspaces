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

## Fixture Library

The benchmark generator supports named fixtures for targeted regressions:

| Fixture | Use it when measuring |
|---|---|
| `monorepo` | The launch benchmark shape: many source files, dependency noise, and large binary assets. |
| `small-files` | Metadata-heavy repositories with huge counts of tiny files. |
| `large-binaries` | Repositories with large media, model, fixture, or build artifacts. |
| `deep-tree` | Very deep paths that stress directory walks, path normalization, and restore/fork materialization. |
| `rename-heavy` | Agent or refactor workloads that move many files before checkpointing. |

Generate a fixture directly:

```bash
python3 scripts/spike/gen_tree.py \
  --root /tmp/asp-small-files \
  --fixture small-files \
  --files 100000 \
  --blob-gb 0
```

Run the full harness against a specific fixture:

```bash
cargo build --release -p asp
python3 scripts/bench/run.py --fixture rename-heavy --files 50000 --blob-gb 0.1
```

`rename-heavy` writes a `rename-plan.tsv`; the harness applies the first
`--edit-count` planned moves before the incremental checkpoint. Other fixtures
append text edits to the first editable files they expose.

## Local Capability Probe

Use `asp bench self` before trusting local benchmark numbers from a new machine
or volume:

```bash
asp bench self
asp -C /path/to/repo --json bench self
```

The command reports the selected path, platform, filesystem kind when the OS
exposes it, the observed directory clone method (`clonefile`, `reflink`, or
`copy`), case sensitivity, symlink/hardlink support, atomic rename behavior, and
recommendations. It creates a short-lived `.asp-bench-self-*` directory under
the target path and removes it before exiting.

## CI Trend Artifact

The CI workflow also runs a lightweight benchmark baseline with a much smaller
synthetic tree and uploads `bench-baseline.md` as a non-blocking artifact. These
CI numbers are useful for spotting rough trends across commits, but they are not
the source for README performance claims. CI also generates a tiny copy of every
fixture so shape changes fail early.
