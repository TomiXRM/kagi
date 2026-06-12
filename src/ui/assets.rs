//! Compile-time embedded assets — T-UI-001
//!
//! gpui-component's `Icon` resolves [`IconName`] values to paths like
//! `"icons/arrow-down.svg"` and loads them through the application's
//! [`gpui::AssetSource`].  Without a registered source the icons silently
//! render as nothing (user report).  This module embeds the handful of
//! lucide SVGs we actually use via `include_bytes!` — no extra crates,
//! no runtime file I/O.
//!
//! The SVG files in `assets/icons/` are copied verbatim from
//! gpui-component v0.5.1 (`crates/assets/assets/icons/`, Apache-2.0,
//! originally lucide.dev ISC).

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// Embedded asset source for the kagi binary.
pub struct KagiAssets;

/// (path, bytes) table of every embedded asset.
const ASSETS: &[(&str, &[u8])] = &[
    ("icons/arrow-down.svg", include_bytes!("../../assets/icons/arrow-down.svg")),
    ("icons/arrow-up.svg", include_bytes!("../../assets/icons/arrow-up.svg")),
    ("icons/plus.svg", include_bytes!("../../assets/icons/plus.svg")),
    ("icons/inbox.svg", include_bytes!("../../assets/icons/inbox.svg")),
    ("icons/folder-open.svg", include_bytes!("../../assets/icons/folder-open.svg")),
    ("icons/undo-2.svg", include_bytes!("../../assets/icons/undo-2.svg")),
    ("icons/loader-circle.svg", include_bytes!("../../assets/icons/loader-circle.svg")),
    ("icons/square-terminal.svg", include_bytes!("../../assets/icons/square-terminal.svg")),
    ("icons/menu.svg", include_bytes!("../../assets/icons/menu.svg")),
];

impl AssetSource for KagiAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ASSETS
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ASSETS
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}
