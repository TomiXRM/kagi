//! File tree builder — moved to `kagi-ui-core::file_tree` (ADR-0121 C4) so
//! the Editor Workspace crate can share it; re-exported here so every
//! existing `crate::ui::file_tree::…` path keeps working.

pub use kagi_ui_core::file_tree::*;
