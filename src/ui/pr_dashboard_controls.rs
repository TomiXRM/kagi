//! Named keyboard-operable dashboard header controls.
use super::super::{
    e2e,
    i18n::Msg,
    keyboard_nav,
    theme::{self, theme},
    KagiApp,
};
use gpui::{div, prelude::*, Context, SharedString};

fn bar_button(
    app: &KagiApp,
    id: &'static str,
    slot: usize,
    icon: &'static str,
    label: &'static str,
    click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    cx: &gpui::App,
) -> gpui::Stateful<gpui::Div> {
    #[cfg(feature = "gui-e2e")]
    e2e::pr_detail::record_control(id, Some(gpui::Role::Button), Some(label), None, None);
    keyboard_nav::focusable(
        div()
            .id(id)
            .track_focus(
                &app.ui()
                    .filter_controls
                    .focus
                    .get(slot, cx)
                    .tab_index(0)
                    .tab_stop(true),
            )
            .role(gpui::Role::Button)
            .aria_label(SharedString::from(label)),
    )
    .relative()
    .child(e2e::measure_inside(id))
    .flex()
    .flex_row()
    .items_center()
    .gap_1()
    .flex_shrink_0()
    .px_2()
    .py_px()
    .rounded_sm()
    .border_1()
    .border_color(gpui::rgb(theme().selected))
    .text_xs()
    .text_color(gpui::rgb(theme().text_sub))
    .cursor_pointer()
    .hover(|s| s.bg(gpui::rgb(theme().surface)))
    .on_click(click)
    .child(
        gpui::svg()
            .path(icon)
            .flex_shrink_0()
            .w(theme::scaled_px(12.))
            .h(theme::scaled_px(12.))
            .text_color(gpui::rgb(theme().text_sub)),
    )
    .child(SharedString::from(if slot == 9 {
        format!("← {label}")
    } else {
        label.to_string()
    }))
}

pub(in crate::ui) fn home_button(
    app: &KagiApp,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    let click = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
        this.pr_mode_home(cx);
        if let Some(root) = &this.root_focus {
            window.focus(root, cx);
        }
    });
    bar_button(
        app,
        "pr-mode-home",
        9,
        "icons/git-pull-request.svg",
        Msg::PrAllPrs.t(),
        click,
        cx,
    )
}
pub(in crate::ui) fn refresh_button(
    app: &KagiApp,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    let click =
        cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _, cx| this.pr_mode_refresh(cx));
    bar_button(
        app,
        "pr-mode-refresh",
        10,
        "icons/refresh-cw.svg",
        Msg::PrRefresh.t(),
        click,
        cx,
    )
}

pub(super) fn dashboard_rows(
    app: &KagiApp,
    rows: &[(
        usize,
        kagi_domain::github::PrAttention,
        kagi_domain::github::PrReason,
    )],
    all: &[kagi_domain::github::PullRequest],
    scroll: &gpui::UniformListScrollHandle,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> Option<std::rc::Rc<keyboard_nav::RowList>> {
    let mode = app.pr_mode()?;
    let mut retained = mode.dashboard_row_keys.borrow_mut();
    let changed = retained.len() != rows.len()
        || retained
            .iter()
            .zip(rows)
            .any(|((key, _), (source, _, _))| key.parse::<u64>().ok() != Some(all[*source].number));
    if changed {
        // Reveal a surviving focused PR's new position before reconciling focus.
        if let Some(focused) = mode
            .dashboard_keys
            .borrow()
            .focused(window)
            .and_then(|key| key.parse::<u64>().ok())
        {
            if let Some(index) = rows
                .iter()
                .position(|(source, _, _)| focused == all[*source].number)
            {
                scroll.scroll_to_item(index, gpui::ScrollStrategy::Top);
            }
        }
        *retained = rows
            .iter()
            .enumerate()
            .map(|(index, (source, _, _))| (all[*source].number.to_string(), index))
            .collect();
    }
    Some(std::rc::Rc::new(mode.dashboard_keys.borrow_mut().rows(
        retained.clone(),
        &keyboard_nav::RowScroll::Uniform(scroll.clone(), mode.dashboard_drawn.borrow().clone()),
        app.root_focus.as_ref(),
        window,
        cx,
    )))
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    pub fn dashboard_row_focused_for_e2e(&self, window: &gpui::Window) -> Option<String> {
        self.pr_mode()?
            .dashboard_keys
            .borrow()
            .focused(window)
            .map(str::to_owned)
    }
    pub fn dashboard_keyboard_focused_for_e2e(&self, id: &str, window: &gpui::Window) -> bool {
        if id == "pr-home-list" {
            return self.dashboard_row_focused_for_e2e(window).is_some();
        }
        let ids = [
            "list-filter-state",
            "list-filter-label",
            "list-filter-author",
            "list-filter-draft",
            "list-filter-checks",
            "list-filter-sort",
            "list-filter-direction",
            "list-filter-clear",
            "list-filter-refresh",
            "pr-mode-home",
            "pr-mode-refresh",
        ];
        ids.iter()
            .position(|candidate| *candidate == id)
            .is_some_and(|slot| self.ui().filter_controls.focus.focused(window) == Some(slot))
    }
    pub fn dashboard_filter_menu_for_e2e(&self) -> bool {
        self.filter_menu_visible()
    }
}
