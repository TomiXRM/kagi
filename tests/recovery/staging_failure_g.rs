use super::*;
use git2::{Repository, Signature};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("f.txt"), "base\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("f.txt")).unwrap();
    index.write().unwrap();
    let sig = Signature::now("Test", "test@example.com").unwrap();
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "base",
        &repo.find_tree(index.write_tree().unwrap()).unwrap(),
        &[],
    )
    .unwrap();
    std::fs::write(dir.path().join("f.txt"), "changed\n").unwrap();
    dir
}

#[test]
fn index_lock_failure_has_the_same_footer_and_receipt_for_single_and_batch() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    let paths = vec![PathBuf::from("f.txt")];
    for language in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(language); // isolated child settings stay under its KAGI_LOG_DIR
        for stage in [true, false] {
            let backend = kagi_git::Backend::open(repo).unwrap();
            if !stage {
                backend.stage_file(&paths[0]).unwrap();
            }
            let index = std::fs::read(repo.join(".git/index")).unwrap();
            let lock = repo.join(".git/index.lock");
            std::fs::write(&lock, "held by fixture").unwrap();
            let single = if stage {
                backend.stage_file(&paths[0])
            } else {
                backend.unstage_file(&paths[0])
            }
            .unwrap_err();
            let batch = if stage {
                backend.stage_files(&paths)
            } else {
                backend.unstage_files(&paths)
            }
            .unwrap_err();
            assert_eq!(single.to_string(), batch.to_string());
            for error in [single, batch] {
                let action = if stage {
                    StageAction::Stage
                } else {
                    StageAction::Unstage
                };
                let count = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len();
                let failure = StageFailure::record(action, repo, &paths, &error.to_string(), false);
                assert!(failure.footer.contains("f.txt"));
                assert!(failure.footer.contains(&repo.display().to_string()));
                assert!(
                    failure.footer.contains(&error.to_string()),
                    "{}",
                    failure.footer
                );
                assert!(failure.footer.contains("lock"), "{}", failure.footer);
                assert!(failure.footer.contains(if language == i18n::Lang::En {
                    "failed"
                } else {
                    "失敗しました"
                }));
                assert!(failure.notice);
                let entries = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100);
                assert_eq!(entries.len(), count + 1);
                assert!(matches!(
                    failure.recording.entry().outcome,
                    OpOutcome::Failed { .. }
                ));
                assert_eq!(
                    failure.recording.entry().worktree.as_deref(),
                    Some(repo.to_str().unwrap())
                );
                assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
            }
            std::fs::remove_file(lock).unwrap();
        }
    }
}

#[test]
fn failed_append_keeps_attempted_failure_and_explains_missing_record() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    let lock_path =
        PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap()).join("operations.jsonl.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)
        .unwrap();
    lock.lock().unwrap();
    let failure = StageFailure::record(
        StageAction::Stage,
        dir.path(),
        &["f.txt".into()],
        "index.lock held",
        false,
    );
    assert!(matches!(failure.recording, Recording::Failed { .. }));
    assert!(failure.footer.contains("index.lock held"));
    assert!(failure.footer.contains(i18n::Op::RecordOperation.t()));
    let entry = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(&failure.recording);
    assert!(matches!(entry.outcome, OpOutcome::Failed { .. }));
    assert!(!failure.footer.contains("changed but not recorded"));
    assert!(failure.notice);
}

#[test]
fn trust_refusal_is_recorded_without_a_modal_or_index_change() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    let index = std::fs::read(dir.path().join(".git/index")).unwrap();
    let mut backend = kagi_git::Backend::open(dir.path()).unwrap();
    backend.set_trust_for_test(kagi_git::trust::RepoTrust::Untrusted);
    let error = backend.stage_file(Path::new("f.txt")).unwrap_err();
    let failure = StageFailure::record(
        StageAction::Stage,
        dir.path(),
        &["f.txt".into()],
        &error.to_string(),
        matches!(error, GitError::Untrusted(_)),
    );
    assert!(matches!(
        failure.recording.entry().outcome,
        OpOutcome::Refused { .. }
    ));
    assert!(!failure.notice);
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(dir.path(), 10).len(),
        1
    );
    assert_eq!(std::fs::read(dir.path().join(".git/index")).unwrap(), index);
}

#[test]
fn repo_open_failure_preserves_the_original_cause_and_target() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let error = match kagi_git::Backend::open(dir.path()) {
        Err(error) => error,
        Ok(_) => panic!("fixture is not a repository"),
    };
    let failure = StageFailure::record(
        StageAction::Unstage,
        dir.path(),
        &["missing.txt".into()],
        &error.to_string(),
        false,
    );
    assert!(failure.footer.contains(&error.to_string()));
    assert!(failure.footer.contains("missing.txt"));
    let OpOutcome::Failed { error: recorded } = &failure.recording.entry().outcome else {
        panic!("open failure")
    };
    assert!(recorded.contains(&error.to_string()));
    assert!(recorded.contains(dir.path().to_str().unwrap()));
    assert!(!recorded.contains("session unavailable"));
}
