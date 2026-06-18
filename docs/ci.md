# CI preflight examples

Use these examples to add asp readiness checks without mutating the workspace.

## GitHub Actions

```yaml
name: asp preflight

on:
  pull_request:
  push:
    branches: [main]

jobs:
  asp-preflight:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install asp
        run: |
          curl -fsSL https://raw.githubusercontent.com/ArnavBorkar/agentspaces/main/install.sh | sh
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - name: Validate asp config
        run: asp config validate
      - name: Run asp preflight
        run: asp --json preflight > asp-preflight.json
      - name: Upload asp preflight report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: asp-preflight
          path: asp-preflight.json
```

## GitLab CI

```yaml
asp_preflight:
  image: debian:bookworm-slim
  before_script:
    - apt-get update
    - apt-get install -y --no-install-recommends ca-certificates curl git
    - curl -fsSL https://raw.githubusercontent.com/ArnavBorkar/agentspaces/main/install.sh | sh
    - export PATH="$HOME/.local/bin:$PATH"
  script:
    - asp config validate
    - asp --json preflight > asp-preflight.json
  artifacts:
    when: always
    paths:
      - asp-preflight.json
```

## Rules

- Keep these jobs read-only: do not run `asp doctor --fix`, `asp undo`,
  `asp restore`, `asp promote`, or `asp discard`.
- Run `asp config validate` before `asp preflight` so syntax failures are easy
  to spot in logs.
- Upload the JSON report when teams want persistent evidence for security or
  platform review.
- If `asp preflight` fails, use `asp doctor --runbook` and `asp secrets scan`
  locally to triage the finding.
