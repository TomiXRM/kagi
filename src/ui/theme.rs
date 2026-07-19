//! Re-export shim (ADR-0121 C1): the theme registry lives in `kagi-ui-core`.
//! Existing `crate::ui::theme::…` / `super::theme::…` paths keep working.

pub use kagi_ui_core::theme::*;
