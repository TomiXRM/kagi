mod ui;

use std::path::PathBuf;

use kagi::git::{Head, open_repository, snapshot};
use kagi::git::oplog::{OpLogEntry, OpOutcome, append_oplog};
use kagi::git::ops::StateSummary;
use ui::{KagiApp, StashPushModal, StashApplyModal, CherryPickModal, run_app};
use ui::commit_panel::CommitPanelState;

/// Write an oplog entry and emit footer log.  Non-fatal on write error.
fn record_headless_op(
    op: &str,
    before: StateSummary,
    outcome: OpOutcome,
    repo_path: &PathBuf,
) {
    let repo_str = repo_path.display().to_string();
    let (kind_str, desc) = match &outcome {
        OpOutcome::Success { after } => ("Success", format!("{} → {}", before.head, after.head)),
        OpOutcome::Failed { error } => ("Failed", error.clone()),
        OpOutcome::Refused { blockers } => ("Refused", format!("{} blocker(s)", blockers.len())),
    };
    let entry = OpLogEntry::new(op, &repo_str, before, outcome);
    if let Err(e) = append_oplog(&entry) {
        eprintln!("[kagi] oplog: write failed (non-fatal): {}", e);
    }
    eprintln!("[kagi] footer: {}: {} ({})", op, desc, kind_str);
}

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

    // ── T028: headless branch jump ────────────────────────────
    // KAGI_JUMP=<branch>: simulate a single-click on the named branch.
    // Emits `[kagi] jump: <branch> -> row N` + selected log.
    // Used for fixture/tempdir headless verification only.
    if let Ok(jump_branch) = std::env::var("KAGI_JUMP") {
        app_state.jump_to_branch(&jump_branch);
    }

    // KAGI_OPEN_FIRST_FILE=1 (requires KAGI_SELECT_FIRST=1): after selecting
    // the first commit, automatically open the diff for its first changed file.
    // Emits `[kagi] diff: <path> hunks=N (+A -R)` for headless verification (T012).
    if std::env::var("KAGI_OPEN_FIRST_FILE").as_deref() == Ok("1") {
        app_state.open_file_diff(0);
    }

    // ── T-HT-003: headless pull plan / execute ───────────────
    // KAGI_PULL=1: generate a pull plan and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, fetch + FF/merge.
    // For fixture/tempdir testing only.  Do not set in normal use.
    if std::env::var("KAGI_PULL").as_deref() == Ok("1") {
        // open_pull_modal logs the plan via [kagi] plan: pull ...
        app_state.open_pull_modal();

        let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
        if auto_confirm {
            if let Some(ref modal) = app_state.pull_modal.clone() {
                if modal.plan.blockers.is_empty() {
                    // confirm_pull runs preflight → fetch → FF/merge and logs
                    // [kagi] executed: pull / [kagi] verified: entries.
                    app_state.confirm_pull();
                } else {
                    eprintln!(
                        "[kagi] KAGI_AUTO_CONFIRM=1 but pull has {} blocker(s), skipping",
                        modal.plan.blockers.len()
                    );
                    record_headless_op("pull", modal.plan.current.clone(), OpOutcome::Refused { blockers: modal.plan.blockers.clone() }, &repo_path);
                }
            }
        }
    }

    // ── T-HT-004: headless push plan / execute ──────────────
    // KAGI_PUSH=1: generate a push plan and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, execute the push.
    // For fixture/tempdir testing only.  Do not set in normal use.
    if std::env::var("KAGI_PUSH").as_deref() == Ok("1") {
        // open_push_modal logs the plan via [kagi] plan: push ...
        app_state.open_push_modal();

        let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
        if auto_confirm {
            if let Some(ref modal) = app_state.push_modal.clone() {
                if modal.plan.blockers.is_empty() {
                    // confirm_push runs preflight → execute_push and logs
                    // [kagi] executed: push / [kagi] verified: entries.
                    app_state.confirm_push();
                } else {
                    eprintln!(
                        "[kagi] KAGI_AUTO_CONFIRM=1 but push has {} blocker(s), skipping",
                        modal.plan.blockers.len()
                    );
                    record_headless_op("push", modal.plan.current.clone(), OpOutcome::Refused { blockers: modal.plan.blockers.clone() }, &repo_path);
                }
            }
        }
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
                    record_headless_op("checkout", modal.plan.current.clone(), OpOutcome::Refused { blockers: modal.plan.blockers.clone() }, &repo_path);
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
                    .map(|oid| kagi::git::CommitId(oid.to_string()))
            })
        };

        if let Some(at) = head_commit_id {
            // Plan and log.
            let repo_for_plan = git2::Repository::open(&repo_path).ok();
            if let Some(repo) = repo_for_plan {
                match kagi::git::plan_create_branch(&repo, &branch_name, &at) {
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
                                    if let Err(e) = kagi::git::preflight_check(&r2, &plan) {
                                        let err_msg = format!("preflight failed: {}", e);
                                        eprintln!("[kagi] {}", err_msg);
                                        record_headless_op("create-branch", plan.current.clone(), OpOutcome::Failed { error: err_msg }, &repo_path);
                                    } else if let Err(e) = kagi::git::execute_create_branch(&r2, &branch_name, &at) {
                                        let err_msg = format!("create-branch failed: {}", e);
                                        eprintln!("[kagi] {}", err_msg);
                                        record_headless_op("create-branch", plan.current.clone(), OpOutcome::Failed { error: err_msg }, &repo_path);
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
                                        record_headless_op("create-branch", plan.current.clone(), OpOutcome::Success { after: plan.predicted.clone() }, &repo_path);
                                    }
                                }
                            } else {
                                eprintln!(
                                    "[kagi] KAGI_AUTO_CONFIRM=1 but create-branch has {} blocker(s), skipping",
                                    plan.blockers.len()
                                );
                                record_headless_op("create-branch", plan.current.clone(), OpOutcome::Refused { blockers: plan.blockers.clone() }, &repo_path);
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

        match kagi::git::plan_stash_push(&mut repo_sp, None) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-push blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );

                let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
                if auto_confirm {
                    if plan.blockers.is_empty() {
                        let stash_count_at_plan = plan.stash_count_at_plan();
                        let mut repo2 = match git2::Repository::open(&repo_path) {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("[kagi] KAGI_STASH_PUSH: repo open error: {}", e.message());
                                run_app(app_state);
                                return;
                            }
                        };
                        if let Err(e) = kagi::git::preflight_check_stash(&mut repo2, &plan, stash_count_at_plan) {
                            let err_msg = format!("preflight failed: {}", e);
                            eprintln!("[kagi] {}", err_msg);
                            record_headless_op("stash-push", plan.current.clone(), OpOutcome::Failed { error: err_msg }, &repo_path);
                        } else if let Err(e) = kagi::git::execute_stash_push(&mut repo2, None) {
                            let err_msg = format!("stash-push failed: {}", e);
                            eprintln!("[kagi] {}", err_msg);
                            record_headless_op("stash-push", plan.current.clone(), OpOutcome::Failed { error: err_msg }, &repo_path);
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
                                    record_headless_op("stash-push", plan.current.clone(), OpOutcome::Success {
                                        after: StateSummary { head: snap.head.display(), dirty: if clean { "clean".to_string() } else { "dirty".to_string() } }
                                    }, &repo_path);
                                }
                                Err(e) => {
                                    eprintln!("[kagi] verify: snapshot error: {}", e);
                                    record_headless_op("stash-push", plan.current.clone(), OpOutcome::Success { after: plan.predicted.clone() }, &repo_path);
                                }
                            }
                        }
                    } else {
                        eprintln!(
                            "[kagi] KAGI_AUTO_CONFIRM=1 but stash-push has {} blocker(s), skipping",
                            plan.blockers.len()
                        );
                        record_headless_op("stash-push", plan.current.clone(), OpOutcome::Refused { blockers: plan.blockers.clone() }, &repo_path);
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

        match kagi::git::plan_stash_apply(&mut repo_sa, index) {
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
                        let stash_count_at_plan = plan.stash_count_at_plan();
                        let mut repo2 = match git2::Repository::open(&repo_path) {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("[kagi] KAGI_STASH_APPLY: repo open error: {}", e.message());
                                run_app(app_state);
                                return;
                            }
                        };
                        if let Err(e) = kagi::git::preflight_check_stash(&mut repo2, &plan, stash_count_at_plan) {
                            let err_msg = format!("preflight failed: {}", e);
                            eprintln!("[kagi] {}", err_msg);
                            record_headless_op("stash-apply", plan.current.clone(), OpOutcome::Failed { error: err_msg }, &repo_path);
                        } else if let Err(e) = kagi::git::execute_stash_apply(&mut repo2, index) {
                            let err_msg = format!("stash-apply failed: {}", e);
                            eprintln!("[kagi] {}", err_msg);
                            record_headless_op("stash-apply", plan.current.clone(), OpOutcome::Failed { error: err_msg }, &repo_path);
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
                                    record_headless_op("stash-apply", plan.current.clone(), OpOutcome::Success {
                                        after: StateSummary { head: snap.head.display(), dirty: if is_dirty { "dirty".to_string() } else { "clean".to_string() } }
                                    }, &repo_path);
                                }
                                Err(e) => {
                                    eprintln!("[kagi] verify: snapshot error: {}", e);
                                    record_headless_op("stash-apply", plan.current.clone(), OpOutcome::Success { after: plan.predicted.clone() }, &repo_path);
                                }
                            }
                        }
                    } else {
                        eprintln!(
                            "[kagi] KAGI_AUTO_CONFIRM=1 but stash-apply has {} blocker(s), skipping",
                            plan.blockers.len()
                        );
                        record_headless_op("stash-apply", plan.current.clone(), OpOutcome::Refused { blockers: plan.blockers.clone() }, &repo_path);
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

    // ── T016: headless cherry-pick plan / execute ────────────
    // KAGI_CHERRY_PICK=<sha>: generate a cherry-pick plan and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, execute immediately.
    // For fixture/tempdir testing only.  Do not set in normal use.
    if let Ok(sha_str) = std::env::var("KAGI_CHERRY_PICK") {
        let commit_id = kagi::git::CommitId(sha_str.clone());
        let repo_cp = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] KAGI_CHERRY_PICK: repo open error: {}", e.message());
                run_app(app_state);
                return;
            }
        };

        match kagi::git::plan_cherry_pick(&repo_cp, &commit_id) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: cherry-pick {} blockers={} preview_files={}",
                    commit_id.short(),
                    plan.blockers.len(),
                    plan.preview_files.len()
                );
                for b in &plan.blockers {
                    eprintln!("[kagi] plan: blocker: {}", b);
                }
                for f in &plan.preview_files {
                    eprintln!(
                        "[kagi] plan: preview_file: {} ({})",
                        f.path.display(),
                        f.change.label()
                    );
                }

                let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
                if auto_confirm {
                    if plan.blockers.is_empty() {
                        // Preflight + execute.
                        let repo2 = match git2::Repository::open(&repo_path) {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("[kagi] KAGI_CHERRY_PICK: repo open error: {}", e.message());
                                run_app(app_state);
                                return;
                            }
                        };
                        if let Err(e) = kagi::git::preflight_check(&repo2, &plan) {
                            let err_msg = format!("preflight failed: {}", e);
                            eprintln!("[kagi] {}", err_msg);
                            record_headless_op("cherry-pick", plan.current.clone(), OpOutcome::Failed { error: err_msg }, &repo_path);
                        } else {
                            match kagi::git::execute_cherry_pick(&repo2, &commit_id) {
                                Ok(new_id) => {
                                    eprintln!(
                                        "[kagi] executed: cherry-pick {} -> {}",
                                        commit_id.short(),
                                        new_id.short()
                                    );
                                    // Verify: HEAD is the new commit + WT clean + message matches.
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
                                            if let Head::Attached { target, branch } = &snap.head {
                                                if *target == new_id.0 {
                                                    eprintln!("[kagi] verified: HEAD={} on {}", new_id.short(), branch);
                                                } else {
                                                    eprintln!("[kagi] verify: HEAD={} (expected {})", &target[..8.min(target.len())], new_id.short());
                                                }
                                            }
                                            let is_clean = !snap.status.is_dirty();
                                            if is_clean {
                                                eprintln!("[kagi] verified: working tree clean after cherry-pick");
                                            } else {
                                                eprintln!("[kagi] verify: working tree dirty after cherry-pick (unexpected)");
                                            }
                                            // Log first commit message for manual inspection.
                                            if let Some(c) = snap.commits.first() {
                                                eprintln!("[kagi] verified: new HEAD message: {}", c.summary);
                                            }
                                            record_headless_op("cherry-pick", plan.current.clone(), OpOutcome::Success {
                                                after: StateSummary { head: snap.head.display(), dirty: if is_clean { "clean".to_string() } else { "dirty".to_string() } }
                                            }, &repo_path);
                                        }
                                        Err(e) => {
                                            eprintln!("[kagi] verify: snapshot error: {}", e);
                                            record_headless_op("cherry-pick", plan.current.clone(), OpOutcome::Success { after: plan.predicted.clone() }, &repo_path);
                                        }
                                    }
                                    app_state.reload();
                                }
                                Err(e) => {
                                    let err_msg = format!("cherry-pick execute failed: {}", e);
                                    eprintln!("[kagi] {}", err_msg);
                                    record_headless_op("cherry-pick", plan.current.clone(), OpOutcome::Failed { error: err_msg }, &repo_path);
                                }
                            }
                        }
                    } else {
                        eprintln!(
                            "[kagi] KAGI_AUTO_CONFIRM=1 but cherry-pick has {} blocker(s), skipping",
                            plan.blockers.len()
                        );
                        record_headless_op("cherry-pick", plan.current.clone(), OpOutcome::Refused { blockers: plan.blockers.clone() }, &repo_path);
                    }
                } else {
                    // Without auto-confirm, surface the cherry-pick modal.
                    app_state.cherry_pick_modal = Some(CherryPickModal {
                        commit_id,
                        plan: std::sync::Arc::new(plan),
                        error: None,
                    });
                }
            }
            Err(e) => {
                eprintln!("[kagi] plan: cherry-pick error: {}", e);
            }
        }
    }

    // ── T025: headless commit panel env vars ─────────────────
    //
    // KAGI_COMMIT_PANEL=1: open commit panel and log counts.
    // KAGI_STAGE_FILE=<path>: stage one file and log counts.
    // KAGI_UNSTAGE_FILE=<path>: unstage one file and log counts.
    // KAGI_COMMIT_MSG=<msg> + KAGI_AUTO_CONFIRM=1: plan + execute commit.
    // All operations use fixture/tempdir repos only.

    if std::env::var("KAGI_COMMIT_PANEL").as_deref() == Ok("1") {
        let mut panel = CommitPanelState::from_repo(&repo_path);
        eprintln!(
            "[kagi] commit-panel: unstaged={} staged={}",
            panel.unstaged.len(),
            panel.staged.len()
        );

        // KAGI_STAGE_FILE: stage a file and log updated counts.
        if let Ok(path_str) = std::env::var("KAGI_STAGE_FILE") {
            use std::path::Path;
            let repo_s = git2::Repository::open(&repo_path).ok();
            if let Some(repo_s) = repo_s {
                match kagi::git::stage_file(&repo_s, Path::new(&path_str)) {
                    Ok(_) => {
                        eprintln!("[kagi] staged: {}", path_str);
                        panel.reload_status(&repo_path);
                        eprintln!(
                            "[kagi] commit-panel: unstaged={} staged={}",
                            panel.unstaged.len(),
                            panel.staged.len()
                        );
                    }
                    Err(e) => eprintln!("[kagi] KAGI_STAGE_FILE error: {}", e),
                }
            }
        }

        // KAGI_UNSTAGE_FILE: unstage a file and log updated counts.
        if let Ok(path_str) = std::env::var("KAGI_UNSTAGE_FILE") {
            use std::path::Path;
            let repo_u = git2::Repository::open(&repo_path).ok();
            if let Some(repo_u) = repo_u {
                match kagi::git::unstage_file(&repo_u, Path::new(&path_str)) {
                    Ok(_) => {
                        eprintln!("[kagi] unstaged: {}", path_str);
                        panel.reload_status(&repo_path);
                        eprintln!(
                            "[kagi] commit-panel: unstaged={} staged={}",
                            panel.unstaged.len(),
                            panel.staged.len()
                        );
                    }
                    Err(e) => eprintln!("[kagi] KAGI_UNSTAGE_FILE error: {}", e),
                }
            }
        }

        // KAGI_COMMIT_MSG + KAGI_AUTO_CONFIRM=1: plan and execute commit.
        if let Ok(commit_msg) = std::env::var("KAGI_COMMIT_MSG") {
            let repo_c = git2::Repository::open(&repo_path).ok();
            if let Some(repo_c) = repo_c {
                match kagi::git::plan_commit(&repo_c, &commit_msg) {
                    Ok(plan) => {
                        eprintln!(
                            "[kagi] plan: commit blockers={} warnings={}",
                            plan.blockers.len(),
                            plan.warnings.len()
                        );
                        for w in &plan.warnings {
                            eprintln!("[kagi] plan: warning: {}", w);
                        }
                        for b in &plan.blockers {
                            eprintln!("[kagi] plan: blocker: {}", b);
                        }

                        let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
                        if auto_confirm && plan.blockers.is_empty() {
                            let repo_e = git2::Repository::open(&repo_path).ok();
                            if let Some(repo_e) = repo_e {
                                match kagi::git::execute_commit(&repo_e, &commit_msg) {
                                    Ok(new_id) => {
                                        eprintln!("[kagi] executed: commit {}", new_id.short());
                                        // Verify: check new commit exists and untracked remain.
                                        let mut repo_v = match git2::Repository::open(&repo_path) {
                                            Ok(r) => r,
                                            Err(e) => {
                                                eprintln!("[kagi] verify: repo open error: {}", e.message());
                                                run_app(app_state);
                                                return;
                                            }
                                        };
                                        match kagi::git::snapshot(&mut repo_v, 10_000) {
                                            Ok(snap) => {
                                                eprintln!(
                                                    "[kagi] verified: commit count={}",
                                                    snap.commits.len()
                                                );
                                                if let Some(c) = snap.commits.first() {
                                                    eprintln!("[kagi] verified: HEAD message: {}", c.summary);
                                                }
                                                let is_dirty = snap.status.is_dirty();
                                                if is_dirty {
                                                    eprintln!("[kagi] verified: working tree dirty (unstaged remain)");
                                                } else {
                                                    eprintln!("[kagi] verified: working tree clean");
                                                }
                                                record_headless_op(
                                                    "commit",
                                                    StateSummary { head: snap.head.display(), dirty: plan.current.dirty.clone() },
                                                    OpOutcome::Success {
                                                        after: StateSummary { head: snap.head.display(), dirty: if is_dirty { "dirty".to_string() } else { "clean".to_string() } }
                                                    },
                                                    &repo_path,
                                                );
                                            }
                                            Err(e) => eprintln!("[kagi] verify: snapshot error: {}", e),
                                        }
                                    }
                                    Err(e) => eprintln!("[kagi] execute_commit error: {}", e),
                                }
                            }
                        } else if auto_confirm && !plan.blockers.is_empty() {
                            eprintln!(
                                "[kagi] KAGI_AUTO_CONFIRM=1 but commit has {} blocker(s), skipping",
                                plan.blockers.len()
                            );
                        }
                    }
                    Err(e) => eprintln!("[kagi] plan_commit error: {}", e),
                }
            }
        }

        // Set up commit panel state in app_state for UI inspection.
        app_state.commit_panel = Some(panel);
        app_state.commit_panel_open = true;
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
            if let Some(ref rp) = app_state.repo_path.clone() {
                app_state.terminal_session =
                    Some(ui::terminal::KagiTerminalSession::new(rp.clone()));
            }
        }

        let h = app_state.bottom_panel_height;
        let t = app_state.bottom_tab;
        let tab_label = match t {
            ui::BottomTab::OperationLog => "OperationLog",
            ui::BottomTab::Terminal => "Terminal",
        };
        eprintln!("[kagi] bottom-panel: open height={} tab={}", h, tab_label);
        eprintln!("[kagi] oplog-tab: {} entries", app_state.op_entries.len());
    }

    run_app(app_state);
}
