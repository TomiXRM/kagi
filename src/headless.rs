//! KAGI_* headless test harness (relocated from main.rs by T-MAINSLIM-001).
//!
//! This module is the env-var-driven, test-only harness used by the fixture /
//! tempdir headless tests that grep stderr.  It is a PURE RELOCATION of the
//! logic that used to live inline in `main()`: every `[kagi] ...` string, env
//! var name, precedence and ordering is byte-identical to the pre-extraction
//! `main.rs`.  This is the bin shell (out of the `src/ui/` git2 invariant); do
//! not add new git logic here — only what was moved.
//!
//! ADR-0077 plans to retire/relocate this harness once the layered test
//! pyramid (kagi-domain / kagi-git / kagi-app / kagi-ui) makes the VM and
//! controller directly testable.  Until then it stays, behaviour-frozen.

use std::path::PathBuf;

use kagi::git::oplog::{append_oplog, OpLogEntry, OpOutcome};
use kagi::git::ops::StateSummary;
use kagi::git::{open_repository, snapshot, Head};

use crate::ui::commit_panel::CommitPanelState;
use crate::ui::{
    self, run_app, CherryPickModal, KagiApp, RevertModal, StashApplyModal, StashPushModal,
};

/// Write an oplog entry and emit footer log.  Non-fatal on write error.
pub(crate) fn record_headless_op(
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
        klog!("oplog: write failed (non-fatal): {}", e);
    }
    klog!("footer: {}: {} ({})", op, desc, kind_str);
}

