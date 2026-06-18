# Config templates

Start from these templates when introducing `.asp/config.toml` to a team. Treat
them as examples, not universal defaults.

Use a built-in template at init time when the repository fits a common shape:

```bash
asp init --template service
asp init --template monorepo
asp init --template generated-code
asp init --template media-heavy
```

The template is written to `.asp/config.toml`; review it like any other local
policy input before using it across a team.

Review a template without creating `.asp/`:

```bash
asp init --print-template monorepo
asp --json init --print-template monorepo
```

## Service Repository

Use this for typical application or service repos that produce coverage and
temporary files but otherwise fit the default capture model.

```toml
[capture]
extra_excludes = [
  "coverage/",
  "tmp/",
]
blob_threshold_mb = 50

[promote]
branch_template = "asp/{workspace}/{fork}"
```

## Monorepo

Use this when the repo has multiple package managers and large generated build
trees.

```toml
[capture]
extra_excludes = [
  "bazel-bin/",
  "bazel-out/",
  "bazel-testlogs/",
  "coverage/",
  "tmp/",
]
blob_threshold_mb = 50

[promote]
branch_template = "asp/{workspace}/{fork}"
```

Review notes:

- Prefer `extra_excludes` so the built-in defaults still apply.
- Keep source packages, migration files, fixtures, and lockfiles checkpointed.
- Pair with `.asp/policy.toml` branch prefixes if the team requires
  `asp/<workspace>/...` branches.

## Media-Heavy Repository

Use this when designers, ML teams, or documentation teams keep large binary
assets beside source.

```toml
[capture]
extra_excludes = [
  "renders/cache/",
  "exports/tmp/",
]
blob_threshold_mb = 10

[promote]
branch_template = "media/{workspace}/{fork}"
```

Review notes:

- Lowering `blob_threshold_mb` moves more large files into `.asp/blobs/`.
- Keep original assets checkpointed unless they are truly reproducible.
- Verify backup coverage for `.asp/blobs/` before relying on old checkpoints.

## Generated-Code Repository

Use this when generated clients or schemas are reproducible but some generated
outputs still need review.

```toml
[capture]
extra_excludes = [
  "generated/cache/",
  "generated/tmp/",
  "openapi/.cache/",
]
blob_threshold_mb = 25

[promote]
branch_template = "gen/{workspace}/{fork}"
```

Review notes:

- Exclude caches, not reviewed generated output.
- Keep generator inputs and lockfiles checkpointed.
- Use `asp diff --stat` and `asp diff --patch` to confirm generated churn is
  intentional before promotion.

## Verify A Template

```bash
asp config validate
asp --json config show
asp checkpoint -m "config template smoke test"
asp fork --name config-smoke
asp discard config-smoke
```

Run the smoke test in a disposable branch before rolling the template into a
shared repository.
