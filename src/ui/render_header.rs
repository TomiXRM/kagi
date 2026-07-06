//! Header slot (toolbar bar) and menu-action registration, split out of
//! `render.rs` (T-SPLIT-RENDER-001 / ADR-0116 Wave 3). Child module of
//! `crate::ui`, so it keeps direct access to `KagiApp`'s private state.
//! Behaviour is unchanged — a pure physical move.

#![allow(clippy::too_many_arguments)]

use super::*;

// ── AppShell layout slots ────────────────────────────────────────────────────
// ADR-0007 / T-BP-001: KagiApp::render is decomposed into four vertical
// flex slots.  Each slot is a plain method so that later tickets
// (T-BP-002, T-HT-001, …) can extend their signatures without
// touching the caller site.
impl KagiApp {
    /// W5-MENU / ADR-0029: register an `on_action` handler for every menu
    /// command, **but only when that command is currently enabled**.  Leaving a
    /// handler unregistered is exactly how macOS greys the matching menu item
    /// out (gpui validates each item via `is_action_available`, which checks the
    /// dispatch tree).  All handlers funnel into `handle_menu_command`, so the
    /// behaviour stays in `commands.rs` (no menu-specific logic lives here).
    pub(super) fn register_menu_actions(&self, el: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        use commands as cmds;

        // Helper: conditionally attach one action handler bound to its registry
        // id.  `$ty` is the gpui Action type; `$id` is the registry id string.
        macro_rules! menu_act {
            ($el:expr, $ty:ty, $id:literal) => {{
                let enabled = cmds::is_enabled(self, $id);
                $el.when(enabled, |el| {
                    el.on_action(cx.listener(|this, _: &$ty, window, cx| {
                        this.handle_menu_command($id, window, cx);
                    }))
                })
            }};
        }

        let el = menu_act!(el, cmds::About, "app.about");
        // T-SETTINGS-001: open Settings (menu item + cmd-,).
        let el = menu_act!(el, cmds::OpenSettings, "app.settings");
        let el = menu_act!(el, cmds::Quit, "app.quit");
        let el = menu_act!(el, cmds::NewTab, "file.newTab");
        let el = menu_act!(el, cmds::CloseTab, "file.closeTab");
        let el = menu_act!(el, cmds::CloneRepository, "file.cloneRepository");
        let el = menu_act!(el, cmds::OpenRepository, "file.openRepository");
        let el = menu_act!(el, cmds::OpenInTerminal, "file.openInTerminal");
        let el = menu_act!(el, cmds::ConnectRemote, "file.connectRemote");
        let el = menu_act!(el, cmds::RefreshRepository, "file.refresh");
        let el = menu_act!(el, cmds::ZoomIn, "view.zoomIn");
        let el = menu_act!(el, cmds::ZoomOut, "view.zoomOut");
        let el = menu_act!(el, cmds::ZoomReset, "view.zoomReset");
        let el = menu_act!(el, cmds::EnterFullScreen, "view.fullScreen");
        let el = menu_act!(el, cmds::ToggleSidebar, "view.toggleSidebar");
        let el = menu_act!(el, cmds::ToggleCommitDetails, "view.toggleCommitDetails");
        let el = menu_act!(el, cmds::ToggleDiffView, "view.toggleDiffView");
        let el = menu_act!(
            el,
            cmds::ToggleEditorWorkspace,
            "view.toggleEditorWorkspace"
        );
        let el = menu_act!(el, cmds::Fetch, "repo.fetch");
        let el = menu_act!(el, cmds::Pull, "repo.pull");
        let el = menu_act!(el, cmds::Push, "repo.push");
        let el = menu_act!(el, cmds::OpenInFinder, "repo.openInFinder");
        let el = menu_act!(el, cmds::NewBranch, "branch.new");
        let el = menu_act!(el, cmds::CheckoutBranch, "branch.checkout");
        let el = menu_act!(el, cmds::RenameBranch, "branch.rename");
        let el = menu_act!(el, cmds::DeleteBranch, "branch.delete");
        let el = menu_act!(el, cmds::CopyCommitHash, "commit.copyHash");
        let el = menu_act!(el, cmds::CheckoutCommit, "commit.checkout");
        let el = menu_act!(el, cmds::CreateBranchFromCommit, "commit.createBranch");
        let el = menu_act!(el, cmds::CherryPickCommit, "commit.cherryPick");
        let el = menu_act!(el, cmds::RevertCommit, "commit.revert");
        let el = menu_act!(el, cmds::ResetToCommit, "commit.reset");
        let el = menu_act!(
            el,
            cmds::CompareWithWorkingTree,
            "commit.compareWorkingTree"
        );
        let el = menu_act!(el, cmds::MinimizeWindow, "window.minimize");
        let el = menu_act!(el, cmds::ZoomWindow, "window.zoom");
        let el = menu_act!(el, cmds::NewWindow, "window.new");
        let el = menu_act!(el, cmds::CloseWindow, "window.close");
        let el = menu_act!(el, cmds::KeyboardShortcuts, "help.shortcuts");
        let el = menu_act!(el, cmds::Documentation, "help.documentation");
        let el = menu_act!(el, cmds::ReportIssue, "help.reportIssue");
        // W9-THEME: theme switch actions (always enabled).
        let el = menu_act!(el, cmds::ThemeCatppuccin, "theme.catppuccin");
        let el = menu_act!(el, cmds::ThemeXcodeDark, "theme.xcodeDark");
        let el = menu_act!(el, cmds::ThemeXcodeLight, "theme.xcodeLight");
        let el = menu_act!(el, cmds::ThemeOneDark, "theme.oneDark");
        let el = menu_act!(el, cmds::ThemeOneLight, "theme.oneLight");
        let el = menu_act!(el, cmds::ThemeMonokai, "theme.monokai");
        // W22-I18N: language switch actions (always enabled).
        let el = menu_act!(el, cmds::LangEnglish, "lang.english");
        let el = menu_act!(el, cmds::LangJapanese, "lang.japanese");
        el
    }

