# npm/npx wrapper

The npm wrapper source lives in `npm/asp` and publishes as `@agentspaces/asp`.
The unscoped `asp` package name is already taken on npm, so use the scoped name
unless the project intentionally changes package ownership.

## What it does

On first run, the wrapper:

1. maps the local OS/CPU to a release target;
2. downloads the matching GitHub Release archive and `.sha256` file;
3. verifies the archive SHA-256 digest;
4. extracts and caches the native `asp` binary;
5. execs the binary with the original arguments.

It supports macOS arm64/x86_64 and Linux arm64/x86_64 musl release artifacts.
It does not replace Sigstore release verification; use
`docs/release-verification.md` for manual signature/provenance checks.

## Local validation

```bash
npm test --prefix npm/asp
(cd npm/asp && npm pack --dry-run)
```

The tests fake downloads and extraction, so they never call GitHub.

## Publishing

Publish after the matching GitHub Release assets exist:

```bash
cd npm/asp
npm publish --access public
```

After publishing, smoke-test the package from a clean directory:

```bash
npx @agentspaces/asp --version
```

The package version should match the release tag and Rust crate version.
