//! Property tests for the trust-bearing layers: journal crash recovery and
//! checkpoint/restore round-trips over arbitrary file trees.

use std::collections::BTreeMap;
use std::path::Path;

use asp_core::journal::{Entry, Journal, Op};
use asp_core::store::windows_path_violation;
use asp_core::workspace::CheckpointOpts;
use asp_core::Workspace;
use proptest::prelude::*;

// ---------------------------------------------------------------- journal

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Truncating the journal at ANY byte offset (a crash mid-append) must
    /// recover exactly the longest valid prefix of entries — no error, no
    /// entry loss before the cut, no phantom entries after it.
    #[test]
    fn journal_survives_truncation_anywhere(
        seqs in prop::collection::vec(1u64..1000, 1..20),
        cut_fraction in 0.0f64..1.0,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::new(dir.path().join("j.jsonl"));
        for s in &seqs {
            let mut e = Entry::new(Op::Checkpoint);
            e.seq = Some(*s);
            journal.append(&e).unwrap();
        }
        let bytes = std::fs::read(journal.path()).unwrap();
        let cut = (bytes.len() as f64 * cut_fraction) as usize;
        std::fs::write(journal.path(), &bytes[..cut]).unwrap();

        let report = journal.read().unwrap();
        prop_assert!(
            report.corrupt_lines.is_empty(),
            "tail damage is torn_tail, never corrupt_lines"
        );
        // The recovered entries are exactly a prefix of what was written.
        let recovered: Vec<u64> = report.entries.iter().filter_map(|e| e.seq).collect();
        prop_assert!(recovered.len() <= seqs.len());
        prop_assert_eq!(&seqs[..recovered.len()], &recovered[..]);
        // heal() repairs the tail; the journal accepts appends afterwards
        // and reads back clean.
        journal.heal().unwrap();
        let mut e = Entry::new(Op::Checkpoint);
        e.seq = Some(9999);
        journal.append(&e).unwrap();
        let again = journal.read().unwrap();
        prop_assert!(!again.torn_tail);
        prop_assert!(again.corrupt_lines.is_empty());
        prop_assert_eq!(again.entries.last().unwrap().seq, Some(9999));
    }

    /// Flipping a single byte anywhere must never make read() fail or
    /// fabricate entries — every surviving entry is one that was written.
    #[test]
    fn journal_corruption_never_fabricates(
        seqs in prop::collection::vec(1u64..1000, 2..12),
        flip_fraction in 0.0f64..1.0,
        xor in 1u8..255,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::new(dir.path().join("j.jsonl"));
        for s in &seqs {
            let mut e = Entry::new(Op::Checkpoint);
            e.seq = Some(*s);
            journal.append(&e).unwrap();
        }
        let mut bytes = std::fs::read(journal.path()).unwrap();
        let idx = ((bytes.len().saturating_sub(1)) as f64 * flip_fraction) as usize;
        bytes[idx] ^= xor;
        std::fs::write(journal.path(), &bytes).unwrap();

        let report = journal.read().unwrap();
        let written: std::collections::HashSet<u64> = seqs.iter().copied().collect();
        for e in &report.entries {
            if let Some(s) = e.seq {
                prop_assert!(written.contains(&s), "fabricated entry seq {s}");
            }
        }
    }
}

// ------------------------------------------------- checkpoint/restore

/// Strategy: a small file tree with safe-but-varied relative paths and
/// arbitrary binary content.
fn file_tree() -> impl Strategy<Value = BTreeMap<String, Vec<u8>>> {
    let name = "[a-zA-Z0-9][a-zA-Z0-9_.-]{0,12}";
    let rel_path = prop::collection::vec(
        name.prop_filter("no dots-only", |s| s != "." && s != ".."),
        1..4,
    )
    .prop_map(|parts| parts.join("/"))
    .prop_filter("Windows-portable checkpoint path", |path| {
        windows_path_violation(path).is_none()
    });
    prop::collection::btree_map(rel_path, prop::collection::vec(any::<u8>(), 0..512), 1..12)
        .prop_filter("case-insensitive-unique and no prefix-dir conflicts", |m| {
            // Lowercase comparisons: paths differing only by case are the
            // SAME file on default macOS APFS — a tree containing both is
            // unrepresentable on disk (filesystem semantics, not asp's).
            let keys: Vec<String> = m.keys().map(|k| k.to_lowercase()).collect();
            let unique: std::collections::HashSet<&String> = keys.iter().collect();
            if unique.len() != keys.len() {
                return false;
            }
            !keys.iter().any(|a| {
                keys.iter()
                    .any(|b| a != b && b.starts_with(&format!("{a}/")))
            })
        })
}

fn materialize(root: &Path, tree: &BTreeMap<String, Vec<u8>>) {
    // Clear previous user files.
    for entry in std::fs::read_dir(root).unwrap().filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".asp" || name == ".git" {
            continue;
        }
        let p = entry.path();
        if p.is_dir() {
            std::fs::remove_dir_all(p).unwrap();
        } else {
            std::fs::remove_file(p).unwrap();
        }
    }
    for (rel, content) in tree {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut map = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()) {
            let p = entry.path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().to_string();
            if rel.starts_with(".asp") || rel.starts_with(".git") {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                map.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    map
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// checkpoint(A); mutate to B; checkpoint(B); restore A == A; restore B == B.
    #[test]
    fn checkpoint_restore_round_trip(tree_a in file_tree(), tree_b in file_tree()) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ws");
        std::fs::create_dir_all(&root).unwrap();
        let ws = Workspace::init(&root, None).unwrap();

        materialize(&root, &tree_a);
        let a = ws.checkpoint(CheckpointOpts {
            message: Some("A".into()),
            ..Default::default()
        }).unwrap();
        let expect_a = snapshot(&root);

        materialize(&root, &tree_b);
        ws.checkpoint(CheckpointOpts {
            message: Some("B".into()),
            ..Default::default()
        }).unwrap();
        let expect_b = snapshot(&root);

        if let Some(a) = a {
            ws.restore(&a.seq.to_string(), &[], None).unwrap();
            prop_assert_eq!(snapshot(&root), expect_a);
        }
        // Find B's seq via the latest checkpoint ref that matches.
        let refs = ws.checkpoint_refs().unwrap();
        // restore to the checkpoint taken right after materializing B: it is
        // the highest seq created by an explicit "B" checkpoint — walk the log.
        let b_seq = ws
            .log(100).unwrap()
            .iter()
            .find(|e| e.message.as_deref() == Some("B"))
            .and_then(|e| e.seq);
        if let Some(b_seq) = b_seq {
            prop_assert!(refs.contains_key(&b_seq));
            ws.restore(&b_seq.to_string(), &[], None).unwrap();
            prop_assert_eq!(snapshot(&root), expect_b);
        }
    }
}
