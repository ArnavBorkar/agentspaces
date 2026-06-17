# Release and public-readiness checklist

Use this checklist before public repo visibility changes, package publication,
or a tagged release. It is intentionally boring: every launch should prove the
trust model before it asks users to trust the binary.

## Required local gates

- [ ] `cargo build --workspace`
- [ ] `cargo test --workspace`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo deny check`
- [ ] `git diff --check`
- [ ] README headline claims are present in [claims.md](claims.md)
- [ ] New mutation paths have engine, integration, or torture coverage
- [ ] New CLI/MCP behavior has JSON output coverage where applicable
- [ ] User-facing errors include a corrective `hint`

## Required CI gates

- [ ] macOS workflow green
- [ ] Ubuntu workflow green
- [ ] Linux btrfs reflink job green
- [ ] Dependency audit workflow green or manually rerun if the scheduled run is stale
- [ ] Release workflow dry run or tagged release workflow green
- [ ] Checksums generated for every binary artifact
- [ ] Sigstore bundles generated for every checksum file
- [ ] GitHub provenance attestations generated for every release archive
- [ ] Downloaded release artifact smoke-tested outside the build tree

## Public repo readiness

- [ ] No internal strategy docs, private notes, or derivative-product framing
- [ ] No secrets, local machine paths, tokens, or private customer/user data
- [ ] `README.md` quickstart works from a fresh clone
- [ ] [architecture.md](architecture.md) matches the current crate/module layout
- [ ] [design/v1-brief.md](design/v1-brief.md) matches current product scope
- [ ] [SECURITY.md](../SECURITY.md) has the current reporting path and threat model
- [ ] [CONTRIBUTING.md](../CONTRIBUTING.md) has current build/test commands
- [ ] Issue templates are present for bugs and feature requests
- [ ] License files and crate license metadata agree

## Package publication readiness

- [ ] Version number bumped consistently across workspace crates
- [ ] `CHANGELOG.md` has the release date, user impact, and migration notes
- [ ] Install script points at the intended release tag or channel
- [ ] Package metadata is ready for crates.io/Homebrew/npm if publishing there
- [ ] Unsupported platforms fail with a helpful message
- [ ] Rollback plan is written: unpublish limits, yanked versions, replacement tag

## Founder or maintainer decisions

- [ ] Repo visibility change approved
- [ ] crates.io publication approved
- [ ] Release notes approved
- [ ] Launch post or announcement approved
- [ ] Support/triage window assigned for the first 48 hours

## Post-launch triage

- [ ] Watch CI on `main` after the public push
- [ ] Watch GitHub Issues and Discussions for install failures
- [ ] Pin or label duplicate reports quickly
- [ ] Add every confirmed launch gap to [BACKLOG.md](../BACKLOG.md)
- [ ] Cut a patch release for broken install, data-loss risk, or misleading docs
- [ ] Update [claims.md](claims.md) if any public claim is softened or expanded

## Stop-the-line conditions

Do not publish or flip visibility if any of these are true:

- crash-safety tests fail or are skipped;
- a storage mutation lacks a recoverability story;
- release artifacts cannot be verified by checksum;
- README quickstart fails on a clean clone;
- a known issue can lose checkpointed data;
- docs imply secrets are checkpointed or protected when they are not;
- a change requires rewriting user history or force-pushing user branches.
