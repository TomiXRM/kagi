//! #1721: unknown operation identity and preservation across live editing owners.
use super::draft_revision::{recovery_texts, saved_document, write_document, write_index_conflict};
use super::*;

fn assert_unreadable_context_is_inspection_only(repo: &Repository, path: &Path) {
    let mut unknown = ResolutionBuffer::from_repo_with_autosave(repo).unwrap();
    assert_eq!(unknown.file_identity(path), None);
    assert!(!unknown.has_resolution(path));
    unknown
        .set_manual_text(path, "written while context unreadable\n")
        .unwrap();
    unknown.autosave().unwrap();
    for _ in 0..3 {
        let restored = ResolutionBuffer::from_repo_with_autosave(repo).unwrap();
        assert_eq!(restored.file_identity(path), None);
        assert!(!restored.has_resolution(path));
        assert!(
            recovery_texts(&restored).contains(&"written while context unreadable\n".to_string())
        );
        restored.autosave().unwrap();
    }
}

#[test]
fn unreadable_head_never_issues_or_restores_known_draft_identity() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let head_path = repo.path().join("HEAD");
    let head = std::fs::read(&head_path).unwrap();
    std::fs::write(&head_path, "ref: refs/heads/missing-context-head\n").unwrap();
    assert_unreadable_context_is_inspection_only(&repo, Path::new("file.txt"));
    std::fs::write(head_path, head).unwrap();
}

#[test]
fn optional_metadata_absence_is_known_but_actual_read_error_is_not() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let metadata = repo.path().join("ORIG_HEAD");
    let original = std::fs::read(&metadata).unwrap();
    std::fs::remove_file(&metadata).unwrap();
    let path = Path::new("file.txt");
    let mut known = ResolutionBuffer::from_repo(&repo).unwrap();
    assert!(
        known.file_identity(path).is_some(),
        "legitimate optional absence is observable"
    );
    known
        .set_manual_text(path, "known with optional absence\n")
        .unwrap();
    known.autosave().unwrap();
    assert!(ResolutionBuffer::from_repo_with_autosave(&repo)
        .unwrap()
        .has_resolution(path));
    // A directory causes read() to fail even when fixture permissions are ignored.
    std::fs::create_dir(&metadata).unwrap();
    assert_unreadable_context_is_inspection_only(&repo, path);
    std::fs::remove_dir(&metadata).unwrap();
    std::fs::write(metadata, original).unwrap();
}

#[test]
fn missing_required_rebase_progress_cannot_authorize_saved_drafts() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = rebase_two_conflicting_steps_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let metadata = repo.path().join("rebase-merge/msgnum");
    let original = std::fs::read(&metadata).unwrap();
    std::fs::remove_file(&metadata).unwrap();
    assert_unreadable_context_is_inspection_only(&repo, Path::new("file.txt"));
    std::fs::write(metadata, original).unwrap();
}

#[test]
fn unreadable_rebase_source_fallback_is_not_a_known_operation_context() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = rebase_two_conflicting_steps_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let metadata = repo.path().join("rebase-merge/stopped-sha");
    let original = std::fs::read(&metadata).unwrap();
    std::fs::remove_file(&metadata).unwrap();
    std::fs::create_dir(&metadata).unwrap();
    assert_unreadable_context_is_inspection_only(&repo, Path::new("file.txt"));
    std::fs::remove_dir(&metadata).unwrap();
    std::fs::write(metadata, original).unwrap();
}

