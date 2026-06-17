//! MCP server tests: drive `asp mcp` over stdio exactly like a harness does.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::Duration;

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpClient {
    fn start(dir: &std::path::Path) -> Self {
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

    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next_id += 1;
        let id = self.next_id;
        let msg = serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        });
        let resp = self.request_value(msg);
        assert_eq!(resp["id"], id, "response id matches");
        resp
    }

    fn request_value(&mut self, msg: serde_json::Value) -> serde_json::Value {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        self.read_response()
    }

    fn write_raw(&mut self, bytes: &[u8]) {
        self.stdin.write_all(bytes).unwrap();
        self.stdin.flush().unwrap();
    }

    fn read_response(&mut self) -> serde_json::Value {
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).unwrap();
        assert!(read > 0, "mcp server closed stdout");
        serde_json::from_str(&line).expect("valid json response")
    }

    fn call_tool(&mut self, name: &str, args: serde_json::Value) -> serde_json::Value {
        let resp = self.request(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": args }),
        );
        resp["result"].clone()
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/app.py"), "print('v1')\n").unwrap();
    (tmp, root)
}

#[test]
fn full_mcp_session() {
    let (_tmp, root) = project();
    let mut mcp = McpClient::start(&root);

    // Handshake.
    let init = mcp.request(
        "initialize",
        serde_json::json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" }
        }),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "agentspaces");
    assert_eq!(
        init["result"]["capabilities"]["experimental"]["asp"]["serverVersion"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        init["result"]["capabilities"]["experimental"]["asp"]["localOnlyByDefault"],
        true
    );
    assert!(init["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("fork"));

    // Tool list: all workspace tools present, schemas well-formed.
    let tools = mcp.request("tools/list", serde_json::json!({}));
    let list = tools["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = list.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "workspace_status",
        "workspace_init",
        "workspace_checkpoint",
        "workspace_log",
        "workspace_undo",
        "workspace_restore",
        "workspace_fork",
        "workspace_forks",
        "workspace_diff",
        "workspace_promote",
        "workspace_discard",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
    for t in list {
        assert!(t["description"].as_str().unwrap().len() > 40);
        assert_eq!(t["inputSchema"]["type"], "object");
    }

    // Calling a tool before init returns a self-correcting error.
    let status = mcp.call_tool("workspace_status", serde_json::json!({}));
    assert_eq!(status["isError"], true);
    let text = status["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("workspace_init") || text.contains("asp init"),
        "{text}"
    );

    // init → checkpoint → fork → forks → undo through the protocol.
    let init = mcp.call_tool("workspace_init", serde_json::json!({}));
    assert_eq!(init["isError"], false);

    let cp = mcp.call_tool(
        "workspace_checkpoint",
        serde_json::json!({ "message": "base" }),
    );
    assert_eq!(cp["isError"], false);
    assert_eq!(cp["structuredContent"]["seq"], 1);

    std::fs::write(root.join("src/app.py"), "print('agent broke it')\n").unwrap();
    mcp.call_tool(
        "workspace_checkpoint",
        serde_json::json!({ "message": "damage" }),
    );

    let undo = mcp.call_tool("workspace_undo", serde_json::json!({}));
    assert_eq!(undo["isError"], false);
    assert_eq!(
        std::fs::read_to_string(root.join("src/app.py")).unwrap(),
        "print('v1')\n"
    );

    let forks = mcp.call_tool(
        "workspace_fork",
        serde_json::json!({ "count": 2, "name": "mcp-try" }),
    );
    assert_eq!(forks["isError"], false);
    assert_eq!(forks["structuredContent"].as_array().unwrap().len(), 2);

    let compare = mcp.call_tool("workspace_forks", serde_json::json!({}));
    assert_eq!(compare["structuredContent"].as_array().unwrap().len(), 2);

    // Unknown tool: actionable error listing valid tools.
    let bad = mcp.call_tool("workspace_nope", serde_json::json!({}));
    assert_eq!(bad["isError"], true);
    assert!(bad["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("workspace_status"));

    // ping works; unknown method is a protocol error.
    let pong = mcp.request("ping", serde_json::json!({}));
    assert!(pong["result"].is_object());
    let nope = mcp.request("bogus/method", serde_json::json!({}));
    assert_eq!(nope["error"]["code"], -32601);
}

#[test]
fn explicit_directory_argument() {
    let (_tmp, root) = project();
    // Server started OUTSIDE the project; tools target it via `directory`.
    let elsewhere = tempfile::tempdir().unwrap();
    let mut mcp = McpClient::start(elsewhere.path());
    mcp.request(
        "initialize",
        serde_json::json!({ "protocolVersion": "2025-06-18" }),
    );

    let dir = root.to_string_lossy().to_string();
    let init = mcp.call_tool("workspace_init", serde_json::json!({ "directory": dir }));
    assert_eq!(init["isError"], false, "{init}");
    let cp = mcp.call_tool(
        "workspace_checkpoint",
        serde_json::json!({ "directory": dir, "message": "remote" }),
    );
    assert_eq!(cp["structuredContent"]["seq"], 1);
}

#[test]
fn malformed_json_rpc_errors_are_recoverable() {
    let (_tmp, root) = project();
    let mut mcp = McpClient::start(&root);

    mcp.write_raw(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\",\xff}\n");
    let err = mcp.read_response();
    assert_eq!(err["id"], serde_json::Value::Null);
    assert_eq!(err["error"]["code"], -32700);
    assert!(err["error"]["message"]
        .as_str()
        .unwrap()
        .contains("valid JSON-RPC 2.0 object"));

    let pong = mcp.request("ping", serde_json::json!({}));
    assert!(pong["result"].is_object());
}

#[test]
fn invalid_ids_notifications_and_unknown_methods_are_handled() {
    let (_tmp, root) = project();
    let mut mcp = McpClient::start(&root);

    mcp.write_raw(b"[]\n");
    let not_object = mcp.read_response();
    assert_eq!(not_object["id"], serde_json::Value::Null);
    assert_eq!(not_object["error"]["code"], -32600);

    mcp.write_raw(br#"{"jsonrpc":"2.0","id":{"bad":true},"method":"ping"}"#);
    mcp.write_raw(b"\n");
    let bad_id = mcp.read_response();
    assert_eq!(bad_id["id"], serde_json::Value::Null);
    assert_eq!(bad_id["error"]["code"], -32600);
    assert!(bad_id["error"]["message"]
        .as_str()
        .unwrap()
        .contains("id must be"));

    mcp.write_raw(br#"{"jsonrpc":"2.0","method":"ping"}"#);
    mcp.write_raw(b"\n");
    let pong = mcp.request("ping", serde_json::json!({}));
    assert!(pong["result"].is_object());

    let unknown = mcp.request_value(serde_json::json!({
        "jsonrpc": "2.0",
        "id": "client-string-id",
        "method": "bogus/method"
    }));
    assert_eq!(unknown["id"], "client-string-id");
    assert_eq!(unknown["error"]["code"], -32601);
}

#[test]
fn bad_tools_call_params_return_invalid_params() {
    let (_tmp, root) = project();
    let mut mcp = McpClient::start(&root);

    for params in [
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!({ "name": 42 }),
        serde_json::json!({ "name": "workspace_status", "arguments": [] }),
    ] {
        let resp = mcp.request("tools/call", params);
        assert_eq!(resp["error"]["code"], -32602, "{resp}");
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("invalid params"));
    }
}

#[test]
fn partial_lines_are_buffered_until_newline() {
    let (_tmp, root) = project();
    let mut mcp = McpClient::start(&root);

    mcp.write_raw(br#"{"jsonrpc":"2.0","id":77,"method":"ping","#);
    thread::sleep(Duration::from_millis(20));
    mcp.write_raw(br#""params":{}}"#);
    mcp.write_raw(b"\n");

    let resp = mcp.read_response();
    assert_eq!(resp["id"], 77);
    assert!(resp["result"].is_object());
}
