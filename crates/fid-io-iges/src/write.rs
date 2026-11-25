//! The deterministic fixed-column writer.
//!
//! Layout (fixed, so equal files give equal bytes):
//!
//! - every line is 80 columns followed by `\n`: data in columns 1-72, the section letter
//!   in column 73 and the sequence number, zero-padded to 7 digits, in columns 74-80;
//! - Start lines left-justified;
//! - the Global section and each parameter record are a stream of tokens, one per
//!   parameter, each followed by its delimiter (the parameter delimiter, or the record
//!   delimiter after the last). Integers and pointers are written in decimal, reals as
//!   their lexeme, strings as Hollerith constants `<n>H<text>`, defaulted parameters as
//!   nothing. The Global stream starts with `1H<pd><pd>1H<rd>` and then the parameter
//!   delimiter (the record delimiter if there are no further parameters); a record starts
//!   with the entity type number. Tokens are packed greedily into 72 (Global) or 64
//!   (Parameter Data) columns: a token that does not fit on the current line starts a new
//!   one, and a token longer than the width is cut into full-width pieces;
//! - Parameter Data lines carry the entry's sequence number right-justified in columns
//!   65-72; the directory's parameter pointers and line counts are recomputed;
//! - directory fields right-justified in 8 columns, the status as four two-digit numbers;
//! - the Terminate line `S<7 digits>G<7 digits>D<7 digits>P<7 digits>`.

use crate::error::IgesError;
use crate::{printable, DirEntry, IgesFile, IgesParam};

const SEQ_MAX: usize = 9_999_999;

fn unwritable(reason: &'static str, de: Option<u32>) -> IgesError {
    IgesError::Unwritable { reason, de }
}