    /// Header slot — the Toolbar bar (T-HT-001 / ADR-0013).
    ///
    /// Layout (34 px):
    ///   LEFT:   repo-name | branch → upstream ↑A ↓B
    ///   CENTRE: Pull(↓N) Push(↑N) | Branch Stash Pop | Undo("<summary>") Terminal
    ///   RIGHT:  Refresh
    pub(super) fn render_header_slot(
        &mut self,
        toolbar: ToolbarState,
        summary: StatusBarSummary,
        // HEAD commit summary for Undo label (first row in commit list). ADR-0013.
        undo_summary: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // ── Click handlers ──────────────────────────────────────────────────
        // Pull — disabled when behind=0 or no upstream (ADR-0013).
        let pull_on = toolbar.pull_on;
        let pull_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if pull_on {
                this.open_pull_modal(cx);
            } else {
                let reason = if this.busy_op.is_some() {
                    Msg::PullBusy.t()
                } else if this.active_view.status_summary.is_detached {
                    Msg::PullDetached.t()
                } else if this.active_view.status_summary.is_unborn {
                    Msg::PullUnborn.t()
                } else if this.active_view.status_summary.no_upstream {
                    Msg::PullNoUpstream.t()
                } else {
                    Msg::PullNothing.t()
                };
                this.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
            cx.notify();
        });

