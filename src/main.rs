#[macro_use]
mod klog;
mod headless;
mod single_instance;
mod ui;

use std::path::PathBuf;

use kagi::git::{open_repository, snapshot, Head};
use ui::{run_app, KagiApp};

/// True when the process is running under the `KAGI_*` headless test harness.
///
/// The single-instance socket logic (ADR-0102) MUST be disabled here: the
/// integration tests spawn the `kagi` binary in parallel (e.g. `tests/i18n_test`)
/// and grep stderr, so a shared per-user socket would let one test forward to /
/// focus another, breaking the `[kagi]` stderr contract and the oneshot exit.
///
/// `KAGI_LOG_DIR` is the test-isolation signal (every binary-spawning test sets
/// it to keep settings.json out of the picture); the other vars cover the
/// env-driven headless paths in `src/headless.rs`.  Real GUI launches set none
/// of these, so single-instance stays on for users.
fn headless_mode() -> bool {
    const SIGNALS: &[&str] = &[
        "KAGI_LOG_DIR",
        "KAGI_OPEN_REPO",
        "KAGI_MENU_DUMP",
        "KAGI_SELECT_FIRST",
        "KAGI_NO_SINGLE_INSTANCE",
    ];
    SIGNALS.iter().any(|k| std::env::var_os(k).is_some())
}