/// W17-DISCARD headless path (ADR-0046). Collects target paths, then runs
/// plan → (optionally) execute → verify with `[kagi] planned/executed/verified:`
/// logging in the same style as the other KAGI_* env paths.
///
/// `single` discards one path; `all` discards every eligible unstaged file
/// (untracked / conflicted are skipped, as the UI does). Execution only happens
/// when `KAGI_AUTO_CONFIRM=1` is set (test-only).
fn run_headless_discard(repo_path: &PathBuf, single: Option<String>, all: bool) {
    use kagi::git::{execute_discard, plan_discard, working_tree_status};

    let repo = match git2::Repository::open(repo_path) {
        Ok(r) => r,
        Err(e) => {
            klog!("KAGI_DISCARD: repo open error: {}", e.message());
            return;
        }
    };

    // Build the target list.
    let mut paths: Vec<String> = Vec::new();
    if all {
        match working_tree_status(&repo) {
            Ok(status) => {
                let conflicted: std::collections::HashSet<String> = status
                    .conflicted
                    .iter()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .collect();
                // ADR-0083: untracked files ARE discardable (deleted after an ODB
                // backup); only conflicted rows are skipped. Mirrors the UI's
                // `discard_partition`.
                for f in &status.unstaged {
                    let rel = f.path.to_string_lossy().replace('\\', "/");
                    if !conflicted.contains(&rel) {
                        paths.push(rel);
                    }
                }
                // Untracked files are surfaced in the unstaged section by the UI;
                // include them here too so the headless path matches.
                for p in &status.untracked {
                    let rel = p.to_string_lossy().replace('\\', "/");
                    if !paths.contains(&rel) {
                        paths.push(rel);
                    }
                }
            }
            Err(e) => {
                klog!("KAGI_DISCARD_ALL: status error: {}", e);
                return;
            }
        }
    }
    if let Some(p) = single {
        if !paths.contains(&p) {
            paths.push(p);
        }
    }

    let plan = match plan_discard(&repo, &paths) {
        Ok(p) => p,
        Err(e) => {
            klog!("KAGI_DISCARD: plan error: {}", e);
            return;
        }
    };
    eprintln!(
        "[kagi] planned: discard {} target(s), blockers={} warnings={} destructive={}",
        paths.len(),
        plan.blockers.len(),
        plan.warnings.len(),
        plan.destructive
    );

    let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
    if !auto_confirm {
        return;
    }
    if !plan.blockers.is_empty() {
        eprintln!(
            "[kagi] KAGI_AUTO_CONFIRM=1 but discard has {} blocker(s), skipping",
            plan.blockers.len()
        );
        record_headless_op(
            "discard",
            plan.current.clone(),
            OpOutcome::Refused {
                blockers: plan.blockers.clone(),
            },
            repo_path,
        );
        return;
    }

    match execute_discard(&repo, &plan, &paths) {
        Ok(outcome) => {
            let summary = outcome.oplog_summary();
            klog!("executed: {}", summary);
            // Verify: re-read status; none of the targets should be unstaged.
            let after_dirty = match working_tree_status(&repo) {
                Ok(status) => {
                    let still: std::collections::HashSet<String> = status
                        .unstaged
                        .iter()
                        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
                        .collect();
                    let leftover = paths.iter().filter(|p| still.contains(*p)).count();
                    if leftover == 0 {
                        eprintln!(
                            "[kagi] verified: {} target(s) left the unstaged set",
                            paths.len()
                        );
                    } else {
                        klog!("verify: {} target(s) still unstaged", leftover);
                    }
                    summary.clone()
                }
                Err(e) => {
                    klog!("verify: status error: {}", e);
                    summary.clone()
                }
            };
            record_headless_op(
                "discard",
                plan.current.clone(),
                OpOutcome::Success {
                    after: StateSummary {
                        head: plan.current.head.clone(),
                        dirty: after_dirty,
                    },
                },
                repo_path,
            );
        }
        Err(e) => {
            let err_msg = format!("discard failed: {}", e);
            klog!("{}", err_msg);
            record_headless_op(
                "discard",
                plan.current.clone(),
                OpOutcome::Failed { error: err_msg },
                repo_path,
            );
        }
    }
}

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
pub fn run_repo_flow(mut app_state: KagiApp, repo_path: PathBuf, env_open_repo: Option<PathBuf>) {
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
        app_state.select(0);
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

    // ── T-HT-003: headless pull plan / execute ───────────────
    // KAGI_PULL=1: generate a pull plan and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, fetch + FF/merge.
    // For fixture/tempdir testing only.  Do not set in normal use.
    if std::env::var("KAGI_PULL").as_deref() == Ok("1") {
        // open_pull_modal logs the plan via [kagi] plan: pull ...
        app_state.open_pull_modal();

        let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
        if auto_confirm {
            if let Some(ref modal) = app_state.pull_modal().cloned() {
                if modal.plan.blockers.is_empty() {
                    // confirm_pull runs preflight → fetch → FF/merge and logs
                    // [kagi] executed: pull / [kagi] verified: entries.
                    app_state.confirm_pull();
                } else {
                    eprintln!(
                        "[kagi] KAGI_AUTO_CONFIRM=1 but pull has {} blocker(s), skipping",
                        modal.plan.blockers.len()
                    );
                    record_headless_op(
                        "pull",
                        modal.plan.current.clone(),
                        OpOutcome::Refused {
                            blockers: modal.plan.blockers.clone(),
                        },
                        &repo_path,
                    );
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
            if let Some(ref modal) = app_state.push_modal().cloned() {
                if modal.plan.blockers.is_empty() {
                    // confirm_push runs preflight → execute_push and logs
                    // [kagi] executed: push / [kagi] verified: entries.
                    app_state.confirm_push();
                } else {
                    eprintln!(
                        "[kagi] KAGI_AUTO_CONFIRM=1 but push has {} blocker(s), skipping",
                        modal.plan.blockers.len()
                    );
                    record_headless_op(
                        "push",
                        modal.plan.current.clone(),
                        OpOutcome::Refused {
                            blockers: modal.plan.blockers.clone(),
                        },
                        &repo_path,
                    );
                }
            }
        }
    }

    // ── T-HT-009/007: headless undo / pop ────────────────────
    if std::env::var("KAGI_UNDO").as_deref() == Ok("1") {
        app_state.open_undo_modal();
        if std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1") {
            if let Some(ref modal) = app_state.undo_modal().cloned() {
                if modal.plan.blockers.is_empty() {
                    app_state.confirm_undo();
                } else {
                    eprintln!(
                        "[kagi] KAGI_AUTO_CONFIRM=1 but undo has {} blocker(s), skipping",
                        modal.plan.blockers.len()
                    );
                }
            }
        }
    }
    if let Ok(idx) = std::env::var("KAGI_POP") {
        let index: usize = idx.parse().unwrap_or(0);
        app_state.open_pop_modal(index);
        if std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1") {
            if let Some(ref modal) = app_state.pop_modal().cloned() {
                if modal.plan.blockers.is_empty() {
                    app_state.confirm_pop();
                } else {
                    eprintln!(
                        "[kagi] KAGI_AUTO_CONFIRM=1 but pop has {} blocker(s), skipping",
                        modal.plan.blockers.len()
                    );
                }
            }
        }
    }

    // ── W17-DISCARD / ADR-0046: headless discard ─────────────
    // KAGI_DISCARD=<path>     discard one unstaged file's changes
    // KAGI_DISCARD_ALL=1      discard ALL eligible unstaged files (one operation)
    // KAGI_AUTO_CONFIRM=1     (TEST-ONLY) actually execute after planning
    // Emits `[kagi] planned:/executed:/verified:` like the other KAGI_* paths.
    {
        let single = std::env::var("KAGI_DISCARD").ok().filter(|s| !s.is_empty());
        let all = std::env::var("KAGI_DISCARD_ALL").as_deref() == Ok("1");
        if single.is_some() || all {
            run_headless_discard(&repo_path, single, all);
        }
    }

    // ── T-COMMIT-010/011: headless amend plan / execute ──────
    // KAGI_AMEND=<mode>: mode ∈ {message, staged, both}. Build an amend plan and
    // log it.  KAGI_AMEND_MSG=<text> supplies the new message (for message/both).
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, drive the TWO-STAGE confirm
    // (arm + execute).  For fixture/tempdir testing only.  Do not set in normal use.
    if let Ok(mode_str) = std::env::var("KAGI_AMEND") {
        match kagi::git::ops::AmendMode::from_env_str(&mode_str) {
            Some(mode) => {
                let message = std::env::var("KAGI_AMEND_MSG").unwrap_or_default();
                app_state.open_amend_modal_with_message(mode, message);
                if std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1") {
                    if let Some(modal) = app_state.amend_modal().cloned() {
                        if modal.plan.blockers.is_empty() {
                            // Stage 1: arm the two-stage confirm.
                            app_state.confirm_amend();
                            // Stage 2: execute.
                            app_state.confirm_amend();
                        } else {
                            eprintln!(
                                "[kagi] KAGI_AUTO_CONFIRM=1 but amend has {} blocker(s), skipping",
                                modal.plan.blockers.len()
                            );
                        }
                    }
                }
            }
            None => {
                eprintln!(
                    "[kagi] KAGI_AMEND: invalid mode '{}' (use message|staged|both)",
                    mode_str
                );
            }
        }
    }

    // ── W2-DELETE: headless delete-branch plan / execute ─────
    // KAGI_DELETE_BRANCH=<name>: generate a delete-branch plan and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, execute immediately.
    // For fixture/tempdir testing only.  Do not set in normal use.
    if let Ok(del_branch) = std::env::var("KAGI_DELETE_BRANCH") {
        app_state.open_delete_branch_modal(&del_branch);

        let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
        if auto_confirm {
            if let Some(ref modal) = app_state.delete_branch_modal().cloned() {
                if modal.plan.blockers.is_empty() {
                    app_state.confirm_delete_branch();
                } else {
                    eprintln!(
                        "[kagi] KAGI_AUTO_CONFIRM=1 but delete-branch has {} blocker(s), skipping",
                        modal.plan.blockers.len()
                    );
                    record_headless_op(
                        "delete-branch",
                        modal.plan.current.clone(),
                        OpOutcome::Refused {
                            blockers: modal.plan.blockers.clone(),
                        },
                        &repo_path,
                    );
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
            if let Some(ref modal) = app_state.plan_modal().cloned() {
                if modal.plan.blockers.is_empty() {
                    app_state.confirm_checkout();
                } else {
                    eprintln!(
                        "[kagi] KAGI_AUTO_CONFIRM=1 but checkout has {} blocker(s), skipping",
                        modal.plan.blockers.len()
                    );
                    record_headless_op(
                        "checkout",
                        modal.plan.current.clone(),
                        OpOutcome::Refused {
                            blockers: modal.plan.blockers.clone(),
                        },
                        &repo_path,
                    );
                }
            }
        }
    }

    // ── T-CM-041: headless detached commit checkout plan / execute ─
    // KAGI_CHECKOUT_COMMIT=<row>: generate a detached checkout plan for the
    // commit row and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, proceed to execute.
    // For fixture/tempdir testing only. Do not set in normal use.
    if let Ok(row_str) = std::env::var("KAGI_CHECKOUT_COMMIT") {
        let row_index: usize = row_str.parse().unwrap_or(usize::MAX);
        let commit_id = app_state
            .active_view
            .details
            .get(row_index)
            .map(|detail| kagi::git::CommitId(detail.full_sha.as_ref().to_string()));

        match commit_id {
            Some(commit_id) => {
                app_state.open_checkout_commit_modal(commit_id);

                let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
                if auto_confirm {
                    if let Some(ref modal) = app_state.plan_modal().cloned() {
                        if modal.plan.blockers.is_empty() {
                            app_state.confirm_checkout();
                        } else {
                            eprintln!(
                                "[kagi] KAGI_AUTO_CONFIRM=1 but checkout-commit has {} blocker(s), skipping",
                                modal.plan.blockers.len()
                            );
                            record_headless_op(
                                "checkout-commit",
                                modal.plan.current.clone(),
                                OpOutcome::Refused {
                                    blockers: modal.plan.blockers.clone(),
                                },
                                &repo_path,
                            );
                        }
                    }
                }
            }
            None => {
                eprintln!(
                    "[kagi] KAGI_CHECKOUT_COMMIT: row={} out of range",
                    row_index
                );
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
                r.head()
                    .ok()
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
                            app_state.set_create_branch_modal(ui::CreateBranchModal {
                                input_state: None,
                                at: at.clone(),
                                start_title: String::new(),
                                input: branch_name.clone(),
                                checkout_after: false,
                                plan: Some(std::sync::Arc::new(plan.clone())),
                                error: None,
                                localized_blockers: Vec::new(),
                            });
                        }
                        if auto_confirm {
                            if plan.blockers.is_empty() {
                                // Preflight + execute.
                                let repo2 = git2::Repository::open(&repo_path).ok();
                                if let Some(r2) = repo2 {
                                    if let Err(e) = kagi::git::preflight_check(&r2, &plan) {
                                        let err_msg = format!("preflight failed: {}", e);
                                        klog!("{}", err_msg);
                                        record_headless_op(
                                            "create-branch",
                                            plan.current.clone(),
                                            OpOutcome::Failed { error: err_msg },
                                            &repo_path,
                                        );
                                    } else if let Err(e) =
                                        kagi::git::execute_create_branch(&r2, &branch_name, &at)
                                    {
                                        let err_msg = format!("create-branch failed: {}", e);
                                        klog!("{}", err_msg);
                                        record_headless_op(
                                            "create-branch",
                                            plan.current.clone(),
                                            OpOutcome::Failed { error: err_msg },
                                            &repo_path,
                                        );
                                    } else {
                                        eprintln!(
                                            "[kagi] executed: create-branch '{}' @ {}",
                                            branch_name,
                                            at.short()
                                        );
                                        // Verify.
                                        let repo3 = git2::Repository::open(&repo_path).ok();
                                        if let Some(r3) = repo3 {
                                            let exists = r3
                                                .find_branch(&branch_name, git2::BranchType::Local)
                                                .is_ok();
                                            if exists {
                                                eprintln!(
                                                    "[kagi] verified: branch '{}' exists",
                                                    branch_name
                                                );
                                            } else {
                                                eprintln!(
                                                    "[kagi] verify: branch '{}' NOT found",
                                                    branch_name
                                                );
                                            }
                                            // Log current branch to confirm HEAD unchanged.
                                            if let Ok(head_ref) = r3.head() {
                                                let cur = head_ref.shorthand().unwrap_or("?");
                                                eprintln!(
                                                    "[kagi] verified: current branch = {}",
                                                    cur
                                                );
                                            }
                                        }
                                        record_headless_op(
                                            "create-branch",
                                            plan.current.clone(),
                                            OpOutcome::Success {
                                                after: plan.predicted.clone(),
                                            },
                                            &repo_path,
                                        );
                                    }
                                }
                            } else {
                                eprintln!(
                                    "[kagi] KAGI_AUTO_CONFIRM=1 but create-branch has {} blocker(s), skipping",
                                    plan.blockers.len()
                                );
                                record_headless_op(
                                    "create-branch",
                                    plan.current.clone(),
                                    OpOutcome::Refused {
                                        blockers: plan.blockers.clone(),
                                    },
                                    &repo_path,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        klog!("plan: create-branch error: {}", e);
                    }
                }
            }
        } else {
            klog!("KAGI_CREATE_BRANCH: could not resolve HEAD commit");
        }
    }

    // ── T-CM-023: headless create-worktree plan / execute ─────
    // KAGI_PLAN_WORKTREE=<branch>:<path>: generate a create-worktree plan
    // using HEAD commit as the start point and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, execute immediately.
    if let Ok(spec) = std::env::var("KAGI_PLAN_WORKTREE") {
        let (branch_name, worktree_path) = match spec.split_once(':') {
            Some((branch, path)) => (branch.to_string(), path.to_string()),
            None => {
                klog!("KAGI_PLAN_WORKTREE: expected <branch>:<path>");
                run_app(app_state);
                return;
            }
        };
        let head_commit_id = git2::Repository::open(&repo_path).ok().and_then(|r| {
            r.head()
                .ok()
                .and_then(|h| h.target())
                .map(|oid| kagi::git::CommitId(oid.to_string()))
        });

        if let Some(at) = head_commit_id {
            let repo_for_plan = git2::Repository::open(&repo_path).ok();
            if let Some(repo) = repo_for_plan {
                match kagi::git::plan_create_worktree(&repo, &branch_name, &worktree_path, &at) {
                    Ok(plan) => {
                        eprintln!(
                            "[kagi] plan: create-worktree '{}' path='{}' blockers={} warnings={}",
                            branch_name,
                            worktree_path,
                            plan.blockers.len(),
                            plan.warnings.len()
                        );
                        let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
                        if auto_confirm {
                            if plan.blockers.is_empty() {
                                let repo2 = git2::Repository::open(&repo_path).ok();
                                if let Some(r2) = repo2 {
                                    if let Err(e) = kagi::git::preflight_check(&r2, &plan) {
                                        let err_msg = format!("preflight failed: {}", e);
                                        klog!("{}", err_msg);
                                        record_headless_op(
                                            "create-worktree",
                                            plan.current.clone(),
                                            OpOutcome::Failed { error: err_msg },
                                            &repo_path,
                                        );
                                    } else if let Err(e) = kagi::git::execute_create_worktree(
                                        &r2,
                                        &branch_name,
                                        &worktree_path,
                                        &at,
                                    ) {
                                        let err_msg = format!("create-worktree failed: {}", e);
                                        klog!("{}", err_msg);
                                        record_headless_op(
                                            "create-worktree",
                                            plan.current.clone(),
                                            OpOutcome::Failed { error: err_msg },
                                            &repo_path,
                                        );
                                    } else {
                                        eprintln!(
                                            "[kagi] executed: create-worktree '{}' path='{}' @ {}",
                                            branch_name,
                                            worktree_path,
                                            at.short()
                                        );
                                        let verify_path = {
                                            let path = PathBuf::from(&worktree_path);
                                            if path.is_absolute() {
                                                path
                                            } else {
                                                repo_path.join(path)
                                            }
                                        };
                                        match git2::Repository::open(&verify_path) {
                                            Ok(linked) => {
                                                let head = linked.head().ok().and_then(|h| {
                                                    h.shorthand().ok().map(|s| s.to_string())
                                                });
                                                eprintln!(
                                                    "[kagi] verified: worktree '{}' HEAD={}",
                                                    verify_path.display(),
                                                    head.unwrap_or_else(|| "?".to_string())
                                                );
                                            }
                                            Err(e) => eprintln!(
                                                "[kagi] verify: worktree open error: {}",
                                                e.message()
                                            ),
                                        }
                                        record_headless_op(
                                            "create-worktree",
                                            plan.current.clone(),
                                            OpOutcome::Success {
                                                after: plan.predicted.clone(),
                                            },
                                            &repo_path,
                                        );
                                        app_state.reload();
                                    }
                                }
                            } else {
                                eprintln!(
                                    "[kagi] KAGI_AUTO_CONFIRM=1 but create-worktree has {} blocker(s), skipping",
                                    plan.blockers.len()
                                );
                                record_headless_op(
                                    "create-worktree",
                                    plan.current.clone(),
                                    OpOutcome::Refused {
                                        blockers: plan.blockers.clone(),
                                    },
                                    &repo_path,
                                );
                            }
                        }
                    }
                    Err(e) => klog!("plan: create-worktree error: {}", e),
                }
            }
        } else {
            klog!("KAGI_PLAN_WORKTREE: could not resolve HEAD commit");
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
                klog!("KAGI_STASH_PUSH: repo open error: {}", e.message());
                run_app(app_state);
                return;
            }
        };

        match kagi::git::plan_stash_push(&mut repo_sp, None, true) {
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
                                eprintln!(
                                    "[kagi] KAGI_STASH_PUSH: repo open error: {}",
                                    e.message()
                                );
                                run_app(app_state);
                                return;
                            }
                        };
                        if let Err(e) =
                            kagi::git::preflight_check_stash(&mut repo2, &plan, stash_count_at_plan)
                        {
                            let err_msg = format!("preflight failed: {}", e);
                            klog!("{}", err_msg);
                            record_headless_op(
                                "stash-push",
                                plan.current.clone(),
                                OpOutcome::Failed { error: err_msg },
                                &repo_path,
                            );
                        } else if let Err(e) = kagi::git::execute_stash_push(&mut repo2, None, true)
                        {
                            let err_msg = format!("stash-push failed: {}", e);
                            klog!("{}", err_msg);
                            record_headless_op(
                                "stash-push",
                                plan.current.clone(),
                                OpOutcome::Failed { error: err_msg },
                                &repo_path,
                            );
                        } else {
                            klog!("executed: stash-push");
                            // Verify: working tree clean + stash count.
                            let mut repo3 = match git2::Repository::open(&repo_path) {
                                Ok(r) => r,
                                Err(e) => {
                                    klog!("verify: repo open error: {}", e.message());
                                    run_app(app_state);
                                    return;
                                }
                            };
                            match snapshot(&mut repo3, 10_000) {
                                Ok(snap) => {
                                    let clean = !snap.status.is_dirty();
                                    let stash_count = snap.stashes.len();
                                    if clean {
                                        klog!("verified: working tree clean");
                                    } else {
                                        klog!("verify: working tree NOT clean");
                                    }
                                    klog!("verified: stash count={}", stash_count);
                                    record_headless_op(
                                        "stash-push",
                                        plan.current.clone(),
                                        OpOutcome::Success {
                                            after: StateSummary {
                                                head: snap.head.display(),
                                                dirty: if clean {
                                                    "clean".to_string()
                                                } else {
                                                    "dirty".to_string()
                                                },
                                            },
                                        },
                                        &repo_path,
                                    );
                                }
                                Err(e) => {
                                    klog!("verify: snapshot error: {}", e);
                                    record_headless_op(
                                        "stash-push",
                                        plan.current.clone(),
                                        OpOutcome::Success {
                                            after: plan.predicted.clone(),
                                        },
                                        &repo_path,
                                    );
                                }
                            }
                        }
                    } else {
                        eprintln!(
                            "[kagi] KAGI_AUTO_CONFIRM=1 but stash-push has {} blocker(s), skipping",
                            plan.blockers.len()
                        );
                        record_headless_op(
                            "stash-push",
                            plan.current.clone(),
                            OpOutcome::Refused {
                                blockers: plan.blockers.clone(),
                            },
                            &repo_path,
                        );
                    }
                } else {
                    // Without auto-confirm, surface the modal so it can be inspected headlessly.
                    app_state.set_stash_push_modal(StashPushModal {
                        input_state: None,
                        input: String::new(),
                        plan: Some(std::sync::Arc::new(plan)),
                        error: None,
                    });
                }
            }
            Err(e) => {
                klog!("plan: stash-push error: {}", e);
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
                klog!("KAGI_STASH_APPLY: invalid index '{}'", idx_str);
                run_app(app_state);
                return;
            }
        };

        let mut repo_sa = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("KAGI_STASH_APPLY: repo open error: {}", e.message());
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
                                eprintln!(
                                    "[kagi] KAGI_STASH_APPLY: repo open error: {}",
                                    e.message()
                                );
                                run_app(app_state);
                                return;
                            }
                        };
                        if let Err(e) =
                            kagi::git::preflight_check_stash(&mut repo2, &plan, stash_count_at_plan)
                        {
                            let err_msg = format!("preflight failed: {}", e);
                            klog!("{}", err_msg);
                            record_headless_op(
                                "stash-apply",
                                plan.current.clone(),
                                OpOutcome::Failed { error: err_msg },
                                &repo_path,
                            );
                        } else if let Err(e) = kagi::git::execute_stash_apply(&mut repo2, index) {
                            let err_msg = format!("stash-apply failed: {}", e);
                            klog!("{}", err_msg);
                            record_headless_op(
                                "stash-apply",
                                plan.current.clone(),
                                OpOutcome::Failed { error: err_msg },
                                &repo_path,
                            );
                        } else {
                            klog!("executed: stash-apply index={}", index);
                            // Verify: working tree dirty + stash still present.
                            let mut repo3 = match git2::Repository::open(&repo_path) {
                                Ok(r) => r,
                                Err(e) => {
                                    klog!("verify: repo open error: {}", e.message());
                                    run_app(app_state);
                                    return;
                                }
                            };
                            match snapshot(&mut repo3, 10_000) {
                                Ok(snap) => {
                                    let is_dirty = snap.status.is_dirty();
                                    let stash_count = snap.stashes.len();
                                    if is_dirty {
                                        klog!("verified: working tree dirty (restored)");
                                    } else {
                                        eprintln!(
                                            "[kagi] verify: working tree NOT dirty after apply"
                                        );
                                    }
                                    eprintln!(
                                        "[kagi] verified: stash count={} (entry preserved)",
                                        stash_count
                                    );
                                    record_headless_op(
                                        "stash-apply",
                                        plan.current.clone(),
                                        OpOutcome::Success {
                                            after: StateSummary {
                                                head: snap.head.display(),
                                                dirty: if is_dirty {
                                                    "dirty".to_string()
                                                } else {
                                                    "clean".to_string()
                                                },
                                            },
                                        },
                                        &repo_path,
                                    );
                                }
                                Err(e) => {
                                    klog!("verify: snapshot error: {}", e);
                                    record_headless_op(
                                        "stash-apply",
                                        plan.current.clone(),
                                        OpOutcome::Success {
                                            after: plan.predicted.clone(),
                                        },
                                        &repo_path,
                                    );
                                }
                            }
                        }
                    } else {
                        eprintln!(
                            "[kagi] KAGI_AUTO_CONFIRM=1 but stash-apply has {} blocker(s), skipping",
                            plan.blockers.len()
                        );
                        record_headless_op(
                            "stash-apply",
                            plan.current.clone(),
                            OpOutcome::Refused {
                                blockers: plan.blockers.clone(),
                            },
                            &repo_path,
                        );
                    }
                } else {
                    // Without auto-confirm, surface the modal.
                    app_state.set_stash_apply_modal(StashApplyModal {
                        index,
                        plan: std::sync::Arc::new(plan),
                        error: None,
                    });
                }
            }
            Err(e) => {
                klog!("plan: stash-apply error: {}", e);
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
                klog!("KAGI_CHERRY_PICK: repo open error: {}", e.message());
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
                    klog!("plan: blocker: {}", b);
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
                                eprintln!(
                                    "[kagi] KAGI_CHERRY_PICK: repo open error: {}",
                                    e.message()
                                );
                                run_app(app_state);
                                return;
                            }
                        };
                        if let Err(e) = kagi::git::preflight_check(&repo2, &plan) {
                            let err_msg = format!("preflight failed: {}", e);
                            klog!("{}", err_msg);
                            record_headless_op(
                                "cherry-pick",
                                plan.current.clone(),
                                OpOutcome::Failed { error: err_msg },
                                &repo_path,
                            );
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
                                            eprintln!(
                                                "[kagi] verify: repo open error: {}",
                                                e.message()
                                            );
                                            run_app(app_state);
                                            return;
                                        }
                                    };
                                    match snapshot(&mut repo3, 10_000) {
                                        Ok(snap) => {
                                            if let Head::Attached { target, branch } = &snap.head {
                                                if *target == new_id.0 {
                                                    eprintln!(
                                                        "[kagi] verified: HEAD={} on {}",
                                                        new_id.short(),
                                                        branch
                                                    );
                                                } else {
                                                    eprintln!(
                                                        "[kagi] verify: HEAD={} (expected {})",
                                                        &target[..8.min(target.len())],
                                                        new_id.short()
                                                    );
                                                }
                                            }
                                            let is_clean = !snap.status.is_dirty();
                                            if is_clean {
                                                klog!("verified: working tree clean after cherry-pick");
                                            } else {
                                                klog!("verify: working tree dirty after cherry-pick (unexpected)");
                                            }
                                            // Log first commit message for manual inspection.
                                            if let Some(c) = snap.commits.first() {
                                                eprintln!(
                                                    "[kagi] verified: new HEAD message: {}",
                                                    c.summary
                                                );
                                            }
                                            record_headless_op(
                                                "cherry-pick",
                                                plan.current.clone(),
                                                OpOutcome::Success {
                                                    after: StateSummary {
                                                        head: snap.head.display(),
                                                        dirty: if is_clean {
                                                            "clean".to_string()
                                                        } else {
                                                            "dirty".to_string()
                                                        },
                                                    },
                                                },
                                                &repo_path,
                                            );
                                        }
                                        Err(e) => {
                                            klog!("verify: snapshot error: {}", e);
                                            record_headless_op(
                                                "cherry-pick",
                                                plan.current.clone(),
                                                OpOutcome::Success {
                                                    after: plan.predicted.clone(),
                                                },
                                                &repo_path,
                                            );
                                        }
                                    }
                                    app_state.reload();
                                }
                                Err(e) => {
                                    let err_msg = format!("cherry-pick execute failed: {}", e);
                                    klog!("{}", err_msg);
                                    record_headless_op(
                                        "cherry-pick",
                                        plan.current.clone(),
                                        OpOutcome::Failed { error: err_msg },
                                        &repo_path,
                                    );
                                }
                            }
                        }
                    } else {
                        eprintln!(
                            "[kagi] KAGI_AUTO_CONFIRM=1 but cherry-pick has {} blocker(s), skipping",
                            plan.blockers.len()
                        );
                        record_headless_op(
                            "cherry-pick",
                            plan.current.clone(),
                            OpOutcome::Refused {
                                blockers: plan.blockers.clone(),
                            },
                            &repo_path,
                        );
                    }
                } else {
                    // Without auto-confirm, surface the cherry-pick modal.
                    app_state.set_cherry_pick_modal(CherryPickModal {
                        commit_id,
                        plan: std::sync::Arc::new(plan),
                        error: None,
                    });
                }
            }
            Err(e) => {
                klog!("plan: cherry-pick error: {}", e);
            }
        }
    }

    // ── T-CM-034: headless revert plan / execute ────────────
    // KAGI_REVERT=<row-or-sha>: generate a revert plan and log it.
    // KAGI_AUTO_CONFIRM=1: (TEST-ONLY) if no blockers, execute immediately.
    if let Ok(sha_str) = std::env::var("KAGI_REVERT") {
        let commit_id = match sha_str.parse::<usize>() {
            Ok(row) => match app_state.active_view.details.get(row) {
                Some(detail) => {
                    eprintln!(
                        "[kagi] KAGI_REVERT: row {} -> {}",
                        row,
                        detail.full_sha.as_ref()
                    );
                    kagi::git::CommitId(detail.full_sha.to_string())
                }
                None => {
                    klog!("KAGI_REVERT: row {} out of range", row);
                    run_app(app_state);
                    return;
                }
            },
            Err(_) => kagi::git::CommitId(sha_str.clone()),
        };
        let repo_revert = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("KAGI_REVERT: repo open error: {}", e.message());
                run_app(app_state);
                return;
            }
        };

        match kagi::git::plan_revert(&repo_revert, &commit_id) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: revert {} blockers={} preview_files={}",
                    commit_id.short(),
                    plan.blockers.len(),
                    plan.preview_files.len()
                );
                for b in &plan.blockers {
                    klog!("plan: blocker: {}", b);
                }
                for w in &plan.warnings {
                    klog!("plan: warning: {}", w);
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
                        let repo2 = match git2::Repository::open(&repo_path) {
                            Ok(r) => r,
                            Err(e) => {
                                klog!("KAGI_REVERT: repo open error: {}", e.message());
                                run_app(app_state);
                                return;
                            }
                        };
                        if let Err(e) = kagi::git::preflight_check(&repo2, &plan) {
                            let err_msg = format!("preflight failed: {}", e);
                            klog!("{}", err_msg);
                            record_headless_op(
                                "revert",
                                plan.current.clone(),
                                OpOutcome::Failed { error: err_msg },
                                &repo_path,
                            );
                        } else {
                            match kagi::git::execute_revert(&repo2, &commit_id) {
                                Ok(new_id) => {
                                    eprintln!(
                                        "[kagi] executed: revert {} -> {}",
                                        commit_id.short(),
                                        new_id.short()
                                    );
                                    let mut repo3 = match git2::Repository::open(&repo_path) {
                                        Ok(r) => r,
                                        Err(e) => {
                                            eprintln!(
                                                "[kagi] verify: repo open error: {}",
                                                e.message()
                                            );
                                            run_app(app_state);
                                            return;
                                        }
                                    };
                                    match snapshot(&mut repo3, 10_000) {
                                        Ok(snap) => {
                                            if let Head::Attached { target, branch } = &snap.head {
                                                if *target == new_id.0 {
                                                    eprintln!(
                                                        "[kagi] verified: revert HEAD={} on {}",
                                                        new_id.short(),
                                                        branch
                                                    );
                                                } else {
                                                    eprintln!(
                                                        "[kagi] verify: HEAD={} (expected {})",
                                                        &target[..8.min(target.len())],
                                                        new_id.short()
                                                    );
                                                }
                                            }
                                            let is_clean = !snap.status.is_dirty();
                                            eprintln!(
                                                "[kagi] verified: working tree {} after revert",
                                                if is_clean { "clean" } else { "dirty" }
                                            );
                                            if let Some(c) = snap.commits.first() {
                                                eprintln!(
                                                    "[kagi] verified: new HEAD message: {}",
                                                    c.summary
                                                );
                                            }
                                            record_headless_op(
                                                "revert",
                                                plan.current.clone(),
                                                OpOutcome::Success {
                                                    after: StateSummary {
                                                        head: snap.head.display(),
                                                        dirty: if is_clean {
                                                            "clean".to_string()
                                                        } else {
                                                            "dirty".to_string()
                                                        },
                                                    },
                                                },
                                                &repo_path,
                                            );
                                        }
                                        Err(e) => {
                                            klog!("verify: snapshot error: {}", e);
                                            record_headless_op(
                                                "revert",
                                                plan.current.clone(),
                                                OpOutcome::Success {
                                                    after: plan.predicted.clone(),
                                                },
                                                &repo_path,
                                            );
                                        }
                                    }
                                    app_state.reload();
                                }
                                Err(e) => {
                                    let err_msg = format!("revert execute failed: {}", e);
                                    klog!("{}", err_msg);
                                    record_headless_op(
                                        "revert",
                                        plan.current.clone(),
                                        OpOutcome::Failed { error: err_msg },
                                        &repo_path,
                                    );
                                }
                            }
                        }
                    } else {
                        eprintln!(
                            "[kagi] KAGI_AUTO_CONFIRM=1 but revert has {} blocker(s), skipping",
                            plan.blockers.len()
                        );
                        record_headless_op(
                            "revert",
                            plan.current.clone(),
                            OpOutcome::Refused {
                                blockers: plan.blockers.clone(),
                            },
                            &repo_path,
                        );
                    }
                } else {
                    app_state.set_revert_modal(RevertModal {
                        commit_id,
                        plan: std::sync::Arc::new(plan),
                        error: None,
                    });
                }
            }
            Err(e) => {
                klog!("plan: revert error: {}", e);
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
                        klog!("staged: {}", path_str);
                        panel.reload_status(&repo_path);
                        eprintln!(
                            "[kagi] commit-panel: unstaged={} staged={}",
                            panel.unstaged.len(),
                            panel.staged.len()
                        );
                    }
                    Err(e) => klog!("KAGI_STAGE_FILE error: {}", e),
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
                        klog!("unstaged: {}", path_str);
                        panel.reload_status(&repo_path);
                        eprintln!(
                            "[kagi] commit-panel: unstaged={} staged={}",
                            panel.unstaged.len(),
                            panel.staged.len()
                        );
                    }
                    Err(e) => klog!("KAGI_UNSTAGE_FILE error: {}", e),
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
                            klog!("plan: warning: {}", w);
                        }
                        for b in &plan.blockers {
                            klog!("plan: blocker: {}", b);
                        }

                        let auto_confirm = std::env::var("KAGI_AUTO_CONFIRM").as_deref() == Ok("1");
                        if auto_confirm && plan.blockers.is_empty() {
                            let repo_e = git2::Repository::open(&repo_path).ok();
                            if let Some(repo_e) = repo_e {
                                match kagi::git::execute_commit(&repo_e, &commit_msg) {
                                    Ok(new_id) => {
                                        klog!("executed: commit {}", new_id.short());
                                        // Verify: check new commit exists and untracked remain.
                                        let mut repo_v = match git2::Repository::open(&repo_path) {
                                            Ok(r) => r,
                                            Err(e) => {
                                                eprintln!(
                                                    "[kagi] verify: repo open error: {}",
                                                    e.message()
                                                );
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
                                                    eprintln!(
                                                        "[kagi] verified: HEAD message: {}",
                                                        c.summary
                                                    );
                                                }
                                                let is_dirty = snap.status.is_dirty();
                                                if is_dirty {
                                                    klog!("verified: working tree dirty (unstaged remain)");
                                                } else {
                                                    eprintln!(
                                                        "[kagi] verified: working tree clean"
                                                    );
                                                }
                                                record_headless_op(
                                                    "commit",
                                                    StateSummary {
                                                        head: snap.head.display(),
                                                        dirty: plan.current.dirty.clone(),
                                                    },
                                                    OpOutcome::Success {
                                                        after: StateSummary {
                                                            head: snap.head.display(),
                                                            dirty: if is_dirty {
                                                                "dirty".to_string()
                                                            } else {
                                                                "clean".to_string()
                                                            },
                                                        },
                                                    },
                                                    &repo_path,
                                                );
                                            }
                                            Err(e) => {
                                                klog!("verify: snapshot error: {}", e)
                                            }
                                        }
                                    }
                                    Err(e) => klog!("execute_commit error: {}", e),
                                }
                            }
                        } else if auto_confirm && !plan.blockers.is_empty() {
                            eprintln!(
                                "[kagi] KAGI_AUTO_CONFIRM=1 but commit has {} blocker(s), skipping",
                                plan.blockers.len()
                            );
                        }
                    }
                    Err(e) => klog!("plan_commit error: {}", e),
                }
            }
        }

        // Set up commit panel state in app_state for UI inspection.
        app_state.commit_panel = Some(panel);
        app_state.commit_panel_open = true;
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
        };
        // W2-STATUS: the height is resolved at first render (18% of viewport);
        // before that the field holds the 0.0 sentinel.
        let h_label = if h > 0.0 {
            format!("{}", h)
        } else {
            "18%-of-viewport".to_string()
        };
        eprintln!(
            "[kagi] bottom-panel: open height={} tab={}",
            h_label, tab_label
        );
        klog!("oplog-tab: {} entries", app_state.op_entries.len());
    }

    // ── W5-MENU: headless command-registry state dump ─────────
    // KAGI_MENU_DUMP=1: log every command's id/label/keystroke/state.  Reflects
    // the current app state (repo open, selection via KAGI_SELECT_FIRST, etc.).
    if std::env::var("KAGI_MENU_DUMP").as_deref() == Ok("1") {
        ui::commands::dump_menu_states(&app_state);
    }

    run_app(app_state);
}
