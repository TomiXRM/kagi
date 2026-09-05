//! #454: copying a popup's content.
//!
//! Popup text was not selectable and had no copy affordance at all (user
//! report 2026-09-06), so every card carries a hover-quiet copy button: one on
//! the title row for the whole dialog, one per list panel for its rows. Kept
//! in its own module because `modal_shell` is the layout shell — this is the
//! clipboard concern, and both were pushing the shell past the 800-LOC gate.

use super::i18n::Msg;
use super::theme::{self, theme as current_theme};
use super::types::ToastKind;
use super::KagiApp;
use gpui::{div, prelude::*, rgb, Context, SharedString};
use kagi_domain::plan::OperationPlan;
use kagi_ui_core::i18n::{plan_note_text, plan_recovery_text, plan_title_text};

/// A hover-quiet copy button for a popup surface.
///
/// #454: popup text could not be copied at all — no selection, no button (user
/// report 2026-09-06). Same shape as the PR-hunk copy button
/// (`pr_conversation::render_diff_hunk`): quiet until hovered, a tooltip, a
/// toast on success, and it swallows the mouse-**down** as well as the click so
/// pressing it never starts a text selection underneath.
pub(crate) fn modal_copy_button(
    id: &'static str,
    tooltip: &'static str,
    text: String,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let copy = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        cx.stop_propagation();
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        this.push_toast(
            ToastKind::Info,
            SharedString::from(Msg::ModalCopied.t()),
            cx,
        );
    });
    div()
        .id(id)
        .flex_shrink_0()
        .p_1()
        .rounded_sm()
        .cursor_pointer()
        .opacity(0.55)
        .hover(|st| st.bg(rgb(current_theme().selected)).opacity(1.0))
        .tooltip(move |w, cx| gpui_component::tooltip::Tooltip::new(tooltip).build(w, cx))
        .on_mouse_down(gpui::MouseButton::Left, |_e, _w, cx| {
            cx.stop_propagation();
        })
        .on_click(copy)
        .child(
            gpui::svg()
                .path("icons/copy.svg")
                .w(theme::scaled_px(12.))
                .h(theme::scaled_px(12.))
                .text_color(rgb(current_theme().text_sub)),
        )
        .into_any_element()
}

/// The whole popup as plain text, for [`modal_copy_button`].
///
/// Renders what the card shows, in the card's own order and already localized:
/// title, current → predicted, warnings, blockers, the row list the caller
/// passes in, then the recovery text. Plain text, not markdown: it is going
/// into a terminal or an issue, and the commands must survive verbatim.
pub(crate) fn plan_clipboard_text(plan: &OperationPlan, rows: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&plan_title_text(&plan.title));
    out.push('\n');
    out.push_str(&format!(
        "\ncurrent:   {} [{}]\npredicted: {} [{}]\n",
        plan.current.head, plan.current.dirty, plan.predicted.head, plan.predicted.dirty
    ));
    for w in &plan.warnings {
        out.push_str(&format!("\nwarning: {}", plan_note_text(w)));
    }
    for b in &plan.blockers {
        out.push_str(&format!("\nblocker: {}", plan_note_text(b)));
    }
    if !plan.warnings.is_empty() || !plan.blockers.is_empty() {
        out.push('\n');
    }
    if !rows.is_empty() {
        out.push('\n');
        for r in rows {
            out.push_str(r);
            out.push('\n');
        }
    }
    let recovery = plan_recovery_text(plan.recovery.as_ref());
    if !recovery.is_empty() {
        out.push('\n');
        out.push_str(&recovery);
        if !recovery.ends_with('\n') {
            out.push('\n');
        }
    }
    // #454 review: the prose above is localized explanation with the commands
    // interleaved, so pasting the whole payload into a shell would try to run
    // sentences. `PlanRecovery::commands` is the structured, paste-able set —
    // repeat it as its own block so a user can grab just those lines.
    if let Some(rec) = plan.recovery.as_ref() {
        if !rec.commands.is_empty() {
            out.push_str("\ncommands:\n");
            for c in &rec.commands {
                out.push_str("  ");
                out.push_str(c);
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::plan_clipboard_text;

    /// The copy button's payload must carry the **full** rows, not what the
    /// card shows: the list truncates a deep path visually (leading ellipsis),
    /// and a clipboard copy of `…/while/file.rs` would be useless in a shell.
    #[test]
    fn clipboard_text_carries_full_rows_and_recovery() {
        use kagi_domain::head::Head;
        use kagi_domain::plan::{OperationPlan, StateSummary};
        use kagi_domain::plan_note::{PlanDisposition, PlanRecovery, PlanTitle, RecoveryKind};

        let deep = "crates/kagi-git/src/ops/very/deeply/nested/file.rs".to_string();
        let plan = OperationPlan {
            title: PlanTitle::Discard {
                single: None,
                count: 2,
            },
            current: StateSummary {
                head: "branch: main".into(),
                dirty: "2 modified".into(),
            },
            predicted: StateSummary {
                head: "branch: main".into(),
                dirty: "2 file(s) discarded".into(),
            },
            warnings: Vec::new(),
            blockers: Vec::new(),
            recovery: Some(PlanRecovery {
                kind: RecoveryKind::Discard,
                commands: vec!["git cat-file -p <blob-sha>".into()],
            }),
            disposition: PlanDisposition::Ready,
            head_at_plan: Head::Attached {
                branch: "main".into(),
                target: "0".repeat(40),
            },
            stash_count_at_plan: 0,
            worktree_digest: None,
            preview_files: Vec::new(),
            preview_commits: Vec::new(),
            destructive: true,
            equivalent_command: None,
        };

        let text = plan_clipboard_text(&plan, &[deep.clone(), "a.txt".into()]);
        assert!(text.contains(&deep), "full path missing:\n{text}");
        assert!(
            !text.contains('\u{2026}'),
            "clipboard must not carry the display ellipsis"
        );
        assert!(text.contains("a.txt"));
        assert!(text.contains("current:"), "state summary missing:\n{text}");
        assert!(
            text.contains("git cat-file -p <blob-sha>"),
            "the structured recovery command must appear verbatim:\n{text}"
        );
        // #454 review: assert the rows themselves, not just the absence of the
        // display ellipsis — a future `chars().take(n)` would truncate without
        // ever emitting that glyph.
        for row in [deep.as_str(), "a.txt"] {
            assert!(
                text.lines().any(|l| l == row),
                "row {row} must appear verbatim:\n{text}"
            );
        }
    }
}
