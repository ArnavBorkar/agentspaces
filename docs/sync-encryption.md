# Sync Client-Side Encryption Design

This document defines the target design for encrypting remote sync objects
before they leave the workstation. The local `.asp/` store stays unchanged and
recoverable with stock git; encryption applies only to object-storage remotes.

## Goals

- Remote providers never see checkpoint object bytes, CAS blob bytes, or ref
  JSON plaintext.
- Conditional writes still work for refs, so concurrent pushes cannot silently
  clobber each other.
- Every encrypted object remains self-describing enough for diagnostics without
  exposing source content.
- Recovery is possible from a remote backup plus the sync key material, without
  a hosted service.
- Encryption can be introduced as an opt-in remote format version without
  breaking existing local remotes.

## Non-Goals

- Encrypting the local `.asp/` directory.
- Hiding the fact that a workspace synced at a given time.
- Making object storage the source of truth for local checkpoint history.
- Depending on a managed control plane to unwrap keys.

## Threat Model

Client-side encryption protects against:

- accidental public bucket/container exposure;
- provider operator access to object bytes;
- support bundles or storage logs that include object payloads;
- cross-team bucket reuse where IAM boundaries fail open.

It does not fully hide:

- approximate object counts and object sizes unless padding is enabled;
- write timing and access patterns;
- the organization account that owns the bucket/container;
- data from a compromised local machine that has the sync key loaded.

## Format Overview

Encrypted remotes use a new namespace:

```text
asp-sync/v2/encrypted/workspaces/<workspace-id>/
```

The namespace contains one public descriptor:

```json
{
  "v": 1,
  "workspace_id": "ws_...",
  "format": "asp-sync-encrypted-v1",
  "key_id": "k2026q2",
  "key_derivation": "hkdf-sha256",
  "content_cipher": "xchacha20-poly1305",
  "name_mac": "hmac-sha256",
  "padding": "none"
}
```

The descriptor contains algorithms and key ids only. It never stores raw keys,
wrapped keys, recovery passphrases, or account credentials.

## Key Hierarchy

The operator provides a sync root key through a local secret source such as an
environment variable, OS keychain, hardware-backed secret, or enterprise secret
broker. `asp` derives scoped keys locally:

```text
root sync key
  -> workspace encryption key
  -> object content encryption key
  -> object name MAC key
  -> ref content encryption key
```

Derivation context includes:

- `workspace_id`;
- remote namespace version;
- key id;
- purpose string such as `content`, `name`, or `ref`;
- provider-independent logical key.

The same root key can back multiple workspaces only if derivation includes the
workspace id. Operators should still prefer one key per high-risk environment.

## Object Names

Plain object keys leak object type and content addresses. Encrypted remotes
therefore store objects by an HMAC of the logical sync key:

```text
objects/<first-2-hex>/<hmac-sha256(logical-key)>
refs/<first-2-hex>/<hmac-sha256(logical-ref-key)>
```

The encrypted payload header stores the logical key so `asp` can reconstruct
reports after decryption. Remote list operations reveal only opaque names and
sizes.

## Object Envelope

Every encrypted object is one binary envelope:

```text
magic:        ASPENC1
key_id:       length-prefixed UTF-8
nonce:        random 192-bit nonce
aad_hash:     SHA-256 of associated data
ciphertext:   AEAD(plaintext)
```

Associated data includes:

- remote namespace version;
- workspace id;
- logical key;
- object kind: `git-object`, `cas-blob`, `checkpoint-ref`, `meta-ref`, or
  `head-ref`;
- expected plaintext digest when available.

Decryption fails if the object is moved to another logical key, workspace, or
kind.

## Immutable Objects

For checkpoint git objects and CAS blobs:

1. Verify the local plaintext object first.
2. Encrypt the object envelope with a fresh nonce.
3. Write to the HMAC-derived remote key with create-only semantics.
4. If the remote key already exists, fetch and decrypt it.
5. Treat matching plaintext as already present.
6. Treat different plaintext as remote corruption.

The remote provider sees only ciphertext. `asp` verifies plaintext hashes after
decryption before importing objects locally.

## Mutable Refs

Refs remain compare-and-swap writes:

1. Read and decrypt the current encrypted ref, if present.
2. Compare the plaintext ref fields to the intended transition.
3. Encrypt the new ref with a fresh nonce.
4. Use the provider remote version from the read response as the conditional
   write precondition.

The remote version remains provider-native: S3 ETag, GCS generation, Azure ETag,
or local remote version. It authenticates the ciphertext object, while the AEAD
authenticates the plaintext ref contents.

## Rotation

Key rotation creates a new key id. New writes use the new key immediately. Reads
try the key id named in the object envelope and fail with an actionable error if
the operator has not provided that key.

Re-encryption is an explicit maintenance operation:

```text
asp sync reencrypt --from-key k2026q2 --to-key k2026q3 --dry-run
```

The operation rewrites encrypted remote objects under new opaque names only
after decrypting and verifying plaintext. It must not delete old objects unless
the operator asks for a cleanup plan.

## Recovery

Remote-only recovery requires:

- the bucket/container and prefix;
- provider credentials with read/list access;
- the sync root key or required historical key ids;
- the `asp` binary matching the encrypted remote format.

If key material is missing, the recovery command must fail before writing local
state and print the missing key id. A hosted service must not be required to
unwrap keys.

## Rollout Sequence

1. Add encrypted remote envelope types and test vectors.
2. Add a `SyncRemote` decorator that encrypts plaintext before calling an inner
   remote.
3. Add local-file remote tests that prove encrypted push/fetch round trips.
4. Add emulator fixture coverage for S3-compatible, GCS, and Azure Blob remotes.
5. Add CLI credential UX only after key-source and recovery errors are
   documented.

## Review Checklist

- Does every decrypt authenticate workspace id, object kind, and logical key?
- Are nonces random and never reused with the same derived content key?
- Does `Debug` output redact key material and derived secrets?
- Can refs still detect remote races through provider versions?
- Can operators rotate keys without deleting old backups?
- Does remote-only restore fail safely when a key id is missing?
