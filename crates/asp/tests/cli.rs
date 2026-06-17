//! End-to-end CLI tests against the real binary (CARGO_BIN_EXE_asp).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn asp(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_asp"))
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("asp spawns")
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let out = asp(dir, args);
    assert!(
        out.status.success(),
        "asp {args:?} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn ok_json(dir: &Path, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["--json"];
    full.extend_from_slice(args);
    let stdout = ok(dir, &full);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("bad json from {args:?}: {e}\n{stdout}"))
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/app.py"), "print('v1')\n").unwrap();
    std::fs::write(root.join("README.md"), "# demo\n").unwrap();
    (tmp, root)
}

#[test]
fn full_cli_loop() {
    let (_tmp, root) = project();

    let out = ok(&root, &["init"]);
    assert!(out.contains("initialized"));
    let policy = ok_json(&root, &["policy", "validate"]);
    assert_eq!(policy["result"]["valid"], true);
    assert_eq!(
        policy["result"]["policy"]["paths"]["protected"],
        serde_json::json!([])
    );

    // status before any checkpoint
    let st = ok_json(&root, &["status"]);
    assert_eq!(st["ok"], true);
    assert!(st["result"]["last_checkpoint"].is_null());

    let cp = ok_json(&root, &["checkpoint", "-m", "base"]);
    assert_eq!(cp["result"]["seq"], 1);

    let stats = ok_json(&root, &["stats"]);
    assert_eq!(stats["result"]["checkpoints"], 1);
    assert_eq!(stats["result"]["forks_total"], 0);
    assert!(stats["result"]["store_bytes"].as_u64().unwrap() > 0);
    assert_eq!(stats["result"]["last_operation"]["op"], "checkpoint");
    assert!(ok(&root, &["stats"]).contains("checkpoints"));

    let diagnostics = ok_json(&root, &["diagnostics"]);
    assert_eq!(
        diagnostics["result"]["workspace"]["root"],
        "<workspace-root>"
    );
    assert_eq!(diagnostics["result"]["redaction"]["paths_redacted"], true);
    let report = root.parent().unwrap().join("diagnostics.json");
    ok(
        &root,
        &["diagnostics", "--output", report.to_str().unwrap()],
    );
    let report_json = std::fs::read_to_string(&report).unwrap();
    assert!(
        !report_json.contains(root.to_str().unwrap()),
        "{report_json}"
    );

    // no-op checkpoint exits 0
    let noop = ok_json(&root, &["checkpoint"]);
    assert_eq!(noop["result"]["no_changes"], true);

    // edit + checkpoint + log
    std::fs::write(root.join("src/app.py"), "print('v2')\n").unwrap();
    ok(&root, &["checkpoint", "-m", "v2"]);
    let log = ok(&root, &["log"]);
    assert!(log.contains("checkpoint") && log.contains("v2"));

    // diff between checkpoints
    let diff = ok_json(&root, &["diff", "1", "2"]);
    let rows = diff["result"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], "src/app.py");

    // undo steps back; file content reverts
    ok(&root, &["undo"]);
    assert_eq!(
        std::fs::read_to_string(root.join("src/app.py")).unwrap(),
        "print('v1')\n"
    );

    // fork + forks table
    let forks = ok_json(&root, &["fork", "-n", "2", "--name", "try"]);
    let infos = forks["result"].as_array().unwrap();
    assert_eq!(infos.len(), 2);
    let fork1 = PathBuf::from(infos[0]["path"].as_str().unwrap());
    std::fs::write(fork1.join("src/app.py"), "print('forked')\n").unwrap();

    let table = ok_json(&root, &["forks"]);
    let rows = table["result"].as_array().unwrap();
    assert_eq!(rows.len(), 2);

    // discard guard: refuses without --force, json error has code+hint
    let out = asp(&root, &["--json", "discard", "try-1"]);
    assert!(!out.status.success());
    let err: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(err["ok"], false);
    assert_eq!(err["error"]["code"], "fork_has_unpromoted_work");
    assert!(err["error"]["hint"].as_str().unwrap().contains("promote"));

    ok(&root, &["discard", "try-1", "--force"]);
    ok(&root, &["discard", "try-2"]);

    // doctor: healthy
    let doc = ok_json(&root, &["doctor"]);
    assert_eq!(doc["result"].as_array().unwrap().len(), 0);
}

