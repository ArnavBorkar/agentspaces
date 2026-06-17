//! End-to-end engine tests: the full life of a workspace.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use asp_core::journal::{Entry, Op};
use asp_core::store::ForkStatus;
use asp_core::workspace::{CheckpointOpts, RetentionAction};
use asp_core::{ErrorCode, Workspace};

fn write(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
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
}

fn init_user_git(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "u@example.com"]);
    git(root, &["config", "user.name", "U"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "init"]);
}

fn child_names(root: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

/// Project dir with a few files, inside a fresh tempdir.
fn project() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir_all(&root).unwrap();
    write(&root, "src/main.rs", "fn main() {}\n");
    write(&root, "README.md", "# proj\n");
    write(&root, ".env", "SECRET=1\n");
    (tmp, root)
}

fn cp(ws: &Workspace, msg: &str) -> Option<asp_core::workspace::CheckpointInfo> {
    ws.checkpoint(CheckpointOpts {
        message: Some(msg.into()),
        ..Default::default()
    })
    .unwrap()
}

fn append_stale_checkpoint_entry(ws: &Workspace, seq: u64) {
    let commit = ws.checkpoint_refs().unwrap().get(&seq).unwrap().clone();
    let mut entry = Entry::new(Op::Checkpoint);
    entry.ts = "1970-01-01T00:00:00Z".to_string();
    entry.seq = Some(seq);
    entry.commit = Some(commit);
    entry.message = Some("stale checkpoint marker".to_string());
    ws.journal().append(&entry).unwrap();
}

#[test]
fn init_checkpoint_log_roundtrip() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    assert!(root.join(".asp/policy.toml").is_file());

    let c1 = cp(&ws, "first").expect("first checkpoint captures everything");
    assert_eq!(c1.seq, 1);
    assert!(c1.files_changed >= 3, "captures untracked files incl .env");

    // No-op capture creates no checkpoint.
    assert!(cp(&ws, "noop").is_none());

    write(&root, "src/main.rs", "fn main() { println!(\"hi\"); }\n");
    let c2 = cp(&ws, "second").unwrap();
    assert_eq!(c2.seq, 2);
    assert_eq!(c2.files_changed, 1);

    let log = ws.log(10).unwrap();
    assert_eq!(log[0].op, Op::Checkpoint);
    assert_eq!(log[0].seq, Some(2));
    assert_eq!(
        log[0].detail.as_ref().unwrap()["paths"],
        serde_json::json!(["src/main.rs"])
    );

    let status = ws.status().unwrap();
    assert_eq!(status.last_checkpoint.as_ref().unwrap().seq, 2);
    assert_eq!(status.dirty_files + status.untracked_files, 0);
}

#[test]
fn checkpoint_maintains_rebuildable_file_state_index() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    let c1 = cp(&ws, "base").unwrap();
    let index_path = root.join(".asp/file-state.json");

    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    assert_eq!(index["v"], 1);
    assert_eq!(index["head"], c1.commit);
    assert!(index["entries"]["src/main.rs"]["mtime_ms"].is_number());
    assert!(index["entries"].get(".asp/format-version").is_none());

    std::fs::write(&index_path, b"{").unwrap();
    assert!(cp(&ws, "noop").is_none());
    let rebuilt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    assert_eq!(rebuilt["head"], c1.commit);
    assert!(rebuilt["entries"]["README.md"]["size"].is_number());

    write(
        &root,
        "src/main.rs",
        "fn main() { println!(\"indexed\"); }\n",
    );
    let c2 = cp(&ws, "change").unwrap();
    let updated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    assert_eq!(updated["head"], c2.commit);
}

#[test]
fn noop_checkpoint_latency_stays_bounded_on_many_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir_all(root.join("src")).unwrap();
    for i in 0..1_200 {
        write(
            &root,
            &format!("src/pkg_{:03}/file_{:04}.rs", i / 40, i),
            &format!("pub fn f_{i}() -> usize {{ {i} }}\n"),
        );
    }
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();

    let started = Instant::now();
    assert!(cp(&ws, "noop").is_none());
    let elapsed = started.elapsed();
    let budget_ms = std::env::var("ASP_NOOP_LATENCY_BUDGET_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(4_000);
    assert!(
        elapsed <= Duration::from_millis(budget_ms),
        "no-op checkpoint took {elapsed:?}, budget {budget_ms}ms"
    );
}

#[test]
fn checkpoint_stages_only_changed_literal_paths() {
    let (_tmp, root) = project();
    for i in 0..120 {
        write(
            &root,
            &format!("src/many/file_{i:03}.txt"),
            &format!("v{i}\n"),
        );
    }
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();

    write(&root, "src/many/file_042.txt", "changed\n");
    write(&root, "src/path with spaces.txt", "new spaced path\n");
    write(&root, "src/-leading-dash.txt", "new dash path\n");

    let changed = cp(&ws, "literal paths").unwrap();
    assert_eq!(changed.files_changed, 3);

    let log = ws.log(1).unwrap();
    let paths = log[0].detail.as_ref().unwrap()["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![
            "src/-leading-dash.txt".to_string(),
            "src/many/file_042.txt".to_string(),
            "src/path with spaces.txt".to_string()
        ]
    );
    assert!(cp(&ws, "noop after literal paths").is_none());
}

