//! Character classes of XML 1.0 (fifth edition) and Namespaces in XML 1.0, and the
//! name scanner.

/// XML white space: space, tab, carriage return, line feed.
pub(crate) fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// The index of the first non-white-space byte at or after `i` (or `s.len()`).
pub(crate) fn skip_ws(s: &[u8], i: usize) -> usize {
    let mut j = i;
    while s.get(j).is_some_and(|&b| is_ws(b)) {
        j += 1;
    }
    j
}

/// NameStartChar.
pub(crate) fn is_name_start(c: char) -> bool {
    matches!(c,
        ':' | 'A'..='Z' | '_' | 'a'..='z'
        | '\u{C0}'..='\u{D6}'
        | '\u{D8}'..='\u{F6}'
        | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}'
        | '\u{37F}'..='\u{1FFF}'
        | '\u{200C}'..='\u{200D}'
        | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}'
        | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}'
        | '\u{FDF0}'..='\u{FFFD}'
        | '\u{10000}'..='\u{EFFFF}')
}

/// NameChar.
pub(crate) fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

/// The Char production, for character references.
pub(crate) fn is_xml_char(c: u32) -> bool {
    c == 0x9
        || c == 0xA
        || c == 0xD
        || (0x20..=0xD7FF).contains(&c)
        || (0xE000..=0xFFFD).contains(&c)
        || (0x10000..=0x10FFFF).contains(&c)
}

/// The character starting at byte `i` of the UTF-8 bytes `s`, with its encoded length.
pub(crate) fn char_at(s: &[u8], i: usize) -> Option<(char, usize)> {
    let b0 = *s.get(i)?;
    let len = if b0 < 0x80 {
        1
    } else if b0 >= 0xF0 {
        4
    } else if b0 >= 0xE0 {
        3
    } else {
        2
    };
    let bytes = s.get(i..i.checked_add(len)?)?;
    let c = std::str::from_utf8(bytes).ok()?.chars().next()?;
    Some((c, len))
}

/// Why a name could not be scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameErr {
    /// The byte at this index cannot start a name.
    Bad(usize),
    /// The name starting at this index is longer than allowed.
    TooLong(usize),
}

/// Scans a Name starting at byte `i` of `s` and returns the index just after it. When
/// the returned index equals `s.len()` the name may continue in input not yet read.
/// A name longer than `max` bytes fails as soon as `max + 1` bytes of it are seen.
pub(crate) fn scan_name(s: &[u8], i: usize, max: usize) -> Result<usize, NameErr> {
    let mut j = i;
    while j < s.len() {
        let Some((c, n)) = char_at(s, j) else {
            return Err(NameErr::Bad(j));
        };
        let ok = if j == i { is_name_start(c) } else { is_name_char(c) };
        if !ok {
            if j == i {
                return Err(NameErr::Bad(j));
            }
            return Ok(j);
        }
        j += n;
        if j - i > max {
            return Err(NameErr::TooLong(i));
        }
    }
    Ok(j)
}

/// True if all of `s` is one Name.
pub(crate) fn is_name(s: &[u8]) -> bool {
    !s.is_empty() && scan_name(s, 0, usize::MAX) == Ok(s.len())
}

/// Splits a QName into prefix and local part; `None` if it is not a QName (empty part
/// or more than one colon).
pub(crate) fn split_qname(name: &str) -> Option<(Option<&str>, &str)> {
    match name.split_once(':') {
        None => (!name.is_empty()).then_some((None, name)),
        Some((p, l)) => (!p.is_empty() && !l.is_empty() && !l.contains(':')).then_some((Some(p), l)),
    }
}

/// The first index of `needle` in `h` at or after `from`.
pub(crate) fn find(h: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    h.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}
