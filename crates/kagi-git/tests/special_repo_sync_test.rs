//! #1137: Sync must refuse variants its backups/checkout cannot preserve.
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod isolated;

use git_fixture::{commit_all, git, git_output, init_repo, write_file};
use kagi_domain::plan_note::{CommonNote, PlanNote};
use kagi_git::{backend::ExecutionPolicy, Backend, Operation};
use std::path::Path;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path(), "main");
    write_file(dir.path(), "plain.txt", "base\n");
    commit_all(dir.path(), "base");
    git(dir.path(), &["branch", "upstream"]);
    git(dir.path(), &["config", "branch.main.remote", "."]);
    git(
        dir.path(),
        &["config", "branch.main.merge", "refs/heads/upstream"],
    );
    write_file(dir.path(), "plain.txt", "local\n");
    commit_all(dir.path(), "local ahead of upstream");
    dir
}

fn backend(dir: &Path) -> Backend {
    let mut backend = Backend::open_with_policy(dir, ExecutionPolicy::human(false)).unwrap();
    backend.set_auto_snapshot(false);
    backend
}
fn op() -> Operation {
    Operation::SyncToRemote {
        branch: "main".into(),
    }
}

fn assert_refused(dir: &Path, reason: &str) {
    let mut backend = backend(dir);
    let head = git_output(dir, &["rev-parse", "HEAD"]);
    let index = std::fs::read(dir.join(".git/index")).unwrap();
    let bytes = std::fs::read(dir.join("plain.txt")).unwrap();
    let refs = git_output(dir, &["for-each-ref", "--format=%(refname) %(objectname)"]);
    let plan = backend.plan(&op()).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|n| n.message_en().contains(reason)),
        "missing {reason} refusal: {:?}",
        plan.blockers
    );
    assert!(backend.run(&op(), &plan).is_err());
    assert_eq!(git_output(dir, &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read(dir.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(dir.join("plain.txt")).unwrap(), bytes);
    assert_eq!(
        git_output(dir, &["for-each-ref", "--format=%(refname) %(objectname)"]),
        refs
    );
}

#[test]
fn special_sync_sparse_cone_and_non_cone_refuse_without_checkout() {
    if !isolated::run_isolated() {
        return;
    }
    for cone in [true, false] {
        let dir = fixture();
        git(
            dir.path(),
            &[
                "sparse-checkout",
                "init",
                if cone { "--cone" } else { "--no-cone" },
            ],
        );
        assert_refused(
            dir.path(),
            if cone {
                "cone sparse-checkout"
            } else {
                "non-cone sparse-checkout"
            },
        );
    }
}

#[test]
fn special_sync_sparse_config_or_patterns_alone_refuse() {
    if !isolated::run_isolated() {
        return;
    }
    for config in [true, false] {
        let dir = fixture();
        if config {
            git(dir.path(), &["config", "core.sparseCheckoutCone", "true"]);
        } else {
            write_file(dir.path(), ".git/info/sparse-checkout", "/plain.txt\n");
        }
        assert_refused(dir.path(), "sparse-checkout");
    }
}

fn submodule_fixture() -> TempDir {
    let dir = fixture();
    let source = dir.path().join("source");
    std::fs::create_dir(&source).unwrap();
    init_repo(&source, "main");
    write_file(&source, "file.txt", "submodule base\n");
    commit_all(&source, "submodule base");
    git(
        dir.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "./source",
            "module",
        ],
    );
    git(dir.path(), &["add", ".gitmodules", "module"]);
    git(dir.path(), &["commit", "-qm", "add submodule"]);
    dir
}

#[test]
fn special_sync_dirty_submodule_variants_and_uninitialized_refuse() {
    if !isolated::run_isolated() {
        return;
    }
    for variant in ["worktree", "staged", "untracked", "head", "uninitialized"] {
        let dir = submodule_fixture();
        let module = dir.path().join("module");
        match variant {
            "worktree" => write_file(&module, "file.txt", "precious local work\n"),
            "staged" => {
                write_file(&module, "file.txt", "precious staged work\n");
                git(&module, &["add", "file.txt"]);
            }
            "untracked" => write_file(&module, "untracked.txt", "precious untracked work\n"),
            "head" => {
                write_file(&module, "file.txt", "different checked-out commit\n");
                commit_all(&module, "local submodule commit");
            }
            "uninitialized" => {
                git(dir.path(), &["submodule", "deinit", "-q", "module"]);
            }
            _ => unreachable!(),
        }
        // Even ignore=all must not hide work that Kagi cannot back up.
        git(dir.path(), &["config", "submodule.module.ignore", "all"]);
        let before = if variant == "uninitialized" {
            None
        } else {
            Some(git_output(&module, &["status", "--porcelain"]))
        };
        let nested_bytes =
            (variant != "uninitialized").then(|| std::fs::read(module.join("file.txt")).unwrap());
        let nested_index =
            (variant != "uninitialized").then(|| git_output(&module, &["ls-files", "--stage"]));
        assert_refused(
            dir.path(),
            if variant == "uninitialized" {
                "uninitialized submodule"
            } else {
                "dirty submodule"
            },
        );
        if let Some(before) = before {
            assert_eq!(git_output(&module, &["status", "--porcelain"]), before);
        }
        if let Some(bytes) = nested_bytes {
            assert_eq!(std::fs::read(module.join("file.txt")).unwrap(), bytes);
            assert_eq!(
                git_output(&module, &["ls-files", "--stage"]),
                nested_index.unwrap()
            );
        }
        if variant == "untracked" {
            assert_eq!(
                std::fs::read_to_string(module.join("untracked.txt")).unwrap(),
                "precious untracked work\n"
            );
        }
    }
}