#[test]
fn retention_plan_retains_latest_and_active_fork_points() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    write(&root, "src/main.rs", "fn main() { println!(\"v2\"); }\n");
    cp(&ws, "v2").unwrap();
    ws.fork(Some("active".into()), None).unwrap();
    write(&root, "README.md", "# proj\n\nv3\n");
    cp(&ws, "v3").unwrap();

    std::fs::write(
        root.join(".asp/policy.toml"),
        "[retention]\nkeep_last = 1\n",
    )
    .unwrap();
    let ws = Workspace::open(&root).unwrap();
    let plan = ws.retention_plan().unwrap();
    let entry = |seq| {
        plan.checkpoints
            .iter()
            .find(|entry| entry.seq == seq)
            .unwrap()
    };

    assert_eq!(entry(1).action, RetentionAction::Delete);
    assert_eq!(entry(1).reason, "outside_keep_last");
    assert_eq!(entry(2).action, RetentionAction::Retain);
    assert_eq!(entry(2).reason, "fork_point");
    assert_eq!(entry(3).action, RetentionAction::Retain);
    assert_eq!(entry(3).reason, "latest_checkpoint");
}

#[test]
fn invalid_policy_blocks_workspace_open() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    assert_eq!(ws.policy.forks.max_active, None);
    drop(ws);

    std::fs::write(root.join(".asp/policy.toml"), "[forks]\nmax_active = 0\n").unwrap();
    let err = Workspace::open(&root).unwrap_err();
    assert_eq!(err.code, ErrorCode::StoreCorrupt);
    assert!(err.message.contains("forks.max_active"));
    assert!(err.hint.unwrap().contains("policy.toml"));
}

#[test]
fn policy_enforces_max_active_forks() {
    let (_tmp, root) = project();
    Workspace::init(&root, None).unwrap();
    std::fs::write(root.join(".asp/policy.toml"), "[forks]\nmax_active = 1\n").unwrap();
    let ws = Workspace::open(&root).unwrap();

    let first = ws.fork(Some("one".into()), None).unwrap();
    assert!(first.path.exists());
    let err = ws.fork(Some("two".into()), None).unwrap_err();
    assert_eq!(err.code, ErrorCode::PolicyViolation);
    assert!(err.message.contains("forks.max_active"));
    assert!(!root.parent().unwrap().join("proj@two").exists());
}

#[test]
fn policy_enforces_checkpoint_age_before_risky_operations() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    append_stale_checkpoint_entry(&ws, 1);
    std::fs::write(
        root.join(".asp/policy.toml"),
        "[checkpoints]\nmax_age_hours = 1\n",
    )
    .unwrap();
    let ws = Workspace::open(&root).unwrap();

    let err = ws.fork(Some("late".into()), None).unwrap_err();
    assert_eq!(err.code, ErrorCode::PolicyViolation);
    assert!(err.message.contains("checkpoints.max_age_hours"));
    assert!(err.hint.unwrap().contains("asp checkpoint"));
    assert!(!root.parent().unwrap().join("proj@late").exists());
}

#[test]
fn policy_blocks_restore_of_protected_paths() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    write(&root, "src/main.rs", "broken\n");
    cp(&ws, "damage").unwrap();
    std::fs::write(
        root.join(".asp/policy.toml"),
        "[paths]\nprotected = [\"src/**\"]\n",
    )
    .unwrap();
    let ws = Workspace::open(&root).unwrap();

    let err = ws.restore("1", &[], None).unwrap_err();
    assert_eq!(err.code, ErrorCode::PolicyViolation);
    assert!(err.message.contains("protected path"));
    assert_eq!(read(&root, "src/main.rs"), "broken\n");
}

#[test]
fn policy_blocks_promote_of_protected_paths() {
    let (_tmp, root) = project();
    init_user_git(&root);
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    let fork = ws.fork(Some("winner".into()), None).unwrap();
    write(&fork.path, "src/main.rs", "fn main() { /* protected */ }\n");
    std::fs::write(
        root.join(".asp/policy.toml"),
        "[paths]\nprotected = [\"src/**\"]\n",
    )
    .unwrap();
    let ws = Workspace::open(&root).unwrap();

    let err = ws.promote("winner", None).unwrap_err();
    assert_eq!(err.code, ErrorCode::PolicyViolation);
    assert!(err.message.contains("protected path"));
    let missing = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/asp/winner"])
        .output()
        .unwrap();
    assert!(
        !missing.status.success(),
        "protected promote created a branch"
    );
}

