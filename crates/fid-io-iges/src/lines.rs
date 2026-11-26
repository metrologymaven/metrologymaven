//! Line splitting, form detection, section classification, sequence continuity and the
//! Terminate counts (REQ-IO.CAD.IGES.SECT-004).

use crate::error::{IgesError, Section};
use fid_io_reg::{Budget, Position};

/// Columns per line, without the line ending.
pub(crate) const WIDTH: usize = 80;
/// Index of the section letter (column 73).
const LETTER: usize = 72;

/// One 80-column line.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Line<'a> {
    /// The 80 bytes, all printable ASCII.
    pub data: &'a [u8],
    /// 1-based line number in the input.
    pub line_no: u32,
    /// Byte offset of the first column.
    pub offset: u64,
}

impl Line<'_> {
    /// The position of 0-based column `col`.
    pub fn at(&self, col: usize) -> Position {
        let c = u32::try_from(col).unwrap_or(u32::MAX).saturating_add(1);
        let byte = self.offset.saturating_add(u64::try_from(col).unwrap_or(u64::MAX));
        Position::line_col(byte, self.line_no, c)
    }

    /// Columns `from..to` (0-based, `to <= 80`).
    pub fn cols(&self, from: usize, to: usize) -> &[u8] {
        self.data.get(from..to).unwrap_or(&[])
    }
}

/// The lines of each section, in order.
#[derive(Debug, Default)]
pub(crate) struct Sections<'a> {
    pub s: Vec<Line<'a>>,
    pub g: Vec<Line<'a>>,
    pub d: Vec<Line<'a>>,
    pub p: Vec<Line<'a>>,
}

impl<'a> Sections<'a> {
    fn of(&mut self, s: Section) -> Option<&mut Vec<Line<'a>>> {
        match s {
            Section::Start => Some(&mut self.s),
            Section::Global => Some(&mut self.g),
            Section::Directory => Some(&mut self.d),
            Section::Parameter => Some(&mut self.p),
            Section::Terminate => None,
        }
    }
}

/// Splits `input` into raw lines: `(start, end, line number)` with `end` excluding the line
/// ending (`\n`, `\r\n` or a lone `\r`). A final line without an ending is included.
struct RawLines<'a> {
    input: &'a [u8],
    pos: usize,
    line_no: u32,
}

impl Iterator for RawLines<'_> {
    type Item = (usize, usize, u32);

    fn next(&mut self) -> Option<(usize, usize, u32)> {
        let rest = self.input.get(self.pos..)?;
        if rest.is_empty() {
            return None;
        }
        let start = self.pos;
        let len = rest.iter().position(|&b| b == b'\n' || b == b'\r').unwrap_or(rest.len());
        let end = start + len;
        let ending = match (rest.get(len), rest.get(len + 1)) {
            (Some(b'\r'), Some(b'\n')) => 2,
            (Some(_), _) => 1,
            (None, _) => 0,
        };
        self.pos = end + ending;
        self.line_no = self.line_no.saturating_add(1);
        Some((start, end, self.line_no))
    }
}

fn pos_of(offset: usize, line_no: u32) -> Position {
    Position::line_col(u64::try_from(offset).unwrap_or(u64::MAX), line_no, 1)
}

/// Rejects the compressed ASCII and binary forms by their first line
/// [unverified: recognition heuristics]: a flag line (`F` in column 73) means the
/// compressed form; a first byte `B` on a first line that is not an 80-column Start line
/// means the binary form.
fn check_form(input: &[u8]) -> Result<(), IgesError> {
    let first_len = input.iter().position(|&b| b == b'\n' || b == b'\r').unwrap_or(input.len());
    let first = input.get(..first_len).unwrap_or(&[]);
    if first.get(LETTER) == Some(&b'F') {
        return Err(IgesError::FormUnsupported { at: pos_of(LETTER, 1), form: "compressed" });
    }
    if input.first() == Some(&b'B') && (first.len() != WIDTH || first.get(LETTER) != Some(&b'S')) {
        return Err(IgesError::FormUnsupported { at: pos_of(0, 1), form: "binary" });
    }
    Ok(())
}

/// Parses a right-justified unsigned field (leading spaces or zeros allowed, at least one digit).
pub(crate) fn unsigned(field: &[u8]) -> Option<u32> {
    let t = trim(field);
    if t.is_empty() || !t.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(t).ok()?.parse::<u32>().ok()
}

