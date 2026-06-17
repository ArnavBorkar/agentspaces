//! Sync remote building blocks.
//!
//! The first implementation target is a local filesystem remote. It gives the
//! sync protocol deterministic tests before object-storage backends exist.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::error::{Error, ErrorCode, Result};

const LOCAL_REMOTE_LOCK: &str = ".asp-local-remote.lock";

#[derive(Debug, Clone)]
pub struct LocalRemote {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteObject {
    pub bytes: Vec<u8>,
    pub version: RemoteVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub key: String,
    pub bytes: u64,
    pub version: RemoteVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteVersion(String);

impl RemoteVersion {
    pub fn token(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    Created,
    Replaced,
    AlreadyExists,
}

pub trait SyncRemote {
    fn get(&self, key: &str) -> Result<Option<RemoteObject>>;
    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>>;
    fn put_immutable(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome>;
    fn put_if_match(
        &self,
        key: &str,
        bytes: &[u8],
        expected: Option<&RemoteVersion>,
    ) -> Result<PutOutcome>;
}

impl LocalRemote {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        std::fs::create_dir_all(root.as_ref())?;
        let root = root.as_ref().canonicalize()?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn get(&self, key: &str) -> Result<Option<RemoteObject>> {
        <Self as SyncRemote>::get(self, key)
    }

    pub fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>> {
        <Self as SyncRemote>::list(self, prefix)
    }

    pub fn put_immutable(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        <Self as SyncRemote>::put_immutable(self, key, bytes)
    }

    pub fn put_if_match(
        &self,
        key: &str,
        bytes: &[u8],
        expected: Option<&RemoteVersion>,
    ) -> Result<PutOutcome> {
        <Self as SyncRemote>::put_if_match(self, key, bytes, expected)
    }

    fn get_unlocked(&self, key: &str) -> Result<Option<RemoteObject>> {
        let path = self.key_path(key)?;
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if meta.file_type().is_symlink() {
            return Err(remote_corrupt(format!(
                "remote key is a symlink: {}",
                path.display()
            )));
        }
        if !meta.is_file() {
            return Err(remote_corrupt(format!("remote key '{key}' is not a file")));
        }
        let bytes = std::fs::read(path)?;
        Ok(Some(RemoteObject {
            version: version_for(&bytes),
            bytes,
        }))
    }

    fn list_unlocked(&self, prefix: &str) -> Result<Vec<RemoteEntry>> {
        let prefix = normalize_prefix(prefix)?;
        let path = if prefix.is_empty() {
            self.root.clone()
        } else {
            self.key_path(&prefix)?
        };
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        if meta.file_type().is_symlink() {
            return Err(remote_corrupt(format!(
                "remote key is a symlink: {}",
                path.display()
            )));
        }

        let mut entries = Vec::new();
        for entry in walkdir::WalkDir::new(&path).follow_links(false) {
            let entry = entry.map_err(|e| {
                Error::new(
                    ErrorCode::Io,
                    format!("read remote {}: {e}", path.display()),
                )
            })?;
            if entry.path() == path {
                continue;
            }
            if entry.file_type().is_symlink() {
                return Err(remote_corrupt(format!(
                    "remote key contains a symlink: {}",
                    entry.path().display()
                )));
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().strip_prefix(&self.root).map_err(|e| {
                Error::new(
                    ErrorCode::Io,
                    format!("remote path escaped {}: {e}", self.root.display()),
                )
                .with_source(e)
            })?;
            let key = rel_key(rel)?;
            if key == LOCAL_REMOTE_LOCK {
                continue;
            }
            let bytes = std::fs::read(entry.path())?;
            entries.push(RemoteEntry {
                key,
                bytes: bytes.len() as u64,
                version: version_for(&bytes),
            });
        }
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(entries)
    }

    fn put_immutable_unlocked(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        let path = self.key_path(key)?;
        ensure_parent_dirs(&self.root, key)?;

        match self.get_unlocked(key)? {
            Some(existing) if existing.bytes == bytes => return Ok(PutOutcome::AlreadyExists),
            Some(_) => {
                return Err(remote_corrupt(format!(
                    "remote immutable key '{key}' already exists with different bytes"
                )));
            }
            None => {}
        }

        let parent = path.parent().ok_or_else(|| {
            Error::new(
                ErrorCode::Io,
                format!("remote key '{key}' has no parent directory"),
            )
        })?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| {
            Error::new(
                ErrorCode::Io,
                format!("temp remote object in {}: {e}", parent.display()),
            )
            .with_source(e)
        })?;
        tmp.write_all(bytes)?;
        tmp.as_file().sync_data()?;

        match tmp.persist_noclobber(&path) {
            Ok(_) => {
                let _ = sync_dir(parent);
                Ok(PutOutcome::Created)
            }
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = std::fs::read(&path)?;
                if existing == bytes {
                    Ok(PutOutcome::AlreadyExists)
                } else {
                    Err(remote_corrupt(format!(
                        "remote immutable key '{key}' appeared with different bytes"
                    )))
                }
            }
            Err(e) => {
                let error = e.error;
                Err(Error::new(
                    ErrorCode::Io,
                    format!("publish remote object {}: {error}", path.display()),
                )
                .with_source(error))
            }
        }
    }

