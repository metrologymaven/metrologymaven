//! Code points no MCP tool text may hold.
//!
//! Tool names, titles, descriptions and schemas are shown to agents, so a control, bidi or
//! invisible character in them could hide or reorder what an agent reads. Generation refuses
//! any such character (`forbidden_character`), it never strips one: the registry text is
//! fixed, so a bad character is a declaration bug to fix at its source.
//!
//! [`FORBIDDEN_RANGES`] is the table `CORE.BUS.POL.PROJ` uses for rejection reasons
//! (`fid_bus::policy::projection::REASON_FORBIDDEN_RANGES`, Unicode 16.0): general
//! categories Cc, Cf, Zl and Zp, the noncharacters U+FDD0..U+FDEF and the tag block. The
//! plane-end noncharacters U+xFFFE and U+xFFFF are refused as well. This crate may not link
//! the bus, so the table is repeated here; a test fails if the two ever differ.

/// Code point ranges (inclusive) a tool text may not hold, with what each range is.
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

/// Whether a tool text may not hold `c`: any [`FORBIDDEN_RANGES`] code point or a plane-end
/// noncharacter (U+xFFFE, U+xFFFF).
pub fn forbidden(c: char) -> bool {
    let u = u32::from(c);
    (u & 0xFFFE) == 0xFFFE || FORBIDDEN_RANGES.iter().any(|(lo, hi, _)| (*lo..=*hi).contains(&u))
}

/// The first forbidden character of `s`, if any.
pub fn first_forbidden(s: &str) -> Option<char> {
    s.chars().find(|c| forbidden(*c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_disjoint() {
        for w in FORBIDDEN_RANGES.windows(2) {
            assert!(w[0].0 <= w[0].1 && w[0].1 < w[1].0, "{:x?}", w);
        }
    }

    #[test]
    fn examples() {
        for c in ['\n', '\t', '\u{7f}', '\u{200e}', '\u{202e}', '\u{2066}', '\u{feff}', '\u{e0041}', '\u{fffe}'] {
            assert!(forbidden(c), "{:x}", u32::from(c));
        }
        assert!(forbidden(char::from_u32(0x1FFFF).unwrap_or('a')));
        for c in ['a', ' ', 'µ', 'é', '\u{2265}', '\u{1f600}'] {
            assert!(!forbidden(c), "{:x}", u32::from(c));
        }
        assert_eq!(first_forbidden("ok \u{202e}x"), Some('\u{202e}'));
        assert_eq!(first_forbidden("plain text"), None);
    }
}