#[test]
fn policy_enforces_promote_clean_status_and_branch_prefix() {
    let (_tmp, root) = project();
    init_user_git(&root);
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    let fork = ws.fork(Some("winner".into()), None).unwrap();
    write(&fork.path, "feature.txt", "ship it\n");
    std::fs::write(
        root.join(".asp/policy.toml"),
        "[promote]\nrequire_clean_status = true\nallowed_branch_prefixes = [\"review/\"]\n",
    )
    .unwrap();
    let ws = Workspace::open(&root).unwrap();

    write(&root, "dirty.txt", "main workspace change\n");
    let err = ws
        .promote("winner", Some("review/winner".into()))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PolicyViolation);
    assert!(err.message.contains("require_clean_status"));

    cp(&ws, "clean main").unwrap();
    let err = ws.promote("winner", Some("asp/winner".into())).unwrap_err();
    assert_eq!(err.code, ErrorCode::PolicyViolation);
    assert!(err.message.contains("allowed_branch_prefixes"));

    let ok = ws.promote("winner", Some("review/winner".into())).unwrap();
    assert_eq!(ok.branch, "review/winner");
}

#[test]
fn stats_reports_local_store_counts() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();

    let initial = ws.stats().unwrap();
    assert_eq!(initial.checkpoints, 0);
    assert_eq!(initial.journal_entries, 1);
    assert_eq!(initial.last_operation.as_ref().unwrap().op, Op::Init);
    assert!(initial.store_bytes > 0);

    cp(&ws, "base").unwrap();
    let fork = ws.fork(Some("measure".into()), None).unwrap();
    let stats = ws.stats().unwrap();
    assert_eq!(stats.checkpoints, 1);
    assert_eq!(stats.forks_total, 1);
    assert_eq!(stats.forks_active, 1);
    assert_eq!(stats.forks_pending, 0);
    assert_eq!(stats.forks_promoted, 0);
    assert_eq!(stats.forks_discarded, 0);
    assert_eq!(stats.last_operation.as_ref().unwrap().op, Op::Fork);
    assert!(stats.last_operation.as_ref().unwrap().duration_ms.is_some());
    assert!(stats
        .recent_operations
        .iter()
        .any(|op| op.op == Op::Checkpoint && op.duration_ms.is_some()));
    assert!(stats.store_bytes >= initial.store_bytes);
    assert!(fork.path.exists());
}

#[test]
fn mutating_journal_entries_include_durations() {
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
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "u@example.com"]);
    git(&["config", "user.name", "U"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "init"]);

    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    write(&root, "src/main.rs", "changed\n");
    cp(&ws, "changed").unwrap();
    ws.restore("1", &[], None).unwrap();
    let fork = ws.fork(Some("timed".into()), None).unwrap();
    write(&fork.path, "timed.txt", "promote me\n");
    ws.promote("timed", Some("asp/timed".into())).unwrap();
    ws.discard("timed", false).unwrap();

    let entries = ws.journal().read().unwrap().entries;
    for op in [Op::Restore, Op::Fork, Op::Promote, Op::Discard] {
        assert!(
            entries
                .iter()
                .any(|entry| entry.op == op && entry.duration_ms.is_some()),
            "missing duration for {op:?}: {entries:?}"
        );
    }
}

#[test]
fn diagnostics_redacts_paths_and_secretish_messages_by_default() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "token=sk-testdiagnosticsecret").unwrap();

    let redacted = ws.diagnostics(false).unwrap();
    let json = serde_json::to_string(&redacted).unwrap();
    assert_eq!(redacted.workspace.root, "<workspace-root>");
    assert!(redacted.redaction.paths_redacted);
    assert!(!json.contains(root.to_str().unwrap()), "{json}");
    assert!(!json.contains("sk-testdiagnosticsecret"), "{json}");
    assert!(json.contains("token=<redacted>"), "{json}");

    let unredacted = ws.diagnostics(true).unwrap();
    assert!(!unredacted.redaction.paths_redacted);
    assert_eq!(
        unredacted.workspace.root,
        root.canonicalize().unwrap().display().to_string(),
        "{unredacted:?}"
    );
}

#[test]
fn init_is_guarded() {
    let (_tmp, root) = project();
    Workspace::init(&root, None).unwrap();
    let err = Workspace::init(&root, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyInitialized);

    let nested = root.join("src");
    let err = Workspace::init(&nested, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyInitialized);

    let err = Workspace::open(Path::new("/")).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAWorkspace);
    assert!(err.hint.unwrap().contains("asp init"));
}

#[test]
fn restore_full_brings_back_deleted_and_removes_new() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();

    // Mutate: edit one, delete one, add one.
    write(&root, "src/main.rs", "broken\n");
    std::fs::remove_file(root.join("README.md")).unwrap();
    write(&root, "junk/garbage.tmp", "agent damage\n");
    let c2 = cp(&ws, "damage").unwrap();
    assert_eq!(c2.seq, 2);

    let report = ws.restore("1", &[], None).unwrap();
    assert_eq!(report.target_seq, 1);
    assert_eq!(read(&root, "src/main.rs"), "fn main() {}\n");
    assert_eq!(read(&root, "README.md"), "# proj\n");
    assert!(
        !root.join("junk/garbage.tmp").exists(),
        "added file removed"
    );
    assert!(!root.join("junk").exists(), "empty dir pruned");

    // Restore is itself undoable: the damage state was safety-checkpointed.
    assert!(
        report.safety_seq.is_none(),
        "tree was clean at restore time"
    );
}

