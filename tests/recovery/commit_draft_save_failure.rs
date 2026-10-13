//! Real draft destination failure, localized feedback and explicit recovery (#1435).
use super::smart_generation_commit_boundary::{assert_message, edit};
use super::*;

pub fn scenario_commit_draft_save_failure(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let drafts = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap()).join("drafts");
    for case in matrix() {
        let (app, window) = open(cx, &repo, case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        std::fs::create_dir_all(&drafts).unwrap();
        let saved_dir = drafts.with_extension("failure-backup");
        std::fs::rename(&drafts, &saved_dir).unwrap();
        std::fs::write(&drafts, "not a directory").unwrap();
        let before = contract_count("[kagi] draft: saved main");
        edit(
            cx,
            window,
            &panel,
            "Draft B",
            "日本語 body B\nPreserve bytes",
        );
        cx.advance_clock(std::time::Duration::from_millis(300));
        paint(cx, window);
        // Restore the shared directory before any oracle can panic.
        std::fs::remove_file(&drafts).unwrap();
        std::fs::rename(&saved_dir, &drafts).unwrap();
        assert_message(cx, &panel, "Draft B", "日本語 body B\nPreserve bytes");
        assert_ne!(
            cx.read(|cx| panel.read(cx).last_draft_value.clone()),
            "Draft B\n\n日本語 body B\nPreserve bytes",
            "failed autosave must retain an unsaved baseline"
        );
        assert_eq!(
            contract_count("[kagi] draft: saved main"),
            before,
            "failure cannot log saved"
        );
        let feedback = labels::accessible_name("cp-draft-save-error").unwrap();
        assert!(feedback.starts_with(if case.locale == "ja" {
            "下書きを保存できません"
        } else {
            "Draft could not be saved"
        }));
        let retry = bounds(window, "cp-draft-save-retry");
        contained(
            bounds(window, "cp-footer"),
            retry,
            "draft retry within footer",
        );
        cx.simulate_click(window, retry.center(), gpui::Modifiers::default());
        paint(cx, window);
        assert_eq!(
            kagi_git::load_draft(&repo, "main").unwrap().message,
            "Draft B\n\n日本語 body B\nPreserve bytes"
        );
        assert_eq!(contract_count("[kagi] draft: saved main"), before + 1);
        drop(panel);
        unmount(cx, app, window);
        let (app, window) = open(cx, &repo, case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        assert_message(cx, &panel, "Draft B", "日本語 body B\nPreserve bytes");
        drop(panel);
        unmount(cx, app, window);
        kagi_git::clear_draft(&repo, "main").unwrap();
    }
    eprintln!("[gui-e2e] PASS commit_draft_save_failure");
}
