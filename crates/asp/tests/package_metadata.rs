use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_file(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(path)
}

#[test]
fn homebrew_formula_matches_release_matrix() {
    let formula = fs::read_to_string(repo_file("packaging/homebrew/Formula/asp.rb")).unwrap();
    let version = env!("CARGO_PKG_VERSION");

    assert!(
        formula.contains(&format!("version \"{version}\"")),
        "formula version should match crate version"
    );
    assert!(
        formula.contains("depends_on \"git\""),
        "formula should install Homebrew git for the runtime storage engine"
    );

    let targets = [
        (
            "aarch64-apple-darwin",
            "a105a90822024a7383f2991b4dad1be4a89c95fea2336c25dd7051a2dea7e03a",
        ),
        (
            "x86_64-apple-darwin",
            "7d195d178a78b4b67d3f9a50b386c76c5a01703bd208f4f9dfcc9cd687659b14",
        ),
        (
            "aarch64-unknown-linux-musl",
            "f3076d02108b1abf921b7abd2241c815b58eb1ed20d5ef5b842cda484a0add98",
        ),
        (
            "x86_64-unknown-linux-musl",
            "60b8ec2fe0d93acbb13a86c00a3f4676ba2749559b1a65055d0f7f9e37cc9ad2",
        ),
    ];

    for (target, sha256) in targets {
        let asset = format!("asp-v{version}-{target}.tar.gz");
        assert!(
            formula.contains(&asset),
            "formula should reference release asset {asset}"
        );
        assert!(
            formula.contains(&format!("sha256 \"{sha256}\"")),
            "formula should pin checksum for {asset}"
        );
    }

    assert_eq!(
        formula.matches("sha256 \"").count(),
        targets.len(),
        "formula should pin exactly one checksum per supported asset"
    );
}

