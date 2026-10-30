//! Background-only text, encoded image and parsed PDF preparation for main diffs.

use super::{
    build_main_diff_view, CompareTarget, DiffImagePair, Landed, MainDiffRead, MainDiffSource,
    MainDiffView,
};
use crate::ui::diff_view::pdf::is_pdf_path;
#[cfg(test)]
use crate::ui::diff_view::pdf::PreparedPdfSide;
use crate::ui::diff_view::{SideContent, SideFailure, SideHeader, SideMetadata, SidePair};
use crate::ui::i18n::Msg;

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
                // The backend classifies both sides before generating hunks.
                // LFS pointers are binary presentation, never media sources.
                let pair = if file_diff.is_lfs {
                    None
                } else {
                    prepare_diff_media(
                        &backend,
                        &file_diff,
                        &source,
                        path,
                        compare_head.as_ref(),
                        rename_from,
                    )
                };
                let pdfs = pair.as_ref().and_then(SidePair::pdf_sources);
                let images = pair.as_ref().and_then(image_pair);
                let view = if pair
                    .as_ref()
                    .is_some_and(|pair| pair.headers[0].id == "main-diff-pdf-old")
                {
                    // A PDF is a pair of documents, not text rows or hunk actions.
                    MainDiffView {
                        title: path.to_string_lossy().into_owned().into(),
                        stats: Default::default(),
                        rows: Default::default(),
                        hunk_approvals: Default::default(),
                        height_source: Default::default(),
                        source,
                        images,
                        side_pair: pair,
                        lang: None,
                        highlighted: None,
                    }
                } else {
                    let mut view = build_main_diff_view(&file_diff, path, file_index, source);
                    view.images = images;
                    view.side_pair = pair;
                    view
                };
                Landed::Show(Box::new((file_diff, view, pdfs)))
            }
            Err(e) => Landed::Failed(e.to_string()),
        }
    }
}

/// Source-faithful adapters feed the same independent classification as conflict.
fn prepare_diff_media(
    backend: &kagi_git::Backend,
    file_diff: &kagi_git::FileDiff,
    source: &MainDiffSource,
    path: &std::path::Path,
    compare_head: Option<&kagi_git::CommitId>,
    rename_from: Option<&std::path::Path>,
) -> Option<SidePair> {
    use kagi_git::backend::file_reads::FileReadSource;

    if matches!(
        source,
        MainDiffSource::Synthetic | MainDiffSource::Commit { commit: None, .. }
    ) {
        return None;
    }
    let (old_path, new_path) = side_paths(file_diff, path, rename_from);
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
    // Keep ordinary text on its existing row/projection path without full copies.
    let media = |side: &MediaSide<'_>| match side {
        MediaSide::File(source, path) => {
            is_pdf_path(path)
                || matches!(
                    backend.file_identity(*source, path),
                    Ok(Some((_, 0o120000 | 0o160000)))
                )
                || matches!(probe_media(backend, *source, path), Ok(Some(_)))
        }
        _ => false,
    };
    if !file_diff.is_binary && !media(&old) && !media(&new) {
        return None;
    }
    let [left, right] = prepare_content_pair(backend, [old, new]);
    let pdf = matches!(left, SideContent::Pdf { .. })
        || matches!(right, SideContent::Pdf { .. })
        || old_path.is_some_and(is_pdf_path)
        || new_path.is_some_and(is_pdf_path);
    Some(SidePair {
        left,
        right,
        headers: [
            SideHeader {
                label: (if pdf {
                    Msg::DiffPdfPrevious
                } else {
                    Msg::DiffImageBefore
                })
                .t()
                .into(),
                accent: None,
                id: if pdf {
                    "main-diff-pdf-old"
                } else {
                    "main-diff-image-old"
                },
                absent: Msg::DiffPdfNoPrevious,
            },
            SideHeader {
                label: (if pdf {
                    Msg::DiffPdfCurrent
                } else {
                    Msg::DiffImageAfter
                })
                .t()
                .into(),
                accent: None,
                id: if pdf {
                    "main-diff-pdf-new"
                } else {
                    "main-diff-image-new"
                },
                absent: Msg::DiffPdfNoCurrent,
            },
        ],
    })
}

