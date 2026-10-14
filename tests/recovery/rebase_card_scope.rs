use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use gpui::VisualTestAppContext;
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};

pub fn scenario_rebase_card_scope(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let language = i18n::lang();
    let zoom = theme::zoom();
    let fixture = build_fixture();
    let path = fixture.path();
    git(path, &["checkout", "-qb", "feature", "HEAD~1"]);
    for n in 1..=3 {
        std::fs::write(path.join(format!("scope-{n}")), format!("{n}\n")).unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-qm", &format!("scope commit {n}")]);
    }
    let before = repo_fingerprint(path);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        for scale in [1., 1.67] {
            theme::set_zoom(scale);
            let (app, window) = mount(cx, path);
            app.update(cx, |app, cx| app.open_rebase_modal("main".into(), cx));
            crate::recovery_operations::wait_idle(cx, &app);
            cx.update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear();
            })
            .unwrap();
            cx.read(|cx| {
                let plan = &app.read(cx).rebase_current_onto_modal().unwrap().plan;
                assert_eq!(
                    plan.preview_commits.len(),
                    3,
                    "rebase card must disclose three replay candidates"
                );
                for (i, entry) in plan.preview_commits.iter().enumerate() {
                    assert!(entry.ends_with(&format!("scope commit {}", i + 1)));
                }
            });
            assert_eq!(
                repo_fingerprint(path),
                before,
                "opening rebase scope is read-only"
            );
            crate::native_ax::activate(cx);
            cx.update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear();
            })
            .unwrap();
            let tree = crate::native_ax::tree(cx, window).to_string();
            assert!(
                tree.contains(match lang {
                    Lang::En => "Replay candidates: 3",
                    Lang::Ja => "replay 候補の commit: 3",
                }),
                "native AX must expose the localized replay scope heading: {tree}"
            );
            let entries = cx.read(|cx| {
                app.read(cx)
                    .rebase_current_onto_modal()
                    .unwrap()
                    .plan
                    .preview_commits
                    .clone()
            });
            let list = kagi::ui::e2e::control_bounds(window.window_id(), "modal-target-list")
                .expect("replay list drawn");
            for entry in entries {
                let bounds = kagi::ui::e2e::control_bounds(
                    window.window_id(),
                    &format!("modal-commit-{entry}"),
                )
                .expect("replay candidate drawn");
                assert!(
                    bounds.origin.y >= list.origin.y && bounds.bottom() <= list.bottom(),
                    "all three replay candidates fit the scope list"
                );
            }
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(language);
    theme::set_zoom(zoom);
}
