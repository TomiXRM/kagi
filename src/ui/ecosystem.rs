//! Bin-side glue for the Code Ecosystem / Analyze pane (ADR-0121 Phase C2).
//!
//! The view itself lives in `crates/kagi-ui-ecosystem` (Git-free). This module
//! keeps the app-owned side: the whole-repo mine (needs `kagi_git::Backend`),
//! the per-repo cache/inflight/generation fields on `KagiApp`, the Operation
//! Log row + completion snackbar, and the [`EcosystemEvent`] subscription that
//! maps the pane's requests (close, toast) onto `KagiApp`.

pub use kagi_ui_ecosystem::*;

use super::*;

impl KagiApp {
    /// Open the full-screen Code Ecosystem view for the current repo and kick
    /// off its async mine. No-op when no repository is open.
    pub fn open_ecosystem_view(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let head = self.active_view.head_oid.clone();
        // Reuse a cached mine only if it reflects the current HEAD (instant
        // reopen, even after switching to another repo tab and back). A cache
        // mined at a different HEAD is stale → drop it so the mine below isn't
        // skipped by `start_ecosystem_mine`'s cache guard.
        let cached = match self.ecosystem_cache.get(&repo_path) {
            Some(c) if c.head == head => Some(c.raw.clone()),
            Some(_) => {
                self.ecosystem_cache.remove(&repo_path);
                None
            }
            None => None,
        };
        let has_cache = cached.is_some();
        let entity = cx.new(|_| {
            let mut v = EcosystemView::new(repo_path.clone());
            if let Some(raw) = cached {
                v.seed(raw); // instant
            } // else: stays in the loading state; the app drives the mine
            v
        });
        // The pane's outward surface: it emits, the app decides (ADR-0121 C2).
        cx.subscribe(&entity, |app, _view, event, cx| match event {
            EcosystemEvent::CloseRequested => {
                app.close_ecosystem_view();
                cx.notify();
            }
            EcosystemEvent::DiagnosticCopied => {
                app.push_toast(ToastKind::Info, Msg::EcoDiagnosticCopied.t(), cx);
            }
        })
        .detach();
        self.ecosystem = Some(entity);
        klog!("ecosystem: opened");
        // No (fresh) cache → start (or join) the app-owned mine, which survives
        // the view being closed and notifies on completion.
        if !has_cache {
            self.start_ecosystem_mine(repo_path, head, cx);
        }
        cx.notify();
    }

    /// Start the whole-repo mine for `repo_path` **on the app** (not the view),
    /// so it keeps running if the user closes the Analyze view, caches the
    /// result, logs to the Operation Log, and shows a completion snackbar
    /// (ADR-0119). Single-flighted per repo; no-op if already mining or cached.
    pub fn start_ecosystem_mine(
        &mut self,
        repo_path: PathBuf,
        head: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.ecosystem_inflight.as_ref() == Some(&repo_path)
            || self.ecosystem_cache.contains_key(&repo_path)
        {
            return;
        }
        self.ecosystem_inflight = Some(repo_path.clone());
        // Stamp this mine with a fresh generation token; the completion handler
        // only accepts the result if this token is still current (guards the
        // same-repo reload race where an older task could otherwise win).
        self.ecosystem_gen += 1;
        let my_gen = self.ecosystem_gen;
        klog!("ecosystem: analyzing {}", repo_path.display());

        let bg_path = repo_path.clone();
        // Exclude patterns (gitignore syntax) from the user's analyze_ignore file.
        let ignore_patterns = super::settings::analyze_ignore_patterns();
        let task = cx.background_spawn(async move {
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
                // Drop the result if this mine was superseded — either the repo
                // reloaded (inflight cleared) or a newer same-repo mine took
                // over (generation bumped). Path alone is not enough: a stale
                // task for the same path must lose to the newer one.
                let still_ours = app.ecosystem_gen == my_gen
                    && app.ecosystem_inflight.as_deref() == Some(repo_path.as_path());
                if still_ours {
                    app.ecosystem_inflight = None;
                }
                if !still_ours {
                    return;
                }
                match result {
                    Ok(raw) => {
                        klog!("ecosystem: loaded {} commits", raw.commits.len());
                        let commits = raw.commits.len();
                        let files = raw.loc.len();
                        app.ecosystem_cache.insert(
                            repo_path.clone(),
                            CachedMine {
                                raw: raw.clone(),
                                head: head.clone(),
                            },
                        );
                        app.record_ecosystem_done(&repo_path, commits, files, cx);
                        // Update the view only if it is still showing this repo.
                        if let Some(view) = app.ecosystem.clone() {
                            view.update(cx, |v, cx| {
                                if v.repo_matches(&repo_path) {
                                    v.seed(raw);
                                    cx.notify();
                                }
                            });
                        }
                    }
                    Err(e) => {
                        klog!("ecosystem: load failed: {}", e);
                        app.push_toast(ToastKind::Error, format!("Analyze failed: {e}"), cx);
                        if let Some(view) = app.ecosystem.clone() {
                            view.update(cx, |v, cx| {
                                if v.repo_matches(&repo_path) {
                                    v.set_error(e.clone());
                                    cx.notify();
                                }
                            });
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Push a completion snackbar + a read-only Operation Log row for a finished
    /// Analyze mine. (Not persisted to the on-disk oplog — it's not a mutation.)
    fn record_ecosystem_done(
        &mut self,
        repo: &std::path::Path,
        commits: usize,
        files: usize,
        cx: &mut Context<Self>,
    ) {
        let summary = format!("{files} files · {commits} commits");
        self.push_toast(
            ToastKind::Success,
            format!("Analyze complete — {summary}"),
            cx,
        );
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
        self.ecosystem = None;
    }
}
