//! The table dialect: delimiter, decimal mark, header row, RFC 4180 quoting, UTF-8.
//!
//! Every rule here is byte-exact and locale-free: no platform number formatting or parsing
//! is used, the only decimal grammar is the one in [`Dialect::to_point_decimal`], and the
//! only encoding is UTF-8 (a leading byte-order mark is accepted on read, never written).

use fid_io_reg::{IoError, Position};
use std::fmt;

/// The quoting rule of `colmap@1` (the only one).
pub const QUOTE: &str = "rfc4180";
/// The encoding of `colmap@1` (the only one).
pub const ENCODING: &str = "utf-8";
/// The UTF-8 byte-order mark.
pub const BOM: &[u8] = b"\xEF\xBB\xBF";
/// Most fields one record may hold.
pub const MAX_FIELDS: usize = 65_536;
/// Longest decimal text accepted, in bytes.
pub const MAX_DECIMAL_BYTES: usize = 1024;

/// Field delimiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Delimiter {
    /// `,`
    Comma,
    /// `;`
    Semicolon,
    /// Horizontal tab (TSV).
    Tab,
}

impl Delimiter {
    /// Wire name in `colmap@1`.
    pub fn as_str(self) -> &'static str {
        match self {
            Delimiter::Comma => "comma",
            Delimiter::Semicolon => "semicolon",
            Delimiter::Tab => "tab",
        }
    }

    /// Parses a wire name; exact match only.
    pub fn parse(s: &str) -> Option<Delimiter> {
        [Delimiter::Comma, Delimiter::Semicolon, Delimiter::Tab].into_iter().find(|d| d.as_str() == s)
    }

    /// The delimiter byte.
    pub fn byte(self) -> u8 {
        match self {
            Delimiter::Comma => b',',
            Delimiter::Semicolon => b';',
            Delimiter::Tab => b'\t',
        }
    }
}

/// Decimal mark of decimal fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecimalMark {
    /// `.`
    Point,
    /// `,` (only with a non-comma delimiter).
    Comma,
}

impl DecimalMark {
    /// Wire value in `colmap@1`.
    pub fn as_str(self) -> &'static str {
        match self {
            DecimalMark::Point => ".",
            DecimalMark::Comma => ",",
        }
    }

    /// Parses a wire value; exact match only.
    pub fn parse(s: &str) -> Option<DecimalMark> {
        match s {
            "." => Some(DecimalMark::Point),
            "," => Some(DecimalMark::Comma),
            _ => None,
        }
    }

    /// The mark byte.
    pub fn byte(self) -> u8 {
        match self {
            DecimalMark::Point => b'.',
            DecimalMark::Comma => b',',
        }
    }
}

/// Record terminator used when writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineEnding {
    /// `\n`
    Lf,
    /// `\r\n`
    Crlf,
}

impl LineEnding {
    /// Wire name in `colmap@1`.
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "lf",
            LineEnding::Crlf => "crlf",
        }
    }

    /// Parses a wire name; exact match only.
    pub fn parse(s: &str) -> Option<LineEnding> {
        match s {
            "lf" => Some(LineEnding::Lf),
            "crlf" => Some(LineEnding::Crlf),
            _ => None,
        }
    }

    /// The terminator text.
    pub fn terminator(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }
}

/// Reason of [`Dialect::new`] failing: decimal comma with comma delimiter.
pub const REASON_COMMA_COMMA: &str = "decimal_comma_with_comma_delimiter";

/// A validated table dialect. Quoting is always RFC 4180 (`"` quote, `""` escape) and the
/// encoding always UTF-8, so neither is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dialect {
    delimiter: Delimiter,
    decimal_mark: DecimalMark,
    header: bool,
    line_ending: LineEnding,
}

fn malformed(at: usize, reason: &'static str) -> IoError {
    IoError::Malformed { at: Position::byte(at as u64), reason }
}

