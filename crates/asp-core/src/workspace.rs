//! The Workspace: asp's primary API. Open/init a directory, then checkpoint,
//! fork, diff, restore, promote, discard against it. CLI and MCP server are
//! thin shells over this type.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::Serialize;

use crate::blobs::{self, BigFiles, Manifest, ManifestEntry};
use crate::config::Config;
use crate::error::{Error, ErrorCode, Result};
use crate::fork::{clone_tree, CloneMethod};
use crate::gitx::Shadow;
use crate::journal::{Entry, Journal, Op, Source};
use crate::store::{
    atomic_write, atomic_write_json, find_root, read_json, ForkRecord, ForkRegistry, ForkStatus,
    Layout, ParentRef, StoreLock, WorkspaceMeta, FORMAT_VERSION,
};

pub const CHECKPOINT_REF_PREFIX: &str = "refs/asp/checkpoints/";
pub const HEAD_REF: &str = "refs/asp/head";
pub const META_REF_PREFIX: &str = "refs/asp/meta/";

#[derive(Debug)]
pub struct Workspace {
    layout: Layout,
    pub meta: WorkspaceMeta,
    pub config: Config,
    shadow: Shadow,
    journal: Journal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckpointInfo {
    pub seq: u64,
    pub commit: String,
    pub files_changed: u64,
    pub duration_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub root: PathBuf,
    pub workspace_id: String,
    pub dirty_files: u64,
    pub untracked_files: u64,
    pub deleted_files: u64,
    pub last_checkpoint: Option<LastCheckpoint>,
    pub active_forks: u64,
    pub is_fork: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LastCheckpoint {
    pub seq: u64,
    pub ts: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForkInfo {
    pub name: String,
    pub path: PathBuf,
    pub fork_point_seq: u64,
    pub method: CloneMethod,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreReport {
    pub target_seq: u64,
    pub target_commit: String,
    pub safety_seq: Option<u64>,
    pub files_written: u64,
    pub files_deleted: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffRow {
    pub path: String,
    /// A=added, M=modified, D=deleted, T=type-change
    pub status: String,
    pub insertions: Option<u64>,
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffReport {
    pub from: String,
    pub to: String,
    pub rows: Vec<DiffRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForkCompareRow {
    pub name: String,
    pub status: ForkStatus,
    pub fork_point_seq: u64,
    pub files_changed: u64,
    pub insertions: u64,
    pub deletions: u64,
    pub last_activity: Option<String>,
    pub path: PathBuf,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromoteReport {
    pub fork: String,
    pub branch: String,
    pub commit: String,
}

struct ScanResult {
    /// (path, needs_add): every user-visible change, with whether the
    /// worktree differs from the index for it (index-only changes — e.g.
    /// staged deletions after a restore's read-tree — need no `git add`).
    changed: Vec<(String, bool)>,
    bigfiles: BigFiles,
    /// Paths with non-UTF-8 names exist (legal on Linux): they cannot ride
    /// a UTF-8 pathspec, so staging falls back to a full `git add -A .`.
    force_full_scan: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CheckpointOpts {
    pub message: Option<String>,
    pub source: Option<Source>,
    pub session_id: Option<String>,
    pub tool: Option<String>,
}

impl Workspace {
    // ----------------------------------------------------------------- open

    /// Open the workspace containing `start` (walks up like git does).
    pub fn open(start: &Path) -> Result<Self> {
        // Relative starts (-C dir, MCP `directory`) must become absolute:
        // the shadow git env requires cwd-independent paths.
        let canonical;
        let start = match start.canonicalize() {
            Ok(c) => {
                canonical = c;
                canonical.as_path()
            }
            Err(_) => start,
        };
        let root = find_root(start).ok_or_else(|| {
            Error::new(
                ErrorCode::NotAWorkspace,
                format!("no asp workspace found at or above {}", start.display()),
            )
            .with_hint("run `asp init` in your project root to create one")
        })?;
        Self::open_root(&root)
    }

    fn open_root(root: &Path) -> Result<Self> {
        let layout = Layout::new(root.to_path_buf());
        let version_text = std::fs::read_to_string(layout.format_version())?;
        let version: u32 = version_text.trim().parse().map_err(|_| {
            Error::new(ErrorCode::StoreCorrupt, "unreadable .asp/format-version")
                .with_hint("run `asp doctor`")
        })?;
        if version > FORMAT_VERSION {
            return Err(Error::new(
                ErrorCode::FormatTooNew,
                format!("workspace format v{version} is newer than this asp understands (v{FORMAT_VERSION})"),
            )
            .with_hint("upgrade asp: https://github.com/ArnavBorkar/agentspaces/releases"));
        }
        let meta: WorkspaceMeta = read_json(&layout.workspace_json())?;
        let config = Config::load(&layout.config_toml())?;
        let shadow = Shadow::new(
            layout.shadow_git(),
            layout.root.clone(),
            layout.shadow_index(),
        );
        let journal = Journal::new(layout.journal());
        // Crash recovery: reading validates and self-heals a torn tail.
        journal.read()?;
        Ok(Self {
            layout,
            meta,
            config,
            shadow,
            journal,
        })
    }

    /// Initialize a new workspace in `root` (adopts existing content as-is).
    pub fn init(root: &Path, label: Option<String>) -> Result<Self> {
        crate::gitx::ensure_git_version()?;
        let root = root.canonicalize().map_err(|e| {
            Error::new(
                ErrorCode::Io,
                format!("cannot resolve {}: {e}", root.display()),
            )
        })?;
        if root.join(crate::store::ASP_DIR).exists() {
            return Err(Error::new(
                ErrorCode::AlreadyInitialized,
                format!("{} is already an asp workspace", root.display()),
            )
            .with_hint("use `asp status` to inspect it"));
        }
        if let Some(existing) = find_root(&root) {
            if existing != root {
                return Err(Error::new(
                    ErrorCode::AlreadyInitialized,
                    format!(
                        "{} is inside the asp workspace at {}",
                        root.display(),
                        existing.display()
                    ),
                )
                .with_hint("asp workspaces don't nest; run commands from the existing workspace"));
            }
        }

        let layout = Layout::new(root.clone());
        std::fs::create_dir_all(&layout.asp)?;
        std::fs::create_dir_all(layout.blobs())?;

        let meta = WorkspaceMeta {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: crate::now_rfc3339(),
            label,
            parent: None,
        };
        let config = Config::default();

        let shadow = Shadow::new(
            layout.shadow_git(),
            layout.root.clone(),
            layout.shadow_index(),
        );
        shadow.init()?;
        shadow.write_excludes(&config.shadow_excludes())?;

        atomic_write_json(&layout.workspace_json(), &meta)?;
        atomic_write(&layout.config_toml(), Config::template().as_bytes())?;
        atomic_write_json(
            &layout.forks_json(),
            &ForkRegistry {
                v: 1,
                forks: vec![],
            },
        )?;
        let journal = Journal::new(layout.journal());
        journal.append(&Entry::new(Op::Init))?;
        // format-version is written LAST: its presence marks a complete store.
        atomic_write(
            &layout.format_version(),
            format!("{FORMAT_VERSION}\n").as_bytes(),
        )?;

        Ok(Self {
            layout,
            meta,
            config,
            shadow,
            journal,
        })
    }

    pub fn root(&self) -> &Path {
        &self.layout.root
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn shadow(&self) -> &Shadow {
        &self.shadow
    }

    // --------------------------------------------------------------- status

    pub fn status(&self) -> Result<StatusReport> {
        let raw = self.shadow.run_raw(&["status", "--porcelain", "-z"])?;
        // Managed big files live outside the index by design; their staged-
        // deletion entries are expected state, not user changes — as long as
        // the real file is still on disk.
        let bigfiles = blobs::load_bigfiles(&blobs::bigfiles_path(&self.layout.asp))?;
        let (mut modified, mut untracked, mut deleted) = (0u64, 0u64, 0u64);
        let mut iter = raw.split(|&b| b == 0).filter(|s| !s.is_empty());
        while let Some(entry) = iter.next() {
            if entry.len() < 3 {
                continue;
            }
            let xy = &entry[..2];
            if xy.contains(&b'R') || xy.contains(&b'C') {
                iter.next(); // rename/copy carries a second path record
            }
            if entry.len() > 3 {
                let path = String::from_utf8_lossy(&entry[3..]).to_string();
                if xy.contains(&b'D')
                    && bigfiles.files.contains_key(&path)
                    && self.layout.root.join(&path).is_file()
                {
                    continue;
                }
            }
            if xy == b"??" {
                untracked += 1;
            } else if xy.contains(&b'D') {
                deleted += 1;
            } else {
                modified += 1;
            }
        }
        // Managed big files are invisible to git status — stat-check them so
        // an edited 2GB asset still reads as a dirty workspace.
        for (path, rec) in &bigfiles.files {
            match self.layout.root.join(path).metadata() {
                Ok(md) if md.is_file() => {
                    if md.len() != rec.size || blobs::mtime_ms(&md) != rec.mtime_ms {
                        modified += 1;
                    }
                }
                _ => deleted += 1,
            }
        }
        let last = self.last_checkpoint()?;
        let registry = self.fork_registry()?;
        Ok(StatusReport {
            root: self.layout.root.clone(),
            workspace_id: self.meta.id.clone(),
            dirty_files: modified,
            untracked_files: untracked,
            deleted_files: deleted,
            last_checkpoint: last,
            active_forks: registry
                .forks
                .iter()
                .filter(|f| f.status == ForkStatus::Active)
                .count() as u64,
            is_fork: self.meta.parent.is_some(),
        })
    }

    fn last_checkpoint(&self) -> Result<Option<LastCheckpoint>> {
        let entries = self.journal.read()?.entries;
        Ok(entries
            .iter()
            .rev()
            .find(|e| e.op == Op::Checkpoint)
            .and_then(|e| {
                e.seq.map(|seq| LastCheckpoint {
                    seq,
                    ts: e.ts.clone(),
                    message: e.message.clone(),
                })
            }))
    }

    // ----------------------------------------------------------- checkpoint

    /// Capture the current state. Returns Ok(None) when nothing changed
    /// (no empty checkpoints — hook storms stay cheap).
    pub fn checkpoint(&self, opts: CheckpointOpts) -> Result<Option<CheckpointInfo>> {
        // Retry: concurrent auto-checkpoint hooks must not drop work.
        let _lock = StoreLock::acquire_with_retry(&self.layout)?;
        self.journal.heal()?;
        self.checkpoint_locked(opts)
    }

    fn checkpoint_locked(&self, opts: CheckpointOpts) -> Result<Option<CheckpointInfo>> {
        let t0 = Instant::now();
        let parent = self.shadow.rev_parse(HEAD_REF)?;
        let (tree, bigfiles) = self.stage_tree()?;
        if let Some(ref p) = parent {
            if self.shadow.tree_of(p)? == tree {
                return Ok(None);
            }
        }

        let message = opts
            .message
            .clone()
            .unwrap_or_else(|| "checkpoint".to_string());
        let commit = self
            .shadow
            .commit_tree(&tree, parent.as_deref(), &message)?;
        let seq = self.next_seq()?;
        self.shadow
            .update_ref(&format!("{CHECKPOINT_REF_PREFIX}{seq}"), &commit)?;

        // Pointer manifest for this checkpoint (restore needs it to know
        // which paths to materialize from the CAS).
        if !bigfiles.files.is_empty() {
            let manifest = Manifest {
                v: 1,
                pointers: bigfiles
                    .files
                    .iter()
                    .map(|(path, e)| ManifestEntry {
                        path: path.clone(),
                        blake3: e.blake3.clone(),
                        size: e.size,
                    })
                    .collect(),
            };
            let json = serde_json::to_string(&manifest)
                .map_err(|e| Error::new(ErrorCode::Io, format!("manifest encode: {e}")))?;
            let oid = self
                .shadow
                .run_with_stdin(&["hash-object", "-w", "--stdin"], &json)?;
            self.shadow
                .update_ref(&format!("{META_REF_PREFIX}{seq}"), &oid)?;
        }

        let files_changed = match parent {
            Some(ref p) => self.count_changed(p, &commit)?,
            None => self
                .shadow
                .run(&["ls-tree", "-r", "--name-only", &commit])?
                .lines()
                .count() as u64,
        };

        let mut entry = Entry::new(Op::Checkpoint);
        entry.seq = Some(seq);
        entry.commit = Some(commit.clone());
        entry.source = opts.source.clone();
        entry.session_id = opts.session_id.clone();
        entry.tool = opts.tool.clone();
        entry.message = Some(message.clone());
        entry.files_changed = Some(files_changed);
        entry.duration_ms = Some(t0.elapsed().as_millis() as u64);
        self.journal.append(&entry)?;
        self.shadow.update_ref(HEAD_REF, &commit)?;

        // Large captures leave tens of thousands of loose objects, which
        // makes whole-tree fork clones pay for every inode. Repack once so
        // the store collapses into a handful of files. Best-effort.
        if files_changed > 5_000 {
            let _ = self.shadow.run(&["repack", "-a", "-d", "-q"]);
        }

        Ok(Some(CheckpointInfo {
            seq,
            commit,
            files_changed,
            duration_ms: t0.elapsed().as_millis() as u64,
            message,
        }))
    }

    /// Pointer-aware staging: one status scan drives change detection, big-
    /// file maintenance, and a no-op fast path; only changed paths are staged
    /// (full scan above a threshold); pointer blobs are spliced in; pointer
    /// paths are removed from the index afterwards so the next capture skips
    /// them (untracked + excluded). Returns the tree oid + big-file set.
    fn stage_tree(&self) -> Result<(String, BigFiles)> {
        let ScanResult {
            changed,
            bigfiles,
            force_full_scan,
        } = self.scan_changes()?;

        let mut excludes = self.config.shadow_excludes();
        excludes.push("# --- asp generated: large-blob sidecar ---".to_string());
        for path in bigfiles.files.keys() {
            excludes.push(format!("/{path}"));
        }
        self.shadow.write_excludes(&excludes)?;

        // Fast path: nothing changed -> the head tree already describes the
        // working state exactly (the index deliberately lacks pointer paths,
        // so we must not write-tree here).
        let head = self.shadow.rev_parse(HEAD_REF)?;
        if changed.is_empty() && !force_full_scan {
            if let Some(ref h) = head {
                return Ok((self.shadow.tree_of(h)?, bigfiles));
            }
        }

        // Stage what changed. Above the threshold a full scan is cheaper
        // than a giant pathspec; big files never go through `add`.
        const FULL_SCAN_THRESHOLD: usize = 2000;
        let to_add: Vec<&str> = changed
            .iter()
            .filter(|(p, needs_add)| *needs_add && !bigfiles.files.contains_key(p.as_str()))
            .map(|(p, _)| p.as_str())
            .collect();
        if force_full_scan || changed.len() > FULL_SCAN_THRESHOLD {
            self.shadow.run(&["add", "-A", "."])?;
        } else if !to_add.is_empty() {
            self.shadow.run_with_stdin(
                &[
                    "--literal-pathspecs",
                    "add",
                    "-A",
                    "--pathspec-from-file=-",
                    "--pathspec-file-nul",
                ],
                &to_add.join("\0"),
            )?;
        }

        // Splice pointer blobs for all big files in ONE batched index call;
        // pointer oids are cached in bigfiles.json (computed only when a big
        // file is new or changed).
        let mut bigfiles = bigfiles;
        if !bigfiles.files.is_empty() {
            let mut oids_changed = false;
            for entry in bigfiles.files.values_mut() {
                if entry.pointer_oid.is_none() {
                    let oid = self.shadow.run_with_stdin(
                        &["hash-object", "-w", "--stdin"],
                        &blobs::pointer_json(entry),
                    )?;
                    entry.pointer_oid = Some(oid);
                    oids_changed = true;
                }
            }
            if oids_changed {
                blobs::save_bigfiles(&blobs::bigfiles_path(&self.layout.asp), &bigfiles)?;
            }
            let add_input: String = bigfiles
                .files
                .iter()
                .map(|(path, e)| {
                    format!(
                        "100644 {}\t{path}\n",
                        e.pointer_oid.as_deref().expect("oid cached above")
                    )
                })
                .collect();
            self.shadow
                .run_with_stdin(&["update-index", "--index-info"], &add_input)?;
        }
        let tree = self.shadow.run(&["write-tree"])?;
        if !bigfiles.files.is_empty() {
            // Mode 0 removes the entry — one spawn for all pointer paths.
            let rm_input: String = bigfiles
                .files
                .keys()
                .map(|path| format!("0 {}\t{path}\n", "0".repeat(40)))
                .collect();
            self.shadow
                .run_with_stdin(&["update-index", "--index-info"], &rm_input)?;
        }
        Ok((tree, bigfiles))
    }

    /// One `git status` scan shared by everything stage_tree needs: the list
    /// of user-visible changed paths (rename pairs included, managed big-file
    /// pointer deletions filtered out) and the maintained big-file cache.
    fn scan_changes(&self) -> Result<ScanResult> {
        let threshold = self.config.blob_threshold_bytes();
        let bf_path = blobs::bigfiles_path(&self.layout.asp);
        let mut bf = blobs::load_bigfiles(&bf_path)?;
        let mut bf_changed = false;

        // -uall expands untracked dirs into individual files so new big
        // files deep in fresh directories are seen.
        let raw = self
            .shadow
            .run_raw(&["status", "--porcelain", "-z", "-uall"])?;
        let mut changed: Vec<(String, bool)> = Vec::new();
        let mut force_full_scan = false;
        let mut iter = raw.split(|&b| b == 0).filter(|s| !s.is_empty());
        while let Some(entry) = iter.next() {
            if entry.len() < 4 {
                continue;
            }
            let xy = &entry[..2];
            // Y != ' ' means the worktree differs from the index; '??' is
            // untracked. Everything else is already staged in the index.
            let needs_add = xy == b"??" || xy[1] != b' ';
            let Ok(path) = std::str::from_utf8(&entry[3..]).map(str::to_string) else {
                // Non-UTF-8 filename: git itself handles the raw bytes via a
                // full-tree add; it just can't ride our pathspec.
                force_full_scan = true;
                if xy.contains(&b'R') || xy.contains(&b'C') {
                    iter.next();
                }
                continue;
            };
            if xy.contains(&b'R') || xy.contains(&b'C') {
                // Rename/copy: the following record is the source path; its
                // staged deletion is already in the index.
                if let Some(orig) = iter.next() {
                    changed.push((String::from_utf8_lossy(orig).to_string(), false));
                }
            }
            // The staged-deletion entry for a managed big file is asp's own
            // bookkeeping, not a user change.
            if xy.contains(&b'D')
                && bf.files.contains_key(&path)
                && self.layout.root.join(&path).is_file()
            {
                continue;
            }
            changed.push((path, needs_add));
        }

        if threshold > 0 {
            // Promote new big files out of the changed set into the CAS.
            for (path, _) in &changed {
                if bf.files.contains_key(path) {
                    continue;
                }
                let abs = self.layout.root.join(path);
                if let Ok(md) = abs.metadata() {
                    if md.is_file() && md.len() >= threshold {
                        let entry = blobs::store_in_cas(&self.layout.blobs(), &abs)?;
                        bf.files.insert(path.clone(), entry);
                        bf_changed = true;
                    }
                }
            }
            // Stat-check known big files (they are invisible to status):
            // rehash on change, demote on shrink, drop on delete — each of
            // those is a real change that must defeat the no-op fast path.
            let known: Vec<String> = bf.files.keys().cloned().collect();
            for path in known {
                let abs = crate::store::safe_rel_path(&self.layout.root, &path)?;
                match abs.metadata() {
                    Ok(md) if md.is_file() && md.len() >= threshold => {
                        let rec = &bf.files[&path];
                        if md.len() != rec.size || blobs::mtime_ms(&md) != rec.mtime_ms {
                            let entry = blobs::store_in_cas(&self.layout.blobs(), &abs)?;
                            bf.files.insert(path.clone(), entry);
                            bf_changed = true;
                            changed.push((path, false));
                        }
                    }
                    Ok(md) if md.is_file() => {
                        // Small file where a big one should be. If it is a
                        // pointer JSON left by a crash between checkout and
                        // CAS materialization, self-heal from the CAS.
                        if md.len() < 4096 {
                            if let Ok(bytes) = std::fs::read(&abs) {
                                if let Some(ptr) = blobs::parse_pointer(&bytes) {
                                    if self.layout.blobs().join(&ptr.blake3).exists() {
                                        blobs::restore_from_cas(
                                            &self.layout.blobs(),
                                            &ptr.blake3,
                                            &abs,
                                        )?;
                                        let md2 = abs.metadata()?;
                                        if let Some(rec) = bf.files.get_mut(&path) {
                                            rec.blake3 = ptr.blake3;
                                            rec.size = ptr.size;
                                            rec.mtime_ms = blobs::mtime_ms(&md2);
                                            rec.pointer_oid = None;
                                        }
                                        bf_changed = true;
                                        continue;
                                    }
                                }
                            }
                        }
                        // Genuinely shrunk below the threshold: it goes back
                        // to being an ordinary git blob — STAGE it, or the
                        // checkpoint would record a deletion of a live file.
                        bf.files.remove(&path);
                        bf_changed = true;
                        changed.push((path, true));
                    }
                    _ => {
                        bf.files.remove(&path);
                        bf_changed = true;
                        changed.push((path, false));
                    }
                }
            }
        }

        if bf_changed {
            blobs::save_bigfiles(&bf_path, &bf)?;
        }
        Ok(ScanResult {
            changed,
            bigfiles: bf,
            force_full_scan,
        })
    }

    /// Pointer manifest of a checkpoint, if it has one.
    pub fn manifest_for(&self, seq: u64) -> Result<Option<Manifest>> {
        let refname = format!("{META_REF_PREFIX}{seq}");
        let out = self.shadow.run_raw(&["cat-file", "-p", &refname]);
        match out {
            Ok(bytes) => {
                let m: Manifest = serde_json::from_slice(&bytes).map_err(|e| {
                    Error::new(
                        ErrorCode::StoreCorrupt,
                        format!("corrupt manifest for #{seq}: {e}"),
                    )
                    .with_hint("run `asp doctor`")
                })?;
                Ok(Some(m))
            }
            Err(_) => Ok(None),
        }
    }

    fn count_changed(&self, from: &str, to: &str) -> Result<u64> {
        let raw = self
            .shadow
            .run_raw(&["diff-tree", "-r", "-z", "--name-only", from, to])?;
        Ok(raw.split(|&b| b == 0).filter(|s| !s.is_empty()).count() as u64)
    }

    /// Next checkpoint seq: max(journal, refs) + 1 — robust to a crash
    /// between update-ref and journal append.
    fn next_seq(&self) -> Result<u64> {
        let journal_max = self.journal.last_seq()?.unwrap_or(0);
        let refs_max = self.checkpoint_refs()?.keys().max().copied().unwrap_or(0);
        Ok(journal_max.max(refs_max) + 1)
    }

    /// All checkpoint refs as seq → commit.
    pub fn checkpoint_refs(&self) -> Result<BTreeMap<u64, String>> {
        let out = self.shadow.run(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            CHECKPOINT_REF_PREFIX.trim_end_matches('/'),
        ])?;
        let mut map = BTreeMap::new();
        for line in out.lines() {
            if let Some((name, oid)) = line.split_once(' ') {
                if let Some(seq) = name
                    .strip_prefix(CHECKPOINT_REF_PREFIX)
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    map.insert(seq, oid.to_string());
                }
            }
        }
        Ok(map)
    }

    /// Resolve "42" / "#42" / commit-sha-prefix to (seq, commit).
    pub fn resolve_checkpoint(&self, spec: &str) -> Result<(u64, String)> {
        let refs = self.checkpoint_refs()?;
        let trimmed = spec.trim_start_matches('#');
        if let Ok(seq) = trimmed.parse::<u64>() {
            if let Some(commit) = refs.get(&seq) {
                return Ok((seq, commit.clone()));
            }
        }
        // sha prefix match
        let matches: Vec<(u64, String)> = refs
            .iter()
            .filter(|(_, c)| c.starts_with(trimmed))
            .map(|(s, c)| (*s, c.clone()))
            .collect();
        match matches.len() {
            1 => Ok(matches.into_iter().next().unwrap()),
            0 => Err(Error::new(
                ErrorCode::CheckpointNotFound,
                format!("no checkpoint matches '{spec}'"),
            )
            .with_hint("run `asp log` to list checkpoints; use the #seq number or commit prefix")),
            _ => Err(Error::new(
                ErrorCode::CheckpointNotFound,
                format!("'{spec}' is ambiguous ({} matches)", matches.len()),
            )
            .with_hint("use the #seq number from `asp log` instead")),
        }
    }

    /// Timeline entries, newest first.
    pub fn log(&self, limit: usize) -> Result<Vec<Entry>> {
        let mut entries = self.journal.read()?.entries;
        entries.reverse();
        entries.truncate(limit);
        Ok(entries)
    }

    // -------------------------------------------------------------- restore

    /// Restore the working tree to a checkpoint. A safety checkpoint is taken
    /// first, so restore itself is always undoable.
    pub fn restore(
        &self,
        spec: &str,
        paths: &[String],
        source: Option<Source>,
    ) -> Result<RestoreReport> {
        let _lock = StoreLock::acquire(&self.layout)?;
        self.journal.heal()?;
        let (target_seq, target_commit) = self.resolve_checkpoint(spec)?;

        // Validate targeted paths up front — a friendly error beats raw git.
        for p in paths {
            let listed = self
                .shadow
                .run(&["ls-tree", "--name-only", &target_commit, "--", p])?;
            if listed.is_empty() {
                return Err(Error::new(
                    ErrorCode::CheckpointNotFound,
                    format!("path '{p}' does not exist in checkpoint #{target_seq}"),
                )
                .with_hint(format!(
                    "see what that checkpoint contains: asp diff {target_seq}"
                )));
            }
        }

        let safety = self.checkpoint_locked(CheckpointOpts {
            message: Some(format!("auto: before restore to #{target_seq}")),
            source: source.clone(),
            ..Default::default()
        })?;
        let current = self
            .shadow
            .rev_parse(HEAD_REF)?
            .expect("head exists after safety checkpoint");

        let (files_written, files_deleted) = if paths.is_empty() {
            // Full restore: delete files that exist now but not in the
            // target FIRST, then materialize the target. Order matters on
            // case-insensitive filesystems: materializing `L/a` while the
            // old `l/a` still exists reuses the old name, and the deletion
            // pass afterwards would clobber the freshly-restored file.
            let raw = self.shadow.run_raw(&[
                "diff-tree",
                "-r",
                "-z",
                "--name-status",
                "--diff-filter=D",
                &current,
                &target_commit,
            ])?;
            let mut deleted = 0u64;
            let mut parts = raw.split(|&b| b == 0).filter(|s| !s.is_empty());
            while let (Some(_status), Some(path)) = (parts.next(), parts.next()) {
                // Store-supplied path: never let it escape the workspace.
                let p = crate::store::safe_rel_path(
                    &self.layout.root,
                    String::from_utf8_lossy(path).as_ref(),
                )?;
                if std::fs::remove_file(&p).is_ok() {
                    deleted += 1;
                    // Prune now-empty parent dirs up to the root.
                    let mut dir = p.parent().map(Path::to_path_buf);
                    while let Some(d) = dir {
                        if d == self.layout.root || std::fs::remove_dir(&d).is_err() {
                            break;
                        }
                        dir = d.parent().map(Path::to_path_buf);
                    }
                }
            }

            self.shadow.run(&["read-tree", &target_commit])?;
            self.shadow.run(&["checkout-index", "-a", "-f"])?;
            let written = self
                .shadow
                .run(&["ls-tree", "-r", "--name-only", &target_commit])?
                .lines()
                .count() as u64;
            // Materialize big files: replace restored pointer files with
            // their CAS content, and reset the big-file cache + index so the
            // next capture treats them correctly.
            if let Some(manifest) = self.manifest_for(target_seq)? {
                let mut bf = BigFiles {
                    v: 1,
                    files: Default::default(),
                };
                for ptr in &manifest.pointers {
                    let abs = crate::store::safe_rel_path(&self.layout.root, &ptr.path)?;
                    blobs::restore_from_cas(&self.layout.blobs(), &ptr.blake3, &abs)?;
                    self.shadow
                        .run(&["update-index", "--force-remove", "--", &ptr.path])?;
                    let md = abs.metadata()?;
                    bf.files.insert(
                        ptr.path.clone(),
                        crate::blobs::BigFileEntry {
                            blake3: ptr.blake3.clone(),
                            size: ptr.size,
                            mtime_ms: blobs::mtime_ms(&md),
                            pointer_oid: None,
                        },
                    );
                }
                blobs::save_bigfiles(&blobs::bigfiles_path(&self.layout.asp), &bf)?;
            } else {
                // Target predates any big files: clear the cache.
                blobs::save_bigfiles(
                    &blobs::bigfiles_path(&self.layout.asp),
                    &BigFiles {
                        v: 1,
                        files: Default::default(),
                    },
                )?;
            }
            (written, deleted)
        } else {
            // Targeted restore through a temp index; no deletions.
            let tmp_dir = tempfile::tempdir_in(&self.layout.asp)?;
            let scoped = Shadow::new(
                self.layout.shadow_git(),
                self.layout.root.clone(),
                tmp_dir.path().join("index"),
            );
            scoped.run(&["read-tree", &target_commit])?;
            let mut args: Vec<&str> = vec!["checkout-index", "-f", "--"];
            for p in paths {
                args.push(p);
            }
            scoped.run(&args)?;
            // Materialize any requested big files from the CAS.
            if let Some(manifest) = self.manifest_for(target_seq)? {
                let bf_path = blobs::bigfiles_path(&self.layout.asp);
                let mut bf = blobs::load_bigfiles(&bf_path)?;
                for ptr in manifest.pointers.iter().filter(|p| paths.contains(&p.path)) {
                    let abs = crate::store::safe_rel_path(&self.layout.root, &ptr.path)?;
                    blobs::restore_from_cas(&self.layout.blobs(), &ptr.blake3, &abs)?;
                    // mtime_ms: 0 marks the entry stale so the next capture
                    // re-stats it and recomputes the pointer — otherwise the
                    // no-op fast path could record the wrong tree.
                    bf.files.insert(
                        ptr.path.clone(),
                        crate::blobs::BigFileEntry {
                            blake3: ptr.blake3.clone(),
                            size: ptr.size,
                            mtime_ms: 0,
                            pointer_oid: None,
                        },
                    );
                }
                blobs::save_bigfiles(&bf_path, &bf)?;
            }
            // Re-sync the main index with reality for the next capture.
            self.shadow.run(&["add", "-A", "."])?;
            (paths.len() as u64, 0)
        };

        // Append a checkpoint recording the restored state, keeping the
        // timeline linear (undo-stack semantics: "where we are now" is
        // always the latest checkpoint). Full restores commit the TARGET's
        // tree deterministically — scanning could miss big-file-only deltas
        // (their paths are excluded from git status by design).
        let post = if paths.is_empty() {
            self.checkpoint_from_commit(
                &target_commit,
                target_seq,
                format!("auto: state after restore to #{target_seq}"),
                source.clone(),
            )?
        } else {
            self.checkpoint_locked(CheckpointOpts {
                message: Some(format!("auto: state after restore to #{target_seq}")),
                source: source.clone(),
                ..Default::default()
            })?
        };

        let mut entry = Entry::new(Op::Restore);
        entry.source = source;
        entry.detail = Some(serde_json::json!({
            "target_seq": target_seq,
            "target_commit": target_commit,
            "safety_seq": safety.as_ref().map(|c| c.seq),
            "post_seq": post.as_ref().map(|c| c.seq),
            "paths": paths,
        }));
        self.journal.append(&entry)?;

        Ok(RestoreReport {
            target_seq,
            target_commit,
            safety_seq: safety.map(|c| c.seq),
            files_written,
            files_deleted,
        })
    }

    /// Record a checkpoint whose tree is exactly `source_commit`'s tree
    /// (no scanning). Copies the source checkpoint's pointer manifest so
    /// restores of the new checkpoint materialize big files correctly.
    fn checkpoint_from_commit(
        &self,
        source_commit: &str,
        source_seq: u64,
        message: String,
        source: Option<Source>,
    ) -> Result<Option<CheckpointInfo>> {
        let t0 = Instant::now();
        let tree = self.shadow.tree_of(source_commit)?;
        let parent = self.shadow.rev_parse(HEAD_REF)?;
        if let Some(ref p) = parent {
            if self.shadow.tree_of(p)? == tree {
                return Ok(None);
            }
        }
        let commit = self
            .shadow
            .commit_tree(&tree, parent.as_deref(), &message)?;
        let seq = self.next_seq()?;
        self.shadow
            .update_ref(&format!("{CHECKPOINT_REF_PREFIX}{seq}"), &commit)?;
        if let Some(manifest_oid) = self
            .shadow
            .rev_parse_any(&format!("{META_REF_PREFIX}{source_seq}"))?
        {
            self.shadow
                .update_ref(&format!("{META_REF_PREFIX}{seq}"), &manifest_oid)?;
        }
        let files_changed = match parent {
            Some(ref p) => self.count_changed(p, &commit)?,
            None => 0,
        };
        let mut entry = Entry::new(Op::Checkpoint);
        entry.seq = Some(seq);
        entry.commit = Some(commit.clone());
        entry.source = source;
        entry.message = Some(message.clone());
        entry.files_changed = Some(files_changed);
        entry.duration_ms = Some(t0.elapsed().as_millis() as u64);
        self.journal.append(&entry)?;
        self.shadow.update_ref(HEAD_REF, &commit)?;
        Ok(Some(CheckpointInfo {
            seq,
            commit,
            files_changed,
            duration_ms: t0.elapsed().as_millis() as u64,
            message,
        }))
    }

    /// Undo: if the tree is dirty, revert to the current position; if clean,
    /// step back one checkpoint from the current position. The position
    /// follows restores — repeated undo WALKS BACK through history instead
    /// of ping-ponging between the last two states.
    pub fn undo(&self, source: Option<Source>) -> Result<RestoreReport> {
        let refs = self.checkpoint_refs()?;
        let latest = *refs.keys().next_back().ok_or_else(|| {
            Error::new(ErrorCode::NothingToDo, "no checkpoints exist yet")
                .with_hint("run `asp checkpoint` first — undo needs a point to return to")
        })?;

        // Current position: normally the latest checkpoint, but if the most
        // recent state change was a restore (e.g. a previous undo), we are
        // AT its target — stepping back must continue from there.
        let mut position = latest;
        let entries = self.journal.read()?.entries;
        if let Some(last_restore) = entries.iter().rev().find(|e| e.op == Op::Restore) {
            let post_seq = last_restore
                .detail
                .as_ref()
                .and_then(|d| d.get("post_seq"))
                .and_then(|v| v.as_u64());
            let target_seq = last_restore
                .detail
                .as_ref()
                .and_then(|d| d.get("target_seq"))
                .and_then(|v| v.as_u64());
            if let (Some(post), Some(target)) = (post_seq, target_seq) {
                // No checkpoints after the restore's post marker → still there.
                if latest <= post {
                    position = target;
                }
            }
        }

        let st = self.status()?;
        let dirty = st.dirty_files + st.untracked_files + st.deleted_files > 0;
        if dirty {
            self.restore(&position.to_string(), &[], source)
        } else {
            let prev = refs
                .range(..position)
                .next_back()
                .map(|(s, _)| *s)
                .ok_or_else(|| {
                    Error::new(ErrorCode::NothingToDo, "already at the first checkpoint")
                        .with_hint("nothing earlier to undo to; see `asp log`")
                })?;
            self.restore(&prev.to_string(), &[], source)
        }
    }

    // ----------------------------------------------------------------- fork

    pub fn fork(&self, label: Option<String>, source: Option<Source>) -> Result<ForkInfo> {
        let _lock = StoreLock::acquire(&self.layout)?;
        let t0 = Instant::now();

        // The fork point must be a real checkpoint.
        let cp = self.checkpoint_locked(CheckpointOpts {
            message: Some("auto: fork point".into()),
            source: source.clone(),
            ..Default::default()
        })?;
        let fork_point_seq = match cp {
            Some(c) => c.seq,
            None => self
                .checkpoint_refs()?
                .keys()
                .next_back()
                .copied()
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::StoreCorrupt,
                        "no checkpoint exists after capture",
                    )
                })?,
        };

        let mut registry = self.fork_registry()?;
        let name = match &label {
            Some(l) => sanitize_name(l),
            None => format!("fork-{}", registry.forks.len() + 1),
        };
        if registry
            .forks
            .iter()
            .any(|f| f.name == name && f.status == ForkStatus::Active)
        {
            return Err(Error::new(
                ErrorCode::ForkExists,
                format!("an active fork named '{name}' already exists"),
            )
            .with_hint(format!("pick another name, or `asp discard {name}` first")));
        }

        let dir_name = self
            .layout
            .root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".into());
        let parent_dir =
            self.layout.root.parent().ok_or_else(|| {
                Error::new(ErrorCode::Io, "workspace root has no parent directory")
            })?;
        let dst = parent_dir.join(format!("{dir_name}@{name}"));

        // Intent journaling: register the fork as Pending BEFORE the clone.
        // If we die mid-clone, doctor sees a Pending entry — deterministic
        // torn-state detection, no heuristics over look-alike directories.
        registry.forks.push(ForkRecord {
            name: name.clone(),
            path: dst.clone(),
            created_at: crate::now_rfc3339(),
            fork_point_seq,
            label: label.clone(),
            status: ForkStatus::Pending,
        });
        atomic_write_json(&self.layout.forks_json(), &registry)?;

        let method = match clone_tree(&self.layout.root, &dst) {
            Ok(m) => m,
            Err(e) => {
                // Clean up best-effort and withdraw the pending record.
                let _ = std::fs::remove_dir_all(&dst);
                registry
                    .forks
                    .retain(|f| !(f.name == name && f.status == ForkStatus::Pending));
                let _ = atomic_write_json(&self.layout.forks_json(), &registry);
                return Err(e);
            }
        };

        // Fix up the fork's identity: it is a new workspace with a parent.
        let fork_layout = Layout::new(dst.clone());
        let fork_meta = WorkspaceMeta {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: crate::now_rfc3339(),
            label: label.clone(),
            parent: Some(ParentRef {
                workspace_id: self.meta.id.clone(),
                fork_point_seq,
                fork_name: name.clone(),
            }),
        };
        atomic_write_json(&fork_layout.workspace_json(), &fork_meta)?;
        atomic_write_json(
            &fork_layout.forks_json(),
            &ForkRegistry {
                v: 1,
                forks: vec![],
            },
        )?;
        let fork_journal = Journal::new(fork_layout.journal());
        let mut fe = Entry::new(Op::Fork);
        fe.source = source.clone();
        fe.detail = Some(serde_json::json!({
            "role": "child", "name": name, "fork_point_seq": fork_point_seq,
            "parent_workspace": self.meta.id,
        }));
        fork_journal.append(&fe)?;

        // Clone + fixup complete: flip Pending → Active.
        if let Some(rec) = registry
            .forks
            .iter_mut()
            .find(|f| f.name == name && f.status == ForkStatus::Pending)
        {
            rec.status = ForkStatus::Active;
        }
        atomic_write_json(&self.layout.forks_json(), &registry)?;
        let mut entry = Entry::new(Op::Fork);
        entry.source = source;
        entry.detail = Some(serde_json::json!({
            "role": "parent", "name": name, "path": dst,
            "fork_point_seq": fork_point_seq, "method": method,
        }));
        self.journal.append(&entry)?;

        Ok(ForkInfo {
            name,
            path: dst,
            fork_point_seq,
            method,
            duration_ms: t0.elapsed().as_millis() as u64,
        })
    }

    pub fn fork_registry(&self) -> Result<ForkRegistry> {
        if !self.layout.forks_json().exists() {
            return Ok(ForkRegistry {
                v: 1,
                forks: vec![],
            });
        }
        read_json(&self.layout.forks_json())
    }

    /// N-way comparison of forks against their fork points. Non-committal:
    /// measures each fork's current tree without creating checkpoints.
    pub fn fork_compare(&self) -> Result<Vec<ForkCompareRow>> {
        let registry = self.fork_registry()?;
        let mut rows = Vec::new();
        for rec in &registry.forks {
            if rec.status != ForkStatus::Active {
                continue;
            }
            if !rec.path.exists() {
                rows.push(ForkCompareRow {
                    name: rec.name.clone(),
                    status: rec.status,
                    fork_point_seq: rec.fork_point_seq,
                    files_changed: 0,
                    insertions: 0,
                    deletions: 0,
                    last_activity: None,
                    path: rec.path.clone(),
                    missing: true,
                });
                continue;
            }
            let fork_ws = Workspace::open_root(&rec.path)?;
            // Staging mutates the fork's index — take ITS lock, not ours.
            let _fork_lock = StoreLock::acquire_with_retry(&fork_ws.layout)?;
            let base = fork_ws
                .checkpoint_refs()?
                .get(&rec.fork_point_seq)
                .cloned()
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::StoreCorrupt,
                        format!("fork '{}' is missing its fork-point checkpoint", rec.name),
                    )
                    .with_hint("run `asp doctor` inside the fork")
                })?;
            let (tree, _) = fork_ws.stage_tree()?;
            let (files, ins, del) = fork_ws.numstat_totals(&format!("{base}^{{tree}}"), &tree)?;
            let last = fork_ws.journal.read()?.entries.last().map(|e| e.ts.clone());
            rows.push(ForkCompareRow {
                name: rec.name.clone(),
                status: rec.status,
                fork_point_seq: rec.fork_point_seq,
                files_changed: files,
                insertions: ins,
                deletions: del,
                last_activity: last,
                path: rec.path.clone(),
                missing: false,
            });
        }
        Ok(rows)
    }

    fn numstat_totals(&self, from: &str, to: &str) -> Result<(u64, u64, u64)> {
        let raw = self
            .shadow
            .run_raw(&["diff-tree", "-r", "-z", "--numstat", from, to])?;
        let text = String::from_utf8_lossy(&raw);
        let (mut files, mut ins, mut del) = (0u64, 0u64, 0u64);
        for rec in text.split('\0').filter(|s| !s.is_empty()) {
            let mut it = rec.split('\t');
            let (a, b) = (it.next().unwrap_or("-"), it.next().unwrap_or("-"));
            files += 1;
            ins += a.parse::<u64>().unwrap_or(0);
            del += b.parse::<u64>().unwrap_or(0);
        }
        Ok((files, ins, del))
    }

    // ----------------------------------------------------------------- diff

    /// Diff two checkpoints, or a checkpoint against the working tree.
    pub fn diff(&self, from_spec: &str, to_spec: Option<&str>) -> Result<DiffReport> {
        let (from_label, from_commit) = {
            let (seq, c) = self.resolve_checkpoint(from_spec)?;
            (format!("#{seq}"), c)
        };
        let (to_label, to_tree) = match to_spec {
            Some(spec) => {
                let (seq, c) = self.resolve_checkpoint(spec)?;
                (format!("#{seq}"), c)
            }
            None => {
                let _lock = StoreLock::acquire(&self.layout)?;
                let (tree, _) = self.stage_tree()?;
                ("working tree".to_string(), tree)
            }
        };

        // name-status + numstat joined by path.
        let ns_raw = self.shadow.run_raw(&[
            "diff-tree",
            "-r",
            "-z",
            "--name-status",
            &from_commit,
            &to_tree,
        ])?;
        let mut status_by_path: BTreeMap<String, String> = BTreeMap::new();
        let mut parts = ns_raw.split(|&b| b == 0).filter(|s| !s.is_empty());
        while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
            status_by_path.insert(
                String::from_utf8_lossy(path).to_string(),
                String::from_utf8_lossy(status).to_string(),
            );
        }
        let num_raw =
            self.shadow
                .run_raw(&["diff-tree", "-r", "-z", "--numstat", &from_commit, &to_tree])?;
        let num_text = String::from_utf8_lossy(&num_raw);
        let mut rows = Vec::new();
        for rec in num_text.split('\0').filter(|s| !s.is_empty()) {
            let mut it = rec.split('\t');
            let ins = it.next().unwrap_or("-").parse::<u64>().ok();
            let del = it.next().unwrap_or("-").parse::<u64>().ok();
            let path = it.next().unwrap_or("").to_string();
            let status = status_by_path
                .get(&path)
                .cloned()
                .unwrap_or_else(|| "M".into());
            rows.push(DiffRow {
                path,
                status,
                insertions: ins,
                deletions: del,
            });
        }
        Ok(DiffReport {
            from: from_label,
            to: to_label,
            rows,
        })
    }

    // -------------------------------------------------------------- promote

    /// Land a fork's work as an ordinary branch in the user's git repo.
    /// Never touches HEAD, never force-pushes, never runs user hooks.
    pub fn promote(&self, fork_name: &str, branch: Option<String>) -> Result<PromoteReport> {
        let _lock = StoreLock::acquire(&self.layout)?;
        let mut registry = self.fork_registry()?;
        let rec = registry
            .forks
            .iter_mut()
            .find(|f| f.name == fork_name && f.status == ForkStatus::Active)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ForkNotFound,
                    format!("no active fork named '{fork_name}'"),
                )
                .with_hint("run `asp forks` to list forks")
            })?;
        if !rec.path.exists() {
            return Err(Error::new(
                ErrorCode::ForkNotFound,
                format!("fork directory is missing: {}", rec.path.display()),
            )
            .with_hint("run `asp doctor` to clean up the registry"));
        }
        let user_git = self.layout.root.join(".git");
        if !user_git.exists() {
            return Err(Error::new(
                ErrorCode::NoUserGitRepo,
                "promote lands a fork as a git branch, but this directory is not a git repository",
            )
            .with_hint("run `git init && git add -A && git commit -m init` first, or copy files from the fork manually"));
        }

        let branch = branch.unwrap_or_else(|| format!("asp/{fork_name}"));
        if user_git_ref_exists(&self.layout.root, &branch)? {
            return Err(Error::new(
                ErrorCode::BranchExists,
                format!("branch '{branch}' already exists in the user repo"),
            )
            .with_hint("pass a different name: `asp promote <fork> --branch <name>`"));
        }

        // Build a commit in the FORK's user repo via plumbing (no checkout,
        // no HEAD move, no hooks), then fetch it into the original repo.
        let commit = build_user_commit(&rec.path, fork_name)?;
        let tmp_ref = format!("refs/asp-promote/{fork_name}");
        run_user_git(&rec.path, &["update-ref", &tmp_ref, &commit])?;
        let fetch_result = run_user_git(
            &self.layout.root,
            &[
                "fetch",
                "--quiet",
                rec.path.to_str().unwrap_or_default(),
                &format!("{tmp_ref}:refs/heads/{branch}"),
            ],
        );
        let _ = run_user_git(&rec.path, &["update-ref", "-d", &tmp_ref]);
        fetch_result?;

        rec.status = ForkStatus::Promoted;
        atomic_write_json(&self.layout.forks_json(), &registry)?;
        let mut entry = Entry::new(Op::Promote);
        entry.detail = Some(serde_json::json!({
            "fork": fork_name, "branch": branch, "commit": commit,
        }));
        self.journal.append(&entry)?;

        Ok(PromoteReport {
            fork: fork_name.to_string(),
            branch,
            commit,
        })
    }

    // -------------------------------------------------------------- discard

    pub fn discard(&self, fork_name: &str, force: bool) -> Result<()> {
        let _lock = StoreLock::acquire(&self.layout)?;
        let mut registry = self.fork_registry()?;
        let rec = registry
            .forks
            .iter_mut()
            .find(|f| f.name == fork_name && f.status != ForkStatus::Discarded)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ForkNotFound,
                    format!("no fork named '{fork_name}' to discard"),
                )
                .with_hint("run `asp forks` to list forks")
            })?;

        // Promoted forks need no guard — their work already landed as a branch.
        if rec.status == ForkStatus::Active && rec.path.exists() && !force {
            let fork_ws = Workspace::open_root(&rec.path)?;
            let _fork_lock = StoreLock::acquire_with_retry(&fork_ws.layout)?;
            let base = fork_ws.checkpoint_refs()?.get(&rec.fork_point_seq).cloned();
            if let Some(base) = base {
                let (tree, _) = fork_ws.stage_tree()?;
                if fork_ws.shadow.tree_of(&base)? != tree {
                    return Err(Error::new(
                        ErrorCode::ForkHasUnpromotedWork,
                        format!("fork '{fork_name}' has changes that were never promoted"),
                    )
                    .with_hint(format!(
                        "promote it first (`asp promote {fork_name}`) or pass --force to delete the work"
                    )));
                }
            }
        }
        if rec.path.exists() {
            std::fs::remove_dir_all(&rec.path)?;
        }
        rec.status = ForkStatus::Discarded;
        atomic_write_json(&self.layout.forks_json(), &registry)?;
        let mut entry = Entry::new(Op::Discard);
        entry.detail = Some(serde_json::json!({ "fork": fork_name, "forced": force }));
        self.journal.append(&entry)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
    pub fixed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Workspace {
    /// Diagnose (and with `fix`, repair) workspace-store health issues.
    pub fn doctor(&self, fix: bool, deep: bool) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        let mut add = |severity: Severity, message: String, fixed: bool| {
            findings.push(Finding {
                severity,
                message,
                fixed,
            })
        };

        // Repairs mutate the store — hold the lock for a --fix run.
        let _lock = if fix {
            Some(StoreLock::acquire_with_retry(&self.layout)?)
        } else {
            None
        };

        // 1. Journal integrity.
        let report = self.journal.read()?;
        if report.torn_tail {
            let fixed = if fix {
                self.journal.heal()?;
                true
            } else {
                false
            };
            add(
                Severity::Warning,
                "journal has a torn tail from a crash mid-append".to_string(),
                fixed,
            );
        }
        for line in &report.corrupt_lines {
            add(
                Severity::Error,
                format!("journal line {line} is corrupt (CRC mismatch); provenance for that entry is lost"),
                false,
            );
        }

        // 2. Checkpoint refs resolvable + head consistency.
        let refs = self.checkpoint_refs()?;
        for (seq, commit) in &refs {
            if self.shadow.rev_parse(commit)?.is_none() {
                add(
                    Severity::Error,
                    format!("checkpoint #{seq} points at missing commit {commit}"),
                    false,
                );
            }
        }
        if let Some((max_seq, max_commit)) = refs.iter().next_back() {
            let head = self.shadow.rev_parse(HEAD_REF)?;
            if head.as_deref() != Some(max_commit.as_str()) {
                let fixed = if fix {
                    self.shadow.update_ref(HEAD_REF, max_commit)?;
                    true
                } else {
                    false
                };
                add(
                    Severity::Warning,
                    format!("head ref does not match latest checkpoint #{max_seq}"),
                    fixed,
                );
            }
        }

        // 3. Journal entries referencing refs that don't exist (crash window).
        for e in report.entries.iter().filter(|e| e.op == Op::Checkpoint) {
            if let Some(seq) = e.seq {
                if !refs.contains_key(&seq) {
                    add(
                        Severity::Warning,
                        format!("journal records checkpoint #{seq} but its ref is missing"),
                        false,
                    );
                }
            }
        }

        // 4. Fork registry vs reality. Pending entries are deterministic
        //    torn-clone markers (intent journaling in fork()), so cleanup
        //    never has to guess about directories asp didn't create.
        let mut registry = self.fork_registry()?;
        let mut registry_changed = false;
        let mut drop_pending: Vec<String> = Vec::new();
        for rec in registry.forks.iter_mut() {
            match rec.status {
                ForkStatus::Active if !rec.path.exists() => {
                    let fixed = fix;
                    if fix {
                        rec.status = ForkStatus::Discarded;
                        registry_changed = true;
                    }
                    add(
                        Severity::Warning,
                        format!(
                            "fork '{}' is registered active but its directory is gone ({})",
                            rec.name,
                            rec.path.display()
                        ),
                        fixed,
                    );
                }
                ForkStatus::Pending => {
                    let exists = rec.path.exists();
                    let fixed = if fix {
                        if exists {
                            std::fs::remove_dir_all(&rec.path)?;
                        }
                        drop_pending.push(rec.name.clone());
                        registry_changed = true;
                        true
                    } else {
                        false
                    };
                    add(
                        Severity::Warning,
                        format!(
                            "fork '{}' is a torn clone (creation crashed mid-flight{})",
                            rec.name,
                            if exists {
                                "; directory removed by --fix"
                            } else {
                                ""
                            }
                        ),
                        fixed,
                    );
                }
                _ => {}
            }
        }
        if !drop_pending.is_empty() {
            registry
                .forks
                .retain(|f| !(f.status == ForkStatus::Pending && drop_pending.contains(&f.name)));
        }
        if registry_changed {
            atomic_write_json(&self.layout.forks_json(), &registry)?;
        }

        // 5. Unregistered fork-looking sibling dirs: REPORT ONLY. asp never
        //    deletes a directory it cannot prove it created (a user's manual
        //    `cp -r proj proj@backup` is indistinguishable from a torn clone
        //    by inspection — the Pending registry above is the proof).
        if let (Some(parent_dir), Some(dir_name)) = (
            self.layout.root.parent(),
            self.layout
                .root
                .file_name()
                .map(|s| s.to_string_lossy().to_string()),
        ) {
            let prefix = format!("{dir_name}@");
            if let Ok(entries) = std::fs::read_dir(parent_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with(&prefix) || !entry.path().is_dir() {
                        continue;
                    }
                    if !Layout::new(entry.path()).asp.exists() {
                        continue;
                    }
                    let registered = registry
                        .forks
                        .iter()
                        .any(|f| f.path == entry.path() && f.status != ForkStatus::Discarded);
                    if !registered {
                        add(
                            Severity::Info,
                            format!(
                                "directory {} looks like a fork of this workspace but is not in the registry; remove it manually if unwanted",
                                entry.path().display()
                            ),
                            false,
                        );
                    }
                }
            }
        }

        // 6. Big-file CAS integrity.
        let bf = blobs::load_bigfiles(&blobs::bigfiles_path(&self.layout.asp))?;
        for (path, entry) in &bf.files {
            let cas = self.layout.blobs().join(&entry.blake3);
            if !cas.exists() {
                let abs = self.layout.root.join(path);
                if abs.is_file() {
                    let fixed = if fix {
                        blobs::store_in_cas(&self.layout.blobs(), &abs)?;
                        true
                    } else {
                        false
                    };
                    add(
                        Severity::Warning,
                        format!("CAS blob for {path} missing (re-creatable from the working file)"),
                        fixed,
                    );
                } else {
                    add(
                        Severity::Error,
                        format!(
                            "CAS blob for {path} is missing and the file is gone — checkpointed versions of it cannot be restored"
                        ),
                        false,
                    );
                }
            } else if deep {
                let actual = blobs::hash_file(&cas)?;
                if actual != entry.blake3 {
                    add(
                        Severity::Error,
                        format!(
                            "CAS blob for {path} is corrupt (expected {}, got {actual})",
                            entry.blake3
                        ),
                        false,
                    );
                }
            }
        }

        Ok(findings)
    }
}

