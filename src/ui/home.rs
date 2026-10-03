//! The Home tab (#923, ADR-0219 decision 1): Kagi's dashboard, opened by the
//! tab strip's `+` and New Tab (⌘T), and shown instead of an empty window
//! when no repository tab is open.
//!
//! Home is not a [`super::tabs::RepoTab`]: it owns no session, worktree or
//! read model, so nothing keyed by `SessionId` has to know it exists. There is
//! at most one. While it is in front the repository behind it is not on
//! screen, so its repo-scoped modals close as on any tab switch and every
//! repository command is disabled (`commands::command_state`).
//!
//! Opening a repository while Home is in front — from Home itself, the folder
//! dialog or anywhere else — turns Home into that repository's tab; clicking
//! another tab only moves Home to the back, like a browser's new-tab page.

use std::path::PathBuf;

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants as _};

use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

/// The Home tab in the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HomeTab {
    /// In front of the repository tabs (otherwise it waits in the strip).
    pub front: bool,
    /// The repository tab whose visit ended when Home came to the front. A
    /// plan or result still in flight for it is dropped (its visit changed),
    /// and going back to it re-enters it like any tab switch.
    left: Option<crate::app::SessionId>,
}

const SIDEBAR_W: f32 = 300.;
const MAIN_MAX_W: f32 = 720.;

impl KagiApp {
    /// Does the window show Home? Always when no tab is open (except the
    /// read-only remote view, which has no tab but a workspace).
    pub fn home_in_front(&self) -> bool {
        if self.tabs.is_empty() {
            self.remote_view.is_none()
        } else {
            self.home.is_some_and(|home| home.front)
        }
    }

