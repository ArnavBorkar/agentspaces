# agentspaces — Product Backlog

*Living document. Updated continuously as tasks complete and learning arrives. Source of truth for scope is [docs/design/v1-brief.md](docs/design/v1-brief.md).*

**Status legend:** `[ ]` pending · `[~]` in progress · `[x]` done · `[-]` dropped (reason noted)

---

## EPIC 0 — Foundation

**PM intent:** A repo a stranger (or a future Claude session) can pick up cold: builds in one command, CI green on every push, conventions written down.
**Done when:** `cargo build` works from a fresh clone, CI runs fmt+clippy+tests on macOS+Linux, licenses and contributor docs exist.

- **S0.1 Toolchain & scaffold**
  - [x] T0.1.1 Install Rust stable toolchain (1.96.0)
  - [x] T0.1.2 Cargo workspace: `crates/asp-core` (engine lib), `crates/asp` (binary: CLI + `asp mcp` stdio server in one static binary)
  - [x] T0.1.3 rustfmt + clippy config, .gitignore
  - [x] T0.1.4 Dual-license MIT/Apache-2.0 files + per-crate license fields
  - [x] T0.1.5 CLAUDE.md: build/test commands, conventions, gh-account note
- **S0.2 CI**
  - [x] T0.2.1 GitHub Actions: fmt + clippy -D warnings + test on macos-latest & ubuntu-latest (green)
  - [x] T0.2.2 CI badge in README

## EPIC 1 — De-risk spikes (existential — nothing else proceeds if these fail)

**PM intent:** The two claims the whole pitch rests on, proven with published numbers before we invest in features: (1) whole-directory CoW fork + status is sub-second on a big dirty tree; (2) every byte is recoverable with stock git.
**Done when:** docs/benchmarks/spike-results.md has honest numbers from this machine, and a written go/reposition decision.

- **S1.1 CoW fork benchmark** ✅
  - [x] T1.1.1 Generator: synthetic monorepo (100,026 files, 3.28 GiB)
  - [x] T1.1.2 clonefile(dir) **919ms/32MB** vs cp -R 27s/3.7GB vs worktree 13.8s — 15x win
  - [x] T1.1.3 Change-detection scan: 263ms warm on 100k files
  - [x] T1.1.4 docs/benchmarks/spike-results.md — **verdict: GO**
- **S1.2 Shadow-git capture spike** ✅
  - [x] T1.2.1 Sidecar GIT_DIR captures untracked files; user .git untouched (PASS)
  - [x] T1.2.2 Incremental checkpoint **462ms**; initial 66s → mitigations decided (excludes, blob sidecar, capture-on-first-checkpoint)
  - [x] T1.2.3 Stock-git restore byte-identical (PASS); runbook = read-tree + checkout-index
  - [x] T1.2.4 Blob policy: >50MB → BLAKE3 CAS sidecar via clonefile + pointer in shadow git (6 format decisions recorded in spike-results.md)

## EPIC 2 — Core engine (`asp-core`)

**PM intent:** The trust-bearing layer. Boring, correct, crash-safe. Everything recoverable with stock git is the product's one-sentence trust model.
**Done when:** all engine ops have unit+integration tests, crash-recovery test passes, format doc matches implementation.

- **S2.1 Store layout & format doc** ✅ (format.md authoritative; ops behind Workspace API — backend swap = internal refactor)
- **S2.2 init / adopt** ✅ (guarded, never touches user .git; default derived-state excludes + config overrides)
- **S2.3 Checkpoint engine** ✅ (no-op skip handles hook storms; provenance metadata: source/session/tool; large-blob CAS sidecar w/ pointer manifests at refs/asp/meta/<seq>)
- **S2.4 Journal** ✅ (CRC lines, fsync, torn-tail self-heal; mid-file corruption surfaced via doctor, never dropped)
- **S2.5 Fork** ✅ (clonefile/FICLONE/copy-fallback; registry; fork-of-fork works; CoW independence tested)
- **S2.6 Timeline** ✅ (linear undo-stack semantics: restore appends safety + post checkpoints; dirty-undo vs clean-undo)
- **S2.7 Diff** ✅ (checkpoint↔checkpoint, checkpoint↔worktree, N-way fork compare; duration/test markers land with `asp race`, EPIC 3)
- **S2.8 Promote / discard** ✅ (plumbing-only commit + local fetch; no HEAD moves, no user hooks, no force-push; unpromoted-work guard)
- **S2.9 Crash safety** ✅ (advisory lock, atomic renames everywhere, doctor detects/repairs torn forks, tampered head, missing CAS blobs — kill -9 torture matrix lands in EPIC 6)

