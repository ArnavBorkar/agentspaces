# MCP Error Codes

`asp mcp` uses two error channels so clients can distinguish broken protocol
traffic from a workspace operation that ran and failed.

## Protocol Errors

Protocol errors are JSON-RPC failures with an `error` object at the top level.
They mean the server could not dispatch the request as an MCP operation.

| Code | Name | When it appears | Recovery |
| --- | --- | --- | --- |
| `-32700` | Parse error | A line is not valid JSON after UTF-8 loss-tolerant decoding. | Send exactly one valid JSON-RPC 2.0 object per newline. |
| `-32600` | Invalid request | The message is not an object, `jsonrpc` is not `"2.0"`, `method` is missing or not a string, or `id` is not a string, number, or null. | Fix the request envelope and retry. |
| `-32601` | Method not found | The method is not `initialize`, `ping`, `tools/list`, or `tools/call`. | Call `tools/list`, then use a listed tool through `tools/call`. |
| `-32602` | Invalid params | `tools/call` params are not an object, `name` is missing or not a string, or `arguments` is present but not an object. | Retry with `{ "name": "...", "arguments": { ... } }`. |

Requests without an `id` are notifications and do not receive a response.
Invalid request ids receive `id: null` because they cannot be safely correlated.
After any protocol error, the server keeps reading later lines on the same
stdio session.

## Tool Errors

Workspace tool failures are returned as JSON-RPC success responses because the
`tools/call` method itself was valid. The result has `isError: true`,
human-readable text in `content`, and a stable machine-readable error in
`structuredContent.error`:

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": {
    "isError": true,
    "content": [
      {
        "type": "text",
        "text": "this directory is not an asp workspace\nnext step: run `asp init` in your project root to create one"
      }
    ],
    "structuredContent": {
      "error": {
        "code": "not_a_workspace",
        "message": "this directory is not an asp workspace",
        "hint": "run `asp init` in your project root to create one"
      }
    }
  }
}
```

Clients should branch on `structuredContent.error.code`, show `message` to a
human when needed, and treat `hint` as the corrective next action. Do not parse
`content[0].text`; it is optimized for model readability and may add wording.

## Stable Tool Codes

These codes are the same stable enum used by `asp --json` errors.

| Code | Meaning | Typical recovery |
| --- | --- | --- |
| `not_a_workspace` | The target directory has no `.asp` store. | Run `workspace_init`, then retry. |
| `already_initialized` | The target directory already has an asp store. | Continue with normal workspace tools. |
| `git_missing` | `git` is not installed or not on `PATH`. | Install Git and retry. |
| `git_failed` | A Git command failed while operating on the shadow or user repo. | Read the message and hint; run `workspace_status` or `asp doctor` when advised. |
| `nothing_to_do` | The requested operation has no valid work to perform, or the tool name is unknown. | Check tool name, current state, or `workspace_log`. |
| `fork_exists` | A fork destination already exists. | Choose another fork name or discard the old fork. |
| `fork_not_found` | The named fork does not exist. | Call `workspace_forks` and retry with a listed fork. |
| `fork_has_unpromoted_work` | A discard would delete fork work that was not promoted. | Promote it first or retry discard with `force: true` after user confirmation. |
| `checkpoint_not_found` | A checkpoint number, commit prefix, or path target is invalid. | Call `workspace_log` or `workspace_diff` and retry with a valid reference. |
| `no_user_git_repo` | Promote needs a user Git repository and none was found. | Initialize or move into the real project Git repo before promoting. |
| `branch_exists` | Promote would overwrite an existing branch. | Choose a different branch name. |
| `policy_violation` | `.asp/policy.toml` blocks the requested workspace operation. | Follow the hint, usually by checkpointing, choosing an allowed branch, reducing active forks, narrowing paths, or editing policy after review. |
| `cross_volume` | A same-volume clone operation was required but impossible. | Put the workspace and fork destination on the same volume, or use a supported copy path. |
| `store_corrupt` | The `.asp` store or journal failed an integrity check. | Run `asp doctor`; preserve the directory for investigation if repair is not offered. |
| `format_too_new` | The store was created by a newer asp version. | Upgrade `asp` and retry. |
| `unsupported_platform` | The current OS/filesystem combination is intentionally unsupported. | Use a supported platform or follow the platform plan in the docs. |
| `locked` | Another asp process is mutating the same workspace. | Wait for that process to finish, then retry. |
| `io` | The OS or filesystem returned an unexpected I/O failure. | Check permissions, disk space, path validity, and retry. |

## Tool Matrix

Any workspace tool can return `git_missing`, `git_failed`, `store_corrupt`,
`format_too_new`, `unsupported_platform`, `locked`, or `io` when the local
environment or store prevents the operation. The table below lists the
user-correctable codes most specific to each MCP tool.

| MCP tool | Specific stable codes |
| --- | --- |
| `workspace_status` | `not_a_workspace` |
| `workspace_init` | `already_initialized` |
| `workspace_checkpoint` | `not_a_workspace` |
| `workspace_log` | `not_a_workspace` |
| `workspace_undo` | `not_a_workspace`, `nothing_to_do`, `checkpoint_not_found` |
| `workspace_restore` | `not_a_workspace`, `checkpoint_not_found`, `policy_violation` |
| `workspace_fork` | `not_a_workspace`, `fork_exists`, `policy_violation`, `cross_volume` |
| `workspace_forks` | `not_a_workspace` |
| `workspace_diff` | `not_a_workspace`, `checkpoint_not_found` |
| `workspace_promote` | `not_a_workspace`, `fork_not_found`, `no_user_git_repo`, `branch_exists`, `policy_violation` |
| `workspace_discard` | `not_a_workspace`, `fork_not_found`, `fork_has_unpromoted_work` |
| Unknown tool name passed to `tools/call` | `nothing_to_do` |

## Compatibility Rules

Changing the meaning of an existing code is a breaking automation change.
Adding a new code is allowed only with a schema update, snapshot update,
changelog entry, and documentation in this file.
