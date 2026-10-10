//! Exact local branch subsection migration (#1129).
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod test_support;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};
use kagi_git::{backend::ExecutionPolicy, Backend, Operation};
use std::path::Path;

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    write_file(dir.path(), "base", "base\n");
    commit_all(dir.path(), "base");
    git(dir.path(), &["branch", "foo"]);
    git(dir.path(), &["branch", "foo.bar"]);
    dir
}
fn rename(path: &Path) {
    let mut backend = Backend::open_with_policy(path, ExecutionPolicy::human(false)).unwrap();
    let op = Operation::RenameBranch {
        old_name: "foo".into(),
        new_name: "renamed".into(),
    };
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let keys = plan
        .warnings
        .iter()
        .find_map(|note| match note {
            kagi_domain::plan_note::PlanNote::Branch(
                kagi_domain::plan_note::BranchNote::RenameConfig { keys, .. },
            ) => Some(keys),
            _ => None,
        })
        .expect("plan lists config keys even when the section is absent");
    let list = git_output(
        path,
        &["config", "--null", "--local", "--includes", "--list"],
    );
    let expected: std::collections::BTreeSet<_> = list
        .split('\0')
        .filter_map(|entry| {
            let key = entry.split_once('\n').map_or(entry, |(key, _)| key);
            let (section, tail) = key.split_once('.')?;
            let (branch, _) = tail.rsplit_once('.')?;
            (section == "branch" && branch == "foo").then_some(key.to_owned())
        })
        .collect();
    assert_eq!(
        keys.iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    backend.run(&op, &plan).unwrap();
    let log = std::fs::read_to_string(
        Path::new(&std::env::var("KAGI_LOG_DIR").unwrap()).join("operations.jsonl"),
    )
    .unwrap();
    assert_eq!(log.lines().count(), 1, "exactly one receipt");
}
fn without_moved(list: &str, branch: &str) -> String {
    list.split('\0')
        .filter(|entry| {
            let key = entry.split_once('\n').map_or(*entry, |(key, _)| key);
            key.split_once('.').and_then(|(section, tail)| {
                tail.rsplit_once('.')
                    .map(|(subsection, _)| (section, subsection))
            }) != Some(("branch", branch))
        })
        .collect::<Vec<_>>()
        .join("\0")
}
#[test]
fn rename_config_preserves_multivalue_order() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    git(
        dir.path(),
        &["config", "--add", "branch.foo.merge", "refs/heads/one"],
    );
    git(
        dir.path(),
        &["config", "--add", "branch.foo.merge", "refs/heads/two"],
    );
    rename(dir.path());
    assert_eq!(
        git_output(
            dir.path(),
            &["config", "--local", "--get-all", "branch.renamed.merge"]
        ),
        "refs/heads/one\nrefs/heads/two"
    );
}
#[test]
fn rename_config_preserves_siblings_and_unrelated_sections() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    git(dir.path(), &["config", "branch.foo.remote", "origin"]);
    git(
        dir.path(),
        &["config", "branch.foo.bar.remote", "canonical"],
    );
    git(
        dir.path(),
        &[
            "config",
            "--add",
            "branch.foo.bar.merge",
            "refs/heads/sibling",
        ],
    );
    git(
        dir.path(),
        &[
            "config",
            "other.foo.description",
            "unrelated \"quotes\" \\ escape",
        ],
    );
    let before = git_output(dir.path(), &["config", "--null", "--local", "--list"]);
    rename(dir.path());
    let after = git_output(dir.path(), &["config", "--null", "--local", "--list"]);
    assert_eq!(
        without_moved(&before, "foo"),
        without_moved(&after, "renamed")
    );
}
#[test]
fn rename_config_preserves_verbatim_values_and_local_includes() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let include = dir.path().join("branch-settings");
    let text = "[branch \"foo\"]\n\tpushRemote = fork\n\trebase = merges\n\tdescription = \"spaces \\\"quotes\\\" and \\\\ escape\"\n\tmerge = refs/heads/one\n\tmerge = refs/heads/two\n[branch \"foo.bar\"]\n\tremote = canonical\n";
    std::fs::write(&include, text).unwrap();
    git(
        dir.path(),
        &["config", "include.path", include.to_str().unwrap()],
    );
    rename(dir.path());
    assert_eq!(
        std::fs::read_to_string(include).unwrap(),
        text.replacen("[branch \"foo\"]", "[branch \"renamed\"]", 1)
    );
    assert_eq!(
        git_output(
            dir.path(),
            &[
                "config",
                "--local",
                "--includes",
                "--get",
                "branch.renamed.description"
            ]
        ),
        "spaces \"quotes\" and \\ escape"
    );
}
#[test]
fn rename_config_without_section_leaves_config_unchanged() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let before = std::fs::read(dir.path().join(".git/config")).unwrap();
    rename(dir.path());
    assert_eq!(
        before,
        std::fs::read(dir.path().join(".git/config")).unwrap()
    );
}
#[test]
fn rename_config_drift_refuses_before_mutation() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    git(dir.path(), &["config", "branch.foo.pushRemote", "approved"]);
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let op = Operation::RenameBranch {
        old_name: "foo".into(),
        new_name: "renamed".into(),
    };
    let plan = backend.plan(&op).unwrap();
    git(
        dir.path(),
        &["config", "branch.foo.pushRemote", "unapproved"],
    );
    let before = std::fs::read(dir.path().join(".git/config")).unwrap();
    let error = backend.run(&op, &plan).unwrap_err();
    assert!(error.is_preflight(), "{error:?}");
    assert_eq!(
        before,
        std::fs::read(dir.path().join(".git/config")).unwrap()
    );
    assert_eq!(git_output(dir.path(), &["branch", "--list", "foo"]), "foo");
    assert!(git_output(dir.path(), &["branch", "--list", "renamed"]).is_empty());
    let log = std::fs::read_to_string(
        Path::new(&std::env::var("KAGI_LOG_DIR").unwrap()).join("operations.jsonl"),
    )
    .unwrap();
    assert_eq!(log.lines().count(), 1);
}

