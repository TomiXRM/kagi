use super::*;

fn hunk(current_len: usize, incoming_len: usize, choice: HunkChoice) -> ConflictHunk {
    ConflictHunk {
        current: (0..current_len).map(|i| format!("c{i}")).collect(),
        incoming: (0..incoming_len).map(|i| format!("i{i}")).collect(),
        base: Vec::new(),
        choice,
        line_select: None,
    }
}

#[test]
fn empty_side_state_survives_choice_materialization() {
    for current_len in 0..=3 {
        for incoming_len in 0..=3 {
            for choice in [
                HunkChoice::Unresolved,
                HunkChoice::Manual(String::new()),
                HunkChoice::AcceptCurrent,
                HunkChoice::AcceptIncoming,
                HunkChoice::BothCurrentFirst,
                HunkChoice::BothIncomingFirst,
            ] {
                let mut hunk = hunk(current_len, incoming_len, choice);
                let before = [
                    hunk.side_state(SelectionSide::Current),
                    hunk.side_state(SelectionSide::Incoming),
                ];
                hunk.ensure_line_selection();
                assert_eq!(
                    before,
                    [
                        hunk.side_state(SelectionSide::Current),
                        hunk.side_state(SelectionSide::Incoming)
                    ],
                    "lengths={current_len},{incoming_len}; choice={:?}",
                    hunk.choice
                );
            }
        }
    }
}

#[test]
fn marker_gate_respects_chosen_source_provenance() {
    let mut hunk = hunk(1, 1, HunkChoice::Unresolved);
    hunk.current[0] = "<<<<<<< user".into();
    hunk.incoming[0] = ">>>>>>> user".into();
    assert!(resolved_lines_have_conflict_marker(&hunk.resolved_lines()));
    for choice in [
        HunkChoice::AcceptCurrent,
        HunkChoice::AcceptIncoming,
        HunkChoice::BothCurrentFirst,
    ] {
        hunk.choice = choice;
        assert!(
            !resolved_lines_have_conflict_marker(&hunk.resolved_lines()),
            "chosen source text is not marker residue"
        );
    }
    hunk.choice = HunkChoice::Manual("<<<<<<< user".into());
    assert!(resolved_lines_have_conflict_marker(&hunk.resolved_lines()));
    let context = [ResolvedLine {
        text: "=======".into(),
        origin: LineOrigin::Context,
    }];
    assert!(!resolved_lines_have_conflict_marker(&context));
}

#[test]
fn invalid_hunk_line_edit_is_a_complete_noop() {
    for current_len in 0..=3 {
        for incoming_len in 0..=3 {
            for (side, len) in [
                (SelectionSide::Current, current_len),
                (SelectionSide::Incoming, incoming_len),
            ] {
                for taken in [false, true] {
                    let original = hunk(current_len, incoming_len, HunkChoice::Unresolved);
                    let mut model = HunkModel {
                        regions: vec![Region::Hunk(original.clone())],
                    };
                    assert!(!model.set_hunk_line(0, side, len, taken));
                    assert_eq!(*model.hunks()[0], original);
                    assert!(model.hunks()[0].line_select.is_none());
                    assert!(!model.hunks()[0].is_resolved());
                }
            }
        }
    }
}

#[test]
fn stale_selection_dimensions_never_panic_or_emit_missing_lines() {
    for current_len in 0..=3 {
        for incoming_len in 0..=3 {
            for current_mask in 0..=4 {
                for incoming_mask in 0..=4 {
                    for order in [
                        LineOrder::CurrentFirst,
                        LineOrder::IncomingFirst,
                        LineOrder::CurrentBlockFirst,
                        LineOrder::IncomingBlockFirst,
                    ] {
                        let mut hunk = hunk(current_len, incoming_len, HunkChoice::Unresolved);
                        let mut selection = LineSelection::from_choice(
                            current_mask,
                            incoming_mask,
                            &HunkChoice::BothCurrentFirst,
                        );
                        selection.order = order;
                        hunk.line_select = Some(selection);
                        let lines = hunk.resolved_lines();
                        assert_eq!(
                            lines.len(),
                            current_len.min(current_mask) + incoming_len.min(incoming_mask)
                        );
                        assert!(lines.iter().all(|line| match line.origin {
                            LineOrigin::Current => hunk.current.contains(&line.text),
                            LineOrigin::Incoming => hunk.incoming.contains(&line.text),
                            _ => false,
                        }));
                    }
                }
            }
        }
    }
}

#[test]
fn selection_constructor_rejects_mismatched_dimensions() {
    for source_len in 0..=3 {
        for mask_len in 0..=4 {
            for side in [SelectionSide::Current, SelectionSide::Incoming] {
                let current = vec![
                    false;
                    if side == SelectionSide::Current {
                        mask_len
                    } else {
                        source_len
                    }
                ];
                let incoming = vec![
                    false;
                    if side == SelectionSide::Incoming {
                        mask_len
                    } else {
                        source_len
                    }
                ];
                assert_eq!(
                    LineSelection::new(
                        source_len,
                        source_len,
                        current,
                        incoming,
                        LineOrder::CurrentFirst,
                        [false; 2]
                    )
                    .is_ok(),
                    source_len == mask_len,
                );
            }
        }
    }
}

#[test]
fn deselecting_other_side_preserves_an_adopted_deletion() {
    let mut model = HunkModel {
        regions: vec![Region::Hunk(hunk(0, 1, HunkChoice::AcceptCurrent))],
    };
    assert!(model.set_hunk_side(0, SelectionSide::Incoming, true));
    assert!(model.set_hunk_line(0, SelectionSide::Incoming, 0, false));
    assert!(model.all_resolved());
    assert_eq!(
        model.hunk_side_state(0, SelectionSide::Current),
        Some(TriState::All)
    );
    assert_eq!(model.assembled_text(), "");
    assert!(model.set_hunk_side(0, SelectionSide::Current, false));
    assert!(!model.all_resolved());
}
