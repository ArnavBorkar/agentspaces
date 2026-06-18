//! Harness integration tests: setup file wiring and `asp hook-event` behavior
//! with real Claude Code-shaped payloads.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn asp(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_asp"))
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("asp spawns")
}

fn hook_event(dir: &Path, payload: &serde_json::Value) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_asp"))
        .arg("hook-event")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawns");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.py"), "v1\n").unwrap();
    (tmp, root)
}

#[test]
fn setup_claude_wires_everything_and_is_reversible() {
    let (_tmp, root) = project();

    // setup on a non-workspace auto-inits it.
    let out = asp(&root, &["setup", "claude"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".claude/settings.json")).unwrap())
            .unwrap();
    let post = settings["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(post.len(), 2, "file-tools group + bash group");
    assert!(post
        .iter()
        .all(|g| g["hooks"][0]["command"] == "asp hook-event"));
    assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);

    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(mcp["mcpServers"]["agentspaces"]["command"], "asp");

    // Idempotent.
    asp(&root, &["setup", "claude"]);
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings["hooks"]["PostToolUse"].as_array().unwrap().len(),
        2
    );

    // Removal cleans both files but keeps the workspace.
    let out = asp(&root, &["setup", "claude", "--remove"]);
    assert!(out.status.success());
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert!(settings["hooks"].get("PostToolUse").is_none());
    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap()).unwrap();
    assert!(mcp["mcpServers"].get("agentspaces").is_none());
    assert!(root.join(".asp").exists());
}

#[test]
fn setup_codex_writes_mcp_config_without_clobbering() {
    let (_tmp, root) = project();
    let codex_dir = root.join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    std::fs::write(
        codex_dir.join("config.toml"),
        r#"# keep my comments
model = "gpt-5.5"

[mcp_servers.existing]
command = "existing-tool"
"#,
    )
    .unwrap();

    let out = asp(&root, &["setup", "codex"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let config = std::fs::read_to_string(codex_dir.join("config.toml")).unwrap();
    assert!(config.contains("# keep my comments"));
    assert!(config.contains("model = \"gpt-5.5\""));
    assert!(config.contains("[mcp_servers.existing]"));
    assert!(config.contains("# agentspaces: begin asp setup codex"));
    assert!(config.contains("[mcp_servers.agentspaces]"));
    assert!(config.contains("args = [\"mcp\"]"));
    let parsed: toml::Value = toml::from_str(&config).unwrap();
    assert_eq!(
        parsed["mcp_servers"]["agentspaces"]["command"].as_str(),
        Some("asp")
    );

    let out = asp(&root, &["setup", "codex"]);
    assert!(out.status.success());
    let config = std::fs::read_to_string(codex_dir.join("config.toml")).unwrap();
    assert_eq!(
        config.matches("[mcp_servers.agentspaces]").count(),
        1,
        "setup should be idempotent"
    );

    let out = asp(&root, &["setup", "codex", "--remove"]);
    assert!(out.status.success());
    let config = std::fs::read_to_string(codex_dir.join("config.toml")).unwrap();
    assert!(config.contains("# keep my comments"));
    assert!(config.contains("[mcp_servers.existing]"));
    assert!(!config.contains("[mcp_servers.agentspaces]"));
    assert!(!config.contains("asp setup codex"));
    assert!(root.join(".asp").exists());
}

#[test]
fn setup_codex_refuses_to_clobber_unmanaged_agentspaces_server() {
    let (_tmp, root) = project();
    let codex_dir = root.join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let original = r#"[mcp_servers.agentspaces]
command = "custom"
"#;
    std::fs::write(codex_dir.join("config.toml"), original).unwrap();

    let out = asp(&root, &["setup", "codex"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("already contains [mcp_servers.agentspaces]"));
    assert!(stderr.contains("hint:"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(codex_dir.join("config.toml")).unwrap(),
        original
    );
}

#[test]
fn hook_event_checkpoints_with_provenance() {
    let (_tmp, root) = project();
    asp(&root, &["init"]);
    asp(&root, &["checkpoint", "-m", "base"]);

    // Claude Code edits a file, then PostToolUse fires.
    std::fs::write(root.join("main.py"), "v2 by agent\n").unwrap();
    let out = hook_event(
        &root,
        &serde_json::json!({
            "session_id": "sess-abc123",
            "transcript_path": "/tmp/t.jsonl",
            "cwd": root.to_string_lossy(),
            "hook_event_name": "PostToolUse",
            "tool_name": "Edit",
            "tool_input": { "file_path": "main.py" },
            "tool_response": {}
        }),
    );
    assert!(out.status.success());

    let log = asp(&root, &["--json", "log"]);
    let log: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&log.stdout)).unwrap();
    let latest = &log["result"][0];
    assert_eq!(latest["op"], "checkpoint");
    assert_eq!(latest["source"], "hook");
    assert_eq!(latest["session_id"], "sess-abc123");
    assert_eq!(latest["tool"], "Edit");
    assert_eq!(latest["message"], "auto: after Edit");

    // No-change hook event creates no checkpoint (hook storms are free).
    let before = log["result"].as_array().unwrap().len();
    hook_event(
        &root,
        &serde_json::json!({
            "session_id": "sess-abc123",
            "cwd": root.to_string_lossy(),
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
        }),
    );
    let log2 = asp(&root, &["--json", "log"]);
    let log2: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&log2.stdout)).unwrap();
    assert_eq!(log2["result"].as_array().unwrap().len(), before);
}

#[test]
fn hook_event_is_silent_outside_workspaces() {
    let tmp = tempfile::tempdir().unwrap();
    let out = hook_event(
        tmp.path(),
        &serde_json::json!({
            "cwd": tmp.path().to_string_lossy(),
            "hook_event_name": "PostToolUse",
            "tool_name": "Edit",
        }),
    );
    assert!(out.status.success(), "must never break a session");
    assert!(out.stdout.is_empty());

    // Garbage on stdin: still exits 0.
    let mut child = Command::new(env!("CARGO_BIN_EXE_asp"))
        .arg("hook-event")
        .current_dir(tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"not json")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
}
