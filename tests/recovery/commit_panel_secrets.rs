//! #1702: detected staged secrets are visible before a write and never quiet.
use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::{contained, draw, GlobalSettings};
use gpui::VisualTestAppContext;
use kagi::ui::e2e;
use kagi_ui_core::{i18n, theme};

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    for dimensions in [(1392., 883.), (940., 660.)] {
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            for zoom in [1., 1.67] {
                let fixture = build_fixture();
                git(fixture.path(), &["config", "commit.template", "/dev/null"]);
                std::fs::write(fixture.path().join(".env"), "EXAMPLE=fixture\n").unwrap();
                std::fs::write(
                    fixture.path().join("token.txt"),
                    "-----BEGIN PRIVATE KEY-----\nfixture\n",
                )
                .unwrap();
                std::fs::write(fixture.path().join(".env.example"), "EXAMPLE=placeholder\n")
                    .unwrap();
                git(fixture.path(), &["add", "."]);
                // Detection follows the staged BLOB, not this harmless WT edit.
                std::fs::write(fixture.path().join("token.txt"), "harmless now\n").unwrap();
                let head = git(fixture.path(), &["rev-parse", "HEAD"]);
                let index = std::fs::read(fixture.path().join(".git/index")).unwrap();
                theme::set_zoom(zoom);
                i18n::set_lang(lang);
                let (app, window) =
                    crate::presentation_support::mount(cx, fixture.path(), dimensions);
                cx.update_window(window.into(), |_, window, cx| {
                    app.update(cx, |app, cx| app.open_commit_panel(window, cx));
                })
                .unwrap();
                for _ in 0..4 {
                    cx.run_until_parked();
                    draw(cx, window.into(), dimensions);
                }
                let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
                cx.update_window(window.into(), |_, window, cx| {
                    let title = panel.read(cx).title_input.clone().unwrap();
                    title.update(cx, |input, cx| {
                        input.set_value("Review staged files", window, cx)
                    });
                })
                .unwrap();
                for tree in [false, true] {
                    panel.update(cx, |panel, cx| {
                        panel.state.tree_view = tree;
                        cx.notify();
                    });
                    draw(cx, window.into(), dimensions);
                    cx.read(|cx| assert_secret_rows(&panel.read(cx).state, window.window_id()));
                    let review = e2e::control_bounds(window.window_id(), "cp-secret-review")
                        .expect("warning near Commit");
                    let commit =
                        e2e::control_bounds(window.window_id(), "cp-commit-action").unwrap();
                    let footer = e2e::control_bounds(window.window_id(), "cp-footer").unwrap();
                    contained(footer, review, "secret review");
                    contained(footer, commit, "Commit after warning");
                    assert!(
                        review.bottom() <= commit.top(),
                        "warning precedes the action"
                    );
                }
                let commit = e2e::control_bounds(window.window_id(), "cp-commit-action").unwrap();
                cx.simulate_click(window.into(), commit.center(), gpui::Modifiers::none());
                cx.run_until_parked();
                draw(cx, window.into(), dimensions);
                assert!(
                    cx.read(|cx| panel.read(cx).state.plan_modal.is_some()),
                    "secret warning requires explicit confirmation instead of quiet commit"
                );
                assert_eq!(git(fixture.path(), &["rev-parse", "HEAD"]), head);
                assert_eq!(
                    std::fs::read(fixture.path().join(".git/index")).unwrap(),
                    index
                );
                app.update(cx, |app, cx| app.cancel_commit_plan_modal(cx));
                // The existing Unstage exclusion path removes both inline warnings.
                app.update(cx, |app, cx| {
                    app.do_unstage_all(app.active_session().unwrap(), cx)
                });
                cx.run_until_parked();
                e2e::clear_control_bounds(window.window_id(), "cp-secret-review");
                draw(cx, window.into(), dimensions);
                assert!(cx.read(|cx| panel.read(cx).state.secret_paths.is_empty()));
                assert!(e2e::control_bounds(window.window_id(), "cp-secret-review").is_none());
                assert_eq!(git(fixture.path(), &["rev-parse", "HEAD"]), head);
                drop(panel);
                unmount(cx, app, window.into());
            }
        }
    }
    eprintln!("[gui-e2e] PASS commit_panel_secret_warnings");
}

fn assert_secret_rows(state: &kagi::ui::commit_panel::CommitPanelState, window: gpui::WindowId) {
    assert_eq!(state.secret_paths.len(), 2);
    for (fi, file) in state.staged.iter().enumerate() {
        let warning = e2e::control_bounds(window, &format!("cp-secret-file-{fi}"));
        if state.secret_paths.contains(&file.path) {
            let warning = warning.expect("secret warning painted on target file row");
            contained(
                e2e::control_bounds(window, "cp-files").unwrap(),
                warning,
                "secret row warning",
            );
        } else {
            assert!(warning.is_none(), "example file is not warned");
        }
    }
}
