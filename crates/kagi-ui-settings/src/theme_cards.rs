//! Palette-derived theme choices; preview colors never substitute invented art.
use super::{
    kit_style::{ink, metrics, pt},
    *,
};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use kagi_ui_core::tooltip::HoverTooltip as _;

impl SettingsView {
    pub(crate) fn theme_cards(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        // Paired families align in both appearances; the remaining catalog
        // keeps its stable source order. This vector also owns keyboard order.
        let mut themes = self.snapshot.themes.clone();
        themes.sort_by_key(|palette| {
            let family = match palette.slug.as_ref() {
                "apple-light" | "apple-dark" => 0,
                "catppuccin-latte" | "catppuccin" => 1,
                "nagi-light" | "nagi-dark" => 2,
                "one-light" | "one-dark" => 3,
                _ => 4,
            };
            (palette.dark, family)
        });
        let ids: Vec<_> = themes
            .iter()
            .map(|theme| format!("settings-theme-{}", theme.slug))
            .collect();
        let selected_index = themes
            .iter()
            .position(|theme| theme.slug.as_ref() == self.snapshot.theme_slug.as_ref())
            .unwrap_or(0);
        let focused_index = ids.iter().position(|id| {
            self.focus
                .get(id)
                .is_some_and(|focus| focus.is_focused(window))
        });
        self.groups
            .insert("themes".into(), focused_index.unwrap_or(selected_index));
        let focused_theme = focused_index.map(|index| themes[index].clone());
        let selection_changed = self
            .theme_reveal
            .as_ref()
            .is_none_or(|last| last.selected_slug != self.snapshot.theme_slug);
        let target = if selection_changed {
            selected_index
        } else {
            focused_index.unwrap_or(selected_index)
        };
        let reveal = self.theme_reveal.as_ref().is_none_or(|last| {
            selection_changed
                || last.previous_catalog.is_some()
                || last.zoom != self.snapshot.zoom
                || (focused_theme.is_some()
                    && last.focused_theme.as_ref().map(|theme| &theme.slug)
                        != focused_theme.as_ref().map(|theme| &theme.slug))
        });
        let reveal_vertically =
            focused_theme.is_some() || (self.theme_reveal.is_some() && selection_changed);
        self.theme_reveal = Some(ThemePresentation {
            selected_slug: self.snapshot.theme_slug.clone(),
            focused_theme,
            zoom: self.snapshot.zoom,
            previous_catalog: None,
        });
        let mut sections = div()
            .id("settings-theme-cards")
            .role(Role::RadioGroup)
            .aria_label(Msg::SettingsTheme.t())
            .flex()
            .flex_col()
            .gap(pt(20.))
            .min_w_0()
            .w_full();
        let light_count = themes.iter().take_while(|palette| !palette.dark).count();
        for mode in 0..2 {
            let (start, end, label, scroll_id) = if mode == 0 {
                (
                    0,
                    light_count,
                    Msg::SettingsLightMode,
                    "settings-themes-light-scroll",
                )
            } else {
                (
                    light_count,
                    themes.len(),
                    Msg::SettingsDarkMode,
                    "settings-themes-dark-scroll",
                )
            };
            let target_in_strip = (start..end).contains(&target);
            let local_target = target.saturating_sub(start);
            if reveal && target_in_strip {
                self.theme_scroll[mode].scroll_to_item(local_target);
            }
            let mut cards = div()
                .id(SharedString::from(format!("{scroll_id}-items")))
                .flex()
                .flex_nowrap()
                .items_start()
                // Wrappers reserve the existing unscaled ring clearance without
                // changing the canonical eight-point visible card gap.
                .gap(pt(8.) - px(metrics::FOCUS_REVEAL_MARGIN * 2.))
                .min_w_0()
                .w_full()
                .overflow_x_scroll()
                .track_scroll(&self.theme_scroll[mode])
                .map(|mut strip| {
                    // Vertical gestures belong to the containing Settings form.
                    strip.style().restrict_scroll_to_axis = Some(true);
                    strip
                });
            for (index, palette) in themes.iter().enumerate().take(end).skip(start) {
                let selected = palette.slug.as_ref() == self.snapshot.theme_slug.as_ref();
                let rgb_tuple =
                    |(r, g, b): (u8, u8, u8)| ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                let term_bg = rgb_tuple(palette.term_bg);
                let term_fg = rgb_tuple(palette.term_fg);
                let badge_ink = [term_bg, term_fg, palette.text_main, palette.bg_base]
                    .into_iter()
                    .find(|color| contrast(palette.accent, *color) >= 4.5);
                let readable = contrast(palette.bg_base, palette.text_main) >= 4.5
                    && contrast(term_bg, term_fg) >= 4.5
                    && badge_ink.is_some();
                let preview = div()
                    .relative()
                    .w_full()
                    .rounded(pt(metrics::GROUP_RADIUS))
                    .p(pt(10.))
                    .h(pt(84.))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .bg(rgb(palette.bg_base))
                    .border_2()
                    .border_color(if selected {
                        rgb(self.snapshot.effective_accent).into()
                    } else {
                        transparent_black()
                    })
                    .children(cfg!(feature = "gui-e2e").then(|| {
                        self.measured(
                            format!("settings-theme-preview-{}", palette.slug),
                            Role::Image,
                            palette.display_name().to_string(),
                            String::new(),
                            false,
                            selected,
                            false,
                        )
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .when(readable, |el| {
                                el.child(
                                    div()
                                        .text_size(pt(25.))
                                        .text_color(rgb(palette.text_main))
                                        .child("Aa"),
                                )
                            })
                            .when(!readable, |el| {
                                el.child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "settings-theme-contrast-label-{}",
                                            palette.slug
                                        )))
                                        .text_size(pt(metrics::SECONDARY))
                                        .role(Role::Label)
                                        .aria_label(Msg::SettingsThemeContrastWarning.t())
                                        .text_color(ink::text())
                                        .bg(rgb(ink::window()))
                                        .rounded(pt(3.))
                                        .px(pt(4.))
                                        .child("!")
                                        .hover_tooltip(
                                            SharedString::from(format!(
                                                "settings-theme-contrast-{}",
                                                palette.slug
                                            )),
                                            SharedString::from(
                                                Msg::SettingsThemeContrastWarning.t(),
                                            ),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(pt(6.))
                                    .child(
                                        div()
                                            .relative()
                                            .px(pt(5.))
                                            .h(pt(26.))
                                            .flex()
                                            .items_center()
                                            .rounded(pt(5.))
                                            .bg(rgb(palette.accent))
                                            .when_some(badge_ink, |el, color| {
                                                el.text_color(rgb(color))
                                            })
                                            .text_size(pt(16.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .children(cfg!(feature = "gui-e2e").then(|| {
                                                self.measured(
                                                    format!(
                                                        "settings-theme-badge-{}",
                                                        palette.slug
                                                    ),
                                                    Role::Image,
                                                    "Palette accent badge".into(),
                                                    palette.accent.to_string(),
                                                    false,
                                                    false,
                                                    false,
                                                )
                                            }))
                                            .when(readable, |el| el.child("~/")),
                                    )
                                    .child(
                                        div()
                                            .w(pt(10.))
                                            .h(pt(26.))
                                            .rounded(pt(3.))
                                            .bg(rgb(rgb_tuple(palette.term_cursor))),
                                    ),
                            ),
                    )
                    .child(
                        div().flex().items_center().justify_between().children(
                            [
                                ("black", palette.term_black),
                                ("red", palette.term_red),
                                ("green", palette.term_green),
                                ("yellow", palette.term_yellow),
                                ("blue", palette.term_blue),
                                ("magenta", palette.term_magenta),
                                ("cyan", palette.term_cyan),
                                ("white", palette.term_white),
                            ]
                            .into_iter()
                            .map(|(label, color)| {
                                let color = rgb_tuple(color);
                                div()
                                    .id(SharedString::from(format!(
                                        "settings-theme-swatch-{}-{label}",
                                        palette.slug
                                    )))
                                    .role(Role::Image)
                                    .aria_label(format!("{label}: #{color:06x}"))
                                    .size(pt(11.))
                                    .flex_shrink_0()
                                    .rounded(pt(6.))
                                    .bg(rgb(color))
                            }),
                        ),
                    );
                let name: SharedString =
                    kagi_domain::text_safety::sanitize_control_bytes(palette.display_name()).into();
                let caption_text = StyledText::new(name.clone());
                #[cfg(feature = "gui-e2e")]
                let caption_text_probe = Some(self.measured_text(
                    format!("settings-theme-caption-text-{}", palette.slug),
                    name.to_string(),
                    &caption_text,
                ));
                #[cfg(not(feature = "gui-e2e"))]
                let caption_text_probe = None::<AnyElement>;
                #[cfg(feature = "gui-e2e")]
                let caption_probe = Some(self.measured(
                    format!("settings-theme-caption-{}", palette.slug),
                    Role::Label,
                    name.to_string(),
                    String::new(),
                    false,
                    false,
                    false,
                ));
                #[cfg(not(feature = "gui-e2e"))]
                let caption_probe = None::<AnyElement>;
                let card = self
                    .control(
                        ids[index].clone(),
                        name.clone(),
                        selected,
                        true,
                        Role::RadioButton,
                        ink::card(),
                        px(0.),
                        16.,
                        Some(SettingsIntent::Theme(palette.slug.to_string().into())),
                        Some(("themes".into(), index, ids.clone())),
                        window,
                        cx,
                    )
                    .w(pt(super::kit_style::metrics::THEME_CARD_WIDTH))
                    .flex_shrink_0()
                    .min_w_0()
                    .rounded(pt(metrics::GROUP_RADIUS))
                    .hover(|style| style.bg(rgb(ink::control_hover())))
                    .active(|style| style.bg(rgb(ink::control_pressed())))
                    .flex()
                    .flex_col()
                    .gap(pt(6.))
                    .child(preview)
                    .child(
                        div()
                            .relative()
                            // The full caption wraps within the fixed preview/card width.
                            .flex()
                            .flex_col()
                            .w_full()
                            .min_w_0()
                            .text_center()
                            .text_size(pt(metrics::SECONDARY))
                            .text_color(if selected {
                                ink::text()
                            } else {
                                ink::secondary()
                            })
                            .when(selected, |el| el.font_weight(FontWeight::SEMIBOLD))
                            .children(caption_probe)
                            .child(caption_text)
                            .children(caption_text_probe),
                    );
                cards = cards.child(
                    div()
                        .w(pt(metrics::THEME_CARD_WIDTH) + px(metrics::FOCUS_REVEAL_MARGIN * 2.))
                        .flex_shrink_0()
                        .p(px(metrics::FOCUS_REVEAL_MARGIN))
                        .child(card),
                );
            }
            let viewport = self.theme_viewport[mode].clone();
            let scroll = self.theme_scroll[mode].clone();
            let detail_scroll = self.scroll.clone();
            let resize = canvas(
                move |bounds, window, cx| {
                    let resized = viewport.replace(bounds.size) != bounds.size;
                    if target_in_strip && (reveal || resized) {
                        scroll.scroll_to_item(local_target);
                        if reveal_vertically {
                            // Read the ancestor's current allocation after prepaint,
                            // not its previous-frame (initially empty) viewport.
                            let detail_scroll = detail_scroll.clone();
                            window.defer(cx, move |window, _| {
                                let visible = detail_scroll.bounds();
                                let margin = px(super::kit_style::metrics::FOCUS_REVEAL_MARGIN);
                                let delta = if bounds.top() < visible.top() + margin {
                                    visible.top() + margin - bounds.top()
                                } else if bounds.bottom() > visible.bottom() - margin {
                                    visible.bottom() - margin - bounds.bottom()
                                } else {
                                    px(0.)
                                };
                                let old = detail_scroll.offset();
                                let offset = point(
                                    old.x,
                                    (old.y + delta).clamp(-detail_scroll.max_offset().y, px(0.)),
                                );
                                if offset != old {
                                    detail_scroll.set_offset(offset);
                                    window.refresh();
                                }
                            });
                        }
                        window.on_next_frame(|window, _| window.refresh());
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full();
            let persistent =
                gpui_component::Theme::global(cx).scrollbar_show != ScrollbarShow::Scrolling;
            let area = div()
                .id(SharedString::from(scroll_id))
                .relative()
                .min_w_0()
                .mx(-px(metrics::FOCUS_REVEAL_MARGIN))
                .when(persistent, |area| area.pb(px(16.)))
                .child(cards)
                .children(cfg!(feature = "gui-e2e").then(|| {
                    self.measured(
                        scroll_id.into(),
                        Role::Group,
                        label.t().into(),
                        String::new(),
                        false,
                        false,
                        false,
                    )
                }))
                .child(
                    Scrollbar::horizontal(&self.theme_scroll[mode])
                        .id(SharedString::from(format!("{scroll_id}-scrollbar")))
                        .when(persistent, |bar| bar.scrollbar_show(ScrollbarShow::Always)),
                );
            sections = sections.child(
                div()
                    .relative()
                    .min_w_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(
                        self.section(
                            label,
                            None,
                            self.group(if mode == 0 {
                                "settings-themes-light-group"
                            } else {
                                "settings-themes-dark-group"
                            })
                            .child(
                                div()
                                    .py(pt(12.) - px(metrics::FOCUS_REVEAL_MARGIN))
                                    .child(area),
                            ),
                        ),
                    )
                    // Reveal the real section, including its heading, rather
                    // than clipping that heading above a revealed card strip.
                    .child(resize)
                    .children(cfg!(feature = "gui-e2e").then(|| {
                        self.measured(
                            format!("settings-theme-mode-section-{mode}"),
                            Role::Group,
                            label.t().into(),
                            String::new(),
                            false,
                            false,
                            false,
                        )
                    })),
            );
        }
        sections.into_any_element()
    }
}
fn contrast(a: u32, b: u32) -> f32 {
    fn luminance(color: u32) -> f32 {
        let channel = |shift: u32| {
            let v = ((color >> shift) & 255u32) as f32 / 255.;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::contrast;
    #[test]
    fn contrast_requires_real_palette_legibility() {
        assert!((contrast(0, 0xffffff) - 21.).abs() < 0.01);
        assert_eq!(contrast(0x777777, 0x777777), 1.);
    }
}
