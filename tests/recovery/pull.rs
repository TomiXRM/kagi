//! Dirty Pull GUI E2E scenarios for ADR-0189.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use gpui::VisualTestAppContext;
use kagi_domain::plan_note::{PlanNote, PullNote};
use kagi_git::oplog::{read_oplog_tail_for_repo, recovery, OpOutcome};

use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_operations::{press_enter, wait_idle};

fn output(repo: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command");
    assert!(result.status.success(), "git {args:?}: {:?}", result.stderr);
    String::from_utf8(result.stdout)
        .expect("utf8 git output")
        .trim()
        .to_string()
}

fn records(repo: &Path, op: &str) -> Vec<kagi_git::oplog::OpLogEntry> {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|entry| entry.op == op)
        .collect()
}

fn assert_stash_push_recovery(repo: &Path) {
    let pushes = records(repo, "stash-push");
    assert_eq!(pushes.len(), 1, "auto-stash must be recorded exactly once");
    assert!(
        pushes[0]
            .recovery
            .iter()
            .any(|handle| handle.kind == recovery::STASH && handle.oid.len() == 40),
        "the created stash OID must be a typed recovery handle"
    );
}

pub fn scenario_pull_auto_stash_success(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let gitlink_path = "PCB/EM2/SM20/SteppingDriverBoard_L/.history";
    let cacheinfo = format!("160000,7489b69c1ec9e5763a469d9b367deac0aee76bc4,{gitlink_path}");
    git(repo, &["update-index", "--add", "--cacheinfo", &cacheinfo]);
    git(repo, &["commit", "-qm", "add unpopulated gitlink"]);
    std::fs::create_dir_all(repo.join(gitlink_path)).unwrap();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    let upstream_head = output(&other, &["rev-parse", "HEAD"]);

    std::fs::write(repo.join("README.md"), "local staged change\n").unwrap();
    std::fs::write(repo.join("scratch.txt"), "local untracked change\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "fixture Pull must be executable"
        );
        assert!(
            modal.error.is_none(),
            "confirmation must not begin as an error"
        );
    });

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.run_until_parked();

    assert_eq!(output(repo, &["rev-parse", "HEAD"]), upstream_head);
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "local staged change\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("scratch.txt")).unwrap(),
        "local untracked change\n"
    );
    assert!(output(repo, &["stash", "list"]).is_empty());
    assert_stash_push_recovery(repo);
    assert!(
        records(repo, "pull")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Success { .. })),
        "Pull success must be durable"
    );
    cx.read(|cx| assert!(app.read(cx).pull_modal().is_none()));

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_auto_stash_success: dirty changes restored after Pull");
}

