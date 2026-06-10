# Launch checklist (v0.1.0 soft launch)

*Working doc. Items marked (Arnav) need the founder; everything else is automatable.*

## Before flipping the repo public

- [x] CI green on macOS + Linux + btrfs reflink job
- [x] kill -9 torture suite in CI
- [x] Benchmarks published with methodology (docs/benchmarks/)
- [x] README: hero, quickstart, trust section, open-core declaration
- [x] Positioning docs: why-not-worktrees, why-not-agentfs, FAQ
- [x] LICENSE-MIT + LICENSE-APACHE, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT
- [x] install.sh (checksum-verified) + release workflow (4 targets)
- [ ] Tag v0.1.0 → verify release artifacts download & run on a clean machine
- [ ] Demo GIF/asciinema in README (record `asp race` on a real repo)
- [ ] (Arnav) Repo visibility flip private → public — exposes full history
- [ ] (Arnav) Decide crates.io publish (`asp` name availability) — `cargo install --git` works meanwhile

## Soft launch (week 4–5 per v1-brief)

- [ ] (Arnav) Recruit 5–10 Claude Code power users + 2–3 eval/fleet engineers as design partners
- [ ] Wire feedback channel (GitHub Discussions on + issue templates)
- [ ] (Arnav) X/HN draft: lead with the race demo + the worktree comparison numbers
- [ ] 90-day metrics instrumented per v1-brief (installs proxy: release download counts; stars; retention via design-partner check-ins)

## Known gaps accepted for v0.1.0 (documented in FAQ)

- Windows unsupported
- `asp race` requires the runner CLI (e.g. `claude`) on PATH; no built-in agent
- BYO-bucket sync: format is ready, feature is post-v1 (first milestone after launch)
- Codex/OpenCode port: named milestone ~week 6
