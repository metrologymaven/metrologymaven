//! CIELAB and the CIEDE2000 colour difference, used to verify the locked status palette
//! (REQ-UX.TOKENS.STATUS-004). This is the formula only: the release gate that applies it is
//! spec node `UX.TOKENS.CHECK`, and the runtime guard is `VIZ.OVERLAY.STATUS`; `status-check`
//! never calls it. It is public so that the provisional AC-4 evidence of `UX.TOKENS.STATUS` can be
//! computed in tests and compared with the independent Python oracle.
//!
//! - CIELAB: linear sRGB to CIE XYZ with the IEC 61966-2-1 matrix, then L*a*b* relative to the D65
//!   white point (0.95047, 1, 1.08883), 2 degree observer.
//! - CIEDE2000: Sharma, Wu and Dalal (2005) with `kL = kC = kH = 1`, hue angles in degrees, the
//!   mean hue and hue difference taken the short way round the circle.
//!
//! Plain multiplications and additions only (no fused multiply-add).

/// CIELAB `[L*, a*, b*]` of a linear sRGB triple (each channel in `[0, 1]`).
pub fn lab_of_linear(c: [f64; 3]) -> [f64; 3] {
    let (r, g, b) = (c[0], c[1], c[2]);
    let x = 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b;
    let z = 0.019_333_9 * r + 0.119_192_0 * g + 0.950_304_1 * b;
    let (fx, fy, fz) = (f(x / 0.950_47), f(y), f(z / 1.088_83));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn f(t: f64) -> f64 {
    let d = 6.0 / 29.0;
    if t > d * d * d {
        t.cbrt()
    } else {
        t / (3.0 * d * d) + 4.0 / 29.0
    }
}

fn hue_deg(b: f64, a: f64) -> f64 {
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

/// CIEDE2000 colour difference of two CIELAB colours.
pub fn delta_e2000(p: [f64; 3], q: [f64; 3]) -> f64 {
    let (l1, a1, b1) = (p[0], p[1], p[2]);
    let (l2, a2, b2) = (q[0], q[1], q[2]);
    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let k25 = pow7(25.0);
    let cbar7 = pow7((c1 + c2) / 2.0);
    let g = 0.5 * (1.0 - (cbar7 / (cbar7 + k25)).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let c1p = (a1p * a1p + b1 * b1).sqrt();
    let c2p = (a2p * a2p + b2 * b2).sqrt();
    let (h1p, h2p) = (hue_deg(b1, a1p), hue_deg(b2, a2p));

    let dl = l2 - l1;
    let dc = c2p - c1p;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// All 34 test pairs of the table published with Sharma, Wu and Dalal (2005): L*, a*, b* of
    /// each colour and the expected difference, to four decimals.
    #[test]
    fn sharma_pairs() {
        let cases: [([f64; 3], [f64; 3], f64); 34] = [
            ([50.0000, 2.6772, -79.7751], [50.0000, 0.0000, -82.7485], 2.0425),
            ([50.0000, 3.1571, -77.2803], [50.0000, 0.0000, -82.7485], 2.8615),
            ([50.0000, 2.8361, -74.0200], [50.0000, 0.0000, -82.7485], 3.4412),
            ([50.0000, -1.3802, -84.2814], [50.0000, 0.0000, -82.7485], 1.0000),
            ([50.0000, -1.1848, -84.8006], [50.0000, 0.0000, -82.7485], 1.0000),
            ([50.0000, -0.9009, -85.5211], [50.0000, 0.0000, -82.7485], 1.0000),
            ([50.0000, 0.0000, 0.0000], [50.0000, -1.0000, 2.0000], 2.3669),
            ([50.0000, -1.0000, 2.0000], [50.0000, 0.0000, 0.0000], 2.3669),
            ([50.0000, 2.4900, -0.0010], [50.0000, -2.4900, 0.0009], 7.1792),
            ([50.0000, 2.4900, -0.0010], [50.0000, -2.4900, 0.0010], 7.1792),
            ([50.0000, 2.4900, -0.0010], [50.0000, -2.4900, 0.0011], 7.2195),
            ([50.0000, 2.4900, -0.0010], [50.0000, -2.4900, 0.0012], 7.2195),
            ([50.0000, -0.0010, 2.4900], [50.0000, 0.0009, -2.4900], 4.8045),
            ([50.0000, -0.0010, 2.4900], [50.0000, 0.0010, -2.4900], 4.8045),
            ([50.0000, -0.0010, 2.4900], [50.0000, 0.0011, -2.4900], 4.7461),
            ([50.0000, 2.5000, 0.0000], [50.0000, 0.0000, -2.5000], 4.3065),
            ([50.0000, 2.5000, 0.0000], [73.0000, 25.0000, -18.0000], 27.1492),
            ([50.0000, 2.5000, 0.0000], [61.0000, -5.0000, 29.0000], 22.8977),
            ([50.0000, 2.5000, 0.0000], [56.0000, -27.0000, -3.0000], 31.9030),
            ([50.0000, 2.5000, 0.0000], [58.0000, 24.0000, 15.0000], 19.4535),
            ([50.0000, 2.5000, 0.0000], [50.0000, 3.1736, 0.5854], 1.0000),
            ([50.0000, 2.5000, 0.0000], [50.0000, 3.2972, 0.0000], 1.0000),
            ([50.0000, 2.5000, 0.0000], [50.0000, 1.8634, 0.5757], 1.0000),
            ([50.0000, 2.5000, 0.0000], [50.0000, 3.2592, 0.3350], 1.0000),
            ([60.2574, -34.0099, 36.2677], [60.4626, -34.1751, 39.4387], 1.2644),
            ([63.0109, -31.0961, -5.8663], [62.8187, -29.7946, -4.0864], 1.2630),
            ([61.2901, 3.7196, -5.3901], [61.4292, 2.2480, -4.9620], 1.8731),
            ([35.0831, -44.1164, 3.7933], [35.0232, -40.0716, 1.5901], 1.8645),
            ([22.7233, 20.0904, -46.6940], [23.0331, 14.9730, -42.5619], 2.0373),
            ([36.4612, 47.8580, 18.3852], [36.2715, 50.5065, 21.2231], 1.4146),
            ([90.8027, -2.0831, 1.4410], [91.1528, -1.6435, 0.0447], 1.4441),
            ([90.9257, -0.5406, -0.9208], [88.6381, -0.8985, -0.7239], 1.5381),
            ([6.7747, -0.2908, -2.4247], [5.8714, -0.0985, -2.2286], 0.6377),
            ([2.0776, 0.0795, -1.1350], [0.9033, -0.0636, -0.5514], 0.9082),
        ];
        for (a, b, want) in cases {
            assert!((delta_e2000(a, b) - want).abs() < 1e-4, "{a:?} {b:?}: {}", delta_e2000(a, b));
            assert!((delta_e2000(b, a) - want).abs() < 1e-4);
        }
        assert_eq!(delta_e2000([40.0, 10.0, -5.0], [40.0, 10.0, -5.0]), 0.0);
    }

    #[test]
    fn lab_anchors() {
        let w = lab_of_linear([1.0, 1.0, 1.0]);
        assert!((w[0] - 100.0).abs() < 1e-3 && w[1].abs() < 1e-2 && w[2].abs() < 1e-2, "{w:?}");
        assert!(lab_of_linear([0.0, 0.0, 0.0]).iter().all(|x| x.abs() < 1e-12));
    }
}
