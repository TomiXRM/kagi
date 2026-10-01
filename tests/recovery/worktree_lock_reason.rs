//! Lock reason entry is not approval: only the reviewed plan may change Git.
use std::path::Path;

use gpui::{AnyWindowHandle, ClipboardItem, Entity, VisualTestAppContext};
use kagi::ui::{e2e, i18n, KagiApp};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};

use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use crate::recovery_operations::{press_enter, press_key};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output;

fn lock_reason(repo: &Path) -> Option<String> {
    git_output(repo, &["worktree", "list", "--porcelain", "-z"])
        .split('\0')
        .find_map(|line| {
            if line == "locked" {
                Some(String::new())
            } else {
                line.strip_prefix("locked ").map(str::to_owned)
            }
        })
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    e2e::clear_control_bounds(window.window_id(), id);
    paint(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), id).expect("visible modal control");
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn enter_reason(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    reason: &str,
) {
    app.update(cx, |app, cx| {
        app.open_lock_worktree_modal("linked".into());
        cx.notify();
    });
    click(cx, window, "worktree-lock-reason-input");
    cx.simulate_keystrokes(window, "cmd-a backspace");
    if !reason.is_empty() {
        app.update(cx, |_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(reason.into()));
        });
        cx.simulate_keystrokes(window, "cmd-v");
    }
}

fn confirm_reason(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    reason: &str,
) {
    enter_reason(cx, app, window, reason);
    // Enter is delivered with the real InputState focused, without a manual
    // root-focus change or a render-time draft sync after the last input.
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_some()));
    assert_eq!(lock_reason(repo), None, "reviewing must not acquire a lock");
    paint(cx, window);
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    assert_eq!(lock_reason(repo).as_deref(), Some(reason));
    assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_none()));
    let entries = read_oplog_tail_for_repo(repo, 100);
    let entry = entries
        .iter()
        .rev()
        .find(|entry| entry.op == "lock-worktree")
        .unwrap();
    assert!(matches!(entry.outcome, OpOutcome::Success { .. }));
}

pub fn scenario_worktree_lock_reason(cx: &mut VisualTestAppContext) {
    let original_lang = i18n::lang();
    for language in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(language);
        let fixture = build_fixture();
        let linked_root = tempfile::tempdir().unwrap();
        let linked = linked_root.path().join("linked");
        git(
            fixture.path(),
            &[
                "worktree",
                "add",
                "-b",
                "reason-target",
                linked.to_str().unwrap(),
            ],
        );
        let before = repo_fingerprint(fixture.path());
        let (app, window) = mount(cx, fixture.path());

        enter_reason(cx, &app, window, "cancelled edit");
        cx.simulate_keystrokes(window, "escape");
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).worktree_lock_reason_modal().is_none()));
        assert_eq!(lock_reason(fixture.path()), None);

        enter_reason(cx, &app, window, "cancelled plan");
        click(cx, window, "worktree-lock-reason-review");
        assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_some()));
        assert_eq!(lock_reason(fixture.path()), None);
        crate::recovery_operations::press_key(cx, &app, window, "escape");
        cx.run_until_parked();
        assert_eq!(lock_reason(fixture.path()), None);

        confirm_reason(cx, &app, window, fixture.path(), "release \"QA\" — 作業中");
        app.update(cx, |app, cx| {
            app.open_unlock_worktree_modal("linked".into());
            cx.notify();
        });
        press_enter(cx, &app, window);
        cx.run_until_parked();
        assert_eq!(lock_reason(fixture.path()), None);
        confirm_reason(cx, &app, window, fixture.path(), "");
        assert_eq!(repo_fingerprint(fixture.path()), before);
        unmount(cx, app, window);
    }
    i18n::set_lang(original_lang);
    eprintln!("[gui-e2e] PASS worktree_lock_reason EN/JA input/plan cancellation, focused Enter, Unicode and blank reasons, Git porcelain and durable success");
}