## EPIC 3 — CLI (`asp`)

**PM intent:** The first five minutes ARE the product. Every command: fast, beautiful output, `--json` for agents, errors that state the corrective next action.
**Done when:** the demo loop (`init → fork -n 3 → ... → diff → promote → discard`) feels great, help text teaches the model, all commands have --json.

- **S3.1 CLI scaffold** ✅ (clap; global --json with stable ok/result|error envelope; exit-code contract; hint: lines)
- **S3.2 Core commands** ✅ (init/status/checkpoint(cp)/log/undo/restore/fork/forks/diff/promote/discard)
- **S3.3 `asp race`** ✅ (parallel lanes, exit/time/±diff table, per-lane logs in fork/.asp/race.log, headless --json)
- **S3.4 Help & completions** — rich --help with examples ✅; shell completions/man page deferred post-v0.1 (clap_complete, low risk)

## EPIC 4 — MCP server

**PM intent:** The agent is a first-class user; MCP is the distribution channel (`claude mcp add agentspaces`). Tool descriptions are prompts — write them like product copy for models.
**Done when:** all workspace tools callable from Claude Code, descriptions tested with a real agent session, errors self-correcting.

- **S4.1 Server** ✅ (hand-rolled newline JSON-RPC 2.0: initialize/ping/tools-list/tools-call; zero deps)
- **S4.2 Tools** ✅ (11 workspace_* tools, model-facing descriptions, self-correcting errors, structuredContent)
  - [x] T4.2.2 Real-session test: live Claude Code session called workspace_status + workspace_checkpoint via .mcp.json, interpreted results (incl. no-op) correctly — PASS 2026-06-11

## EPIC 5 — Claude Code integration (hedged: contract stays harness-neutral)

**PM intent:** Auto-checkpoint around every agent change — the `/rewind`-for-everything experience, zero config after one command.
**Done when:** `asp hooks install` wires PostToolUse auto-checkpoints with session/prompt correlation; verified in a real session; uninstall clean.

- **S5.1 Hooks** ✅ (`asp setup claude`: PostToolUse file-tools+Bash, PreToolUse Bash; idempotent merge preserving user settings; --remove; hook-event always exits 0; session/tool provenance in journal)
- **S5.2 Packaging** ✅ (.mcp.json written by setup; Codex/OpenCode port = named post-v1 milestone)
  - [x] T5.2.2 Real-session verification: live headless Claude Code session → 'auto: after Edit' + 'auto: after Bash' checkpoints with session ids; `asp undo` reverted the agent edit — PASS 2026-06-11

## EPIC 6 — Quality gates (trust artifacts)

**PM intent:** Storage tools get one strike. The torture suite and honest benchmarks ARE marketing.
**Done when:** kill-9 suite green in CI; BENCHMARKS.md published with methodology + this-machine numbers; property tests on journal/store.

- **S6.1 Torture suite** ✅ (SIGKILL sweeps over checkpoint/fork/restore; 3 invariants verified; in CI, ~20s)
- **S6.2 Benchmarks** ✅ (scripts/bench/run.py reproducible markdown report; first run exposed 2 regressions → fixed: single-scan staging w/ no-op fast path + pathspec-limited add + post-capture repack)
- **S6.3 Cross-platform matrix** ✅ (btrfs loopback CI job exercises real FICLONE; macOS+ubuntu standard jobs)
- **S6.4 Property tests** ✅ (journal truncation-anywhere recovery, corruption-never-fabricates, checkpoint/restore round-trip over arbitrary trees)

## EPIC 7 — Docs, packaging, launch readiness

**PM intent:** Out-of-the-box: one install command, 90 seconds to wow. Positioning docs preempt the two obvious objections.
**Done when:** fresh-machine install → demo works following README only; release pipeline produces signed-ish artifacts; open-core boundary declared.

