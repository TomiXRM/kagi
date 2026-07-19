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
//! change, so the entity owns the state + lifecycle (create / update-in-place /
//! dispose via the workspace registry) and `workspace::CompareItem` stays a
//! thin function-rendered adapter like `InspectorItem`.

use gpui::{AppContext as _, Context};

use super::diff_view::CompareView;
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
        match self.compare_view.clone() {
            Some(pane) => pane.update(cx, |p, cx| {
                p.view = view;
                cx.notify();
            }),
            None => self.compare_view = Some(cx.new(|_| ComparePane { view })),
        }
    }
}
