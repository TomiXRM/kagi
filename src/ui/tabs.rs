//! W4-TABS: Repository tab strip + directory picker + Welcome screen.
//!
//! All multi-repository logic is intentionally collected here (a parallel lane
//! is editing `src/ui/mod.rs`, so the surface touched there is kept minimal —
//! see the W4-TABS completion report for the exact list of mod.rs changes).
//!
//! Model (ADR-0027 / ADR-0197): lightweight [`RepoTab`] descriptors plus
//! session-owned read models and pane resources. `switch_repo` changes the active
//! owner, revalidates repository-derived state, and retains that owner's panes.
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

use gpui::{div, prelude::*, px, rgb, Context, PathPromptOptions, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::tooltip::Tooltip;
use gpui_component::Sizable as _;
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::i18n::{self, Msg};
use super::theme::{self, theme};
use super::{EditorPendingIntent, FooterStatus, KagiApp, ToastKind};

mod overflow;
mod strip;

/// Lightweight descriptor for one open repository tab (ADR-0027).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoTab {
    /// #482 stage 1: the application-layer session this tab is attached to.
    /// Issued once per open by `Sessions::attach`, so a closed-and-reopened path
    /// is a different owner even when it lands on the same strip index.
    pub session: crate::app::SessionId,
    /// Absolute path to the repository working tree root. For a **remote** tab
    /// (ADR-0089 Phase 2b) this is a synthetic identity key (`<host>:<root>`),
    /// not a local path — `remote` is `Some` in that case.
    pub path: PathBuf,
    /// Display name (working-tree directory name).
    pub name: String,
    /// `Some` for a remote read-only repository opened over SSH; `None` for a
    /// normal local repository.
    pub remote: Option<super::RemoteRepoView>,
    /// True when this tab is a linked git worktree (shown with a 🌲 marker so
    /// it is not mistaken for the main repository).
    pub is_worktree: bool,
}

/// Fixed logical titlebar height, independent of UI zoom and shared density.
/// Native macOS traffic lights remain at (9,9); document labels cap at 14px.
pub(crate) const TAB_STRIP_H: f32 = 36.0;
/// Minimum / maximum width of a single tab (truncate beyond max).
const TAB_MIN_W: f32 = 80.0;
const TAB_MAX_W: f32 = 200.0;

/// Document tabs use ordinary text on subtle selection fills, not accent-selected rows.
fn tab_colors(palette: &theme::Theme, selected: bool) -> (gpui::Hsla, u32) {
    match palette.slug.as_ref() {
        "apple-light" | "nagi-light" => (
            rgb(if selected {
                palette.bg_base
            } else {
                palette.surface
            })
            .into(),
            palette.text_main,
        ),
        "apple-dark" | "nagi-dark" => (
            rgb(if selected {
                palette.surface
            } else {
                palette.bg_base
            })
            .into(),
            palette.text_main,
        ),
        _ => (
            if selected {
                palette.segment_selected_bg()
            } else {
                rgb(palette.surface).into()
            },
            if selected {
                palette.text_main
            } else {
                palette.text_sub
            },
        ),
    }
}

fn tab_has_rail(palette: &theme::Theme) -> bool {
    !matches!(
        palette.slug.as_ref(),
        "apple-light" | "apple-dark" | "nagi-light" | "nagi-dark"
    )
}

