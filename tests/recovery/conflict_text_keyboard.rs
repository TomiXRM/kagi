//! #1311: production resolver Tab/arrow/activation dispatch, not native VoiceOver.
use crate::app_conflict::click_control;
use crate::macos::{git, mount, repo_fingerprint, unmount};
use gpui::Entity;
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::conflict_editor_a11y as ax;
use kagi::ui::{conflict_view::ConflictView, e2e};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    theme,
};
use std::path::Path;

pub(super) fn fixture() -> tempfile::TempDir {
    fixture_for_operation(false)
}
fn fixture_for_operation(rebase: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    for side in ["base", "theirs", "ours"] {
        if side == "theirs" {
            git(dir.path(), &["checkout", "-qb", "feature"]);
        }
        if side == "ours" {
            git(dir.path(), &["checkout", "-q", "main"]);
        }
        let mut lines: Vec<String> = (1..=24).map(|n| format!("context {n}\n")).collect();
        for n in [1, 2, 3, 17, 18, 19] {
            lines[n] = format!("{side} {n}\n");
        }
        std::fs::write(dir.path().join("notes.txt"), lines.concat()).unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", side]);
    }
    if rebase {
        git(dir.path(), &["checkout", "-q", "feature"]);
    }
    assert!(!std::process::Command::new("git")
        .args(if rebase {
            ["rebase", "main"]
        } else {
            ["merge", "feature"]
        })
        .current_dir(dir.path())
        .output()
        .unwrap()
        .status
        .success());
    dir
}
fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn assert_source_hunk_visible(cx: &mut VisualTestAppContext, window: AnyWindowHandle, hunk: usize) {
    for side in ["current", "incoming"] {
        e2e::clear_control_bounds(
            window.window_id(),
            &format!("resolver-hunk-{hunk}-{side}-line-0"),
        );
    }
    paint(cx, window);
    let dimensions = cx
        .update_window(window, |_, window, _| window.viewport_size())
        .unwrap();
    for side in ["current", "incoming"] {
        let line = e2e::control_bounds(
            window.window_id(),
            &format!("resolver-hunk-{hunk}-{side}-line-0"),
        )
        .expect("selected hunk's first source line is rendered");
        let viewport =
            e2e::control_bounds(window.window_id(), &format!("resolver-{side}-viewport")).unwrap();
        assert!(
            line.top() >= viewport.top() && line.bottom() <= viewport.bottom(),
            "{side}: navigation brings hunk {hunk} inside the whole-file viewport; line={line:?}, viewport={viewport:?}, zoom={}, dimensions={dimensions:?}, lang={:?}",
            theme::zoom(), i18n::lang()
        );
    }
    assert_row_alignment(cx, window);
    for side in ["current", "incoming"] {
        let viewport =
            e2e::control_bounds(window.window_id(), &format!("resolver-{side}-viewport")).unwrap();
        for delta in [-48., 48.] {
            cx.simulate_event(
                window,
                gpui::ScrollWheelEvent {
                    position: viewport.center(),
                    delta: gpui::ScrollDelta::Pixels(gpui::point(
                        gpui::px(0.),
                        theme::scaled_px(delta),
                    )),
                    touch_phase: gpui::TouchPhase::Moved,
                    ..Default::default()
                },
            );
            paint(cx, window);
            assert_row_alignment(cx, window);
        }
    }
}

fn assert_row_alignment(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    let counts = ax::row_counts();
    assert!(counts[0] > 0);
    assert_eq!(
        counts, [counts[0]; 3],
        "both source panes and gutter have equal row counts"
    );
    for index in 0..counts[0] {
        for side in ["current", "incoming", "gutter"] {
            e2e::clear_control_bounds(window.window_id(), &format!("resolver-{side}-row-{index}"));
        }
    }
    paint(cx, window);
    let mut aligned = 0;
    for index in 0..counts[0] {
        let rows = ["current", "incoming", "gutter"].map(|side| {
            e2e::control_bounds(window.window_id(), &format!("resolver-{side}-row-{index}"))
        });
        if rows.iter().all(Option::is_none) {
            continue;
        }
        let rows = rows.map(|row| row.expect("same-index row is rendered in all three panes"));
        for row in &rows[1..] {
            assert!(
                (f32::from(row.top() - rows[0].top())).abs() <= 1.,
                "same-index row {index} is y-aligned after navigation/scroll"
            );
            assert!(
                (f32::from(row.size.height) - f32::from(theme::scaled_px(24.))).abs() <= 1.,
                "action, source and gutter rows share uniform 24px height"
            );
        }
        aligned += 1;
    }
    assert!(
        aligned > 0,
        "alignment measures visible rows, not an empty probe"
    );
}

