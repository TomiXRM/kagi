//! #1721: the real conflict producer and its autosave/adoption consumers.
use super::*;
use kagi_git::resolution::lines_to_text;

fn recovery_texts(buffer: &ResolutionBuffer) -> Vec<String> {
    buffer.recovery_drafts().filter_map(|draft| draft.result.map(lines_to_text)).collect()
}

fn saved_document(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn write_document(path: &Path, document: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec(document).unwrap()).unwrap();
}

pub(super) fn write_index_conflict(index: &mut git2::Index, conflict: &git2::IndexConflict) {
    // Index::add preserves each entry's stage bits and raw path bytes.
    for entry in [&conflict.ancestor, &conflict.our, &conflict.their].into_iter().flatten() {
        index.add(entry).unwrap();
    }
}

#[test]
fn continue_same_path_requires_distinct_second_revision_input() {
    if !test_support::run_isolated() { return; }
    let tmp = rebase_two_conflicting_steps_repo();
    let dir = tmp.path();
    let path = Path::new("file.txt");
    let repo = Repository::open(dir).unwrap();
    let first_session = detect_conflict_session(&repo).unwrap();
    let first_rebase_head = std::fs::read(dir.join(".git/REBASE_HEAD")).unwrap();
    let mut first = ResolutionBuffer::from_repo(&repo).unwrap();
    let first_identity = first.file_identity(path).unwrap().clone();
    first.set_manual_text(path, "RESOLVED ONE\n").unwrap();
    let saved_path = first.autosave().unwrap();
    let first_disk = std::fs::read(&saved_path).unwrap();
    execute_conflict_continue(&repo, dir, &first_session, &first).unwrap();
    assert_ne!(first_rebase_head, std::fs::read(dir.join(".git/REBASE_HEAD")).unwrap());
    assert_eq!(git_output(dir, &["show", "HEAD:file.txt"]), "RESOLVED ONE");
    assert!(git_output(dir, &["status", "--porcelain"]).contains("UU file.txt"));
    let repo = Repository::open(dir).unwrap();
    let second_session = detect_conflict_session(&repo).unwrap();
    let conflict = repo.index().unwrap().conflicts().unwrap().next().unwrap().unwrap();
    assert!(conflict.ancestor.is_some() && conflict.our.is_some() && conflict.their.is_some());
    // Simulate restoring the first revision's autosave after Git advanced.
    std::fs::write(&saved_path, &first_disk).unwrap();
    let mut second = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert_ne!(second.file_identity(path), Some(&first_identity));
    second.retain_drafts(first);
    assert!(!second.has_resolution(path));
    assert_eq!(recovery_texts(&second), ["RESOLVED ONE\n"]);
    let index_before = std::fs::read(dir.join(".git/index")).unwrap();
    let worktree_before = std::fs::read(dir.join(path)).unwrap();
    for _ in 0..3 {
        assert!(!second.has_resolution(path));
        assert!(continue_blockers(&repo, &second_session, &second).iter()
            .any(|blocker| blocker.code() == "unresolved-files"));
        second.autosave().unwrap();
        second = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert_eq!(recovery_texts(&second), ["RESOLVED ONE\n"]);
    }
    assert_eq!(index_before, std::fs::read(dir.join(".git/index")).unwrap());
    assert_eq!(worktree_before, std::fs::read(dir.join(path)).unwrap());
    second.set_manual_text(path, "RESOLVED TWO\n").unwrap();
    assert!(continue_blockers(&repo, &second_session, &second).is_empty());
    execute_conflict_continue(&repo, dir, &second_session, &second).unwrap();
    assert!(detect_conflict_session(&Repository::open(dir).unwrap()).is_none());
    assert_eq!(git_output(dir, &["show", "HEAD:file.txt"]), "RESOLVED TWO");
    assert_eq!(git_output(dir, &["show", "HEAD^:file.txt"]), "RESOLVED ONE");
}

