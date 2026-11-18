//! String parameters: decoding the escapes to Unicode and encoding them back.
//!
//! Decoding: `''` is a quote, `\\` a backslash, `\S\c` the upper-half Latin-1 character
//! `c + 0x80`, `\X\hh` the Latin-1 character `hh`, `\X2\hhhh...\X0\` UTF-16 code units
//! (surrogate pairs combined), `\X4\hhhhhhhh...\X0\` code points, `\PA\` selects the
//! Latin-1 code page (the only one supported). Line breaks inside a string are ignored,
//! including between the hex digits of a `\X2\` or `\X4\` group. Raw bytes 0x80 and above
//! are accepted only when they form valid UTF-8; other raw bytes (for example Latin-1
//! from legacy exporters) are malformed, never guessed. Any other control byte is
//! malformed.
//!
//! Encoding (printer): printable ASCII stays as is (`'` doubled, `\` doubled); every
//! maximal run of other characters becomes one `\X2\...\X0\` group, or one
//! `\X4\...\X0\` group if the run holds a character above U+FFFF.

/// Why decoding stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextError {
    /// Bad syntax at the byte offset.
    Malformed(&'static str, usize),
    /// The input ended inside the string.
    Truncated,
    /// The decoded text would exceed the limit; offset of the offending byte.
    TooLong(usize),
    /// An unsupported construct at the byte offset.
    Unsupported(&'static str, usize),
}

fn push(out: &mut String, c: char, max: usize, at: usize) -> Result<(), TextError> {
    if out.len().saturating_add(c.len_utf8()) > max {
        return Err(TextError::TooLong(at));
    }
    out.push(c);
    Ok(())
}

fn hex_value(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some(u32::from(b - b'0')),
        b'A'..=b'F' => Some(u32::from(b - b'A') + 10),
        b'a'..=b'f' => Some(u32::from(b - b'a') + 10),
        _ => None,
    }
}

/// Reads `n` hex digits at `pos`.
fn hex_at(src: &[u8], pos: usize, n: usize) -> Result<u32, TextError> {
    let mut v: u32 = 0;
    for i in 0..n {
        let b = *src.get(pos + i).ok_or(TextError::Truncated)?;
        let d = hex_value(b).ok_or(TextError::Malformed("bad_hex", pos + i))?;
        v = (v << 4) | d;
    }
    Ok(v)
}

/// The offset of the first byte at or after `pos` that is not a line break.
fn skip_breaks(src: &[u8], mut pos: usize) -> usize {
    while matches!(src.get(pos), Some(b'\r' | b'\n')) {
        pos += 1;
    }
    pos
}

/// Reads `n` hex digits starting at `pos`, skipping line breaks between them. Returns the
/// value and the offset after the last digit.
fn hex_group(src: &[u8], mut pos: usize, n: usize) -> Result<(u32, usize), TextError> {
    let mut v: u32 = 0;
    for _ in 0..n {
        pos = skip_breaks(src, pos);
        v = (v << 4) | hex_at(src, pos, 1)?;
        pos += 1;
    }
    Ok((v, pos))
}

/// Expects `\` at `pos`.
fn backslash(src: &[u8], pos: usize) -> Result<(), TextError> {
    match src.get(pos) {
        Some(b'\\') => Ok(()),
        Some(_) => Err(TextError::Malformed("bad_string_escape", pos)),
        None => Err(TextError::Truncated),
    }
}

const END_GROUP: &[u8] = b"\\X0\\";

