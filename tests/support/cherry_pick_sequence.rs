//! Three picks: two clean additions followed by a conflicting edit (#1127).
use std::path::Path;
use tempfile::TempDir;

#[path = "git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, git_succeeds, init_repo, write_file};

pub struct Fixture {
    dir: TempDir,
    pub start: String,
}

impl Fixture {
    pub fn new(sequence: bool, detached: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo(root, "main");
        write_file(root, "a.txt", "base\n");
        commit_all(root, "base");
        git(root, &["switch", "-c", "side"]);
        write_file(root, "first.txt", "first pick\n");
        commit_all(root, "first");
        let c1 = git_output(root, &["rev-parse", "HEAD"]);
        write_file(root, "second.txt", "second pick\n");
        commit_all(root, "second");
        let c2 = git_output(root, &["rev-parse", "HEAD"]);
        write_file(root, "a.txt", "side\n");
        write_file(root, "third.txt", "clean part of conflicting pick\n");
        commit_all(root, "third");
        let c3 = git_output(root, &["rev-parse", "HEAD"]);
        git(root, &["switch", "main"]);
        write_file(root, "a.txt", "main\n");
        commit_all(root, "main diverges");
        let start = git_output(root, &["rev-parse", "HEAD"]);
        if detached {
            git(root, &["switch", "--detach"]);
        }
        let args = if sequence {
            vec!["cherry-pick", c1.as_str(), c2.as_str(), c3.as_str()]
        } else {
            vec!["cherry-pick", c3.as_str()]
        };
        assert!(!git_succeeds(root, &args), "third pick must conflict");
        if sequence {
            assert_eq!(
                std::fs::read_to_string(root.join(".git/sequencer/head"))
                    .unwrap()
                    .trim(),
                start
            );
        }
        Self { dir, start }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}