#[test]
fn unknown_same_live_autosaves_do_not_multiply_result_or_large_side_texts() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let current_side = "large current side\n".repeat(8192);
    let incoming_side = "large incoming side\n".repeat(8192);
    let mut index = repo.index().unwrap();
    let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
    conflict.our.as_mut().unwrap().id = repo.blob(current_side.as_bytes()).unwrap();
    conflict.their.as_mut().unwrap().id = repo.blob(incoming_side.as_bytes()).unwrap();
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    std::fs::write(
        repo.path().join("HEAD"),
        "ref: refs/heads/missing-draft-context\n",
    )
    .unwrap();
    let mut live = ResolutionBuffer::from_repo(&repo).unwrap();
    assert_eq!(live.file_identity(path), None);
    let mut first_size = 0;
    let mut origin = serde_json::Value::Null;
    for edit in 0..12 {
        let text = format!("unknown edit {edit:02}\n");
        live.set_manual_text(path, &text).unwrap();
        let saved_path = live.autosave().unwrap();
        let document = saved_document(&saved_path);
        assert_eq!(document["recoveries"].as_array().unwrap().len(), 0);
        assert_eq!(
            document["files"][0]["current"].as_str(),
            Some(current_side.as_str())
        );
        assert_eq!(
            document["files"][0]["incoming"].as_str(),
            Some(incoming_side.as_str())
        );
        let size = std::fs::metadata(&saved_path).unwrap().len();
        if edit == 0 {
            first_size = size;
            origin = document["files"][0]["editing_origin"].clone();
            assert!(origin.as_str().is_some_and(|value| !value.is_empty()));
        } else {
            assert_eq!(
                size, first_size,
                "unchanged side texts must occur only once"
            );
            assert_eq!(document["files"][0]["editing_origin"], origin);
        }
        let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
        assert_eq!(
            inspection.resolved_text(path).as_deref(),
            Some(text.as_str())
        );
    }
    let restarted = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert_eq!(restarted.file_identity(path), None);
    assert!(
        !restarted.has_resolution(path),
        "producer provenance is not restore authority"
    );
    assert_eq!(recovery_texts(&restarted), ["unknown edit 11\n"]);
}

#[test]
fn first_legacy_body_and_side_texts_survive_repeated_unknown_live_saves() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut original = ResolutionBuffer::from_repo(&repo).unwrap();
    original
        .set_manual_text(path, "first legacy body\n")
        .unwrap();
    let saved_path = original.autosave().unwrap();
    let mut legacy = saved_document(&saved_path);
    let legacy_current = "preserved legacy current\n".repeat(8192);
    let legacy_incoming = "preserved legacy incoming\n".repeat(8192);
    let file = legacy["files"][0].as_object_mut().unwrap();
    file.remove("identity");
    file.remove("editing_origin");
    file.insert("current".into(), serde_json::json!(legacy_current));
    file.insert("incoming".into(), serde_json::json!(legacy_incoming));
    write_document(&saved_path, &legacy);
    std::fs::write(
        repo.path().join("HEAD"),
        "ref: refs/heads/missing-draft-context\n",
    )
    .unwrap();
    let mut live = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!live.has_resolution(path));
    assert_eq!(recovery_texts(&live), ["first legacy body\n"]);
    let mut first_size = 0;
    for edit in 0..8 {
        live.set_manual_text(path, &format!("new unknown edit {edit}\n"))
            .unwrap();
        live.autosave().unwrap();
        let document = saved_document(&saved_path);
        assert_eq!(document["recoveries"].as_array().unwrap().len(), 1);
        assert_eq!(
            document["recoveries"][0]["current"].as_str(),
            Some(legacy_current.as_str())
        );
        assert_eq!(
            document["recoveries"][0]["incoming"].as_str(),
            Some(legacy_incoming.as_str())
        );
        let size = std::fs::metadata(&saved_path).unwrap().len();
        if edit == 0 {
            first_size = size;
        } else {
            assert_eq!(size, first_size);
        }
    }
    let restarted = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!restarted.has_resolution(path));
    assert_eq!(
        recovery_texts(&restarted),
        ["first legacy body\n", "new unknown edit 7\n"]
    );
}

#[test]
fn distinct_unknown_live_owners_preserve_predecessors_without_adopting_result() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    std::fs::write(
        repo.path().join("HEAD"),
        "ref: refs/heads/missing-draft-context\n",
    )
    .unwrap();
    let mut first = ResolutionBuffer::from_repo(&repo).unwrap();
    first
        .set_manual_text(path, "first unknown owner\n")
        .unwrap();
    let saved_path = first.autosave().unwrap();
    let first_origin = saved_document(&saved_path)["files"][0]["editing_origin"].clone();
    // A different live producer need not have loaded the previous document.
    let mut second = ResolutionBuffer::from_repo(&repo).unwrap();
    assert!(!second.has_resolution(path));
    second
        .set_manual_text(path, "second unknown owner\n")
        .unwrap();
    second.autosave().unwrap();
    let second_origin = saved_document(&saved_path)["files"][0]["editing_origin"].clone();
    assert_ne!(first_origin, second_origin);
    for edit in 0..4 {
        second
            .set_manual_text(path, &format!("second owner edit {edit}\n"))
            .unwrap();
        second.autosave().unwrap();
        let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
        assert_eq!(recovery_texts(&inspection), ["first unknown owner\n"]);
    }
    let mut restarted = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert_eq!(restarted.file_identity(path), None);
    assert!(!restarted.has_resolution(path));
    assert_eq!(
        recovery_texts(&restarted),
        ["first unknown owner\n", "second owner edit 3\n"]
    );
    restarted
        .set_manual_text(path, "restart explicit draft\n")
        .unwrap();
    restarted.autosave().unwrap();
    restarted
        .set_manual_text(path, "restart latest draft\n")
        .unwrap();
    restarted.autosave().unwrap();
    let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
    assert_eq!(
        recovery_texts(&inspection),
        ["first unknown owner\n", "second owner edit 3\n"]
    );
    let returned = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!returned.has_resolution(path));
    assert_eq!(
        recovery_texts(&returned),
        [
            "first unknown owner\n",
            "second owner edit 3\n",
            "restart latest draft\n"
        ]
    );
}

