//! Over-discard sentinels & coverage gaps for the discard pipeline (issue #303).
//!
//! `tests/discard_test.rs` proves discard restores the *target*, but before this
//! file NO test had a *bystander* dirty file — so an implementation that ignored
//! `paths` and stomped the whole repo (`checkout_index` over everything +
//! `git clean -fd`) would pass. These tests add the missing sentinels:
//!
//! - bystander dirty files (tracked incl. glob-metachar names, and untracked)
//!   must be byte-for-byte untouched when they are NOT selected (the acceptance
//!   criterion for the still-open P0 #282 over-discard);
//! - bulk discard-all over 150 eligible files is exact and lists every backup;
//! - assertion-quality upgrades the audit called out: empty-blob SHA for a
//!   deletion backup, and multi-file path→blob correspondence by blob CONTENT;
//! - the `NoUnstagedChanges` blocker, an absolute-path input, and an
//!   exec-bit-only change.
//!
//! All writes are confined to `TempDir` repositories — never user repos.

#[path = "../../../tests/support/backend_ops.rs"]
mod backend_ops;
use backend_ops::execute_discard;
use std::path::Path;

use git2::Repository;
use tempfile::TempDir;

use kagi_domain::plan_note::{DiscardNote, PlanNote};
use kagi_git::plan_discard;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_command, init_repo};

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

/// Byte-exact stdout (not trimmed): callers compare blob contents including
/// the trailing newline.
fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = git_command(dir)
        .args(args)
        .output()
        .expect("git command failed to start");
    assert!(out.status.success(), "git {} failed", args.join(" "));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Unlike the shared `write_file`, creates missing parent directories.
fn write_file(dir: &Path, name: &str, content: &str) {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(p, content).expect("write_file failed");
}

fn read_file(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).expect("read_file failed")
}

fn build_repo(tmp: &TempDir) -> std::path::PathBuf {
    let d = tmp.path();
    init_repo(d, "main");
    write_file(d, "tracked.txt", "committed\n");
    git(d, &["add", "tracked.txt"]);
    git(d, &["commit", "-qm", "initial commit"]);
    d.to_path_buf()
}

#[cfg(unix)]
#[test]
fn discard_backup_mode_matches_git_executable_policy() {
    if !crate::test_support::run_isolated() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    for (filemode, indexed_permissions, worktree_permissions, expected_mode) in [
        (true, 0o644, 0o654, 0o100644),
        (false, 0o644, 0o755, 0o100644),
        (false, 0o755, 0o644, 0o100755),
    ] {
        let tmp = TempDir::new().unwrap();
        let d = build_repo(&tmp);
        git(&d, &["config", "core.fileMode", "true"]);
        std::fs::set_permissions(
            d.join("tracked.txt"),
            std::fs::Permissions::from_mode(indexed_permissions),
        )
        .unwrap();
        git(&d, &["add", "tracked.txt"]);
        git(&d, &["commit", "--allow-empty", "-qm", "index mode"]);
        git(
            &d,
            &[
                "config",
                "core.fileMode",
                if filemode { "true" } else { "false" },
            ],
        );
        write_file(&d, "tracked.txt", "dirty content\n");
        std::fs::set_permissions(
            d.join("tracked.txt"),
            std::fs::Permissions::from_mode(worktree_permissions),
        )
        .unwrap();
        let repo = Repository::open(&d).unwrap();
        let index_before = std::fs::read(d.join(".git/index")).unwrap();
        let paths = vec!["tracked.txt".into()];
        let plan = plan_discard(&repo, &paths).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        let outcome = execute_discard(&repo, &plan, &paths).unwrap();
        assert!(!outcome.is_partial(), "{outcome:?}");
        let tree = repo
            .find_reference(&outcome.backups[0].reference)
            .unwrap()
            .peel_to_tree()
            .unwrap();
        let entry = tree.get_name("file").unwrap();
        assert_eq!(
            entry.filemode(),
            expected_mode,
            "fileMode={filemode}, permissions={worktree_permissions:o}"
        );
        assert_eq!(
            repo.find_blob(entry.id()).unwrap().content(),
            b"dirty content\n"
        );
        assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index_before);
    }
}

