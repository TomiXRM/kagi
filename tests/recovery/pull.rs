//! Dirty Pull GUI E2E scenarios for ADR-0189.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use gpui::VisualTestAppContext;
use kagi_domain::plan_note::{PlanNote, PullNote};
use kagi_git::oplog::{read_oplog_tail_for_repo, recovery, OpOutcome};

use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_operations::{press_enter, wait_idle};

fn output(repo: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command");
    assert!(result.status.success(), "git {args:?}: {:?}", result.stderr);
    String::from_utf8(result.stdout)
        .expect("utf8 git output")
        .trim()
        .to_string()
}

fn records(repo: &Path, op: &str) -> Vec<kagi_git::oplog::OpLogEntry> {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|entry| entry.op == op)
        .collect()
}

fn assert_stash_push_recovery(repo: &Path) {
    let pushes = records(repo, "stash-push");
    assert_eq!(pushes.len(), 1, "auto-stash must be recorded exactly once");
    assert!(
        pushes[0]
            .recovery
            .iter()
            .any(|handle| handle.kind == recovery::STASH && handle.oid.len() == 40),
        "the created stash OID must be a typed recovery handle"
    );
}

pub fn scenario_pull_auto_stash_success(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let gitlink_path = "PCB/EM2/SM20/SteppingDriverBoard_L/.history";
    let cacheinfo = format!("160000,7489b69c1ec9e5763a469d9b367deac0aee76bc4,{gitlink_path}");
    git(repo, &["update-index", "--add", "--cacheinfo", &cacheinfo]);
    git(repo, &["commit", "-qm", "add unpopulated gitlink"]);
    std::fs::create_dir_all(repo.join(gitlink_path)).unwrap();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    let upstream_head = output(&other, &["rev-parse", "HEAD"]);

    std::fs::write(repo.join("README.md"), "local staged change\n").unwrap();
    std::fs::write(repo.join("scratch.txt"), "local untracked change\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "fixture Pull must be executable"
        );
        assert!(
            modal.error.is_none(),
            "confirmation must not begin as an error"
        );
    });

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.run_until_parked();

    assert_eq!(output(repo, &["rev-parse", "HEAD"]), upstream_head);
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "local staged change\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("scratch.txt")).unwrap(),
        "local untracked change\n"
    );
    assert!(output(repo, &["stash", "list"]).is_empty());
    assert_stash_push_recovery(repo);
    assert!(
        records(repo, "pull")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Success { .. })),
        "Pull success must be durable"
    );
    cx.read(|cx| assert!(app.read(cx).pull_modal().is_none()));

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_auto_stash_success: dirty changes restored after Pull");
}

/// ADR-0196 Wave 3: the pull workflow presents the backend's own receipts.
///
/// A dirty pull is three writes, each of which already records itself. The UI
/// used to add a fourth entry it synthesized from the composite result — so a
/// successful auto-stash pull wrote two `pull` rows, one of them not produced
/// by any execution. This asserts the three children in execution order, one
/// durable `pull` entry, and a panel row that *is* the durable entry.
pub fn scenario_pull_presents_backend_receipts(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);

    std::fs::write(repo.join("README.md"), "local staged change\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx)
                .pull_modal()
                .is_some_and(|modal| modal.auto_stash && modal.plan.blockers.is_empty()),
            "the fixture must produce a confirmable auto-stash pull"
        );
    });

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    // The tail reads newest first; the workflow ran the other way round.
    let durable = read_oplog_tail_for_repo(repo, 100);
    let workflow: Vec<&str> = durable
        .iter()
        .rev()
        .map(|entry| entry.op.as_str())
        .filter(|op| matches!(*op, "stash-push" | "pull" | "stash-pop"))
        .collect();
    assert_eq!(
        workflow,
        vec!["stash-push", "pull", "stash-pop"],
        "a dirty pull records its three children, in execution order"
    );
    let pulls = records(repo, "pull");
    assert_eq!(
        pulls.len(),
        1,
        "the one pull entry is the backend's; the UI must not synthesize a copy"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        let panel = app.op_log.as_ref().unwrap().read(cx);
        // The panel is seeded from the whole log, so scope it to this fixture.
        let mine: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.repo == pulls[0].repo)
            .collect();
        let shown: Vec<_> = mine.iter().filter(|entry| entry.op == "pull").collect();
        assert_eq!(
            shown.len(),
            1,
            "the panel shows the receipt once, not a UI copy"
        );
        assert_eq!(
            shown[0].id, pulls[0].id,
            "the panel entry is the durable receipt"
        );
        assert_eq!(
            shown[0].failure_code, pulls[0].failure_code,
            "the presented entry keeps the backend's failure code"
        );
        for op in ["stash-push", "pull", "stash-pop"] {
            let durable = records(repo, op);
            assert_eq!(durable.len(), 1, "{op} is recorded once");
            assert!(
                mine.iter().any(|entry| entry.id == durable[0].id),
                "the panel must show the durable {op} receipt"
            );
        }
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_presents_backend_receipts: three child receipts, no UI-synthesized entry"
    );
}

