//! Pinned Unicode 16.0 tables, copied verbatim from `crates/fid-bus/src/unicode16.rs`
//! (generated there by `fixtures/core/bus/principal/make_unicode_tables.py`). A test checks
//! that the copies still equal the fid-bus source text. Each table is sorted, inclusive and
//! non-overlapping. Copied rather than depended on, so the report model does not link the
//! command bus for two constant tables.

/// Code points of general category Cf (format), Unicode 16.0.
#[rustfmt::skip]
pub const FORMAT_RANGES: &[(u32, u32)] = &[
    (0x00AD, 0x00AD), (0x0600, 0x0605), (0x061C, 0x061C), (0x06DD, 0x06DD), (0x070F, 0x070F), (0x0890, 0x0891),
    (0x08E2, 0x08E2), (0x180E, 0x180E), (0x200B, 0x200F), (0x202A, 0x202E), (0x2060, 0x2064), (0x2066, 0x206F),
    (0xFEFF, 0xFEFF), (0xFFF9, 0xFFFB), (0x110BD, 0x110BD), (0x110CD, 0x110CD), (0x13430, 0x1343F), (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A), (0xE0001, 0xE0001), (0xE0020, 0xE007F),
];

/// Code points with the derived property Default_Ignorable_Code_Point, Unicode 16.0.
#[rustfmt::skip]
pub const DEFAULT_IGNORABLE_RANGES: &[(u32, u32)] = &[
    (0x00AD, 0x00AD), (0x034F, 0x034F), (0x061C, 0x061C), (0x115F, 0x1160), (0x17B4, 0x17B5), (0x180B, 0x180F),
    (0x200B, 0x200F), (0x202A, 0x202E), (0x2060, 0x206F), (0x3164, 0x3164), (0xFE00, 0xFE0F), (0xFEFF, 0xFEFF),
    (0xFFA0, 0xFFA0), (0xFFF0, 0xFFF8), (0x1BCA0, 0x1BCA3), (0x1D173, 0x1D17A), (0xE0000, 0xE0FFF),
];

/// Whether `c` lies in one of `ranges` (binary search over sorted inclusive ranges).
pub(crate) fn in_ranges(ranges: &[(u32, u32)], c: char) -> bool {
    let u = u32::from(c);
    ranges
        .binary_search_by(|&(lo, hi)| {
            if hi < u {
                std::cmp::Ordering::Less
            } else if lo > u {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}