// #1125: a POSIX backslash is a filename byte, not a directory separator.
#[cfg(unix)]
fn discard_literal_backslash_fixture(bulk: bool) {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    for (path, content) in [
        (r"a\b.txt", "selected base\n"),
        ("a/b.txt", "neighbor base\n"),
    ] {
        write_file(&d, path, content);
    }
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "literal path baselines"]);
    write_file(&d, r"a\b.txt", "selected dirty\n");
    write_file(&d, "a/b.txt", "neighbor dirty\n");
    let index_before = std::fs::read(d.join(".git/index")).unwrap();
    let mut backend =
        kagi_git::Backend::open_with_policy(&d, kagi_git::backend::ExecutionPolicy::human(false))
            .unwrap();
    let mut paths = vec![r"a\b.txt".to_string()];
    if bulk {
        paths.push("a/b.txt".to_string());
    }
    let plan = backend.plan_discard(&paths).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let outcome = backend
        .run(
            &kagi_git::Operation::Discard {
                paths: paths.clone(),
            },
            &plan,
        )
        .unwrap();
    let kagi_git::OperationOutcome::Discard(outcome) = outcome else {
        panic!("unexpected discard outcome");
    };
    assert_eq!(
        read_file(&d, "a/b.txt"),
        if bulk {
            "neighbor base\n"
        } else {
            "neighbor dirty\n"
        },
        "unapproved neighboring file must remain byte-identical"
    );
    assert_eq!(read_file(&d, r"a\b.txt"), "selected base\n");
    assert!(!outcome.is_partial(), "{outcome:?}");
    assert_eq!(outcome.backups.len(), paths.len());
    assert_eq!(
        plan.preview_files
            .iter()
            .map(|f| f.path.to_str().unwrap())
            .collect::<Vec<_>>(),
        paths.iter().map(String::as_str).collect::<Vec<_>>()
    );
    for backup in &outcome.backups {
        let expected = match backup.path.as_str() {
            r"a\b.txt" => b"selected dirty\n".as_slice(),
            "a/b.txt" if bulk => b"neighbor dirty\n".as_slice(),
            other => panic!("backup for unapproved path: {other}"),
        };
        assert_eq!(backend.read_backup(&backup.reference).unwrap(), expected);
    }
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index_before);
}

#[cfg(unix)]
#[test]
fn discard_literal_backslash_single() {
    if !crate::test_support::run_isolated() {
        return;
    }
    discard_literal_backslash_fixture(false);
}

#[cfg(unix)]
#[test]
fn discard_literal_backslash_bulk() {
    if !crate::test_support::run_isolated() {
        return;
    }
    discard_literal_backslash_fixture(true);
}

#[test]
fn discard_unsafe_path_refuses_before_backup() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    write_file(&d, "tracked.txt", "keep dirty\n");
    let index_before = std::fs::read(d.join(".git/index")).unwrap();
    let repo = Repository::open(&d).unwrap();
    for unsafe_path in ["tracked.txt\0", "../tracked.txt", "nested/../tracked.txt"] {
        let paths = vec![unsafe_path.to_string()];
        let plan = plan_discard(&repo, &paths).unwrap();
        assert!(plan
            .blockers
            .iter()
            .any(|note| matches!(note, PlanNote::Discard(DiscardNote::UnsafePath { .. }))));
        assert!(execute_discard(&repo, &plan, &paths).is_err());
        // Even an artificially unblocked plan cannot bypass path preflight.
        let mut unblocked = plan;
        unblocked.blockers.clear();
        assert!(execute_discard(&repo, &unblocked, &paths).is_err());
        assert_eq!(read_file(&d, "tracked.txt"), "keep dirty\n");
        assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index_before);
        assert_eq!(
            repo.references_glob("refs/kagi/backups/*").unwrap().count(),
            0
        );
    }
}

