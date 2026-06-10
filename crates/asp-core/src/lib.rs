//! asp-core: the engine behind agentspaces.
//!
//! Durable, branchable agent workspaces over real directories. The trust
//! model: every checkpoint is recoverable with stock git, and the worst-case
//! failure mode degrades to a plain git repository.

pub mod config;
pub mod error;
pub mod fork;
pub mod gitx;
pub mod journal;
pub mod store;
pub mod workspace;

pub use error::{Error, ErrorCode, Result};
pub use workspace::Workspace;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Current time as RFC3339 (UTC, second precision).
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::version().is_empty());
    }

    #[test]
    fn now_is_rfc3339() {
        let ts = super::now_rfc3339();
        assert!(ts.ends_with('Z') && ts.contains('T'), "{ts}");
    }
}