#[test]
fn policy_validate_reports_invalid_policy() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    std::fs::write(root.join(".asp/policy.toml"), "[forks]\nmax_active = 0\n").unwrap();

    let out = asp(&root, &["--json", "policy", "validate"]);
    assert!(!out.status.success());
    let err: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(err["ok"], false);
    assert_eq!(err["error"]["code"], "store_corrupt");
    assert!(err["error"]["message"]
        .as_str()
        .unwrap()
        .contains("max_active"));
    assert!(err["error"]["hint"]
        .as_str()
        .unwrap()
        .contains("policy.toml"));
}

#[test]
fn race_runs_and_compares() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base"]);

    // Each lane appends its randomish marker — lanes diverge.
    let result = ok_json(
        &root,
        &[
            "race",
            "-n",
            "2",
            "--name",
            "lane",
            "--",
            "sh",
            "-c",
            "echo done-$$ >> src/app.py",
        ],
    );
    let lanes = result["result"].as_array().unwrap();
    assert_eq!(lanes.len(), 2);
    for lane in lanes {
        assert_eq!(lane["exit_code"], 0);
        assert!(lane["label"].as_str().unwrap().starts_with("lane-"));
        assert_eq!(lane["files_changed"], 1);
        assert!(lane["insertions"].as_u64().unwrap() >= 1);
        // log file exists inside the fork
        assert!(PathBuf::from(lane["log_file"].as_str().unwrap()).exists());
    }

    // The parent tree is untouched by the race.
    assert_eq!(
        std::fs::read_to_string(root.join("src/app.py")).unwrap(),
        "print('v1')\n"
    );
}

#[test]
fn race_labels_and_env_templates_reach_lanes() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base"]);

    let result = ok_json(
        &root,
        &[
            "race",
            "-n",
            "2",
            "--name",
            "variant",
            "--label",
            "red",
            "--label",
            "blue",
            "--env",
            "ASP_VARIANT={label}:{lane}:{fork}",
            "--",
            "sh",
            "-c",
            "printf '%s' \"$ASP_RACE_LABEL|$ASP_RACE_LANE|$ASP_RACE_FORK|$ASP_VARIANT\" > lane.txt",
        ],
    );
    let lanes = result["result"].as_array().unwrap();
    assert_eq!(lanes.len(), 2);

    for lane in lanes {
        let label = lane["label"].as_str().unwrap();
        let fork = lane["fork"].as_str().unwrap();
        let path = PathBuf::from(lane["path"].as_str().unwrap());
        let body = std::fs::read_to_string(path.join("lane.txt")).unwrap();
        let parts: Vec<_> = body.split('|').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], label);
        assert_eq!(parts[2], fork);
        assert_eq!(parts[3], format!("{label}:{}:{fork}", parts[1]));
        match label {
            "red" => assert_eq!(parts[1], "1"),
            "blue" => assert_eq!(parts[1], "2"),
            other => panic!("unexpected label {other}"),
        }
    }

    let bad = asp(
        &root,
        &[
            "--json", "race", "-n", "1", "--label", "one", "--label", "two", "--", "true",
        ],
    );
    assert!(!bad.status.success());
    let err: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&bad.stdout)).unwrap();
    assert_eq!(err["ok"], false);
    assert_eq!(err["error"]["code"], "nothing_to_do");
    assert!(err["error"]["hint"].as_str().unwrap().contains("--label"));
}

