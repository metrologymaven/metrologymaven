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

use super::guard::{defects, is_bidi, is_extra_invisible};
use super::SCHEMA;
use crate::finding::Finding;
use std::collections::BTreeSet;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

pub const NAME_LEAK: &str = "REVIEW-NAME-LEAK";
/// Subject of findings about the deny list itself.
pub const DENY_SUBJECT: &str = "deny-names";
/// Largest package file scanned (16 MiB).
pub const MAX_SCAN_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Most files and directories walked under the root.
pub const MAX_SCAN_ENTRIES: usize = 100_000;
/// Deepest directory nesting walked.
pub const MAX_SCAN_DEPTH: usize = 32;

/// One deny-list entry: its line and comparison key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenyName {
    pub line: usize,
    pub key: String,
}

fn hidden(c: char) -> bool {
    is_bidi(c)
        || is_extra_invisible(c)
        || matches!(c, '\u{00AD}' | '\u{034F}' | '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
}

/// The comparison key of a text: NFKC, hidden characters dropped, Turkish i folded, lowercased.
pub fn scan_key(s: &str) -> String {
    let n: String = s.nfkc().filter(|c| !hidden(*c)).collect();
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

/// Walks `root` and reports every file whose content or path holds a deny-listed name.
/// `Err` is an I/O failure (exit 2).
pub fn scan(root: &Path, names: &[DenyName], f: &mut Vec<Finding>) -> Result<(), String> {
    if names.is_empty() {
        return Ok(());
    }
    let mut stack: Vec<(std::path::PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    let mut seen = 0usize;
    let mut leaks: BTreeSet<(String, String)> = BTreeSet::new();
    while let Some((dir, rel_dir, depth)) = stack.pop() {
        let rd = std::fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let mut entries = Vec::new();
        for e in rd {
            entries.push(e.map_err(|e| format!("cannot read {}: {e}", dir.display()))?);
        }
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            seen += 1;
            if seen > MAX_SCAN_ENTRIES {
                f.push(Finding::new(SCHEMA, ".", format!("more than {MAX_SCAN_ENTRIES} entries; not fully scanned")));
                return Ok(());
            }
            let name = e.file_name();
            let Some(name) = name.to_str() else {
                f.push(Finding::new(SCHEMA, rel_or_dot(&rel_dir), "a file name is not UTF-8; not scanned"));
                continue;
            };
            let rel = if rel_dir.is_empty() { name.to_string() } else { format!("{rel_dir}/{name}") };
            let ft = std::fs::symlink_metadata(e.path()).map_err(|e2| format!("cannot read {rel}: {e2}"))?;
            if ft.file_type().is_symlink() {
                f.push(Finding::new(SCHEMA, &rel, "symbolic link; not scanned"));
            } else if ft.is_dir() {
                if depth + 1 > MAX_SCAN_DEPTH {
                    f.push(Finding::new(SCHEMA, &rel, format!("nested deeper than {MAX_SCAN_DEPTH}; not scanned")));
                } else {
                    let lines = hits(&rel, &[], names);
                    if !lines.is_empty() {
                        leaks.insert((rel.clone(), lines_text(&lines)));
                    }
                    stack.push((e.path(), rel, depth + 1));
                }
            } else if ft.is_file() {
                if ft.len() > MAX_SCAN_FILE_BYTES {
                    f.push(Finding::new(SCHEMA, &rel, format!("larger than {MAX_SCAN_FILE_BYTES} bytes; not scanned")));
                    continue;
                }
                let bytes = std::fs::read(e.path()).map_err(|e2| format!("cannot read {rel}: {e2}"))?;
                let lines = hits(&rel, &bytes, names);
                if !lines.is_empty() {
                    leaks.insert((rel, lines_text(&lines)));
                }
            } else {
                f.push(Finding::new(SCHEMA, &rel, "not a regular file or directory; not scanned"));
            }
        }
    }
    for (rel, lines) in leaks {
        f.push(Finding::new(NAME_LEAK, rel, format!("holds the deny-list name on line(s) {lines} (name withheld)")));
    }
    Ok(())
}

fn rel_or_dot(s: &str) -> &str {
    if s.is_empty() {
        "."
    } else {
        s
    }
}

fn lines_text(l: &[usize]) -> String {
    l.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")
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
}
