//! Suite-wide marker rules of spec node AGENT.REDTEAM.PRED (REQ-AGENT.REDTEAM.PRED-003 to
//! -005), applied by [`crate::load_suite`] and so by `fid-eval suite check`.
//!
//! The predicate schema itself (the six kinds, the marker format, `missing_predicate` and
//! `unknown_predicate`) is parsed per task in `redteam.rs`. The rules here need the whole
//! suite and the payload files inside the start-state archives:
//!
//! - `marker_not_in_payload`: an `entry_with_marker` marker that appears in none of the
//!   task's payload files. A payload file is searched in its archive path (vector
//!   `file_name` carries the marker in the name) and in its bytes. A task without payloads,
//!   a payload file missing from the archive, or an archive that cannot be listed
//!   fail-closed all count as "appears in none".
//! - `marker_in_prompt`: the marker appears in the task prompt after [`normalise`].
//! - `duplicate_marker`: a marker declared by two `entry_with_marker` predicates of the
//!   suite, or a declared marker that appears, after [`normalise`], in a payload file of
//!   another task (two payloads sharing a marker would make a hit untraceable to its source).
//!
//! Presence in the task's own payload is an exact byte match, so it fails closed: an
//! aliased or split copy does not count as present. The two "must not appear" rules match
//! the normalised form, so a disguised copy still counts as appearing. Limits (documented in
//! docs/redteam-predicates.md): compressed, nested or UTF-16 payloads are not decoded, and a
//! marker split by a line break or other kept character is not joined.
//!
//! Every error is reported on the declaring task's `task.json`, so it names the task.

use crate::redteam::{is_marker, PredicateKind};
use crate::task::Task;
use crate::{tar, ErrorCode, FormatError};
use std::collections::{BTreeMap, BTreeSet};

const PREFIX: &[u8] = b"FIDRT-";
/// `FIDRT-` plus 26 characters.
const MARKER_LEN: usize = 32;

/// What the marker rules need from a task's payload files: the exact markers (for
/// presence) and the normalised text of every payload path and file (for leaks).
#[derive(Debug, Default)]
pub(crate) struct PayloadText {
    exact: BTreeSet<String>,
    norm: Vec<Vec<Tok>>,
}

/// The payload text of a task, or why its archive could not be listed.
pub(crate) type Found = Result<PayloadText, String>;

/// One character of normalised text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    /// An ASCII character after case folding and the Crockford aliases.
    Ch(u8),
    /// A Greek or Cyrillic letter or digit: a possible homoglyph of any marker character.
    Wild,
    /// Any other character; it matches nothing.
    Other,
}

/// Characters dropped by [`normalise`]: hyphens (ASCII, soft, Unicode dashes and minus) and
/// zero-width or format characters that render as nothing.
fn ignorable(c: char) -> bool {
    matches!(
        c,
        '-' | '\u{00AD}' | '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}' | '\u{2010}'..='\u{2015}' | '\u{2212}'
    )
}

/// The single marker normalisation (maintainer decision on PR #172), used for the prompt
/// rule and the cross-task leak rule:
///
/// - hyphens and zero-width or format characters are dropped (U+200B to U+200D, U+2060,
///   U+FEFF, U+00AD, U+2010 to U+2015, U+2212);
/// - full-width ASCII (U+FF01 to U+FF5E) maps to ASCII;
/// - ASCII maps to upper case, then the Crockford aliases O to 0 and I, L to 1;
/// - Greek and Cyrillic letters and digits become [`Tok::Wild`], which matches any
///   character, so a homoglyph next to marker-shaped text fails closed;
/// - every other character is kept as [`Tok::Other`] and matches nothing, so a line break
///   or space still splits a marker.
fn normalise(text: &str) -> Vec<Tok> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        let c = match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(u32::from(c) - 0xFEE0).unwrap_or(c),
            _ => c,
        };
        if ignorable(c) {
            continue;
        }
        let tok = match u8::try_from(c) {
            Ok(b) if b.is_ascii() => Tok::Ch(match b.to_ascii_uppercase() {
                b'O' => b'0',
                b'I' | b'L' => b'1',
                other => other,
            }),
            _ if c.is_alphanumeric() && matches!(u32::from(c), 0x0370..=0x03FF | 0x0400..=0x052F) => Tok::Wild,
            _ => Tok::Other,
        };
        out.push(tok);
    }
    out
}

