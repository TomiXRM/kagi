//! Per-worktree port allocation + the `KAGI_*` environment map (issue #342,
//! ADR-0171).
//!
//! **Pure model only — no I/O, no settings, no git.** Persistence (a JSON store
//! keyed by canonical worktree path) and the settings that feed the range live
//! in the outer layers (`kagi-git` / `kagi-ui-core`); this crate must stay
//! dependency-free per the layering invariant. Both functions here are total and
//! deterministic, so the allocation logic and the env contract are unit-tested
//! in isolation.
//!
//! ## What "allocation" means (v1)
//!
//! We assign **numbers only — we do not bind sockets**. A worktree gets a block
//! of `ports_per_worktree` consecutive ports starting at [`allocate_block`]'s
//! return value; the block's first port becomes `KAGI_PORT`. Because nothing is
//! actually bound, a number handed out here can still be taken by an unrelated
//! process before the user's dev server grabs it — that race is accepted for v1
//! and documented in the ADR. Binding-to-reserve is a follow-up.

use std::collections::BTreeMap;

/// An inclusive port range parsed from the `"start-end"` settings string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    /// Number of ports in the (inclusive) range.
    pub fn len(self) -> u32 {
        if self.end < self.start {
            0
        } else {
            (self.end as u32) - (self.start as u32) + 1
        }
    }

    /// True when the range holds no ports (`end < start`).
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}

/// Parse `"3000-3099"` into a [`PortRange`]. Returns `None` for anything that is
/// not two `u16`s separated by a single `-` with `start <= end`.
pub fn parse_port_range(s: &str) -> Option<PortRange> {
    let (a, b) = s.trim().split_once('-')?;
    let start = a.trim().parse::<u16>().ok()?;
    let end = b.trim().parse::<u16>().ok()?;
    if start > end {
        return None;
    }
    Some(PortRange { start, end })
}

/// The five `KAGI_*` environment variable names injected per worktree, in the
/// order [`env_map`] returns them. Public so callers/tests can reference the
/// contract by name rather than string literals.
pub const ENV_KEYS: [&str; 5] = [
    "KAGI_WORKTREE_PATH",
    "KAGI_WORKTREE_NAME",
    "KAGI_MAIN_WORKTREE",
    "KAGI_DEFAULT_BRANCH",
    "KAGI_PORT",
];

/// Compute the first port of this worktree's consecutive block.
///
/// * Idempotent: if `target` already appears in `assigned`, its stored first
///   port is returned unchanged — the whole point of persisting assignments is
///   that a worktree keeps the same block across kagi restarts and across
///   sibling worktrees being created or removed.
/// * Otherwise the lowest **aligned** block of `per` consecutive ports that
///   fits inside `range` and overlaps no other assigned block is returned
///   (aligned = `start`, `start + per`, `start + 2·per`, …, matching the
///   `3000 / 3010 / 3020` layout in the issue).
/// * Returns `None` when `per` is 0, the range is empty, or every aligned block
///   in the range is already taken (**exhaustion** — the caller surfaces this
///   rather than handing out an out-of-range or overlapping port).
///
/// `assigned` maps *other* worktrees' keys to their first port; each is treated
/// as occupying `[first, first + per - 1]`.
pub fn allocate_block(
    range: PortRange,
    per: u16,
    assigned: &BTreeMap<String, u16>,
    target: &str,
) -> Option<u16> {
    // Idempotent: an already-assigned worktree keeps its block.
    if let Some(&existing) = assigned.get(target) {
        return Some(existing);
    }
    if per == 0 || range.is_empty() {
        return None;
    }
    let per32 = per as u32;

    // Occupied intervals from the *other* worktrees.
    let occupied: Vec<(u32, u32)> = assigned
        .iter()
        .filter(|(k, _)| k.as_str() != target)
        .map(|(_, &first)| {
            let f = first as u32;
            (f, f + per32 - 1)
        })
        .collect();

    let mut candidate = range.start as u32;
    let range_end = range.end as u32;
    while candidate + per32 - 1 <= range_end {
        let block = (candidate, candidate + per32 - 1);
        let overlaps = occupied
            .iter()
            .any(|&(lo, hi)| block.0 <= hi && lo <= block.1);
        if !overlaps {
            return Some(candidate as u16);
        }
        candidate += per32;
    }
    None
}

