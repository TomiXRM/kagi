//! Native dialog labels and both response failure layers use production completion.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{px, size, VisualTestAppContext};
use kagi::ui::{types::FooterStatus, KagiApp, ToastKind};
use kagi_ui_core::{
    i18n::{Lang, Msg},
    theme,
};

pub fn scenario_open_repository_picker_localized(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    for (width, height) in [(940., 660.), (1392., 883.)] {
        cx.update_window(window, |_, window, _| {
            window.resize(size(px(width), px(height)))
        })
        .unwrap();
        crate::macos::drain_native_events();
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            for (lang, expected) in [
                (Lang::En, "Open Repository"),
                (Lang::Ja, "リポジトリを開く"),
            ] {
                app.update(cx, |app, cx| app.set_lang(lang, cx));
                cx.update_window(window, |_, window, cx| window.draw(cx).clear())
                    .unwrap();
                let options = KagiApp::repository_picker_options_for_e2e();
                assert_eq!(
                    options.prompt.as_deref(),
                    Some(expected),
                    "native approve button must follow UI language"
                );
                assert!(!options.files && options.directories && !options.multiple);
            }
        }
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS open_repository_picker_localized");
}

pub fn scenario_open_repository_picker_error(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
        let before = kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100).len();
        let toasts = cx.read(|cx| stack.read(cx).toasts().len());
        let tabs = cx.read(|cx| app.read(cx).tabs.clone());
        app.update(cx, |app, cx| {
            app.deliver_repository_picker_for_e2e(Ok(Ok(None)), cx)
        });
        assert_eq!(
            cx.read(|cx| stack.read(cx).toasts().len()),
            toasts,
            "Cancel must stay silent"
        );
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100).len(),
            before
        );
        for channel in [false, true] {
            let detail = format!(
                "native picker {lang:?} channel={channel}: {}",
                "long diagnostic 日本語\n".repeat(80)
            );
            let response = if channel {
                Err(detail.clone())
            } else {
                Ok(Err(detail.clone()))
            };
            app.update(cx, |app, cx| {
                app.deliver_repository_picker_for_e2e(response, cx)
            });
            cx.run_until_parked();
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(
                    matches!(state.status_footer, FooterStatus::Failed(_)),
                    "picker Err must produce a visible failure, not Cancel"
                );
                assert_eq!(state.tabs, tabs, "failed picker must not alter tabs");
                let toast = stack.read(cx).toasts().last().unwrap();
                assert_eq!(toast.kind, ToastKind::Error);
                assert!(
                    toast.message.contains(Msg::RepositoryPickerOpen.t()),
                    "feedback names localized picker"
                );
                assert!(
                    toast.message.chars().count() < 1000 && toast.message.ends_with('…'),
                    "toast is a bounded preview"
                );
            });
            let entries = kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100);
            assert!(entries.iter().any(|entry| matches!(&entry.outcome, kagi_git::oplog::OpOutcome::Failed { error } if error == &detail)), "oplog must preserve complete diagnostic");
        }
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100).len(),
            before + 2,
            "each failure recorded exactly once"
        );
        // The next successful response remains usable; reselecting the open repo adds no tab.
        app.update(cx, |app, cx| {
            app.deliver_repository_picker_for_e2e(
                Ok(Ok(Some(vec![fixture.path().to_path_buf()]))),
                cx,
            )
        });
        cx.read(|cx| assert_eq!(app.read(cx).tabs, tabs));
        drop(stack);
    }
    unmount(cx, app, window);
    let (app, window) = crate::macos::mount_state(cx, KagiApp::with_error(""));
    let before = kagi_git::oplog::read_oplog_tail(100).len();
    for response in [
        Ok(Err("Home platform failure".to_string())),
        Err("Home channel failure".to_string()),
    ] {
        app.update(cx, |app, cx| {
            app.deliver_repository_picker_for_e2e(response, cx)
        });
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(state.tabs.is_empty());
            let stack = state.toast_stack.as_ref().unwrap().read(cx);
            assert_eq!(stack.toasts().last().unwrap().kind, ToastKind::Error);
        });
    }
    assert_eq!(
        kagi_git::oplog::read_oplog_tail(100).len(),
        before,
        "Home cannot record a fabricated repository"
    );
    app.update(cx, |app, cx| {
        app.deliver_repository_picker_for_e2e(Ok(Ok(Some(vec![fixture.path().to_path_buf()]))), cx)
    });
    cx.read(|cx| {
        assert_eq!(
            app.read(cx).tabs.len(),
            1,
            "valid picker still opens after Home failure"
        )
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS open_repository_picker_error");
}

pub fn scenario_open_repository_picker_home_owner(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    let before = kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100).len();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx));
    })
    .unwrap();
    let owner = cx.read(|cx| app.read(cx).repository_picker_owner_for_e2e());
    app.update(cx, |app, cx| {
        app.deliver_owned_repository_picker_for_e2e(
            owner,
            "Home foreground picker failure".into(),
            cx,
        )
    });
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100).len(),
        before,
        "Home foreground must not record against hidden repository"
    );
    unmount(cx, app, window);
}

pub fn scenario_open_repository_picker_frozen_owner(cx: &mut VisualTestAppContext) {
    let a = build_fixture();
    let b = build_fixture();
    let (app, window) = mount(cx, a.path());
    let before_a = kagi_git::oplog::read_oplog_tail_for_repo(a.path(), 100).len();
    let before_b = kagi_git::oplog::read_oplog_tail_for_repo(b.path(), 100).len();
    let owner = cx.read(|cx| app.read(cx).repository_picker_owner_for_e2e());
    app.update(cx, |app, cx| {
        assert!(app.open_repository(b.path().to_path_buf(), cx))
    });
    app.update(cx, |app, cx| {
        app.deliver_owned_repository_picker_for_e2e(
            owner,
            "A picker after switching to B".into(),
            cx,
        )
    });
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(a.path(), 100).len(),
        before_a + 1,
        "picker receipt must belong to launch repository"
    );
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(b.path(), 100).len(),
        before_b,
        "later tab must not receive another repository's failure"
    );
    unmount(cx, app, window);
}
