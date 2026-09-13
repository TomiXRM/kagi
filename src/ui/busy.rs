//! Busy presentation is independent of the writer lease's lifecycle marker.
use super::KagiApp;

impl KagiApp {
    pub(crate) fn mark_write_busy(&mut self, name: &'static str) {
        self.write_busy_op = Some(name);
    }

    /// Latch the one write that cannot hold a lease: remote pull over SSH,
    /// whose `WriteScope::Remote(RemoteRepoId)` needs two network probes that
    /// cannot run on the UI thread before the spawn. (The remote stash family
    /// gets its id from a background plan job; a pull plan synthesised from a
    /// cached snapshot has no equivalent.) ADR-0196 決定 5 has the rationale.
    ///
    /// This is **not** a lease mirror, so [`settle_write_busy`] must never see
    /// it: a lease-derived retire would drop it on the very next
    /// `refresh_write_busy()` — which `render` → `poll_app_jobs` and every
    /// admission preamble call — and the pull would run unlatched (#708
    /// review P1). Only the pull's own terminal callback clears it, before
    /// every branch, so success, failure and a panicked task all release.
    ///
    /// Known gap, unchanged by this slice: `may_close_host` reads leases, so a
    /// remote pull does not hold quit. Putting it on a real lease fixes both,
    /// and is the same follow-up slice as #703.
    pub(crate) fn mark_remote_write(&mut self, name: &'static str) {
        self.remote_write = Some(name);
    }

    /// Is any operation latched — a write or a planning task? The single
    /// question every gate asks (ADR-0196 Wave 3).
    ///
    /// A held **lease** is the truth about every write that can take one;
    /// `remote_write` covers the one that cannot; `planning` writes nothing but
    /// owns the modal slot it is about to fill. `write_busy_op` is deliberately
    /// absent: it is only a presentation mirror of the lease, so reading it
    /// here would answer with the lease twice and with nothing new.
    pub(crate) fn op_latched(&self) -> bool {
        !super::operations::op_may_start(
            self.app_sessions.has_leases(),
            self.remote_write,
            self.planning,
        )
    }

    pub(crate) fn busy_snackbar_label(&self) -> Option<&'static str> {
        self.write_busy_op
            .or(self.remote_write)
            .or(self.planning)
            .map(kagi_ui_core::i18n::busy_label)
    }
}

/// Retire the write mirror once no lease survives. A retained lease means the
/// writer's termination is unconfirmed (or its task panicked, which proves
/// nothing), so it may still be running — clearing the mirror there would leave
/// `has_leases()` true with the name gone from the snackbar.
///
/// Only ever hand this the lease mirror. `remote_write` is owned by its writer,
/// not by the lease count, and passing it here is the #708 P1 defect.
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
