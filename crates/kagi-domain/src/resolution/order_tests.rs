use super::*;

const MARKERS: &str =
    "top\n<<<<<<< Current\nc1\nc2\nc3\n=======\ni1\ni2\ni3\n>>>>>>> Incoming\nbottom\n";

#[test]
fn hunk_order_untouched_preserves_markers_and_unresolved_status() {
    let mut model = HunkModel::from_marker_text(MARKERS);
    let before = model.clone();
    for order in [
        LineOrder::IncomingFirst,
        LineOrder::CurrentFirst,
        LineOrder::IncomingBlockFirst,
        LineOrder::CurrentBlockFirst,
    ] {
        assert!(!model.set_hunk_line_order(0, order));
        assert_eq!(model, before);
        assert_eq!(model.assembled_text().as_bytes(), MARKERS.as_bytes());
        assert_eq!(model.resolved_hunk_count(), 0);
        assert!(!model.all_resolved());
        assert!(model.hunks()[0].line_select.is_none());
    }
    assert!(!model.set_hunk_line_order(1, LineOrder::IncomingFirst));
    assert_eq!(model, before);
}

#[test]
fn hunk_order_preserves_one_side_manual_and_intentional_empty_choices() {
    for (choice, expected) in [
        (HunkChoice::AcceptCurrent, "top\nc1\nc2\nc3\nbottom\n"),
        (HunkChoice::AcceptIncoming, "top\ni1\ni2\ni3\nbottom\n"),
        (
            HunkChoice::Manual("edited\n".into()),
            "top\nedited\nbottom\n",
        ),
        (HunkChoice::Manual(String::new()), "top\nbottom\n"),
    ] {
        let mut model = HunkModel::from_marker_text(MARKERS);
        assert!(model.set_choice(0, choice));
        let before = model.clone();
        for order in [
            LineOrder::IncomingFirst,
            LineOrder::CurrentFirst,
            LineOrder::IncomingBlockFirst,
            LineOrder::CurrentBlockFirst,
        ] {
            assert!(!model.set_hunk_line_order(0, order));
            assert_eq!(model, before);
            assert_eq!(model.assembled_text().as_bytes(), expected.as_bytes());
            assert!(model.all_resolved());
        }
    }

    // An explicitly adopted zero-line side is a valid deletion, not unresolved.
    let mut model = HunkModel::from_marker_text("<<<<<<< Current\n=======\ni1\n>>>>>>> Incoming\n");
    assert!(model.set_hunk_side(0, SelectionSide::Current, true));
    let before = model.clone();
    assert!(!model.set_hunk_line_order(0, LineOrder::IncomingFirst));
    assert_eq!(model, before);
    assert_eq!(model.assembled_text().as_bytes(), b"");
    assert!(model.all_resolved());
}

#[test]
fn hunk_order_both_choices_remain_whole_side_groups() {
    let mut model = HunkModel::from_marker_text(MARKERS);
    assert!(model.set_choice(0, HunkChoice::BothCurrentFirst));
    assert!(!model.set_hunk_line_order(0, LineOrder::CurrentFirst));
    assert!(model.set_hunk_line_order(0, LineOrder::IncomingFirst));
    assert_eq!(model.hunks()[0].choice, HunkChoice::BothIncomingFirst);
    assert!(model.hunks()[0].line_select.is_none());
    assert_eq!(
        model.assembled_text().as_bytes(),
        b"top\ni1\ni2\ni3\nc1\nc2\nc3\nbottom\n"
    );
    assert!(model.all_resolved());
    assert!(!model.set_hunk_line_order(0, LineOrder::IncomingFirst));
    assert!(model.set_hunk_line_order(0, LineOrder::CurrentFirst));
    assert_eq!(
        model.assembled_text().as_bytes(),
        b"top\nc1\nc2\nc3\ni1\ni2\ni3\nbottom\n"
    );
}