#[test]
fn discard_uses_original_gitdir_with_separate_core_worktree() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    let workdir = tempfile::tempdir().unwrap();
    std::fs::rename(d.join("tracked.txt"), workdir.path().join("tracked.txt")).unwrap();
    git(
        &d,
        &["config", "core.worktree", workdir.path().to_str().unwrap()],
    );
    write_file(workdir.path(), "tracked.txt", "separate dirty\n");
    let index_before = std::fs::read(d.join(".git/index")).unwrap();
    let mut backend = kagi_git::Backend::open_with_policy(
        &d.join(".git"),
        kagi_git::backend::ExecutionPolicy::human(false),
    )
    .unwrap();
    let op = kagi_git::Operation::Discard {
        paths: vec!["tracked.txt".into()],
    };
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let kagi_git::OperationOutcome::Discard(outcome) = backend.run(&op, &plan).unwrap() else {
        panic!("unexpected outcome");
    };
    assert!(!outcome.is_partial(), "{outcome:?}");
    assert_eq!(read_file(workdir.path(), "tracked.txt"), "committed\n");
    assert_eq!(
        backend.read_backup(&outcome.backups[0].reference).unwrap(),
        b"separate dirty\n"
    );
    assert_eq!(std::fs::read(d.join(".git/index")).unwrap(), index_before);
}

// ════════════════════════════════════════════════════════════
// #303 PRIORITY 1 — over-discard sentinel (also the #282 P0 gate).
//
// ONE tracked target with glob metachars in its name; five bystander dirty
// files (tracked incl. one that a glob would match, plus untracked incl. an
// empty-dir-prune sibling). Discard ONLY the target; assert every bystander is
// byte-for-byte untouched and the target reverts.
// ════════════════════════════════════════════════════════════

#[test]
fn discard_does_not_touch_non_target_dirty_files() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);

    // Committed baselines for the tracked set.
    write_file(&d, "a*b.txt", "TARGET committed\n");
    write_file(&d, "aXXXb.txt", "victim committed\n");
    write_file(&d, "a[1].txt", "bracket committed\n");
    write_file(&d, "plain.txt", "plain committed\n");
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "add tracked bystanders"]);

    // Dirty EVERYTHING. Only `a*b.txt` will be discarded.
    write_file(&d, "a*b.txt", "TARGET dirty\n");
    write_file(&d, "aXXXb.txt", "EDIT-victim\n"); // would fall to a glob 'a*b.txt'
    write_file(&d, "a[1].txt", "bracket dirty\n");
    write_file(&d, "plain.txt", "plain dirty\n");
    write_file(&d, "untracked_bystander.txt", "untracked stays\n");
    write_file(&d, "emptydir_bystander/keep.txt", "in a fresh dir\n");

    let repo = Repository::open(&d).unwrap();
    let paths = vec!["a*b.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");
    assert!(!outcome.is_partial(), "outcome: {:?}", outcome);

    // Exactly one backup, for the target only.
    assert_eq!(outcome.backups.len(), 1, "only the target is backed up");
    assert_eq!(outcome.backups[0].path, "a*b.txt");

    // Target reverted to index/HEAD content.
    assert_eq!(read_file(&d, "a*b.txt"), "TARGET committed\n");

    // ── The sentinels: every bystander UNCHANGED. ──
    // A glob interpretation or whole-index checkout would revert this sentinel.
    assert_eq!(
        read_file(&d, "aXXXb.txt"),
        "EDIT-victim\n",
        "OVER-DISCARD: a glob-matched tracked bystander was reverted"
    );
    assert_eq!(read_file(&d, "a[1].txt"), "bracket dirty\n");
    assert_eq!(read_file(&d, "plain.txt"), "plain dirty\n");
    assert_eq!(
        read_file(&d, "untracked_bystander.txt"),
        "untracked stays\n"
    );
    assert!(
        d.join("emptydir_bystander/keep.txt").exists(),
        "untracked bystander in a fresh dir must survive"
    );
    assert_eq!(
        read_file(&d, "emptydir_bystander/keep.txt"),
        "in a fresh dir\n"
    );
}

