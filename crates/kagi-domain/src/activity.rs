//! Commit-activity aggregation — pure Rust, no deps.
//!
//! Each [`Granularity`] is a fixed **recent window** anchored at `now`, split
//! into evenly-spaced sub-buckets for the bottom-panel "Activity" line chart:
//!
//! | Granularity | window      | bucket   | buckets |
//! |-------------|-------------|----------|---------|
//! | Day         | last 24 h   | 30 min   | 48      |
//! | Week        | last 7 days | 4 h      | 42      |
//! | Month       | last 30 days| 1 day    | 30      |
//! | Year        | last 365 d  | 1 week   | 52      |
//! | All         | whole history (earliest commit → now) split into 52 buckets |
//!
//! The contributor ranking for each granularity is aggregated over the same
//! window, so toggling re-scopes both the chart and the ranking. Timestamps are
//! `author.time` (Unix epoch seconds, UTC).

use crate::commit::Commit;
use std::collections::BTreeMap;

/// A fixed recent window + chart resolution. `All` covers the whole history
/// (its window is computed from the data, not a fixed length).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Day,
    Week,
    Month,
    Year,
    All,
}

/// Number of sub-buckets the `All` window is split into.
const ALL_BUCKETS: usize = 52;

impl Granularity {
    /// All variants in toggle order.
    pub const ALL: [Granularity; 5] = [
        Granularity::Day,
        Granularity::Week,
        Granularity::Month,
        Granularity::Year,
        Granularity::All,
    ];

    /// Toggle button label.
    pub fn label(self) -> &'static str {
        match self {
            Granularity::Day => "Day",
            Granularity::Week => "Week",
            Granularity::Month => "Month",
            Granularity::Year => "Year",
            Granularity::All => "All",
        }
    }

    /// Human window description (e.g. for the header).
    pub fn window_label(self) -> &'static str {
        match self {
            Granularity::Day => "last 24 hours",
            Granularity::Week => "last 7 days",
            Granularity::Month => "last 30 days",
            Granularity::Year => "last year",
            Granularity::All => "all time",
        }
    }

    /// Fixed window length in seconds (`None` for `All`, which is data-driven).
    fn fixed_window_secs(self) -> Option<i64> {
        Some(match self {
            Granularity::Day => 86_400,
            Granularity::Week => 7 * 86_400,
            Granularity::Month => 30 * 86_400,
            Granularity::Year => 365 * 86_400,
            Granularity::All => return None,
        })
    }

    /// Fixed sub-bucket length in seconds (`None` for `All`).
    fn fixed_bucket_secs(self) -> Option<i64> {
        Some(match self {
            Granularity::Day => 1_800,       // 30 min
            Granularity::Week => 4 * 3_600,  // 4 h
            Granularity::Month => 86_400,    // 1 day
            Granularity::Year => 7 * 86_400, // 1 week
            Granularity::All => return None,
        })
    }
}

/// One sub-bucket: commit / merge counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityBucket {
    pub commits: u32,
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

/// Chart + ranking for one granularity's window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GranularityData {
    /// Sub-buckets, chronological (oldest first).
    pub buckets: Vec<ActivityBucket>,
    /// Contributors in this window, sorted by commit count desc.
    pub contributors: Vec<Contributor>,
    pub total_commits: u32,
    pub total_merges: u32,
    /// Left x-axis label for the window start (e.g. `"−7d"` or, for `All`, the
    /// earliest commit date `"YYYY-MM-DD"`). The right edge is always "now".
    pub start_label: String,
}

/// Aggregated activity for one repository snapshot, per granularity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityData {
    pub day: GranularityData,
    pub week: GranularityData,
    pub month: GranularityData,
    pub year: GranularityData,
    pub all: GranularityData,
}

impl ActivityData {
    pub fn get(&self, g: Granularity) -> &GranularityData {
        match g {
            Granularity::Day => &self.day,
            Granularity::Week => &self.week,
            Granularity::Month => &self.month,
            Granularity::Year => &self.year,
            Granularity::All => &self.all,
        }
    }
}

/// Aggregate a commit slice into [`ActivityData`], using `now` (epoch secs) as
/// the right edge of every window.
pub fn aggregate(commits: &[Commit], now: i64) -> ActivityData {
    ActivityData {
        day: aggregate_one(commits, now, Granularity::Day),
        week: aggregate_one(commits, now, Granularity::Week),
        month: aggregate_one(commits, now, Granularity::Month),
        year: aggregate_one(commits, now, Granularity::Year),
        all: aggregate_one(commits, now, Granularity::All),
    }
}

