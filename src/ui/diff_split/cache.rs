//! Retention policy for the existing split-diff derivation cache.

use std::collections::HashSet;
use std::mem::size_of;
use std::sync::{Arc, Weak};

use super::{moved_rows, split_rows, DiffRow, SplitDiffRow};

const MAX_ENTRIES: usize = 8;
// Limit cache-owned derived allocations to 8 MiB, independent of source size.
// Values still held by an active renderer are outside the retention budget.
const MAX_DERIVED_BYTES: usize = 8 * 1024 * 1024;

pub(crate) struct SplitProjection {
    pub(crate) rows: Vec<SplitDiffRow>,
    pub(crate) moved: HashSet<usize>,
}

impl SplitProjection {
    fn retained_bytes(&self) -> usize {
        let pairs = self
            .rows
            .capacity()
            .saturating_mul(size_of::<SplitDiffRow>());
        // The standard SwissTable has spare buckets and one control byte per
        // bucket. Two buckets per usable capacity slot conservatively cover
        // its load factor and power-of-two rounding; 64 bytes cover the trailing
        // control group and alignment. Count capacity, never just live marks.
        let marks = if self.moved.capacity() == 0 {
            0
        } else {
            self.moved
                .capacity()
                .saturating_add(1)
                .saturating_mul(2 * (size_of::<usize>() + 1))
                .saturating_add(64)
        };
        // Include the projection's Arc header and the weak source's allocation
        // header/Vec shell (not its buffer, which the cache does not retain).
        pairs
            .saturating_add(marks)
            .saturating_add(size_of::<Self>())
            .saturating_add(size_of::<Vec<DiffRow>>())
            .saturating_add(4 * size_of::<usize>())
    }
}

struct Entry {
    source: Weak<Vec<DiffRow>>,
    projection: Arc<SplitProjection>,
    weight: usize,
}

pub(super) struct SplitCache {
    entries: Vec<Entry>,
    weight: usize,
}

