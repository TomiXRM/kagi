//! Pull's freshness promise is exercised through the real native toolbar and menu.
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
        Self::new_named(repo, "origin")
    }

    fn new_named(repo: &Path, remote: &str) -> Self {
        let root = tempfile::tempdir().expect("origin root");
        let bare = root.path().join("origin.git");
        let other = root.path().join("other");
        git(
            repo,
            &["init", "--bare", "-q", "-b", "main", bare.to_str().unwrap()],
        );
        git(repo, &["remote", "add", remote, bare.to_str().unwrap()]);
        git(repo, &["push", "-q", "-u", remote, "main"]);
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

    fn advance_tree(&self) -> String {
        std::fs::write(self.other.join("README.md"), "updated tracked content\n").unwrap();
        std::fs::remove_file(self.other.join("pull-remove.txt")).unwrap();
        std::fs::create_dir(self.other.join("incoming")).unwrap();
        std::fs::write(self.other.join("incoming/added.txt"), "new tracked path\n").unwrap();
        git(&self.other, &["add", "-A"]);
        git(
            &self.other,
            &["commit", "-q", "-m", "modify add and remove paths"],
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

#[derive(Clone, Copy)]
enum AutoStashGuardDrift {
    Tracking,
    Remote,
    DirtyPaths,
    RestorePreview,
}

fn auto_stash_guard_drift(
    cx: &mut VisualTestAppContext,
    drift: AutoStashGuardDrift,
    language: Lang,
) {
    let _settings = SettingsGuard::install();
    i18n::set_lang(language);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    let upstream = origin.advance();
    git(&origin.other, &["push", "-q", "origin", "main:alternate"]);
    if matches!(drift, AutoStashGuardDrift::Remote) {
        let url = output(&repo, &["remote", "get-url", "origin"]);
        git(&repo, &["remote", "add", "mirror", &url]);
        git(&repo, &["fetch", "-q", "mirror"]);
    }

    // A pre-existing stash must survive as well as the split index/worktree.
    std::fs::write(repo.join("README.md"), "older stash content\n").unwrap();
    git(&repo, &["stash", "push", "-q", "-m", "keep existing stash"]);
    std::fs::write(repo.join("README.md"), "approved staged content\n").unwrap();
    git(&repo, &["add", "README.md"]);
    std::fs::write(repo.join("README.md"), "approved unstaged content\n").unwrap();
    std::fs::write(
        repo.join("identity-untracked.txt"),
        "approved untracked content\n",
    )
    .unwrap();

    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    let approved_identity = cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash);
        assert!(modal.plan.blockers.is_empty());
        assert!(modal.error.is_none());
        modal
            .plan
            .pull_identity
            .clone()
            .expect("approved Pull identity")
    });
    let backend = kagi_git::Backend::open(&repo).expect("open approved checkout");
    let approved_digest = backend.working_tree_status().unwrap().digest();
    let approved_plan = backend.plan_pull().unwrap();
    drop(backend);
    assert_eq!(output(&repo, &["rev-parse", "origin/main"]), upstream);
    match drift {
        AutoStashGuardDrift::Tracking | AutoStashGuardDrift::Remote => {
            let target = if matches!(drift, AutoStashGuardDrift::Remote) {
                "mirror/main"
            } else {
                "origin/alternate"
            };
            assert_eq!(output(&repo, &["rev-parse", target]), upstream);
            git(
                &repo,
                &["branch", &format!("--set-upstream-to={target}"), "main"],
            );
        }
        AutoStashGuardDrift::DirtyPaths => {
            std::fs::write(repo.join("new-dirty-path.txt"), "new unapproved content\n").unwrap();
        }
        AutoStashGuardDrift::RestorePreview => {
            // Move only the captured upstream's commit, not its identity or
            // the local dirty contents. Its new README edit changes restore.
            std::fs::write(
                origin.other.join("README.md"),
                "new upstream conflicting content\n",
            )
            .unwrap();
            git(&origin.other, &["add", "README.md"]);
            git(
                &origin.other,
                &["commit", "-qm", "change the stash restore preview"],
            );
            git(&origin.other, &["push", "-q", "origin", "main"]);
            git(&repo, &["fetch", "-q", "origin"]);
        }
    }
    let backend = kagi_git::Backend::open(&repo).expect("open changed promise");
    let fresh = backend.plan_pull().unwrap();
    if matches!(drift, AutoStashGuardDrift::DirtyPaths) {
        assert_ne!(
            backend.working_tree_status().unwrap().digest(),
            approved_digest
        );
        assert_eq!(fresh.pull_identity.as_ref(), Some(&approved_identity));
    } else if matches!(drift, AutoStashGuardDrift::RestorePreview) {
        use kagi_domain::plan_note::{PlanNote, PullNote};
        assert_eq!(
            backend.working_tree_status().unwrap().digest(),
            approved_digest
        );
        assert_eq!(fresh.pull_identity.as_ref(), Some(&approved_identity));
        assert_eq!(fresh.current, approved_plan.current);
        assert!(!approved_plan.warnings.iter().any(|note| matches!(
            note,
            PlanNote::Pull(
                PullNote::RestoreConflict { .. } | PullNote::RestoreConflictPossible { .. }
            )
        )));
        assert!(fresh.warnings.iter().any(|note| matches!(
            note, PlanNote::Pull(PullNote::RestoreConflict { paths }) if paths.iter().any(|path| path == "README.md")
        )));
    } else {
        assert_eq!(
            backend.working_tree_status().unwrap().digest(),
            approved_digest
        );
        assert_eq!(
            fresh.warnings, approved_plan.warnings,
            "restore predictions stay identical"
        );
        assert_eq!(
            fresh.current, approved_plan.current,
            "HEAD and dirty counts stay identical"
        );
        assert_ne!(fresh.pull_identity.as_ref(), Some(&approved_identity));
    }
    assert_eq!(fresh.head_at_plan, approved_plan.head_at_plan);
    drop(backend);

    let before = Checkout::read(&repo);
    let refs = output(&repo, &["show-ref"]);
    let stash = output(&repo, &["stash", "list", "--format=%H"]);
    assert!(!stash.is_empty(), "fixture retains a real older stash");
    assert_eq!(
        output(&repo, &["show", ":README.md"]),
        "approved staged content"
    );
    assert_eq!(records(&repo, "pull").len(), 0);
    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.run_until_parked();

    assert_eq!(
        Checkout::read(&repo),
        before,
        "identity refusal must preserve staged OID/mode and every working file"
    );
    assert_eq!(
        output(&repo, &["show-ref"]),
        refs,
        "no relevant ref may move"
    );
    assert_eq!(
        output(&repo, &["stash", "list", "--format=%H"]),
        stash,
        "stash OID stack is unchanged"
    );
    assert!(
        records(&repo, "stash-push").is_empty(),
        "refusal must precede any stash write"
    );
    assert!(
        records(&repo, "stash-pop").is_empty(),
        "refusal must not need restoration"
    );
    let pulls = records(&repo, "pull");
    assert_eq!(pulls.len(), 1, "one actual Pull refusal receipt");
    let OpOutcome::Refused { blockers } = &pulls[0].outcome else {
        panic!(
            "the real Stash & Pull consumer must refuse: {:?}",
            pulls[0].outcome
        );
    };
    assert_eq!(blockers.len(), 1);
    let reason = &blockers[0];
    let expected_reason = match drift {
        AutoStashGuardDrift::Tracking | AutoStashGuardDrift::Remote => {
            Msg::PullAutoStashIdentityChanged
        }
        AutoStashGuardDrift::DirtyPaths => Msg::PullAutoStashPlanStale,
        AutoStashGuardDrift::RestorePreview => Msg::PullAutoStashRestoreChanged,
    };
    assert_eq!(
        reason,
        expected_reason.t(),
        "the real changed promise must select its typed cause"
    );
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(matches!(&state.status_footer, FooterStatus::Failed(message) if message.contains(reason.as_str())));
        let toasts = state.toast_stack.as_ref().expect("actual toast stack").read(cx).toasts();
        assert_eq!(toasts.iter().filter(|toast| toast.kind == ToastKind::Error && toast.message.contains(reason.as_str())).count(), 1);
        let visible = state.op_log.as_ref().expect("visible Operation Log").read(cx);
        assert_eq!(visible.entries().iter().filter(|entry| entry.id == pulls[0].id).count(), 1);
        assert!(kagi::ui::oplog_panel::detail_lines(&pulls[0]).join("\n").contains(reason.as_str()));
    });
    unmount(cx, app, window);
}

