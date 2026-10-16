//! Localized explanations shared by binary and image diff bodies.
use super::super::{i18n::Msg, theme::theme};
use gpui::{div, prelude::*, px, rgb, Div, SharedString};

pub(crate) fn caption(message: Msg) -> Div {
    let text = message.t();
    #[cfg(feature = "gui-e2e")]
    super::super::e2e::diff_presentation::record_description(text);
    div()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(text))
}

pub(crate) fn image_panel(pair: super::super::diff_view::DiffImagePair) -> gpui::AnyElement {
    let state = match (pair.old_exists, pair.new_exists) {
        (true, true) => Msg::DiffImageModified,
        (true, false) => Msg::DiffImageDeleted,
        (false, true) => Msg::DiffImageAdded,
        (false, false) => Msg::DiffBinaryNoText,
    };
    div()
        .id("main-diff-image-panel")
        .flex_1()
        .min_h(px(0.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .p_4()
        .child(caption(state))
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .flex()
                .flex_row()
                .gap_4()
                .when(pair.old_exists, |el| {
                    el.child(image_column(Msg::DiffImageBefore.t(), pair.old))
                })
                .when(pair.new_exists, |el| {
                    el.child(image_column(Msg::DiffImageAfter.t(), pair.new))
                }),
        )
        .into_any_element()
}

fn image_column(label: &'static str, image: Option<std::sync::Arc<gpui::Image>>) -> Div {
    // An absolute contained image keeps portrait images inside the flex
    // viewport; percentage heights on an in-flow image resolve to zero (#362).
    div()
        .flex_1()
        .min_w(px(0.))
        .self_stretch()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(label)),
        )
        .child(
            div()
                .relative()
                .flex_1()
                .min_h(px(0.))
                .self_stretch()
                .child(match image {
                    Some(image) => gpui::img(gpui::ImageSource::Image(image))
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Contain)
                        .into_any_element(),
                    None => div()
                        .text_sm()
                        .text_color(rgb(theme().text_muted))
                        .child(Msg::DiffImagePreviewUnavailable.t())
                        .into_any_element(),
                }),
        )
}
