//! Agent harness integrations: hook event handling + one-command setup.
//!
//! `asp setup claude` wires the project into Claude Code:
//!   - PostToolUse hooks on file-editing tools and Bash → auto-checkpoint
//!     after every change (the /rewind-for-everything experience, including
//!     bash side-effects)
//!   - PreToolUse hook on Bash → checkpoint manual edits before commands run
//!   - `.mcp.json` registration of the `asp mcp` server
//!
//! `asp setup codex` registers the same MCP server in Codex's documented
//! `.codex/config.toml` shape.
//!
//! `asp setup opencode` registers the MCP server in OpenCode's documented
//! `opencode.json` shape.
//!
//! `asp hook-event` (hidden) is the command those hooks invoke: it reads the
//! hook JSON from stdin and takes a provenance-stamped checkpoint. It NEVER
//! fails the session: any error exits 0 with a note on stderr.

use std::io::Read;
use std::path::{Path, PathBuf};

use asp_core::journal::Source;
use asp_core::workspace::CheckpointOpts;
use asp_core::{Error, ErrorCode, Workspace};
use serde_json::{json, Value};

/// Tools whose completion should snapshot the tree.
const FILE_TOOLS: &str = "Write|Edit|MultiEdit|NotebookEdit";
const CODEX_BLOCK_START: &str = "# agentspaces: begin asp setup codex";
const CODEX_BLOCK_END: &str = "# agentspaces: end asp setup codex";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEvent {
    pub event: String,
    pub tool: String,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
}

/// Entry point for `asp hook-event`. Reads Claude Code's hook payload from
/// stdin. Exit code is always 0 — a state layer must never break the session.
pub fn handle_hook_event() {
    if let Err(e) = try_handle_hook_event() {
        eprintln!("asp hook-event: {e} (session unaffected)");
    }
}

fn try_handle_hook_event() -> Result<(), Error> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| Error::new(ErrorCode::Io, format!("stdin: {e}")))?;
    let payload: Value = serde_json::from_str(&input)
        .map_err(|e| Error::new(ErrorCode::Io, format!("hook payload not JSON: {e}")))?;
    let event = parse_hook_payload(&payload)?;

    // Not an asp workspace → silently do nothing (user-scope hooks fire in
    // every project; that must be free of noise and side effects).
    let Ok(ws) = Workspace::open(&event.cwd) else {
        return Ok(());
    };

    let message = match event.event.as_str() {
        "PreToolUse" => format!("auto: before {}", event.tool),
        _ => format!("auto: after {}", event.tool),
    };
    ws.checkpoint(CheckpointOpts {
        message: Some(message),
        source: Some(Source::Hook),
        session_id: event.session_id,
        tool: Some(event.tool),
    })?;
    Ok(())
}

pub fn parse_hook_payload(payload: &Value) -> Result<HookEvent, Error> {
    let event = payload
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let tool = payload
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let session_id = payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let cwd = payload
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| Error::new(ErrorCode::Io, "no cwd in hook payload"))?;

    Ok(HookEvent {
        event,
        tool,
        session_id,
        cwd,
    })
}

