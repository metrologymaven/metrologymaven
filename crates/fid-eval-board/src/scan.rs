//! Scanning Markdown and HTML sources for claim tags and untagged capability phrases.
//!
//! A source is cut into *units*; a tag covers only the unit it is in, and a unit with a
//! capability phrase and no tag fails with `untagged_claim` at the first phrase it contains.
//!
//! - **Markdown** (`.md`, `.markdown`): units end at blank lines; every heading, list item and
//!   table row is a unit of its own. Fenced code blocks (``` or ~~~) and inline code spans
//!   are skipped, so docs can show the tag syntax without making a claim. A tag is a one-line
//!   HTML comment `<!-- fid-claim: <id> -->`.
//! - **HTML** (`.html`, `.htm`): read as a stream (minified files work). Units end at
//!   block-level tags (`p`, `li`, `tr`, `h1`-`h6`, `div`, `section`, `br`, lists, tables and
//!   other sectioning elements) and at blank lines. The content of `script`, `style`, `pre`,
//!   `code`, `textarea` and `template` is skipped. A tag is any comment
//!   `<!-- fid-claim: <id> -->`, also over several lines. Markdown rules do not apply.
//!
//! Before matching, comment text and markup are removed: HTML tags (in both kinds), Markdown
//! emphasis (`*`, `_`, `~`) and link syntax (`[text](url)` keeps `text`), and character
//! references (`&nbsp;`, `&#160;`, `&#xA0;`, ...) are decoded. Soft hyphens and zero-width
//! characters are dropped. Phrases then match case-insensitively (ASCII), on whole words,
//! with any run of whitespace (including a line break or no-break space) matching one space.
//! This is a heuristic.

use crate::{is_claim_id, Code, Finding};
use std::path::Path;

/// File extensions scanned in source directories.
pub const SOURCE_EXTENSIONS: [&str; 4] = ["htm", "html", "markdown", "md"];

/// Elements whose start or end tag ends a unit in HTML.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "br",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "section",
    "summary",
    "table",
    "tbody",
    "title",
    "tr",
    "ul",
];

/// Elements whose content is not prose in HTML.
const SKIP_TAGS: &[&str] = &["code", "pre", "script", "style", "template", "textarea"];

/// Characters dropped before matching (soft hyphen, zero-width characters, BOM).
const INVISIBLE: &[char] = &['\u{00AD}', '\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}'];

/// The markup of a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Markdown,
    Html,
}

impl SourceKind {
    /// From a file extension (`md`, `markdown`, `html`, `htm`; any case).
    pub fn from_extension(ext: &str) -> Option<SourceKind> {
        match ext.to_ascii_lowercase().as_str() {
            "md" | "markdown" => Some(SourceKind::Markdown),
            "html" | "htm" => Some(SourceKind::Html),
            _ => None,
        }
    }

    /// From a path's extension; `None` for other files.
    pub fn from_path(p: &Path) -> Option<SourceKind> {
        p.extension().and_then(|x| x.to_str()).and_then(SourceKind::from_extension)
    }
}

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

/// Text of one unit, with the source line of each stretch.
#[derive(Default)]
struct Unit {
    text: String,
    /// (byte offset in `text`, 1-based source line), one entry per line change.
    marks: Vec<(usize, usize)>,
    tagged: bool,
}

impl Unit {
    fn push(&mut self, s: &str, line: usize) {
        if s.is_empty() {
            return;
        }
        if self.marks.last().map(|m| m.1) != Some(line) {
            self.marks.push((self.text.len(), line));
        }
        self.text.push_str(s);
    }
}

struct Scanner<'a> {
    file: &'a str,
    phrases: &'a [String],
    unit: Unit,
    tags: Vec<Tag>,
    findings: Vec<Finding>,
}

impl Scanner<'_> {
    fn flush(&mut self) {
        let u = std::mem::take(&mut self.unit);
        if !u.tagged {
            if let Some(f) = untagged(&u, self.phrases, self.file) {
                self.findings.push(f);
            }
        }
    }

    /// Handles the inner text of an HTML comment starting on `line`.
    fn comment(&mut self, inner: &str, line: usize) {
        let inner = inner.trim();
        let Some(rest) = inner.strip_prefix("fid-claim:") else {
            return;
        };
        let id = rest.trim();
        if is_claim_id(id) {
            self.tags.push(Tag { file: self.file.to_string(), line, id: id.to_string() });
        } else {
            self.findings.push(Finding::new(
                Code::UnknownClaim,
                None,
                self.file,
                line,
                format!("malformed claim tag {inner:?} (expected `<!-- fid-claim: <id> -->`)"),
            ));
        }
        self.unit.tagged = true;
    }
}

