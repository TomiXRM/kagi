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
use kagi_git::oplog::OpOutcome;

use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};
use crate::recovery_operations::{press_enter, wait_idle};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_output as output};

pub(super) struct SettingsGuard {
    _saved: crate::gui_isolation::SavedKeys,
    language: Lang,
    auto_fetch: bool,
}

impl SettingsGuard {
    pub(super) fn install() -> Self {
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

pub(super) struct Origin {
    _root: tempfile::TempDir,
    other: PathBuf,
}

impl Origin {
    pub(super) fn new(repo: &Path) -> Self {
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

    pub(super) fn advance(&self) -> String {
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

    fn advance_protection(&self) -> String {
        std::fs::write(
            self.other.join("protection.txt"),
            "incoming\n1\n2\n3\n4\n5\n6\n7\n8\nbottom\n",
        )
        .unwrap();
        git(&self.other, &["add", "protection.txt"]);
        git(
            &self.other,
            &["commit", "-qm", "incoming protection overlap"],
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
pub(super) struct Checkout {
    head: String,
    index: String,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl Checkout {
    pub(super) fn read(repo: &Path) -> Self {
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
    crate::recovery_pull::protection_baseline(&repo);
    let origin = Origin::new(&repo);
    let upstream = origin.advance_protection();
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
    // Incoming and local edits touch opposite ends of a tracked path: direct
    // Pull needs protection, but the approved stash restore is a clean merge.
    std::fs::write(
        repo.join("protection.txt"),
        "top\n1\n2\n3\n4\n5\n6\n7\n8\nlocal\n",
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
    assert_eq!(oplog::records(&repo, "pull", 100, true).len(), 0);
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
        oplog::records(&repo, "stash-push", 100, true).is_empty(),
        "refusal must precede any stash write"
    );
    assert!(
        oplog::records(&repo, "stash-pop", 100, true).is_empty(),
        "refusal must not need restoration"
    );
    let pulls = oplog::records(&repo, "pull", 100, true);
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
            kagi::ui::i18n::op::pull_identity_changed_contract(kagi::ui::i18n::lang())
        }
        AutoStashGuardDrift::DirtyPaths => {
            kagi::ui::i18n::op::pull_files_changed_contract(kagi::ui::i18n::lang())
        }
        AutoStashGuardDrift::RestorePreview => {
            kagi::ui::i18n::op::pull_restore_changed_contract(kagi::ui::i18n::lang())
        }
    };
    assert_eq!(
        reason, expected_reason,
        "the real changed promise must select its typed cause"
    );
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            matches!(&state.status_footer, FooterStatus::Failed(message) if !message.is_empty())
        );
        let toasts = state
            .toast_stack
            .as_ref()
            .expect("actual toast stack")
            .read(cx)
            .toasts();
        assert_eq!(
            toasts
                .iter()
                .filter(|toast| toast.kind == ToastKind::Error)
                .count(),
            1
        );
        let visible = state
            .op_log
            .as_ref()
            .expect("visible Operation Log")
            .read(cx);
        assert_eq!(
            visible
                .entries()
                .iter()
                .filter(|entry| entry.id == pulls[0].id)
                .count(),
            1
        );
        let details = kagi::ui::oplog_panel::detail_lines(&pulls[0]).join("\n");
        assert!(
            details.contains(reason),
            "Details retain the recorded cause"
        );
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
        crate::recovery_pull::protection_baseline(&repo);
        let origin = Origin::new(&repo);
        let cached = origin.advance_protection();
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
        std::fs::write(
            repo.join("protection.txt"),
            "top\n1\n2\n3\n4\n5\n6\n7\n8\nlocal\n",
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
        let pulls = oplog::records(&repo, "pull", 100, true);
        assert_eq!(pulls.len(), 1);
        assert!(
            matches!(pulls[0].outcome, OpOutcome::Success { .. }),
            "{:?}",
            pulls[0].outcome
        );
        assert_eq!(oplog::records(&repo, "stash-push", 100, true).len(), 1);
        assert_eq!(oplog::records(&repo, "stash-pop", 100, true).len(), 1);
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

#[path = "../support/oplog.rs"]
mod oplog;

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn activate_toolbar(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &'static str) {
    draw(cx, window);
    assert_eq!(e2e::toolbar_unavailable(id), Some(false));
    for _ in 0..120 {
        cx.update_window(window, |_, window, cx| {
            window.focus_next(cx);
            window.draw(cx).clear();
        })
        .unwrap();
        let _ = e2e::take_focus_probe();
        cx.simulate_keystrokes(window, "f19");
        if e2e::take_focus_probe() == Some(id) {
            crate::keyboard_nav::keys(cx, window, "enter");
            return;
        }
    }
    panic!("native toolbar button {id} is not keyboard reachable");
}

/// The toolbar intent itself approves a safe Pull, including a split staged/
/// unstaged path. A stash/pop would lose this index OID even if bytes survived.
pub fn scenario_pull_safe_dirty_intent(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for advance in [false, true] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let origin = Origin::new(&repo);
        std::fs::write(repo.join("README.md"), "staged local content\n").unwrap();
        git(&repo, &["add", "README.md"]);
        git(&repo, &["update-index", "--chmod=+x", "README.md"]);
        std::fs::write(repo.join("README.md"), "unstaged local content\n").unwrap();
        std::fs::write(repo.join("scratch.txt"), "untracked local content\n").unwrap();
        let staged = output(&repo, &["ls-files", "--stage", "--", "README.md"]);
        let before = Checkout::read(&repo);
        let (app, window) = mount(cx, &repo);
        let expected = if advance {
            origin.advance()
        } else {
            before.head.clone()
        };
        let (hold, release) = deferred::<()>(cx);
        KagiApp::hold_next_fetch_for_e2e(hold);
        activate_toolbar(cx, window, "tb-pull");
        assert_waiting(cx, &app, &repo, &before);
        release.send(());
        wait_idle(cx, &app);
        cx.run_until_parked();
        assert_eq!(output(&repo, &["rev-parse", "HEAD"]), expected);
        assert_eq!(
            output(&repo, &["ls-files", "--stage", "--", "README.md"]),
            staged
        );
        let after = Checkout::read(&repo);
        for (path, bytes) in &before.files {
            assert_eq!(
                after.files.get(path),
                Some(bytes),
                "preserved {}",
                path.display()
            );
        }
        if advance {
            assert_eq!(
                output(&repo, &["show", ":upstream.txt"]),
                "fresh upstream content"
            );
        } else {
            assert_eq!(after, before, "up-to-date dirty intent is not a write");
        }
        assert!(oplog::records(&repo, "stash-push", 100, true).is_empty());
        assert!(oplog::records(&repo, "stash-pop", 100, true).is_empty());
        let pulls = oplog::records(&repo, "pull", 100, true);
        assert_eq!(pulls.len(), usize::from(advance));
        assert!(pulls
            .iter()
            .all(|entry| matches!(entry.outcome, OpOutcome::Success { .. })));
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.pull_modal().is_none());
            assert!(!e2e::active_modal_present(app));
            assert!(!app.app_sessions.has_leases());
        });
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS pull_safe_dirty_intent");
}

pub fn scenario_pull_safe_merge_intent(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    std::fs::write(repo.join("local-commit.txt"), "local history\n").unwrap();
    git(&repo, &["add", "local-commit.txt"]);
    git(&repo, &["commit", "-qm", "noninterfering local commit"]);
    let local = output(&repo, &["rev-parse", "HEAD"]);
    let incoming = origin.advance();
    let (app, window) = mount(cx, &repo);
    activate_toolbar(cx, window, "tb-pull");
    wait_idle(cx, &app);
    cx.run_until_parked();
    let parents = output(&repo, &["show", "-s", "--format=%P", "HEAD"]);
    assert_eq!(parents, format!("{local} {incoming}"));
    assert_eq!(
        output(&repo, &["show", ":local-commit.txt"]),
        "local history"
    );
    assert_eq!(
        output(&repo, &["show", ":upstream.txt"]),
        "fresh upstream content"
    );
    assert!(output(&repo, &["status", "--porcelain"]).is_empty());
    assert!(oplog::records(&repo, "stash-push", 100, true).is_empty());
    let pulls = oplog::records(&repo, "pull", 100, true);
    assert_eq!(pulls.len(), 1);
    assert!(matches!(pulls[0].outcome, OpOutcome::Success { .. }));
    cx.read(|cx| assert!(!e2e::active_modal_present(app.read(cx))));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_safe_merge_intent");
}

pub fn scenario_push_safe_intent(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    git(
        &repo,
        &["commit", "-qm", "safe outgoing commit", "--allow-empty"],
    );
    std::fs::write(repo.join("README.md"), "staged local content\n").unwrap();
    git(&repo, &["add", "README.md"]);
    std::fs::write(repo.join("README.md"), "unstaged local content\n").unwrap();
    let before = Checkout::read(&repo);
    let remote_before = output(&origin.other, &["ls-remote", "origin", "refs/heads/main"]);
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    activate_toolbar(cx, window, "tb-push");
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.app_sessions.has_leases(), "button intent admitted Push");
        assert!(
            app.push_modal().is_none(),
            "safe Push has no extra approval"
        );
    });
    assert_eq!(Checkout::read(&repo), before);
    assert_eq!(
        output(&origin.other, &["ls-remote", "origin", "refs/heads/main"]),
        remote_before
    );
    release.send(());
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert_eq!(Checkout::read(&repo), before);
    assert_eq!(
        output(&origin.other, &["ls-remote", "origin", "refs/heads/main"]),
        format!("{}\trefs/heads/main", before.head)
    );
    let pushes = oplog::records(&repo, "push", 100, true);
    assert_eq!(pushes.len(), 1);
    assert!(matches!(pushes[0].outcome, OpOutcome::Success { .. }));
    cx.read(|cx| assert!(!e2e::active_modal_present(app.read(cx))));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS push_safe_intent");
}

/// Committed-history conflicts have a real Merge/Resolve route; stashing a
/// clean checkout cannot cure the competing committed contents.
pub fn scenario_pull_history_conflict_choice(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    std::fs::write(repo.join("upstream.txt"), "committed local side\n").unwrap();
    git(&repo, &["add", "upstream.txt"]);
    git(&repo, &["commit", "-qm", "competing local history"]);
    let before = Checkout::read(&repo);
    let incoming = origin.advance();
    let (app, window) = mount(cx, &repo);
    activate_toolbar(cx, window, "tb-pull");
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("history conflict choice");
        assert!(
            !modal.auto_stash,
            "history conflict must never claim stash is its cure"
        );
    });
    assert_eq!(Checkout::read(&repo), before);
    assert!(oplog::records(&repo, "pull", 100, true).is_empty());
    press_enter(cx, &app, window);
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.pull_modal().is_none());
        assert!(matches!(
            app.merge_modal()
                .expect("existing conflict resolution choice")
                .kind,
            kagi_git::MergeKind::Conflicts(_)
        ));
    });
    assert_eq!(
        Checkout::read(&repo),
        before,
        "review is not merge approval"
    );
    draw(cx, window);
    let confirm = e2e::control_bounds(window.window_id(), "plan-confirm").unwrap();
    cx.simulate_click(window, confirm.center(), gpui::Modifiers::none());
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert_eq!(output(&repo, &["rev-parse", "HEAD"]), before.head);
    assert_eq!(output(&repo, &["rev-parse", "MERGE_HEAD"]), incoming);
    assert!(output(&repo, &["ls-files", "--unmerged"]).contains("upstream.txt"));
    assert!(oplog::records(&repo, "stash-push", 100, true).is_empty());
    assert!(oplog::records(&repo, "stash-pop", 100, true).is_empty());
    assert_eq!(oplog::records(&repo, "merge", 100, true).len(), 1);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.pull_modal().is_none());
        assert!(app.merge_modal().is_none());
        assert!(
            app.ui().conflict.is_some(),
            "existing Conflict Mode owns the real merge conflict"
        );
        assert!(!app.app_sessions.has_leases());
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_history_conflict_choice");
}

