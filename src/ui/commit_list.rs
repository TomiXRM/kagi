//! Commit list row data and badge helpers — T008
//!
//! All display strings are pre-computed at snapshot time; the render closure
//! only clones SharedString values, never calling format! per frame.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::SharedString;

use crate::git::{Commit, CommitId, Head, RepoSnapshot};

// ──────────────────────────────────────────────────────────────
// Badge types
// ──────────────────────────────────────────────────────────────

/// The kind of a ref badge shown on a commit row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadgeKind {
    /// Current HEAD (attached to this branch tip).
    HeadBranch,
    /// Local branch (not HEAD branch).
    Branch,
    /// Remote-tracking branch (e.g. `origin/main`).
    Remote,
    /// Tag.
    Tag,
}

/// A single ref badge to be displayed on a commit row.
#[derive(Debug, Clone)]
pub struct RefBadge {
    pub kind: BadgeKind,
    /// Display label, e.g. `"main ✓"`, `"origin/main"`, `"v0.1.0"`.
    pub label: SharedString,
}

// ──────────────────────────────────────────────────────────────
// Badge map helper
// ──────────────────────────────────────────────────────────────

/// Build a `CommitId → Vec<RefBadge>` map from a [`RepoSnapshot`].
///
/// The HEAD branch badge integrates the HEAD indicator (`✓`) so we don't
/// show a separate HEAD chip when attached.
pub fn build_badge_map(snap: &RepoSnapshot) -> HashMap<CommitId, Vec<RefBadge>> {
    let mut map: HashMap<CommitId, Vec<RefBadge>> = HashMap::new();

    // Determine the HEAD branch name and target (when attached).
    let head_branch_name: Option<&str> = match &snap.head {
        Head::Attached { branch, .. } => Some(branch.as_str()),
        _ => None,
    };

    // Local branches.
    for b in &snap.branches {
        let is_head_branch = head_branch_name == Some(b.name.as_str());
        let label = if is_head_branch {
            SharedString::from(format!("{} ✓", b.name))
        } else {
            SharedString::from(b.name.clone())
        };
        let kind = if is_head_branch {
            BadgeKind::HeadBranch
        } else {
            BadgeKind::Branch
        };
        map.entry(b.target.clone())
            .or_default()
            .push(RefBadge { kind, label });
    }

    // Detached HEAD: add a standalone HEAD badge.
    if let Head::Detached { target } = &snap.head {
        let commit_id = CommitId(target.clone());
        map.entry(commit_id)
            .or_default()
            .insert(0, RefBadge {
                kind: BadgeKind::HeadBranch,
                label: SharedString::from("HEAD"),
            });
    }

    // Remote-tracking branches.
    for rb in &snap.remote_branches {
        let label = SharedString::from(format!("{}/{}", rb.remote, rb.name));
        map.entry(rb.target.clone())
            .or_default()
            .push(RefBadge { kind: BadgeKind::Remote, label });
    }

    // Tags.
    for t in &snap.tags {
        let label = SharedString::from(t.name.clone());
        map.entry(t.target.clone())
            .or_default()
            .push(RefBadge { kind: BadgeKind::Tag, label });
    }

    map
}

// ──────────────────────────────────────────────────────────────
// Pre-computed row data
// ──────────────────────────────────────────────────────────────

/// Pre-computed display data for one commit row.
///
/// All strings are [`SharedString`] so the render closure can cheaply clone
/// them without re-allocating.
#[derive(Clone)]
pub struct CommitRow {
    /// Short (8-hex) commit id.
    pub short_id: SharedString,
    /// First line of the commit message (truncated to 72 chars at build time).
    pub summary: SharedString,
    /// Author name.
    pub author: SharedString,
    /// Relative date string, e.g. `"3d ago"`, `"2y ago"`.
    pub date: SharedString,
    /// Ref badges for this commit, if any.
    pub badges: Vec<RefBadge>,
}

/// Build the full list of [`CommitRow`]s from a snapshot, pre-computing all
/// display strings.  This is called once when the snapshot is ingested; the
/// render closure only clones SharedStrings.
pub fn build_commit_rows(snap: &RepoSnapshot) -> Vec<CommitRow> {
    let badge_map = build_badge_map(snap);
    let now_secs = now_unix_secs();

    snap.commits
        .iter()
        .map(|c| commit_to_row(c, &badge_map, now_secs))
        .collect()
}

fn commit_to_row(
    c: &Commit,
    badge_map: &HashMap<CommitId, Vec<RefBadge>>,
    now_secs: i64,
) -> CommitRow {
    let short_id = SharedString::from(c.id.short().to_string());

    // Truncate summary at 72 chars to keep rows manageable.
    // Count chars (not bytes): byte slicing would panic on multi-byte
    // summaries (e.g. Japanese commit messages).
    let summary = if c.summary.chars().count() > 72 {
        let truncated: String = c.summary.chars().take(71).collect();
        SharedString::from(format!("{truncated}…"))
    } else {
        SharedString::from(c.summary.clone())
    };

    let author = SharedString::from(c.author.name.clone());
    let date = SharedString::from(relative_time(c.author.time, now_secs));
    let badges = badge_map.get(&c.id).cloned().unwrap_or_default();

    CommitRow { short_id, summary, author, date, badges }
}

// ──────────────────────────────────────────────────────────────
// Relative time helper (no external crates)
// ──────────────────────────────────────────────────────────────

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
