//! Sentence extraction from Markdown and HTML sources (REQ-OUTR.BRAND.LINT-003).
//!
//! Both formats are read by small, bounded, hand-written scanners (no CommonMark or HTML
//! library): they keep visible prose and link destinations, and drop front matter, fenced
//! code, inline code spans, HTML comments and the contents of `code`, `pre`, `script` and
//! `style` elements. Text is split into sentences at block boundaries and at `.`, `!` or `?`
//! followed by whitespace and an uppercase letter. Every character keeps its source line and
//! column (1-based, column counted in Unicode scalar values). HTML comments are returned so the
//! caller can read suppression markers.

use std::collections::BTreeMap;

/// Longest HTML tag (from `<` to `>`) the scanners accept, in characters.
pub const MAX_TAG_CHARS: usize = 8192;
/// Longest Markdown link construct (`[text](dest "title")`) the scanner accepts, in characters.
pub const MAX_LINK_CHARS: usize = 4096;

/// Source format, chosen from the file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Html,
}

/// One character of a sentence with its source position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SChar {
    pub c: char,
    pub line: u32,
    pub col: u32,
}

/// One sentence: text (whitespace collapsed to single spaces) and the link destinations in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    pub line: u32,
    pub col: u32,
    pub chars: Vec<SChar>,
    pub links: Vec<String>,
}

impl Sentence {
    /// The sentence text.
    pub fn text(&self) -> String {
        self.chars.iter().map(|s| s.c).collect()
    }
}

/// An HTML comment (`<!-- ... -->`) and the line its `<!--` starts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub line: u32,
    pub text: String,
}

/// Everything the rules need from one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extracted {
    pub sentences: Vec<Sentence>,
    pub comments: Vec<Comment>,
}

/// Extract sentences and comments from a file's text.
pub fn extract(text: &str, format: Format) -> Extracted {
    let mut src = Src::new(text);
    let fm = front_matter_lines(&src);
    for l in 0..fm {
        src.blank_line(l);
    }
    let (kinds, refs) = match format {
        Format::Markdown => classify(&mut src, fm),
        Format::Html => (Vec::new(), BTreeMap::new()),
    };
    let mut eng =
        Engine { s: &src, md: format == Format::Markdown, kinds, refs, toks: Vec::new(), comments: Vec::new() };
    eng.run();
    let Engine { toks, comments, .. } = eng;
    Extracted { sentences: split_sentences(toks), comments }
}

// ------------------------------------------------------------------ source

struct Src {
    c: Vec<char>,
    line: Vec<u32>,
    col: Vec<u32>,
    /// Index of the first character of each line.
    starts: Vec<usize>,
}

impl Src {
    fn new(text: &str) -> Src {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut s = Src { c: Vec::new(), line: Vec::new(), col: Vec::new(), starts: vec![0] };
        let (mut l, mut k) = (1u32, 1u32);
        let mut it = text.chars().peekable();
        while let Some(ch) = it.next() {
            let ch = if ch == '\r' {
                if it.peek() == Some(&'\n') {
                    continue;
                }
                '\n'
            } else {
                ch
            };
            s.c.push(ch);
            s.line.push(l);
            s.col.push(k);
            if ch == '\n' {
                l = l.saturating_add(1);
                k = 1;
                s.starts.push(s.c.len());
            } else {
                k = k.saturating_add(1);
            }
        }
        s
    }

    fn n_lines(&self) -> usize {
        self.starts.len()
    }

    /// Character range of line `l` (0-based), without its newline.
    fn range(&self, l: usize) -> (usize, usize) {
        let a = self.starts.get(l).copied().unwrap_or(self.c.len());
        let b = match self.starts.get(l + 1) {
            Some(&next) => next.saturating_sub(1),
            None => self.c.len(),
        };
        (a, b.max(a))
    }

    fn text(&self, l: usize) -> &[char] {
        let (a, b) = self.range(l);
        &self.c[a..b]
    }

    fn blank_line(&mut self, l: usize) {
        let (a, b) = self.range(l);
        for ch in &mut self.c[a..b] {
            *ch = ' ';
        }
    }

    /// 0-based line index of character `i`.
    fn line_of(&self, i: usize) -> usize {
        self.line.get(i).map(|l| (*l as usize).saturating_sub(1)).unwrap_or(0)
    }

