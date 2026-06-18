//! `.asp/` store layout, discovery, locking, and atomic file helpers.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

pub const ASP_DIR: &str = ".asp";
pub const FORMAT_VERSION: u32 = 1;

/// Resolved paths inside one workspace's `.asp` directory.
#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
    pub asp: PathBuf,
}

impl Layout {
    pub fn new(root: PathBuf) -> Self {
        let asp = root.join(ASP_DIR);
        Self { root, asp }
    }

    pub fn format_version(&self) -> PathBuf {
        self.asp.join("format-version")
    }
    pub fn workspace_json(&self) -> PathBuf {
        self.asp.join("workspace.json")
    }
    pub fn config_toml(&self) -> PathBuf {
        self.asp.join("config.toml")
    }
    pub fn policy_toml(&self) -> PathBuf {
        self.asp.join("policy.toml")
    }
    pub fn lock_file(&self) -> PathBuf {
        self.asp.join("lock")
    }
    pub fn shadow_git(&self) -> PathBuf {
        self.asp.join("shadow.git")
    }
    pub fn shadow_index(&self) -> PathBuf {
        self.asp.join("shadow.index")
    }
    pub fn journal(&self) -> PathBuf {
        self.asp.join("journal.jsonl")
    }
    pub fn file_state_index(&self) -> PathBuf {
        self.asp.join("file-state.json")
    }
    pub fn blobs(&self) -> PathBuf {
        self.asp.join("blobs")
    }
    pub fn forks_json(&self) -> PathBuf {
        self.asp.join("forks.json")
    }
}

/// Identity record (`.asp/workspace.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMeta {
    pub id: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentRef {
    pub workspace_id: String,
    pub fork_point_seq: u64,
    pub fork_name: String,
}

/// Fork registry (`.asp/forks.json`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ForkRegistry {
    pub v: u32,
    pub forks: Vec<ForkRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkRecord {
    pub name: String,
    /// Absolute path of the fork directory.
    pub path: PathBuf,
    pub created_at: String,
    pub fork_point_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub status: ForkStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkStatus {
    /// Registered before the clone starts; flipped to Active after the
    /// post-clone identity fixup. A Pending entry that persists marks a torn
    /// clone — deterministic, so doctor can clean it without heuristics.
    Pending,
    Active,
    Promoted,
    Discarded,
}

/// Validate a store-supplied relative path before joining it onto the
/// workspace root. Rejects absolute paths and any non-normal component
/// (`..`, `.`, prefixes) — a corrupt or malicious `.asp` store must never
/// be able to direct writes or deletes outside the workspace.
pub fn safe_rel_path(root: &Path, rel: &str) -> Result<PathBuf> {
    use std::path::Component;
    let p = Path::new(rel);
    let valid = !rel.is_empty()
        && !p.is_absolute()
        && p.components().all(|c| matches!(c, Component::Normal(_)));
    if !valid {
        return Err(Error::new(
            ErrorCode::StoreCorrupt,
            format!("unsafe path in workspace store: {rel:?}"),
        )
        .with_hint("the .asp store may be corrupt or tampered with; run `asp doctor`"));
    }
    if let Some(reason) = windows_path_violation(rel) {
        return Err(unsafe_store_path(
            rel,
            &format!("Windows-incompatible path: {reason}"),
        ));
    }
    Ok(root.join(p))
}

/// Report why a slash-separated workspace-relative path cannot round-trip on
/// native Windows. The store format is OS-neutral, so checkpoint metadata must
/// not admit names that become device aliases, alternate streams, or unopenable
/// paths when the same workspace is recovered on Windows later.
pub fn windows_path_violation(rel: &str) -> Option<String> {
    const MAX_COMPONENT_UTF16: usize = 255;
    const MAX_EXTENDED_REL_UTF16: usize = 32_000;

    if rel.is_empty() {
        return Some("path is empty".to_string());
    }
    if rel.encode_utf16().count() > MAX_EXTENDED_REL_UTF16 {
        return Some(format!(
            "path exceeds {MAX_EXTENDED_REL_UTF16} UTF-16 code units"
        ));
    }
    if rel.contains('\\') {
        return Some("contains a Windows path separator".to_string());
    }

    for component in rel.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Some("contains an empty, dot, or parent component".to_string());
        }
        if component.encode_utf16().count() > MAX_COMPONENT_UTF16 {
            return Some(format!(
                "component {component:?} exceeds {MAX_COMPONENT_UTF16} UTF-16 code units"
            ));
        }
        if component.ends_with(' ') || component.ends_with('.') {
            return Some(format!("component {component:?} ends with a space or dot"));
        }
        if component.chars().any(windows_forbidden_char) {
            return Some(format!(
                "component {component:?} contains a Windows-forbidden character"
            ));
        }
        if windows_reserved_device_name(component) {
            return Some(format!(
                "component {component:?} is a reserved Windows device name"
            ));
        }
    }

    None
}

fn windows_forbidden_char(ch: char) -> bool {
    matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') || ch.is_control()
}

fn windows_reserved_device_name(component: &str) -> bool {
    let base = component
        .split_once('.')
        .map(|(base, _)| base)
        .unwrap_or(component)
        .to_uppercase();
    matches!(
        base.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "CONIN$"
            | "CONOUT$"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "COM\u{00B9}"
            | "COM\u{00B2}"
            | "COM\u{00B3}"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
            | "LPT\u{00B9}"
            | "LPT\u{00B2}"
            | "LPT\u{00B3}"
    )
}

