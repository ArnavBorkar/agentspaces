//! `asp race`: fork N ways, run the same command in each fork in parallel,
//! and render a comparison table — best-of-N as one command. The killer demo
//! and the daily fan-out workflow.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use asp_core::journal::Source;
use asp_core::{Error, ErrorCode, Workspace};
use serde::Serialize;

use crate::ui;

#[derive(Debug, Serialize)]
pub struct LaneResult {
    pub fork: String,
    pub label: String,
    pub path: PathBuf,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub attempts: u32,
    pub timed_out: bool,
    pub canceled: bool,
    pub files_changed: u64,
    pub insertions: u64,
    pub deletions: u64,
    pub log_file: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub struct RunOptions {
    pub timeout: Option<Duration>,
    pub retries: u32,
    pub cancel_on_success: bool,
}

pub struct RunRequest<'a> {
    pub count: u32,
    pub name: &'a str,
    pub labels: &'a [String],
    pub env_templates: &'a [String],
    pub options: RunOptions,
    pub command: &'a [String],
    pub json: bool,
}

#[derive(Debug, Clone)]
struct LaneRun {
    exit_code: Option<i32>,
    duration_ms: u64,
    attempts: u32,
    timed_out: bool,
    canceled: bool,
    log_file: PathBuf,
}

pub fn run(ws: &Workspace, request: RunRequest<'_>) -> Result<(), Error> {
    let RunRequest {
        count,
        name,
        labels,
        env_templates,
        options,
        command,
        json,
    } = request;
    if command.is_empty() {
        return Err(
            Error::new(ErrorCode::NothingToDo, "race needs a command to run").with_hint(
                "pass the command after `--`, e.g.: asp race -n 3 -- claude -p \"fix the test\"",
            ),
        );
    }
    if labels.len() > count as usize {
        return Err(Error::new(
            ErrorCode::NothingToDo,
            format!(
                "received {} lane labels but race only creates {count} lane(s)",
                labels.len()
            ),
        )
        .with_hint("pass at most one --label per lane, or increase --count"));
    }
    if labels.iter().any(|label| label.trim().is_empty()) {
        return Err(
            Error::new(ErrorCode::NothingToDo, "race lane labels cannot be empty")
                .with_hint("pass a non-empty --label value for each labeled lane"),
        );
    }
    let env_templates = parse_env_templates(env_templates)?;
    let max_attempts = options.retries.checked_add(1).ok_or_else(|| {
        Error::new(ErrorCode::NothingToDo, "race retry count is too large")
            .with_hint("pass a smaller --retries value")
    })?;
    let cmd_string = shell_join(command);

    // Create the lanes (forks) up front, sequentially — fork creation takes
    // the workspace lock.
    let mut lanes = Vec::new();
    for i in 1..=count {
        let info = ws.fork(Some(format!("{name}-{i}")), Some(Source::Race))?;
        let label = labels
            .get((i - 1) as usize)
            .cloned()
            .unwrap_or_else(|| info.name.clone());
        if !json {
            println!(
                "{} lane {} → {} ready at {}",
                ui::green("✓"),
                ui::bold(&label),
                ui::cyan(&info.name),
                ui::dim(&info.path.display().to_string())
            );
        }
        lanes.push((i, label, info));
    }
    if !json {
        println!(
            "\n{} running `{}` in {} lanes…\n",
            ui::bold("race:"),
            cmd_string,
            lanes.len()
        );
    }

    // Run all lanes in parallel; each lane's output is captured to a log
    // file inside the fork so the working tree stays clean for diffing.
    let cancel = Arc::new(AtomicBool::new(false));
    let handles: Vec<_> = lanes
        .iter()
        .map(|(lane_index, label, lane)| {
            let path = lane.path.clone();
            let lane_name = lane.name.clone();
            let label = label.clone();
            let env_vars = lane_env(*lane_index, name, &lane_name, &label, &path, &env_templates);
            let cmd = cmd_string.clone();
            let cancel = Arc::clone(&cancel);
            std::thread::spawn(move || {
                let run = run_lane(
                    &path,
                    &lane_name,
                    &cmd,
                    &env_vars,
                    options,
                    max_attempts,
                    cancel,
                );
                (lane_name, run)
            })
        })
        .collect();

    let mut exit_by_lane = std::collections::HashMap::new();
    for handle in handles {
        let (lane_name, run) = handle
            .join()
            .map_err(|_| Error::new(ErrorCode::Io, "race worker thread panicked"))?;
        exit_by_lane.insert(lane_name, run);
    }

    // Compare lanes against their fork points.
    let compare = ws.fork_compare()?;
    let mut results = Vec::new();
    for (_lane_index, label, lane) in &lanes {
        let run = exit_by_lane
            .get(&lane.name)
            .cloned()
            .unwrap_or_else(|| LaneRun {
                exit_code: None,
                duration_ms: 0,
                attempts: 0,
                timed_out: false,
                canceled: true,
                log_file: lane.path.join(".asp/race.log"),
            });
        let row = compare.iter().find(|r| r.name == lane.name);
        results.push(LaneResult {
            fork: lane.name.clone(),
            label: label.clone(),
            path: lane.path.clone(),
            exit_code: run.exit_code,
            duration_ms: run.duration_ms,
            attempts: run.attempts,
            timed_out: run.timed_out,
            canceled: run.canceled,
            files_changed: row.map(|r| r.files_changed).unwrap_or(0),
            insertions: row.map(|r| r.insertions).unwrap_or(0),
            deletions: row.map(|r| r.deletions).unwrap_or(0),
            log_file: run.log_file,
        });
    }

    if json {
        ui::print_json(true, &results);
        return Ok(());
    }

    let mut table = vec![vec![
        "LANE".to_string(),
        "LABEL".to_string(),
        "EXIT".to_string(),
        "TRIES".to_string(),
        "TIME".to_string(),
        "FILES±".to_string(),
        "+LINES".to_string(),
        "-LINES".to_string(),
    ]];
    for r in &results {
        let exit = if r.canceled {
            ui::yellow("canceled")
        } else if r.timed_out {
            ui::yellow("timeout")
        } else {
            match r.exit_code {
                Some(0) => ui::green("0 ✓"),
                Some(c) => ui::red(&format!("{c} ✗")),
                None => ui::red("spawn failed"),
            }
        };
        table.push(vec![
            ui::bold(&r.fork),
            r.label.clone(),
            exit,
            r.attempts.to_string(),
            format!("{:.1}s", r.duration_ms as f64 / 1000.0),
            r.files_changed.to_string(),
            ui::green(&format!("+{}", r.insertions)),
            ui::red(&format!("-{}", r.deletions)),
        ]);
    }
    print!("{}", ui::table(&table));
    println!("\nlogs: {}", ui::dim("<fork>/.asp/race.log"));
    println!(
        "inspect: {}   promote the winner: {}   clean up: {}",
        ui::cyan("asp diff <#> | git -C <fork-path> diff"),
        ui::cyan(&format!("asp promote {name}-N")),
        ui::cyan(&format!("asp discard {name}-N"))
    );
    Ok(())
}