#[test]
fn same_context_restores_disk_provenance_and_moves_latest_live_undo() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut previous = ResolutionBuffer::from_repo(&repo).unwrap();
    previous.apply_choice(path, ResolutionChoice::Incoming).unwrap();
    previous.autosave().unwrap();
    let mut restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert_eq!(restored.resolved_text(path), previous.resolved_text(path));
    assert!(restored.provenance(path).unwrap().iter().all(|origin| *origin == LineOrigin::Incoming));
    let markers = previous.materialized_markers(&repo, path).unwrap();
    previous.ensure_hunks(path, &markers);
    previous.set_manual_text(path, "latest unsaved\n").unwrap();
    restored.retain_drafts(previous);
    assert_eq!(restored.resolved_text(path).as_deref(), Some("latest unsaved\n"));
    assert!(restored.hunk_model(path).is_some());
    assert!(restored.undo(path));
    assert!(restored.provenance(path).unwrap().iter().all(|origin| *origin == LineOrigin::Incoming));
    assert!(restored.redo(path));
    assert_eq!(restored.resolved_text(path).as_deref(), Some("latest unsaved\n"));
    assert!(restored.undo(path));
    assert!(restored.undo(path));
    assert!(!restored.has_resolution(path));
    let mut fresh = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    fresh.retain_drafts(restored);
    assert!(!fresh.has_resolution(path), "latest live null wins over saved authority");
    fresh.autosave().unwrap();
    assert!(!ResolutionBuffer::from_repo_with_autosave(&repo).unwrap().has_resolution(path));
    assert!(fresh.redo(path));
}

#[test]
fn legacy_unknown_and_unbound_results_are_inspection_only() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut live = ResolutionBuffer::from_repo(&repo).unwrap();
    live.set_manual_text(path, "legacy \u{1D11E}\n").unwrap();
    let saved_path = live.autosave().unwrap();
    let original = saved_document(&saved_path);
    for identity in [serde_json::Value::Null, serde_json::json!({"version":2,"fingerprint":"a".repeat(64)}),
        serde_json::json!({"version":1,"fingerprint":"not-a-revision"})] {
        let mut saved = original.clone();
        saved["files"][0]["identity"] = identity;
        write_document(&saved_path, &saved);
        let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
        assert_eq!(inspection.resolved_text(path).as_deref(), Some("legacy \u{1D11E}\n"));
        let mut current = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert!(!current.has_resolution(path));
        assert_eq!(current.recovery_drafts().next().unwrap().identity, None);
        assert_eq!(recovery_texts(&current), ["legacy \u{1D11E}\n"]);
        current.retain_drafts(inspection);
        current.autosave().unwrap();
        assert_eq!(ResolutionBuffer::load(tmp.path()).unwrap().recovery_drafts().count(), 1);
    }
}

#[test]
fn empty_resolution_and_null_remain_distinct_through_bound_restore() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut current = ResolutionBuffer::from_repo(&repo).unwrap();
    current.set_manual_text(path, "").unwrap();
    current.autosave().unwrap();
    let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(restored.has_resolution(path));
    assert_eq!(restored.resolved_text(path).as_deref(), Some(""));
    assert_eq!(restored.provenance(path), Some(vec![]));
    assert!(current.undo(path));
    let mut adopted = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    adopted.retain_drafts(current);
    adopted.autosave().unwrap();
    let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!restored.has_resolution(path));
    assert_eq!(restored.resolved_text(path), None);
}

#[test]
fn exact_file_identity_excludes_unrelated_saves_and_includes_stages() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let original = ResolutionBuffer::from_repo(&repo).unwrap().file_identity(path).unwrap().clone();
    write_file(tmp.path(), "unrelated.txt", "staged elsewhere\n");
    git(tmp.path(), &["add", "unrelated.txt"]);
    let mut index = repo.index().unwrap();
    index.read(true).unwrap();
    assert_eq!(ResolutionBuffer::from_repo(&repo).unwrap().file_identity(path), Some(&original));
    let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
    conflict.our.as_mut().unwrap().mode = 0o100755;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    let changed_mode = ResolutionBuffer::from_repo(&repo).unwrap().file_identity(path).unwrap().clone();
    assert_ne!(original, changed_mode);
    conflict.our.as_mut().unwrap().mode = 0o100644;
    conflict.their.as_mut().unwrap().id = repo.blob(b"different incoming\n").unwrap();
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    let changed_oid = ResolutionBuffer::from_repo(&repo).unwrap().file_identity(path).unwrap().clone();
    assert_ne!(original, changed_oid);
    assert_ne!(changed_mode, changed_oid);
    index.remove(path, 1).unwrap();
    conflict.ancestor = None;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    assert_ne!(ResolutionBuffer::from_repo(&repo).unwrap().file_identity(path), Some(&changed_oid));
}

