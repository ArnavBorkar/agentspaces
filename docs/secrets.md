# Secret Scanning

`asp secrets scan` checks workspace files that are normally in checkpoint scope
for common accidental secret inclusions before they land in local history.

```bash
asp secrets scan
asp --json secrets scan
asp secrets scan --sarif > asp-secrets.sarif
```

The scanner skips `.asp/`, `.git/`, symlinks, binary files, large files, and
asp's derived-state excludes by default. Pass `--include-excluded` to also scan
files under excluded paths such as `target/` or `node_modules/`.

Detected patterns include:

- private key headers;
- OpenAI-style `sk-...` keys;
- GitHub `ghp_...`, `gho_...`, `ghu_...`, `ghs_...`, and `ghr_...` tokens;
- AWS access key ids;
- generic assignments such as `password = ...`, `token: ...`, and
  `api_key = ...`.

Findings are redacted in both human and JSON output. A scan with findings exits
nonzero so teams can use it in pre-promotion checks:

```bash
asp secrets scan && asp checkpoint -m "reviewed"
```

`--sarif` emits raw SARIF 2.1.0 instead of the normal CLI JSON envelope. Each
finding becomes one result with a stable `secrets.<kind>` rule ID, a redacted
message, and a workspace-relative file and line location.

If a finding is real, remove it from the file, rotate the credential, and then
checkpoint again. The scanner is a guardrail, not a replacement for provider
revocation, repository secret scanning, or organization-wide policy.

To block known sensitive paths from being checkpointed at all, add
`paths.deny_checkpoint` rules in [.asp/policy.toml](policy.md).
