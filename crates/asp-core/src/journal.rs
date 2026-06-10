//! Append-only operation journal: the audit log. Each line is
//! `<crc32-hex-8> <json>\n`. Crash recovery truncates a torn tail line;
//! non-tail corruption is surfaced, never silently dropped.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Init,
    Checkpoint,
    Fork,
    Restore,
    Undo,
    Promote,
    Discard,
}

/// What caused an operation. Agents are first-class users — provenance is
/// the product.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Manual,
    Hook,
    Mcp,
    Race,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub v: u32,
    pub ts: String,
    pub op: Op,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_changed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Op-specific details (fork name, restore target, promote branch, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl Entry {
    pub fn new(op: Op) -> Self {
        Self {
            v: 1,
            ts: crate::now_rfc3339(),
            op,
            seq: None,
            commit: None,
            source: None,
            session_id: None,
            tool: None,
            message: None,
            files_changed: None,
            duration_ms: None,
            detail: None,
        }
    }
}

#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
}

#[derive(Debug, Default)]
pub struct ReadReport {
    pub entries: Vec<Entry>,
    /// Lines that failed CRC/parse, with 1-based line numbers. A torn tail
    /// (crash mid-append) is reported via `torn_tail`, not here.
    pub corrupt_lines: Vec<usize>,
    /// A final invalid region consistent with a crash mid-append. Repaired
    /// by `heal()` — which must only run while holding the store lock.
    pub torn_tail: bool,
}

impl Journal {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one entry. fsyncs the file (the journal is the audit log —
    /// checkpoint refs are only as trustworthy as their provenance).
    pub fn append(&self, entry: &Entry) -> Result<()> {
        let json = serde_json::to_string(entry).map_err(|e| {
            Error::new(ErrorCode::Io, format!("journal encode: {e}")).with_source(e)
        })?;
        let crc = crc32fast::hash(json.as_bytes());
        let line = format!("{crc:08x} {json}\n");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(line.as_bytes())?;
        f.sync_data()?;
        Ok(())
    }

    /// Read all entries. Side-effect free: a torn tail is reported, not
    /// repaired (see `heal`), so lock-free readers can never race a writer.
    pub fn read(&self) -> Result<ReadReport> {
        let bytes = match std::fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ReadReport::default()),
            Err(e) => return Err(e.into()),
        };
        let mut report = ReadReport::default();
        let mut valid_prefix_len = 0usize;
        let mut offset = 0usize;
        let mut line_no = 0usize;
        let mut bad: Vec<(usize, usize)> = Vec::new(); // (line_no, start_offset)

        while offset < bytes.len() {
            line_no += 1;
            let end = match bytes[offset..].iter().position(|&b| b == b'\n') {
                Some(rel) => offset + rel,
                None => {
                    // No trailing newline: torn tail by definition.
                    bad.push((line_no, offset));
                    break;
                }
            };
            let line = &bytes[offset..end];
            match parse_line(line) {
                Some(entry) => {
                    report.entries.push(entry);
                    valid_prefix_len = end + 1;
                }
                None => bad.push((line_no, offset)),
            }
            offset = end + 1;
        }

        // A single bad region at the very end = torn tail from a crash.
        let tail_torn = bad.len() == 1 && bad[0].1 == valid_prefix_len;
        if tail_torn {
            report.torn_tail = true;
        } else {
            report.corrupt_lines = bad.into_iter().map(|(n, _)| n).collect();
        }
        Ok(report)
    }

    /// Truncate a torn tail back to the last valid prefix. Callers MUST hold
    /// the workspace store lock — truncating concurrently with a writer's
    /// append could destroy a valid in-flight entry.
    pub fn heal(&self) -> Result<()> {
        let bytes = match std::fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let mut valid_prefix_len = 0usize;
        let mut offset = 0usize;
        while offset < bytes.len() {
            let Some(rel) = bytes[offset..].iter().position(|&b| b == b'\n') else {
                break;
            };
            let end = offset + rel;
            if parse_line(&bytes[offset..end]).is_some() {
                valid_prefix_len = end + 1;
                offset = end + 1;
            } else {
                break;
            }
        }
        if valid_prefix_len < bytes.len() {
            // Only truncate when everything past the valid prefix is a single
            // torn region (no later valid lines — those need doctor, not us).
            let report = self.read()?;
            if report.torn_tail {
                let f = OpenOptions::new().write(true).open(&self.path)?;
                f.set_len(valid_prefix_len as u64)?;
                f.sync_data()?;
            }
        }
        Ok(())
    }

    /// Highest checkpoint seq recorded, if any.
    pub fn last_seq(&self) -> Result<Option<u64>> {
        Ok(self.read()?.entries.iter().filter_map(|e| e.seq).max())
    }
}

