//! Commit Panel rendering, split out of `render_helpers.rs` (T-SPLIT-HELPERS-001
//! / ADR-0116 Wave 3). These build the Commit Panel view tree.
//!
//! ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001: the Commit Panel is now an
//! `Entity<CommitPanelView>` (correction #6). The per-row builders are pure
//! `&CommitPanelView` reads; the 22 listeners are `|view: &mut CommitPanelView|`.
//! Every listener that touches `app.commit_panel` (stage/unstage, file select,
//! commit/amend, discard, smart-commit, the parent `file_menu` overlay) DEFERS to
//! the parent via `cx.spawn_in(window, …)` + `weak_app.update_in(acx, …)` so the
//! leased entity is never re-entered. Pure entity-internal mutations (tree↔flat,
//! the co-author picker) stay synchronous + a child `cx.notify()`.
//! Element tree / styles / [kagi] lines / i18n are byte-identical to the
//! pre-entity version.

#![allow(clippy::too_many_arguments)]

use super::commit_panel::CommitPanelView;
use super::render_helpers::*;
use super::*;
use gpui::Focusable as _;
use gpui_component::ActiveTheme as _;

#[path = "commit_panel_controls.rs"]
mod controls;
#[path = "commit_panel_rows.rs"]
mod rows;
use rows::*;

// ──────────────────────────────────────────────────────────────
// Commit Panel — virtualized per-row builders (PERF)
// ──────────────────────────────────────────────────────────────
//
// These free functions build a SINGLE file row, reading live data from the
// `CommitPanelView` entity (NOT a captured-by-value clone). They are invoked
// from the `uniform_list` processors below for only the visible `range`, so the
// commit panel costs O(visible rows) per frame instead of O(all files).

/// The co-author popover, or `None` when the picker is closed.
///
/// Anchored above the icon row (the footer sits at the bottom of the panel, so
/// a downward menu would open off-screen) and wrapped in `gpui::deferred` so it
/// paints after all of its ancestors. Without that the body box's own border is
/// drawn over the menu, which overflows that box (user report).
fn render_coauthor_menu(
    view: &CommitPanelView,
    window: &Window,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let candidates = view.coauthor_menu.as_deref()?;
    let viewport = window.viewport_size();
    let width = theme::scaled_px(260.).min((viewport.width - px(16.)).max(px(0.)));
    let height = theme::scaled_px(
        (candidates.len().max(1) as f32 * super::commit_panel_coauthor::ROW_HEIGHT).min(220.),
    )
    .min((viewport.height - px(20.)).max(px(0.)));
    let content = super::commit_panel_coauthor::render_content(view, height, window, cx)?;
    let menu = div()
        .w(width)
        .rounded_sm()
        .border_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().panel))
        .flex()
        .flex_col()
        .occlude()
        .on_mouse_down_out(cx.listener(
            |view: &mut CommitPanelView, _event: &gpui::MouseDownEvent, window, cx| {
                view.coauthor_menu = None;
                view.reconcile_coauthor_menu(window, cx);
                cx.notify();
            },
        ))
        .child(content);
    Some(
        gpui::deferred(
            div()
                .absolute()
                .bottom(theme::scaled_px(26.))
                .right_0()
                .w_0()
                .h_0()
                .child(
                    gpui::anchored()
                        .position_mode(gpui::AnchoredPositionMode::Local)
                        .position(gpui::point(px(0.), px(0.)))
                        .anchor(gpui::Anchor::BottomRight)
                        .snap_to_window_with_margin(px(8.))
                        .child(menu),
                ),
        )
        .into_any_element(),
    )
}

// ──────────────────────────────────────────────────────────────
// CommitPanelView — deferred Backend dispatch (re-entrancy invariant)
// ──────────────────────────────────────────────────────────────
//
// Every method here marshals to the parent `KagiApp` via `spawn_in`/`update_in`:
// the called `KagiApp` method reads/updates `app.commit_panel` (this very
// entity), so calling it synchronously from a leased listener would re-lease the
// entity and panic ("already borrowed"). By the time the spawned task runs the
// listener has returned and the lease is released. Mirrors `ConflictView`.

