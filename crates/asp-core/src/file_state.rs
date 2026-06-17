//! Rebuildable file-state cache for performance-sensitive scans.
//!
//! This is deliberately not part of the recovery source of truth. The shadow
//! git refs, CAS, and journal remain authoritative; this file can be missing,
//! stale, or corrupt and the engine will rebuild it after a checkpoint.

use std::collections::BTreeMap;
use std::fs::Metadata;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::store::{atomic_write_json, read_json};

pub const FILE_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStateIndex {
    pub v: u32,
    pub head: String,
    pub entries: BTreeMap<String, FileStateEntry>,
}

impl FileStateIndex {
    pub fn new(head: impl Into<String>) -> Self {
        Self {
            v: FILE_STATE_VERSION,
            head: head.into(),
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStateEntry {
    pub kind: FileStateKind,
    pub size: u64,
    pub mtime_ms: i64,
}

impl FileStateEntry {
    pub fn from_metadata(md: &Metadata) -> Self {
        let kind = if md.file_type().is_symlink() {
            FileStateKind::Symlink
        } else if md.is_file() {
            FileStateKind::File
        } else {
            FileStateKind::Other
        };
        Self {
            kind,
            size: md.len(),
            mtime_ms: crate::blobs::mtime_ms(md),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStateKind {
    File,
    Symlink,
    Other,
}

pub fn load(path: &Path) -> Result<FileStateIndex> {
    read_json(path)
}

pub fn save(path: &Path, index: &FileStateIndex) -> Result<()> {
    atomic_write_json(path, index)
}