#[test]
fn rename_config_preserves_global_config_and_existing_destination() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let global = dir.path().join("global-config");
    let text = "[branch \"foo\"]\n\tmerge = refs/heads/global\n\tdescription = global only\n";
    std::fs::write(&global, text).unwrap();
    // This test owns a child process, so no parallel test observes this env.
    std::env::set_var("GIT_CONFIG_GLOBAL", &global);
    git(
        dir.path(),
        &["config", "--add", "branch.foo.merge", "refs/heads/local"],
    );
    git(
        dir.path(),
        &[
            "config",
            "branch.renamed.description",
            "existing destination",
        ],
    );
    rename(dir.path());
    assert_eq!(std::fs::read_to_string(global).unwrap(), text);
    assert_eq!(
        git_output(
            dir.path(),
            &["config", "--local", "--get-all", "branch.renamed.merge"]
        ),
        "refs/heads/local"
    );
    assert_eq!(
        git_output(
            dir.path(),
            &["config", "--local", "--get", "branch.renamed.description"]
        ),
        "existing destination"
    );
}

#[test]
fn rename_config_from_linked_worktree_updates_all_heads() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let worktree = dir.path().join("linked");
    git(
        dir.path(),
        &["worktree", "add", "-q", worktree.to_str().unwrap(), "foo"],
    );
    git(
        dir.path(),
        &["config", "--add", "branch.foo.merge", "refs/heads/one"],
    );
    git(
        dir.path(),
        &["config", "--add", "branch.foo.merge", "refs/heads/two"],
    );
    rename(&worktree);
    assert_eq!(
        git_output(&worktree, &["symbolic-ref", "--short", "HEAD"]),
        "renamed"
    );
    assert_eq!(
        git_output(dir.path(), &["symbolic-ref", "--short", "HEAD"]),
        "main"
    );
    assert_eq!(
        git_output(
            dir.path(),
            &["config", "--local", "--get-all", "branch.renamed.merge"]
        ),
        "refs/heads/one\nrefs/heads/two"
    );
}

