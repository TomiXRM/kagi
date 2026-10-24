use super::*;

const MARKERS: &str = "before\n<<<<<<< Current\na1\na2\n=======\nb1\nb2\n>>>>>>> Incoming\nafter\n";

fn live(identity: char) -> (ResolutionBuffer, PathBuf) {
    let mut buffer = ResolutionBuffer::new(Path::new("/unused/resume-codec"));
    let path = PathBuf::from("a.txt");
    let mut file = FileResolution::empty();
    file.identity = Some(ConflictRevision::from_fingerprint(
        identity.to_string().repeat(64),
    ));
    buffer.live_context = Some((
        ConflictRevision::from_fingerprint("c".repeat(64)),
        PathBuf::from("/unused/resume-codec/.git"),
    ));
    buffer.files.insert(path.clone(), file);
    (buffer, path)
}

fn decoded(buffer: &ResolutionBuffer) -> ResolutionBuffer {
    parse_buffer_json(buffer.repo_path(), &buffer.to_json().unwrap()).unwrap()
}

#[test]
fn autosave_roundtrips_hunk_choices_and_all_line_orders() {
    let choices = [
        HunkChoice::AcceptCurrent,
        HunkChoice::AcceptIncoming,
        HunkChoice::BothCurrentFirst,
        HunkChoice::BothIncomingFirst,
        HunkChoice::Manual("custom\ntext".into()),
        HunkChoice::Unresolved,
    ];
    for choice in choices {
        let (mut source, path) = live('a');
        assert!(source.ensure_hunks(&path, MARKERS, 7));
        assert!(source.apply_hunk_choice(&path, 0, choice));
        let (mut restored, _) = live('a');
        restored.retain_drafts(decoded(&source));
        assert!(restored.ensure_hunks(&path, MARKERS, 7));
        assert_eq!(restored.hunk_model(&path), source.hunk_model(&path));
        assert_eq!(restored.resolved_text(&path), source.resolved_text(&path));
        assert!(!restored.result_differs_from_hunks(&path));
    }
    for order in [
        LineOrder::CurrentFirst,
        LineOrder::IncomingFirst,
        LineOrder::CurrentBlockFirst,
        LineOrder::IncomingBlockFirst,
    ] {
        let (mut source, path) = live('a');
        source.ensure_hunks(&path, MARKERS, 7);
        source.apply_hunk_choice(&path, 0, HunkChoice::BothCurrentFirst);
        assert!(source.set_hunk_line_selection(&path, 0, SelectionSide::Current, 0, false));
        source.set_hunk_line_order(&path, 0, order);
        let (mut restored, _) = live('a');
        // Existing fresh materialization must not mask the saved decisions.
        restored.ensure_hunks(&path, MARKERS, 7);
        restored.retain_drafts(decoded(&source));
        restored.ensure_hunks(&path, MARKERS, 7);
        assert_eq!(restored.hunk_model(&path), source.hunk_model(&path));
        assert_eq!(restored.resolved_text(&path), source.resolved_text(&path));
        assert!(!restored.result_differs_from_hunks(&path));
        assert!(restored.set_hunk_line_selection(&path, 0, SelectionSide::Incoming, 1, false));
        assert!(source.set_hunk_line_selection(&path, 0, SelectionSide::Incoming, 1, false));
        assert_eq!(restored.resolved_text(&path), source.resolved_text(&path));
    }
}

#[test]
fn autosave_hunk_state_requires_live_identity_and_matching_materialization() {
    let (mut source, path) = live('a');
    source.ensure_hunks(&path, MARKERS, 7);
    source.apply_hunk_choice(&path, 0, HunkChoice::BothIncomingFirst);
    let mut unbound = decoded(&source);
    unbound.ensure_hunks(&path, MARKERS, 7);
    assert!(!unbound.hunks_all_resolved(&path));
    let (mut different, _) = live('b');
    different.retain_drafts(decoded(&source));
    different.ensure_hunks(&path, MARKERS, 7);
    assert!(!different.hunks_all_resolved(&path));
    assert!(!different.has_resolution(&path));
    let (mut changed, _) = live('a');
    changed.retain_drafts(decoded(&source));
    changed.ensure_hunks(&path, &MARKERS.replace("a1", "different"), 7);
    assert!(!changed.hunks_all_resolved(&path));
    assert!(!changed.has_resolution(&path));
    let recovery: Vec<_> = changed.recovery_drafts().collect();
    assert_eq!(recovery.len(), 1);
    assert_eq!(recovery[0].path, path);
    assert_eq!(
        lines_to_text(recovery[0].result.unwrap()),
        source.resolved_text(&path).unwrap()
    );
}

#[test]
fn autosave_legacy_result_without_hunk_state_still_loads() {
    let json = r#"{"repo":"old","updated":1,"files":[{"path":"a.txt","binary":false,"current":"a\n","incoming":"b\n","result":[{"t":"saved","o":"m"}],"raw_result":null}]}"#;
    let buffer = parse_buffer_json(Path::new("/unused/legacy"), json).unwrap();
    assert_eq!(
        buffer.resolved_text(Path::new("a.txt")).as_deref(),
        Some("saved\n")
    );
    assert!(buffer.files[Path::new("a.txt")].hunk_state.is_none());
    let again = decoded(&buffer);
    assert_eq!(
        again.resolved_text(Path::new("a.txt")),
        buffer.resolved_text(Path::new("a.txt"))
    );
}
