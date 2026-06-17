# Backup And Disaster Recovery

This runbook helps operators decide how to back up `.asp/`, prove the backup is
usable, and recover when a workspace or store is damaged.

`asp` is local-first. It does not upload checkpoints to a service, and it does
not replace normal repository, endpoint, or secrets backups. The goal is
simple: if a machine, directory, or agent workflow fails, a team should know
which files matter and which command restores confidence.

## What Must Be Backed Up

Back up the entire `.asp/` directory as one unit:

| Path | Why It Matters |
| --- | --- |
| `.asp/format-version` | Tells future `asp` versions how to open the store. |
| `.asp/workspace.json` | Workspace identity and fork ancestry. |
| `.asp/config.toml` | Checkpoint excludes and large-file threshold policy. |
| `.asp/policy.toml` | Local team policy for fork, checkpoint, path, and promote controls. |
| `.asp/shadow.git/` | Ordinary git repository that stores checkpoint commits. |
| `.asp/journal.jsonl` | Append-only operation and provenance log. |
| `.asp/blobs/` | Content-addressed large-file sidecar. |
| `.asp/forks.json` | Registry for sibling forks and cleanup status. |
| `.asp/races/` | Saved race metadata for interrupted or re-rankable runs. |

The safest rule is: copy `.asp/` exactly, do not cherry-pick files inside it.
Leaving out `.asp/blobs/` can make large-file checkpoints unrecoverable.
Leaving out `.asp/shadow.git/` loses the checkpoint commits themselves.

## What `.asp/` Does Not Cover

`asp` intentionally does not back up everything:

- User git history, remotes, hooks, and branches still live in `.git/`.
- Ignored files are not checkpointed by default.
- Forks are whole physical tree copies and may contain secrets, build artifacts,
  caches, and other local-only files.
- Environment variables, shell history, package caches, and external services
  are outside the store.
- `.asp/` is not encrypted by `asp`; rely on endpoint or backup encryption.

For a full business-continuity backup, protect both the repository and `.asp/`.
For source recovery only, `.asp/` plus the stock-git runbook can materialize any
checkpointed files.

## Backup Cadence

Use a cadence that matches how much agent work the team can afford to repeat:

| Workflow | Suggested RPO | Trigger |
| --- | --- | --- |
| Solo evaluation | Daily | Before and after agent sessions. |
| Active pilot repo | 1-4 hours | Before races, before promotion, and at end of day. |
| Regulated or critical repo | Snapshot per workflow | Before granting agent write access and before promotion review. |
| CI or ephemeral runners | Best-effort artifact | Upload diagnostics and race metadata when a lane fails. |

RPO means recovery point objective: the maximum amount of work the team accepts
losing if the local machine disappears.

## Safe Backup Procedure

Prefer filesystem or endpoint snapshots that copy `.asp/` atomically with the
workspace root. If the backup tool is file-level only, pause agents and editors
that might run `asp`, then take a checkpoint and health snapshot first:

```bash
asp checkpoint -m "backup: before scheduled snapshot"
asp doctor --deep
asp --json stats > asp-stats-before-backup.json
asp diagnostics --output asp-diagnostics-before-backup.json
```

Then copy `.asp/` with your normal backup tool:

```bash
rsync -a --delete /path/to/repo/.asp/ /backups/repo/.asp/
```

Notes:

- Run the copy while no `asp` mutation is in progress, or use a real filesystem
  snapshot. This avoids a backup that mixes old and new metadata.
- Do not commit diagnostics bundles or backup manifests to the project unless
  the team has reviewed them.
- Treat `.asp/` backups as source-code backups. They can contain proprietary
  source and non-gitignored secrets.
- Keep `.asp/` on the same local volume as the workspace during normal use so
  fork copy-on-write behavior remains available.

## Restore Drill

Run a drill before a pilot. Do it in a temporary directory so production files
are not touched:

```bash
mkdir -p /tmp/asp-drill
rm -f /tmp/asp-drill.index
GIT_DIR=/path/to/repo/.asp/shadow.git \
GIT_WORK_TREE=/tmp/asp-drill \
GIT_INDEX_FILE=/tmp/asp-drill.index \
git log --all --oneline | head
GIT_DIR=/path/to/repo/.asp/shadow.git \
GIT_WORK_TREE=/tmp/asp-drill \
GIT_INDEX_FILE=/tmp/asp-drill.index \
git read-tree <checkpoint-commit>
GIT_DIR=/path/to/repo/.asp/shadow.git \
GIT_WORK_TREE=/tmp/asp-drill \
GIT_INDEX_FILE=/tmp/asp-drill.index \
git checkout-index -a -f
```

