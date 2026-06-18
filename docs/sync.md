# Sync

`asp sync` is an explicit, opt-in backup/sync surface for user-owned storage.
The first implementation supports a local filesystem remote so teams can test
the protocol with a mounted drive, shared volume, or fixture directory before
object-storage backends exist.

```bash
asp sync push --remote /path/to/asp-remote
asp sync push --json --remote /path/to/asp-remote
```

## What Gets Pushed

`asp sync push` writes a versioned namespace under the remote:

```text
asp-sync/v1/workspaces/<workspace-id>/
```

It uploads:

- checkpoint commits, trees, and blobs from `.asp/shadow.git` that are available
  as loose objects;
- large-file sidecar blobs from `.asp/blobs/` referenced by checkpoint
  manifests;
- tiny JSON refs for `refs/asp/checkpoints/*`, `refs/asp/meta/*`, and
  `refs/asp/head`;
- a `workspace.json` descriptor with the workspace id and format version.

It does not upload diagnostics bundles, race logs, fork directories, telemetry,
user Git history, or files outside the checkpoint graph.

## Safety Model

Objects and CAS blobs are immutable: if a remote key already exists with
different bytes, sync stops with a corrective error. Checkpoint refs are
append-only: an existing checkpoint sequence cannot be overwritten with another
target. The remote head ref is mutable and uses conditional writes so concurrent
pushes do not silently clobber newer state.

Before uploading a CAS blob, `asp` re-hashes it and refuses to sync if the local
bytes do not match the BLAKE3 content address. Run:

```bash
asp doctor --deep
```

if sync reports a corrupt or missing local object.

## JSON Output

The JSON result is `#/$defs/syncPushReport` from
[docs/schemas.md](schemas.md). Counts are split into uploaded, already-present,
created, unchanged, and updated buckets so automation can distinguish a first
backup from an idempotent retry.

## Current Limits

This first command is push-only. It is enough to create an auditable remote
backup, but it is not yet a full multi-device workflow. `asp sync fetch` is the
next milestone and will import missing refs conservatively without overwriting
newer local state.
