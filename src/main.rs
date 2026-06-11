mod git;
mod graph;
mod ui;

use std::path::PathBuf;

use git::{Head, open_repository, snapshot};
use ui::{KagiApp, run_app};

fn main() {
    // Collect CLI arguments (skip argv[0]).
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("[kagi] usage: kagi <repo-path>");
        run_app(KagiApp::with_error(
            "Usage: kagi <repo-path>\n\nProvide the path to a git repository as the first argument.",
        ));
        return;
    }

    let repo_path = PathBuf::from(&args[0]);

    // ── Open repository ──────────────────────────────────────
    let info = match open_repository(&repo_path) {
        Ok(info) => info,
        Err(e) => {
            let msg = format!("Error: {e}");
            eprintln!("[kagi] {}", msg);
            run_app(KagiApp::with_error(msg));
            return;
        }
    };

    eprintln!("[kagi] repo: {}", info.name);
    eprintln!("[kagi] path: {}", info.workdir.display());
    eprintln!("[kagi] HEAD: {}", info.head.display());

    // ── Snapshot ─────────────────────────────────────────────
    let mut repo2 = match git2::Repository::open(&repo_path) {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("repo open error: {}", e.message());
            eprintln!("[kagi] {}", msg);
            run_app(KagiApp::with_error(msg));
            return;
        }
    };

    let snap = match snapshot(&mut repo2, 10_000) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("snapshot error: {e}");
            eprintln!("[kagi] {}", msg);
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
        eprintln!("[kagi] working tree clean");
    }

    // HEAD-branch label for unborn repos.
    match &snap.head {
        Head::Unborn { branch } => {
            eprintln!("[kagi] unborn HEAD on branch '{branch}', no commits");
        }
        _ => {}
    }

    eprintln!("[kagi] commits in snapshot: {}", snap.commits.len());
    for c in snap.commits.iter().take(3) {
        eprintln!("[kagi]   {} {}", c.id.short(), c.summary);
    }

    eprintln!("[kagi] branches: {}", snap.branches.len());
    eprintln!("[kagi] remote branches: {}", snap.remote_branches.len());
    eprintln!("[kagi] tags: {}", snap.tags.len());
    eprintln!("[kagi] stashes: {}", snap.stashes.len());

    // ── Build app state and launch window ────────────────────
    let mut app_state = KagiApp::from_snapshot(&info.name, &snap);
    // T011: store repo path so the UI can fetch changed files on-demand.
    app_state.repo_path = Some(repo_path.clone());

    // KAGI_SELECT_FIRST=1: auto-select row 0 at startup for headless
    // verification of the detail panel render path (T010).
    if std::env::var("KAGI_SELECT_FIRST").as_deref() == Ok("1") {
        if !app_state.rows.is_empty() {
            app_state.select(0);
        }
    }

    run_app(app_state);
}