    fn sc(&self, c: char, i: usize) -> SChar {
        SChar { c, line: self.line.get(i).copied().unwrap_or(0), col: self.col.get(i).copied().unwrap_or(0) }
    }
}

fn trimmed_eq(t: &[char], s: &str) -> bool {
    let mut end = t.len();
    while end > 0 && (t[end - 1] == ' ' || t[end - 1] == '\t') {
        end -= 1;
    }
    t[..end].iter().copied().eq(s.chars())
}

/// Number of leading lines that form a YAML (`---`) or TOML (`+++`) front-matter block;
/// 0 when the file has none or the block is never closed.
fn front_matter_lines(s: &Src) -> usize {
    if s.n_lines() < 2 {
        return 0;
    }
    let first = s.text(0);
    let closers: &[&str] = if trimmed_eq(first, "---") {
        &["---", "..."]
    } else if trimmed_eq(first, "+++") {
        &["+++"]
    } else {
        return 0;
    };
    for l in 1..s.n_lines() {
        let t = s.text(l);
        if closers.iter().any(|c| trimmed_eq(t, c)) {
            return l + 1;
        }
    }
    0
}

// ------------------------------------------------------------------ Markdown lines

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Paragraph text; joins the previous paragraph line.
    Para,
    /// Starts a new block (list item, block quote).
    Start,
    /// ATX heading: a block on its own.
    Heading,
    /// Table row: a block on its own; `|` separates cells.
    Table,
    /// Blank line.
    Blank,
    /// Not prose (front matter, fences, rules, link definitions); blanked out.
    Excluded,
}

fn indent(t: &[char]) -> usize {
    t.iter().take_while(|c| **c == ' ').count()
}

fn fence_open(t: &[char]) -> Option<(char, usize)> {
    let ind = indent(t);
    if ind > 3 {
        return None;
    }
    let ch = *t.get(ind)?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let n = t[ind..].iter().take_while(|c| **c == ch).count();
    if n < 3 || (ch == '`' && t[ind + n..].contains(&'`')) {
        return None;
    }
    Some((ch, n))
}

fn fence_close(t: &[char], ch: char, n: usize) -> bool {
    let ind = indent(t);
    if ind > 3 {
        return false;
    }
    let run = t[ind..].iter().take_while(|c| **c == ch).count();
    run >= n && t[ind + run..].iter().all(|c| c.is_whitespace())
}

fn is_heading(t: &[char]) -> bool {
    let ind = indent(t);
    if ind > 3 {
        return false;
    }
    let n = t[ind..].iter().take_while(|c| **c == '#').count();
    (1..=6).contains(&n) && t.get(ind + n).map_or(true, |c| *c == ' ' || *c == '\t')
}

fn is_rule_line(t: &[char]) -> bool {
    let mut it = t.iter().filter(|c| !c.is_whitespace());
    let Some(&first) = it.next() else {
        return false;
    };
    if !matches!(first, '-' | '*' | '_' | '=') {
        return false;
    }
    let mut n = 1;
    for &c in it {
        if c != first {
            return false;
        }
        n += 1;
    }
    n >= 3
}

fn is_list_item(t: &[char]) -> bool {
    let ind = indent(t);
    let rest = &t[ind..];
    let sep = |c: Option<&char>| c.map_or(true, |c| *c == ' ' || *c == '\t');
    match rest.first() {
        Some('-' | '*' | '+') => sep(rest.get(1)),
        Some(c) if c.is_ascii_digit() => {
            let d = rest.iter().take_while(|c| c.is_ascii_digit()).count();
            d <= 9 && matches!(rest.get(d), Some('.' | ')')) && sep(rest.get(d + 1))
        }
        _ => false,
    }
}

fn is_quote(t: &[char]) -> bool {
    let ind = indent(t);
    ind <= 3 && t.get(ind) == Some(&'>')
}