fn assert_operation_placement(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
) {
    let ids = [
        "toolbar-row",
        "conflict-operation-strip",
        "operation-strip",
        "operation-strip-abort",
        "operation-strip-skip",
        "conflict-session-header",
        "conflict-session-identity",
        "conflict-sidebar-clip",
        "conflict-file-header",
        "conflict-sidebar-toggle",
        "conflict-operation-actions",
        "conflict-file-session-separator",
        "conflict-abort",
        "conflict-save",
        "conflict-skip",
        "conflict-skip-label",
        "conflict-continue",
        "tb-graph-mode",
        "tb-pr-mode",
        "tb-editor-ws",
        "tb-ecosystem",
        "tb-settings",
    ];
    for id in ids {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    paint(cx, window);
    let bounds = |id| {
        e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| {
            panic!(
                "missing control {id}: {:?} zoom={}",
                i18n::lang(),
                theme::zoom()
            )
        })
    };
    let contains = |outer: gpui::Bounds<gpui::Pixels>, inner: gpui::Bounds<gpui::Pixels>| {
        inner.left() >= outer.left()
            && inner.right() <= outer.right()
            && inner.top() >= outer.top()
            && inner.bottom() <= outer.bottom()
    };
    let (hidden, sequencer, identity, skip_armed) = cx.read(|cx| {
        let view = conflict.read(cx);
        let mode = view.mode.as_ref().unwrap();
        let op = &mode.session.op;
        let identity = match op {
            kagi_git::conflicts::ConflictOp::Rebase { step, total, .. } => {
                assert!(*step > 0 && *total >= *step);
                format!("{} {step}/{total}", op.slug())
            }
            op => op.slug().to_owned(),
        };
        (
            view.sidebar_hidden,
            op.is_sequencer(),
            identity,
            mode.skip_approval.is_some(),
        )
    });
    let toolbar = bounds("toolbar-row");
    let header = bounds("conflict-file-header");
    let actions = bounds("conflict-operation-actions");
    let separator = bounds("conflict-file-session-separator");
    assert!(e2e::control_bounds(window.window_id(), "conflict-operation-strip").is_none());
    assert!(
        contains(header, actions),
        "header={header:?}, actions={actions:?}, hidden={hidden}, zoom={}",
        theme::zoom()
    );
    assert!(contains(header, separator));
    assert!(bounds("conflict-save").right() <= separator.left());
    assert!(separator.right() <= actions.left());
    assert!(contains(actions, bounds("conflict-continue")));
    if hidden {
        for id in ["conflict-session-header", "conflict-abort", "conflict-skip"] {
            assert!(
                e2e::control_bounds(window.window_id(), id).is_none(),
                "{id} is not moved into the file header when the sidebar is hidden"
            );
        }
        let strip = bounds("operation-strip");
        assert!(strip.top() >= toolbar.bottom());
        assert!(strip.bottom() <= header.top());
        let abort = bounds("operation-strip-abort");
        assert!(contains(strip, abort));
        assert!(abort.size.width > gpui::px(0.) && abort.size.height > gpui::px(0.));
        assert_eq!(
            e2e::control_bounds(window.window_id(), "operation-strip-skip").is_some(),
            sequencer,
            "hidden sequencer Skip remains visible beside Abort"
        );
    } else {
        assert_eq!(ConflictView::session_header_label_for_e2e(), identity);
        assert!(e2e::control_bounds(window.window_id(), "operation-strip").is_none());
        assert!(e2e::control_bounds(window.window_id(), "operation-strip-abort").is_none());
        let session = bounds("conflict-session-header");
        assert!(contains(session, bounds("conflict-abort")));
        assert!(contains(bounds("conflict-sidebar-clip"), session));
        assert!(session.top() >= toolbar.bottom());
        assert!(
            (f32::from(session.size.height - header.size.height)).abs() <= 1.,
            "session and file headers keep one aligned 36px row"
        );
        let identity_paint =
            e2e::control_paint(window.window_id(), "conflict-session-identity").unwrap();
        assert!(!identity_paint.ellipsis && !identity_paint.middle_ellipsis);
        if sequencer && skip_armed {
            assert_eq!(
                ConflictView::session_skip_label_for_e2e(),
                Msg::ConflictConfirmSkipButton.t(),
                "the rendered armed caption is the short EN/JA confirmation verb"
            );
            let caption = e2e::control_paint(window.window_id(), "conflict-skip-label").unwrap();
            assert!(!caption.ellipsis && !caption.middle_ellipsis);
            assert!(
                contains(bounds("conflict-skip"), caption.bounds),
                "armed {:?} caption={caption:?}, button={:?}",
                i18n::lang(),
                bounds("conflict-skip")
            );
            assert!(contains(caption.mask, caption.bounds));
        }
        cx.update_window(window, |_, window, _| {
            let mut font = kagi_ui_core::fonts::ui_font();
            font.weight = gpui::FontWeight::SEMIBOLD;
            for text in [
                identity.as_str(),
                "cherry-pick 10/10",
                if sequencer && skip_armed {
                    Msg::ConflictConfirmSkipButton.t()
                } else if sequencer {
                    Msg::ConflictSkip.t()
                } else {
                    identity.as_str()
                },
            ] {
                let caption = (text != identity && text != "cherry-pick 10/10").then(|| {
                    e2e::control_paint(window.window_handle().window_id(), "conflict-skip-label")
                        .unwrap()
                });
                let mut text_font = font.clone();
                if let Some(caption) = caption {
                    text_font.weight = caption.font_weight;
                }
                let width = window
                    .text_system()
                    .shape_line(
                        text.to_owned().into(),
                        caption.map_or(theme::scaled_px(12.), |paint| paint.font_size),
                        &[gpui::TextRun {
                            len: text.len(),
                            font: text_font,
                            color: gpui::rgb(theme::theme().text_main).into(),
                            ..Default::default()
                        }],
                        None,
                    )
                    .width;
                let available = if text == identity {
                    identity_paint.bounds.size.width
                } else if text == "cherry-pick 10/10" {
                    session.size.width
                        - bounds("conflict-abort").size.width
                        - theme::scaled_px(if sequencer { 50. } else { 20. })
                } else {
                    bounds("conflict-skip-label").size.width
                };
                assert!(
                    available >= width,
                    "full operation/step fits before shrinking action labels: {text}"
                );
            }
        })
        .unwrap();
        assert!(
            session.right() <= header.left(),
            "Abort and Skip stay in the sidebar, apart from file actions and Continue"
        );
        if sequencer {
            assert!(contains(session, bounds("conflict-skip")));
            assert!(bounds("conflict-abort").right() <= bounds("conflict-skip").left());
            assert_eq!(
                bounds("conflict-abort").center().y,
                bounds("conflict-skip").center().y
            );
        } else {
            assert!(e2e::control_bounds(window.window_id(), "conflict-skip").is_none());
        }
    }
    for id in ["conflict-save", "conflict-continue"] {
        assert!(
            bounds(id).top() >= toolbar.bottom(),
            "{id} is outside the main toolbar"
        );
    }
    for id in [
        "tb-graph-mode",
        "tb-pr-mode",
        "tb-editor-ws",
        "tb-ecosystem",
        "tb-settings",
    ] {
        if let Some(mode) = e2e::control_bounds(window.window_id(), id) {
            let mode = mode.dilate(gpui::px(kagi_ui_core::keyboard_nav::RING));
            assert!(mode.size.width > gpui::px(0.), "{id} remains visible");
            assert!(mode.right() <= toolbar.right(), "{id} fits the toolbar");
        } else {
            assert!(
                e2e::control_bounds(window.window_id(), "tb-overflow").is_some(),
                "ordinary toolbar overflow retains hidden navigation: {id}"
            );
        }
    }
}

