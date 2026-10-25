//! Production Input names and IME boundaries inspected through native AX.
//! The oracle is the actual TextInput node, not a named ancestor or SDK recorder.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};
use crate::native_ax;
use crate::pr_fields_focus::OfflineGh;
use gpui::{AnyWindowHandle, Entity, EntityInputHandler, Focusable, VisualTestAppContext};
use gpui_component::WindowExt;
use kagi::ui::{
    e2e,
    i18n::{Lang, Msg},
    modals::EditorFsPromptKind,
    KagiApp,
};
use std::time::{Duration, Instant};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn check(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    surface: &str,
    expected: &[Msg],
) {
    for language in [Lang::En, Lang::Ja, Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(language, cx));
        native_ax::activate(cx);
        draw(cx, window);
        let tree = native_ax::tree(cx, window);
        let names = tree["nodes"]
            .as_object()
            .unwrap()
            .values()
            .filter(|node| {
                matches!(
                    node["aria"]["role"].as_str(),
                    Some("TextInput" | "MultilineTextInput")
                )
            })
            .map(|node| {
                let name = node["aria"]["label"].as_str().unwrap_or_else(|| {
                    panic!("{surface} {language:?}: unnamed native TextInput: {node}")
                });
                assert!(!name.trim().is_empty(), "{surface}: empty TextInput name");
                (
                    node["aria"]["role"].as_str().unwrap().to_owned(),
                    name.to_owned(),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            names.len() >= expected.len(),
            "{surface}: missing native fields: {tree}"
        );
        for message in expected {
            assert!(
                names.iter().any(|(_, name)| name == message.t()),
                "{surface} {language:?}: missing localized field {:?}; rendered {names:?}",
                message.t()
            );
        }
        for (role, name) in &names {
            native_ax::focus_named(cx, window, role, name);
            let focused = native_ax::focused(cx, window);
            assert_eq!(focused["aria"]["role"], role.as_str());
            assert_eq!(focused["aria"]["label"], name.as_str());
        }
        eprintln!(
            "[gui-e2e] input_names_inventory {surface} {language:?}: {} named focused Inputs",
            names.len()
        );
    }
}

pub fn scenario_input_names_inventory(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "lang",
        "recent_repos",
        "settings_pane",
        "settings_window_frame",
    ]);
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let head = kagi_git::CommitId(git_fixture::git_output(
        fixture.path(),
        &["rev-parse", "HEAD"],
    ));

    // Each leg owns and unmounts one root, so modal state, hidden retained
    // editors and asynchronous reads cannot disguise a missing renderer.
    for surface in [
        "sidebar",
        "palette",
        "home",
        "pr-filter",
        "issue-title",
        "issue-body",
        "issue-reply",
        "commit-panel",
        "branch",
        "worktree",
        "tag",
        "rename",
        "upstream",
        "lock-reason",
        "stash",
        "remote",
        "editor-name",
        "editor",
    ] {
        let (app, window) = mount(cx, fixture.path());
        let expected: &[Msg] = match surface {
            "sidebar" => &[Msg::A11ySidebarFilter],
            "palette" => {
                cx.update_window(window, |_, window, cx| {
                    app.update(cx, |app, cx| app.open_command_palette(window, cx));
                })
                .unwrap();
                &[Msg::A11yCommandPaletteQuery]
            }
            "home" => {
                cx.update_window(window, |_, window, cx| {
                    app.update(cx, |app, cx| app.open_home_tab(window, cx));
                })
                .unwrap();
                &[Msg::A11yHomeFilter]
            }
            "pr-filter" => {
                app.update(cx, |app, cx| app.show_pr_mode(cx));
                &[Msg::A11yListTitleFilter]
            }
            "issue-title" | "issue-body" | "issue-reply" => {
                app.update(cx, |app, cx| {
                    app.seed_issue_composer_for_e2e(cx);
                    app.seed_issue_navigation_for_e2e(cx);
                    app.show_issues_mode(cx);
                });
                if surface == "issue-reply" {
                    app.update(cx, |app, cx| app.seed_issue_reply_for_e2e(4, cx));
                    cx.update_window(window, |_, window, cx| {
                        app.update(cx, |app, cx| {
                            app.focus_issue_reply_input_for_e2e(4, window, cx)
                        });
                    })
                    .unwrap();
                    cx.run_until_parked();
                    &[Msg::A11yCommentBody]
                } else {
                    draw(cx, window);
                    if surface == "issue-body" {
                        cx.update_window(window, |_, window, cx| {
                            app.update(cx, |app, cx| app.focus_issue_title_for_e2e(window, cx));
                        })
                        .unwrap();
                        keys(cx, window, "enter");
                        &[
                            Msg::A11yListTitleFilter,
                            Msg::A11yIssueTitle,
                            Msg::A11yIssueBody,
                        ]
                    } else {
                        &[Msg::A11yListTitleFilter, Msg::A11yIssueTitle]
                    }
                }
            }
            "commit-panel" => {
                cx.update_window(window, |_, window, cx| {
                    app.update(cx, |app, cx| app.open_commit_panel(window, cx));
                })
                .unwrap();
                &[Msg::A11yCommitSummary, Msg::A11yCommitDescription]
            }
            "branch" => {
                app.update(cx, |app, cx| app.open_create_branch_modal(head.clone(), cx));
                &[Msg::InputBranchName]
            }
            "worktree" => {
                app.update(cx, |app, cx| {
                    app.open_create_worktree_modal(head.clone(), cx)
                });
                &[Msg::InputNewBranchName, Msg::InputWorktreePath]
            }
            "tag" => {
                app.update(cx, |app, cx| app.open_create_tag_modal(head.clone(), cx));
                &[Msg::InputTagName]
            }
            "rename" => {
                app.update(cx, |app, cx| {
                    app.open_rename_branch_modal("main".into());
                    cx.notify();
                });
                &[Msg::InputNewBranchName]
            }
            "upstream" => {
                app.update(cx, |app, cx| {
                    app.open_set_upstream_modal("main".into());
                    cx.notify();
                });
                &[Msg::InputUpstream]
            }
            "lock-reason" => {
                app.update(cx, |app, cx| {
                    app.open_lock_worktree_modal("main".into());
                    cx.notify();
                });
                &[Msg::WorktreeLockReason]
            }
            "stash" => {
                app.update(cx, |app, cx| app.open_stash_push_modal(cx));
                &[Msg::A11yStashMessage]
            }
            "remote" => {
                app.update(cx, |app, cx| app.open_remote_browse(cx));
                &[
                    Msg::RemoteHostLabel,
                    Msg::RemotePortLabel,
                    Msg::RemoteIdentityLabel,
                ]
            }
            "editor-name" => {
                app.update(cx, |app, cx| {
                    app.open_editor_fs_prompt(
                        EditorFsPromptKind::NewFile,
                        Default::default(),
                        String::new(),
                        cx,
                    )
                });
                &[Msg::EditorFsPromptNameLabel]
            }
            "editor" => {
                app.update(cx, |app, cx| app.show_editor_mode(cx));
                let editor = cx
                    .read(|cx| app.read(cx).ui().editor_workspace.clone())
                    .unwrap();
                editor.update(cx, |view, cx| {
                    view.open_tab("README.md".into(), cx);
                    // Inventory the rendered editable Input, not Markdown's
                    // default Preview surface, which has no code field.
                    view.set_markdown_preview(false, cx);
                });
                let deadline = Instant::now() + Duration::from_secs(15);
                loop {
                    cx.run_until_parked();
                    draw(cx, window);
                    if cx.read(|cx| {
                        !editor.read(cx).file_loading && editor.read(cx).editor.is_some()
                    }) {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "Editor inventory file did not load"
                    );
                    std::thread::sleep(Duration::from_millis(2));
                }
                drop(editor);
                &[Msg::A11yEditorFileContents]
            }
            _ => unreachable!(),
        };
        cx.run_until_parked();
        check(cx, &app, window, surface, expected);
        unmount(cx, app, window);
    }

    // Settings' production macOS host has its own window and focus tree.
    {
        let _host = crate::settings_window::SettingsHostGuard::native(cx);
        kagi::ui::settings::write_setting("settings_pane", Some("3"));
        let (app, repo_window) = mount(cx, fixture.path());
        cx.update_window(repo_window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("app.settings", window, cx)
            });
        })
        .unwrap();
        cx.run_until_parked();
        let settings_window = cx.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|window| window.window_id() != repo_window.window_id())
                .unwrap()
        });
        check(
            cx,
            &app,
            settings_window,
            "settings-analyze",
            &[Msg::A11yAnalyzeIgnorePatterns],
        );
        cx.update_window(settings_window, |_, window, _| window.remove_window())
            .unwrap();
        unmount(cx, app, repo_window);
    }

    // The result Input is only mounted by the real editable conflict pane.
    {
        let fixture = crate::app_conflict::content_fixture();
        let (app, window) = mount(cx, fixture.path());
        app.update(cx, |app, cx| app.detect_conflict_mode(cx));
        cx.run_until_parked();
        let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
        conflict.update(cx, |view, cx| {
            view.conflict_open_editor(std::path::Path::new("file.txt"));
            view.conflict_editor_set_result_mode(true);
            cx.notify();
        });
        check(
            cx,
            &app,
            window,
            "conflict-result",
            &[Msg::A11yConflictResult],
        );
        drop(conflict);
        unmount(cx, app, window);
    }

    // PR reply uses the shared body renderer too, but has a separate caller.
    {
        let (_fixture, repo, _remote, head) = crate::pr_threads::c_txt_pr();
        let (app, window) = mount(cx, &repo);
        e2e::queue_github_pr_conversation(gpui::Task::ready((Ok((vec![], vec![])), Ok(vec![]))));
        app.update(cx, |app, cx| {
            app.pr_mode_open(&crate::pr_viewed::pr_at(&head), cx)
        });
        crate::pr_viewed::wait_loaded(cx, &app, &head);
        check(cx, &app, window, "pr-reply", &[Msg::A11yCommentBody]);
        app.update(cx, |app, cx| {
            app.open_pr_fields_modal(kagi::ui::modals::PrField::Reviewers, cx)
        });
        cx.run_until_parked();
        check(cx, &app, window, "pr-fields", &[Msg::A11yPrReviewersFilter]);
        unmount(cx, app, window);
    }
}