/// Decodes the string whose body starts at `pos` (just after the opening quote). Returns
/// the text and the offset just after the closing quote.
pub(crate) fn decode(src: &[u8], mut pos: usize, max: usize) -> Result<(String, usize), TextError> {
    let mut out = String::new();
    loop {
        let b = *src.get(pos).ok_or(TextError::Truncated)?;
        match b {
            b'\'' => {
                if src.get(pos + 1) == Some(&b'\'') {
                    push(&mut out, '\'', max, pos)?;
                    pos += 2;
                } else {
                    return Ok((out, pos + 1));
                }
            }
            b'\r' | b'\n' => pos += 1,
            b'\\' => pos = escape(src, pos, &mut out, max)?,
            0x20..=0x26 | 0x28..=0x5b | 0x5d..=0x7e => {
                push(&mut out, char::from(b), max, pos)?;
                pos += 1;
            }
            0x80..=0xff => {
                let len = match b {
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => return Err(TextError::Malformed("invalid_utf8", pos)),
                };
                for k in 1..len {
                    match src.get(pos + k) {
                        None => return Err(TextError::Truncated),
                        Some(0x80..=0xbf) => {}
                        Some(_) => return Err(TextError::Malformed("invalid_utf8", pos)),
                    }
                }
                let bytes = src.get(pos..pos + len).ok_or(TextError::Truncated)?;
                let s = std::str::from_utf8(bytes).map_err(|_| TextError::Malformed("invalid_utf8", pos))?;
                for c in s.chars() {
                    push(&mut out, c, max, pos)?;
                }
                pos += len;
            }
            _ => return Err(TextError::Malformed("control_in_string", pos)),
        }
    }
}

/// Decodes the escape starting with `\` at `pos`; returns the offset after it.
fn escape(src: &[u8], pos: usize, out: &mut String, max: usize) -> Result<usize, TextError> {
    let at = |i: usize| src.get(pos + i).copied();
    match at(1) {
        None => Err(TextError::Truncated),
        Some(b'\\') => {
            push(out, '\\', max, pos)?;
            Ok(pos + 2)
        }
        Some(b'S') => {
            backslash(src, pos + 2)?;
            let c = at(3).ok_or(TextError::Truncated)?;
            if !(0x20..=0x7e).contains(&c) {
                return Err(TextError::Malformed("bad_string_escape", pos));
            }
            let ch = char::from_u32(u32::from(c) + 0x80).ok_or(TextError::Malformed("bad_code_point", pos))?;
            push(out, ch, max, pos)?;
            Ok(pos + 4)
        }
        Some(b'P') => {
            let page = at(2).ok_or(TextError::Truncated)?;
            backslash(src, pos + 3)?;
            match page {
                b'A' => Ok(pos + 4),
                b'B'..=b'I' => Err(TextError::Unsupported("code_page", pos)),
                _ => Err(TextError::Malformed("bad_string_escape", pos)),
            }
        }
        Some(b'X') => match at(2) {
            None => Err(TextError::Truncated),
            Some(b'\\') => {
                let v = hex_at(src, pos + 3, 2)?;
                let ch = char::from_u32(v).ok_or(TextError::Malformed("bad_code_point", pos))?;
                push(out, ch, max, pos)?;
                Ok(pos + 5)
            }
            Some(b'2') => {
                backslash(src, pos + 3)?;
                let mut p = skip_breaks(src, pos + 4);
                let mut units: Vec<u16> = Vec::new();
                while src.get(p..p + 4) != Some(END_GROUP) {
                    let (v, next) = hex_group(src, p, 4)?;
                    if units.len() >= max {
                        return Err(TextError::TooLong(p));
                    }
                    units.push(u16::try_from(v).map_err(|_| TextError::Malformed("bad_hex", p))?);
                    p = skip_breaks(src, next);
                }
                for r in char::decode_utf16(units.iter().copied()) {
                    let ch = r.map_err(|_| TextError::Malformed("bad_code_point", pos))?;
                    push(out, ch, max, pos)?;
                }
                Ok(p + 4)
            }
            Some(b'4') => {
                backslash(src, pos + 3)?;
                let mut p = skip_breaks(src, pos + 4);
                while src.get(p..p + 4) != Some(END_GROUP) {
                    let (v, next) = hex_group(src, p, 8)?;
                    let ch = char::from_u32(v).ok_or(TextError::Malformed("bad_code_point", p))?;
                    push(out, ch, max, p)?;
                    p = skip_breaks(src, next);
                }
                Ok(p + 4)
            }
            Some(_) => Err(TextError::Malformed("bad_string_escape", pos)),
        },
        Some(_) => Err(TextError::Malformed("bad_string_escape", pos)),
    }
}

fn hex_digit(v: u32) -> char {
    char::from_digit(v & 0xf, 16).map_or('0', |c| c.to_ascii_uppercase())
}

