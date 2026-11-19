//! Transcription-marking check of an exported table (REQ-IO.RES.CSV.OUT-004).
//!
//! `IO.RES.TRANS.VIEW` (module `trans::view`, its `MarkExtractor` trait and conformance
//! kit) is not built yet. Until it is, this module is the CSV extractor that kit will
//! call: it reads the exported bytes back (RFC 4180, the map's dialect) and checks that
//! every transcribed result has a row carrying `record_status = transcribed` and its
//! source software and version. A missing marking fails `transcription_unmarked` naming
//! the result ID.

use super::records::{ResultRow, TRANSCRIBED};
use super::render::{header_of, scope_positions, slots, unneutralise, Slot};
use super::OutError;
use crate::csv::map::{decode_utf8, ColumnMap, Field, Layout, Scope, WIDE_SEPARATOR};

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

fn cell(rec: &[String], at: Option<usize>) -> Option<&str> {
    at.and_then(|i| rec.get(i)).map(|s| unneutralise(s))
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
            for &t in &transcribed {
                let marked = status_i.is_some()
                    && sw_i.is_some()
                    && data.iter().any(|rec| {
                        cell(rec, key_i).unwrap_or("") == t.get(key).unwrap_or("")
                            && (part_i.is_none() || cell(rec, part_i) == Some(t.part()))
                            && cell(rec, status_i) == Some(TRANSCRIBED)
                            && cell(rec, sw_i) == t.get(Field::SourceSoftware)
                            && (ver_i.is_none() || cell(rec, ver_i) == t.get(Field::SourceVersion))
                    });
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
            for &t in &transcribed {
                let status_i = match (header, status_header) {
                    (Some(h), Some(sh)) => {
                        let want = format!("{}{WIDE_SEPARATOR}{sh}", t.characteristic());
                        h.iter().position(|c| unneutralise(c) == want)
                    }
                    _ => None,
                };
                let marked = status_i.is_some()
                    && sw_i.is_some()
                    && data.iter().any(|rec| {
                        (part_i.is_none() || cell(rec, part_i) == Some(t.part()))
                            && cell(rec, status_i) == Some(TRANSCRIBED)
                            && cell(rec, sw_i) == t.get(Field::SourceSoftware)
                            && (ver_i.is_none() || cell(rec, ver_i) == t.get(Field::SourceVersion))
                    });
                if !marked {
                    return Err(unmarked(t));
                }
            }
        }
    }
    Ok(())
}