/// Same commit and dirty promise, but a different full tracking ref.
pub fn scenario_pull_auto_stash_tracking_identity_drift(cx: &mut VisualTestAppContext) {
    for language in [Lang::En, Lang::Ja] {
        auto_stash_guard_drift(cx, AutoStashGuardDrift::Tracking, language);
    }
    eprintln!("[gui-e2e] PASS pull_auto_stash_tracking_identity_drift");
}

/// Same commit and dirty promise, but a different configured remote.
pub fn scenario_pull_auto_stash_remote_identity_drift(cx: &mut VisualTestAppContext) {
    for language in [Lang::En, Lang::Ja] {
        auto_stash_guard_drift(cx, AutoStashGuardDrift::Remote, language);
    }
    eprintln!("[gui-e2e] PASS pull_auto_stash_remote_identity_drift");
}

/// A new dirty path changes the stash promise, not the approved Pull identity.
pub fn scenario_pull_auto_stash_dirty_plan_drift(cx: &mut VisualTestAppContext) {
    for language in [Lang::En, Lang::Ja] {
        auto_stash_guard_drift(cx, AutoStashGuardDrift::DirtyPaths, language);
    }
    eprintln!("[gui-e2e] PASS pull_auto_stash_dirty_plan_drift");
}

/// Same approved identity and dirty bytes, but newer incoming edits change restore.
pub fn scenario_pull_auto_stash_restore_preview_drift(cx: &mut VisualTestAppContext) {
    for language in [Lang::En, Lang::Ja] {
        auto_stash_guard_drift(cx, AutoStashGuardDrift::RestorePreview, language);
    }
    eprintln!("[gui-e2e] PASS pull_auto_stash_restore_preview_drift");
}

