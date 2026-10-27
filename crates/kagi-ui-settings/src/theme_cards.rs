//! Palette-derived theme choices; preview colors never substitute invented art.
use super::{controls::pt, *};
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
        if !ids.iter().any(|id| {
            self.focus
                .get(id)
                .is_some_and(|focus| focus.is_focused(window))
        }) {
            self.groups.insert("themes".into(), selected_index);
        }
        let mut cards = div()
            .id("settings-theme-cards")
            .role(Role::RadioGroup)
            .aria_label(Msg::SettingsTheme.t())
            .flex()
            .flex_wrap()
            .gap(pt(12.))
            .min_w_0();
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
            let caption_probe = self.measured(
                format!("settings-theme-caption-{}", palette.slug),
                Role::Label,
                name.to_string(),
                String::new(),
                false,
                false,
                false,
            );
            let card = self
                .control(
                    ids[index].clone(),
                    name.clone(),
                    selected,
                    true,
                    Role::RadioButton,
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
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(caption_probe)
                                .child(name.clone()),
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
            cards = cards.child(card.hover_tooltip(
                SharedString::from(format!("settings-theme-name-{}", palette.slug)),
                name,
            ));
        }
        cards.into_any_element()
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
