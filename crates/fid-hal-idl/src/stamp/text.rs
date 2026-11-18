//! Untrusted device-supplied text (CONSTITUTION C6, REQ-HAL.IDL-005).

use std::fmt::Write as _;

/// A string a device or file supplied: make, model, serial, firmware, a claim's method
/// description. It is data, never instructions (CONSTITUTION C6).
///
/// Deliberately thin, like `fid-io-reg`'s type of the same name (which lives in a higher
/// layer and so cannot be used here): no `Deref<Target = str>`, no `AsRef<str>`, no
/// `Display`, no conversion into `String`. The only views are [`UntrustedText::as_display`],
/// which escapes `\` and control, bidirectional, zero-width and line-separator characters,
/// and [`UntrustedText::len`]. The wire form is the plain JSON string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct UntrustedText(Box<str>);

#[allow(clippy::len_without_is_empty)] // Empty text is not a valid record value.
impl UntrustedText {
    /// Wraps device-supplied text. Length bounds are checked when the record is validated.
    pub fn new(text: &str) -> UntrustedText {
        UntrustedText(Box::from(text))
    }

    /// Length in bytes (UTF-8).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// An escaped copy for showing to a person: `\` becomes `\\`, and every control
    /// character, U+200B..U+200F, U+2028..U+202E, U+2060..U+206F and U+FEFF become a
    /// `\u{..}` escape (lowercase hex).
    pub fn as_display(&self) -> String {
        let mut out = String::with_capacity(self.0.len());
        for c in self.0.chars() {
            let u = c as u32;
            let hidden = c.is_control()
                || (0x200b..=0x200f).contains(&u)
                || (0x2028..=0x202e).contains(&u)
                || (0x2060..=0x206f).contains(&u)
                || u == 0xfeff;
            if c == '\\' {
                out.push_str("\\\\");
            } else if hidden {
                let _ = write!(out, "\\u{{{u:x}}}");
            } else {
                out.push(c);
            }
        }
        out
    }

    /// The raw text, for writing the wire form only.
    pub(crate) fn raw(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_escapes_hidden_characters() {
        let t = UntrustedText::new("a\\b\u{202e}c\n\u{feff}é");
        assert_eq!(t.as_display(), "a\\\\b\\u{202e}c\\u{a}\\u{feff}é");
        assert_eq!(t.len(), "a\\b\u{202e}c\n\u{feff}é".len());
    }
}
