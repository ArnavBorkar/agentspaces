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
pub const DEFAULT_PROMOTE_BRANCH_TEMPLATE: &str = "asp/{fork}";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub capture: CaptureConfig,
    #[serde(default)]
    pub promote: PromoteConfig,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromoteConfig {
    /// Branch template used by `asp promote` when --branch is omitted.
    #[serde(default = "default_promote_branch_template")]
    pub branch_template: String,
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

impl Default for PromoteConfig {
    fn default() -> Self {
        Self {
            branch_template: default_promote_branch_template(),
        }
    }
}

fn default_excludes() -> Vec<String> {
    DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect()
}

fn default_blob_threshold_mb() -> u64 {
    DEFAULT_BLOB_THRESHOLD_MB
}

fn default_promote_branch_template() -> String {
    DEFAULT_PROMOTE_BRANCH_TEMPLATE.to_string()
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let config: Self = toml::from_str(&text).map_err(|e| {
            Error::new(
                ErrorCode::StoreCorrupt,
                format!("invalid {}: {e}", path.display()),
            )
            .with_hint("fix the TOML syntax, or delete the file to restore defaults")
        })?;
        config.validate(path)?;
        Ok(config)
    }

    fn validate(&self, path: &Path) -> Result<()> {
        validate_branch_template(path, &self.promote.branch_template)?;
        Ok(())
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

[promote]
# Branch template used by `asp promote <fork>` when --branch is omitted.
# Supported placeholders: {fork}, {workspace}, {workspace_id}
# branch_template = "asp/{fork}"
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

    pub fn render_promote_branch(&self, fork: &str, workspace: &str, workspace_id: &str) -> String {
        self.promote
            .branch_template
            .replace("{workspace_id}", workspace_id)
            .replace("{workspace}", workspace)
            .replace("{fork}", fork)
    }
}

fn validate_branch_template(path: &Path, template: &str) -> Result<()> {
    if template.trim().is_empty() {
        return Err(config_error(
            path,
            "promote.branch_template cannot be empty",
        ));
    }
    if template.chars().any(char::is_whitespace) {
        return Err(config_error(
            path,
            "promote.branch_template cannot contain whitespace",
        ));
    }
    if !template.contains("{fork}") {
        return Err(config_error(
            path,
            "promote.branch_template must include {fork} to avoid branch collisions",
        ));
    }

    let allowed = ["{fork}", "{workspace}", "{workspace_id}"];
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                let Some(end) = template[i + 1..].find('}').map(|offset| i + 1 + offset) else {
                    return Err(config_error(
                        path,
                        "promote.branch_template has an unclosed placeholder",
                    ));
                };
                let token = &template[i..=end];
                if !allowed.contains(&token) {
                    return Err(config_error(
                        path,
                        format!("promote.branch_template uses unsupported placeholder {token}"),
                    ));
                }
                i = end + 1;
            }
            b'}' => {
                return Err(config_error(
                    path,
                    "promote.branch_template has an unopened placeholder",
                ));
            }
            _ => i += 1,
        }
    }

    Ok(())
}

fn config_error(path: &Path, message: impl Into<String>) -> Error {
    Error::new(
        ErrorCode::StoreCorrupt,
        format!("invalid {}: {}", path.display(), message.into()),
    )
    .with_hint("edit .asp/config.toml, or delete it to restore defaults")
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
        assert_eq!(c.promote.branch_template, DEFAULT_PROMOTE_BRANCH_TEMPLATE);
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
        assert_eq!(c.promote.branch_template, DEFAULT_PROMOTE_BRANCH_TEMPLATE);
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

    #[test]
    fn promote_branch_template_renders_known_placeholders() {
        let c: Config =
            toml::from_str("[promote]\nbranch_template=\"review/{workspace}/{fork}\"\n").unwrap();
        c.validate(Path::new(".asp/config.toml")).unwrap();
        assert_eq!(
            c.render_promote_branch("fix-1", "api-service", "workspace-id"),
            "review/api-service/fix-1"
        );
    }

    #[test]
    fn promote_branch_template_must_include_fork() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[promote]\nbranch_template=\"review/static\"\n").unwrap();
        let err = Config::load(&p).unwrap_err();
        assert!(err.message.contains("must include {fork}"), "{err:?}");
        assert!(err.hint.is_some());
    }

    #[test]
    fn promote_branch_template_rejects_unknown_placeholders() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(
            &p,
            "[promote]\nbranch_template=\"review/{ticket}/{fork}\"\n",
        )
        .unwrap();
        let err = Config::load(&p).unwrap_err();
        assert!(
            err.message.contains("unsupported placeholder {ticket}"),
            "{err:?}"
        );
    }
}
