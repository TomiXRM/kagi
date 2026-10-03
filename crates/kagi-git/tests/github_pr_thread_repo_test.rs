//! #940 review P1 — a PR's conversation, review threads and merge status are
//! read from the repository the PR lives in, never from the working
//! directory's `gh` default.
//!
//! The fixture is the case Home opens: a clone whose `origin` is repository B
//! (`github.com/b/target`) while `gh repo set-default` names A
//! (`a/default`). The stand-in `gh` answers B only when a call names B, and
//! answers A for anything left to `gh`'s own resolution: an unaddressed
//! `gh pr view`, the `{owner}` / `{repo}` placeholders and `gh repo view`.
//! PR #7 exists in both, so a read that drifts to A comes back looking valid.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use kagi_git::github::{pr_conversation, pr_merge_status, pr_review_threads};

/// PATH is process-global; the tests in this binary share it.
static ENV_LOCK: Mutex<()> = Mutex::new(());

const BASE_REPO: &str = "github.com/b/target";

struct Environment(Option<OsString>);

impl Drop for Environment {
    fn drop(&mut self) {
        match &self.0 {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
}

impl Environment {
    fn install(bin: &Path) -> Self {
        let restore = Environment(std::env::var_os("PATH"));
        let mut paths = vec![bin.to_path_buf()];
        paths.extend(std::env::split_paths(
            &restore.0.clone().unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        restore
    }
}

/// `gh` as it behaves in a clone whose default repository is A.
const GH: &str = r#"#!/bin/sh
case "$*" in
  "pr view 7 -R github.com/b/target --json reviews,comments")
    echo '{"reviews":[{"author":{"login":"b-reviewer"},"state":"APPROVED","body":"B looks right","submittedAt":"2026-10-03T00:00:00Z"}],"comments":[{"author":{"login":"b-commenter"},"body":"on B","createdAt":"2026-10-03T00:00:00Z"}]}'
    exit 0 ;;
  "pr view 7 --json reviews,comments"*)
    echo '{"reviews":[{"author":{"login":"a-reviewer"},"state":"CHANGES_REQUESTED","body":"A is wrong","submittedAt":"2026-10-03T00:00:00Z"}],"comments":[{"author":{"login":"a-commenter"},"body":"on A","createdAt":"2026-10-03T00:00:00Z"}]}'
    exit 0 ;;
  "repo view"*)
    echo 'a/default'
    exit 0 ;;
esac
case "$*" in
  "api graphql"*) ;;
  *) echo "unexpected gh $*" >&2; exit 2 ;;
esac
# A graphql read is B's only when it names B's owner and name on B's host;
# otherwise gh would have filled in the default repository.
case "$*" in
  *"--hostname github.com"*"owner=b "*"name=target "*) who=b ;;
  *) who=a ;;
esac
case "$*" in
  *mergeStateStatus*)
    echo '{"data":{"repository":{"pullRequest":{"id":"PR_'$who'","mergeStateStatus":"CLEAN","reviewThreads":{"nodes":[]},"mergeQueueEntry":null}}}}' ;;
  *)
    echo '{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"path":"'$who'.rs","line":1,"startLine":null,"originalLine":1,"diffSide":"RIGHT","isOutdated":false,"isResolved":false,"viewerCanResolve":false,"comments":{"nodes":[{"databaseId":1,"author":{"login":"'$who'-threader"},"body":"x","createdAt":"2026-10-03T00:00:00Z","diffHunk":"","replyTo":null}]}}]}}}}}' ;;
esac
"#;

/// A clone of B with the stand-in `gh` first on PATH.
fn fixture() -> (tempfile::TempDir, std::path::PathBuf, Environment) {
    let root = tempfile::tempdir().unwrap();
    let (bin, workdir) = (root.path().join("bin"), root.path().join("repo"));
    for dir in [&bin, &workdir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for args in [
        &["init", "-q"][..],
        &["remote", "add", "origin", "https://github.com/b/target.git"][..],
    ] {
        assert!(Command::new("git")
            .args(args)
            .current_dir(&workdir)
            .status()
            .unwrap()
            .success());
    }
    let gh = bin.join("gh");
    std::fs::write(&gh, GH).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let restore = Environment::install(&bin);
    (root, workdir, restore)
}

#[test]
fn the_conversation_is_read_from_the_prs_repository() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_root, workdir, _restore) = fixture();
    let (reviews, comments) = pr_conversation(&workdir, BASE_REPO, 7).unwrap();
    let authors: Vec<_> = reviews
        .iter()
        .map(|r| r.author.as_str())
        .chain(comments.iter().map(|c| c.author.as_str()))
        .collect();
    assert_eq!(authors, ["b-reviewer", "b-commenter"]);
}

#[test]
fn review_threads_are_read_from_the_prs_repository() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_root, workdir, _restore) = fixture();
    let threads = pr_review_threads(&workdir, BASE_REPO, 7).unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].path, "b.rs");
    assert_eq!(threads[0].comments[0].author, "b-threader");
}

#[test]
fn merge_status_is_read_from_the_prs_repository() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_root, workdir, _restore) = fixture();
    let status = pr_merge_status(&workdir, BASE_REPO, 7).unwrap();
    assert_eq!(status.node_id, "PR_b");
}
