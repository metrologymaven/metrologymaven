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
/// Longest Markdown link text or destination the scanner accepts, in characters.
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

/// Why a file could not be read as prose: an opener whose end is missing would hide the rest
/// of the file, so the lint fails closed instead of skipping it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractError {
    pub line: u32,
    pub reason: String,
}

/// Extract sentences and comments from a file's text.
pub fn extract(text: &str, format: Format) -> Result<Extracted, ExtractError> {
    let mut src = Src::new(text);
    let fm = front_matter_lines(&src);
    for l in 0..fm {
        src.blank_line(l);
    }
    let (kinds, refs) = match format {
        Format::Markdown => classify(&mut src, fm),
        Format::Html => (Vec::new(), BTreeMap::new()),
    };
    let md = format == Format::Markdown;
    let brackets = if md { match_brackets(&src, &kinds) } else { Vec::new() };
    let mut eng = Engine {
        s: &src,
        md,
        kinds,
        refs,
        brackets,
        gt: None,
        runs: None,
        toks: Vec::new(),
        comments: Vec::new(),
        error: None,
    };
    eng.run();
    let Engine { toks, comments, error, .. } = eng;
    if let Some(e) = error {
        return Err(e);
    }
    Ok(Extracted { sentences: split_sentences(toks), comments })
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
    let mut words = rest.split_whitespace();
    let raw = words.next()?;
    let title_ok = words.next().map_or(true, |w| w.starts_with(['"', '\'', '(']));
    let (dest, angled) = match raw.strip_prefix('<').and_then(|d| d.strip_suffix('>')) {
        Some(d) => (d, true),
        None => (raw, false),
    };
    if label.is_empty() || !title_ok || !(angled || url_like(dest)) {
        return None;
    }
    Some((label, dest.to_string()))
}

/// A reference-definition destination that looks like a URL or path, so that prose such as
/// `[Note]: read this first.` stays text.
fn url_like(d: &str) -> bool {
    let scheme_len = d.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')).count();
    let has_scheme = d.starts_with(|c: char| c.is_ascii_alphabetic())
        && (2..=32).contains(&scheme_len)
        && d[scheme_len..].starts_with(':')
        && d.len() > scheme_len + 1;
    has_scheme
        || d.starts_with('/')
        || d.starts_with('#')
        || d.starts_with("./")
        || d.starts_with("../")
        || d.contains('/')
        || d.trim_end_matches('.').contains('.')
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
//
// Every lookahead is bounded and amortised linear, so adversarial input cannot make the scan
// quadratic: brackets are matched once up front; the next `>` is cached; backtick runs are
// indexed once per line; link-destination and title scans stop at `[` or `]`, so two scans
// never cover the same characters; reference labels are only normalised when bracket-free.

enum Tok {
    Ch(SChar),
    Link(String),
    Break,
}

/// Longest bare URL recorded as a link; a longer run of URL characters is read as text.
pub const MAX_URL_CHARS: usize = 8192;
/// Longest link label (CommonMark's limit).
const MAX_LABEL_CHARS: usize = 999;
const NO_MATCH: u32 = u32::MAX;

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

/// Characters that are invisible in rendered text and would split a word: dropped.
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{ad}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}')
}

struct Tag {
    name: String,
    closing: bool,
    self_closing: bool,
    attrs: BTreeMap<String, String>,
    /// Index just past the closing `>`.
    end: usize,
}

/// A backtick run of one line: (start, length, index of the next run of the same length or `usize::MAX`).
type Run = (usize, usize, usize);

struct Engine<'a> {
    s: &'a Src,
    md: bool,
    kinds: Vec<Kind>,
    refs: BTreeMap<String, String>,
    /// Markdown: for each `[`, the index of its matching `]` (or [`NO_MATCH`]).
    brackets: Vec<u32>,
    /// Cache of the last `>` search: (searched from, first `>` at or after it).
    gt: Option<(usize, Option<usize>)>,
    /// Backtick runs of the line last asked about.
    runs: Option<(usize, Vec<Run>)>,
    toks: Vec<Tok>,
    comments: Vec<Comment>,
    error: Option<ExtractError>,
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

/// Match `[` and `]` once for the whole file (stack; escapes skipped; reset at block ends).
fn match_brackets(s: &Src, kinds: &[Kind]) -> Vec<u32> {
    let c = &s.c;
    let mut out = vec![NO_MATCH; c.len()];
    let mut stack: Vec<usize> = Vec::new();
    let mut j = 0;
    while j < c.len() {
        match c[j] {
            '\\' => {
                j += 2;
                continue;
            }
            '[' => stack.push(j),
            ']' => {
                if let Some(o) = stack.pop() {
                    if j - o <= MAX_LINK_CHARS {
                        out[o] = u32::try_from(j).unwrap_or(NO_MATCH);
                    }
                }
            }
            '\n' if kinds.get(s.line_of(j) + 1).copied().unwrap_or(Kind::Blank) != Kind::Para => stack.clear(),
            _ => {}
        }
        j += 1;
    }
    out
}

