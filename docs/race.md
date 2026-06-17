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
