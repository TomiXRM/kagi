//! Shared immutable side presentation. Hosts alone supply adoption controls.
use super::super::{i18n::Msg, theme::theme};
use crate::ui::diff_view::{pdf::DiffPdfViewers, SideContent, SideFailure, SidePair};
use gpui::{div, prelude::*, px, rgb, Div, SharedString};

pub(crate) fn caption(message: Msg) -> Div {
    let text = message.t();
    #[cfg(feature = "gui-e2e")]
    if matches!(
        message,
        Msg::DiffBinaryNoText
            | Msg::DiffImageAdded
            | Msg::DiffImageModified
            | Msg::DiffImageDeleted
    ) {
        super::super::e2e::diff_presentation::record_description(text);
    }
    div()
        .text_sm()
        .text_color(rgb(theme().text_main))
        .child(SharedString::from(text))
}

fn failure_message(error: &SideFailure) -> Msg {
    match error {
        SideFailure::TooLarge => Msg::SidePreviewTooLarge,
        SideFailure::Decode => Msg::SidePreviewDecodeFailed,
        SideFailure::Read(_) => Msg::SidePreviewReadFailed,
    }
}

pub(crate) fn side_header(label: SharedString, accent: Option<u32>) -> Div {
    use crate::ui::theme::scaled_px;
    div()
        .flex()
        .items_center()
        .gap(scaled_px(8.))
        .h(scaled_px(26.))
        .flex_shrink_0()
        .px(scaled_px(12.))
        .bg(rgb(theme().panel))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .when_some(accent, |el, accent| {
            el.child(
                div()
                    .size(scaled_px(8.))
                    .rounded_full()
                    .bg(rgb(accent))
                    .flex_shrink_0(),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(scaled_px(11.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(theme().text_main))
                .child(label),
        )
}

/// Maximum source extent establishes one scale and top-left origin for both sides.
fn image_extent(dimensions: [Option<(u32, u32)>; 2]) -> (u32, u32) {
    dimensions
        .into_iter()
        .flatten()
        .fold((1, 1), |extent, (w, h)| (extent.0.max(w), extent.1.max(h)))
}

fn image_scale(extent: (u32, u32), viewport: (f32, f32)) -> f32 {
    (viewport.0 / extent.0 as f32)
        .min(viewport.1 / extent.1 as f32)
        .max(0.)
}

type ImageViewports = std::rc::Rc<std::cell::Cell<[Option<gpui::Bounds<gpui::Pixels>>; 2]>>;
type SizedImage = (std::sync::Arc<gpui::RenderImage>, (u32, u32));

pub(crate) fn side_pair(
    pair: SidePair,
    actions: [Option<gpui::AnyElement>; 2],
    pdfs: Option<&DiffPdfViewers>,
    probes: [&'static str; 2],
) -> gpui::AnyElement {
    let extent = image_extent([&pair.left, &pair.right].map(|side| match side {
        SideContent::Image { dimensions, .. } => *dimensions,
        _ => None,
    }));
    let viewports: ImageViewports = Default::default();
    let images = [&pair.left, &pair.right].map(|side| match side {
        SideContent::Image {
            decoded: Ok(image),
            dimensions: Some(dimensions),
            ..
        } => Some((image.clone(), *dimensions)),
        _ => None,
    });
    let image_labels = pair.headers.each_ref().map(|header| header.label.clone());
    let action_row = actions.iter().any(Option::is_some);
    let mut row = div()
        .relative()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .flex()
        .overflow_hidden();
    for (index, (((content, header), action), probe)) in [pair.left, pair.right]
        .into_iter()
        .zip(pair.headers)
        .zip(actions)
        .zip(probes)
        .enumerate()
    {
        if index != 0 {
            row = row.child(
                div()
                    .w(px(1.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(rgb(theme().surface)),
            );
        }
        // PDF already owns its toolbar/status presentation. A second fixed
        // metadata footer can consume the document viewport at narrow/high-zoom
        // sizes, leaving its scroll bounds outside the clipped body.
        let metadata = match &content {
            SideContent::Pdf { .. } => None,
            _ => content.metadata().cloned(),
        };
        let dimensions = match &content {
            SideContent::Image { dimensions, .. } => *dimensions,
            _ => None,
        };
        let description =
            if let Some(error) = content.metadata().and_then(|meta| meta.error.as_ref()) {
                failure_message(error).t()
            } else {
                match &content {
                    SideContent::Absent => header.absent.t(),
                    SideContent::Pdf { .. } => Msg::DiffPdfPreview.t(),
                    SideContent::Symlink { .. } => Msg::ConflictSymlinkTarget.t(),
                    _ => "",
                }
            };
        let body = body(content, &header, viewports.clone(), pdfs, index == 0, probe);
        let mut column = div()
            .id(header.id)
            .role(gpui::Role::Group)
            .aria_label(header.label.clone())
            .aria_description(description)
            .relative()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(super::super::e2e::measure_inside(format!("{probe}-column")))
            .child(
                side_header(header.label, header.accent)
                    .relative()
                    .child(super::super::e2e::measure_inside(format!("{probe}-header"))),
            );
        if action_row {
            column = column.child(
                div()
                    .h(crate::ui::theme::scaled_px(24.))
                    .flex_shrink_0()
                    .px_2()
                    .flex()
                    .items_center()
                    .children(action),
            );
        }
        column = column.child(body);
        if let Some(meta) = metadata {
            let mut summary = div()
                .relative()
                .px_2()
                .py_1()
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_main))
                .child(super::super::e2e::measure_inside(format!(
                    "{probe}-caption"
                )));
            let caption_text = match (meta.size, dimensions) {
                (Some(size), Some((w, h))) => format!(
                    "{}: {size} B · {}: {w} × {h} px",
                    Msg::ImageFileSize.t(),
                    Msg::ImageDimensions.t()
                ),
                (Some(size), None) => format!("{}: {size} B", Msg::ImageFileSize.t()),
                _ => String::new(),
            };
            #[cfg(feature = "gui-e2e")]
            if dimensions.is_some() {
                super::super::e2e::diff_presentation::record_image_caption(probe, &caption_text);
            }
            summary = summary.child(caption_text);
            if let Some(oid) = meta.oid {
                summary = summary.child(super::super::oplog_render::selectable_text(
                    format!("{probe}-oid"),
                    &oid,
                ));
            }
            if let Some(error) = meta.error {
                summary = summary.child(caption(failure_message(&error)));
                if let SideFailure::Read(detail) = error {
                    summary = summary.child(div().text_sm().child(detail));
                }
            }
            column = column.child(summary);
        }
        row = row.child(
            column.child(super::super::e2e::measure_inside(if index == 0 {
                "conflict-binary-current-body"
            } else {
                "conflict-binary-incoming-body"
            })),
        );
    }
    row.when(images.iter().any(Option::is_some), |row| {
        row.child(image_overlay(
            images,
            image_labels,
            probes,
            extent,
            viewports,
        ))
    })
    .into_any_element()
}

fn body(
    content: SideContent,
    header: &crate::ui::diff_view::SideHeader,
    viewports: ImageViewports,
    pdfs: Option<&DiffPdfViewers>,
    old: bool,
    probe: &'static str,
) -> gpui::AnyElement {
    let image = matches!(
        content,
        SideContent::Image {
            decoded: Ok(_),
            dimensions: Some(_),
            ..
        }
    );
    let pdf = matches!(content, SideContent::Pdf { .. });
    let text = matches!(
        content,
        SideContent::Text { .. } | SideContent::Symlink { .. }
    );
    let body = match content {
        SideContent::Loading => div().into_any_element(),
        SideContent::Absent => caption(header.absent).into_any_element(),
        SideContent::Image {
            decoded: Ok(_),
            dimensions: Some(_),
            ..
        } => gpui::canvas(
            move |bounds, _, _| {
                let mut sizes = viewports.get();
                sizes[usize::from(!old)] = Some(bounds);
                viewports.set(sizes);
            },
            |_, (), _, _| {},
        )
        .size_full()
        .into_any_element(),
        SideContent::Image { .. } => caption(Msg::DiffImagePreviewUnavailable).into_any_element(),
        SideContent::Pdf { .. } => match pdfs {
            Some(pdfs) => pdfs.render_side(old),
            None => caption(Msg::SidePreviewReadFailed).into_any_element(),
        },
        SideContent::Text { text, .. } => {
            super::super::oplog_render::selectable_text(format!("{probe}-text"), &text)
                .font_family(crate::ui::MONO_FONT)
                .into_any_element()
        }
        SideContent::Symlink {
            target: Some(text), ..
        } => div()
            .flex()
            .flex_col()
            .child(caption(Msg::ConflictSymlinkTarget))
            .child(
                super::super::oplog_render::selectable_text(format!("{probe}-text"), &text)
                    .font_family(crate::ui::MONO_FONT),
            )
            .into_any_element(),
        SideContent::Symlink { target: None, .. } => {
            caption(Msg::SidePreviewReadFailed).into_any_element()
        }
        SideContent::Submodule(_) => caption(Msg::ConflictSubmoduleCommit).into_any_element(),
        SideContent::Binary(meta) => caption(if meta.error.is_some() {
            Msg::DiffImagePreviewUnavailable
        } else {
            Msg::DiffBinaryNoText
        })
        .into_any_element(),
    };
    let element = div()
        .id(probe)
        .role(gpui::Role::Group)
        .aria_label(header.label.clone())
        .relative()
        .when(image || pdf, |el| el.flex_1())
        .when(!pdf, |el| el.px_2().pt_1())
        .min_h(px(0.))
        .min_w(px(0.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .when(image, |el| el.max_h(crate::ui::theme::scaled_px(260.)))
        .when(text, |el| el.overflow_y_scroll())
        .child(body)
        .child(super::super::e2e::measure_inside(probe));
    element.into_any_element()
}

/// Both body rectangles have been measured before this trailing overlay prepaints.
/// The upstream Img owns animation and GPU handles; no second decoder or timer.
fn image_overlay(
    images: [Option<SizedImage>; 2],
    labels: [SharedString; 2],
    probes: [&'static str; 2],
    extent: (u32, u32),
    viewports: ImageViewports,
) -> impl IntoElement {
    gpui::canvas(
        move |_, window, cx| {
            let bounds = viewports.get();
            let viewport =
                bounds
                    .into_iter()
                    .flatten()
                    .fold((f32::INFINITY, f32::INFINITY), |a, b| {
                        (
                            a.0.min(f32::from(b.size.width)),
                            a.1.min(f32::from(b.size.height)),
                        )
                    });
            let scale = image_scale(extent, viewport);
            let mut elements: [Option<gpui::AnyElement>; 2] = [None, None];
            for (index, ((image, label), probe)) in
                images.into_iter().zip(labels).zip(probes).enumerate()
            {
                if let (Some((image, (w, h))), Some(bounds)) = (image, bounds[index]) {
                    let size = gpui::size(px(w as f32 * scale), px(h as f32 * scale));
                    let mut element = super::super::e2e::measure_leaf(
                        format!("{probe}-pixels"),
                        gpui::img(gpui::ImageSource::Render(image))
                            .id(("side-pair-image", index))
                            .role(gpui::Role::Image)
                            .aria_label(label)
                            .w(size.width)
                            .h(size.height),
                    );
                    element.layout_as_root(size.map(gpui::AvailableSpace::Definite), window, cx);
                    element.prepaint_at(bounds.origin, window, cx);
                    elements[index] = Some(element);
                }
            }
            elements
        },
        |_, mut elements, window, cx| {
            for element in elements.iter_mut().flatten() {
                element.paint(window, cx);
            }
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    #[test]
    fn accessible_failure_matches_the_visible_reason() {
        use super::{failure_message, Msg, SideFailure};
        assert!(matches!(
            failure_message(&SideFailure::TooLarge),
            Msg::SidePreviewTooLarge
        ));
        assert!(matches!(
            failure_message(&SideFailure::Decode),
            Msg::SidePreviewDecodeFailed
        ));
        assert!(matches!(
            failure_message(&SideFailure::Read("missing frozen object".into())),
            Msg::SidePreviewReadFailed
        ));
    }

    #[test]
    fn unequal_images_use_one_scale_and_origin() {
        assert_eq!(
            super::image_extent([Some((800, 150)), Some((300, 600))]),
            (800, 600)
        );
        assert_eq!(super::image_extent([None, Some((300, 150))]), (300, 150));
        assert_eq!(super::image_extent([Some((300, 150)), None]), (300, 150));
        assert_eq!(super::image_extent([None, None]), (1, 1));
        let extent = (800, 600);
        let scale = super::image_scale(extent, (400., 200.));
        assert_eq!(scale, 1. / 3.);
        assert_eq!((300. * scale, 150. * scale), (100., 50.));
        assert_eq!(super::image_scale(extent, (1600., 1200.)), 2.);
        assert_eq!(super::image_scale(extent, (0., 200.)), 0.);
    }
}
