//! View-model layer (ADR-0076 / issue #13 P5).
//!
//! View-models are **plain data** projections of the already-pure snapshot/
//! `TabViewState` data into render-ready form. They contain no `gpui` widgets
//! and no `git2`, so they can be unit-tested without opening a window — the
//! foundation for retiring the `KAGI_*` headless harness over time (ADR-0077
//! test pyramid). Views read a VM and emit widgets; the VM owns the
//! presentation *decisions* (which chips show, what their labels are), the view
//! owns only the `gpui` assembly (colours, spacing).
//!
//! This module is introduced incrementally; `StatusBarVM` is the first slice.

pub mod status_bar;

pub use status_bar::{StatusBarVM, StatusChipRole};
