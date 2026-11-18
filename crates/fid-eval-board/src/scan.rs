//! Scanning Markdown and HTML sources for claim tags and untagged capability phrases.
//!
//! A source is split into paragraphs at blank lines. Fenced code blocks (``` or ~~~) and
//! inline code spans are skipped, so docs can show the tag syntax without making a claim.
//! A tag is an HTML comment on one line, `<!-- fid-claim: <id> -->`; comment text is not
//! scanned for phrases. A paragraph with no tag fails with `untagged_claim` at the first
//! phrase it contains. Phrases match case-insensitively (ASCII), on whole words, with any run
//! of whitespace (including a line break) matching one space. This is a heuristic.

use crate::{is_claim_id, Code, Finding};

/// File extensions scanned in source directories.
pub const SOURCE_EXTENSIONS: [&str; 4] = ["htm", "html", "markdown", "md"];

/// One claim tag in a source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tag {
    pub file: String,
    pub line: usize,
    pub id: String,
}

/// Parses the phrase list: one phrase per line; blank lines and lines starting with `#` are
/// skipped; phrases are lower-cased (ASCII) with whitespace runs collapsed. Duplicates dropped.
pub fn parse_phrases(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let p = normalize(t);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(ch.to_ascii_lowercase());
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

struct Scanner<'a> {
    file: &'a str,
    phrases: &'a [String],
    para: Vec<(usize, String)>,
    has_tag: bool,
    tags: Vec<Tag>,
    findings: Vec<Finding>,
}

impl Scanner<'_> {
    fn flush(&mut self) {
        if !self.has_tag {
            if let Some(f) = untagged(&self.para, self.phrases, self.file) {
                self.findings.push(f);
            }
        }
        self.para.clear();
        self.has_tag = false;
    }
}

/// Scans one source text. `file` is the path shown in findings. Returns the tags found and
/// the findings (`untagged_claim`, and `unknown_claim` for malformed tags).
pub fn scan_text(text: &str, file: &str, phrases: &[String]) -> (Vec<Tag>, Vec<Finding>) {
    let mut s = Scanner { file, phrases, para: Vec::new(), has_tag: false, tags: Vec::new(), findings: Vec::new() };
    let mut fence: Option<(u8, usize)> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let t = raw.trim_start();
        if let Some((c, len)) = fence {
            if let Some((c2, l2, rest)) = fence_marker(t) {
                if c2 == c && l2 >= len && rest.trim().is_empty() {
                    fence = None;
                }
            }
            continue;
        }
        if let Some((c, len, _)) = fence_marker(t) {
            s.flush();
            fence = Some((c, len));
            continue;
        }
        if t.is_empty() {
            s.flush();
            continue;
        }
        let no_code = strip_inline_code(raw);
        let (visible, ids, bad) = take_comments(&no_code);
        for id in ids {
            s.tags.push(Tag { file: file.to_string(), line: n, id });
            s.has_tag = true;
        }
        for d in bad {
            s.findings.push(Finding::new(Code::UnknownClaim, None, file, n, d));
            s.has_tag = true;
        }
        s.para.push((n, visible));
    }
    s.flush();
    (s.tags, s.findings)
}

/// A fence opener/closer: at least three backticks or tildes. Returns (char, count, rest).
fn fence_marker(t: &str) -> Option<(u8, usize, &str)> {
    let c = *t.as_bytes().first()?;
    if c != b'`' && c != b'~' {
        return None;
    }
    let len = t.bytes().take_while(|&b| b == c).count();
    if len < 3 {
        return None;
    }
    Some((c, len, &t[len..]))
}

/// Replaces inline code spans (matching runs of backticks) with a space.
fn strip_inline_code(line: &str) -> String {
    let b = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut copied = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'`' {
            i += 1;
            continue;
        }
        let k = b[i..].iter().take_while(|&&c| c == b'`').count();
        match closing_run(b, i + k, k) {
            Some(end) => {
                out.push_str(&line[copied..i]);
                out.push(' ');
                i = end + k;
                copied = i;
            }
            None => i += k,
        }
    }
    out.push_str(&line[copied..]);
    out
}

/// Start of the next run of exactly `k` backticks at or after `from`.
fn closing_run(b: &[u8], from: usize, k: usize) -> Option<usize> {
    let mut j = from;
    while j < b.len() {
        if b[j] == b'`' {
            let run = b[j..].iter().take_while(|&&c| c == b'`').count();
            if run == k {
                return Some(j);
            }
            j += run;
        } else {
            j += 1;
        }
    }
    None
}

