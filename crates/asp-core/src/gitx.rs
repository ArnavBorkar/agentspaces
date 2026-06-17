//! Shadow-git subprocess wrapper. All checkpoint storage goes through stock
//! git plumbing against a sidecar GIT_DIR — that is the trust model: the
//! worst-case failure mode degrades to a plain git repository.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use crate::error::{Error, ErrorCode, Result};

const INDEX_LOCK_RETRY_ATTEMPTS: usize = 25;
const INDEX_LOCK_RETRY_DELAY: Duration = Duration::from_millis(120);

/// Handle to the shadow repository of one workspace.
#[derive(Debug, Clone)]
pub struct Shadow {
    git_dir: PathBuf,
    work_tree: PathBuf,
    index_file: PathBuf,
}

impl Shadow {
    pub fn new(git_dir: PathBuf, work_tree: PathBuf, index_file: PathBuf) -> Self {
        Self {
            git_dir,
            work_tree,
            index_file,
        }
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new("git");
        // Fully pin the environment: cwd-independent, immune to user/global
        // git config, with a stable synthetic identity for shadow commits.
        cmd.env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env("GIT_DIR", &self.git_dir)
            .env("GIT_WORK_TREE", &self.work_tree)
            .env("GIT_INDEX_FILE", &self.index_file)
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_CONFIG_SYSTEM", null_device())
            .env("GIT_AUTHOR_NAME", "asp")
            .env("GIT_AUTHOR_EMAIL", "asp@agentspaces.local")
            .env("GIT_COMMITTER_NAME", "asp")
            .env("GIT_COMMITTER_EMAIL", "asp@agentspaces.local")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .current_dir(&self.work_tree);
        cmd
    }

    /// Run a git command, returning trimmed stdout. Errors carry stderr.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        let output = self.run_with_index_lock_retry(args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(git_failed(args.first().copied().unwrap_or(""), &stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run a git command feeding `input` to stdin, returning trimmed stdout.
    /// stdin is written from a separate thread while stdout/stderr drain —
    /// immune to pipe-buffer deadlocks on large inputs and early git exits.
    pub fn run_with_stdin(&self, args: &[&str], input: &str) -> Result<String> {
        use std::io::Write;
        use std::process::Stdio;
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
            })?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let payload = input.as_bytes().to_vec();
        let writer = std::thread::spawn(move || {
            // A write error here means git exited early; its stderr tells
            // the real story below.
            let _ = stdin.write_all(&payload);
        });
        let output = child.wait_with_output()?;
        let _ = writer.join();
        if !output.status.success() {
            return Err(Error::new(
                ErrorCode::GitFailed,
                format!(
                    "git {} failed: {}",
                    args.iter().find(|a| !a.starts_with('-')).unwrap_or(&""),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run a git command, returning raw stdout bytes (for -z output).
    pub fn run_raw(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = self.run_with_index_lock_retry(args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(git_failed(args.first().copied().unwrap_or(""), &stderr));
        }
        Ok(output.stdout)
    }

    fn run_with_index_lock_retry(&self, args: &[&str]) -> Result<Output> {
        let mut last_output = None;
        for attempt in 0..INDEX_LOCK_RETRY_ATTEMPTS {
            let output = self.command().args(args).output().map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::new(ErrorCode::GitMissing, "git is not installed or not on PATH")
                        .with_hint(
                            "install git (https://git-scm.com) — asp uses it as its storage engine",
                        )
                } else {
                    Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}"))
                        .with_source(e)
                }
            })?;
            if output.status.success() || !is_index_lock_error(&output.stderr) {
                return Ok(output);
            }
            last_output = Some(output);
            if attempt + 1 < INDEX_LOCK_RETRY_ATTEMPTS {
                std::thread::sleep(INDEX_LOCK_RETRY_DELAY);
            }
        }
        Ok(last_output.expect("at least one git attempt"))
    }

    /// Initialize the bare shadow repo with asp's pinned configuration.
    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.git_dir)?;
        // `git init --bare` must not see the work-tree env.
        let output = Command::new("git")
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_CONFIG_SYSTEM", null_device())
            .args(["init", "--bare", "-q"])
            .arg(&self.git_dir)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::new(ErrorCode::GitMissing, "git is not installed or not on PATH")
                        .with_hint(
                            "install git (https://git-scm.com) — asp uses it as its storage engine",
                        )
                } else {
                    Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}"))
                        .with_source(e)
                }
            })?;
        if !output.status.success() {
            return Err(Error::new(
                ErrorCode::GitFailed,
                format!(
                    "git init failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        self.run(&["config", "core.compression", "0"])?;
        // Packs get light compression: repack speed matters less than the
        // loose-object hot path, and source text compresses well.
        self.run(&["config", "pack.compression", "1"])?;
        self.run(&["config", "gc.auto", "0"])?;
        self.run(&["config", "core.untrackedCache", "true"])?;
        // Point HEAD at asp's ref so `git status` compares against the latest
        // checkpoint (HEAD is never checked out; no branch ever exists).
        self.run(&["symbolic-ref", "HEAD", crate::workspace::HEAD_REF])?;
        Ok(())
    }

    /// Overwrite `info/exclude` with the given patterns (one per line).
    pub fn write_excludes(&self, patterns: &[String]) -> Result<()> {
        let info = self.git_dir.join("info");
        std::fs::create_dir_all(&info)?;
        let body = patterns.join("\n") + "\n";
        crate::store::atomic_write(&info.join("exclude"), body.as_bytes())?;
        Ok(())
    }

    /// Stage the entire worktree (respecting excludes) and return the tree oid.
    pub fn capture_tree(&self) -> Result<String> {
        self.run(&["add", "-A", "."])?;
        self.run(&["write-tree"])
    }

    /// Commit a tree with optional parent; returns the commit oid.
    pub fn commit_tree(&self, tree: &str, parent: Option<&str>, message: &str) -> Result<String> {
        let mut args = vec!["commit-tree", tree, "-m", message];
        if let Some(p) = parent {
            args.push("-p");
            args.push(p);
        }
        self.run(&args)
    }

    pub fn update_ref(&self, name: &str, value: &str) -> Result<()> {
        self.run(&["update-ref", name, value])?;
        Ok(())
    }

    pub fn rev_parse(&self, rev: &str) -> Result<Option<String>> {
        let output = self
            .command()
            .args([
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{rev}^{{commit}}"),
            ])
            .output()
            .map_err(|e| {
                Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
            })?;
        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        } else {
            Ok(None)
        }
    }

    /// Tree oid of a commit.
    pub fn tree_of(&self, commit: &str) -> Result<String> {
        self.run(&["rev-parse", &format!("{commit}^{{tree}}")])
    }

    /// Resolve a ref to any object kind (blob refs like refs/asp/meta/*).
    pub fn rev_parse_any(&self, rev: &str) -> Result<Option<String>> {
        let output = self
            .command()
            .args(["rev-parse", "--verify", "--quiet", rev])
            .output()
            .map_err(|e| {
                Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
            })?;
        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        } else {
            Ok(None)
        }
    }
}