fn is_table_delim(t: &[char]) -> bool {
    t.contains(&'|') && t.contains(&'-') && t.iter().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

fn norm_label(t: &[char]) -> String {
    let s: String = t.iter().collect::<String>().to_lowercase();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `[label]: destination` reference definition (not a `[^footnote]:`).
fn ref_def(t: &[char]) -> Option<(String, String)> {
    let ind = indent(t);
    if ind > 3 || t.get(ind) != Some(&'[') || t.get(ind + 1) == Some(&'^') {
        return None;
    }
    let close = ind + 1 + t[ind + 1..].iter().position(|c| *c == ']')?;
    if t[ind + 1..close].contains(&'[') || t.get(close + 1) != Some(&':') {
        return None;
    }
    let label = norm_label(&t[ind + 1..close]);
    let rest: String = t[close + 2..].iter().collect();
    let dest = rest.split_whitespace().next()?;
    let dest = dest.strip_prefix('<').and_then(|d| d.strip_suffix('>')).unwrap_or(dest);
    if label.is_empty() {
        return None;
    }
    Some((label, dest.to_string()))
}

/// Classify Markdown lines and blank the excluded ones. Returns line kinds and link references.
fn classify(s: &mut Src, fm: usize) -> (Vec<Kind>, BTreeMap<String, String>) {
    let n = s.n_lines();
    let mut kinds = vec![Kind::Excluded; n];
    let mut refs = BTreeMap::new();
    let mut fence: Option<(char, usize)> = None;
    let mut prev_quote = false;
    for (l, kind) in kinds.iter_mut().enumerate().skip(fm) {
        let t = s.text(l);
        if let Some((ch, len)) = fence {
            if fence_close(t, ch, len) {
                fence = None;
            }
            *kind = Kind::Excluded;
            prev_quote = false;
            continue;
        }
        let quote = is_quote(t);
        *kind = if let Some(f) = fence_open(t) {
            fence = Some(f);
            Kind::Excluded
        } else if t.iter().all(|c| c.is_whitespace()) {
            Kind::Blank
        } else if is_heading(t) {
            Kind::Heading
        } else if is_rule_line(t) || is_table_delim(t) {
            Kind::Excluded
        } else if let Some((label, dest)) = ref_def(t) {
            refs.entry(label).or_insert(dest);
            Kind::Excluded
        } else if t.get(indent(t)) == Some(&'|') {
            Kind::Table
        } else if is_list_item(t) || (quote && !prev_quote) {
            Kind::Start
        } else {
            Kind::Para
        };
        prev_quote = quote;
    }
    for (l, k) in kinds.iter().enumerate() {
        if *k == Kind::Excluded {
            s.blank_line(l);
        }
    }
    (kinds, refs)
}

// ------------------------------------------------------------------ inline scanner

enum Tok {
    Ch(SChar),
    Link(String),
    Break,
}

const RAW_ELEMENTS: [&str; 4] = ["code", "pre", "script", "style"];
const BLOCK_ELEMENTS: [&str; 44] = [
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "br",
    "caption",
    "dd",
    "details",
    "dialog",
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
    "option",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "td",
    "th",
    "title",
    "tr",
    "ul",
    "button",
];

struct Tag {
    name: String,
    closing: bool,
    self_closing: bool,
    attrs: BTreeMap<String, String>,
    /// Index just past the closing `>`.
    end: usize,
}

struct Engine<'a> {
    s: &'a Src,
    md: bool,
    kinds: Vec<Kind>,
    refs: BTreeMap<String, String>,
    toks: Vec<Tok>,
    comments: Vec<Comment>,
}

fn starts_with_at(c: &[char], i: usize, pat: &str) -> bool {
    let mut j = i;
    for p in pat.chars() {
        match c.get(j) {
            Some(x) if *x == p => j += 1,
            _ => return false,
        }
    }
    true
}

fn starts_with_ci(c: &[char], i: usize, pat: &str) -> bool {
    let mut j = i;
    for p in pat.chars() {
        match c.get(j) {
            Some(x) if x.to_ascii_lowercase() == p => j += 1,
            _ => return false,
        }
    }
    true
}

fn find_from(c: &[char], from: usize, pat: &str) -> Option<usize> {
    (from..c.len()).find(|&j| starts_with_at(c, j, pat))
}

impl<'a> Engine<'a> {
    fn emit(&mut self, ch: char, i: usize) {
        let sc = self.s.sc(ch, i);
        self.toks.push(Tok::Ch(sc));
    }

    fn kind(&self, line: usize) -> Kind {
        self.kinds.get(line).copied().unwrap_or(Kind::Blank)
    }

    fn run(&mut self) {
        let src: &'a Src = self.s;
        let c = &src.c;
        let n = c.len();
        let mut i = 0usize;
        // Markdown link constructs: (index of the closing `]`, index to resume at).
        let mut jumps: Vec<(usize, usize)> = Vec::new();
        while i < n {
            if let Some(&(close, resume)) = jumps.last() {
                if i >= close {
                    jumps.pop();
                    i = i.max(resume);
                    continue;
                }
            }
            let ch = c[i];
            let next = c.get(i + 1).copied();
            match ch {
                '\n' => {
                    if self.md {
                        let l = self.s.line_of(i);
                        let joins = matches!(self.kind(l), Kind::Para | Kind::Start) && self.kind(l + 1) == Kind::Para;
                        if joins {
                            self.emit(' ', i);
                        } else {
                            self.toks.push(Tok::Break);
                        }
                    } else {
                        self.emit(' ', i);
                    }
                    i += 1;
                }
                '<' => i = self.angle(i),
                '&' => i = self.entity(i),
                '\\' if self.md => match next {
                    Some(p) if p.is_ascii_punctuation() => {
                        self.emit(p, i + 1);
                        i += 2;
                    }
                    _ => {
                        self.emit('\\', i);
                        i += 1;
                    }
                },
                '`' if self.md => i = self.code_span(i),
                '!' if self.md && next == Some('[') => {
                    if let Some((close, resume, _)) = self.md_link(i + 1) {
                        jumps.push((close, resume));
                        i += 2;
                    } else {
                        self.emit('!', i);
                        i += 1;
                    }
                }
                '[' if self.md => {
                    if let Some((close, resume, dest)) = self.md_link(i) {
                        self.toks.push(Tok::Link(dest));
                        jumps.push((close, resume));
                    } else {
                        self.emit('[', i);
                    }
                    i += 1;
                }
                '|' if self.md && self.kind(self.s.line_of(i)) == Kind::Table => {
                    self.toks.push(Tok::Break);
                    i += 1;
                }
                'h' if self.md && self.bare_url_start(i) => i = self.bare_url(i),
                w if w.is_whitespace() => {
                    self.emit(' ', i);
                    i += 1;
                }
                other => {
                    self.emit(other, i);
                    i += 1;
                }
            }
        }
    }

    /// Inline code span: skipped when a closing run of the same length is on the same line.
    fn code_span(&mut self, i: usize) -> usize {
        let src: &'a Src = self.s;
        let c = &src.c;
        let run = c[i..].iter().take_while(|x| **x == '`').count();
        let mut j = i + run;
        while j < c.len() && c[j] != '\n' {
            if c[j] == '`' {
                let r = c[j..].iter().take_while(|x| **x == '`').count();
                if r == run {
                    return j + r;
                }
                j += r;
            } else {
                j += 1;
            }
        }
        for k in i..i + run {
            self.emit('`', k);
        }
        i + run
    }

    /// Index of the `]` matching the `[` at `open`, staying inside one paragraph.
    fn close_bracket(&self, open: usize) -> Option<usize> {
        let src: &'a Src = self.s;
        let c = &src.c;
        let mut depth = 0usize;
        let mut j = open;
        let lim = (open + MAX_LINK_CHARS).min(c.len());
        while j < lim {
            match c[j] {
                '\\' => j += 2,
                '`' => {
                    let run = c[j..].iter().take_while(|x| **x == '`').count();
                    j += run;
                }
                '[' => {
                    depth += 1;
                    j += 1;
                }
                ']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(j);
                    }
                    j += 1;
                }
                '\n' => {
                    if self.kind(self.s.line_of(j) + 1) != Kind::Para {
                        return None;
                    }
                    j += 1;
                }
                _ => j += 1,
            }
        }
        None
    }

    /// A Markdown inline or reference link starting with `[` at `open`:
    /// (index of the closing `]`, index after the whole construct, destination).
    fn md_link(&self, open: usize) -> Option<(usize, usize, String)> {
        let src: &'a Src = self.s;
        let c = &src.c;
        let close = self.close_bracket(open)?;
        let lim = (open + MAX_LINK_CHARS).min(c.len());
        match c.get(close + 1) {
            Some('(') => {
                let mut p = close + 2;
                while p < lim && c[p].is_whitespace() {
                    p += 1;
                }
                let dest: String = if c.get(p) == Some(&'<') {
                    let start = p + 1;
                    let end = start + c.get(start..lim)?.iter().position(|x| *x == '>' || *x == '\n' || *x == '<')?;
                    if c[end] != '>' {
                        return None;
                    }
                    p = end + 1;
                    c[start..end].iter().collect()
                } else {
                    let start = p;
                    let mut depth = 0usize;
                    while p < lim {
                        let x = c[p];
                        if x.is_whitespace() {
                            break;
                        }
                        if x == '\\' {
                            p += 2;
                            continue;
                        }
                        if x == '(' {
                            depth += 1;
                        } else if x == ')' {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                        }
                        p += 1;
                    }
                    let p_end = p.min(lim);
                    p = p_end;
                    c[start..p_end].iter().collect()
                };
                while p < lim && c[p].is_whitespace() {
                    p += 1;
                }
                if let Some(&q) = c.get(p) {
                    if q == '"' || q == '\'' || q == '(' {
                        let closer = if q == '(' { ')' } else { q };
                        let rel = c.get(p + 1..lim)?.iter().position(|x| *x == closer)?;
                        p = p + 1 + rel + 1;
                        while p < lim && c[p].is_whitespace() {
                            p += 1;
                        }
                    }
                }
                if p < lim && c[p] == ')' {
                    Some((close, p + 1, dest))
                } else {
                    None
                }
            }
            Some('[') => {
                let start = close + 2;
                let end = start + c.get(start..lim)?.iter().position(|x| *x == ']' || *x == '[')?;
                if c[end] != ']' {
                    return None;
                }
                let label = if end == start { norm_label(&c[open + 1..close]) } else { norm_label(&c[start..end]) };
                self.refs.get(&label).map(|d| (close, end + 1, d.clone()))
            }
            _ => {
                if c.get(open + 1) == Some(&'^') {
                    return None;
                }
                let label = norm_label(&c[open + 1..close]);
                self.refs.get(&label).map(|d| (close, close + 1, d.clone()))
            }
        }
    }

    fn bare_url_start(&self, i: usize) -> bool {
        let src: &'a Src = self.s;
        let c = &src.c;
        let boundary = i == 0 || {
            let p = c[i - 1];
            !(p.is_alphanumeric() || matches!(p, '/' | ':' | '@' | '.' | '-' | '_'))
        };
        boundary && (starts_with_at(c, i, "http://") || starts_with_at(c, i, "https://"))
    }

    /// A bare `http(s)://` URL (GFM autolink): recorded as a link, not emitted as text.
    fn bare_url(&mut self, i: usize) -> usize {
        let src: &'a Src = self.s;
        let c = &src.c;
        let mut j = i;
        while j < c.len() && !c[j].is_whitespace() && !matches!(c[j], '<' | '>' | ']' | '[') {
            j += 1;
        }
        let mut end = j;
        while end > i {
            let last = c[end - 1];
            let opens = c[i..end].iter().filter(|x| **x == '(').count();
            let closes = c[i..end].iter().filter(|x| **x == ')').count();
            if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '*' | '_' | '~')
                || (last == ')' && closes > opens)
            {
                end -= 1;
            } else {
                break;
            }
        }
        self.toks.push(Tok::Link(c[i..end].iter().collect()));
        end
    }

    /// `&...;` character reference.
    fn entity(&mut self, i: usize) -> usize {
        let src: &'a Src = self.s;
        let c = &src.c;
        let lim = (i + 34).min(c.len());
        if let Some(rel) = c[i + 1..lim].iter().position(|x| *x == ';') {
            let name: String = c[i + 1..i + 1 + rel].iter().collect();
            if let Some(ch) = decode_entity(&name) {
                let ch = if ch.is_whitespace() { ' ' } else { ch };
                self.emit(ch, i);
                return i + rel + 2;
            }
        }
        self.emit('&', i);
        i + 1
    }

    /// Everything that starts with `<`: comments, declarations, autolinks and tags.
    fn angle(&mut self, i: usize) -> usize {
        let src: &'a Src = self.s;
        let c = &src.c;
        let n = c.len();
        if starts_with_at(c, i, "<!--") {
            let (end, after) = match find_from(c, i + 4, "-->") {
                Some(e) => (e, e + 3),
                None => (n, n),
            };
            let text: String = c[i + 4..end].iter().collect();
            self.comments.push(Comment { line: self.s.sc('<', i).line, text });
            return after;
        }
        if matches!(c.get(i + 1), Some('!' | '?')) {
            let lim = (i + MAX_TAG_CHARS).min(n);
            if let Some(rel) = c[i..lim].iter().position(|x| *x == '>') {
                return i + rel + 1;
            }
            self.emit('<', i);
            return i + 1;
        }
        if self.md {
            if let Some((url, after)) = autolink(c, i) {
                self.toks.push(Tok::Link(url));
                return after;
            }
        }
        let Some(tag) = parse_tag(c, i) else {
            self.emit('<', i);
            return i + 1;
        };
        if !tag.closing && (tag.name == "a" || tag.name == "area") {
            if let Some(href) = tag.attrs.get("href") {
                self.toks.push(Tok::Link(decode_entities(href)));
            }
        }
        if !tag.closing && !tag.self_closing && RAW_ELEMENTS.contains(&tag.name.as_str()) {
            let block = tag.name == "pre";
            if block {
                self.toks.push(Tok::Break);
            }
            let close_pat = format!("</{}", tag.name);
            let mut j = tag.end;
            let after = loop {
                if j >= n {
                    break n;
                }
                if starts_with_ci(c, j, &close_pat) {
                    let k = j + close_pat.chars().count();
                    if c.get(k).map_or(true, |x| *x == '>' || x.is_whitespace()) {
                        let lim = (k + MAX_TAG_CHARS).min(n);
                        break match c[k..lim].iter().position(|x| *x == '>') {
                            Some(rel) => k + rel + 1,
                            None => n,
                        };
                    }
                }
                j += 1;
            };
            if block {
                self.toks.push(Tok::Break);
            }
            return after;
        }
        if BLOCK_ELEMENTS.contains(&tag.name.as_str()) {
            self.toks.push(Tok::Break);
        }
        tag.end
    }
}

