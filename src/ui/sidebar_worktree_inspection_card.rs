//! Shared pointer/keyboard worktree inspection card and named native Button actions.
use super::super::{
    e2e, keyboard_nav,
    theme::{self, theme},
};
use super::{size_text, InspectionEntry, KagiApp, Msg};
use gpui::{div, prelude::*, px, rgb, svg, Context, SharedString};
use gpui_component::{
    button::{Button, ButtonCustomVariant, ButtonVariants},
    Sizable,
};
use std::path::PathBuf;
type ActionFocus = std::rc::Rc<std::cell::RefCell<Option<gpui::FocusHandle>>>;

const CARD_W: f32 = 288.;
const CARD_H: f32 = 132.;
const REFRESH: &str = "worktree-inspection-refresh";
const URL: &str = "worktree-inspection-port";

/// Pointer cards belong to the tab that built them, even for shared registries.
pub(super) struct WorktreeHoverCard {
    app: gpui::WeakEntity<KagiApp>,
    owner: Option<crate::app::SessionId>,
    path: PathBuf,
    name: SharedString,
    branch: Option<SharedString>,
    port: Option<u16>,
    action_focus: [Option<ActionFocus>; 2],
    _app_changed: Option<gpui::Subscription>,
}

impl WorktreeHoverCard {
    pub(super) fn build(
        app: gpui::WeakEntity<KagiApp>,
        path: PathBuf,
        name: SharedString,
        branch: Option<SharedString>,
        port: Option<u16>,
        cx: &mut gpui::App,
    ) -> gpui::AnyView {
        cx.new(|cx: &mut Context<Self>| {
            let owner = app
                .upgrade()
                .and_then(|entity| entity.read(cx).active_session());
            let _app_changed = app
                .upgrade()
                .map(|entity| cx.observe(&entity, |_, _, cx| cx.notify()));
            Self {
                app,
                owner,
                path,
                name,
                branch,
                port,
                action_focus: [None, None],
                _app_changed,
            }
        })
        .into()
    }
}

impl gpui::Render for WorktreeHoverCard {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(entity) = self.app.upgrade() else {
            return div().into_any_element();
        };
        // Tooltip layout flips at the edge without available space.
        let viewport = f32::from(window.viewport_size().height);
        let height = px((viewport - 16.)
            .min(f32::from(theme::scaled_px(CARD_H)))
            .max(0.));
        inspection_card(entity.read(cx), self, height, cx)
    }
}

/// The row and its card form one focus scope; the actions remain distinct Tab stops.
pub(in crate::ui) fn keyboard_card(
    app: &KagiApp,
    path: PathBuf,
    name: &str,
    branch: Option<&str>,
    port: Option<u16>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let card = WorktreeHoverCard {
        app: cx.weak_entity(),
        owner: app.active_session(),
        path,
        name: kagi_domain::text_safety::sanitize_control_bytes(name).into(),
        branch: branch
            .map(kagi_domain::text_safety::sanitize_control_bytes)
            .map(SharedString::from),
        port,
        action_focus: app
            .sidebar
            .focus
            .card_actions
            .each_ref()
            .map(|slot| Some(slot.clone())),
        _app_changed: None,
    };
    div()
        .id("sidebar-worktree-keyboard-card")
        .track_focus(
            app.sidebar
                .focus
                .card_focus
                .as_ref()
                .expect("opened card focus scope"),
        )
        .relative()
        .occlude()
        .child(e2e::measure_inside("sidebar-worktree-keyboard-card"))
        .child(inspection_card(app, &card, theme::scaled_px(CARD_H), cx))
        .into_any_element()
}

/// Forward names onto the pinned Button itself, not a second focus target.
#[derive(gpui::IntoElement)]
struct InspectionButton {
    button: Button,
    id: &'static str,
    focus_slot: Option<ActionFocus>,
    #[cfg(feature = "gui-e2e")]
    name: SharedString,
}
impl gpui::InteractiveElement for InspectionButton {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.button.interactivity()
    }
}
impl gpui::StatefulInteractiveElement for InspectionButton {}
impl gpui::RenderOnce for InspectionButton {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
        if self.focus_slot.is_some() || cfg!(feature = "gui-e2e") {
            let focus = window
                .use_keyed_state(self.id, cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone();
            if let Some(slot) = self.focus_slot {
                *slot.borrow_mut() = Some(focus.clone());
            }
            #[cfg(feature = "gui-e2e")]
            e2e::worktree_inspection::record_action(
                self.id,
                gpui::Role::Button,
                self.name.clone(),
                Some(focus),
            );
        }
        self.button.render(window, cx)
    }
}

