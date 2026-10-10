//! #643 Wave 4 S2a: a Commit Panel file-menu intent freezes its session and
//! repository-relative path before crossing the deferred entity → root boundary.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{point, px, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, file_menu::FileMenu};

fn dirty_fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("alpha.txt"), "alpha dirty\n").unwrap();
    std::fs::write(fixture.path().join("zeta.txt"), "zeta dirty\n").unwrap();
    fixture
}

fn defer_first_menu(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    window: gpui::AnyWindowHandle,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            e2e::defer_file_menu(app, 0, point(px(120.0), px(120.0)), window, cx);
        });
    })
    .unwrap();
}

fn draw_menu_and_bounds(
    cx: &mut VisualTestAppContext,
    window: gpui::AnyWindowHandle,
) -> gpui::Bounds<gpui::Pixels> {
    e2e::clear_control_bounds(window.window_id(), "file-menu-discard");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(window.window_id(), "file-menu-discard")
        .expect("the real Discard menu item was not laid out")
}

/// #1125: actual menu selection, confirmation, execution and backup agree on
/// a literal POSIX filename; bulk keeps the two distinct identities too.
pub fn scenario_discard_literal_backslash(cx: &mut VisualTestAppContext) {
    for bulk in [false, true] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        std::fs::create_dir(repo.join("a")).unwrap();
        std::fs::write(repo.join(r"a\b.txt"), b"selected base\n").unwrap();
        std::fs::write(repo.join("a/b.txt"), b"neighbor base\n").unwrap();
        crate::macos::git(&repo, &["add", "-A"]);
        crate::macos::git(&repo, &["commit", "-qm", "literal paths"]);
        std::fs::write(repo.join(r"a\b.txt"), b"selected dirty\n").unwrap();
        std::fs::write(repo.join("a/b.txt"), b"neighbor dirty\n").unwrap();
        let index_before = kagi_git::Backend::open(&repo)
            .unwrap()
            .staged_set_digest()
            .unwrap();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
        });
        cx.run_until_parked();
        if bulk {
            let owner = cx.read(|cx| {
                app.read(cx)
                    .ui()
                    .commit_panel
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .owner
            });
            app.update(cx, |app, cx| app.open_discard_all_modal(owner, cx));
        } else {
            let row = cx.read(|cx| {
                app.read(cx)
                    .ui()
                    .commit_panel
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .state
                    .unstaged
                    .iter()
                    .position(|f| f.path == std::path::Path::new(r"a\b.txt"))
                    .unwrap()
            });
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| {
                    e2e::defer_file_menu(app, row, point(px(120.0), px(120.0)), window, cx)
                });
            })
            .unwrap();
            cx.run_until_parked();
            let bounds = draw_menu_and_bounds(cx, window);
            cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
            cx.run_until_parked();
            cx.simulate_click(window, bounds.center(), Modifiers::none());
        }
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| {
            let modal = app.read(cx).discard_modal().unwrap();
            assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
            assert!(modal.paths.iter().any(|p| p == r"a\b.txt"));
            assert_eq!(modal.paths.len(), if bulk { 2 } else { 1 });
            assert!(modal.plan.title.message_en().contains(if bulk {
                "2 file(s)"
            } else {
                r"a\b.txt"
            }));
            for path in &modal.paths {
                assert!(modal
                    .plan
                    .preview_files
                    .iter()
                    .any(|f| f.path == std::path::Path::new(path)));
                assert!(
                    modal.kinds.contains_key(path),
                    "badge must use literal identity"
                );
            }
        });
        app.update(cx, |app, cx| app.start_discard(cx));
        assert_eq!(
            std::fs::read(repo.join(r"a\b.txt")).unwrap(),
            b"selected dirty\n"
        );
        app.update(cx, |app, cx| app.start_discard(cx));
        cx.run_until_parked();
        assert_eq!(
            std::fs::read(repo.join(r"a\b.txt")).unwrap(),
            b"selected base\n"
        );
        assert_eq!(
            std::fs::read(repo.join("a/b.txt")).unwrap(),
            if bulk {
                b"neighbor base\n".as_slice()
            } else {
                b"neighbor dirty\n".as_slice()
            }
        );
        let backend = kagi_git::Backend::open(&repo).unwrap();
        // UI reload may repair stat/cache-tree data (ADR-0193); approved index
        // path/OID/mode/stage identity must remain exactly the same.
        assert_eq!(backend.staged_set_digest().unwrap(), index_before);
        let receipts = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 20);
        let receipt = receipts.iter().rev().find(|r| r.op == "discard").unwrap();
        assert!(matches!(
            receipt.outcome,
            kagi_git::oplog::OpOutcome::Success { .. }
        ));
        assert_eq!(receipt.backup_refs.len(), if bulk { 2 } else { 1 });
        let contents: Vec<_> = receipt
            .backup_refs
            .iter()
            .map(|reference| backend.read_backup(reference).unwrap())
            .collect();
        assert_eq!(
            contents
                .iter()
                .filter(|bytes| bytes.as_slice() == b"selected dirty\n")
                .count(),
            1
        );
        if bulk {
            assert_eq!(
                contents
                    .iter()
                    .filter(|bytes| bytes.as_slice() == b"neighbor dirty\n")
                    .count(),
                1
            );
        }
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS discard_literal_backslash");
}