impl CommitPanelView {
    /// Marshal a panel action to the parent on the next tick — a listener leases
    /// this entity, so a synchronous `KagiApp` call would re-enter and panic —
    /// carrying the frozen `owner` so a tab switch before the task runs drops it
    /// instead of acting on the panel now on screen (ADR-0197 決定 5 / #722 P1-c).
    fn defer_owned(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut KagiApp, crate::app::SessionId, &mut Window, &mut Context<KagiApp>)
            + 'static,
    ) {
        let weak_app = self.app.clone();
        let owner = self.owner;
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, window, cx| action(app, owner, window, cx));
        })
        .detach();
    }

    fn defer_stage_file(&self, fi: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, move |app, owner, _w, cx| {
            app.do_stage_file(owner, fi, cx)
        });
    }

    fn defer_unstage_file(&self, fi: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, move |app, owner, _w, cx| {
            app.do_unstage_file(owner, fi, cx)
        });
    }

    fn defer_stage_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| app.do_stage_all(owner, cx));
    }

    fn defer_unstage_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.do_unstage_all(owner, cx)
        });
    }

    fn defer_select_file(
        &self,
        file_ref: CommitPanelFileRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.defer_owned(window, cx, move |app, owner, _w, cx| {
            app.select_commit_panel_file(owner, file_ref, cx)
        });
    }

    fn defer_open_commit_plan_modal(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.open_commit_plan_modal(owner, cx)
        });
    }

    fn defer_open_discard_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.open_discard_all_modal(owner, cx)
        });
    }

    pub(crate) fn defer_open_file_menu(
        &self,
        fi: usize,
        pos: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Freeze both identities while the row listener still leases this
        // panel; the deferred callback must never resolve them from active UI.
        let Some(path) = self.state.unstaged.get(fi).map(|file| file.path.clone()) else {
            return;
        };
        let menu = file_menu::FileMenu {
            owner: self.owner,
            path,
            anchor: pos,
        };
        let panel_repo = self.repo_path.clone();
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| {
                app.open_file_menu(menu, &panel_repo, cx)
            });
        })
        .detach();
    }

    fn defer_amend(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.commit_panel_amend(owner, cx)
        });
    }

    fn defer_smart_suggest(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, window, cx| {
            app.smart_suggest(owner, window, cx);
        });
    }

    fn defer_smart_generate(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, window, cx| {
            app.smart_generate(owner, window, cx)
        });
    }
}

// ──────────────────────────────────────────────────────────────
// Commit Panel renderer (T025) — now self-rendering on the entity
// ──────────────────────────────────────────────────────────────

impl Render for CommitPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.window_activation_subscription.is_none() {
            self.window_activation_subscription =
                Some(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
        }
        self.window_active = window.is_window_active();
        #[cfg(feature = "gui-e2e")]
        {
            self.window_active = self.window_active_override.unwrap_or(self.window_active);
        }
        // Correction #1/#2: input sync + draft autosave run on the entity's own
        // render path (with `&mut Window`), never as a parent per-frame read of
        // the child's input.
        self.sync_inputs(window, cx);
        self.reconcile_coauthor_menu(window, cx);
        controls::reconcile_bulk_focus(self, window, cx);
        self.refresh_tree_layout();
        let panel_width = self.panel_render_width;
        self.render_panel(panel_width, window, cx)
    }
}

impl CommitPanelView {
    /// Keep the accessible sibling positions in sync with the displayed tree.
    /// The virtual list can render many batches for one scroll; each batch
    /// only indexes the layout cached here.
    fn refresh_tree_layout(&mut self) {
        if !self.state.tree_view {
            return;
        }
        let key = (
            self.state.tree_revision,
            self.state.generated_expanded,
            self.state.agent_expanded,
        );
        if self.tree_layout_for == Some(key) {
            return;
        }
        self.unstaged_tree_layout = cp_tree_layout(&self.state, false);
        self.staged_tree_layout = cp_tree_layout(&self.state, true);
        self.tree_layout_for = Some(key);
    }

