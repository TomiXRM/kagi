use super::*;

fn hunk(current_len: usize, incoming_len: usize, choice: HunkChoice) -> ConflictHunk {
    ConflictHunk {
        current: (0..current_len).map(|i| format!("c{i}")).collect(),
        incoming: (0..incoming_len).map(|i| format!("i{i}")).collect(),
        base: Vec::new(),
        choice,
        line_select: None,
        marker_width: 7,
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

/// A source opener inside Current must not restart the parser and erase text.
#[test]
fn marker_parser_preserves_a_literal_source_opener() {
    let model = HunkModel::from_marker_text(
        "<<<<<<< Current\n<<<<<<< user\n=======\nincoming\n>>>>>>> Incoming\n",
    );
    assert_eq!(model.hunks()[0].current, vec!["<<<<<<< user"]);
}

#[test]
fn wide_markers_preserve_shorter_source_runs_and_unresolved_width() {
    for width in [10, 32] {
        let current = "<<<<<<< literal\n||||||| literal\n=======\n>>>>>>>> literal\n=========\n";
        let base = "<<<<<<<<< literal\n||||||||| literal\n=========\n>>>>>>>>> literal\n";
        let incoming = "<<<<<<< literal\n||||||| literal\n=======\n>>>>>>> literal\n";
        let markers = format!(
            "before\n{} Current\n{current}{} Base\n{base}{}\n{incoming}{} Incoming\nafter\n",
            "<".repeat(width),
            "|".repeat(width),
            "=".repeat(width),
            ">".repeat(width),
        );
        let mut model = HunkModel::from_marker_text_with_width(&markers, width);
        assert_eq!(model.hunk_count(), 1);
        let hunk = model.hunks()[0];
        assert_eq!(hunk.marker_width, width);
        assert_eq!(
            hunk.current,
            current.split_terminator('\n').collect::<Vec<_>>()
        );
        assert_eq!(hunk.base, base.split_terminator('\n').collect::<Vec<_>>());
        assert_eq!(
            hunk.incoming,
            incoming.split_terminator('\n').collect::<Vec<_>>()
        );
        assert_eq!(model.assembled_text(), markers);
        assert_eq!(
            HunkModel::from_marker_text_with_width(&model.assembled_text(), width),
            model
        );
        assert!(resolved_lines_have_conflict_marker(&model.assemble()));
        assert!(model.set_choice(0, HunkChoice::AcceptCurrent));
        assert_eq!(model.assembled_text(), format!("before\n{current}after\n"));
        assert!(!resolved_lines_have_conflict_marker(&model.assemble()));
        assert!(model.set_choice(0, HunkChoice::AcceptIncoming));
        assert_eq!(model.assembled_text(), format!("before\n{incoming}after\n"));
    }
}

#[test]
fn default_markers_only_transition_in_their_expected_states() {
    let outside = "||||||| outside\n=======\n>>>>>>> outside\n";
    let current = "<<<<<<< literal\n>>>>>>> literal\n";
    let base = "<<<<<<< literal\n||||||| literal\n>>>>>>> literal\n";
    let incoming = "<<<<<<< literal\n||||||| literal\n=======\n";
    let text = format!(
        "{outside}<<<<<<< Current\n{current}||||||| Base\n{base}=======\n{incoming}>>>>>>> Incoming\n{outside}"
    );
    let model = HunkModel::from_marker_text(&text);
    assert_eq!(model.hunk_count(), 1);
    assert_eq!(model.regions.len(), 3);
    let hunk = model.hunks()[0];
    assert_eq!(
        hunk.current,
        current.split_terminator('\n').collect::<Vec<_>>()
    );
    assert_eq!(hunk.base, base.split_terminator('\n').collect::<Vec<_>>());
    assert_eq!(
        hunk.incoming,
        incoming.split_terminator('\n').collect::<Vec<_>>()
    );
    assert_eq!(model.assembled_text(), text);
}

#[test]
fn marker_boundaries_reject_long_runs_and_unseparated_labels() {
    for width in [7, 10] {
        let open = "<".repeat(width);
        let base = "|".repeat(width);
        let separator = "=".repeat(width);
        let close = ">".repeat(width);
        let outside = format!("{open}< Current\n{open}not-a-label\n🦀\n");
        let current = format!("{base}| Base\n{base}not-a-label\n{separator}=\n{separator} label\n");
        let incoming = format!("{close}> Incoming\n{close}not-a-label\n");
        let text = format!("{outside}{open}\n{current}{separator}\n{incoming}{close}\n");
        let model = HunkModel::from_marker_text_with_width(&text, width);
        assert_eq!(model.hunk_count(), 1);
        let hunk = model.hunks()[0];
        assert_eq!(
            hunk.current,
            current.split_terminator('\n').collect::<Vec<_>>()
        );
        assert!(hunk.base.is_empty());
        assert_eq!(
            hunk.incoming,
            incoming.split_terminator('\n').collect::<Vec<_>>()
        );
        let Region::Passthrough(lines) = &model.regions[0] else {
            panic!("invalid openers must remain passthrough");
        };
        assert_eq!(lines, &outside.split_terminator('\n').collect::<Vec<_>>());
    }
}

#[test]
fn truncated_hunks_retain_all_source_text_in_each_phase() {
    for width in [7, 10] {
        let open = "<".repeat(width);
        let base = "|".repeat(width);
        let separator = "=".repeat(width);
        let close = ">".repeat(width);
        let current = format!("current\n{open} literal\n{close} literal");
        let ancestor = format!("base\n{open} literal\n{base} literal\n{close} literal");
        let incoming = format!("incoming\n{open} literal\n{base} literal\n{separator}");
        for (tail, expected_base, expected_incoming) in [
            (String::new(), "", ""),
            (format!("\n{base} Base\n{ancestor}"), ancestor.as_str(), ""),
            (
                format!("\n{base} Base\n{ancestor}\n{separator}\n{incoming}"),
                ancestor.as_str(),
                incoming.as_str(),
            ),
        ] {
            let text = format!("context\n{open} Current\n{current}{tail}");
            let model = HunkModel::from_marker_text_with_width(&text, width);
            assert_eq!(model.hunk_count(), 1);
            assert!(!model.all_resolved());
            let hunk = model.hunks()[0];
            assert_eq!(
                hunk.current,
                current.split_terminator('\n').collect::<Vec<_>>()
            );
            assert_eq!(
                hunk.base,
                expected_base.split_terminator('\n').collect::<Vec<_>>()
            );
            assert_eq!(
                hunk.incoming,
                expected_incoming.split_terminator('\n').collect::<Vec<_>>()
            );
            assert_eq!(
                HunkModel::from_marker_text_with_width(&model.assembled_text(), width),
                model
            );
        }
    }
}

#[test]
fn empty_and_zero_width_input_remain_passthrough() {
    assert!(HunkModel::from_marker_text("").regions.is_empty());
    assert_eq!(HunkModel::from_marker_text("\n").assembled_text(), "\n");
    let text = "<<<<<<< Current\ncurrent\n=======\nincoming\n>>>>>>> Incoming\n";
    let model = HunkModel::from_marker_text_with_width(text, 0);
    assert_eq!(model.hunk_count(), 0);
    assert_eq!(model.assembled_text(), text);
}

#[test]
fn chosen_seven_wide_literal_is_not_residue_but_unresolved_nine_wide_is() {
    let mut chosen = HunkModel::from_marker_text_with_width(
        "<<<<<<<<< Current\n<<<<<<< user\n=========\nincoming\n>>>>>>>>> Incoming\n",
        9,
    );
    assert!(chosen.set_choice(0, HunkChoice::AcceptCurrent));
    assert_eq!(chosen.assembled_text(), "<<<<<<< user\n");
    assert!(!resolved_lines_have_conflict_marker(&chosen.assemble()));
    let unresolved = HunkModel::from_marker_text_with_width(
        "<<<<<<<<< Current\ncurrent\n=========\nincoming\n>>>>>>>>> Incoming\n",
        9,
    );
    assert!(resolved_lines_have_conflict_marker(&unresolved.assemble()));
    for marker in ["<<<<<<<<<", "========= ", ">>>>>>>>> Incoming"] {
        assert!(resolution_text_has_conflict_marker(marker));
    }
    for content in ["<<<<<<", "=========literal", "x>>>>>>>>> label"] {
        assert!(!resolution_text_has_conflict_marker(content));
    }
}
