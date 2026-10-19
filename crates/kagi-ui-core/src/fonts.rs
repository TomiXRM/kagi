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

/// Static Japanese faces shared by non-platform renderers (for example PDF).
/// Keep asset ownership here rather than embedding another copy in each viewer.
pub fn japanese_font_data(bold: bool) -> &'static [u8] {
    if bold {
        include_bytes!("../../../assets/fonts/NotoSansJP-Bold.ttf").as_slice()
    } else {
        include_bytes!("../../../assets/fonts/NotoSansJP-Regular.ttf").as_slice()
    }
}

/// The deterministic UI font stack used by the gpui-component window root.
pub fn ui_font() -> Font {
    let mut ui = font(UI_FONT);
    ui.fallbacks = Some(FontFallbacks::from_fonts(vec![CJK_FONT.to_string()]));
    ui
}

/// Renderer-fed inherited fonts for native acceptance, never a configured-font mirror.
#[cfg(feature = "gui-e2e")]
pub mod e2e {
    use gpui::{AnyElement, Font, IntoElement as _, Styled as _, WindowId};
    use std::{cell::RefCell, collections::HashMap};

    thread_local! {
        static FONTS: RefCell<HashMap<(WindowId, &'static str), Font>> = RefCell::default();
    }

    /// Zero-size, out-of-flow probe inside the actual styled text container.
    pub fn probe(name: &'static str) -> AnyElement {
        gpui::canvas(
            move |_, window, _| {
                FONTS.with(|fonts| {
                    fonts.borrow_mut().insert(
                        (window.window_handle().window_id(), name),
                        window.text_style().font(),
                    );
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
        .into_any_element()
    }

    pub fn take(window: WindowId, name: &'static str) -> Option<Font> {
        FONTS.with(|fonts| fonts.borrow_mut().remove(&(window, name)))
    }
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
            for ch in ['日', '本', '語', '直', '骨', '誤', '漢', '字', 'あ', 'ア'] {
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
    mod linux {
        use super::*;
        use gpui::{
            px, FontId, FontWeight, PlatformTextSystem, TextRun, TextSystem, WindowTextSystem,
        };
        use gpui_wgpu::CosmicTextSystem;
        use std::{borrow::Cow, sync::Arc};

        fn text_system(
            extra: Vec<Cow<'static, [u8]>>,
            platform: CosmicTextSystem,
        ) -> WindowTextSystem {
            let system = Arc::new(TextSystem::new(Arc::new(platform)));
            let mut fonts = vec![
                Cow::Borrowed(include_bytes!("../../../assets/fonts/Inter-Regular.ttf").as_slice()),
                Cow::Borrowed(include_bytes!("../../../assets/fonts/Inter-Bold.ttf").as_slice()),
                Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf").as_slice(),
                ),
                Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/JetBrainsMono-Bold.ttf").as_slice(),
                ),
                Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/NotoSansJP-Regular.ttf").as_slice(),
                ),
                Cow::Borrowed(
                    include_bytes!("../../../assets/fonts/NotoSansJP-Bold.ttf").as_slice(),
                ),
            ];
            fonts.extend(extra);
            system.add_fonts(fonts).expect("test fonts must load");
            WindowTextSystem::new(system)
        }

        fn face(system: &WindowTextSystem, text: &str, font: Font) -> FontId {
            let run = TextRun {
                len: text.len(),
                font,
                ..TextRun::default()
            };
            let layout = system.layout_line(text, px(16.0), &[run], None);
            assert_eq!(layout.runs.len(), 1, "reference must use one explicit face");
            assert!(!layout.runs[0].glyphs.is_empty());
            assert!(
                layout.runs[0].glyphs.iter().all(|glyph| glyph.id.0 != 0),
                "reference must not contain missing-glyph boxes"
            );
            layout.runs[0].font_id
        }

        fn assert_fallback(system: &WindowTextSystem, primary: &str) {
            let text = "ABC日本語直骨誤";
            let mut japanese_faces = Vec::new();
            for weight in [FontWeight::NORMAL, FontWeight::BOLD] {
                let mut requested = ui_font();
                requested.family = primary.to_owned().into();
                requested.weight = weight;
                let mut reference = font(CJK_FONT);
                reference.weight = weight;
                let japanese = face(system, "日本語直骨誤", reference);
                let latin = face(system, "ABC", requested.clone());
                assert_ne!(latin, japanese, "ASCII must retain {primary}");
                let run = TextRun {
                    len: text.len(),
                    font: requested,
                    ..TextRun::default()
                };
                let layout = system.layout_line(text, px(16.0), &[run], None);
                for (index, ch) in text.char_indices() {
                    let (run, glyph) = layout
                        .runs
                        .iter()
                        .find_map(|run| {
                            run.glyphs
                                .iter()
                                .find(|glyph| glyph.index == index)
                                .map(|glyph| (run, glyph))
                        })
                        .unwrap_or_else(|| panic!("missing shaped glyph {ch}"));
                    assert_ne!(glyph.id.0, 0, "missing glyph {ch} in {primary}");
                    assert_eq!(
                        run.font_id,
                        if ch.is_ascii() { latin } else { japanese },
                        "{primary} {weight:?} {ch} selected the wrong face"
                    );
                }
                eprintln!("{primary} {weight:?}: ASCII={latin:?}, Japanese={japanese:?}");
                japanese_faces.push(japanese);
            }
            assert_ne!(
                japanese_faces[0], japanese_faces[1],
                "Japanese Regular/Bold must select distinct bundled static faces"
            );
        }

        #[test]
        fn linux_cosmic_text_uses_bundled_fallback_for_regular_and_bold() {
            let system = text_system(
                Vec::new(),
                CosmicTextSystem::new_without_system_fonts(UI_FONT),
            );
            assert_fallback(&system, UI_FONT);
            assert_fallback(&system, MONO_FONT);
        }

        // Explicit acceptance lane: never silently pass without a real Nerd font.
        #[test]
        #[ignore = "requires KAGI_TEST_NERD_FONT_REGULAR and KAGI_TEST_NERD_FONT_BOLD"]
        fn linux_cosmic_text_nerd_primary_preserves_icons_and_japanese_fallback() {
            let files = ["KAGI_TEST_NERD_FONT_REGULAR", "KAGI_TEST_NERD_FONT_BOLD"];
            let fonts = files
                .map(|key| {
                    let path =
                        std::env::var(key).expect("supply a real JetBrainsMono Nerd Font Mono");
                    Cow::Owned(std::fs::read(path).expect("Nerd font must be readable"))
                })
                .into();
            let system = text_system(fonts, CosmicTextSystem::new_without_system_fonts(UI_FONT));
            let primary = "JetBrainsMono Nerd Font Mono";
            assert_fallback(&system, primary);
            let mut requested = ui_font();
            requested.family = primary.into();
            assert_eq!(
                face(&system, "\u{e0b0}", requested.clone()),
                face(&system, "ABC", requested),
                "Nerd powerline icon must retain its primary face"
            );
        }

        #[test]
        #[ignore = "requires isolated Linux fontconfig with system Noto Sans CJK SC and JP"]
        fn linux_cosmic_text_system_fonts_preserve_explicit_japanese_fallback() {
            let platform = CosmicTextSystem::new(UI_FONT);
            let names = platform.all_font_names();
            for required in ["Noto Sans CJK SC", "Noto Sans CJK JP"] {
                assert!(
                    names.iter().any(|name| name == required),
                    "missing {required}"
                );
            }
            eprintln!("LANG={:?}", std::env::var("LANG"));
            let system = text_system(Vec::new(), platform);
            assert_fallback(&system, UI_FONT);
            assert_fallback(&system, MONO_FONT);
        }
    }
}
