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
        "effective checkpoint excludes",
        "large-file blob threshold",
        "promote branch template",
    ] {
        assert!(docs.contains(needle), "config docs missing {needle}");
    }
}
