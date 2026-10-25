//! #1721: full Continue owns commit-producing metadata, not file-edit identity.
use super::continue_revision::{open_backend, physical_state};
use super::*;
use kagi_git::backend::conflict_ops::ConflictSnapshot;
use kagi_git::Backend;

fn commit_author(path: &Path, revision: &str) -> String {
    git_output(path, &["show", "-s", "--format=%an%n%ae%n%aI", revision])
}

pub(super) fn saved_fixture(fixture: TempDir) -> (TempDir, Backend) {
    let backend = open_backend(fixture.path());
    let mut resolving = backend.resolution_buffer_from_repo().unwrap();
    resolving
        .set_manual_text(Path::new("file.txt"), "METADATA SAVED RESOLUTION\n")
        .unwrap();
    backend
        .execute_conflict_save(&resolving, Path::new("file.txt"))
        .unwrap();
    (fixture, backend)
}

#[derive(PartialEq, Eq)]
enum MetadataEntry {
    Directory,
    File(Result<Vec<u8>, std::io::ErrorKind>),
    Symlink(Result<std::path::PathBuf, std::io::ErrorKind>),
    Unreadable(std::io::ErrorKind),
    Special(std::fs::FileType),
}

fn metadata_state(path: &Path) -> std::collections::BTreeMap<std::path::PathBuf, MetadataEntry> {
    fn visit(
        root: &Path,
        relative: &Path,
        entries: &mut std::collections::BTreeMap<std::path::PathBuf, MetadataEntry>,
    ) {
        let path = root.join(relative);
        let value = match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_symlink() => {
                MetadataEntry::Symlink(std::fs::read_link(&path).map_err(|error| error.kind()))
            }
            Ok(metadata) if metadata.is_dir() => MetadataEntry::Directory,
            Ok(metadata) if metadata.is_file() => {
                MetadataEntry::File(std::fs::read(&path).map_err(|error| error.kind()))
            }
            Ok(metadata) => MetadataEntry::Special(metadata.file_type()),
            Err(error) => MetadataEntry::Unreadable(error.kind()),
        };
        let directory = matches!(value, MetadataEntry::Directory);
        entries.insert(relative.to_owned(), value);
        if directory {
            match std::fs::read_dir(&path) {
                Ok(children) => {
                    for child in children {
                        let child = child.unwrap();
                        let child = relative.join(child.file_name());
                        // Object counts are independently witnessed by Git. Do not
                        // copy compressed object payloads merely to compare metadata.
                        if child != Path::new("objects") {
                            visit(root, &child, entries);
                        }
                    }
                }
                Err(error) => {
                    entries.insert(relative.to_owned(), MetadataEntry::Unreadable(error.kind()));
                }
            }
        }
    }
    let mut entries = std::collections::BTreeMap::new();
    visit(&path.join(".git"), Path::new(""), &mut entries);
    entries
}

pub(super) fn assert_stale_approval(
    path: &Path,
    backend: &Backend,
    approved: &ConflictSnapshot,
    buffer: &ResolutionBuffer,
) {
    let expected = &approved.observation.revision;
    let current = backend
        .conflict_snapshot()
        .map(|value| value.map(|value| value.observation.revision));
    let before = physical_state(path);
    let before_commit = git_output(path, &["cat-file", "-p", "HEAD"]);
    let objects = git_output(path, &["count-objects", "-v"]);
    let metadata_before = metadata_state(path);
    let plan = backend.plan_conflict_continue(&approved.session, buffer, expected);
    let route = backend.plan_conflict_continue_route(&approved.session, buffer, "side", expected);
    let stage = backend.stage_conflict_resolution(&approved.session, buffer, expected);
    let execution = backend.execute_conflict_continue(&approved.session, buffer, expected);
    let after = physical_state(path);
    let after_commit = git_output(path, &["cat-file", "-p", "HEAD"]);
    let objects_after = git_output(path, &["count-objects", "-v"]);
    let metadata_after = metadata_state(path);
    eprintln!(
        "metadata approval: original={expected:?}; current={current:?}; plan_admitted={} \
         route_admitted={} stage_admitted={} execute_admitted={} error={:?}\n\
         before={before:#?}\nafter={after:#?}\nbefore_commit={before_commit}\n\
         after_commit={after_commit}\nobjects_before={objects}\nobjects_after={objects_after}",
        plan.is_ok(),
        route.is_ok(),
        stage.is_ok(),
        execution.is_ok(),
        execution.as_ref().err().map(ToString::to_string),
    );
    assert!(
        plan.is_err()
            && route.is_err()
            && stage.is_err()
            && execution.is_err()
            && before == after
            && objects == objects_after
            && metadata_before == metadata_after,
        "an old approval must refuse changed or unreadable operation inputs before any write",
    );
}

