#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/http_basic_401.rs"]
mod http_basic_401;
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

#[test]
fn fetch_http_401_retains_auth_detail_without_helper_failure() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git_fixture::init_repo(repo, "main");
    git_fixture::write_file(repo, "base.txt", "base\n");
    git_fixture::commit_all(repo, "base");
    let server = http_basic_401::HttpBasic401::start();
    git_fixture::git(repo, &["remote", "add", "origin", &server.url()]);
    git_fixture::git(repo, &["config", "credential.helper", ""]);
    git_fixture::git(repo, &["config", "branch.main.remote", "origin"]);
    git_fixture::git(repo, &["config", "branch.main.merge", "refs/heads/main"]);
    let backend = kagi_git::Backend::open(repo).unwrap();
    let before = git_fixture::git_output(repo, &["show-ref"]);
    for error in [
        backend.fetch_remote().unwrap_err(),
        backend.fetch_remote_branch("origin/main").unwrap_err(),
    ] {
        assert!(
            matches!(&error, kagi_git::GitError::FetchAuthentication { remote, .. } if remote == "origin"),
            "typed authentication cause: {error}"
        );
        let detail = error.to_string();
        assert!(
            !detail.contains("cannot exec"),
            "askpass helper failure must not pollute authentication detail: {detail}"
        );
        assert!(!detail.contains("/bin/false"), "{detail}");
        assert!(
            detail.contains("terminal prompts disabled"),
            "complete Git cause retained: {detail}"
        );
        assert_eq!(
            git_fixture::git_output(repo, &["show-ref"]),
            before,
            "failed fetch changes no refs"
        );
    }
}