#[test]
fn restore_takes_safety_checkpoint_when_dirty() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();

    write(&root, "src/main.rs", "uncommitted mess\n");
    let report = ws.restore("1", &[], None).unwrap();
    let safety = report.safety_seq.expect("dirty tree → safety checkpoint");
    assert_eq!(read(&root, "src/main.rs"), "fn main() {}\n");

    // The mess is recoverable.
    ws.restore(&safety.to_string(), &[], None).unwrap();
    assert_eq!(read(&root, "src/main.rs"), "uncommitted mess\n");
}

#[test]
fn restore_targeted_paths_only() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();

    write(&root, "src/main.rs", "v2\n");
    write(&root, "README.md", "# v2\n");
    cp(&ws, "v2").unwrap();

    ws.restore("1", &["src/main.rs".into()], None).unwrap();
    assert_eq!(read(&root, "src/main.rs"), "fn main() {}\n");
    assert_eq!(
        read(&root, "README.md"),
        "# v2\n",
        "untargeted path untouched"
    );
}

#[test]
fn undo_dirty_then_clean() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "one").unwrap();
    write(&root, "src/main.rs", "v2\n");
    cp(&ws, "two").unwrap();

    // Clean tree: undo steps back one checkpoint.
    ws.undo(None).unwrap();
    assert_eq!(read(&root, "src/main.rs"), "fn main() {}\n");

    // Dirty tree: undo reverts to last checkpoint... which is the restore
    // point we are at; make a fresh edit on top.
    write(&root, "src/main.rs", "scribble\n");
    ws.undo(None).unwrap();
    assert_eq!(read(&root, "src/main.rs"), "fn main() {}\n");

    let err = Workspace::init(&root, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyInitialized);
}

#[test]
fn undo_with_no_checkpoints_errors_helpfully() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    let err = ws.undo(None).unwrap_err();
    assert_eq!(err.code, ErrorCode::NothingToDo);
    assert!(err.hint.unwrap().contains("asp checkpoint"));
}

#[test]
fn fork_is_independent_and_compared() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();

    let f1 = ws.fork(Some("attempt-1".into()), None).unwrap();
    let f2 = ws.fork(Some("attempt-2".into()), None).unwrap();
    assert!(f1.path.exists() && f2.path.exists());
    assert_eq!(f1.name, "attempt-1");

    // Forks carry EVERYTHING, including dotfiles outside git.
    assert_eq!(read(&f1.path, ".env"), "SECRET=1\n");

    // CoW independence.
    write(&f1.path, "src/main.rs", "fork one wins\n");
    write(&f2.path, "src/lib.rs", "fork two adds a file\n");
    write(
        &f2.path,
        "Cargo.toml",
        "[package]\nname = \"fork-two\"\nversion = \"0.0.0\"\n",
    );
    assert_eq!(read(&root, "src/main.rs"), "fn main() {}\n");

    let rows = ws.fork_compare().unwrap();
    assert_eq!(rows.len(), 2);
    let r1 = rows.iter().find(|r| r.name == "attempt-1").unwrap();
    let r2 = rows.iter().find(|r| r.name == "attempt-2").unwrap();
    assert_eq!(r1.files_changed, 1);
    assert_eq!(r1.review.files_touched, 1);
    assert!(r1.review.risk_markers.is_empty());
    assert_eq!(r2.files_changed, 2);
    assert!(r2.insertions >= 1);
    assert!(r2
        .review
        .risk_markers
        .iter()
        .any(|marker| marker.kind == "dependency_manifest" && marker.path == "Cargo.toml"));
    assert_eq!(r2.review.risk_score, 20);

    // Duplicate active fork name is refused.
    let err = ws.fork(Some("attempt-1".into()), None).unwrap_err();
    assert_eq!(err.code, ErrorCode::ForkExists);
}

#[test]
fn fork_inside_fork_works() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    let f1 = ws.fork(Some("a".into()), None).unwrap();
    let fork_ws = Workspace::open(&f1.path).unwrap();
    assert!(fork_ws.meta.parent.is_some());
    write(&f1.path, "x.txt", "deep\n");
    let nested = fork_ws.fork(Some("b".into()), None).unwrap();
    assert!(nested.path.exists());
    assert_eq!(read(&nested.path, "x.txt"), "deep\n");
}

#[test]
fn diff_between_checkpoints_and_worktree() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    write(&root, "src/main.rs", "fn main() { /* changed */ }\n");
    write(&root, "new.txt", "added\n");
    std::fs::remove_file(root.join("README.md")).unwrap();
    cp(&ws, "changes").unwrap();

    let report = ws.diff("1", Some("2")).unwrap();
    assert_eq!(report.summary.files, 3);
    assert_eq!(
        report
            .summary
            .by_path
            .iter()
            .find(|bucket| bucket.name == "src/")
            .unwrap()
            .files,
        1
    );
    assert_eq!(
        report
            .summary
            .by_language
            .iter()
            .find(|bucket| bucket.name == "Rust")
            .unwrap()
            .files,
        1
    );
    assert_eq!(
        report
            .summary
            .by_change_type
            .iter()
            .find(|bucket| bucket.name == "added")
            .unwrap()
            .files,
        1
    );
    let by_path: std::collections::HashMap<_, _> =
        report.rows.iter().map(|r| (r.path.as_str(), r)).collect();
    assert_eq!(by_path["src/main.rs"].status, "M");
    assert_eq!(by_path["new.txt"].status, "A");
    assert_eq!(by_path["README.md"].status, "D");

    // Diff against the working tree.
    write(&root, "wip.txt", "work in progress\n");
    let report = ws.diff("2", None).unwrap();
    assert!(report.rows.iter().any(|r| r.path == "wip.txt"));
    assert_eq!(report.to, "working tree");
}

