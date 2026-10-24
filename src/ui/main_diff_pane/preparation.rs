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
    let prepared = prepare_side_pair(
        backend,
        [old, new],
        PreparationPolicy::Diff {
            pdf_extension,
            binary: file_diff.is_binary,
            expected: [old_path.is_some(), new_path.is_some()],
        },
    );
    (prepared.pdfs, prepared.images)
}

pub(crate) enum PreparationPolicy {
    Diff {
        pdf_extension: bool,
        binary: bool,
        expected: [bool; 2],
    },
    Conflict(kagi_git::ConflictKind),
}

#[derive(Default)]
pub(crate) struct PreparedSidePair {
    pub pdfs: Option<PreparedPdfPair>,
    pub images: Option<DiffImagePair>,
    pub raw: Option<[crate::ui::conflict_binary_view::SideData; 2]>,
}

/// The only side-pair byte/media preparation boundary. Both callers invoke it
/// in a background task; descriptors are frozen before leaving their owner.
pub(crate) fn prepare_side_pair(
    backend: &kagi_git::Backend,
    [old, new]: [MediaSide<'_>; 2],
    policy: PreparationPolicy,
) -> PreparedSidePair {
    const MAX_PDF_BYTES: usize = 10 * 1024 * 1024;
    let PreparationPolicy::Diff {
        pdf_extension,
        binary,
        expected,
    } = policy
    else {
        let PreparationPolicy::Conflict(kind) = policy else {
            unreachable!()
        };
        return PreparedSidePair {
            raw: Some([
                prepare_raw_side(backend, old, kind),
                prepare_raw_side(backend, new, kind),
            ]),
            ..Default::default()
        };
    };
    let signature = |side: &MediaSide<'_>| {
        matches!(
            side.read(|source, path| backend.file_has_prefix(source, path, b"%PDF-")),
            Ok(Some(true))
        )
    };
    if pdf_extension || signature(&old) || signature(&new) {
        return PreparedSidePair {
            pdfs: Some(PreparedPdfPair {
                old: PreparedPdfSide::from_read(
                    expected[0],
                    old.read(|source, path| backend.file_bytes(source, path, MAX_PDF_BYTES)),
                ),
                new: PreparedPdfSide::from_read(
                    expected[1],
                    new.read(|source, path| backend.file_bytes(source, path, MAX_PDF_BYTES)),
                ),
            }),
            ..Default::default()
        };
    }
    if !binary {
        return PreparedSidePair::default();
    }
    // Only the existing binary-image path needs unrestricted encoded bytes.
    let bytes = |side: &MediaSide<'_>| {
        side.read(|source, path| backend.file_bytes(source, path, usize::MAX))
            .ok()
            .flatten()
    };
    PreparedSidePair {
        images: image_pair_from_bytes(bytes(&old), bytes(&new)),
        ..Default::default()
    }
}

pub(crate) enum MediaSide<'a> {
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