pub(crate) const IMAGE_SIZE_CAP: usize = 8 * 1024 * 1024;
pub(crate) const PDF_SIZE_CAP: usize = 10 * 1024 * 1024;

/// Called only by host preparation workers. Each side has its own mode and budget.
pub(crate) fn prepare_content_pair(
    backend: &kagi_git::Backend,
    sides: [MediaSide<'_>; 2],
) -> [SideContent; 2] {
    sides.map(|side| prepare_content(backend, side))
}

pub(crate) enum MediaSide<'a> {
    Absent,
    File(
        kagi_git::backend::file_reads::FileReadSource<'a>,
        &'a std::path::Path,
    ),
    Failed(String),
}

fn prepare_content(backend: &kagi_git::Backend, side: MediaSide<'_>) -> SideContent {
    use kagi_git::backend::file_reads::FileReadSource;
    let mut meta = SideMetadata::default();
    let (source, path) = match side {
        MediaSide::Absent => return SideContent::Absent,
        MediaSide::Failed(error) => {
            meta.error = Some(SideFailure::Read(error.into()));
            return SideContent::Binary(meta);
        }
        MediaSide::File(source, path) => (source, path),
    };
    let conflict = matches!(source, FileReadSource::ConflictStage { .. });
    let (oid, mode) = match backend.file_identity(source, path) {
        Ok(Some(identity)) => identity,
        Ok(None) => return SideContent::failed("Missing source"),
        Err(error) => return SideContent::failed(error.to_string()),
    };
    let oid = oid.map(gpui::SharedString::from);
    meta.oid = oid.clone();
    if mode == 0o160000 {
        return SideContent::Submodule(meta);
    }
    let symlink = mode == 0o120000;
    // Freeze index/HEAD reads to the identity just classified.
    let source = match (source, oid.as_deref()) {
        (FileReadSource::Workdir, _) => FileReadSource::Workdir,
        (_, Some(oid)) => FileReadSource::ConflictStage { oid, mode },
        _ => source,
    };
    let read = (|| {
        meta.size = backend
            .file_size(source, path)
            .map_err(|e| SideFailure::Read(e.to_string().into()))?;
        if meta.size.is_none() {
            return Err(SideFailure::Read("Missing source".into()));
        }
        // Even a prefix read inflates an entire ODB blob in libgit2. Reject
        // anything beyond the largest format budget using its header first.
        if meta.size.is_some_and(|size| size > PDF_SIZE_CAP as u64) {
            return Err(SideFailure::TooLarge);
        }
        let kind = if symlink {
            None
        } else {
            probe_media(backend, source, path)
                .map_err(|e| SideFailure::Read(e.to_string().into()))?
        };
        let pdf = !symlink
            && (matches!(kind, Some(kagi_domain::diff::MediaKind::Pdf))
                || (kind.is_none() && is_pdf_path(path)));
        let cap = if pdf { PDF_SIZE_CAP } else { IMAGE_SIZE_CAP };
        if meta.size.is_some_and(|size| size > cap as u64) {
            return Err(SideFailure::TooLarge);
        }
        let bytes = backend
            .file_bytes(source, path, cap)
            .map_err(|e| SideFailure::Read(e.to_string().into()))?
            .ok_or_else(|| SideFailure::Read("Missing source".into()))?;
        Ok((bytes, pdf))
    })();
    let (bytes, pdf) = match read {
        Ok(value) => value,
        Err(error) => {
            meta.error = Some(error);
            return if symlink {
                SideContent::Symlink { target: None, meta }
            } else {
                SideContent::Binary(meta)
            };
        }
    };
    if symlink {
        return SideContent::Symlink {
            target: Some(String::from_utf8_lossy(&bytes).into_owned().into()),
            meta,
        };
    }
    if pdf {
        return SideContent::Pdf {
            source: kagi_ui_editor::PdfSource::from_bytes(bytes),
            meta,
        };
    }
    content_from_bytes(path, bytes, meta, conflict)
}

