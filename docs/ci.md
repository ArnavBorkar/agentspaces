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

## Config Drift Gate

Use this pattern when a platform team has a reviewed baseline config and wants CI
to fail on unsafe drift without mutating the workspace. Store the baseline in
the repo, for example `.ci/asp/config.baseline.toml`.

```yaml
name: asp config drift

on:
  pull_request:

jobs:
  asp-config-drift:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install asp
        run: |
          curl -fsSL https://raw.githubusercontent.com/ArnavBorkar/agentspaces/main/install.sh | sh
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - name: Compare asp config to baseline
        run: |
          asp config validate
          asp --json config diff \
            --against .ci/asp/config.baseline.toml \
            > asp-config-diff.json
          python3 - <<'PY'
          import json
          import sys

          unsafe_fields = {
              "capture.excludes",
              "capture.extra_excludes",
              "capture.blob_threshold_mb",
              "promote.branch_template",
              "shadow_excludes",
              "blob_threshold_bytes",
          }
          with open("asp-config-diff.json", encoding="utf-8") as handle:
              report = json.load(handle)["result"]

          unsafe = [
              change for change in report["changes"]
              if change["field"] in unsafe_fields
          ]
          for change in unsafe:
              print(
                  "::error title=asp config drift::"
                  f"{change['field']} differs from .ci/asp/config.baseline.toml"
              )
          sys.exit(1 if unsafe else 0)
          PY
      - name: Upload asp config drift report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: asp-config-diff-${{ github.sha }}
          if-no-files-found: warn
          path: asp-config-diff.json
```

For GitLab:

```yaml
asp_config_drift:
  image: debian:bookworm-slim
  before_script:
    - apt-get update
    - apt-get install -y --no-install-recommends ca-certificates curl git python3
    - curl -fsSL https://raw.githubusercontent.com/ArnavBorkar/agentspaces/main/install.sh | sh
    - export PATH="$HOME/.local/bin:$PATH"
  script:
    - asp config validate
    - asp --json config diff --against .ci/asp/config.baseline.toml > asp-config-diff.json
    - |
      python3 - <<'PY'
      import json
      import sys

      unsafe_fields = {
          "capture.excludes",
          "capture.extra_excludes",
          "capture.blob_threshold_mb",
          "promote.branch_template",
          "shadow_excludes",
          "blob_threshold_bytes",
      }
      with open("asp-config-diff.json", encoding="utf-8") as handle:
          report = json.load(handle)["result"]
      unsafe = [change for change in report["changes"] if change["field"] in unsafe_fields]
      if unsafe:
          for change in unsafe:
              print(f"asp config drift: {change['field']} differs from baseline")
          sys.exit(1)
      PY
  artifacts:
    when: always
    expire_in: 14 days
    paths:
      - asp-config-diff.json
```

Both examples are read-only: `asp config validate` and `asp config diff` parse
configuration and write only the requested CI artifact. They do not run
`asp checkpoint`, `asp doctor --fix`, `asp restore`, `asp promote`, or
`asp discard`.

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

## Evidence Bundle Artifacts

Use this pattern when a failed readiness gate should leave a complete support
handoff: redacted evidence packet, packet manifest, verification log, and SARIF
files. The job still does not upload anything except normal CI artifacts.

```yaml
name: asp evidence bundle

on:
  workflow_dispatch:
  pull_request:

jobs:
  asp-evidence:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install asp
        run: |
          curl -fsSL https://raw.githubusercontent.com/ArnavBorkar/agentspaces/main/install.sh | sh
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - name: Run readiness checks
        run: |
          set +e
          asp config validate
          config_status=$?
          asp preflight --sarif > asp-preflight.sarif
          preflight_status=$?
          asp secrets scan --sarif > asp-secrets.sarif
          secrets_status=$?
          exit $((config_status || preflight_status || secrets_status))
      - name: Collect redacted evidence packet
        if: always()
        run: |
          asp evidence collect --audit-limit 50 --output asp-evidence.json
          asp evidence manifest \
            --packet asp-evidence.json \
            --output asp-evidence.manifest.json
          asp evidence verify \
            --packet asp-evidence.json \
            --manifest asp-evidence.manifest.json \
            > asp-evidence.verify.txt
      - name: Upload asp support evidence
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: asp-support-evidence-${{ github.sha }}
          if-no-files-found: warn
          path: |
            asp-evidence.json
            asp-evidence.manifest.json
            asp-evidence.verify.txt
            asp-preflight.sarif
            asp-secrets.sarif
```

Add a manifest signature artifact when your CI environment already signs build
outputs. Keep the signature next to `asp-evidence.manifest.json`; see
[Evidence Packets](evidence.md) for Sigstore and minisign commands.

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

## GitLab Evidence Bundle

```yaml
asp_evidence_bundle:
  image: debian:bookworm-slim
  before_script:
    - apt-get update
    - apt-get install -y --no-install-recommends ca-certificates curl git
    - curl -fsSL https://raw.githubusercontent.com/ArnavBorkar/agentspaces/main/install.sh | sh
    - export PATH="$HOME/.local/bin:$PATH"
  script:
    - asp config validate
    - asp preflight --sarif > asp-preflight.sarif || true
    - asp secrets scan --sarif > asp-secrets.sarif || true
    - asp evidence collect --audit-limit 50 --output asp-evidence.json
    - asp evidence manifest --packet asp-evidence.json --output asp-evidence.manifest.json
    - asp evidence verify --packet asp-evidence.json --manifest asp-evidence.manifest.json > asp-evidence.verify.txt
  artifacts:
    when: always
    expire_in: 14 days
    paths:
      - asp-evidence.json
      - asp-evidence.manifest.json
      - asp-evidence.verify.txt
      - asp-preflight.sarif
      - asp-secrets.sarif
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
- Upload the evidence packet, manifest, verification log, and SARIF files
  together when teams want persistent evidence for security or platform review.
- If `asp preflight` fails, use `asp doctor --runbook` and `asp secrets scan`
  locally to triage the finding.
