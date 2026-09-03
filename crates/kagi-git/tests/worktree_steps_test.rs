//! Acceptance tests for issue #341 (ADR-0161): typed worktree steps + trust.
//!
//! Mirrors §6 exactly:
//! - copy/symlink run WITHOUT trust
//! - command is NOT run untrusted
//! - a config SHA change re-prompts (trust no longer matches)
//! - headless NEVER runs a command
//! - control bytes are neutralized in the prompt
//! - a pre_remove failure keeps the worktree
//! - symlink never overwrites
//! - the plan lists steps per type

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use kagi_domain::plan_note::PlanNote;
use kagi_domain::worktree_steps::{escape_control_bytes, WorktreeStep};
use kagi_git::ops::{
    is_worktree_config_trusted, load_worktree_config, post_create_note, pre_remove_note,
    run_post_create, trust_worktree_config, StepEnv,
};

// The trust store (`KAGI_LOG_DIR`) and the headless markers are process-global,
// so every env-touching test serializes on this and saves/restores what it set.
static ENV_LOCK: Mutex<()> = Mutex::new(());

const HEADLESS_MARKERS: &[&str] = &[
    "KAGI_OPEN_REPO",
    "KAGI_MENU_DUMP",
    "KAGI_SELECT_FIRST",
    "KAGI_NO_SINGLE_INSTANCE",
];

fn clear_headless() {
    for k in HEADLESS_MARKERS {
        std::env::remove_var(k);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .expect("git");
    assert!(st.success(), "git {:?} failed", args);
}

/// Write `.kagi/worktree.toml` under `root` with the given body.
fn write_config(root: &Path, body: &str) {
    let dir = root.join(".kagi");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("worktree.toml"), body).unwrap();
}

// ── copy / symlink run WITHOUT trust; symlink never overwrites ──

#[test]
fn copy_and_symlink_run_without_trust_and_never_overwrite() {
    let main = tempfile::tempdir().unwrap();
    let wt = tempfile::tempdir().unwrap();
    std::fs::write(main.path().join(".env.example"), "SRC").unwrap();
    std::fs::create_dir_all(main.path().join(".claude")).unwrap();
    std::fs::write(main.path().join(".claude/x"), "linked").unwrap();
    // A pre-existing symlink destination that must NOT be overwritten.
    std::fs::write(wt.path().join(".claude"), "PREEXISTING").unwrap();

    let steps = vec![
        WorktreeStep::Copy {
            from: ".env.example".into(),
            to: ".env".into(),
        },
        WorktreeStep::Symlink {
            from: ".claude".into(),
            to: ".claude".into(),
        },
    ];
    let env = StepEnv {
        main_root: main.path().to_path_buf(),
        worktree: wt.path().to_path_buf(),
    };
    // trusted = false — copy/symlink must still run.
    let report = run_post_create(&steps, &env, false);

    // Copy ran without trust.
    assert_eq!(
        std::fs::read_to_string(wt.path().join(".env")).unwrap(),
        "SRC",
        "copy must run without trust"
    );
    // Symlink refused to overwrite the pre-existing file (still the original).
    assert_eq!(
        std::fs::read_to_string(wt.path().join(".claude")).unwrap(),
        "PREEXISTING",
        "symlink must never overwrite an existing destination"
    );
    assert!(
        report.iter().any(|l| l.contains("copy ok")),
        "report: {report:?}"
    );
    // The never-overwrite guard must be what fired (its exact wording), not an
    // incidental OS EEXIST — this pins the guard so its removal is detectable.
    assert!(
        report.iter().any(|l| l.contains("never overwritten")),
        "the never-overwrite guard must fire: {report:?}"
    );
}

// ── command: untrusted → not run; trusted → run; SHA change → re-prompt;
//    headless → not run even when trusted ──

#[test]
fn command_trust_sha_and_headless_gating() {
    let _guard = ENV_LOCK.lock().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", store.path());
    clear_headless();

    let main = tempfile::tempdir().unwrap();
    let wt = tempfile::tempdir().unwrap();
    // A command whose side effect (a file) proves whether it ran.
    write_config(
        main.path(),
        "[[post_create]]\ntype = \"command\"\nrun = \"touch ran.marker\"\n",
    );
    let env = StepEnv {
        main_root: main.path().to_path_buf(),
        worktree: wt.path().to_path_buf(),
    };
    let marker = wt.path().join("ran.marker");

    // 1. Untrusted → command NOT run.
    let cfg = load_worktree_config(main.path()).unwrap().unwrap();
    assert!(
        !is_worktree_config_trusted(&cfg),
        "fresh config is untrusted"
    );
    let report = run_post_create(&cfg.steps.post_create, &env, false);
    assert!(
        !marker.exists(),
        "untrusted command must not run: {report:?}"
    );
    assert!(report.iter().any(|l| l.contains("skipped (untrusted)")));

    // 2. Trust it, then it runs.
    trust_worktree_config(&cfg).unwrap();
    assert!(is_worktree_config_trusted(&cfg));
    let report = run_post_create(&cfg.steps.post_create, &env, true);
    assert!(marker.exists(), "trusted command must run: {report:?}");
    std::fs::remove_file(&marker).unwrap();

    // 3. Change the config content → new SHA → trust no longer matches (re-prompt).
    write_config(
        main.path(),
        "[[post_create]]\ntype = \"command\"\nrun = \"touch ran.marker\"\n# changed\n",
    );
    let cfg2 = load_worktree_config(main.path()).unwrap().unwrap();
    assert_ne!(cfg.sha256, cfg2.sha256, "content change must move the SHA");
    assert!(
        !is_worktree_config_trusted(&cfg2),
        "a content change must force a re-confirm"
    );

    // 4. Headless → command NOT run even when trusted.
    trust_worktree_config(&cfg2).unwrap();
    assert!(is_worktree_config_trusted(&cfg2));
    std::env::set_var("KAGI_OPEN_REPO", "1");
    let report = run_post_create(&cfg2.steps.post_create, &env, true);
    assert!(
        !marker.exists(),
        "command must never run under the headless harness: {report:?}"
    );
    assert!(report.iter().any(|l| l.contains("skipped (headless)")));

    clear_headless();
    std::env::remove_var("KAGI_LOG_DIR");
}

