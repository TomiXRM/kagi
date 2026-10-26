//! Existing safe whole-file conflict choices and previews.
use super::*;
// ────────────────────────────────────────────────────────────
// Center pane: per-file choose + Result preview (W30; W32 supersedes)
// ────────────────────────────────────────────────────────────

/// Directory/file choices freeze an owner-bound intent. The parent prepares
/// the exact namespace changes and opens the shared confirmation sheet; no
/// index or working-tree mutation occurs before its explicit confirmation.
fn render_dir_file_center(
    path: &std::path::Path,
    writer_busy: bool,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    use kagi_git::DirFileChoice;

    let keep_dir = {
        let path = path.to_path_buf();
        cx.listener(
            move |view: &mut ConflictView, _e: &gpui::ClickEvent, window, cx| {
                let weak_app = view.app.clone();
                let intent = view.freeze_dir_file_intent(&path, DirFileChoice::KeepDirectory);
                cx.spawn_in(window, async move |_view, acx| {
                    let _ = weak_app.update_in(acx, |app, _window, cx| {
                        app.accept_conflict_intent(intent, cx)
                    });
                })
                .detach();
            },
        )
    };
    let keep_file = {
        let path = path.to_path_buf();
        cx.listener(
            move |view: &mut ConflictView, _e: &gpui::ClickEvent, window, cx| {
                let weak_app = view.app.clone();
                let intent = view.freeze_dir_file_intent(&path, DirFileChoice::KeepFile);
                cx.spawn_in(window, async move |_view, acx| {
                    let _ = weak_app.update_in(acx, |app, _window, cx| {
                        app.accept_conflict_intent(intent, cx)
                    });
                })
                .detach();
            },
        )
    };

    let choose_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_2()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(super::e2e::measure_control(
            "conflict-keep-directory",
            super::toolbar::compact_button(
                "conflict-keep-directory",
                Msg::ConflictKeepDirectory.t(),
                ButtonRole::Secondary,
                cx,
            )
            .on_click(keep_dir)
            .disabled(writer_busy),
        ))
        .child(super::e2e::measure_control(
            "conflict-keep-file",
            super::toolbar::compact_button(
                "conflict-keep-file",
                Msg::ConflictKeepFile.t(),
                ButtonRole::Secondary,
                cx,
            )
            .on_click(keep_file)
            .disabled(writer_busy),
        ));

    div()
        .flex()
        .flex_col()
        .flex_grow(1.)
        .h_full()
        .child(choose_row)
        .child(
            div()
                .p(theme::scaled_px(12.))
                .text_size(theme::scaled_px(13.))
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::ConflictDirFileHint.t())),
        )
        .into_any_element()
}

