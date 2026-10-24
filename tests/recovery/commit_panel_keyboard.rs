//! Tier A keyboard and renderer-fed AX regressions for #1229–#1231.
use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::{contained, draw, GlobalSettings};
use e2e::commit_panel_labels as labels;
use gpui::{
    px, size, AnyWindowHandle, Bounds, Entity, Focusable, Pixels, Role, VisualTestAppContext,
};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::plan_note::{CommonNote, OpPhrase, PlanNote};
use kagi_ui_core::{
    i18n::{self, Msg},
    theme,
};
use std::{cell::RefCell, rc::Rc};

#[path = "commit_panel_bulk_hover.rs"]
mod bulk_hover;
#[path = "changes_keyboard.rs"]
mod changes_keyboard;
#[path = "commit_panel_coauthor_tab.rs"]
mod commit_panel_coauthor_tab;
#[path = "commit_panel_keyboard_focus.rs"]
mod focus_regressions;
pub use bulk_hover::scenario_commit_panel_bulk_hover;
pub use changes_keyboard::{
    scenario_changes_discard_keyboard, scenario_changes_file_menu_viewport,
    scenario_changes_focus_follows_selection, scenario_changes_rows_keyboard,
};

#[path = "smart_footer_provider.rs"]
mod smart_footer_provider;
pub use smart_footer_provider::scenario_smart_commit_footer_provider;

#[path = "commit_draft_save_failure.rs"]
mod commit_draft_save_failure;
#[path = "smart_generation_commit_boundary.rs"]
mod smart_generation_commit_boundary;
#[path = "smart_replace_undo.rs"]
mod smart_replace_undo;
pub use commit_draft_save_failure::scenario_commit_draft_save_failure;
pub use smart_generation_commit_boundary::scenario_smart_generation_commit_boundary;
pub use smart_generation_commit_boundary::scenario_smart_generation_replaced_panel;
pub use smart_replace_undo::scenario_smart_replace_undo;

const TEMPLATE: &str = "# Keyboard regression template\n";

#[derive(Clone, Copy)]
struct Case {
    locale: &'static str,
    theme: &'static str,
    dimensions: (f32, f32),
    zoom: f32,
}

fn matrix() -> impl Iterator<Item = Case> {
    ["apple-light", "apple-dark"].into_iter().flat_map(|theme| {
        [(1392., 883.), (940., 660.)]
            .into_iter()
            .flat_map(move |dimensions| {
                [1., 1.67].into_iter().flat_map(move |zoom| {
                    ["en", "ja"].into_iter().map(move |locale| Case {
                        locale,
                        theme,
                        dimensions,
                        zoom,
                    })
                })
            })
    })
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    let dimensions = cx
        .update_window(window, |_, window, _| {
            let viewport = window.viewport_size();
            (f32::from(viewport.width), f32::from(viewport.height))
        })
        .unwrap();
    draw(cx, window, dimensions);
}

fn press(cx: &mut VisualTestAppContext, window: AnyWindowHandle, key: &str) {
    paint(cx, window);
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(window, keystroke.clone());
    cx.simulate_event(window, gpui::KeyUpEvent { keystroke });
    paint(cx, window);
}

fn bounds(window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("{id} must be laid out"))
}

fn focus(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    let handle = labels::focus_handle(id)
        .unwrap_or_else(|| panic!("{id} must have a production keyboard focus handle"));
    cx.update_window(window, |_, window, cx| handle.focus(window, cx))
        .unwrap();
    paint(cx, window);
}