#[test]
fn raw_oid_mode_and_text_incompatibilities_remain_recovery_only() {
    if !test_support::run_isolated() { return; }
    let tmp = binary_merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("blob.bin");
    let mut current = ResolutionBuffer::from_repo(&repo).unwrap();
    current.apply_choice(path, ResolutionChoice::Current).unwrap();
    let saved_path = current.autosave().unwrap();
    let original = saved_document(&saved_path);
    let chosen = current.raw_resolution(path).unwrap();
    for (oid, mode) in [(git2::Oid::ZERO_SHA1, chosen.mode), (chosen.oid, 0o100755)] {
        let mut document = original.clone();
        document["files"][0]["raw_result"] = serde_json::json!({"oid":oid.to_string(),"mode":mode});
        write_document(&saved_path, &document);
        let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert!(!restored.has_resolution(path));
        let recovery = restored.recovery_drafts().next().unwrap();
        assert_eq!(recovery.raw_result.unwrap().oid, oid);
        assert_eq!(recovery.raw_result.unwrap().mode, mode);
        restored.autosave().unwrap();
        assert!(!ResolutionBuffer::from_repo_with_autosave(&repo).unwrap().has_resolution(path));
    }
    let mut document = original;
    document["files"][0]["raw_result"] = serde_json::Value::Null;
    document["files"][0]["result"] = serde_json::json!([{"t":"not binary bytes","o":"m"}]);
    write_document(&saved_path, &document);
    let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!restored.has_resolution(path));
    assert_eq!(recovery_texts(&restored), ["not binary bytes\n"]);
}

#[test]
fn recoveries_survive_fresh_autosave_clear_and_return_to_exact_context() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut first = ResolutionBuffer::from_repo(&repo).unwrap();
    first.set_manual_text(path, "known original\n").unwrap();
    let first_identity = first.file_identity(path).unwrap().clone();
    first.autosave().unwrap();
    let mut index = repo.index().unwrap();
    let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
    let original_mode = conflict.our.as_ref().unwrap().mode;
    conflict.our.as_mut().unwrap().mode = 0o100755;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    for _ in 0..3 {
        // Deliberately do not load the previous document before saving.
        ResolutionBuffer::from_repo(&repo).unwrap().autosave().unwrap();
        let current = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert!(!current.has_resolution(path));
        assert_eq!(recovery_texts(&current), ["known original\n"]);
    }
    ResolutionBuffer::clear(tmp.path()).unwrap();
    assert_eq!(recovery_texts(&ResolutionBuffer::load(tmp.path()).unwrap()), ["known original\n"]);
    conflict.our.as_mut().unwrap().mode = original_mode;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert_eq!(restored.file_identity(path), Some(&first_identity));
    assert_eq!(restored.resolved_text(path).as_deref(), Some("known original\n"));
    assert_eq!(restored.recovery_drafts().count(), 0);
}

#[cfg(unix)]
#[test]
fn raw_non_utf8_stage_paths_round_trip_without_lossy_alias_authority() {
    use std::os::unix::ffi::OsStrExt;
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let mut index = repo.index().unwrap();
    let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
    index.conflict_remove(Path::new("file.txt")).unwrap();
    let bytes = b"invalid-\xff.txt";
    for entry in [&mut conflict.ancestor, &mut conflict.our, &mut conflict.their].into_iter().flatten() {
        entry.path = bytes.to_vec();
    }
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    let path = Path::new(std::ffi::OsStr::from_bytes(bytes));
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer.set_manual_text(path, "byte faithful draft\n").unwrap();
    buffer.autosave().unwrap();
    let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert_eq!(restored.file_identity(path), buffer.file_identity(path));
    assert_eq!(restored.resolved_text(path).as_deref(), Some("byte faithful draft\n"));
    assert_eq!(restored.file_identity(Path::new("invalid-\u{fffd}.txt")), None);
}

