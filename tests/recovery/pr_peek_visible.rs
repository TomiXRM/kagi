//! #1102: PR Peek must expose the existing read-only Compare consumer.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpui::{point, px, AnyWindowHandle, Entity, Modifiers, MouseButton, VisualTestAppContext};
use kagi::ui::diff_view::{CompareTarget, DiffRow, MainDiffSource};
use kagi::ui::{e2e, theme, KagiApp};
use kagi_git::CommitId;

use crate::evidence_support::pull_request;
use crate::macos::{build_fixture, mount, unmount};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_output};

#[derive(Debug, PartialEq, Eq)]
struct RepositoryState {
    head: String,
    index: String,
    refs: String,
    stash: String,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

fn working_files(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).expect("working files") {
        let entry = entry.expect("working file");
        let path = entry.path();
        if path == root.join(".git") {
            continue;
        }
        if entry.file_type().expect("file type").is_dir() {
            working_files(root, &path, files);
        } else {
            files.insert(
                path.strip_prefix(root)
                    .expect("relative working path")
                    .into(),
                std::fs::read(path).expect("working file bytes"),
            );
        }
    }
}

fn repository_state(repo: &Path) -> RepositoryState {
    let mut files = BTreeMap::new();
    working_files(repo, repo, &mut files);
    RepositoryState {
        head: git_output(repo, &["rev-parse", "HEAD"]),
        index: git_output(repo, &["ls-files", "--stage", "-z"]),
        refs: git_output(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]),
        stash: git_output(repo, &["stash", "list", "--format=%H %gd %gs"]),
        files,
    }
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("paint PR Peek consumer");
}

fn peek_from_row(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    control: &str,
) {
    let id = window.window_id();
    e2e::clear_control_bounds(id, "pr-mode-center-pane");
    e2e::clear_control_bounds(id, control);
    paint(cx, window);
    assert!(e2e::control_bounds(id, "pr-mode-center-pane").is_some());
    let row = e2e::control_bounds(id, control).expect("actual PR context row");
    // Open away from the viewport edges; the menu's first row is Peek.
    let anchor = point(row.left() + px(30.), row.center().y);
    cx.simulate_mouse_down(window, anchor, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(window, anchor, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    let menu_anchor = cx.read(|cx| {
        let (pr, pos) = app.read(cx).ui().pr_menu.as_ref().expect("PR context menu");
        assert_eq!(pr.number, 77);
        *pos
    });
    paint(cx, window);
    let peek = menu_anchor + point(theme::scaled_px(10.), theme::scaled_px(10.));
    cx.simulate_mouse_move(window, peek, None, Modifiers::none());
    cx.simulate_click(window, peek, Modifiers::none());
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).ui().pr_menu.is_none()));
}

fn peek_fixture() -> (tempfile::TempDir, PathBuf, CommitId, CommitId) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().expect("fixture path");
    let base = CommitId(git_output(&repo, &["rev-parse", "HEAD"]));
    git(&repo, &["checkout", "-qb", "peek-visible"]);
    std::fs::write(repo.join("peek.txt"), "PR peek visible\n").unwrap();
    git(&repo, &["add", "peek.txt"]);
    git(&repo, &["commit", "-qm", "PR peek file"]);
    let head = CommitId(git_output(&repo, &["rev-parse", "HEAD"]));
    git(&repo, &["checkout", "-q", "main"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", &base.0]);
    git(
        &repo,
        &["update-ref", "refs/remotes/origin/peek-visible", &head.0],
    );
    (fixture, repo, base, head)
}

pub fn scenario_pr_peek_visible_table(cx: &mut VisualTestAppContext) {
    let (_fixture, repo, base, head) = peek_fixture();
    let before = repository_state(&repo);
    let receipts_before = kagi_git::oplog::read_oplog_tail(500).len();
    let (app, window) = mount(cx, &repo);
    let mut pr = pull_request(77, "Peek visibility", "peek-visible");
    pr.head_sha = head.0.clone();
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(vec![pr])));
    app.update(cx, |app, cx| {
        app.refresh_github_prs(cx);
        app.show_pr_mode(cx);
    });
    cx.run_until_parked();
    peek_from_row(cx, &app, window, "pr-home-row-77");

    assert_eq!(repository_state(&repo), before, "Peek is a repository read");
    assert_compare_consumer(cx, &app, window, &base, &head);
    assert_eq!(
        repository_state(&repo),
        before,
        "Compare file click must not write"
    );
    assert_eq!(kagi_git::oplog::read_oplog_tail(500).len(), receipts_before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pr_peek_visible_table");
}

