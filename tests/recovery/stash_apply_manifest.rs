use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{e2e, i18n};

pub fn scenario_stash_apply_manifest(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let original = i18n::lang();
    let zoom = kagi::ui::theme::zoom();
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        for scale in [1., 1.67] {
            i18n::set_lang(lang);
            kagi::ui::theme::set_zoom(scale);
            let fixture = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            std::fs::write(repo.join("index-only.txt"), "base\n").unwrap();
            git(&repo, &["add", "index-only.txt"]);
            git(&repo, &["commit", "-qm", "index-only base"]);
            std::fs::write(repo.join("index-only.txt"), "saved index only\n").unwrap();
            git(&repo, &["add", "index-only.txt"]);
            std::fs::write(repo.join("index-only.txt"), "base\n").unwrap();
            std::fs::write(repo.join("README.md"), "modified\n").unwrap();
            std::fs::write(repo.join("index-entry.txt"), "staged\n").unwrap();
            git(&repo, &["add", "index-entry.txt"]);
            std::fs::write(repo.join("untracked-entry.txt"), "untracked\n").unwrap();
            git(&repo, &["stash", "push", "-u", "-qm", "manifest"]);
            for action in ["apply", "pop"] {
                let (app, window) = mount(cx, &repo);
                app.update(cx, |app, cx| match action {
                    "apply" => app.open_stash_apply_modal(0, cx),
                    _ => app.open_pop_modal(0, cx),
                });
                cx.run_until_parked();
                crate::recovery_operations::paint(cx, window);
                assert!(
                    e2e::control_bounds(window.window_id(), "plan-preview-files").is_some(),
                    "Apply/Pop approval must render the restoration manifest"
                );
                for row in 0..3 {
                    assert!(e2e::control_bounds(
                        window.window_id(),
                        &format!("plan-preview-file-{row}")
                    )
                    .is_some());
                }
                let copy_id = if action == "apply" {
                    "stash-apply-card-copy"
                } else {
                    "plan-card-copy"
                };
                let copy = e2e::control_bounds(window.window_id(), copy_id).unwrap();
                cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
                cx.run_until_parked();
                let copied = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap();
                let heading = i18n::Msg::ModalFilesToChange.t().replace("{}", "3");
                assert!(copied.contains(&heading), "{action} {lang:?}: {copied}");
                for row in [
                    "Modified README.md",
                    "Added index-entry.txt",
                    "Added untracked-entry.txt",
                ] {
                    assert!(copied.contains(row), "{action} {lang:?}: {copied}");
                }
                let not_restored = if lang == i18n::Lang::En {
                    "Index-only content not restored (1)"
                } else {
                    "復元されない index のみの内容 (1)"
                };
                assert!(copied.contains(not_restored));
                assert!(copied.contains("Modified index-only.txt"));
                let manifest = copied
                    .split(&heading)
                    .nth(1)
                    .unwrap()
                    .split("\n\n")
                    .next()
                    .unwrap();
                assert!(
                    !manifest.contains("index-only.txt"),
                    "non-restored content must not be called a restoration"
                );
                unmount(cx, app, window);
            }
            {
                let repository = git2::Repository::open(&repo).unwrap();
                let tree = repository.head().unwrap().peel_to_tree().unwrap();
                let signature = repository.signature().unwrap();
                let orphan = repository
                    .commit(None, &signature, &signature, "orphan stash", &tree, &[])
                    .unwrap();
                repository
                    .reference("refs/stash", orphan, true, "orphan stash")
                    .unwrap();
            }
            for action in ["apply", "pop"] {
                let (app, window) = mount(cx, &repo);
                app.update(cx, |app, cx| match action {
                    "apply" => app.open_stash_apply_modal(0, cx),
                    _ => app.open_pop_modal(0, cx),
                });
                cx.run_until_parked();
                crate::recovery_operations::paint(cx, window);
                let unavailable = if lang == i18n::Lang::En {
                    "Stash manifest unavailable"
                } else {
                    "stash manifest を取得できません"
                };
                crate::native_ax::activate(cx);
                crate::recovery_operations::paint(cx, window);
                assert!(
                    crate::native_ax::tree(cx, window)
                        .to_string()
                        .contains(unavailable),
                    "{action} must expose the actual unavailable-manifest warning"
                );
                let copy_id = if action == "apply" {
                    "stash-apply-card-copy"
                } else {
                    "plan-card-copy"
                };
                let copy = e2e::control_bounds(window.window_id(), copy_id).unwrap();
                cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
                cx.run_until_parked();
                let copied = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap();
                assert!(copied.contains(unavailable));
                assert!(
                    !copied.contains(&i18n::Msg::ModalFilesToChange.t().replace("{}", "0")),
                    "unavailable is not an empty restoration manifest"
                );
                if action == "pop" {
                    assert!(
                        kagi::ui::button_style::recorded_modal_button("plan-confirm")
                            .unwrap()
                            .disabled
                    );
                }
                unmount(cx, app, window);
            }
        }
    }
    i18n::set_lang(original);
    kagi::ui::theme::set_zoom(zoom);
}
