//! Untrusted document text and the decoding of character data and attribute values.

use crate::chars::{is_name, is_xml_char};
use fid_io_reg::UntrustedText;
use std::fmt;

/// Text taken from the document: character data, CDATA content, comments, PI data and
/// attribute values (CONSTITUTION C6). It is data, never instructions.
///
/// There is no `Deref<Target = str>`, no `Display` and no conversion into `String`.
/// [`XmlText::raw`] gives the characters to code that parses them as typed values
/// (numbers, identifiers, enumerations); everything else goes through
/// [`XmlText::to_untrusted`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct XmlText(Box<str>);

impl XmlText {
    /// Wraps text.
    pub fn new(text: &str) -> Self {
        XmlText(Box::from(text))
    }

    pub(crate) fn from_string(text: String) -> Self {
        XmlText(text.into_boxed_str())
    }

    /// The characters, for typed parsing only. Never use them as a command, path,
    /// prompt or other instruction.
    pub fn raw(&self) -> &str {
        &self.0
    }

    /// Length in bytes (UTF-8).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True for empty text.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The same text as the harness's [`UntrustedText`], for display and journaling.
    pub fn to_untrusted(&self) -> UntrustedText {
        UntrustedText::new(&self.0)
    }
}

impl fmt::Debug for XmlText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = self.to_untrusted().as_display().to_string();
        write!(f, "XmlText({shown:?})")
    }
}

/// How raw text is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Character data: references expanded, line ends normalised, `]]>` refused.
    Text,
    /// Attribute value: references expanded, line ends and white space become spaces,
    /// `<` refused.
    Attr,
    /// Comment, CDATA or PI data: line ends normalised only.
    Plain,
}

/// Why decoding failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeErr {
    /// A reference to an entity other than the five predefined ones.
    Forbidden,
    /// Not well-formed.
    Malformed(&'static str),
}

/// Decodes raw (valid UTF-8, XML-Char checked) text. On failure returns the index in
/// `raw` where the offending construct starts.
pub(crate) fn decode(raw: &[u8], mode: Mode) -> Result<String, (usize, DecodeErr)> {
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while let Some(&b) = raw.get(i) {
        match b {
            b'\r' => {
                out.push(if mode == Mode::Attr { b' ' } else { b'\n' });
                i += if raw.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
            }
            b'\n' | b'\t' if mode == Mode::Attr => {
                out.push(b' ');
                i += 1;
            }
            b'&' if mode != Mode::Plain => {
                let (c, len) = reference(raw, i)?;
                let mut tmp = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
                i += len;
            }
            b'<' if mode == Mode::Attr => return Err((i, DecodeErr::Malformed("lt_in_attribute_value"))),
            b']' if mode == Mode::Text && raw.get(i..i + 3) == Some(&b"]]>"[..]) => {
                return Err((i, DecodeErr::Malformed("cdata_end_in_text")));
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| (0, DecodeErr::Malformed("invalid_utf8")))
}

/// Decodes the reference starting with `&` at `at`: the character and the bytes used.
fn reference(raw: &[u8], at: usize) -> Result<(char, usize), (usize, DecodeErr)> {
    let bad = |reason: &'static str| (at, DecodeErr::Malformed(reason));
    let rest = raw.get(at + 1..).unwrap_or(&[]);
    let semi = rest.iter().position(|&b| b == b';').ok_or_else(|| bad("unterminated_reference"))?;
    let body = rest.get(..semi).unwrap_or(&[]);
    let used = semi + 2;
    if let Some(num) = body.strip_prefix(b"#") {
        let (digits, radix) = match num.strip_prefix(b"x") {
            Some(hex) => (hex, 16),
            None => (num, 10),
        };
        if digits.is_empty() {
            return Err(bad("bad_character_reference"));
        }
        let mut v: u32 = 0;
        for &d in digits {
            let dv = char::from(d).to_digit(radix).ok_or_else(|| bad("bad_character_reference"))?;
            v = v
                .checked_mul(radix)
                .and_then(|v| v.checked_add(dv))
                .filter(|&v| v <= 0x10FFFF)
                .ok_or_else(|| bad("bad_character_reference"))?;
        }
        if !is_xml_char(v) {
            return Err(bad("bad_character_reference"));
        }
        let c = char::from_u32(v).ok_or_else(|| bad("bad_character_reference"))?;
        return Ok((c, used));
    }
    let c = match body {
        b"lt" => '<',
        b"gt" => '>',
        b"amp" => '&',
        b"apos" => '\'',
        b"quot" => '"',
        _ if is_name(body) => return Err((at, DecodeErr::Forbidden)),
        _ => return Err(bad("bad_reference")),
    };
    Ok((c, used))
}
