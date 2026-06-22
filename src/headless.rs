//! KAGI_* headless test harness (relocated from main.rs by T-MAINSLIM-001).
//!
//! This module is the env-var-driven, test-only harness used by the fixture /
//! tempdir headless tests that grep stderr.  This is the bin shell (out of the
//! `src/ui/` git2 invariant); do not add new git logic here.
//!
//! ADR-0077 retirement (Phase D / issue #13): the mutating `plan → execute`
//! hooks (`KAGI_PULL` / `PUSH` / `UNDO` / `POP` / `DISCARD` / `AMEND` /
//! `DELETE_BRANCH` / `PLAN_CHECKOUT` / `CHECKOUT_COMMIT` / `CREATE_BRANCH` /
//! `PLAN_WORKTREE` / `STASH_*` / `CHERRY_PICK` / `REVERT` / `COMMIT_PANEL` …)
//! were removed: they duplicated the git-layer integration tests in `tests/`,
//! which now exercise the same `kagi_git::` plan/execute paths directly.  What
//! remains are the read-only UI-state hooks that drive render paths not
//! reachable from the lib layer (`KAGI_OPEN_REPO` / `SELECT_FIRST` / `JUMP` /
//! `CONTEXT_MENU` / `COMPARE_HEAD` / `COMPARE_WT` / `OPEN_FIRST_FILE` /
//! `COMPACT` / `BOTTOM_PANEL` / `TERMINAL` / `MENU_DUMP`).  The surviving
//! `[kagi] …` contract lines are unchanged.

use std::path::PathBuf;

use kagi_git::open_repository;

use crate::ui::{self, run_app, KagiApp};

/// W4-TABS: open `path` as a repository tab on `app` and make it active,
/// rebuilding the heavyweight per-repo state from a fresh snapshot.
///
/// Used by the headless `KAGI_OPEN_REPO` path (main.rs has no gpui context, so
/// it cannot call `KagiApp::switch_repo`, which needs a `Context`).  The GUI
/// picker path uses `KagiApp::open_repository` instead.
///
/// On failure the tab is not added and an error is logged (no panic).
pub(crate) fn init_tab(app: &mut KagiApp, path: &PathBuf) {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());

    // Skip if already open → just switch active index + rebuild.
    if let Some(idx) = app.tabs.iter().position(|t| t.path == path) {
        app.active_tab = idx;
        app.repo_path = Some(path.clone());
        app.reload();
        app.log_tabs();
        return;
    }

    let info = match open_repository(&path) {
        Ok(info) => info,
        Err(e) => {
            klog!("KAGI_OPEN_REPO: open error: {}", e);
            return;
        }
    };

    app.tabs.push(ui::tabs::RepoTab {
        path: path.clone(),
        name: info.name.clone(),
        remote: None,
        is_worktree: info.is_worktree,
        wt_color_idx: None,
    });
    app.active_tab = app.tabs.len() - 1;
    app.repo_path = Some(path.clone());
    // Rebuild the heavyweight per-repo display state from a fresh snapshot.
    app.reload();
    app.log_tabs();
}

/// W4-TABS / ADR-0028: headless hooks for the no-argument (Welcome) launch.
///
/// Relocated verbatim from `main()`'s `args.is_empty()` branch:
/// `KAGI_OPEN_REPO` opens the named repo as the first tab, and `KAGI_MENU_DUMP`
/// dumps the command-registry states.  Runs after session-restore + `log_tabs`,
/// before `run_app`.  `main()` keeps the welcome construction / session restore.
pub fn run_welcome_hooks(welcome: &mut KagiApp, env_open_repo: &Option<PathBuf>) {
    // KAGI_OPEN_REPO: open the named repo as the first tab before launching.
    if let Some(ref env_path) = env_open_repo {
        init_tab(welcome, env_path);
    }
    // W5-MENU: dump command-registry states for headless verification.
    if std::env::var("KAGI_MENU_DUMP").as_deref() == Ok("1") {
        ui::commands::dump_menu_states(welcome);
    }
}

