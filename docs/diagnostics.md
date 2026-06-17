# Diagnostics Bundles

`asp diagnostics` emits a JSON bundle for bug reports, support handoffs, and
CI artifacts. It is designed to be attachable by default: full local paths are
redacted, file contents are never collected, and environment variables are never
collected.

```bash
asp diagnostics --output asp-diagnostics.json
```

The bundle includes:

- asp version and workspace format version;
- workspace health counts from `asp status`;
- local store counts from `asp stats`;
- shallow `asp doctor` findings, including cause and next-action text;
- fork registry status with redacted paths;
- the last few journal operations with timing fields.

The bundle does not include source files, checkpoint contents, shell history,
environment variables, git remotes, or raw config patterns. Checkpoint messages
and doctor finding text, causes, and next actions are scanned for common
token-shaped secrets such as `token=...`, `password=...`, `sk-...`, and GitHub
token prefixes.

If a maintainer explicitly needs full local paths to debug a path-resolution
bug, run:

```bash
asp diagnostics --include-paths --output asp-diagnostics-with-paths.json
```

Review the file before posting it publicly. Prefer attaching the redacted bundle
to GitHub issues; send `--include-paths` bundles only in a trusted channel.
