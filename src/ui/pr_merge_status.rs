//! PR merge-status card (#347): the four `mergeStateStatus` actions, the merge
//! queue position, and the "still missing" list.
//!
//! Split out of `pr_mode.rs` (already at its LOC ceiling) as a pure renderer
//! over a [`MergeStatusView`]. The *decision* of which lines to show is a pure
//! function ([`status_lines`]) so it is unit-tested here rather than in GPUI;
//! `render` is a thin translation of those lines into elements.
//!
//! Deliberately absent: any rule-override / `--admin` button. kagi never offers
//! a bypass ([`MergeStatusView::show_admin_button`] is always `false`), so this
//! renderer has no branch that could draw one.

use gpui::{div, prelude::*, rgb, Context, SharedString};
use kagi_domain::github::ReviewState;
use kagi_domain::merge_state::{
    BypassCapability, MergeStatusView, MissingRequirements, RecommendedAction,
};
use kagi_git::github::PrMergeStatus;

use super::i18n::Msg;
use super::pr_mode::{card_bg, card_border};
use super::theme::theme;
use super::KagiApp;

/// One rendered line of the card. Kept as data (not strings) so the pure
/// [`status_lines`] can be asserted on without a running UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusLine {
    /// The recommended next action for the merge state.
    Action(RecommendedAction),
    /// Merge-queue position + optional ETA. Only ever produced when the repo
    /// actually has a merge queue with this PR in it.
    Queue {
        position: Option<u64>,
        eta: Option<String>,
    },
    /// `BLOCKED`: N approving reviews still needed.
    MissingApprovals(u32),
    /// `BLOCKED`: a CODEOWNERS entry whose review is required.
    MissingCodeowner(String),
    /// `BLOCKED`: N unresolved review threads.
    UnresolvedThreads(u32),
}

/// Build the render view-model from a fetched [`PrMergeStatus`] and the PR's
/// review decision.
///
/// `codeowner_reviews` is left empty here: matching the PR's changed paths
/// against a CODEOWNERS file is implemented and golden-tested in
/// `kagi_domain::codeowners`, but reading CODEOWNERS off the base ref is a
/// git-layer fetch not yet wired into the PR tab — a documented follow-up. The
/// bypass is always `Never`: kagi never offers an override.
pub fn view_from(status: &PrMergeStatus, review: ReviewState) -> MergeStatusView {
    let approvals_needed = matches!(
        review,
        ReviewState::ReviewRequired | ReviewState::ChangesRequested
    ) as u32;
    MergeStatusView::build(
        status.state,
        MissingRequirements {
            approvals_needed,
            codeowner_reviews: Vec::new(),
            unresolved_threads: status.unresolved_threads,
        },
        status.queue.clone(),
        BypassCapability::Never,
    )
}

/// The lines to render for a merge-status view, in display order. Pure.
///
/// Merge-queue absent ⇒ no [`StatusLine::Queue`]; the UI simply has nothing to
/// draw for the queue, which is how the MQ-absent case "stays intact".
pub fn status_lines(view: &MergeStatusView) -> Vec<StatusLine> {
    let mut lines = vec![StatusLine::Action(view.action)];
    if let Some(q) = &view.queue {
        lines.push(StatusLine::Queue {
            position: q.position,
            eta: q.estimated_time_to_merge.clone(),
        });
    }
    if let Some(m) = &view.missing {
        if m.approvals_needed > 0 {
            lines.push(StatusLine::MissingApprovals(m.approvals_needed));
        }
        for owner in &m.codeowner_reviews {
            lines.push(StatusLine::MissingCodeowner(owner.clone()));
        }
        if m.unresolved_threads > 0 {
            lines.push(StatusLine::UnresolvedThreads(m.unresolved_threads));
        }
    }
    lines
}

fn action_msg(action: RecommendedAction) -> Msg {
    match action {
        RecommendedAction::OpenConflictEditor => Msg::PrMergeActionConflict,
        RecommendedAction::UpdateBranch => Msg::PrMergeActionUpdateBranch,
        RecommendedAction::ShowMissingRequirements => Msg::PrMergeActionBlocked,
        RecommendedAction::ShowFailingChecks => Msg::PrMergeActionUnstable,
        RecommendedAction::ReadyToMerge => Msg::PrMergeActionReady,
        RecommendedAction::MarkReady => Msg::PrMergeActionDraft,
        RecommendedAction::Wait => Msg::PrMergeActionWait,
    }
}

