//! Co-author trailer parsing.
//!
//! The pure parser now lives in `kagi_domain::trailers` (ADR-0072). This is a
//! re-export bridge kept so existing paths resolve during the migration.
pub use kagi_domain::trailers::*;
