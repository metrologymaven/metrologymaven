//! Untrusted file-derived text (CONSTITUTION C6, REQ-IO.REG.LIMITS-005).

use std::fmt::{self, Write as _};

/// A string a reader took from file content: names, comments, metadata, labels. It is
/// data, never instructions (CONSTITUTION C6).
///
/// Deliberately thin: there is no `Deref<Target = str>`, no `AsRef<str>`, no `Borrow<str>`,
/// no `Display`, and no conversion into `String`. The only views are [`as_display`],
/// which escapes `\` and every control, format and line/paragraph-separator character,
/// and [`len`]. Nothing in this crate turns it into an instruction or command string.
///
/// ```compile_fail,E0614
/// let t = fid_io_reg::UntrustedText::new("rm -rf /");
/// let s: &str = &*t; // no Deref<Target = str>
/// ```
///
/// ```compile_fail,E0277
/// let t = fid_io_reg::UntrustedText::new("rm -rf /");
/// let s: String = t.into(); // no Into<String>
/// ```
///
/// ```compile_fail,E0599
/// let t = fid_io_reg::UntrustedText::new("rm -rf /");
/// let s = t.to_string(); // no Display
/// ```
///
/// [`as_display`]: UntrustedText::as_display
/// [`len`]: UntrustedText::len
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct UntrustedText(Box<str>);

#[allow(clippy::len_without_is_empty)] // AC-6: only as_display() and len() are offered.
impl UntrustedText {
    /// Wraps text taken from a file.
    pub fn new(text: &str) -> Self {
        UntrustedText(Box::from(text))
    }

    /// Wraps an owned string taken from a file.
    pub fn from_string(text: String) -> Self {
        UntrustedText(text.into_boxed_str())
    }

    /// Wraps file bytes, replacing invalid UTF-8 with U+FFFD.
    pub fn from_utf8_lossy(bytes: &[u8]) -> Self {
        UntrustedText::from_string(String::from_utf8_lossy(bytes).into_owned())
    }

    /// Length in bytes (UTF-8).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The raw text, for this crate's machine-readable serialisers (canonical JSON) only.
    /// Never used for human-readable output, which goes through [`as_display`].
    ///
    /// [`as_display`]: UntrustedText::as_display
    pub(crate) fn raw(&self) -> &str {
        &self.0
    }

    /// A display view for showing the text to a person. `\` is written as `\\`, and
    /// every character of general category Cc (control), Cf (format, which includes the
    /// bidirectional overrides, zero-width characters and the BOM), Zl or Zp is written
    /// as a `\u{..}` escape (lowercase hex), so the output is unambiguous.
    pub fn as_display(&self) -> UntrustedDisplay<'_> {
        UntrustedDisplay(&self.0)
    }
}

impl fmt::Debug for UntrustedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UntrustedText({:?})", self.as_display().to_string())
    }
}

/// Display view of an [`UntrustedText`]; see [`UntrustedText::as_display`].
#[derive(Debug, Clone, Copy)]
pub struct UntrustedDisplay<'a>(&'a str);

/// General category Cf (Unicode 16.0) plus Zl (U+2028) and Zp (U+2029), as inclusive
/// code point ranges. Checked against Python's `unicodedata` by
/// `fixtures/io/reg/limits/make_escape_cases.py`.
#[rustfmt::skip]
const FORMAT_OR_SEPARATOR: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),
    (0x0600, 0x0605),
    (0x061C, 0x061C),
    (0x06DD, 0x06DD),
    (0x070F, 0x070F),
    (0x0890, 0x0891),
    (0x08E2, 0x08E2),
    (0x180E, 0x180E),
    (0x200B, 0x200F),
    (0x2028, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0001, 0xE0001),
    (0xE0020, 0xE007F),
];

fn needs_escape(c: char) -> bool {
    let cp = u32::from(c);
    c.is_control() || FORMAT_OR_SEPARATOR.iter().any(|&(lo, hi)| lo <= cp && cp <= hi)
}

impl fmt::Display for UntrustedDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in self.0.chars() {
            if c == '\\' {
                f.write_str("\\\\")?;
            } else if needs_escape(c) {
                write!(f, "\\u{{{:x}}}", u32::from(c))?;
            } else {
                f.write_char(c)?;
            }
        }
        Ok(())
    }
}
