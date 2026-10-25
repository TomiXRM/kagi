//! REFS-first confirmation for Operation Log ref restore. The executable plan
//! remains untouched; this card only presents its once-decoded ref rows.

use crate::ui::button_style::{modal_button, ModalButtonKind};
use crate::ui::dialog_a11y::{apply_dialog, dialog_a11y, DialogHandler};
use crate::ui::modal_command::{equivalent_command, plan_ready};
use crate::ui::modal_copy::modal_copy_button;
use crate::ui::modal_renderers::modal_overlay;
use crate::ui::modal_shell::{modal_card, MODAL_W_LG};
use crate::ui::modals::oplog_restore::OplogRestoreModal;
use crate::ui::plan_card_rows::{note_presentation, render_note_row};
use crate::ui::theme::{self, theme};
use crate::ui::KagiApp;
use crate::ui::MONO_FONT;
use gpui::{div, prelude::*, rgb, Context, SharedString};
use kagi_domain::head::Head;
use kagi_domain::plan_note::{OplogRestoreNote, OplogRestoreTitle, PlanNote, PlanTitle};
use kagi_domain::ref_restore::RefRestore;
use kagi_ui_core::i18n::{self, oplog_panel::OplogPanelMsg, plan_note_text, Msg};
use kagi_ui_core::tooltip::HoverTooltip as _;
use std::rc::Rc;

fn text(msg: OplogPanelMsg) -> &'static str {
    Msg::OplogPanel(msg).t()
}

fn ref_short(oid: Option<&str>) -> &str {
    oid.map(|s| s.get(..8).unwrap_or(s)).unwrap_or("–")
}

fn ref_name(refname: &str) -> &str {
    refname
        .strip_prefix("refs/heads/")
        .or_else(|| refname.strip_prefix("refs/tags/"))
        .unwrap_or(refname)
}

fn chip(label: impl Into<SharedString>, color: u32) -> gpui::AnyElement {
    let (bg, border, foreground) = theme::badge_style(color);
    div()
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(gpui::rgba(border))
        .bg(gpui::rgba(bg))
        .text_color(rgb(foreground))
        .text_xs()
        .child(label.into())
        .into_any_element()
}
fn delete_chip(label: impl Into<SharedString>) -> gpui::AnyElement {
    let color = theme().color_blocker;
    let (bg, _, _) = theme::badge_style(color);
    div()
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(gpui::rgba((color << 8) | 0x99))
        .bg(gpui::rgba(bg))
        .text_color(rgb(color))
        .text_xs()
        .child(label.into())
        .into_any_element()
}

fn ref_row(row: &RefRestore, head: Option<&str>, index: usize) -> gpui::Stateful<gpui::Div> {
    let name = ref_name(&row.refname);
    let is_head = row.refname.strip_prefix("refs/heads/") == head;
    let color = if is_head {
        theme().color_head
    } else if row.refname.starts_with("refs/tags/") {
        theme().color_tag
    } else {
        theme().color_branch
    };
    let ref_label = if is_head {
        format!("✓ {name}")
    } else {
        format!(
            "{} {name}",
            if row.refname.starts_with("refs/tags/") {
                "◆"
            } else {
                "⑂"
            }
        )
    };
    let tooltip: SharedString = row.refname.clone().into();
    let (bg, border, foreground) = theme::badge_style(color);
    let name_id: SharedString = format!("restore-ref-name-{index}").into();
    let name_chip =
        crate::ui::dialog_a11y::apply_note(name_id.clone(), div().id(name_id), false, &row.refname)
            .relative()
            .max_w(gpui::relative(0.45))
            .min_w(gpui::px(0.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .px_1()
            .rounded_sm()
            .border_1()
            .border_color(gpui::rgba(border))
            .bg(gpui::rgba(bg))
            .text_color(rgb(foreground))
            .text_xs()
            .child(SharedString::from(ref_label))
            .child(crate::ui::e2e::measure_inside(format!(
                "restore-ref-name-{index}"
            )))
            .hover_tooltip_builder(("restore-ref-name-help", index), move |window, cx| {
                gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            });
    div()
        .id(SharedString::from(format!("restore-ref-{index}")))
        .relative()
        .w_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .h(theme::scaled_px(31.))
        .min_h(theme::scaled_px(31.))
        .child(name_chip)
        .child(div().flex_1())
        .child(
            div()
                .id(format!("restore-ref-expected-{index}"))
                .relative()
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(
                    ref_short(row.expect.as_deref()).to_string(),
                ))
                .child(crate::ui::e2e::measure_inside(format!(
                    "restore-ref-expected-{index}"
                ))),
        )
        .child(div().text_color(rgb(color)).child("→"))
        .when_some(row.restore_to.as_deref(), |el, to| {
            el.child(
                div()
                    .id(format!("restore-ref-destination-{index}"))
                    .relative()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(ref_short(Some(to)).to_string()))
                    .child(crate::ui::e2e::measure_inside(format!(
                        "restore-ref-destination-{index}"
                    ))),
            )
        })
        .when(row.restore_to.is_none(), |el| {
            el.child(delete_chip(format!(
                "× {}",
                text(OplogPanelMsg::RestoreDelete)
            )))
        })
        .child(crate::ui::e2e::measure_inside(format!(
            "restore-ref-{index}"
        )))
}

