//! asp — agentspaces CLI: instant, disposable, fully-reviewable forks of your
//! real working directory for AI agents.
//!
//! Agents are first-class users: every command supports `--json`, and every
//! error states the corrective next action.

mod bench;
mod hooks;
mod mcp;
mod race;
mod ui;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use asp_core::journal::{Entry, Op, Source};
use asp_core::store::{atomic_write, find_root, ASP_DIR, FORMAT_VERSION};
use asp_core::workspace::{CheckpointOpts, DiffTextMode, Finding, Severity};
use asp_core::{Error, ErrorCode, Workspace};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Parser)]
#[command(
    name = "asp",
    version,
    about = "Durable, branchable workspaces for AI agents — fork your real working directory in \
             milliseconds, checkpoint every change, promote the winner.",
    after_help = "\
EXAMPLES:
  asp init                          adopt this directory (instant; nothing is captured yet)
  asp checkpoint -m \"before refactor\"
  asp fork -n 3                     three instant copy-on-write forks, side by side
  asp race -n 3 -- claude -p \"fix the failing test\"
  asp forks                         compare what each fork changed
  asp promote fork-2                land the winner as git branch asp/fork-2
  asp undo                          step back / revert agent damage (bash included)

Every command accepts --json for machine-readable output."
)]
struct Cli {
    /// Machine-readable JSON output (for agents and scripts).
    #[arg(long, global = true)]
    json: bool,

    /// Run as if started in this directory.
    #[arg(short = 'C', long, global = true, value_name = "DIR")]
    dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Adopt this directory as an asp workspace (never touches your files or .git).
    Init {
        /// Optional human label for the workspace.
        #[arg(long)]
        label: Option<String>,
    },
    /// Workspace summary: dirty files, last checkpoint, active forks.
    Status,
    /// Local store statistics: checkpoints, forks, blobs, size, recent timings.
    Stats,
    /// Print a safe first-five-minutes workflow for the current directory.
    Quickstart,
    /// Run a read-only readiness gate for CI and team onboarding.
    Preflight {
        /// Include deep CAS verification in the doctor check.
        #[arg(long)]
        deep: bool,
    },
    /// Inspect effective workspace configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCmd,
    },
    /// Benchmark and local filesystem probes.
    Bench {
        #[command(subcommand)]
        command: BenchCmd,
    },
    /// Print supported schema and format versions.
    Schema,
    /// Generate shell completion scripts.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Generate a roff manpage for the asp CLI.
    Manpage,
    /// Show filtered audit events from the local journal.
    Audit(AuditArgs),
    /// Inspect and validate local workspace policy.
    Policy {
        #[command(subcommand)]
        command: PolicyCmd,
    },
    /// Find likely secrets before they enter checkpoint history.
    Secrets {
        #[command(subcommand)]
        command: SecretsCmd,
    },
    /// Plan non-destructive checkpoint retention from local policy.
    Retention {
        #[command(subcommand)]
        command: RetentionCmd,
    },
    /// Sync checkpoints and large-file blobs to an explicit remote.
    Sync {
        #[command(subcommand)]
        command: SyncCmd,
    },
    /// Capture the current state as a checkpoint (no-op if nothing changed).
    #[command(visible_alias = "cp")]
    Checkpoint {
        /// Checkpoint message.
        #[arg(short, long)]
        message: Option<String>,
        #[command(flatten)]
        provenance: Provenance,
    },
    /// Timeline of checkpoints and operations, newest first.
    Log {
        /// Max entries to show.
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,
    },
    /// Step back one checkpoint (or revert uncommitted changes if dirty).
    Undo,
    /// Restore the working tree to a checkpoint (whole tree, or specific paths).
    Restore {
        /// Checkpoint: a #seq number (e.g. 42) or commit prefix from `asp log`.
        checkpoint: String,
        /// Restore only these paths (relative to the workspace root).
        paths: Vec<String>,
    },
    /// Create instant copy-on-write fork(s) of the whole directory.
    Fork {
        /// How many forks to create.
        #[arg(short = 'n', long, default_value = "1")]
        count: u32,
        /// Name (single fork) or name prefix (multiple forks).
        #[arg(long)]
        name: Option<String>,
    },
    /// List forks and compare what each one changed.
    Forks,
    /// Emit a dashboard/CI-comment review packet for the workspace.
    Review,
    /// Show what changed between two checkpoints, or a checkpoint and now.
    Diff {
        /// Show a unified patch instead of the summary table.
        #[arg(long)]
        patch: bool,
        /// Show git-style diffstat instead of the summary table.
        #[arg(long)]
        stat: bool,
        /// Write an offline HTML diff review artifact.
        #[arg(long)]
        html: bool,
        /// Output path for --html (relative paths resolve inside the workspace).
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
        /// Compare a fork against its fork point.
        #[arg(long, value_name = "NAME")]
        fork: Option<String>,
        /// From: #seq or commit prefix.
        from: Option<String>,
        /// To: #seq or commit prefix (default: the working tree).
        to: Option<String>,
    },
    /// Land a fork's work as an ordinary git branch in this repo.
    Promote {
        /// Fork name (see `asp forks`).
        fork: String,
        /// Branch name to create (default: promote.branch_template in .asp/config.toml).
        #[arg(long)]
        branch: Option<String>,
        /// Push the created branch after promoting it.
        #[arg(long)]
        push: bool,
        /// Remote to push to with --push (for example: origin).
        #[arg(long, value_name = "REMOTE")]
        remote: Option<String>,
        /// Create a draft pull request with gh after pushing; falls back to instructions.
        #[arg(long)]
        pr_draft: bool,
    },
    /// Delete a fork (refuses if it has unpromoted work, unless --force).
    Discard {
        /// Fork name (see `asp forks`).
        fork: String,
        /// Delete even if the fork has unpromoted work.
        #[arg(long)]
        force: bool,
    },
    /// Fork N ways, run the same command in each, and compare the results.
    Race(RaceArgs),
    /// Check workspace health; --fix applies safe repairs.
    Doctor {
        /// Apply safe repairs.
        #[arg(long)]
        fix: bool,
        /// Re-hash large-file CAS blobs to detect silent corruption.
        #[arg(long)]
        deep: bool,
        /// Show cause and next action for each finding.
        #[arg(long)]
        explain: bool,
        /// Show runbook links for common repair scenarios.
        #[arg(long)]
        runbook: bool,
    },
    /// Emit a redacted diagnostics bundle for issue reports and support.
    Diagnostics {
        /// Include full local paths. Secrets are still redacted.
        #[arg(long)]
        include_paths: bool,
        /// Write the bundle to this JSON file instead of stdout.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// Run the MCP stdio server (for agent harnesses like Claude Code).
    ///
    /// Register it with: claude mcp add agentspaces -- asp mcp
    Mcp,
    /// Wire this workspace into an agent harness (hooks + MCP registration).
    Setup {
        /// The harness to integrate with.
        #[command(subcommand)]
        harness: SetupHarness,
    },
    /// (internal) Invoked by harness hooks; reads the event from stdin.
    #[command(hide = true, name = "hook-event")]
    HookEvent,
}

