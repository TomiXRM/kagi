//! Header slot (toolbar bar) and menu-action registration, split out of
//! `render.rs` (T-SPLIT-RENDER-001 / ADR-0116 Wave 3). Child module of
//! `crate::ui`, so it keeps direct access to `KagiApp`'s private state.
//! Behaviour is unchanged — a pure physical move.

#![allow(clippy::too_many_arguments)]

pub(super) mod count_badge;
pub(super) mod overflow_trigger;
#[cfg_attr(not(feature = "gui-e2e"), allow(dead_code))]
pub(super) mod toolbar_a11y;
pub(super) mod toolbar_overflow;

use super::*;
use crate::ui::workspace_mode::WorkspaceMode;
use kagi_ui_core::tooltip::HoverTooltip as _;

// Keep the trigger configurable until its click handler is installed, then
// wrap the complete element so tooltip geometry follows the whole control.
struct ToolbarButton {
    id: &'static str,
    element: gpui::Stateful<gpui::Div>,
    help: Option<SharedString>,
}

impl ToolbarButton {
    fn tooltip_summary(mut self, summary: Option<SharedString>) -> Self {
        if let Some(summary) = summary {
            self.help = Some(summary);
        }
        self
    }

    fn on_click(
        self,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> gpui::AnyElement {
        let element = self.element.on_click(on_click);
        if let Some(help) = self.help {
            element
                .hover_tooltip_builder((self.id, 0usize), move |window, cx| {
                    Tooltip::new(help.clone()).build(window, cx)
                })
                .into_any_element()
        } else {
            element.into_any_element()
        }
    }
}

/// Shared workspace/Home frame height; both presentation modes fit ≤54px at 100%.
pub(super) fn chrome_frame_height() -> gpui::Pixels {
    theme::scaled_px(
        if settings::toolbar_display(&theme().slug) == settings::ToolbarDisplay::IconOnly {
            52.
        } else {
            60.
        },
    )
}

// ── AppShell layout slots ────────────────────────────────────────────────────
// ADR-0007 / T-BP-001: KagiApp::render is decomposed into four vertical
// flex slots.  Each slot is a plain method so that later tickets
// (T-BP-002, T-HT-001, …) can extend their signatures without
// touching the caller site.
impl KagiApp {
    /// Only blank spacers are draggable. macOS disables AppKit's whole-titlebar
    /// gesture and delegates movement/double-click from this exact hit target.
    pub(super) fn render_chrome_spacer(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .relative()
            .flex_1()
            .min_w(theme::scaled_px(8.))
            .h_full()
            .window_control_area(gpui::WindowControlArea::Drag)
            .child(super::e2e::measure_inside(id))
            .when(cfg!(target_os = "macos"), |el| {
                el.on_mouse_up_out(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, _| this.titlebar_should_move = false),
                )
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        this.titlebar_should_move = true;
                    }),
                )
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        this.titlebar_should_move = false;
                    }),
                )
                .on_mouse_move(
                    cx.listener(|this, event: &gpui::MouseMoveEvent, window, _| {
                        if this.titlebar_should_move
                            && event.pressed_button == Some(gpui::MouseButton::Left)
                        {
                            this.titlebar_should_move = false;
                            window.start_window_move();
                        }
                    }),
                )
                .on_click(|event, window, _| {
                    if event.click_count() == 2 {
                        window.titlebar_double_click();
                    }
                })
            })
    }

    /// Pointer, keyboard and overflow items share exactly one activation path.
    fn activate_toolbar(
        &mut self,
        index: usize,
        reason: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(reason) = reason {
            self.status_footer = FooterStatus::Idle(reason.into());
            cx.notify();
            return;
        }
        match index {
            0 => self.open_pull_modal(cx),
            1 => self.open_push_modal(cx),
            2 => {
                if let Some(id) = self.selected_or_head_commit() {
                    self.open_create_branch_modal(id, cx);
                }
            }
            3 => self.open_stash_push_modal(cx),
            4 => self.open_pop_modal(0, cx),
            5 => {
                if !self.reject_if_busy(cx) {
                    self.open_history_undo_modal();
                }
            }
            6 => {
                if !self.reject_if_busy(cx) {
                    self.open_history_redo_modal();
                }
            }
            7 => {
                if self.bottom_panel_open && self.bottom_tab == BottomTab::Terminal {
                    self.bottom_panel_open = false;
                } else {
                    self.bottom_panel_open = true;
                    self.bottom_tab = BottomTab::Terminal;
                    self.activate_terminal(window, cx);
                }
            }
            8 => self.handle_menu_command("view.showGraph", window, cx),
            9 => self.handle_menu_command("view.togglePrMode", window, cx),
            10 => self.handle_menu_command("view.toggleEditorWorkspace", window, cx),
            11 => self.open_ecosystem_view(cx),
            12 => self.open_settings(window, cx),
            13 => self.open_update_modal(),
            _ => unreachable!("toolbar command index"),
        }
        cx.notify();
    }
    /// W5-MENU / ADR-0029: register an `on_action` handler for every menu
    /// command, **but only when that command is currently enabled**.  Leaving a
    /// handler unregistered is exactly how macOS greys the matching menu item
    /// out (gpui validates each item via `is_action_available`, which checks the
    /// dispatch tree).  All handlers funnel into `handle_menu_command`, so the
    /// behaviour stays in `commands.rs` (no menu-specific logic lives here).
    pub(super) fn register_menu_actions(&self, el: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        use commands as cmds;
        let el = cmds::register_recent_clear(el, cx);

        // Helper: conditionally attach one action handler bound to its registry
        // id.  `$ty` is the gpui Action type; `$id` is the registry id string.
        macro_rules! menu_act {
            ($el:expr, $ty:ty, $id:literal) => {{
                let enabled = cmds::is_enabled(self, $id, cx);
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
        let el = menu_act!(el, cmds::TogglePrSwimlane, "view.togglePrSwimlane");
        let el = menu_act!(el, cmds::ToggleCommitDetails, "view.toggleCommitDetails");
        let el = menu_act!(el, cmds::ToggleEditorDetails, "view.toggleEditorDetails");
        let el = menu_act!(el, cmds::ToggleDiffView, "view.toggleDiffView");
        let el = menu_act!(
            el,
            cmds::ToggleEditorWorkspace,
            "view.toggleEditorWorkspace"
        );
        let el = menu_act!(el, cmds::TogglePrMode, "view.togglePrMode");
        let el = menu_act!(el, cmds::ShowGraph, "view.showGraph");
        // Issue #352: command palette (Cmd/Ctrl+P).
        let el = menu_act!(el, cmds::OpenCommandPalette, "view.commandPalette");
        let el = menu_act!(el, cmds::Fetch, "repo.fetch");
        let el = menu_act!(el, cmds::Pull, "repo.pull");
        let el = menu_act!(el, cmds::Push, "repo.push");
        let el = menu_act!(el, cmds::CreateSnapshot, "repo.createSnapshot");
        let el = menu_act!(el, cmds::ConflictAbort, "repo.conflictAbort");
        let el = menu_act!(el, cmds::ConflictSkip, "repo.conflictSkip");
        let el = menu_act!(el, cmds::ConflictContinue, "repo.conflictContinue");
        let el = menu_act!(el, cmds::OpenInFinder, "repo.openInFinder");
        let el = menu_act!(el, cmds::NewBranch, "branch.new");
        let el = menu_act!(el, cmds::CheckoutBranch, "branch.checkout");
        let el = menu_act!(el, cmds::RenameBranch, "branch.rename");
        let el = menu_act!(el, cmds::DeleteBranch, "branch.delete");
        let el = menu_act!(el, cmds::CopySelectedBranchName, "branch.copySelectedName");
        let el = el.when(
            cmds::is_enabled(self, "branch.copySelectedName", cx),
            |el| {
                el.on_action(
                    cx.listener(|this, _: &cmds::CopyGraphBranchName, window, cx| {
                        if this.commit_list_has_focus(window, cx) {
                            this.handle_menu_command("branch.copySelectedName", window, cx);
                        } else {
                            cx.propagate();
                        }
                    }),
                )
            },
        );
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
        let el = menu_act!(el, cmds::PreviousTab, "window.previousTab");
        let el = menu_act!(el, cmds::NextTab, "window.nextTab");
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
        let el = menu_act!(el, cmds::ThemeColorVision, "theme.colorVision");
        // #922: the View → Theme menu items name a theme by slug (built-in or
        // custom), always available like the fixed actions above.
        let el = el.on_action(cx.listener(|this, action: &cmds::SetTheme, _window, cx| {
            this.set_theme(&action.slug, cx);
        }));
        let el = el.on_action(cx.listener(|_, action: &cmds::SetToolbarDisplay, _, cx| {
            settings::set_toolbar_display(action.mode);
            cmds::publish_menus(cx);
            cx.notify();
        }));
        // W22-I18N: language switch actions (always enabled).
        let el = menu_act!(el, cmds::LangEnglish, "lang.english");
        let el = menu_act!(el, cmds::LangJapanese, "lang.japanese");
        el
    }

    /// Second-row toolbar: repository identity, actions and workspace group.
    /// Document tabs and native traffic lights belong to the titlebar above.
    pub(super) fn render_header_slot(
        &mut self,
        toolbar: ToolbarState,
        summary: StatusBarSummary,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // ── Click handlers ──────────────────────────────────────────────────
        // Pull — the same reason feeds the button's AX description and its
        // pointer/keyboard click; resolve it only when the action is unavailable.
        let pull_reason = (!toolbar.pull_on).then(|| {
            if self.op_latched() {
                Msg::PullBusy.t()
            } else if self.view().status_summary.is_detached {
                Msg::PullDetached.t()
            } else if self.view().status_summary.is_unborn {
                Msg::PullUnborn.t()
            } else if self.view().status_summary.no_upstream {
                Msg::PullNoUpstream.t()
            } else {
                Msg::PullNothing.t()
            }
        });
        let pull_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(0, pull_reason, window, cx);
        });

        // Push (T-HT-004).
        let push_reason = (!toolbar.push_on).then(|| {
            if self.op_latched() {
                Msg::PushBusy.t()
            } else if self.view().status_summary.is_detached {
                Msg::PushDetached.t()
            } else if self.view().status_summary.is_unborn {
                Msg::PushUnborn.t()
            } else if self.view().status_summary.no_upstream
                && !self.view().status_summary.has_remote
            {
                Msg::PushNoRemote.t()
            } else {
                Msg::PushNothing.t()
            }
        });
        let push_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(1, push_reason, window, cx);
        });

        // Ecosystem (ADR-0119) — read-only hot-spot analysis; open the
        // full-screen view. Disabled when no repo is open.
        let ecosystem_on = self.repo_path.is_some();
        let no_repo_reason = (!ecosystem_on).then(|| Msg::NoRepoOpen.t());
        let ecosystem_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(11, no_repo_reason, window, cx);
        });

        // Editor Workspace toggle (T-WS-EDITOR-004 / ADR-0120 §4) — placed
        // just left of Analyze. Available clicks keep the View menu's exact
        // command path and `[kagi] menu:` log line.
        let editor_ws_on = self.repo_path.is_some();
        let editor_ws_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(10, no_repo_reason, window, cx);
        });
        // ── Workspace mode switcher: Graph | PRs | Editor ──────────────
        // Three mutually exclusive modes, one button each. Every button names
        // the mode it selects and lights up while that mode is on screen.
        // (They used to be two toggles that each morphed into a "Graph"
        // button; with two takeovers open at once both read "Graph" and
        // neither said which one you'd land in — user report.)
        let pr_mode_on = self.repo_path.is_some() && kagi_git::github::gh_available();
        let mode = self.workspace_mode();
        if self.conflict_body_visible() {
            if let Some(view) = self.ui().conflict.clone() {
                if view.read(cx).operation_chrome_subscription.is_none() {
                    let mut previous = view.read(cx).operation_chrome_state();
                    let subscription = cx.observe(&view, move |app, view, cx| {
                        let current = view.read(cx).operation_chrome_state();
                        if current != previous {
                            previous = current;
                            if app.ui().conflict.as_ref() == Some(&view) {
                                cx.notify();
                            }
                        }
                    });
                    view.update(cx, |view, _| {
                        view.operation_chrome_subscription = Some(subscription);
                    });
                }
            }
        }
        let graph_click = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(8, None, window, cx);
        });
        let pr_mode_click = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(9, None, window, cx);
        });

        // Branch — always enabled; use selected commit if any, else HEAD.
        let branch_click = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(2, None, window, cx);
        });

        // Stash — enabled only when dirty, and not while another operation
        // runs (`render` turns it off then): say which (#975 review).
        let stash_reason = (!toolbar.stash_on).then(|| {
            if self.op_latched() {
                Msg::StashBusy.t()
            } else {
                Msg::StashClean.t()
            }
        });
        let stash_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(3, stash_reason, window, cx);
        });

        // Pop — enabled only when a stash exists, and not while another
        // operation runs.
        let pop_reason = (!toolbar.pop_on).then(|| {
            if self.op_latched() {
                Msg::PopBusy.t()
            } else {
                Msg::PopEmpty.t()
            }
        });
        let pop_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(4, pop_reason, window, cx);
        });

        // Undo — operation-history undo (T-UNDOREDO-001, ADR-0081). Enabled per
        // the in-session history cursor (can_undo). Click opens the undo plan
        // modal (preview → confirm runs the safe ref move).
        let undo_on = self.ui().operation_history.can_undo();
        let undo_reason = (!undo_on).then(|| Msg::NothingToUndo.t());
        let undo_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(5, undo_reason, window, cx);
        });

        // Redo — operation-history redo. Enabled per can_redo().
        let redo_on = self.ui().operation_history.can_redo();
        let redo_reason = (!redo_on).then(|| Msg::NothingToRedo.t());
        let redo_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(6, redo_reason, window, cx);
        });

        // Refresh — always enabled.
        let refresh_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            this.refresh_spin_started = Some(Instant::now());
            // Re-read local .git (background, #487) …
            // W3-NOTIFY: explicit refresh gets a completion toast (the watcher's
            // automatic reloads stay silent to avoid spam); a failed reload
            // surfaces an error toast instead of a misleading "Refreshed". Both
            // are emitted by `reload_manual` when its read lands.
            this.reload_manual(cx);
            // … then also fetch the remote in the background so changes pushed
            // elsewhere (e.g. a GitHub merge) show up. Quiet: success reloads the
            // graph, failure (offline / no remote) is silent — no error spam.
            this.fetch_async(true, cx);
            cx.notify();
        });

        // Terminal — toggles bottom panel to Terminal tab (ADR-0013).
        let terminal_on = self.bottom_panel_open && self.bottom_tab == BottomTab::Terminal;
        let terminal_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.activate_toolbar(7, None, window, cx);
        });

        use toolbar_a11y::{button_flags, ButtonState};
        // No allocation or contrast work when no count chip is drawn; share
        // one foreground calculation if both Pull and Push have counts.
        let chip_fg = std::cell::LazyCell::new(|| theme::primary_button_foreground(&theme()));
        let icon_only =
            settings::toolbar_display(&theme().slug) == settings::ToolbarDisplay::IconOnly;
        let undo_has_summary = self.ui().operation_history.peek_undo().is_some();
        let redo_has_summary = self.ui().operation_history.peek_redo().is_some();
        let present = (if mode == WorkspaceMode::Graph {
            0xff
        } else {
            0
        }) | 0x1d00
            | if pr_mode_on { 1 << 9 } else { 0 }
            | if self.update_available.is_some() {
                1 << 13
            } else {
                0
            };
        let selected = match mode {
            WorkspaceMode::Graph => 8,
            WorkspaceMode::Prs => 9,
            WorkspaceMode::Editor => 10,
            _ => usize::MAX,
        };
        let style_key = (icon_only, i18n::lang(), theme::zoom().to_bits());
        let pending_widths = self.toolbar_layout.pending(present, style_key);
        let show_sidebar_toggle = mode == WorkspaceMode::Prs && !super::compact::for_window(window);
        let hidden = self
            .toolbar_layout
            .hidden(present, selected, show_sidebar_toggle, style_key);
        let sidebar_width = self
            .toolbar_layout
            .sidebar_width(show_sidebar_toggle, style_key);
        let identity_width = self
            .toolbar_layout
            .identity_width(show_sidebar_toggle, style_key);
        let visible = present & !hidden;
        let workspace_width = self
            .toolbar_layout
            .workspace_width(visible, hidden, style_key);
        let gap = |left, right| toolbar_overflow::gap(visible, left, right);
        let layout_app = cx.entity().downgrade();

        // ── Helper: build a single Finder/Keynote-style toolbar button ──────
        // W10-TOOLBAR: icon on top (20px ≈ Size::Medium), text_xs label below,
        // vertically stacked. Whole button gets a hover bg + rounded; width is
        // content-fit with a shared min-width so the row reads as a grid.
        //
        // `id` must be a unique string for GPUI element tracking.
        // `count` (>0) renders a small chip overlay at the icon's top-right;
        // 0 hides it (ADR-0013: Pull ↓N / Push ↑N).
        // `state` drives the muted colour, and for `Availability(false)` also
        // the AccessKit disabled flag; unavailable buttons still keep their
        // click handler, which sets the reason footer.
        // Takes a full `gpui_component::Icon` (not `IconName`) so buttons can
        // use custom embedded SVGs (`Icon::default().path("icons/…")`) for
        // glyphs gpui-component has no variant for (e.g. the Editor button's
        // square-pen). IconName callers wrap with `Icon::new(..)`.
        let make_btn = |id: &'static str,
                        label: &'static str,
                        icon: gpui_component::Icon,
                        state: ButtonState,
                        reason: Option<&'static str>,
                        count: usize| {
            let index = toolbar_overflow::IDS
                .iter()
                .position(|candidate| *candidate == id)
                .unwrap();
            let mode_selected = index == selected;
            // Both variants carry the same "renders bright" bool as the old
            // `enabled` argument, so the palette is unchanged.
            let (enabled, unavailable) = button_flags(state);
            let accessible_label = match id {
                "tb-branch" => Msg::ToolbarBranch.t(),
                "tb-stash" => Msg::ToolbarStash.t(),
                "tb-pop" => Msg::ToolbarPop.t(),
                "tb-editor-ws" => Msg::ToolbarEditor.t(),
                "tb-ecosystem" => Msg::ToolbarAnalyze.t(),
                "tb-terminal" => Msg::ToolbarTerminal.t(),
                "tb-graph-mode" => Msg::WorkspaceGraph.t(),
                "tb-pr-mode" => Msg::WorkspacePrs.t(),
                _ => label,
            };
            // Pending hidden commands need natural widths, not interactive
            // descendants; retaining the last fit keeps the open menu stable.
            if hidden & (1 << index) != 0 {
                let element = if pending_widths & (1 << index) != 0 {
                    toolbar_overflow::measure_hidden(
                        layout_app.clone(),
                        index,
                        accessible_label,
                        icon_only,
                        style_key,
                    )
                } else {
                    div().id(id).hidden()
                };
                return ToolbarButton {
                    id,
                    element,
                    help: None,
                };
            }
            #[cfg(feature = "gui-e2e")]
            toolbar_a11y::record_label(id, accessible_label);
            // #354: the exact bool that gates `set_disabled` below, recorded
            // for the GUI E2E oracle (the AccessKit tree itself is only built
            // once assistive technology connects, which a test cannot force).
            toolbar_a11y::record_unavailable(id, unavailable);
            let text_color = rgb(if mode_selected || enabled {
                theme().text_main
            } else {
                theme().text_muted
            });

            // The count is an overlay on the icon cell. Native probes stay
            // inside that cell so they do not change the toolbar's layout.
            let glyph = icon
                .with_size(gpui_component::Size::Size(theme::scaled_px(20.0)))
                .text_color(text_color);
            #[cfg(feature = "gui-e2e")]
            let glyph = super::e2e::measure_control(format!("{id}-glyph"), glyph);
            let mut icon_cell = div()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .w(theme::scaled_px(22.0))
                .h(theme::scaled_px(22.0))
                .child(glyph)
                .when(cfg!(feature = "gui-e2e"), |el| {
                    el.child(super::e2e::measure_inside(format!("{id}-icon")))
                });
            if count > 0 {
                icon_cell = icon_cell.child(count_badge::render(id, count, enabled, *chip_fg));
            }

            let btn = div()
                .id(id)
                // Bare string captions have no AX node;
                // share the visible label explicitly.
                .role(gpui::Role::Button)
                .aria_label(accessible_label)
                .when_some(reason, |el, reason| {
                    toolbar_a11y::record_description(id, reason);
                    el.aria_description(reason)
                })
                .when(unavailable, |el| {
                    // gpui has no aria_disabled setter. `parent_node()` is this
                    // element's own AccessKit node — it needs the id and role
                    // above, adds no children, and the hook slot is single-use.
                    el.a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
                        builder.parent_node().set_disabled();
                    })
                })
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(theme::scaled_px(1.0))
                .flex_shrink_0()
                .relative()
                .child(toolbar_overflow::probe(
                    layout_app.clone(),
                    Some(index),
                    style_key,
                ))
                .when(cfg!(feature = "gui-e2e"), |el| {
                    el.child(super::e2e::measure_inside(id))
                })
                .min_w(theme::scaled_px(toolbar_overflow::minimum_width(icon_only)))
                .px(keyboard_nav::inset(4.0))
                // At 0.7× the old 2px zoomed padding is smaller than the
                // fixed 2px ring. The centred row keeps its icon/text in
                // place while the extra fraction belongs to the ring.
                .py(px((f32::from(theme::scaled_px(2.0)) - 2.0).max(0.0)))
                .rounded_md()
                .when(mode_selected, |el| {
                    el.bg(theme().segment_selected_bg()).text_color(text_color)
                })
                .when(!mode_selected, |el| {
                    el.hover(|style| style.bg(rgb(theme().surface)))
                })
                .cursor(gpui::CursorStyle::Arrow)
                .child(icon_cell)
                .when(!icon_only, |el| {
                    #[cfg(feature = "gui-e2e")]
                    toolbar_a11y::record_caption(id, accessible_label);
                    el.child(
                        div()
                            .text_xs()
                            .text_color(text_color)
                            .child(SharedString::from(accessible_label)),
                    )
                });
            let help =
                if !(id == "tb-undo" && undo_has_summary || id == "tb-redo" && redo_has_summary) {
                    #[cfg(feature = "gui-e2e")]
                    toolbar_a11y::record_tooltip(id, accessible_label);
                    Some(SharedString::from(accessible_label))
                } else {
                    None
                };
            #[cfg(feature = "gui-e2e")]
            let btn = btn.on_key_down(move |event, _, _| {
                if event.keystroke.key == "f19" {
                    toolbar_a11y::record_focus_probe(id);
                }
            });
            ToolbarButton {
                id,
                element: keyboard_nav::focusable(btn),
                help,
            }
        };

        // ── Undo / Redo tooltips: previewed operation summary (ADR-0081) ────
        // Labels stay the fixed "Undo"/"Redo"; the (possibly long) operation
        // summary is surfaced on hover. Sourced from the operation-history
        // cursor (peek_undo / peek_redo).
        let undo_tooltip_text: Option<SharedString> = self
            .ui()
            .operation_history
            .peek_undo()
            .map(|e| SharedString::from(format!("{}: {}", Msg::Undo.t(), e.summary)));
        let redo_tooltip_text: Option<SharedString> = self
            .ui()
            .operation_history
            .peek_redo()
            .map(|e| SharedString::from(format!("{}: {}", Msg::Redo.t(), e.summary)));

        // ── Left label: branch info (ADR-0013) ─────────────────────────────
        // Format: `branch → upstream ↑A ↓B`  or state labels when detached/unborn.
        let branch_label = if summary.is_detached {
            "detached HEAD".to_string()
        } else if summary.is_unborn {
            "no commits yet".to_string()
        } else if summary.no_upstream {
            format!("{} (no upstream)", summary.branch)
        } else {
            let counts = match (summary.ahead, summary.behind) {
                (Some(ahead), Some(behind)) => format!("\u{2191}{ahead} \u{2193}{behind}"),
                // #355: a skipped count is unknown, not zero.
                _ => "\u{2014}".to_string(),
            };
            if summary.upstream_name.is_empty() {
                format!("{} {counts}", summary.branch)
            } else {
                format!(
                    "{} \u{2192} {} {counts}",
                    summary.branch, summary.upstream_name
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
                .bg(rgb(theme().surface))
                .mx_1()
                .flex_shrink_0()
        };

        // Toolbar controls follow the separate titlebar strip.
        div()
            .id("toolbar-bar")
            .flex()
            .flex_row()
            .items_center()
            .relative()
            .overflow_hidden()
            .child(toolbar_overflow::probe(layout_app.clone(), None, style_key))
            .w_full()
            .px_3()
            .h(chrome_frame_height())
            .flex_shrink_0()
            .bg(rgb(theme().chrome_surface()))
            .when(theme().uses_neutral_dark_chrome(), |bar| {
                bar.border_b_1().border_color(rgb(theme().surface))
            })
            .text_color(rgb(theme().text_sub))
            .when(cfg!(feature = "gui-e2e"), |el| {
                el.child(super::e2e::measure_inside("toolbar-row"))
            })
            // Equal flex side columns center commands when space permits.
            // At narrow widths the left retains readable repository identity,
            // while the right may yield its secondary-workspace reservation.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .when(icon_only, |el| {
                        el.min_w(px(
                            self.toolbar_layout.icon_identity_width(style_key) + sidebar_width
                        ))
                    })
                    .when(!icon_only, |el| el.min_w(px(identity_width)))
                    .when(show_sidebar_toggle, |el| {
                        el.child(
                            div()
                                .relative()
                                .flex_shrink_0()
                                .pr_2()
                                .child(toolbar_overflow::probe(
                                    layout_app.clone(),
                                    Some(16),
                                    style_key,
                                ))
                                .child(super::pr_layout::navigator_toggle(self, window, cx)),
                        )
                    })
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
                        let spinning = self.op_latched() || self.refresh_spin_started.is_some();
                        let icon = gpui::svg()
                            .path("icons/refresh-cw.svg")
                            .w(theme::scaled_px(if icon_only { 20.0 } else { 16.0 }))
                            .h(theme::scaled_px(if icon_only { 20.0 } else { 16.0 }))
                            .text_color(rgb(theme().text_main));
                        let icon: gpui::AnyElement = if spinning && !theme::reduce_motion() {
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
                        let refresh = div()
                            .id("tb-refresh")
                            .role(gpui::Role::Button)
                            .when_some(commands::command("file.refresh"), |el, command| {
                                el.aria_label(command.label)
                            })
                            .relative()
                            .child(super::e2e::measure_inside("tb-refresh"))
                            .when(!icon_only, |el| {
                                el.child(toolbar_overflow::probe(
                                    layout_app.clone(),
                                    Some(14),
                                    style_key,
                                ))
                            })
                            .flex_shrink_0()
                            .mr_2()
                            .p(keyboard_nav::inset(4.0))
                            .rounded_md()
                            .hover(|st| st.bg(rgb(theme().surface)).cursor_default())
                            .child(icon);
                        #[cfg(feature = "gui-e2e")]
                        let refresh = refresh.on_key_down(|event, _, _| {
                            if event.keystroke.key == "f19" {
                                toolbar_a11y::record_focus_probe("tb-refresh");
                            }
                        });
                        let refresh = keyboard_nav::focusable(refresh).on_click(refresh_click);
                        if let Some(command) = commands::command("file.refresh") {
                            refresh
                                .hover_tooltip_builder(
                                    "tb-refresh-hover-help",
                                    move |window, cx| Tooltip::new(command.label).build(window, cx),
                                )
                                .into_any_element()
                        } else {
                            refresh.into_any_element()
                        }
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
                            .w(theme::scaled_px(180.))
                            .min_w_0()
                            .when(icon_only, |el| {
                                el.flex_shrink(1.).min_w(theme::scaled_px(40.))
                            })
                            .relative()
                            .when(!icon_only, |el| {
                                el.flex_shrink_0().child(toolbar_overflow::probe(
                                    layout_app.clone(),
                                    Some(15),
                                    style_key,
                                ))
                            })
                            .child(super::e2e::measure_inside("toolbar-identity"))
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
                    )
                    .child(self.render_chrome_spacer("toolbar-drag-region", cx)),
            ) // ── end LEFT column ──
            // ── CENTRE: window-centred cluster (flex_shrink_0 group) ──
            // Pull Push | Branch Stash Pop | Undo Terminal
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    // Pull … Terminal belong to Graph, so only Graph draws them;
                    // the other modes get this slot for their own actions, added
                    // when each mode needs them (user request).
                    .when(mode == WorkspaceMode::Graph, |el| {
                        el.child(
                            div()
                                .relative()
                                .flex()
                                .flex_row()
                                .items_center()
                                .child(super::e2e::measure_inside("tb-repo-actions"))
                                // Pull (↓N chip when behind>0)
                                .child(
                                    make_btn(
                                        "tb-pull",
                                        "Pull",
                                        gpui_component::Icon::new(
                                            gpui_component::IconName::ArrowDown,
                                        ),
                                        ButtonState::Availability(toolbar.pull_on),
                                        pull_reason,
                                        toolbar.behind,
                                    )
                                    .on_click(pull_click),
                                )
                                .when(gap(1, 2), |el| el.child(div().w(theme::scaled_px(2.0))))
                                // Push (↑N chip when ahead>0)
                                .child(
                                    make_btn(
                                        "tb-push",
                                        "Push",
                                        gpui_component::Icon::new(
                                            gpui_component::IconName::ArrowUp,
                                        ),
                                        ButtonState::Availability(toolbar.push_on),
                                        push_reason,
                                        toolbar.ahead,
                                    )
                                    .on_click(push_click),
                                )
                                .when(gap(3, 0xfc), |el| el.child(sep()))
                                // Branch
                                .child(
                                    make_btn(
                                        "tb-branch",
                                        "Branch",
                                        gpui_component::Icon::new(gpui_component::IconName::Plus),
                                        ButtonState::Availability(true),
                                        None,
                                        0,
                                    )
                                    .on_click(branch_click),
                                )
                                .when(gap(4, 0x18), |el| el.child(div().w(theme::scaled_px(2.0))))
                                // Stash
                                .child(
                                    make_btn(
                                        "tb-stash",
                                        "Stash",
                                        gpui_component::Icon::new(gpui_component::IconName::Inbox),
                                        ButtonState::Availability(toolbar.stash_on),
                                        stash_reason,
                                        0,
                                    )
                                    .on_click(stash_click),
                                )
                                .when(gap(8, 0x10), |el| el.child(div().w(theme::scaled_px(2.0))))
                                // Pop
                                .child(
                                    make_btn(
                                        "tb-pop",
                                        "Pop",
                                        gpui_component::Icon::new(
                                            gpui_component::IconName::FolderOpen,
                                        ),
                                        ButtonState::Availability(toolbar.pop_on),
                                        pop_reason,
                                        0,
                                    )
                                    .on_click(pop_click),
                                )
                                .when(gap(0x1c, 0xe0), |el| el.child(sep()))
                                // Undo — operation-history undo (T-UNDOREDO-001). Label fixed; the
                                // previewed operation summary is shown in the tooltip.
                                .child(
                                    make_btn(
                                        "tb-undo",
                                        Msg::Undo.t(),
                                        gpui_component::Icon::new(gpui_component::IconName::Undo2),
                                        ButtonState::Availability(undo_on),
                                        undo_reason,
                                        0,
                                    )
                                    .tooltip_summary(undo_tooltip_text)
                                    .on_click(undo_click),
                                )
                                // Redo — operation-history redo (T-UNDOREDO-001).
                                .child(
                                    make_btn(
                                        "tb-redo",
                                        Msg::Redo.t(),
                                        gpui_component::Icon::new(gpui_component::IconName::Redo2),
                                        ButtonState::Availability(redo_on),
                                        redo_reason,
                                        0,
                                    )
                                    .tooltip_summary(redo_tooltip_text)
                                    .on_click(redo_click),
                                )
                                .when(gap(0x60, 0x80), |el| {
                                    el.child(div().w(theme::scaled_px(2.0)))
                                })
                                // Terminal (toggles bottom panel Terminal tab)
                                .child(
                                    make_btn(
                                        "tb-terminal",
                                        "Terminal",
                                        gpui_component::Icon::new(
                                            gpui_component::IconName::SquareTerminal,
                                        ),
                                        ButtonState::Selection(terminal_on),
                                        None,
                                        0,
                                    )
                                    .on_click(terminal_click),
                                ),
                        )
                    }),
            ) // ── end CENTRE cluster ──
            // ── RIGHT column (flex_1; natural workspace/overflow minimum) ──
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
                    .min_w(px(workspace_width))
                    // Auto-update (ADR-0082): "↑ Update vX.Y.Z" chip when a newer
                    // release is available; click opens the update modal.
                    .when_some(
                        self.update_available
                            .as_ref()
                            .filter(|_| hidden & (1 << 13) == 0 || pending_widths & (1 << 13) != 0)
                            .map(|(p, _)| p.tag.clone()),
                        |el, tag| {
                            let open = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.activate_toolbar(13, None, window, cx);
                            });
                            let chip_text = SharedString::from(format!("\u{2191} Update {}", tag));
                            if hidden & (1 << 13) != 0 {
                                return if pending_widths & (1 << 13) != 0 {
                                    el.child(
                                        div()
                                            .absolute()
                                            .invisible()
                                            .flex()
                                            .border_2()
                                            .border_color(gpui::transparent_black())
                                            .px(keyboard_nav::inset(8.0))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .child(chip_text),
                                            )
                                            .child(toolbar_overflow::probe(
                                                layout_app.clone(),
                                                Some(13),
                                                style_key,
                                            )),
                                    )
                                } else {
                                    el
                                };
                            }
                            let hover_fill =
                                gpui_component::ActiveTheme::theme(&**cx).button_primary_hover;
                            el.child(
                                keyboard_nav::focusable(
                                    div()
                                        .id("tb-update")
                                        .role(gpui::Role::Button)
                                        .aria_label(chip_text.clone())
                                        .relative()
                                        .child(super::e2e::measure_inside("tb-update"))
                                        .flex_shrink_0()
                                        .child(toolbar_overflow::probe(
                                            layout_app.clone(),
                                            Some(13),
                                            style_key,
                                        ))
                                        .flex()
                                        .items_center()
                                        .px(keyboard_nav::inset(8.0))
                                        .py(keyboard_nav::inset(4.0))
                                        .mr(theme::scaled_px(8.0))
                                        .rounded_md()
                                        .bg(rgb(theme().ui_accent()))
                                        .cursor(gpui::CursorStyle::Arrow)
                                        .hover(move |s| s.bg(hover_fill))
                                        .child(
                                            div()
                                                .text_color(rgb(theme::primary_button_foreground(
                                                    &theme(),
                                                )))
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .child(chip_text),
                                        ),
                                )
                                .on_click(open),
                            )
                        },
                    )
                    // Graph — the default mode; always available.
                    .child(
                        make_btn(
                            "tb-graph-mode",
                            "Graph",
                            gpui_component::Icon::default().path("icons/waypoints.svg"),
                            ButtonState::Availability(true),
                            None,
                            0,
                        )
                        .on_click(graph_click),
                    )
                    .when(gap(0x100, 0xe00), |el| {
                        el.child(div().w(theme::scaled_px(2.0)))
                    })
                    // PRs — hidden when gh isn't installed (nothing to show).
                    .when(pr_mode_on, |el| {
                        el.child(
                            make_btn(
                                "tb-pr-mode",
                                "PRs",
                                gpui_component::Icon::default().path("icons/git-pull-request.svg"),
                                ButtonState::Availability(true),
                                None,
                                0,
                            )
                            .on_click(pr_mode_click),
                        )
                        .when(gap(0x200, 0xc00), |el| {
                            el.child(div().w(theme::scaled_px(2.0)))
                        })
                    })
                    // Editor
                    .child(
                        make_btn(
                            "tb-editor-ws",
                            "Editor",
                            gpui_component::Icon::default().path("icons/square-pen.svg"),
                            ButtonState::Availability(editor_ws_on),
                            no_repo_reason,
                            0,
                        )
                        .on_click(editor_ws_click),
                    )
                    .when(gap(0x400, 0x800), |el| {
                        el.child(div().w(theme::scaled_px(2.0)))
                    })
                    // Analyze / Code Ecosystem (ADR-0119) — read-only hot-spot
                    // analysis; placed just left of Settings.
                    .child(
                        make_btn(
                            "tb-ecosystem",
                            "Analyze",
                            gpui_component::Icon::new(gpui_component::IconName::ChartPie),
                            ButtonState::Availability(ecosystem_on),
                            no_repo_reason,
                            0,
                        )
                        .on_click(ecosystem_click),
                    )
                    .when(gap(0xf00, 0x1000), |el| el.child(sep()))
                    .child({
                        let settings_click =
                            cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.activate_toolbar(12, None, window, cx)
                            });
                        // Measured so Tier A opens Settings by pointer (#974).
                        super::e2e::measure_control(
                            "tb-settings",
                            make_btn(
                                "tb-settings",
                                "Settings",
                                gpui_component::Icon::new(gpui_component::IconName::Settings),
                                ButtonState::Availability(true),
                                None,
                                0,
                            )
                            .on_click(settings_click),
                        )
                    })
                    .child(toolbar_overflow::button(
                        layout_app.clone(),
                        hidden,
                        [
                            pull_reason,
                            push_reason,
                            None,
                            stash_reason,
                            pop_reason,
                            undo_reason,
                            redo_reason,
                            None,
                            None,
                            None,
                            no_repo_reason,
                            no_repo_reason,
                            None,
                            None,
                        ],
                        self.root_focus.clone(),
                    )),
            )
    }
}
