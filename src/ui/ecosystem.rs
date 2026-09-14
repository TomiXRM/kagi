//! Bin-side glue for the Code Ecosystem / Analyze pane (ADR-0121 Phase C2).
//!
//! The view itself lives in `crates/kagi-ui-ecosystem` (Git-free). This module
//! keeps the app-owned side: the whole-repo mine (needs `kagi_git::Backend`),
//! session-owned cache/inflight/generation data, the Operation Log row +
//! completion snackbar, and the [`EcosystemEvent`] subscription that maps the
//! pane's requests (close, toast) onto `KagiApp`.

pub use kagi_ui_ecosystem::*;

use super::*;

impl KagiApp {
    /// Open the full-screen Code Ecosystem view for the current repo and kick
    /// off its async mine. No-op when no repository is open.
    pub fn open_ecosystem_view(&mut self, cx: &mut Context<Self>) {
        let (Some(owner), Some(repo_path)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        // File History outranks Analyze in `resolve_workspace`, so opening
        // Analyze underneath it changed nothing on screen and the button read
        // as dead. Close everything Analyze has to displace.
        self.close_file_history();
        self.branch_cleanup_open = false;
        self.pr_mode = None;
        let head = self.view().head_oid.clone();
        // HEAD is the cache key inside this owner. A stale cache is discarded;
        // another session's cache is never consulted.
        if self
            .ui()
            .ecosystem_cache
            .as_ref()
            .is_some_and(|cached| cached.head != head)
        {
            self.ui_mut().ecosystem_cache = None;
        }
        let cached = self
            .ui()
            .ecosystem_cache
            .as_ref()
            .map(|cached| cached.raw.clone());
        let has_cache = cached.is_some();
        let entity = cx.new(|_| {
            let mut v = EcosystemView::new(repo_path.clone());
            if let Some(raw) = cached {
                v.seed(raw); // instant
            } // else: stays in the loading state; the app drives the mine
            v
        });
        // Freeze both the session and entity identity. A retained callback from
        // a departed/replaced pane cannot close or toast on a foreign pane.
        let pane_id = entity.entity_id();
        cx.subscribe(&entity, move |app, _view, event, cx| {
            let is_current = app.active_session() == Some(owner)
                && app
                    .ui()
                    .ecosystem
                    .as_ref()
                    .is_some_and(|current| current.entity_id() == pane_id);
            if !is_current {
                return;
            }
            match event {
                EcosystemEvent::CloseRequested => {
                    app.close_ecosystem_view();
                    cx.notify();
                }
                EcosystemEvent::DiagnosticCopied => {
                    app.push_toast(ToastKind::Info, Msg::EcoDiagnosticCopied.t(), cx);
                }
            }
        })
        .detach();
        self.ui_mut().ecosystem = Some(entity);
        klog!("ecosystem: opened");
        // No (fresh) cache → start (or join) the app-owned mine, which survives
        // the view being closed and notifies on completion.
        if !has_cache {
            self.start_ecosystem_mine(repo_path, head, cx);
        }
        cx.notify();
    }

    /// Mine outside the pane lifetime. Evidence settles into its frozen owner;
    /// window-global presentation is emitted only while that owner is active.
    pub fn start_ecosystem_mine(
        &mut self,
        repo_path: PathBuf,
        head: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.active_session() else {
            return;
        };
        let my_gen = {
            let ui = self.ui_mut();
            if ui
                .ecosystem_cache
                .as_ref()
                .is_some_and(|cached| cached.head == head)
                || (ui.ecosystem_inflight && ui.ecosystem_mine_head == head)
            {
                return;
            }
            ui.ecosystem_inflight = true;
            ui.ecosystem_gen = ui.ecosystem_gen.wrapping_add(1);
            ui.ecosystem_mine_head = head.clone();
            ui.ecosystem_gen
        };
        klog!("ecosystem: analyzing {}", repo_path.display());

        let bg_path = repo_path.clone();
        // Exclude patterns (gitignore syntax) from the user's analyze_ignore file.
        let ignore_patterns = super::settings::analyze_ignore_patterns();
        #[cfg(feature = "gui-e2e")]
        let deferred = super::e2e::take_ecosystem_mine();
        let task = cx.background_spawn(async move {
            #[cfg(feature = "gui-e2e")]
            if let Some(deferred) = deferred {
                return deferred.await;
            }
            kagi_git::Backend::open(&bg_path)
                .map_err(|e| e.to_string())
                .and_then(|b| {
                    b.ecosystem(ECOSYSTEM_COMMIT_LIMIT, ignore_patterns)
                        .map_err(|e| e.to_string())
                })
        });

        cx.spawn(async move |app, acx| {
            let result = task.await;
            let _ = app.update(acx, |app, cx| {
                let active = app.active_session() == Some(owner);
                let Some(ui) = app.ui.get_mut(&owner) else {
                    return;
                };
                if !ui.ecosystem_inflight || ui.ecosystem_gen != my_gen {
                    return;
                }
                ui.ecosystem_inflight = false;
                ui.ecosystem_mine_head = None;
                match result {
                    Ok(raw) => {
                        klog!("ecosystem: loaded {} commits", raw.commits.len());
                        let commits = raw.commits.len();
                        let files = raw.loc.len();
                        let pane = ui
                            .ecosystem
                            .clone()
                            .filter(|view| view.read(cx).repo_matches(&repo_path));
                        // Move into cache; clone only when a live pane also needs the data.
                        let pane_raw = pane.as_ref().map(|_| raw.clone());
                        ui.ecosystem_cache = Some(CachedMine { raw, head });
                        app.record_ecosystem_done(&repo_path, commits, files, active, cx);
                        if let Some((view, raw)) = pane.zip(pane_raw) {
                            view.update(cx, |view, cx| {
                                view.seed(raw);
                                cx.notify();
                            });
                        }
                    }
                    Err(error) => {
                        klog!("ecosystem: load failed: {}", error);
                        if let Some(view) = ui.ecosystem.clone() {
                            view.update(cx, |view, cx| {
                                if view.repo_matches(&repo_path) {
                                    view.set_error(error.clone());
                                    cx.notify();
                                }
                            });
                        }
                        if active {
                            app.push_toast(
                                ToastKind::Error,
                                format!("Analyze failed: {error}"),
                                cx,
                            );
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Revalidate the active session's HEAD-versioned Analyze data.
    ///
    /// A changed HEAD invalidates only this owner. An older in-flight request is
    /// superseded by generation and a replacement is launched only while the
    /// Analyze pane is open.
    pub fn revalidate_ecosystem(&mut self, new_head: Option<String>, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let invalidated = {
            let ui = self.ui_mut();
            let cache_stale = ui
                .ecosystem_cache
                .as_ref()
                .is_some_and(|cached| cached.head != new_head);
            if cache_stale {
                ui.ecosystem_cache = None;
            }
            let flight_stale = ui.ecosystem_inflight && ui.ecosystem_mine_head != new_head;
            if flight_stale {
                ui.ecosystem_inflight = false;
                ui.ecosystem_mine_head = None;
                ui.ecosystem_gen = ui.ecosystem_gen.wrapping_add(1);
            }
            cache_stale || flight_stale
        };
        let pane_open = self
            .ui()
            .ecosystem
            .as_ref()
            .is_some_and(|view| view.read(cx).repo_matches(&repo_path));
        if invalidated && pane_open {
            self.start_ecosystem_mine(repo_path, new_head, cx);
        }
    }

    /// Push a completion snackbar + a read-only Operation Log row for a finished
    /// Analyze mine. (Not persisted to the on-disk oplog — it's not a mutation.)
    fn record_ecosystem_done(
        &mut self,
        repo: &std::path::Path,
        commits: usize,
        files: usize,
        active: bool,
        cx: &mut Context<Self>,
    ) {
        let summary = format!("{files} files · {commits} commits");
        if active {
            self.push_toast(
                ToastKind::Success,
                format!("Analyze complete — {summary}"),
                cx,
            );
        }
        let repo_name = repo
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| repo.display().to_string());
        let before = StateSummary {
            head: repo_name,
            dirty: "read-only".into(),
        };
        let entry = OpLogEntry::new(
            "analyze",
            repo.display().to_string(),
            before,
            OpOutcome::Success {
                after: StateSummary {
                    head: summary,
                    dirty: "read-only".into(),
                },
            },
        );
        if let Some(panel) = self.op_log.clone() {
            panel.update(cx, |panel, cx| {
                panel.push(entry);
                panel.collapse();
                cx.notify();
            });
        }
    }

    /// Close the Ecosystem view (the app-owned mine keeps running if in flight).
    pub fn close_ecosystem_view(&mut self) {
        self.ui_mut().ecosystem = None;
    }
}
