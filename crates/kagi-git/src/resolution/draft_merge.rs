//! Merge only this owner's edits; other processes' acknowledged decisions survive.
use super::*;
use drafts::{compatible, has_draft_state};

pub(super) struct MergedBody {
    state: Option<hunk_state::HunkState>,
    result: Option<Vec<ResolvedLine>>,
}

pub(super) struct MergedFiles<'a> {
    disk: BTreeMap<&'a Path, &'a FileResolution>,
    bodies: BTreeMap<&'a Path, MergedBody>,
}

impl<'a> MergedFiles<'a> {
    pub fn record(&'a self, path: &'a Path, current: &'a FileResolution) -> FileRecord<'a> {
        let mut record = file_record(path, self.disk.get(path).copied().unwrap_or(current));
        if let Some(body) = self.bodies.get(path) {
            record.result = &body.result;
            record.hunk_state = &body.state;
        }
        record
    }

    /// Deduplicate against exactly the body `record` will persist, not the
    /// editor's pre-merge body (which may itself now survive only in recovery).
    pub fn matches_record(
        &self,
        path: &Path,
        current: &FileResolution,
        saved: &FileResolution,
    ) -> bool {
        let selected = self.disk.get(path).copied().unwrap_or(current);
        if let Some(body) = self.bodies.get(path) {
            selected.recovery_only == saved.recovery_only
                && drafts::same_draft_metadata(selected, saved)
                && body.result == saved.result
                && body.state == saved.hunk_state
        } else {
            drafts::same_draft(selected, saved)
        }
    }

    pub fn supersedes(
        &self,
        path: &Path,
        current: &FileResolution,
        disk: &FileResolution,
        snapshot: &draft_storage::Snapshot,
    ) -> bool {
        if self.disk.contains_key(path) || self.bodies.contains_key(path) {
            return true;
        }
        let same_owner = current.live_editing_origin
            && current.editing_origin.is_some()
            && current.editing_origin == disk.editing_origin;
        has_draft_state(current)
            && (same_owner || compatible(current, disk))
            && snapshot
                .files
                .get(path)
                .is_some_and(|base| base.hash == draft_storage::file_hash(path, disk))
    }
}

impl ResolutionBuffer {
    pub(super) fn merge_draft_files<'a>(
        &'a self,
        disk: Option<&'a Self>,
        snapshot: &draft_storage::Snapshot,
        changed: bool,
    ) -> MergedFiles<'a> {
        let mut merged = MergedFiles {
            disk: BTreeMap::new(),
            bodies: BTreeMap::new(),
        };
        let Some(disk) = disk else {
            return merged;
        };
        for (path, current) in &self.files {
            let Some(saved) = disk
                .files
                .get(path)
                .filter(|saved| compatible(current, saved) && has_draft_state(saved))
            else {
                continue;
            };
            // A preservation clone clears PendingEdit without committing its
            // body. Only an unchanged loaded baseline may defer to disk.
            if !current.pending_edit.is_pending()
                && snapshot
                    .files
                    .get(path)
                    .is_some_and(|base| base.hash == draft_storage::file_hash(path, current))
            {
                merged.disk.insert(path.as_path(), saved);
                continue;
            }
            // A prior save may have merged disk-only decisions without changing
            // this immutable live buffer. Even an unchanged document hash then
            // needs a re-merge against this owner's (unmerged) file baseline.
            if !changed
                && snapshot
                    .files
                    .get(path)
                    .is_some_and(|base| base.hash == draft_storage::file_hash(path, saved))
            {
                continue;
            }
            let Some(body) = self.merge_hunk_body(
                path,
                current,
                saved,
                snapshot
                    .files
                    .get(path)
                    .and_then(|base| base.state.as_ref()),
            ) else {
                continue;
            };
            merged.bodies.insert(path.as_path(), body);
        }
        merged
    }

    fn merge_hunk_body(
        &self,
        path: &Path,
        current: &FileResolution,
        disk: &FileResolution,
        base: Option<&hunk_state::HunkState>,
    ) -> Option<MergedBody> {
        let ours = current.hunk_state.as_ref()?;
        let theirs = disk.hunk_state.as_ref()?;
        let model = self.hunks.get(path)?;
        // A manual Result is not a set of independent hunk decisions. Keep both
        // complete bodies in that case, never reconstruct over manual input.
        if current.result.as_ref()? != &model.assemble() {
            return None;
        }
        let mut combined = model.clone();
        if !theirs.apply(&mut combined) || disk.result.as_ref()? != &combined.assemble() {
            return None;
        }
        let state = ours.merge(theirs, base)?;
        if !state.apply(&mut combined) {
            return None;
        }
        Some(MergedBody {
            result: Some(combined.assemble()),
            state: Some(state),
        })
    }
}