fn assert_compare_consumer(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    base: &CommitId,
    head: &CommitId,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        let view = app
            .ui()
            .compare_view
            .as_ref()
            .expect("Peek succeeded")
            .read(cx)
            .view();
        assert_eq!(&view.base, base);
        assert_eq!(view.target, CompareTarget::Commit(head.clone()));
        assert_eq!(view.files.len(), 1);
        assert_eq!(view.files[0].path, Path::new("peek.txt"));
    });
    e2e::clear_control_bounds(window.window_id(), "inspector-file-0");
    paint(cx, window);
    cx.advance_clock(std::time::Duration::from_millis(600));
    cx.run_until_parked();
    paint(cx, window);
    let file = e2e::control_bounds(window.window_id(), "inspector-file-0")
        .expect("successful PR Peek must visibly expose its Compare changed file");
    cx.simulate_click(window, file.center(), Modifiers::none());
    cx.run_until_parked();
    cx.read(|cx| {
        let pane = app
            .read(cx)
            .ui()
            .main_diff
            .as_ref()
            .expect("Compare file opens MainDiff");
        let diff = &pane.read(cx).view;
        assert_eq!(diff.title.as_ref(), "peek.txt");
        assert!(matches!(
            &diff.source,
            MainDiffSource::Compare { base: shown_base, target, file_index: 0 }
                if shown_base == base && target == &CompareTarget::Commit(head.clone())
        ));
        assert!(diff.rows.iter().any(|row| matches!(
            row,
            DiffRow::Line { kind: kagi_domain::diff::DiffLineKind::Added, text, .. }
                if text.as_ref() == "+PR peek visible"
        )));
    });
    let fit = cx.read(|cx| {
        app.read(cx)
            .ui()
            .main_diff
            .as_ref()
            .expect("MainDiff")
            .read(cx)
            .fit
            .clone()
    });
    for _ in 0..3 {
        cx.run_until_parked();
        paint(cx, window);
    }
    assert!(
        fit.control_bounds("main-diff-back").is_some(),
        "the existing MainDiff is rendered, not merely retained"
    );
}

fn open_pr_context(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, head: &CommitId) {
    let mut pr = pull_request(77, "Peek visibility", "peek-visible");
    pr.head_sha = head.0.clone();
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(vec![pr])));
    app.update(cx, |app, cx| {
        app.github_host_logins
            .insert(Some("github.com".into()), "alice".into());
        app.refresh_github_prs(cx);
        app.show_pr_mode(cx);
    });
    cx.run_until_parked();
}

