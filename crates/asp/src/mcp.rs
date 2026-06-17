//! `asp mcp` — Model Context Protocol stdio server.
//!
//! A deliberately small, dependency-free JSON-RPC loop: newline-delimited
//! JSON-RPC 2.0 over stdin/stdout, implementing initialize / tools-list /
//! tools-call. Tool descriptions are written for models: they say when to
//! reach for the tool, and every error text states the corrective next step
//! so an agent can self-correct without human help.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use asp_core::journal::Source;
use asp_core::store::FORMAT_VERSION;
use asp_core::workspace::CheckpointOpts;
use asp_core::{Error, Workspace};
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: &str = "2025-06-18";

pub fn serve() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut reader = stdin.lock();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        // Byte-level reads: one bad UTF-8 byte must not kill the server.
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&buf);
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line.trim_end()) {
            Ok(v) => v,
            Err(e) => {
                let resp = error_response(
                    Value::Null,
                    -32700,
                    format!("parse error: {e}; send one valid JSON-RPC 2.0 object per line"),
                );
                writeln!(out, "{resp}")?;
                out.flush()?;
                continue;
            }
        };
        if !msg.is_object() {
            let resp = error_response(
                Value::Null,
                -32600,
                "invalid request: expected a JSON-RPC 2.0 object",
            );
            writeln!(out, "{resp}")?;
            out.flush()?;
            continue;
        }

        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        // Notifications (no id) get no response.
        let Some(id) = id else { continue };
        if !valid_request_id(&id) {
            let resp = error_response(
                Value::Null,
                -32600,
                "invalid request: id must be a string, number, or null",
            );
            writeln!(out, "{resp}")?;
            out.flush()?;
            continue;
        }
        if msg.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") {
            let resp = error_response(
                id,
                -32600,
                "invalid request: jsonrpc must be exactly \"2.0\"",
            );
            writeln!(out, "{resp}")?;
            out.flush()?;
            continue;
        }
        let Some(method) = msg.get("method").and_then(|m| m.as_str()) else {
            let resp = error_response(id, -32600, "invalid request: method must be a string");
            writeln!(out, "{resp}")?;
            out.flush()?;
            continue;
        };

        let result = match method {
            "initialize" => Ok(initialize_result(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => handle_tool_call(&params),
            other => Err(json!({
                "code": -32601,
                "message": format!("method not found: {other}")
            })),
        };
        let resp = match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        };
        writeln!(out, "{resp}")?;
        out.flush()?;
    }
    Ok(())
}

fn valid_request_id(id: &Value) -> bool {
    matches!(id, Value::String(_) | Value::Number(_) | Value::Null)
}

fn rpc_error(code: i64, message: impl Into<String>) -> Value {
    json!({ "code": code, "message": message.into() })
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": rpc_error(code, message) })
}

fn invalid_params(message: impl Into<String>) -> Value {
    rpc_error(-32602, format!("invalid params: {}", message.into()))
}

