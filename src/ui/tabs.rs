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

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{div, prelude::*, px, rgb, Context, PathPromptOptions, SharedString, Timer, Window};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::tooltip::Tooltip;
use gpui_component::Sizable as _;

use super::i18n::{self, Msg};
use super::theme::{self, theme};
use super::{FooterStatus, KagiApp, ToastKind};

/// Lightweight descriptor for one open repository tab (ADR-0027).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoTab {
    /// Absolute path to the repository working tree root. For a **remote** tab
    /// (ADR-0089 Phase 2b) this is a synthetic identity key (`<host>:<root>`),
    /// not a local path — `remote` is `Some` in that case.
    pub path: PathBuf,
    /// Display name (working-tree directory name).
    pub name: String,
    /// `Some` for a remote read-only repository opened over SSH; `None` for a
    /// normal local repository.
    pub remote: Option<super::RemoteRepoView>,
    /// True when this tab is a linked git worktree (shown with a 🌳 marker and a
    /// distinct tab colour so it's not mistaken for the main repository).
    pub is_worktree: bool,
    /// Lane-colour index for a worktree tab, matching that worktree's WIP-row
    /// colour (its rank in the repo's worktree list). Set when the tab's view is
    /// applied; `None` until then / for non-worktree tabs.
    pub wt_color_idx: Option<usize>,
}