pub fn scenario_pr_peek_visible_edges(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        kagi::ui::i18n::set_lang(lang);
        for zoom in [1., 1.667] {
            theme::set_zoom(zoom);
            for control in ["pr-home-row-77", "pr-mode-card-77"] {
                for panel_open in [false, true] {
                    let (_fixture, repo, base, head) = peek_fixture();
                    // A real bounded reload will see only this newer main tip;
                    // the PR head remains fetched but outside the loaded Graph.
                    let status = git_fixture::git_command(&repo)
                        .env("GIT_COMMITTER_DATE", "2030-01-01T00:00:00Z")
                        .args(["commit", "--allow-empty", "-qm", "newer main"])
                        .status()
                        .expect("newer main commit");
                    assert!(status.success());
                    if panel_open {
                        std::fs::write(repo.join("README.md"), "# fixture\nretained WIP\n")
                            .unwrap();
                    }
                    let before = repository_state(&repo);
                    let receipts_before = kagi_git::oplog::read_oplog_tail(500).len();
                    let (app, window) = mount(cx, &repo);
                    let retained_panel = if panel_open {
                        use gpui::Focusable;
                        cx.update_window(window, |_, window, cx| {
                            app.update(cx, |app, cx| app.open_commit_panel(window, cx));
                            window.draw(cx).clear();
                        })
                        .unwrap();
                        cx.run_until_parked();
                        let panel = cx.read(|cx| {
                            let state = app.read(cx);
                            assert!(state.ui().commit_panel_open);
                            assert!(state.ui().selected.is_none());
                            state.ui().commit_panel.clone().expect("real CommitPanel")
                        });
                        cx.update_window(window, |_, window, cx| {
                            let input = panel
                                .read(cx)
                                .title_input
                                .as_ref()
                                .expect("real draft input");
                            window.focus(&input.read(cx).focus_handle(cx), cx);
                            window.draw(cx).clear();
                        })
                        .unwrap();
                        cx.simulate_keystrokes(window, "p");
                        cx.run_until_parked();
                        let draft = cx.read(|cx| {
                            assert!(!panel.read(cx).state.unstaged.is_empty());
                            panel.read(cx).effective_commit_message(cx)
                        });
                        assert!(
                            !draft.is_empty(),
                            "native input creates a real nonempty draft"
                        );
                        Some((panel, draft))
                    } else {
                        None
                    };
                    app.update(cx, |app, cx| {
                        app.ui_mut().expect("active fixture owner").commit_limit = 1;
                        app.reload(cx);
                    });
                    cx.run_until_parked();
                    assert!(cx.read(|cx| app
                        .read(cx)
                        .view()
                        .rows
                        .iter()
                        .all(|row| row.id != head)));
                    assert!(cx.read(|cx| app.read(cx).ui().selected.is_none()));
                    app.update(cx, |app, _| app.inspector_visible = false);
                    open_pr_context(cx, &app, &head);
                    peek_from_row(cx, &app, window, control);
                    assert_compare_consumer(cx, &app, window, &base, &head);
                    if let Some((panel, draft)) = &retained_panel {
                        cx.read(|cx| {
                            let current = app
                                .read(cx)
                                .ui()
                                .commit_panel
                                .as_ref()
                                .expect("retained CommitPanel");
                            assert_eq!(
                                current.entity_id(),
                                panel.entity_id(),
                                "Peek retains the real panel entity"
                            );
                            assert_eq!(
                                current.read(cx).effective_commit_message(cx),
                                *draft,
                                "Peek preserves the actual draft"
                            );
                        });
                    }
                    assert_eq!(
                        repository_state(&repo),
                        before,
                        "{lang:?} {zoom} {control}: read-only Peek"
                    );
                    assert_eq!(kagi_git::oplog::read_oplog_tail(500).len(), receipts_before);
                    drop(retained_panel);
                    unmount(cx, app, window);
                }
            }
        }
    }
    eprintln!("[gui-e2e] PASS pr_peek_visible_edges");
}

