//! `StatusBarVM` — the bottom status-bar chips as plain data (ADR-0076 / P5).
//!
//! Projects a [`StatusBarSummary`] (already a pure snapshot of repo state) into
//! the ordered list of chips the status bar shows, with their exact labels. The
//! view (`render_status_bar`) maps each [`StatusChipRole`] to a colour/margin
//! and builds the `gpui` element — no presentation *decision* lives in the view
//! anymore. Because this is plain data it is unit-tested below without a window.

use crate::ui::StatusBarSummary;

/// Kind of a status-bar chip. The view maps this to a theme colour + margin;
/// the VM decides which chips exist and in what order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusChipRole {
    /// Dirty working-tree bullet (●).
    Dirty,
    /// Staged file count (`+N`).
    Staged,
    /// Unstaged file count (`~N`).
    Unstaged,
    /// Conflicted file count (`!N`).
    Conflict,
    /// Stash entry count (`⚑N`).
    Stash,
    /// Ahead/behind upstream (`↑A ↓B`).
    AheadBehind,
    /// No upstream configured (and not detached).
    NoUpstream,
    /// Upstream tracking-ref name (`→ origin/main`).
    UpstreamName,
    /// Age of the last fetch (`⇣ 3m`), fresh (ADR-0127).
    FetchAge,
    /// Age of the last fetch once past [`FETCH_STALE_WARN_SECS`] — the view
    /// paints this in the warning colour so a silently-failing auto-fetch
    /// (network down) is noticeable (ADR-0127).
    FetchStale,
}

/// One chip: its role (drives colour/margin in the view) and rendered text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusChip {
    pub role: StatusChipRole,
    pub text: String,
}

impl StatusChip {
    fn new(role: StatusChipRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
        }
    }
}

/// Plain-data view-model for the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBarVM {
    /// Branch label (short name, `detached HEAD`, or `no commits yet`).
    pub branch: String,
    /// Chips in left-to-right display order (matching the historical order:
    /// dirty, staged, unstaged, conflict, stash, ahead/behind|no-upstream,
    /// upstream-name).
    pub chips: Vec<StatusChip>,
}

impl StatusBarVM {
    /// Build the view-model from a status summary, applying the exact same
    /// visibility conditions and label formats the status bar used inline.
    pub fn from_summary(s: &StatusBarSummary) -> Self {
        use StatusChipRole::*;
        let mut chips = Vec::new();
        if s.is_dirty {
            chips.push(StatusChip::new(Dirty, "\u{25cf}")); // ●
        }
        if s.staged > 0 {
            chips.push(StatusChip::new(Staged, format!("+{}", s.staged)));
        }
        if s.unstaged > 0 {
            chips.push(StatusChip::new(Unstaged, format!("~{}", s.unstaged)));
        }
        if s.conflict_count > 0 {
            chips.push(StatusChip::new(Conflict, format!("!{}", s.conflict_count)));
        }
        if s.stash_count > 0 {
            chips.push(StatusChip::new(Stash, format!("\u{2691}{}", s.stash_count)));
            // ⚑N
        }
        // Ahead/behind (or "no upstream") comes before the tracking-ref name,
        // matching the historical element-assembly order.
        match (s.ahead, s.behind) {
            (Some(a), Some(b)) => {
                chips.push(StatusChip::new(
                    AheadBehind,
                    format!("\u{2191}{} \u{2193}{}", a, b), // ↑A ↓B
                ));
            }
            _ if s.no_upstream => {
                chips.push(StatusChip::new(NoUpstream, "no upstream"));
            }
            _ => {} // detached HEAD or unborn: nothing shown
        }
        if !s.upstream_name.is_empty() {
            chips.push(StatusChip::new(
                UpstreamName,
                format!("\u{2192} {}", s.upstream_name), // → origin/main
            ));
        }
        Self {
            branch: s.branch.clone(),
            chips,
        }
    }
}

/// Fetch-age threshold past which the indicator turns into the warning
/// colour (ADR-0127). 5× the 180 s auto-fetch interval — a couple of missed
/// cycles is noise, a quarter hour of silence is worth a highlight.
pub const FETCH_STALE_WARN_SECS: i64 = 15 * 60;

/// Build the fetch-age chip (`⇣ <age>`) for the status bar (ADR-0127).
///
/// `None` when the repo has no remote (nothing to fetch) or has never
/// fetched (no `FETCH_HEAD` yet — the first auto-fetch creates it within one
/// interval). Fresh ages use [`StatusChipRole::FetchAge`]; ages past
/// [`FETCH_STALE_WARN_SECS`] use [`StatusChipRole::FetchStale`] so the view
/// paints them in the warning colour.
pub fn fetch_age_chip(s: &StatusBarSummary, now_secs: i64) -> Option<StatusChip> {
    if !s.has_remote {
        return None;
    }
    let last = s.last_fetch_secs?;
    let age = (now_secs - last).max(0);
    let role = if age >= FETCH_STALE_WARN_SECS {
        StatusChipRole::FetchStale
    } else {
        StatusChipRole::FetchAge
    };
    Some(StatusChip {
        role,
        text: format!("\u{21e3} {}", age_label(age)), // ⇣ 3m
    })
}

