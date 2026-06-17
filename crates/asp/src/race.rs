//! `asp race`: fork N ways, run the same command in each fork in parallel,
//! and render a comparison table — best-of-N as one command. The killer demo
//! and the daily fan-out workflow.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

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
    pub files_changed: u64,
    pub insertions: u64,
    pub deletions: u64,
    pub log_file: PathBuf,
}

pub fn run(
    ws: &Workspace,
    count: u32,
    name: &str,
    labels: &[String],
    env_templates: &[String],
    command: &[String],
    json: bool,
) -> Result<(), Error> {
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
    let handles: Vec<_> = lanes
        .iter()
        .map(|(lane_index, label, lane)| {
            let path = lane.path.clone();
            let lane_name = lane.name.clone();
            let label = label.clone();
            let env_vars = lane_env(*lane_index, name, &lane_name, &label, &path, &env_templates);
            let cmd = cmd_string.clone();
            std::thread::spawn(move || -> (String, Option<i32>, u64, PathBuf) {
                let log_file = path.join(".asp").join("race.log");
                let t0 = Instant::now();
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .current_dir(&path)
                    .envs(env_vars)
                    .output();
                let duration = t0.elapsed().as_millis() as u64;
                match output {
                    Ok(out) => {
                        let mut log = Vec::new();
                        log.extend_from_slice(&out.stdout);
                        log.extend_from_slice(&out.stderr);
                        let _ = std::fs::write(&log_file, &log);
                        (lane_name, out.status.code(), duration, log_file)
                    }
                    Err(e) => {
                        let _ = std::fs::write(&log_file, format!("failed to spawn: {e}"));
                        (lane_name, None, duration, log_file)
                    }
                }
            })
        })
        .collect();

    let mut exit_by_lane = std::collections::HashMap::new();
    for handle in handles {
        let (lane_name, code, duration, log_file) = handle
            .join()
            .map_err(|_| Error::new(ErrorCode::Io, "race worker thread panicked"))?;
        exit_by_lane.insert(lane_name, (code, duration, log_file));
    }

    // Compare lanes against their fork points.
    let compare = ws.fork_compare()?;
    let mut results = Vec::new();
    for (_lane_index, label, lane) in &lanes {
        let (exit_code, duration_ms, log_file) = exit_by_lane.get(&lane.name).cloned().unwrap_or((
            None,
            0,
            lane.path.join(".asp/race.log"),
        ));
        let row = compare.iter().find(|r| r.name == lane.name);
        results.push(LaneResult {
            fork: lane.name.clone(),
            label: label.clone(),
            path: lane.path.clone(),
            exit_code,
            duration_ms,
            files_changed: row.map(|r| r.files_changed).unwrap_or(0),
            insertions: row.map(|r| r.insertions).unwrap_or(0),
            deletions: row.map(|r| r.deletions).unwrap_or(0),
            log_file,
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
        "TIME".to_string(),
        "FILES±".to_string(),
        "+LINES".to_string(),
        "-LINES".to_string(),
    ]];
    for r in &results {
        let exit = match r.exit_code {
            Some(0) => ui::green("0 ✓"),
            Some(c) => ui::red(&format!("{c} ✗")),
            None => ui::red("spawn failed"),
        };
        table.push(vec![
            ui::bold(&r.fork),
            r.label.clone(),
            exit,
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
