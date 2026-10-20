//! Background-only text, encoded image and parsed PDF preparation for main diffs.

use super::{
    build_main_diff_view, CompareTarget, DiffImagePair, Landed, MainDiffRead, MainDiffSource,
    MainDiffView,
};
use crate::ui::diff_view::pdf::{is_pdf_path, PreparedPdfPair, PreparedPdfSide};

impl MainDiffRead {
    /// Open `repo` read-only and prepare the selected file's presentation,
    /// including document parsing, without doing I/O or projection on the UI thread.
    pub(super) fn run(&self, repo: &std::path::Path, path: &std::path::Path) -> Landed {
        let backend = match kagi_git::Backend::open(repo) {
            Ok(backend) => backend,
            Err(e) => return Landed::OpenFailed(e.to_string()),
        };
        // Resolve HEAD once so Compare's text and image sides use the same OID.
        let compare_head = matches!(
            self,
            Self::Compare {
                target: CompareTarget::Head,
                ..
            }
        )
        .then(|| backend.head_commit_id())
        .flatten();
        // Reload restores retained diffs before refreshing the panel's lists.
        // Recompute this side's rename identity here, never from the old pane
        // or a still-stale panel (which may already have closed).
        let refreshed_status = if matches!(self, Self::Wip { refresh: true, .. }) {
            match backend.working_tree_status() {
                Ok(status) => Some(status),
                Err(error) => return Landed::Failed(error.to_string()),
            }
        } else {
            None
        };
        let rename_from = match self {
            Self::Wip {
                staged,
                rename_from,
                ..
            } => match &refreshed_status {
                Some(status) => {
                    let files = if *staged {
                        &status.staged
                    } else {
                        &status.unstaged
                    };
                    files
                        .iter()
                        .find(|file| file.path == path)
                        .and_then(|file| match &file.change {
                            kagi_git::ChangeKind::Renamed { from } => Some(from.as_path()),
                            _ => None,
                        })
                }
                None => rename_from.as_deref(),
            },
            Self::Commit { rename_from, .. } | Self::Compare { rename_from, .. } => {
                rename_from.as_deref()
            }
        };
        let (result, source, file_index) = match self {
            MainDiffRead::Compare {
                base,
                target,
                file_index,
                ..
            } => {
                let result = match target {
                    CompareTarget::Head => match compare_head.as_ref() {
                        Some(head) => backend.compare_file_diff(base, head, path),
                        None => return Landed::Nothing,
                    },
                    CompareTarget::WorkingTree => {
                        backend.compare_commit_to_workdir_file_diff(base, path)
                    }
                    CompareTarget::Commit(id) => backend.compare_file_diff(base, id, path),
                };
                let source = MainDiffSource::Compare {
                    base: base.clone(),
                    target: target.clone(),
                    file_index: *file_index,
                };
                (result.map(std::sync::Arc::new), source, *file_index)
            }
            MainDiffRead::Wip {
                staged, refresh, ..
            } => {
                let result = if *staged {
                    backend.staged_file_diff(path)
                } else {
                    backend.unstaged_file_diff(path)
                };
                if *refresh && matches!(&result, Ok(fd) if fd.hunks.is_empty() && !fd.is_binary) {
                    return Landed::Nothing;
                }
                let path = path.to_path_buf();
                let source = if *staged {
                    MainDiffSource::Staged { path }
                } else {
                    MainDiffSource::Unstaged { path }
                };
                (result.map(std::sync::Arc::new), source, 0)
            }
            MainDiffRead::Commit {
                commit,
                row,
                file_index,
                ..
            } => {
                let source = MainDiffSource::Commit {
                    row_index: *row,
                    file_index: *file_index,
                    commit: Some(commit.clone()),
                };
                let result = match self {
                    MainDiffRead::Commit {
                        cached: Some(cached),
                        ..
                    } => Ok(cached.clone()),
                    _ => backend
                        .commit_file_diff(commit, path)
                        .map(std::sync::Arc::new),
                };
                (result, source, *file_index)
            }
        };
        match result {
            Ok(file_diff) => {
                let (pdfs, images) = prepare_diff_media(
                    &backend,
                    &file_diff,
                    &source,
                    path,
                    compare_head.as_ref(),
                    rename_from,
                );
                let view = if pdfs.is_some() {
                    // A PDF is a pair of documents, not text rows or hunk actions.
                    MainDiffView {
                        title: path.to_string_lossy().into_owned().into(),
                        stats: Default::default(),
                        rows: Default::default(),
                        hunk_approvals: Default::default(),
                        height_source: Default::default(),
                        source,
                        images: None,
                        lang: None,
                        highlighted: None,
                    }
                } else {
                    let mut view = build_main_diff_view(&file_diff, path, file_index, source);
                    view.images = images;
                    view
                };
                Landed::Show(Box::new((file_diff, view, pdfs)))
            }
            Err(e) => Landed::Failed(e.to_string()),
        }
    }
}

