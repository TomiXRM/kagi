//! Code Ecosystem / hot-spot view (ADR-0119).
//!
//! A **read-only**, full-screen main-pane `Entity<EcosystemView>` that ranks the
//! repository's files by `churn × complexity` (the Hotspots mode) so the
//! maintenance risk is visible at a glance, and exports that picture as
//! LLM-ready text ("Copy diagnostic").
//!
//! The `Entity<EcosystemView>` is a **thin reflector**: it renders cached/loading
//! state and owns only its view-local toggles (mode / granularity / list-vs-map).
//! The slow whole-repo mine is **app-owned** (`KagiApp::start_ecosystem_mine` in
//! the bin) so it keeps running if the user closes the view, caches its result
//! per repo, writes an Operation Log row, and shows a completion snackbar. The
//! app seeds the view on completion **only if it still shows the same repo**
//! (`repo_matches`).
//!
//! ADR-0121 Phase C2: this crate is Git-free. Everything the pane needs from
//! the app travels **inward** as data (`seed`/`set_error` with a mined
//! `RawEcosystem`) and **outward** as [`EcosystemEvent`]s the bin subscribes to
//! (close, toast) — no `KagiApp` back-reference.

mod graph;
mod lists;
mod mermaid_url;
mod render;
mod viz;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::prelude::*;
use gpui::{div, px, rgb, ClipboardItem, Context, EventEmitter, Render, SharedString, Window};
use kagi_domain::activity::Granularity;
use kagi_domain::coupling_graph::{build_graph, CouplingGraph};
use kagi_domain::hotspot::{
    analyze, coupling_for, ownership, top_couplings, CouplingEdge, CouplingPair, Ecosystem,
    EcosystemMode, FileOwnership, RawEcosystem,
};
use kagi_domain::hotspot_report::{
    render as render_report, render_coupling_mermaid, render_couplings, render_ownership,
    ReportFormat,
};
use kagi_ui_core::i18n::Msg;
use kagi_ui_core::klog;
use kagi_ui_core::theme::{self, theme};

/// Commits scanned per load. Generous but bounded so a pathologically large
/// history can't hang the background mine; the granularity windows filter
/// further. `0` would mean unlimited.
pub const ECOSYSTEM_COMMIT_LIMIT: usize = 10_000;

/// How many top hot-spots the "Copy diagnostic" export includes.
const DIAGNOSTIC_TOP_N: usize = 30;

/// Output format for the "Copy diagnostic" export. Markdown and JSON apply to
/// every mode; Mermaid is a graph diagram that only makes sense for Coupling
/// (where the 1:many co-change structure is the signal), so it is offered only
/// in that mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Json,
    Mermaid,
}

impl ExportFormat {
    /// Short toggle-chip label.
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Markdown => "MD",
            ExportFormat::Json => "JSON",
            ExportFormat::Mermaid => "Mermaid",
        }
    }
}

/// Coupling sub-view: the pair list, the native force-directed graph, or the
/// Mermaid source (with a one-click "open in mermaid.live").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CouplingView {
    List,
    Graph,
    Mermaid,
}

/// How many change-coupling pairs the Coupling mode lists.
const COUPLING_TOP_N: usize = 100;

/// How many partners the expanded (1:many) coupling row shows for a file.
const COUPLING_PARTNERS_N: usize = 50;

/// How many top pairs feed the Coupling graph (keeps it legible + fast).
const GRAPH_MAX_EDGES: usize = 60;

/// How many files the Ownership mode lists (single-owner / high-share first).
const OWNERSHIP_TOP_N: usize = 200;

