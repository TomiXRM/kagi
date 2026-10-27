//! Conflict shares the centre-content bottom-left stack without moving controls.
use super::*;

pub(super) fn run(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]);
    let _ports = crate::gui_isolation::PortStore::keep();
    let old_zoom = theme::zoom();
    let old_lang = i18n::lang();
    let old_theme = theme::theme().slug.to_string();
    for zoom in [1., 1.67] {
        theme::set_zoom(zoom);
        for preset in ["apple-light", "apple-dark"] {
            for lang in [i18n::Lang::En, i18n::Lang::Ja] {
                for layout in ["text", "raw", "directory-file", "saved"] {
                    run_case(cx, layout, lang, preset);
                }
            }
        }
    }
    theme::set_zoom(old_zoom);
    theme::set_active(&old_theme);
    i18n::set_lang(old_lang);
}

fn fixture(layout: &str) -> tempfile::TempDir {
    match layout {
        "raw" => crate::recovery_diff_presentation::conflict_image_fixture(),
        "directory-file" => crate::app_conflict::dir_file_fixture(),
        "saved" => {
            let fixture = crate::app_conflict::content_fixture();
            std::fs::write(fixture.path().join("file.txt"), "resolved\n").unwrap();
            git(fixture.path(), &["add", "file.txt"]);
            fixture
        }
        "text" => crate::app_conflict::content_fixture(),
        _ => unreachable!(),
    }
}

fn run_case(cx: &mut VisualTestAppContext, layout: &str, lang: i18n::Lang, preset: &str) {
    let fixture = fixture(layout);
    let index = std::fs::read(fixture.path().join(".git/index")).unwrap();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        theme::set_active(preset);
        app.set_lang(lang, cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        if layout == "saved" {
            view.conflict_open_saved(0, cx);
        } else {
            view.conflict_select_file(0, cx);
        }
        cx.notify();
    });
    cx.run_until_parked();
    for dimensions in [(1440., 900.), (940., 660.)] {
        crate::pr_detail_adaptive::resize(cx, window, dimensions);
        exercise_lane(cx, &app, window, layout);
    }
    assert_eq!(
        std::fs::read(fixture.path().join(".git/index")).unwrap(),
        index
    );
    drop(conflict);
    unmount(cx, app, window);
}

fn controls(layout: &str) -> &'static [&'static str] {
    match layout {
        "text" => &[
            "conflict-hunk-0-current-use",
            "conflict-hunk-0-current-both",
            "conflict-hunk-0-incoming-use",
            "conflict-hunk-0-incoming-both",
        ],
        "raw" => &["conflict-raw-current-use", "conflict-raw-incoming-use"],
        "directory-file" => &["conflict-directory-file-choices"],
        "saved" => &["conflict-saved-preview"],
        _ => unreachable!(),
    }
}

fn bounds(window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), id)
        .unwrap_or_else(|| panic!("{id} must be actually painted"))
}

fn control_bounds(window: AnyWindowHandle, layout: &str) -> Vec<Bounds<Pixels>> {
    controls(layout)
        .iter()
        .map(|id| {
            let bounds = bounds(window, id);
            assert!(bounds.size.height > px(0.), "{id} remains visible");
            bounds
        })
        .collect()
}

fn assert_safe_lane(window: AnyWindowHandle, _layout: &str) -> Bounds<Pixels> {
    let anchor = bounds(window, "toast-content-anchor");
    let stack = bounds(window, "toast-container");
    assert!((stack.left() - anchor.left() - theme::scaled_px(12.)).abs() <= px(2.));
    assert!((anchor.bottom() - stack.bottom() - theme::scaled_px(12.)).abs() <= px(2.));
    assert!(stack.top() >= anchor.top() && stack.right() <= anchor.right());
    assert!(stack.size.width <= anchor.size.width * 0.45 + px(1.));
    let sidebar = bounds(window, "conflict-sidebar-clip");
    let footer = bounds(window, "pr-menu-footer");
    assert!(stack.left() >= sidebar.right());
    assert!(stack.bottom() <= footer.top());
    anchor
}

