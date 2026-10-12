//! Commit Panel keyboard frames and ghost composer actions.
// Render parameters retain the existing theme/geometry/action contract explicitly.
#![allow(clippy::too_many_arguments)]
use crate::ui::{
    commit_panel::CommitPanelView,
    e2e,
    i18n::Msg,
    keyboard_nav,
    theme::{self, theme},
};
use gpui::{div, prelude::*, px, rgb, App, ClickEvent, Context, FocusHandle, Role, Window};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    Icon, Sizable as _,
};

/// The named focus target extends outside the paint box without changing flow.
/// Negative vertical margins collapse the shrinking composer's intrinsic basis.
pub(super) fn action_frame(
    id: &'static str,
    slot: usize,
    label: &'static str,
    focus: FocusHandle,
    tab_stop: bool,
    disabled: bool,
    pressed: Option<bool>,
    reason: Option<gpui::SharedString>,
    content: impl IntoElement,
    activate: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    e2e::commit_panel_labels::focus(id, &focus);
    let disabled = e2e::commit_panel_labels::disabled(id, disabled);
    let pointer_focus = focus.clone();
    let frame = keyboard_nav::focusable(div().id(id))
        .absolute()
        .top(px(-keyboard_nav::RING))
        .bottom(px(-keyboard_nav::RING))
        .left(px(-keyboard_nav::RING))
        .right(px(-keyboard_nav::RING))
        .track_focus(&focus.tab_index(0).tab_stop(tab_stop))
        .rounded_sm()
        .role(e2e::commit_panel_labels::role(id, Role::Button))
        .aria_label(e2e::commit_panel_labels::name(id, label))
        .when_some(pressed, |el, pressed| {
            el.aria_toggled(if e2e::commit_panel_labels::pressed(id, pressed) {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
        })
        .a11y_synthetic_children(move |builder: &mut gpui::A11ySubtreeBuilder| {
            if disabled {
                builder.parent_node().set_disabled();
            }
        })
        .when_some(reason, |el, reason| {
            el.aria_description(e2e::commit_panel_labels::description(id, reason.clone()))
                .tooltip(move |w, cx| {
                    gpui_component::tooltip::Tooltip::new(reason.clone()).build(w, cx)
                })
        })
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            pointer_focus.focus(window, cx);
        })
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            if !disabled {
                activate(event, window, cx);
            }
        });
    div()
        .id(("cp-control-layout", slot))
        .relative()
        .flex_shrink_0()
        .child(content)
        .child(frame)
}

fn button_paint(button: impl IntoElement) -> gpui::Stateful<gpui::Div> {
    div()
        .id("cp-button-paint")
        .role(Role::Group)
        .flex_shrink_0()
        .a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
            builder.parent_node().set_hidden();
        })
        .child(button)
}