    /// Render the Commit Panel: unstaged/staged sections + diff viewer + message
    /// input + commit button. (Was `KagiApp::render_commit_panel`; retargeted to
    /// the entity — reads `self.state` + the entity's own inputs/scroll handles.
    /// `smart` is read off the parent `KagiApp` via the weak handle: it is safe
    /// because render runs after the parent's render returns, and the value is
    /// pushed in by the parent each frame via `set_smart_snapshot`.)
    pub(crate) fn render_panel(
        &self,
        panel_width: f32,
        window: &Window,
        cx: &mut Context<CommitPanelView>,
    ) -> impl IntoElement {
        let panel = &self.state;
        let title_input = self.title_input.clone();
        let body_input = self.body_input.clone();
        let smart_status = self.smart_status.clone();
        let unstaged_scroll_handle = self.unstaged_scroll_handle.clone();
        let staged_scroll_handle = self.staged_scroll_handle.clone();
        // #476 slice 3: every commit-panel write — stage, commit, amend,
        // discard — now resolves the PANEL's repository, so nothing is hidden
        // on a linked worktree's panel any more. `foreign` is left as what it
        // always was underneath: the header chip's label + lane colour.
        let foreign = self.foreign.clone();
        let tree_view = panel.tree_view;
        let unstaged_count = panel.unstaged.len();
        let staged_count = panel.staged.len();
        // T026: the subject line alone decides whether there is a message —
        // a body without a subject is not a commit.
        let input_msg_nonempty = title_input
            .as_ref()
            .map(|e| !e.read(cx).value().trim().is_empty())
            .unwrap_or(!panel.commit_msg.trim().is_empty());
        let can_commit = !panel.staged.is_empty() && input_msg_nonempty;
        let has_unstaged_warning = !panel.unstaged.is_empty() && staged_count > 0;
        // PERF: selected_file is read per visible row from the entity inside the
        // uniform_list processors, not captured here.

        // ── Helper: build file rows for a section ────────────────
        // Returns a Vec of (element, depth, name, is_conflicted) as IntoElement.
        // We render inline to avoid capture issues.

        // ── Unstaged section ─────────────────────────────────────
        // T027: ヘッダ行は箱の外に固定し、ファイル行のみをスクロールボックス内に入れる

        let unstaged_header = controls::unstaged_header(self, cx);

        // PERF: unstaged file rows are virtualized via `uniform_list` (built from
        // free row functions reading the entity), not a prebuilt div.
        let unstaged_row_count = if tree_view {
            panel.unstaged_tree.len()
        } else {
            panel.unstaged_normal_files.len()
        } + panel.generated_extra_rows(false)
            + panel.agent_extra_rows(false);

        // ── Staged section ───────────────────────────────────────
        // T027: ヘッダ行は箱の外に固定し、ファイル行のみをスクロールボックス内に入れる

        let staged_header = controls::staged_header(self, cx);

        // PERF: staged file rows are virtualized via `uniform_list` (built from
        // free row functions reading the entity), not a prebuilt div.
        let staged_row_count = if tree_view {
            panel.staged_tree.len()
        } else {
            panel.staged_normal_files.len()
        } + panel.generated_extra_rows(true)
            + panel.agent_extra_rows(true);

        let commit_btn = controls::commit_button(self, can_commit, cx);
        let icon_row = div()
            .relative()
            .child(controls::icon_row(self, cx))
            .children(render_coauthor_menu(self, window, cx));

        // The radius `Input` gives itself, read once so both boxes match.
        let input_radius = gpui_component::ActiveTheme::theme(&**cx).radius;

        let template_toggle = controls::template_toggle(self, cx);

        // ── Commit message: subject + body (ADR-0134) ─────────────────
        // Two inputs mirroring git's own shape, replacing the single field and
        // the six-field template mode. The body is seeded from the user's
        // `commit.template` on first open and holds the co-author trailers.
        //
        // The icon row lives INSIDE the body's box: the box is a plain div
        // wearing the Input's own border/background, with an unstyled `Input`
        // and the icons stacked in it. Drawn as a real row rather than absolutely
        // positioned so it can never overlap a long body.
        // AutoGrow's inner minimum prevents flex shrinking at high zoom. Keep
        // its cached visual-row measurement, without imposing that minimum.
        let body_rows = body_input
            .as_ref()
            .map_or(3, |body| body.read(cx).wrap_row_count().clamp(3, 12));
        let body_height = px(body_rows as f32 * theme::rem_size_px() * 1.25)
            + gpui_component::Size::Medium.input_py() * 2.;
        let msg_inputs: gpui::AnyElement = match (&title_input, &body_input) {
            (Some(title), Some(body)) => div()
                .flex()
                .flex_col()
                .min_h(px(0.))
                .flex_shrink(1.)
                .overflow_hidden()
                .gap_2()
                .child(Input::new(title).appearance(true).bordered(true))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        // As in Issues composer: Tab traverses controls, while
                        // Cmd+] / Cmd+[ retain the editor's explicit indentation.
                        .capture_action(cx.listener(
                            |_, _: &gpui_component::input::IndentInline, window, cx| {
                                window.focus_next(cx);
                                cx.stop_propagation();
                            },
                        ))
                        .capture_action(cx.listener(
                            |_, _: &gpui_component::input::OutdentInline, window, cx| {
                                window.focus_prev(cx);
                                cx.stop_propagation();
                            },
                        ))
                        // Icon, its p_1, row pb_1, and the body's two borders.
                        .min_h(theme::scaled_px(16.) + px(theme::rem_size_px() * 0.75 + 2.))
                        .flex_shrink(1.)
                        // Same radius the `Input` gives itself, so the two boxes
                        // do not have visibly different corners.
                        .rounded(input_radius)
                        .border_1()
                        .border_color(cx.theme().input)
                        .when(body.read(cx).focus_handle(cx).is_focused(window), |el| {
                            el.border_color(cx.theme().ring)
                        })
                        .bg(rgb(theme().bg_base))
                        .child(
                            div()
                                .h(body_height)
                                .min_h(px(0.))
                                .flex_shrink(1.)
                                .overflow_hidden()
                                .child(Input::new(body).h_full().appearance(false).bordered(false)),
                        )
                        .relative()
                        .child(super::e2e::measure_inside("cp-body"))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .flex_shrink_0()
                                .px_1()
                                .pb_1()
                                .relative()
                                .child(super::e2e::measure_inside("cp-footer-tools"))
                                .child(template_toggle)
                                .child(icon_row),
                        ),
                )
                .into_any_element(),
            // No window yet (headless); the message still flows through
            // `state.commit_msg`, so this is a placeholder, not a failure.
            _ => div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("(commit message input unavailable)"))
                .into_any_element(),
        };

        // ── Assemble panel ───────────────────────────────────────
        // T-UI-003: diff ボックス廃止。Unstaged/Staged 箱が flex_1 で全体を占める(1:1)。
        div()
            // `panel_width` is the unscaled, persisted right-panel width; scale at
            // render so it tracks zoom (the Panel divider drag uses the same space).
            .w(theme::scaled_px(panel_width))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(theme().panel))
            // Header. #473: a worktree panel adds the worktree's chip, in the
            // WIP row's own lane colour, so it is obvious *which* working tree
            // the file list below belongs to.
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .bg(rgb(theme().surface))
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .child(super::e2e::commit_panel_labels::caption(
                        "cp-title",
                        Msg::CommitPanelTitle.t(),
                    ))
                    .when_some(foreign.clone(), |el, (label, color_idx)| {
                        let color = theme().lane_color(color_idx);
                        el.child(
                            div()
                                .px_1()
                                .rounded_sm()
                                .flex_shrink_0()
                                .overflow_hidden()
                                .truncate()
                                .border_1()
                                .border_color(color)
                                .text_xs()
                                .text_color(color)
                                .child(SharedString::from(format!("🌲 {label}"))),
                        )
                    }),
            )
            // File lists share the remaining height with the shrinking composer.
            .child(
                div()
                    .id("cp-files-container")
                    .flex_1()
                    // Keep both section headers while sharing space with editing.
                    .min_h(theme::scaled_px(80.))
                    .flex()
                    .flex_col()
                    .relative()
                    .child(super::e2e::measure_inside("cp-files"))
                    // Unstaged ヘッダ (固定)
                    .child(unstaged_header)
                    // Unstaged スクロールボックス — PERF: virtualized uniform_list.
                    .child(
                        div()
                            .id("cp-unstaged-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .mx_1()
                            .mb_px()
                            .border_1()
                            .border_color(rgb(theme().surface))
                            .rounded_sm()
                            .flex()
                            .flex_col()
                            .child({
                                let handle = unstaged_scroll_handle.clone();
                                let list = with_vertical_scrollbar(
                                    "cp-unstaged-list-scroll",
                                    &handle,
                                    uniform_list(
                                        "cp-unstaged-list",
                                        unstaged_row_count,
                                        cx.processor(
                                            move |view,
                                                  range: std::ops::Range<usize>,
                                                  _window,
                                                  cx| {
                                                let layout = view.state.tree_view.then_some(
                                                    view.unstaged_tree_layout.as_slice(),
                                                );
                                                range
                                                    .filter_map(|i| match &layout {
                                                        Some(layout) => render_unstaged_tree_row(
                                                            view,
                                                            i,
                                                            cp_tree_slot(
                                                                layout,
                                                                "cp-unstaged-tree",
                                                                i,
                                                            ),
                                                            cx,
                                                        ),
                                                        None => {
                                                            render_unstaged_flat_row(view, i, cx)
                                                        }
                                                    })
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .track_scroll(&unstaged_scroll_handle)
                                    .flex_1()
                                    .min_h(px(0.)),
                                    false,
                                );
                                if tree_view {
                                    kagi_ui_core::tree_a11y::tree(
                                        "cp-unstaged-tree",
                                        list,
                                        Msg::A11yUnstagedTree.t(),
                                    )
                                } else {
                                    list
                                }
                            }),
                    )
                    // Staged ヘッダ (固定)
                    .child(staged_header)
                    // Staged スクロールボックス — PERF: virtualized uniform_list.
                    .child(
                        div()
                            .id("cp-staged-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .mx_1()
                            .mb_px()
                            .border_1()
                            .border_color(rgb(theme().surface))
                            .rounded_sm()
                            .flex()
                            .flex_col()
                            .child({
                                let handle = staged_scroll_handle.clone();
                                let list = with_vertical_scrollbar(
                                    "cp-staged-list-scroll",
                                    &handle,
                                    uniform_list(
                                        "cp-staged-list",
                                        staged_row_count,
                                        cx.processor(
                                            move |view,
                                                  range: std::ops::Range<usize>,
                                                  _window,
                                                  cx| {
                                                let layout = view
                                                    .state
                                                    .tree_view
                                                    .then_some(view.staged_tree_layout.as_slice());
                                                range
                                                    .filter_map(|i| match &layout {
                                                        Some(layout) => render_staged_tree_row(
                                                            view,
                                                            i,
                                                            cp_tree_slot(
                                                                layout,
                                                                "cp-staged-tree",
                                                                i,
                                                            ),
                                                            cx,
                                                        ),
                                                        None => render_staged_flat_row(view, i, cx),
                                                    })
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .track_scroll(&staged_scroll_handle)
                                    .flex_1()
                                    .min_h(px(0.)),
                                    false,
                                );
                                if tree_view {
                                    kagi_ui_core::tree_a11y::tree(
                                        "cp-staged-tree",
                                        list,
                                        Msg::A11yStagedTree.t(),
                                    )
                                } else {
                                    list
                                }
                            }),
                    ),
            )
            // Commit footer: subject + body, icon actions, commit button.
            // #476 slices 2–3: shown on a worktree panel too — every control in
            // it writes into that worktree.
            .child(
                div()
                    .flex_shrink(1.)
                    .min_h(px(0.))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .bg(rgb(theme().surface))
                    // Subject + body (the icon actions live inside the body box)
                    .child(msg_inputs)
                    .relative()
                    .child(super::e2e::measure_inside("cp-footer"))
                    // Transient smart-commit status. Its own full-width line:
                    // inside the right-aligned icon group it pushed the icons
                    // sideways as the text appeared (user report).
                    .when_some(smart_status, |el, status| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().text_muted))
                                .child(SharedString::from(status)),
                        )
                    })
                    // Unstaged warning
                    .when(has_unstaged_warning, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().color_warning))
                                .child(SharedString::from(i18n::unstaged_not_included(
                                    unstaged_count,
                                ))),
                        )
                    })
                    .child(commit_btn),
            )
    }
}
