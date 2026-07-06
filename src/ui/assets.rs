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
    (
        "icons/arrow-down.svg",
        include_bytes!("../../assets/icons/arrow-down.svg"),
    ),
    (
        "icons/arrow-up.svg",
        include_bytes!("../../assets/icons/arrow-up.svg"),
    ),
    (
        "icons/plus.svg",
        include_bytes!("../../assets/icons/plus.svg"),
    ),
    (
        "icons/inbox.svg",
        include_bytes!("../../assets/icons/inbox.svg"),
    ),
    (
        "icons/folder-open.svg",
        include_bytes!("../../assets/icons/folder-open.svg"),
    ),
    (
        "icons/undo-2.svg",
        include_bytes!("../../assets/icons/undo-2.svg"),
    ),
    (
        "icons/loader-circle.svg",
        include_bytes!("../../assets/icons/loader-circle.svg"),
    ),
    (
        "icons/square-terminal.svg",
        include_bytes!("../../assets/icons/square-terminal.svg"),
    ),
    (
        "icons/menu.svg",
        include_bytes!("../../assets/icons/menu.svg"),
    ),
    (
        "icons/refresh-cw.svg",
        include_bytes!("../../assets/icons/refresh-cw.svg"),
    ),
    // T-CONFLICT-POLISH-040/041: Conflict Editor toolbar icons.
    (
        "icons/external-link.svg",
        include_bytes!("../../assets/icons/external-link.svg"),
    ),
    (
        "icons/trash-2.svg",
        include_bytes!("../../assets/icons/trash-2.svg"),
    ),
    // T-CONFLICT-DASH-022: per-file card actions (IconName::Copy / Ellipsis).
    (
        "icons/copy.svg",
        include_bytes!("../../assets/icons/copy.svg"),
    ),
    (
        "icons/ellipsis.svg",
        include_bytes!("../../assets/icons/ellipsis.svg"),
    ),
    // T-SETTINGS-001: header Settings gear (gpui_component IconName::Settings).
    (
        "icons/settings.svg",
        include_bytes!("../../assets/icons/settings.svg"),
    ),
    // ADR-0119: Analyze / Code Ecosystem toolbar button (IconName::ChartPie).
    (
        "icons/chart-pie.svg",
        include_bytes!("../../assets/icons/chart-pie.svg"),
    ),
    // T-WS-EDITOR-004: header Editor Workspace toggle (lucide square-pen —
    // custom path, no matching gpui-component IconName variant).
    (
        "icons/square-pen.svg",
        include_bytes!("../../assets/icons/square-pen.svg"),
    ),
    // Header Editor⇄Graph toggle shows the commit-graph glyph while the
    // Editor workspace is open (lucide waypoints — user request: the button
    // should indicate what it switches back to).
    (
        "icons/waypoints.svg",
        include_bytes!("../../assets/icons/waypoints.svg"),
    ),
    // Editor Workspace tree: expand-all / collapse-all buttons (lucide
    // unfold/fold glyphs; ChevronsDownUp has no IconName variant).
    (
        "icons/chevrons-up-down.svg",
        include_bytes!("../../assets/icons/chevrons-up-down.svg"),
    ),
    (
        "icons/chevrons-down-up.svg",
        include_bytes!("../../assets/icons/chevrons-down-up.svg"),
    ),
    // T-UNDOREDO-001: toolbar Redo button (gpui_component IconName::Redo2).
    (
        "icons/redo-2.svg",
        include_bytes!("../../assets/icons/redo-2.svg"),
    ),
    // T-LINUX-TITLEBAR: client-side window control buttons drawn by
    // gpui_component::TitleBar on Linux/freebsd (WindowControls →
    // IconName::WindowMinimize/Maximize/Restore/Close). Without these the
    // close/maximize/minimize buttons render as nothing (blank title bar).
    (
        "icons/window-minimize.svg",
        include_bytes!("../../assets/icons/window-minimize.svg"),
    ),
    (
        "icons/window-maximize.svg",
        include_bytes!("../../assets/icons/window-maximize.svg"),
    ),
    (
        "icons/window-restore.svg",
        include_bytes!("../../assets/icons/window-restore.svg"),
    ),
    (
        "icons/window-close.svg",
        include_bytes!("../../assets/icons/window-close.svg"),
    ),
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