/// Packs tokens into lines of at most `width` columns (see the module documentation).
fn pack(tokens: &[String], width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for tok in tokens {
        if cur.len() + tok.len() <= width {
            cur.push_str(tok);
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        let mut rest = tok.as_str();
        while rest.len() > width {
            let (head, tail) = rest.split_at(width);
            lines.push(head.to_owned());
            rest = tail;
        }
        cur.push_str(rest);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// One parameter as text (without its delimiter).
fn render(p: &IgesParam, de: Option<u32>) -> Result<String, IgesError> {
    Ok(match p {
        IgesParam::Int(v) => v.to_string(),
        IgesParam::Ptr(v) => v.to_string(),
        IgesParam::Real(v, lexeme) => {
            if lexeme.value().map(f64::to_bits) != Some(v.to_bits()) {
                return Err(unwritable("real_lexeme_mismatch", de));
            }
            lexeme.as_str().to_owned()
        }
        IgesParam::Str(t) => format!("{}H{}", t.len(), t.raw()),
        IgesParam::Default => String::new(),
    })
}

/// The tokens of `values`, each followed by `pd`, the last by `rd`.
fn tokens(values: &[IgesParam], pd: u8, rd: u8, de: Option<u32>, out: &mut Vec<String>) -> Result<(), IgesError> {
    for (i, v) in values.iter().enumerate() {
        let mut t = render(v, de)?;
        t.push(char::from(if i + 1 < values.len() { pd } else { rd }));
        out.push(t);
    }
    Ok(())
}

fn push_line(out: &mut Vec<u8>, data: &str, letter: u8, seq: usize) -> Result<(), IgesError> {
    if data.len() > 72 || !data.bytes().all(printable) {
        return Err(unwritable("line_content", None));
    }
    if seq > SEQ_MAX {
        return Err(unwritable("too_many_lines", None));
    }
    out.extend_from_slice(format!("{data:<72}").as_bytes());
    out.push(letter);
    out.extend_from_slice(format!("{seq:07}\n").as_bytes());
    Ok(())
}

/// An integer in an 8-column field.
fn int8(v: i64, de: u32) -> Result<String, IgesError> {
    if !(-9_999_999..=99_999_999).contains(&v) {
        return Err(unwritable("de_field_width", Some(de)));
    }
    Ok(format!("{v:>8}"))
}

/// A text field (label or reserved field) in 8 columns, right-justified. The parser trims
/// spaces, so a value with a leading or trailing space could not be read back unchanged and
/// is refused.
fn text8(t: &crate::IgesText, de: u32) -> Result<String, IgesError> {
    let raw = t.raw();
    if t.len() > 8 || raw.starts_with(' ') || raw.ends_with(' ') {
        return Err(unwritable("de_text_width", Some(de)));
    }
    Ok(format!("{:>8}", t.raw()))
}

fn de_lines(e: &DirEntry, seq: u32, ptr: usize, count: usize) -> Result<(String, String), IgesError> {
    let s = &e.status;
    if [s.blank, s.subordinate, s.entity_use, s.hierarchy].iter().any(|&v| v > 99) {
        return Err(unwritable("de_status", Some(seq)));
    }
    let ptr = i64::try_from(ptr).unwrap_or(i64::MAX);
    let count = i64::try_from(count).unwrap_or(i64::MAX);
    let mut l1 = String::new();
    for v in [e.entity_type, 0, e.structure, e.line_font, e.level, e.view, e.transform, e.label_display]
        .iter()
        .enumerate()
        .map(|(k, &v)| if k == 1 { ptr } else { i64::from(v) })
    {
        l1.push_str(&int8(v, seq)?);
    }
    l1.push_str(&format!("{:02}{:02}{:02}{:02}", s.blank, s.subordinate, s.entity_use, s.hierarchy));
    let mut l2 = String::new();
    for v in [i64::from(e.entity_type), i64::from(e.line_weight), i64::from(e.color), count, i64::from(e.form)] {
        l2.push_str(&int8(v, seq)?);
    }
    l2.push_str(&text8(&e.reserved1, seq)?);
    l2.push_str(&text8(&e.reserved2, seq)?);
    l2.push_str(&text8(&e.label, seq)?);
    l2.push_str(&int8(i64::from(e.subscript), seq)?);
    Ok((l1, l2))
}

/// Writes `file` in the fixed-column ASCII form (layout in the module documentation).
/// Fails with `unwritable` when a value does not fit the form: bad delimiters, directory
/// keys other than 1, 3, 5, ..., a missing parameter list, a real whose lexeme does not
/// parse to its value, a field too wide, or more than 9,999,999 lines in a section.
/// `file.units` is not written; the Global unit parameters carry the unit.
pub fn write(file: &IgesFile) -> Result<Vec<u8>, IgesError> {
    let g = &file.global;
    let (pd, rd) = (g.param_delim, g.record_delim);
    let delim_ok = |c: u8| c.is_ascii_punctuation() && !matches!(c, b'+' | b'-' | b'.');
    if pd == rd || !delim_ok(pd) || !delim_ok(rd) {
        return Err(unwritable("global_delimiter", None));
    }
    let mut out = Vec::new();
    for (i, s) in file.start.iter().enumerate() {
        push_line(&mut out, s.raw(), b'S', i + 1)?;
    }
    let mut gt = vec![format!("1H{}{}", char::from(pd), char::from(pd))];
    gt.push(format!("1H{}{}", char::from(rd), char::from(if g.params.is_empty() { rd } else { pd })));
    tokens(&g.params, pd, rd, None, &mut gt)?;
    let g_lines = pack(&gt, 72);
    for (i, l) in g_lines.iter().enumerate() {
        push_line(&mut out, l, b'G', i + 1)?;
    }
    // Parameter Data first, to know every entry's pointer and line count.
    let mut p_out = Vec::new();
    let mut d_out = Vec::new();
    let mut p_seq = 0usize;
    for (k, (&seq, e)) in file.directory.iter().enumerate() {
        let want = k.checked_mul(2).and_then(|k| k.checked_add(1));
        if usize::try_from(seq).ok() != want {
            return Err(unwritable("directory_sequence", Some(seq)));
        }
        let params = file.params.get(&seq).ok_or_else(|| unwritable("params_missing", Some(seq)))?;
        let mut pt = Vec::new();
        tokens(
            &[IgesParam::Int(i64::from(e.entity_type))],
            pd,
            if params.is_empty() { rd } else { pd },
            Some(seq),
            &mut pt,
        )?;
        tokens(params, pd, rd, Some(seq), &mut pt)?;
        let lines = pack(&pt, 64);
        let ptr = p_seq + 1;
        for l in &lines {
            p_seq += 1;
            push_line(&mut p_out, &format!("{l:<64}{seq:>8}"), b'P', p_seq)?;
        }
        let (l1, l2) = de_lines(e, seq, ptr, lines.len())?;
        push_line(&mut d_out, &l1, b'D', 2 * k + 1)?;
        push_line(&mut d_out, &l2, b'D', 2 * k + 2)?;
    }
    if file.params.keys().any(|k| !file.directory.contains_key(k)) {
        return Err(unwritable("params_without_entry", None));
    }
    out.extend_from_slice(&d_out);
    out.extend_from_slice(&p_out);
    let t = format!("S{:07}G{:07}D{:07}P{:07}", file.start.len(), g_lines.len(), 2 * file.directory.len(), p_seq);
    push_line(&mut out, &t, b'T', 1)?;
    Ok(out)
}
