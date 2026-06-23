# ADR-0119: Code Ecosystem / Hotspot viewer (read-only behavioral-analysis main-pane view)

- Status: Accepted
- Date: 2026-06-23
- Follows: ADR-0117 (FileHistory → `Entity<FileHistoryView>`), ADR-0118 (KagiApp decomposition Phase 5.2),
  `docs/rearch/migration/README.md` S6
- Tickets: `T-ECO-DOMAIN-001`, `T-ECO-GIT-002`, `T-ECO-VIEW-003`, `T-ECO-VIZ-004`, `T-ECO-DIAG-005`

## Context

Kagi already mines Git history for the **Activity** bottom-tab (`aggregate(&snap.commits, …)`,
`crates/kagi-domain/src/activity.rs`). The same history — *who changed what, how often* — is the
raw material for **behavioral code analysis**: locating the small fraction of files that concentrate
change and complexity, i.e. the likely *bug hot-spots* (Tornhill, *Your Code as a Crime Scene*;
Nagappan & Ball 2005 on relative churn; Kim et al. 2007 FixCache). We want a first-class place to
**browse the "code ecosystem"** of a repo — risk-ranked files, change-coupling, and (later) a
zoomable hot-spot map — and to **export that picture as LLM-ready context** ("Copy diagnostic").

This is a *read-only analysis* feature. It deliberately does **not** introduce a write operation, so
the `plan → confirm → preflight → execute → verify → oplog` pipeline (CLAUDE.md invariant 4) does not
apply; it sits alongside the other read-only history miners (`log.rs`, `snapshot.rs`,
`file_history.rs`).

### Surveyed prior art (informs scope, not a dependency)

- **CodeScene / `code-maat`** (Tornhill): hot-spots = *change-frequency × complexity*; enclosure
  diagram (circle packing), X-Ray (function-level churn), change/temporal coupling, knowledge maps.
- **Google "Bug Prediction" (2011)** time-decayed bug-fix score `Σ 1/(1+e^(−12·t_i+12))` — **deliberately
  NOT adopted.** Google itself retired it (ICSE 2013: not actionable, no behavior change), it
  mis-flags healthy high-churn/refactor files, and in an AI-assisted workflow rapid auto-fix churn
  pollutes `t_i` further. We keep scoring to `churn × complexity` and avoid bug-fix heuristics.
- **Aider repo-map**: hot-spot ranking as an *LLM context pre-filter* — the basis for "Copy diagnostic".

### Why now / interaction with Phase 5.2

ADR-0118 is **actively reshaping `KagiApp`** (the avatar/diff-cache clusters at
`src/ui/mod.rs:850-867,1138-1142`, the struct init, and the invalidation sites), and its tickets are
explicitly **sequential because they all edit the same `KagiApp` struct + init regions**. Our one
unavoidable `KagiApp` edit (a single `Option<Entity<…>>` field + its init + reset) must therefore be
**sequenced against** in-flight 5.2 tickets, and placed to avoid the regions 5.2 is rewriting (see
*Conflict-minimization* below). Everything else is **new files**, which do not collide.

The chosen UI mechanism is the **ADR-0117 `Entity<T>` template verbatim**, so this feature *advances*
the S6 god-file decomposition rather than fighting it.

## Decision

Add a **read-only "Code Ecosystem" view**, opened from a toolbar button, rendered **full-screen in the
main pane** (replacing the commit list + right inspector, exactly like `FileHistoryView`), with a
**mode switch** for multiple evaluation axes and a **Copy diagnostic** action.

### Layering (which crate owns what)

| Concern | Location | git2/gpui? | Tested by |
|---|---|---|---|
| Scoring, ranking, normalization, circle-pack **layout**, diagnostic **serialization**, mode enum | `crates/kagi-domain/` | none (pure) | unit tests in-crate |
| Git mining: per-file churn + current LOC + co-change pairs | `crates/kagi-git/` | git CLI (like `file_history.rs`) | `tests/` integration |
| `EcosystemView` entity, async load, canvas paint, toolbar button, clipboard | `src/ui/ecosystem/` | gpui only | `cargo test` + headless `klog` |