#[test]
fn failed_autosave_cannot_replace_latest_live_edits_or_undo_on_adoption() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut previous = ResolutionBuffer::from_repo(&repo).unwrap();
    previous.set_manual_text(path, "saved predecessor\n").unwrap();
    let saved_path = previous.autosave().unwrap();
    // Fixture-owned obstruction: serialization succeeds but persistence cannot.
    std::fs::remove_file(&saved_path).unwrap();
    std::fs::create_dir(&saved_path).unwrap();
    previous.set_manual_text(path, "latest despite failure\n").unwrap();
    assert!(previous.autosave().is_err());
    let mut current = ResolutionBuffer::from_repo(&repo).unwrap();
    current.retain_drafts(previous);
    assert_eq!(current.resolved_text(path).as_deref(), Some("latest despite failure\n"));
    assert!(current.undo(path));
    assert_eq!(current.resolved_text(path).as_deref(), Some("saved predecessor\n"));
    assert!(current.redo(path));
    assert_eq!(current.resolved_text(path).as_deref(), Some("latest despite failure\n"));
}

#[test]
fn raw_choice_cannot_be_restored_into_a_text_conflict_even_with_bound_identity() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let saved_path = buffer.autosave().unwrap();
    let mut document = saved_document(&saved_path);
    let oid = repo.blob(b"not a text resolution").unwrap();
    document["files"][0]["raw_result"] = serde_json::json!({"oid":oid.to_string(),"mode":0o100644});
    write_document(&saved_path, &document);
    let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!restored.has_resolution(path));
    assert_eq!(restored.recovery_drafts().next().unwrap().raw_result.unwrap().oid, oid);
}

#[test]
fn returning_context_uses_latest_known_body_without_deleting_older_recoveries() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut first = ResolutionBuffer::from_repo(&repo).unwrap();
    first.set_manual_text(path, "latest known body\n").unwrap();
    let saved_path = first.autosave().unwrap();
    let mut document = saved_document(&saved_path);
    let mut older = document["files"][0].clone();
    older["result"] = serde_json::json!([{"t":"older preserved body","o":"i"}]);
    document["recoveries"] = serde_json::json!([older]);
    write_document(&saved_path, &document);
    let mut index = repo.index().unwrap();
    let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
    conflict.our.as_mut().unwrap().mode = 0o100755;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    for _ in 0..3 {
        ResolutionBuffer::from_repo(&repo).unwrap().autosave().unwrap();
    }
    conflict.our.as_mut().unwrap().mode = 0o100644;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    let mut current = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert_eq!(current.resolved_text(path).as_deref(), Some("latest known body\n"));
    assert_eq!(recovery_texts(&current), ["older preserved body\n"]);
    assert_eq!(current.recovery_drafts().next().unwrap().result.unwrap()[0].origin, LineOrigin::Incoming);
    current.set_manual_text(path, "new explicit current body\n").unwrap();
    current.autosave().unwrap();
    // Clearing today's current draft cannot remove yesterday's preserved body.
    ResolutionBuffer::clear(tmp.path()).unwrap();
    let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
    assert!(!inspection.has_resolution(path));
    assert_eq!(recovery_texts(&inspection), ["older preserved body\n", "latest known body\n"]);
    for _ in 0..3 {
        let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert_eq!(restored.resolved_text(path).as_deref(), Some("latest known body\n"));
        assert_eq!(recovery_texts(&restored), ["older preserved body\n"]);
        restored.autosave().unwrap();
    }
}

#[test]
fn fresh_same_context_null_cannot_erase_saved_or_latest_live_draft_state() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut edited = ResolutionBuffer::from_repo(&repo).unwrap();
    edited.apply_choice(path, ResolutionChoice::Incoming).unwrap();
    edited.autosave().unwrap();
    let expected = edited.resolved_text(path);
    for _ in 0..3 {
        ResolutionBuffer::from_repo(&repo).unwrap().autosave().unwrap();
        let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert_eq!(restored.resolved_text(path), expected);
        assert_eq!(restored.provenance(path), edited.provenance(path));
        assert_eq!(restored.recovery_drafts().count(), 0);
    }
    let mut restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    restored.retain_drafts(ResolutionBuffer::from_repo(&repo).unwrap());
    assert_eq!(restored.resolved_text(path), expected);
    edited.set_manual_text(path, "latest live revision\n").unwrap();
    restored.retain_drafts(edited);
    assert_eq!(restored.resolved_text(path).as_deref(), Some("latest live revision\n"));
    assert!(restored.undo(path));
    assert_eq!(restored.resolved_text(path), expected);
}