fn assert_abort_interaction_ink(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
) {
    use gpui::{Modifiers, MouseButton};
    for (hidden, control_id, label_id) in [
        (false, "conflict-abort", "conflict-abort-label"),
        (true, "operation-strip-abort", "operation-strip-abort-label"),
    ] {
        conflict.update(cx, |view, cx| {
            view.sidebar_hidden = hidden;
            cx.notify();
        });
        assert_operation_placement(cx, window, conflict);
        let button = e2e::control_bounds(window.window_id(), control_id).unwrap();
        let outside = gpui::point(button.right() + gpui::px(12.), button.center().y);
        cx.simulate_mouse_move(window, outside, None, Modifiers::none());
        paint(cx, window);
        let ink = e2e::control_paint(window.window_id(), label_id)
            .unwrap()
            .text_color;
        let base = gpui::Hsla::from(gpui::rgb(theme::theme().color_blocker));
        assert!(
            (ink.h - base.h).abs() < 0.015 && (ink.s - base.s).abs() < 0.02,
            "contrast adjustment preserves destructive hue and saturation"
        );
        let ink = u32::from(ink.to_rgb()) >> 8;
        assert_abort_paint(cx, window, control_id, label_id, ink, "rest");
        cx.simulate_mouse_move(window, button.center(), None, Modifiers::none());
        paint(cx, window);
        assert_abort_paint(cx, window, control_id, label_id, ink, "hover");
        cx.simulate_mouse_down(
            window,
            button.center(),
            MouseButton::Left,
            Modifiers::none(),
        );
        paint(cx, window);
        assert_abort_paint(cx, window, control_id, label_id, ink, "pressed");
        // Release outside: this paint test never opens or confirms Abort.
        cx.simulate_mouse_move(window, outside, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(window, outside, MouseButton::Left, Modifiers::none());
        conflict.update(cx, |view, cx| {
            view.writer_busy = true;
            cx.notify();
        });
        paint(cx, window);
        let disabled = e2e::control_paint(window.window_id(), label_id).unwrap();
        assert_ne!(
            u32::from(disabled.text_color.to_rgb()) >> 8,
            ink,
            "disabled Abort uses muted, not active destructive ink"
        );
        conflict.update(cx, |view, cx| {
            view.writer_busy = false;
            cx.notify();
        });
        paint(cx, window);
    }
    conflict.update(cx, |view, cx| {
        view.sidebar_hidden = false;
        cx.notify();
    });
    assert_operation_placement(cx, window, conflict);
}

fn assert_abort_paint(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    control_id: &str,
    label_id: &str,
    expected_ink: u32,
    state: &str,
) {
    let label = e2e::control_paint(window.window_id(), label_id).unwrap();
    let ink = u32::from(label.text_color.to_rgb()) >> 8;
    assert_eq!(
        ink, expected_ink,
        "{control_id} {state}: stable destructive ink"
    );
    assert_eq!(label.font_weight, gpui::FontWeight::BOLD);
    let button = e2e::control_bounds(window.window_id(), control_id).unwrap();
    cx.update_window(window, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native Abort density capture");
        let scale = window.scale_factor();
        let rgb = |pixel: [u8; 4]| {
            (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2])
        };
        let background = rgb(image
            .get_pixel(
                (f32::from(button.left() + gpui::px(4.)) * scale) as u32,
                (f32::from(button.center().y) * scale) as u32,
            )
            .0);
        assert!(
            theme::contrast_ratio(ink, background) >= 4.5,
            "{control_id} {state}: Abort text contrast {:.3}:1",
            theme::contrast_ratio(ink, background)
        );
        let mut strongest: f64 = 1.;
        for y in (f32::from(label.bounds.top()) * scale).ceil() as u32
            ..(f32::from(label.bounds.bottom()) * scale).floor() as u32
        {
            for x in (f32::from(label.bounds.left()) * scale).ceil() as u32
                ..(f32::from(label.bounds.right()) * scale).floor() as u32
            {
                strongest = strongest.max(theme::contrast_ratio(
                    rgb(image.get_pixel(x, y).0),
                    background,
                ));
            }
        }
        assert!(
            strongest >= 4.5,
            "{control_id} {state}: actual glyph contrast {strongest:.3}:1"
        );
    })
    .unwrap();
}

