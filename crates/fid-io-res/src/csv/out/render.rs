//! Rendering rows into table bytes through a column map.

use super::records::{check_rows, ResultRow};
use super::OutError;
use crate::csv::map::{ColumnMap, Dialect, Field, FieldType, Layout, Scope, MAX_FIELDS, WIDE_SEPARATOR};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Leading characters that make a spreadsheet read a text cell as a formula
/// (REQ-IO.RES.CSV.OUT-003).
pub const FORMULA_TRIGGERS: &[char] = &['=', '+', '-', '@', '\t', '\r'];
/// What is put in front of a text cell that starts with a [`FORMULA_TRIGGERS`] character.
pub const NEUTRALISER: char = '\'';

/// A text cell as written: with a leading [`NEUTRALISER`] when it starts with one of
/// [`FORMULA_TRIGGERS`], otherwise unchanged. Never applied to decimal fields.
pub fn neutralise(text: &str) -> Cow<'_, str> {
    if text.starts_with(FORMULA_TRIGGERS) {
        let mut s = String::with_capacity(text.len().saturating_add(1));
        s.push(NEUTRALISER);
        s.push_str(text);
        Cow::Owned(s)
    } else {
        Cow::Borrowed(text)
    }
}

/// Drops the [`NEUTRALISER`] that [`neutralise`] added, for readers of exported tables.
/// Not an exact inverse: text that itself starts with `'` and a trigger character (which
/// [`neutralise`] leaves alone) loses its apostrophe too.
pub fn unneutralise(cell: &str) -> &str {
    match cell.strip_prefix(NEUTRALISER) {
        Some(rest) if rest.starts_with(FORMULA_TRIGGERS) => rest,
        _ => cell,
    }
}

/// One written column: a map column (its position in [`ColumnMap::columns`]) or a gap.
pub(crate) type Slot = Option<usize>;

/// Places the map columns at `positions` into written columns: a column with an `index`
/// goes to that index; the others fill the lowest free places in map order; places left
/// over are empty gap columns. Indexes are unique (the map checked), so this never
/// collides.
pub(crate) fn slots(map: &ColumnMap, positions: &[usize]) -> Vec<Slot> {
    let cols = map.columns();
    let index_of = |p: usize| cols.get(p).and_then(|c| c.index).map(|i| i as usize);
    let max_index = positions.iter().filter_map(|p| index_of(*p)).max();
    let width = positions.len().max(max_index.map_or(0, |i| i.saturating_add(1)));
    let mut out: Vec<Slot> = vec![None; width];
    for &p in positions {
        if let Some(slot) = index_of(p).and_then(|i| out.get_mut(i)) {
            *slot = Some(p);
        }
    }
    let mut free = 0usize;
    for &p in positions {
        if cols.get(p).is_some_and(|c| c.index.is_none()) {
            while out.get(free).is_some_and(Option::is_some) {
                free += 1;
            }
            if let Some(slot) = out.get_mut(free) {
                *slot = Some(p);
            }
        }
    }
    out
}

/// The header text a map column is written with: its `header`, or the field's wire name
/// for an index-only column.
pub(crate) fn header_of(map: &ColumnMap, position: usize) -> &str {
    match map.header_text(position) {
        Some(h) => h,
        None => map.columns().get(position).map_or("", |c| c.field.as_str()),
    }
}

/// Map positions of the columns of a given wide-layout scope, in map order.
pub(crate) fn scope_positions(map: &ColumnMap, scope: Scope) -> Vec<usize> {
    map.columns().iter().enumerate().filter(|(_, c)| c.field.wide_scope() == scope).map(|(p, _)| p).collect()
}

/// Rows in export order: by characteristic ID, then part ID (absent first), then result
/// ID, each compared byte-wise. Paired with their input positions.
pub(crate) fn ordered(rows: &[ResultRow]) -> Vec<(usize, &ResultRow)> {
    let mut order: Vec<(usize, &ResultRow)> = rows.iter().enumerate().collect();
    order.sort_by(|(_, a), (_, b)| {
        (a.characteristic(), a.part(), a.result_id()).cmp(&(b.characteristic(), b.part(), b.result_id()))
    });
    order
}

/// A rendered table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The file bytes (UTF-8, no byte-order mark).
    pub bytes: Vec<u8>,
    /// Data rows written (results in long layout, parts in wide layout).
    pub rows: u64,
}

fn value_cell<'a>(d: &Dialect, field: Field, value: Option<&'a str>) -> Result<Cow<'a, str>, OutError> {
    match value {
        None => Ok(Cow::Borrowed("")),
        Some(v) => match field.field_type() {
            FieldType::Decimal => d.from_point_decimal(v).map(Cow::Owned).map_err(OutError::Encode),
            FieldType::Text | FieldType::Enum(_) => Ok(neutralise(v)),
        },
    }
}