fn run_lane(
    path: &Path,
    lane_name: &str,
    cmd: &str,
    base_env: &[(String, String)],
    options: RunOptions,
    max_attempts: u32,
    cancel: Arc<AtomicBool>,
) -> LaneRun {
    let log_file = path.join(".asp").join("race.log");
    let t0 = Instant::now();
    let mut log = Vec::new();
    let mut exit_code = None;
    let mut attempts = 0;
    let mut timed_out = false;
    let mut canceled = false;

    for attempt in 1..=max_attempts {
        if cancel.load(Ordering::SeqCst) {
            canceled = true;
            break;
        }
        attempts = attempt;
        let mut env_vars = base_env.to_vec();
        env_vars.push(("ASP_RACE_ATTEMPT".to_string(), attempt.to_string()));
        env_vars.push((
            "ASP_RACE_MAX_ATTEMPTS".to_string(),
            max_attempts.to_string(),
        ));
        if max_attempts > 1 {
            append_log_line(
                &mut log,
                &format!("asp race: attempt {attempt}/{max_attempts}"),
            );
        }

        let outcome = run_attempt(path, cmd, env_vars, options.timeout, &cancel);
        log.extend_from_slice(&outcome.log);
        exit_code = outcome.exit_code;

        match outcome.status {
            AttemptStatus::Success => {
                if options.cancel_on_success {
                    cancel.store(true, Ordering::SeqCst);
                }
                timed_out = false;
                canceled = false;
                break;
            }
            AttemptStatus::Failed | AttemptStatus::SpawnFailed => {
                timed_out = false;
                canceled = false;
                if attempt == max_attempts {
                    break;
                }
            }
            AttemptStatus::TimedOut => {
                timed_out = true;
                canceled = false;
                if attempt == max_attempts {
                    break;
                }
            }
            AttemptStatus::Canceled => {
                timed_out = false;
                canceled = true;
                break;
            }
        }
    }

    if attempts == 0 {
        append_log_line(
            &mut log,
            &format!("asp race: lane {lane_name} canceled before first attempt"),
        );
    }
    let _ = std::fs::write(&log_file, log);
    LaneRun {
        exit_code,
        duration_ms: t0.elapsed().as_millis() as u64,
        attempts,
        timed_out,
        canceled,
        log_file,
    }
}

