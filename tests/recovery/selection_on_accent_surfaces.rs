use super::*;

pub(super) fn pr_card_legs(cx: &mut VisualTestAppContext) {
    use crate::pr_viewed::{pr_at, push_pr_head, wait_loaded};
    use kagi_domain::github::{Check, CiState, ReviewState};
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote_dir = tempfile::tempdir().unwrap();
    let remote = remote_dir.path().join("repo.git");
    git(remote_dir.path(), &["init", "-q", "--bare", "repo.git"]);
    git(
        &repo,
        &[
            "push",
            "-q",
            remote.to_str().unwrap(),
            "main:refs/heads/main",
        ],
    );
    let url = "https://github.com/example/repo.git";
    git(&repo, &["remote", "add", "origin", url]);
    git(
        &repo,
        &[
            "config",
            &format!("url.file://{}.insteadOf", remote.display()),
            url,
        ],
    );
    std::fs::create_dir_all(repo.join("src")).unwrap();
    let head = push_pr_head(
        &repo,
        &remote,
        "main",
        &[
            ("README.md", "changed\nsecond line\n"),
            ("src/file.txt", "new file\n"),
        ],
    );
    let mut pr = pr_at(&head);
    pr.title = "Selected PR sidebar title".into();
    pr.ci = CiState::Failure;
    pr.review = ReviewState::ChangesRequested;
    pr.checks = vec![Check {
        name: "build".into(),
        workflow: "ci".into(),
        state: CiState::Failure,
        url: String::new(),
    }];
    let (app, window) = mount(cx, &repo);
    e2e::queue_github_pr_conversation(gpui::Task::ready((Ok((vec![], vec![])), Ok(vec![]))));
    app.update(cx, |app, cx| {
        app.github_host_logins
            .insert(Some("github.com".into()), "alice".into());
        app.ui_mut().unwrap().github_prs = vec![pr.clone()];
        app.pr_mode_open(&pr, cx);
    });
    wait_loaded(cx, &app, &head);
    // A detail read may refresh list evidence; use the same real PR fixture for its card.
    app.update(cx, |app, cx| {
        app.ui_mut().unwrap().github_prs = vec![pr.clone()];
        app.seed_pr_status_fresh_for_e2e(7, cx);
    });
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
    cx.run_until_parked();
    let other = non_apple_slug();
    for (slug, system) in [
        ("apple-light", true),
        ("apple-dark", true),
        ("apple-dark", false),
        (other.as_str(), true),
    ] {
        let _accent = theme::AccentInjection::new(theme::SystemAccent::Blue, Some(system));
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for active in [true, false] {
            app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
            move_to(cx, window, None);
            let palette = theme::theme();
            let on_accent = active && matches!(slug, "apple-light" | "apple-dark");
            let expected_fill = if active {
                if on_accent {
                    palette.ui_accent()
                } else {
                    palette.selected
                }
            } else {
                palette.inactive_selected
            };
            if on_accent {
                assert_eq!(
                    expected_fill,
                    if system {
                        theme::SystemAccent::Blue.rgb(slug == "apple-dark")
                    } else {
                        0xffd600
                    },
                    "literal system Blue or Theme yellow"
                );
            }
            assert_eq!(fill(cx, window, "pr-mode-card-7"), expected_fill);
            let ink = if on_accent {
                if system {
                    0xffffff
                } else {
                    0x000000
                }
            } else {
                palette.text_main
            };
            assert_paint(
                cx,
                window,
                "pr-mode-card-title-7",
                Hsla::from(gpui::rgb(ink)),
                ink,
            );
            let secondary = if on_accent {
                Hsla::from(gpui::rgb(ink)).alpha(0.7)
            } else {
                Hsla::from(gpui::rgb(palette.text_muted))
            };
            assert_paint(
                cx,
                window,
                "pr-mode-card-secondary-7",
                secondary,
                if on_accent {
                    composite(ink, 0.7, expected_fill)
                } else {
                    palette.text_muted
                },
            );
            let file = bounds(window, "pr-mode-file-0");
            assert_label_pixels(cx, window, file, ink);
            if !on_accent {
                // Actual check/review children override their muted parent; retain both roles.
                let secondary = bounds(window, "pr-mode-card-secondary-7");
                assert_label_pixels(cx, window, secondary, palette.color_blocker);
                assert_label_pixels(cx, window, secondary, palette.color_warning);
                assert_label_pixels(cx, window, file, palette.change_modified);
                app.update(cx, |app, cx| app.pr_mode_select_file(1, cx));
                cx.run_until_parked();
                draw(cx, window);
                let added = bounds(window, "pr-mode-file-1");
                assert_label_pixels(cx, window, added, palette.change_added);
                assert_label_pixels(cx, window, added, palette.text_muted);
                app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
                cx.run_until_parked();
            } else {
                // Draft dimming must not attenuate the selected opaque accent or its label.
                app.update(cx, |app, cx| {
                    app.ui_mut().unwrap().github_prs[0].is_draft = true;
                    cx.notify();
                });
                draw(cx, window);
                assert_eq!(fill(cx, window, "pr-mode-card-7"), expected_fill);
                assert_paint(
                    cx,
                    window,
                    "pr-mode-card-title-7",
                    Hsla::from(gpui::rgb(ink)),
                    ink,
                );
                assert_paint(
                    cx,
                    window,
                    "pr-mode-card-secondary-7",
                    secondary,
                    composite(ink, 0.7, expected_fill),
                );
                app.update(cx, |app, cx| {
                    app.ui_mut().unwrap().github_prs[0].is_draft = false;
                    cx.notify();
                });
            }
            assert_eq!(
                cx.read(|cx| app.read(cx).pr_mode().unwrap().tabs[0].selected_file),
                Some(0)
            );
            assert_eq!(
                cx.read(|cx| app.read(cx).pr_mode().unwrap().tabs[0].pr.key()),
                pr.key()
            );
        }
    }
    unmount(cx, app, window);
}

pub(super) fn conflict_segment_legs(cx: &mut VisualTestAppContext) {
    let fixture = crate::app_conflict::content_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(std::path::Path::new("file.txt"));
        cx.notify();
    });
    for slug in ["apple-light", "apple-dark"] {
        let _accent = theme::AccentInjection::new(theme::SystemAccent::Blue, Some(true));
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for active in [true, false] {
            app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
            draw(cx, window);
            assert_neutral_segment(
                cx,
                window,
                "conflict-result-preview",
                "conflict-result-preview",
            );
            let edit = bounds(window, "conflict-result-edit");
            cx.simulate_click(window, edit.center(), Modifiers::none());
            cx.run_until_parked();
            draw(cx, window);
            assert_neutral_segment(cx, window, "conflict-result-edit", "conflict-result-edit");
            let preview = bounds(window, "conflict-result-preview");
            cx.simulate_click(window, preview.center(), Modifiers::none());
            cx.run_until_parked();
        }
    }
    drop(conflict);
    unmount(cx, app, window);
}
