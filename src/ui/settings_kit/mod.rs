//! Kagi-owned settings components (experiment `experiments/settings-skill`,
//! #1799 / #1800 / #1802).
//!
//! One owner per settings control kind: a page composes these parts and never
//! re-implements a row, group, switch, segmented control, pop-up value,
//! stepper, navigation row, icon tile, sidebar item or capsule button. Every
//! colour comes from the active [`Theme`](super::theme::Theme), except the
//! fixed category [`Hue`]s of symbol tiles; every size goes through [`pt`],
//! so UI zoom scales the whole page.
//!
//! [`Forced`] draws an interaction state without input. A window that is not
//! key never receives hover or focus (#1799), so captures of those states are
//! drawn, not observed; real pointer hover still uses GPUI `.hover()`.
//!
//! The production Settings window (`settings_view.rs`) does not use this kit
//! yet; importing it there replaces `section_header` / `settings_group` /
//! `setting_row` (see `experiments/settings-skill/import.md`).
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, prelude::*, px, rgb, AnyElement, App, ClickEvent, Div, ElementId, FontWeight, Hsla,
    Pixels, Role, SharedString, Stateful, Window,
};
use gpui_component::{Disableable as _, Icon, IconName, Sizable as _, Size};
use std::rc::Rc;

use super::theme::{self, theme};

/// An interaction state drawn without input, for gallery captures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Forced {
    #[default]
    Rest,
    Hover,
    Pressed,
    Focus,
    Disabled,
}

impl Forced {
    pub const ALL: [Forced; 5] = [
        Forced::Rest,
        Forced::Hover,
        Forced::Pressed,
        Forced::Focus,
        Forced::Disabled,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Forced::Rest => "rest",
            Forced::Hover => "hover",
            Forced::Pressed => "pressed",
            Forced::Focus => "focus",
            Forced::Disabled => "disabled",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.name() == name)
    }
}

/// Design points: `pt(13.)` is 13 logical px at 100% zoom, scaled with zoom.
pub fn pt(n: f32) -> Pixels {
    theme::scaled_px(n * 16. / theme::BASE_REM_PX)
}

/// Fixed geometry of the kit, in design points (scaled through [`pt`]).
pub mod metrics {
    pub const SIDEBAR_WIDTH: f32 = 212.;
    pub const SIDEBAR_ROW: f32 = 32.;
    pub const TILE: f32 = 20.;
    pub const TILE_LARGE: f32 = 32.;
    pub const ROW: f32 = 40.;
    pub const ROW_WITH_SUBTITLE: f32 = 52.;
    pub const GROUP_RADIUS: f32 = 10.;
    pub const GROUP_PADDING_X: f32 = 12.;
    /// Every trailing control (pop-up, segmented, stepper buttons, capsule,
    /// checkbox, section action) is this tall, so controls side by side in a
    /// row share one height and one baseline. Page-local controls use it too.
    pub const CONTROL_HEIGHT: f32 = 24.;
    pub const CONTROL_RADIUS: f32 = 7.;
    pub const BODY: f32 = 13.;
    pub const SECONDARY: f32 = 11.5;
    pub const TITLE: f32 = 15.;
    /// Focus and selection rings stay 2 px at every zoom (`keyboard_nav::RING`).
    pub const RING: f32 = 2.;
}
use metrics::*;

fn hsla(color: u32) -> Hsla {
    rgb(color).into()
}

