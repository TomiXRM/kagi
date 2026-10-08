//! Background-only text and encoded image-side preparation for main diffs.

use super::{
    build_main_diff_view, CompareTarget, DiffImagePair, Landed, MainDiffRead, MainDiffSource,
};

impl MainDiffRead {
    /// Open `repo` read-only, diff `path`, and build the text rows — all of
    /// the I/O and projection, none of it on the UI thread.
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
                let mut view = build_main_diff_view(&file_diff, path, file_index, source);
                view.images = prepare_diff_images(
                    &backend,
                    &file_diff,
                    &view.source,
                    path,
                    compare_head.as_ref(),
                );
                Landed::Show(Box::new((file_diff, view)))
            }
            Err(e) => Landed::Failed(e.to_string()),
        }
    }
}

/// Gather bytes while the diff's Backend is already open on the worker.
/// Format sniffing wraps encoded bytes in GPUI images; it does not establish
/// that GPUI has decoded any pixels before publication.
fn prepare_diff_images(
    backend: &kagi_git::Backend,
    file_diff: &kagi_git::FileDiff,
    source: &MainDiffSource,
    path: &std::path::Path,
    compare_head: Option<&kagi_git::CommitId>,
) -> Option<DiffImagePair> {
    if !file_diff.is_binary {
        return None;
    }
    let workdir_bytes = || {
        backend
            .workdir()
            .and_then(|root| std::fs::read(root.join(path)).ok())
    };
    let (old, new) = match source {
        MainDiffSource::Synthetic => return None,
        MainDiffSource::Commit {
            commit: Some(commit),
            ..
        } => {
            let old = backend
                .first_parent(commit)
                .ok()
                .flatten()
                .and_then(|parent| backend.blob_bytes_at(&parent, path).ok().flatten());
            (old, backend.blob_bytes_at(commit, path).ok().flatten())
        }
        MainDiffSource::Commit { commit: None, .. } => return None,
        MainDiffSource::Compare { base, target, .. } => {
            let old = backend.blob_bytes_at(base, path).ok().flatten();
            let new = match target {
                CompareTarget::Head => {
                    compare_head.and_then(|head| backend.blob_bytes_at(head, path).ok().flatten())
                }
                CompareTarget::WorkingTree => workdir_bytes(),
                CompareTarget::Commit(commit) => backend.blob_bytes_at(commit, path).ok().flatten(),
            };
            (old, new)
        }
        MainDiffSource::Unstaged { .. } => {
            let old = backend
                .blob_bytes_index(path)
                .ok()
                .flatten()
                .or_else(|| backend.blob_bytes_head(path).ok().flatten());
            (old, workdir_bytes())
        }
        MainDiffSource::Staged { .. } => (
            backend.blob_bytes_head(path).ok().flatten(),
            backend.blob_bytes_index(path).ok().flatten(),
        ),
    };
    let pair = DiffImagePair {
        old: old.and_then(crate::ui::avatar_fetch::image_from_bytes),
        new: new.and_then(crate::ui::avatar_fetch::image_from_bytes),
    };
    (pair.old.is_some() || pair.new.is_some()).then_some(pair)
}
