//! GitHub `mergeStateStatus` → recommended-action mapping, merge-queue state,
//! and the "what is still missing" summary — all pure (ADR-0153).
//!
//! GitHub's `mergeStateStatus` has **four** actionable states, but kagi's PR
//! conflict preview (ADR-0145) only ever handled `DIRTY`. This module is the
//! pure core that turns each state into the one next action, so the UI is a
//! thin renderer over a table that is unit-tested here rather than in GPUI.
//!
//! Product invariant baked in here: **kagi never offers a rule-override /
//! `--admin` bypass.** We show *what* is missing (approvals, CODEOWNERS
//! reviews, unresolved threads); we never show a button to step over it. That
//! is why [`MergeStatusView::show_admin_button`] is hardwired `false` and has
//! no code path that can flip it — even when the API reports the viewer *could*
//! bypass. See §5 of #347.

/// GitHub's `PullRequest.mergeStateStatus`.
///
/// The enum GitHub returns is larger than the four *actionable* states the
/// issue names; every value is kept so we degrade honestly rather than folding
/// an unknown state into a wrong action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStateStatus {
    /// Mergeable and passing — the merge button is live.
    Clean,
    /// The head branch is out of date with the base; rebase / update branch.
    Behind,
    /// A required gate is unmet: approvals, CODEOWNERS review, or an unresolved
    /// review thread. We surface *which*.
    Blocked,
    /// The merge would conflict — open the conflict preview/editor (ADR-0145).
    Dirty,
    /// A **non-required** check is failing (required-check failure surfaces as
    /// `Blocked`). We list the failing optional checks.
    Unstable,
    /// The PR is a draft.
    Draft,
    /// Merge is queued behind a pre-receive hook / mergeability recompute.
    HasHooks,
    /// GitHub has not computed a state yet, or returned one we do not model.
    Unknown,
}

impl MergeStateStatus {
    /// Parse GitHub's GraphQL `MergeStateStatus` enum. Case-insensitive; any
    /// unrecognised value degrades to [`MergeStateStatus::Unknown`] rather than
    /// guessing.
    pub fn from_graphql(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "CLEAN" => Self::Clean,
            "BEHIND" => Self::Behind,
            "BLOCKED" => Self::Blocked,
            "DIRTY" => Self::Dirty,
            "UNSTABLE" => Self::Unstable,
            "DRAFT" => Self::Draft,
            "HAS_HOOKS" => Self::HasHooks,
            _ => Self::Unknown,
        }
    }

    /// The single next action kagi proposes for this state.
    pub fn recommended_action(self) -> RecommendedAction {
        match self {
            Self::Dirty => RecommendedAction::OpenConflictEditor,
            Self::Behind => RecommendedAction::UpdateBranch,
            Self::Blocked => RecommendedAction::ShowMissingRequirements,
            Self::Unstable => RecommendedAction::ShowFailingChecks,
            Self::Clean => RecommendedAction::ReadyToMerge,
            Self::Draft => RecommendedAction::MarkReady,
            Self::HasHooks => RecommendedAction::Wait,
            Self::Unknown => RecommendedAction::Wait,
        }
    }
}

/// The one action the PR screen offers for a given [`MergeStateStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecommendedAction {
    /// `DIRTY` — open the conflict preview/editor (ADR-0145).
    OpenConflictEditor,
    /// `BEHIND` — rebase onto / update from base.
    UpdateBranch,
    /// `BLOCKED` — show the missing-requirements list (never a bypass button).
    ShowMissingRequirements,
    /// `UNSTABLE` — show the failing non-required checks.
    ShowFailingChecks,
    /// `CLEAN` — the merge button is live.
    ReadyToMerge,
    /// `DRAFT` — mark ready for review first.
    MarkReady,
    /// `HAS_HOOKS` / `UNKNOWN` — nothing to do but wait for GitHub.
    Wait,
}