fn mix(base: u32, over: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let a = ((base >> shift) & 0xff) as f32;
        let b = ((over >> shift) & 0xff) as f32;
        ((a + (b - a) * amount).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// Colour roles of the kit, all derived from the active theme.
pub mod ink {
    use super::{hsla, mix, theme};
    use gpui::Hsla;

    pub fn window() -> u32 {
        let t = theme();
        if t.dark {
            t.bg_base
        } else {
            t.panel
        }
    }
    pub fn sidebar() -> u32 {
        let t = theme();
        mix(window(), t.surface, if t.dark { 0.22 } else { 0.35 })
    }
    pub fn card() -> u32 {
        let t = theme();
        if t.dark {
            mix(t.bg_base, t.surface, 0.42)
        } else {
            t.bg_base
        }
    }
    pub fn hairline() -> u32 {
        let t = theme();
        mix(card(), t.text_sub, if t.dark { 0.18 } else { 0.22 })
    }
    pub fn control() -> u32 {
        mix(card(), theme().text_sub, 0.16)
    }
    pub fn control_hover() -> u32 {
        mix(card(), theme().text_sub, 0.26)
    }
    pub fn control_pressed() -> u32 {
        mix(card(), theme().text_sub, 0.36)
    }
    pub fn selected() -> Hsla {
        theme().segment_selected_bg()
    }
    pub fn text() -> Hsla {
        hsla(theme().text_main)
    }
    pub fn secondary() -> Hsla {
        hsla(theme().text_sub)
    }
    pub fn accent() -> Hsla {
        hsla(theme().ui_accent())
    }
    pub fn accent_text() -> Hsla {
        hsla(theme().accent_text_on(card()))
    }
    pub fn disabled_opacity() -> f32 {
        0.45
    }
}

/// A 2 px accent ring that does not change the element's outer size.
pub fn ring<E: Styled>(element: E, on: bool, radius: f32) -> E {
    if on {
        element
            .border(px(RING))
            .border_color(ink::accent())
            .rounded(pt(radius))
    } else {
        element
            .border(px(RING))
            .border_color(gpui::transparent_black())
            .rounded(pt(radius))
    }
}

fn interactive_fill(el: Stateful<Div>, forced: Forced, rest: Option<u32>) -> Stateful<Div> {
    let fill = match forced {
        Forced::Hover => Some(ink::control_hover()),
        Forced::Pressed => Some(ink::control_pressed()),
        _ => rest,
    };
    el.when_some(fill, |el, fill| el.bg(rgb(fill)))
        .when(forced != Forced::Disabled, |el| {
            el.hover(|s| s.bg(rgb(ink::control_hover())))
                .active(|s| s.bg(rgb(ink::control_pressed())))
        })
        .when(forced == Forced::Disabled, |el| {
            el.opacity(ink::disabled_opacity())
        })
}

// ────────────────────────────────────────────────────────────
// Window scaffold
// ────────────────────────────────────────────────────────────

/// The whole settings window: inset sidebar on the leading side, detail pane.
pub fn settings_window(sidebar: impl IntoElement, detail: impl IntoElement) -> Div {
    div()
        .size_full()
        .flex()
        .flex_row()
        .bg(rgb(ink::window()))
        .text_color(ink::text())
        .text_size(pt(BODY))
        .font(theme::ui_font())
        .child(sidebar)
        .child(detail)
}

/// Inset sidebar panel. Leaves room for the traffic lights at the top.
pub fn sidebar(children: impl IntoIterator<Item = AnyElement>) -> Stateful<Div> {
    div()
        .id("settings-kit-sidebar")
        .w(pt(SIDEBAR_WIDTH))
        .flex_shrink_0()
        .m(pt(8.))
        .pt(pt(40.))
        .px(pt(8.))
        .flex()
        .flex_col()
        .gap(pt(2.))
        .rounded(pt(12.))
        .bg(rgb(ink::sidebar()))
        .border_1()
        .border_color(rgb(ink::hairline()))
        .role(Role::List)
        .children(children)
}

/// Vertical space between sidebar item clusters.
pub fn sidebar_gap() -> AnyElement {
    div().h(pt(10.)).into_any_element()
}

/// Search field look for the sidebar (placeholder only; no text editing).
pub fn search_field(
    id: impl Into<ElementId>,
    placeholder: impl Into<SharedString>,
) -> Stateful<Div> {
    let placeholder = placeholder.into();
    div()
        .id(id)
        .h(pt(28.))
        .mb(pt(8.))
        .px(pt(10.))
        .flex()
        .items_center()
        .gap(pt(6.))
        .rounded(pt(14.))
        .bg(rgb(ink::control()))
        .text_color(ink::secondary())
        .role(Role::SearchInput)
        .aria_label(placeholder.clone())
        .child(Icon::new(IconName::Search).with_size(Size::Size(pt(13.))))
        .child(placeholder)
}

/// Fixed category colours for symbol tiles — the macOS system palette, so a
/// category keeps its colour in every theme the way System Settings does
/// (owner ruling 2026-10-27: accent-only tiles read as monotonous). Use one
/// hue per kind of destination; status rows keep the theme's status roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hue {
    Blue,
    Indigo,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Gray,
}

impl Hue {
    /// Apple system colour (light, dark).
    pub fn rgb(self, dark: bool) -> u32 {
        let (light, dark_rgb) = match self {
            Hue::Blue => (0x007aff, 0x0a84ff),
            Hue::Indigo => (0x5856d6, 0x5e5ce6),
            Hue::Purple => (0xaf52de, 0xbf5af2),
            Hue::Pink => (0xff2d55, 0xff375f),
            Hue::Red => (0xff3b30, 0xff453a),
            Hue::Orange => (0xff9500, 0xff9f0a),
            Hue::Yellow => (0xffcc00, 0xffd60a),
            Hue::Green => (0x34c759, 0x30d158),
            Hue::Teal => (0x30b0c7, 0x40c8e0),
            Hue::Gray => (0x8e8e93, 0x6e6e73),
        };
        if dark {
            dark_rgb
        } else {
            light
        }
    }
}

/// Background of an [`icon_tile`]: a theme role, or a fixed category hue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    Accent,
    Neutral,
    Success,
    Warning,
    Danger,
    Branch,
    Remote,
    Tag,
    Purple,
    Hue(Hue),
}