/// Scans one source text of the given kind. `file` is the path shown in findings. Returns
/// the tags found and the findings (`untagged_claim`, and `unknown_claim` for malformed tags).
pub fn scan_text(text: &str, file: &str, kind: SourceKind, phrases: &[String]) -> (Vec<Tag>, Vec<Finding>) {
    let mut s = Scanner { file, phrases, unit: Unit::default(), tags: Vec::new(), findings: Vec::new() };
    match kind {
        SourceKind::Markdown => scan_markdown(&mut s, text),
        SourceKind::Html => scan_html(&mut s, text),
    }
    s.flush();
    (s.tags, s.findings)
}

fn scan_markdown(s: &mut Scanner<'_>, text: &str) {
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
        let heading = t.starts_with('#');
        let row = t.starts_with('|');
        if heading || row || is_list_item(t) {
            s.flush();
        }
        let no_code = strip_inline_code(raw);
        let visible = md_comments(s, &no_code, n);
        let clean = strip_md_inline(&strip_tags(&visible));
        s.unit.push(&clean, n);
        s.unit.push(" ", n);
        if heading || row {
            s.flush();
        }
    }
}

/// A Markdown list item: `-`, `*` or `+`, or 1-9 digits and `.` or `)`, then a space or end.
fn is_list_item(t: &str) -> bool {
    let b = t.as_bytes();
    let after = if matches!(b.first(), Some(b'-' | b'*' | b'+')) {
        1
    } else {
        let d = b.iter().take_while(|c| c.is_ascii_digit()).count();
        if d == 0 || d > 9 || !matches!(b.get(d), Some(b'.' | b')')) {
            return false;
        }
        d + 1
    };
    match b.get(after) {
        None => true,
        Some(c) => c.is_ascii_whitespace(),
    }
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

/// Removes one-line HTML comments from a Markdown line, handing each to the scanner.
fn md_comments(s: &mut Scanner<'_>, line: &str, n: usize) -> String {
    let mut visible = String::with_capacity(line.len());
    let mut pos = 0;
    while let Some(off) = line[pos..].find("<!--") {
        let start = pos + off;
        let Some(eoff) = line[start + 4..].find("-->") else {
            break;
        };
        let end = start + 4 + eoff;
        visible.push_str(&line[pos..start]);
        visible.push(' ');
        s.comment(&line[start + 4..end], n);
        pos = end + 3;
    }
    visible.push_str(&line[pos..]);
    visible
}

/// An HTML tag starting at byte `i` (which holds `<`): (lower-case name, is end tag, index of
/// the closing `>`). Declarations (`<!doctype ...>`, `<?...>`) have the name `!`.
fn tag_at(text: &str, i: usize) -> Option<(String, bool, usize)> {
    let b = text.as_bytes();
    let mut j = i + 1;
    let first = *b.get(j)?;
    let (name, closing) = if first == b'!' || first == b'?' {
        ("!".to_string(), false)
    } else {
        let closing = first == b'/';
        if closing {
            j += 1;
        }
        if !b.get(j)?.is_ascii_alphabetic() {
            return None;
        }
        let len = b[j..].iter().take_while(|c| c.is_ascii_alphanumeric()).count();
        let name = text[j..j + len].to_ascii_lowercase();
        j += len;
        match b.get(j) {
            Some(c) if c.is_ascii_whitespace() || *c == b'>' || *c == b'/' => {}
            _ => return None,
        }
        (name, closing)
    };
    let mut quote: Option<u8> = None;
    while j < b.len() {
        let c = b[j];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'>' => return Some((name, closing, j)),
            None => {}
        }
        j += 1;
    }
    None
}

