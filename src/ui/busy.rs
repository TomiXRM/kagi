//! Busy presentation is independent of the writer lease's lifecycle marker.
use super::KagiApp;

impl KagiApp {
    pub(crate) fn mark_write_busy(&mut self, name: &'static str) {
        self.write_busy_op = Some(name);
    }

    /// Is any operation latched — a write or a planning task? The single
    /// question every gate asks (ADR-0196 Wave 3). The **lease** is the truth
    /// about writes; `write_busy_op` is its name mirror and additionally the
    /// only latch the one remaining lease-less writer has (remote pull over
    /// SSH, which has no `RemoteRepoId` before its own probes), so dropping it
    /// from the gate would loosen exclusion for that one path. `planning`
    /// writes nothing but owns the modal slot it is about to fill.
    pub(crate) fn op_latched(&self) -> bool {
        !super::operations::op_may_start(
            self.app_sessions.has_leases(),
            self.write_busy_op,
            self.planning,
        )
    }

    pub(crate) fn busy_snackbar_label(&self) -> Option<&'static str> {
        self.write_busy_op
            .or(self.planning)
            .map(kagi_ui_core::i18n::busy_label)
    }
}

/// Retire the write mirror once no lease survives. A retained lease means the
/// writer's termination is unconfirmed (or its task panicked, which proves
/// nothing), so it may still be running — clearing the mirror there would leave
/// `has_leases()` true with the name gone from the snackbar.
pub(super) fn settle_write_busy(writer: &mut Option<&'static str>, has_leases: bool) {
    if !has_leases {
        *writer = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retained_lease_keeps_its_mirror() {
        for name in [
            "fetch",
            "stash-apply",
            "remove-worktree",
            "editor-save",
            "stage",
        ] {
            // Unconfirmed termination (or a panicked task): the lease is still
            // reserved, so the writer may still be running — keep it visible.
            let mut writer = Some(name);
            settle_write_busy(&mut writer, true);
            assert_eq!(writer, Some(name));
            // Known termination: the lease went with the settle, so does the
            // mirror.
            settle_write_busy(&mut writer, false);
            assert_eq!(writer, None);
        }
    }
}