If the checkpoint contains large-file pointer blobs, each pointer names a blob in
`.asp/blobs/<blake3>`. Copy that blob over the pointer path to materialize the
large file with stock tools.

Success criteria:

- `git log --all` lists the expected checkpoints.
- A recent checkpoint materializes into the temporary directory.
- Large-file pointer paths can be matched to files in `.asp/blobs/`.
- `asp doctor --deep` is clean in the original workspace.
- The team can explain where `.git/` and `.asp/` are backed up separately.

## Recovery Scenarios

### Working Tree Damaged, `.asp/` Healthy

Use normal `asp` recovery:

```bash
asp status
asp log -n 10
asp restore <checkpoint-seq>
asp doctor
```

If only a few files were damaged, pass relative paths after the checkpoint:

```bash
asp restore <checkpoint-seq> src/lib.rs README.md
```

### `.asp/` Damaged, Backup Available

Preserve the broken store first, then restore the backup:

```bash
mv .asp ".asp.broken.$(date -u +%Y%m%dT%H%M%SZ)"
rsync -a /backups/repo/.asp/ .asp/
asp doctor --deep
asp log -n 10
```

If `asp doctor --deep` reports a missing CAS blob, restore `.asp/blobs/` again
from backup before trusting large-file checkpoints.

### Machine Lost, Git Remote Available

Restore the source repository from git, then restore `.asp/` from endpoint or
backup storage:

```bash
git clone <repo-url> repo
cd repo
rsync -a /backups/repo/.asp/ .asp/
asp doctor --deep
asp log -n 10
```

This recovers the user git repo from the remote and the agent checkpoint history
from `.asp/`.

### `asp` Binary Unavailable

Use stock git against `.asp/shadow.git`:

```bash
mkdir -p /tmp/asp-recovered
rm -f /tmp/asp-recovered.index
GIT_DIR=/path/to/repo/.asp/shadow.git git show-ref refs/asp/checkpoints/42
GIT_DIR=/path/to/repo/.asp/shadow.git \
GIT_WORK_TREE=/tmp/asp-recovered \
GIT_INDEX_FILE=/tmp/asp-recovered.index \
git read-tree refs/asp/checkpoints/42
GIT_DIR=/path/to/repo/.asp/shadow.git \
GIT_WORK_TREE=/tmp/asp-recovered \
GIT_INDEX_FILE=/tmp/asp-recovered.index \
git checkout-index -a -f
```

This restores checkpointed source files only. It does not recreate user git
branches, hooks, ignored files, or active fork directories.

### Fork Or Race Cleanup Needed

First inspect, then repair:

```bash
asp forks
asp doctor --deep
asp doctor --fix --deep
```

`asp doctor --fix` only removes fork directories that the registry proves were
created by `asp` and left torn or stale. It should not be used as a substitute
for reviewing active fork work before deletion.

## Incident Checklist

When a workspace report mentions possible data loss:

- Stop running agents in the affected workspace.
- Copy the current `.asp/` directory to a safe location before repair attempts.
- Run `asp diagnostics --output asp-diagnostics.json`.
- Run `asp doctor --deep` and save the output.
- Identify the last known-good checkpoint with `asp log -n 20`.
- Verify whether the missing file was checkpointed, ignored, or only present in
  a fork.
- Restore into a temporary directory with stock git before overwriting the
  production working tree.
- Keep `.asp.broken.*` until maintainers confirm it is no longer needed.

For public issues, attach the redacted diagnostics bundle by default. Use
`--include-paths` only in a trusted support channel.

## Pilot Readiness Checklist

Before adopting `asp` in an enterprise repo:

- Endpoint backup includes `.asp/` and the repository root.
- Backup encryption and retention match source-code requirements.
- Restore drills cover one normal checkpoint and one large-file checkpoint.
- Operators know that `.git/` and `.asp/` are separate recovery domains.
- Teams know whether ignored files are intentionally excluded from checkpoints.
- Fork retention and discard policy are documented for agent races.
- Security reviewers have read the [trust model whitepaper](trust-model.md).

## Related Docs

- [Trust model whitepaper](trust-model.md)
- [On-disk format](design/format.md)
- [Diagnostics bundles](diagnostics.md)
- [Filesystem feature detection](filesystems.md)
- [Enterprise workflow playbooks](playbooks.md)
