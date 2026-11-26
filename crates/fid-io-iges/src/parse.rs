//! The Global section, directory entries and parameter records.

use crate::error::{Bound, IgesError};
use crate::lines::{self, Line};
use crate::real::{self, Shape};
use crate::unit::resolve_units;
use crate::{DirEntry, GlobalParams, IgesFile, IgesParam, IgesText, Lexeme, ParseOptions, Status};
use fid_io_reg::{Budget, Position};
use std::collections::BTreeMap;
use std::mem::size_of;

/// Data columns of a Global line.
const G_WIDTH: usize = 72;
/// Data columns of a Parameter Data line (columns 65-72 hold the back-pointer).
const P_WIDTH: usize = 64;

/// Budget charge for one parameter: twice its size (the `Vec` may double).
fn param_cost() -> u64 {
    u64::try_from(size_of::<IgesParam>()).unwrap_or(u64::MAX).saturating_mul(2)
}

/// Budget charge for one directory entry stored in the map: its size plus a map node's
/// key and links (counted generously as another entry's size).
fn de_cost() -> u64 {
    u64::try_from(size_of::<DirEntry>() + size_of::<u32>()).unwrap_or(u64::MAX).saturating_mul(2)
}

fn len_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Parses the whole file after its bytes have been charged.
pub(crate) fn file(input: &[u8], budget: &mut Budget, opts: &ParseOptions) -> Result<IgesFile, IgesError> {
    let sections = lines::split(input, budget)?;
    let mut start = Vec::new();
    for l in &sections.s {
        let text = l.cols(0, G_WIDTH);
        let end = text.iter().rposition(|&b| b != b' ').map_or(0, |i| i + 1);
        budget.charge_alloc(len_u64(end).saturating_add(param_cost()), l.at(0))?;
        start.push(IgesText::from_ascii(text.get(..end).unwrap_or(&[])));
    }
    let global = global(&sections.g, budget, opts)?;
    let mut directory = BTreeMap::new();
    for (k, pair) in sections.d.chunks_exact(2).enumerate() {
        let seq = u32::try_from(k).ok().and_then(|k| k.checked_mul(2)).and_then(|k| k.checked_add(1)).unwrap_or(0);
        if let [l1, l2] = pair {
            budget.charge_elements(1, l1.at(0))?;
            budget.charge_alloc(de_cost(), l1.at(0))?;
            directory.insert(seq, dir_entry(l1, l2, seq, sections.p.len())?);
        }
    }
    let mut params = BTreeMap::new();
    for (&seq, de) in &directory {
        params.insert(seq, record(&sections.p, de, seq, &global, budget, opts)?);
    }
    let units = resolve_units(&global, opts.unit)?;
    Ok(IgesFile { start, global, directory, params, units })
}

/// A free-format record: the joined data columns of its lines, read field by field.
struct Record<'a> {
    s: &'a [u8],
    i: usize,
    pd: u8,
    rd: u8,
    max_token: usize,
}

