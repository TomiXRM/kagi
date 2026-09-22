//! Pull/Push/Branch/Stash/Pop/Undo/Redo/Terminal belong to Graph. PRs, Editor and
//! Analyze must not draw them, and returning to Graph brings them back.
use crate::evidence_support::pull_request;
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::e2e;
use kagi::ui::i18n::{self, Lang};
use kagi::ui::workspace_mode::WorkspaceMode;

const REPO_ACTIONS: &str = "tb-repo-actions";

/// Draw one fresh frame and report whether it laid out the repo actions.
fn repo_actions_drawn(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> bool {
    cx.run_until_parked();
    let id = win.window_id();
    e2e::clear_control_bounds(id, REPO_ACTIONS);
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(id, REPO_ACTIONS).is_some()
}

fn swipe_phase(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
    x: f32,
    touch_phase: gpui::TouchPhase,
) {
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(x), gpui::px(0.))),
            touch_phase,
            ..Default::default()
        },
    );
}

/// Draw one fresh frame and report the control's laid-out bounds.
fn measure(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    control: &str,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    e2e::clear_control_bounds(win.window_id(), control);
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(win.window_id(), control)
}

fn click_control(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) {
    let bounds = measure(cx, win, id).unwrap_or_else(|| panic!("missing {id}"));
    cx.simulate_click(win, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn assert_list_filters(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    prefix: &str,
    all: &[u64],
    retained: &[u64],
    updated_first: u64,
    created_first: u64,
) {
    click_control(cx, win, "list-filter-label");
    click_control(cx, win, "list-filter-option-0-1"); // Loaded label "bug".
    cx.simulate_keystrokes(win, "escape");
    cx.run_until_parked();
    for number in all {
        assert_eq!(
            measure(cx, win, &format!("{prefix}-{number}")).is_some(),
            retained.contains(number),
            "the selected label must change the rendered rows"
        );
    }
    let updated = measure(cx, win, &format!("{prefix}-{updated_first}")).unwrap();
    let created = measure(cx, win, &format!("{prefix}-{created_first}")).unwrap();
    assert!(
        updated.origin.y < created.origin.y,
        "updated-desc is the initial order"
    );
    click_control(cx, win, "list-filter-sort");
    click_control(cx, win, "list-filter-option-0-1"); // Created.
    let updated = measure(cx, win, &format!("{prefix}-{updated_first}")).unwrap();
    let created = measure(cx, win, &format!("{prefix}-{created_first}")).unwrap();
    assert!(
        created.origin.y < updated.origin.y,
        "created-desc must change the first row"
    );
    click_control(cx, win, "list-filter-clear");
    for number in all {
        assert!(measure(cx, win, &format!("{prefix}-{number}")).is_some());
    }
}

/// Run the sidebar's settle animation to rest (ADR-0199). The settle is a
/// timer-driven spring, so the test clock has to be advanced past it before
/// the page it committed to becomes the active workspace.
fn settle_sidebar(cx: &mut VisualTestAppContext) {
    cx.advance_clock(std::time::Duration::from_millis(600));
    cx.run_until_parked();
}

fn assert_pr_state_isolation(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    app: &gpui::Entity<kagi::ui::KagiApp>,
) {
    use kagi_domain::{github::IssueState, list_filter::StateFilter};
    let open = cx.read(|cx| app.read(cx).ui().github_prs.clone());
    let (task, closed_reply) = crate::evidence_support::deferred(cx);
    e2e::queue_github_pr_fetch(task);
    click_control(cx, win, "list-filter-state");
    click_control(cx, win, "list-filter-option-0-1");
    assert!(measure(cx, win, "pr-list-refreshing").is_some());
    assert_eq!(cx.read(|cx| app.read(cx).ui().github_prs.clone()), open);
    let mut closed = pull_request(77, "closed record", "main");
    closed.state = IssueState::Closed;
    closed_reply.send(Ok(vec![closed.clone()]));
    cx.run_until_parked();
    assert!(measure(cx, win, "pr-home-row-77").is_some());
    assert!(measure(cx, win, "pr-home-row-7").is_none());
    assert!(measure(cx, win, "pr-list-refreshing").is_none());
    assert_eq!(cx.read(|cx| app.read(cx).ui().github_prs.clone()), open);

    // This is the ticker's public entry point, not a strip refresh.
    let mut refreshed = open.clone();
    refreshed[2].title = "ticker refreshed open record".into();
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(refreshed.clone())));
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    cx.run_until_parked();
    assert!(measure(cx, win, "pr-home-row-77").is_some());
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().github_prs.clone()),
        refreshed
    );

    let mut all = refreshed.clone();
    all.push(closed.clone());
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(all)));
    click_control(cx, win, "list-filter-state");
    click_control(cx, win, "list-filter-option-0-2");
    assert!(measure(cx, win, "pr-home-row-77").is_some());
    assert!(measure(cx, win, "pr-home-row-7").is_some());
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().github_prs.clone()),
        refreshed
    );

    // A departed Closed request cannot replace Graph's open evidence or
    // restore the strip collection when PR mode is entered again.
    let (task, stale_reply) = crate::evidence_support::deferred(cx);
    e2e::queue_github_pr_fetch(task);
    click_control(cx, win, "list-filter-state");
    click_control(cx, win, "list-filter-option-0-1");
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    stale_reply.send(Ok(vec![closed]));
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().github_prs.clone()),
        refreshed
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().github_pr_filter.common.state),
        StateFilter::Open
    );
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    assert!(measure(cx, win, "pr-home-row-77").is_none());
    assert!(measure(cx, win, "pr-home-row-7").is_some());
}