fn initialize_result(params: &Value) -> Value {
    // Echo the client's protocol version when it asks for an older one we
    // can serve; otherwise state ours.
    let requested = params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .unwrap_or(PROTOCOL_VERSION);
    json!({
        "protocolVersion": requested,
        "capabilities": {
            "tools": { "listChanged": false },
            "experimental": { "asp": asp_capabilities() },
        },
        "serverInfo": {
            "name": "agentspaces",
            "title": "agentspaces — branchable agent workspaces",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "asp gives this session durable, branchable state over the real working \
    directory. Checkpoint before risky changes (workspace_checkpoint), undo damage including bash \
    side-effects (workspace_undo / workspace_restore), fork the whole directory to try N approaches \
    in parallel (workspace_fork), compare them (workspace_forks), and land the winner as an ordinary \
    git branch (workspace_promote). Checkpoints capture untracked files too — everything is \
    recoverable with stock git."
    })
}

fn asp_capabilities() -> Value {
    json!({
        "serverVersion": env!("CARGO_PKG_VERSION"),
        "protocolVersion": PROTOCOL_VERSION,
        "formatVersion": FORMAT_VERSION,
        "localOnlyByDefault": true,
        "stockGitRecovery": true,
        "toolCount": tool_definitions().len(),
        "toolAnnotations": true,
        "jsonSchemas": {
            "mcpToolResult": "schemas/mcp-tool-result.schema.json",
            "cliEnvelope": "schemas/cli-json-envelope.schema.json",
            "sharedResults": "schemas/asp-result.schema.json",
        }
    })
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

fn dir_prop() -> Value {
    json!({
        "type": "string",
        "description": "Workspace directory (defaults to the server's working directory)."
    })
}

fn annotations(title: &str, read_only: bool, destructive: bool, idempotent: bool) -> Value {
    json!({
        "title": title,
        "readOnlyHint": read_only,
        "destructiveHint": destructive,
        "idempotentHint": idempotent,
        "openWorldHint": false,
    })
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "workspace_status",
            "description": "Summarize the asp workspace: files changed since the last checkpoint, \
        last checkpoint info, active forks. Use this first to orient yourself. If the directory is not \
        an asp workspace yet, the error will say so — call workspace_init then.",
            "inputSchema": schema(json!({ "directory": dir_prop() }), &[]),
            "annotations": annotations("Workspace status", true, false, true),
        }),
        json!({
            "name": "workspace_init",
            "description": "Adopt a directory as an asp workspace. Instant; touches nothing — it \
        only creates a .asp sidecar. Call once per project, before any other workspace tool.",
            "inputSchema": schema(json!({ "directory": dir_prop() }), &[]),
            "annotations": annotations("Initialize workspace", false, false, false),
        }),
        json!({
            "name": "workspace_checkpoint",
            "description": "Capture the current state of every file (untracked included) as a \
        checkpoint. Cheap (sub-second) and a no-op if nothing changed — call it freely before and after \
        risky operations so workspace_undo always has a point to return to.",
            "inputSchema": schema(
                json!({
                    "directory": dir_prop(),
                    "message": { "type": "string", "description": "What this state represents, e.g. 'before dependency upgrade'." }
                }),
                &[],
            ),
            "annotations": annotations("Checkpoint workspace", false, false, false),
        }),
        json!({
            "name": "workspace_log",
            "description": "Timeline of checkpoints and operations (newest first) with what caused \
        each one. Use it to find a checkpoint number for workspace_restore or workspace_diff.",
            "inputSchema": schema(
                json!({
                    "directory": dir_prop(),
                    "limit": { "type": "integer", "description": "Max entries (default 20)." }
                }),
                &[],
            ),
            "annotations": annotations("Workspace log", true, false, true),
        }),
        json!({
            "name": "workspace_undo",
            "description": "Step back: if there are uncommitted changes since the last checkpoint \
        (including bash side-effects like deleted or generated files), revert them; if the tree is clean, \
        go back one checkpoint. The pre-undo state is saved automatically, so undo is always safe. Do not \
        call just to inspect history or compare states; use workspace_log or workspace_diff. If the user \
        named a checkpoint, call workspace_restore instead.",
            "inputSchema": schema(json!({ "directory": dir_prop() }), &[]),
            "annotations": annotations("Undo workspace", false, true, false),
        }),
        json!({
            "name": "workspace_restore",
            "description": "Restore the working tree (or specific paths) to a checkpoint from \
        workspace_log. The current state is safety-checkpointed first, so nothing is ever lost. Do not \
        call to browse history; use workspace_log or workspace_diff first. Prefer paths when the user \
        asked to recover only specific files.",
            "inputSchema": schema(
                json!({
                    "directory": dir_prop(),
                    "checkpoint": { "type": "string", "description": "Checkpoint number (e.g. \"3\") or commit prefix from workspace_log." },
                    "paths": { "type": "array", "items": { "type": "string" }, "description": "Optional: restore only these paths." }
                }),
                &["checkpoint"],
            ),
            "annotations": annotations("Restore workspace", false, true, false),
        }),
        json!({
            "name": "workspace_fork",
            "description": "Create instant copy-on-write fork(s) of the WHOLE directory — \
        untracked files, node_modules, build artifacts, everything — each a fully runnable sibling \
        directory. Use forks to try multiple approaches in parallel or to experiment without touching \
        the main tree. Returns each fork's path; run commands there.",
            "inputSchema": schema(
                json!({
                    "directory": dir_prop(),
                    "count": { "type": "integer", "description": "How many forks (default 1)." },
                    "name": { "type": "string", "description": "Fork name or name prefix." }
                }),
                &[],
            ),
            "annotations": annotations("Fork workspace", false, false, false),
        }),
        json!({
            "name": "workspace_forks",
            "description": "Compare all active forks against their fork points: files changed, \
        lines added/removed, last activity. Use after running work in forks to decide which to promote.",
            "inputSchema": schema(json!({ "directory": dir_prop() }), &[]),
            "annotations": annotations("Compare forks", true, false, true),
        }),
        json!({
            "name": "workspace_diff",
            "description": "What changed between two checkpoints, or between a checkpoint and the \
        current working tree (omit 'to'). File-level summary with +/- line counts.",
            "inputSchema": schema(
                json!({
                    "directory": dir_prop(),
                    "from": { "type": "string", "description": "Checkpoint number or commit prefix." },
                    "to": { "type": "string", "description": "Optional second checkpoint; defaults to the working tree." }
                }),
                &["from"],
            ),
            "annotations": annotations("Diff workspace", true, false, true),
        }),
        json!({
            "name": "workspace_promote",
            "description": "Land a fork's work as an ordinary git branch in the main repository \
        (never touches HEAD or the user's worktree). Use after workspace_forks shows a winner. The \
        result names the branch; suggest a PR or merge to the user afterwards. Do not call before \
        comparing forks or when the user only asked to inspect alternatives.",
            "inputSchema": schema(
                json!({
                    "directory": dir_prop(),
                    "fork": { "type": "string", "description": "Fork name from workspace_forks." },
                    "branch": { "type": "string", "description": "Branch name. Defaults to .asp/config.toml promote.branch_template." }
                }),
                &["fork"],
            ),
            "annotations": annotations("Promote fork", false, false, false),
        }),
        json!({
            "name": "workspace_discard",
            "description": "Delete a fork. Refuses if the fork has work that was never promoted — \
        pass force=true only when the user has confirmed the work should be thrown away. Do not force \
        discard work that may matter; promote it first or ask for confirmation.",
            "inputSchema": schema(
                json!({
                    "directory": dir_prop(),
                    "fork": { "type": "string", "description": "Fork name from workspace_forks." },
                    "force": { "type": "boolean", "description": "Delete even with unpromoted work." }
                }),
                &["fork"],
            ),
            "annotations": annotations("Discard fork", false, true, false),
        }),
    ]
}

