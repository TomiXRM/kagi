//! Issue #512: the Commit Inspector's input-derived display model.
//!
//! `inspector::render_inspector` only reads this and builds elements from it;
//! it never truncates, classifies, tree-builds, tallies or converts the
//! message itself. The model is plain data — GPUI elements, which carry
//! `Context<KagiApp>` listeners, are still built every frame.
//!
//! Two slots, each rebuilt only when its key changes:
//!
//! | slot    | key                                                        | changes on                                              |
//! |---------|------------------------------------------------------------|---------------------------------------------------------|
//! | files   | [`FilesSource`]: selected row + [`DiffCaches::revision`], or the Compare entity + its revision | selection, a changed-files load landing, any cache clear (reload, row renumber, tab activation), a new compare |
//! | message | the commit's full SHA (a commit's message is immutable)    | selection                                               |
//!
//! Display options never invalidate: Path⇄Tree, the "Generated (N)"
//! disclosure and the active-file highlight only choose which derived rows
//! render, so the tree rows and the flat rows are both derived once per key.
//! Localized text is not derived either (the language can change without any
//! input changing); the model holds the numbers and render formats them.
//!
//! [`DiffCaches::revision`]: super::diff_cache::DiffCaches::revision

use gpui::SharedString;
use kagi_git::{find_stat, ChangeKind, FileDiffStat, FileStatus};
use kagi_ui_core::file_tree::{self, TreeRow};

use super::tab_view::TabUiState;

/// Changed files shown per commit (T018); the rest collapse into "… and N more".
pub(super) const MAX_FILES: usize = 100;

/// What the files slot was derived from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FilesSource {
    /// The selected commit's row in the session's `DiffCaches`.
    Commit { row: usize, revision: u64 },
    /// The session's `ComparePane` entity at one of its views.
    Compare { pane: gpui::EntityId, revision: u64 },
}

/// One shown changed file. Its position in [`InspectorFiles::files`] is the
/// `file_index` every click / context-menu / copy handler resolves against the
/// source list, so it must stay the index in that list.
pub(super) struct InspectorFile {
    /// Repository-relative path (the flat-row label).
    pub path: SharedString,
    pub change: ChangeKind,
    /// Folded under "Generated (N)" (issue #348).
    pub generated: bool,
    /// W16-DIFFSTAT: additions / deletions, when the diffstat has the path.
    pub stat: Option<FileDiffStat>,
}

/// ChangeKind tally over the whole changed-file list (not just the shown part).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ChangeCounts {
    pub modified: usize,
    pub added: usize,
    pub deleted: usize,
    pub renamed: usize,
    pub typechange: usize,
}

pub(super) struct InspectorFiles {
    /// The first [`MAX_FILES`] files, in source order.
    pub files: Vec<InspectorFile>,
    /// Files past [`MAX_FILES`]; `0` when nothing is cut.
    pub hidden: usize,
    /// Tree rows of the shown files that are not generated.
    pub tree: Vec<TreeRow>,
    pub generated_count: usize,
    pub counts: ChangeCounts,
}

impl InspectorFiles {
    /// Derive the shown list from one changed-file list and its optional
    /// per-file diffstat and generated flags. Generated flags that are missing
    /// or shorter than the shown list fold nothing (compare mode / not yet
    /// classified).
    pub(super) fn derive(
        files: &[FileStatus],
        stats: Option<&[FileDiffStat]>,
        generated: Option<&[bool]>,
    ) -> Self {
        let shown = &files[..files.len().min(MAX_FILES)];
        let generated = generated.filter(|g| g.len() >= shown.len());
        let is_generated = |i: usize| generated.is_some_and(|g| g[i]);
        let rows: Vec<InspectorFile> = shown
            .iter()
            .enumerate()
            .map(|(i, fs)| InspectorFile {
                path: SharedString::from(fs.path.to_string_lossy().into_owned()),
                change: fs.change.clone(),
                generated: is_generated(i),
                stat: stats.and_then(|s| find_stat(s, &fs.path)).cloned(),
            })
            .collect();
        let tree = file_tree::retain_files(file_tree::build_file_tree(shown), |i| !is_generated(i));
        let mut counts = ChangeCounts::default();
        for fs in files {
            match fs.change {
                ChangeKind::Modified => counts.modified += 1,
                ChangeKind::Added => counts.added += 1,
                ChangeKind::Deleted => counts.deleted += 1,
                ChangeKind::Renamed { .. } => counts.renamed += 1,
                ChangeKind::TypeChange => counts.typechange += 1,
            }
        }
        #[cfg(any(test, feature = "gui-e2e"))]
        DERIVATIONS.with(|d| d.set((d.get().0 + 1, d.get().1)));
        Self {
            generated_count: rows.iter().filter(|f| f.generated).count(),
            files: rows,
            hidden: files.len().saturating_sub(MAX_FILES),
            tree,
            counts,
        }
    }
}

