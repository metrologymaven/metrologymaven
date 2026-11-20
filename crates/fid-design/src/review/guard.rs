//! Fail-closed checks on untrusted text, relative paths, globs and dates used by `review-check`.
//!
//! Every string read from a record, a declaration, the rule slot, the path map, the changed-path
//! list or the deny list passes [`defects`]. Text is refused (never silently cleaned) when it
//! holds a control character, a bidirectional control, an invisible or formatting character, a
//! compatibility form, a word mixing Latin with Cyrillic or Greek letters, or is not in Unicode
//! NFC. The reasons never quote the text itself, so a refused deny-list entry is not echoed.

use fid_canon::CValue;
use unicode_normalization::is_nfc;

/// Bidirectional controls: ALM, LRM, RLM, the embeddings and overrides, and the isolates.
pub fn is_bidi(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Invisible characters not covered by the `words` check: Mongolian vowel separator, Hangul
/// fillers, variation selectors and tag characters.
pub fn is_extra_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{115F}'
            | '\u{1160}'
            | '\u{180E}'
            | '\u{3164}'
            | '\u{FFA0}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{E0000}'..='\u{E007F}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

/// The reasons a piece of untrusted text is refused, sorted and unique; empty when acceptable.
/// The empty string has no defect here; callers that need content check for it themselves.
pub fn defects(s: &str) -> Vec<&'static str> {
    let mut v = Vec::new();
    if s.chars().any(is_bidi) {
        v.push("bidirectional control character");
    }
    if s.chars().any(char::is_control) {
        v.push("control character");
    }
    if s.chars().any(is_extra_invisible) || !crate::words::obfuscation(s).is_empty() {
        v.push("invisible character, compatibility form or mixed-script word");
    }
    if !is_nfc(s) {
        v.push("not in Unicode NFC");
    }
    v
}

/// Checks every key and string of a parsed JSON document; returns `"<pointer>: <reasons>"` for
/// the first defective string, in document order.
pub fn value_defect(v: &CValue) -> Option<String> {
    fn walk(v: &CValue, at: &str) -> Option<String> {
        match v {
            CValue::Str(s) => {
                let d = defects(s);
                (!d.is_empty()).then(|| format!("{}: {}", if at.is_empty() { "/" } else { at }, d.join(", ")))
            }
            CValue::Arr(a) => a.iter().enumerate().find_map(|(i, x)| walk(x, &format!("{at}/{i}"))),
            CValue::Obj(m) => m.iter().find_map(|(k, x)| {
                let d = defects(k);
                if d.is_empty() {
                    walk(x, &format!("{at}/{}", k.replace('~', "~0").replace('/', "~1")))
                } else {
                    Some(format!("{} (a key): {}", if at.is_empty() { "/" } else { at }, d.join(", ")))
                }
            }),
            _ => None,
        }
    }
    walk(v, "")
}

/// Largest relative path accepted, in bytes.
pub const MAX_PATH_BYTES: usize = 1024;
/// Most segments in a path or glob.
pub const MAX_SEGMENTS: usize = 64;

/// Why a relative path is refused, or `None` when it is a clean path below the design root:
/// `/`-separated, no empty, `.` or `..` segment, no leading `/`, no backslash, colon, glob
/// metacharacter or control character, and free of the [`defects`] above.
pub fn path_defect(p: &str) -> Option<String> {
    if p.is_empty() {
        return Some("empty path".to_string());
    }
    if p.len() > MAX_PATH_BYTES {
        return Some(format!("path longer than {MAX_PATH_BYTES} bytes"));
    }
    let d = defects(p);
    if !d.is_empty() {
        return Some(d.join(", "));
    }
    if p.contains(['\\', ':', '*', '?', '[', ']', '{', '}']) {
        return Some("path holds a backslash, colon or glob character".to_string());
    }
    let segs: Vec<&str> = p.split('/').collect();
    if segs.len() > MAX_SEGMENTS {
        return Some(format!("path has more than {MAX_SEGMENTS} segments"));
    }
    if segs.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return Some("path is absolute or has an empty, `.` or `..` segment".to_string());
    }
    None
}

/// Why a glob of the review path map is refused, or `None`. Globs use `/`-separated segments;
/// `*` and `?` match within a segment and a segment that is exactly `**` matches zero or more
/// segments. Character classes and braces are not supported and are refused.
pub fn glob_defect(g: &str) -> Option<String> {
    if g.is_empty() || g.len() > MAX_PATH_BYTES {
        return Some("glob is empty or too long".to_string());
    }
    let d = defects(g);
    if !d.is_empty() {
        return Some(d.join(", "));
    }
    if g.contains(['\\', ':', '[', ']', '{', '}']) {
        return Some("glob holds a backslash, colon, character class or brace".to_string());
    }
    let segs: Vec<&str> = g.split('/').collect();
    if segs.len() > MAX_SEGMENTS {
        return Some(format!("glob has more than {MAX_SEGMENTS} segments"));
    }
    if segs.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return Some("glob is absolute or has an empty, `.` or `..` segment".to_string());
    }
    if segs.iter().any(|s| s.contains("**") && *s != "**") {
        return Some("`**` must be a whole segment".to_string());
    }
    None
}