fn handle_tool_call(params: &Value) -> Result<Value, Value> {
    if !params.is_object() {
        return Err(invalid_params(
            "tools/call params must be an object with a string 'name'",
        ));
    }
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid_params("tools/call requires a string 'name'"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return Err(invalid_params(
            "tools/call 'arguments' must be an object when present",
        ));
    }
    match call_tool(name, &args) {
        Ok(payload) => Ok(json!({
            "content": [{ "type": "text", "text": payload.to_string() }],
            "structuredContent": payload,
            "isError": false,
        })),
        Err(e) => {
            let code = e.code;
            let message = e.message;
            let hint = e.hint;
            let mut text = message.clone();
            if let Some(hint) = &hint {
                text.push_str(&format!("\nnext step: {hint}"));
            }
            Ok(json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": {
                    "error": {
                        "code": code,
                        "message": message,
                        "hint": hint,
                    }
                },
                "isError": true,
            }))
        }
    }
}

fn workspace_for(args: &Value) -> Result<Workspace, Error> {
    let dir = directory_arg(args)?;
    Workspace::open(&dir)
}

fn directory_arg(args: &Value) -> Result<PathBuf, Error> {
    match args.get("directory").and_then(|v| v.as_str()) {
        Some(d) => Ok(PathBuf::from(d)),
        None => std::env::current_dir().map_err(|e| {
            Error::new(
                asp_core::ErrorCode::Io,
                format!("cannot resolve working directory: {e}"),
            )
        }),
    }
}

fn to_value<T: serde::Serialize>(v: &T) -> Result<Value, Error> {
    serde_json::to_value(v).map_err(|e| Error::new(asp_core::ErrorCode::Io, format!("encode: {e}")))
}

