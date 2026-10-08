//! Pull's freshness promise is exercised through the real native toolbar.
//! The second clone advances origin while the mounted checkout still says ↓0.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{self, Lang, Msg},
    theme, FooterStatus, KagiApp, ToastKind,
};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpLogEntry, OpOutcome};

use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};
use crate::recovery_operations::{press_enter, wait_idle};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_output as output};

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
        i18n::set_lang(Lang::Ja);
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

struct Origin {
    _root: tempfile::TempDir,
    other: PathBuf,
}

impl Origin {
    fn new(repo: &Path) -> Self {
        let root = tempfile::tempdir().expect("origin root");
        let bare = root.path().join("origin.git");
        let other = root.path().join("other");
        git(
            repo,
            &["init", "--bare", "-q", "-b", "main", bare.to_str().unwrap()],
        );
        git(repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git(repo, &["push", "-q", "-u", "origin", "main"]);
        git(
            root.path(),
            &[
                "clone",
                "-q",
                bare.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        Self { _root: root, other }
    }

    fn advance(&self) -> String {
        std::fs::write(self.other.join("upstream.txt"), "fresh upstream content\n").unwrap();
        git(&self.other, &["add", "upstream.txt"]);
        git(
            &self.other,
            &[
                "commit",
                "-q",
                "-m",
                "advance without fetching the consumer",
            ],
        );
        git(&self.other, &["push", "-q", "origin", "main"]);
        output(&self.other, &["rev-parse", "HEAD"])
    }

    fn make_unreachable(&self, repo: &Path) {
        // Keep origin/main intact. A cached-zero planner would still claim latest.
        let missing = self._root.path().join("missing.git");
        git(
            repo,
            &["remote", "set-url", "origin", missing.to_str().unwrap()],
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Checkout {
    head: String,
    index: String,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl Checkout {
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
            index: output(repo, &["ls-files", "--stage", "-z"]),
            files: worktree,
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
    assert_eq!(
        e2e::toolbar_unavailable("tb-pull"),
        Some(false),
        "cached ↓0 Pull remains actionable"
    );
    // Toolbar IDs record AX availability, not control_bounds. Use the same
    // real focus traversal and harmless F19 probe as toolbar_keyboard.
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

fn assert_cached_zero(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, repo: &Path) {
    cx.read(|cx| {
        let summary = &app.read(cx).view().status_summary;
        assert!(!summary.no_upstream);
        assert!(!app.read(cx).view().is_dirty);
        assert_eq!(
            summary.behind,
            Some(0),
            "the consumer really has stale ↓0 knowledge"
        );
    });
    assert_eq!(
        output(repo, &["rev-parse", "HEAD"]),
        output(repo, &["rev-parse", "origin/main"])
    );
}

fn latest_count(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> usize {
    cx.read(|cx| {
        app.read(cx)
            .toast_stack
            .as_ref()
            .map(|stack| {
                stack
                    .read(cx)
                    .toasts()
                    .iter()
                    .filter(|toast| {
                        toast.kind == ToastKind::Sync
                            && toast.message.as_ref() == Msg::AlreadyUpToDatePull.t()
                    })
                    .count()
            })
            .unwrap_or(0)
    })
}

fn assert_waiting(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &Path,
    before: &Checkout,
) {
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        let flight = app
            .fetch_in_flight
            .as_ref()
            .expect("Pull waits on an admitted real fetch");
        assert_eq!(Some(flight.owner), app.active_session());
        assert!(app.app_sessions.has_leases());
        assert!(
            app.pull_modal().is_none(),
            "no executable plan before freshness succeeds"
        );
    });
    assert_eq!(
        latest_count(cx, app),
        0,
        "no premature latest toast while fetch is held"
    );
    assert_eq!(
        &Checkout::read(repo),
        before,
        "fetch planning cannot change HEAD/index/worktree"
    );
    assert!(
        records(repo, "pull").is_empty(),
        "no Pull execution before confirmation"
    );
}

fn assert_fresh_confirm(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(!app.app_sessions.has_leases());
        let modal = app
            .pull_modal()
            .expect("fresh upstream requires a real Pull confirmation");
        assert!(!modal.auto_stash, "clean checkout stays a plain Pull");
        assert!(modal.error.is_none());
        assert!(
            modal.plan.blockers.is_empty(),
            "fresh fast-forward is executable"
        );
        assert!(!matches!(
            modal.plan.disposition,
            kagi_git::ops::PlanDisposition::NoOp(_)
        ));
    });
    e2e::clear_control_bounds(window.window_id(), "plan-cancel");
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "plan-cancel").is_some(),
        "confirmation is actually drawn"
    );
    assert_eq!(
        latest_count(cx, app),
        0,
        "stale zero must never become a latest claim"
    );
}

fn fetch_and_confirm(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    before: &Checkout,
) {
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_fetch_for_e2e(hold);
    click_pull(cx, window);
    assert_waiting(cx, app, repo, before);
    release.send(());
    assert_fresh_confirm(cx, app, window);
}

/// The reported bug: origin moved, but neither local refs nor the mounted UI know.
/// Only the production Pull click fetches. Its own reload must not erase approval.
pub fn scenario_pull_freshness_clean_updates(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    let (app, window) = mount(cx, &repo);
    let expected = origin.advance();
    assert_cached_zero(cx, &app, &repo);
    let before = Checkout::read(&repo);
    fetch_and_confirm(cx, &app, window, &repo, &before);
    assert_eq!(
        output(&repo, &["rev-parse", "origin/main"]),
        expected,
        "the real fetch observed origin's new tip"
    );
    assert_eq!(
        Checkout::read(&repo),
        before,
        "confirmation preserves HEAD, staged paths/OIDs/modes and working files"
    );
    assert!(records(&repo, "pull").is_empty());

    // Model the watcher reads caused by this same fetch, not a synthetic modal.
    for _ in 0..2 {
        let (owner, revision) = cx.read(|cx| {
            let app = app.read(cx);
            let owner = app.active_session().unwrap();
            (owner, app.reads.revision(owner))
        });
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.advance_clock(Duration::from_secs(1));
        assert_fresh_confirm(cx, &app, window);
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(
                app.reads.revision(owner) > revision,
                "watcher reload began a new read"
            );
            assert!(
                !app.reads.is_loading(owner),
                "watcher reload was accepted before asserting preservation"
            );
            assert_eq!(
                app.view().status_summary.behind,
                Some(1),
                "accepted reload has fresh counts"
            );
        });
        assert_eq!(
            Checkout::read(&repo),
            before,
            "reload cannot consume the user's approval"
        );
    }

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert_eq!(output(&repo, &["rev-parse", "HEAD"]), expected);
    assert_eq!(
        output(&repo, &["show", ":upstream.txt"]),
        "fresh upstream content"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("upstream.txt")).unwrap(),
        "fresh upstream content\n"
    );
    assert_eq!(
        std::fs::read(repo.join("README.md")).unwrap(),
        before.files[Path::new("README.md")]
    );
    assert!(output(&repo, &["status", "--porcelain"]).is_empty());
    let durable = records(&repo, "pull");
    assert_eq!(durable.len(), 1, "one confirmed Pull, one durable receipt");
    assert!(matches!(durable[0].outcome, OpOutcome::Success { .. }));
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.pull_modal().is_none());
        assert!(!app.app_sessions.has_leases());
        let panel = app.op_log.as_ref().expect("Operation Log").read(cx);
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.op == "pull" && entry.repo == durable[0].repo)
            .collect();
        assert_eq!(shown.len(), 1);
        assert_eq!(
            shown[0].id, durable[0].id,
            "the visible receipt is the persisted backend receipt"
        );
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_freshness_clean_updates");
}