- **S7.1 README & demo** — hero/quickstart/trust/benchmarks/open-core ✅; demo GIF still to record (launch checklist)
- **S7.2 Positioning** ✅ (why-not-git-worktrees, why-not-agentfs, FAQ)
- **S7.3 Install paths** — install.sh (checksum-verified, no sudo) ✅; cargo install --git ✅; brew tap + npx wrapper deferred post-launch (need public repo + release assets first)
- **S7.4 Release automation** ✅ (tag → 4-target build incl. linux-arm, sha256, GitHub Release)
- **S7.5 OSS hygiene** ✅ (CONTRIBUTING w/ trust-model ground rules, SECURITY scope+model, CODE_OF_CONDUCT, open-core boundary in README); launch checklist at docs/launch-checklist.md (repo public flip = Arnav)

## EPIC 8 — Dogfood & final review

**PM intent:** We feel every rough edge before users do. agentspaces is built with agentspaces from the moment the alpha exists.
**Done when:** asp manages this repo's own development; first-five-minutes walkthrough passes on a clean simulated setup; adversarial multi-agent review finds no release-blocking issues.

- [x] T8.1 Dogfood: asp init on this repo (52 files, 255ms); managing remaining development
- [x] T8.2 Fresh-machine walkthrough: clean clone → release build → full README loop (init/checkpoint/fork/forks/undo/race/discard/doctor) — PASS 2026-06-11
- [x] T8.3 Adversarial review: 28 agents / 5 dimensions / 0 refuted → 4 unique criticals + 12 majors + 17 minors confirmed (docs/design/review-findings-v0.1.json); ALL criticals+majors fixed in 2 waves with regression tests (shrink data-loss, promote .asp leak, store path traversal, doctor rm -rf safety via Pending intent journaling, undo ping-pong, status big-file blindness, journal read/heal race, CAS TOCTOU, non-UTF8 filenames, musl static builds, git 2.32 gate, .env docs honesty, +more)
- [x] T8.4 Final gate PASSED: full suite + torture green locally and in CI (incl. btrfs reflink assert); **v0.1.0 tagged and released** — 4 targets built (macOS arm64/x86_64, Linux x86_64/aarch64 musl static), checksums verified, downloaded artifact smoke-tested end-to-end

## Enterprise adoption roadmap (100 concrete tasks)

**PM intent:** Turn the strong local engine into a high-adoption open-source project that teams can trust, operate, extend, and recommend. These are post-v0.1 tasks; each implementation task must include tests, docs when user-facing, and a commit pushed after validation.

## EPIC 9 — Public repo readiness and positioning

**Done when:** a new contributor can understand the project without internal strategy residue, competitor framing, or private-launch ambiguity.

- **S9.1 Public narrative**
  - [x] T9.1.1 Remove or rewrite internal market-research artifacts that make the repo read like a derivative project.
  - [x] T9.1.2 Keep the product brief focused on agentspaces principles, trust boundaries, and v1 scope.
  - [x] T9.1.3 Add a one-page architecture overview for first-time contributors.
- **S9.2 Launch hygiene**
  - [x] T9.2.1 Verify all README claims map to reproducible docs, tests, or benchmark scripts.
  - [x] T9.2.2 Add a release-readiness checklist for public visibility flips, package publication, and post-launch triage.

## EPIC 10 — Supply-chain and release trust

**Done when:** enterprise users can verify what they download, audit dependencies, and reproduce release artifacts.

- **S10.1 Artifact integrity**
  - [x] T10.1.1 Sign release checksums with cosign or minisign and document verification.
  - [x] T10.1.2 Publish provenance attestations for release artifacts.
  - [x] T10.1.3 Add an SBOM artifact to every release.
- **S10.2 Dependency governance**
  - [x] T10.2.1 Add cargo-deny for advisories, licenses, bans, and duplicate dependency policy.
  - [x] T10.2.2 Add a scheduled dependency audit workflow with clear maintainer runbooks.

## EPIC 11 — Installer and package distribution

**Done when:** users can install asp from standard channels without weakening the no-sudo, verifiable install path.

