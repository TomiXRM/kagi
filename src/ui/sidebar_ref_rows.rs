//! Dense outline group and ref identities, with shared pointer/keyboard navigation.
use gpui::{div, prelude::*, rgb, Context, SharedString};
use gpui_component::tooltip::Tooltip;
use kagi_git::CommitId;

use super::sidebar::{
    name_tooltip, SECTION_LOCAL, SECTION_REMOTE, SECTION_STASHES, SECTION_TAGS, SECTION_WORKTREES,
    SIDEBAR_AUX_TEXT, SIDEBAR_ROW_H, SIDEBAR_SECTION_HEADER_H,
};
use super::theme::{self, theme};
use super::{KagiApp, Msg, ToastKind};

/// Section header row (LOCAL BRANCHES / …). Click toggles `sidebar_collapsed`.
pub(super) fn build_section_header(
    section: &'static str,
    title: Msg,
    count: usize,
    collapsed: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let label = SharedString::from(format!("({count})"));
    let toggle = cx.listener(
        move |this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
            this.sidebar.toggle_section(section);
            cx.notify();
        },
    );
    div()
        .id(SharedString::from(format!("sidebar-section-{}", section)))
        .relative()
        .child(super::e2e::measure_inside(section))
        .h(theme::scaled_px(SIDEBAR_SECTION_HEADER_H))
        .flex_shrink_0()
        .px_3()
        .py(theme::scaled_px(8.))
        .flex()
        .flex_row()
        .items_center()
        .text_size(theme::scaled_px(12.))
        .line_height(theme::scaled_px(12.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(theme().text_sub))
        .on_click(toggle)
        .hover(|s| s.bg(rgb(theme().surface)))
        .child(
            div()
                .relative()
                .size(theme::scaled_px(10.))
                .flex_shrink_0()
                .child(super::e2e::measure_inside(section_header_chevron_id(
                    section,
                )))
                .child(
                    gpui::svg()
                        .path("icons/chevron-right.svg")
                        .size(theme::scaled_px(10.))
                        .text_color(rgb(theme().text_sub))
                        .with_transformation(gpui::Transformation::rotate(gpui::radians(
                            if collapsed {
                                0.
                            } else {
                                std::f32::consts::FRAC_PI_2
                            },
                        ))),
                ),
        )
        .child(
            div()
                .relative()
                .ml(theme::scaled_px(6.))
                .h(theme::scaled_px(12.))
                .child(super::e2e::measure_inside(section_header_label_id(section)))
                .child(title.t()),
        )
        .child(
            div()
                .ml(theme::scaled_px(6.))
                .rounded_full()
                .px(theme::scaled_px(4.))
                .bg(rgb(theme().bg_base))
                .font_weight(gpui::FontWeight::NORMAL)
                .text_size(theme::scaled_px(11.))
                .child(label),
        )
        .into_any()
}

fn section_header_label_id(section: &str) -> &'static str {
    match section {
        SECTION_LOCAL => "sidebar-local-heading-label",
        SECTION_REMOTE => "sidebar-remote-heading-label",
        SECTION_WORKTREES => "sidebar-worktrees-heading-label",
        SECTION_TAGS => "sidebar-tags-heading-label",
        SECTION_STASHES => "sidebar-stashes-heading-label",
        _ => unreachable!("known sidebar section"),
    }
}

fn section_header_chevron_id(section: &str) -> &'static str {
    match section {
        SECTION_LOCAL => "sidebar-local-heading-chevron",
        SECTION_REMOTE => "sidebar-remote-heading-chevron",
        SECTION_WORKTREES => "sidebar-worktrees-heading-chevron",
        SECTION_TAGS => "sidebar-tags-heading-chevron",
        SECTION_STASHES => "sidebar-stashes-heading-chevron",
        _ => unreachable!("known sidebar section"),
    }
}

