# agentspaces architecture

This is the short map for contributors who want to change `asp` without
breaking the trust model. The full on-disk contract lives in
[docs/design/format.md](design/format.md).

## Shape of the system

`asp` is one Rust workspace with two crates:

- `crates/asp-core` is the engine. It owns the `.asp/` store, checkpointing,
  forks, restore, diff, promote, discard, doctor, journal, and config.
- `crates/asp` is the binary. It exposes the engine as a CLI, an MCP stdio
  server, Claude Code hooks, human-readable tables, and `asp race`.

The important rule is that CLI/MCP/UI code should stay thin. If a behavior
changes workspace state, it belongs in `asp-core` first and should be tested
there before a command or tool exposes it.

## Runtime model

Every workspace has a sidecar directory:

```text
project/
  .asp/
    format-version
    workspace.json
    config.toml
    lock
    shadow.git/
    shadow.index
    journal.jsonl
    blobs/
    forks.json
```

The user's `.git` remains separate. The only command that intentionally writes
to the user's git repo is `asp promote`, and it creates an ordinary branch.

There are two storage primitives:

- **Fork** copies the whole physical directory into a sibling path such as
  `project@race-1`. On macOS this uses `clonefile(2)`; on Linux it uses
  reflink when available; otherwise it falls back to a normal copy. Forks are
  for runnable parallel workspaces.
- **Checkpoint** captures source-of-truth files into `.asp/shadow.git`.
  Checkpoints respect `.gitignore` and configured excludes. Large files are
  stored once in `.asp/blobs/` and represented in git by pointer blobs.

## Main modules

- `asp_core::workspace` is the primary API. `Workspace` implements
  `init/open/status/stats/diagnostics/checkpoint/log/undo/restore/fork/forks/diff/promote/discard/doctor`.
- `asp_core::store` defines the `.asp/` layout, workspace identity, fork
  registry, advisory lock, safe path validation, and atomic writes.
- `asp_core::gitx` wraps shadow-git subprocess calls and pins the git
  environment so user config and user `.git` do not leak into the store.
- `asp_core::journal` implements the append-only CRC journal and torn-tail
  recovery.
- `asp_core::fork` implements platform-specific whole-tree copy-on-write
  cloning and copy fallback.
- `asp_core::blobs` implements the large-file content-addressed sidecar.
- `asp_core::config` owns capture excludes and default config templates.
- `crates/asp/src/main.rs` maps CLI commands to `Workspace` calls and formats
  JSON or human output.
- `crates/asp/src/mcp.rs` maps MCP tool calls to the same `Workspace` API.
- `crates/asp/src/hooks.rs` installs Claude Code hooks and handles hook
  checkpoint events without failing the agent session.
- `crates/asp/src/race.rs` creates forks, runs one command per lane, records
  results, and compares fork diffs.

## Mutation rules

Assume the process can die at any line.

- Write structured store files with temp-file plus atomic rename.
- Append journal records with CRC and fsync at checkpoint boundaries.
- Let git write git objects and refs through its own atomic mechanisms.
- Hold the workspace lock for mutating operations.
- Validate every path read from `.asp/` before joining it onto the workspace
  root.
- Add torture or regression coverage for new mutation paths.

## Data flow

Typical checkpoint:

1. CLI, MCP, or hook builds `CheckpointOpts` with provenance.
2. `Workspace::checkpoint` takes the store lock.
3. The engine scans for changed, untracked, deleted, and large files.
4. Large files are cloned into `.asp/blobs/` and pointer blobs are staged.
5. Shadow git stages source files and writes `refs/asp/checkpoints/<seq>`.
6. Metadata lands at `refs/asp/meta/<seq>`.
7. The journal appends a checkpoint entry with source/session/tool details.

Typical fork:

1. `Workspace::fork` registers a `Pending` fork in `.asp/forks.json`.
2. The physical tree is cloned into a sibling directory.
3. The fork's `.asp/workspace.json` is rewritten with a new workspace id and
   parent pointer.
4. The registry entry flips to `Active`.
5. If creation dies mid-flight, `doctor` can reason from the pending intent.

## Test expectations

Run the standard loop before pushing:

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Use focused tests while iterating, but finish with the full suite. Storage
changes should normally add or update tests under `crates/asp-core/tests/` or
`crates/asp/tests/torture.rs`; CLI/MCP changes should include command or
protocol-level coverage under `crates/asp/tests/`.