/// Classify by extension first, then bounded signatures. Ordinary text never
/// gets additional full old/new byte vectors just to decide its presentation.
fn prepare_diff_media(
    backend: &kagi_git::Backend,
    file_diff: &kagi_git::FileDiff,
    source: &MainDiffSource,
    path: &std::path::Path,
    compare_head: Option<&kagi_git::CommitId>,
    rename_from: Option<&std::path::Path>,
) -> (Option<PreparedPdfPair>, Option<DiffImagePair>) {
    use kagi_git::backend::file_reads::FileReadSource;
    const MAX_PDF_BYTES: usize = 10 * 1024 * 1024;

    if matches!(
        source,
        MainDiffSource::Synthetic | MainDiffSource::Commit { commit: None, .. }
    ) {
        return (None, None);
    }
    let (old_path, new_path) = side_paths(file_diff, path, rename_from);
    let pdf_extension =
        is_pdf_path(path) || old_path.is_some_and(is_pdf_path) || new_path.is_some_and(is_pdf_path);
    let parent = match source {
        MainDiffSource::Commit {
            commit: Some(commit),
            ..
        } if old_path.is_some() => backend
            .first_parent(commit)
            .map_err(|error| error.to_string()),
        _ => Ok(None),
    };
    let side = |source, path| match (source, path) {
        (Some(source), Some(path)) => MediaSide::File(source, path),
        _ => MediaSide::Absent,
    };
    let (old, new) = match source {
        MainDiffSource::Commit {
            commit: Some(commit),
            ..
        } => (
            match &parent {
                Ok(parent) => side(parent.as_ref().map(FileReadSource::Commit), old_path),
                Err(error) => MediaSide::Failed(error.clone()),
            },
            side(Some(FileReadSource::Commit(commit)), new_path),
        ),
        MainDiffSource::Compare { base, target, .. } => (
            side(Some(FileReadSource::Commit(base)), old_path),
            side(
                match target {
                    CompareTarget::Head => compare_head.map(FileReadSource::Commit),
                    CompareTarget::WorkingTree => Some(FileReadSource::Workdir),
                    CompareTarget::Commit(commit) => Some(FileReadSource::Commit(commit)),
                },
                new_path,
            ),
        ),
        MainDiffSource::Unstaged { .. } => (
            side(Some(FileReadSource::IndexOrHead), old_path),
            side(Some(FileReadSource::Workdir), new_path),
        ),
        MainDiffSource::Staged { .. } => (
            side(Some(FileReadSource::Head), old_path),
            side(Some(FileReadSource::Index), new_path),
        ),
        MainDiffSource::Synthetic | MainDiffSource::Commit { commit: None, .. } => unreachable!(),
    };
    let signature = |side: &MediaSide<'_>| {
        matches!(
            side.read(|source, path| backend.file_has_prefix(source, path, b"%PDF-")),
            Ok(Some(true))
        )
    };
    if pdf_extension || signature(&old) || signature(&new) {
        return (
            Some(PreparedPdfPair {
                old: PreparedPdfSide::from_read(
                    old_path.is_some(),
                    old.read(|source, path| backend.file_bytes(source, path, MAX_PDF_BYTES)),
                ),
                new: PreparedPdfSide::from_read(
                    new_path.is_some(),
                    new.read(|source, path| backend.file_bytes(source, path, MAX_PDF_BYTES)),
                ),
            }),
            None,
        );
    }
    if !file_diff.is_binary {
        return (None, None);
    }
    // Only the existing binary-image path needs unrestricted encoded bytes.
    let bytes = |side: &MediaSide<'_>| {
        side.read(|source, path| backend.file_bytes(source, path, usize::MAX))
            .ok()
            .flatten()
    };
    (None, image_pair_from_bytes(bytes(&old), bytes(&new)))
}

enum MediaSide<'a> {
    Absent,
    File(
        kagi_git::backend::file_reads::FileReadSource<'a>,
        &'a std::path::Path,
    ),
    Failed(String),
}