/// Even a genuinely synced checkout owes the user a successful freshness check.
pub fn scenario_pull_freshness_synced_waits(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let _origin = Origin::new(&repo);
    let (app, window) = mount(cx, &repo);
    assert_cached_zero(cx, &app, &repo);
    let before = Checkout::read(&repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_fetch_for_e2e(hold);
    click_pull(cx, window);
    assert_waiting(cx, &app, &repo, &before);
    release.send(());
    cx.run_until_parked();
    assert_eq!(
        latest_count(cx, &app),
        1,
        "latest is announced only after successful fetch"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(
            app.pull_modal().is_none(),
            "synced checkout needs no approval"
        );
        assert!(!app.app_sessions.has_leases());
    });
    assert_eq!(Checkout::read(&repo), before);
    assert!(
        records(&repo, "pull").is_empty(),
        "latest never manufactures a Pull receipt"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_freshness_synced_waits");
}

/// A real transport failure cannot become cached-zero certainty or disappear
/// when the user's Pull joins a previously silent background fetch.
pub fn scenario_pull_freshness_fetch_failure(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for (join_silent, pull_requested) in [(false, true), (true, true), (true, false)] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let origin = Origin::new(&repo);
        let (app, window) = mount(cx, &repo);
        let presentation = |app: &KagiApp| {
            let message = match &app.status_footer {
                FooterStatus::Success(message)
                | FooterStatus::Failed(message)
                | FooterStatus::Idle(message)
                | FooterStatus::Busy(message) => message.clone(),
            };
            (
                std::mem::discriminant(&app.status_footer),
                message,
                app.bottom_panel_open,
                std::mem::discriminant(&app.bottom_tab),
            )
        };
        let presentation_before = cx.read(|cx| presentation(app.read(cx)));
        origin.advance();
        origin.make_unreachable(&repo);
        assert_cached_zero(cx, &app, &repo);
        let before = Checkout::read(&repo);
        let (hold, release) = deferred::<()>(cx);
        KagiApp::hold_next_fetch_for_e2e(hold);
        if join_silent {
            app.update(cx, |app, cx| assert!(app.fetch_async_for(true, None, cx)));
            cx.run_until_parked();
            if pull_requested {
                // The busy toolbar is disabled; an already admitted menu intent joins.
                cx.update_window(window, |_, window, cx| {
                    app.update(cx, |app, cx| {
                        app.handle_menu_command("repo.pull", window, cx)
                    });
                })
                .unwrap();
            }
        } else {
            click_pull(cx, window);
        }
        if pull_requested {
            assert_waiting(cx, &app, &repo, &before);
        }
        release.send(());
        cx.run_until_parked();
        assert_eq!(
            latest_count(cx, &app),
            0,
            "failed fetch must never report latest"
        );
        let failures = records(&repo, "fetch");
        assert_eq!(failures.len(), 1, "one flight owns one durable receipt");
        assert!(matches!(failures[0].outcome, OpOutcome::Failed { .. }));
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.fetch_in_flight.is_none());
            assert!(!app.app_sessions.has_leases());
            assert!(
                !e2e::active_modal_present(app),
                "failed freshness cannot offer an executable modal"
            );
            if pull_requested {
                assert!(
                    matches!(&app.status_footer, FooterStatus::Failed(_)),
                    "Pull must see its fetch failure (joined silent={join_silent})"
                );
            } else {
                assert_eq!(presentation(app), presentation_before,
                    "a silent fetch without Pull must not replace the footer or open a panel");
            }
            let stack = app.toast_stack.as_ref().unwrap().read(cx);
            let errors = stack
                .toasts()
                .iter()
                .filter(|toast| toast.kind == ToastKind::Error)
                .count();
            assert_eq!(
                errors, usize::from(pull_requested),
                "one failed Pull-owned fetch must emit one notification; background failure stays quiet"
            );
            let panel = app.op_log.as_ref().unwrap().read(cx);
            assert!(panel
                .entries()
                .iter()
                .any(|entry| entry.id == failures[0].id));
        });
        assert_eq!(Checkout::read(&repo), before);
        assert!(records(&repo, "pull").is_empty());
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS pull_freshness_fetch_failure");
}

