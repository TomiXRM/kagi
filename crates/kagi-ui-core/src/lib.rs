//! kagi-ui-core (ADR-0121 Phase C1): shared UI foundation extracted from the
//! bin crate — the `[kagi]` klog contract macro, settings persistence, i18n,
//! and theme tokens. Dependency direction:
//! `kagi(bin)` → `kagi-ui-*` → `kagi-ui-core` → kagi-domain (no Git backend).
//! The bin's `src/ui/{theme,i18n,settings}.rs` are re-export shims over this
//! crate, so existing `crate::ui::theme::…` paths keep working.

#[macro_use]
pub mod klog;

pub mod avatar;
pub mod change_badge;
pub mod commit_header;
pub mod commit_row;
pub mod divider;
pub mod file_tree;
pub mod fonts;
pub mod i18n;
pub mod markdown;
pub mod settings;
pub mod theme;
pub mod theme_apple_dark;
pub mod theme_apple_light;
pub mod theme_catppuccin_latte;
pub mod theme_catppuccin_mocha;
pub mod theme_dracula;
pub mod theme_flower_road;
pub mod theme_flower_road_bloom;
pub mod theme_flower_road_vivid;
pub mod theme_ibm_pc;
pub mod theme_monokai;
pub mod theme_one_dark;
pub mod theme_one_light;
pub mod theme_pinky_boo;
pub mod theme_tokyo_night;
pub mod time;
pub mod time_parse;