#[derive(Debug, serde::Serialize)]
pub struct SetupReport {
    pub settings_file: PathBuf,
    pub mcp_file: Option<PathBuf>,
    pub hooks_installed: bool,
    pub mcp_registered: bool,
    pub removed: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct CodexSetupReport {
    pub config_file: PathBuf,
    pub user_scope: bool,
    pub mcp_registered: bool,
    pub removed: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct OpencodeSetupReport {
    pub config_file: PathBuf,
    pub user_scope: bool,
    pub mcp_registered: bool,
    pub removed: bool,
}

/// Install (or remove) the Claude Code integration.
pub fn setup_claude(root: &Path, user_scope: bool, remove: bool) -> Result<SetupReport, Error> {
    let settings_file = if user_scope {
        home_dir()?.join(".claude").join("settings.json")
    } else {
        root.join(".claude").join("settings.json")
    };

    let existing_settings = read_json_file(&settings_file)?;
    if remove && existing_settings.is_none() {
        // Nothing to remove; do not create files in untouched projects.
    } else {
        let mut settings = existing_settings.unwrap_or_else(|| json!({}));
        if remove {
            remove_our_hooks(&mut settings);
        } else {
            install_our_hooks(&mut settings);
        }
        write_json_file(&settings_file, &settings)?;
    }

    // MCP registration is project-level (.mcp.json at the workspace root).
    let mut mcp_file = None;
    if !user_scope {
        let path = root.join(".mcp.json");
        let existing_mcp = read_json_file(&path)?;
        if remove && existing_mcp.is_none() {
            // Nothing to remove; leave the project untouched.
        } else {
            let mut mcp = existing_mcp.unwrap_or_else(|| json!({}));
            if remove {
                if let Some(servers) = mcp.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                    servers.remove("agentspaces");
                }
            } else {
                mcp["mcpServers"]["agentspaces"] = json!({
                    "command": "asp",
                    "args": ["mcp"],
                });
            }
            write_json_file(&path, &mcp)?;
        }
        mcp_file = Some(path);
    }

    Ok(SetupReport {
        settings_file,
        mcp_registered: mcp_file.is_some() && !remove,
        mcp_file,
        hooks_installed: !remove,
        removed: remove,
    })
}

/// Install (or remove) the Codex MCP configuration.
pub fn setup_codex(root: &Path, user_scope: bool, remove: bool) -> Result<CodexSetupReport, Error> {
    let config_file = if user_scope {
        home_dir()?.join(".codex").join("config.toml")
    } else {
        root.join(".codex").join("config.toml")
    };

    let existing = read_text_file(&config_file)?;
    let next = if remove {
        remove_codex_block(existing.as_deref(), &config_file)?
    } else {
        Some(install_codex_block(
            existing.as_deref().unwrap_or(""),
            &config_file,
        )?)
    };

    if let Some(text) = next {
        if text.trim().is_empty() {
            if config_file.exists() {
                std::fs::remove_file(&config_file)?;
            }
        } else {
            write_text_file(&config_file, &text)?;
        }
    }

    Ok(CodexSetupReport {
        config_file,
        user_scope,
        mcp_registered: !remove,
        removed: remove,
    })
}

/// Install (or remove) the OpenCode MCP configuration.
pub fn setup_opencode(
    root: &Path,
    user_scope: bool,
    remove: bool,
) -> Result<OpencodeSetupReport, Error> {
    let config_file = if user_scope {
        home_dir()?
            .join(".config")
            .join("opencode")
            .join("opencode.json")
    } else {
        root.join("opencode.json")
    };

    let existing = read_json_file_for(&config_file, "asp setup opencode")?;
    let next = if remove {
        remove_opencode_server(existing, &config_file)?
    } else {
        Some(install_opencode_server(existing, &config_file)?)
    };

    if let Some(config) = next {
        let empty = config
            .as_object()
            .map(|object| object.is_empty())
            .unwrap_or(false);
        if empty && config_file.exists() {
            std::fs::remove_file(&config_file)?;
        } else {
            write_json_file(&config_file, &config)?;
        }
    }

    Ok(OpencodeSetupReport {
        config_file,
        user_scope,
        mcp_registered: !remove,
        removed: remove,
    })
}

fn codex_mcp_block() -> &'static str {
    r#"# agentspaces: begin asp setup codex
[mcp_servers.agentspaces]
command = "asp"
args = ["mcp"]
startup_timeout_sec = 10
tool_timeout_sec = 60
# agentspaces: end asp setup codex
"#
}

fn install_codex_block(existing: &str, path: &Path) -> Result<String, Error> {
    validate_codex_toml(path, existing)?;
    if let Some((start, end)) = codex_block_range(existing)? {
        let mut out = String::new();
        out.push_str(&existing[..start]);
        out.push_str(codex_mcp_block());
        out.push_str(&existing[end..]);
        validate_codex_toml(path, &out)?;
        return Ok(out);
    }

    if codex_has_agentspaces_server(existing, path)? {
        return Err(Error::new(
            ErrorCode::Io,
            format!(
                "{} already contains [mcp_servers.agentspaces]",
                path.display()
            ),
        )
        .with_hint(
            "rename or remove the existing Codex MCP server entry, then re-run `asp setup codex`",
        ));
    }

    let mut out = existing.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(codex_mcp_block());
    validate_codex_toml(path, &out)?;
    Ok(out)
}

fn remove_codex_block(existing: Option<&str>, path: &Path) -> Result<Option<String>, Error> {
    let Some(existing) = existing else {
        return Ok(None);
    };
    validate_codex_toml(path, existing)?;
    if let Some((start, end)) = codex_block_range(existing)? {
        let mut out = String::new();
        out.push_str(existing[..start].trim_end());
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(existing[end..].trim_start());
        validate_codex_toml(path, &out)?;
        return Ok(Some(out));
    }

    if codex_has_agentspaces_server(existing, path)? {
        return Err(Error::new(
            ErrorCode::Io,
            format!(
                "{} contains an unmanaged [mcp_servers.agentspaces]",
                path.display()
            ),
        )
        .with_hint(
            "remove or rename that Codex MCP server entry manually; asp will not delete unmanaged config",
        ));
    }

    Ok(None)
}