/// #772 Phase 1 (ADR-0208): the terminal auto-lock is opt-in, and even when
/// on it only ever *offers* a plan. Mounted on a linked worktree:
/// - opt-in off: starting the terminal opens no card and locks nothing;
/// - opt-in on: starting the terminal opens the lock card with a Kagi token
///   reason and nothing is locked until Enter; the session owns the shell's
///   PID and spawn generation;
/// - the shell exiting is observed by the background wait (not the render
///   path): the session records the exit and the release card opens with
///   the token/identity-checked plan; a manual relock in between is refused
///   (contract B) and the manual lock survives.
pub fn scenario_terminal_auto_lock(cx: &mut VisualTestAppContext) {
    use kagi::ui::settings as theme;
    use kagi::ui::terminal::ShellExit;

    let fixture = build_fixture();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(
        fixture.path(),
        &[
            "worktree",
            "add",
            "-b",
            "auto-target",
            linked.to_str().unwrap(),
        ],
    );
    let linked = linked.canonicalize().unwrap();
    let before = repo_fingerprint(fixture.path());
    let restore_auto = theme::terminal_auto_lock();

    // ── opt-in off ───────────────────────────────────────────────────────
    theme::set_terminal_auto_lock(false);
    let (app, window) = mount(cx, &linked);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.ensure_terminal(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.lock_worktree_modal().is_none(),
            "opt-in off: no lock card"
        );
        let session = state
            .ui()
            .terminal_session
            .as_ref()
            .expect("terminal started");
        let shell = session.shell.as_ref().expect("the session owns its shell");
        assert_eq!(shell.generation, 1);
        assert!(shell.pid.is_some(), "a PID was recorded");
        assert!(shell.exit.is_none());
        assert!(session.auto_lock.is_none());
    });
    assert_eq!(
        lock_reason(fixture.path()),
        None,
        "opt-in off locked something"
    );
    unmount(cx, app, window);

    // An unconfirmed offer is not ownership. Even if a lock with the offered
    // reason appears after Cancel (for example after PID/session reuse on a
    // restart), this shell's exit must not offer to release it.
    theme::set_terminal_auto_lock(true);
    let (app, window) = mount(cx, &linked);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.ensure_terminal(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    let unconfirmed_reason = cx.read(|cx| {
        app.read(cx)
            .lock_worktree_modal()
            .expect("offer opens a confirmation card")
            .reason
            .clone()
    });
    press_key(cx, &app, window, "escape");
    assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_none()));
    git(
        fixture.path(),
        &[
            "worktree",
            "lock",
            "--reason",
            &unconfirmed_reason,
            linked.to_str().unwrap(),
        ],
    );
    app.update(cx, |state, _| {
        state
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .paste_writer
            .as_ref()
            .unwrap()
            .paste_text("exit\n");
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| {
            app.read(cx)
                .ui()
                .terminal_session
                .as_ref()
                .and_then(|session| session.shell.as_ref())
                .and_then(|shell| shell.exit.as_ref())
                .is_some()
        }) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "unconfirmed shell never exited"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        cx.read(|cx| app.read(cx).unlock_worktree_modal().is_none()),
        "a canceled acquire must not offer to release someone else's lock"
    );
    assert_eq!(
        lock_reason(fixture.path()).as_deref(),
        Some(unconfirmed_reason.as_str())
    );
    git(
        fixture.path(),
        &["worktree", "unlock", linked.to_str().unwrap()],
    );
    unmount(cx, app, window);

    // An offer still on screen when its shell exits must disappear. Enter
    // cannot subsequently acquire a lock for a shell that no longer exists.
    let (app, window) = mount(cx, &linked);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.ensure_terminal(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_some()));
    app.update(cx, |state, _| {
        state
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .paste_writer
            .as_ref()
            .unwrap()
            .paste_text("exit\n");
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| {
            app.read(cx)
                .ui()
                .terminal_session
                .as_ref()
                .unwrap()
                .shell
                .as_ref()
                .unwrap()
                .exit
                .is_some()
        }) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "shell never exited");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_none()));
    assert!(cx.read(|cx| app.read(cx).unlock_worktree_modal().is_none()));
    assert_eq!(lock_reason(fixture.path()), None);
    unmount(cx, app, window);

    // ── opt-in on: offer, no write before confirm ────────────────────────
    theme::set_terminal_auto_lock(true);
    let (app, window) = mount(cx, &linked);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.ensure_terminal(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    let (pid, token_reason) = cx.read(|cx| {
        let state = app.read(cx);
        let modal = state
            .lock_worktree_modal()
            .expect("opt-in on: the lock card opens");
        assert_eq!(modal.name, "linked");
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
        assert!(modal.reason.starts_with("kagi:auto:"), "{}", modal.reason);
        let session = state
            .ui()
            .terminal_session
            .as_ref()
            .expect("terminal started");
        assert!(session.auto_lock.is_none(), "an offer is not ownership");
        let offer = modal.auto.as_ref().expect("auto offer has provenance");
        assert_eq!(offer.target.token.reason(), modal.reason);
        assert!(offer.target.worktree.git_dir.ends_with("worktrees/linked"));
        (
            session.shell.as_ref().unwrap().pid.unwrap(),
            modal.reason.clone(),
        )
    });
    assert_eq!(
        lock_reason(fixture.path()),
        None,
        "a lock was written before confirm"
    );
    // The shell's real cwd is the linked worktree (macOS/Linux probe).
    let probed = kagi_git::proc::cwd_of_pid(pid).expect("cwd probe");
    assert_eq!(probed.canonicalize().unwrap(), linked);

    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert_eq!(
        lock_reason(fixture.path()).as_deref(),
        Some(token_reason.as_str())
    );
    assert!(cx.read(|cx| app.read(cx).lock_worktree_modal().is_none()));
    cx.read(|cx| {
        let state = app.read(cx);
        let claim = state
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .auto_lock
            .as_ref()
            .expect("successful confirm records ownership");
        assert_eq!(claim.target.token.reason(), token_reason);
        assert_eq!(claim.generation, 1);
    });

    // Exit while a different repository owns the visible tab. Its worktree
    // has the same registry name and a manual lock; neither its modal nor
    // its Git state may be touched by the background shell's completion.
    let other = build_fixture();
    let other_linked_root = tempfile::tempdir().unwrap();
    let other_linked = other_linked_root.path().join("linked");
    git(
        other.path(),
        &[
            "worktree",
            "add",
            "-b",
            "foreign-target",
            other_linked.to_str().unwrap(),
        ],
    );
    git(
        other.path(),
        &[
            "worktree",
            "lock",
            "--reason",
            "foreign manual",
            other_linked.to_str().unwrap(),
        ],
    );
    let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
    app.update(cx, |state, cx| {
        assert!(state.open_repository(other_linked.clone(), cx));
        assert_ne!(state.active_session(), Some(owner));
        state.open_unlock_worktree_modal("linked".into());
        assert!(
            state
                .unlock_worktree_modal()
                .is_some_and(|modal| modal.auto.is_none()),
            "foreign manual confirmation owns the modal slot"
        );
        state
            .ui
            .get(&owner)
            .unwrap()
            .terminal_session
            .as_ref()
            .unwrap()
            .paste_writer
            .as_ref()
            .unwrap()
            .paste_text("exit\n");
    });
    // ── shell exit → owner-scoped wait delivery → pending release ───────
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let exited = cx.read(|cx| {
            app.read(cx)
                .ui
                .get(&owner)
                .and_then(|ui| ui.terminal_session.as_ref())
                .and_then(|s| s.shell.as_ref())
                .and_then(|s| s.exit.clone())
        });
        if let Some(exit) = exited {
            assert!(matches!(exit, ShellExit::Exited { .. }), "{exit:?}");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "shell exit was never delivered"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(cx.read(|cx| {
        app.read(cx)
            .unlock_worktree_modal()
            .is_some_and(|modal| modal.auto.is_none())
    }));
    assert_eq!(lock_reason(other.path()).as_deref(), Some("foreign manual"));
    assert_eq!(
        lock_reason(fixture.path()).as_deref(),
        Some(token_reason.as_str())
    );
    press_key(cx, &app, window, "escape");
    assert!(cx.read(|cx| app.read(cx).unlock_worktree_modal().is_none()));
    app.update(cx, |state, cx| state.switch_repo(0, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        let modal = state
            .unlock_worktree_modal()
            .expect("the release card opens on exit");
        assert_eq!(modal.name, "linked");
        assert!(modal.auto.is_some(), "the release is the auto-checked op");
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
    });
    assert_eq!(
        lock_reason(fixture.path()).as_deref(),
        Some(token_reason.as_str())
    );

    // Someone relocks by hand between the offer and the confirm: refused, kept.
    git(
        fixture.path(),
        &["worktree", "unlock", linked.to_str().unwrap()],
    );
    git(
        fixture.path(),
        &[
            "worktree",
            "lock",
            "--reason",
            "taken over",
            linked.to_str().unwrap(),
        ],
    );
    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert_eq!(lock_reason(fixture.path()).as_deref(), Some("taken over"));
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .unlock_worktree_modal()
            .expect("refusal keeps the card");
        let err = modal
            .error
            .as_ref()
            .expect("the refusal is shown")
            .to_string();
        assert!(err.contains("refused at preflight"), "{err}");
    });
    crate::recovery_operations::press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    git(
        fixture.path(),
        &["worktree", "unlock", linked.to_str().unwrap()],
    );
    assert_eq!(repo_fingerprint(fixture.path()), before);
    unmount(cx, app, window);

    // Confirmed ownership survives disabling the setting. After the real
    // shell exits, the release still requires a second explicit confirmation
    // and records the linked worktree, not a different visible repository.
    theme::set_terminal_auto_lock(true);
    let (app, window) = mount(cx, &linked);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.ensure_terminal(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    let owned_reason = cx.read(|cx| app.read(cx).lock_worktree_modal().unwrap().reason.clone());
    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert_eq!(
        lock_reason(fixture.path()).as_deref(),
        Some(owned_reason.as_str())
    );
    theme::set_terminal_auto_lock(false);
    app.update(cx, |state, _| {
        state
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .paste_writer
            .as_ref()
            .unwrap()
            .paste_text("exit\n");
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).unlock_worktree_modal().is_some()) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "confirmed owner never received release confirmation"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(
        lock_reason(fixture.path()).as_deref(),
        Some(owned_reason.as_str()),
        "exit alone must never unlock"
    );
    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert_eq!(lock_reason(fixture.path()), None);
    assert!(cx.read(|cx| {
        app.read(cx)
            .ui()
            .terminal_session
            .as_ref()
            .unwrap()
            .auto_lock
            .is_none()
    }));
    assert!(
        read_oplog_tail_for_repo(&linked, 50).iter().any(|entry| {
            entry.op == "unlock-worktree"
                && entry.repo == linked.display().to_string()
                && matches!(entry.outcome, OpOutcome::Success { .. })
        }),
        "the owning worktree has a durable successful unlock"
    );
    assert_eq!(lock_reason(other.path()).as_deref(), Some("foreign manual"));
    unmount(cx, app, window);
    theme::set_terminal_auto_lock(restore_auto);
    eprintln!("[gui-e2e] PASS terminal_auto_lock opt-in off/on, confirm-gated acquire, wait-delivered exit, refused release after a manual relock");
}

