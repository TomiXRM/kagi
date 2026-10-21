//! #1562 / #1565: visible named outcomes and independent recovery availability.
use crate::macos::{build_fixture, mount, open_offscreen, unmount};
use crate::recovery_oplog_panel::paint;
use gpui::{px, size, AnyWindowHandle, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, i18n::Lang, oplog_panel::OpLogPanel, theme, BottomTab};
use kagi_git::{oplog::OpOutcome, Backend, Operation};

fn mount_sized(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    dimensions: (f32, f32),
) -> (gpui::Entity<kagi::ui::KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).unwrap();
    let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
    let out = captured.clone();
    let window = open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &out),
    );
    let app = captured.borrow().clone().unwrap();
    cx.run_until_parked();
    (app, window.into())
}
fn bounds(window: AnyWindowHandle, name: &str) -> gpui::Bounds<gpui::Pixels> {
    e2e::control_bounds(window.window_id(), name)
        .unwrap_or_else(|| panic!("{name} must be visibly painted"))
}

pub fn scenario_oplog_row_167_result_visible(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let old_theme = theme::theme().slug.to_string();
    let fixture = build_fixture();
    for language in [Lang::En, Lang::Ja] {
        for zoom in [1., 1.67] {
            for slug in ["apple-light", "apple-dark"] {
                for dimensions in [(1000., 720.), (1400., 900.)] {
                    let (app, window) = mount_sized(cx, fixture.path(), dimensions);
                    theme::set_zoom(zoom);
                    app.update(cx, |app, cx| {
                        app.set_lang(language, cx);
                        app.set_theme(slug, cx);
                        app.select(0);
                        app.bottom_panel_open = true;
                        app.bottom_tab = BottomTab::OperationLog;
                        app.bottom_panel_height = 360.;
                        cx.notify();
                    });
                    let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
                    let entries = (0..5)
                        .map(|i| {
                            let mut entry =
                                crate::recovery_oplog_keyboard::entry(fixture.path(), i, i, true);
                            entry.worktree = Some(format!(
                                "/tmp/round16-wt-{}",
                                "very-long-worktree-name-".repeat(12)
                            ));
                            let state = entry.before.clone();
                            entry.outcome = match i {
                                0 => OpOutcome::Success { after: state },
                                1 => OpOutcome::Failed {
                                    error: "network failure".repeat(100),
                                },
                                2 => OpOutcome::Unknown {
                                    after: state,
                                    evidence: "unknown".repeat(100),
                                },
                                3 => OpOutcome::Partial {
                                    after: state,
                                    error: "partial".repeat(100),
                                },
                                _ => OpOutcome::Refused {
                                    blockers: vec!["refused".repeat(100)],
                                },
                            };
                            entry
                        })
                        .collect();
                    panel.update(cx, |panel, cx| {
                        *panel = OpLogPanel::from_entries(entries);
                        cx.notify();
                    });
                    paint(cx, window);
                    for (i, status) in ["Success", "Failed", "Unknown", "Partial", "Refused"]
                        .iter()
                        .enumerate()
                    {
                        // Compact windows own a smaller viewport independently
                        // of bottom_panel_height; reveal every virtualized row.
                        cx.read(|cx| panel.read(cx).scroll_handle().scroll_to_reveal_item(i));
                        paint(cx, window);
                        let row = cx.read(|cx| {
                            panel.read(cx).scroll_handle().bounds_for_item(i)
                        }).unwrap_or_else(|| {
                            panic!("row {i} must be visible: {language:?} {zoom} {slug} {dimensions:?}")
                        });
                        let status = bounds(window, &format!("oplog-status-{i}-{status}"));
                        let copy = bounds(window, &format!("oplog-copy-{i}"));
                        assert!(
                            status.size.width >= theme::scaled_px(16.),
                            "named status must not collapse"
                        );
                        assert!(
                            status.left() >= row.left() && status.right() <= row.right(),
                            "status stays in row"
                        );
                        assert!(
                            copy.left() >= row.left() && copy.right() <= row.right(),
                            "Copy stays in row"
                        );
                        assert!(
                            status.right() <= copy.left(),
                            "status and Copy do not overlap"
                        );
                        crate::native_ax::activate(cx);
                        paint(cx, window);
                        let ax = crate::native_ax::node(cx, window, ("oplog-status", i).into());
                        let expected = match (language, i) {
                            (Lang::En, 0) => "Success",
                            (Lang::En, 1) => "Failed",
                            (Lang::En, 2) => "Unknown",
                            (Lang::En, 3) => "Partial",
                            (Lang::En, _) => "Refused",
                            (Lang::Ja, 0) => "成功",
                            (Lang::Ja, 1) => "失敗",
                            (Lang::Ja, 2) => "不明",
                            (Lang::Ja, 3) => "一部完了",
                            (Lang::Ja, _) => "拒否",
                        };
                        assert_eq!(ax["aria"]["label"], expected, "native status name");
                    }
                    drop(panel);
                    unmount(cx, app, window);
                }
            }
        }
    }
    theme::set_active(&old_theme);
    cx.update(theme::sync_gpui_component_theme);
}