#[derive(Args)]
struct AuditArgs {
    /// Include only entries with this agent session id.
    #[arg(long)]
    session: Option<String>,
    /// Include only entries created by this tool.
    #[arg(long)]
    tool: Option<String>,
    /// Include only this operation. Repeat to include multiple operations.
    #[arg(long = "op", value_parser = parse_op_filter)]
    ops: Vec<Op>,
    /// Include only path-aware entries touching this workspace-relative path.
    #[arg(long = "path")]
    paths: Vec<String>,
    /// Include entries at or after this RFC3339 timestamp.
    #[arg(long, value_parser = parse_rfc3339)]
    since: Option<OffsetDateTime>,
    /// Include entries at or before this RFC3339 timestamp.
    #[arg(long, value_parser = parse_rfc3339)]
    until: Option<OffsetDateTime>,
    /// Output format. Use global --json for the enveloped JSON API.
    #[arg(long, value_enum, default_value_t = AuditFormat::Table)]
    format: AuditFormat,
    /// Max entries to show after filtering.
    #[arg(short = 'n', long, default_value = "100")]
    limit: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum AuditFormat {
    Table,
    Jsonl,
    Csv,
}

#[derive(Subcommand)]
enum BenchCmd {
    /// Report local filesystem capabilities used by asp benchmarks and forks.
    #[command(name = "self")]
    Self_,
}

#[derive(Subcommand)]
enum SecretsCmd {
    /// Scan checkpoint-scoped workspace files for common secret patterns.
    Scan(SecretsScanArgs),
}

#[derive(Args)]
struct SecretsScanArgs {
    /// Scan files that match asp's derived-state excludes too.
    #[arg(long)]
    include_excluded: bool,
    /// Maximum bytes to inspect per file.
    #[arg(long, default_value_t = 1_048_576)]
    max_bytes: u64,
}

#[derive(Subcommand)]
enum PolicyCmd {
    /// Validate `.asp/policy.toml` and print the resolved policy.
    Validate,
}

#[derive(Subcommand)]
enum RetentionCmd {
    /// Show the checkpoint retention plan without deleting anything.
    Plan,
}

#[derive(Subcommand)]
enum SyncCmd {
    /// Push checkpoints and large-file blobs to a local filesystem remote.
    Push {
        /// Local remote directory to create or update.
        #[arg(long, value_name = "DIR")]
        remote: PathBuf,
    },
    /// Fetch missing checkpoints and blobs from a local filesystem remote.
    Fetch {
        /// Local remote directory to read.
        #[arg(long, value_name = "DIR")]
        remote: PathBuf,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Show effective `.asp/config.toml` settings.
    Show,
    /// Validate `.asp/config.toml` without reading other workspace state.
    Validate,
}

#[derive(Subcommand)]
enum SetupHarness {
    /// Claude Code: auto-checkpoint hooks (file edits + bash) and .mcp.json.
    Claude {
        /// Install hooks user-wide (~/.claude) instead of per-project.
        #[arg(long)]
        user: bool,
        /// Remove the integration instead of installing it.
        #[arg(long)]
        remove: bool,
    },
    /// Codex: project-scoped MCP registration in .codex/config.toml.
    Codex {
        /// Write ~/.codex/config.toml instead of project .codex/config.toml.
        #[arg(long)]
        user: bool,
        /// Remove the integration instead of installing it.
        #[arg(long)]
        remove: bool,
    },
    /// OpenCode: MCP registration in opencode.json.
    Opencode {
        /// Write ~/.config/opencode/opencode.json instead of project opencode.json.
        #[arg(long)]
        user: bool,
        /// Remove the integration instead of installing it.
        #[arg(long)]
        remove: bool,
    },
}

#[derive(Args)]
struct RaceArgs {
    /// How many forks to race.
    #[arg(short = 'n', long, default_value = "3")]
    count: u32,
    /// Name prefix for the race forks.
    #[arg(long, default_value = "race")]
    name: String,
    /// Human label for a lane. Repeat to label lanes in order.
    #[arg(long = "label", value_name = "LABEL")]
    labels: Vec<String>,
    /// Per-lane environment template: KEY=VALUE, with {lane}, {fork}, {label}, {path}, {name}.
    #[arg(long = "env", value_name = "KEY=VALUE")]
    env: Vec<String>,
    /// JUnit XML report path template to ingest from each lane. Repeat for multiple reports.
    #[arg(long = "junit", value_name = "PATH")]
    junit_reports: Vec<String>,
    /// Per-attempt timeout, such as 500ms, 30s, 2m, or bare seconds.
    #[arg(long, value_name = "DURATION", value_parser = parse_duration)]
    timeout: Option<Duration>,
    /// Retry failed or timed-out lanes this many times.
    #[arg(long, default_value_t = 0)]
    retries: u32,
    /// Stop still-running lanes after the first successful lane exits 0.
    #[arg(long)]
    cancel_on_success: bool,
    /// Resume an interrupted race from .asp/races/<name>.json.
    #[arg(long)]
    resume: bool,
    /// Optional saved-race action.
    #[command(subcommand)]
    action: Option<RaceAction>,
    /// The command to run in each fork (everything after --).
    #[arg(last = true, value_name = "RUNNER_COMMAND")]
    command: Vec<String>,
}

#[derive(Subcommand)]
enum RaceAction {
    /// Re-rank saved race lanes without rerunning commands.
    Compare {
        /// Saved race name from .asp/races/<name>.json.
        #[arg(long, default_value = "race")]
        name: String,
    },
}

/// Provenance flags used by hooks/MCP to attribute checkpoints to sessions.
#[derive(Args, Default)]
struct Provenance {
    /// What caused this checkpoint.
    #[arg(long, hide = true, value_parser = parse_source)]
    source: Option<Source>,
    /// Agent session id (set by hooks).
    #[arg(long, hide = true)]
    session_id: Option<String>,
    /// Tool that caused the change (set by hooks).
    #[arg(long, hide = true)]
    tool: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct SchemaReport {
    asp_version: &'static str,
    schemas: Vec<SchemaInfo>,
}

#[derive(Debug, serde::Serialize)]
struct SchemaInfo {
    name: &'static str,
    version: u32,
    kind: &'static str,
    path: &'static str,
}

#[derive(Debug, serde::Serialize)]
struct QuickstartReport {
    directory: PathBuf,
    workspace_root: Option<PathBuf>,
    initialized: bool,
    steps: Vec<QuickstartStep>,
    docs: Vec<QuickstartDoc>,
}

#[derive(Debug, serde::Serialize)]
struct QuickstartStep {
    title: &'static str,
    commands: Vec<&'static str>,
    purpose: &'static str,
}

#[derive(Debug, serde::Serialize)]
struct QuickstartDoc {
    title: &'static str,
    path: &'static str,
}

#[derive(Debug, serde::Serialize)]
struct ConfigShowReport {
    root: PathBuf,
    path: PathBuf,
    exists: bool,
    valid: bool,
    config: asp_core::config::Config,
    shadow_excludes: Vec<String>,
    blob_threshold_bytes: u64,
}

#[derive(Debug, serde::Serialize)]
struct PreflightReport {
    root: PathBuf,
    ready: bool,
    checks: Vec<PreflightCheck>,
    doctor_findings: Vec<Finding>,
    secret_findings: Vec<SecretFinding>,
}

#[derive(Debug, serde::Serialize)]
struct PreflightCheck {
    id: &'static str,
    name: &'static str,
    ok: bool,
    summary: String,
    runbook: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct DoctorRunbookReport {
    findings: Vec<DoctorFindingWithRunbook>,
    common_runbooks: Vec<DoctorRunbookLink>,
}

#[derive(Debug, serde::Serialize)]
struct DoctorFindingWithRunbook {
    #[serde(flatten)]
    finding: Finding,
    runbook: DoctorRunbookLink,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
struct DoctorRunbookLink {
    scenario: &'static str,
    link: &'static str,
    operations: &'static [&'static str],
}

#[derive(Debug, serde::Serialize)]
struct PolicyValidateReport {
    path: PathBuf,
    valid: bool,
    policy: asp_core::policy::Policy,
}

#[derive(Debug, serde::Serialize)]
struct SecretsScanReport {
    root: PathBuf,
    files_scanned: u64,
    files_skipped: u64,
    bytes_scanned: u64,
    findings: Vec<SecretFinding>,
}

#[derive(Debug, serde::Serialize)]
struct SecretFinding {
    path: String,
    line: u64,
    kind: String,
    redacted: String,
}

#[derive(Debug, serde::Serialize)]
struct ReviewReport {
    generated_at: String,
    workspace: asp_core::workspace::StatusReport,
    forks: Vec<asp_core::workspace::ForkCompareRow>,
    markdown: String,
}

#[derive(Debug, serde::Serialize)]
struct DiffHtmlOutputResult {
    from: String,
    to: String,
    summary: asp_core::workspace::DiffSummary,
    path: PathBuf,
    bytes: u64,
}

fn parse_source(s: &str) -> Result<Source, String> {
    match s {
        "manual" => Ok(Source::Manual),
        "hook" => Ok(Source::Hook),
        "mcp" => Ok(Source::Mcp),
        "race" => Ok(Source::Race),
        other => Err(format!("unknown source '{other}' (manual|hook|mcp|race)")),
    }
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    match run(cli) {
        Ok(()) => {}
        Err(err) => {
            if json {
                let body = serde_json::json!({
                    "ok": false,
                    "error": {
                        "code": err.code,
                        "message": err.message,
                        "hint": err.hint,
                    }
                });
                println!("{}", serde_json::to_string_pretty(&body).expect("json"));
            } else {
                eprintln!("{} {}", ui::red("error:"), err.message);
                if let Some(hint) = &err.hint {
                    eprintln!("{} {}", ui::yellow("hint:"), hint);
                }
            }
            std::process::exit(1);
        }
    }
}

fn cwd(cli_dir: &Option<PathBuf>) -> Result<PathBuf, Error> {
    match cli_dir {
        Some(d) => Ok(d.clone()),
        None => std::env::current_dir()
            .map_err(|e| Error::new(ErrorCode::Io, format!("cannot read current dir: {e}"))),
    }
}

fn open(cli_dir: &Option<PathBuf>) -> Result<Workspace, Error> {
    Workspace::open(&cwd(cli_dir)?)
}

fn run(cli: Cli) -> Result<(), Error> {
    let json = cli.json;
    match cli.command {
        Cmd::Init { label } => {
            let ws = Workspace::init(&cwd(&cli.dir)?, label)?;
            if json {
                ui::print_json(true, &serde_json::json!({ "root": ws.root() }));
            } else {
                println!(
                    "{} initialized asp workspace at {}",
                    ui::green("✓"),
                    ui::bold(&ws.root().display().to_string())
                );
                println!(
                    "  nothing was captured yet — run {} to take the first checkpoint",
                    ui::cyan("asp checkpoint")
                );
            }
            Ok(())
        }
        Cmd::Status => {
            let ws = open(&cli.dir)?;
            let st = ws.status()?;
            if json {
                ui::print_json(true, &st);
                return Ok(());
            }
            println!("{}", ui::bold(&format!("workspace {}", st.root.display())));
            match &st.last_checkpoint {
                Some(c) => println!(
                    "  last checkpoint: {} {} {}",
                    ui::cyan(&format!("#{}", c.seq)),
                    c.message.as_deref().unwrap_or(""),
                    ui::dim(&c.ts)
                ),
                None => println!("  no checkpoints yet — run {}", ui::cyan("asp checkpoint")),
            }
            let dirty = st.dirty_files + st.untracked_files + st.deleted_files;
            if dirty == 0 {
                println!("  changes since:   {}", ui::green("none"));
            } else {
                println!(
                    "  changes since:   {} ({} modified, {} new, {} deleted)",
                    ui::yellow(&format!("{dirty} files")),
                    st.dirty_files,
                    st.untracked_files,
                    st.deleted_files
                );
            }
            println!("  active forks:    {}", st.active_forks);
            if st.is_fork {
                println!("  {}", ui::dim("this workspace is itself a fork"));
            }
            Ok(())
        }
        Cmd::Stats => {
            let ws = open(&cli.dir)?;
            let stats = ws.stats()?;
            if json {
                ui::print_json(true, &stats);
                return Ok(());
            }
            println!(
                "{}",
                ui::bold(&format!("workspace {}", stats.root.display()))
            );
            println!("  checkpoints:    {}", stats.checkpoints);
            println!("  journal entries: {}", stats.journal_entries);
            println!(
                "  forks:          {} active / {} promoted / {} discarded / {} pending / {} total",
                stats.forks_active,
                stats.forks_promoted,
                stats.forks_discarded,
                stats.forks_pending,
                stats.forks_total
            );
            println!(
                "  blobs:          {} ({})",
                stats.blobs,
                human_bytes(stats.blob_bytes)
            );
            println!("  store size:     {}", human_bytes(stats.store_bytes));
            if let Some(last) = &stats.last_operation {
                println!(
                    "  last operation: {} {}",
                    op_name(&last.op),
                    last.duration_ms
                        .map(|ms| format!("({ms} ms)"))
                        .unwrap_or_else(|| "(timing not recorded)".to_string())
                );
            }
            if !stats.recent_operations.is_empty() {
                let mut rows = vec![vec![
                    "WHEN".to_string(),
                    "OP".to_string(),
                    "#".to_string(),
                    "FILES".to_string(),
                    "MS".to_string(),
                    "MESSAGE".to_string(),
                ]];
                for op in &stats.recent_operations {
                    rows.push(vec![
                        op.ts.clone(),
                        op_name(&op.op).to_string(),
                        op.seq.map(|s| format!("#{s}")).unwrap_or_default(),
                        op.files_changed.map(|v| v.to_string()).unwrap_or_default(),
                        op.duration_ms.map(|v| v.to_string()).unwrap_or_default(),
                        op.message.clone().unwrap_or_default(),
                    ]);
                }
                println!();
                print!("{}", ui::table(&rows));
            }
            Ok(())
        }
        Cmd::Quickstart => {
            let report = quickstart_report(cwd(&cli.dir)?);
            if json {
                ui::print_json(true, &report);
            } else {
                print_quickstart(&report);
            }
            Ok(())
        }
        Cmd::Preflight { deep } => {
            let report = preflight_report(&cli.dir, deep)?;
            if json {
                ui::print_json(true, &report);
            } else {
                print_preflight(&report);
            }
            if !report.ready {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Config { command } => match command {
            ConfigCmd::Show => {
                let report = config_show_report(&cli.dir)?;
                if json {
                    ui::print_json(true, &report);
                } else {
                    print_config_show(&report);
                }
                Ok(())
            }
            ConfigCmd::Validate => {
                let report = config_show_report(&cli.dir)?;
                if json {
                    ui::print_json(true, &report);
                } else {
                    print_config_validate(&report);
                }
                Ok(())
            }
        },
        Cmd::Bench { command } => match command {
            BenchCmd::Self_ => {
                let report = bench::self_report(&cwd(&cli.dir)?)?;
                if json {
                    ui::print_json(true, &report);
                } else {
                    bench::print_self_report(&report);
                }
                Ok(())
            }
        },
        Cmd::Schema => {
            let report = schema_report();
            if json {
                ui::print_json(true, &report);
                return Ok(());
            }
            println!("{}", ui::bold("asp schema versions"));
            println!("  asp version: {}", report.asp_version);
            let mut rows = vec![vec![
                "NAME".to_string(),
                "VERSION".to_string(),
                "KIND".to_string(),
                "REFERENCE".to_string(),
            ]];
            for schema in &report.schemas {
                rows.push(vec![
                    schema.name.to_string(),
                    schema.version.to_string(),
                    schema.kind.to_string(),
                    schema.path.to_string(),
                ]);
            }
            print!("{}", ui::table(&rows));
            Ok(())
        }
        Cmd::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            let mut buf = Vec::new();
            clap_complete::generate(shell, &mut cmd, name, &mut buf);
            let completion = String::from_utf8(buf).map_err(|e| {
                Error::new(
                    ErrorCode::Io,
                    format!("completion output was not UTF-8: {e}"),
                )
            })?;
            if json {
                ui::print_json(
                    true,
                    &serde_json::json!({
                        "shell": shell.to_string(),
                        "completion": completion,
                    }),
                );
            } else {
                print!("{completion}");
            }
            Ok(())
        }
        Cmd::Manpage => {
            let cmd = Cli::command();
            let mut buf = Vec::new();
            clap_mangen::Man::new(cmd)
                .render(&mut buf)
                .map_err(|e| Error::new(ErrorCode::Io, format!("render manpage: {e}")))?;
            let manpage = String::from_utf8(buf).map_err(|e| {
                Error::new(ErrorCode::Io, format!("manpage output was not UTF-8: {e}"))
            })?;
            if json {
                ui::print_json(
                    true,
                    &serde_json::json!({
                        "name": "asp",
                        "manpage": manpage,
                    }),
                );
            } else {
                print!("{manpage}");
            }
            Ok(())
        }
        Cmd::Audit(args) => {
            let ws = open(&cli.dir)?;
            let entries = audit_entries(&ws, &args)?;
            if json {
                ui::print_json(true, &entries);
                return Ok(());
            }
            match args.format {
                AuditFormat::Table => print_audit_table(&entries),
                AuditFormat::Jsonl => print_audit_jsonl(&entries),
                AuditFormat::Csv => print_audit_csv(&entries),
            }
        }
        Cmd::Policy { command } => match command {
            PolicyCmd::Validate => {
                let ws = open(&cli.dir)?;
                let report = PolicyValidateReport {
                    path: ws.root().join(".asp/policy.toml"),
                    valid: true,
                    policy: ws.policy.clone(),
                };
                if json {
                    ui::print_json(true, &report);
                    return Ok(());
                }
                println!(
                    "{} policy valid: {}",
                    ui::green("✓"),
                    ui::bold(&report.path.display().to_string())
                );
                let rules = policy_rule_count(&report.policy);
                if rules == 0 {
                    println!("  {}", ui::dim("no local policy rules are set"));
                } else {
                    println!("  active rules: {rules}");
                }
                Ok(())
            }
        },
        Cmd::Secrets { command } => match command {
            SecretsCmd::Scan(args) => {
                let ws = open(&cli.dir)?;
                let report = secrets_scan(&ws, &args)?;
                if json {
                    ui::print_json(true, &report);
                } else {
                    print_secrets_scan(&report);
                }
                if !report.findings.is_empty() {
                    std::process::exit(1);
                }
                Ok(())
            }
        },
        Cmd::Retention { command } => match command {
            RetentionCmd::Plan => {
                let ws = open(&cli.dir)?;
                let plan = ws.retention_plan()?;
                if json {
                    ui::print_json(true, &plan);
                    return Ok(());
                }
                println!("{}", ui::bold("retention plan (dry run)"));
                println!(
                    "  policy: keep_last={}, max_age_days={}",
                    plan.policy
                        .keep_last
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unset".to_string()),
                    plan.policy
                        .max_age_days
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unset".to_string())
                );
                println!(
                    "  checkpoints: {} retained / {} eligible for deletion",
                    plan.retain_count, plan.delete_count
                );
                if plan.checkpoints.is_empty() {
                    println!("  {}", ui::dim("no checkpoints yet"));
                    return Ok(());
                }
                let mut rows = vec![vec![
                    "ACTION".to_string(),
                    "#".to_string(),
                    "AGE".to_string(),
                    "REASON".to_string(),
                    "MESSAGE".to_string(),
                ]];
                for entry in &plan.checkpoints {
                    let action = match entry.action {
                        asp_core::workspace::RetentionAction::Retain => ui::green("retain"),
                        asp_core::workspace::RetentionAction::Delete => ui::yellow("delete"),
                    };
                    rows.push(vec![
                        action,
                        format!("#{}", entry.seq),
                        entry
                            .age_hours
                            .map(|hours| format!("{hours}h"))
                            .unwrap_or_default(),
                        entry.reason.clone(),
                        entry.message.clone().unwrap_or_default(),
                    ]);
                }
                print!("{}", ui::table(&rows));
                Ok(())
            }
        },
        Cmd::Sync { command } => match command {
            SyncCmd::Push { remote } => {
                let ws = open(&cli.dir)?;
                let report = ws.sync_push_local(remote)?;
                if json {
                    ui::print_json(true, &report);
                    return Ok(());
                }
                println!(
                    "{} synced {} checkpoint{} to {}",
                    ui::green("✓"),
                    report.checkpoints,
                    if report.checkpoints == 1 { "" } else { "s" },
                    ui::bold(&report.remote.display().to_string())
                );
                println!(
                    "  git objects: {} uploaded, {} already present",
                    report.git_objects_uploaded, report.git_objects_present
                );
                println!(
                    "  CAS blobs:   {} uploaded, {} already present",
                    report.cas_blobs_uploaded, report.cas_blobs_present
                );
                println!(
                    "  refs:        {} created, {} unchanged, {} updated",
                    report.refs_created, report.refs_present, report.refs_replaced
                );
                Ok(())
            }
            SyncCmd::Fetch { remote } => {
                let ws = open(&cli.dir)?;
                let report = ws.sync_fetch_local(remote)?;
                if json {
                    ui::print_json(true, &report);
                    return Ok(());
                }
                if report.refs_conflicted > 0 {
                    println!(
                        "{} sync fetch found {} conflict{} in {}",
                        ui::yellow("!"),
                        report.refs_conflicted,
                        if report.refs_conflicted == 1 { "" } else { "s" },
                        ui::bold(&report.remote.display().to_string())
                    );
                    for conflict in &report.conflicts {
                        println!(
                            "  {} #{}: local {}, remote {}",
                            conflict.kind,
                            conflict.seq,
                            conflict.local.as_deref().unwrap_or("missing"),
                            conflict.remote.as_deref().unwrap_or("missing")
                        );
                    }
                    println!("  {}", ui::dim("local refs were left untouched"));
                    return Ok(());
                }
                println!(
                    "{} fetched sync remote {}",
                    ui::green("✓"),
                    ui::bold(&report.remote.display().to_string())
                );
                println!(
                    "  refs:        {} imported, {} already present",
                    report.refs_imported, report.refs_present
                );
                println!(
                    "  git objects: {} downloaded, {} already present",
                    report.git_objects_downloaded, report.git_objects_present
                );
                println!(
                    "  CAS blobs:   {} downloaded, {} already present",
                    report.cas_blobs_downloaded, report.cas_blobs_present
                );
                if report.head_updated {
                    if let Some(seq) = report.head_seq {
                        println!("  head:        updated to checkpoint #{seq}");
                    }
                }
                Ok(())
            }
        },
        Cmd::Checkpoint {
            message,
            provenance,
        } => {
            let ws = open(&cli.dir)?;
            let result = ws.checkpoint(CheckpointOpts {
                message,
                source: provenance.source.or(Some(Source::Manual)),
                session_id: provenance.session_id,
                tool: provenance.tool,
            })?;
            match result {
                Some(info) => {
                    if json {
                        ui::print_json(true, &info);
                    } else {
                        println!(
                            "{} checkpoint {} — {} file(s), {} ms",
                            ui::green("✓"),
                            ui::cyan(&format!("#{}", info.seq)),
                            info.files_changed,
                            info.duration_ms
                        );
                    }
                }
                None => {
                    if json {
                        ui::print_json(
                            true,
                            &serde_json::json!({ "no_changes": true, "message": "nothing changed since the last checkpoint" }),
                        );
                    } else {
                        println!("{}", ui::dim("nothing changed since the last checkpoint"));
                    }
                }
            }
            Ok(())
        }
        Cmd::Log { limit } => {
            let ws = open(&cli.dir)?;
            let entries = ws.log(limit)?;
            if json {
                ui::print_json(true, &entries);
                return Ok(());
            }
            if entries.is_empty() {
                println!("{}", ui::dim("no history yet"));
                return Ok(());
            }
            let mut rows = vec![vec![
                "WHEN".to_string(),
                "OP".to_string(),
                "#".to_string(),
                "FILES".to_string(),
                "MESSAGE".to_string(),
            ]];
            for e in &entries {
                let op = match e.op {
                    Op::Checkpoint => ui::green("checkpoint"),
                    Op::Restore => ui::yellow("restore"),
                    Op::Undo => ui::yellow("undo"),
                    Op::Fork => ui::cyan("fork"),
                    Op::Promote => ui::bold("promote"),
                    Op::Discard => ui::dim("discard"),
                    Op::Init => ui::dim("init"),
                };
                let seq = e.seq.map(|s| format!("#{s}")).unwrap_or_default();
                let files = e.files_changed.map(|f| f.to_string()).unwrap_or_default();
                let msg = e
                    .message
                    .clone()
                    .or_else(|| e.detail.as_ref().map(detail_summary))
                    .unwrap_or_default();
                rows.push(vec![e.ts.clone(), op, seq, files, msg]);
            }
            print!("{}", ui::table(&rows));
            Ok(())
        }
        Cmd::Undo => {
            let ws = open(&cli.dir)?;
            let report = ws.undo(Some(Source::Manual))?;
            if json {
                ui::print_json(true, &report);
            } else {
                println!(
                    "{} restored to checkpoint {} ({} files written, {} removed)",
                    ui::green("✓"),
                    ui::cyan(&format!("#{}", report.target_seq)),
                    report.files_written,
                    report.files_deleted
                );
                if let Some(s) = report.safety_seq {
                    println!(
                        "  your previous state was saved as {} — `asp restore {}` brings it back",
                        ui::cyan(&format!("#{s}")),
                        s
                    );
                }
            }
            Ok(())
        }
        Cmd::Restore { checkpoint, paths } => {
            let ws = open(&cli.dir)?;
            let report = ws.restore(&checkpoint, &paths, Some(Source::Manual))?;
            if json {
                ui::print_json(true, &report);
            } else {
                println!(
                    "{} restored {} to checkpoint {}",
                    ui::green("✓"),
                    if paths.is_empty() {
                        "working tree".to_string()
                    } else {
                        format!("{} path(s)", paths.len())
                    },
                    ui::cyan(&format!("#{}", report.target_seq))
                );
                if let Some(s) = report.safety_seq {
                    println!(
                        "  your previous state was saved as {} — `asp restore {}` brings it back",
                        ui::cyan(&format!("#{s}")),
                        s
                    );
                }
            }
            Ok(())
        }
        Cmd::Fork { count, name } => {
            let ws = open(&cli.dir)?;
            let mut infos = Vec::new();
            for i in 0..count {
                let label = match (&name, count) {
                    (Some(n), 1) => Some(n.clone()),
                    (Some(n), _) => Some(format!("{n}-{}", i + 1)),
                    (None, _) => None,
                };
                infos.push(ws.fork(label, Some(Source::Manual))?);
            }
            if json {
                ui::print_json(true, &infos);
                return Ok(());
            }
            for info in &infos {
                println!(
                    "{} fork {} → {} {}",
                    ui::green("✓"),
                    ui::bold(&info.name),
                    info.path.display(),
                    ui::dim(&format!("({:?}, {} ms)", info.method, info.duration_ms))
                );
            }
            if count > 1 {
                println!(
                    "  run your agents in each fork, then {} to compare",
                    ui::cyan("asp forks")
                );
            }
            Ok(())
        }
        Cmd::Forks => {
            let ws = open(&cli.dir)?;
            let rows = ws.fork_compare()?;
            if json {
                ui::print_json(true, &rows);
                return Ok(());
            }
            if rows.is_empty() {
                println!(
                    "{} no active forks — create one with {}",
                    ui::dim("·"),
                    ui::cyan("asp fork")
                );
                return Ok(());
            }
            let mut table = vec![vec![
                "FORK".to_string(),
                "FILES±".to_string(),
                "+LINES".to_string(),
                "-LINES".to_string(),
                "RISK".to_string(),
                "LAST ACTIVITY".to_string(),
                "PATH".to_string(),
            ]];
            for r in &rows {
                table.push(vec![
                    if r.missing {
                        ui::red(&format!("{} (missing)", r.name))
                    } else {
                        ui::bold(&r.name)
                    },
                    r.files_changed.to_string(),
                    ui::green(&format!("+{}", r.insertions)),
                    ui::red(&format!("-{}", r.deletions)),
                    review_cell(&r.review),
                    r.last_activity.clone().unwrap_or_default(),
                    ui::dim(&r.path.display().to_string()),
                ]);
            }
            print!("{}", ui::table(&table));
            println!(
                "\npromote a winner: {}   discard the rest: {}",
                ui::cyan("asp promote <fork>"),
                ui::cyan("asp discard <fork>")
            );
            Ok(())
        }
        Cmd::Review => {
            let ws = open(&cli.dir)?;
            let report = review_report(&ws)?;
            if json {
                ui::print_json(true, &report);
            } else {
                print!("{}", report.markdown);
            }
            Ok(())
        }
        Cmd::Diff {
            patch,
            stat,
            html,
            output,
            fork,
            from,
            to,
        } => {
            let ws = open(&cli.dir)?;
            validate_diff_mode(patch, stat, html, output.as_ref())?;
            if html {
                let output = output.expect("validated --html output");
                let report = if let Some(fork) = fork {
                    if from.is_some() || to.is_some() {
                        return Err(Error::new(
                            ErrorCode::NothingToDo,
                            "`asp diff --fork` does not accept checkpoint arguments",
                        )
                        .with_hint("run `asp diff --fork <name>` or `asp diff <from> [to]`"));
                    }
                    ws.diff_fork_text(&fork, DiffTextMode::Patch)?
                } else {
                    let from = from.ok_or_else(|| {
                        Error::new(ErrorCode::NothingToDo, "diff needs a checkpoint or fork")
                            .with_hint("run `asp diff <from> [to]` or `asp diff --fork <name>`")
                    })?;
                    ws.diff_text(&from, to.as_deref(), DiffTextMode::Patch)?
                };
                let path = resolve_output_path(ws.root(), output);
                let html = render_diff_html(&report);
                write_bytes_file(&path, html.as_bytes())?;
                let result = DiffHtmlOutputResult {
                    from: report.from,
                    to: report.to,
                    summary: report.summary,
                    path: path.clone(),
                    bytes: html.len() as u64,
                };
                if json {
                    ui::print_json(true, &result);
                } else {
                    println!(
                        "{} wrote HTML diff {} ({} bytes)",
                        ui::green("✓"),
                        ui::cyan(&path.display().to_string()),
                        result.bytes
                    );
                }
                return Ok(());
            }
            let mode = diff_text_mode(patch, stat);
            if let Some(fork) = fork {
                if from.is_some() || to.is_some() {
                    return Err(Error::new(
                        ErrorCode::NothingToDo,
                        "`asp diff --fork` does not accept checkpoint arguments",
                    )
                    .with_hint("run `asp diff --fork <name>` or `asp diff <from> [to]`"));
                }
                if let Some(mode) = mode {
                    let report = ws.diff_fork_text(&fork, mode)?;
                    if json {
                        ui::print_json(true, &report);
                    } else {
                        print_diff_text_report(&report);
                    }
                    return Ok(());
                }
                let report = ws.diff_fork(&fork)?;
                if json {
                    ui::print_json(true, &report);
                } else {
                    print_diff_report(&report);
                }
                return Ok(());
            }
            let from = from.ok_or_else(|| {
                Error::new(ErrorCode::NothingToDo, "diff needs a checkpoint or fork")
                    .with_hint("run `asp diff <from> [to]` or `asp diff --fork <name>`")
            })?;
            if let Some(mode) = mode {
                let report = ws.diff_text(&from, to.as_deref(), mode)?;
                if json {
                    ui::print_json(true, &report);
                } else {
                    print_diff_text_report(&report);
                }
                return Ok(());
            }
            let report = ws.diff(&from, to.as_deref())?;
            if json {
                ui::print_json(true, &report);
                return Ok(());
            }
            print_diff_report(&report);
            Ok(())
        }
        Cmd::Promote {
            fork,
            branch,
            push,
            remote,
            pr_draft,
        } => {
            validate_promote_push(push, remote.as_deref(), pr_draft)?;
            let ws = open(&cli.dir)?;
            let mut report = ws.promote(&fork, branch)?;
            if push {
                let remote = remote.as_deref().expect("validated push remote");
                report.push = Some(ws.push_promoted_branch(remote, &report.branch)?);
            }
            if pr_draft {
                report.pr = Some(create_draft_pr(ws.root(), &report.branch));
            }
            if json {
                ui::print_json(true, &report);
            } else {
                println!(
                    "{} fork {} landed as branch {}",
                    ui::green("✓"),
                    ui::bold(&report.fork),
                    ui::cyan(&report.branch)
                );
                println!(
                    "  review it: {}   merge it: {}",
                    ui::cyan(&format!("git diff HEAD...{}", report.branch)),
                    ui::cyan(&format!("git merge {}", report.branch))
                );
                println!(
                    "  fork directory remains: {}   clean up: {}",
                    ui::cyan(&report.fork_path.display().to_string()),
                    ui::cyan(&report.cleanup_command)
                );
                if let Some(push) = &report.push {
                    println!(
                        "  pushed: remote {} branch {} ({})",
                        ui::cyan(&push.remote),
                        ui::cyan(&push.branch),
                        ui::cyan(&push.command)
                    );
                }
                if let Some(pr) = &report.pr {
                    if pr.created {
                        if let Some(url) = &pr.url {
                            println!("  draft PR: {}", ui::cyan(url));
                        }
                    } else {
                        println!("  draft PR not created: {}", pr.message);
                        println!("  fallback: {}", ui::cyan(&pr.fallback_command));
                    }
                }
            }
            Ok(())
        }
        Cmd::Discard { fork, force } => {
            let ws = open(&cli.dir)?;
            ws.discard(&fork, force)?;
            if json {
                ui::print_json(true, &serde_json::json!({ "discarded": fork }));
            } else {
                println!("{} fork {} discarded", ui::green("✓"), ui::bold(&fork));
            }
            Ok(())
        }
        Cmd::Race(args) => match args.action {
            Some(RaceAction::Compare { name }) => {
                let name = if name == "race" { args.name } else { name };
                race::compare(&open(&cli.dir)?, &name, json)
            }
            None => race::run(
                &open(&cli.dir)?,
                race::RunRequest {
                    count: args.count,
                    name: &args.name,
                    labels: &args.labels,
                    env_templates: &args.env,
                    junit_reports: &args.junit_reports,
                    options: race::RunOptions {
                        timeout: args.timeout,
                        retries: args.retries,
                        cancel_on_success: args.cancel_on_success,
                    },
                    command: &args.command,
                    resume: args.resume,
                    json,
                },
            ),
        },
        Cmd::Mcp => {
            if let Some(dir) = &cli.dir {
                std::env::set_current_dir(dir).map_err(|e| {
                    Error::new(
                        ErrorCode::Io,
                        format!("cannot enter {}: {e}", dir.display()),
                    )
                })?;
            }
            mcp::serve()
                .map_err(|e| Error::new(ErrorCode::Io, format!("mcp server I/O error: {e}")))
        }
        Cmd::Setup { harness } => {
            match harness {
                SetupHarness::Claude { user, remove } => {
                    // Setting up implies the directory should be a workspace.
                    let dir = cwd(&cli.dir)?;
                    let root = match Workspace::open(&dir) {
                        Ok(ws) => ws.root().to_path_buf(),
                        Err(_) if !remove => {
                            let ws = Workspace::init(&dir, None)?;
                            if !json {
                                println!(
                                    "{} initialized asp workspace at {}",
                                    ui::green("✓"),
                                    ws.root().display()
                                );
                            }
                            ws.root().to_path_buf()
                        }
                        Err(e) => return Err(e),
                    };
                    let report = hooks::setup_claude(&root, user, remove)?;
                    if json {
                        ui::print_json(true, &report);
                        return Ok(());
                    }
                    if remove {
                        println!("{} Claude Code integration removed", ui::green("✓"));
                        return Ok(());
                    }
                    println!(
                        "{} auto-checkpoint hooks installed → {}",
                        ui::green("✓"),
                        report.settings_file.display()
                    );
                    if let Some(m) = &report.mcp_file {
                        println!("{} MCP server registered → {}", ui::green("✓"), m.display());
                    }
                    println!(
                        "\nEvery file edit and bash command in Claude Code sessions is now \
                     checkpointed.\nTry it: make some changes in a session, then {} or ask the \
                     agent to call {}.",
                        ui::cyan("asp log"),
                        ui::cyan("workspace_undo")
                    );
                    println!(
                    "{}",
                    ui::dim(
                        "note: `asp` must be on PATH for hooks to fire (restart Claude Code after install)"
                    )
                );
                    Ok(())
                }
                SetupHarness::Codex { user, remove } => {
                    // Setting up implies the directory should be a workspace.
                    let dir = cwd(&cli.dir)?;
                    let root = match Workspace::open(&dir) {
                        Ok(ws) => ws.root().to_path_buf(),
                        Err(_) if !remove => {
                            let ws = Workspace::init(&dir, None)?;
                            if !json {
                                println!(
                                    "{} initialized asp workspace at {}",
                                    ui::green("✓"),
                                    ws.root().display()
                                );
                            }
                            ws.root().to_path_buf()
                        }
                        Err(e) => return Err(e),
                    };
                    let report = hooks::setup_codex(&root, user, remove)?;
                    if json {
                        ui::print_json(true, &report);
                        return Ok(());
                    }
                    if remove {
                        println!("{} Codex integration removed", ui::green("✓"));
                        return Ok(());
                    }
                    println!(
                        "{} Codex MCP server registered → {}",
                        ui::green("✓"),
                        report.config_file.display()
                    );
                    if !user {
                        println!(
                        "{}",
                        ui::dim(
                            "note: Codex loads project .codex/config.toml after the project is trusted"
                        )
                    );
                    }
                    println!(
                    "{}",
                    ui::dim("note: restart Codex or open a new session, then use /mcp to inspect servers")
                );
                    Ok(())
                }
                SetupHarness::Opencode { user, remove } => {
                    // Setting up implies the directory should be a workspace.
                    let dir = cwd(&cli.dir)?;
                    let root = match Workspace::open(&dir) {
                        Ok(ws) => ws.root().to_path_buf(),
                        Err(_) if !remove => {
                            let ws = Workspace::init(&dir, None)?;
                            if !json {
                                println!(
                                    "{} initialized asp workspace at {}",
                                    ui::green("✓"),
                                    ws.root().display()
                                );
                            }
                            ws.root().to_path_buf()
                        }
                        Err(e) => return Err(e),
                    };
                    let report = hooks::setup_opencode(&root, user, remove)?;
                    if json {
                        ui::print_json(true, &report);
                        return Ok(());
                    }
                    if remove {
                        println!("{} OpenCode integration removed", ui::green("✓"));
                        return Ok(());
                    }
                    println!(
                        "{} OpenCode MCP server registered → {}",
                        ui::green("✓"),
                        report.config_file.display()
                    );
                    println!(
                    "{}",
                    ui::dim("note: restart OpenCode, then run `opencode mcp list` to inspect servers")
                );
                    Ok(())
                }
            }
        }
        Cmd::HookEvent => {
            hooks::handle_hook_event();
            Ok(())
        }
        Cmd::Doctor {
            fix,
            deep,
            explain,
            runbook,
        } => {
            let ws = open(&cli.dir)?;
            let findings = ws.doctor(fix, deep)?;
            if json {
                if runbook {
                    ui::print_json(true, &doctor_runbook_report(&findings));
                } else {
                    ui::print_json(true, &findings);
                }
                return Ok(());
            }
            if findings.is_empty() {
                println!("{} workspace is healthy", ui::green("✓"));
                if runbook {
                    print_common_doctor_runbooks();
                }
                return Ok(());
            }
            for f in &findings {
                let sev = match f.severity {
                    Severity::Error => ui::red("error"),
                    Severity::Warning => ui::yellow("warning"),
                    Severity::Info => ui::dim("info"),
                };
                let fixed = if f.fixed {
                    format!(" {}", ui::green("[fixed]"))
                } else {
                    String::new()
                };
                println!("{sev}: {}{fixed}", f.message);
                if explain {
                    println!("  cause: {}", f.cause);
                    println!("  next: {}", f.next_action);
                }
                if runbook {
                    let link = doctor_runbook_for_finding(f);
                    println!("  runbook: {} ({})", ui::cyan(link.link), link.scenario);
                }
            }
            if !fix && findings.iter().any(|f| !f.fixed) {
                println!(
                    "\nrun {} to apply safe repairs (not every finding is auto-repairable)",
                    ui::cyan(if deep {
                        "asp doctor --fix --deep"
                    } else {
                        "asp doctor --fix"
                    })
                );
            }
            // Unrepaired error-severity findings: nonzero exit for scripts/CI.
            if findings
                .iter()
                .any(|f| f.severity == Severity::Error && !f.fixed)
            {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Diagnostics {
            include_paths,
            output,
        } => {
            let ws = open(&cli.dir)?;
            let bundle = ws.diagnostics(include_paths)?;
            if let Some(output) = output {
                let path = resolve_output_path(ws.root(), output);
                write_json_file(&path, &bundle)?;
                if json {
                    ui::print_json(
                        true,
                        &serde_json::json!({
                            "path": path,
                            "redacted": !include_paths,
                            "bundle": bundle,
                        }),
                    );
                } else {
                    println!(
                        "{} wrote {}diagnostics bundle to {}",
                        ui::green("✓"),
                        if include_paths { "" } else { "redacted " },
                        ui::bold(&path.display().to_string())
                    );
                }
                return Ok(());
            }
            if json {
                ui::print_json(true, &bundle);
            } else {
                println!("{}", json_pretty(&bundle)?);
            }
            Ok(())
        }
    }
}

/// One-line summary of an op's detail payload for the log table.
fn detail_summary(detail: &serde_json::Value) -> String {
    if let Some(name) = detail.get("name").and_then(|v| v.as_str()) {
        return format!("fork '{name}'");
    }
    if let Some(t) = detail.get("target_seq").and_then(|v| v.as_u64()) {
        return format!("→ #{t}");
    }
    if let Some(b) = detail.get("branch").and_then(|v| v.as_str()) {
        return format!("→ {b}");
    }
    if let Some(f) = detail.get("fork").and_then(|v| v.as_str()) {
        return format!("fork '{f}'");
    }
    String::new()
}

fn op_name(op: &Op) -> &'static str {
    match op {
        Op::Init => "init",
        Op::Checkpoint => "checkpoint",
        Op::Fork => "fork",
        Op::Restore => "restore",
        Op::Undo => "undo",
        Op::Promote => "promote",
        Op::Discard => "discard",
    }
}

fn secrets_scan(ws: &Workspace, args: &SecretsScanArgs) -> Result<SecretsScanReport, Error> {
    let mut report = SecretsScanReport {
        root: ws.root().to_path_buf(),
        files_scanned: 0,
        files_skipped: 0,
        bytes_scanned: 0,
        findings: Vec::new(),
    };
    let excludes = ws.config.shadow_excludes();
    scan_secrets_dir(
        ws.root(),
        ws.root(),
        &excludes,
        args.include_excluded,
        args.max_bytes,
        &mut report,
    )?;
    Ok(report)
}

fn scan_secrets_dir(
    root: &Path,
    dir: &Path,
    excludes: &[String],
    include_excluded: bool,
    max_bytes: u64,
    report: &mut SecretsScanReport,
) -> Result<(), Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = match path.strip_prefix(root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || should_skip_secret_path(&rel, excludes, include_excluded) {
            if file_type.is_file() {
                report.files_skipped += 1;
            }
            continue;
        }
        if file_type.is_dir() {
            scan_secrets_dir(root, &path, excludes, include_excluded, max_bytes, report)?;
        } else if file_type.is_file() {
            scan_secret_file(root, &path, max_bytes, report)?;
        }
    }
    Ok(())
}

fn should_skip_secret_path(rel: &str, excludes: &[String], include_excluded: bool) -> bool {
    let first = rel.split('/').next().unwrap_or("");
    if first == ".asp" || first == ".git" {
        return true;
    }
    if include_excluded {
        return false;
    }
    excludes
        .iter()
        .any(|pattern| secret_exclude_matches(rel, pattern))
}

fn secret_exclude_matches(rel: &str, pattern: &str) -> bool {
    let pattern = pattern.trim_start_matches('/').trim_end_matches('/');
    if pattern.is_empty() {
        return false;
    }
    if rel == pattern || rel.starts_with(&format!("{pattern}/")) {
        return true;
    }
    if pattern.contains('*') {
        return false;
    }
    rel.split('/').any(|component| component == pattern)
}

fn scan_secret_file(
    root: &Path,
    path: &Path,
    max_bytes: u64,
    report: &mut SecretsScanReport,
) -> Result<(), Error> {
    let md = std::fs::metadata(path)?;
    if md.len() > max_bytes {
        report.files_skipped += 1;
        return Ok(());
    }
    let bytes = std::fs::read(path)?;
    if bytes.contains(&0) {
        report.files_skipped += 1;
        return Ok(());
    }
    report.files_scanned += 1;
    report.bytes_scanned += bytes.len() as u64;

    let text = String::from_utf8_lossy(&bytes);
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    for (idx, line) in text.lines().enumerate() {
        if let Some((kind, redacted)) = detect_secret_line(line) {
            report.findings.push(SecretFinding {
                path: rel.clone(),
                line: idx as u64 + 1,
                kind: kind.to_string(),
                redacted,
            });
        }
    }
    Ok(())
}

fn detect_secret_line(line: &str) -> Option<(&'static str, String)> {
    if line.contains("-----BEGIN ") && line.contains(" PRIVATE KEY-----") {
        return Some((
            "private_key",
            "-----BEGIN [redacted] PRIVATE KEY-----".to_string(),
        ));
    }
    for prefix in ["sk-", "sk-proj-"] {
        if let Some((start, end)) = find_prefixed_token(line, prefix, 20) {
            return Some(("openai_key", redact_span(line, start, end)));
        }
    }
    for prefix in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
        if let Some((start, end)) = find_prefixed_token(line, prefix, 20) {
            return Some(("github_token", redact_span(line, start, end)));
        }
    }
    if let Some((start, end)) = find_aws_access_key(line) {
        return Some(("aws_access_key_id", redact_span(line, start, end)));
    }
    if let Some(redacted) = redact_generic_assignment(line) {
        return Some(("secret_assignment", redacted));
    }
    None
}

fn find_prefixed_token(line: &str, prefix: &str, min_tail: usize) -> Option<(usize, usize)> {
    let start = line.find(prefix)?;
    let mut end = start + prefix.len();
    let bytes = line.as_bytes();
    while end < bytes.len() && is_token_byte(bytes[end]) {
        end += 1;
    }
    (end - start >= prefix.len() + min_tail).then_some((start, end))
}

fn find_aws_access_key(line: &str) -> Option<(usize, usize)> {
    let bytes = line.as_bytes();
    for start in 0..bytes.len().saturating_sub(19) {
        let end = start + 20;
        let candidate = &bytes[start..end];
        if (candidate.starts_with(b"AKIA") || candidate.starts_with(b"ASIA"))
            && candidate
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return Some((start, end));
        }
    }
    None
}

fn redact_generic_assignment(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let keys = [
        "api_key",
        "apikey",
        "access_key",
        "secret",
        "token",
        "password",
        "passwd",
    ];
    for key in keys {
        let Some(key_pos) = lower.find(key) else {
            continue;
        };
        let search_from = key_pos + key.len();
        let rest = &line[search_from..];
        let sep_rel = rest.find(['=', ':'])?;
        let sep = search_from + sep_rel;
        let value = line[sep + 1..]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'' || c == '`');
        if value.len() < 8 || looks_placeholder_secret(value) {
            continue;
        }
        return Some(format!(
            "{}{} ***",
            line[..sep].trim_end(),
            &line[sep..=sep]
        ));
    }
    None
}

fn looks_placeholder_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("example")
        || lower.contains("placeholder")
        || lower.contains("changeme")
        || lower.contains('<')
        || lower.contains("...")
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn redact_span(line: &str, start: usize, end: usize) -> String {
    let mut redacted = String::new();
    redacted.push_str(line[..start].trim_start());
    redacted.push_str("[redacted]");
    redacted.push_str(line[end..].trim_end());
    truncate_redacted_line(&redacted)
}

fn truncate_redacted_line(line: &str) -> String {
    const LIMIT: usize = 180;
    let mut chars = line.chars();
    let truncated: String = chars.by_ref().take(LIMIT).collect();
    if chars.next().is_none() {
        line.to_string()
    } else {
        format!("{truncated}...")
    }
}

fn print_secrets_scan(report: &SecretsScanReport) {
    if report.findings.is_empty() {
        println!(
            "{} no likely secrets found ({} files scanned, {} skipped)",
            ui::green("✓"),
            report.files_scanned,
            report.files_skipped
        );
        return;
    }
    println!(
        "{} {} likely secret(s) found ({} files scanned, {} skipped)",
        ui::yellow("!"),
        report.findings.len(),
        report.files_scanned,
        report.files_skipped
    );
    for finding in &report.findings {
        println!(
            "{}:{} [{}] {}",
            finding.path, finding.line, finding.kind, finding.redacted
        );
    }
    println!(
        "\n{} remove the secret, rotate it if real, then checkpoint again",
        ui::cyan("next:")
    );
}

fn audit_entries(ws: &Workspace, args: &AuditArgs) -> Result<Vec<Entry>, Error> {
    let mut entries = ws.journal().read()?.entries;
    entries.reverse();
    let mut filtered = Vec::new();
    for entry in entries {
        if !audit_entry_matches(&entry, args)? {
            continue;
        }
        filtered.push(entry);
        if filtered.len() >= args.limit {
            break;
        }
    }
    Ok(filtered)
}

fn print_audit_table(entries: &[Entry]) -> Result<(), Error> {
    if entries.is_empty() {
        println!("{}", ui::dim("no audit events matched"));
        return Ok(());
    }
    let mut rows = vec![vec![
        "WHEN".to_string(),
        "OP".to_string(),
        "#".to_string(),
        "SESSION".to_string(),
        "TOOL".to_string(),
        "MESSAGE".to_string(),
    ]];
    for entry in entries {
        rows.push(vec![
            entry.ts.clone(),
            op_name(&entry.op).to_string(),
            entry.seq.map(|seq| format!("#{seq}")).unwrap_or_default(),
            entry.session_id.clone().unwrap_or_default(),
            entry.tool.clone().unwrap_or_default(),
            entry
                .message
                .clone()
                .or_else(|| entry.detail.as_ref().map(detail_summary))
                .unwrap_or_default(),
        ]);
    }
    print!("{}", ui::table(&rows));
    Ok(())
}

fn print_audit_jsonl(entries: &[Entry]) -> Result<(), Error> {
    for entry in entries {
        println!("{}", audit_json(entry)?);
    }
    Ok(())
}

fn print_audit_csv(entries: &[Entry]) -> Result<(), Error> {
    println!(
        "{}",
        csv_line(&[
            "v",
            "ts",
            "op",
            "seq",
            "commit",
            "source",
            "session_id",
            "tool",
            "message",
            "files_changed",
            "duration_ms",
            "detail",
        ])
    );
    for entry in entries {
        let detail = match &entry.detail {
            Some(detail) => audit_json(detail)?,
            None => String::new(),
        };
        let fields = vec![
            entry.v.to_string(),
            entry.ts.clone(),
            op_name(&entry.op).to_string(),
            entry.seq.map(|seq| seq.to_string()).unwrap_or_default(),
            entry.commit.clone().unwrap_or_default(),
            entry
                .source
                .as_ref()
                .map(source_name)
                .unwrap_or_default()
                .to_string(),
            entry.session_id.clone().unwrap_or_default(),
            entry.tool.clone().unwrap_or_default(),
            entry.message.clone().unwrap_or_default(),
            entry
                .files_changed
                .map(|value| value.to_string())
                .unwrap_or_default(),
            entry
                .duration_ms
                .map(|value| value.to_string())
                .unwrap_or_default(),
            detail,
        ];
        println!("{}", csv_line(&fields));
    }
    Ok(())
}

fn audit_json(value: &impl serde::Serialize) -> Result<String, Error> {
    serde_json::to_string(value)
        .map_err(|e| Error::new(ErrorCode::Io, format!("audit export encode: {e}")).with_source(e))
}

fn csv_line<T: AsRef<str>>(fields: &[T]) -> String {
    fields
        .iter()
        .map(|field| csv_cell(field.as_ref()))
        .collect::<Vec<_>>()
        .join(",")
}

fn csv_cell(value: &str) -> String {
    if !value.contains([',', '"', '\n', '\r']) {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn source_name(source: &Source) -> &'static str {
    match source {
        Source::Manual => "manual",
        Source::Hook => "hook",
        Source::Mcp => "mcp",
        Source::Race => "race",
    }
}

fn audit_entry_matches(entry: &Entry, args: &AuditArgs) -> Result<bool, Error> {
    if let Some(session) = &args.session {
        if entry.session_id.as_deref() != Some(session.as_str()) {
            return Ok(false);
        }
    }
    if let Some(tool) = &args.tool {
        if entry.tool.as_deref() != Some(tool.as_str()) {
            return Ok(false);
        }
    }
    if !args.ops.is_empty() && !args.ops.iter().any(|op| op == &entry.op) {
        return Ok(false);
    }
    if args.since.is_some() || args.until.is_some() {
        let ts = OffsetDateTime::parse(&entry.ts, &Rfc3339).map_err(|e| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("journal entry timestamp is unreadable: {e}"),
            )
            .with_hint("run `asp doctor`; if the journal cannot be repaired, export a narrower audit range")
        })?;
        if args.since.is_some_and(|since| ts < since) {
            return Ok(false);
        }
        if args.until.is_some_and(|until| ts > until) {
            return Ok(false);
        }
    }
    if !args.paths.is_empty()
        && !args
            .paths
            .iter()
            .any(|path| audit_entry_touches_path(entry, path))
    {
        return Ok(false);
    }
    Ok(true)
}

fn audit_entry_touches_path(entry: &Entry, path: &str) -> bool {
    let Some(detail) = &entry.detail else {
        return false;
    };
    let normalized = path.trim_matches('/');
    if normalized.is_empty() {
        return false;
    }
    if detail_path_matches(detail.get("path"), normalized) {
        return true;
    }
    if let Some(paths) = detail.get("paths").and_then(|value| value.as_array()) {
        return paths
            .iter()
            .any(|value| detail_path_matches(Some(value), normalized));
    }
    false
}

fn detail_path_matches(value: Option<&serde_json::Value>, wanted: &str) -> bool {
    let Some(path) = value.and_then(|value| value.as_str()) else {
        return false;
    };
    let path = path.trim_matches('/');
    path == wanted || path.starts_with(&format!("{wanted}/"))
}

fn parse_op_filter(raw: &str) -> Result<Op, String> {
    match raw {
        "init" => Ok(Op::Init),
        "checkpoint" | "cp" => Ok(Op::Checkpoint),
        "fork" => Ok(Op::Fork),
        "restore" => Ok(Op::Restore),
        "undo" => Ok(Op::Undo),
        "promote" => Ok(Op::Promote),
        "discard" => Ok(Op::Discard),
        _ => Err(
            "operation must be init, checkpoint, fork, restore, undo, promote, or discard"
                .to_string(),
        ),
    }
}

fn parse_rfc3339(raw: &str) -> Result<OffsetDateTime, String> {
    OffsetDateTime::parse(raw, &Rfc3339).map_err(|_| "timestamp must be RFC3339".to_string())
}

fn parse_duration(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("duration cannot be empty".to_string());
    }
    let digit_len = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    if digit_len == 0 {
        return Err("duration must start with a positive integer".to_string());
    }
    let value: u64 = raw[..digit_len]
        .parse()
        .map_err(|_| "duration value is too large".to_string())?;
    if value == 0 {
        return Err("duration must be greater than zero".to_string());
    }
    let unit = raw[digit_len..].trim();
    match unit {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => Ok(Duration::from_secs(value)),
        "ms" | "millisecond" | "milliseconds" => Ok(Duration::from_millis(value)),
        "m" | "min" | "mins" | "minute" | "minutes" => value
            .checked_mul(60)
            .map(Duration::from_secs)
            .ok_or_else(|| "duration value is too large".to_string()),
        _ => Err("duration unit must be ms, s, or m".to_string()),
    }
}