/// CommonMark autolink `<scheme:...>`; returns the URL and the index after `>`.
fn autolink(c: &[char], i: usize) -> Option<(String, usize)> {
    let start = i + 1;
    let first = *c.get(start)?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let scheme_len =
        c[start..].iter().take_while(|x| x.is_ascii_alphanumeric() || matches!(x, '+' | '.' | '-')).count();
    if !(2..=32).contains(&scheme_len) || c.get(start + scheme_len) != Some(&':') {
        return None;
    }
    let lim = (start + MAX_LINK_CHARS).min(c.len());
    let rel = c[start..lim].iter().position(|x| *x == '>' || *x == '<' || x.is_whitespace())?;
    let end = start + rel;
    if c[end] != '>' {
        return None;
    }
    Some((c[start..end].iter().collect(), end + 1))
}

/// Bounded HTML tag parser for `<name attr="v">`, `</name>` and `<name/>`.
fn parse_tag(c: &[char], i: usize) -> Option<Tag> {
    let lim = (i + MAX_TAG_CHARS).min(c.len());
    let mut j = i + 1;
    let closing = c.get(j) == Some(&'/');
    if closing {
        j += 1;
    }
    if !c.get(j)?.is_ascii_alphabetic() {
        return None;
    }
    let ns = j;
    while j < lim && (c[j].is_ascii_alphanumeric() || c[j] == '-') {
        j += 1;
    }
    let name: String = c[ns..j].iter().map(|x| x.to_ascii_lowercase()).collect();
    match c.get(j) {
        Some(x) if x.is_whitespace() || *x == '/' || *x == '>' => {}
        _ => return None,
    }
    let mut attrs = BTreeMap::new();
    let mut self_closing = false;
    loop {
        while j < lim && c[j].is_whitespace() {
            j += 1;
        }
        if j >= lim {
            return None;
        }
        if c[j] == '>' {
            j += 1;
            break;
        }
        if c[j] == '/' {
            if c.get(j + 1) == Some(&'>') {
                self_closing = true;
                j += 2;
                break;
            }
            j += 1;
            continue;
        }
        let an = j;
        while j < lim && !c[j].is_whitespace() && !matches!(c[j], '/' | '>' | '=') {
            j += 1;
        }
        if j == an {
            j += 1;
            continue;
        }
        let aname: String = c[an..j].iter().map(|x| x.to_ascii_lowercase()).collect();
        while j < lim && c[j].is_whitespace() {
            j += 1;
        }
        let mut value = String::new();
        if j < lim && c[j] == '=' {
            j += 1;
            while j < lim && c[j].is_whitespace() {
                j += 1;
            }
            if j < lim && (c[j] == '"' || c[j] == '\'') {
                let q = c[j];
                let rel = c[j + 1..lim].iter().position(|x| *x == q)?;
                value = c[j + 1..j + 1 + rel].iter().collect();
                j = j + 1 + rel + 1;
            } else {
                let vs = j;
                while j < lim && !c[j].is_whitespace() && c[j] != '>' {
                    j += 1;
                }
                value = c[vs..j].iter().collect();
            }
        }
        attrs.entry(aname).or_insert(value);
    }
    Some(Tag { name, closing, self_closing, attrs, end: j })
}

