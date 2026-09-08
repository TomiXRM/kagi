//! #623: the stash push CLI must touch the planned repository and must never
//! hand back an OID it did not create.
//!
//! Both properties are tested against **real** concurrency and a real inherited
//! environment, not a mocked one: an `ssh`-style `git` wrapper on `PATH` pushes
//! a competing stash inside the window (before or after Kagi's own `git stash
//! push`), and the repository-local Git environment variables are exported for
//! the whole operation the way a shell would export them.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use kagi_git::{Backend, GitError, Operation, OperationOutcome};

/// Kagi's own message for the stash under test, so the identification has
/// something to match on the way the auto-stash path does.
const MESSAGE: &str = "kagi: auto-stash before pull";

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git failed to start");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// One commit on `main`, two tracked files, clean.
fn build_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("mine.txt"), "base\n").unwrap();
    std::fs::write(dir.join("theirs.txt"), "base\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "base"]);
}

fn real_git() -> String {
    let out = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// `refs/stash` entries as `(oid, message)`, newest first.
fn stash_entries(dir: &Path) -> Vec<(String, String)> {
    git(dir, &["stash", "list", "--format=%H%x00%gs"])
        .lines()
        .map(|line| {
            let (oid, message) = line.split_once('\0').expect("NUL-framed stash row");
            (oid.to_string(), message.to_string())
        })
        .collect()
}

fn oid_of_message(dir: &Path, message: &str) -> String {
    stash_entries(dir)
        .into_iter()
        .find(|(_, stored)| stored == message)
        .unwrap_or_else(|| panic!("no stash entry says {message:?}: {:?}", stash_entries(dir)))
        .0
}

/// Install a `git` wrapper that pushes a competing stash of `theirs.txt` either
/// side of the real invocation, exactly once, and return the directory to put
/// on `PATH`. This is the race the old "read `refs/stash` afterwards" code lost:
/// nothing in the app-layer write lease stops a terminal, hook or other tool
/// from stashing at the same moment.
///
/// `message` is the competing stash's message: a different one is a stash kagi
/// can tell apart, kagi's own is one it cannot. The once-only marker lives
/// beside the wrapper, never in the worktree — an untracked file left in the
/// repository would fail the push's own worktree verification and mask what is
/// being tested.
fn interfering_git(bin: &Path, repo: &Path, when: &str, message: &str) -> PathBuf {
    std::fs::create_dir_all(bin).unwrap();
    let script = format!(
        r#"#!/bin/sh
real='{real}'
repo='{repo}'
marker='{marker}'
interfere() {{
    if [ -e "$marker" ]; then return 0; fi
    : > "$marker"
    echo theirs > "$repo/theirs.txt"
    "$real" -C "$repo" stash push -q -m '{message}' -- theirs.txt
}}
case " $* " in
    *" stash "*)
        if [ '{when}' = before ]; then interfere; fi
        "$real" "$@"
        status=$?
        if [ '{when}' = after ]; then interfere; fi
        exit $status
        ;;
esac
exec "$real" "$@"
"#,
        real = real_git(),
        repo = repo.display(),
        marker = bin.join("interfered").display(),
    );
    let path = bin.join("git");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin.to_path_buf()
}

struct PathGuard(Option<std::ffi::OsString>);

impl PathGuard {
    fn prepend(dir: PathBuf) -> Self {
        let old = std::env::var_os("PATH");
        let mut paths = vec![dir];
        paths.extend(std::env::split_paths(&old.clone().unwrap_or_default()));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        Self(old)
    }
}

impl Drop for PathGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(old) => std::env::set_var("PATH", old),
            None => std::env::remove_var("PATH"),
        }
    }
}

/// Plan and run the stash push the way the auto-stash path does.
fn kagi_stash_push(repo: &Path) -> Result<String, GitError> {
    let mut backend = Backend::open(repo).expect("open");
    let op = Operation::StashPush {
        message: Some(MESSAGE.to_string()),
        include_untracked: true,
    };
    let plan = backend.plan(&op).expect("plan");
    match backend.run(&op, &plan)? {
        OperationOutcome::StashPush { oid } => Ok(oid),
        other => panic!("expected a stash-push outcome, got {other:?}"),
    }
}

/// `(repo, bin)` under one temp root, with the repo dirtied and ready to stash.
///
/// `bin` is a **sibling** of the worktree, never inside it: kagi pushes with
/// `--include-untracked`, so a wrapper living under the repo would be stashed
/// away mid-run — the script would vanish from disk while git was executing it.
fn fixture(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let root = tmp.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    build_repo(&repo);
    std::fs::write(repo.join("mine.txt"), "kagi work\n").unwrap();
    (repo, root.join("bin"))
}

/// The undisturbed case still returns Kagi's own stash.
#[test]
fn push_returns_its_own_oid_when_nothing_interferes() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (repo, _bin) = fixture(&tmp);

    let oid = kagi_stash_push(&repo).expect("stash push");

    assert_eq!(oid, oid_of_message(&repo, &format!("On main: {MESSAGE}")));
    assert_eq!(git(&repo, &["show", "refs/stash:mine.txt"]), "kagi work\n");
}