fn policy_rule_count(policy: &asp_core::policy::Policy) -> usize {
    usize::from(policy.forks.max_active.is_some())
        + usize::from(policy.checkpoints.max_age_hours.is_some())
        + policy.paths.protected.len()
        + policy.paths.deny_checkpoint.len()
        + usize::from(policy.promote.require_clean_status)
        + usize::from(policy.promote.require_checkpoint)
        + policy.promote.allowed_branch_prefixes.len()
}

fn review_report(ws: &Workspace) -> Result<ReviewReport, Error> {
    let workspace = ws.status()?;
    let forks = ws.fork_compare()?;
    let generated_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| Error::new(ErrorCode::Io, format!("format review timestamp: {e}")))?;
    let markdown = review_markdown(&workspace, &forks);
    Ok(ReviewReport {
        generated_at,
        workspace,
        forks,
        markdown,
    })
}

fn review_markdown(
    workspace: &asp_core::workspace::StatusReport,
    forks: &[asp_core::workspace::ForkCompareRow],
) -> String {
    let last_checkpoint = workspace
        .last_checkpoint
        .as_ref()
        .map(|checkpoint| {
            let message = checkpoint.message.clone().unwrap_or_default();
            if message.is_empty() {
                format!("#{}", checkpoint.seq)
            } else {
                format!("#{} {}", checkpoint.seq, message)
            }
        })
        .unwrap_or_else(|| "none".to_string());
    let mut out = String::new();
    out.push_str("## agentspaces review\n\n");
    out.push_str(&format!("- Last checkpoint: {last_checkpoint}\n"));
    out.push_str(&format!(
        "- Working tree: {} dirty, {} untracked, {} deleted\n",
        workspace.dirty_files, workspace.untracked_files, workspace.deleted_files
    ));
    out.push_str(&format!("- Active forks: {}\n\n", workspace.active_forks));

    if forks.is_empty() {
        out.push_str("No active forks.\n");
        return out;
    }

    out.push_str("| Fork | Files | Lines | Tests | Risk |\n");
    out.push_str("| --- | ---: | ---: | --- | --- |\n");
    for fork in forks {
        out.push_str(&format!(
            "| {} | {} | +{} / -{} | {} | {} |\n",
            markdown_cell(&fork.name),
            fork.files_changed,
            fork.insertions,
            fork.deletions,
            review_markdown_tests(&fork.review),
            markdown_cell(&review_risk_label(&fork.review))
        ));
    }
    out
}