pub fn scenario_input_ime_escape_boundaries(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "lang",
        "recent_repos",
        "settings_pane",
        "settings_window_frame",
    ]);
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    for surface in ["sidebar", "commit-body", "issue-body", "settings"] {
        let _host = crate::settings_window::SettingsHostGuard::native(cx);
        let (app, repo_window) = mount(cx, fixture.path());
        let mut window = repo_window;
        let name = match surface {
            "sidebar" => Msg::A11ySidebarFilter,
            "commit-body" => {
                cx.update_window(window, |_, window, cx| {
                    app.update(cx, |app, cx| app.open_commit_panel(window, cx));
                })
                .unwrap();
                Msg::A11yCommitDescription
            }
            "issue-body" => {
                app.update(cx, |app, cx| {
                    app.seed_issue_composer_for_e2e(cx);
                    app.seed_issue_navigation_for_e2e(cx);
                    app.show_issues_mode(cx);
                });
                draw(cx, window);
                cx.update_window(window, |_, window, cx| {
                    app.update(cx, |app, cx| app.focus_issue_title_for_e2e(window, cx));
                })
                .unwrap();
                keys(cx, window, "enter");
                Msg::A11yIssueBody
            }
            "settings" => {
                kagi::ui::settings::write_setting("settings_pane", Some("3"));
                cx.update_window(window, |_, window, cx| {
                    app.update(cx, |app, cx| {
                        app.handle_menu_command("app.settings", window, cx)
                    });
                })
                .unwrap();
                cx.run_until_parked();
                window = cx.read(|cx| {
                    cx.windows()
                        .into_iter()
                        .find(|other| other.window_id() != repo_window.window_id())
                        .unwrap()
                });
                Msg::A11yAnalyzeIgnorePatterns
            }
            _ => unreachable!(),
        };
        cx.run_until_parked();
        native_ax::activate(cx);
        draw(cx, window);
        let role = if surface == "sidebar" {
            "TextInput"
        } else {
            "MultilineTextInput"
        };
        native_ax::focus_named(cx, window, role, name.t());
        let input = cx
            .update_window(window, |_, window, cx| {
                let input = window.focused_input(cx).expect("actual focused Input");
                assert!(input.read(cx).focus_handle(cx).is_focused(window));
                input.update(cx, |state, cx| {
                    state.set_value("draft", window, cx);
                    state.replace_and_mark_text_in_range(None, "変換", None, window, cx);
                    assert!(state.marked_text_range(window, cx).is_some());
                });
                input
            })
            .unwrap();
        draw(cx, window);
        assert_eq!(
            native_ax::focused(cx, window)["aria"]["label"],
            name.t(),
            "{surface}: the composed field still owns the dispatched Escape"
        );
        let value = cx.read(|cx| input.read(cx).value());
        keys(cx, window, "escape");
        draw(cx, window);
        cx.update_window(window, |_, window, cx| {
            input.update(cx, |state, cx| {
                assert!(
                    state.marked_text_range(window, cx).is_none(),
                    "{surface}: unmarked"
                );
                assert_eq!(
                    state.value(),
                    value,
                    "{surface}: Escape does not clear draft"
                );
                assert!(
                    state.focus_handle(cx).is_focused(window),
                    "{surface}: focus retained"
                );
            });
        })
        .unwrap();
        assert_eq!(native_ax::focused(cx, window)["aria"]["label"], name.t());
        // The next capture must overwrite its previous marked flag. Ordinary
        // Escape still follows the existing host policy, rather than sticking.
        if surface == "sidebar" {
            keys(cx, window, "escape");
            draw(cx, window);
            assert!(cx.read(|cx| input.read(cx).value().is_empty()));
            assert_eq!(native_ax::focused(cx, window)["aria"]["label"], name.t());
            keys(cx, window, "escape");
            draw(cx, window);
            cx.update_window(window, |_, window, cx| {
                assert!(app.read(cx).sidebar_row_focused_for_e2e(window).is_some());
            })
            .unwrap();
        } else {
            keys(cx, window, "escape");
            draw(cx, window);
            assert_eq!(native_ax::focused(cx, window)["aria"]["label"], name.t());
            assert_eq!(cx.read(|cx| input.read(cx).value()), value);
        }
        drop(input);
        if window.window_id() != repo_window.window_id() {
            cx.update_window(window, |_, window, _| window.remove_window())
                .unwrap();
        }
        unmount(cx, app, repo_window);
    }
    input_menu_priority(cx, fixture.path());
}