/// A newer commit on the same full upstream must not become identity drift.
pub fn scenario_pull_auto_stash_same_upstream_advancement(cx: &mut VisualTestAppContext) {
    for language in [Lang::En, Lang::Ja] {
        let _settings = SettingsGuard::install();
        i18n::set_lang(language);
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let origin = Origin::new(&repo);
        let cached = origin.advance();
        std::fs::write(repo.join("README.md"), "older stash content\n").unwrap();
        git(&repo, &["stash", "push", "-q", "-m", "keep existing stash"]);
        std::fs::write(repo.join("README.md"), "approved staged content\n").unwrap();
        git(&repo, &["add", "README.md"]);
        std::fs::write(repo.join("README.md"), "approved unstaged content\n").unwrap();
        std::fs::write(
            repo.join("identity-untracked.txt"),
            "approved untracked content\n",
        )
        .unwrap();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| app.open_pull_modal(cx));
        cx.run_until_parked();
        let (approved_identity, approved_digest) = cx.read(|cx| {
            let modal = app
                .read(cx)
                .pull_modal()
                .expect("actual Stash & Pull confirmation");
            assert!(modal.auto_stash && modal.plan.blockers.is_empty());
            (
                modal.plan.pull_identity.clone().unwrap(),
                modal.dirty_digest.unwrap(),
            )
        });
        let backend = kagi_git::Backend::open(&repo).unwrap();
        let approved_backend_plan = backend.plan_pull().unwrap();
        drop(backend);
        assert_eq!(output(&repo, &["rev-parse", "origin/main"]), cached);
        std::fs::write(
            origin.other.join("later-upstream.txt"),
            "newer same upstream content\n",
        )
        .unwrap();
        git(&origin.other, &["add", "later-upstream.txt"]);
        git(
            &origin.other,
            &["commit", "-qm", "newer commit on the approved upstream"],
        );
        git(&origin.other, &["push", "-q", "origin", "main"]);
        git(&repo, &["fetch", "-q", "origin"]);
        let incoming = output(&origin.other, &["rev-parse", "HEAD"]);
        assert_ne!(incoming, cached);
        let backend = kagi_git::Backend::open(&repo).unwrap();
        let fresh = backend.plan_pull().unwrap();
        assert_eq!(fresh.pull_identity.as_ref(), Some(&approved_identity));
        assert_eq!(
            backend.working_tree_status().unwrap().digest(),
            approved_digest
        );
        assert_eq!(fresh.warnings, approved_backend_plan.warnings);
        drop(backend);
        let before = Checkout::read(&repo);
        let stash = output(&repo, &["stash", "list", "--format=%H"]);
        press_enter(cx, &app, window);
        wait_idle(cx, &app);
        cx.run_until_parked();
        let after = Checkout::read(&repo);
        assert_eq!(after.head, incoming);
        for path in ["README.md", "identity-untracked.txt"] {
            assert_eq!(
                after.files.get(Path::new(path)),
                before.files.get(Path::new(path))
            );
        }
        assert_eq!(
            output(&repo, &["show", ":later-upstream.txt"]),
            "newer same upstream content"
        );
        assert_eq!(output(&repo, &["stash", "list", "--format=%H"]), stash);
        let pulls = records(&repo, "pull");
        assert_eq!(pulls.len(), 1);
        assert!(
            matches!(pulls[0].outcome, OpOutcome::Success { .. }),
            "{:?}",
            pulls[0].outcome
        );
        assert_eq!(records(&repo, "stash-push").len(), 1);
        assert_eq!(records(&repo, "stash-pop").len(), 1);
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(!matches!(state.status_footer, FooterStatus::Failed(_)));
            assert!(!state
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .iter()
                .any(|toast| toast.kind == ToastKind::Error));
            assert_eq!(
                state
                    .op_log
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .entries()
                    .iter()
                    .filter(|entry| entry.id == pulls[0].id)
                    .count(),
                1
            );
        });
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS pull_auto_stash_same_upstream_advancement");
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

