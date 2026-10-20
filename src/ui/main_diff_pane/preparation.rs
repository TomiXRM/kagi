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
            MainDiffRead::Wip { staged, refresh } => {
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
                let rename_from = match self {
                    MainDiffRead::Commit { rename_from, .. }
                    | MainDiffRead::Compare { rename_from, .. } => rename_from.as_deref(),
                    _ => None,
                };
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

/// Gather each side once while the selected read's Backend is open on the worker.
/// PDF classification precedes both text projection and ordinary image sniffing.
fn prepare_diff_media(
    backend: &kagi_git::Backend,
    file_diff: &kagi_git::FileDiff,
    source: &MainDiffSource,
    path: &std::path::Path,
    compare_head: Option<&kagi_git::CommitId>,
    rename_from: Option<&std::path::Path>,
) -> (Option<PreparedPdfPair>, Option<DiffImagePair>) {
    let (old_path, new_path) = side_paths(file_diff, path, rename_from);
    let at = |commit: &kagi_git::CommitId, path: Option<&std::path::Path>| {
        path.map(|path| {
            backend
                .blob_bytes_at(commit, path)
                .map_err(|e| e.to_string())
        })
        .unwrap_or(Ok(None))
    };
    let workdir_bytes = || -> Result<Option<Vec<u8>>, String> {
        let Some(path) = new_path else {
            return Ok(None);
        };
        let root = backend
            .workdir()
            .ok_or_else(|| "No working directory".to_string())?;
        std::fs::read(root.join(path))
            .map(Some)
            .map_err(|error| error.to_string())
    };
    let (old, new) = match source {
        MainDiffSource::Synthetic | MainDiffSource::Commit { commit: None, .. } => {
            return (None, None);
        }
        MainDiffSource::Commit {
            commit: Some(commit),
            ..
        } => {
            let old = if old_path.is_some() {
                backend
                    .first_parent(commit)
                    .map_err(|e| e.to_string())
                    .and_then(|parent| {
                        parent
                            .as_ref()
                            .map(|parent| at(parent, old_path))
                            .unwrap_or(Ok(None))
                    })
            } else {
                Ok(None)
            };
            (old, at(commit, new_path))
        }
        MainDiffSource::Compare { base, target, .. } => {
            let new = match target {
                CompareTarget::Head => compare_head
                    .map(|head| at(head, new_path))
                    .unwrap_or(Ok(None)),
                CompareTarget::WorkingTree => workdir_bytes(),
                CompareTarget::Commit(commit) => at(commit, new_path),
            };
            (at(base, old_path), new)
        }
        MainDiffSource::Unstaged { .. } => {
            let old = old_path
                .map(|path| {
                    backend
                        .blob_bytes_index(path)
                        .map_err(|e| e.to_string())
                        .and_then(|bytes| match bytes {
                            Some(_) => Ok(bytes),
                            None => backend.blob_bytes_head(path).map_err(|e| e.to_string()),
                        })
                })
                .unwrap_or(Ok(None));
            (old, workdir_bytes())
        }
        MainDiffSource::Staged { .. } => (
            old_path
                .map(|path| backend.blob_bytes_head(path).map_err(|e| e.to_string()))
                .unwrap_or(Ok(None)),
            new_path
                .map(|path| backend.blob_bytes_index(path).map_err(|e| e.to_string()))
                .unwrap_or(Ok(None)),
        ),
    };
    let signature = |bytes: &Result<Option<Vec<u8>>, String>| {
        bytes
            .as_ref()
            .ok()
            .and_then(|bytes| bytes.as_ref())
            .is_some_and(|bytes| bytes.starts_with(b"%PDF-"))
    };
    let pdf = is_pdf_path(path)
        || old_path.is_some_and(is_pdf_path)
        || new_path.is_some_and(is_pdf_path)
        || signature(&old)
        || signature(&new);
    if pdf {
        return (
            Some(PreparedPdfPair {
                old: PreparedPdfSide::from_read(old_path.is_some(), old),
                new: PreparedPdfSide::from_read(new_path.is_some(), new),
            }),
            None,
        );
    }
    let images = file_diff
        .is_binary
        .then(|| image_pair_from_bytes(old.ok().flatten(), new.ok().flatten()))
        .flatten();
    (None, images)
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
