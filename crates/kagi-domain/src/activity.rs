//! Commit-activity aggregation — pure Rust, no deps.
//!
//! Buckets a commit history into per-day / per-week / per-month counts (total
//! commits and merge commits) for the bottom-panel "Activity" line chart, and
//! ranks contributors. Line-stat fields (`additions`/`deletions`) are reserved
//! here but filled by the git backend (a per-commit diff pass), since the pure
//! domain has no access to file contents.
//!
//! All timestamps are `author.time` (Unix epoch seconds, UTC). Date maths uses
//! Howard Hinnant's civil-from-days algorithm so we stay dependency-free.

use crate::commit::Commit;
use std::collections::BTreeMap;

/// Time bucket size for the activity chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Day,
    Week,
    Month,
}

impl Granularity {
    /// Short label for the toggle button.
    pub fn label(self) -> &'static str {
        match self {
            Granularity::Day => "Day",
            Granularity::Week => "Week",
            Granularity::Month => "Month",
        }
    }
}

/// One time bucket: commit / merge counts plus a short axis label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityBucket {
    /// Compact label, e.g. `"06-21"` (day/week-start) or `"2026-06"` (month).
    pub label: String,
    /// Total commits whose author time fell in this bucket.
    pub commits: u32,
    /// Of those, how many were merge commits (2+ parents).
    pub merges: u32,
}

/// Per-author aggregate for the contributor ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contributor {
    pub name: String,
    pub email: String,
    pub commits: u32,
    pub merges: u32,
    /// Added lines across this author's commits. Filled by the git backend
    /// (0 from pure aggregation).
    pub additions: u64,
    /// Deleted lines across this author's commits. Filled by the git backend.
    pub deletions: u64,
}

/// Aggregated activity for one repository snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityData {
    /// Per-day buckets (most recent [`DAY_BUCKETS`], chronological).
    pub day: Vec<ActivityBucket>,
    /// Per-week buckets (Monday-aligned, most recent [`WEEK_BUCKETS`]).
    pub week: Vec<ActivityBucket>,
    /// Per-month buckets (most recent [`MONTH_BUCKETS`]).
    pub month: Vec<ActivityBucket>,
    /// Contributors sorted by commit count desc (UI shows the top N).
    pub contributors: Vec<Contributor>,
    pub total_commits: u32,
    pub total_merges: u32,
}

impl ActivityData {
    /// The bucket series for a given [`Granularity`].
    pub fn series(&self, g: Granularity) -> &[ActivityBucket] {
        match g {
            Granularity::Day => &self.day,
            Granularity::Week => &self.week,
            Granularity::Month => &self.month,
        }
    }
}

/// How many trailing buckets each granularity keeps (chart stays readable).
pub const DAY_BUCKETS: usize = 60;
pub const WEEK_BUCKETS: usize = 52;
pub const MONTH_BUCKETS: usize = 24;

const SECS_PER_DAY: i64 = 86_400;

/// Aggregate a commit slice into [`ActivityData`].
pub fn aggregate(commits: &[Commit]) -> ActivityData {
    // Per-bucket (key -> (commits, merges)) maps, ordered by key.
    let mut day: BTreeMap<i64, (u32, u32)> = BTreeMap::new();
    let mut week: BTreeMap<i64, (u32, u32)> = BTreeMap::new();
    let mut month: BTreeMap<i64, (u32, u32)> = BTreeMap::new();
    // email -> contributor aggregate.
    let mut authors: BTreeMap<String, Contributor> = BTreeMap::new();

    let mut total_commits: u32 = 0;
    let mut total_merges: u32 = 0;

    for c in commits {
        let is_merge = c.parents.len() >= 2;
        total_commits += 1;
        if is_merge {
            total_merges += 1;
        }

        let day_idx = c.author.time.div_euclid(SECS_PER_DAY);
        // Monday-aligned week index (epoch day 0 = Thursday → +3).
        let week_idx = (day_idx + 3).div_euclid(7);
        let (y, m, _d) = civil_from_days(day_idx);
        let month_idx = y * 12 + (m as i64 - 1);

        for (map, key) in [
            (&mut day, day_idx),
            (&mut week, week_idx),
            (&mut month, month_idx),
        ] {
            let e = map.entry(key).or_insert((0, 0));
            e.0 += 1;
            if is_merge {
                e.1 += 1;
            }
        }

        let entry = authors
            .entry(c.author.email.clone())
            .or_insert_with(|| Contributor {
                name: c.author.name.clone(),
                email: c.author.email.clone(),
                commits: 0,
                merges: 0,
                additions: 0,
                deletions: 0,
            });
        entry.commits += 1;
        if is_merge {
            entry.merges += 1;
        }
    }

    let mut contributors: Vec<Contributor> = authors.into_values().collect();
    contributors.sort_by(|a, b| {
        b.commits
            .cmp(&a.commits)
            .then(b.merges.cmp(&a.merges))
            .then(a.name.cmp(&b.name))
    });

    ActivityData {
        day: build_series(&day, DAY_BUCKETS, |k| label_day(k)),
        week: build_series(&week, WEEK_BUCKETS, |k| label_day(k * 7 - 3)),
        month: build_series(&month, MONTH_BUCKETS, label_month),
        contributors,
        total_commits,
        total_merges,
    }
}

