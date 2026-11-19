//! User-entered text (REQ-009, REQ-010, CONSTITUTION C6).
//!
//! [`Text`] is a bounded string: at most [`MAX_TEXT_BYTES`] bytes of UTF-8 and no NUL.
//! [`Untrusted`] wraps content-derived values so they are never mistaken for instructions:
//! it has no `Display` and no `Deref`. Its machine form is always the object
//! `{"text": <string>, "untrusted": true}` (storage, `--json` and MCP alike), and its only
//! human form is [`Untrusted::escaped`], which spells every control, bidi and invisible
//! format character as `\u{XXXX}`.

use super::error::{ReportError, TextFault};
use super::unicode16::{in_ranges, DEFAULT_IGNORABLE_RANGES, FORMAT_RANGES};
use fid_canon::CValue;
use std::collections::BTreeMap;

/// Largest user string, in bytes of UTF-8 (REQ-010).
pub const MAX_TEXT_BYTES: usize = 4096;

/// A bounded user string: at most [`MAX_TEXT_BYTES`] bytes and no NUL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Text(String);

impl Text {
    /// Checks the bounds. `report.text_invalid` if longer than [`MAX_TEXT_BYTES`] bytes or
    /// if it holds U+0000. The length is checked first, so a long string with a NUL reports
    /// `too_long`.
    pub fn new(s: &str) -> Result<Text, ReportError> {
        if s.len() > MAX_TEXT_BYTES {
            return Err(ReportError::TextInvalid(TextFault::TooLong));
        }
        if s.contains('\0') {
            return Err(ReportError::TextInvalid(TextFault::Nul));
        }
        Ok(Text(s.to_string()))
    }

    /// The raw string, for machine serialisers and comparisons.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Length in bytes of UTF-8.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A content-derived value (CONSTITUTION C6). Reading the value needs the deliberately
/// named [`Untrusted::untrusted_value`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Untrusted<T>(T);

impl<T> Untrusted<T> {
    pub fn new(value: T) -> Untrusted<T> {
        Untrusted(value)
    }

    /// The wrapped value. Callers must keep treating it as data.
    pub fn untrusted_value(&self) -> &T {
        &self.0
    }
}

impl Untrusted<Text> {
    /// Checks and wraps a user string (`report.text_invalid` on failure).
    pub fn text(s: &str) -> Result<Untrusted<Text>, ReportError> {
        Text::new(s).map(Untrusted)
    }

    /// Machine form: `{"text": <string>, "untrusted": true}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("text".to_string(), CValue::Str(self.0.as_str().to_string()));
        o.insert("untrusted".to_string(), CValue::Bool(true));
        CValue::Obj(o)
    }

    /// Reads the machine form back. Exactly the keys `text` and `untrusted`, with
    /// `untrusted` equal to `true`; anything else is `report.store_invalid`. Text bounds
    /// apply (`report.text_invalid`).
    pub fn from_value(v: &CValue) -> Result<Untrusted<Text>, ReportError> {
        let CValue::Obj(o) = v else { return Err(ReportError::StoreInvalid("untrusted text is not an object")) };
        if o.len() != 2 || o.get("untrusted") != Some(&CValue::Bool(true)) {
            return Err(ReportError::StoreInvalid("untrusted text marker"));
        }
        match o.get("text") {
            Some(CValue::Str(s)) => Untrusted::text(s),
            _ => Err(ReportError::StoreInvalid("untrusted text is not a string")),
        }
    }

    /// Human form for terminals and logs: the text with `\` doubled and every character in
    /// [`must_escape`] written as `\u{XXXX}` (uppercase hex, at least four digits). The
    /// result holds no control, bidi or invisible format character.
    pub fn escaped(&self) -> String {
        let mut out = String::with_capacity(self.0.len());
        for c in self.0.as_str().chars() {
            if c == '\\' {
                out.push_str("\\\\");
            } else if must_escape(c) {
                out.push_str(&format!("\\u{{{:04X}}}", u32::from(c)));
            } else {
                out.push(c);
            }
        }
        out
    }
}

/// Characters [`Untrusted::escaped`] never prints raw: C0 and C1 controls and DEL; every
/// code point of general category Cf (format) and every Default_Ignorable_Code_Point, both
/// from the pinned Unicode 16.0 tables in [`super::unicode16`] (bidi controls and marks,
/// zero-width characters, tag characters, variation selectors, U+034F, U+115F, U+1160,
/// U+180E, U+3164, U+FFA0 and the rest); the line and paragraph separators (U+2028,
/// U+2029); and the object replacement character (U+FFFC).
pub fn must_escape(c: char) -> bool {
    let u = u32::from(c);
    u < 0x20
        || (0x7F..=0x9F).contains(&u)
        || u == 0x2028
        || u == 0x2029
        || u == 0xFFFC
        || in_ranges(FORMAT_RANGES, c)
        || in_ranges(DEFAULT_IGNORABLE_RANGES, c)
}
