# Trust Model Whitepaper

This document is for security reviewers, platform teams, and maintainers who
need to decide whether `asp` can be piloted in an enterprise development
environment.

`asp` is a local storage and workflow tool for AI-agent work. It creates
durable checkpoints and branchable workspace forks over real directories. The
central trust claim is deliberately narrow:

> If `asp` breaks or disappears, every checkpoint remains recoverable with
> stock `git`, and the user's own repository history is not rewritten.

## Executive Summary

`asp` is trusted with local source trees, untracked files that are in checkpoint
scope, forked working copies, and the `.asp/` sidecar store. It is not a
sandbox, policy engine, secret scanner, malware detector, or hosted sync
service.

The design favors boring, inspectable primitives:

- Checkpoints are ordinary commits in `.asp/shadow.git`.
- The operation journal is append-only JSONL with CRC framing.
- Forks are real sibling directories, not virtual overlays.
- Store metadata is written with atomic rename.
- Promotion creates ordinary user-git branches and never force-pushes or moves
  existing user history.
- CLI and MCP outputs are scriptable through stable JSON envelopes.

The highest-value review question is not "can agents still do dangerous
things?" They can, because they run in a real working directory with the
permissions the user gives them. The review question is whether `asp` gives the
team a durable, inspectable safety layer around that work without hiding state
in a proprietary service or corrupting the user's git repository.

## Assets

`asp` protects or manages these assets:

| Asset | Why It Matters | Where It Lives |
| --- | --- | --- |
| User working tree | Agents edit real files, including untracked files. | Workspace root |
| User git repo | Existing branches, refs, hooks, config, and history must not be rewritten. | `.git/` |
| Checkpoint history | Recovery and audit timeline for in-scope files. | `.asp/shadow.git`, `.asp/journal.jsonl` |
| Fork registry | Tracks sibling workspaces and cleanup state. | `.asp/forks.json` |
| Large-file CAS | Stores checkpointed large files outside git objects. | `.asp/blobs/` |
| Provenance | Records manual, hook, MCP, or race sources for operations. | `.asp/journal.jsonl` |
| Diagnostics | Issue-report bundles that may describe local state. | User-selected output path |

## Trust Boundaries

### Inside The Boundary

The local `asp` binary is trusted to:

- Read and write `.asp/`.
- Call the local `git` binary for shadow-git and user-git operations.
- Create sibling fork directories on the same filesystem.
- Install or remove supported harness integration files when explicitly asked.
- Return accurate human and JSON output for automation.

### Outside The Boundary

`asp` does not assume control over:

- The user's operating system, filesystem driver, shell, or editor.
- The behavior of the AI agent or commands the user runs in a fork.
- Network access, package managers, language toolchains, or CI providers.
- Secrets already present in the working directory or environment.
- The user's remote git hosting service.

If an agent can read a file or environment variable, `asp` does not prevent the
agent from exfiltrating it. Use normal endpoint, network, and credential
controls around agent processes.

## Security Goals

### G1. Stock-Git Recovery

Every checkpoint is recoverable without `asp`:

```bash
GIT_DIR=.asp/shadow.git git log --all
GIT_DIR=.asp/shadow.git GIT_WORK_TREE=out \
GIT_INDEX_FILE=/tmp/asp-index git read-tree <checkpoint-commit>
GIT_DIR=.asp/shadow.git GIT_WORK_TREE=out \
GIT_INDEX_FILE=/tmp/asp-index git checkout-index -a -f
```

Large-file pointer blobs name immutable files in `.asp/blobs/`; copying the
named blob materializes the file content.

Evidence:

