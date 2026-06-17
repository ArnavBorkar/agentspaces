# Enterprise Agent Workflow Playbooks

These playbooks turn `asp` primitives into repeatable team workflows. They are
written for local-first use: no hosted control plane, no telemetry, and no
required account beyond the agent tool you already run.

Use them after the [30-minute evaluation](evaluation.md) proves the repository
is a good pilot candidate. Pair them with the [trust model whitepaper](trust-model.md)
when security reviewers need boundaries, residual risks, and evidence links.

## Shared Guardrails

Before any playbook:

```bash
asp init
asp checkpoint -m "baseline before agent workflow"
asp doctor
```

Agree on these rules:

- Run agents only with credentials that are safe in sibling workspace copies.
- Keep production secrets out of prompts, logs, diagnostics, and issue reports.
- Remember the scope split: `asp fork` copies the whole physical tree;
  checkpoints capture tracked plus untracked, non-gitignored source files.
- Prefer `--label` values that describe strategy, not model internals.
- Promote one winner, then explicitly discard every lane you no longer need.
- Use `asp race compare --json` when another script or dashboard needs to rank
  lanes.

Recommended naming pattern:

```bash
asp race -n 3 --name <ticket-or-run-id> \
  --label conservative \
  --label tests-first \
  --label broad \
  -- <agent-or-script-command>
```

## Bug-Fix Fleet

Use this when a bug is understood enough to describe, but the best patch shape
is not obvious.

### Setup

Capture the failing state:

```bash
asp checkpoint -m "bug baseline: <ticket>"
```

Write down:

- The failing command.
- The smallest reproduction.
- Files or modules that should not be touched.
- Any compatibility constraint the agent must preserve.

### Run

```bash
asp race -n 3 --name bug-1234 \
  --label minimal \
  --label tests-first \
  --label refactor-ok \
  --timeout 20m \
  --retries 1 \
  -- claude -p "Fix BUG-1234. Reproduce with '<test command>'. Keep the patch minimal and avoid unrelated formatting."
```

If the bug has a deterministic test report, ingest it:

```bash
asp race -n 3 --name bug-1234 \
  --junit reports/{label}.xml \
  -- pytest tests/regression --junitxml "reports/$ASP_RACE_LABEL.xml"
```

### Review

```bash
asp race compare --name bug-1234
asp forks
git -C ../<repo>@bug-1234-1 diff
```

Pick the lane that:

- Fixes the reproduction.
- Adds or updates a regression test.
- Touches the fewest unrelated files.
- Leaves generated files intentional and explainable.

### Land

```bash
asp promote bug-1234-1
git diff HEAD...asp/bug-1234-1
asp discard bug-1234-2
asp discard bug-1234-3
```

Stop and rerun with clearer prompts if all lanes pass tests but make
unreviewable or risky changes.

## Test-Generation Race

Use this when code appears stable but coverage, edge cases, or regression tests
are weak.

### Setup

Choose the test target and the acceptance command:

```bash
asp checkpoint -m "test generation baseline"
```

Examples:

- `cargo test -p crate_name`
- `pytest tests/module`
- `npm test -- --runInBand`
- `go test ./pkg/...`

### Run

```bash
asp race -n 4 --name tests-api \
  --label edge-cases \
  --label property-tests \
  --label regression-only \
  --label failure-modes \
  --junit reports/{label}.xml \
  --timeout 15m \
  -- claude -p "Add high-value tests for the API module. Do not change production behavior. Run '<acceptance command>' and write a JUnit report to reports/$ASP_RACE_LABEL.xml if the test runner supports it."
```

If your test runner cannot emit JUnit, still use `asp race`; compare exit code,
runtime, and diff size.

### Review

Rank by:

- Tests fail before the production fix or cover a known risk.
- Assertions are specific, not snapshot noise.
- Fixtures are small and local.
- Runtime stays acceptable.

Useful commands:

```bash
asp race compare --name tests-api
git -C ../<repo>@tests-api-2 diff --stat
git -C ../<repo>@tests-api-2 diff
```

Land only tests that the team understands. Discard clever but opaque test
generation.

## Docs Generation

Use this when the product behavior exists but onboarding, runbooks, or API docs
lag behind.

### Setup

Define the reader and the source of truth:

```bash
asp checkpoint -m "docs baseline"
```

Examples:

- New contributor setup from `docs/development.md`.
- Operator recovery from `docs/design/format.md`.
- User workflow from `README.md` plus CLI help.

### Run

```bash
asp race -n 3 --name docs-onboarding \
  --label concise \
  --label runbook \
  --label examples-heavy \
  -- claude -p "Improve onboarding docs for a new enterprise evaluator. Keep claims grounded in existing tests, docs, or commands. Do not invent features."
```

For docs-only work, a cheap scripted lane can validate links or formatting if
your repo has such a tool:

```bash
asp race -n 2 --name docs-links \
  --label markdownlint \
  --label linkcheck \
  -- sh -c '<your docs validation command>'
```

### Review

Prefer the lane that:

- Names the intended reader.
- Gives exact commands and expected outcomes.
- Links to source-of-truth docs instead of duplicating subtle invariants.
- Does not make performance, security, or compatibility claims without
  evidence.

Before promoting, scan for unsupported promises:

```bash
rg -n "always|never|guarantee|secure|enterprise|production" \
  ../<repo>@docs-onboarding-1/docs \
  ../<repo>@docs-onboarding-1/README.md
```

## CI Repair

Use this when a CI failure is reproducible locally or the logs identify a
likely failing command.

### Setup

Save the failing context:

```bash
asp checkpoint -m "ci failure baseline"
```

Collect:

- CI run URL.
- Failing job and command.
- Relevant log excerpt.
- Whether the failure is deterministic or flaky.

### Run

Use labels that reflect hypotheses:

```bash
asp race -n 3 --name ci-277118 \
  --label reproduce-first \
  --label dependency-path \
  --label platform-assumption \
  --timeout 25m \
  --cancel-on-success \
  -- claude -p "Fix the CI failure from <run URL>. First reproduce with '<failing command>'. Keep the patch scoped and explain the root cause."
```

For command-only diagnosis:

```bash
asp race -n 3 --name ci-command \
  --label linux \
  --label macos-assumption \
  --label clean-cache \
  -- sh -c '<failing command>'
```

### Review

The winning lane should include:

- A root-cause explanation in the commit or PR description.
- A regression test if the failure was product behavior.
- A workflow change only if the product was already correct.
- No weakening of trust gates to get green CI.

Commands:

```bash
asp race compare --name ci-277118
git -C ../<repo>@ci-277118-1 diff
```

Do not promote a lane that only skips, relaxes, or deletes a failing check
unless the team explicitly agrees the check was invalid.

## Cleanup Checklist

After every playbook:

```bash
asp forks
asp promote <winner>
asp discard <loser-1>
asp discard <loser-2>
asp doctor
```

Record:

- Which prompt or script won.
- Which command proved it.
- Which files changed.
- Which lanes were discarded.
- Any follow-up issue for product gaps, docs gaps, or policy gaps.
