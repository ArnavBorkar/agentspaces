//! Shadow-git subprocess wrapper. All checkpoint storage goes through stock
//! git plumbing against a sidecar GIT_DIR — that is the trust model: the
//! worst-case failure mode degrades to a plain git repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, ErrorCode, Result};

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
        let output = self.command().args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::new(ErrorCode::GitMissing, "git is not installed or not on PATH").with_hint(
                    "install git (https://git-scm.com) — asp uses it as its storage engine",
                )
            } else {
                Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
            }
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(Error::new(
                ErrorCode::GitFailed,
                format!("git {} failed: {stderr}", args.first().unwrap_or(&"")),
            )
            .with_hint("run `asp doctor` to check workspace health"));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run a git command, returning raw stdout bytes (for -z output).
    pub fn run_raw(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = self.command().args(args).output().map_err(|e| {
            Error::new(ErrorCode::GitFailed, format!("failed to spawn git: {e}")).with_source(e)
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(Error::new(
                ErrorCode::GitFailed,
                format!("git {} failed: {stderr}", args.first().unwrap_or(&"")),
            ));
        }
        Ok(output.stdout)
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
}

fn null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}