pub(super) fn footer_button(
    view: &CommitPanelView,
    id: &'static str,
    slot: usize,
    icon: Option<&'static str>,
    label: &'static str,
    tip: impl Into<gpui::SharedString>,
    description: Option<gpui::SharedString>,
    enabled: bool,
    pressed: Option<bool>,
    accent: u32,
    cx: &mut Context<CommitPanelView>,
    activate: impl Fn(&mut CommitPanelView, &ClickEvent, &mut Window, &mut Context<CommitPanelView>)
        + Clone
        + 'static,
) -> gpui::Stateful<gpui::Div> {
    let tip = tip.into();
    let pointer = activate.clone();
    let keyboard = activate;
    let focus = view.controls_focus.get(slot, cx);
    let size = theme::scaled_px(16.) + px(theme::rem_size_px() * 0.5);
    let generating = slot == 4 && view.smart_generating;
    let button = Button::new(id)
        .ghost()
        .small()
        .tab_stop(false)
        .h(size)
        .when(icon.is_some(), |b| b.w(size))
        .when_some(icon.filter(|_| !generating), |b, path| {
            b.icon(
                Icon::empty()
                    .path(path)
                    .size(theme::scaled_px(16.))
                    .text_color(rgb(if enabled { accent } else { theme().text_muted })),
            )
        })
        .when(generating, |b| b.child(generation_glyph(accent)))
        .when(icon.is_none(), |b| {
            b.icon(
                Icon::empty()
                    .path("icons/file-text.svg")
                    .size(theme::scaled_px(16.))
                    .text_color(rgb(accent)),
            )
            .label(e2e::commit_panel_labels::caption(id, Msg::Template.t()))
            .px_1p5()
        })
        .text_xs()
        .text_color(rgb(if enabled { accent } else { theme().text_muted }))
        .tooltip(tip.clone())
        .relative()
        .child(e2e::measure_inside(id))
        .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
            cx.stop_propagation();
            if enabled {
                pointer(view, event, window, cx);
            }
        }));
    action_frame(
        id,
        slot,
        label,
        focus,
        true,
        !enabled,
        pressed,
        description,
        button_paint(button),
        cx.listener(move |view, event, window, cx| keyboard(view, event, window, cx)),
    )
    .child(e2e::commit_panel_labels::measure_hit_box(
        match slot {
            3 => "cp-template-toggle-hit",
            4 => "cp-smart-suggest-hit",
            5 => "cp-coauthor-hit",
            6 => "cp-amend-btn-hit",
            _ => unreachable!("footer action slot"),
        },
        keyboard_nav::RING,
    ))
}

/// Release hidden bulk actions, or empty actions reached by pointer, to navigation.
/// Like `release_closed` / `yield_focus`, unrelated input or modal focus stays put.
pub(super) fn reconcile_bulk_focus(
    view: &CommitPanelView,
    window: &mut Window,
    cx: &mut Context<CommitPanelView>,
) {
    let compact = super::compact::for_window(window);
    let keyboard = window.last_input_was_keyboard();
    if !compact && view.staged_view_focus.focused(window).is_some() {
        let handles = view.view_focus.sync(&[0, 1], cx);
        view.staged_view_focus.yield_focus(
            Some(&handles[usize::from(view.state.tree_view)]),
            window,
            cx,
        );
    }
    let release = match view.controls_focus.focused(window) {
        Some(0) => (!compact || !keyboard) && view.state.unstaged.is_empty(),
        Some(1) => view.state.unstaged.is_empty(),
        Some(2) => (!compact || !keyboard) && view.state.staged.is_empty(),
        _ => false,
    };
    if release {
        if !keyboard {
            if let Some(root) = &view.pointer_root_focus {
                root.focus(window, cx);
                return;
            }
        }
        let handles = view.view_focus.sync(&[0, 1], cx);
        handles[usize::from(view.state.tree_view)].focus(window, cx);
    }
}

