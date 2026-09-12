//! Busy presentation is independent of the writer lease's lifecycle marker.
use super::KagiApp;

impl KagiApp {
    pub(crate) fn mark_write_busy(&mut self, name: &'static str) {
        self.busy_op = Some(name);
        self.write_busy_op = Some(name);
    }

    /// Is any operation latched — a write (`busy_op`, lease-mirrored) or a
    /// planning task (`planning`)? The single question every gate asks
    /// (ADR-0196 Wave 3); `busy_op` alone no longer answers it.
    pub(crate) fn op_latched(&self) -> bool {
        !super::operations::op_may_start(self.busy_op, self.planning)
    }

    pub(crate) fn busy_snackbar_label(&self) -> Option<&'static str> {
        self.busy_op
            .or(self.planning)
            .map(kagi_ui_core::i18n::busy_label)
    }
}

/// Only retire the lease-backed mirror. A legacy plan can be busy with no lease.
pub(super) fn settle_write_busy(
    busy: &mut Option<&'static str>,
    writer: &mut Option<&'static str>,
    has_leases: bool,
) {
    if has_leases {
        return;
    }
    if let Some(name) = writer.take() {
        if *busy == Some(name) {
            *busy = None;
        }
    }
}

/// Which latches a finished operation may drop (#289, ADR-0196 Wave 3).
///
/// The write mirror goes only when no lease survived the settle: a retained
/// lease means the writer's termination is unconfirmed (or its task panicked,
/// which proves nothing), so it may still be running — clearing the mirror
/// there would leave `has_leases()` true with `op_latched()` false, and a plan
/// could start against a live writer. `planning` is this completion's to clear
/// only while the tag it latched is still the one in place.
pub(super) fn release_finished_latches(
    busy: &mut Option<&'static str>,
    writer: &mut Option<&'static str>,
    planning: &mut Option<&'static str>,
    planning_tag: Option<&'static str>,
    has_leases: bool,
) {
    if !has_leases {
        *busy = None;
        *writer = None;
    }
    if planning_tag.is_some() && *planning == planning_tag {
        *planning = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retained_lease_keeps_its_mirror_and_planning_is_only_its_owners() {
        // Known termination: the lease went with the settle, so the mirror goes.
        let (mut busy, mut writer, mut planning) = (Some("pr-merge"), Some("pr-merge"), None);
        release_finished_latches(&mut busy, &mut writer, &mut planning, None, false);
        assert_eq!((busy, writer), (None, None));

        // Unconfirmed termination (or a panicked task): the lease is still
        // reserved, so the writer may still be running — keep it visible.
        let (mut busy, mut writer, mut planning) = (Some("pr-merge"), Some("pr-merge"), None);
        release_finished_latches(&mut busy, &mut writer, &mut planning, None, true);
        assert_eq!((busy, writer), (Some("pr-merge"), Some("pr-merge")));

        // A legacy plan owns its tag; another completion must not drop it.
        let mut planning = Some("merge-plan");
        release_finished_latches(&mut None, &mut None, &mut planning, None, false);
        assert_eq!(planning, Some("merge-plan"));
        release_finished_latches(
            &mut None,
            &mut None,
            &mut planning,
            Some("delete-branch-plan"),
            false,
        );
        assert_eq!(planning, Some("merge-plan"));
        release_finished_latches(
            &mut None,
            &mut None,
            &mut planning,
            Some("merge-plan"),
            false,
        );
        assert_eq!(planning, None);
    }

    #[test]
    fn named_writer_settles_but_live_leases_and_legacy_plans_do_not() {
        for name in [
            "fetch",
            "stash-apply",
            "remove-worktree",
            "editor-save",
            "stage",
        ] {
            let mut busy = Some(name);
            let mut writer = Some(name);
            settle_write_busy(&mut busy, &mut writer, true);
            assert_eq!(busy, Some(name));
            assert_eq!(writer, Some(name));
            settle_write_busy(&mut busy, &mut writer, false);
            assert_eq!((busy, writer), (None, None));
        }
        let mut busy = Some("delete-branch-plan");
        let mut writer = None;
        settle_write_busy(&mut busy, &mut writer, false);
        assert_eq!(busy, Some("delete-branch-plan"));
        writer = Some("fetch");
        settle_write_busy(&mut busy, &mut writer, false);
        assert_eq!(busy, Some("delete-branch-plan"));
        assert_eq!(writer, None);
    }
}