#[test]
fn hunk_order_partial_selection_only_reorders_selected_position_ties() {
    let mut model = HunkModel::from_marker_text(MARKERS);
    assert!(model.set_hunk_line(0, SelectionSide::Current, 0, true));
    assert!(model.set_hunk_line(0, SelectionSide::Incoming, 2, true));
    let before = model.clone();
    assert!(!model.set_hunk_line_order(0, LineOrder::IncomingFirst));
    assert_eq!(model, before);
    assert_eq!(model.assembled_text().as_bytes(), b"top\nc1\ni3\nbottom\n");

    assert!(model.set_hunk_line(0, SelectionSide::Incoming, 0, true));
    assert!(model.set_hunk_line(0, SelectionSide::Current, 1, true));
    let selection = model.hunks()[0].line_select.clone().unwrap();
    assert!(model.set_hunk_line_order(0, LineOrder::IncomingFirst));
    let reordered = model.hunks()[0].line_select.as_ref().unwrap();
    assert_eq!(reordered.current_taken, selection.current_taken);
    assert_eq!(reordered.incoming_taken, selection.incoming_taken);
    assert_eq!(
        model.assembled_text().as_bytes(),
        b"top\ni1\nc1\nc2\ni3\nbottom\n"
    );
    assert!(model.all_resolved());
    assert!(!model.set_hunk_line_order(0, LineOrder::IncomingFirst));
    assert!(model.set_hunk_line_order(0, LineOrder::CurrentFirst));
    assert_eq!(
        model.assembled_text().as_bytes(),
        b"top\nc1\ni1\nc2\ni3\nbottom\n"
    );
}

#[test]
fn both_current_first_checkbox_edit_preserves_block_order() {
    let mut model = HunkModel::from_marker_text(MARKERS);
    assert!(model.set_choice(0, HunkChoice::BothCurrentFirst));
    assert!(model.set_hunk_line(0, SelectionSide::Incoming, 1, false));
    assert_eq!(model.assembled_text(), "top\nc1\nc2\nc3\ni1\ni3\nbottom\n");
    assert_eq!(
        model.hunks()[0].line_select.as_ref().unwrap().order,
        LineOrder::CurrentBlockFirst
    );
}

#[test]
fn both_incoming_first_checkbox_edit_preserves_block_order() {
    let mut model = HunkModel::from_marker_text(MARKERS);
    assert!(model.set_choice(0, HunkChoice::BothIncomingFirst));
    assert!(model.set_hunk_line(0, SelectionSide::Current, 1, false));
    assert_eq!(model.assembled_text(), "top\ni1\ni2\ni3\nc1\nc3\nbottom\n");
    assert_eq!(
        model.hunks()[0].line_select.as_ref().unwrap().order,
        LineOrder::IncomingBlockFirst
    );
}

#[test]
fn from_scratch_both_sides_still_interleave_by_position() {
    let mut model = HunkModel::from_marker_text(MARKERS);
    for index in 0..3 {
        assert!(model.set_hunk_line(0, SelectionSide::Current, index, true));
        assert!(model.set_hunk_line(0, SelectionSide::Incoming, index, true));
    }
    assert_eq!(
        model.assembled_text(),
        "top\nc1\ni1\nc2\ni2\nc3\ni3\nbottom\n"
    );
    assert_eq!(
        model.hunks()[0].line_select.as_ref().unwrap().order,
        LineOrder::CurrentFirst
    );
}

#[test]
fn block_order_flip_syncs_both_choice_and_preserves_taken_lines() {
    let mut model = HunkModel::from_marker_text(MARKERS);
    assert!(model.set_choice(0, HunkChoice::BothCurrentFirst));
    assert!(model.set_hunk_line(0, SelectionSide::Incoming, 1, false));
    assert!(model.set_hunk_line_order(0, LineOrder::IncomingBlockFirst));
    assert_eq!(model.assembled_text(), "top\ni1\ni3\nc1\nc2\nc3\nbottom\n");
    assert_eq!(model.hunks()[0].choice, HunkChoice::BothIncomingFirst);
    assert!(!model.set_hunk_line_order(0, LineOrder::IncomingBlockFirst));
    assert!(model.set_hunk_line_order(0, LineOrder::CurrentBlockFirst));
    assert_eq!(model.hunks()[0].choice, HunkChoice::BothCurrentFirst);
    assert_eq!(model.assembled_text(), "top\nc1\nc2\nc3\ni1\ni3\nbottom\n");
}

#[test]
fn block_order_applies_without_same_position_ties() {
    let mut model = HunkModel::from_marker_text(MARKERS);
    assert!(model.set_hunk_line(0, SelectionSide::Current, 0, true));
    assert!(model.set_hunk_line(0, SelectionSide::Incoming, 2, true));
    assert!(model.set_hunk_line_order(0, LineOrder::IncomingBlockFirst));
    assert_eq!(model.assembled_text(), "top\ni3\nc1\nbottom\n");
    assert!(model.set_hunk_line_order(0, LineOrder::CurrentBlockFirst));
    assert_eq!(model.assembled_text(), "top\nc1\ni3\nbottom\n");
}