// The untracked-target twin: discarding one untracked file must not delete
// sibling untracked/tracked bystanders (exercises the remove_file + empty-dir
// prune path rather than checkout_index).
#[test]
fn discard_untracked_target_does_not_delete_bystanders() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);

    // Tracked bystander, made dirty.
    write_file(&d, "plain.txt", "plain committed\n");
    git(&d, &["add", "plain.txt"]);
    git(&d, &["commit", "-qm", "add plain"]);
    write_file(&d, "plain.txt", "plain dirty\n");

    // Untracked target with glob metachars, plus untracked bystanders.
    write_file(&d, "u*x.txt", "TARGET untracked\n");
    write_file(&d, "uYYYx.txt", "victim untracked\n"); // glob 'u*x.txt' would match
    write_file(&d, "bystander_dir/keep.txt", "keep me\n");

    let repo = Repository::open(&d).unwrap();
    let paths = vec!["u*x.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");
    assert!(!outcome.is_partial(), "outcome: {:?}", outcome);

    // Target deleted (backed up), everyone else intact.
    assert!(!d.join("u*x.txt").exists(), "target untracked file deleted");
    assert_eq!(outcome.backups.len(), 1);
    assert_eq!(outcome.backups[0].path, "u*x.txt");

    assert!(
        d.join("uYYYx.txt").exists(),
        "OVER-DISCARD: a glob-matched untracked bystander was deleted"
    );
    assert_eq!(read_file(&d, "uYYYx.txt"), "victim untracked\n");
    assert!(d.join("bystander_dir/keep.txt").exists());
    assert_eq!(read_file(&d, "plain.txt"), "plain dirty\n");
}

// ════════════════════════════════════════════════════════════
// #303 PRIORITY 2 — bulk discard-all over 150 eligible files is exact.
//
// 200 committed files; 100 modified + 20 deleted (unstaged) + 30 untracked =
// 150 eligible; 80 left clean. Assert every eligible path is discarded, the
// clean 80 are untouched and absent from the backups, and the oplog summary
// lists all 150 (no silent truncation).
// ════════════════════════════════════════════════════════════