/// A manual fetch already holding this visit's writer lease satisfies Pull once;
/// repeated requests join its waiter, never execute or authorize a duplicate fetch.
pub fn scenario_pull_freshness_joins_held_fetch(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    let (app, window) = mount(cx, &repo);
    let expected = origin.advance();
    assert_cached_zero(cx, &app, &repo);
    let before = Checkout::read(&repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_fetch_for_e2e(hold);
    app.update(cx, |app, cx| assert!(app.fetch_async_for(false, None, cx)));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("repo.pull", window, cx);
            app.handle_menu_command("repo.pull", window, cx);
        });
    })
    .unwrap();
    assert_waiting(cx, &app, &repo, &before);
    release.send(());
    assert_fresh_confirm(cx, &app, window);
    assert_eq!(output(&repo, &["rev-parse", "origin/main"]), expected);
    assert_eq!(Checkout::read(&repo), before);
    assert!(records(&repo, "pull").is_empty());
    // Cancel through the real modal key path: joining is freshness, not approval.
    crate::recovery_operations::press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    cx.read(|cx| assert!(app.read(cx).pull_modal().is_none()));
    assert_eq!(Checkout::read(&repo), before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_freshness_joins_held_fetch");
}

fn assert_reload_invalidates(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    changed: &Checkout,
) {
    let (owner, revision) = cx.read(|cx| {
        let app = app.read(cx);
        let owner = app.active_session().unwrap();
        (owner, app.reads.revision(owner))
    });
    app.update(cx, |app, cx| app.reload_external(cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.reads.revision(owner) > revision);
        assert!(
            !app.reads.is_loading(owner),
            "the external snapshot must be accepted, not merely requested"
        );
        assert!(
            !e2e::active_modal_present(app),
            "changed repository state invalidates the clean fetch confirmation"
        );
        assert!(app.fetch_in_flight.is_none());
        assert!(!app.app_sessions.has_leases());
    });
    e2e::clear_control_bounds(window.window_id(), "plan-cancel");
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "plan-cancel").is_none(),
        "the invalidated confirmation is no longer drawn"
    );
    assert_eq!(&Checkout::read(repo), changed);

    // The old approval's Enter path must no longer be able to write.
    press_enter(cx, app, window);
    wait_idle(cx, app);
    cx.run_until_parked();
    assert_eq!(
        &Checkout::read(repo),
        changed,
        "neither accepted reload nor Enter may consume the stale approval"
    );
    assert!(records(repo, "pull").is_empty());
    assert_eq!(latest_count(cx, app), 0);
}

