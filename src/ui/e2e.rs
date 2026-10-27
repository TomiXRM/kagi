//! ADR-0166 — GUI E2E seam (Lane 1, macOS native).
//!
//! The minimal, dependency-hiding entry points a `harness = false` main-thread
//! runner (`tests/gui_e2e_runner.rs`) needs to mount the real [`KagiApp`]
//! offscreen against a fixture repo.
//!
//! Instrumentation and deferred report transports are gated by `gui-e2e`.
//! Plain helpers may compile in normal builds, but must not require
//! `gpui/test-support`; the native runner opts into that dependency explicitly.

#[cfg(feature = "gui-e2e")]
pub use super::home_clone_card::{
    clone_destination_presentation, take_clone_copy_activations, CloneDestinationPresentation,
};

/// Exercise the real Copy all projection without adding a button to input cards.
#[cfg(feature = "gui-e2e")]
pub fn plan_clipboard_text_for_e2e(plan: &kagi_git::OperationPlan) -> String {
    super::modal_copy::plan_clipboard_text(plan, &[], kagi_domain::plan_note::ShellKind::Posix)
}

use std::cell::RefCell;
use std::path::Path;
pub mod commit_panel_labels;
pub mod palette;
pub mod sidebar_labels;
#[cfg(feature = "gui-e2e")]
pub mod system_accessibility;

#[cfg(feature = "gui-e2e")]
pub mod activity;
#[cfg(feature = "gui-e2e")]
pub mod inspector_metadata;
#[cfg(feature = "gui-e2e")]
pub mod tab_strip;

#[cfg(feature = "gui-e2e")]
pub mod compact_rail;

#[cfg(feature = "gui-e2e")]
pub mod text_caps;

#[cfg(feature = "gui-e2e")]
pub mod plan_subjects;

#[cfg(feature = "gui-e2e")]
pub mod worktree_inspection;

#[cfg(feature = "gui-e2e")]
pub mod diff_presentation;

#[cfg(feature = "gui-e2e")]
pub mod diff_controls;

#[cfg(feature = "gui-e2e")]
pub mod diff_content;

#[cfg(feature = "gui-e2e")]
pub mod pr_detail;

#[cfg(feature = "gui-e2e")]
pub fn issue_thread_list(
    app: &super::KagiApp,
    cx: &gpui::App,
) -> kagi_ui_core::variable_list::VariableListState {
    app.selected_issue_conversation()
        .map(|owner| owner.read(cx).list.clone())
        .unwrap_or_else(|| app.ui().issue_thread_pending_list.clone())
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_plan_command_disclosure(id: &str) -> Option<(String, String, bool)> {
    super::modal_command::recorded_disclosure(id)
}

#[cfg(feature = "gui-e2e")]
pub mod unborn;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static STATUS_CONTROLS: RefCell<std::collections::HashMap<&'static str, (gpui::Role, &'static str, gpui::Toggled)>> = RefCell::new(Default::default());
    static STATUS_BRANCH_IDENTITY: RefCell<Option<(gpui::SharedString, gpui::SharedString)>> = const { RefCell::new(None) };
    static STATUS_CHIPS: RefCell<Vec<(String, Option<String>, Option<String>)>> = const { RefCell::new(Vec::new()) };
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn record_status_control(
    id: &'static str,
    role: gpui::Role,
    label: &'static str,
    toggled: gpui::Toggled,
) {
    STATUS_CONTROLS.with(|map| map.borrow_mut().insert(id, (role, label, toggled)));
}