fn tab_rail(palette: &theme::Theme, window_active: bool) -> gpui::Div {
    let ring = super::keyboard_nav::RING;
    div()
        .absolute()
        .top(px(-ring))
        .left(px(-ring))
        .right(px(-ring))
        .h(px(2.))
        .bg(rgb(if window_active {
            palette.ui_accent()
        } else {
            palette.text_muted
        }))
}

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
            record_recent_repo(&path);
            cx.add_recent_document(&path);
            super::commands::publish_menus(cx);
            let left = self.home_yields_to_repository();
            self.return_to_tab(idx, left, cx);
            return true;
        }

        // Validate the repository before creating a tab.
        let info = match open_repository(&path) {
            Ok(info) => info,
            Err(e) => {
                self.present_repository_open_failure(e, cx);
                return false;
            }
        };

        // Remember it for the Welcome screen's "Recent" list.
        record_recent_repo(&path);
        cx.add_recent_document(&path);
        super::commands::publish_menus(cx);

        // #482: `attach` unifies by the *resolved* `WorktreeId`, so two locators
        // for one worktree (`/repo` and `/repo/.git`, a symlink) come back as the
        // session an existing tab already holds. Path comparison above cannot
        // see that, so switch to that tab rather than opening a second one.
        let session = self.attach_session(path.clone());
        if let Some(idx) = self.tabs.iter().position(|t| t.session == session) {
            let left = self.home_yields_to_repository();
            self.return_to_tab(idx, left, cx);
            return true;
        }

        let tab = RepoTab {
            session,
            path: path.clone(),
            name: info.name.clone(),
            remote: None,
            is_worktree: info.is_worktree,
        };
        self.tabs.push(tab);
        let new_idx = self.tabs.len() - 1;
        let left = self.home_yields_to_repository();
        self.return_to_tab(new_idx, left, cx);
        true
    }

    /// Finder only opens folders; never discover a repository from a file's parent.
    pub(super) fn open_external_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for path in paths {
            if path.is_dir() {
                self.open_repository(path.clone(), cx);
            } else {
                self.present_repository_open_failure(
                    kagi_git::GitError::NotARepository(path.display().to_string()),
                    cx,
                );
            }
        }
    }

    fn present_repository_open_failure(&mut self, e: kagi_git::GitError, cx: &mut Context<Self>) {
        let msg = format!("Error: {e}");
        klog!("open: {}", msg);
        self.status_footer = FooterStatus::Failed(SharedString::from(msg.clone()));
        self.push_toast(ToastKind::Error, msg, cx);
        cx.notify();
    }

    /// The application-layer session on screen (#482 stage 1). `None` on the
    /// Welcome screen. This is the **only** owner test for app-family plans and
    /// deliveries: a path cannot distinguish a reopened tab from its old session.
    pub fn active_session(&self) -> Option<crate::app::SessionId> {
        self.tabs.get(self.active_tab).map(|tab| tab.session)
    }

    /// The user is leaving the tab that is on screen (#482). The tab stays open
    /// and keeps its incarnation, but the visit ends: a pending stash follow-up
    /// proposal is discarded and a completion landing afterwards cannot create a
    /// new one. Returning re-observes the real conflict state instead.
    pub(crate) fn depart_active_tab(&mut self, cx: &mut Context<Self>) {
        self.close_departing_screen_overlays();
        if let Some(conflict) = self.ui().conflict.clone() {
            conflict.update(cx, |view, _| view.retire_raw_preview());
        }
        if let Some(session) = self.active_session() {
            super::diff_selection::retire_session(session);
            self.app_sessions.depart(session);
            if self
                .file_menu
                .as_ref()
                .is_some_and(|menu| menu.owner == session)
            {
                self.file_menu = None;
            }
            if let Some(ui) = self.ui.get_mut(&session) {
                ui.retire_issue_conversation_scope();
                ui.worktree_inspections.cancel();
                if ui.pr_mode.is_some() {
                    ui.invalidate_pr_list_visit();
                }
            }
        }
        self.close_window_slots_of_departing_tab();
    }

    /// Identity of the repository currently on screen, in tab-path terms:
    /// `repo_path` for a local tab, the synthetic `<host>:<root>` key for a
    /// remote one (ADR-0089). `None` on the Welcome screen. Used to tell a
    /// genuine tab switch from re-selecting the live tab (#488).
    fn live_tab_path(&self) -> Option<PathBuf> {
        match &self.remote_view {
            Some(v) => Some(PathBuf::from(format!("{}:{}", v.host.label(), v.root))),
            None => self.repo_path.clone(),
        }
    }

    /// Switch the active tab to `index` (W6-TABSPEED / ADR-0030).
    /// Session-owned reads, presentation state, panes, and resources become
    /// active immediately; repository-derived state is then revalidated in the
    /// background and the watcher is re-armed (ADR-0197).
    pub fn switch_repo(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        // #488: re-selecting the tab that is already on screen is a no-op. A
        // reset here would drop selection, undo history and the owner's visit.
        if index == self.active_tab && self.live_tab_path().as_ref() == Some(&tab.path) {
            return;
        }
        self.enter_tab(index, cx);
    }

    /// Make tab `index` the one on screen and begin its visit: the body of a
    /// tab switch, also used to come back to the tab Home was covering (whose
    /// visit Home ended, so returning is not the #488 no-op).
    pub(crate) fn enter_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let tab = match self.tabs.get(index) {
            Some(t) => t.clone(),
            None => return,
        };
        // ADR-0197 決定 3: a plain tab switch retains the departing owner's
        // editor and its unsaved buffer, so it must not be gated on dirtiness.
        // The dirty guard stays only on owner-destroying paths (close / reopen).
        // A different session's Inspector cannot finish this tab's slide.
        self.panel_motion.reset_right();
        self.depart_active_tab(cx);
        self.active_tab = index;
        // Re-enter the retained final list without replaying another visit's
        // presentation; neither row membership nor scroll position changes.
        self.ui().inspector_model.keyboard.settle_motion();
        let panel = self.ui().commit_panel.clone();
        if let Some(panel) = panel {
            panel.update(cx, |panel, _| panel.settle_file_motion());
        }
        self.error = None;
        if let Some(editor) = self.ui().editor_workspace.clone() {
            editor.update(cx, |editor, _| editor.settle_tree_motion());
        }

        // ── Remote tab (ADR-0089 Phase 2b): re-enter the read-only view from
        //    the cached snapshot. No local path, no Backend, no watcher. ──
        if let Some(rv) = tab.remote.clone() {
            self.repo_path = None;
            if let Some(ui) = self.ui_mut() {
                ui.repo_session = None;
            }
            self.remote_view = Some(rv);
            self.begin_session_revalidation(tab.session);
            self.queue_pane_revalidation(tab.session); // no read follows: the snapshot is it
                                                       // #482 stage 2: the remote snapshot belongs to this tab's session
                                                       // and never left it, so there is nothing to copy back. A restored
                                                       // session has no read for it (the SSH snapshot was never persisted)
                                                       // and simply shows the empty view until the user reconnects.
            self.on_view_switched();
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
        // ADR-0107 / ADR-0197: open the owner's repository session once. A
        // returning tab reuses the session it retained while inactive.
        if self.ui().repo_session.is_none() {
            let session = kagi_git::session::RepoSession::open(&tab.path).ok();
            if let Some(ui) = self.ui_mut() {
                ui.repo_session = session;
            }
        }

        self.begin_session_revalidation(tab.session);
        // GitHub Phase 1: refetch PRs for the new repo right away (the
        // ticker alone left the tab at 0 PRs until its next tick).
        self.refresh_github_prs(cx);
        // A selected Closed/All first read interrupted by departure has no
        // authoritative empty answer. Reauthorize that retained intent for
        // this visit through its ordinary owner-bound read.
        if self.ui().pr_mode.is_some() && self.ui().github_prs_strip.loading {
            self.refresh_pr_strip(cx);
        }

        // #482 stage 2: "cached" now means "this session already has a read".
        // The swap is the switch itself — `view()` reads a different key — so
        // there is no instant-apply copy to make.
        let cached = self.reads.share(tab.session).is_some();
        eprintln!(
            "[kagi] tab-switch: {} cached={}",
            tab.name,
            if cached { "yes" } else { "no" }
        );

        if !cached {
            // First open: the `Loading <name>…` placeholder is derived from the
            // read this switch is about to start (`loading_tab()`), so only the
            // footer is set here.
            self.status_footer =
                FooterStatus::Busy(SharedString::from(i18n::loading_fmt(&tab.name)));
        }
        self.on_view_switched();

        // Re-arm the watcher for the new repo and repaint immediately so the
        // instant-apply / loading placeholder is visible this frame.
        self.save_session();
        self.log_tabs();
        self.arm_watcher(cx);
        // ADR-0160: a foreign-owned repo opened via git2 is read-only until the
        // user confirms trust — raise the prompt now that per-repo UI is reset.
        self.prompt_trust_if_untrusted();
        cx.notify();

        // T-PERF-RENDER-001 (ADR-0116 Wave 2): tab-switch commit point for the
        // per-repo background I/O (conflict detect + reflog seed + auto-fetch
        // ticker) that used to run synchronously in `render()`. Each sub-task is
        // run-once guarded; activation revalidation re-arms the new owner's
        // conflict/history guards before this call.
        self.ensure_startup_repo_io(cx);

        // Background (re)load to refresh / fill the cache.
        self.load_repo_async(tab.session, tab.path.clone(), tab.name.clone(), cx);

        // #772: a shell may have exited while its owner was in another tab.
        // Only that owner's return may present its pending, confirmed release.
        self.offer_auto_release(tab.session, cx);
    }

    /// Show a **remote** repository (already snapshotted over SSH) in the main
    /// graph/sidebar/detail views, read-only (ADR-0089 Phase 2b).
    ///
    /// Mirrors the local apply path — tab departure + `publish_tab_view` from
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
        // Entering a remote session is a tab transition even when it reuses
        // an existing slot; a previous Inspector exit must not cross it.
        self.panel_motion.reset_right();
        self.depart_active_tab(cx);
        self.repo_path = None;
        // No repo_session write here (P1-a): the departing local owner keeps its
        // retained session, the remote owner attached below has none by default,
        // and ui_mut() would panic when Connect Remote runs from Welcome.

        // #482 stage 2: build the read model first, but publish it only once the
        // tab (and therefore its session) exists — the read belongs to an owner,
        // not to the screen.
        let view = super::build_tab_view(&snap, &name);

        // Reuse an existing tab for the same remote repo, else open a new one.
        let idx = match self.tabs.iter().position(|t| t.path == key) {
            Some(i) => {
                // Same tab slot, fresh session: this snapshot replaces the old
                // read-only view, so nothing the old incarnation owned survives
                // — including its read. Without this the rows/details of every
                // previous refresh stayed keyed under a `SessionId` no tab
                // names any more, and `close_tab` only ever released the
                // current one (#482 stage 2 review, item 4).
                self.tabs[i].session = self.reattach_session(self.tabs[i].session, key.clone());
                self.tabs[i].remote = Some(rv.clone());
                i
            }
            None => {
                let session = self.attach_session(key.clone());
                self.tabs.push(RepoTab {
                    session,
                    path: key.clone(),
                    name: name.clone(),
                    remote: Some(rv.clone()),
                    is_worktree: false,
                });
                self.tabs.len() - 1
            }
        };
        let _ = self.home_yields_to_repository();
        self.active_tab = idx;
        self.remote_view = Some(rv);
        self.publish_tab_view(self.tabs[idx].session, view);

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
        let Some(owner) = self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id))
        else {
            return;
        };
        let key = self.reads.begin(owner.session);
        self.status_footer = FooterStatus::Busy(SharedString::from(format!(
            "Refreshing {}:{root}\u{2026}",
            host.label()
        )));
        cx.notify();
        let (host_load, root_load) = (host.clone(), root.clone());
        let commit_limit = self.ui[&owner.session].commit_limit;
        #[cfg(feature = "gui-e2e")]
        let deferred = super::e2e::take_remote_refresh();
        let task = cx.background_spawn(async move {
            #[cfg(feature = "gui-e2e")]
            if let Some(deferred) = deferred {
                return deferred.await;
            }
            crate::remote::remote_snapshot(&host_load, &root_load, commit_limit)
                .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                // Settle only this request; newer refreshes keep their slot.
                let fresh = app.reads.fail(key);
                let current = app
                    .active_session()
                    .and_then(|id| app.app_sessions.attachment(id));
                if !fresh
                    || !current.is_some_and(|current| {
                        current.session == owner.session && current.visit == owner.visit
                    })
                {
                    return;
                }
                match result {
                    Ok(snap) => app.enter_remote_view(host, root, snap, cx),
                    Err(e) => {
                        app.status_footer = FooterStatus::Failed(SharedString::from(e));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Snapshot + build the [`TabViewState`] on a background thread
    /// (`RepoSnapshot` is `Send`), then hand it to its **owner** on the main
    /// thread (#482 stage 2): the read is bound to `session` and to the read
    /// revision issued here, so a superseded load writes nothing and a load that
    /// finishes after the user moved on still refreshes the tab it belongs to.
    /// Only the *display* half (placeholder, footer, `[kagi] tab-load:` line) is
    /// gated on that tab still being on screen.
    fn load_repo_async(
        &mut self,
        session: crate::app::SessionId,
        path: PathBuf,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let key = self.reads.begin(session);
        let bg_path = path.clone();
        let bg_name = name.clone();
        let commit_limit = self.ui[&session].commit_limit;
        #[cfg(feature = "gui-e2e")]
        super::e2e::record_tab_load_commit_limit(session, commit_limit);
        let read = self.begin_slow_read(session, None, cx);
        #[cfg(feature = "gui-e2e")]
        let hold = super::slow_reads::take_snapshot_hold();
        let task = cx.background_spawn(async move {
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            let mut backend = kagi_git::Backend::open(&bg_path)
                .map_err(|e| i18n::op::op_failed_contract(i18n::Op::RepoOpen, e))?;
            let snap = backend
                .snapshot_repairing_stat_cache(commit_limit, read.probe())
                .map_err(|e| i18n::op::op_failed_contract(i18n::Op::Snapshot, e))?;
            let wip_diffstat = KagiApp::wip_diffstat_from_backend(&backend);
            let status = snap.status.clone();
            let repo_name = bg_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bg_name.clone());
            let view = super::build_tab_view(&snap, &repo_name);
            Ok::<_, String>((view, wip_diffstat, status))
        });

        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                match result {
                    Ok((view, wip_diffstat, status)) => {
                        let rows = view.rows.len();
                        // Superseded (a newer read, or a mutation admitted
                        // against this owner) → write nothing, say nothing.
                        // `accept_tab_view` is one of the three publish seams,
                        // so it — not this call site — owns raising the
                        // revalidation gate and queueing the pane pass (#722).
                        if !app.accept_tab_view(key, view) {
                            return;
                        }
                        if let Some(ui) = app.ui.get_mut(&session) {
                            ui.wip_diffstat = Some(wip_diffstat);
                            ui.wip_diffstat_request = ui.wip_diffstat_request.wrapping_add(1);
                            ui.last_working_status = Some(status);
                        }
                        app.app_sessions.read_applied(session);
                        if app.active_session() != Some(session) {
                            return; // background owner: data only, no display.
                        }
                        app.ensure_worktree_inspections(cx);
                        if matches!(app.status_footer, FooterStatus::Busy(_)) {
                            app.status_footer =
                                FooterStatus::Idle(SharedString::from(Msg::Ready.t()));
                        }
                        klog!("tab-load: {} rows={}", name, rows);
                        cx.notify();
                    }
                    Err(err) => {
                        // Settle the slot so the same read can be asked for
                        // again — a failure must never stick on "Loading…".
                        if !app.reads.fail(key) || app.active_session() != Some(session) {
                            return;
                        }
                        let msg = i18n::op::user_text::diagnostic(&err).into_owned();
                        klog!("tab-load: {} error: {}", name, err);
                        app.status_footer = FooterStatus::Failed(SharedString::from(msg));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Persist open local tabs and the active repository identity so a fresh
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
            let active = self
                .tabs
                .get(self.active_tab)
                .filter(|tab| tab.remote.is_none());
            super::settings::write_setting(
                "session_active",
                active.map(|tab| tab.path.to_string_lossy()).as_deref(),
            );
        }
    }

    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        // #488: the app layer owns which tab is active afterwards, so the
        // "background close" case can be told from the "active close" one.
        let decision = crate::app::close_tab(self.tabs.len(), index, self.active_tab);
        if decision == crate::app::TabClose::Nothing {
            return;
        }
        let closing_session = self.tabs[index].session;
        if self.editor_dirty_for(closing_session, cx)
            || self.failed_conflict_draft_for(closing_session, cx)
        {
            self.open_editor_dirty_guard(EditorPendingIntent::CloseRepoTab(closing_session), cx);
            return;
        }
        let closed = self.tabs.remove(index);
        self.release_session(closed.session);

        match decision {
            crate::app::TabClose::Nothing => unreachable!("filtered above"),
            crate::app::TabClose::Welcome => {
                // Last tab closed → Welcome screen.
                self.close_departing_screen_overlays();
                self.active_tab = 0;
                self.repo_path = None;
                // Clear any remote view so the Welcome gate (tabs empty &&
                // remote_view none) actually shows the Welcome screen (ADR-0089).
                self.remote_view = None;
                // The window slots go as on any departure (`depart_active_tab`;
                // the session itself is already released): a context menu left
                // open would keep its keys and a focus on an item Home never
                // draws (#1000). Home places the focus on the window.
                self.close_window_slots_of_departing_tab();
                self.show_welcome();
                self.home_takes_window();
                self.focus_root_for_modal();
                self.save_session();
                self.log_tabs();
                // Bump generation so the old watcher loop terminates; no new arm.
                self.watcher_generation = self.watcher_generation.wrapping_add(1);
                cx.notify();
            }
            // #488: a background tab went away. Renumber the strip and persist
            // it — nothing else. Touching the live session here would reset the
            // selection/modals/undo history and strand in-flight operations.
            crate::app::TabClose::Keep(new_active) => {
                self.active_tab = new_active;
                self.save_session();
                self.log_tabs();
                cx.notify();
            }
            // The active tab itself is gone. Behind Home, the neighbour only
            // becomes where Home returns to: entering it now would start a
            // visit nobody sees, whose deliveries (a parked Pull confirm)
            // could fill the modal slot that Home's key routing confirms
            // (#930 review). Leaving Home enters it (`return_to_tab`).
            crate::app::TabClose::Activate(new_active) if self.home_in_front() => {
                self.active_tab = new_active;
                let session = self.tabs[new_active].session;
                self.retarget_home(session);
                self.save_session();
                self.log_tabs();
                cx.notify();
            }
            // Otherwise really switch to the neighbour.
            crate::app::TabClose::Activate(new_active) => self.switch_repo(new_active, cx),
        }
    }

    /// #482 stage 1: close the tab that owns `session`. Routed by session rather
    /// than by path so a completion can never close a *different* tab that has
    /// since been opened on the same path.
    pub(crate) fn close_tab_by_session(
        &mut self,
        session: crate::app::SessionId,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tabs.iter().position(|t| t.session == session) {
            self.close_tab(index, cx);
        }
    }

    /// Reset root-owned presentation after the last session is released.
    fn show_welcome(&mut self) {
        self.error = None;
        // With no tab, `view()` and `ui()` expose immutable empty defaults.
        self.view_epoch = self.view_epoch.wrapping_add(1);
        self.drop_repo_scoped_modal();
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
                acx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;

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
                acx.background_executor()
                    .timer(super::watcher::DEBOUNCE)
                    .await;
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
                            if app.ui().conflict.is_some() || app.ui().conflict_merge_pending {
                                app.reload_external(cx);
                            } else {
                                app.refresh_working_tree_external(cx);
                            }
                        } else if saw_worktree {
                            if app.ui().conflict.is_some() {
                                app.reload_external(cx);
                            } else {
                                app.refresh_working_tree_external(cx);
                            }
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
        let owner = self.repository_picker_owner();
        let receiver = cx.prompt_for_paths(Self::repository_picker_options());

        cx.spawn(async move |weak, acx| {
            let response = receiver.await;
            let _ = weak.update(acx, |app, cx| {
                app.complete_repository_picker_response(owner, response, cx);
            });
        })
        .detach();
    }

    pub(crate) fn repository_picker_options() -> PathPromptOptions {
        PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::from(Msg::RepositoryPickerOpen.t())),
        }
    }

    fn complete_repository_picker(
        &mut self,
        owner: Option<PathBuf>,
        result: Result<Option<Vec<PathBuf>>, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(Some(paths)) => {
                if let Some(path) = paths.into_iter().next() {
                    self.open_repository(path, cx);
                }
            }
            Ok(None) => {}
            Err(error) => self.report_path_picker_failure(
                owner,
                "open-repository-picker",
                Msg::RepositoryPickerOpen.t(),
                error,
                cx,
            ),
        }
    }

    fn complete_repository_picker_response<E: std::fmt::Display, C: std::fmt::Display>(
        &mut self,
        owner: Option<PathBuf>,
        response: Result<Result<Option<Vec<PathBuf>>, E>, C>,
        cx: &mut Context<Self>,
    ) {
        let result = response
            .map_err(|error| error.to_string())
            .and_then(|result| result.map_err(|error| error.to_string()));
        self.complete_repository_picker(owner, result, cx);
    }

    #[cfg(feature = "gui-e2e")]
    pub fn repository_picker_options_for_e2e() -> PathPromptOptions {
        Self::repository_picker_options()
    }

    #[cfg(feature = "gui-e2e")]
    pub fn deliver_repository_picker_for_e2e(
        &mut self,
        response: Result<Result<Option<Vec<PathBuf>>, String>, String>,
        cx: &mut Context<Self>,
    ) {
        self.complete_repository_picker_response(self.repository_picker_owner(), response, cx);
    }

    #[cfg(feature = "gui-e2e")]
    pub fn repository_picker_owner_for_e2e(&self) -> Option<PathBuf> {
        self.repository_picker_owner()
    }

    #[cfg(feature = "gui-e2e")]
    pub fn deliver_owned_repository_picker_for_e2e(
        &mut self,
        owner: Option<PathBuf>,
        error: String,
        cx: &mut Context<Self>,
    ) {
        self.complete_repository_picker(owner, Err(error), cx);
    }

    fn repository_picker_owner(&self) -> Option<PathBuf> {
        self.repo_path.clone().filter(|_| !self.home_in_front())
    }

    // ──────────────────────────────────────────────────────────────────────
    // Rendering
    // ──────────────────────────────────────────────────────────────────────

    /// New-document affordance follows document tabs in the titlebar strip.
    pub(super) fn render_tab_add(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = if self.tabs.is_empty() && self.remote_view.is_some() {
            match i18n::lang() {
                i18n::Lang::Ja => {
                    i18n::command_label_ja("file.openRepository").unwrap_or("Open Repository…")
                }
                i18n::Lang::En => "Open Repository…",
            }
        } else {
            Msg::HomeTabTitle.t()
        };
        // Like the named overflow trigger, a glyph-only node cannot take its AX
        // name from gpui-kit's visible label (which would name it merely "+").
        super::e2e::measure_control(
            "tab-add",
            div()
                .id("tab-add")
                .role(gpui::Role::Button)
                .aria_label(label)
                .flex()
                .items_center()
                .justify_center()
                .w(theme::scaled_px(28.))
                .h(px(f32::from(theme::scaled_px(24.)).min(24.)))
                .flex_shrink_0()
                .rounded_md()
                .text_size(px(f32::from(theme::scaled_px(14.)).min(14.)))
                .text_color(rgb(theme().text_main))
                .hover(|style| style.bg(rgb(theme().surface)))
                .cursor_pointer()
                .child("+")
                // Cmd+T / File → New Tab is the keyboard path.
                .on_click(cx.listener(|this, _, window, cx| this.open_home_tab(window, cx)))
                .hover_tooltip_builder("tab-add-hover-help", move |window, cx| {
                    Tooltip::new(label).build(window, cx)
                }),
        )
    }
}