#[test]
fn direct_load_edit_save_starts_new_unknown_owner_instead_of_inheriting_disk_origin() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    std::fs::write(
        repo.path().join("HEAD"),
        "ref: refs/heads/missing-draft-context\n",
    )
    .unwrap();
    let mut first = ResolutionBuffer::from_repo(&repo).unwrap();
    first
        .set_manual_text(path, "prior process unknown body\n")
        .unwrap();
    let saved_path = first.autosave().unwrap();
    let first_origin = saved_document(&saved_path)["files"][0]["editing_origin"].clone();
    drop(first);
    // load is explicit inspection, not a live-index restore, but remains editable.
    let mut inspection = ResolutionBuffer::load(tmp.path()).unwrap();
    assert_eq!(
        inspection.resolved_text(path).as_deref(),
        Some("prior process unknown body\n")
    );
    inspection
        .set_manual_text(path, "new process first edit\n")
        .unwrap();
    inspection.autosave().unwrap();
    let new_origin = saved_document(&saved_path)["files"][0]["editing_origin"].clone();
    assert_ne!(first_origin, new_origin);
    for edit in 0..4 {
        inspection
            .set_manual_text(path, &format!("new process edit {edit}\n"))
            .unwrap();
        inspection.autosave().unwrap();
        let saved = saved_document(&saved_path);
        assert_eq!(saved["files"][0]["editing_origin"], new_origin);
        assert_eq!(saved["recoveries"].as_array().unwrap().len(), 1);
    }
    let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!restored.has_resolution(path));
    assert_eq!(
        recovery_texts(&restored),
        ["prior process unknown body\n", "new process edit 3\n"]
    );
    ResolutionBuffer::clear(tmp.path()).unwrap();
    let cleared = ResolutionBuffer::load(tmp.path()).unwrap();
    assert_eq!(recovery_texts(&cleared), ["prior process unknown body\n"]);
}

#[test]
fn cloned_unknown_buffer_edits_form_a_distinct_live_preservation_boundary() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    std::fs::write(
        repo.path().join("HEAD"),
        "ref: refs/heads/missing-draft-context\n",
    )
    .unwrap();
    let mut original = ResolutionBuffer::from_repo(&repo).unwrap();
    original
        .set_manual_text(path, "original before fork\n")
        .unwrap();
    let saved_path = original.autosave().unwrap();
    let original_origin = saved_document(&saved_path)["files"][0]["editing_origin"].clone();
    let mut fork = original.clone();
    fork.set_manual_text(path, "fork first edit\n").unwrap();
    fork.autosave().unwrap();
    let fork_origin = saved_document(&saved_path)["files"][0]["editing_origin"].clone();
    assert_ne!(original_origin, fork_origin);
    fork.set_manual_text(path, "fork latest edit\n").unwrap();
    fork.autosave().unwrap();
    let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
    assert_eq!(recovery_texts(&inspection), ["original before fork\n"]);
    original
        .set_manual_text(path, "original resumes edit\n")
        .unwrap();
    original.autosave().unwrap();
    original
        .set_manual_text(path, "original latest edit\n")
        .unwrap();
    original.autosave().unwrap();
    let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
    assert_eq!(
        recovery_texts(&inspection),
        ["original before fork\n", "fork latest edit\n"]
    );
    assert_eq!(
        inspection.resolved_text(path).as_deref(),
        Some("original latest edit\n")
    );
    assert_eq!(
        saved_document(&saved_path)["files"][0]["editing_origin"],
        original_origin
    );
    let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!restored.has_resolution(path));
    assert_eq!(
        recovery_texts(&restored),
        [
            "original before fork\n",
            "fork latest edit\n",
            "original latest edit\n"
        ]
    );
}
