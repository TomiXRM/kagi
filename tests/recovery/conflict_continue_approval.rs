//! #1721: an empty conflict session is not authority to reuse an old approval.
use crate::app_conflict::{click_control, wait_idle};
use crate::conflict_draft_revision::{
    conflict_read, detect, open_continue, paint, replace_result, result_input, FILE, FIRST,
};
use crate::macos::{mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{FooterStatus, KagiApp};
use kagi_domain::conflict_family::ConflictRevision;
use kagi_git::oplog::OpOutcome;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

#[path = "../support/git_fixture.rs"]
mod fixture_git;
#[path = "../support/oplog.rs"]
mod oplog;

const REPLACEMENT: &str = "replacement staged resolution\n";
const UNRELATED: &str = "unrelated explicitly approved content\n";

struct Fixture {
    directory: tempfile::TempDir,
    incoming: git2::Oid,
    base: git2::Oid,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path();
        fixture_git::init_repo(repo, "main");
        fixture_git::write_file(repo, FILE, "base\n");
        fixture_git::commit_all(repo, "base");
        fixture_git::git(repo, &["checkout", "-qb", "side"]);
        fixture_git::write_file(repo, FILE, "side\n");
        fixture_git::commit_all(repo, "one incoming step");
        let incoming = oid(repo, "HEAD");
        fixture_git::git(repo, &["checkout", "-q", "main"]);
        fixture_git::write_file(repo, FILE, "main\n");
        fixture_git::commit_all(repo, "original main base");
        let base = oid(repo, "HEAD");
        fixture_git::git(repo, &["checkout", "-q", "side"]);
        assert!(!fixture_git::git_succeeds(repo, &["rebase", "main"]));
        Self {
            directory,
            incoming,
            base,
        }
    }

    fn path(&self) -> PathBuf {
        self.directory.path().canonicalize().unwrap()
    }

    fn replace_operation(&self) -> git2::Oid {
        let repo = self.path();
        let step = rebase_step(&repo);
        assert_eq!(oid(&repo, "REBASE_HEAD"), self.incoming);
        fixture_git::git(&repo, &["rebase", "--abort"]);
        fixture_git::git(&repo, &["checkout", "-q", "main"]);
        fixture_git::write_file(&repo, FILE, "different new main base\n");
        fixture_git::commit_all(&repo, "replacement main base");
        let base = oid(&repo, "HEAD");
        assert_ne!(base, self.base);
        fixture_git::git(&repo, &["checkout", "-q", "side"]);
        assert_eq!(oid(&repo, "HEAD"), self.incoming);
        assert!(!fixture_git::git_succeeds(&repo, &["rebase", "main"]));
        assert_eq!(oid(&repo, "REBASE_HEAD"), self.incoming);
        assert_eq!(
            rebase_step(&repo),
            step,
            "same commit and same sequencer step"
        );
        stage(&repo, FILE, REPLACEMENT);
        assert_eq!(oid(&repo, "HEAD"), base);
        base
    }
}

fn oid(repo: &Path, name: &str) -> git2::Oid {
    git2::Repository::open(repo)
        .unwrap()
        .revparse_single(name)
        .unwrap()
        .id()
}

fn rebase_step(repo: &Path) -> (Vec<u8>, Vec<u8>) {
    let git = git2::Repository::open(repo).unwrap().path().to_path_buf();
    (
        std::fs::read(git.join("rebase-merge/msgnum")).unwrap(),
        std::fs::read(git.join("rebase-merge/end")).unwrap(),
    )
}

fn stage(repo: &Path, path: &str, text: &str) {
    fixture_git::write_file(repo, path, text);
    fixture_git::git(repo, &["add", "--", path]);
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fixture_git::worktree_files(
        root,
        fixture_git::WorktreeFilesOptions {
            skip_nested_git: false,
            follow_directory_symlinks: false,
        },
    )
}

/// Oplog is intentionally separate: refusal must append a receipt, not mutate Git.
/// Include raw index, every object/pack, all refs/reflogs and real sequencer bytes.
#[derive(Debug, PartialEq, Eq)]
struct NoWrite {
    working: BTreeMap<PathBuf, Vec<u8>>,
    git: BTreeMap<PathBuf, Vec<u8>>,
    staged: String,
    refs: String,
    head: git2::Oid,
}