fn title_chip(title: &PlanTitle) -> String {
    match title {
        PlanTitle::OplogRestore(
            OplogRestoreTitle::Revert { id, op } | OplogRestoreTitle::RestoreTo { id, op },
        ) => format!("#{id} · {op}"),
        _ => unreachable!("restore modal only contains restore plans"),
    }
}

fn checked_out_warning_row(
    index: usize,
    reason: &str,
    path: &str,
    glyph: &'static str,
    color: u32,
) -> gpui::AnyElement {
    let id: SharedString = format!("restore-warning-{index}").into();
    let (bg, border, _) = theme::badge_style(color);
    let full = format!("{reason} · {path}");
    let path_text = SharedString::from(path.to_owned());
    let tooltip = path_text.clone();
    crate::ui::dialog_a11y::apply_note(id.clone(), div().id(id), false, &full)
        .relative()
        .flex()
        .items_start()
        .gap_2()
        .rounded_md()
        .bg(gpui::rgba(bg))
        .border_1()
        .border_color(gpui::rgba(border))
        .px_2()
        .py(theme::scaled_px(4.))
        .when(!glyph.is_empty(), |row| {
            row.child(div().flex_shrink_0().text_color(rgb(color)).child(glyph))
        })
        .child(
            div()
                .flex_1()
                .min_w(gpui::px(0.))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(reason.to_owned())),
        )
        .child(
            div()
                .id(format!("restore-warning-path-{index}"))
                .relative()
                .max_w(gpui::relative(0.45))
                .min_w(gpui::px(0.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(MONO_FONT)
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(path_text)
                .child(crate::ui::e2e::measure_inside(format!(
                    "restore-warning-path-{index}"
                )))
                .hover_tooltip_builder(("restore-warning-path-help", index), move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                }),
        )
        .child(crate::ui::e2e::measure_inside(format!(
            "restore-warning-{index}"
        )))
        .into_any_element()
}

fn ready(modal: &OplogRestoreModal) -> bool {
    plan_ready(&modal.plan) && !modal.restores.is_empty()
}

fn copy_text(modal: &OplogRestoreModal) -> String {
    let kind = title_chip(&modal.plan.title);
    let mut out = format!(
        "{} · {kind}\n{}\n",
        modal.i18n_op().t(),
        text(OplogPanelMsg::RestoreRefs)
    );
    for row in modal.restores.iter() {
        out.push_str(&format!(
            "{}: {} → {}\n",
            row.refname,
            row.expect.as_deref().unwrap_or("–"),
            row.restore_to.as_deref().unwrap_or("–")
        ));
    }
    for warning in &modal.plan.warnings {
        match warning {
            PlanNote::OplogRestore(OplogRestoreNote::Moves { .. } | OplogRestoreNote::RefsOnly) => {
                continue
            }
            PlanNote::OplogRestore(OplogRestoreNote::MovesCheckedOutBranch { branch, path }) => {
                out.push_str(&format!(
                    "{} · {path}\n",
                    i18n::oplog_panel::restore_checked_out(branch, false)
                ));
            }
            PlanNote::OplogRestore(OplogRestoreNote::CheckedOutDirty { branch, path }) => {
                out.push_str(&format!(
                    "{} · {path}\n",
                    i18n::oplog_panel::restore_checked_out(branch, true)
                ));
            }
            _ => out.push_str(&format!("{}\n", plan_note_text(warning))),
        }
    }
    for blocker in &modal.plan.blockers {
        out.push_str(&format!("{}\n", plan_note_text(blocker)));
    }
    if modal
        .plan
        .warnings
        .iter()
        .any(|w| matches!(w, PlanNote::OplogRestore(OplogRestoreNote::RefsOnly)))
    {
        for item in unchanged_items() {
            out.push_str(&format!("✓ {}\n", text(item)));
        }
    }
    if let Some(preview) = modal.preview.as_deref() {
        out.push_str(&super::oplog_restore_preview::clipboard_text(preview));
        out.push('\n');
    }
    if let Some(command) = equivalent_command(&modal.plan) {
        out.push_str(command);
    }
    out
}

