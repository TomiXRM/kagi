//! Retained review context inside the existing virtualized PR feed (#1258).
use crate::ui::{
    e2e,
    i18n::{pr_threads, Msg},
    theme::{self, theme},
    types::ToastKind,
    KagiApp,
};
use gpui::{div, prelude::*, px, rgb, Context, ListState, ScrollHandle, SharedString};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    scroll::Scrollbar,
    Sizable as _,
};
use kagi_ui_core::tooltip::HoverTooltip as _;
use std::{cell::Cell, rc::Rc};

const PREVIEW_LINES: usize = 12;

pub(super) struct Hunk {
    pub(super) full: SharedString,
    lines: Vec<SharedString>,
    expanded: Cell<bool>,
    scroll: ScrollHandle,
}

impl Hunk {
    pub(super) fn new(source: &str) -> Option<Rc<Self>> {
        if source.trim().is_empty() {
            return None;
        }
        #[cfg(feature = "gui-e2e")]
        PROJECTIONS.with(|count| count.set(count.get() + 1));
        Some(Rc::new(Self {
            full: source.to_owned().into(),
            lines: source
                .lines()
                .map(|line| SharedString::from(line.to_owned()))
                .collect(),
            expanded: Cell::new(false),
            scroll: ScrollHandle::new(),
        }))
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static PROJECTIONS: Cell<usize> = const { Cell::new(0) };
    static ATTRIBUTES: std::cell::RefCell<std::collections::HashMap<usize, (gpui::Role, SharedString, usize)>> = std::cell::RefCell::new(std::collections::HashMap::new());
}
#[cfg(feature = "gui-e2e")]
pub fn projection_builds() -> usize {
    PROJECTIONS.with(Cell::get)
}
#[cfg(feature = "gui-e2e")]
pub fn attributes(id: usize) -> Option<(gpui::Role, SharedString, usize)> {
    ATTRIBUTES.with(|records| records.borrow().get(&id).cloned())
}

fn rows(hunk: &Hunk, id: usize, start: usize) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w_auto()
        .min_w_full()
        .py_1()
        .children(
            hunk.lines
                .iter()
                .enumerate()
                .skip(start)
                .map(|(index, line)| {
                    let (bg, fg) = match line.as_bytes().first() {
                        Some(b'+') => (Some(theme().diff_added_bg), theme().change_added),
                        Some(b'-') => (Some(theme().diff_removed_bg), theme().change_deleted),
                        Some(b'@') => (None, theme().diff_hunk),
                        _ => (None, theme().text_sub),
                    };
                    div()
                        .px_2()
                        .whitespace_nowrap()
                        .font_family(crate::ui::MONO_FONT)
                        .text_xs()
                        .text_color(rgb(fg))
                        .when_some(bg, |el, color| el.bg(rgb(color)))
                        .child(e2e::measure_control(
                            format!("pr-convo-hunk-line-{id}-{index}"),
                            div().child(line.clone()),
                        ))
                }),
        )
}

fn copy_button(hunk: &Hunk, id: usize, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let name = format!("pr-convo-hunk-copy-{id}");
    let full = hunk.full.clone();
    let copy = cx.listener(move |app: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
        cx.stop_propagation();
        #[cfg(feature = "gui-e2e")]
        e2e::pr_detail::record_activation(&format!("pr-convo-hunk-copy-{id}"));
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(full.to_string()));
        let subject = app.toast_subject_for_window(window, cx);
        app.push_toast_for(subject, ToastKind::Info, Msg::PrHunkCopied.t(), cx);
    });
    let button = Button::new(("pr-convo-hunk-copy", id))
        .ghost()
        .xsmall()
        .icon(gpui_component::Icon::empty().path("icons/copy.svg"))
        .size(theme::scaled_px(24.))
        .tab_stop(true)
        .map(|button| crate::ui::button_style::named_button(button, Msg::PrHunkCopy.t(), None))
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(copy)
        .hover_tooltip(("pr-convo-hunk-copy-hover-help", id), Msg::PrHunkCopy.t());
    #[cfg(feature = "gui-e2e")]
    {
        e2e::pr_detail::record_control(
            &name,
            Some(gpui::Role::Button),
            Some(Msg::PrHunkCopy.t()),
            None,
            None,
        );
        e2e::pr_detail::record_button_state(&name, true, false, None);
    }
    e2e::measure_control(name, button).into_any_element()
}

pub(super) fn render(
    hunk: &Rc<Hunk>,
    id: usize,
    item: (ListState, usize),
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let shown = if hunk.expanded.get() {
        hunk.lines.len()
    } else {
        hunk.lines.len().min(PREVIEW_LINES)
    };
    let rows = rows(hunk, id, hunk.lines.len() - shown);
    div()
        .id(("pr-convo-hunk", id))
        .relative()
        .w_full()
        .min_w(px(0.))
        .mb_2()
        .rounded_md()
        .overflow_hidden()
        .border_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().bg_base))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .when(hunk.lines.len() > PREVIEW_LINES, |el| {
                    el.child(disclosure(hunk, id, shown, item, cx))
                })
                .child(div().flex_1())
                .child(copy_button(hunk, id, cx)),
        )
        .child(
            div()
                .relative()
                .w_full()
                .min_w(px(0.))
                .child(e2e::measure_control(
                    format!("pr-convo-hunk-scroll-{id}"),
                    div()
                        .id(("pr-convo-hunk-scroll", id))
                        .w_full()
                        .flex()
                        .overflow_x_scroll()
                        .track_scroll(&hunk.scroll)
                        .map(|mut area| {
                            // Vertical wheel deltas belong only to the outer feed.
                            area.style().restrict_scroll_to_axis = Some(true);
                            area
                        })
                        .child(rows),
                ))
                .child(Scrollbar::horizontal(&hunk.scroll)),
        )
        .into_any_element()
}

fn disclosure(
    hunk: &Rc<Hunk>,
    id: usize,
    shown: usize,
    item: (ListState, usize),
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let label: SharedString = if hunk.expanded.get() {
        pr_threads::collapse_hunk().into()
    } else {
        pr_threads::show_more_lines(hunk.lines.len() - shown).into()
    };
    let target = hunk.clone();
    let button = Button::new(("pr-convo-hunk-disclosure", id))
        .ghost()
        .small()
        // The pinned Button installs Role::Button and aria_label from this label.
        .label(label.clone())
        .on_click(
            cx.listener(move |_: &mut KagiApp, _: &gpui::ClickEvent, _, cx| {
                cx.stop_propagation();
                target.expanded.set(!target.expanded.get());
                if item.1 < item.0.item_count() {
                    item.0.remeasure_items(item.1..item.1 + 1);
                }
                cx.notify();
            }),
        );
    #[cfg(feature = "gui-e2e")]
    ATTRIBUTES.with(|records| {
        records
            .borrow_mut()
            .insert(id, (gpui::Role::Button, label, shown))
    });
    e2e::measure_control(format!("pr-convo-hunk-disclosure-{id}"), button).into_any_element()
}
