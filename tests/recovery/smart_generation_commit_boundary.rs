//! A consumed message ends the lifetime of its pending Smart result (#1434).
use super::*;
use kagi::ui::commit_panel::CommitPanelView;

pub fn scenario_smart_generation_commit_boundary(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("staged-a"), "A\n").unwrap();
    git(&repo, &["add", "staged-a"]);
    std::fs::write(repo.join("unstaged-b"), "B\n").unwrap();
    let (app, window) = open(
        cx,
        &repo,
        Case {
            locale: "en",
            theme: "apple-light",
            dimensions: (1392., 883.),
            zoom: 1.,
        },
    );
    let panel = cx
        .read(|cx| app.read(cx).ui().commit_panel.clone())
        .unwrap();
    edit(cx, window, &panel, "Commit A", "Handwritten A");
    let timer = cx.background_executor.clone();
    e2e::queue_smart_generation(cx.background_executor.spawn(async move {
        timer.timer(std::time::Duration::from_secs(10)).await;
        Some(("Old A result\n\nOld A body".into(), true))
    }));
    generate(cx, window, &app);
    app.update(cx, |app, cx| {
        app.open_commit_plan_modal(app.active_session().unwrap(), cx);
    });
    paint(cx, window);
    app.update(cx, |app, cx| app.start_commit(cx));
    paint(cx, window);
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%s"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("Commit A"));
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().entity_id()),
        panel.entity_id()
    );
    edit(cx, window, &panel, "Next B", "次の本文 B\nKeep exactly");
    cx.advance_clock(std::time::Duration::from_secs(10));
    paint(cx, window);
    assert_message(cx, &panel, "Next B", "次の本文 B\nKeep exactly");
    assert!(cx.read(|cx| panel.read(cx).pending_smart_msg.is_none()));
    cx.advance_clock(std::time::Duration::from_millis(300));
    paint(cx, window);
    assert_eq!(
        kagi_git::load_draft(&repo, "main").unwrap().message,
        "Next B\n\n次の本文 B\nKeep exactly"
    );
    e2e::queue_smart_generation(
        cx.background_executor
            .spawn(async { Some(("Explicit B\n\nNew generation".into(), true)) }),
    );
    generate(cx, window, &app);
    paint(cx, window);
    assert_message(cx, &panel, "Explicit B", "New generation");
    drop(panel);
    unmount(cx, app, window);
    discarded_panel_generation(cx);
    complete_commit_closes_panel(cx);
    eprintln!("[gui-e2e] PASS smart_generation_commit_boundary");
}

pub(super) fn edit(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    panel: &Entity<CommitPanelView>,
    title: &str,
    body: &str,
) {
    cx.update_window(window, |_, window, cx| {
        let (title_input, body_input) = {
            let panel = panel.read(cx);
            (
                panel.title_input.clone().unwrap(),
                panel.body_input.clone().unwrap(),
            )
        };
        title_input.update(cx, |s, cx| s.replace_all(title.to_string(), window, cx));
        body_input.update(cx, |s, cx| s.replace_all(body.to_string(), window, cx));
        panel.update(cx, |_, cx| cx.notify());
    })
    .unwrap();
    paint(cx, window);
}

pub(super) fn assert_message(
    cx: &mut VisualTestAppContext,
    panel: &Entity<CommitPanelView>,
    title: &str,
    body: &str,
) {
    cx.read(|cx| {
        let panel = panel.read(cx);
        assert_eq!(
            panel
                .title_input
                .as_ref()
                .unwrap()
                .read(cx)
                .value()
                .as_ref(),
            title,
            "Smart must not overwrite the next draft"
        );
        assert_eq!(
            panel.body_input.as_ref().unwrap().read(cx).value().as_ref(),
            body
        );
        assert_eq!(
            panel.effective_commit_message(cx),
            kagi_git::join_title_body(title, body)
        );
    });
}

pub(super) fn generate(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
) {
    app.update(cx, |app, _| {
        app.smart_commit.llm_enabled = true;
        app.smart_commit.provider = kagi::ui::smart_commit::SmartProvider::Ollama;
        app.smart_commit.ollama_available = true;
        app.smart_commit.model = Some("fixture-model".into());
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.smart_generate(app.active_session().unwrap(), window, cx)
        });
    })
    .unwrap();
    paint(cx, window);
}

fn discarded_panel_generation(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("staged"), "staged\n").unwrap();
    git(fixture.path(), &["add", "staged"]);
    let case = Case {
        locale: "en",
        theme: "apple-light",
        dimensions: (1392., 883.),
        zoom: 1.,
    };
    let (app, window) = open(cx, fixture.path(), case);
    let timer = cx.background_executor.clone();
    e2e::queue_smart_generation(cx.background_executor.spawn(async move {
        timer.timer(std::time::Duration::from_secs(10)).await;
        Some(("Discarded panel's result".into(), true))
    }));
    generate(cx, window, &app);
    let weak = cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().downgrade());
    app.update(cx, |app, cx| {
        let ui = app.ui_mut().unwrap();
        ui.commit_panel = None;
        ui.commit_panel_open = false;
        cx.notify();
    });
    paint(cx, window);
    assert!(
        weak.upgrade().is_none(),
        "discard must release the initiating panel"
    );
    cx.advance_clock(std::time::Duration::from_secs(10));
    paint(cx, window);
    assert!(!cx.read(|cx| app.read(cx).ui().smart_commit_generating));
    assert!(cx.read(|cx| app.read(cx).ui().smart_commit_generation.is_none()));
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    paint(cx, window);
    assert_eq!(labels::accessible_disabled("cp-smart-suggest"), Some(false));
    unmount(cx, app, window);
}

fn complete_commit_closes_panel(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("staged"), "staged\n").unwrap();
    git(fixture.path(), &["add", "staged"]);
    let (app, window) = open(
        cx,
        fixture.path(),
        Case {
            locale: "en",
            theme: "apple-light",
            dimensions: (1392., 883.),
            zoom: 1.,
        },
    );
    let panel = cx
        .read(|cx| app.read(cx).ui().commit_panel.clone())
        .unwrap();
    edit(cx, window, &panel, "Commit all staged files", "");
    app.update(cx, |app, cx| {
        app.open_commit_plan_modal(app.active_session().unwrap(), cx)
    });
    paint(cx, window);
    app.update(cx, |app, cx| app.start_commit(cx));
    paint(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).ui().commit_panel.is_none()),
        "a consumed message cannot retain an empty commit panel during reload"
    );
    drop(panel);
    unmount(cx, app, window);
}
