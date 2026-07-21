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
        let bytes = include_bytes!("../../../assets/fonts/NotoSansJP-Variable.ttf");
        let face = ttf_parser::Face::parse(bytes, 0).expect("Noto Sans JP must stay parseable");
        for ch in ['日', '本', '語', '漢', '字', 'あ', 'ア'] {
            assert!(face.glyph_index(ch).is_some(), "missing bundled glyph {ch}");
        }
        assert!(
            face.variation_axes()
                .into_iter()
                .any(|axis| axis.tag == ttf_parser::Tag::from_bytes(b"wght")),
            "Noto Sans JP must retain its weight axis"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_cosmic_text_uses_bundled_fallback_for_regular_and_bold() {
        use std::{borrow::Cow, sync::Arc};

        use gpui::{px, FontWeight, TextRun, TextSystem, WindowTextSystem};
        use gpui_wgpu::CosmicTextSystem;

        let platform = Arc::new(CosmicTextSystem::new_without_system_fonts(UI_FONT));
        let text_system = Arc::new(TextSystem::new(platform));
        text_system
            .add_fonts(vec![
                Cow::Borrowed(include_bytes!("../../../assets/fonts/Inter-Regular.ttf").as_slice()),
                Cow::Borrowed(include_bytes!("../../../assets/fonts/Inter-Bold.ttf").as_slice()),
                Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/NotoSansJP-Variable.ttf").as_slice(),
                ),
            ])
            .expect("bundled fonts must load into cosmic-text");
        let window_text_system = WindowTextSystem::new(text_system);

        let text = "ABC日本語";
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
        }
    }
}