#[track_caller]
fn assert_focus(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    let (focused, actual, handle, expected) = cx
        .update_window(window, |_, window, cx| {
            let actual = [
                "cp-stage-all",
                "cp-discard-all",
                "cp-view-list",
                "cp-view-tree",
                "cp-unstage-all",
                "cp-staged-view-list",
                "cp-staged-view-tree",
                "cp-template-toggle",
                "cp-smart-suggest",
                "cp-coauthor",
                "cp-amend-btn",
                "cp-commit-btn",
            ]
            .into_iter()
            .find(|candidate| labels::focused(candidate, window));
            let focused = labels::focused(id, window);
            (
                focused,
                actual,
                window.focused(cx),
                labels::focus_handle(id),
            )
        })
        .unwrap();
    assert!(
        focused,
        "{id} must own window focus after real keyboard input; actual control={actual:?}, handle={handle:?}, expected={expected:?}"
    );
}

fn ax(id: &str, role: Role, name: &str) {
    assert_eq!(labels::accessible_role(id), Some(role), "{id} role");
    assert_eq!(
        labels::accessible_name(id).as_deref(),
        Some(name),
        "{id} name"
    );
}

fn open(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    case: Case,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    labels::clear();
    theme::set_zoom(case.zoom);
    std::env::set_var("KAGI_LANG", case.locale);
    i18n::init_lang();
    let state = e2e::app_state(repo).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let window = crate::macos::open_offscreen(
        cx,
        size(px(case.dimensions.0), px(case.dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    );
    let app = captured.borrow().clone().unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        app.update(cx, |app, cx| {
            app.set_theme(case.theme, cx);
            app.open_commit_panel(window, cx);
        });
    })
    .unwrap();
    settle_inputs(cx, window.into());
    (app, window.into())
}

pub fn scenario_commit_panel_segment_keys(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("selection.txt"), "staged selection\n").unwrap();
    git(fixture.path(), &["add", "selection.txt"]);
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    for case in matrix() {
        let (app, window) = open(cx, fixture.path(), case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        seed_segment_state(cx, window, &app, &panel);
        let preserved = panel_snapshot(cx, &panel);
        let selected_file = active_selection(cx, &app);
        let initial = bounds(window, "cp-view-list");
        ax("cp-view-list", Role::Tab, Msg::CommitPanelList.t());
        ax("cp-view-tree", Role::Tab, Msg::CommitPanelTree.t());
        ax("cp-view-group", Role::TabList, Msg::CommitPanelTitle.t());
        focus(cx, window, "cp-view-list");
        for (key, id, selected) in [
            ("right", "cp-view-tree", true),
            ("left", "cp-view-list", false),
            ("down", "cp-view-tree", true),
            ("up", "cp-view-list", false),
            ("end", "cp-view-tree", true),
            ("home", "cp-view-list", false),
            ("right", "cp-view-tree", true),
        ] {
            press(cx, window, key);
            assert_focus(cx, window, id);
            assert_eq!(cx.read(|cx| panel.read(cx).state.tree_view), selected);
            assert_eq!(labels::accessible_selected("cp-view-tree"), Some(selected));
            assert_eq!(labels::accessible_selected("cp-view-list"), Some(!selected));
            assert_eq!(
                bounds(window, "cp-view-list"),
                initial,
                "segment focus must not reflow"
            );
            assert_eq!(
                panel_snapshot(cx, &panel),
                preserved,
                "view arrows preserve draft and scroll"
            );
            assert_eq!(
                active_selection(cx, &app),
                selected_file,
                "view arrows preserve the selected diff entity, path and owner"
            );
        }
        for key in ["enter", "space"] {
            press(cx, window, key);
            assert!(cx.read(|cx| panel.read(cx).state.tree_view));
        }
        press(cx, window, "tab");
        assert!(
            !cx.update_window(window, |_, window, _| labels::focused(
                "cp-view-list",
                window
            ) || labels::focused(
                "cp-view-tree",
                window
            ))
            .unwrap(),
            "segment has one roving Tab stop"
        );
        press(cx, window, "shift-tab");
        assert_focus(cx, window, "cp-view-tree");
        drop(panel);
        labels::clear();
        unmount(cx, app, window);
    }
    focus_regressions::pointer_selector(cx);
    eprintln!("[gui-e2e] PASS commit_panel_segment_keys");
}

