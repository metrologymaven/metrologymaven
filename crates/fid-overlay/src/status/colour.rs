//! Colour difference for the palette guard: sRGB to linear (IEC 61966-2-1) to CIE XYZ (D65) to
//! CIELAB, CIEDE2000 (Sharma, Wu and Dalal, 2005, `kL = kC = kH = 1`), and colour-vision
//! deficiency simulated with the matrices of Machado, Oliveira and Fernandes (2009) at severity
//! 1.0 on linear sRGB, clipped to `[0, 1]`. The same model and formula as the release gate of
//! `UX.TOKENS.CHECK`, which lives in a tool-layer crate the viewer cannot depend on.
//!
//! Plain multiplications and additions only (no fused multiply-add).

/// An opaque 8-bit sRGB colour.
pub type Rgb = [u8; 3];

/// Parses lowercase `#rrggbb`.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// Lowercase `#rrggbb`.
pub fn to_hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// The four vision conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

    fn matrix(self) -> Option<[[f64; 3]; 3]> {
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

fn linear(v: u8) -> f64 {
    let c = f64::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn seen(v: Vision, c: Rgb) -> [f64; 3] {
    let lin = [linear(c[0]), linear(c[1]), linear(c[2])];
    let Some(m) = v.matrix() else {
        return lin;
    };
    let mut out = [0.0; 3];
    for (o, row) in out.iter_mut().zip(m.iter()) {
        *o = (row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2]).clamp(0.0, 1.0);
    }
    out
}

fn lab(c: [f64; 3]) -> [f64; 3] {
    let (r, g, b) = (c[0], c[1], c[2]);
    let x = 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b;
    let z = 0.019_333_9 * r + 0.119_192_0 * g + 0.950_304_1 * b;
    let f = |t: f64| {
        let d = 6.0 / 29.0;
        if t > d * d * d {
            t.cbrt()
        } else {
            t / (3.0 * d * d) + 4.0 / 29.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.950_47), f(y), f(z / 1.088_83));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn hue(b: f64, a: f64) -> f64 {
    if a == 0.0 && b == 0.0 {
        0.0
    } else {
        b.atan2(a).to_degrees().rem_euclid(360.0)
    }
}

fn pow7(x: f64) -> f64 {
    let x2 = x * x;
    x2 * x2 * x2 * x
}

/// CIEDE2000 between two CIELAB colours.
pub fn delta_e2000(p: [f64; 3], q: [f64; 3]) -> f64 {
    let (l1, a1, b1) = (p[0], p[1], p[2]);
    let (l2, a2, b2) = (q[0], q[1], q[2]);
    let k25 = pow7(25.0);
    let cbar7 = pow7(((a1 * a1 + b1 * b1).sqrt() + (a2 * a2 + b2 * b2).sqrt()) / 2.0);
    let g = 0.5 * (1.0 - (cbar7 / (cbar7 + k25)).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = ((a1p * a1p + b1 * b1).sqrt(), (a2p * a2p + b2 * b2).sqrt());
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let zero = c1p * c2p == 0.0;
    let dh = if zero {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dl = l2 - l1;
    let dc = c2p - c1p;
    let d_big_h = 2.0 * (c1p * c2p).sqrt() * (dh / 2.0).to_radians().sin();
    let lbar = (l1 + l2) / 2.0;
    let cbarp = (c1p + c2p) / 2.0;
    let hbar = if zero {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let cos = |deg: f64| deg.to_radians().cos();
    let t = 1.0 - 0.17 * cos(hbar - 30.0) + 0.24 * cos(2.0 * hbar) + 0.32 * cos(3.0 * hbar + 6.0)
        - 0.20 * cos(4.0 * hbar - 63.0);
    let e = (hbar - 275.0) / 25.0;
    let dtheta = 30.0 * (-(e * e)).exp();
    let cbarp7 = pow7(cbarp);
    let rc = 2.0 * (cbarp7 / (cbarp7 + k25)).sqrt();
    let l50 = (lbar - 50.0) * (lbar - 50.0);
    let sl = 1.0 + 0.015 * l50 / (20.0 + l50).sqrt();
    let sc = 1.0 + 0.045 * cbarp;
    let sh = 1.0 + 0.015 * cbarp * t;
    let rt = -(2.0 * dtheta).to_radians().sin() * rc;
    let (x, y, z) = (dl / sl, dc / sc, d_big_h / sh);
    (x * x + y * y + z * z + rt * y * z).sqrt()
}

/// CIEDE2000 between two sRGB colours as seen under `v`.
pub fn delta_e2000_under(v: Vision, a: Rgb, b: Rgb) -> f64 {
    delta_e2000(lab(seen(v, a)), lab(seen(v, b)))
}

/// The smallest CIEDE2000 over the four vision conditions, and the condition that gives it.
pub fn min_delta_e2000(a: Rgb, b: Rgb) -> (f64, Vision) {
    let mut worst = (f64::INFINITY, Vision::Normal);
    for v in Vision::ALL {
        let d = delta_e2000_under(v, a, b);
        if d < worst.0 {
            worst = (d, v);
        }
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A selection of the Sharma, Wu and Dalal (2005) test pairs.
    #[test]
    fn sharma_pairs() {
        let cases: [([f64; 3], [f64; 3], f64); 8] = [
            ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
            ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0011], 7.2195),
            ([50.0, -0.001, 2.49], [50.0, 0.0011, -2.49], 4.7461),
            ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
            ([50.0, 2.5, 0.0], [58.0, 24.0, 15.0], 19.4535),
            ([60.2574, -34.0099, 36.2677], [60.4626, -34.1751, 39.4387], 1.2644),
            ([22.7233, 20.0904, -46.694], [23.0331, 14.973, -42.5619], 2.0373),
            ([2.0776, 0.0795, -1.135], [0.9033, -0.0636, -0.5514], 0.9082),
        ];
        for (a, b, want) in cases {
            assert!((delta_e2000(a, b) - want).abs() < 1e-4, "{a:?} {b:?}");
            assert!((delta_e2000(b, a) - want).abs() < 1e-4);
        }
    }

    #[test]
    fn hex() {
        assert_eq!(parse_hex("#00637e"), Some([0, 0x63, 0x7e]));
        assert_eq!(parse_hex("#00637E"), None);
        assert_eq!(parse_hex("00637e"), None);
        assert_eq!(to_hex([0, 0x63, 0x7e]), "#00637e");
    }
}