/// Take the most recent `n` buckets (clamped to the available range), filling
/// gaps with zero so the line chart has contiguous, evenly-spaced points.
fn build_series(
    counts: &BTreeMap<i64, (u32, u32)>,
    n: usize,
    label: impl Fn(i64) -> String,
) -> Vec<ActivityBucket> {
    let (Some(&min_key), Some(&max_key)) = (counts.keys().next(), counts.keys().next_back()) else {
        return Vec::new();
    };
    let start = (max_key - (n as i64 - 1)).max(min_key);
    (start..=max_key)
        .map(|k| {
            let (commits, merges) = counts.get(&k).copied().unwrap_or((0, 0));
            ActivityBucket {
                label: label(k),
                commits,
                merges,
            }
        })
        .collect()
}

/// `"MM-DD"` label for a day index (days since the Unix epoch).
fn label_day(day_idx: i64) -> String {
    let (_y, m, d) = civil_from_days(day_idx);
    format!("{:02}-{:02}", m, d)
}

/// `"YYYY-MM"` label for a month key (`year * 12 + (month - 1)`).
fn label_month(month_key: i64) -> String {
    let y = month_key.div_euclid(12);
    let m = month_key.rem_euclid(12) + 1;
    format!("{:04}-{:02}", y, m)
}

/// Howard Hinnant's `civil_from_days`: convert days-since-epoch to
/// `(year, month, day)` (proleptic Gregorian, UTC). Pure integer maths.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::{CommitId, Signature};

    fn commit(time: i64, parents: usize, name: &str, email: &str) -> Commit {
        Commit {
            id: CommitId("x".into()),
            parents: (0..parents).map(|i| CommitId(format!("p{i}"))).collect(),
            author: Signature {
                name: name.into(),
                email: email.into(),
                time,
            },
            committer: Signature {
                name: name.into(),
                email: email.into(),
                time,
            },
            summary: "s".into(),
            message: "m".into(),
        }
    }

    // 2021-01-01 00:00:00 UTC = 1609459200; that day index:
    const D_2021_01_01: i64 = 1_609_459_200;

    #[test]
    fn empty_input() {
        let a = aggregate(&[]);
        assert!(a.day.is_empty() && a.week.is_empty() && a.month.is_empty());
        assert!(a.contributors.is_empty());
        assert_eq!(a.total_commits, 0);
        assert_eq!(a.total_merges, 0);
    }

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(D_2021_01_01 / SECS_PER_DAY), (2021, 1, 1));
    }

    #[test]
    fn counts_commits_and_merges_per_day() {
        let t = D_2021_01_01;
        let commits = vec![
            commit(t, 1, "A", "a@x"),                // normal
            commit(t + 100, 2, "A", "a@x"),          // merge, same day
            commit(t + SECS_PER_DAY, 1, "B", "b@x"), // next day
        ];
        let a = aggregate(&commits);
        assert_eq!(a.total_commits, 3);
        assert_eq!(a.total_merges, 1);
        // Two contiguous days.
        assert_eq!(a.day.len(), 2);
        assert_eq!((a.day[0].commits, a.day[0].merges), (2, 1));
        assert_eq!((a.day[1].commits, a.day[1].merges), (1, 0));
    }

    #[test]
    fn fills_gaps_between_days() {
        let t = D_2021_01_01;
        // day 0 and day 3 (gap of 2 empty days between).
        let commits = vec![
            commit(t, 1, "A", "a@x"),
            commit(t + 3 * SECS_PER_DAY, 1, "A", "a@x"),
        ];
        let a = aggregate(&commits);
        assert_eq!(a.day.len(), 4);
        assert_eq!(
            a.day.iter().map(|b| b.commits).collect::<Vec<_>>(),
            vec![1, 0, 0, 1]
        );
    }

    #[test]
    fn contributor_ranking_by_commits_then_merges() {
        let t = D_2021_01_01;
        let commits = vec![
            commit(t, 1, "Alice", "a@x"),
            commit(t, 1, "Alice", "a@x"),
            commit(t, 2, "Alice", "a@x"), // alice: 3 commits, 1 merge
            commit(t, 1, "Bob", "b@x"),
            commit(t, 2, "Bob", "b@x"), // bob: 2 commits, 1 merge
        ];
        let a = aggregate(&commits);
        assert_eq!(a.contributors.len(), 2);
        assert_eq!(a.contributors[0].email, "a@x");
        assert_eq!(
            (a.contributors[0].commits, a.contributors[0].merges),
            (3, 1)
        );
        assert_eq!(a.contributors[1].email, "b@x");
        assert_eq!(
            (a.contributors[1].commits, a.contributors[1].merges),
            (2, 1)
        );
    }

    #[test]
    fn month_label_format() {
        let a = aggregate(&[commit(D_2021_01_01, 1, "A", "a@x")]);
        assert_eq!(a.month.len(), 1);
        assert_eq!(a.month[0].label, "2021-01");
    }
}
