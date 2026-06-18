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

The JSON result includes:

| Field | Meaning |
| --- | --- |
| `kind` | Always `recovery` for this drill. |
| `status` | `passed` when the stock-git restore completed. |
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