/// Removes inline HTML tags from Markdown text; block-level tags become a space.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s.as_bytes()[i] == b'<' {
            if let Some((name, _, gt)) = tag_at(s, i) {
                if BLOCK_TAGS.contains(&name.as_str()) {
                    out.push(' ');
                }
                i = gt + 1;
                continue;
            }
        }
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Removes Markdown emphasis markers and link syntax, keeping the link text.
fn strip_md_inline(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut k = 0;
    while k < cs.len() {
        match cs[k] {
            '*' | '_' | '~' | '[' => k += 1,
            '!' if cs.get(k + 1) == Some(&'[') => k += 1,
            ']' => {
                k += 1;
                match cs.get(k) {
                    Some('(') => {
                        let mut depth = 0usize;
                        while k < cs.len() {
                            match cs[k] {
                                '(' => depth += 1,
                                ')' => {
                                    depth = depth.saturating_sub(1);
                                    if depth == 0 {
                                        k += 1;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            k += 1;
                        }
                    }
                    Some('[') => {
                        while k < cs.len() && cs[k] != ']' {
                            k += 1;
                        }
                        k += 1;
                    }
                    _ => {}
                }
            }
            c => {
                out.push(c);
                k += 1;
            }
        }
    }
    out
}

/// Byte offset of the first ASCII-case-insensitive occurrence of `pat` at or after `from`.
fn find_ci(text: &str, from: usize, pat: &str) -> Option<usize> {
    text.get(from..)?.to_ascii_lowercase().find(pat).map(|o| from + o)
}

fn newlines(s: &str) -> usize {
    s.bytes().filter(|&b| b == b'\n').count()
}

fn scan_html(s: &mut Scanner<'_>, text: &str) {
    let b = text.as_bytes();
    let mut i = 0;
    let mut line = 1;
    let mut blank = true;
    while i < b.len() {
        if text[i..].starts_with("<!--") {
            let end = text[i + 4..].find("-->").map(|e| i + 4 + e);
            let stop = end.map_or(b.len(), |e| e + 3);
            if let Some(e) = end {
                s.comment(&text[i + 4..e], line);
            }
            line += newlines(&text[i..stop]);
            i = stop;
            blank = false;
            continue;
        }
        if b[i] == b'<' {
            if let Some((name, closing, gt)) = tag_at(text, i) {
                line += newlines(&text[i..gt]);
                blank = false;
                let mut next = gt + 1;
                if !closing && SKIP_TAGS.contains(&name.as_str()) {
                    let stop = match find_ci(text, next, &format!("</{name}")) {
                        Some(c) => text[c..].find('>').map_or(b.len(), |g| c + g + 1),
                        None => b.len(),
                    };
                    line += newlines(&text[next..stop]);
                    next = stop;
                    s.unit.push(" ", line);
                } else if BLOCK_TAGS.contains(&name.as_str()) {
                    s.flush();
                }
                i = next;
                continue;
            }
        }
        let Some(ch) = text[i..].chars().next() else {
            break;
        };
        let len = ch.len_utf8();
        if ch == '\n' {
            if blank {
                s.flush();
            }
            line += 1;
            blank = true;
            s.unit.push(" ", line);
        } else {
            if !ch.is_whitespace() {
                blank = false;
            }
            s.unit.push(&text[i..i + len], line);
        }
        i += len;
    }
}

/// Decodes a character reference at the start of `s` (which starts with `&`). Returns the
/// character (`None` for one dropped before matching) and the bytes consumed.
fn decode_entity(s: &str) -> Option<(Option<char>, usize)> {
    let end = s.as_bytes().iter().take(40).position(|&c| c == b';')?;
    let body = s.get(1..end)?;
    let ch = if let Some(num) = body.strip_prefix('#') {
        let v = match num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse::<u32>().ok()?,
        };
        char::from_u32(v)?
    } else {
        match body {
            "nbsp" | "ensp" | "emsp" | "thinsp" | "NonBreakingSpace" => ' ',
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "shy" | "zwj" | "zwnj" | "ZeroWidthSpace" | "NoBreak" => return Some((None, end + 1)),
            _ => return None,
        }
    };
    Some((Some(ch), end + 1))
}

/// The first whole-word phrase match in a unit, as an `untagged_claim` finding.
fn untagged(unit: &Unit, phrases: &[String], file: &str) -> Option<Finding> {
    if unit.text.is_empty() || phrases.is_empty() {
        return None;
    }
    let t = unit.text.as_str();
    let mut norm = String::with_capacity(t.len());
    let mut nmarks: Vec<(usize, usize)> = Vec::new();
    let mut mi = 0;
    let mut cur_line = 0;
    let mut i = 0;
    while i < t.len() {
        while mi < unit.marks.len() && unit.marks[mi].0 <= i {
            cur_line = unit.marks[mi].1;
            mi += 1;
        }
        let decoded = if t.as_bytes()[i] == b'&' { decode_entity(&t[i..]) } else { None };
        let (ch, adv) = match decoded {
            Some(d) => d,
            None => {
                let c = t[i..].chars().next()?;
                (Some(c), c.len_utf8())
            }
        };
        i += adv;
        let Some(ch) = ch else {
            continue;
        };
        if INVISIBLE.contains(&ch) {
            continue;
        }
        if ch.is_whitespace() {
            if !norm.is_empty() && !norm.ends_with(' ') {
                norm.push(' ');
            }
            continue;
        }
        if nmarks.last().map(|m| m.1) != Some(cur_line) {
            nmarks.push((norm.len(), cur_line));
        }
        norm.push(ch.to_ascii_lowercase());
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
    let line = nmarks.iter().take_while(|(off, _)| *off <= pos).last().map_or(0, |(_, n)| *n);
    Some(Finding::new(
        Code::UntaggedClaim,
        None,
        file,
        line,
        format!("capability phrase {phrase:?} with no fid-claim tag in the same unit"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: SourceKind = SourceKind::Markdown;
    const HTML: SourceKind = SourceKind::Html;

    fn phrases() -> Vec<String> {
        parse_phrases("# comment\nour agent\n\n  The   Agent can \nour agent\n")
    }

    fn lines(text: &str, kind: SourceKind) -> Vec<usize> {
        let (_, f) = scan_text(text, "a", kind, &phrases());
        assert!(f.iter().all(|x| x.code == Code::UntaggedClaim), "{f:?}");
        f.iter().map(|x| x.line).collect()
    }

    #[test]
    fn phrase_list() {
        assert_eq!(phrases(), vec!["our agent".to_string(), "the agent can".to_string()]);
    }

    #[test]
    fn whole_words_and_line_breaks() {
        assert!(lines("Tour agents are fine.\n\nyour agent is fine\n", MD).is_empty());
        assert_eq!(lines("intro\n\nsee how The\n  agent   CAN do it\n", MD), vec![3]);
    }

    #[test]
    fn tags_cover_their_unit_only() {
        let text = "our agent does x\n<!-- fid-claim: c1 -->\n\nour agent does y\n";
        let (tags, f) = scan_text(text, "a.md", MD, &phrases());
        assert_eq!(tags, vec![Tag { file: "a.md".into(), line: 2, id: "c1".into() }]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].line, 4);
        // List items and table rows are units of their own.
        assert_eq!(lines("- ok <!-- fid-claim: c1 -->\n- our agent\n", MD), vec![2]);
        assert_eq!(lines("1. ok <!-- fid-claim: c1 -->\n2) our agent\n", MD), vec![2]);
        assert_eq!(lines("| a <!-- fid-claim: c1 --> |\n| our agent |\n", MD), vec![2]);
    }

    #[test]
    fn markup_is_removed_before_matching() {
        assert_eq!(lines("our <b>agent</b>\n", MD), vec![1]);
        assert_eq!(lines("our&nbsp;agent\n", MD), vec![1]);
        assert_eq!(lines("our&#160;agent and our&#xA0;agent\n", MD), vec![1]);
        assert_eq!(lines("our *agent*\n", MD), vec![1]);
        assert_eq!(lines("__our__ ~~agent~~\n", MD), vec![1]);
        assert_eq!(lines("[our](https://x.org/a_(b)) agent\n", MD), vec![1]);
        assert_eq!(lines("[our][ref] agent\n", MD), vec![1]);
        assert_eq!(lines("our ag&shy;ent\n", MD), vec![1]);
        assert!(lines("our&amp;agent\n", MD).is_empty());
    }

    #[test]
    fn code_is_skipped_in_markdown() {
        let text =
            "```md\n<!-- fid-claim: c1 -->\nour agent\n```\n\nUse `<!-- fid-claim: <id> -->` and ``our agent``.\n";
        let (tags, f) = scan_text(text, "a.md", MD, &phrases());
        assert!(tags.is_empty());
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn html_units_and_skipped_elements() {
        let min = "<html><body><p>ok our agent <!-- fid-claim: c1 --></p><p>our<br>agent</p>\
                   <ul><li>x<!-- fid-claim: c2 --></li><li>Our&nbsp;<em>agent</em></li></ul>\
                   <pre><code>our agent</code></pre><script>let a = '</p>our agent';</script>\
                   <style>p::after{content:'our agent'}</style><code>our agent</code></body></html>";
        // `<br>` splits "our" from "agent"; the second list item is caught.
        assert_eq!(lines(min, HTML), vec![1]);
        // Markdown rules do not apply to HTML: a ``` line is text.
        assert_eq!(lines("<div>\n```\nour agent\n```\n</div>\n", HTML), vec![3]);
        // Inline code backticks are text in HTML too.
        assert_eq!(lines("<p>`our agent`</p>\n", HTML), vec![1]);
        // A tag in a multi-line comment covers its unit.
        assert!(lines("<p>our agent <!--\n fid-claim: c1\n--></p>\n", HTML).is_empty());
    }

    #[test]
    fn malformed_tag_is_reported() {
        let (tags, f) = scan_text("x <!-- fid-claim: Bad Id --> y\n", "a.md", MD, &phrases());
        assert!(tags.is_empty());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].code, Code::UnknownClaim);
    }

    #[test]
    fn comment_text_is_not_scanned() {
        assert!(lines("<!-- our agent -->\n", HTML).is_empty());
        assert!(lines("<!-- our agent -->\n", MD).is_empty());
    }

    #[test]
    fn kinds_from_paths() {
        assert_eq!(SourceKind::from_path(Path::new("a/B.MD")), Some(MD));
        assert_eq!(SourceKind::from_path(Path::new("x.htm")), Some(HTML));
        assert_eq!(SourceKind::from_path(Path::new("x.txt")), None);
    }
}
