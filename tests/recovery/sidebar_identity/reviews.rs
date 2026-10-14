//! P2 review regressions: Solo is a filter, and keyboard reveals follow focus.
use gpui::{Entity, Modifiers, MouseButton, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp, ToastKind};
use kagi_ui_core::i18n::Lang;

use super::support::*;
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, unmount};

pub fn scenario_tag_unloaded_target_solo(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    for language in [Lang::En, Lang::Ja] {
        let fixture = build_fixture();
        let repo = fixture.path();
        git(repo, &["checkout", "-qb", "other", "HEAD~1"]);
        git(repo, &["commit", "--allow-empty", "-qm", "other tip"]);
        let other = kagi_git::CommitId(output(repo, &["rev-parse", "HEAD"]));
        git(repo, &["checkout", "-q", "main"]);
        git(repo, &["tag", "solo-hidden-tag"]);
        let target = kagi_git::CommitId(output(repo, &["rev-parse", "HEAD"]));
        let (app, window) = mount_dimensions(cx, repo, (1000., 720.));
        appearance(cx, &app, window, language, "apple-light", 1., 3);
        app.update(cx, |app, cx| {
            app.toggle_branch_solo("other".into(), other.clone(), cx)
        });
        paint(cx, &app, window);
        app.read_with(cx, |app, _| {
            assert!(!app.view().commit_row_index.contains_key(&target));
            assert!(app
                .view()
                .branch_solo
                .as_ref()
                .unwrap()
                .saved_row_index
                .contains_key(&target));
        });
        let generation = app.read_with(cx, |app, _| app.ui().load_more_gen);
        let before = Witness::read(repo);
        for keyboard in [false, true] {
            if keyboard {
                focus(cx, &app, window, 3, "tag:solo-hidden-tag");
                keys(cx, window, "enter");
            } else {
                let at = bounds(window, "sidebar-tag-solo-hidden-tag").center();
                cx.simulate_mouse_down(window, at, MouseButton::Left, Modifiers::none());
                cx.simulate_mouse_up(window, at, MouseButton::Left, Modifiers::none());
            }
            cx.run_until_parked();
            paint(cx, &app, window);
            assert_solo_tag_feedback(cx, &app, &other, generation, language);
            assert_eq!(Witness::read(repo), before, "Solo navigation never writes");
        }
        unmount(cx, app, window);
        tag_no_more_cell(cx, language);
    }
}

fn assert_solo_tag_feedback(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    other: &kagi_git::CommitId,
    generation: u64,
    language: Lang,
) {
    app.read_with(cx, |app, cx| {
        let solo = app
            .view()
            .branch_solo
            .as_ref()
            .expect("tag activation preserves Branch Solo");
        assert_eq!(&solo.target, other);
        assert_eq!(
            app.ui().load_more_gen,
            generation,
            "hidden loaded target never pages"
        );
        let toast = app
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .unwrap();
        assert_eq!(toast.kind, ToastKind::Info);
        let message = match language {
            Lang::En => "Commit is hidden by Branch Solo.",
            Lang::Ja => "commit は Branch Solo により非表示です。",
        };
        assert_eq!(toast.message.as_ref(), message);
        assert_eq!(
            e2e::sidebar_labels::recorded_description("sidebar-tag-solo-hidden-tag"),
            None
        );
    });
}

fn tag_no_more_cell(cx: &mut VisualTestAppContext, language: Lang) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["tag", "no-more-tag", "HEAD~1"]);
    git(repo, &["commit", "--allow-empty", "-qm", "latest"]);
    let (app, window) = mount_dimensions(cx, repo, (1000., 720.));
    appearance(cx, &app, window, language, "apple-light", 1., 3);
    let snapshot = kagi_git::Backend::open(repo).unwrap().snapshot(2).unwrap();
    super::tag::install_history_page(cx, &app, repo, &snapshot);
    paint(cx, &app, window);
    let target = kagi_git::CommitId(output(repo, &["rev-parse", "no-more-tag"]));
    let generation = app.read_with(cx, |app, _| {
        assert!(!app.view().commit_row_index.contains_key(&target));
        app.ui().load_more_gen
    });
    let before = Witness::read(repo);
    for limit in [0, 1000] {
        app.update(cx, |app, _| app.ui_mut().unwrap().commit_limit = limit);
        focus(cx, &app, window, 3, "tag:no-more-tag");
        keys(cx, window, "enter");
        paint(cx, &app, window);
        app.read_with(cx, |app, _| {
            assert_eq!(app.ui().commit_limit, limit);
            assert_eq!(
                app.ui().load_more_gen,
                generation,
                "no page when history cannot load more"
            );
        });
        assert_eq!(Witness::read(repo), before);
    }
    unmount(cx, app, window);
}

pub fn scenario_sidebar_group_reveal_lifetime(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "feat/a"]);
    git(fixture.path(), &["branch", "feat/b"]);
    let (app, window) = mount_dimensions(cx, fixture.path(), (940., 660.));
    appearance(cx, &app, window, Lang::En, "apple-light", 1., 0);
    focus(cx, &app, window, 0, "group:local:feat");
    keys(cx, window, "shift-f10");
    paint(cx, &app, window);
    let card = "sidebar-group-local:feat-reveal";
    bounds(window, card);
    keys(cx, window, "down");
    paint(cx, &app, window);
    keys(cx, window, "up");
    e2e::clear_control_bounds(window.window_id(), card);
    paint(cx, &app, window);
    assert_eq!(
        focused(cx, &app, window),
        Some((0, "group:local:feat".into()))
    );
    assert!(
        e2e::control_bounds(window.window_id(), card).is_none(),
        "focus departure retires reveal instead of resurrecting it"
    );
    keys(cx, window, "shift-f10");
    paint(cx, &app, window);
    bounds(window, card);
    for _ in 0..2 {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("view.toggleSidebar", window, cx)
            });
            window.draw(cx).clear();
        })
        .unwrap();
    }
    focus(cx, &app, window, 0, "group:local:feat");
    e2e::clear_control_bounds(window.window_id(), card);
    paint(cx, &app, window);
    assert!(
        e2e::control_bounds(window.window_id(), card).is_none(),
        "yielding sidebar focus retires reveal"
    );
    unmount(cx, app, window);
}
