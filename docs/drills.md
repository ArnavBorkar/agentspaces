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

## Corrective Action Matrix

Use this matrix for local drills, scheduled CI drill artifacts, and support
handoffs. Start with the `error.code` from the standard JSON error envelope when
the command exits nonzero. For successful fork drills, also inspect
`promote.ready`, `cleanup.path_removed`, and `cleanup.registry_status`.

| Drill Signal | Evidence Field | First Command | Corrective Action | Escalate When |
| --- | --- | --- | --- | --- |
| No checkpoint exists | `error.code: "nothing_to_do"` | `asp log -n 5` | Run `asp checkpoint -m "baseline"` in the workspace or CI temp copy, then rerun `asp drill recovery`. | `asp log` shows checkpoints but the drill still reports `nothing_to_do`. |
| Requested checkpoint is unknown | `error.code: "checkpoint_not_found"` | `asp log -n 20` | Rerun with a listed `#seq` or commit prefix; update scheduled jobs that pin old checkpoint IDs. | The checkpoint ref should exist in a backup but is missing locally. |
| Git is unavailable | `error.code: "git_missing"` | `git --version` | Install Git on the runner or workstation and ensure it is on `PATH`. | The runner image claims Git is installed but `asp` still cannot spawn it. |
| Checkpoint ref is missing | `error.code: "git_failed"` and `show-ref` in the message | `asp doctor --deep` | Restore `.asp/` from the latest backup that contains `refs/asp/checkpoints/<seq>`, then rerun the drill. | The ref is missing from every backup or `asp doctor --deep` reports shadow-git corruption. |
| Shadow object restore fails | `error.code: "git_failed"` with `read-tree` or `checkout-index` | `asp doctor --deep` | Check `TMPDIR` permissions and free space; restore `.asp/shadow.git/objects` from backup if objects are missing. | The same object is missing after backup restore. Preserve `.asp/` before repair. |
| Large file appears as pointer JSON | Successful recovery report with sidecar pointer content | `asp doctor --deep` | Locate the named blob under `.asp/blobs/` and copy it over the pointer path during manual recovery. | The pointer references a missing blob; preserve the report and run `asp diagnostics --include-paths` in a trusted channel. |
| Fork policy blocks the drill | `error.code: "policy_violation"` | `asp policy explain` | Resolve max-active-fork or checkpoint-age policy requirements, then rerun `asp drill fork`. | Policy and current workspace state disagree, or the rule cannot be satisfied safely. |
| Fork cleanup did not finish | `cleanup.path_removed: false`, `cleanup.registry_status`, or `error.code: "store_corrupt"` | `asp forks` | Preserve the fork path if it contains unexpected work; otherwise run `asp doctor --fix` and rerun `asp drill fork`. | Cleanup fails twice or the registry points at a path outside the expected fork directory. |
| Promote readiness is false | `promote.ready: false` | `git status` | Initialize or clone with ordinary Git, or choose a branch template that does not collide with an existing branch. | Incident workflow depends on `asp promote` and no safe user-git branch can be created. |
| Scheduled CI artifact is missing | Missing `asp-drill-evidence/*.json` artifact | CI job log | Confirm the install step, temp-copy step, and `asp checkpoint -m "ci scheduled drill baseline"` ran before the drills. | The job reached the upload step but both JSON reports are absent. |

When a drill failure repeats after the corrective action, preserve evidence
before running destructive cleanup:

```bash
asp diagnostics --output asp-diagnostics.json
asp doctor --deep > asp-doctor-deep.txt
asp log -n 20 --json > asp-log.json
```

Keep the failing drill JSON report, command stderr, CI run URL, and a backup or
snapshot of `.asp/` until the root cause is understood. Do not run
`asp doctor --fix`, delete drill fork paths, or rotate backups before preserving
that evidence when the signal is `store_corrupt`, missing shadow objects, or a
missing sidecar blob.
