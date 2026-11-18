//! Untrusted file-derived text (CONSTITUTION C6, REQ-IO.REG.LIMITS-005).

use std::fmt::{self, Write as _};

/// A string a reader took from file content: names, comments, metadata, labels. It is
/// data, never instructions (CONSTITUTION C6).
///
/// Deliberately thin: there is no `Deref<Target = str>`, no `AsRef<str>`, no `Borrow<str>`,
/// no `Display`, and no conversion into `String`. The only views are [`as_display`],
/// which escapes control and bidirectional-override characters, and [`len`]. Nothing
/// in this crate turns it into an instruction or command string.
///
/// ```compile_fail
/// let t = fid_io_reg::UntrustedText::new("rm -rf /");
/// let s: &str = &*t; // no Deref<Target = str>
/// ```
///
/// ```compile_fail
/// let t = fid_io_reg::UntrustedText::new("rm -rf /");
/// let s: String = t.into(); // no Into<String>
/// ```
///
/// ```compile_fail
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

    /// A display view for showing the text to a person. Control characters and
    /// bidirectional formatting characters are written as `\u{..}` escapes.
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

fn needs_escape(c: char) -> bool {
    c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

impl fmt::Display for UntrustedDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in self.0.chars() {
            if needs_escape(c) {
                write!(f, "\\u{{{:x}}}", u32::from(c))?;
            } else {
                f.write_char(c)?;
            }
        }
        Ok(())
    }
}