/// A `git` child that leaves a descendant holding the pipes: the wait resolves,
/// but the capture is not proven complete — `run_git`'s `TerminationUnknown`.
/// `remote.<name>.uploadpack` is what `git fetch` runs for a local remote, and
/// the CLI hardening does not neutralise it, so this is the real path.
fn wait_for(
    cx: &mut VisualTestAppContext,
    mut done: impl FnMut(&mut VisualTestAppContext) -> bool,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the completion never arrived"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A stoppable stand-in for the `ssh` binary `run_ssh` spawns through `PATH`.
///
/// Every invocation is logged with its argv. A `git … pull` blocks until
/// `release` appears, so the pull can be held genuinely in flight — the state
/// the latch exists for — while the remote view's own reads still answer, as a
/// real host would. No canned-report seam: this goes through `run_ssh`.
fn blocking_fake_ssh(bin: &Path, release: &Path, calls: &Path) {
    std::fs::create_dir_all(bin).expect("shim dir");
    let path = bin.join("ssh");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             echo \"$*\" >> {calls:?}\n\
             case \"$*\" in\n\
             *pull*) while [ ! -f {release:?} ]; do sleep 0.05; done\n\
             echo 'Already up to date.' ;;\n\
             *) echo '' ;;\n\
             esac\n",
        ),
    )
    .expect("write fake ssh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// How many `git … pull` transports the fake host has been asked to run.
fn ssh_pulls(calls: &Path) -> usize {
    std::fs::read_to_string(calls)
        .map(|text| text.lines().filter(|line| line.contains("pull")).count())
        .unwrap_or(0)
}

/// #708 review P1: the lease-less remote pull's latch must actually hold.
///
/// A remote pull over SSH takes no lease — its `RemoteRepoId` needs network
/// probes that cannot run on the UI thread before the spawn — so it owns
/// `remote_write` outright. The bug this pins: while that latch was the
/// lease-derived `write_busy_op`, `refresh_write_busy()` erased it on the very
/// next `render` → `poll_app_jobs`, and on every admission preamble, so a
/// second write (a stage, a fetch, a conflict abort, another pull to the same
/// remote) could be admitted straight into a running `git pull`.
pub fn scenario_remote_pull_holds_its_latch(cx: &mut VisualTestAppContext) {
    use kagi::ui::e2e;

    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().expect("fixture path");
    // A real behind-by-one, so the remote view plans a pull from its snapshot.
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let (bare_path, other_path) = (bare.to_str().unwrap(), other.to_str().unwrap());
    git(&repo, &["init", "--bare", "-q", bare_path]);
    git(&repo, &["remote", "add", "origin", bare_path]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    git(&repo, &["fetch", "-q", "origin"]);

    let shim = tempfile::tempdir().expect("shim root");
    let release = shim.path().join("release");
    let calls = shim.path().join("calls");
    blocking_fake_ssh(shim.path(), &release, &calls);
    let original_path = std::env::var_os("PATH");
    std::env::set_var(
        "PATH",
        format!(
            "{}:{}",
            shim.path().display(),
            original_path
                .clone()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref()
        ),
    );

    let snap = kagi_git::Backend::open(&repo)
        .expect("open fixture")
        .snapshot(10_000)
        .expect("snapshot");
    let host = kagi_domain::remote::RemoteHost {
        user: Some("kagi-e2e".into()),
        host: "e2e.invalid".into(),
        port: None,
        identity_file: None,
    };
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.enter_remote_view(host, "/srv/repo".into(), snap, cx);
        app.open_pull_modal(cx);
        assert!(
            app.pull_modal().is_some(),
            "the remote view is behind by one, so Pull plans"
        );
        app.start_pull(cx);
        assert_eq!(app.remote_write, Some("pull"), "the pull latches itself");
        assert!(e2e::op_latched(app), "and the gate reads that latch");
        assert!(
            !app.app_sessions.has_leases(),
            "with no lease at all — this is the lease-less writer"
        );
    });
    // Everything below runs between the dispatch and the completion, so it
    // cannot use `run_until_parked`: the test executor is deterministic, and
    // pumping it here would wait out the very transport this is about (the
    // fake `ssh` blocks). `poll_app_jobs` is what a frame runs, and its first
    // act is the `refresh_write_busy()` that used to erase the latch.
    app.update(cx, |app, cx| {
        e2e::poll_app_jobs(app, cx);
        assert_eq!(
            app.remote_write,
            Some("pull"),
            "a lease-derived retire must not reach the remote latch (#708 P1)"
        );
        assert!(e2e::op_latched(app), "so the gate is still latched");
        // Refused on the silent path, which consults the gate directly…
        assert!(
            !app.fetch_async_for(true, None, cx),
            "no second write may start while the remote pull runs"
        );
        // …and on the admission preamble, which calls `refresh_write_busy()`
        // immediately before asking `op_latched()`.
        assert!(
            !app.fetch_async_for(false, None, cx),
            "not through the admission preamble either"
        );
        app.open_pull_modal(cx);
        assert!(
            app.pull_modal().is_none(),
            "and not a second pull to the same remote"
        );
    });

    // Only the terminal completion releases it. The transport runs — and
    // blocks — inside this pump, so the release goes in first.
    std::fs::write(&release, b"go").expect("release the transport");
    wait_for(cx, |cx| cx.read(|cx| app.read(cx).remote_write.is_none()));
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            !e2e::op_latched(state),
            "the gate opens with the completion"
        );
        assert!(state.app_sessions.may_close_host());
    });
    // Which makes every refusal above provable: had any of them been admitted,
    // the host would have been asked to pull a second time.
    assert_eq!(
        ssh_pulls(&calls),
        1,
        "exactly one pull transport ever reached the host"
    );

    match original_path {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS remote_pull_latch: the lease-less pull's latch survives refresh and only its completion clears it"
    );
}

fn leaky_helper(root: &Path, program: &str) -> String {
    let path = root.join(format!("{program}.sh"));
    std::fs::write(
        &path,
        format!("#!/bin/sh\nsleep 4 &\nexec git {program} \"$@\"\n"),
    )
    .expect("write helper");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path.to_str().expect("utf-8 path").to_string()
}

