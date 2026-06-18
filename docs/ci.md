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
      - name: Annotate asp preflight failures
        if: failure() && hashFiles('asp-preflight.json') != ''
        run: |
          python3 - <<'PY'
          import json

          with open("asp-preflight.json", encoding="utf-8") as handle:
              report = json.load(handle)["result"]

          for check in report["checks"]:
              if not check["ok"]:
                  print(f"::error title={check['id']}::{check['summary']} ({check['runbook']})")
          PY
      - name: Write asp preflight SARIF
        if: failure() && hashFiles('asp-preflight.json') != ''
        run: asp preflight --sarif > asp-preflight.sarif || true
      - name: Upload asp preflight SARIF
        if: failure() && hashFiles('asp-preflight.sarif') != ''
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: asp-preflight.sarif
      - name: Upload asp preflight report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: asp-preflight
          path: asp-preflight.json
```

## Direct Secret Scan SARIF

```yaml
      - name: Run asp secrets scan SARIF
        run: asp secrets scan --sarif > asp-secrets.sarif || true
      - name: Upload asp secrets scan SARIF
        if: always() && hashFiles('asp-secrets.sarif') != ''
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: asp-secrets.sarif
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

## GitLab Code Quality

GitLab Code Quality consumes CodeClimate JSON, not SARIF. Keep SARIF as an
artifact and convert the redacted `asp --json secrets scan` report for merge
request widgets:

```yaml
asp_secret_quality:
  image: debian:bookworm-slim
  before_script:
    - apt-get update
    - apt-get install -y --no-install-recommends ca-certificates curl git python3
    - curl -fsSL https://raw.githubusercontent.com/ArnavBorkar/agentspaces/main/install.sh | sh
    - export PATH="$HOME/.local/bin:$PATH"
  script:
    - asp --json secrets scan > asp-secrets.json || true
    - asp secrets scan --sarif > asp-secrets.sarif || true
    - |
      python3 - <<'PY'
      import hashlib
      import json

      with open("asp-secrets.json", encoding="utf-8") as handle:
          report = json.load(handle)

      issues = []
      for finding in report.get("result", {}).get("findings", []):
          text = f"{finding['kind']}:{finding['path']}:{finding['line']}:{finding['redacted']}"
          issues.append({
              "type": "issue",
              "check_name": f"asp.secrets.{finding['kind']}",
              "description": f"{finding['kind']} candidate: {finding['redacted']}",
              "categories": ["Security"],
              "severity": "blocker",
              "fingerprint": hashlib.sha256(text.encode()).hexdigest(),
              "location": {
                  "path": finding["path"],
                  "lines": {"begin": finding["line"]},
              },
          })

      with open("gl-code-quality-report.json", "w", encoding="utf-8") as handle:
          json.dump(issues, handle, indent=2)
      PY
  artifacts:
    when: always
    reports:
      codequality: gl-code-quality-report.json
    paths:
      - asp-secrets.json
      - asp-secrets.sarif
      - gl-code-quality-report.json
```

## Generic SARIF Artifacts

```bash
asp preflight --sarif > asp-preflight.sarif || true
asp secrets scan --sarif > asp-secrets.sarif || true
```

Upload those files to any CI platform or dashboard that accepts SARIF 2.1.0.
`asp` only writes local artifacts; it never uploads findings on its own.

## Rules

- Keep these jobs read-only: do not run `asp doctor --fix`, `asp undo`,
  `asp restore`, `asp promote`, or `asp discard`.
- Run `asp config validate` before `asp preflight` so syntax failures are easy
  to spot in logs.
- Use the JSON `id` field, such as `preflight.secrets`, for stable CI
  annotations and dashboards; `name` is human display text.
- Upload `asp preflight --sarif` when the platform can ingest SARIF 2.1.0, for
  example GitHub code scanning.
- Use `asp secrets scan --sarif` for direct secret-scan uploads when a full
  readiness gate is not needed.
- For GitLab Code Quality, convert redacted `asp --json secrets scan` findings
  to CodeClimate JSON and keep SARIF as a normal artifact.
- Upload the JSON report when teams want persistent evidence for security or
  platform review.
- If `asp preflight` fails, use `asp doctor --runbook` and `asp secrets scan`
  locally to triage the finding.