#[test]
fn explicit_null_survives_fresh_autosave_and_context_return_without_reviving_old_body() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut edited = ResolutionBuffer::from_repo(&repo).unwrap();
    edited.set_manual_text(path, "active before undo\n").unwrap();
    let saved_path = edited.autosave().unwrap();
    let mut document = saved_document(&saved_path);
    let mut older = document["files"][0].clone();
    older["result"] = serde_json::json!([{"t":"older body stays inspectable","o":"c"}]);
    document["recoveries"] = serde_json::json!([older]);
    write_document(&saved_path, &document);
    assert!(edited.undo(path));
    let mut adopted = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    adopted.retain_drafts(edited);
    assert!(!adopted.has_resolution(path));
    adopted.autosave().unwrap();
    assert!(!ResolutionBuffer::from_repo_with_autosave(&repo).unwrap().has_resolution(path));
    ResolutionBuffer::clear(tmp.path()).unwrap();
    let after_clear = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!after_clear.has_resolution(path));
    assert_eq!(recovery_texts(&after_clear), ["older body stays inspectable\n"]);
    // Fresh materialization must carry the explicit null intent forward too.
    for _ in 0..3 {
        ResolutionBuffer::from_repo(&repo).unwrap().autosave().unwrap();
        let current = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert!(!current.has_resolution(path));
        assert_eq!(recovery_texts(&current), ["older body stays inspectable\n"]);
    }
    let original_identity = adopted.file_identity(path).unwrap().clone();
    let mut index = repo.index().unwrap();
    let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
    conflict.our.as_mut().unwrap().mode = 0o100755;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    for _ in 0..3 {
        ResolutionBuffer::from_repo(&repo).unwrap().autosave().unwrap();
        let other = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert!(!other.has_resolution(path));
        assert_eq!(recovery_texts(&other), ["older body stays inspectable\n"]);
    }
    conflict.our.as_mut().unwrap().mode = 0o100644;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    for _ in 0..3 {
        let returned = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert_eq!(returned.file_identity(path), Some(&original_identity));
        assert!(!returned.has_resolution(path), "returning context cannot revive a body explicitly undone");
        assert_eq!(recovery_texts(&returned), ["older body stays inspectable\n"]);
        // Public inspection contains bodies, never internal null intent records.
        assert_eq!(returned.recovery_drafts().count(), 1);
        returned.autosave().unwrap();
    }
}

#[test]
fn failed_autosave_latest_live_null_moves_duplicate_intent_after_newer_disk_body() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut live = ResolutionBuffer::from_repo(&repo).unwrap();
    live.set_manual_text(path, "Y remains inspectable\n").unwrap();
    let saved_path = live.autosave().unwrap();
    let body = saved_document(&saved_path)["files"][0].clone();
    assert!(live.undo(path));
    live.autosave().unwrap();
    let null = saved_document(&saved_path)["files"][0].clone();
    assert!(live.redo(path));
    assert!(live.undo(path));
    // A real write failure leaves the latest undo only in the live buffer.
    std::fs::remove_file(&saved_path).unwrap();
    std::fs::create_dir(&saved_path).unwrap();
    assert!(live.autosave().is_err());
    std::fs::remove_dir(&saved_path).unwrap();
    let disk = serde_json::json!({"files":[],"recoveries":[null,body]});
    write_document(&saved_path, &disk);
    let mut index = repo.index().unwrap();
    let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
    conflict.our.as_mut().unwrap().mode = 0o100755;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    let mut other = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    other.retain_drafts(live);
    assert_eq!(recovery_texts(&other), ["Y remains inspectable\n"]);
    other.autosave().unwrap();
    assert_eq!(saved_document(&saved_path)["recoveries"].as_array().unwrap().len(), 2);
    conflict.our.as_mut().unwrap().mode = 0o100644;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    for _ in 0..3 {
        let returned = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert!(!returned.has_resolution(path), "latest equal null must be newer than Y");
        assert_eq!(recovery_texts(&returned), ["Y remains inspectable\n"]);
        returned.autosave().unwrap();
    }
}

fn assert_unreadable_context_is_inspection_only(repo: &Repository, path: &Path) {
    let mut unknown = ResolutionBuffer::from_repo_with_autosave(repo).unwrap();
    assert_eq!(unknown.file_identity(path), None);
    assert!(!unknown.has_resolution(path));
    unknown.set_manual_text(path, "written while context unreadable\n").unwrap();
    unknown.autosave().unwrap();
    for _ in 0..3 {
        let restored = ResolutionBuffer::from_repo_with_autosave(repo).unwrap();
        assert_eq!(restored.file_identity(path), None);
        assert!(!restored.has_resolution(path));
        assert!(recovery_texts(&restored).contains(&"written while context unreadable\n".to_string()));
        restored.autosave().unwrap();
    }
}

