# The State Layer for AI Agents

## Archil, Mesa, and Code Storage — company research, 50 agent use cases, infra-paradigm comparison, and a proposed universal primitive

*Prepared June 10, 2026. Facts verified against primary sources where possible; vendor performance claims are flagged as such. All three companies were researched via their sites, docs, founder statements (HN), investor posts, and third-party coverage.*

---

## 1. Executive summary

Archil, Mesa, and Code Storage are three expressions of the same bet: **compute for agents is commoditizing; durable state is the layer that's missing and will be the layer that matters.** The agent stack has stratified — models handle intelligence, harnesses (Claude Code, Codex, OpenCode) handle orchestration, sandboxes (E2B, Modal, Daytona) handle isolation — and each of these companies is building the fourth layer, storage, with a different interface to it:

- **Archil** sells a *POSIX cloud filesystem*: mount an S3 bucket (or an elastic disk) as a local filesystem anywhere — EC2, Kubernetes, E2B, Modal, a laptop — with sub-millisecond cached reads, plus serverless compute (`disk.exec`) attached directly to the disk.
- **Mesa** sells a *versioned filesystem*: every write is a version-controlled change (engine built on Jujutsu), giving agents instant forks, checkpoints, rollbacks, and human-approval gates as native filesystem semantics.
- **Code Storage** (Pierre Computer Company) sells *headless Git infrastructure for machines*: GitHub's architecture re-built API-first, with no rate limits, repo-per-user-session economics, ephemeral branches, and warm/cold tiering.

Section 3 enumerates 50 popular agent use cases and their infrastructure footprints. Section 4 compares six infra paradigms from a system-design perspective. The headline finding: roughly four-fifths of the 50 use cases are best served by **durable, branchable, shared state attached to disposable compute** — not by ephemeral sandboxes alone (state dies), not by persistent VMs alone (state is trapped in a machine), and not by machine snapshots alone (state is opaque and non-mergeable). Section 5 develops the primitive this implies: **the branchable agent workspace** — a durable, version-native, lazily-materialized file tree, mountable into any compute substrate, with execution attached as a function rather than a place, and security scoped to the data rather than the box. State is the noun; compute is a verb; branches are control flow; commits are the audit log.

---

## 2. The three companies

### 2.1 Archil — "the file system your agents run on"