fn external_change_invalidates(cx: &mut VisualTestAppContext, head_change: bool) {
    let _settings = SettingsGuard::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    let (app, window) = mount(cx, &repo);
    let expected = origin.advance();
    assert_cached_zero(cx, &app, &repo);
    let before = Checkout::read(&repo);
    fetch_and_confirm(cx, &app, window, &repo, &before);
    assert_eq!(Checkout::read(&repo), before);
    assert_eq!(output(&repo, &["rev-parse", "origin/main"]), expected);

    if head_change {
        git(
            &repo,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "external HEAD change",
            ],
        );
    } else {
        std::fs::write(repo.join("README.md"), "external uncommitted edit\n").unwrap();
    }
    let changed = Checkout::read(&repo);
    assert_eq!(changed.index, before.index);
    if head_change {
        assert_ne!(changed.head, before.head);
        assert_eq!(changed.files, before.files);
    } else {
        assert_eq!(changed.head, before.head);
        assert_ne!(changed.files, before.files);
    }
    assert_reload_invalidates(cx, &app, window, &repo, &changed);
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.view().is_dirty, !head_change);
        assert_eq!(
            app.view().status_summary.ahead,
            Some(usize::from(head_change))
        );
        assert_eq!(app.view().status_summary.behind, Some(1));
    });
    assert_eq!(output(&repo, &["rev-parse", "origin/main"]), expected);
    assert!(output(&repo, &["stash", "list"]).is_empty());
    unmount(cx, app, window);
}

/// An external empty commit changes only HEAD, not the worktree fingerprint.
pub fn scenario_pull_freshness_external_head_invalidates(cx: &mut VisualTestAppContext) {
    external_change_invalidates(cx, true);
    eprintln!("[gui-e2e] PASS pull_freshness_external_head_invalidates");
}

/// A real unstaged edit must not turn the old plain approval into auto-stash.
pub fn scenario_pull_freshness_external_worktree_invalidates(cx: &mut VisualTestAppContext) {
    external_change_invalidates(cx, false);
    eprintln!("[gui-e2e] PASS pull_freshness_external_worktree_invalidates");
}

