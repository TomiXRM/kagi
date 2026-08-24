//! Commit author avatar primitives — the deterministic fallback colour/initial
//! (T020) plus the resolved-image map type, moved here from the bin's
//! `src/ui/avatar.rs` so the extracted pane crates can render the same avatar
//! as Graph mode's Inspector.
//!
//! Everything here is pure/deterministic: no network calls. The *resolution*
//! of a real GitHub/Gravatar image (ADR-0037/0123 — HTTP, disk cache,
//! per-repo retry state) stays bin-side in `avatar_fetch`/`AvatarStore`; this
//! module only names the resulting map and draws from it.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{hsla, Hsla};

/// Resolved avatar images keyed by author email — the render-facing half of
/// the bin's `AvatarStore`. Pane crates receive a clone of this (pushed in by
/// the host right before the entity is embedded, like `show_tree` /
/// `panel_width`) since they can't reach `KagiApp` themselves.
pub type AvatarImages = HashMap<String, Arc<gpui::Image>>;

// ──────────────────────────────────────────────────────────────
// FNV-1a hash (32-bit)
// ──────────────────────────────────────────────────────────────

/// Compute a 32-bit FNV-1a hash of `s`.
///
/// This is a self-contained implementation that does not depend on any
/// external crate so the hash value is fully stable across platform and
/// compiler versions.
fn fnv1a_32(s: &str) -> u32 {
    const OFFSET_BASIS: u32 = 2_166_136_261;
    const PRIME: u32 = 16_777_619;
    let mut hash = OFFSET_BASIS;
    for byte in s.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

// ──────────────────────────────────────────────────────────────
// Hue palette (12 divisions, Catppuccin-inspired saturation/lightness)
// ──────────────────────────────────────────────────────────────

/// 12 evenly-spaced hue values (0.0–1.0 range, i.e. hue/360°).
const HUE_COUNT: u32 = 12;

/// Return a stable [`Hsla`] colour for `email`.
///
/// The hue is selected from 12 evenly-distributed values using the low bits
/// of a FNV-1a hash of the email string.  Saturation and lightness are fixed
/// so all avatars look equally vibrant regardless of which hue is chosen.
///
/// The same email always produces the same colour (deterministic).
pub fn avatar_color(email: &str) -> Hsla {
    let hash = fnv1a_32(email);
    let bucket = hash % HUE_COUNT;
    // hue in [0.0, 1.0) — gpui uses the 0–1 range for HSLA hue.
    let hue = (bucket as f32) / (HUE_COUNT as f32);
    // Saturation / lightness come from the active theme so light themes get
    // legible avatars (W9-THEME). Catppuccin (default) keeps 0.70 / 0.60.
    let t = crate::theme::theme();
    hsla(hue, t.avatar_sat, t.avatar_light, 1.0)
}

// ──────────────────────────────────────────────────────────────
// Initial extraction
// ──────────────────────────────────────────────────────────────

/// Return the display initial for an author `name`.
///
/// - Returns the first Unicode scalar (char) of the name, converted to
///   uppercase via [`char::to_uppercase`].  For ASCII letters this is the
///   capital letter; for Japanese / CJK characters the character is returned
///   as-is (uppercase is a no-op).
/// - Returns `"?"` if `name` is empty.
///
/// Note: grapheme-cluster boundaries are intentionally ignored.  A single
/// `char` is sufficient for the avatar initial use-case and keeps the
/// implementation dependency-free.
pub fn avatar_initial(name: &str) -> String {
    match name.chars().next() {
        Some(ch) => ch.to_uppercase().collect(),
        None => "?".to_string(),
    }
}

// ──────────────────────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── avatar_color tests ───────────────────────────────────

    /// Different emails spread across multiple hues (collision on 12 buckets
    /// is unlikely for 4 very different inputs).
    #[test]
    fn test_color_spreads() {
        let emails = [
            "alice@example.com",
            "bob@example.com",
            "carol@foo.org",
            "dave@bar.net",
        ];
        let hues: Vec<f32> = emails.iter().map(|e| avatar_color(e).h).collect();
        // Not all hues should be identical.
        let all_same = hues.windows(2).all(|w| (w[0] - w[1]).abs() < f32::EPSILON);
        assert!(!all_same, "expected hues to differ: {:?}", hues);
    }

    /// Hue is always in the valid [0.0, 1.0) range.
    #[test]
    fn test_color_hue_range() {
        let samples = [
            "",
            "a@b.com",
            "very.long.email.address+tag@subdomain.example.co.uk",
            "中文@example.com",
        ];
        for s in &samples {
            let c = avatar_color(s);
            assert!(
                c.h >= 0.0 && c.h < 1.0,
                "hue out of range for {:?}: {}",
                s,
                c.h
            );
        }
    }

    /// W9-THEME: saturation/lightness come from the ACTIVE theme, not from
    /// hard-coded constants - that is the whole point of the theme fields, and
    /// hard-coding 0.70/0.60 back into `avatar_color` must fail here.
    /// Mutates the global active theme, so it holds the theme test lock.
    #[test]
    fn test_color_follows_active_theme() {
        let _g = crate::theme::ACTIVE_THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let prev = crate::theme::theme().slug;
        std::env::set_var("KAGI_LOG_DIR", tmp.path());

        // Two themes with deliberately different avatar_sat / avatar_light.
        for slug in ["catppuccin", "one-light"] {
            assert!(crate::theme::set_active(slug));
            let t = crate::theme::theme();
            let c = avatar_color("test@example.com");
            assert_eq!(c.s, t.avatar_sat, "{slug} saturation");
            assert_eq!(c.l, t.avatar_light, "{slug} lightness");
            assert_eq!(c.a, 1.0);
        }
        // Guard against the two themes accidentally converging (which would
        // make the loop above tautological).
        let a = crate::theme::THEMES
            .iter()
            .find(|t| t.slug == "catppuccin")
            .unwrap();
        let b = crate::theme::THEMES
            .iter()
            .find(|t| t.slug == "one-light")
            .unwrap();
        assert_ne!(a.avatar_light, b.avatar_light);

        crate::theme::set_active(prev);
        std::env::remove_var("KAGI_LOG_DIR");
    }

    // ── avatar_initial tests ─────────────────────────────────

    /// ASCII lowercase name → uppercase initial.
    #[test]
    fn test_initial_ascii() {
        assert_eq!(avatar_initial("alice"), "A");
        assert_eq!(avatar_initial("Bob"), "B");
    }

    /// Already-uppercase name → same initial.
    #[test]
    fn test_initial_already_upper() {
        assert_eq!(avatar_initial("Carol"), "C");
    }

    /// Japanese name → first character returned as-is.
    #[test]
    fn test_initial_japanese() {
        assert_eq!(avatar_initial("田中"), "田");
    }

    /// Empty name → "?".
    #[test]
    fn test_initial_empty() {
        assert_eq!(avatar_initial(""), "?");
    }

    /// Single character name.
    #[test]
    fn test_initial_single_char() {
        assert_eq!(avatar_initial("z"), "Z");
    }

    /// Multi-code-point uppercase (e.g. German ß → SS).
    #[test]
    fn test_initial_german_eszett() {
        // 'ß'.to_uppercase() == "SS"
        assert_eq!(avatar_initial("ße"), "SS");
    }
}
