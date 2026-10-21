//! #1706: shared no-write witnesses retain bytes and absence, not decoded text.
use std::process::Command;

#[path = "support/git_fixture.rs"]
mod git_fixture;
#[path = "support/repository_state.rs"]
mod repository_state;
use repository_state::RepositoryState;

#[test]
fn repository_state_preserves_bytes_and_missing_oplog() {
    if std::env::var_os("KAGI_REPOSITORY_STATE_CHILD").is_none() {
        let logs = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                child.env_remove(key);
            }
        }
        let output = child
            .args([
                "--exact",
                "repository_state_preserves_bytes_and_missing_oplog",
            ])
            .env("KAGI_REPOSITORY_STATE_CHILD", "1")
            .env("KAGI_LOG_DIR", logs.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(output.status.success(), "snapshot child failed: {output:?}");
        return;
    }
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git_fixture::init_repo(repo, "main");
    git_fixture::write_file(repo, "tracked.txt", "base\n");
    git_fixture::commit_all(repo, "base");
    std::fs::create_dir_all(repo.join("nested/.git")).unwrap();
    let file = repo.join("nested/.git/opaque");
    std::fs::write(&file, [0xfe, 0, b'\n']).unwrap();
    let original = RepositoryState::read(repo);
    assert_eq!(
        RepositoryState::read(repo),
        original,
        "collection itself is read-only"
    );
    std::fs::write(&file, [0xff, 0, b'\n']).unwrap();
    assert_ne!(
        RepositoryState::read(repo),
        original,
        "non-UTF8 files, even nested .git, retain exact bytes"
    );
    std::fs::write(&file, [0xfe, 0, b'\n']).unwrap();
    assert_eq!(RepositoryState::read(repo), original);

    let log = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
        .join("operations.jsonl");
    std::fs::write(&log, []).unwrap();
    assert_ne!(
        RepositoryState::read(repo),
        original,
        "missing oplog differs from empty"
    );
    let empty = RepositoryState::read(repo);
    std::fs::write(&log, [0xfe]).unwrap();
    let opaque = RepositoryState::read(repo);
    assert_ne!(opaque, empty);
    std::fs::write(&log, [0xff]).unwrap();
    assert_ne!(
        RepositoryState::read(repo),
        opaque,
        "oplog also retains non-UTF8 bytes"
    );
    std::fs::remove_file(&log).unwrap();
    assert_eq!(RepositoryState::read(repo), original);
    std::fs::create_dir(&log).unwrap();
    assert!(
        std::panic::catch_unwind(|| RepositoryState::read(repo)).is_err(),
        "only NotFound may become absent"
    );
}
