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
    match age_parts(diff) {
        Some((count, unit)) => format!("{count}{unit} ago"),
        None => "just now".to_string(),
    }
}

/// Compact relative age for the Graph column; the verbose form remains in AX labels.
pub fn relative_time_short(epoch_secs: i64, now_secs: i64) -> String {
    let diff = now_secs.saturating_sub(epoch_secs).max(0);
    match age_parts(diff) {
        Some((count, unit)) => format!("{count}{unit}"),
        None => "now".to_string(),
    }
}

fn age_parts(diff: i64) -> Option<(i64, &'static str)> {
    if diff < 60 {
        None
    } else if diff < 3_600 {
        Some((diff / 60, "m"))
    } else if diff < 86_400 {
        Some((diff / 3_600, "h"))
    } else if diff < 86_400 * 30 {
        Some((diff / 86_400, "d"))
    } else if diff < 86_400 * 365 {
        Some((diff / (86_400 * 30), "mo"))
    } else {
        Some((diff / (86_400 * 365), "y"))
    }
}

#[cfg(test)]
mod tests {
    use super::{relative_time, relative_time_short};

    #[test]
    fn compact_age_changes_units_at_the_same_boundaries_as_verbose_age() {
        for (seconds, short, verbose) in [
            (0, "now", "just now"),
            (59, "now", "just now"),
            (60, "1m", "1m ago"),
            (2_159, "35m", "35m ago"),
            (3_600, "1h", "1h ago"),
            (86_399, "23h", "23h ago"),
            (86_400, "1d", "1d ago"),
            (86_400 * 30 - 1, "29d", "29d ago"),
            (86_400 * 30, "1mo", "1mo ago"),
            (86_400 * 365 - 1, "12mo", "12mo ago"),
            (86_400 * 365, "1y", "1y ago"),
        ] {
            assert_eq!(relative_time_short(0, seconds), short, "{seconds}s");
            assert_eq!(relative_time(0, seconds), verbose, "{seconds}s");
        }
        assert_eq!(relative_time_short(60, 0), "now", "future timestamp");
        assert_eq!(relative_time(60, 0), "just now", "future timestamp");
    }
}
