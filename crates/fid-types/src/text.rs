//! Code points untrusted text shown to people or agents may not hold.
//!
//! One table for the whole workspace: `CORE.BUS.POL.PROJ` refuses them in rejection reasons
//! (#328, re-exported there as `REASON_FORBIDDEN_RANGES`), `IO.REG.ENV.IMPORT` refuses them in
//! imported element names. A control, bidi or other invisible character could hide or reorder
//! what a person or an agent reads.

/// Code point ranges (inclusive) forbidden in untrusted display text (Unicode 16.0), with what
/// each range is: general categories Cc, Cf, Zl and Zp, the noncharacters U+FDD0..U+FDEF, and
/// the whole tag block U+E0000..U+E007F. The plane-end noncharacters U+xFFFE and U+xFFFF of
/// every plane are refused by [`is_forbidden_char`] as well.
#[rustfmt::skip]
pub const FORBIDDEN_RANGES: &[(u32, u32, &str)] = &[
    (0x0000, 0x001F, "Cc: C0 controls"),
    (0x007F, 0x009F, "Cc: DEL and C1 controls"),
    (0x00AD, 0x00AD, "Cf: soft hyphen"),
    (0x0600, 0x0605, "Cf: Arabic number signs"),
    (0x061C, 0x061C, "Cf: Arabic letter mark"),
    (0x06DD, 0x06DD, "Cf: Arabic end of ayah"),
    (0x070F, 0x070F, "Cf: Syriac abbreviation mark"),
    (0x0890, 0x0891, "Cf: Arabic pound and piastre marks above"),
    (0x08E2, 0x08E2, "Cf: Arabic disputed end of ayah"),
    (0x180E, 0x180E, "Cf: Mongolian vowel separator"),
    (0x200B, 0x200F, "Cf: zero width space, joiners, LRM, RLM"),
    (0x2028, 0x2028, "Zl: line separator"),
    (0x2029, 0x2029, "Zp: paragraph separator"),
    (0x202A, 0x202E, "Cf: bidi embeddings and overrides"),
    (0x2060, 0x2064, "Cf: word joiner and invisible operators"),
    (0x2066, 0x206F, "Cf: bidi isolates and deprecated format controls"),
    (0xFDD0, 0xFDEF, "noncharacters"),
    (0xFEFF, 0xFEFF, "Cf: byte order mark"),
    (0xFFF9, 0xFFFB, "Cf: interlinear annotation controls"),
    (0x110BD, 0x110BD, "Cf: Kaithi number sign"),
    (0x110CD, 0x110CD, "Cf: Kaithi number sign above"),
    (0x13430, 0x1343F, "Cf: Egyptian hieroglyph format controls"),
    (0x1BCA0, 0x1BCA3, "Cf: shorthand format controls"),
    (0x1D173, 0x1D17A, "Cf: musical symbol format controls"),
    (0xE0000, 0xE007F, "Cf and reserved: tag characters"),
];

/// Whether `c` is forbidden: any [`FORBIDDEN_RANGES`] code point or a plane-end
/// noncharacter (U+xFFFE, U+xFFFF).
pub fn is_forbidden_char(c: char) -> bool {
    let u = u32::from(c);
    (u & 0xFFFE) == 0xFFFE || FORBIDDEN_RANGES.iter().any(|(lo, hi, _)| (*lo..=*hi).contains(&u))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_sorted_and_disjoint() {
        for w in FORBIDDEN_RANGES.windows(2) {
            assert!(w[0].0 <= w[0].1 && w[0].1 < w[1].0, "{} / {}", w[0].2, w[1].2);
        }
    }

    #[test]
    fn edges() {
        for (lo, hi, _) in FORBIDDEN_RANGES {
            for u in [*lo, *hi] {
                assert!(char::from_u32(u).is_some_and(is_forbidden_char), "U+{u:04X}");
            }
        }
        for c in ['a', ' ', '\u{00E9}', '\u{4E2D}', '\u{1F600}', '\u{FE0F}'] {
            assert!(!is_forbidden_char(c), "{c:?}");
        }
        assert!(is_forbidden_char('\u{FFFE}') && is_forbidden_char('\u{10FFFF}'));
    }
}