#[test]
fn discard_all_bulk_150_files_is_exact() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);

    // 200 committed files: d0..d9 / f0..f19.
    for dir in 0..10 {
        for f in 0..20 {
            write_file(
                &d,
                &format!("d{dir}/f{f}.txt"),
                &format!("orig {dir}-{f}\n"),
            );
        }
    }
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "200 files"]);

    let mut eligible: Vec<String> = Vec::new();
    let mut modified: Vec<String> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();

    // 100 modified (d0..d4 fully), 20 deleted (d5 f0..f19), 80 clean (d6..d9).
    for dir in 0..5 {
        for f in 0..20 {
            let p = format!("d{dir}/f{f}.txt");
            write_file(&d, &p, &format!("DIRTY {dir}-{f}\n"));
            modified.push(p.clone());
            eligible.push(p);
        }
    }
    for f in 0..20 {
        let p = format!("d5/f{f}.txt");
        std::fs::remove_file(d.join(&p)).unwrap();
        deleted.push(p.clone());
        eligible.push(p);
    }
    // 30 untracked in new/.
    for i in 0..30 {
        let p = format!("new/u{i}.txt");
        write_file(&d, &p, &format!("untracked {i}\n"));
        eligible.push(p);
    }
    assert_eq!(eligible.len(), 150);

    let repo = Repository::open(&d).unwrap();
    let plan = plan_discard(&repo, &eligible).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    // Warning must be the typed variant with the exact untracked count.
    assert!(
        plan.warnings.iter().any(|w| matches!(
            w,
            PlanNote::Discard(DiscardNote::UntrackedWillBeDeleted { count: 30 })
        )),
        "expected UntrackedWillBeDeleted{{count:30}}, got {:?}",
        plan.warnings
    );

    let outcome = execute_discard(&repo, &plan, &eligible).expect("execute");
    assert!(!outcome.is_partial(), "outcome: {:?}", outcome);

    // Backup set is exactly the eligible set, bijectively (no dupes, no extras).
    assert_eq!(outcome.backups.len(), 150);
    let backed: std::collections::HashSet<&str> =
        outcome.backups.iter().map(|b| b.path.as_str()).collect();
    assert_eq!(backed.len(), 150, "no duplicate backup paths");
    for p in &eligible {
        assert!(backed.contains(p.as_str()), "missing backup for {p}");
    }

    // 100 modified reverted, 20 deleted restored, 30 untracked gone + pruned.
    for p in &modified {
        let parts: Vec<&str> = p.trim_end_matches(".txt").split('/').collect();
        let dir = parts[0].trim_start_matches('d');
        let f = parts[1].trim_start_matches('f');
        assert_eq!(read_file(&d, p), format!("orig {dir}-{f}\n"), "revert {p}");
    }
    for p in &deleted {
        assert!(d.join(p).exists(), "restored {p}");
    }
    for i in 0..30 {
        assert!(
            !d.join(format!("new/u{i}.txt")).exists(),
            "deleted new/u{i}"
        );
    }
    assert!(!d.join("new").exists(), "empty new/ pruned");

    // 80 clean bystanders untouched AND absent from the backups.
    for dir in 6..10 {
        for f in 0..20 {
            let p = format!("d{dir}/f{f}.txt");
            assert_eq!(read_file(&d, &p), format!("orig {dir}-{f}\n"), "clean {p}");
            assert!(
                !backed.contains(p.as_str()),
                "clean bystander backed up: {p}"
            );
        }
    }

    // oplog summary lists ALL 150 pairs (no silent truncation).
    let summary = outcome.oplog_summary();
    assert!(summary.contains("discarded 150 file(s)"), "summary head");
    for p in &eligible {
        assert!(summary.contains(&format!("{p}=")), "summary missing {p}");
    }
}

// ════════════════════════════════════════════════════════════
// #303 assertion-quality upgrades
// ════════════════════════════════════════════════════════════

// Audit :139 — the deletion backup was only checked by count. Pin the empty-blob
// SHA so an unstaged deletion's recovery handle is the well-known empty blob.
#[test]
fn discard_unstaged_deletion_backup_is_the_empty_blob() {
    if !crate::test_support::run_isolated() {
        return;
    }
    const EMPTY_BLOB: &str = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";

    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    std::fs::remove_file(d.join("tracked.txt")).unwrap();

    let repo = Repository::open(&d).unwrap();
    let paths = vec!["tracked.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");

    assert_eq!(outcome.backups.len(), 1);
    assert_eq!(
        outcome.backups[0].blob, EMPTY_BLOB,
        "an absent file is backed up as the empty blob"
    );
    // And the empty blob really is in this repo's ODB.
    assert_eq!(git_out(&d, &["cat-file", "-p", EMPTY_BLOB]), "");
    assert_eq!(read_file(&d, "tracked.txt"), "committed\n", "restored");
}

