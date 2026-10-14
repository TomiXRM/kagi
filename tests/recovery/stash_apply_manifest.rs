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
            std::fs::write(repo.join("README.md"), "modified\n").unwrap();
            std::fs::write(repo.join("index-entry.txt"), "staged\n").unwrap();
            git(&repo, &["add", "index-entry.txt"]);
            std::fs::write(repo.join("untracked-entry.txt"), "untracked\n").unwrap();
            git(&repo, &["stash", "push", "-u", "-qm", "manifest"]);
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| app.open_stash_apply_modal(0, cx));
            cx.run_until_parked();
            crate::recovery_operations::paint(cx, window);
            assert!(
                e2e::control_bounds(window.window_id(), "plan-preview-files").is_some(),
                "Apply approval must render the restoration manifest"
            );
            for row in 0..3 {
                assert!(e2e::control_bounds(
                    window.window_id(),
                    &format!("plan-preview-file-{row}")
                )
                .is_some());
            }
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(original);
    kagi::ui::theme::set_zoom(zoom);
}