fn parse_line(line: &[u8]) -> Option<Entry> {
    let line = std::str::from_utf8(line).ok()?;
    let (crc_hex, json) = line.split_once(' ')?;
    let crc: u32 = u32::from_str_radix(crc_hex, 16).ok()?;
    if crc32fast::hash(json.as_bytes()) != crc {
        return None;
    }
    serde_json::from_str(json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(dir: &Path) -> Journal {
        Journal::new(dir.join("journal.jsonl"))
    }

    #[test]
    fn round_trip() {
        let d = tempfile::tempdir().unwrap();
        let j = mk(d.path());
        let mut e = Entry::new(Op::Checkpoint);
        e.seq = Some(1);
        e.message = Some("hello".into());
        j.append(&e).unwrap();
        let r = j.read().unwrap();
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].seq, Some(1));
        assert!(r.corrupt_lines.is_empty());
    }

    #[test]
    fn torn_tail_self_heals() {
        let d = tempfile::tempdir().unwrap();
        let j = mk(d.path());
        j.append(&Entry::new(Op::Init)).unwrap();
        j.append(&Entry::new(Op::Checkpoint)).unwrap();
        // Simulate a crash mid-append: garbage partial line, no newline.
        {
            let mut f = OpenOptions::new().append(true).open(j.path()).unwrap();
            f.write_all(b"deadbeef {\"v\":1,\"ts\":\"tr").unwrap();
        }
        let r = j.read().unwrap();
        assert_eq!(r.entries.len(), 2);
        assert!(r.corrupt_lines.is_empty());
        assert!(r.torn_tail, "torn tail reported, not silently dropped");
        // read() is side-effect free; heal() repairs under the store lock.
        j.heal().unwrap();
        let r = j.read().unwrap();
        assert!(!r.torn_tail);
        j.append(&Entry::new(Op::Checkpoint)).unwrap();
        assert_eq!(j.read().unwrap().entries.len(), 3);
    }

    #[test]
    fn mid_file_corruption_is_reported_not_dropped() {
        let d = tempfile::tempdir().unwrap();
        let j = mk(d.path());
        j.append(&Entry::new(Op::Init)).unwrap();
        j.append(&Entry::new(Op::Checkpoint)).unwrap();
        // Corrupt the FIRST line in place.
        let mut content = std::fs::read_to_string(j.path()).unwrap();
        content.replace_range(0..8, "00000000");
        std::fs::write(j.path(), content).unwrap();
        let r = j.read().unwrap();
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.corrupt_lines, vec![1]);
    }

    #[test]
    fn missing_file_is_empty() {
        let d = tempfile::tempdir().unwrap();
        let r = mk(d.path()).read().unwrap();
        assert!(r.entries.is_empty());
    }

    #[test]
    fn last_seq() {
        let d = tempfile::tempdir().unwrap();
        let j = mk(d.path());
        assert_eq!(j.last_seq().unwrap(), None);
        for s in [1u64, 2, 3] {
            let mut e = Entry::new(Op::Checkpoint);
            e.seq = Some(s);
            j.append(&e).unwrap();
        }
        assert_eq!(j.last_seq().unwrap(), Some(3));
    }
}
