//! Integration tests for the commit checklist — W14-CHECK
//! (T-COMMIT-004 conflict marker / T-COMMIT-005 secret·.env / T-COMMIT-006 large binary)
//!
//! All write operations are confined to `TempDir` repositories created within
//! each test.  The checklist inspects **staged index BLOBs** (not the working
//! tree), so each test stages content via `git add` before asserting.
//!
//! The checklist is exercised both directly (`checklist(repo, status)`) and
//! end-to-end through `plan_commit`, since `plan_commit` appends the checklist
//! blockers/warnings to its plan.
//!
//! # Test inventory
//!
//! | # | Name | Rule |
//! |---|------|------|
//! |  1 | `marker_text_file_blocks` | 4 — conflict marker in staged text → blocker |
//! |  2 | `clean_text_no_block` | 4 — text without markers → no blocker |
//! |  3 | `binary_with_marker_bytes_skipped` | 4 — binary BLOB never scanned for markers |
//! |  4 | `marker_only_in_unstaged_not_flagged` | 4 — WT marker not staged → no blocker |
//! |  5 | `large_blob_prefix_only_scanned` | 4 — only first N bytes scanned (perf) |
//! |  6 | `env_dotfile_warns` | 5 — `.env` staged → warning |
//! |  7 | `env_example_excluded` | 5 — `.env.example` staged → no warning |
//! |  8 | `private_key_content_warns` | 5 — PRIVATE KEY header → warning |
//! |  9 | `ordinary_code_no_secret_warn` | 5 — normal code → no warning |
//! | 10 | `pem_key_name_warns` | 5 — `*.pem` file name → warning |
//! | 11 | `large_binary_warns` | 6 — binary over threshold → warning (size+name) |
//! | 12 | `small_binary_no_warn` | 6 — binary under threshold → no warning |
//! | 13 | `large_text_no_warn` | 6 — large text → no warning |
//! | 14 | `env_threshold_override` | 6 — `KAGI_LARGE_BLOB_BYTES` changes threshold |
//! | 15 | `plan_commit_surfaces_marker_blocker` | end-to-end via plan_commit |

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{checklist, plan_commit, working_tree_status};

// ────────────────────────────────────────────────────────────
// Env serialization (KAGI_LARGE_BLOB_BYTES is process-global)
// ────────────────────────────────────────────────────────────

/// Serialize all tests that read or set `KAGI_LARGE_BLOB_BYTES` so the env var
/// races (oplog_test.rs uses the same pattern for `KAGI_LOG_DIR`).
static ENV_LOCK: Mutex<()> = Mutex::new(());

const LARGE_BLOB_ENV: &str = "KAGI_LARGE_BLOB_BYTES";

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

/// Run a git command inside `dir`, asserting it succeeds.
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git command failed to start");
    assert!(
        status.success(),
        "git {} exited with {:?}",
        args.join(" "),
        status.code()
    );
}

/// Build a minimal repo with one commit on `main`; HEAD clean.
fn init_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository) {
    let d = tmp.path().to_path_buf();
    git(&d, &["init", "-q", "-b", "main", "."]);
    git(&d, &["config", "user.name", "Test"]);
    git(&d, &["config", "user.email", "test@example.com"]);
    git(&d, &["config", "commit.gpgsign", "false"]);
    std::fs::write(d.join("README.md"), "# test\n").unwrap();
    git(&d, &["add", "README.md"]);
    git(&d, &["commit", "-qm", "initial commit"]);
    let repo = Repository::open(&d).expect("open repo");
    (d, repo)
}

fn write(dir: &Path, name: &str, content: &[u8]) {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, content).unwrap();
}

/// Stage a file and return `(blockers, warnings)` from the checklist.
fn run_checklist(repo: &Repository) -> (Vec<String>, Vec<String>) {
    let status = working_tree_status(repo).expect("status");
    checklist(repo, &status).expect("checklist")
}

fn any_contains(v: &[String], needle: &str) -> bool {
    v.iter().any(|s| s.contains(needle))
}

// ────────────────────────────────────────────────────────────
// Rule 4 — conflict markers (block)
// ────────────────────────────────────────────────────────────

#[test]
fn marker_text_file_blocks() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    let body = b"fn main() {\n<<<<<<< HEAD\nlet a = 1;\n=======\nlet a = 2;\n>>>>>>> other\n}\n";
    write(&d, "conflict.rs", body);
    git(&d, &["add", "conflict.rs"]);

    let (blockers, _warnings) = run_checklist(&repo);
    assert!(
        any_contains(&blockers, "Conflict marker") && any_contains(&blockers, "conflict.rs"),
        "expected conflict-marker blocker, got {:?}",
        blockers
    );
}

