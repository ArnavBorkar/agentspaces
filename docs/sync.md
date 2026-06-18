# Sync

`asp sync` is an explicit, opt-in backup/sync surface for user-owned storage.
The first implementation supports a local filesystem remote so teams can test
the protocol with a mounted drive, shared volume, or fixture directory before
turning on cloud storage.

```bash
asp sync push --remote /path/to/asp-remote
asp sync fetch --remote /path/to/asp-remote
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

The JSON results are `#/$defs/syncPushReport` and `#/$defs/syncFetchReport`
from [docs/schemas.md](schemas.md). Counts are split into uploaded/downloaded,
already-present, created/imported, unchanged, updated, and conflicted buckets
so automation can distinguish a first backup, an idempotent retry, and a
manual reconciliation case.

## Fetch Behavior

`asp sync fetch` imports missing checkpoint and meta refs only after remote git
objects and CAS blobs verify locally. If a remote checkpoint sequence already
exists locally with another target, fetch reports a conflict and leaves local
refs untouched. The local head moves only when the remote head is newer in
checkpoint-sequence terms or when the local head is missing.

## Current Limits

The `asp` CLI sync commands support local filesystem remotes only. They are
enough to create and restore an auditable remote backup, but they are not yet a
full multi-device reconciliation workflow.

`asp-core` also includes an S3-compatible adapter behind the `SyncRemote` trait
for integrators that are ready to wire their own credential loading and HTTP
transport. The adapter signs AWS SigV4 requests, scopes keys under an optional
prefix, maps remote versions to S3 ETags, uses `If-None-Match: *` for immutable
objects, uses `If-Match` for compare-and-swap ref writes, and parses paginated
`ListObjectsV2` responses. The CLI will stay local-only until the credential,
policy, and recovery UX is explicit enough for operators to audit.
