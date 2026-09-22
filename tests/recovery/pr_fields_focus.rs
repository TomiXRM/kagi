//! #755: PR Fields Cancel/Apply must leave Escape routing live after the focused
//! filter disappears. A queued AppNotice is the observable target after each
//! exit; no test-side refocusing repairs the path. PR reads and Apply use an
//! offline gh shim that refuses edits.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use gpui::{AnyWindowHandle, ClipboardItem, Entity, Focusable, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};

use crate::evidence_support::pull_request;
use crate::macos::{build_fixture, mount, unmount};

/// An offline transport for the real Apply dispatch, restored on Drop.
struct OfflineGh {
    previous: Option<OsString>,
    _bin: tempfile::TempDir,
}

impl OfflineGh {
    fn install() -> Self {
        // Do not poison the process-global OnceLock with this test transport.
        let _ = kagi_git::github::gh_available();
        let bin = tempfile::tempdir().expect("offline gh dir");
        let gh = bin.path().join("gh");
        // Candidate reads succeed; edits fail without reaching GitHub.
        std::fs::write(
            &gh,
            r#"#!/bin/sh
case "$1" in
  api) echo octocat; echo hubot ;;
  pr)
    case "$2" in
      edit) echo 'refused: octocat cannot be requested for review' >&2; exit 1 ;;
      *) echo '{}' ;;
    esac ;;
  *) echo 'no GitHub repository here' >&2; exit 1 ;;
esac
"#,
        )
        .expect("write offline gh");
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700))
            .expect("offline gh permissions");
        let previous = std::env::var_os("PATH");
        let mut paths = vec![bin.path().to_path_buf()];
        paths.extend(std::env::split_paths(
            previous.as_deref().unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).expect("join PATH"));
        Self {
            previous,
            _bin: bin,
        }
    }
}

impl Drop for OfflineGh {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Exit {
    Cancel,
    Apply,
}

impl Exit {
    fn control(self) -> &'static str {
        match self {
            Self::Cancel => "pr-fields-cancel",
            Self::Apply => "pr-fields-confirm",
        }
    }
}

/// Collect both exits before asserting, so the pre-fix run measures both.
struct Observation {
    exit: Exit,
    escape_route_live: bool,
    notice_closed: bool,
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw PR field picker window");
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    e2e::clear_control_bounds(window.window_id(), id);
    paint(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), id)
        .unwrap_or_else(|| panic!("{id} was not laid out"));
    cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

/// The PR ref fetch must settle before Apply can be admitted.
fn wait_idle(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).write_busy_op.is_none()) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the PR tab's ref fetch never settled"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn pr_edit_entries(repo: &Path) -> Vec<OpOutcome> {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "pr-edit")
        .map(|entry| entry.outcome)
        .collect()
}

