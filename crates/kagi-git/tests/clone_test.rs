//! `gh repo clone` as a recorded write (#923, ADR-0219): the destination is
//! never overwritten, whatever a failed or stopped clone left is kept and
//! named, and every attempt has a receipt keyed by the destination path.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kagi_domain::plan_note::{CloneNote, PlanDisposition, PlanNote};
use kagi_git::oplog::{read_oplog_tail, OpOutcome};
use kagi_git::ops::{clone_args, execute_clone, execute_clone_within, plan_clone, CloneRequest};
use kagi_git::GitError;

const SOURCE: &str = "github.com/acme/widgets";

/// A fake `gh` on `PATH` whose `repo clone` runs `action` with `$3` = source
/// and `$4` = destination, recording its argv and each attempt.
struct Fixture {
    _root: tempfile::TempDir,
    _origin: git_fixture::RemoteFixture,
    root: PathBuf,
    logs: PathBuf,
}

impl Fixture {
    fn new(action: &str) -> Self {
        let root_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path().canonicalize().unwrap();
        let origin = git_fixture::repo_with_bare_origin("main");
        let bin = root.join("bin");
        let logs = root.join("logs");
        for dir in [&bin, &logs] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let action = action.replace("$BARE", &origin.remote.display().to_string());
        let gh = bin.join("gh");
        std::fs::write(
            &gh,
            format!(
                "#!/bin/sh\ncase \"$1 $2\" in\n\
                 'repo clone')\n\
                 printf '%s\\n' \"$@\" > '{logs}/argv.txt'\n\
                 echo attempt >> '{logs}/attempts.txt'\n\
                 {action}\n;;\n*) exit 8 ;;\nesac\n",
                logs = logs.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(gh, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        Self {
            _root: root_dir,
            _origin: origin,
            root,
            logs,
        }
    }

    fn request(&self, dest: &Path) -> CloneRequest {
        CloneRequest {
            source: SOURCE.into(),
            dest: dest.to_path_buf(),
            is_fork: false,
        }
    }

    fn attempted(&self) -> bool {
        self.logs.join("attempts.txt").exists()
    }
}

/// A real clone of the local bare repository, with `origin` pointed at the
/// GitHub URL `gh` would have used.
const CLONE_OK: &str = "git clone -q '$BARE' \"$4\" && \
     git -C \"$4\" remote set-url origin https://github.com/acme/widgets.git";

fn only_receipt() -> kagi_git::oplog::OpLogEntry {
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].op, "clone");
    entries[0].clone()
}

/// Nothing that already exists is a destination, except an empty folder.
#[test]
fn the_plan_refuses_every_destination_it_would_overwrite() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let full = root.join("full");
    std::fs::create_dir(&full).unwrap();
    std::fs::write(full.join("keep.txt"), "mine").unwrap();
    let file = root.join("file");
    std::fs::write(&file, "mine").unwrap();
    let empty = root.join("empty");
    std::fs::create_dir(&empty).unwrap();
    let link = root.join("link");
    std::os::unix::fs::symlink(&empty, &link).unwrap();

    let plan = |dest: &Path| {
        plan_clone(&CloneRequest {
            source: SOURCE.into(),
            dest: dest.to_path_buf(),
            is_fork: false,
        })
    };
    for dest in [&full, &file, &link] {
        assert_eq!(
            plan(dest).blockers,
            vec![PlanNote::Clone(CloneNote::DestinationNotEmpty {
                path: dest.display().to_string()
            })],
            "{}",
            dest.display()
        );
    }
    assert_eq!(
        plan(&root.join("missing/widgets")).blockers,
        vec![PlanNote::Clone(CloneNote::ParentMissing {
            path: root.join("missing").display().to_string()
        })]
    );
    assert!(matches!(
        plan(Path::new("relative/widgets")).blockers.as_slice(),
        [PlanNote::Clone(CloneNote::DestinationNotAbsolute { .. })]
    ));
    for dest in [empty, root.join("new")] {
        let ready = plan(&dest);
        assert_eq!(ready.disposition, PlanDisposition::Ready, "{ready:?}");
        assert!(!ready.destructive);
    }
    assert_eq!(
        std::fs::read_to_string(full.join("keep.txt")).unwrap(),
        "mine"
    );

    let fork = plan_clone(&CloneRequest {
        source: SOURCE.into(),
        dest: root.join("fork"),
        is_fork: true,
    });
    assert_eq!(
        fork.warnings,
        vec![PlanNote::Clone(CloneNote::ForkAddsUpstream)]
    );
}