pub fn scenario_commit_panel_footer_tab_order(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("template"), TEMPLATE).unwrap();
    git(fixture.path(), &["config", "commit.template", "template"]);
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme", "commit_template_enabled"]);
    for case in matrix() {
        let (app, window) = open(cx, fixture.path(), case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        footer_case(cx, window, &panel);
        drop(panel);
        labels::clear();
        unmount(cx, app, window);
    }
    enabled_footer(cx);
    // Model-picker activation above keeps its real online admission contract.
    // The remaining phases exercise local Git/keyboard state, not networking.
    let _offline = crate::gui_isolation::OfflineNetwork::install();
    focus_regressions::bulk_keys(cx);
    disabled_bulk_keys(cx);
    focus_regressions::pointer_bulk(cx);
    focus_regressions::hidden_bulk_focus(cx);
    eprintln!("[gui-e2e] PASS commit_panel_footer_tab_order");
}

fn author_fixture(repo: &std::path::Path) {
    for index in 0..20 {
        let author = format!(
            "Long candidate {index:02} {} <candidate{index:02}.{}@example.test>",
            "名前".repeat(25),
            "long".repeat(20)
        );
        git(
            repo,
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "--author",
                &author,
                "-m",
                &format!("candidate {index}"),
            ],
        );
    }
}

