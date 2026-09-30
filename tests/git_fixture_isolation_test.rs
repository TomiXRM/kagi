//! #514: a fixture built through `support/git_fixture.rs` is the same whatever
//! Git setup the developer has.
//!
//! The same fixture is built in two child processes of this test binary: one
//! with an empty `HOME`, one whose `HOME`, XDG config and `GIT_*` environment
//! carry hostile global config, hooks and templates, and a `GIT_DIR` pointing
//! at a decoy repository. Both must describe identical repositories, and the
//! hostile `HOME` and the decoy must come out untouched.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

#[path = "support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, repo_with_bare_origin, write_file};

/// Set in a child: where to write the fixture description.
const PROBE_OUT: &str = "KAGI_FIXTURE_PROBE_OUT";
/// Set in the hostile child: the global `core.hooksPath` it must see.
const EXPECT_HOOKS: &str = "KAGI_FIXTURE_EXPECT_HOOKS";

/// Remote + work tree, two branches, files a hostile ignore/filter would touch.
fn build_and_describe() -> String {
    let f = repo_with_bare_origin("main");
    write_file(&f.local, "README.md", "# readme\n");
    write_file(&f.local, "notes.txt", "alpha\n");
    commit_all(&f.local, "second");
    git(&f.local, &["checkout", "-q", "-b", "topic"]);
    write_file(&f.local, "notes.txt", "alpha\nbeta\n");
    commit_all(&f.local, "topic work");
    git(&f.local, &["push", "-q", "origin", "topic"]);
    git(&f.local, &["tag", "v1"]);
    let description = format!(
        "== local\n{}== remote\n{}",
        describe(&f.local, &f.local.join(".git")),
        describe(&f.remote, &f.remote)
    );
    // The remote URL names the per-run temp directory.
    description.replace(f.local.parent().unwrap().to_str().unwrap(), "<fixture>")
}

/// Everything a hostile setup could change, minus timestamp-bound commit IDs.
fn describe(dir: &Path, git_dir: &Path) -> String {
    let mut out = String::new();
    for args in [
        &["rev-parse", "--show-object-format"][..],
        &["symbolic-ref", "HEAD"],
        &["for-each-ref", "--format=%(refname) %(objecttype)"],
        &[
            "log",
            "--all",
            "--topo-order",
            "--format=%T %an <%ae> %cn <%ce> %s%n%b",
        ],
        &["config", "--local", "--list"],
    ] {
        out += &format!("$ git {}\n{}\n", args.join(" "), git_output(dir, args));
    }
    if !git_dir.ends_with(".git") {
        return out;
    }
    out += &format!("ls-files\n{}\n", git_output(dir, &["ls-files", "-s"]));
    out += &format!(
        "status\n{}\n",
        git_output(dir, &["status", "--porcelain", "--ignored"])
    );
    let mut hooks: Vec<String> = std::fs::read_dir(git_dir.join("hooks"))
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| !name.ends_with(".sample"))
                .collect()
        })
        .unwrap_or_default();
    hooks.sort();
    out + &format!("active hooks {hooks:?}\n")
}

/// Every file under `root` with its bytes, to prove nothing was written there.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    files
}

