use super::*;

const MARKERS: &str =
    "top\n<<<<<<< Current\nc1\nc2\nc3\n=======\ni1\ni2\ni3\n>>>>>>> Incoming\nbottom\n";

fn buffer() -> (ResolutionBuffer, PathBuf) {
    let mut buffer = ResolutionBuffer::new(Path::new("/tmp/repo"));
    let path = PathBuf::from("file.txt");
    buffer.files.insert(path.clone(), FileResolution::empty());
    assert!(buffer.ensure_hunks(&path, MARKERS, 7));
    (buffer, path)
}

#[test]
fn buffer_hunk_order_untouched_is_not_an_edit_or_resolution() {
    let (mut buffer, path) = buffer();
    for order in [LineOrder::IncomingFirst, LineOrder::CurrentFirst] {
        assert!(!buffer.set_hunk_line_order(&path, 0, order));
        assert!(!buffer.has_resolution(&path));
        assert_eq!(buffer.resolved_text(&path), None);
        assert!(!buffer.hunks_all_resolved(&path));
        let model = buffer.hunk_model(&path).unwrap();
        assert_eq!(model.assembled_text().as_bytes(), MARKERS.as_bytes());
        assert_eq!(model.resolved_hunk_count(), 0);
        let file = &buffer.files[&path];
        assert!(file.undo.is_empty());
        assert!(file.redo.is_empty());
        assert!(!file.draft_state);
        assert!(!file.pending_edit.is_pending());
        assert!(file.editing_origin.is_none());
    }
    assert!(!buffer.undo(&path));
    assert!(!buffer.redo(&path));
}

#[test]
fn buffer_hunk_order_both_has_one_history_entry_per_changed_order() {
    let (mut buffer, path) = buffer();
    assert!(buffer.apply_hunk_choice(&path, 0, HunkChoice::BothCurrentFirst));
    let current_first = b"top\nc1\nc2\nc3\ni1\ni2\ni3\nbottom\n";
    let incoming_first = b"top\ni1\ni2\ni3\nc1\nc2\nc3\nbottom\n";
    assert_eq!(
        buffer.resolved_text(&path).unwrap().as_bytes(),
        current_first
    );
    assert!(buffer.set_hunk_line_order(&path, 0, LineOrder::IncomingFirst));
    assert_eq!(
        buffer.resolved_text(&path).unwrap().as_bytes(),
        incoming_first
    );
    assert!(buffer.has_resolution(&path));
    assert!(buffer.hunks_all_resolved(&path));
    assert_eq!(buffer.files[&path].undo.len(), 2);

    buffer.files[&path].pending_edit.mark_saved();
    assert!(!buffer.set_hunk_line_order(&path, 0, LineOrder::IncomingFirst));
    assert!(!buffer.files[&path].pending_edit.is_pending());
    assert_eq!(buffer.files[&path].undo.len(), 2);
    assert!(buffer.undo(&path));
    assert_eq!(
        buffer.resolved_text(&path).unwrap().as_bytes(),
        current_first
    );
    assert_eq!(buffer.files[&path].redo.len(), 1);
    // The unchanged command must not checkpoint, clear redo or dirty the draft.
    buffer.files[&path].pending_edit.mark_saved();
    assert!(!buffer.set_hunk_line_order(&path, 0, LineOrder::IncomingFirst));
    assert!(!buffer.files[&path].pending_edit.is_pending());
    assert_eq!(buffer.files[&path].redo.len(), 1);
    assert!(buffer.redo(&path));
    assert_eq!(
        buffer.resolved_text(&path).unwrap().as_bytes(),
        incoming_first
    );
    assert!(buffer.undo(&path));
    assert!(buffer.undo(&path));
    assert_eq!(buffer.resolved_text(&path), None);
    assert!(!buffer.has_resolution(&path));
    assert!(!buffer.undo(&path));
}

#[test]
fn buffer_hunk_order_partial_preserves_selection_and_undo_bytes() {
    let (mut buffer, path) = buffer();
    assert!(buffer.set_hunk_line_selection(&path, 0, SelectionSide::Current, 0, true));
    assert!(buffer.set_hunk_line_selection(&path, 0, SelectionSide::Incoming, 0, true));
    assert!(buffer.set_hunk_line_selection(&path, 0, SelectionSide::Incoming, 2, true));
    assert_eq!(
        buffer.resolved_text(&path).unwrap().as_bytes(),
        b"top\nc1\ni1\ni3\nbottom\n"
    );
    assert!(buffer.set_hunk_line_order(&path, 0, LineOrder::IncomingFirst));
    assert_eq!(
        buffer.resolved_text(&path).unwrap().as_bytes(),
        b"top\ni1\nc1\ni3\nbottom\n"
    );
    assert_eq!(
        buffer.provenance(&path).unwrap(),
        vec![
            LineOrigin::Context,
            LineOrigin::Incoming,
            LineOrigin::Current,
            LineOrigin::Incoming,
            LineOrigin::Context,
        ]
    );
    assert!(buffer.hunks_all_resolved(&path));
    assert!(buffer.undo(&path));
    assert_eq!(
        buffer.resolved_text(&path).unwrap().as_bytes(),
        b"top\nc1\ni1\ni3\nbottom\n"
    );
}

#[test]
fn buffer_hunk_order_inapplicable_preserves_chosen_result_and_history() {
    for choice in [
        HunkChoice::AcceptCurrent,
        HunkChoice::AcceptIncoming,
        HunkChoice::Manual("edited\n".into()),
        HunkChoice::Manual(String::new()),
    ] {
        let (mut buffer, path) = buffer();
        assert!(buffer.apply_hunk_choice(&path, 0, choice));
        let before = buffer.resolved_text(&path);
        buffer.files[&path].pending_edit.mark_saved();
        assert!(!buffer.set_hunk_line_order(&path, 0, LineOrder::IncomingFirst));
        assert!(!buffer.files[&path].pending_edit.is_pending());
        assert_eq!(buffer.resolved_text(&path), before);
        assert!(buffer.has_resolution(&path));
        assert!(buffer.hunks_all_resolved(&path));
        assert_eq!(buffer.files[&path].undo.len(), 1);
        assert!(buffer.undo(&path));
        assert!(!buffer.has_resolution(&path));
        assert!(!buffer.undo(&path));
    }
}