/// Closing a newer user modal does not restore the older pending Pull intent.
/// Exercise both possible successful outcomes: confirmation and already-latest.
pub fn scenario_pull_freshness_closed_modal_displaces(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for advanced in [true, false] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let origin = Origin::new(&repo);
        let (app, window) = mount(cx, &repo);
        let expected = if advanced {
            origin.advance()
        } else {
            output(&repo, &["rev-parse", "origin/main"])
        };
        assert_cached_zero(cx, &app, &repo);
        let before = Checkout::read(&repo);
        let (hold, release) = deferred::<()>(cx);
        KagiApp::hold_next_fetch_for_e2e(hold);
        click_pull(cx, window);
        assert_waiting(cx, &app, &repo, &before);

        app.update(cx, |app, cx| {
            app.open_create_branch_modal(kagi_git::CommitId(before.head.clone()), cx);
        });
        e2e::clear_control_bounds(window.window_id(), "create-branch-cancel");
        draw(cx, window);
        assert!(
            e2e::control_bounds(window.window_id(), "create-branch-cancel").is_some(),
            "the newer real user modal was presented"
        );
        crate::recovery_operations::press_key(cx, &app, window, "escape");
        cx.run_until_parked();
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(
                !e2e::active_modal_present(app),
                "the user closed the newer modal before fetch completed"
            );
            assert!(
                app.fetch_in_flight.is_some(),
                "the real fetch is still held"
            );
            assert!(app.app_sessions.has_leases());
        });
        assert_eq!(Checkout::read(&repo), before);
        release.send(());
        cx.run_until_parked();
        draw(cx, window);
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.fetch_in_flight.is_none());
            assert!(!app.app_sessions.has_leases());
            assert!(app.create_branch_modal().is_none());
            assert!(
                app.pull_modal().is_none(),
                "vacancy cannot resurrect the displaced Pull confirmation"
            );
        });
        assert_eq!(
            latest_count(cx, &app),
            0,
            "displaced Pull cannot say latest"
        );
        assert_eq!(output(&repo, &["rev-parse", "origin/main"]), expected);
        assert_eq!(Checkout::read(&repo), before);
        assert!(records(&repo, "pull").is_empty());
        assert!(records(&repo, "create-branch").is_empty());
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS pull_freshness_closed_modal_displaces");
}

/// A captured unchanged snapshot cannot authorize a replan of a changed checkout.
pub fn scenario_pull_freshness_captured_reload_drift(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for head_change in [true, false] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let origin = Origin::new(&repo);
        let (app, window) = mount(cx, &repo);
        let expected = origin.advance();
        let before = Checkout::read(&repo);
        fetch_and_confirm(cx, &app, window, &repo, &before);

        let (owner, revision) = app.update(cx, |app, cx| {
            let owner = app.active_session().unwrap();
            app.reload_external(cx);
            (owner, app.reads.revision(owner))
        });
        // Existing GPUI scheduler control: finish the real synchronous snapshot
        // worker, but do not let its foreground acceptance closure run yet.
        let dispatcher = cx.background_executor.dispatcher().as_test().unwrap();
        let mut ticks = 0;
        while dispatcher.tick(true) {
            ticks += 1;
        }
        assert!(ticks > 0, "the real snapshot worker ran");
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(
                app.reads.is_loading(owner),
                "captured read is not yet accepted"
            );
            assert!(
                app.pull_modal().is_some(),
                "original approval is still shown"
            );
        });
        if head_change {
            git(
                &repo,
                &[
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "drift after snapshot",
                ],
            );
        } else {
            std::fs::write(repo.join("README.md"), "edit after captured snapshot\n").unwrap();
        }
        let changed = Checkout::read(&repo);
        assert_ne!(changed, before);
        assert_eq!(changed.index, before.index);
        cx.run_until_parked();
        cx.read(|cx| {
            let app = app.read(cx);
            assert_eq!(app.reads.revision(owner), revision, "the captured read won");
            assert!(
                !app.reads.is_loading(owner),
                "the captured read was accepted"
            );
            assert!(
                !app.view().is_dirty,
                "accepted snapshot predates the live edit"
            );
            assert_eq!(app.view().status_summary.ahead, Some(0));
            assert_eq!(app.view().status_summary.behind, Some(1));
            assert!(
                app.pull_modal().is_none(),
                "live replan must invalidate, not rewrite the old clean approval"
            );
        });
        e2e::clear_control_bounds(window.window_id(), "plan-cancel");
        draw(cx, window);
        assert!(e2e::control_bounds(window.window_id(), "plan-cancel").is_none());
        press_enter(cx, &app, window);
        wait_idle(cx, &app);
        cx.run_until_parked();
        assert_eq!(Checkout::read(&repo), changed);
        assert_eq!(output(&repo, &["rev-parse", "origin/main"]), expected);
        assert_eq!(latest_count(cx, &app), 0);
        assert!(records(&repo, "pull").is_empty());
        assert!(output(&repo, &["stash", "list"]).is_empty());
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS pull_freshness_captured_reload_drift");
}
