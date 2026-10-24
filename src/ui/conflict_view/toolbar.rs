//! File jump bar and unified-toolbar operation controls; neither owns a draft.
use super::*;
use crate::ui::{commands, keyboard_nav};

#[cfg(feature = "gui-e2e")]
thread_local! { static CONTINUE_PRIMARY: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
#[cfg(feature = "gui-e2e")]
pub fn continue_primary_for_e2e() -> Option<bool> {
    CONTINUE_PRIMARY.with(|value| value.get())
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static FILE_HEADER_STATUS: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

impl ConflictView {
    #[cfg(feature = "gui-e2e")]
    pub fn file_header_status_for_e2e(&self) -> String {
        FILE_HEADER_STATUS.with(|status| status.borrow().clone())
    }

    pub(super) fn render_workspace(
        &mut self,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.mode.is_none() {
            return div().into_any_element();
        }
        let now = kagi_ui_core::motion::now();
        let shown = !self.sidebar_hidden;
        self.panel_motion
            .sync_sidebar(shown, shown, now, kagi_ui_core::motion::instant());
        let fraction = self.panel_motion.left.visible(now);
        kagi_ui_core::motion::request_frame(window, self.panel_motion.left.animating(now));
        // A closing row can regain focus before its clip disappears.
        // Hand it back at the actual unmount boundary as well as on toggle.
        if fraction == 0. {
            self.yield_sidebar_focus(window, cx);
        }
        let tree = (fraction > 0.).then(|| {
            kagi_ui_core::motion::clip(
                "conflict-sidebar-clip",
                kagi_ui_core::motion::Axis::Horizontal,
                kagi_ui_core::motion::Anchor::End,
                theme::scaled_px(232.),
                fraction,
                self.render_sidebar(active, window, cx),
            )
            .child(e2e::measure_inside("conflict-sidebar-clip"))
        });
        let header = self.render_file_header(cx);
        let chrome = self.editor_chrome();
        let body = if self.selected_saved.is_some() {
            self.render_saved(&chrome, cx)
        } else {
            flow::render(self.mode.as_ref().expect("workspace mode"), &chrome, cx)
        };
        let paint_owner = cx.entity().downgrade();
        div()
            .relative()
            .flex()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .children(tree)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .child(header)
                    .child(body),
            )
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        let owner = paint_owner.clone();
                        window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, _, cx| {
                            if phase.capture() && event.button == gpui::MouseButton::Left {
                                let _ = owner.update(cx, |view, _| view.line_paint = None);
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    }

    fn render_file_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mode = self.mode.as_ref().expect("workspace mode");
        let path = self
            .selected_saved
            .as_ref()
            .or(self.editing.as_ref())
            .or_else(|| {
                mode.selected_file
                    .and_then(|i| mode.session.files.get(i).map(|f| &f.path))
            });
        let saved = self.selected_saved.is_some();
        let external = path.is_some_and(|path| mode.buffer.worktree_changed(path));
        let status = if saved {
            if self.saved_review_needed {
                Msg::ConflictReviewNeeded.t()
            } else {
                Msg::ConflictSavedStaged.t()
            }
            .to_owned()
        } else if external {
            Msg::ConflictWorktreeChanged.t().to_owned()
        } else if self.save_refused.as_ref() == path && path.is_some() {
            Msg::ConflictSaveRefused.t().to_owned()
        } else {
            path.map(|p| {
                if let Some(model) = mode.buffer.hunk_model(p) {
                    let resolved = model.regions.iter().filter(|region| {
                        matches!(region, kagi_git::resolution::Region::Hunk(hunk) if hunk.is_resolved())
                    }).count();
                    Msg::conflict_counts(model.hunk_count(), resolved)
                } else {
                    flow::draft_label(mode, p)
                }
            })
            .unwrap_or_default()
        };
        #[cfg(feature = "gui-e2e")]
        FILE_HEADER_STATUS.with(|value| value.borrow_mut().clone_from(&status));
        let mut header = div()
            .relative()
            .flex()
            .items_center()
            .flex_shrink_0()
            .h(theme::scaled_px(36.))
            .min_w(px(0.))
            .pl(theme::scaled_px(12.))
            .pr(theme::scaled_px(16.))
            .gap(theme::scaled_px(8.))
            .bg(rgb(theme().panel))
            .border_b_1()
            .border_color(rgb(theme().surface))
            .child(e2e::measure_inside("conflict-file-header"))
            .child(self.render_sidebar_toggle(cx))
            .when_some(path, |el, path| el.child(jump_path(path)))
            .child(
                div()
                    .id("conflict-file-count")
                    .role(gpui::Role::Status)
                    .aria_label(SharedString::from(status.clone()))
                    .text_size(theme::scaled_px(11.))
                    .text_color(rgb(
                        if external
                            || self.saved_review_needed
                            || self.save_refused.as_ref() == path && path.is_some()
                            || !saved && path.is_some_and(|p| flow::draft_ready(mode, p))
                        {
                            theme().color_warning
                        } else if saved {
                            theme().color_success
                        } else {
                            theme().text_sub
                        },
                    ))
                    .child(status),
            )
            .child(div().flex_1());
        if saved && !mode.session.files.is_empty() {
            header = header.child(e2e::measure_control(
                "conflict-next-file",
                compact_button(
                    "conflict-next-file",
                    Msg::ConflictNextFile.t(),
                    ButtonRole::Secondary,
                    cx,
                )
                .on_click(cx.listener(|view, _: &gpui::ClickEvent, window, cx| {
                    view.conflict_open_file(0, cx);
                    view.focus_navigation(window, cx);
                    cx.notify();
                })),
            ));
        }
        if external {
            header = header.child(e2e::measure_control(
                "conflict-reload",
                compact_button(
                    "conflict-reload",
                    Msg::ConflictReload.t(),
                    ButtonRole::Secondary,
                    cx,
                )
                .on_click(cx.listener(|view, _: &gpui::ClickEvent, _, cx| {
                    let app = view.app.clone();
                    cx.defer(move |cx| {
                        let _ = app.update(cx, |app, cx| app.reload_external(cx));
                    });
                })),
            ));
        }
        if !saved {
            if let Some(path) = path.filter(|p| {
                mode.session
                    .files
                    .iter()
                    .any(|f| &f.path == *p && f.kind != ConflictKind::DirFile)
            }) {
                header = header.child(crate::ui::conflict_editor::save_button(
                    path,
                    self.writer_busy,
                    external,
                    !external && self.save_refused.as_ref() == Some(path),
                    flow::draft_ready(mode, path),
                    cx,
                ));
            }
        }
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(header)
            .when(external, |column| {
                column.child(
                    div()
                        .px(theme::scaled_px(12.))
                        .py(theme::scaled_px(6.))
                        .text_size(theme::scaled_px(11.))
                        .text_color(rgb(theme().color_warning))
                        .child(Msg::ConflictWorktreeChangedHelp.t()),
                )
            })
            .into_any_element()
    }

    fn render_sidebar_toggle(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tip = if self.sidebar_hidden {
            Msg::ConflictShowSidebar.t()
        } else {
            Msg::ConflictHideSidebar.t()
        };
        let shortcut = commands::effective_keystroke("view.toggleSidebar")
            .map(|s| commands::display_keystroke(&s))
            .unwrap_or_default();
        let tip = SharedString::from(format!("{tip} {shortcut}").trim_end().to_owned());
        keyboard_nav::with_ring(
            div()
                .id("conflict-sidebar-toggle")
                .track_focus(self.sidebar_toggle_focus.as_ref().expect("toggle focus")),
        )
        .role(gpui::Role::Button)
        .aria_label(tip.clone())
        .h(theme::scaled_px(20.))
        .px(keyboard_nav::inset(4.))
        .flex()
        .items_center()
        .gap(theme::scaled_px(4.))
        .flex_shrink_0()
        .rounded(theme::scaled_px(5.))
        .cursor_pointer()
        .when(!self.sidebar_hidden, |el| el.bg(rgb(theme().surface)))
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_key_down(keyboard_nav::stop_activation_keys)
        .on_click(
            cx.listener(|view, _: &gpui::ClickEvent, window, cx| view.toggle_sidebar(window, cx)),
        )
        .child(
            gpui::svg()
                .path("icons/panel-left.svg")
                .size(theme::scaled_px(13.))
                .text_color(rgb(theme().text_main)),
        )
        .when(self.sidebar_hidden, |el| {
            let count = SharedString::from(
                self.mode
                    .as_ref()
                    .map_or(0, |m| m.session.files.len())
                    .to_string(),
            );
            el.child(
                div()
                    .id("conflict-sidebar-inline-count")
                    .role(gpui::Role::Status)
                    .aria_label(count.clone())
                    .relative()
                    .text_size(theme::scaled_px(11.))
                    .line_height(theme::scaled_px(14.))
                    .text_color(rgb(theme().ui_accent()))
                    .child(count)
                    .child(e2e::measure_inside("conflict-sidebar-inline-count")),
            )
        })
        .relative()
        .child(e2e::measure_inside("conflict-sidebar-toggle"))
        .hover_tooltip("conflict-sidebar-help", tip)
        .into_any_element()
    }

    /// Only strip-visible changes invalidate the parent, not every editor keystroke.
    pub(crate) fn operation_chrome_state(&self) -> (bool, bool, bool, bool, Option<&'static str>) {
        let mode = self.mode.as_ref();
        let blocker = if self.saved_review_needed {
            Some(Msg::ConflictReviewNeeded)
        } else {
            mode.and_then(|m| m.continue_blocker())
        };
        (
            mode.is_some_and(|m| m.session.files.is_empty()),
            mode.is_some_and(|m| m.can_continue())
                && !self.writer_busy
                && !self.saved_review_needed,
            self.writer_busy,
            mode.is_some_and(|m| m.skip_approval.is_some()),
            blocker.map(|msg| msg.t()),
        )
    }

    /// Rendered by the app's unified toolbar, but callbacks remain session-bound.
    pub(in crate::ui) fn render_operation_strip(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(mode) = &self.mode else {
            return div().into_any_element();
        };
        let all_saved = mode.session.files.is_empty();
        #[cfg(feature = "gui-e2e")]
        CONTINUE_PRIMARY.with(|value| value.set(Some(all_saved)));
        let can_continue = mode.can_continue() && !self.writer_busy && !self.saved_review_needed;
        let blocker = if self.saved_review_needed {
            Some(Msg::ConflictReviewNeeded)
        } else {
            mode.continue_blocker()
        };
        div()
            .relative()
            .flex()
            .items_center()
            .flex_shrink_0()
            .gap(theme::scaled_px(8.))
            .mr(theme::scaled_px(16.))
            .text_size(theme::scaled_px(12.))
            .text_color(rgb(theme().text_sub))
            .child(e2e::measure_inside("conflict-operation-strip"))
            .child(
                div()
                    .size(theme::scaled_px(8.))
                    .rounded_full()
                    .bg(rgb(theme().color_warning)),
            )
            .child(SharedString::from(mode.session.op.slug()))
            .child(e2e::measure_control(
                "conflict-abort",
                compact_button(
                    "conflict-abort",
                    Msg::ConflictAbort.t(),
                    ButtonRole::Secondary,
                    cx,
                )
                .disabled(self.writer_busy)
                .on_click(cx.listener(|view, _: &gpui::ClickEvent, window, cx| {
                    let app = view.app.clone();
                    let owner = view.owner.clone();
                    cx.spawn_in(window, async move |_, cx| {
                        let _ = app.update_in(cx, |app, window, cx| {
                            app.open_conflict_abort_modal(owner, window, cx)
                        });
                    })
                    .detach();
                })),
            ))
            .when(mode.session.op.is_sequencer(), |el| {
                el.child(
                    compact_button(
                        "conflict-skip",
                        if mode.skip_approval.is_some() {
                            Msg::ConflictConfirmSkip.t()
                        } else {
                            Msg::ConflictSkip.t()
                        },
                        ButtonRole::Secondary,
                        cx,
                    )
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
                )
            })
            .child(e2e::measure_control(
                "conflict-continue",
                compact_button(
                    "conflict-continue",
                    if all_saved
                        && matches!(
                            mode.session.op,
                            kagi_git::conflicts::ConflictOp::Merge { .. }
                        )
                    {
                        Msg::ConflictToCommit.t()
                    } else {
                        Msg::ConflictReviewContinue.t()
                    },
                    if all_saved {
                        ButtonRole::Primary
                    } else {
                        ButtonRole::Secondary
                    },
                    cx,
                )
                .disabled(!can_continue)
                .when_some(blocker, |button, reason| {
                    button.aria_description(reason.t())
                })
                .on_click(cx.listener(|view, _: &gpui::ClickEvent, window, cx| {
                    let app = view.app.clone();
                    let owner = view.owner.clone();
                    cx.spawn_in(window, async move |_, cx| {
                        let _ = app.update_in(cx, |app, window, cx| {
                            app.conflict_continue(owner, window, cx)
                        });
                    })
                    .detach();
                }))
                .hover_tooltip(
                    "conflict-continue-help",
                    blocker
                        .map(|msg| msg.t())
                        .unwrap_or(Msg::ConflictReviewContinue.t()),
                ),
            ))
            .into_any_element()
    }
}

fn compact_button(
    id: &'static str,
    label: &'static str,
    role: ButtonRole,
    cx: &gpui::App,
) -> Button {
    KagiButton::styled(id, label, role, cx)
        .small()
        .h(theme::scaled_px(20.))
        .rounded(theme::scaled_px(5.))
        .text_size(theme::scaled_px(12.))
}

fn jump_path(path: &std::path::Path) -> gpui::Div {
    let mut bar = div()
        .flex()
        .items_center()
        .flex_shrink_0()
        .max_w(theme::scaled_px(380.))
        .overflow_hidden()
        .gap(theme::scaled_px(6.))
        .text_size(theme::scaled_px(12.));
    let mut components = path.components().peekable();
    let mut first = true;
    while let Some(component) = components.next() {
        if !first {
            bar = bar.child(
                gpui_component::Icon::new(gpui_component::IconName::ChevronRight)
                    .size(theme::scaled_px(10.))
                    .text_color(rgb(theme().text_sub)),
            );
        }
        first = false;
        let last = components.peek().is_none();
        bar = bar
            .child(
                gpui::svg()
                    .path(if last {
                        "icons/file-text.svg"
                    } else {
                        "icons/folder-open.svg"
                    })
                    .size(theme::scaled_px(12.))
                    .flex_shrink_0()
                    .text_color(rgb(if last {
                        theme().text_sub
                    } else {
                        theme().ui_accent()
                    })),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .text_color(rgb(if last {
                        theme().text_main
                    } else {
                        theme().text_sub
                    }))
                    .child(SharedString::from(
                        component.as_os_str().to_string_lossy().into_owned(),
                    )),
            );
    }
    bar
}