impl MediaSide<'_> {
    fn read<T>(
        &self,
        read: impl FnOnce(
            kagi_git::backend::file_reads::FileReadSource<'_>,
            &std::path::Path,
        ) -> Result<Option<T>, kagi_git::GitError>,
    ) -> Result<Option<T>, String> {
        match self {
            Self::Absent => Ok(None),
            Self::File(source, path) => read(*source, path).map_err(|error| error.to_string()),
            Self::Failed(error) => Err(error.clone()),
        }
    }
}

/// Honor the actual delta's distinct paths, never look up a renamed old blob
/// at the new path. Added/deleted deltas may still carry both libgit2 paths.
fn side_paths<'a>(
    file_diff: &'a kagi_git::FileDiff,
    requested: &'a std::path::Path,
    rename_from: Option<&'a std::path::Path>,
) -> (Option<&'a std::path::Path>, Option<&'a std::path::Path>) {
    use kagi_git::ChangeKind;
    let old = rename_from.or_else(|| match &file_diff.change {
        ChangeKind::Added => None,
        ChangeKind::Renamed { from } => Some(file_diff.old_path.as_deref().unwrap_or(from)),
        _ => Some(file_diff.old_path.as_deref().unwrap_or(requested)),
    });
    let new = match file_diff.change {
        ChangeKind::Deleted => None,
        _ => Some(file_diff.new_path.as_deref().unwrap_or(requested)),
    };
    (old, new)
}

fn image_pair_from_bytes(old: Option<Vec<u8>>, new: Option<Vec<u8>>) -> Option<DiffImagePair> {
    let pair = DiffImagePair {
        old_exists: old.is_some(),
        new_exists: new.is_some(),
        old: old.and_then(crate::ui::avatar_fetch::image_from_bytes),
        new: new.and_then(crate::ui::avatar_fetch::image_from_bytes),
    };
    (pair.old.is_some() || pair.new.is_some()).then_some(pair)
}

#[cfg(test)]
mod tests {
    #[test]
    fn partially_previewable_modified_image_keeps_both_versions() {
        let png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
        let bmp = b"BM unsupported preview encoding".to_vec();
        for (old, new) in [(bmp.clone(), png.clone()), (png.clone(), bmp.clone())] {
            let pair = super::image_pair_from_bytes(Some(old), Some(new)).unwrap();
            assert!(pair.old_exists && pair.new_exists);
            assert_ne!(pair.old.is_some(), pair.new.is_some());
        }
        let added = super::image_pair_from_bytes(None, Some(png.clone())).unwrap();
        assert!(!added.old_exists && added.new_exists);
        let deleted = super::image_pair_from_bytes(Some(png), None).unwrap();
        assert!(deleted.old_exists && !deleted.new_exists);
    }

    #[test]
    fn document_paths_preserve_absence_and_actual_rename_identity() {
        use kagi_git::{ChangeKind, FileDiff};
        use std::path::Path;
        let requested = Path::new("after.pdf");
        let mut diff = FileDiff {
            old_path: Some("before.pdf".into()),
            new_path: Some(requested.into()),
            change: ChangeKind::Renamed {
                from: "before.pdf".into(),
            },
            hunks: Vec::new(),
            is_binary: false,
        };
        assert_eq!(
            super::side_paths(&diff, requested, None),
            (Some(Path::new("before.pdf")), Some(requested))
        );
        diff.change = ChangeKind::Added;
        assert_eq!(
            super::side_paths(&diff, requested, None),
            (None, Some(requested))
        );
        // A path-filtered backend delta can be Added even when Inspector's
        // complete immutable changed-files read already established a rename.
        assert_eq!(
            super::side_paths(&diff, requested, Some(Path::new("before.pdf"))),
            (Some(Path::new("before.pdf")), Some(requested))
        );
        diff.change = ChangeKind::Deleted;
        assert_eq!(
            super::side_paths(&diff, requested, None),
            (Some(Path::new("before.pdf")), None)
        );
    }

    #[test]
    fn document_errors_do_not_become_absent_versions() {
        use crate::ui::diff_view::pdf::PreparedPdfSide;
        assert!(matches!(
            PreparedPdfSide::from_read(false, Ok(None)),
            PreparedPdfSide::Absent
        ));
        assert!(matches!(
            PreparedPdfSide::from_read(true, Ok(None)),
            PreparedPdfSide::Failed(_)
        ));
        assert!(matches!(
            PreparedPdfSide::from_read(true, Err("missing object".into())),
            PreparedPdfSide::Failed(_)
        ));
        assert!(matches!(
            PreparedPdfSide::from_read(true, Ok(Some(b"malformed".to_vec()))),
            PreparedPdfSide::Source(_)
        ));
    }
}