/// What a `BLOCKED` PR is still waiting on. Approvals count comes from
/// `reviewDecision`; `codeowner_reviews` is computed locally by matching the
/// PR's changed paths against CODEOWNERS (GitHub's API does not return owners —
/// see [`crate::codeowners`]); unresolved threads from the review-thread query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MissingRequirements {
    /// Number of additional approving reviews still required (0 if unknown).
    pub approvals_needed: u32,
    /// CODEOWNERS entries (users / `@org/team`) whose review is required and
    /// not yet given, for the files this PR touches.
    pub codeowner_reviews: Vec<String>,
    /// Count of unresolved review threads.
    pub unresolved_threads: u32,
}

impl MissingRequirements {
    /// True when nothing is actually outstanding — used to decide whether the
    /// list is worth rendering at all.
    pub fn is_empty(&self) -> bool {
        self.approvals_needed == 0
            && self.codeowner_reviews.is_empty()
            && self.unresolved_threads == 0
    }
}

/// GitHub's `MergeQueueEntry.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeQueueEntryState {
    Queued,
    AwaitingChecks,
    Mergeable,
    Unmergeable,
    Locked,
    Unknown,
}

impl MergeQueueEntryState {
    pub fn from_graphql(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "QUEUED" => Self::Queued,
            "AWAITING_CHECKS" => Self::AwaitingChecks,
            "MERGEABLE" => Self::Mergeable,
            "UNMERGEABLE" => Self::Unmergeable,
            "LOCKED" => Self::Locked,
            _ => Self::Unknown,
        }
    }
}

/// This PR's position in the repository's merge queue.
///
/// `None` merge queue = the repo does not use one (it is a paid GitHub
/// feature). The UI **hides** the queue section entirely in that case rather
/// than graying it out; see [`MergeStatusView::queue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuePosition {
    /// 1-based position; `None` while GitHub is still computing it.
    pub position: Option<u64>,
    /// Human string GitHub provides, e.g. "about 5 minutes".
    pub estimated_time_to_merge: Option<String>,
    /// The entry's own state.
    pub state: MergeQueueEntryState,
    /// Estimate for the *next* entry to merge (queue-wide progress hint).
    pub next_entry_estimated_time_to_merge: Option<String>,
}

/// Whether the viewer could, per GitHub, bypass branch-protection rules.
///
/// Modelled faithfully because the API reports it, but it has **no** effect on
/// what kagi renders: kagi never provides an override. Kept so the intent is
/// explicit in the type, not just absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BypassCapability {
    /// The viewer cannot bypass — the honest, common case.
    Never,
    /// GitHub says the viewer *could* bypass. kagi still shows no button.
    Allowed,
}

/// The complete, render-ready model for the PR merge-status surface. Building
/// it is pure; the GPUI layer only reads fields off it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeStatusView {
    pub status: MergeStateStatus,
    pub action: RecommendedAction,
    /// Present only when [`MergeStateStatus::Blocked`]; the list to render.
    pub missing: Option<MissingRequirements>,
    /// Present only when the repo actually has a merge queue with this PR in
    /// it. `None` ⇒ the UI renders no queue section (MQ-absent stays intact).
    pub queue: Option<QueuePosition>,
    /// **Always `false`.** kagi never renders a rule-override / `--admin`
    /// button. There is deliberately no input that can make this `true`.
    pub show_admin_button: bool,
}