// Audit :445-458 — multi-file backups were checked by count + substring only,
// so a path→blob swap would pass. Assert each blob's CONTENT is its own file's.
#[test]
fn discard_multi_file_blobs_correspond_by_content() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    write_file(&d, "alpha.txt", "alpha base\n");
    write_file(&d, "beta.txt", "beta base\n");
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "alpha beta"]);

    write_file(&d, "alpha.txt", "ALPHA dirty\n");
    write_file(&d, "beta.txt", "BETA dirty\n");

    let repo = Repository::open(&d).unwrap();
    let paths = vec!["alpha.txt".to_string(), "beta.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");
    assert_eq!(outcome.backups.len(), 2);

    for (path, want) in [("alpha.txt", "ALPHA dirty\n"), ("beta.txt", "BETA dirty\n")] {
        let b = outcome
            .backups
            .iter()
            .find(|b| b.path == path)
            .unwrap_or_else(|| panic!("backup for {path}"));
        assert_eq!(
            git_out(&d, &["cat-file", "-p", &b.blob]),
            want,
            "{path}'s backup blob must hold ITS pre-discard content, not a sibling's"
        );
    }
    assert_eq!(read_file(&d, "alpha.txt"), "alpha base\n");
    assert_eq!(read_file(&d, "beta.txt"), "beta base\n");
}

// ════════════════════════════════════════════════════════════
// #303 remaining gaps
// ════════════════════════════════════════════════════════════

// The `NoUnstagedChanges` blocker had zero integration coverage. A clean tracked
// file has nothing to discard → typed blocker, no working-tree change.
#[test]
fn discard_no_unstaged_changes_blocker() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    let repo = Repository::open(&d).unwrap();

    // tracked.txt is clean.
    let paths = vec!["tracked.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(
        plan.blockers.iter().any(|b| matches!(
            b,
            PlanNote::Discard(DiscardNote::NoUnstagedChanges { path }) if path == "tracked.txt"
        )),
        "expected NoUnstagedChanges for tracked.txt, got {:?}",
        plan.blockers
    );

    // Blocked plan is refused at execute; no change on disk.
    let err = execute_discard(&repo, &plan, &paths).expect_err("blocked plan refused");
    assert!(format!("{err:?}").contains("blocker"));
    assert_eq!(read_file(&d, "tracked.txt"), "committed\n");
}

// Absolute-path input exercises `discard_rel_path`'s canonicalize+strip branch
// end-to-end (the audit flagged it as dead code across the suite).
#[test]
fn discard_absolute_path_input_reverts_target() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    write_file(&d, "tracked.txt", "DIRTY\n");

    let repo = Repository::open(&d).unwrap();
    let paths = vec![d.join("tracked.txt").to_string_lossy().into_owned()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");
    assert_eq!(outcome.backups.len(), 1);
    assert_eq!(
        outcome.backups[0].path, "tracked.txt",
        "stripped to rel form"
    );
    assert_eq!(read_file(&d, "tracked.txt"), "committed\n");
}

// Exec-bit-only change: `chmod +x` with identical content. Discard must restore
// the committed mode (no exec bit). The audit flags that the backup blob == the
// index blob, so the oplog handle alone can't restore the mode — this test
// checks the ON-DISK mode after discard, which is what the user sees.
#[cfg(unix)]
#[test]
fn discard_exec_bit_only_restores_mode() {
    if !crate::test_support::run_isolated() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    write_file(&d, "s.sh", "#!/bin/sh\necho hi\n");
    git(&d, &["add", "s.sh"]);
    git(&d, &["commit", "-qm", "add s.sh (mode 644)"]);

    // Flip only the exec bit; content is byte-identical.
    let mut perm = std::fs::metadata(d.join("s.sh")).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(d.join("s.sh"), perm).unwrap();

    let repo = Repository::open(&d).unwrap();
    let paths = vec!["s.sh".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(
        plan.blockers.is_empty(),
        "exec-bit change should be discardable: {:?}",
        plan.blockers
    );
    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");
    assert!(!outcome.is_partial(), "outcome: {:?}", outcome);

    let mode = std::fs::metadata(d.join("s.sh"))
        .unwrap()
        .permissions()
        .mode()
        & 0o111;
    assert_eq!(
        mode, 0,
        "discard must strip the exec bit back to the committed mode"
    );
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