fn prepare_raw_side(
    backend: &kagi_git::Backend,
    side: MediaSide<'_>,
    kind: kagi_git::ConflictKind,
) -> crate::ui::conflict_binary_view::SideData {
    use crate::ui::avatar_fetch::{classify_image, ImageClassification};
    use crate::ui::conflict_binary_view::{viewer_mode, BinaryViewMode, SideData, IMAGE_SIZE_CAP};
    use kagi_git::backend::file_reads::FileReadSource;
    let MediaSide::File(source @ FileReadSource::ConflictStage { oid, .. }, path) = side else {
        return SideData::absent();
    };
    let mut data = SideData {
        present: true,
        oid_short: oid.chars().take(7).collect(),
        size: None,
        mode: viewer_mode(kind, false, None),
        image: None,
        symlink_target: None,
        error: None,
    };
    match backend.file_size(source, path) {
        Ok(size) => data.size = size,
        Err(error) => {
            data.error = Some(error.to_string());
            return data;
        }
    }
    if kind == kagi_git::ConflictKind::Submodule {
        return data;
    }
    if kind != kagi_git::ConflictKind::Symlink && data.size.is_none_or(|size| size > IMAGE_SIZE_CAP)
    {
        return data;
    }
    match backend.file_bytes(source, path, IMAGE_SIZE_CAP as usize) {
        Ok(Some(bytes)) if kind == kagi_git::ConflictKind::Symlink => {
            data.symlink_target = Some(String::from_utf8_lossy(&bytes).into_owned());
        }
        Ok(Some(bytes)) => {
            if let Some(fmt) = classify_image(path, &bytes, ImageClassification::ExtensionThenMagic)
            {
                klog!(
                    "conflict-view: image decoded oid={} fmt={:?} src={}bytes",
                    data.oid_short,
                    fmt,
                    bytes.len()
                );
                data.image = Some(std::sync::Arc::new(gpui::Image::from_bytes(fmt, bytes)));
                data.mode = BinaryViewMode::Image;
            }
        }
        Ok(None) => {}
        Err(error) => data.error = Some(error.to_string()),
    }
    data
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
    fn shared_preparation_preserves_diff_media_snapshot() {
        use super::*;
        use kagi_git::backend::file_reads::FileReadSource;
        use std::path::Path;
        let temp = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(temp.path())
            .status()
            .unwrap()
            .success());
        let backend = kagi_git::Backend::open(temp.path()).unwrap();
        let path = Path::new("image.png");
        let png = b"\x89PNG\r\n\x1a\n";
        std::fs::write(temp.path().join(path), png).unwrap();
        let prepared = prepare_side_pair(
            &backend,
            [
                MediaSide::Absent,
                MediaSide::File(FileReadSource::Workdir, path),
            ],
            PreparationPolicy::Diff {
                pdf_extension: false,
                binary: true,
                expected: [false, true],
            },
        );
        let images = prepared.images.unwrap();
        // Frozen pre-refactor MainDiffView.images snapshot: added PNG, one image.
        assert_eq!(
            (
                images.old_exists,
                images.new_exists,
                images.old.is_some(),
                images.new.is_some()
            ),
            (false, true, false, true)
        );
        assert!(prepared.pdfs.is_none());
        std::fs::write(
            temp.path().join(path),
            b"BM legacy diff does not recognize BMP",
        )
        .unwrap();
        let prepared = prepare_side_pair(
            &backend,
            [
                MediaSide::Absent,
                MediaSide::File(FileReadSource::Workdir, path),
            ],
            PreparationPolicy::Diff {
                pdf_extension: false,
                binary: true,
                expected: [false, true],
            },
        );
        assert!(prepared.images.is_none() && prepared.pdfs.is_none());
        std::fs::write(temp.path().join(path), b"%PDF-invalid document").unwrap();
        let prepared = prepare_side_pair(
            &backend,
            [
                MediaSide::Absent,
                MediaSide::File(FileReadSource::Workdir, path),
            ],
            PreparationPolicy::Diff {
                pdf_extension: false,
                binary: true,
                expected: [false, true],
            },
        );
        let pdfs = prepared.pdfs.unwrap();
        // Signature still wins over image extension; parse failure stays a PDF source.
        assert!(matches!(pdfs.old, PreparedPdfSide::Absent));
        assert!(matches!(pdfs.new, PreparedPdfSide::Source(_)));
        assert!(prepared.images.is_none());
        let prepared = prepare_side_pair(
            &backend,
            [MediaSide::Absent, MediaSide::Failed("read failed".into())],
            PreparationPolicy::Diff {
                pdf_extension: true,
                binary: true,
                expected: [false, true],
            },
        );
        let pdfs = prepared.pdfs.unwrap();
        assert!(matches!(pdfs.old, PreparedPdfSide::Absent));
        assert!(matches!(&pdfs.new, PreparedPdfSide::Failed(error) if error == "read failed"));
    }

    #[test]
    fn conflict_preparation_preserves_absence_and_read_failure() {
        use super::*;
        use kagi_git::backend::file_reads::FileReadSource;
        let temp = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(temp.path())
            .status()
            .unwrap()
            .success());
        let backend = kagi_git::Backend::open(temp.path()).unwrap();
        let prepared = prepare_side_pair(
            &backend,
            [
                MediaSide::Absent,
                MediaSide::File(
                    FileReadSource::ConflictStage {
                        oid: "invalid",
                        mode: 0o100644,
                    },
                    std::path::Path::new("image.png"),
                ),
            ],
            PreparationPolicy::Conflict(kagi_git::ConflictKind::Binary),
        );
        let [absent, failed] = prepared.raw.unwrap();
        assert!(!absent.present && absent.error.is_none());
        assert!(failed.present && failed.error.is_some());
    }

    #[test]
    fn image_classification_policies_preserve_both_existing_surfaces() {
        use crate::ui::avatar_fetch::{classify_image, ImageClassification};
        use gpui::ImageFormat;
        use std::path::Path;
        for extension in ["png", "PNG", "jpg", "jpeg", "gif", "webp", "bmp"] {
            let name = format!("image.{extension}");
            assert!(classify_image(
                Path::new(&name),
                &[],
                ImageClassification::ExtensionThenMagic
            )
            .is_some());
            assert!(
                classify_image(Path::new(&name), &[], ImageClassification::MagicOnly).is_none()
            );
        }
        let png = b"\x89PNG\r\n\x1a\n";
        assert_eq!(
            classify_image(
                Path::new("image.jpg"),
                png,
                ImageClassification::ExtensionThenMagic
            ),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            classify_image(Path::new("image.jpg"), png, ImageClassification::MagicOnly),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            classify_image(
                Path::new("image"),
                b"BM",
                ImageClassification::ExtensionThenMagic
            ),
            Some(ImageFormat::Bmp)
        );
        assert!(
            classify_image(Path::new("image"), b"BM", ImageClassification::MagicOnly).is_none()
        );
    }
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