Pure logic (scores, layout geometry, the diagnostic string) lives in `kagi-domain` so it is
window-free unit-testable; the UI only paints and wires events. **No `git2::` enters `src/ui/`**
(ADR-0078): the view calls `Backend`, which calls the new `kagi-git` free function.

### Module map + LOC budget (file-splitting is a first-class constraint here)

All targets honour CLAUDE.md (≤ 800 LOC/file, ≤ 80 LOC/fn). New files, one responsibility each:

```
crates/kagi-domain/src/
  hotspot.rs            NEW  ~250  types (FileMetric, EcosystemMode, RiskScore), normalize,
                                   risk = f(churn, complexity), rank, coupling ratio (pure)
  hotspot_layout.rs     NEW  ~200  circle-packing / treemap layout → Vec<PlacedNode{rect|circle}>
                                   (pure geometry; unit-testable, no gpui)
  hotspot_report.rs     NEW  ~150  diagnostic serialization (markdown + json) from ranked metrics
  lib.rs                EDIT  +3   `pub mod hotspot; pub mod hotspot_layout; pub mod hotspot_report;`

crates/kagi-git/src/
  hotspot.rs            NEW  ~220  repo_churn(): `git log --numstat` over whole repo (CLI via
                                   cli::run_git, mirrors file_history.rs), + current-LOC scan;
                                   returns raw EcosystemSnapshot (per-file counts + co-change pairs)
  backend.rs            EDIT  +6   one façade method `ecosystem_snapshot(window, opts)`
  lib.rs                EDIT  +1   `mod hotspot;`

src/ui/ecosystem/
  mod.rs                NEW  ~300  `EcosystemView` entity: state, open/close, mode switch,
                                   async load via own cx.spawn + generation guard (ADR-0117 §3),
                                   WeakEntity<KagiApp> backref (events only, never in Render)
  render.rs             NEW  ~350  impl Render: view header (mode toggle + Copy diagnostic + close),
                                   risk-ranked list rows, loading/empty/error states
  viz.rs                NEW  ~300  canvas() + PathBuilder paint of circle-pack / heatmap
                                   (consumes hotspot_layout output; same primitives as
                                   graph_view.rs:30 / activity_view.rs:18)
  i18n: src/ui/i18n.rs  EDIT  +N   EN + JA Msg variants (Ecosystem, EcoHotspots, EcoCoupling,
                                   EcoCopyDiagnostic, EcoCopied, EcoLoading, EcoEmpty)

src/ui/
  render_header.rs      EDIT  +~15 one make_btn() (template: Push @ :527-536) → open_ecosystem_view
  render_body.rs        EDIT  +~5  one `if let Some(view) = ecosystem_view { … return }` branch
                                   in the priority chain (@ :400-417), adjacent to file_history
  mod.rs (KagiApp)      EDIT  +~3  ONE field `ecosystem_view: Option<Entity<EcosystemView>>`
                                   + init `None` + reset in tabs::reset_per_repo_ui
```

If `render.rs` or `viz.rs` approaches 800 LOC during implementation, split the list-row builder
(`render_row.rs`) or the layout-paint (`viz_paint.rs` vs `viz_legend.rs`) on the obvious seam — do
**not** let a single ecosystem file grow past budget.

### Scoring (domain, pure) — MVP

```
risk(file) = normalize(churn_count) × normalize(complexity)
```
- `churn_count` = number of commits in the selected window that touched the file (from `--numstat`).
- `complexity` = LOC of the current file (proxy; cheapest, language-independent, well-correlated).
  A later ticket may swap in `rust-code-analysis` cyclomatic/cognitive without changing the UI.
- Window = a granularity selector mirroring Activity (`Day/Week/Month/Year/All`).
- **No bug-fix time-decay term** (see Context). Output is presented as "**hot-spots / attention**",
  never as a verdict — Kagi does not accuse code.

### Mechanism — `Entity<T>` per ADR-0117 (the blessed pattern)

1. `KagiApp.ecosystem_view: Option<Entity<EcosystemView>>` (like `file_history`, `mod.rs:1179`).
2. `EcosystemView` is **"fat"**: it holds `repo_path`, drives `Backend::ecosystem_snapshot` on its
   **own** `cx.spawn`, and updates **itself**; a per-entity `generation` counter is bumped per
   (re)load and checked inside the same self-update (ADR-0117 §3) so rapid mode/window switches
   discard stale results.