/// Repo-path launch flow: apply every `KAGI_*` headless hook to `app_state`
/// (relocated verbatim from `main()`), then launch the window via `run_app`.
///
/// Takes ownership of `app_state` so the early-exit `run_app(...); return;`
/// paths from the original `main()` are preserved exactly.  `main()` builds the
/// initial single-tab `app_state` from the snapshot and delegates here; this
/// function owns ALL behaviour from the `KAGI_OPEN_REPO` second-tab hook through
/// the final `run_app(app_state)` call.
pub fn run_repo_flow(mut app_state: KagiApp, env_open_repo: Option<PathBuf>) {
    // W4-TABS / ADR-0027: KAGI_OPEN_REPO opens a second tab and switches to it
    // (headless picker substitute).  The sidebar / status-bar logs that follow
    // will reflect the newly-active repo.
    if let Some(ref env_path) = env_open_repo {
        init_tab(&mut app_state, env_path);
    }

    // KAGI_SELECT_FIRST=1: auto-select row 0 at startup for headless
    // verification of the detail panel render path (T010).
    if std::env::var("KAGI_SELECT_FIRST").as_deref() == Ok("1")
        && !app_state.active_view.rows.is_empty()
    {
        app_state.select_headless(0);
    }

    // ── T028: headless branch jump ────────────────────────────
    // KAGI_JUMP=<branch>: simulate a single-click on the named branch.
    // Emits `[kagi] jump: <branch> -> row N` + selected log.
    // Used for fixture/tempdir headless verification only.
    if let Ok(jump_branch) = std::env::var("KAGI_JUMP") {
        app_state.jump_to_branch(&jump_branch);
    }

    // ── T-CM-004: headless commit context menu model ───────────
    // KAGI_CONTEXT_MENU=<row>: simulate right-clicking a commit row and log
    // the pure menu model. Used only with fixture/tempdir repositories.
    if let Ok(row_str) = std::env::var("KAGI_CONTEXT_MENU") {
        match row_str.parse::<usize>() {
            Ok(row) => app_state.open_commit_menu_headless(row),
            Err(_) => klog!("context-menu: invalid row '{}'", row_str),
        }
    }

    // ── ADR-0026: headless compare paths ─────────────────────
    // KAGI_COMPARE_HEAD=<row>: compare the row commit with HEAD.
    // KAGI_COMPARE_WT=<row>: compare the row commit with the working tree
    // (staged + unstaged + untracked). Read-only.
    if let Ok(row_str) = std::env::var("KAGI_COMPARE_HEAD") {
        match row_str.parse::<usize>() {
            Ok(row) => app_state.open_compare_with_head_row(row),
            Err(_) => klog!("compare: invalid row '{}'", row_str),
        }
    }
    if let Ok(row_str) = std::env::var("KAGI_COMPARE_WT") {
        match row_str.parse::<usize>() {
            Ok(row) => app_state.open_compare_with_working_tree_row(row),
            Err(_) => klog!("compare: invalid row '{}'", row_str),
        }
    }

    // KAGI_OPEN_FIRST_FILE=1 (requires KAGI_SELECT_FIRST=1): after selecting
    // the first commit, automatically open the diff for its first changed file
    // in the full-width main pane (T-UI-003).
    // Emits `[kagi] diff: <path> hunks=N (+A -R)` (legacy compat) +
    // `[kagi] main-diff: open <path> rows=N` for headless verification.
    if std::env::var("KAGI_OPEN_FIRST_FILE").as_deref() == Ok("1") {
        app_state.open_main_diff_commit_headless(0);
    }

    // ── W2-GRAPH: KAGI_COMPACT=1 — enable compact row mode ─────
    if std::env::var("KAGI_COMPACT").as_deref() == Ok("1") {
        app_state.graph_compact = true;
        let rh = 18u32; // compact row height
        klog!("graph: compact=on row_h={}", rh);
    }

    // ── T-BP-002: KAGI_BOTTOM_PANEL=1 — open bottom panel at startup ──
    // Emits `[kagi] bottom-panel: open height=H tab=T` for headless verification.
    // T-BP-004: also emits `[kagi] oplog-tab: N entries` (loaded from JSONL at startup).
    if std::env::var("KAGI_BOTTOM_PANEL").as_deref() == Ok("1") {
        app_state.bottom_panel_open = true;

        // T-BP-007: KAGI_TERMINAL=1 switches to the Terminal tab and pre-wires
        // the session container so the PTY can be started inside run_app (where
        // a Window context is available).
        if std::env::var("KAGI_TERMINAL").as_deref() == Ok("1") {
            app_state.bottom_tab = ui::BottomTab::Terminal;
            // Pre-create the session container so it exists when run_app starts.
            // W4-TABS: sessions are keyed by repo path.
            if let Some(ref rp) = app_state.repo_path.clone() {
                app_state.terminal_sessions.insert(
                    rp.clone(),
                    ui::terminal::KagiTerminalSession::new(rp.clone()),
                );
            }
        }

        let h = app_state.bottom_panel_height;
        let t = app_state.bottom_tab;
        let tab_label = match t {
            ui::BottomTab::OperationLog => "OperationLog",
            ui::BottomTab::Terminal => "Terminal",
            ui::BottomTab::Activity => "Activity",
        };
        // W2-STATUS: the height is resolved at first render (18% of viewport);
        // before that the field holds the 0.0 sentinel.
        let h_label = if h > 0.0 {
            format!("{}", h)
        } else {
            "18%-of-viewport".to_string()
        };
        klog!("bottom-panel: open height={} tab={}", h_label, tab_label);
        klog!("oplog-tab: {} entries", app_state.op_log_seed.len());
    }

    // ── W5-MENU: headless command-registry state dump ─────────
    // KAGI_MENU_DUMP=1: log every command's id/label/keystroke/state.  Reflects
    // the current app state (repo open, selection via KAGI_SELECT_FIRST, etc.).
    if std::env::var("KAGI_MENU_DUMP").as_deref() == Ok("1") {
        ui::commands::dump_menu_states(&app_state);
    }

    run_app(app_state);
}
