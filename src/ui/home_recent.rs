//! Home's recent paths retain their own virtual list and keyboard focus.
use super::{
    i18n::Msg,
    render_helpers::safe_text,
    theme::{self, theme},
    KagiApp,
};
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, Window};
use std::{path::PathBuf, rc::Rc};

pub(super) struct RecentList {
    paths: Rc<[PathBuf]>,
    keys: Rc<[String]>,
    pub(super) keyboard: super::list_keyboard::ListKeyboard,
    scroll: gpui::ListState,
}
impl Default for RecentList {
    fn default() -> Self {
        Self {
            paths: Rc::from([]),
            keys: Rc::from([]),
            keyboard: Default::default(),
            scroll: gpui::ListState::new(2, gpui::ListAlignment::Top, px(0.)),
        }
    }
}

impl KagiApp {
    pub(super) fn render_home_sidebar(
        &mut self,
        width: gpui::Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let recent = super::tabs::recent_repos();
        let navigation = &mut self.home_github.recent;
        if navigation.paths.as_ref() != recent.as_slice() {
            navigation.keys = recent
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            navigation.paths = recent.into();
            navigation.scroll.reset(navigation.paths.len().max(1) + 1);
        }
        let paths = navigation.paths.clone();
        let count = paths.len();
        let state = navigation.scroll.clone();
        let rows = navigation.keyboard.rows(
            navigation
                .keys
                .iter()
                .enumerate()
                .map(|(at, key)| (key.as_str(), at + 1)),
            &state,
            self.root_focus.as_ref(),
            window,
            cx,
        );
        let list_rows = rows.clone();
        let render = cx.processor(move |_app: &mut KagiApp, index: usize, _window, cx| {
            if index == 0 {
                return recent_heading().into_any_element();
            }
            match paths.get(index - 1) {
                Some(path) => {
                    recent_row(index - 1, path.clone(), &rows, count, cx).into_any_element()
                }
                None => div()
                    .px_2()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(Msg::HomeNoRecent.t())
                    .into_any_element(),
            }
        });
        let content = list_rows
            .list(super::list_a11y::plain_list(
                "home-recent-list",
                div()
                    .id("home-recent-scroll")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(super::e2e::measure_inside("home-recent-scroll")),
                Msg::HomeRecent.t(),
            ))
            .child(super::render_helpers::with_system_vertical_scrollbar(
                "home-sidebar-scrollbar",
                &state,
                gpui::list(state.clone(), move |index, window, cx| {
                    render(index, window, cx)
                })
                .flex_1()
                .min_h_0(),
                cx,
            ));
        sidebar_frame(width, content.into_any_element())
    }
}

fn sidebar_frame(width: gpui::Pixels, content: AnyElement) -> AnyElement {
    div()
        .id("home-sidebar")
        .relative()
        .child(super::e2e::measure_inside("home-sidebar-bounds"))
        .flex()
        .flex_col()
        .gap_2()
        .w(width)
        .flex_shrink_0()
        .h_full()
        .p_3()
        .bg(rgb(theme().panel))
        .border_r_1()
        .border_color(rgb(theme().surface))
        .child(content)
        .into_any_element()
}

fn recent_heading() -> impl IntoElement {
    div()
        .id("home-recent-heading")
        .relative()
        .child(super::e2e::measure_inside("home-recent-heading"))
        .px_2()
        .py(theme::scaled_px(8.))
        .mb_2()
        .w_full()
        .min_w(px(0.))
        .text_size(theme::scaled_px(12.))
        .line_height(theme::scaled_px(12.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(theme().text_sub))
        .overflow_x_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .tooltip(|window, cx| {
            gpui_component::tooltip::Tooltip::new(Msg::HomeRecent.t()).build(window, cx)
        })
        .child(
            div()
                .relative()
                .h(theme::scaled_px(12.))
                .child(super::e2e::measure_inside("home-recent-heading-line"))
                .child(Msg::HomeRecent.t()),
        )
}

/// One recently opened repository: its name over its path. Opening it turns
/// Home into its tab (or switches to the tab it already has).
fn recent_row(
    i: usize,
    path: PathBuf,
    rows: &super::keyboard_nav::RowList,
    count: usize,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let name = safe_text(&name);
    let full = safe_text(&path.display().to_string());
    let label = super::i18n::recent_row_name(&name);
    let tooltip = full.clone();
    let open = cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
        if !app.home_in_front() || !super::tabs::recent_repos().contains(&path) {
            return;
        }
        app.open_repository(path.clone(), cx);
        cx.notify();
    });
    let row = div()
        .id(("home-recent", i))
        .aria_description(full.clone())
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
        })
        .flex()
        .flex_col()
        .px(super::keyboard_nav::inset(8.))
        .py(super::keyboard_nav::inset(4.))
        .rounded_md()
        .cursor(gpui::CursorStyle::Arrow)
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(open)
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(name),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .truncate()
                .child(safe_text(&full)),
        );
    let row = rows.row(
        i,
        super::list_a11y::list_item("home-recent-list", row, i, count, label),
    );
    super::e2e::measure_control(format!("home-recent-{i}"), row).into_any_element()
}