/// The session's derived Inspector model (lives in `TabUiState`, so it is
/// owned and dropped with the tab like the caches it is derived from).
#[derive(Default)]
pub(super) struct InspectorModel {
    /// `None` inside = the source list is unavailable ("(diff unavailable)").
    files: Option<(FilesSource, Option<InspectorFiles>)>,
    /// `(full SHA, escaped HTML of the reflowed message)`.
    message: Option<(SharedString, SharedString)>,
}

impl InspectorModel {
    /// Re-derive the files slot iff it was built for another `source`.
    pub(super) fn sync_files(
        &mut self,
        source: FilesSource,
        derive: impl FnOnce() -> Option<InspectorFiles>,
    ) {
        if self.files.as_ref().is_some_and(|(key, _)| *key == source) {
            return;
        }
        self.files = Some((source, derive()));
    }

    /// Re-derive the message HTML iff it was built for another commit.
    pub(super) fn sync_message(&mut self, sha: &SharedString, message: &str) {
        if self.message.as_ref().is_some_and(|(key, _)| key == sha) {
            return;
        }
        // Hard-wrapped bodies (git's 72-col convention) are reflowed first —
        // otherwise the soft wrap stacks on the hard breaks and orphan
        // fragments flap in and out while resizing. HTML, not Markdown: the
        // selectable `TextView` parses only those two, and Markdown would
        // misread the message (`* fix` as a bullet, `#123` as a heading).
        let html =
            kagi_domain::message::message_to_html(&kagi_domain::message::reflow_message(message));
        #[cfg(any(test, feature = "gui-e2e"))]
        DERIVATIONS.with(|d| d.set((d.get().0, d.get().1 + 1)));
        self.message = Some((sha.clone(), SharedString::from(html)));
    }

    /// The synced file list; `None` = unavailable.
    pub(super) fn files(&self) -> Option<&InspectorFiles> {
        self.files.as_ref().and_then(|(_, files)| files.as_ref())
    }

    pub(super) fn message_html(&self) -> SharedString {
        self.message
            .as_ref()
            .map(|(_, html)| html.clone())
            .unwrap_or_default()
    }
}

impl TabUiState {
    /// Sync the Inspector files slot to the selected commit's cached row.
    pub(super) fn sync_inspector_commit_files(&mut self, row: usize) {
        let caches = &self.diff_caches;
        let source = FilesSource::Commit {
            row,
            revision: caches.revision(),
        };
        self.inspector_model.sync_files(source, || {
            let files = caches.changed_files().get(&row)?.as_deref()?;
            Some(InspectorFiles::derive(
                files,
                caches.diffstat().get(&row).map(Vec::as_slice),
                caches.generated().get(&row).map(Vec::as_slice),
            ))
        });
    }
}

#[cfg(any(test, feature = "gui-e2e"))]
thread_local! {
    /// `(file-list derivations, message HTML conversions)` on this thread.
    static DERIVATIONS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// `(file-list derivations, message HTML conversions)` so far on this thread.
#[cfg(any(test, feature = "gui-e2e"))]
pub(super) fn derivations() -> (usize, usize) {
    DERIVATIONS.with(|d| d.get())
}

#[cfg(feature = "gui-e2e")]
impl super::KagiApp {
    /// The temporary derivation counter, for the `inspector_derived` scenario.
    pub fn inspector_derivations_for_e2e() -> (usize, usize) {
        derivations()
    }
}

#[cfg(test)]
#[path = "inspector_model_tests.rs"]
mod tests;
