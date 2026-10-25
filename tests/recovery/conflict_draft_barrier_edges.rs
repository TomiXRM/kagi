//! #1760: bounded OS shutdown and explicit retries after real storage repair.
use crate::app_conflict::{click_control, content_fixture, wait_idle};
use crate::conflict_draft_persist_failure::{
    queue_result, IsolatedLog, UnwritableDirectory, DRAFT,
};
use crate::conflict_draft_revision::{conflict_read, detect, replace_result, result_input, FILE};
use crate::host_dirty_guard::cleanup;
use crate::macos::mount;
use gpui::VisualTestAppContext;
use kagi::ui::i18n::Lang;
use kagi_git::resolution::ResolutionBuffer;
use std::path::{Path, PathBuf};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

pub fn scenario_conflict_draft_shutdown(cx: &mut VisualTestAppContext) {
    let _isolated = IsolatedLog::new();
    let fixtures = [content_fixture(), content_fixture()];
    let panes = fixtures.each_ref().map(|fixture| {
        let repo = fixture.path().canonicalize().unwrap();
        let (app, window) = mount(cx, &repo);
        detect(cx, &app, window);
        let input = result_input(cx, &app, window);
        (app, window, input, repo)
    });
    for (app, window, input, _) in &panes {
        queue_result(cx, *window, input, DRAFT);
        assert!(cx.read(|cx| conflict_read(app, cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .has_unflushed_drafts()));
    }
    // Do not park the executor: shutdown must urgently flush both live panes,
    // not rely on their ordinary 250 ms coalescing timers or surviving windows.
    cx.update(|cx| cx.shutdown());
    for (app, window, input, repo) in panes {
        let restored = ResolutionBuffer::load(&repo).expect("shutdown flushed this live pane");
        assert_eq!(
            restored.resolved_text(Path::new(FILE)).as_deref(),
            Some(DRAFT)
        );
        drop(input);
        cleanup(cx, app, window);
    }
}

pub fn scenario_conflict_draft_repaired_actions(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for lang in [Lang::En, Lang::Ja] {
        for save in [true, false] {
            let _isolated = IsolatedLog::new();
            let fixture = content_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| app.set_lang(lang, cx));
            detect(cx, &app, window);
            let input = result_input(cx, &app, window);
            let denied = UnwritableDirectory::new(
                PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap()).join("conflicts"),
            );
            replace_result(cx, &app, window, &input, DRAFT);
            assert!(cx.read(|cx| conflict_read(&app, cx).draft_persist_failed.is_some()));
            denied.restore();
            if save {
                click_control(cx, window, "conflict-save");
            } else {
                click_control(cx, window, "conflict-abort");
            }
            crate::busy_dismiss::wait_ui(
                cx,
                &app,
                "explicit action retries repaired storage",
                |state, cx| {
                    let view = conflict_read(&app, cx);
                    view.draft_persist_failed.is_none()
                        && if save {
                            std::fs::read_to_string(repo.join(FILE)).ok().as_deref() == Some(DRAFT)
                        } else {
                            state.conflict_abort_modal().is_some()
                        }
                },
            );
            if save {
                wait_idle(cx, &app);
            } else {
                assert!(
                    repo.join(".git/MERGE_HEAD").exists(),
                    "opening Abort never executes it"
                );
            }
            if save {
                assert_eq!(
                    git_fixture::git_output(&repo, &["show", &format!(":{FILE}")]),
                    DRAFT.trim_end(),
                    "Save durably stages the recovered current body",
                );
            } else {
                assert_eq!(
                    ResolutionBuffer::load(&repo)
                        .unwrap()
                        .resolved_text(Path::new(FILE))
                        .as_deref(),
                    Some(DRAFT),
                    "Abort approval follows successful current-generation persistence",
                );
            }
            drop(input);
            cleanup(cx, app, window);
        }
    }
}
