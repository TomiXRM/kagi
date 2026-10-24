//! First accepted reads must detect conflicts after superseding speculative startup I/O.
use crate::app_conflict::content_fixture;
use crate::macos::{build_fixture, mount, mount_state, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::KagiApp;
use kagi_ui_core::i18n::Lang;
use std::path::Path;

pub fn scenario_conflict_open_fresh(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [Lang::En, Lang::Ja] {
        let fixture = content_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let (app, window) = mount_state(cx, KagiApp::with_error(""));
        assert!(cx.read(|cx| app.read(cx).tabs.is_empty()));
        app.update(cx, |app, cx| {
            app.set_lang(language, cx);
            // Use the real native-picker completion and its production Open path,
            // not a synthetic read/detector result or a corrective manual Reload.
            app.deliver_repository_picker_for_e2e(Ok(Ok(Some(vec![repo.clone()]))), cx);
        });
        checked_first_read(cx, &app, window, &repo, 1);
        unmount(cx, app, window);
    }
}

pub fn scenario_conflict_open_second_tab(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [Lang::En, Lang::Ja] {
        let clean = build_fixture();
        let fixture = content_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let (app, window) = mount(cx, clean.path());
        let first = cx.read(|cx| app.read(cx).active_session().unwrap());
        app.update(cx, |app, cx| {
            app.set_lang(language, cx);
            app.deliver_repository_picker_for_e2e(Ok(Ok(Some(vec![repo.clone()]))), cx);
        });
        checked_first_read(cx, &app, window, &repo, 2);
        assert_ne!(cx.read(|cx| app.read(cx).active_session().unwrap()), first);
        unmount(cx, app, window);
    }
}

pub fn scenario_conflict_launch(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [Lang::En, Lang::Ja] {
        let fixture = content_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        // This is the production CLI-launch construction, not enter_tab/Open.
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| app.set_lang(language, cx));
        checked_first_read(cx, &app, window, &repo, 1);
        unmount(cx, app, window);
    }
}

fn checked_first_read(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    tabs: usize,
) {
    crate::busy_dismiss::wait(
        cx,
        app,
        "first accepted conflict read opens workspace",
        |state| state.ui().conflict.is_some() && !state.ui().panes_revalidating(),
    );
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.read(|cx| {
        let state = app.read(cx);
        let owner = state.active_session().unwrap();
        assert_eq!(state.repo_path.as_deref(), Some(repo));
        assert_eq!(state.tabs.len(), tabs);
        assert!(!state.reads.is_loading(owner));
        assert!(state.ui().conflict_detected);
        assert!(!state.ui().conflict_restore_failed);
        let accepted = state.view().operation.as_ref().unwrap().revision();
        let view = state.ui().conflict.as_ref().unwrap().read(cx);
        let mode = view.mode.as_ref().unwrap();
        assert_eq!(
            &mode.revision, accepted,
            "only this accepted read can admit the pane"
        );
        assert!(!mode.session.files.is_empty());
        assert!(
            view.editor_inputs.is_some(),
            "first Result must render without Reload"
        );
        assert!(git2::Repository::open(repo)
            .unwrap()
            .index()
            .unwrap()
            .has_conflicts());
    });
}
