//! Renderer-fed provider help regression for #1342.
use super::*;
use kagi::ui::smart_commit::SmartProvider;
use kagi_domain::message_gen::CliProvider;

pub fn scenario_smart_commit_footer_provider(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("footer-provider"), "staged\n").unwrap();
    git(fixture.path(), &["add", "footer-provider"]);
    for locale in ["en", "ja"] {
        let case = Case {
            locale,
            theme: "apple-light",
            dimensions: (1392., 883.),
            zoom: 1.,
        };
        let (app, window) = open(cx, fixture.path(), case);
        app.update(cx, |app, cx| {
            app.smart_commit.llm_enabled = false;
            cx.notify();
        });
        labels::clear();
        paint(cx, window);
        assert_eq!(labels::accessible_description("cp-smart-suggest"), None);
        for (provider, name) in [
            (SmartProvider::Ollama, "Ollama (fixture-model)"),
            (SmartProvider::Cli(CliProvider::ClaudeCode), "Claude Code"),
            (SmartProvider::Cli(CliProvider::Codex), "Codex"),
        ] {
            app.update(cx, |app, cx| {
                app.smart_commit.provider = provider;
                app.smart_commit.model = Some("fixture-model".into());
                app.smart_commit.llm_enabled = true;
                app.smart_commit.ollama_available = true;
                app.smart_commit.claude_available = true;
                app.smart_commit.codex_available = true;
                cx.notify();
            });
            paint(cx, window);
            let expected = if locale == "en" {
                format!("Generate commit message with {name}")
            } else {
                format!("{name} で commit メッセージを生成")
            };
            assert_eq!(
                labels::accessible_description("cp-smart-suggest").as_deref(),
                Some(expected.as_str())
            );
            freeze_then_change_provider(cx, window, &app, provider);
            let expected = if locale == "en" {
                format!("Generating with {name}…")
            } else {
                format!("{name} で生成中…")
            };
            assert_eq!(
                labels::accessible_description("cp-smart-suggest").as_deref(),
                Some(expected.as_str())
            );
            app.update(cx, |app, cx| {
                app.ui_mut().unwrap().smart_commit_generating = false;
                cx.notify();
            });
        }
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS smart_commit_footer_provider");
}

fn freeze_then_change_provider(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
    provider: SmartProvider,
) {
    app.update(cx, |app, cx| {
        let ui = app.ui_mut().unwrap();
        ui.smart_commit_generating = true;
        ui.smart_commit_generation = Some(kagi::ui::smart_commit::SmartGeneration {
            provider,
            model: (provider == SmartProvider::Ollama).then(|| "fixture-model".into()),
        });
        cx.notify();
    });
    paint(cx, window);
    app.update(cx, |app, cx| {
        app.smart_commit.provider = if provider == SmartProvider::Ollama {
            SmartProvider::Cli(CliProvider::Codex)
        } else {
            SmartProvider::Ollama
        };
        app.smart_commit.model = Some("changed-model".into());
        cx.notify();
    });
    paint(cx, window);
}
