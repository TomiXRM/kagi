use kagi::remote::stash::{
    remote_stash_drop_script_for_test, remote_stash_read_token_script_for_test,
    verify_known_hosts_snapshot_for_test,
};
use kagi_domain::remote::{parse_completion_token, parse_stash_frame, RemoteStashPhase};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn text(repo: &Path, args: &[&str]) -> String {
    String::from_utf8(git(repo, args))
        .unwrap()
        .trim()
        .to_string()
}

fn hash_stdin(repo: &Path, bytes: &[u8]) -> String {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start git hash-object");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

struct Fixture {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    runtime: PathBuf,
    fake_bin: PathBuf,
}

impl Fixture {
    fn new(stat_style: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        let fake_bin = temp.path().join("bin");
        fs::create_dir(&repo).unwrap();
        fs::create_dir(&runtime).unwrap();
        fs::create_dir(&fake_bin).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        git(&repo, &["init"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Kagi Test"]);
        fs::write(repo.join("tracked"), "base\n").unwrap();
        git(&repo, &["add", "tracked"]);
        git(&repo, &["commit", "-m", "base"]);
        for index in 0..3 {
            fs::write(repo.join("tracked"), format!("stash {index}\n")).unwrap();
            git(&repo, &["stash", "push", "-m", &format!("stash-{index}")]);
        }
        let stat = fake_bin.join("stat");
        fs::write(
            &stat,
            format!(
                "#!/bin/sh\nlast=\nfor arg in \"$@\"; do last=$arg; done\ncase $last in */0123456789abcdef0123456789abcdef) mode=600;; *) mode=700;; esac\ncase {stat_style}:$1 in\n  bsd:-f) printf '%s %s\\n' \"$(id -u)\" \"$mode\";;\n  gnu:-f) printf 'discard-this-output\\n'; exit 1;;\n  gnu:-c) printf '%s %s\\n' \"$(id -u)\" \"$mode\";;\n  *) exit 1;;\nesac\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&stat, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            _temp: temp,
            repo,
            runtime,
            fake_bin,
        }
    }

    fn state(&self) -> (String, String, String, String) {
        let head = text(&self.repo, &["rev-parse", "--verify", "HEAD"]);
        let oids = String::from_utf8(git(&self.repo, &["stash", "list", "--format=%H"]))
            .unwrap()
            .lines()
            .collect::<Vec<_>>()
            .join(",");
        let index_path = text(&self.repo, &["rev-parse", "--git-path", "index"]);
        let index = text(&self.repo, &["hash-object", &index_path]);
        let status = git(
            &self.repo,
            &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        );
        let worktree = hash_stdin(&self.repo, &status);
        (head, oids, index, worktree)
    }

    fn run_drop(&self, runtime: &Path) -> std::process::Output {
        let (head, oids, index, worktree) = self.state();
        let selected = oids.split(',').nth(1).unwrap().to_string();
        let physical_runtime = fs::canonicalize(runtime).unwrap();
        let common = fs::canonicalize(text(
            &self.repo,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        ))
        .unwrap();
        let original_path = std::env::var_os("PATH").unwrap();
        let mut paths = vec![self.fake_bin.clone()];
        paths.extend(std::env::split_paths(&original_path));
        let path = std::env::join_paths(paths).unwrap();
        Command::new("sh")
            .args(["-c", remote_stash_drop_script_for_test(), "kagi"])
            .args([
                self.repo.to_str().unwrap(),
                common.to_str().unwrap(),
                "41",
                "0123456789abcdef0123456789abcdef",
                "1",
                &selected,
                &head,
                &oids,
                &index,
                &worktree,
                &"1".repeat(64),
                physical_runtime.to_str().unwrap(),
            ])
            .env("LC_ALL", "ja_JP.UTF-8")
            .env("PATH", path)
            .env("XDG_RUNTIME_DIR", runtime)
            .output()
            .expect("run actual remote drop script locally")
    }

    fn read_token(&self) -> std::process::Output {
        let common = fs::canonicalize(text(
            &self.repo,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        ))
        .unwrap();
        let physical_runtime = fs::canonicalize(&self.runtime).unwrap();
        let token = physical_runtime.join("kagi/remote-ops/0123456789abcdef0123456789abcdef");
        let original_path = std::env::var_os("PATH").unwrap();
        let mut paths = vec![self.fake_bin.clone()];
        paths.extend(std::env::split_paths(&original_path));
        Command::new("sh")
            .args(["-c", remote_stash_read_token_script_for_test(), "kagi"])
            .args([
                self.repo.to_str().unwrap(),
                common.to_str().unwrap(),
                physical_runtime.to_str().unwrap(),
                token.to_str().unwrap(),
            ])
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .output()
            .expect("run actual token reader locally")
    }
}

#[test]
fn actual_drop_script_round_trips_for_bsd_and_gnu_stat() {
    for style in ["bsd", "gnu"] {
        let fixture = Fixture::new(style);
        let output = fixture.run_drop(&fixture.runtime);
        assert!(
            output.status.success(),
            "{style}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let frame = parse_stash_frame(&output.stdout).expect("script frame must match parser");
        assert_eq!(frame.phase, RemoteStashPhase::Complete);
        assert_eq!(frame.exit, 0);
        assert_eq!(frame.after.ordered_oids.len(), 2);
        let token_output = fixture.read_token();
        assert!(token_output.status.success());
        let token = parse_completion_token(&token_output.stdout)
            .expect("script token must match parser through guarded reader");
        assert_eq!(token.result, frame);
    }
}

#[test]
fn actual_drop_script_refuses_runtime_inside_repository_before_writing() {
    let fixture = Fixture::new("bsd");
    let unsafe_runtime = fixture.repo.join("runtime");
    fs::create_dir(&unsafe_runtime).unwrap();
    let output = fixture.run_drop(&unsafe_runtime);
    assert!(output.status.success());
    let frame = parse_stash_frame(&output.stdout).expect("refusal frame must match parser");
    assert_eq!(frame.phase, RemoteStashPhase::Preflight);
    assert!(!unsafe_runtime.join("kagi").exists());
    assert_eq!(fixture.state().1.split(',').count(), 3);
}

#[cfg(unix)]
#[test]
fn known_hosts_snapshot_is_revalidated_before_use() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let snapshot = temp.path().join("known_hosts");
    fs::write(&snapshot, b"host key\n").unwrap();
    fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o600)).unwrap();
    verify_known_hosts_snapshot_for_test(&snapshot, b"host key\n").unwrap();

    fs::write(&snapshot, b"changed\n").unwrap();
    assert!(verify_known_hosts_snapshot_for_test(&snapshot, b"host key\n").is_err());
    fs::write(&snapshot, b"host key\n").unwrap();
    fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(verify_known_hosts_snapshot_for_test(&snapshot, b"host key\n").is_err());
    fs::remove_file(&snapshot).unwrap();
    assert!(verify_known_hosts_snapshot_for_test(&snapshot, b"host key\n").is_err());
}

#[cfg(unix)]
#[test]
fn drop_forces_c_locale_inside_the_remote_shell() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new("bsd");
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real_git = String::from_utf8(real_git.stdout).unwrap();
    let wrapper = fixture.fake_bin.join("git");
    // Simulate a remote Git translation even on hosts without that locale installed.
    // The destructive command still runs, but non-C output has different punctuation.
    let script = format!(
        r#"#!/bin/sh
if test "$1 $2" = "stash drop"; then
    output=$('{}' "$@") || exit $?
    if test "$LC_ALL" = C; then printf '%s\n' "$output"; else printf '削除しました：%s\n' "$output" | tr '()' '[]'; fi
else
    exec '{}' "$@"
fi
"#,
        real_git.trim(),
        real_git.trim()
    );
    fs::write(&wrapper, script).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let output = fixture.run_drop(&fixture.runtime);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let frame = parse_stash_frame(&output.stdout).expect("C-locale completion must be verifiable");
    assert_eq!(frame.exit, 0);
    assert_eq!(frame.stdout_oid, frame.selected_oid);
    assert_eq!(frame.after.ordered_oids.len(), 2);
    assert_eq!(
        parse_completion_token(&fixture.read_token().stdout)
            .unwrap()
            .result,
        frame
    );
}
