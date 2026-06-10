//! `asp race`: fork N ways, run the same command in each fork in parallel,
//! and render a comparison table — best-of-N as one command. The killer demo
//! and the daily fan-out workflow.

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use asp_core::journal::Source;
use asp_core::{Error, ErrorCode, Workspace};
use serde::Serialize;

use crate::ui;

#[derive(Debug, Serialize)]
pub struct LaneResult {
    pub fork: String,
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
    let cmd_string = shell_join(command);

    // Create the lanes (forks) up front, sequentially — fork creation takes
    // the workspace lock.
    let mut lanes = Vec::new();
    for i in 1..=count {
        let info = ws.fork(Some(format!("{name}-{i}")), Some(Source::Race))?;
        if !json {
            println!(
                "{} lane {} ready at {}",
                ui::green("✓"),
                ui::bold(&info.name),
                ui::dim(&info.path.display().to_string())
            );
        }
        lanes.push(info);
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
        .map(|lane| {
            let path = lane.path.clone();
            let lane_name = lane.name.clone();
            let cmd = cmd_string.clone();
            std::thread::spawn(move || -> (String, Option<i32>, u64, PathBuf) {
                let log_file = path.join(".asp").join("race.log");
                let t0 = Instant::now();
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .current_dir(&path)
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
    for lane in &lanes {
        let (exit_code, duration_ms, log_file) = exit_by_lane.get(&lane.name).cloned().unwrap_or((
            None,
            0,
            lane.path.join(".asp/race.log"),
        ));
        let row = compare.iter().find(|r| r.name == lane.name);
        results.push(LaneResult {
            fork: lane.name.clone(),
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
    use super::shell_join;

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
}
