//! Commit detail panel — T010 / T019
//!
//! Pre-built display data for the right-side 360 px metadata panel.
//! All strings are computed at snapshot time; the render closure only clones.

use gpui::SharedString;

use kagi::git::{Commit, RepoSnapshot};

// ──────────────────────────────────────────────────────────────
// soft_wrap — display-only ZWSP insertion helper (T019)
// ──────────────────────────────────────────────────────────────

/// Insert U+200B (ZERO WIDTH SPACE) after every `every` consecutive
/// non-whitespace characters so that the display engine can break the line
/// at arbitrary positions.
///
/// **Display use only.**  Never pass the result to git operations, log
/// comparisons, or any place that expects a raw string — the ZWSP characters
/// are invisible but present in the byte stream.
///
/// - Works on `char` boundaries, never splitting multi-byte sequences.
/// - `every` must be at least 1; passing 0 returns the string unchanged.
/// - Existing whitespace resets the run counter (natural break points are
///   preferred by the layout engine already; we only need to handle runs
///   of non-space characters that are longer than `every`).
pub fn soft_wrap(text: &str, every: usize) -> String {
    if every == 0 || text.is_empty() {
        return text.to_string();
    }
    const ZWSP: char = '\u{200B}';
    let mut out = String::with_capacity(text.len() + text.len() / every + 1);
    let mut run = 0usize;
    for ch in text.chars() {
        if ch.is_whitespace() {
            // Natural break point — reset the run counter.
            run = 0;
            out.push(ch);
        } else {
            run += 1;
            out.push(ch);
            if run >= every {
                out.push(ZWSP);
                run = 0;
            }
        }
    }
    out
}

// ──────────────────────────────────────────────────────────────
// CommitDetail — one entry in the pre-built Vec
// ──────────────────────────────────────────────────────────────

/// Pre-computed display data for the detail panel shown when a commit row is
/// selected.
///
/// Built once from [`RepoSnapshot`]; the render path only reads by index.
///
/// Fields ending in `_display` contain U+200B (ZWSP) characters inserted by
/// [`soft_wrap`] so that the layout engine can break long runs of
/// non-whitespace text.  **Do not use `_display` fields for git operations,
/// log output, or any comparison that must match the raw string.**
#[derive(Clone)]
pub struct CommitDetail {
    /// Full 40-hex SHA (raw — no ZWSP; use `sha_display` for rendering).
    pub full_sha: SharedString,
    /// Full SHA with ZWSP every 8 chars — for display only.
    pub sha_display: SharedString,
    /// `name <email>` + absolute UTC date — for display only (ZWSP-wrapped).
    pub author_line: SharedString,
    /// Committer line (only set when committer differs from author) — ZWSP-wrapped.
    pub committer_line: Option<SharedString>,
    /// Short IDs of parent commits, e.g. `["a1b2c3d4", "e5f6a7b8"]`.
    pub parent_ids: Vec<SharedString>,
    /// Full commit message (may be multi-line) — ZWSP-wrapped at 16 chars for display.
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
    // Display version: break SHA every 8 chars (short-id size) so gpui can wrap it.
    let sha_display = SharedString::from(soft_wrap(&c.id.0, 8));

    // Author: soft-wrap every 16 chars to handle long emails and names.
    let author_raw = format!(
        "{}  <{}>  {}",
        c.author.name,
        c.author.email,
        format_utc(c.author.time),
    );
    let author_line = SharedString::from(soft_wrap(&author_raw, 16));

    // Show committer only when it differs from the author.
    let committer_line = if c.committer.name != c.author.name
        || c.committer.email != c.author.email
        || c.committer.time != c.author.time
    {
        let raw = format!(
            "{}  <{}>  {}",
            c.committer.name,
            c.committer.email,
            format_utc(c.committer.time),
        );
        Some(SharedString::from(soft_wrap(&raw, 16)))
    } else {
        None
    };

    let parent_ids = c
        .parents
        .iter()
        .map(|p| SharedString::from(p.short().to_string()))
        .collect();