pub(super) fn view_selector(
    view: &CommitPanelView,
    staged: bool,
    cx: &mut Context<CommitPanelView>,
) -> gpui::Stateful<gpui::Div> {
    let weak = cx.entity().downgrade();
    let tabs = keyboard_nav::TabList::new(
        if staged {
            &view.staged_view_focus
        } else {
            &view.view_focus
        },
        keyboard_nav::slot_keys(2),
        vec![0, 1],
        Some(usize::from(view.state.tree_view)),
        keyboard_nav::Activation::Automatic,
        view.pointer_root_focus.clone(),
        move |slot, _, _, cx| {
            let _ = weak.update(cx, |view, cx| {
                view.state.tree_view = slot == 1;
                cx.notify();
            });
        },
        cx,
    );
    let cell = |slot, id: &'static str, label: &'static str| {
        let selected = view.state.tree_view == (slot == 1);
        e2e::commit_panel_labels::focus(id, tabs.focus_handle(slot));
        tabs.cell(
            slot,
            label,
            div()
                .id(id)
                .m(px(-keyboard_nav::RING))
                .relative()
                .role(e2e::commit_panel_labels::role(id, Role::Tab))
                .aria_label(e2e::commit_panel_labels::name(id, label))
                .aria_selected(e2e::commit_panel_labels::selected(id, selected))
                .child(e2e::measure_inside(id))
                .child(
                    div()
                        .min_h(px(20.))
                        .flex_shrink_0()
                        .px_1p5()
                        .py_px()
                        .text_xs()
                        .bg(if selected {
                            theme().selected_for_window(view.window_active)
                        } else {
                            rgb(theme().surface).into()
                        })
                        .text_color(rgb(if selected {
                            theme().text_main
                        } else {
                            theme().text_muted
                        }))
                        .hover(|s| s.cursor_default())
                        .child(e2e::commit_panel_labels::caption(id, label)),
                ),
        )
    };
    let group_id = if staged {
        "cp-staged-view-group"
    } else {
        "cp-view-group"
    };
    tabs.list(
        div()
            .id(group_id)
            .flex()
            .flex_row()
            .flex_shrink_0()
            .rounded_sm()
            .border_1()
            .border_color(rgb(theme().surface)),
    )
    .key_context("KagiTabList KagiCommitPanelView")
    .role(e2e::commit_panel_labels::role(group_id, Role::TabList))
    .aria_label(e2e::commit_panel_labels::name(
        group_id,
        Msg::CommitPanelTitle.t(),
    ))
    .child(cell(
        0,
        if staged {
            "cp-staged-view-list"
        } else {
            "cp-view-list"
        },
        Msg::CommitPanelList.t(),
    ))
    .child(cell(
        1,
        if staged {
            "cp-staged-view-tree"
        } else {
            "cp-view-tree"
        },
        Msg::CommitPanelTree.t(),
    ))
}

fn generation_glyph(accent: u32) -> gpui::AnyElement {
    use gpui::AnimationExt as _;
    let icon = gpui::svg()
        .path("icons/loader-circle.svg")
        .size(theme::scaled_px(16.))
        .text_color(rgb(accent));
    if theme::reduce_motion() {
        icon.into_any_element()
    } else {
        icon.with_animation(
            "cp-smart-spinner",
            gpui::Animation::new(std::time::Duration::from_millis(900)).repeat(),
            |svg, delta| {
                svg.with_transformation(gpui::Transformation::rotate(gpui::radians(
                    delta * std::f32::consts::TAU,
                )))
            },
        )
        .into_any_element()
    }
}

pub(super) fn icon_row(view: &CommitPanelView, cx: &mut Context<CommitPanelView>) -> gpui::Div {
    let llm_on = view.smart_snapshot.llm_offered();
    let enabled = !view.state.staged.is_empty() && !view.smart_generating;
    let generation = view
        .smart_generation
        .as_ref()
        .filter(|_| view.smart_generating);
    let provider_help = generation.is_some() || (llm_on && enabled);
    let tip: gpui::SharedString = if let Some(generation) = generation {
        kagi_ui_core::i18n::smart_footer_provider(
            generation.provider.display_name(),
            generation.model.as_deref(),
            true,
        )
        .into()
    } else if llm_on && enabled {
        let state = &view.smart_snapshot;
        let model = (state.provider == crate::ui::smart_commit::SmartProvider::Ollama)
            .then_some(state.model.as_deref())
            .flatten();
        kagi_ui_core::i18n::smart_footer_provider(state.provider.display_name(), model, false)
            .into()
    } else if view.state.staged.is_empty() {
        Msg::AdviceCommitNothingStaged.t().into()
    } else {
        Msg::GenerateMessage.t().into()
    };
    let accent = if llm_on {
        theme().color_success
    } else {
        theme().accent_text_on(theme().panel)
    };
    let suggest = footer_button(
        view,
        "cp-smart-suggest",
        4,
        Some("icons/sparkles.svg"),
        Msg::GenerateMessage.t(),
        tip.clone(),
        (provider_help || !enabled).then_some(tip),
        enabled,
        None,
        accent,
        cx,
        move |view, _, window, cx| {
            if llm_on {
                view.defer_smart_generate(window, cx);
            } else {
                view.defer_smart_suggest(window, cx);
            }
        },
    );
    let coauthor = footer_button(
        view,
        "cp-coauthor",
        5,
        Some("icons/user-plus.svg"),
        Msg::AddCoAuthor.t(),
        Msg::AddCoAuthor.t(),
        None,
        true,
        None,
        theme().text_main,
        cx,
        |view, _, window, cx| view.toggle_coauthor_menu(window, cx),
    );
    let amend = footer_button(
        view,
        "cp-amend-btn",
        6,
        Some("icons/undo-2.svg"),
        Msg::AmendLastCommit.t(),
        Msg::AmendLastCommit.t(),
        None,
        true,
        None,
        theme().color_warning,
        cx,
        |view, _, window, cx| view.defer_amend(window, cx),
    );
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(suggest)
        .child(coauthor)
        .child(amend)
}

