//! Commit author avatar helpers — T020
//!
//! Provides:
//! - `avatar_color(email)` — stable HSLA colour from email hash (FNV-1a, 12-hue palette)
//! - `avatar_initial(name)` — first char of name uppercased; "?" for empty strings
//!
//! No network calls are made; avatars are purely local / deterministic.

use gpui::{hsla, Hsla};

// ──────────────────────────────────────────────────────────────
// Avatar cache store
// ──────────────────────────────────────────────────────────────

/// Cohesive cache for resolved commit-author avatars (ADR-0118 Phase 5.2).
///
/// Groups the two formerly-flat `KagiApp` fields so the avatar cache moves as a
/// unit. Behaviour-preserving.
#[derive(Default)]
pub struct AvatarStore {
    /// Resolved avatar images keyed by author email.  Populated by a background
    /// resolution pass; rows/inspector swap the initial circle for `img(...)`
    /// when an entry exists.  Memory cache (the disk cache lives under
    /// `~/.kagi/avatars/`).
    pub images: std::collections::HashMap<String, std::sync::Arc<gpui::Image>>,
    /// Repo path the `attempted` set belongs to. Switching repos resets the
    /// set so an email unresolved in one repo can retry with the next repo's
    /// Commits API map (ADR-0123).
    pub fetch_for: Option<std::path::PathBuf>,
    /// Emails a resolution pass has already been spawned for in the current
    /// repo (ADR-0123 incremental resolution). Emails deferred by the
    /// search-budget cap are removed again on completion so they retry.
    pub attempted: std::collections::HashSet<String>,
    /// `KagiApp::view_epoch` value the rows were last scanned at — the scan
    /// re-runs only when the view data changed (reload / load more / tab
    /// switch), keeping the per-frame `ensure_avatars` call one comparison.
    pub scan_epoch: Option<u64>,
}

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
    let t = crate::ui::theme::theme();
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

    /// Same email always produces the same colour (stability).
    #[test]
    fn test_color_stable() {
        let c1 = avatar_color("alice@example.com");
        let c2 = avatar_color("alice@example.com");
        // Compare via hue (f32 exact equality is fine here — same computation).
        assert_eq!(c1.h, c2.h);
        assert_eq!(c1.s, c2.s);
        assert_eq!(c1.l, c2.l);
        assert_eq!(c1.a, c2.a);
    }

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
            assert_eq!(c.s, 0.70);
            assert_eq!(c.l, 0.60);
            assert_eq!(c.a, 1.0);
        }
    }

    /// Saturation, lightness and alpha are fixed regardless of input.
    #[test]
    fn test_color_fixed_saturation_lightness() {
        let c = avatar_color("test@example.com");
        assert_eq!(c.s, 0.70, "saturation must be 0.70");
        assert_eq!(c.l, 0.60, "lightness must be 0.60");
        assert_eq!(c.a, 1.0, "alpha must be 1.0");
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