impl MergeStatusView {
    /// Assemble the view-model. `missing` is attached only for `Blocked`;
    /// `queue` is passed through as-is (its `None` means "no merge queue →
    /// hide"). `_bypass` is accepted to document that even `Allowed` produces
    /// no admin button — the parameter is intentionally ignored.
    pub fn build(
        status: MergeStateStatus,
        missing: MissingRequirements,
        queue: Option<QueuePosition>,
        _bypass: BypassCapability,
    ) -> Self {
        MergeStatusView {
            status,
            action: status.recommended_action(),
            missing: (status == MergeStateStatus::Blocked && !missing.is_empty())
                .then_some(missing),
            queue,
            // ponytail: hardwired. No override button, ever — offering a bypass
            // contradicts the "no destructive, no override" ethos (#347 §5).
            show_admin_button: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance §6: each of the four actionable `mergeStateStatus` values
    /// maps to its distinct next action.
    #[test]
    fn four_merge_states_map_to_distinct_actions() {
        assert_eq!(
            MergeStateStatus::Dirty.recommended_action(),
            RecommendedAction::OpenConflictEditor
        );
        assert_eq!(
            MergeStateStatus::Behind.recommended_action(),
            RecommendedAction::UpdateBranch
        );
        assert_eq!(
            MergeStateStatus::Blocked.recommended_action(),
            RecommendedAction::ShowMissingRequirements
        );
        assert_eq!(
            MergeStateStatus::Unstable.recommended_action(),
            RecommendedAction::ShowFailingChecks
        );
    }

    #[test]
    fn parses_graphql_states_case_insensitively_and_degrades() {
        assert_eq!(
            MergeStateStatus::from_graphql("DIRTY"),
            MergeStateStatus::Dirty
        );
        assert_eq!(
            MergeStateStatus::from_graphql("behind"),
            MergeStateStatus::Behind
        );
        assert_eq!(
            MergeStateStatus::from_graphql("HAS_HOOKS"),
            MergeStateStatus::HasHooks
        );
        assert_eq!(
            MergeStateStatus::from_graphql("wat"),
            MergeStateStatus::Unknown
        );
        assert_eq!(
            MergeStateStatus::from_graphql(""),
            MergeStateStatus::Unknown
        );
    }

    /// Acceptance §6: `current_user_can_bypass: never` ⇒ no admin button. And,
    /// per §5, not even `Allowed` produces one.
    #[test]
    fn never_shows_admin_button_regardless_of_bypass() {
        for bypass in [BypassCapability::Never, BypassCapability::Allowed] {
            let v = MergeStatusView::build(
                MergeStateStatus::Blocked,
                MissingRequirements {
                    approvals_needed: 1,
                    ..Default::default()
                },
                None,
                bypass,
            );
            assert!(!v.show_admin_button, "no admin button for {bypass:?}");
        }
    }

    /// Acceptance §6: merge queue absent ⇒ the view carries no queue section
    /// (the UI hides it; nothing to render, nothing to break).
    #[test]
    fn merge_queue_absent_leaves_no_queue_section() {
        let v = MergeStatusView::build(
            MergeStateStatus::Clean,
            MissingRequirements::default(),
            None,
            BypassCapability::Never,
        );
        assert!(v.queue.is_none());
        assert_eq!(v.action, RecommendedAction::ReadyToMerge);
    }

    #[test]
    fn missing_requirements_attached_only_when_blocked_and_nonempty() {
        let blocked = MergeStatusView::build(
            MergeStateStatus::Blocked,
            MissingRequirements {
                approvals_needed: 2,
                codeowner_reviews: vec!["@org/security".into()],
                unresolved_threads: 1,
            },
            None,
            BypassCapability::Never,
        );
        let m = blocked.missing.expect("blocked carries requirements");
        assert_eq!(m.approvals_needed, 2);
        assert_eq!(m.codeowner_reviews, vec!["@org/security".to_string()]);

        let clean = MergeStatusView::build(
            MergeStateStatus::Clean,
            MissingRequirements {
                approvals_needed: 2,
                ..Default::default()
            },
            None,
            BypassCapability::Never,
        );
        assert!(clean.missing.is_none());
    }

    #[test]
    fn queue_entry_state_parses() {
        assert_eq!(
            MergeQueueEntryState::from_graphql("AWAITING_CHECKS"),
            MergeQueueEntryState::AwaitingChecks
        );
        assert_eq!(
            MergeQueueEntryState::from_graphql("locked"),
            MergeQueueEntryState::Locked
        );
        assert_eq!(
            MergeQueueEntryState::from_graphql("???"),
            MergeQueueEntryState::Unknown
        );
    }
}