/// Decode one character reference name (without `&` and `;`).
fn decode_entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let v = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
            if hex.is_empty() || hex.len() > 6 {
                return None;
            }
            u32::from_str_radix(hex, 16).ok()?
        } else {
            if num.is_empty() || num.len() > 7 || !num.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            num.parse::<u32>().ok()?
        };
        return Some(char::from_u32(v).filter(|c| *c != '\0').unwrap_or('\u{fffd}'));
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "hellip" => '\u{2026}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "copy" => '\u{a9}',
        "reg" => '\u{ae}',
        "trade" => '\u{2122}',
        _ => return None,
    })
}

/// Decode character references in an attribute value.
fn decode_entities(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '&' {
            let lim = (i + 34).min(c.len());
            if let Some(rel) = c[i + 1..lim].iter().position(|x| *x == ';') {
                let name: String = c[i + 1..i + 1 + rel].iter().collect();
                if let Some(ch) = decode_entity(&name) {
                    out.push(ch);
                    i += rel + 2;
                    continue;
                }
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

// ------------------------------------------------------------------ sentences

struct Builder {
    out: Vec<Sentence>,
    cur: Vec<SChar>,
    links: Vec<String>,
    /// Links seen after a possible sentence end; they go to whichever sentence the next character joins.
    pending: Vec<String>,
}

impl Builder {
    fn at_possible_end(&self) -> bool {
        let n = self.cur.len();
        n >= 2 && self.cur[n - 1].c == ' ' && matches!(self.cur[n - 2].c, '.' | '!' | '?')
    }

    fn flush(&mut self) {
        while self.cur.last().is_some_and(|s| s.c == ' ') {
            self.cur.pop();
        }
        let links = std::mem::take(&mut self.links);
        if let Some(first) = self.cur.first().copied() {
            let chars = std::mem::take(&mut self.cur);
            self.out.push(Sentence { line: first.line, col: first.col, chars, links });
        }
    }
}

fn split_sentences(toks: Vec<Tok>) -> Vec<Sentence> {
    let mut b = Builder { out: Vec::new(), cur: Vec::new(), links: Vec::new(), pending: Vec::new() };
    for t in toks {
        match t {
            Tok::Break => {
                let p = std::mem::take(&mut b.pending);
                b.links.extend(p);
                b.flush();
            }
            Tok::Link(u) => {
                if b.at_possible_end() {
                    b.pending.push(u);
                } else {
                    b.links.push(u);
                }
            }
            Tok::Ch(sc) => {
                if sc.c == ' ' {
                    if b.cur.last().is_some_and(|l| l.c != ' ') {
                        b.cur.push(sc);
                    }
                    continue;
                }
                let p = std::mem::take(&mut b.pending);
                if b.at_possible_end() && sc.c.is_uppercase() {
                    b.flush();
                }
                b.links.extend(p);
                b.cur.push(sc);
            }
        }
    }
    let p = std::mem::take(&mut b.pending);
    b.links.extend(p);
    b.flush();
    b.out
}