/// #702 review P1 + Codex — an unproven termination, end to end.
///
/// The whole chain in one scenario: the fetch's `TerminationUnknown` keeps the
/// lease and parks a reconcile requirement; settlement presents an inspectable
/// AppNotice that reaches reconcile without restoring the consumed Pull
/// confirmation.
pub fn scenario_pull_unknown_offers_its_reconcile(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    std::fs::write(repo.join("README.md"), "local staged change\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx)
                .pull_modal()
                .is_some_and(|modal| modal.auto_stash),
            "the fixture must produce a confirmable auto-stash pull"
        );
    });
    // The leak is installed only after the confirmation exists: a dirty Pull
    // fetches before it opens the modal (#625), and that read must succeed —
    // execution's own fetch is the one whose termination must be unproven.
    let helper = leaky_helper(remote_root.path(), "upload-pack");
    git(repo, &["config", "remote.origin.uploadpack", &helper]);

    press_enter(cx, &app, window);
    // Not `wait_idle`: a retained lease deliberately keeps the busy mirror set,
    // which is the state under test. Wait for the completion itself.
    wait_for(cx, |cx| {
        cx.read(|cx| {
            app.read(cx)
                .app_notice()
                .is_some_and(|notice| notice.inspect.is_some())
        })
    });

    assert_eq!(
        records(repo, "stash-pop").len(),
        0,
        "a fetch that is not proven stopped must not be followed by a pop"
    );
    assert!(
        !output(repo, &["stash", "list"]).is_empty(),
        "the user's work stays in the stash"
    );
    assert!(
        records(repo, "pull")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Unknown { .. })),
        "the indeterminate Pull outcome must be durable"
    );
    // The descendant of the fetch is still holding the pipes, so nothing about
    // this write is proven stopped: the scope stays reserved (ADR-0175).
    cx.read(|cx| {
        assert!(
            app.read(cx).app_sessions.has_leases(),
            "a writer whose process group is still alive keeps its lease"
        );
    });

    // Settlement presents the reconcile action directly; the consumed Pull
    // confirmation must not be resurrected in front of it.
    cx.read(|cx| {
        let notice = app
            .read(cx)
            .app_notice()
            .expect("an unacknowledged reconcile must offer itself");
        assert!(
            app.read(cx).pull_modal().is_none(),
            "Unknown must not restore the consumed Pull confirmation"
        );
        assert!(
            notice.inspect.is_some(),
            "and it must be the inspectable kind: {}",
            notice.message
        );
    });

    // Inspect while the descendant lives: the read must come back with nothing
    // observed and another look, never an acknowledge.
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    wait_for(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| app.read(cx).app_notice().is_some())
    });
    cx.read(|cx| {
        let app = app.read(cx);
        let notice = app.app_notice().expect("still unresolved, still offered");
        assert!(
            notice.inspect.is_some() && notice.acknowledge.is_none(),
            "a live process group is not a stop proof: {}",
            notice.message
        );
        assert!(
            app.app_sessions.has_leases(),
            "and the scope stays reserved while it runs"
        );
    });

    // The descendant exits. Only now is the writer proven stopped, and only now
    // may the read be acknowledged and the scope released.
    std::thread::sleep(Duration::from_secs(4));
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    wait_for(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| {
            app.read(cx)
                .app_notice()
                .is_some_and(|notice| notice.acknowledge.is_some())
        })
    });
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            !app.read(cx).app_sessions.has_leases(),
            "acknowledging a proven-stopped writer releases the scope"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_unknown_offers_its_reconcile: no pop, stash kept, reconcile reachable"
    );
}

/// #702 re-review — the 20 run-pipeline families need the same way in.
///
/// `oplog_outcome_from` records `Unknown` for a push whose termination could
/// not be proven, and `apply` parks a requirement that refuses every later
/// write in the repository. The notice was wired into the pull family only, so
/// a push in that state wedged the repository with no way to open the entry.
/// `finish_run` queues it at settlement now, like `finish_pull` does.
///
/// The other entrance — a later write's refusal naming the entry — is asserted
/// at the application layer (`blocking_reconcile`): reaching it from the GUI
/// needs a refusal that gets past the busy pre-check, and a parked
/// requirement holds that latch too.
pub fn scenario_run_unknown_offers_its_reconcile(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let bare_path = bare.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(repo, &["commit", "-q", "--allow-empty", "-m", "to push"]);
    // The push's own `git push` leaves a descendant holding the pipes, so its
    // termination cannot be proven — the real production path.
    let helper = leaky_helper(remote_root.path(), "receive-pack");
    git(repo, &["config", "remote.origin.receivepack", &helper]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_push_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx).push_modal().is_some(),
            "the fixture must produce a push confirmation"
        );
    });
    press_enter(cx, &app, window);
    wait_for(cx, |cx| {
        cx.read(|cx| {
            app.read(cx)
                .app_notice()
                .is_some_and(|notice| notice.inspect.is_some())
        })
    });
    let durable = records(repo, "push");
    assert_eq!(durable.len(), 1, "one attempt, one durable Push entry");
    assert!(matches!(durable[0].outcome, OpOutcome::Unknown { .. }));

    // Entrance 1: the completion itself.
    cx.read(|cx| {
        let state = app.read(cx);
        let notice = state
            .app_notice()
            .expect("a run completion that parked a requirement offers it");
        assert!(notice.inspect.is_some(), "{}", notice.message);
        assert!(
            state.push_modal().is_none(),
            "Unknown must not restore the consumed Push confirmation"
        );
    });

    // The Inspect action runs reconciliation. While the descendant still
    // owns the pipes it remains inspectable and cannot be acknowledged.
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    wait_for(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| app.read(cx).app_notice().is_some())
    });
    cx.read(|cx| {
        let app = app.read(cx);
        let notice = app.app_notice().expect("still unresolved, still offered");
        assert!(notice.inspect.is_some() && notice.acknowledge.is_none());
        assert!(app.app_sessions.has_leases());
    });

    std::thread::sleep(Duration::from_secs(4));
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    wait_for(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| {
            app.read(cx)
                .app_notice()
                .is_some_and(|notice| notice.acknowledge.is_some())
        })
    });
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            !app.read(cx).app_sessions.has_leases(),
            "acknowledging the proven-stopped Push releases its scope"
        );
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS run_unknown_offers_its_reconcile: the completion opens it");
}