/// Whether normalised `hay` contains normalised `needle` (a [`Tok::Wild`] in `hay`
/// matches any character).
fn contains(hay: &[Tok], needle: &[Tok]) -> bool {
    !needle.is_empty()
        && hay.windows(needle.len()).any(|w| w.iter().zip(needle).all(|(h, n)| h == n || *h == Tok::Wild))
}

/// Whether `marker` appears in `text` after [`normalise`].
#[cfg(test)]
fn appears_in(text: &str, marker: &str) -> bool {
    contains(&normalise(text), &normalise(marker))
}

/// Every marker-shaped string (`FIDRT-` plus 26 Crockford base32 characters) in `bytes`.
/// A longer run of Crockford characters still contains the marker of its first 26.
fn markers_in(bytes: &[u8]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut i = 0usize;
    while i + MARKER_LEN <= bytes.len() {
        if bytes[i..].starts_with(PREFIX) {
            if let Ok(s) = std::str::from_utf8(&bytes[i..i + MARKER_LEN]) {
                if is_marker(s) {
                    out.insert(s.to_string());
                    i += MARKER_LEN;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// The markers in the payload files of `task`, read from its start-state archive bytes.
/// A task without payloads has none, and its archive is not listed.
/// Payload bytes that are not UTF-8 are read lossily for the normalised form.
pub(crate) fn payload_markers(task: &Task, archive: &[u8]) -> Found {
    let mut out = PayloadText::default();
    if task.payloads.is_empty() {
        return Ok(out);
    }
    let files = tar::contents(archive)?;
    for p in &task.payloads {
        for (path, data) in files.iter().filter(|(path, _)| path == p.file.as_str()) {
            out.exact.extend(markers_in(path.as_bytes()));
            out.exact.extend(markers_in(data));
            out.norm.push(normalise(path));
            out.norm.push(normalise(&String::from_utf8_lossy(data)));
        }
    }
    Ok(out)
}

/// The `entry_with_marker` markers of a task with their predicate index.
fn declared(task: &Task) -> impl Iterator<Item = (usize, &str)> {
    task.predicates.iter().enumerate().filter_map(|(i, p)| match &p.kind {
        PredicateKind::EntryWithMarker { marker } => Some((i, marker.as_str())),
        _ => None,
    })
}

/// Applies the marker rules to the parsed tasks of a suite (`tasks` by ID) and the
/// markers found in their payload files (`found` by task ID; a task missing from it has
/// no readable archive). Errors are in task order, then predicate order.
pub(crate) fn check(tasks: &BTreeMap<String, Task>, found: &BTreeMap<String, Found>) -> Vec<FormatError> {
    let mut owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (id, t) in tasks {
        for (_, m) in declared(t) {
            owners.entry(m).or_default().push(id.as_str());
        }
    }
    let mut errs = Vec::new();
    for (id, t) in tasks {
        let file = format!("tasks/{id}/task.json");
        let prompt = normalise(&t.prompt);
        for (i, m) in declared(t) {
            let field = format!("predicates[{i}].marker");
            let nm = normalise(m);
            if contains(&prompt, &nm) {
                errs.push(FormatError::new(
                    ErrorCode::MarkerInPrompt,
                    &file,
                    "prompt",
                    format!("task {id}: marker {m} of predicate {} appears in the prompt", t.predicates[i].id),
                ));
            }
            let not_in = |why: String| FormatError::new(ErrorCode::MarkerNotInPayload, &file, &field, why);
            match found.get(id) {
                Some(Ok(pt)) if pt.exact.contains(m) => {}
                Some(Err(e)) => errs.push(not_in(format!("task {id}: start-state archive cannot be listed: {e}"))),
                _ if t.payloads.is_empty() => {
                    errs.push(not_in(format!("task {id}: marker {m} appears in no payload file (the task has none)")))
                }
                _ => errs.push(not_in(format!("task {id}: marker {m} appears in none of its payload files"))),
            }
            let owner_list = owners.get(m).map(Vec::as_slice).unwrap_or(&[]);
            let mut others: Vec<&str> = owner_list.iter().copied().filter(|o| *o != id.as_str()).collect();
            let repeats_here = owner_list.iter().filter(|o| **o == id.as_str()).count() > 1;
            if repeats_here || !others.is_empty() {
                if repeats_here {
                    others.insert(0, id.as_str());
                }
                others.dedup();
                errs.push(FormatError::new(
                    ErrorCode::DuplicateMarker,
                    &file,
                    &field,
                    format!("task {id}: marker {m} is also declared by task(s) {}", others.join(", ")),
                ));
            }
            let leaked: Vec<&str> = found
                .iter()
                .filter(|(o, _)| o.as_str() != id.as_str() && !owner_list.contains(&o.as_str()))
                .filter(|(_, f)| matches!(f, Ok(pt) if pt.norm.iter().any(|n| contains(n, &nm))))
                .map(|(o, _)| o.as_str())
                .collect();
            if !leaked.is_empty() {
                errs.push(FormatError::new(
                    ErrorCode::DuplicateMarker,
                    &file,
                    &field,
                    format!("task {id}: marker {m} also appears in a payload file of task(s) {}", leaked.join(", ")),
                ));
            }
        }
    }
    errs
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const M: &str = "FIDRT-0123456789ABCDEFGHJKMNPQRS";

    #[test]
    fn scan_finds_markers_by_the_spec_shape() {
        let set = |b: &[u8]| markers_in(b).into_iter().collect::<Vec<_>>();
        assert_eq!(set(format!("x {M} y").as_bytes()), vec![M.to_string()]);
        assert_eq!(set(M.as_bytes()), vec![M.to_string()]);
        // Longer Crockford runs contain the marker of their first 26 characters.
        assert_eq!(set(format!("{M}TV").as_bytes()), vec![M.to_string()]);
        // Too short, a non-Crockford letter, lower case or the wrong prefix: no marker.
        assert!(set(&M.as_bytes()[..31]).is_empty());
        assert!(set(M.replace('S', "U").as_bytes()).is_empty());
        assert!(set(M.to_ascii_lowercase().as_bytes()).is_empty());
        assert!(set(M.replace("FIDRT", "FIDRX").as_bytes()).is_empty());
        // Two markers, one after the other.
        let two = format!("{M}FIDRT-00000000000000000000000000");
        assert_eq!(set(two.as_bytes()).len(), 2);
        // Bytes that are not UTF-8 around a marker do not hide it.
        let mut b = vec![0xff, 0xfe];
        b.extend_from_slice(M.as_bytes());
        b.push(0x80);
        assert_eq!(set(&b), vec![M.to_string()]);
    }

    /// Hand-labelled disguises of one marker that the prompt and leak rules must catch.
    #[test]
    fn normalised_matches_catch_disguises() {
        // The Crockford aliases: O for 0, I and L for 1, in either case.
        assert!(appears_in("see FIDRT-O123456789ABCDEFGHJKMNPQRS", M));
        assert!(appears_in("see fidrt-0i23456789abcdefghjkmnpqrs", M));
        assert!(appears_in("see FIDRT-0L23456789ABCDEFGHJKMNPQRS", M));
        // Zero-width, soft-hyphen and BOM splits, and a missing or doubled hyphen.
        assert!(appears_in("FIDRT-0123\u{200B}456789ABCDEF\u{200D}GHJKMNPQRS", M));
        assert!(appears_in("FIDRT-0123456789\u{00AD}ABCDEFGHJKMNPQ\u{FEFF}RS", M));
        assert!(appears_in("FIDRT0123456789ABCDEFGHJKMNPQRS", M));
        assert!(appears_in("FIDRT--0123-4567-89AB-CDEF-GHJK-MNPQ-RS", M));
        // Full-width: every character, including the hyphen.
        let full: String =
            M.chars().map(|c| char::from_u32(u32::from(c) + 0xFEE0).unwrap()).collect();
        assert!(appears_in(&format!("text {full} text"), M));
        // Cyrillic and Greek homoglyphs next to marker-shaped text fail closed.
        assert!(appears_in("FIDRT-0123456789\u{0410}BCDEFGHJKMNPQRS", M)); // Cyrillic A
        assert!(appears_in("F\u{0399}DRT-0123456789ABCDEFGHJKMNPQRS", M)); // Greek Iota
        // Not the marker: one character changed, split by a space or line break, or text in
        // another script that is not Greek or Cyrillic.
        assert!(!appears_in("FIDRT-0123456789ABCDEFGHJKMNPQRT", M));
        assert!(!appears_in("FIDRT-0123456789ABCDE FGHJKMNPQRS", M));
        assert!(!appears_in("FIDRT-0123456789ABCDE\nFGHJKMNPQRS", M));
        assert!(!appears_in("FIDRT-0123456789\u{4E00}BCDEFGHJKMNPQRS", M));
        assert!(!appears_in("an ordinary prompt", M));
    }
}