#[test]
fn special_sync_lfs_worktree_and_incoming_paths_refuse() {
    if !isolated::run_isolated() {
        return;
    }
    for incoming in [false, true] {
        let dir = fixture();
        if incoming {
            git(dir.path(), &["checkout", "-q", "upstream"]);
        }
        write_file(
            dir.path(),
            ".gitattributes",
            "*.bin filter=lfs diff=lfs merge=lfs -text\n",
        );
        write_file(dir.path(), "asset.bin", "version https://git-lfs.github.com/spec/v1\noid sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nsize 42\n");
        commit_all(dir.path(), "LFS pointer without git-lfs");
        if incoming {
            git(dir.path(), &["checkout", "-q", "main"]);
        } else {
            write_file(dir.path(), "asset.bin", "precious real LFS content\n");
        }
        assert_refused(dir.path(), "external filter (lfs)");
        if !incoming {
            assert_eq!(
                std::fs::read_to_string(dir.path().join("asset.bin")).unwrap(),
                "precious real LFS content\n"
            );
        }
    }
}

#[test]
fn special_sync_preflight_refuses_late_sparse_submodule_or_attributes() {
    if !isolated::run_isolated() {
        return;
    }
    for variant in ["sparse", "submodule", "lfs"] {
        let dir = if variant == "submodule" {
            submodule_fixture()
        } else {
            fixture()
        };
        let mut backend = backend(dir.path());
        backend.set_auto_snapshot(true);
        let plan = backend.plan(&op()).unwrap();
        assert!(plan.blockers.is_empty());
        match variant {
            "sparse" => git(dir.path(), &["config", "core.sparseCheckout", "true"]),
            "submodule" => write_file(
                &dir.path().join("module"),
                "file.txt",
                "late precious work\n",
            ),
            "lfs" => write_file(dir.path(), ".git/info/attributes", "plain.txt filter=lfs\n"),
            _ => unreachable!(),
        }
        let head = git_output(dir.path(), &["rev-parse", "HEAD"]);
        let index = std::fs::read(dir.path().join(".git/index")).unwrap();
        let refs = git_output(
            dir.path(),
            &["for-each-ref", "--format=%(refname) %(objectname)"],
        );
        let error = backend
            .run(&op(), &plan)
            .expect_err("late special variant must refuse before backup");
        assert!(
            matches!(
                error.blocker(),
                Some(PlanNote::Common(
                    CommonNote::SparseCheckoutUnsupported { .. }
                        | CommonNote::SubmoduleCheckoutUnsupported { .. }
                        | CommonNote::ExternalFilter { .. }
                ))
            ),
            "late {variant} refusal must remain typed: {error:?}"
        );
        assert_eq!(
            git_output(
                dir.path(),
                &["for-each-ref", "--format=%(refname) %(objectname)"]
            ),
            refs
        );
        assert_eq!(git_output(dir.path(), &["rev-parse", "HEAD"]), head);
        assert_eq!(std::fs::read(dir.path().join(".git/index")).unwrap(), index);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("plain.txt")).unwrap(),
            "local\n"
        );
        assert!(git_output(
            dir.path(),
            &["for-each-ref", "--format=%(refname)", "refs/kagi/backups"]
        )
        .is_empty());
    }
}

#[test]
fn special_sync_ref_only_branch_does_not_refuse_sparse_worktree() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    git(dir.path(), &["checkout", "-q", "-b", "other"]);
    git(dir.path(), &["config", "core.sparseCheckout", "true"]);
    let mut backend = backend(dir.path());
    let index = std::fs::read(dir.path().join(".git/index")).unwrap();
    let plan = backend.plan(&op()).unwrap();
    assert!(plan.blockers.is_empty());
    backend.run(&op(), &plan).unwrap();
    assert_eq!(std::fs::read(dir.path().join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("plain.txt")).unwrap(),
        "local\n"
    );
}

#[test]
fn special_sync_skip_worktree_without_sparse_config_refuses() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    git(
        dir.path(),
        &["update-index", "--skip-worktree", "plain.txt"],
    );
    assert_refused(dir.path(), "non-cone sparse-checkout");
}