pub fn scenario_pull_auto_stash_failure_restores(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let bare_path = bare.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(repo.join("README.md"), "restore after failed Pull\n").unwrap();
    std::fs::write(repo.join("scratch.txt"), "restore untracked\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "failure must come from execution"
        );
    });

    // The remote disappears only after the confirmation exists: since #625 a
    // dirty Pull fetches *before* it opens the modal (ADR-0192), so removing it
    // up front would fail that fetch and there would be no modal to confirm —
    // a different scenario. Execution's own fetch is the one that must fail
    // here.
    std::fs::remove_dir_all(&bare).unwrap();

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "restore after failed Pull\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("scratch.txt")).unwrap(),
        "restore untracked\n"
    );
    assert!(output(repo, &["stash", "list"]).is_empty());
    assert_stash_push_recovery(repo);
    assert!(
        records(repo, "pull")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Failed { .. })),
        "Pull failure must be durable"
    );
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .pull_modal()
            .expect("failed Pull modal must survive watcher reload");
        assert!(modal.error.is_some(), "failed Pull must show its error");
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_auto_stash_failure_restores: failed Pull restores changes and keeps its modal"
    );
}

/// A repository whose working tree edits the same line `origin/main` moved:
/// `shared.txt` collides, and the collision is a real content conflict.
///
/// Deliberately **not** fetched: `origin/main` is unknown to the repository
/// when the Pull button is pressed, so a path can only be named if the UI
/// fetched first (ADR-0192).
fn overlap_fixture(repo: &Path, remote_root: &Path) {
    let bare = remote_root.join("origin.git");
    let other = remote_root.join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    std::fs::write(repo.join("shared.txt"), "base\n").unwrap();
    git(repo, &["add", "shared.txt"]);
    git(repo, &["commit", "-qm", "add shared.txt"]);
    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root, &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("shared.txt"), "base\nupstream edit\n").unwrap();
    git(&other, &["add", "shared.txt"]);
    git(
        &other,
        &["commit", "-q", "-m", "upstream touches shared.txt"],
    );
    git(&other, &["push", "-q", "origin", "main"]);
    // The same line, edited here and not committed: the restore conflicts.
    std::fs::write(repo.join("shared.txt"), "base\nlocal edit\n").unwrap();
}

/// #625: a dirty Pull whose dirty path is also changed upstream must name that
/// path in the confirmation modal — *before* the user confirms.
///
/// The pull is a fast-forward, so neither the behind count nor
/// `MergePrediction` can see the collision: the path appears only because the
/// UI fetched first and the plan merged the two sides in memory (ADR-0192) —
/// exactly what used to surface after confirming, when the auto-stash failed
/// to restore.
pub fn scenario_pull_auto_stash_overlap_preview(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "the collision is a warning, not a refusal: {:?}",
            modal.plan.blockers
        );
        let shown: String = modal
            .plan
            .blockers
            .iter()
            .chain(modal.plan.warnings.iter())
            .map(|note| note.message_en())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            shown.contains("shared.txt"),
            "the modal must name the colliding path before confirmation:\n{shown}"
        );
        assert!(
            modal
                .plan
                .warnings
                .iter()
                .any(|note| matches!(note, PlanNote::Pull(PullNote::RestoreConflict { .. }))),
            "the collision must travel as a typed note: {:?}",
            modal.plan.warnings
        );
        // The auto-stash swap must not have dropped it (#625 Part 3).
        assert!(
            modal
                .plan
                .warnings
                .iter()
                .any(|note| matches!(note, PlanNote::Pull(PullNote::AutoStash { .. }))),
            "the auto-stash summary still belongs in the modal: {:?}",
            modal.plan.warnings
        );
    });

    // The regression PM caught: the confirmation opens and then a reload lands
    // on top of it. That reload is *caused by kagi's own fetch* (the watcher
    // sees `.git` change), and the modal-clearing sweep in `apply_reload_data`
    // wiped it ~0.5s later — Pull looked dead: pressed, nothing on screen.
    // Drive the same external reload twice, since the watcher can fire more
    // than once, and require the confirmation to still be there and still name
    // the path.
    for round in 1..=2 {
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            let modal = app
                .read(cx)
                .pull_modal()
                .unwrap_or_else(|| panic!("reload {round} closed the Pull confirmation"));
            assert!(modal.auto_stash, "round {round}");
            let shown: String = modal
                .plan
                .warnings
                .iter()
                .map(|note| note.message_en())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                shown.contains("shared.txt"),
                "reload {round} must keep the colliding path named:\n{shown}"
            );
        });
    }

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_auto_stash_overlap_preview: the modal names the colliding path and survives reloads"
    );
}