- **S11.1 Package channels**
  - [x] T11.1.1 Prepare crates.io package metadata, categories, keywords, crate READMEs, and README validation.
  - [x] T11.1.2 Create a Homebrew tap formula with checksum verification.
  - [x] T11.1.3 Add an npm/npx wrapper that downloads verified native binaries.
  - [ ] T11.1.4 Publish `asp-core` then `asp` to crates.io once a crates.io owner token is available.
  - [ ] T11.1.5 Publish/update the external Homebrew tap once the tap repository is available.
  - [ ] T11.1.6 Publish `@agentspaces/asp` to npm once the npm scope/token is available.
- **S11.2 Installer resilience**
  - [x] T11.2.1 Add installer tests for macOS arm64/x86_64 and Linux x86_64/aarch64 selection.
  - [x] T11.2.2 Add checksum failure, unsupported platform, and offline-mode diagnostics.

## EPIC 12 — Cross-harness integrations

**Done when:** agentspaces works naturally across major coding-agent harnesses through explicit setup commands and tested docs.

- **S12.1 Codex integration**
  - [x] T12.1.1 Add `asp setup codex` that writes documented MCP/config entries without clobbering user settings.
  - [x] T12.1.2 Add hook or wrapper guidance for Codex file and shell checkpoints where supported.
  - [x] T12.1.3 Add an end-to-end Codex setup smoke test using a temporary HOME.
- **S12.2 Other harnesses**
  - [x] T12.2.1 Add `asp setup opencode` with idempotent install/remove behavior.
  - [x] T12.2.2 Add generic MCP client instructions and schema examples for unsupported harnesses.

## EPIC 13 — MCP maturity

**Done when:** the MCP server is discoverable, well-specified, robust under bad clients, and pleasant for models.

- **S13.1 Protocol completeness**
  - [x] T13.1.1 Add MCP capability metadata, server version, and tool schema snapshots.
  - [x] T13.1.2 Add tests for malformed JSON-RPC, unknown ids, bad params, and partial lines.
  - [x] T13.1.3 Add stable error-code documentation for all MCP tools.
- **S13.2 Model ergonomics**
  - [x] T13.2.1 Add tool descriptions with examples of when not to call destructive operations.
  - [x] T13.2.2 Add a replayable transcript test that asserts model-facing outputs stay concise and actionable.

## EPIC 14 — Team policy and governance

**Done when:** teams can encode local workspace rules without a hosted control plane.

- **S14.1 Policy file**
  - [x] T14.1.1 Add `.asp/policy.toml` with schema, validation, and helpful errors.
  - [x] T14.1.2 Support policy for max fork count, max checkpoint age, protected paths, and promote requirements.
  - [x] T14.1.3 Add `asp policy validate --json`.
- **S14.2 Enforcement**
  - [x] T14.2.1 Enforce protected path prompts or hard blocks for promote and restore.
  - [x] T14.2.2 Add tests proving invalid policy cannot make destructive operations less safe.

## EPIC 15 — Audit, retention, and compliance exports

**Done when:** a team can answer who changed what, when, with which tool, and export evidence without a SaaS dependency.

- **S15.1 Audit views**
  - [x] T15.1.1 Add `asp audit` with filters for session, tool, operation, path, and time range.
  - [x] T15.1.2 Add JSONL and CSV export formats for audit events.
  - [x] T15.1.3 Add checkpoint-to-path attribution for changed files in audit output.
- **S15.2 Retention**
  - [x] T15.2.1 Add configurable checkpoint retention policies with dry-run output.
  - [x] T15.2.2 Add tests proving retention never removes reachable promoted or recovery-required data.

## EPIC 16 — Large repository performance

**Done when:** performance claims remain honest and improve for monorepos with many files, large blobs, and hook storms.

- **S16.1 Measurement**
  - [x] T16.1.1 Add benchmark baselines to CI as non-blocking trend artifacts.
  - [x] T16.1.2 Add benchmark fixtures for many small files, large binaries, deep trees, and rename-heavy workloads.
  - [x] T16.1.3 Add a `asp bench self` command that reports local filesystem capabilities.
