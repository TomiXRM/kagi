//! kagi-ui-core (ADR-0121 Phase C1): shared UI foundation extracted from the
//! bin crate — the `[kagi]` klog contract macro, settings persistence, i18n,
//! and theme tokens. Dependency direction:
//! `kagi(bin)` → `kagi-ui-*` → `kagi-ui-core` → kagi-domain (no Git backend).
//! The bin's `src/ui/{theme,i18n,settings}.rs` are re-export shims over this
//! crate, so existing `crate::ui::theme::…` paths keep working.

#[macro_use]
pub mod klog;

pub mod divider;
pub mod file_tree;
pub mod i18n;
pub mod settings;
pub mod theme;
pub mod time;
