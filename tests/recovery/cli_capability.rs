//! #627 E2 observation: what Kagi does today when the Git CLI is missing or
//! reports a version below the one a candidate would require.
//!
//! **This records facts; it does not assert them.** The only assertion is that
//! nothing panics or hangs. `run_git` has no version detection, no minimum-version
//! check and no structured capability code (`crates/kagi-git/src/cli.rs`), and no
//! production path invokes `git merge-tree --write-tree`. Asserting the behaviour
//! a capability gate *should* have would implement the gate the E2 experiment
//! exists to decide on, so the experiment would confirm its own premise.
//!
//! The observed values are handed to the P2 owner, who writes them into
//! `docs/research/627/E-cli-environment.md` and reaches the E2 verdict.

use std::path::Path;

use gpui::VisualTestAppContext;
use kagi_git::benchmark::fingerprint_repository;
use kagi_git::oplog::read_oplog_tail_for_repo;

use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_operations::wait_idle;

/// A `bin` directory that becomes the whole of `PATH` for one observation.
fn shim_dir(kind: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("shim dir");
    if kind == "old-version" {
        // `git --version` answers 2.37.0; everything else execs the real Git, so
        // `git fetch` still succeeds. Failing the fetch itself would measure a
        // transport error rather than the absence of a version gate.
        let real = which_git();
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'git version 2.37.0'; exit 0; fi\nexec {} \"$@\"\n",
            real.display()
        );
        let path = dir.path().join("git");
        std::fs::write(&path, script).expect("write shim");
        set_executable(&path);
    }
    dir
}

fn which_git() -> std::path::PathBuf {
    let out = std::process::Command::new("/usr/bin/which")
        .arg("git")
        .output()
        .expect("which git");
    std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("chmod");
}

/// One condition: set `PATH`, fetch, and report the four facts.
fn observe(cx: &mut VisualTestAppContext, condition: &str) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote = tempfile::tempdir().expect("remote");
    let bare = remote.path().join("origin.git");
    git(repo, &["init", "--bare", "-q", bare.to_str().unwrap()]);
    git(repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);

    let before = fingerprint_repository(repo).expect("fingerprint before");
    let shim = shim_dir(condition);
    let restore = std::env::var("PATH").unwrap_or_default();
    // SAFETY: the GUI E2E runner is single-threaded on the main thread.
    unsafe { std::env::set_var("PATH", shim.path()) };

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.fetch_async(false, cx));
    wait_idle(cx, &app);
    cx.run_until_parked();

    // Only public surfaces: the crate-internal modal accessors are not reachable
    // from the runner, and widening them for an observation would be a production
    // change this experiment must not make.
    let modal = cx.read(|cx| {
        let app = app.read(cx);
        format!(
            "footer={:?} pull_modal={}",
            app.status_footer,
            app.pull_modal().is_some()
        )
    });
    let records = read_oplog_tail_for_repo(repo, 100);
    let fetches: Vec<_> = records.iter().filter(|e| e.op == "fetch").collect();
    let outcomes: Vec<String> = fetches.iter().map(|e| format!("{:?}", e.outcome)).collect();
    let after = fingerprint_repository(repo).expect("fingerprint after");

    eprintln!("[e2-observe] condition = {condition}");
    eprintln!("[e2-observe]   1. modal: {modal}");
    eprintln!(
        "[e2-observe]   2. oplog: fetch records = {}, outcomes = {:?}",
        fetches.len(),
        outcomes
    );
    eprintln!(
        "[e2-observe]   3. fingerprint unchanged = {}",
        before == after
    );
    eprintln!("[e2-observe]   4. reached the end without panic or hang");

    unsafe { std::env::set_var("PATH", restore) };
    unmount(cx, app, window);
}

pub fn scenario_backend_cli_capability_observation(cx: &mut VisualTestAppContext) {
    observe(cx, "missing-git");
    observe(cx, "old-version");
    eprintln!(
        "[gui-e2e] PASS backend_cli_capability_observation: four facts recorded per condition"
    );
}