#[test]
fn clean_text_no_block() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    write(&d, "clean.rs", b"fn main() {\n    let a = 1;\n}\n");
    git(&d, &["add", "clean.rs"]);

    let (blockers, _warnings) = run_checklist(&repo);
    assert!(
        !any_contains(&blockers, "Conflict marker"),
        "clean text must not block, got {:?}",
        blockers
    );
}

#[test]
fn binary_with_marker_bytes_skipped() {
    // A binary BLOB (NUL bytes) that *also* contains the marker byte sequence
    // must be skipped (binary files are not scanned for markers).
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(b"\x00\x01\x02binary\x00");
    body.extend_from_slice(b"<<<<<<< HEAD\n=======\n>>>>>>> x\n");
    write(&d, "blob.bin", &body);
    git(&d, &["add", "blob.bin"]);

    let (blockers, _warnings) = run_checklist(&repo);
    assert!(
        !any_contains(&blockers, "Conflict marker"),
        "binary blob must not be scanned for markers, got {:?}",
        blockers
    );
}

#[test]
fn marker_only_in_unstaged_not_flagged() {
    // Marker exists in the WT but the staged version is clean → no blocker.
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    write(&d, "f.rs", b"clean staged content\n");
    git(&d, &["add", "f.rs"]);
    // Now dirty the WT with markers, but do NOT re-stage.
    write(&d, "f.rs", b"<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> z\n");

    let (blockers, _warnings) = run_checklist(&repo);
    assert!(
        !any_contains(&blockers, "Conflict marker"),
        "unstaged marker must not block (checklist reads index), got {:?}",
        blockers
    );
}

#[test]
fn large_blob_prefix_only_scanned() {
    // A marker far beyond the 1 MiB scan window is NOT detected (prefix-only
    // scan). We place > 1 MiB of clean text, then a marker at the very end.
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    let mut body: Vec<u8> = Vec::with_capacity(2 * 1024 * 1024);
    // ~1.5 MiB of clean lines.
    while body.len() < 1_500_000 {
        body.extend_from_slice(b"this is a perfectly normal line of text\n");
    }
    body.extend_from_slice(b"<<<<<<< HEAD\n");
    write(&d, "big.txt", &body);
    git(&d, &["add", "big.txt"]);

    let (blockers, _warnings) = run_checklist(&repo);
    assert!(
        !any_contains(&blockers, "Conflict marker"),
        "marker past the scan window must not be detected, got {:?}",
        blockers
    );
}

// ────────────────────────────────────────────────────────────
// Rule 5 — secret / .env (warn)
// ────────────────────────────────────────────────────────────

#[test]
fn env_dotfile_warns() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    write(&d, ".env", b"API_KEY=plainvalue\n");
    git(&d, &["add", "-f", ".env"]);

    let (_blockers, warnings) = run_checklist(&repo);
    assert!(
        any_contains(&warnings, "secret") && any_contains(&warnings, ".env"),
        "expected .env secret warning, got {:?}",
        warnings
    );
}

#[test]
fn env_example_excluded() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    write(&d, ".env.example", b"API_KEY=changeme\n");
    git(&d, &["add", "-f", ".env.example"]);

    let (_blockers, warnings) = run_checklist(&repo);
    assert!(
        !any_contains(&warnings, ".env.example"),
        ".env.example must be excluded, got {:?}",
        warnings
    );
}

#[test]
fn private_key_content_warns() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    // Ordinary-looking name so only the *content* rule can fire.
    write(
        &d,
        "notes.txt",
        b"-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----\n",
    );
    git(&d, &["add", "notes.txt"]);

    let (_blockers, warnings) = run_checklist(&repo);
    assert!(
        any_contains(&warnings, "secret content") && any_contains(&warnings, "notes.txt"),
        "expected private-key content warning, got {:?}",
        warnings
    );
}

#[test]
fn ordinary_code_no_secret_warn() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    write(
        &d,
        "src/lib.rs",
        b"pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    );
    git(&d, &["add", "src/lib.rs"]);

    let (_blockers, warnings) = run_checklist(&repo);
    assert!(
        !any_contains(&warnings, "secret"),
        "ordinary code must not warn, got {:?}",
        warnings
    );
}

#[test]
fn pem_key_name_warns() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    write(
        &d,
        "certs/server.pem",
        b"not actually a key but named .pem\n",
    );
    git(&d, &["add", "certs/server.pem"]);

    let (_blockers, warnings) = run_checklist(&repo);
    assert!(
        any_contains(&warnings, "secret file") && any_contains(&warnings, "server.pem"),
        "expected *.pem name warning, got {:?}",
        warnings
    );
}

