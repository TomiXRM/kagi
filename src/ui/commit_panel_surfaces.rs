//! Alternate allocations of the original panel, without alternate panel state.

use super::commit_panel::CommitPanelView;
use super::*;

impl KagiApp {
    /// Keep the persisted desktop width, but reserve a readable sliver of graph
    /// after the actual sidebar and divider footprints at the current zoom.
    pub(super) fn commit_panel_viewport_width(&self, window: &Window) -> f32 {
        let scale = f32::from(super::theme::scaled_px(1.));
        let sidebar = if super::compact::for_window(window) {
            f32::from(super::theme::scaled_px(super::compact::RAIL_WIDTH))
        } else {
            f32::from(super::theme::scaled_px(self.sidebar.width + 4.))
                * self.panel_motion.left.visible(kagi_ui_core::motion::now())
        };
        let available = (f32::from(window.viewport_size().width) - sidebar - 120.) / scale - 4.;
        self.panel_width.min(available.max(0.))
    }

    /// Publish owner-scoped inputs once for either desktop or compact projection.
    pub(super) fn prepare_commit_panel_presentation(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<CommitPanelView>> {
        let entity = self.ui().commit_panel.clone()?;
        let active_wip = match self
            .ui()
            .main_diff
            .as_ref()
            .map(|d| d.read(cx).view.source.clone())
        {
            Some(MainDiffSource::Unstaged { path }) => Some((false, path)),
            Some(MainDiffSource::Staged { path }) => Some((true, path)),
            _ => None,
        };
        let smart = self.smart_commit.clone();
        let smart_ui = &self.ui[&entity.read(cx).owner];
        let panel_width = self.commit_panel_viewport_width(window);
        entity.update(cx, |v, _| {
            v.active_wip = active_wip;
            v.panel_render_width = panel_width;
            v.smart_snapshot = smart;
            v.smart_generating = smart_ui.smart_commit_generating;
            v.smart_generation = smart_ui.smart_commit_generation.clone();
            v.smart_status = smart_ui.smart_commit_status.clone();
            #[cfg(feature = "gui-e2e")]
            {
                v.window_active_override = self.window_active_override;
            }
        });
        Some(entity)
    }
}

pub(super) struct CommitPanelSurfaces {
    pub panel: gpui::EntityId,
    pub composer: Entity<CommitPanelSurfaceView>,
    pub changes: Entity<CommitPanelSurfaceView>,
}

impl CommitPanelSurfaces {
    pub(super) fn new(panel: &Entity<CommitPanelView>, cx: &mut App) -> Self {
        Self {
            panel: panel.entity_id(),
            composer: cx.new(|cx| CommitPanelSurfaceView::new(panel, Surface::Composer, cx)),
            changes: cx.new(|cx| CommitPanelSurfaceView::new(panel, Surface::Changes, cx)),
        }
    }
}

#[derive(Clone, Copy)]
enum Surface {
    Composer,
    Changes,
}

pub(super) struct CommitPanelSurfaceView {
    panel: gpui::WeakEntity<CommitPanelView>,
    surface: Surface,
    _observe: gpui::Subscription,
}

impl CommitPanelSurfaceView {
    fn new(panel: &Entity<CommitPanelView>, surface: Surface, cx: &mut Context<Self>) -> Self {
        Self {
            panel: panel.downgrade(),
            surface,
            _observe: cx.observe(panel, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for CommitPanelSurfaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(panel) = self.panel.upgrade() else {
            return div().into_any_element();
        };
        panel.update(cx, |view, pcx| {
            view.prepare_render(window, pcx);
            match self.surface {
                Surface::Composer => {
                    // Only the projection drawing the picker may reconcile it.
                    // Changes also prepares this owner, but a second reconcile
                    // before lazy-list layout would mistake an End destination
                    // for a row scrolled away and hand its focus back.
                    view.reconcile_coauthor_menu(window, pcx);
                    div()
                        .w_full()
                        .min_h(px(0.))
                        .flex_shrink(1.)
                        .flex()
                        .flex_col()
                        .when(view.foreign.is_some(), |el| {
                            el.child(view.render_panel_header())
                        })
                        .child(view.render_composer(true, window, pcx))
                        .into_any_element()
                }
                Surface::Changes => {
                    // A second reconcile from Composer would consume the focus
                    // transition before lazy-list layout reveals its destination.
                    view.reconcile_file_rows(window, pcx);
                    view.render_changes(pcx).into_any_element()
                }
            }
        })
    }
}

impl CommitPanelView {
    fn render_changes(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        div()
            .id("cp-files-container")
            .relative()
            .w_full()
            .h_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .flex()
            .flex_col()
            .bg(rgb(theme().panel))
            .child(super::e2e::measure_inside("cp-files"))
            .when(self.foreign.is_some(), |el| {
                el.child(self.render_panel_header())
            })
            .when(self.status_request.is_loading(), |el| {
                el.child(Self::render_status_loading())
            })
            .when(!self.status_request.is_loading(), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .w_full()
                        .flex_1()
                        .min_h(px(0.))
                        .child(self.render_changes_column(false, cx))
                        .child(self.render_changes_column(true, cx)),
                )
            })
    }

    fn render_changes_column(&self, staged: bool, cx: &mut Context<Self>) -> gpui::Div {
        let marker = if staged {
            "compact-changes-staged"
        } else {
            "compact-changes-unstaged"
        };
        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(super::e2e::measure_inside(marker))
            .child(if staged {
                super::commit_panel_controls::staged_header(self, true, cx)
            } else {
                super::commit_panel_controls::unstaged_header(self, true, cx)
            })
            .child(if staged {
                self.render_staged_list(cx)
            } else {
                self.render_unstaged_list(cx)
            })
    }
}