fn flush(out: &mut String, run: &mut Vec<char>) {
    if run.is_empty() {
        return;
    }
    let wide = run.iter().any(|&c| u32::from(c) > 0xffff);
    let (tag, digits): (&str, u32) = if wide { ("\\X4\\", 8) } else { ("\\X2\\", 4) };
    out.push_str(tag);
    for &c in run.iter() {
        let v = u32::from(c);
        for k in (0..digits).rev() {
            out.push(hex_digit(v >> (4 * k)));
        }
    }
    out.push_str("\\X0\\");
    run.clear();
}

/// Appends `text` as a quoted string token.
pub(crate) fn encode(out: &mut String, text: &str) {
    out.push('\'');
    let mut run: Vec<char> = Vec::new();
    for c in text.chars() {
        match c {
            '\'' => {
                flush(out, &mut run);
                out.push_str("''");
            }
            '\\' => {
                flush(out, &mut run);
                out.push_str("\\\\");
            }
            ' '..='&' | '('..='[' | ']'..='~' => {
                flush(out, &mut run);
                out.push(c);
            }
            _ => run.push(c),
        }
    }
    flush(out, &mut run);
    out.push('\'');
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn dec(s: &str) -> Result<String, TextError> {
        decode(s.as_bytes(), 0, 1 << 20).map(|(t, _)| t)
    }

    #[test]
    fn escapes_decode_to_hand_labelled_text() {
        assert_eq!(dec("it''s'").unwrap(), "it's");
        assert_eq!(dec("a\\\\b'").unwrap(), "a\\b");
        assert_eq!(dec("\\S\\D'").unwrap(), "\u{c4}");
        assert_eq!(dec("\\X\\E9'").unwrap(), "\u{e9}");
        assert_eq!(dec("\\X2\\03B103B2\\X0\\'").unwrap(), "\u{3b1}\u{3b2}");
        assert_eq!(dec("\\X2\\D83DDE00\\X0\\'").unwrap(), "\u{1f600}");
        assert_eq!(dec("\\X4\\0001F600\\X0\\'").unwrap(), "\u{1f600}");
        assert_eq!(dec("\\PA\\x'").unwrap(), "x");
        assert_eq!(dec("a\r\nb'").unwrap(), "ab");
        // Line breaks between and inside the hex digits of \X2\ and \X4\ groups.
        assert_eq!(dec("\\X2\\03B1\r\n03B2\\X0\\'").unwrap(), "\u{3b1}\u{3b2}");
        assert_eq!(dec("\\X2\\\n03\nB1\n\\X0\\'").unwrap(), "\u{3b1}");
        assert_eq!(dec("\\X4\\0001\r\nF600\\X0\\'").unwrap(), "\u{1f600}");
        // Raw Latin-1 (not UTF-8) is rejected, not guessed.
        assert_eq!(decode(b"caf\xe9'", 0, 1 << 20), Err(TextError::Malformed("invalid_utf8", 3)));
        assert_eq!(dec("\\PB\\x'"), Err(TextError::Unsupported("code_page", 0)));
        assert_eq!(dec("\\Q'"), Err(TextError::Malformed("bad_string_escape", 0)));
        assert_eq!(dec("\\X2\\D83D\\X0\\'"), Err(TextError::Malformed("bad_code_point", 0)));
        assert_eq!(dec("abc"), Err(TextError::Truncated));
        assert_eq!(decode(b"abcd'", 0, 3), Err(TextError::TooLong(3)));
    }

    #[test]
    fn encode_round_trips() {
        for s in ["", "plain", "it's", "a\\b", "Gr\u{fc}\u{df}e", "x\u{1f600}\u{e9}y", "\n\t"] {
            let mut out = String::new();
            encode(&mut out, s);
            let body = out.get(1..).unwrap();
            assert_eq!(dec(body).unwrap(), s, "{out}");
        }
        let mut out = String::new();
        encode(&mut out, "Gr\u{fc}\u{df}e");
        assert_eq!(out, "'Gr\\X2\\00FC00DF\\X0\\e'");
    }
}
