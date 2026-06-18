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

fn shadow_git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("--git-dir")
        .arg(root.join(".asp/shadow.git"))
        .args(args)
        .output()
        .expect("git spawns");
    assert!(
        out.status.success(),
        "git {args:?} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
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
fn bench_self_runs_outside_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let human = ok(tmp.path(), &["bench", "self"]);
    assert!(human.contains("bench self"));
    assert!(human.contains("dir clone"));

    let json = ok_json(tmp.path(), &["bench", "self"]);
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["result"]["path"],
        tmp.path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert!(json["result"]["platform"]["supported"].is_boolean());
    assert!(json["result"]["filesystem"]["atomic_rename"].is_boolean());
}

#[test]
fn quickstart_is_context_aware_and_json() {
    let tmp = tempfile::tempdir().unwrap();

    let human = ok(tmp.path(), &["quickstart"]);
    assert!(human.contains("workspace: not initialized"), "{human}");
    assert!(human.contains("asp init"), "{human}");
    assert!(human.contains("asp checkpoint -m \"baseline\""), "{human}");
    assert!(human.contains("docs/quickstart.md"), "{human}");

    let json = ok_json(tmp.path(), &["quickstart"]);
    assert_eq!(json["ok"], true);
    assert_eq!(json["result"]["initialized"], false);
    assert!(json["result"]["workspace_root"].is_null());
    assert_eq!(
        json["result"]["steps"][0]["commands"][0],
        serde_json::json!("asp init")
    );

    ok(tmp.path(), &["init"]);
    let initialized = ok_json(tmp.path(), &["quickstart"]);
    assert_eq!(initialized["result"]["initialized"], true);
    assert_eq!(
        initialized["result"]["workspace_root"],
        serde_json::json!(tmp.path())
    );
    assert_eq!(
        initialized["result"]["steps"][0]["title"],
        serde_json::json!("Check the workspace")
    );
}

#[test]
fn completions_emit_shell_scripts_and_json() {
    let tmp = tempfile::tempdir().unwrap();
    let bash = ok(tmp.path(), &["completions", "bash"]);
    assert!(bash.contains("_asp"), "{bash}");
    assert!(bash.contains("completions"), "{bash}");

    let json = ok_json(tmp.path(), &["completions", "zsh"]);
    assert_eq!(json["ok"], true);
    assert_eq!(json["result"]["shell"], "zsh");
    let completion = json["result"]["completion"].as_str().unwrap();
    assert!(completion.contains("#compdef asp"), "{completion}");
    assert!(completion.contains("completions"), "{completion}");
}

