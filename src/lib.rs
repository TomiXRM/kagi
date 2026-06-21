//! kagi library — exposes the graph/remote/update helpers for integration tests.
//!
//! The Git backend lives in the standalone `kagi-git` crate (ADR-0072 / Phase E);
//! reach it via `kagi_git::` rather than `kagi::git::`.

pub mod graph;
pub mod remote;
pub mod update;
