//! Workspace configuration (`.asp/config.toml`). User-editable; every field
//! has a default so an empty file is valid.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

pub const DEFAULT_EXCLUDES: &[&str] = &[
    "node_modules/",
    "target/",
    ".venv/",
    "venv/",
    "__pycache__/",
    "build/",
    "dist/",
    ".next/",
    ".cache/",
];

pub const DEFAULT_BLOB_THRESHOLD_MB: u64 = 50;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub capture: CaptureConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureConfig {
    /// Derived-state directories excluded from checkpoints (forks still carry
    /// them physically). Replaces the default list when set.
    #[serde(default = "default_excludes")]
    pub excludes: Vec<String>,
    /// Extra excludes appended to the list above.
    #[serde(default)]
    pub extra_excludes: Vec<String>,
    /// Files larger than this go to the BLAKE3 CAS sidecar instead of git.
    #[serde(default = "default_blob_threshold_mb")]
    pub blob_threshold_mb: u64,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            excludes: default_excludes(),
            extra_excludes: Vec::new(),
            blob_threshold_mb: default_blob_threshold_mb(),
        }
    }
}

fn default_excludes() -> Vec<String> {
    DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect()
}

fn default_blob_threshold_mb() -> u64 {
    DEFAULT_BLOB_THRESHOLD_MB
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        toml::from_str(&text).map_err(|e| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("invalid {}: {e}", path.display()),
            )
            .with_hint("fix the TOML syntax, or delete the file to restore defaults")
        })
    }

    /// The default config file written by `asp init`, with docs inline.
    pub fn template() -> &'static str {
        r#"# asp workspace configuration. Every setting is optional.

[capture]
# Derived-state directories excluded from checkpoints (forks still carry them
# physically — they are rebuildable from lockfiles). Uncomment to override:
# excludes = ["node_modules/", "target/", ".venv/", "venv/", "__pycache__/", "build/", "dist/", ".next/", ".cache/"]

# Append extra exclude patterns (gitignore syntax) without replacing defaults:
# extra_excludes = ["data/raw/"]

# Files larger than this many MB are stored in the content-addressed sidecar
# (.asp/blobs) instead of git objects:
# blob_threshold_mb = 50
"#
    }

    /// Effective exclude patterns for the shadow repo's info/exclude,
    /// including asp's own mandatory entries.
    pub fn shadow_excludes(&self) -> Vec<String> {
        let mut v = vec!["/.asp/".to_string()];
        v.extend(self.capture.excludes.iter().cloned());
        v.extend(self.capture.extra_excludes.iter().cloned());
        v
    }

    pub fn blob_threshold_bytes(&self) -> u64 {
        self.capture.blob_threshold_mb * 1024 * 1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_valid_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "").unwrap();
        let c = Config::load(&p).unwrap();
        assert_eq!(c.capture.blob_threshold_mb, DEFAULT_BLOB_THRESHOLD_MB);
        assert!(c.capture.excludes.contains(&"node_modules/".to_string()));
    }

    #[test]
    fn missing_file_is_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let c = Config::load(&dir.path().join("nope.toml")).unwrap();
        assert!(!c.shadow_excludes().is_empty());
        assert_eq!(c.shadow_excludes()[0], "/.asp/");
    }

    #[test]
    fn template_parses() {
        let c: Config = toml::from_str(Config::template()).unwrap();
        assert_eq!(c.capture.blob_threshold_mb, DEFAULT_BLOB_THRESHOLD_MB);
    }

    #[test]
    fn extra_excludes_append() {
        let c: Config = toml::from_str("[capture]\nextra_excludes=[\"data/\"]").unwrap();
        let ex = c.shadow_excludes();
        assert!(ex.contains(&"data/".to_string()));
        assert!(ex.contains(&"node_modules/".to_string()));
    }

    #[test]
    fn bad_toml_has_hint() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "not [valid").unwrap();
        let err = Config::load(&p).unwrap_err();
        assert!(err.hint.is_some());
    }
}
