//! A conflict created externally during Pull's admitted fetch is a real blocker,
//! not evidence that the unchanged upstream moved to another remote.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpui::{AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::{
    button_style, dialog_a11y, e2e,
    i18n::{self, Lang, Msg},
    theme, FooterStatus, KagiApp, ToastKind,
};
use kagi_domain::plan_note::{CommonNote, OpPhrase, PlanNote};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpLogEntry, OpOutcome};

use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};
use crate::recovery_operations::{press_enter, press_key, wait_idle};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_output as output, git_succeeds};

struct SettingsGuard {
    _saved: crate::gui_isolation::SavedKeys,
    language: Lang,
    auto_fetch: bool,
}

impl SettingsGuard {
    fn install() -> Self {
        let guard = Self {
            _saved: crate::gui_isolation::SavedKeys::keep(&["lang", "auto_fetch"]),
            language: i18n::lang(),
            auto_fetch: theme::auto_fetch(),
        };
        theme::set_auto_fetch(false);
        guard
    }
}

impl Drop for SettingsGuard {
    fn drop(&mut self) {
        i18n::set_lang(self.language);
        theme::set_auto_fetch(self.auto_fetch);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ConflictCheckout {
    head: String,
    refs: String,
    index: Vec<u8>,
    stages: String,
    files: BTreeMap<PathBuf, Vec<u8>>,
    merge_state: Vec<Option<Vec<u8>>>,
    stash: String,
}

impl ConflictCheckout {
    fn read(repo: &Path) -> Self {
        fn files(root: &Path, dir: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path == root.join(".git") {
                    continue;
                }
                if path.is_dir() {
                    files(root, &path, result);
                } else {
                    result.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        std::fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut worktree = BTreeMap::new();
        files(repo, repo, &mut worktree);
        Self {
            head: output(repo, &["rev-parse", "HEAD"]),
            refs: output(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]),
            index: std::fs::read(repo.join(".git/index")).unwrap(),
            stages: output(repo, &["ls-files", "--stage", "-z"]),
            files: worktree,
            merge_state: ["MERGE_HEAD", "MERGE_MSG", "MERGE_MODE", "ORIG_HEAD"]
                .iter()
                .map(|name| std::fs::read(repo.join(".git").join(name)).ok())
                .collect(),
            stash: output(repo, &["stash", "list", "--format=%H %gs"]),
        }
    }
}

fn records(repo: &Path, op: &str) -> Vec<OpLogEntry> {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|entry| entry.op == op)
        .collect()
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn click_pull(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    draw(cx, window);
    assert_eq!(e2e::toolbar_unavailable("tb-pull"), Some(false));
    // The toolbar records AX availability rather than mouse bounds. Traverse
    // its real focus tree, as pull_freshness and toolbar_keyboard do.
    for _ in 0..120 {
        cx.update_window(window, |_, window, cx| {
            window.focus_next(cx);
            window.draw(cx).clear();
        })
        .unwrap();
        let _ = e2e::take_focus_probe();
        cx.simulate_keystrokes(window, "f19");
        if e2e::take_focus_probe() == Some("tb-pull") {
            crate::keyboard_nav::keys(cx, window, "enter");
            return;
        }
    }
    panic!("native Pull toolbar button is not keyboard reachable");
}

fn assert_no_false_drift(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let false_reason = Msg::PullUpstreamChangedDuringFetch.t();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            !matches!(&app.status_footer, FooterStatus::Failed(text) if text.contains(false_reason)),
            "a conflict must not be reported as an upstream change"
        );
        assert!(app.toast_stack.as_ref().unwrap().read(cx).toasts().iter().all(|toast| {
            !toast.message.contains(false_reason)
                && !(toast.kind == ToastKind::Sync
                    && toast.message.as_ref() == Msg::AlreadyUpToDatePull.t())
        }));
    });
}

