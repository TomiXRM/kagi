#[path = "../../../tests/support/revision_fixture.rs"]
mod fixture;
use kagi_git::{revision::RevisionError, Backend, GitError};

#[test]
fn direct_revision_resolution_is_typed_and_unbounded() {
    let dir = fixture::history();
    let history = fixture::git(dir.path(), &["rev-list", "HEAD"]);
    let commits: Vec<_> = history.lines().collect();
    let root = *commits.last().unwrap();
    let (prefix, candidates) = fixture::collision(&commits);
    let backend = Backend::open(dir.path()).unwrap();
    match backend.resolve_commit(&prefix) {
        Err(GitError::Revision(RevisionError::Ambiguous {
            candidates: actual, ..
        })) => {
            for candidate in candidates {
                assert!(actual.contains(&candidate));
            }
        }
        result => panic!("expected typed ambiguity: {result:?}"),
    }
    assert_eq!(backend.resolve_commit(root).unwrap().id.0, root);
    assert!(matches!(
        backend.resolve_commit("missing-revision"),
        Err(GitError::Revision(RevisionError::NotFound { .. }))
    ));
    let blob = fixture::git(dir.path(), &["hash-object", "-w", "--stdin"]);
    assert!(matches!(
        backend.resolve_commit(&blob),
        Err(GitError::Revision(RevisionError::NotACommit { .. }))
    ));
    let tree = fixture::git(dir.path(), &["rev-parse", "HEAD^{tree}"]);
    assert!(matches!(
        backend.resolve_commit(&tree),
        Err(GitError::Revision(RevisionError::NotACommit { .. }))
    ));
    fixture::git(
        dir.path(),
        &[
            "-c",
            "user.name=Survey",
            "-c",
            "user.email=survey@example.invalid",
            "tag",
            "-a",
            "root-tag",
            "-m",
            "root",
            root,
        ],
    );
    assert_eq!(backend.resolve_commit("root-tag").unwrap().id.0, root);
    assert_eq!(backend.resolve_commit("HEAD~2499").unwrap().id.0, root);
}