#[test]
fn promote_lands_branch_in_user_repo() {
    let (_tmp, root) = project();
    // Make the project a real git repo first (like most users).
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

    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    let fork = ws.fork(Some("winner".into()), None).unwrap();
    write(
        &fork.path,
        "src/main.rs",
        "fn main() { /* the good version */ }\n",
    );
    write(&fork.path, "src/new_module.rs", "pub fn add() {}\n");

    let report = ws.promote("winner", None).unwrap();
    assert_eq!(report.branch, "asp/winner");

    // The branch exists in the ORIGINAL repo with the fork's content,
    // and the user's HEAD/worktree were not touched.
    let tree = git(&["ls-tree", "-r", "--name-only", "asp/winner"]);
    assert!(tree.contains("src/new_module.rs"));
    assert!(
        !tree.contains(".asp"),
        "promote must never leak the .asp store into the branch:\n{tree}"
    );
    assert_eq!(read(&root, "src/main.rs"), "fn main() {}\n");
    let show = git(&["show", "asp/winner:src/main.rs"]);
    assert!(show.contains("the good version"));

    // Promoted fork can now be discarded without force.
    ws.discard("winner", false).unwrap();
    assert!(!fork.path.exists());

    // Promoting again: branch exists error.
    let f2 = ws.fork(Some("winner".into()), None).unwrap();
    write(&f2.path, "z.txt", "z\n");
    let err = ws.promote("winner", None).unwrap_err();
    assert_eq!(err.code, ErrorCode::BranchExists);
    let ok = ws.promote("winner", Some("asp/winner-2".into())).unwrap();
    assert_eq!(ok.branch, "asp/winner-2");
}

#[test]
fn promote_without_user_git_errors_helpfully() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    ws.fork(Some("f".into()), None).unwrap();
    let err = ws.promote("f", None).unwrap_err();
    assert_eq!(err.code, ErrorCode::NoUserGitRepo);
    assert!(err.hint.unwrap().contains("git init"));
}

#[test]
fn doctor_reports_promoted_fork_cleanup_candidate() {
    use asp_core::workspace::Severity;
    let (_tmp, root) = project();
    Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "-q"])
        .status()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["config", "user.name", "Test"])
        .status()
        .unwrap();

    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    let fork = ws.fork(Some("winner".into()), None).unwrap();
    write(&fork.path, "result.txt", "accepted\n");
    ws.promote("winner", None).unwrap();

    let findings = ws.doctor(false, false).unwrap();
    assert!(
        findings.iter().any(|f| {
            f.severity == Severity::Info
                && f.message.contains("was promoted")
                && f.message.contains("asp discard winner")
        }),
        "{findings:?}"
    );
}

#[test]
fn discard_guards_unpromoted_work() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    let fork = ws.fork(Some("risky".into()), None).unwrap();
    write(&fork.path, "src/main.rs", "unsaved work\n");

    let err = ws.discard("risky", false).unwrap_err();
    assert_eq!(err.code, ErrorCode::ForkHasUnpromotedWork);
    assert!(fork.path.exists(), "fork not deleted on refusal");

    ws.discard("risky", true).unwrap();
    assert!(!fork.path.exists());
    let registry = ws.fork_registry().unwrap();
    assert_eq!(registry.forks[0].status, ForkStatus::Discarded);
}

#[test]
fn excludes_keep_derived_state_out_of_checkpoints() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    write(&root, "node_modules/dep/index.js", "x\n");
    write(&root, "target/debug/binary", "x\n");
    let c = cp(&ws, "with noise").unwrap();
    let listed = ws
        .shadow()
        .run(&["ls-tree", "-r", "--name-only", &c.commit])
        .unwrap();
    assert!(!listed.contains("node_modules"));
    assert!(!listed.contains("target/"));
    assert!(listed.contains("src/main.rs"));
}

#[test]
fn user_git_dir_never_captured() {
    let (_tmp, root) = project();
    let out = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "-q"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let ws = Workspace::init(&root, None).unwrap();
    let c = cp(&ws, "base").unwrap();
    let listed = ws
        .shadow()
        .run(&["ls-tree", "-r", "--name-only", &c.commit])
        .unwrap();
    assert!(!listed.contains(".git/"));
    assert!(!listed.contains(".asp"));
}

#[test]
fn stock_git_recovery_runbook_works() {
    // The trust model, executed literally as documented in format.md.
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    let c = cp(&ws, "precious").unwrap();

    let out_dir = root.parent().unwrap().join("recovered");
    std::fs::create_dir_all(&out_dir).unwrap();
    let idx = root.parent().unwrap().join("tmp.index");
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .env("GIT_DIR", root.join(".asp/shadow.git"))
            .env("GIT_WORK_TREE", &out_dir)
            .env("GIT_INDEX_FILE", &idx)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["read-tree", &c.commit]);
    run(&["checkout-index", "-a", "-f"]);
    assert_eq!(read(&out_dir, "src/main.rs"), "fn main() {}\n");
    assert_eq!(read(&out_dir, ".env"), "SECRET=1\n");
}

