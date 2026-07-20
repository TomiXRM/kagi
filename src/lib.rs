//! kagi library — exposes the graph/remote/update helpers for integration tests.
//!
//! The Git backend lives in the standalone `kagi-git` crate (ADR-0072 / Phase E);
//! reach it via `kagi_git::` rather than `kagi::git::`.

pub use kagi_domain::graph; // ADR-0121: was a shim file
pub mod remote;
pub mod update;