    fn put_if_match_unlocked(
        &self,
        key: &str,
        bytes: &[u8],
        expected: Option<&RemoteVersion>,
    ) -> Result<PutOutcome> {
        let current = self.get_unlocked(key)?;
        match (current, expected) {
            (None, None) => self.put_immutable_unlocked(key, bytes),
            (None, Some(_)) => Err(sync_conflict(format!(
                "remote key '{key}' is missing; conditional write expected an existing version"
            ))),
            (Some(_), None) => Err(sync_conflict(format!(
                "remote key '{key}' already exists; conditional create expected it to be absent"
            ))),
            (Some(current), Some(expected)) => {
                if &current.version != expected {
                    return Err(sync_conflict(format!(
                        "remote key '{key}' changed before conditional write"
                    )));
                }
                if current.bytes == bytes {
                    return Ok(PutOutcome::AlreadyExists);
                }
                self.replace_existing(key, bytes)?;
                Ok(PutOutcome::Replaced)
            }
        }
    }

    fn replace_existing(&self, key: &str, bytes: &[u8]) -> Result<()> {
        let path = self.key_path(key)?;
        ensure_parent_dirs(&self.root, key)?;
        let parent = path.parent().ok_or_else(|| {
            Error::new(
                ErrorCode::Io,
                format!("remote key '{key}' has no parent directory"),
            )
        })?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| {
            Error::new(
                ErrorCode::Io,
                format!("temp remote object in {}: {e}", parent.display()),
            )
            .with_source(e)
        })?;
        tmp.write_all(bytes)?;
        tmp.as_file().sync_data()?;
        tmp.persist(&path).map_err(|e| {
            Error::new(
                ErrorCode::Io,
                format!("replace remote object {}: {}", path.display(), e.error),
            )
            .with_source(e.error)
        })?;
        let _ = sync_dir(parent);
        Ok(())
    }

    fn with_lock<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let lock_path = self.root.join(LOCAL_REMOTE_LOCK);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)?;
        lock.lock_exclusive().map_err(|e| {
            Error::new(
                ErrorCode::Locked,
                "another asp process is modifying this sync remote",
            )
            .with_hint("wait for it to finish and retry")
            .with_source(e)
        })?;
        let result = f();
        let _ = FileExt::unlock(&lock);
        result
    }

    fn key_path(&self, key: &str) -> Result<PathBuf> {
        let parts = validate_key(key)?;
        let mut path = self.root.clone();
        for part in parts {
            path.push(part);
        }
        Ok(path)
    }
}

impl SyncRemote for LocalRemote {
    fn get(&self, key: &str) -> Result<Option<RemoteObject>> {
        self.get_unlocked(key)
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>> {
        self.list_unlocked(prefix)
    }

    fn put_immutable(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        self.with_lock(|| self.put_immutable_unlocked(key, bytes))
    }

    fn put_if_match(
        &self,
        key: &str,
        bytes: &[u8],
        expected: Option<&RemoteVersion>,
    ) -> Result<PutOutcome> {
        self.with_lock(|| self.put_if_match_unlocked(key, bytes, expected))
    }
}

fn validate_key(key: &str) -> Result<Vec<&str>> {
    if key.is_empty()
        || key.starts_with('/')
        || key.ends_with('/')
        || key.contains('\\')
        || key.as_bytes().contains(&0)
    {
        return Err(invalid_key(key));
    }
    let mut parts = Vec::new();
    for part in key.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(invalid_key(key));
        }
        parts.push(part);
    }
    Ok(parts)
}

fn normalize_prefix(prefix: &str) -> Result<String> {
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        Ok(String::new())
    } else {
        validate_key(prefix)?;
        Ok(prefix.to_string())
    }
}

fn ensure_parent_dirs(root: &Path, key: &str) -> Result<()> {
    let parts = validate_key(key)?;
    let mut dir = root.to_path_buf();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        dir.push(part);
        match std::fs::symlink_metadata(&dir) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(remote_corrupt(format!(
                    "remote parent is a symlink: {}",
                    dir.display()
                )));
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(remote_corrupt(format!(
                    "remote parent is not a directory: {}",
                    dir.display()
                )));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&dir)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn rel_key(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for part in path.components() {
        let std::path::Component::Normal(name) = part else {
            return Err(remote_corrupt(format!(
                "remote path has non-normal component: {}",
                path.display()
            )));
        };
        let name = name.to_str().ok_or_else(|| {
            remote_corrupt(format!("remote path is not UTF-8: {}", path.display()))
        })?;
        parts.push(name);
    }
    Ok(parts.join("/"))
}