fn named_button(
    button: Button,
    id: &'static str,
    name: SharedString,
    focus_slot: Option<ActionFocus>,
) -> InspectionButton {
    InspectionButton {
        button: button.tab_stop(focus_slot.is_some()),
        id,
        focus_slot,
        #[cfg(feature = "gui-e2e")]
        name: name.clone(),
    }
    .role(gpui::Role::Button)
    .aria_label(name)
}

fn action_button(id: &'static str, color: u32, cx: &gpui::App) -> Button {
    Button::new(id)
        .small()
        .compact()
        .custom(
            ButtonCustomVariant::new(cx)
                .color(rgb(theme().surface).into())
                .foreground(rgb(color).into())
                .hover(rgb(theme().selected).into())
                .active(rgb(theme().selected).into()),
        )
        .on_key_down(keyboard_nav::stop_activation_keys)
        .relative()
        .cursor_pointer()
        .child(e2e::measure_inside(id))
}

fn refresh_button(card: &WorktreeHoverCard, cx: &gpui::App) -> InspectionButton {
    let app = card.app.clone();
    let path = card.path.clone();
    named_button(
        action_button(REFRESH, theme().text_sub, cx)
            .w(theme::scaled_px(24.))
            .h(theme::scaled_px(24.))
            .px_0()
            .border_1()
            .border_color(rgb(theme().selected))
            .rounded_sm()
            .child(
                svg()
                    .path("icons/refresh-cw.svg")
                    .w(theme::scaled_px(14.))
                    .h(theme::scaled_px(14.)),
            )
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                #[cfg(feature = "gui-e2e")]
                e2e::worktree_inspection::record_refresh_activation();
                let _ = app.update(cx, |app, cx| {
                    app.refresh_worktree_inspections(Some(std::slice::from_ref(&path)), cx);
                });
            }),
        REFRESH,
        Msg::WorktreeInspectionRefresh.t().into(),
        card.action_focus[0].clone(),
    )
}

fn url_button(card: &WorktreeHoverCard, port: u16, cx: &gpui::App) -> InspectionButton {
    let url = format!("http://localhost:{port}");
    named_button(
        action_button(URL, theme().color_branch, cx)
            .xsmall()
            .h(theme::scaled_px(20.))
            .px_1()
            .gap_0p5()
            .rounded_sm()
            .when(theme().slug == "apple-dark", |button| {
                button
                    .border_1()
                    .border_color(rgb(theme().elevated_border(theme().surface)))
            })
            .child(
                svg()
                    .path("icons/square-terminal.svg")
                    .w(theme::scaled_px(12.))
                    .h(theme::scaled_px(12.)),
            )
            .child(format!(":{port}"))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                #[cfg(feature = "gui-e2e")]
                e2e::record_opened_url(&url);
                cx.open_url(&url);
            }),
        URL,
        Msg::worktree_inspection_open_url(port).into(),
        card.action_focus[1].clone(),
    )
}

fn heading(card: &WorktreeHoverCard, cx: &gpui::App) -> gpui::Stateful<gpui::Div> {
    let branch = card
        .branch
        .clone()
        .unwrap_or_else(|| Msg::WorktreeDetachedShort.t().into());
    div()
        .id("worktree-inspection-heading")
        .relative()
        .child(e2e::measure_inside("worktree-inspection-heading"))
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    svg()
                        .path("icons/tree-pine.svg")
                        .w(theme::scaled_px(14.))
                        .h(theme::scaled_px(14.))
                        .text_color(rgb(theme().color_branch)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(theme().text_main))
                        .child(card.name.clone()),
                )
                .child(refresh_button(card, cx)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    svg()
                        .path("icons/git-branch.svg")
                        .w(theme::scaled_px(14.))
                        .h(theme::scaled_px(14.))
                        .text_color(rgb(theme().text_muted)),
                )
                .child(
                    div()
                        .id("worktree-inspection-branch")
                        .relative()
                        .child(e2e::measure_inside("worktree-inspection-branch"))
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .child(branch),
                ),
        )
}

fn state_chip(label: &'static str, color: u32) -> gpui::Div {
    div()
        .h(theme::scaled_px(20.))
        .flex()
        .items_center()
        .gap_0p5()
        .px_1()
        .rounded_sm()
        .bg(rgb(theme().surface))
        .when(theme().slug == "apple-dark", |chip| {
            chip.border_1()
                .border_color(rgb(theme().elevated_border(theme().surface)))
        })
        .text_color(rgb(color))
        .child(label)
}