fn main() {
    // Collect CLI arguments (skip argv[0]).
    let args: Vec<String> = std::env::args().skip(1).collect();

    // W9-THEME / ADR-0036: resolve the active colour theme before anything
    // renders.  Priority: KAGI_THEME env → ~/.kagi/settings.json → default
    // (Catppuccin Mocha).  Logs `[kagi] theme: <slug> dark=<bool>`.
    crate::ui::theme::init_active();

    // W27-UIPOLISH: resolve the persisted UI zoom factor (settings.json
    // "ui_zoom", stored as permille) before the first render applies it via
    // `window.set_rem_size`.  Defaults to 1.0x.
    crate::ui::theme::init_zoom();

    // T-SETTINGS-001: resolve the persisted compact-graph flag (settings.json
    // "graph_compact") so new tabs/windows open in the user's chosen density.
    crate::ui::theme::init_compact_graph();

    // Resolve the persisted auto-fetch flag (settings.json "auto_fetch") so the
    // background fetch ticker starts in the user's chosen state.
    crate::ui::theme::init_auto_fetch();

    // W22-I18N / ADR-0048: resolve the UI language before anything renders.
    // Priority: KAGI_LANG env → settings.json "lang" → LANG/LC_ALL → English.
    crate::ui::i18n::init_lang();

    // W4-TABS: KAGI_OPEN_REPO=<path> opens a repo as a tab even when no CLI
    // arg is given (headless picker substitute, ADR-0027/0028).
    let env_open_repo = std::env::var("KAGI_OPEN_REPO").ok().map(PathBuf::from);

    // ── Single-instance forwarding (ADR-0102) ────────────────
    // If another Kagi instance is already running, forward the repo arg (or a
    // focus-only request when bare `kagi`) to it and exit; that instance opens
    // a new tab + raises its window.  Disabled under the headless test harness
    // (parallel binary spawns must not share a socket).  Non-fatal: if no
    // instance answers, fall through to a normal launch and become the primary.
    let headless = headless_mode();
    if !headless {
        let forward_arg = args
            .first()
            .map(PathBuf::from)
            .map(|p| std::fs::canonicalize(&p).unwrap_or(p));
        if single_instance::try_forward(forward_arg) {
            klog!("forwarded to running instance");
            return;
        }
        // No instance answered → we are the primary.  Bind the listener and
        // stash the receiver for the UI drain loop to pick up during window
        // init.  Bind failure is non-fatal: run normally without single-instance.
        #[cfg(unix)]
        if let Some(listener) = single_instance::bind_listener() {
            let (tx, rx) = std::sync::mpsc::channel();
            single_instance::spawn_accept_thread(listener, tx);
            single_instance::store_receiver(rx);
        }
    }

    if args.is_empty() {
        // W4-TABS / ADR-0028: no argument → Welcome screen (tabs empty).
        // The usage error string is still emitted to stderr for headless compat.
        klog!("usage: kagi <repo-path>");
        let mut welcome = KagiApp::with_error("");
        // Session restore: a plain GUI launch (Kagi.app from Dock/Finder has
        // no argv) reopens the previous session's tabs instead of the Welcome
        // screen.  Skipped for headless paths (KAGI_OPEN_REPO / KAGI_MENU_DUMP
        // drive their own state) and with KAGI_NO_RESTORE=1.
        let headless_env = env_open_repo.is_some()
            || std::env::var("KAGI_MENU_DUMP").as_deref() == Ok("1")
            || std::env::var("KAGI_NO_RESTORE").as_deref() == Ok("1");
        if !headless_env {
            ui::tabs::restore_saved_session(&mut welcome);
        }
        welcome.log_tabs();
        // KAGI_* headless hooks for the no-argument (Welcome) launch:
        // KAGI_OPEN_REPO opens the first tab, KAGI_MENU_DUMP dumps menu states.
        headless::run_welcome_hooks(&mut welcome, &env_open_repo);
        run_app(welcome);
        return;
    }

    let repo_path = PathBuf::from(&args[0]);

    // ── Open repository ──────────────────────────────────────
    let info = match open_repository(&repo_path) {
        Ok(info) => info,
        Err(e) => {
            let msg = format!("Error: {e}");
            klog!("{}", msg);
            run_app(KagiApp::with_error(msg));
            return;
        }
    };

    klog!("repo: {}", info.name);
    klog!("path: {}", info.workdir.display());
    klog!("HEAD: {}", info.head.display());

    // ── Snapshot ─────────────────────────────────────────────
    let mut repo2 = match git2::Repository::open(&repo_path) {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("repo open error: {}", e.message());
            klog!("{}", msg);
            run_app(KagiApp::with_error(msg));
            return;
        }
    };

    let snap = match snapshot(&mut repo2, 10_000) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("snapshot error: {e}");
            klog!("{}", msg);
            run_app(KagiApp::with_error(msg));
            return;
        }
    };

    // ── stderr diagnostics (required by T008) ────────────────
    let status = &snap.status;
    if status.is_dirty() {
        eprintln!(
            "[kagi] working tree dirty: {}S {}M {}? {}!",
            status.staged.len(),
            status.unstaged.len(),
            status.untracked.len(),
            status.conflicted.len(),
        );
    } else {
        klog!("working tree clean");
    }

    // HEAD-branch label for unborn repos.
    if let Head::Unborn { branch } = &snap.head {
        klog!("unborn HEAD on branch '{branch}', no commits");
    }

    klog!("commits in snapshot: {}", snap.commits.len());
    for c in snap.commits.iter().take(3) {
        klog!("  {} {}", c.id.short(), c.summary);
    }

    klog!("branches: {}", snap.branches.len());
    klog!("remote branches: {}", snap.remote_branches.len());
    klog!("tags: {}", snap.tags.len());
    klog!("stashes: {}", snap.stashes.len());
    klog!("worktrees: {}", snap.worktrees.len());

    // ── Build app state and launch window ────────────────────
    let mut app_state = KagiApp::from_snapshot(&info.name, &snap);
    // T011: store repo path so the UI can fetch changed files on-demand.
    app_state.repo_path = Some(repo_path.clone());
    app_state.refresh_wip_diffstat();

    // W4-TABS / ADR-0027: the CLI argument becomes the initial tab.
    app_state.tabs.push(ui::tabs::RepoTab {
        path: repo_path.clone(),
        name: info.name.clone(),
        remote: None,
    });
    app_state.active_tab = 0;
    app_state.log_tabs();

    // ── KAGI_* headless harness + window launch ───────────────
    // All env-driven test hooks (KAGI_OPEN_REPO second tab, SELECT_FIRST, JUMP,
    // COMPARE_*, PULL/PUSH/CHECKOUT/COMMIT/AMEND/DISCARD/STASH_*/CHERRY_PICK/
    // REVERT/UNDO/POP/CREATE_BRANCH/DELETE_BRANCH/PLAN_*/BOTTOM_PANEL/TERMINAL/
    // COMMIT_PANEL/MENU_DUMP, …) are applied in `headless::run_repo_flow`, which
    // takes ownership of `app_state` and ends by calling `run_app`.  See ADR-0077.
    headless::run_repo_flow(app_state, repo_path, env_open_repo);
}
