# Sync Remote Recovery

This runbook is for the uncomfortable case where a sync remote survived but the
local `.asp/` directory did not. Treat the remote as source-code backup data:
checkpoint git objects and large-file blobs can contain proprietary source and
non-gitignored secrets.

The preferred restore path is still an ordinary `.asp/` backup. Restore that
store first, then run `asp doctor --deep` and `asp sync fetch --remote <dir>`.
A full `.asp/` backup includes the journal, config, policy, fork registry, race
metadata, and workspace identity that the sync remote intentionally does not
store.

For a remote-only backup, the current CLI does not fully rebuild `.asp/` from
the remote namespace yet. The steps below recover checkpointed source bytes with
stock git and give operators a deterministic escalation path.

## What The Remote Contains

`asp sync push` writes one namespace per workspace:

```text
asp-sync/v1/workspaces/<workspace-id>/
```

Inside that namespace:

| Remote key | Recovery role |
| --- | --- |
| `workspace.json` | Identifies the workspace id and format version. |
| `refs/checkpoints/<seq>.json` | Maps a checkpoint sequence to a git commit object id. |
| `refs/meta/<seq>.json` | Maps a checkpoint sequence to metadata about large-file sidecars. |
| `refs/head.json` | Names the latest pushed checkpoint sequence. |
| `objects/git/sha1/<fanout>/<tail>` | Loose git object bytes for checkpoint commits, trees, and blobs. |
| `objects/blobs/blake3/<hash>` | Large-file sidecar bytes addressed by BLAKE3. |

The remote does not contain `.asp/journal.jsonl`, `.asp/config.toml`,
`.asp/policy.toml`, `.asp/forks.json`, active fork directories, diagnostics
bundles, race metadata, ignored files, or the user's `.git/` history.

## Preserve Evidence First

Do not work in the damaged workspace. Copy the remote and recover into a fresh
directory:

```bash
mkdir -p /tmp/asp-sync-recovery
rsync -a /path/to/asp-remote/ /tmp/asp-sync-recovery/remote-copy/
cd /tmp/asp-sync-recovery
```

Find the workspace namespace and inspect the descriptor:

```bash
find remote-copy/asp-sync/v1/workspaces -mindepth 1 -maxdepth 1 -type d
NS=remote-copy/asp-sync/v1/workspaces/<workspace-id>
cat "$NS/workspace.json"
```

Keep the original remote read-only until the recovery is verified.

## Rebuild A Shadow Git Repository

Create an empty bare repository and copy the remote loose objects into the
standard git object fanout layout:

```bash
git init --bare recovered-shadow.git
rsync -a "$NS/objects/git/sha1/" recovered-shadow.git/objects/
git --git-dir recovered-shadow.git fsck --full
```

Recreate checkpoint refs from the remote ref JSON files. Each JSON file contains
a `seq` and `target`; `target` is the git object id to place under the matching
`refs/asp/*` ref.

```bash
for ref in "$NS"/refs/checkpoints/*.json; do
  seq=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["seq"])' "$ref")
  target=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["target"])' "$ref")
  git --git-dir recovered-shadow.git update-ref "refs/asp/checkpoints/$seq" "$target"
done

if [ -d "$NS/refs/meta" ]; then
  for ref in "$NS"/refs/meta/*.json; do
    [ -e "$ref" ] || continue
    seq=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["seq"])' "$ref")
    target=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["target"])' "$ref")
    git --git-dir recovered-shadow.git update-ref "refs/asp/meta/$seq" "$target"
  done
fi
```

Check that the refs resolve:

```bash
git --git-dir recovered-shadow.git show-ref refs/asp/checkpoints
git --git-dir recovered-shadow.git log --oneline --all --decorate
```

## Extract Checkpointed Source

Pick the checkpoint sequence you want to recover. To create a tarball:

```bash
git --git-dir recovered-shadow.git archive \
  --format=tar \
  --output recovered-checkpoint.tar \
  refs/asp/checkpoints/<seq>
```

Or extract into a directory:

```bash
mkdir recovered-worktree
rm -f recovered.index
GIT_DIR="$PWD/recovered-shadow.git" \
GIT_WORK_TREE="$PWD/recovered-worktree" \
GIT_INDEX_FILE="$PWD/recovered.index" \
git read-tree refs/asp/checkpoints/<seq>
GIT_DIR="$PWD/recovered-shadow.git" \
GIT_WORK_TREE="$PWD/recovered-worktree" \
GIT_INDEX_FILE="$PWD/recovered.index" \
git checkout-index -a -f
```

Review the recovered files before copying anything into a production checkout.

## Recover Large-File Sidecars

Large files are represented in checkpoint git history by small pointer JSON
files. If a recovered file looks like this, it is a sidecar pointer:

```json
{"asp_ptr":1,"blake3":"<hash>","size":123456789,"mode":"100644"}
```

Recover those large-file sidecars from:

```text
$NS/objects/blobs/blake3/<hash>
```

Verify the hash, then copy the sidecar bytes over the pointer file path in the
recovered worktree:

```bash
python3 -c 'import pathlib,sys; import blake3; p=pathlib.Path(sys.argv[1]); print(blake3.blake3(p.read_bytes()).hexdigest())' \
  "$NS/objects/blobs/blake3/<hash>"
cp "$NS/objects/blobs/blake3/<hash>" recovered-worktree/path/from/pointer
```

If the optional Python `blake3` module is unavailable, verify with another
trusted BLAKE3 tool before trusting the sidecar. Do not guess: the hash is the
content address.

## When A Partial `.asp/` Restore Exists

If you restored a `.asp/` directory from backup but suspect it missed recent
sync data, work in a copy of the repository and run:

```bash
asp doctor --deep
asp sync status --remote /path/to/asp-remote
asp sync fetch --remote /path/to/asp-remote
asp doctor --deep
```

`asp sync fetch` requires the local `.asp/workspace.json` id to match the remote
`workspace.json`. If it reports a workspace-id mismatch, stop and use the
remote-only stock-git recovery flow above or restore the correct `.asp/` backup.

## Verification Checklist

- The recovery happened in a fresh directory, not the damaged workspace.
- `workspace.json` names the expected workspace id.
- `git --git-dir recovered-shadow.git fsck --full` passes.
- `git --git-dir recovered-shadow.git show-ref refs/asp/checkpoints` lists the
  expected checkpoint sequences.
- A selected checkpoint extracts with stock git.
- Every large-file pointer has a matching `objects/blobs/blake3/<hash>` sidecar
  and the sidecar hash verifies.
- Operators understand that journal provenance, policy, config, forks, and race
  metadata require a full `.asp/` backup.

## Future Automation

A first-class `asp sync restore` command should automate this runbook by
creating a matching local store, importing remote objects and refs, verifying
CAS blobs, and making the absence of journal/config/policy data explicit. Until
that exists, remote-only backup recovery is intentionally a manual, auditable
stock-git procedure.
