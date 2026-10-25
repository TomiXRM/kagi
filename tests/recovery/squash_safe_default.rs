//! #1529 / #1531: PR merge safety and GitHub-owned confirmation scope.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    button_style, e2e,
    i18n::{self, Lang},
    KagiApp,
};
use kagi_ui_core::theme;

fn settle(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while cx.read(|cx| app.read(cx).planning.is_some()) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "PR merge plan never settled"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.run_until_parked();
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    button_style::clear_recorded_modal_buttons();
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    }
}

fn focus_confirm(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for _ in 0..32 {
        button_style::modal_focus_probe();
        cx.simulate_keystrokes(window, "tab");
        paint(cx, window);
        cx.simulate_keystrokes(window, "f19");
        if button_style::modal_focus_probe() == Some("plan-confirm") {
            return;
        }
    }
    panic!("Merge must be an explicit Tab stop inside the alert trap");
}

fn matrix() -> impl Iterator<Item = (Lang, &'static str, f32)> {
    [Lang::En, Lang::Ja].into_iter().flat_map(|language| {
        ["apple-light", "apple-dark"]
            .into_iter()
            .flat_map(move |palette| {
                [1., 1.67]
                    .into_iter()
                    .map(move |zoom| (language, palette, zoom))
            })
    })
}

fn pr(repo: &std::path::Path) -> kagi_domain::github::PullRequest {
    kagi_domain::github::PullRequest {
        number: 1529,
        head: "feature".into(),
        head_sha: crate::pr_viewed::rev(repo, "HEAD~1"),
        base: "release".into(),
        base_repo: "github.example/acme/widgets".into(),
        cross_repository: false,
        changed_files: 3,
        additions: 17,
        deletions: 4,
        ..Default::default()
    }
}

