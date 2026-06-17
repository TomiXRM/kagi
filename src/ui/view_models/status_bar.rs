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
}