// ── Footer message preview (#547) ───────────────────────────────
//
// The footer is one fixed 22 px line laid out with `items_center()` +
// `overflow_hidden()`. Handing it a raw operation result (which may be
// multi-line, or a whole `git` stderr) made GPUI wrap the *full* text into a
// tall block and then centre-clip it, so the first line — the op name and the
// failure reason — was scrolled out of view and a middle line showed instead.
// So the view renders a bounded, single-line preview; the untruncated body
// keeps living in the Operation Log (and the persisted oplog), untouched.

/// Character budget for the footer's one-line preview. Wide enough that at a
/// 1,440 px window the CSS ellipsis, not this cap, is what usually trims —
/// small enough that a 1 MB failure body is never normalised in full.
pub const FOOTER_LINE_CHARS: usize = 256;

/// Character budget for the hover excerpt. The tooltip is a peek at the
/// beginning of the body, not a second copy of it: the full text is one click
/// away in the Operation Log.
pub const FOOTER_TOOLTIP_CHARS: usize = 512;

/// The footer's one-line preview: the first line (LF / CRLF / CR), with runs
/// of whitespace collapsed to single spaces and a `…` when anything was left
/// behind. Reads at most [`FOOTER_LINE_CHARS`] characters of `msg` plus the
/// whitespace run that follows.
pub fn footer_line(msg: &str) -> String {
    let mut out = String::new();
    let mut chars = 0usize;
    let mut gap = false;
    let mut more = false;
    let mut rest = msg.chars();
    while let Some(ch) = rest.next() {
        if ch == '\n' || ch == '\r' {
            // A trailing newline is not "more text": only ink counts.
            more = rest.any(|c| !c.is_whitespace());
            break;
        }
        if ch.is_whitespace() {
            gap = !out.is_empty();
            continue;
        }
        if chars >= FOOTER_LINE_CHARS {
            more = true;
            break;
        }
        if std::mem::take(&mut gap) {
            out.push(' ');
            chars += 1;
        }
        out.push(ch);
        chars += 1;
    }
    if more {
        out.push('\u{2026}'); // …
    }
    out
}

/// Bounded hover text for the footer message, or `None` when the one-line
/// preview already shows everything there is.
pub fn footer_excerpt(msg: &str) -> Option<String> {
    let head: String = msg.chars().take(FOOTER_TOOLTIP_CHARS).collect();
    let truncated = msg.chars().nth(FOOTER_TOOLTIP_CHARS).is_some();
    let mut out = head.trim().to_string();
    if truncated {
        out.push('\u{2026}');
    }
    (out != footer_line(msg)).then_some(out)
}