// ────────────────────────────────────────────────────────────
// Rule 6 — large binary (warn) — env-serialized
// ────────────────────────────────────────────────────────────

/// Helper: set `KAGI_LARGE_BLOB_BYTES`, run `f`, restore previous value.
fn with_threshold<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev = std::env::var(LARGE_BLOB_ENV).ok();
    match value {
        Some(v) => std::env::set_var(LARGE_BLOB_ENV, v),
        None => std::env::remove_var(LARGE_BLOB_ENV),
    }
    let out = f();
    match prev {
        Some(v) => std::env::set_var(LARGE_BLOB_ENV, v),
        None => std::env::remove_var(LARGE_BLOB_ENV),
    }
    out
}

/// Build a binary blob of `len` bytes (leading NUL guarantees binary status).
fn binary_blob(len: usize) -> Vec<u8> {
    let mut v = vec![0u8; len];
    // Vary bytes a little so it isn't all zeros (still binary via NUL probe).
    for (i, b) in v.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
    v[0] = 0; // ensure a NUL in the probe window
    v
}

#[test]
fn large_binary_warns() {
    with_threshold(Some("1024"), || {
        let tmp = TempDir::new().unwrap();
        let (d, repo) = init_repo(&tmp);
        write(&d, "asset.bin", &binary_blob(4096)); // > 1024 threshold
        git(&d, &["add", "asset.bin"]);

        let (_blockers, warnings) = run_checklist(&repo);
        assert!(
            any_contains(&warnings, "Large binary") && any_contains(&warnings, "asset.bin"),
            "expected large-binary warning, got {:?}",
            warnings
        );
    });
}

#[test]
fn small_binary_no_warn() {
    with_threshold(Some("1048576"), || {
        let tmp = TempDir::new().unwrap();
        let (d, repo) = init_repo(&tmp);
        write(&d, "small.bin", &binary_blob(2048)); // < 1 MiB threshold
        git(&d, &["add", "small.bin"]);

        let (_blockers, warnings) = run_checklist(&repo);
        assert!(
            !any_contains(&warnings, "Large binary"),
            "small binary must not warn, got {:?}",
            warnings
        );
    });
}

#[test]
fn large_text_no_warn() {
    with_threshold(Some("1024"), || {
        let tmp = TempDir::new().unwrap();
        let (d, repo) = init_repo(&tmp);
        // Large *text* (no NUL) well over the 1024 threshold.
        let mut body = Vec::new();
        while body.len() < 8192 {
            body.extend_from_slice(b"a line of legitimate text content\n");
        }
        write(&d, "big.md", &body);
        git(&d, &["add", "big.md"]);

        let (_blockers, warnings) = run_checklist(&repo);
        assert!(
            !any_contains(&warnings, "Large binary"),
            "large text must not warn, got {:?}",
            warnings
        );
    });
}

#[test]
fn env_threshold_override() {
    // Same binary: warns under a tiny threshold, silent under a huge one.
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    write(&d, "x.bin", &binary_blob(4096));
    git(&d, &["add", "x.bin"]);

    let warns_small = with_threshold(Some("1024"), || run_checklist(&repo).1);
    let warns_big = with_threshold(Some("10485760"), || run_checklist(&repo).1);

    assert!(
        any_contains(&warns_small, "Large binary"),
        "tiny threshold should warn, got {:?}",
        warns_small
    );
    assert!(
        !any_contains(&warns_big, "Large binary"),
        "huge threshold should not warn, got {:?}",
        warns_big
    );
}

// ────────────────────────────────────────────────────────────
// End-to-end: plan_commit surfaces checklist results
// ────────────────────────────────────────────────────────────

#[test]
fn plan_commit_surfaces_marker_blocker() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    write(
        &d,
        "merge.rs",
        b"x\n<<<<<<< HEAD\na\n=======\nb\n>>>>>>> y\n",
    );
    git(&d, &["add", "merge.rs"]);

    let plan = plan_commit(&repo, "fix: resolve").expect("plan_commit");
    assert!(
        any_contains(&plan.blockers, "Conflict marker"),
        "plan_commit must surface the marker blocker, got {:?}",
        plan.blockers
    );
}

#[test]
fn plan_commit_surfaces_secret_warning() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    write(&d, ".env", b"TOKEN=ghp_xxxxxxxxxxxxxxxxxxxx\n");
    git(&d, &["add", "-f", ".env"]);

    let plan = plan_commit(&repo, "chore: add config").expect("plan_commit");
    assert!(
        plan.warnings.iter().any(|w| w.contains(".env")),
        "plan_commit must surface the .env warning, got {:?}",
        plan.warnings
    );
}