#[test]
fn manpage_emits_roff_and_json() {
    let tmp = tempfile::tempdir().unwrap();
    let roff = ok(tmp.path(), &["manpage"]);
    assert!(roff.contains(".TH"), "{roff}");
    assert!(roff.contains("asp"), "{roff}");
    assert!(roff.contains("completions"), "{roff}");

    let json = ok_json(tmp.path(), &["manpage"]);
    assert_eq!(json["ok"], true);
    assert_eq!(json["result"]["name"], "asp");
    let manpage = json["result"]["manpage"].as_str().unwrap();
    assert!(manpage.contains(".TH"), "{manpage}");
    assert!(manpage.contains("manpage"), "{manpage}");
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
    assert_eq!(diff["result"]["summary"]["files"], 1);
    assert_eq!(
        diff["result"]["summary"]["by_language"][0]["name"],
        "Python"
    );
    assert!(ok(&root, &["diff", "--stat", "1", "2"]).contains("src/app.py"));
    let patch = ok_json(&root, &["diff", "--patch", "1", "2"]);
    assert_eq!(patch["result"]["mode"], "patch");
    assert!(patch["result"]["text"]
        .as_str()
        .unwrap()
        .contains("src/app.py"));
    let html_out = root.parent().unwrap().join("review.html");
    let html = ok_json(
        &root,
        &[
            "diff",
            "--html",
            "--output",
            html_out.to_str().unwrap(),
            "1",
            "2",
        ],
    );
    assert_eq!(html["result"]["summary"]["files"], 1);
    let html_path = PathBuf::from(html["result"]["path"].as_str().unwrap());
    assert!(html_path.exists());
    assert!(std::fs::read_to_string(&html_path)
        .unwrap()
        .contains("agentspaces diff review"));

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
    assert_eq!(rows[0]["review"]["files_touched"], 1);
    assert!(rows[0]["review"]["tests_passed"].is_null());
    let fork_diff = ok_json(&root, &["diff", "--fork", "try-1"]);
    assert_eq!(fork_diff["result"]["to"], "fork try-1");
    assert!(fork_diff["result"]["rows"][0]["path"]
        .as_str()
        .unwrap()
        .contains("src/app.py"));
    assert!(ok(&root, &["diff", "--fork", "try-1", "--stat"]).contains("src/app.py"));
    let fork_html_out = root.parent().unwrap().join("fork-review.html");
    ok(
        &root,
        &[
            "diff",
            "--fork",
            "try-1",
            "--html",
            "--output",
            fork_html_out.to_str().unwrap(),
        ],
    );
    assert!(fork_html_out.exists());
    let review = ok_json(&root, &["review"]);
    assert_eq!(review["result"]["forks"].as_array().unwrap().len(), 2);
    assert!(review["result"]["markdown"]
        .as_str()
        .unwrap()
        .contains("| Fork | Files | Lines | Tests | Risk |"));

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
fn doctor_explain_reports_cause_and_next_action() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);

    let drift = Command::new("git")
        .arg("--git-dir")
        .arg(root.join(".asp/shadow.git"))
        .args(["config", "core.compression", "9"])
        .output()
        .unwrap();
    assert!(
        drift.status.success(),
        "git config failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&drift.stdout),
        String::from_utf8_lossy(&drift.stderr)
    );

    let human = ok(&root, &["doctor", "--explain"]);
    assert!(human.contains("shadow git config"));
    assert!(human.contains("  cause:"));
    assert!(human.contains("  next:"));
    assert!(human.contains("asp doctor --fix"));

    let json = ok_json(&root, &["doctor"]);
    let finding = json["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| {
            finding["message"]
                .as_str()
                .unwrap()
                .contains("shadow git config core.compression")
        })
        .expect("shadow git config finding");
    assert_eq!(finding["severity"], "warning");
    assert!(finding["cause"]
        .as_str()
        .unwrap()
        .contains("shadow git repository"));
    assert!(finding["next_action"]
        .as_str()
        .unwrap()
        .contains("asp doctor --fix"));
    assert_eq!(
        finding["repair_plan"]["operation"],
        "reset_shadow_git_config"
    );
    assert_eq!(finding["repair_plan"]["command"], "asp doctor --fix");
    assert_eq!(finding["repair_plan"]["destructive"], false);
    assert_eq!(finding["fixed"], false);

    let fixed_json = ok_json(&root, &["doctor", "--fix"]);
    let fixed_finding = fixed_json["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| {
            finding["message"]
                .as_str()
                .unwrap()
                .contains("shadow git config core.compression")
        })
        .expect("fixed shadow git config finding");
    assert_eq!(fixed_finding["fixed"], true);
    assert_eq!(
        fixed_finding["repair_plan"]["operation"],
        "reset_shadow_git_config"
    );

    let drift = Command::new("git")
        .arg("--git-dir")
        .arg(root.join(".asp/shadow.git"))
        .args(["config", "core.compression", "9"])
        .output()
        .unwrap();
    assert!(drift.status.success());

    let fixed = ok(&root, &["doctor", "--fix", "--explain"]);
    assert!(fixed.contains("[fixed]"));
    assert!(fixed.contains("no further action is needed"));
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
fn secrets_scan_reports_redacted_findings() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);

    let ignored_secret = "OPENAI_API_KEY=sk-ignored12345678901234567890\n";
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join("target/secret.txt"), ignored_secret).unwrap();
    let clean = ok(&root, &["secrets", "scan"]);
    assert!(clean.contains("no likely secrets found"), "{clean}");

    let secret = "sk-live123456789012345678901234567890";
    std::fs::write(
        root.join("src/config.py"),
        format!("OPENAI_API_KEY={secret}\n"),
    )
    .unwrap();

    let human = asp(&root, &["secrets", "scan"]);
    assert!(!human.status.success(), "scanner should fail on findings");
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("src/config.py:1 [openai_key]"), "{stdout}");
    assert!(stdout.contains("[redacted]"), "{stdout}");
    assert!(
        !stdout.contains(secret),
        "scanner leaked the secret: {stdout}"
    );
    assert!(
        !stdout.contains("sk-ignored"),
        "excluded files should be skipped"
    );

    let json = asp(&root, &["--json", "secrets", "scan"]);
    assert!(
        !json.status.success(),
        "JSON scanner should fail on findings"
    );
    let body: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(body["ok"], true);
    let findings = body["result"]["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1, "{body}");
    assert_eq!(findings[0]["kind"], "openai_key");
    assert!(
        !serde_json::to_string(&body).unwrap().contains(secret),
        "JSON scanner leaked the secret: {body}"
    );
}

