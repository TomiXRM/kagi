//! Serialized application fixtures with repository and oplog lifetimes tied together.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use kagi::app::{PullRequest, RunRequest, Sessions};
use kagi_git::{Backend, Operation};

use crate::git_fixture::{commit_all, git_output as git, init_repo};

static ENV_LOCK: Mutex<()> = Mutex::new(());

pub struct AppFixture {
    // Keep the original field drop order: restore the environment in Drop,
    // then release the mutex before removing the temporary repository/logs.
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    pub repo: PathBuf,
    old_log: Option<std::ffi::OsString>,
}

impl Drop for AppFixture {
    fn drop(&mut self) {
        match &self.old_log {
            Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}

impl AppFixture {
    fn new(setup: impl FnOnce(&Path, &Path)) -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_repo(&repo, "main");
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        commit_all(&repo, "initial");
        setup(&repo, &root);
        let old_log = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", root.join("log"));
        Self {
            _guard: guard,
            _dir: dir,
            repo,
            old_log,
        }
    }

    pub fn pull() -> Self {
        Self::new(|repo, root| {
            let bare = root.join("origin.git");
            git(repo, &["init", "--bare", "-q", bare.to_str().unwrap()]);
            git(repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
            git(repo, &["push", "-q", "-u", "origin", "main"]);
        })
    }

    pub fn checkout() -> Self {
        Self::new(|repo, _| {
            git(repo, &["branch", "side"]);
        })
    }

    pub fn pull_request(&self, sessions: &mut Sessions) -> PullRequest {
        let session = sessions.attach(self.repo.clone());
        let owner = sessions.attachment(session).expect("attached");
        let backend = Backend::open(&self.repo).unwrap();
        PullRequest {
            owner,
            name: "pull",
            path: self.repo.clone(),
            repo: backend.write_repo_id().unwrap(),
            plan: Arc::new(backend.plan_pull().expect("plan pull")),
            auto_stash: true,
            promised_dirty: None,
        }
    }

    /// A checkout-to-`side` request owned by the one attached session.
    pub fn checkout_request(&self, sessions: &mut Sessions) -> RunRequest {
        let session = sessions.attach(self.repo.clone());
        let owner = sessions.attachment(session).expect("attached");
        let backend = Backend::open(&self.repo).unwrap();
        let plan = backend
            .plan(&Operation::Checkout {
                branch: "side".into(),
            })
            .expect("plan checkout");
        RunRequest {
            owner,
            name: "checkout",
            path: self.repo.clone(),
            repo: backend.write_repo_id().unwrap(),
            plan: Arc::new(plan),
            remote: Vec::new(),
        }
    }
}
