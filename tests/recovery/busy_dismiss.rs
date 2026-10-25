//! #1719: close the existing progress card, never its work or final result.
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, i18n::Lang, theme, FooterStatus, KagiApp};
use kagi_git::oplog::{self, OpOutcome};

use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};
use crate::pull_freshness::{Origin, SettingsGuard};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_output};

const TICK: Duration = Duration::from_millis(250);

struct ThemeGuard {
    slug: String,
    _saved: crate::gui_isolation::SavedKeys,
}

impl ThemeGuard {
    fn keep() -> Self {
        Self {
            slug: theme::theme().slug.to_string(),
            _saved: crate::gui_isolation::SavedKeys::keep(&["theme"]),
        }
    }
}

impl Drop for ThemeGuard {
    fn drop(&mut self) {
        theme::set_active(&self.slug);
    }
}

pub(super) fn wait(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    what: &str,
    ready: impl Fn(&KagiApp) -> bool,
) {
    wait_ui(cx, app, what, |state, _| ready(state));
}

pub(super) fn wait_ui(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    what: &str,
    ready: impl Fn(&KagiApp, &gpui::App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| ready(app.read(cx), cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn advance(cx: &mut VisualTestAppContext, ticks: u32) {
    for _ in 0..ticks {
        cx.advance_clock(TICK);
        cx.run_until_parked();
    }
}

pub(super) fn paint(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    for id in [
        "busy-snackbar",
        "busy-snackbar-advice",
        "busy-snackbar-skip",
        "busy-snackbar-dismiss",
    ] {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        window.draw(cx).clear();
    })
    .unwrap();
}

pub(super) fn assert_hidden(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    paint(cx, app, window);
    for id in [
        "busy-snackbar",
        "busy-snackbar-dismiss",
        "busy-snackbar-skip",
    ] {
        assert!(
            e2e::control_bounds(window.window_id(), id).is_none(),
            "hidden progress must not retain {id}'s painted/hit bounds"
        );
    }
}

pub(super) fn close_by_pointer(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    paint(cx, app, window);
    let close = e2e::control_bounds(window.window_id(), "busy-snackbar-dismiss")
        .expect("the running progress card has a real close Button");
    cx.simulate_click(window, close.center(), Modifiers::none());
    cx.run_until_parked();
    assert_hidden(cx, app, window);
}

fn focus_by_keyboard(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> gpui::FocusHandle {
    paint(cx, app, window);
    let focus = cx
        .read(|cx| e2e::busy_dismiss_focus(app.read(cx), window.window_id()))
        .expect("the rendered component owns a retained focus handle");
    let mut reached = false;
    for _ in 0..150 {
        cx.simulate_keystrokes(window, "tab");
        cx.run_until_parked();
        if cx
            .update_window(window, |_, window, _| focus.is_focused(window))
            .unwrap()
        {
            reached = true;
            break;
        }
    }
    assert!(reached, "production Tab navigation reaches progress close");
    focus
}

fn close_by_keyboard(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    activation: &str,
) {
    let focus = focus_by_keyboard(cx, app, window);
    cx.simulate_keystrokes(window, activation);
    cx.simulate_event(
        window,
        gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse(activation).unwrap(),
        },
    );
    cx.run_until_parked();
    assert_hidden(cx, app, window);
    let root = cx.read(|cx| app.read(cx).root_focus.clone().unwrap());
    cx.update_window(window, |_, window, _| {
        assert!(
            !focus.is_focused(window),
            "removed close releases its focus"
        );
        assert!(
            root.is_focused(window),
            "the existing removal event repairs focus"
        );
    })
    .unwrap();
}

pub(super) fn ready(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    wait(cx, app, "the current repository read completes", |state| {
        state.active_session().is_some_and(|owner| {
            !state.reads.is_loading(owner) && state.slow_reads_inflight_for_e2e(owner) == 0
        })
    });
}

// Exercise coalescing at the existing delivered-menu-action boundary.
// Live controls remain disabled: this is not evidence of a fresh native click.
fn deliver_pull_intent(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    paint(cx, app, window);
    assert_eq!(e2e::toolbar_unavailable("tb-pull"), Some(true));
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("repo.pull", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
}

pub fn scenario_busy_read_dismiss(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let _theme = ThemeGuard::keep();
    let _layout = crate::recovery_layout::GlobalSettings::capture();
    theme::set_zoom(1.);
    for preset in ["apple-light", "apple-dark"] {
        assert!(theme::set_active(preset));
        for language in [Lang::En, Lang::Ja] {
            let fixture = build_fixture();
            let other = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let _origin = Origin::new(&repo);
            git(&repo, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
            let receipts = oplog::read_oplog_tail_for_repo(&repo, 100).len();
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| app.set_lang(language, cx));
            ready(cx, &app);
            let owner_a = cx.read(|cx| app.read(cx).active_session().unwrap());
            let (first, release_first) = deferred::<()>(cx);
            KagiApp::hold_next_snapshot_for_e2e(first);
            app.update(cx, |app, cx| app.reload_external(cx));
            let (second, release_second) = deferred::<()>(cx);
            KagiApp::hold_next_snapshot_for_e2e(second);
            app.update(cx, |app, cx| app.reload_external(cx));
            cx.run_until_parked();
            advance(cx, 10);
            cx.read(|cx| {
                assert_eq!(app.read(cx).slow_reads_inflight_for_e2e(owner_a), 2);
            });
            close_by_keyboard(
                cx,
                &app,
                window,
                if language == Lang::En {
                    "enter"
                } else {
                    "space"
                },
            );
            advance(cx, 20);
            assert_hidden(cx, &app, window);
            cx.read(|cx| {
                assert_eq!(app.read(cx).slow_reads_inflight_for_e2e(owner_a), 2);
                assert!(!app.read(cx).app_sessions.has_leases());
            });

            app.update(cx, |app, cx| {
                assert!(app.open_repository(other.path().to_path_buf(), cx));
            });
            ready(cx, &app);
            let owner_b = cx.read(|cx| app.read(cx).active_session().unwrap());
            assert_ne!(owner_a, owner_b);
            let (third, release_third) = deferred::<()>(cx);
            KagiApp::hold_next_snapshot_for_e2e(third);
            app.update(cx, |app, cx| app.reload_external(cx));
            cx.run_until_parked();
            advance(cx, 10);
            paint(cx, &app, window);
            assert!(e2e::control_bounds(window.window_id(), "busy-snackbar-dismiss").is_some());
            app.update(cx, |app, cx| app.switch_repo(0, cx));
            wait(cx, &app, "A's new revalidation completes", |state| {
                !state.reads.is_loading(owner_a) && state.slow_reads_inflight_for_e2e(owner_a) == 2
            });
            assert_hidden(cx, &app, window);
            app.update(cx, |app, cx| app.switch_repo(1, cx));
            wait(cx, &app, "B's new revalidation completes", |state| {
                !state.reads.is_loading(owner_b) && state.slow_reads_inflight_for_e2e(owner_b) == 1
            });
            paint(cx, &app, window);
            assert!(e2e::control_bounds(window.window_id(), "busy-snackbar-dismiss").is_some());
            close_by_pointer(cx, &app, window);
            release_first.send(());
            release_second.send(());
            release_third.send(());
            wait(cx, &app, "all hidden reads really finish", |state| {
                state.slow_reads_inflight_for_e2e(owner_a) == 0
                    && state.slow_reads_inflight_for_e2e(owner_b) == 0
            });
            advance(cx, 1);
            assert_hidden(cx, &app, window);
            app.update(cx, |app, cx| app.switch_repo(0, cx));
            ready(cx, &app);
            cx.read(|cx| {
                let counts = app.read(cx).view().branch_upstream_info["main"].counts;
                assert_eq!(
                    counts,
                    Some(kagi_git::AheadBehind {
                        ahead: 1,
                        behind: 0
                    })
                );
            });
            let (next, release_next) = deferred::<()>(cx);
            KagiApp::hold_next_snapshot_for_e2e(next);
            app.update(cx, |app, cx| app.reload_external(cx));
            cx.run_until_parked();
            advance(cx, 10);
            paint(cx, &app, window);
            assert!(e2e::control_bounds(window.window_id(), "busy-snackbar-dismiss").is_some());
            crate::native_ax::activate(cx);
            paint(cx, &app, window);
            assert!(crate::native_ax::press(
                cx,
                window,
                kagi::ui::i18n::Msg::BusyHideProgress.t()
            ));
            cx.run_until_parked();
            assert_hidden(cx, &app, window);
            release_next.send(());
            ready(cx, &app);
            let (finishing, release_finishing) = deferred::<()>(cx);
            KagiApp::hold_next_snapshot_for_e2e(finishing);
            app.update(cx, |app, cx| app.reload_external(cx));
            cx.run_until_parked();
            advance(cx, 10);
            let focus = focus_by_keyboard(cx, &app, window);
            release_finishing.send(());
            ready(cx, &app);
            advance(cx, 1);
            assert_hidden(cx, &app, window);
            let root = cx.read(|cx| app.read(cx).root_focus.clone().unwrap());
            cx.update_window(window, |_, window, _| {
                assert!(!focus.is_focused(window));
                assert!(root.is_focused(window), "natural completion repairs focus");
            })
            .unwrap();
            assert_eq!(oplog::read_oplog_tail_for_repo(&repo, 100).len(), receipts);
            unmount(cx, app, window);
        }
    }
}

pub fn scenario_busy_pull_progress_dismiss(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let _theme = ThemeGuard::keep();
    for language in [Lang::En, Lang::Ja] {
        for branch_ff in [false, true] {
            let fixture = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let origin = Origin::new(&repo);
            let before_head = git_output(&repo, &["rev-parse", "HEAD"]);
            let branch = if branch_ff { "progress-other" } else { "main" };
            if branch_ff {
                git(&repo, &["branch", "--track", branch, "origin/main"]);
            }
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                app.set_lang(language, cx);
                app.set_window_active_for_e2e(true, cx);
            });
            ready(cx, &app);
            let expected = origin.advance();
            let (fetch_hold, fetch_release) = deferred::<()>(cx);
            let (run_hold, run_release) = deferred::<()>(cx);
            KagiApp::hold_next_fetch_for_e2e(fetch_hold);
            KagiApp::hold_next_run_for_e2e(run_hold);
            let click = |cx: &mut VisualTestAppContext| {
                if branch_ff {
                    crate::pull_freshness::click_branch_sync(cx, window, branch, true);
                } else {
                    crate::pull_freshness::click_pull(cx, window);
                }
            };
            click(cx);
            cx.run_until_parked();
            let (fetch_id, requested_at) = cx.read(|cx| {
                let state = app.read(cx);
                (
                    state.app_sessions.running_lease().unwrap().0,
                    state.fetch_in_flight.as_ref().unwrap().waiters[0].3,
                )
            });
            close_by_pointer(cx, &app, window);
            if !branch_ff {
                deliver_pull_intent(cx, &app, window);
                cx.run_until_parked();
                assert_hidden(cx, &app, window);
                cx.read(|cx| {
                    let state = app.read(cx);
                    assert_eq!(state.fetch_in_flight.as_ref().unwrap().waiters.len(), 1);
                    assert_eq!(state.app_sessions.running_lease().unwrap().0, fetch_id);
                });
            }
            fetch_release.send(());
            wait(
                cx,
                &app,
                "freshness Fetch hands off to the admitted Pull",
                |state| {
                    state.fetch_in_flight.is_none()
                        && state
                            .app_sessions
                            .running_lease()
                            .is_some_and(|(id, _)| id != fetch_id)
                },
            );
            assert_hidden(cx, &app, window);
            cx.read(|cx| assert!(e2e::op_latched(app.read(cx))));
            run_release.send(());
            wait(cx, &app, "hidden Pull completes and retires", |state| {
                !state.app_sessions.has_leases()
                    && state.write_busy_op.is_none()
                    && !matches!(state.status_footer, FooterStatus::Busy(_))
            });
            assert_eq!(git_output(&repo, &["rev-parse", branch]), expected);
            if branch_ff {
                assert_eq!(git_output(&repo, &["rev-parse", "HEAD"]), before_head);
            }
            assert!(git_output(&repo, &["status", "--porcelain"]).is_empty());
            let entries = oplog::read_oplog_tail_for_repo(&repo, 100);
            let pull: Vec<_> = entries
                .iter()
                .filter(|entry| matches!(entry.op.as_str(), "pull" | "branch-pull-ff"))
                .collect();
            assert_eq!(pull.len(), 1);
            assert!(matches!(pull[0].outcome, OpOutcome::Success { .. }));
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(matches!(state.status_footer, FooterStatus::Success(_)));
                assert!(state
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .toasts()
                    .is_empty());
            });

            // No advance_clock between the two requests: a reused request time
            // is not a permanent identity and must not inherit old suppression.
            let (next, release_next) = deferred::<()>(cx);
            KagiApp::hold_next_fetch_for_e2e(next);
            click(cx);
            cx.run_until_parked();
            cx.read(|cx| {
                assert_eq!(
                    app.read(cx).fetch_in_flight.as_ref().unwrap().waiters[0].3,
                    requested_at
                );
            });
            paint(cx, &app, window);
            assert!(e2e::control_bounds(window.window_id(), "busy-snackbar-dismiss").is_some());
            release_next.send(());
            wait(cx, &app, "the next no-op Pull settles normally", |state| {
                state.fetch_in_flight.is_none() && !state.app_sessions.has_leases()
            });
            assert_eq!(
                oplog::read_oplog_tail_for_repo(&repo, 100)
                    .iter()
                    .filter(|entry| matches!(entry.op.as_str(), "pull" | "branch-pull-ff"))
                    .count(),
                1,
                "a new no-op request still does not invent a write receipt"
            );
            unmount(cx, app, window);
        }
    }
}

