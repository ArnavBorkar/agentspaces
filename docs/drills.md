# Incident Drills

Incident drills prove that an `asp` workspace can be recovered before a real
incident. They are intentionally non-destructive: a drill should exercise the
same recovery primitive an operator would trust during an outage while leaving
the current workspace alone.

## Recovery Drill

Run the recovery drill from an initialized workspace with at least one
checkpoint:

```bash
asp drill recovery
```

By default, the drill restores the latest checkpoint. To rehearse a specific
checkpoint, pass its sequence number or commit prefix:

```bash
asp drill recovery --checkpoint 42
asp drill recovery --checkpoint f00dbabe
```

The drill creates a unique temp directory, creates a separate temporary Git
index file, then uses stock `git` against `.asp/shadow.git`:

```bash
git show-ref --verify refs/asp/checkpoints/<seq>
git ls-tree -r -z --name-only <checkpoint-commit>
git read-tree <checkpoint-commit>
git checkout-index -a -f
```

The live workspace is not restored, cleaned, or rewritten. The recovered files
land under the reported `recovered_tree` path, and the temporary index file is
reported separately as `index_file`.

## Audit Output

Use JSON output when a CI job, support script, or compliance control needs to
archive evidence:

```bash
asp --json drill recovery > asp-recovery-drill.json
```

Every JSON drill report includes `metadata` with `schema_version`, a unique
`report_id`, UTC `generated_at`, `asp_version`, `command`, `workspace_id`,
`workspace_root`, `drill`, and `status`. Audit systems should index those
fields before reading drill-specific sections.

The JSON result includes:

| Field | Meaning |
| --- | --- |
| `kind` | Always `recovery` for this drill. |
| `status` | `passed` when the stock-git restore completed. |
| `metadata` | Shared audit metadata for report indexing and retention. |
| `workspace_root` | Workspace that owns the `.asp/shadow.git` store. |
| `checkpoint.seq` / `checkpoint.commit` | The exact checkpoint restored. |
| `recovered_tree` | Temp directory containing restored files. |
| `index_file` | Temp Git index used by the restore. |
| `files_restored` | Number of paths listed by `git ls-tree -r`. |
| `stock_git_commands` | Commands the drill executed with recovery env vars. |
| `current_workspace_untouched` | Always `true` for a successful drill. |
| `next_actions` | Cleanup and follow-up validation steps. |

Do not commit the JSON report or recovered tree unless the team has reviewed
the contents. They may contain proprietary source or checkpointed secrets.
For scheduled CI drills that run in a temporary copy of the checkout and upload
only JSON evidence, see [CI preflight examples](ci.md#scheduled-recovery-drills).

## Failure Triage

| Symptom | Likely Cause | Corrective Action |
| --- | --- | --- |
| `nothing_to_do` | The workspace has no checkpoints. | Run `asp checkpoint -m "baseline"` and rerun the drill. |
| `checkpoint_not_found` | The requested sequence or commit prefix does not match a checkpoint. | Run `asp log` and rerun with a listed `#seq` or commit prefix. |
| `git_missing` | `git` is not installed or is not on `PATH`. | Install Git, verify `git --version`, and rerun the drill. |
| `git_failed` from `show-ref` | The checkpoint ref is missing or the shadow repo is damaged. | Run `asp doctor --deep`; restore `.asp/` from backup if the ref is gone. |
| `git_failed` from `read-tree` or `checkout-index` | Shadow git objects are missing or the temp directory is not writable. | Run `asp doctor --deep`; check temp directory permissions; restore `.asp/shadow.git/` from backup if objects are missing. |

Large files managed through the sidecar are restored as pointer files by the
stock-git drill. The pointer names the content-addressed blob under
`.asp/blobs/`; copy that blob over the pointer path when you need to materialize
the large file during a manual recovery exercise.

## Cleanup

After saving any evidence your team needs, remove the reported temp paths:

```bash
rm -rf <recovered_tree>
rm -f <index_file>
```

Then validate the original workspace:

```bash
asp doctor --deep
```

## Fork Drill

Run the fork drill to prove the current filesystem can create, inspect, and
clean up an `asp` fork:

```bash
asp drill fork
```

The drill uses the same engine paths as normal fork operations:

1. Creates a uniquely named disposable fork with `asp fork` internals.
2. Observes that fork through the same comparison path as `asp forks`.
3. Discards the fork through the normal cleanup guard.
4. Checks whether a future `asp promote` would have a user `.git` repository
   and an available preview branch name.

The fork drill does not create a user git branch, does not push, and does not
edit files in the original workspace. It does update `.asp/` metadata and the
journal because real fork and discard operations are the thing being tested.

Use JSON output for audit evidence:

```bash
asp --json drill fork > asp-fork-drill.json
```

The shared `metadata` block has the same fields as the recovery drill report,
with `drill: "fork"` and `command: "asp drill fork"`.

The JSON result includes:

| Field | Meaning |
| --- | --- |
| `kind` | Always `fork` for this drill. |
| `metadata` | Shared audit metadata for report indexing and retention. |
| `fork.name` / `fork.path` | The temporary fork that was created. |
| `fork.method` | Clone method used on this filesystem. |
| `compare.seen` | Whether fork comparison observed the temporary fork. |
| `cleanup.path_removed` | Whether discard removed the fork directory. |
| `cleanup.registry_status` | Final fork registry state, normally `discarded`. |
| `promote.branch_preview` | Branch name a future promote would use by default. |
| `promote.ready` | Whether a user git repo exists and the preview branch is available. |
| `current_workspace_files_untouched` | Always `true` for a successful drill. |

## Fork Failure Triage

| Symptom | Likely Cause | Corrective Action |
| --- | --- | --- |
| `fork_exists` | A previous drill or user fork reused the generated name. | Rerun the drill; names include process and timestamp entropy. |
| `cross_volume` | The workspace is on a filesystem where fork destinations would cross volumes. | Move the workspace so its parent directory is on the same volume, then rerun. |
| `policy_violation` | Local policy blocks another fork or requires a fresher checkpoint. | Run `asp policy explain`, resolve the policy condition, and rerun. |
| `fork_has_unpromoted_work` | The disposable fork changed unexpectedly before cleanup. | Inspect the reported fork path, then run `asp discard <fork> --force` if the work is disposable. |
| `store_corrupt` during cleanup | Fork registry and filesystem state disagree. | Run `asp doctor --fix`, verify no drill fork directories remain, and rerun. |

If `promote.ready` is `false` because `user_git_repo` is `false`, initialize or
clone the project with ordinary Git before relying on `asp promote` during an
incident workflow. The drill intentionally reports readiness rather than
creating and deleting branches in the user's repository.