/// Height of the tab strip in pixels. The strip doubles as the themed title bar
/// (the OS bar is transparent), so it carries a little extra height for padding
/// around the tabs and the traffic lights.
const TAB_STRIP_H: f32 = 40.0;
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
        use kagi_git::open_repository;

        // Normalise so the same repo opened via different relative paths maps
        // to one tab.  Fall back to the original path if canonicalize fails.
        let path = std::fs::canonicalize(&path).unwrap_or(path);

        // Already open? → switch to the existing tab. Compare on canonicalized
        // paths on BOTH sides so the same repo still maps to ONE tab even when an
        // existing tab was created from a non-canonical path (e.g. a CLI/session
        // `/tmp/...` vs this call's `/private/tmp/...` on macOS). Without this,
        // opening the main repo from inside a worktree would spawn a second tab
        // for the same repository (tab-driver dedup bug).
        if let Some(idx) = self.tabs.iter().position(|t| {
            t.remote.is_none()
                && (t.path == path
                    || std::fs::canonicalize(&t.path)
                        .map(|c| c == path)
                        .unwrap_or(false))
        }) {
            self.switch_repo(idx, cx);
            return true;
        }

        // Validate the repository before creating a tab.
        let info = match open_repository(&path) {
            Ok(info) => info,
            Err(e) => {
                let msg = format!("Error: {e}");
                klog!("open: {}", msg);
                self.status_footer = FooterStatus::Failed(SharedString::from(msg.clone()));
                self.push_toast(ToastKind::Error, msg);
                cx.notify();
                return false;
            }
        };

        // Remember it for the Welcome screen's "Recent" list.
        record_recent_repo(&path);

        let tab = RepoTab {
            path: path.clone(),
            name: info.name.clone(),
            remote: None,
            is_worktree: info.is_worktree,
            wt_color_idx: None,
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

        // ── Remote tab (ADR-0089 Phase 2b): re-enter the read-only view from
        //    the cached snapshot. No local path, no Backend, no watcher. ──
        if let Some(rv) = tab.remote.clone() {
            self.repo_path = None;
            self.repo_session = None;
            self.remote_view = Some(rv);
            self.reset_per_repo_ui();
            self.switch_generation = self.switch_generation.wrapping_add(1);
            if let Some(view) = self.tab_cache.get(&tab.path).cloned() {
                self.loading_tab = None;
                self.apply_tab_view(view);
            } else {
                // No cache (e.g. restored session): drop the stale tab rather
                // than show an empty view; the user can reconnect.
                self.loading_tab = None;
            }
            self.save_session();
            self.log_tabs();
            self.arm_watcher(cx); // returns early — repo_path is None
            cx.notify();
            return;
        }

        // Leaving any remote read-only view (ADR-0089 Phase 2b) — a local repo
        // is becoming active again.
        self.remote_view = None;

        // Point repo_path at the new repo before any apply.
        self.repo_path = Some(tab.path.clone());
        // ADR-0107: open (or re-use) a RepoSession for this tab so read paths
        // don't re-open the repo per interaction. Failure is non-fatal — read
        // paths fall back to Backend::open until the session succeeds.
        self.repo_session = kagi_git::session::RepoSession::open(&tab.path).ok();

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
            self.loading_tab = Some(SharedString::from(i18n::loading_fmt(&tab.name)));
            self.status_footer =
                FooterStatus::Busy(SharedString::from(i18n::loading_fmt(&tab.name)));
        }
        self.refresh_wip_diffstat();

        // Re-arm the watcher for the new repo and repaint immediately so the
        // instant-apply / loading placeholder is visible this frame.
        self.save_session();
        self.log_tabs();
        self.arm_watcher(cx);
        cx.notify();

        // Background (re)load to refresh / fill the cache.
        self.load_repo_async(tab.path.clone(), tab.name.clone(), generation, cx);
    }

    /// Show a **remote** repository (already snapshotted over SSH) in the main
    /// graph/sidebar/detail views, read-only (ADR-0089 Phase 2b).
    ///
    /// Mirrors the local apply path — `reset_per_repo_ui` + `apply_tab_view` from
    /// `build_tab_view(&snap, name)` — but with no `repo_path` (so the fs watcher
    /// stays disarmed and every local-path operation guards itself off). Unlike a
    /// local repo there is no working tree, so the tab carries a `remote` marker
    /// and a **synthetic identity path** (`<host>:<root>`) used as the tab-cache
    /// key and for de-duplication; `remote_view` keeps the workspace visible and
    /// drives the read-only UI.
    pub fn enter_remote_view(
        &mut self,
        host: kagi_domain::remote::RemoteHost,
        root: String,
        snap: kagi_git::RepoSnapshot,
        cx: &mut Context<Self>,
    ) {
        let name = root
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("remote")
            .to_string();
        let label = host.label();
        let key = PathBuf::from(format!("{label}:{root}"));
        let rv = super::RemoteRepoView {
            host,
            root: root.clone(),
        };

        // No local path: every `self.repo_path.as_ref()?` operation no-ops, and
        // `arm_watcher` returns early.
        self.repo_path = None;
        self.repo_session = None;
        self.reset_per_repo_ui();
        // Supersede any in-flight local background load.
        self.switch_generation = self.switch_generation.wrapping_add(1);

        // Build + cache the view (so switching back to this tab is instant), then
        // apply it.
        let view = super::build_tab_view(&snap, &name);
        self.tab_cache.insert(key.clone(), view.clone());
        self.apply_tab_view(view);
        self.loading_tab = None;

        // Reuse an existing tab for the same remote repo, else open a new one.
        let idx = match self.tabs.iter().position(|t| t.path == key) {
            Some(i) => {
                self.tabs[i].remote = Some(rv.clone());
                i
            }
            None => {
                self.tabs.push(RepoTab {
                    path: key.clone(),
                    name: name.clone(),
                    remote: Some(rv.clone()),
                    is_worktree: false,
                    wt_color_idx: None,
                });
                self.tabs.len() - 1
            }
        };
        self.active_tab = idx;
        self.remote_view = Some(rv);

        self.status_footer = FooterStatus::Idle(SharedString::from(format!(
            "Remote (read-only) — {label}:{root}"
        )));
        klog!("remote: entered read-only view {label}:{root} (tab {idx})");
        self.save_session();
        self.log_tabs();
        cx.notify();
    }

    /// Re-snapshot the currently-open remote repository over SSH and re-apply it
    /// (ADR-0089 Phase 2b — the remote equivalent of `reload`/refresh). No-op
    /// when no remote view is active.
    pub fn refresh_remote_view(&mut self, cx: &mut Context<Self>) {
        let (host, root) = match &self.remote_view {
            Some(v) => (v.host.clone(), v.root.clone()),
            None => return,
        };
        self.status_footer = FooterStatus::Busy(SharedString::from(format!(
            "Refreshing {}:{root}\u{2026}",
            host.label()
        )));
        cx.notify();

        let (host_load, root_load) = (host.clone(), root.clone());
        let task = cx.background_spawn(async move {
            kagi::remote::remote_snapshot(&host_load, &root_load, 10_000).map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| match result {
                Ok(snap) => app.enter_remote_view(host, root, snap, cx),
                Err(e) => {
                    app.status_footer = FooterStatus::Failed(SharedString::from(e));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Reset all per-repo transient UI state (selection / diffs / modals /
    /// commit panel).  Shared by `switch_repo` (W6-TABSPEED instant-apply path)
    /// so a cached swap never leaks the previous tab's UI.
    fn reset_per_repo_ui(&mut self) {
        self.selected = None;
        self.diff_cache.clear();
        self.file_diff_cache.clear();
        self.remote_diff_inflight.clear();
        self.wip_diffstat = None;
        self.main_diff = None;
        self.clear_plan_modal();
        self.clear_pull_modal();
        self.clear_undo_modal();
        self.clear_pop_modal();
        self.clear_push_modal();
        self.clear_create_branch_modal();
        self.clear_stash_push_modal();
        self.clear_stash_apply_modal();
        self.clear_cherry_pick_modal();
        self.clear_delete_branch_modal();
        self.commit_panel_open = false;
        self.commit_panel = None;
        self.commit_input = None;
        // ADR-0084: drop the previous repo's undo/redo history and re-arm the
        // reflog seed so the next repo seeds its own (else Cmd+Z would target
        // the old repo's branch).
        self.operation_history = kagi_git::OperationHistory::new();
        self.history_seed_attempted = false;
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
            let mut backend =
                kagi_git::Backend::open(&bg_path).map_err(|e| format!("repo open error: {}", e))?;
            let snap = backend
                .snapshot(10_000)
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
                        app.refresh_wip_diffstat();
                        app.loading_tab = None;
                        if matches!(app.status_footer, FooterStatus::Busy(_)) {
                            app.status_footer =
                                FooterStatus::Idle(SharedString::from(Msg::Ready.t()));
                        }
                        klog!("tab-load: {} rows={}", name, rows);
                        cx.notify();
                    }
                    Err(err) => {
                        app.loading_tab = None;
                        let msg = format!("Error: {err}");
                        klog!("tab-load: {} error: {}", name, err);
                        app.status_footer = FooterStatus::Failed(SharedString::from(msg));
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
    /// Persist the open tabs + active index to `settings.json` so a fresh
    /// launch (or a Dock-reopen after the last window closed) restores the
    /// previous session.  Paths are joined with U+001F (unit separator) —
    /// settings.json is kagi-private and read back by the same tolerant
    /// parser, and U+001F cannot appear in a sane path.
    pub fn save_session(&self) {
        // KAGI_NO_RESTORE disables session persistence entirely (both save and
        // restore) so dev/test launches never clobber the user's real session.
        if std::env::var("KAGI_NO_RESTORE").as_deref() == Ok("1") {
            return;
        }
        // Remote tabs (ADR-0089) are ephemeral SSH views with synthetic paths —
        // never persist them (they'd fail to reopen as local repos on restart).
        let joined = self
            .tabs
            .iter()
            .filter(|t| t.remote.is_none())
            .map(|t| t.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\u{1f}");
        if joined.is_empty() {
            super::settings::write_setting("session_repos", None);
            super::settings::write_setting("session_active", None);
        } else {
            super::settings::write_setting("session_repos", Some(&joined));
            super::settings::write_setting("session_active", Some(&self.active_tab.to_string()));
        }
    }

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
            self.repo_session = None;
            // Clear any remote view so the Welcome gate (tabs empty &&
            // remote_view none) actually shows the Welcome screen (ADR-0089).
            self.remote_view = None;
            self.show_welcome();
            self.save_session();
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
        self.active_view.header = SharedString::from("kagi");
        self.active_view.rows = blank.active_view.rows;
        self.active_view.details = blank.active_view.details;
        self.selected = None;
        self.diff_cache.clear();
        self.file_diff_cache.clear();
        self.remote_diff_inflight.clear();
        self.main_diff = None;
        self.active_view.branches = blank.active_view.branches;
        self.active_view.remote_branches = blank.active_view.remote_branches;
        self.active_view.tags = blank.active_view.tags;
        self.active_view.stashes = blank.active_view.stashes;
        self.active_view.is_dirty = false;
        self.active_view.branch_targets = blank.active_view.branch_targets;
        self.active_view.commit_row_index = blank.active_view.commit_row_index;
        self.active_view.branch_upstream_info = blank.active_view.branch_upstream_info;
        self.active_view.branch_solo = blank.active_view.branch_solo;
        self.wip_diffstat = None;
        self.active_view.status_summary = blank.active_view.status_summary;
        self.active_view.toolbar_state = blank.active_view.toolbar_state;
        self.clear_plan_modal();
        self.clear_pull_modal();
        self.clear_undo_modal();
        self.clear_pop_modal();
        self.clear_push_modal();
        self.clear_create_branch_modal();
        self.clear_stash_push_modal();
        self.clear_stash_apply_modal();
        self.clear_cherry_pick_modal();
        self.clear_delete_branch_modal();
        self.commit_panel_open = false;
        self.commit_panel = None;
        self.commit_input = None;
        self.status_footer = FooterStatus::Idle(SharedString::from(Msg::Ready.t()));
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

                use super::watcher::WatchEvent;
                let mut saw_git = false;
                let mut saw_index = false;
                let mut saw_worktree = false;
                match rx.try_recv() {
                    Ok(WatchEvent::Git) => saw_git = true,
                    Ok(WatchEvent::Index) => saw_index = true,
                    Ok(WatchEvent::WorkTree) => saw_worktree = true,
                    Err(_) => continue,
                }

                // Debounce, then drain + coalesce any extra signals.
                Timer::after(super::watcher::DEBOUNCE).await;
                while let Ok(ev) = rx.try_recv() {
                    match ev {
                        WatchEvent::Git => saw_git = true,
                        WatchEvent::Index => saw_index = true,
                        WatchEvent::WorkTree => saw_worktree = true,
                    }
                }

                // Re-check generation after the debounce window — a switch may
                // have happened while we slept. A graph-affecting git change
                // re-snapshots the graph (full reload). An index-only stage/
                // unstage or a working-tree edit does a cheap, in-place WIP +
                // commit-panel refresh that keeps the commit panel OPEN — a full
                // reload would close it (mod.rs `reload()`), so staging a file
                // would bounce the user out of the panel ~`DEBOUNCE` after the
                // click. (During a conflict / continued-merge flow, fall back to
                // the full reload so conflict re-detection still runs.)
                let result = acx.update(|cx| {
                    weak.update(cx, |app, cx| {
                        if app.watcher_generation != generation {
                            return;
                        }
                        if saw_git {
                            app.reload_external(cx);
                        } else if saw_index {
                            if app.conflict.is_some() || app.conflict_merge_commit_pending {
                                app.reload_external(cx);
                            } else {
                                app.refresh_working_tree_external(cx);
                            }
                        } else if saw_worktree {
                            app.refresh_working_tree_external(cx);
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
    // Single-instance listener (ADR-0102)
    // ──────────────────────────────────────────────────────────────────────

    /// Drain the single-instance accept-thread channel on the UI thread.
    ///
    /// Mirrors [`arm_watcher`]'s spawn/weak/update/notify shape: a background
    /// `std::thread` (spawned in `main`) feeds an `mpsc` channel as secondary
    /// `kagi …` invocations forward repo paths; this loop polls the receiver and
    /// opens each path as a new tab (`open_repository`, Backend-backed — no git2
    /// in UI) and raises the window via `cx.activate(true)` (the same call the
    /// Dock-reopen handler uses).  A `None` message is a focus-only request
    /// (bare `kagi`).  Called once from `open_main_window`; a no-op when the
    /// receiver is absent (bind failed, or headless).
    pub fn arm_single_instance_listener(&mut self, cx: &mut Context<Self>) {
        let rx = match crate::single_instance::take_receiver() {
            Some(rx) => rx,
            None => return,
        };

        cx.spawn(async move |weak, acx| {
            use std::sync::mpsc::TryRecvError;
            loop {
                Timer::after(Duration::from_millis(200)).await;
                match rx.try_recv() {
                    Ok(Some(path)) => {
                        let result = acx.update(|cx| {
                            cx.activate(true);
                            weak.update(cx, |app, cx| {
                                klog!("single-instance: open tab {}", path.display());
                                app.open_repository(path.clone(), cx);
                                cx.notify();
                            })
                        });
                        if result.is_err() {
                            break; // app gone
                        }
                    }
                    Ok(None) => {
                        // Focus-only request (bare `kagi`).
                        if acx.update(|cx| cx.activate(true)).is_err() {
                            break;
                        }
                        let _ = weak.update(acx, |_app, cx| {
                            klog!("single-instance: focus");
                            cx.notify();
                        });
                    }
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => break,
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
            .h(theme::scaled_px(TAB_STRIP_H))
            .bg(rgb(theme().panel))
            .border_b_1()
            .border_color(rgb(theme().surface))
            // Themed title bar: the strip now fills the (transparent) OS title-bar
            // area. Drag it to move the window, and on macOS reserve space at the
            // left for the traffic lights drawn over it.
            .window_control_area(gpui::WindowControlArea::Drag)
            .when(cfg!(target_os = "macos"), |s| s.pl(gpui::px(80.)));

        for (i, tab) in tabs.into_iter().enumerate() {
            let is_active = i == active;
            let is_wt = tab.is_worktree;
            // Match the tab colour to the worktree's WIP-row lane colour.
            let wt_color = is_wt.then(|| theme().lane_color(tab.wt_color_idx.unwrap_or(0)));
            let bg = if is_active {
                theme().selected
            } else {
                theme().surface
            };
            let fg = if is_active {
                theme().text_main
            } else {
                theme().text_sub
            };
            let full_path = tab.path.display().to_string();

            // chars()-based truncation is handled by `.truncate()` on the label
            // div (the byte-slice approach panics on multi-byte names). Remote
            // tabs (ADR-0089) get a ☁ marker; worktree tabs get a 🌳 marker so
            // they're distinct from the main repository.
            let label = SharedString::from(if tab.remote.is_some() {
                format!("\u{2601} {}", tab.name) // ☁ name
            } else if is_wt {
                format!("\u{1f333} {}", tab.name) // 🌳 name
            } else {
                tab.name.clone()
            });

            let switch = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
                this.switch_repo(i, cx);
            });
            let close = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
                this.close_tab(i, cx);
            });

            let close_btn = Button::new(("tab-close", i))
                .label("\u{00d7}") // ×
                .ghost()
                .xsmall()
                .ml(theme::scaled_px(4.))
                .on_click(close);

            let tab_el = div()
                .id(("repo-tab", i))
                .flex()
                .flex_row()
                .items_center()
                .h_full()
                .min_w(theme::scaled_px(TAB_MIN_W))
                .max_w(theme::scaled_px(TAB_MAX_W))
                .px_2()
                .gap_1()
                // Worktree tabs are tinted with the SAME lane colour as that
                // worktree's WIP row (user request), washed when inactive.
                .when(!(is_wt && !is_active), |el| el.bg(rgb(bg)))
                .when_some(wt_color.filter(|_| is_wt && !is_active), |el, c| {
                    el.bg(gpui::hsla(c.h, c.s, c.l, 0.20))
                })
                .text_sm()
                .text_color(rgb(fg))
                .border_r_1()
                .border_color(rgb(theme().panel))
                // Top accent: the worktree's lane colour (always), or the normal
                // blue accent for an active main-repo tab.
                .when(is_active && !is_wt, |el| {
                    el.border_t_2().border_color(rgb(theme().color_branch))
                })
                .when_some(wt_color, |el, c| el.border_t_2().border_color(c))
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
        let plus_btn = Button::new("tab-add")
            .label("+")
            .ghost()
            .small()
            .tooltip("Open Repository…")
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
        let remote_click = cx.listener(|this, _: &gpui::ClickEvent, _w, cx| {
            this.open_remote_browse_modal(cx);
        });

        // Primary action: open a local repository (filled accent button).
        let open_button = div()
            .id("welcome-open")
            .px_4()
            .py_2()
            .rounded_md()
            .bg(rgb(theme().selected))
            .text_color(rgb(theme().text_main))
            .text_lg()
            .hover(|s| {
                s.bg(rgb(theme().color_branch))
                    .text_color(rgb(theme().bg_base))
            })
            .cursor(gpui::CursorStyle::PointingHand)
            .child(SharedString::from("Open Repository\u{2026}"))
            .on_click(open_click);

        // Secondary action: connect to a repository over SSH (reuses the
        // existing remote-browse modal — the same flow as file.connectRemote).
        let remote_button = div()
            .id("welcome-remote")
            .px_4()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(theme().selected))
            .text_color(rgb(theme().text_main))
            .text_lg()
            .hover(|s| s.bg(rgb(theme().surface)))
            .cursor(gpui::CursorStyle::PointingHand)
            .child(SharedString::from("Connect to SSH remote\u{2026}"))
            .on_click(remote_click);

        let buttons = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .child(open_button)
            .child(remote_button);

        // Recently-opened repositories (most-recent first; missing paths drop).
        let recent = recent_repos();
        let recent_section = (!recent.is_empty()).then(|| {
            let mut list = div()
                .flex()
                .flex_col()
                .gap_1()
                .w(px(420.))
                .max_w(px(420.))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .pb_1()
                        .child(SharedString::from("Recent")),
                );
            for path in recent.into_iter().take(8) {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                let dir = path.to_string_lossy().into_owned();
                let p = path.clone();
                let click = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
                    this.open_repository(p.clone(), cx);
                });
                list = list.child(
                    div()
                        .id(SharedString::from(format!("recent-{dir}")))
                        .flex()
                        .flex_col()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .cursor(gpui::CursorStyle::PointingHand)
                        .hover(|s| s.bg(rgb(theme().surface)))
                        .on_click(click)
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(name)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().text_muted))
                                .truncate()
                                .child(SharedString::from(dir)),
                        ),
                );
            }
            list
        });

        let welcome = div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .size_full()
            .font_family(super::UI_FONT)
            .bg(rgb(theme().bg_base))
            // Themed transparent title bar leaves no OS drag area, so let the
            // (otherwise empty) welcome surface drag the window; the buttons'
            // own clicks still take precedence.
            .window_control_area(gpui::WindowControlArea::Drag)
            .child(
                div()
                    .text_2xl()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from("kagi")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::NoRepositoryOpenWelcome.t())),
            )
            .child(buttons)
            .when_some(recent_section, |el, list| el.child(list));

        // The Welcome screen short-circuits the normal render before the modal
        // layer, so overlay the remote-browse modal here when it is open.
        let modal_focus = self.modal_focus.clone();
        welcome
            .when_some(self.remote_browse_modal.clone(), |el, modal| {
                el.child(super::remote_browse::render_remote_browse_modal(
                    modal,
                    modal_focus,
                    cx,
                ))
            })
            .into_any()
    }
}

// ──────────────────────────────────────────────────────────────
// Recent repositories (Welcome screen)
// ──────────────────────────────────────────────────────────────

/// settings.json key holding the recent-repo list (`\u{1f}`-separated paths,
/// most-recent first). Mirrors the `session_repos` encoding.
const RECENT_REPOS_KEY: &str = "recent_repos";
/// How many recent repositories to remember.
const RECENT_REPOS_MAX: usize = 12;

fn recent_repo_strings() -> Vec<String> {
    match super::settings::read_setting(RECENT_REPOS_KEY) {
        Some(s) if !s.is_empty() => s.split('\u{1f}').map(|x| x.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Record `path` as the most-recently-opened repository (deduped, most-recent
/// first, capped at [`RECENT_REPOS_MAX`]). Best-effort; persisted to settings.
pub fn record_recent_repo(path: &Path) {
    let p = path.to_string_lossy().to_string();
    let mut list = recent_repo_strings();
    list.retain(|x| x != &p);
    list.insert(0, p);
    list.truncate(RECENT_REPOS_MAX);
    super::settings::write_setting(RECENT_REPOS_KEY, Some(&list.join("\u{1f}")));
}

/// Recently-opened repositories that still exist on disk (most-recent first).
pub fn recent_repos() -> Vec<PathBuf> {
    recent_repo_strings()
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect()
}

/// Rebuild tabs from the saved session (`session_repos` / `session_active`).
///
/// Pre-window path (no `Context` available) used by a fresh `.app` launch and
/// by the Dock-reopen handler after the last window closed.  Paths that no
/// longer exist or fail to open are skipped silently; with zero valid paths
/// the app stays on the Welcome screen.
pub fn restore_saved_session(app: &mut super::KagiApp) {
    let saved = match super::settings::read_setting("session_repos") {
        Some(s) if !s.is_empty() => s,
        _ => return,
    };
    for raw in saved.split('\u{1f}') {
        let path = PathBuf::from(raw);
        if app.tabs.iter().any(|t| t.path == path) {
            continue;
        }
        match kagi_git::open_repository(&path) {
            Ok(info) => {
                app.tabs.push(RepoTab {
                    path: path.clone(),
                    name: info.name.clone(),
                    remote: None,
                    is_worktree: info.is_worktree,
                    wt_color_idx: None,
                });
            }
            Err(e) => klog!("session: skip {} ({})", path.display(), e),
        }
    }
    if app.tabs.is_empty() {
        return;
    }
    let active = super::settings::read_setting("session_active")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0)
        .min(app.tabs.len() - 1);
    app.active_tab = active;
    app.repo_path = Some(app.tabs[active].path.clone());
    app.repo_session = kagi_git::session::RepoSession::open(&app.tabs[active].path).ok();
    app.error = None;
    app.reload();
    klog!("session: restored {} tab(s)", app.tabs.len());
}