#[test]
fn big_files_go_to_cas_sidecar_and_restore() {
    let (_tmp, root) = project();
    let _ = Workspace::init(&root, None).unwrap();
    // Lower the threshold to 1 MB for the test.
    std::fs::write(
        root.join(".asp/config.toml"),
        "[capture]\nblob_threshold_mb = 1\n",
    )
    .unwrap();
    let ws = Workspace::open(&root).unwrap(); // reload config

    let big_v1: Vec<u8> = (0..2 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    write_bytes(&root, "assets/model.bin", &big_v1);
    let c1 = cp(&ws, "with big file").unwrap();

    // The committed object at the big path is a small pointer, not 2 MB.
    let blob_size: u64 = ws
        .shadow()
        .run(&["cat-file", "-s", &format!("{}:assets/model.bin", c1.commit)])
        .unwrap()
        .parse()
        .unwrap();
    assert!(blob_size < 256, "pointer blob, got {blob_size} bytes");
    let cas_entries = std::fs::read_dir(root.join(".asp/blobs")).unwrap().count();
    assert_eq!(cas_entries, 1);

    // Status stays clean (big file is excluded + not in index).
    let st = ws.status().unwrap();
    assert_eq!(
        st.dirty_files + st.untracked_files + st.deleted_files,
        0,
        "{st:?}"
    );

    // Change the big file, checkpoint, then restore v1 — bytes must match.
    let big_v2: Vec<u8> = (0..3 * 1024 * 1024u32).map(|i| (i % 13) as u8).collect();
    write_bytes(&root, "assets/model.bin", &big_v2);
    cp(&ws, "big file v2").unwrap();
    assert_eq!(
        std::fs::read_dir(root.join(".asp/blobs")).unwrap().count(),
        2
    );

    ws.restore(&c1.seq.to_string(), &[], None).unwrap();
    assert_eq!(
        std::fs::read(root.join("assets/model.bin")).unwrap(),
        big_v1
    );

    // No-op capture after restore stays a no-op (no churn from pointers).
    assert!(cp(&ws, "noop").is_none());
}

fn write_bytes(root: &Path, rel: &str, content: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

#[test]
fn doctor_detects_and_repairs() {
    use asp_core::workspace::Severity;
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    let fork = ws.fork(Some("gone".into()), None).unwrap();

    // Healthy workspace: no findings.
    assert!(ws.doctor(false, false).unwrap().is_empty());

    // Damage 1: fork dir vanishes outside asp's control.
    std::fs::remove_dir_all(&fork.path).unwrap();
    // Damage 2: head ref tampered to an older commit.
    write(&root, "x.txt", "x\n");
    let c2 = cp(&ws, "two").unwrap();
    let refs = ws.checkpoint_refs().unwrap();
    let c1_commit = refs.get(&1).unwrap().clone();
    ws.shadow()
        .run(&["update-ref", "refs/asp/head", &c1_commit])
        .unwrap();
    // Damage 3: an unregistered look-alike directory (e.g. the user's own
    // `cp -r proj proj@torn`). Doctor must REPORT it and never delete it.
    let torn = root.parent().unwrap().join("proj@torn");
    asp_core::fork::clone_tree(&root, &torn).unwrap();

    let findings = ws.doctor(false, false).unwrap();
    assert!(findings.len() >= 3, "{findings:?}");
    assert!(findings.iter().all(|f| !f.fixed));

    let findings = ws.doctor(true, false).unwrap();
    assert!(
        findings.iter().filter(|f| f.fixed).count() >= 2,
        "{findings:?}"
    );
    assert!(
        torn.exists(),
        "doctor must never delete a directory it can't prove it created"
    );

    // Repairs hold: only the info-level look-alike note remains.
    let after = ws.doctor(false, false).unwrap();
    assert!(
        after.iter().all(|f| f.severity == Severity::Info),
        "{after:?}"
    );
    let _ = c2;
    assert!(!findings
        .iter()
        .any(|f| f.severity == Severity::Error && !f.fixed));
}

#[test]
fn doctor_repairs_shadow_git_config_drift() {
    use asp_core::workspace::Severity;
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();

    ws.shadow()
        .run(&["config", "core.compression", "9"])
        .unwrap();
    let findings = ws.doctor(false, false).unwrap();
    assert!(
        findings.iter().any(|f| {
            f.severity == Severity::Warning && !f.fixed && f.message.contains("core.compression")
        }),
        "{findings:?}"
    );

    let findings = ws.doctor(true, false).unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.fixed && f.message.contains("core.compression")),
        "{findings:?}"
    );
    let compression = ws
        .shadow()
        .run(&["config", "--get", "core.compression"])
        .unwrap();
    assert_eq!(compression, "0");
}

