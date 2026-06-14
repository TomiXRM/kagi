//! Git ref domain models.
//!
//! The pure types (`Branch`, `RemoteBranch`, `Tag`, `Stash`, `Worktree`,
//! `UpstreamInfo`) now live in `kagi_domain::refs` (ADR-0072). This is a
//! re-export bridge kept so existing `crate::git::refs::*` paths resolve during
//! the migration. Population of these types from a repository happens in
//! `src/git/snapshot.rs` (git2 backend).
pub use kagi_domain::refs::*;
