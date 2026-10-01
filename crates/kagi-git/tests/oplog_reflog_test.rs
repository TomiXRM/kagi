//! #334 slice 1 (ADR-0214): the reflog lines an Operation Log entry shows are
//! the ones its own write produced — found by the entry's time window, from the
//! oplog's own timestamps — and reading them changes nothing.

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git_output, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

use kagi_domain::oplog_reflog::{Attribution, ReflogWindow};
use kagi_git::oplog::read_oplog_tail_for_repo;
use kagi_git::{Backend, CommitId, Operation};
use std::path::Path;

/// Sleep into the next wall-clock second, so two writes never share one.
fn next_second() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_nanos(
        1_000_000_000 - u64::from(now.subsec_nanos()) + 20_000_000,
    ));
}

fn run(backend: &mut Backend, op: Operation) {
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    backend.run(&op, &plan).unwrap();
}

fn snapshot(repo: &Path) -> (String, String) {
    (
        git_output(repo, &["show-ref"]),
        git_output(repo, &["reflog", "--all", "--date=raw"]),
    )
}

#[test]
fn each_entry_window_holds_its_own_write_and_not_its_neighbours() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo, "main");
    write_file(&repo, "a.txt", "a\n");
    commit_all(&repo, "base");
    let head = CommitId(git_output(&repo, &["rev-parse", "HEAD"]));

    let mut backend = Backend::open(&repo).unwrap();
    next_second();
    run(
        &mut backend,
        Operation::CreateBranch {
            name: "feature".into(),
            at: head,
        },
    );
    next_second();
    run(
        &mut backend,
        Operation::Checkout {
            branch: "feature".into(),
        },
    );

    // Newest first, from the log the panel reads.
    let entries = read_oplog_tail_for_repo(&repo, 10);
    assert_eq!(entries.len(), 2, "{entries:?}");
    let (checkout, create) = (&entries[0], &entries[1]);
    assert!(create.timestamp < checkout.timestamp);

    let before = snapshot(&repo);
    let lines_for = |ts: i64, older: Option<i64>, newer: Option<i64>| {
        let window = ReflogWindow::for_entry(ts, older, newer);
        let lines = backend
            .reflog_lines_between(window.after, window.until)
            .unwrap();
        window.attribute(lines)
    };
    let create_lines = lines_for(create.timestamp, None, Some(checkout.timestamp));
    let checkout_lines = lines_for(checkout.timestamp, Some(create.timestamp), None);

    // The branch creation: refs/heads/feature's first line. The window has
    // no loaded predecessor, so the fixture's own commit is in it as well.
    assert!(
        create_lines
            .iter()
            .any(|(l, a)| l.refname == "refs/heads/feature"
                && l.message.starts_with("branch: Created from")
                && *a == Attribution::Within),
        "{create_lines:?}"
    );
    assert!(
        !create_lines
            .iter()
            .any(|(l, _)| l.message.starts_with("checkout:")),
        "the later checkout is not the creation's: {create_lines:?}"
    );
    // The checkout: HEAD only, moving main → feature.
    assert_eq!(checkout_lines.len(), 1, "{checkout_lines:?}");
    let (line, attribution) = &checkout_lines[0];
    assert_eq!(line.refname, "HEAD");
    assert_eq!(line.message, "checkout: moving from main to feature");
    assert_eq!(*attribution, Attribution::Within);

    assert_eq!(snapshot(&repo), before, "reading reflogs writes nothing");
    assert_eq!(
        read_oplog_tail_for_repo(&repo, 10).len(),
        2,
        "and logs nothing"
    );
}
