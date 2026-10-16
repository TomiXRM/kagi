//! A pinned kit Popover projected from the existing CommitPanelView.
use gpui::{
    div, point, prelude::*, px, rgb, size, Anchor, AnyElement, App, Bounds, Context, IntoElement,
    Pixels, RenderOnce, Window,
};
use gpui_component::{popover::Popover, ElementExt, Selectable};
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::{e2e, i18n::Msg, keyboard_nav, theme, KagiApp};

/// A layout anchor, not another interactive control or Tab stop.
#[derive(IntoElement)]
struct ComposerAnchor;
impl Selectable for ComposerAnchor {
    fn selected(self, _: bool) -> Self {
        self
    }
    fn is_selected(&self) -> bool {
        false
    }
}
impl RenderOnce for ComposerAnchor {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().w(px(0.)).h(px(0.))
    }
}

/// Prefer directly below; flip above when needed; clamp if neither side fits.
fn placement(
    anchor: Bounds<Pixels>,
    viewport: gpui::Size<Pixels>,
    preferred: gpui::Size<Pixels>,
    margin: Pixels,
    gap: Pixels,
) -> Bounds<Pixels> {
    let margin = margin.min(viewport.width / 2.).min(viewport.height / 2.);
    let width = preferred
        .width
        .min((viewport.width - margin * 2.).max(px(0.)));
    let height = preferred
        .height
        .min((viewport.height - margin * 2.).max(px(0.)));
    let max_x = (viewport.width - margin - width).max(margin);
    let max_y = (viewport.height - margin - height).max(margin);
    let below = anchor.bottom() + gap;
    let y = if below + height <= viewport.height - margin {
        below
    } else {
        anchor.top() - gap - height
    };
    Bounds::new(
        point(anchor.left().clamp(margin, max_x), y.clamp(margin, max_y)),
        size(width, height),
    )
}

impl KagiApp {
    pub(super) fn render_compact_composer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.compact.enabled
            || self.compact.popup_owner != self.active_session()
            || self.compact.suspended
            || self.home_in_front()
        {
            return None;
        }
        let anchor = self.compact.wip_bounds.get()?;
        let composer = self.compact_surface(true, cx)?;
        let viewport = window.viewport_size();
        let margin = theme::scaled_px(8.).min(viewport.height / 2.);
        let max_height = theme::scaled_px(360.).min((viewport.height - margin * 2.).max(px(0.)));
        let bounds = placement(
            anchor,
            viewport,
            size(
                theme::scaled_px(520.),
                self.compact
                    .popup_height
                    .get()
                    .unwrap_or(max_height)
                    .min(max_height),
            ),
            margin,
            theme::scaled_px(4.),
        );
        let popover = composer_popover(
            &self.compact,
            bounds,
            max_height,
            composer,
            cx.entity_id(),
            cx.weak_entity(),
        );
        Some(
            div()
                .absolute()
                .left(bounds.left())
                .top(bounds.top())
                .w(px(0.))
                .h(px(0.))
                .child(popover)
                .into_any_element(),
        )
    }
}

fn composer_popover(
    state: &super::compact::CompactState,
    bounds: Bounds<Pixels>,
    max_height: Pixels,
    composer: gpui::Entity<super::commit_panel_surfaces::CommitPanelSurfaceView>,
    parent: gpui::EntityId,
    weak_app: gpui::WeakEntity<KagiApp>,
) -> Popover {
    let cache = state.popup_state.clone();
    let preserve_pin = state.preserve_pin.clone();
    let close_app = weak_app.clone();
    let owner = state.popup_owner;
    let measured_height = state.popup_height.clone();
    Popover::new("compact-composer-popover")
        .anchor(Anchor::TopLeft)
        .trigger(ComposerAnchor)
        .open(true)
        .appearance(false)
        .overlay_closable(false)
        .top(px(0.))
        .on_open_change(move |open, _, cx| {
            if !open && !preserve_pin.get() {
                let app = weak_app.clone();
                cx.defer(move |cx| {
                    let _ = app.update(cx, |app, cx| {
                        if app.active_session() == owner && app.compact.popup_owner == owner {
                            app.compact.popup_owner = None;
                            cx.notify();
                        }
                    });
                });
            }
        })
        .content(move |_, _, cx| {
            *cache.borrow_mut() = Some(cx.entity());
            div()
                .id("compact-composer-content")
                .relative()
                .w(bounds.size.width)
                .max_h(max_height)
                .flex()
                .flex_col()
                .min_h(px(0.))
                .bg(rgb(theme::theme().panel))
                .border_1()
                .border_color(rgb(theme::theme().selected))
                .rounded(theme::scaled_px(8.))
                .shadow_lg()
                .child(e2e::measure_inside("compact-composer"))
                .child(composer_header(close_app.clone()))
                .child(composer.clone())
                .on_prepaint({
                    let measured_height = measured_height.clone();
                    move |bounds, window, cx| {
                        if measured_height.replace(Some(bounds.size.height))
                            != Some(bounds.size.height)
                        {
                            // Use the actual content height for below/above placement
                            // on the next frame, without rendering a second input tree.
                            cx.notify(parent);
                            window.request_animation_frame();
                        }
                    }
                })
        })
}

fn composer_header(app: gpui::WeakEntity<KagiApp>) -> impl IntoElement {
    let close = keyboard_nav::focusable(div().id("compact-popup-close"))
        .role(gpui::Role::Button)
        .aria_label(Msg::CompactCommitClose.t())
        .w(theme::scaled_px(28.))
        .h(theme::scaled_px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::scaled_px(4.))
        .cursor_pointer()
        .text_color(rgb(theme::theme().text_muted))
        .hover(|style| style.bg(rgb(theme::theme().surface)))
        .on_click(move |_, window, cx| {
            let _ = app.update(cx, |app, cx| {
                app.close_compact_composer(window, cx);
                cx.notify();
            });
        })
        .child("×")
        .relative()
        .child(e2e::measure_inside("compact-popup-close"))
        .hover_tooltip_builder("compact-popup-close-help", |window, cx| {
            gpui_component::tooltip::Tooltip::new(Msg::CompactCommitClose.t()).build(window, cx)
        });
    div()
        .flex()
        .items_center()
        .justify_between()
        .flex_shrink_0()
        .px_2()
        .py_1()
        .text_sm()
        .text_color(rgb(theme::theme().text_main))
        .child(Msg::CompactCommitComposer.t())
        .child(close)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn anchor(y: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(44.), px(y)), size(px(896.), px(24.)))
    }
    #[test]
    fn compact_popup_prefers_below_and_flips_above() {
        let viewport = size(px(940.), px(660.));
        let preferred = size(px(520.), px(300.));
        let below = placement(anchor(100.), viewport, preferred, px(8.), px(4.));
        assert_eq!(below.top(), px(128.));
        let above = placement(anchor(550.), viewport, preferred, px(8.), px(4.));
        assert_eq!(above.bottom(), px(546.));
    }
    #[test]
    fn compact_popup_clamps_small_windows_without_overflow() {
        let viewport = size(px(320.), px(180.));
        let popup = placement(
            anchor(100.),
            viewport,
            size(px(782.), px(541.)),
            px(12.),
            px(6.),
        );
        assert_eq!(
            popup,
            Bounds::new(point(px(12.), px(12.)), size(px(296.), px(156.)))
        );
    }
}
