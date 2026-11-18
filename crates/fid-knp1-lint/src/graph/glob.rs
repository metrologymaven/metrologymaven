//! Exclusion globs over `/`-separated paths relative to a crate root (REQ-KRN.NUM.LINT.GRAPH-005).
//!
//! `**` matches any number of whole segments (including none); within a segment `*` matches
//! any run of characters and `?` one character. Matching is case-sensitive and bounded.

/// Most segments in a glob or a path considered for matching.
pub const MAX_SEGMENTS: usize = 64;

/// True if the glob may be used as an exclusion: its first segments are literally `tests/`,
/// `benches/`, `examples/` or `src/bin/` followed by at least one more segment, with no empty,
/// `.` or `..` segment and no backslash.
pub fn exclusion_allowed(glob: &str) -> bool {
    if glob.is_empty() || glob.contains('\\') || glob.starts_with('/') {
        return false;
    }
    let segs: Vec<&str> = glob.split('/').collect();
    if segs.len() > MAX_SEGMENTS || segs.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return false;
    }
    match segs.as_slice() {
        [first, rest @ ..] if matches!(*first, "tests" | "benches" | "examples") => !rest.is_empty(),
        ["src", "bin", rest @ ..] => !rest.is_empty(),
        _ => false,
    }
}

fn segment_matches(pat: &[u8], text: &[u8]) -> bool {
    match pat.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|k| text.get(k..).is_some_and(|t| segment_matches(rest, t))),
        Some((b'?', rest)) => text.split_first().is_some_and(|(_, t)| segment_matches(rest, t)),
        Some((c, rest)) => text.split_first().is_some_and(|(d, t)| c == d && segment_matches(rest, t)),
    }
}

fn segments_match(pat: &[&str], path: &[&str]) -> bool {
    match pat.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|k| path.get(k..).is_some_and(|p| segments_match(rest, p))),
        Some((seg, rest)) => path
            .split_first()
            .is_some_and(|(p, tail)| segment_matches(seg.as_bytes(), p.as_bytes()) && segments_match(rest, tail)),
    }
}

/// True if `path` (relative, `/`-separated) matches `glob`.
pub fn matches(glob: &str, path: &str) -> bool {
    let g: Vec<&str> = glob.split('/').collect();
    let p: Vec<&str> = path.split('/').collect();
    g.len() <= MAX_SEGMENTS && p.len() <= MAX_SEGMENTS && segments_match(&g, &p)
}