#[test]
fn invalid_policy_blocks_destructive_commands_before_mutation() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base"]);
    std::fs::write(root.join("src/app.py"), "print('damage')\n").unwrap();
    ok(&root, &["checkpoint", "-m", "damage"]);
    let fork = ok_json(&root, &["fork", "--name", "guard"]);
    let fork_path = PathBuf::from(fork["result"][0]["path"].as_str().unwrap());
    assert!(fork_path.exists());

    std::fs::write(
        root.join(".asp/policy.toml"),
        "[paths]\nprotected = [\"../escape\"]\n",
    )
    .unwrap();

    let restore = asp(&root, &["--json", "restore", "1"]);
    assert!(!restore.status.success());
    let restore_err: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&restore.stdout)).unwrap();
    assert_eq!(restore_err["error"]["code"], "store_corrupt");
    assert_eq!(
        std::fs::read_to_string(root.join("src/app.py")).unwrap(),
        "print('damage')\n"
    );

    let discard = asp(&root, &["--json", "discard", "guard", "--force"]);
    assert!(!discard.status.success());
    let discard_err: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&discard.stdout)).unwrap();
    assert_eq!(discard_err["error"]["code"], "store_corrupt");
    assert!(fork_path.exists(), "invalid policy must not permit discard");
}

#[test]
fn retention_plan_reports_dry_run_from_policy() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base"]);
    std::fs::write(root.join("src/app.py"), "print('v2')\n").unwrap();
    ok(&root, &["checkpoint", "-m", "v2"]);
    std::fs::write(
        root.join(".asp/policy.toml"),
        "[retention]\nkeep_last = 1\n",
    )
    .unwrap();

    let plan = ok_json(&root, &["retention", "plan"]);
    assert_eq!(plan["result"]["dry_run"], true);
    assert_eq!(plan["result"]["total_checkpoints"], 2);
    assert_eq!(plan["result"]["delete_count"], 1);
    let checkpoints = plan["result"]["checkpoints"].as_array().unwrap();
    let old = checkpoints.iter().find(|entry| entry["seq"] == 1).unwrap();
    assert_eq!(old["action"], "delete");
    assert_eq!(old["reason"], "outside_keep_last");
    let newest = checkpoints.iter().find(|entry| entry["seq"] == 2).unwrap();
    assert_eq!(newest["action"], "retain");

    let table = ok(&root, &["retention", "plan"]);
    assert!(table.contains("retention plan"));
    assert!(table.contains("outside_keep_last"));
}

#[test]
fn sync_push_uploads_checkpoints_and_blobs_to_local_remote() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    std::fs::write(
        root.join(".asp/config.toml"),
        "[capture]\nblob_threshold_mb = 1\n",
    )
    .unwrap();
    let big: Vec<u8> = (0..(2 * 1024 * 1024)).map(|i| (i % 251) as u8).collect();
    std::fs::write(root.join("asset.bin"), &big).unwrap();
    ok(&root, &["checkpoint", "-m", "with big"]);

    let remote = root.parent().unwrap().join("sync-remote");
    let pushed = ok_json(
        &root,
        &["sync", "push", "--remote", remote.to_str().unwrap()],
    );
    assert_eq!(pushed["result"]["checkpoints"], 1);
    assert!(pushed["result"]["git_objects_uploaded"].as_u64().unwrap() > 0);
    assert_eq!(pushed["result"]["cas_blobs_uploaded"], 1);

    let workspace_id = pushed["result"]["workspace_id"].as_str().unwrap();
    let base = remote.join("asp-sync/v1/workspaces").join(workspace_id);
    assert!(base.join("workspace.json").is_file());
    assert!(base.join("refs/checkpoints/1.json").is_file());
    assert!(base.join("refs/meta/1.json").is_file());
    assert!(base.join("refs/head.json").is_file());
    assert!(std::fs::read_dir(base.join("objects/git/sha1"))
        .unwrap()
        .next()
        .is_some());
    assert_eq!(
        std::fs::read_dir(base.join("objects/blobs/blake3"))
            .unwrap()
            .count(),
        1
    );

    let again = ok_json(
        &root,
        &["sync", "push", "--remote", remote.to_str().unwrap()],
    );
    assert_eq!(again["result"]["git_objects_uploaded"], 0);
    assert_eq!(again["result"]["cas_blobs_uploaded"], 0);
    assert!(again["result"]["git_objects_present"].as_u64().unwrap() > 0);
    assert_eq!(again["result"]["cas_blobs_present"], 1);

    let head = base.join("refs/head.json");
    std::fs::write(
        &head,
        serde_json::to_vec_pretty(&serde_json::json!({
            "v": 1,
            "name": "refs/asp/head",
            "seq": 99,
            "target": "0123456789012345678901234567890123456789",
            "workspace_id": workspace_id,
            "updated_at": "2099-01-01T00:00:00Z",
            "writer": "test",
        }))
        .unwrap(),
    )
    .unwrap();
    let conflict = asp(
        &root,
        &[
            "--json",
            "sync",
            "push",
            "--remote",
            remote.to_str().unwrap(),
        ],
    );
    assert!(!conflict.status.success());
    let err: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&conflict.stdout)).unwrap();
    assert_eq!(err["error"]["code"], "sync_conflict");
    assert!(err["error"]["hint"].as_str().unwrap().contains("fetch"));
}