3. `WeakEntity<KagiApp>` back-ref used **only** in event/listener closures (open→close, row-click →
   `jump_to_commit`/open file history) — **never** in a `Render` read path (ADR-0117 §1 guardrail).
4. Notify scope: mode toggle, hover, scroll → notify the **child only**; close + jump-to-commit →
   notify **`KagiApp`** (parent-owned body/graph state).
5. Closed on repo switch via `tabs::reset_per_repo_ui` (set `ecosystem_view = None`).
6. Behaviour/contract: add `klog!("ecosystem: loaded {n} files")` / `klog!("ecosystem: mode {…}")`
   in the established format for headless coverage (CLAUDE.md logging rules).

### View shell (the "browse" surface the user described)

```
[toolbar … Push  Pull │ 🔬 Ecosystem]            ← read button, set apart from write ops
        ↓ open_ecosystem_view
┌ main pane (full-screen) ──────────────────────────────────────┐
│ [Hotspots │ Coupling │ Ownership]   window:[W▾]   [Copy diag] [×]│
│ ───────────────────────────────────────────────────────────── │
│  risk-ranked list  (file · churn · LOC · risk-bar · sparkline) │   ← MVP
│  (+ circle-pack / heatmap canvas in T-ECO-VIZ-004)             │
└───────────────────────────────────────────────────────────────┘
```

- **MVP mode = Hotspots as a ranked list** (cheapest, proven effect: review-order study
  arXiv:1812.09510). `Coupling` / `Ownership` modes ship as enum variants with a stub panel so the
  switch exists; their data/paint land in follow-ups.
- **Copy diagnostic** serializes the current mode's top-N (path, churn, LOC, risk, top coupling
  partners) to markdown/json (`hotspot_report.rs`) and writes to the clipboard — paste straight into
  an LLM. This is the AI-context-prefilter idea, kept entirely in-app.
- **Visualization is GPUI-native 2D** (`canvas()` + `PathBuilder`, per `graph_view.rs`/
  `activity_view.rs`). **No webview / Three.js / wry** — it breaks the single-binary, native,
  safety-first model. A true 3D code-city, if ever wanted, would use wgpu directly, far later.

### Conflict-minimization vs Phase 5.2 (explicit)

- **Maximize new files, minimize shared edits.** Of the whole feature, only **6 shared files** get a
  small additive hunk; the rest is new modules.
- The single `KagiApp` struct edit is **one field appended next to `file_history`** (`mod.rs:1179`),
  **not** in the avatar (`:1138-1142`) or diff-cache (`:850-867`) clusters ADR-0118 is rewriting —
  different hunks, so no textual collision with 001/002.
- `T-ECO-VIEW-003` (the only `KagiApp`-touching ticket) is **serialized** with in-flight 5.2 tickets:
  land it on its own branch and **rebase after** the current 5.2 ticket merges; do not run it in a
  concurrent worktree against a live 5.2 struct edit.
- `T-ECO-DOMAIN-001` and `T-ECO-GIT-002` touch **zero** `KagiApp`/UI lines → can proceed in parallel
  with 5.2 immediately (only `lib.rs`/`backend.rs` additive `mod`/method lines).
- `render_body.rs` is also touched by 5.2's conflict-entity work; our one branch is added **adjacent
  to the existing `file_history` branch** to keep the hunk small and rebase-friendly.

## Ticket sequence (risk-ordered, one PR each)

1. **T-ECO-DOMAIN-001** — pure scoring + layout + report in `kagi-domain` (`hotspot*.rs`), full unit
   tests. No UI, no git, no `KagiApp`. **Zero 5.2 conflict.**
2. **T-ECO-GIT-002** — `repo_churn`/`ecosystem_snapshot` free fn in `kagi-git` + `Backend` method +
   `tests/` integration on a fixture repo. Additive `backend.rs`/`lib.rs` only. **Zero 5.2 conflict.**