fn write_script(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A `HOME` whose global config, XDG files, hooks and template would each
/// change the fixture if a helper let them through.
fn hostile_home(root: &Path) -> PathBuf {
    let home = root.join("home");
    let (hooks, template) = (home.join("hooks"), home.join("template"));
    std::fs::create_dir_all(home.join(".config/git")).unwrap();
    std::fs::write(
        home.join(".gitconfig"),
        format!(
            "[user]\n\tname = Hostile\n\temail = hostile@example.invalid\n\
             [init]\n\tdefaultBranch = hostile\n\ttemplateDir = {template}\n\
             [core]\n\thooksPath = {hooks}\n\texcludesFile = {home}/ignore\n\tautocrlf = true\n\
             [filter \"hostile\"]\n\tclean = sed s/alpha/HOSTILE/\n\tsmudge = cat\n",
            template = template.display(),
            hooks = hooks.display(),
            home = home.display(),
        ),
    )
    .unwrap();
    std::fs::write(home.join("ignore"), "*.md\n").unwrap();
    std::fs::write(
        home.join(".config/git/attributes"),
        "*.txt filter=hostile\n",
    )
    .unwrap();
    std::fs::write(home.join(".config/git/ignore"), "notes.txt\n").unwrap();
    std::fs::write(
        home.join(".config/git/config"),
        "[commit]\n\tgpgSign = true\n[gpg]\n\tprogram = false\n",
    )
    .unwrap();
    write_script(
        &hooks.join("pre-commit"),
        "echo hooked > hooked.txt && git add hooked.txt",
    );
    write_script(&hooks.join("commit-msg"), "echo HOOKED >> \"$1\"");
    write_script(
        &template.join("hooks/post-commit"),
        "echo template > template.txt",
    );
    std::fs::create_dir_all(template.join("info")).unwrap();
    std::fs::write(template.join("info/exclude"), "*.txt\n").unwrap();
    std::fs::write(
        template.join("config"),
        "[core]\n\tbare = false\n\tfileMode = false\n",
    )
    .unwrap();
    home
}

/// Run this test in a child with `GIT_*`/`HOME` replaced by `env`; return what
/// the child's fixture looks like.
fn describe_in_child(scratch: &Path, label: &str, env: &[(&str, &Path)]) -> String {
    let name = std::thread::current().name().unwrap().to_string();
    let out_file = scratch.join(format!("{label}.txt"));
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", &name, "--include-ignored"]);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    for (key, value) in env {
        cmd.env(key, value);
    }
    let output = cmd.env(PROBE_OUT, &out_file).output().unwrap();
    assert!(
        output.status.success(),
        "{label} child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read_to_string(out_file).unwrap()
}

/// The child half: confirm the hostile setup is live for plain `git`, then
/// build and describe the fixture.
fn child(out_file: &Path) {
    if let Some(hooks) = std::env::var_os(EXPECT_HOOKS) {
        let plain = Command::new("git")
            .args(["config", "--global", "core.hooksPath"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&plain.stdout).trim(),
            hooks.to_string_lossy(),
            "the hostile global config must be what plain git sees"
        );
    }
    std::fs::write(out_file, build_and_describe()).unwrap();
}

#[test]
fn fixture_is_identical_under_a_hostile_home() {
    if let Some(out_file) = std::env::var_os(PROBE_OUT) {
        child(Path::new(&out_file));
        return;
    }
    let scratch = TempDir::new().unwrap();
    let benign_home = scratch.path().join("benign");
    std::fs::create_dir(&benign_home).unwrap();
    let benign = describe_in_child(
        scratch.path(),
        "benign",
        &[
            ("HOME", &benign_home),
            ("XDG_CONFIG_HOME", &benign_home.join(".config")),
        ],
    );

    let home = hostile_home(scratch.path());
    let decoy = scratch.path().join("decoy");
    std::fs::create_dir(&decoy).unwrap();
    init_repo(&decoy, "main");
    write_file(&decoy, "decoy.txt", "decoy\n");
    commit_all(&decoy, "decoy");
    let decoy_before = tree(&decoy);
    let home_before = tree(&home);

    let hostile = describe_in_child(
        scratch.path(),
        "hostile",
        &[
            ("HOME", &home),
            ("XDG_CONFIG_HOME", &home.join(".config")),
            ("GIT_TEMPLATE_DIR", &home.join("template")),
            ("GIT_DIR", &decoy.join(".git")),
            ("GIT_WORK_TREE", &decoy),
            ("GIT_DEFAULT_HASH", Path::new("sha256")),
            ("GIT_CONFIG_COUNT", Path::new("1")),
            ("GIT_CONFIG_KEY_0", Path::new("user.name")),
            ("GIT_CONFIG_VALUE_0", Path::new("EnvHostile")),
            (EXPECT_HOOKS, &home.join("hooks")),
        ],
    );

    assert_eq!(hostile, benign, "the hostile setup changed the fixture");
    assert!(
        benign.contains("Test <test@example.com>") && benign.contains("notes.txt"),
        "the description must cover identity and content:\n{benign}"
    );
    assert_eq!(tree(&home), home_before, "the fixture wrote into HOME");
    assert_eq!(tree(&decoy), decoy_before, "the fixture wrote into GIT_DIR");
}

/// #819: the `GIT_DIR` untouched-check above flaked on CI because `git commit`
/// in the decoy spawned `git maintenance run --auto --detach`, whose
/// `.git/objects/maintenance.lock` was caught by the "before" snapshot and
/// gone by the "after" one. The window is too short to hit on demand, but
/// whether the fixture command *spawns* that process at all is deterministic
/// and is what `GIT_TRACE` reports.
#[test]
fn fixture_commands_spawn_no_detached_maintenance() {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path(), "main");
    write_file(tmp.path(), "a.txt", "a\n");
    git(tmp.path(), &["add", "-A"]);
    let out = git_fixture::git_command(tmp.path())
        .env("GIT_TRACE", "1")
        .args(["commit", "-qm", "one"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let trace = String::from_utf8_lossy(&out.stderr);
    assert!(
        trace.contains("built-in: git commit"),
        "GIT_TRACE must be live for this check to mean anything:\n{trace}"
    );
    assert!(
        !trace.contains("maintenance run") && !trace.contains("gc --auto"),
        "a fixture commit spawned background maintenance:\n{trace}"
    );
    assert!(
        !tmp.path().join(".git/objects/maintenance.lock").exists(),
        "a maintenance lock was left in the fixture"
    );
}