fn call_tool(name: &str, args: &Value) -> Result<Value, Error> {
    match name {
        "workspace_status" => to_value(&workspace_for(args)?.status()?),
        "workspace_init" => {
            let ws = Workspace::init(&directory_arg(args)?, None)?;
            Ok(json!({
                "root": ws.root(),
                "note": "workspace created; nothing captured yet — call workspace_checkpoint to take the first checkpoint"
            }))
        }
        "workspace_checkpoint" => {
            let ws = workspace_for(args)?;
            let info = ws.checkpoint(CheckpointOpts {
                message: args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                source: Some(Source::Mcp),
                ..Default::default()
            })?;
            match info {
                Some(i) => to_value(&i),
                None => Ok(json!({ "no_changes": true, "note": "nothing changed since the last checkpoint" })),
            }
        }
        "workspace_log" => {
            let ws = workspace_for(args)?;
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
            to_value(&ws.log(limit)?)
        }
        "workspace_undo" => to_value(&workspace_for(args)?.undo(Some(Source::Mcp))?),
        "workspace_restore" => {
            let ws = workspace_for(args)?;
            let checkpoint = args
                .get("checkpoint")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    Error::new(
                        asp_core::ErrorCode::CheckpointNotFound,
                        "missing required argument 'checkpoint'",
                    )
                    .with_hint("call workspace_log to list checkpoints, then pass its number")
                })?;
            let paths: Vec<String> = args
                .get("paths")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|p| p.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            to_value(&ws.restore(checkpoint, &paths, Some(Source::Mcp))?)
        }
        "workspace_fork" => {
            let ws = workspace_for(args)?;
            let count = args.get("count").and_then(|v| v.as_u64()).unwrap_or(1).clamp(1, 64);
            let name = args.get("name").and_then(|v| v.as_str());
            let mut infos = Vec::new();
            for i in 0..count {
                let label = match (name, count) {
                    (Some(n), 1) => Some(n.to_string()),
                    (Some(n), _) => Some(format!("{n}-{}", i + 1)),
                    (None, _) => None,
                };
                infos.push(ws.fork(label, Some(Source::Mcp))?);
            }
            to_value(&infos)
        }
        "workspace_forks" => to_value(&workspace_for(args)?.fork_compare()?),
        "workspace_diff" => {
            let ws = workspace_for(args)?;
            let from = args.get("from").and_then(|v| v.as_str()).ok_or_else(|| {
                Error::new(
                    asp_core::ErrorCode::CheckpointNotFound,
                    "missing required argument 'from'",
                )
                .with_hint("call workspace_log to list checkpoints, then pass its number")
            })?;
            let to = args.get("to").and_then(|v| v.as_str());
            to_value(&ws.diff(from, to)?)
        }
        "workspace_promote" => {
            let ws = workspace_for(args)?;
            let fork = args.get("fork").and_then(|v| v.as_str()).ok_or_else(|| {
                Error::new(asp_core::ErrorCode::ForkNotFound, "missing required argument 'fork'")
                    .with_hint("call workspace_forks to list forks, then pass a name")
            })?;
            let branch = args
                .get("branch")
                .and_then(|v| v.as_str())
                .map(String::from);
            to_value(&ws.promote(fork, branch)?)
        }
        "workspace_discard" => {
            let ws = workspace_for(args)?;
            let fork = args.get("fork").and_then(|v| v.as_str()).ok_or_else(|| {
                Error::new(asp_core::ErrorCode::ForkNotFound, "missing required argument 'fork'")
                    .with_hint("call workspace_forks to list forks, then pass a name")
            })?;
            let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
            ws.discard(fork, force)?;
            Ok(json!({ "discarded": fork }))
        }
        other => Err(Error::new(
            asp_core::ErrorCode::NothingToDo,
            format!("unknown tool: {other}"),
        )
        .with_hint("valid tools: workspace_status, workspace_init, workspace_checkpoint, workspace_log, workspace_undo, workspace_restore, workspace_fork, workspace_forks, workspace_diff, workspace_promote, workspace_discard")),
    }
}