fn push_record(out: &mut String, d: &Dialect, cells: &[Cow<'_, str>]) -> Result<(), OutError> {
    let refs: Vec<&str> = cells.iter().map(|c| c.as_ref()).collect();
    out.push_str(&d.encode_record(&refs).map_err(OutError::Encode)?);
    Ok(())
}

/// Renders `rows` through `map` (REQ-IO.RES.CSV.OUT-001, -003, -005). Byte-deterministic:
/// the output depends only on the rows (not their input order) and the map.
///
/// - Rows are checked ([`check_rows`]); none at all fails `no_results`.
/// - Decimal cells are the authored text with only the decimal mark changed; text and
///   enumeration cells (also header cells) go through [`neutralise`]; then RFC 4180
///   quoting and the map's line ending, with a terminator after every record.
/// - Long layout: one row per result in [`ordered`] order, columns per [`slots`].
/// - Wide layout: one row per part (byte-wise order, absent part first). Part-scope
///   columns come first (per [`slots`]), then one group per characteristic (byte-wise
///   order) holding every characteristic-scope column in map order, headed
///   `<characteristic id>:<header>`. A part-scope value is the one non-empty value the
///   part's results agree on. Reasons (`records_invalid`): `wide_duplicate` (two results
///   of one part and characteristic), `wide_part_conflict` (results of one part disagree
///   on a part-scope value), `wide_characteristic_separator` (a characteristic ID holds
///   `:`), `wide_too_many_columns` (more than [`MAX_FIELDS`] columns, which no reader
///   of this crate would accept; checked before any row is built).
/// - A map without `record_status` refuses any transcribed result
///   (`transcription_status_missing`): transcribed values must never look measured.
pub fn render(rows: &[ResultRow], map: &ColumnMap) -> Result<Rendered, OutError> {
    check_rows(rows)?;
    if rows.is_empty() {
        return Err(OutError::NoResults);
    }
    if !map.has_field(Field::RecordStatus) {
        if let Some(t) = rows.iter().find(|r| r.is_transcribed()) {
            return Err(OutError::TranscriptionStatusMissing { result: t.result_id().to_owned() });
        }
    }
    let order = ordered(rows);
    let d = map.dialect();
    let cols = map.columns();
    let mut out = String::new();
    let mut written = 0u64;
    match map.layout() {
        Layout::Long => {
            let positions: Vec<usize> = (0..cols.len()).collect();
            let slots = slots(map, &positions);
            if d.header() {
                let cells: Vec<Cow<'_, str>> =
                    slots.iter().map(|s| s.map_or(Cow::Borrowed(""), |p| neutralise(header_of(map, p)))).collect();
                push_record(&mut out, d, &cells)?;
            }
            for &(_, row) in &order {
                let mut cells = Vec::with_capacity(slots.len());
                for s in &slots {
                    cells.push(match s.and_then(|p| cols.get(p)) {
                        None => Cow::Borrowed(""),
                        Some(c) => value_cell(d, c.field, row.get(c.field))?,
                    });
                }
                push_record(&mut out, d, &cells)?;
                written += 1;
            }
        }
        Layout::Wide => {
            let part_slots = slots(map, &scope_positions(map, Scope::Part));
            let group = scope_positions(map, Scope::Characteristic);
            let mut chars: Vec<&str> = order.iter().map(|(_, r)| r.characteristic()).collect();
            chars.dedup();
            if chars.iter().any(|c| c.contains(WIDE_SEPARATOR)) {
                return Err(OutError::Records { reason: "wide_characteristic_separator", record: None });
            }
            let width = chars.len().saturating_mul(group.len()).saturating_add(part_slots.len());
            if width > MAX_FIELDS {
                return Err(OutError::Records { reason: "wide_too_many_columns", record: None });
            }
            let mut parts: BTreeMap<&str, BTreeMap<&str, (usize, &ResultRow)>> = BTreeMap::new();
            for &(n, row) in &order {
                if parts.entry(row.part()).or_default().insert(row.characteristic(), (n, row)).is_some() {
                    return Err(OutError::Records { reason: "wide_duplicate", record: Some(n) });
                }
            }
            if d.header() {
                let mut cells: Vec<Cow<'_, str>> =
                    part_slots.iter().map(|s| s.map_or(Cow::Borrowed(""), |p| neutralise(header_of(map, p)))).collect();
                for c in &chars {
                    for p in &group {
                        let h = format!("{c}{WIDE_SEPARATOR}{}", header_of(map, *p));
                        cells.push(Cow::Owned(neutralise(&h).into_owned()));
                    }
                }
                push_record(&mut out, d, &cells)?;
            }
            for by_char in parts.values() {
                let mut cells = Vec::with_capacity(width);
                for s in &part_slots {
                    cells.push(match s.and_then(|p| cols.get(p)) {
                        None => Cow::Borrowed(""),
                        Some(c) => value_cell(d, c.field, part_value(by_char, c.field)?)?,
                    });
                }
                for c in &chars {
                    let row = by_char.get(c).map(|&(_, r)| r);
                    for p in &group {
                        cells.push(match cols.get(*p) {
                            None => Cow::Borrowed(""),
                            Some(col) => value_cell(d, col.field, row.and_then(|r| r.get(col.field)))?,
                        });
                    }
                }
                push_record(&mut out, d, &cells)?;
                written += 1;
            }
        }
    }
    Ok(Rendered { bytes: out.into_bytes(), rows: written })
}

/// The one non-empty value of a part-scope field among a part's results.
/// A disagreeing row fails `wide_part_conflict` naming its input position.
fn part_value<'a>(
    by_char: &BTreeMap<&str, (usize, &'a ResultRow)>,
    field: Field,
) -> Result<Option<&'a str>, OutError> {
    let mut found: Option<&'a str> = None;
    for &(n, row) in by_char.values() {
        match (found, row.get(field)) {
            (_, None) | (_, Some("")) => {}
            (None, Some(v)) => found = Some(v),
            (Some(a), Some(b)) if a == b => {}
            (Some(_), Some(_)) => return Err(OutError::Records { reason: "wide_part_conflict", record: Some(n) }),
        }
    }
    Ok(found)
}