/// #836 / ADR-0212: the release is a compare-and-unlock.
///
/// (b) Another process replaces this terminal's lock between the preflight
/// read and the release (reproduced at a fixed point through the Backend race
/// seam, not by timing): the moved-aside lock is not ours, so it goes back
/// and the release refuses — the other reason is still the lock.
/// (c) An interrupted release's `locked.kagi-*` leftover, placed by hand: the
/// real manual unlock card says so, and the auto release card is blocked —
/// Enter records the refusal and leaves both the lock and the leftover alone.
pub fn scenario_terminal_auto_lock_race(cx: &mut VisualTestAppContext) {
    use kagi_domain::plan_note::{PlanNote, WorktreeNote};
    use kagi_domain::worktree_autolock::{AutoLockToken, AutoUnlockRace, AutoUnlockTarget};

    let fixture = build_fixture();
    let main = fixture.path().canonicalize().unwrap();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-b",
            "race-target",
            linked.to_str().unwrap(),
        ],
    );
    let backend = kagi_git::Backend::open(&main).unwrap();
    let target = AutoUnlockTarget {
        token: AutoLockToken::new("tier-a").unwrap(),
        worktree: backend.linked_worktree_identity("linked").unwrap(),
    };
    let token = target.token.reason();
    let lock_ours = || {
        git(
            &main,
            &[
                "worktree",
                "lock",
                "--reason",
                &token,
                linked.to_str().unwrap(),
            ],
        )
    };
    let admin = main.join(".git/worktrees/linked");
    let leftovers = || {
        let mut found: Vec<String> = std::fs::read_dir(&admin)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("locked.kagi-"))
            .collect();
        found.sort();
        found
    };

    // (b) relocked by someone else after preflight: put back, refused.
    lock_ours();
    let plan = backend
        .plan_auto_unlock_worktree("linked", &target)
        .unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let err = backend
        .execute_auto_unlock_worktree_racing(
            &plan,
            "linked",
            &target,
            AutoUnlockRace::RelockBeforeMove("someone else".into()),
        )
        .unwrap_err();
    assert!(err.to_string().contains("put back untouched"), "{err}");
    assert_eq!(lock_reason(&main).as_deref(), Some("someone else"));
    assert!(leftovers().is_empty(), "restored, nothing left aside");

    // (c) an interrupted release's leftover, placed by hand.
    git(&main, &["worktree", "unlock", linked.to_str().unwrap()]);
    lock_ours();
    let leftover = "locked.kagi-4242-1";
    std::fs::write(admin.join(leftover), "someone else\n").unwrap();
    let is_leftover = |note: &PlanNote| matches!(note, PlanNote::Worktree(WorktreeNote::LockLeftover { files, .. }) if files == &vec![leftover.to_string()]);

    let (app, window) = mount(cx, &main);
    app.update(cx, |app, cx| {
        app.open_unlock_worktree_modal("linked".into());
        cx.notify();
    });
    paint(cx, window);
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .unlock_worktree_modal()
            .expect("manual unlock card");
        assert!(
            modal.plan.warnings.iter().any(is_leftover),
            "the manual card names the leftover: {:?}",
            modal.plan.warnings
        );
    });

    // The release plan for the same worktree is blocked. Do not forge a UI
    // terminal owner for this backend race fixture: ownership exists only
    // after that shell's real acquire confirmation succeeds.
    let auto_plan = backend
        .plan_auto_unlock_worktree("linked", &target)
        .unwrap();
    assert!(
        auto_plan.blockers.iter().any(is_leftover),
        "{:?}",
        auto_plan.blockers
    );
    assert!(backend
        .execute_auto_unlock_worktree(&auto_plan, "linked", &target)
        .is_err());
    assert_eq!(lock_reason(&main).as_deref(), Some(token.as_str()));
    assert_eq!(leftovers(), vec![leftover.to_string()]);

    std::fs::remove_file(admin.join(leftover)).unwrap();
    git(&main, &["worktree", "unlock", linked.to_str().unwrap()]);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS terminal_auto_lock_race: a replaced lock is put back and refused; a leftover blocks the release and shows on the manual card");
}

