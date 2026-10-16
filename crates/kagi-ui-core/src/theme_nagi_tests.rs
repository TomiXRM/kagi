//! Mounted inside `theme` so palette invariants reuse the production contrast
//! and filled-foreground helpers without expanding their API visibility.

use super::*;
use crate::color_vision::{delta_e, Cvd};
use crate::theme_nagi::{NAGI_DARK, NAGI_LIGHT};

fn palettes() -> [&'static Theme; 2] {
    [&NAGI_LIGHT, &NAGI_DARK]
}

fn rgb24(color: Hsla) -> u32 {
    let c: gpui::Rgba = color.into();
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    q(c.r) << 16 | q(c.g) << 8 | q(c.b)
}

fn tuple_rgb((r, g, b): (u8, u8, u8)) -> u32 {
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

fn composite(fg: u32, bg: u32, alpha: f32) -> u32 {
    rgb24(to_hsla(bg).blend(to_hsla(fg).alpha(alpha)))
}

fn readable(t: &Theme, role: &str, ink: u32, bg: u32, minimum: f64) {
    let ratio = contrast_ratio(ink, bg);
    assert!(
        ratio >= minimum,
        "{} {role}: #{ink:06x} on #{bg:06x} = {ratio:.3}:1 < {minimum}",
        t.slug,
    );
}

fn surfaces(t: &Theme) -> [u32; 10] {
    [
        t.bg_base,
        t.bg_row_alt,
        t.panel,
        t.sidebar,
        t.modal,
        t.surface,
        t.selected,
        t.inactive_selected,
        t.diff_added_bg,
        t.diff_removed_bg,
    ]
}

fn normal_surfaces(t: &Theme) -> impl Iterator<Item = u32> + '_ {
    surfaces(t).into_iter().filter(move |bg| *bg != t.selected)
}

fn row_ink(t: &Theme, normal: u32, active: bool, selection: f32, background: u32) -> u32 {
    if !t.selection_uses_on_accent(selection > 0.0, active) {
        normal
    } else if selection >= 1.0 {
        rgb24(t.selection_foreground(active))
    } else {
        // Same actual-background policy as file_selection_ink during fades.
        on_accent_foreground(background)
    }
}

fn semantic_inks(t: &Theme) -> [u32; 17] {
    [
        t.text_main,
        t.text_sub,
        t.text_muted,
        t.text_label,
        t.link,
        t.color_head,
        t.color_branch,
        t.color_remote,
        t.color_tag,
        t.color_success,
        t.color_warning,
        t.color_blocker,
        t.change_added,
        t.change_modified,
        t.change_deleted,
        t.change_renamed,
        t.change_typechange,
    ]
}

