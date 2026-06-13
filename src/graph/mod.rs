//! Commit graph layout.
//!
//! The implementation now lives in the pure `kagi-domain` crate
//! (`kagi_domain::graph`, ADR-0072). This module is a re-export bridge kept so
//! existing `crate::graph::*` / `kagi::graph::*` paths continue to resolve
//! during the v1.0 migration (`docs/rearch/migration/README.md`).
pub use kagi_domain::graph::*;