pub fn scenario_pull_quiet_late_dirty_refuses(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    let incoming = origin.advance();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    activate_toolbar(cx, window, "tb-pull");
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.pull_modal().is_none());
        assert!(app.app_sessions.has_leases());
    });
    std::fs::write(repo.join("upstream.txt"), "late unapproved local content\n").unwrap();
    let changed = Checkout::read(&repo);
    release.send(());
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert_eq!(
        Checkout::read(&repo),
        changed,
        "late dirty edit cannot be consumed"
    );
    assert_eq!(output(&repo, &["rev-parse", "origin/main"]), incoming);
    assert!(
        oplog::records(&repo, "stash-push", 100, true).is_empty(),
        "intent never approved stash"
    );
    assert!(oplog::records(&repo, "stash-pop", 100, true).is_empty());
    let pulls = oplog::records(&repo, "pull", 100, true);
    assert_eq!(
        pulls.len(),
        1,
        "one admitted attempt, one refusal/failure receipt"
    );
    assert!(matches!(
        pulls[0].outcome,
        OpOutcome::Refused { .. } | OpOutcome::Failed { .. }
    ));
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.app_sessions.has_leases());
        assert!(matches!(app.status_footer, FooterStatus::Failed(_)));
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_quiet_late_dirty_refuses");
}

