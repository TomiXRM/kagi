use super::*;
use kagi_git::HunkChoice;

#[test]
fn add_add_materialization_accepts_regular_modes_and_rejects_raw_modes() {
    if !test_support::run_isolated() {
        return;
    }
    for (mode, binary, text_allowed) in [
        (0o100644, false, true),
        (0o100755, false, true),
        (0o100644, true, false),
        (0o120000, false, false),
        (0o160000, false, false),
    ] {
        let tmp = merge_conflict_repo();
        let repo = Repository::open(tmp.path()).unwrap();
        let path = Path::new("file.txt");
        let mut index = repo.index().unwrap();
        let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
        index.remove(path, 1).unwrap();
        conflict.ancestor = None;
        conflict.our.as_mut().unwrap().mode = mode;
        conflict.their.as_mut().unwrap().mode = mode;
        if binary {
            conflict.our.as_mut().unwrap().id = repo.blob(b"\0current\xff").unwrap();
        }
        draft_revision::write_index_conflict(&mut index, &conflict);
        index.write().unwrap();
        let index_before = std::fs::read(repo.path().join("index")).unwrap();
        let worktree_before = std::fs::read(tmp.path().join(path)).unwrap();

        let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        let markers = buffer.materialized_markers(&repo, path);
        assert_eq!(markers.is_some(), text_allowed, "mode={mode:o}, binary={binary}");
        assert_eq!(buffer.is_raw(path), !text_allowed);
        if let Some(markers) = markers {
            assert!(buffer.ensure_hunks(path, &markers));
            assert!(buffer.hunk_count(path) > 0);
            assert!(!buffer.has_resolution(path));
            for hunk in 0..buffer.hunk_count(path) {
                assert!(buffer.apply_hunk_choice(path, hunk, HunkChoice::AcceptCurrent));
            }
            assert_eq!(buffer.resolved_text(path).unwrap().as_bytes(), b"line one\nMAIN change\nline three\n");
            assert!(buffer.hunks_all_resolved(path));
            buffer.set_manual_text(path, "manual AddAdd\n").unwrap();
            assert_eq!(buffer.resolved_text(path).unwrap().as_bytes(), b"manual AddAdd\n");
        } else {
            assert!(!buffer.ensure_hunks(path, "<<<<<<< Current\nc\n=======\ni\n>>>>>>> Incoming\n"));
            assert!(buffer.hunk_model(path).is_none());
            assert!(!buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptCurrent));
            assert!(!buffer.has_resolution(path));
        }
        assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), index_before);
        assert_eq!(std::fs::read(tmp.path().join(path)).unwrap(), worktree_before);
        assert_eq!(repo.index().unwrap().get_path(path, 2).unwrap().mode, mode);
        assert_eq!(repo.index().unwrap().get_path(path, 3).unwrap().mode, mode);
    }
}