/// Validate a store-supplied path before materializing file contents into the
/// worktree. This is stricter than `safe_rel_path`: existing parent components
/// must be real directories, not symlinks that could redirect writes outside
/// the workspace or into asp's own metadata.
pub fn safe_worktree_write_path(root: &Path, rel: &str) -> Result<PathBuf> {
    use std::path::Component;

    let abs = safe_rel_path(root, rel)?;
    let mut cur = root.to_path_buf();
    let mut components = Path::new(rel).components().peekable();
    let mut first = true;

    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            unreachable!("safe_rel_path rejects non-normal components");
        };
        if first && (name == ASP_DIR || name == ".git") {
            return Err(unsafe_store_path(rel, "reserved workspace metadata path"));
        }
        first = false;

        cur.push(name);
        let is_leaf = components.peek().is_none();
        match std::fs::symlink_metadata(&cur) {
            Ok(md) if md.file_type().is_symlink() => {
                return Err(unsafe_store_path(rel, "symlink component"));
            }
            Ok(md) if !is_leaf && md.is_dir() => {}
            Ok(md) if is_leaf && md.is_file() => {}
            Ok(_) if is_leaf => return Err(unsafe_store_path(rel, "non-file destination")),
            Ok(_) => return Err(unsafe_store_path(rel, "non-directory parent")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => {
                return Err(Error::new(
                    ErrorCode::Io,
                    format!("inspect workspace path {}: {e}", cur.display()),
                ));
            }
        }
    }

    Ok(abs)
}

fn unsafe_store_path(rel: &str, reason: &str) -> Error {
    Error::new(
        ErrorCode::StoreCorrupt,
        format!("unsafe path in workspace store: {rel:?} ({reason})"),
    )
    .with_hint("the .asp store may be corrupt or tampered with; run `asp doctor`")
}

/// Walk up from `start` to find a workspace root (a dir containing `.asp`).
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(ASP_DIR).join("format-version").is_file() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// Write bytes to `path` atomically (temp file + rename, same directory).
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| {
        Error::new(
            ErrorCode::Io,
            format!("no parent dir for {}", path.display()),
        )
    })?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| {
        Error::new(
            ErrorCode::Io,
            format!("temp file in {}: {e}", dir.display()),
        )
        .with_source(e)
    })?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_data()?;
    tmp.persist(path).map_err(|e| {
        Error::new(
            ErrorCode::Io,
            format!("atomic rename to {}: {e}", path.display()),
        )
    })?;
    Ok(())
}

pub fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_vec_pretty(value).map_err(|e| {
        Error::new(ErrorCode::Io, format!("encode {}: {e}", path.display())).with_source(e)
    })?;
    atomic_write(path, &json)
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|e| {
        Error::new(
            ErrorCode::StoreCorrupt,
            format!("corrupt {}: {e}", path.display()),
        )
        .with_hint("run `asp doctor` to diagnose and repair the workspace store")
    })
}

/// Exclusive advisory lock over the workspace for mutations. Released on drop.
#[derive(Debug)]
pub struct StoreLock {
    file: File,
}

impl StoreLock {
    /// Acquire with a short retry — auto-checkpoint hooks race each other
    /// and must not silently drop work on transient contention.
    pub fn acquire_with_retry(layout: &Layout) -> Result<Self> {
        let mut last = None;
        for _ in 0..5 {
            match Self::acquire(layout) {
                Ok(lock) => return Ok(lock),
                Err(e) => {
                    last = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(120));
                }
            }
        }
        Err(last.expect("at least one attempt"))
    }

    pub fn acquire(layout: &Layout) -> Result<Self> {
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(layout.lock_file())?;
        file.try_lock_exclusive().map_err(|_| {
            Error::new(
                ErrorCode::Locked,
                "another asp process is modifying this workspace",
            )
            .with_hint("wait for it to finish and retry; if a process crashed, the lock clears automatically")
        })?;
        Ok(Self { file })
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.json");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
    }

    #[test]
    fn find_root_walks_up() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("proj");
        let deep = root.join("a/b/c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(root.join(".asp")).unwrap();
        std::fs::write(root.join(".asp/format-version"), "1").unwrap();
        assert_eq!(find_root(&deep).unwrap(), root);
        assert!(find_root(d.path()).is_none());
    }

    #[test]
    fn lock_is_exclusive() {
        let d = tempfile::tempdir().unwrap();
        let layout = Layout::new(d.path().to_path_buf());
        std::fs::create_dir_all(&layout.asp).unwrap();
        let l1 = StoreLock::acquire(&layout).unwrap();
        let err = StoreLock::acquire(&layout).unwrap_err();
        assert_eq!(err.code, ErrorCode::Locked);
        drop(l1);
        StoreLock::acquire(&layout).unwrap();
    }

    #[test]
    fn windows_path_violation_rejects_reserved_and_unopenable_names() {
        for path in [
            "CON",
            "dir/NUL.txt",
            "COM1.log",
            "LPT9",
            "name.",
            "name ",
            "src/main.rs:Zone.Identifier",
            "src\\main.rs",
            "bad<name>",
            "line\nbreak",
        ] {
            assert!(
                windows_path_violation(path).is_some(),
                "{path:?} should be rejected"
            );
        }
    }

    #[test]
    fn windows_path_violation_rejects_overlong_components() {
        let path = format!("src/{}.txt", "a".repeat(256));
        assert!(windows_path_violation(&path)
            .unwrap()
            .contains("exceeds 255 UTF-16 code units"));
    }

    #[test]
    fn windows_path_violation_allows_portable_slash_paths() {
        for path in [
            "src/main.rs",
            ".env.example",
            "docs/README.v1.md",
            "unicode/cafe\u{301}.txt",
        ] {
            assert_eq!(windows_path_violation(path), None, "{path:?}");
        }
    }
}
