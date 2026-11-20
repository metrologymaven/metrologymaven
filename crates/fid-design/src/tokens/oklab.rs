//! OKLab coordinates of opaque sRGB colours and their Euclidean distance (`ΔE_OK`), used by the
//! claimed-versus-measured distance floor of `tokens-check`.
//!
//! OKLab is Bjorn Ottosson's published colour space (2020). sRGB channels are linearised with
//! the IEC 61966-2-1 transfer function (threshold 0.04045), mapped to LMS, cube-rooted, then
//! mapped to `L, a, b` with the published matrices. `ΔE_OK = sqrt(ΔL² + Δa² + Δb²)`. Plain
//! multiplications and additions only (no fused multiply-add).

use super::wcag::{linear, Rgb};

/// `[L, a, b]` of an opaque sRGB colour.
pub fn oklab(c: Rgb) -> [f64; 3] {
    let (r, g, b) = (linear(c[0]), linear(c[1]), linear(c[2]));
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// Euclidean distance in OKLab.
pub fn delta_e_ok(a: Rgb, b: Rgb) -> f64 {
    let (p, q) = (oklab(a), oklab(b));
    let (dl, da, db) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
    (dl * dl + da * da + db * db).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors() {
        let w = oklab([255, 255, 255]);
        assert!((w[0] - 1.0).abs() < 1e-6 && w[1].abs() < 1e-6 && w[2].abs() < 1e-6, "{w:?}");
        let k = oklab([0, 0, 0]);
        assert!(k.iter().all(|x| x.abs() < 1e-12), "{k:?}");
        assert!((delta_e_ok([0, 0, 0], [255, 255, 255]) - 1.0).abs() < 1e-6);
        assert!(delta_e_ok([10, 20, 30], [10, 20, 30]) < 1e-15);
        // Pure sRGB red, from the published OKLab reference values (L 0.6280, a 0.2249, b 0.1258).
        let r = oklab([255, 0, 0]);
        assert!((r[0] - 0.6280).abs() < 1e-3 && (r[1] - 0.2249).abs() < 1e-3 && (r[2] - 0.1258).abs() < 1e-3, "{r:?}");
    }
}
