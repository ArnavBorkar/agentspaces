# Workspace Config

`asp init` writes `.asp/config.toml` as a user-editable workspace config file.
Every field is optional, and a missing or empty config file means defaults.
For team review workflows, see [config review](config-review.md).

The parser is strict: unknown tables or keys are rejected with a
`store_corrupt` error and a hint to fix the TOML or delete the file to restore
defaults.

Inspect the effective config from the CLI:

```bash
asp config show
asp --json config show
asp config validate
asp --json config validate
```

`show` reports whether `.asp/config.toml` exists, the resolved config values,
the effective checkpoint excludes written into the shadow git repo, the
large-file blob threshold in bytes, and the promote branch template.

`validate` uses a narrow read path: it discovers the workspace root and parses
only `.asp/config.toml`. That makes it suitable for CI and for diagnosing config
syntax even if another store component needs `asp doctor`.

## Schema

```toml
[capture]
excludes = ["node_modules/", "target/"]
extra_excludes = ["data/raw/"]
blob_threshold_mb = 50

[promote]
branch_template = "asp/{fork}"
```

| TOML path | Type | Default | Meaning |
| --- | --- | --- | --- |
| `capture.excludes` | array of strings | `["node_modules/", "target/", ".venv/", "venv/", "__pycache__/", "build/", "dist/", ".next/", ".cache/"]` | Derived-state patterns excluded from checkpoints. Setting this replaces the default list. |
| `capture.extra_excludes` | array of strings | `[]` | Additional checkpoint exclude patterns appended after `capture.excludes`. Use this when you want the defaults plus project-specific generated state. |
| `capture.blob_threshold_mb` | unsigned integer | `50` | Files larger than this many MiB are stored in the BLAKE3 content-addressed sidecar under `.asp/blobs/` instead of as shadow-git blobs. |
| `promote.branch_template` | string | `"asp/{fork}"` | Branch template used by `asp promote <fork>` when `--branch` is omitted. |

All exclude patterns are written to the shadow git repo's `info/exclude` file,
so they use gitignore pattern syntax. `asp` also always excludes `/.asp/`
internally; users do not need to list it.

## Capture Semantics

The config only affects checkpoints. Forks are physical copy-on-write clones of
the whole directory and carry excluded paths too.

Checkpoints also respect the workspace's normal `.gitignore`. `extra_excludes`
can exclude more files, but it cannot force-include a file that `.gitignore`
already ignores because gitignore rules take precedence over the shadow repo's
`info/exclude` file.

`capture.excludes` replaces the default derived-state list. Prefer
`capture.extra_excludes` unless you intentionally want to manage the full list
yourself.

## Promote Branch Templates

`promote.branch_template` controls the default branch created by
`asp promote <fork>`. Passing `--branch <name>` still takes precedence for a
single promotion.

Supported placeholders:

- `{fork}`: the sanitized fork name. This placeholder is required so repeated
  promotions do not collide by default.
- `{workspace}`: the sanitized workspace directory name.
- `{workspace_id}`: the workspace UUID from `.asp/workspace.json`.

Templates cannot be empty, cannot contain whitespace, and cannot use unknown
placeholders. Combine this setting with
`promote.allowed_branch_prefixes` in `.asp/policy.toml` when a team wants both
friendly defaults and enforceable branch rules.

## Examples

For larger repositories, see [monorepo tuning](monorepo-tuning.md) before
changing excludes or large-file thresholds.

Append project-specific generated output while keeping defaults:

```toml
[capture]
extra_excludes = ["tmp/", "data/raw/", "coverage/"]
```

Replace the default exclude list completely:

```toml
[capture]
excludes = ["node_modules/", "target/", "bazel-bin/", "bazel-out/"]
```

Lower the large-file sidecar threshold for media-heavy repositories:

```toml
[capture]
blob_threshold_mb = 10
```

Name promoted branches by workspace for multi-repo dashboards:

```toml
[promote]
branch_template = "review/{workspace}/{fork}"
```

## Recovery

If `.asp/config.toml` is invalid, commands that open the workspace fail before
mutating the store. Fix the TOML syntax, or delete `.asp/config.toml` to use
defaults again.

Changing config does not rewrite old checkpoints. The new settings apply to the
next checkpoint.