impl SplitCache {
    pub(super) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            weight: 0,
        }
    }

    pub(super) fn get(&mut self, rows: &Arc<Vec<DiffRow>>) -> Arc<SplitProjection> {
        let mut live_weight = 0;
        self.entries.retain(|entry| {
            let live = entry.source.strong_count() != 0;
            if live {
                live_weight += entry.weight;
            }
            live
        });
        self.weight = live_weight;
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.source.as_ptr() == Arc::as_ptr(rows))
        {
            return Arc::clone(&entry.projection);
        }

        let projection = Arc::new(SplitProjection {
            rows: split_rows(rows),
            moved: moved_rows(rows),
        });
        let weight = projection.retained_bytes();
        if weight > MAX_DERIVED_BYTES {
            return projection;
        }
        while self.entries.len() >= MAX_ENTRIES || self.weight + weight > MAX_DERIVED_BYTES {
            self.weight -= self.entries.remove(0).weight;
        }
        self.entries.push(Entry {
            // A weak allocation identity cannot be recycled while retained.
            // Arc::make_mut detaches from weak observers before mutation, so
            // the next lookup also misses when a sole owner replaces its rows.
            source: Arc::downgrade(rows),
            projection: Arc::clone(&projection),
            weight,
        });
        self.weight += weight;
        projection
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::diff_view::{MainDiffSource, MainDiffView};
    use kagi_git::DiffLineKind;

    const FIRST: &str = "first_long_function_name_for_move_detection";
    const SECOND: &str = "second_long_function_name_for_move_detection";
    const CHANGED: &str = "a_different_function_name_for_move_detection";

    fn line(kind: DiffLineKind, content: &str) -> DiffRow {
        let sigil = match kind {
            DiffLineKind::Added => '+',
            DiffLineKind::Removed => '-',
            DiffLineKind::Context => ' ',
        };
        DiffRow::Line {
            kind,
            text: format!("{sigil}{content}").into(),
            old_lineno: None,
            new_lineno: None,
            highlights: Vec::new(),
        }
    }

    fn moved_diff() -> Arc<Vec<DiffRow>> {
        Arc::new(vec![
            line(DiffLineKind::Removed, FIRST),
            line(DiffLineKind::Removed, SECOND),
            line(DiffLineKind::Added, FIRST),
            line(DiffLineKind::Added, SECOND),
        ])
    }

    fn indices(projection: &SplitProjection) -> Vec<(Option<usize>, Option<usize>)> {
        projection
            .rows
            .iter()
            .map(|row| match row {
                SplitDiffRow::Full(index) => (Some(*index), Some(*index)),
                SplitDiffRow::Pair { left, right } => (*left, *right),
            })
            .collect()
    }

    #[test]
    fn same_title_and_size_diffs_keep_their_own_move_marks_on_repeated_retrieval() {
        let first = MainDiffView {
            title: "same-file.rs".into(),
            stats: "+2 −2".into(),
            rows: moved_diff(),
            source: MainDiffSource::Synthetic,
            images: None,
        };
        let mut second = MainDiffView {
            rows: moved_diff(),
            ..first.clone()
        };
        Arc::make_mut(&mut second.rows)[2] = line(DiffLineKind::Added, CHANGED);
        let mut cache = SplitCache::new();
        for _ in 0..2 {
            let first = cache.get(&first.rows);
            let second = cache.get(&second.rows);
            assert_eq!(first.moved, HashSet::from([0, 1, 2, 3]));
            assert_eq!(second.moved, HashSet::from([1, 3]));
            assert_eq!(
                indices(&first),
                vec![(Some(0), Some(2)), (Some(1), Some(3))]
            );
            assert_eq!(indices(&second), indices(&first));
        }
    }

    #[test]
    fn source_replacement_recomputes_pairs_and_moves_without_changing_old_projection() {
        let mut source = moved_diff();
        let mut cache = SplitCache::new();
        let original = cache.get(&source);
        source = Arc::new(vec![
            DiffRow::HunkHeader("@@".into()),
            line(DiffLineKind::Removed, FIRST),
            line(DiffLineKind::Added, CHANGED),
            DiffRow::Binary,
        ]);
        let replacement = cache.get(&source);
        assert!(replacement.moved.is_empty());
        assert_eq!(
            indices(&replacement),
            vec![(Some(0), Some(0)), (Some(1), Some(2)), (Some(3), Some(3))]
        );
        assert_eq!(original.moved, HashSet::from([0, 1, 2, 3]));
        assert_eq!(
            indices(&original),
            vec![(Some(0), Some(2)), (Some(1), Some(3))]
        );
    }

    #[test]
    fn make_mut_invalidates_weak_identity_with_or_without_another_source_owner() {
        for shared in [false, true] {
            let mut source = moved_diff();
            let other_owner = shared.then(|| Arc::clone(&source));
            let mut cache = SplitCache::new();
            let original = cache.get(&source);
            Arc::make_mut(&mut source)[2] = line(DiffLineKind::Added, CHANGED);
            let changed = cache.get(&source);
            assert_eq!(changed.moved, HashSet::from([1, 3]));
            assert_eq!(original.moved, HashSet::from([0, 1, 2, 3]));
            if let Some(other_owner) = other_owner {
                assert_eq!(cache.get(&other_owner).moved, HashSet::from([0, 1, 2, 3]));
            }
        }
    }

    #[test]
    fn entry_eviction_preserves_live_inputs_and_outstanding_projections() {
        let source = moved_diff();
        let mut cache = SplitCache::new();
        let original = cache.get(&source);
        let retained = Arc::downgrade(&original);
        let other_sources: Vec<_> = (0..MAX_ENTRIES).map(|_| moved_diff()).collect();
        for rows in &other_sources {
            cache.get(rows);
        }
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        assert_eq!(
            indices(&original),
            vec![(Some(0), Some(2)), (Some(1), Some(3))]
        );
        drop(original);
        assert!(
            retained.upgrade().is_none(),
            "oldest derivation must leave the cache"
        );
        let rebuilt = cache.get(&source);
        assert_eq!(
            indices(&rebuilt),
            vec![(Some(0), Some(2)), (Some(1), Some(3))]
        );
        assert_eq!(rebuilt.moved, HashSet::from([0, 1, 2, 3]));
    }

    #[test]
    fn weak_entries_release_source_data_and_prune_dead_derivations() {
        let source = moved_diff();
        let source_lifetime = Arc::downgrade(&source);
        let mut cache = SplitCache::new();
        let projection = cache.get(&source);
        let derived_lifetime = Arc::downgrade(&projection);
        drop(source);
        assert!(
            source_lifetime.upgrade().is_none(),
            "cache must not retain source rows"
        );
        assert_eq!(projection.moved, HashSet::from([0, 1, 2, 3]));
        drop(projection);
        let next = moved_diff();
        cache.get(&next);
        assert!(
            derived_lifetime.upgrade().is_none(),
            "dead inputs must be pruned on lookup"
        );
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn allocation_budget_evicts_before_entry_limit_and_counts_vector_capacity() {
        // Header rows avoid move-detection work. Half the byte budget in paired
        // storage plus allocation overhead means two entries cannot be retained.
        let size = MAX_DERIVED_BYTES / (2 * size_of::<SplitDiffRow>());
        let first = Arc::new(vec![DiffRow::Binary; size]);
        let second = Arc::new(vec![DiffRow::Binary; size]);
        let mut cache = SplitCache::new();
        let initial = cache.get(&first);
        let initial_lifetime = Arc::downgrade(&initial);
        assert_eq!(initial.rows.len(), size);
        drop(initial);
        let current = cache.get(&second);
        assert_eq!(current.rows.len(), size);
        assert!(
            initial_lifetime.upgrade().is_none(),
            "byte budget must evict before eight entries"
        );
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.weight <= MAX_DERIVED_BYTES);
    }

    #[test]
    fn move_set_capacity_is_charged_to_the_retention_budget() {
        let mut projection = SplitProjection {
            rows: Vec::new(),
            moved: HashSet::new(),
        };
        let empty_weight = projection.retained_bytes();
        projection.moved.reserve(1024);
        projection.moved.insert(7);
        let capacity = projection.moved.capacity();
        let allocated_weight = projection.retained_bytes();
        assert!(allocated_weight >= empty_weight + capacity * size_of::<usize>());
        projection.moved.clear();
        assert_eq!(
            projection.retained_bytes(),
            allocated_weight,
            "clearing marks keeps the table allocation"
        );
        projection.rows.reserve(1024);
        assert!(
            projection.retained_bytes()
                >= allocated_weight + projection.rows.capacity() * size_of::<SplitDiffRow>()
        );
    }

    #[test]
    fn oversized_projection_renders_without_retention_or_evicting_small_live_entries() {
        let size = MAX_DERIVED_BYTES / size_of::<SplitDiffRow>() + 1;
        let oversized = Arc::new(vec![DiffRow::Binary; size]);
        let small = moved_diff();
        let mut cache = SplitCache::new();
        let small_projection = cache.get(&small);
        let small_lifetime = Arc::downgrade(&small_projection);
        drop(small_projection);
        let projection = cache.get(&oversized);
        let lifetime = Arc::downgrade(&projection);
        assert_eq!(projection.rows.len(), size);
        assert!(
            matches!(projection.rows.last(), Some(SplitDiffRow::Full(index)) if *index == size - 1)
        );
        assert!(projection.moved.is_empty());
        assert!(small_lifetime.upgrade().is_some());
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.weight <= MAX_DERIVED_BYTES);
        drop(projection);
        assert!(
            lifetime.upgrade().is_none(),
            "oversized result must not be retained"
        );
        assert_eq!(cache.get(&oversized).rows.len(), size);
    }
}
