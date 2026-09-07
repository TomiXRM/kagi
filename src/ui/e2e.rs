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

#[cfg(feature = "gui-e2e")]
thread_local! {
    static CONFIRM_BOUNDS: RefCell<std::collections::HashMap<gpui::WindowId, gpui::Bounds<gpui::Pixels>>> = RefCell::new(Default::default());
    static CONTROL_BOUNDS: RefCell<std::collections::HashMap<(gpui::WindowId, String), gpui::Bounds<gpui::Pixels>>> = RefCell::new(Default::default());
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_confirm_bounds(id: gpui::WindowId, bounds: gpui::Bounds<gpui::Pixels>) {
    CONFIRM_BOUNDS.with(|map| map.borrow_mut().insert(id, bounds));
}
#[cfg(feature = "gui-e2e")]
pub fn confirm_bounds(id: gpui::WindowId) -> Option<gpui::Bounds<gpui::Pixels>> {
    CONFIRM_BOUNDS.with(|map| map.borrow().get(&id).copied())
}
#[cfg(feature = "gui-e2e")]
pub fn control_bounds(id: gpui::WindowId, name: &str) -> Option<gpui::Bounds<gpui::Pixels>> {
    CONTROL_BOUNDS.with(|map| map.borrow().get(&(id, name.to_string())).copied())
}
pub(crate) fn measure_control(
    name: impl Into<String>,
    control: impl gpui::IntoElement,
) -> gpui::AnyElement {
    #[cfg(feature = "gui-e2e")]
    use gpui::{IntoElement as _, ParentElement as _};
    #[cfg(feature = "gui-e2e")]
    {
        let name = name.into();
        gpui::div()
            .relative()
            .child(
                gpui::canvas(
                    move |bounds, window, _| {
                        CONTROL_BOUNDS.with(|map| {
                            map.borrow_mut()
                                .insert((window.window_handle().window_id(), name.clone()), bounds);
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(control)
            .into_any_element()
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        let _ = name;
        control.into_any_element()
    }
}
pub(crate) fn measure_confirm(button: impl gpui::IntoElement) -> gpui::AnyElement {
    #[cfg(feature = "gui-e2e")]
    use gpui::{IntoElement as _, ParentElement as _};
    #[cfg(feature = "gui-e2e")]
    {
        gpui::div()
            .relative()
            .child(
                gpui::canvas(
                    |bounds, window, _| {
                        record_confirm_bounds(window.window_handle().window_id(), bounds)
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(button)
            .into_any_element()
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        button.into_any_element()
    }
}

use gpui::{App, AppContext as _, AssetSource, Entity, Platform, Styled as _, Window};

use super::assets::KagiAssets;
use super::{fonts, oplog_panel, theme, toast_stack, KagiApp};

#[cfg(feature = "gui-e2e")]
pub fn app_notice_message(app: &KagiApp) -> Option<&str> {
    app.app_notice().map(|notice| notice.message.as_str())
}

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
/// fonts, `gpui_component` init, theme sync, and the app command setup.
///
/// #492: this used to hand-pick two bindings (`cmd-j`, `cmd-c`), so scenarios
/// ran against a different keymap than the app — `escape` was bound to nothing
/// here, and a modal survived an Esc that closes it for real users. Call
/// [`super::commands::setup_app`] instead of re-listing bindings or menus; keep
/// it after `gpui_component::init`, which several bindings deliberately outrank.
pub fn init_app(cx: &mut App) {
    fonts::load_bundled_fonts(cx);
    gpui_component::init(cx);
    theme::sync_gpui_component_theme(cx);
    super::commands::setup_app(cx);
}

/// Build the real [`KagiApp`] state for a fixture repo: open + snapshot (via
/// `kagi_git`, no `git2` token) + a single active tab, as `main.rs` does.
pub fn app_state(repo_path: &Path) -> Result<KagiApp, String> {
    let info = kagi_git::open_repository(repo_path).map_err(|e| e.to_string())?;
    let mut backend = kagi_git::Backend::open(repo_path).map_err(|e| e.to_string())?;
    let snap = backend.snapshot(10_000).map_err(|e| e.to_string())?;
    let mut app = KagiApp::from_snapshot(repo_path, &info.name, info.is_worktree, &snap);
    app.repo_path = Some(repo_path.to_path_buf());
    // ADR-0107: the per-tab session every real launch has (`tabs.rs`). Without
    // it the staging / diff paths that go through `repo_session` silently
    // no-op, which would let a scenario pass for the wrong reason (#473).
    app.repo_session = kagi_git::session::RepoSession::open(repo_path).ok();
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
    let close_owner = kagi.downgrade();
    window.on_window_should_close(cx, move |_, cx| {
        close_owner
            .update(cx, |app, cx| !app.hold_host_close(cx))
            .unwrap_or(true)
    });
    if let Some(fh) = kagi.read(cx).root_focus.clone() {
        window.focus(&fh, cx);
    }
    kagi
}

/// Issue #547: the laid-out bounds of the footer's message element after the
/// last draw, in window coordinates. The footer is a fixed 22 px
/// `items_center()` row, so a message that wraps is centre-clipped and the
/// first line — the op name and reason — is what gets hidden; the scenario
/// asserts these bounds stay one line inside the bar.
#[cfg(feature = "gui-e2e")]
pub fn footer_message_bounds() -> gpui::Bounds<gpui::Pixels> {
    super::render_status::footer_message_bounds()
}

/// Issue #468: push a synthetic `Failed` op-log entry onto the panel, through
/// the same `OpLogPanel::push` the real `record_op` path uses. Part of the seam
/// because `kagi_git::oplog::OpLogEntry` is a normal dep the runner cannot name.
pub fn push_failed_op(app: &KagiApp, op: &str, error: String, cx: &mut App) {
    let Some(panel) = app.op_log.clone() else {
        return;
    };
    let state = |head: &str| kagi_git::ops::StateSummary {
        head: head.to_string(),
        dirty: "clean".to_string(),
    };
    let entry = kagi_git::oplog::OpLogEntry::new(
        op,
        "e2e",
        state("HEAD \u{2192} main"),
        kagi_git::oplog::OpOutcome::Failed { error },
    );
    panel.update(cx, |panel, cx| {
        panel.push(entry);
        cx.notify();
    });
}

/// Issue #473/#476: put a **linked worktree's** commit panel up, exactly as
/// clicking its WIP row does — minus the two `gpui_component::InputState`s.
///
/// The runner cannot afford those: `InputState::new` registers an App-level
/// observer that holds a strong handle to itself, so the entity outlives the
/// window and gpui's end-of-run leak detector fails the whole run. Everything
/// slice 1–2 is about (the panel's `repo_path` + `foreign` marking, and the
/// write ops resolving through them) is set up by `attach_commit_panel_at`;
/// with no inputs, the commit message is read from `state.commit_msg` — the
/// same fallback the headless `KAGI_COMMIT_MSG` path uses.
pub fn open_worktree_panel_no_inputs(
    app: &mut KagiApp,
    path: std::path::PathBuf,
    label: &str,
    color_idx: usize,
    cx: &mut gpui::Context<KagiApp>,
) {
    app.attach_commit_panel_at(path, Some((label.to_string().into(), color_idx)), cx);
}

/// Set the commit panel's message without touching an `InputState` (see
/// [`open_worktree_panel_no_inputs`]) — the `state.commit_msg` fallback.
pub fn set_commit_message(app: &KagiApp, msg: &str, cx: &mut App) {
    if let Some(panel) = app.commit_panel.clone() {
        panel.update(cx, |v, _| v.state.commit_msg = msg.to_string());
    }
}

/// `(op, repo)` of an op-log entry — the runner cannot name `OpLogEntry`, but
/// it can pass one out of `OpLogPanel::entries()` (as `entry_clipboard_text`
/// already does). #476: the oplog's `repo` must be the repository the write
/// actually went into.
pub fn entry_op_and_repo(entry: &kagi_git::oplog::OpLogEntry) -> (String, String) {
    (entry.op.clone(), entry.repo.clone())
}

/// `(op, repo)` of the newest **persisted** oplog entry (`$KAGI_LOG_DIR`), the
/// one `Backend::run` writes. `None` when the log is empty.
pub fn latest_persisted_op() -> Option<(String, String)> {
    kagi_git::oplog::read_oplog_tail(1)
        .first()
        .map(entry_op_and_repo)
}

/// The after-state's `dirty` line of an op-log entry, or `None` for an outcome
/// that carries no after-state (`Failed` / `Refused`).
///
/// #476 slice 3: for a discard this is `discarded N file(s); backup: <path>=<blob>`
/// — the ODB backup SHAs that are the user's only handle on the overwritten
/// content (ADR-0083).
pub fn entry_after_dirty(entry: &kagi_git::oplog::OpLogEntry) -> Option<String> {
    match &entry.outcome {
        kagi_git::oplog::OpOutcome::Success { after }
        | kagi_git::oplog::OpOutcome::Partial { after, .. } => Some(after.dirty.clone()),
        _ => None,
    }
}

/// The entry the tab's Cmd+Z would apply next, as `(branch, after SHA,
/// summary)` — `None` when there is nothing to undo.
///
/// `record` pushes at the cursor and advances past it, so a newly-recorded
/// entry IS this one: checking the undo head is enough to catch a foreign op
/// that leaked onto the stack.
///
/// #476 slice 3: `operation_history` is per tab and `head_branch_and_sha`
/// reads the TAB's HEAD, so an op that ran in a linked worktree would land
/// here as *the tab's branch* pointed at *the worktree's* commit — Cmd+Z would
/// then move the tab's branch onto a commit from another working tree. The
/// invariant the runner asserts is therefore about the SHAs: no entry may
/// reference a commit a worktree write produced. (Equality against a snapshot
/// would not do: `reload` legitimately seeds this stack from the tab's own
/// branch reflog whenever it is empty — ADR-0084.)
pub fn undo_head(app: &KagiApp) -> Option<(String, String, String)> {
    app.operation_history
        .peek_undo()
        .map(|e| (e.branch.clone(), e.after.0.clone(), e.summary.clone()))
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

/// Exact text consumed by the real busy snackbar renderer (#607).
#[cfg(feature = "gui-e2e")]
pub fn busy_snackbar_label(app: &KagiApp) -> Option<&'static str> {
    app.busy_snackbar_label()
}
