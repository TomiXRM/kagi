use super::*;

fn text_buffer(markers: &str) -> (ResolutionBuffer, PathBuf) {
    let mut buffer = ResolutionBuffer::new(Path::new("/tmp/repo"));
    let path = PathBuf::from("file.txt");
    buffer.files.insert(path.clone(), FileResolution::empty());
    assert!(buffer.ensure_hunks(&path, markers, 7));
    (buffer, path)
}

#[test]
fn manual_override_tracks_actual_text_through_undo_and_redo() {
    let (mut buffer, path) =
        text_buffer("context\n<<<<<<< Current\ncurrent\n=======\nincoming\n>>>>>>> Incoming\n");
    assert!(!buffer.result_differs_from_hunks(&path));
    assert!(buffer.set_hunk_side_selection(&path, 0, SelectionSide::Current, true));
    assert!(!buffer.result_differs_from_hunks(&path));

    buffer.set_manual_text(&path, "context\ncurrent\n").unwrap();
    assert!(!buffer.result_differs_from_hunks(&path));
    buffer.set_manual_text(&path, "context\ncustom\n").unwrap();
    assert!(buffer.result_differs_from_hunks(&path));
    assert_eq!(
        buffer.resolved_text(&path).as_deref(),
        Some("context\ncustom\n")
    );
    assert!(buffer.undo(&path));
    assert!(!buffer.result_differs_from_hunks(&path));
    assert!(buffer.redo(&path));
    assert!(buffer.result_differs_from_hunks(&path));

    buffer.set_manual_text(&path, "").unwrap();
    assert!(buffer.result_differs_from_hunks(&path));
    assert_eq!(buffer.resolved_text(&path).as_deref(), Some(""));
}

#[test]
fn manual_empty_is_not_an_unresolved_or_chosen_empty_hunk() {
    let (mut buffer, path) = text_buffer("<<<<<<< Current\n=======\nincoming\n>>>>>>> Incoming\n");
    buffer.set_manual_text(&path, "").unwrap();
    assert!(buffer.result_differs_from_hunks(&path));
    assert!(buffer.undo(&path));
    assert!(!buffer.result_differs_from_hunks(&path));
    assert!(buffer.set_hunk_side_selection(&path, 0, SelectionSide::Current, true));
    assert_eq!(buffer.resolved_text(&path).as_deref(), Some(""));
    assert!(!buffer.result_differs_from_hunks(&path));
}

#[test]
fn manual_override_detects_added_removed_and_changed_context_lines() {
    let (mut buffer, path) =
        text_buffer("context\n<<<<<<< Current\ncurrent\n=======\nincoming\n>>>>>>> Incoming\n");
    assert!(buffer.apply_hunk_choice(&path, 0, HunkChoice::AcceptIncoming));
    for text in [
        "context\n",
        "context\nincoming\nextra\n",
        "changed\nincoming\n",
    ] {
        buffer.set_manual_text(&path, text).unwrap();
        assert!(buffer.result_differs_from_hunks(&path));
        assert_eq!(buffer.resolved_text(&path).as_deref(), Some(text));
    }
}
