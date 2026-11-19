//! Transcription-marking check of an exported table (REQ-IO.RES.CSV.OUT-004).
//!
//! `IO.RES.TRANS.VIEW` (module `trans::view`, its `MarkExtractor` trait and conformance
//! kit) is not built yet. Until it is, this module is the CSV extractor that kit will
//! call: it reads the exported bytes back (RFC 4180, the map's dialect) and checks that
//! every transcribed result has a row carrying `record_status = transcribed` and its
//! source software and version. A missing marking fails `transcription_unmarked` naming
//! the result ID.

use super::records::{ResultRow, TRANSCRIBED};
use super::render::{header_of, neutralise, scope_positions, slots, Slot};
use super::OutError;
use crate::csv::map::{decode_utf8, ColumnMap, Field, Layout, Scope, WIDE_SEPARATOR};
use std::collections::BTreeMap;

/// Splits table text into records at `terminator` outside quoted fields. A final
/// terminator does not start an empty record.
pub fn split_records<'a>(text: &'a str, terminator: &str) -> Vec<&'a str> {
    let b = text.as_bytes();
    let t = terminator.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut quoted = false;
    while i < b.len() {
        if b.get(i) == Some(&b'"') {
            quoted = !quoted;
            i += 1;
            continue;
        }
        if !quoted && !t.is_empty() && b.get(i..).is_some_and(|r| r.starts_with(t)) {
            if let Some(r) = text.get(start..i) {
                out.push(r);
            }
            i += t.len();
            start = i;
            continue;
        }
        i += 1;
    }
    if start < b.len() {
        if let Some(r) = text.get(start..) {
            out.push(r);
        }
    }
    out
}

/// Reads an exported table back into records of fields, in the map's dialect.
pub fn read_table(bytes: &[u8], map: &ColumnMap) -> Result<Vec<Vec<String>>, OutError> {
    let d = map.dialect();
    let text = decode_utf8(bytes).map_err(OutError::Encode)?;
    split_records(text, d.line_ending().terminator())
        .into_iter()
        .map(|r| d.decode_record(r).map_err(OutError::Encode))
        .collect()
}

fn find(slots: &[Slot], map: &ColumnMap, field: Field) -> Option<usize> {
    slots.iter().position(|s| s.and_then(|p| map.columns().get(p)).is_some_and(|c| c.field == field))
}

/// Whether the cell at `at` holds `value` as the exporter writes it (neutralised). The
/// written form is compared, not a reversal of it, so text that itself starts with `'`
/// cannot be mistaken for a neutralised cell.
fn is(rec: &[String], at: Option<usize>, value: &str) -> bool {
    at.and_then(|i| rec.get(i)).is_some_and(|c| c.as_str() == neutralise(value).as_ref())
}

/// Data rows indexed by their written (key, part) cells; a missing column indexes as "".
/// Built once, so the check is O((N + T) log N) for N rows and T transcribed results.
type RowIndex<'a> = BTreeMap<(String, String), Vec<&'a [String]>>;

fn cell_at(rec: &[String], i: Option<usize>) -> String {
    i.and_then(|i| rec.get(i)).map_or_else(String::new, String::clone)
}

fn index_rows(data: &[Vec<String>], key_i: Option<usize>, part_i: Option<usize>) -> RowIndex<'_> {
    let mut index: RowIndex<'_> = BTreeMap::new();
    for rec in data {
        index.entry((cell_at(rec, key_i), cell_at(rec, part_i))).or_default().push(rec.as_slice());
    }
    index
}

/// The rows whose written key and part cells hold `key` and `part` (each compared only
/// when its column exists).
fn candidates<'i, 'a>(
    index: &'i RowIndex<'a>,
    key_i: Option<usize>,
    key: &str,
    part_i: Option<usize>,
    part: &str,
) -> &'i [&'a [String]] {
    let written = |at: Option<usize>, v: &str| if at.is_some() { neutralise(v).into_owned() } else { String::new() };
    index.get(&(written(key_i, key), written(part_i, part))).map_or(&[], Vec::as_slice)
}

/// Whether a row carries the marking of transcribed result `t`.
fn marks(rec: &[String], t: &ResultRow, status_i: Option<usize>, sw_i: Option<usize>, ver_i: Option<usize>) -> bool {
    is(rec, status_i, TRANSCRIBED)
        && is(rec, sw_i, t.get(Field::SourceSoftware).unwrap_or(""))
        && (ver_i.is_none() || is(rec, ver_i, t.get(Field::SourceVersion).unwrap_or("")))
}

/// Checks that every transcribed result in `rows` is marked in `output`, a table written
/// through `map`. A map without `record_status` or `source_software` columns cannot mark
/// anything, so any transcribed result fails.
pub fn check_marked(output: &[u8], map: &ColumnMap, rows: &[ResultRow]) -> Result<(), OutError> {
    let transcribed: Vec<&ResultRow> = rows.iter().filter(|r| r.is_transcribed()).collect();
    if transcribed.is_empty() {
        return Ok(());
    }
    let table = read_table(output, map)?;
    let (header, data): (Option<&Vec<String>>, &[Vec<String>]) = match table.split_first() {
        Some((h, rest)) if map.dialect().header() => (Some(h), rest),
        _ => (None, table.as_slice()),
    };
    let unmarked = |r: &ResultRow| OutError::TranscriptionUnmarked { result: r.result_id().to_owned() };
    match map.layout() {
        Layout::Long => {
            let positions: Vec<usize> = (0..map.columns().len()).collect();
            let s = slots(map, &positions);
            let key = if map.has_field(Field::CharacteristicId) {
                Field::CharacteristicId
            } else {
                Field::CharacteristicName
            };
            let (key_i, part_i) = (find(&s, map, key), find(&s, map, Field::PartId));
            let (status_i, sw_i, ver_i) = (
                find(&s, map, Field::RecordStatus),
                find(&s, map, Field::SourceSoftware),
                find(&s, map, Field::SourceVersion),
            );
            let index = index_rows(data, key_i, part_i);
            for &t in &transcribed {
                let found = candidates(&index, key_i, t.get(key).unwrap_or(""), part_i, t.part());
                let marked = status_i.is_some()
                    && sw_i.is_some()
                    && found.iter().any(|&rec| marks(rec, t, status_i, sw_i, ver_i));
                if !marked {
                    return Err(unmarked(t));
                }
            }
        }
        Layout::Wide => {
            let s = slots(map, &scope_positions(map, Scope::Part));
            let (part_i, sw_i, ver_i) = (
                find(&s, map, Field::PartId),
                find(&s, map, Field::SourceSoftware),
                find(&s, map, Field::SourceVersion),
            );
            let status_header =
                map.columns().iter().position(|c| c.field == Field::RecordStatus).map(|p| header_of(map, p));
            let mut header_index: BTreeMap<&str, usize> = BTreeMap::new();
            for (i, c) in header.map_or(&[][..], |h| h.as_slice()).iter().enumerate() {
                header_index.entry(c.as_str()).or_insert(i);
            }
            let index = index_rows(data, None, part_i);
            for &t in &transcribed {
                let status_i = status_header.and_then(|sh| {
                    let want = format!("{}{WIDE_SEPARATOR}{sh}", t.characteristic());
                    header_index.get(neutralise(&want).as_ref()).copied()
                });
                let found = candidates(&index, None, "", part_i, t.part());
                let marked = status_i.is_some()
                    && sw_i.is_some()
                    && found.iter().any(|&rec| marks(rec, t, status_i, sw_i, ver_i));
                if !marked {
                    return Err(unmarked(t));
                }
            }
        }
    }
    Ok(())
}
