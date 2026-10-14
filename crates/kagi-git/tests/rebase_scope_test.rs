#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod test_support;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};

#[test]
fn rebase_plan_discloses_three_commits_in_replay_order_without_writes() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    init_repo(path, "main");
    write_file(path, "base", "base");
    commit_all(path, "base");
    git(path, &["checkout", "-qb", "feature"]);
    let mut expected = Vec::new();
    for n in 1..=3 {
        write_file(path, &format!("feature-{n}"), "feature");
        commit_all(path, &format!("replay {n}"));
        let oid = git_output(path, &["rev-parse", "HEAD"]);
        expected.push(format!("{}  replay {n}", &oid[..8]));
    }
    git(path, &["checkout", "-q", "main"]);
    write_file(path, "main", "main");
    commit_all(path, "new base");
    git(path, &["checkout", "-q", "feature"]);
    let before = git_output(path, &["show-ref"]);
    let repo = git2::Repository::open(path).unwrap();
    let plan = kagi_git::ops::plan_rebase_current_onto(&repo, "main").unwrap();
    assert_eq!(
        plan.preview_commits, expected,
        "rebase scope must disclose all three replay candidates in oldest-first order"
    );
    assert_eq!(git_output(path, &["show-ref"]), before);
    assert!(git_output(path, &["status", "--porcelain"]).is_empty());
}