fn dirty_editor(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> (Entity<kagi_ui_editor::EditorWorkspaceView>, String) {
    use gpui::Focusable;
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx.read(|cx| app.read(cx).ui().editor_workspace.clone().expect("editor"));
    editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        paint(cx, window);
        if cx.read(|cx| editor.read(cx).editor.is_some()) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "README editor did not load"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.update_window(window, |_, window, cx| {
        let input = editor.read(cx).editor.as_ref().expect("real editor input");
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    let text = cx.read(|cx| {
        assert!(editor.read(cx).any_dirty(), "real input change is dirty");
        editor
            .read(cx)
            .editor
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()
    });
    (editor, text)
}

pub fn scenario_pr_peek_dirty_guard(cx: &mut VisualTestAppContext) {
    use crate::recovery_operations::{press_enter, press_key};
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    theme::set_zoom(1.);
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        kagi::ui::i18n::set_lang(lang);
        for case in ["cancel", "approve", "tab", "superseded", "buffer", "editor"] {
            let (_fixture, repo, base, head) = peek_fixture();
            std::fs::write(repo.join("other.txt"), "other buffer\n").unwrap();
            let mut before = repository_state(&repo);
            let receipts_before = kagi_git::oplog::read_oplog_tail(500).len();
            let (app, window) = mount(cx, &repo);
            let (editor, edited) = dirty_editor(cx, &app, window);
            app.update(cx, |app, _| app.inspector_visible = false);
            open_pr_context(cx, &app, &head);
            let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
            let attachment = cx.read(|cx| app.read(cx).app_sessions.attachment(owner));
            peek_from_row(cx, &app, window, "pr-home-row-77");
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(state.editor_dirty_guard_modal().is_some());
                assert!(state.pr_mode().is_some(), "guard preserves PR context");
                assert!(
                    state.ui().compare_view.is_none(),
                    "unapproved Compare is not published"
                );
                assert!(!state.inspector_visible);
                assert_eq!(state.active_session(), Some(owner));
                assert_eq!(state.app_sessions.attachment(owner), attachment);
                assert_eq!(
                    editor
                        .read(cx)
                        .editor
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .value()
                        .to_string(),
                    edited
                );
            });
            match case {
                "cancel" => {
                    press_key(cx, &app, window, "escape");
                    cx.read(|cx| {
                        let state = app.read(cx);
                        assert!(state.editor_dirty_guard_modal().is_none());
                        assert!(state.pr_mode().is_some());
                        assert_eq!(
                            state.ui().editor_workspace.as_ref().unwrap().entity_id(),
                            editor.entity_id()
                        );
                        assert!(editor.read(cx).any_dirty());
                        assert_eq!(
                            editor
                                .read(cx)
                                .editor
                                .as_ref()
                                .unwrap()
                                .read(cx)
                                .value()
                                .to_string(),
                            edited
                        );
                        assert!(state.ui().compare_view.is_none());
                    });
                }
                "approve" => {
                    // An external ref update and real reload must not change
                    // the Compare frozen by the original menu click.
                    git(
                        &repo,
                        &["update-ref", "refs/remotes/origin/peek-visible", &base.0],
                    );
                    app.update(cx, |app, cx| app.reload(cx));
                    cx.run_until_parked();
                    assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_some()));
                    assert_eq!(
                        cx.read(|cx| {
                            app.read(cx)
                                .view()
                                .remote_branches
                                .iter()
                                .find(|branch| branch.name == "peek-visible")
                                .unwrap()
                                .target
                                .clone()
                        }),
                        base
                    );
                    before = repository_state(&repo);
                    press_enter(cx, &app, window);
                    cx.run_until_parked();
                    assert_compare_consumer(cx, &app, window, &base, &head);
                    let shown = cx.read(|cx| {
                        let state = app.read(cx);
                        assert!(state.editor_dirty_guard_modal().is_none());
                        assert!(state.ui().editor_workspace.is_none());
                        assert_eq!(state.active_session(), Some(owner));
                        assert_eq!(state.app_sessions.attachment(owner), attachment);
                        let pane = state.ui().compare_view.as_ref().unwrap();
                        (
                            pane.entity_id(),
                            pane.read(cx).view().files.as_ptr(),
                            state.ui().main_diff.as_ref().unwrap().entity_id(),
                        )
                    });
                    app.update(cx, |app, cx| app.confirm_editor_dirty_guard(cx));
                    assert_eq!(
                        cx.read(|cx| {
                            let state = app.read(cx);
                            let pane = state.ui().compare_view.as_ref().unwrap();
                            (
                                pane.entity_id(),
                                pane.read(cx).view().files.as_ptr(),
                                state.ui().main_diff.as_ref().unwrap().entity_id(),
                            )
                        }),
                        shown,
                        "the original Peek is consumed once, without reinvocation"
                    );
                }
                "tab" => {
                    let other = build_fixture();
                    let other_before = repository_state(other.path());
                    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(Vec::new())));
                    app.update(cx, |app, cx| {
                        assert!(app.open_repository(other.path().to_path_buf(), cx));
                    });
                    cx.run_until_parked();
                    let active = cx.read(|cx| app.read(cx).active_session().unwrap());
                    assert_ne!(active, owner);
                    app.update(cx, |app, cx| app.confirm_editor_dirty_guard(cx));
                    cx.read(|cx| {
                        let state = app.read(cx);
                        assert_eq!(state.active_session(), Some(active));
                        assert!(state.ui().compare_view.is_none());
                        assert!(state.ui[&owner].compare_view.is_none());
                        assert_eq!(
                            state.ui[&owner]
                                .editor_workspace
                                .as_ref()
                                .unwrap()
                                .entity_id(),
                            editor.entity_id()
                        );
                        assert!(editor.read(cx).any_dirty());
                    });
                    assert_eq!(repository_state(other.path()), other_before);
                }
                "superseded" => {
                    let newer = pull_request(88, "Unfetched replacement", "not-fetched");
                    app.update(cx, |app, cx| app.open_pr_peek(&newer, cx));
                    app.update(cx, |app, cx| app.confirm_editor_dirty_guard(cx));
                    cx.read(|cx| {
                        let state = app.read(cx);
                        assert!(state.pr_mode().is_some());
                        assert!(state.editor_dirty_guard_modal().is_none());
                        assert!(state.ui().compare_view.is_none());
                        assert!(editor.read(cx).any_dirty());
                    });
                }
                "buffer" => {
                    editor.update(cx, |view, cx| view.open_tab("other.txt".into(), cx));
                    cx.run_until_parked();
                    app.update(cx, |app, cx| app.confirm_editor_dirty_guard(cx));
                    cx.read(|cx| {
                        let state = app.read(cx);
                        assert!(state.pr_mode().is_some());
                        assert_eq!(
                            state.ui().editor_workspace.as_ref().unwrap().entity_id(),
                            editor.entity_id()
                        );
                        assert_eq!(
                            editor.read(cx).open_path.as_deref(),
                            Some(Path::new("other.txt"))
                        );
                        assert!(
                            editor.read(cx).any_dirty(),
                            "older edited tab remains retained"
                        );
                        assert!(state.ui().compare_view.is_none());
                    });
                }
                "editor" => {
                    app.update(cx, |app, cx| {
                        app.close_editor_workspace();
                        app.open_editor_workspace(cx);
                        app.show_pr_mode(cx);
                    });
                    let replacement = cx.read(|cx| {
                        app.read(cx)
                            .ui()
                            .editor_workspace
                            .as_ref()
                            .unwrap()
                            .entity_id()
                    });
                    app.update(cx, |app, cx| app.confirm_editor_dirty_guard(cx));
                    cx.read(|cx| {
                        let state = app.read(cx);
                        assert!(state.pr_mode().is_some());
                        assert_eq!(
                            state.ui().editor_workspace.as_ref().unwrap().entity_id(),
                            replacement
                        );
                        assert!(state.ui().compare_view.is_none());
                    });
                }
                _ => unreachable!(),
            }
            assert_eq!(
                repository_state(&repo),
                before,
                "{lang:?} {case}: guard does not write"
            );
            assert_eq!(kagi_git::oplog::read_oplog_tail(500).len(), receipts_before);
            drop(editor);
            unmount(cx, app, window);
        }
    }
    eprintln!("[gui-e2e] PASS pr_peek_dirty_guard");
}