/// Compact age label: `42s` / `3m` / `2h` / `5d`.
fn age_label(age_secs: i64) -> String {
    match age_secs {
        s if s < 60 => format!("{}s", s),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use StatusChipRole::*;

    fn roles(vm: &StatusBarVM) -> Vec<StatusChipRole> {
        vm.chips.iter().map(|c| c.role).collect()
    }

    #[test]
    fn clean_synced_branch_has_no_chips() {
        let s = StatusBarSummary {
            branch: "main".into(),
            ahead: Some(0),
            behind: Some(0),
            ..Default::default()
        };
        let vm = StatusBarVM::from_summary(&s);
        assert_eq!(vm.branch, "main");
        // ahead=0/behind=0 still renders a ↑0 ↓0 chip (historical behaviour).
        assert_eq!(roles(&vm), vec![AheadBehind]);
        assert_eq!(vm.chips[0].text, "\u{2191}0 \u{2193}0");
    }

    #[test]
    fn dirty_counts_and_upstream_in_order() {
        let s = StatusBarSummary {
            branch: "feature".into(),
            is_dirty: true,
            staged: 2,
            unstaged: 3,
            conflict_count: 1,
            stash_count: 4,
            ahead: Some(5),
            behind: Some(6),
            upstream_name: "origin/feature".into(),
            ..Default::default()
        };
        let vm = StatusBarVM::from_summary(&s);
        assert_eq!(
            roles(&vm),
            vec![
                Dirty,
                Staged,
                Unstaged,
                Conflict,
                Stash,
                AheadBehind,
                UpstreamName
            ]
        );
        let texts: Vec<&str> = vm.chips.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "\u{25cf}",
                "+2",
                "~3",
                "!1",
                "\u{2691}4",
                "\u{2191}5 \u{2193}6",
                "\u{2192} origin/feature",
            ]
        );
    }

    #[test]
    fn no_upstream_chip_when_flagged() {
        let s = StatusBarSummary {
            branch: "main".into(),
            no_upstream: true,
            ..Default::default()
        };
        let vm = StatusBarVM::from_summary(&s);
        assert_eq!(roles(&vm), vec![NoUpstream]);
        assert_eq!(vm.chips[0].text, "no upstream");
    }

    #[test]
    fn detached_shows_no_upstream_related_chip() {
        // ahead/behind None and no_upstream false (detached/unborn) → nothing.
        let s = StatusBarSummary {
            branch: "detached HEAD".into(),
            is_detached: true,
            ..Default::default()
        };
        let vm = StatusBarVM::from_summary(&s);
        assert!(roles(&vm).is_empty());
    }

    // ── ADR-0127: fetch-age chip ───────────────────────────────

    fn remote_summary(last_fetch_secs: Option<i64>) -> StatusBarSummary {
        StatusBarSummary {
            has_remote: true,
            last_fetch_secs,
            ..Default::default()
        }
    }

    #[test]
    fn fetch_age_fresh_and_stale_roles() {
        let now = 1_000_000;
        // 3 minutes ago → fresh, minute label.
        let chip = fetch_age_chip(&remote_summary(Some(now - 180)), now).unwrap();
        assert_eq!(chip.role, FetchAge);
        assert_eq!(chip.text, "\u{21e3} 3m");
        // Exactly at the threshold → stale.
        let chip = fetch_age_chip(&remote_summary(Some(now - FETCH_STALE_WARN_SECS)), now).unwrap();
        assert_eq!(chip.role, FetchStale);
        assert_eq!(chip.text, "\u{21e3} 15m");
        // Two days of silence → stale, day label.
        let chip = fetch_age_chip(&remote_summary(Some(now - 2 * 86_400)), now).unwrap();
        assert_eq!(chip.role, FetchStale);
        assert_eq!(chip.text, "\u{21e3} 2d");
    }

    #[test]
    fn fetch_age_hidden_without_remote_or_fetch() {
        let now = 1_000_000;
        // Never fetched → hidden (first auto-fetch creates FETCH_HEAD).
        assert!(fetch_age_chip(&remote_summary(None), now).is_none());
        // No remote → hidden even with a FETCH_HEAD timestamp.
        let s = StatusBarSummary {
            has_remote: false,
            last_fetch_secs: Some(now - 30),
            ..Default::default()
        };
        assert!(fetch_age_chip(&s, now).is_none());
    }

    // ── #547: footer one-line preview ──────────────────────────

    #[test]
    fn footer_line_keeps_the_first_line_of_every_newline_flavour() {
        for msg in [
            "first line\nsecond line\nthird line",
            "first line\r\nsecond line\r\nthird line",
            "first line\rsecond line\rthird line",
        ] {
            assert_eq!(footer_line(msg), "first line\u{2026}");
        }
    }

    #[test]
    fn footer_line_collapses_whitespace_and_stays_single_line() {
        assert_eq!(footer_line("push:   ok\t\tdone  "), "push: ok done");
        assert_eq!(footer_line(""), "");
        assert_eq!(footer_line("   "), "");
        // A trailing newline is not "more text" — no misleading ellipsis.
        assert_eq!(footer_line("push: ok\n"), "push: ok");
        assert_eq!(footer_line("push: ok\n\n  \n"), "push: ok");
        for msg in [
            "a\nb".to_string(),
            "\u{1f469}\u{200d}\u{1f4bb}e\u{301}".repeat(200),
        ] {
            assert!(!footer_line(&msg).contains(['\n', '\r']));
        }
    }

    #[test]
    fn footer_line_is_bounded_for_huge_bodies() {
        let huge = "failed: long message ".repeat(50_000); // ~1 MB, one line
        let line = footer_line(&huge);
        assert!(line.ends_with('\u{2026}'), "{line}");
        assert!(line.chars().count() <= FOOTER_LINE_CHARS + 2, "{line}");
        assert!(line.starts_with("failed: long message"), "{line}");
        // Japanese and combining marks are counted as chars, never split
        // mid-`char` (which would not even be valid UTF-8).
        let ja = footer_line(&"\u{5931}\u{6557}: \u{9577}\u{3044}".repeat(200));
        assert!(ja.chars().count() <= FOOTER_LINE_CHARS + 2);
        assert!(ja.starts_with("\u{5931}\u{6557}:"));
    }

    #[test]
    fn footer_excerpt_only_when_there_is_more_to_see() {
        assert_eq!(footer_excerpt("push: ok"), None);
        assert_eq!(footer_excerpt(""), None);
        assert_eq!(
            footer_excerpt("first line\nsecond line").as_deref(),
            Some("first line\nsecond line"),
        );
        let huge = "x".repeat(FOOTER_TOOLTIP_CHARS * 4);
        let tip = footer_excerpt(&huge).expect("truncated body gets an excerpt");
        assert_eq!(tip.chars().count(), FOOTER_TOOLTIP_CHARS + 1);
        assert!(tip.ends_with('\u{2026}'));
    }

    #[test]
    fn fetch_age_clock_skew_clamps_to_zero() {
        // FETCH_HEAD mtime in the future (clock skew) must not underflow.
        let now = 1_000_000;
        let chip = fetch_age_chip(&remote_summary(Some(now + 500)), now).unwrap();
        assert_eq!(chip.role, FetchAge);
        assert_eq!(chip.text, "\u{21e3} 0s");
    }
}