/// View-model data for the ecosystem view (mined snapshot + per-mode rankings +
/// view-local toggles). Separated from the entity so render reads plain data.
pub struct EcosystemData {
    /// The mined raw history (kept so a granularity change re-ranks without a
    /// re-mine). `None` until the first load resolves.
    pub raw: Option<RawEcosystem>,
    /// The ranked ecosystem for the current granularity. `None` while loading.
    pub ecosystem: Option<Ecosystem>,
    /// Top change-coupling pairs for the current granularity (Coupling mode).
    pub couplings: Vec<CouplingPair>,
    /// The expanded coupling row (index into `couplings`), if any, and that
    /// row's left file's full 1:many partner list.
    pub coupling_focus: Option<usize>,
    pub coupling_partners: Vec<CouplingEdge>,
    /// Coupling sub-view: pair list / force-directed graph / Mermaid source.
    pub coupling_view: CouplingView,
    /// Laid-out graph for the current window (built lazily when Graph is shown).
    pub coupling_graph: Option<CouplingGraph>,
    /// Zoom / pan / drag / painted-bounds for the Coupling graph (see
    /// [`graph::GraphViewport`]).
    pub viewport: graph::GraphViewport,
    /// Per-file ownership for the current granularity (Ownership mode).
    pub ownership: Vec<FileOwnership>,
    pub mode: EcosystemMode,
    /// Selected "Copy diagnostic" output format (Markdown / JSON / Mermaid).
    pub export_format: ExportFormat,
    /// Whether the "How to read Analyze" help overlay is open.
    pub help_open: bool,
    /// Hotspots sub-view: `false` = ranked list, `true` = treemap heatmap.
    pub map: bool,
    pub granularity: Granularity,
    pub loading: bool,
    pub error: Option<String>,
}

impl EcosystemData {
    fn new() -> Self {
        Self {
            raw: None,
            ecosystem: None,
            couplings: Vec::new(),
            coupling_focus: None,
            coupling_partners: Vec::new(),
            coupling_view: CouplingView::List,
            coupling_graph: None,
            viewport: graph::GraphViewport::new(),
            ownership: Vec::new(),
            mode: EcosystemMode::Hotspots,
            export_format: ExportFormat::Markdown,
            help_open: false,
            map: false,
            granularity: Granularity::All,
            loading: true,
            error: None,
        }
    }
}

/// A completed mine plus the HEAD commit it reflects. The mine is `git log`
/// (HEAD history) + a working-tree LOC scan, so its staleness is keyed on the
/// HEAD OID: a reload whose HEAD is unchanged (e.g. an auto-fetch that only
/// moved remote-tracking refs) leaves the mine valid and must NOT discard it.
#[derive(Clone)]
pub struct CachedMine {
    pub raw: RawEcosystem,
    /// HEAD OID the mine was started at (`None` for an unborn HEAD).
    pub head: Option<String>,
}

/// App-level cache of completed mines, keyed by repository path, so reopening
/// the view — or switching tabs to another repo and **back** — reuses the
/// ~minute-long `git log` scan instead of re-running it. Entries persist across
/// tab switches; an entry is invalidated only when its repo's HEAD actually
/// moves (see [`CachedMine`]). (ADR-0119)
pub type EcosystemCache = HashMap<PathBuf, CachedMine>;

/// What the pane asks of its host (ADR-0121 C2). The bin subscribes with
/// `cx.subscribe` and maps these onto `KagiApp` — the only outward coupling.
pub enum EcosystemEvent {
    /// The user clicked ✕ — the host should drop the entity (ADR-0117: the
    /// handler only clears its field, never re-leases the entity).
    CloseRequested,
    /// "Copy diagnostic" wrote the clipboard — the host should confirm with a
    /// toast (the write itself is done here; only the snackbar is app-owned).
    DiagnosticCopied,
}

/// The Code Ecosystem view entity (ADR-0119). A thin reflector of cached /
/// loading state; the mine itself is app-owned (`start_ecosystem_mine`).
pub struct EcosystemView {
    pub(crate) data: EcosystemData,
    repo_path: PathBuf,
}

impl EventEmitter<EcosystemEvent> for EcosystemView {}

impl EcosystemView {
    pub fn new(repo_path: PathBuf) -> Self {
        Self {
            data: EcosystemData::new(),
            repo_path,
        }
    }

    /// Seed from a completed mine (instant; no Backend work) and rank.
    pub fn seed(&mut self, raw: RawEcosystem) {
        self.data.raw = Some(raw);
        self.data.loading = false;
        self.data.error = None;
        self.recompute();
    }

