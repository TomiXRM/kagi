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
        assert_eq!(
            markers.is_some(),
            text_allowed,
            "mode={mode:o}, binary={binary}"
        );
        assert_eq!(buffer.is_raw(path), !text_allowed);
        if let Some(markers) = markers {
            assert_eq!(markers.marker_width, 7);
            assert!(buffer.ensure_hunks(path, &markers.text, markers.marker_width));
            assert!(buffer.hunk_count(path) > 0);
            assert!(!buffer.has_resolution(path));
            for hunk in 0..buffer.hunk_count(path) {
                assert!(buffer.apply_hunk_choice(path, hunk, HunkChoice::AcceptCurrent));
            }
            assert_eq!(
                buffer.resolved_text(path).unwrap().as_bytes(),
                b"line one\nMAIN change\nline three\n"
            );
            assert!(buffer.hunks_all_resolved(path));
            buffer.set_manual_text(path, "manual AddAdd\n").unwrap();
            assert_eq!(
                buffer.resolved_text(path).unwrap().as_bytes(),
                b"manual AddAdd\n"
            );
        } else {
            assert!(!buffer.ensure_hunks(
                path,
                "<<<<<<< Current\nc\n=======\ni\n>>>>>>> Incoming\n",
                7,
            ));
            assert!(buffer.hunk_model(path).is_none());
            assert!(!buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptCurrent));
            assert!(!buffer.has_resolution(path));
        }
        assert_eq!(
            std::fs::read(repo.path().join("index")).unwrap(),
            index_before
        );
        assert_eq!(
            std::fs::read(tmp.path().join(path)).unwrap(),
            worktree_before
        );
        assert_eq!(repo.index().unwrap().get_path(path, 2).unwrap().mode, mode);
        assert_eq!(repo.index().unwrap().get_path(path, 3).unwrap().mode, mode);
    }
}

fn materialize_sources(
    base: Option<&str>,
    current: &str,
    incoming: &str,
) -> (tempfile::TempDir, Repository, ResolutionBuffer) {
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let mut index = repo.index().unwrap();
    let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
    if let Some(base) = base {
        conflict.ancestor.as_mut().unwrap().id = repo.blob(base.as_bytes()).unwrap();
    } else {
        index.remove(Path::new("file.txt"), 1).unwrap();
        conflict.ancestor = None;
    }
    conflict.our.as_mut().unwrap().id = repo.blob(current.as_bytes()).unwrap();
    conflict.their.as_mut().unwrap().id = repo.blob(incoming.as_bytes()).unwrap();
    draft_revision::write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    (tmp, repo, buffer)
}

#[test]
fn nine_character_separator_uses_ten_character_markers() {
    if !test_support::run_isolated() {
        return;
    }
    for base in [Some("base\n"), None] {
        let current = "current\n=========\nliteral separator\n";
        let incoming = "incoming\n";
        let (_tmp, repo, mut buffer) = materialize_sources(base, current, incoming);
        let path = Path::new("file.txt");
        let marker = buffer.materialized_markers(&repo, path).unwrap();
        assert_eq!(marker.marker_width, 10);
        assert!(marker.text.contains("<<<<<<<<<< Current\n"));
        assert!(marker.text.contains("\n==========\n"));
        assert!(marker.text.contains("\n>>>>>>>>>> Incoming\n"));
        assert!(buffer.ensure_hunks(path, &marker.text, marker.marker_width));
        assert_eq!(buffer.hunk_count(path), 1);
        assert!(buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptCurrent));
        assert_eq!(buffer.resolved_text(path).unwrap(), current);
        assert!(buffer.hunks_all_resolved(path));
        assert!(buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptIncoming));
        assert_eq!(buffer.resolved_text(path).unwrap(), incoming);
    }
}

#[test]
fn marker_width_accounts_for_every_source_and_delimiter_character() {
    if !test_support::run_isolated() {
        return;
    }
    for source in 0..3 {
        for delimiter in ['<', '=', '>', '|'] {
            let mut sources = [
                "base\n".to_owned(),
                "current\n".to_owned(),
                "incoming\n".to_owned(),
            ];
            sources[source].push_str(&delimiter.to_string().repeat(12));
            sources[source].push_str(" literal\n");
            let (_tmp, repo, buffer) =
                materialize_sources(Some(&sources[0]), &sources[1], &sources[2]);
            let marker = buffer
                .materialized_markers(&repo, Path::new("file.txt"))
                .unwrap();
            assert_eq!(marker.marker_width, 13, "source={source}, char={delimiter}");
            assert!(marker.text.contains("<<<<<<<<<<<<< Current\n"));
            assert!(marker.text.contains("\n=============\n"));
            assert!(marker.text.contains("\n>>>>>>>>>>>>> Incoming\n"));
        }
    }
}

#[test]
fn unrepresentable_marker_width_refuses_materialization() {
    if !test_support::run_isolated() {
        return;
    }
    let current = format!("{}\n", "=".repeat(usize::from(u16::MAX)));
    let (_tmp, repo, buffer) = materialize_sources(Some("base\n"), &current, "incoming\n");
    assert!(buffer
        .materialized_markers(&repo, Path::new("file.txt"))
        .is_none());
}

#[test]
fn add_add_width_accounts_for_both_sources_and_all_delimiters() {
    if !test_support::run_isolated() {
        return;
    }
    for source in 0..2 {
        for delimiter in ['<', '=', '>', '|'] {
            let mut sources = ["current\n".to_owned(), "incoming\n".to_owned()];
            sources[source].push_str(&delimiter.to_string().repeat(9));
            sources[source].push('\n');
            let (_tmp, repo, mut buffer) = materialize_sources(None, &sources[0], &sources[1]);
            let path = Path::new("file.txt");
            let marker = buffer.materialized_markers(&repo, path).unwrap();
            assert_eq!(marker.marker_width, 10);
            assert!(buffer.ensure_hunks(path, &marker.text, marker.marker_width));
            assert_eq!(buffer.hunk_count(path), 1);
            assert!(buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptCurrent));
            assert_eq!(buffer.resolved_text(path).unwrap(), sources[0]);
            assert!(buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptIncoming));
            assert_eq!(buffer.resolved_text(path).unwrap(), sources[1]);
        }
    }
}

#[test]
fn only_line_start_runs_increase_the_default_marker_width() {
    if !test_support::run_isolated() {
        return;
    }
    let current = "current <<<<<<<<<< ========== >>>>>>>>>> ||||||||||\n";
    let (_tmp, repo, buffer) = materialize_sources(Some("base\n"), current, "incoming\n");
    let marker = buffer
        .materialized_markers(&repo, Path::new("file.txt"))
        .unwrap();
    assert_eq!(marker.marker_width, 7);
    assert!(marker.text.starts_with("<<<<<<< Current\n"));
    assert!(marker.text.contains("\n||||||| Base\n"));
    assert!(marker.text.contains("\n=======\n"));
    assert!(marker.text.ends_with(">>>>>>> Incoming\n"));
}