impl Dialect {
    /// A dialect; decimal comma with comma delimiter fails with
    /// [`REASON_COMMA_COMMA`].
    pub fn new(
        delimiter: Delimiter,
        decimal_mark: DecimalMark,
        header: bool,
        line_ending: LineEnding,
    ) -> Result<Dialect, &'static str> {
        if delimiter == Delimiter::Comma && decimal_mark == DecimalMark::Comma {
            return Err(REASON_COMMA_COMMA);
        }
        Ok(Dialect { delimiter, decimal_mark, header, line_ending })
    }

    /// Field delimiter.
    pub fn delimiter(&self) -> Delimiter {
        self.delimiter
    }
    /// Decimal mark.
    pub fn decimal_mark(&self) -> DecimalMark {
        self.decimal_mark
    }
    /// Whether the first record is a header row.
    pub fn header(&self) -> bool {
        self.header
    }
    /// Record terminator used when writing.
    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Whether `field` must be quoted: it holds the delimiter, `"`, CR or LF.
    pub fn needs_quotes(&self, field: &str) -> bool {
        let d = self.delimiter.byte();
        field.bytes().any(|b| b == d || b == b'"' || b == b'\r' || b == b'\n')
    }

    /// One field in RFC 4180 form: as is, or in double quotes with every `"` doubled when
    /// [`Dialect::needs_quotes`]. Formula-prefix neutralisation is the exporter's job
    /// (`IO.RES.CSV.OUT`), not the dialect's.
    pub fn encode_field(&self, field: &str) -> String {
        if !self.needs_quotes(field) {
            return field.to_owned();
        }
        let mut out = String::with_capacity(field.len().saturating_add(2));
        out.push('"');
        for c in field.chars() {
            if c == '"' {
                out.push('"');
            }
            out.push(c);
        }
        out.push('"');
        out
    }

    /// One record: encoded fields joined by the delimiter, plus the line ending. A record
    /// of a single empty field is written `""`, so it is not an empty line. A record of no
    /// fields fails `malformed{empty_record}`, as [`Dialect::decode_record`] fails on the
    /// empty line it would give, so encoding and decoding stay inverse.
    pub fn encode_record(&self, fields: &[&str]) -> Result<String, IoError> {
        let mut out = String::new();
        match fields {
            [] => return Err(malformed(0, "empty_record")),
            [""] => {
                out.push_str("\"\"");
                out.push_str(self.line_ending.terminator());
                return Ok(out);
            }
            _ => {}
        }
        for (i, f) in fields.iter().enumerate() {
            if i > 0 {
                out.push(char::from(self.delimiter.byte()));
            }
            out.push_str(&self.encode_field(f));
        }
        out.push_str(self.line_ending.terminator());
        Ok(out)
    }

    /// Splits one record (without its terminator; quoted fields may hold CR and LF) into
    /// fields. Strict RFC 4180: a `"` may only open a field or appear doubled inside a
    /// quoted field; a closing quote must be followed by the delimiter or the end; CR and
    /// LF are only allowed inside quotes. Errors are `malformed` at the byte offset within
    /// `record`, with reasons `empty_record`, `bare_quote`, `unterminated_quote`,
    /// `text_after_quote`, `unquoted_line_break` or `too_many_fields`.
    pub fn decode_record(&self, record: &str) -> Result<Vec<String>, IoError> {
        let b = record.as_bytes();
        if b.is_empty() {
            return Err(malformed(0, "empty_record"));
        }
        let d = self.delimiter.byte();
        let mut fields: Vec<String> = Vec::new();
        let mut i = 0usize;
        loop {
            if fields.len() >= MAX_FIELDS {
                return Err(malformed(i, "too_many_fields"));
            }
            let start = i;
            let mut more = false;
            if b.get(i) == Some(&b'"') {
                i += 1;
                let mut buf: Vec<u8> = Vec::new();
                loop {
                    match b.get(i) {
                        None => return Err(malformed(start, "unterminated_quote")),
                        Some(&b'"') => {
                            if b.get(i + 1) == Some(&b'"') {
                                buf.push(b'"');
                                i += 2;
                            } else {
                                i += 1;
                                break;
                            }
                        }
                        Some(&c) => {
                            buf.push(c);
                            i += 1;
                        }
                    }
                }
                match b.get(i) {
                    None => {}
                    Some(&c) if c == d => {
                        i += 1;
                        more = true;
                    }
                    Some(_) => return Err(malformed(i, "text_after_quote")),
                }
                // Splits happen only at ASCII bytes, so `buf` is valid UTF-8.
                fields.push(String::from_utf8(buf).map_err(|_| malformed(start, "invalid_utf8"))?);
            } else {
                loop {
                    match b.get(i) {
                        None => break,
                        Some(&c) if c == d => {
                            more = true;
                            break;
                        }
                        Some(&b'"') => return Err(malformed(i, "bare_quote")),
                        Some(&b'\r') | Some(&b'\n') => return Err(malformed(i, "unquoted_line_break")),
                        Some(_) => i += 1,
                    }
                }
                let text = record.get(start..i).ok_or_else(|| malformed(start, "invalid_utf8"))?;
                fields.push(text.to_owned());
                if more {
                    i += 1;
                }
            }
            if !more {
                return Ok(fields);
            }
        }
    }

    /// Converts decimal text in this dialect's mark to `.` form. The grammar is
    /// `[+-]? digit+ (mark digit+)? ([eE] [+-]? digit+)?` with ASCII digits only: no
    /// thousands separators, no spaces, no leading or trailing mark. Errors are
    /// `malformed` with reasons `bad_decimal`, `wrong_decimal_mark` (the other mark was
    /// used) or `decimal_too_long`.
    pub fn to_point_decimal(&self, text: &str) -> Result<String, IoError> {
        check_decimal(text, self.decimal_mark.byte())?;
        Ok(text.replace(char::from(self.decimal_mark.byte()), "."))
    }

    /// Converts `.`-form decimal text (same grammar) to this dialect's mark.
    pub fn from_point_decimal(&self, text: &str) -> Result<String, IoError> {
        check_decimal(text, b'.')?;
        Ok(text.replace('.', self.decimal_mark.as_str()))
    }
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "delimiter={} decimal_mark={} header={} quote={QUOTE} encoding={ENCODING} line_ending={}",
            self.delimiter.as_str(),
            self.decimal_mark.as_str(),
            self.header,
            self.line_ending.as_str()
        )
    }
}