#[test]
fn a_clone_lands_in_a_new_folder_and_is_recorded() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new(CLONE_OK);
    let dest = fixture.root.join("widgets");
    let request = fixture.request(&dest);
    let plan = plan_clone(&request);
    assert_eq!(plan.disposition, PlanDisposition::Ready);

    let report = execute_clone(&request, &plan);
    let Ok(kagi_git::OperationOutcome::Clone { path, source }) = &report.result else {
        panic!("{:?}", report.result);
    };
    assert_eq!(path, &dest.display().to_string());
    assert_eq!(source, SOURCE);
    assert_eq!(std::fs::read_to_string(dest.join("a.txt")).unwrap(), "a\n");
    let argv: Vec<String> = std::fs::read_to_string(fixture.logs.join("argv.txt"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    let expected: Vec<String> = clone_args(&request)
        .into_iter()
        .map(|arg| arg.into_string().unwrap())
        .collect();
    assert_eq!(argv, expected);

    let receipt = only_receipt();
    assert_eq!(receipt.repo, dest.display().to_string());
    assert!(matches!(receipt.outcome, OpOutcome::Success { .. }));
}

/// The folder filled up between the card and the click: refused before `gh`
/// runs, and what is there stays untouched.
#[test]
fn a_destination_filled_after_the_plan_is_refused_before_gh() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new(CLONE_OK);
    let dest = fixture.root.join("widgets");
    std::fs::create_dir(&dest).unwrap();
    let request = fixture.request(&dest);
    let plan = plan_clone(&request);
    assert_eq!(plan.disposition, PlanDisposition::Ready);
    std::fs::write(dest.join("notes.txt"), "mine").unwrap();

    let report = execute_clone(&request, &plan);
    assert!(
        matches!(&report.result, Err(GitError::Blocked(note))
            if matches!(**note, PlanNote::Clone(CloneNote::DestinationNotEmpty { .. }))),
        "{:?}",
        report.result
    );
    assert!(!fixture.attempted(), "gh repo clone must not run");
    assert_eq!(
        std::fs::read_to_string(dest.join("notes.txt")).unwrap(),
        "mine"
    );
    assert!(matches!(only_receipt().outcome, OpOutcome::Refused { .. }));
}

/// The plan the user confirmed was for one destination; the request that
/// reaches the executor names another (also empty, also valid). Refused
/// before `gh` runs, and neither folder is cloned into (#926 review).
#[test]
fn a_request_that_is_not_the_confirmed_plan_is_refused() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new(CLONE_OK);
    let confirmed = fixture.root.join("widgets");
    let plan = plan_clone(&fixture.request(&confirmed));
    assert_eq!(plan.disposition, PlanDisposition::Ready);
    let other = fixture.root.join("elsewhere");
    std::fs::create_dir(&other).unwrap();

    let report = execute_clone(&fixture.request(&other), &plan);
    assert!(
        matches!(&report.result, Err(GitError::Blocked(note))
            if matches!(**note, PlanNote::Clone(CloneNote::PlanMismatch { .. }))),
        "{:?}",
        report.result
    );
    assert!(!fixture.attempted(), "gh repo clone must not run");
    assert!(
        !confirmed.exists(),
        "the confirmed destination was not created"
    );
    assert_eq!(
        std::fs::read_dir(&other).unwrap().count(),
        0,
        "nothing cloned into the other one"
    );
    assert!(matches!(only_receipt().outcome, OpOutcome::Refused { .. }));
}

/// A clone that fails part-way leaves its files where they are, and the
/// receipt names the folder so the user can decide.
#[test]
fn a_failed_clone_keeps_what_it_wrote_and_says_where() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new(
        "mkdir -p \"$4\" && echo half > \"$4/partial\"; echo 'fatal: early EOF' >&2; exit 128",
    );
    let dest = fixture.root.join("widgets");
    let request = fixture.request(&dest);
    let report = execute_clone(&request, &plan_clone(&request));
    let Err(GitError::Other(error)) = &report.result else {
        panic!("{:?}", report.result);
    };
    assert!(error.contains("early EOF"), "{error}");
    assert_eq!(
        std::fs::read_to_string(dest.join("partial")).unwrap(),
        "half\n"
    );
    let OpOutcome::Failed { error } = only_receipt().outcome else {
        panic!("expected a Failed receipt");
    };
    assert!(
        error.contains(&dest.display().to_string()) && error.contains("did not delete"),
        "{error}"
    );
}

/// Past its deadline the clone's own process group is stopped — `gh` and
/// what it started — the outcome is Unknown, and the partial clone is kept.
#[test]
fn a_clone_past_its_deadline_is_stopped_kept_and_unknown() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new("mkdir -p \"$4\" && echo half > \"$4/partial\"; exec sleep 60");
    let dest = fixture.root.join("widgets");
    let request = fixture.request(&dest);
    // Long enough for the fake to write its partial file even on a loaded
    // machine; the deadline is what is under test, not its length.
    let report = execute_clone_within(&request, &plan_clone(&request), Duration::from_secs(5));
    let Err(GitError::TerminationUnknown(termination)) = &report.result else {
        panic!("{:?}", report.result);
    };
    assert!(
        termination.child_stopped(),
        "the clone's process group was stopped: {termination:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("partial")).unwrap(),
        "half\n"
    );
    let OpOutcome::Unknown { evidence, .. } = only_receipt().outcome else {
        panic!("expected an Unknown receipt");
    };
    assert!(
        evidence.contains(&dest.display().to_string()) && evidence.contains("did not delete"),
        "{evidence}"
    );
}

/// The folder is a repository, but not of what was asked for.
#[test]
fn a_clone_whose_origin_is_another_repository_is_partial() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new(
        "git clone -q '$BARE' \"$4\" && \
         git -C \"$4\" remote set-url origin https://github.com/someone/else.git",
    );
    let dest = fixture.root.join("widgets");
    let request = fixture.request(&dest);
    let report = execute_clone(&request, &plan_clone(&request));
    assert!(report.result.is_err(), "{:?}", report.result);
    let OpOutcome::Partial { error, .. } = only_receipt().outcome else {
        panic!("expected a Partial receipt");
    };
    assert!(error.contains("github.com/someone/else"), "{error}");
}

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod test_support;
