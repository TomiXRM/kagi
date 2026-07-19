//! `kagi-domain` — the pure Rust domain layer for Kagi.
//!
//! No `gpui`, no `git2`, no I/O. Everything here is unit-testable without a
//! window or a repository. This is the foundation of the v1.0 architecture
//! (see `docs/rearch/architecture.md` §2.1 and ADR-0072).
//!
//! Modules are migrated here incrementally from the v0.2.0 single-crate layout
//! via the strangler plan (`docs/rearch/migration/README.md`). The old
//! `kagi::git` / `kagi::graph` paths continue to work through re-export bridges
//! during the migration.

pub mod activity;
pub mod checklist;
pub mod commit;
pub mod coupling_graph;
pub mod diff;
pub mod diffstat;
pub mod file_history;
pub mod graph;
pub mod head;
pub mod history;
pub mod hotspot;
pub mod hotspot_layout;
pub mod hotspot_report;
pub mod message;
pub mod message_gen;
pub mod message_template;
pub mod operation;
pub mod plan;
pub mod refs;
pub mod remote;
pub mod remote_diff;
pub mod remote_snapshot;
pub mod resolution;
pub mod status;
pub mod trailers;
pub mod update;