    // Message: soft-wrap every 16 chars (handles unspaced CJK runs and long tokens).
    // Trim trailing whitespace; cap at 2000 chars to avoid extreme panel heights.
    let raw_msg = c.message.trim_end();
    const MAX_MSG_CHARS: usize = 2000;
    let capped: String = if raw_msg.chars().count() > MAX_MSG_CHARS {
        let s: String = raw_msg.chars().take(MAX_MSG_CHARS - 1).collect();
        format!("{}\u{2026}", s) // append ellipsis
    } else {
        raw_msg.to_string()
    };
    let full_message = SharedString::from(soft_wrap(&capped, 16));

    CommitDetail {
        full_sha,
        sha_display,
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
// Unit tests
// ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Date formatting tests ─────────────────────────────────

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

    // ── soft_wrap tests (T019) ────────────────────────────────

    const ZWSP: &str = "\u{200B}";

    /// Helper: collect the visible (non-ZWSP) characters from the output.
    fn visible(s: &str) -> String {
        s.chars().filter(|&c| c != '\u{200B}').collect()
    }

    #[test]
    fn soft_wrap_empty_string_unchanged() {
        assert_eq!(soft_wrap("", 8), "");
    }

    #[test]
    fn soft_wrap_every_zero_returns_unchanged() {
        // every=0 is a no-op guard.
        let input = "abcdefghij";
        assert_eq!(soft_wrap(input, 0), input);
    }

    #[test]
    fn soft_wrap_ascii_long_token_inserts_zwsp() {
        // 40-char SHA should get ZWSP every 8 chars.
        let sha = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";
        let result = soft_wrap(sha, 8);
        // Visible characters must be identical to the input.
        assert_eq!(visible(&result), sha);
        // Exactly 5 ZWSP characters (positions 8, 16, 24, 32, 40).
        let zwsp_count = result.chars().filter(|&c| c == '\u{200B}').count();
        assert_eq!(zwsp_count, 5, "expected 5 ZWSPs in SHA split by 8, got {}", zwsp_count);
    }

    #[test]
    fn soft_wrap_japanese_long_run_inserts_zwsp() {
        // 20 consecutive CJK characters with no space → ZWSP every 16.
        let jp = "あいうえおかきくけこさしすせそたちつてと";
        let result = soft_wrap(jp, 16);
        assert_eq!(visible(&result), jp);
        // 20 chars / 16 → 1 ZWSP (inserted after position 16).
        let zwsp_count = result.chars().filter(|&c| c == '\u{200B}').count();
        assert_eq!(zwsp_count, 1, "expected 1 ZWSP for 20-char CJK run, got {}", zwsp_count);
    }

    #[test]
    fn soft_wrap_already_spaced_text_unchanged_in_visible_content() {
        // "hello world" has a space before the run limit → no ZWSP inserted
        // because each word is shorter than `every`.
        let text = "hello world foo bar";
        let result = soft_wrap(text, 8);
        // Visible content must equal input.
        assert_eq!(visible(&result), text);
        // No ZWSP needed (each run ≤ 8 chars).
        assert!(!result.contains('\u{200B}'));
    }

    #[test]
    fn soft_wrap_mixed_spaced_and_long_token() {
        // "fix: " (5 chars + space) then a 40-char SHA.
        let sha = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";
        let text = format!("fix: {}", sha);
        let result = soft_wrap(&text, 8);
        assert_eq!(visible(&result), text);
        // "fix:" is 4 chars (no ZWSP), space resets, then SHA → 5 ZWSPs.
        let zwsp_count = result.chars().filter(|&c| c == '\u{200B}').count();
        assert_eq!(zwsp_count, 5);
    }

    #[test]
    fn soft_wrap_exactly_boundary_inserts_zwsp_at_boundary() {
        // 8 chars exactly → ZWSP appended after the 8th char.
        let text = "12345678";
        let result = soft_wrap(text, 8);
        assert_eq!(visible(&result), text);
        assert_eq!(result, format!("12345678{}", ZWSP));
    }

    #[test]
    fn soft_wrap_short_string_no_zwsp() {
        // 7 chars < every=8 → no ZWSP.
        assert_eq!(soft_wrap("abcdefg", 8), "abcdefg");
    }
}