#[test]
fn unreadable_head_never_issues_or_restores_known_draft_identity() {
    if !test_support::run_isolated() { return; }
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
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let metadata = repo.path().join("ORIG_HEAD");
    let original = std::fs::read(&metadata).unwrap();
    std::fs::remove_file(&metadata).unwrap();
    let path = Path::new("file.txt");
    let mut known = ResolutionBuffer::from_repo(&repo).unwrap();
    assert!(known.file_identity(path).is_some(), "legitimate optional absence is observable");
    known.set_manual_text(path, "known with optional absence\n").unwrap();
    known.autosave().unwrap();
    assert!(ResolutionBuffer::from_repo_with_autosave(&repo).unwrap().has_resolution(path));
    // A directory causes read() to fail even when fixture permissions are ignored.
    std::fs::create_dir(&metadata).unwrap();
    assert_unreadable_context_is_inspection_only(&repo, path);
    std::fs::remove_dir(&metadata).unwrap();
    std::fs::write(metadata, original).unwrap();
}

#[test]
fn missing_required_rebase_progress_cannot_authorize_saved_drafts() {
    if !test_support::run_isolated() { return; }
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
    if !test_support::run_isolated() { return; }
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
    if !test_support::run_isolated() { return; }
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
    std::fs::write(repo.path().join("HEAD"), "ref: refs/heads/missing-draft-context\n").unwrap();
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
        assert_eq!(document["files"][0]["current"].as_str(), Some(current_side.as_str()));
        assert_eq!(document["files"][0]["incoming"].as_str(), Some(incoming_side.as_str()));
        let size = std::fs::metadata(&saved_path).unwrap().len();
        if edit == 0 {
            first_size = size;
            origin = document["files"][0]["editing_origin"].clone();
            assert!(origin.as_str().is_some_and(|value| !value.is_empty()));
        } else {
            assert_eq!(size, first_size, "unchanged side texts must occur only once");
            assert_eq!(document["files"][0]["editing_origin"], origin);
        }
        let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
        assert_eq!(inspection.resolved_text(path).as_deref(), Some(text.as_str()));
    }
    let restarted = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert_eq!(restarted.file_identity(path), None);
    assert!(!restarted.has_resolution(path), "producer provenance is not restore authority");
    assert_eq!(recovery_texts(&restarted), ["unknown edit 11\n"]);
}

#[test]
fn first_legacy_body_and_side_texts_survive_repeated_unknown_live_saves() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    let mut original = ResolutionBuffer::from_repo(&repo).unwrap();
    original.set_manual_text(path, "first legacy body\n").unwrap();
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
    std::fs::write(repo.path().join("HEAD"), "ref: refs/heads/missing-draft-context\n").unwrap();
    let mut live = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!live.has_resolution(path));
    assert_eq!(recovery_texts(&live), ["first legacy body\n"]);
    let mut first_size = 0;
    for edit in 0..8 {
        live.set_manual_text(path, &format!("new unknown edit {edit}\n")).unwrap();
        live.autosave().unwrap();
        let document = saved_document(&saved_path);
        assert_eq!(document["recoveries"].as_array().unwrap().len(), 1);
        assert_eq!(document["recoveries"][0]["current"].as_str(), Some(legacy_current.as_str()));
        assert_eq!(document["recoveries"][0]["incoming"].as_str(), Some(legacy_incoming.as_str()));
        let size = std::fs::metadata(&saved_path).unwrap().len();
        if edit == 0 { first_size = size; } else { assert_eq!(size, first_size); }
    }
    let restarted = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!restarted.has_resolution(path));
    assert_eq!(recovery_texts(&restarted), ["first legacy body\n", "new unknown edit 7\n"]);
}