fn unchanged_items() -> [OplogPanelMsg; 6] {
    [
        OplogPanelMsg::RestoreWorkingTree,
        OplogPanelMsg::RestoreIndex,
        OplogPanelMsg::RestoreUntracked,
        OplogPanelMsg::RestoreStash,
        OplogPanelMsg::RestoreRemotes,
        OplogPanelMsg::RestoreExternalTags,
    ]
}

pub(crate) fn render(
    modal: OplogRestoreModal,
    overrides: &crate::ui::modal_shell::ModalSections,
    window: &mut gpui::Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let op_chip = title_chip(&modal.plan.title);
    let label = modal.confirm_label();
    let blocked = !ready(&modal);
    let title = modal.i18n_op().t();
    let cancel_listener = cx.listener(|app, _: &(), window, cx| {
        app.cancel_oplog_restore_modal();
        if let Some(fh) = app.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let confirm_listener = cx.listener(|app, _: &(), window, cx| {
        app.start_oplog_restore(cx);
        if let Some(fh) = app.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let cancel: DialogHandler = Rc::new(move |w, a| cancel_listener(&(), w, a));
    let confirm: DialogHandler = Rc::new(move |w, a| confirm_listener(&(), w, a));
    let spec = dialog_a11y(
        title,
        (!blocked).then_some(label.as_str()),
        true,
        modal.confirm_stage(),
    );
    let card = apply_dialog(
        "plan-card",
        modal_card(MODAL_W_LG).id("plan-card"),
        spec,
        (!blocked).then(|| confirm.clone()),
        cancel.clone(),
    );
    let header = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .child(
            gpui::svg()
                .path("icons/undo-2.svg")
                .w(theme::scaled_px(18.))
                .h(theme::scaled_px(18.))
                .text_color(rgb(theme().color_blocker)),
        )
        .child(
            div()
                .text_lg()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(title.to_string())),
        )
        .child(chip(op_chip, theme().text_muted))
        .child(div().flex_1())
        .child(modal_copy_button(
            "plan-card-copy",
            Msg::ModalCopyAll.t(),
            copy_text(&modal),
            cx,
        ));
    let head = match &modal.plan.head_at_plan {
        Head::Attached { branch, .. } => Some(branch.as_str()),
        _ => None,
    };
    let refs_h = theme::scaled_px(if crate::ui::modal_shell::modal_compact() {
        3. * 31.
    } else {
        6. * 31.
    });
    let inspection = modal.inspection.clone();
    let ref_rows = inspection.refs.rows(
        modal
            .restores
            .iter()
            .enumerate()
            .map(|(ix, row)| (row.refname.as_str(), ix)),
        None,
        window,
        cx,
    );
    let render_rows = ref_rows.clone();
    let snapshot = modal.clone();
    let head = head.map(str::to_owned);
    let drawn = inspection.refs.drawn.clone();
    let content = gpui::uniform_list(
        "restore-ref-rows",
        modal.restores.len(),
        move |range, _window, _cx| {
            *drawn.borrow_mut() = range.clone();
            let render_rows = Rc::new(render_rows.as_drawn(&range));
            range
                .map(|index| {
                    let row = &snapshot.restores[index];
                    let focus = render_rows.clone();
                    render_rows
                        .row(
                            index,
                            crate::ui::list_a11y::list_item(
                                "restore-refs",
                                ref_row(row, head.as_deref(), index),
                                index,
                                snapshot.restores.len(),
                                crate::ui::render_helpers::safe_text(&format!(
                                    "{}: {} → {}",
                                    row.refname,
                                    ref_short(row.expect.as_deref()),
                                    row.restore_to
                                        .as_deref()
                                        .unwrap_or(text(OplogPanelMsg::RestoreDelete)),
                                ))
                                .to_string(),
                            ),
                        )
                        .on_click(move |_, window, cx| focus.focus_row(index, window, cx))
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        },
    )
    .track_scroll(&inspection.refs.scroll)
    .map(|mut list| {
        list.style().restrict_scroll_to_axis = Some(true);
        list
    })
    .h(theme::scaled_px(modal.restores.len().max(1) as f32 * 31.).min(refs_h))
    .w_full();
    let refs = ref_rows
        .list(crate::ui::list_a11y::plain_list(
            "restore-refs",
            div()
                .id("restore-refs")
                .relative()
                .h(theme::scaled_px(modal.restores.len().max(1) as f32 * 31.).min(refs_h))
                .flex_shrink_0()
                .on_scroll_wheel(inspection.refs.wheel_handler())
                .child(inspection.refs.wheel_capture())
                .child(crate::ui::e2e::measure_inside("restore-refs")),
            text(OplogPanelMsg::RestoreRefs),
        ))
        .child(crate::ui::render_helpers::with_system_vertical_scrollbar(
            "restore-refs-scrollbar",
            &inspection.refs.scroll,
            content,
            cx,
        ));
    let mut body = inspection.body(title);
    if !modal.plan.blockers.is_empty() {
        body = body.child(
            div()
                .id("restore-blockers")
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap_1()
                .children(
                    modal
                        .plan
                        .blockers
                        .iter()
                        .enumerate()
                        .map(|(index, blocker)| {
                            crate::ui::e2e::measure_control(
                                format!("restore-blocker-{index}"),
                                render_note_row(
                                    format!("plan-blocker-{index}").into(),
                                    true,
                                    "✗",
                                    theme().color_blocker,
                                    &plan_note_text(blocker),
                                    true,
                                ),
                            )
                        }),
                ),
        );
    }
    body = body
        .child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(text(OplogPanelMsg::RestoreRefs)),
        )
        .child(refs);
    let mut warnings = Vec::new();
    for (index, warning) in modal.plan.warnings.iter().enumerate() {
        let (glyph, color) = note_presentation(warning.kind(), &theme());
        match warning {
            PlanNote::OplogRestore(OplogRestoreNote::Moves { .. } | OplogRestoreNote::RefsOnly) => {
                continue
            }
            PlanNote::OplogRestore(OplogRestoreNote::MovesCheckedOutBranch { branch, path }) => {
                warnings.push(checked_out_warning_row(
                    index,
                    &i18n::oplog_panel::restore_checked_out(branch, false),
                    path,
                    glyph,
                    color,
                ));
            }
            PlanNote::OplogRestore(OplogRestoreNote::CheckedOutDirty { branch, path }) => {
                warnings.push(checked_out_warning_row(
                    index,
                    &i18n::oplog_panel::restore_checked_out(branch, true),
                    path,
                    glyph,
                    color,
                ));
            }
            _ => warnings.push(render_note_row(
                format!("restore-warning-{index}").into(),
                false,
                glyph,
                color,
                &plan_note_text(warning),
                true,
            )),
        }
    }
    if !warnings.is_empty() {
        body = body.child(
            div()
                .id("restore-warnings")
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap_1()
                .children(warnings),
        );
    }
    if modal
        .plan
        .warnings
        .iter()
        .any(|w| matches!(w, PlanNote::OplogRestore(OplogRestoreNote::RefsOnly)))
    {
        body = body.child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(text(OplogPanelMsg::RestoreUnchanged)),
                )
                .children(unchanged_items().into_iter().map(|item| {
                    chip(
                        SharedString::from(format!("✓ {}", text(item))),
                        theme().text_muted,
                    )
                })),
        );
    }
    if let Some(preview) = modal.preview.clone() {
        body = body.child(
            div()
                .flex_shrink_0()
                .child(super::oplog_restore_preview::render(
                    preview,
                    &inspection.preview,
                    window,
                    cx,
                )),
        );
    }
    if let Some(cmd) = equivalent_command(&modal.plan) {
        body = body.child(div().min_h(gpui::px(0.)).overflow_hidden().child(
            crate::ui::modal_command::render_equivalent_command(
                cmd,
                Some(modal.restores.len()),
                overrides,
                cx,
            ),
        ));
    }
    if let Some(error) = &modal.error {
        body = body.child(
            div()
                .text_color(rgb(theme().color_blocker))
                .child(error.clone()),
        );
    }
    inspection
        .refs
        .reveal_parent(&inspection.body_scroll, window);
    inspection
        .preview
        .reveal_parent(&inspection.body_scroll, window);
    let body = crate::ui::render_helpers::with_system_vertical_scrollbar(
        "restore-body-scrollbar",
        &inspection.body_scroll,
        body,
        cx,
    )
    .child(crate::ui::e2e::measure_inside("restore-body"));
    let mut buttons = crate::ui::button_style::modal_button_row()
        .flex_shrink_0()
        .child(crate::ui::e2e::measure_control(
            "plan-cancel",
            crate::ui::button_style::modal_button(
                "plan-cancel",
                Msg::PlanCancel.t(),
                crate::ui::button_style::ModalButtonKind::Cancel,
                None,
                move |_, w, a| cancel(w, a),
                cx,
            ),
        ));
    if !blocked {
        let button = modal_button(
            "plan-confirm",
            SharedString::from(label),
            if modal.confirm_armed {
                ModalButtonKind::Destructive
            } else {
                ModalButtonKind::Primary
            },
            None,
            move |_, w, a| confirm(w, a),
            cx,
        );
        buttons = buttons.child(crate::ui::e2e::measure_control("plan-confirm", button));
    }
    #[cfg(feature = "gui-e2e")]
    let buttons = buttons
        .relative()
        .child(crate::ui::modal_shell::modal_probe("modal-footer"));
    modal_overlay(card.child(header).child(body).child(buttons)).into_any_element()
}