/// Build the ordered `KAGI_*` environment map for one worktree. Pure string
/// assembly — the caller supplies every input (path, name, main worktree path,
/// default branch, and the block's first port from [`allocate_block`]).
///
/// Order matches [`ENV_KEYS`]. Paths are emitted with `Path::display()`.
pub fn env_map(
    worktree_path: &std::path::Path,
    worktree_name: &str,
    main_worktree_path: &std::path::Path,
    default_branch: &str,
    first_port: u16,
) -> Vec<(&'static str, String)> {
    vec![
        ("KAGI_WORKTREE_PATH", worktree_path.display().to_string()),
        ("KAGI_WORKTREE_NAME", worktree_name.to_string()),
        (
            "KAGI_MAIN_WORKTREE",
            main_worktree_path.display().to_string(),
        ),
        ("KAGI_DEFAULT_BRANCH", default_branch.to_string()),
        ("KAGI_PORT", first_port.to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn map(pairs: &[(&str, u16)]) -> BTreeMap<String, u16> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn parse_range_ok_and_bad() {
        assert_eq!(
            parse_port_range("3000-3099"),
            Some(PortRange {
                start: 3000,
                end: 3099
            })
        );
        // Surrounding and inner whitespace is tolerated.
        assert_eq!(
            parse_port_range(" 3000 - 3099 "),
            Some(PortRange {
                start: 3000,
                end: 3099
            })
        );
        assert_eq!(parse_port_range("3099-3000"), None); // start > end
        assert_eq!(parse_port_range("3000"), None); // no dash
        assert_eq!(parse_port_range("a-b"), None);
    }

    #[test]
    fn three_worktrees_get_three_different_consecutive_blocks() {
        // AC1: allocate three in sequence, feeding each result back in.
        let range = PortRange {
            start: 3000,
            end: 3099,
        };
        let mut assigned = BTreeMap::new();

        let a = allocate_block(range, 10, &assigned, "a").unwrap();
        assigned.insert("a".to_string(), a);
        let b = allocate_block(range, 10, &assigned, "b").unwrap();
        assigned.insert("b".to_string(), b);
        let c = allocate_block(range, 10, &assigned, "c").unwrap();
        assigned.insert("c".to_string(), c);

        assert_eq!((a, b, c), (3000, 3010, 3020));
        // All three distinct and their [first, first+9] blocks do not overlap.
        assert!(a != b && b != c && a != c);
    }

    #[test]
    fn already_assigned_worktree_keeps_its_block() {
        // Idempotency: even with the range "full" of siblings, the target's own
        // stored block is returned unchanged.
        let range = PortRange {
            start: 3000,
            end: 3099,
        };
        let assigned = map(&[("a", 3000), ("b", 3010), ("keepme", 3020)]);
        assert_eq!(allocate_block(range, 10, &assigned, "keepme"), Some(3020));
        // Repeated calls are stable.
        assert_eq!(allocate_block(range, 10, &assigned, "keepme"), Some(3020));
    }

    #[test]
    fn new_worktree_fills_first_hole() {
        // A removed sibling frees its block; the next new worktree reuses it.
        let range = PortRange {
            start: 3000,
            end: 3099,
        };
        let assigned = map(&[("a", 3000), ("c", 3020)]); // 3010 is a hole
        assert_eq!(allocate_block(range, 10, &assigned, "new"), Some(3010));
    }

    #[test]
    fn allocation_is_bounded_by_range_and_exhaustion_is_none() {
        // Range holds exactly two blocks of 10.
        let range = PortRange {
            start: 3000,
            end: 3019,
        };
        let assigned = map(&[("a", 3000), ("b", 3010)]);
        assert_eq!(allocate_block(range, 10, &assigned, "c"), None);
        // per larger than the range: nothing fits.
        assert_eq!(
            allocate_block(
                PortRange {
                    start: 3000,
                    end: 3004
                },
                10,
                &BTreeMap::new(),
                "x"
            ),
            None
        );
        // per == 0 is rejected.
        assert_eq!(allocate_block(range, 0, &BTreeMap::new(), "x"), None);
    }

    #[test]
    fn env_map_has_all_five_vars_with_correct_values() {
        let m = env_map(
            Path::new("/repo/wt-feature"),
            "wt-feature",
            Path::new("/repo"),
            "main",
            3010,
        );
        let keys: Vec<&str> = m.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ENV_KEYS.to_vec());
        let get = |k: &str| m.iter().find(|(kk, _)| *kk == k).map(|(_, v)| v.as_str());
        assert_eq!(get("KAGI_WORKTREE_PATH"), Some("/repo/wt-feature"));
        assert_eq!(get("KAGI_WORKTREE_NAME"), Some("wt-feature"));
        assert_eq!(get("KAGI_MAIN_WORKTREE"), Some("/repo"));
        assert_eq!(get("KAGI_DEFAULT_BRANCH"), Some("main"));
        assert_eq!(get("KAGI_PORT"), Some("3010"));
    }
}
