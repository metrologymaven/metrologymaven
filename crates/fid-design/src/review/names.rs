//! The placeholder-name check of `review-check --deny-names` (REQ-UX.SYSTEM.REVIEW-005).
//!
//! The deny list (D-002 candidate names) is private: it is read only from the given path, never
//! written, and no finding quotes an entry; entries are named by their line number.
//!
//! Matching is a case-insensitive substring search over a comparison key: the text is put in
//! Unicode NFKC (so fullwidth letters, ligatures and mathematical letters become plain ones),
//! invisible and formatting characters are dropped, and the result is lowercased with Turkish
//! `İ`/`ı` folded to `i`. Every package file is scanned, and so is its path. A file that is not
//! UTF-8 is scanned as bytes with ASCII case folding. A symbolic link, an over-large file or a
//! package too large to walk is a `REVIEW-SCHEMA` finding: what cannot be scanned does not pass.
//!
//! When a deny list is given, every path segment in the report that holds a listed name is
//! printed as `<withheld>` ([`redact`]), so a file named after a candidate does not publish it.

use super::guard::defects;
use super::walk::walk;
use super::SCHEMA;
use crate::finding::Finding;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

pub const NAME_LEAK: &str = "REVIEW-NAME-LEAK";
/// Subject of findings about the deny list itself.
pub const DENY_SUBJECT: &str = "deny-names";
/// Largest package file scanned (16 MiB).
pub const MAX_SCAN_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// What a withheld path segment is printed as.
pub const WITHHELD: &str = "<withheld>";

/// One deny-list entry: its line and comparison key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenyName {
    pub line: usize,
    pub key: String,
}

/// The comparison key of a text: NFKC, hidden characters dropped, Turkish i folded, lowercased.
pub fn scan_key(s: &str) -> String {
    let n: String = s.nfkc().filter(|c| !crate::words::is_hidden(*c)).collect();
    crate::words::fold(&n)
}

/// Parses the deny list: one name per line, blank lines and lines starting with `#` skipped.
/// A refused or empty line and a list without any name are `REVIEW-SCHEMA deny-names` findings
/// that never quote the entry.
pub fn parse_deny(text: &str, f: &mut Vec<Finding>) -> Vec<DenyName> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let t = raw.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\r');
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let d = defects(t);
        if !d.is_empty() {
            f.push(Finding::new(SCHEMA, DENY_SUBJECT, format!("line {line}: entry refused ({})", d.join(", "))));
            continue;
        }
        let key = scan_key(t);
        if key.trim().is_empty() {
            f.push(Finding::new(SCHEMA, DENY_SUBJECT, format!("line {line}: entry is empty after normalisation")));
            continue;
        }
        out.push(DenyName { line, key });
    }
    if out.is_empty() {
        f.push(Finding::new(SCHEMA, DENY_SUBJECT, "the deny list names no product name"));
    }
    out
}

fn ascii_lower_contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && hay.windows(needle.len()).any(|w| w.iter().zip(needle).all(|(a, b)| a.to_ascii_lowercase() == *b))
}

/// The lines of the deny-list entries found in a file's bytes or its path.
pub fn hits(rel: &str, bytes: &[u8], names: &[DenyName]) -> Vec<usize> {
    let path_key = scan_key(rel);
    let text = std::str::from_utf8(bytes).ok().map(|s| scan_key(s.strip_prefix('\u{feff}').unwrap_or(s)));
    names
        .iter()
        .filter(|n| {
            path_key.contains(&n.key)
                || match &text {
                    Some(t) => t.contains(&n.key),
                    None => ascii_lower_contains(bytes, n.key.as_bytes()),
                }
        })
        .map(|n| n.line)
        .collect()
}

/// Replaces every `/`-separated segment of every space-separated word of `s` that holds a
/// deny-listed name with [`WITHHELD`].
pub fn redact(s: &str, names: &[DenyName]) -> String {
    if names.is_empty() {
        return s.to_string();
    }
    let seg = |x: &str| {
        let k = scan_key(x);
        if names.iter().any(|n| k.contains(&n.key)) {
            WITHHELD.to_string()
        } else {
            x.to_string()
        }
    };
    s.split(' ').map(|w| w.split('/').map(seg).collect::<Vec<_>>().join("/")).collect::<Vec<_>>().join(" ")
}

/// Walks `root` (reported as `prefix` + the path below it) and reports every file or directory
/// whose content or path holds a deny-listed name. `Err` is an I/O failure (exit 2).
pub fn scan(root: &Path, prefix: &str, names: &[DenyName], f: &mut Vec<Finding>) -> Result<(), String> {
    if names.is_empty() {
        return Ok(());
    }
    let top = prefix.trim_end_matches('/');
    for e in walk(root, top, f)? {
        let lines = if e.is_dir {
            hits(&e.rel, &[], names)
        } else if e.len > MAX_SCAN_FILE_BYTES {
            f.push(Finding::new(SCHEMA, &e.rel, format!("larger than {MAX_SCAN_FILE_BYTES} bytes; not scanned")));
            continue;
        } else {
            let bytes = std::fs::read(&e.path).map_err(|x| format!("cannot read {}: {x}", e.rel))?;
            hits(&e.rel, &bytes, names)
        };
        if !lines.is_empty() {
            let l = lines.iter().map(usize::to_string).collect::<Vec<_>>().join(", ");
            f.push(Finding::new(NAME_LEAK, &e.rel, format!("holds the deny-list name on line(s) {l} (name withheld)")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_fold_compat_and_hidden_characters() {
        assert_eq!(scan_key("Examplar"), "examplar");
        assert_eq!(scan_key("\u{FF25}\u{FF38}AMPLAR"), "examplar");
        assert_eq!(scan_key("Exa\u{200B}mplar"), "examplar");
        assert_eq!(scan_key("EXAMPLAR\u{FE0F}"), "examplar");
        assert_eq!(scan_key("\u{0130}"), "i");
    }

    #[test]
    fn deny_list_parsing_never_quotes_entries() {
        let mut f = Vec::new();
        let n = parse_deny("# comment\n\nExamplar\nBad\u{202E}name\n", &mut f);
        assert_eq!(n, [DenyName { line: 3, key: "examplar".into() }]);
        assert_eq!(f.len(), 1);
        assert!(!f[0].detail.contains("Bad") && !f[0].detail.contains("name\u{202E}"), "{}", f[0].detail);
        let mut f = Vec::new();
        assert!(parse_deny("# only comments\n", &mut f).is_empty());
        assert_eq!(f[0].subject, DENY_SUBJECT);
    }

    #[test]
    fn hits_in_text_bytes_and_paths() {
        let names = [DenyName { line: 2, key: "examplar".into() }];
        assert_eq!(hits("a.md", b"the EXAMPLAR toolbar", &names), [2]);
        assert_eq!(hits("examplar-logo.svg", b"<svg/>", &names), [2]);
        assert_eq!(hits("a.bin", &[0xff, b'E', b'x', b'a', b'm', b'p', b'l', b'a', b'r'], &names), [2]);
        assert!(hits("a.md", b"an example", &names).is_empty());
    }

    #[test]
    fn redaction_withholds_matching_segments_only() {
        let names = [DenyName { line: 2, key: "examplar".into() }];
        assert_eq!(redact("icons/Examplar-mark.svg", &names), "icons/<withheld>");
        assert_eq!(redact("a EXAMPLAR/b.svg c", &names), "a <withheld>/b.svg c");
        assert_eq!(redact("icons/probe.svg", &names), "icons/probe.svg");
        assert_eq!(redact("icons/Examplar.svg", &[]), "icons/Examplar.svg");
    }
}