#[derive(Debug)]
struct AttemptOutcome {
    status: AttemptStatus,
    exit_code: Option<i32>,
    log: Vec<u8>,
}

#[derive(Debug)]
enum AttemptStatus {
    Success,
    Failed,
    TimedOut,
    Canceled,
    SpawnFailed,
}

fn run_attempt(
    path: &Path,
    cmd: &str,
    env_vars: Vec<(String, String)>,
    timeout: Option<Duration>,
    cancel: &AtomicBool,
) -> AttemptOutcome {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(cmd)
        .current_dir(path)
        .envs(env_vars)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        command.process_group(0);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let mut log = Vec::new();
            append_log_line(
                &mut log,
                &format!("asp race: failed to spawn lane command: {e}"),
            );
            return AttemptOutcome {
                status: AttemptStatus::SpawnFailed,
                exit_code: None,
                log,
            };
        }
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return collect_completed(child),
            Ok(None) => {}
            Err(e) => {
                let _ = kill_child(&mut child);
                let mut log = collect_child_log(child);
                append_log_line(
                    &mut log,
                    &format!("asp race: failed to wait for command: {e}"),
                );
                return AttemptOutcome {
                    status: AttemptStatus::SpawnFailed,
                    exit_code: None,
                    log,
                };
            }
        }

        if cancel.load(Ordering::SeqCst) {
            let _ = kill_child(&mut child);
            let mut log = collect_child_log(child);
            append_log_line(&mut log, "asp race: attempt canceled");
            return AttemptOutcome {
                status: AttemptStatus::Canceled,
                exit_code: None,
                log,
            };
        }

        if timeout.is_some_and(|deadline| started.elapsed() >= deadline) {
            let _ = kill_child(&mut child);
            let mut log = collect_child_log(child);
            append_log_line(&mut log, "asp race: attempt timed out");
            return AttemptOutcome {
                status: AttemptStatus::TimedOut,
                exit_code: None,
                log,
            };
        }

        std::thread::sleep(Duration::from_millis(25));
    }
}

fn collect_completed(child: Child) -> AttemptOutcome {
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(e) => {
            let mut log = Vec::new();
            append_log_line(
                &mut log,
                &format!("asp race: failed to collect command output: {e}"),
            );
            return AttemptOutcome {
                status: AttemptStatus::SpawnFailed,
                exit_code: None,
                log,
            };
        }
    };
    let exit_code = output.status.code();
    let mut log = output.stdout;
    log.extend_from_slice(&output.stderr);
    AttemptOutcome {
        status: if exit_code == Some(0) {
            AttemptStatus::Success
        } else {
            AttemptStatus::Failed
        },
        exit_code,
        log,
    }
}

fn collect_child_log(child: Child) -> Vec<u8> {
    match child.wait_with_output() {
        Ok(output) => {
            let mut log = output.stdout;
            log.extend_from_slice(&output.stderr);
            log
        }
        Err(e) => {
            let mut log = Vec::new();
            append_log_line(
                &mut log,
                &format!("asp race: failed to collect command output: {e}"),
            );
            log
        }
    }
}

fn kill_child(child: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        let killed_group = unsafe { libc::kill(-pid, libc::SIGKILL) == 0 };
        if killed_group {
            return Ok(());
        }
    }
    child.kill()
}

fn append_log_line(log: &mut Vec<u8>, line: &str) {
    log.extend_from_slice(line.as_bytes());
    log.push(b'\n');
}