fn sanitize_name(label: &str) -> String {
    let s: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "fork".to_string()
    } else {
        s
    }
}

/// Run git in the USER repo (their config applies; we only add safety flags).
/// Repo-location env vars are scrubbed: a user shell exporting GIT_DIR must
/// not redirect promote's writes into an unrelated repository.
fn run_user_git(repo_dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .arg("-C")
        .arg(repo_dir)
        .args(args)
        .output()
        .map_err(|e| {
            Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
        })?;
    if !output.status.success() {
        return Err(Error::new(
            ErrorCode::GitFailed,
            format!(
                "git {} failed in {}: {}",
                args.first().unwrap_or(&""),
                repo_dir.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn user_git_ref_exists(repo_dir: &Path, branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .arg("-C")
        .arg(repo_dir)
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .map_err(|e| {
            Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
        })?;
    Ok(output.status.success())
}

/// Stage the fork's whole tree (respecting the user's .gitignore) through a
/// temp index and return a commit, parented on the fork's HEAD if it exists.
fn build_user_commit(fork_dir: &Path, fork_name: &str) -> Result<String> {
    // The temp index path must NOT exist yet — git rejects an empty file.
    let tmp_dir = tempfile::tempdir()?;
    let tmp_index = tmp_dir.path().join("index");
    let run = |args: &[&str], with_identity_fallback: bool| -> Result<String> {
        let mut cmd = Command::new("git");
        cmd.env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .arg("-C")
            .arg(fork_dir)
            .env("GIT_INDEX_FILE", &tmp_index)
            .args(args);
        if with_identity_fallback {
            cmd.env("GIT_AUTHOR_NAME", "asp")
                .env("GIT_AUTHOR_EMAIL", "asp@agentspaces.local")
                .env("GIT_COMMITTER_NAME", "asp")
                .env("GIT_COMMITTER_EMAIL", "asp@agentspaces.local");
        }
        let output = cmd.output().map_err(|e| {
            Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
        })?;
        if !output.status.success() {
            return Err(Error::new(
                ErrorCode::GitFailed,
                format!(
                    "git {} failed: {}",
                    args.first().unwrap_or(&""),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    };

    // Stage the fork's tree EXCLUDING the .asp store — promote must land
    // the work, not the sidecar (shadow objects, journal, CAS blobs).
    run(&["add", "-A", "--", ".", ":(exclude).asp"], false)?;
    let tree = run(&["write-tree"], false)?;
    let head = Command::new("git")
        .arg("-C")
        .arg(fork_dir)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let message = format!("asp: promote fork '{fork_name}'");
    let mut args = vec!["commit-tree", tree.as_str(), "-m", message.as_str()];
    if let Some(ref h) = head {
        args.push("-p");
        args.push(h);
    }
    // Try with the user's identity first; fall back to asp's if unset.
    match run(&args, false) {
        Ok(c) => Ok(c),
        Err(_) => run(&args, true),
    }
}
