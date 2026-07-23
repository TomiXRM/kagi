//! Bundled font families and the deterministic UI fallback stack.

use gpui::{font, Font, FontFallbacks};

/// Bundled UI sans family (OFL Inter), loaded at startup via `add_fonts`, so the
/// UI looks identical on every OS instead of relying on the platform default.
pub const UI_FONT: &str = "Inter";

/// Bundled monospace family (OFL JetBrains Mono) for the terminal / conflict
/// editor / code — replaces the macOS-only "Menlo" fallback.
pub const MONO_FONT: &str = "JetBrains Mono";

/// Bundled Japanese fallback used when Inter does not contain a glyph.
pub const CJK_FONT: &str = "Noto Sans JP";

/// The deterministic UI font stack used by the gpui-component window root.
pub fn ui_font() -> Font {
    let mut ui = font(UI_FONT);
    ui.fallbacks = Some(FontFallbacks::from_fonts(vec![CJK_FONT.to_string()]));
    ui
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_font_has_bundled_japanese_fallback() {
        let font = ui_font();
        assert_eq!(font.family.as_ref(), UI_FONT);
        assert_eq!(
            font.fallbacks
                .expect("UI font must carry a fallback")
                .fallback_list(),
            &[CJK_FONT.to_string()]
        );
    }

    #[test]
    fn bundled_japanese_font_covers_kana_and_kanji() {
        for bytes in [
            include_bytes!("../../../assets/fonts/NotoSansJP-Regular.ttf").as_slice(),
            include_bytes!("../../../assets/fonts/NotoSansJP-Bold.ttf").as_slice(),
        ] {
            let face = ttf_parser::Face::parse(bytes, 0).expect("Noto Sans JP must stay parseable");
            for ch in ['日', '本', '語', '漢', '字', 'あ', 'ア'] {
                assert!(face.glyph_index(ch).is_some(), "missing bundled glyph {ch}");
            }
        }
    }

    /// The bundled Japanese faces must be *static* Regular(400)/Bold(700), not
    /// the variable font. On Linux cosmic-text renders a variable fallback at
    /// its default axis, and Noto Sans JP's variable default is Thin (wght=100),
    /// which made Japanese text render thin. Static faces carry the requested
    /// weight in `usWeightClass` so fontdb resolves it directly.
    #[test]
    fn bundled_japanese_faces_are_static_regular_and_bold() {
        let regular = include_bytes!("../../../assets/fonts/NotoSansJP-Regular.ttf");
        let bold = include_bytes!("../../../assets/fonts/NotoSansJP-Bold.ttf");
        let reg = ttf_parser::Face::parse(regular, 0).expect("Regular must parse");
        let bld = ttf_parser::Face::parse(bold, 0).expect("Bold must parse");
        assert!(
            reg.variation_axes().into_iter().next().is_none(),
            "Regular must be static (no fvar)"
        );
        assert!(
            bld.variation_axes().into_iter().next().is_none(),
            "Bold must be static (no fvar)"
        );
        assert_eq!(
            reg.weight(),
            ttf_parser::Weight::Normal,
            "Regular must be weight 400"
        );
        assert_eq!(
            bld.weight(),
            ttf_parser::Weight::Bold,
            "Bold must be weight 700"
        );
        assert!(!reg.is_bold(), "Regular must not set the bold flag");
        assert!(bld.is_bold(), "Bold must set the bold flag");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_cosmic_text_uses_bundled_fallback_for_regular_and_bold() {
        use std::{borrow::Cow, sync::Arc};

        use gpui::{px, FontId, FontWeight, TextRun, TextSystem, WindowTextSystem};
        use gpui_wgpu::CosmicTextSystem;

        let platform = Arc::new(CosmicTextSystem::new_without_system_fonts(UI_FONT));
        let text_system = Arc::new(TextSystem::new(platform));
        text_system
            .add_fonts(vec![
                Cow::Borrowed(include_bytes!("../../../assets/fonts/Inter-Regular.ttf").as_slice()),
                Cow::Borrowed(include_bytes!("../../../assets/fonts/Inter-Bold.ttf").as_slice()),
                Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/NotoSansJP-Regular.ttf").as_slice(),
                ),
                Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/NotoSansJP-Bold.ttf").as_slice(),
                ),
            ])
            .expect("bundled fonts must load into cosmic-text");
        let window_text_system = WindowTextSystem::new(text_system);

        let text = "ABC日本語";
        let mut jp_font_ids: Vec<FontId> = Vec::new();
        for weight in [FontWeight::NORMAL, FontWeight::BOLD] {
            let mut font = ui_font();
            font.weight = weight;
            let run = TextRun {
                len: text.len(),
                font,
                ..TextRun::default()
            };
            let layout = window_text_system.layout_line(text, px(16.0), &[run], None);
            assert_eq!(
                layout.runs.len(),
                2,
                "{weight:?} must shape Inter Latin and Noto Japanese separately"
            );
            assert_ne!(layout.runs[0].font_id, layout.runs[1].font_id);
            jp_font_ids.push(layout.runs[1].font_id);
        }

        // Regression guard for the "thin Japanese on Linux" bug: with the old
        // *variable* fallback, cosmic-text had a single Noto face and rendered
        // both weights at the font's default axis (Thin/wght=100), so the
        // Japanese run resolved to the *same* font id for NORMAL and BOLD. The
        // static Regular/Bold pair must resolve to two *distinct* faces, so the
        // requested weight actually reaches the glyphs.
        assert_ne!(
            jp_font_ids[0], jp_font_ids[1],
            "NORMAL and BOLD Japanese must resolve to distinct static faces, not one thin variable face"
        );
    }
}
