# Race Workflow

`asp race` creates N sibling forks, runs the same command in each fork in
parallel, then compares exit status, runtime, and diff size. It is meant for
best-of-N agent work: try several prompts, models, or strategies against the
same starting tree, inspect the results, promote the winner, and discard the
rest.

## Lane Identity

Use repeated `--label` flags to give lanes human-stable names:

```bash
asp race -n 3 --name fix \
  --label baseline \
  --label refactor \
  --label tests-first \
  -- claude -p "make the test suite pass"
```

Labels are assigned in lane order. If a lane has no explicit label, its fork
name is used. Human output shows both the fork and label, and `asp race --json`
includes `label` on each lane result.

## Lane Environment

Every lane process receives these metadata variables:

| Variable | Value |
| --- | --- |
| `ASP_RACE_LANE` | 1-based lane number |
| `ASP_RACE_NAME` | race name passed with `--name` |
| `ASP_RACE_FORK` | fork name, such as `fix-2` |
| `ASP_RACE_LABEL` | explicit label or fork name |
| `ASP_RACE_PATH` | lane working directory |
| `ASP_RACE_ATTEMPT` | current 1-based attempt number |
| `ASP_RACE_MAX_ATTEMPTS` | first attempt plus configured retries |

Add repeated `--env KEY=VALUE` flags for custom lane variables. Values may use
these placeholders: `{lane}`, `{fork}`, `{label}`, `{path}`, `{name}`.

```bash
asp race -n 2 --name variant \
  --label conservative \
  --label aggressive \
  --env AGENT_VARIANT={label} \
  --env TRACE_FILE={path}/trace-{lane}.json \
  -- claude -p "optimize this API without changing behavior"
```

Environment keys must start with a letter or `_` and contain only letters,
digits, and `_`. Invalid labels or templates are rejected before forks are
created.

## Runner Controls

Use `--timeout` to cap each attempt. Durations accept `ms`, `s`, `m`, or bare
seconds. Timed-out attempts are killed, logged, and reported with
`timed_out: true` when no later retry succeeds.

```bash
asp race -n 3 --timeout 5m -- claude -p "make the tests pass"
```

Use `--retries N` to rerun failed, timed-out, or spawn-failed attempts inside
the same lane directory. This lets a lane keep local attempt artifacts while
still reporting the final diff against the fork point.

```bash
asp race -n 3 --retries 1 --timeout 2m -- pytest
```

Use `--cancel-on-success` when the first exit-code-0 lane is good enough and
slower lanes should stop spending time or agent budget.

```bash
asp race -n 5 --cancel-on-success -- claude -p "fix the flaky test"
```

## Resuming Interrupted Races

Each race writes recoverable metadata to `.asp/races/<name>.json` before lanes
start and updates each lane as it runs. If the `asp race` process is interrupted
after lanes are created, resume it by name:

```bash
asp race --name variant --resume
```

Resume uses the recorded command, labels, environment templates, timeout,
retry, and cancellation settings. Lanes already marked `complete` are reported
from metadata and not rerun; lanes left `pending` or `running` are executed in
their existing fork directories.

## Test Result Ingestion

Use repeated `--junit PATH` flags to ingest JUnit XML reports from each lane
after the lane command exits. Paths are relative to the lane directory unless
absolute, and support the same `{lane}`, `{fork}`, `{label}`, `{path}`, and
`{name}` placeholders as `--env`.

```bash
asp race -n 3 --junit reports/{label}.xml -- \
  pytest --junitxml "reports/$ASP_RACE_LABEL.xml"
```

When a report exists and parses, `asp race --json` includes a `tests` summary
with report, test, failure, error, skipped, and runtime totals for that lane.

## Review And Cleanup

Lane logs are written to `<fork>/.asp/race.log`. The parent workspace is not
modified by lane commands.

```bash
asp forks
asp diff variant-2
asp promote variant-2
asp discard variant-1
asp discard variant-2
```

Promotion keeps the fork directory until you discard it, which makes post-merge
inspection and artifact recovery explicit.