pub fn scenario_push_non_ff_refuses(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    git(
        &repo,
        &[
            "commit",
            "-qm",
            "outgoing divergent commit",
            "--allow-empty",
        ],
    );
    let before = Checkout::read(&repo);
    let incoming = origin.advance();
    let (app, window) = mount(cx, &repo);
    activate_toolbar(cx, window, "tb-push");
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert_eq!(Checkout::read(&repo), before);
    assert_eq!(
        output(&origin.other, &["ls-remote", "origin", "refs/heads/main"]),
        format!("{incoming}\trefs/heads/main"),
        "ordinary intent cannot force a remote tip"
    );
    let pushes = oplog::records(&repo, "push", 100, true);
    assert_eq!(pushes.len(), 1);
    assert!(matches!(pushes[0].outcome, OpOutcome::Failed { .. }));
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.push_modal().is_none());
        assert!(!app.app_sessions.has_leases());
        assert!(matches!(app.status_footer, FooterStatus::Failed(_)));
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS push_non_ff_refuses");
}

pub(super) fn click_pull(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    activate_toolbar(cx, window, "tb-pull");
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
    e2e::clear_control_bounds(window.window_id(), "branch-menu-item-1-0");
    draw(cx, window);
    let root_pull = e2e::control_bounds(window.window_id(), "branch-menu-item-1-0");
    let id = if ff_only || root_pull.is_none() {
        let advanced = e2e::control_bounds(window.window_id(), "branch-menu-item-3-2")
            .expect("Advanced submenu header is drawn");
        cx.simulate_click(window, advanced.center(), gpui::Modifiers::none());
        draw(cx, window);
        if ff_only {
            "branch-menu-item-3-2-submenu-3-1"
        } else {
            "branch-menu-item-3-2-submenu-2-0"
        }
    } else {
        "branch-menu-item-1-0"
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
        oplog::records(repo, "pull", 100, true).is_empty(),
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
            .expect("predicted history conflict requires a choice");
        assert!(
            !modal.auto_stash,
            "committed history is not repaired by stash"
        );
        assert!(modal.error.is_none());
        assert!(
            modal.plan.blockers.is_empty(),
            "history conflict review is available"
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

pub(super) fn clean_updates_from(
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
    wait_idle(cx, &app);
    cx.run_until_parked();
    cx.read(|cx| assert!(!e2e::active_modal_present(app.read(cx))));
    assert_eq!(
        output(&repo, &["rev-parse", "origin/main"]),
        expected,
        "the real fetch observed origin's new tip"
    );
    assert_eq!(
        latest_count(cx, &app),
        0,
        "fresh changes were pulled, not latest"
    );
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
    let durable = oplog::records(&repo, "pull", 100, true);
    assert_eq!(durable.len(), 1, "one Pull intent, one durable receipt");
    assert!(matches!(durable[0].outcome, OpOutcome::Success { .. }));
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!e2e::active_modal_present(app));
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

pub fn scenario_pull_current_slash_remote(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for click in [click_pull, click_branch_pull] {
        current_configured_upstream_updates(cx, "team/origin", click);
    }
    eprintln!("[gui-e2e] PASS pull_current_slash_remote");
}

pub fn scenario_pull_current_local_upstream(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for click in [click_pull, click_branch_pull] {
        current_configured_upstream_updates(cx, ".", click);
    }
    eprintln!("[gui-e2e] PASS pull_current_local_upstream");
}

fn current_configured_upstream_updates(
    cx: &mut VisualTestAppContext,
    remote: &str,
    click: fn(&mut VisualTestAppContext, AnyWindowHandle),
) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("pull-remove.txt"), "remove this tracked path\n").unwrap();
    git(&repo, &["add", "pull-remove.txt"]);
    git(
        &repo,
        &["commit", "-q", "-m", "regular Pull removal baseline"],
    );
    let origin = Origin::new(&repo);
    let local_upstream = remote == ".";
    let alternate = (!local_upstream).then(|| Origin::new_named(&repo, remote));
    let upstream_ref = if local_upstream {
        git(&repo, &["branch", "local-upstream"]);
        git(
            &repo,
            &["branch", "--set-upstream-to=local-upstream", "main"],
        );
        "refs/heads/local-upstream".to_string()
    } else {
        format!("refs/remotes/{remote}/main")
    };
    assert_eq!(output(&repo, &["config", "branch.main.remote"]), remote);
    let (app, window) = mount(cx, &repo);
    cx.read(|cx| {
        let summary = &app.read(cx).view().status_summary;
        assert_eq!(
            summary.behind,
            Some(0),
            "ordinary current Pull starts with cached zero"
        );
        assert!(!summary.no_upstream);
    });
    let upstream = alternate.as_ref().unwrap_or(&origin);
    let expected = upstream.advance_tree();
    let expected_checkout = Checkout::read(&upstream.other);
    if local_upstream {
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
    let before = Checkout::read(&repo);
    let refs = || -> BTreeMap<String, String> {
        output(&repo, &["show-ref"])
            .lines()
            .map(|line| {
                let (oid, name) = line.split_once(' ').unwrap();
                (name.to_string(), oid.to_string())
            })
            .collect()
    };
    let before_refs = refs();
    let fetch_head_before = std::fs::read(repo.join(".git/FETCH_HEAD")).ok();
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_fetch_for_e2e(hold);
    // The current branch's ordinary menu item and toolbar both request
    // PullTarget::Current; neither enters the branch ff-only operation.
    click(cx, window);
    assert_waiting(cx, &app, &repo, &before);
    assert_eq!(
        refs(),
        before_refs,
        "ordinary Pull fetch hold leaves all refs intact"
    );
    release.send(());
    cx.run_until_parked();
    let fetches = oplog::records(&repo, "fetch", 100, true);
    assert!(
        fetches
            .iter()
            .all(|entry| matches!(entry.outcome, OpOutcome::Success { .. })),
        "ordinary current Pull must fetch its exact configured remote: {fetches:?}"
    );
    assert_ne!(
        std::fs::read(repo.join(".git/FETCH_HEAD")).ok(),
        fetch_head_before,
        "ordinary current Pull completes its real fetch"
    );
    assert_eq!(output(&repo, &["rev-parse", &upstream_ref]), expected);
    wait_idle(cx, &app);
    cx.run_until_parked();
    let mut expected_refs = before_refs.clone();
    // Fetch may create a remote HEAD alias; Pull owns the local branch update.
    expected_refs.retain(|name, _| name.starts_with("refs/heads/"));
    if !local_upstream {
        assert_eq!(fetches.len(), 1);
    }
    for _ in 0..2 {
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| assert!(app.read(cx).pull_modal().is_none()));
        assert_eq!(Checkout::read(&repo), expected_checkout);
    }
    assert_eq!(
        Checkout::read(&repo),
        expected_checkout,
        "ordinary Pull updates HEAD, all index entries and all files"
    );
    assert_eq!(output(&repo, &["symbolic-ref", "--short", "HEAD"]), "main");
    assert!(output(&repo, &["status", "--porcelain"]).is_empty());
    expected_refs.insert("refs/heads/main".to_string(), expected);
    let mut actual_local_refs = refs();
    actual_local_refs.retain(|name, _| name.starts_with("refs/heads/"));
    assert_eq!(
        actual_local_refs, expected_refs,
        "ordinary Pull changes only its selected local branch"
    );
    let durable = oplog::records(&repo, "pull", 100, true);
    assert_eq!(durable.len(), 1, "one ordinary Pull durable receipt");
    assert!(matches!(durable[0].outcome, OpOutcome::Success { .. }));
    assert_eq!(
        oplog::records(&repo, "fetch", 100, true).len(),
        fetches.len()
    );
    assert!(oplog::records(&repo, "stash-push", 100, true).is_empty());
    assert!(oplog::records(&repo, "stash-pop", 100, true).is_empty());
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.pull_modal().is_none());
        assert!(app.branch_plan_modal().is_none());
        assert!(!app.app_sessions.has_leases());
        let panel = app.op_log.as_ref().unwrap().read(cx);
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.op == "pull" && entry.repo == durable[0].repo)
            .collect();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].id, durable[0].id);
    });
    unmount(cx, app, window);
}

