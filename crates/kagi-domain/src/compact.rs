//! Compact layout membership in root-rem units (#1640).
//!
//! The UI supplies its logical viewport width and actual current root rem size.
//! OS display scaling is already reflected in logical coordinates and must not
//! be applied again here. A rem is a font-size unit, not a measured glyph width.

/// Default switch width. At the current 14.4px base this is 1008 logical px,
/// deliberately close to, but not exactly, the legacy 1000px breakpoint.
pub const DEFAULT_THRESHOLD_REM: u32 = 70;
/// Smallest configurable switch width, in whole rem.
pub const MIN_THRESHOLD_REM: u32 = 40;
/// Largest configurable switch width, in whole rem.
pub const MAX_THRESHOLD_REM: u32 = 160;

/// Whether the viewport is strictly narrower than the configured rem width.
///
/// Inputs are the window's logical width, its positive root rem size in logical
/// pixels, and the validated whole-rem preference. At exactly the threshold the
/// layout is wide. Compare the product rather than dividing the viewport width:
/// a rounded quotient could put a width just below the threshold on its boundary.
#[inline]
pub fn is_compact(logical_width_px: f32, root_rem_size_px: f32, threshold_rem: u32) -> bool {
    logical_width_px < root_rem_size_px * threshold_rem as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_strict_at_every_supported_zoom() {
        for threshold in [
            MIN_THRESHOLD_REM,
            DEFAULT_THRESHOLD_REM,
            80,
            MAX_THRESHOLD_REM,
        ] {
            for zoom in [0.7, 1.0, 1.25, 1.67, 2.0] {
                let rem = 14.4 * zoom;
                let boundary = rem * threshold as f32;
                // Adjacent representable widths also catch division-rounding
                // regressions which whole-pixel boundary probes can miss.
                let below = f32::from_bits(boundary.to_bits() - 1);
                let above = f32::from_bits(boundary.to_bits() + 1);
                assert!(
                    is_compact(below, rem, threshold),
                    "{threshold}rem at {zoom}"
                );
                assert!(
                    !is_compact(boundary, rem, threshold),
                    "{threshold}rem at {zoom}"
                );
                assert!(
                    !is_compact(above, rem, threshold),
                    "{threshold}rem at {zoom}"
                );
            }
        }
    }

    #[test]
    fn equal_rem_widths_have_equal_membership_across_zoom() {
        for threshold in [
            MIN_THRESHOLD_REM,
            DEFAULT_THRESHOLD_REM,
            80,
            MAX_THRESHOLD_REM,
        ] {
            for width_rem in [
                threshold as f32 - 1.0,
                threshold as f32,
                threshold as f32 + 1.0,
            ] {
                let expected = width_rem < threshold as f32;
                for zoom in [0.7, 1.0, 1.25, 1.67, 2.0] {
                    let rem = 14.4 * zoom;
                    assert_eq!(is_compact(width_rem * rem, rem, threshold), expected);
                }
            }
        }
    }

    #[test]
    fn increasing_text_size_changes_membership_at_the_same_logical_width() {
        assert!(!is_compact(1080.0, 14.4, DEFAULT_THRESHOLD_REM));
        assert!(is_compact(1080.0, 18.0, DEFAULT_THRESHOLD_REM));
    }

    #[test]
    fn configured_threshold_changes_membership_without_changing_viewport() {
        assert!(!is_compact(1080.0, 14.4, 70));
        assert!(is_compact(1080.0, 14.4, 80));
    }
}
