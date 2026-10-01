//! Colour-difference checks for colour-vision deficiency (#354 slice 4,
//! ADR-0216).
//!
//! Two pieces, both pure:
//! - [`simulate`]: Machado, Oliveira & Fernandes (2009) dichromat simulation
//!   at severity 1.0, applied in linear sRGB.
//! - [`delta_e2000`]: CIEDE2000 between two colours (sRGB → XYZ D65 → CIELAB).
//!
//! [`delta_e`] combines them: the perceived difference between two theme
//! colours for a viewer with the given deficiency (or typical vision).

/// A colour-vision deficiency to simulate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cvd {
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

impl Cvd {
    pub const ALL: [Cvd; 3] = [Cvd::Protanopia, Cvd::Deuteranopia, Cvd::Tritanopia];

    /// Machado 2009, severity 1.0, linear-RGB matrices.
    fn matrix(self) -> [[f64; 3]; 3] {
        match self {
            Cvd::Protanopia => [
                [0.152286, 1.052583, -0.204868],
                [0.114503, 0.786281, 0.099216],
                [-0.003882, -0.048116, 1.051998],
            ],
            Cvd::Deuteranopia => [
                [0.367322, 0.860646, -0.227968],
                [0.280085, 0.672501, 0.047413],
                [-0.011820, 0.042940, 0.968881],
            ],
            Cvd::Tritanopia => [
                [1.255528, -0.076749, -0.178779],
                [-0.078411, 0.930809, 0.147602],
                [0.004733, 0.691367, 0.303900],
            ],
        }
    }
}

fn to_linear(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_rgb(rgb: u32) -> [f64; 3] {
    [
        to_linear((rgb >> 16) as u8),
        to_linear((rgb >> 8) as u8),
        to_linear(rgb as u8),
    ]
}

/// Linear-RGB colour as seen with `cvd` (clamped to the gamut).
pub fn simulate(rgb: u32, cvd: Cvd) -> [f64; 3] {
    let l = linear_rgb(rgb);
    let m = cvd.matrix();
    let mut out = [0.0; 3];
    for (i, row) in m.iter().enumerate() {
        out[i] = (row[0] * l[0] + row[1] * l[1] + row[2] * l[2]).clamp(0.0, 1.0);
    }
    out
}

/// CIELAB (D65) of a linear-RGB colour.
fn lab(l: [f64; 3]) -> [f64; 3] {
    let x = (0.4124 * l[0] + 0.3576 * l[1] + 0.1805 * l[2]) / 0.95047;
    let y = 0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2];
    let z = (0.0193 * l[0] + 0.1192 * l[1] + 0.9505 * l[2]) / 1.08883;
    let f = |t: f64| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000 colour difference between two CIELAB colours.
pub fn delta_e2000(a: [f64; 3], b: [f64; 3]) -> f64 {
    let ([l1, a1, b1], [l2, a2, b2]) = (a, b);
    let pow7 = |v: f64| v.powi(7);
    let c1 = a1.hypot(b1);
    let c2 = a2.hypot(b2);
    let cm = (c1 + c2) / 2.0;
    let g = 0.5 * (1.0 - (pow7(cm) / (pow7(cm) + pow7(25.0))).sqrt());
    let a1p = (1.0 + g) * a1;
    let a2p = (1.0 + g) * a2;
    let c1p = a1p.hypot(b1);
    let c2p = a2p.hypot(b2);
    let hue = |b: f64, a: f64| {
        if a == 0.0 && b == 0.0 {
            0.0
        } else {
            b.atan2(a).to_degrees().rem_euclid(360.0)
        }
    };
    let h1 = hue(b1, a1p);
    let h2 = hue(b2, a2p);
    let dl = l2 - l1;
    let dc = c2p - c1p;
    let dh = if c1p * c2p == 0.0 {
        0.0
    } else if (h2 - h1).abs() <= 180.0 {
        h2 - h1
    } else if h2 - h1 > 180.0 {
        h2 - h1 - 360.0
    } else {
        h2 - h1 + 360.0
    };
    let dhh = 2.0 * (c1p * c2p).sqrt() * (dh.to_radians() / 2.0).sin();
    let lm = (l1 + l2) / 2.0;
    let cpm = (c1p + c2p) / 2.0;
    let hm = if c1p * c2p == 0.0 {
        h1 + h2
    } else if (h1 - h2).abs() <= 180.0 {
        (h1 + h2) / 2.0
    } else if h1 + h2 < 360.0 {
        (h1 + h2 + 360.0) / 2.0
    } else {
        (h1 + h2 - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * (hm - 30.0).to_radians().cos()
        + 0.24 * (2.0 * hm).to_radians().cos()
        + 0.32 * (3.0 * hm + 6.0).to_radians().cos()
        - 0.20 * (4.0 * hm - 63.0).to_radians().cos();
    let dtheta = 30.0 * (-((hm - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (pow7(cpm) / (pow7(cpm) + pow7(25.0))).sqrt();
    let sl = 1.0 + 0.015 * (lm - 50.0).powi(2) / (20.0 + (lm - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cpm;
    let sh = 1.0 + 0.015 * cpm * t;
    let rt = -(2.0 * dtheta).to_radians().sin() * rc;
    ((dl / sl).powi(2) + (dc / sc).powi(2) + (dhh / sh).powi(2) + rt * (dc / sc) * (dhh / sh))
        .sqrt()
}

/// Perceived CIEDE2000 difference between two `0xRRGGBB` colours, for typical
/// vision (`None`) or simulated `cvd`.
pub fn delta_e(a: u32, b: u32, cvd: Option<Cvd>) -> f64 {
    let (la, lb) = match cvd {
        Some(c) => (simulate(a, c), simulate(b, c)),
        None => (linear_rgb(a), linear_rgb(b)),
    };
    delta_e2000(lab(la), lab(lb))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sharma, Wu & Dalal (2005) reference pairs for CIEDE2000.
    #[test]
    fn ciede2000_matches_published_reference_pairs() {
        let cases = [
            ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
            ([50.0, 3.1571, -77.2803], [50.0, 0.0, -82.7485], 2.8615),
            ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
            (
                [60.2574, -34.0099, 36.2677],
                [60.4626, -34.1751, 39.4387],
                1.2644,
            ),
            (
                [22.7233, 20.0904, -46.6940],
                [23.0331, 14.9730, -42.5619],
                2.0373,
            ),
        ];
        for (a, b, want) in cases {
            let got = delta_e2000(a, b);
            assert!((got - want).abs() < 1e-3, "{a:?} vs {b:?}: {got} != {want}");
        }
    }

    #[test]
    fn identical_colours_differ_by_zero_and_grey_survives_simulation() {
        assert_eq!(delta_e(0x56b4e9, 0x56b4e9, None), 0.0);
        for cvd in Cvd::ALL {
            // A neutral grey is a fixed point of every Machado matrix (rows sum
            // to ~1), so its simulated colour stays near itself.
            assert!(delta_e(0x808080, 0x808080, Some(cvd)) < 1e-9);
            let g = simulate(0x808080, cvd);
            assert!((g[0] - g[1]).abs() < 0.01 && (g[1] - g[2]).abs() < 0.01);
        }
    }

    #[test]
    fn red_green_collapses_for_deuteranopia_not_for_typical_vision() {
        // The textbook confusion pair: obvious normally, close for deutans.
        let (red, green) = (0xd03030, 0x30a030);
        assert!(delta_e(red, green, None) > 40.0);
        assert!(delta_e(red, green, Some(Cvd::Deuteranopia)) < delta_e(red, green, None) / 2.0);
    }
}