/// asp's config isolation relies on GIT_CONFIG_GLOBAL/GIT_CONFIG_SYSTEM,
/// which git honors from 2.32. Called once at workspace init.
pub fn ensure_git_version() -> Result<()> {
    let output = Command::new("git").arg("--version").output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::new(ErrorCode::GitMissing, "git is not installed or not on PATH").with_hint(
                "install git >= 2.32 (https://git-scm.com) — asp uses it as its storage engine",
            )
        } else {
            Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
        }
    })?;
    let text = String::from_utf8_lossy(&output.stdout);
    // "git version 2.45.2" (possibly with platform suffixes)
    let nums: Vec<u32> = text
        .split_whitespace()
        .nth(2)
        .unwrap_or("")
        .split('.')
        .take(2)
        .filter_map(|n| n.parse().ok())
        .collect();
    if let [major, minor] = nums[..] {
        if (major, minor) < (2, 32) {
            return Err(Error::new(
                ErrorCode::GitMissing,
                format!("git {major}.{minor} is too old — asp needs git >= 2.32"),
            )
            .with_hint("upgrade git (https://git-scm.com); asp pins its config via GIT_CONFIG_GLOBAL, added in 2.32"));
        }
    }
    // Unparseable version strings: proceed optimistically.
    Ok(())
}

fn is_index_lock_error(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr);
    stderr.contains("index.lock") || stderr.contains("shadow.index.lock")
}

fn git_failed(command: &str, stderr: &str) -> Error {
    let hint = if is_index_lock_error(stderr.as_bytes()) {
        "wait a moment and retry; if the lock persists, run `asp doctor --fix`"
    } else {
        "run `asp doctor` to check workspace health"
    };
    Error::new(
        ErrorCode::GitFailed,
        format!("git {command} failed: {stderr}"),
    )
    .with_hint(hint)
}

fn null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

#[cfg(test)]
mod tests {
    use super::is_index_lock_error;

    #[test]
    fn detects_git_index_lock_errors() {
        assert!(is_index_lock_error(
            b"fatal: Unable to create '/tmp/proj/.asp/shadow.index.lock': File exists."
        ));
        assert!(is_index_lock_error(
            b"fatal: Unable to create '.git/index.lock': File exists."
        ));
        assert!(!is_index_lock_error(b"fatal: ambiguous argument 'HEAD'"));
    }
}
