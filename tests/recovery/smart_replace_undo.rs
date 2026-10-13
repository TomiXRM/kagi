//! Standard Input Undo/Redo retains pre-generation edits for every Smart path (#1436).
use super::smart_generation_commit_boundary::{assert_message, edit, generate};
use super::*;

pub fn scenario_smart_replace_undo(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(fixture.path().join("undo-staged"), "new file\n").unwrap();
    git(fixture.path(), &["add", "undo-staged"]);
    for case in [
        Case {
            locale: "en",
            theme: "apple-light",
            dimensions: (1392., 883.),
            zoom: 1.,
        },
        Case {
            locale: "ja",
            theme: "apple-dark",
            dimensions: (940., 660.),
            zoom: 1.67,
        },
    ] {
        for path in ["rule", "llm", "fallback"] {
            let (app, window) = open(cx, fixture.path(), case);
            let panel = cx
                .read(|cx| app.read(cx).ui().commit_panel.clone())
                .unwrap();
            edit(
                cx,
                window,
                &panel,
                "Earlier title",
                "Earlier body\n# Keep template",
            );
            edit(
                cx,
                window,
                &panel,
                "Handwritten 日本語",
                "Long handwritten body\nSecond line\n# Keep template",
            );
            if path == "rule" {
                cx.update_window(window, |_, window, cx| {
                    app.update(cx, |app, cx| {
                        app.smart_suggest(app.active_session().unwrap(), window, cx);
                    });
                })
                .unwrap();
            } else {
                let used_llm = path == "llm";
                e2e::queue_smart_generation(cx.background_executor.spawn(async move {
                    Some(("Generated subject\n\nGenerated body".into(), used_llm))
                }));
                generate(cx, window, &app);
            }
            paint(cx, window);
            let generated = cx.read(|cx| {
                let p = panel.read(cx);
                (
                    p.title_input.as_ref().unwrap().read(cx).value().to_string(),
                    p.body_input.as_ref().unwrap().read(cx).value().to_string(),
                )
            });
            assert_ne!(generated.0, "Handwritten 日本語");
            assert!(
                generated.1.contains("# Keep template"),
                "Smart must retain template comments"
            );
            for body in [false, true] {
                input_key(cx, window, &panel, body, "cmd-z");
            }
            assert_message(
                cx,
                &panel,
                "Handwritten 日本語",
                "Long handwritten body\nSecond line\n# Keep template",
            );
            for body in [false, true] {
                input_key(cx, window, &panel, body, "cmd-shift-z");
            }
            assert_message(cx, &panel, &generated.0, &generated.1);
            for body in [false, true] {
                input_key(cx, window, &panel, body, "cmd-z");
                input_key(cx, window, &panel, body, "cmd-z");
            }
            assert_message(cx, &panel, "Earlier title", "Earlier body\n# Keep template");
            cx.advance_clock(std::time::Duration::from_millis(300));
            paint(cx, window);
            assert_eq!(
                kagi_git::load_draft(&repo, "main").unwrap().message,
                "Earlier title\n\nEarlier body\n# Keep template"
            );
            drop(panel);
            unmount(cx, app, window);
        }
    }
    eprintln!("[gui-e2e] PASS smart_replace_undo");
}

fn input_key(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    panel: &Entity<kagi::ui::commit_panel::CommitPanelView>,
    body: bool,
    key: &str,
) {
    let handle = cx.read(|cx| {
        let p = panel.read(cx);
        let input = if body {
            p.body_input.as_ref()
        } else {
            p.title_input.as_ref()
        }
        .unwrap();
        input.read(cx).focus_handle(cx)
    });
    cx.update_window(window, |_, window, cx| handle.focus(window, cx))
        .unwrap();
    press(cx, window, key);
}
