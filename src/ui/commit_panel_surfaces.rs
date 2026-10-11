//! Alternate allocations of the original panel, without alternate panel state.

use super::commit_panel::CommitPanelView;
use super::*;

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
                Surface::Composer => div()
                    .w_full()
                    .h_full()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .when(view.foreign.is_some(), |el| {
                        el.child(view.render_panel_header())
                    })
                    .child(view.render_composer(true, pcx))
                    .into_any_element(),
                Surface::Changes => view.render_changes(pcx).into_any_element(),
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
            .child(
                div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.render_changes_column(false, cx))
                    .child(self.render_changes_column(true, cx)),
            )
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
                self.render_staged_header(true, cx)
            } else {
                self.render_unstaged_header(true, cx)
            })
            .child(if staged {
                self.render_staged_list(cx)
            } else {
                self.render_unstaged_list(cx)
            })
    }
}
