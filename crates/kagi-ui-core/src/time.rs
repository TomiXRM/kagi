//! Relative-time display helpers (no external crates), moved here from the
//! bin's `src/ui/commit_list.rs` (ADR-0121 C3) so extracted pane crates can
//! reuse them. The bin re-exports them from `commit_list` (shim).

use std::time::{SystemTime, UNIX_EPOCH};

/// Return the current time as seconds since Unix epoch.
/// Falls back to 0 if SystemTime is unavailable (should never happen).
pub fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Format a Unix-epoch timestamp as a human-readable relative string.
///
/// | Range          | Output example |
/// |----------------|----------------|
/// | < 60 s         | `"just now"`   |
/// | < 60 min       | `"42m ago"`    |
/// | < 24 h         | `"5h ago"`     |
/// | < 30 days      | `"3d ago"`     |
/// | < 12 months    | `"4mo ago"`    |
/// | ≥ 12 months    | `"2y ago"`     |
pub fn relative_time(epoch_secs: i64, now_secs: i64) -> String {
    let diff = now_secs.saturating_sub(epoch_secs).max(0);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3_600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86_400 {
        format!("{}h ago", diff / 3_600)
    } else if diff < 86_400 * 30 {
        format!("{}d ago", diff / 86_400)
    } else if diff < 86_400 * 365 {
        format!("{}mo ago", diff / (86_400 * 30))
    } else {
        format!("{}y ago", diff / (86_400 * 365))
    }
}