#[test]
fn race_timeout_retry_and_cancel_controls() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base"]);

    let timed = ok_json(
        &root,
        &[
            "race",
            "-n",
            "1",
            "--name",
            "slow",
            "--timeout",
            "100ms",
            "--",
            "sh",
            "-c",
            "exec sleep 5",
        ],
    );
    let timed_lane = &timed["result"].as_array().unwrap()[0];
    assert!(timed_lane["exit_code"].is_null());
    assert_eq!(timed_lane["attempts"], 1);
    assert_eq!(timed_lane["timed_out"], true);
    assert_eq!(timed_lane["canceled"], false);
    assert!(timed_lane["duration_ms"].as_u64().unwrap() < 3_000);
    let timed_log =
        std::fs::read_to_string(PathBuf::from(timed_lane["log_file"].as_str().unwrap())).unwrap();
    assert!(timed_log.contains("timed out"));

    let retried = ok_json(
        &root,
        &[
            "race",
            "-n",
            "1",
            "--name",
            "retry",
            "--retries",
            "1",
            "--",
            "sh",
            "-c",
            "if [ ! -f attempt ]; then echo first; touch attempt; exit 7; fi; echo retry >> src/app.py",
        ],
    );
    let retry_lane = &retried["result"].as_array().unwrap()[0];
    assert_eq!(retry_lane["exit_code"], 0);
    assert_eq!(retry_lane["attempts"], 2);
    assert_eq!(retry_lane["timed_out"], false);
    assert_eq!(retry_lane["canceled"], false);
    let retry_log =
        std::fs::read_to_string(PathBuf::from(retry_lane["log_file"].as_str().unwrap())).unwrap();
    assert!(retry_log.contains("attempt 1/2"));
    assert!(retry_log.contains("attempt 2/2"));

    let canceled = ok_json(
        &root,
        &[
            "race",
            "-n",
            "2",
            "--name",
            "cancel",
            "--label",
            "fast",
            "--label",
            "slow",
            "--cancel-on-success",
            "--",
            "sh",
            "-c",
            "if [ \"$ASP_RACE_LABEL\" = fast ]; then echo fast >> src/app.py; else exec sleep 5; fi",
        ],
    );
    let lanes = canceled["result"].as_array().unwrap();
    let fast = lanes.iter().find(|lane| lane["label"] == "fast").unwrap();
    let slow = lanes.iter().find(|lane| lane["label"] == "slow").unwrap();
    assert_eq!(fast["exit_code"], 0);
    assert_eq!(fast["attempts"], 1);
    assert_eq!(fast["canceled"], false);
    assert_eq!(slow["canceled"], true);
    assert_eq!(slow["timed_out"], false);
    if let Some(code) = slow["exit_code"].as_i64() {
        assert_ne!(code, 0, "canceled lane must not look successful");
    }
    assert!(slow["duration_ms"].as_u64().unwrap() < 3_000);
}

#[test]
fn race_resume_reruns_only_incomplete_lanes() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base"]);

    let first = ok_json(
        &root,
        &[
            "race",
            "-n",
            "2",
            "--name",
            "resumable",
            "--label",
            "keep",
            "--label",
            "rerun",
            "--",
            "sh",
            "-c",
            "if [ \"$ASP_RACE_LABEL\" = keep ]; then echo keep >> keep.txt; else echo \"$ASP_RACE_ATTEMPT\" >> rerun.txt; fi",
        ],
    );
    let first_lanes = first["result"].as_array().unwrap();
    let keep_path = PathBuf::from(
        first_lanes
            .iter()
            .find(|lane| lane["label"] == "keep")
            .unwrap()["path"]
            .as_str()
            .unwrap(),
    );
    let rerun_path = PathBuf::from(
        first_lanes
            .iter()
            .find(|lane| lane["label"] == "rerun")
            .unwrap()["path"]
            .as_str()
            .unwrap(),
    );

    let metadata_path = root.join(".asp/races/resumable.json");
    let mut metadata: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&metadata_path).unwrap()).unwrap();
    for lane in metadata["lanes"].as_array_mut().unwrap() {
        if lane["label"] == "rerun" {
            lane["status"] = serde_json::json!("running");
            lane["exit_code"] = serde_json::Value::Null;
            lane["attempts"] = serde_json::json!(0);
        }
    }
    std::fs::write(
        &metadata_path,
        serde_json::to_vec_pretty(&metadata).unwrap(),
    )
    .unwrap();

    let resumed = ok_json(&root, &["race", "--name", "resumable", "--resume"]);
    let lanes = resumed["result"].as_array().unwrap();
    let keep = lanes.iter().find(|lane| lane["label"] == "keep").unwrap();
    let rerun = lanes.iter().find(|lane| lane["label"] == "rerun").unwrap();
    assert_eq!(keep["exit_code"], 0);
    assert_eq!(rerun["exit_code"], 0);
    assert_eq!(keep["attempts"], 1);
    assert_eq!(rerun["attempts"], 1);
    assert_eq!(
        std::fs::read_to_string(keep_path.join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert_eq!(
        std::fs::read_to_string(rerun_path.join("rerun.txt")).unwrap(),
        "1\n1\n"
    );
}

#[test]
fn race_ingests_junit_reports() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base"]);

    let result = ok_json(
        &root,
        &[
            "race",
            "-n",
            "1",
            "--name",
            "junit",
            "--label",
            "unit",
            "--junit",
            "{label}.xml",
            "--",
            "sh",
            "-c",
            "printf '%s\\n' '<testsuite tests=\"4\" failures=\"1\" errors=\"0\" skipped=\"1\" time=\"0.25\" />' > \"$ASP_RACE_LABEL.xml\"",
        ],
    );
    let lane = &result["result"].as_array().unwrap()[0];
    let tests = &lane["tests"];
    assert_eq!(tests["reports"], 1);
    assert_eq!(tests["tests"], 4);
    assert_eq!(tests["failures"], 1);
    assert_eq!(tests["errors"], 0);
    assert_eq!(tests["skipped"], 1);
    assert_eq!(tests["time_seconds"], 0.25);
}

