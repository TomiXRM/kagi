use gpui::{div, prelude::*, px, rgb, AnyElement, Context};
use gpui_component::scroll::ScrollableElement;

use super::render_main_issue_list;
use crate::ui::{e2e, issues_composer, issues_thread, theme::theme, KagiApp};

pub(super) fn render_center(
    app: &KagiApp,
    window: &mut gpui::Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let selected = app.ui().selected_github_issue;
    let editors = &app.ui().issue_composer.editors;
    let home_focused = editors.get(&None).is_some_and(|editor| editor.focused);
    let mut center = div()
        .id("issue-mode-center-pane")
        .relative()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .overflow_hidden()
        .flex()
        .flex_col()
        .bg(rgb(theme().bg_base))
        // Keep the pane's h_full/flex_1 chain intact: an auto-height probe
        // wrapper would let content extend outside the clickable window.
        .child(e2e::measure_inside("issue-mode-center-pane"));
    match selected {
        Some(number) => {
            if editors
                .get(&Some(number))
                .is_some_and(|editor| editor.focused)
            {
                center = center.child(issues_composer::render_composer(
                    app,
                    Some(number),
                    window,
                    cx,
                ));
            } else {
                center = center.child(issues_thread::render_thread(app, window, cx));
            }
        }
        None => {
            if home_focused {
                center = center.child(issues_composer::render_composer(app, None, window, cx));
            } else {
                center = center.child(render_main_issue_list(app, cx));
            }
        }
    }
    if home_focused
        || selected.is_some_and(|number| {
            editors
                .get(&Some(number))
                .is_some_and(|editor| editor.focused)
        })
    {
        // Scrollable's auto-height content would make the composer's h_full
        // indefinite. Keep this pane as the actual viewport and attach only
        // the scrollbar layer so the body receives the remaining flex height.
        let scroll = window
            .use_keyed_state("issue-focused-scroll", cx, |_, _| {
                gpui::ScrollHandle::default()
            })
            .read(cx)
            .clone();
        center
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .vertical_scrollbar(&scroll)
            .into_any_element()
    } else {
        center.into_any_element()
    }
}