/// Independently mount each exit so neither inherits the other's window focus.
fn exit_leg(cx: &mut VisualTestAppContext, exit: Exit) -> Observation {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().expect("fixture path");
    let (app, window) = mount(cx, &repo);
    let pr = pull_request(7, "field picker", "main");

    // Supply the conversation read; the remaining PR reads use offline gh.
    e2e::queue_github_pr_conversation(gpui::Task::ready((
        Ok((Vec::new(), Vec::new())),
        Ok(Vec::new()),
    )));
    app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
    wait_idle(cx, &app);
    paint(cx, window);
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("PR mode is open");
        assert!(mode.active.is_some(), "the PR opened its own tab");
    });

    app.update(cx, |app, cx| {
        app.open_pr_fields_modal(kagi::ui::modals::PrField::Reviewers, cx)
    });
    paint(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).pr_fields_modal().is_some()),
        "precondition: the picker owns the modal slot"
    );

    // Paste into the input the product focused, without a test-side focus call.
    app.update(cx, |_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("oct".into()));
    });
    cx.simulate_keystrokes(window, "cmd-v");
    paint(cx, window);
    // Do not retain an input entity or focus handle across its real teardown.
    let (filter_text, filter_focused) = {
        let input = cx
            .read(|cx| app.read(cx).pr_fields_input.clone())
            .expect("the picker mounts its filter input");
        let text = cx.read(|cx| input.read(cx).value().to_string());
        let focused = cx
            .update_window(window, |_, window, cx| {
                input.read(cx).focus_handle(cx).is_focused(window)
            })
            .expect("read window focus");
        (text, focused)
    };
    assert_eq!(
        filter_text, "oct",
        "precondition: the picker's own filter input took the keystrokes"
    );
    assert!(
        filter_focused,
        "precondition: the window is focused on the filter input, not the root"
    );
    assert!(
        cx.update_window(window, |_, window, cx| window
            .is_action_available(&kagi::ui::CloseMainDiff, cx))
            .expect("read action availability"),
        "precondition: from the picker's live focus, Escape still reaches the modal slot"
    );

    // Use the row's selection handler to enable Apply.
    app.update(cx, |app, cx| app.pr_fields_toggle("octocat".into(), cx));
    paint(cx, window);

    // Queue the notice while the picker still owns the slot.
    let message = format!("held behind the field picker ({exit:?})");
    app.update(cx, |app, cx| {
        e2e::deliver_app_notice(app, &message);
        cx.notify();
    });
    assert!(
        cx.read(|cx| e2e::app_notice_message(app.read(cx)).is_none()
            && e2e::queued_notice_contains(app.read(cx), &message)),
        "precondition: an arriving notice queues behind the picker instead of replacing it"
    );

    // Leave through the control the user presses. Nothing here re-focuses.
    click(cx, window, exit.control());
    assert!(
        cx.read(|cx| app.read(cx).pr_fields_modal().is_none()),
        "{:?} must remove the picker from the modal slot",
        exit
    );
    // Apply dispatches; its receipt is durable only once the writer settles.
    wait_idle(cx, &app);
    paint(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).pr_fields_input.is_none()),
        "the picker's filter input is dropped with it"
    );
    assert_eq!(
        cx.read(|cx| e2e::app_notice_message(app.read(cx)).map(str::to_string)),
        Some(message.clone()),
        "the queued notice takes the seat the picker vacated"
    );

    // Cancel dispatches nothing; Apply makes one recorded, refused attempt.
    let recorded = pr_edit_entries(&repo);
    match exit {
        Exit::Cancel => assert!(
            recorded.is_empty(),
            "cancelling the picker must dispatch nothing: {recorded:?}"
        ),
        Exit::Apply => {
            assert_eq!(recorded.len(), 1, "Apply records exactly one attempt");
            assert!(
                matches!(recorded[0], OpOutcome::Failed { .. }),
                "the offline transport refused the edit, so nothing was written: {:?}",
                recorded[0]
            );
        }
    }

    // Deliver Escape into whatever focus the exit left behind.
    let escape_route_live = cx
        .update_window(window, |_, window, cx| {
            window.is_action_available(&kagi::ui::CloseMainDiff, cx)
        })
        .expect("read action availability");
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    // A later notice taking the freed slot must not mask successful dismissal.
    let notice_closed = cx.read(|cx| e2e::app_notice_message(app.read(cx)).map(str::to_string))
        != Some(message.clone());

    unmount(cx, app, window);
    drop(fixture);
    Observation {
        exit,
        escape_route_live,
        notice_closed,
    }
}

/// Both picker exits, measured before either is judged.
pub fn scenario_pr_fields_escape_focus(cx: &mut VisualTestAppContext) {
    let _offline = OfflineGh::install();
    let observations: Vec<Observation> = [Exit::Cancel, Exit::Apply]
        .into_iter()
        .map(|exit| exit_leg(cx, exit))
        .collect();

    let report: Vec<String> = observations
        .iter()
        .map(|o| {
            format!(
                "{:?}: escape route {}, notice {}",
                o.exit,
                if o.escape_route_live {
                    "reachable"
                } else {
                    "lost"
                },
                if o.notice_closed {
                    "closed"
                } else {
                    "stayed up"
                },
            )
        })
        .collect();
    let stuck: Vec<Exit> = observations
        .iter()
        .filter(|o| !o.notice_closed)
        .map(|o| o.exit)
        .collect();
    assert!(
        stuck.is_empty(),
        "after leaving the PR field picker the window must still route Escape to \
         the modal slot, so the notice waiting behind it can be dismissed; \
         it could not after {stuck:?} — {report:?}"
    );

    eprintln!(
        "[gui-e2e] PASS pr_fields_escape_focus ({})",
        report.join("; ")
    );
}