fn probe_media(
    backend: &kagi_git::Backend,
    source: kagi_git::backend::file_reads::FileReadSource<'_>,
    path: &std::path::Path,
) -> Result<Option<kagi_domain::diff::MediaKind>, kagi_git::GitError> {
    let mut prefix = [0; 12];
    Ok(backend
        .file_prefix(source, path, &mut prefix)?
        .and_then(|count| kagi_domain::diff::media_kind(&prefix[..count])))
}

fn content_from_bytes(
    path: &std::path::Path,
    bytes: Vec<u8>,
    mut meta: SideMetadata,
    conflict: bool,
) -> SideContent {
    use crate::ui::avatar_fetch::{classify_image, ImageClassification};
    if let Some(fmt) = classify_image(path, &bytes, ImageClassification::FilePreview) {
        let decoded = decode_image(&bytes);
        let (decoded, dimensions) = match decoded {
            Ok((image, dimensions)) => (Ok(image), Some(dimensions)),
            Err(error) => {
                meta.error = Some(SideFailure::Decode);
                (Err(error), None)
            }
        };
        if conflict {
            let oid_short: String = meta.oid.as_deref().unwrap_or("").chars().take(7).collect();
            klog!(
                "conflict-view: image decoded oid={} fmt={:?} src={}bytes",
                oid_short,
                fmt,
                bytes.len()
            );
        }
        return SideContent::Image {
            encoded: std::sync::Arc::new(gpui::Image::from_bytes(fmt, bytes)),
            decoded,
            dimensions,
            meta,
        };
    }
    // This text is rendered by the same selectable TextView as oplog details.
    if bytes.len() <= crate::ui::oplog_render::SELECTABLE_DETAIL_MAX && !bytes.contains(&0) {
        if let Ok(text) = String::from_utf8(bytes) {
            return SideContent::Text {
                text: text.into(),
                meta,
            };
        }
    }
    SideContent::Binary(meta)
}

fn decode_image(
    bytes: &[u8],
) -> Result<(std::sync::Arc<gpui::RenderImage>, (u32, u32)), gpui::SharedString> {
    use image::AnimationDecoder as _;
    let frames = (|| -> Result<Vec<image::Frame>, image::ImageError> {
        match kagi_domain::diff::media_kind(bytes) {
            Some(kagi_domain::diff::MediaKind::Gif) => {
                image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))?
                    .into_frames()
                    .collect_frames()
            }
            Some(kagi_domain::diff::MediaKind::Webp) => {
                let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes))?;
                if decoder.has_animation() {
                    decoder.into_frames().collect_frames()
                } else {
                    Ok(vec![image::Frame::new(
                        image::DynamicImage::from_decoder(decoder)?.into_rgba8(),
                    )])
                }
            }
            _ => Ok(vec![image::Frame::new(
                image::load_from_memory(bytes)?.into_rgba8(),
            )]),
        }
    })();
    let mut frames = frames.map_err(|error| gpui::SharedString::from(error.to_string()))?;
    let dimensions = frames
        .first()
        .ok_or_else(|| gpui::SharedString::from("No image frames"))?
        .buffer()
        .dimensions();
    for frame in &mut frames {
        for pixel in frame.buffer_mut().pixels_mut() {
            pixel.0.swap(0, 2);
        }
    }
    Ok((
        std::sync::Arc::new(gpui::RenderImage::new(frames)),
        dimensions,
    ))
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