fn invoke_menu(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<kagi::ui::KagiApp>,
    command: &'static str,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.handle_menu_command(command, window, cx));
    })
    .unwrap();
    paint(cx, window);
}

fn assert_sidebar_focus_yields(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<kagi::ui::KagiApp>,
    conflict: &Entity<ConflictView>,
    toggle: &serde_json::Value,
) {
    crate::native_ax::focus_named(cx, window, "Button", i18n::Msg::ConflictAbort.t());
    invoke_menu(cx, window, app, "view.toggleSidebar");
    assert!(cx.read(|cx| conflict.read(cx).sidebar_hidden));
    assert_eq!(
        crate::native_ax::focused(cx, window)["element_id"],
        toggle["element_id"],
        "View > Hide Sidebar returns session-button focus to its visible toggle"
    );
    keys(cx, window, "enter");
    assert!(
        !cx.read(|cx| conflict.read(cx).sidebar_hidden),
        "the returned toggle remains keyboard actionable"
    );
}

fn assert_session_accessibility(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    fn has_abort(tree: &serde_json::Value, node: &serde_json::Value) -> bool {
        (node["aria"]["role"] == "Button" && node["aria"]["label"] == i18n::Msg::ConflictAbort.t())
            || node["children"].as_array().is_some_and(|children| {
                children
                    .iter()
                    .any(|id| has_abort(tree, &tree["nodes"][id.as_str().unwrap()]))
            })
    }
    let tree = crate::native_ax::tree(cx, window);
    let session = crate::native_ax::node(cx, window, "conflict-session-header".into());
    let files = crate::native_ax::node(cx, window, "conflict-tree".into());
    assert_eq!(session["aria"]["role"], "Group");
    assert_eq!(files["aria"]["role"], "ListBox");
    assert!(has_abort(&tree, &session));
    assert!(
        !has_abort(&tree, &files),
        "session controls are not file-list items"
    );
}
fn probe(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Option<String> {
    ax::take_focus_probe();
    cx.simulate_keystrokes(window, "f19");
    cx.run_until_parked();
    ax::take_focus_probe()
}
fn keys(cx: &mut VisualTestAppContext, window: AnyWindowHandle, key: &str) {
    crate::keyboard_nav::keys(cx, window, key);
    paint(cx, window);
}

fn assembled(cx: &mut VisualTestAppContext, conflict: &Entity<ConflictView>) -> String {
    cx.read(|cx| {
        conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .hunk_model(Path::new("notes.txt"))
            .unwrap()
            .assembled_text()
    })
}

fn horizontal_controls(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
    lang: Lang,
) {
    choose(cx, window, 1, "current");
    let ours_first = assembled(cx, conflict);
    assert!(ours_first.find("ours 17").unwrap() < ours_first.find("theirs 17").unwrap());
    choose(cx, window, 1, "incoming");
    let theirs_first = assembled(cx, conflict);
    assert!(theirs_first.find("theirs 17").unwrap() < theirs_first.find("ours 17").unwrap());
    let line = "resolver-hunk-1-current-line-0";
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("conflict-hunk-1-incoming-change"),
        "choosing ordered Both focuses the replacement Change action"
    );
    // Horizontal movement stays within this hunk's action row and lines.
    keys(cx, window, "left");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("conflict-hunk-1-current-change")
    );
    keys(cx, window, "right right");
    assert_eq!(probe(cx, window).as_deref(), Some(line));
    assert_eq!(
        ax::control(line).unwrap().1,
        if lang == Lang::En {
            "Use Current branch (main) line 18"
        } else {
            "Current branch (main) の 18 行目を採用"
        }
    );
    keys(cx, window, "enter");
    cx.read(|cx| {
        assert_eq!(
            conflict
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .hunk_model(Path::new("notes.txt"))
                .unwrap()
                .file_side_state(kagi_git::resolution::SelectionSide::Current),
            kagi_git::resolution::TriState::Partial
        );
    });
    assert!(!assembled(cx, conflict).contains("ours 17"));
    let bounds = e2e::control_bounds(window.window_id(), line).unwrap();
    let viewport = e2e::control_bounds(window.window_id(), "resolver-current-viewport").unwrap();
    assert!(
        bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom(),
        "focused line is inside the resolver viewport"
    );
    conflict.update(cx, |view, cx| {
        let buffer = &mut view.mode.as_mut().unwrap().buffer;
        let after = buffer.resolved_text(Path::new("notes.txt"));
        assert!(buffer.undo(Path::new("notes.txt")));
        assert_ne!(buffer.resolved_text(Path::new("notes.txt")), after);
        assert!(buffer.redo(Path::new("notes.txt")));
        assert_eq!(buffer.resolved_text(Path::new("notes.txt")), after);
        cx.notify();
    });
    keys(cx, window, "up");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("conflict-hunk-1-current-change"),
        "line editing and undo/redo retain the same hunk's keyboard owner"
    );
}

