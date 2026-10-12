//! Local branch chooser: frozen command/name snapshot, no approval or writes.
use super::{
    commands::{BranchPickerMode, MenuOverlay},
    i18n::Msg,
    theme::{self, theme},
    KagiApp,
};
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, FocusHandle, ListState, Window};
use gpui_component::FocusTrapElement as _;
use std::{cell::Cell, rc::Rc};

pub struct PickerRows {
    pub(super) keyboard: super::list_keyboard::ListKeyboard,
    scroll: ListState,
    first: Cell<bool>,
    trap: FocusHandle,
}
impl std::fmt::Debug for PickerRows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PickerRows").finish_non_exhaustive()
    }
}
impl PickerRows {
    pub(super) fn new(count: usize, cx: &gpui::App) -> Self {
        Self {
            keyboard: Default::default(),
            scroll: ListState::new(count, gpui::ListAlignment::Top, px(0.)),
            first: Cell::new(true),
            trap: cx.focus_handle(),
        }
    }
}
fn title(mode: BranchPickerMode) -> &'static str {
    match mode {
        BranchPickerMode::Checkout => Msg::BranchPickerCheckout.t(),
        BranchPickerMode::Delete => Msg::BranchPickerDelete.t(),
        BranchPickerMode::Rename => Msg::BranchPickerRename.t(),
    }
}
impl KagiApp {
    pub(super) fn render_branch_picker(
        &self,
        mode: BranchPickerMode,
        branches: Rc<[String]>,
        navigation: Rc<PickerRows>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = navigation.keyboard.rows(
            branches
                .iter()
                .enumerate()
                .map(|(at, name)| (name.as_str(), at)),
            &navigation.scroll,
            self.root_focus.as_ref(),
            window,
            cx,
        );
        if navigation.first.replace(false)
            && !navigation
                .keyboard
                .focus_first(&navigation.scroll, window, cx)
        {
            navigation.trap.focus(window, cx);
        }
        let list_rows = rows.clone();
        let count = branches.len();
        let snapshot = navigation.clone();
        let render = cx.processor(move |_app: &mut KagiApp, index: usize, _window, cx| {
            let Some(name) = branches.get(index) else {
                return div().into_any_element();
            };
            picker_row(mode, name, index, count, &rows, snapshot.clone(), cx)
        });
        let cap = theme::scaled_px(420.)
            .min((window.viewport_size().height - theme::scaled_px(48.)).max(px(0.)));
        let height = (theme::scaled_px(36.).ceil() * count.max(1))
            .min((cap - theme::scaled_px(44.)).max(px(0.)));
        let content = if count == 0 {
            div()
                .px_3()
                .py_2()
                .text_color(rgb(theme().text_muted))
                .child(Msg::NoLocalBranches.t())
                .into_any_element()
        } else {
            gpui::list(navigation.scroll.clone(), move |index, window, cx| {
                render(index, window, cx)
            })
            .h(height)
            .w_full()
            .into_any_element()
        };
        let panel = picker_panel(
            mode,
            &list_rows,
            &navigation,
            (height, cap),
            content,
            window,
            cx,
        );
        self.wrap_overlay(panel, cx)
    }
}

fn picker_panel(
    mode: BranchPickerMode,
    rows: &super::keyboard_nav::RowList,
    navigation: &PickerRows,
    (height, cap): (gpui::Pixels, gpui::Pixels),
    content: AnyElement,
    window: &Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    div()
        .id("branch-picker")
        .role(gpui::Role::Dialog)
        .aria_label(title(mode))
        .track_focus(&navigation.trap)
        .occlude()
        .flex()
        .flex_col()
        .w(theme::scaled_px(360.)
            .min((window.viewport_size().width - theme::scaled_px(48.)).max(px(0.))))
        .max_h(cap)
        .overflow_hidden()
        .rounded(theme::scaled_px(8.))
        .border_1()
        .border_color(rgb(theme().elevated_border(theme().selected)))
        .bg(rgb(theme().elevated_surface(theme().panel)))
        .shadow_lg()
        .child(
            div()
                .px_3()
                .py_2()
                .flex_shrink_0()
                .border_b_1()
                .border_color(rgb(theme().selected))
                .text_color(rgb(theme().text_main))
                .child(title(mode)),
        )
        .child(
            rows.list(super::list_a11y::plain_list(
                "branch-picker-list",
                div()
                    .id("branch-picker-scroll")
                    .relative()
                    .w_full()
                    .h(height)
                    .min_h_0()
                    .child(super::e2e::measure_inside("branch-picker-scroll")),
                title(mode),
            ))
            .child(super::render_helpers::with_system_vertical_scrollbar(
                "branch-picker-scrollbar",
                &navigation.scroll,
                content,
                cx,
            )),
        )
        .focus_trap("branch-picker-trap", &navigation.trap)
        .into_any_element()
}
fn picker_row(
    mode: BranchPickerMode,
    name: &str,
    index: usize,
    count: usize,
    rows: &super::keyboard_nav::RowList,
    snapshot: Rc<PickerRows>,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let raw = name.to_owned();
    let name = super::render_helpers::safe_text(name);
    let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
        if !matches!(&this.menu_overlay, Some(MenuOverlay::BranchPicker { rows, .. }) if Rc::ptr_eq(rows, &snapshot)) { return; }
        this.menu_overlay = None;
        match mode {
            BranchPickerMode::Checkout => {
                this.focus_root_for_modal();
                this.open_plan_modal(raw.clone(), cx);
            }
            BranchPickerMode::Delete => {
                this.focus_root_for_modal();
                this.open_delete_branch_modal(raw.clone(), cx);
            }
            BranchPickerMode::Rename => {
                // The input's first render owns focus; a delayed root target steals it.
                this.pending_focus = None;
                this.open_rename_branch_modal(raw.clone());
            }
        }
        cx.notify();
    });
    let label = format!("{}: {}", title(mode), name);
    let tooltip = name.clone();
    let row = div()
        .id(("branch-pick", index))
        .px(super::keyboard_nav::inset(12.))
        .py(super::keyboard_nav::inset(6.))
        .h(theme::scaled_px(36.).ceil())
        .text_sm()
        .text_color(rgb(theme().text_sub))
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(rgb(theme().selected))
                .text_color(rgb(theme().text_main))
        })
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
        })
        .on_click(click)
        .child(div().min_w_0().truncate().child(name));
    let row = rows.row(
        index,
        super::list_a11y::list_item("branch-picker-list", row, index, count, label),
    );
    super::e2e::measure_control(format!("branch-pick-{index}"), row).into_any_element()
}