fn click_branch_pull(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    click_branch_sync(cx, window, "main", false);
}

fn click_branch_sync(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    branch: &str,
    ff_only: bool,
) {
    draw(cx, window);
    let row = e2e::control_bounds(window.window_id(), &format!("sidebar-local-{branch}"))
        .expect("selected branch row is drawn");
    cx.simulate_mouse_down(
        window,
        row.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_up(
        window,
        row.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    draw(cx, window);
    let id = if ff_only {
        "branch-menu-item-1-2"
    } else {
        "branch-menu-item-1-1"
    };
    let pull = e2e::control_bounds(window.window_id(), id)
        .expect("branch context-menu Pull action is drawn");
    cx.simulate_click(window, pull.center(), gpui::Modifiers::none());
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
        assert!(
            app.branch_plan_modal().is_none(),
            "no executable branch plan before freshness succeeds"
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
    clean_updates_from(cx, click_pull);
    clean_updates_from(cx, click_branch_pull);
    eprintln!("[gui-e2e] PASS pull_freshness_clean_updates");
}

fn clean_updates_from(
    cx: &mut VisualTestAppContext,
    click: fn(&mut VisualTestAppContext, AnyWindowHandle),
) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    let (app, window) = mount(cx, &repo);
    let expected = origin.advance();
    assert_cached_zero(cx, &app, &repo);
    let before = Checkout::read(&repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_fetch_for_e2e(hold);
    click(cx, window);
    assert_waiting(cx, &app, &repo, &before);
    release.send(());
    assert_fresh_confirm(cx, &app, window);
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
    #[derive(Clone, Copy)]
    enum Drift {
        Head,
        Worktree,
        Upstream,
    }
    for branch_ff in [false, true] {
        for drift in [Drift::Head, Drift::Worktree, Drift::Upstream] {
            let fixture = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let origin = Origin::new(&repo);
            if branch_ff {
                git(&repo, &["branch", "feature"]);
                git(
                    &repo,
                    &["branch", "--set-upstream-to=origin/main", "feature"],
                );
            }
            let (app, window) = mount(cx, &repo);
            let expected = origin.advance();
            if matches!(drift, Drift::Upstream) {
                git(&origin.other, &["push", "-q", "origin", "main:alternate"]);
            }
            let before = Checkout::read(&repo);
            if branch_ff {
                click_branch_sync(cx, window, "feature", true);
                assert_fresh_branch_confirm(cx, &app, window, "feature");
            } else {
                fetch_and_confirm(cx, &app, window, &repo, &before);
            }

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
                    if branch_ff {
                        app.branch_plan_modal().is_some()
                    } else {
                        app.pull_modal().is_some()
                    },
                    "original approval is still shown"
                );
            });
            if matches!(drift, Drift::Head) {
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
            } else if matches!(drift, Drift::Worktree) {
                std::fs::write(repo.join("README.md"), "edit after captured snapshot\n").unwrap();
            } else {
                let branch = if branch_ff { "feature" } else { "main" };
                git(
                    &repo,
                    &["branch", "--set-upstream-to=origin/alternate", branch],
                );
            }
            let changed = Checkout::read(&repo);
            if matches!(drift, Drift::Upstream) {
                assert_eq!(changed, before, "upstream drift changes only config");
            } else {
                assert_ne!(changed, before);
            }
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
                    if branch_ff {
                        app.branch_plan_modal().is_none()
                    } else {
                        app.pull_modal().is_none()
                    },
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
    }
    eprintln!("[gui-e2e] PASS pull_freshness_captured_reload_drift");
}

/// Every branch-menu variant must refresh its selected branch's live upstream.
/// The noncurrent legs deliberately leave main on origin while feature tracks
/// either origin/main or a distinct remote: fetching main's remote is insufficient.
pub fn scenario_pull_freshness_branch_entries(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    branch_ref_updates(cx, true, true, "origin");
    for ff_only in [false, true] {
        for remote in ["origin", "alternate"] {
            branch_ref_updates(cx, false, ff_only, remote);
        }
    }
    eprintln!("[gui-e2e] PASS pull_freshness_branch_entries");
}

/// Current strict FF must synchronize the actual checkout, not only its ref.
pub fn scenario_pull_ff_only_current_tree_consistency(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    branch_ref_updates(cx, true, true, "origin");
    eprintln!("[gui-e2e] PASS pull_ff_only_current_tree_consistency");
}

pub fn scenario_pull_branch_slash_remote_current(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    branch_ref_updates(cx, true, true, "team/origin");
    eprintln!("[gui-e2e] PASS pull_branch_slash_remote_current");
}

pub fn scenario_pull_branch_slash_remote_noncurrent(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for ff_only in [false, true] {
        branch_ref_updates(cx, false, ff_only, "team/origin");
    }
    eprintln!("[gui-e2e] PASS pull_branch_slash_remote_noncurrent");
}

pub fn scenario_pull_branch_local_upstream_current(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    branch_ref_updates(cx, true, true, ".");
    eprintln!("[gui-e2e] PASS pull_branch_local_upstream_current");
}

pub fn scenario_pull_branch_local_upstream_noncurrent(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for ff_only in [false, true] {
        branch_ref_updates(cx, false, ff_only, ".");
    }
    eprintln!("[gui-e2e] PASS pull_branch_local_upstream_noncurrent");
}

fn assert_fresh_branch_confirm(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    branch: &str,
) {
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(!app.app_sessions.has_leases());
        assert!(app.pull_modal().is_none());
        let modal = app
            .branch_plan_modal()
            .expect("fetched branch opens confirmation");
        assert_eq!(modal.branch_name, branch);
        assert!(modal.error.is_none());
        assert!(
            modal.plan.blockers.is_empty(),
            "fetched branch can fast-forward"
        );
        assert!(
            !matches!(
                modal.plan.disposition,
                kagi_git::ops::PlanDisposition::NoOp(_)
            ),
            "stale tracking equality must not survive the fetch"
        );
    });
    e2e::clear_control_bounds(window.window_id(), "plan-cancel");
    draw(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "plan-cancel").is_some());
    assert_eq!(latest_count(cx, app), 0);
}