/// #625 P1-a: a Pull confirmation is delivered to the tab that asked for it,
/// even when that tab is not on screen while the fetch finishes.
///
/// Three earlier attempts fixed one branch each and left another: a bare flag
/// was cleared by a background tab's reload, then consumed only while the tab
/// was active, so "press Pull, switch tabs, come back" showed nothing at all.
/// The request now travels inside its own fetch task and parks for its tab.
pub fn scenario_pull_confirm_parks_for_its_tab(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());
    let other_tab = build_fixture();

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_tab.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    // Tab A presses Pull, then the user leaves for tab B — both synchronous, so
    // the fetch task is queued and has not run yet. The executor is driven only
    // after the switch, which is what makes "the fetch finishes while another
    // tab is on screen" the certain order rather than a hoped-for one.
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight,
            "the confirmation must be waiting on a fetch"
        );
        app.switch_repo(1, cx);
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_none(),
            "tab A's confirmation must not open over tab B"
        );
        assert!(
            !app.read(cx).pending_pull_confirm.is_empty(),
            "it must be parked for the tab that asked, not dropped"
        );
    });

    // Coming back to tab A delivers it.
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .pull_modal()
            .expect("returning to tab A must show the confirmation it asked for");
        let shown: String = modal
            .plan
            .warnings
            .iter()
            .map(|note| note.message_en())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(shown.contains("shared.txt"), "{shown}");
        assert!(
            app.read(cx).pending_pull_confirm.is_empty(),
            "delivery consumes the parked request"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_confirm_parks_for_its_tab: the confirmation waits for the tab that asked"
    );
}

/// #625 P2: a modal opened while the pre-Pull fetch runs must not be replaced.
///
/// "One modal at a time" is structural (ADR-0093), so the completion cannot
/// simply set its own: the branch-name the user is typing would vanish.
pub fn scenario_pull_confirm_yields_to_another_modal(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    let head = output(repo, &["rev-parse", "HEAD"]);
    // Pull, then a different modal — both before the executor runs the fetch,
    // so the completion certainly lands with the other modal already open.
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight,
            "the confirmation must be waiting on a fetch"
        );
        app.open_create_branch_modal(kagi_git::CommitId(head.clone()), cx);
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    cx.read(|cx| {
        // Both halves of "one modal at a time": the request yields instead of
        // replacing what the user opened, and what the user opened is still
        // there — the fetch's own reload no longer sweeps a pure-input modal.
        assert!(
            app.read(cx).create_branch_modal().is_some(),
            "the modal opened during the fetch must still be on screen"
        );
        assert!(
            app.read(cx).pull_modal().is_none(),
            "the Pull confirmation must not take over the modal slot"
        );
        assert!(
            app.read(cx).pending_pull_confirm.is_empty(),
            "a request for the tab on screen is cancelled, not parked"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_confirm_yields_to_another_modal: a modal opened during the fetch is kept"
    );
}

/// #625 P1: the confirmation's promise is checked before anything is stashed.
///
/// After the modal is on screen an editor saves *another* path the update also
/// changes. Stashing first would hide it from every downstream guard — the
/// preflight and the execute-time dirty-path check both see a clean tree — and
/// only the restore would conflict, unannounced. The run must refuse instead.
pub fn scenario_pull_refuses_when_the_dirty_set_moved(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());
    // A second path the upstream also changed, still clean at confirmation time.
    let other = remote_root.path().join("other");
    std::fs::write(other.join("second.txt"), "upstream second\n").unwrap();
    git(&other, &["add", "second.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream adds second.txt"]);
    git(&other, &["push", "-q", "origin", "main"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_some(),
            "the confirmation must be on screen first"
        );
    });

    // The promise goes stale: an editor writes a path the modal never named.
    std::fs::write(repo.join("second.txt"), "local second\n").unwrap();

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert!(
        output(repo, &["stash", "list"]).is_empty(),
        "nothing may be stashed once the confirmation is stale"
    );
    assert_eq!(
        output(repo, &["rev-parse", "HEAD"]),
        output(repo, &["rev-parse", "HEAD@{0}"]),
        "and nothing may be pulled"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("second.txt")).unwrap(),
        "local second\n",
        "the work the user did after confirming is untouched"
    );
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .pull_modal()
            .expect("the modal stays, carrying the refusal");
        let error = modal.error.clone().expect("a refusal message");
        assert!(
            error.contains("changed after this confirmation"),
            "the refusal must say why: {error}"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_refuses_when_the_dirty_set_moved: a stale confirmation stashes nothing"
    );
}