/// The loaded read model is the sole availability source for tag navigation.
pub(super) fn tag_navigation_reason(app: &KagiApp, target: &CommitId) -> Option<Msg> {
    let view = app.view();
    let loaded = view.commit_row_index.contains_key(target)
        || view
            .branch_solo
            .as_ref()
            .is_some_and(|solo| solo.saved_row_index.contains_key(target));
    (!loaded).then_some(Msg::SidebarTagCommitNotLoaded)
}

impl KagiApp {
    pub(super) fn activate_sidebar_tag(&mut self, target: &CommitId, cx: &mut Context<Self>) {
        if let Some(reason) = tag_navigation_reason(self, target) {
            self.push_toast(ToastKind::Info, reason.t(), cx);
            let view = self.view();
            let row_count = view
                .branch_solo
                .as_ref()
                .map_or(view.rows.len(), |solo| solo.saved_rows.len());
            let limit = self.ui().commit_limit;
            if limit > 0 && row_count >= limit {
                self.load_more_commits(cx);
            }
        } else if !self.view().commit_row_index.contains_key(target) {
            self.push_toast(ToastKind::Info, Msg::SidebarTagHiddenBySolo.t(), cx);
        } else {
            self.jump_to_commit(target);
        }
        cx.notify();
    }

    /// Filter results auto-expand groups; never rewrite the hidden normal choice.
    pub(super) fn toggle_sidebar_group(&mut self, key: &str, cx: &mut Context<Self>) {
        let filtering = self
            .sidebar
            .filter
            .as_ref()
            .is_some_and(|input| !input.read(cx).value().is_empty());
        if filtering {
            self.push_toast(ToastKind::Info, Msg::SidebarGroupFilterExpanded.t(), cx);
        } else {
            self.with_ui(|ui| ui.toggle_branch_group(key));
        }
        cx.notify();
    }
}

pub(super) fn build_group_row(
    row: &super::sidebar::SidebarRow,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    use super::sidebar::SidebarRow;
    match row {
        SidebarRow::LocalGroupHeader {
            key,
            prefix,
            count,
            collapsed,
        }
        | SidebarRow::RemoteHeader {
            key,
            remote: prefix,
            count,
            collapsed,
        } => build_group_header(
            key,
            prefix,
            prefix,
            *count,
            *collapsed,
            theme::scaled_px(20.),
            cx,
        ),
        SidebarRow::RemoteSubGroup {
            key,
            remote,
            prefix,
            count,
            collapsed,
        } => build_group_header(
            key,
            prefix,
            &format!("{remote}/{prefix}"),
            *count,
            *collapsed,
            theme::scaled_px(32.),
            cx,
        ),
        _ => unreachable!("only outline group rows use the group renderer"),
    }
}

/// A dynamic prefix header keeps disclosure and count outside the shrinking name.
pub(super) fn build_group_header(
    key: &str,
    label_text: &str,
    full_name: &str,
    count: usize,
    collapsed: bool,
    left_pad: gpui::Pixels,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let label = SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(label_text));
    let full = if label_text == full_name {
        label.clone()
    } else {
        SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(full_name))
    };
    let id = SharedString::from(format!("sidebar-group-{key}"));
    let tooltip_id = id.clone();
    let key_for_toggle = key.to_string();
    let toggle = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.toggle_sidebar_group(&key_for_toggle, cx);
    });
    div()
        .id(id.clone())
        .relative()
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.child(super::e2e::measure_inside(id.to_string()))
        })
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .items_center()
        .on_click(toggle)
        .tooltip(move |window, cx| {
            super::e2e::sidebar_labels::record_tooltip(&tooltip_id, &full);
            Tooltip::new(full.clone())
                .max_w(theme::scaled_px(340.))
                .build(window, cx)
        })
        .child(
            div()
                .h(theme::scaled_px(18.))
                .w_full()
                .min_w(gpui::px(0.))
                .flex()
                .items_center()
                .gap(theme::scaled_px(4.))
                .pl(left_pad)
                .pr(theme::scaled_px(12.))
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .hover(|s| s.bg(rgb(theme().surface)))
                .child(group_chevron(&id, collapsed))
                .child(
                    div()
                        .relative()
                        .min_w(gpui::px(0.))
                        .flex_1()
                        .truncate()
                        .child(label)
                        .when(cfg!(feature = "gui-e2e"), |el| {
                            el.child(super::e2e::measure_inside(format!("{id}-name")))
                        }),
                )
                .child(group_count(&id, count)),
        )
        .into_any_element()
}