/// Non-UTF-8 input is refused before any write, including an entire bulk
/// selection. The invalid name is in memory: macOS cannot create it on disk.
pub fn scenario_discard_unsafe_selection(cx: &mut VisualTestAppContext) {
    use kagi_ui_core::i18n::{self, Lang, Msg};
    use std::os::unix::ffi::OsStringExt;
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_language = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        let fixture = dirty_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
        });
        cx.run_until_parked();
        let invalid =
            std::path::PathBuf::from(std::ffi::OsString::from_vec(b"invalid-\xff".to_vec()));
        let owner = cx.read(|cx| {
            app.read(cx)
                .ui()
                .commit_panel
                .as_ref()
                .unwrap()
                .read(cx)
                .owner
        });
        let expected = Msg::DiscardUnsafePath
            .t()
            .replace("{}", &invalid.display().to_string());
        for bulk in [false, true] {
            app.update(cx, |app, cx| {
                if bulk {
                    let panel = app.ui().commit_panel.as_ref().unwrap().clone();
                    panel.update(cx, |panel, _| {
                        panel.state.unstaged[0].path = invalid.clone()
                    });
                    app.open_discard_all_modal(owner, cx);
                } else {
                    app.open_discard_modal_for_path(
                        owner,
                        invalid.clone(),
                        kagi::ui::worktree_wip::WriteOrigin::CommitPanel,
                        cx,
                    );
                }
            });
            cx.run_until_parked();
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(state.discard_modal().is_none());
                assert!(
                    matches!(&state.status_footer, kagi::ui::FooterStatus::Failed(text)
                    if text.as_ref() == expected)
                );
            });
            assert_eq!(
                std::fs::read(repo.join("alpha.txt")).unwrap(),
                b"alpha dirty\n"
            );
            assert_eq!(
                std::fs::read(repo.join("zeta.txt")).unwrap(),
                b"zeta dirty\n"
            );
            let records = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 20);
            let record = records.iter().rev().find(|r| r.op == "discard").unwrap();
            assert!(matches!(
                record.outcome,
                kagi_git::oplog::OpOutcome::Refused { .. }
            ));
            assert!(record.backup_refs.is_empty());
        }
        unmount(cx, app, window);
    }
    i18n::set_lang(original_language);
    eprintln!("[gui-e2e] PASS discard_unsafe_selection");
}

pub fn scenario_file_menu_freezes_path(cx: &mut VisualTestAppContext) {
    let fixture = dirty_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    cx.run_until_parked();

    let (owner, frozen, other) = cx.read(|cx| {
        let app = app.read(cx);
        let panel = app
            .ui()
            .commit_panel
            .as_ref()
            .expect("commit panel")
            .read(cx);
        assert_eq!(panel.state.unstaged.len(), 2, "fixture has two menu rows");
        (
            panel.owner,
            panel.state.unstaged[0].path.clone(),
            panel.state.unstaged[1].path.clone(),
        )
    });

    // Start the production deferred callback, then renumber its source rows
    // before it lands. Resolving `fi` in the callback would now select `other`.
    defer_first_menu(cx, &app, window);
    app.update(cx, |app, cx| {
        let panel = app
            .ui()
            .commit_panel
            .as_ref()
            .expect("commit panel")
            .clone();
        panel.update(cx, |panel, _| panel.state.unstaged.swap(0, 1));
    });
    cx.run_until_parked();

    let menu = cx.read(|cx| app.read(cx).file_menu.clone().expect("menu opened"));
    assert_eq!(menu.owner, owner, "deferred menu changed its owner");
    assert_eq!(
        menu.path, frozen,
        "deferred menu re-resolved its row after renumbering"
    );

    // Click the real rendered menu item and execute the real discard path.
    let bounds = draw_menu_and_bounds(cx, window);
    cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).discard_modal().is_some()),
        "the real menu item did not dispatch Discard"
    );
    app.update(cx, |app, cx| app.start_discard(cx));
    app.update(cx, |app, cx| app.start_discard(cx));
    cx.run_until_parked();

    assert!(
        !repo.join(&frozen).exists(),
        "discard acted on a row other than the frozen path"
    );
    assert!(
        repo.join(&other).exists(),
        "discard removed the row that moved into the frozen index"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS file_menu_freezes_path");
}

