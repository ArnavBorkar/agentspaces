//! Error model. Every user-facing error carries a machine-readable code and a
//! corrective hint — agents are first-class users and must be able to
//! self-correct from error text alone.

use serde::Serialize;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotAWorkspace,
    AlreadyInitialized,
    GitMissing,
    GitFailed,
    NothingToDo,
    ForkExists,
    ForkNotFound,
    ForkHasUnpromotedWork,
    CheckpointNotFound,
    NoUserGitRepo,
    BranchExists,
    CrossVolume,
    StoreCorrupt,
    FormatTooNew,
    Locked,
    Io,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    /// Corrective next action, e.g. "run `asp init` in the project root first".
    pub hint: Option<String>,
    #[source]
    pub source: Option<anyhow::Error>,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            source: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<anyhow::Error>) -> Self {
        self.source = Some(source.into());
        self
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::new(ErrorCode::Io, format!("I/O error: {e}")).with_source(e)
    }
}