fn group_chevron(id: &str, collapsed: bool) -> gpui::Div {
    div()
        .relative()
        .size(theme::scaled_px(10.))
        .flex_shrink_0()
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.child(super::e2e::measure_inside(format!("{id}-chevron")))
        })
        .child(
            gpui::svg()
                .path("icons/chevron-right.svg")
                .size(theme::scaled_px(10.))
                .text_color(rgb(theme().text_sub))
                .with_transformation(gpui::Transformation::rotate(gpui::radians(if collapsed {
                    0.
                } else {
                    std::f32::consts::FRAC_PI_2
                }))),
        )
}

fn group_count(id: &str, count: usize) -> gpui::Div {
    let digits = if count == 0 { 1 } else { count.ilog10() + 1 };
    let width = (digits as f32 * 7. + 12.).max(28.);
    div()
        .relative()
        .w(theme::scaled_px(width))
        .flex_shrink_0()
        .rounded_full()
        .bg(rgb(theme().bg_base))
        .text_size(theme::scaled_px(SIDEBAR_AUX_TEXT))
        .text_center()
        .child(count.to_string())
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.child(super::e2e::measure_inside(format!("{id}-count")))
        })
}

pub(super) fn build_tag_row(
    app: &KagiApp,
    tag_name: &str,
    tag_target: CommitId,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let label = SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(tag_name));
    let reason = tag_navigation_reason(app, &tag_target);
    let full = match reason {
        Some(reason) => SharedString::from(format!("{label}\n{}", reason.t())),
        None => label.clone(),
    };
    let id = SharedString::from(format!("sidebar-tag-{tag_name}"));
    let tooltip_id = id.clone();
    let menu_name = tag_name.to_string();
    let menu = cx.listener(move |app: &mut KagiApp, e: &gpui::MouseDownEvent, _w, cx| {
        app.open_tag_menu(menu_name.clone(), e.position);
        cx.stop_propagation();
        cx.notify();
    });
    let click = cx.listener(move |app: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        app.activate_sidebar_tag(&tag_target, cx);
    });
    div()
        .id(id.clone())
        .relative()
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.child(super::e2e::measure_inside(id.to_string()))
        })
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .items_center()
        .px_3()
        .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
        .line_height(gpui::relative(1.2))
        .text_color(rgb(if reason.is_some() {
            theme().text_muted
        } else {
            theme().color_tag
        }))
        .overflow_hidden()
        .tooltip(move |window, cx| {
            super::e2e::sidebar_labels::record_tooltip(&tooltip_id, &full);
            Tooltip::new(full.clone())
                .max_w(theme::scaled_px(340.))
                .build(window, cx)
        })
        .on_mouse_down(gpui::MouseButton::Right, menu)
        .on_click(click)
        .hover(|s| s.bg(rgb(theme().surface)))
        .child(
            div()
                .relative()
                .flex_1()
                .truncate()
                .line_height(gpui::relative(1.2))
                .child(label)
                .when(cfg!(feature = "gui-e2e"), |label| {
                    label.child(super::e2e::measure_inside(format!(
                        "sidebar-tag-name-{tag_name}"
                    )))
                }),
        )
        .into_any_element()
}

