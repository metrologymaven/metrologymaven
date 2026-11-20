//! Locale presentation of a kernel canonical string (REQ-RPT.MODEL.FMT-010).
//!
//! The render takes the kernel formatter's canonical string (ASCII digits, at most one `.`,
//! a leading `-` only for a nonzero negative output; `fmt.angle@1` in dms adds `:` between
//! degrees, minutes and seconds) and changes exactly three things: the minus sign, the
//! decimal separator and the grouping separators inserted into the integer part. It never
//! rounds, drops, adds or reorders a digit, so trailing zeros stay. Dms strings are not
//! grouped. Fractional digits are never grouped (`I18N.LOCALE` open question; default no).
//!
//! [`LocaleRules::new`] refuses rules that could make two canonical strings render alike:
//! the decimal separator, the grouping separator and every character of the minus sign must
//! be pairwise different, none may be an ASCII digit, `:`, `-` (except the minus itself) or
//! a control character. Then the canonical string is recovered from the display string by
//! deleting the grouping separators and mapping the decimal separator and the minus sign
//! back, so the render is injective.

use core::fmt;

/// Longest locale ID or data release ID, in bytes.
pub const MAX_LOCALE_ID_BYTES: usize = 64;
/// Most characters in a minus sign.
pub const MAX_MINUS_CHARS: usize = 4;

/// Why locale rules or a render were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocaleError {
    /// The rules break injectivity or bounds; the detail names the field.
    InvalidRules(&'static str),
    /// The input is not a kernel canonical string.
    NotCanonical,
}

impl fmt::Display for LocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocaleError::InvalidRules(d) => write!(f, "report.locale_invalid: {d}"),
            LocaleError::NotCanonical => f.write_str("report.locale_invalid: not_canonical"),
        }
    }
}

impl std::error::Error for LocaleError {}

/// Number presentation rules of one locale, as published in the pinned `I18N.LOCALE` data
/// file (its release ID goes into the resolved report's provenance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleRules {
    id: String,
    release: String,
    decimal: char,
    group: Option<char>,
    primary: u8,
    secondary: u8,
    minus: String,
}

fn reserved(c: char) -> bool {
    c.is_ascii_digit() || c == ':' || c.is_control()
}

fn id_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_LOCALE_ID_BYTES && s.bytes().all(|b| b.is_ascii_graphic())
}

impl LocaleRules {
    /// Checks and builds rules. `group` is `None` for a locale that does not group;
    /// `primary` is the size of the group next to the decimal separator and `secondary` of
    /// every group further left (3 and 3 for most locales, 3 and 2 for `en-IN`), each 1..=9.
    pub fn new(
        id: &str,
        release: &str,
        decimal: char,
        group: Option<char>,
        primary: u8,
        secondary: u8,
        minus: &str,
    ) -> Result<LocaleRules, LocaleError> {
        let bad = LocaleError::InvalidRules;
        if !id_ok(id) {
            return Err(bad("id"));
        }
        if !id_ok(release) {
            return Err(bad("release"));
        }
        if reserved(decimal) || decimal == '-' {
            return Err(bad("decimal"));
        }
        if let Some(g) = group {
            if reserved(g) || g == '-' || g == decimal {
                return Err(bad("group"));
            }
        }
        if !(1..=9).contains(&primary) || !(1..=9).contains(&secondary) {
            return Err(bad("grouping"));
        }
        let n = minus.chars().count();
        if n == 0 || n > MAX_MINUS_CHARS {
            return Err(bad("minus"));
        }
        if minus.chars().any(|c| reserved(c) || c == decimal || Some(c) == group || c.is_whitespace()) {
            return Err(bad("minus"));
        }
        Ok(LocaleRules {
            id: id.to_owned(),
            release: release.to_owned(),
            decimal,
            group,
            primary,
            secondary,
            minus: minus.to_owned(),
        })
    }

    /// The locale ID, for example `de`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The locale data release the rules come from.
    pub fn release(&self) -> &str {
        &self.release
    }

    /// Renders a kernel canonical string. Refuses anything that is not one
    /// ([`LocaleError::NotCanonical`]); never re-rounds.
    pub fn render(&self, canonical: &str) -> Result<String, LocaleError> {
        let (neg, body) = match canonical.strip_prefix('-') {
            Some(b) => (true, b),
            None => (false, canonical),
        };
        let (int_part, frac) = match body.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (body, None),
        };
        let dms = int_part.contains(':');
        if !well_formed(int_part, frac, dms) {
            return Err(LocaleError::NotCanonical);
        }
        let mut out = String::with_capacity(canonical.len() + canonical.len() / 2 + self.minus.len());
        if neg {
            out.push_str(&self.minus);
        }
        match (self.group, dms) {
            (Some(g), false) => self.push_grouped(&mut out, int_part, g),
            _ => out.push_str(int_part),
        }
        if let Some(f) = frac {
            out.push(self.decimal);
            out.push_str(f);
        }
        Ok(out)
    }

    fn push_grouped(&self, out: &mut String, digits: &str, g: char) {
        // Group sizes from the right: primary first, then secondary.
        let len = digits.len();
        let mut cuts = Vec::new();
        let mut at = len;
        let mut size = usize::from(self.primary);
        while at > size {
            at -= size;
            cuts.push(at);
            size = usize::from(self.secondary);
        }
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && cuts.contains(&i) {
                out.push(g);
            }
            out.push(c);
        }
    }
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn well_formed(int_part: &str, frac: Option<&str>, dms: bool) -> bool {
    if frac.is_some_and(|f| !digits(f)) {
        return false;
    }
    if dms {
        let mut parts = int_part.split(':');
        let ok = matches!(
            (parts.next(), parts.next(), parts.next(), parts.next()),
            (Some(d), Some(m), Some(s), None) if digits(d) && m.len() == 2 && digits(m) && s.len() == 2 && digits(s)
        );
        return ok;
    }
    digits(int_part)
}
