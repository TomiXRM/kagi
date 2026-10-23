//! Workspace-level controls stay visible while file navigation is hidden.
use super::*;
use crate::ui::{commands, keyboard_nav, render_header::count_badge};

#[cfg(feature = "gui-e2e")]
thread_local! { static CONTINUE_PRIMARY: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
#[cfg(feature = "gui-e2e")]
pub fn continue_primary_for_e2e() -> Option<bool> {
    CONTINUE_PRIMARY.with(|value| value.get())
}

impl ConflictView {
    pub(super) fn render_workspace(
        &mut self,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.mode.is_none() {
            return div().into_any_element();
        }
        let toolbar = self.render_workspace_toolbar(cx);
        let tree = if self.sidebar_hidden {
            None
        } else {
            Some(self.render_sidebar(active, window, cx))
        };
        let chrome = self.editor_chrome();
        let body = if self.selected_saved.is_some() {
            self.render_saved(&chrome, cx)
        } else {
            flow::render(self.mode.as_ref().expect("workspace mode"), &chrome, cx)
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .child(toolbar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .min_w(px(0.))
                    .children(tree)
                    .child(body),
            )
            .into_any_element()
    }

    fn render_workspace_toolbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mode = self.mode.as_ref().expect("workspace mode");
        let all_saved = mode.session.files.is_empty();
        #[cfg(feature = "gui-e2e")]
        CONTINUE_PRIMARY.with(|value| value.set(Some(all_saved)));
        let can_continue = mode.can_continue() && !self.writer_busy && !self.saved_review_needed;
        let tip = if self.sidebar_hidden {
            Msg::ConflictShowSidebar.t()
        } else {
            Msg::ConflictHideSidebar.t()
        };
        let shortcut = commands::effective_keystroke("view.toggleSidebar")
            .map(|s| commands::display_keystroke(&s))
            .unwrap_or_default();
        let tip = SharedString::from(format!("{tip} {shortcut}").trim_end().to_owned());
        let mut icon = div()
            .relative()
            .size(theme::scaled_px(22.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                gpui::svg()
                    .path("icons/panel-left.svg")
                    .size(theme::scaled_px(16.))
                    .text_color(rgb(theme().text_main)),
            );
        if self.sidebar_hidden {
            icon = icon.child(count_badge::render(
                "conflict-sidebar-toggle",
                mode.session.files.len(),
                true,
                theme::primary_button_foreground(&theme()),
            ));
        }
        let toggle = keyboard_nav::with_ring(
            div()
                .id("conflict-sidebar-toggle")
                .track_focus(self.sidebar_toggle_focus.as_ref().expect("toggle focus")),
        )
        .role(gpui::Role::Button)
        .aria_label(tip.clone())
        .h(theme::scaled_px(24.))
        .px(keyboard_nav::inset(4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(theme().surface))
        .when(!self.sidebar_hidden, |el| {
            el.bg(theme().segment_selected_bg())
        })
        .hover(|s| s.bg(rgb(theme().bg_row_alt)))
        .active(|s| s.bg(rgb(theme().surface)))
        .on_key_down(keyboard_nav::stop_activation_keys)
        .on_click(
            cx.listener(|view, _: &gpui::ClickEvent, window, cx| view.toggle_sidebar(window, cx)),
        )
        .child(icon)
        .relative()
        .child(e2e::measure_inside("conflict-sidebar-toggle"))
        .hover_tooltip("conflict-sidebar-help", tip);
        let continue_handler = cx.listener(
            |view: &mut ConflictView, _: &gpui::ClickEvent, window, cx| {
                let app = view.app.clone();
                let owner = view.owner.clone();
                cx.spawn_in(window, async move |_, cx| {
                    let _ = app.update_in(cx, |app, window, cx| {
                        app.conflict_continue(owner, window, cx)
                    });
                })
                .detach();
            },
        );
        let abort = cx.listener(
            |view: &mut ConflictView, _: &gpui::ClickEvent, window, cx| {
                let app = view.app.clone();
                let owner = view.owner.clone();
                cx.spawn_in(window, async move |_, cx| {
                    let _ = app.update_in(cx, |app, window, cx| {
                        app.open_conflict_abort_modal(owner, window, cx)
                    });
                })
                .detach();
            },
        );
        let path = self
            .selected_saved
            .as_ref()
            .or(self.editing.as_ref())
            .or_else(|| {
                mode.selected_file
                    .and_then(|i| mode.session.files.get(i).map(|f| &f.path))
            });
        let mut toolbar = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .min_w(px(0.))
            .bg(rgb(theme().surface))
            .border_b_1()
            .border_color(rgb(theme().sidebar));
        let row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px(theme::scaled_px(8.))
            .py(theme::scaled_px(4.))
            .child(toggle)
            .child(SharedString::from(mode.session.op.slug()))
            .when(
                matches!(
                    mode.session.op.kind(),
                    kagi_domain::conflict_family::ConflictOperationKind::Repository(_)
                ),
                |el| {
                    el.child(sidebar::chip(
                        Msg::ConflictNotCommitted.t(),
                        theme().text_main,
                    ))
                },
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis_middle()
                    .text_size(theme::scaled_px(11.))
                    .text_color(rgb(theme().text_sub))
                    .when_some(path, |el, path| {
                        el.child(SharedString::from(path.to_string_lossy().into_owned()))
                    }),
            )
            .child(e2e::measure_control(
                "conflict-abort",
                KagiButton::styled(
                    "conflict-abort",
                    Msg::ConflictAbort.t(),
                    ButtonRole::NeutralTinted,
                    cx,
                )
                .small()
                .disabled(self.writer_busy)
                .on_click(abort),
            ))
            .child(e2e::measure_control(
                "conflict-tools",
                KagiButton::styled(
                    "conflict-tools",
                    Msg::ConflictTools.t(),
                    ButtonRole::NeutralTinted,
                    cx,
                )
                .small()
                .on_click(cx.listener(|view, _: &gpui::ClickEvent, _, cx| {
                    view.show_tools = !view.show_tools;
                    cx.notify();
                })),
            ))
            .child(e2e::measure_control(
                "conflict-continue",
                KagiButton::styled(
                    "conflict-continue",
                    if matches!(
                        mode.session.op,
                        kagi_git::conflicts::ConflictOp::Merge { .. }
                    ) {
                        Msg::ConflictToCommit.t()
                    } else {
                        Msg::ConflictContinue.t()
                    },
                    if all_saved {
                        ButtonRole::Primary
                    } else {
                        ButtonRole::Secondary
                    },
                    cx,
                )
                .when(!all_saved, |button| button.outline())
                .small()
                .disabled(!can_continue)
                .on_click(continue_handler),
            ));
        toolbar = toolbar.child(row);
        if self.show_tools {
            let blocker = if self.saved_review_needed {
                Some(Msg::ConflictReviewNeeded)
            } else {
                mode.continue_blocker()
            };
            let mut tools = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .px(theme::scaled_px(8.))
                .pb(theme::scaled_px(4.))
                .when_some(blocker, |el, msg| {
                    el.child(
                        div()
                            .text_size(theme::scaled_px(11.))
                            .text_color(rgb(theme().color_blocker))
                            .child(msg.t()),
                    )
                });
            if mode.session.op.is_sequencer() {
                tools = tools.child(
                    KagiButton::styled(
                        "conflict-skip",
                        if mode.skip_armed {
                            Msg::ConflictConfirmSkip.t()
                        } else {
                            Msg::ConflictSkip.t()
                        },
                        ButtonRole::NeutralTinted,
                        cx,
                    )
                    .small()
                    .disabled(self.writer_busy)
                    .on_click(cx.listener(
                        |view, _: &gpui::ClickEvent, window, cx| {
                            if !view.skip_request_arm() {
                                cx.notify();
                                return;
                            }
                            let app = view.app.clone();
                            let owner = view.owner.clone();
                            cx.spawn_in(window, async move |_, cx| {
                                let _ =
                                    app.update_in(cx, |app, _, cx| app.conflict_skip(owner, cx));
                            })
                            .detach();
                        },
                    )),
                );
            }
            toolbar = toolbar.child(tools);
        }
        toolbar.into_any_element()
    }
}
