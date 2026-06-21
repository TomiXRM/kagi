//! Commit-activity aggregation — pure Rust, no deps.
//!
//! Buckets a commit history into per-day / per-week / per-month counts (total
//! commits and merge commits) for the bottom-panel "Activity" line chart, and
//! ranks contributors **within each granularity's visible window** (so toggling
//! Day/Week/Month re-scopes the ranking too).
//!
//! All timestamps are `author.time` (Unix epoch seconds, UTC). Date maths uses
//! Howard Hinnant's civil/days algorithms so we stay dependency-free.

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
}

/// Aggregated activity for one repository snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityData {
    pub day: Vec<ActivityBucket>,
    pub week: Vec<ActivityBucket>,
    pub month: Vec<ActivityBucket>,
    /// Contributors scoped to the Day window (sorted by commit count desc).
    pub day_contributors: Vec<Contributor>,
    pub week_contributors: Vec<Contributor>,
    pub month_contributors: Vec<Contributor>,
    /// Window start (epoch secs): a commit with `author.time >= cutoff` is in
    /// the corresponding granularity's window. `i64::MAX` when empty.
    pub day_cutoff: i64,
    pub week_cutoff: i64,
    pub month_cutoff: i64,
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

    /// The contributor ranking for a given [`Granularity`] (windowed).
    pub fn contributors(&self, g: Granularity) -> &[Contributor] {
        match g {
            Granularity::Day => &self.day_contributors,
            Granularity::Week => &self.week_contributors,
            Granularity::Month => &self.month_contributors,
        }
    }

    /// Window start (epoch secs) for a given [`Granularity`].
    pub fn cutoff(&self, g: Granularity) -> i64 {
        match g {
            Granularity::Day => self.day_cutoff,
            Granularity::Week => self.week_cutoff,
            Granularity::Month => self.month_cutoff,
        }
    }

    /// Total (commits, merges) shown in the chart window for a granularity.
    pub fn window_totals(&self, g: Granularity) -> (u32, u32) {
        self.series(g)
            .iter()
            .fold((0u32, 0u32), |(c, m), b| (c + b.commits, m + b.merges))
    }
}

/// How many trailing buckets each granularity keeps (chart stays readable and
/// the line-stat window stays cheap to diff).
pub const DAY_BUCKETS: usize = 60;
pub const WEEK_BUCKETS: usize = 52;
pub const MONTH_BUCKETS: usize = 24;

const SECS_PER_DAY: i64 = 86_400;

/// Aggregate a commit slice into [`ActivityData`].
pub fn aggregate(commits: &[Commit]) -> ActivityData {
    // Pass 1: per-bucket (key -> (commits, merges)) maps, ordered by key.
    let mut day: BTreeMap<i64, (u32, u32)> = BTreeMap::new();
    let mut week: BTreeMap<i64, (u32, u32)> = BTreeMap::new();
    let mut month: BTreeMap<i64, (u32, u32)> = BTreeMap::new();

    for c in commits {
        let is_merge = c.parents.len() >= 2;
        let day_idx = c.author.time.div_euclid(SECS_PER_DAY);
        let week_idx = (day_idx + 3).div_euclid(7); // Monday-aligned
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
    }

    // Window cutoffs (epoch secs) derived from the trailing-N bucket start.
    let day_cutoff = window_start_key(&day, DAY_BUCKETS)
        .map(|k| k * SECS_PER_DAY)
        .unwrap_or(i64::MAX);
    let week_cutoff = window_start_key(&week, WEEK_BUCKETS)
        .map(|k| (k * 7 - 3) * SECS_PER_DAY)
        .unwrap_or(i64::MAX);
    let month_cutoff = window_start_key(&month, MONTH_BUCKETS)
        .map(|k| {
            let y = k.div_euclid(12);
            let m = (k.rem_euclid(12) + 1) as u32;
            days_from_civil(y, m, 1) * SECS_PER_DAY
        })
        .unwrap_or(i64::MAX);

    ActivityData {
        day: build_series(&day, DAY_BUCKETS, |k| label_day(k)),
        week: build_series(&week, WEEK_BUCKETS, |k| label_day(k * 7 - 3)),
        month: build_series(&month, MONTH_BUCKETS, label_month),
        day_contributors: rank_window(commits, day_cutoff),
        week_contributors: rank_window(commits, week_cutoff),
        month_contributors: rank_window(commits, month_cutoff),
        day_cutoff,
        week_cutoff,
        month_cutoff,
    }
}

