//! Busy presentation is independent of the writer lease's lifecycle marker.
use super::KagiApp;

impl KagiApp {
    pub(crate) fn mark_write_busy(&mut self, name: &'static str) {
        self.busy_op = Some(name);
        self.write_busy_op = Some(name);
    }

    pub(crate) fn busy_snackbar_label(&self) -> Option<&'static str> {
        self.busy_op.map(kagi_ui_core::i18n::busy_label)
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

#[cfg(test)]
mod tests {
    use super::*;

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