fn choose(cx: &mut VisualTestAppContext, window: AnyWindowHandle, hunk: usize, side: &str) {
    let change = format!("conflict-hunk-{hunk}-current-change");
    if ax::control(&change).is_some() {
        if probe(cx, window).as_deref()
            == Some(format!("conflict-hunk-{hunk}-incoming-change").as_str())
        {
            keys(cx, window, "left");
        }
        tab_to(cx, window, &change);
        keys(cx, window, "enter");
    }
    tab_to(cx, window, &format!("conflict-hunk-{hunk}-{side}-use"));
    keys(cx, window, "right");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some(format!("conflict-hunk-{hunk}-{side}-both").as_str()),
        "Both is keyboard-reachable beside this pane's Use action"
    );
    keys(cx, window, "enter");
}
#[track_caller]
fn tab_to(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    // Activation retains the row's real focus handle.
    let initial = probe(cx, window);
    if initial.as_deref() == Some(id) {
        return;
    }
    let mut focus_sequence = vec![initial];
    for _ in 0..100 {
        keys(cx, window, "tab");
        let focused = probe(cx, window);
        if focused.as_deref() == Some(id) {
            return;
        }
        focus_sequence.push(focused);
    }
    let viewport = cx
        .update_window(window, |_, window, _| window.viewport_size())
        .unwrap();
    panic!(
        "Tab must reach the resolver adoption controls: {id}; lang={:?}, zoom={}, viewport={viewport:?}, focus_sequence={focus_sequence:?}",
        i18n::lang(), theme::zoom(),
    );
}

fn hunk_adoption(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
    lang: Lang,
) {
    tab_to(cx, window, "conflict-hunk-0-current-use");
    keys(cx, window, "down");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("conflict-hunk-1-current-use"),
        "whole-file roving focus reaches the next hunk"
    );
    assert_eq!(
        cx.read(|cx| conflict.read(cx).selected_hunk),
        0,
        "keyboard focus does not change the selected decision hunk"
    );
    click_control(cx, window, "conflict-next-hunk");
    paint(cx, window);
    assert_source_hunk_visible(cx, window, 1);
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(view.selected_hunk, 1);
    });
    tab_to(cx, window, "conflict-hunk-1-current-use");
    keys(cx, window, "right right");
    let (role, name, state) =
        ax::control("conflict-hunk-1-incoming-use").expect("named incoming Use");
    assert_eq!(role, gpui::Role::Button);
    assert_eq!(name, i18n::Msg::ConflictUseThisSideAccessible.t());
    assert_eq!(state, "none");
    keys(cx, window, "enter");
    let text = assembled(cx, conflict);
    assert!(text.contains("theirs 17"));
    assert_eq!(
        ax::control("conflict-hunk-1-incoming-state").unwrap().1,
        i18n::Msg::ConflictUsed.t()
    );
    conflict.update(cx, |view, cx| {
        view.conflict_editor_reset_all(Path::new("notes.txt"));
        cx.notify();
    });
    paint(cx, window);
    click_control(cx, window, "conflict-hunk-1-incoming-use");
    assert_eq!(text, assembled(cx, conflict));
    horizontal_controls(cx, window, conflict, lang);
    line_range_keyboard(cx, window, conflict);
}

