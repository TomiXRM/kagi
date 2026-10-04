//! #358 / ADR-0205: Analyze's Health axis suggests a fix and runs nothing
//! until it is confirmed.
//!
//! The fixture has history and no commit-graph. Opening Analyze reads its
//! health; the Health axis' "Enable…" asks the app to plan the fix, which
//! opens the shared plan card. Until that card is confirmed — here with a
//! real root Enter — no commit-graph exists, no receipt is written and the
//! repository is unchanged. A cancelled card changes nothing either.
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::{ecosystem::EcosystemView, KagiApp};
use kagi_domain::hotspot::EcosystemMode;
use kagi_domain::plan_note::MaintenanceTitle;
use kagi_domain::repo_health::{HealthFinding, HealthFix};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use kagi_ui_core::i18n::{self, plan::maintenance, Lang};
use std::path::Path;
use std::time::{Duration, Instant};

fn commit_graph_exists(repo: &Path) -> bool {
    let info = repo.join(".git/objects/info");
    info.join("commit-graph").exists() || info.join("commit-graphs/commit-graph-chain").exists()
}

fn receipts(repo: &Path) -> Vec<OpOutcome> {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "write-commit-graph")
        .map(|entry| entry.outcome)
        .collect()
}

fn wait_until(
    cx: &mut VisualTestAppContext,
    what: &str,
    done: impl Fn(&mut VisualTestAppContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "{what} did not happen");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn health(
    cx: &mut VisualTestAppContext,
    view: &Entity<EcosystemView>,
) -> Option<Vec<HealthFinding>> {
    cx.read(|cx| {
        view.read(cx)
            .health()
            .and_then(|h| h.as_ref().ok().cloned())
    })
}

fn ask_to_fix(cx: &mut VisualTestAppContext, view: &Entity<EcosystemView>) {
    view.update(cx, |view, cx| {
        view.request_health_fix(HealthFix::WriteCommitGraph, cx)
    });
    cx.run_until_parked();
}

/// Observe the *drawn* confirmation, AFTER chip, accessible row and Copy all.
/// The plan's English prediction remains untouched by language switching.
fn assert_plan_presentation(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    title: &MaintenanceTitle,
) {
    for lang in [Lang::En, Lang::Ja] {
        let (confirm, expected_chip, detail_phrase) = match (title, lang) {
            (MaintenanceTitle::WriteCommitGraph, Lang::En) => (
                "Write commit-graph",
                "commit-graph written",
                "every reachable commit; working tree unchanged",
            ),
            (MaintenanceTitle::WriteCommitGraph, Lang::Ja) => (
                "commit-graph を書き込む",
                "commit-graph 書き込み済み",
                "すべてのコミットの commit-graph を書き込みます。作業ツリーは変更されません",
            ),
            (MaintenanceTitle::EnableFsmonitor, Lang::En) => (
                "Enable fsmonitor",
                "fsmonitor enabled",
                "core.fsmonitor = true (local config); working tree unchanged",
            ),
            (MaintenanceTitle::EnableFsmonitor, Lang::Ja) => (
                "fsmonitor を有効化",
                "fsmonitor 有効",
                "core.fsmonitor = true にします。作業ツリーは変更されません",
            ),
        };
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        let dialog = kagi::ui::dialog_a11y::recorded_dialog("plan-card")
            .expect("maintenance plan card drawn");
        assert_eq!(dialog.actions[0].1, confirm, "{lang:?}");
        let chip = kagi::ui::e2e::last_plan_status_chip().expect("AFTER state chip rendered");
        assert_eq!(chip, expected_chip, "{lang:?}");
        let detail = maintenance::after_state_detail(title);
        assert!(detail.contains(detail_phrase), "{lang:?} detail: {detail}");
        assert_ne!(chip, detail, "full prose must not occupy the {lang:?} chip");
        let (role, ax) = kagi::ui::dialog_a11y::recorded_note("plan-state-after")
            .expect("AFTER state is available to assistive technology");
        assert_eq!(role, Role::Group);
        assert!(ax.contains(detail), "{lang:?} AFTER accessibility: {ax}");

        let copy = kagi::ui::e2e::control_bounds(window.window_id(), "plan-card-copy")
            .expect("Copy all button is drawn");
        cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .expect("Copy all text");
        assert!(copied.contains(detail), "{lang:?} Copy all: {copied}");
    }
}

pub fn scenario_repo_health_proposal(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_language = i18n::lang();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    assert!(
        !commit_graph_exists(&repo),
        "the fixture starts without one"
    );
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_ecosystem_view(cx));
    let view = cx
        .read(|cx| app.read(cx).ui().ecosystem.clone())
        .expect("Analyze is open");
    view.update(cx, |view, cx| view.set_mode(EcosystemMode::Health, cx));
    wait_until(cx, "the health read", |cx| health(cx, &view).is_some());
    assert!(
        health(cx, &view)
            .unwrap()
            .contains(&HealthFinding::CommitGraphMissing),
        "the missing commit-graph is suggested"
    );
    let before = repo_fingerprint(&repo);

    // Asking opens the plan card, and nothing else happens.
    ask_to_fix(cx, &view);
    cx.read(|cx| {
        let app = app.read(cx);
        let modal = app
            .repo_health_modal()
            .expect("the fix's plan card is open");
        assert_eq!(modal.fix, HealthFix::WriteCommitGraph);
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
        assert_eq!(
            modal.plan.equivalent_command.as_deref(),
            Some("git commit-graph write --reachable")
        );
    });
    assert_plan_presentation(cx, &app, window, &MaintenanceTitle::WriteCommitGraph);
    assert!(
        !commit_graph_exists(&repo),
        "nothing is written before confirm"
    );
    assert!(
        receipts(&repo).is_empty(),
        "nothing is recorded before confirm"
    );
    assert_eq!(repo_fingerprint(&repo), before);

    // Cancelling changes nothing either.
    app.update(cx, |app, _| app.cancel_repo_health_modal());
    cx.run_until_parked();
    assert!(!commit_graph_exists(&repo) && receipts(&repo).is_empty());

    // The other health fix has its own confirmation and short AFTER state.
    app.update(cx, |app, _| {
        app.open_repo_health_modal(HealthFix::EnableFsmonitor)
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).repo_health_modal().unwrap().clone();
        assert_eq!(modal.fix, HealthFix::EnableFsmonitor);
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
    });
    assert_plan_presentation(cx, &app, window, &MaintenanceTitle::EnableFsmonitor);
    app.update(cx, |app, _| app.cancel_repo_health_modal());
    cx.run_until_parked();
    assert!(!commit_graph_exists(&repo) && receipts(&repo).is_empty());
    assert_eq!(repo_fingerprint(&repo), before);

    // A real root Enter on the card confirms: now it is written and recorded.
    ask_to_fix(cx, &view);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "enter");
    wait_until(cx, "the confirmed write", |cx| {
        cx.read(|cx| app.read(cx).write_busy_op.is_none()) && !receipts(&repo).is_empty()
    });
    assert!(commit_graph_exists(&repo));
    assert!(
        matches!(receipts(&repo).as_slice(), [OpOutcome::Success { .. }]),
        "one durable success"
    );
    assert_eq!(repo_fingerprint(&repo), before, "HEAD and status unchanged");
    wait_until(cx, "the refreshed health read", |cx| {
        health(cx, &view)
            .is_some_and(|findings| !findings.contains(&HealthFinding::CommitGraphMissing))
    });
    app.update(cx, |app, cx| app.set_lang(original_language, cx));

    drop(view);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS repo_health_proposal: suggested, confirm-gated, written once");
}
