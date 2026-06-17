# Sync Protocol Design (v0)

This document defines the first BYO-bucket sync protocol for `.asp/` stores.
It is a design contract for the upcoming local filesystem remote and later
object-storage remotes such as S3, R2, GCS, and Azure Blob.

The local workspace remains the source of truth. Sync is opt-in, explicit, and
recoverable without a hosted service.

## Goals

- Mirror checkpoint history to user-owned storage without custody by default.
- Preserve the existing recovery story: checkpoint commits remain ordinary git
  objects and large files remain files named by BLAKE3 hash.
- Make uploads resumable and idempotent after interruption.
- Use immutable writes for content and conditional writes for mutable refs.
- Detect conflicts without overwriting newer local state.
- Keep the protocol implementable by a simple local filesystem remote first.

## Non-Goals

- No automatic background upload.
- No hosted account requirement.
- No remote deletion, garbage collection, or retention enforcement in the first
  implementation.
- No fork-directory sync. Forks can be recreated from local workspaces and are
  intentionally whole-tree, secret-bearing physical copies.
- No diagnostics, race logs, or telemetry upload unless a later design adds a
  separate explicit opt-in.
- No encryption layer in the first protocol. Users should rely on bucket,
  endpoint, or backup encryption until a dedicated envelope-encryption design
  exists.

## Data Model

The protocol has three object classes.

| Class | Local Source | Remote Semantics | Integrity |
| --- | --- | --- | --- |
| Git objects | `.asp/shadow.git/objects` or `git cat-file` | Immutable by object id | Git object id |
| CAS blobs | `.asp/blobs/<blake3>` | Immutable by BLAKE3 hash | BLAKE3 |
| Refs | `refs/asp/checkpoints/*`, `refs/asp/meta/*`, `refs/asp/head` | Conditional tiny JSON files | JSON schema plus target existence |

Journal sync is intentionally not in the first command. The journal is an audit
log with ordering and redaction questions that deserve a separate design. The
first sync milestone backs up the checkpoint graph and large-file sidecar so a
workspace can recover source state.

## Remote Layout

All keys live under a caller-provided remote root:

```text
asp-sync/v1/
  workspaces/
    <workspace-id>/
      workspace.json
      objects/
        git/
          sha1/
            <first-two-hex>/<remaining-hex>
        blobs/
          blake3/
            <blake3-hex>
      refs/
        checkpoints/
          <seq>.json
        meta/
          <seq>.json
        head.json
      manifests/
        push-<timestamp>-<nonce>.json
```

`workspace.json` is immutable once created for a workspace id:

```json
{
  "v": 1,
  "workspace_id": "01HX...",
  "format_version": 1,
  "created_by": "asp",
  "created_at": "2026-06-17T23:00:00Z"
}
```

The workspace id partitions remote state. A future team remote can list multiple
workspace ids, but one workspace never writes into another workspace's prefix.

## Immutable Objects

### Git Objects

Checkpoint commits, trees, blobs, and meta manifests are uploaded as canonical
loose git object bytes under:

```text
objects/git/sha1/<first-two-hex>/<remaining-hex>
```

The key is the object id. The remote write must be create-if-absent:

- if the object does not exist, upload it;
- if it exists and verifies to the same object id, treat it as success;
- if it exists but verifies to different bytes, fail with `sync_corrupt_remote`.

An implementation may read local loose object files directly when present. If a
local shadow repo later packs objects, the implementation should read the object
through git and write canonical loose bytes to the remote. The recovery invariant
is that downloading these keys into `.asp/shadow.git/objects` produces a usable
stock-git object database.

### CAS Blobs

Large-file sidecar blobs are uploaded under:

```text
objects/blobs/blake3/<blake3-hex>
```

The filename is the content address. Before upload, asp must re-hash the local
file and refuse to upload if the bytes do not match the filename. On download,
asp must re-hash the remote bytes before writing them into `.asp/blobs/`.

CAS upload is also create-if-absent:

- absent: upload bytes with a conditional create;
- present and hash matches: success;
- present and hash differs: fail, never overwrite.

## Ref Files

Refs are tiny JSON files. They are the only mutable logical state in the first
protocol, and every write must be conditional.

Checkpoint refs are append-only by sequence:

```json
{
  "v": 1,
  "name": "refs/asp/checkpoints/42",
  "seq": 42,
  "target": "<commit-sha1>",
  "workspace_id": "01HX...",
  "updated_at": "2026-06-17T23:00:00Z",
  "writer": "hostname-or-agent-label"
}
```

Meta refs use the same shape with `name: "refs/asp/meta/<seq>"` and the meta
commit target.

`head.json` points at the latest checkpoint observed by that writer:

```json
{
  "v": 1,
  "name": "refs/asp/head",
  "seq": 42,
  "target": "<commit-sha1>",
  "workspace_id": "01HX...",
  "updated_at": "2026-06-17T23:00:00Z",
  "writer": "hostname-or-agent-label",
  "previous_target": "<old-head-sha1-or-null>"
}
```

`head.json` is advisory. It helps dashboards and fetch planning, but recovery
must rely on the checkpoint refs. Fetch must never replace a newer local
`refs/asp/head` only because the remote head is different.

## Push Ordering

`asp sync push` should operate on a locked local snapshot:

1. Take the workspace lock.
2. Read the local checkpoint refs, meta refs, and current head.
3. Compute the git object closure for selected checkpoint and meta refs.
4. Compute the CAS blobs referenced by selected meta manifests.
5. Release the lock after the immutable upload plan is complete.
6. Upload git objects with create-if-absent.
7. Upload CAS blobs with create-if-absent.
8. Conditionally create missing checkpoint and meta ref JSON files.
9. Conditionally update `head.json` only if the remote still matches the
   expected previous value or is absent.
