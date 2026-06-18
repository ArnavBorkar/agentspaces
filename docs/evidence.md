# Evidence Packets

`asp evidence collect` creates a local JSON packet for security review, support
handoffs, and incident timelines. It does not upload anything.

```bash
asp evidence collect
asp --json evidence collect
asp evidence collect --output asp-evidence.json
```

The packet includes:

- a redacted diagnostics bundle;
- a preflight readiness summary and check runbook links;
- the installed schema inventory;
- recent audit event timing and operation metadata.

By default, local paths are redacted and audit event `message` and `detail`
payloads are omitted. Secrets remain redacted even when `--include-paths` is
used.

```bash
asp evidence collect --audit-limit 50 --output asp-evidence.json
```

Use `--deep` when the packet should include deep preflight doctor checks such
as CAS verification. Use `--include-paths` only for a private support channel
where full local paths are acceptable.