3. **T-ECO-VIEW-003** — `EcosystemView` entity (Hotspots **list** mode), toolbar button,
   `render_body` branch, the one `KagiApp` field, i18n, `klog`. **Sequenced after the current 5.2
   ticket merges; rebased.** Human-UI-verify-pending (subagents can't drive the GUI, CLAUDE.md).
4. **T-ECO-VIZ-004** — circle-pack / heatmap `viz.rs` canvas paint (consumes `hotspot_layout`).
   New file + a render switch in `render.rs`. Human-UI-verify-pending.
5. **T-ECO-DIAG-005** — Copy-diagnostic wiring (clipboard) on top of `hotspot_report.rs`.

Each PR: green on `cargo build` + `cargo test --workspace` + `cargo fmt --check` + no new clippy,
cross-model review (codex), per the ADR-0118 pipeline.

## Implementation status (as built — deltas from the plan above)

- **DOMAIN-001 / GIT-002 / VIEW-003** landed as planned (`kagi_domain::hotspot` + `hotspot_report`,
  `kagi_git::hotspot::repo_ecosystem` + `Backend::ecosystem`, `src/ui/ecosystem/{mod,render}.rs`).
  **Copy diagnostic (DIAG-005) was folded into VIEW-003** rather than a separate ticket.
- **Window reuse:** the granularity selector reuses `activity::Granularity` instead of a new enum.
- **Icon registration gotcha:** gpui-component `IconName` SVGs only render if the app registers them
  in `KagiAssets` (`src/ui/assets.rs`). The Analyze button needed a new `assets/icons/chart-pie.svg`
  **and** an `ASSETS` table entry — a blank icon otherwise. Button label is "**Analyze**" (was
  "Ecosystem"); placed just left of Settings.
- **Loading UX:** a spinning `loader-circle` + indeterminate progress bar + "large repos take a
  minute" hint (the mine reports no increments, so the bar is indeterminate by necessity).
- **Mine cache:** a completed mine is cached on `KagiApp.ecosystem_cache` (keyed by repo path) so
  reopening the view reuses the ~minute-long `git log` scan; invalidated on reload / repo switch.
  Granularity switches re-rank in-memory (no re-mine).
- **Artifact exclusion (user request):** `kagi_domain::hotspot::is_excluded` drops PDFs, raster /
  vector images, CAD/3D models (`step`/`stp`/`stl`/`iges`/`3mf`) and KiCad files (`*.kicad_*`) from
  analysis — enforced in `analyze`/`coupling`/`ownership` and at the git mining boundary.
- **Coupling + Ownership modes shipped** (were stubs): `top_couplings` (Gall-style logical coupling,
  Jaccard degree) and `ownership` (per-file primary author + share + author count, single-owner /
  bus-factor-of-one flagged). Required adding `author` (email) to `CommitChanges` and `%ae` to the
  mine format.
- **T-ECO-VIZ-004 shipped:** Hotspots gains a List ⇄ Map sub-toggle; Map is a GPUI-native **treemap
  heatmap** (tile size = LOC, colour = risk) via a pure binary-split layout in `hotspot_layout.rs`
  (unit-square rects positioned with `relative()` lengths — no canvas, labels stay plain elements).
- **Still pending:** function-level X-Ray (tree-sitter / `rust-code-analysis`); headless `klog`
  assertions; preflight hot-spot warnings; circle-packing (the treemap covers the heatmap need).

## Consequences

- Reuses the ADR-0117 entity precedent, so the feature **adds to** S6 decomposition (a new isolated
  notify-scope) instead of growing the `KagiApp` god-file beyond one field.
- Pure scoring/layout/report in `kagi-domain` are unit-tested without a window; UI stays thin.
- Read-only: no new write op, no destructive command, the safety pipeline is untouched.
- Hot-spots are framed as *attention*, with a window filter and (future) complexity-trend signal, to
  avoid the "healthy churn mis-flag" / warning-fatigue failure mode of naive bug-prediction.
- Out of scope (deferred): Coupling/Ownership data + paint, function-level X-Ray (tree-sitter /
  `rust-code-analysis`), file-tree heat overlay, preflight hot-spot warnings on write ops, secondary
  windows (multi-window is disabled, `commands.rs:756`).

## Rollout

Tickets land sequentially per above; `T-ECO-VIEW-003`+ flagged for **human in-app verification** of
the open/close, mode-switch, window-switch, and stale-load race paths before release. ADR moves
`Proposed → Accepted` once T-ECO-DOMAIN-001 + T-ECO-GIT-002 are green and the view shape is confirmed
in-app.