/// #851: a lock placed outside Kagi after launch reaches the sidebar on the
/// next refresh — the row's 🔐 and the right-click menu's lock state, from the
/// tab of the linked worktree itself (the reported case) and of main.
pub fn scenario_external_lock_reload(cx: &mut VisualTestAppContext) {
    use kagi::ui::sidebar::SidebarRow;

    let fixture = build_fixture();
    let main = fixture.path().canonicalize().unwrap();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("wt-feat");
    git(
        &main,
        &["worktree", "add", "-b", "wt-feat", linked.to_str().unwrap()],
    );
    let linked = linked.canonicalize().unwrap();

    let row_locked = |cx: &mut VisualTestAppContext, app: &Entity<KagiApp>| {
        cx.read(|cx| {
            app.read(cx).sidebar.rows.iter().find_map(|row| match row {
                SidebarRow::Worktree { name, locked, .. } if name == "wt-feat" => Some(*locked),
                _ => None,
            })
        })
    };
    let view_locked = |cx: &mut VisualTestAppContext, app: &Entity<KagiApp>| {
        cx.read(|cx| {
            app.read(cx)
                .view()
                .worktrees
                .iter()
                .find(|w| w.name == "wt-feat")
                .map(|w| w.locked)
        })
    };
    let menu_locked =
        |cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle| {
            let id = "sidebar-worktree-wt-feat";
            e2e::clear_control_bounds(window.window_id(), id);
            paint(cx, window);
            let bounds = e2e::control_bounds(window.window_id(), id).expect("sidebar worktree row");
            cx.simulate_mouse_down(
                window,
                bounds.center(),
                gpui::MouseButton::Right,
                gpui::Modifiers::none(),
            );
            cx.run_until_parked();
            app.update(cx, |app, cx| {
                let menu = app.worktree_menu.take().expect("worktree menu");
                cx.notify();
                assert_eq!(menu.name, "wt-feat");
                menu.locked
            })
        };

    for open_at in [&linked, &main] {
        let (app, window) = mount(cx, open_at);
        paint(cx, window);
        assert_eq!(
            view_locked(cx, &app),
            Some(false),
            "fixture starts unlocked"
        );
        assert_eq!(row_locked(cx, &app), Some(false));
        assert!(!menu_locked(cx, &app, window));

        // Outside Kagi, after launch — the watcher does not see admin files.
        git(
            &main,
            &[
                "worktree",
                "lock",
                "--reason",
                "manual: keep",
                linked.to_str().unwrap(),
            ],
        );
        assert_eq!(lock_reason(&main).as_deref(), Some("manual: keep"));

        // The real Cmd+R: `file.refresh` = manual reload + quiet fetch.
        press_key(cx, &app, window, "cmd-r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while view_locked(cx, &app) != Some(true) && std::time::Instant::now() < deadline {
            cx.run_until_parked();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            view_locked(cx, &app),
            Some(true),
            "the refreshed read model carries the external lock (opened at {})",
            open_at.display()
        );
        paint(cx, window);
        assert_eq!(
            row_locked(cx, &app),
            Some(true),
            "the sidebar row shows 🔐 after the refresh (opened at {})",
            open_at.display()
        );
        assert!(
            menu_locked(cx, &app, window),
            "the row's menu offers Unlock after the refresh (opened at {})",
            open_at.display()
        );

        git(&main, &["worktree", "unlock", linked.to_str().unwrap()]);
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS external_lock_reload: an external lock reaches the sidebar row and menu on refresh, from the linked and the main tab");
}
