# Sync Credential Scopes

`asp` sync credentials must be scoped as if the token will be copied into an
agent prompt by mistake: one storage namespace, one prefix, no deletes, no
bucket administration, short lifetime where the provider supports it.

The default CLI remains local-only. These rules apply to object-storage
integrations built on `asp-core` remotes and to future CLI credential UX.

## Required Scope

Use a dedicated bucket, container, or managed folder when possible. If the
organization must share a storage namespace, restrict access to:

```text
asp-sync/v1/workspaces/<workspace-id>/
```

The sync writer needs only these capabilities under that prefix:

- list objects below the prefix;
- read object bytes and remote versions;
- create new immutable object keys;
- replace tiny mutable ref objects only when the previous remote version
  matches.

Do not grant:

- delete, permanent delete, lifecycle, retention, or object-lock bypass;
- bucket or account policy changes;
- public access, ACL changes, ownership changes, or key-management admin;
- cross-bucket copy, replication, inventory, analytics, or billing access;
- credentials that can list unrelated buckets, containers, or projects.

Prefer workload identity, managed identity, user delegation, or another
short-lived credential flow over long-lived static keys. Rotate credentials on
every rollout phase, incident, or contractor offboarding event.

## S3-Compatible Storage

The S3 adapter signs SigV4 requests and uses:

- `ListObjectsV2` for prefix scans;
- `GetObject` for reads and ETag versions;
- `PutObject` with `If-None-Match: *` for immutable objects;
- `PutObject` with `If-Match: <etag>` for ref compare-and-swap.

Minimum AWS IAM actions:

- `s3:ListBucket` on the bucket ARN, constrained by `s3:prefix`;
- `s3:GetObject` on the object prefix ARN;
- `s3:PutObject` on the object prefix ARN.

Example identity policy:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "ListOnlyAspWorkspacePrefix",
      "Effect": "Allow",
      "Action": "s3:ListBucket",
      "Resource": "arn:aws:s3:::example-asp-sync",
      "Condition": {
        "StringLike": {
          "s3:prefix": [
            "asp-sync/v1/workspaces/ws_1234567890abcdef/*"
          ]
        }
      }
    },
    {
      "Sid": "ReadAndWriteOnlyAspWorkspaceObjects",
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:PutObject"
      ],
      "Resource": "arn:aws:s3:::example-asp-sync/asp-sync/v1/workspaces/ws_1234567890abcdef/*"
    }
  ]
}
```

S3-compatible providers use the same operation names inconsistently. Translate
the policy to the provider's IAM model, but keep the same shape: list the prefix,
read objects, put objects, and no delete/admin permissions.

Do not grant `s3:DeleteObject`, `s3:PutBucketPolicy`, `s3:PutBucketAcl`,
`s3:PutObjectAcl`, `s3:ListAllMyBuckets`, replication, lifecycle, or inventory
actions to a sync writer.

## Google Cloud Storage

A GCS adapter should use generation preconditions for the same create-only and
compare-and-swap behavior. The minimum custom role should include:

- `storage.objects.list`;
- `storage.objects.get`;
- `storage.objects.create`;
- `storage.objects.update`.

Use a bucket-level binding, managed folder binding, or IAM Condition to restrict
the principal to the workspace prefix. A condition should match the object
resource name prefix:

```text
resource.name.startsWith("projects/_/buckets/example-asp-sync/objects/asp-sync/v1/workspaces/ws_1234567890abcdef/")
```

Do not grant `storage.objects.delete`, legacy owner roles, project-wide Storage
Admin, HMAC key administration, bucket IAM administration, or bucket metadata
mutation to a sync writer.

## Azure Blob Storage

Prefer a user delegation SAS or managed identity over an account key. For a SAS,
scope it to a dedicated container when possible and use only:

```text
sp=rlcw
```

That grants read, list, create, and write. Omit delete permissions. Keep expiry
short and bind the SAS to a stored access policy when the operation needs
server-side revocation.

For Microsoft Entra ID, built-in roles are usually too broad for least
privilege: Storage Blob Data Contributor includes delete. Use a custom role or
role assignment condition scoped to the container or prefix-equivalent naming
scheme. The adapter should rely on blob ETags with `If-None-Match` and
`If-Match`, not on delete-and-recreate workflows.

Do not distribute storage account keys to agents. Do not grant account-level
Contributor, Owner, `listkeys`, delete, permanent delete, container management,
immutability-policy bypass, or public-access changes.

## Operational Checklist

Before enabling an object-storage sync integration:

- record the bucket/container, prefix, principal id, credential type, expiry,
  and rotation owner;
- prove the principal cannot read or list a sibling workspace prefix;
- prove delete fails for a synced object;
- prove policy and ACL mutation fails;
- keep provider access logs enabled for reads, writes, and denied operations;
- document the emergency revocation command next to the rollout owner.

## Provider References

- AWS: [Actions, resources, and condition keys for Amazon S3](https://docs.aws.amazon.com/service-authorization/latest/reference/list_amazons3.html)
- AWS: [Amazon S3 bucket policy examples](https://docs.aws.amazon.com/AmazonS3/latest/userguide/example-bucket-policies.html)
- Google Cloud: [Cloud Storage IAM permissions](https://docs.cloud.google.com/storage/docs/access-control/iam-permissions)
- Google Cloud: [Cloud Storage IAM roles](https://docs.cloud.google.com/storage/docs/access-control/iam-roles)
- Microsoft: [Create a service SAS](https://learn.microsoft.com/en-us/rest/api/storageservices/create-service-sas)
- Microsoft: [Authorize access to blobs with Microsoft Entra ID](https://learn.microsoft.com/en-us/azure/storage/blobs/authorize-access-azure-active-directory)
