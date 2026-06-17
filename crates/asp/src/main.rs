//! asp — agentspaces CLI: instant, disposable, fully-reviewable forks of your
//! real working directory for AI agents.
//!
//! Agents are first-class users: every command supports `--json`, and every
//! error states the corrective next action.

mod hooks;
mod mcp;
mod race;
mod ui;

use std::path::{Path, PathBuf};
use std::time::Duration;

use asp_core::journal::{Op, Source};
use asp_core::store::{atomic_write, FORMAT_VERSION};
use asp_core::workspace::{CheckpointOpts, Severity};
use asp_core::{Error, ErrorCode, Workspace};
use clap::{Args, Parser, Subcommand};

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
    /// Print supported schema and format versions.
    Schema,
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
    /// Show what changed between two checkpoints, or a checkpoint and now.
    Diff {
        /// From: #seq or commit prefix.
        from: String,
        /// To: #seq or commit prefix (default: the working tree).
        to: Option<String>,
    },
    /// Land a fork's work as an ordinary git branch in this repo.
    Promote {
        /// Fork name (see `asp forks`).
        fork: String,
        /// Branch name to create (default: asp/<fork>).
        #[arg(long)]
        branch: Option<String>,
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
    Race {
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
        /// Per-attempt timeout, such as 500ms, 30s, 2m, or bare seconds.
        #[arg(long, value_name = "DURATION", value_parser = parse_duration)]
        timeout: Option<Duration>,
        /// Retry failed or timed-out lanes this many times.
        #[arg(long, default_value_t = 0)]
        retries: u32,
        /// Stop still-running lanes after the first successful lane exits 0.
        #[arg(long)]
        cancel_on_success: bool,
        /// The command to run in each fork (everything after --).
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Check workspace health; --fix applies safe repairs.
    Doctor {
        /// Apply safe repairs.
        #[arg(long)]
        fix: bool,
        /// Re-hash large-file CAS blobs to detect silent corruption.
        #[arg(long)]
        deep: bool,
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
        Cmd::Diff { from, to } => {
            let ws = open(&cli.dir)?;
            let report = ws.diff(&from, to.as_deref())?;
            if json {
                ui::print_json(true, &report);
                return Ok(());
            }
            println!(
                "{} {} → {}",
                ui::bold("diff"),
                ui::cyan(&report.from),
                ui::cyan(&report.to)
            );
            if report.rows.is_empty() {
                println!("{}", ui::dim("no differences"));
                return Ok(());
            }
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
            Ok(())
        }
        Cmd::Promote { fork, branch } => {
            let ws = open(&cli.dir)?;
            let report = ws.promote(&fork, branch)?;
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
        Cmd::Race {
            count,
            name,
            labels,
            env,
            timeout,
            retries,
            cancel_on_success,
            command,
        } => race::run(
            &open(&cli.dir)?,
            race::RunRequest {
                count,
                name: &name,
                labels: &labels,
                env_templates: &env,
                options: race::RunOptions {
                    timeout,
                    retries,
                    cancel_on_success,
                },
                command: &command,
                json,
            },
        ),
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
        Cmd::Setup { harness } => match harness {
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
        },
        Cmd::HookEvent => {
            hooks::handle_hook_event();
            Ok(())
        }
        Cmd::Doctor { fix, deep } => {
            let ws = open(&cli.dir)?;
            let findings = ws.doctor(fix, deep)?;
            if json {
                ui::print_json(true, &findings);
                return Ok(());
            }
            if findings.is_empty() {
                println!("{} workspace is healthy", ui::green("✓"));
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
