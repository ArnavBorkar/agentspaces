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
fn tracked_files_do_not_reference_old_clone_framing() {
    let root = repo_file(".");
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("ls-files")
        .output()
        .expect("git ls-files should run");

    assert!(
        output.status.success(),
        "git ls-files failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let forbidden = format!("{}{}", "arch", "ile");
    let mut offenders = Vec::new();
    for relative in String::from_utf8_lossy(&output.stdout).lines() {
        let path = root.join(relative);
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if text.to_ascii_lowercase().contains(&forbidden) {
            offenders.push(relative.to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "tracked files mention old clone framing: {offenders:?}"
    );
}

#[test]
fn backlog_tracks_next_enterprise_adoption_wave() {
    let backlog = fs::read_to_string(repo_file("BACKLOG.md")).unwrap();
    for needle in [
        "## Enterprise adoption roadmap, wave 2 (next 100 tasks)",
        "## EPIC 36 — Fleet onboarding and policy packs",
        "## EPIC 37 — User-owned remote sync backends",
        "## EPIC 38 — Native Windows support",
        "## EPIC 39 — Incident recovery and forensic drills",
        "## EPIC 40 — IDE and agent harness deep integrations",
        "## EPIC 41 — Scale and performance v2",
        "## EPIC 42 — Policy-as-code and local approvals",
        "## EPIC 43 — Review intelligence without SaaS",
        "## EPIC 44 — Security and privacy hardening v2",
        "## EPIC 45 — Ecosystem, docs, and community operations",
        "T36.1.1 Add `asp init --template <name>`",
        "T37.1.1 Add an S3-compatible remote adapter",
        "T38.1.1 Implement Windows-safe path handling",
        "T39.1.1 Add `asp drill recovery`",
        "T40.1.1 Add VS Code task and command-palette integration docs",
        "T41.1.1 Add a million-file benchmark fixture",
        "T42.1.1 Add path glob groups with owners and rationale fields",
        "T43.1.1 Add configurable risk markers",
        "T44.1.1 Add secret-scan baselines",
        "T45.2.5 Add governance docs for accepting new integrations",
    ] {
        assert!(backlog.contains(needle), "backlog missing {needle}");
    }

    let wave = backlog
        .split("## Enterprise adoption roadmap, wave 2 (next 100 tasks)")
        .nth(1)
        .and_then(|tail| tail.split("## Decision log").next())
        .expect("wave 2 roadmap section should exist");
    let roadmap_tasks = wave
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("- [") && line.contains("] T")
        })
        .count();

    assert_eq!(
        roadmap_tasks, 100,
        "wave 2 roadmap should contain exactly 100 tasks"
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
fn sync_credential_docs_cover_least_privilege_storage_scopes() {
    let docs = fs::read_to_string(repo_file("docs/sync-credentials.md")).unwrap();

    for needle in [
        "asp-sync/v1/workspaces/<workspace-id>/",
        "no deletes",
        "s3:ListBucket",
        "s3:GetObject",
        "s3:PutObject",
        "storage.objects.create",
        "storage.objects.update",
        "sp=rlcw",
        "Storage Blob Data Contributor includes delete",
        "prove delete fails for a synced object",
    ] {
        assert!(
            docs.contains(needle),
            "sync credential docs missing {needle}"
        );
    }

    let readme = fs::read_to_string(repo_file("README.md")).unwrap();
    assert!(
        readme.contains("docs/sync-credentials.md"),
        "README should link sync credential docs"
    );

    let sync = fs::read_to_string(repo_file("docs/sync.md")).unwrap();
    assert!(
        sync.contains("sync credential scopes"),
        "sync docs should link credential scopes"
    );
}

#[test]
fn sync_emulator_docs_cover_local_backend_fixtures() {
    let docs = fs::read_to_string(repo_file("docs/sync-emulators.md")).unwrap();

    for needle in [
        "scripts/sync-emulators.sh",
        "ASP_SYNC_S3_ENDPOINT",
        "ASP_SYNC_GCS_ENDPOINT",
        "ASP_SYNC_AZURE_ENDPOINT",
        "create an immutable object",
        "replace the ref only when the prior remote version matches",
        "do not delete remote objects",
    ] {
        assert!(docs.contains(needle), "sync emulator docs missing {needle}");
    }

    let readme = fs::read_to_string(repo_file("README.md")).unwrap();
    assert!(
        readme.contains("docs/sync-emulators.md"),
        "README should link sync emulator docs"
    );

    let sync = fs::read_to_string(repo_file("docs/sync.md")).unwrap();
    assert!(
        sync.contains("sync emulator fixtures"),
        "sync docs should link emulator fixtures"
    );
}

#[test]
fn sync_docs_cover_resumable_interruption_retries() {
    let sync = fs::read_to_string(repo_file("docs/sync.md")).unwrap();

    for needle in [
        "## Interrupted Sync",
        "resumable by rerunning the same command",
        "matching",
        "already\npresent",
        "missing bytes are uploaded or downloaded",
        "asp sync status --remote /path/to/asp-remote",
        "not git object or CAS blob payloads",
    ] {
        assert!(sync.contains(needle), "sync docs missing {needle}");
    }
}

#[test]
fn sync_recovery_docs_cover_remote_only_restore_runbook() {
    let docs = fs::read_to_string(repo_file("docs/sync-recovery.md")).unwrap();

    for needle in [
        "remote-only backup",
        "asp-sync/v1/workspaces/<workspace-id>/",
        "workspace.json",
        "refs/checkpoints",
        "objects/git/sha1",
        "objects/blobs/blake3",
        "git init --bare recovered-shadow.git",
        "git --git-dir recovered-shadow.git update-ref",
        "git --git-dir recovered-shadow.git archive",
        "large-file sidecars",
        "current CLI does not fully rebuild `.asp/`",
    ] {
        assert!(docs.contains(needle), "sync recovery docs missing {needle}");
    }

    let readme = fs::read_to_string(repo_file("README.md")).unwrap();
    assert!(
        readme.contains("docs/sync-recovery.md"),
        "README should link sync recovery docs"
    );

    let sync = fs::read_to_string(repo_file("docs/sync.md")).unwrap();
    assert!(
        sync.contains("sync remote recovery"),
        "sync docs should link remote recovery docs"
    );
}

#[test]
fn sync_encryption_docs_cover_remote_object_protection_design() {
    let docs = fs::read_to_string(repo_file("docs/sync-encryption.md")).unwrap();

    for needle in [
        "asp-sync/v2/encrypted/workspaces/<workspace-id>/",
        "root sync key",
        "object name MAC key",
        "xchacha20-poly1305",
        "HMAC of the logical sync key",
        "Associated data includes",
        "provider remote version",
        "Key rotation creates a new key id",
        "Remote-only recovery requires",
        "Add a `SyncRemote` decorator",
    ] {
        assert!(
            docs.contains(needle),
            "sync encryption docs missing {needle}"
        );
    }

    let readme = fs::read_to_string(repo_file("README.md")).unwrap();
    assert!(
        readme.contains("docs/sync-encryption.md"),
        "README should link sync encryption docs"
    );

    let sync = fs::read_to_string(repo_file("docs/sync.md")).unwrap();
    assert!(
        sync.contains("sync client-side encryption design"),
        "sync docs should link encryption design"
    );
}

#[test]
fn contributor_checklists_cover_schema_snapshot_updates() {
    let development = fs::read_to_string(repo_file("docs/development.md")).unwrap();
    for needle in [
        "Serialized CLI or MCP output changes update [docs/schemas.md]",
        "[schemas/](../schemas/)",
        "add or update JSON snapshots for changed",
        "automation-facing payloads",
    ] {
        assert!(
            development.contains(needle),
            "development checklist missing {needle}"
        );
    }

    let pr_template = fs::read_to_string(repo_file(".github/pull_request_template.md")).unwrap();
    for needle in [
        "User-facing commands support `--json`",
        "Serialized CLI/MCP output changes update schemas, docs, and JSON snapshots",
    ] {
        assert!(pr_template.contains(needle), "PR template missing {needle}");
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
fn windows_docs_cover_portable_checkpoint_path_guards() {
    let docs = fs::read_to_string(repo_file("docs/windows.md")).unwrap();

    for needle in [
        "Windows-portable shape",
        "reserved device names",
        "`CON`",
        "`NUL`",
        "alternate data streams",
        "trailing-space/trailing-dot",
        "overlong Windows components",
        "rename the path before checkpointing",
        "Supported macOS/Linux symlinks are preserved as links",
        "`unsupported_platform`",
        "junctions, mount\npoints, or other reparse points",
        "rejecting unsafe reparse points",
    ] {
        assert!(docs.contains(needle), "windows docs missing {needle}");
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
        "asp config diff --against baseline.toml",
        "asp --json config diff --against baseline.toml",
        "narrow read path",
        "field-level drift",
        "effective checkpoint excludes",
        "large-file blob threshold",
        "promote branch template",
    ] {
        assert!(docs.contains(needle), "config docs missing {needle}");
    }
}

#[test]
fn schema_docs_cover_config_result_contracts() {
    let docs = fs::read_to_string(repo_file("docs/schemas.md")).unwrap();
    for needle in [
        "asp config show --json",
        "asp config validate --json",
        "asp config diff --against <file> --json",
        "#/$defs/configShowReport",
        "#/$defs/configDiffReport",
        "`configShowReport`",
        "`configDiffReport`",
        "`matches` plus field-level `changes[]`",
        "successful results always carry `valid: true`",
        "schema-inventory-audit.md",
    ] {
        assert!(docs.contains(needle), "schema docs missing {needle}");
    }

    let schema_text = fs::read_to_string(repo_file("schemas/asp-result.schema.json")).unwrap();
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("result schema should be valid JSON");
    let defs = schema["$defs"].as_object().expect("schema defs object");
    for def in ["configShowReport", "configDiffReport", "workspaceConfig"] {
        assert!(defs.contains_key(def), "result schema missing {def}");
    }

    let variants = schema["anyOf"].as_array().expect("schema anyOf array");
    for def in ["configShowReport", "configDiffReport"] {
        let expected = format!("#/$defs/{def}");
        assert!(
            variants
                .iter()
                .any(|variant| variant["$ref"].as_str() == Some(expected.as_str())),
            "result schema anyOf missing {def}"
        );
    }
}

#[test]
fn schema_inventory_audit_tracks_known_result_map_gaps() {
    let docs = fs::read_to_string(repo_file("docs/schema-inventory-audit.md")).unwrap();

    for needle in [
        "## Covered Surfaces",
        "## Follow-Up Inventory",
        "asp quickstart --json",
        "asp completions <shell> --json",
        "asp manpage --json",
        "asp setup codex --json",
        "asp setup opencode --json",
        "asp diff --json --patch",
        "asp diff --json --stat",
        "asp diff --json --html --output review.html",
        "diffTextReport",
        "diffHtmlOutputResult",
        "asp doctor --json --runbook",
        "doctorRunbookReport",
        "asp policy explain --json",
        "policyExplainReport",
        "asp evidence manifest --packet file.json --output manifest.json --json",
        "evidenceManifestOutputResult",
        "asp evidence verify --packet file.json --manifest manifest.json --json",
        "evidenceVerifyReport",
        "_None currently._",
        "## Audit Rule",
        "the Result Map points at an existing shared schema",
    ] {
        assert!(
            docs.contains(needle),
            "schema inventory audit missing {needle}"
        );
    }
}

#[test]
fn schema_docs_cover_discovery_result_contracts() {
    let docs = fs::read_to_string(repo_file("docs/schemas.md")).unwrap();
    for needle in [
        "asp quickstart --json",
        "#/$defs/quickstartReport",
        "asp completions <shell> --json",
        "#/$defs/completionResult",
        "asp manpage --json",
        "#/$defs/manpageResult",
    ] {
        assert!(docs.contains(needle), "schema docs missing {needle}");
    }

    let schema_text = fs::read_to_string(repo_file("schemas/asp-result.schema.json")).unwrap();
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("result schema should be valid JSON");
    let defs = schema["$defs"].as_object().expect("schema defs object");
    for def in [
        "quickstartReport",
        "quickstartStep",
        "quickstartDoc",
        "completionResult",
        "manpageResult",
    ] {
        assert!(defs.contains_key(def), "result schema missing {def}");
    }

    let variants = schema["anyOf"].as_array().expect("schema anyOf array");
    for def in ["quickstartReport", "completionResult", "manpageResult"] {
        let reference = format!("#/$defs/{def}");
        assert!(
            variants.iter().any(|variant| variant["$ref"] == reference),
            "result schema anyOf missing {reference}"
        );
    }
}

#[test]
fn schema_docs_cover_setup_and_diff_variant_contracts() {
    let docs = fs::read_to_string(repo_file("docs/schemas.md")).unwrap();
    for needle in [
        "asp setup codex --json",
        "#/$defs/codexSetupReport",
        "asp setup opencode --json",
        "#/$defs/opencodeSetupReport",
        "asp diff --json --patch",
        "asp diff --json --stat",
        "#/$defs/diffTextReport",
        "asp diff --json --html --output review.html",
        "#/$defs/diffHtmlOutputResult",
    ] {
        assert!(docs.contains(needle), "schema docs missing {needle}");
    }

    let audit = fs::read_to_string(repo_file("docs/schema-inventory-audit.md")).unwrap();
    assert!(
        !audit.contains("Returns `setupReport`, but the Result Map only lists"),
        "setup variants should no longer be tracked as Result Map gaps"
    );
    assert!(
        !audit.contains("the Result Map omits"),
        "diff variants should no longer be tracked as Result Map gaps"
    );

    let schema_text = fs::read_to_string(repo_file("schemas/asp-result.schema.json")).unwrap();
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("result schema should be valid JSON");
    let defs = schema["$defs"].as_object().expect("schema defs object");
    for def in [
        "codexSetupReport",
        "opencodeSetupReport",
        "diffTextReport",
        "diffHtmlOutputResult",
    ] {
        assert!(defs.contains_key(def), "result schema missing {def}");
    }

    let variants = schema["anyOf"].as_array().expect("schema anyOf array");
    for def in [
        "codexSetupReport",
        "opencodeSetupReport",
        "diffTextReport",
        "diffHtmlOutputResult",
    ] {
        let reference = format!("#/$defs/{def}");
        assert!(
            variants.iter().any(|variant| variant["$ref"] == reference),
            "result schema anyOf missing {reference}"
        );
    }
}

#[test]
fn schema_docs_cover_doctor_runbook_contract() {
    let docs = fs::read_to_string(repo_file("docs/schemas.md")).unwrap();
    for needle in [
        "asp doctor --json --runbook",
        "#/$defs/doctorRunbookReport",
        "`common_runbooks` catalog",
    ] {
        assert!(docs.contains(needle), "schema docs missing {needle}");
    }

    let audit = fs::read_to_string(repo_file("docs/schema-inventory-audit.md")).unwrap();
    for stale_gap in [
        "missing `doctorRunbookReport`",
        "Add schema definition, Result Map row, and snapshot",
    ] {
        assert!(
            !audit.contains(stale_gap),
            "doctor runbook should no longer be tracked as a schema inventory gap"
        );
    }

    let schema_text = fs::read_to_string(repo_file("schemas/asp-result.schema.json")).unwrap();
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("result schema should be valid JSON");
    let defs = schema["$defs"].as_object().expect("schema defs object");
    for def in [
        "doctorFinding",
        "doctorFindingWithRunbook",
        "doctorRunbookLink",
        "doctorRunbookReport",
    ] {
        assert!(defs.contains_key(def), "result schema missing {def}");
    }

    let variants = schema["anyOf"].as_array().expect("schema anyOf array");
    assert!(
        variants
            .iter()
            .any(|variant| variant["$ref"] == "#/$defs/doctorRunbookReport"),
        "result schema anyOf missing doctorRunbookReport"
    );
}

#[test]
fn known_cli_json_surfaces_are_mapped_or_audited() {
    let schemas = fs::read_to_string(repo_file("docs/schemas.md")).unwrap();
    let audit = fs::read_to_string(repo_file("docs/schema-inventory-audit.md")).unwrap();

    let mapped = [
        "asp init --json",
        "asp init --print-template <name> --json",
        "asp status --json",
        "asp stats --json",
        "asp quickstart --json",
        "asp config show --json",
        "asp config validate --json",
        "asp config diff --against <file> --json",
        "asp bench self --json",
        "asp schema --json",
        "asp completions <shell> --json",
        "asp manpage --json",
        "asp audit --json",
        "asp policy validate --json",
        "asp policy explain --json",
        "asp preflight --json",
        "asp secrets scan --json",
        "asp evidence collect --json",
        "asp evidence collect --json --output file.json",
        "asp evidence manifest --packet file.json --output manifest.json --json",
        "asp evidence verify --packet file.json --manifest manifest.json --json",
        "asp retention plan --json",
        "asp sync status --json --remote <dir>",
        "asp sync push --json --remote <dir>",
        "asp sync fetch --json --remote <dir>",
        "asp checkpoint --json",
        "asp log --json",
        "asp undo --json",
        "asp restore --json",
        "asp fork --json",
        "asp forks --json",
        "asp review --json",
        "asp diff --json",
        "asp diff --json --patch",
        "asp diff --json --stat",
        "asp diff --json --html --output review.html",
        "asp promote --json",
        "asp discard --json",
        "asp race --json",
        "asp race compare --json",
        "asp setup claude --json",
        "asp setup codex --json",
        "asp setup opencode --json",
        "asp doctor --json",
        "asp doctor --json --runbook",
        "asp diagnostics --json",
        "asp diagnostics --json --output file.json",
    ];
    for command in mapped {
        assert!(
            schemas.contains(command),
            "mapped CLI JSON surface missing from docs/schemas.md: {command}"
        );
    }

    assert!(
        audit.contains("| _None currently._ |"),
        "schema inventory audit should state that no known CLI JSON surfaces are pending"
    );
}

#[test]
fn schema_docs_cover_sync_result_contracts() {
    let docs = fs::read_to_string(repo_file("docs/schemas.md")).unwrap();
    for (command, schema) in [
        (
            "asp sync status --json --remote <dir>",
            "#/$defs/syncStatusReport",
        ),
        (
            "asp sync push --json --remote <dir>",
            "#/$defs/syncPushReport",
        ),
        (
            "asp sync fetch --json --remote <dir>",
            "#/$defs/syncFetchReport",
        ),
    ] {
        assert!(docs.contains(command), "schema docs missing {command}");
        assert!(
            docs.contains(schema),
            "schema docs missing {schema} for {command}"
        );
    }

    let schema_text = fs::read_to_string(repo_file("schemas/asp-result.schema.json")).unwrap();
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("result schema should be valid JSON");
    let defs = schema["$defs"].as_object().expect("schema defs object");
    for def in [
        "syncStatusReport",
        "syncPushReport",
        "syncFetchReport",
        "syncRefConflict",
    ] {
        assert!(defs.contains_key(def), "result schema missing {def}");
    }
    assert!(
        defs["syncRefConflict"]["required"]
            .as_array()
            .expect("sync conflict required array")
            .iter()
            .any(|field| field.as_str() == Some("ref_name")),
        "syncRefConflict should require exact ref_name"
    );

    let variants = schema["anyOf"].as_array().expect("schema anyOf array");
    for def in ["syncStatusReport", "syncPushReport", "syncFetchReport"] {
        let reference = format!("#/$defs/{def}");
        assert!(
            variants.iter().any(|variant| variant["$ref"] == reference),
            "result schema anyOf missing {reference}"
        );
    }

    for needle in ["`ref_name`", "refs/asp/checkpoints/1"] {
        assert!(docs.contains(needle), "schema docs missing {needle}");
    }
}

#[test]
fn config_review_docs_cover_security_and_rollout_checks() {
    let docs = fs::read_to_string(repo_file("docs/config-review.md")).unwrap();

    for needle in [
        "asp config validate",
        "asp --json config show",
        "asp --json config diff --against baseline.toml",
        "asp policy validate --json",
        "asp secrets scan",
        "capture.excludes",
        "capture.extra_excludes",
        "capture.blob_threshold_mb",
        "promote.branch_template",
        "`.gitignore` alignment",
        "## JSON Review Artifact",
        "#/$defs/configShowReport",
        "#/$defs/configDiffReport",
        "`changes[]`",
        "\"shadow_excludes\"",
        "\"blob_threshold_bytes\"",
        "\"branch_template\": \"review/{workspace}/{fork}\"",
        "Rollout Pattern",
        "Red Flags",
    ] {
        assert!(docs.contains(needle), "config review docs missing {needle}");
    }
}

#[test]
fn policy_docs_cover_config_pairing_for_promotion_rules() {
    let docs = fs::read_to_string(repo_file("docs/policy.md")).unwrap();

    for needle in [
        "organization policy bundles",
        "policy-packs.md",
        "## Config Pairing",
        "asp --json config show > asp-config.json",
        "asp policy validate --json > asp-policy.json",
        "asp policy explain",
        "asp policy explain --json",
        "#/$defs/policyExplainReport",
        "`rules[]` entries",
        "`field`, `value`, `reason`, `affects`, and `enforced`",
        "which commands it affects",
        "asp-config.json.result.config.promote.branch_template",
        "asp-policy.json.result.policy.promote.allowed_branch_prefixes",
        "\"branch_template\": \"review/{workspace}/{fork}\"",
        "\"allowed_branch_prefixes\": [\"review/\"]",
        "fail that policy before `asp promote` creates a branch",
    ] {
        assert!(docs.contains(needle), "policy docs missing {needle}");
    }

    let schema_text = fs::read_to_string(repo_file("schemas/asp-result.schema.json")).unwrap();
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("result schema should be valid JSON");
    let defs = schema["$defs"].as_object().expect("schema defs object");
    for def in ["policyExplainReport", "policyExplanation"] {
        assert!(defs.contains_key(def), "result schema missing {def}");
    }
    let variants = schema["anyOf"].as_array().expect("schema anyOf array");
    assert!(
        variants
            .iter()
            .any(|variant| variant["$ref"].as_str() == Some("#/$defs/policyExplainReport")),
        "result schema anyOf missing policyExplainReport"
    );
}

#[test]
fn policy_pack_docs_cover_org_rollout_profiles() {
    let docs = fs::read_to_string(repo_file("docs/policy-packs.md")).unwrap();
    for needle in [
        "# Organization Policy Bundles",
        "## Regulated Workflow",
        "## Startup Workflow",
        "## OSS Maintainer Workflow",
        "asp policy validate",
        "asp policy explain",
        "asp --json policy explain > asp-policy-explain.json",
        "asp preflight",
        "forks.max_active",
        "checkpoints.max_age_hours",
        "paths.protected",
        "paths.deny_checkpoint",
        "promote.require_clean_status",
        "promote.require_checkpoint",
        "promote.allowed_branch_prefixes",
        "retention.keep_last",
        "retention.max_age_days",
        "allowed_branch_prefixes = [\"asp/reg/\", \"review/\"]",
        "allowed_branch_prefixes = [\"asp/\", \"ship/\"]",
        "allowed_branch_prefixes = [\"asp/\", \"contrib/\"]",
        "asp config diff --against <file>",
        "asp secrets scan",
        "fleet rollout checklist",
        "rollout handoff guide",
    ] {
        assert!(docs.contains(needle), "policy packs docs missing {needle}");
    }

    let readme = fs::read_to_string(repo_file("README.md")).unwrap();
    assert!(
        readme.contains("docs/policy-packs.md"),
        "README should link policy packs"
    );
    let policy = fs::read_to_string(repo_file("docs/policy.md")).unwrap();
    assert!(
        policy.contains("policy-packs.md"),
        "policy docs should link policy packs"
    );
    let rollout = fs::read_to_string(repo_file("docs/fleet-rollout.md")).unwrap();
    assert!(
        rollout.contains("policy-packs.md"),
        "fleet rollout docs should link policy packs"
    );
}

#[test]
fn config_template_docs_cover_common_repository_shapes() {
    let docs = fs::read_to_string(repo_file("docs/config-templates.md")).unwrap();

    for needle in [
        "asp init --template service",
        "asp init --template monorepo",
        "asp init --template generated-code",
        "asp init --template media-heavy",
        "asp init --print-template monorepo",
        "asp --json init --print-template monorepo",
        "## Service Repository",
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
        "fleet rollout checklist",
    ] {
        assert!(docs.contains(needle), "config templates missing {needle}");
    }
}

#[test]
fn fleet_rollout_docs_cover_multi_repo_adoption() {
    let docs = fs::read_to_string(repo_file("docs/fleet-rollout.md")).unwrap();
    for needle in [
        "# Fleet Rollout Checklist",
        "2-3 repositories",
        "10+ repos",
        "asp init --print-template monorepo",
        "asp init --template monorepo",
        "asp config validate",
        "asp --json config show > asp-config.json",
        "asp --json config diff --against baseline.toml > asp-config-diff.json",
        "asp preflight",
        "asp checkpoint -m \"rollout: baseline\"",
        "asp fork --name rollout-smoke",
        "asp doctor --deep",
        "asp evidence collect --output asp-evidence.json",
        "asp evidence verify",
        "Do not run `asp doctor --fix`",
        "asp setup claude",
        "asp setup codex",
        "asp setup opencode",
        "asp config diff --against <file>",
        "rollout-handoff.md",
        "## Rollback",
        "Only delete `.asp/`",
        "## Done When",
        "Support ticket templates",
    ] {
        assert!(docs.contains(needle), "fleet rollout docs missing {needle}");
    }

    let readme = fs::read_to_string(repo_file("README.md")).unwrap();
    assert!(
        readme.contains("docs/fleet-rollout.md"),
        "README should link fleet rollout checklist"
    );
}

#[test]
fn rollout_handoff_docs_cover_phase_rollback_and_owner_handoff() {
    let docs = fs::read_to_string(repo_file("docs/rollout-handoff.md")).unwrap();
    for needle in [
        "# Phased Rollout And Owner Handoff",
        "## Phase Gates",
        "0: Pilot",
        "1: Early teams",
        "2: Critical repos",
        "3: Default recommendation",
        "asp config validate",
        "asp config diff",
        "asp preflight",
        "asp doctor --deep",
        "## Rollback Levels",
        "Harness rollback",
        "Policy/config rollback",
        "Workspace rollback",
        "Do not run `asp doctor --fix`",
        "## Owner Handoff Packet",
        "# asp owner handoff",
        "Repository:",
        "Owner:",
        "Rollback owner:",
        "Evidence packet:",
        "asp --json config show > asp-config.json",
        "asp --json config diff --against baseline.toml > asp-config-diff.json",
        "asp --json policy explain > asp-policy-explain.json",
        "asp evidence verify --packet asp-evidence.json --manifest asp-evidence.manifest.json",
        "## Review Cadence",
        "Support ticket templates",
    ] {
        assert!(
            docs.contains(needle),
            "rollout handoff docs missing {needle}"
        );
    }

    let readme = fs::read_to_string(repo_file("README.md")).unwrap();
    assert!(
        readme.contains("docs/rollout-handoff.md"),
        "README should link rollout handoff docs"
    );
    let fleet = fs::read_to_string(repo_file("docs/fleet-rollout.md")).unwrap();
    assert!(
        fleet.contains("rollout-handoff.md"),
        "fleet rollout docs should link rollout handoff"
    );
    let policy_packs = fs::read_to_string(repo_file("docs/policy-packs.md")).unwrap();
    assert!(
        policy_packs.contains("rollout-handoff.md"),
        "policy packs should link rollout handoff"
    );
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
        "## Config Drift Gate",
        "asp config drift",
        ".ci/asp/config.baseline.toml",
        "asp --json config diff",
        "asp-config-diff.json",
        "unsafe_fields",
        "capture.excludes",
        "promote.branch_template",
        "shadow_excludes",
        "blob_threshold_bytes",
        "asp-config-diff-${{ github.sha }}",
        "asp_config_drift",
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
        "## Evidence Bundle Artifacts",
        "asp evidence collect --audit-limit 50 --output asp-evidence.json",
        "asp evidence manifest",
        "asp evidence verify",
        "asp-evidence.manifest.json",
        "asp-evidence.verify.txt",
        "asp-support-evidence-${{ github.sha }}",
        "if-no-files-found: warn",
        "## GitLab Evidence Bundle",
        "asp_evidence_bundle",
        "expire_in: 14 days",
        "## Generic SARIF Artifacts",
        "SARIF 2.1.0",
        "never uploads findings",
        "do not run `asp doctor --fix`",
        "without mutating the workspace",
        "They do not run",
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
        "## Signed Manifest",
        "asp-evidence.manifest.json",
        "asp evidence manifest",
        "asp evidence verify",
        "sha256",
        "created_by: \"asp evidence manifest\"",
        "exits nonzero",
        "Sign the manifest, not the packet",
        "## Sigstore Keyless Signing",
        "cosign sign-blob",
        "cosign verify-blob",
        "--certificate-oidc-issuer",
        "https://token.actions.githubusercontent.com",
        "Do not accept \"some valid Sigstore signature\"",
        "## Offline Minisign Signing",
        "minisign",
        "minisign -G -p asp-evidence.pub -s asp-evidence.sec",
        "asp-evidence.manifest.minisig",
        "Rotate the key",
        "support ticket templates",
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
fn support_ticket_templates_cover_evidence_handoff() {
    let docs = fs::read_to_string(repo_file("docs/support-ticket-templates.md")).unwrap();
    for needle in [
        "# Support Ticket Templates",
        "## Public Issue",
        "## Private Support Incident",
        "## Security Or Sensitive Data Report",
        "## CI Evidence Handoff",
        "## Collection Commands",
        "## Maintainer Intake",
        "asp evidence collect --output asp-evidence.json",
        "asp evidence manifest",
        "asp evidence verify",
        "asp-evidence.manifest.json",
        "redaction.paths_redacted",
        "redaction.secrets_redacted",
        "audit_messages_included",
        "audit_details_included",
        "Sigstore",
        "minisign",
        "cosign verify-blob",
        "asp-preflight.sarif",
        "asp-secrets.sarif",
        "retention expectation",
        "Do not ask for source archives",
    ] {
        assert!(
            docs.contains(needle),
            "support ticket templates missing {needle}"
        );
    }

    let readme = fs::read_to_string(repo_file("README.md")).unwrap();
    assert!(
        readme.contains("docs/support-ticket-templates.md"),
        "README should link support ticket templates"
    );
}

#[test]
fn schema_docs_cover_preflight_and_evidence_contracts() {
    let docs = fs::read_to_string(repo_file("docs/schemas.md")).unwrap();
    for needle in [
        "asp preflight --json",
        "#/$defs/preflightReport",
        "asp evidence collect --json",
        "#/$defs/evidenceReport",
        "asp evidence collect --json --output file.json",
        "#/$defs/evidenceOutputResult",
        "asp evidence manifest --packet file.json --output manifest.json --json",
        "#/$defs/evidenceManifestOutputResult",
        "asp evidence verify --packet file.json --manifest manifest.json --json",
        "#/$defs/evidenceVerifyReport",
        "preflight.config",
        "preflight.policy",
        "preflight.doctor",
        "preflight.secrets",
        "omitting free-form `message` and `detail` fields",
        "## Raw Export Formats",
        "asp preflight --sarif",
        "asp secrets scan --sarif",
        "SARIF 2.1.0",
        "`ruleId` values are stable preflight check IDs",
        "`ruleId` values use stable `secrets.<kind>` names",
        "Changing a SARIF `ruleId`",
        "switching to a different SARIF version is a breaking",
    ] {
        assert!(docs.contains(needle), "schema docs missing {needle}");
    }

    let schema_text = fs::read_to_string(repo_file("schemas/asp-result.schema.json")).unwrap();
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("result schema should be valid JSON");
    let defs = schema["$defs"].as_object().expect("schema defs object");
    for def in [
        "preflightReport",
        "preflightCheck",
        "preflightCheckId",
        "evidenceReport",
        "evidenceOutputResult",
        "evidenceManifest",
        "evidenceManifestOutputResult",
        "evidenceVerifyReport",
        "evidencePreflightReport",
        "evidenceAuditEvent",
    ] {
        assert!(defs.contains_key(def), "result schema missing {def}");
    }

    let variants = schema["anyOf"].as_array().expect("schema anyOf array");
    for def in [
        "preflightReport",
        "evidenceReport",
        "evidenceOutputResult",
        "evidenceManifestOutputResult",
        "evidenceVerifyReport",
    ] {
        let reference = format!("#/$defs/{def}");
        assert!(
            variants.iter().any(|variant| variant["$ref"] == reference),
            "result schema anyOf missing {reference}"
        );
    }
}

#[test]
fn schema_docs_cover_post_epic_30_machine_readable_outputs() {
    let docs = fs::read_to_string(repo_file("docs/schemas.md")).unwrap();

    let enveloped_outputs = [
        (
            "asp init --print-template <name> --json",
            "#/$defs/initTemplateResult",
        ),
        ("asp preflight --json", "#/$defs/preflightReport"),
        ("asp evidence collect --json", "#/$defs/evidenceReport"),
        (
            "asp evidence collect --json --output file.json",
            "#/$defs/evidenceOutputResult",
        ),
        (
            "asp evidence manifest --packet file.json --output manifest.json --json",
            "#/$defs/evidenceManifestOutputResult",
        ),
        (
            "asp evidence verify --packet file.json --manifest manifest.json --json",
            "#/$defs/evidenceVerifyReport",
        ),
    ];
    for (command, schema) in enveloped_outputs {
        assert!(docs.contains(command), "schema docs missing {command}");
        assert!(
            docs.contains(schema),
            "schema docs missing {schema} for {command}"
        );
    }

    let raw_outputs = [
        ("asp preflight --sarif", "SARIF 2.1.0"),
        ("asp secrets scan --sarif", "SARIF 2.1.0"),
    ];
    for (command, contract) in raw_outputs {
        assert!(docs.contains(command), "schema docs missing {command}");
        assert!(
            docs.contains(contract),
            "schema docs missing {contract} for {command}"
        );
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