// ──────────────────────────────────────────────────────────────
// Recent repositories (Home)
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

/// Forget only this history entry; repository files and open tabs are untouched.
pub fn remove_recent_repo(path: &Path) {
    let p = path.to_string_lossy();
    let mut list = recent_repo_strings();
    list.retain(|entry| entry != p.as_ref());
    super::settings::write_setting(RECENT_REPOS_KEY, Some(&list.join("\u{1f}")));
}

/// Clear history only; leave open tabs and all repository files untouched.
pub fn clear_recent_repos() {
    super::settings::write_setting(RECENT_REPOS_KEY, None);
}

/// Dock and utility-window dispatch need a repository host even after the last
/// repository window closes. Opening still belongs to the normal tab path.
pub(super) fn open_recent_from_app(path: &Path, cx: &mut gpui::App) {
    let host = super::window_menu::preferred_repository_window(cx)
        .into_iter()
        .chain(cx.windows())
        .find_map(|handle| {
            handle
                .update(cx, |_, window, cx| super::repository_window(window, cx))
                .ok()
                .flatten()
                .map(|host| (handle, host))
        });
    if let Some((handle, host)) = host {
        let _ = handle.update(cx, |_, window, cx| {
            super::window_menu::activate_recent_repository(window, cx);
        });
        host.update(cx, |app, cx| app.open_repository(path.to_path_buf(), cx));
        return;
    }
    super::open_main_window(KagiApp::with_error(""), cx);
    for handle in cx.windows() {
        let handled = handle
            .update(cx, |_, window, cx| {
                let Some(host) = super::repository_window(window, cx) else {
                    return false;
                };
                super::window_menu::activate_recent_repository(window, cx);
                host.update(cx, |app, cx| app.open_repository(path.to_path_buf(), cx));
                true
            })
            .unwrap_or(false);
        if handled {
            break;
        }
    }
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
    // Legacy numeric settings name an entry in the saved set, not in the
    // filtered result. All new saves contain the local repository path itself.
    let active_path = super::settings::read_setting("session_active").and_then(|identity| {
        match identity.parse::<usize>() {
            Ok(index) => saved.split('\u{1f}').nth(index).map(PathBuf::from),
            Err(_) => Some(PathBuf::from(identity)),
        }
    });
    for raw in saved.split('\u{1f}') {
        let path = PathBuf::from(raw);
        if app.tabs.iter().any(|t| t.path == path) {
            continue;
        }
        match kagi_git::open_repository(&path) {
            Ok(info) => {
                let session = app.attach_session(path.clone());
                app.tabs.push(RepoTab {
                    session,
                    path: path.clone(),
                    name: info.name.clone(),
                    remote: None,
                    is_worktree: info.is_worktree,
                });
            }
            Err(e) => klog!("session: skip {} ({})", path.display(), e),
        }
    }
    if app.tabs.is_empty() {
        return;
    }
    // Missing/ephemeral active owners fall back to the first surviving local
    // repository. With no survivors the early return above keeps initial Home.
    let active = active_path
        .and_then(|path| {
            app.tabs
                .iter()
                .position(|tab| tab.remote.is_none() && tab.path == path)
        })
        .unwrap_or(0);
    app.active_tab = active;
    app.repo_path = Some(app.tabs[active].path.clone());
    let session = kagi_git::session::RepoSession::open(&app.tabs[active].path).ok();
    if let Some(ui) = app.ui_mut() {
        ui.repo_session = session;
    }
    app.error = None;
    app.reload_prelaunch();
    app.prompt_trust_if_untrusted(); // ADR-0160: prompt on restore too.
    klog!("session: restored {} tab(s)", app.tabs.len());
}