    /// Show a mine error in the body (clears the loading spinner).
    pub fn set_error(&mut self, error: String) {
        self.data.ecosystem = None;
        self.data.loading = false;
        self.data.error = Some(error);
    }

    /// True when this view belongs to `repo` — guards an app-driven seed so a
    /// completed mine never lands on a view that has since switched repos.
    pub fn repo_matches(&self, repo: &std::path::Path) -> bool {
        self.repo_path == repo
    }

    /// Re-rank the already-mined history for the current granularity (cheap,
    /// pure). No-op until the first mine resolves.
    fn recompute(&mut self) {
        // The pair list changes with the window, so any expanded row is stale.
        self.data.coupling_focus = None;
        self.data.coupling_partners.clear();
        if let Some(raw) = &self.data.raw {
            let now = now_unix_secs();
            let g = self.data.granularity;
            self.data.ecosystem = Some(analyze(raw, now, g));
            self.data.couplings = top_couplings(raw, now, g, COUPLING_TOP_N);
            self.data.ownership = ownership(raw, now, g, OWNERSHIP_TOP_N);
        }
        // Rebuild the graph only if it is currently being shown.
        self.data.coupling_graph = (self.data.coupling_view == CouplingView::Graph)
            .then(|| build_graph(&self.data.couplings, GRAPH_MAX_EDGES));
        self.data.viewport.reset();
    }

    /// Zoom the graph by a scroll delta, anchored on the cursor (window px).
    pub fn graph_zoom_by(&mut self, dy: f32, cursor: (f32, f32), cx: &mut Context<Self>) {
        if self.data.viewport.zoom_by(dy, cursor) {
            cx.notify();
        }
    }

    /// Begin a pan drag at the given window pixel position.
    pub fn graph_drag_start(&mut self, x: f32, y: f32) {
        self.data.viewport.drag_start(x, y);
    }

    /// Continue a pan drag — translate the viewport by the pointer delta.
    pub fn graph_drag_move(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        if self.data.viewport.drag_move(x, y) {
            cx.notify();
        }
    }

    pub fn graph_drag_end(&mut self) {
        self.data.viewport.drag_end();
    }

    /// Reset zoom + pan to the default fit.
    pub fn graph_reset(&mut self, cx: &mut Context<Self>) {
        self.data.viewport.reset();
        cx.notify();
    }

    /// Switch the Coupling sub-view (List / Graph / Mermaid); builds the
    /// (force-directed) graph layout lazily the first time Graph is shown.
    pub fn set_coupling_view(&mut self, v: CouplingView, cx: &mut Context<Self>) {
        if self.data.coupling_view != v {
            self.data.coupling_view = v;
            if v == CouplingView::Graph && self.data.coupling_graph.is_none() {
                self.data.coupling_graph = Some(build_graph(&self.data.couplings, GRAPH_MAX_EDGES));
                self.data.viewport.reset();
            }
            cx.notify();
        }
    }

    /// The current Coupling pairs as a Mermaid flowchart (shared by the Mermaid
    /// sub-view, the export, and the "open in mermaid.live" link).
    pub fn coupling_mermaid_source(&self) -> String {
        let window = self
            .data
            .ecosystem
            .as_ref()
            .map(|e| e.granularity.window_label())
            .unwrap_or("");
        render_coupling_mermaid(&self.data.couplings, window)
    }

    /// Open the current coupling diagram in the mermaid.live editor (renders in
    /// the browser). The whole diagram travels in the URL fragment as URL-safe
    /// base64 of the editor's state JSON — mermaid.live decodes it client-side
    /// (the fragment is never sent to a server), so this leaks nothing.
    pub fn open_in_mermaid_live(&self, cx: &mut Context<Self>) {
        let url = mermaid_url::mermaid_live_url(&self.coupling_mermaid_source());
        cx.open_url(&url);
    }