impl NoWrite {
    fn read(repo: &Path) -> Self {
        let gitdir = git2::Repository::open(repo).unwrap().path().to_path_buf();
        let git = files(&gitdir)
            .into_iter()
            .filter(|(path, _)| {
                let top = path.components().next().unwrap().as_os_str();
                [
                    "HEAD",
                    "ORIG_HEAD",
                    "REBASE_HEAD",
                    "CHERRY_PICK_HEAD",
                    "REVERT_HEAD",
                    "MERGE_HEAD",
                    "MERGE_MSG",
                    "MERGE_MODE",
                    "AUTO_MERGE",
                    "index",
                    "refs",
                    "packed-refs",
                    "logs",
                    "objects",
                    "rebase-merge",
                    "rebase-apply",
                    "sequencer",
                    "config",
                    "shallow",
                ]
                .iter()
                .any(|name| top == std::ffi::OsStr::new(name))
            })
            .collect();
        Self {
            working: files(repo),
            git,
            staged: fixture_git::git_output(repo, &["ls-files", "--stage", "-z"]),
            refs: fixture_git::git_output(
                repo,
                &["for-each-ref", "--format=%(refname) %(objectname)"],
            ),
            head: oid(repo, "HEAD"),
        }
    }
}

struct Release(Option<crate::evidence_support::Reply<()>>);

impl Release {
    fn send(&mut self) {
        if let Some(reply) = self.0.take() {
            reply.send(());
        }
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        self.send();
        // Clear only the existing slots; a taken task is released by the sender.
        KagiApp::clear_continue_confirm_hold_for_e2e();
        KagiApp::clear_continue_completion_hold_for_e2e();
    }
}

fn saved_empty(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
) -> ConflictRevision {
    detect(cx, app, window);
    let original_head = oid(repo, "HEAD");
    let input = result_input(cx, app, window);
    replace_result(cx, app, window, &input, FIRST);
    click_control(cx, window, "conflict-save");
    wait_idle(cx, app);
    crate::busy_dismiss::ready(cx, app);
    paint(cx, app, window);
    assert_eq!(
        oid(repo, "HEAD"),
        original_head,
        "actual Save must not continue"
    );
    let repository = git2::Repository::open(repo).unwrap();
    let index = repository.index().unwrap();
    assert!(!index.has_conflicts());
    let entry = index.get_path(Path::new(FILE), 0).unwrap();
    assert_eq!(
        repository.find_blob(entry.id).unwrap().content(),
        FIRST.as_bytes()
    );
    assert_eq!(std::fs::read(repo.join(FILE)).unwrap(), FIRST.as_bytes());
    drop(input);
    empty_revision(cx, app)
}

fn empty_revision(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> ConflictRevision {
    cx.read(|cx| {
        let state = app.read(cx);
        let mode = conflict_read(app, cx).mode.as_ref().unwrap();
        assert!(
            mode.session.files.is_empty(),
            "actual Save emptied the session"
        );
        assert!(mode.can_continue());
        assert_eq!(
            Some(&mode.revision),
            state.view().operation.as_ref().map(|op| op.revision())
        );
        mode.revision.clone()
    })
}

fn assert_refusal(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, repo: &Path) {
    let receipts = oplog::records(repo, "rebase-continue", 100, true);
    assert_eq!(
        receipts.len(),
        1,
        "one durable result for the actual old approval"
    );
    assert!(matches!(
        receipts[0].outcome,
        OpOutcome::Failed { .. } | OpOutcome::Refused { .. }
    ));
    assert!(receipts[0].ref_moves.as_ref().unwrap().is_empty());
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(!state.app_sessions.has_leases());
        assert!(state.conflict_continue_modal().is_none());
        assert!(state.app_notice().is_none());
        assert!(!matches!(state.status_footer, FooterStatus::Success(_)));
    });
}