#[cfg(feature = "gui-e2e")]
pub fn status_control(id: &str) -> Option<(gpui::Role, &'static str, gpui::Toggled)> {
    STATUS_CONTROLS.with(|map| map.borrow().get(id).copied())
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn record_status_branch_identity(
    label: gpui::SharedString,
    tooltip: gpui::SharedString,
) {
    STATUS_BRANCH_IDENTITY.with(|identity| *identity.borrow_mut() = Some((label, tooltip)));
}

/// Renderer-fed full AX name and tooltip for the status bar's branch.
#[cfg(feature = "gui-e2e")]
pub fn status_branch_identity() -> Option<(gpui::SharedString, gpui::SharedString)> {
    STATUS_BRANCH_IDENTITY.with(|identity| identity.borrow().clone())
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn record_status_chips(chips: Vec<(String, Option<String>, Option<String>)>) {
    STATUS_CHIPS.with(|value| *value.borrow_mut() = chips);
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn record_status_chip(text: String, label: String, tooltip: String) {
    STATUS_CHIPS.with(|value| value.borrow_mut().push((text, Some(label), Some(tooltip))));
}

#[cfg(feature = "gui-e2e")]
pub fn status_chips() -> Vec<(String, Option<String>, Option<String>)> {
    STATUS_CHIPS.with(|value| value.borrow().clone())
}

#[cfg(feature = "gui-e2e")]
#[derive(Clone, Copy, Debug)]
pub struct ConflictDraftGuardPresentation {
    pub title: &'static str,
    pub retry: &'static str,
    pub discard: &'static str,
    pub detail: &'static str,
    pub editor_count: usize,
    pub saving: bool,
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static CONFLICT_DRAFT_GUARD: RefCell<Option<ConflictDraftGuardPresentation>> = const { RefCell::new(None) };
}

/// Observe the labels actually selected by the conflict draft guard renderer.
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_conflict_draft_guard(presentation: ConflictDraftGuardPresentation) {
    CONFLICT_DRAFT_GUARD.with(|value| *value.borrow_mut() = Some(presentation));
}

#[cfg(feature = "gui-e2e")]
pub fn take_conflict_draft_guard() -> Option<ConflictDraftGuardPresentation> {
    CONFLICT_DRAFT_GUARD.with(|value| value.borrow_mut().take())
}

/// #707 review: the revision-laundering and cross-session regressions need to
/// produce a detector payload for one state and land it against another.
pub use super::operations::conflict_detect::detect_payload_for_test;
/// #704: the stale-detector regression applies a detector outcome by hand, to
/// stand in for a job that started before the read the tab has now accepted.
/// `ui::operations` is private, so the seam is here with the rest.
pub use super::operations::conflict_detect::ConflictDetectOutcome;

#[cfg(feature = "gui-e2e")]
pub fn menu_is_front(app: &KagiApp, cx: &gpui::App) -> bool {
    app.front_layer(cx) == super::front_layer::FrontLayer::Menu
}

/// Put the active tab's pane pass in the state where it waits on a conflict
/// detection against the accepted read (#722).
pub fn await_conflict_revalidation(app: &mut KagiApp) {
    if let Some(ui) = app.ui_mut() {
        ui.pane_revalidation = super::tab_ui_state_ops::PaneRevalidation::AwaitingConflict;
    }
}

/// Read-only witnesses for superseded restore admission, without exposing the
/// production fields or manufacturing detector outcomes.
#[cfg(feature = "gui-e2e")]
pub fn conflict_detector_generation(app: &KagiApp) -> u64 {
    app.ui().conflict_detect_generation
}

#[cfg(feature = "gui-e2e")]
pub fn conflict_saved_review_needed(view: &super::conflict_view::ConflictView) -> bool {
    view.saved_review_needed
}
use std::rc::Rc;
use std::sync::Arc;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static CONFIRM_BOUNDS: RefCell<std::collections::HashMap<gpui::WindowId, gpui::Bounds<gpui::Pixels>>> = RefCell::new(Default::default());
    static CONTROL_BOUNDS: RefCell<std::collections::HashMap<(gpui::WindowId, String), gpui::Bounds<gpui::Pixels>>> = RefCell::new(Default::default());
    static CONTROL_PAINTS: RefCell<std::collections::HashMap<(gpui::WindowId, String), ControlPaint>> = RefCell::new(Default::default());
    static CONTROL_PAINT_ORDER: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static TAB_LOAD_LIMITS: RefCell<std::collections::HashMap<crate::app::SessionId, usize>> = RefCell::new(Default::default());
    static EDITOR_DIFF_SEEDS: RefCell<std::collections::HashMap<gpui::EntityId, u64>> = RefCell::new(Default::default());
    static BUSY_ADVICE: RefCell<Option<String>> = const { RefCell::new(None) };
    static SETTINGS_ZOOM_LABEL: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static CHROME_BORDERS: RefCell<std::collections::HashMap<&'static str, gpui::Hsla>> = RefCell::new(Default::default());
}

/// Read the actual rendered element's explicit boundary role, not a theme prediction.
#[cfg(feature = "gui-e2e")]
pub fn chrome_border(name: &'static str) -> Option<gpui::Hsla> {
    CHROME_BORDERS.with(|map| map.borrow().get(name).copied())
}

pub(crate) fn probe_chrome_border<E: gpui::Styled>(name: &'static str, mut element: E) -> E {
    #[cfg(feature = "gui-e2e")]
    if let Some(color) = element.style().border_color {
        CHROME_BORDERS.with(|map| map.borrow_mut().insert(name, color));
    }
    #[cfg(not(feature = "gui-e2e"))]
    let _ = (name, &mut element);
    element
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static PR_MENU_COPY_HOVERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(feature = "gui-e2e")]
pub fn pr_menu_copy_hovered() -> bool {
    PR_MENU_COPY_HOVERED.with(std::cell::Cell::get)
}

pub(crate) fn probe_pr_menu_copy(control: gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div> {
    #[cfg(feature = "gui-e2e")]
    {
        use gpui::StatefulInteractiveElement as _;
        control.on_hover(|hovered, _, _| {
            PR_MENU_COPY_HOVERED.with(|value| value.set(*hovered));
        })
    }
    #[cfg(not(feature = "gui-e2e"))]
    control
}
/// #354: the toolbar's AccessKit-disabled inputs, for the GUI E2E oracle.
#[cfg(feature = "gui-e2e")]
pub use super::render_header::toolbar_a11y::{
    clear_toolbar_unavailable, take_focus_probe, toolbar_caption_and_tooltip, toolbar_description,
    toolbar_label, toolbar_unavailable,
};
#[cfg(feature = "gui-e2e")]
pub fn last_plan_status_chip() -> Option<String> {
    super::modal_renderers::status_chip_probe::last()
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
pub(crate) fn record_control_bounds(
    id: gpui::WindowId,
    name: &str,
    bounds: gpui::Bounds<gpui::Pixels>,
) {
    CONTROL_BOUNDS.with(|map| map.borrow_mut().insert((id, name.to_string()), bounds));
}
/// Wheel events that reached the workspace root (the panes behind overlays).
static WORKSPACE_SCROLLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) fn note_workspace_scroll() {
    WORKSPACE_SCROLLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}
#[cfg(feature = "gui-e2e")]
pub fn workspace_scrolls() -> usize {
    WORKSPACE_SCROLLS.load(std::sync::atomic::Ordering::Relaxed)
}
/// Times Home's GitHub list entries were built (#937): once per change of
/// what they are built from, not per frame.
static HOME_ITEM_BUILDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) fn note_home_items_built() {
    HOME_ITEM_BUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}
#[cfg(feature = "gui-e2e")]
pub fn home_item_builds() -> usize {
    HOME_ITEM_BUILDS.load(std::sync::atomic::Ordering::Relaxed)
}
#[cfg(feature = "gui-e2e")]
pub fn control_bounds(id: gpui::WindowId, name: &str) -> Option<gpui::Bounds<gpui::Pixels>> {
    CONTROL_BOUNDS.with(|map| map.borrow().get(&(id, name.to_string())).copied())
}

/// Actual paint-stage coverage and traversal order, independent of layout bounds.
#[cfg(feature = "gui-e2e")]
#[derive(Clone, Copy, Debug)]
pub struct ControlPaint {
    pub bounds: gpui::Bounds<gpui::Pixels>,
    pub mask: gpui::Bounds<gpui::Pixels>,
    pub order: u64,
    /// Resolved inherited text size at the actual paint stage.
    pub font_size: gpui::Pixels,
    pub font_weight: gpui::FontWeight,
    pub text_color: gpui::Hsla,
    pub ellipsis: bool,
    pub middle_ellipsis: bool,
}

#[cfg(feature = "gui-e2e")]
pub fn control_paint(id: gpui::WindowId, name: &str) -> Option<ControlPaint> {
    CONTROL_PAINTS.with(|map| map.borrow().get(&(id, name.to_string())).copied())
}

#[cfg(feature = "gui-e2e")]
fn record_control_paint(name: &str, bounds: gpui::Bounds<gpui::Pixels>, window: &Window) {
    let order = CONTROL_PAINT_ORDER.with(|value| {
        let next = value.get() + 1;
        value.set(next);
        next
    });
    let paint = ControlPaint {
        bounds,
        mask: window.content_mask().bounds,
        order,
        font_size: window.text_style().font_size.to_pixels(window.rem_size()),
        font_weight: window.text_style().font_weight,
        text_color: window.text_style().color,
        ellipsis: matches!(
            window.text_style().text_overflow.as_ref(),
            Some(gpui::TextOverflow::Truncate(affix)) if affix.as_ref() == "…"
        ),
        middle_ellipsis: matches!(
            window.text_style().text_overflow.as_ref(),
            Some(gpui::TextOverflow::TruncateMiddle(affix)) if affix.as_ref() == "…"
        ),
    };
    CONTROL_PAINTS.with(|map| {
        map.borrow_mut()
            .insert((window.window_handle().window_id(), name.to_owned()), paint);
    });
}
/// Clear a bound so the next draw proves presence rather than reusing the previous frame.
#[cfg(feature = "gui-e2e")]
pub fn clear_control_bounds(id: gpui::WindowId, name: &str) {
    CONTROL_BOUNDS.with(|map| map.borrow_mut().remove(&(id, name.to_string())));
    CONTROL_PAINTS.with(|map| map.borrow_mut().remove(&(id, name.to_string())));
}
#[cfg(feature = "gui-e2e")]
thread_local! {
    static GRAPH_AGE_TEXT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_graph_age_text(text: &str) {
    GRAPH_AGE_TEXT.with(|slot| *slot.borrow_mut() = Some(text.to_owned()));
}
#[cfg(feature = "gui-e2e")]
pub fn take_graph_age_text() -> Option<String> {
    GRAPH_AGE_TEXT.with(|slot| slot.borrow_mut().take())
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_busy_advice(text: &str) {
    BUSY_ADVICE.with(|slot| *slot.borrow_mut() = Some(text.to_owned()));
}
#[cfg(feature = "gui-e2e")]
pub fn take_busy_advice() -> Option<String> {
    BUSY_ADVICE.with(|slot| slot.borrow_mut().take())
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_settings_zoom_label(label: &str) {
    SETTINGS_ZOOM_LABEL.with(|slot| *slot.borrow_mut() = Some(label.to_owned()));
}
#[cfg(feature = "gui-e2e")]
pub fn settings_zoom_label() -> Option<String> {
    SETTINGS_ZOOM_LABEL.with(|slot| slot.borrow().clone())
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_tab_load_commit_limit(session: crate::app::SessionId, limit: usize) {
    TAB_LOAD_LIMITS.with(|limits| {
        limits.borrow_mut().insert(session, limit);
    });
}
#[cfg(feature = "gui-e2e")]
pub fn tab_load_commit_limit(session: crate::app::SessionId) -> Option<usize> {
    TAB_LOAD_LIMITS.with(|limits| limits.borrow().get(&session).copied())
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_editor_diff_seed_request(editor: gpui::EntityId, req: u64) {
    EDITOR_DIFF_SEEDS.with(|seeds| {
        seeds.borrow_mut().insert(editor, req);
    });
}
/// Last accepted WIP-diff request, recorded only after the Editor's admission
/// checks and typed installation. A previously loaded diff is not a receipt.
#[cfg(feature = "gui-e2e")]
pub fn editor_diff_seed_request(editor: gpui::EntityId) -> Option<u64> {
    EDITOR_DIFF_SEEDS.with(|seeds| seeds.borrow().get(&editor).copied())
}
/// Measure a rendered control, including component probes in the native runner.
pub fn measure_control(
    name: impl Into<String>,
    control: impl gpui::IntoElement,
) -> gpui::AnyElement {
    #[cfg(feature = "gui-e2e")]
    use gpui::{IntoElement as _, ParentElement as _};
    #[cfg(feature = "gui-e2e")]
    {
        let name = name.into();
        let paint_name = name.clone();
        gpui::div()
            .relative()
            .child(
                gpui::canvas(
                    move |bounds, window, _| {
                        record_control_bounds(window.window_handle().window_id(), &name, bounds);
                    },
                    move |bounds, _, window, _| record_control_paint(&paint_name, bounds, window),
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

#[cfg(feature = "gui-e2e")]
mod measured_leaf;

/// Record the leaf's outer bounds without introducing a layout/AX wrapper.
pub(crate) fn measure_leaf(
    name: impl Into<String>,
    control: impl gpui::IntoElement,
) -> gpui::AnyElement {
    #[cfg(feature = "gui-e2e")]
    {
        use gpui::IntoElement as _;
        measured_leaf::MeasuredLeaf {
            name: name.into(),
            control: control.into_any_element(),
            confirm: false,
        }
        .into_any_element()
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        let _ = name;
        control.into_any_element()
    }
}
/// Record a container's own bounds from *inside* it: an absolute, full-size
/// canvas to add as one more child of an already-sized element.
///
/// `measure_control` wraps its control in an auto-sized `relative` div, which
/// is right for a button or a row but wrong for a pane whose children size
/// themselves with `h_full` / `flex_1`: the percentage then resolves against
/// the wrapper's *content* height, and a virtualized list inside the PR page
/// was handed a 16px viewport (test-only, but it made the scenario blind).
pub(crate) fn measure_inside(name: impl Into<String>) -> gpui::AnyElement {
    #[cfg(feature = "gui-e2e")]
    use gpui::IntoElement as _;
    #[cfg(feature = "gui-e2e")]
    {
        let name = name.into();
        let paint_name = name.clone();
        gpui::canvas(
            move |bounds, window, _| {
                CONTROL_BOUNDS.with(|map| {
                    map.borrow_mut()
                        .insert((window.window_handle().window_id(), name.clone()), bounds);
                });
            },
            move |bounds, _, window, _| record_control_paint(&paint_name, bounds, window),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        let _ = name;
        use gpui::IntoElement as _;
        measure_inside_placeholder().into_any_element()
    }
}

#[cfg(any(test, not(feature = "gui-e2e")))]
/// Production probes must not become flex items or introduce gaps.
fn measure_inside_placeholder() -> gpui::Div {
    use gpui::Styled as _;
    gpui::div().absolute().size_0()
}

#[cfg(test)]
mod measure_inside_tests {
    #[test]
    fn measure_inside_placeholder_is_out_of_flow() {
        use gpui::Styled as _;
        let mut placeholder = super::measure_inside_placeholder();
        assert_eq!(placeholder.style().position, Some(gpui::Position::Absolute));
        let mut zero = gpui::div().size_0();
        assert_eq!(placeholder.style().size, zero.style().size);
    }
}

pub(crate) fn measure_confirm(button: impl gpui::IntoElement) -> gpui::AnyElement {
    #[cfg(feature = "gui-e2e")]
    use gpui::IntoElement as _;
    #[cfg(feature = "gui-e2e")]
    {
        measured_leaf::MeasuredLeaf {
            name: "plan-confirm".into(),
            control: button.into_any_element(),
            confirm: true,
        }
        .into_any_element()
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        button.into_any_element()
    }
}

use gpui::{App, AppContext as _, AssetSource, Entity, Platform, Styled as _, Window};

use super::assets::KagiAssets;
use super::{fonts, theme, toast_stack, KagiApp, Msg, ToastKind};

#[cfg(feature = "gui-e2e")]
pub fn app_notice_message(app: &KagiApp) -> Option<&str> {
    app.app_notice().map(|notice| notice.message.as_str())
}

#[cfg(feature = "gui-e2e")]
pub fn queued_notice_contains(app: &KagiApp, needle: &str) -> bool {
    app.app_notices
        .iter()
        .any(|notice| notice.message.contains(needle))
}

#[cfg(feature = "gui-e2e")]
pub fn host_close_guard_count(app: &KagiApp) -> usize {
    usize::from(
        app.app_notice()
            .is_some_and(|notice| notice.host_close_guard),
    ) + app
        .app_notices
        .iter()
        .filter(|notice| notice.host_close_guard)
        .count()
}

#[cfg(feature = "gui-e2e")]
pub fn set_remote_browse_host_input(
    app: &mut KagiApp,
    value: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let input = app
        .remote_browse()
        .and_then(|modal| modal.host_state.clone())
        .expect("Remote Browse host input must be rendered");
    input.update(cx, |state, cx| state.set_value(value, window, cx));
    app.remote_browse_mut()
        .expect("Remote Browse must still own the slot")
        .host_input = value.to_string();
}
#[cfg(feature = "gui-e2e")]
pub fn deliver_app_notice(app: &mut KagiApp, message: &str) {
    app.app_notices.push_back(message.to_string().into());
    app.present_app_notice();
}
#[cfg(feature = "gui-e2e")]
pub fn deliver_reconcile_notice(app: &mut KagiApp, id: crate::app::OperationId) {
    let mut notice = super::modals::AppNotice::from("Unconfirmed remote operation".to_string());
    notice.inspect = Some(id);
    app.app_notices.push_back(notice);
    app.present_app_notice();
}
#[cfg(feature = "gui-e2e")]
pub fn present_app_notice(app: &mut KagiApp) {
    app.present_app_notice();
}
#[cfg(feature = "gui-e2e")]
pub fn deliver_acknowledge_notice(app: &mut KagiApp, message: &str) -> bool {
    let Some(session) = app.active_session() else {
        return false;
    };
    let Some(read) = crate::app::seed_acknowledge_for_test(&mut app.app_sessions, session) else {
        return false;
    };
    app.app_notices.push_back(super::modals::AppNotice {
        message: message.to_string(),
        inspect: None,
        acknowledge: Some(read),
        release_armed: false,
        host_close_guard: false,
    });
    app.present_app_notice();
    true
}

#[cfg(feature = "gui-e2e")]
pub fn enqueue_checkout(app: &mut KagiApp, branch: &str, cx: &mut gpui::Context<KagiApp>) -> bool {
    app.enqueue_checkout(
        &super::modals::CheckoutPlanTarget::Branch(branch.into()),
        cx,
    )
}

#[cfg(feature = "gui-e2e")]
pub fn app_notice_is_acknowledgeable(app: &KagiApp) -> bool {
    app.app_notice()
        .is_some_and(|notice| notice.acknowledge.is_some())
}

#[cfg(feature = "gui-e2e")]
pub fn seed_diff_selection() {
    let key = super::diff_selection::SelectionKey::new(
        super::diff_selection::SelectionOwner::Test,
        Default::default(),
        Default::default(),
    );
    super::diff_selection::begin(key, 0);
    super::diff_selection::set_text(key, "selected".to_string());
}

#[cfg(feature = "gui-e2e")]
pub fn diff_selection_present() -> bool {
    super::diff_selection::has_selection()
}
#[cfg(feature = "gui-e2e")]
pub fn begin_update_install_for_test(app: &mut KagiApp) -> bool {
    app.begin_update_install()
}

#[cfg(feature = "gui-e2e")]
pub fn fail_update_install_for_test(app: &mut KagiApp, error: &str) {
    app.finish_update_install_failure(error);
}

/// Queued installer completion keeps native tests off the network and away
/// from the running application's installation directory.
#[cfg(feature = "gui-e2e")]
type UpdateInstallTask = gpui::Task<Result<crate::update::Relaunch, String>>;
#[cfg(feature = "gui-e2e")]
thread_local! {
    static UPDATE_INSTALL: RefCell<Option<UpdateInstallTask>> = const { RefCell::new(None) };
}
#[cfg(feature = "gui-e2e")]
pub fn queue_update_install(task: UpdateInstallTask) {
    UPDATE_INSTALL.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn take_update_install() -> Option<UpdateInstallTask> {
    UPDATE_INSTALL.with(|slot| slot.borrow_mut().take())
}
#[cfg(feature = "gui-e2e")]
pub fn update_install_queued() -> bool {
    UPDATE_INSTALL.with(|slot| slot.borrow().is_some())
}
#[cfg(feature = "gui-e2e")]
pub fn discard_queued_update_install() {
    drop(take_update_install());
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
    #[cfg(all(target_os = "macos", feature = "gui-e2e"))]
    cx.set_global(super::settings_view::SettingsHostForE2e::default());
}

/// Run the production Dock-reopen decision/session restore without activating
/// a visible window; callers mount the returned state through the normal root.
pub fn dock_reopen_state(cx: &mut App) -> Option<KagiApp> {
    super::dock_reopen_state(cx)
}

/// Explicit opt-in interception of Quit for native restart regressions. The
/// normal key/action dispatch and quit-time settings flush still execute.
#[cfg(feature = "gui-e2e")]
#[derive(Default)]
pub struct QuitForE2e {
    pub requested: bool,
}
#[cfg(feature = "gui-e2e")]
impl gpui::Global for QuitForE2e {}

#[cfg(feature = "gui-e2e")]
pub(super) fn capture_app_quit(cx: &mut App) -> bool {
    if cx.try_global::<QuitForE2e>().is_none() {
        return false;
    }
    // Mirrors the unconditional final on_app_quit preference observer.
    super::settings::flush();
    cx.global_mut::<QuitForE2e>().requested = true;
    true
}

#[cfg(feature = "gui-e2e")]
#[derive(Default)]
pub struct RelaunchForE2e {
    pub requested: bool,
}
#[cfg(feature = "gui-e2e")]
impl gpui::Global for RelaunchForE2e {}

#[cfg(feature = "gui-e2e")]
pub(super) fn capture_update_relaunch(cx: &mut App) -> bool {
    if cx.try_global::<RelaunchForE2e>().is_none() {
        return false;
    }
    super::settings::flush();
    cx.global_mut::<RelaunchForE2e>().requested = true;
    true
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
    let session = kagi_git::session::RepoSession::open(repo_path).ok();
    if let Some(ui) = app.ui_mut() {
        ui.repo_session = session;
    }
    Ok(app)
}

/// The `KagiApp` entity construction shared with `open_main_window`: the root
/// focus handle, toast stack, op-log panel and Settings theme picker — the
/// parts that need a `cx` / `Window`. Extracted so the offscreen mount and the
/// real window build the entity the exact same way (ADR-0166).
pub fn build_kagi_entity(
    mut app_state: KagiApp,
    window: &mut Window,
    cx: &mut App,
) -> Entity<KagiApp> {
    super::window_menu::register_main_window(window, cx);
    let kagi: Entity<KagiApp> = cx.new(|cx: &mut gpui::Context<KagiApp>| {
        app_state.root_focus = Some(cx.focus_handle());
        app_state.sidebar.filter = Some(cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx)
                .placeholder(super::i18n::Msg::HomeGithubFilter.t())
        }));
        if let Some(input) = &app_state.sidebar.filter {
            cx.observe(input, |_, _, cx| cx.notify()).detach();
        }
        app_state.toast_stack = Some(cx.new(|_| toast_stack::ToastStack::new()));
        if let Some(stack) = app_state.toast_stack.as_ref() {
            cx.observe(stack, |_, _, cx| cx.notify()).detach();
            cx.subscribe_in(
                stack,
                window,
                |app: &mut KagiApp, _, removed: &toast_stack::ToastRemoved, window, cx| {
                    if let Some(root) = app.root_focus.as_ref() {
                        let closed_focused = removed.was_focused(window);
                        if closed_focused
                            || window.focused(cx).is_none()
                            || !root.contains_focused(window, cx)
                        {
                            root.focus(window, cx);
                        }
                    }
                },
            )
            .detach();
        }
        if let Some(stack) = app_state.toast_stack.as_ref() {
            cx.subscribe(
                stack,
                |app, stack, request: &toast_stack::ToastActionRequested, cx| {
                    let opened = match &request.action {
                        super::types::ToastAction::OpenRepository(path) => {
                            app.open_repository(path.clone(), cx)
                        }
                        super::types::ToastAction::OpenOperationLog(entry) => {
                            app.open_operation_result(entry, cx)
                        }
                        super::types::ToastAction::RefreshRepository(owner) => {
                            let index = app.tabs.iter().position(|tab| {
                                tab.session == owner.session && tab.path == owner.path
                            });
                            if let Some(index) = index {
                                let left = app.home_yields_to_repository();
                                app.return_to_tab(index, left, cx);
                            }
                            let attached = app.app_sessions.attachment(owner.session);
                            if index.is_some()
                                && app.active_session() == Some(owner.session)
                                && app.repo_path.as_ref() == Some(&owner.path)
                                && attached.as_ref().is_some_and(|current| {
                                    current.path == owner.path && current.worktree == owner.worktree
                                })
                            {
                                app.reload_manual(cx);
                            } else {
                                app.push_toast(
                                    ToastKind::Error,
                                    Msg::conflict_draft_refresh_unavailable(),
                                    cx,
                                );
                            }
                            false
                        }
                    };
                    let recovery_details = stack
                        .read(cx)
                        .toasts()
                        .iter()
                        .any(|toast| toast.id == request.id && toast.refresh_owner.is_some());
                    if opened && !recovery_details {
                        stack.update(cx, |stack, cx| stack.begin_exit(request.id, cx));
                    }
                },
            )
            .detach();
        }
        let seed = std::mem::take(&mut app_state.op_log_seed);
        app_state.op_log = Some(super::operations::oplog_restore::op_log_panel(seed, cx));
        #[cfg(target_os = "macos")]
        cx.observe_window_activation(window, |app, window, cx| {
            if window.is_window_active() {
                app.publish_window_menu_state(window, cx);
                cx.notify();
            }
            if window.is_window_active() && theme::refresh_system_accent() {
                theme::sync_gpui_component_theme(cx);
                app.seed_settings(cx);
                cx.refresh_windows();
                cx.notify();
            }
        })
        .detach();
        app_state
    });
    let close_owner = kagi.downgrade();
    window.on_window_should_close(cx, move |window, cx| {
        close_owner
            .update(cx, |app, cx| {
                !app.hold_host_close(
                    super::EditorPendingIntent::CloseWindow(window.window_handle()),
                    cx,
                )
            })
            .unwrap_or(true)
    });
    super::settings_themes::install(&kagi, window, cx);
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

#[cfg(feature = "gui-e2e")]
pub fn footer_display_text() -> gpui::SharedString {
    super::render_status::footer_display_text()
}

/// Drive the persisted-record presentation with an already-observed outcome.
/// The append itself remains real, including lock contention and failure.
pub fn record_persisted_op(
    app: &mut KagiApp,
    repo: &std::path::Path,
    outcome: kagi_git::oplog::OpOutcome,
    cx: &mut gpui::Context<KagiApp>,
) {
    let state = |dirty: &str| kagi_git::ops::StateSummary {
        head: "branch: main".to_string(),
        dirty: dirty.to_string(),
    };
    app.record_op_persist("e2e-record", state("unchanged"), outcome, repo, cx);
}

/// Present a recorded stopped-operation fixture through the production completion owner.
/// The receipt append is real; the fixture supplies the already-observed outcome.
pub fn present_recorded_info_result(app: &mut KagiApp, cx: &mut gpui::Context<KagiApp>) -> u64 {
    let repo = app
        .repo_path
        .as_ref()
        .or_else(|| app.tabs.first().map(|tab| &tab.path))
        .expect("recorded-result fixture requires an opened repository");
    let state = kagi_git::ops::StateSummary {
        head: "branch: main".into(),
        dirty: "unchanged".into(),
    };
    let entry = kagi_git::oplog::OpLogEntry::new(
        "merge",
        repo.display().to_string(),
        state.clone(),
        kagi_git::oplog::OpOutcome::Success { after: state },
    );
    let recording = kagi_git::backend::recording::finalize(entry);
    assert!(matches!(
        recording,
        kagi_git::backend::recording::Recording::Appended { .. }
    ));
    assert!(!app.present_operation_completion(&recording, false, true, None, None, cx));
    app.toast_stack
        .as_ref()
        .expect("mounted toast stack")
        .read(cx)
        .toasts()
        .last()
        .expect("stopped completion produces a result")
        .id
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

/// The ordinary (non-foreign) commit panel, inputs-free — the local-repo
/// counterpart of [`open_worktree_panel_no_inputs`], same `InputState` caveat.
pub fn open_local_panel_no_inputs(
    app: &mut KagiApp,
    path: std::path::PathBuf,
    cx: &mut gpui::Context<KagiApp>,
) {
    app.attach_commit_panel_at(path, None, cx);
}

/// Set the commit panel's message without touching an `InputState` (see
/// [`open_worktree_panel_no_inputs`]) — the `state.commit_msg` fallback.
pub fn set_commit_message(app: &KagiApp, msg: &str, cx: &mut App) {
    if let Some(panel) = app.ui().commit_panel.clone() {
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
    app.ui()
        .operation_history
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

/// Static write/planning snackbar label (#607); clone's source-named label has its own renderer probe.
#[cfg(feature = "gui-e2e")]
pub fn busy_snackbar_label(app: &KagiApp) -> Option<&'static str> {
    app.busy_snackbar_label()
}

/// Retained focus of the actual rendered progress close Button.
#[cfg(feature = "gui-e2e")]
pub fn busy_dismiss_focus(app: &KagiApp, window: gpui::WindowId) -> Option<gpui::FocusHandle> {
    app.busy_close_focus
        .borrow()
        .iter()
        .find(|(host, _)| *host == window)
        .map(|(_, focus)| focus.clone())
}

/// The one gate every state-changing entry point consults (ADR-0196 Wave 3),
/// so a scenario can assert on it rather than on a field that mirrors it.
#[cfg(feature = "gui-e2e")]
pub fn op_latched(app: &KagiApp) -> bool {
    app.op_latched()
}

/// Whether any foreground modal currently occupies the window-global slot.
#[cfg(feature = "gui-e2e")]
pub fn active_modal_present(app: &KagiApp) -> bool {
    app.active_modal.is_some()
}

/// The sidebar's current gesture offset in rendered pixels (ADR-0199) — the
/// one value its pages are positioned by, so a scenario can assert on the
/// motion itself rather than on bounds the viewport clip hides.
#[cfg(feature = "gui-e2e")]
pub fn sidebar_page_offset(app: &KagiApp) -> f32 {
    app.sidebar.swipe.offset()
}

/// What `render` runs every frame, callable on its own.
///
/// `run_until_parked` cannot stand in for a frame here: it drains the
/// background executor, so an operation held deliberately in flight would have
/// to finish first. #708's P1 is precisely a latch that this call erased while
/// the writer was still running, so the scenario needs the frame without the
/// completion.
#[cfg(feature = "gui-e2e")]
pub fn poll_app_jobs(app: &mut KagiApp, cx: &mut gpui::Context<KagiApp>) {
    app.poll_app_jobs(cx);
}

/// Arm one canned pr-merge terminal.
///
/// Without a GitHub remote every `gh` call fails, including the re-read, so a
/// real merge attempt in a fixture is honestly `Unknown`. These are the two
/// terminals a fixture cannot otherwise reach — the report is the same shape
/// the transport returns, recording included.
#[cfg(feature = "gui-e2e")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrMergeTerminal {
    /// The server refused it: a plain, retryable failure.
    Failed,
    /// Merged, but `--delete-branch` never answered — the receipt is `Partial`,
    /// the lease is released, and nothing but the transport hold stops a
    /// second merge of a PR that is already merged.
    Partial,
}
#[cfg(feature = "gui-e2e")]
pub fn arm_pr_merge_terminal(terminal: PrMergeTerminal) {
    *PR_MERGE_TERMINAL.lock().unwrap() = Some(terminal);
}
#[cfg(feature = "gui-e2e")]
static PR_MERGE_TERMINAL: std::sync::Mutex<Option<PrMergeTerminal>> = std::sync::Mutex::new(None);
#[cfg(feature = "gui-e2e")]
pub(crate) fn pr_merge_terminal_fault(
    repo: &Path,
    number: u64,
    plan: &kagi_git::OperationPlan,
) -> Option<kagi_git::backend::recording::RunReport> {
    let terminal = PR_MERGE_TERMINAL.lock().unwrap().take()?;
    const FAILED: &str = "injected: the server refused the merge";
    const PARTIAL: &str = "injected: merged; branch deletion unconfirmed";
    let (outcome, result) = match terminal {
        PrMergeTerminal::Failed => (
            kagi_git::oplog::OpOutcome::Failed {
                error: FAILED.to_string(),
            },
            Err(kagi_git::GitError::Other(FAILED.to_string())),
        ),
        PrMergeTerminal::Partial => (
            kagi_git::oplog::OpOutcome::Partial {
                after: plan.predicted.clone(),
                error: PARTIAL.to_string(),
            },
            Ok(kagi_git::OperationOutcome::PrMerge {
                number,
                detail: PARTIAL.to_string(),
                confirmed: false,
                local_branch: None,
            }),
        ),
    };
    let entry = kagi_git::oplog::OpLogEntry::new(
        "pr-merge",
        repo.display().to_string(),
        plan.current.clone(),
        outcome,
    );
    Some(kagi_git::backend::recording::RunReport {
        result,
        recording: kagi_git::backend::recording::finalize(entry),
        stash: None,
    })
}

#[cfg(feature = "gui-e2e")]
type EditorHistoryTask = gpui::Task<Result<kagi_domain::file_history::FileHistory, String>>;
#[cfg(feature = "gui-e2e")]
thread_local! {
    static EDITOR_HISTORY: RefCell<Option<EditorHistoryTask>> = const { RefCell::new(None) };
}
#[cfg(feature = "gui-e2e")]
pub fn queue_editor_history(task: EditorHistoryTask) {
    EDITOR_HISTORY.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn take_editor_history() -> Option<EditorHistoryTask> {
    EDITOR_HISTORY.with(|slot| slot.borrow_mut().take())
}

/// Supply only the transport task; dispatch, owner capture and settlement stay real.
#[cfg(feature = "gui-e2e")]
type RemoteRefreshTask = gpui::Task<Result<kagi_git::RepoSnapshot, String>>;
#[cfg(feature = "gui-e2e")]
thread_local! {
    static REMOTE_REFRESH: RefCell<Option<RemoteRefreshTask>> = const { RefCell::new(None) };
}
#[cfg(feature = "gui-e2e")]
pub fn queue_remote_refresh(task: RemoteRefreshTask) {
    REMOTE_REFRESH.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn take_remote_refresh() -> Option<RemoteRefreshTask> {
    REMOTE_REFRESH.with(|slot| slot.borrow_mut().take())
}
#[cfg(feature = "gui-e2e")]
pub type SmartGenerationResult = Option<(String, bool)>;
#[cfg(feature = "gui-e2e")]
type SmartGenerationTask = gpui::Task<SmartGenerationResult>;
#[cfg(feature = "gui-e2e")]
thread_local! {
    static SMART_GENERATION: RefCell<Option<SmartGenerationTask>> = const { RefCell::new(None) };
}
#[cfg(feature = "gui-e2e")]
pub fn queue_smart_generation(task: SmartGenerationTask) {
    SMART_GENERATION.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn take_smart_generation() -> Option<SmartGenerationTask> {
    SMART_GENERATION.with(|slot| slot.borrow_mut().take())
}
/// Whether a queued generation is still waiting: a scenario asserts its
/// generation consumed it (left queued, the real provider CLI ran instead).
#[cfg(feature = "gui-e2e")]
pub fn smart_generation_queued() -> bool {
    SMART_GENERATION.with(|slot| slot.borrow().is_some())
}

#[cfg(feature = "gui-e2e")]
pub fn ensure_smart_commit_detection(app: &mut KagiApp, cx: &mut gpui::Context<KagiApp>) {
    app.ensure_smart_commit_detection(cx);
}

#[cfg(feature = "gui-e2e")]
pub fn smart_model_row(
    app: &KagiApp,
    window: &gpui::Window,
) -> Option<(
    usize,
    gpui::Bounds<gpui::Pixels>,
    gpui::Bounds<gpui::Pixels>,
)> {
    let state = &app.smart_model_focus;
    let focus = state.focus.borrow();
    let key = focus.focused(window)?;
    let (_, ix) = state.keys.iter().find(|(k, _)| k == key)?;
    Some((
        *ix,
        state.scroll.bounds_for_item(*ix)?,
        state.scroll.viewport_bounds(),
    ))
}

/// Fetch the avatars of Home's review-request authors now, as a landed read
/// does (#968).
#[cfg(feature = "gui-e2e")]
pub fn ensure_home_avatars(app: &mut KagiApp, cx: &mut gpui::Context<KagiApp>) {
    app.ensure_home_avatars(cx);
}

/// Whether the review-request row for `item` finds its author's avatar
/// image (else it draws the initials circle).
#[cfg(feature = "gui-e2e")]
pub fn home_review_avatar_shown(
    app: &KagiApp,
    item: &kagi_git::github_repos_cache::WorkItem,
) -> bool {
    app.avatars
        .images
        .contains_key(super::home_work_list::review_avatar_key(item).as_ref())
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static CLONE_TICKERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The clone card's redraw tickers running now (#968).
#[cfg(feature = "gui-e2e")]
pub fn clone_tickers() -> usize {
    CLONE_TICKERS.with(std::cell::Cell::get)
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn clone_ticker_started() {
    CLONE_TICKERS.with(|n| n.set(n.get() + 1));
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn clone_ticker_ended() {
    CLONE_TICKERS.with(|n| n.set(n.get() - 1));
}

#[cfg(feature = "gui-e2e")]
pub fn seed_modal_list_scroll(app: &KagiApp, item: usize) {
    app.modal_list_scroll
        .scroll_to_item(item, gpui::ScrollStrategy::Center);
}

/// #1069: the actual palette list handle, for native geometry/scroll assertions.
#[cfg(feature = "gui-e2e")]
pub fn command_palette_scroll(app: &KagiApp) -> Option<super::palette_scroll::PaletteScroll> {
    app.command_palette_scroll.clone()
}

#[cfg(feature = "gui-e2e")]
pub fn modal_list_scroll_top(app: &KagiApp) -> usize {
    app.modal_list_scroll.logical_scroll_top_index()
}

#[cfg(feature = "gui-e2e")]
pub type RemoteOpenResult = Result<(String, kagi_git::RepoSnapshot), String>;
#[cfg(feature = "gui-e2e")]
type RemoteOpenTask = gpui::Task<RemoteOpenResult>;
#[cfg(feature = "gui-e2e")]
thread_local! {
    static REMOTE_OPEN: RefCell<Option<RemoteOpenTask>> = const { RefCell::new(None) };
}
#[cfg(feature = "gui-e2e")]
pub fn queue_remote_open(task: RemoteOpenTask) {
    REMOTE_OPEN.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn take_remote_open() -> Option<RemoteOpenTask> {
    REMOTE_OPEN.with(|slot| slot.borrow_mut().take())
}
#[cfg(feature = "gui-e2e")]
pub fn prepare_remote_browse_open(
    app: &mut KagiApp,
    host: kagi_domain::remote::RemoteHost,
    path: &str,
) {
    let modal = app.remote_browse_mut().expect("Remote Browse must be open");
    modal.stage = super::remote_browse::RemoteBrowseStage::Browse;
    modal.host = Some(host);
    modal.cwd = path.to_string();
    modal.current_is_repo = true;
    modal.busy = false;
}

#[cfg(feature = "gui-e2e")]
pub fn defer_file_menu(
    app: &KagiApp,
    fi: usize,
    pos: gpui::Point<gpui::Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let panel = app
        .ui()
        .commit_panel
        .as_ref()
        .expect("mounted commit panel");
    panel.update(cx, |panel, cx| {
        panel.defer_open_file_menu(fi, pos, window, cx)
    });
}

#[cfg(feature = "gui-e2e")]
pub fn dispatch_file_menu_discard(
    app: &mut KagiApp,
    menu: &super::file_menu::FileMenu,
    cx: &mut gpui::Context<KagiApp>,
) {
    app.dispatch_file_menu_action(menu, super::file_menu::FileMenuAction::Discard, cx);
}

#[cfg(feature = "gui-e2e")]
type PrFetchResult = Result<kagi_domain::github::PrListSnapshot, kagi_git::github::PrFetchError>;
#[cfg(feature = "gui-e2e")]
type CleanupScanResult = Result<
    (
        Vec<kagi_domain::branch_cleanup::BranchCleanupRow>,
        Result<Vec<kagi_domain::github::PullRequest>, kagi_git::github::PrFetchError>,
    ),
    String,
>;
#[cfg(feature = "gui-e2e")]
type EcosystemMineResult = Result<kagi_domain::hotspot::RawEcosystem, String>;
#[cfg(feature = "gui-e2e")]
type SquashScanResult = Result<Vec<kagi_git::ops::SquashLink>, kagi_git::GitError>;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static GITHUB_PR_FETCH: RefCell<Option<gpui::Task<PrFetchResult>>> = const { RefCell::new(None) };
    static CLEANUP_SCAN: RefCell<Option<gpui::Task<CleanupScanResult>>> = const { RefCell::new(None) };
    static ECOSYSTEM_MINE: RefCell<Option<gpui::Task<EcosystemMineResult>>> = const { RefCell::new(None) };
    static SQUASH_SCAN: RefCell<Option<gpui::Task<SquashScanResult>>> = const { RefCell::new(None) };
}

/// What one conversation read returns: the verdicts + issue comments, and the
/// review threads (#351), each with its own failure. Unconditional so the
/// production load site can name it in its `None` arm.
pub type PrConversationResult = (
    Result<
        (
            Vec<kagi_domain::github::Review>,
            Vec<kagi_domain::github::Comment>,
        ),
        kagi_git::GitError,
    >,
    Result<Vec<kagi_domain::review_thread::ReviewThread>, kagi_git::GitError>,
);

#[cfg(feature = "gui-e2e")]
thread_local! {
    static GITHUB_PR_CONVERSATION: RefCell<Option<gpui::Task<PrConversationResult>>> =
        const { RefCell::new(None) };
}

/// Stand in for the next PR conversation read (`gh pr view` + `gh api`), so a
/// scenario can land reviews and comments on a tab without GitHub (ADR-0200).
#[cfg(feature = "gui-e2e")]
pub fn queue_github_pr_conversation(task: gpui::Task<PrConversationResult>) {
    GITHUB_PR_CONVERSATION.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn take_github_pr_conversation() -> Option<gpui::Task<PrConversationResult>> {
    GITHUB_PR_CONVERSATION.with(|slot| slot.borrow_mut().take())
}

#[cfg(feature = "gui-e2e")]
pub fn queue_github_pr_fetch(task: gpui::Task<PrFetchResult>) {
    GITHUB_PR_FETCH.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn take_github_pr_fetch() -> Option<gpui::Task<PrFetchResult>> {
    GITHUB_PR_FETCH.with(|slot| slot.borrow_mut().take())
}

/// Inspect the collection the production PR consumers currently read.
#[cfg(feature = "gui-e2e")]
pub fn pr_page_info(
    app: &KagiApp,
) -> (
    Vec<kagi_domain::github::PrKey>,
    Option<String>,
    Option<String>,
    bool,
) {
    let ui = app.ui();
    let paging = if ui.github_prs_strip.rows.is_some() {
        &ui.github_prs_strip.paging
    } else {
        &ui.github_prs_paging
    };
    (
        ui.pr_list_rows().iter().map(|pr| pr.key()).collect(),
        paging.base_repo.clone(),
        paging.cursor.clone(),
        ui.pr_list_loading_more(),
    )
}

/// First-page authority/error shown by the actual selected-list consumer.
/// This is inspection only; it neither delivers nor schedules a transport.
#[cfg(feature = "gui-e2e")]
pub fn pr_list_read_status(app: &KagiApp) -> (bool, Option<String>) {
    let ui = app.ui();
    (ui.pr_list_loading(), ui.pr_list_error().map(str::to_owned))
}

/// Use the same owner-targeted continuation boundary as the tail and buttons.
#[cfg(feature = "gui-e2e")]
pub fn load_more_prs(app: &mut KagiApp, cx: &mut gpui::Context<KagiApp>) {
    if let (Some(owner), Some(repo)) = (app.active_session(), app.repo_path.clone()) {
        app.load_more_github_prs_for(owner, repo, cx);
    }
}

/// Actual lazy-controller targets, not an echo of a fixture or rendered rows.
/// The pending tuple's boolean distinguishes opened-body work from status work.
#[cfg(feature = "gui-e2e")]
pub fn pr_detail_demand(
    app: &KagiApp,
) -> (
    std::collections::BTreeSet<kagi_domain::github::PrKey>,
    std::collections::BTreeSet<kagi_domain::github::PrKey>,
    Vec<(kagi_domain::github::PrKey, bool)>,
    usize,
) {
    let (visible, opened, pending, active) = app.ui().pr_details.paging_demand_for_e2e();
    (
        visible,
        opened,
        pending
            .into_iter()
            .map(|(key, stage)| (key, stage == super::github_pr_detail::PrDetailStage::Body))
            .collect(),
        active,
    )
}

#[cfg(feature = "gui-e2e")]
pub fn queue_cleanup_scan(task: gpui::Task<CleanupScanResult>) {
    CLEANUP_SCAN.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn take_cleanup_scan() -> Option<gpui::Task<CleanupScanResult>> {
    CLEANUP_SCAN.with(|slot| slot.borrow_mut().take())
}

#[cfg(feature = "gui-e2e")]
pub fn queue_ecosystem_mine(task: gpui::Task<EcosystemMineResult>) {
    ECOSYSTEM_MINE.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn take_ecosystem_mine() -> Option<gpui::Task<EcosystemMineResult>> {
    ECOSYSTEM_MINE.with(|slot| slot.borrow_mut().take())
}

#[cfg(feature = "gui-e2e")]
pub fn queue_squash_scan(task: gpui::Task<SquashScanResult>) {
    SQUASH_SCAN.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn take_squash_scan() -> Option<gpui::Task<SquashScanResult>> {
    SQUASH_SCAN.with(|slot| slot.borrow_mut().take())
}

/// ADR-0197 S5 prerequisite: ownerless state has no writable fallback cell.
#[cfg(feature = "gui-e2e")]
pub fn active_ui_writer_available(app: &mut KagiApp) -> bool {
    app.ui_mut().is_some()
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static OPENED_URLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static GITHUB_NAV: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FOLDER_PROMPTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static FOLDER_ANSWER: RefCell<Option<gpui::Task<Result<Option<std::path::PathBuf>, String>>>> = const { RefCell::new(None) };
    static CLONE_RUN: RefCell<Option<gpui::Task<()>>> = const { RefCell::new(None) };
    static CLONE_DISPATCHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static CLONE_BUSY_STATUS: RefCell<Option<(gpui::Role, String)>> = const { RefCell::new(None) };
    static CLONE_BUSY_FOCUS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// #1043 / #1383: record the native folder request and await its answer through
/// the same completion task as production. An unqueued request is cancelled.
#[cfg(feature = "gui-e2e")]
pub(crate) fn take_folder_prompt(
    prompt: &str,
) -> Option<gpui::Task<Result<Option<std::path::PathBuf>, String>>> {
    FOLDER_PROMPTS.with(|prompts| prompts.borrow_mut().push(prompt.to_string()));
    FOLDER_ANSWER.with(|answer| answer.borrow_mut().take())
}

/// Queue the next dialog answer using the shared deferred-task transport.
#[cfg(feature = "gui-e2e")]
pub fn queue_folder_answer(task: gpui::Task<Result<Option<std::path::PathBuf>, String>>) {
    FOLDER_ANSWER.with(|answer| assert!(answer.borrow_mut().replace(task).is_none()));
}
#[cfg(feature = "gui-e2e")]
pub fn answer_folder_prompt(folder: Option<std::path::PathBuf>) {
    queue_folder_answer(gpui::Task::ready(Ok(folder)));
}

/// Hold dispatch only: the real clone still executes, verifies and records.
#[cfg(feature = "gui-e2e")]
pub fn queue_clone_run(task: gpui::Task<()>) {
    CLONE_RUN.with(|slot| assert!(slot.borrow_mut().replace(task).is_none()));
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn take_clone_run() -> Option<gpui::Task<()>> {
    CLONE_DISPATCHES.with(|count| count.set(count.get() + 1));
    CLONE_RUN.with(|slot| slot.borrow_mut().take())
}

#[cfg(feature = "gui-e2e")]
pub fn take_clone_dispatches() -> usize {
    CLONE_DISPATCHES.with(|count| count.replace(0))
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn record_clone_busy_status(role: gpui::Role, name: &str) {
    CLONE_BUSY_STATUS.with(|status| *status.borrow_mut() = Some((role, name.to_owned())));
}

#[cfg(feature = "gui-e2e")]
pub fn take_clone_busy_status() -> Option<(gpui::Role, String)> {
    CLONE_BUSY_STATUS.with(|status| status.borrow_mut().take())
}

/// Observe the pinned Button's existing keyed focus owner, not a second handle.
#[cfg(feature = "gui-e2e")]
pub(crate) fn probe_clone_busy_button(button: gpui_component::button::Button) -> gpui::AnyElement {
    use gpui::IntoElement as _;
    #[derive(gpui::IntoElement)]
    struct Probe(gpui_component::button::Button);
    impl gpui::RenderOnce for Probe {
        fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl gpui::IntoElement {
            let focus = window
                .use_keyed_state("clone-busy-show", cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone();
            if focus.is_focused(window) {
                CLONE_BUSY_FOCUS.with(|slot| slot.set(true));
            }
            self.0.render(window, cx)
        }
    }
    Probe(button).into_any_element()
}

#[cfg(feature = "gui-e2e")]
pub fn take_clone_busy_focus() -> bool {
    CLONE_BUSY_FOCUS.with(|focus| focus.replace(false))
}

/// The prompts of the folder dialogs requested since the last call.
#[cfg(feature = "gui-e2e")]
pub fn take_folder_prompts() -> Vec<String> {
    FOLDER_PROMPTS.with(|prompts| std::mem::take(&mut *prompts.borrow_mut()))
}

/// A URL Kagi asked the OS to open, recorded instead of launching the
/// user's browser from the GUI runner (#940 review).
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_opened_url(url: &str) {
    OPENED_URLS.with(|urls| urls.borrow_mut().push(url.to_string()));
}

/// The URLs opened since the last call, oldest first.
#[cfg(feature = "gui-e2e")]
pub fn take_opened_urls() -> Vec<String> {
    OPENED_URLS.with(|urls| std::mem::take(&mut *urls.borrow_mut()))
}

/// Draw the PRs / Issues cells of the workspace-mode nav whether or not this
/// machine has a `gh` (#960 review): a scenario on them must not pass by
/// skipping them.
#[cfg(feature = "gui-e2e")]
pub fn set_github_nav(shown: bool) {
    GITHUB_NAV.with(|nav| nav.set(shown));
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn github_nav_forced() -> bool {
    GITHUB_NAV.with(|nav| nav.get())
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static FETCH_COMPLETIONS_RETURNED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// #992 review: counts fetch completions whose UI update has fully returned,
/// so a scenario can tell a notification flushed by the completion itself
/// from one a later, unrelated task happened to send.
#[cfg(feature = "gui-e2e")]
pub(crate) fn mark_fetch_completion_returned() {
    FETCH_COMPLETIONS_RETURNED.with(|count| count.set(count.get() + 1));
}

#[cfg(feature = "gui-e2e")]
pub fn fetch_completions_returned() -> u64 {
    FETCH_COMPLETIONS_RETURNED.with(std::cell::Cell::get)
}

/// Transport-only hold for the real Issue detail loader. The loader still
/// freezes session/base repository/generation and owns acceptance/rejection.
#[cfg(feature = "gui-e2e")]
pub type IssueDetailResult = Result<kagi_domain::github::Issue, kagi_git::github::PrFetchError>;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static ISSUE_DETAIL_READ: RefCell<Option<(u64, gpui::Task<IssueDetailResult>)>> =
        const { RefCell::new(None) };
    static ISSUE_DETAIL_READ_SERIAL: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

/// Clears an unconsumed task on unwind without disturbing a later request.
#[cfg(feature = "gui-e2e")]
pub struct IssueDetailReadGuard(u64);

#[cfg(feature = "gui-e2e")]
impl Drop for IssueDetailReadGuard {
    fn drop(&mut self) {
        ISSUE_DETAIL_READ.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.as_ref().is_some_and(|(serial, _)| *serial == self.0) {
                slot.take();
            }
        });
    }
}

#[cfg(feature = "gui-e2e")]
pub fn queue_github_issue_detail(task: gpui::Task<IssueDetailResult>) -> IssueDetailReadGuard {
    let serial = ISSUE_DETAIL_READ_SERIAL.with(|serial| {
        let next = serial.get().wrapping_add(1);
        serial.set(next);
        next
    });
    ISSUE_DETAIL_READ.with(|slot| {
        let mut slot = slot.borrow_mut();
        assert!(slot.is_none(), "unconsumed Issue detail task");
        *slot = Some((serial, task));
    });
    IssueDetailReadGuard(serial)
}

#[cfg(feature = "gui-e2e")]
pub(super) fn take_github_issue_detail() -> Option<gpui::Task<IssueDetailResult>> {
    ISSUE_DETAIL_READ.with(|slot| slot.borrow_mut().take().map(|(_, task)| task))
}

/// Enter the existing accepted repository-address transition, not a test owner.
#[cfg(feature = "gui-e2e")]
pub fn retarget_github_issues(app: &mut KagiApp, identity: &str, cx: &mut gpui::Context<KagiApp>) {
    app.address_issues_to(identity, cx);
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static HOME_REPOSITORY_DESCRIPTIONS: std::cell::RefCell<std::collections::HashMap<String, String>> =
        std::cell::RefCell::new(Default::default());
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn record_home_repository_description(name: &str, description: &str) {
    HOME_REPOSITORY_DESCRIPTIONS.with(|values| {
        values
            .borrow_mut()
            .insert(name.to_owned(), description.to_owned());
    });
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn take_home_repository_description(name: &str) -> Option<String> {
    HOME_REPOSITORY_DESCRIPTIONS.with(|values| values.borrow_mut().remove(name))
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static HOME_REPOSITORY_ACTIONS: std::cell::RefCell<std::collections::HashMap<String, (gpui::Role, String)>> =
        std::cell::RefCell::new(Default::default());
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn record_home_repository_action(name: &str, label: &str) {
    HOME_REPOSITORY_ACTIONS.with(|values| {
        values
            .borrow_mut()
            .insert(name.to_owned(), (gpui::Role::Button, label.to_owned()));
    });
}

/// Attributes supplied by the named gpui-kit Button constructor, not live AX delivery.
#[cfg(feature = "gui-e2e")]
pub fn home_repository_action(name: &str) -> Option<(gpui::Role, String)> {
    HOME_REPOSITORY_ACTIONS.with(|values| values.borrow().get(name).cloned())
}

#[cfg(feature = "gui-e2e")]
pub fn clear_home_repository_probes() {
    HOME_REPOSITORY_DESCRIPTIONS.with(|values| values.borrow_mut().clear());
    HOME_REPOSITORY_ACTIONS.with(|values| values.borrow_mut().clear());
}