impl Tint {
    pub fn color(self) -> u32 {
        let t = theme();
        match self {
            Tint::Accent => t.ui_accent(),
            Tint::Neutral => mix(t.text_sub, t.surface, 0.35),
            Tint::Success => t.color_success,
            Tint::Warning => t.color_warning,
            Tint::Danger => t.color_blocker,
            Tint::Branch => t.color_branch,
            Tint::Remote => t.color_remote,
            Tint::Tag => t.color_tag,
            Tint::Purple => t.accent,
            Tint::Hue(hue) => hue.rgb(t.dark),
        }
    }
}

/// A rounded-square symbol tile (sidebar items, rows, cards). Category hues
/// get the soft top-lit gradient of macOS settings icons; theme roles stay
/// flat so status reads as status.
pub fn icon_tile(icon: IconName, tint: Tint, size: f32) -> Div {
    let fill = tint.color();
    let tile = div()
        .size(pt(size))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(pt(size * 0.25))
        .text_color(hsla(theme::on_accent_foreground(fill)))
        .child(Icon::new(icon).with_size(Size::Size(pt(size * 0.62))));
    match tint {
        Tint::Hue(_) => tile.bg(gpui::linear_gradient(
            180.,
            gpui::linear_color_stop(hsla(mix(fill, 0xffffff, 0.22)), 0.),
            gpui::linear_color_stop(hsla(fill), 1.),
        )),
        _ => tile.bg(rgb(fill)),
    }
}

/// One sidebar destination: tile, label, optional status line.
pub fn sidebar_item(
    id: impl Into<ElementId>,
    tile: Div,
    label: impl Into<SharedString>,
    subtitle: Option<SharedString>,
    selected: bool,
    forced: Forced,
) -> Stateful<Div> {
    let label = label.into();
    let el = div()
        .id(id)
        .min_h(pt(SIDEBAR_ROW))
        .px(pt(8.) - px(RING))
        .py(pt(4.) - px(RING))
        .flex()
        .items_center()
        .gap(pt(9.))
        .role(Role::Tab)
        .aria_label(label.clone())
        .aria_selected(selected)
        .when(selected, |el| el.bg(ink::selected()))
        .child(tile)
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .when(selected, |el| el.font_weight(FontWeight::SEMIBOLD))
                        .child(label),
                )
                .when_some(subtitle, |el, subtitle| {
                    el.child(
                        div()
                            .text_size(pt(SECONDARY))
                            .text_color(ink::secondary())
                            .child(subtitle),
                    )
                }),
        );
    let el = ring(el, forced == Forced::Focus, 8.);
    if selected {
        el.when(forced == Forced::Disabled, |el| {
            el.opacity(ink::disabled_opacity())
        })
    } else {
        interactive_fill(el, forced, None)
    }
}

/// Detail pane: page title, then a scrolling column of sections.
pub fn detail_page(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    sections: impl IntoIterator<Item = AnyElement>,
) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .h(pt(52.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .px(pt(14.))
                .text_size(pt(TITLE))
                .font_weight(FontWeight::BOLD)
                .id("settings-kit-title")
                .role(Role::Heading)
                .child(title.into()),
        )
        .child(
            div()
                .id(id)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pl(pt(6.))
                .pr(pt(18.))
                .pb(pt(20.))
                .flex()
                .flex_col()
                .gap(pt(20.))
                .children(sections),
        )
}

/// A titled section: heading, optional subtitle, optional trailing accessory.
pub fn section(
    title: impl Into<SharedString>,
    subtitle: Option<SharedString>,
    accessory: Option<AnyElement>,
    body: impl IntoElement,
) -> AnyElement {
    let title = title.into();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(pt(8.))
        .child(
            div()
                .w_full()
                // Light groups carry a 1 px border; keep titles on the row inset.
                .px(pt(GROUP_PADDING_X) + px(if theme().dark { 0. } else { 1. }))
                .flex()
                .items_end()
                .justify_between()
                .gap(pt(12.))
                .child(
                    // The title column takes the remaining width, so a narrow
                    // or width-less parent cannot collapse it to one glyph.
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(pt(2.))
                        .min_w(pt(80.))
                        .child(
                            div()
                                .id(SharedString::from(format!("heading-{title}")))
                                .font_weight(FontWeight::BOLD)
                                .role(Role::Heading)
                                .child(title),
                        )
                        .when_some(subtitle, |el, subtitle| {
                            el.child(
                                div()
                                    .text_size(pt(SECONDARY + 0.5))
                                    .text_color(ink::secondary())
                                    .child(subtitle),
                            )
                        }),
                )
                .when_some(accessory, |el, accessory| {
                    el.child(div().flex_shrink_0().child(accessory))
                }),
        )
        .child(body)
        .into_any_element()
}