#[cfg(test)]
mod tab_chrome_tests {
    use super::*;

    fn luminance(color: gpui::Hsla) -> f64 {
        let color: gpui::Rgba = color.into();
        let channel = |value: f32| {
            let value = value as f64;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
    }

    #[test]
    fn apple_tab_selection_contrast_and_preset_roles() {
        for palette in theme::THEMES {
            let (active, active_text) = tab_colors(palette, true);
            let (inactive, inactive_text) = tab_colors(palette, false);
            if !tab_has_rail(palette) {
                let first = luminance(active);
                let second = luminance(inactive);
                assert!(
                    (first.max(second) + 0.05) / (first.min(second) + 0.05) >= 1.15,
                    "{} selected tab must be distinct",
                    palette.slug
                );
                for (fill, text) in [(active, active_text), (inactive, inactive_text)] {
                    let fill = luminance(fill);
                    let text = luminance(rgb(text).into());
                    assert!((fill.max(text) + 0.05) / (fill.min(text) + 0.05) >= 4.5);
                }
            } else {
                assert_eq!(
                    active,
                    palette.segment_selected_bg(),
                    "{} keeps segment selection palette",
                    palette.slug
                );
                assert_eq!(inactive, gpui::Hsla::from(rgb(palette.surface)));
                assert_eq!(
                    (active_text, inactive_text),
                    (palette.text_main, palette.text_sub)
                );
            }
        }
    }
}
