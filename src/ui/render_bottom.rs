//! Bottom-panel renderer (Operation Log / Terminal / Activity tabs) split out
//! of `render.rs` (T-SPLIT-RENDER-001 / ADR-0116 Wave 3). `render_body` places
//! the panel below the center pane, beside the full-height sidebar and inspector.

use super::*;

impl KagiApp {
    #[cfg(feature = "gui-e2e")]
    pub fn bottom_tab_focused_for_e2e(&self, window: &Window) -> Option<usize> {
        self.bottom_tab_focus.focused(window)
    }

    #[cfg(feature = "gui-e2e")]
    pub fn bottom_tab_attributes_for_e2e(&self) -> Vec<(gpui::Role, String, bool, bool)> {
        self.bottom_tab_focus.recorded_tabs()
    }

    /// Bottom panel slot — T-BP-002: open/close + height resize.
    ///
    /// Returns `None` when the panel is closed (so `div().children(…)` adds no
    /// child element).  When open, returns the panel div with:
    /// - a 4px horizontal divider at the top (drag to resize)
    /// - a tab bar (OperationLog / Terminal)
    /// - a placeholder body area
    ///
    /// `visible` is the shown fraction of its height while it opens or
    /// closes (#950): the panel keeps its full height inside a box that
    /// clips it, so the Terminal's grid and PTY size stay put.
    pub(super) fn render_bottom_panel_slot(
        &mut self,
        visible: f32,
        height: f32,
        active_tab: BottomTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if visible <= 0. {
            self.bottom_tab_focus
                .yield_focus(self.root_focus.as_ref(), window, cx);
            return None;
        }
        let compact = self.compact.enabled;
        let active_tab = if !compact && active_tab == BottomTab::Changes {
            BottomTab::OperationLog
        } else {
            active_tab
        };

        // ── Horizontal resize divider at top of panel ──
        let h_divider = div()
            .id("divider-bottom-panel")
            .group("divider-bottom-panel")
            .w_full()
            .h(theme::scaled_px(BOTTOM_PANEL_DIVIDER_H))
            .flex_shrink_0()
            .relative()
            .when(cfg!(feature = "gui-e2e"), |divider| {
                divider.child(e2e::measure_inside("divider-bottom-panel"))
            })
            .bg(rgb(theme().panel))
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(1.))
                    .flex_shrink_0()
                    .bg(rgb(theme().surface))
                    .group_hover("divider-bottom-panel", |s| s.bg(rgb(theme().ui_accent())))
                    .group_drag_over::<DividerDrag>("divider-bottom-panel", |s| {
                        s.bg(rgb(theme().ui_accent()))
                    })
                    .when(cfg!(feature = "gui-e2e"), |line| {
                        line.child(e2e::measure_inside("divider-bottom-panel-line"))
                    }),
            )
            .hover(|style| style.bg(rgb(theme().ui_accent())).cursor_row_resize())
            .drag_over::<DividerDrag>(|style, drag, _, _| {
                if drag.kind == DividerKind::BottomPanel {
                    style.bg(rgb(theme().ui_accent()))
                } else {
                    style
                }
            })
            .cursor_row_resize()
            .on_drag(
                DividerDrag {
                    kind: DividerKind::BottomPanel,
                },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        // Manual activation: focusing Terminal must never start its process.
        const TABS: [BottomTab; 4] = [
            BottomTab::OperationLog,
            BottomTab::Terminal,
            BottomTab::Activity,
            BottomTab::Changes,
        ];
        let entity = cx.weak_entity();
        let tabs = keyboard_nav::TabList::new(
            &self.bottom_tab_focus,
            keyboard_nav::slot_keys(if compact { 4 } else { 3 }),
            if compact {
                vec![3, 0, 1, 2]
            } else {
                vec![0, 1, 2]
            },
            TABS.iter().position(|&tab| tab == active_tab),
            keyboard_nav::Activation::Manual,
            // This selector routes pointer focus itself: Terminal receives
            // text input, while the other panes return to the app root.
            None,
            move |slot, is_keyboard, window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    let kept = is_keyboard
                        .then(|| this.bottom_tab_focus.focused_cell(window))
                        .flatten();
                    this.bottom_tab = TABS[slot];
                    if this.bottom_tab == BottomTab::Terminal {
                        this.ensure_terminal(window, cx);
                    }
                    if this.bottom_tab == BottomTab::Changes {
                        this.open_compact_changes(window, cx);
                    }
                    // Lazy startup focuses the terminal; keyboard activation
                    // must keep the selector's focus until Tab leaves it.
                    if let Some(cell) = kept {
                        window.focus(&cell, cx);
                    }
                    if !is_keyboard && this.bottom_tab != BottomTab::Terminal {
                        if let Some(root) = &this.root_focus {
                            root.focus(window, cx);
                        }
                    }
                    cx.notify();
                });
            },
            cx,
        );
        self.bottom_tab_focus
            .release_closed(self.root_focus.as_ref(), window, cx);
        let cell = |slot: usize, id: &'static str| {
            let tab = TABS[slot];
            let selected = tab == active_tab;
            tabs.cell(slot, tab.label(), div().id(id))
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .h(theme::scaled_px(BOTTOM_PANEL_TAB_H))
                .px(keyboard_nav::inset(12.))
                .rounded(theme::scaled_px(4.))
                .text_xs()
                .cursor_pointer()
                .text_color(rgb(if selected {
                    theme().text_main
                } else {
                    theme().text_muted
                }))
                .when(selected, |cell| {
                    cell.bg(theme().selected_for_window(self.window_active))
                        .font_weight(gpui::FontWeight::MEDIUM)
                })
                .when(!selected, |cell| {
                    cell.hover(|style| style.bg(rgb(theme().surface)))
                })
                .child(e2e::measure_control(
                    match slot {
                        0 => "tab-oplog-label",
                        1 => "tab-terminal-label",
                        2 => "tab-activity-label",
                        _ => "compact-tab-changes-label",
                    },
                    SharedString::from(tab.label()),
                ))
                .when(cfg!(feature = "gui-e2e"), |cell| {
                    cell.child(e2e::measure_inside(id))
                })
        };
        let tab_bar = tabs
            .list(div().id("bottom-panel-tab-bar"))
            .flex()
            .items_center()
            .w_full()
            .flex_shrink_0()
            .bg(rgb(theme().panel))
            .child(
                div()
                    .flex()
                    .mx_2()
                    .gap_1()
                    .rounded(theme::scaled_px(4.))
                    .when(compact, |el| el.child(cell(3, "compact-tab-changes")))
                    .child(cell(0, "tab-oplog"))
                    .child(cell(1, "tab-terminal"))
                    .child(cell(2, "tab-activity")),
            );

        // ── Body: Operation Log or Terminal ──
        let body = match active_tab {
            // ADR-0110 Phase 5: the op-log renders as its own child entity so a
            // push / row-expand re-renders only this subtree.
            BottomTab::OperationLog => self
                .op_log
                .clone()
                .map(|e| e.into_any_element())
                .unwrap_or_else(|| div().flex_1().min_h(px(0.)).into_any_element()),
            BottomTab::Terminal => self.render_terminal_body(cx),
            BottomTab::Activity => self.render_activity_body(cx),
            BottomTab::Changes => self
                .compact_surface(false, cx)
                .map(IntoElement::into_any_element)
                .unwrap_or_else(|| div().flex_1().min_h(px(0.)).into_any_element()),
        };

        // ── Panel container (height = fixed, flex_shrink_0) ──
        // `height` is the unscaled, persisted body height; the whole container
        // (body + divider + tab strip) is scaled at render so it tracks zoom.
        // The BottomPanel drag math converts the raw cursor back to this
        // unscaled space (see divider_drag_move).
        let panel_h = height + BOTTOM_PANEL_DIVIDER_H + BOTTOM_PANEL_TAB_H;
        let panel = div()
            .id("bottom-panel")
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .h(theme::scaled_px(panel_h))
            .flex_shrink_0()
            .child(h_divider)
            .child(tab_bar)
            .child(body)
            .when(cfg!(feature = "gui-e2e"), |panel| {
                panel.child(e2e::measure_inside("bottom-panel"))
            });
        // #950: only the clip's height moves. The panel hangs from its top
        // edge, so the tab strip rises from the bottom with it.
        Some(
            super::panel_motion::clip(
                "bottom-panel-clip",
                super::panel_motion::Axis::Vertical,
                super::panel_motion::Anchor::Start,
                theme::scaled_px(panel_h),
                visible,
                panel,
            )
            .when(cfg!(feature = "gui-e2e"), |clip| {
                clip.child(e2e::measure_inside("bottom-panel-clip"))
            }),
        )
    }

    /// Render the Activity tab body: a Day/Week/Month granularity toggle, a
    /// commit + merge line chart, and the top-5 contributor ranking. The data is
    /// pre-aggregated in `view().activity` (built in `build_tab_view`), so
    /// this method only lays it out and wires the toggle.
    fn render_activity_body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use kagi_domain::activity::Granularity;
        let activity = &self.view().activity;
        let gran = self.activity_granularity;
        let gdata = activity.get(gran);
        let buckets = gdata.buckets.clone();

        // Compact granularity toggle (Day / Week / Month / Year) — sized to match
        // the rest of the app's chrome rather than standing out.
        let mk_gran = |g: Granularity| {
            let active = g == gran;
            let click = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
                this.activity_granularity = g;
                this.activity_hover = None;
                cx.notify();
            });
            div()
                .id(SharedString::from(format!("activity-gran-{}", g.label())))
                .px(theme::scaled_px(6.))
                .py(theme::scaled_px(1.))
                .rounded(theme::scaled_px(4.))
                .text_xs()
                .text_color(rgb(if active {
                    theme().text_main
                } else {
                    theme().text_muted
                }))
                .bg(if active {
                    theme().selected_for_window(self.window_active)
                } else {
                    rgb(theme().panel).into()
                })
                .hover(|s| s.cursor_pointer().bg(rgb(theme().surface)))
                .on_click(click)
                .child(SharedString::from(g.label()))
        };

        // Legend entry: a short line swatch (matching the chart strokes) + text.
        let legend = |color: u32, text: String| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(div().w(px(14.)).h(px(3.)).rounded_full().bg(rgb(color)))
                .child(SharedString::from(text))
        };

        // Window totals + ranking are scoped to the selected granularity.
        let win_commits = gdata.total_commits;
        let win_merges = gdata.total_merges;
        let contribs = &gdata.contributors;

        // Instant hover read-out: when the pointer is over a chart bucket, the
        // header shows that slice's relative time + counts instead of the window
        // totals (no tooltip delay).
        let hovered = self.activity_hover.filter(|&i| i < buckets.len()).map(|i| {
            let b = &buckets[i];
            let center = gdata.start + (i as i64) * gdata.bucket_secs + gdata.bucket_secs / 2;
            (
                commit_list::relative_time(center, gdata.now),
                b.commits,
                b.merges,
            )
        });
        let (head_label, head_commits, head_merges) = match &hovered {
            Some((ago, c, m)) => (format!("at {ago}"), *c, *m),
            None => (gran.window_label().to_string(), win_commits, win_merges),
        };

        let mut gran_row = div().flex().flex_row().items_center().gap_1();
        for g in Granularity::ALL {
            gran_row = gran_row.child(mk_gran(g));
        }

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .flex_shrink_0()
            .child(gran_row)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child(SharedString::from(head_label))
                    .child(legend(
                        theme().color_success,
                        format!("{head_commits} commits"),
                    ))
                    .child(legend(theme().color_head, format!("{head_merges} merges"))),
            );

        let chart = if win_commits == 0 {
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!(
                    "No commits in the {}",
                    gran.window_label()
                )))
                .into_any_element()
        } else {
            let max_c = buckets.iter().map(|b| b.commits).max().unwrap_or(0);
            let first_label = gdata.start_label.clone();
            let last_label = "now".to_string();
            let tick = |t: String| {
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(t))
            };
            let axis_w = theme::scaled_px(26.);
            div()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .flex()
                        .flex_row()
                        .child(
                            div()
                                .w(axis_w)
                                .flex_shrink_0()
                                .flex()
                                .flex_col()
                                .justify_between()
                                .items_end()
                                .pr_1()
                                .py(theme::scaled_px(6.))
                                .child(tick(max_c.to_string()))
                                .child(tick("0".into())),
                        )
                        .child({
                            // Canvas + a transparent per-bucket hover overlay.
                            // Each column updates `activity_hover` instantly via
                            // on_hover (gpui's own tooltip has a fixed 500ms
                            // delay that felt too slow), so the header read-out
                            // appears with no perceptible lag.
                            let hover_bg = if theme().dark {
                                gpui::hsla(0., 0., 1., 0.07)
                            } else {
                                gpui::hsla(0., 0., 0., 0.06)
                            };
                            let mut overlay = div().absolute().inset_0().flex().flex_row();
                            for i in 0..buckets.len() {
                                let on_hover = cx.listener(move |this, hovered: &bool, _w, cx| {
                                    if *hovered {
                                        this.activity_hover = Some(i);
                                    } else if this.activity_hover == Some(i) {
                                        this.activity_hover = None;
                                    }
                                    cx.notify();
                                });
                                overlay = overlay.child(
                                    div()
                                        .id(SharedString::from(format!("activity-bucket-{i}")))
                                        .flex_1()
                                        .h_full()
                                        .hover(move |s| s.bg(hover_bg))
                                        .on_hover(on_hover),
                                );
                            }
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .relative()
                                .child(activity_view::activity_chart(buckets.clone()).size_full())
                                .child(overlay)
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .child(div().w(axis_w).flex_shrink_0())
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_row()
                                .justify_between()
                                .child(tick(first_label))
                                .child(tick(last_label)),
                        ),
                )
                .into_any_element()
        };

        let max_commits = contribs.first().map(|c| c.commits).unwrap_or(0);
        let mut rows_el = div()
            .id("activity-ranking-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .track_scroll(&self.ui().activity_ranking_scroll)
            .flex()
            .flex_col();
        if contribs.is_empty() {
            rows_el = rows_el.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("—")),
            );
        } else {
            for (i, c) in contribs.iter().take(50).enumerate() {
                rows_el = rows_el.child(activity_view::contributor_row(i + 1, c, max_commits));
            }
        }
        let ranking = div()
            .w(theme::scaled_px(300.))
            .flex_shrink_0()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(theme().accent_text_on(theme().panel)))
                    .flex_shrink_0()
                    .child(SharedString::from("Top contributors")),
            )
            .child(super::render_helpers::with_system_vertical_scrollbar(
                "activity-ranking-scrollbar",
                &self.ui().activity_ranking_scroll,
                rows_el,
                cx,
            ));

        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .gap(theme::scaled_px(4.))
            .p(theme::scaled_px(6.))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_row()
                    .gap(theme::scaled_px(8.))
                    .child(chart)
                    .child(ranking),
            )
            .into_any_element()
    }

    /// Render the Terminal tab body (T-BP-007).
    ///
    /// Three possible states:
    /// 1. Session running → render `TerminalView` entity directly (flex_1 + min_h).
    /// 2. Session failed to start → show the error message.
    /// 3. Not yet started (session is None, or view is None with no error) →
    ///    show a "starting…" placeholder.  The Terminal tab click listener has
    ///    already called `ensure_terminal`; the view will appear on next repaint.
    fn render_terminal_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let active_session = self.ui().terminal_session.as_ref();
        // Case 1: running terminal view.
        if let Some(session) = active_session {
            if let Some(ref view_entity) = session.view {
                // cmd-v paste: gpui-terminal 0.1.0 has no built-in clipboard
                // paste, so an ancestor key listener reads the gpui clipboard
                // and writes straight to the PTY. Key events bubble along the
                // focus path, so this fires while the terminal is focused.
                let paste_writer = session.paste_writer.clone();
                let tab_writer = paste_writer.clone();
                let shift_tab_writer = paste_writer.clone();
                let term_focus = view_entity.read(cx).focus_handle().clone();
                return div()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    // Mark this subtree as the "Terminal" key context so global
                    // arrow/escape KeyBindings (scoped `!Terminal`) don't consume
                    // those keys while the terminal is focused — they flow to the
                    // terminal's own on_key_down → PTY (history, vim, etc.).
                    .key_context("Terminal")
                    // Tab / Shift-Tab → PTY (shell completion; user report).
                    // gpui_component::Root binds "tab" to focus cycling in its
                    // "Root" context, and bindings run before key listeners —
                    // so the terminal's on_key_down never saw Tab. These
                    // actions are bound in this deeper "Terminal" context
                    // (mod.rs), which outranks Root's binding.
                    .on_action(cx.listener(move |_this, _: &TerminalSendTab, _w, _cx| {
                        if let Some(writer) = tab_writer.as_ref() {
                            writer.paste_text("\t");
                        }
                    }))
                    .on_action(
                        cx.listener(move |_this, _: &TerminalSendShiftTab, _w, _cx| {
                            if let Some(writer) = shift_tab_writer.as_ref() {
                                writer.paste_text("\x1b[Z");
                            }
                        }),
                    )
                    // Clicking anywhere in the terminal area refocuses the
                    // terminal (the view's own mouse handling is a no-op in
                    // gpui-terminal 0.1.0, so a stray click could leave the
                    // keyboard focus elsewhere and break typing/cmd-v).
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_this, _e: &gpui::MouseDownEvent, window, cx| {
                            window.focus(&term_focus, cx);
                        }),
                    )
                    .on_key_down(
                        cx.listener(move |_this, event: &KeyDownEvent, _window, cx| {
                            let ks = &event.keystroke;
                            if ks.modifiers.platform && ks.key == "v" {
                                if let Some(writer) = paste_writer.as_ref() {
                                    if let Some(text) =
                                        cx.read_from_clipboard().and_then(|item| item.text())
                                    {
                                        writer.paste_text(&text);
                                        eprintln!(
                                            "[kagi] terminal: paste {} chars",
                                            text.chars().count()
                                        );
                                    }
                                }
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .child(view_entity.clone())
                    .into_any();
            }

            // Case 2: start failed — show error.
            if let Some(ref err) = session.start_error {
                let msg = SharedString::from(format!("terminal error: {}", err));
                return div()
                    .flex_1()
                    .min_h(px(0.))
                    .bg(rgb(theme().panel))
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(rgb(theme().color_blocker))
                    .child(msg)
                    .into_any();
            }
        }

        // Case 3: placeholder (no session yet / shell exited, will restart).
        div()
            .flex_1()
            .min_h(px(0.))
            .bg(rgb(theme().panel))
            .px_3()
            .py_2()
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(
                "(terminal exited — re-opening will restart)",
            ))
            .into_any()
    }
}
