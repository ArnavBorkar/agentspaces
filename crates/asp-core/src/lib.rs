//! asp-core: the engine behind agentspaces.
//!
//! Durable, branchable agent workspaces over real directories. The trust
//! model: every checkpoint is recoverable with stock git, and the worst-case
//! failure mode degrades to a plain git repository.

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::version().is_empty());
    }
}
