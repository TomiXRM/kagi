//! #842 (Refs #357): Stage / Unstage hunk on the Commit Panel diff.
//!
//! A committed 20-line file edited at lines 2 and 18 shows two hunks in its
//! unstaged diff. Clicking the first hunk header's measured "Stage hunk"
//! puts exactly that edit in the index: the panel lists the file on both
//! sides and the re-read diff shows the one hunk left. The staged diff's
//! "Unstage hunk" takes it back out and the pane closes, the staged side being
//! empty. Split view draws the same button on the full-width header row. A
//! held `index.lock` fails through #490's staging failure delivery.

use std::path::Path;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::commit_panel::CommitPanelFileRef;
use kagi::ui::diff_view::DiffRow;
use kagi::ui::{e2e, theme, FooterStatus, KagiApp, WipDiffStat};

use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_operations::{press_enter, wait_idle};

fn text(edit: impl Fn(usize) -> Option<&'static str>) -> String {
    (1..=20)
        .map(|n| match edit(n) {
            Some(t) => format!("{t}\n"),
            None => format!("line {n}\n"),
        })
        .collect()
}

fn index_text(repo: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(["show", ":f.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

/// Row indices of the open main diff's hunk headers.
fn headers(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Vec<usize> {
    cx.read(|cx| {
        let Some(pane) = app.read(cx).ui().main_diff.clone() else {
            return Vec::new();
        };
        let rows = pane.read(cx).view.rows.clone();
        rows.iter()
            .enumerate()
            .filter(|(_, r)| matches!(r, DiffRow::HunkHeader(_)))
            .map(|(i, _)| i)
            .collect()
    })
}

fn panel_counts(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> (usize, usize) {
    cx.read(|cx| {
        let panel = app.read(cx).ui().commit_panel.clone().expect("panel");
        let state = &panel.read(cx).state;
        (state.unstaged.len(), state.staged.len())
    })
}

fn open(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, file: CommitPanelFileRef) {
    app.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    cx.run_until_parked();
}

/// Click the hunk button on header row `row`.
fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, row: usize) {
    let id = format!("main-diff-hunk-stage-{row}");
    e2e::clear_control_bounds(window.window_id(), &id);
    for _ in 0..2 {
        draw(cx, window);
    }
    let bounds = e2e::control_bounds(window.window_id(), &id)
        .unwrap_or_else(|| panic!("{id} was not laid out"));
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

pub fn scenario_hunk_staging(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split", "lang"]);
    let language = kagi::ui::i18n::lang();
    let split_before = theme::diff_split();
    theme::set_diff_split(false);
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().canonicalize().unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("f.txt"), text(|_| None)).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let both = text(|n| match n {
        2 => Some("TWO"),
        18 => Some("EIGHTEEN"),
        _ => None,
    });
    let first_only = text(|n| (n == 2).then_some("TWO"));
    std::fs::write(repo.join("f.txt"), &both).unwrap();

    let (app, window) = mount(cx, &repo);
    let panel_repo = repo.clone();
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, panel_repo, cx)
    });
    cx.run_until_parked();
    assert_eq!(panel_counts(cx, &app), (1, 0));

    // Stage the first of the two unstaged hunks.
    open(cx, &app, CommitPanelFileRef::Unstaged { index: 0 });
    let shown = headers(cx, &app);
    assert_eq!(shown.len(), 2, "two edits, two hunks");
    click(cx, window, shown[0]);
    wait_idle(cx, &app);
    assert_eq!(
        index_text(&repo),
        first_only,
        "exactly the first hunk staged"
    );
    assert_eq!(
        panel_counts(cx, &app),
        (1, 1),
        "the file is listed as unstaged and staged"
    );
    assert_eq!(
        headers(cx, &app).len(),
        1,
        "the re-read unstaged diff shows the hunk left"
    );
    assert_eq!(std::fs::read_to_string(repo.join("f.txt")).unwrap(), both);

    // Unstage it from the staged diff, in split view: the pane closes, the
    // staged side being empty again.
    theme::set_diff_split(true);
    open(cx, &app, CommitPanelFileRef::Staged { index: 0 });
    let staged = headers(cx, &app);
    assert_eq!(staged.len(), 1);
    click(cx, window, staged[0]);
    wait_idle(cx, &app);
    assert_eq!(index_text(&repo), text(|_| None), "the index is HEAD again");
    assert_eq!(panel_counts(cx, &app), (1, 0));
    assert!(
        cx.read(|cx| app.read(cx).ui().main_diff.is_none()),
        "the empty staged side closes the pane"
    );
    theme::set_diff_split(false);

    // #490: preserve localized failure delivery, including JA, under a held
    // index.lock. The same English durable detail appears in both wrappers.
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        kagi::ui::i18n::set_lang(lang);
        open(cx, &app, CommitPanelFileRef::Unstaged { index: 0 });
        let shown = headers(cx, &app);
        let lock = repo.join(".git/index.lock");
        std::fs::write(&lock, "fixture lock").unwrap();
        click(cx, window, shown[0]);
        wait_idle(cx, &app);
        std::fs::remove_file(&lock).unwrap();
        let entries = kagi_git::read_oplog_tail_for_repo(&repo, 1);
        let kagi_git::OpOutcome::Failed { error } = &entries[0].outcome else {
            panic!("index.lock failure receipt");
        };
        let expected = kagi::ui::i18n::op_failed(kagi::ui::i18n::Op::Stage, error);
        cx.read(|cx| {
            let state = app.read(cx);
            let FooterStatus::Failed(footer) = &state.status_footer else {
                panic!("missing staging failure footer")
            };
            assert!(footer.contains("lock"), "{footer}");
            assert!(footer.contains("f.txt"), "{footer}");
            assert_eq!(
                footer.as_ref(),
                expected,
                "{lang:?} failure must be localized"
            );
            let toast = state
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .last()
                .unwrap();
            assert!(matches!(toast.kind, kagi::ui::ToastKind::Error));
            assert!(toast.message.ends_with(&expected));
        });
        assert_eq!(
            index_text(&repo),
            text(|_| None),
            "the failure wrote nothing"
        );
    }
    kagi::ui::i18n::set_lang(language);

    theme::set_diff_split(split_before);
    if cx.read(|cx| e2e::app_notice_message(app.read(cx)).is_some()) {
        press_enter(cx, &app, window);
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS hunk_staging");
}

/// #1131: a real click consumes the content shown before an external edit,
/// not the new content now occupying the same header range.
pub fn scenario_hunk_staging_content_identity(cx: &mut VisualTestAppContext) {
    use kagi::ui::i18n::{self, Lang};
    use kagi_domain::plan_note::{CommonNote, PlanNote};
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "diff_split"]);
    let language = i18n::lang();
    let split = theme::diff_split();
    theme::set_diff_split(false);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        for staged in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let repo = temp.path().canonicalize().unwrap();
            git(&repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join("f.txt"), "one\ntwo\nthree\n").unwrap();
            git(&repo, &["add", "."]);
            git(&repo, &["commit", "-qm", "base"]);
            std::fs::write(repo.join("f.txt"), "one\nAPPROVED\nthree\n").unwrap();
            if staged {
                git(&repo, &["add", "--", "f.txt"]);
            }
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
            });
            cx.run_until_parked();
            open(
                cx,
                &app,
                if staged {
                    CommitPanelFileRef::Staged { index: 0 }
                } else {
                    CommitPanelFileRef::Unstaged { index: 0 }
                },
            );
            let shown = headers(cx, &app);
            assert_eq!(shown.len(), 1);
            draw(cx, window);
            let id = format!("main-diff-hunk-stage-{}", shown[0]);
            let bounds = e2e::control_bounds(window.window_id(), &id).expect("hunk button");
            let head = git_value(&repo, &["rev-parse", "HEAD"]);
            // The UI is already drawn. Edit externally, then click its existing
            // measured button without refreshing or planning a replacement.
            std::fs::write(repo.join("f.txt"), "one\nNOT_APPROVED\nthree\n").unwrap();
            if staged {
                git(&repo, &["add", "--", "f.txt"]);
            }
            let index = std::fs::read(repo.join(".git/index")).unwrap();
            let blob = git_value(&repo, &["rev-parse", ":f.txt"]);
            assert!(
                diff_has_text(cx, &app, "APPROVED"),
                "old diff still displayed"
            );
            cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
            cx.run_until_parked();
            wait_idle(cx, &app);
            let op = if staged { "unstage" } else { "stage" };
            let note = PlanNote::Common(CommonNote::HunkChanged {
                path: "f.txt".into(),
            });
            let expected = i18n::op_refused(op, i18n::plan_note_text(&note), 0);
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(matches!(&state.status_footer, FooterStatus::Failed(m) if m.as_ref() == expected),
                    "{lang:?} {op}: {:?}", state.status_footer);
                let toast = state.toast_stack.as_ref().unwrap().read(cx).toasts().last().unwrap();
                assert!(matches!(toast.kind, kagi::ui::ToastKind::Error));
                assert!(toast.message.ends_with(&expected), "{:?}", toast.message);
                assert!(e2e::app_notice_message(state).is_none());
            });
            assert_eq!(git_value(&repo, &["rev-parse", "HEAD"]), head);
            assert_eq!(
                git_value(&repo, &["rev-parse", ":f.txt"]),
                blob,
                "index blob unchanged"
            );
            assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
            assert_eq!(
                std::fs::read_to_string(repo.join("f.txt")).unwrap(),
                "one\nNOT_APPROVED\nthree\n"
            );
            let entries = kagi_git::read_oplog_tail_for_repo(&repo, 100);
            assert_eq!(entries.len(), 1, "exactly one refusal receipt");
            assert_eq!(entries[0].op, op);
            assert!(
                matches!(&entries[0].outcome, kagi_git::OpOutcome::Refused { blockers }
                if blockers == &vec![note.message_en()])
            );
            assert!(
                diff_has_text(cx, &app, "NOT_APPROVED"),
                "refusal refreshes diff"
            );
            unmount(cx, app, window);
        }
        typechange_has_no_hunk_button(cx);
    }
    i18n::set_lang(language);
    theme::set_diff_split(split);
    eprintln!("[gui-e2e] PASS hunk_staging_content_identity EN/JA stage/unstage");
}