/// Current normal and strict-FF Pull share the same fresh, localized no-op.
pub fn scenario_pull_current_up_to_date(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        for ff_only in [false, true] {
            for ahead in [false, true] {
                current_up_to_date(cx, ff_only, ahead);
            }
        }
    }
    eprintln!("[gui-e2e] PASS pull_current_up_to_date");
}

fn current_up_to_date(cx: &mut VisualTestAppContext, ff_only: bool, ahead: bool) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let _origin = Origin::new(&repo);
    git(&repo, &["remote", "set-head", "origin", "-a"]);
    if ahead {
        git(&repo, &["commit", "-qm", "local ahead", "--allow-empty"]);
    }
    std::fs::write(repo.join("README.md"), "staged local content\n").unwrap();
    git(&repo, &["add", "README.md"]);
    std::fs::write(repo.join("README.md"), "unstaged local content\n").unwrap();
    std::fs::write(repo.join("untracked.txt"), "untracked local content\n").unwrap();
    let before = Checkout::read(&repo);
    let refs_before = output(&repo, &["show-ref"]);
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_fetch_for_e2e(hold);
    if ff_only {
        click_branch_sync(cx, window, "main", true);
    } else {
        click_pull(cx, window);
    }
    assert_waiting(cx, &app, &repo, &before);
    release.send(());
    cx.run_until_parked();
    assert_eq!(latest_count(cx, &app), 1, "fresh Pull explains its no-op");
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(!app.app_sessions.has_leases());
        assert!(app.pull_modal().is_none());
        assert!(app.branch_plan_modal().is_none());
        let toasts = app
            .toast_stack
            .as_ref()
            .expect("Pull outcome toast")
            .read(cx);
        let latest = toasts.toasts().last().expect("explanatory Pull toast");
        assert_eq!(latest.kind, ToastKind::Sync);
        assert_eq!(latest.message.as_ref(), Msg::AlreadyUpToDatePull.t());
        assert!(matches!(&app.status_footer, FooterStatus::Idle(text) if text.is_empty()));
    });
    // The fetch's watcher must neither lose the explanation nor open a card.
    app.update(cx, |app, cx| app.reload_external(cx));
    cx.run_until_parked();
    assert_eq!(latest_count(cx, &app), 1);
    assert_eq!(Checkout::read(&repo), before);
    assert_eq!(output(&repo, &["show-ref"]), refs_before);
    for op in ["pull", "branch-pull-ff", "stash-push", "stash-pop"] {
        assert!(
            oplog::records(&repo, op, 100, true).is_empty(),
            "no fabricated {op} execution"
        );
    }
    unmount(cx, app, window);
}