/// #702 re-review — the reconcile notice is settlement, not presentation.
///
/// A pull whose tab the user left still parks a reconcile requirement, and that
/// requirement refuses every later write in the scope. Building the inspectable
/// notice after the current-tab guard left those completions with an entry
/// nobody could open: the write refusals said `NeedsReconcile` and there was no
/// other way in. The notice is queued with the recording-failure notices now,
/// before the guard, so it survives the switch.
pub fn scenario_pull_unknown_notice_survives_a_tab_switch(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let (bare, other) = (
        remote_root.path().join("origin.git"),
        remote_root.path().join("other"),
    );
    let (bare_path, other_path) = (bare.to_str().unwrap(), other.to_str().unwrap());

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    git(repo, &["fetch", "-q", "origin"]);
    let helper = leaky_helper(remote_root.path(), "upload-pack");
    git(repo, &["config", "remote.origin.uploadpack", &helper]);
    let other_tab = build_fixture();

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_tab.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_some(),
            "tab A must have a confirmation to press"
        );
    });

    // Confirm and leave in one synchronous turn: the completion certainly
    // arrives while tab B is on screen, so its presentation is dropped.
    app.update(cx, |app, cx| {
        app.start_pull(cx);
        app.switch_repo(1, cx);
    });
    wait_for(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| app.read(cx).app_notice().is_some())
    });

    cx.read(|cx| {
        let notice = app
            .read(cx)
            .app_notice()
            .expect("a parked reconcile must offer itself even to the tab that stayed");
        assert!(
            notice.inspect.is_some(),
            "and it must be the inspectable kind: {}",
            notice.message
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_unknown_notice_survives_a_tab_switch: settlement queued it, not presentation"
    );
}

/// #702 review P1 — a restored failure must not end on a success.
///
/// `steps` is `stash-push Success → pull Failed → stash-pop Success`, and
/// the old presentation path announced each receipt: the last announcement won,
/// so the window ended with a Pull failure modal over a `stash-pop: … → …`
/// success footer and three toasts. The receipts are panel rows now and the
/// decisive failure gets one toast, without a dismiss-only modal.
pub fn scenario_pull_failure_presents_only_the_decisive_receipt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let bare_path = bare.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(repo.join("README.md"), "restore after failed Pull\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx)
                .pull_modal()
                .is_some_and(|modal| modal.auto_stash && modal.plan.blockers.is_empty()),
            "the fixture must produce a confirmable auto-stash pull"
        );
    });
    // The remote goes away only after the confirmation exists (see
    // `scenario_pull_auto_stash_failure_restores`): the pull's own fetch is the
    // one that must fail, and the restore that follows it must succeed.
    std::fs::remove_dir_all(&bare).unwrap();

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    for op in ["stash-push", "pull", "stash-pop"] {
        assert_eq!(records(repo, op).len(), 1, "{op} is recorded once");
    }
    assert!(
        matches!(records(repo, "pull")[0].outcome, OpOutcome::Failed { .. }),
        "the pull is the failure the workflow settles on"
    );
    assert!(
        matches!(
            records(repo, "stash-pop")[0].outcome,
            OpOutcome::Success { .. }
        ),
        "and the restore after it succeeded — the case that used to win the footer"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.pull_modal().is_none(),
            "Failed must not restore the consumed Pull confirmation"
        );
        assert!(
            app.app_notice().is_none(),
            "a recorded Pull failure must not open a dismiss-only modal"
        );
        match &app.status_footer {
            kagi::ui::FooterStatus::Failed(text) => assert!(
                text.contains("pull"),
                "the footer belongs to the decisive receipt: {text}"
            ),
            other => panic!("a failed workflow must not end on a success footer: {other:?}"),
        }
        let toasts: Vec<String> = app
            .toast_stack
            .as_ref()
            .map(|stack| {
                stack
                    .read(cx)
                    .toasts()
                    .iter()
                    .map(|toast| toast.message.to_string())
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            !toasts.iter().any(|message| message.contains("stash-pop")),
            "a sibling receipt must not announce itself: {toasts:?}"
        );
        let panel = app.op_log.as_ref().unwrap().read(cx);
        for op in ["stash-push", "pull", "stash-pop"] {
            let durable = records(repo, op);
            assert!(
                panel
                    .entries()
                    .iter()
                    .any(|entry| entry.id == durable[0].id),
                "the panel must still hold the durable {op} receipt"
            );
        }
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_failure_presents_only_the_decisive_receipt: one announcement, three rows"
    );
}