- [On-disk format recovery runbook](design/format.md#stock-git-recovery-runbook-the-trust-model)
- `stock_git_recovery_runbook_works` integration test

### G2. User Git Is Sacred

`asp` does not write to `.git/` during init, checkpoint, fork, restore, undo,
discard, doctor, diagnostics, MCP calls, or race comparison.

The intentional exception is `asp promote <fork>`, which creates a new ordinary
branch in the user's git repository. It does not force-push, rewrite existing
history, move `HEAD`, or include `.asp/` internals in the promoted tree.

Evidence:

- `user_git_dir_never_captured`
- `promote_lands_branch_in_user_repo`
- `promote_without_user_git_errors_helpfully`

### G3. Crash-Safe Store Mutations

Assume `kill -9` at any line. Mutations must be one of:

- Git object/ref writes through git's atomic mechanisms.
- Atomic rename of complete metadata files.
- Append-only journal writes with CRC framing.

The torture suite kills real `asp` processes during checkpoint, fork, and
restore, then verifies the store opens and checkpointed data is not lost.

Evidence:

- `crates/asp/tests/torture.rs`
- `journal_survives_truncation_anywhere`
- `journal_corruption_never_fabricates`

### G4. Path Containment

Paths read from `.asp/`, git trees, config, or user input must not escape the
workspace root when restored, deleted, or inspected.

Evidence:

- `restore_rejects_unsafe_store_paths`
- Store safe-path validation in `asp_core::store`

### G5. Agent-Readable Automation

Every CLI command supports `--json`. Errors include a stable machine-readable
code and a corrective `hint` when the next action is knowable.

Evidence:

- [JSON schemas](schemas.md)
- CLI JSON snapshot tests
- MCP session and shape tests

## Non-Goals

`asp` does not promise:

- Sandboxing of agent commands.
- Secret detection or prevention of prompt/log exfiltration.
- Encryption at rest for `.asp/`.
- Multi-user access control inside one local checkout.
- Malware scanning of generated code.
- Replacement of code review, branch protection, CI, or dependency policy.
- Recovery of files intentionally excluded from checkpoint scope.
- Native Windows workspace support in the current release line.

## Data Scope

### Forks

Forks are whole physical tree copies. They include files that checkpoints may
exclude, such as ignored build artifacts or `.env` files. This is intentional:
a fork should be runnable with the same local context as the parent workspace.

Security implication: if the parent directory contains secrets, sibling forks
contain them too. Treat forks like local working copies with the same
confidentiality requirements.

### Checkpoints

Checkpoints capture tracked plus untracked source-of-truth files, minus:

- Files ignored by `.gitignore`.
- Default derived-state excludes such as `node_modules/`, `target/`, `.venv/`,
  build output, and `.asp/`.
- Extra excludes configured in `.asp/config.toml`.
- Large files moved to the CAS sidecar and represented by pointer blobs.

Security implication: gitignored secrets are not checkpointed by default, but
non-gitignored secrets are in scope. Teams should pair `asp` with normal secret
handling and `.gitignore` policy.

### Diagnostics

`asp diagnostics` redacts local paths and secret-like values by default. Users
can opt into full paths with `--include-paths`, which should be used only in a
trusted support channel.

Evidence:

- [Diagnostics guide](diagnostics.md)
- `diagnostics_redacts_paths_and_secretish_messages_by_default`

## Command Write Behavior

| Command | Writes `.asp/` | Writes User Files | Writes User `.git/` |
| --- | --- | --- | --- |
| `asp init` | Creates sidecar | No source edits | No |
| `asp checkpoint` | Yes | No | No |
| `asp restore` | Yes, safety/post checkpoints | Yes, restores files | No |
| `asp undo` | Yes, via restore | Yes, restores files | No |
| `asp fork` | Yes, registry | Creates sibling fork | No |
| `asp forks` | May refresh fork shadow indexes | No parent edits | No |
| `asp race` | Yes, forks and race metadata | Writes inside forks | No |
| `asp race compare` | Reads race metadata and fork diffs | No | No |
| `asp promote` | Updates fork status | No parent source edits | Creates one branch |
| `asp discard` | Updates registry | Deletes selected fork dir | No |
| `asp doctor --fix` | Repairs proven asp state | May remove proven torn forks | No |
| `asp diagnostics` | No by default | Writes selected bundle path | No |
| `asp setup claude` | No store mutation required | Writes harness config files | No |

## Failure Modes And Mitigations

| Failure Mode | Mitigation |
| --- | --- |
| Process killed mid-journal append | CRC detects torn tail; read/heal keeps valid prefix. |
| Process killed mid-fork | Pending registry entry lets `doctor` reason about cleanup. |
| Process killed mid-restore | Restore starts with a safety checkpoint and ends with a post-restore checkpoint. |
| Shadow git index lock left behind | Shadow git retries transient index-lock contention. |
| Large-file checkpointing | CAS blobs are immutable and pointer manifests are tested. |
| Case-only path rename on case-insensitive FS | Staging removes stale index spelling before adding real path. |
| User exports dangerous `GIT_*` env vars | Shadow and user-git calls scrub repo-location env vars. |
| Unsupported Windows semantics | CLI fails closed with `unsupported_platform` until native support lands. |

## Supply-Chain Review Points

Reviewers should inspect:

- Release artifact checksums and Sigstore verification:
  [release verification](release-verification.md)
- Dependency policy:
  [dependency governance](dependency-governance.md)
- CI gates:
  `.github/workflows/ci.yml`
- Published automation schemas:
  [JSON schemas](schemas.md)
- Development and review checklist:
  [development guide](development.md)

## Reviewer Checklist

Ask these questions before a pilot:

- Which repos are allowed to store `.asp/` locally?
- Which directories are too sensitive to fork wholesale?
- Are `.gitignore` and `.asp/config.toml` aligned with the team's secret and
  generated-file policy?
- Is `.asp/` included in endpoint backup for pilot repos?
- Who can run `asp promote`, and what branch naming policy applies?
- Which agent credentials are safe in sibling forks?
- Which diagnostics bundles may be shared externally?
- Which test command proves the target workflow before promotion?

## Evidence Map

| Claim | Evidence |
| --- | --- |
| Stock-git recovery works | `stock_git_recovery_runbook_works`, [format runbook](design/format.md) |
| Store survives killed processes | `crates/asp/tests/torture.rs` |
| Journal truncation/corruption is bounded | `crates/asp-core/tests/properties.rs` |
| User `.git` is not captured | `user_git_dir_never_captured` |
| Promotion excludes `.asp/` | `promote_lands_branch_in_user_repo` |
| JSON output is stable | `crates/asp/tests/json_snapshots.rs`, [schemas](schemas.md) |
| Diagnostics redact by default | `diagnostics_redacts_paths_and_secretish_messages_by_default` |
| Filesystem behavior is documented | [filesystem detection](filesystems.md), [Windows status](windows.md) |

## Residual Risk

The most important residual risks are operational:

- A user can still run an unsafe agent command.
- A fork can copy secrets present in the parent directory.
- A non-gitignored secret can be checkpointed.
- Local disk compromise exposes `.asp/` unless the endpoint encrypts storage.
- A future hosted service must remain additive and must not weaken local
  recoverability.

For enterprise pilots, treat `asp` as a local safety and review layer around
agent work, not as a replacement for endpoint security, CI, code review, or
credential governance.