/// Equal tips do not erase a conflict that appears during the freshness fetch.
pub fn scenario_pull_current_ff_only_blocker(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        git(&repo, &["checkout", "-qb", "conflicting"]);
        std::fs::write(repo.join("README.md"), "other committed content\n").unwrap();
        git(&repo, &["commit", "-qam", "other conflict side"]);
        git(&repo, &["checkout", "-q", "main"]);
        std::fs::write(repo.join("README.md"), "main committed content\n").unwrap();
        git(&repo, &["commit", "-qam", "main conflict side"]);
        let _origin = Origin::new(&repo);
        git(&repo, &["remote", "set-head", "origin", "-a"]);
        let (app, window) = mount(cx, &repo);
        let before_fetch = Checkout::read(&repo);
        let (hold, release) = deferred::<()>(cx);
        KagiApp::hold_next_fetch_for_e2e(hold);
        click_branch_sync(cx, window, "main", true);
        assert_waiting(cx, &app, &repo, &before_fetch);
        assert!(!git_fixture::git_succeeds(
            &repo,
            &["merge", "--no-commit", "conflicting"]
        ));
        let conflicted = Checkout::read(&repo);
        let refs_before = output(&repo, &["show-ref"]);
        let merge_head = std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap();
        release.send(());
        cx.run_until_parked();
        let fetched = std::fs::read_to_string(repo.join(".git/FETCH_HEAD"))
            .expect("freshness fetch completed");
        assert!(fetched.contains(&before_fetch.head));
        assert!(oplog::records(&repo, "fetch", 100, true)
            .iter()
            .all(|entry| matches!(entry.outcome, OpOutcome::Success { .. })));
        let plan = kagi_git::Backend::open(&repo)
            .unwrap()
            .plan_pull_branch_ff("main")
            .unwrap();
        assert_eq!(plan.disposition, kagi_git::ops::PlanDisposition::Blocked);
        assert!(plan.blockers.iter().any(|note| matches!(
            note,
            kagi_domain::plan_note::PlanNote::Common(
                kagi_domain::plan_note::CommonNote::ConflictedFiles { .. }
            )
        )));
        assert_eq!(latest_count(cx, &app), 0, "conflicts are not a Pull no-op");
        assert_eq!(Checkout::read(&repo), conflicted);
        assert_eq!(output(&repo, &["show-ref"]), refs_before);
        assert_eq!(
            std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
            merge_head
        );
        for op in ["pull", "branch-pull-ff", "stash-push", "stash-pop"] {
            assert!(oplog::records(&repo, op, 100, true).is_empty());
        }
        cx.read(|cx| assert!(!app.read(cx).app_sessions.has_leases()));
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS pull_current_ff_only_blocker");
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
        oplog::records(&repo, "pull", 100, true).is_empty(),
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
        let failures = oplog::records(&repo, "fetch", 100, true);
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
        assert!(oplog::records(&repo, "pull", 100, true).is_empty());
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
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert_eq!(output(&repo, &["rev-parse", "origin/main"]), expected);
    assert_eq!(output(&repo, &["rev-parse", "HEAD"]), expected);
    assert_eq!(
        oplog::records(&repo, "pull", 100, true).len(),
        1,
        "joined requests execute once"
    );
    assert!(oplog::records(&repo, "stash-push", 100, true).is_empty());
    cx.read(|cx| assert!(app.read(cx).pull_modal().is_none()));
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
    assert!(oplog::records(repo, "pull", 100, true).is_empty());
    assert_eq!(latest_count(cx, app), 0);
}