- **S16.2 Optimizations**
  - [x] T16.2.1 Add a persistent file-state index guarded by crash-safe writes.
  - [x] T16.2.2 Add regression tests for no-op checkpoint latency and changed-path staging behavior.

## EPIC 17 — Diff and review excellence

**Done when:** humans can compare agent attempts quickly enough that best-of-N becomes a daily workflow.

- **S17.1 Better summaries**
  - [x] T17.1.1 Add path-grouped diff summaries with counts by language and change type.
  - [x] T17.1.2 Add fork comparison scoring hooks for tests passed, files touched, and risk markers.
  - [x] T17.1.3 Add JSON output suitable for dashboards and CI comments.
- **S17.2 Review artifacts**
  - [x] T17.2.1 Add `asp diff --patch` and `--stat` modes for checkpoint and fork comparisons.
  - [x] T17.2.2 Add HTML diff export for offline review.

## EPIC 18 — Promote-to-PR workflow

**Done when:** a winning fork can become a normal reviewable branch or draft PR with minimal friction and no history surprises.

- **S18.1 Branch polish**
  - [x] T18.1.1 Add configurable branch naming templates for promote.
  - [x] T18.1.2 Add promote output that clearly states the fork directory remains on disk and how to clean it.
  - [x] T18.1.3 Add tests for protected branch-name collisions and unsafe ref names.
- **S18.2 GitHub flow**
  - [x] T18.2.1 Add `asp promote --push` with explicit remote/branch confirmation and JSON output.
  - [x] T18.2.2 Add `asp promote --pr-draft` using gh when available, with graceful fallback instructions.

## EPIC 19 — Doctor and self-healing

**Done when:** `asp doctor` is the trusted first stop for every broken workspace report.

- **S19.1 Detection**
  - [x] T19.1.1 Add checks for git availability/version and shadow-git config drift.
  - [x] T19.1.2 Add optional deep CAS verification that re-hashes sidecar blobs.
  - [x] T19.1.3 Add checks for orphan fork directories and promoted fork cleanup candidates.
- **S19.2 Repair UX**
  - [x] T19.2.1 Add `asp doctor --explain` with human-readable cause and next action per finding.
  - [x] T19.2.2 Add JSON repair plans before applying `--fix`.

## EPIC 20 — BYO-bucket sync foundation

**Done when:** the local format can sync to user-owned object storage without custody, hidden telemetry, or lock-in.

- **S20.1 Sync design**
  - [x] T20.1.1 Write the sync protocol design for immutable objects, CAS blobs, refs, and conflict handling.
  - [x] T20.1.2 Add a local filesystem remote implementation for deterministic tests.
  - [x] T20.1.3 Add conditional-write semantics to the remote trait.
- **S20.2 First sync command**
  - [x] T20.2.1 Add `asp sync push` for checkpoints and blobs to a local remote.
  - [x] T20.2.2 Add `asp sync fetch` that restores refs without overwriting newer local state.

## EPIC 21 — Security hardening

**Done when:** threat models are documented, tests cover known file-system attacks, and security-sensitive behavior fails closed.

- **S21.1 Threat coverage**
  - [x] T21.1.1 Expand SECURITY.md with threat model diagrams and non-goals.
  - [x] T21.1.2 Add symlink and hardlink attack regression tests around store paths and fork cleanup.
  - [x] T21.1.3 Add fuzzing harnesses for config, journal, MCP params, and hook payload parsing.
- **S21.2 Secret safety**
  - [x] T21.2.1 Add `asp secrets scan` for common accidental checkpoint inclusions.
  - [x] T21.2.2 Add configurable deny patterns that block checkpoint with a corrective hint.

## EPIC 22 — Observability without telemetry

**Done when:** users can inspect local health and performance while the project remains no-phone-home.

- **S22.1 Local metrics**
  - [x] T22.1.1 Add `asp stats` for store size, checkpoint count, fork count, blob count, and last operation timings.
  - [x] T22.1.2 Add `asp stats --json` for scripts and CI.
  - [x] T22.1.3 Add per-command timing fields to journal entries where missing.
- **S22.2 Diagnostics bundles**
  - [x] T22.2.1 Add `asp diagnostics` that redacts paths/secrets by default.
  - [x] T22.2.2 Add docs for attaching diagnostics to issues safely.