/// A field error at an offset in the joined record.
enum FieldErr {
    Malformed(&'static str, usize),
    Token(usize),
}

impl Record<'_> {
    fn skip_spaces(&mut self) {
        while self.s.get(self.i) == Some(&b' ') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn starts_with(&self, p: &[u8]) -> bool {
        self.s.get(self.i..).is_some_and(|r| r.starts_with(p))
    }

    /// After a value: optional spaces, then a delimiter. Returns true at the record end.
    fn delimiter(&mut self, reason: &'static str) -> Result<bool, FieldErr> {
        self.skip_spaces();
        match self.peek() {
            Some(c) if c == self.pd => {
                self.i += 1;
                Ok(false)
            }
            Some(c) if c == self.rd => {
                self.i += 1;
                Ok(true)
            }
            Some(_) => Err(FieldErr::Malformed(reason, self.i)),
            None => Err(FieldErr::Malformed("missing_record_delimiter", self.i)),
        }
    }

    /// The next field and whether it ended the record.
    fn field(&mut self) -> Result<(IgesParam, bool), FieldErr> {
        self.skip_spaces();
        let start = self.i;
        match self.peek() {
            None => return Err(FieldErr::Malformed("missing_record_delimiter", start)),
            Some(c) if c == self.pd => {
                self.i += 1;
                return Ok((IgesParam::Default, false));
            }
            Some(c) if c == self.rd => {
                self.i += 1;
                return Ok((IgesParam::Default, true));
            }
            Some(_) => {}
        }
        let rest = self.s.get(start..).unwrap_or(&[]);
        let ndig = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        if ndig > 0 && rest.get(ndig) == Some(&b'H') {
            // Hollerith constant: <count>H<count characters>.
            let count = std::str::from_utf8(rest.get(..ndig).unwrap_or(&[]))
                .ok()
                .and_then(|d| d.parse::<usize>().ok())
                .ok_or(FieldErr::Malformed("hollerith_count", start))?;
            let text = rest
                .get(ndig + 1..)
                .and_then(|r| r.get(..count))
                .ok_or(FieldErr::Malformed("hollerith_count", start))?;
            self.i = start + ndig + 1 + count;
            let ended = self.delimiter("after_string")?;
            return Ok((IgesParam::Str(IgesText::from_ascii(text)), ended));
        }
        let len = rest
            .iter()
            .position(|&b| b == self.pd || b == self.rd)
            .ok_or(FieldErr::Malformed("missing_record_delimiter", self.s.len()))?;
        let tok = lines::trim(rest.get(..len).unwrap_or(&[]));
        if tok.len() > self.max_token {
            return Err(FieldErr::Token(start));
        }
        let param = match real::shape(tok) {
            Some(Shape::Int) => std::str::from_utf8(tok)
                .ok()
                .and_then(|t| t.parse::<i64>().ok())
                .map(IgesParam::Int)
                .ok_or(FieldErr::Malformed("integer_range", start))?,
            Some(Shape::Real) => {
                let lexeme = Lexeme::checked(tok).ok_or(FieldErr::Malformed("real_range", start))?;
                let v = lexeme.value().ok_or(FieldErr::Malformed("real_range", start))?;
                IgesParam::Real(v, lexeme)
            }
            None => return Err(FieldErr::Malformed("number", start)),
        };
        self.i = start + len;
        let ended = self.delimiter("number")?;
        Ok((param, ended))
    }
}

/// Joins columns `0..width` of `lines`, charging the buffer.
fn join(lines: &[Line<'_>], width: usize, budget: &mut Budget) -> Result<Vec<u8>, IgesError> {
    let at = lines.first().map_or(Position::default(), |l| l.at(0));
    budget.charge_alloc(len_u64(lines.len()).saturating_mul(len_u64(width)), at)?;
    let mut out = Vec::with_capacity(lines.len().saturating_mul(width));
    for l in lines {
        out.extend_from_slice(l.cols(0, width));
    }
    Ok(out)
}

/// The position of offset `off` of a record joined from `lines` at `width` columns each.
fn pos_in(lines: &[Line<'_>], width: usize, off: usize) -> Position {
    match lines.get(off / width) {
        Some(l) => l.at(off % width),
        None => lines.last().map_or(Position::default(), |l| l.at(width)),
    }
}

fn field_error(e: FieldErr, lines: &[Line<'_>], width: usize, de: Option<u32>, max_token: usize) -> IgesError {
    match e {
        FieldErr::Malformed(reason, off) => {
            IgesError::Malformed { at: pos_in(lines, width, off), reason, de, back_pointer: None }
        }
        FieldErr::Token(off) => {
            IgesError::LimitExceeded { bound: Bound::Token, value: len_u64(max_token), at: pos_in(lines, width, off) }
        }
    }
}

/// Charges one parsed parameter.
fn charge_param(p: &IgesParam, budget: &mut Budget, at: Position) -> Result<(), IgesError> {
    budget.charge_elements(1, at)?;
    let extra = match p {
        IgesParam::Str(t) => t.len(),
        IgesParam::Real(_, l) => l.as_str().len(),
        _ => 0,
    };
    budget.charge_alloc(param_cost().saturating_add(len_u64(extra)), at)?;
    Ok(())
}

/// A delimiter must be printable punctuation that cannot start or continue a number.
fn valid_delimiter(c: u8) -> bool {
    c.is_ascii_punctuation() && !matches!(c, b'+' | b'-' | b'.')
}

/// Parses the Global section: delimiters (parameters 1 and 2), then parameters 3.. up to
/// the record delimiter. Only spaces may follow the record delimiter.
fn global(g: &[Line<'_>], budget: &mut Budget, opts: &ParseOptions) -> Result<GlobalParams, IgesError> {
    let joined = join(g, G_WIDTH, budget)?;
    let mut r = Record { s: &joined, i: 0, pd: b',', rd: b';', max_token: opts.max_token_bytes };
    let bad = |r: &Record<'_>| IgesError::malformed(pos_in(g, G_WIDTH, r.i), "global_delimiter");
    // Parameter 1: `1Hc` followed by c, or defaulted (a leading `,`).
    r.skip_spaces();
    let pd = if r.starts_with(b"1H") {
        r.i += 2;
        let c = r.peek().ok_or_else(|| bad(&r))?;
        r.i += 1;
        r.skip_spaces();
        if r.peek() != Some(c) {
            return Err(bad(&r));
        }
        r.i += 1;
        c
    } else if r.peek() == Some(b',') {
        r.i += 1;
        b','
    } else {
        return Err(bad(&r));
    };
    // Parameter 2: `1Hc` or defaulted (`;`), followed by the parameter delimiter, or by the
    // record delimiter when the section holds no further parameters.
    r.skip_spaces();
    let (rd, mut ended) = if r.starts_with(b"1H") {
        r.i += 2;
        let c = r.peek().ok_or_else(|| bad(&r))?;
        r.i += 1;
        r.skip_spaces();
        let ended = match r.peek() {
            Some(x) if x == pd && x != c => false,
            Some(x) if x == c => true,
            _ => return Err(bad(&r)),
        };
        r.i += 1;
        (c, ended)
    } else {
        let ended = match r.peek() {
            Some(x) if x == pd && x != b';' => false,
            Some(b';') => true,
            _ => return Err(bad(&r)),
        };
        r.i += 1;
        (b';', ended)
    };
    if pd == rd || !valid_delimiter(pd) || !valid_delimiter(rd) {
        return Err(IgesError::malformed(pos_in(g, G_WIDTH, 0), "global_delimiter"));
    }
    r.pd = pd;
    r.rd = rd;
    let mut params = Vec::new();
    while !ended {
        let at = pos_in(g, G_WIDTH, r.i);
        let (p, e) = r.field().map_err(|e| field_error(e, g, G_WIDTH, None, opts.max_token_bytes))?;
        charge_param(&p, budget, at)?;
        params.push(p);
        ended = e;
    }
    if let Some(off) = joined.get(r.i..).and_then(|rest| rest.iter().position(|&b| b != b' ')) {
        return Err(IgesError::malformed(pos_in(g, G_WIDTH, r.i + off), "data_after_record"));
    }
    Ok(GlobalParams { param_delim: pd, record_delim: rd, params })
}

/// A signed integer field of 8 columns; blank reads as 0.
fn int_field(line: &Line<'_>, k: usize, seq: u32) -> Result<i32, IgesError> {
    let t = lines::trim(line.cols(8 * k, 8 * k + 8));
    if t.is_empty() {
        return Ok(0);
    }
    if real::shape(t) == Some(Shape::Int) {
        if let Some(v) = std::str::from_utf8(t).ok().and_then(|s| s.parse::<i32>().ok()) {
            return Ok(v);
        }
    }
    Err(IgesError::malformed_de(line.at(8 * k), "de_field", seq))
}

/// A text field of 8 columns, spaces trimmed.
fn text_field(line: &Line<'_>, k: usize) -> IgesText {
    IgesText::from_ascii(lines::trim(line.cols(8 * k, 8 * k + 8)))
}

/// The status field (columns 65-72 of the first line): blank, or eight digits where
/// leading spaces stand for zeros.
fn status_field(line: &Line<'_>, seq: u32) -> Result<Status, IgesError> {
    let raw = line.cols(64, 72);
    let bad = || IgesError::malformed_de(line.at(64), "de_status", seq);
    let first = raw.iter().position(|&b| b != b' ').unwrap_or(raw.len());
    let digits = raw.get(first..).unwrap_or(&[]);
    if !digits.iter().all(u8::is_ascii_digit) {
        return Err(bad());
    }
    let mut d = [0u8; 8];
    for (slot, &b) in d.iter_mut().skip(first).zip(digits) {
        *slot = b - b'0';
    }
    let two = |i: usize| d.get(i).copied().unwrap_or(0) * 10 + d.get(i + 1).copied().unwrap_or(0);
    Ok(Status { blank: two(0), subordinate: two(2), entity_use: two(4), hierarchy: two(6) })
}

/// Parses one directory entry and checks its pointer range (REQ-IO.CAD.IGES.SECT-001).
fn dir_entry(l1: &Line<'_>, l2: &Line<'_>, seq: u32, p_lines: usize) -> Result<DirEntry, IgesError> {
    let entity_type = int_field(l1, 0, seq)?;
    let ptr = int_field(l1, 1, seq)?;
    let structure = int_field(l1, 2, seq)?;
    let line_font = int_field(l1, 3, seq)?;
    let level = int_field(l1, 4, seq)?;
    let view = int_field(l1, 5, seq)?;
    let transform = int_field(l1, 6, seq)?;
    let label_display = int_field(l1, 7, seq)?;
    let status = status_field(l1, seq)?;
    if int_field(l2, 0, seq)? != entity_type {
        return Err(IgesError::malformed_de(l2.at(0), "de_type_mismatch", seq));
    }
    let line_weight = int_field(l2, 1, seq)?;
    let color = int_field(l2, 2, seq)?;
    let count = int_field(l2, 3, seq)?;
    let form = int_field(l2, 4, seq)?;
    let p_lines = i64::try_from(p_lines).unwrap_or(i64::MAX);
    if ptr < 1 || i64::from(ptr) > p_lines {
        return Err(IgesError::malformed_de(l1.at(8), "param_pointer_out_of_range", seq));
    }
    if count < 1 || i64::from(ptr) - 1 + i64::from(count) > p_lines {
        return Err(IgesError::malformed_de(l2.at(24), "param_pointer_out_of_range", seq));
    }
    Ok(DirEntry {
        entity_type,
        param_ptr: u32::try_from(ptr).unwrap_or(0),
        structure,
        line_font,
        level,
        view,
        transform,
        label_display,
        status,
        line_weight,
        color,
        param_line_count: u32::try_from(count).unwrap_or(0),
        form,
        reserved1: text_field(l2, 5),
        reserved2: text_field(l2, 6),
        label: text_field(l2, 7),
        subscript: int_field(l2, 8, seq)?,
    })
}

/// Parses the parameter record of directory entry `seq` (REQ-IO.CAD.IGES.SECT-002). Text
/// after the record delimiter (comments) is ignored.
fn record(
    p: &[Line<'_>],
    de: &DirEntry,
    seq: u32,
    global: &GlobalParams,
    budget: &mut Budget,
    opts: &ParseOptions,
) -> Result<Vec<IgesParam>, IgesError> {
    let from = usize::try_from(de.param_ptr).unwrap_or(usize::MAX).saturating_sub(1);
    let count = usize::try_from(de.param_line_count).unwrap_or(usize::MAX);
    let lines = from.checked_add(count).and_then(|to| p.get(from..to)).unwrap_or(&[]);
    for l in lines {
        let back = lines::unsigned(l.cols(P_WIDTH, 72))
            .ok_or_else(|| IgesError::malformed_de(l.at(P_WIDTH), "back_pointer_field", seq))?;
        if back != seq {
            return Err(IgesError::Malformed {
                at: l.at(P_WIDTH),
                reason: "back_pointer_mismatch",
                de: Some(seq),
                back_pointer: Some(back),
            });
        }
    }
    let joined = join(lines, P_WIDTH, budget)?;
    let mut r =
        Record { s: &joined, i: 0, pd: global.param_delim, rd: global.record_delim, max_token: opts.max_token_bytes };
    let err = |e| field_error(e, lines, P_WIDTH, Some(seq), opts.max_token_bytes);
    let (first, mut ended) = r.field().map_err(err)?;
    if first.as_int() != Some(i64::from(de.entity_type)) {
        return Err(IgesError::malformed_de(pos_in(lines, P_WIDTH, 0), "param_entity_type", seq));
    }
    let mut params = Vec::new();
    while !ended {
        let at = pos_in(lines, P_WIDTH, r.i);
        let (param, e) = r.field().map_err(err)?;
        charge_param(&param, budget, at)?;
        params.push(param);
        ended = e;
    }
    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Allocation charges are at least the real sizes plus `Vec` growth (brief rule).
    #[test]
    fn costs_cover_the_real_sizes() {
        let size = |n: usize| u64::try_from(n).unwrap_or(0);
        assert!(param_cost() >= 2 * size(size_of::<IgesParam>()));
        assert!(de_cost() >= size(size_of::<DirEntry>() + size_of::<u32>()) * 2);
        assert!(param_cost() >= size(size_of::<IgesText>()));
    }
}