fn external_change_invalidates(cx: &mut VisualTestAppContext, head_change: bool) {
    let _settings = SettingsGuard::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    std::fs::write(repo.join("upstream.txt"), "local committed side\n").unwrap();
    git(&repo, &["add", "upstream.txt"]);
    git(&repo, &["commit", "-qm", "conflicting local history"]);
    let (app, window) = mount(cx, &repo);
    let expected = origin.advance();
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
            Some(1 + usize::from(head_change))
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
        assert!(oplog::records(&repo, "pull", 100, true).is_empty());
        assert!(oplog::records(&repo, "create-branch", 100, true).is_empty());
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS pull_freshness_closed_modal_displaces");
}

/// A captured unchanged snapshot cannot authorize a retained risk choice after
/// the checkout changes. Safe branch FF has no retained choice after #1697;
/// its immediate admission and held-run invariants live in branch_ref_updates.
pub fn scenario_pull_freshness_captured_reload_drift(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    #[derive(Clone, Copy)]
    enum Drift {
        Head,
        Worktree,
        Upstream,
    }
    for drift in [Drift::Head, Drift::Worktree, Drift::Upstream] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let origin = Origin::new(&repo);
        std::fs::write(repo.join("upstream.txt"), "local committed side\n").unwrap();
        git(&repo, &["add", "upstream.txt"]);
        git(&repo, &["commit", "-qm", "conflicting local history"]);
        let (app, window) = mount(cx, &repo);
        let expected = origin.advance();
        if matches!(drift, Drift::Upstream) {
            git(&origin.other, &["push", "-q", "origin", "main:alternate"]);
        }
        let before = Checkout::read(&repo);
        fetch_and_confirm(cx, &app, window, &repo, &before);

        let (owner, revision) = app.update(cx, |app, cx| {
            let owner = app.active_session().unwrap();
            app.reload_external(cx);
            (owner, app.reads.revision(owner))
        });
        // Finish the real snapshot worker without accepting its read yet.
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
                "original risk choice is still shown"
            );
        });
        match drift {
            Drift::Head => git(
                &repo,
                &[
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "drift after snapshot",
                ],
            ),
            Drift::Worktree => {
                std::fs::write(repo.join("README.md"), "edit after captured snapshot\n").unwrap();
            }
            Drift::Upstream => git(
                &repo,
                &["branch", "--set-upstream-to=origin/alternate", "main"],
            ),
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
            assert_eq!(app.view().status_summary.ahead, Some(1));
            assert_eq!(app.view().status_summary.behind, Some(1));
            assert!(
                app.pull_modal().is_none(),
                "live replan must invalidate, not rewrite the old risk choice"
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
        assert!(oplog::records(&repo, "pull", 100, true).is_empty());
        assert!(output(&repo, &["stash", "list"]).is_empty());
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS pull_freshness_captured_reload_drift");
}

/// Noncurrent branch-menu variants must refresh their selected live upstream.
/// The noncurrent legs deliberately leave main on origin while feature tracks
/// either origin/main or a distinct remote: fetching main's remote is insufficient.
pub fn scenario_pull_freshness_branch_entries(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
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
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        for confirm in [false, true] {
            strict_ff_overlap_card(cx, language, confirm);
        }
    }
    eprintln!("[gui-e2e] PASS pull_ff_only_current_tree_consistency");
}