#[test]
fn distinct_unknown_live_owners_preserve_predecessors_without_adopting_result() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    std::fs::write(repo.path().join("HEAD"), "ref: refs/heads/missing-draft-context\n").unwrap();
    let mut first = ResolutionBuffer::from_repo(&repo).unwrap();
    first.set_manual_text(path, "first unknown owner\n").unwrap();
    let saved_path = first.autosave().unwrap();
    let first_origin = saved_document(&saved_path)["files"][0]["editing_origin"].clone();
    // A different live producer need not have loaded the previous document.
    let mut second = ResolutionBuffer::from_repo(&repo).unwrap();
    assert!(!second.has_resolution(path));
    second.set_manual_text(path, "second unknown owner\n").unwrap();
    second.autosave().unwrap();
    let second_origin = saved_document(&saved_path)["files"][0]["editing_origin"].clone();
    assert_ne!(first_origin, second_origin);
    for edit in 0..4 {
        second.set_manual_text(path, &format!("second owner edit {edit}\n")).unwrap();
        second.autosave().unwrap();
        let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
        assert_eq!(recovery_texts(&inspection), ["first unknown owner\n"]);
    }
    let mut restarted = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert_eq!(restarted.file_identity(path), None);
    assert!(!restarted.has_resolution(path));
    assert_eq!(recovery_texts(&restarted), ["first unknown owner\n", "second owner edit 3\n"]);
    restarted.set_manual_text(path, "restart explicit draft\n").unwrap();
    restarted.autosave().unwrap();
    restarted.set_manual_text(path, "restart latest draft\n").unwrap();
    restarted.autosave().unwrap();
    let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
    assert_eq!(recovery_texts(&inspection), ["first unknown owner\n", "second owner edit 3\n"]);
    let returned = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!returned.has_resolution(path));
    assert_eq!(recovery_texts(&returned),
        ["first unknown owner\n", "second owner edit 3\n", "restart latest draft\n"]);
}

#[test]
fn direct_load_edit_save_starts_new_unknown_owner_instead_of_inheriting_disk_origin() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    std::fs::write(repo.path().join("HEAD"), "ref: refs/heads/missing-draft-context\n").unwrap();
    let mut first = ResolutionBuffer::from_repo(&repo).unwrap();
    first.set_manual_text(path, "prior process unknown body\n").unwrap();
    let saved_path = first.autosave().unwrap();
    let first_origin = saved_document(&saved_path)["files"][0]["editing_origin"].clone();
    drop(first);
    // load is explicit inspection, not a live-index restore, but remains editable.
    let mut inspection = ResolutionBuffer::load(tmp.path()).unwrap();
    assert_eq!(inspection.resolved_text(path).as_deref(), Some("prior process unknown body\n"));
    inspection.set_manual_text(path, "new process first edit\n").unwrap();
    inspection.autosave().unwrap();
    let new_origin = saved_document(&saved_path)["files"][0]["editing_origin"].clone();
    assert_ne!(first_origin, new_origin);
    for edit in 0..4 {
        inspection.set_manual_text(path, &format!("new process edit {edit}\n")).unwrap();
        inspection.autosave().unwrap();
        let saved = saved_document(&saved_path);
        assert_eq!(saved["files"][0]["editing_origin"], new_origin);
        assert_eq!(saved["recoveries"].as_array().unwrap().len(), 1);
    }
    let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!restored.has_resolution(path));
    assert_eq!(recovery_texts(&restored), ["prior process unknown body\n", "new process edit 3\n"]);
    ResolutionBuffer::clear(tmp.path()).unwrap();
    let cleared = ResolutionBuffer::load(tmp.path()).unwrap();
    assert_eq!(recovery_texts(&cleared), ["prior process unknown body\n"]);
}

#[test]
fn cloned_unknown_buffer_edits_form_a_distinct_live_preservation_boundary() {
    if !test_support::run_isolated() { return; }
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");
    std::fs::write(repo.path().join("HEAD"), "ref: refs/heads/missing-draft-context\n").unwrap();
    let mut original = ResolutionBuffer::from_repo(&repo).unwrap();
    original.set_manual_text(path, "original before fork\n").unwrap();
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
    original.set_manual_text(path, "original resumes edit\n").unwrap();
    original.autosave().unwrap();
    original.set_manual_text(path, "original latest edit\n").unwrap();
    original.autosave().unwrap();
    let inspection = ResolutionBuffer::load(tmp.path()).unwrap();
    assert_eq!(recovery_texts(&inspection), ["original before fork\n", "fork latest edit\n"]);
    assert_eq!(inspection.resolved_text(path).as_deref(), Some("original latest edit\n"));
    assert_eq!(saved_document(&saved_path)["files"][0]["editing_origin"], original_origin);
    let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!restored.has_resolution(path));
    assert_eq!(recovery_texts(&restored),
        ["original before fork\n", "fork latest edit\n", "original latest edit\n"]);
}
