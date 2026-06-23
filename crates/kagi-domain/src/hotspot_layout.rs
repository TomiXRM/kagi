//! Treemap layout for the Ecosystem "Map" heatmap — pure Rust, no deps
//! (ADR-0119).
//!
//! A **binary-split** treemap: recursively split the (descending-sorted) weight
//! list into two halves of roughly equal total, splitting the longer side of
//! the current rectangle each time. Simpler and more robust than squarified
//! treemaps while still producing reasonable aspect ratios, and the tile area
//! is **exactly** proportional to the weight. Output rectangles are normalized
//! to the unit square `[0,1]²`, so the UI can place them with `relative()`
//! lengths (no pixel measurement, labels & hit-testing stay as plain elements).

/// A normalized rectangle in `[0,1]²` (`x,y` = top-left, `w,h` = size).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    const UNIT: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };
    const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    pub fn area(&self) -> f64 {
        self.w * self.h
    }
}

/// Lay `weights` out as a treemap over the unit square. The returned vector is
/// index-aligned with `weights` (`out[i]` ↔ `weights[i]`); non-positive weights
/// get a [`Rect::ZERO`]. Tile areas are proportional to the weights.
pub fn treemap(weights: &[f64]) -> Vec<Rect> {
    let mut out = vec![Rect::ZERO; weights.len()];
    let mut items: Vec<(usize, f64)> = weights
        .iter()
        .enumerate()
        .filter(|(_, &w)| w > 0.0)
        .map(|(i, &w)| (i, w))
        .collect();
    if items.is_empty() {
        return out;
    }
    // Descending weight → larger tiles placed first gives squarer rectangles.
    items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    layout(&items, Rect::UNIT, &mut out);
    out
}

fn layout(items: &[(usize, f64)], rect: Rect, out: &mut [Rect]) {
    match items {
        [] => {}
        [(i, _)] => out[*i] = rect,
        _ => {
            let total: f64 = items.iter().map(|x| x.1).sum();
            // Smallest prefix whose sum reaches half the total (≥1, ≤len-1).
            let mut acc = 0.0;
            let mut mid = 0;
            for (k, it) in items.iter().enumerate() {
                acc += it.1;
                mid = k + 1;
                if acc >= total / 2.0 {
                    break;
                }
            }
            let mid = mid.clamp(1, items.len() - 1);
            let left_sum: f64 = items[..mid].iter().map(|x| x.1).sum();
            let frac = left_sum / total;

            // Split the longer side so tiles stay closer to square.
            let (lr, rr) = if rect.w >= rect.h {
                (
                    Rect {
                        w: rect.w * frac,
                        ..rect
                    },
                    Rect {
                        x: rect.x + rect.w * frac,
                        w: rect.w * (1.0 - frac),
                        ..rect
                    },
                )
            } else {
                (
                    Rect {
                        h: rect.h * frac,
                        ..rect
                    },
                    Rect {
                        y: rect.y + rect.h * frac,
                        h: rect.h * (1.0 - frac),
                        ..rect
                    },
                )
            };
            layout(&items[..mid], lr, out);
            layout(&items[mid..], rr, out);
        }
    }
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn within_unit(r: &Rect) -> bool {
        r.x >= -1e-9 && r.y >= -1e-9 && r.x + r.w <= 1.0 + 1e-9 && r.y + r.h <= 1.0 + 1e-9
    }

    #[test]
    fn empty_and_zero_weights() {
        assert!(treemap(&[]).is_empty());
        let out = treemap(&[0.0, 0.0]);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| r.area() == 0.0));
    }

    #[test]
    fn single_fills_unit_square() {
        let out = treemap(&[5.0]);
        assert_eq!(out[0], Rect::UNIT);
    }

    #[test]
    fn areas_are_proportional_and_within_bounds() {
        let weights = [10.0, 5.0, 5.0, 2.0, 1.0, 1.0, 8.0];
        let total: f64 = weights.iter().sum();
        let out = treemap(&weights);
        let covered: f64 = out.iter().map(|r| r.area()).sum();
        assert!((covered - 1.0).abs() < 1e-9, "tiles should tile the square");
        for (i, r) in out.iter().enumerate() {
            assert!(within_unit(r), "rect {i} out of bounds: {r:?}");
            assert!(
                (r.area() - weights[i] / total).abs() < 1e-9,
                "rect {i} area not proportional"
            );
        }
    }

    #[test]
    fn index_alignment_is_preserved() {
        // Largest weight is at index 2; its tile must be the biggest.
        let out = treemap(&[1.0, 2.0, 9.0, 3.0]);
        let biggest = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.area().partial_cmp(&b.1.area()).unwrap())
            .unwrap()
            .0;
        assert_eq!(biggest, 2);
    }
}
