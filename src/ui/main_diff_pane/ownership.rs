//! Foreground-only ownership checks for a prepared main-diff read.

use super::{KagiApp, MainDiffRead, MainDiffSource};
use gpui::Context;

/// A source may be replaced and return to the same path/base/target while a
/// read is out. Entity identity and Compare's existing revision distinguish
/// that transition without retaining the source pane or allocating a cache.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MainDiffReadOwner {
    Commit,
    Compare(gpui::EntityId, u64),
    Wip(gpui::EntityId),
    WipRefresh(gpui::EntityId),
}

impl MainDiffRead {
    pub(super) fn source_owner(
        &self,
        app: &KagiApp,
        cx: &Context<KagiApp>,
    ) -> Option<MainDiffReadOwner> {
        match self {
            Self::Commit { .. } => Some(MainDiffReadOwner::Commit),
            Self::Compare { .. } => {
                app.ui().compare_view.as_ref().map(|pane| {
                    MainDiffReadOwner::Compare(pane.entity_id(), pane.read(cx).revision())
                })
            }
            // A refresh belongs to the retained diff, even when the panel's
            // empty status makes the panel close before Nothing/error lands.
            Self::Wip { refresh: true, .. } => app
                .ui()
                .main_diff
                .as_ref()
                .map(|pane| MainDiffReadOwner::WipRefresh(pane.entity_id())),
            Self::Wip { .. } => app
                .ui()
                .commit_panel
                .as_ref()
                .map(|pane| MainDiffReadOwner::Wip(pane.entity_id())),
        }
    }

    /// Selection/source changes can close a pane without issuing another read.
    /// Check the captured identity as well as the request/visit/source-owner guard.
    pub(super) fn is_current(
        &self,
        app: &KagiApp,
        repo: &std::path::Path,
        path: &std::path::Path,
        cx: &Context<KagiApp>,
    ) -> bool {
        match self {
            Self::Commit { commit, .. } => {
                app.repo_path.as_deref() == Some(repo)
                    && app
                        .ui()
                        .selected
                        .and_then(|row| app.commit_id_for_row(row))
                        .as_ref()
                        == Some(commit)
            }
            Self::Compare {
                base,
                target,
                file_index,
            } => {
                app.repo_path.as_deref() == Some(repo)
                    && app.ui().compare_view.as_ref().is_some_and(|pane| {
                        let view = pane.read(cx).view();
                        &view.base == base
                            && &view.target == target
                            && view
                                .files
                                .get(*file_index)
                                .is_some_and(|file| file.path == path)
                    })
            }
            Self::Wip { staged, refresh } => {
                if *refresh {
                    // The request already froze this retained pane's repository.
                    // Its empty source panel may disappear before the result lands.
                    return app.ui().main_diff.as_ref().is_some_and(|pane| {
                        match &pane.read(cx).view.source {
                            MainDiffSource::Staged { path: shown } => *staged && shown == path,
                            MainDiffSource::Unstaged { path: shown } => !*staged && shown == path,
                            _ => false,
                        }
                    });
                }
                if app.commit_panel_repo_path(cx).as_deref() != Some(repo) {
                    return false;
                }
                app.ui().commit_panel_open
                    && app.ui().commit_panel.as_ref().is_some_and(|pane| {
                        let panel = &pane.read(cx).state;
                        let files = if *staged {
                            &panel.staged
                        } else {
                            &panel.unstaged
                        };
                        files.iter().any(|file| file.path == path)
                    })
            }
        }
    }
}
