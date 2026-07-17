//! Pane-resize divider drag types (T023), moved here from the bin's
//! `src/ui/types.rs` (ADR-0121 C3) so extracted pane crates can start divider
//! drags. The drag-move *handling* (mapping cursor → widths) stays on
//! `KagiApp` in the bin; these are just the shared payload/ghost types.

use gpui::div;

/// Which divider is being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DividerKind {
    /// The divider between the sidebar and the commit list.
    Sidebar,
    /// The divider between the commit list and the detail/diff panel.
    Panel,
    /// T030: The divider between the badge column and the graph column.
    BadgeCol,
    /// T030: The divider between the graph column and the message column.
    GraphCol,
    /// T-BP-002: The divider at the top edge of the bottom panel.
    BottomPanel,
    /// W7-INSPECTOR2: The horizontal divider inside the inspector between the
    /// message scroll box (top) and the changed-files list (bottom).
    InspectorSplit,
    /// T-CONFLICT-UI-003: vertical divider between the A (Current) and B
    /// (Incoming) panes in the Conflict Editor (adjusts the A|B width ratio).
    ConflictAB,
    /// T-CONFLICT-UI-003: horizontal divider between the A·B row (top) and the
    /// Result pane (bottom) in the Conflict Editor (adjusts that split ratio).
    ConflictResult,
    /// ADR-0089: horizontal divider between the File History commit list (top)
    /// and the diff viewer (bottom).  Adjusts the list/diff vertical split.
    FileHistoryRows,
    /// T-WS-EDITOR-004: vertical divider between the Editor Workspace's left
    /// file tree and its center code viewer. Adjusts `EditorWorkspaceView::tree_w`.
    EditorTree,
    /// T-WS-EDITOR-004: vertical divider between the Editor Workspace's center
    /// code viewer and its right hunks pane. Adjusts `EditorWorkspaceView::hunks_w`.
    EditorHunks,
}

/// Drag payload for a divider drag.  Only the divider kind is needed: widths
/// are derived from the absolute cursor position during drag-move (see the
/// drag-move listener), so no drag-start anchor has to be carried around.
#[derive(Clone, Copy, Debug)]
pub struct DividerDrag {
    pub kind: DividerKind,
}

/// Invisible ghost view rendered during a divider drag.  gpui requires a
/// `Render`-able entity as the drag ghost, so we use this zero-size placeholder.
pub struct DividerGhost;
impl gpui::Render for DividerGhost {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        div()
    }
}