/// Accent text action for a section header ("⊕ New").
pub fn section_action(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    forced: Forced,
) -> Stateful<Div> {
    let label = label.into();
    let el = div()
        .id(id)
        .h(pt(CONTROL_HEIGHT))
        .px(pt(6.) - px(RING))
        .flex()
        .items_center()
        .gap(pt(5.))
        .text_color(ink::accent_text())
        .role(Role::Button)
        .aria_label(label.clone())
        .child(Icon::new(icon).with_size(Size::Size(pt(14.))))
        .child(label);
    interactive_fill(ring(el, forced == Forced::Focus, 6.), forced, None)
}

/// An inset grouped card; rows are separated by inset hairlines.
pub fn group(rows: impl IntoIterator<Item = AnyElement>) -> Div {
    let mut card = div()
        .flex()
        .flex_col()
        .px(pt(GROUP_PADDING_X))
        .rounded(pt(GROUP_RADIUS))
        .bg(rgb(ink::card()))
        .when(!theme().dark, |el| {
            el.border_1().border_color(rgb(ink::hairline()))
        });
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            card = card.child(div().h(px(1.)).bg(rgb(ink::hairline())));
        }
        card = card.child(row);
    }
    card
}

/// One settings row: optional leading element, title and subtitle, trailing
/// control. Build with [`Row::new`]; it renders through [`IntoElement`].
#[derive(IntoElement)]
pub struct Row {
    title: SharedString,
    subtitle: Option<SharedString>,
    leading: Option<AnyElement>,
    trailing: Option<AnyElement>,
    nav: Option<(ElementId, Forced)>,
    on_click: Option<ClickHandler>,
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

impl Row {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            leading: None,
            trailing: None,
            nav: None,
            on_click: None,
        }
    }
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
    pub fn leading(mut self, leading: impl IntoElement) -> Self {
        self.leading = Some(leading.into_any_element());
        self
    }
    pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
        self.trailing = Some(trailing.into_any_element());
        self
    }
    /// A drill-in row: trailing chevron, whole row pressable.
    pub fn navigation(mut self, id: impl Into<ElementId>, forced: Forced) -> Self {
        self.nav = Some((id.into(), forced));
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Row {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let two_lines = self.subtitle.is_some();
        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(pt(2.))
            .child(div().child(self.title.clone()))
            .when_some(self.subtitle, |el, subtitle| {
                el.child(
                    div()
                        .text_size(pt(SECONDARY))
                        .text_color(ink::secondary())
                        .child(subtitle),
                )
            });
        let content = div()
            .w_full()
            .min_h(pt(if two_lines { ROW_WITH_SUBTITLE } else { ROW }))
            .py(pt(7.))
            .flex()
            .items_center()
            .gap(pt(12.))
            .when_some(self.leading, |el, leading| el.child(leading))
            .child(text)
            .when_some(self.trailing, |el, trailing| {
                el.child(div().flex_shrink_0().flex().items_center().child(trailing))
            });
        match self.nav {
            None => content.into_any_element(),
            Some((id, forced)) => {
                let on_click = self.on_click;
                let row = div()
                    .id(id)
                    .mx(-pt(GROUP_PADDING_X))
                    .px(pt(GROUP_PADDING_X) - px(RING))
                    .rounded(pt(GROUP_RADIUS))
                    .role(Role::Button)
                    .aria_label(self.title)
                    .child(
                        content.child(
                            Icon::new(IconName::ChevronRight)
                                .with_size(Size::Size(pt(13.)))
                                .text_color(ink::secondary()),
                        ),
                    )
                    .when_some(on_click, |el, f| el.on_click(move |e, w, cx| f(e, w, cx)));
                interactive_fill(
                    ring(row, forced == Forced::Focus, GROUP_RADIUS),
                    forced,
                    None,
                )
                .into_any_element()
            }
        }
    }
}

mod controls;
pub use controls::*;

/// Vertical divider between clusters inside one group.
pub fn vertical_divider() -> Div {
    div()
        .w(px(1.))
        .self_stretch()
        .my(pt(10.))
        .bg(rgb(ink::hairline()))
}

/// Small secondary caption above a cluster inside a group ("Color scheme").
pub fn caption(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(pt(SECONDARY + 0.5))
        .text_color(ink::secondary())
        .child(text.into())
}