fn review_markdown_tests(review: &asp_core::workspace::ForkReviewSignals) -> &'static str {
    match review.tests_passed {
        Some(true) => "pass",
        Some(false) => "fail",
        None => "not reported",
    }
}

fn review_risk_label(review: &asp_core::workspace::ForkReviewSignals) -> String {
    if review.risk_markers.is_empty() {
        return "low".to_string();
    }
    let label = review
        .risk_markers
        .iter()
        .map(|marker| marker.kind.as_str())
        .collect::<Vec<_>>()
        .join(",");
    format!("{} {label}", review.risk_score)
}

fn markdown_cell(raw: &str) -> String {
    raw.replace('\n', " ").replace('|', "\\|")
}

fn validate_diff_mode(
    patch: bool,
    stat: bool,
    html: bool,
    output: Option<&PathBuf>,
) -> Result<(), Error> {
    let modes = u8::from(patch) + u8::from(stat) + u8::from(html);
    if modes > 1 {
        return Err(
            Error::new(ErrorCode::NothingToDo, "choose only one diff output mode")
                .with_hint("use one of --patch, --stat, or --html"),
        );
    }
    if html && output.is_none() {
        return Err(
            Error::new(ErrorCode::NothingToDo, "--html needs an output path")
                .with_hint("pass `--output review.html`"),
        );
    }
    if !html && output.is_some() {
        return Err(
            Error::new(ErrorCode::NothingToDo, "--output is only used with --html")
                .with_hint("add `--html`, or remove --output"),
        );
    }
    Ok(())
}