/// #702 review — the completion goes to the tab that asked for it, or nowhere.
///
/// The app-level test proves the stamp is frozen; this one runs the branch that
/// *acts* on it. Tab A confirms a pull that will fail, the user leaves for tab B
/// before the completion lands, and its failure notice must not open over tab B.
pub fn scenario_pull_completion_drops_when_its_tab_is_left(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    // Behind by one, by local knowledge: the confirmation opens without a fetch.
    git(repo, &["fetch", "-q", "origin"]);
    let other_tab = build_fixture();

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_tab.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_some(),
            "tab A must have a confirmation to press"
        );
    });
    std::fs::remove_dir_all(&bare).unwrap();

    // Confirm and leave in one synchronous turn, so the job is queued and has
    // not run yet: the executor is driven only after the switch, which makes
    // "the completion arrives while another tab is on screen" the certain order
    // rather than a hoped-for one (as in `pull_confirm_parks_for_its_tab`).
    app.update(cx, |app, cx| {
        app.start_pull(cx);
        app.switch_repo(1, cx);
    });
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    let durable = records(repo, "pull");
    assert_eq!(
        durable.len(),
        1,
        "the write still ran and still recorded itself"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.pull_modal().is_none(),
            "tab A's failure must not open over tab B"
        );
        // The receipt is durable either way; what must not happen is its
        // *presentation* landing on the tab the user is now looking at.
        let panel = app.op_log.as_ref().unwrap().read(cx);
        assert!(
            !panel
                .entries()
                .iter()
                .any(|entry| entry.id == durable[0].id),
            "tab A's receipt must not be presented on tab B"
        );
        match &app.status_footer {
            kagi::ui::FooterStatus::Failed(text) => {
                assert!(!text.contains("pull"), "nor its footer: {text}")
            }
            _ => {}
        }
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_completion_drops_when_its_tab_is_left: the stamp routes it, or nothing does"
    );
}

/// #702 review P2 — a pull refused for known blockers is the core's receipt.
///
/// The UI used to author this `Refused` entry itself, which left pull with two
/// receipt authors. Nothing executes here, so the assertion is that the entry
/// exists, is durable, carries the plan's blockers, and is the row the panel
/// shows — no second, UI-made copy beside it.
pub fn scenario_pull_blocked_plan_presents_a_core_receipt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let (app, window) = mount(cx, repo);
    // No remote at all: `plan_pull` blocks, and the modal is opened directly so
    // the blocker reaches `start_pull` rather than being filtered earlier.
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        cx.notify();
    });
    cx.run_until_parked();
    let blocked = cx.read(|cx| {
        app.read(cx)
            .pull_modal()
            .is_some_and(|modal| !modal.plan.blockers.is_empty())
    });
    assert!(
        blocked,
        "the fixture must produce a Pull confirmation carrying blockers"
    );

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.run_until_parked();

    let durable = records(repo, "pull");
    assert_eq!(
        durable.len(),
        1,
        "one refusal, one entry — not a UI copy beside the core's"
    );
    let OpOutcome::Refused { blockers } = &durable[0].outcome else {
        panic!(
            "a blocked plan is refused, not failed: {:?}",
            durable[0].outcome
        );
    };
    assert!(
        !blockers.is_empty(),
        "the refusal must carry the plan's blockers"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        let panel = app.op_log.as_ref().unwrap().read(cx);
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.op == "pull" && entry.repo == durable[0].repo)
            .collect();
        assert_eq!(shown.len(), 1, "the panel shows the refusal once");
        assert_eq!(
            shown[0].id, durable[0].id,
            "and it is the durable receipt, not one the UI made"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_blocked_plan_presents_a_core_receipt: the UI authors no pull outcome"
    );
}

/// ADR-0196 Wave 3: the pull workflow presents the backend's own receipts.
///
/// A dirty pull is three writes, each of which already records itself. The UI
/// used to add a fourth entry it synthesized from the composite result — so a
/// successful auto-stash pull wrote two `pull` rows, one of them not produced
/// by any execution. This asserts the three children in execution order, one
/// durable `pull` entry, and a panel row that *is* the durable entry.
pub fn scenario_pull_presents_backend_receipts(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);

    std::fs::write(repo.join("README.md"), "local staged change\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx)
                .pull_modal()
                .is_some_and(|modal| modal.auto_stash && modal.plan.blockers.is_empty()),
            "the fixture must produce a confirmable auto-stash pull"
        );
    });

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    // The tail reads newest first; the workflow ran the other way round.
    let durable = read_oplog_tail_for_repo(repo, 100);
    let workflow: Vec<&str> = durable
        .iter()
        .rev()
        .map(|entry| entry.op.as_str())
        .filter(|op| matches!(*op, "stash-push" | "pull" | "stash-pop"))
        .collect();
    assert_eq!(
        workflow,
        vec!["stash-push", "pull", "stash-pop"],
        "a dirty pull records its three children, in execution order"
    );
    let pulls = records(repo, "pull");
    assert_eq!(
        pulls.len(),
        1,
        "the one pull entry is the backend's; the UI must not synthesize a copy"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        let panel = app.op_log.as_ref().unwrap().read(cx);
        // The panel is seeded from the whole log, so scope it to this fixture.
        let mine: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.repo == pulls[0].repo)
            .collect();
        let shown: Vec<_> = mine.iter().filter(|entry| entry.op == "pull").collect();
        assert_eq!(
            shown.len(),
            1,
            "the panel shows the receipt once, not a UI copy"
        );
        assert_eq!(
            shown[0].id, pulls[0].id,
            "the panel entry is the durable receipt"
        );
        assert_eq!(
            shown[0].failure_code, pulls[0].failure_code,
            "the presented entry keeps the backend's failure code"
        );
        for op in ["stash-push", "pull", "stash-pop"] {
            let durable = records(repo, op);
            assert_eq!(durable.len(), 1, "{op} is recorded once");
            assert!(
                mine.iter().any(|entry| entry.id == durable[0].id),
                "the panel must show the durable {op} receipt"
            );
        }
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_presents_backend_receipts: three child receipts, no UI-synthesized entry"
    );
}

