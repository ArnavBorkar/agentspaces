# Evidence Packets

`asp evidence collect` creates a local JSON packet for security review, support
handoffs, and incident timelines. It does not upload anything.

```bash
asp evidence collect
asp --json evidence collect
asp evidence collect --output asp-evidence.json
```

The packet includes:

- a redacted diagnostics bundle;
- a preflight readiness summary and check runbook links;
- the installed schema inventory;
- recent audit event timing and operation metadata.

By default, local paths are redacted and audit event `message` and `detail`
payloads are omitted. Secrets remain redacted even when `--include-paths` is
used.

```bash
asp evidence collect --audit-limit 50 --output asp-evidence.json
```

Use `--deep` when the packet should include deep preflight doctor checks such
as CAS verification. Use `--include-paths` only for a private support channel
where full local paths are acceptable.

## Signed Manifest

When an evidence packet leaves a workstation or CI runner, attach a small
manifest and sign that manifest with the same external tooling used for release
verification.

```bash
asp evidence collect --output asp-evidence.json
asp evidence manifest \
  --packet asp-evidence.json \
  --output asp-evidence.manifest.json
asp evidence verify \
  --packet asp-evidence.json \
  --manifest asp-evidence.manifest.json
```

The manifest records the packet file name, byte length, SHA-256 digest,
creation time, and `created_by: "asp evidence manifest"`. It does not include
the full local path to the packet. Verification recomputes the packet digest
and exits nonzero when the artifact name, byte length, or SHA-256 digest does
not match.

Sign the manifest, not the packet. Reviewers should verify in this order:

1. verify the manifest signature;
2. run `asp evidence verify` to bind the manifest back to the packet bytes;
3. open the evidence packet only after both checks pass.

If the packet changes after signing, regenerate the manifest and signature.
Appending a note, reformatting JSON, or changing redaction settings changes the
packet bytes and invalidates the old manifest.

## Sigstore Keyless Signing

Sign with Sigstore when the environment already uses keyless signing:

```bash
cosign sign-blob \
  --yes \
  --bundle asp-evidence.manifest.sigstore.json \
  asp-evidence.manifest.json
```

Verify later with the expected identity and issuer for the signer:

```bash
cosign verify-blob \
  --bundle asp-evidence.manifest.sigstore.json \
  --certificate-identity-regexp "<expected signer identity>" \
  --certificate-oidc-issuer "<expected OIDC issuer>" \
  asp-evidence.manifest.json
```

For GitHub Actions, the issuer is normally
`https://token.actions.githubusercontent.com`, and the identity should identify
the workflow that produced the packet:

```bash
cosign verify-blob \
  --bundle asp-evidence.manifest.sigstore.json \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  --certificate-identity-regexp "https://github.com/<org>/<repo>/.github/workflows/<workflow>.yml@refs/.*" \
  asp-evidence.manifest.json
```

For workstation signing, record the human or service identity shown by
`cosign sign-blob` in the ticket. Do not accept "some valid Sigstore signature"
as sufficient evidence; the identity and issuer must match the channel where
the packet was produced.

## Offline Minisign Signing

Use minisign when the signer cannot use online OIDC identity or the support
process requires an offline public key.

```bash
minisign -G -p asp-evidence.pub -s asp-evidence.sec
minisign -S \
  -s asp-evidence.sec \
  -m asp-evidence.manifest.json \
  -x asp-evidence.manifest.minisig
minisign -Vm asp-evidence.manifest.json \
  -x asp-evidence.manifest.minisig \
  -p asp-evidence.pub
```

Publish `asp-evidence.pub` through the same trusted channel used for release or
incident keys, not inside the same ticket as the first signature. Rotate the key
when access changes, and keep the old public key available for the retention
period of tickets it signed.

Keep the packet, `asp-evidence.manifest.json`, and the signature or Sigstore
bundle together. The signature proves the manifest; the manifest binds the
reviewed packet bytes through `sha256`.

For copyable public, private incident, security, and CI ticket formats, use the
[support ticket templates](support-ticket-templates.md). Each template calls out
the expected redaction statement, manifest verification command, and signature
artifact before any richer support escalation.

## Review Checklist

Before sharing:

- confirm `redaction.paths_redacted` and `redaction.secrets_redacted` match the
  channel where the packet will be sent;
- keep `audit_messages_included` and `audit_details_included` false unless a
  private incident channel explicitly asks for richer context;
- run `asp preflight` and include SARIF artifacts when the reviewer needs CI
  annotations as well as the packet;
- do not attach fork directories, source archives, or `.asp/` backups unless a
  separate incident plan requires them.

During review:

- start with `preflight.ready` and the per-check runbooks;
- inspect `diagnostics.doctor_findings` for repairable workspace issues;
- compare `schema.asp_version` and schema paths with the installed binary under
  review;
- use `recent_audit_events` to establish operation order, timing, and checkpoint
  sequence without exposing free-form journal messages;
- ask for a fresh packet with `--deep` when CAS integrity or store health is in
  question.

Escalate only when the redacted packet cannot answer the question. If full paths
are necessary, rerun with `--include-paths` and share through a private support
channel with an explicit retention expectation.