pub fn scenario_oplog_revert_eligibility(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let mut backend = Backend::open(&repo).unwrap();
    let at = kagi_git::CommitId(
        String::from_utf8(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .into(),
    );
    crate::recovery_oplog_panel::run(
        &mut backend,
        kagi_git::oplog::Actor::Human,
        Operation::CreateBranch {
            name: "eligible".into(),
            at,
        },
    );
    let recorded = kagi_git::oplog::read_oplog_tail(1).pop().unwrap();
    assert!(matches!(recorded.outcome, OpOutcome::Success { .. }));
    assert!(recorded
        .ref_moves
        .as_ref()
        .is_some_and(|moves| !moves.is_empty()));
    assert_eq!(
        std::path::Path::new(kagi::ui::oplog_panel::entry_worktree(&recorded)),
        repo,
        "mount the actual repository owner recorded by the backend"
    );
    for language in [Lang::En, Lang::Ja] {
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            app.set_lang(language, cx);
            app.bottom_panel_open = true;
            app.bottom_tab = BottomTab::OperationLog;
            app.bottom_panel_height = 500.;
            cx.notify();
        });
        let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
        let restore_events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let captured_events = restore_events.clone();
        app.update(cx, |_, cx| {
            cx.subscribe(
                &panel,
                move |_, _, event: &kagi::ui::oplog_panel::OpLogPanelEvent, _| {
                    captured_events.borrow_mut().push(event.clone());
                },
            )
            .detach();
        });
        for op in ["fetch", "lock"] {
            let mut empty = recorded.clone();
            empty.op = op.into();
            empty.ref_moves = Some(Vec::new());
            panel.update(cx, |panel, cx| {
                *panel = OpLogPanel::from_entries([empty].into());
                panel.toggle_expanded(0);
                cx.notify();
            });
            paint(cx, window);
            let disabled = bounds(window, "oplog-revert-0-disabled");
            bounds(window, "oplog-revert-reason-0");
            crate::native_ax::activate(cx);
            paint(cx, window);
            let reason = crate::native_ax::node(cx, window, ("oplog-revert-reason", 0usize).into());
            let reason_text = reason["aria"]["label"]
                .as_str()
                .expect("native unavailable reason");
            assert!(
                reason_text.contains("ref"),
                "the matching-owner empty record explains its missing ref moves"
            );
            let revert_ax = crate::native_ax::node(cx, window, "oplog-revert-0".into());
            assert_eq!(revert_ax["aria"]["role"], "Button");
            assert_eq!(revert_ax["aria"]["description"], reason["aria"]["label"]);
            bounds(window, "oplog-restore-0-enabled");
            cx.simulate_click(window, disabled.center(), Modifiers::none());
            paint(cx, window);
            assert!(
                cx.read(|cx| app.read(cx).oplog_restore_modal().is_none()),
                "empty Revert cannot open rejection card"
            );
            let restore = bounds(window, "oplog-restore-0-enabled");
            cx.simulate_click(window, restore.center(), Modifiers::none());
            paint(cx, window);
            assert!(
                cx.read(|cx| app.read(cx).oplog_restore_modal().is_some()),
                "Restore independently replans"
            );
            crate::recovery_operations::press_key(cx, &app, window, "escape");
        }
        panel.update(cx, |panel, cx| {
            *panel = OpLogPanel::from_entries([recorded.clone()].into());
            panel.toggle_expanded(0);
            cx.notify();
        });
        paint(cx, window);
        let revert = bounds(window, "oplog-revert-0-enabled");
        cx.simulate_click(window, revert.center(), Modifiers::none());
        paint(cx, window);
        cx.read(|cx| {
            let state = app.read(cx);
            let modal = state
                .oplog_restore_modal()
                .expect("matching-owner nonempty record opens its real revert plan");
            assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
        });
        crate::recovery_operations::press_key(cx, &app, window, "escape");
        let restore_events = restore_events.borrow().clone();
        assert!(
            restore_events.iter().any(|event| matches!(
                event,
                kagi::ui::oplog_panel::OpLogPanelEvent::Restore {
                    op: Operation::OpRevert { .. },
                    ..
                }
            )) && restore_events.iter().any(|event| matches!(
                event,
                kagi::ui::oplog_panel::OpLogPanelEvent::Restore {
                    op: Operation::RestoreToPoint { .. },
                    ..
                }
            )),
            "both matching-owner actions reach the actual planning event"
        );
        let other = build_fixture();
        let unchanged_owner = std::fs::read(repo.join(".git/index")).unwrap();
        let unchanged_other = std::fs::read(other.path().join(".git/index")).unwrap();
        for context in ["other-repository", "home", "remote", "closed-owner"] {
            app.update(cx, |app, cx| {
                assert!(app.open_repository(repo.clone(), cx));
            });
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| match context {
                    "other-repository" => {
                        assert!(app.open_repository(other.path().to_path_buf(), cx));
                    }
                    "home" => app.open_home_tab(window, cx),
                    "remote" => app.enter_remote_view(
                        kagi_domain::remote::RemoteHost {
                            user: None,
                            host: "log-inspection.invalid".into(),
                            port: None,
                            identity_file: None,
                        },
                        "/srv/log-inspection".into(),
                        Backend::open(&repo).unwrap().snapshot(10_000).unwrap(),
                        cx,
                    ),
                    "closed-owner" => {
                        for index in (0..app.tabs.len()).rev() {
                            app.close_tab(index, cx);
                        }
                    }
                    _ => unreachable!(),
                });
            })
            .unwrap();
            assert_inspection_only(cx, &app, window, &recorded, &restore_events);
            assert_eq!(
                std::fs::read(repo.join(".git/index")).unwrap(),
                unchanged_owner
            );
            assert_eq!(
                std::fs::read(other.path().join(".git/index")).unwrap(),
                unchanged_other
            );
        }
        drop(panel);
        unmount(cx, app, window);
    }
}

