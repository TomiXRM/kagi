//! W4-TABS: Repository tab strip + directory picker + Welcome screen.
//!
//! All multi-repository logic is intentionally collected here (a parallel lane
//! is editing `src/ui/mod.rs`, so the surface touched there is kept minimal —
//! see the W4-TABS completion report for the exact list of mod.rs changes).
//!
//! Model (ADR-0027): a lightweight tab descriptor [`RepoTab`] plus the single
//! heavyweight per-repo state on [`KagiApp`].  `switch_repo` rebuilds that
//! heavyweight state from a fresh snapshot and resets per-repo UI state.
//!
//! Picker (ADR-0028): `cx.prompt_for_paths` (NSOpenPanel on macOS).  The
//! oneshot `Receiver` is awaited on a `cx.spawn` task.
//!
//! Watcher (ADR-0027): `watcher_generation` is bumped on every switch/open/
//! close so the previously-armed loop terminates itself on a generation
//! mismatch.  `arm_watcher` replaces the fixed spawn that used to live in
//! `run_app`.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    Context, PathPromptOptions, SharedString, Timer, Window,
    div, prelude::*, px, rgb,
};
use gpui_component::tooltip::Tooltip;

use super::{
    KagiApp, ToastKind, FooterStatus,
    BG_BASE, BG_SURFACE, BG_SELECTED, BG_PANEL,
    TEXT_MAIN, TEXT_SUB, TEXT_MUTED, COLOR_BRANCH,
};

/// Lightweight descriptor for one open repository tab (ADR-0027).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoTab {
    /// Absolute path to the repository working tree root.
    pub path: PathBuf,
    /// Display name (working-tree directory name).
    pub name: String,
}

/// Height of the tab strip in pixels.
const TAB_STRIP_H: f32 = 30.0;
/// Minimum / maximum width of a single tab (truncate beyond max).
const TAB_MIN_W: f32 = 80.0;
const TAB_MAX_W: f32 = 200.0;

impl KagiApp {
    // ──────────────────────────────────────────────────────────────────────
    // Tab model operations
    // ──────────────────────────────────────────────────────────────────────

    /// Open the repository at `path` as a new tab (or switch to it if already
    /// open).  Validates with `open_repository`; on failure no tab is created
    /// and an error toast + footer message is shown (ADR-0028).
    ///
    /// Returns `true` if a tab is now active for the repo, `false` on failure.
    pub fn open_repository(&mut self, path: PathBuf, cx: &mut Context<Self>) -> bool {
        use kagi::git::open_repository;

        // Normalise so the same repo opened via different relative paths maps
        // to one tab.  Fall back to the original path if canonicalize fails.
        let path = std::fs::canonicalize(&path).unwrap_or(path);

        // Already open? → switch to the existing tab.
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.switch_repo(idx, cx);
            return true;
        }

        // Validate the repository before creating a tab.
        let info = match open_repository(&path) {
            Ok(info) => info,
            Err(e) => {
                let msg = format!("Error: {e}");
                eprintln!("[kagi] open: {}", msg);
                self.status_footer = FooterStatus::Failed(SharedString::from(msg.clone()));
                self.push_toast(ToastKind::Error, msg);
                cx.notify();
                return false;
            }
        };

