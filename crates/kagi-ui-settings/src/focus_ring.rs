//! Local focus decoration color; hosts still own effective accent selection.
use gpui::{rgb, Hsla};
use kagi_ui_core::theme;

pub(super) fn color(accent: u32, background: u32) -> Hsla {
    let base: Hsla = rgb(accent).into();
    let surface: Hsla = rgb(background).into();
    let candidate = |lightness| Hsla {
        l: lightness,
        a: 0.75,
        ..base
    };
    let contrast =
        |ring| theme::contrast_ratio(u32::from(surface.blend(ring).to_rgb()) >> 8, background);
    let initial = candidate(base.l);
    // Leave rounding headroom for final composited native/browser pixels.
    if contrast(initial) >= 3.1 {
        return initial;
    }
    let endpoint = if contrast(candidate(0.)) >= contrast(candidate(1.)) {
        0.
    } else {
        1.
    };
    for step in 1..=32 {
        let ring = candidate(base.l + (endpoint - base.l) * step as f32 / 32.);
        if contrast(ring) >= 3.1 {
            return ring;
        }
    }
    candidate(endpoint)
}

#[cfg(test)]
#[test]
fn translucent_ring_contrasts_after_compositing() {
    for background in [0xf7f7f7, 0xffffff, 0xe5e5e5, 0x1e1e1e, 0x29292c, 0x444444] {
        for accent in [0xffcc00, 0x007aff, 0x0a84ff, 0xaf52de, 0xffffff, 0x000000] {
            let ring = color(accent, background);
            let surface: Hsla = rgb(background).into();
            let base: Hsla = rgb(accent).into();
            assert!(ring.a > 0. && ring.a < 1.);
            assert_eq!(ring.h, base.h);
            assert_eq!(ring.s, base.s);
            let painted = u32::from(surface.blend(ring).to_rgb()) >> 8;
            assert!(theme::contrast_ratio(painted, background) >= 3.0);
        }
    }
}