pub fn pr_merge_delete_branch_no_default(cx: &mut VisualTestAppContext) {
    let _settings = crate::recovery_layout::GlobalSettings::capture();
    let _theme = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let transport = tempfile::tempdir().unwrap();
    let calls = transport.path().join("merge-args");
    let _offline = crate::pr_fields_focus::OfflineGh::with_script(&format!(
        "#!/bin/sh\ncase \"$1 $2\" in\n'pr merge') printf '%s\\n' \"$@\" >> '{}'; echo queued ;;\n'pr view') echo '{{\"mergedAt\":null}}' ;;\n*) echo '[]' ;;\nesac\n", calls.display()
    ));
    for (language, palette, zoom) in matrix() {
        i18n::set_lang(language);
        theme::set_zoom(zoom);
        let fixture = build_fixture();
        git(fixture.path(), &["branch", "feature", "HEAD~1"]);
        let raw = git2::Repository::open(fixture.path()).unwrap();
        let before = raw.refname_to_id("refs/heads/feature").unwrap();
        let head_before = raw.head().unwrap().target();
        let request = kagi_domain::github::PullRequest {
            changed_files: 0,
            additions: 0,
            deletions: 0,
            ..pr(fixture.path())
        };
        let (app, window) = mount(cx, fixture.path());
        app.update(cx, |app, cx| {
            app.set_theme(palette, cx);
            app.open_pr_merge_modal(&request, kagi_git::github::MergeMethod::Squash, true, cx);
        });
        settle(cx, &app);
        paint(cx, window);
        assert!(
            !button_style::recorded_modal_button("plan-confirm")
                .unwrap()
                .disabled
        );
        assert!(
            cx.read(|cx| app.read(cx).pr_merge_modal().unwrap().plan.destructive),
            "delete-branch Squash merge must not be Primary"
        );
        let copy = e2e::control_bounds(window.window_id(), "plan-card-copy").unwrap();
        cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        let copied = cx.update(|cx| cx.read_from_clipboard().unwrap().text().unwrap());
        assert!(
            copied.contains("totals are unavailable")
                || copied.contains("合計は取得できていません"),
            "Missing L3 totals must disclose unavailable, not assert a default zero scope"
        );
        assert!(!copied.contains("Files: 0") && !copied.contains("ファイル: 0 件"));
        let calls_before = std::fs::read(&calls).unwrap_or_default();
        crate::destructive_modal_return::root_return_keeps_modal(cx, &app, window, fixture.path());
        assert!(
            cx.read(|cx| app.read(cx).pr_merge_modal().is_some()),
            "root Return retains the merge sheet"
        );
        assert!(
            !cx.read(|cx| app.read(cx).app_sessions.has_leases()),
            "Return admits no merge write"
        );
        assert_eq!(std::fs::read(&calls).unwrap_or_default(), calls_before);
        assert_eq!(raw.refname_to_id("refs/heads/feature").unwrap(), before);
        assert_eq!(raw.head().unwrap().target(), head_before);
        crate::recovery_operations::press_key(cx, &app, window, "escape");
        assert!(cx.read(|cx| app.read(cx).pr_merge_modal().is_none()));

        let mut blocked = request.clone();
        blocked.is_draft = true;
        app.update(cx, |app, cx| {
            app.open_pr_merge_modal(&blocked, kagi_git::github::MergeMethod::Squash, true, cx)
        });
        settle(cx, &app);
        paint(cx, window);
        assert!(
            button_style::recorded_modal_button("plan-confirm")
                .unwrap()
                .disabled
        );
        let calls_before = std::fs::read(&calls).unwrap_or_default();
        crate::destructive_modal_return::root_return_keeps_modal(cx, &app, window, fixture.path());
        assert_eq!(std::fs::read(&calls).unwrap_or_default(), calls_before);
        crate::recovery_operations::press_key(cx, &app, window, "escape");
        assert!(cx.read(|cx| app.read(cx).pr_merge_modal().is_none()));
        app.update(cx, |app, cx| {
            app.open_pr_merge_modal(&request, kagi_git::github::MergeMethod::Squash, true, cx)
        });
        settle(cx, &app);
        paint(cx, window);
        focus_confirm(cx, window);
        crate::keyboard_nav::keys(cx, window, "space");
        cx.run_until_parked();
        let argv = std::fs::read_to_string(&calls).unwrap();
        assert!(argv.contains(&format!("--match-head-commit\n{}", request.head_sha)));
        assert!(argv.contains(&format!("-R\n{}", request.base_repo)));
        assert_eq!(
            raw.refname_to_id("refs/heads/feature").unwrap(),
            before,
            "queued is not server merge confirmation; no local deletion"
        );
        assert_eq!(raw.head().unwrap().target(), head_before);
        assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));

        unmount(cx, app, window);
    }
}