/// Rank contributors over commits with `author.time >= cutoff`.
fn rank_window(commits: &[Commit], cutoff: i64) -> Vec<Contributor> {
    let mut authors: BTreeMap<String, Contributor> = BTreeMap::new();
    for c in commits {
        if c.author.time < cutoff {
            continue;
        }
        let is_merge = c.parents.len() >= 2;
        let e = authors
            .entry(c.author.email.clone())
            .or_insert_with(|| Contributor {
                name: c.author.name.clone(),
                email: c.author.email.clone(),
                commits: 0,
                merges: 0,
            });
        e.commits += 1;
        if is_merge {
            e.merges += 1;
        }
    }
    let mut v: Vec<Contributor> = authors.into_values().collect();
    v.sort_by(|a, b| {
        b.commits
            .cmp(&a.commits)
            .then(b.merges.cmp(&a.merges))
            .then(a.name.cmp(&b.name))
    });
    v
}

/// The key of the first bucket shown in the trailing-`n` window (clamped to the
/// available range), or `None` when there are no commits.
fn window_start_key(counts: &BTreeMap<i64, (u32, u32)>, n: usize) -> Option<i64> {
    let min = *counts.keys().next()?;
    let max = *counts.keys().next_back()?;
    Some((max - (n as i64 - 1)).max(min))
}

/// Take the most recent `n` buckets (clamped to the available range), filling
/// gaps with zero so the line chart has contiguous, evenly-spaced points.
fn build_series(
    counts: &BTreeMap<i64, (u32, u32)>,
    n: usize,
    label: impl Fn(i64) -> String,
) -> Vec<ActivityBucket> {
    let Some(start) = window_start_key(counts, n) else {
        return Vec::new();
    };
    let max_key = *counts.keys().next_back().unwrap();
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

/// Howard Hinnant's `civil_from_days`: days-since-epoch → `(year, month, day)`.
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

/// Inverse of [`civil_from_days`]: `(year, month, day)` → days-since-epoch.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
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

    const D_2021_01_01: i64 = 1_609_459_200;

    #[test]
    fn empty_input() {
        let a = aggregate(&[]);
        assert!(a.day.is_empty() && a.week.is_empty() && a.month.is_empty());
        assert!(a.contributors(Granularity::Week).is_empty());
        assert_eq!(a.window_totals(Granularity::Day), (0, 0));
    }

    #[test]
    fn civil_roundtrip() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        let d = D_2021_01_01 / SECS_PER_DAY;
        let (y, m, day) = civil_from_days(d);
        assert_eq!((y, m, day), (2021, 1, 1));
        assert_eq!(days_from_civil(y, m, day), d);
    }

    #[test]
    fn counts_commits_and_merges_per_day() {
        let t = D_2021_01_01;
        let commits = vec![
            commit(t, 1, "A", "a@x"),
            commit(t + 100, 2, "A", "a@x"),
            commit(t + SECS_PER_DAY, 1, "B", "b@x"),
        ];
        let a = aggregate(&commits);
        assert_eq!(a.window_totals(Granularity::Day), (3, 1));
        assert_eq!(a.day.len(), 2);
        assert_eq!((a.day[0].commits, a.day[0].merges), (2, 1));
        assert_eq!((a.day[1].commits, a.day[1].merges), (1, 0));
    }

    #[test]
    fn fills_gaps_between_days() {
        let t = D_2021_01_01;
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
    fn contributor_ranking_windowed_by_granularity() {
        // Two commits long ago (outside the 60-day window) + recent ones.
        let recent = D_2021_01_01;
        let old = recent - 200 * SECS_PER_DAY; // ~200 days earlier
        let commits = vec![
            commit(old, 1, "Old", "old@x"),
            commit(recent, 1, "Alice", "a@x"),
            commit(recent, 2, "Alice", "a@x"),
            commit(recent, 1, "Bob", "b@x"),
        ];
        let a = aggregate(&commits);
        // Day window (60d) excludes the old commit.
        let day = a.contributors(Granularity::Day);
        assert!(!day.iter().any(|c| c.email == "old@x"));
        assert_eq!(day[0].email, "a@x");
        assert_eq!((day[0].commits, day[0].merges), (2, 1));
        // Month window (24mo) includes the old commit.
        let month = a.contributors(Granularity::Month);
        assert!(month.iter().any(|c| c.email == "old@x"));
    }

    #[test]
    fn month_label_format() {
        let a = aggregate(&[commit(D_2021_01_01, 1, "A", "a@x")]);
        assert_eq!(a.month.len(), 1);
        assert_eq!(a.month[0].label, "2021-01");
    }
}
