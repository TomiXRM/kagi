//! Byte-exact no-write witness for native conversation selection scenarios.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

#[path = "git_fixture.rs"]
mod git_fixture;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RepositoryState {
    head: Vec<u8>,
    staged: Vec<u8>,
    refs: Vec<u8>,
    stash: Vec<u8>,
    working: BTreeMap<PathBuf, Vec<u8>>,
    oplog: Option<Vec<u8>>,
}

impl RepositoryState {
    /// An expected, separately asserted receipt is the only allowed difference;
    /// retain byte-exact comparison of every Git field and the complete log.
    pub(crate) fn with_appended_oplog(mut self, entry: &kagi_git::oplog::OpLogEntry) -> Self {
        let bytes = self.oplog.get_or_insert_with(Vec::new);
        bytes.extend_from_slice(kagi_git::oplog::entry_to_json(entry).as_bytes());
        bytes.push(b'\n');
        self
    }

    pub(crate) fn read(repo: &Path) -> Self {
        fn output(repo: &Path, args: &[&str]) -> Vec<u8> {
            let result = Command::new("git")
                .current_dir(repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "read-only git {args:?}: {:?}",
                result.stderr
            );
            result.stdout
        }
        let working = git_fixture::worktree_files(
            repo,
            git_fixture::WorktreeFilesOptions {
                skip_nested_git: false,
                follow_directory_symlinks: true,
            },
        );
        let log =
            PathBuf::from(std::env::var_os("KAGI_LOG_DIR").expect("runner-owned log directory"))
                .join("operations.jsonl");
        let oplog = match std::fs::read(log) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("read runner oplog: {error}"),
        };
        Self {
            head: output(repo, &["rev-parse", "HEAD"]),
            staged: output(repo, &["ls-files", "--stage", "-z"]),
            refs: output(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]),
            stash: output(repo, &["stash", "list", "--format=%gd %H %gs"]),
            working,
            oplog,
        }
    }
}