#[test]
fn sync_fetch_restores_missing_local_refs_and_objects() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    let checkpoint = ok_json(&root, &["checkpoint", "-m", "base"]);
    let commit = checkpoint["result"]["commit"].as_str().unwrap().to_string();
    let remote = root.parent().unwrap().join("sync-remote");
    ok(
        &root,
        &["sync", "push", "--remote", remote.to_str().unwrap()],
    );

    shadow_git(&root, &["update-ref", "-d", "refs/asp/checkpoints/1"]);
    shadow_git(&root, &["update-ref", "-d", "refs/asp/head"]);
    for entry in std::fs::read_dir(root.join(".asp/shadow.git/objects")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.len() == 2 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
            std::fs::remove_dir_all(entry.path()).unwrap();
        }
    }

    let fetched = ok_json(
        &root,
        &["sync", "fetch", "--remote", remote.to_str().unwrap()],
    );
    assert_eq!(fetched["result"]["refs_imported"], 1);
    assert!(
        fetched["result"]["git_objects_downloaded"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(fetched["result"]["head_updated"], true);
    assert_eq!(fetched["result"]["head_seq"], 1);
    assert_eq!(fetched["result"]["conflicts"], serde_json::json!([]));
    shadow_git(&root, &["cat-file", "-e", &commit]);
    ok(&root, &["status"]);
}

#[test]
fn sync_fetch_reports_conflicts_without_overwriting_local_refs() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    let checkpoint = ok_json(&root, &["checkpoint", "-m", "base"]);
    let commit = checkpoint["result"]["commit"].as_str().unwrap().to_string();
    let remote = root.parent().unwrap().join("sync-remote");
    let pushed = ok_json(
        &root,
        &["sync", "push", "--remote", remote.to_str().unwrap()],
    );
    let workspace_id = pushed["result"]["workspace_id"].as_str().unwrap();
    let base = remote.join("asp-sync/v1/workspaces").join(workspace_id);
    std::fs::write(
        base.join("refs/checkpoints/1.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "v": 1,
            "name": "refs/asp/checkpoints/1",
            "seq": 1,
            "target": "0123456789012345678901234567890123456789",
            "workspace_id": workspace_id,
            "updated_at": "2099-01-01T00:00:00Z",
            "writer": "test",
        }))
        .unwrap(),
    )
    .unwrap();

    let fetched = ok_json(
        &root,
        &["sync", "fetch", "--remote", remote.to_str().unwrap()],
    );
    assert_eq!(fetched["result"]["refs_conflicted"], 1);
    assert_eq!(fetched["result"]["refs_imported"], 0);
    assert_eq!(fetched["result"]["git_objects_downloaded"], 0);
    let conflict = &fetched["result"]["conflicts"][0];
    assert_eq!(conflict["kind"], "checkpoint_ref");
    assert_eq!(conflict["seq"], 1);
    assert_eq!(conflict["local"], commit);
    assert_eq!(
        conflict["remote"],
        "0123456789012345678901234567890123456789"
    );
    assert_eq!(
        shadow_git(&root, &["rev-parse", "refs/asp/checkpoints/1"]),
        commit
    );
}