pub fn scenario_file_menu_rejects_stale_owner(cx: &mut VisualTestAppContext) {
    let fixture_a = dirty_fixture();
    let fixture_b = dirty_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);

    let (owner_a, owner_b) = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        let owner_b = app.active_session().expect("B owner");
        app.switch_repo(0, cx);
        e2e::open_local_panel_no_inputs(app, repo_a.clone(), cx);
        (app.active_session().expect("A owner"), owner_b)
    });
    assert_ne!(owner_a, owner_b);
    cx.run_until_parked();

    // Event-source gate: the parent callback lands only after B owns the UI.
    defer_first_menu(cx, &app, window);
    app.update(cx, |app, cx| {
        app.switch_repo(1, cx);
        e2e::open_local_panel_no_inputs(app, repo_b.clone(), cx);
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).file_menu.is_none()),
        "A's deferred menu opened after B became active"
    );

    // Action gate: open A's menu, retain its exact intent, then dispatch it
    // after B owns both the active session and Commit Panel.
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        e2e::open_local_panel_no_inputs(app, repo_a.clone(), cx);
    });
    defer_first_menu(cx, &app, window);
    cx.run_until_parked();
    let stale: FileMenu = cx.read(|cx| app.read(cx).file_menu.clone().expect("A menu"));

    // Switch to B, attach B's real panel, and restore the stale intent as if
    // switch-time cleanup had missed it. The narrow seam invokes the same
    // dispatch helper retained render callbacks call.
    app.update(cx, |app, cx| {
        app.switch_repo(1, cx);
        e2e::open_local_panel_no_inputs(app, repo_b.clone(), cx);
        app.file_menu = Some(stale.clone());
    });
    app.update(cx, |app, cx| {
        e2e::dispatch_file_menu_discard(app, &stale, cx);
    });

    assert_eq!(
        cx.read(|cx| app.read(cx).active_session()),
        Some(owner_b),
        "fixture did not switch to B"
    );
    assert!(
        cx.read(|cx| app.read(cx).discard_modal().is_none()),
        "A's retained menu callback dispatched against B"
    );
    assert!(
        repo_b.join("alpha.txt").exists(),
        "B was mutated by A's menu"
    );
    assert!(
        repo_b.join("zeta.txt").exists(),
        "B was mutated by A's menu"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS file_menu_rejects_stale_owner");
}

/// A menu retained for A must neither swallow B's keyboard nor survive Escape.
pub fn scenario_file_menu_focus_after_open_repository(cx: &mut VisualTestAppContext) {
    let fixture_a = dirty_fixture();
    let fixture_b = dirty_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo_a, cx)
    });
    cx.run_until_parked();
    defer_first_menu(cx, &app, window);
    cx.run_until_parked();
    let stale = cx.read(|cx| app.read(cx).file_menu.clone().expect("A menu opened"));
    assert!(cx.read(|cx| e2e::menu_is_front(app.read(cx), cx)));
    // File-menu callback opens without moving focus. Start from the workspace
    // focus the Graph's arrow handler requires, before switching repositories.
    cx.update_window(window, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().expect("root focus");
        window.focus(&root, cx);
    })
    .unwrap();

    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).file_menu.is_none()),
        "departing A clears its owner-scoped menu"
    );
    app.update(cx, |app, cx| {
        assert!(app.view().rows.len() >= 2, "B has a parent commit");
        // A retained callback can still arrive after session-switch cleanup.
        app.file_menu = Some(stale.clone());
        app.select(1);
        cx.notify();
    });
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(1));
    assert_ne!(
        cx.read(|cx| app.read(cx).active_session()),
        Some(stale.owner)
    );
    assert!(
        !cx.read(|cx| e2e::menu_is_front(app.read(cx), cx)),
        "A's invisible file menu must not own B's keyboard"
    );
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    assert!(
        cx.update_window(window, |_, window, cx| {
            app.read(cx)
                .root_focus
                .as_ref()
                .is_some_and(|root| root.is_focused(window))
        })
        .unwrap(),
        "the drawn B workspace must retain focus"
    );
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).plan_modal().is_some()),
        "Enter must open the selected Graph commit's checkout plan on B"
    );
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_none()));

    // A visible menu takes Escape, closes through the common path, and
    // does not let the Graph's selected row toggle off.
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo_b.clone(), cx);
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.select(1);
        cx.notify();
    });
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(1));
    defer_first_menu(cx, &app, window);
    cx.run_until_parked();
    assert!(cx.read(|cx| e2e::menu_is_front(app.read(cx), cx)));
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).file_menu.is_none()));
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(1));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS file_menu_focus_after_open_repository");
}