pub(super) fn render_center(
    mode: &ConflictMode,
    writer_busy: bool,
    raw_preview_scroll: &gpui::ScrollHandle,
    raw_preview: &conflict_binary_view::PreviewState,
    pdfs: Option<&crate::ui::diff_view::pdf::DiffPdfViewers>,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    let Some(idx) = mode.selected_file else {
        return div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .flex_grow(1.)
            .h_full()
            .child(
                div()
                    .text_size(theme::scaled_px(13.))
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::ConflictSelectFile.t())),
            )
            .into_any_element();
    };
    let Some(file) = mode.session.files.get(idx) else {
        return div().flex_grow(1.).h_full().into_any_element();
    };

    let labels = mode.labels();
    let kind = file.kind;
    let path = file.path.clone();

    // Directory/file choices use the same prepared-plan confirmation as deletion.
    if kind == ConflictKind::DirFile {
        return render_dir_file_center(&path, writer_busy, cx);
    }
    if kind.is_raw() {
        return super::conflict_binary_view::render_raw_preview(
            raw_preview,
            mode,
            &path,
            &labels,
            raw_preview_scroll,
            pdfs,
            writer_busy,
            cx,
        );
    }

    use kagi_domain::conflict_family::Resolution;
    let resolutions = mode.buffer.file_resolutions(&path).unwrap_or_default();
    if resolutions.contains(&Resolution::Delete) {
        let delete_label = if resolutions.len() == 1 {
            Msg::ConflictConfirmDeletion
        } else {
            Msg::ConflictUseDeletedSide
        };
        let delete_path = path.clone();
        let mut choices = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .px(theme::scaled_px(12.))
            .py(theme::scaled_px(8.))
            .border_b_1()
            .border_color(rgb(theme().surface))
            .child(super::e2e::measure_control(
                "conflict-delete",
                super::toolbar::compact_button(
                    "conflict-delete",
                    delete_label.t(),
                    ButtonRole::Secondary,
                    cx,
                )
                .disabled(writer_busy)
                .on_click(cx.listener(move |view, _, window, cx| {
                    let intent = view.freeze_delete_intent(&delete_path);
                    let app = view.app.clone();
                    cx.spawn_in(window, async move |_, cx| {
                        let _ =
                            app.update_in(cx, |app, _, cx| app.accept_conflict_intent(intent, cx));
                    })
                    .detach();
                })),
            ));
        let side = resolutions.iter().find_map(|resolution| match resolution {
            Resolution::Current => Some(kagi_git::resolution::ResolutionChoice::Current),
            Resolution::Incoming => Some(kagi_git::resolution::ResolutionChoice::Incoming),
            Resolution::Delete => None,
        });
        if let Some(side) = side {
            let modified_path = path.clone();
            choices = choices.child(super::e2e::measure_control(
                "conflict-use-modified",
                super::toolbar::compact_button(
                    "conflict-use-modified",
                    Msg::ConflictUseModifiedSide.t(),
                    ButtonRole::Secondary,
                    cx,
                )
                .disabled(writer_busy)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.conflict_apply_choice(&modified_path, side, cx);
                    cx.notify();
                })),
            ));
        }
        return div()
            .flex()
            .flex_col()
            .flex_grow(1.)
            .h_full()
            .child(choices)
            .when(resolutions.len() == 1, |pane| {
                pane.child(
                    div()
                        .p(theme::scaled_px(12.))
                        .text_size(theme::scaled_px(13.))
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(Msg::ConflictDeletedBothHint.t())),
                )
            })
            .when(resolutions.len() > 1, |pane| {
                pane.child(render_preview(
                    mode,
                    &path,
                    kind,
                    &labels,
                    raw_preview_scroll,
                    raw_preview,
                    cx,
                ))
            })
            .into_any_element();
    }

    let keep_current_label = format!("{} ({})", Msg::ConflictKeepCurrent.t(), labels.current.name);
    let take_incoming_label = format!(
        "{} ({})",
        Msg::ConflictTakeIncoming.t(),
        labels.incoming.name
    );
    let keep_both_label = Msg::ConflictKeepBoth.t().to_string();

    let p1 = path.clone();
    let keep_current = cx.listener(
        move |view: &mut ConflictView, _e: &gpui::ClickEvent, _w, cx| {
            view.conflict_apply_choice(&p1, kagi_git::resolution::ResolutionChoice::Current, cx);
            cx.notify();
        },
    );
    let p2 = path.clone();
    let take_incoming = cx.listener(
        move |view: &mut ConflictView, _e: &gpui::ClickEvent, _w, cx| {
            view.conflict_apply_choice(&p2, kagi_git::resolution::ResolutionChoice::Incoming, cx);
            cx.notify();
        },
    );
    let p3 = path.clone();
    let keep_both = cx.listener(
        move |view: &mut ConflictView, _e: &gpui::ClickEvent, _w, cx| {
            view.conflict_apply_choice(
                &p3,
                kagi_git::resolution::ResolutionChoice::BothCurrentFirst,
                cx,
            );
            cx.notify();
        },
    );

    let mut choose_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_2()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            choose_button(
                keep_current_label,
                ButtonRole::SideCurrent,
                keep_current,
                cx,
            )
            .disabled(writer_busy),
        )
        .child(
            choose_button(
                take_incoming_label,
                ButtonRole::SideIncoming,
                take_incoming,
                cx,
            )
            .disabled(writer_busy),
        );

    if kind == ConflictKind::Content {
        choose_row = choose_row.child(choose_button(
            keep_both_label,
            ButtonRole::Neutral,
            keep_both,
            cx,
        ));
    }

    let preview = render_preview(mode, &path);

    div()
        .flex()
        .flex_col()
        .flex_grow(1.)
        .h_full()
        .child(choose_row)
        .child(preview)
        .into_any_element()
}

fn choose_button<H>(label: String, role: ButtonRole, handler: H, cx: &gpui::App) -> Button
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    KagiButton::styled(
        SharedString::from(format!("conflict-choose-{}", label)),
        SharedString::from(label),
        role,
        cx,
    )
    .small()
    .on_click(handler)
}

/// Read-only plain-text Result preview for whole-file choices.
fn render_preview(mode: &ConflictMode, path: &std::path::Path) -> gpui::AnyElement {
    let body: gpui::AnyElement = if let Some(text) = mode.buffer.resolved_text(path) {
        let mut col = div().flex().flex_col();
        for line in text.split('\n') {
            col = col.child(
                div()
                    .text_size(theme::scaled_px(12.))
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(line.to_string())),
            );
        }
        col.into_any_element()
    } else {
        div()
            .text_size(theme::scaled_px(12.))
            .text_color(rgb(theme().text_sub))
            .child(SharedString::from(Msg::ConflictPreviewHint.t()))
            .into_any_element()
    };

    div()
        .id("conflict-preview")
        .flex()
        .flex_col()
        .flex_grow(1.)
        .w_full()
        .overflow_y_scroll()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .child(
            div()
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(theme().text_label))
                .pb(px(4.))
                .child(SharedString::from(Msg::ConflictResultPreview.t())),
        )
        .child(body)
        .into_any_element()
}