fn input_menu_priority(cx: &mut VisualTestAppContext, repo: &std::path::Path) {
    let _toolbar = crate::toolbar_overflow::ToolbarRestore::capture(cx);
    kagi_ui_core::theme::set_zoom(1.67);
    kagi::ui::settings::set_toolbar_display(kagi::ui::settings::ToolbarDisplay::IconAndText);
    let (app, window) = mount(cx, repo);
    crate::pr_detail_adaptive::resize(cx, window, (1000., 720.));
    native_ax::activate(cx);
    draw(cx, window);
    keys(cx, window, "cmd-f");
    draw(cx, window);
    let input = cx.read(|cx| app.read(cx).sidebar.filter.clone().unwrap());
    let overflow = e2e::control_bounds(window.window_id(), "tb-overflow")
        .expect("real toolbar overflow is mounted at enlarged desktop density");
    native_ax::focus_named(cx, window, "Button", Msg::ToolbarOverflow.t());
    // A retained editor may still carry marked text after another control
    // takes focus. Neither observation nor an editing action may target it.
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |state, cx| {
            state.replace_and_mark_text_in_range(None, "変換", None, window, cx);
        });
        assert!(!input.read(cx).focus_handle(cx).is_focused(window));
        assert!(!kagi_ui_core::input_actions::focused_input_has_marked_text(
            window, cx
        ));
        window.dispatch_action(Box::new(gpui_component::input::Escape), cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |state, cx| {
            assert!(
                state.marked_text_range(window, cx).is_some(),
                "button-focused Escape does not edit a retained Input"
            );
        });
    })
    .unwrap();
    cx.simulate_click(window, overflow.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "tb-overflow-menu").is_some(),
        "real component PopupMenu opens"
    );
    let bottom_open = cx.read(|cx| app.read(cx).bottom_panel_open);
    keys(cx, window, "escape");
    e2e::clear_control_bounds(window.window_id(), "tb-overflow-menu");
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "tb-overflow-menu").is_none(),
        "PopupMenu consumes Escape before the workspace fallback (#1554)"
    );
    assert_eq!(cx.read(|cx| app.read(cx).bottom_panel_open), bottom_open);
    assert_eq!(
        native_ax::focused(cx, window)["aria"]["label"],
        Msg::ToolbarOverflow.t()
    );
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |state, cx| {
            assert!(
                state.marked_text_range(window, cx).is_some(),
                "menu cancellation does not unmark its unfocused editor"
            );
        });
    })
    .unwrap();
    drop(input);
    unmount(cx, app, window);
}