pub(super) fn template_toggle(
    view: &CommitPanelView,
    cx: &mut Context<CommitPanelView>,
) -> gpui::AnyElement {
    if view.commit_template.is_none() {
        return div().into_any_element();
    }
    let on = view.template_active(cx);
    footer_button(
        view,
        "cp-template-toggle",
        3,
        None,
        Msg::ToggleCommitTemplate.t(),
        Msg::ToggleCommitTemplate.t(),
        None,
        true,
        Some(on),
        if on {
            theme().accent_text_on(theme().panel)
        } else {
            theme().text_muted
        },
        cx,
        |view, _, window, cx| view.toggle_template(window, cx),
    )
    .into_any_element()
}

pub(super) fn commit_button(
    view: &CommitPanelView,
    enabled: bool,
    cx: &mut Context<CommitPanelView>,
) -> gpui::Stateful<gpui::Div> {
    use crate::ui::button_style::{ButtonRole, KagiButton};
    use gpui_component::Disableable as _;
    let label = crate::ui::i18n::commit_button();
    let focus = view.controls_focus.get(7, cx);
    let button = KagiButton::styled("cp-commit-inner", label, ButtonRole::Primary, cx)
        .small()
        .mt_1()
        .w_full()
        .tab_stop(false)
        .disabled(!enabled)
        .when(enabled, |b| {
            b.on_click(
                cx.listener(|view: &mut CommitPanelView, _: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    view.defer_open_commit_plan_modal(window, cx);
                }),
            )
        });
    action_frame(
        "cp-commit-btn",
        7,
        label,
        focus.clone(),
        enabled,
        !enabled,
        None,
        None,
        button_paint(button).w_full(),
        cx.listener(|view: &mut CommitPanelView, _, window, cx| {
            view.defer_open_commit_plan_modal(window, cx);
        }),
    )
    .w_full()
    .flex_shrink_0()
    .child(e2e::measure_inside("cp-commit-action"))
}

fn bulk_button(
    view: &CommitPanelView,
    id: &'static str,
    slot: usize,
    msg: Msg,
    enabled: bool,
    reason: Option<&'static str>,
    color: u32,
    cx: &mut Context<CommitPanelView>,
    activate: impl Fn(&CommitPanelView, &mut Window, &mut Context<CommitPanelView>) + Clone + 'static,
) -> gpui::Stateful<gpui::Div> {
    let pointer = activate.clone();
    let keyboard = activate;
    let content = div()
        .id(("cp-bulk-paint", slot))
        .relative()
        .min_h(px(20.))
        .flex_shrink_0()
        .px_1p5()
        .py_px()
        .rounded_sm()
        .bg(rgb(theme().surface))
        .text_xs()
        .text_color(rgb(if enabled { color } else { theme().text_muted }))
        .when(enabled, |el| {
            el.hover(|s| s.bg(rgb(theme().selected)).cursor_default())
        })
        .on_click(cx.listener(
            move |view: &mut CommitPanelView, _: &ClickEvent, window, cx| {
                if enabled {
                    pointer(view, window, cx);
                }
            },
        ))
        .child(e2e::measure_inside(id))
        .child(e2e::commit_panel_labels::caption(id, msg.t()));
    action_frame(
        id,
        slot,
        msg.t(),
        view.controls_focus.get(slot, cx),
        true,
        !enabled,
        None,
        reason.map(Into::into),
        content,
        cx.listener(move |view: &mut CommitPanelView, _, window, cx| keyboard(view, window, cx)),
    )
    .child(e2e::commit_panel_labels::measure_hit_box(
        match slot {
            0 => "cp-stage-all-hit",
            1 => "cp-discard-all-hit",
            2 => "cp-unstage-all-hit",
            _ => unreachable!("bulk action slot"),
        },
        keyboard_nav::RING,
    ))
}