fn validate_promote_push(push: bool, remote: Option<&str>, pr_draft: bool) -> Result<(), Error> {
    if pr_draft && !push {
        return Err(Error::new(
            ErrorCode::NothingToDo,
            "--pr-draft needs --push so the branch exists on a remote",
        )
        .with_hint("retry with `--push --remote <remote>`, for example `asp promote <fork> --push --remote origin --pr-draft`"));
    }
    if push {
        let has_remote = remote.is_some_and(|remote| !remote.trim().is_empty());
        if !has_remote {
            return Err(Error::new(
                ErrorCode::NothingToDo,
                "--push needs an explicit --remote <REMOTE>",
            )
            .with_hint("retry with the remote to push to, for example `asp promote <fork> --push --remote origin`"));
        }
    } else if remote.is_some() {
        return Err(
            Error::new(ErrorCode::NothingToDo, "--remote is only used with --push")
                .with_hint("add --push to push after promote, or remove --remote"),
        );
    }
    Ok(())
}

fn create_draft_pr(root: &Path, branch: &str) -> asp_core::workspace::PromotePrDraftReport {
    let command_parts = ["gh", "pr", "create", "--draft", "--fill", "--head", branch];
    let command = command_parts
        .iter()
        .map(|part| shell_arg(part))
        .collect::<Vec<_>>()
        .join(" ");

    let output = Command::new("gh")
        .arg("pr")
        .arg("create")
        .arg("--draft")
        .arg("--fill")
        .arg("--head")
        .arg(branch)
        .env("GH_PROMPT_DISABLED", "1")
        .env_remove("GH_REPO")
        .current_dir(root)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            asp_core::workspace::PromotePrDraftReport {
                attempted: true,
                created: true,
                url: (!url.is_empty()).then_some(url),
                command: command.clone(),
                fallback_command: command,
                message: "draft pull request created".to_string(),
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                "gh pr create exited unsuccessfully".to_string()
            };
            asp_core::workspace::PromotePrDraftReport {
                attempted: true,
                created: false,
                url: None,
                command: command.clone(),
                fallback_command: command,
                message: format!("gh could not create a draft PR: {detail}"),
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            asp_core::workspace::PromotePrDraftReport {
                attempted: true,
                created: false,
                url: None,
                command: command.clone(),
                fallback_command: command,
                message: "gh is not installed or not on PATH".to_string(),
            }
        }
        Err(err) => asp_core::workspace::PromotePrDraftReport {
            attempted: true,
            created: false,
            url: None,
            command: command.clone(),
            fallback_command: command,
            message: format!("failed to run gh: {err}"),
        },
    }
}