#[test]
fn nagi_normal_labels_and_selected_ink_follow_the_actual_rendering_policy() {
    for t in palettes() {
        for bg in normal_surfaces(t) {
            for ink in semantic_inks(t)
                .into_iter()
                .chain([t.change_dir, t.diff_hunk])
            {
                readable(t, "label/status/link", ink, bg, 4.5);
                // Text selections are source-alpha overlays, not the raw tint.
                readable(
                    t,
                    "selected label/status/link",
                    ink,
                    composite(t.ui_selection_tint(), bg, SELECTION_ALPHA),
                    4.5,
                );
            }
        }
        for active in [false, true] {
            let bg = rgb24(t.selected_for_window(active));
            readable(
                t,
                "selection primary",
                rgb24(t.selection_foreground(active)),
                bg,
                4.5,
            );
            readable(
                t,
                "selection secondary",
                composite(
                    rgb24(t.selection_secondary_foreground(active)),
                    bg,
                    t.selection_secondary_foreground(active).a,
                ),
                bg,
                4.5,
            );
        }
        for stripe in [t.bg_base, t.bg_row_alt] {
            for active in [false, true] {
                for step in 0..=20 {
                    let selection = step as f32 / 20.0;
                    let wash = t.selection_wash(active);
                    let bg = rgb24(to_hsla(stripe).blend(wash.alpha(wash.a * selection)));
                    for ink in semantic_inks(t) {
                        readable(
                            t,
                            "row semantic/primary/secondary fade",
                            row_ink(t, ink, active, selection, bg),
                            bg,
                            4.5,
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn nagi_selected_component_roles_use_selection_ink_without_recoloring_normal_text() {
    for t in palettes() {
        let mut component = gpui_component::Theme::default();
        apply_gpui_component_theme(t, &mut component);
        let c = component.colors;
        for (role, ink, background) in [
            (
                "selected sidebar",
                c.sidebar_accent_foreground,
                c.sidebar_accent,
            ),
            ("selected tab", c.tab_active_foreground, c.tab_active),
            // The pinned kit has no separate selected list/table text slot.
            // Their ordinary primary ink remains readable; app secondary and
            // semantic labels use selection_foreground at their call sites.
            ("selected list primary", c.foreground, c.list_active),
            ("selected table primary", c.foreground, c.table_active),
        ] {
            readable(t, role, rgb24(ink), rgb24(background), 4.5);
        }
        assert_eq!(c.foreground, to_hsla(t.text_main));
        assert_eq!(c.muted_foreground, to_hsla(t.text_muted));
        assert_eq!(t.selected_for_window(true), to_hsla(t.selected));
        if t.dark {
            assert_eq!(t.selection_foreground(true), to_hsla(0xffffff));
            assert_eq!(t.selection_secondary_foreground(true), to_hsla(0xffffff));
            assert_ne!(rgb24(t.selection_foreground(true)), t.on_accent());
        }
    }
}

#[test]
fn nagi_ref_badges_are_readable_without_tint_stacking_on_selected_rows() {
    let _guard = ACTIVE_THEME_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    struct Restore(ThemeHandle);
    impl Drop for Restore {
        fn drop(&mut self) {
            registry_mut().activate(self.0.clone());
        }
    }
    let _restore = Restore(theme());
    for t in palettes() {
        registry_mut().activate(ThemeHandle::builtin(index_of(&t.slug).unwrap()));
        for color in (0..6).map(|i| rgb24(t.lane_color(i))).chain([
            t.color_head,
            t.color_remote,
            t.color_tag,
        ]) {
            let (fill, border, ink) = graph_badge_style(color);
            let fill_color: Hsla = gpui::rgba(fill).into();
            let border_color: Hsla = gpui::rgba(border).into();
            if t.dark {
                assert!(
                    fill_color.is_opaque(),
                    "selected rows must not stack ref tints"
                );
                assert!(border_color.is_opaque());
            }
            for stripe in [t.bg_base, t.bg_row_alt] {
                for active in [false, true] {
                    for step in 0..=20 {
                        let wash = t.selection_wash(active);
                        let row = to_hsla(stripe).blend(wash.alpha(wash.a * step as f32 / 20.0));
                        let background = rgb24(row.blend(fill_color));
                        readable(t, "ref badge label", ink, background, 4.5);
                        if t.dark {
                            readable(t, "ref badge border", rgb24(border_color), background, 3.0);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn nagi_dark_non_graph_chips_keep_semantic_ink_readable_on_modal_and_surface() {
    let _guard = ACTIVE_THEME_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    struct Restore(ThemeHandle);
    impl Drop for Restore {
        fn drop(&mut self) {
            registry_mut().activate(self.0.clone());
        }
    }
    let _restore = Restore(theme());
    let t = &NAGI_DARK;
    registry_mut().activate(ThemeHandle::builtin(index_of(&t.slug).unwrap()));
    // Modal/title/plan chips keep their semantic foreground instead of the
    // returned badge label. Test that actual composition, not white graph ink.
    for surface in [t.elevated_surface(t.panel), t.surface] {
        for ink in [
            t.ui_accent(),
            t.color_success,
            t.color_warning,
            t.color_blocker,
        ] {
            let (fill, _, _) = badge_style(ink);
            let background = rgb24(to_hsla(surface).blend(gpui::rgba(fill).into()));
            readable(t, "non-graph semantic chip", ink, background, 4.5);
        }
    }
}

#[test]
fn nagi_syntax_reads_on_diff_rows_even_under_text_selection() {
    for t in palettes() {
        let syntax = serde_json::to_value(t.syntax).unwrap();
        for (role, value) in syntax.as_object().unwrap() {
            let ink = value.as_u64().unwrap() as u32;
            for bg in [t.bg_base, t.bg_row_alt, t.diff_added_bg, t.diff_removed_bg] {
                readable(t, role, ink, bg, 4.5);
                readable(
                    t,
                    role,
                    ink,
                    composite(t.selection_tint, bg, SELECTION_ALPHA),
                    4.5,
                );
            }
        }
    }
}

#[test]
fn nagi_filled_labels_reuse_production_foreground_and_interaction_helpers() {
    for t in palettes() {
        for fill in [
            t.ui_accent(),
            t.color_head,
            t.color_remote,
            t.color_tag,
            t.color_success,
            t.color_warning,
            t.color_blocker,
            t.accent,
        ] {
            let foreground = filled_button_foreground(fill, t);
            readable(t, "filled chip", foreground, fill, 4.5);
            let (foreground, hover, pressed) = filled_button_interaction_colors(fill, foreground);
            for bg in [fill, rgb24(hover), rgb24(pressed)] {
                readable(t, "filled interaction", foreground, bg, 4.5);
            }
        }
        readable(
            t,
            "primary helper",
            primary_button_foreground(t),
            t.ui_accent(),
            4.5,
        );
        // Nagi's semantic secondary actions use the existing Default variant,
        // not Custom's fixed 40% pressed tint. Cover its actual neutral states.
        let mut component = gpui_component::Theme::default();
        apply_gpui_component_theme(t, &mut component);
        let c = component.colors;
        for bg in [c.button, c.button_hover, c.button_active] {
            readable(
                t,
                "secondary action",
                rgb24(c.button_foreground),
                rgb24(bg),
                4.5,
            );
        }
        for track in [c.switch, c.primary] {
            readable(t, "switch thumb", rgb24(c.switch_thumb), rgb24(track), 3.0);
            readable(t, "switch track", rgb24(track), t.panel, 3.0);
        }
        readable(
            t,
            "switch off/on distinction",
            rgb24(c.switch),
            rgb24(c.primary),
            1.5,
        );
    }
}

#[test]
fn nagi_graph_strokes_focus_and_labels_survive_real_lane_band_opacities() {
    for t in palettes() {
        for bg in surfaces(t) {
            readable(t, "focus ring", t.ui_accent(), bg, 3.0);
        }
        for i in 0..t.lane_hsl.len() {
            let lane = rgb24(t.lane_color(i));
            let (normal, hover) = (t.lane_band_alpha(false), t.lane_band_alpha(true));
            for bg in [t.bg_base, t.bg_row_alt, t.selected, t.inactive_selected] {
                readable(t, "lane", lane, bg, 3.0);
            }
            for stripe in [t.bg_base, t.bg_row_alt] {
                for alpha in [normal, hover] {
                    let band = composite(lane, stripe, alpha);
                    readable(t, "lane on band", lane, band, 3.0);
                    // render_helpers interpolates the selection wash over lane
                    // bands; partial transitions matter as well as endpoints.
                    for selected in [0.0, 0.5, 1.0] {
                        for active in [false, true] {
                            let wash = t.selection_wash(active);
                            let bg = rgb24(to_hsla(band).blend(wash.alpha(wash.a * selected)));
                            readable(t, "lane on band/selection", lane, bg, 3.0);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn nagi_ansi_slots_and_default_text_read_on_terminal_selection() {
    for t in palettes() {
        let bg = tuple_rgb(t.term_bg);
        let (r, g, b, alpha) = t.term_selection;
        let selected = composite(tuple_rgb((r, g, b)), bg, f32::from(alpha) / 255.0);
        for ink in [
            t.term_fg,
            t.term_black,
            t.term_red,
            t.term_green,
            t.term_yellow,
            t.term_blue,
            t.term_magenta,
            t.term_cyan,
            t.term_white,
            t.term_bright_black,
            t.term_bright_red,
            t.term_bright_green,
            t.term_bright_yellow,
            t.term_bright_blue,
            t.term_bright_magenta,
            t.term_bright_cyan,
            t.term_bright_white,
        ] {
            for bg in [bg, selected] {
                readable(t, "terminal ANSI/default", tuple_rgb(ink), bg, 4.5);
            }
        }
        readable(t, "terminal cursor", tuple_rgb(t.term_cursor), bg, 3.0);
    }
}

#[test]
fn nagi_all_palette_tokens_stay_neutral_or_in_four_families() {
    fn check(t: &Theme, role: &str, color: u32) {
        let c: Hsla = rgb(color).into();
        let channels = [(color >> 16) & 255, (color >> 8) & 255, color & 255];
        let span = channels.iter().max().unwrap() - channels.iter().min().unwrap();
        if span <= 8 {
            return; // Near-neutral quantized tints do not introduce a hue family.
        }
        let hue = c.h * 360.0;
        assert!(
            [8.0_f32, 38.0, 150.0, 205.0].into_iter().any(|family| {
                let distance = (hue - family).abs();
                distance.min(360.0 - distance) <= 8.0
            }),
            "{} {role}: unexpected hue {hue:.1} for #{color:06x}",
            t.slug,
        );
    }
    for t in palettes() {
        // Walk serialized tokens so a future inherited RGB field is covered,
        // rather than silently forgetting it in a hand-maintained allowlist.
        let value = serde_json::to_value(t).unwrap();
        for (role, value) in value.as_object().unwrap() {
            match value {
                serde_json::Value::Number(n) if !role.starts_with("avatar_") => {
                    check(t, role, n.as_u64().unwrap() as u32);
                }
                serde_json::Value::Object(values) => {
                    for (name, value) in values {
                        check(t, name, value.as_u64().unwrap() as u32);
                    }
                }
                serde_json::Value::Array(values) if role.starts_with("term_") => {
                    check(
                        t,
                        role,
                        tuple_rgb((
                            values[0].as_u64().unwrap() as u8,
                            values[1].as_u64().unwrap() as u8,
                            values[2].as_u64().unwrap() as u8,
                        )),
                    );
                }
                _ => {}
            }
        }
        let mut component = gpui_component::Theme::default();
        apply_gpui_component_theme(t, &mut component);
        let colors = serde_json::to_value(component.colors).unwrap();
        for (role, value) in colors.as_object().unwrap() {
            let color: Hsla = serde_json::from_value(value.clone()).unwrap();
            check(t, role, rgb24(color));
        }
        for i in 0..t.lane_hsl.len() {
            check(t, "lane", rgb24(t.lane_color(i)));
        }
        for bg in [
            t.bg_base,
            t.bg_row_alt,
            t.panel,
            t.sidebar,
            t.surface,
            t.modal,
            t.inactive_selected,
        ] {
            let c: Hsla = rgb(bg).into();
            assert!(
                c.s < f32::EPSILON,
                "{} non-neutral surface #{bg:06x}",
                t.slug
            );
        }
    }
}

#[test]
fn nagi_graph_metadata_and_ordinary_code_reserve_color_for_structure_and_state() {
    for t in palettes() {
        for (role, color) in [
            ("remote", t.color_remote),
            ("tag", t.color_tag),
            ("string", t.syntax.string),
            ("function", t.syntax.function),
            ("number", t.syntax.number),
            ("variable", t.syntax.variable),
            ("comment", t.syntax.comment),
            ("operator", t.syntax.operator),
            ("punctuation", t.syntax.punctuation),
            ("attribute", t.syntax.attribute),
        ] {
            let color = to_hsla(color);
            assert!(
                color.s < f32::EPSILON,
                "{} {role} spends a chromatic role",
                t.slug
            );
        }
        for color in [
            t.ui_accent(),
            t.color_head,
            t.syntax.keyword,
            t.syntax.type_name,
        ] {
            let color = to_hsla(color);
            assert!(
                (color.h * 360.0 - 205.0).abs() <= 8.0 && color.s > 0.2,
                "{} interaction/structure must remain visibly blue",
                t.slug
            );
        }
        let mut families = std::collections::BTreeSet::new();
        for i in 0..24 {
            let color = t.lane_color(i);
            assert!(color.s > 0.2, "{} lane {i} must be chromatic", t.slug);
            let hue = color.h * 360.0;
            let family = [38.0_f32, 150.0, 211.0]
                .into_iter()
                .position(|family| (hue - family).abs() <= 8.0)
                .expect("lanes must reuse blue/green/amber families");
            families.insert(family);
            assert_eq!(color, t.lane_color(i + 3), "{} three-lane cycle", t.slug);
            let next = t.lane_color(i + 1);
            let distance = (color.h - next.h).abs() * 360.0;
            assert!(
                distance.min(360.0 - distance) >= 45.0,
                "{} adjacent lane hues",
                t.slug
            );
        }
        assert_eq!(
            families.len(),
            3,
            "{} must retain three lane families",
            t.slug
        );
    }
}

#[test]
fn nagi_active_selection_and_diff_surfaces_remain_visibly_distinct() {
    // ΔE >= 10 is a deliberate visibility floor, not an accessibility or CVD
    // certification. It prevents almost-gray active selection and barely tinted
    // diff rows without prescribing literal colors. The independent 4.5:1 ink
    // and 3:1 stroke contracts still apply; labels and +/- remain necessary.
    for t in palettes() {
        let active = rgb24(t.selected_for_window(true));
        let inactive = rgb24(t.selected_for_window(false));
        if t.dark {
            // A chromatic but near-navy fill is not a visibly bright selection.
            for context in [t.bg_base, t.bg_row_alt] {
                readable(t, "bright active selection/context", active, context, 3.0);
            }
        }
        let color = to_hsla(active);
        assert!(
            (color.h * 360.0 - 205.0).abs() <= 8.0 && color.s > 0.2,
            "{} active selection must be visibly blue",
            t.slug
        );
        let difference = delta_e(active, inactive, None);
        assert!(
            difference >= 10.0,
            "{} active/inactive selection ΔE {difference:.2}",
            t.slug
        );
        for context in [t.bg_base, t.bg_row_alt] {
            for (role, surface) in [
                ("active selection/context", active),
                ("added/context", t.diff_added_bg),
                ("removed/context", t.diff_removed_bg),
            ] {
                let difference = delta_e(surface, context, None);
                assert!(difference >= 10.0, "{} {role} ΔE {difference:.2}", t.slug);
            }
        }
        let difference = delta_e(t.diff_added_bg, t.diff_removed_bg, None);
        assert!(
            difference >= 10.0,
            "{} added/removed ΔE {difference:.2}",
            t.slug
        );
    }
}

#[test]
fn nagi_simulated_color_vision_keeps_ink_distinct_from_its_surface() {
    // This guards foreground/background separation, NOT semantic hue identity
    // or certification. Green/brick and repeated lanes still need glyphs/text.
    for t in palettes() {
        for cvd in Cvd::ALL {
            for ink in semantic_inks(t) {
                for bg in normal_surfaces(t) {
                    let difference = delta_e(ink, bg, Some(cvd));
                    assert!(
                        difference >= 25.0,
                        "{} {cvd:?}: ink/surface ΔE {difference:.2}",
                        t.slug
                    );
                }
            }
            for active in [false, true] {
                let bg = rgb24(t.selected_for_window(active));
                for ink in [
                    rgb24(t.selection_foreground(active)),
                    rgb24(t.selection_secondary_foreground(active)),
                ] {
                    assert!(
                        delta_e(ink, bg, Some(cvd)) >= 25.0,
                        "{} {cvd:?}: selected ink/surface",
                        t.slug
                    );
                }
            }
            eprintln!(
                "{} {cvd:?}: success/danger ΔE {:.2}; added/removed backgrounds ΔE {:.2} (non-color cues required)",
                t.slug,
                delta_e(t.color_success, t.color_blocker, Some(cvd)),
                delta_e(t.diff_added_bg, t.diff_removed_bg, Some(cvd)),
            );
        }
    }
}

#[test]
fn nagi_real_generated_avatars_are_neutral_and_keep_readable_initials() {
    let _guard = ACTIVE_THEME_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    struct Restore(ThemeHandle);
    impl Drop for Restore {
        fn drop(&mut self) {
            registry_mut().activate(self.0.clone());
        }
    }
    let _restore = Restore(theme());
    for t in palettes() {
        let index = index_of(&t.slug).expect("Nagi must be registered");
        registry_mut().activate(ThemeHandle::builtin(index));
        // Exercise the actual hashed-email path, not just avatar_sat == 0.
        for i in 0..96 {
            let avatar = rgb24(crate::avatar::avatar_color(&format!(
                "author-{i}@example.test"
            )));
            let (r, g, b) = ((avatar >> 16) & 255, (avatar >> 8) & 255, avatar & 255);
            assert_eq!(r, g, "{} generated avatar is chromatic", t.slug);
            assert_eq!(g, b, "{} generated avatar is chromatic", t.slug);
            readable(t, "avatar initial", t.bg_base, avatar, 4.5);
        }
    }
}
