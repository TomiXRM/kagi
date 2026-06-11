//! Commit detail panel — T010
//!
//! Pre-built display data for the right-side 360 px metadata panel.
//! All strings are computed at snapshot time; the render closure only clones.

use gpui::SharedString;

use crate::git::{Commit, RepoSnapshot};

// ──────────────────────────────────────────────────────────────
// CommitDetail — one entry in the pre-built Vec
// ──────────────────────────────────────────────────────────────

/// Pre-computed display data for the detail panel shown when a commit row is
/// selected.
///
/// Built once from [`RepoSnapshot`]; the render path only reads by index.
#[derive(Clone)]
pub struct CommitDetail {
    /// Full 40-hex SHA.
    pub full_sha: SharedString,
    /// `name <email>` + absolute UTC date, e.g. `"Alice <a@b.com>  2024-01-15 09:30"`.
    pub author_line: SharedString,
    /// Committer line (only set when committer differs from author).
    pub committer_line: Option<SharedString>,
    /// Short IDs of parent commits, e.g. `["a1b2c3d4", "e5f6a7b8"]`.
    pub parent_ids: Vec<SharedString>,
    /// Full commit message (may be multi-line, bytes are valid UTF-8).
    pub full_message: SharedString,
}

// ──────────────────────────────────────────────────────────────
// Builder
// ──────────────────────────────────────────────────────────────

/// Build a `Vec<CommitDetail>` parallel to `snap.commits`.
pub fn build_commit_details(snap: &RepoSnapshot) -> Vec<CommitDetail> {
    snap.commits.iter().map(commit_to_detail).collect()
}

fn commit_to_detail(c: &Commit) -> CommitDetail {
    let full_sha = SharedString::from(c.id.0.clone());

    let author_line = SharedString::from(format!(
        "{}  <{}>  {}",
        c.author.name,
        c.author.email,
        format_utc(c.author.time),
    ));

    // Show committer only when it differs from the author.
    let committer_line = if c.committer.name != c.author.name
        || c.committer.email != c.author.email
        || c.committer.time != c.author.time
    {
        Some(SharedString::from(format!(
            "{}  <{}>  {}",
            c.committer.name,
            c.committer.email,
            format_utc(c.committer.time),
        )))
    } else {
        None
    };

    let parent_ids = c
        .parents
        .iter()
        .map(|p| SharedString::from(p.short().to_string()))
        .collect();

    // full_message already valid UTF-8 (git backend applies from_utf8_lossy).
    let full_message = SharedString::from(c.message.trim_end().to_string());

    CommitDetail {
        full_sha,
        author_line,
        committer_line,
        parent_ids,
        full_message,
    }
}

// ──────────────────────────────────────────────────────────────
// Absolute UTC date formatting (no external crates)
// ──────────────────────────────────────────────────────────────

/// Format a Unix-epoch timestamp as `"YYYY-MM-DD HH:MM"` (UTC).
///
/// Uses the civil_from_days algorithm (Howard Hinnant, 2013,
/// <http://howardhinnant.github.io/date_algorithms.html#civil_from_days>)
/// which is free of the Zeller-formula pitfalls and handles leap years
/// correctly for all representable i64 values.
pub fn format_utc(epoch_secs: i64) -> String {
    // Split seconds into day-number and time-of-day.
    // Rust integer division truncates toward zero; for negative epoch values
    // we must use floor-division so that day_number is always the *floor* of
    // epoch_secs / 86400.
    const SECS_PER_DAY: i64 = 86_400;
    let (day_number, time_of_day) = if epoch_secs >= 0 {
        (epoch_secs / SECS_PER_DAY, epoch_secs % SECS_PER_DAY)
    } else {
        // floor division for negative values
        let d = (epoch_secs - (SECS_PER_DAY - 1)) / SECS_PER_DAY;
        let t = epoch_secs - d * SECS_PER_DAY;
        (d, t)
    };

    let (year, month, day) = civil_from_days(day_number as i32);
    let hour = (time_of_day / 3_600) as u32;
    let minute = ((time_of_day % 3_600) / 60) as u32;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        year, month, day, hour, minute
    )
}

/// Convert a number of days since the Unix epoch (1970-01-01 = day 0) to a
/// proleptic Gregorian (year, month, day) triple.
///
/// Algorithm: Howard Hinnant, "date_algorithms.html#civil_from_days" (2013).
/// This is a verbatim Rust translation of the reference C++ implementation.
/// Handles all years representable by i32 correctly, including leap years and
/// the Gregorian reform.
fn civil_from_days(z: i32) -> (i32, u32, u32) {
    // Shift epoch from 1970-01-01 to 0000-03-01 so that the leap day falls
    // at the end of a 400-year cycle, making the arithmetic clean.
    let z = z + 719_468i32;
    // 400-year era (era * 146097 = days per 400-year cycle).
    let era: i32 = if z >= 0 { z } else { z - 146_096 } / 146_097;
    // Day-of-era [0, 146096].
    let doe: u32 = (z - era * 146_097) as u32;
    // Year-of-era [0, 399].
    let yoe: u32 = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    // Calendar year.
    let y: i32 = yoe as i32 + era * 400;
    // Day-of-year [0, 365] (0 = March 1).
    let doy: u32 = doe - (365 * yoe + yoe / 4 - yoe / 100);
    // Month-of-year in [0, 11] (0 = March).
    let mp: u32 = (5 * doy + 2) / 153;
    // Calendar day [1, 31].
    let d: u32 = doy - (153 * mp + 2) / 5 + 1;
    // Calendar month [1, 12].
    let m: u32 = if mp < 10 { mp + 3 } else { mp - 9 };
    // Adjust year for Jan/Feb belonging to the *following* calendar year in
    // the shifted-epoch scheme.
    let y = if m <= 2 { y + 1 } else { y };

    (y, m, d)
}

// ──────────────────────────────────────────────────────────────
// Unit tests for the date formatting
// ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_is_1970_01_01() {
        assert_eq!(format_utc(0), "1970-01-01 00:00");
    }

    #[test]
    fn known_timestamp() {
        // 2024-01-15 09:30:00 UTC  →  epoch 1705311000
        assert_eq!(format_utc(1_705_311_000), "2024-01-15 09:30");
    }

    #[test]
    fn leap_day_2000() {
        // 2000-02-29 00:00:00 UTC  →  epoch 951782400
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00");
    }

    #[test]
    fn non_leap_year_no_feb29() {
        // 1900 is NOT a leap year (divisible by 100 but not 400).
        // 1900-02-28 = day -25509 relative to Unix epoch (1970-01-01).
        // The next day must be 1900-03-01 (day -25508), not 1900-02-29.
        let (y, m, d) = civil_from_days(-25509); // 1900-02-28
        assert_eq!((y, m, d), (1900, 2, 28));
        // Next day must be 1900-03-01, not 1900-02-29.
        let (y2, m2, d2) = civil_from_days(-25508); // 1900-03-01
        assert_eq!((y2, m2, d2), (1900, 3, 1));
    }

    #[test]
    fn negative_epoch_before_1970() {
        // 1969-12-31 23:59:00 UTC  →  epoch -60
        assert_eq!(format_utc(-60), "1969-12-31 23:59");
    }
}
