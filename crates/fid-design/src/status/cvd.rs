//! Colour-vision-deficiency simulation for verifying the locked status palette: the model of
//! Machado, Oliveira and Fernandes (2009) at severity 1.0 (full dichromacy), applied to linear
//! sRGB and clipped to `[0, 1]` per channel. Like [`super::ciede2000`], this is a formula only;
//! `UX.TOKENS.CHECK` owns the release gate and records the model version (REQ-UX.TOKENS-008).

use super::ciede2000::{delta_e2000, lab_of_linear};
use crate::tokens::wcag::{linear, Rgb};

/// The four vision conditions of REQ-UX.TOKENS.STATUS-004.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Vision {
    Normal,
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

impl Vision {
    pub const ALL: [Vision; 4] = [Vision::Normal, Vision::Protanopia, Vision::Deuteranopia, Vision::Tritanopia];

    pub fn as_str(self) -> &'static str {
        match self {
            Vision::Normal => "normal",
            Vision::Protanopia => "protanopia",
            Vision::Deuteranopia => "deuteranopia",
            Vision::Tritanopia => "tritanopia",
        }
    }

    pub fn parse(s: &str) -> Option<Vision> {
        Vision::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// The severity-1.0 matrix on linear sRGB (`None`: identity).
    pub fn matrix(self) -> Option<[[f64; 3]; 3]> {
        match self {
            Vision::Normal => None,
            Vision::Protanopia => Some([
                [0.152_286, 1.052_583, -0.204_868],
                [0.114_503, 0.786_281, 0.099_216],
                [-0.003_882, -0.048_116, 1.051_998],
            ]),
            Vision::Deuteranopia => Some([
                [0.367_322, 0.860_646, -0.227_968],
                [0.280_085, 0.672_501, 0.047_413],
                [-0.011_820, 0.042_940, 0.968_881],
            ]),
            Vision::Tritanopia => Some([
                [1.255_528, -0.076_749, -0.178_779],
                [-0.078_411, 0.930_809, 0.147_602],
                [0.004_733, 0.691_367, 0.303_900],
            ]),
        }
    }
}

/// Linear sRGB of `c` as seen under `v`, each channel clipped to `[0, 1]`.
pub fn simulate(v: Vision, c: Rgb) -> [f64; 3] {
    let lin = [linear(c[0]), linear(c[1]), linear(c[2])];
    let Some(m) = v.matrix() else {
        return lin;
    };
    let mut out = [0.0; 3];
    for (o, row) in out.iter_mut().zip(m.iter()) {
        let s = row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2];
        *o = s.clamp(0.0, 1.0);
    }
    out
}

/// CIEDE2000 between two opaque sRGB colours as seen under `v`.
pub fn delta_e2000_under(v: Vision, a: Rgb, b: Rgb) -> f64 {
    delta_e2000(lab_of_linear(simulate(v, a)), lab_of_linear(simulate(v, b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_keep_white_and_grey() {
        for v in Vision::ALL {
            if let Some(m) = v.matrix() {
                for row in m {
                    assert!((row[0] + row[1] + row[2] - 1.0).abs() < 2e-6, "{v:?}");
                }
            }
            let g = simulate(v, [128, 128, 128]);
            assert!((g[0] - g[1]).abs() < 1e-5 && (g[1] - g[2]).abs() < 1e-5, "{v:?} {g:?}");
            assert_eq!(Vision::parse(v.as_str()), Some(v));
        }
    }

    #[test]
    fn red_and_green_collapse_for_deuteranopia_only() {
        let (red, green) = ([200, 40, 40], [40, 140, 40]);
        let normal = delta_e2000_under(Vision::Normal, red, green);
        let deutan = delta_e2000_under(Vision::Deuteranopia, red, green);
        assert!(normal > 50.0 && deutan < normal / 2.0, "{normal} {deutan}");
    }
}