#[test]
fn big_file_shrink_below_threshold_round_trip() {
    let (_tmp, root) = project();
    let _ = Workspace::init(&root, None).unwrap();
    std::fs::write(
        root.join(".asp/config.toml"),
        "[capture]\nblob_threshold_mb = 1\n",
    )
    .unwrap();
    let ws = Workspace::open(&root).unwrap();

    let big: Vec<u8> = (0..2 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    write_bytes(&root, "data.log", &big);
    let c1 = cp(&ws, "big").unwrap();

    // Shrink below the threshold — an ordinary, everyday operation.
    std::fs::write(root.join("data.log"), b"tiny now\n").unwrap();
    let c2 = cp(&ws, "shrunk").expect("shrink is a real change");

    // The shrunken file must be IN the checkpoint tree with its new content.
    let listed = ws
        .shadow()
        .run(&["ls-tree", "-r", "--name-only", &c2.commit])
        .unwrap();
    assert!(
        listed.contains("data.log"),
        "shrunken file recorded as deleted!"
    );
    let content = ws
        .shadow()
        .run(&["cat-file", "-p", &format!("{}:data.log", c2.commit)])
        .unwrap();
    assert_eq!(content, "tiny now");

    // Restoring the shrink checkpoint must keep the file on disk...
    ws.restore(&c2.seq.to_string(), &[], None).unwrap();
    assert_eq!(read(&root, "data.log"), "tiny now\n");
    // ...and restoring the big version brings the bytes back.
    ws.restore(&c1.seq.to_string(), &[], None).unwrap();
    assert_eq!(std::fs::read(root.join("data.log")).unwrap(), big);
}

#[test]
fn doctor_deep_detects_corrupt_cas_blob() {
    use asp_core::workspace::Severity;
    let (_tmp, root) = project();
    let _ = Workspace::init(&root, None).unwrap();
    std::fs::write(
        root.join(".asp/config.toml"),
        "[capture]\nblob_threshold_mb = 1\n",
    )
    .unwrap();
    let ws = Workspace::open(&root).unwrap();
    write_bytes(&root, "asset.bin", &vec![3u8; 2 * 1024 * 1024]);
    cp(&ws, "big").unwrap();

    let bf = asp_core::blobs::load_bigfiles(&root.join(".asp/bigfiles.json")).unwrap();
    let entry = bf.files.get("asset.bin").unwrap();
    std::fs::write(root.join(".asp/blobs").join(&entry.blake3), b"corrupt").unwrap();

    assert!(
        ws.doctor(false, false).unwrap().is_empty(),
        "shallow doctor only checks that the CAS blob exists"
    );
    let findings = ws.doctor(false, true).unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.severity == Severity::Error && f.message.contains("is corrupt")),
        "{findings:?}"
    );
}

#[test]
fn restore_rejects_unsafe_store_paths() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    let c1 = cp(&ws, "base").unwrap();

    // Tamper: a malicious pointer manifest aiming outside the workspace.
    let evil = serde_json::json!({
        "v": 1,
        "pointers": [{ "path": "../escape.txt", "blake3": "ab".repeat(32), "size": 4 }]
    })
    .to_string();
    let oid = ws
        .shadow()
        .run_with_stdin(&["hash-object", "-w", "--stdin"], &evil)
        .unwrap();
    ws.shadow()
        .run(&["update-ref", &format!("refs/asp/meta/{}", c1.seq), &oid])
        .unwrap();

    let err = ws.restore(&c1.seq.to_string(), &[], None).unwrap_err();
    assert_eq!(err.code, ErrorCode::StoreCorrupt, "{err}");
    assert!(!root.parent().unwrap().join("escape.txt").exists());
}

#[test]
fn undo_walks_back_through_history() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    write(&root, "src/main.rs", "v1\n");
    cp(&ws, "one").unwrap();
    write(&root, "src/main.rs", "v2\n");
    cp(&ws, "two").unwrap();
    write(&root, "src/main.rs", "v3\n");
    cp(&ws, "three").unwrap();

    ws.undo(None).unwrap();
    assert_eq!(read(&root, "src/main.rs"), "v2\n", "first undo → v2");
    ws.undo(None).unwrap();
    assert_eq!(
        read(&root, "src/main.rs"),
        "v1\n",
        "second undo walks to v1, not back to v3"
    );
    let err = ws.undo(None).unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::NothingToDo,
        "bottom of history reached"
    );
}

#[test]
fn status_reports_big_file_edits() {
    let (_tmp, root) = project();
    let _ = Workspace::init(&root, None).unwrap();
    std::fs::write(
        root.join(".asp/config.toml"),
        "[capture]\nblob_threshold_mb = 1\n",
    )
    .unwrap();
    let ws = Workspace::open(&root).unwrap();
    write_bytes(&root, "model.bin", &vec![9u8; 2 * 1024 * 1024]);
    cp(&ws, "big").unwrap();
    let st = ws.status().unwrap();
    assert_eq!(st.dirty_files + st.untracked_files + st.deleted_files, 0);

    // Edit the big file: status must notice even though git can't see it.
    write_bytes(&root, "model.bin", &vec![8u8; 3 * 1024 * 1024]);
    let st = ws.status().unwrap();
    assert!(st.dirty_files >= 1, "{st:?}");
}

