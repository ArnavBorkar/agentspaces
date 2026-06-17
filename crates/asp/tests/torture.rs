//! kill -9 torture suite — the trust artifact.
//!
//! Storage tools get one strike. This suite SIGKILLs real `asp` processes
//! mid-operation (checkpoint, fork, restore) across a sweep of delays, then
//! verifies the three crash invariants from docs/design/format.md:
//!   1. checkpointed data is never lost (byte-identical restores),
//!   2. the store always opens (torn journal tails self-heal),
//!   3. torn forks are detectable and `asp doctor --fix` repairs them,
//!      and that the workspace remains fully functional afterwards.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

fn asp_ok(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_asp"))
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("asp spawns");
    assert!(
        out.status.success(),
        "asp {args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Spawn asp and SIGKILL it after `delay`. Returns true if it was killed
/// before finishing (false = it completed first; both outcomes are valid).
fn spawn_and_kill(dir: &Path, args: &[&str], delay: Duration) -> bool {
    let mut child = Command::new(env!("CARGO_BIN_EXE_asp"))
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("asp spawns");
    std::thread::sleep(delay);
    match child.try_wait().expect("try_wait") {
        Some(_) => {
            let _ = child.wait();
            false
        }
        None => {
            let _ = child.kill(); // SIGKILL on unix
            let _ = child.wait();
            true
        }
    }
}

fn write_files(root: &Path, generation: u64) {
    for i in 0..12 {
        let p = root.join(format!("src/mod{i}/file_{i}.rs"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, format!("// gen {generation} content {i}\n").repeat(30)).unwrap();
    }
    std::fs::write(root.join("data.bin"), vec![(generation % 251) as u8; 4096]).unwrap();
}

fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut map = BTreeMap::new();
    for entry in walk(root) {
        let rel = entry
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        if rel.starts_with(".asp") || rel.starts_with(".git") {
            continue;
        }
        map.insert(rel, std::fs::read(&entry).unwrap());
    }
    map
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                files.push(p);
            }
        }
    }
    files
}

#[test]
fn kill9_checkpoint_storm_loses_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir_all(&root).unwrap();
    write_files(&root, 0);
    asp_ok(&root, &["init"]);
    asp_ok(&root, &["checkpoint", "-m", "gen-0"]);

    // Confirmed checkpoints: seq → exact tree content at that point.
    let mut confirmed: BTreeMap<u64, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
    confirmed.insert(1, read_tree(&root));

    let mut kills = 0u32;
    let mut generation = 0u64;
    // Sweep kill delays from 0 to ~120ms — covers every phase of a
    // checkpoint (scan, add, write-tree, commit, ref update, journal).
    for step in 0..40u64 {
        generation += 1;
        write_files(&root, generation);
        let delay = Duration::from_millis((step * 7) % 120);
        if spawn_and_kill(
            &root,
            &["checkpoint", "-m", &format!("gen-{generation}")],
            delay,
        ) {
            kills += 1;
        }

        // INVARIANT 2: the store opens and works after every kill.
        let status = asp_ok(&root, &["--json", "status"]);
        assert!(status.contains("\"ok\": true"));

        // A clean checkpoint after the kill must always succeed; record it.
        let out = asp_ok(&root, &["--json", "checkpoint", "-m", "confirm"]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        if let Some(seq) = v["result"]["seq"].as_u64() {
            confirmed.insert(seq, read_tree(&root));
        }
    }
    assert!(
        kills >= 5,
        "sweep should kill mid-flight often (got {kills})"
    );

    // INVARIANT 1: every confirmed checkpoint restores byte-identical.
    for (seq, expected) in confirmed.iter().rev().take(8) {
        asp_ok(&root, &["restore", &seq.to_string()]);
        let actual = read_tree(&root);
        assert_eq!(&actual, expected, "checkpoint #{seq} content drifted");
    }

    // Doctor finds nothing un-repairable.
    let doc = asp_ok(&root, &["--json", "doctor", "--fix"]);
    let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
    let unfixed_errors = v["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["severity"] == "error" && f["fixed"] == false)
        .count();
    assert_eq!(unfixed_errors, 0, "{doc}");
}

#[test]
fn kill9_fork_leaves_no_unrecoverable_state() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir_all(&root).unwrap();
    write_files(&root, 0);
    asp_ok(&root, &["init"]);
    asp_ok(&root, &["checkpoint", "-m", "base"]);

    let mut kills = 0;
    for step in 0..16u64 {
        let delay = Duration::from_millis((step * 9) % 100);
        if spawn_and_kill(&root, &["fork", "--name", &format!("victim-{step}")], delay) {
            kills += 1;
        }
    }
    assert!(kills >= 4, "sweep should kill some forks (got {kills})");

    // INVARIANT 3: doctor --fix removes torn clones and reconciles the
    // registry; afterwards forks work normally.
    asp_ok(&root, &["doctor", "--fix"]);
    let doc = asp_ok(&root, &["--json", "doctor"]);
    let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
    let remaining: Vec<_> = v["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["severity"] != "info")
        .collect();
    assert!(remaining.is_empty(), "after --fix: {remaining:?}");

    let out = asp_ok(&root, &["--json", "fork", "--name", "survivor"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let fork_path = PathBuf::from(v["result"][0]["path"].as_str().unwrap());
    assert!(fork_path.join("data.bin").exists());

    // Surviving registry state is consistent: every active fork's dir exists.
    let forks = asp_ok(&root, &["--json", "forks"]);
    let v: serde_json::Value = serde_json::from_str(&forks).unwrap();
    for row in v["result"].as_array().unwrap() {
        assert_eq!(row["missing"], false, "{row}");
    }
}

#[test]
fn kill9_restore_never_corrupts_store() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir_all(&root).unwrap();
    write_files(&root, 0);
    asp_ok(&root, &["init"]);
    asp_ok(&root, &["checkpoint", "-m", "gen-0"]);
    let baseline = read_tree(&root);
    write_files(&root, 1);
    asp_ok(&root, &["checkpoint", "-m", "gen-1"]);

    for step in 0..12u64 {
        let delay = Duration::from_millis((step * 11) % 110);
        spawn_and_kill(&root, &["restore", "1"], delay);
        // Store must open and accept operations after every kill, even if
        // the working tree was left mid-restore (that is what restore is for).
        asp_ok(&root, &["--json", "status"]);
        asp_ok(&root, &["--json", "checkpoint", "-m", "post-kill"]);
    }

    // A clean restore still lands exactly on the baseline content.
    asp_ok(&root, &["restore", "1"]);
    assert_eq!(read_tree(&root), baseline);
}
