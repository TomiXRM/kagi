//! Relative-time display helpers (no external crates), moved here from the
//! bin's `src/ui/commit_list.rs` (ADR-0121 C3) so extracted pane crates can
//! reuse them. The bin re-exports them from `commit_list` (shim).

use crate::i18n::time::{relative_age, AgeStyle, AgeUnit};
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
    relative_age(
        crate::i18n::lang(),
        age_parts(epoch_secs, now_secs),
        AgeStyle::Compact,
    )
}

/// Compact relative age for the Graph column; the verbose form remains in AX labels.
pub fn relative_time_short(epoch_secs: i64, now_secs: i64) -> String {
    relative_age(
        crate::i18n::lang(),
        age_parts(epoch_secs, now_secs),
        AgeStyle::Short,
    )
}

/// Fully spoken units for accessibility, using the same age boundaries.
pub fn relative_time_description(epoch_secs: i64, now_secs: i64) -> String {
    relative_age(
        crate::i18n::lang(),
        age_parts(epoch_secs, now_secs),
        AgeStyle::Description,
    )
}

fn age_parts(epoch_secs: i64, now_secs: i64) -> Option<(i64, AgeUnit)> {
    let diff = now_secs.saturating_sub(epoch_secs).max(0);
    if diff < 60 {
        None
    } else if diff < 3_600 {
        Some((diff / 60, AgeUnit::Minute))
    } else if diff < 86_400 {
        Some((diff / 3_600, AgeUnit::Hour))
    } else if diff < 86_400 * 30 {
        Some((diff / 86_400, AgeUnit::Day))
    } else if diff < 86_400 * 365 {
        Some((diff / (86_400 * 30), AgeUnit::Month))
    } else {
        Some((diff / (86_400 * 365), AgeUnit::Year))
    }
}

#[cfg(test)]
mod tests {
    use super::{age_parts, relative_age, AgeStyle};
    use crate::i18n::Lang;

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
            assert_eq!(
                relative_age(Lang::En, age_parts(0, seconds), AgeStyle::Short),
                short,
                "{seconds}s"
            );
            assert_eq!(
                relative_age(Lang::En, age_parts(0, seconds), AgeStyle::Compact),
                verbose,
                "{seconds}s"
            );
        }
        assert_eq!(
            relative_age(Lang::En, age_parts(60, 0), AgeStyle::Short),
            "now",
            "future timestamp"
        );
        assert_eq!(
            relative_age(Lang::En, age_parts(60, 0), AgeStyle::Compact),
            "just now",
            "future timestamp"
        );
    }
}