fn fresh_approval(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    base: git2::Oid,
    expected: &str,
    unrelated: bool,
) {
    let before = NoWrite::read(repo);
    detect(cx, app, window);
    let revision = empty_revision(cx, app);
    open_continue(cx, app, window);
    cx.read(|cx| {
        assert_eq!(
            app.read(cx).conflict_continue_modal().unwrap().revision,
            revision
        )
    });
    assert_eq!(
        before,
        NoWrite::read(repo),
        "explicit refresh/replan is read-only"
    );
    click_control(cx, window, "plan-confirm");
    wait_idle(cx, app);
    crate::busy_dismiss::ready(cx, app);
    paint(cx, app, window);
    let repository = git2::Repository::open(repo).unwrap();
    let head = repository.find_commit(oid(repo, "HEAD")).unwrap();
    assert_ne!(head.id(), base);
    assert_eq!(head.parent_id(0).unwrap(), base);
    let tree = head.tree().unwrap();
    let entry = tree.get_path(Path::new(FILE)).unwrap();
    assert_eq!(
        repository.find_blob(entry.id()).unwrap().content(),
        expected.as_bytes()
    );
    assert_eq!(std::fs::read(repo.join(FILE)).unwrap(), expected.as_bytes());
    assert!(!repository.index().unwrap().has_conflicts());
    assert_eq!(repository.state(), git2::RepositoryState::Clean);
    if unrelated {
        let entry = tree.get_path(Path::new("unrelated.txt")).unwrap();
        assert_eq!(
            repository.find_blob(entry.id()).unwrap().content(),
            UNRELATED.as_bytes()
        );
    }
    let receipts = oplog::records(repo, "rebase-continue", 100, true);
    assert!(matches!(receipts[0].outcome, OpOutcome::Success { .. }));
    assert_eq!(
        receipts
            .iter()
            .filter(|r| matches!(r.outcome, OpOutcome::Success { .. }))
            .count(),
        1
    );
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.ui().conflict.is_none());
        assert!(!state.app_sessions.has_leases());
        assert!(state.conflict_continue_modal().is_none());
    });
}

#[derive(Clone, Copy)]
enum Drift {
    ReplacementQueued,
    Stage0Queued,
    ModalRevisionReplaced,
    UnrelatedReplan,
}

fn exercise(cx: &mut VisualTestAppContext, drift: Drift) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["auto_snapshot"]);
    let _settings = crate::pull_freshness::SettingsGuard::install();
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    kagi::ui::settings::write_setting("auto_snapshot", Some("false"));
    let fixture = Fixture::new();
    let repo = fixture.path();
    let (app, window) = mount(cx, &repo);
    // Unmount the owning root even when an oracle panics. All input/hold guards
    // inside the body unwind first, so neither tasks nor child entities survive.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(cx, &app, window, &fixture, drift);
    }));
    let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        unmount(cx, app, window);
    }));
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
    if let Err(error) = cleanup {
        std::panic::resume_unwind(error);
    }
}

/// Exercise the normal accepted-read owner without the full-reload modal sweep.
/// The snapshot is entirely real; render owns the subsequent async conflict
/// revalidation. No mode, observation, buffer or confirmation is assigned here.
fn accept_snapshot(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
) {
    let (owner, key, published) = app.update(cx, |app, _| {
        let owner = app.active_session().unwrap();
        (owner, app.reads.begin(owner), app.ui().view_publish_gen)
    });
    let mut backend = kagi_git::Backend::open(repo).unwrap();
    let snapshot = backend.snapshot(1_000).unwrap();
    let name = repo.file_name().unwrap().to_string_lossy();
    let view = kagi::ui::build_tab_view(&snapshot, &name);
    let revision = view.operation.as_ref().unwrap().revision().clone();
    app.update(cx, |app, cx| {
        assert_eq!(app.active_session(), Some(owner));
        assert!(app.accept_tab_view(key, view));
        assert!(app.ui().view_publish_gen > published);
        cx.notify();
    });
    paint(cx, app, window);
    crate::busy_dismiss::wait(
        cx,
        app,
        "accepted read revalidates retained conflict",
        |state| {
            state.active_session() == Some(owner)
                && !state.reads.is_loading(owner)
                && !state.ui().panes_revalidating()
        },
    );
    assert_eq!(empty_revision(cx, app), revision);
    paint(cx, app, window);
}

fn run(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    fixture: &Fixture,
    drift: Drift,
) {
    let repo = fixture.path();
    let revision = saved_empty(cx, app, window, &repo);
    assert!(oplog::records(&repo, "rebase-continue", 100, true).is_empty());
    open_continue(cx, app, window);
    let plan = cx.read(|cx| {
        let modal = app.read(cx).conflict_continue_modal().unwrap();
        assert_eq!(modal.revision, revision);
        modal.plan.clone()
    });
    if matches!(drift, Drift::UnrelatedReplan) {
        unrelated_replan(cx, app, window, fixture, &revision);
        return;
    }
    if matches!(drift, Drift::ModalRevisionReplaced) {
        // A real snapshot enters the existing accepted-read owner directly,
        // without full reload's intentional plan-modal retirement. Normal
        // retained-pane revalidation must project its new operation revision.
        stage(&repo, FILE, REPLACEMENT);
        let before = NoWrite::read(&repo);
        accept_snapshot(cx, app, window, &repo);
        assert_ne!(empty_revision(cx, app), revision);
        cx.read(|cx| {
            let modal = app.read(cx).conflict_continue_modal().unwrap();
            assert!(std::sync::Arc::ptr_eq(&modal.plan, &plan));
            assert_eq!(modal.revision, revision);
        });
        assert_eq!(before, NoWrite::read(&repo));
    }
    queued_refusal(cx, app, window, fixture, drift, &revision);
}

