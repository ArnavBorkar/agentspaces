# Crates.io publishing

Use this checklist when publishing the Rust crates. The package dry run is
credential-free and runs in CI; the final publish steps require a crates.io
owner token.

## Preflight

```bash
cargo package --workspace --locked
```

This validates package metadata, crate-local READMEs, included files, and that
the packaged crates build from the temporary registry Cargo creates for the
workspace.

## Publish order

Publish the library first, then wait for crates.io indexing before publishing
the binary crate:

```bash
cargo publish -p asp-core --locked
cargo info asp-core --registry crates-io
cargo publish -p asp --locked
```

`asp` depends on `asp-core` by version, so publishing `asp` before `asp-core` is
indexed will fail dependency resolution. Do not use `--allow-dirty` for a real
publish.

## After publish

- Confirm `cargo install asp` installs the expected version.
- Add the crates.io links to the release notes.
- Keep the install script as the preferred checksum-verified binary path for
  users who do not want to compile from source.
