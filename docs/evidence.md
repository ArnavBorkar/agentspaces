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

## Review Checklist

Before sharing:

- confirm `redaction.paths_redacted` and `redaction.secrets_redacted` match the
  channel where the packet will be sent;
- keep `audit_messages_included` and `audit_details_included` false unless a
  private incident channel explicitly asks for richer context;
- run `asp preflight` and include SARIF artifacts when the reviewer needs CI
  annotations as well as the packet;
- do not attach fork directories, source archives, or `.asp/` backups unless a
  separate incident plan requires them.

During review:

- start with `preflight.ready` and the per-check runbooks;
- inspect `diagnostics.doctor_findings` for repairable workspace issues;
- compare `schema.asp_version` and schema paths with the installed binary under
  review;
- use `recent_audit_events` to establish operation order, timing, and checkpoint
  sequence without exposing free-form journal messages;
- ask for a fresh packet with `--deep` when CAS integrity or store health is in
  question.

Escalate only when the redacted packet cannot answer the question. If full paths
are necessary, rerun with `--include-paths` and share through a private support
channel with an explicit retention expectation.
