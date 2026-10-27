//! kagi-ui-core (ADR-0121 Phase C1): shared UI foundation extracted from the
//! bin crate — the `[kagi]` klog contract macro, settings persistence, i18n,
//! and theme tokens. Dependency direction:
//! `kagi(bin)` → `kagi-ui-*` → `kagi-ui-core` → kagi-domain (no Git backend).
//! The bin's `src/ui/{theme,i18n,settings}.rs` are re-export shims over this
//! crate, so existing `crate::ui::theme::…` paths keep working.
//!
//! On wasm, settings types and render-time tokens remain available, but native
//! persistence and discovery entry points do not. Browser hosts initialize the
//! existing presentation state through `i18n::set_lang_no_persist` and
//! `theme::{init_loaded_active, init_loaded_zoom, init_loaded_accent}` before
//! rendering; these APIs do not create a settings store or imply a saved result.

#[macro_use]
pub mod klog;

pub mod accessibility;
#[cfg(not(target_arch = "wasm32"))]
mod atomic_file;
pub mod avatar;
pub mod change_badge;
pub mod color_vision;
pub mod commit_header;
pub mod commit_row;
pub mod diff_list;
pub mod divider;
pub mod file_tree;
pub mod find;
pub mod fonts;
pub mod header_fit;
pub mod i18n;
pub mod input;
pub mod input_actions;
pub mod keyboard_nav;
pub mod markdown;
pub mod motion;
#[cfg(not(target_arch = "wasm32"))]
pub mod pr_viewed;
pub mod settings;
pub mod slow_read;
mod system_accent;
pub mod theme;
pub mod theme_apple_dark;
pub mod theme_apple_light;
pub mod theme_catppuccin_latte;
pub mod theme_catppuccin_mocha;
pub mod theme_color_vision;
pub mod theme_custom;
pub mod theme_dracula;
pub mod theme_flower_road;
pub mod theme_ibm_pc;
pub mod theme_monokai;
pub mod theme_nagi;
pub mod theme_one_dark;
pub mod theme_one_light;
pub mod theme_pinky_boo;
pub mod theme_tokyo_night;
pub mod time;
pub mod time_parse;
pub mod tooltip;
pub mod tree_a11y;
pub mod variable_list;
