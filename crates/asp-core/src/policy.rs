//! Local workspace policy (`.asp/policy.toml`).
//!
//! The policy file is local-first: teams can commit and review it like any
//! other repo file, and the engine enforces it before risky mutations.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default)]
    pub forks: ForkPolicy,
    #[serde(default)]
    pub checkpoints: CheckpointPolicy,
    #[serde(default)]
    pub paths: PathPolicy,
    #[serde(default)]
    pub promote: PromotePolicy,
    #[serde(default)]
    pub retention: RetentionPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ForkPolicy {
    pub max_active: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CheckpointPolicy {
    pub max_age_hours: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PathPolicy {
    #[serde(default)]
    pub protected: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PromotePolicy {
    #[serde(default)]
    pub require_clean_status: bool,
    #[serde(default)]
    pub require_checkpoint: bool,
    #[serde(default)]
    pub allowed_branch_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RetentionPolicy {
    pub keep_last: Option<u64>,
    pub max_age_days: Option<u64>,
}

impl Policy {
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let policy: Self = toml::from_str(&text).map_err(|e| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("invalid {}: {e}", path.display()),
            )
            .with_hint("fix the TOML syntax, or delete the file to disable local policy")
        })?;
        policy.validate(path)?;
        Ok(policy)
    }

    pub fn template() -> &'static str {
        r#"# asp local policy. Every setting is optional.
# When set, these controls are enforced locally before risky workspace changes.

[forks]
# Maximum active sibling forks allowed before new fork creation is blocked:
# max_active = 8

[checkpoints]
# Maximum acceptable age for the latest checkpoint before fork/restore/promote:
# max_age_hours = 24

[paths]
# Path patterns protected from restore and promote:
# protected = ["src/security/**", ".github/workflows/**"]

[promote]
# require_clean_status = true
# require_checkpoint = true
# allowed_branch_prefixes = ["asp/"]

[retention]
# Keep at least this many newest checkpoints, even if they exceed max_age_days:
# keep_last = 50
# Mark checkpoints older than this many days as eligible in retention plans:
# max_age_days = 30
"#
    }

    pub fn validate(&self, path: &Path) -> Result<()> {
        if self.forks.max_active == Some(0) {
            return Err(policy_error(
                path,
                "forks.max_active must be greater than 0 when set",
            ));
        }
        if self.checkpoints.max_age_hours == Some(0) {
            return Err(policy_error(
                path,
                "checkpoints.max_age_hours must be greater than 0 when set",
            ));
        }
        if self.retention.keep_last == Some(0) {
            return Err(policy_error(
                path,
                "retention.keep_last must be greater than 0 when set",
            ));
        }
        if self.retention.max_age_days == Some(0) {
            return Err(policy_error(
                path,
                "retention.max_age_days must be greater than 0 when set",
            ));
        }
        for protected in &self.paths.protected {
            validate_path_pattern(path, "paths.protected", protected)?;
        }
        for prefix in &self.promote.allowed_branch_prefixes {
            if prefix.trim().is_empty() {
                return Err(policy_error(
                    path,
                    "promote.allowed_branch_prefixes entries cannot be empty",
                ));
            }
            if prefix.chars().any(char::is_whitespace) {
                return Err(policy_error(
                    path,
                    "promote.allowed_branch_prefixes entries cannot contain whitespace",
                ));
            }
        }
        Ok(())
    }
}

fn validate_path_pattern(path: &Path, key: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(policy_error(path, format!("{key} entries cannot be empty")));
    }
    if value.starts_with('/') {
        return Err(policy_error(
            path,
            format!("{key} entries must be workspace-relative, not absolute"),
        ));
    }
    if value.split('/').any(|part| part == "..") {
        return Err(policy_error(
            path,
            format!("{key} entries cannot contain '..' path segments"),
        ));
    }
    Ok(())
}

fn policy_error(path: &Path, message: impl Into<String>) -> Error {
    Error::new(
        ErrorCode::StoreCorrupt,
        format!("invalid {}: {}", path.display(), message.into()),
    )
    .with_hint("edit .asp/policy.toml, or delete it to disable local policy")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_default_policy() {
        let dir = tempfile::tempdir().unwrap();
        let policy = Policy::load(&dir.path().join("policy.toml")).unwrap();
        assert_eq!(policy.forks.max_active, None);
        assert!(!policy.promote.require_clean_status);
    }

    #[test]
    fn template_parses() {
        let policy: Policy = toml::from_str(Policy::template()).unwrap();
        policy.validate(Path::new(".asp/policy.toml")).unwrap();
    }

    #[test]
    fn policy_fields_parse() {
        let policy: Policy = toml::from_str(
            r#"
[forks]
max_active = 4

[checkpoints]
max_age_hours = 12

[paths]
protected = ["src/security/**"]

[promote]
require_clean_status = true
require_checkpoint = true
allowed_branch_prefixes = ["asp/", "review/"]

[retention]
keep_last = 20
max_age_days = 30
"#,
        )
        .unwrap();
        policy.validate(Path::new(".asp/policy.toml")).unwrap();
        assert_eq!(policy.forks.max_active, Some(4));
        assert!(policy.promote.require_clean_status);
        assert_eq!(policy.retention.keep_last, Some(20));
    }

    #[test]
    fn bad_toml_has_hint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.toml");
        std::fs::write(&path, "[forks\n").unwrap();
        let err = Policy::load(&path).unwrap_err();
        assert_eq!(err.code, ErrorCode::StoreCorrupt);
        assert!(err.hint.unwrap().contains("TOML"));
    }

    #[test]
    fn semantic_errors_have_hints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.toml");
        std::fs::write(&path, "[forks]\nmax_active = 0\n").unwrap();
        let err = Policy::load(&path).unwrap_err();
        assert_eq!(err.code, ErrorCode::StoreCorrupt);
        assert!(err.message.contains("forks.max_active"));
        assert!(err.hint.unwrap().contains("policy.toml"));
    }

    #[test]
    fn protected_paths_are_workspace_relative() {
        let policy: Policy = toml::from_str("[paths]\nprotected = [\"../secrets\"]\n").unwrap();
        let err = policy.validate(Path::new(".asp/policy.toml")).unwrap_err();
        assert!(err.message.contains("paths.protected"));
    }

    #[test]
    fn retention_values_must_be_positive() {
        let policy: Policy = toml::from_str("[retention]\nkeep_last = 0\n").unwrap();
        let err = policy.validate(Path::new(".asp/policy.toml")).unwrap_err();
        assert!(err.message.contains("retention.keep_last"));
    }
}