/// An external stash landing in the window *before* Kagi's own push moves the
/// stack under it. The returned OID must still be Kagi's, never the other one.
#[test]
fn external_stash_pushed_before_kagis_is_never_returned() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (repo, bin) = fixture(&tmp);
    let bin = interfering_git(&bin, &repo, "before", "external");

    let oid = {
        let _path = PathGuard::prepend(bin);
        kagi_stash_push(&repo)
    }
    .expect("stash push");

    let mine = oid_of_message(&repo, &format!("On main: {MESSAGE}"));
    let theirs = oid_of_message(&repo, "On main: external");
    assert_ne!(
        mine, theirs,
        "the fixture must produce two distinct stashes"
    );
    assert_eq!(oid, mine, "Kagi returned the external stash's OID");
    assert_eq!(
        git(&repo, &["show", &format!("{oid}:mine.txt")]),
        "kagi work\n"
    );
}

/// An external stash landing *after* Kagi's push leaves the `refs/stash` tip
/// pointing at someone else's work — the #618 mistake this closes.
#[test]
fn external_stash_pushed_after_kagis_is_never_returned() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (repo, bin) = fixture(&tmp);
    let bin = interfering_git(&bin, &repo, "after", "external");

    let oid = {
        let _path = PathGuard::prepend(bin);
        kagi_stash_push(&repo)
    }
    .expect("stash push");

    let mine = oid_of_message(&repo, &format!("On main: {MESSAGE}"));
    let theirs = oid_of_message(&repo, "On main: external");
    assert_ne!(
        mine, theirs,
        "the fixture must produce two distinct stashes"
    );
    assert_eq!(
        git(&repo, &["rev-parse", "refs/stash"]).trim(),
        theirs,
        "the fixture must leave the external stash on top",
    );
    assert_eq!(oid, mine, "Kagi returned the tip instead of its own stash");
    assert_eq!(
        git(&repo, &["show", &format!("{oid}:mine.txt")]),
        "kagi work\n"
    );
}

/// Two stashes Kagi cannot tell apart are reported as unidentified, with the
/// typed error the UI turns into "your work is saved, the entry is unverified".
/// An OID is never guessed.
#[test]
fn indistinguishable_external_stash_is_reported_unidentified() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (repo, bin) = fixture(&tmp);
    // The external push uses Kagi's own message, from Kagi's own HEAD: nothing
    // distinguishes the two entries.
    let bin = interfering_git(&bin, &repo, "before", MESSAGE);

    let result = {
        let _path = PathGuard::prepend(bin);
        kagi_stash_push(&repo)
    };

    assert!(
        matches!(result, Err(GitError::StashIdentityUnverified(_))),
        "expected an unidentified-stash error, got {result:?}"
    );
    // Both stashes exist: the user's work was saved either way, which is what
    // the UI message promises.
    assert_eq!(stash_entries(&repo).len(), 2, "{:?}", stash_entries(&repo));
}

/// #623 P1: an inherited `GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE`
/// overrides `-C` and `current_dir`, so without clearing them the push writes
/// into whatever repository the *parent process* was pointed at.
#[test]
fn repository_env_vars_cannot_redirect_the_push() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let planned = tmp.path().join("planned");
    let decoy = tmp.path().join("decoy");
    std::fs::create_dir_all(&planned).unwrap();
    std::fs::create_dir_all(&decoy).unwrap();
    build_repo(&planned);
    build_repo(&decoy);
    std::fs::write(planned.join("mine.txt"), "kagi work\n").unwrap();
    let decoy_index_before = git(&decoy, &["ls-files", "--stage"]);

    let vars = [
        ("GIT_DIR", decoy.join(".git")),
        ("GIT_WORK_TREE", decoy.clone()),
        ("GIT_INDEX_FILE", decoy.join(".git").join("index")),
        ("GIT_OBJECT_DIRECTORY", decoy.join(".git").join("objects")),
    ];
    for (key, value) in &vars {
        std::env::set_var(key, value);
    }
    let pushed = kagi_stash_push(&planned);
    for (key, _) in &vars {
        std::env::remove_var(key);
    }
    let oid = pushed.expect("stash push");

    // The planned repository is the one that changed.
    assert_eq!(
        oid,
        oid_of_message(&planned, &format!("On main: {MESSAGE}"))
    );
    assert_eq!(
        git(&planned, &["show", "refs/stash:mine.txt"]),
        "kagi work\n"
    );
    assert_eq!(
        std::fs::read_to_string(planned.join("mine.txt")).unwrap(),
        "base\n",
        "the planned worktree must be restored to HEAD"
    );

    // The decoy is untouched: no stash, no index or worktree change.
    assert!(
        stash_entries(&decoy).is_empty(),
        "the push leaked into the decoy repository: {:?}",
        stash_entries(&decoy)
    );
    assert_eq!(git(&decoy, &["ls-files", "--stage"]), decoy_index_before);
    assert!(git(&decoy, &["status", "--porcelain"]).is_empty());
}

#[path = "support/isolated.rs"]
mod test_support;
