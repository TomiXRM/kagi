mod git;
mod graph;
mod ui;

use std::path::PathBuf;

use git::{Head, open_repository, snapshot};
use ui::{KagiApp, StashPushModal, StashApplyModal, run_app};

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

    // KAGI_OPEN_FIRST_FILE=1 (requires KAGI_SELECT_FIRST=1): after selecting
    // the first commit, automatically open the diff for its first changed file.
    // Emits `[kagi] diff: <path> hunks=N (+A -R)` for headless verification (T012).
    if std::env::var("KAGI_OPEN_FIRST_FILE").as_deref() == Ok("1") {
        app_state.open_file_diff(0);
    }

    // ── T013: headless checkout plan / execute ───────────────
    // KAGI_PLAN_CHECKOUT=<branch>: generate a plan for the branch and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, proceed to execute.
    // Both variables are for fixture/tempdir testing only.  Do not set them
    // in normal use.
    if let Ok(target_branch) = std::env::var("KAGI_PLAN_CHECKOUT") {
        // open_plan_modal logs the plan via [kagi] plan: ...
        app_state.open_plan_modal(&target_branch);

        let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
        if auto_confirm {
            // confirm_checkout runs preflight → execute → reload and logs
            // [kagi] executed: and [kagi] verified: entries.
            if let Some(ref modal) = app_state.plan_modal.clone() {
                if modal.plan.blockers.is_empty() {
                    app_state.confirm_checkout();
                } else {
                    eprintln!(
                        "[kagi] KAGI_AUTO_CONFIRM=1 but checkout has {} blocker(s), skipping",
                        modal.plan.blockers.len()
                    );
                }
            }
        }
    }

    // ── T014: headless create-branch plan / execute ──────────
    // KAGI_CREATE_BRANCH=<name>: generate a create-branch plan using HEAD commit
    // as the starting point and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, execute immediately.
    // For fixture/tempdir testing only.  Do not set in normal use.
    if let Ok(branch_name) = std::env::var("KAGI_CREATE_BRANCH") {
        // Resolve HEAD commit id.
        let repo_path2 = repo_path.clone();
        let head_commit_id = {
            let repo_tmp = git2::Repository::open(&repo_path2).ok();
            repo_tmp.and_then(|r| {
                r.head().ok()
                    .and_then(|h| h.target())
                    .map(|oid| git::CommitId(oid.to_string()))
            })
        };

        if let Some(at) = head_commit_id {
            // Plan and log.
            let repo_for_plan = git2::Repository::open(&repo_path).ok();
            if let Some(repo) = repo_for_plan {
                match git::plan_create_branch(&repo, &branch_name, &at) {
                    Ok(plan) => {
                        eprintln!(
                            "[kagi] plan: create-branch '{}' blockers={} warnings={}",
                            branch_name,
                            plan.blockers.len(),
                            plan.warnings.len()
                        );

                        let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
                        if !auto_confirm {
                            // Without auto-confirm, surface the modal itself so
                            // the create-branch UI can be inspected headlessly.
                            app_state.create_branch_modal = Some(ui::CreateBranchModal {
                                at: at.clone(),
                                input: branch_name.clone(),
                                plan: Some(std::sync::Arc::new(plan.clone())),
                                error: None,
                            });
                        }
                        if auto_confirm {
                            if plan.blockers.is_empty() {
                                // Preflight + execute.
                                let repo2 = git2::Repository::open(&repo_path).ok();
                                if let Some(r2) = repo2 {
                                    if let Err(e) = git::preflight_check(&r2, &plan) {
                                        eprintln!("[kagi] preflight failed: {}", e);
                                    } else if let Err(e) = git::execute_create_branch(&r2, &branch_name, &at) {
                                        eprintln!("[kagi] create-branch failed: {}", e);
                                    } else {
                                        eprintln!("[kagi] executed: create-branch '{}' @ {}", branch_name, at.short());
                                        // Verify.
                                        let repo3 = git2::Repository::open(&repo_path).ok();
                                        if let Some(r3) = repo3 {
                                            let exists = r3.find_branch(&branch_name, git2::BranchType::Local).is_ok();
                                            if exists {
                                                eprintln!("[kagi] verified: branch '{}' exists", branch_name);
                                            } else {
                                                eprintln!("[kagi] verify: branch '{}' NOT found", branch_name);
                                            }
                                            // Log current branch to confirm HEAD unchanged.
                                            if let Ok(head_ref) = r3.head() {
                                                let cur = head_ref.shorthand().unwrap_or("?");
                                                eprintln!("[kagi] verified: current branch = {}", cur);
                                            }
                                        }
                                    }
                                }
                            } else {
                                eprintln!(
                                    "[kagi] KAGI_AUTO_CONFIRM=1 but create-branch has {} blocker(s), skipping",
                                    plan.blockers.len()
                                );
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[kagi] plan: create-branch error: {}", e);
                    }
                }
            }
        } else {
            eprintln!("[kagi] KAGI_CREATE_BRANCH: could not resolve HEAD commit");
        }
    }

    // ── T015: headless stash push plan / execute ─────────────
    // KAGI_STASH_PUSH=1: generate a stash-push plan and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, execute immediately.
    // For fixture/tempdir testing only.  Do not set in normal use.
    if std::env::var("KAGI_STASH_PUSH").as_deref() == Ok("1") {
        let mut repo_sp = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] KAGI_STASH_PUSH: repo open error: {}", e.message());
                run_app(app_state);
                return;
            }
        };

        match git::plan_stash_push(&mut repo_sp, None) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-push blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );

                let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
                if auto_confirm {
                    if plan.blockers.is_empty() {
                        let stash_count_at_plan = plan.stash_count_at_plan;
                        let mut repo2 = match git2::Repository::open(&repo_path) {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("[kagi] KAGI_STASH_PUSH: repo open error: {}", e.message());
                                run_app(app_state);
                                return;
                            }
                        };
                        if let Err(e) = git::preflight_check_stash(&mut repo2, &plan, stash_count_at_plan) {
                            eprintln!("[kagi] preflight failed: {}", e);
                        } else if let Err(e) = git::execute_stash_push(&mut repo2, None) {
                            eprintln!("[kagi] stash-push failed: {}", e);
                        } else {
                            eprintln!("[kagi] executed: stash-push");
                            // Verify: working tree clean + stash count.
                            let mut repo3 = match git2::Repository::open(&repo_path) {
                                Ok(r) => r,
                                Err(e) => {
                                    eprintln!("[kagi] verify: repo open error: {}", e.message());
                                    run_app(app_state);
                                    return;
                                }
                            };
                            match snapshot(&mut repo3, 10_000) {
                                Ok(snap) => {
                                    let clean = !snap.status.is_dirty();
                                    let stash_count = snap.stashes.len();
                                    if clean {
                                        eprintln!("[kagi] verified: working tree clean");
                                    } else {
                                        eprintln!("[kagi] verify: working tree NOT clean");
                                    }
                                    eprintln!("[kagi] verified: stash count={}", stash_count);
                                }
                                Err(e) => eprintln!("[kagi] verify: snapshot error: {}", e),
                            }
                        }
                    } else {
                        eprintln!(
                            "[kagi] KAGI_AUTO_CONFIRM=1 but stash-push has {} blocker(s), skipping",
                            plan.blockers.len()
                        );
                    }
                } else {
                    // Without auto-confirm, surface the modal so it can be inspected headlessly.
                    app_state.stash_push_modal = Some(StashPushModal {
                        input: String::new(),
                        plan: Some(std::sync::Arc::new(plan)),
                        error: None,
                    });
                }
            }
            Err(e) => {
                eprintln!("[kagi] plan: stash-push error: {}", e);
            }
        }
    }

    // ── T015: headless stash apply plan / execute ─────────────
    // KAGI_STASH_APPLY=<index>: generate a stash-apply plan for stash@{index}.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, execute immediately.
    // For fixture/tempdir testing only.  Do not set in normal use.
    if let Ok(idx_str) = std::env::var("KAGI_STASH_APPLY") {
        let index: usize = match idx_str.parse() {
            Ok(i) => i,
            Err(_) => {
                eprintln!("[kagi] KAGI_STASH_APPLY: invalid index '{}'", idx_str);
                run_app(app_state);
                return;
            }
        };

        let mut repo_sa = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] KAGI_STASH_APPLY: repo open error: {}", e.message());
                run_app(app_state);
                return;
            }
        };

        match git::plan_stash_apply(&mut repo_sa, index) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-apply index={} blockers={} warnings={}",
                    index,
                    plan.blockers.len(),
                    plan.warnings.len()
                );

                let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
                if auto_confirm {
                    if plan.blockers.is_empty() {
                        let stash_count_at_plan = plan.stash_count_at_plan;
                        let mut repo2 = match git2::Repository::open(&repo_path) {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("[kagi] KAGI_STASH_APPLY: repo open error: {}", e.message());
                                run_app(app_state);
                                return;
                            }
                        };
                        if let Err(e) = git::preflight_check_stash(&mut repo2, &plan, stash_count_at_plan) {
                            eprintln!("[kagi] preflight failed: {}", e);
                        } else if let Err(e) = git::execute_stash_apply(&mut repo2, index) {
                            eprintln!("[kagi] stash-apply failed: {}", e);
                        } else {
                            eprintln!("[kagi] executed: stash-apply index={}", index);
                            // Verify: working tree dirty + stash still present.
                            let mut repo3 = match git2::Repository::open(&repo_path) {
                                Ok(r) => r,
                                Err(e) => {
                                    eprintln!("[kagi] verify: repo open error: {}", e.message());
                                    run_app(app_state);
                                    return;
                                }
                            };
                            match snapshot(&mut repo3, 10_000) {
                                Ok(snap) => {
                                    let is_dirty = snap.status.is_dirty();
                                    let stash_count = snap.stashes.len();
                                    if is_dirty {
                                        eprintln!("[kagi] verified: working tree dirty (restored)");
                                    } else {
                                        eprintln!("[kagi] verify: working tree NOT dirty after apply");
                                    }
                                    eprintln!("[kagi] verified: stash count={} (entry preserved)", stash_count);
                                }
                                Err(e) => eprintln!("[kagi] verify: snapshot error: {}", e),
                            }
                        }
                    } else {
                        eprintln!(
                            "[kagi] KAGI_AUTO_CONFIRM=1 but stash-apply has {} blocker(s), skipping",
                            plan.blockers.len()
                        );
                    }
                } else {
                    // Without auto-confirm, surface the modal.
                    app_state.stash_apply_modal = Some(StashApplyModal {
                        index,
                        plan: std::sync::Arc::new(plan),
                        error: None,
                    });
                }
            }
            Err(e) => {
                eprintln!("[kagi] plan: stash-apply error: {}", e);
            }
        }
    }

    run_app(app_state);
}
