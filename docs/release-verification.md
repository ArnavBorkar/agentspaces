# Verifying release artifacts

Agentspaces release archives are published with three files per target:

- `asp-<version>-<target>.tar.gz`
- `asp-<version>-<target>.tar.gz.sha256`
- `asp-<version>-<target>.tar.gz.sha256.sigstore.json`

The `.sigstore.json` file is a keyless Sigstore bundle for the checksum file.
Verify the bundle first, then use the signed checksum to verify the archive.

## Prerequisites

Install `cosign` from Sigstore and GitHub CLI:

```bash
cosign version
gh --version
```

## Verify a release

Set the archive name for your platform:

```bash
ASSET="asp-v0.1.1-aarch64-apple-darwin.tar.gz"
```

Verify the checksum file was signed by this repository's release workflow:

```bash
cosign verify-blob \
  --bundle "${ASSET}.sha256.sigstore.json" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  --certificate-identity-regexp "https://github.com/ArnavBorkar/agentspaces/.github/workflows/release.yml@refs/tags/v.*" \
  "${ASSET}.sha256"
```

Then verify the archive bytes:

```bash
shasum -a 256 -c "${ASSET}.sha256"
```

On Linux, `sha256sum -c "${ASSET}.sha256"` is also fine.

Finally, verify the GitHub provenance attestation for the archive:

```bash
gh attestation verify "${ASSET}" \
  -R ArnavBorkar/agentspaces \
  --signer-workflow "github.com/ArnavBorkar/agentspaces/.github/workflows/release.yml"
```

## What this proves

- the checksum file was signed by GitHub Actions running the tagged
  `release.yml` workflow for `ArnavBorkar/agentspaces`;
- Sigstore recorded the signing event in the transparency log;
- the downloaded archive matches the signed checksum.
- GitHub has a SLSA provenance attestation linking the archive to this
  repository's release workflow.

It does not prove that the source is bug-free or that your machine is trusted.
It does make tampering with release assets visible.
