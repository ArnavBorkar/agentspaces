//! Snapshot-style tests for automation-facing JSON shapes.
//!
//! Dynamic values (temp paths, workspace ids, timestamps, git oids, timings,
//! and duplicate MCP text payloads) are normalized before comparison. The
//! remaining structure is the public contract scripts and MCP clients consume.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};

use serde_json::{json, Value};

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

fn ok_json(dir: &Path, args: &[&str]) -> Value {
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

fn snapshot(name: &str, actual: Value) {
    let expected = match name {
        "cli_init" => include_str!("snapshots/cli_init.json"),
        "cli_checkpoint" => include_str!("snapshots/cli_checkpoint.json"),
        "cli_status" => include_str!("snapshots/cli_status.json"),
        "cli_stats" => include_str!("snapshots/cli_stats.json"),
        "cli_log" => include_str!("snapshots/cli_log.json"),
        "cli_race" => include_str!("snapshots/cli_race.json"),
        "cli_schema" => include_str!("snapshots/cli_schema.json"),
        "cli_policy_validate" => include_str!("snapshots/cli_policy_validate.json"),
        "cli_error" => include_str!("snapshots/cli_error.json"),
        "mcp_initialize" => include_str!("snapshots/mcp_initialize.json"),
        "mcp_tools" => include_str!("snapshots/mcp_tools.json"),
        "mcp_transcript" => include_str!("snapshots/mcp_transcript.json"),
        "mcp_status" => include_str!("snapshots/mcp_status.json"),
        "mcp_error" => include_str!("snapshots/mcp_error.json"),
        other => panic!("unknown snapshot {other}"),
    };
    let expected: Value = serde_json::from_str(expected).expect("snapshot is valid json");
    assert_eq!(
        actual,
        expected,
        "{name} snapshot changed\nactual:\n{}",
        serde_json::to_string_pretty(&actual).unwrap()
    );
}

fn normalize(mut value: Value, root: &Path) -> Value {
    normalize_value(&mut value, root);
    value
}

fn normalize_value(value: &mut Value, root: &Path) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                match key.as_str() {
                    "root" | "path" | "log_file" | "settings_file" => {
                        if let Some(s) = child.as_str() {
                            *child = json!(normalize_path(s, root));
                        }
                    }
                    "mcp_file" => {
                        if let Some(s) = child.as_str() {
                            *child = json!(normalize_path(s, root));
                        }
                    }
                    "workspace_id" => *child = json!("<workspace-id>"),
                    "asp_version" | "serverVersion" => *child = json!("<asp-version>"),
                    "version" if child.as_str() == Some(env!("CARGO_PKG_VERSION")) => {
                        *child = json!("<asp-version>");
                    }
                    "commit" | "target_commit" => *child = json!("<git-oid>"),
                    "ts" | "generated_at" => *child = json!("<timestamp>"),
                    "duration_ms" | "store_bytes" | "blob_bytes" if child.is_number() => {
                        *child = json!(0);
                    }
                    "message" | "hint" => {
                        if let Some(s) = child.as_str() {
                            *child = json!(normalize_text(s, root));
                        }
                    }
                    "text" => *child = json!("<text>"),
                    _ => normalize_value(child, root),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_value(item, root);
            }
        }
        _ => {}
    }
}

fn normalize_path(s: &str, root: &Path) -> String {
    let canonical = root.canonicalize().ok();
    for candidate in [Some(root), canonical.as_deref()].into_iter().flatten() {
        let root_s = candidate.to_string_lossy();
        if s == root_s {
            return "<workspace-root>".to_string();
        }
        if let Some(rest) = s.strip_prefix(root_s.as_ref()) {
            return format!("<workspace-root>{rest}");
        }
    }
    s.to_string()
}

fn normalize_text(s: &str, root: &Path) -> String {
    let canonical = root.canonicalize().ok();
    let mut normalized = s.to_string();
    let mut candidates: Vec<String> = [Some(root), canonical.as_deref()]
        .into_iter()
        .flatten()
        .map(|candidate| candidate.to_string_lossy().to_string())
        .collect();
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.len()));
    candidates.dedup();
    for candidate in candidates {
        normalized = normalized.replace(&candidate, "<workspace-root>");
    }
    normalized
}