    /// Toggle the 1:many expansion of a Coupling row: show `focus_file`'s full
    /// set of co-change partners beneath it (or collapse if already open).
    pub fn toggle_coupling(&mut self, row: usize, focus_file: String, cx: &mut Context<Self>) {
        if self.data.coupling_focus == Some(row) {
            self.data.coupling_focus = None;
            self.data.coupling_partners.clear();
        } else if let Some(raw) = &self.data.raw {
            let now = now_unix_secs();
            self.data.coupling_partners = coupling_for(
                raw,
                &focus_file,
                now,
                self.data.granularity,
                COUPLING_PARTNERS_N,
            );
            self.data.coupling_focus = Some(row);
        }
        cx.notify();
    }

    /// Toggle the "How to read Analyze" help overlay.
    pub fn toggle_help(&mut self, cx: &mut Context<Self>) {
        self.data.help_open = !self.data.help_open;
        cx.notify();
    }

    pub fn set_mode(&mut self, mode: EcosystemMode, cx: &mut Context<Self>) {
        if self.data.mode != mode {
            self.data.mode = mode;
            // Mermaid only exists for Coupling; leaving that mode falls back to a
            // format that every mode supports so the toggle never shows a stale
            // selection that the new mode can't render.
            if mode != EcosystemMode::Coupling && self.data.export_format == ExportFormat::Mermaid {
                self.data.export_format = ExportFormat::Markdown;
            }
            cx.notify();
        }
    }

    /// Set the "Copy diagnostic" output format (Markdown / JSON / Mermaid).
    pub fn set_export_format(&mut self, fmt: ExportFormat, cx: &mut Context<Self>) {
        if self.data.export_format != fmt {
            self.data.export_format = fmt;
            cx.notify();
        }
    }

    /// Toggle the Hotspots sub-view between the ranked list and the heatmap.
    pub fn set_map(&mut self, map: bool, cx: &mut Context<Self>) {
        if self.data.map != map {
            self.data.map = map;
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

    /// Copy the **current mode's** ranking as markdown to the clipboard (the
    /// LLM-context "Copy diagnostic"). Hotspots exports the risk ranking,
    /// Coupling the change-coupling pairs (which files move together), and
    /// Ownership the per-file bus-factor — so the relationships of each view are
    /// exportable, not just Hotspots.
    pub fn copy_diagnostic(&self, cx: &mut Context<Self>) {
        let Some(eco) = &self.data.ecosystem else {
            return;
        };
        let window = eco.granularity.window_label();
        // Markdown / JSON share a tabular path; Mermaid is a Coupling-only graph.
        let tabular = match self.data.export_format {
            ExportFormat::Json => ReportFormat::Json,
            _ => ReportFormat::Markdown,
        };
        let (text, what) = match self.data.mode {
            EcosystemMode::Hotspots => (render_report(eco, DIAGNOSTIC_TOP_N, tabular), "hotspots"),
            EcosystemMode::Coupling => {
                let text = if self.data.export_format == ExportFormat::Mermaid {
                    render_coupling_mermaid(&self.data.couplings, window)
                } else {
                    render_couplings(
                        &self.data.couplings,
                        window,
                        self.data.couplings.len(),
                        tabular,
                    )
                };
                (text, "coupling")
            }
            EcosystemMode::Ownership => (
                render_ownership(
                    &self.data.ownership,
                    window,
                    self.data.ownership.len(),
                    tabular,
                ),
                "ownership",
            ),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        klog!(
            "ecosystem: diagnostic copied ({what} {})",
            self.data.export_format.label()
        );
        // Confirm the (otherwise invisible) clipboard write with a toast
        // (host-owned; see [`EcosystemEvent`]).
        cx.emit(EcosystemEvent::DiagnosticCopied);
    }

    /// Ask the parent to close this view (drops the entity). Safe per ADR-0117:
    /// the parent callback only clears the field, never re-leases the entity.
    pub fn request_close(&self, cx: &mut Context<Self>) {
        cx.emit(EcosystemEvent::CloseRequested);
    }
}

/// Seconds since the Unix epoch. (Same as the bin's `commit_list::now_unix_secs`;
/// duplicated here so the crate stays free of bin imports — 5 lines of std.)
fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Render for EcosystemView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render::render_ecosystem(self, cx)
    }
}