#[test]
fn race_compare_reranks_saved_lanes() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base"]);

    let first = ok_json(
        &root,
        &[
            "race",
            "-n",
            "2",
            "--name",
            "ranked",
            "--label",
            "fail",
            "--label",
            "pass",
            "--junit",
            "{label}.xml",
            "--",
            "sh",
            "-c",
            "if [ \"$ASP_RACE_LABEL\" = pass ]; then printf '%s\\n' '<testsuite tests=\"4\" failures=\"0\" errors=\"0\" skipped=\"0\" time=\"0.10\" />' > \"$ASP_RACE_LABEL.xml\"; echo pass >> src/app.py; else printf '%s\\n' '<testsuite tests=\"4\" failures=\"1\" errors=\"0\" skipped=\"0\" time=\"0.20\" />' > \"$ASP_RACE_LABEL.xml\"; echo fail >> src/app.py; fi",
        ],
    );
    let initial_lanes = first["result"].as_array().unwrap();
    assert_eq!(initial_lanes[0]["label"], "fail");
    assert_eq!(initial_lanes[1]["label"], "pass");
    assert!(initial_lanes[0]["rank"].is_null());
    let fail_path = PathBuf::from(initial_lanes[0]["path"].as_str().unwrap());
    std::fs::write(fail_path.join("manual-review.txt"), "needs follow-up\n").unwrap();

    let compared = ok_json(&root, &["race", "compare", "--name", "ranked"]);
    let lanes = compared["result"].as_array().unwrap();
    assert_eq!(lanes[0]["label"], "pass");
    assert_eq!(lanes[0]["rank"], 1);
    assert_eq!(lanes[0]["tests"]["failures"], 0);
    assert_eq!(lanes[0]["attempts"], 1);
    assert_eq!(lanes[1]["label"], "fail");
    assert_eq!(lanes[1]["rank"], 2);
    assert_eq!(lanes[1]["tests"]["failures"], 1);
    assert!(lanes[1]["files_changed"].as_u64().unwrap() >= 3);

    let compared_prefix = ok_json(&root, &["race", "--name", "ranked", "compare"]);
    assert_eq!(compared_prefix["result"][0]["label"], "pass");
}

#[test]
fn promote_via_cli_lands_branch() {
    let (_tmp, root) = project();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "u@example.com"]);
    git(&["config", "user.name", "U"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "init"]);

    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base"]);
    let forks = ok_json(&root, &["fork", "--name", "winner"]);
    let fork_path = PathBuf::from(forks["result"][0]["path"].as_str().unwrap());
    std::fs::write(fork_path.join("src/app.py"), "print('better')\n").unwrap();

    let p = ok_json(&root, &["promote", "winner"]);
    assert_eq!(p["result"]["branch"], "asp/winner");
    let content = git(&["show", "asp/winner:src/app.py"]);
    assert_eq!(content, "print('better')");
}

#[test]
fn errors_are_actionable_outside_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let out = asp(tmp.path(), &["status"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error:"), "{stderr}");
    assert!(
        stderr.contains("hint:") && stderr.contains("asp init"),
        "{stderr}"
    );
}

#[test]
fn restore_targeted_path_via_cli() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base"]);
    std::fs::write(root.join("src/app.py"), "broken\n").unwrap();
    std::fs::write(root.join("README.md"), "# changed\n").unwrap();
    ok(&root, &["checkpoint", "-m", "damage"]);

    ok(&root, &["restore", "1", "src/app.py"]);
    assert_eq!(
        std::fs::read_to_string(root.join("src/app.py")).unwrap(),
        "print('v1')\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "# changed\n"
    );
}