#[test]
fn pointer_residue_self_heals() {
    let (_tmp, root) = project();
    let _ = Workspace::init(&root, None).unwrap();
    std::fs::write(
        root.join(".asp/config.toml"),
        "[capture]\nblob_threshold_mb = 1\n",
    )
    .unwrap();
    let ws = Workspace::open(&root).unwrap();
    let big: Vec<u8> = (0..2 * 1024 * 1024u32).map(|i| (i % 13) as u8).collect();
    write_bytes(&root, "asset.bin", &big);
    cp(&ws, "big").unwrap();

    // Simulate crash residue: the real file replaced by its pointer JSON.
    let bf = asp_core::blobs::load_bigfiles(&root.join(".asp/bigfiles.json")).unwrap();
    let entry = bf.files.get("asset.bin").unwrap();
    std::fs::write(root.join("asset.bin"), asp_core::blobs::pointer_json(entry)).unwrap();

    // The next capture heals the file from the CAS instead of recording
    // 104 bytes of JSON as the asset.
    assert!(
        cp(&ws, "noop-after-heal").is_none(),
        "healed → no content change"
    );
    assert_eq!(std::fs::read(root.join("asset.bin")).unwrap(), big);
}

#[test]
fn doctor_cleans_pending_forks_only() {
    use asp_core::store::{ForkRegistry, ForkStatus};
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();
    let fork = ws.fork(Some("real".into()), None).unwrap();

    // Simulate a torn clone: flip the record back to Pending.
    let reg_path = root.join(".asp/forks.json");
    let mut reg: ForkRegistry = serde_json::from_slice(&std::fs::read(&reg_path).unwrap()).unwrap();
    reg.forks[0].status = ForkStatus::Pending;
    std::fs::write(&reg_path, serde_json::to_vec(&reg).unwrap()).unwrap();

    // A user's innocent look-alike directory next door.
    let innocent = root.parent().unwrap().join("proj@backup");
    std::fs::create_dir_all(innocent.join(".asp")).unwrap();
    std::fs::write(innocent.join("precious.txt"), "do not delete\n").unwrap();

    let findings = ws.doctor(true, false).unwrap();
    assert!(findings.iter().any(|f| f.fixed), "{findings:?}");
    assert!(!fork.path.exists(), "torn (pending) clone removed");
    assert!(
        innocent.join("precious.txt").exists(),
        "doctor must NEVER delete directories it can't prove it created"
    );
    let reg: ForkRegistry = serde_json::from_slice(&std::fs::read(&reg_path).unwrap()).unwrap();
    assert!(reg.forks.iter().all(|f| f.status != ForkStatus::Pending));
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_filenames_are_captured() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    cp(&ws, "base").unwrap();

    let weird = OsString::from_vec(b"caf\xe9.txt".to_vec());
    std::fs::write(root.join(&weird), "latin-1 name\n").unwrap();
    let c = cp(&ws, "non-utf8").expect("non-UTF-8 names must not break capture");
    assert!(c.files_changed >= 1);
    // And subsequent checkpoints still work.
    write(&root, "src/main.rs", "after\n");
    cp(&ws, "after").unwrap();
}

#[test]
fn restore_handles_case_only_renames() {
    // Found by proptest on macOS CI: checkpoint A has "L/a", state B has
    // "l/a" (same file on case-insensitive filesystems). Restoring A must
    // not let the deletion pass clobber the freshly-materialized file.
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    write(&root, "L/a", "upper content\n");
    let c1 = cp(&ws, "upper").unwrap();

    std::fs::remove_dir_all(root.join("L")).unwrap();
    write(&root, "l/a", "lower content\n");
    cp(&ws, "lower").unwrap();

    ws.restore(&c1.seq.to_string(), &[], None).unwrap();
    assert_eq!(read(&root, "L/a"), "upper content\n");
    // Round-trip back to the lowercase state too (explicit restore — undo
    // correctly walks BACKWARD from the restored position, not forward).
    ws.restore("2", &[], None).unwrap();
    assert!(child_names(&root).contains(&"l".to_string()));
    assert!(!child_names(&root).contains(&"L".to_string()));
    assert_eq!(read(&root, "l/a"), "lower content\n");
}

#[test]
fn restore_handles_top_level_case_only_renames() {
    let (_tmp, root) = project();
    let ws = Workspace::init(&root, None).unwrap();
    write(&root, "U", "upper content\n");
    let c1 = cp(&ws, "upper").unwrap();

    std::fs::remove_file(root.join("U")).unwrap();
    write(&root, "u", "lower content\n");
    cp(&ws, "lower").unwrap();

    ws.restore(&c1.seq.to_string(), &[], None).unwrap();
    assert!(child_names(&root).contains(&"U".to_string()));
    assert!(!child_names(&root).contains(&"u".to_string()));
    assert_eq!(read(&root, "U"), "upper content\n");
    ws.restore("2", &[], None).unwrap();
    assert!(child_names(&root).contains(&"u".to_string()));
    assert!(!child_names(&root).contains(&"U".to_string()));
    assert_eq!(read(&root, "u"), "lower content\n");
}