pub fn pr_merge_card_scope(cx: &mut VisualTestAppContext) {
    let _settings = crate::recovery_layout::GlobalSettings::capture();
    let _theme = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    for (language, palette, zoom) in matrix() {
        i18n::set_lang(language);
        theme::set_zoom(zoom);
        let fixture = build_fixture();
        let remote_dir = tempfile::tempdir().unwrap();
        let remote = remote_dir.path().join("repo.git");
        git(remote_dir.path(), &["init", "-q", "--bare", "repo.git"]);
        git(
            fixture.path(),
            &["push", "-q", remote.to_str().unwrap(), "main"],
        );
        let url = "https://github.com/example/repo.git";
        let file_url = format!("file://{}", remote.display());
        git(fixture.path(), &["remote", "add", "origin", url]);
        git(
            fixture.path(),
            &["config", &format!("url.{file_url}.insteadOf"), url],
        );
        let head = crate::pr_viewed::push_pr_head(
            fixture.path(),
            &remote,
            "main",
            &[("scope.txt", "one\ntwo\n")],
        );
        git(fixture.path(), &["checkout", "-q", "-b", "local-only"]);
        let request = kagi_domain::github::PullRequest {
            changed_files: 1,
            additions: 2,
            deletions: 0,
            ..crate::pr_viewed::pr_at(&head)
        };
        let _offline = crate::pr_fields_focus::OfflineGh::with_script(&format!(
            "#!/bin/sh\ncase \"$1 $2\" in\n'pr view') echo '{{\"number\":7,\"headRefOid\":\"{head}\",\"body\":\"scope body\",\"changedFiles\":1,\"additions\":2,\"deletions\":0}}' ;;\n*) echo '{{}}' ;;\nesac\n"
        ));
        let (app, window) = mount(cx, fixture.path());
        app.update(cx, |app, cx| app.set_theme(palette, cx));
        e2e::queue_github_pr_conversation(gpui::Task::ready((
            Ok((Vec::new(), Vec::new())),
            Ok(Vec::new()),
        )));
        app.update(cx, |app, cx| app.pr_mode_open(&request, cx));
        crate::pr_viewed::wait_loaded(cx, &app, &head);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !cx.read(|cx| {
            app.read(cx)
                .pr_mode()
                .is_some_and(|mode| mode.tabs[0].pr.body == "scope body")
        }) {
            assert!(
                std::time::Instant::now() < deadline,
                "same-head L3 body must load"
            );
            cx.run_until_parked();
        }
        app.update(cx, |app, cx| {
            app.pr_mode_select_commit(Some(0), cx);
            app.open_pr_merge_modal(&request, kagi_git::github::MergeMethod::Squash, false, cx);
        });
        settle(cx, &app);
        app.update(cx, |app, _| {
            let plan = &app.pr_merge_modal().unwrap().plan;
            assert_eq!(
                plan.predicted.head, "github.com/example/repo:main",
                "AFTER must name the GitHub target, not local HEAD"
            );
            assert_eq!(plan.current.head, "local-only");
            assert_eq!(plan.preview_files.len(), 1);
            assert_eq!(plan.preview_files[0].path.to_str(), Some("scope.txt"));
            assert!(
                plan.warnings
                    .iter()
                    .any(|note| i18n::plan_note_text(note).contains(&head)),
                "source full SHA must be visible"
            );
            assert!(
                plan.warnings.iter().any(|note| {
                    let text = i18n::plan_note_text(note);
                    text.contains("1") && text.contains("+2") && text.contains("−0")
                }),
                "known PR file/change totals must appear in the card"
            );
            assert!(
                !plan.destructive,
                "retaining branches keeps the ordinary merge action"
            );
        });
        paint(cx, window);
        assert!(
            !button_style::recorded_modal_button("plan-confirm")
                .unwrap()
                .disabled
        );
        assert!(
            e2e::control_bounds(window.window_id(), "plan-preview-files").is_some(),
            "same-head whole-PR paths are rendered"
        );
        let copy = e2e::control_bounds(window.window_id(), "plan-card-copy").unwrap();
        cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        let copied = cx.update(|cx| cx.read_from_clipboard().unwrap().text().unwrap());
        assert!(copied.contains("scope.txt") && copied.contains(&head));
        assert!(copied.contains("github.com/example/repo:main"));
        cx.simulate_keystrokes(window, "escape");
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).pr_merge_modal().is_none()));
        assert_eq!(
            crate::pr_viewed::rev(fixture.path(), "HEAD"),
            crate::pr_viewed::rev(fixture.path(), "main")
        );
        // A detail read for a different head cannot contribute stale paths.
        app.update(cx, |app, cx| {
            let mut changed = request.clone();
            changed.head_sha = crate::pr_viewed::rev(fixture.path(), "HEAD");
            app.open_pr_merge_modal(&changed, kagi_git::github::MergeMethod::Squash, false, cx);
        });
        settle(cx, &app);
        app.update(cx, |app, _| {
            let plan = &app.pr_merge_modal().unwrap().plan;
            assert!(plan.preview_files.is_empty());
            assert!(plan.warnings.iter().any(|note| matches!(
                note,
                kagi_domain::plan_note::PlanNote::Github(
                    kagi_domain::plan_note::GithubNote::MergeFilesUnavailable
                )
            )));
        });
        unmount(cx, app, window);
    }
}
