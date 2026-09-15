//! ADR-0121 B2: the read-only Compare mode (ADR-0026) as an entity, registered
//! as the `RightPane::Compare` [`super::workspace::WorkspaceItem`]
//! (`workspace::CompareItem`).
//!
//! What moved in here from `KagiApp`:
//! - the [`CompareView`] itself (was `KagiApp.compare_view: Option<CompareView>`;
//!   the field is now `Option<Entity<ComparePane>>`).
//!
//! Unlike `MainDiffPane`, this entity has no `Render` impl: compare mode draws
//! as the Inspector body (compare banner + compare changed-files list) via
//! `inspector::render_inspector`, whose listeners are `Context<KagiApp>`
//! listeners shared with the plain Inspector. Entity-rendering compare would
//! mean duplicating that renderer with weak-handle listeners for zero behavior
//! change, so the entity owns the state and its `TabUiState` owns the lifetime.
//! `workspace::CompareItem` stays a thin function-rendered adapter like
//! `InspectorItem`.

use gpui::{AppContext as _, Context};

use super::diff_view::{CompareTarget, CompareView};
use super::KagiApp;

/// Entity for the active compare (base commit ↔ HEAD / working tree).
pub struct ComparePane {
    /// The compare currently shown (base, target, changed files, title).
    pub view: CompareView,
}

impl KagiApp {
    /// Show `view` in compare mode: update the live entity in place or create
    /// it on first open (mirrors `show_main_diff`).
    pub(crate) fn show_compare(&mut self, view: CompareView, cx: &mut Context<Self>) {
        match self.ui().compare_view.clone() {
            Some(pane) => pane.update(cx, |p, cx| {
                p.view = view;
                cx.notify();
            }),
            None => {
                if let Some(ui) = self.ui_mut() {
                    ui.compare_view = Some(cx.new(|_| ComparePane { view }));
                }
            }
        }
    }

    /// Re-read a retained compare against the snapshot a reload just installed.
    /// The entity and its mode survive; only repository-derived file data is
    /// replaced. A compare that can no longer be read (no session, no HEAD, a
    /// git error) keeps its last non-authoritative display until a later reload
    /// can refresh it.
    pub(crate) fn restore_compare(&mut self, view: CompareView, cx: &mut Context<Self>) {
        let files = {
            let Some(session) = self.ui().repo_session.as_ref() else {
                return;
            };
            let repo = session.backend();
            match &view.target {
                CompareTarget::Head => match repo.head_commit_id() {
                    Some(head) => repo.compare_commits(&view.base, &head),
                    None => return,
                },
                CompareTarget::WorkingTree => repo.compare_commit_to_workdir(&view.base),
                CompareTarget::Commit(id) => repo.compare_commits(&view.base, id),
            }
        };
        match files {
            Ok(files) => self.show_compare(CompareView { files, ..view }, cx),
            Err(e) => klog!("compare refresh error: {}", e),
        }
    }
}