fn blocked_after_fetch(cx: &mut VisualTestAppContext, lang: Lang, refuse: bool) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    // Both sides alter the same tracked line. Main is clean and synchronized
    // with origin before the product Pull starts; only the later external merge
    // introduces the conflict, without changing main or origin's identity.
    git(&repo, &["checkout", "-q", "-b", "external-conflict"]);
    std::fs::write(repo.join("README.md"), "# external side\nsecond line\n").unwrap();
    git(&repo, &["commit", "-qam", "external conflicting side"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("README.md"), "# main side\nsecond line\n").unwrap();
    git(&repo, &["commit", "-qam", "main conflicting side"]);
    let origin_root = tempfile::tempdir().unwrap();
    let origin = origin_root.path().join("origin.git");
    git(
        &repo,
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    // Match a canonical clone's remote HEAD before the baseline: fetching
    // must not introduce origin/HEAD as an incidental tracking-ref write.
    git(&repo, &["remote", "set-head", "origin", "-a"]);
    assert_eq!(
        output(&repo, &["symbolic-ref", "refs/remotes/origin/HEAD"]),
        "refs/remotes/origin/main"
    );
    let clean = ConflictCheckout::read(&repo);
    assert_eq!(output(&repo, &["status", "--porcelain"]), "");
    assert_eq!(output(&repo, &["rev-parse", "origin/main"]), clean.head);

    i18n::set_lang(lang);
    let (app, window) = mount(cx, &repo);
    cx.read(|cx| {
        let summary = &app.read(cx).view().status_summary;
        assert_eq!(summary.behind, Some(0));
        assert_eq!(summary.conflict_count, 0);
        assert!(!summary.is_dirty);
        assert!(!summary.no_upstream);
    });
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_fetch_for_e2e(hold);
    click_pull(cx, window);
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        let flight = app
            .fetch_in_flight
            .as_ref()
            .expect("ordinary Pull admitted fetch");
        assert_eq!(Some(flight.owner), app.active_session());
        assert!(app.app_sessions.has_leases());
        assert!(app.pull_modal().is_none());
    });
    assert_eq!(ConflictCheckout::read(&repo), clean);

    assert!(!git_succeeds(
        &repo,
        &["merge", "--no-commit", "external-conflict"]
    ));
    let conflicted = ConflictCheckout::read(&repo);
    assert_eq!(conflicted.head, clean.head);
    assert_eq!(conflicted.refs, clean.refs);
    assert_eq!(
        output(&repo, &["diff", "--name-only", "--diff-filter=U"]),
        "README.md"
    );
    for stage in [":1:README.md", ":2:README.md", ":3:README.md"] {
        assert!(
            !output(&repo, &["show", stage]).is_empty(),
            "real conflict stage {stage}"
        );
    }
    assert!(String::from_utf8_lossy(&conflicted.files[Path::new("README.md")]).contains("<<<<<<<"));
    release.send(());
    wait_idle(cx, &app);
    cx.run_until_parked();

    let blocker = PlanNote::Common(CommonNote::ConflictedFiles {
        count: 1,
        before: OpPhrase::Pulling,
    });
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(!app.app_sessions.has_leases());
        let modal = app.pull_modal().expect(
            "successful fetch must show the actual conflict blocker, not false upstream drift",
        );
        assert_eq!(modal.plan.blockers, vec![blocker.clone()]);
        assert!(
            modal.plan.pull_identity.is_none(),
            "early-blocked backend plan has no executable identity"
        );
        assert!(modal.error.is_none());
        assert!(!modal.auto_stash);
        assert!(
            !matches!(&app.status_footer, FooterStatus::Failed(_)),
            "successful no-op freshness must not report a planning failure"
        );
        assert!(app
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .iter()
            .all(|toast| toast.kind != ToastKind::Error));
    });
    assert_no_false_drift(cx, &app);
    dialog_a11y::clear_recorded_a11y();
    button_style::clear_recorded_modal_buttons();
    e2e::clear_control_bounds(window.window_id(), "plan-cancel");
    draw(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "plan-cancel").is_some());
    let reason = i18n::plan_note_text(&blocker);
    assert_eq!(
        dialog_a11y::recorded_note("plan-blocker-0"),
        Some((Role::Alert, reason.clone()))
    );
    let confirm =
        button_style::recorded_modal_button("plan-confirm").expect("blocked confirmation is drawn");
    assert!(confirm.disabled);
    assert_eq!(confirm.description.as_deref(), Some(reason.as_str()));
    let dialog = dialog_a11y::recorded_dialog("plan-card").unwrap();
    assert_eq!(dialog.actions.len(), 1, "blocked dialog offers only Cancel");
    assert_eq!(dialog.actions[0].1, Msg::PlanCancel.t());
    assert_eq!(ConflictCheckout::read(&repo), conflicted);
    assert!(records(&repo, "pull").is_empty());
    assert!(
        records(&repo, "fetch").is_empty(),
        "unchanged origin makes this a no-op fetch, not a write receipt"
    );
    let bounds =
        e2e::confirm_bounds(window.window_id()).expect("disabled Pull confirmation is laid out");
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|cx| assert!(app.read(cx).pull_modal().is_some()));
    assert!(records(&repo, "pull").is_empty());
    assert_eq!(ConflictCheckout::read(&repo), conflicted);

    // Root Enter retains the product's no-execute refusal receipt; Escape
    // declines the offered blocked plan without manufacturing an operation.
    if refuse {
        press_enter(cx, &app, window);
        wait_idle(cx, &app);
        let expected = i18n::op_refused("pull", reason, 0);
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(matches!(&app.status_footer, FooterStatus::Failed(text) if text.as_ref() == expected));
            let toast = app.toast_stack.as_ref().unwrap().read(cx).toasts().last().unwrap();
            assert_eq!(toast.kind, ToastKind::Error);
            assert!(toast.message.ends_with(&expected));
        });
        let pulls = records(&repo, "pull");
        assert_eq!(pulls.len(), 1);
        assert!(
            matches!(&pulls[0].outcome, OpOutcome::Refused { blockers } if *blockers == vec![blocker.message_en()])
        );
        cx.read(|cx| {
            let panel = app
                .read(cx)
                .op_log
                .as_ref()
                .expect("visible refusal receipt")
                .read(cx);
            // The app seeds this panel from the global oplog tail, including
            // earlier fixture repositories. Count all Pull rows for this repo,
            // not just the matching ID, so duplicate presentation still fails.
            let shown: Vec<_> = panel
                .entries()
                .iter()
                .filter(|entry| entry.op == "pull" && entry.repo == pulls[0].repo)
                .collect();
            assert_eq!(shown.len(), 1);
            assert_eq!(shown[0].id, pulls[0].id);
            assert!(
                matches!(&shown[0].outcome, OpOutcome::Refused { blockers } if *blockers == vec![blocker.message_en()])
            );
        });
    } else {
        press_key(cx, &app, window, "escape");
        cx.run_until_parked();
        assert!(records(&repo, "pull").is_empty());
    }
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.pull_modal().is_none());
        assert!(app.fetch_in_flight.is_none());
        assert!(!app.app_sessions.has_leases());
    });
    assert_no_false_drift(cx, &app);
    assert_eq!(
        ConflictCheckout::read(&repo),
        conflicted,
        "no pull/stash, index resolution, conflict-file edit or merge abort is approved"
    );
    for op in ["stash-push", "stash-pop", "stash-apply"] {
        assert!(
            records(&repo, op).is_empty(),
            "blocked Pull cannot start {op}"
        );
    }
    unmount(cx, app, window);
}

pub fn scenario_pull_fetch_preserves_conflict_blocker(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for lang in [Lang::En, Lang::Ja] {
        for refuse in [false, true] {
            blocked_after_fetch(cx, lang, refuse);
        }
    }
    eprintln!("[gui-e2e] PASS pull_fetch_preserves_conflict_blocker EN/JA cancel/refuse");
}