pub fn scenario_pull_auto_stash_failure_restores(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let bare_path = bare.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(repo.join("README.md"), "restore after failed Pull\n").unwrap();
    std::fs::write(repo.join("scratch.txt"), "restore untracked\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "failure must come from execution"
        );
    });

    // The remote disappears only after the confirmation exists: since #625 a
    // dirty Pull fetches *before* it opens the modal (ADR-0192), so removing it
    // up front would fail that fetch and there would be no modal to confirm —
    // a different scenario. Execution's own fetch is the one that must fail
    // here.
    std::fs::remove_dir_all(&bare).unwrap();

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "restore after failed Pull\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("scratch.txt")).unwrap(),
        "restore untracked\n"
    );
    assert!(output(repo, &["stash", "list"]).is_empty());
    assert_stash_push_recovery(repo);
    assert!(
        records(repo, "pull")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Failed { .. })),
        "Pull failure must be durable"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.pull_modal().is_none(),
            "Failed must not restore the consumed Pull confirmation"
        );
        assert!(
            app.app_notice().is_none(),
            "the durable Operation Log row replaces the old plain AppNotice"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_auto_stash_failure_restores: failed Pull restores changes and records its result"
    );
}

/// A repository whose working tree edits the same line `origin/main` moved:
/// `shared.txt` collides, and the collision is a real content conflict.
///
/// Deliberately **not** fetched: `origin/main` is unknown to the repository
/// when the Pull button is pressed, so a path can only be named if the UI
/// fetched first (ADR-0192).
fn overlap_fixture(repo: &Path, remote_root: &Path) {
    let bare = remote_root.join("origin.git");
    let other = remote_root.join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    std::fs::write(repo.join("shared.txt"), "base\n").unwrap();
    git(repo, &["add", "shared.txt"]);
    git(repo, &["commit", "-qm", "add shared.txt"]);
    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root, &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("shared.txt"), "base\nupstream edit\n").unwrap();
    git(&other, &["add", "shared.txt"]);
    git(
        &other,
        &["commit", "-q", "-m", "upstream touches shared.txt"],
    );
    git(&other, &["push", "-q", "origin", "main"]);
    // The same line, edited here and not committed: the restore conflicts.
    std::fs::write(repo.join("shared.txt"), "base\nlocal edit\n").unwrap();
}

/// #625: a dirty Pull whose dirty path is also changed upstream must name that
/// path in the confirmation modal — *before* the user confirms.
///
/// The pull is a fast-forward, so neither the behind count nor
/// `MergePrediction` can see the collision: the path appears only because the
/// UI fetched first and the plan merged the two sides in memory (ADR-0192) —
/// exactly what used to surface after confirming, when the auto-stash failed
/// to restore.
pub fn scenario_pull_auto_stash_overlap_preview(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "the collision is a warning, not a refusal: {:?}",
            modal.plan.blockers
        );
        let shown: String = modal
            .plan
            .blockers
            .iter()
            .chain(modal.plan.warnings.iter())
            .map(|note| note.message_en())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            shown.contains("shared.txt"),
            "the modal must name the colliding path before confirmation:\n{shown}"
        );
        assert!(
            modal
                .plan
                .warnings
                .iter()
                .any(|note| matches!(note, PlanNote::Pull(PullNote::RestoreConflict { .. }))),
            "the collision must travel as a typed note: {:?}",
            modal.plan.warnings
        );
        // The auto-stash swap must not have dropped it (#625 Part 3).
        assert!(
            modal
                .plan
                .warnings
                .iter()
                .any(|note| matches!(note, PlanNote::Pull(PullNote::AutoStash { .. }))),
            "the auto-stash summary still belongs in the modal: {:?}",
            modal.plan.warnings
        );
    });

    // The regression PM caught: the confirmation opens and then a reload lands
    // on top of it. That reload is *caused by kagi's own fetch* (the watcher
    // sees `.git` change), and the modal-clearing sweep in `apply_reload_data`
    // wiped it ~0.5s later — Pull looked dead: pressed, nothing on screen.
    // Drive the same external reload twice, since the watcher can fire more
    // than once, and require the confirmation to still be there and still name
    // the path.
    for round in 1..=2 {
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            let modal = app
                .read(cx)
                .pull_modal()
                .unwrap_or_else(|| panic!("reload {round} closed the Pull confirmation"));
            assert!(modal.auto_stash, "round {round}");
            let shown: String = modal
                .plan
                .warnings
                .iter()
                .map(|note| note.message_en())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                shown.contains("shared.txt"),
                "reload {round} must keep the colliding path named:\n{shown}"
            );
        });
    }

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_auto_stash_overlap_preview: the modal names the colliding path and survives reloads"
    );
}

