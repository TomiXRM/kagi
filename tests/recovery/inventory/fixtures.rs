use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::macos::{build_fixture, git};

#[derive(Clone, Copy)]
pub(super) enum FixtureKind {
    Basic,
    Conflict,
    ConflictSequencer,
    SlowRead,
    LinkedWorktree,
    LockedWorktree,
    Stash,
    Branches,
    RemoteBranch,
    Tag,
    TagRemote,
    Dirty,
    CommitBlocker,
    ForceLease,
    CherryPick,
    Oplog,
}

/// Owns both the working repository and any remote used by a capture. Neither
/// may disappear until the mounted app has been drawn and unmounted.
pub(super) struct Fixture {
    pub repo: PathBuf,
    _root: tempfile::TempDir,
    _extra: Vec<tempfile::TempDir>,
    hold: RefCell<Option<crate::evidence_support::Reply<()>>>,
}

impl Fixture {
    pub fn build(kind: FixtureKind) -> Self {
        if matches!(kind, FixtureKind::Conflict) {
            let root = crate::app_conflict::content_fixture();
            return Self {
                repo: root.path().to_path_buf(),
                _root: root,
                _extra: Vec::new(),
                hold: RefCell::new(None),
            };
        }
        let root = build_fixture();
        let repo = root.path().to_path_buf();
        let mut fixture = Self {
            repo,
            _root: root,
            _extra: Vec::new(),
            hold: RefCell::new(None),
        };
        if matches!(
            kind,
            FixtureKind::RemoteBranch
                | FixtureKind::ForceLease
                | FixtureKind::SlowRead
                | FixtureKind::TagRemote
        ) {
            fixture.add_remote();
        }
        let p = fixture.repo.as_path();
        match kind {
            FixtureKind::Basic | FixtureKind::Conflict => {}
            FixtureKind::Branches => {
                git(p, &["branch", "feature", "HEAD~1"]);
                git(p, &["branch", "merged", "HEAD~1"]);
            }
            FixtureKind::Tag | FixtureKind::TagRemote => git(p, &["tag", "v1.0.0"]),
            FixtureKind::Dirty => {
                std::fs::write(p.join("README.md"), "# fixture\nunsaved work\n").unwrap();
                std::fs::write(p.join("untracked.txt"), "new file\n").unwrap();
            }
            FixtureKind::CommitBlocker => {
                std::fs::write(
                    p.join("README.md"),
                    "<<<<<<< HEAD\nfirst\n=======\nsecond\n>>>>>>> feature\n",
                )
                .unwrap();
                git(p, &["add", "README.md"]);
            }
            FixtureKind::Stash => {
                std::fs::write(p.join("README.md"), "# fixture\nstashed work\n").unwrap();
                git(p, &["stash", "push", "-qm", "Inventory saved work"]);
            }
            FixtureKind::LinkedWorktree | FixtureKind::LockedWorktree => {
                let linked = p.join("linked");
                git(
                    p,
                    &[
                        "worktree",
                        "add",
                        "-q",
                        "-b",
                        "linked",
                        linked.to_str().unwrap(),
                        "HEAD~1",
                    ],
                );
                if matches!(kind, FixtureKind::LockedWorktree) {
                    git(
                        p,
                        &[
                            "worktree",
                            "lock",
                            "--reason",
                            "Review in progress",
                            linked.to_str().unwrap(),
                        ],
                    );
                }
            }
            FixtureKind::RemoteBranch => {
                // The remote and its ref were created before mounting the app.
                git(p, &["push", "-q", "origin", "HEAD:refs/heads/feature"]);
                git(p, &["fetch", "-q", "origin"]);
            }
            FixtureKind::SlowRead => {
                git(p, &["push", "-qu", "origin", "main"]);
                git(p, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
            }
            FixtureKind::ForceLease => {
                git(p, &["push", "-qu", "origin", "main"]);
                std::fs::write(p.join("f.txt"), "new local commit to force push\n").unwrap();
                git(p, &["add", "f.txt"]);
                git(p, &["commit", "-qm", "Local tip differs from remote"]);
            }
            FixtureKind::CherryPick => {
                git(p, &["checkout", "-q", "-b", "feature", "HEAD~1"]);
                std::fs::write(p.join("feature.txt"), "Independent feature\n").unwrap();
                git(p, &["add", "feature.txt"]);
                git(p, &["commit", "-qm", "Independent feature commit"]);
                git(p, &["checkout", "-q", "main"]);
            }
            FixtureKind::ConflictSequencer => {
                git(p, &["checkout", "-qb", "feature"]);
                std::fs::write(p.join("README.md"), "feature\n").unwrap();
                git(p, &["commit", "-qam", "feature"]);
                git(p, &["checkout", "-q", "main"]);
                std::fs::write(p.join("README.md"), "main\n").unwrap();
                git(p, &["commit", "-qam", "main"]);
                let status = Command::new("git")
                    .current_dir(p)
                    .args(["cherry-pick", "feature"])
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .status()
                    .unwrap();
                assert!(!status.success(), "sequencer fixture must conflict");
            }
            FixtureKind::Oplog => {
                let mut backend = kagi_git::Backend::open(p).unwrap();
                let op = kagi_git::Operation::CreateBranch {
                    name: "restore-anchor".into(),
                    at: oid(p, "HEAD"),
                };
                let plan = backend.plan(&op).unwrap();
                assert!(plan.blockers.is_empty());
                backend.run(&op, &plan).unwrap();
            }
        }
        fixture
    }

    pub fn hold(&self, reply: crate::evidence_support::Reply<()>) {
        assert!(self.hold.borrow_mut().replace(reply).is_none());
    }

    /// Release a held read before unmounting the GPUI entity.
    pub fn finish(&self, cx: &mut gpui::VisualTestAppContext) {
        if let Some(reply) = self.hold.borrow_mut().take() {
            reply.send(());
            cx.run_until_parked();
        }
    }

    fn add_remote(&mut self) {
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare"]);
        git(
            &self.repo,
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        self._extra.push(remote);
    }
}

pub(super) fn oid(repo: &Path, revision: &str) -> kagi_git::CommitId {
    let out = Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", revision])
        .output()
        .unwrap();
    assert!(out.status.success(), "rev-parse {revision} failed");
    kagi_git::CommitId(String::from_utf8(out.stdout).unwrap().trim().into())
}
