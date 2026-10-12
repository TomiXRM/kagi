//! Computed plan-file previews (#1381); display only, within the body scroller.
use super::{
    e2e,
    i18n::Msg,
    theme::{self, theme},
};
use gpui::{div, prelude::*, rgb, SharedString, TextOverflow};
use kagi_domain::status::{ChangeKind, FileStatus};

pub(crate) const PREVIEW_FILE_LIMIT: usize = 25;

pub(crate) fn render(files: &[FileStatus]) -> gpui::AnyElement {
    let heading = Msg::ModalFilesToChange
        .t()
        .replace("{}", &files.len().to_string());
    let rows = files
        .iter()
        .take(PREVIEW_FILE_LIMIT)
        .enumerate()
        .map(|(index, file)| render_row(index, file));
    let remaining = files.len().saturating_sub(PREVIEW_FILE_LIMIT);
    e2e::measure_control(
        "plan-preview-files",
        super::dialog_a11y::apply_group(
            "plan-preview-files",
            div().id("plan-preview-files"),
            heading.clone().into(),
        )
        .flex()
        .flex_col()
        .flex_shrink_0()
        .min_w(gpui::px(0.))
        .text_sm()
        .text_color(rgb(theme().text_main))
        .child(
            div()
                .flex_shrink_0()
                .text_color(rgb(theme().text_label))
                .child(heading),
        )
        .children(rows)
        .when(remaining > 0, |column| {
            let more = Msg::ModalFilesMore
                .t()
                .replace("{}", &remaining.to_string());
            column.child(e2e::measure_control(
                "plan-preview-more",
                super::dialog_a11y::apply_note(
                    "plan-preview-more".into(),
                    div().id("plan-preview-more"),
                    false,
                    &more,
                )
                .text_color(rgb(theme().text_sub))
                .child(more),
            ))
        }),
    )
    .into_any_element()
}

fn render_row(index: usize, file: &FileStatus) -> gpui::AnyElement {
    let (glyph, color) = match &file.change {
        ChangeKind::Added => ("A", theme().change_added),
        ChangeKind::Modified => ("M", theme().change_modified),
        ChangeKind::Deleted => ("D", theme().change_deleted),
        ChangeKind::Renamed { .. } => ("R", theme().change_renamed),
        ChangeKind::TypeChange => ("T", theme().change_typechange),
    };
    let path: SharedString =
        kagi_domain::text_safety::sanitize_control_bytes(&file.path.to_string_lossy()).into();
    let tooltip = path.clone();
    e2e::measure_control(
        format!("plan-preview-file-{index}"),
        div()
            .flex()
            .items_center()
            .gap_1()
            .min_w(gpui::px(0.))
            .flex_shrink_0()
            .child(
                div()
                    .flex_shrink_0()
                    .w(theme::scaled_px(12.))
                    .text_color(rgb(color))
                    .child(glyph),
            )
            .child(
                super::dialog_a11y::apply_note(
                    format!("plan-preview-path-{index}").into(),
                    div().id(("plan-preview-path", index)),
                    false,
                    path.as_ref(),
                )
                .flex_1()
                .min_w(gpui::px(0.))
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                })
                .overflow_hidden()
                .whitespace_nowrap()
                .text_overflow(TextOverflow::TruncateMiddle("…".into()))
                .child(e2e::measure_inside(format!("plan-preview-path-{index}")))
                .child(path),
            ),
    )
    .into_any_element()
}
