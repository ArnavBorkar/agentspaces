# @agentspaces/asp

This npm package is a thin `npx` wrapper for the native `asp` binary. On first
run it downloads the matching agentspaces release archive for your platform,
downloads the `.sha256` sidecar, verifies the archive, caches the binary under
`~/.cache/agentspaces/asp`, and then execs `asp`.

Supported native targets:

- macOS arm64: `aarch64-apple-darwin`
- macOS Intel: `x86_64-apple-darwin`
- Linux arm64: `aarch64-unknown-linux-musl`
- Linux x86_64: `x86_64-unknown-linux-musl`

```bash
npx @agentspaces/asp --version
npx @agentspaces/asp init
```

The wrapper has no runtime npm dependencies. For the full project README,
trust model, and release verification docs, see
<https://github.com/ArnavBorkar/agentspaces>.
