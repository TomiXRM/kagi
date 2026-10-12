//! #1331: inventory of production Input renderers and their focused-node names.
//!
//! The fork records the aria_label passed to its track_focus TextInput div,
//! not Kagi's Group ancestors. This is Tier A renderer/focus evidence; native
//! AccessKit delivery and VoiceOver remain Tier B.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use gpui::{AnyWindowHandle, Entity, Focusable, VisualTestAppContext};
use gpui_component::input::{take_rendered_inputs_for_test, InputState};
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

fn rendered(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Vec<Entity<InputState>> {
    // Discard previous windows/frames. Weak entries cannot keep closed views
    // alive; only Inputs rendered by this fresh production frame are checked.
    take_rendered_inputs_for_test();
    draw(cx, window);
    let mut inputs = cx.read(|_cx| {
        take_rendered_inputs_for_test()
            .into_iter()
            .filter_map(|input| input.upgrade())
            .collect::<Vec<_>>()
    });
    inputs.sort_by_key(|input| input.entity_id().as_u64());
    inputs.dedup_by_key(|input| input.entity_id());
    inputs
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
        let inputs = rendered(cx, window);
        assert!(
            inputs.len() >= expected.len(),
            "{surface} {language:?}: expected fields actually render ({} < {})",
            inputs.len(),
            expected.len()
        );
        let names = cx.read(|cx| {
            inputs
                .iter()
                .map(|input| {
                    let name = input.read(cx).rendered_aria_label().unwrap_or_else(|| {
                        panic!(
                            "{surface} {language:?}: Input {:?} has no own aria_label",
                            input.entity_id()
                        )
                    });
                    assert!(
                        !name.trim().is_empty(),
                        "{surface} {language:?}: Input {:?} has an empty own aria_label",
                        input.entity_id()
                    );
                    name
                })
                .collect::<Vec<_>>()
        });
        for message in expected {
            assert!(
                names.iter().any(|name| name.as_ref() == message.t()),
                "{surface} {language:?}: missing localized field {:?}; rendered {names:?}",
                message.t()
            );
        }
        for (input, name) in inputs.iter().zip(&names) {
            cx.update_window(window, |_, window, cx| {
                input.read(cx).focus_handle(cx).focus(window, cx);
            })
            .unwrap();
            draw(cx, window);
            cx.update_window(window, |_, window, cx| {
                assert!(
                    input.read(cx).focus_handle(cx).is_focused(window),
                    "{surface} {language:?}: focus belongs to the rendered Input itself"
                );
                assert_eq!(
                    input.read(cx).rendered_aria_label().as_ref(),
                    Some(name),
                    "{surface} {language:?}: the focused TextInput retains its own name"
                );
            })
            .unwrap();
        }
        eprintln!(
            "[gui-e2e] input_names_inventory {surface} {language:?}: {} named focused Inputs",
            inputs.len()
        );
    }
    take_rendered_inputs_for_test();
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
                &[Msg::InputBranchName, Msg::InputWorktreePath]
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
                editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
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
            view.result_editing = true;
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