pub(super) fn execute_fresh(
    backend: &Backend,
    current: &ConflictSnapshot,
    buffer: &ResolutionBuffer,
) {
    backend
        .plan_conflict_continue_route(
            &current.session,
            buffer,
            "side",
            &current.observation.revision,
        )
        .unwrap();
    backend
        .execute_conflict_continue(&current.session, buffer, &current.observation.revision)
        .unwrap();
}

fn changed_message_requires_new_approval(fixture: TempDir, relative: &str, source: &str) {
    let (fixture, backend) = saved_fixture(fixture);
    let path = fixture.path();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    assert!(approved.session.files.is_empty());
    let tree = git_output(path, &["write-tree"]);
    let author = commit_author(path, source);
    backend
        .plan_conflict_continue_route(
            &approved.session,
            &buffer,
            "side",
            &approved.observation.revision,
        )
        .unwrap();
    let metadata = Repository::open(path).unwrap().path().join(relative);
    let original_message = std::fs::read(&metadata).unwrap();
    let changed_message = b"EXTERNALLY CHANGED APPROVAL MESSAGE\n";
    assert_ne!(original_message, changed_message);
    std::fs::write(&metadata, changed_message).unwrap();
    let current = backend.conflict_snapshot().unwrap().unwrap();
    assert_eq!(current.session.op, approved.session.op);
    assert_stale_approval(path, &backend, &approved, &buffer);
    assert_eq!(std::fs::read(&metadata).unwrap(), changed_message);
    assert_ne!(current.observation.revision, approved.observation.revision);
    // This empty buffer proves live-context retention. The author-script case
    // below separately proves retention of actual nonempty manually edited input.
    execute_fresh(&backend, &current, &buffer);
    assert_eq!(git_output(path, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        git_output(path, &["show", "-s", "--format=%B", "HEAD"]),
        "EXTERNALLY CHANGED APPROVAL MESSAGE"
    );
    assert_eq!(commit_author(path, "HEAD"), author);
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[test]
fn rebase_message_change_requires_new_empty_continue_approval() {
    if !test_support::run_isolated() {
        return;
    }
    changed_message_requires_new_approval(
        rebase_conflict_repo(),
        "rebase-merge/message",
        "REBASE_HEAD",
    );
}

#[test]
fn cherry_pick_message_change_requires_new_empty_continue_approval() {
    if !test_support::run_isolated() {
        return;
    }
    changed_message_requires_new_approval(
        cherry_pick_conflict_repo(),
        "MERGE_MSG",
        "CHERRY_PICK_HEAD",
    );
}

#[test]
fn apply_backend_message_change_requires_new_approval() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = rebase_conflict_repo();
    git(fixture.path(), &["rebase", "--abort"]);
    assert!(!git_allow_fail(
        fixture.path(),
        &["rebase", "--apply", "main"]
    ));
    assert!(fixture
        .path()
        .join(".git/rebase-apply/final-commit")
        .is_file());
    changed_message_requires_new_approval(fixture, "rebase-apply/final-commit", "REBASE_HEAD");
}

fn nonempty_expected_tree(repo: &Repository, text: &[u8]) -> String {
    let head = repo.head().unwrap().peel_to_tree().unwrap();
    let mut tree = repo.treebuilder(Some(&head)).unwrap();
    tree.insert("file.txt", repo.blob(text).unwrap(), 0o100644)
        .unwrap();
    tree.write().unwrap().to_string()
}

#[test]
fn nonempty_input_survives_new_author_script_approval() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = rebase_conflict_repo();
    let path = fixture.path();
    let backend = open_backend(path);
    let repo = Repository::open(path).unwrap();
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    let text = "MANUALLY RETAINED NONEMPTY INPUT\n";
    buffer.set_manual_text(Path::new("file.txt"), text).unwrap();
    let tree = nonempty_expected_tree(&repo, text.as_bytes());
    let parent = git_output(path, &["rev-parse", "HEAD"]);
    let message = git_output(path, &["show", "-s", "--format=%B", "REBASE_HEAD"]);
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    assert_eq!(approved.session.files.len(), 1);
    backend
        .plan_conflict_continue_route(
            &approved.session,
            &buffer,
            "side",
            &approved.observation.revision,
        )
        .unwrap();
    std::fs::write(
        repo.path().join("rebase-merge/author-script"),
        "GIT_AUTHOR_NAME='Changed Metadata Author'\nGIT_AUTHOR_EMAIL='metadata@example.com'\nGIT_AUTHOR_DATE='@1234567890 +0000'\n",
    ).unwrap();
    let current = backend.conflict_snapshot().unwrap().unwrap();
    assert_stale_approval(path, &backend, &approved, &buffer);
    assert_ne!(current.observation.revision, approved.observation.revision);
    execute_fresh(&backend, &current, &buffer);
    assert_eq!(git_output(path, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(git_output(path, &["rev-parse", "HEAD^"]), parent);
    assert_eq!(
        git_output(path, &["show", "HEAD:file.txt"]),
        text.trim_end()
    );
    assert_eq!(
        git_output(path, &["show", "-s", "--format=%B", "HEAD"]),
        message
    );
    assert_eq!(
        git_output(path, &["show", "-s", "--format=%an%n%ae%n%at", "HEAD"]),
        "Changed Metadata Author\nmetadata@example.com\n1234567890"
    );
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[test]
fn squash_prefix_is_a_separate_cherry_pick_message_input() {
    if !test_support::run_isolated() {
        return;
    }
    let (fixture, backend) = saved_fixture(cherry_pick_conflict_repo());
    let path = fixture.path();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let tree = git_output(path, &["write-tree"]);
    let author = commit_author(path, "CHERRY_PICK_HEAD");
    std::fs::write(path.join(".git/SQUASH_MSG"), b"EXTERNAL SQUASH PRELUDE\n\n").unwrap();
    let current = backend.conflict_snapshot().unwrap().unwrap();
    assert_stale_approval(path, &backend, &approved, &buffer);
    execute_fresh(&backend, &current, &buffer);
    assert_eq!(git_output(path, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        git_output(path, &["show", "-s", "--format=%B", "HEAD"]),
        "EXTERNAL SQUASH PRELUDE\n\nside change"
    );
    assert_eq!(commit_author(path, "HEAD"), author);
}

fn pending_signoff_fixture() -> TempDir {
    let fixture = rebase_conflict_repo();
    let path = fixture.path();
    git(path, &["rebase", "--abort"]);
    write_file(path, "aux.txt", "PENDING SIGNOFF INPUT\n");
    git(path, &["add", "aux.txt"]);
    git(path, &["commit", "-qm", "pending signoff proof"]);
    assert!(!git_allow_fail(path, &["rebase", "main"]));
    fixture
}

#[test]
fn zero_byte_signoff_option_presence_requires_new_approval() {
    if !test_support::run_isolated() {
        return;
    }
    let (fixture, backend) = saved_fixture(pending_signoff_fixture());
    let path = fixture.path();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let saved = git_output(path, &["write-tree"]);
    let repo = Repository::open(path).unwrap();
    let saved = repo
        .find_tree(git2::Oid::from_str(&saved).unwrap())
        .unwrap();
    let mut tree = repo.treebuilder(Some(&saved)).unwrap();
    tree.insert(
        "aux.txt",
        repo.blob(b"PENDING SIGNOFF INPUT\n").unwrap(),
        0o100644,
    )
    .unwrap();
    let tree = tree.write().unwrap().to_string();
    let author = commit_author(path, "side");
    std::fs::write(path.join(".git/rebase-merge/signoff"), b"").unwrap();
    let current = backend.conflict_snapshot().unwrap().unwrap();
    assert_stale_approval(path, &backend, &approved, &buffer);
    execute_fresh(&backend, &current, &buffer);
    let expected = format!(
        "pending signoff proof\n\nSigned-off-by: {} <{}>",
        git_output(path, &["config", "user.name"]),
        git_output(path, &["config", "user.email"])
    );
    assert_eq!(git_output(path, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        git_output(path, &["show", "-s", "--format=%B", "HEAD"]),
        expected
    );
    // The stopped step consumes its already-generated message. The flag is
    // consumed while constructing the subsequent pending pick's message.
    assert_eq!(
        git_output(path, &["show", "-s", "--format=%B", "HEAD^"]),
        "side change"
    );
    assert_eq!(commit_author(path, "HEAD"), author);
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[cfg(unix)]
#[test]
fn unreadable_metadata_and_state_directory_refuse_before_writes() {
    use std::os::unix::fs::PermissionsExt;
    if !test_support::run_isolated() {
        return;
    }
    struct Restore(std::path::PathBuf, std::fs::Permissions);
    impl Drop for Restore {
        fn drop(&mut self) {
            std::fs::set_permissions(&self.0, self.1.clone()).unwrap();
        }
    }
    for relative in ["rebase-merge/author-script", "rebase-merge"] {
        let (fixture, backend) = saved_fixture(rebase_conflict_repo());
        let path = fixture.path();
        let approved = backend.conflict_snapshot().unwrap().unwrap();
        let buffer = backend.resolution_buffer_from_repo().unwrap();
        let target = path.join(".git").join(relative);
        let restore = Restore(
            target.clone(),
            std::fs::metadata(&target).unwrap().permissions(),
        );
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(backend.conflict_snapshot().is_err());
        assert_stale_approval(path, &backend, &approved, &buffer);
        drop(restore);
    }
}

#[cfg(unix)]
#[test]
fn nonregular_message_inputs_are_not_authorizable() {
    if !test_support::run_isolated() {
        return;
    }
    for kind in ["directory", "symlink", "fifo"] {
        let (fixture, backend) = saved_fixture(cherry_pick_conflict_repo());
        let path = fixture.path();
        let approved = backend.conflict_snapshot().unwrap().unwrap();
        let buffer = backend.resolution_buffer_from_repo().unwrap();
        let target = path.join(".git/MERGE_MSG");
        let content = std::fs::read(&target).unwrap();
        std::fs::remove_file(&target).unwrap();
        match kind {
            "symlink" => {
                let backing = path.join(".git/owned-message-target");
                std::fs::write(&backing, content).unwrap();
                std::os::unix::fs::symlink(backing, target).unwrap();
            }
            "fifo" => assert!(std::process::Command::new("mkfifo")
                .arg(target)
                .status()
                .unwrap()
                .success()),
            _ => std::fs::create_dir(target).unwrap(),
        }
        assert!(backend.conflict_snapshot().is_err());
        assert_stale_approval(path, &backend, &approved, &buffer);
    }
}