pub fn scenario_commit_panel_coauthor_keys(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _offline = crate::gui_isolation::OfflineNetwork::install();
    let fixture = build_fixture();
    author_fixture(fixture.path());
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    for case in matrix() {
        theme::set_zoom(case.zoom);
        // A compact composer is anchored to real WIP, unlike the wide panel.
        // Keep the author history clean in wide cases and add only an owned,
        // unstaged file for each narrow case (never synthetic panel rows).
        let compact_wip = kagi_domain::compact::is_compact(
            case.dimensions.0,
            theme::rem_size_px(),
            theme::compact_threshold_rem(),
        )
        .then(|| fixture.path().join("coauthor-keyboard-wip.txt"));
        if let Some(path) = &compact_wip {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            std::fs::write(path, "Real WIP for the compact co-author composer\n").unwrap();
        }
        let (app, window) = open(cx, fixture.path(), case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        focus(cx, window, "cp-coauthor");
        commit_panel_coauthor_tab::check_tab_dismiss(cx, window, &panel);
        for activation in ["enter", "space"] {
            coauthor_activation(cx, window, &panel, case, activation);
        }
        press(cx, window, "enter");
        press(cx, window, "escape");
        assert!(cx.read(|cx| panel.read(cx).coauthor_menu.is_none()));
        assert_focus(cx, window, "cp-coauthor");
        drop(panel);
        labels::clear();
        unmount(cx, app, window);
        if let Some(path) = compact_wip {
            std::fs::remove_file(path).unwrap();
        }
    }
    empty_coauthors(cx);
    eprintln!("[gui-e2e] PASS commit_panel_coauthor_keys");
}

fn empty_coauthors(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    for case in matrix().filter(|case| {
        case.theme == "apple-light" && case.dimensions == (1392., 883.) && case.zoom == 1.
    }) {
        let (app, window) = open(cx, fixture.path(), case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        focus(cx, window, "cp-coauthor");
        commit_panel_coauthor_tab::check_tab_dismiss(cx, window, &panel);
        press(cx, window, "space");
        assert!(cx.read(|cx| panel.read(cx).coauthor_menu.as_ref().unwrap().is_empty()));
        ax("cp-coauthor-menu", Role::ListBox, Msg::AddCoAuthor.t());
        ax("cp-coauthor-empty", Role::Status, Msg::NoRecentAuthors.t());
        assert_focus(cx, window, "cp-coauthor-empty");
        let message = cx.read(|cx| panel.read(cx).committable_message(cx));
        contained(
            bounds(window, "cp-coauthor-menu"),
            bounds(window, "cp-coauthor-empty"),
            "empty candidates",
        );
        for key in ["down", "up", "enter", "space"] {
            press(cx, window, key);
            assert_eq!(
                cx.read(|cx| panel.read(cx).committable_message(cx)),
                message
            );
        }
        press(cx, window, "escape");
        assert!(cx.read(|cx| panel.read(cx).coauthor_menu.is_none()));
        assert_focus(cx, window, "cp-coauthor");
        drop(panel);
        labels::clear();
        unmount(cx, app, window);
    }
}

fn enabled_footer(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    std::fs::write(
        fixture.path().join("staged.txt"),
        "<<<<<<< keyboard\nours\n=======\ntheirs\n>>>>>>> keyboard\n",
    )
    .unwrap();
    git(fixture.path(), &["add", "staged.txt"]);
    for case in matrix() {
        let (app, window) = open(cx, fixture.path(), case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        let title = cx.read(|cx| panel.read(cx).title_input.clone()).unwrap();
        cx.update_window(window, |_, window, cx| {
            title.update(cx, |input, cx| {
                input.set_value("Keyboard commit subject", window, cx)
            });
        })
        .unwrap();
        settle_inputs(cx, window);
        assert_eq!(labels::accessible_disabled("cp-smart-suggest"), Some(false));
        assert_eq!(labels::accessible_disabled("cp-commit-btn"), Some(false));
        let footer = bounds(window, "cp-footer");
        focus(cx, window, "cp-amend-btn");
        press(cx, window, "tab");
        assert_focus(cx, window, "cp-commit-btn");
        assert_eq!(bounds(window, "cp-footer"), footer);
        press(cx, window, "shift-tab");
        assert_focus(cx, window, "cp-amend-btn");
        for key in ["enter", "space"] {
            focus(cx, window, "cp-commit-btn");
            press(cx, window, key);
            assert!(
                cx.read(|cx| panel.read(cx).state.plan_modal.is_some()),
                "blocked keyboard commit opens its safety plan without approval"
            );
            press(cx, window, "escape");
            assert!(cx.read(|cx| panel.read(cx).state.plan_modal.is_none()));
        }
        glyph_modals(cx, window, &app);
        drop(title);
        drop(panel);
        labels::clear();
        unmount(cx, app, window);
    }
}

fn ax_candidate(candidates: &[kagi_git::AuthorCandidate], index: usize) {
    let candidate = &candidates[index];
    ax(
        &format!("cp-coauthor-row-{index}"),
        Role::ListBoxOption,
        &format!("{} <{}>", candidate.name, candidate.email),
    );
}

fn seed_segment_state(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
    panel: &Entity<kagi::ui::commit_panel::CommitPanelView>,
) {
    let title = cx.read(|cx| panel.read(cx).title_input.clone()).unwrap();
    cx.update_window(window, |_, window, cx| {
        title.update(cx, |input, cx| {
            input.set_value("Preserved keyboard draft", window, cx)
        });
    })
    .unwrap();
    settle_inputs(cx, window);
    let owner = cx.read(|cx| panel.read(cx).owner);
    cx.update_window(window, |_, _, cx| {
        app.update(cx, |app, cx| {
            app.select_commit_panel_file(
                owner,
                kagi::ui::commit_panel::CommitPanelFileRef::Staged { index: 0 },
                cx,
            );
        });
    })
    .unwrap();
    paint(cx, window);
    assert_eq!(
        active_selection(cx, app).1,
        std::path::PathBuf::from("selection.txt")
    );
    drop(title);
}

fn contract_count(line: &str) -> usize {
    kagi_ui_core::klog::tail()
        .iter()
        .filter(|entry| entry.as_str() == line)
        .count()
}

fn conflict_fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    git(fixture.path(), &["checkout", "-q", "-b", "conflict-side"]);
    std::fs::write(fixture.path().join("README.md"), "side\n").unwrap();
    git(fixture.path(), &["commit", "-q", "-am", "side"]);
    git(fixture.path(), &["checkout", "-q", "main"]);
    std::fs::write(fixture.path().join("README.md"), "main\n").unwrap();
    git(fixture.path(), &["commit", "-q", "-am", "main"]);
    let output = std::process::Command::new("git")
        .current_dir(fixture.path())
        .args(["merge", "--no-edit", "conflict-side"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "fixture merge must leave a real conflict"
    );
    assert!(fixture.path().join(".git/MERGE_HEAD").exists());
    fixture
}

fn disabled_bulk_keys(cx: &mut VisualTestAppContext) {
    let fixture = conflict_fixture();
    for case in matrix() {
        let (app, window) = open(cx, fixture.path(), case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        assert_eq!(cx.read(|cx| panel.read(cx).state.conflicted_paths.len()), 1);
        let index = std::fs::read(fixture.path().join(".git/index")).unwrap();
        let file = std::fs::read(fixture.path().join("README.md")).unwrap();
        let discard_reason = i18n::plan_note_text(&PlanNote::Common(CommonNote::ConflictedFiles {
            count: 1,
            before: OpPhrase::DiscardingChanges,
        }));
        assert_ne!(discard_reason, Msg::ModalNoDiscardTargets.t());
        assert_eq!(
            discard_reason,
            match case.locale {
                "ja" => "conflict が 1 件あります。変更の破棄の前に解決してください。",
                _ => "Repository has 1 conflicted file(s). Resolve conflicts before discarding changes.",
            }
        );
        for (id, name, reason) in [
            (
                "cp-stage-all",
                Msg::CommitPanelStageAll,
                Msg::CommitPanelNoStageTargets.t(),
            ),
            (
                "cp-discard-all",
                Msg::CommitPanelDiscardAll,
                discard_reason.as_str(),
            ),
        ] {
            ax(id, Role::Button, name.t());
            assert_eq!(labels::accessible_disabled(id), Some(true));
            assert_eq!(labels::accessible_description(id).as_deref(), Some(reason));
            focus(cx, window, id);
            assert_focus(cx, window, id);
            for key in ["enter", "space"] {
                press(cx, window, key);
                assert_eq!(
                    std::fs::read(fixture.path().join(".git/index")).unwrap(),
                    index
                );
                assert_eq!(
                    std::fs::read(fixture.path().join("README.md")).unwrap(),
                    file
                );
                assert!(cx.read(|cx| app.read(cx).discard_modal().is_none()));
                assert_eq!(cx.read(|cx| panel.read(cx).state.conflicted_paths.len()), 1);
            }
        }
        drop(panel);
        labels::clear();
        unmount(cx, app, window);
    }
}

fn settle_inputs(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for _ in 0..4 {
        paint(cx, window);
    }
}

fn footer_case(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    panel: &Entity<kagi::ui::commit_panel::CommitPanelView>,
) {
    let body = cx.read(|cx| panel.read(cx).body_input.clone()).unwrap();
    let title = cx.read(|cx| panel.read(cx).title_input.clone()).unwrap();
    let (footer, tools) = footer_geometry(window);
    cx.update_window(window, |_, window, cx| {
        title.read(cx).focus_handle(cx).focus(window, cx)
    })
    .unwrap();
    press(cx, window, "tab");
    assert!(cx
        .update_window(window, |_, window, cx| body
            .read(cx)
            .focus_handle(cx)
            .is_focused(window))
        .unwrap());
    for id in [
        "cp-template-toggle",
        "cp-smart-suggest",
        "cp-coauthor",
        "cp-amend-btn",
    ] {
        press(cx, window, "tab");
        assert_focus(cx, window, id);
        assert_footer_stable(window, footer, tools);
    }
    focus(cx, window, "cp-smart-suggest");
    let before = cx.read(|cx| panel.read(cx).committable_message(cx));
    for key in ["enter", "space"] {
        press(cx, window, key);
        assert_eq!(
            cx.read(|cx| panel.read(cx).committable_message(cx)),
            before,
            "disabled generation must be inert"
        );
    }
    focus(cx, window, "cp-template-toggle");
    assert_eq!(labels::accessible_pressed("cp-template-toggle"), Some(true));
    press(cx, window, "space");
    assert_eq!(
        labels::accessible_pressed("cp-template-toggle"),
        Some(false)
    );
    press(cx, window, "enter");
    assert_eq!(labels::accessible_pressed("cp-template-toggle"), Some(true));
    assert_eq!(bounds(window, "cp-footer-tools"), tools);
    drop(title);
    drop(body);
}

fn coauthor_activation(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    panel: &Entity<kagi::ui::commit_panel::CommitPanelView>,
    case: Case,
    activation: &str,
) {
    let footer = bounds(window, "cp-footer");
    let tools = bounds(window, "cp-footer-tools");
    press(cx, window, activation);
    let candidates = cx.read(|cx| panel.read(cx).coauthor_menu.clone()).unwrap();
    assert_eq!(candidates.len(), 20);
    ax("cp-coauthor-menu", Role::ListBox, Msg::AddCoAuthor.t());
    let row = bounds(window, "cp-coauthor-row-0");
    let menu = bounds(window, "cp-coauthor-menu");
    assert!(
        menu.size.height <= theme::scaled_px(320.),
        "popup must have a bounded viewport"
    );
    contained(
        Bounds::new(
            gpui::point(px(0.), px(0.)),
            size(px(case.dimensions.0), px(case.dimensions.1)),
        ),
        menu,
        "popup inside window",
    );
    assert_footer_stable(window, footer, tools);
    ax_candidate(&candidates, 0);
    assert_focus(cx, window, "cp-coauthor-row-0");
    press(cx, window, "down");
    assert_eq!(labels::accessible_selected("cp-coauthor-row-1"), Some(true));
    ax_candidate(&candidates, 1);
    assert_focus(cx, window, "cp-coauthor-row-1");
    press(cx, window, "up");
    assert_eq!(labels::accessible_selected("cp-coauthor-row-0"), Some(true));
    assert_eq!(
        bounds(window, "cp-coauthor-row-0").size.height,
        row.size.height,
        "row focus must not change height"
    );
    press(cx, window, "end");
    assert_eq!(
        labels::accessible_selected("cp-coauthor-row-19"),
        Some(true)
    );
    ax_candidate(&candidates, 19);
    assert_focus(cx, window, "cp-coauthor-row-19");
    contained(
        bounds(window, "cp-coauthor-viewport"),
        bounds(window, "cp-coauthor-row-19"),
        "tail candidate",
    );
    assert_eq!(
        bounds(window, "cp-coauthor-row-19").size.height,
        row.size.height
    );
    assert_footer_stable(window, footer, tools);
    press(cx, window, activation);
    assert!(cx.read(|cx| panel.read(cx).coauthor_menu.is_none()));
    let message = cx.read(|cx| panel.read(cx).committable_message(cx));
    assert_eq!(
        message.matches(&candidates[19].trailer()).count(),
        1,
        "trailer added exactly once"
    );
    assert_focus(cx, window, "cp-coauthor");
}

fn panel_snapshot(
    cx: &mut VisualTestAppContext,
    panel: &Entity<kagi::ui::commit_panel::CommitPanelView>,
) -> (String, gpui::Point<Pixels>, gpui::Point<Pixels>) {
    cx.read(|cx| {
        let panel = panel.read(cx);
        let unstaged = panel.unstaged_scroll_handle.0.borrow();
        let staged = panel.staged_scroll_handle.0.borrow();
        (
            panel.committable_message(cx),
            unstaged.base_handle.offset(),
            staged.base_handle.offset(),
        )
    })
}

fn footer_geometry(window: AnyWindowHandle) -> (Bounds<Pixels>, Bounds<Pixels>) {
    for (id, msg) in [
        ("cp-template-toggle", Msg::ToggleCommitTemplate),
        ("cp-smart-suggest", Msg::GenerateMessage),
        ("cp-coauthor", Msg::AddCoAuthor),
        ("cp-amend-btn", Msg::AmendLastCommit),
    ] {
        ax(id, Role::Button, msg.t());
    }
    ax("cp-commit-btn", Role::Button, i18n::commit_button());
    assert_eq!(labels::accessible_disabled("cp-smart-suggest"), Some(true));
    assert_eq!(labels::accessible_disabled("cp-commit-btn"), Some(true));
    assert_eq!(
        labels::accessible_description("cp-smart-suggest").as_deref(),
        Some(Msg::AdviceCommitNothingStaged.t())
    );
    let footer = bounds(window, "cp-footer");
    let tools = bounds(window, "cp-footer-tools");
    for id in [
        "cp-template-toggle",
        "cp-smart-suggest",
        "cp-coauthor",
        "cp-amend-btn",
    ] {
        contained(footer, bounds(window, id), id);
    }
    for id in [
        "cp-smart-suggest-hit",
        "cp-coauthor-hit",
        "cp-amend-btn-hit",
    ] {
        assert!(
            bounds(window, id).size.height >= theme::scaled_px(24.),
            "{id} keyboard hit height"
        );
    }
    (footer, tools)
}

fn glyph_modals(cx: &mut VisualTestAppContext, window: AnyWindowHandle, app: &Entity<KagiApp>) {
    let _consent = crate::gui_isolation::SavedKeys::keep(&["smart_commit_consent_accepted"]);
    kagi::ui::settings::write_setting("smart_commit_consent_accepted", Some("1"));
    // Stop at model selection: no keyboard leg approves generation or a repository write.
    app.update(cx, |app, cx| {
        app.smart_commit.consent_accepted = true;
        app.smart_commit.llm_enabled = true;
        app.smart_commit.ollama_available = true;
        app.smart_commit.provider = kagi::ui::smart_commit::SmartProvider::Ollama;
        app.smart_commit.model = None;
        app.smart_commit.detected_models = vec!["keyboard-fixture-model".into()];
        cx.notify();
    });
    paint(cx, window);
    for key in ["enter", "space"] {
        focus(cx, window, "cp-smart-suggest");
        press(cx, window, key);
        assert!(cx.read(|cx| matches!(
            app.read(cx).smart_commit_modal(),
            Some(kagi::ui::smart_commit::SmartCommitModal::ModelPicker { .. })
        )));
        let list = kagi::ui::list_a11y::recorded_list("smart-model-list").unwrap();
        assert_eq!(list.role, Some(Role::ListBox));
        assert_eq!(list.label, Msg::SmartModelTitle.t());
        assert_eq!(list.size, 1);
        press(cx, window, "escape");
        assert!(cx.read(|cx| app.read(cx).smart_commit_modal().is_none()));
        focus(cx, window, "cp-amend-btn");
        press(cx, window, key);
        assert!(cx.read(|cx| app.read(cx).amend_modal().is_some()));
        press(cx, window, "escape");
        assert!(cx.read(|cx| app.read(cx).amend_modal().is_none()));
    }
}

fn active_selection(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
) -> (gpui::EntityId, std::path::PathBuf, kagi::app::SessionId) {
    cx.read(|cx| {
        let app = app.read(cx);
        let pane = app
            .ui()
            .main_diff
            .as_ref()
            .expect("selected file opens its real diff");
        let kagi::ui::MainDiffSource::Staged { path } = &pane.read(cx).view.source else {
            panic!("selection must remain on the staged fixture file");
        };
        (
            pane.entity_id(),
            path.clone(),
            app.active_session().unwrap(),
        )
    })
}

fn assert_footer_stable(window: AnyWindowHandle, footer: Bounds<Pixels>, tools: Bounds<Pixels>) {
    assert_eq!(
        bounds(window, "cp-footer"),
        footer,
        "focus must not shift footer"
    );
    assert_eq!(
        bounds(window, "cp-footer-tools"),
        tools,
        "focus must not shift tools"
    );
}