pub fn scenario_pr_peek_read_failure_context(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    theme::set_zoom(1.);
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        kagi::ui::i18n::set_lang(lang);
        for missing in [true, false] {
            let (_fixture, repo, _base, head) = peek_fixture();
            let before = repository_state(&repo);
            let receipts_before = kagi_git::oplog::read_oplog_tail(500).len();
            let (app, window) = mount(cx, &repo);
            let (editor, edited) = dirty_editor(cx, &app, window);
            if missing {
                let pr = pull_request(77, "Unfetched Peek", "not-fetched");
                e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(vec![pr])));
                app.update(cx, |app, cx| {
                    app.refresh_github_prs(cx);
                    app.show_pr_mode(cx);
                });
                cx.run_until_parked();
            } else {
                open_pr_context(cx, &app, &head);
                // Remove real object access only for the consumer read. No mocked
                // Compare result or renderer stands in for this backend failure.
                std::fs::rename(repo.join(".git/objects"), repo.join(".git/objects-held")).unwrap();
            }
            app.update(cx, |app, _| app.inspector_visible = false);
            peek_from_row(cx, &app, window, "pr-home-row-77");
            if !missing {
                std::fs::rename(repo.join(".git/objects-held"), repo.join(".git/objects")).unwrap();
                assert!(cx.read(|cx| matches!(
                    app.read(cx).status_footer,
                    kagi::ui::FooterStatus::Failed(_)
                )));
            }
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(state.pr_mode().is_some());
                assert!(!state.inspector_visible);
                assert!(state.ui().compare_view.is_none());
                assert!(state.ui().main_diff.is_none());
                assert!(state.editor_dirty_guard_modal().is_none());
                assert_eq!(
                    state.ui().editor_workspace.as_ref().unwrap().entity_id(),
                    editor.entity_id()
                );
                assert!(editor.read(cx).any_dirty());
                assert_eq!(
                    editor
                        .read(cx)
                        .editor
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .value()
                        .to_string(),
                    edited
                );
            });
            e2e::clear_control_bounds(window.window_id(), "pr-mode-center-pane");
            paint(cx, window);
            assert!(e2e::control_bounds(window.window_id(), "pr-mode-center-pane").is_some());
            assert_eq!(
                repository_state(&repo),
                before,
                "failed Peek preserves repository"
            );
            assert_eq!(kagi_git::oplog::read_oplog_tail(500).len(), receipts_before);
            drop(editor);
            unmount(cx, app, window);
        }
    }
    eprintln!("[gui-e2e] PASS pr_peek_read_failure_context");
}