/// `field` without leading and trailing spaces.
pub(crate) fn trim(field: &[u8]) -> &[u8] {
    let a = field.iter().position(|&b| b != b' ').unwrap_or(field.len());
    let b = field.iter().rposition(|&b| b != b' ').map_or(a, |i| i + 1);
    field.get(a..b).unwrap_or(&[])
}

/// Splits, classifies and checks every line. Each line is charged to the budget's
/// allocation counter at twice its in-memory size (the `Vec` may double).
pub(crate) fn split<'a>(input: &'a [u8], budget: &mut Budget) -> Result<Sections<'a>, IgesError> {
    check_form(input)?;
    let line_cost = u64::try_from(std::mem::size_of::<Line<'_>>()).unwrap_or(u64::MAX).saturating_mul(2);
    let mut out = Sections::default();
    let mut current = Section::Start;
    let mut terminate: Option<Line<'a>> = None;
    let mut last_line = 0;
    for (start, end, line_no) in (RawLines { input, pos: 0, line_no: 0 }) {
        last_line = line_no;
        let data = input.get(start..end).unwrap_or(&[]);
        let offset = u64::try_from(start).unwrap_or(u64::MAX);
        if terminate.is_some() {
            if data.is_empty() {
                continue;
            }
            return Err(IgesError::malformed(pos_of(start, line_no), "data_after_terminate"));
        }
        if data.len() != WIDTH {
            let col = data.len().min(WIDTH);
            return Err(IgesError::malformed(Line { data, line_no, offset }.at(col), "line_length"));
        }
        let line = Line { data, line_no, offset };
        if let Some(i) = data.iter().position(|&b| !(b' '..=b'~').contains(&b)) {
            return Err(IgesError::malformed(line.at(i), "non_ascii"));
        }
        let letter = data.get(LETTER).copied().unwrap_or(b' ');
        if letter == b'F' {
            return Err(IgesError::FormUnsupported { at: line.at(LETTER), form: "compressed" });
        }
        let section =
            Section::from_letter(letter).ok_or_else(|| IgesError::malformed(line.at(LETTER), "section_letter"))?;
        if section.index() < current.index() {
            return Err(IgesError::malformed(line.at(LETTER), "section_order"));
        }
        current = section;
        let seq = unsigned(line.cols(LETTER + 1, WIDTH));
        if section == Section::Terminate {
            if seq != Some(1) {
                return Err(IgesError::malformed(line.at(LETTER + 1), "sequence"));
            }
            terminate = Some(line);
            continue;
        }
        let lines = match out.of(section) {
            Some(v) => v,
            None => return Err(IgesError::malformed(line.at(LETTER), "section_letter")),
        };
        let want = u32::try_from(lines.len()).ok().and_then(|n| n.checked_add(1));
        if seq.is_none() || seq != want {
            return Err(IgesError::malformed(line.at(LETTER + 1), "sequence"));
        }
        budget.charge_alloc(line_cost, line.at(0))?;
        lines.push(line);
    }
    let Some(t) = terminate else {
        let end = Position::line_col(u64::try_from(input.len()).unwrap_or(u64::MAX), last_line.max(1), 1);
        return Err(IgesError::TruncatedFile { section: Section::Terminate, at: end });
    };
    check_terminate(&t, &out)?;
    if out.g.is_empty() {
        return Err(IgesError::malformed(t.at(0), "missing_global"));
    }
    if out.d.len() % 2 != 0 {
        let at = out.d.last().map_or(t.at(0), |l| l.at(0));
        return Err(IgesError::malformed(at, "directory_odd"));
    }
    Ok(out)
}

/// Checks the Terminate counts (columns 1-32: `S`, `G`, `D`, `P`, each with a 7-digit count)
/// against the section lengths; the first disagreement fails `truncated_file{section}`.
fn check_terminate(t: &Line<'_>, s: &Sections<'_>) -> Result<(), IgesError> {
    let actual = [s.s.len(), s.g.len(), s.d.len(), s.p.len()];
    for (k, section) in Section::ALL.into_iter().take(4).enumerate() {
        let col = 8 * k;
        if t.data.get(col) != Some(&section.letter()) {
            return Err(IgesError::malformed(t.at(col), "terminate_format"));
        }
        let count = unsigned(t.cols(col + 1, col + 8))
            .ok_or_else(|| IgesError::malformed(t.at(col + 1), "terminate_format"))?;
        let n = actual.get(k).copied().unwrap_or(0);
        if usize::try_from(count).ok() != Some(n) {
            return Err(IgesError::TruncatedFile { section, at: t.at(col) });
        }
    }
    Ok(())
}