fn aggregate_one(commits: &[Commit], now: i64, g: Granularity) -> GranularityData {
    // Window [start, now], sub-bucket size, count, and the left x-axis label.
    let (start, n, bucket, start_label) = match (g.fixed_window_secs(), g.fixed_bucket_secs()) {
        (Some(win), Some(bkt)) => {
            let lbl = match g {
                Granularity::Day => "−24h",
                Granularity::Week => "−7d",
                Granularity::Month => "−30d",
                Granularity::Year => "−1y",
                Granularity::All => "",
            };
            (now - win, (win / bkt) as usize, bkt, lbl.to_string())
        }
        // All: span the earliest in-range commit → now, split into N buckets.
        _ => {
            let earliest = commits
                .iter()
                .map(|c| c.author.time)
                .filter(|&t| t <= now)
                .min()
                .unwrap_or(now);
            let span = (now - earliest).max(1);
            let bucket = (span / ALL_BUCKETS as i64).max(1);
            (earliest, ALL_BUCKETS, bucket, fmt_ymd(earliest))
        }
    };

    let mut buckets = vec![ActivityBucket::default(); n];
    let mut authors: BTreeMap<String, Contributor> = BTreeMap::new();
    let mut total_commits = 0u32;
    let mut total_merges = 0u32;

    for c in commits {
        let t = c.author.time;
        if t < start || t > now {
            continue;
        }
        let idx = (((t - start) / bucket) as usize).min(n - 1);
        let is_merge = c.parents.len() >= 2;
        buckets[idx].commits += 1;
        total_commits += 1;
        if is_merge {
            buckets[idx].merges += 1;
            total_merges += 1;
        }
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

    let mut contributors: Vec<Contributor> = authors.into_values().collect();
    contributors.sort_by(|a, b| {
        b.commits
            .cmp(&a.commits)
            .then(b.merges.cmp(&a.merges))
            .then(a.name.cmp(&b.name))
    });

    GranularityData {
        buckets,
        contributors,
        total_commits,
        total_merges,
        start_label,
    }
}

/// `"YYYY-MM-DD"` for an epoch-seconds instant (UTC).
fn fmt_ymd(epoch: i64) -> String {
    let (y, m, d) = civil_from_days(epoch.div_euclid(86_400));
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Howard Hinnant's `civil_from_days`: days-since-epoch → `(year, month, day)`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
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

    const NOW: i64 = 1_700_000_000;

    #[test]
    fn bucket_counts() {
        let a = aggregate(&[], NOW);
        assert_eq!(a.get(Granularity::Day).buckets.len(), 48);
        assert_eq!(a.get(Granularity::Week).buckets.len(), 42);
        assert_eq!(a.get(Granularity::Month).buckets.len(), 30);
        assert_eq!(a.get(Granularity::Year).buckets.len(), 52);
    }

    #[test]
    fn all_window_spans_history() {
        let commits = vec![
            commit(NOW - 400 * 86_400, 1, "Old", "old@x"), // >1y ago
            commit(NOW - 3600, 1, "New", "new@x"),
        ];
        let a = aggregate(&commits, NOW);
        let all = a.get(Granularity::All);
        assert_eq!(all.total_commits, 2); // All includes the >1y-old commit
        assert_eq!(all.buckets.len(), 52);
        assert!(all.start_label.starts_with("20")); // a YYYY-.. date
                                                    // Year window excludes the >1y-old one.
        assert_eq!(a.get(Granularity::Year).total_commits, 1);
    }

    #[test]
    fn empty_input() {
        let a = aggregate(&[], NOW);
        let d = a.get(Granularity::Day);
        assert_eq!(d.buckets.len(), 48);
        assert!(d.contributors.is_empty());
        assert_eq!((d.total_commits, d.total_merges), (0, 0));
    }

    #[test]
    fn day_window_excludes_older_than_24h() {
        let commits = vec![
            commit(NOW - 3600, 1, "A", "a@x"),    // 1h ago — in
            commit(NOW - 100_000, 1, "B", "b@x"), // >24h ago — out of Day
        ];
        let day = aggregate(&commits, NOW).get(Granularity::Day).clone();
        assert_eq!(day.total_commits, 1);
        assert_eq!(day.contributors.len(), 1);
        assert_eq!(day.contributors[0].email, "a@x");
        // But the Week/Month windows include the older one.
        let month = aggregate(&commits, NOW).get(Granularity::Month).clone();
        assert_eq!(month.total_commits, 2);
    }

    #[test]
    fn buckets_land_in_the_right_slot() {
        // Day: 30-min buckets. A commit 1h ago → 23 buckets before the end.
        let a = aggregate(&[commit(NOW - 3600, 1, "A", "a@x")], NOW);
        let d = a.get(Granularity::Day);
        // start = NOW-86400; idx = (86400-3600)/1800 = 46.
        assert_eq!(d.buckets[46].commits, 1);
        assert_eq!(d.total_commits, 1);
    }

    #[test]
    fn merges_counted_separately() {
        let commits = vec![
            commit(NOW - 100, 1, "A", "a@x"),
            commit(NOW - 200, 2, "A", "a@x"),
        ];
        let d = aggregate(&commits, NOW).get(Granularity::Day).clone();
        assert_eq!((d.total_commits, d.total_merges), (2, 1));
        assert_eq!(
            (d.contributors[0].commits, d.contributors[0].merges),
            (2, 1)
        );
    }
}