fn codex_block_range(text: &str) -> Result<Option<(usize, usize)>, Error> {
    let Some(start) = text.find(CODEX_BLOCK_START) else {
        return Ok(None);
    };
    let Some(end_offset) = text[start..].find(CODEX_BLOCK_END) else {
        return Err(Error::new(
            ErrorCode::Io,
            "Codex config contains an incomplete agentspaces managed block",
        )
        .with_hint("remove the partial block, then re-run `asp setup codex`"));
    };
    let mut end = start + end_offset + CODEX_BLOCK_END.len();
    if text[end..].starts_with("\r\n") {
        end += 2;
    } else if text[end..].starts_with('\n') {
        end += 1;
    }
    Ok(Some((start, end)))
}

fn codex_has_agentspaces_server(text: &str, path: &Path) -> Result<bool, Error> {
    if text.trim().is_empty() {
        return Ok(false);
    }
    let parsed: toml::Value = toml::from_str(text).map_err(|e| codex_toml_error(path, e))?;
    Ok(parsed
        .get("mcp_servers")
        .and_then(|servers| servers.get("agentspaces"))
        .is_some())
}

fn validate_codex_toml(path: &Path, text: &str) -> Result<(), Error> {
    if text.trim().is_empty() {
        return Ok(());
    }
    toml::from_str::<toml::Value>(text)
        .map(|_| ())
        .map_err(|e| codex_toml_error(path, e))
}

fn codex_toml_error(path: &Path, error: toml::de::Error) -> Error {
    Error::new(
        ErrorCode::Io,
        format!("{} is not valid TOML: {error}", path.display()),
    )
    .with_hint("fix the Codex config TOML, then re-run `asp setup codex`")
}

fn opencode_server_value() -> Value {
    json!({
        "type": "local",
        "command": ["asp", "mcp"],
        "enabled": true,
    })
}

fn install_opencode_server(existing: Option<Value>, path: &Path) -> Result<Value, Error> {
    let mut config = existing.unwrap_or_else(|| {
        json!({
            "$schema": "https://opencode.ai/config.json"
        })
    });
    ensure_json_object(&mut config, path, "root")?;
    if config.get("$schema").is_none() {
        config["$schema"] = json!("https://opencode.ai/config.json");
    }
    if config
        .get("mcp")
        .map(|mcp| !mcp.is_object())
        .unwrap_or(true)
    {
        config["mcp"] = json!({});
    }

    let existing_server = config["mcp"].get("agentspaces").cloned();
    if let Some(server) = existing_server {
        if !is_our_opencode_server(&server) {
            return Err(Error::new(
                ErrorCode::Io,
                format!("{} already contains mcp.agentspaces", path.display()),
            )
            .with_hint(
                "rename or remove the existing OpenCode MCP server entry, then re-run `asp setup opencode`",
            ));
        }
    }

    config["mcp"]["agentspaces"] = opencode_server_value();
    Ok(config)
}

fn remove_opencode_server(existing: Option<Value>, path: &Path) -> Result<Option<Value>, Error> {
    let Some(mut config) = existing else {
        return Ok(None);
    };
    ensure_json_object(&mut config, path, "root")?;
    let Some(mcp) = config.get_mut("mcp") else {
        return Ok(None);
    };
    if !mcp.is_object() {
        return Err(Error::new(
            ErrorCode::Io,
            format!("{} has non-object mcp config", path.display()),
        )
        .with_hint("fix the OpenCode config JSON, then re-run `asp setup opencode`"));
    }
    let Some(server) = mcp.get("agentspaces").cloned() else {
        return Ok(None);
    };
    if !is_our_opencode_server(&server) {
        return Err(Error::new(
            ErrorCode::Io,
            format!("{} contains unmanaged mcp.agentspaces", path.display()),
        )
        .with_hint(
            "remove or rename that OpenCode MCP server entry manually; asp will not delete unmanaged config",
        ));
    }
    mcp.as_object_mut()
        .expect("mcp is object")
        .remove("agentspaces");
    if mcp
        .as_object()
        .map(|object| object.is_empty())
        .unwrap_or(false)
    {
        config
            .as_object_mut()
            .expect("config is object")
            .remove("mcp");
    }
    Ok(Some(config))
}