#[test]
fn cli_json_shapes_match_snapshots() {
    let (_tmp, root) = project();

    let schema = ok_json(&root, &["schema"]);
    snapshot("cli_schema", normalize(schema, &root));

    let init = ok_json(&root, &["init"]);
    snapshot("cli_init", normalize(init, &root));

    let policy = ok_json(&root, &["policy", "validate"]);
    snapshot("cli_policy_validate", normalize(policy, &root));

    let checkpoint = ok_json(&root, &["checkpoint", "-m", "base"]);
    snapshot("cli_checkpoint", normalize(checkpoint, &root));

    let status = ok_json(&root, &["status"]);
    snapshot("cli_status", normalize(status, &root));

    let stats = ok_json(&root, &["stats"]);
    snapshot("cli_stats", normalize(stats, &root));

    let log = ok_json(&root, &["log", "-n", "2"]);
    snapshot("cli_log", normalize(log, &root));

    let race = ok_json(
        &root,
        &[
            "race",
            "-n",
            "1",
            "--name",
            "snap",
            "--label",
            "primary",
            "--",
            "sh",
            "-c",
            "echo race >> src/app.py",
        ],
    );
    snapshot("cli_race", normalize(race, &root));

    let outside = tempfile::tempdir().unwrap();
    let out = asp(outside.path(), &["--json", "status"]);
    assert!(!out.status.success());
    let mut error: Value = serde_json::from_slice(&out.stdout).expect("error json");
    error["error"]["message"] = json!("<message>");
    snapshot("cli_error", error);
}

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpClient {
    fn start(dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_asp"))
            .arg("-C")
            .arg(dir)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("asp mcp spawns");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            next_id: 0,
        }
    }

    fn exchange(&mut self, method: &str, params: Value) -> (Value, Value) {
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": method,
            "params": params,
        });
        writeln!(self.stdin, "{request}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let resp: Value = serde_json::from_str(&line).expect("valid mcp response");
        assert_eq!(resp["id"], self.next_id);
        (request, resp)
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.exchange(method, params).1
    }

    fn call_tool(&mut self, name: &str, args: Value) -> Value {
        let resp = self.request("tools/call", json!({ "name": name, "arguments": args }));
        resp["result"].clone()
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn assert_tool_list_is_concise_and_actionable(tools: &[Value]) {
    assert!(tools.len() <= 16, "unexpected tool sprawl: {}", tools.len());
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let description = tool["description"].as_str().unwrap();
        assert!(
            description.len() <= 560,
            "{name} description is too long for tool selection: {} chars",
            description.len()
        );
        assert!(
            !description.contains('\n'),
            "{name} description should be a compact paragraph"
        );
        let risky = tool["annotations"]["destructiveHint"] == true || name == "workspace_promote";
        if risky {
            assert!(
                description.contains("Do not"),
                "{name} must say when not to call it"
            );
        }
    }
}

fn assert_tool_error_is_actionable(result: &Value) {
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.len() <= 520,
        "tool error text is too long for a model turn: {} chars",
        text.len()
    );
    assert!(text.contains("next step:"), "tool error lacks next step");
    assert!(result["structuredContent"]["error"]["code"].is_string());
    assert!(result["structuredContent"]["error"]["hint"].is_string());
}

fn transcript_tool(tool: &Value) -> Value {
    json!({
        "name": tool["name"],
        "description": tool["description"],
        "annotations": tool["annotations"],
    })
}

#[test]
fn mcp_tool_result_shapes_match_snapshots() {
    let (_tmp, root) = project();
    let mut mcp = McpClient::start(&root);

    let init = mcp.request(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "snapshot", "version": "0" }
        }),
    );
    snapshot("mcp_initialize", normalize(init["result"].clone(), &root));

    let tools = mcp.request("tools/list", json!({}));
    snapshot("mcp_tools", tools["result"].clone());

    let error = mcp.call_tool("workspace_status", json!({}));
    snapshot("mcp_error", normalize(error, &root));

    let init = mcp.call_tool("workspace_init", json!({}));
    assert_eq!(init["isError"], false);
    let checkpoint = mcp.call_tool("workspace_checkpoint", json!({ "message": "base" }));
    assert_eq!(checkpoint["structuredContent"]["seq"], 1);

    let status = mcp.call_tool("workspace_status", json!({}));
    snapshot("mcp_status", normalize(status, &root));
}

#[test]
fn mcp_transcript_stays_concise_and_actionable() {
    let (_tmp, root) = project();
    let mut mcp = McpClient::start(&root);

    let (init_req, init_resp) = mcp.exchange(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "transcript", "version": "0" }
        }),
    );
    let instructions = init_resp["result"]["instructions"].as_str().unwrap();
    assert!(instructions.len() <= 900);
    assert!(instructions.contains("workspace_checkpoint"));
    assert!(instructions.contains("workspace_undo"));

    let (tools_req, tools_resp) = mcp.exchange("tools/list", json!({}));
    let tools = tools_resp["result"]["tools"].as_array().unwrap();
    assert_tool_list_is_concise_and_actionable(tools);
    let tools_transcript_resp = json!({
        "jsonrpc": "2.0",
        "id": tools_resp["id"],
        "result": {
            "tools": tools.iter().map(transcript_tool).collect::<Vec<_>>()
        }
    });

    let (status_req, status_resp) = mcp.exchange(
        "tools/call",
        json!({ "name": "workspace_status", "arguments": {} }),
    );
    assert_tool_error_is_actionable(&status_resp["result"]);

    let (init_tool_req, init_tool_resp) = mcp.exchange(
        "tools/call",
        json!({ "name": "workspace_init", "arguments": {} }),
    );
    assert_eq!(init_tool_resp["result"]["isError"], false);

    let (checkpoint_req, checkpoint_resp) = mcp.exchange(
        "tools/call",
        json!({ "name": "workspace_checkpoint", "arguments": { "message": "base" } }),
    );
    assert_eq!(checkpoint_resp["result"]["structuredContent"]["seq"], 1);

    let transcript = json!([
        { "label": "initialize", "request": init_req, "response": init_resp },
        { "label": "tools/list", "request": tools_req, "response": tools_transcript_resp },
        { "label": "status before init", "request": status_req, "response": status_resp },
        { "label": "init workspace", "request": init_tool_req, "response": init_tool_resp },
        { "label": "checkpoint base", "request": checkpoint_req, "response": checkpoint_resp }
    ]);
    snapshot("mcp_transcript", normalize(transcript, &root));
}