fn shell_arg(raw: &str) -> String {
    if raw
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':'))
    {
        raw.to_string()
    } else {
        format!("'{}'", raw.replace('\'', "'\\''"))
    }
}

fn diff_text_mode(patch: bool, stat: bool) -> Option<DiffTextMode> {
    if patch {
        Some(DiffTextMode::Patch)
    } else if stat {
        Some(DiffTextMode::Stat)
    } else {
        None
    }
}

fn render_diff_html(report: &asp_core::workspace::DiffTextReport) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>agentspaces diff review</title>\n");
    out.push_str(
        "<style>
body{font-family:-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif;margin:0;background:#f7f7f4;color:#191a1f}
main{max-width:1120px;margin:0 auto;padding:32px}
h1{font-size:28px;margin:0 0 8px}
.meta{color:#5b5f6a;margin:0 0 24px}
.summary{display:flex;gap:12px;flex-wrap:wrap;margin:0 0 24px}
.pill{background:#fff;border:1px solid #ddd;border-radius:6px;padding:8px 10px}
pre{background:#111318;color:#e8e8e8;border-radius:8px;overflow:auto;padding:16px;line-height:1.45}
.add{color:#8ee391}.del{color:#ff9b9b}.hunk{color:#8bb8ff}.file{color:#ffd479}
</style>\n",
    );
    out.push_str("</head>\n<body>\n<main>\n");
    out.push_str("<h1>agentspaces diff review</h1>\n");
    out.push_str(&format!(
        "<p class=\"meta\"><strong>{}</strong> to <strong>{}</strong></p>\n",
        html_escape(&report.from),
        html_escape(&report.to)
    ));
    out.push_str("<section class=\"summary\">\n");
    out.push_str(&format!(
        "<div class=\"pill\"><strong>{}</strong> files</div>\n",
        report.summary.files
    ));
    out.push_str(&format!(
        "<div class=\"pill\"><strong>+{}</strong> insertions</div>\n",
        report.summary.insertions
    ));
    out.push_str(&format!(
        "<div class=\"pill\"><strong>-{}</strong> deletions</div>\n",
        report.summary.deletions
    ));
    out.push_str("</section>\n<pre>");
    for line in report.text.lines() {
        let class = if line.starts_with("+++") || line.starts_with("---") {
            "file"
        } else if line.starts_with('+') {
            "add"
        } else if line.starts_with('-') {
            "del"
        } else if line.starts_with("@@") {
            "hunk"
        } else if line.starts_with("diff --git") || line.starts_with("index ") {
            "file"
        } else {
            ""
        };
        if class.is_empty() {
            out.push_str(&html_escape(line));
        } else {
            out.push_str(&format!(
                "<span class=\"{}\">{}</span>",
                class,
                html_escape(line)
            ));
        }
        out.push('\n');
    }
    out.push_str("</pre>\n</main>\n</body>\n</html>\n");
    out
}

fn html_escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn print_diff_report(report: &asp_core::workspace::DiffReport) {
    println!(
        "{} {} → {}",
        ui::bold("diff"),
        ui::cyan(&report.from),
        ui::cyan(&report.to)
    );
    if report.rows.is_empty() {
        println!("{}", ui::dim("no differences"));
        return;
    }
    print_diff_summary(&report.summary);
    let mut table = vec![vec![
        "".to_string(),
        "PATH".to_string(),
        "+".to_string(),
        "-".to_string(),
    ]];
    for row in &report.rows {
        let status = match row.status.as_str() {
            "A" => ui::green("A"),
            "D" => ui::red("D"),
            "M" => ui::yellow("M"),
            other => other.to_string(),
        };
        table.push(vec![
            status,
            row.path.clone(),
            row.insertions
                .map(|v| v.to_string())
                .unwrap_or_else(|| "·".into()),
            row.deletions
                .map(|v| v.to_string())
                .unwrap_or_else(|| "·".into()),
        ]);
    }
    print!("{}", ui::table(&table));
}

fn print_diff_text_report(report: &asp_core::workspace::DiffTextReport) {
    println!(
        "{} {} → {} ({})",
        ui::bold("diff"),
        ui::cyan(&report.from),
        ui::cyan(&report.to),
        report.mode
    );
    if report.summary.files == 0 {
        println!("{}", ui::dim("no differences"));
        return;
    }
    print_diff_summary(&report.summary);
    if report.text.trim().is_empty() {
        println!("{}", ui::dim("no patch output"));
        return;
    }
    print!("{}", report.text);
    if !report.text.ends_with('\n') {
        println!();
    }
}

fn print_diff_summary(summary: &asp_core::workspace::DiffSummary) {
    println!(
        "summary: {} file{}, {}, {}",
        summary.files,
        if summary.files == 1 { "" } else { "s" },
        ui::green(&format!("+{}", summary.insertions)),
        ui::red(&format!("-{}", summary.deletions))
    );
    print_diff_summary_table("path", &summary.by_path);
    print_diff_summary_table("language", &summary.by_language);
    print_diff_summary_table("change", &summary.by_change_type);
}

fn config_show_report(cli_dir: &Option<PathBuf>) -> Result<ConfigShowReport, Error> {
    let root = config_workspace_root(cli_dir)?;
    let path = root.join(ASP_DIR).join("config.toml");
    let config = asp_core::config::Config::load(&path)?;
    Ok(ConfigShowReport {
        root,
        exists: path.is_file(),
        path,
        valid: true,
        shadow_excludes: config.shadow_excludes(),
        blob_threshold_bytes: config.blob_threshold_bytes(),
        config,
    })
}

fn config_workspace_root(cli_dir: &Option<PathBuf>) -> Result<PathBuf, Error> {
    let start = cwd(cli_dir)?;
    let canonical;
    let start = match start.canonicalize() {
        Ok(path) => {
            canonical = path;
            canonical.as_path()
        }
        Err(_) => start.as_path(),
    };
    find_root(start).ok_or_else(|| {
        Error::new(
            ErrorCode::NotAWorkspace,
            format!("no asp workspace found at or above {}", start.display()),
        )
        .with_hint("run `asp init` in your project root to create one")
    })
}

fn print_config_show(report: &ConfigShowReport) {
    println!("{}", ui::bold(&format!("config {}", report.root.display())));
    println!(
        "  file: {} ({})",
        report.path.display(),
        if report.exists {
            "present"
        } else {
            "missing; defaults in effect"
        }
    );
    println!(
        "  blob threshold: {} MiB ({})",
        report.config.capture.blob_threshold_mb,
        human_bytes(report.blob_threshold_bytes)
    );
    println!(
        "  promote branch: {}",
        report.config.promote.branch_template
    );

    let mut rows = vec![vec!["EFFECTIVE CHECKPOINT EXCLUDE".to_string()]];
    rows.extend(
        report
            .shadow_excludes
            .iter()
            .map(|pattern| vec![pattern.clone()]),
    );
    println!();
    print!("{}", ui::table(&rows));
}

fn print_config_validate(report: &ConfigShowReport) {
    println!(
        "{} config valid at {}",
        ui::green("✓"),
        report.path.display()
    );
    println!(
        "  source: {}",
        if report.exists {
            ".asp/config.toml"
        } else {
            "defaults (config file missing)"
        }
    );
}

fn preflight_report(cli_dir: &Option<PathBuf>, deep: bool) -> Result<PreflightReport, Error> {
    let config = config_show_report(cli_dir)?;
    let ws = open(cli_dir)?;
    let policy_rules = policy_rule_count(&ws.policy);
    let doctor_findings = ws.doctor(false, deep)?;
    let doctor_blocking = doctor_findings
        .iter()
        .filter(|finding| finding.severity != Severity::Info)
        .count();
    let secrets = secrets_scan(
        &ws,
        &SecretsScanArgs {
            include_excluded: false,
            max_bytes: 1_048_576,
        },
    )?;
    let secret_count = secrets.findings.len();

    let checks = vec![
        PreflightCheck {
            id: "preflight.config",
            name: "config",
            ok: true,
            summary: format!(
                "{} ({})",
                config.path.display(),
                if config.exists {
                    "present"
                } else {
                    "defaults in effect"
                }
            ),
            runbook: "docs/config.md",
            hint: None,
        },
        PreflightCheck {
            id: "preflight.policy",
            name: "policy",
            ok: true,
            summary: if policy_rules == 0 {
                "valid; no local policy rules are set".to_string()
            } else {
                format!("valid; {policy_rules} active rule(s)")
            },
            runbook: "docs/policy.md",
            hint: None,
        },
        PreflightCheck {
            id: "preflight.doctor",
            name: "doctor",
            ok: doctor_blocking == 0,
            summary: if doctor_findings.is_empty() {
                "workspace is healthy".to_string()
            } else {
                format!(
                    "{} finding(s), {} require attention",
                    doctor_findings.len(),
                    doctor_blocking
                )
            },
            runbook: "docs/doctor-runbook.md",
            hint: (doctor_blocking > 0).then(|| {
                if deep {
                    "run `asp doctor --deep --runbook` for details".to_string()
                } else {
                    "run `asp doctor --runbook` for details".to_string()
                }
            }),
        },
        PreflightCheck {
            id: "preflight.secrets",
            name: "secrets",
            ok: secret_count == 0,
            summary: if secret_count == 0 {
                format!(
                    "{} file(s) scanned; no likely secrets found",
                    secrets.files_scanned
                )
            } else {
                format!("{secret_count} likely secret(s) found")
            },
            runbook: "docs/ignore-config-secrets.md",
            hint: (secret_count > 0).then(|| {
                "run `asp secrets scan` and remove or protect the reported values".to_string()
            }),
        },
    ];
    let ready = checks.iter().all(|check| check.ok);

    Ok(PreflightReport {
        root: ws.root().to_path_buf(),
        ready,
        checks,
        doctor_findings,
        secret_findings: secrets.findings,
    })
}

fn print_preflight(report: &PreflightReport) {
    println!(
        "{}",
        ui::bold(&format!("preflight {}", report.root.display()))
    );
    for check in &report.checks {
        let marker = if check.ok {
            ui::green("✓")
        } else {
            ui::red("✗")
        };
        println!("{marker} {}: {}", check.name, check.summary);
        if let Some(hint) = &check.hint {
            println!("  hint: {hint}");
        }
        if !check.ok {
            println!("  runbook: {}", ui::cyan(check.runbook));
        }
    }
    if report.ready {
        println!("\n{}", ui::green("ready"));
    } else {
        println!("\n{}", ui::red("not ready"));
    }
}

fn quickstart_report(directory: PathBuf) -> QuickstartReport {
    let workspace_root = find_root(&directory);
    let initialized = workspace_root.is_some();
    let mut steps = Vec::new();

    if !initialized {
        steps.push(QuickstartStep {
            title: "Adopt this directory",
            commands: vec!["asp init"],
            purpose: "Creates .asp metadata only; it does not capture files or write your .git.",
        });
    }

    steps.extend([
        QuickstartStep {
            title: "Check the workspace",
            commands: vec!["asp status"],
            purpose: "Shows dirty files, the latest checkpoint, and active forks.",
        },
        QuickstartStep {
            title: "Capture a baseline",
            commands: vec!["asp checkpoint -m \"baseline\""],
            purpose: "Records the current tree so later agent changes are easy to compare or undo.",
        },
        QuickstartStep {
            title: "Wire your agent client",
            commands: vec!["asp setup codex", "asp setup opencode", "asp setup claude"],
            purpose: "Run the one command for the harness you use so checkpoints and MCP tools are available.",
        },
        QuickstartStep {
            title: "Try work in disposable forks",
            commands: vec!["asp race -n 3 -- <agent command>"],
            purpose: "Runs competing attempts side by side without risking the main workspace.",
        },
        QuickstartStep {
            title: "Review and land the winner",
            commands: vec!["asp forks", "asp diff --fork <name>", "asp promote <name>"],
            purpose: "Compares candidates, inspects changes, and promotes the winner as an ordinary git branch.",
        },
        QuickstartStep {
            title: "Recover or diagnose",
            commands: vec!["asp undo", "asp doctor --explain"],
            purpose: "Steps back from bad changes or explains the next repair action when the store needs attention.",
        },
    ]);

    QuickstartReport {
        directory,
        workspace_root,
        initialized,
        steps,
        docs: vec![
            QuickstartDoc {
                title: "Quickstart guide",
                path: "docs/quickstart.md",
            },
            QuickstartDoc {
                title: "Command cheat sheet",
                path: "docs/cheatsheet.md",
            },
            QuickstartDoc {
                title: "30-minute evaluation guide",
                path: "docs/evaluation.md",
            },
        ],
    }
}

fn print_quickstart(report: &QuickstartReport) {
    println!("{}", ui::bold("asp quickstart"));
    println!("  directory: {}", report.directory.display());
    match &report.workspace_root {
        Some(root) => println!("  workspace: {}", root.display()),
        None => println!("  workspace: not initialized"),
    }
    println!();

    for (index, step) in report.steps.iter().enumerate() {
        println!("{}. {}", index + 1, ui::bold(step.title));
        for command in &step.commands {
            println!("   {}", ui::cyan(command));
        }
        println!("   {}", step.purpose);
        println!();
    }

    println!("{}", ui::bold("Docs"));
    for doc in &report.docs {
        println!("  {} - {}", doc.title, doc.path);
    }
}

const OPS_NONE: &[&str] = &[];
const OPS_RESET_SHADOW_GIT_CONFIG: &[&str] = &["reset_shadow_git_config"];
const OPS_TRUNCATE_TORN_JOURNAL_TAIL: &[&str] = &["truncate_torn_journal_tail"];
const OPS_REPOINT_SHADOW_HEAD: &[&str] = &["repoint_shadow_head"];
const OPS_MARK_MISSING_FORK_DISCARDED: &[&str] = &["mark_missing_fork_discarded"];
const OPS_REMOVE_PENDING_FORK: &[&str] = &["remove_pending_fork"];
const OPS_RECREATE_MISSING_CAS_BLOB: &[&str] = &["recreate_missing_cas_blob"];

const DOCTOR_RUNBOOK_GENERAL: DoctorRunbookLink = DoctorRunbookLink {
    scenario: "General doctor triage",
    link: "docs/doctor-runbook.md#general-doctor-triage",
    operations: OPS_NONE,
};

const DOCTOR_RUNBOOKS: &[DoctorRunbookLink] = &[
    DoctorRunbookLink {
        scenario: "Shadow git config drift",
        link: "docs/doctor-runbook.md#shadow-git-config-drift",
        operations: OPS_RESET_SHADOW_GIT_CONFIG,
    },
    DoctorRunbookLink {
        scenario: "Torn journal tail",
        link: "docs/doctor-runbook.md#torn-journal-tail",
        operations: OPS_TRUNCATE_TORN_JOURNAL_TAIL,
    },
    DoctorRunbookLink {
        scenario: "Shadow HEAD drift",
        link: "docs/doctor-runbook.md#shadow-head-drift",
        operations: OPS_REPOINT_SHADOW_HEAD,
    },
    DoctorRunbookLink {
        scenario: "Missing active fork directory",
        link: "docs/doctor-runbook.md#missing-active-fork-directory",
        operations: OPS_MARK_MISSING_FORK_DISCARDED,
    },
    DoctorRunbookLink {
        scenario: "Torn fork clone",
        link: "docs/doctor-runbook.md#torn-fork-clone",
        operations: OPS_REMOVE_PENDING_FORK,
    },
    DoctorRunbookLink {
        scenario: "Missing CAS blob that can be recreated",
        link: "docs/doctor-runbook.md#missing-cas-blob-recreatable",
        operations: OPS_RECREATE_MISSING_CAS_BLOB,
    },
    DoctorRunbookLink {
        scenario: "Journal CRC mismatch",
        link: "docs/doctor-runbook.md#journal-crc-mismatch",
        operations: OPS_NONE,
    },
    DoctorRunbookLink {
        scenario: "Missing checkpoint commit",
        link: "docs/doctor-runbook.md#missing-checkpoint-commit",
        operations: OPS_NONE,
    },
    DoctorRunbookLink {
        scenario: "Promoted fork cleanup",
        link: "docs/doctor-runbook.md#promoted-fork-cleanup",
        operations: OPS_NONE,
    },
    DoctorRunbookLink {
        scenario: "Unregistered fork-looking directory",
        link: "docs/doctor-runbook.md#unregistered-fork-looking-directory",
        operations: OPS_NONE,
    },
    DoctorRunbookLink {
        scenario: "Missing CAS blob and working file",
        link: "docs/doctor-runbook.md#missing-cas-blob-and-working-file",
        operations: OPS_NONE,
    },
    DoctorRunbookLink {
        scenario: "Corrupt CAS blob",
        link: "docs/doctor-runbook.md#corrupt-cas-blob",
        operations: OPS_NONE,
    },
    DoctorRunbookLink {
        scenario: "Runtime prerequisite failure",
        link: "docs/doctor-runbook.md#runtime-prerequisite-failure",
        operations: OPS_NONE,
    },
    DOCTOR_RUNBOOK_GENERAL,
];

fn doctor_runbook_report(findings: &[Finding]) -> DoctorRunbookReport {
    DoctorRunbookReport {
        findings: findings
            .iter()
            .map(|finding| DoctorFindingWithRunbook {
                finding: finding.clone(),
                runbook: doctor_runbook_for_finding(finding),
            })
            .collect(),
        common_runbooks: DOCTOR_RUNBOOKS.to_vec(),
    }
}

fn doctor_runbook_for_finding(finding: &Finding) -> DoctorRunbookLink {
    if let Some(plan) = &finding.repair_plan {
        if let Some(link) = doctor_runbook_by_operation(&plan.operation) {
            return link;
        }
    }

    let message = &finding.message;
    if message.contains("CRC mismatch") {
        doctor_runbook_by_link("docs/doctor-runbook.md#journal-crc-mismatch")
    } else if message.contains("points at missing commit")
        || (message.contains("journal records checkpoint") && message.contains("ref is missing"))
    {
        doctor_runbook_by_link("docs/doctor-runbook.md#missing-checkpoint-commit")
    } else if message.contains("was promoted but its directory still exists") {
        doctor_runbook_by_link("docs/doctor-runbook.md#promoted-fork-cleanup")
    } else if message.contains("looks like a fork of this workspace but is not in the registry") {
        doctor_runbook_by_link("docs/doctor-runbook.md#unregistered-fork-looking-directory")
    } else if message.contains("CAS blob") && message.contains("is missing and the file is gone") {
        doctor_runbook_by_link("docs/doctor-runbook.md#missing-cas-blob-and-working-file")
    } else if message.contains("CAS blob") && message.contains("is corrupt") {
        doctor_runbook_by_link("docs/doctor-runbook.md#corrupt-cas-blob")
    } else if message.contains("hint:") {
        doctor_runbook_by_link("docs/doctor-runbook.md#runtime-prerequisite-failure")
    } else {
        DOCTOR_RUNBOOK_GENERAL
    }
}

fn doctor_runbook_by_operation(operation: &str) -> Option<DoctorRunbookLink> {
    DOCTOR_RUNBOOKS
        .iter()
        .copied()
        .find(|link| link.operations.contains(&operation))
}

fn doctor_runbook_by_link(link: &str) -> DoctorRunbookLink {
    DOCTOR_RUNBOOKS
        .iter()
        .copied()
        .find(|runbook| runbook.link == link)
        .unwrap_or(DOCTOR_RUNBOOK_GENERAL)
}

fn print_common_doctor_runbooks() {
    println!();
    println!("{}", ui::bold("Common runbooks"));
    for link in DOCTOR_RUNBOOKS {
        println!("  {} - {}", link.scenario, ui::cyan(link.link));
    }
}

fn review_cell(review: &asp_core::workspace::ForkReviewSignals) -> String {
    let cell = review_risk_label(review);
    if review.risk_markers.is_empty() {
        return ui::green(&cell);
    }
    if review
        .risk_markers
        .iter()
        .any(|marker| marker.severity == "high")
    {
        ui::red(&cell)
    } else {
        ui::yellow(&cell)
    }
}

fn print_diff_summary_table(title: &str, rows: &[asp_core::workspace::DiffSummaryBucket]) {
    let mut table = vec![vec![
        title.to_ascii_uppercase(),
        "FILES".to_string(),
        "+".to_string(),
        "-".to_string(),
    ]];
    for row in rows {
        table.push(vec![
            row.name.clone(),
            row.files.to_string(),
            ui::green(&format!("+{}", row.insertions)),
            ui::red(&format!("-{}", row.deletions)),
        ]);
    }
    print!("{}", ui::table(&table));
}

fn schema_report() -> SchemaReport {
    SchemaReport {
        asp_version: asp_core::version(),
        schemas: vec![
            SchemaInfo {
                name: "cli_json_envelope",
                version: 1,
                kind: "json_schema",
                path: "schemas/cli-json-envelope.schema.json",
            },
            SchemaInfo {
                name: "result_payloads",
                version: 1,
                kind: "json_schema",
                path: "schemas/asp-result.schema.json",
            },
            SchemaInfo {
                name: "mcp_tool_result",
                version: 1,
                kind: "json_schema",
                path: "schemas/mcp-tool-result.schema.json",
            },
            SchemaInfo {
                name: "workspace_config_toml",
                version: 1,
                kind: "toml_schema_doc",
                path: "docs/config.md",
            },
            SchemaInfo {
                name: "workspace_policy_toml",
                version: 1,
                kind: "toml_schema_doc",
                path: "docs/policy.md",
            },
            SchemaInfo {
                name: "on_disk_format",
                version: FORMAT_VERSION,
                kind: "store_format",
                path: "docs/design/format.md",
            },
        ],
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn resolve_output_path(root: &Path, output: PathBuf) -> PathBuf {
    if output.is_absolute() {
        output
    } else {
        root.join(output)
    }
}

fn write_json_file<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    let parent = path.parent().ok_or_else(|| {
        Error::new(
            ErrorCode::Io,
            format!("output path has no parent: {}", path.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| {
        Error::new(ErrorCode::Io, format!("diagnostics encode: {e}")).with_source(e)
    })?;
    atomic_write(path, &bytes)
}

fn write_bytes_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let parent = path.parent().ok_or_else(|| {
        Error::new(
            ErrorCode::Io,
            format!("output path has no parent: {}", path.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;
    atomic_write(path, bytes)
}

fn json_pretty<T: serde::Serialize>(value: &T) -> Result<String, Error> {
    serde_json::to_string_pretty(value)
        .map_err(|e| Error::new(ErrorCode::Io, format!("diagnostics encode: {e}")).with_source(e))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::parse_duration;

    #[test]
    fn race_timeout_duration_parser_accepts_common_units() {
        assert_eq!(parse_duration("250ms").unwrap(), Duration::from_millis(250));
        assert_eq!(parse_duration("2s").unwrap(), Duration::from_secs(2));
        assert_eq!(parse_duration("3").unwrap(), Duration::from_secs(3));
        assert_eq!(parse_duration("4m").unwrap(), Duration::from_secs(240));
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("1h").is_err());
    }
}