fn queued_refusal(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    fixture: &Fixture,
    drift: Drift,
    revision: &ConflictRevision,
) {
    let repo = fixture.path();
    let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
    let old_op = cx.read(|cx| {
        conflict_read(app, cx)
            .mode
            .as_ref()
            .unwrap()
            .session
            .op
            .clone()
    });
    let (hold, release) = crate::evidence_support::deferred(cx);
    let mut release = Release(Some(release));
    let (completion, completed) = crate::evidence_support::deferred(cx);
    let mut completed = Release(Some(completed));
    let reached = KagiApp::hold_next_continue_completion_for_e2e(completion);
    KagiApp::hold_next_continue_confirm_for_e2e(hold);
    click_control(cx, window, "plan-confirm");
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.active_session(), Some(owner));
        assert!(
            state.app_sessions.has_leases(),
            "actual Confirm admitted background work"
        );
        assert!(state.write_busy_op.is_some());
        assert_eq!(&state.conflict_continue_modal().unwrap().revision, revision);
    });
    assert!(
        !reached.load(Ordering::Acquire),
        "background execution is held"
    );
    assert!(oplog::records(&repo, "rebase-continue", 100, true).is_empty());
    let base = if matches!(drift, Drift::ReplacementQueued) {
        fixture.replace_operation()
    } else {
        if matches!(drift, Drift::Stage0Queued) {
            stage(&repo, FILE, REPLACEMENT);
        }
        fixture.base
    };
    assert_eq!(oid(&repo, "REBASE_HEAD"), fixture.incoming);
    // Independent backend read proves the CLI replacement has the same broad
    // ConflictOp, including commit/step. It grants no UI write authority.
    let backend = kagi_git::Backend::open(&repo).unwrap();
    let live = backend.detect_conflict_session().unwrap();
    assert_eq!(live.op, old_op);
    assert!(live.files.is_empty());
    let before = NoWrite::read(&repo);
    release.send();
    crate::busy_dismiss::wait(cx, app, "old approval reaches real BG completion", |_| {
        reached.load(Ordering::Acquire)
    });
    assert_eq!(
        before,
        NoWrite::read(&repo),
        "backend refuses before ANY Git mutation"
    );
    assert!(
        oplog::records(&repo, "rebase-continue", 100, true).is_empty(),
        "foreground receipt is still held; this is the actual BG boundary"
    );
    completed.send();
    wait_idle(cx, app);
    paint(cx, app, window);
    assert_refusal(cx, app, &repo);
    assert_eq!(
        before,
        NoWrite::read(&repo),
        "durable refusal also preserves replacement state"
    );
    assert_eq!(oplog::records(&repo, "rebase-continue", 100, true).len(), 1);
    drop(completed);
    drop(release);
    fresh_approval(cx, app, window, &repo, base, REPLACEMENT, false);
    assert_eq!(oplog::records(&repo, "rebase-continue", 100, true).len(), 2);
}

fn unrelated_replan(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    fixture: &Fixture,
    revision: &ConflictRevision,
) {
    let repo = fixture.path();
    // Retire the actual modal explicitly before using full reload: reload
    // independently sweeps plan modals and must not be mistaken for Cancel.
    click_control(cx, window, "plan-cancel");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).conflict_continue_modal().is_none()));
    // Same accepted revision remains usable; no blanket ban on empty sessions.
    let before = NoWrite::read(&repo);
    detect(cx, app, window);
    assert_eq!(&empty_revision(cx, app), revision);
    assert_eq!(before, NoWrite::read(&repo));
    stage(&repo, "unrelated.txt", UNRELATED);
    detect(cx, app, window);
    assert_ne!(&empty_revision(cx, app), revision);
    assert!(oplog::records(&repo, "rebase-continue", 100, true).is_empty());
    fresh_approval(cx, app, window, &repo, fixture.base, FIRST, true);
    assert_eq!(oplog::records(&repo, "rebase-continue", 100, true).len(), 1);
}

pub fn scenario_conflict_continue_approval_replacement_queued(cx: &mut VisualTestAppContext) {
    exercise(cx, Drift::ReplacementQueued);
}

pub fn scenario_conflict_continue_approval_stage0_queued(cx: &mut VisualTestAppContext) {
    exercise(cx, Drift::Stage0Queued);
}