fn typechange_has_no_hunk_button(cx: &mut VisualTestAppContext) {
    for staged in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().canonicalize().unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("f.txt"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "base"]);
        std::fs::remove_file(repo.join("f.txt")).unwrap();
        std::os::unix::fs::symlink("new-target", repo.join("f.txt")).unwrap();
        if staged {
            git(&repo, &["add", "--", "f.txt"]);
        }
        let index = std::fs::read(repo.join(".git/index")).unwrap();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
        });
        cx.run_until_parked();
        open(
            cx,
            &app,
            if staged {
                CommitPanelFileRef::Staged { index: 0 }
            } else {
                CommitPanelFileRef::Unstaged { index: 0 }
            },
        );
        let shown = headers(cx, &app);
        assert!(!shown.is_empty(), "typechange diff is displayed");
        draw(cx, window);
        for row in shown {
            assert!(
                e2e::control_bounds(window.window_id(), &format!("main-diff-hunk-stage-{row}"))
                    .is_none(),
                "a typechange half must not offer hunk staging"
            );
        }
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        assert!(kagi_git::read_oplog_tail_for_repo(&repo, 100).is_empty());
        unmount(cx, app, window);
    }
}

fn git_value(repo: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

fn diff_has_text(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, text: &str) -> bool {
    cx.read(|cx| {
        let pane = app
            .read(cx)
            .ui()
            .main_diff
            .as_ref()
            .expect("diff pane")
            .read(cx);
        pane.view
            .rows
            .iter()
            .any(|row| matches!(row, DiffRow::Line { text: line, .. } if line.strip_prefix('+') == Some(text)))
    })
}

/// A completed but held first diffstat read must not overwrite the badge from
/// a second stage in the same cache epoch. The panel and index update while
/// the first read is still held: staging never waits for the badge's two diffs.
pub fn scenario_wip_diffstat_stage_order(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("README.md"), "# fixture\nsecond line\nfirst\n").unwrap();
    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    cx.run_until_parked();
    let owner = cx.read(|cx| app.read(cx).active_session().expect("stage owner"));
    let epoch = cx.read(|cx| app.read(cx).ui().cache_epoch);
    let before = cx.read(|cx| app.read(cx).ui().wip_diffstat);
    assert_eq!(
        before,
        Some(WipDiffStat {
            additions: 1,
            deletions: 0
        }),
        "initial badge reflects the unstaged edit"
    );

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_wip_diffstat_scan_for_e2e(hold);
    app.update(cx, |app, cx| {
        app.do_stage_file_by_path(owner, "README.md".into(), cx);
        assert!(app.write_busy_op.is_none(), "stage did not return");
        assert_eq!(app.ui().wip_diffstat, before);
        let panel = app.ui().commit_panel.as_ref().expect("panel");
        assert_eq!(
            panel.read(cx).state.staged.len(),
            1,
            "panel updated before scan"
        );
    });
    cx.run_until_parked(); // First result is computed, but held before publishing.
    assert_eq!(
        index_text_readme(&repo),
        "# fixture\nsecond line\nfirst\n",
        "first stage completed while its scan is held"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().wip_diffstat),
        before,
        "held scan changed the badge"
    );

    std::fs::write(
        repo.join("README.md"),
        "# fixture\nsecond line\nfirst\nsecond\nthird\n",
    )
    .unwrap();
    app.update(cx, |app, cx| {
        app.do_stage_file_by_path(owner, "README.md".into(), cx);
        assert!(app.write_busy_op.is_none(), "second stage did not return");
        assert_eq!(
            app.ui().cache_epoch,
            epoch,
            "the ordering is within one epoch"
        );
    });
    cx.run_until_parked();
    let final_stat = WipDiffStat {
        additions: 3,
        deletions: 0,
    };
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().wip_diffstat),
        Some(final_stat)
    );
    assert_eq!(
        index_text_readme(&repo),
        "# fixture\nsecond line\nfirst\nsecond\nthird\n"
    );

    release.send(());
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().wip_diffstat),
        Some(final_stat),
        "the older request overwrote the second stage's badge"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS wip_diffstat_stage_order");
}