pub fn scenario_workspace_mode_toolbar(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, win) = mount(cx, &repo);

    // A non-default width must be shared, not merely equal by coincidence.
    app.update(cx, |app, cx| {
        app.sidebar.width = 287.0;
        // Composer-only coverage supplies read evidence; no real gh repo lookup.
        app.seed_issue_composer_for_e2e(cx);
        cx.notify();
    });
    let mut graph_nav_width = None;
    let steps: [(
        &str,
        fn(&mut kagi::ui::KagiApp, &mut gpui::Context<kagi::ui::KagiApp>),
    ); 6] = [
        ("graph", |_, _| {}),
        ("prs", |app, cx| app.show_pr_mode(cx)),
        ("issues", |app, cx| app.show_issues_mode(cx)),
        ("editor", |app, cx| app.show_editor_mode(cx)),
        ("analyze", |app, cx| app.open_ecosystem_view(cx)),
        ("graph again", |app, cx| app.show_graph_mode(cx)),
    ];
    for (name, enter) in steps {
        app.update(cx, |app, cx| enter(app, cx));
        let mode = cx.read(|cx| app.read(cx).workspace_mode());
        let is_graph = mode == WorkspaceMode::Graph;
        assert_eq!(is_graph, name.starts_with("graph"), "{name}: mode {mode:?}");
        e2e::clear_control_bounds(win.window_id(), "sidebar-mode-nav");
        assert_eq!(
            repo_actions_drawn(cx, win),
            is_graph,
            "{name} ({mode:?}): repo actions drawn only in Graph"
        );
        if matches!(
            mode,
            WorkspaceMode::Graph | WorkspaceMode::Prs | WorkspaceMode::Issues
        ) {
            let bounds = e2e::control_bounds(win.window_id(), "sidebar-mode-nav")
                .expect("mode navigation is drawn");
            let width = f32::from(bounds.size.width);
            let expected = *graph_nav_width.get_or_insert(width);
            assert!(width > 200.0, "navigation must fill the sidebar");
            assert!(
                (width - expected).abs() < 1.0,
                "{name}: {width} != {expected}"
            );
        }
    }

    // The PR workspace keeps mode navigation in the fixed-width left column
    // and gives every remaining pixel to its centre: there is no outer right
    // rail at all any more (ADR-0200), with or without a PR selected.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    for control in [
        "pr-mode-left-pane",
        "pr-mode-center-pane",
        "sidebar-mode-nav",
    ] {
        e2e::clear_control_bounds(win.window_id(), control);
    }
    assert!(!repo_actions_drawn(cx, win));
    let left =
        e2e::control_bounds(win.window_id(), "pr-mode-left-pane").expect("PR list column is drawn");
    let center =
        e2e::control_bounds(win.window_id(), "pr-mode-center-pane").expect("PR center is drawn");
    let nav = e2e::control_bounds(win.window_id(), "sidebar-mode-nav")
        .expect("PR mode navigation is drawn");
    let swipe_position = nav.center();
    assert!(
        f32::from(nav.origin.x) >= f32::from(left.origin.x)
            && f32::from(nav.origin.x + nav.size.width)
                <= f32::from(left.origin.x + left.size.width),
        "mode navigation must stay inside the PR list column"
    );
    assert!(
        f32::from(center.origin.x) >= f32::from(left.origin.x + left.size.width),
        "PR center must follow the fixed left column"
    );
    assert!(
        f32::from(center.size.width) > f32::from(left.size.width),
        "dashboard must receive the remaining workspace width"
    );
    let viewport = cx
        .update_window(win, |_, window, _| window.viewport_size())
        .unwrap();
    assert!(
        f32::from(viewport.width) - f32::from(center.origin.x + center.size.width) < 2.0,
        "the PR body must reach the window's right edge: no outer stack/files rail"
    );

    // Entering Issues starts its list request at the UI boundary, before any
    // completion can land. The takeover owns its sidebar + main pane and exposes the
    // loading state rather than borrowing the graph or PR workspace.
    app.update(cx, |app, cx| app.show_issues_mode(cx));
    for control in [
        "issue-mode-left-pane",
        "issue-mode-center-pane",
        "issue-mode-list-loading",
    ] {
        e2e::clear_control_bounds(win.window_id(), control);
    }
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Issues
    );
    for control in [
        "issue-mode-left-pane",
        "issue-mode-center-pane",
        "issue-mode-list-loading",
    ] {
        assert!(
            e2e::control_bounds(win.window_id(), control).is_some(),
            "{control} must be visible when Issues opens"
        );
    }
    assert!(
        e2e::control_bounds(win.window_id(), "issue-mode-right-pane").is_none(),
        "Issues follows the PR two-pane structure; no duplicate metadata rail"
    );

    app.update(cx, |app, cx| app.show_empty_issues_for_e2e(cx));
    e2e::clear_control_bounds(win.window_id(), "issue-mode-list-empty");
    e2e::clear_control_bounds(win.window_id(), "issue-main-list-empty");
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        e2e::control_bounds(win.window_id(), "issue-mode-list-empty").is_some(),
        "successful empty Issue slice must have a visible state"
    );
    assert!(
        e2e::control_bounds(win.window_id(), "issue-main-list-empty").is_some(),
        "the empty Issue home must keep its list empty state below Composer"
    );

    // Navigator tabs select a collection; the shared strip further filters
    // that collection in both the sidebar and the main viewport.
    app.update(cx, |app, cx| app.seed_issue_navigation_for_e2e(cx));
    for index in 0..4 {
        assert!(
            measure(cx, win, &format!("issue-filter-tab-{index}")).is_some(),
            "Issue filter {index} must be visible"
        );
    }
    assert!(measure(cx, win, "issue-mode-card-1").is_some());
    assert!(measure(cx, win, "issue-mode-card-2").is_some());
    assert!(measure(cx, win, "issue-composer").is_some());
    assert!(measure(cx, win, "issue-main-list").is_some());
    let original_lang = i18n::lang();
    let alternate_lang = match original_lang {
        Lang::En => Lang::Ja,
        Lang::Ja => Lang::En,
    };
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_placeholder_lang_for_e2e(None)),
        Some(original_lang),
        "the persistent Composer InputState must record its initial placeholder language"
    );
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.set_issue_input_lang_for_e2e(alternate_lang, window, cx)
        });
    })
    .unwrap();
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_placeholder_lang_for_e2e(None)),
        Some(alternate_lang),
        "a live language switch must replace the existing InputState placeholder"
    );
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.set_issue_input_lang_for_e2e(original_lang, window, cx)
        });
    })
    .unwrap();
    assert!(measure(cx, win, "issue-main-row-1").is_some());
    assert!(measure(cx, win, "issue-main-row-2").is_some());

    let created_tab = measure(cx, win, "issue-filter-tab-1").expect("Created by me tab");
    cx.simulate_click(win, created_tab.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(measure(cx, win, "issue-mode-card-1").is_none());
    assert!(measure(cx, win, "issue-mode-card-2").is_some());
    assert!(
        measure(cx, win, "issue-main-row-1").is_none()
            && measure(cx, win, "issue-main-row-2").is_some(),
        "the main feed and sidebar must share the selected collection"
    );
    click_control(cx, win, "issue-filter-tab-3");
    assert_list_filters(cx, win, "issue-main-row", &[1, 2, 3, 4], &[1, 3], 3, 1);

    // Click the first (newest) main row. It is guaranteed to be in the center
    // viewport even when the compact Composer grows; lower rows are reachable
    // through the center pane's production scrollbar.
    let center = measure(cx, win, "issue-mode-center-pane").expect("Issues main pane");
    let main_recent = measure(cx, win, "issue-main-row-4").expect("visible main Issue row");
    assert!(
        main_recent.origin.y >= center.origin.y
            && main_recent.origin.y + main_recent.size.height
                <= center.origin.y + center.size.height,
        "the newest main Issue row must be inside the clickable center viewport"
    );
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_issue_title_for_e2e(window, cx));
    })
    .unwrap();
    assert!(
        cx.update_window(win, |_, window, cx| {
            app.read(cx).issue_inputs_focused_for_e2e(None, window, cx)
        })
        .unwrap(),
        "the production New Issue input must own focus before row selection"
    );
    cx.simulate_click(win, main_recent.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).selected_issue_for_e2e()),
        Some(4),
        "the main row click handler must update the session-owned selection"
    );
    assert!(measure(cx, win, "issue-thread").is_some());
    assert!(measure(cx, win, "issue-thread-back").is_some());
    assert!(measure(cx, win, "issue-main-list").is_none());
    assert!(
        measure(cx, win, "issue-composer").is_none(),
        "selecting an Issue must hide the New Issue Composer"
    );
    assert!(
        measure(cx, win, "issue-reply-composer").is_some(),
        "the selected Thread must keep its Reply Composer"
    );
    assert!(
        !cx.update_window(win, |_, window, cx| {
            app.read(cx).issue_inputs_focused_for_e2e(None, window, cx)
        })
        .unwrap(),
        "the hidden New Issue inputs must release focus after row selection"
    );
    assert!(
        measure(cx, win, "issue-mode-card-2").is_some(),
        "the sidebar remains while the Thread is shown"
    );

    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.focus_issue_reply_input_for_e2e(4, window, cx)
        });
    })
    .unwrap();
    assert!(
        cx.update_window(win, |_, window, cx| {
            app.read(cx)
                .issue_inputs_focused_for_e2e(Some(4), window, cx)
        })
        .unwrap(),
        "the production Reply input must own focus before returning home"
    );
    let back = measure(cx, win, "issue-thread-back").expect("Issues home control");
    cx.simulate_click(win, back.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(measure(cx, win, "issue-thread").is_none());
    assert!(measure(cx, win, "issue-main-list").is_some());
    assert!(measure(cx, win, "issue-main-row-1").is_some());
    assert!(measure(cx, win, "issue-main-row-4").is_some());
    assert!(measure(cx, win, "issue-composer").is_some());
    assert!(
        measure(cx, win, "issue-reply-composer").is_none(),
        "returning home must hide the Reply Composer"
    );
    assert!(
        !cx.update_window(win, |_, window, cx| {
            app.read(cx)
                .issue_inputs_focused_for_e2e(Some(4), window, cx)
        })
        .unwrap(),
        "the hidden Reply input must release focus after returning home"
    );
    assert_eq!(cx.read(|cx| app.read(cx).selected_issue_for_e2e()), None);

    // The compact Composer has one stable mode toggle: eye in edit mode and
    // square-pen in preview mode. The old two-button controls must not coexist.
    assert!(measure(cx, win, "issue-composer-mode-toggle").is_some());
    assert!(measure(cx, win, "issue-composer-write").is_none());
    assert!(measure(cx, win, "issue-composer-preview").is_none());
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.focus_issue_title_for_e2e(window, cx);
        });
    })
    .unwrap();
    cx.simulate_keystrokes(win, "enter");
    cx.run_until_parked();
    let (body_revealed, body_focused, title_value, body_value) = cx
        .update_window(win, |_, window, cx| {
            app.read(cx).issue_composer_enter_state_for_e2e(window, cx)
        })
        .unwrap();
    assert!(body_revealed, "plain Enter must reveal the body editor");
    assert!(body_focused, "plain Enter must focus the body editor");
    assert!(
        title_value.is_empty(),
        "plain Enter must not insert a newline in the title InputState: {title_value:?}"
    );
    assert!(
        body_value.is_empty(),
        "the title Enter action must not fall through into the body InputState: {body_value:?}"
    );
    let (empty_draft, _) = cx.read(|cx| app.read(cx).issue_composer_snapshot_for_e2e());
    assert!(
        empty_draft.title.is_empty() && empty_draft.body.is_empty(),
        "Enter must not insert a newline or otherwise edit the empty draft"
    );

    // Editor changes must reach the actual session-owned draft subscription.
    // Do not set draft.body directly: that would hide a missing Change listener.
    let compact = measure(cx, win, "issue-composer").expect("Composer is always visible");
    let original = "USB が復帰しない\n再現条件を調べる\n";
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.insert_issue_body_for_e2e(original, window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    let (draft, focused) = cx.read(|cx| app.read(cx).issue_composer_snapshot_for_e2e());
    assert_eq!(draft.body, original);
    assert!(
        draft.title.is_empty(),
        "default title is a projection, not an edit"
    );
    assert_eq!(draft.effective_title(), "USB が復帰しない");
    assert!(!focused);

    let mode_toggle = measure(cx, win, "issue-composer-mode-toggle").expect("Composer mode toggle");
    cx.simulate_click(win, mode_toggle.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).issue_preview_for_e2e()));
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_composer_snapshot_for_e2e().0.body),
        original
    );
    assert!(
        measure(cx, win, "issue-composer-mode-toggle").is_some(),
        "preview mode must keep the same single toggle control"
    );
    assert!(measure(cx, win, "issue-composer-write").is_none());
    assert!(measure(cx, win, "issue-composer-preview").is_none());
    let mode_toggle = measure(cx, win, "issue-composer-mode-toggle").expect("Composer mode toggle");
    cx.simulate_click(win, mode_toggle.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(!cx.read(|cx| app.read(cx).issue_preview_for_e2e()));
    // Focus the existing source input again without changing its text/history.
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| app.insert_issue_body_for_e2e("", window, cx));
    })
    .unwrap();

    // Dispatch the real registered shortcut under the focused Input context.
    // This proves it reaches Composer rather than Input Enter/modal checkout.
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.simulate_keystrokes(win, "secondary-shift-enter");
    let expanded = measure(cx, win, "issue-composer").expect("focused Composer is visible");
    assert!(cx.read(|cx| app.read(cx).issue_composer_snapshot_for_e2e().1));
    assert!(
        expanded.size.height > compact.size.height + gpui::px(100.),
        "Focus Editor must expand the real rendered source editor"
    );

    // Paste is dispatched through Input's action; the wrapper must capture it
    // before Input consumes it. The clipboard itself must remain untouched.
    // Input's undo grouping uses wall-clock instant, not GPUI's test clock.
    // Separate the earlier typing from this paste as two editing gestures.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let pasted = "fn reproduce() {\n    reconnect();\n}";
    app.update(cx, |_, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(pasted.into()));
    });
    cx.update_window(win, |_, window, cx| {
        window.dispatch_action(Box::new(gpui_component::input::Paste), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let (draft, _) = cx.read(|cx| app.read(cx).issue_composer_snapshot_for_e2e());
    assert_eq!(
        draft.body,
        format!("{original}```rust\n{pasted}\n```\n"),
        "multiline paste must be fenced and inserted at the existing cursor"
    );
    app.update(cx, |_, cx| {
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some(pasted)
        );
    });
    cx.update_window(win, |_, window, cx| {
        window.dispatch_action(Box::new(gpui_component::input::Undo), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_composer_snapshot_for_e2e().0.body),
        original,
        "one Undo must remove the paste without erasing the earlier draft"
    );
    cx.simulate_keystrokes(win, "secondary-shift-enter");
    assert!(!cx.read(|cx| app.read(cx).issue_composer_snapshot_for_e2e().1));

    // Focus mode belongs to whichever Composer owns the focused input. A
    // Reply must hide the New Issue Composer and thread just as New Issue does.
    app.update(cx, |app, cx| app.seed_issue_reply_for_e2e(7, cx));
    measure(cx, win, "issue-reply-composer").expect("Reply Composer is visible");
    let reply_mode_toggle = measure(cx, win, "issue-reply-mode-toggle").expect("Reply mode toggle");
    assert!(measure(cx, win, "issue-reply-write").is_none());
    assert!(measure(cx, win, "issue-reply-preview").is_none());
    cx.simulate_click(win, reply_mode_toggle.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).issue_reply_preview_for_e2e(7)));
    let reply_mode_toggle = measure(cx, win, "issue-reply-mode-toggle")
        .expect("Reply mode toggle stays stable in Preview");
    cx.simulate_click(win, reply_mode_toggle.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(!cx.read(|cx| app.read(cx).issue_reply_preview_for_e2e(7)));
    assert!(
        !cx.read(|cx| app.read(cx).issue_reply_has_title_input_for_e2e(7)),
        "Reply must not allocate the New Issue-only title InputState"
    );
    let reply = "Reply body still follows InputState::Change";
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.insert_issue_reply_body_for_e2e(7, reply, window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    let reply_draft = cx.read(|cx| app.read(cx).issue_reply_draft_for_e2e(7));
    assert_eq!(reply_draft.body, reply);
    assert!(reply_draft.title.is_empty());
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.focus_issue_reply_input_for_e2e(7, window, cx)
        });
    })
    .unwrap();
    cx.simulate_keystrokes(win, "secondary-shift-enter");
    assert!(cx.read(|cx| app.read(cx).issue_reply_focused_for_e2e(7)));
    assert!(
        measure(cx, win, "issue-reply-composer").is_some(),
        "the focused Reply stays visible"
    );
    assert!(
        measure(cx, win, "issue-composer").is_none(),
        "Reply focus must hide the New Issue Composer"
    );
    // Selecting another row must exit #7's focus before metadata and the
    // eventual Reply destination move to #8.
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| app.select_issue_for_e2e(8, window, cx));
    })
    .unwrap();
    assert!(!cx.read(|cx| app.read(cx).issue_reply_focused_for_e2e(7)));
    assert_eq!(cx.read(|cx| app.read(cx).selected_issue_for_e2e()), Some(8));
    assert!(measure(cx, win, "issue-composer").is_none());
    assert!(measure(cx, win, "issue-reply-composer").is_none());

    // A write completion still consumes the exact sent draft after the user
    // leaves Issues, but must not let its follow-up refresh reopen the mode.
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Graph
    );
    app.update(cx, |app, cx| app.settle_issue_write_for_e2e(cx));
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Graph,
        "settling a hidden Issues write must not reopen Issues"
    );
    let settled_draft = cx.read(|cx| app.read(cx).issue_composer_snapshot_for_e2e().0);
    assert!(
        settled_draft.title.is_empty() && settled_draft.body.is_empty(),
        "the posted draft must still be consumed while Issues is hidden"
    );

    // ── Sidebar gesture navigation (ADR-0199) ─────────────────────
    // A gesture slides the sidebar's pages and nothing else: the main pane
    // neither moves nor changes content until the sidebar has settled.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    let left_before = measure(cx, win, "pr-mode-left-pane").expect("PR list column is drawn");
    let center_before = measure(cx, win, "pr-mode-center-pane").expect("PR center is drawn");
    swipe_phase(cx, win, swipe_position, 70.0, gpui::TouchPhase::Started);
    let left_mid = measure(cx, win, "pr-mode-left-pane").expect("sidebar shell stays drawn");
    let center_mid = measure(cx, win, "pr-mode-center-pane").expect("PR center stays drawn");
    // Graph is local Git data, flattened every frame, so the page this gesture
    // heads for is previewed as the real navigator — never as a shell.
    e2e::clear_control_bounds(win.window_id(), "sidebar-adjacent-page-shell");
    let adjacent =
        measure(cx, win, "sidebar-adjacent-page").expect("the adjacent page follows it in");
    assert!(
        e2e::control_bounds(win.window_id(), "sidebar-adjacent-page-shell").is_none(),
        "an already-loaded page is previewed with its own content"
    );
    assert!(
        measure(cx, win, "sidebar-gesture-shield").is_some(),
        "the gesture must own the wheel, so the page under it cannot scroll"
    );
    let offset = cx.read(|cx| e2e::sidebar_page_offset(app.read(cx)));
    assert_eq!(
        (center_before.origin.x, center_before.size.width),
        (center_mid.origin.x, center_mid.size.width),
        "the main pane must not move while the sidebar is dragged"
    );
    assert_eq!(
        (left_before.origin.x, left_before.size.width),
        (left_mid.origin.x, left_mid.size.width),
        "the sidebar shell is fixed; only the pages inside it slide"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "the workspace must not change during a gesture"
    );
    assert!(
        offset > 0.0 && offset < 70.0,
        "the resisted sidebar offset ({offset}px) must trail the 70px gesture"
    );
    assert!(
        offset < f32::from(left_mid.size.width),
        "one gesture may never carry the sidebar past one page"
    );
    assert_eq!(
        adjacent.size.width, left_before.size.width,
        "a sliding page is translated, never re-laid out narrower"
    );

    swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "the committed page must wait for the sidebar to settle"
    );
    settle_sidebar(cx);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Graph
    );
    assert!(
        measure(cx, win, "sidebar-adjacent-page").is_none(),
        "a settled sidebar shows one page at offset 0"
    );
    assert!(
        measure(cx, win, "sidebar-gesture-shield").is_none(),
        "a settled sidebar hands the wheel back to the page"
    );

    // A GitHub page has a shell only until its list has arrived.
    // Proof that the doubly-guarded PR half below actually ran.
    let mut pr_section_ran = false;
    if kagi_git::github::gh_available() {
        app.update(cx, |app, cx| app.show_graph_mode(cx));
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        assert!(
            measure(cx, win, "sidebar-adjacent-page-shell").is_some(),
            "an unloaded PR page has nothing to preview but its shape"
        );
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);

        // Cache one PR through the real fetch path, then gesture again.
        app.update(cx, |app, cx| app.show_graph_mode(cx));
        e2e::queue_github_pr_fetch(cx.background_executor.spawn(async move {
            Ok(vec![
                kagi_domain::github::PullRequest {
                    state: kagi_domain::github::IssueState::Open,
                    labels: vec![kagi_domain::github::IssueLabel {
                        name: "bug".into(),
                        color: "aabbcc".into(),
                        description: String::new(),
                    }],
                    updated_at: "2026-09-01T00:00:00Z".into(),
                    created_at: "2026-09-03T00:00:00Z".into(),
                    // One check, so the page's checks card exists to fold and
                    // unfold (mock 7a/7b).
                    checks: vec![kagi_domain::github::Check {
                        name: "build".into(),
                        workflow: "ci".into(),
                        state: kagi_domain::github::CiState::Success,
                        url: "https://example.com/run/1".into(),
                    }],
                    // `main` is the fixture's only branch, and a head the repository
                    // actually has is what lets the PR open a tab at all - which is
                    // what the feed assertions below need.
                    ..pull_request(7, "cached", "main")
                },
                kagi_domain::github::PullRequest {
                    state: kagi_domain::github::IssueState::Open,
                    labels: vec![kagi_domain::github::IssueLabel {
                        name: "docs".into(),
                        color: "aabbcc".into(),
                        description: String::new(),
                    }],
                    updated_at: "2026-09-02T00:00:00Z".into(),
                    created_at: "2026-09-02T00:00:00Z".into(),
                    ..pull_request(8, "documentation", "main")
                },
                kagi_domain::github::PullRequest {
                    state: kagi_domain::github::IssueState::Open,
                    labels: vec![kagi_domain::github::IssueLabel {
                        name: "bug".into(),
                        color: "aabbcc".into(),
                        description: String::new(),
                    }],
                    updated_at: "2026-09-03T00:00:00Z".into(),
                    created_at: "2026-09-01T00:00:00Z".into(),
                    ..pull_request(9, "repair", "main")
                },
            ])
        }));
        app.update(cx, |app, cx| app.refresh_github_prs(cx));
        cx.run_until_parked();
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        // Forget the shell the *previous* gesture drew, so this frame decides.
        e2e::clear_control_bounds(win.window_id(), "sidebar-adjacent-page-shell");
        assert!(
            measure(cx, win, "sidebar-adjacent-page").is_some(),
            "the PR page still slides in"
        );
        assert!(
            e2e::control_bounds(win.window_id(), "sidebar-adjacent-page-shell").is_none(),
            "a cached PR list must be shown instead of the shell"
        );
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Prs
        );
        assert_list_filters(cx, win, "pr-home-row", &[7, 8, 9], &[7, 9], 9, 7);
        assert_pr_state_isolation(cx, win, &app);

        // ADR-0200: the lane pane belongs to the PR on screen. Home keeps its
        // tabs, so a pane gated on "any tab open" stood there with the lanes
        // of the PR just left (user report).
        // A PR tab only opens against branches the repository has actually
        // fetched, so give the fixture the remote-tracking ref its PR names.
        crate::macos::git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        app.update(cx, |app, cx| app.reload(cx));
        cx.run_until_parked();
        let cached = cx.read(|cx| app.read(cx).ui().github_prs.first().cloned());
        if let Some(pr) = cached {
            // The conversation lands through the injected read: two reviews,
            // one issue comment and one line comment carrying both a
            // ```suggestion fence and a diff hunk, which must become four
            // entries of the page's list. The first virtualized feed
            // flattened the entries *before* assigning what had landed and
            // showed none (review finding, w5:p19). The line comment sorts
            // first, so #750's shared row chrome has to keep its hunk and its
            // suggestion marker on the entry the page always draws.
            e2e::queue_github_pr_conversation(cx.background_executor.spawn(async move {
                use kagi_domain::github::{Comment, Review, ReviewComment};
                (
                    Ok((
                        vec![
                            Review {
                                author: "alice".into(),
                                state: "APPROVED".into(),
                                body: "looks good".into(),
                                submitted_at: "2026-09-01T00:00:00Z".into(),
                            },
                            Review {
                                author: "bob".into(),
                                state: "COMMENTED".into(),
                                body: "one nit".into(),
                                submitted_at: "2026-09-02T00:00:00Z".into(),
                            },
                        ],
                        vec![Comment {
                            author: "carol".into(),
                            body: "thanks".into(),
                            created_at: "2026-09-03T00:00:00Z".into(),
                        }],
                    )),
                    Ok(vec![ReviewComment {
                        author: "copilot".into(),
                        path: "src/lib.rs".into(),
                        line: 12,
                        start_line: None,
                        body: "prefer the helper\n\n```suggestion\nlet x = helper();\n```\n".into(),
                        diff_hunk: "@@ -10,3 +10,3 @@\n-let x = 1;\n+let x = 2;\n context".into(),
                        created_at: "2026-08-31T00:00:00Z".into(),
                        in_reply_to: None,
                    }]),
                )
            }));
            app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
            cx.run_until_parked();
            cx.read(|cx| {
                let m = app.read(cx).pr_mode().expect("PR mode");
                let tab = &m.tabs[m.active.unwrap()];
                assert!(tab.conversation_loaded, "the injected conversation landed");
                assert_eq!(
                    tab.feed_entries.len(),
                    4,
                    "every review, comment and line comment that landed is an entry of the page"
                );
            });
            // ADR-0200: 概要 and レビュー are two anchors into ONE virtualized
            // list, and the tabs are navigation into it. Each tab must draw
            // its own anchor, and both must be looking at the same list - the
            // same item count - where a tab that swapped the body would give
            // each view a list of its own. (The other anchor may legitimately
            // be off screen: that is what virtualization means.)
            let mut counts = Vec::new();
            for (view, anchor) in [
                (kagi::ui::pr_mode::PrView::Overview, "pr-mode-headline"),
                (kagi::ui::pr_mode::PrView::Review, "pr-feed-review"),
            ] {
                app.update(cx, |app, cx| app.pr_mode_show(view, cx));
                e2e::clear_control_bounds(win.window_id(), anchor);
                // A virtualized list measures its items on one frame and lays
                // them out on the next, so two draws are what "the page is on
                // screen" means here.
                for _ in 0..2 {
                    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                        .unwrap();
                }
                assert!(
                    measure(cx, win, anchor).is_some(),
                    "{view:?} must reveal its own anchor on the page"
                );
                counts.push(cx.read(|cx| {
                    let m = app.read(cx).pr_mode().expect("PR mode");
                    m.tabs[m.active.unwrap()].feed_list.item_count()
                }));
            }
            assert!(
                counts[0] == counts[1] && counts[0] > 2,
                "both tabs read one list: {counts:?}"
            );
            // The レビュー tab is on screen: the entries under its heading
            // must actually be drawn, not merely counted (user report: "PR
            // reviews are not shown").
            for _ in 0..2 {
                cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                    .unwrap();
            }
            // The line comment sorts first, so entry 0 is the one that must be
            // under the heading the レビュー tab just revealed.
            for id in [
                "pr-feed-entry-0",
                "pr-convo-hunk-7000",
                "pr-convo-suggestion-7000",
            ] {
                e2e::clear_control_bounds(win.window_id(), id);
            }
            for _ in 0..2 {
                cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                    .unwrap();
            }
            assert!(
                e2e::control_bounds(win.window_id(), "pr-feed-entry-0").is_some(),
                "the first entry under the heading is drawn"
            );
            // #750: moving the entry onto the shared row must not drop what
            // only a PR entry carries — the diff hunk above the body and the
            // ```suggestion marker on the meta line are drawn with it.
            assert!(
                e2e::control_bounds(win.window_id(), "pr-convo-hunk-7000").is_some(),
                "the line comment's diff hunk is drawn inside the shared row"
            );
            assert!(
                e2e::control_bounds(win.window_id(), "pr-convo-suggestion-7000").is_some(),
                "the suggestion marker rides on the shared row's meta line"
            );
            // ...and the heading sits at the top of the page, not at its
            // bottom edge with the reviews below the fold: pressing レビュー
            // must show reviews, which is the whole point of the tab.
            let heading = e2e::control_bounds(win.window_id(), "pr-feed-review")
                .expect("the heading is drawn after the レビュー tab");
            let pane = e2e::control_bounds(win.window_id(), "pr-mode-center-pane")
                .expect("the centre pane is measured");
            let from_top = f32::from(heading.origin.y) - f32::from(pane.origin.y);
            assert!(
                from_top < f32::from(pane.size.height) / 2.0,
                "the レビュー tab must put its heading in the upper half of the page, got {from_top}px from the top of a {}px pane",
                f32::from(pane.size.height)
            );

            // mock 7a/7b: the checks card is folded on the page, and opens to
            // the per-check rows in place. The fixture PR carries one check,
            // so the card exists and the disclosure must change the pane's
            // height rather than open a second surface.
            //
            // The card is near the top of the virtualized page and the レビュー
            // jump above left the feed at its conversation: go back to 概要 and
            // let the list lay out, or there is nothing on screen to measure.
            app.update(cx, |app, cx| {
                app.pr_mode_show(kagi::ui::pr_mode::PrView::Overview, cx)
            });
            e2e::clear_control_bounds(win.window_id(), "pr-mode-checks");
            for _ in 0..2 {
                cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                    .unwrap();
            }
            let folded = measure(cx, win, "pr-mode-checks").expect("the checks card is drawn");
            app.update(cx, |app, cx| app.pr_mode_toggle_checks(cx));
            e2e::clear_control_bounds(win.window_id(), "pr-mode-checks");
            cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            let opened = measure(cx, win, "pr-mode-checks").expect("the checks card stays drawn");
            assert!(
                f32::from(opened.size.height) > f32::from(folded.size.height),
                "opening the checks card must reveal its rows in place"
            );
            app.update(cx, |app, cx| app.pr_mode_toggle_checks(cx));

            // ADR-0200 §11: the gear on a properties row opens the field
            // picker with what the PR already carries, before any read of
            // what the repository offers has returned - so a value can be
            // removed offline. Confirm is dead until something changed.
            app.update(cx, |app, cx| {
                app.open_pr_fields_modal(kagi::ui::modals::PrField::Reviewers, cx)
            });
            cx.read(|cx| {
                let modal = app
                    .read(cx)
                    .pr_fields_modal()
                    .cloned()
                    .expect("the picker is the active modal");
                assert_eq!(modal.number, 7);
                assert_eq!(
                    modal.selected, modal.current,
                    "opens on the PR's own values"
                );
            });
            app.update(cx, |app, cx| app.pr_fields_toggle("octocat".into(), cx));
            cx.read(|cx| {
                let modal = app.read(cx).pr_fields_modal().cloned().unwrap();
                assert!(modal.selected.contains(&"octocat".to_string()));
                let (add, remove) =
                    kagi_domain::github::PrFieldEdit::diff(&modal.current, &modal.selected);
                assert_eq!(add, vec!["octocat".to_string()]);
                assert!(remove.is_empty());
            });
            app.update(cx, |app, cx| {
                app.clear_pr_fields_modal();
                cx.notify();
            });
            cx.read(|cx| assert!(app.read(cx).pr_fields_modal().is_none()));

            // #750: the composer is the Issues chrome — one box, one toggle,
            // an amber POST that is only live when it can be pressed.
            assert!(
                measure(cx, win, "pr-mode-composer").is_some(),
                "the PR page pins its composer"
            );
            assert!(
                measure(cx, win, "pr-composer-mode-toggle").is_some(),
                "edit/preview is one control, like the Issues composer"
            );
            // Type into the composer's own box, then drive the toggle the way
            // the reader does: a click on the control that is on screen. What
            // is written must survive the trip to the preview and back — the
            // preview reads the live box, it does not replace it.
            let typed = "hunk を直す\n\n```suggestion\nlet x = helper();\n```\n";
            cx.update_window(win, |_, window, cx| {
                app.update(cx, |app, cx| {
                    let input = app
                        .pr_comment_input
                        .clone()
                        .expect("the composer's box exists once it has been drawn");
                    input.update(cx, |state, cx| {
                        state.focus(window, cx);
                        state.replace(typed.to_owned(), window, cx);
                    });
                });
            })
            .unwrap();
            cx.run_until_parked();
            let composer_text = |cx: &mut VisualTestAppContext| {
                cx.read(|cx| {
                    app.read(cx)
                        .pr_comment_input
                        .as_ref()
                        .expect("the composer's box")
                        .read(cx)
                        .value()
                        .to_string()
                })
            };
            assert_eq!(composer_text(cx), typed, "the box holds what was typed");
            assert!(
                measure(cx, win, "pr-composer-preview").is_none(),
                "the composer opens on its box, not on its preview"
            );
            let toggle = measure(cx, win, "pr-composer-mode-toggle")
                .expect("the single edit/preview control");
            cx.simulate_click(win, toggle.center(), gpui::Modifiers::none());
            cx.run_until_parked();
            e2e::clear_control_bounds(win.window_id(), "pr-composer-mode-toggle");
            cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            assert!(
                measure(cx, win, "pr-composer-preview").is_some(),
                "clicking the toggle shows the typed text as markdown"
            );
            assert!(
                measure(cx, win, "pr-composer-mode-toggle").is_some(),
                "preview keeps the same single toggle control"
            );
            let toggle = measure(cx, win, "pr-composer-mode-toggle")
                .expect("the toggle is still the way back");
            cx.simulate_click(win, toggle.center(), gpui::Modifiers::none());
            cx.run_until_parked();
            e2e::clear_control_bounds(win.window_id(), "pr-composer-preview");
            cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            assert!(
                e2e::control_bounds(win.window_id(), "pr-composer-preview").is_none(),
                "the second click gives the box back"
            );
            assert_eq!(
                composer_text(cx),
                typed,
                "a preview round trip must not touch what was written"
            );

            // #750 review: the preview belongs to the composer, and the
            // composer follows the PR on screen. Leaving it on while another
            // PR takes the box would open that PR on a preview of a draft
            // nobody has written. Every activation path (lane, tab close,
            // home then another PR) funnels through
            // `sync_pr_comment_input`, which is where the reset lives.
            let toggle =
                measure(cx, win, "pr-composer-mode-toggle").expect("the edit/preview control");
            cx.simulate_click(win, toggle.center(), gpui::Modifiers::none());
            cx.run_until_parked();
            cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            assert!(
                measure(cx, win, "pr-composer-preview").is_some(),
                "the composer is in its preview before the switch"
            );
            e2e::queue_github_pr_conversation(
                cx.background_executor
                    .spawn(async move { (Ok((Vec::new(), Vec::new())), Ok(Vec::new())) }),
            );
            let second = pull_request(8, "second", "main");
            app.update(cx, |app, cx| app.pr_mode_open(&second, cx));
            cx.run_until_parked();
            e2e::clear_control_bounds(win.window_id(), "pr-composer-preview");
            for _ in 0..2 {
                cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                    .unwrap();
            }
            assert!(
                e2e::control_bounds(win.window_id(), "pr-composer-preview").is_none(),
                "another PR takes the box, not a preview of nothing"
            );
            assert!(
                composer_text(cx).is_empty(),
                "the new PR's composer starts on its own (empty) draft"
            );
            // ...and the first PR's text was parked, not lost: coming back
            // restores exactly what was typed.
            app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
            cx.run_until_parked();
            cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            assert_eq!(
                composer_text(cx),
                typed,
                "switching away and back keeps the draft with its own PR"
            );

            app.update(cx, |app, cx| app.pr_mode_home(cx));
            assert!(
                measure(cx, win, "pr-mode-lane-pane").is_none(),
                "back on the home list there is no PR to draw a lane for"
            );
            // The PR half of this scenario is guarded twice (gh, and a cached
            // PR). Say so in the log: a silent skip must not read as proof.
            eprintln!("[gui-e2e] workspace_mode_toolbar: PR section exercised");
            pr_section_ran = true;
        } else {
            eprintln!(
                "[gui-e2e] workspace_mode_toolbar: PR section SKIPPED (no cached PR to open)"
            );
        }
    } else {
        eprintln!("[gui-e2e] workspace_mode_toolbar: PR section SKIPPED (gh unavailable)");
    }
    assert!(
        pr_section_ran || !kagi_git::github::gh_available(),
        "with gh on PATH the PR section must run, not skip"
    );

    // Releasing under the 20% commit boundary returns to the origin page.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    swipe_phase(cx, win, swipe_position, 40.0, gpui::TouchPhase::Started);
    swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
    settle_sidebar(cx);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "40px of a 287px sidebar is under the commit boundary"
    );

    // One committed gesture moves exactly one adjacent page, however far it
    // travels. At either edge a further gesture is inert; without gh the Graph
    // edge is inert as well.
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    if kagi_git::github::gh_available() {
        swipe_phase(cx, win, swipe_position, -2000.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, -2000.0, gpui::TouchPhase::Moved);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Prs,
            "a huge gesture still moves exactly one page"
        );
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Issues
        );
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Issues,
            "right edge must not move"
        );
        swipe_phase(cx, win, swipe_position, 70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Prs
        );
    } else {
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Graph,
            "swipe must not enter a GitHub workspace without gh"
        );
    }

    // Momentum scroll arrives as Moved/Ended with no Started (macOS maps the
    // phase that way), so it must not navigate on its own.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    swipe_phase(cx, win, swipe_position, 100.0, gpui::TouchPhase::Moved);
    swipe_phase(cx, win, swipe_position, 100.0, gpui::TouchPhase::Ended);
    settle_sidebar(cx);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "momentum without a new gesture must not navigate"
    );

    // Opening a modal occludes the sidebar, so it must cancel the in-flight
    // gesture at the canonical modal transition rather than waiting for wheel
    // input that cannot reach the sidebar.
    assert!(!repo_actions_drawn(cx, win));
    swipe_phase(cx, win, swipe_position, 30.0, gpui::TouchPhase::Started);
    swipe_phase(cx, win, swipe_position, 40.0, gpui::TouchPhase::Moved);
    app.update(cx, |app, _| e2e::deliver_app_notice(app, "swipe blocker"));
    assert!(cx.read(|cx| app.read(cx).app_notice().is_some()));
    app.update(cx, |app, _| app.clear_app_notice());
    swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
    settle_sidebar(cx);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "a gesture interrupted by a modal must snap back"
    );

    assert_eq!(before, repo_fingerprint(&repo), "repo mutated");
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS workspace_mode_toolbar");
}