fn branch_ref_updates(cx: &mut VisualTestAppContext, current: bool, ff_only: bool, remote: &str) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    if current {
        std::fs::write(repo.join("pull-remove.txt"), "remove this tracked path\n").unwrap();
        git(&repo, &["add", "pull-remove.txt"]);
        git(&repo, &["commit", "-q", "-m", "tracked removal baseline"]);
    }
    let origin = Origin::new(&repo);
    let different_remote = remote != "origin" && remote != ".";
    let alternate = different_remote.then(|| Origin::new_named(&repo, remote));
    if different_remote && !current {
        // new_named's push -u must not change the checked-out branch's upstream.
        git(&repo, &["branch", "--set-upstream-to=origin/main", "main"]);
    }
    let branch = if current { "main" } else { "feature" };
    let local_upstream = remote == ".";
    let tracking = if local_upstream {
        git(&repo, &["branch", "local-upstream"]);
        "local-upstream".to_string()
    } else {
        format!("{remote}/main")
    };
    if !current {
        git(&repo, &["branch", branch]);
    }
    if !current || local_upstream {
        git(
            &repo,
            &["branch", &format!("--set-upstream-to={tracking}"), branch],
        );
    }
    assert_eq!(
        output(&repo, &["config", &format!("branch.{branch}.remote")]),
        remote,
        "fixture uses the exact configured remote, including slash or dot"
    );
    let target_ref = format!("refs/heads/{branch}");
    let (app, window) = mount(cx, &repo);
    let upstream = alternate.as_ref().unwrap_or(&origin);
    let expected = if current {
        upstream.advance_tree()
    } else {
        upstream.advance()
    };
    let expected_checkout = Checkout::read(&upstream.other);
    if local_upstream {
        // Import the producer's actual commit into a local upstream branch.
        // The selected branch and its active checkout remain at the old commit.
        git(
            &repo,
            &[
                "fetch",
                "-q",
                upstream.other.to_str().unwrap(),
                "main:refs/heads/local-upstream",
            ],
        );
    }
    assert_cached_zero(cx, &app, &repo);
    let before = Checkout::read(&repo);
    let target_before = output(&repo, &["rev-parse", &target_ref]);
    let origin_before = output(&repo, &["rev-parse", "origin/main"]);
    if local_upstream {
        assert_eq!(output(&repo, &["rev-parse", &tracking]), expected);
    } else {
        assert_eq!(target_before, output(&repo, &["rev-parse", &tracking]));
    }
    let fetch_head_before = std::fs::read(repo.join(".git/FETCH_HEAD")).ok();
    let refs = || -> BTreeMap<String, String> {
        output(&repo, &["show-ref"])
            .lines()
            .map(|line| {
                let (oid, name) = line.split_once(' ').unwrap();
                (name.to_string(), oid.to_string())
            })
            .collect()
    };
    let mut expected_refs = refs();
    assert_ne!(target_before, expected);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_fetch_for_e2e(hold);
    click_branch_sync(cx, window, branch, ff_only);
    assert_waiting(cx, &app, &repo, &before);
    assert_eq!(output(&repo, &["rev-parse", &target_ref]), target_before);
    assert_eq!(
        output(&repo, &["rev-parse", &tracking]),
        if local_upstream {
            expected.as_str()
        } else {
            target_before.as_str()
        }
    );
    assert_eq!(refs(), expected_refs, "fetch hold preserves every ref");
    release.send(());
    cx.run_until_parked();
    assert_ne!(
        std::fs::read(repo.join(".git/FETCH_HEAD")).ok(),
        fetch_head_before,
        "the menu's real fetch must finish before confirmation"
    );
    assert!(
        records(&repo, "fetch")
            .iter()
            .all(|entry| matches!(entry.outcome, OpOutcome::Success { .. })),
        "a dot upstream fetch failure is a different prerequisite, not a title mismatch"
    );
    let fetched_refs = refs();
    let selected_remote_refs = format!("refs/remotes/{remote}/");
    assert!(
        fetched_refs
            .iter()
            .filter(|(name, _)| !name.starts_with(&selected_remote_refs))
            .eq(expected_refs
                .iter()
                .filter(|(name, _)| !name.starts_with(&selected_remote_refs))),
        "fetch preserves every local and other-remote ref: before={expected_refs:?}, after={fetched_refs:?}"
    );
    assert_eq!(output(&repo, &["rev-parse", &tracking]), expected);
    expected_refs = fetched_refs;
    let plan = kagi_git::Backend::open(&repo)
        .unwrap()
        .plan(&kagi_git::Operation::PullBranchFf {
            branch_name: branch.into(),
        })
        .unwrap();
    assert!(
        plan.blockers.is_empty(),
        "real upstream is supported by the backend"
    );
    let identity = plan
        .pull_identity
        .as_ref()
        .expect("executable typed Pull identity");
    assert_eq!(identity.branch, branch);
    assert_eq!(identity.remote, remote);
    assert_eq!(identity.local_oid.0.as_str(), target_before);
    assert_eq!(
        identity.upstream_ref,
        if local_upstream {
            "refs/heads/local-upstream".to_string()
        } else {
            format!("refs/remotes/{remote}/main")
        }
    );
    assert_fresh_branch_confirm(cx, &app, window, branch);
    cx.read(|cx| {
        let app = app.read(cx);
        let modal = app.branch_plan_modal().unwrap();
        let fresh_identity = modal.plan.pull_identity.as_ref().unwrap();
        assert_eq!(
            fresh_identity, identity,
            "confirmation preserves exact branch/upstream identity"
        );
        assert!(modal.fetch_owner.is_some());
        assert!(modal.fetched_refs.is_some());
    });
    assert_eq!(output(&repo, &["rev-parse", &tracking]), expected);
    if different_remote {
        assert_eq!(output(&repo, &["rev-parse", "origin/main"]), origin_before);
    }
    assert_eq!(Checkout::read(&repo), before);
    assert_eq!(output(&repo, &["rev-parse", &target_ref]), target_before);
    assert!(records(&repo, "pull").is_empty());
    let fetches = records(&repo, "fetch");
    assert_eq!(
        fetches.len(),
        usize::from(!local_upstream),
        "only a changed fetch emits a receipt"
    );
    assert!(fetches
        .iter()
        .all(|entry| matches!(entry.outcome, OpOutcome::Success { .. })));

    for _ in 0..2 {
        let (owner, revision) = cx.read(|cx| {
            let app = app.read(cx);
            let owner = app.active_session().unwrap();
            (owner, app.reads.revision(owner))
        });
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.advance_clock(Duration::from_secs(1));
        assert_fresh_branch_confirm(cx, &app, window, branch);
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.reads.revision(owner) > revision);
            assert!(
                !app.reads.is_loading(owner),
                "own watcher read was accepted"
            );
        });
        assert_eq!(Checkout::read(&repo), before);
        assert_eq!(output(&repo, &["rev-parse", &target_ref]), target_before);
    }

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert_eq!(output(&repo, &["rev-parse", &target_ref]), expected);
    assert_eq!(output(&repo, &["symbolic-ref", "--short", "HEAD"]), "main");
    let after = Checkout::read(&repo);
    assert_eq!(
        after.head.as_str(),
        if current {
            expected.as_str()
        } else {
            before.head.as_str()
        }
    );
    if current {
        assert_eq!(
            after.index, expected_checkout.index,
            "every staged path/OID/mode must equal the committed upstream tree"
        );
        assert_eq!(
            after.files, expected_checkout.files,
            "modified, added and removed paths must match the new HEAD tree"
        );
        assert!(
            output(&repo, &["diff", "--cached", "--name-status"]).is_empty(),
            "ref-only advancement must not manufacture staged reverse changes"
        );
        assert!(output(&repo, &["status", "--porcelain"]).is_empty());
    } else {
        assert_eq!(after.index, before.index, "unoccupied pull is ref-only");
        assert_eq!(
            after.files, before.files,
            "active checkout remains untouched"
        );
    }
    expected_refs.insert(target_ref.clone(), expected.clone());
    assert_eq!(
        refs(),
        expected_refs,
        "confirmed Pull moves only the selected local ref"
    );
    let durable = records(&repo, "pull");
    assert_eq!(durable.len(), 1);
    assert!(matches!(durable[0].outcome, OpOutcome::Success { .. }));
    assert_eq!(
        records(&repo, "fetch").len(),
        fetches.len(),
        "execution adds no extra fetch receipt"
    );
    assert!(records(&repo, "stash-push").is_empty());
    assert!(records(&repo, "stash-pop").is_empty());
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.branch_plan_modal().is_none());
        assert!(app.pull_modal().is_none());
        assert!(!app.app_sessions.has_leases());
        let panel = app.op_log.as_ref().expect("Operation Log").read(cx);
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.op == "pull" && entry.repo == durable[0].repo)
            .collect();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].id, durable[0].id);
        let toasts = app.toast_stack.as_ref().expect("toast stack").read(cx);
        assert_eq!(
            toasts
                .toasts()
                .iter()
                .filter(|toast| {
                    toast.kind == ToastKind::Success && toast.message.starts_with("pull:")
                })
                .count(),
            1,
            "one confirmed Pull delivers one success toast"
        );
    });
    unmount(cx, app, window);
}