fn chips(
    app: &KagiApp,
    card: &WorktreeHoverCard,
    entry: Option<&InspectionEntry>,
    cx: &gpui::App,
) -> gpui::Stateful<gpui::Div> {
    let state = &app.ui().worktree_inspections;
    let pending = state.pending.contains(&card.path);
    let stale = entry.is_some_and(|e| !app.reads.is_fresh(e.read) || app.op_latched());
    let error = entry.is_some_and(|e| e.report.disk_usage.is_err());
    let dirty = app
        .view()
        .worktrees
        .iter()
        .any(|w| w.path == card.path && w.wip.is_some_and(|wip| wip.is_dirty()))
        || entry
            .is_some_and(|e| e.report.removal.clean == kagi_domain::remove::WorktreeEvidence::No);
    let locked = app
        .view()
        .worktrees
        .iter()
        .any(|w| w.path == card.path && w.locked);
    let mut chips = div()
        .id("worktree-inspection-body")
        .relative()
        .child(e2e::measure_inside("worktree-inspection-body"))
        .flex()
        .items_center()
        .gap_1()
        .min_w(px(0.));
    if pending {
        chips = chips.child(e2e::measure_control(
            "worktree-inspection-measuring",
            state_chip(Msg::WorktreeMeasuring.t(), theme().text_sub),
        ));
    } else if error {
        chips = chips.child(e2e::measure_control(
            "worktree-inspection-error",
            state_chip(Msg::WorktreeErrorShort.t(), theme().color_blocker),
        ));
    } else if stale {
        chips = chips.child(e2e::measure_control(
            "worktree-inspection-stale",
            state_chip(Msg::WorktreeStaleShort.t(), theme().color_warning),
        ));
    } else if entry.is_none() {
        chips = chips.child(state_chip(Msg::WorktreeNotMeasured.t(), theme().text_muted));
    }
    if dirty {
        chips = chips.child(e2e::measure_control(
            "worktree-inspection-dirty",
            state_chip(Msg::WorktreeDirtyShort.t(), theme().color_blocker),
        ));
    }
    if locked {
        chips = chips.child(e2e::measure_control(
            "worktree-inspection-locked",
            state_chip(Msg::WorktreeLockedShort.t(), theme().color_warning).child(
                svg()
                    .path("icons/lock-keyhole.svg")
                    .w(theme::scaled_px(12.))
                    .h(theme::scaled_px(12.)),
            ),
        ));
    }
    if let Some(port) = card.port {
        chips = chips.child(url_button(card, port, cx));
    }
    chips
}

fn path_and_size(card: &WorktreeHoverCard, entry: Option<&InspectionEntry>) -> gpui::Div {
    let path_label: SharedString =
        kagi_domain::text_safety::sanitize_control_bytes(&card.path.to_string_lossy()).into();
    // Pending/stale observations retain capacity; only their verdict is suppressed.
    let size = entry
        .and_then(|e| e.report.disk_usage.as_ref().ok())
        .map(|usage| size_text(usage.allocated_bytes));
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .id("worktree-inspection-path")
                .relative()
                .child(e2e::measure_inside("worktree-inspection-path"))
                .role(gpui::Role::Label)
                .aria_label(path_label.clone())
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_color(rgb(theme().text_muted))
                .child(path_label),
        )
        .children(size.map(|size| {
            div()
                .flex_shrink_0()
                .text_color(rgb(theme().text_sub))
                .child(size)
        }))
}

fn inspection_card(
    app: &KagiApp,
    card: &WorktreeHoverCard,
    height: gpui::Pixels,
    cx: &gpui::App,
) -> gpui::AnyElement {
    if app.active_session() != card.owner
        || app.remote_view.is_some()
        || app.worktree_menu.is_some()
        || !app.view().worktrees.iter().any(|w| w.path == card.path)
    {
        return div().into_any_element();
    }
    let entry = app.ui().worktree_inspections.entries.get(&card.path);
    div()
        .id("worktree-inspection")
        // GPUI's interactive hoverable_tooltip is also a separate root, outside Kit Root.
        .font(theme::ui_font())
        .relative()
        .child(e2e::measure_inside("worktree-inspection"))
        .map(|card| {
            #[cfg(feature = "gui-e2e")]
            let card = card.child(kagi_ui_core::fonts::e2e::probe("worktree-hover-card"));
            card
        })
        .occlude()
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .w(theme::scaled_px(CARD_W))
        .h(height)
        .flex()
        .flex_col()
        .p_2()
        .gap_1()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(rgb(theme().elevated_border(theme().surface)))
        .bg(rgb(theme().elevated_surface(theme().panel)))
        .shadow_md()
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .child(heading(card, cx))
        .child(chips(app, card, entry, cx))
        .child(path_and_size(card, entry))
        .into_any_element()
}
