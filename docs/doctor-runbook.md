# Doctor runbook

Use `asp doctor --runbook` to print the matching link beside each finding.
Use `asp doctor --json --runbook` when an agent or CI job needs structured
repair guidance.

## Shadow git config drift

Run:

```bash
asp doctor --fix
```

This restores asp's expected shadow-git performance and safety settings. The
repair does not touch the user's `.git` repository.

## Torn journal tail

Run:

```bash
asp doctor --fix
```

This truncates only incomplete trailing journal bytes after the last valid CRC
record. Keep a copy of `.asp/journal.jsonl` first if the crash is under
investigation.

## Shadow HEAD drift

Run:

```bash
asp doctor --fix
```

This repoints the shadow HEAD ref to the latest checkpoint ref. Checkpoint refs
remain ordinary git refs inside `.asp/shadow.git`.

## Missing active fork directory

Run:

```bash
asp doctor --fix
```

This marks the missing fork discarded in `.asp/forks.json`. It does not delete
any directory because the directory is already gone.

## Torn fork clone

Run:

```bash
asp doctor --fix
```

This removes a fork that has a recorded pending intent from an interrupted
clone. asp only performs this cleanup when the registry proves it owns the
pending fork path.

## Missing CAS blob recreatable

Run:

```bash
asp doctor --fix
```

This recreates a missing large-file sidecar blob from the current working file.
Review the working file first if you suspect it changed after the checkpoint.

## Journal CRC mismatch

Do not run destructive cleanup first. Preserve `.asp/journal.jsonl`, inspect the
corrupt line, and restore the journal from backup if that provenance record is
needed for audit.

## Missing checkpoint commit

Restore `.asp/shadow.git` from backup if the missing checkpoint matters. You can
still inspect surviving checkpoints with stock git commands against
`.asp/shadow.git`.

## Promoted fork cleanup

After review is complete, remove the promoted fork directory with the exact
command shown by doctor:

```bash
asp discard <fork>
```

## Unregistered fork-looking directory

Inspect the sibling directory manually. asp will not delete a directory unless
its registry proves asp created it.

## Missing CAS blob and working file

Restore `.asp/blobs/` or the working file from backup before relying on older
checkpoints that reference the missing large file.

## Corrupt CAS blob

Restore the corrupt blob from backup, then rerun:

```bash
asp doctor --deep
```

## Runtime prerequisite failure

Follow the hint embedded in the finding, then rerun:

```bash
asp doctor
```

## General doctor triage

Read the finding, keep a backup of `.asp/`, and prefer `asp doctor --fix` only
when the finding says the repair is safe. For support bundles, use:

```bash
asp diagnostics --output diagnostics.json
```
