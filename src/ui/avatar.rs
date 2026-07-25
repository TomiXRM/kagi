//! Commit author avatar cache (T020, ADR-0118 Phase 5.2).
//!
//! The deterministic fallback helpers (`avatar_color` / `avatar_initial`) and
//! the resolved-image map type now live in `kagi_ui_core::avatar` so the
//! extracted pane crates (`kagi-ui-editor`'s History header,
//! `kagi-ui-file-history`'s detail pane) can draw the same avatar Graph
//! mode's Inspector does. They're re-exported here so existing
//! `crate::ui::avatar::…` paths keep resolving (the same shim recipe as
//! `theme`/`i18n`/`settings`).
//!
//! What stays bin-side is the part that isn't pure: [`AvatarStore`], the
//! resolved-image cache plus its per-repo fetch bookkeeping, filled by the
//! network resolution pass in `avatar_fetch`/`avatar_resolve` (ADR-0037,
//! ADR-0123).

pub use kagi_ui_core::avatar::{avatar_color, avatar_initial, AvatarImages};

// ──────────────────────────────────────────────────────────────
// Avatar cache store
// ──────────────────────────────────────────────────────────────

/// Cohesive cache for resolved commit-author avatars (ADR-0118 Phase 5.2).
///
/// Groups the two formerly-flat `KagiApp` fields so the avatar cache moves as a
/// unit. Behaviour-preserving.
#[derive(Default)]
pub struct AvatarStore {
    /// Resolved avatar images keyed by author email.  Populated by a background
    /// resolution pass; rows/inspector swap the initial circle for `img(...)`
    /// when an entry exists.  Memory cache (the disk cache lives under
    /// `~/.kagi/avatars/`).
    pub images: AvatarImages,
    /// Repo path the `attempted` set belongs to. Switching repos resets the
    /// set so an email unresolved in one repo can retry with the next repo's
    /// Commits API map (ADR-0123).
    pub fetch_for: Option<std::path::PathBuf>,
    /// Emails a resolution pass has already been spawned for in the current
    /// repo (ADR-0123 incremental resolution). Emails deferred by the
    /// search-budget cap are removed again on completion so they retry.
    pub attempted: std::collections::HashSet<String>,
    /// `KagiApp::view_epoch` value the rows were last scanned at — the scan
    /// re-runs only when the view data changed (reload / load more / tab
    /// switch), keeping the per-frame `ensure_avatars` call one comparison.
    pub scan_epoch: Option<u64>,
}