    /// `+` / New Tab: open the Home tab, or bring it to the front.
    ///
    /// The repository on screen is left like on any tab switch: its visit
    /// ends (`depart_active_tab`), so an async plan or result for it that
    /// lands while Home is in front is dropped rather than put in the modal
    /// slot, where Home's key routing could confirm it unseen (#927 review).
    pub fn open_home_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            // Home is already the whole window — except in the remote view,
            // which has no strip to come back to: there New Tab keeps its old
            // meaning, the folder dialog.
            if self.remote_view.is_some() {
                self.pick_repository(window, cx);
            }
            return;
        }
        let left = match self.home {
            Some(home) if home.front => home.left,
            _ => {
                let left = self.active_session();
                self.depart_active_tab();
                left
            }
        };
        self.home = Some(HomeTab { front: true, left });
        if let Some(root) = self.root_focus.clone() {
            window.focus(&root, cx);
        }
        klog!("home: front tabs={}", self.tabs.len());
        cx.notify();
    }

    /// The Home tab's ×. With no repository tab, Home stays the window;
    /// otherwise the tab behind it is entered again.
    pub fn close_home_tab(&mut self, cx: &mut Context<Self>) {
        let left = self
            .home
            .filter(|home| home.front)
            .and_then(|home| home.left);
        self.home = None;
        klog!("home: closed");
        if !self.tabs.is_empty() {
            self.return_to_tab(self.active_tab, left, cx);
        }
        cx.notify();
    }

    /// The tab Home covered was closed while Home is in front: `session` is
    /// now the tab behind it, not yet entered, and leaving Home enters it.
    pub(crate) fn retarget_home(&mut self, session: crate::app::SessionId) {
        if let Some(home) = self.home.as_mut().filter(|home| home.front) {
            home.left = Some(session);
        }
    }

    /// The last repository tab was closed: an open Home is now the whole
    /// window, so it is in front — opening a repository then turns it into
    /// that tab instead of leaving it in the strip (#930 review).
    pub(crate) fn home_takes_window(&mut self) {
        if let Some(home) = self.home.as_mut() {
            home.front = true;
            home.left = None;
        }
    }

    /// A repository tab was clicked: Home (if open) waits in the strip.
    /// Returns the tab it had left, for [`KagiApp::return_to_tab`].
    pub(crate) fn send_home_back(&mut self) -> Option<crate::app::SessionId> {
        let home = self.home.as_mut()?;
        let left = if home.front { home.left.take() } else { None };
        home.front = false;
        left
    }

    /// A repository is being opened (or switched to) as a tab: if Home was in
    /// front, it becomes that tab. Returns the tab Home had left.
    pub(crate) fn home_yields_to_repository(&mut self) -> Option<crate::app::SessionId> {
        let home = self.home.filter(|home| home.front)?;
        self.home = None;
        home.left
    }

    /// Show tab `index` after Home: the tab Home had left is entered again
    /// (its visit ended, so a re-select must not be the usual no-op), any
    /// other is an ordinary switch.
    pub(crate) fn return_to_tab(
        &mut self,
        index: usize,
        left: Option<crate::app::SessionId>,
        cx: &mut Context<Self>,
    ) {
        let session = self.tabs.get(index).map(|tab| tab.session);
        if left.is_some() && session == left {
            self.enter_tab(index, cx);
        } else {
            self.switch_repo(index, cx);
        }
        cx.notify();
    }

    /// The Home body: the tab strip (when tabs exist), a sidebar of recently
    /// opened repositories and the main column. Window-global modals and the
    /// menu commands are attached here because this path returns before the
    /// workspace compositor.
    pub fn render_home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_home_github(window, cx);
        let body = div()
            .id("home-tab-panel")
            .flex()
            .flex_row()
            .flex_1()
            .min_h(px(0.))
            .child(self.render_home_sidebar(cx))
            .child(self.render_home_main(window, cx));
        // Home's content is the Home cell's panel while the strip is drawn
        // (#983); with no tab there is no strip, so no tab list.
        let body = if self.tabs.is_empty() {
            body
        } else {
            super::tab_panel_a11y::tab_panel(body, "home-tab-panel", Msg::HomeTabTitle.t())
        };
        let home = div()
            .id("home")
            .flex()
            .flex_col()
            .size_full()
            .font_family(super::UI_FONT)
            .bg(rgb(theme().bg_base))
            .when_some(self.root_focus.clone(), |el, fh| el.track_focus(&fh))
            // With no tab there is no strip, but the (transparent, themed)
            // title bar still needs its band: the traffic lights are drawn
            // over it and it is what drags the window.
            .child(match self.render_tab_strip(window, cx) {
                Some(strip) => strip,
                None => div()
                    .w_full()
                    .h(theme::scaled_px(super::tabs::TAB_STRIP_H))
                    .flex_shrink_0()
                    .bg(rgb(theme().panel))
                    .border_b_1()
                    .border_color(rgb(theme().surface))
                    .window_control_area(gpui::WindowControlArea::Drag)
                    .into_any_element(),
            })
            .child(body);
        // A flex box, as the Welcome screen's root was: the window-global
        // modals attached below are absolute overlays, and in a plain block
        // box they landed after Home's content instead of covering it.
        // The menu-driven overlays (Settings, About, Shortcuts, the command
        // palette) are window-global and stay enabled on Home, so they are
        // drawn here too, below the modals as in the workspace (#927 review).
        let menu_overlay = self.render_menu_overlay(window, cx);
        let home = self.register_menu_actions(
            div()
                .flex()
                .flex_col()
                .relative()
                .size_full()
                .child(home)
                .children(menu_overlay),
            cx,
        );
        // The same modal layer as the workspace: every modal the shared key
        // routing below can confirm is drawn — a guard or plan for the tab
        // behind Home included (#930 review). The tab's own popovers are
        // not: Home covers that tab. Toasts above everything, as in the
        // workspace: an operation that ends while Home is in front (a failed
        // clone) says so here, with its details in Operation Log.
        let content = self
            .attach_modal_layer(home, false, window, cx)
            .children(self.render_toasts(cx))
            .into_any();
        self.attach_active_modal_key_routing(content, false, cx)
    }

    fn render_home_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let recent = super::tabs::recent_repos();
        let mut list = div().flex().flex_col().gap_1();
        if recent.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::HomeNoRecent.t())),
            );
        }
        for (i, path) in recent.into_iter().enumerate() {
            list = list.child(super::e2e::measure_control(
                format!("home-recent-{i}"),
                recent_row(i, path, cx),
            ));
        }
        div()
            .id("home-sidebar")
            .flex()
            .flex_col()
            .gap_2()
            .w(theme::scaled_px(SIDEBAR_W))
            .flex_shrink_0()
            .h_full()
            .p_3()
            .bg(rgb(theme().panel))
            .border_r_1()
            .border_color(rgb(theme().surface))
            .overflow_y_scroll()
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::HomeRecent.t())),
            )
            .child(list)
            .into_any_element()
    }

    fn render_home_main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let open_folder = cx.listener(|app, _: &gpui::ClickEvent, window, cx| {
            app.pick_repository(window, cx);
        });
        let remote = cx.listener(|app, _: &gpui::ClickEvent, _, cx| {
            app.open_remote_browse(cx);
        });
        let actions = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(super::e2e::measure_control(
                "home-open-folder",
                Button::new("home-open-folder")
                    .primary()
                    .label(Msg::HomeOpenFolder.t())
                    .on_click(open_folder),
            ))
            .child(super::e2e::measure_control(
                "home-remote",
                Button::new("home-remote")
                    .outline()
                    .label(Msg::HomeConnectRemote.t())
                    .on_click(remote),
            ));
        // The column is bounded and does not scroll as a whole: the GitHub
        // list below the fixed top part is a virtualized list with its own
        // scrolling, so typing in its filter re-renders only visible rows.
        div()
            .id("home-main")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            // Most of Home is empty surface: let it drag the window, as the
            // Welcome screen did (the themed title bar has no OS drag area
            // when no tab strip is drawn). Controls keep their own clicks.
            .window_control_area(gpui::WindowControlArea::Drag)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .mx_auto()
                    .w_full()
                    .h_full()
                    .max_w(theme::scaled_px(MAIN_MAX_W))
                    .px_6()
                    .pt(theme::scaled_px(48.))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_2xl()
                            .text_color(rgb(theme().text_main))
                            .child(SharedString::from(Msg::HomeTitle.t())),
                    )
                    .child(actions.flex_shrink_0())
                    .child(self.render_home_github(window, cx)),
            )
            .into_any_element()
    }
}

/// One recently opened repository: its name over its path. Opening it turns
/// Home into its tab (or switches to the tab it already has).
fn recent_row(i: usize, path: PathBuf, cx: &mut Context<KagiApp>) -> impl IntoElement {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let full = path.display().to_string();
    let open = cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
        app.open_repository(path.clone(), cx);
        cx.notify();
    });
    div()
        .id(("home-recent", i))
        .flex()
        .flex_col()
        .px_2()
        .py_1()
        .rounded_md()
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(open)
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(SharedString::from(name)),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .truncate()
                .child(safe_text(&full)),
        )
}
