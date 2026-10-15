//! Hover help belongs to the live, currently positioned trigger (#1591).
//! GPUI owns the delay and unmount lifetime; Kit owns content and styling.
use gpui::{
    div, AnyElement, AnyView, App, AppContext as _, Bounds, Context, Element, ElementId, Entity,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, ParentElement as _, Pixels, Render,
    RenderOnce, SharedString, StatefulInteractiveElement, Subscription, Window,
};
use gpui_component::tooltip::Tooltip;

/// Attach hover help after configuring the control. The stable ID belongs to
/// this tooltip boundary, independently of the control's own element ID.
pub trait HoverTooltip: StatefulInteractiveElement + IntoElement + Sized + 'static {
    fn hover_tooltip(
        self,
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
    ) -> impl IntoElement {
        let text = text.into();
        self.hover_tooltip_builder(id, move |window, cx| {
            Tooltip::new(text.clone()).build(window, cx)
        })
    }

    fn hover_tooltip_builder(
        self,
        id: impl Into<ElementId>,
        build: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> impl IntoElement {
        HoverTrigger {
            id: id.into(),
            control: self,
            build,
        }
    }
}

impl<T: StatefulInteractiveElement + IntoElement + 'static> HoverTooltip for T {}

#[derive(IntoElement)]
struct HoverTrigger<
    T: StatefulInteractiveElement + IntoElement + 'static,
    F: Fn(&mut Window, &mut App) -> AnyView + 'static,
> {
    id: ElementId,
    control: T,
    build: F,
}

impl<T, F> RenderOnce for HoverTrigger<T, F>
where
    T: StatefulInteractiveElement + IntoElement + 'static,
    F: Fn(&mut Window, &mut App) -> AnyView + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let geometry = window.use_keyed_state(self.id, cx, |_, _| Bounds::<Pixels>::default());
        let live_geometry = geometry.clone();
        let control = StatefulInteractiveElement::tooltip(self.control, move |window, cx| {
            let content = (self.build)(window, cx);
            cx.new(|cx| {
                let activation =
                    cx.observe_window_activation(window, |tooltip: &mut HoverHelp, window, cx| {
                        if !window.is_window_active() {
                            tooltip.cancelled = true;
                            cx.notify();
                        }
                    });
                HoverHelp {
                    content,
                    geometry: live_geometry.clone(),
                    cancelled: false,
                    _activation: activation,
                }
            })
            .into()
        });
        TriggerGeometry {
            element: control.into_any_element(),
            geometry,
        }
    }
}

// Forward the control's exact layout rather than inserting a Div or adding a
// child. Kit uses children.is_empty() to choose icon-button size and padding.
struct TriggerGeometry {
    element: AnyElement,
    geometry: Entity<Bounds<Pixels>>,
}

impl IntoElement for TriggerGeometry {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TriggerGeometry {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.element.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let visible = bounds.intersect(&window.content_mask().bounds);
        self.geometry.update(cx, |geometry, _| *geometry = visible);
        self.element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element.paint(window, cx);
    }
}

struct HoverHelp {
    content: AnyView,
    geometry: Entity<Bounds<Pixels>>,
    cancelled: bool,
    _activation: Subscription,
}

impl Render for HoverHelp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds = *self.geometry.read(cx);
        // Native tooltip callbacks retain the bounds at hover entry. Reconcile
        // actual geometry on every frame, including layout/scroll without input.
        self.cancelled |= !bounds.contains(&window.mouse_position());
        // Reactivation or moving back must not resurrect stale help.
        div().children((!self.cancelled).then(|| self.content.clone()))
    }
}

#[cfg(all(test, feature = "gui-e2e"))]
mod tests {
    use super::*;
    use gpui::{
        point, px, InteractiveElement as _, Styled as _, TestAppContext, VisualTestContext,
    };
    use std::{cell::Cell, rc::Rc, time::Duration};

    struct Content(Rc<Cell<usize>>);
    impl Render for Content {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            div().child("Hover help")
        }
    }

    struct Trigger(Rc<Cell<usize>>, gpui::Pixels);
    impl Render for Trigger {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let rendered = self.0.clone();
            div().size_full().child(
                div()
                    .id("trigger")
                    .ml(self.1)
                    .size(px(100.))
                    .hover_tooltip_builder("trigger-help", move |_, cx| {
                        cx.new(|_| Content(rendered.clone())).into()
                    }),
            )
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear();
        });
    }

    #[gpui::test]
    fn deactivation_cancels_help_until_a_fresh_hover(cx: &mut TestAppContext) {
        let rendered = Rc::new(Cell::new(0));
        let (_, cx) = cx.add_window_view(|_, _| Trigger(rendered.clone(), px(0.)));
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        draw(cx);
        cx.simulate_mouse_move(point(px(50.), px(50.)), None, Default::default());
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        draw(cx);
        assert!(rendered.get() > 0, "hover help is actually rendered");

        cx.deactivate_window();
        let before = rendered.get();
        draw(cx);
        assert_eq!(rendered.get(), before, "deactivation hides help");
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        draw(cx);
        assert_eq!(
            rendered.get(),
            before,
            "reactivation does not resurrect help"
        );

        cx.simulate_mouse_move(point(px(150.), px(150.)), None, Default::default());
        draw(cx);
        cx.simulate_mouse_move(point(px(50.), px(50.)), None, Default::default());
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        draw(cx);
        assert!(rendered.get() > before, "fresh hover builds visible help");
    }

    #[gpui::test]
    fn moving_a_live_trigger_cancels_help_without_mouse_input(cx: &mut TestAppContext) {
        let rendered = Rc::new(Cell::new(0));
        let (trigger, cx) = cx.add_window_view(|_, _| Trigger(rendered.clone(), px(0.)));
        draw(cx);
        cx.simulate_mouse_move(point(px(50.), px(50.)), None, Default::default());
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        draw(cx);
        assert!(rendered.get() > 0, "hover help is actually rendered");
        let before = rendered.get();
        cx.update(|_, cx| {
            trigger.update(cx, |trigger, cx| {
                trigger.1 = px(200.);
                cx.notify();
            });
        });
        draw(cx);
        assert_eq!(rendered.get(), before, "moving trigger hides its old help");
        cx.update(|_, cx| {
            trigger.update(cx, |trigger, cx| {
                trigger.1 = px(0.);
                cx.notify();
            });
        });
        draw(cx);
        assert_eq!(rendered.get(), before, "moving back cannot resurrect help");
        cx.simulate_mouse_move(point(px(150.), px(150.)), None, Default::default());
        draw(cx);
        cx.simulate_mouse_move(point(px(50.), px(50.)), None, Default::default());
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        draw(cx);
        assert!(
            rendered.get() > before,
            "fresh hover after movement shows help"
        );
    }
}
