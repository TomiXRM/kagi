//! #1721: saved historical live drafts cannot outrank a later editor's Result.
use super::*;
use super::draft_revision::write_index_conflict;
use kagi_git::resolution::lines_to_text;

#[test]
fn saved_live_recovery_cannot_outrank_later_fork_body() {
    if !test_support::run_isolated() { return; }
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
    other_context.autosave().unwrap();

    conflict.our.as_mut().unwrap().mode = 0o100644;
    write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    fork.set_manual_text(path, "Z later fork result\n").unwrap();
    fork.autosave().unwrap();
    for _ in 0..3 {
        // This buffer did not edit Y again. Saving B must not renew Y's recency.
        other_context.autosave().unwrap();
        let restored = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
        assert_eq!(restored.resolved_text(path).as_deref(), Some("Z later fork result\n"));
        assert_eq!(restored.recovery_drafts().filter_map(|draft| draft.result.map(lines_to_text)).collect::<Vec<_>>(), ["Y previously saved\n"]);
    }
}