/// The strict FF card uses the generic Cancel/Pull footer, not ordinary
/// Pull's stash/review choices. Both cancelling and attempting it keep work.
fn strict_ff_overlap_card(cx: &mut VisualTestAppContext, language: Lang, confirm: bool) {
    use kagi::ui::{button_style, dialog_a11y};

    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let origin = Origin::new(&repo);
    std::fs::write(origin.other.join("README.md"), "incoming README\n").unwrap();
    git(&origin.other, &["commit", "-qam", "incoming overlap"]);
    git(&origin.other, &["push", "-q", "origin", "main"]);
    std::fs::write(repo.join("README.md"), "local staged README\n").unwrap();
    git(&repo, &["add", "README.md"]);
    std::fs::write(repo.join("README.md"), "local unstaged README\n").unwrap();
    std::fs::write(repo.join("untracked.txt"), "preserved untracked\n").unwrap();
    let before = Checkout::read(&repo);
    let (app, window) = mount(cx, &repo);
    click_branch_sync(cx, window, "main", true);
    assert_fresh_branch_confirm(cx, &app, window, "main");
    let refs_before = output(&repo, &["show-ref"]);
    button_style::clear_recorded_modal_buttons();
    draw(cx, window);
    let (_, advice) =
        dialog_a11y::recorded_note("plan-warning-0").expect("actual strict FF caution is rendered");
    let expected = match language {
        Lang::En => "Incoming changes would overwrite local changes in:\n  - README.md\nCancel this pull, commit or stash the changes to these paths, then retry the fast-forward-only pull.",
        Lang::Ja => "incoming の変更が次のローカル変更を上書きします:\n  - README.md\nこの Pull をキャンセルし、対象パスの変更を commit または stash してから、fast-forward のみの Pull を再試行してください。",
    };
    assert_eq!(advice, expected);
    for nonexistent in [
        "Stash & Pull",
        "Review changes",
        "StashしてPull",
        "変更を確認",
    ] {
        assert!(!advice.contains(nonexistent), "{advice}");
    }
    for id in ["pull-stash-changes", "pull-review-changes"] {
        assert!(
            button_style::recorded_modal_button(id).is_none(),
            "strict FF must not claim ordinary Pull choices"
        );
    }
    assert_eq!(
        button_style::recorded_modal_button("plan-cancel")
            .unwrap()
            .label,
        Msg::PlanCancel.t()
    );
    assert!(
        !button_style::recorded_modal_button("plan-confirm")
            .unwrap()
            .disabled
    );
    let action = if confirm {
        "plan-confirm"
    } else {
        "plan-cancel"
    };
    let bounds = e2e::control_bounds(window.window_id(), action).unwrap();
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    wait_idle(cx, &app);
    assert_eq!(Checkout::read(&repo), before);
    assert_eq!(output(&repo, &["show-ref"]), refs_before);
    assert!(output(&repo, &["stash", "list"]).is_empty());
    for op in ["stash-push", "stash-pop"] {
        assert!(oplog::records(&repo, op, 100, true).is_empty());
    }
    if confirm {
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(
                matches!(&state.status_footer, FooterStatus::Failed(_))
                    || state
                        .branch_plan_modal()
                        .is_some_and(|modal| modal.error.is_some()),
                "strict FF execution must report its dirty-path refusal"
            );
        });
    } else {
        assert!(oplog::records(&repo, "branch-pull-ff", 100, true).is_empty());
        cx.read(|cx| assert!(app.read(cx).branch_plan_modal().is_none()));
    }
    cx.read(|cx| assert!(!app.read(cx).app_sessions.has_leases()));
    unmount(cx, app, window);
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

