//! Full Unicode case folding of package paths, from the pinned table in `casefold_table.rs`
//! (statuses C and F of CaseFolding.txt, Unicode version [`UNICODE_VERSION`]).
//!
//! Two paths collide on a case-insensitive file system when their foldings are equal. Full
//! folding (unlike `str::to_lowercase`) maps `ς` and `σ` together, `ſ` to `s` and `ß` to `ss`,
//! and the pinned table keeps the result independent of the Rust toolchain's Unicode version.

use crate::casefold_table::FOLD;
pub use crate::casefold_table::UNICODE_VERSION;

/// The full case folding of `s`. Its UTF-8 length is at most three times that of `s`.
pub(crate) fn casefold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match FOLD.binary_search_by(|(k, _)| k.cmp(&c)) {
            Ok(i) => out.push_str(FOLD.get(i).map_or("", |(_, f)| f)),
            Err(_) => out.push(c),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_pinned() {
        assert!(FOLD.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(UNICODE_VERSION, "16.0.0");
        for (c, f) in FOLD.iter() {
            let mut buf = [0u8; 4];
            assert!(f.len() <= 3 * c.encode_utf8(&mut buf).len(), "{c:?}");
        }
    }

    #[test]
    fn full_folding() {
        assert_eq!(casefold("ABC"), "abc");
        assert_eq!(casefold("\u{3a3}\u{3c3}\u{3c2}"), "\u{3c3}\u{3c3}\u{3c3}");
        assert_eq!(casefold("\u{17f}"), "s");
        assert_eq!(casefold("Stra\u{df}e"), "strasse");
        assert_eq!(casefold("\u{212a}"), "k");
        assert_eq!(casefold("\u{130}"), "i\u{307}");
        assert_eq!(casefold("caf\u{e9}/\u{65e5}"), "caf\u{e9}/\u{65e5}");
    }
}
