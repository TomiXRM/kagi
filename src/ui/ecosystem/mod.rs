//! Code Ecosystem / hot-spot view (ADR-0119).
//!
//! A **read-only**, full-screen main-pane `Entity<EcosystemView>` that ranks the
//! repository's files by `churn × complexity` (the Hotspots mode) so the
//! maintenance risk is visible at a glance, and exports that picture as
//! LLM-ready text ("Copy diagnostic").
//!
//! Follows the ADR-0117 `Entity<T>` template **verbatim**: the entity is "fat"
//! (it holds `repo_path` and drives the `Backend` mining on its **own**
//! `cx.spawn`, updating *itself*), a `WeakEntity<KagiApp>` back-ref is used only
//! in event closures (never in `Render`), and an atomic `generation` guard
//! discards stale async results. The only parent callback is `close`.
//!
//! The visualization (circle-pack / heatmap) and the Coupling / Ownership modes
//! are stubs here — their paint/data land in later ADR-0119 tickets.

mod render;

use std::path::PathBuf;

use super::*;
use gpui::WeakEntity;
use kagi_domain::activity::Granularity;
use kagi_domain::hotspot::{analyze, Ecosystem, EcosystemMode, RawEcosystem};
use kagi_domain::hotspot_report::{render as render_report, ReportFormat};

/// Commits scanned per load. Generous but bounded so a pathologically large
/// history can't hang the background mine; the granularity windows filter
/// further. `0` would mean unlimited.
const ECOSYSTEM_COMMIT_LIMIT: usize = 10_000;

/// How many top hot-spots the "Copy diagnostic" export includes.
const DIAGNOSTIC_TOP_N: usize = 30;

/// View-model data for the ecosystem view (loaded snapshot, selection, async
/// generation). Separated from the entity so the render path reads plain data.
pub struct EcosystemData {
    /// The mined raw history (kept so a granularity change re-ranks without a
    /// re-mine). `None` until the first load resolves.
    pub raw: Option<RawEcosystem>,
    /// The ranked ecosystem for the current granularity. `None` while loading.
    pub ecosystem: Option<Ecosystem>,
    pub mode: EcosystemMode,
    pub granularity: Granularity,
    pub loading: bool,
    pub error: Option<String>,
    /// Monotonic load generation; bumped per (re)load, checked before applying
    /// an async result so a stale mine is dropped.
    pub generation: u64,
}

impl EcosystemData {
    fn new() -> Self {
        Self {
            raw: None,
            ecosystem: None,
            mode: EcosystemMode::Hotspots,
            granularity: Granularity::All,
            loading: true,
            error: None,
            generation: 0,
        }
    }
}

/// The Code Ecosystem view entity (ADR-0119). "Fat": owns its Backend mining.
pub struct EcosystemView {
    pub(crate) data: EcosystemData,
    /// Back-ref to the app — used ONLY in event closures (close), never in render.
    app: WeakEntity<super::KagiApp>,
    repo_path: PathBuf,
}

impl EcosystemView {
    pub fn new(app: WeakEntity<super::KagiApp>, repo_path: PathBuf) -> Self {
        Self {
            data: EcosystemData::new(),
            app,
            repo_path,
        }
    }

    /// Kick off the async whole-repo mine on a background thread, then re-rank
    /// on the UI thread. Stale results (superseded by a newer load) are dropped
    /// via the generation guard.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.data.generation += 1;
        let generation = self.data.generation;
        self.data.loading = true;
        self.data.error = None;

        let repo_path = self.repo_path.clone();
        let task = cx.background_spawn(async move {
            kagi_git::Backend::open(&repo_path)
                .map_err(|e| e.to_string())
                .and_then(|b| {
                    b.ecosystem(ECOSYSTEM_COMMIT_LIMIT)
                        .map_err(|e| e.to_string())
                })
        });

        cx.spawn(async move |view, acx| {
            let result = task.await;
            let _ = view.update(acx, |v, cx| {
                if v.data.generation != generation {
                    return; // a newer load supersedes this one
                }
                match result {
                    Ok(raw) => {
                        klog!("ecosystem: loaded {} commits", raw.commits.len());
                        v.data.raw = Some(raw);
                        v.recompute();
                    }
                    Err(e) => {
                        klog!("ecosystem: load failed: {}", e);
                        v.data.ecosystem = None;
                        v.data.error = Some(e);
                    }
                }
                v.data.loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-rank the already-mined history for the current granularity (cheap,
    /// pure). No-op until the first mine resolves.
    fn recompute(&mut self) {
        if let Some(raw) = &self.data.raw {
            let now = super::commit_list::now_unix_secs();
            self.data.ecosystem = Some(analyze(raw, now, self.data.granularity));
        }
    }

    pub fn set_mode(&mut self, mode: EcosystemMode, cx: &mut Context<Self>) {
        if self.data.mode != mode {
            self.data.mode = mode;
            cx.notify();
        }
    }

    pub fn set_granularity(&mut self, g: Granularity, cx: &mut Context<Self>) {
        if self.data.granularity != g {
            self.data.granularity = g;
            self.recompute();
            cx.notify();
        }
    }

    /// Copy the current Hotspots ranking as markdown to the clipboard (the
    /// LLM-context "Copy diagnostic").
    pub fn copy_diagnostic(&self, cx: &mut Context<Self>) {
        if let Some(eco) = &self.data.ecosystem {
            let text = render_report(eco, DIAGNOSTIC_TOP_N, ReportFormat::Markdown);
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            klog!(
                "ecosystem: diagnostic copied ({} files)",
                eco.files.len().min(DIAGNOSTIC_TOP_N)
            );
        }
    }

    /// Ask the parent to close this view (drops the entity). Safe per ADR-0117:
    /// the parent callback only clears the field, never re-leases the entity.
    pub fn request_close(&self, cx: &mut Context<Self>) {
        let _ = self.app.update(cx, |app, cx| {
            app.close_ecosystem_view();
            cx.notify();
        });
    }
}

impl Render for EcosystemView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render::render_ecosystem(self, cx)
    }
}

// ── KagiApp entry points (ADR-0119) ─────────────────────────────

impl super::KagiApp {
    /// Open the full-screen Code Ecosystem view for the current repo and kick
    /// off its async mine. No-op when no repository is open.
    pub fn open_ecosystem_view(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let weak = cx.weak_entity();
        let entity = cx.new(|cx| {
            let mut v = EcosystemView::new(weak, repo_path);
            v.load(cx);
            v
        });
        self.ecosystem = Some(entity);
        klog!("ecosystem: opened");
        cx.notify();
    }

    /// Close the Ecosystem view (drops the entity + any in-flight mine).
    pub fn close_ecosystem_view(&mut self) {
        self.ecosystem = None;
    }
}