fn line_range_keyboard(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
) {
    keys(cx, window, "right enter");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("conflict-hunk-1-incoming-use"),
        "Change restores this pane's Use action in the roving group"
    );
    keys(cx, window, "enter");
    keys(cx, window, "right");
    let line = |index| format!("resolver-hunk-1-current-line-{index}");
    assert_eq!(probe(cx, window).as_deref(), Some(line(0).as_str()));
    assert_eq!(ax::control(&line(0)).unwrap().2, "none");
    keys(cx, window, "space");
    assert_eq!(ax::control(&line(0)).unwrap().2, "all");
    assert_eq!(probe(cx, window).as_deref(), Some(line(0).as_str()));
    for index in 1..=2 {
        keys(cx, window, "shift-down");
        assert_eq!(probe(cx, window).as_deref(), Some(line(index).as_str()));
        assert_eq!(ax::control(&line(index)).unwrap().2, "all");
        assert_eq!(
            ax::control(&format!("resolver-hunk-1-incoming-line-{index}"))
                .unwrap()
                .2,
            "all",
            "range painting does not clear the opposite pane"
        );
    }
    keys(cx, window, "space");
    assert_eq!(ax::control(&line(2)).unwrap().2, "none");
    for index in [1, 0] {
        keys(cx, window, "shift-up");
        assert_eq!(probe(cx, window).as_deref(), Some(line(index).as_str()));
        assert_eq!(ax::control(&line(index)).unwrap().2, "none");
    }
    assert!(!assembled(cx, conflict).contains("ours 17"));
    assert!(!assembled(cx, conflict).contains("ours 18"));
    assert!(!assembled(cx, conflict).contains("ours 19"));
    assert!(assembled(cx, conflict).contains("theirs 17"));
}

fn matrix_leg(
    cx: &mut VisualTestAppContext,
    lang: Lang,
    zoom: f32,
    slug: &str,
    dimensions: (f32, f32),
) {
    let fixture = fixture();
    let before = repo_fingerprint(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.set_theme(slug, cx);
        app.detect_conflict_mode(cx);
    });
    crate::pr_detail_adaptive::resize(cx, window, dimensions);
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    assert_eq!(
        cx.read(|cx| conflict.read(cx).page),
        kagi::ui::conflict_view::ConflictPage::Hunk,
        "first file opens directly at every language, zoom and window size"
    );
    paint(cx, window);
    assert_operation_placement(cx, window, &conflict);
    if zoom == 1. && dimensions.0 > 1000. {
        assert_abort_interaction_ink(cx, window, &conflict);
    }
    assert_source_hunk_visible(cx, window, 0);
    let row_height = f32::from(theme::scaled_px(24.));
    for side in ["current", "incoming"] {
        let viewport =
            e2e::control_bounds(window.window_id(), &format!("resolver-{side}-viewport")).unwrap();
        let line = e2e::control_bounds(
            window.window_id(),
            &format!("resolver-hunk-0-{side}-line-0"),
        )
        .unwrap()
        .dilate(gpui::px(kagi_ui_core::keyboard_nav::RING));
        assert!(
            (f32::from(line.size.height) - row_height).abs() <= 1.,
            "{side}: source row including fixed focus border is 24 scaled px: actual={:?}, expected={row_height}, zoom={zoom}",
            line.size.height
        );
        let height = f32::from(viewport.size.height);
        assert!(
            height >= row_height,
            "{side}: responsive viewport retains visible code"
        );
        let footer =
            e2e::control_bounds(window.window_id(), &format!("resolver-{side}-footer")).unwrap();
        assert!(
            footer.top() >= viewport.bottom(),
            "{side}: commit footer follows source"
        );
        if side == "current" {
            for id in ["conflict-prev-hunk", "conflict-next-hunk"] {
                let button = e2e::control_bounds(window.window_id(), id).unwrap();
                assert!(button.left() >= footer.left() && button.right() <= footer.right());
                assert!(button.top() >= footer.top() && button.bottom() <= footer.bottom());
            }
        }
    }
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx)
    })
    .unwrap();
    hunk_adoption(cx, window, &conflict, lang);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx)
    })
    .unwrap();
    click_control(cx, window, "conflict-prev-hunk");
    paint(cx, window);
    assert_source_hunk_visible(cx, window, 0);
    tab_to(cx, window, "conflict-hunk-0-current-use");
    keys(cx, window, "space");
    assert_eq!(
        ax::control("conflict-hunk-0-current-state").unwrap().1,
        i18n::Msg::ConflictUsed.t()
    );
    assert!(
        kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 20).is_empty(),
        "adoption emits no Git receipt"
    );
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "adoption remains buffer-only"
    );
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_conflict_text_keyboard(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    let original = (i18n::lang(), theme::zoom(), theme::theme().slug.to_string());
    for lang in [Lang::En, Lang::Ja] {
        for zoom in [1., 1.67] {
            for slug in ["apple-light", "apple-dark"] {
                for dimensions in [(940., 660.), (1392., 883.)] {
                    matrix_leg(cx, lang, zoom, slug, dimensions);
                }
            }
        }
    }
    i18n::set_lang(original.0);
    theme::set_zoom(original.1);
    theme::set_active(&original.2);
    cx.update(theme::sync_gpui_component_theme);
}

