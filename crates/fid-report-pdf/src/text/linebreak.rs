//! Line-break opportunities: a conservative subset of UAX #14.
//!
//! Rules, applied between characters `a` (before) and `b` (after); a break opportunity is
//! the byte offset of `b`:
//!
//! 1. never at the start or the end of the string;
//! 2. after a run of spaces (U+0020) or tabs, before the next character that is neither;
//! 3. after a hyphen (U+002D, U+2010) or dash (U+2013, U+2014) or soft hyphen (U+00AD) that
//!    stands between two letters (`a` is the dash and both of its neighbours are
//!    alphabetic), so a sign (`-0,050`) or a range of numbers is never broken;
//! 4. between two CJK ideographs or kana (U+3040..U+30FF, U+3400..U+4DBF, U+4E00..U+9FFF,
//!    U+F900..U+FAFF), unless `b` is one of the closing marks U+3001, U+3002, U+30FC.
//!
//! No break is ever offered at a no-break space (U+00A0, U+202F) or a thin space (U+2009),
//! which digit grouping uses, so a formatted value is never split.

use super::error::TextError;
use super::MAX_TEXT_BYTES;

fn is_space(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn is_dash(c: char) -> bool {
    matches!(c, '-' | '\u{2010}' | '\u{2013}' | '\u{2014}' | '\u{00AD}')
}

fn is_cjk(c: char) -> bool {
    matches!(u32::from(c), 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

/// Byte offsets at which a line may break (before the character at that offset), ascending.
/// Input longer than [`MAX_TEXT_BYTES`] fails with `report.text_limit`, as shaping does.
pub fn break_opportunities(text: &str) -> Result<Vec<usize>, TextError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(TextError::Limit { what: "text longer than MAX_TEXT_BYTES" });
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    for i in 1..chars.len() {
        let (pos, b) = chars[i];
        let a = chars[i - 1].1;
        let rule2 = is_space(a) && !is_space(b);
        let rule3 = is_dash(a) && i >= 2 && chars[i - 2].1.is_alphabetic() && b.is_alphabetic();
        let rule4 = is_cjk(a) && is_cjk(b) && !matches!(b, '\u{3001}' | '\u{3002}' | '\u{30FC}');
        if rule2 || rule3 || rule4 {
            out.push(pos);
        }
    }
    Ok(out)
}
