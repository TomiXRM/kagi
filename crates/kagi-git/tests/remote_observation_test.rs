//! #706 review 2 — a replaced ssh transport is *asked*, never written off.
//!
//! `GIT_SSH_COMMAND` (like a user-level `core.sshCommand`) means kagi cannot
//! read the configuration that maps an `ssh_config` alias. It does not mean the
//! remote is out of reach: git talks to it through that very replacement, and
//! `git ls-remote` is exactly what can answer. Reading the override itself as
//! "unobservable" took the ordinary acknowledgement away from pushes that had
//! always been confirmed, leaving an audited release as their only exit.
//!
//! The override lives in the process environment, which no in-process test may
//! set without racing every sibling thread, so the body runs in a child of this
//! test binary started with it — the re-exec `tests/support/isolated.rs` uses,
//! plus the environment that is the point of the test.
//!
//! No host is contacted. The replacement transport is a script: it serves the
//! local fixture with `git upload-pack` for one destination and refuses the
//! other, which is the same pair of outcomes a real network produces, and it
//! records that git reached it at all.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use kagi_git::backend::remote_ref::RemoteRefObservation;
use kagi_git::backend::Backend;

const TEST: &str = "a_replaced_ssh_transport_is_asked_rather_than_declared_unobservable";

#[test]
fn a_replaced_ssh_transport_is_asked_rather_than_declared_unobservable() {
    let Some(scratch) = run_with_replaced_ssh() else {
        return;
    };
    let tip = origin_with_main(&scratch.join("origin.git"));

    let work = scratch.join("work");
    let repo = Repository::init(&work).unwrap();
    let mut config = repo.config().unwrap();
    // A URL that spells its host out. It is an SSH remote, the override is in
    // force, and it is still identified and still read.
    let literal = "git@kagi-literal.invalid:acme/widgets.git";
    // An alias — the one shape whose host really is unreadable while ssh is
    // replaced. It is asked anyway, over the replacement, and what comes back
    // is a transport failure.
    let aliased = "git@kagialias:acme/widgets.git";
    config.set_str("remote.literal.url", literal).unwrap();
    config.set_str("remote.aliased.url", aliased).unwrap();

    assert_eq!(
        Backend::read_remote_ref(&work, "literal", "refs/heads/main").unwrap(),
        RemoteRefObservation::Observed(Some(tip)),
        "a remote whose URL names its host is observed however git transports it"
    );
    assert_eq!(
        Backend::read_remote_ref(&work, "literal", "refs/heads/never").unwrap(),
        RemoteRefObservation::Observed(None),
        "and the remote's own \"I do not have that ref\" is still evidence"
    );

    match Backend::read_remote_ref(&work, "aliased", "refs/heads/main") {
        Err(error) => assert!(
            error.to_string().contains("ls-remote"),
            "a failed transport must say what failed: {error}"
        ),
        Ok(observation) => panic!(
            "the transport ran and failed, which is an error and never permission \
             to release an unobserved write: {observation:?}"
        ),
    }

    // The address-less case is unchanged: a remote that is not configured at
    // all is what unobservable is for.
    match Backend::read_remote_ref(&work, "gone", "refs/heads/main").unwrap() {
        RemoteRefObservation::Unobservable { reason } => {
            assert!(reason.contains("no matching remote"), "{reason}")
        }
        other => panic!("there is no remote \"gone\" to have answered: {other:?}"),
    }
}

/// `Some(scratch)` in the child that should run the body; `None` in the parent,
/// which by then has run the child and checked what it left behind.
///
/// Both markers are the proof the test exists for: a passing `Err` that never
/// reached the transport, or an `Observed` served by something other than the
/// replacement, would say nothing about the override.
fn run_with_replaced_ssh() -> Option<PathBuf> {
    if std::env::var("KAGI_TEST_ISOLATED").as_deref() == Ok(TEST) {
        return Some(PathBuf::from(
            std::env::var("KAGI_TEST_SCRATCH").expect("scratch directory"),
        ));
    }
    let scratch = tempfile::tempdir().expect("scratch directory");
    let root = scratch.path().display();
    let transport = scratch.path().join("stand-in-ssh");
    write_script(
        &transport,
        // `-G` is git's own probe for which ssh variant this is; refusing it
        // settles that question without serving anything.
        &format!(
            "case \"$*\" in *-G*) exit 255 ;; esac\n\
             case \"$*\" in\n\
             *kagi-literal.invalid*)\n\
             : > '{root}/asked-literal'\n\
             exec git upload-pack '{root}/origin.git' ;;\n\
             esac\n\
             : > '{root}/asked-alias'\n\
             echo 'kagi test transport: no route to this host' >&2\n\
             exit 255"
        ),
    );

    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", "--nocapture", TEST])
        .env("KAGI_TEST_ISOLATED", TEST)
        .env("KAGI_LOG_DIR", scratch.path())
        .env("KAGI_TEST_SCRATCH", scratch.path())
        .env("GIT_SSH_COMMAND", &transport)
        .output()
        .expect("start the isolated child");
    assert!(
        output.status.success(),
        "isolated child failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    for (marker, what) in [
        (
            "asked-literal",
            "the literal-host remote was answered by something other than the \
             replacement transport",
        ),
        (
            "asked-alias",
            "the aliased remote was never asked: `ls-remote` did not reach the \
             transport, so the refusal came from kagi rather than from the remote",
        ),
    ] {
        assert!(scratch.path().join(marker).exists(), "{what}");
    }
    None
}

fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

/// A bare repository with one commit on `refs/heads/main`, and that commit's id.
fn origin_with_main(path: &Path) -> String {
    let bare = Repository::init_bare(path).unwrap();
    let tree_id = bare.treebuilder(None).unwrap().write().unwrap();
    let tree = bare.find_tree(tree_id).unwrap();
    let who = git2::Signature::now("Kagi Test", "kagi@example.com").unwrap();
    bare.commit(Some("refs/heads/main"), &who, &who, "base", &tree, &[])
        .unwrap()
        .to_string()
}
