# Sync Emulator Fixtures

`asp-core` ships ignored integration fixtures for exercising object-storage
sync remotes against local emulators. They are not part of normal CI because
they require provider-specific services, buckets, containers, and credentials.

Run all configured fixtures:

```bash
scripts/sync-emulators.sh
```

Each fixture skips when its endpoint variable is missing, so a developer can run
only MinIO, only fake-gcs-server, only Azurite, or any combination.

## S3-Compatible

Use MinIO or another S3-compatible emulator with a bucket that already exists:

```bash
export ASP_SYNC_S3_ENDPOINT=http://127.0.0.1:9000
export ASP_SYNC_S3_BUCKET=agentspaces-sync
export ASP_SYNC_S3_REGION=us-east-1
export ASP_SYNC_S3_ACCESS_KEY_ID=minioadmin
export ASP_SYNC_S3_SECRET_ACCESS_KEY=minioadmin

scripts/sync-emulators.sh
```

Optional:

```bash
export ASP_SYNC_S3_SESSION_TOKEN=...
```

## Google Cloud Storage

Use fake-gcs-server or a local GCS-compatible endpoint with a bucket that
already exists:

```bash
export ASP_SYNC_GCS_ENDPOINT=http://127.0.0.1:4443
export ASP_SYNC_GCS_BUCKET=agentspaces-sync
export ASP_SYNC_GCS_BEARER_TOKEN=test-token

scripts/sync-emulators.sh
```

Many local GCS emulators ignore the bearer token. Keep the variable anyway so
the fixture exercises the same request shape used by production integrations.

## Azure Blob

Use Azurite or a local Azure Blob-compatible endpoint with a container that
already exists and a SAS token granting read, list, create, and write:

```bash
export ASP_SYNC_AZURE_ENDPOINT=http://127.0.0.1:10000/devstoreaccount1
export ASP_SYNC_AZURE_CONTAINER=agentspaces-sync
export ASP_SYNC_AZURE_SAS='sv=...&sp=rlcw&sig=...'

scripts/sync-emulators.sh
```

If an emulator exposes the account name as part of the path instead of the
origin, put that full origin in `ASP_SYNC_AZURE_ENDPOINT`. The adapter appends
`/<container>/<blob>` after the endpoint origin.

## Contract

Every provider fixture performs the same contract:

- create an immutable object and retry the same bytes idempotently;
- read the object and verify bytes;
- list the object through the provider's paginated list API;
- create a ref with a conditional create;
- replace the ref only when the prior remote version matches.

The fixtures intentionally do not delete remote objects. Use a fresh bucket,
container, or lifecycle rule for cleanup.