## EPIC 23 — Windows and filesystem portability

**Done when:** unsupported platforms fail kindly today and Windows has a tested path to support.

- **S23.1 Unsupported-platform UX**
  - [x] T23.1.1 Improve Windows error messages with current limitations and tracking issue links.
  - [x] T23.1.2 Add CI that ensures Windows builds either pass or fail with intentional cfg gates.
  - [x] T23.1.3 Document filesystem feature detection across APFS, btrfs, XFS, ext4, tmpfs, and network filesystems.
- **S23.2 Windows plan**
  - [x] T23.2.1 Spike block cloning and copy fallback behavior on ReFS/NTFS.
  - [x] T23.2.2 Write a Windows support design note covering symlinks, permissions, and git behavior.

## EPIC 24 — Contributor experience

**Done when:** outside contributors can find good first issues, run the right tests, and avoid trust-model mistakes.

- **S24.1 Development docs**
  - [x] T24.1.1 Add `docs/development.md` with architecture map, command map, and test guide.
  - [x] T24.1.2 Add module-level ownership notes for `asp-core` vs CLI/MCP code.
  - [x] T24.1.3 Add a PR checklist aligned with crash safety, JSON output, hints, and docs.
- **S24.2 Issue flow**
  - [x] T24.2.1 Add issue forms for performance reports, crash-safety bugs, integration requests, and docs fixes.
  - [x] T24.2.2 Add labels and triage docs for maintainers.

## EPIC 25 — Config and schema stability

**Done when:** config, policy, JSON output, and on-disk data have schemas that automation can trust.

- **S25.1 Schemas**
  - [x] T25.1.1 Publish JSON Schemas for CLI JSON envelopes and MCP tool results.
  - [x] T25.1.2 Publish TOML schema docs for `.asp/config.toml`.
  - [x] T25.1.3 Add snapshot tests that prevent accidental JSON shape drift.
- **S25.2 Versioning**
  - [x] T25.2.1 Add `asp schema` command to print supported schema versions.
  - [x] T25.2.2 Add changelog rules for breaking vs additive JSON changes.

## EPIC 26 — Race workflow upgrades

**Done when:** `asp race` supports real best-of-N agent work with resumability, clear logs, and safe cleanup.

- **S26.1 Runner controls**
  - [x] T26.1.1 Add per-lane environment variable templates and labels.
  - [x] T26.1.2 Add timeout, retry, and cancellation behavior with tests.
  - [x] T26.1.3 Add resumable race metadata for interrupted runs.
- **S26.2 Result quality**
  - [x] T26.2.1 Add structured per-lane test result ingestion from common formats.
  - [x] T26.2.2 Add `asp race compare` to re-rank existing lanes after manual inspection.

## EPIC 27 — Enterprise docs and adoption playbooks

**Done when:** teams can evaluate agentspaces with clear workflows, risks, and migration paths.

- **S27.1 Evaluation guides**
  - [x] T27.1.1 Add a 30-minute team evaluation guide with success criteria.
  - [x] T27.1.2 Add playbooks for bug-fix fleets, test-generation races, docs generation, and CI repair.
  - [x] T27.1.3 Add a trust-model whitepaper suitable for security review.
- **S27.2 Operations docs**
  - [x] T27.2.1 Add backup and disaster-recovery docs for `.asp/`.
  - [x] T27.2.2 Add monorepo tuning docs for excludes, blob thresholds, and filesystem choice.

## EPIC 28 — Hosted-adjacent optional services boundary

**Done when:** future hosted work is clearly additive and cannot weaken the open-source local engine.

- **S28.1 Boundary contracts**
  - [x] T28.1.1 Write an open-core boundary policy with non-negotiable OSS guarantees.
  - [x] T28.1.2 Add a governance note for features that must remain in the local engine.
  - [x] T28.1.3 Add design constraints for any future control plane: zero custody by default, opt-in sync, exportability.
- **S28.2 Team features on paper**
  - [x] T28.2.1 Draft team audit, policy, and approval workflows that can run locally first.
  - [x] T28.2.2 Draft enterprise support/SLA boundaries without adding telemetry or mandatory accounts.

## EPIC 29 — CLI polish and operator ergonomics

