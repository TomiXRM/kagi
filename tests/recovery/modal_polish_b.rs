//! #1016 PR B: drawn modal controls and full-detail AFTER-state presentation.
use std::path::PathBuf;

use gpui::{AnyWindowHandle, Role, VisualTestAppContext};
use kagi::ui::modals::{EditorFsPromptKind, TrustRepoModal};
use kagi_ui_core::i18n::{Lang, Msg};

use crate::macos::{build_fixture, git, mount, unmount};

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
}

fn button(id: &str, label: &str, description: Option<&str>, disabled: bool) {
    assert_eq!(
        kagi::ui::button_style::recorded_modal_button(id),
        Some(kagi::ui::button_style::ModalButtonA11y {
            role: Role::Button,
            label: label.to_owned(),
            description: description.map(str::to_owned),
            disabled,
        }),
        "{id} must keep its accessible action and reason"
    );
}

fn height(window: AnyWindowHandle, id: &str) {
    let bounds = kagi::ui::e2e::control_bounds(window.window_id(), id)
        .unwrap_or_else(|| panic!("{id} button is drawn"));
    assert_eq!(bounds.size.height, gpui::px(24.), "{id} must be 24px");
}

pub fn scenario_modal_polish_editor_fs(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            app.open_editor_fs_prompt(
                EditorFsPromptKind::NewFile,
                PathBuf::new(),
                String::new(),
                cx,
            );
        });
        paint(cx, window);
        button(
            "editor-fs-prompt-confirm",
            Msg::EditorFsPromptCreateButton.t(),
            Some(Msg::EditorFsNameEmpty.t()),
            true,
        );
        height(window, "editor-fs-prompt-confirm");
        height(window, "editor-fs-prompt-cancel");
        let bounds =
            kagi::ui::e2e::control_bounds(window.window_id(), "editor-fs-prompt-confirm").unwrap();
        cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(
            cx.read(|cx| app.read(cx).editor_fs_prompt_modal().is_some()),
            "invalid name must not submit"
        );

        app.update(cx, |app, cx| {
            app.open_editor_fs_prompt(
                EditorFsPromptKind::NewFile,
                PathBuf::new(),
                "a/b".into(),
                cx,
            );
        });
        paint(cx, window);
        button(
            "editor-fs-prompt-confirm",
            Msg::EditorFsPromptCreateButton.t(),
            Some(Msg::EditorFsNameSeparator.t()),
            true,
        );
        app.update(cx, |app, cx| {
            app.open_editor_fs_prompt(
                EditorFsPromptKind::NewFile,
                PathBuf::new(),
                "valid.txt".into(),
                cx,
            );
        });
        paint(cx, window);
        button(
            "editor-fs-prompt-confirm",
            Msg::EditorFsPromptCreateButton.t(),
            None,
            false,
        );
    }

    app.update(cx, |app, cx| {
        app.open_editor_delete_confirm(PathBuf::from(".git"), true, cx)
    });
    paint(cx, window);
    button(
        "editor-delete-confirm",
        Msg::EditorDeleteConfirmButton.t(),
        Some(Msg::EditorDeleteGitBlocked.t()),
        true,
    );
    height(window, "editor-delete-confirm");
    height(window, "editor-delete-cancel");
    app.update(cx, |app, cx| {
        app.set_trust_repo_modal(TrustRepoModal {
            repo_path: fixture.path().to_path_buf(),
        });
        cx.notify();
    });
    paint(cx, window);
    button("trust-repo-confirm", Msg::TrustRepoConfirm.t(), None, false);
    height(window, "trust-repo-confirm");
    height(window, "trust-repo-cancel");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS modal_polish_editor_fs");
}

pub fn scenario_modal_polish_remote(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            app.open_remote_browse(cx);
        });
        paint(cx, window);
        button("remote-connect-go", Msg::RemoteConnect.t(), None, false);
        height(window, "remote-connect-go");
        height(window, "remote-connect-cancel");
        app.update(cx, |app, cx| {
            app.remote_browse_mut().unwrap().busy = true;
            cx.notify();
        });
        paint(cx, window);
        button(
            "remote-connect-go",
            Msg::RemoteConnecting.t(),
            Some(Msg::RemoteRequestBusy.t()),
            true,
        );
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS modal_polish_remote");
}

pub fn scenario_modal_polish_stash_after(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("README.md"), "stash drop full detail\n").unwrap();
    git(repo, &["stash", "push", "-q", "-m", "pr-b"]);
    let (app, window) = mount(cx, repo);
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            app.open_stash_drop_modal(0, cx);
        });
        cx.run_until_parked();
        paint(cx, window);
        let detail = cx.read(|cx| {
            app.read(cx)
                .stash_drop_modal()
                .unwrap()
                .plan
                .as_ref()
                .unwrap()
                .predicted
                .dirty
                .clone()
        });
        assert_eq!(detail, "working tree unchanged (stash@{0} entry deleted)");
        let chip = kagi::ui::e2e::last_plan_status_chip().expect("drawn AFTER chip");
        assert_eq!(chip, Msg::AfterStashDrop.t(), "{lang:?}");
        assert_ne!(chip, detail);
        let (role, ax) = kagi::ui::dialog_a11y::recorded_note("plan-state-after").unwrap();
        assert_eq!(role, Role::Group);
        assert!(ax.contains(&detail), "{lang:?} AX: {ax}");
        let copy = kagi::ui::e2e::control_bounds(window.window_id(), "plan-card-copy").unwrap();
        cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        let text = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap();
        assert!(text.contains(&detail), "{lang:?} Copy all: {text}");
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS modal_polish_stash_after");
}