fn replace_model(
    cx: &mut VisualTestAppContext,
    conflict: &Entity<ConflictView>,
    repo: &Path,
    hunks: usize,
    lines: usize,
) {
    let backend = kagi_git::Backend::open(repo).unwrap();
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    let mut markers = String::new();
    for hunk in 0..hunks {
        markers.push_str("context\n<<<<<<< Current\n");
        for line in 0..lines {
            markers.push_str(&format!("current {hunk} {line}\n"));
        }
        markers.push_str("=======\n");
        for line in 0..lines {
            markers.push_str(&format!("incoming {hunk} {line}\n"));
        }
        markers.push_str(">>>>>>> Incoming\n");
    }
    assert!(buffer.ensure_hunks(Path::new("notes.txt"), &markers, 7));
    conflict.update(cx, |view, cx| {
        view.mode.as_mut().unwrap().buffer = buffer;
        cx.notify();
    });
}

pub fn scenario_conflict_resolver_rebuild(cx: &mut VisualTestAppContext) {
    let fixture = fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    click_control(cx, window, "conflict-row-0");
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx)
    })
    .unwrap();
    tab_to(cx, window, "conflict-hunk-0-current-use");
    replace_model(cx, &conflict, fixture.path(), 3, 1);
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx)
    })
    .unwrap();
    click_control(cx, window, "conflict-next-hunk");
    click_control(cx, window, "conflict-next-hunk");
    paint(cx, window);
    tab_to(cx, window, "conflict-hunk-2-current-use");
    keys(cx, window, "right right enter");
    assert!(assembled(cx, &conflict).contains("incoming 2 0"));
    replace_model(cx, &conflict, fixture.path(), 3, 4);
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx)
    })
    .unwrap();
    tab_to(cx, window, "conflict-hunk-2-current-use");
    keys(cx, window, "right right right right right right right");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("resolver-hunk-2-current-line-3")
    );
    keys(cx, window, "space");
    assert!(assembled(cx, &conflict).contains("current 2 3"));
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_conflict_resolver_roles(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        for rebase in [false, true] {
            let fixture = fixture_for_operation(rebase);
            let (app, window) = mount(cx, fixture.path());
            app.update(cx, |app, cx| {
                app.set_lang(lang, cx);
                app.detect_conflict_mode(cx);
            });
            cx.run_until_parked();
            let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
            click_control(cx, window, "conflict-row-0");
            paint(cx, window);
            let labels = cx.read(|cx| conflict.read(cx).mode.as_ref().unwrap().labels());
            assert_eq!(
                labels.current.role,
                if rebase { "New base" } else { "Current branch" }
            );
            assert_eq!(
                labels.incoming.role,
                if rebase {
                    "Your commit being replayed"
                } else {
                    "Merging in"
                }
            );
            crate::native_ax::activate(cx);
            paint(cx, window);
            crate::conflict_controls::assert_prominence(0);
            let tree = crate::native_ax::tree(cx, window);
            assert_session_accessibility(cx, window);
            for message in [
                i18n::Msg::ConflictBothCurrentFirst,
                i18n::Msg::ConflictBothIncomingFirst,
                i18n::Msg::ConflictResetFocusedAccessible,
                i18n::Msg::EditorReset,
            ] {
                let expected = message.t();
                assert!(
                    tree["nodes"].as_object().unwrap().values().any(|node| {
                        node["aria"]["role"] == "Button" && node["aria"]["label"] == expected
                    }),
                    "action rows and Result reset expose their localized choices: {expected}"
                );
            }
            for id in [
                "conflict-hunk-0-current-both",
                "conflict-hunk-0-incoming-both",
                "conflict-reset-hunk",
                "conflict-reset-all",
            ] {
                assert!(e2e::control_bounds(window.window_id(), id).is_some());
            }
            assert_eq!(
                ax::control("conflict-hunk-0-current-use").unwrap().2,
                "none"
            );
            for (side, label) in [("current", &labels.current), ("incoming", &labels.incoming)] {
                let identity = format!("{} ({})", label.role, label.name);
                let line = format!("resolver-hunk-0-{side}-line-0");
                assert_eq!(
                    ax::control(&line).unwrap().1,
                    if lang == Lang::En {
                        format!("Use {identity} line 2")
                    } else {
                        format!("{identity} の 2 行目を採用")
                    },
                    "source line accessible names retain operation-specific side identity"
                );
            }
            assert_order_only(cx, window, &conflict);
            assert_header_tab_order(cx, window, &app, &conflict);
            drop(conflict);
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(original);
}

fn assert_order_only(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
) {
    let before = assembled(cx, conflict);
    conflict.update(cx, |view, cx| {
        view.conflict_editor_set_hunk_order(
            Path::new("notes.txt"),
            0,
            kagi_git::resolution::LineOrder::IncomingFirst,
        );
        cx.notify();
    });
    paint(cx, window);
    assert_eq!(assembled(cx, conflict), before);
    cx.read(|cx| {
        assert!(
            !conflict
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .has_resolution(Path::new("notes.txt")),
            "ordering an untouched hunk must not adopt an empty resolution"
        );
    });
    assert_eq!(
        ax::control("conflict-hunk-0-current-use").unwrap().2,
        "none"
    );
}

fn assert_header_tab_order(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<kagi::ui::KagiApp>,
    conflict: &Entity<ConflictView>,
) {
    // Enable Save and Continue through real buffer-only adoption, without Git writes.
    for hunk in 0..2 {
        if hunk > 0 {
            click_control(cx, window, "conflict-next-hunk");
            paint(cx, window);
        }
        click_control(cx, window, &format!("conflict-hunk-{hunk}-current-use"));
        paint(cx, window);
    }
    assert!(cx.read(|cx| conflict.read(cx).mode.as_ref().unwrap().can_continue()));
    let sequencer = cx.read(|cx| {
        conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .session
            .op
            .is_sequencer()
    });
    for hidden in [false, true] {
        if cx.read(|cx| conflict.read(cx).sidebar_hidden) != hidden {
            click_control(cx, window, "conflict-sidebar-toggle");
        }
        assert_operation_placement(cx, window, conflict);
        let toggle = crate::native_ax::node(cx, window, "conflict-sidebar-toggle".into());
        crate::native_ax::focus_named(
            cx,
            window,
            "Button",
            toggle["aria"]["label"].as_str().unwrap(),
        );
        // Abort and Skip never interrupt the file-header traversal, even with
        // the sidebar hidden and the legacy operation strip mounted.
        // Path navigation may intervene, but progression cannot precede Save.
        let mut reached_save = false;
        for _ in 0..20 {
            keys(cx, window, "tab");
            let node = crate::native_ax::focused(cx, window);
            let label = node["aria"]["label"].as_str().unwrap_or_default();
            assert_ne!(label, i18n::Msg::ConflictSkip.t());
            assert_ne!(label, i18n::Msg::ConflictReviewContinue.t());
            assert_ne!(label, i18n::Msg::ConflictAbort.t());
            if label == i18n::Msg::EditorSave.t() {
                reached_save = true;
                break;
            }
        }
        assert!(
            reached_save,
            "Tab from the header toggle reaches Save before progression"
        );
        keys(cx, window, "tab");
        assert_eq!(
            crate::native_ax::focused(cx, window)["aria"]["label"],
            i18n::Msg::ConflictReviewContinue.t(),
            "Continue alone follows Save across the file/session separator"
        );
        if !hidden {
            crate::native_ax::focus_named(
                cx,
                window,
                "Button",
                toggle["aria"]["label"].as_str().unwrap(),
            );
            let mut reached_abort = false;
            // Walk backward from the header: forward Tab legitimately enters
            // the Result text input, where Tab is an editing command.
            for _ in 0..10 {
                keys(cx, window, "shift-tab");
                if crate::native_ax::focused(cx, window)["aria"]["label"]
                    == i18n::Msg::ConflictAbort.t()
                {
                    reached_abort = true;
                    break;
                }
            }
            assert!(
                reached_abort,
                "Tab reaches Abort in the sidebar session header"
            );
            if sequencer {
                keys(cx, window, "tab");
                assert_eq!(
                    crate::native_ax::focused(cx, window)["aria"]["label"],
                    i18n::Msg::ConflictSkip.t(),
                    "Skip follows Abort in the sidebar, not Save in the file header"
                );
            }
            assert_sidebar_focus_yields(cx, window, app, conflict, &toggle);
        }
    }
    if sequencer {
        let repo = cx.read(|cx| app.read(cx).repo_path.clone().unwrap());
        let before = repo_fingerprint(&repo);
        invoke_menu(cx, window, app, "repo.conflictSkip");
        assert_eq!(
            crate::native_ax::node(cx, window, "operation-strip-skip".into())["aria"]["label"],
            i18n::Msg::ConflictConfirmSkip.t(),
            "a menu-armed hidden sequencer shows its confirmation in the strip"
        );
        assert_eq!(
            before,
            repo_fingerprint(&repo),
            "the first menu Skip only arms"
        );
        click_control(cx, window, "conflict-sidebar-toggle");
        assert_operation_placement(cx, window, conflict);
        assert!(
            cx.read(|cx| conflict
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .skip_approval
                .is_some()),
            "armed EN/JA identity is fully readable at the default sidebar width"
        );
    }
}
