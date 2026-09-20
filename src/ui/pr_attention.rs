//! Attention classification presentation shared by the PR dashboard, list,
//! conversation, and merge-status cards.

use gpui::rgb;
use kagi_domain::github::{stack_order, CiState, PrAttention, PrGroup, PrReason, PullRequest};

use super::i18n::Msg;
use super::theme::theme;
use super::KagiApp;

/// The card is defined by its border rather than a heavy fill, so the border
/// is a muted-foreground tint instead of the near-background `selected`.
pub(super) fn card_border() -> gpui::Hsla {
    let mut color: gpui::Hsla = rgb(theme().text_muted).into();
    color.a = if theme().dark { 0.45 } else { 0.55 };
    color
}

pub(super) fn ci_glyph(ci: CiState) -> (&'static str, u32) {
    match ci {
        CiState::Success => ("\u{2713}", theme().color_success),
        CiState::Failure => ("\u{2717}", theme().color_blocker),
        CiState::Pending => ("\u{25CF}", theme().color_warning),
        CiState::None => ("\u{25CB}", theme().text_muted),
    }
}

/// PRs bucketed by what the user should do. L2 availability is part of the
/// verdict, so absent checks cannot look ready.
pub(super) fn focus_queue(app: &KagiApp) -> Vec<(PrAttention, Vec<(PullRequest, PrReason)>)> {
    let login = app.github_login.clone();
    let local: Vec<String> = app
        .view()
        .branches
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    let mut buckets: Vec<(PrAttention, Vec<(PullRequest, PrReason)>)> = [
        PrAttention::NeedsYou,
        PrAttention::Pending,
        PrAttention::InProgress,
        PrAttention::Ready,
        PrAttention::Waiting,
        PrAttention::Dormant,
    ]
    .into_iter()
    .map(|attention| (attention, Vec::new()))
    .collect();
    for pr in &app.ui().github_prs {
        let group = pr.group_for(login.as_deref(), &local);
        let (attention, reason) = pr.attention_with_status(
            group == PrGroup::Mine,
            group == PrGroup::ReviewRequested,
            app.pr_status_availability(pr),
        );
        if let Some(bucket) = buckets
            .iter_mut()
            .find(|(candidate, _)| *candidate == attention)
        {
            bucket.1.push((pr.clone(), reason));
        }
    }
    for (_, members) in &mut buckets {
        let prs: Vec<PullRequest> = members.iter().map(|(pr, _)| pr.clone()).collect();
        *members = stack_order(&prs)
            .into_iter()
            .map(|(index, _)| members[index].clone())
            .collect();
    }
    buckets.retain(|(_, members)| !members.is_empty());
    buckets
}

pub(super) fn queue_bucket_label(attention: PrAttention) -> &'static str {
    match attention {
        PrAttention::NeedsYou => Msg::PrQueueNeedsYou.t(),
        PrAttention::Pending => Msg::PrQueuePending.t(),
        PrAttention::InProgress => Msg::PrQueueInProgress.t(),
        PrAttention::Ready => Msg::PrQueueReady.t(),
        PrAttention::Waiting => Msg::PrQueueWaiting.t(),
        PrAttention::Dormant => Msg::PrQueueDormant.t(),
    }
}

pub(super) fn attention_color(attention: PrAttention) -> u32 {
    match attention {
        PrAttention::NeedsYou => theme().color_blocker,
        PrAttention::Pending => theme().text_muted,
        PrAttention::InProgress => theme().color_warning,
        PrAttention::Ready => theme().color_success,
        PrAttention::Waiting => theme().color_branch,
        PrAttention::Dormant => theme().text_muted,
    }
}

pub(super) fn reason_text(reason: &PrReason) -> String {
    match reason {
        PrReason::CiFailed(count) => format!(
            "{} CI {}",
            count,
            if *count == 1 { "failure" } else { "failures" }
        ),
        PrReason::ChangesRequested => Msg::PrWhyChangesRequested.t().to_string(),
        PrReason::Conflicting => Msg::PrWhyConflicting.t().to_string(),
        PrReason::CiRunning => Msg::PrWhyCiRunning.t().to_string(),
        PrReason::ReadyToMerge => Msg::PrWhyReadyToMerge.t().to_string(),
        PrReason::ReviewRequested => Msg::PrWhyReviewRequested.t().to_string(),
        PrReason::AwaitingReview => Msg::PrWhyAwaitingReview.t().to_string(),
        PrReason::Pending => Msg::PrWhyPending.t().to_string(),
        PrReason::Draft => Msg::PrDraft.t().to_string(),
        PrReason::None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_failure_reason_is_pluralised() {
        assert_eq!(reason_text(&PrReason::CiFailed(1)), "1 CI failure");
        assert_eq!(reason_text(&PrReason::CiFailed(3)), "3 CI failures");
        assert_eq!(reason_text(&PrReason::CiFailed(0)), "0 CI failures");
    }
}
