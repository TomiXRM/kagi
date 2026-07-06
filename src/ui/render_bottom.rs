//! Bottom-panel slot (Operation Log / Terminal / Activity tabs) split out of
//! `render.rs` (T-SPLIT-RENDER-001 / ADR-0116 Wave 3). Child module of
//! `crate::ui`, so it keeps direct access to `KagiApp`'s private state.
//! Behaviour is unchanged — a pure physical move.

use super::*;

impl KagiApp {
    /// Bottom panel slot — T-BP-002: open/close + height resize.
    ///
    /// Returns `None` when the panel is closed (so `div().children(…)` adds no
    /// child element).  When open, returns the panel div with:
    /// - a 4px horizontal divider at the top (drag to resize)
    /// - a tab bar (OperationLog / Terminal)
    /// - a placeholder body area
    pub(super) fn render_bottom_panel_slot(
        &mut self,
        open: bool,
        height: f32,
        active_tab: BottomTab,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if !open {
            return None;
        }

        // ── Horizontal resize divider at top of panel ──
        let h_divider = div()
            .id("divider-bottom-panel")
            .w_full()
            .h(theme::scaled_px(BOTTOM_PANEL_DIVIDER_H))
            .flex_shrink_0()
            .bg(rgb(theme().surface))
            .hover(|style| style.bg(rgb(theme().color_branch)).cursor_row_resize())
            .cursor_row_resize()
            .on_drag(
                DividerDrag {
                    kind: DividerKind::BottomPanel,
                },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        // ── Tab bar ──
        let tab_bar = {
            let tab_operationlog_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                this.bottom_tab = BottomTab::OperationLog;
                cx.notify();
            });
            let tab_terminal_click = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                this.bottom_tab = BottomTab::Terminal;
                // T-BP-007: lazy-start the terminal on first show.
                this.ensure_terminal(window, cx);
                cx.notify();
            });
            let tab_activity_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                this.bottom_tab = BottomTab::Activity;
                cx.notify();
            });

            let make_tab = |label: &'static str, is_active: bool| {
                let text_color = if is_active {
                    theme().text_main
                } else {
                    theme().text_muted
                };
                let bg_color = if is_active {
                    theme().selected
                } else {
                    theme().panel
                };
                div()
                    .px_3()
                    .h(theme::scaled_px(BOTTOM_PANEL_TAB_H))
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .bg(rgb(bg_color))
                    .text_sm()
                    .text_color(rgb(text_color))
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .child(SharedString::from(label))
            };

            div()
                .id("bottom-panel-tab-bar")
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .flex_shrink_0()
                .bg(rgb(theme().panel))
                .child(
                    div()
                        .id("tab-oplog")
                        .flex()
                        .flex_shrink_0()
                        .on_click(tab_operationlog_click)
                        .hover(|s| s.cursor_pointer())
                        .child(make_tab(
                            BottomTab::OperationLog.label(),
                            active_tab == BottomTab::OperationLog,
                        )),
                )
                .child(
                    div()
                        .id("tab-terminal")
                        .flex()
                        .flex_shrink_0()
                        .on_click(tab_terminal_click)
                        .hover(|s| s.cursor_pointer())
                        .child(make_tab(
                            BottomTab::Terminal.label(),
                            active_tab == BottomTab::Terminal,
                        )),
                )
                .child(
                    div()
                        .id("tab-activity")
                        .flex()
                        .flex_shrink_0()
                        .on_click(tab_activity_click)
                        .hover(|s| s.cursor_pointer())
                        .child(make_tab(
                            BottomTab::Activity.label(),
                            active_tab == BottomTab::Activity,
                        )),
                )
        };

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
        };

        // ── Panel container (height = fixed, flex_shrink_0) ──
        // `height` is the unscaled, persisted body height; the whole container
        // (body + divider + tab strip) is scaled at render so it tracks zoom.
        // The BottomPanel drag math converts the raw cursor back to this
        // unscaled space (see divider_drag_move).
        let panel_h = height + BOTTOM_PANEL_DIVIDER_H + BOTTOM_PANEL_TAB_H;
        Some(
            div()
                .id("bottom-panel")
                .flex()
                .flex_col()
                .w_full()
                .h(theme::scaled_px(panel_h))
                .flex_shrink_0()
                .child(h_divider)
                .child(tab_bar)
                .child(body),
        )
    }

    /// Render the Activity tab body: a Day/Week/Month granularity toggle, a
    /// commit + merge line chart, and the top-5 contributor ranking. The data is
    /// pre-aggregated in `active_view.activity` (built in `build_tab_view`), so
    /// this method only lays it out and wires the toggle.
    fn render_activity_body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use kagi_domain::activity::Granularity;
        let activity = &self.active_view.activity;
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
                .bg(rgb(if active {
                    theme().selected
                } else {
                    theme().panel
                }))
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
                    .text_color(rgb(theme().color_branch))
                    .flex_shrink_0()
                    .child(SharedString::from("Top contributors")),
            )
            .child(rows_el);

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
        // W4-TABS: look up the active repo's session in the HashMap.
        let active_session = self
            .repo_path
            .as_ref()
            .and_then(|rp| self.terminal_sessions.get(rp));
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
                        cx.listener(move |_this, _e: &gpui::MouseDownEvent, window, _cx| {
                            window.focus(&term_focus);
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