// ── pre_remove failure keeps the worktree ──

#[test]
fn pre_remove_failure_keeps_the_worktree() {
    let _guard = ENV_LOCK.lock().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", store.path());
    clear_headless();

    let td = tempfile::tempdir().unwrap();
    let main = td.path().join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "master"]);
    std::fs::write(main.join("a.txt"), "a\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "base"]);

    let wt = td.path().join("wt");
    git(
        &main,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "feat"],
    );
    // A pre_remove command that FAILS (`false` exits 1). Committed so the
    // worktree stays clean (an untracked file would block removal on its own).
    write_config(&wt, "[[pre_remove]]\ntype = \"command\"\nrun = \"false\"\n");
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "-qm", "config"]);

    let backend = kagi_git::Backend::open(&main).expect("open main");
    let plan = backend.plan_remove_worktree("wt", false).expect("plan");

    // Untrusted → removal aborts, worktree survives.
    let err = backend
        .execute_remove_worktree(&plan, "wt", false)
        .expect_err("untrusted pre_remove command must abort the removal");
    assert!(format!("{err:?}").to_lowercase().contains("trust"));
    assert!(wt.exists(), "worktree must survive an aborted removal");

    // Trust it → the command now runs, fails (exit 1), still aborts.
    let cfg = load_worktree_config(&wt).unwrap().unwrap();
    trust_worktree_config(&cfg).unwrap();
    let err = backend
        .execute_remove_worktree(&plan, "wt", false)
        .expect_err("a failing pre_remove command must abort the removal");
    assert!(format!("{err:?}").contains("exited with status"));
    assert!(
        wt.exists(),
        "worktree must survive when the pre_remove command fails"
    );

    std::env::remove_var("KAGI_LOG_DIR");
}

// ── plan lists steps per type + control bytes neutralized ──

#[test]
fn plan_lists_steps_per_type_and_neutralizes_control_bytes() {
    let _guard = ENV_LOCK.lock().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", store.path());

    let main = tempfile::tempdir().unwrap();
    write_config(
        main.path(),
        "[[post_create]]\ntype = \"copy\"\nfrom = \".env.example\"\nto = \".env\"\n\
         [[post_create]]\ntype = \"symlink\"\nfrom = \".claude\"\nto = \".claude\"\n\
         [[post_create]]\ntype = \"command\"\nrun = \"npm ci\"\n",
    );
    let cfg = load_worktree_config(main.path()).unwrap().unwrap();
    let note = post_create_note(&cfg).expect("a post_create note");
    let PlanNote::Worktree(wn) = &note else {
        panic!("expected a worktree note");
    };
    let msg = wn.message_en();
    // Each type is listed by name (that is the point of typed steps).
    assert!(msg.contains("copy: .env.example → .env"), "{msg}");
    assert!(msg.contains("symlink: .claude → .claude"), "{msg}");
    assert!(msg.contains("command (needs trust): npm ci"), "{msg}");
    // A command in an untrusted config marks the note trust-required.
    assert!(msg.contains("TRUSTS this config"), "{msg}");

    // Control bytes in a committed config are escaped in the enumeration.
    write_config(
        main.path(),
        "[[pre_remove]]\ntype = \"command\"\nrun = \"a\\tb\"\n",
    );
    let cfg = load_worktree_config(main.path()).unwrap().unwrap();
    let note = pre_remove_note(&cfg).expect("a pre_remove note");
    let PlanNote::Worktree(wn) = &note else {
        panic!("note");
    };
    let msg = wn.message_en();
    assert!(msg.contains("a\\x09b"), "tab must be escaped: {msg}");
    assert!(!msg.contains("a\tb"), "raw control byte leaked: {msg:?}");
    assert_eq!(escape_control_bytes("a\tb"), "a\\x09b");

    std::env::remove_var("KAGI_LOG_DIR");
}
