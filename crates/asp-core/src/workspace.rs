//! The Workspace: asp's primary API. Open/init a directory, then checkpoint,
//! fork, diff, restore, promote, discard against it. CLI and MCP server are
//! thin shells over this type.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::Serialize;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::blobs::{self, BigFiles, Manifest, ManifestEntry};
use crate::config::Config;
use crate::error::{Error, ErrorCode, Result};
use crate::file_state::{self, FileStateEntry, FileStateIndex, FILE_STATE_VERSION};
use crate::fork::{clone_tree, CloneMethod};
use crate::gitx::Shadow;
use crate::journal::{Entry, Journal, Op, Source};
use crate::policy::Policy;
use crate::store::{
    atomic_write, atomic_write_json, find_root, read_json, ForkRecord, ForkRegistry, ForkStatus,
    Layout, ParentRef, StoreLock, WorkspaceMeta, FORMAT_VERSION,
};
use crate::sync::{LocalRemote, PutOutcome, SyncRemote};
use walkdir::WalkDir;

pub const CHECKPOINT_REF_PREFIX: &str = "refs/asp/checkpoints/";
pub const HEAD_REF: &str = "refs/asp/head";
pub const META_REF_PREFIX: &str = "refs/asp/meta/";

#[derive(Debug)]
pub struct Workspace {
    layout: Layout,
    pub meta: WorkspaceMeta,
    pub config: Config,
    pub policy: Policy,
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
pub struct StatsReport {
    pub root: PathBuf,
    pub workspace_id: String,
    pub checkpoints: u64,
    pub journal_entries: u64,
    pub forks_total: u64,
    pub forks_pending: u64,
    pub forks_active: u64,
    pub forks_promoted: u64,
    pub forks_discarded: u64,
    pub blobs: u64,
    pub blob_bytes: u64,
    pub store_bytes: u64,
    pub last_operation: Option<StatsOperation>,
    pub recent_operations: Vec<StatsOperation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsOperation {
    pub op: Op,
    pub ts: String,
    pub seq: Option<u64>,
    pub duration_ms: Option<u64>,
    pub files_changed: Option<u64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetentionPlan {
    pub dry_run: bool,
    pub policy: RetentionPlanPolicy,
    pub total_checkpoints: u64,
    pub retain_count: u64,
    pub delete_count: u64,
    pub checkpoints: Vec<RetentionPlanEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetentionPlanPolicy {
    pub keep_last: Option<u64>,
    pub max_age_days: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetentionPlanEntry {
    pub seq: u64,
    pub commit: String,
    pub ts: Option<String>,
    pub message: Option<String>,
    pub age_hours: Option<i64>,
    pub action: RetentionAction,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionAction {
    Retain,
    Delete,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticBundle {
    pub generated_at: String,
    pub asp_version: String,
    pub format_version: u32,
    pub redaction: DiagnosticRedaction,
    pub workspace: DiagnosticWorkspace,
    pub status: DiagnosticStatus,
    pub stats: DiagnosticStats,
    pub config: DiagnosticConfig,
    pub forks: Vec<DiagnosticFork>,
    pub doctor_findings: Vec<DiagnosticFinding>,
    pub recent_operations: Vec<DiagnosticOperation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticRedaction {
    pub paths_redacted: bool,
    pub secrets_redacted: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticWorkspace {
    pub root: String,
    pub workspace_id: String,
    pub is_fork: bool,
    pub parent_workspace_id: Option<String>,
    pub parent_fork_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticStatus {
    pub dirty_files: u64,
    pub untracked_files: u64,
    pub deleted_files: u64,
    pub active_forks: u64,
    pub last_checkpoint: Option<LastCheckpoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticStats {
    pub checkpoints: u64,
    pub journal_entries: u64,
    pub forks_total: u64,
    pub forks_pending: u64,
    pub forks_active: u64,
    pub forks_promoted: u64,
    pub forks_discarded: u64,
    pub blobs: u64,
    pub blob_bytes: u64,
    pub store_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticConfig {
    pub blob_threshold_mb: u64,
    pub excludes_count: usize,
    pub extra_excludes_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticFork {
    pub name: String,
    pub status: ForkStatus,
    pub fork_point_seq: u64,
    pub path: String,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticFinding {
    pub severity: Severity,
    pub message: String,
    pub cause: String,
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair_plan: Option<RepairPlan>,
    pub fixed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticOperation {
    pub op: Op,
    pub ts: String,
    pub seq: Option<u64>,
    pub duration_ms: Option<u64>,
    pub files_changed: Option<u64>,
    pub message: Option<String>,
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
    pub summary: DiffSummary,
    pub rows: Vec<DiffRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffTextReport {
    pub from: String,
    pub to: String,
    pub mode: String,
    pub summary: DiffSummary,
    pub text: String,
}

#[derive(Debug, Clone, Copy)]
pub enum DiffTextMode {
    Patch,
    Stat,
}

impl DiffTextMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Stat => "stat",
        }
    }

    fn git_arg(self) -> &'static str {
        match self {
            Self::Patch => "--patch",
            Self::Stat => "--stat",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffSummary {
    pub files: u64,
    pub insertions: u64,
    pub deletions: u64,
    pub by_path: Vec<DiffSummaryBucket>,
    pub by_language: Vec<DiffSummaryBucket>,
    pub by_change_type: Vec<DiffSummaryBucket>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffSummaryBucket {
    pub name: String,
    pub files: u64,
    pub insertions: u64,
    pub deletions: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForkCompareRow {
    pub name: String,
    pub status: ForkStatus,
    pub fork_point_seq: u64,
    pub files_changed: u64,
    pub insertions: u64,
    pub deletions: u64,
    pub review: ForkReviewSignals,
    pub last_activity: Option<String>,
    pub path: PathBuf,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForkReviewSignals {
    pub tests_passed: Option<bool>,
    pub files_touched: u64,
    pub line_churn: u64,
    pub risk_score: u64,
    pub risk_markers: Vec<ForkRiskMarker>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForkRiskMarker {
    pub kind: String,
    pub severity: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromoteReport {
    pub fork: String,
    pub fork_path: PathBuf,
    pub fork_retained: bool,
    pub branch: String,
    pub commit: String,
    pub cleanup_command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push: Option<PromotePushReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<PromotePrDraftReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromotePushReport {
    pub pushed: bool,
    pub remote: String,
    pub branch: String,
    pub refspec: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromotePrDraftReport {
    pub attempted: bool,
    pub created: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub command: String,
    pub fallback_command: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncPushReport {
    pub remote: PathBuf,
    pub workspace_id: String,
    pub checkpoints: u64,
    pub git_objects_uploaded: u64,
    pub git_objects_present: u64,
    pub cas_blobs_uploaded: u64,
    pub cas_blobs_present: u64,
    pub refs_created: u64,
    pub refs_present: u64,
    pub refs_replaced: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncFetchReport {
    pub remote: PathBuf,
    pub workspace_id: String,
    pub refs_imported: u64,
    pub refs_present: u64,
    pub refs_conflicted: u64,
    pub git_objects_downloaded: u64,
    pub git_objects_present: u64,
    pub cas_blobs_downloaded: u64,
    pub cas_blobs_present: u64,
    pub head_updated: bool,
    pub head_seq: Option<u64>,
    pub conflicts: Vec<SyncRefConflict>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatusReport {
    pub remote: PathBuf,
    pub workspace_id: String,
    pub remote_initialized: bool,
    pub local_checkpoint_refs: u64,
    pub remote_checkpoint_refs: u64,
    pub checkpoint_refs_matching: u64,
    pub checkpoint_refs_local_only: u64,
    pub checkpoint_refs_remote_only: u64,
    pub checkpoint_refs_conflicted: u64,
    pub local_meta_refs: u64,
    pub remote_meta_refs: u64,
    pub meta_refs_matching: u64,
    pub meta_refs_local_only: u64,
    pub meta_refs_remote_only: u64,
    pub meta_refs_conflicted: u64,
    pub local_head_seq: Option<u64>,
    pub remote_head_seq: Option<u64>,
    pub head_relation: String,
    pub conflicts: Vec<SyncRefConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncRefConflict {
    pub kind: String,
    pub seq: u64,
    pub local: Option<String>,
    pub remote: Option<String>,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncRef {
    seq: u64,
    target: String,
}

#[derive(Debug, Default)]
struct SyncRefSummary {
    matching: u64,
    local_only: u64,
    remote_only: u64,
    conflicted: u64,
}

struct ScanResult {
    /// (path, needs_add): every user-visible change, with whether the
    /// worktree differs from the index for it (index-only changes — e.g.
    /// staged deletions after a restore's read-tree — need no `git add`).
    changed: Vec<(String, bool)>,
    /// Index paths whose on-disk spelling changed only by case. Remove the
    /// stale spelling before `git add`, or case-insensitive filesystems keep
    /// the old tree entry while updating only the blob content.
    case_index_removals: Vec<String>,
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
        crate::ensure_supported_platform()?;
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
        let policy = Policy::load(&layout.policy_toml())?;
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
            policy,
            shadow,
            journal,
        })
    }

    /// Initialize a new workspace in `root` (adopts existing content as-is).
    pub fn init(root: &Path, label: Option<String>) -> Result<Self> {
        crate::ensure_supported_platform()?;
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
        let policy = Policy::default();

        let shadow = Shadow::new(
            layout.shadow_git(),
            layout.root.clone(),
            layout.shadow_index(),
        );
        shadow.init()?;
        shadow.write_excludes(&config.shadow_excludes())?;

        atomic_write_json(&layout.workspace_json(), &meta)?;
        atomic_write(&layout.config_toml(), Config::template().as_bytes())?;
        atomic_write(&layout.policy_toml(), Policy::template().as_bytes())?;
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
            policy,
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

    // --------------------------------------------------------------- stats

    pub fn stats(&self) -> Result<StatsReport> {
        let refs = self.checkpoint_refs()?;
        let journal_entries = self.journal.read()?.entries;
        let registry = self.fork_registry()?;
        let (mut forks_pending, mut forks_active, mut forks_promoted, mut forks_discarded) =
            (0u64, 0u64, 0u64, 0u64);
        for rec in &registry.forks {
            match rec.status {
                ForkStatus::Pending => forks_pending += 1,
                ForkStatus::Active => forks_active += 1,
                ForkStatus::Promoted => forks_promoted += 1,
                ForkStatus::Discarded => forks_discarded += 1,
            }
        }
        let (blobs, blob_bytes) = blob_stats(&self.layout.blobs())?;
        let store_bytes = dir_file_bytes(&self.layout.asp)?;
        let last_operation = journal_entries.last().map(stats_operation);
        let recent_operations = journal_entries
            .iter()
            .rev()
            .take(10)
            .map(stats_operation)
            .collect();

        Ok(StatsReport {
            root: self.layout.root.clone(),
            workspace_id: self.meta.id.clone(),
            checkpoints: refs.len() as u64,
            journal_entries: journal_entries.len() as u64,
            forks_total: registry.forks.len() as u64,
            forks_pending,
            forks_active,
            forks_promoted,
            forks_discarded,
            blobs,
            blob_bytes,
            store_bytes,
            last_operation,
            recent_operations,
        })
    }

    pub fn retention_plan(&self) -> Result<RetentionPlan> {
        let refs = self.checkpoint_refs()?;
        let journal_entries = self.journal.read()?.entries;
        let registry = self.fork_registry()?;
        let policy = RetentionPlanPolicy {
            keep_last: self.policy.retention.keep_last,
            max_age_days: self.policy.retention.max_age_days,
        };
        let mut checkpoint_entries = BTreeMap::new();
        for entry in &journal_entries {
            if entry.op == Op::Checkpoint {
                if let Some(seq) = entry.seq {
                    checkpoint_entries.insert(seq, entry);
                }
            }
        }

        let latest_seq = refs.keys().next_back().copied();
        let keep_last: BTreeSet<u64> = refs
            .keys()
            .rev()
            .take(policy.keep_last.unwrap_or(0) as usize)
            .copied()
            .collect();
        let fork_points: BTreeSet<u64> = registry
            .forks
            .iter()
            .filter(|fork| matches!(fork.status, ForkStatus::Pending | ForkStatus::Active))
            .map(|fork| fork.fork_point_seq)
            .collect();
        let has_retention_policy = policy.keep_last.is_some() || policy.max_age_days.is_some();
        let now = OffsetDateTime::now_utc();
        let mut entries = Vec::new();
        let mut retain_count = 0u64;
        let mut delete_count = 0u64;

        for (seq, commit) in refs.iter().rev() {
            let journal = checkpoint_entries.get(seq).copied();
            let ts = journal.map(|entry| entry.ts.clone());
            let age_hours = ts
                .as_ref()
                .map(|ts| {
                    OffsetDateTime::parse(ts, &Rfc3339)
                        .map(|parsed| (now - parsed).whole_hours())
                        .map_err(|e| {
                            Error::new(
                                ErrorCode::StoreCorrupt,
                                format!("checkpoint #{seq} has an unreadable timestamp: {e}"),
                            )
                            .with_hint(
                                "run `asp doctor`; if the journal cannot be repaired, create a fresh checkpoint",
                            )
                        })
                })
                .transpose()?;
            let message = journal.and_then(|entry| entry.message.clone());

            let (action, reason) = if !has_retention_policy {
                (RetentionAction::Retain, "no_retention_policy")
            } else if Some(*seq) == latest_seq {
                (RetentionAction::Retain, "latest_checkpoint")
            } else if fork_points.contains(seq) {
                (RetentionAction::Retain, "fork_point")
            } else if keep_last.contains(seq) {
                (RetentionAction::Retain, "keep_last")
            } else if let Some(max_age_days) = policy.max_age_days {
                match age_hours {
                    Some(age_hours) if age_hours > (max_age_days as i64 * 24) => {
                        (RetentionAction::Delete, "older_than_max_age_days")
                    }
                    Some(_) => (RetentionAction::Retain, "within_max_age_days"),
                    None => (RetentionAction::Retain, "missing_journal_entry"),
                }
            } else {
                (RetentionAction::Delete, "outside_keep_last")
            };

            match action {
                RetentionAction::Retain => retain_count += 1,
                RetentionAction::Delete => delete_count += 1,
            }
            entries.push(RetentionPlanEntry {
                seq: *seq,
                commit: commit.clone(),
                ts,
                message,
                age_hours,
                action,
                reason: reason.to_string(),
            });
        }

        Ok(RetentionPlan {
            dry_run: true,
            policy,
            total_checkpoints: refs.len() as u64,
            retain_count,
            delete_count,
            checkpoints: entries,
        })
    }

    // ---------------------------------------------------------- diagnostics

    pub fn diagnostics(&self, include_paths: bool) -> Result<DiagnosticBundle> {
        let redactor = DiagnosticsRedactor::new(&self.layout.root, include_paths);
        let status = self.status()?;
        let stats = self.stats()?;
        let registry = self.fork_registry()?;
        let doctor_findings = self
            .doctor(false, false)?
            .into_iter()
            .map(|finding| DiagnosticFinding {
                severity: finding.severity,
                message: redactor.text(&finding.message),
                cause: redactor.text(&finding.cause),
                next_action: redactor.text(&finding.next_action),
                repair_plan: finding.repair_plan.as_ref().map(|plan| RepairPlan {
                    operation: plan.operation.clone(),
                    description: redactor.text(&plan.description),
                    command: redactor.text(&plan.command),
                    destructive: plan.destructive,
                }),
                fixed: finding.fixed,
            })
            .collect();

        Ok(DiagnosticBundle {
            generated_at: crate::now_rfc3339(),
            asp_version: crate::version().to_string(),
            format_version: FORMAT_VERSION,
            redaction: DiagnosticRedaction {
                paths_redacted: !include_paths,
                secrets_redacted: true,
                notes: vec![
                    "file contents and environment variables are never collected".to_string(),
                    "checkpoint messages and finding text are scanned for common secret tokens"
                        .to_string(),
                ],
            },
            workspace: DiagnosticWorkspace {
                root: redactor.path(&self.layout.root),
                workspace_id: self.meta.id.clone(),
                is_fork: self.meta.parent.is_some(),
                parent_workspace_id: self.meta.parent.as_ref().map(|p| p.workspace_id.clone()),
                parent_fork_name: self
                    .meta
                    .parent
                    .as_ref()
                    .map(|p| redactor.text(&p.fork_name)),
            },
            status: DiagnosticStatus {
                dirty_files: status.dirty_files,
                untracked_files: status.untracked_files,
                deleted_files: status.deleted_files,
                active_forks: status.active_forks,
                last_checkpoint: status.last_checkpoint.map(|mut checkpoint| {
                    checkpoint.message = checkpoint
                        .message
                        .as_ref()
                        .map(|message| redactor.text(message));
                    checkpoint
                }),
            },
            stats: DiagnosticStats {
                checkpoints: stats.checkpoints,
                journal_entries: stats.journal_entries,
                forks_total: stats.forks_total,
                forks_pending: stats.forks_pending,
                forks_active: stats.forks_active,
                forks_promoted: stats.forks_promoted,
                forks_discarded: stats.forks_discarded,
                blobs: stats.blobs,
                blob_bytes: stats.blob_bytes,
                store_bytes: stats.store_bytes,
            },
            config: DiagnosticConfig {
                blob_threshold_mb: self.config.capture.blob_threshold_mb,
                excludes_count: self.config.capture.excludes.len(),
                extra_excludes_count: self.config.capture.extra_excludes.len(),
            },
            forks: registry
                .forks
                .iter()
                .map(|fork| DiagnosticFork {
                    name: redactor.text(&fork.name),
                    status: fork.status,
                    fork_point_seq: fork.fork_point_seq,
                    path: redactor.path(&fork.path),
                    missing: !fork.path.exists(),
                })
                .collect(),
            recent_operations: stats
                .recent_operations
                .iter()
                .map(|op| DiagnosticOperation {
                    op: op.op.clone(),
                    ts: op.ts.clone(),
                    seq: op.seq,
                    duration_ms: op.duration_ms,
                    files_changed: op.files_changed,
                    message: op.message.as_ref().map(|message| redactor.text(message)),
                })
                .collect(),
            doctor_findings,
        })
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
        let (tree, bigfiles) = self.stage_tree_for_checkpoint()?;
        if let Some(ref p) = parent {
            if self.shadow.tree_of(p)? == tree {
                let _ = self.refresh_file_state_index_if_needed(p);
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

        let changed_paths = self.checkpoint_changed_paths(parent.as_deref(), &commit)?;
        let files_changed = changed_paths.len() as u64;

        let mut entry = Entry::new(Op::Checkpoint);
        entry.seq = Some(seq);
        entry.commit = Some(commit.clone());
        entry.source = opts.source.clone();
        entry.session_id = opts.session_id.clone();
        entry.tool = opts.tool.clone();
        entry.message = Some(message.clone());
        entry.files_changed = Some(files_changed);
        entry.duration_ms = Some(t0.elapsed().as_millis() as u64);
        entry.detail = Some(serde_json::json!({ "paths": changed_paths }));
        self.journal.append(&entry)?;
        self.shadow.update_ref(HEAD_REF, &commit)?;
        let _ = self.write_file_state_index(&commit);

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

    fn refresh_file_state_index_if_needed(&self, head: &str) -> Result<()> {
        let path = self.layout.file_state_index();
        match file_state::load(&path) {
            Ok(index) if index.v == FILE_STATE_VERSION && index.head == head => Ok(()),
            _ => self.write_file_state_index(head),
        }
    }

    fn write_file_state_index(&self, head: &str) -> Result<()> {
        let raw = self
            .shadow
            .run_raw(&["ls-tree", "-r", "-z", "--name-only", head])?;
        let mut index = FileStateIndex::new(head);
        for path in paths_from_nul(&raw) {
            let abs = crate::store::safe_rel_path(&self.layout.root, &path)?;
            match std::fs::symlink_metadata(&abs) {
                Ok(md) => {
                    index
                        .entries
                        .insert(path, FileStateEntry::from_metadata(&md));
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }
        file_state::save(&self.layout.file_state_index(), &index)
    }

    /// Pointer-aware staging: one status scan drives change detection, big-
    /// file maintenance, and a no-op fast path; only changed paths are staged
    /// (full scan above a threshold); pointer blobs are spliced in; pointer
    /// paths are removed from the index afterwards so the next capture skips
    /// them (untracked + excluded). Returns the tree oid + big-file set.
    fn stage_tree(&self) -> Result<(String, BigFiles)> {
        self.stage_tree_inner(false)
    }

    fn stage_tree_for_checkpoint(&self) -> Result<(String, BigFiles)> {
        self.stage_tree_inner(true)
    }

    fn stage_tree_inner(&self, enforce_checkpoint_deny: bool) -> Result<(String, BigFiles)> {
        let ScanResult {
            changed,
            case_index_removals,
            bigfiles,
            force_full_scan,
        } = self.scan_changes(enforce_checkpoint_deny)?;

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
        if !case_index_removals.is_empty() {
            let rm_input: String = case_index_removals
                .iter()
                .map(|path| format!("0 {}\t{path}\n", "0".repeat(40)))
                .collect();
            self.shadow
                .run_with_stdin(&["update-index", "--index-info"], &rm_input)?;
        }
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
    fn scan_changes(&self, enforce_checkpoint_deny: bool) -> Result<ScanResult> {
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
        let mut case_index_removals = Vec::new();
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
            let Ok(mut path) = std::str::from_utf8(&entry[3..]).map(str::to_string) else {
                // Non-UTF-8 filename: git itself handles the raw bytes via a
                // full-tree add; it just can't ride our pathspec.
                force_full_scan = true;
                if xy.contains(&b'R') || xy.contains(&b'C') {
                    iter.next();
                }
                continue;
            };
            if needs_add {
                if let Some(actual) = actual_worktree_case(&self.layout.root, &path) {
                    if actual != path {
                        case_index_removals.push(path);
                        path = actual;
                    }
                }
            }
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

        if enforce_checkpoint_deny {
            self.enforce_checkpoint_deny_paths(
                changed
                    .iter()
                    .filter(|(_, needs_add)| *needs_add)
                    .map(|(path, _)| path.clone()),
            )?;
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
                            if enforce_checkpoint_deny {
                                self.enforce_checkpoint_deny_paths(std::iter::once(path.clone()))?;
                            }
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
                        if enforce_checkpoint_deny {
                            self.enforce_checkpoint_deny_paths(std::iter::once(path.clone()))?;
                        }
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
            case_index_removals,
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

    fn validate_manifest_restore_paths(
        &self,
        manifest: &Manifest,
        paths: Option<&[String]>,
    ) -> Result<()> {
        for ptr in manifest
            .pointers
            .iter()
            .filter(|ptr| paths.map(|paths| paths.contains(&ptr.path)).unwrap_or(true))
        {
            crate::store::safe_worktree_write_path(&self.layout.root, &ptr.path)?;
        }
        Ok(())
    }

    fn checkpoint_changed_paths(&self, parent: Option<&str>, commit: &str) -> Result<Vec<String>> {
        match parent {
            Some(parent) => self.changed_paths_between(parent, commit),
            None => {
                let raw = self
                    .shadow
                    .run_raw(&["ls-tree", "-r", "-z", "--name-only", commit])?;
                Ok(paths_from_nul(&raw))
            }
        }
    }

    fn changed_paths_between(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let raw = self
            .shadow
            .run_raw(&["diff-tree", "-r", "-z", "--name-only", from, to])?;
        Ok(paths_from_nul(&raw))
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

    /// All checkpoint metadata refs as seq -> blob object.
    pub fn meta_refs(&self) -> Result<BTreeMap<u64, String>> {
        let out = self.shadow.run(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            META_REF_PREFIX.trim_end_matches('/'),
        ])?;
        let mut map = BTreeMap::new();
        for line in out.lines() {
            if let Some((name, oid)) = line.split_once(' ') {
                if let Some(seq) = name
                    .strip_prefix(META_REF_PREFIX)
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    map.insert(seq, oid.to_string());
                }
            }
        }
        Ok(map)
    }

    pub fn sync_push_local(&self, remote_root: impl AsRef<Path>) -> Result<SyncPushReport> {
        let _lock = StoreLock::acquire(&self.layout)?;
        self.journal.heal()?;
        let remote_root = remote_root.as_ref().to_path_buf();
        let remote = LocalRemote::open(&remote_root)?;
        let prefix = format!("asp-sync/v1/workspaces/{}", self.meta.id);
        let checkpoints = self.checkpoint_refs()?;
        let meta_refs = self.meta_refs()?;
        let mut report = SyncPushReport {
            remote: remote_root,
            workspace_id: self.meta.id.clone(),
            checkpoints: checkpoints.len() as u64,
            git_objects_uploaded: 0,
            git_objects_present: 0,
            cas_blobs_uploaded: 0,
            cas_blobs_present: 0,
            refs_created: 0,
            refs_present: 0,
            refs_replaced: 0,
        };

        let workspace_record = serde_json::to_vec_pretty(&serde_json::json!({
            "v": 1,
            "workspace_id": self.meta.id.clone(),
            "format_version": FORMAT_VERSION,
            "created_by": "asp",
            "created_at": self.meta.created_at.clone(),
        }))
        .map_err(|e| Error::new(ErrorCode::Io, format!("encode sync workspace record: {e}")))?;
        let (mut descriptor_uploaded, mut descriptor_present) = (0, 0);
        put_immutable_count(
            &remote,
            &format!("{prefix}/workspace.json"),
            &workspace_record,
            &mut descriptor_uploaded,
            &mut descriptor_present,
        )?;

        for oid in self.sync_git_object_ids(&checkpoints, &meta_refs)? {
            let bytes = self.read_loose_git_object(&oid)?;
            put_immutable_count(
                &remote,
                &format!("{prefix}/objects/git/sha1/{}/{}", &oid[..2], &oid[2..]),
                &bytes,
                &mut report.git_objects_uploaded,
                &mut report.git_objects_present,
            )?;
        }

        for hash in self.sync_cas_blob_hashes(&checkpoints)? {
            let path = self.layout.blobs().join(&hash);
            if !path.is_file() {
                return Err(Error::new(
                    ErrorCode::StoreCorrupt,
                    format!("CAS blob {hash} is missing"),
                )
                .with_hint("run `asp doctor --deep` before syncing"));
            }
            let actual = blobs::hash_file(&path)?;
            if actual != hash {
                return Err(Error::new(
                    ErrorCode::StoreCorrupt,
                    format!("CAS blob {hash} is corrupt (got {actual})"),
                )
                .with_hint("run `asp doctor --deep` before syncing"));
            }
            let bytes = std::fs::read(&path)?;
            put_immutable_count(
                &remote,
                &format!("{prefix}/objects/blobs/blake3/{hash}"),
                &bytes,
                &mut report.cas_blobs_uploaded,
                &mut report.cas_blobs_present,
            )?;
        }

        let now = crate::now_rfc3339();
        for (seq, target) in &checkpoints {
            let key = format!("{prefix}/refs/checkpoints/{seq}.json");
            let bytes = sync_ref_json(&self.meta.id, CHECKPOINT_REF_PREFIX, *seq, target, &now)?;
            put_append_only_ref(&remote, &key, &bytes, &mut report)?;
        }
        for (seq, target) in &meta_refs {
            let key = format!("{prefix}/refs/meta/{seq}.json");
            let bytes = sync_ref_json(&self.meta.id, META_REF_PREFIX, *seq, target, &now)?;
            put_append_only_ref(&remote, &key, &bytes, &mut report)?;
        }
        if let Some((seq, target)) = checkpoints.iter().next_back() {
            let key = format!("{prefix}/refs/head.json");
            let bytes = sync_ref_json(&self.meta.id, HEAD_REF, *seq, target, &now)?;
            put_head_ref(&remote, &key, &bytes, *seq, &mut report)?;
        }

        Ok(report)
    }

    pub fn sync_fetch_local(&self, remote_root: impl AsRef<Path>) -> Result<SyncFetchReport> {
        let _lock = StoreLock::acquire(&self.layout)?;
        self.journal.heal()?;
        let remote_root = remote_root.as_ref().to_path_buf();
        let remote = LocalRemote::open(&remote_root)?;
        let prefix = format!("asp-sync/v1/workspaces/{}", self.meta.id);
        verify_remote_workspace(&remote, &prefix, &self.meta.id)?;

        let local_checkpoints = self.checkpoint_refs()?;
        let local_meta_refs = self.meta_refs()?;
        let remote_checkpoints = remote_sync_refs(
            &remote,
            &format!("{prefix}/refs/checkpoints"),
            "checkpoint_ref",
        )?;
        let remote_meta_refs =
            remote_sync_refs(&remote, &format!("{prefix}/refs/meta"), "meta_ref")?;

        let mut report = SyncFetchReport {
            remote: remote_root,
            workspace_id: self.meta.id.clone(),
            refs_imported: 0,
            refs_present: 0,
            refs_conflicted: 0,
            git_objects_downloaded: 0,
            git_objects_present: 0,
            cas_blobs_downloaded: 0,
            cas_blobs_present: 0,
            head_updated: false,
            head_seq: self.head_seq_for(&local_checkpoints)?,
            conflicts: Vec::new(),
        };

        let checkpoint_imports = plan_ref_imports(
            "checkpoint_ref",
            &remote_checkpoints,
            &local_checkpoints,
            &mut report,
        );
        let meta_imports =
            plan_ref_imports("meta_ref", &remote_meta_refs, &local_meta_refs, &mut report);
        if !report.conflicts.is_empty() {
            report.refs_conflicted = report.conflicts.len() as u64;
            return Ok(report);
        }

        let remote_git_objects = remote_git_objects(&remote, &prefix)?;
        verify_git_object_batch(&remote_git_objects)?;
        for (oid, bytes) in &remote_git_objects {
            match write_local_git_object(self.shadow.git_dir(), oid, bytes)? {
                PutOutcome::Created => report.git_objects_downloaded += 1,
                PutOutcome::AlreadyExists => report.git_objects_present += 1,
                PutOutcome::Replaced => unreachable!("local object writes do not replace"),
            }
        }

        let remote_cas_blobs = remote_cas_blobs(&remote, &prefix)?;
        for (hash, bytes) in &remote_cas_blobs {
            match write_local_cas_blob(&self.layout.blobs(), hash, bytes)? {
                PutOutcome::Created => report.cas_blobs_downloaded += 1,
                PutOutcome::AlreadyExists => report.cas_blobs_present += 1,
                PutOutcome::Replaced => unreachable!("local CAS writes do not replace"),
            }
        }

        for remote_ref in checkpoint_imports {
            ensure_checkpoint_object(&self.shadow, remote_ref.seq, &remote_ref.target)?;
            self.shadow.update_ref(
                &format!("{CHECKPOINT_REF_PREFIX}{}", remote_ref.seq),
                &remote_ref.target,
            )?;
            report.refs_imported += 1;
        }
        for remote_ref in meta_imports {
            ensure_git_object(
                &self.shadow,
                "meta manifest",
                remote_ref.seq,
                &remote_ref.target,
            )?;
            self.shadow.update_ref(
                &format!("{META_REF_PREFIX}{}", remote_ref.seq),
                &remote_ref.target,
            )?;
            report.refs_imported += 1;
        }

        if let Some(head) = remote_head_ref(&remote, &prefix)? {
            let mut all_checkpoints = local_checkpoints.clone();
            for (seq, remote_ref) in &remote_checkpoints {
                all_checkpoints
                    .entry(*seq)
                    .or_insert(remote_ref.target.clone());
            }
            let current_head = self.head_seq_for(&local_checkpoints)?;
            if current_head.is_none_or(|seq| head.seq > seq)
                && all_checkpoints.get(&head.seq) == Some(&head.target)
            {
                ensure_checkpoint_object(&self.shadow, head.seq, &head.target)?;
                self.shadow.update_ref(HEAD_REF, &head.target)?;
                report.head_updated = true;
                report.head_seq = Some(head.seq);
            } else {
                report.head_seq = current_head;
            }
        }

        Ok(report)
    }

    pub fn sync_status_local(&self, remote_root: impl AsRef<Path>) -> Result<SyncStatusReport> {
        let _lock = StoreLock::acquire(&self.layout)?;
        self.journal.heal()?;
        let remote_root = remote_root.as_ref().to_path_buf();
        let remote = LocalRemote::open(&remote_root)?;
        let prefix = format!("asp-sync/v1/workspaces/{}", self.meta.id);

        let local_checkpoints = self.checkpoint_refs()?;
        let local_meta_refs = self.meta_refs()?;
        let local_head = local_head_ref(self.head_seq_for(&local_checkpoints)?, &local_checkpoints);
        let mut report = SyncStatusReport {
            remote: remote_root,
            workspace_id: self.meta.id.clone(),
            remote_initialized: false,
            local_checkpoint_refs: local_checkpoints.len() as u64,
            remote_checkpoint_refs: 0,
            checkpoint_refs_matching: 0,
            checkpoint_refs_local_only: local_checkpoints.len() as u64,
            checkpoint_refs_remote_only: 0,
            checkpoint_refs_conflicted: 0,
            local_meta_refs: local_meta_refs.len() as u64,
            remote_meta_refs: 0,
            meta_refs_matching: 0,
            meta_refs_local_only: local_meta_refs.len() as u64,
            meta_refs_remote_only: 0,
            meta_refs_conflicted: 0,
            local_head_seq: local_head.as_ref().map(|head| head.seq),
            remote_head_seq: None,
            head_relation: "remote_missing".to_string(),
            conflicts: Vec::new(),
        };

        let workspace_key = format!("{prefix}/workspace.json");
        if remote.get(&workspace_key)?.is_none() {
            return Ok(report);
        }
        verify_remote_workspace(&remote, &prefix, &self.meta.id)?;
        report.remote_initialized = true;

        let remote_checkpoints = remote_sync_refs(
            &remote,
            &format!("{prefix}/refs/checkpoints"),
            "checkpoint_ref",
        )?;
        let remote_meta_refs =
            remote_sync_refs(&remote, &format!("{prefix}/refs/meta"), "meta_ref")?;
        let remote_head = remote_head_ref(&remote, &prefix)?;

        let checkpoint_summary = summarize_sync_refs(
            "checkpoint_ref",
            &local_checkpoints,
            &remote_checkpoints,
            &mut report.conflicts,
        );
        let meta_summary = summarize_sync_refs(
            "meta_ref",
            &local_meta_refs,
            &remote_meta_refs,
            &mut report.conflicts,
        );

        report.remote_checkpoint_refs = remote_checkpoints.len() as u64;
        report.checkpoint_refs_matching = checkpoint_summary.matching;
        report.checkpoint_refs_local_only = checkpoint_summary.local_only;
        report.checkpoint_refs_remote_only = checkpoint_summary.remote_only;
        report.checkpoint_refs_conflicted = checkpoint_summary.conflicted;
        report.remote_meta_refs = remote_meta_refs.len() as u64;
        report.meta_refs_matching = meta_summary.matching;
        report.meta_refs_local_only = meta_summary.local_only;
        report.meta_refs_remote_only = meta_summary.remote_only;
        report.meta_refs_conflicted = meta_summary.conflicted;
        report.remote_head_seq = remote_head.as_ref().map(|head| head.seq);
        report.head_relation = sync_head_relation(local_head.as_ref(), remote_head.as_ref());

        Ok(report)
    }

    fn sync_git_object_ids(
        &self,
        checkpoints: &BTreeMap<u64, String>,
        meta_refs: &BTreeMap<u64, String>,
    ) -> Result<BTreeSet<String>> {
        let mut objects: BTreeSet<String> = BTreeSet::new();
        if !checkpoints.is_empty() {
            let mut args: Vec<String> = vec!["rev-list".into(), "--objects".into()];
            args.extend(checkpoints.values().cloned());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let out = self.shadow.run(&refs)?;
            for line in out.lines() {
                if let Some(oid) = line.split_whitespace().next() {
                    objects.insert(oid.to_string());
                }
            }
        }
        objects.extend(checkpoints.values().cloned());
        objects.extend(meta_refs.values().cloned());
        Ok(objects)
    }

    fn read_loose_git_object(&self, oid: &str) -> Result<Vec<u8>> {
        if oid.len() != 40 || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::new(
                ErrorCode::StoreCorrupt,
                format!("invalid git object id in shadow store: {oid}"),
            )
            .with_hint("run `asp doctor` before syncing"));
        }
        let path = self
            .shadow
            .git_dir()
            .join("objects")
            .join(&oid[..2])
            .join(&oid[2..]);
        if !path.is_file() {
            return Err(Error::new(
                ErrorCode::StoreCorrupt,
                format!("shadow git object {oid} is not available as a loose object"),
            )
            .with_hint(
                "run `asp doctor`; packed shadow objects are not supported by sync push yet",
            ));
        }
        std::fs::read(path).map_err(Into::into)
    }

    fn sync_cas_blob_hashes(
        &self,
        checkpoints: &BTreeMap<u64, String>,
    ) -> Result<BTreeSet<String>> {
        let mut hashes = BTreeSet::new();
        for seq in checkpoints.keys() {
            if let Some(manifest) = self.manifest_for(*seq)? {
                for pointer in manifest.pointers {
                    hashes.insert(pointer.blake3);
                }
            }
        }
        Ok(hashes)
    }

    fn head_seq_for(&self, refs: &BTreeMap<u64, String>) -> Result<Option<u64>> {
        let Some(head) = self.shadow.rev_parse(HEAD_REF)? else {
            return Ok(None);
        };
        Ok(refs
            .iter()
            .find_map(|(seq, target)| (target == &head).then_some(*seq)))
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

    fn enforce_checkpoint_age_policy(&self, operation: &str) -> Result<()> {
        let Some(max_hours) = self.policy.checkpoints.max_age_hours else {
            return Ok(());
        };
        let checkpoint = self.last_checkpoint()?.ok_or_else(|| {
            policy_violation(
                format!("policy blocks {operation}: no checkpoint exists"),
                format!("run `asp checkpoint` before `{operation}`, or edit .asp/policy.toml"),
            )
        })?;
        let checkpoint_time = OffsetDateTime::parse(&checkpoint.ts, &Rfc3339).map_err(|e| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!(
                    "checkpoint #{} has an unreadable timestamp: {e}",
                    checkpoint.seq
                ),
            )
            .with_hint(
                "run `asp doctor`; if the journal cannot be repaired, create a fresh checkpoint",
            )
        })?;
        let age_hours = (OffsetDateTime::now_utc() - checkpoint_time).whole_hours();
        if i128::from(age_hours) > i128::from(max_hours) {
            return Err(policy_violation(
                format!(
                    "policy blocks {operation}: latest checkpoint #{} is {age_hours}h old, exceeding checkpoints.max_age_hours={max_hours}",
                    checkpoint.seq
                ),
                format!("run `asp checkpoint` before `{operation}`, or raise checkpoints.max_age_hours in .asp/policy.toml"),
            ));
        }
        Ok(())
    }

    fn enforce_fork_count_policy(&self, registry: &ForkRegistry) -> Result<()> {
        let Some(max_active) = self.policy.forks.max_active else {
            return Ok(());
        };
        let active = registry
            .forks
            .iter()
            .filter(|fork| fork.status == ForkStatus::Active)
            .count() as u64;
        if active >= max_active {
            return Err(policy_violation(
                format!(
                    "policy blocks fork: {active} active forks already meet forks.max_active={max_active}"
                ),
                "discard or promote an active fork, or raise forks.max_active in .asp/policy.toml",
            ));
        }
        Ok(())
    }

    fn enforce_promote_policy(
        &self,
        branch: &str,
        protected_paths: impl IntoIterator<Item = String>,
    ) -> Result<()> {
        self.enforce_checkpoint_age_policy("promote")?;
        if self.policy.promote.require_checkpoint && self.last_checkpoint()?.is_none() {
            return Err(policy_violation(
                "policy blocks promote: promote.require_checkpoint is true but no checkpoint exists",
                "run `asp checkpoint` before `asp promote`, or edit .asp/policy.toml",
            ));
        }
        if self.policy.promote.require_clean_status {
            let status = self.status()?;
            let dirty = status.dirty_files + status.untracked_files + status.deleted_files;
            if dirty > 0 {
                return Err(policy_violation(
                    format!(
                        "policy blocks promote: promote.require_clean_status is true and the main workspace has {dirty} dirty paths"
                    ),
                    "run `asp checkpoint` or clean the main workspace before `asp promote`",
                ));
            }
        }
        if !self.policy.promote.allowed_branch_prefixes.is_empty()
            && !self
                .policy
                .promote
                .allowed_branch_prefixes
                .iter()
                .any(|prefix| branch.starts_with(prefix))
        {
            return Err(policy_violation(
                format!("policy blocks promote: branch '{branch}' does not match promote.allowed_branch_prefixes"),
                "choose an allowed branch prefix, or edit .asp/policy.toml after review",
            ));
        }
        self.enforce_unprotected_paths("promote", protected_paths)
    }

    fn enforce_unprotected_paths(
        &self,
        operation: &str,
        paths: impl IntoIterator<Item = String>,
    ) -> Result<()> {
        if self.policy.paths.protected.is_empty() {
            return Ok(());
        }
        let mut unique_paths = BTreeSet::new();
        for path in paths {
            unique_paths.insert(path);
        }
        for path in unique_paths {
            if let Some(pattern) = self
                .policy
                .paths
                .protected
                .iter()
                .find(|pattern| policy_path_matches(pattern, &path))
            {
                return Err(policy_violation(
                    format!(
                        "policy blocks {operation}: protected path '{path}' matches '{pattern}'"
                    ),
                    "review the change, narrow the command to unprotected paths, or edit .asp/policy.toml",
                ));
            }
        }
        Ok(())
    }

    fn enforce_checkpoint_deny_paths(&self, paths: impl IntoIterator<Item = String>) -> Result<()> {
        if self.policy.paths.deny_checkpoint.is_empty() {
            return Ok(());
        }
        let mut unique_paths = BTreeSet::new();
        for path in paths {
            unique_paths.insert(path);
        }
        for path in unique_paths {
            if let Some(pattern) = self
                .policy
                .paths
                .deny_checkpoint
                .iter()
                .find(|pattern| policy_path_matches(pattern, &path))
            {
                return Err(policy_violation(
                    format!(
                        "policy blocks checkpoint: path '{path}' matches paths.deny_checkpoint entry '{pattern}'"
                    ),
                    "remove or move the file, adjust paths.deny_checkpoint after review, or run `asp secrets scan` to inspect likely secrets",
                ));
            }
        }
        Ok(())
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
        let t0 = Instant::now();
        let _lock = StoreLock::acquire(&self.layout)?;
        self.journal.heal()?;
        let (target_seq, target_commit) = self.resolve_checkpoint(spec)?;
        self.enforce_checkpoint_age_policy("restore")?;

        // Validate targeted paths up front — a friendly error beats raw git.
        if paths.is_empty() {
            let (current_tree, _) = self.stage_tree()?;
            let touched = self.changed_paths_between(&current_tree, &target_commit)?;
            self.enforce_unprotected_paths("restore", touched)?;
        } else {
            let mut touched = Vec::new();
            for p in paths {
                let listed = self.shadow.run_raw(&[
                    "ls-tree",
                    "-r",
                    "-z",
                    "--name-only",
                    &target_commit,
                    "--",
                    p,
                ])?;
                let listed_paths: Vec<String> = listed
                    .split(|&b| b == 0)
                    .filter(|s| !s.is_empty())
                    .map(|path| String::from_utf8_lossy(path).to_string())
                    .collect();
                if listed_paths.is_empty() {
                    return Err(Error::new(
                        ErrorCode::CheckpointNotFound,
                        format!("path '{p}' does not exist in checkpoint #{target_seq}"),
                    )
                    .with_hint(format!(
                        "see what that checkpoint contains: asp diff {target_seq}"
                    )));
                }
                touched.extend(listed_paths);
            }
            self.enforce_unprotected_paths("restore", touched)?;
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
        let target_manifest = self.manifest_for(target_seq)?;

        let (files_written, files_deleted) = if paths.is_empty() {
            if let Some(manifest) = target_manifest.as_ref() {
                self.validate_manifest_restore_paths(manifest, None)?;
            }
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
            if let Some(manifest) = target_manifest.as_ref() {
                let mut bf = BigFiles {
                    v: 1,
                    files: Default::default(),
                };
                for ptr in &manifest.pointers {
                    let abs = crate::store::safe_worktree_write_path(&self.layout.root, &ptr.path)?;
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
            if let Some(manifest) = target_manifest.as_ref() {
                self.validate_manifest_restore_paths(manifest, Some(paths))?;
            }
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
            if let Some(manifest) = target_manifest.as_ref() {
                let bf_path = blobs::bigfiles_path(&self.layout.asp);
                let mut bf = blobs::load_bigfiles(&bf_path)?;
                for ptr in manifest.pointers.iter().filter(|p| paths.contains(&p.path)) {
                    let abs = crate::store::safe_worktree_write_path(&self.layout.root, &ptr.path)?;
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
        entry.duration_ms = Some(t0.elapsed().as_millis() as u64);
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
        let changed_paths = self.checkpoint_changed_paths(parent.as_deref(), &commit)?;
        let files_changed = changed_paths.len() as u64;
        let mut entry = Entry::new(Op::Checkpoint);
        entry.seq = Some(seq);
        entry.commit = Some(commit.clone());
        entry.source = source;
        entry.message = Some(message.clone());
        entry.files_changed = Some(files_changed);
        entry.duration_ms = Some(t0.elapsed().as_millis() as u64);
        entry.detail = Some(serde_json::json!({ "paths": changed_paths }));
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
        self.enforce_checkpoint_age_policy("fork")?;
        self.enforce_fork_count_policy(&registry)?;

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
                let _ = remove_fork_path(&dst);
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
        fe.duration_ms = Some(t0.elapsed().as_millis() as u64);
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
        entry.duration_ms = Some(t0.elapsed().as_millis() as u64);
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
                    review: fork_review_signals(&[], None),
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
            let diff_rows = fork_ws.diff_rows(&format!("{base}^{{tree}}"), &tree)?;
            let diff_summary = summarize_diff(&diff_rows);
            let review = fork_review_signals(&diff_rows, None);
            let last = fork_ws.journal.read()?.entries.last().map(|e| e.ts.clone());
            rows.push(ForkCompareRow {
                name: rec.name.clone(),
                status: rec.status,
                fork_point_seq: rec.fork_point_seq,
                files_changed: diff_summary.files,
                insertions: diff_summary.insertions,
                deletions: diff_summary.deletions,
                review,
                last_activity: last,
                path: rec.path.clone(),
                missing: false,
            });
        }
        Ok(rows)
    }

    // ----------------------------------------------------------------- diff

    /// Diff two checkpoints, or a checkpoint against the working tree.
    pub fn diff(&self, from_spec: &str, to_spec: Option<&str>) -> Result<DiffReport> {
        let (from_label, from_tree, to_label, to_tree) =
            self.resolve_diff_trees(from_spec, to_spec)?;
        self.diff_report(from_label, from_tree, to_label, to_tree)
    }

    /// Patch/stat text between two checkpoints, or a checkpoint and the working tree.
    pub fn diff_text(
        &self,
        from_spec: &str,
        to_spec: Option<&str>,
        mode: DiffTextMode,
    ) -> Result<DiffTextReport> {
        let (from_label, from_tree, to_label, to_tree) =
            self.resolve_diff_trees(from_spec, to_spec)?;
        self.diff_text_report(from_label, from_tree, to_label, to_tree, mode)
    }

    /// Diff an active fork against its fork point.
    pub fn diff_fork(&self, fork_name: &str) -> Result<DiffReport> {
        let (from_label, from_tree, to_label, to_tree, fork_ws) =
            self.resolve_fork_diff_trees(fork_name)?;
        fork_ws.diff_report(from_label, from_tree, to_label, to_tree)
    }

    /// Patch/stat text for an active fork against its fork point.
    pub fn diff_fork_text(&self, fork_name: &str, mode: DiffTextMode) -> Result<DiffTextReport> {
        let (from_label, from_tree, to_label, to_tree, fork_ws) =
            self.resolve_fork_diff_trees(fork_name)?;
        fork_ws.diff_text_report(from_label, from_tree, to_label, to_tree, mode)
    }

    fn resolve_diff_trees(
        &self,
        from_spec: &str,
        to_spec: Option<&str>,
    ) -> Result<(String, String, String, String)> {
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
        Ok((from_label, from_commit, to_label, to_tree))
    }

    fn diff_report(
        &self,
        from_label: String,
        from_tree: String,
        to_label: String,
        to_tree: String,
    ) -> Result<DiffReport> {
        let rows = self.diff_rows(&from_tree, &to_tree)?;
        Ok(DiffReport {
            from: from_label,
            to: to_label,
            summary: summarize_diff(&rows),
            rows,
        })
    }

    fn diff_text_report(
        &self,
        from_label: String,
        from_tree: String,
        to_label: String,
        to_tree: String,
        mode: DiffTextMode,
    ) -> Result<DiffTextReport> {
        let rows = self.diff_rows(&from_tree, &to_tree)?;
        let raw = self.shadow.run_raw(&[
            "diff-tree",
            "-r",
            mode.git_arg(),
            "--no-ext-diff",
            &from_tree,
            &to_tree,
        ])?;
        Ok(DiffTextReport {
            from: from_label,
            to: to_label,
            mode: mode.as_str().to_string(),
            summary: summarize_diff(&rows),
            text: String::from_utf8_lossy(&raw).to_string(),
        })
    }

    fn resolve_fork_diff_trees(
        &self,
        fork_name: &str,
    ) -> Result<(String, String, String, String, Workspace)> {
        let registry = self.fork_registry()?;
        let rec = registry
            .forks
            .iter()
            .find(|fork| fork.name == fork_name && fork.status == ForkStatus::Active)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ForkNotFound,
                    format!("no active fork named '{fork_name}'"),
                )
                .with_hint("run `asp forks` to list active forks")
            })?;
        if !rec.path.exists() {
            return Err(Error::new(
                ErrorCode::ForkNotFound,
                format!("fork '{}' is missing at {}", rec.name, rec.path.display()),
            )
            .with_hint("run `asp doctor --fix` to inspect missing fork directories"));
        }
        let fork_ws = Workspace::open_root(&rec.path)?;
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
        Ok((
            format!("#{}", rec.fork_point_seq),
            format!("{base}^{{tree}}"),
            format!("fork {}", rec.name),
            tree,
            fork_ws,
        ))
    }

    fn diff_rows(&self, from: &str, to: &str) -> Result<Vec<DiffRow>> {
        // name-status + numstat joined by path.
        let ns_raw = self
            .shadow
            .run_raw(&["diff-tree", "-r", "-z", "--name-status", from, to])?;
        let mut status_by_path: BTreeMap<String, String> = BTreeMap::new();
        let mut parts = ns_raw.split(|&b| b == 0).filter(|s| !s.is_empty());
        while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
            status_by_path.insert(
                String::from_utf8_lossy(path).to_string(),
                String::from_utf8_lossy(status).to_string(),
            );
        }
        let num_raw = self
            .shadow
            .run_raw(&["diff-tree", "-r", "-z", "--numstat", from, to])?;
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
        Ok(rows)
    }

    // -------------------------------------------------------------- promote

    fn fork_changed_paths_for_policy(&self, rec: &ForkRecord) -> Result<Vec<String>> {
        let fork_ws = Workspace::open_root(&rec.path)?;
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
        fork_ws.changed_paths_between(&format!("{base}^{{tree}}"), &tree)
    }

    /// Land a fork's work as an ordinary branch in the user's git repo.
    /// Never touches HEAD, never force-pushes, never runs user hooks.
    pub fn promote(&self, fork_name: &str, branch: Option<String>) -> Result<PromoteReport> {
        let t0 = Instant::now();
        let _lock = StoreLock::acquire(&self.layout)?;
        let mut registry = self.fork_registry()?;
        let rec_index = registry
            .forks
            .iter()
            .position(|f| f.name == fork_name && f.status == ForkStatus::Active)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ForkNotFound,
                    format!("no active fork named '{fork_name}'"),
                )
                .with_hint("run `asp forks` to list forks")
            })?;
        let rec = registry.forks[rec_index].clone();
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

        let branch = branch.unwrap_or_else(|| self.default_promote_branch(fork_name));
        validate_user_branch_name(&self.layout.root, &branch)?;
        if user_git_ref_exists(&self.layout.root, &branch)? {
            return Err(Error::new(
                ErrorCode::BranchExists,
                format!("branch '{branch}' already exists in the user repo"),
            )
            .with_hint("pass a different name: `asp promote <fork> --branch <name>`"));
        }
        let protected_paths = self.fork_changed_paths_for_policy(&rec)?;
        self.enforce_promote_policy(&branch, protected_paths)?;

        // Build a commit in the FORK's user repo via plumbing (no checkout,
        // no HEAD move, no hooks), then fetch it into the original repo.
        let fork_path = rec.path.clone();
        let cleanup_command = format!("asp discard {fork_name}");
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

        registry.forks[rec_index].status = ForkStatus::Promoted;
        atomic_write_json(&self.layout.forks_json(), &registry)?;
        let mut entry = Entry::new(Op::Promote);
        entry.duration_ms = Some(t0.elapsed().as_millis() as u64);
        entry.detail = Some(serde_json::json!({
            "fork": fork_name,
            "fork_path": fork_path,
            "fork_retained": true,
            "branch": branch,
            "commit": commit,
            "cleanup_command": cleanup_command,
        }));
        self.journal.append(&entry)?;

        Ok(PromoteReport {
            fork: fork_name.to_string(),
            fork_path,
            fork_retained: true,
            branch,
            commit,
            cleanup_command,
            push: None,
            pr: None,
        })
    }

    pub fn push_promoted_branch(&self, remote: &str, branch: &str) -> Result<PromotePushReport> {
        let remote = remote.trim();
        if remote.is_empty() || remote.chars().any(char::is_whitespace) {
            return Err(Error::new(
                ErrorCode::NothingToDo,
                "promote --push needs a non-empty remote name",
            )
            .with_hint("retry with an explicit remote, for example `asp promote <fork> --push --remote origin`"));
        }
        validate_user_branch_name(&self.layout.root, branch)?;

        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
        run_user_git(&self.layout.root, &["push", "--porcelain", remote, &refspec]).map_err(
            |err| {
                err.with_hint(format!(
                    "the local branch '{branch}' still exists; check remote '{remote}' and retry `git push {remote} {refspec}`"
                ))
            },
        )?;

        Ok(PromotePushReport {
            pushed: true,
            remote: remote.to_string(),
            branch: branch.to_string(),
            command: format!("git push {remote} {refspec}"),
            refspec,
        })
    }

    // -------------------------------------------------------------- discard

    pub fn discard(&self, fork_name: &str, force: bool) -> Result<()> {
        let t0 = Instant::now();
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

        let rec_path_meta = match std::fs::symlink_metadata(&rec.path) {
            Ok(md) => Some(md),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(Error::new(
                    ErrorCode::Io,
                    format!("inspect fork path {}: {e}", rec.path.display()),
                ));
            }
        };

        // Promoted forks need no guard — their work already landed as a branch.
        if rec.status == ForkStatus::Active && !force {
            if let Some(md) = rec_path_meta.as_ref() {
                if md.file_type().is_symlink() || !md.is_dir() {
                    return Err(Error::new(
                        ErrorCode::StoreCorrupt,
                        format!(
                            "fork '{}' path is not a real directory ({})",
                            fork_name,
                            rec.path.display()
                        ),
                    )
                    .with_hint(format!(
                        "inspect the path, then remove the registry entry with `asp discard {fork_name} --force`"
                    )));
                }
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
        }
        remove_fork_path(&rec.path)?;
        rec.status = ForkStatus::Discarded;
        atomic_write_json(&self.layout.forks_json(), &registry)?;
        let mut entry = Entry::new(Op::Discard);
        entry.duration_ms = Some(t0.elapsed().as_millis() as u64);
        entry.detail = Some(serde_json::json!({ "fork": fork_name, "forced": force }));
        self.journal.append(&entry)?;
        Ok(())
    }

    fn default_promote_branch(&self, fork_name: &str) -> String {
        let workspace_name = self
            .layout
            .root
            .file_name()
            .map(|name| sanitize_component(&name.to_string_lossy(), "workspace"))
            .unwrap_or_else(|| "workspace".to_string());
        self.config
            .render_promote_branch(fork_name, &workspace_name, &self.meta.id)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
    pub cause: String,
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair_plan: Option<RepairPlan>,
    pub fixed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairPlan {
    pub operation: String,
    pub description: String,
    pub command: String,
    pub destructive: bool,
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
            let (cause, next_action) = doctor_explanation(&message, fixed);
            let repair_plan = doctor_repair_plan(&message, deep);
            findings.push(Finding {
                severity,
                message,
                cause,
                next_action,
                repair_plan,
                fixed,
            })
        };

        // Repairs mutate the store — hold the lock for a --fix run.
        let _lock = if fix {
            Some(StoreLock::acquire_with_retry(&self.layout)?)
        } else {
            None
        };

        // 1. Runtime prerequisites and shadow-git config.
        if let Err(e) = crate::gitx::ensure_git_version() {
            add(
                Severity::Error,
                match e.hint {
                    Some(hint) => format!("{} (hint: {hint})", e.message),
                    None => e.message,
                },
                false,
            );
        }
        for (key, expected) in [
            ("core.compression", "0"),
            ("pack.compression", "1"),
            ("gc.auto", "0"),
            ("core.untrackedCache", "true"),
        ] {
            let actual = self
                .shadow
                .run(&["config", "--get", key])
                .unwrap_or_default();
            if actual.trim() != expected {
                let fixed = if fix {
                    self.shadow.run(&["config", key, expected])?;
                    true
                } else {
                    false
                };
                add(
                    Severity::Warning,
                    format!(
                        "shadow git config {key} is {:?}, expected {expected:?}",
                        actual.trim()
                    ),
                    fixed,
                );
            }
        }

        // 2. Journal integrity.
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

        // 3. Checkpoint refs resolvable + head consistency.
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

        // 4. Journal entries referencing refs that don't exist (crash window).
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

        // 5. Fork registry vs reality. Pending entries are deterministic
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
                    let exists = std::fs::symlink_metadata(&rec.path).is_ok();
                    let fixed = if fix {
                        remove_fork_path(&rec.path)?;
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
                ForkStatus::Promoted if rec.path.exists() => {
                    add(
                        Severity::Info,
                        format!(
                            "fork '{}' was promoted but its directory still exists ({}); run `asp discard {}` to remove it",
                            rec.name,
                            rec.path.display(),
                            rec.name
                        ),
                        false,
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

        // 6. Unregistered fork-looking sibling dirs: REPORT ONLY. asp never
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

        // 7. Big-file CAS integrity.
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

fn doctor_explanation(message: &str, fixed: bool) -> (String, String) {
    let (cause, next_action) = if message.contains("shadow git config") {
        (
            "The shadow git repository has drifted from asp's expected performance and safety settings.".to_string(),
            "Run `asp doctor --fix` to restore the shadow git config values.".to_string(),
        )
    } else if message.contains("torn tail") {
        (
            "A process stopped while appending to the journal, leaving a partial trailing record."
                .to_string(),
            "Run `asp doctor --fix` to truncate the torn journal tail.".to_string(),
        )
    } else if message.contains("CRC mismatch") {
        (
            "A journal record failed its checksum, so asp cannot trust that provenance entry.".to_string(),
            "Preserve the workspace for investigation; restore `.asp/journal.jsonl` from backup if that provenance is required.".to_string(),
        )
    } else if message.contains("points at missing commit") {
        (
            "A checkpoint ref names a git object that is missing from `.asp/shadow.git`.".to_string(),
            "Restore `.asp/shadow.git` from backup or use stock git recovery to inspect remaining checkpoint refs.".to_string(),
        )
    } else if message.contains("head ref does not match") {
        (
            "The shadow HEAD ref is stale relative to the latest checkpoint ref.".to_string(),
            "Run `asp doctor --fix` to repoint the shadow HEAD to the latest checkpoint."
                .to_string(),
        )
    } else if message.contains("journal records checkpoint") && message.contains("ref is missing") {
        (
            "The journal recorded a checkpoint, but the matching checkpoint ref was never created or was removed.".to_string(),
            "Create a fresh checkpoint after reviewing the current workspace state; keep backups if the missing checkpoint matters.".to_string(),
        )
    } else if message.contains("registered active but its directory is gone") {
        (
            "The fork registry still lists an active fork whose directory was deleted outside asp."
                .to_string(),
            "Run `asp doctor --fix` to mark the missing fork discarded in the registry."
                .to_string(),
        )
    } else if message.contains("torn clone") {
        (
            "Fork creation was interrupted after asp wrote a pending fork intent.".to_string(),
            "Run `asp doctor --fix` to remove the proven torn fork entry and any half-created directory.".to_string(),
        )
    } else if message.contains("was promoted but its directory still exists") {
        (
            "Promotion keeps the fork directory on disk so the work remains inspectable until you clean it up.".to_string(),
            "Run the `asp discard <fork>` command shown in the finding after review is complete.".to_string(),
        )
    } else if message.contains("looks like a fork of this workspace but is not in the registry") {
        (
            "A sibling directory matches asp's fork naming pattern, but asp has no registry proof that it owns it.".to_string(),
            "Inspect the directory manually; remove it yourself only if it is not needed.".to_string(),
        )
    } else if message.contains("CAS blob") && message.contains("missing (re-creatable") {
        (
            "A large-file sidecar object is missing, but the current working file still has bytes asp can re-store.".to_string(),
            "Run `asp doctor --fix` to recreate the missing CAS blob from the working file.".to_string(),
        )
    } else if message.contains("CAS blob") && message.contains("is missing and the file is gone") {
        (
            "Both the large-file sidecar object and the working file are missing.".to_string(),
            "Restore `.asp/blobs/` or the working file from backup before relying on checkpoints that reference it.".to_string(),
        )
    } else if message.contains("CAS blob") && message.contains("is corrupt") {
        (
            "Deep verification re-hashed a large-file sidecar object and found bytes that do not match its content address.".to_string(),
            "Restore the corrupt blob from backup, then rerun `asp doctor --deep`.".to_string(),
        )
    } else if message.contains("hint:") {
        (
            "A runtime prerequisite check failed.".to_string(),
            "Follow the hint embedded in the finding, then rerun `asp doctor`.".to_string(),
        )
    } else {
        (
            "Doctor found workspace state that may need attention.".to_string(),
            "Read the finding, keep backups, and rerun `asp doctor --fix` only for repairs asp says are safe.".to_string(),
        )
    };

    if fixed {
        (
            cause,
            "Doctor applied the safe repair; no further action is needed for this finding."
                .to_string(),
        )
    } else {
        (cause, next_action)
    }
}

fn doctor_repair_plan(message: &str, deep: bool) -> Option<RepairPlan> {
    let command = if deep {
        "asp doctor --fix --deep"
    } else {
        "asp doctor --fix"
    };
    let plan = |operation: &str, description: &str, destructive: bool| {
        Some(RepairPlan {
            operation: operation.to_string(),
            description: description.to_string(),
            command: command.to_string(),
            destructive,
        })
    };

    if message.contains("shadow git config") {
        plan(
            "reset_shadow_git_config",
            "Reset the drifted shadow-git config key to asp's expected value.",
            false,
        )
    } else if message.contains("torn tail") {
        plan(
            "truncate_torn_journal_tail",
            "Truncate the incomplete trailing journal bytes after the last valid record.",
            true,
        )
    } else if message.contains("head ref does not match") {
        plan(
            "repoint_shadow_head",
            "Repoint the shadow HEAD ref to the latest checkpoint ref.",
            false,
        )
    } else if message.contains("registered active but its directory is gone") {
        plan(
            "mark_missing_fork_discarded",
            "Mark the missing active fork as discarded in the fork registry.",
            false,
        )
    } else if message.contains("torn clone") {
        plan(
            "remove_pending_fork",
            "Remove the proven pending fork directory if it exists and delete its pending registry entry.",
            true,
        )
    } else if message.contains("CAS blob") && message.contains("missing (re-creatable") {
        plan(
            "recreate_missing_cas_blob",
            "Recreate the missing large-file CAS blob from the current working file.",
            false,
        )
    } else {
        None
    }
}

fn put_immutable_count(
    remote: &dyn SyncRemote,
    key: &str,
    bytes: &[u8],
    created: &mut u64,
    present: &mut u64,
) -> Result<()> {
    match remote.put_immutable(key, bytes)? {
        PutOutcome::Created => *created += 1,
        PutOutcome::AlreadyExists => *present += 1,
        PutOutcome::Replaced => unreachable!("immutable writes do not replace existing keys"),
    }
    Ok(())
}

fn sync_ref_json(
    workspace_id: &str,
    ref_name: &str,
    seq: u64,
    target: &str,
    updated_at: &str,
) -> Result<Vec<u8>> {
    let name = if ref_name.ends_with('/') {
        format!("{ref_name}{seq}")
    } else {
        ref_name.to_string()
    };
    serde_json::to_vec_pretty(&serde_json::json!({
        "v": 1,
        "name": name,
        "seq": seq,
        "target": target,
        "workspace_id": workspace_id,
        "updated_at": updated_at,
        "writer": "local",
    }))
    .map_err(|e| Error::new(ErrorCode::Io, format!("encode sync ref {name}: {e}")))
}

fn put_append_only_ref(
    remote: &dyn SyncRemote,
    key: &str,
    bytes: &[u8],
    report: &mut SyncPushReport,
) -> Result<()> {
    match remote.get(key)? {
        Some(existing) => {
            let expected = sync_ref_fields(bytes, key)?;
            let actual = sync_ref_fields(&existing.bytes, key)?;
            if actual == expected {
                report.refs_present += 1;
                Ok(())
            } else {
                Err(Error::new(
                    ErrorCode::SyncConflict,
                    format!("remote ref {key} already exists with a different target"),
                )
                .with_hint("fetch the latest remote state, review conflicts, and retry"))
            }
        }
        None => match remote.put_if_match(key, bytes, None)? {
            PutOutcome::Created => {
                report.refs_created += 1;
                Ok(())
            }
            PutOutcome::AlreadyExists => {
                report.refs_present += 1;
                Ok(())
            }
            PutOutcome::Replaced => unreachable!("conditional create does not replace refs"),
        },
    }
}

fn put_head_ref(
    remote: &dyn SyncRemote,
    key: &str,
    bytes: &[u8],
    local_seq: u64,
    report: &mut SyncPushReport,
) -> Result<()> {
    match remote.get(key)? {
        Some(existing) => {
            let expected = sync_ref_fields(bytes, key)?;
            let actual = sync_ref_fields(&existing.bytes, key)?;
            if actual == expected {
                report.refs_present += 1;
                return Ok(());
            }
            if actual.0 >= local_seq {
                return Err(Error::new(
                    ErrorCode::SyncConflict,
                    format!(
                        "remote head is at checkpoint #{}; local head is checkpoint #{local_seq}",
                        actual.0
                    ),
                )
                .with_hint("fetch the latest remote state, review conflicts, and retry"));
            }

            match remote.put_if_match(key, bytes, Some(&existing.version))? {
                PutOutcome::Replaced => {
                    report.refs_replaced += 1;
                    Ok(())
                }
                PutOutcome::AlreadyExists => {
                    report.refs_present += 1;
                    Ok(())
                }
                PutOutcome::Created => {
                    report.refs_created += 1;
                    Ok(())
                }
            }
        }
        None => match remote.put_if_match(key, bytes, None)? {
            PutOutcome::Created => {
                report.refs_created += 1;
                Ok(())
            }
            PutOutcome::AlreadyExists => {
                report.refs_present += 1;
                Ok(())
            }
            PutOutcome::Replaced => unreachable!("conditional create does not replace refs"),
        },
    }
}

fn verify_remote_workspace(
    remote: &dyn SyncRemote,
    prefix: &str,
    workspace_id: &str,
) -> Result<()> {
    let key = format!("{prefix}/workspace.json");
    let Some(object) = remote.get(&key)? else {
        return Err(Error::new(
            ErrorCode::NothingToDo,
            format!("sync remote has no workspace record for {workspace_id}"),
        )
        .with_hint("run `asp sync push --remote <dir>` from this workspace first"));
    };
    let value: serde_json::Value = serde_json::from_slice(&object.bytes).map_err(|e| {
        Error::new(
            ErrorCode::StoreCorrupt,
            format!("remote workspace record is not valid JSON: {e}"),
        )
        .with_hint("inspect the sync remote before retrying")
    })?;
    if value.get("workspace_id").and_then(|id| id.as_str()) != Some(workspace_id) {
        return Err(Error::new(
            ErrorCode::StoreCorrupt,
            "remote workspace record does not match this workspace id",
        )
        .with_hint(
            "use the remote created for this workspace, or initialize a matching restore workspace",
        ));
    }
    Ok(())
}

fn remote_sync_refs(
    remote: &dyn SyncRemote,
    key_prefix: &str,
    kind: &str,
) -> Result<BTreeMap<u64, SyncRef>> {
    let mut refs = BTreeMap::new();
    for entry in remote.list(key_prefix)? {
        if !entry.key.ends_with(".json") {
            continue;
        }
        let object = remote.get(&entry.key)?.ok_or_else(|| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("remote listed ref {} but it is missing", entry.key),
            )
            .with_hint("inspect the sync remote before retrying")
        })?;
        let (seq, target) = sync_ref_fields(&object.bytes, &entry.key)?;
        if refs.insert(seq, SyncRef { seq, target }).is_some() {
            return Err(Error::new(
                ErrorCode::StoreCorrupt,
                format!("remote has duplicate {kind} for checkpoint #{seq}"),
            )
            .with_hint("inspect the sync remote before retrying"));
        }
    }
    Ok(refs)
}

fn remote_head_ref(remote: &dyn SyncRemote, prefix: &str) -> Result<Option<SyncRef>> {
    let key = format!("{prefix}/refs/head.json");
    let Some(object) = remote.get(&key)? else {
        return Ok(None);
    };
    let (seq, target) = sync_ref_fields(&object.bytes, &key)?;
    Ok(Some(SyncRef { seq, target }))
}

fn local_head_ref(head_seq: Option<u64>, refs: &BTreeMap<u64, String>) -> Option<SyncRef> {
    head_seq.and_then(|seq| {
        refs.get(&seq).map(|target| SyncRef {
            seq,
            target: target.clone(),
        })
    })
}

fn summarize_sync_refs(
    kind: &str,
    local_refs: &BTreeMap<u64, String>,
    remote_refs: &BTreeMap<u64, SyncRef>,
    conflicts: &mut Vec<SyncRefConflict>,
) -> SyncRefSummary {
    let mut summary = SyncRefSummary::default();
    for (seq, local) in local_refs {
        match remote_refs.get(seq) {
            Some(remote) if &remote.target == local => summary.matching += 1,
            Some(remote) => {
                summary.conflicted += 1;
                conflicts.push(SyncRefConflict {
                    kind: kind.to_string(),
                    seq: *seq,
                    local: Some(local.clone()),
                    remote: Some(remote.target.clone()),
                    hint: "review both histories before pushing or fetching this ref".to_string(),
                });
            }
            None => summary.local_only += 1,
        }
    }
    for seq in remote_refs.keys() {
        if !local_refs.contains_key(seq) {
            summary.remote_only += 1;
        }
    }
    summary
}

fn sync_head_relation(local: Option<&SyncRef>, remote: Option<&SyncRef>) -> String {
    match (local, remote) {
        (None, None) => "both_missing",
        (Some(_), None) => "local_only",
        (None, Some(_)) => "remote_only",
        (Some(local), Some(remote)) if local == remote => "matching",
        (Some(local), Some(remote)) if local.seq > remote.seq => "local_ahead",
        (Some(local), Some(remote)) if local.seq < remote.seq => "remote_ahead",
        (Some(_), Some(_)) => "diverged",
    }
    .to_string()
}

fn plan_ref_imports(
    kind: &str,
    remote_refs: &BTreeMap<u64, SyncRef>,
    local_refs: &BTreeMap<u64, String>,
    report: &mut SyncFetchReport,
) -> Vec<SyncRef> {
    let mut imports = Vec::new();
    for (seq, remote_ref) in remote_refs {
        match local_refs.get(seq) {
            Some(local) if local == &remote_ref.target => report.refs_present += 1,
            Some(local) => report.conflicts.push(SyncRefConflict {
                kind: kind.to_string(),
                seq: *seq,
                local: Some(local.clone()),
                remote: Some(remote_ref.target.clone()),
                hint: "fetch into a clean workspace or create a new checkpoint after reviewing both histories".to_string(),
            }),
            None => imports.push(remote_ref.clone()),
        }
    }
    imports
}

fn remote_git_objects(remote: &dyn SyncRemote, prefix: &str) -> Result<BTreeMap<String, Vec<u8>>> {
    let object_prefix = format!("{prefix}/objects/git/sha1");
    let mut objects = BTreeMap::new();
    for entry in remote.list(&object_prefix)? {
        let oid = remote_git_oid(&object_prefix, &entry.key)?;
        let object = remote.get(&entry.key)?.ok_or_else(|| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("remote listed git object {} but it is missing", entry.key),
            )
            .with_hint("inspect the sync remote before retrying")
        })?;
        objects.insert(oid, object.bytes);
    }
    Ok(objects)
}

fn remote_git_oid(object_prefix: &str, key: &str) -> Result<String> {
    let rest = key
        .strip_prefix(&format!("{object_prefix}/"))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("remote git object key is outside object prefix: {key}"),
            )
            .with_hint("inspect the sync remote before retrying")
        })?;
    let Some((fanout, tail)) = rest.split_once('/') else {
        return Err(Error::new(
            ErrorCode::StoreCorrupt,
            format!("remote git object key has invalid fanout: {key}"),
        )
        .with_hint("inspect the sync remote before retrying"));
    };
    let oid = format!("{fanout}{tail}");
    if fanout.len() != 2 || tail.len() != 38 || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::new(
            ErrorCode::StoreCorrupt,
            format!("remote git object key has invalid object id: {key}"),
        )
        .with_hint("inspect the sync remote before retrying"));
    }
    Ok(oid)
}

fn remote_cas_blobs(remote: &dyn SyncRemote, prefix: &str) -> Result<BTreeMap<String, Vec<u8>>> {
    let blob_prefix = format!("{prefix}/objects/blobs/blake3");
    let mut blobs = BTreeMap::new();
    for entry in remote.list(&blob_prefix)? {
        let hash = remote_cas_hash(&blob_prefix, &entry.key)?;
        let object = remote.get(&entry.key)?.ok_or_else(|| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("remote listed CAS blob {} but it is missing", entry.key),
            )
            .with_hint("inspect the sync remote before retrying")
        })?;
        blobs.insert(hash, object.bytes);
    }
    Ok(blobs)
}

fn remote_cas_hash(blob_prefix: &str, key: &str) -> Result<String> {
    let hash = key
        .strip_prefix(&format!("{blob_prefix}/"))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("remote CAS key is outside blob prefix: {key}"),
            )
            .with_hint("inspect the sync remote before retrying")
        })?;
    if hash.len() != 64 || hash.contains('/') || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::new(
            ErrorCode::StoreCorrupt,
            format!("remote CAS key has invalid BLAKE3 hash: {key}"),
        )
        .with_hint("inspect the sync remote before retrying"));
    }
    Ok(hash.to_string())
}

fn verify_git_object_batch(objects: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    if objects.is_empty() {
        return Ok(());
    }
    let tmp = tempfile::tempdir()?;
    let git_dir = tmp.path().join("verify.git");
    let init = clean_git_command()
        .args(["init", "--bare", "-q"])
        .arg(&git_dir)
        .output()
        .map_err(|e| {
            Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
        })?;
    if !init.status.success() {
        return Err(Error::new(
            ErrorCode::GitFailed,
            format!(
                "git init failed while verifying sync objects: {}",
                String::from_utf8_lossy(&init.stderr).trim()
            ),
        ));
    }
    for (oid, bytes) in objects {
        write_loose_object_bytes(&git_dir, oid, bytes)?;
    }
    for oid in objects.keys() {
        let output = clean_git_command()
            .arg("--git-dir")
            .arg(&git_dir)
            .args(["cat-file", "-e", oid])
            .output()
            .map_err(|e| {
                Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
            })?;
        if !output.status.success() {
            return Err(Error::new(
                ErrorCode::StoreCorrupt,
                format!("remote git object {oid} does not verify"),
            )
            .with_hint("inspect the sync remote before retrying"));
        }
    }
    Ok(())
}

fn write_local_git_object(git_dir: &Path, oid: &str, bytes: &[u8]) -> Result<PutOutcome> {
    let path = loose_object_path(git_dir, oid)?;
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            let existing = std::fs::read(&path)?;
            if existing == bytes {
                return Ok(PutOutcome::AlreadyExists);
            }
            return Err(Error::new(
                ErrorCode::StoreCorrupt,
                format!("local shadow git object {oid} exists with different bytes"),
            )
            .with_hint("run `asp doctor`; preserve the workspace before retrying sync"));
        }
        Ok(_) => {
            return Err(Error::new(
                ErrorCode::StoreCorrupt,
                format!("local shadow git object path {oid} is not a regular file"),
            )
            .with_hint("run `asp doctor`; preserve the workspace before retrying sync"));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    write_loose_object_bytes(git_dir, oid, bytes)?;
    Ok(PutOutcome::Created)
}

fn write_loose_object_bytes(git_dir: &Path, oid: &str, bytes: &[u8]) -> Result<()> {
    let path = loose_object_path(git_dir, oid)?;
    let parent = path.parent().ok_or_else(|| {
        Error::new(
            ErrorCode::Io,
            format!("git object {oid} has no parent directory"),
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| {
        Error::new(
            ErrorCode::Io,
            format!("temp git object in {}: {e}", parent.display()),
        )
        .with_source(e)
    })?;
    {
        use std::io::Write;
        tmp.write_all(bytes)?;
    }
    tmp.as_file().sync_data()?;
    match tmp.persist_noclobber(&path) {
        Ok(_) => {
            let _ = sync_dir(parent);
            Ok(())
        }
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = std::fs::symlink_metadata(&path)?;
            if meta.is_file() && !meta.file_type().is_symlink() {
                Ok(())
            } else {
                Err(Error::new(
                    ErrorCode::StoreCorrupt,
                    format!("git object path {oid} appeared as a non-file"),
                )
                .with_hint("inspect the workspace or sync remote before retrying"))
            }
        }
        Err(e) => {
            let error = e.error;
            Err(Error::new(
                ErrorCode::Io,
                format!("publish git object {}: {error}", path.display()),
            )
            .with_source(error))
        }
    }
}

fn loose_object_path(git_dir: &Path, oid: &str) -> Result<PathBuf> {
    if oid.len() != 40 || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::new(
            ErrorCode::StoreCorrupt,
            format!("invalid git object id: {oid}"),
        )
        .with_hint("inspect the sync remote before retrying"));
    }
    Ok(git_dir.join("objects").join(&oid[..2]).join(&oid[2..]))
}

fn write_local_cas_blob(cas_dir: &Path, hash: &str, bytes: &[u8]) -> Result<PutOutcome> {
    let actual = blake3::hash(bytes).to_hex().to_string();
    if actual != hash {
        return Err(Error::new(
            ErrorCode::StoreCorrupt,
            format!("remote CAS blob {hash} is corrupt (got {actual})"),
        )
        .with_hint("inspect the sync remote before retrying"));
    }
    let path = cas_dir.join(hash);
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            let local = blobs::hash_file(&path)?;
            if local == hash {
                return Ok(PutOutcome::AlreadyExists);
            }
            return Err(Error::new(
                ErrorCode::StoreCorrupt,
                format!("local CAS blob {hash} exists with different bytes"),
            )
            .with_hint("run `asp doctor --deep`; preserve the workspace before retrying sync"));
        }
        Ok(_) => {
            return Err(Error::new(
                ErrorCode::StoreCorrupt,
                format!("local CAS blob path {hash} is not a regular file"),
            )
            .with_hint("run `asp doctor --deep`; preserve the workspace before retrying sync"));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    std::fs::create_dir_all(cas_dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(cas_dir).map_err(|e| {
        Error::new(
            ErrorCode::Io,
            format!("temp CAS blob in {}: {e}", cas_dir.display()),
        )
        .with_source(e)
    })?;
    {
        use std::io::Write;
        tmp.write_all(bytes)?;
    }
    tmp.as_file().sync_data()?;
    match tmp.persist_noclobber(&path) {
        Ok(_) => {
            let _ = sync_dir(cas_dir);
            Ok(PutOutcome::Created)
        }
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            let local = blobs::hash_file(&path)?;
            if local == hash {
                Ok(PutOutcome::AlreadyExists)
            } else {
                Err(Error::new(
                    ErrorCode::StoreCorrupt,
                    format!("local CAS blob {hash} appeared with different bytes"),
                )
                .with_hint("run `asp doctor --deep`; preserve the workspace before retrying sync"))
            }
        }
        Err(e) => {
            let error = e.error;
            Err(Error::new(
                ErrorCode::Io,
                format!("publish CAS blob {}: {error}", path.display()),
            )
            .with_source(error))
        }
    }
}

fn ensure_checkpoint_object(shadow: &Shadow, seq: u64, target: &str) -> Result<()> {
    let rev = format!("{target}^{{commit}}");
    if shadow.run(&["cat-file", "-e", &rev]).is_ok() {
        return Ok(());
    }
    Err(Error::new(
        ErrorCode::StoreCorrupt,
        format!("remote checkpoint #{seq} points at missing commit {target}"),
    )
    .with_hint("rerun `asp sync push` from the source workspace, then retry fetch"))
}

fn ensure_git_object(shadow: &Shadow, label: &str, seq: u64, target: &str) -> Result<()> {
    if shadow.run(&["cat-file", "-e", target]).is_ok() {
        return Ok(());
    }
    Err(Error::new(
        ErrorCode::StoreCorrupt,
        format!("remote {label} #{seq} points at missing object {target}"),
    )
    .with_hint("rerun `asp sync push` from the source workspace, then retry fetch"))
}

fn sync_ref_fields(bytes: &[u8], key: &str) -> Result<(u64, String)> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| {
        Error::new(
            ErrorCode::StoreCorrupt,
            format!("remote ref {key} is not valid JSON: {e}"),
        )
        .with_hint("inspect the sync remote before retrying")
    })?;
    let seq = value
        .get("seq")
        .and_then(|seq| seq.as_u64())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("remote ref {key} is missing numeric seq"),
            )
            .with_hint("inspect the sync remote before retrying")
        })?;
    let target = value
        .get("target")
        .and_then(|target| target.as_str())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("remote ref {key} is missing string target"),
            )
            .with_hint("inspect the sync remote before retrying")
        })?;
    Ok((seq, target.to_string()))
}

fn clean_git_command() -> Command {
    let mut cmd = Command::new("git");
    cmd.env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
    cmd
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .read(true)
        .open(path)?
        .sync_all()
}

fn sanitize_name(label: &str) -> String {
    sanitize_component(label, "fork")
}

fn sanitize_component(label: &str, fallback: &str) -> String {
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
        fallback.to_string()
    } else {
        s
    }
}

fn stats_operation(entry: &Entry) -> StatsOperation {
    StatsOperation {
        op: entry.op.clone(),
        ts: entry.ts.clone(),
        seq: entry.seq,
        duration_ms: entry.duration_ms,
        files_changed: entry.files_changed,
        message: entry.message.clone(),
    }
}

struct DiagnosticsRedactor {
    root: PathBuf,
    workspace_parent: Option<PathBuf>,
    home: Option<String>,
    include_paths: bool,
}

impl DiagnosticsRedactor {
    fn new(root: &Path, include_paths: bool) -> Self {
        Self {
            root: root.to_path_buf(),
            workspace_parent: root.parent().map(Path::to_path_buf),
            home: std::env::var_os("HOME").map(|h| h.to_string_lossy().to_string()),
            include_paths,
        }
    }

    fn path(&self, path: &Path) -> String {
        if self.include_paths {
            return self.text(&path.display().to_string());
        }
        if path == self.root {
            return "<workspace-root>".to_string();
        }
        if let Ok(rel) = path.strip_prefix(&self.root) {
            return format!("<workspace-root>/{}", rel.display());
        }
        if self
            .workspace_parent
            .as_ref()
            .is_some_and(|parent| path.starts_with(parent))
        {
            return "<workspace-sibling>".to_string();
        }
        "<redacted-path>".to_string()
    }

    fn text(&self, input: &str) -> String {
        let mut text = input.to_string();
        if !self.include_paths {
            text = text.replace(&self.root.display().to_string(), "<workspace-root>");
            if let Some(parent) = &self.workspace_parent {
                text = text.replace(&parent.display().to_string(), "<workspace-parent>");
            }
            if let Some(home) = &self.home {
                if !home.is_empty() {
                    text = text.replace(home, "<home>");
                }
            }
        }
        redact_secret_tokens(&text)
    }
}

fn redact_secret_tokens(input: &str) -> String {
    input
        .split_whitespace()
        .map(redact_secret_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_secret_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    if looks_like_standalone_secret(&lower) {
        return "<redacted-token>".to_string();
    }
    const SECRET_KEYS: &[&str] = &[
        "access_token",
        "api_key",
        "apikey",
        "auth",
        "credential",
        "passwd",
        "password",
        "refresh_token",
        "secret",
        "token",
    ];
    if SECRET_KEYS.iter().any(|key| lower.contains(key)) {
        if let Some(pos) = token.find('=').or_else(|| token.find(':')) {
            return format!("{}<redacted>", &token[..=pos]);
        }
    }
    token.to_string()
}

fn looks_like_standalone_secret(lower: &str) -> bool {
    (lower.starts_with("sk-") && lower.len() > 12)
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
}

fn blob_stats(blobs_dir: &Path) -> Result<(u64, u64)> {
    let mut count = 0u64;
    let mut bytes = 0u64;
    if !blobs_dir.exists() {
        return Ok((0, 0));
    }
    for entry in std::fs::read_dir(blobs_dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(".tmp-") {
            continue;
        }
        let md = entry.metadata()?;
        if md.is_file() {
            count += 1;
            bytes += md.len();
        }
    }
    Ok((count, bytes))
}

fn dir_file_bytes(root: &Path) -> Result<u64> {
    let mut bytes = 0u64;
    for entry in WalkDir::new(root) {
        let entry = entry.map_err(|e| {
            Error::new(
                ErrorCode::Io,
                format!("walk {} while computing store stats: {e}", root.display()),
            )
        })?;
        if entry.file_type().is_file() {
            bytes += entry
                .metadata()
                .map_err(|e| {
                    Error::new(
                        ErrorCode::Io,
                        format!(
                            "stat {} while computing store stats: {e}",
                            entry.path().display()
                        ),
                    )
                })?
                .len();
        }
    }
    Ok(bytes)
}

fn actual_worktree_case(root: &Path, rel: &str) -> Option<String> {
    let mut dir = root.to_path_buf();
    let mut actual_parts = Vec::new();
    for component in Path::new(rel).components() {
        let std::path::Component::Normal(wanted) = component else {
            return None;
        };
        let mut found = None;
        for entry in std::fs::read_dir(&dir).ok()?.filter_map(|entry| entry.ok()) {
            let name = entry.file_name();
            if name.as_os_str() == wanted {
                found = Some(name);
                break;
            }
            if found.is_none()
                && name
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&wanted.to_string_lossy())
            {
                found = Some(name);
            }
        }
        let name = found?;
        dir.push(&name);
        actual_parts.push(name.to_string_lossy().to_string());
    }
    Some(actual_parts.join("/"))
}

fn policy_violation(message: impl Into<String>, hint: impl Into<String>) -> Error {
    Error::new(ErrorCode::PolicyViolation, message).with_hint(hint)
}

fn policy_path_matches(pattern: &str, path: &str) -> bool {
    let pattern_parts: Vec<&str> = pattern.split('/').filter(|part| !part.is_empty()).collect();
    let path_parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    glob_parts_match(&pattern_parts, &path_parts)
}

fn glob_parts_match(pattern: &[&str], path: &[&str]) -> bool {
    match (pattern.split_first(), path.split_first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some((&"**", rest)), _) => {
            glob_parts_match(rest, path)
                || path
                    .split_first()
                    .is_some_and(|(_, tail)| glob_parts_match(pattern, tail))
        }
        (Some((part, rest)), Some((path_part, path_rest))) => {
            glob_segment_match(part, path_part) && glob_parts_match(rest, path_rest)
        }
        (Some(_), None) => false,
    }
}

fn glob_segment_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut star_text = 0usize;
    while ti < text_chars.len() {
        if pi < pattern_chars.len() && pattern_chars[pi] == text_chars[ti] {
            pi += 1;
            ti += 1;
        } else if pi < pattern_chars.len() && pattern_chars[pi] == '*' {
            star = Some(pi);
            pi += 1;
            star_text = ti;
        } else if let Some(star_index) = star {
            pi = star_index + 1;
            star_text += 1;
            ti = star_text;
        } else {
            return false;
        }
    }
    while pi < pattern_chars.len() && pattern_chars[pi] == '*' {
        pi += 1;
    }
    pi == pattern_chars.len()
}

fn summarize_diff(rows: &[DiffRow]) -> DiffSummary {
    let files = rows.len() as u64;
    let insertions = rows.iter().filter_map(|row| row.insertions).sum();
    let deletions = rows.iter().filter_map(|row| row.deletions).sum();

    DiffSummary {
        files,
        insertions,
        deletions,
        by_path: diff_buckets(rows, |row| diff_path_group(&row.path)),
        by_language: diff_buckets(rows, |row| diff_language(&row.path)),
        by_change_type: diff_buckets(rows, |row| diff_change_type(&row.status)),
    }
}

fn diff_buckets<F>(rows: &[DiffRow], label: F) -> Vec<DiffSummaryBucket>
where
    F: Fn(&DiffRow) -> String,
{
    let mut buckets: BTreeMap<String, DiffSummaryBucket> = BTreeMap::new();
    for row in rows {
        let name = label(row);
        let bucket = buckets
            .entry(name.clone())
            .or_insert_with(|| DiffSummaryBucket {
                name,
                files: 0,
                insertions: 0,
                deletions: 0,
            });
        bucket.files += 1;
        bucket.insertions += row.insertions.unwrap_or(0);
        bucket.deletions += row.deletions.unwrap_or(0);
    }
    buckets.into_values().collect()
}

fn diff_path_group(path: &str) -> String {
    match path.split('/').next().filter(|part| !part.is_empty()) {
        Some(first) if path.contains('/') => format!("{first}/"),
        Some(_) | None => "(root)".to_string(),
    }
}

fn diff_language(path: &str) -> String {
    let Some(ext) = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
    else {
        return "Other".to_string();
    };

    match ext.as_str() {
        "rs" => "Rust",
        "py" => "Python",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "md" | "mdx" => "Markdown",
        "toml" => "TOML",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "sh" | "bash" | "zsh" => "Shell",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" => "CSS",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "c" | "h" => "C/C++",
        "cc" | "cpp" | "cxx" | "hpp" => "C++",
        "txt" => "Text",
        other => other,
    }
    .to_string()
}

fn diff_change_type(status: &str) -> String {
    match status.chars().next().unwrap_or('M') {
        'A' => "added",
        'D' => "deleted",
        'M' => "modified",
        'T' => "type_changed",
        'R' => "renamed",
        'C' => "copied",
        _ => "other",
    }
    .to_string()
}

fn fork_review_signals(rows: &[DiffRow], tests_passed: Option<bool>) -> ForkReviewSignals {
    let risk_markers = fork_risk_markers(rows);
    let risk_score = risk_markers
        .iter()
        .map(|marker| risk_marker_score(&marker.severity))
        .sum();

    ForkReviewSignals {
        tests_passed,
        files_touched: rows.len() as u64,
        line_churn: rows
            .iter()
            .map(|row| row.insertions.unwrap_or(0) + row.deletions.unwrap_or(0))
            .sum(),
        risk_score,
        risk_markers,
    }
}

fn fork_risk_markers(rows: &[DiffRow]) -> Vec<ForkRiskMarker> {
    let mut markers = Vec::new();
    for row in rows {
        let path = row.path.as_str();
        if path.starts_with(".git/") {
            markers.push(risk_marker(
                "git_metadata",
                "high",
                path,
                "touches user git metadata",
            ));
        }
        if path.starts_with(".github/workflows/") {
            markers.push(risk_marker(
                "ci_workflow",
                "high",
                path,
                "changes CI workflow definitions",
            ));
        }
        if is_dependency_manifest(path) {
            markers.push(risk_marker(
                "dependency_manifest",
                "medium",
                path,
                "changes dependency or package manager inputs",
            ));
        }
        if is_env_or_secret_path(path) {
            markers.push(risk_marker(
                "credential_or_env",
                "medium",
                path,
                "changes environment or credential-like files",
            ));
        }
        let churn = row.insertions.unwrap_or(0) + row.deletions.unwrap_or(0);
        if churn >= 500 {
            markers.push(risk_marker(
                "large_churn",
                "medium",
                path,
                "changes at least 500 lines in one file",
            ));
        }
        if row.status.starts_with('D') {
            markers.push(risk_marker(
                "deletion",
                "low",
                path,
                "deletes a tracked path",
            ));
        }
    }
    markers
}

fn risk_marker(kind: &str, severity: &str, path: &str, message: &str) -> ForkRiskMarker {
    ForkRiskMarker {
        kind: kind.to_string(),
        severity: severity.to_string(),
        path: path.to_string(),
        message: message.to_string(),
    }
}

fn risk_marker_score(severity: &str) -> u64 {
    match severity {
        "high" => 50,
        "medium" => 20,
        "low" => 5,
        _ => 1,
    }
}

fn is_dependency_manifest(path: &str) -> bool {
    let Some(file_name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        file_name,
        "Cargo.toml"
            | "Cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "go.mod"
            | "go.sum"
            | "Gemfile"
            | "Gemfile.lock"
            | "requirements.txt"
            | "pyproject.toml"
            | "poetry.lock"
    )
}

fn is_env_or_secret_path(path: &str) -> bool {
    Path::new(path)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .any(|component| {
            component == ".env"
                || component.starts_with(".env.")
                || component.eq_ignore_ascii_case("secrets.toml")
                || component.eq_ignore_ascii_case("secrets.json")
        })
}

fn paths_from_nul(raw: &[u8]) -> Vec<String> {
    raw.split(|&b| b == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).to_string())
        .collect()
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

fn validate_user_branch_name(repo_dir: &Path, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .arg("-C")
        .arg(repo_dir)
        .args(["check-ref-format", "--branch", branch])
        .output()
        .map_err(|e| {
            Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
        })?;
    if output.status.success() {
        return Ok(());
    }

    Err(Error::new(
        ErrorCode::InvalidBranch,
        format!("invalid branch name '{branch}'"),
    )
    .with_hint("pass a normal branch name, for example `asp promote <fork> --branch asp/<fork>`"))
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

fn remove_fork_path(path: &Path) -> Result<()> {
    let md = match std::fs::symlink_metadata(path) {
        Ok(md) => md,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(Error::new(
                ErrorCode::Io,
                format!("inspect fork path {}: {e}", path.display()),
            ));
        }
    };
    if md.file_type().is_symlink() || md.is_file() {
        std::fs::remove_file(path)?;
    } else {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
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