fn digits(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    i
}

fn check_decimal(text: &str, mark: u8) -> Result<(), IoError> {
    let b = text.as_bytes();
    if b.len() > MAX_DECIMAL_BYTES {
        return Err(malformed(MAX_DECIMAL_BYTES, "decimal_too_long"));
    }
    let other = if mark == b'.' { b',' } else { b'.' };
    let mut i = 0usize;
    if matches!(b.first(), Some(&b'+') | Some(&b'-')) {
        i = 1;
    }
    let j = digits(b, i);
    if j == i {
        return Err(malformed(i, if b.get(i) == Some(&other) { "wrong_decimal_mark" } else { "bad_decimal" }));
    }
    i = j;
    if b.get(i) == Some(&mark) {
        let j = digits(b, i + 1);
        if j == i + 1 {
            return Err(malformed(i + 1, "bad_decimal"));
        }
        i = j;
    } else if b.get(i) == Some(&other) {
        return Err(malformed(i, "wrong_decimal_mark"));
    }
    if matches!(b.get(i), Some(&b'e') | Some(&b'E')) {
        i += 1;
        if matches!(b.get(i), Some(&b'+') | Some(&b'-')) {
            i += 1;
        }
        let j = digits(b, i);
        if j == i {
            return Err(malformed(i, "bad_decimal"));
        }
        i = j;
    }
    if i != b.len() {
        return Err(malformed(i, "bad_decimal"));
    }
    Ok(())
}

/// `bytes` without one leading UTF-8 byte-order mark.
pub fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(BOM).unwrap_or(bytes)
}

/// Table text: one leading BOM is dropped, the rest must be valid UTF-8 (`malformed`
/// with reason `invalid_utf8` at the first bad byte, counted from the start of `bytes`).
pub fn decode_utf8(bytes: &[u8]) -> Result<&str, IoError> {
    let body = strip_bom(bytes);
    let skipped = bytes.len() - body.len();
    std::str::from_utf8(body).map_err(|e| malformed(skipped + e.valid_up_to(), "invalid_utf8"))
}
