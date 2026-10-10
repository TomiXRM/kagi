use std::io::{BufWriter, Write};
use std::process::Stdio;
#[path = "git_fixture.rs"]
mod git_fixture;

pub fn history() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git_fixture::init_repo(dir.path(), "main");
    let mut child = git_fixture::git_command(dir.path())
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = BufWriter::new(child.stdin.take().unwrap());
        for n in 1..=2500 {
            let message = format!("commit {n}");
            writeln!(input, "commit refs/heads/main\nmark :{n}\ncommitter Survey <survey@example.invalid> {} +0000\ndata {}\n{message}", 1_700_000_000 + n, message.len()).unwrap();
            if n > 1 {
                writeln!(input, "from :{}", n - 1).unwrap();
            }
            writeln!(input).unwrap();
        }
    }
    assert!(child.wait().unwrap().success());
    dir
}

pub fn git(path: &std::path::Path, args: &[&str]) -> String {
    git_fixture::git_output(path, args)
}

pub fn collision(commits: &[&str]) -> (String, Vec<String>) {
    let mut groups = std::collections::BTreeMap::<&str, Vec<String>>::new();
    for sha in commits {
        groups.entry(&sha[..4]).or_default().push(sha.to_string());
    }
    groups
        .into_iter()
        .find(|(_, shas)| shas.len() > 1)
        .map(|(prefix, shas)| (prefix.to_string(), shas))
        .expect("deterministic history contains a collision")
}