**What it is.** Archil (formerly Regatta Storage; renamed by mid-2025) mounts cloud object storage as an "infinite, local" POSIX filesystem. The S3 bucket stays the source of truth in native object format; Archil sits between client and bucket as a managed, multi-AZ-replicated NVMe caching layer that synchronizes bidirectionally ([archil.com](https://archil.com/), [architecture docs](https://docs.archil.com/details/architecture.md)). It claims full POSIX, including the parts object-storage adapters skip: atomic renames, hard/symlinks, fsync, file locking, sparse files, mmap. For contrast, AWS's own Mountpoint for S3 deliberately supports none of those ([Mountpoint semantics](https://github.com/awslabs/mountpoint-s3/blob/main/doc/SEMANTICS.md)) — that gap is Archil's wedge.

**Architecture.** Three tiers: client (FUSE-based mount, Kubernetes CSI, macOS FSKit app, or in-process SDK) → Archil's durable cache fleet → your bucket. Writes are acknowledged after multi-AZ replication in the cache (99.999% durability pre-sync), then flushed asynchronously to S3, typically within ~1–5 minutes. Consistency is strong read-after-write *between Archil clients* and eventual *with the bucket* ([consistency docs](https://docs.archil.com/details/consistency.md)). The company originally mounted over NFS; after a "10-month company-wide bet" it shipped a proprietary protocol (GA Sept 16, 2025) whose key trick is client-held write delegations over subtrees — creates and writes acknowledge locally without server round-trips, and a single client fans out to multiple storage servers, Lustre-style ([Show HN](https://news.ycombinator.com/item?id=45264956), [comparison docs](https://docs.archil.com/details/comparison.md)). Verified performance claims from docs: sub-millisecond time-to-first-byte on cached ops, 10–30 ms on uncached S3 reads, ~100 ms mounts, default 10 GB/s and 10K IOPS per disk ([performance docs](https://docs.archil.com/details/performance.md)).

**The two agent-native features.**
1. **Branches and checkpoints**: immutable named snapshots plus independent writable forks, forming a git-like tree; billing counts only unique data per branch (100 GiB parent + 50 branches with ~5 GiB unique each bills ≈ 350 GiB, not 5,100 GiB) ([branches docs](https://docs.archil.com/concepts/branches-and-checkpoints.md), [metering](https://docs.archil.com/concepts/metering.md)). One important verified limitation: a disk can sync to S3 *or* support branches — not both.
2. **Serverless execution**: `disk.exec("cmd")` runs bash/python/node in a managed container with the disk mounted, billed per active millisecond (100 ms minimum), with `disk.grep` for server-side fan-out search ([serverless execution docs](https://docs.archil.com/compute/serverless-execution.md)). This makes Archil the only one of the three that bundles compute: "skip the sandbox" is an explicit pitch.

**Positioning.** The homepage thesis is a direct inversion of the sandbox industry: "Sandbox providers treat compute as the primitive — every run starts amnesic. Archil inverts it: the file system is the resource." The deeper argument, from the Series A post: file systems are the best interface for agents "because their representation in the training data means that most models inherently know how to work with files and folders" ([Series A post](https://archil.com/post/series-a)).

**Company.** Sole founder Hunter Leath spent ~9–10 years in cloud storage across Amazon EFS (founding engineer, later senior PM) and Netflix's core storage team ([YC profile](https://www.ycombinator.com/companies/archil), [Felicis](https://www.felicis.com/insight/investing-in-archil)). YC Fall 2024; ~11 people. **Funding: $6.7M seed led by Felicis (June 2025; YC, Peak XV, General Catalyst, Wayfinder + angels from Modal, WarpStream, T3), then an $11M Series A led by Standard Capital (~April 2026) — ~$18M total, raised less than a year after the seed.** Named early customers/partners: Depot, Fly.io (per Felicis), Antithesis, Clay (per Archil). Pricing: $0.20/GiB-month metered only on *active* (cached) data — data falls out of billing ~1 hour after last access; no egress or per-request fees ([billing docs](https://docs.archil.com/administration/billing.md)).

**Startup-lens risks.** Single-founder organization; a proprietary closed protocol that asks for kernel-adjacent trust; AWS incumbency (EFS, and Archil's own docs now benchmark against an "Amazon S3 Files" offering); the cost gravity of operating an NVMe cache fleet; and thin public case studies relative to the claims. The branch-vs-S3-sync exclusivity also splits its two stories ("your bucket, faster" vs "agent state with forks") into two different disk types.

### 2.2 Mesa — the versioned filesystem for agents

**What it is.** Mesa (Mesa Systems, Inc., mesa.dev) is a durable, POSIX-compatible cloud filesystem in which **version control is a property of the filesystem itself**. Agents mount a repo as a directory (FUSE on Linux/macOS, or an in-process virtual filesystem via the TypeScript SDK paired with Vercel's `just-bash` — no FUSE, no container, no clone); files materialize on demand; every write is durable and versioned on return ([mesa.dev](https://www.mesa.dev/), [VFS docs](https://docs.mesa.dev/content/core-concepts/virtual-filesystem)).

**Architecture.** The engine is **built on Jujutsu (jj)**, with a Git translation layer so standard `git clone/push` works against the same repos ([git-server page](https://www.mesa.dev/features/git-server)). The primitives are jj-like: a DAG of *changes* referenced by *bookmarks*; no staging area; conflicts are non-blocking (a change can sit conflicted without stopping an agent). The most opinionated design choice is the write-safety model: mounts open in "observe mode," and **the first write automatically forks a new change — the mounted bookmark never moves implicitly**. Publishing is an explicit bookmark move, which is exactly a human-approval gate expressed as a storage operation ([versioning docs](https://docs.mesa.dev/content/core-concepts/versioning)). Marketed numbers (vendor-stated, unverified): sub-50ms p95 random reads on a 10 GB file, <1s to "mount" a 10 GB repo vs minutes to clone it, millisecond forks, unlimited concurrent writers.

**Positioning.** Mesa names the stack explicitly: "Model providers handle intelligence. Harnesses handle orchestration. Sandboxes handle isolation. The missing layer is storage." And it attacks both incumbent options: S3 gives durability without version semantics ("concurrent writes clobber each other silently"); GitHub gives version semantics but rate limits and an org model that can't do repo-per-agent; raw git imposes a clone latency tax. "Your agents don't want to git clone and git push, they just want to read and write files" ([launch post](https://www.mesa.dev/blog/introducing-mesa-filesystem-for-agents)). It ships integration guides for Daytona, E2B, Modal, Blaxel, Cloudflare, Freestyle, Sprites, and Vercel — storage that mounts *into* everyone's sandbox.

**Company.** Founders: Oliver Gilan (CEO; co-founded Antimetal; ex-Census, Microsoft) and Ben Warren (CTO; co-founder/CTO of Snowpilot, YC S24; ex-Census, Microsoft); South Park Commons portfolio; team with Microsoft/AWS/Google/Tesla/Bun alumni ([about](https://www.mesa.dev/about)). **Funding: ~$5M seed led by Innovation Endeavors (announced Nov 3, 2025), with Essence VC, South Park Commons, Thomas Wolf (Hugging Face CSO), and Soleio.** History matters here: Mesa launched publicly on **Nov 3–4, 2025 as a multi-agent AI code-review product**, then announced the filesystem on **April 28, 2026** — a pivot (or at minimum a radical refocus) about six months after the seed, with the filesystem still in private beta behind a waitlist. Pricing: free 50 GB tier; then $0.18/GB-month storage and $0.11/GB egress charged only on Git/REST paths — **virtual-filesystem reads carry zero egress**, a pricing structure designed to make the mount, not the clone, the default access path ([pricing](https://www.mesa.dev/pricing)).

**Startup-lens risks.** Earliest-stage of the three and pre-GA, so every capability claim is unverifiable externally; minimal public traction signals (its HN launches drew single-digit points); a recent pivot; a crowded 2026 category (Turso's AgentFS, Cloudflare's versioned-storage work, Freestyle's git-as-filesystem — Freestyle being, awkwardly, also a listed integration partner); and the jj bet is technically elegant but unproven at agent-fleet scale. Counterweight: of the three, Mesa's semantics map most directly onto what agent products actually need (forks, approval gates, audit, rollback), and its design partners reportedly span legal, healthcare, GTM, and coding agents.

### 2.3 Code Storage — Git infrastructure for machines

**What it is.** Code Storage (code.storage), by Pierre Computer Company, is **white-label, API-first hosted Git**: "think GitHub's infrastructure layer, but API-first and tuned for LLMs… like what Stripe does for payments" (CEO, [HN](https://news.ycombinator.com/item?id=46957629)). Platforms — especially app-builder/codegen companies — programmatically create repos at user-session scale, expose `git clone` under their own domain, and get agent-native operations: `createCommit` and `createCommitFromDiff` (a 10-file LLM patch is one API call vs ~15 GitHub API requests), server-side grep, restore/reset for rollbacks, branch-scoped JWTs, GitHub/GitLab sync, and **ephemeral branches** — isolated disposable ref namespaces that never sync upstream, share underlying objects, and can be promoted into the real namespace when an agent's work is accepted ([docs](https://code.storage/docs/getting-started/introduction.md), [ephemeral branches](https://code.storage/changelog/ephemeral-branches)).

**Architecture.** A distributed, quorum-based Git layer: sharded replicated ref storage (3+ replicas), built by "speed-running GitHub's infrastructure (with a lot of help from early GitHub folks)" — GitHub's spoke architecture plus an object store as a cold tier. Repos untouched for 7 days are compressed into cold storage automatically. Claims (vendor-stated): clones 60x faster than S3/R2-backed git layers, "millions of new repos a day," 99.99% multi-AZ SLA ([code.storage](https://code.storage/), [launch post](https://code.storage/changelog/introducing-code-storage)). Notably, there is **no mount layer**: access is Git protocol, REST, and SDKs (TS/Python/Go). It is Git-as-a-database, not a filesystem.

**Positioning.** The pitch is anti-dependency as much as pro-performance: codegen platforms hitting GitHub rate limits, and "fear of Microsoft shutting off API access." Git is framed as the right state format for agents because it is deterministic, content-addressed, pinnable in prompts and evals, and natively diffable for human-agent collaboration. Sandbox guides exist for Modal, E2B, and Daytona — clone in, work, push out; sandboxes are complements. Target buyer is the platform, not the end user: "end users won't really know we exist" — and usage already extends beyond code ("folks using us to back CRMs, design tools").

**Company.** Pierre Computer Company: YC W23, founded 2023, ~10–11 people, San Francisco. Founders **Jacob Thornton** (CEO; co-creator of Twitter Bootstrap; early Twitter, Medium, Coinbase) and **Ian Ownbey** (CTO; early Twitter, Coinbase). The company spent ~3 years building a full GitHub competitor (pierre.co, now "RIP 2023–2026"), found codegen companies asking for its git-scaling internals, and pivoted to selling the infrastructure (announced Oct 14, 2025). **Funding: ~$23M total; latest round led by CRV + "O1A" (almost certainly 01 Advisors); the careers page additionally names Sequoia (uncorroborated elsewhere); PitchBook adds Audacious, Rogue Capital, Wayfinder.** All funding disclosure is self-published — no press coverage exists. An investor (Davis Treybig) wrote in April 2026 that "a few of the major vibe coding startups now use code.storage as their headless git storage layer" ([The Two Software Development Stacks](https://davistreybig.substack.com/p/the-two-software-development-stacks)).

**Startup-lens risks.** Pricing drew sharp HN criticism ($1.00/GB-month *per replica with a 3-replica minimum* for hot data plus ingress fees — ~$36/GB-year hot, vs $0.15/GB-month cold); customer concentration in a handful of large codegen platforms; GitHub remains both the sync target and the incumbent that could ship agent-native APIs; and the interface choice cuts against the "agents just want files" thesis that both Archil and Mesa champion — the clone tax is pushed to the client, partially offset by server-side operations. Counterweight: strongest team pedigree and capitalization of the three, the deepest moat (years of distributed-git systems work), and a wedge directly into the highest-revenue agent category.

### 2.4 Side-by-side

| | **Archil** | **Mesa** | **Code Storage (Pierre)** |
|---|---|---|---|
| Core primitive | POSIX filesystem over object storage | Versioned filesystem (Jujutsu engine) | Headless Git repos at machine scale |
| Interface | Mount (FUSE/CSI/macOS), SDK VFS, `disk.exec`, read-only S3 API | Mount (FUSE), SDK VFS (+just-bash), Git server | Git protocol, REST/SDK, server-side ops — no mount |
| Source of truth | Your S3 bucket (or Archil disk) | Mesa repos (GitHub sync available) | Git repos, 3+ replicas, object-store cold tier |
| Branching | Checkpoints + writable forks; **not on S3-synced disks** | First-class changes/bookmarks; fork-on-first-write; non-blocking conflicts | Ephemeral branch namespaces; promote on accept |
| Compute | **Yes** — serverless exec billed per active ms | No — mounts into partner sandboxes | No — clone into partner sandboxes |
| Consistency | Strong between clients; eventual with bucket | Strong (claimed); durable-on-return writes | Git semantics; quorum replication |
| Pricing shape | $0.20/GiB-mo on *active* data only | $0.18/GB-mo; zero egress via VFS | $1/GB-mo ×3 replicas hot; $0.15 cold; ingress+egress fees |
| Stage | GA; custom protocol since Sept 2025 | Private beta (Apr 2026) | Private-beta→platform deals |
| Funding | ~$18M (Felicis seed; Standard Capital A) | ~$5M (Innovation Endeavors) | ~$23M (CRV + O1A; self-reported) |
| Founder edge | 9–10 yrs AWS EFS/Netflix storage | Census/Microsoft; Antimetal & Snowpilot (YC S24) founders | Bootstrap co-creator + early Twitter; ex-GitHub help |
| Sharpest risk | Single founder; proprietary protocol; AWS | Pre-GA, pivot, crowded category | Pricing friction; no file interface; GitHub |

**Convergence and divergence.** All three agree on the diagnosis (sandboxes made compute ephemeral but left state homeless), agree that version semantics and forking are agent-native requirements, and agree on object-storage economics underneath. They diverge on the *interface*: Archil bets agents want **files with maximum fidelity and speed** (POSIX everywhere, compute attached); Mesa bets agents want **files with maximum semantics** (every write versioned, approval gates in the storage layer); Pierre bets platforms want **Git itself, industrialized** (interop with the entire human SDLC for free). These are not mutually exclusive — and Section 5 argues the end-state primitive is effectively the union.

---

## 3. Fifty popular agent use cases and their infrastructure footprints

Grounding, before the list. Agents are mainstream: 57.3% of surveyed organizations had agents in production by December 2025, with customer service (26.5%) and research/data analysis (24.4%) the top deployments ([LangChain State of Agent Engineering](https://www.langchain.com/state-of-agent-engineering), n=1,340). Coding is the revenue outlier "by nearly an order of magnitude" among enterprise use cases ([a16z, Apr 2026](https://a16z.com/where-enterprises-are-actually-adopting-ai/)). Anthropic's API traffic is ~75% automation-shaped, with office/admin work (email, document processing, CRM, scheduling) the fastest-rising category at 13% ([Anthropic Economic Index, Jan 2026](https://www.anthropic.com/research/anthropic-economic-index-january-2026-report)). And the execution substrate is industrial: E2B reports hundreds of millions of sandbox sessions and a $21M Series A; Modal advertises 1B+ sandboxes run, with Lovable running tens of thousands of simultaneous app-creation sessions ([modal.com](https://modal.com/products/sandboxes), [VentureBeat](https://venturebeat.com/ai/how-e2b-became-essential-to-88-of-fortune-100-companies-and-raised-21-million)).

**Footprint key** — **E**: code execution · **D**: durable state across runs · **S**: files shared across agents/runs · **F**: fork/parallel exploration · **B**: browser · **G**: GPU · **H**: human-in-the-loop gates. Best-fit paradigms (P1–P6) are defined in Section 4; A/M/C marks where Archil, Mesa, or Code Storage is a natural fit.

### Software creation (the revenue outlier)

| # | Use case | Footprint | Best-fit infra |
|---|---|---|---|
| 1 | Background coding agent: issue → tested PR (Devin, Codex, Claude Code, Cursor cloud agents) | E·D·F·H | Snapshot sandbox for the env + versioned repo state out (P2+P5); C for repo infra, A for shared dep caches |
| 2 | Bug-fix fleets: triage → reproduce → patch, dozens in flight | E·D·F·H | Ephemeral branch per attempt, promote winners (P5: C); env via snapshot (P2) |
| 3 | AI code review (Mesa's own first product; CodeRabbit, Graphite) | D·H, read-heavy | Git-as-DB with server-side diff/grep (P5: C); lazy mount for context (M) |
| 4 | Test generation and repair | E·F | Pure ephemeral sandbox + branch (P1/P2) |
| 5 | CI-failure auto-fix on red builds | E·D(caches) | Blueprint/snapshot sandboxes (P2) + git (P5) |
| 6 | Mass migrations/refactors across hundreds of repos | E·S·F·H | Repo-per-job at API scale (P5: C); fan-out `disk.grep`/exec over a shared FS (P4: A) |
| 7 | Codebase Q&A / onboarding agent over a monorepo | S, read-only | Sparse/lazy mount — pay per byte touched (P4: M, A) |
| 8 | Documentation generation kept in sync with code | E·D·H | Versioned workspace + scheduled exec (P4/P5) |
| 9 | Fleet-wide dependency & security upgrades (Dependabot++) | E·F·H | Repo infra + ephemeral compute (P5+P1) |
| 10 | Best-of-N parallel attempts on one task (SWE-bench-style fan-out) | F·E·D | O(1) forks: data branches (M, C, A-branches) or VM branches (Morph-class, P2) |

### App builders ("vibe coding" — the strongest persistent-state demand)

| # | Use case | Footprint | Best-fit infra |
|---|---|---|---|
| 11 | Prompt-to-app platforms (Lovable, Bolt, Replit, v0) | E·D·S·F·H | Per-user durable workspace + instant-attach compute (P4+P2); this is C's core market today |
| 12 | Live preview / dev-server per user session | E·B, warm pools | Snapshot sandboxes (P2) + lazy mounts (P4) |
| 13 | Design-to-code (Figma → component) | E·F·H | Lighter #11; variant forks |
| 14 | Landing-page/site generators at consumer scale | E·D, huge volume | Isolates + virtual FS (P6) — container economics don't work |
| 15 | Internal-tool builders on org data | E·D·S·H | Workspace + shared component library FS (P4) |

### Data, analytics, ML

| # | Use case | Footprint | Best-fit infra |
|---|---|---|---|
| 16 | Ad-hoc data analysis: CSV/parquet → insight (the #2 LangChain use case) | E·D·S | FS-with-compute is the literal shape: `disk.exec` (A) or sandbox + dataset mount (P4+P1) |
| 17 | Text-to-SQL BI and scheduled reporting | D(artifacts) | Warehouse does the work; durable artifact store suffices |
| 18 | Spreadsheet agents (finance modeling, FP&A) | E·D·H | Versioned workspace — checkpoint per model revision (P4: M) |
| 19 | ETL pipeline construction & maintenance | E·D, scheduled | Durable workspace + scheduled ephemeral exec (P4+P1) |
| 20 | Data cleaning / labeling at scale | E·S·F | Shared FS + parallel exec shards (P4: A) |
| 21 | ML training pipelines (data prep → train → eval) | E·G·S | Object-storage-backed FS for datasets/checkpoints + GPU compute (P4 substrate) |
| 22 | RL rollouts to train agents (Cognition: "millions of sandboxes") | E·F·G, 10k–100k concurrent | **Machine snapshots win**: identical resets need process+env determinism (P2: Morph, Modal, E2B) |

### Research & knowledge work

| # | Use case | Footprint | Best-fit infra |
|---|---|---|---|
| 23 | Deep-research agents (this report is one) | B·F, artifacts | Ephemeral compute + workspace for notes/citations (P1+P4) |
| 24 | Continuous market/competitive monitoring | B·D, scheduled | Durable memory files + ephemeral runs (P4: A's `memory.jsonl` pattern) |
| 25 | Scientific discovery loops (design-make-test-analyze) | E·G·F·D | Shared experiment FS + GPU bursts (P4+P2) |
| 26 | Data-room due diligence (Hebbia-style parallel doc agents) | S·F·H, audit | Versioned shared corpus with scoped access (P4: M — legal design partners) |
| 27 | Patent / prior-art / literature search | B·F | Like #23 |

### Browser & computer use

| # | Use case | Footprint | Best-fit infra |
|---|---|---|---|
| 28 | Web scraping / structured extraction fleets | B·F·D(sessions) | Browser fleet; machine snapshots preserve authed sessions (P2) |
| 29 | Portal automation: tax, insurance, gov filings (Browserbase's bread and butter) | B·D·H, audit | Machine snapshot for logged-in state (P2) + durable case files (P4) |
| 30 | E2E QA testing across a matrix | B·E·F, clean env per run | Snapshot-restore is the point (P2); recordings to object FS |
| 31 | Desktop computer-use agents (legacy apps, RPA replacement) | Full VM·D | Persistent VM/devbox (P3) or machine snapshots (P2) |
| 32 | Price/inventory/compliance monitors | B, scheduled, tiny state | Isolates + KV/virtual FS (P6) |

### Back office & business ops (the fastest-rising API category)

| # | Use case | Footprint | Best-fit infra |
|---|---|---|---|
| 33 | Customer support resolution (the #1 production use case: Fin, Sierra, Decagon) | D(conversation)·H | No sandbox needed; durable conversation store + skills/artifact workspace |
| 34 | SDR / outbound personalization (the single largest bottom-up API task) | F·B, batch | Isolate-scale compute (P6); CRM is the state |
| 35 | CRM hygiene & enrichment | F | Same as #34 |
| 36 | Invoice / AP processing | D·H, audit | Durable doc workspace + light exec; versioned audit trail (P4: M) |
| 37 | Insurance claims processing (days-long cases) | D·H | Durable case workspace outliving every compute session (P4) |
| 38 | Contract review & redlining | S·H, diff-native | **Versioned FS is a near-perfect fit** — redlines are branches, approvals are bookmark moves (M) |
| 39 | Compliance monitoring with mandatory audit trails | B·D, append-only | Versioned append-only workspace (P4/P5) |
| 40 | Financial reconciliation & monthly close | E·D·H | Workspace + exec; checkpoint per close cycle (P4) |
| 41 | Recruiting: sourcing & screening at volume | B·F | Isolate-scale (P6); ATS is the state |
| 42 | Email/calendar personal assistants | D(memory)·H | Tiny durable memory files; no sandbox (P4-lite or P6) |

### Content & media

| # | Use case | Footprint | Best-fit infra |
|---|---|---|---|
| 43 | Brand-aware marketing content pipelines | D·S·F·H | Shared asset FS + variant branches (P4) |
| 44 | Document / presentation generation | E, artifacts | Ephemeral exec + artifact store (P1+P4) |
| 45 | Video/audio generation pipelines (Runway, Suno on Modal) | G·E·S | GPU compute + object-FS staging for large media (P4 substrate: A) |
| 46 | Localization with translation memory | D·S, batch | Durable TM files + batch exec (P4) |

### Ops, security, and the maximal case

| # | Use case | Footprint | Best-fit infra |
|---|---|---|---|
| 47 | SRE incident assistants (13.8% autonomous on ITBench — assistive, not autonomous) | E·F·H | Ephemeral exec + incident workspace (P1+P4) |
| 48 | DevOps/IaC agents (plan → human gate → apply) | E·H, audit | Git-native state (P5: C) + ephemeral exec; plan = branch |
| 49 | SOC alert triage at 24/7 volume | F, streaming | Isolate-scale compute (P6) + case store |
| 50 | Autonomous "virtual computer" assistants (Manus-class, 27 tools, dozens-of-minutes tasks) | E·B·D·S·F·H | The maximal footprint: durable workspace (P4) + resumable machine state (P2/P3) — the hybrid frontier |

**Tally** (one paradigm assigned as primary per row): durable shared/versioned state with disposable compute (P4/P5) is primary for ~31 of 50 and a required complement in ~8 more; machine-snapshot compute (P2) is primary for ~8 (RL, QA, browser sessions, env caching); isolate-scale minimal-state (P6) for ~6 consumer/volume cases; persistent VMs (P3) and pure ephemeral sandboxes (P1) are primary for only ~2–3 each. The pattern: **state outlives compute in almost every economically important use case, and the state that matters is overwhelmingly files-and-versions, not machine images.**

---

## 4. System-design comparison of the infrastructure paradigms

### The six paradigms

- **P1 — Pure ephemeral sandbox.** Create, run, destroy; state exits via git push or API. (Vercel Sandbox, AWS AgentCore code interpreter, ChatGPT containers.) AWS makes the principled case: per-session microVM destruction makes cross-session state explicit and security trivial to reason about ([Brooker, Seven Years of Firecracker](https://brooker.co.za/blog/2025/09/18/firecracker.html)).
- **P2 — Snapshot/branchable sandbox.** Ephemeral substrate plus save-states of disk or disk+memory. (Morph Infinibranch <250ms branch of a running VM; E2B pause/resume — ~4s/GiB to pause, ~1s to resume; Modal's three snapshot kinds; Cognition's blockdiff — a 20 GB disk snapshot in ~200ms via XFS reflink after rejecting OverlayFS and ZFS ([blockdiff](https://cognition.ai/blog/blockdiff)).)
- **P3 — Persistent VM / devbox.** The agent owns a durable computer. (Fly Sprites: 1–2s create, sleeps without metering, 100 GB durable storage, casual checkpoint/restore — and a manifesto: "ephemeral sandboxes are obsolete… they don't want sandboxes. They want computers" ([Code And Let Live](https://fly.io/blog/code-and-let-live/)); Devin's machine snapshots; Runloop devboxes.)
- **P4 — Durable shared filesystem + disposable compute.** State lives in a network-mounted, versioned-or-not file tree that outlives any sandbox. (**Archil, Mesa**, JuiceFS-class systems, Cloudflare Sandboxes + R2.)
- **P5 — Git-as-storage.** The repo is the state database; compute clones in and pushes out. (**Code Storage**, Freestyle, GitHub-as-backend.)
- **P6 — Isolates + virtual FS.** V8 isolates starting in milliseconds at ~MB memory cost, with a simulated filesystem over SQLite/R2 — no real OS. (Cloudflare Dynamic Workers: "100x faster and 10–100x more memory-efficient than a typical container," $0.002/day per unique Worker ([Dynamic Workers](https://blog.cloudflare.com/dynamic-workers/)); `just-bash`-style simulated environments — which both Archil and Mesa pointedly support as mount targets.)

### Design-dimension matrix

| Dimension | P1 Ephemeral | P2 Snapshot/branch | P3 Persistent VM | P4 Durable FS + compute | P5 Git-as-storage | P6 Isolate + VFS |
|---|---|---|---|---|---|---|
| Where truth lives | Nowhere (exits via push) | Snapshot images | The machine | The file tree (object-store backed) | The repo DAG | KV/SQLite/R2 |
| Survives sandbox death | ✗ | ✓ (as image) | ✓ | ✓ | ✓ | ✓ |
| Shared by N concurrent agents | ✗ | ✗ (fork = copies) | weak (one machine) | ✓ native | ✓ (via branches) | ✓ |
| Fork semantics | none | **machine** fork (mem+disk) | none/manual | **data** fork (CoW tree) | **data** fork (refs) | none/manual |
| Mergeable / diffable state | ✗ | ✗ (opaque images) | ✗ | ✓ (if versioned: M) | ✓✓ native | partial |
| Audit / rollback / HITL gates | external | coarse (restore image) | external | ✓ (checkpoint/observe-mode) | ✓✓ (commits, PRs) | external |
| Attach latency | 100–400ms boot | ~ms–1s restore | ~1–2s wake | ~100ms mount, lazy materialize | clone tax (offset server-side) | ~ms |
| Partial access (don't move all data) | ✗ | ✗ | n/a | ✓ sparse/lazy | shallow/sparse clones | ✓ |
| Live process/memory state | ✗ | ✓✓ unique strength | ✓ | ✗ | ✗ | ✗ |
| Idle economics | perfect (nothing exists) | storage only | storage + sleep | active-data pricing; cold ≈ S3 | warm/cold tiers | near-zero |
| Security boundary | the box (strongest story) | the box | the box (long-lived = drift) | the data (scoped tokens) + any box | the data (repo-scoped JWTs) | the isolate (weaker; defense-in-depth) |
| Interface familiarity to models | bash ✓ | bash ✓ | bash ✓ | **files/bash ✓✓** | git ✓ (clone friction) | restricted bash subset |
| Failure coupling | none | image registry | the one machine | shared service (correlated risk) | shared service | platform |

### The four tensions that decide fit

**1. Ephemerality vs. persistence is a false binary — the resolution is *which layer* persists.** Fly argues agents want durable computers; AWS argues sessions should die for security; both are right about different layers. The synthesis the industry is converging on (RisingWave's framing: "pause = state death" is the failure; once sandboxes are stateful you have a *state-management* problem ([stateful sandboxes](https://risingwave.com/blog/stateful-sandboxes-for-ai-agents/))) is: **compute ephemeral, state durable, and an explicit boundary between them.** That is precisely the P4/P5 shape — and even the sandbox vendors now ship it piecemeal (E2B persistence, Modal volumes/snapshots, Daytona "stateful sandboxes", Fly's new storage stack).

**2. Branch the machine or branch the data?** Machine forks (P2) capture *everything* — running processes, loaded kernels, authed browsers — and are unbeatable where in-flight state is the asset: RL resets, QA reproducibility, browser sessions (#22, #28–31). But machine images are opaque (no diff, no review), non-mergeable (no way to combine two forks' work), hardware-coupled (Modal restores require the same instance type), and heavy. Data forks (P4/P5) are O(1), inspectable, mergeable, promotable, and reviewable — which is what best-of-N, swarms, and HITL actually require (#1, #2, #10, #26, #38). The 50-case tally says data-branching serves ~4x more use cases. The mature stack uses both, with a clean division: **fork data for exploration and collaboration; snapshot machines as a *cache of derived state* (deps, env) that is always rebuildable from files** (lockfile → layer — exactly how Cursor's `environment.json` disk snapshots and Runloop blueprints work).

**3. Data gravity: move data to compute, or mount data where compute lands?** Clone-per-run (P5 naive, P1) taxes every session; Mesa calls this the clone latency tax, Archil's answer is ~100ms mounts with lazy materialization, Pierre's answer is making the server do the work (grep, archive, diff-commits) so less moves. For large corpora (#21, #26, #45) mounting wins outright; for small hot repos, clones are fine and Git's interop is worth the tax. The deciding factor is *partial access*: agents typically touch a tiny fraction of the bytes they're pointed at, so pay-per-byte-touched (sparse materialization) beats pay-per-corpus-moved.

**4. Idle economics decide the consumer/agent-fleet frontier.** Agents are idle most of their wall-clock life (waiting on LLM calls, humans, schedules). Every winning pricing model converges on "pay ~nothing at rest": Vercel's active-CPU billing (up to 95% savings on I/O-bound loops), Fly's sleep-without-metering, Archil's active-data metering, Pierre's warm/cold tiers, Cloudflare's per-day isolate pricing. System-design corollary: **the substrate must make state cheap when cold and fast when hot** — which forces object storage as the bottom layer and a cache hierarchy above it, i.e., the architecture Archil, Mesa, Fly Sprites, and Pierre's cold tier all independently adopted.

**Security note (system-design, not technique):** for untrusted LLM-generated code the isolation boundary is commoditizing downward (Firecracker/gVisor for cloud, bubblewrap/Seatbelt/Landlock for CLI harnesses — both Anthropic's and OpenAI's harnesses converged on OS-primitives + network egress control). The interesting *architectural* move in 2025–26 is relocating the **authorization** boundary from the box to the data: short-TTL capability tokens scoped to a repo/branch/path (Pierre's per-repo JWTs, Mesa's branch-glob/path-scoped keys, Archil's per-disk session tags), plus credential injection at the proxy so agents never hold secrets. Once authority lives in the data layer, the sandbox stops being a trust anchor and becomes a disposable wrapper — which is what lets compute be truly fungible across P1/P2/P6.

---

## 5. The primitive: the branchable agent workspace

### Statement

> **The universal primitive for agents is not a machine, a sandbox, or a bucket. It is a *workspace*: a durable, branchable, lazily-materialized file tree with version semantics, mountable into any compute substrate, with execution attached to it as a function — and authority scoped to it rather than to the box.**
>
> State is the noun. Compute is a verb. Branches are control flow. Commits are the audit log.

Interface sketch (every line below exists somewhere across Archil, Mesa, and Code Storage today — no single product has all of it):

```python
ws  = workspace.open("acme/claims-q2")            # durable named file tree (O(1), lazy)
b   = ws.fork()                                   # O(1) copy-on-write branch per agent/attempt
r   = b.exec("python reconcile.py", cpu=2)        # compute attached to state, billed per active ms
ck  = b.checkpoint("post-reconcile")              # immutable, addressable, auditable point
d   = b.diff(ws)                                  # reviewable change → human approval gate
ws.merge(b)        # or b.promote() / b.discard() # land, ship, or throw away
tok = b.token(scope="claims/**", mode="rw", ttl="15m")   # capability lives in the data layer
```

### Why each property is forced by the evidence

**Files as the interface.** Models are trained on decades of unix tools, file paths, and git output; every harness (Claude Code, Codex, OpenCode) is already a files-and-bash loop, and Anthropic's own guidance is to keep intermediate state in the execution environment's filesystem rather than the context window — their MCP code-execution pattern cut a workflow from 150,000 to 2,000 tokens by doing exactly that ([Anthropic](https://www.anthropic.com/engineering/code-execution-with-mcp)). The filesystem is simultaneously the agent's working memory, its tool I/O bus, and its cheapest context-offload. Archil's "representation in the training data" argument is the right one, and it compounds: every new model generation is trained on more agent-filesystem transcripts.

**Durable and shared, with compute disposable.** Section 3's tally: state outlives compute in ~39 of 50 use cases, and in the multi-agent ones the state must be *shared*, not trapped per-machine. This inverts the sandbox-era resource model — exactly Archil's "the filesystem is the resource" inversion — and it is where the puck is moving even among compute vendors (E2B persistence, Modal volumes, Daytona statefulness, Fly building a storage stack under Sprites).

**Version-native, not version-bolted-on.** Approval gates (#1, #36–40, #48), audit trails (#26, #39), rollbacks, replayable evals, and agent attribution all reduce to version-control operations *if and only if* every write is already a change. Mesa's observe-mode/fork-on-first-write and Pierre's ephemeral-branches-then-promote are the same insight from two directions: **agent output is untrusted by default, and promotion to trusted is an explicit, recorded act.** This is the storage-layer expression of human-in-the-loop, and it is what enterprises will buy.

**O(1) fork, mergeable results.** Parallel exploration is the dominant agent scaling pattern (best-of-N attempts, swarms, eval matrices). Forks must cost nothing to create (copy-on-write), and — unlike machine images — results must be diffable and mergeable so winners can be promoted and combined. Branching data, not machines, is the only version of this that supports review.

**Lazily materialized.** Agents touch a sliver of what they're pointed at. Mounting in ~100ms and faulting bytes in on demand (Archil), or materializing files on access (Mesa), beats both clone-everything (git) and copy-into-the-image (snapshots). This single property collapses the cold-start problem and the data-gravity problem into one solution.

**Object-storage substrate, tiered.** Cold state at S3 economics, hot state at NVMe latency, billing keyed to activity. Every player converged here independently; it is what makes "one workspace per user-session" (the app-builder pattern, #11) and "one workspace per claim/contract/matter" (#36–38) economically possible at consumer scale.

**Git-interoperable at the edge.** Whatever the internal engine (Archil's protocol, Mesa's jj, Pierre's distributed refs), the workspace must speak git at its boundary — because git is the interchange format with humans, CI, GitHub, and the entire existing SDLC. Pierre's whole company is evidence of how much that compatibility is worth; Mesa ships a git translation layer for the same reason.

**Authority on the data, not the box.** Scoped, short-TTL capability tokens per branch/subtree make the sandbox a disposable wrapper rather than a trust anchor, which is what makes compute substrate-agnostic — the same workspace mounted in a microVM today, an isolate tomorrow, a local laptop offline, and whatever replaces them. The primitive must outlive any particular isolation technology to be "future-proof," and only data-scoped authority allows that.

**Compute attached, not bundled.** `exec` as a method on the workspace (Archil's `disk.exec` is the cleanest existing expression) means the default path needs no sandbox vendor at all for the long tail of small executions — while still allowing a full microVM/browser/GPU to mount the same workspace when the job demands it (#22, #28–31, #45).

### What deliberately stays out

**Live process memory is not part of the universal primitive.** Machine snapshots (Morph, E2B pause, Modal memory snapshots, CRIU-style restore) remain the right tool for the ~15% of cases where in-flight state *is* the asset — authed browser sessions, RL determinism, hot kernels. But process state is opaque, non-mergeable, non-reviewable, and hardware-coupled, so it cannot be the layer of record. The clean architecture treats the machine image as a **derived cache of the workspace** (environment = deterministic function of lockfiles/config, snapshot it for warm starts, throw it away freely) — files remain the source of truth, machines become memoization. Cognition's blockdiff, Cursor's environment caching, and Runloop blueprints are all already this pattern in practice.

### Scored against the three companies

| Property of the primitive | Archil | Mesa | Code Storage |
|---|---|---|---|
| POSIX file interface, mount-anywhere | ✓✓ (its core) | ✓ | ✗ (git/API only) |
| Lazy materialization / partial access | ✓ | ✓ | partial (server-side ops, shallow clones) |
| Every-write versioning, HITL gates | partial (checkpoints; not commit-grained) | ✓✓ (its core) | ✓ (commits/branches) |
| O(1) fork + merge/promote | ✓ branches (but not on S3-synced disks) | ✓✓ | ✓✓ (ephemeral→promote) |
| Git interop at the edge | partial (S3 API, not git) | ✓ (translation layer) | ✓✓ (its core) |
| Object-store economics, tiering | ✓✓ (active-data pricing) | ✓ | ✓ (warm/cold) |
| Data-scoped capability tokens | ✓ (per-disk) | ✓✓ (branch/path globs) | ✓✓ (per-repo JWTs) |
| Attached compute | ✓✓ (`disk.exec`) | ✗ | ✗ |
| Proven scale / GA | ✓ | ✗ (private beta) | ✓ (platform deals) |

No one has the full primitive. **Archil** has the interface, the latency, and the only attached compute, but its version semantics are coarse and its fork capability is walled off from its bring-your-own-bucket story. **Mesa** has the most correct semantic core (jj's change model is arguably the right theory of agent writes) but no compute, no GA, and the least proof. **Code Storage** has the interop, the economics, the team, and real platform customers, but its interface choice — repos you clone rather than trees you mount — concedes the "agents just want files" thesis to the other two. The end-state product is plainly the union: *Mesa-grade semantics on an Archil-grade mount with Pierre-grade git interop and tiering.* Expect convergence — each is one feature launch away from invading the others' ground (Archil adding commit-grained history; Mesa adding exec; Pierre adding a FUSE/VFS mount) — and expect the sandbox vendors (who own distribution) and hyperscalers (who own the buckets) to bundle approximations of the same primitive.

### Honest counter-theses

*"Agents want computers, not filesystems"* (Fly). True for ergonomics on single long-horizon tasks, and Sprites is a great product thesis — but a fleet of computers without a shared, reviewable state layer recreates the pre-git era of software at agent speed. Notably, Fly had to build a durable storage stack *under* Sprites anyway; the computer is the cockpit, the workspace is still the cargo.

*"Ephemerality is the security model"* (AWS). Fully compatible: the workspace primitive is what makes total compute ephemerality *affordable* — destroy every box after every session precisely because nothing of value lives in it.

*"Long context will obsolete external state."* Context windows solve recall within a session; they do not provide sharing across agents, durability across months, audit for compliance, or 10-TB datasets — and tokens-at-rest in files are orders of magnitude cheaper than tokens-in-context.

*"GitHub/S3 incumbency wins by default."* The strongest objection. The counter is the one Pierre's existence proves: incumbent assumptions (rate limits, repo = human project, bucket = no semantics) break at agent scale, and incumbents monetize humans, not machine traffic. The risk remains real — an "S3 with branches" or "GitHub Agents API" launch would compress this entire category, which is the chief reason all three companies are racing to embed into agent platforms now.

### Bottom line

For a builder choosing infra today: default to **durable branchable workspace + cheapest disposable compute that can mount it**, add machine snapshots only where live process state is the asset, and drop to isolates where unit economics dominate. For an investor: the three companies are not competing storage vendors so much as three claims about the *interface* to the same inevitable layer — and the 50-use-case evidence says the file-tree-with-version-semantics claim covers the most ground, with git interop as the moat-bearing edge and attached compute as the margin-bearing one.

---

## 6. Sources

**Archil:** [archil.com](https://archil.com/) · [Series A post](https://archil.com/post/series-a) · [seed announcement](https://archil.com/post/archil-file-system-to-data-company) · [docs: architecture](https://docs.archil.com/details/architecture.md), [consistency](https://docs.archil.com/details/consistency.md), [performance](https://docs.archil.com/details/performance.md), [branches](https://docs.archil.com/concepts/branches-and-checkpoints.md), [serverless exec](https://docs.archil.com/compute/serverless-execution.md), [billing](https://docs.archil.com/administration/billing.md), [comparison](https://docs.archil.com/details/comparison.md) · [YC profile](https://www.ycombinator.com/companies/archil) · [Felicis investment note](https://www.felicis.com/insight/investing-in-archil) · [Launch HN (Regatta, Nov 2024)](https://news.ycombinator.com/item?id=42174204) · [Show HN (protocol GA, Sept 2025)](https://news.ycombinator.com/item?id=45264956) · [AlleyWatch funding log](https://www.alleywatch.com/2026/04/the-weekly-notable-startup-funding-report-4-27-26/)

**Mesa:** [mesa.dev](https://www.mesa.dev/) · [filesystem launch (Apr 28, 2026)](https://www.mesa.dev/blog/introducing-mesa-filesystem-for-agents) · [code-review launch](https://www.mesa.dev/blog/introducing-mesa) · [git server](https://www.mesa.dev/features/git-server) · [docs: versioning](https://docs.mesa.dev/content/core-concepts/versioning), [virtual filesystem](https://docs.mesa.dev/content/core-concepts/virtual-filesystem) · [pricing](https://www.mesa.dev/pricing) · [about](https://www.mesa.dev/about) · [Innovation Endeavors: Meet Mesa](https://www.innovationendeavors.com/insights/mesa-code-verification) · [South Park Commons portfolio](https://www.southparkcommons.com/companies/mesa/) · ["Coding agents are infra"](https://www.mesa.dev/blog/coding-agents-are-infra)

**Code Storage / Pierre:** [code.storage](https://code.storage/) · [launch post (Oct 14, 2025)](https://code.storage/changelog/introducing-code-storage) · [ephemeral branches](https://code.storage/changelog/ephemeral-branches) · [docs: introduction](https://code.storage/docs/getting-started/introduction.md), [sandboxes guide](https://code.storage/docs/guides/sandboxes), [core concepts](https://code.storage/docs/getting-started/core-concepts) · [pricing](https://code.storage/pricing) · [pierre.computer](https://pierre.computer) · [careers](https://pierre.computer/careers/systems-engineer) · [YC profile](https://www.ycombinator.com/companies/pierre) · [HN thread with founder comments (Feb 2026)](https://news.ycombinator.com/item?id=46957629) · [Treybig: The Two Software Development Stacks](https://davistreybig.substack.com/p/the-two-software-development-stacks)

**Landscape & evidence base:** [Fly: Code And Let Live](https://fly.io/blog/code-and-let-live/) · [Cognition: blockdiff](https://cognition.ai/blog/blockdiff) · [Brooker: Seven Years of Firecracker](https://brooker.co.za/blog/2025/09/18/firecracker.html) · [E2B persistence docs](https://e2b.dev/docs/sandbox/persistence) · [E2B Series A (VentureBeat)](https://venturebeat.com/ai/how-e2b-became-essential-to-88-of-fortune-100-companies-and-raised-21-million) · [Modal sandbox snapshots](https://modal.com/docs/guide/sandbox-snapshots) · [Modal sandboxes](https://modal.com/products/sandboxes) · [Morph Infinibranch](https://cloud.morph.so/docs/blog/developers) · [Cloudflare Dynamic Workers](https://blog.cloudflare.com/dynamic-workers/) · [RisingWave: stateful sandboxes](https://risingwave.com/blog/stateful-sandboxes-for-ai-agents/) · [Anthropic: code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp) · [Mountpoint-S3 semantics](https://github.com/awslabs/mountpoint-s3/blob/main/doc/SEMANTICS.md) · [LangChain State of Agent Engineering](https://www.langchain.com/state-of-agent-engineering) · [a16z: Where enterprises are adopting AI](https://a16z.com/where-enterprises-are-actually-adopting-ai/) · [Anthropic Economic Index (Jan 2026)](https://www.anthropic.com/research/anthropic-economic-index-january-2026-report) · [OpenAI State of Enterprise AI](https://openai.com/index/the-state-of-enterprise-ai-2025-report/) · [Browserbase use cases](https://www.browserbase.com/blog/what-can-i-use-browserbase-for) · [E2B × Manus](https://e2b.dev/blog/how-manus-uses-e2b-to-provide-agents-with-virtual-computers) · [Daytona](https://www.daytona.io/) · [sandbox comparison survey 2026](https://michaellivs.com/blog/sandbox-comparison-2026/)

*Caveats: all vendor performance figures (Archil 45x/sub-ms, Mesa sub-50ms, Pierre 60x) are self-published without methodology; Mesa and Code Storage funding figures rest on investor/self-published sources; Mesa and Code Storage are pre-GA/private-beta so capability claims are externally unverifiable; ARR figures cited for context (Cursor, Harvey, Sierra, Lovable) are company-reported or analyst-estimated.*