fn parse_env_templates(raw: &[String]) -> Result<Vec<(String, String)>, Error> {
    let mut out = Vec::new();
    for entry in raw {
        let (key, value) = entry.split_once('=').ok_or_else(|| {
            Error::new(
                ErrorCode::NothingToDo,
                format!("invalid race env template {entry:?}"),
            )
            .with_hint(
                "use --env KEY=VALUE; VALUE may contain {lane}, {fork}, {label}, {path}, {name}",
            )
        })?;
        if !is_env_key(key) {
            return Err(Error::new(
                ErrorCode::NothingToDo,
                format!("invalid race env key {key:?}"),
            )
            .with_hint(
                "env keys must start with a letter or '_' and contain only letters, digits, and '_'",
            ));
        }
        out.push((key.to_string(), value.to_string()));
    }
    Ok(out)
}

fn is_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn lane_env(
    lane_index: u32,
    race_name: &str,
    fork: &str,
    label: &str,
    path: &Path,
    templates: &[(String, String)],
) -> Vec<(String, String)> {
    let path = path.display().to_string();
    let lane = lane_index.to_string();
    let mut env = vec![
        ("ASP_RACE_LANE".to_string(), lane.clone()),
        ("ASP_RACE_NAME".to_string(), race_name.to_string()),
        ("ASP_RACE_FORK".to_string(), fork.to_string()),
        ("ASP_RACE_LABEL".to_string(), label.to_string()),
        ("ASP_RACE_PATH".to_string(), path.clone()),
    ];
    for (key, value) in templates {
        env.push((
            key.clone(),
            render_template(value, &lane, race_name, fork, label, &path),
        ));
    }
    env
}

fn render_template(
    template: &str,
    lane: &str,
    race_name: &str,
    fork: &str,
    label: &str,
    path: &str,
) -> String {
    template
        .replace("{lane}", lane)
        .replace("{name}", race_name)
        .replace("{fork}", fork)
        .replace("{label}", label)
        .replace("{path}", path)
}

/// Join argv into a shell command. Single-token commands are passed through
/// verbatim (they may already be a shell snippet).
fn shell_join(parts: &[String]) -> String {
    if parts.len() == 1 {
        return parts[0].clone();
    }
    parts
        .iter()
        .map(|p| {
            if p.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c))
            {
                p.clone()
            } else {
                format!("'{}'", p.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use asp_core::ErrorCode;

    use super::{lane_env, parse_env_templates, shell_join};

    #[test]
    fn join_quotes_only_when_needed() {
        assert_eq!(
            shell_join(&["echo hi > out.txt".into()]),
            "echo hi > out.txt"
        );
        assert_eq!(
            shell_join(&["claude".into(), "-p".into(), "fix the test".into()]),
            "claude -p 'fix the test'"
        );
        assert_eq!(
            shell_join(&["echo".into(), "it's".into()]),
            r"echo 'it'\''s'"
        );
    }

    #[test]
    fn env_templates_render_lane_metadata() {
        let templates = parse_env_templates(&[
            "ASP_VARIANT={label}:{lane}:{fork}".into(),
            "ASP_LOCATION={path}".into(),
            "ASP_SUITE={name}".into(),
        ])
        .unwrap();
        let path = PathBuf::from("/tmp/project@variant-2");
        let env = lane_env(2, "variant", "variant-2", "blue", &path, &templates);
        let get = |key: &str| {
            env.iter()
                .find(|(candidate, _)| candidate == key)
                .map(|(_, value)| value.as_str())
                .unwrap()
        };

        assert_eq!(get("ASP_RACE_LANE"), "2");
        assert_eq!(get("ASP_RACE_NAME"), "variant");
        assert_eq!(get("ASP_RACE_FORK"), "variant-2");
        assert_eq!(get("ASP_RACE_LABEL"), "blue");
        assert_eq!(get("ASP_RACE_PATH"), path.display().to_string());
        assert_eq!(get("ASP_VARIANT"), "blue:2:variant-2");
        assert_eq!(get("ASP_LOCATION"), path.display().to_string());
        assert_eq!(get("ASP_SUITE"), "variant");
    }

    #[test]
    fn env_template_keys_are_validated() {
        let err = parse_env_templates(&["1BAD=value".into()]).unwrap_err();
        assert_eq!(err.code, ErrorCode::NothingToDo);
        assert!(err.hint.unwrap().contains("env keys"));

        let err = parse_env_templates(&["NO_EQUALS".into()]).unwrap_err();
        assert_eq!(err.code, ErrorCode::NothingToDo);
        assert!(err.hint.unwrap().contains("KEY=VALUE"));
    }
}
