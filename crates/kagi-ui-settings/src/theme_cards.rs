//! Palette-derived theme choices; preview colors never substitute invented art.
use super::{controls::pt, *};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use kagi_ui_core::{theme::theme, tooltip::HoverTooltip as _};

impl SettingsView {
    pub(crate) fn theme_cards(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let themes = self.snapshot.themes.clone();
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
        if focused_index.is_none() {
            self.groups.insert("themes".into(), selected_index);
        }
        let selection_changed =
            self.theme_reveal
                .as_ref()
                .is_none_or(|(slug, selected, _, _, _)| {
                    slug != &self.snapshot.theme_slug || *selected != selected_index
                });
        let target = if selection_changed {
            selected_index
        } else {
            focused_index.unwrap_or(selected_index)
        };
        if self
            .theme_reveal
            .as_ref()
            .is_none_or(|(slug, selected, focused, zoom, count)| {
                slug != &self.snapshot.theme_slug
                    || *selected != selected_index
                    || (focused_index.is_some() && *focused != focused_index)
                    || *zoom != self.snapshot.zoom
                    || *count != themes.len()
            })
        {
            self.theme_scroll.scroll_to_item(target);
        }
        self.theme_reveal = Some((
            self.snapshot.theme_slug.clone(),
            selected_index,
            focused_index,
            self.snapshot.zoom,
            themes.len(),
        ));
        let mut cards = div()
            .id("settings-theme-cards")
            .role(Role::RadioGroup)
            .aria_label(Msg::SettingsTheme.t())
            .flex()
            .flex_nowrap()
            .items_start()
            .gap(pt(12.))
            .min_w_0()
            .w_full()
            .overflow_x_scroll()
            .track_scroll(&self.theme_scroll)
            .map(|mut strip| {
                // Vertical gestures belong to the containing Settings form.
                strip.style().restrict_scroll_to_axis = Some(true);
                strip
            });
        for (index, palette) in themes.iter().enumerate() {
            let selected = palette.slug.as_ref() == self.snapshot.theme_slug.as_ref();
            let rgb_tuple =
                |(r, g, b): (u8, u8, u8)| ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            let term_bg = rgb_tuple(palette.term_bg);
            let term_fg = rgb_tuple(palette.term_fg);
            let readable = contrast(palette.bg_base, palette.text_main) >= 4.5
                && contrast(term_bg, term_fg) >= 4.5;
            let preview = div()
                .rounded(pt(5.))
                .p(pt(10.))
                .h(pt(74.))
                .flex()
                .justify_between()
                .bg(rgb(palette.bg_base))
                .when(readable, |el| {
                    el.child(
                        div()
                            .text_size(pt(22.))
                            .text_color(rgb(palette.text_main))
                            .child("Aa"),
                    )
                    .child(
                        div()
                            .p(pt(6.))
                            .rounded(pt(3.))
                            .bg(rgb(term_bg))
                            .text_color(rgb(term_fg))
                            .child("~/"),
                    )
                })
                .when(!readable, |el| {
                    el.child(div().w_full().h_full().bg(rgb(term_bg)))
                });
            let name: SharedString =
                kagi_domain::text_safety::sanitize_control_bytes(palette.display_name()).into();
            let caption_text = StyledText::new(name.clone());
            #[cfg(feature = "gui-e2e")]
            let caption_text_probe = {
                let layout = caption_text.layout().clone();
                let probes = self.probes.clone();
                let id = format!("settings-theme-caption-text-{}", palette.slug);
                let name = name.to_string();
                Some(
                    canvas(
                        |_, _, _| (),
                        move |_, _, window, _| {
                            let style = window.text_style();
                            probes.borrow_mut().insert(
                                id.clone(),
                                ControlProbe {
                                    id: id.clone(),
                                    bounds: layout.bounds(),
                                    focused: false,
                                    selected: false,
                                    disabled: false,
                                    role: Role::Label,
                                    name: name.clone(),
                                    value: layout.text(),
                                    paint: Some(ControlPaint {
                                        font_size: style.font_size.to_pixels(window.rem_size()),
                                        font_weight: style.font_weight,
                                        text_color: style.color,
                                    }),
                                },
                            );
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size(px(0.))
                    .into_any_element(),
                )
            };
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
                    theme().panel,
                    px(2.),
                    SettingsIntent::Theme(palette.slug.to_string().into()),
                    Some(("themes".into(), index, ids.clone())),
                    window,
                    cx,
                )
                .w(pt(152.))
                .min_w_0()
                .max_w_full()
                .p(pt(5.))
                .flex()
                .flex_col()
                .gap(pt(6.))
                .border_2()
                .border_color(if selected {
                    rgb(self.snapshot.effective_accent)
                } else {
                    rgb(theme().surface)
                })
                .child(preview)
                .child(
                    div().flex().gap(pt(4.)).children(
                        [
                            ("HEAD", palette.color_head),
                            ("Branch", palette.color_branch),
                            ("Remote", palette.color_remote),
                            ("Tag", palette.color_tag),
                        ]
                        .into_iter()
                        .map(|(label, color)| {
                            div()
                                .id(SharedString::from(format!(
                                    "settings-theme-swatch-{}-{label}",
                                    palette.slug
                                )))
                                .role(Role::Image)
                                .aria_label(format!("{label}: #{color:06x}"))
                                .size(pt(12.))
                                .rounded_full()
                                .border_1()
                                .border_color(rgb(theme().text_main))
                                .bg(rgb(color))
                        }),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .gap(pt(4.))
                        .child(
                            div()
                                .relative()
                                // Stretch the text leaf to the allocated caption width,
                                // rather than retaining its block max-content width.
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .children(caption_probe)
                                .child(caption_text)
                                .children(caption_text_probe),
                        )
                        .when(selected, |el| el.child("✓")),
                )
                .when(!readable, |el| {
                    el.child(
                        div()
                            .text_size(pt(11.5))
                            .child(Msg::SettingsThemeContrastWarning.t()),
                    )
                });
            // The scroll item includes the complete 2px ring and 3px gap.
            // A transparent wrapper is not another visual card.
            cards = cards.child(
                div()
                    .w(pt(152.) + px(14.))
                    .max_w_full()
                    .flex_shrink_0()
                    .p(px(7.))
                    .child(card.hover_tooltip(
                        SharedString::from(format!("settings-theme-name-{}", palette.slug)),
                        name,
                    )),
            );
        }
        let viewport = self.theme_viewport.clone();
        let scroll = self.theme_scroll.clone();
        let resize = canvas(
            move |bounds, window, _| {
                if viewport.replace(bounds.size) != bounds.size {
                    scroll.scroll_to_item(target);
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
        div()
            .id("settings-themes-area")
            .relative()
            .min_w_0()
            .w_full()
            .when(persistent, |area| area.pb(px(16.)))
            .child(cards)
            .child(resize)
            .children(cfg!(feature = "gui-e2e").then(|| {
                self.measured(
                    "settings-themes-scroll".into(),
                    Role::Group,
                    Msg::SettingsTheme.t().into(),
                    String::new(),
                    false,
                    false,
                    false,
                )
            }))
            .child(
                Scrollbar::horizontal(&self.theme_scroll)
                    .id("settings-themes-scrollbar")
                    .when(persistent, |bar| bar.scrollbar_show(ScrollbarShow::Always)),
            )
            .into_any_element()
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