fn is_our_opencode_server(server: &Value) -> bool {
    server
        .get("type")
        .and_then(|kind| kind.as_str())
        .is_some_and(|kind| kind == "local")
        && server.get("command") == Some(&json!(["asp", "mcp"]))
}

fn ensure_json_object(value: &mut Value, path: &Path, label: &str) -> Result<(), Error> {
    if value.is_object() {
        Ok(())
    } else {
        Err(Error::new(
            ErrorCode::Io,
            format!("{} {label} must be a JSON object", path.display()),
        )
        .with_hint("fix the OpenCode config JSON, then re-run `asp setup opencode`"))
    }
}

fn hook_command() -> Value {
    json!({ "type": "command", "command": "asp hook-event", "timeout": 60 })
}

fn our_groups() -> Vec<(&'static str, Value)> {
    vec![
        (
            "PostToolUse",
            json!({ "matcher": FILE_TOOLS, "hooks": [hook_command()] }),
        ),
        (
            "PostToolUse",
            json!({ "matcher": "Bash", "hooks": [hook_command()] }),
        ),
        (
            "PreToolUse",
            json!({ "matcher": "Bash", "hooks": [hook_command()] }),
        ),
    ]
}

fn is_ours(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.contains("asp hook-event"))
            })
        })
        .unwrap_or(false)
}

fn install_our_hooks(settings: &mut Value) {
    if !settings.is_object() {
        *settings = json!({});
    }
    if settings
        .get("hooks")
        .map(|h| !h.is_object())
        .unwrap_or(true)
    {
        settings["hooks"] = json!({});
    }
    for (event, group) in our_groups() {
        let arr = settings["hooks"]
            .as_object_mut()
            .expect("hooks is object")
            .entry(event)
            .or_insert_with(|| json!([]));
        if !arr.is_array() {
            *arr = json!([]);
        }
        let groups = arr.as_array_mut().expect("event list is array");
        let same_matcher_ours = |g: &Value| is_ours(g) && g.get("matcher") == group.get("matcher");
        if !groups.iter().any(same_matcher_ours) {
            groups.push(group);
        }
    }
}

fn remove_our_hooks(settings: &mut Value) {
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return;
    };
    for (_, list) in hooks.iter_mut() {
        if let Some(arr) = list.as_array_mut() {
            arr.retain(|g| !is_ours(g));
        }
    }
    hooks.retain(|_, list| list.as_array().map(|a| !a.is_empty()).unwrap_or(true));
}

fn read_json_file(path: &Path) -> Result<Option<Value>, Error> {
    read_json_file_for(path, "asp setup claude")
}

fn read_json_file_for(path: &Path, command: &str) -> Result<Option<Value>, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(None),
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| {
            Error::new(
                ErrorCode::Io,
                format!("{} is not valid JSON: {e}", path.display()),
            )
            .with_hint(format!("fix or remove the file, then re-run `{command}`"))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn write_json_file(path: &Path, value: &Value) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| Error::new(ErrorCode::Io, format!("encode: {e}")))?;
    asp_core::store::atomic_write(path, format!("{text}\n").as_bytes())
}

fn read_text_file(path: &Path) -> Result<Option<String>, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn write_text_file(path: &Path, text: &str) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    asp_core::store::atomic_write(path, text.as_bytes())
}

fn home_dir() -> Result<PathBuf, Error> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(ErrorCode::Io, "HOME is not set"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent_and_preserves_existing() {
        let mut settings = json!({
            "model": "opus",
            "hooks": {
                "PostToolUse": [
                    { "matcher": "Bash", "hooks": [{ "type": "command", "command": "my-linter" }] }
                ]
            }
        });
        install_our_hooks(&mut settings);
        install_our_hooks(&mut settings); // twice: no duplicates
        assert_eq!(settings["model"], "opus", "unrelated keys preserved");
        let post = settings["hooks"]["PostToolUse"].as_array().unwrap();
        // user's linter group + our file-tools group + our bash group
        assert_eq!(post.len(), 3, "{post:?}");
        assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);

        remove_our_hooks(&mut settings);
        let post = settings["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(post.len(), 1, "only the user's own hook remains");
        assert_eq!(post[0]["hooks"][0]["command"], "my-linter");
        assert!(settings["hooks"].get("PreToolUse").is_none());
    }
}