#[test]
fn audit_filters_journal_events() {
    let (_tmp, root) = project();
    ok(&root, &["init"]);
    ok(&root, &["checkpoint", "-m", "base", "--tool", "editor"]);
    std::fs::write(root.join("src/app.py"), "print('v2')\n").unwrap();
    ok(
        &root,
        &[
            "checkpoint",
            "-m",
            "agent, update",
            "--tool",
            "claude",
            "--session-id",
            "session-1",
        ],
    );

    let audit = ok_json(
        &root,
        &[
            "audit",
            "--op",
            "checkpoint",
            "--tool",
            "claude",
            "--session",
            "session-1",
        ],
    );
    let rows = audit["result"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["message"], "agent, update");
    assert_eq!(rows[0]["tool"], "claude");
    assert_eq!(rows[0]["session_id"], "session-1");
    assert_eq!(
        rows[0]["detail"]["paths"],
        serde_json::json!(["src/app.py"])
    );

    let checkpoint_path_audit = ok_json(
        &root,
        &[
            "audit",
            "--op",
            "checkpoint",
            "--tool",
            "claude",
            "--path",
            "src/app.py",
        ],
    );
    assert_eq!(checkpoint_path_audit["result"].as_array().unwrap().len(), 1);

    let jsonl = ok(
        &root,
        &[
            "audit",
            "--format",
            "jsonl",
            "--op",
            "checkpoint",
            "--tool",
            "claude",
        ],
    );
    let jsonl_rows: Vec<serde_json::Value> = jsonl
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(jsonl_rows.len(), 1);
    assert_eq!(jsonl_rows[0]["message"], "agent, update");

    let csv = ok(
        &root,
        &[
            "audit",
            "--format",
            "csv",
            "--op",
            "checkpoint",
            "--tool",
            "claude",
        ],
    );
    let csv_lines: Vec<_> = csv.lines().collect();
    assert_eq!(csv_lines.len(), 2);
    assert_eq!(
        csv_lines[0],
        "v,ts,op,seq,commit,source,session_id,tool,message,files_changed,duration_ms,detail"
    );
    assert!(csv_lines[1].contains("\"agent, update\""), "{csv}");

    ok(&root, &["restore", "1", "src/app.py"]);
    let path_audit = ok_json(&root, &["audit", "--op", "restore", "--path", "src/app.py"]);
    let path_rows = path_audit["result"].as_array().unwrap();
    assert_eq!(path_rows.len(), 1);
    assert_eq!(path_rows[0]["op"], "restore");

    let future = ok_json(&root, &["audit", "--since", "2999-01-01T00:00:00Z"]);
    assert!(future["result"].as_array().unwrap().is_empty());
    let past_until = ok_json(&root, &["audit", "--until", "1970-01-01T00:00:00Z"]);
    assert!(past_until["result"].as_array().unwrap().is_empty());
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
    assert_eq!(lane["review"]["tests_passed"], false);
    assert_eq!(lane["review"]["files_touched"], lane["files_changed"]);
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
    let remote = root.parent().unwrap().join("origin.git");
    let remote_init = Command::new("git")
        .arg("init")
        .arg("--bare")
        .arg("-q")
        .arg(&remote)
        .output()
        .unwrap();
    assert!(
        remote_init.status.success(),
        "git init --bare: {}",
        String::from_utf8_lossy(&remote_init.stderr)
    );
    git(&["remote", "add", "origin", remote.to_str().unwrap()]);

    ok(&root, &["init"]);
    std::fs::write(
        root.join(".asp/config.toml"),
        "[promote]\nbranch_template = \"review/{workspace}/{fork}\"\n",
    )
    .unwrap();
    ok(&root, &["checkpoint", "-m", "base"]);
    let forks = ok_json(&root, &["fork", "--name", "winner"]);
    let fork_path = PathBuf::from(forks["result"][0]["path"].as_str().unwrap());
    std::fs::write(fork_path.join("src/app.py"), "print('better')\n").unwrap();

    let p = ok_json(&root, &["promote", "winner"]);
    assert_eq!(p["result"]["branch"], "review/proj/winner");
    assert_eq!(
        p["result"]["fork_path"].as_str().unwrap(),
        fork_path.to_string_lossy().as_ref()
    );
    assert_eq!(p["result"]["fork_retained"], true);
    assert_eq!(p["result"]["cleanup_command"], "asp discard winner");
    let content = git(&["show", "review/proj/winner:src/app.py"]);
    assert_eq!(content, "print('better')");

    let forks = ok_json(&root, &["fork", "--name", "human"]);
    let human_path = PathBuf::from(forks["result"][0]["path"].as_str().unwrap());
    std::fs::write(human_path.join("src/app.py"), "print('human')\n").unwrap();
    let out = ok(&root, &["promote", "human"]);
    assert!(out.contains("fork directory remains:"), "{out}");
    assert!(out.contains(human_path.to_string_lossy().as_ref()), "{out}");
    assert!(out.contains("asp discard human"), "{out}");

    let forks = ok_json(&root, &["fork", "--name", "badref"]);
    let badref_path = PathBuf::from(forks["result"][0]["path"].as_str().unwrap());
    std::fs::write(badref_path.join("src/app.py"), "print('badref')\n").unwrap();
    let out = asp(
        &root,
        &["--json", "promote", "badref", "--branch", "bad..name"],
    );
    assert!(!out.status.success());
    let err: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(err["ok"], false);
    assert_eq!(err["error"]["code"], "invalid_branch");
    assert!(err["error"]["hint"].as_str().unwrap().contains("--branch"));

    let forks = ok_json(&root, &["fork", "--name", "pushme"]);
    let push_path = PathBuf::from(forks["result"][0]["path"].as_str().unwrap());
    std::fs::write(push_path.join("src/app.py"), "print('pushed')\n").unwrap();
    let out = asp(
        &root,
        &[
            "--json",
            "promote",
            "pushme",
            "--branch",
            "review/proj/pushme",
            "--push",
        ],
    );
    assert!(!out.status.success());
    let err: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(err["error"]["code"], "nothing_to_do");
    assert!(err["error"]["hint"].as_str().unwrap().contains("--remote"));

    let pushed = ok_json(
        &root,
        &[
            "promote",
            "pushme",
            "--branch",
            "review/proj/pushme",
            "--push",
            "--remote",
            "origin",
        ],
    );
    assert_eq!(pushed["result"]["branch"], "review/proj/pushme");
    assert_eq!(pushed["result"]["push"]["pushed"], true);
    assert_eq!(pushed["result"]["push"]["remote"], "origin");
    assert_eq!(pushed["result"]["push"]["branch"], "review/proj/pushme");
    assert_eq!(
        pushed["result"]["push"]["refspec"],
        "refs/heads/review/proj/pushme:refs/heads/review/proj/pushme"
    );
    assert!(pushed["result"]["push"]["command"]
        .as_str()
        .unwrap()
        .contains("git push origin"));
    let remote_show = Command::new("git")
        .arg("--git-dir")
        .arg(&remote)
        .args(["show", "refs/heads/review/proj/pushme:src/app.py"])
        .output()
        .unwrap();
    assert!(
        remote_show.status.success(),
        "git show remote branch: {}",
        String::from_utf8_lossy(&remote_show.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&remote_show.stdout).trim(),
        "print('pushed')"
    );

    let forks = ok_json(&root, &["fork", "--name", "draft"]);
    let draft_path = PathBuf::from(forks["result"][0]["path"].as_str().unwrap());
    std::fs::write(draft_path.join("src/app.py"), "print('draft')\n").unwrap();
    let out = asp(
        &root,
        &[
            "--json",
            "promote",
            "draft",
            "--branch",
            "review/proj/draft",
            "--pr-draft",
        ],
    );
    assert!(!out.status.success());
    let err: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(err["error"]["code"], "nothing_to_do");
    assert!(err["error"]["hint"].as_str().unwrap().contains("--push"));

    let draft = ok_json(
        &root,
        &[
            "promote",
            "draft",
            "--branch",
            "review/proj/draft",
            "--push",
            "--remote",
            "origin",
            "--pr-draft",
        ],
    );
    assert_eq!(draft["result"]["push"]["pushed"], true);
    assert_eq!(draft["result"]["pr"]["attempted"], true);
    assert_eq!(draft["result"]["pr"]["created"], false);
    assert!(draft["result"]["pr"]["fallback_command"]
        .as_str()
        .unwrap()
        .contains("gh pr create --draft"));
    assert!(!draft["result"]["pr"]["message"]
        .as_str()
        .unwrap()
        .is_empty());
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