/// #625 P1-a: a Pull confirmation is delivered to the tab that asked for it,
/// even when that tab is not on screen while the fetch finishes.
///
/// Three earlier attempts fixed one branch each and left another: a bare flag
/// was cleared by a background tab's reload, then consumed only while the tab
/// was active, so "press Pull, switch tabs, come back" showed nothing at all.
/// The request now travels inside its own fetch task and parks for its tab.
pub fn scenario_pull_confirm_parks_for_its_tab(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());
    let other_tab = build_fixture();

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_tab.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    // Tab A presses Pull, then the user leaves for tab B — both synchronous, so
    // the fetch task is queued and has not run yet. The executor is driven only
    // after the switch, which is what makes "the fetch finishes while another
    // tab is on screen" the certain order rather than a hoped-for one.
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight.is_some(),
            "the confirmation must be waiting on a fetch"
        );
        app.switch_repo(1, cx);
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_none(),
            "tab A's confirmation must not open over tab B"
        );
        assert!(
            !app.read(cx).pending_pull_confirm.is_empty(),
            "it must be parked for the tab that asked, not dropped"
        );
    });

    // Coming back to tab A delivers it.
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .pull_modal()
            .expect("returning to tab A must show the confirmation it asked for");
        let shown: String = modal
            .plan
            .warnings
            .iter()
            .map(|note| note.message_en())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(shown.contains("shared.txt"), "{shown}");
        assert!(
            app.read(cx).pending_pull_confirm.is_empty(),
            "delivery consumes the parked request"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_confirm_parks_for_its_tab: the confirmation waits for the tab that asked"
    );
}

/// #625 P2: a modal opened while the pre-Pull fetch runs must not be replaced.
///
/// "One modal at a time" is structural (ADR-0093), so the completion cannot
/// simply set its own: the branch-name the user is typing would vanish.
pub fn scenario_pull_confirm_yields_to_another_modal(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    let head = output(repo, &["rev-parse", "HEAD"]);
    // Pull, then a different modal — both before the executor runs the fetch,
    // so the completion certainly lands with the other modal already open.
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight.is_some(),
            "the confirmation must be waiting on a fetch"
        );
        app.open_create_branch_modal(kagi_git::CommitId(head.clone()), cx);
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    cx.read(|cx| {
        // Both halves of "one modal at a time": the request yields instead of
        // replacing what the user opened, and what the user opened is still
        // there — the fetch's own reload no longer sweeps a pure-input modal.
        assert!(
            app.read(cx).create_branch_modal().is_some(),
            "the modal opened during the fetch must still be on screen"
        );
        assert!(
            app.read(cx).pull_modal().is_none(),
            "the Pull confirmation must not take over the modal slot"
        );
        assert!(
            app.read(cx).pending_pull_confirm.is_empty(),
            "a request for the tab on screen is cancelled, not parked"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_confirm_yields_to_another_modal: a modal opened during the fetch is kept"
    );
}

/// A production dirty-Pull fetch failure is recorded without disturbing the
/// Remote Browse modal the user opened meanwhile.
pub fn scenario_pull_failure_notice_waits_for_remote_browse(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight.is_some(),
            "the dirty Pull must be waiting on its production fetch"
        );
        app.open_remote_browse(cx);
        assert!(
            kagi::ui::e2e::deliver_acknowledge_notice(app, "acknowledge before fetch failure"),
            "the queued notice must carry a real reconciliation acknowledgement",
        );
    });

    // The fetch task has been dispatched but the executor has not run it yet.
    // Removing the remote makes pull_push::deliver_pull_confirm record its
    // real fetch-failure path while Remote Browse owns the slot.
    drop(remote_root);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    cx.read(|cx| {
        assert!(
            app.read(cx).remote_browse().is_some(),
            "a Pull fetch failure must not replace Remote Browse",
        );
    });

    app.update(cx, |app, cx| {
        app.cancel_remote_browse();
        kagi::ui::e2e::present_app_notice(app);
        assert!(
            kagi::ui::e2e::app_notice_is_acknowledgeable(app),
            "the actionable notice already waiting must stay first",
        );
        app.confirm_app_notice(cx);
        assert!(
            app.app_sessions.reconcile_ids().is_empty(),
            "async-notice-action-still-executes: the delayed Acknowledge must execute",
        );
        kagi::ui::e2e::present_app_notice(app);
    });
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "the recorded fetch failure must not add a dismiss-only notice"
    );
    assert!(
        records(repo, "fetch")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Failed { .. })),
        "the fetch failure must remain durable in Operation Log"
    );

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_failure_notice_waits_for_remote_browse: production failure uses Operation Log without replacing the modal"
    );
}

