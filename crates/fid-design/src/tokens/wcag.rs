//! WCAG 2.2 relative luminance and contrast ratio of opaque sRGB colours.
//!
//! This is the formula only. Pass/fail gating of a design-system release belongs to spec node
//! `UX.TOKENS.CHECK` (`fid-design color-check`); `tokens-check` never calls it. It is public so
//! that the provisional AC-5 evidence of `UX.TOKENS.COLOR` (the contrast of every declared pair in
//! every theme) can be computed in tests and compared with the independent Python oracle.
//!
//! Relative luminance `L = 0.2126 R + 0.7152 G + 0.0722 B`, where each channel `c = v / 255` is
//! linearised as `c / 12.92` when `c <= 0.04045` and `((c + 0.055) / 1.055)^2.4` otherwise. WCAG 2.0
//! and 2.1 printed the threshold as 0.03928; no 8-bit value lies between the two thresholds
//! (10/255 = 0.0392 and 11/255 = 0.0431), so both give identical results for `#rrggbb` input.
//! Contrast ratio `(L1 + 0.05) / (L2 + 0.05)` with `L1` the lighter luminance. Thresholds are
//! compared without rounding: 4.499:1 fails 4.5:1.

/// An opaque 8-bit sRGB colour.
pub type Rgb = [u8; 3];

/// Parses `#rrggbb` (lowercase or uppercase hex). Colours with alpha (`#rrggbbaa`) return
/// `None`: their contrast depends on what is behind them, which `UX.TOKENS.CHECK` composites.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// One linearised channel.
pub fn linear(v: u8) -> f64 {
    let c = f64::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance in `[0, 1]`.
pub fn relative_luminance(c: Rgb) -> f64 {
    0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2])
}

/// WCAG contrast ratio in `[1, 21]`; symmetric in its arguments.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// The minimum ratio a declared pair needs: `text` 4.5 and `large-text` 3 (SC 1.4.3),
/// `non-text` 3 (SC 1.4.11), and 7 for every text pair, large text included, in the `hc` theme
/// (REQ-UX.TOKENS-004 says "every text/background pair", stricter than SC 1.4.6's 4.5 for
/// large text). `None` for an unknown tag.
pub fn required_ratio(tag: &str, theme: &str) -> Option<f64> {
    let hc = theme == "hc";
    match tag {
        "text" => Some(if hc { 7.0 } else { 4.5 }),
        "large-text" => Some(if hc { 7.0 } else { 3.0 }),
        "non-text" => Some(3.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors() {
        let (black, white) = ([0, 0, 0], [255, 255, 255]);
        assert_eq!(relative_luminance(black), 0.0);
        assert!((relative_luminance(white) - 1.0).abs() < 1e-15);
        assert!((contrast_ratio(black, white) - 21.0).abs() < 1e-12);
        assert!((contrast_ratio(white, black) - 21.0).abs() < 1e-12);
        assert_eq!(contrast_ratio([12, 34, 56], [12, 34, 56]), 1.0);
    }

    #[test]
    fn old_and_new_thresholds_agree_on_8_bit_values() {
        for v in 0..=255u8 {
            let c = f64::from(v) / 255.0;
            assert_eq!(c <= 0.04045, c <= 0.03928, "{v}");
        }
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#0a0B0c"), Some([10, 11, 12]));
        for bad in ["0a0b0c", "#0a0b0", "#0a0b0c80", "#0g0b0c", "", "#", "#+a0b0c"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn thresholds() {
        assert_eq!(required_ratio("text", "light"), Some(4.5));
        assert_eq!(required_ratio("text", "hc"), Some(7.0));
        assert_eq!(required_ratio("large-text", "dark"), Some(3.0));
        assert_eq!(required_ratio("large-text", "hc"), Some(7.0));
        assert_eq!(required_ratio("non-text", "hc"), Some(3.0));
        assert_eq!(required_ratio("icon", "light"), None);
    }
}
