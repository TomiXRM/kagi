use super::*;

/// Exit-only label body: no actions, focus, IDs, accessibility roles or tooltips.
pub(super) fn ghost_content(row: &RowVisual, staged: bool) -> gpui::Div {
    let mut content = div()
        .h(theme::scaled_px(FILE_ROW_HEIGHT).max(px(20.)))
        .w_full()
        // Reserve the same transparent focus-ring inset as the live RowList
        // row, without retaining a focus handle or a keyboard listener.
        .border_2()
        .border_color(gpui::transparent_black())
        .flex()
        .items_center()
        .pl(theme::scaled_px(8. + row.indent))
        .pr_2()
        .bg(rgb(theme().panel))
        .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
        .line_height(theme::scaled_px(16.))
        .text_color(rgb(theme().text_main));
    if matches!(row.key, FileKey::File(_)) {
        let (badge, color, _) = status_badge(row.change.as_ref(), row.conflicted);
        content = content.child(
            div()
                .w(theme::scaled_px(16.))
                .flex_shrink_0()
                .text_color(rgb(color))
                .child(SharedString::from(badge)),
        );
    }
    content = content
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .child(row.name.clone()),
        )
        .child(diffstat_bar::diffstat_unit_inert(row.stat.as_ref(), None));
    if matches!(row.key, FileKey::File(_)) {
        content = content.child(div().ml_2().text_color(rgb(theme().text_muted)).child(
            if staged {
                Msg::CommitPanelUnstage.t()
            } else {
                Msg::CommitPanelStage.t()
            },
        ));
    }
    #[cfg(feature = "gui-e2e")]
    if let FileKey::File(path) = &row.key {
        content = content.relative().child(super::e2e::measure_inside(format!(
            "cp-{}-ghost:{}",
            if staged { "st" } else { "us" },
            path.display()
        )));
    }
    content
}