/// Activating a stash peeks without consuming it; writes remain in its menu.
pub(super) fn build_stash_row(
    index: usize,
    message: &str,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let message = kagi_domain::text_safety::sanitize_control_bytes(message);
    let label = SharedString::from(format!("stash@{{{index}}}: {message}"));
    let full = label.clone();
    let click = cx.listener(move |app: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        app.open_stash_peek(index, cx);
        cx.notify();
    });
    let menu = cx.listener(move |app: &mut KagiApp, e: &gpui::MouseDownEvent, _w, cx| {
        app.open_stash_menu(index, message.to_string(), e.position);
        cx.stop_propagation();
        cx.notify();
    });
    div()
        .id(("sidebar-stash", index))
        .relative()
        .when(cfg!(feature = "gui-e2e"), |row| {
            row.child(super::e2e::measure_inside(format!("sidebar-stash-{index}")))
        })
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .items_center()
        .px_3()
        .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
        .line_height(gpui::relative(1.2))
        .text_color(rgb(theme().color_warning))
        .overflow_hidden()
        .on_click(click)
        .on_mouse_down(gpui::MouseButton::Right, menu)
        .hover(|s| s.bg(rgb(theme().surface)))
        .tooltip(name_tooltip(full))
        .child(
            div()
                .relative()
                .flex_1()
                .truncate()
                .line_height(gpui::relative(1.2))
                .child(label)
                .when(cfg!(feature = "gui-e2e"), |label| {
                    label.child(super::e2e::measure_inside(format!(
                        "sidebar-stash-name-{index}"
                    )))
                }),
        )
        .into_any_element()
}

/// Match the tooltip identity without interpreting the namespaced collapse key.
pub(super) fn group_identity(
    row: &super::sidebar::SidebarRow,
) -> Option<std::borrow::Cow<'_, str>> {
    use super::sidebar::SidebarRow;
    match row {
        SidebarRow::LocalGroupHeader { prefix, .. } => Some(prefix.as_str().into()),
        SidebarRow::RemoteHeader { remote, .. } => Some(remote.as_str().into()),
        SidebarRow::RemoteSubGroup { remote, prefix, .. } => {
            Some(format!("{remote}/{prefix}").into())
        }
        _ => None,
    }
}

/// The context key reveals the same pinned tooltip as pointer hover, not a
/// bounded toast that would discard the end of a long identity.
pub(super) fn with_keyboard_group_reveal(
    app: &KagiApp,
    row: &super::sidebar::SidebarRow,
    focused: bool,
    el: gpui::Stateful<gpui::Div>,
    window: &mut gpui::Window,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    use super::sidebar::SidebarRow;
    let key = match row {
        SidebarRow::LocalGroupHeader { key, .. }
        | SidebarRow::RemoteHeader { key, .. }
        | SidebarRow::RemoteSubGroup { key, .. }
            if focused && app.sidebar.focus.group_reveal.as_ref() == Some(key) =>
        {
            key
        }
        _ => return el,
    };
    let Some(identity) = group_identity(row) else {
        return el;
    };
    let full = SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(&identity));
    let id = format!("sidebar-group-{key}");
    let tooltip = window.use_keyed_state(
        SharedString::from(format!(
            "sidebar-group-reveal-{key}-{}",
            f32::from(theme::scaled_px(340.)),
        )),
        cx,
        |_, _cx| Tooltip::new(full.clone()).max_w(theme::scaled_px(340.)),
    );
    super::e2e::sidebar_labels::record_tooltip(&id, &full);
    let card = div()
        .id(SharedString::from(format!("sidebar-group-tooltip-{key}")))
        .relative()
        .role(gpui::Role::Tooltip)
        .aria_label(full)
        .child(tooltip)
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.child(super::e2e::measure_inside(format!("{id}-reveal")))
        });
    el.key_context("KagiRowList SidebarGroupReveal")
        .on_action(cx.listener(|app, _: &super::CloseMainDiff, _w, cx| {
            app.sidebar.focus.group_reveal = None;
            cx.notify();
        }))
        .child(gpui::deferred(
            gpui::anchored()
                .position_mode(gpui::AnchoredPositionMode::Local)
                .position(gpui::point(
                    // RowList's transparent focus border is always in the layout.
                    theme::scaled_px(app.pane_fit.sidebar) - gpui::px(super::keyboard_nav::RING),
                    gpui::px(0.),
                ))
                .snap_to_window_with_margin(gpui::px(8.))
                .child(card),
        ))
}