        let tab = RepoTab {
            path: path.clone(),
            name: info.name.clone(),
        };
        self.tabs.push(tab);
        let new_idx = self.tabs.len() - 1;
        self.switch_repo(new_idx, cx);
        true
    }

    /// Switch the active tab to `index` (W6-TABSPEED / ADR-0030).
    ///
    /// Stale-while-revalidate: if the target repo's [`TabViewState`] is cached
    /// it is applied **instantly** (zero-frame swap) and a background revalidate
    /// refreshes it; otherwise a `Loading <name>…` placeholder + `Busy` footer
    /// is shown while the snapshot is built on a background thread.  Per-repo UI
    /// state (selection / diff_cache / main_diff / modals / commit_panel) is
    /// reset either way, and the FS watcher is re-armed (ADR-0027 generation
    /// scheme).
    pub fn switch_repo(&mut self, index: usize, cx: &mut Context<Self>) {
        let tab = match self.tabs.get(index) {
            Some(t) => t.clone(),
            None => return,
        };
        self.active_tab = index;
        self.error = None;

        // Point repo_path at the new repo before any apply.
        self.repo_path = Some(tab.path.clone());

        // Reset every per-repo UI surface up-front so a cached instant-apply
        // never shows the previous tab's selection / modals (ADR-0027).
        self.reset_per_repo_ui();

        // W6-TABSPEED: bump the switch generation so an in-flight background
        // load from an earlier (superseded) switch discards its result.
        self.switch_generation = self.switch_generation.wrapping_add(1);
        let generation = self.switch_generation;

        let cached = self.tab_cache.get(&tab.path).cloned();
        eprintln!(
            "[kagi] tab-switch: {} cached={}",
            tab.name,
            if cached.is_some() { "yes" } else { "no" }
        );

        if let Some(view) = cached {
            // Instant swap — no perceptible latency.
            self.loading_tab = None;
            self.apply_tab_view(view);
        } else {
            // First open: show a loading placeholder while we snapshot.
            self.loading_tab = Some(SharedString::from(format!("Loading {}\u{2026}", tab.name)));
            self.status_footer =
                FooterStatus::Busy(SharedString::from(format!("Loading {}\u{2026}", tab.name)));
        }

        // Re-arm the watcher for the new repo and repaint immediately so the
        // instant-apply / loading placeholder is visible this frame.
        self.log_tabs();
        self.arm_watcher(cx);
        cx.notify();

        // Background (re)load to refresh / fill the cache.
        self.load_repo_async(tab.path.clone(), tab.name.clone(), generation, cx);
    }

    /// Reset all per-repo transient UI state (selection / diffs / modals /
    /// commit panel).  Shared by `switch_repo` (W6-TABSPEED instant-apply path)
    /// so a cached swap never leaks the previous tab's UI.
    fn reset_per_repo_ui(&mut self) {
        self.selected = None;
        self.diff_cache.clear();
        self.main_diff = None;
        self.plan_modal = None;
        self.pull_modal = None;
        self.undo_modal = None;
        self.pop_modal = None;
        self.push_modal = None;
        self.create_branch_modal = None;
        self.stash_push_modal = None;
        self.stash_apply_modal = None;
        self.cherry_pick_modal = None;
        self.delete_branch_modal = None;
        self.commit_panel_open = false;
        self.commit_panel = None;
        self.commit_input = None;
    }

    /// W6-TABSPEED / ADR-0030: snapshot + build the [`TabViewState`] on a
    /// background thread (`RepoSnapshot` is `Send`), then apply it on the main
    /// thread iff this load is still the most-recent switch (`generation`
    /// guard).  Updates `tab_cache`, clears any loading placeholder, and emits
    /// `[kagi] tab-load: <name> rows=N`.
    fn load_repo_async(
        &mut self,
        path: PathBuf,
        name: String,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let bg_path = path.clone();
        let bg_name = name.clone();
        let task = cx.background_spawn(async move {
            let mut repo = git2::Repository::open(&bg_path)
                .map_err(|e| format!("repo open error: {}", e.message()))?;
            let snap = kagi::git::snapshot(&mut repo, 10_000)
                .map_err(|e| format!("snapshot error: {e}"))?;
            let repo_name = bg_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bg_name.clone());
            Ok::<super::TabViewState, String>(super::build_tab_view(&snap, &repo_name))
        });

        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                // Generation guard: a later switch supersedes this load.
                if app.switch_generation != generation {
                    return;
                }
                match result {
                    Ok(view) => {
                        let rows = view.rows.len();
                        app.tab_cache.insert(path.clone(), view.clone());
                        app.apply_tab_view(view);
                        app.loading_tab = None;
                        if matches!(app.status_footer, FooterStatus::Busy(_)) {
                            app.status_footer =
                                FooterStatus::Idle(SharedString::from("Ready"));
                        }
                        eprintln!("[kagi] tab-load: {} rows={}", name, rows);
                        cx.notify();
                    }
                    Err(err) => {
                        app.loading_tab = None;
                        let msg = format!("Error: {err}");
                        eprintln!("[kagi] tab-load: {} error: {}", name, err);
                        app.status_footer =
                            FooterStatus::Failed(SharedString::from(msg));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Close the tab at `index`.  Discards that tab's per-repo state only
    /// (the repository itself is untouched).  Closing the last tab returns to
    /// the Welcome screen (ADR-0027 / ADR-0028).
    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let closed = self.tabs.remove(index);
        // Drop the closed repo's terminal session (PTY closes on drop).
        self.terminal_sessions.remove(&closed.path);
        // W6-TABSPEED / ADR-0030: evict the closed repo's cached view state.
        self.tab_cache.remove(&closed.path);

        if self.tabs.is_empty() {
            // Last tab closed → Welcome screen.
            self.active_tab = 0;
            self.repo_path = None;
            self.show_welcome();
            self.log_tabs();
            // Bump generation so the old watcher loop terminates; no new arm.
            self.watcher_generation = self.watcher_generation.wrapping_add(1);
            cx.notify();
            return;
        }

        // Recompute the active tab index. If we closed a tab before the active
        // one, the active index shifts left; clamp into range either way.
        let new_active = if index < self.active_tab {
            self.active_tab - 1
        } else {
            self.active_tab.min(self.tabs.len() - 1)
        };
        self.switch_repo(new_active, cx);
    }

    /// Reset the app to the Welcome screen (no repo open).  Clears per-repo
    /// state so a stale commit list / sidebar is not shown behind the Welcome
    /// overlay.
    fn show_welcome(&mut self) {
        let blank = KagiApp::with_error("");
        self.error = None;
        self.header = SharedString::from("kagi");
        self.rows = blank.rows;
        self.details = blank.details;
        self.selected = None;
        self.diff_cache.clear();
        self.main_diff = None;
        self.branches = blank.branches;
        self.remote_branches = blank.remote_branches;
        self.tags = blank.tags;
        self.stashes = blank.stashes;
        self.is_dirty = false;
        self.branch_targets = blank.branch_targets;
        self.commit_row_index = blank.commit_row_index;
        self.branch_upstream_info = blank.branch_upstream_info;
        self.status_summary = blank.status_summary;
        self.toolbar_state = blank.toolbar_state;
        self.plan_modal = None;
        self.pull_modal = None;
        self.undo_modal = None;
        self.pop_modal = None;
        self.push_modal = None;
        self.create_branch_modal = None;
        self.stash_push_modal = None;
        self.stash_apply_modal = None;
        self.cherry_pick_modal = None;
        self.delete_branch_modal = None;
        self.commit_panel_open = false;
        self.commit_panel = None;
        self.commit_input = None;
        self.status_footer = FooterStatus::Idle(SharedString::from("Ready"));
    }

    /// Emit the headless tabs log line required by ADR-0027:
    /// `[kagi] tabs: n=<N> active=<i> <name>`.
    pub fn log_tabs(&self) {
        let name = self
            .tabs
            .get(self.active_tab)
            .map(|t| t.name.as_str())
            .unwrap_or("-");
        eprintln!(
            "[kagi] tabs: n={} active={} {}",
            self.tabs.len(),
            self.active_tab,
            name
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // Watcher (ADR-0027 generation scheme)
    // ──────────────────────────────────────────────────────────────────────

    /// Arm the `.git` FS watcher for the current `repo_path`, bumping
    /// `watcher_generation` first.  The spawned loop captures the new
    /// generation and exits as soon as `watcher_generation` no longer matches
    /// (i.e. a later switch/open/close re-armed or cleared the watcher),
    /// preventing double-reload from stale loops.
    pub fn arm_watcher(&mut self, cx: &mut Context<Self>) {
        // Bump first so any previously-armed loop sees a mismatch and stops.
        self.watcher_generation = self.watcher_generation.wrapping_add(1);
        let generation = self.watcher_generation;

        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        let (rx, watcher) = match super::watcher::start_git_watcher(&repo_path) {
            Some(pair) => pair,
            None => return,
        };

        cx.spawn(async move |weak, acx| {
            // Hold the watcher alive for the lifetime of this task.
            let _watcher = watcher;

            loop {
                Timer::after(Duration::from_millis(100)).await;

                // Stop if this loop has been superseded (generation bumped).
                let still_current = weak
                    .read_with(acx, |app, _| app.watcher_generation == generation)
                    .unwrap_or(false);
                if !still_current {
                    break;
                }

                let got_signal = rx.try_recv().is_ok();
                if !got_signal {
                    continue;
                }

                // Debounce, then drain any extra signals.
                Timer::after(super::watcher::DEBOUNCE).await;
                while rx.try_recv().is_ok() {}

                // Re-check generation after the debounce window — a switch may
                // have happened while we slept.
                let result = acx.update(|cx| {
                    weak.update(cx, |app, cx| {
                        if app.watcher_generation == generation {
                            app.reload_external(cx);
                        }
                    })
                });
                if result.is_err() {
                    break; // app gone
                }
            }
        })
        .detach();
    }

    // ──────────────────────────────────────────────────────────────────────
    // Directory picker (ADR-0028)
    // ──────────────────────────────────────────────────────────────────────

    /// Open the native directory picker and, on selection, open the chosen
    /// directory as a repository tab.  Headless builds cannot open the panel
    /// (use `KAGI_OPEN_REPO` instead — see main.rs).
    pub fn pick_repository(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::from("Open Repository")),
        });

        cx.spawn(async move |weak, acx| {
            let picked: Option<PathBuf> = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            if let Some(path) = picked {
                let _ = acx.update(|cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.open_repository(path, cx);
                    });
                });
            }
        })
        .detach();
    }

    // ──────────────────────────────────────────────────────────────────────
    // Rendering
    // ──────────────────────────────────────────────────────────────────────

    /// Render the repository tab strip (above the header toolbar).  Returns
    /// `None` when no tabs are open (Welcome screen is shown instead).
    pub fn render_tab_strip(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.tabs.is_empty() {
            return None;
        }

        let active = self.active_tab;
        let tabs: Vec<RepoTab> = self.tabs.clone();

        let mut strip = div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(px(TAB_STRIP_H))
            .bg(rgb(BG_PANEL))
            .border_b_1()
            .border_color(rgb(BG_SURFACE));

        for (i, tab) in tabs.into_iter().enumerate() {
            let is_active = i == active;
            let bg = if is_active { BG_SELECTED } else { BG_SURFACE };
            let fg = if is_active { TEXT_MAIN } else { TEXT_SUB };
            let full_path = tab.path.display().to_string();

            // chars()-based truncation is handled by `.truncate()` on the label
            // div (the byte-slice approach panics on multi-byte names).
            let label = SharedString::from(tab.name.clone());

            let switch = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
                this.switch_repo(i, cx);
            });
            let close = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
                this.close_tab(i, cx);
            });

            let close_btn = div()
                .id(("tab-close", i))
                .ml(px(4.))
                .px(px(3.))
                .rounded_sm()
                .text_color(rgb(TEXT_MUTED))
                .hover(|s| s.bg(rgb(BG_SURFACE)).text_color(rgb(TEXT_MAIN)))
                .cursor(gpui::CursorStyle::PointingHand)
                .child(SharedString::from("\u{00d7}")) // ×
                .on_click(close);

            let tab_el = div()
                .id(("repo-tab", i))
                .flex()
                .flex_row()
                .items_center()
                .h_full()
                .min_w(px(TAB_MIN_W))
                .max_w(px(TAB_MAX_W))
                .px_2()
                .gap_1()
                .bg(rgb(bg))
                .text_sm()
                .text_color(rgb(fg))
                .border_r_1()
                .border_color(rgb(BG_PANEL))
                .when(is_active, |el| el.border_t_2().border_color(rgb(COLOR_BRANCH)))
                .cursor(gpui::CursorStyle::PointingHand)
                .tooltip({
                    let full = full_path.clone();
                    move |window, cx| Tooltip::new(full.clone()).build(window, cx)
                })
                .on_click(switch)
                .child(div().flex_1().truncate().child(label))
                .child(close_btn);

            strip = strip.child(tab_el);
        }

        // [+] new-tab button at the right end → directory picker.
        let plus = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
            this.pick_repository(window, cx);
        });
        let plus_btn = div()
            .id("tab-add")
            .flex()
            .items_center()
            .justify_center()
            .h_full()
            .px_3()
            .text_color(rgb(TEXT_SUB))
            .hover(|s| s.bg(rgb(BG_SELECTED)).text_color(rgb(TEXT_MAIN)))
            .cursor(gpui::CursorStyle::PointingHand)
            .tooltip(|window, cx| Tooltip::new("Open Repository…").build(window, cx))
            .child(SharedString::from("+"))
            .on_click(plus);

        strip = strip.child(plus_btn);

        Some(strip.into_any())
    }

    /// Render the Welcome screen shown when no tab is open (ADR-0028).
    /// Centred "Open Repository…" button + description.
    pub fn render_welcome(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let open_click = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
            this.pick_repository(window, cx);
        });

        let button = div()
            .id("welcome-open")
            .px_4()
            .py_2()
            .rounded_md()
            .bg(rgb(BG_SELECTED))
            .text_color(rgb(TEXT_MAIN))
            .text_lg()
            .hover(|s| s.bg(rgb(COLOR_BRANCH)).text_color(rgb(BG_BASE)))
            .cursor(gpui::CursorStyle::PointingHand)
            .child(SharedString::from("Open Repository\u{2026}"))
            .on_click(open_click);

        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .size_full()
            .bg(rgb(BG_BASE))
            .child(
                div()
                    .text_2xl()
                    .text_color(rgb(TEXT_MAIN))
                    .child(SharedString::from("kagi")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(TEXT_MUTED))
                    .child(SharedString::from(
                        "No repository open. Choose a directory to get started.",
                    )),
            )
            .child(button)
            .into_any()
    }
}
