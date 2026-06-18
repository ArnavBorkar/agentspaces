# Homebrew formula

The tap-ready formula lives at
`packaging/homebrew/Formula/asp.rb`. It installs the checksum-verified release
archive for the current Homebrew OS/CPU combination:

- macOS arm64: `aarch64-apple-darwin`
- macOS Intel: `x86_64-apple-darwin`
- Linux arm64: `aarch64-unknown-linux-musl`
- Linux x86_64: `x86_64-unknown-linux-musl`

## Local validation

```bash
ruby -c packaging/homebrew/Formula/asp.rb
cargo test -p asp --test package_metadata
```

The cargo test checks that the formula version matches the crate version and
that all supported release assets have SHA-256 entries.

## Publishing to a tap

Copy `packaging/homebrew/Formula/asp.rb` into the tap repository as
`Formula/asp.rb`, then run:

```bash
brew audit --strict --online asp
brew test asp
```

For a new release, update the formula version, release URLs, and SHA-256 values
from the `.sha256` files attached to the GitHub Release. Keep the install script
as the no-sudo checksum-verified path for users who do not use Homebrew.