fn image_pair(pair: &SidePair) -> Option<DiffImagePair> {
    let encoded = |side: &SideContent| match side {
        SideContent::Image { encoded, .. } => Some(encoded.clone()),
        _ => None,
    };
    let images = DiffImagePair {
        old_exists: pair.left.is_present(),
        new_exists: pair.right.is_present(),
        old: encoded(&pair.left),
        new: encoded(&pair.right),
    };
    (images.old.is_some() || images.new.is_some()).then_some(images)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_git::backend::file_reads::FileReadSource;
    use std::path::Path;

    fn repository() -> (tempfile::TempDir, kagi_git::Backend) {
        let temp = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(temp.path())
            .status()
            .unwrap()
            .success());
        let backend = kagi_git::Backend::open(temp.path()).unwrap();
        (temp, backend)
    }

    #[test]
    fn bm_text_keeps_ordinary_diff_hunks() {
        let (temp, backend) = repository();
        let path = Path::new("ordinary.txt");
        for prefix in ["BMI,height", "BM25 ranking"] {
            std::fs::write(temp.path().join(path), format!("{prefix}\nold\n")).unwrap();
            assert!(std::process::Command::new("git")
                .args(["add", "ordinary.txt"])
                .current_dir(temp.path())
                .status()
                .unwrap()
                .success());
            std::fs::write(temp.path().join(path), format!("{prefix}\nnew\n")).unwrap();
            let diff = backend.unstaged_file_diff(path).unwrap();
            assert!(!diff.is_binary && !diff.hunks.is_empty());
            assert!(prepare_diff_media(
                &backend,
                &diff,
                &MainDiffSource::Unstaged { path: path.into() },
                path,
                None,
                None,
            )
            .is_none());
        }
    }

    #[test]
    fn selectable_text_budget_is_inclusive_and_preserves_metadata() {
        let cap = crate::ui::oplog_render::SELECTABLE_DETAIL_MAX;
        for size in [cap - 1, cap, cap + 1, IMAGE_SIZE_CAP] {
            let side = content_from_bytes(
                Path::new("text"),
                vec![b'x'; size],
                SideMetadata {
                    size: Some(size as u64),
                    oid: Some("frozen".into()),
                    ..SideMetadata::default()
                },
                false,
            );
            assert_eq!(side.metadata().unwrap().size, Some(size as u64));
            assert_eq!(side.metadata().unwrap().oid.as_deref(), Some("frozen"));
            assert!(side.metadata().unwrap().error.is_none());
            if size <= cap {
                assert!(matches!(side, SideContent::Text { text, .. } if text.len() == size));
            } else {
                assert!(matches!(side, SideContent::Binary(_)));
            }
        }
    }

    #[test]
    fn attributed_large_utf8_diff_uses_binary_metadata() {
        let (temp, backend) = repository();
        let path = Path::new("data.json");
        std::fs::write(temp.path().join(".gitattributes"), "data.json -diff\n").unwrap();
        let text = "é".repeat(crate::ui::oplog_render::SELECTABLE_DETAIL_MAX);
        std::fs::write(temp.path().join(path), &text).unwrap();
        assert!(std::process::Command::new("git")
            .args(["add", ".gitattributes", "data.json"])
            .current_dir(temp.path())
            .status()
            .unwrap()
            .success());
        std::fs::write(temp.path().join(path), format!("{text}\n")).unwrap();
        let diff = backend.unstaged_file_diff(path).unwrap();
        assert!(diff.is_binary);
        let pair = prepare_diff_media(
            &backend,
            &diff,
            &MainDiffSource::Unstaged { path: path.into() },
            path,
            None,
            None,
        )
        .unwrap();
        for (side, size) in [(&pair.left, text.len()), (&pair.right, text.len() + 1)] {
            assert!(matches!(side, SideContent::Binary(_)));
            assert_eq!(side.metadata().unwrap().size, Some(size as u64));
            assert!(side.metadata().unwrap().error.is_none());
        }
        assert!(pair.left.metadata().unwrap().oid.is_some());
    }

    #[test]
    fn oversized_frozen_blob_is_rejected_before_payload_probe() {
        let (temp, backend) = repository();
        let size = PDF_SIZE_CAP + 1;
        std::fs::write(temp.path().join("blob"), vec![b'x'; size]).unwrap();
        let output = std::process::Command::new("git")
            .args(["hash-object", "-w", "blob"])
            .current_dir(temp.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let oid = String::from_utf8(output.stdout).unwrap();
        let oid = oid.trim();
        let object = temp
            .path()
            .join(".git/objects")
            .join(&oid[..2])
            .join(&oid[2..]);
        // Keep the complete deflate stream and readable ODB header, but damage
        // its checksum. A payload probe must fail; truncating the stream instead
        // can leave libgit2's inflater waiting indefinitely for its missing end.
        let mut compressed = std::fs::read(&object).unwrap();
        *compressed.last_mut().unwrap() ^= 1;
        // Git creates read-only loose objects; replace this fixture object.
        std::fs::remove_file(&object).unwrap();
        std::fs::write(&object, &compressed).unwrap();
        let source = FileReadSource::ConflictStage {
            oid,
            mode: 0o100644,
        };
        assert_eq!(
            backend.file_size(source, Path::new("blob")).unwrap(),
            Some(size as u64)
        );
        let side = prepare_content(&backend, MediaSide::File(source, Path::new("blob")));
        assert!(matches!(
            side,
            SideContent::Binary(SideMetadata {
                size: Some(actual),
                error: Some(SideFailure::TooLarge),
                ..
            }) if actual == size as u64
        ));
        assert!(backend
            .file_prefix(source, Path::new("blob"), &mut [0; 12])
            .is_err());
    }

    #[test]
    fn shared_preparation_preserves_diff_media_snapshot() {
        let (temp, backend) = repository();
        let path = Path::new("image.png");
        let png = b"\x89PNG\r\n\x1a\n";
        std::fs::write(temp.path().join(path), png).unwrap();
        let [absent, image] = prepare_content_pair(
            &backend,
            [
                MediaSide::Absent,
                MediaSide::File(FileReadSource::Workdir, path),
            ],
        );
        assert!(matches!(absent, SideContent::Absent));
        let SideContent::Image {
            encoded,
            decoded,
            dimensions,
            meta,
        } = image
        else {
            panic!("PNG signature");
        };
        assert_eq!(encoded.bytes(), png);
        assert!(decoded.is_err() && dimensions.is_none());
        assert!(matches!(meta.error, Some(SideFailure::Decode)));
        // A BM text prefix is not a BMP header, even with an image extension.
        std::fs::write(temp.path().join(path), b"BM invalid").unwrap();
        let [_, bmp] = prepare_content_pair(
            &backend,
            [
                MediaSide::Absent,
                MediaSide::File(FileReadSource::Workdir, path),
            ],
        );
        assert!(matches!(bmp, SideContent::Text { text, .. } if text.as_ref() == "BM invalid"));
        std::fs::write(temp.path().join(path), b"%PDF-invalid document").unwrap();
        let [absent, pdf] = prepare_content_pair(
            &backend,
            [
                MediaSide::Absent,
                MediaSide::File(FileReadSource::Workdir, path),
            ],
        );
        assert!(matches!(absent.pdf_source(), PreparedPdfSide::Absent));
        assert!(matches!(pdf.pdf_source(), PreparedPdfSide::Source(_)));
    }

    #[test]
    fn conflict_preparation_preserves_absence_and_read_failure() {
        let (_temp, backend) = repository();
        let [absent, failed] = prepare_content_pair(
            &backend,
            [
                MediaSide::Absent,
                MediaSide::File(
                    FileReadSource::ConflictStage {
                        oid: "invalid",
                        mode: 0o100644,
                    },
                    Path::new("image.png"),
                ),
            ],
        );
        assert!(!absent.is_present() && absent.metadata().is_none());
        assert!(
            failed.is_present()
                && matches!(failed.metadata().unwrap().error, Some(SideFailure::Read(_)))
        );
        let [absent, failed] = prepare_content_pair(
            &backend,
            [MediaSide::Absent, MediaSide::Failed("read failed".into())],
        );
        assert!(!absent.is_present() && absent.metadata().is_none());
        assert!(failed.is_present());
        assert!(
            matches!(&failed.metadata().unwrap().error, Some(SideFailure::Read(error)) if error.as_ref() == "read failed")
        );
    }

    #[test]
    fn frozen_mode_classifies_each_side_without_gitlink_object_lookup() {
        let (_temp, backend) = repository();
        let missing_oid = "1111111111111111111111111111111111111111";
        let [gitlink, blob] = prepare_content_pair(
            &backend,
            [0o160000, 0o100644].map(|mode| {
                MediaSide::File(
                    FileReadSource::ConflictStage {
                        oid: missing_oid,
                        mode,
                    },
                    Path::new("mixed"),
                )
            }),
        );
        assert!(matches!(gitlink, SideContent::Submodule(_)));
        assert!(gitlink.is_present() && gitlink.metadata().unwrap().error.is_none());
        assert_eq!(
            gitlink.metadata().unwrap().oid.as_deref(),
            Some(missing_oid)
        );
        assert!(matches!(blob, SideContent::Binary(_)));
        assert!(blob.is_present() && blob.metadata().unwrap().error.is_some());
    }

    #[test]
    fn shared_caps_are_inclusive_and_failures_remain_present() {
        let (temp, backend) = repository();
        for (magic, cap, pdf) in [
            (b"\x89PNG\r\n\x1a\n".as_slice(), IMAGE_SIZE_CAP, false),
            (b"%PDF-".as_slice(), PDF_SIZE_CAP, true),
        ] {
            for size in [cap, cap + 1] {
                let mut bytes = vec![0; size];
                bytes[..magic.len()].copy_from_slice(magic);
                std::fs::write(temp.path().join("media"), bytes).unwrap();
                let [side, absent] = prepare_content_pair(
                    &backend,
                    [
                        MediaSide::File(FileReadSource::Workdir, Path::new("media")),
                        MediaSide::Absent,
                    ],
                );
                assert!(side.is_present() && !absent.is_present());
                assert_eq!(side.metadata().unwrap().size, Some(size as u64));
                if size > cap {
                    assert!(matches!(
                        side,
                        SideContent::Binary(SideMetadata {
                            error: Some(SideFailure::TooLarge),
                            ..
                        })
                    ));
                } else if pdf {
                    assert!(matches!(side, SideContent::Pdf { .. }));
                } else {
                    assert!(matches!(
                        side,
                        SideContent::Image {
                            decoded: Err(_),
                            ..
                        }
                    ));
                }
            }
        }
    }

    #[test]
    fn genuine_pixels_decode_once_even_with_misleading_extension() {
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::Jpeg,
            image::ImageFormat::Gif,
            image::ImageFormat::WebP,
            image::ImageFormat::Bmp,
        ] {
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                3,
                2,
                image::Rgb([255, 0, 0]),
            ));
            let mut bytes = std::io::Cursor::new(Vec::new());
            image.write_to(&mut bytes, format).unwrap();
            let bytes = bytes.into_inner();
            let path = if format == image::ImageFormat::Png {
                "wrong.jpg"
            } else {
                "wrong.pdf"
            };
            let side = content_from_bytes(
                Path::new(path),
                bytes.clone(),
                SideMetadata::default(),
                false,
            );
            let SideContent::Image {
                encoded,
                decoded,
                dimensions,
                meta,
            } = side.clone()
            else {
                panic!("real image");
            };
            assert_eq!(encoded.bytes(), bytes.as_slice());
            assert_eq!(dimensions, Some((3, 2)));
            assert!(meta.error.is_none());
            let SideContent::Image {
                decoded: Ok(other), ..
            } = side
            else {
                panic!("decoded image");
            };
            assert!(std::sync::Arc::ptr_eq(decoded.as_ref().unwrap(), &other));
        }
    }

    #[test]
    fn animated_gif_retains_all_decoded_frames_and_delays() {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            encoder
                .encode_frames([[255, 0, 0, 255], [0, 255, 0, 255]].map(|color| {
                    image::Frame::from_parts(
                        image::RgbaImage::from_pixel(2, 2, image::Rgba(color)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(50, 1),
                    )
                }))
                .unwrap();
        }
        let (image, dimensions) = decode_image(&bytes).unwrap();
        assert_eq!(dimensions, (2, 2));
        assert_eq!(image.frame_count(), 2);
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
        assert_eq!(&image.as_bytes(1).unwrap()[..4], &[0, 255, 0, 255]);
        assert_eq!(std::time::Duration::from(image.delay(0)).as_millis(), 50);
    }

    #[test]
    fn frozen_symlink_bytes_are_not_followed_or_classified_as_images() {
        let (temp, backend) = repository();
        let bytes = b"\x89PNG\r\n\x1a\n";
        std::fs::write(temp.path().join("blob"), bytes).unwrap();
        let output = std::process::Command::new("git")
            .args(["hash-object", "-w", "blob"])
            .current_dir(temp.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let oid = String::from_utf8(output.stdout).unwrap();
        let sides = prepare_content_pair(
            &backend,
            [0o120000, 0o100644].map(|mode| {
                MediaSide::File(
                    FileReadSource::ConflictStage {
                        oid: oid.trim(),
                        mode,
                    },
                    Path::new("link.png"),
                )
            }),
        );
        assert!(
            matches!(&sides[0], SideContent::Symlink { target: Some(target), meta }
            if target.as_ref() == String::from_utf8_lossy(bytes) && meta.error.is_none())
        );
        assert!(matches!(
            &sides[1],
            SideContent::Image {
                decoded: Err(_),
                ..
            }
        ));
        let plain = content_from_bytes(
            Path::new("false.png"),
            b"ordinary text".to_vec(),
            SideMetadata::default(),
            false,
        );
        assert!(matches!(plain, SideContent::Text { .. }));
        let binary = content_from_bytes(
            Path::new("false.png"),
            vec![0, 1, 2],
            SideMetadata::default(),
            false,
        );
        assert!(matches!(binary, SideContent::Binary(_)));
    }

    #[test]
    fn file_preview_magic_wins_without_changing_avatar_recognition() {
        use crate::ui::avatar_fetch::{classify_image, ImageClassification};
        use gpui::ImageFormat;
        use std::path::Path;
        for extension in ["png", "PNG", "jpg", "jpeg", "gif", "webp", "bmp"] {
            let name = format!("image.{extension}");
            assert!(
                classify_image(Path::new(&name), &[], ImageClassification::FilePreview).is_none()
            );
            assert!(
                classify_image(Path::new(&name), &[], ImageClassification::MagicOnly).is_none()
            );
        }
        let png = b"\x89PNG\r\n\x1a\n";
        assert_eq!(
            classify_image(
                Path::new("image.jpg"),
                png,
                ImageClassification::FilePreview
            ),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            classify_image(Path::new("image.jpg"), png, ImageClassification::MagicOnly),
            Some(ImageFormat::Png)
        );
        assert!(
            classify_image(Path::new("image"), b"BM", ImageClassification::FilePreview).is_none()
        );
        assert!(
            classify_image(Path::new("image"), b"BM", ImageClassification::MagicOnly).is_none()
        );
    }
    #[test]
    fn mixed_image_pdf_and_absence_keep_independent_modes() {
        let (temp, backend) = repository();
        std::fs::write(temp.path().join("one"), b"\x89PNG\r\n\x1a\n").unwrap();
        std::fs::write(temp.path().join("two"), b"%PDF-invalid").unwrap();
        let [left, right] = prepare_content_pair(
            &backend,
            ["one", "two"].map(|path| MediaSide::File(FileReadSource::Workdir, Path::new(path))),
        );
        assert!(matches!(left, SideContent::Image { .. }));
        assert!(matches!(right, SideContent::Pdf { .. }));
        let headers = [
            SideHeader {
                label: "old".into(),
                accent: None,
                id: "old",
                absent: Msg::DiffPdfNoPrevious,
            },
            SideHeader {
                label: "new".into(),
                accent: None,
                id: "new",
                absent: Msg::DiffPdfNoCurrent,
            },
        ];
        for (left, right, expected) in [
            (left.clone(), right, (true, true, true, false)),
            (
                left.clone(),
                SideContent::Absent,
                (true, false, true, false),
            ),
            (SideContent::Absent, left, (false, true, false, true)),
        ] {
            let pair = image_pair(&SidePair {
                left,
                right,
                headers: headers.clone(),
            })
            .unwrap();
            assert_eq!(
                (
                    pair.old_exists,
                    pair.new_exists,
                    pair.old.is_some(),
                    pair.new.is_some()
                ),
                expected
            );
        }
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
            is_lfs: false,
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