10. Write an immutable push manifest summarizing what happened.

Refs are written after objects. A remote checkpoint ref must never point at an
object that has not already been uploaded and verified.

If the process dies before refs are written, the remote may contain extra
immutable objects. That is safe. A later push can reuse them.

## Fetch Ordering

`asp sync fetch` should be conservative:

1. List remote checkpoint and meta refs for the workspace id.
2. Compare each remote ref with local refs.
3. Download and verify all git objects needed for refs that can be imported.
4. Download and verify referenced CAS blobs.
5. Write missing local refs with `git update-ref`.
6. Leave conflicting local refs untouched and report them in JSON.
7. Repoint local `refs/asp/head` only when doing so is a fast-forward in local
   sequence terms, or when the local head is missing.

Fetch must not delete local objects, CAS blobs, refs, journal entries, forks, or
policy/config files.

## Conflict Handling

The first protocol treats conflicts as reportable states, not automatic merges.

| Situation | Result |
| --- | --- |
| Remote ref missing | Push may create it conditionally. |
| Remote ref exists with same target | Idempotent success. |
| Remote checkpoint `<seq>` exists with different target | Push stops with `sync_conflict`; no overwrite. |
| Local checkpoint `<seq>` exists with different remote target | Fetch reports conflict; local ref is untouched. |
| Remote head differs but checkpoint refs are compatible | Import missing refs; update local head only if it does not move backward. |
| Object key exists with wrong bytes | Fail with `sync_corrupt_remote`. |
| CAS key exists with wrong bytes | Fail with `sync_corrupt_remote`. |

The JSON conflict shape should include:

```json
{
  "kind": "checkpoint_ref",
  "seq": 42,
  "local": "<local-target-or-null>",
  "remote": "<remote-target-or-null>",
  "hint": "fetch into a clean workspace or create a new checkpoint after reviewing both histories"
}
```

A later reconciliation command can import conflicting remote commits under
`refs/asp/remotes/<remote-name>/checkpoints/<seq>-<short-sha>` or renumber them
after user approval. The first implementation should not silently renumber or
overwrite anything.

## Conditional Write Semantics

The remote trait should expose the minimum capabilities needed by both local
filesystem tests and object-storage backends:

```rust
trait SyncRemote {
    fn get(&self, key: &str) -> Result<Option<RemoteObject>>;
    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>>;
    fn put_immutable(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome>;
    fn put_if_match(
        &self,
        key: &str,
        bytes: &[u8],
        expected: Option<RemoteVersion>,
    ) -> Result<PutOutcome>;
}
```

`put_immutable` maps to `If-None-Match: *` for S3-like stores and to
write-temp-plus-atomic-rename for the local filesystem remote. If a key already
exists, the caller verifies the bytes and treats a match as success.

`put_if_match` is compare-and-swap for refs. It takes the version returned by a
prior `get`; `None` means create only if absent. S3 ETags, generation numbers,
or local sidecar version files can back `RemoteVersion`.

## Push Manifest

Each push writes an immutable manifest under `manifests/` after refs are
attempted. It is audit metadata, not recovery authority.

```json
{
  "v": 1,
  "workspace_id": "01HX...",
  "started_at": "2026-06-17T23:00:00Z",
  "finished_at": "2026-06-17T23:00:12Z",
  "git_objects_uploaded": 120,
  "cas_blobs_uploaded": 3,
  "refs_created": 4,
  "refs_conflicted": 0,
  "writer": "hostname-or-agent-label"
}
```

If the manifest is missing because the process died, recovery still works from
objects and refs.

## Security And Privacy

- Sync commands must be explicit: no install, init, checkpoint, restore, fork,
  doctor, race, or MCP operation starts sync by itself.
- The first protocol uploads checkpointed source objects and CAS blobs only.
  That can include proprietary code and non-gitignored secrets, so remotes must
  be documented as source-code backups.
- Credentials are provided by the user or organization and never written into
  `.asp/` unless a later credential-store design says exactly how.
- Remote paths are derived from object ids, BLAKE3 hashes, workspace ids, and
  ref names. No arbitrary local path from the store becomes a remote key.
- Fetch validates object and blob hashes before writing local files.
- Fetch writes only inside `.asp/shadow.git/objects`, `.asp/blobs/`, and
  `refs/asp/*` through git. It never writes the user's `.git`.

## Recovery Story

A remote backup is useful even without `asp sync fetch`:

1. Copy `objects/git/sha1/*` into `.asp/shadow.git/objects/`.
2. Copy `objects/blobs/blake3/*` into `.asp/blobs/`.
3. Recreate checkpoint refs with stock git:

```bash
GIT_DIR=.asp/shadow.git git update-ref refs/asp/checkpoints/42 <commit-sha1>
GIT_DIR=.asp/shadow.git git update-ref refs/asp/meta/42 <meta-sha1>
```

4. Run `asp doctor --deep`.
5. Use the stock-git restore runbook from `docs/design/format.md`.

The remote protocol therefore mirrors the local trust model instead of replacing
it with a private service.

## Implementation Plan

1. Add a local filesystem remote that implements create-if-absent and
   compare-and-swap with temp files and atomic rename.
2. Add the remote trait and conditional-write tests.
3. Add `asp sync push` for local remotes, with JSON counts and conflicts.
4. Add `asp sync fetch` that imports missing refs and reports conflicts without
   overwriting local refs.
5. Add object-storage backends only after the local protocol has deterministic
   integration tests and recovery drills.