pub fn scenario_conflict_continue_approval_modal_revision_replaced(cx: &mut VisualTestAppContext) {
    exercise(cx, Drift::ModalRevisionReplaced);
}

pub fn scenario_conflict_continue_approval_unrelated_replan(cx: &mut VisualTestAppContext) {
    exercise(cx, Drift::UnrelatedReplan);
}

/// #1736: a resolved mid-rebase is not completion, and Continue offers no abort command.
pub fn scenario_conflict_continue_presentation(cx: &mut VisualTestAppContext) {
    use kagi_ui_core::i18n::{self, Lang};
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        for more_steps in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let repo = directory.path().canonicalize().unwrap();
            fixture_git::init_repo(&repo, "main");
            fixture_git::write_file(&repo, FILE, "base\n");
            fixture_git::commit_all(&repo, "base");
            fixture_git::git(&repo, &["checkout", "-qb", "side"]);
            fixture_git::write_file(&repo, FILE, "side\n");
            fixture_git::commit_all(&repo, "first");
            if more_steps {
                fixture_git::write_file(&repo, "next.txt", "next\n");
                fixture_git::commit_all(&repo, "next");
            }
            fixture_git::git(&repo, &["checkout", "-q", "main"]);
            fixture_git::write_file(&repo, FILE, "main\n");
            fixture_git::commit_all(&repo, "main");
            fixture_git::git(&repo, &["checkout", "-q", "side"]);
            assert!(!fixture_git::git_succeeds(&repo, &["rebase", "main"]));
            stage(&repo, FILE, FIRST);
            let (app, window) = mount(cx, &repo);
            detect(cx, &app, window);
            assert!(cx.read(|cx| conflict_read(&app, cx)
                .mode
                .as_ref()
                .unwrap()
                .session
                .files
                .is_empty()));
            let before = NoWrite::read(&repo);
            open_continue(cx, &app, window);
            paint(cx, &app, window);
            cx.read(|cx| {
                let plan = &app.read(cx).conflict_continue_modal().unwrap().plan;
                assert!(plan.recovery.is_none());
                assert!(plan.equivalent_command.is_none());
                let warning = plan.warnings.first().unwrap();
                let text = i18n::plan_note_text(warning);
                let expected = match (lang, more_steps) {
                    (Lang::En, true) => "Continues to the next step (1 remaining)",
                    (Lang::Ja, true) => "次の step に進みます（残り 1）",
                    (Lang::En, false) => {
                        "No conflicting files detected; continue will finish the operation as-is."
                    }
                    (Lang::Ja, false) => {
                        "conflict ファイルはありません。続行すると操作はそのまま完了します。"
                    }
                };
                assert_eq!(text, expected);
            });
            click_control(cx, window, "plan-card-copy");
            let copied = cx.read(|cx| cx.read_from_clipboard().unwrap().text().unwrap());
            assert!(!copied.contains("git rebase --abort"), "{copied}");
            assert!(!copied.contains("git rebase --continue"), "{copied}");
            assert_eq!(
                before,
                NoWrite::read(&repo),
                "preview and copy do not continue"
            );
            click_control(cx, window, "plan-cancel");
            unmount(cx, app, window);
        }
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().canonicalize().unwrap();
        fixture_git::init_repo(&repo, "main");
        fixture_git::write_file(&repo, FILE, "base\n");
        fixture_git::commit_all(&repo, "base");
        fixture_git::write_file(&repo, FILE, "first\n");
        fixture_git::commit_all(&repo, "first");
        fixture_git::write_file(&repo, FILE, "second\n");
        fixture_git::commit_all(&repo, "second");
        assert!(!fixture_git::git_succeeds(
            &repo,
            &["revert", "--no-edit", "HEAD~1"]
        ));
        stage(&repo, FILE, "reverted\n");
        let (app, window) = mount(cx, &repo);
        detect(cx, &app, window);
        open_continue(cx, &app, window);
        paint(cx, &app, window);
        cx.read(|cx| {
            let plan = &app.read(cx).conflict_continue_modal().unwrap().plan;
            assert!(plan.recovery.is_none());
            assert!(plan.equivalent_command.is_none());
        });
        click_control(cx, window, "plan-card-copy");
        let copied = cx.read(|cx| cx.read_from_clipboard().unwrap().text().unwrap());
        assert!(!copied.contains("git revert --abort"), "{copied}");
        assert!(!copied.contains("git revert --continue"), "{copied}");
        click_control(cx, window, "plan-cancel");
        unmount(cx, app, window);
    }
}