/// Backtick runs of line `line`, each linked to the next run of the same length.
fn line_runs(s: &Src, line: usize) -> Vec<Run> {
    let (a, b) = s.range(line);
    let c = &s.c;
    let mut runs: Vec<Run> = Vec::new();
    let mut j = a;
    while j < b {
        if c[j] == '`' {
            let start = j;
            while j < b && c[j] == '`' {
                j += 1;
            }
            runs.push((start, j - start, usize::MAX));
        } else {
            j += 1;
        }
    }
    let mut last: BTreeMap<usize, usize> = BTreeMap::new();
    for (k, r) in runs.iter_mut().enumerate().rev() {
        if let Some(&nx) = last.get(&r.1) {
            r.2 = nx;
        }
        last.insert(r.1, k);
    }
    runs
}

/// True when `t` holds no `[` or `]` (stops at the first one, so callers stay linear).
fn bracket_free(t: &[char]) -> bool {
    !t.iter().any(|x| matches!(x, '[' | ']'))
}

/// Inline link tail `(dest "title")` starting just after `(` at `p0`:
/// (index after `)`, destination). Scans stop at `[` and `]`.
fn link_dest(c: &[char], p0: usize) -> Option<(usize, String)> {
    let lim = p0.saturating_add(MAX_LINK_CHARS).min(c.len());
    let stop = |x: char| matches!(x, '[' | ']');
    let mut p = p0.min(lim);
    while p < lim && c[p].is_whitespace() {
        p += 1;
    }
    let dest: String = if p < lim && c[p] == '<' {
        let start = p + 1;
        let mut e = start;
        while e < lim && !matches!(c[e], '>' | '<' | '\n') && !stop(c[e]) {
            e += 1;
        }
        if e >= lim || c[e] != '>' {
            return None;
        }
        p = e + 1;
        c[start..e].iter().collect()
    } else {
        let start = p;
        let mut depth = 0usize;
        while p < lim {
            let x = c[p];
            if x.is_whitespace() || stop(x) {
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
        p = p.min(lim);
        c[start..p].iter().collect()
    };
    while p < lim && c[p].is_whitespace() {
        p += 1;
    }
    if p < lim && matches!(c[p], '"' | '\'' | '(') {
        let closer = if c[p] == '(' { ')' } else { c[p] };
        let mut e = p + 1;
        while e < lim && c[e] != closer && !stop(c[e]) {
            e += 1;
        }
        if e >= lim || c[e] != closer {
            return None;
        }
        p = e + 1;
        while p < lim && c[p].is_whitespace() {
            p += 1;
        }
    }
    if p < lim && c[p] == ')' {
        Some((p + 1, dest))
    } else {
        None
    }
}

impl<'a> Engine<'a> {
    fn emit(&mut self, ch: char, i: usize) {
        let sc = self.s.sc(ch, i);
        self.toks.push(Tok::Ch(sc));
    }

    fn kind(&self, line: usize) -> Kind {
        self.kinds.get(line).copied().unwrap_or(Kind::Blank)
    }

    fn fail(&mut self, i: usize, reason: String) {
        if self.error.is_none() {
            self.error = Some(ExtractError { line: self.s.sc(' ', i).line, reason });
        }
    }

    /// First `>` at or after `i` (cached; the scan position only moves forward).
    fn next_gt(&mut self, i: usize) -> Option<usize> {
        if let Some((from, at)) = self.gt {
            if from <= i && at.map_or(true, |a| a >= i) {
                return at;
            }
        }
        let src: &'a Src = self.s;
        let c = &src.c;
        let at = (i..c.len()).find(|&j| c[j] == '>');
        self.gt = Some((i, at));
        at
    }

    fn run(&mut self) {
        let src: &'a Src = self.s;
        let c = &src.c;
        let n = c.len();
        let mut i = 0usize;
        // Markdown link constructs: (index of the closing `]`, index to resume at).
        let mut jumps: Vec<(usize, usize)> = Vec::new();
        while i < n && self.error.is_none() {
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
                        let l = src.line_of(i);
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
                '|' if self.md && self.kind(src.line_of(i)) == Kind::Table => {
                    self.toks.push(Tok::Break);
                    i += 1;
                }
                'h' if self.md && self.bare_url_start(i) => i = self.bare_url(i),
                w if w.is_whitespace() => {
                    self.emit(' ', i);
                    i += 1;
                }
                other => {
                    if !is_invisible(other) {
                        self.emit(other, i);
                    }
                    i += 1;
                }
            }
        }
    }

    /// Inline code span: skipped when a run of the same length follows on the same line.
    fn code_span(&mut self, i: usize) -> usize {
        let src: &'a Src = self.s;
        let c = &src.c;
        let line = src.line_of(i);
        if self.runs.as_ref().map_or(true, |(l, _)| *l != line) {
            self.runs = Some((line, line_runs(src, line)));
        }
        let run = c[i..].iter().take_while(|x| **x == '`').count();
        let after = self.runs.as_ref().and_then(|(_, runs)| {
            let k = runs.binary_search_by_key(&i, |r| r.0).ok()?;
            runs.get(runs[k].2).map(|r| r.0 + r.1)
        });
        if let Some(after) = after {
            return after;
        }
        for k in i..i + run {
            self.emit('`', k);
        }
        i + run
    }

    /// A Markdown inline or reference link starting with `[` at `open`:
    /// (index of the closing `]`, index after the whole construct, destination).
    fn md_link(&mut self, open: usize) -> Option<(usize, usize, String)> {
        let src: &'a Src = self.s;
        let c = &src.c;
        let close = *self.brackets.get(open)?;
        if close == NO_MATCH {
            return None;
        }
        let close = close as usize;
        let text_label = || {
            let t = &c[open + 1..close];
            if t.len() <= MAX_LABEL_CHARS && bracket_free(t) {
                Some(norm_label(t))
            } else {
                None
            }
        };
        match c.get(close + 1) {
            Some('(') => link_dest(c, close + 2).map(|(resume, dest)| (close, resume, dest)),
            Some('[') => {
                let start = close + 2;
                let lim = start.saturating_add(MAX_LABEL_CHARS).min(c.len());
                let mut e = start;
                while e < lim && !matches!(c[e], '[' | ']') {
                    e += 1;
                }
                if e >= lim || c[e] != ']' {
                    return None;
                }
                let label = if e == start { text_label()? } else { norm_label(&c[start..e]) };
                self.refs.get(&label).map(|d| (close, e + 1, d.clone()))
            }
            _ => {
                if self.refs.is_empty() || c.get(open + 1) == Some(&'^') {
                    return None;
                }
                let label = text_label()?;
                self.refs.get(&label).map(|d| (close, close + 1, d.clone()))
            }
        }
    }

    fn bare_url_start(&self, i: usize) -> bool {
        let c = &self.s.c;
        let boundary = i == 0 || {
            let p = c[i - 1];
            !(p.is_alphanumeric() || matches!(p, '/' | ':' | '@' | '.' | '-' | '_'))
        };
        boundary && (starts_with_at(c, i, "http://") || starts_with_at(c, i, "https://"))
    }

    /// A bare `http(s)://` URL (GFM autolink): recorded as a link, not emitted as text.
    /// A run longer than [`MAX_URL_CHARS`] is read as text instead.
    fn bare_url(&mut self, i: usize) -> usize {
        let src: &'a Src = self.s;
        let c = &src.c;
        let mut j = i;
        while j < c.len() && !c[j].is_whitespace() && !matches!(c[j], '<' | '>' | ']' | '[') {
            j += 1;
        }
        if j - i > MAX_URL_CHARS {
            for (k, &ch) in c.iter().enumerate().take(j).skip(i) {
                if !is_invisible(ch) {
                    self.emit(ch, k);
                }
            }
            return j;
        }
        let opens = c[i..j].iter().filter(|x| **x == '(').count();
        let mut closes = c[i..j].iter().filter(|x| **x == ')').count();
        let mut end = j;
        while end > i {
            let last = c[end - 1];
            if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '*' | '_' | '~') {
                end -= 1;
            } else if last == ')' && closes > opens {
                closes -= 1;
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
            match decode_entity(&name) {
                Some(Some(ch)) => {
                    let ch = if ch.is_whitespace() { ' ' } else { ch };
                    self.emit(ch, i);
                    return i + rel + 2;
                }
                Some(None) => return i + rel + 2,
                None => {}
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
            let Some(end) = find_from(c, i + 4, "-->") else {
                self.fail(i, "unclosed HTML comment `<!--`: the rest of the file would be hidden".to_string());
                return n;
            };
            let text: String = c[i + 4..end].iter().collect();
            self.comments.push(Comment { line: src.sc('<', i).line, text });
            return end + 3;
        }
        if matches!(c.get(i + 1), Some('!' | '?')) {
            if let Some(g) = self.next_gt(i) {
                if g - i <= MAX_TAG_CHARS {
                    return g + 1;
                }
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
        let tag = match self.next_gt(i) {
            Some(g) if g - i <= MAX_TAG_CHARS => parse_tag(c, i, g),
            _ => None,
        };
        let Some(tag) = tag else {
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
            let mut after = None;
            while j < n {
                if starts_with_ci(c, j, &close_pat) {
                    let k = j + close_pat.chars().count();
                    if c.get(k).map_or(true, |x| *x == '>' || x.is_whitespace()) {
                        let lim = (k + MAX_TAG_CHARS).min(n);
                        after = c[k..lim].iter().position(|x| *x == '>').map(|rel| k + rel + 1);
                        break;
                    }
                }
                j += 1;
            }
            let Some(after) = after else {
                self.fail(i, format!("unclosed <{}> element: the rest of the file would be hidden", tag.name));
                return n;
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

/// HTML tag from `<` at `i` to the first `>` at `gt` (a `>` inside a quoted attribute value
/// also ends the tag). The caller bounds `gt - i`.
fn parse_tag(c: &[char], i: usize, gt: usize) -> Option<Tag> {
    let mut j = i + 1;
    let closing = c.get(j) == Some(&'/');
    if closing {
        j += 1;
    }
    if !c.get(j)?.is_ascii_alphabetic() {
        return None;
    }
    let ns = j;
    while j < gt && (c[j].is_ascii_alphanumeric() || c[j] == '-') {
        j += 1;
    }
    let name: String = c[ns..j].iter().map(|x| x.to_ascii_lowercase()).collect();
    match c.get(j) {
        Some(x) if x.is_whitespace() || *x == '/' || *x == '>' => {}
        _ => return None,
    }
    let mut attrs = BTreeMap::new();
    let mut self_closing = false;
    while j < gt {
        let x = c[j];
        if x.is_whitespace() {
            j += 1;
            continue;
        }
        if x == '/' {
            if j + 1 == gt {
                self_closing = true;
            }
            j += 1;
            continue;
        }
        let an = j;
        while j < gt && !c[j].is_whitespace() && !matches!(c[j], '/' | '=') {
            j += 1;
        }
        if j == an {
            j += 1;
            continue;
        }
        let aname: String = c[an..j].iter().map(|x| x.to_ascii_lowercase()).collect();
        while j < gt && c[j].is_whitespace() {
            j += 1;
        }
        let mut value = String::new();
        if j < gt && c[j] == '=' {
            j += 1;
            while j < gt && c[j].is_whitespace() {
                j += 1;
            }
            if j < gt && (c[j] == '"' || c[j] == '\'') {
                let q = c[j];
                let vs = j + 1;
                let ve = c[vs..gt].iter().position(|x| *x == q).map_or(gt, |r| vs + r);
                value = c[vs..ve].iter().collect();
                j = if ve < gt { ve + 1 } else { gt };
            } else {
                let vs = j;
                while j < gt && !c[j].is_whitespace() {
                    j += 1;
                }
                value = c[vs..j].iter().collect();
            }
        }
        attrs.entry(aname).or_insert(value);
    }
    Some(Tag { name, closing, self_closing, attrs, end: gt + 1 })
}

/// Decode one character reference name (without `&` and `;`). `Some(None)` means the
/// reference stands for an invisible character and is dropped; `None` means it is unknown.
fn decode_entity(name: &str) -> Option<Option<char>> {
    if let Some(num) = name.strip_prefix('#') {
        let (digits, radix) = match num.strip_prefix(|x: char| x == 'x' || x == 'X') {
            Some(h) => (h, 16),
            None => (num, 10),
        };
        if digits.is_empty() || !digits.chars().all(|d| d.is_digit(radix)) {
            return None;
        }
        let sig = digits.trim_start_matches('0');
        let v = if sig.is_empty() {
            Some(0)
        } else if sig.len() > 8 {
            None
        } else {
            u32::from_str_radix(sig, radix).ok()
        };
        let ch = v.and_then(char::from_u32).filter(|x| *x != '\0').unwrap_or('\u{fffd}');
        return Some(if is_invisible(ch) { None } else { Some(ch) });
    }
    let ch = match name {
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
        "shy" | "zwj" | "zwnj" => return Some(None),
        _ => return None,
    };
    Some(Some(ch))
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
                if let Some(d) = decode_entity(&name) {
                    if let Some(ch) = d {
                        out.push(ch);
                    }
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
