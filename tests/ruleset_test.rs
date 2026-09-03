//! Integration tests for GitHub-ruleset local pre-verification (#346,
//! ADR-0150).
//!
//! These exercise the *plan* integration end-to-end against real tempdir
//! repos: the ruleset cache is seeded directly (`kagi_git::ruleset::seed_ruleset`)
//! so no network / `gh` is involved, and the commit / branch-create plans are
//! asserted to surface (or not surface) ruleset findings.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_domain::plan_note::PlanNote;
use kagi_domain::ruleset::{Pattern, PatternOp, Ruleset, RulesetStatus};
use kagi_git::ops::plan_create_branch;
use kagi_git::ruleset::seed_ruleset;
use kagi_git::{plan_commit, CommitId};

// ── helpers ──────────────────────────────────────────────────

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
        .expect("git failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn init_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository) {
    let d = tmp.path().to_path_buf();
    git(&d, &["init", "-q", "-b", "main", "."]);
    git(&d, &["config", "user.name", "Test"]);
    git(&d, &["config", "user.email", "dev@gmail.com"]);
    git(&d, &["config", "commit.gpgsign", "false"]);
    std::fs::write(d.join("README.md"), "# test\n").unwrap();
    git(&d, &["add", "README.md"]);
    git(&d, &["commit", "-qm", "initial commit"]);
    let repo = Repository::open(&d).expect("open repo");
    (d, repo)
}

/// The ruleset cache is keyed by the exact `repo.workdir()` path — seed with
/// the same value the plan will look up.
fn seed_active(repo: &Repository, branch: &str, rs: Ruleset) {
    let wd = repo.workdir().expect("workdir");
    seed_ruleset(wd, branch, RulesetStatus::Active(rs));
}

fn pat(op: PatternOp, s: &str) -> Pattern {
    Pattern {
        operator: op,
        pattern: s.into(),
        negate: false,
    }
}

fn note_texts(notes: &[PlanNote]) -> Vec<String> {
    notes.iter().map(|n| n.message_en()).collect()
}

fn has_ruleset_note(notes: &[PlanNote]) -> bool {
    notes.iter().any(|n| matches!(n, PlanNote::Ruleset(_)))
}

// ── commit message pattern ───────────────────────────────────

#[test]
fn plan_commit_surfaces_commit_message_pattern_violation() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    std::fs::write(d.join("a.txt"), "hi\n").unwrap();
    git(&d, &["add", "a.txt"]);

    seed_active(
        &repo,
        "main",
        Ruleset {
            commit_message: Some(pat(PatternOp::StartsWith, "JIRA-")),
            ..Ruleset::default()
        },
    );

    // Violating message → a ruleset warning is surfaced.
    let plan = plan_commit(&repo, "oops no prefix").expect("plan_commit");
    assert!(
        has_ruleset_note(&plan.warnings),
        "expected a ruleset warning, got {:?}",
        note_texts(&plan.warnings)
    );

    // Satisfying message → no ruleset note.
    let ok = plan_commit(&repo, "JIRA-42 do the thing").expect("plan_commit");
    assert!(
        !has_ruleset_note(&ok.warnings) && !has_ruleset_note(&ok.blockers),
        "satisfying message must not add a ruleset note, got {:?}",
        note_texts(&ok.warnings)
    );
}

// ── max file size at staging ─────────────────────────────────

#[test]
fn plan_commit_surfaces_max_file_size_warning() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    std::fs::write(d.join("big.bin"), vec![7u8; 4096]).unwrap();
    git(&d, &["add", "big.bin"]);

    seed_active(
        &repo,
        "main",
        Ruleset {
            max_file_size_bytes: Some(1024),
            ..Ruleset::default()
        },
    );

    let plan = plan_commit(&repo, "add asset").expect("plan_commit");
    assert!(
        plan.warnings
            .iter()
            .any(|n| n.message_en().contains("max file size")),
        "expected a max-file-size warning, got {:?}",
        note_texts(&plan.warnings)
    );
}

// ── branch name pattern in branch-create path ────────────────

#[test]
fn plan_create_branch_surfaces_branch_name_pattern_violation() {
    let tmp = TempDir::new().unwrap();
    let (_d, repo) = init_repo(&tmp);
    let head = repo.head().unwrap().target().unwrap().to_string();
    let at = CommitId(head);

    // Production only ever populates the cache for the *current* branch
    // (fetch_remote refreshes `main`); a prospective branch is never fetched.
    // Seed the key production writes so this fails if the lookup is keyed on
    // the new name again (#401).
    seed_active(
        &repo,
        "main",
        Ruleset {
            branch_name: Some(pat(PatternOp::StartsWith, "feature/")),
            ..Ruleset::default()
        },
    );

    let plan = plan_create_branch(&repo, "hotfix-1", &at).expect("plan_create_branch");
    assert!(
        has_ruleset_note(&plan.warnings),
        "expected a branch-name ruleset warning, got {:?}",
        note_texts(&plan.warnings)
    );
}

// ── empty response / conventional flow intact ────────────────

#[test]
fn empty_ruleset_is_unknown_not_unconstrained() {
    // The parse of an empty API response must be Unknown, never Active.
    let st = kagi_git::ruleset::parse_ruleset("[]");
    assert_eq!(st, RulesetStatus::Unknown);
    assert!(st.active().is_none());
}

#[test]
fn no_cached_ruleset_leaves_plan_unchanged() {
    // Simulates `gh` unauthenticated / feature disabled: nothing is cached, so
    // the commit plan must carry zero ruleset notes (conventional flow intact).
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    std::fs::write(d.join("a.txt"), "hi\n").unwrap();
    git(&d, &["add", "a.txt"]);

    // Explicitly seed Disabled to model gh-unauth.
    let wd = repo.workdir().unwrap().to_path_buf();
    seed_ruleset(&wd, "main", RulesetStatus::Disabled);

    let plan = plan_commit(&repo, "literally anything").expect("plan_commit");
    assert!(
        !has_ruleset_note(&plan.warnings) && !has_ruleset_note(&plan.blockers),
        "disabled ruleset must add no notes, got {:?}",
        note_texts(&plan.warnings)
    );
}

#[test]
fn unknown_ruleset_adds_no_findings_but_is_not_active() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = init_repo(&tmp);
    std::fs::write(d.join("a.txt"), "hi\n").unwrap();
    git(&d, &["add", "a.txt"]);

    let wd = repo.workdir().unwrap().to_path_buf();
    seed_ruleset(&wd, "main", RulesetStatus::Unknown);

    // Unknown contributes no plan findings (we don't spam every commit), but it
    // is never treated as an active/unconstrained ruleset.
    let cached = kagi_git::ruleset::ruleset_cached(&wd, "main").unwrap();
    assert_eq!(cached, RulesetStatus::Unknown);
    assert!(cached.active().is_none());

    let plan = plan_commit(&repo, "msg").expect("plan_commit");
    assert!(!has_ruleset_note(&plan.warnings) && !has_ruleset_note(&plan.blockers));
}