pub fn scenario_busy_pull_revisit_request(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for language in [Lang::En, Lang::Ja] {
        let fixture = build_fixture();
        let other = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let origin = Origin::new(&repo);
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            app.set_lang(language, cx);
            app.set_window_active_for_e2e(true, cx);
        });
        ready(cx, &app);
        let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
        app.update(cx, |app, cx| {
            assert!(app.open_repository(other.path().to_path_buf(), cx));
        });
        ready(cx, &app);
        app.update(cx, |app, cx| app.switch_repo(0, cx));
        ready(cx, &app);
        let expected = origin.advance();
        let (fetch, release_fetch) = deferred::<()>(cx);
        let (run, release_run) = deferred::<()>(cx);
        KagiApp::hold_next_fetch_for_e2e(fetch);
        KagiApp::hold_next_run_for_e2e(run);
        crate::pull_freshness::click_pull(cx, window);
        cx.run_until_parked();
        let (lease, old_visit) = cx.read(|cx| {
            let state = app.read(cx);
            (
                state.app_sessions.running_lease().unwrap().0,
                state.fetch_in_flight.as_ref().unwrap().waiters[0].1,
            )
        });
        close_by_pointer(cx, &app, window);
        app.update(cx, |app, cx| app.switch_repo(1, cx));
        cx.run_until_parked();
        assert_hidden(cx, &app, window);
        app.update(cx, |app, cx| app.switch_repo(0, cx));
        wait(cx, &app, "the revisited repository revalidates", |state| {
            state.active_session() == Some(owner) && !state.reads.is_loading(owner)
        });
        assert_hidden(cx, &app, window);
        deliver_pull_intent(cx, &app, window);
        cx.run_until_parked();
        cx.read(|cx| {
            let state = app.read(cx);
            let current_visit = state.app_sessions.attachment(owner).unwrap().visit;
            assert_ne!(current_visit, old_visit);
            assert_eq!(state.app_sessions.running_lease().unwrap().0, lease);
            let waiters = &state.fetch_in_flight.as_ref().unwrap().waiters;
            assert_eq!(waiters.len(), 2, "a new visit joins, not a new Fetch");
            assert!(waiters
                .iter()
                .any(|(session, visit, _, _)| { *session == owner && *visit == current_visit }));
        });
        paint(cx, &app, window);
        assert!(
            e2e::control_bounds(window.window_id(), "busy-snackbar-dismiss").is_some(),
            "the new visit never inherits the departed request's dismissal"
        );
        close_by_pointer(cx, &app, window);
        app.update(cx, |app, cx| app.switch_repo(1, cx));
        cx.run_until_parked();
        assert_hidden(cx, &app, window);
        let other_owner = cx.read(|cx| app.read(cx).active_session().unwrap());
        let (other_read, release_other) = deferred::<()>(cx);
        KagiApp::hold_next_snapshot_for_e2e(other_read);
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.run_until_parked();
        advance(cx, 10);
        paint(cx, &app, window);
        assert!(
            e2e::control_bounds(window.window_id(), "busy-snackbar-dismiss").is_some(),
            "another owner's new read is not hidden with the dismissed Fetch"
        );
        close_by_pointer(cx, &app, window);
        cx.read(|cx| {
            assert_eq!(app.read(cx).slow_reads_inflight_for_e2e(other_owner), 1);
            assert_eq!(app.read(cx).app_sessions.running_lease().unwrap().0, lease);
        });
        release_other.send(());
        ready(cx, &app);
        app.update(cx, |app, cx| app.switch_repo(0, cx));
        wait(cx, &app, "the next visit revalidates", |state| {
            state.active_session() == Some(owner) && !state.reads.is_loading(owner)
        });
        assert_hidden(cx, &app, window);
        deliver_pull_intent(cx, &app, window);
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(
                app.read(cx).fetch_in_flight.as_ref().unwrap().waiters.len(),
                3
            );
        });
        paint(cx, &app, window);
        assert!(e2e::control_bounds(window.window_id(), "busy-snackbar-dismiss").is_some());
        close_by_pointer(cx, &app, window);
        release_fetch.send(());
        wait(
            cx,
            &app,
            "the current waiter owns the Pull execution",
            |state| {
                state.fetch_in_flight.is_none()
                    && state
                        .app_sessions
                        .running_lease()
                        .is_some_and(|(id, _)| id != lease)
            },
        );
        assert_hidden(cx, &app, window);
        release_run.send(());
        wait(cx, &app, "the revisited Pull really completes", |state| {
            !state.app_sessions.has_leases()
                && state.write_busy_op.is_none()
                && !matches!(state.status_footer, FooterStatus::Busy(_))
        });
        assert_eq!(git_output(&repo, &["rev-parse", "HEAD"]), expected);
        assert!(git_output(&repo, &["status", "--porcelain"]).is_empty());
        let entries = oplog::read_oplog_tail_for_repo(&repo, 100);
        let pulls: Vec<_> = entries.iter().filter(|entry| entry.op == "pull").collect();
        assert_eq!(pulls.len(), 1);
        assert!(matches!(pulls[0].outcome, OpOutcome::Success { .. }));
        unmount(cx, app, window);
    }
}

