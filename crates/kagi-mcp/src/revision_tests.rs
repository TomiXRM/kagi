use super::tests::call;
use super::Server;
use serde_json::json;
#[path = "../../../tests/support/revision_fixture.rs"]
mod fixture;

#[test]
fn commit_show_ambiguous_prefix() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let dir = fixture::history();
    let history = fixture::git(dir.path(), &["rev-list", "--max-count=2000", "HEAD"]);
    let (prefix, candidates) = fixture::collision(&history.lines().collect::<Vec<_>>());
    let mut server = Server::new(dir.path());
    let result = call(&mut server, "kagi_commit_show", json!({"revision": prefix}));
    assert_eq!(result["isError"], true, "{result}");
    let error = result["structuredContent"]["error"].as_str().unwrap();
    assert!(
        error.contains("longer prefix") && error.contains("full SHA"),
        "{error}"
    );
    for candidate in candidates {
        assert!(error.contains(&candidate), "{error}");
    }
}

#[test]
fn commit_show_direct_revision_lookup() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let dir = fixture::history();
    let root = fixture::git(dir.path(), &["rev-list", "--max-parents=0", "HEAD"]);
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
            &root,
        ],
    );
    let blob = fixture::git(dir.path(), &["hash-object", "-w", "--stdin"]);
    let mut server = Server::new(dir.path());
    for revision in [&root[..], &root[..12], "root-tag"] {
        let result = call(
            &mut server,
            "kagi_commit_show",
            json!({"revision": revision}),
        );
        assert_eq!(result["isError"], false, "{result}");
        assert_eq!(result["structuredContent"]["sha"], root);
    }
    for revision in ["ffffffffffffffffffffffffffffffffffffffff", "", "a", &blob] {
        let result = call(
            &mut server,
            "kagi_commit_show",
            json!({"revision": revision}),
        );
        assert_eq!(result["isError"], true, "{result}");
        if revision == blob {
            assert!(result["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains("not a commit"));
        }
    }
}
