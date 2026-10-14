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
use super::commit_panel_controls as controls;
use super::commit_panel_rows::*;
use super::*;

/// The co-author popover, or `None` when the picker is closed.
///
/// Anchored above the icon row (the footer sits at the bottom of the panel, so
/// a downward menu would open off-screen) and wrapped in `gpui::deferred` so it
/// paints after all of its ancestors. Without that the body box's own border is
/// drawn over the menu, which overflows that box (user report).
pub(super) fn render_coauthor_menu(
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
        .border_color(rgb(theme().elevated_border(theme().selected)))
        .bg(rgb(theme().elevated_surface(theme().panel)))
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

    pub(super) fn defer_stage_file(&self, fi: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, move |app, owner, _w, cx| {
            app.do_stage_file(owner, fi, cx)
        });
    }

    pub(super) fn defer_unstage_file(
        &self,
        fi: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.defer_owned(window, cx, move |app, owner, _w, cx| {
            app.do_unstage_file(owner, fi, cx)
        });
    }

    pub(super) fn defer_stage_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| app.do_stage_all(owner, cx));
    }

    pub(super) fn defer_unstage_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.do_unstage_all(owner, cx)
        });
    }

    pub(super) fn defer_select_file(
        &self,
        file_ref: CommitPanelFileRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (staged, index) = match file_ref {
            CommitPanelFileRef::Staged { index } => (true, index),
            CommitPanelFileRef::Unstaged { index } => (false, index),
        };
        let files = if staged {
            &self.state.staged
        } else {
            &self.state.unstaged
        };
        let Some(path) = files.get(index).map(|file| file.path.clone()) else {
            return;
        };
        let source_repo = self.repo_path.clone();
        self.defer_owned(window, cx, move |app, owner, _w, cx| {
            let index = app.ui().commit_panel.as_ref().and_then(|panel| {
                let panel = panel.read(cx);
                if panel.owner != owner || panel.repo_path != source_repo {
                    return None;
                }
                let files = if staged {
                    &panel.state.staged
                } else {
                    &panel.state.unstaged
                };
                files.iter().position(|file| file.path == path)
            });
            if let Some(index) = index {
                let file_ref = if staged {
                    CommitPanelFileRef::Staged { index }
                } else {
                    CommitPanelFileRef::Unstaged { index }
                };
                app.select_commit_panel_file(owner, file_ref, cx);
            }
        });
    }

    pub(super) fn defer_open_commit_plan_modal(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.open_commit_plan_modal(owner, cx)
        });
    }

    pub(super) fn defer_open_discard_all(&self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn defer_amend(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.commit_panel_amend(owner, cx)
        });
    }

    pub(super) fn defer_smart_suggest(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, window, cx| {
            app.smart_suggest(owner, window, cx);
        });
    }

    pub(super) fn defer_smart_generate(&self, window: &mut Window, cx: &mut Context<Self>) {
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
        self.prepare_render(window, cx);
        self.reconcile_file_rows(window, cx);
        self.reconcile_coauthor_menu(window, cx);
        let panel_width = self.panel_render_width;
        self.render_panel(panel_width, window, cx)
    }
}

impl CommitPanelView {
    pub(super) fn prepare_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        controls::reconcile_bulk_focus(self, window, cx);
    }

    /// Reconcile only from the surface that actually draws these lazy lists.
    pub(super) fn reconcile_file_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_tree_layout();
        self.unstaged_navigation.reconcile(
            &self.state,
            false,
            self.unstaged_scroll_handle.clone(),
            self.pointer_root_focus.as_ref(),
            window,
            cx,
        );
        self.staged_navigation.reconcile(
            &self.state,
            true,
            self.staged_scroll_handle.clone(),
            self.pointer_root_focus.as_ref(),
            window,
            cx,
        );
    }

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
        let unstaged_header = controls::unstaged_header(self, false, cx);
        let staged_header = controls::staged_header(self, false, cx);

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
            .child(self.render_panel_header())
            // File lists share the remaining height with the shrinking composer.
            .child(
                div()
                    .id("cp-files-container")
                    .flex_1()
                    // Headers and a full compact row in each list remain visible;
                    // shorter viewports yield list height before the composer.
                    .flex()
                    .flex_col()
                    .relative()
                    .child(super::e2e::measure_inside("cp-files"))
                    // Unstaged ヘッダ (固定)
                    .child(unstaged_header)
                    // Unstaged スクロールボックス — PERF: virtualized uniform_list.
                    .child(self.render_unstaged_list(cx))
                    // Staged ヘッダ (固定)
                    .child(staged_header)
                    // Staged スクロールボックス — PERF: virtualized uniform_list.
                    .child(self.render_staged_list(cx)),
            )
            // Commit footer: subject + body, icon actions, commit button.
            // #476 slices 2–3: shown on a worktree panel too — every control in
            // it writes into that worktree.
            .child(self.render_composer(false, window, cx))
    }

    pub(super) fn render_panel_header(&self) -> gpui::Div {
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
            .when_some(self.foreign.clone(), |el, (label, color_idx)| {
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
            })
    }
}
