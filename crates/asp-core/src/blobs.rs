//! Large-file sidecar: files over the configured threshold are BLAKE3-hashed
//! into a content-addressed store (`.asp/blobs/<hash>`) via CoW clone
//! (instant, zero-copy on APFS/btrfs/XFS), with a small pointer blob committed
//! to shadow git in their place. Keeps checkpoints fast and the store compact
//! while every byte stays recoverable: pointer files name their CAS entry,
//! and `cp .asp/blobs/<hash> <path>` is the stock-tools runbook.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

/// Pointer file content committed to shadow git at the big file's path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pointer {
    pub asp_ptr: u32,
    pub blake3: String,
    pub size: u64,
}

/// Per-checkpoint pointer manifest, stored as a blob at `refs/asp/meta/<seq>`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub v: u32,
    pub pointers: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Workspace-relative path.
    pub path: String,
    pub blake3: String,
    pub size: u64,
}

/// Cache of currently-known big files (`.asp/bigfiles.json`). Rebuildable.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BigFiles {
    pub v: u32,
    /// path → entry
    pub files: std::collections::BTreeMap<String, BigFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BigFileEntry {
    pub blake3: String,
    pub size: u64,
    /// Quick-check stamp: rehash only when size or mtime changed.
    #[serde(default)]
    pub mtime_ms: i64,
    /// git blob oid of this entry's pointer JSON (cached so captures need
    /// zero per-file hash-object spawns).
    #[serde(default)]
    pub pointer_oid: Option<String>,
}

pub fn pointer_json(entry: &BigFileEntry) -> String {
    serde_json::to_string(&Pointer {
        asp_ptr: 1,
        blake3: entry.blake3.clone(),
        size: entry.size,
    })
    .expect("pointer serializes")
}

pub fn parse_pointer(bytes: &[u8]) -> Option<Pointer> {
    if !bytes.starts_with(b"{\"asp_ptr\":") {
        return None;
    }
    serde_json::from_slice(bytes).ok()
}

/// Streaming BLAKE3 of a file.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Millisecond mtime of a metadata record (0 when unavailable).
pub fn mtime_ms(md: &std::fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Ensure `file` exists in the CAS dir; returns its hash. Dedupes by content.
pub fn store_in_cas(cas_dir: &Path, file: &Path) -> Result<BigFileEntry> {
    let md = file.metadata()?;
    let hash = hash_file(file)?;
    let dst = cas_dir.join(&hash);
    if !dst.exists() {
        std::fs::create_dir_all(cas_dir)?;
        clone_file(file, &dst)?;
    }
    Ok(BigFileEntry {
        blake3: hash,
        size: md.len(),
        mtime_ms: mtime_ms(&md),
        pointer_oid: None,
    })
}

/// Materialize a CAS entry at `dst` (replacing whatever is there).
pub fn restore_from_cas(cas_dir: &Path, hash: &str, dst: &Path) -> Result<()> {
    let src = cas_dir.join(hash);
    if !src.exists() {
        return Err(Error::new(
            ErrorCode::StoreCorrupt,
            format!("missing CAS blob {hash} for {}", dst.display()),
        )
        .with_hint("run `asp doctor`; the original file may still exist in a fork or backup"));
    }
    if dst.exists() {
        std::fs::remove_file(dst)?;
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    clone_file(&src, dst)
}

/// CoW-clone a single file (clonefile on macOS, FICLONE on Linux, copy else).
pub fn clone_file(src: &Path, dst: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let c_src = std::ffi::CString::new(src.as_os_str().as_bytes())
            .map_err(|_| Error::new(ErrorCode::Io, "path contains NUL byte"))?;
        let c_dst = std::ffi::CString::new(dst.as_os_str().as_bytes())
            .map_err(|_| Error::new(ErrorCode::Io, "path contains NUL byte"))?;
        // SAFETY: valid NUL-terminated C strings.
        let rc = unsafe { libc::clonefile(c_src.as_ptr(), c_dst.as_ptr(), 0) };
        if rc == 0 {
            return Ok(());
        }
        // Cross-volume or non-APFS: fall through to byte copy.
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        if let (Ok(fin), Ok(fout)) = (std::fs::File::open(src), std::fs::File::create(dst)) {
            // SAFETY: valid fds.
            let rc = unsafe { libc::ioctl(fout.as_raw_fd(), libc::FICLONE, fin.as_raw_fd()) };
            if rc == 0 {
                return Ok(());
            }
        }
        let _ = std::fs::remove_file(dst);
    }
    std::fs::copy(src, dst)?;
    Ok(())
}

/// Load the bigfiles cache.
pub fn load_bigfiles(path: &Path) -> Result<BigFiles> {
    if !path.exists() {
        return Ok(BigFiles::default());
    }
    crate::store::read_json(path)
}

pub fn save_bigfiles(path: &Path, bf: &BigFiles) -> Result<()> {
    crate::store::atomic_write_json(path, bf)
}

/// Path of the bigfiles cache for a layout.
pub fn bigfiles_path(asp_dir: &Path) -> PathBuf {
    asp_dir.join("bigfiles.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cas_round_trip_and_dedupe() {
        let d = tempfile::tempdir().unwrap();
        let cas = d.path().join("cas");
        let f = d.path().join("big.bin");
        std::fs::write(&f, vec![7u8; 1024 * 1024]).unwrap();

        let e1 = store_in_cas(&cas, &f).unwrap();
        let e2 = store_in_cas(&cas, &f).unwrap();
        assert_eq!(e1.blake3, e2.blake3);
        assert_eq!(std::fs::read_dir(&cas).unwrap().count(), 1);

        let out = d.path().join("restored.bin");
        restore_from_cas(&cas, &e1.blake3, &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), vec![7u8; 1024 * 1024]);
    }

    #[test]
    fn pointer_round_trip() {
        let e = BigFileEntry {
            blake3: "ab".repeat(32),
            size: 123,
            mtime_ms: 0,
            pointer_oid: None,
        };
        let json = pointer_json(&e);
        let p = parse_pointer(json.as_bytes()).unwrap();
        assert_eq!(p.size, 123);
        assert!(parse_pointer(b"not a pointer").is_none());
        assert!(parse_pointer(b"{\"other\":1}").is_none());
    }

    #[test]
    fn missing_cas_blob_is_actionable() {
        let d = tempfile::tempdir().unwrap();
        let err = restore_from_cas(d.path(), "deadbeef", &d.path().join("x")).unwrap_err();
        assert_eq!(err.code, ErrorCode::StoreCorrupt);
        assert!(err.hint.is_some());
    }
}