fn paint_feedback(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for id in [
        "toast-stack",
        "toast-container",
        "busy-snackbar",
        "busy-snackbar-skip",
        "busy-snackbar-dismiss",
    ] {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    draw(cx, window);
}

fn assert_safe_footer(window: AnyWindowHandle, layout: &str) -> Bounds<Pixels> {
    let footer = bounds(window, "pr-menu-footer");
    let message = e2e::footer_message_bounds();
    let text = bounds(window, "status-footer-text");
    assert!(text.size.height > px(0.) && text.size.width > px(0.));
    assert!(message.top() >= footer.top() && message.bottom() <= footer.bottom());
    assert!(text.top() >= footer.top() && text.bottom() <= footer.bottom());
    assert!(text.size.height <= theme::scaled_px(22.) + px(1.));
    for id in controls(layout) {
        let control = bounds(window, id);
        assert!(
            footer.top() >= control.bottom(),
            "{layout}: footer feedback must not intersect {id}"
        );
    }
    footer
}

fn exercise_lane(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
    layout: &str,
) {
    draw(cx, window);
    let original = control_bounds(window, layout);
    let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
    let errors: Vec<_> = stack.update(cx, |stack, cx| {
        (0..3)
            .map(|index| {
                stack.push_notify(
                    kagi::ui::ToastKind::Error,
                    format!("Persistent {layout} lane fixture {index}"),
                    cx,
                );
                stack.toasts().last().unwrap().id
            })
            .collect()
    });
    cx.run_until_parked();
    // Native accessibility is lazy: activate before painting the tree that
    // supplies the real disclosure Press action.
    crate::native_ax::activate(cx);
    draw(cx, window);
    let lane = assert_safe_lane(window, layout);
    let collapsed = bounds(window, "toast-container");
    let more = i18n::Msg::ToastMore.t().replace("{count}", "2");
    assert!(
        crate::native_ax::press(cx, window, &more),
        "Conflict disclosure remains reachable"
    );
    cx.run_until_parked();
    draw(cx, window);
    assert_eq!(
        crate::native_ax::node(cx, window, ("toast-summary", 0u64).into())["aria"]["expanded"],
        true,
        "native disclosure expands the shared Conflict stack"
    );
    let expanded = bounds(window, "toast-container");
    assert!((expanded.bottom() - collapsed.bottom()).abs() <= px(1.));
    assert!(expanded.top() <= collapsed.top());
    assert_eq!(assert_safe_lane(window, layout), lane);
    // Expansion must expose actual older cards, not only flip aria-expanded.
    // Reach the oldest card through the bounded scroll viewport.
    let oldest_message = format!("toast-message-{}", errors[0]);
    e2e::clear_control_bounds(window.window_id(), &oldest_message);
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: gpui::point(expanded.center().x, expanded.bottom() - px(2.)),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-600.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    draw(cx, window);
    let oldest = bounds(window, &oldest_message);
    assert!(
        oldest.bottom() > expanded.top() && oldest.top() < expanded.bottom(),
        "expanded Conflict stack exposes the oldest card through its bounded viewport"
    );
    crate::native_ax::node(cx, window, ("toast-dismiss", errors[0]).into());
    assert_eq!(assert_safe_lane(window, layout), lane);
    let shifted = control_bounds(window, layout);
    assert_eq!(
        shifted, original,
        "persistent feedback does not move conflict controls"
    );
    let original_footer = assert_safe_footer(window, layout);
    let original_text = e2e::footer_display_text();
    for kind in [kagi::ui::ToastKind::Info, kagi::ui::ToastKind::Success] {
        let transient = stack.update(cx, |stack, cx| {
            stack.push_notify(kind, "Unrelated transient fixture\nnot a second line", cx);
            stack.toasts().last().unwrap().id
        });
        cx.run_until_parked();
        paint_feedback(cx, window);
        assert_eq!(assert_safe_lane(window, layout), lane);
        assert_eq!(control_bounds(window, layout), shifted);
        assert_eq!(assert_safe_footer(window, layout), original_footer);
        assert_eq!(
            e2e::footer_display_text(),
            original_text,
            "transient feedback no longer routes through the footer"
        );
        let status =
            kagi::ui::toast_stack::status_for_e2e(window.window_id(), ToastAudience::Repository)
                .expect("actual arrival status");
        assert_eq!(status.role, Some(gpui::Role::Status));
        assert_eq!(status.newest_id, Some(transient));
        assert!(status
            .label
            .as_deref()
            .unwrap()
            .starts_with("Unrelated transient fixture"));
        stack.update(cx, |stack, _| {
            stack.advance_age_for_e2e(Duration::from_secs(4));
            assert!(stack.expiring_ids().contains(&transient));
        });
        cx.advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        paint_feedback(cx, window);
        assert_eq!(e2e::footer_display_text(), original_text);
        assert_eq!(assert_safe_footer(window, layout), original_footer);
        cx.read(|cx| {
            assert!(stack
                .read(cx)
                .toasts()
                .iter()
                .all(|toast| toast.id != transient));
        });
        assert_eq!(
            kagi::ui::toast_stack::status_for_e2e(window.window_id(), ToastAudience::Repository)
                .unwrap()
                .newest_id,
            None,
            "expiry never re-announces an older persistent error"
        );
        assert_eq!(assert_safe_lane(window, layout), lane);
        assert_eq!(control_bounds(window, layout), shifted);
    }
    let (read, release) = crate::evidence_support::deferred::<()>(cx);
    KagiApp::hold_next_snapshot_for_e2e(read);
    app.update(cx, |app, cx| app.reload_external(cx));
    cx.run_until_parked();
    crate::busy_dismiss::advance(cx, 10);
    paint_feedback(cx, window);
    assert_eq!(assert_safe_lane(window, layout), lane);
    assert_eq!(control_bounds(window, layout), shifted);
    assert_eq!(assert_safe_footer(window, layout), original_footer);
    assert!(bounds(window, "busy-snackbar").size.height > px(0.));
    for id in ["busy-snackbar-skip", "busy-snackbar-dismiss"] {
        let control = bounds(window, id);
        assert!(control.size.width > px(0.) && control.size.height > px(0.));
        let container = bounds(window, "toast-container");
        assert!(
            control.top() >= container.top() && control.bottom() <= container.bottom(),
            "{id} fits the shared content stack"
        );
    }
    let control = if matches!(layout, "text" | "raw") {
        "busy-snackbar-skip"
    } else {
        "busy-snackbar-dismiss"
    };
    cx.simulate_click(
        window,
        bounds(window, control).center(),
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();
    paint_feedback(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "busy-snackbar-skip").is_none());
    assert_eq!(control_bounds(window, layout), shifted);
    release.send(());
    crate::busy_dismiss::ready(cx, app);
    paint_feedback(cx, window);
    assert_eq!(
        e2e::footer_display_text().as_ref(),
        kagi::ui::i18n::Msg::Refreshed.t(),
        "the completed read publishes its real footer result"
    );
    let original_text = e2e::footer_display_text();
    assert_eq!(assert_safe_lane(window, layout), lane);
    assert_eq!(control_bounds(window, layout), shifted);
    stack.update(cx, |stack, cx| {
        for error in errors {
            stack.begin_exit(error, cx);
        }
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    draw(cx, window);
    assert_eq!(
        control_bounds(window, layout),
        original,
        "dismissal leaves no empty lane"
    );
    stack.update(cx, |stack, cx| {
        stack.push_notify(kagi::ui::ToastKind::Info, "Older routine feedback", cx);
        stack.push_notify(kagi::ui::ToastKind::Success, "Newest routine feedback", cx);
    });
    cx.run_until_parked();
    paint_feedback(cx, window);
    assert_eq!(e2e::footer_display_text(), original_text);
    assert_safe_lane(window, layout);
    assert_eq!(control_bounds(window, layout), original);
    assert_eq!(assert_safe_footer(window, layout), original_footer);
    stack.update(cx, |stack, _| {
        stack.advance_age_for_e2e(Duration::from_secs(4))
    });
    cx.advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    paint_feedback(cx, window);
    assert_eq!(e2e::footer_display_text(), original_text);
    assert_eq!(control_bounds(window, layout), original);
}

/// Exercise production draft rescue and theme decoding in their real windows.
pub(crate) fn assert_settings_subject_isolation(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    repository: AnyWindowHandle,
) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "settings_pane",
        "settings_window_frame",
        "settings_window_position",
        "settings_window_height",
    ]);
    let _host = crate::settings_window::SettingsHostGuard::native(cx);
    let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
    let rescue_caption = cx.read(|cx| {
        let state = app.read(cx);
        format!(
            "{}\n{}",
            kagi_ui_core::i18n::Msg::conflict_draft_rescued(),
            kagi_ui_core::i18n::Msg::conflict_draft_repository(
                state
                    .repo_path
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
            )
        )
    });
    let rescue = cx.read(|cx| {
        stack
            .read(cx)
            .toasts()
            .iter()
            .find(|toast| {
                toast.subject == ToastSubject::Repository
                    && toast.message.as_ref() == rescue_caption
            })
            .expect("the real typed rescue remains live")
            .id
    });
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone().unwrap());
    conflict.update(cx, |view, cx| {
        view.conflict_select_file(0, cx);
        cx.notify();
    });
    drop(conflict);
    cx.run_until_parked();
    draw(cx, repository);
    let rescue_probe = format!("toast-dismiss-{rescue}");
    assert!(bounds(repository, &rescue_probe).size.height > px(0.));
    cx.simulate_keystrokes(repository, "cmd-,");
    cx.run_until_parked();
    let settings = cx
        .read(|cx| cx.windows())
        .into_iter()
        .find(|window| window.window_id() != repository.window_id())
        .unwrap();
    e2e::clear_control_bounds(settings.window_id(), &rescue_probe);
    draw(cx, settings);
    assert!(e2e::control_bounds(settings.window_id(), &rescue_probe).is_none());
    assert!(
        e2e::control_bounds(settings.window_id(), "toast-stack").is_none(),
        "a repository draft rescue must not create a Settings card or lane"
    );
    assert!(!cx.read(|cx| stack
        .read(cx)
        .toasts()
        .iter()
        .any(|toast| toast.subject == ToastSubject::Settings)));
    let path = theme::themes_dir()
        .unwrap()
        .join("owned-window-invalid.json");
    assert!(!path.exists(), "only create our own isolated theme fixture");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{").unwrap();
    cx.update_window(settings, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("view.reloadThemes", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    let theme_error = cx.read(|cx| {
        stack
            .read(cx)
            .toasts()
            .iter()
            .find(|toast| {
                toast.subject == ToastSubject::Settings
                    && toast.kind == kagi::ui::ToastKind::Error
                    && toast.message.contains("owned-window-invalid.json")
            })
            .expect("the real JSON decoder reports a Settings-owned error")
            .id
    });
    let theme_probe = format!("toast-dismiss-{theme_error}");
    e2e::clear_control_bounds(repository.window_id(), &theme_probe);
    e2e::clear_control_bounds(settings.window_id(), &rescue_probe);
    draw(cx, settings);
    assert!(bounds(settings, &theme_probe).size.height > px(0.));
    assert!(e2e::control_bounds(settings.window_id(), &rescue_probe).is_none());
    draw(cx, repository);
    assert!(bounds(repository, &rescue_probe).size.height > px(0.));
    assert!(e2e::control_bounds(repository.window_id(), &theme_probe).is_none());
    // Escape in the repository must ignore the newer, invisible Settings
    // notices, dismiss its own rescue, then leave Settings untouched.
    cx.update_window(repository, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
    })
    .unwrap();
    cx.simulate_keystrokes(repository, "escape");
    cx.run_until_parked();
    cx.read(|cx| {
        let stack = stack.read(cx);
        assert!(stack
            .toasts()
            .iter()
            .find(|toast| toast.id == rescue)
            .is_none_or(|toast| toast.dismissing.is_some()));
        assert!(stack
            .toasts()
            .iter()
            .any(|toast| toast.id == theme_error && toast.dismissing.is_none()));
    });
    cx.simulate_keystrokes(repository, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| stack
        .read(cx)
        .toasts()
        .iter()
        .any(|toast| toast.id == theme_error && toast.dismissing.is_none())));
    cx.update_window(settings, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    std::fs::remove_file(path).unwrap();
    assert!(theme::reload_custom_themes().is_empty());
}

/// The real restore quarantines bytes even when its receipt cannot be appended.
pub(crate) fn assert_rescue_recording_failure(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    repo: &std::path::Path,
    store: &std::path::Path,
    torn: &[u8],
) {
    let previous = store.with_file_name("buffer.json.prev");
    if previous.exists() {
        std::fs::remove_file(previous).unwrap();
    }
    std::fs::write(store, torn).unwrap();
    let lock_path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
        .join("operations.jsonl.lock");
    let lock = kagi_git::advisory_lock::lock_exclusive(&lock_path, Duration::ZERO).unwrap();
    let payload = e2e::detect_payload_for_test(repo, "main");
    drop(lock);
    let aside = store.with_file_name("buffer.json.corrupt.1");
    assert_eq!(std::fs::read(&aside).unwrap(), torn);
    let toast_count = cx.read(|cx| {
        app.read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .len()
    });
    app.update(cx, |app, cx| {
        let owner = app
            .app_sessions
            .attachment(app.active_session().unwrap())
            .unwrap();
        app.apply_conflict_detect(owner, payload, cx);
    });
    cx.read(|cx| {
        let state = app.read(cx);
        let notice = state
            .app_notice()
            .expect("unrecorded rescue requires acknowledgment");
        let expected = format!(
            "{}\n{}\n\n",
            kagi_ui_core::i18n::Msg::conflict_draft_rescued_detail(&aside.to_string_lossy()),
            kagi_ui_core::i18n::Msg::conflict_draft_repository(
                repo.file_name().unwrap().to_str().unwrap()
            )
        );
        assert!(notice.message.starts_with(&expected));
        assert!(notice
            .message
            .contains(&std::fs::TryLockError::WouldBlock.to_string()));
        let (detail, diagnostic) = notice.message.split_once("\n\n").unwrap();
        assert!(detail.contains(aside.to_str().unwrap()));
        assert!(!detail.contains("Operation Log"));
        assert!(!detail.contains("操作ログ"));
        assert!(diagnostic.contains(&std::fs::TryLockError::WouldBlock.to_string()));
        assert_eq!(
            state.toast_stack.as_ref().unwrap().read(cx).toasts().len(),
            toast_count,
            "the unrecorded rescue must not also create an ordinary card"
        );
    });
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(repo, 100)
            .iter()
            .filter(|entry| entry.op == "conflict-draft-restore")
            .count(),
        1,
        "only the earlier, recorded rescue has a receipt"
    );
    eprintln!(
        "[gui-e2e] ASSERT rescue-recording-failure {:?}/{}: retained-file detail has no log reference; recording diagnostic retained; no extra receipt",
        i18n::lang(),
        theme::theme().slug
    );
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
}
