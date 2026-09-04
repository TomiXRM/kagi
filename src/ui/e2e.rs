//! ADR-0166 — GUI E2E seam (Lane 1, macOS native).
//!
//! The minimal, dependency-hiding entry points a `harness = false` main-thread
//! runner (`tests/gui_e2e_runner.rs`) needs to mount the real [`KagiApp`]
//! offscreen against a fixture repo.
//!
//! Why a seam at all: an integration-test target (`tests/`) links only the
//! `kagi` lib + its **dev**-dependencies. It cannot name the bin's normal deps
//! (`gpui_component`, `gpui_platform`, `kagi_git`). These `pub` helpers wrap
//! those so the runner touches nothing but `gpui` (a dev-dep, which unlocks
//! `VisualTestAppContext`) and `kagi`.
//!
//! Why it lives here and not behind `#[cfg(test)]`: `#[cfg(test)]` items are
//! invisible to integration-test crates. So these compile into every build.
//! That is safe — they touch only plain `gpui` + normal deps, never
//! `gpui/test-support`, so production stays free of test-support (verified by
//! `cargo tree -e no-dev -i gpui`). Being `pub`, they raise no dead-code lint.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext as _, AssetSource, Entity, KeyBinding, Platform, Styled as _, Window};

use super::assets::KagiAssets;
use super::{fonts, oplog_panel, theme, toast_stack, KagiApp, ToggleBottomPanel};

/// The real Mac platform for `VisualTestAppContext::with_asset_source`.
/// (`gpui_platform` is a normal dep, so the runner cannot call it directly.)
pub fn platform() -> Rc<dyn Platform> {
    gpui_platform::current_platform(false)
}

/// Bundled fonts + SVG icons, so an offscreen render is not a blank frame.
pub fn asset_source() -> Arc<dyn AssetSource> {
    Arc::new(KagiAssets)
}

/// App-level one-time init a first render needs, mirroring `run_app`: bundled
/// fonts, `gpui_component` init, theme sync, and the one keybinding the PoC
/// scenario exercises (`cmd-j` → [`ToggleBottomPanel`]).
pub fn init_app(cx: &mut App) {
    fonts::load_bundled_fonts(cx);
    gpui_component::init(cx);
    theme::sync_gpui_component_theme(cx);
    cx.bind_keys([KeyBinding::new("secondary-j", ToggleBottomPanel, None)]);
}

/// Build the real [`KagiApp`] state for a fixture repo: open + snapshot (via
/// `kagi_git`, no `git2` token) + a single active tab, as `main.rs` does.
pub fn app_state(repo_path: &Path) -> Result<KagiApp, String> {
    let info = kagi_git::open_repository(repo_path).map_err(|e| e.to_string())?;
    let mut backend = kagi_git::Backend::open(repo_path).map_err(|e| e.to_string())?;
    let snap = backend.snapshot(10_000).map_err(|e| e.to_string())?;
    let mut app = KagiApp::from_snapshot(&info.name, &snap);
    app.repo_path = Some(repo_path.to_path_buf());
    app.tabs.push(super::tabs::RepoTab {
        path: repo_path.to_path_buf(),
        name: info.name.clone(),
        remote: None,
        is_worktree: info.is_worktree,
        wt_color_idx: None,
    });
    app.active_tab = 0;
    Ok(app)
}

/// The `KagiApp` entity construction shared with `open_main_window`: the root
/// focus handle, toast stack, and op-log panel — the parts that need a `cx`.
/// Extracted so the offscreen mount and the real window build the entity the
/// exact same way (ADR-0166).
pub fn build_kagi_entity(
    mut app_state: KagiApp,
    window: &mut Window,
    cx: &mut App,
) -> Entity<KagiApp> {
    let kagi: Entity<KagiApp> = cx.new(|cx| {
        app_state.root_focus = Some(cx.focus_handle());
        app_state.toast_stack = Some(cx.new(|_| toast_stack::ToastStack::new()));
        let seed = std::mem::take(&mut app_state.op_log_seed);
        app_state.op_log = Some(cx.new(|_| oplog_panel::OpLogPanel::from_entries(seed)));
        app_state
    });
    if let Some(fh) = kagi.read(cx).root_focus.clone() {
        window.focus(&fh, cx);
    }
    kagi
}

/// Mount the real root offscreen: build the [`KagiApp`] entity (captured into
/// `out` so the runner can read observable state) and wrap it in
/// `gpui_component::Root` exactly like `open_main_window`. Returned as the
/// window's root view for `open_offscreen_window`'s build closure.
pub fn mount_root(
    app_state: KagiApp,
    window: &mut Window,
    cx: &mut App,
    out: &Rc<RefCell<Option<Entity<KagiApp>>>>,
) -> Entity<gpui_component::Root> {
    let kagi = build_kagi_entity(app_state, window, cx);
    *out.borrow_mut() = Some(kagi.clone());
    cx.new(|cx| gpui_component::Root::new(kagi, window, cx).font(theme::ui_font()))
}