#[test]
fn rename_config_failure_is_partial_with_one_receipt() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    git(
        dir.path(),
        &["config", "branch.foo.description", "retained"],
    );
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let op = Operation::RenameBranch {
        old_name: "foo".into(),
        new_name: "renamed".into(),
    };
    let plan = backend.plan(&op).unwrap();
    std::fs::write(dir.path().join(".git/config.lock"), "held").unwrap();
    assert!(backend.run(&op, &plan).is_err());
    assert_eq!(
        git_output(
            dir.path(),
            &["config", "--local", "--get", "branch.foo.description"]
        ),
        "retained"
    );
    assert!(git_output(dir.path(), &["branch", "--list", "foo"]).is_empty());
    assert_eq!(
        git_output(dir.path(), &["branch", "--list", "renamed"]),
        "renamed"
    );
    let log = std::fs::read_to_string(
        Path::new(&std::env::var("KAGI_LOG_DIR").unwrap()).join("operations.jsonl"),
    )
    .unwrap();
    assert_eq!(log.lines().count(), 1);
    let receipt: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    assert_eq!(receipt["outcome"]["kind"], "Partial", "{receipt}");
}

#[test]
fn rename_config_refuses_include_destination_and_reordered_values() {
    if !test_support::run_isolated() {
        return;
    }
    for (case, key) in [
        ("include", "branch.foo.description"),
        ("destination", "branch.renamed.rebase"),
        ("ordered", "branch.foo.merge"),
        ("rebase", "branch.foo.rebase"),
    ] {
        let dir = fixture();
        let include = dir.path().join("settings");
        std::fs::write(&include, "[branch \"foo\"]\n\tdescription = approved\n").unwrap();
        git(
            dir.path(),
            &["config", "include.path", include.to_str().unwrap()],
        );
        git(
            dir.path(),
            &["config", "--add", "branch.foo.merge", "refs/heads/one"],
        );
        git(
            dir.path(),
            &["config", "--add", "branch.foo.merge", "refs/heads/two"],
        );
        let mut backend =
            Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
        let op = Operation::RenameBranch {
            old_name: "foo".into(),
            new_name: "renamed".into(),
        };
        let plan = backend.plan(&op).unwrap();
        match case {
            "include" => {
                std::fs::write(&include, "[branch \"foo\"]\n\tdescription = unapproved\n").unwrap()
            }
            "ordered" => {
                git(dir.path(), &["config", "--unset-all", key]);
                git(dir.path(), &["config", "--add", key, "refs/heads/two"]);
                git(dir.path(), &["config", "--add", key, "refs/heads/one"]);
            }
            _ => git(dir.path(), &["config", key, "true"]),
        }
        let config_before = std::fs::read(dir.path().join(".git/config")).unwrap();
        let include_before = std::fs::read(&include).unwrap();
        let error = backend.run(&op, &plan).unwrap_err();
        assert!(error.is_preflight(), "{case}: {error:?}");
        assert_eq!(
            std::fs::read(dir.path().join(".git/config")).unwrap(),
            config_before
        );
        assert_eq!(std::fs::read(&include).unwrap(), include_before);
        assert_eq!(git_output(dir.path(), &["branch", "--list", "foo"]), "foo");
        assert!(git_output(dir.path(), &["branch", "--list", "renamed"]).is_empty());
    }
    let log = std::fs::read_to_string(
        Path::new(&std::env::var("KAGI_LOG_DIR").unwrap()).join("operations.jsonl"),
    )
    .unwrap();
    assert_eq!(log.lines().count(), 4, "one receipt per refused attempt");
}
