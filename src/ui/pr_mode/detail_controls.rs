//! Keyboard and accessibility contracts for the dense PR detail chrome.

use super::super::button_style::{ButtonRole, KagiButton};
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString, Window};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    Sizable as _,
};
use kagi_domain::github::Mergeable;
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::super::keyboard_nav::{self, Activation, TabList};
use super::super::theme::{self, theme};
use super::{KagiApp, Msg, PrView};

const VIEWS: [PrView; 5] = [
    PrView::Overview,
    PrView::Review,
    PrView::Diff,
    PrView::Commits,
    PrView::Conflicts,
];
const IDS: [&str; 5] = [
    "pr-view-overview",
    "pr-view-review",
    "pr-view-diff",
    "pr-view-commits",
    "pr-view-conflicts",
];
const ICONS: [&str; 5] = [
    "icons/file-text.svg",
    "icons/message-square.svg",
    "icons/git-compare.svg",
    "icons/git-commit.svg",
    "icons/git-merge.svg",
];

pub(super) fn selector(
    app: &KagiApp,
    view: PrView,
    mergeable: Mergeable,
    counts: [usize; 3],
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let mode = app.pr_mode().expect("PR detail owns its mode");
    let count = if mergeable == Mergeable::Conflicting {
        5
    } else {
        4
    };
    let owner = app.active_session();
    let entity = cx.entity().downgrade();
    let tabs = TabList::new(
        &mode.detail_focus,
        keyboard_nav::slot_keys(count),
        (0..count).collect(),
        VIEWS.iter().position(|&candidate| candidate == view),
        Activation::Manual,
        app.root_focus.clone(),
        move |slot, _, _, cx| {
            let _ = entity.update(cx, |app, cx| {
                if app.active_session() == owner {
                    #[cfg(feature = "gui-e2e")]
                    super::super::e2e::pr_detail::record_activation(IDS[slot]);
                    app.pr_mode_show(VIEWS[slot], cx);
                }
            });
        },
        cx,
    );
    mode.detail_focus
        .release_closed(app.root_focus.as_ref(), window, cx);
    let labels = [
        Msg::PrModeOverview.t().to_string(),
        counted(Msg::PrModeReview.t(), counts[0]),
        counted(Msg::PrModeFiles.t(), counts[1]),
        counted(Msg::PrModeCommits.t(), counts[2]),
        Msg::PrModeConflicts.t().to_string(),
    ];
    tabs.list(div().id("pr-detail-view-tabs"))
        .map(|element| {
            #[cfg(feature = "gui-e2e")]
            super::super::e2e::pr_detail::record_control(
                "pr-detail-view-tabs",
                Some(gpui::Role::TabList),
                None,
                None,
                None,
            );
            element
        })
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .flex_shrink_0()
        .px_2()
        .bg(rgb(theme().panel))
        .border_b_1()
        .border_color(rgb(theme().selected))
        .children((0..count).map(|slot| cell(&tabs, slot, &labels[slot], view == VIEWS[slot])))
        .into_any_element()
}

fn counted(label: &str, count: usize) -> String {
    if count == 0 {
        label.to_string()
    } else {
        format!("{label} ({count})")
    }
}

fn cell(tabs: &TabList, slot: usize, label: &str, selected: bool) -> AnyElement {
    let element = div()
        .id(IDS[slot])
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px(keyboard_nav::inset(12.))
        .py(keyboard_nav::inset(4.))
        .min_h(theme::scaled_px(24.))
        .cursor_pointer()
        .text_color(rgb(if selected {
            theme().text_main
        } else {
            theme().text_sub
        }))
        .when(!selected, |element| {
            element.hover(|style| style.text_color(rgb(theme().text_main)))
        })
        .child(super::super::e2e::measure_inside(IDS[slot]))
        .child(
            gpui::svg()
                .path(ICONS[slot])
                .flex_shrink_0()
                .w(theme::scaled_px(13.))
                .h(theme::scaled_px(13.))
                .text_color(rgb(if selected {
                    theme().text_main
                } else {
                    theme().text_sub
                })),
        )
        .child(div().text_xs().child(SharedString::from(label.to_string())))
        .when(selected, |element| {
            element.bg(theme().segment_selected_bg())
        });
    tabs.cell(slot, label, element).into_any_element()
}

pub(super) fn description(
    title: SharedString,
    selected: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let label = Msg::PrModeShowDescription.t();
    super::super::button_style::named_button(
        Button::new("pr-mode-title")
            .ghost()
            .small()
            .flex_1()
            .min_w(px(0.)),
        label,
        Some(selected),
    )
    .on_key_down(keyboard_nav::stop_activation_keys)
    .on_click(on_click)
    .relative()
    .child(super::super::e2e::measure_inside("pr-mode-title"))
    .child(
        div()
            .w_full()
            .min_w(px(0.))
            .truncate()
            .text_sm()
            .text_left()
            .child(title),
    )
    .map(|button| {
        #[cfg(feature = "gui-e2e")]
        {
            super::super::e2e::pr_detail::record_control(
                "pr-mode-title",
                Some(gpui::Role::Button),
                Some(label),
                Some(selected),
                None,
            );
            super::super::e2e::pr_detail::record_button_state(
                "pr-mode-title",
                true,
                false,
                Some(ButtonRole::Neutral),
            );
        }
        button
    })
    .hover_tooltip("pr-mode-title-hover-help", label)
}

pub(super) fn merge(
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    cx: &gpui::App,
) -> AnyElement {
    let label = Msg::PrModeMerge.t();
    super::super::button_style::named_button(
        KagiButton::icon(
            "pr-mode-merge",
            "icons/git-merge.svg",
            label,
            ButtonRole::Secondary,
            cx,
        ),
        label,
        None,
    )
    .small()
    .on_key_down(keyboard_nav::stop_activation_keys)
    .on_click(on_click)
    .map(|button| {
        #[cfg(feature = "gui-e2e")]
        {
            super::super::e2e::pr_detail::record_control(
                "pr-mode-merge",
                Some(gpui::Role::Button),
                Some(label),
                None,
                None,
            );
            super::super::e2e::pr_detail::record_button_state(
                "pr-mode-merge",
                true,
                false,
                Some(ButtonRole::Secondary),
            );
        }
        button
    })
    .map(|button| super::super::e2e::measure_control("pr-mode-merge", button))
}

pub(super) fn github(
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let label = Msg::PrOpenOnGitHub.t();
    super::super::button_style::named_button(
        Button::new("pr-mode-open-gh").link().label(label),
        label,
        None,
    )
    .small()
    .h(theme::scaled_px(24.))
    .min_h(px(20.))
    .px_2()
    .on_key_down(keyboard_nav::stop_activation_keys)
    .on_click(on_click)
    .map(|button| {
        #[cfg(feature = "gui-e2e")]
        {
            super::super::e2e::pr_detail::record_control(
                "pr-mode-open-gh",
                Some(gpui::Role::Link),
                Some(label),
                None,
                None,
            );
            super::super::e2e::pr_detail::record_button_state(
                "pr-mode-open-gh",
                true,
                false,
                Some(ButtonRole::Secondary),
            );
        }
        button
    })
    .map(|button| super::super::e2e::measure_control("pr-mode-open-gh", button))
}
