//! Shared parser surfaces for the asp binary.
//!
//! The CLI remains the primary product surface, but parser-only helpers live
//! in the library target so regression and fuzz harnesses can exercise them
//! without spawning commands or mutating workspaces.

pub mod hooks;
pub mod mcp;
