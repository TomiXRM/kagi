//! #1748: workspace navigation changes the mounted body, never its draft owner.
use super::*;
use gpui::{point, px, ScrollStrategy};
use kagi::ui::i18n::Msg;
use kagi::ui::workspace_mode::WorkspaceMode;

fn drawn(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle, id: &str) -> bool {
    e2e::clear_control_bounds(window.window_id(), id);
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(window.window_id(), id).is_some()
}

pub fn scenario_conflict_leave_and_return(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        leave_and_return(cx, lang);
    }
    i18n::set_lang(original);
}

fn leave_and_return(cx: &mut VisualTestAppContext, lang: Lang) {
    let fixture = fixture_with_context(120);
    let repo = fixture.path();
    let before = repo_fingerprint(repo);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    click_control(cx, window, "conflict-row-1");
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    let entity_id = conflict.entity_id();
    conflict.update(cx, |view, cx| {
        view.conflict_editor_select_hunk(1);
        cx.notify();
    });
    click_control(cx, window, "conflict-result-editor");
    let input = cx.read(|cx| {
        conflict
            .read(cx)
            .editor_inputs
            .as_ref()
            .unwrap()
            .result
            .clone()
    });
    let input_id = input.entity_id();
    let manual = (0..120)
        .map(|i| format!("unsaved Result {i}\n"))
        .collect::<String>();
    cx.simulate_keystrokes(window, "cmd-a");
    cx.simulate_input(window, &manual);
    cx.run_until_parked();
    input.update(cx, |input, cx| {
        input.set_scroll_offset(point(px(0.), px(-240.)), cx)
    });
    conflict.update(cx, |view, cx| {
        view.ab_scroll_handle
            .scroll_to_item(50, ScrollStrategy::Top);
        cx.notify();
    });
    assert!(drawn(cx, window, "conflict-result-editor"));
    let (source_scroll, result_scroll, selection) = cx.read(|cx| {
        let view = conflict.read(cx);
        let source = view.ab_scroll_handle.0.borrow().base_handle.offset();
        let result = input.read(cx).scroll_offset();
        assert!(source.y < px(0.), "exercise a genuinely scrolled source");
        assert!(result.y < px(0.), "exercise a genuinely scrolled Result");
        (source, result, view.mode.as_ref().unwrap().selected_file)
    });
    let old_revision = cx.read(|cx| conflict.read(cx).mode.as_ref().unwrap().revision.clone());

    for external_change in [false, true] {
        click_control(cx, window, "tb-graph-mode");
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(state.workspace_mode(), WorkspaceMode::Graph);
            assert!(state.ui().conflict_hidden);
            assert_eq!(state.ui().conflict.as_ref().unwrap().entity_id(), entity_id);
            assert!(state.view().operation.is_some(), "leaving is not aborting");
        });
        assert!(!drawn(cx, window, "conflict-result-editor"));
        assert!(drawn(cx, window, "operation-strip-back"));
        let back = crate::native_ax::node(cx, window, "operation-strip-back".into());
        assert_eq!(back["aria"]["label"], Msg::ConflictBackToConflicts.t());
        click_control(cx, window, "sidebar-local-feature");
        cx.read(|cx| {
            let state = app.read(cx);
            let selected = state
                .ui()
                .selected
                .expect("branch sidebar navigates the graph");
            let repository = git2::Repository::open(repo).unwrap();
            let feature = repository
                .refname_to_id("refs/heads/feature")
                .unwrap()
                .to_string();
            assert_eq!(state.view().rows[selected].id.0, feature);
        });
        // The ordinary Commit Panel is reachable, and its existing #1750
        // disabled reason still explains the active conflicts instead of an empty set.
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| app.open_commit_panel(window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        assert!(drawn(cx, window, "cp-discard-all"));
        let reason = i18n::plan_note_text(&kagi_domain::plan_note::PlanNote::Common(
            kagi_domain::plan_note::CommonNote::ConflictedFiles {
                count: 2,
                before: kagi_domain::plan_note::OpPhrase::DiscardingChanges,
            },
        ));
        assert_eq!(
            e2e::commit_panel_labels::accessible_disabled("cp-discard-all"),
            Some(true)
        );
        assert_eq!(
            e2e::commit_panel_labels::accessible_description("cp-discard-all").as_deref(),
            Some(reason.as_str())
        );
        if external_change {
            // A real accepted changed index observation refreshes the hidden
            // pane in place; it must neither reopen nor discard the live Result.
            std::fs::write(repo.join("external.txt"), "external staged data\n").unwrap();
            git(repo, &["add", "external.txt"]);
            app.update(cx, |app, cx| app.reload(cx));
            crate::busy_dismiss::ready(cx, &app);
            cx.run_until_parked();
            cx.read(|cx| {
                assert!(app.read(cx).ui().conflict_hidden);
                assert_eq!(
                    app.read(cx).ui().conflict.as_ref().unwrap().entity_id(),
                    entity_id
                );
                assert_ne!(
                    conflict.read(cx).mode.as_ref().unwrap().revision,
                    old_revision
                );
            });
            assert!(!drawn(cx, window, "conflict-result-editor"));
            assert!(drawn(cx, window, "operation-strip-back"));
        } else {
            // Editor uses the same explicit navigation path as Graph/PRs.
            click_control(cx, window, "tb-editor-ws");
            cx.read(|cx| assert_eq!(app.read(cx).workspace_mode(), WorkspaceMode::Editor));
            assert!(!drawn(cx, window, "conflict-result-editor"));
            assert!(drawn(cx, window, "operation-strip-back"));
        }
        let repository_before_return = repo_fingerprint(repo);
        click_control(cx, window, "operation-strip-back");
        assert!(drawn(cx, window, "conflict-result-editor"));
        assert!(!drawn(cx, window, "operation-strip-back"));
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(!state.ui().conflict_hidden);
            assert_eq!(state.ui().conflict.as_ref().unwrap().entity_id(), entity_id);
            let view = conflict.read(cx);
            assert_eq!(view.editing.as_deref(), Some(Path::new("b.txt")));
            assert_eq!(view.mode.as_ref().unwrap().selected_file, selection);
            assert_eq!(view.selected_hunk, 1);
            assert!(view.result_editing);
            assert_eq!(
                view.editor_inputs.as_ref().unwrap().result.entity_id(),
                input_id
            );
            assert_eq!(input.read(cx).value().as_str(), manual);
            assert_eq!(
                view.mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .resolved_text(Path::new("b.txt"))
                    .as_deref(),
                Some(manual.as_str())
            );
            assert_eq!(
                view.ab_scroll_handle.0.borrow().base_handle.offset(),
                source_scroll
            );
            assert_eq!(input.read(cx).scroll_offset(), result_scroll);
        });
        assert_eq!(
            repo_fingerprint(repo),
            repository_before_return,
            "navigation never writes Git"
        );
        if !external_change {
            assert_eq!(repo_fingerprint(repo), before);
        }
    }
    drop(input);
    drop(conflict);
    unmount(cx, app, window);
}
