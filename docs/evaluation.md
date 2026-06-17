# 30-Minute Team Evaluation Guide

Use this guide when a team wants to decide whether `asp` is worth piloting on
real agent work. It is designed for one facilitator, one reviewer, and one
engineer who already knows the target repository.

The goal is not to prove `asp` is perfect. The goal is to answer, in 30
minutes, whether it solves a painful workflow safely enough to justify a
longer pilot.

## Before You Start

Pick a repository that resembles your daily work:

- It should have a meaningful test or lint command.
- It should contain both tracked and untracked files you care about.
- It should be safe to create sibling directories beside the repo.
- It should not require production credentials for local tests.

Install `asp` and verify the environment:

```bash
asp --version
git --version
```

On macOS and Linux, `asp fork` uses copy-on-write filesystem features when
available. On unsupported filesystems it may fall back to copy behavior; record
that because it affects enterprise rollout planning.

## The 30-Minute Run

### 0-5 Minutes: Baseline The Workspace

From the repository root:

```bash
asp init
asp status
asp checkpoint -m "evaluation baseline"
asp doctor
```

Success looks like:

- `asp init` does not modify source files or the user `.git` directory.
- `asp checkpoint` captures tracked plus untracked, non-gitignored source
  files.
- `asp doctor` reports no urgent findings or gives a corrective hint.
- `asp --json status` returns a parseable `{ok, result}` envelope.

Stop if:

- The repository cannot initialize.
- The team cannot accept the documented checkpoint scope. Gitignored files are
  excluded from checkpoints by design, while `asp fork` copies the whole
  physical tree.

### 5-10 Minutes: Prove Local Recovery

Make a small tracked edit and add one non-gitignored untracked file:

```bash
printf '\n# asp evaluation\n' >> README.md
printf 'scratch\n' > asp-eval-notes.txt
asp checkpoint -m "evaluation edit"
asp undo
asp restore 1
```

Success looks like:

- `asp undo` or `asp restore` puts the files back where expected.
- The previous state is recoverable through the safety checkpoint message.
- Error output, if any, includes a `hint:` line that tells a human or agent what
  to do next.

Stop if:

- Restore loses data that was in checkpoint scope.
- Your team cannot explain which files are intentionally outside checkpoint
  scope.

### 10-18 Minutes: Run A Best-Of-N Agent Or Script Race

Start with two lanes. Use the command your team already trusts for the
repository. For an agent workflow:

```bash
asp race -n 2 --name eval-fix \
  --label conservative \
  --label broad \
  -- claude -p "fix one failing test without unrelated refactors"
```

For a non-agent dry run, use a deterministic local command:

```bash
asp race -n 2 --name eval-script \
  --label small \
  --label larger \
  -- sh -c 'echo "$ASP_RACE_LABEL" >> asp-eval-notes.txt'
```

Then compare:

```bash
asp race compare --name eval-fix     # or eval-script if you used the dry run
asp forks
```

Success looks like:

- Each lane runs in a sibling fork and leaves the parent workspace unchanged.
- Human output shows lane labels, exit status, attempts, time, and diff size.
- `asp race compare --json` gives ranked results automation can consume.
- Lane logs are available at `<fork>/.asp/race.log`.

Stop if:

- Fork creation is too slow for the team to use daily.
- The command requires credentials or services that cannot safely run in
  multiple sibling copies.

### 18-23 Minutes: Promote And Review A Winner

Pick the best lane and land it as an ordinary branch:

```bash
asp promote eval-fix-1
git diff HEAD...asp/eval-fix-1
```

Replace `eval-fix` with the race name you used if you ran a different example.

Success looks like:

- The branch contains the intended source diff and not `.asp/` internals.
- Reviewers can use normal git and PR tooling.
- The original fork remains available until explicitly discarded.

Clean up after review:

```bash
asp discard eval-fix-1
asp discard eval-fix-2
```

Stop if:

- Promotion produces an unreviewable branch.
- Branch naming or protected-branch policy needs controls before a pilot.

### 23-27 Minutes: Verify The Trust Story

Run the stock-git recovery inspection:

```bash
GIT_DIR=.asp/shadow.git git log --all --oneline | head
asp diagnostics --output asp-eval-diagnostics.json
```

Success looks like:

- Checkpoints are visible as ordinary git commits in `.asp/shadow.git`.
- The diagnostics bundle is redacted by default.
- The team can describe how `.asp/` should be backed up for the pilot.

Stop if:

- Security reviewers are uncomfortable with local checkpoint storage.
- The team needs a policy for large files, excludes, or backup before any pilot.

### 27-30 Minutes: Score The Pilot

Record the result while the workflow is fresh.

| Criterion | Pass | Needs Work | Fail |
| --- | --- | --- | --- |
| Safety | Undo/restore worked for in-scope files. | Scope needs policy. | In-scope data was lost. |
| Speed | Fork/race felt interactive. | Usable only for selected repos. | Too slow for the target repo. |
| Reviewability | Promote produced a clean branch. | Branch policy needs setup. | Diff was not reviewable. |
| Agent Fit | Race or fork improved review quality. | Useful for some tasks only. | Added more process than value. |
| Operability | Doctor/diagnostics gave useful next actions. | Needs runbooks. | Errors were opaque. |
| Automation | `--json` output was easy to consume. | Schema mapping needed. | Not scriptable enough. |

Recommended decision:

- **Pilot now** if Safety, Reviewability, and Operability pass, and at least
  one workflow clearly improved.
- **Pilot after prep** if the only gaps are policy, filesystem choice, branch
  naming, or backup.
- **Do not pilot yet** if restore failed, promotion was unreviewable, or the
  target repo cannot safely run parallel agent lanes.

## Evaluation Notes Template

```text
Repository:
Date:
Facilitator:

Baseline:
- asp version:
- git version:
- OS/filesystem:
- checkpoint time:
- fork time:

Workflow tested:
- command:
- lanes:
- winner:

Scorecard:
- Safety:
- Speed:
- Reviewability:
- Agent fit:
- Operability:
- Automation:

Decision:
- Pilot now / pilot after prep / do not pilot yet

Follow-up:
- issues to file:
- policy docs needed:
- teams/repos for pilot:
```

## What To Read Next

- [Race workflow](race.md) for labels, environment variables, retries, JUnit
  ingestion, and saved-race comparison.
- [Enterprise workflow playbooks](playbooks.md) for bug-fix fleets,
  test-generation races, docs generation, and CI repair.
- [Trust model whitepaper](trust-model.md) for security-review boundaries,
  residual risks, and evidence links.
- [Backup and disaster recovery](backup-recovery.md) for `.asp/` backup policy,
  restore drills, and incident checklists.
- [Architecture](architecture.md) for module boundaries and trust invariants.
- [On-disk format](design/format.md) for stock-git recovery details.
- [Diagnostics](diagnostics.md) for safe issue-report bundles.
- [Filesystem detection](filesystems.md) for copy-on-write rollout planning.