/// A recorded fetch failure does not add a third item to an existing notice
/// queue containing plain and actionable notices.
pub fn scenario_pull_failure_notice_waits_for_app_notice(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight.is_some(),
            "the dirty Pull must be waiting on its production fetch"
        );
        kagi::ui::e2e::deliver_app_notice(app, "first plain notice");
        assert_eq!(
            kagi::ui::e2e::app_notice_message(app),
            Some("first plain notice"),
            "async-notice-keeps-first-plain: the first plain notice must own the vacant slot",
        );
        assert!(
            kagi::ui::e2e::deliver_acknowledge_notice(app, "second actionable notice"),
            "the second notice must carry a real reconciliation acknowledgement",
        );
    });

    // Fail the already-dispatched production fetch while a plain AppNotice is
    // visible and an actionable notice is already waiting behind it.
    drop(remote_root);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    app.update(cx, |app, cx| {
        assert_eq!(
            kagi::ui::e2e::app_notice_message(app),
            Some("first plain notice"),
            "async-notice-does-not-replace-app-notice: the later Pull failure must not replace or discard the visible notice",
        );
        app.clear_app_notice();
        kagi::ui::e2e::present_app_notice(app);
        assert!(
            kagi::ui::e2e::app_notice_is_acknowledgeable(app),
            "async-notice-app-fifo-second: the notice already waiting must be presented second",
        );
        app.confirm_app_notice(cx);
        assert!(
            app.app_sessions.reconcile_ids().is_empty(),
            "async-notice-app-action-executes: the queued Acknowledge must still execute",
        );
        kagi::ui::e2e::present_app_notice(app);
    });
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "the recorded Pull failure must not become a third plain notice"
    );

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_failure_notice_waits_for_app_notice: recorded failures do not extend the notice FIFO"
    );
}

/// A production fetch failure has no modal lifecycle: opening and closing a
/// different modal cannot resurrect a dismiss-only failure notice.
pub fn scenario_pull_failure_notice_displacement_vs_dismissal(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight.is_some(),
            "the dirty Pull must be waiting on its production fetch"
        );
    });

    drop(remote_root);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "a recorded fetch failure must not create a plain AppNotice"
    );

    app.update(cx, |app, cx| app.open_remote_browse(cx));
    assert!(
        cx.read(|cx| app.read(cx).remote_browse().is_some()),
        "notice-displacement-opens-new-modal: Remote Browse must displace the visible notice"
    );
    app.update(cx, |app, _| {
        app.cancel_remote_browse();
        kagi::ui::e2e::present_app_notice(app);
    });
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "closing another modal must not materialize a recorded failure notice"
    );
    assert!(
        records(repo, "fetch")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Failed { .. })),
        "the complete fetch error remains available in Operation Log"
    );

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_failure_notice_displacement_vs_dismissal: recorded failures have no modal lifecycle"
    );
}

/// #625 P1: the confirmation's promise is checked before anything is stashed.
///
/// After the modal is on screen an editor saves *another* path the update also
/// changes. Stashing first would hide it from every downstream guard — the
/// preflight and the execute-time dirty-path check both see a clean tree — and
/// only the restore would conflict, unannounced. The run must refuse instead.
pub fn scenario_pull_refuses_when_the_dirty_set_moved(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());
    // A second path the upstream also changed, still clean at confirmation time.
    let other = remote_root.path().join("other");
    std::fs::write(other.join("second.txt"), "upstream second\n").unwrap();
    git(&other, &["add", "second.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream adds second.txt"]);
    git(&other, &["push", "-q", "origin", "main"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_some(),
            "the confirmation must be on screen first"
        );
    });

    // The promise goes stale: an editor writes a path the modal never named.
    std::fs::write(repo.join("second.txt"), "local second\n").unwrap();

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert!(
        output(repo, &["stash", "list"]).is_empty(),
        "nothing may be stashed once the confirmation is stale"
    );
    assert_eq!(
        output(repo, &["rev-parse", "HEAD"]),
        output(repo, &["rev-parse", "HEAD@{0}"]),
        "and nothing may be pulled"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("second.txt")).unwrap(),
        "local second\n",
        "the work the user did after confirming is untouched"
    );
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .pull_modal()
            .expect("the modal stays, carrying the refusal");
        let error = modal.error.clone().expect("a refusal message");
        assert!(
            error.contains("changed after this confirmation"),
            "the refusal must say why: {error}"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_refuses_when_the_dirty_set_moved: a stale confirmation stashes nothing"
    );
}