fn invalid_key(key: &str) -> Error {
    Error::new(
        ErrorCode::StoreCorrupt,
        format!("invalid remote key: {key:?}"),
    )
    .with_hint("remote keys must be non-empty slash-separated relative paths")
}

fn remote_corrupt(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::StoreCorrupt, message)
        .with_hint("inspect the sync remote before retrying")
}

fn sync_conflict(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::SyncConflict, message)
        .with_hint("fetch the latest remote state, review conflicts, and retry")
}

fn version_for(bytes: &[u8]) -> RemoteVersion {
    RemoteVersion(blake3::hash(bytes).to_hex().to_string())
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    OpenOptions::new().read(true).open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_remote_put_get_and_list_are_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = LocalRemote::open(tmp.path().join("remote")).unwrap();

        assert_eq!(
            remote
                .put_immutable("objects/git/sha1/aa/1111", b"one")
                .unwrap(),
            PutOutcome::Created
        );
        assert_eq!(
            remote
                .put_immutable("objects/git/sha1/aa/1111", b"one")
                .unwrap(),
            PutOutcome::AlreadyExists
        );
        assert_eq!(
            remote
                .put_immutable("objects/blobs/blake3/bbbb", b"two")
                .unwrap(),
            PutOutcome::Created
        );

        let object = remote.get("objects/git/sha1/aa/1111").unwrap().unwrap();
        assert_eq!(object.bytes, b"one");
        assert_eq!(object.version.token(), version_for(b"one").token());

        let keys: Vec<_> = remote
            .list("objects")
            .unwrap()
            .into_iter()
            .map(|entry| (entry.key, entry.bytes))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("objects/blobs/blake3/bbbb".to_string(), 3),
                ("objects/git/sha1/aa/1111".to_string(), 3)
            ]
        );
    }

    #[test]
    fn local_remote_supports_conditional_writes_through_trait() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = LocalRemote::open(tmp.path().join("remote")).unwrap();
        let remote_trait: &dyn SyncRemote = &remote;

        assert_eq!(
            remote_trait
                .put_if_match("refs/head.json", br#"{"seq":1}"#, None)
                .unwrap(),
            PutOutcome::Created
        );
        let v1 = remote_trait.get("refs/head.json").unwrap().unwrap().version;
        assert_eq!(
            remote_trait
                .put_if_match("refs/head.json", br#"{"seq":2}"#, Some(&v1))
                .unwrap(),
            PutOutcome::Replaced
        );
        assert_eq!(
            remote_trait.get("refs/head.json").unwrap().unwrap().bytes,
            br#"{"seq":2}"#
        );

        let stale = remote_trait
            .put_if_match("refs/head.json", br#"{"seq":3}"#, Some(&v1))
            .unwrap_err();
        assert_eq!(stale.code, ErrorCode::SyncConflict);
        assert_eq!(
            remote_trait.get("refs/head.json").unwrap().unwrap().bytes,
            br#"{"seq":2}"#
        );

        let duplicate_create = remote_trait
            .put_if_match("refs/head.json", br#"{"seq":2}"#, None)
            .unwrap_err();
        assert_eq!(duplicate_create.code, ErrorCode::SyncConflict);
    }

    #[test]
    fn local_remote_rejects_conflicting_immutable_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = LocalRemote::open(tmp.path().join("remote")).unwrap();
        remote.put_immutable("refs/head.json", b"one").unwrap();

        let err = remote.put_immutable("refs/head.json", b"two").unwrap_err();
        assert_eq!(err.code, ErrorCode::StoreCorrupt);
        assert_eq!(remote.get("refs/head.json").unwrap().unwrap().bytes, b"one");
    }

    #[test]
    fn local_remote_rejects_escaping_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = LocalRemote::open(tmp.path().join("remote")).unwrap();
        for key in ["", "/abs", "a/", "a//b", "a/./b", "a/../b", "../x", "a\\b"] {
            let err = remote.put_immutable(key, b"x").unwrap_err();
            assert_eq!(err.code, ErrorCode::StoreCorrupt, "{key}");
        }
        assert!(!tmp.path().join("x").exists());
    }

    #[cfg(unix)]
    #[test]
    fn local_remote_rejects_parent_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = LocalRemote::open(tmp.path().join("remote")).unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, remote.root().join("objects")).unwrap();

        let err = remote
            .put_immutable("objects/git/sha1/aa/1111", b"one")
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::StoreCorrupt);
        assert!(!outside.join("git/sha1/aa/1111").exists());
    }
}