/// A watcher refresh with the same status classification still publishes a
/// newer badge than a held scan computed before the file changed.
pub fn scenario_wip_diffstat_watcher_order(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("README.md"), "# fixture\nfirst\n").unwrap();
    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();
    let old_stat = WipDiffStat {
        additions: 1,
        deletions: 1,
    };
    assert_eq!(cx.read(|cx| app.read(cx).ui().wip_diffstat), Some(old_stat));
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_wip_diffstat_scan_for_e2e(hold);
    app.update(cx, |app, cx| app.start_wip_diffstat_scan(cx));
    cx.run_until_parked(); // old diff is computed, then held before publishing.
    std::fs::write(repo.join("README.md"), "# fixture\nfirst\nsecond\n").unwrap();
    app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
    cx.run_until_parked();
    let refreshed = WipDiffStat {
        additions: 2,
        deletions: 1,
    };
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().wip_diffstat),
        Some(refreshed),
        "watcher must publish new badge before old scan completes"
    );
    release.send(());
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().wip_diffstat),
        Some(refreshed),
        "older scan overwrote the watcher's newer result"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS wip_diffstat_watcher_order");
}

fn index_text_readme(repo: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(["show", ":README.md"])
        .current_dir(repo)
        .output()
        .expect("git show");
    assert!(out.status.success());
    String::from_utf8(out.stdout).expect("index UTF-8")
}