pub(super) fn branch_ref_updates(
    cx: &mut VisualTestAppContext,
    current: bool,
    ff_only: bool,
    remote: &str,
) {
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
    let (run_hold, run_release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(run_hold);
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
        "the menu's real fetch must finish before execution"
    );
    assert!(
        oplog::records(&repo, "fetch", 100, true)
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
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            !e2e::active_modal_present(app),
            "safe branch Pull stays quiet"
        );
        assert!(
            app.app_sessions.has_leases(),
            "normal lifecycle owns the run"
        );
        assert!(
            matches!(&app.status_footer, FooterStatus::Busy(message) if message.contains(branch)),
            "progress identifies selected target {branch}: {:?}",
            app.status_footer
        );
    });
    assert_eq!(output(&repo, &["rev-parse", &tracking]), expected);
    if different_remote {
        assert_eq!(output(&repo, &["rev-parse", "origin/main"]), origin_before);
    }
    assert_eq!(Checkout::read(&repo), before);
    assert_eq!(output(&repo, &["rev-parse", &target_ref]), target_before);
    assert!(oplog::records(&repo, "pull", 100, true).is_empty());
    let fetches = oplog::records(&repo, "fetch", 100, true);
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
        cx.run_until_parked();
        cx.read(|cx| assert!(!e2e::active_modal_present(app.read(cx))));
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

    run_release.send(());
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
        "quiet Pull moves only the selected local ref"
    );
    let durable = oplog::records(&repo, "pull", 100, true);
    assert_eq!(durable.len(), 1);
    assert!(matches!(durable[0].outcome, OpOutcome::Success { .. }));
    assert_eq!(
        oplog::records(&repo, "fetch", 100, true).len(),
        fetches.len(),
        "execution adds no extra fetch receipt"
    );
    assert!(oplog::records(&repo, "stash-push", 100, true).is_empty());
    assert!(oplog::records(&repo, "stash-pop", 100, true).is_empty());
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.branch_plan_modal().is_none());
        assert!(!e2e::active_modal_present(app));
        assert!(
            matches!(&app.status_footer, FooterStatus::Success(message) if message.contains(branch)),
            "completion identifies selected target {branch}: {:?}",
            app.status_footer
        );
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
        let pull_prefix = format!("{}:", i18n::op::user_text::operation_label("pull"));
        assert_eq!(
            toasts
                .toasts()
                .iter()
                .filter(|toast| {
                    toast.kind == ToastKind::Success && toast.message.starts_with(&pull_prefix)
                })
                .count(),
            1,
            "one Pull intent delivers one success toast"
        );
    });
    unmount(cx, app, window);
}
