# Local-First Team Workflows

This draft shows how teams can run audit, policy, and approval workflows around
`asp` without a hosted control plane. A future service may make these workflows
easier, but the local version should remain understandable and usable.

These workflows use today's primitives:

- `.asp/journal.jsonl` through `asp log`;
- fork comparison through `asp forks` and `asp race compare`;
- promotion through ordinary git branches;
- diagnostics through redacted JSON bundles;
- repository policy through existing review, CI, and branch-protection tools.

## Workflow Artifacts

Keep the evidence small and reviewable:

| Artifact | Command | Purpose |
| --- | --- | --- |
| Recent operation log | `asp --json log -n 50` | Shows checkpoints, restores, promotes, sources, tools, and timing. |
| Workspace health | `asp doctor --deep` | Proves the local store is healthy before approval. |
| Fork comparison | `asp forks` or `asp race compare --name <race>` | Shows candidate work and ranking. |
| Promoted branch diff | `git diff main...asp/<fork>` | Gives reviewers normal code-review context. |
| Diagnostics bundle | `asp diagnostics --output asp-diagnostics.json` | Redacted support and incident evidence. |
| Test output | Team's normal test command | Proves the chosen lane works. |

Do not attach full diagnostics with paths, race logs, or fork contents to public
issues unless the team has reviewed them.

## Audit Workflow

Goal: preserve enough local evidence to answer "what did the agent do, where,
and why was it accepted?"

### Before Work

```bash
asp init
asp checkpoint -m "audit: baseline before agent task"
asp doctor --deep
```

Record:

- repo and branch;
- task or issue id;
- allowed agent credentials;
- intended test command;
- files or packages in scope.

### During Work

Use labels that describe strategy:

```bash
asp race -n 3 \
  --name issue-1234 \
  --label minimal \
  --label broader \
  --label tests-first \
  -- <agent-command>
```

After lanes finish:

```bash
asp race compare --name issue-1234
asp forks
```

Record:

- lane labels;
- winning lane;
- test result;
- rejected lanes and why.

### After Promotion

```bash
asp promote issue-1234-1 --branch asp/issue-1234
git diff main...asp/issue-1234
asp --json log -n 50 > asp-audit-log.json
asp diagnostics --output asp-diagnostics.json
```

Store the audit packet where the team already reviews work: PR description,
internal ticket, or local release notes. The packet should point to files and
commands, not replace code review.

## Policy Workflow

Goal: make local rules explicit before agents write.

Use existing repo controls first:

- `.gitignore` for secrets and generated state;
- `.asp/config.toml` for checkpoint excludes and blob thresholds;
- branch protection and required CI in the git host;
- CODEOWNERS or reviewer rotation for sensitive paths;
- endpoint controls for credentials and network access.

Before a pilot, write a short policy note:

```text
Agent workspace policy

- allowed repos:
- allowed credentials:
- allowed commands/tools:
- paths requiring human review:
- generated paths excluded from checkpoints:
- large-file threshold:
- required test command:
- backup location for .asp:
- diagnostics sharing channel:
- approval rule before promote:
```

Then verify the local config:

```bash
asp --json status
asp --json stats
asp doctor --deep
```

Future `asp policy` features can automate more of this, but they should still
compile down to local, reviewable configuration and documented checks.

## Approval Workflow

Goal: make `asp promote` a local branch creation step that feeds ordinary review
systems.

### Candidate Selection

```bash
asp forks
asp race compare --name <race>
```

Approve only after:

- the winning fork has a focused diff;
- tests pass in the fork or promoted branch;
- excluded files are understood;
- large-file changes have a recovery path;
- rejected forks are either preserved for evidence or discarded.

### Promotion

```bash
asp promote <winner> --branch asp/<issue-or-task>
git diff main...asp/<issue-or-task>
```

The branch should go through normal code review. `asp promote` is not a review
bypass; it is the handoff from local agent work to normal git workflow.

### Cleanup

```bash
asp discard <loser-1>
asp discard <loser-2>
asp doctor --deep
```

If a loser fork contains important evidence, checkpoint it or copy the evidence
to the review packet before discarding.

## Local Approval Packet

For high-risk changes, include this in the PR or ticket:

```text
asp approval packet

- task:
- baseline checkpoint:
- winning fork:
- promote branch:
- test command and result:
- reviewer:
- approver:
- rejected lanes:
- diagnostics bundle location:
- backup/restore note:
- known residual risk:
```

The packet should be enough for a reviewer to reproduce the decision locally
without a hosted dashboard.

## Future Automation Boundaries

Automation may add:

- a command that writes the approval packet;
- local policy validation before `race` or `promote`;
- signed local audit bundles;
- optional hosted dashboards over explicitly shared metadata;
- approval annotations on promoted branches.

Automation must not:

- require a hosted approval before local recovery;
- hide fork diffs or checkpoint logs behind a service;
- upload diagnostics, source, or race logs by default;
- make the hosted dashboard the only audit record.

## Related Docs

- [Open-core boundary policy](open-core-boundary.md)
- [Local engine governance](local-engine-governance.md)
- [Future control plane constraints](control-plane-constraints.md)
- [Enterprise workflow playbooks](playbooks.md)
- [Trust model whitepaper](trust-model.md)