#[test]
fn homebrew_formula_has_valid_ruby_syntax() {
    let formula = repo_file("packaging/homebrew/Formula/asp.rb");
    let output = Command::new("ruby")
        .arg("-c")
        .arg(&formula)
        .output()
        .expect("ruby should be available to syntax-check the Homebrew formula");

    assert!(
        output.status.success(),
        "ruby -c failed for {}\nstdout:\n{}\nstderr:\n{}",
        formula.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn generic_mcp_docs_include_supported_client_shapes() {
    let docs = fs::read_to_string(repo_file("docs/mcp-clients.md")).unwrap();

    assert!(
        docs.contains("\"mcpServers\""),
        "generic JSON shape missing"
    );
    assert!(
        docs.contains("\"command\": \"asp\""),
        "stdio command missing"
    );
    assert!(docs.contains("\"args\": [\"mcp\"]"), "stdio args missing");
    assert!(
        docs.contains("[mcp_servers.agentspaces]"),
        "Codex TOML shape missing"
    );
    assert!(
        docs.contains("\"type\": \"local\""),
        "OpenCode shape missing"
    );
    assert!(
        docs.contains("workspace_checkpoint"),
        "tool smoke-check guidance missing"
    );
}

#[test]
fn command_cheat_sheet_covers_daily_workflows() {
    let docs = fs::read_to_string(repo_file("docs/cheatsheet.md")).unwrap();

    let expected = [
        "## Start safely",
        "asp quickstart",
        "asp preflight",
        "asp init",
        "asp checkpoint -m \"baseline\"",
        "asp setup codex",
        "## Recover work",
        "asp restore 12 path/to/file",
        "asp doctor --runbook",
        "## Run agent races",
        "asp race -n 3",
        "## Land a winner",
        "asp promote race-2",
        "## Audit and policy",
        "asp secrets scan",
        "asp config show",
        "asp config validate --json",
        "## Sync and support",
        "asp sync push",
        "## Shell and packaging",
        "asp completions zsh",
        "asp manpage",
    ];

    for needle in expected {
        assert!(docs.contains(needle), "cheat sheet missing {needle}");
    }
}

#[test]
fn quickstart_docs_cover_safe_first_five_minutes() {
    let docs = fs::read_to_string(repo_file("docs/quickstart.md")).unwrap();

    for needle in [
        "asp quickstart",
        "read-only",
        "asp init",
        "asp checkpoint -m \"baseline\"",
        "asp race -n 3 -- <agent command>",
        "asp promote <name>",
        "asp undo",
        "asp doctor --runbook",
        "asp doctor --explain",
    ] {
        assert!(docs.contains(needle), "quickstart docs missing {needle}");
    }
}

#[test]
fn doctor_runbook_docs_cover_common_repair_scenarios() {
    let docs = fs::read_to_string(repo_file("docs/doctor-runbook.md")).unwrap();

    for needle in [
        "asp doctor --runbook",
        "## Shadow git config drift",
        "## Torn journal tail",
        "## Shadow HEAD drift",
        "## Missing active fork directory",
        "## Torn fork clone",
        "## Missing CAS blob recreatable",
        "## Journal CRC mismatch",
        "## Missing checkpoint commit",
        "## Corrupt CAS blob",
        "asp diagnostics --output diagnostics.json",
    ] {
        assert!(docs.contains(needle), "doctor runbook missing {needle}");
    }
}

#[test]
fn config_docs_cover_effective_config_inspection() {
    let docs = fs::read_to_string(repo_file("docs/config.md")).unwrap();

    for needle in [
        "asp config show",
        "asp --json config show",
        "asp config validate",
        "asp --json config validate",
        "narrow read path",
        "effective checkpoint excludes",
        "large-file blob threshold",
        "promote branch template",
    ] {
        assert!(docs.contains(needle), "config docs missing {needle}");
    }
}

#[test]
fn config_review_docs_cover_security_and_rollout_checks() {
    let docs = fs::read_to_string(repo_file("docs/config-review.md")).unwrap();

    for needle in [
        "asp config validate",
        "asp --json config show",
        "asp policy validate --json",
        "asp secrets scan",
        "capture.excludes",
        "capture.extra_excludes",
        "capture.blob_threshold_mb",
        "promote.branch_template",
        "`.gitignore` alignment",
        "Rollout Pattern",
        "Red Flags",
    ] {
        assert!(docs.contains(needle), "config review docs missing {needle}");
    }
}

#[test]
fn config_template_docs_cover_common_repository_shapes() {
    let docs = fs::read_to_string(repo_file("docs/config-templates.md")).unwrap();

    for needle in [
        "## Monorepo",
        "## Media-Heavy Repository",
        "## Generated-Code Repository",
        "extra_excludes",
        "blob_threshold_mb = 10",
        "branch_template = \"asp/{workspace}/{fork}\"",
        "branch_template = \"media/{workspace}/{fork}\"",
        "branch_template = \"gen/{workspace}/{fork}\"",
        "asp config validate",
        "asp --json config show",
    ] {
        assert!(docs.contains(needle), "config templates missing {needle}");
    }
}

#[test]
fn ignore_config_secrets_docs_cover_cross_file_ownership() {
    let docs = fs::read_to_string(repo_file("docs/ignore-config-secrets.md")).unwrap();

    for needle in [
        ".gitignore",
        ".asp/config.toml",
        ".asp/policy.toml",
        "asp secrets scan",
        "capture.extra_excludes",
        "protected paths",
        "git check-ignore -v",
        "asp config validate",
        "asp policy validate --json",
        "CI Gate",
        "Do not run `asp doctor --fix`",
    ] {
        assert!(
            docs.contains(needle),
            "ignore/config/secrets docs missing {needle}"
        );
    }
}

#[test]
fn preflight_docs_cover_ci_readiness_gate() {
    let docs = fs::read_to_string(repo_file("docs/preflight.md")).unwrap();

    for needle in [
        "asp preflight",
        "asp --json preflight",
        "asp preflight --deep",
        "asp preflight --sarif",
        ".asp/config.toml",
        ".asp/policy.toml",
        "asp doctor",
        "asp secrets scan",
        "exits nonzero",
        "runbook link",
        "stable check ID",
        "preflight.config",
        "preflight.secrets",
        "SARIF 2.1.0",
        "Do not pair it with `asp doctor --fix`",
    ] {
        assert!(docs.contains(needle), "preflight docs missing {needle}");
    }
}

#[test]
fn ci_docs_cover_preflight_examples() {
    let docs = fs::read_to_string(repo_file("docs/ci.md")).unwrap();

    for needle in [
        "## GitHub Actions",
        "actions/checkout@v4",
        "actions/upload-artifact@v4",
        "## GitLab CI",
        "## GitLab Code Quality",
        "gl-code-quality-report.json",
        "codequality: gl-code-quality-report.json",
        "asp --json secrets scan > asp-secrets.json",
        "asp config validate",
        "asp --json preflight > asp-preflight.json",
        "::error title=",
        "preflight.secrets",
        "github/codeql-action/upload-sarif@v3",
        "asp preflight --sarif",
        "asp secrets scan --sarif",
        "## Generic SARIF Artifacts",
        "SARIF 2.1.0",
        "never uploads findings",
        "do not run `asp doctor --fix`",
        "asp doctor --runbook",
    ] {
        assert!(docs.contains(needle), "CI docs missing {needle}");
    }
}

#[test]
fn secrets_docs_cover_sarif_output() {
    let docs = fs::read_to_string(repo_file("docs/secrets.md")).unwrap();

    for needle in [
        "asp secrets scan",
        "asp --json secrets scan",
        "asp secrets scan --sarif",
        "SARIF 2.1.0",
        "secrets.<kind>",
        "redacted",
        "workspace-relative file and line",
    ] {
        assert!(docs.contains(needle), "secrets docs missing {needle}");
    }
}

#[test]
fn evidence_docs_cover_redacted_local_packets() {
    let docs = fs::read_to_string(repo_file("docs/evidence.md")).unwrap();

    for needle in [
        "asp evidence collect",
        "asp --json evidence collect",
        "asp evidence collect --output asp-evidence.json",
        "redacted diagnostics bundle",
        "preflight readiness summary",
        "schema inventory",
        "recent audit event",
        "message",
        "detail",
        "--include-paths",
        "--deep",
        "does not upload anything",
        "## Review Checklist",
        "redaction.paths_redacted",
        "audit_details_included",
        "preflight.ready",
        "recent_audit_events",
        "private support channel",
    ] {
        assert!(docs.contains(needle), "evidence docs missing {needle}");
    }
}

#[test]
fn agent_preflight_docs_cover_harness_launch_checks() {
    let docs = fs::read_to_string(repo_file("docs/agent-preflight.md")).unwrap();

    for needle in [
        "asp preflight",
        "asp checkpoint -m \"before agent run\"",
        "asp setup codex",
        "asp setup opencode",
        "asp setup claude",
        "asp race -n 3",
        "asp doctor --runbook",
        "asp secrets scan",
        "asp undo",
        "timeout",
    ] {
        assert!(
            docs.contains(needle),
            "agent preflight docs missing {needle}"
        );
    }
}