fn line_text(line: &StatusLine) -> String {
    match line {
        StatusLine::Action(a) => action_msg(*a).t().to_string(),
        StatusLine::Queue { position, eta } => {
            let pos = position.map(|p| format!(" #{p}")).unwrap_or_default();
            let eta = eta
                .as_deref()
                .filter(|e| !e.is_empty())
                .map(|e| format!(" ({e})"))
                .unwrap_or_default();
            format!("{}{}{}", Msg::PrMergeQueueLabel.t(), pos, eta)
        }
        StatusLine::MissingApprovals(n) => format!("• {n} {}", Msg::PrMergeMissingApprovals.t()),
        StatusLine::MissingCodeowner(owner) => {
            format!("• {} {owner}", Msg::PrMergeMissingCodeowner.t())
        }
        StatusLine::UnresolvedThreads(n) => format!("• {n} {}", Msg::PrMergeUnresolvedThreads.t()),
    }
}

/// Render the merge-status card, or nothing when there is nothing worth saying
/// (a plain `Ready to merge` with no queue and no requirements adds only
/// noise — the merge button already carries that).
pub fn render(view: &MergeStatusView, _cx: &mut Context<KagiApp>) -> Option<gpui::AnyElement> {
    let lines = status_lines(view);
    let only_ready = matches!(
        lines.as_slice(),
        [StatusLine::Action(RecommendedAction::ReadyToMerge)]
    );
    if only_ready {
        return None;
    }
    let mut col = div()
        .flex()
        .flex_col()
        .gap_1()
        .mx_3()
        .mb_2()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(card_border())
        .bg(rgb(card_bg()))
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrMergeStatusHeading.t())),
        );
    for line in &lines {
        let color = match line {
            StatusLine::Action(RecommendedAction::ReadyToMerge) => theme().color_success,
            StatusLine::Action(_) => theme().text_main,
            _ => theme().text_sub,
        };
        col = col.child(
            div()
                .text_sm()
                .text_color(rgb(color))
                .child(SharedString::from(line_text(line))),
        );
    }
    Some(col.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::merge_state::{
        BypassCapability, MergeQueueEntryState, MergeStateStatus, MissingRequirements,
        QueuePosition,
    };

    #[test]
    fn blocked_lists_each_missing_requirement() {
        let view = MergeStatusView::build(
            MergeStateStatus::Blocked,
            MissingRequirements {
                approvals_needed: 2,
                codeowner_reviews: vec!["@org/security".into()],
                unresolved_threads: 3,
            },
            None,
            BypassCapability::Never,
        );
        let lines = status_lines(&view);
        assert_eq!(
            lines[0],
            StatusLine::Action(RecommendedAction::ShowMissingRequirements)
        );
        assert!(lines.contains(&StatusLine::MissingApprovals(2)));
        assert!(lines.contains(&StatusLine::MissingCodeowner("@org/security".into())));
        assert!(lines.contains(&StatusLine::UnresolvedThreads(3)));
        // No queue line — this repo has no merge queue.
        assert!(!lines.iter().any(|l| matches!(l, StatusLine::Queue { .. })));
    }

    #[test]
    fn merge_queue_absent_produces_no_queue_line() {
        for status in [
            MergeStateStatus::Clean,
            MergeStateStatus::Behind,
            MergeStateStatus::Dirty,
            MergeStateStatus::Unstable,
        ] {
            let view = MergeStatusView::build(
                status,
                MissingRequirements::default(),
                None,
                BypassCapability::Never,
            );
            let lines = status_lines(&view);
            assert!(
                !lines.iter().any(|l| matches!(l, StatusLine::Queue { .. })),
                "{status:?} must not draw a queue line without a merge queue"
            );
        }
    }

    #[test]
    fn queue_line_shown_when_queued() {
        let view = MergeStatusView::build(
            MergeStateStatus::Clean,
            MissingRequirements::default(),
            Some(QueuePosition {
                position: Some(4),
                estimated_time_to_merge: Some("about 5 minutes".into()),
                state: MergeQueueEntryState::Queued,
                next_entry_estimated_time_to_merge: None,
            }),
            BypassCapability::Never,
        );
        let lines = status_lines(&view);
        assert!(lines.contains(&StatusLine::Queue {
            position: Some(4),
            eta: Some("about 5 minutes".into()),
        }));
        // The rendered text carries the position.
        let text = line_text(&lines[1]);
        assert!(text.contains("#4"), "{text}");
    }
}