        // Push (T-HT-004).
        let push_on = toolbar.push_on;
        let push_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if push_on {
                this.open_push_modal(cx);
            } else {
                let reason = if this.busy_op.is_some() {
                    Msg::PushBusy.t()
                } else if this.active_view.status_summary.is_detached {
                    Msg::PushDetached.t()
                } else if this.active_view.status_summary.is_unborn {
                    Msg::PushUnborn.t()
                } else if this.active_view.status_summary.no_upstream
                    && !this.active_view.status_summary.has_remote
                {
                    Msg::PushNoRemote.t()
                } else {
                    Msg::PushNothing.t()
                };
                this.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
            cx.notify();
        });

        // Ecosystem (ADR-0119) — read-only hot-spot analysis; open the
        // full-screen view. Disabled when no repo is open.
        let ecosystem_on = self.repo_path.is_some();
        let ecosystem_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            this.open_ecosystem_view(cx);
        });

        // Editor Workspace toggle (T-WS-EDITOR-004 / ADR-0120 §4) — placed
        // just left of Analyze. Routes through the exact same
        // `handle_menu_command` path as the View menu item / secondary-shift-e
        // shortcut, so behaviour (open/close + the `[kagi] menu:
        // workspace_mode=…` log line) stays byte-identical.
        let editor_ws_on = self.repo_path.is_some();
        let editor_ws_click = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
            this.handle_menu_command("view.toggleEditorWorkspace", window, cx);
            cx.notify();
        });

        // Branch — always enabled; use selected commit if any, else HEAD.
        let branch_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            // Resolve target commit: selected row → HEAD commit (first detail).
            let at = this
                .selected
                .and_then(|i| this.active_view.details.get(i))
                .map(|d| CommitId(d.full_sha.to_string()))
                .or_else(|| {
                    // Fall back to HEAD commit (first detail entry).
                    this.active_view
                        .details
                        .first()
                        .map(|d| CommitId(d.full_sha.to_string()))
                });
            if let Some(id) = at {
                this.open_create_branch_modal(id, cx);
            }
            cx.notify();
        });

        // Stash — enabled only when dirty.
        let stash_on = toolbar.stash_on;
        let stash_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if stash_on {
                this.open_stash_push_modal(cx);
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::StashClean.t()));
            }
            cx.notify();
        });

        // Pop — enabled only when stash exists.
        let pop_on = toolbar.pop_on;
        let pop_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if pop_on {
                // Pop the newest stash (index 0) — plan with conflict prediction.
                this.open_pop_modal(0);
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::PopEmpty.t()));
            }
            cx.notify();
        });

        // Undo — operation-history undo (T-UNDOREDO-001, ADR-0081). Enabled per
        // the in-session history cursor (can_undo). Click opens the undo plan
        // modal (preview → confirm runs the safe ref move).
        let undo_on = self.operation_history.can_undo();
        let undo_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if this.operation_history.can_undo() {
                this.open_history_undo_modal();
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::NothingToUndo.t()));
            }
            cx.notify();
        });

        // Redo — operation-history redo. Enabled per can_redo().
        let redo_on = self.operation_history.can_redo();
        let redo_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if this.operation_history.can_redo() {
                this.open_history_redo_modal();
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::NothingToRedo.t()));
            }
            cx.notify();
        });

        // Refresh — always enabled.
        let refresh_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            this.refresh_spin_started = Some(Instant::now());
            // Re-read local .git immediately (instant feedback) …
            // W3-NOTIFY: explicit refresh gets a completion toast (the watcher's
            // automatic reloads stay silent to avoid spam). A failed reload now
            // surfaces an error toast instead of a misleading "Refreshed".
            match this.reload_checked(cx) {
                Ok(()) => {
                    this.status_footer = FooterStatus::Idle(SharedString::from(Msg::Refreshed.t()));
                    this.push_toast(ToastKind::Success, Msg::Refreshed.t(), cx);
                }
                Err(e) => {
                    let msg = format!("Refresh failed: {e}");
                    this.status_footer = FooterStatus::Idle(SharedString::from(msg.clone()));
                    this.push_toast(ToastKind::Error, msg, cx);
                }
            }
            // … then also fetch the remote in the background so changes pushed
            // elsewhere (e.g. a GitHub merge) show up. Quiet: success reloads the
            // graph, failure (offline / no remote) is silent — no error spam.
            this.fetch_async(true, cx);
            cx.notify();
        });

        // Terminal — toggles bottom panel to Terminal tab (ADR-0013).
        let terminal_on = self.bottom_panel_open && self.bottom_tab == BottomTab::Terminal;
        let terminal_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            if this.bottom_panel_open && this.bottom_tab == BottomTab::Terminal {
                // Same tab visible → close panel (toggle off).
                this.bottom_panel_open = false;
            } else {
                this.bottom_panel_open = true;
                this.bottom_tab = BottomTab::Terminal;
                // T-BP-007: lazy-start terminal session when first opened.
                this.ensure_terminal(window, cx);
            }
            cx.notify();
        });

        // ── Helper: build a single Finder/Keynote-style toolbar button ──────
        // W10-TOOLBAR: icon on top (20px ≈ Size::Medium), text_xs label below,
        // vertically stacked. Whole button gets a hover bg + rounded; width is
        // content-fit with a shared min-width so the row reads as a grid.
        //
        // `id` must be a unique string for GPUI element tracking.
        // `count` (>0) renders a small chip overlay at the icon's top-right;
        // 0 hides it (ADR-0013: Pull ↓N / Push ↑N).
        // `enabled` drives muted colour; disabled buttons keep their click
        // handler (which sets the reason footer) but render in muted colour.
        let make_btn = |id: &'static str,
                        label: &'static str,
                        icon: gpui_component::IconName,
                        enabled: bool,
                        count: usize| {
            let text_color = if enabled {
                theme().text_main
            } else {
                theme().text_muted
            };
            let chip_bg = theme().color_branch;
            let chip_fg = theme().bg_base;

            // Icon cell — `.relative()` so the count chip can be `.absolute()`
            // anchored to the icon's top-right corner (gpui has no negative
            // clip, so the chip is placed inside the icon bounds).
            let mut icon_cell = div()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .w(theme::scaled_px(22.0))
                .h(theme::scaled_px(22.0))
                .child(
                    gpui_component::Icon::new(icon)
                        .with_size(gpui_component::Size::Size(theme::scaled_px(20.0)))
                        .text_color(rgb(text_color)),
                );
            if count > 0 {
                let chip_text = if count > 99 {
                    "99+".to_string()
                } else {
                    count.to_string()
                };
                icon_cell = icon_cell.child(
                    div()
                        .absolute()
                        .top(theme::scaled_px(-2.0))
                        .right(theme::scaled_px(-2.0))
                        .min_w(theme::scaled_px(14.0))
                        .h(theme::scaled_px(14.0))
                        .px(theme::scaled_px(3.0))
                        .rounded_full()
                        .bg(rgb(chip_bg))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(chip_fg))
                        .text_size(px(9.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .line_height(theme::scaled_px(14.0))
                        .child(SharedString::from(chip_text)),
                );
            }

            div()
                .id(id)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(theme::scaled_px(1.0))
                .min_w(theme::scaled_px(52.0))
                .px_1()
                .py(theme::scaled_px(2.0))
                .rounded_md()
                .hover(|style| style.bg(rgb(theme().selected)))
                .cursor(if enabled {
                    gpui::CursorStyle::PointingHand
                } else {
                    gpui::CursorStyle::Arrow
                })
                .child(icon_cell)
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(text_color))
                        .child(SharedString::from(label)),
                )
        };

        // ── Undo / Redo tooltips: previewed operation summary (ADR-0081) ────
        // Labels stay the fixed "Undo"/"Redo"; the (possibly long) operation
        // summary is surfaced on hover. Sourced from the operation-history
        // cursor (peek_undo / peek_redo). `undo_summary` (legacy undo-commit
        // tooltip) is no longer used now that the button is generalised.
        let _ = &undo_summary;
        let undo_tooltip_text: Option<SharedString> = self
            .operation_history
            .peek_undo()
            .map(|e| SharedString::from(format!("Undo: {}", e.summary)));
        let redo_tooltip_text: Option<SharedString> = self
            .operation_history
            .peek_redo()
            .map(|e| SharedString::from(format!("Redo: {}", e.summary)));

        // ── Left label: branch info (ADR-0013) ─────────────────────────────
        // Format: `branch → upstream ↑A ↓B`  or state labels when detached/unborn.
        let branch_label = if summary.is_detached {
            "detached HEAD".to_string()
        } else if summary.is_unborn {
            "no commits yet".to_string()
        } else if summary.no_upstream {
            format!("{} (no upstream)", summary.branch)
        } else {
            let ahead = summary.ahead.unwrap_or(0);
            let behind = summary.behind.unwrap_or(0);
            if summary.upstream_name.is_empty() {
                format!("{} \u{2191}{} \u{2193}{}", summary.branch, ahead, behind)
            } else {
                format!(
                    "{} \u{2192} {} \u{2191}{} \u{2193}{}",
                    summary.branch, summary.upstream_name, ahead, behind
                )
            }
        };

        // ── Vertical separator ──────────────────────────────────────────────
        let sep = || {
            div()
                // 1px hairline kept literal (scaling a hairline blurs it);
                // only the visible height tracks zoom.
                .w(px(1.0))
                .h(theme::scaled_px(16.0))
                .bg(rgb(theme().text_muted))
                .mx_1()
                .flex_shrink_0()
        };

        // ── Toolbar bar (52 px — W10-TOOLBAR vertical buttons) ──────────────
        div()
            .id("toolbar-bar")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_3()
            .h(theme::scaled_px(52.0))
            .flex_shrink_0()
            .bg(rgb(theme().panel))
            .text_color(rgb(theme().text_sub))
            // ── LEFT column (flex_1, equal width to the RIGHT column so the
            // centre cluster is window-centred regardless of side widths).
            // 3-column layout: [LEFT flex_1][centre cluster][RIGHT flex_1]. ──
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    // ── LEFT: Refresh (user request: left of the repo title) ──
                    .child({
                        // Spin for one full turn after a click (user request).
                        const SPIN_MS: u64 = 700;
                        // Spin while any async op is in flight (merge plan/exec,
                        // pull, push, fetch, …) — the user wants the sync icon to
                        // keep turning during async work — and for one rotation
                        // after an explicit Refresh click.
                        if let Some(t) = self.refresh_spin_started {
                            if t.elapsed() >= Duration::from_millis(SPIN_MS) {
                                self.refresh_spin_started = None;
                            }
                        }
                        let spinning =
                            self.busy_op.is_some() || self.refresh_spin_started.is_some();
                        let icon = gpui::svg()
                            .path("icons/refresh-cw.svg")
                            .w(theme::scaled_px(16.0))
                            .h(theme::scaled_px(16.0))
                            .text_color(rgb(theme().text_main));
                        let icon: gpui::AnyElement = if spinning {
                            use gpui::AnimationExt as _;
                            icon.with_animation(
                                "tb-refresh-spin",
                                // Repeat so it spins continuously for the whole
                                // async op (not just one rotation).
                                gpui::Animation::new(Duration::from_millis(SPIN_MS)).repeat(),
                                |svg, delta| {
                                    svg.with_transformation(gpui::Transformation::rotate(
                                        gpui::radians(delta * std::f32::consts::TAU),
                                    ))
                                },
                            )
                            .into_any_element()
                        } else {
                            icon.into_any_element()
                        };
                        div()
                            .id("tb-refresh")
                            .flex_shrink_0()
                            .mr_2()
                            .p_1()
                            .rounded_md()
                            .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                            .on_click(refresh_click)
                            .child(icon)
                    })
                    // ── repo name (top) + current branch (smaller, below) ──
                    // Stacked vertically so a long branch label never competes
                    // horizontally with the repo name (which used to vanish) nor
                    // runs under the centre Pull/Push/Branch cluster. Each line
                    // shrinks + truncates within the left column (user request).
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .mr_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(theme().text_main))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .line_height(theme::scaled_px(16.0))
                                    .w_full()
                                    .overflow_hidden()
                                    .truncate()
                                    .child(SharedString::from(summary.repo_name.clone())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme().text_sub))
                                    .line_height(theme::scaled_px(13.0))
                                    .w_full()
                                    .overflow_hidden()
                                    .truncate()
                                    .child(SharedString::from(branch_label)),
                            ),
                    ),
            ) // ── end LEFT column ──
            // ── CENTRE: window-centred cluster (flex_shrink_0 group) ──
            // Pull Push | Branch Stash Pop | Undo Terminal
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    // Pull (↓N chip when behind>0)
                    .child(
                        make_btn(
                            "tb-pull",
                            "Pull",
                            gpui_component::IconName::ArrowDown,
                            toolbar.pull_on,
                            toolbar.behind,
                        )
                        .on_click(pull_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Push (↑N chip when ahead>0)
                    .child(
                        make_btn(
                            "tb-push",
                            "Push",
                            gpui_component::IconName::ArrowUp,
                            toolbar.push_on,
                            toolbar.ahead,
                        )
                        .on_click(push_click),
                    )
                    .child(sep())
                    // Branch
                    .child(
                        make_btn(
                            "tb-branch",
                            "Branch",
                            gpui_component::IconName::Plus,
                            true,
                            0,
                        )
                        .on_click(branch_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Stash
                    .child(
                        make_btn(
                            "tb-stash",
                            "Stash",
                            gpui_component::IconName::Inbox,
                            toolbar.stash_on,
                            0,
                        )
                        .on_click(stash_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Pop
                    .child(
                        make_btn(
                            "tb-pop",
                            "Pop",
                            gpui_component::IconName::FolderOpen,
                            toolbar.pop_on,
                            0,
                        )
                        .on_click(pop_click),
                    )
                    .child(sep())
                    // Undo — operation-history undo (T-UNDOREDO-001). Label fixed; the
                    // previewed operation summary is shown in the tooltip.
                    .child(
                        make_btn(
                            "tb-undo",
                            Msg::Undo.t(),
                            gpui_component::IconName::Undo2,
                            undo_on,
                            0,
                        )
                        .when_some(undo_tooltip_text, |btn, text| {
                            btn.tooltip(move |window, cx| {
                                Tooltip::new(text.clone()).build(window, cx)
                            })
                        })
                        .on_click(undo_click),
                    )
                    // Redo — operation-history redo (T-UNDOREDO-001).
                    .child(
                        make_btn(
                            "tb-redo",
                            Msg::Redo.t(),
                            gpui_component::IconName::Redo2,
                            redo_on,
                            0,
                        )
                        .when_some(redo_tooltip_text, |btn, text| {
                            btn.tooltip(move |window, cx| {
                                Tooltip::new(text.clone()).build(window, cx)
                            })
                        })
                        .on_click(redo_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Terminal (toggles bottom panel Terminal tab)
                    .child(
                        make_btn(
                            "tb-terminal",
                            "Terminal",
                            gpui_component::IconName::SquareTerminal,
                            terminal_on,
                            0,
                        )
                        .on_click(terminal_click),
                    ),
            ) // ── end CENTRE cluster ──
            // ── RIGHT column (flex_1, equal width to the LEFT column) ──
            // Settings — now a standard toolbar button (icon + "Settings"
            // label) matching Pull/Push (T-SETTINGS-001 / ADR-0080). Opens the
            // Settings overlay; also reachable via the kagi menu and cmd-,.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .flex_1()
                    // Auto-update (ADR-0082): "↑ Update vX.Y.Z" chip when a newer
                    // release is available; click opens the update modal.
                    .when_some(
                        self.update_available.as_ref().map(|(p, _)| p.tag.clone()),
                        |el, tag| {
                            let open = cx.listener(|this, _: &gpui::ClickEvent, _w, cx| {
                                this.update_modal_open = true;
                                cx.notify();
                            });
                            el.child(
                                div()
                                    .id("tb-update")
                                    .flex()
                                    .items_center()
                                    .px(theme::scaled_px(8.0))
                                    .py(theme::scaled_px(4.0))
                                    .mr(theme::scaled_px(8.0))
                                    .rounded_md()
                                    .bg(rgb(theme().color_branch))
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .hover(|s| s.bg(rgb(theme().color_remote)))
                                    .child(
                                        div()
                                            .text_color(rgb(theme().bg_base))
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child(SharedString::from(format!(
                                                "\u{2191} Update {}",
                                                tag
                                            ))),
                                    )
                                    .on_click(open),
                            )
                        },
                    )
                    // Editor Workspace toggle (T-WS-EDITOR-004) — just left of
                    // Analyze. No pencil/edit icon exists in gpui-component
                    // 0.5.1's `IconName`; `File` is the stand-in.
                    .child(
                        make_btn(
                            "tb-editor-ws",
                            "Editor",
                            gpui_component::IconName::File,
                            editor_ws_on,
                            0,
                        )
                        .on_click(editor_ws_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Analyze / Code Ecosystem (ADR-0119) — read-only hot-spot
                    // analysis; placed just left of Settings.
                    .child(
                        make_btn(
                            "tb-ecosystem",
                            "Analyze",
                            gpui_component::IconName::ChartPie,
                            ecosystem_on,
                            0,
                        )
                        .on_click(ecosystem_click),
                    )
                    .child(sep())
                    .child({
                        let settings_click =
                            cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.menu_overlay = Some(commands::MenuOverlay::Settings);
                                // Probe Ollama so the Smart Commit model picker is
                                // usable without first opening the commit panel.
                                this.refresh_smart_commit_detection(cx);
                                // Seed the Analyze-ignore editor from disk.
                                this.ensure_analyze_ignore_input(window, cx);
                                cx.notify();
                            });
                        make_btn(
                            "tb-settings",
                            "Settings",
                            gpui_component::IconName::Settings,
                            true,
                            0,
                        )
                        .on_click(settings_click)
                    }),
            )
    }
}