pub fn scenario_busy_pull_merge_progress_dismiss(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for language in [Lang::En, Lang::Ja] {
        for from_pull in [true, false] {
            let fixture = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let origin = Origin::new(&repo);
            std::fs::write(repo.join("upstream.txt"), "committed local side\n").unwrap();
            git(&repo, &["add", "upstream.txt"]);
            git(&repo, &["commit", "-qm", "competing local history"]);
            let before_head = git_output(&repo, &["rev-parse", "HEAD"]);
            let incoming = origin.advance();
            if !from_pull {
                git(&repo, &["fetch", "-q", "origin"]);
            }
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                app.set_lang(language, cx);
                app.set_window_active_for_e2e(true, cx);
            });
            ready(cx, &app);
            if from_pull {
                let (fetch, release_fetch) = deferred::<()>(cx);
                KagiApp::hold_next_fetch_for_e2e(fetch);
                crate::pull_freshness::click_pull(cx, window);
                cx.run_until_parked();
                close_by_pointer(cx, &app, window);
                release_fetch.send(());
                wait(
                    cx,
                    &app,
                    "the real history-conflict Pull review appears",
                    |state| state.pull_modal().is_some() && !state.app_sessions.has_leases(),
                );
            }
            let (plan, release_plan) = deferred::<()>(cx);
            KagiApp::hold_next_merge_plan_for_e2e(plan);
            if from_pull {
                crate::recovery_operations::press_enter(cx, &app, window);
            } else {
                app.update(cx, |app, cx| {
                    app.open_merge_modal(
                        "refs/remotes/origin/main".into(),
                        Some("main".into()),
                        cx,
                    );
                });
            }
            cx.run_until_parked();
            advance(cx, 10);
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(state.planning.is_some());
                assert!(state.pull_modal().is_none());
                assert!(state.merge_modal().is_none());
                assert!(!state.app_sessions.has_leases(), "Review is not execution");
            });
            if from_pull {
                assert_hidden(cx, &app, window);
            } else {
                paint(cx, &app, window);
                assert!(
                    e2e::control_bounds(window.window_id(), "busy-snackbar-dismiss").is_some(),
                    "an independent Merge never inherits Pull's dismissal"
                );
                close_by_pointer(cx, &app, window);
            }
            assert_eq!(git_output(&repo, &["rev-parse", "HEAD"]), before_head);
            assert!(git_output(&repo, &["status", "--porcelain"]).is_empty());
            release_plan.send(());
            wait(cx, &app, "the real merge confirmation arrives", |state| {
                state.planning.is_none() && state.merge_modal().is_some()
            });
            cx.read(|cx| {
                assert!(matches!(
                    app.read(cx).merge_modal().unwrap().kind,
                    kagi_git::MergeKind::Conflicts(_)
                ));
            });
            paint(cx, &app, window);
            if !from_pull {
                let cancel = e2e::control_bounds(window.window_id(), "plan-cancel").unwrap();
                cx.simulate_click(window, cancel.center(), Modifiers::none());
                cx.run_until_parked();
                cx.read(|cx| assert!(app.read(cx).merge_modal().is_none()));
                assert_eq!(git_output(&repo, &["rev-parse", "HEAD"]), before_head);
                assert!(git_output(&repo, &["status", "--porcelain"]).is_empty());
                assert!(oplog::read_oplog_tail_for_repo(&repo, 100)
                    .iter()
                    .all(|entry| entry.op != "merge"));
                let (next, release_next) = deferred::<()>(cx);
                KagiApp::hold_next_merge_plan_for_e2e(next);
                app.update(cx, |app, cx| {
                    app.open_merge_modal(
                        "refs/remotes/origin/main".into(),
                        Some("main".into()),
                        cx,
                    );
                });
                cx.run_until_parked();
                paint(cx, &app, window);
                assert!(e2e::control_bounds(window.window_id(), "busy-snackbar-dismiss").is_some());
                release_next.send(());
                wait(
                    cx,
                    &app,
                    "a new normal Merge still owns its confirmation",
                    |state| state.merge_modal().is_some() && state.planning.is_none(),
                );
                paint(cx, &app, window);
                let cancel = e2e::control_bounds(window.window_id(), "plan-cancel").unwrap();
                cx.simulate_click(window, cancel.center(), Modifiers::none());
            } else {
                let (run, release_run) = deferred::<()>(cx);
                KagiApp::hold_next_run_for_e2e(run);
                let confirm = e2e::control_bounds(window.window_id(), "plan-confirm").unwrap();
                cx.simulate_click(window, confirm.center(), Modifiers::none());
                cx.run_until_parked();
                advance(cx, 10);
                cx.read(|cx| {
                    assert!(app.read(cx).app_sessions.running_lease().is_some());
                    assert!(app.read(cx).merge_modal().is_none());
                });
                assert_hidden(cx, &app, window);
                assert_eq!(git_output(&repo, &["rev-parse", "HEAD"]), before_head);
                assert!(git_output(&repo, &["status", "--porcelain"]).is_empty());
                release_run.send(());
                wait(
                    cx,
                    &app,
                    "the hidden merge really enters Conflict Mode",
                    |state| {
                        !state.app_sessions.has_leases()
                            && state.write_busy_op.is_none()
                            && state.ui().conflict.is_some()
                    },
                );
                assert_eq!(git_output(&repo, &["rev-parse", "HEAD"]), before_head);
                assert_eq!(git_output(&repo, &["rev-parse", "MERGE_HEAD"]), incoming);
                assert!(git_output(&repo, &["ls-files", "--unmerged"]).contains("upstream.txt"));
                let entries = oplog::read_oplog_tail_for_repo(&repo, 100);
                assert_eq!(
                    entries.iter().filter(|entry| entry.op == "merge").count(),
                    1
                );
                assert!(entries
                    .iter()
                    .all(|entry| !matches!(entry.op.as_str(), "stash-push" | "stash-pop")));
                cx.read(|cx| {
                    let state = app.read(cx);
                    assert!(!matches!(state.status_footer, FooterStatus::Busy(_)));
                    assert!(state.merge_modal().is_none());
                });
            }
            unmount(cx, app, window);
        }
    }
}
