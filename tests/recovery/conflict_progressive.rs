//! Progressive add/add decisions and accepted Saved inspection.
use super::*;
use kagi::ui::conflict_editor_a11y as ax;
fn progressive_add_add_leg(cx: &mut VisualTestAppContext, lang: Lang, zoom: f32) {
    let comment_setting = if lang == Lang::En { ";" } else { "auto" };
    progressive_add_add_with_comments(cx, lang, zoom, comment_setting);
    if lang == Lang::En && zoom == 1. {
        progressive_add_add_with_comments(cx, lang, zoom, "#");
    }
}

fn progressive_add_add_with_comments(
    cx: &mut VisualTestAppContext,
    lang: Lang,
    zoom: f32,
    comment_setting: &str,
) {
    use kagi::ui::conflict_view::ConflictPage;
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "feature"]);
    std::fs::write(
        repo.join("added.txt"),
        "incoming one\nincoming two\nincoming three\n",
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "incoming"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(
        repo.join("added.txt"),
        "current one\ncurrent two\ncurrent three\n",
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "current"]);
    assert!(!std::process::Command::new("git")
        .args(["merge", "feature"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    // Keep default '#' coverage alongside configured and Git-selected comments.
    if comment_setting != "#" {
        git(repo, &["config", "core.commentChar", comment_setting]);
    }
    let comment_char = if comment_setting == "#" { '#' } else { ';' };
    let merge_template = format!(
        "Merge feature\n\n# User heading\nKeep this body; punctuation.\n\n{comment_char} Conflicts:\n{comment_char}\tadded.txt\n"
    );
    let expected_template = if comment_char == '#' {
        "Merge feature\n\nKeep this body; punctuation."
    } else {
        "Merge feature\n\n# User heading\nKeep this body; punctuation."
    };
    std::fs::write(repo.join(".git/MERGE_MSG"), merge_template).unwrap();
    let before = repo_fingerprint(repo);
    let index_before = std::fs::read(repo.join(".git/index")).unwrap();
    let head = git2::Repository::open(repo)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();
    let (app, window) = mount(cx, repo);
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    assert_eq!(cx.read(|cx| conflict.read(cx).page), ConflictPage::Hunk);
    let window_id = window.window_id();
    e2e::clear_control_bounds(window_id, "resolver-current-viewport");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        e2e::control_bounds(window_id, "resolver-current-viewport").is_some(),
        "the first file opens directly into its source decision"
    );
    assert_eq!(
        kagi::ui::conflict_view::continue_primary_for_e2e(),
        Some(false)
    );
    click_control(cx, window, "conflict-row-0");
    cx.read(|cx| {
        let view = conflict.read(cx);
        let mode = view.mode.as_ref().unwrap();
        assert_eq!(view.page, ConflictPage::Hunk);
        assert_eq!(mode.session.files[0].kind, kagi_git::ConflictKind::AddAdd);
        assert_eq!(mode.buffer.hunk_count(Path::new("added.txt")), 1);
    });
    e2e::clear_control_bounds(window_id, "conflict-row-0");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        e2e::control_bounds(window_id, "conflict-row-0").is_some(),
        "the file tree persists beside the decision page"
    );
    assert!(e2e::control_bounds(window_id, "resolver-current-viewport").is_some());
    let before_both = cx.read(|cx| {
        conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .resolved_text(Path::new("added.txt"))
    });
    for side in ["current", "incoming"] {
        for action in ["use", "both"] {
            assert!(
                e2e::control_bounds(window_id, &format!("conflict-hunk-0-{side}-{action}"))
                    .is_some(),
                "four per-pane adoption commands: {side}-{action}"
            );
        }
    }
    click_control(cx, window, "conflict-hunk-0-current-both");
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    for side in ["current", "incoming"] {
        assert_eq!(
            ax::control(&format!("conflict-hunk-0-{side}-state"))
                .unwrap()
                .1,
            i18n::Msg::ConflictUsed.t()
        );
    }
    cx.read(|cx| {
        let mode = conflict.read(cx).mode.as_ref().unwrap();
        assert_eq!(
            mode.buffer.resolved_text(Path::new("added.txt")).as_deref(),
            Some("current one\ncurrent two\ncurrent three\nincoming one\nincoming two\nincoming three\n")
        );
        assert_eq!(
            conflict.read(cx).editor_inputs.as_ref().unwrap().result.read(cx).value().as_str(),
            "current one\ncurrent two\ncurrent three\nincoming one\nincoming two\nincoming three\n",
            "the drawn Result input shows the action row's block order"
        );
    });
    conflict.update(cx, |view, cx| {
        view.conflict_choose_hunk(
            Path::new("added.txt"),
            0,
            kagi_git::resolution::HunkChoice::BothCurrentFirst,
            cx,
        );
    });
    cx.run_until_parked();
    conflict.update(cx, |view, cx| {
        assert!(view
            .mode
            .as_mut()
            .unwrap()
            .buffer
            .undo(Path::new("added.txt")));
        cx.notify();
    });
    cx.read(|cx| {
        let mode = conflict.read(cx).mode.as_ref().unwrap();
        assert_eq!(
            mode.buffer.resolved_text(Path::new("added.txt")),
            before_both,
            "Both is one reversible choice and repeated Both adds no undo checkpoint"
        );
    });
    // Result undo can diverge from the source model. Only confirmed Reset all
    // may discard that Result before selecting the opposite block order.
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    click_control(cx, window, "conflict-reset-all");
    cx.read(|cx| {
        assert_eq!(
            conflict
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .resolved_text(Path::new("added.txt")),
            before_both,
            "first Reset all only arms confirmation"
        );
    });
    click_control(cx, window, "conflict-reset-all");
    click_control(cx, window, "conflict-hunk-0-incoming-both");
    cx.run_until_parked();
    cx.read(|cx| {
        let mode = conflict.read(cx).mode.as_ref().unwrap();
        assert_eq!(
            mode.buffer.resolved_text(Path::new("added.txt")).as_deref(),
            Some("incoming one\nincoming two\nincoming three\ncurrent one\ncurrent two\ncurrent three\n"),
            "Use Both (Right First) adopts both sides in the requested order"
        );
    });
    click_control(cx, window, "resolver-hunk-0-current-line-1");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_eq!(
        ax::control("conflict-hunk-0-current-state").unwrap().1,
        i18n::Msg::ConflictPartlyUsed.t()
    );
    cx.read(|cx| {
        let view = conflict.read(cx);
        let expected = "incoming one\nincoming two\nincoming three\ncurrent one\ncurrent three\n";
        assert_eq!(
            view.mode
                .as_ref()
                .unwrap()
                .buffer
                .resolved_text(Path::new("added.txt"))
                .as_deref(),
            Some(expected),
            "unchecking a current line retains incoming-first block order"
        );
        assert_eq!(
            view.editor_inputs
                .as_ref()
                .unwrap()
                .result
                .read(cx)
                .value()
                .as_str(),
            expected
        );
    });
    click_control(cx, window, "conflict-hunk-0-incoming-change");
    click_control(cx, window, "conflict-hunk-0-current-both");
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(
            view.editor_inputs
                .as_ref()
                .unwrap()
                .result
                .read(cx)
                .value()
                .as_str(),
            "current one\ncurrent two\ncurrent three\nincoming one\nincoming two\nincoming three\n"
        );
    });
    click_control(cx, window, "resolver-hunk-0-incoming-line-1");
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.read(|cx| {
        let view = conflict.read(cx);
        let expected = "current one\ncurrent two\ncurrent three\nincoming one\nincoming three\n";
        assert_eq!(
            view.mode
                .as_ref()
                .unwrap()
                .buffer
                .resolved_text(Path::new("added.txt"))
                .as_deref(),
            Some(expected),
            "unchecking one right-side line keeps the remaining block order"
        );
        assert_eq!(
            view.editor_inputs
                .as_ref()
                .unwrap()
                .result
                .read(cx)
                .value()
                .as_str(),
            expected,
            "the drawn Result must not interleave after a checkbox edit"
        );
    });
    assert_eq!(
        ax::control("conflict-hunk-0-incoming-state").unwrap().1,
        i18n::Msg::ConflictPartlyUsed.t()
    );
    assert_eq!(
        ax::control("conflict-hunk-0-current-state").unwrap().1,
        i18n::Msg::ConflictUsed.t()
    );
    conflict.update(cx, |view, cx| {
        view.conflict_editor_reset_all(Path::new("added.txt"));
        cx.notify();
    });
    click_control(cx, window, "resolver-hunk-0-incoming-line-0");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_eq!(
        ax::control("conflict-hunk-0-current-state").unwrap().1,
        i18n::Msg::ConflictNotUsed.t()
    );
    assert_eq!(
        ax::control("conflict-hunk-0-incoming-state").unwrap().1,
        i18n::Msg::ConflictPartlyUsed.t(),
        "one incoming line is partly used, never fully adopted"
    );
    cx.read(|cx| {
        let mode = conflict.read(cx).mode.as_ref().unwrap();
        assert_eq!(
            mode.buffer.resolved_text(Path::new("added.txt")).as_deref(),
            Some("incoming one\n")
        );
        assert_eq!(
            mode.buffer
                .hunk_model(Path::new("added.txt"))
                .unwrap()
                .file_side_state(SelectionSide::Incoming),
            kagi_git::resolution::TriState::Partial
        );
    });
    click_control(cx, window, "conflict-result-editor");
    assert!(
        e2e::control_bounds(window_id, "resolver-current-viewport").is_some(),
        "direct Result editing retains source comparison"
    );
    let input = cx.read(|cx| {
        conflict
            .read(cx)
            .editor_inputs
            .as_ref()
            .unwrap()
            .result
            .clone()
    });
    cx.simulate_keystrokes(window, "cmd-a backspace");
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let change = e2e::control_bounds(window_id, "conflict-hunk-0-current-change")
        .expect("manual Result retains an explicit Change action");
    assert_eq!(
        ax::control("conflict-hunk-0-current-change").unwrap().1,
        i18n::Msg::ConflictChangeAccessible.t()
    );
    // Inspecting and dismissing the action must not explicitly reset the draft.
    cx.simulate_mouse_move(window, change.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    crate::keyboard_nav::keys(cx, window, "escape");
    // Even a direct mutation request must refuse to replace divergent manual text.
    conflict.update(cx, |view, cx| {
        view.conflict_choose_hunk(
            Path::new("added.txt"),
            0,
            kagi_git::resolution::HunkChoice::AcceptCurrent,
            cx,
        );
        cx.notify();
    });
    cx.read(|cx| {
        let mode = conflict.read(cx).mode.as_ref().unwrap();
        assert_eq!(
            mode.buffer.resolved_text(Path::new("added.txt")).as_deref(),
            Some(""),
            "an empty manual Result is not an unresolved hunk or an invitation to replace it"
        );
        assert!(!mode.buffer.cached_marker_residue(Path::new("added.txt")));
    });
    click_control(cx, window, "conflict-row-0");
    assert_eq!(
        kagi::ui::conflict_view::continue_primary_for_e2e(),
        Some(false),
        "a ready but unsaved draft does not promote Continue"
    );
    crate::conflict_controls::assert_prominence(1);
    assert_eq!(
        repo_fingerprint(repo),
        before,
        "all page changes and draft choices leave Git unchanged"
    );
    assert_eq!(
        std::fs::read(repo.join(".git/index")).unwrap(),
        index_before,
        "draft adoption and Change preserve index bytes"
    );
    assert_eq!(
        git2::Repository::open(repo)
            .unwrap()
            .head()
            .unwrap()
            .target(),
        Some(head),
        "draft adoption and Change preserve HEAD"
    );
    click_control(cx, window, "conflict-save");
    wait_idle(cx, &app);
    crate::busy_dismiss::ready(cx, &app);
    cx.run_until_parked();
    let repository = git2::Repository::open(repo).unwrap();
    assert!(!repository.index().unwrap().has_conflicts());
    let entry = repository
        .index()
        .unwrap()
        .get_path(Path::new("added.txt"), 0)
        .unwrap();
    assert!(repository.find_blob(entry.id).unwrap().content().is_empty());
    assert_eq!(std::fs::read(repo.join("added.txt")).unwrap(), b"");
    assert_eq!(
        repository.head().unwrap().target(),
        Some(head),
        "Save never commits"
    );
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(view.selected_saved.as_deref(), Some(Path::new("added.txt")));
        assert!(view.editing.is_none(), "saved inspection is read-only");
        assert!(
            view.editor_inputs.is_none(),
            "saved inspection mounts no editor"
        );
        assert!(!view.result_editing);
        let preview = view.saved_preview.as_ref().expect("saved Result bytes");
        assert_eq!(preview.path, Path::new("added.txt"));
        assert_eq!(preview.oid, entry.id.to_string());
        assert_eq!(
            preview.bytes.as_ref(),
            repository.find_blob(entry.id).unwrap().content()
        );
        assert!(
            view.mode.as_ref().unwrap().session.files.is_empty(),
            "fresh index observation is staged, not a draft flag"
        );
    });
    for id in [
        "conflict-save",
        "conflict-result-editor",
        "resolver-current-viewport",
    ] {
        e2e::clear_control_bounds(window_id, id);
    }
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(e2e::control_bounds(window_id, "conflict-saved-row-0").is_some());
    assert!(e2e::control_bounds(window_id, "conflict-saved-preview").is_some());
    for id in [
        "conflict-save",
        "conflict-result-editor",
        "resolver-current-viewport",
    ] {
        assert!(
            e2e::control_bounds(window_id, id).is_none(),
            "saved Result is read-only: {id}"
        );
    }
    assert_eq!(
        kagi::ui::conflict_view::continue_primary_for_e2e(),
        Some(true)
    );
    let saved_before = repo_fingerprint(repo);
    click_control(cx, window, "conflict-sidebar-toggle");
    assert!(cx.read(|cx| conflict.read(cx).sidebar_hidden));
    assert_eq!(
        cx.read(|cx| conflict.read(cx).selected_saved.clone())
            .as_deref(),
        Some(Path::new("added.txt"))
    );
    click_control(cx, window, "conflict-sidebar-toggle");
    click_control(cx, window, "conflict-saved-row-0");
    assert_eq!(
        repo_fingerprint(repo),
        saved_before,
        "saved navigation leaves Git unchanged"
    );
    click_control(cx, window, "conflict-continue");
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).ui().commit_panel_open));
    assert!(cx.read(|cx| app.read(cx).ui().conflict_merge_pending));
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.read(|cx| {
        let state = app.read(cx);
        let panel = state.ui().commit_panel.as_ref().unwrap().read(cx);
        assert_eq!(
            panel.effective_commit_message(cx),
            expected_template,
            "{comment_setting}: actual CommitPanel inputs strip generated merge comments only"
        );
    });
    app.update(cx, |app, cx| app.reload(cx));
    wait_idle(cx, &app);
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.ui().commit_panel_open);
        assert!(state.ui().conflict_merge_pending);
        assert_eq!(
            state
                .ui()
                .commit_panel
                .as_ref()
                .unwrap()
                .read(cx)
                .effective_commit_message(cx),
            expected_template,
            "reload preserves the stripped merge template and user body"
        );
        assert!(
            state.ui().conflict.is_none(),
            "an accepted reload must not repaint the saved workspace over CommitPanel"
        );
    });
    assert_eq!(
        repository.head().unwrap().target(),
        Some(head),
        "Continue opens merge commit review, not a commit"
    );
    assert!(repo.join(".git/MERGE_HEAD").exists());
    let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
    let title_input = cx.read(|cx| panel.read(cx).title_input.clone().unwrap());
    let body_input = cx.read(|cx| panel.read(cx).body_input.clone().unwrap());
    let edited_title = if comment_char == '#' {
        "#1738 の修正"
    } else {
        "; note"
    };
    let edited_body = format!(
        "# Edited heading\nKeep edited body; punctuation.\n{comment_char} Editor-only comment"
    );
    cx.update_window(window, |_, window, cx| {
        title_input.update(cx, |input, cx| input.set_value(edited_title, window, cx));
        body_input.update(cx, |input, cx| input.set_value(edited_body, window, cx));
    })
    .unwrap();
    let expected_message = if comment_char == '#' {
        format!("{edited_title}\n\nKeep edited body; punctuation.")
    } else {
        format!("{edited_title}\n\n# Edited heading\nKeep edited body; punctuation.")
    };
    let displayed_message = cx.read(|cx| panel.read(cx).effective_commit_message(cx));
    // Keep the accepted staged presentation while an external process changes the
    // index, then use the ordinary smooth-commit click without yielding to watchers.
    for remove in [false, true] {
        let backend = kagi_git::Backend::open(repo).unwrap();
        let approved = cx.read(|cx| panel.read(cx).state.displayed_index_digest.clone());
        assert!(
            approved.is_some(),
            "staged rows carry their accepted index identity"
        );
        if remove {
            git(repo, &["rm", "--cached", "external.txt"]);
        } else {
            std::fs::write(repo.join("external.txt"), "not yet approved\n").unwrap();
            git(repo, &["add", "external.txt"]);
        }
        let index = std::fs::read(repo.join(".git/index")).unwrap();
        let merge_head = std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap();
        let merge_msg = std::fs::read(repo.join(".git/MERGE_MSG")).unwrap();
        let fresh = backend.plan_merge_commit(&expected_message).unwrap();
        assert_ne!(approved, fresh.approved_index_digest);
        let prior_toast_receipt = cx.read(|cx| {
            app.read(cx).toast_stack.as_ref().and_then(|stack| {
                stack
                    .read(cx)
                    .toasts()
                    .last()
                    .and_then(|toast| match &toast.action {
                        Some(kagi::ui::ToastAction::OpenOperationLog(key)) => Some(key.clone()),
                        _ => None,
                    })
            })
        });
        app.update(cx, |app, cx| {
            let owner = app.active_session().expect("merge commit owner");
            app.open_commit_plan_modal(owner, cx);
        });
        assert_eq!(repository.head().unwrap().target(), Some(head));
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(
            std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
            merge_head
        );
        assert_eq!(
            std::fs::read(repo.join(".git/MERGE_MSG")).unwrap(),
            merge_msg
        );
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(state.ui().conflict_merge_pending);
            assert!(state.ui().commit_panel_open);
            // ADR-0196 sends unattended results to a toast and clears the footer.
            // The native harness need not own the foreground window; require this
            // refusal's notice, not an attended-only presentation variant.
            let refusal_toast = state.toast_stack.as_ref().is_some_and(|stack| {
                stack.read(cx).toasts().iter().any(|toast| {
                    matches!(&toast.action,
                        Some(kagi::ui::ToastAction::OpenOperationLog(key))
                            if Some(key) != prior_toast_receipt.as_ref())
                        && matches!(toast.kind, kagi::ui::ToastKind::Error)
                        && toast
                            .result_key
                            .as_deref()
                            .is_some_and(|key| key.contains("\0merge-commit\0"))
                        && !toast.message.is_empty()
                })
            });
            assert!(
                matches!(state.status_footer, kagi::ui::FooterStatus::Failed(_)) || refusal_toast,
                "this refusal must reach the current footer or a new merge-commit error toast"
            );
            let panel = state.ui().commit_panel.as_ref().unwrap().read(cx);
            assert!(
                panel.state.plan_modal.is_none(),
                "no new confirmation modal"
            );
            assert_eq!(
                panel.state.displayed_index_digest,
                fresh.approved_index_digest
            );
            assert_eq!(
                panel
                    .state
                    .staged
                    .iter()
                    .any(|file| file.path == Path::new("external.txt")),
                !remove,
                "the refused click re-presents the current staged rows"
            );
            assert_eq!(panel.effective_commit_message(cx), displayed_message);
        });
        let receipts = kagi_git::oplog::read_oplog_tail_for_repo(repo, 1);
        assert!(matches!(
            receipts.last().map(|entry| &entry.outcome),
            Some(kagi_git::oplog::OpOutcome::Refused { .. })
        ));
    }
    let mut approved_index = repository.index().unwrap();
    approved_index.read(true).unwrap();
    let approved_tree = approved_index.write_tree().unwrap();
    // Only another ordinary Commit click may execute the re-presented tree.
    app.update(cx, |app, cx| {
        let owner = app.active_session().expect("merge commit owner");
        app.open_commit_plan_modal(owner, cx);
    });
    wait_idle(cx, &app);
    cx.run_until_parked();
    let commit = repository.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(commit.parent_count(), 2, "the actual merge-commit path ran");
    assert_eq!(commit.parent_id(0).unwrap(), head);
    assert_eq!(
        commit.tree_id(),
        approved_tree,
        "merge tree is the re-presented tree"
    );
    assert_eq!(
        commit.summary().unwrap(),
        Some(edited_title),
        "{comment_setting}: a typed subject is never a template comment"
    );
    assert_eq!(
        commit.message().unwrap().trim_end(),
        expected_message,
        "{comment_setting}: created commit strips only the original effective comment character"
    );
    assert!(!repo.join(".git/MERGE_HEAD").exists());
    drop(input);
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_conflict_progressive_add_add(cx: &mut VisualTestAppContext) {
    matrix(cx, progressive_add_add_leg);
}