pub(super) fn unstaged_header(
    view: &CommitPanelView,
    compact: bool,
    cx: &mut Context<CommitPanelView>,
) -> gpui::Div {
    let count = view.state.unstaged.len();
    let eligible = view
        .state
        .unstaged
        .iter()
        .filter(|f| !view.state.is_conflicted(&f.path))
        .count();
    let header = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .when(compact, |el| el.gap_1())
        .items_center()
        .px_2()
        .py_1()
        .flex_shrink_0()
        .child(
            div()
                .flex_grow(1.)
                .flex_shrink_0()
                .when(compact, |el| el.w_full().flex_none())
                .relative()
                .child(e2e::measure_inside("cp-unstaged-heading"))
                .text_sm()
                .text_color(rgb(theme().text_label))
                .child(e2e::commit_panel_labels::caption(
                    "cp-unstaged-heading",
                    crate::ui::i18n::commit_panel_section(false, count),
                )),
        );
    let header = if compact || count > 0 {
        let stage = bulk_button(
            view,
            "cp-stage-all",
            0,
            Msg::CommitPanelStageAll,
            eligible > 0,
            (eligible == 0).then(|| {
                if count == 0 {
                    Msg::CommitPanelNothingUnstaged.t()
                } else {
                    Msg::CommitPanelNoStageTargets.t()
                }
            }),
            theme().color_success,
            cx,
            |view, window, cx| view.defer_stage_all(window, cx),
        )
        .mr_2();
        header.child(stage).when(count > 0, |el| {
            el.child(
                bulk_button(
                    view,
                    "cp-discard-all",
                    1,
                    Msg::CommitPanelDiscardAll,
                    eligible > 0,
                    (eligible == 0).then(|| Msg::ModalNoDiscardTargets.t()),
                    theme().color_blocker,
                    cx,
                    |view, window, cx| view.defer_open_discard_all(window, cx),
                )
                .mr_2(),
            )
        })
    } else {
        header
    };
    header.child(view_selector(view, false, cx))
}

pub(super) fn staged_header(
    view: &CommitPanelView,
    compact: bool,
    cx: &mut Context<CommitPanelView>,
) -> gpui::Div {
    let count = view.state.staged.len();
    let header = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .when(compact, |el| el.gap_1())
        .items_center()
        .px_2()
        .py_1()
        .flex_shrink_0()
        .child(
            div()
                .flex_grow(1.)
                .flex_shrink_0()
                .when(compact, |el| el.w_full().flex_none())
                .relative()
                .child(e2e::measure_inside("cp-staged-heading"))
                .text_sm()
                .text_color(rgb(theme().text_label))
                .child(e2e::commit_panel_labels::caption(
                    "cp-staged-heading",
                    crate::ui::i18n::commit_panel_section(true, count),
                )),
        );
    let header = if compact || count > 0 {
        header.child(bulk_button(
            view,
            "cp-unstage-all",
            2,
            Msg::CommitPanelUnstageAll,
            count > 0,
            (count == 0).then(|| Msg::AdviceCommitNothingStaged.t()),
            theme().color_warning,
            cx,
            |view, window, cx| view.defer_unstage_all(window, cx),
        ))
    } else {
        header
    };
    header.when(compact, |el| el.child(view_selector(view, true, cx)))
}
