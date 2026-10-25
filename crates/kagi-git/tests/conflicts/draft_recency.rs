//! #1721: saved historical live drafts cannot outrank a later editor's Result.
use super::draft_revision::write_index_conflict;
use super::*;
use kagi_git::resolution::lines_to_text;

#[test]
fn saved_live_recovery_cannot_outrank_later_fork_body() {
    if !test_support::run_isolated() {
        return;
    }
    for save_detour_before_fork in [false, true] {
        let tmp = merge_conflict_repo();
        let repo = Repository::open(tmp.path()).unwrap();
        let path = Path::new("file.txt");
        let mut first = ResolutionBuffer::from_repo(&repo).unwrap();
        first.set_manual_text(path, "Y previously saved\n").unwrap();
        first.autosave().unwrap();
        let mut fork = first.clone();

        let mut index = repo.index().unwrap();
        let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
        conflict.our.as_mut().unwrap().mode = 0o100755;
        write_index_conflict(&mut index, &conflict);
        index.write().unwrap();
        let mut other_context = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        other_context.retain_drafts(first);
        if save_detour_before_fork {
            other_context.autosave().unwrap();
        }

        conflict.our.as_mut().unwrap().mode = 0o100644;
        write_index_conflict(&mut index, &conflict);
        index.write().unwrap();
        fork.set_manual_text(path, "Z later fork result\n").unwrap();
        fork.autosave().unwrap();
        for _ in 0..3 {
            // This buffer did not edit Y again. Saving B must not renew Y's recency.
            other_context.autosave().unwrap();
            let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
            assert_eq!(
                restored.resolved_text(path).as_deref(),
                Some("Z later fork result\n")
            );
            assert_eq!(
                restored
                    .recovery_drafts()
                    .filter_map(|draft| draft.result.map(lines_to_text))
                    .collect::<Vec<_>>(),
                ["Y previously saved\n"]
            );
        }
    }
}

#[test]
fn corrupt_draft_upgrade_restores_base_and_reserves_each_damaged_file() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let base = ResolutionBuffer::from_repo(&repo).unwrap();
    let store = base.autosave().unwrap();
    let index_before = std::fs::read(tmp.path().join(".git/index")).unwrap();
    let worktree_before = std::fs::read(tmp.path().join(path)).unwrap();
    for (suffix, torn) in [
        (".corrupt", "{\"files\":[first damaged document"),
        (".corrupt.1", "{\"files\":[second damaged document"),
    ] {
        std::fs::remove_file(store.with_file_name("buffer.json.prev")).unwrap();
        std::fs::write(&store, torn).unwrap();
        let mut restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert!(!restored.has_resolution(path));
        let notices = restored.take_draft_restore_warnings();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].recording_error.is_none());
        assert_eq!(
            notices[0].rescued_path,
            store.with_file_name(format!("buffer.json{suffix}"))
        );
        assert_eq!(
            std::fs::read_to_string(&notices[0].rescued_path).unwrap(),
            torn
        );
        assert!(!store.exists());
        assert!(restored.take_draft_restore_warnings().is_empty());
        restored
            .set_manual_text(path, "new editable base\n")
            .unwrap();
        restored.autosave().unwrap();
        assert_eq!(
            ResolutionBuffer::from_repo_with_autosave(&repo)
                .unwrap()
                .resolved_text(path)
                .as_deref(),
            Some("new editable base\n")
        );
    }
    assert_eq!(
        std::fs::read_to_string(store.with_file_name("buffer.json.corrupt")).unwrap(),
        "{\"files\":[first damaged document",
        "the second rescue must not overwrite the first"
    );
    let entries = kagi_git::oplog::read_oplog_tail(100);
    let rescues: Vec<_> = entries
        .iter()
        .filter(|entry| entry.op == "conflict-draft-restore")
        .collect();
    assert_eq!(rescues.len(), 2);
    assert!(rescues.iter().all(|entry| matches!(
        &entry.outcome,
        kagi_git::oplog::OpOutcome::Failed { error }
            if error.contains("damaged file was moved aside as")
    )));
    assert_eq!(
        index_before,
        std::fs::read(tmp.path().join(".git/index")).unwrap()
    );
    assert_eq!(
        worktree_before,
        std::fs::read(tmp.path().join(path)).unwrap()
    );
}