fn assert_inspection_only(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    window: AnyWindowHandle,
    recorded: &kagi_git::oplog::OpLogEntry,
    restore_events: &[kagi::ui::oplog_panel::OpLogPanelEvent],
) {
    let before_receipts = kagi_git::oplog::read_oplog_tail(200).len();
    paint(cx, window);
    bounds(window, "oplog-revert-reason-0");
    for action in ["revert", "restore"] {
        let disabled = bounds(window, &format!("oplog-{action}-0-disabled"));
        cx.simulate_click(window, disabled.center(), Modifiers::none());
        paint(cx, window);
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(
                state.oplog_restore_modal().is_none(),
                "unavailable restoration cannot open a plan"
            );
            assert!(
                !state.app_sessions.has_leases(),
                "inspection cannot admit a writer"
            );
            let panel = state.op_log.as_ref().unwrap().read(cx);
            assert_eq!(panel.expanded(), Some(0), "the receipt remains inspectable");
            assert_eq!(panel.entries()[0].id, recorded.id);
        });
    }
    // Replay genuine enabled-row events after the screen/owner changed.
    // A disabled renderer alone must not be the write-admission guard.
    let panel = app.read_with(cx, |app, _| app.op_log.clone().unwrap());
    for event in restore_events {
        panel.update(cx, |_, cx| cx.emit(event.clone()));
        cx.run_until_parked();
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(
                state.oplog_restore_modal().is_none(),
                "stale restore event is refused"
            );
            assert!(!state.app_sessions.has_leases());
        });
    }
    let copy = bounds(window, "oplog-copy-0");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("before-copy".into()));
    cx.simulate_mouse_move(window, copy.center(), None, Modifiers::none());
    paint(cx, window);
    cx.simulate_click(window, copy.center(), Modifiers::none());
    cx.run_until_parked();
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("copied receipt");
    assert_ne!(copied, "before-copy", "the actual Copy button writes the clipboard");
    let moves = recorded.ref_moves.as_ref().expect("recorded ref moves");
    assert!(!moves.is_empty());
    for movement in moves {
        assert!(
            copied.contains(&movement.refname),
            "inspection Copy retains the complete ref name: {copied}"
        );
        for oid in movement.old.iter().chain(movement.new.iter()) {
            assert!(
                copied.contains(oid),
                "inspection Copy retains full ref-move OIDs: {copied}"
            );
        }
    }
    assert_eq!(
        kagi_git::oplog::read_oplog_tail(200).len(),
        before_receipts,
        "unavailable restoration must not manufacture a write receipt"
    );
}