/// Matches one segment pattern (`*`, `?`, literals) against one path segment.
fn seg_match(pat: &str, seg: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let s: Vec<char> = seg.chars().collect();
    // ok[j]: p[i..] matches s[j..], computed from the end.
    let mut next = vec![false; s.len() + 1];
    next[s.len()] = true;
    for i in (0..p.len()).rev() {
        let mut cur = vec![false; s.len() + 1];
        for j in (0..=s.len()).rev() {
            cur[j] = match p[i] {
                '*' => next[j] || (j < s.len() && cur[j + 1]),
                '?' => j < s.len() && next[j + 1],
                c => j < s.len() && s[j] == c && next[j + 1],
            };
        }
        next = cur;
    }
    next[0]
}

/// Whether a (valid) glob matches a (valid) relative path.
pub fn glob_match(glob: &str, path: &str) -> bool {
    let p: Vec<&str> = glob.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    let mut next = vec![false; s.len() + 1];
    next[s.len()] = true;
    for i in (0..p.len()).rev() {
        let mut cur = vec![false; s.len() + 1];
        for j in (0..=s.len()).rev() {
            cur[j] = if p[i] == "**" {
                next[j] || (j < s.len() && cur[j + 1])
            } else {
                j < s.len() && next[j + 1] && seg_match(p[i], s[j])
            };
        }
        next = cur;
    }
    next[0]
}

/// Whether `s` is a calendar date written `YYYY-MM-DD` (proleptic Gregorian, years 1..=9999).
pub fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let mut n = 0u32;
        for &c in b.get(r)? {
            if !c.is_ascii_digit() {
                return None;
            }
            n = n * 10 + u32::from(c - b'0');
        }
        Some(n)
    };
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    y >= 1 && d >= 1 && d <= days
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_cases() {
        // Hand-labelled.
        assert!(glob_match("**", "a"));
        assert!(glob_match("**", "a/b/c"));
        assert!(glob_match("tokens/**", "tokens/color.tokens.json"));
        assert!(glob_match("tokens/**", "tokens/a/b.json"));
        assert!(!glob_match("tokens/**", "icons/a.svg"));
        assert!(glob_match("**/*.svg", "icons/probe.svg"));
        assert!(glob_match("**/*.svg", "probe.svg"));
        assert!(!glob_match("*.svg", "icons/probe.svg"));
        assert!(glob_match("tokens/*.tokens.json", "tokens/color.tokens.json"));
        assert!(!glob_match("tokens/*.tokens.json", "tokens/sub/color.tokens.json"));
        assert!(glob_match("a/?.txt", "a/b.txt"));
        assert!(!glob_match("a/?.txt", "a/bb.txt"));
        assert!(glob_match("a/**/z", "a/z"));
        assert!(glob_match("a/**/z", "a/b/c/z"));
        assert!(!glob_match("a/**/z", "a/b/c/y"));
    }

    #[test]
    fn glob_and_path_validation() {
        assert!(glob_defect("**").is_none());
        assert!(glob_defect("a/**/b").is_none());
        assert!(glob_defect("a**/b").is_some());
        assert!(glob_defect("/abs").is_some());
        assert!(glob_defect("a/[bc]").is_some());
        assert!(glob_defect("").is_some());
        assert!(path_defect("tokens/color.tokens.json").is_none());
        for bad in
            ["", "/a", "a//b", "a/../b", "./a", "a\\b", "C:/a", "a/*.svg", "a\u{202E}b", "e\u{0301}", "a\u{200B}b"]
        {
            assert!(path_defect(bad).is_some(), "{bad:?}");
        }
    }

    #[test]
    fn text_defects() {
        assert!(defects("Ana Lopez").is_empty());
        assert!(defects("Jos\u{00E9}").is_empty());
        assert_eq!(defects("Jose\u{0301}"), ["not in Unicode NFC"]);
        assert!(defects("a\u{2067}b").contains(&"bidirectional control character"));
        assert!(defects("a\u{061C}b").contains(&"bidirectional control character"));
        assert!(defects("a\tb").contains(&"control character"));
        assert!(!defects("a\u{FE0F}").is_empty());
        assert!(!defects("\u{FF21}").is_empty());
        assert!(!defects("p\u{0430}ss").is_empty());
    }

    #[test]
    fn dates() {
        assert!(valid_date("2026-11-19"));
        assert!(valid_date("2024-02-29"));
        assert!(valid_date("2000-02-29"));
        assert!(!valid_date("1900-02-29"));
        assert!(!valid_date("2026-02-29"));
        assert!(!valid_date("2026-13-01"));
        assert!(!valid_date("2026-04-31"));
        assert!(!valid_date("0000-01-01"));
        assert!(!valid_date("2026-1-01"));
        assert!(!valid_date("2026-01-0a"));
    }
}