/// Removes one-line HTML comments from `line`. Returns the visible text, the claim IDs of
/// well-formed tags and the details of malformed `fid-claim` tags.
fn take_comments(line: &str) -> (String, Vec<String>, Vec<String>) {
    let mut visible = String::with_capacity(line.len());
    let mut ids = Vec::new();
    let mut bad = Vec::new();
    let mut pos = 0;
    while let Some(off) = line[pos..].find("<!--") {
        let start = pos + off;
        let Some(eoff) = line[start + 4..].find("-->") else {
            break;
        };
        let end = start + 4 + eoff;
        visible.push_str(&line[pos..start]);
        visible.push(' ');
        let inner = line[start + 4..end].trim();
        if let Some(rest) = inner.strip_prefix("fid-claim:") {
            let id = rest.trim();
            if is_claim_id(id) {
                ids.push(id.to_string());
            } else {
                bad.push(format!("malformed claim tag {inner:?} (expected `<!-- fid-claim: <id> -->`)"));
            }
        }
        pos = end + 3;
    }
    visible.push_str(&line[pos..]);
    (visible, ids, bad)
}

/// The first whole-word phrase match in a paragraph, as an `untagged_claim` finding.
fn untagged(para: &[(usize, String)], phrases: &[String], file: &str) -> Option<Finding> {
    if para.is_empty() || phrases.is_empty() {
        return None;
    }
    let mut norm = String::new();
    let mut starts: Vec<(usize, usize)> = Vec::new();
    for (n, text) in para {
        if !norm.is_empty() && !norm.ends_with(' ') {
            norm.push(' ');
        }
        starts.push((norm.len(), *n));
        for ch in text.chars() {
            if ch.is_whitespace() {
                if !norm.is_empty() && !norm.ends_with(' ') {
                    norm.push(' ');
                }
            } else {
                norm.push(ch.to_ascii_lowercase());
            }
        }
    }
    let mut best: Option<(usize, &str)> = None;
    for p in phrases {
        for (pos, _) in norm.match_indices(p.as_str()) {
            let before = norm[..pos].chars().next_back();
            let after = norm[pos + p.len()..].chars().next();
            if before.is_some_and(char::is_alphanumeric) || after.is_some_and(char::is_alphanumeric) {
                continue;
            }
            let earlier = match best {
                Some((b, _)) => pos < b,
                None => true,
            };
            if earlier {
                best = Some((pos, p.as_str()));
            }
            break;
        }
    }
    let (pos, phrase) = best?;
    let line = starts.iter().take_while(|(off, _)| *off <= pos).last().map_or(0, |(_, n)| *n);
    Some(Finding::new(
        Code::UntaggedClaim,
        None,
        file,
        line,
        format!("capability phrase {phrase:?} with no fid-claim tag in the same paragraph"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrases() -> Vec<String> {
        parse_phrases("# comment\nour agent\n\n  The   Agent can \nour agent\n")
    }

    #[test]
    fn phrase_list() {
        assert_eq!(phrases(), vec!["our agent".to_string(), "the agent can".to_string()]);
    }

    #[test]
    fn whole_words_and_line_breaks() {
        let (_, f) = scan_text("Tour agents are fine.\n\nyour agent is fine\n", "a.md", &phrases());
        assert!(f.is_empty(), "{f:?}");
        let (_, f) = scan_text("intro\n\nsee how The\n  agent   CAN do it\n", "a.md", &phrases());
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].code, f[0].line), (Code::UntaggedClaim, 3));
    }

    #[test]
    fn tags_cover_their_paragraph_only() {
        let text = "our agent does x\n<!-- fid-claim: c1 -->\n\nour agent does y\n";
        let (tags, f) = scan_text(text, "a.md", &phrases());
        assert_eq!(tags, vec![Tag { file: "a.md".into(), line: 2, id: "c1".into() }]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].line, 4);
    }

    #[test]
    fn code_is_skipped() {
        let text = "```md\n<!-- fid-claim: c1 -->\nour agent\n```\n\nUse `<!-- fid-claim: <id> -->` and ``our agent``.\n";
        let (tags, f) = scan_text(text, "a.md", &phrases());
        assert!(tags.is_empty());
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn malformed_tag_is_reported() {
        let (tags, f) = scan_text("x <!-- fid-claim: Bad Id --> y\n", "a.md", &phrases());
        assert!(tags.is_empty());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].code, Code::UnknownClaim);
    }

    #[test]
    fn comment_text_is_not_scanned() {
        let (_, f) = scan_text("<!-- our agent -->\n", "a.html", &phrases());
        assert!(f.is_empty());
    }
}
