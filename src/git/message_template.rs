//! Structured commit-message template (`type(scope): summary` + Test/Risk).
//!
//! The pure parse/assemble logic now lives in `kagi_domain::message_template`
//! (ADR-0072). This is a re-export bridge kept so existing paths resolve during
//! the migration.
pub use kagi_domain::message_template::*;