**Done when:** daily users can install, discover, script, and operate `asp` from normal shells and runbooks without memorizing flags.

- **S29.1 Shell integration**
  - [x] T29.1.1 Add `asp completions <shell>` with JSON output and install docs.
  - [x] T29.1.2 Add generated manpage artifacts or a documented manpage generation command.
  - [x] T29.1.3 Add a command cheat sheet organized by daily workflows.
- **S29.2 Guided workflows**
  - [x] T29.2.1 Add `asp quickstart` to print the safest first-five-minutes flow for the current directory.
  - [x] T29.2.2 Add `asp doctor --runbook` links for common repair scenarios.

---

## EPIC 30 — Configuration visibility and rollout safety

**Done when:** teams can inspect, validate, review, and roll out workspace configuration changes without guessing which defaults are active.

- **S30.1 Config inspection**
  - [x] T30.1.1 Add `asp config show` with human and JSON output for effective settings.
  - [x] T30.1.2 Add `asp config validate` as a non-mutating CI-friendly check that does not require other workspace reads.
  - [x] T30.1.3 Add config review guidance for security and platform teams.
- **S30.2 Safe rollout templates**
  - [x] T30.2.1 Add example config templates for monorepos, media-heavy repos, and generated-code repos.
  - [x] T30.2.2 Add docs for coordinating `.gitignore`, `.asp/config.toml`, and secrets policy.

---

## EPIC 31 — CI readiness and automation gates

**Done when:** teams can add asp to CI and agent launch flows with one clear readiness signal and documented non-mutating checks.

- **S31.1 Readiness command**
  - [x] T31.1.1 Add `asp preflight` with human and JSON output for config, policy, doctor, and secrets checks.
  - [x] T31.1.2 Add GitHub Actions and GitLab CI examples for preflight gates.
  - [x] T31.1.3 Add preflight docs for agent harness launch checks before long-running work.
- **S31.2 Failure triage**
  - [x] T31.2.1 Add preflight-to-runbook links for each failing check.
  - [x] T31.2.2 Add machine-readable preflight check IDs for stable CI annotations.

---

## EPIC 32 — CI evidence exports and security dashboards

**Done when:** teams can route asp readiness, security, and audit evidence into standard CI artifacts and dashboards without adopting a hosted service.

- **S32.1 Security dashboard exports**
  - [x] T32.1.1 Add `asp preflight --sarif` for failed readiness checks with redacted secret locations.
  - [x] T32.1.2 Add `asp secrets scan --sarif` for direct secret-scan upload workflows.
  - [ ] T32.1.3 Add GitLab Code Quality and generic SARIF artifact examples.
- **S32.2 Local evidence packets**
  - [ ] T32.2.1 Add `asp evidence collect` to bundle redacted diagnostics, preflight, schema, and recent audit events.
  - [ ] T32.2.2 Add a signed local manifest for evidence bundles using existing release-trust tooling.
  - [ ] T32.2.3 Add an evidence review checklist for security and platform teams.

---

## Decision log (newest first)

- 2026-06-11 · Doctor never deletes directories it can't prove asp created — fork() registers Pending intent entries before cloning; cleanup is registry-driven, heuristics are info-only. (From review: rm -rf of `cp -r proj proj@backup` was possible.)
- 2026-06-11 · Gitignored files are excluded from checkpoints BY DESIGN (secrets stay out of the store) and the docs say so honestly; forks carry everything physically.
- 2026-06-11 · Linux releases are musl static; macOS releases are the two Apple targets.

- 2026-06-11 · Single binary `asp` contains CLI + MCP server (`asp mcp`) — one artifact to install, per first-five-minutes PM goal.
- 2026-06-11 · Journal = append-only JSONL w/ per-line CRC, not SQLite — simpler crash-safety story, greppable, zero deps. Revisit if query needs grow.
- 2026-06-11 · Engine backend = git plumbing subprocess behind `Workspace` trait for v1 — stock-git recoverability is the trust model; gitoxide/custom CAS only when profiling demands.
- 2026-06-10 · Locked founder forks: venture path · 12+ mo runway · MIT/Apache open core · Claude-first hedged · zero custody (see v1-brief).
