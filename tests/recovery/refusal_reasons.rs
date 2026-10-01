//! #353: a refused plan says why in the footer and the bounded toast.
//!
//! Each family opens a plan the repository really blocks, then presses Enter
//! on the root — the path that closes the modal which showed the reason. The
//! footer and the last toast must then carry the first blocker in the active
//! language (plus how many more), and the durable Operation Log entry every
//! blocker, while the repository stays as it was.
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::i18n::{self, Lang};
use kagi::ui::types::FooterStatus;
use kagi::ui::KagiApp;
use kagi_domain::plan_note::PlanNote;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::path::Path;
use std::time::{Duration, Instant};

fn wait_idle(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let idle = cx.read(|cx| {
            let app = app.read(cx);
            app.write_busy_op.is_none() && app.planning.is_none()
        });
        if idle {
            return;
        }
        assert!(Instant::now() < deadline, "the operation did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Wait until `blockers` reports the open modal's plan, and return it.
fn blocked_plan(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    family: &str,
    blockers: impl Fn(&KagiApp) -> Option<Vec<PlanNote>>,
) -> Vec<PlanNote> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if let Some(notes) = cx.read(|cx| blockers(app.read(cx))) {
            assert!(
                !notes.is_empty(),
                "{family}: the fixture must block the plan"
            );
            return notes;
        }
        assert!(Instant::now() < deadline, "{family}: no plan arrived");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn press_root_enter(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "enter");
    wait_idle(cx, app);
}

/// The footer and last toast name the first blocker; the oplog keeps all.
fn assert_refused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &Path,
    op: &str,
    notes: &[PlanNote],
) {
    let expected = i18n::op_refused(op, i18n::plan_note_text(&notes[0]), notes.len() - 1);
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            matches!(&state.status_footer, FooterStatus::Failed(m) if m.as_ref() == expected),
            "{op}: footer must name the reason {expected:?}, got {:?}",
            state.status_footer
        );
        let toast = state
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .unwrap();
        assert!(
            matches!(toast.kind, kagi::ui::ToastKind::Error) && toast.message.ends_with(&expected),
            "{op}: toast must name the reason {expected:?}, got {:?}",
            toast.message
        );
    });
    let recorded: Vec<_> = read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|entry| entry.op == op)
        .collect();
    let [entry] = recorded.as_slice() else {
        panic!("{op}: expected one durable refusal, got {recorded:?}");
    };
    let all: Vec<String> = notes.iter().map(PlanNote::message_en).collect();
    assert!(
        matches!(&entry.outcome, OpOutcome::Refused { blockers } if *blockers == all),
        "{op}: the oplog must keep every blocker {all:?}, got {:?}",
        entry.outcome
    );
}

/// Delete-branch on the checked-out branch.
fn delete_current_branch(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_delete_branch_modal("main", cx));
    let notes = blocked_plan(cx, &app, "delete-branch", |app| {
        app.delete_branch_modal().map(|m| m.plan.blockers.clone())
    });
    press_root_enter(cx, &app, window);
    assert!(cx.read(|cx| app.read(cx).delete_branch_modal().is_none()));
    assert_refused(cx, &app, &repo, "delete-branch", &notes);
    assert_eq!(repo_fingerprint(&repo), before);
    unmount(cx, app, window);
}

/// Push from a repository with no remote.
fn push_without_remote(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_push_modal(cx));
    let notes = blocked_plan(cx, &app, "push", |app| {
        app.push_modal().map(|m| m.plan.blockers.clone())
    });
    press_root_enter(cx, &app, window);
    assert!(cx.read(|cx| app.read(cx).push_modal().is_none()));
    assert_refused(cx, &app, &repo, "push", &notes);
    assert_eq!(repo_fingerprint(&repo), before);
    unmount(cx, app, window);
}

/// Pull on a branch with no upstream: the core writes the receipt.
fn pull_without_upstream(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    let notes = blocked_plan(cx, &app, "pull", |app| {
        app.pull_modal().map(|m| m.plan.blockers.clone())
    });
    press_root_enter(cx, &app, window);
    assert!(cx.read(|cx| app.read(cx).pull_modal().is_none()));
    assert_refused(cx, &app, &repo, "pull", &notes);
    assert_eq!(repo_fingerprint(&repo), before);
    unmount(cx, app, window);
}

/// Stash push on a clean tree: refused by the stash app flow's backend run.
fn stash_push_clean_tree(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_stash_push_modal(cx));
    let notes = blocked_plan(cx, &app, "stash-push", |app| {
        app.stash_push_modal()
            .and_then(|m| m.plan.as_ref())
            .map(|plan| plan.blockers.clone())
    });
    press_root_enter(cx, &app, window);
    assert!(cx.read(|cx| app.read(cx).stash_push_modal().is_none()));
    assert_refused(cx, &app, &repo, "stash-push", &notes);
    assert_eq!(repo_fingerprint(&repo), before);
    unmount(cx, app, window);
}

pub fn scenario_refusal_reasons(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let language = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        delete_current_branch(cx);
        push_without_remote(cx);
        pull_without_upstream(cx);
        stash_push_clean_tree(cx);
    }
    i18n::set_lang(language);
    eprintln!("[gui-e2e] PASS refusal_reasons EN/JA delete-branch push pull stash-push");
}
