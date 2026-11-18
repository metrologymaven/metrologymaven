//! Guide and recipe content standard linter (spec node `AGENT.SKILLS.STYLE`).
//!
//! Lints the prose agents are taught from: the curated task guides (title, summary and step
//! purposes, read from the manifest for every guide the catalog names), the rendered Skill of
//! each curated guide (`<skills>/<guide-id-with-hyphens>/SKILL.md`, body after front matter),
//! the recipes the catalog names (`<recipes>/<id>.json`: `title`, `prompt`, `commentary[]`)
//! and any extra Markdown given on the command line. The rules are written down for people in
//! `agent/STYLE.md`:
//!
//! - `result_like_number`: a number with a length unit, angle unit or `%` outside a fenced block;
//! - `forbidden_instruction`: naming an M1, S1, A0 or W3 command (ID, ID with major, or MCP tool
//!   name) anywhere, except the first such name after the phrase [`HUMAN_REQUEST_PHRASE`] on the same line;
//! - `missing_proposal_step`: a curated guide with a W1 or W2 step that does not end with
//!   [`BRIEF_STEP`] then [`PROMOTE_STEP`];
//! - `over_budget`: a curated Skill body over [`WORD_BUDGET`] whitespace-separated words;
//! - `possible_standards_text`: [`MIN_DENY_RUN`] or more consecutive prose words matching one
//!   deny-list entry word by word (each prose word starts with the entry's stem), or a quotation
//!   of more than [`MAX_QUOTE_WORDS`] words.
//!
//! Missing guides, Skills and recipes are the catalog check's findings (`AGENT.SKILLS.CATALOG`)
//! and are skipped here. Output is sorted by file, line, rule and detail (D0).

use crate::catalog::{parse_catalog, skill_dir_name, MAX_FILE_BYTES};
use fid_canon::CValue;
use fid_types::SideEffect;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest word count of a curated Skill body (REQ-AGENT.SKILLS.STYLE-004) [unverified budget].
pub const WORD_BUDGET: usize = 1500;
/// Shortest run of deny-list words reported (REQ-AGENT.SKILLS.STYLE-005) [unverified heuristic].
pub const MIN_DENY_RUN: usize = 12;
/// Longest quotation allowed, in words (REQ-AGENT.SKILLS.STYLE-005) [unverified heuristic].
pub const MAX_QUOTE_WORDS: usize = 25;
/// Second-to-last step of a state-changing guide (`AGENT.PROPOSE`).
pub const BRIEF_STEP: (&str, u32) = ("agent.attach_brief", 1);
/// Last step of a state-changing guide.
pub const PROMOTE_STEP: (&str, u32) = ("journal.request_promote_branch", 1);
/// A forbidden command may be named on a line only after this phrase (ASCII case-insensitive).
pub const HUMAN_REQUEST_PHRASE: &str = "request for a human";
/// Deepest JSON nesting accepted.
const MAX_DEPTH: usize = 64;
/// UTF-8 byte order mark, stripped before front-matter and fence detection.
const CHAR_BOM: char = '\u{feff}';
/// Deepest folder nesting searched for Markdown under a `--prose` folder.
pub const MAX_DIR_DEPTH: usize = 32;

/// Unit symbols that make a number look like a result: length, angle and percent. Symbols made
/// of letters must end at a word boundary.
const UNITS: &[(&str, &str)] = &[
    ("nm", "length"),
    ("µm", "length"),
    ("μm", "length"),
    ("um", "length"),
    ("mm", "length"),
    ("cm", "length"),
    ("dm", "length"),
    ("m", "length"),
    ("km", "length"),
    ("micron", "length"),
    ("microns", "length"),
    ("metre", "length"),
    ("metres", "length"),
    ("meter", "length"),
    ("meters", "length"),
    ("millimetre", "length"),
    ("millimetres", "length"),
    ("millimeter", "length"),
    ("millimeters", "length"),
    ("micrometre", "length"),
    ("micrometres", "length"),
    ("micrometer", "length"),
    ("micrometers", "length"),
    ("inch", "length"),
    ("inches", "length"),
    ("ft", "length"),
    ("mil", "length"),
    ("mils", "length"),
    ("thou", "length"),
    ("deg", "angle"),
    ("degree", "angle"),
    ("degrees", "angle"),
    ("°", "angle"),
    ("rad", "angle"),
    ("mrad", "angle"),
    ("µrad", "angle"),
    ("μrad", "angle"),
    ("urad", "angle"),
    ("arcmin", "angle"),
    ("arcsec", "angle"),
    ("′", "angle"),
    ("″", "angle"),
    ("%", "percent"),
    ("percent", "percent"),
];

/// Rule codes with stable wire strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    ResultLikeNumber,
    ForbiddenInstruction,
    MissingProposalStep,
    OverBudget,
    PossibleStandardsText,
    /// An input cannot be read or is malformed.
    BadInput,
}

impl Rule {
    /// Every rule, in `Ord` order.
    pub const ALL: &'static [Rule] = &[
        Rule::ResultLikeNumber,
        Rule::ForbiddenInstruction,
        Rule::MissingProposalStep,
        Rule::OverBudget,
        Rule::PossibleStandardsText,
        Rule::BadInput,
    ];

    /// Stable machine-readable code.
    pub fn as_str(self) -> &'static str {
        match self {
            Rule::ResultLikeNumber => "result_like_number",
            Rule::ForbiddenInstruction => "forbidden_instruction",
            Rule::MissingProposalStep => "missing_proposal_step",
            Rule::OverBudget => "over_budget",
            Rule::PossibleStandardsText => "possible_standards_text",
            Rule::BadInput => "bad_input",
        }
    }

    /// Parses a wire string.
    pub fn parse(s: &str) -> Option<Rule> {
        Rule::ALL.iter().copied().find(|r| r.as_str() == s)
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One finding. `file` is the path as given (with `/` separators); prose read from a JSON field
/// adds `#<pointer>` and then `line` counts lines within that field. Line 0 means the whole input.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LintFinding {
    pub file: String,
    pub line: usize,
    pub rule: Rule,
    pub detail: String,
}

impl LintFinding {
    fn new(file: &str, line: usize, rule: Rule, detail: impl Into<String>) -> LintFinding {
        LintFinding { file: file.to_string(), line, rule, detail: detail.into() }
    }
}

impl fmt::Display for LintFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}: {}", self.file, self.line, self.rule, self.detail)
    }
}

/// Paths of every input of the linter.
#[derive(Debug, Clone)]
pub struct StyleInputs {
    /// Registry manifest (`CORE.REG.MAN`): `commands[]` classes and `task_guides[]`.
    pub manifest: PathBuf,
    /// Catalog file (`agent/catalog.json`): which guides and recipes are curated.
    pub catalog: PathBuf,
    /// Generated Skills folder.
    pub skills: PathBuf,
    /// Recipe folder.
    pub recipes: PathBuf,
    /// Deny list of project-authored phrase stems (`agent/style-denylist.txt`).
    pub denylist: PathBuf,
    /// Optional maintainer allow list: lines `<rule> <line> <file>`.
    pub allow: Option<PathBuf>,
    /// Extra Markdown files, or folders searched recursively for `*.md`.
    pub prose: Vec<PathBuf>,
}

/// Result of a lint run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintReport {
    /// Sorted, without duplicates, allow-listed findings removed. Empty means pass.
    pub findings: Vec<LintFinding>,
    /// Prose units checked (guide fields, Skills, recipe fields, Markdown files).
    pub units: usize,
}

/// A command the prose must not name, with its wire label for details.
struct Forbidden {
    label: String,
}

/// Everything the prose rules need.
struct Ctx {
    forbidden: BTreeMap<String, Forbidden>,
    deny: Vec<(usize, Vec<String>)>,
}

// ------------------------------------------------------------------ input

fn label(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

/// Reads at most [`MAX_FILE_BYTES`] (or `max`) bytes. `Ok(None)` when the file does not exist.
fn read_file(path: &Path, max: usize) -> Result<Option<Vec<u8>>, String> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot open: {e}")),
    };
    let mut buf = Vec::new();
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    file.take(limit).read_to_end(&mut buf).map_err(|e| format!("cannot read: {e}"))?;
    if buf.len() > max {
        return Err(format!("larger than {max} bytes"));
    }
    Ok(Some(buf))
}

fn read_text(path: &Path, max: usize) -> Result<Option<String>, String> {
    match read_file(path, max)? {
        None => Ok(None),
        Some(b) => String::from_utf8(b)
            .map(|t| Some(t.strip_prefix(CHAR_BOM).map(str::to_string).unwrap_or(t)))
            .map_err(|_| "not UTF-8".to_string()),
    }
}

fn as_u32(v: &CValue) -> Option<u32> {
    match v {
        CValue::Num(n) if *n >= 0.0 && *n <= f64::from(u32::MAX) && n.fract() == 0.0 => Some(*n as u32),
        CValue::I64(i) => u32::try_from(*i).ok(),
        CValue::U64(u) => u32::try_from(*u).ok(),
        _ => None,
    }
}

fn get_str<'a>(m: &'a BTreeMap<String, CValue>, key: &str) -> Option<&'a str> {
    match m.get(key) {
        Some(CValue::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// MCP tool name of a command (CONSTITUTION §3): dots to underscores, `_vN` from major 2.
pub fn tool_name(id: &str, major: u32) -> String {
    let base = id.replace('.', "_");
    if major > 1 {
        format!("{base}_v{major}")
    } else {
        base
    }
}

struct Step {
    id: String,
    major: u32,
    purpose: String,
}

struct Guide {
    title: String,
    summary: String,
    steps: Vec<Step>,
}

struct Manifest {
    classes: BTreeMap<(String, u32), SideEffect>,
    guides: BTreeMap<String, Guide>,
}

fn parse_manifest(v: &CValue) -> Result<Manifest, String> {
    let CValue::Obj(top) = v else { return Err("not a JSON object".into()) };
    let mut classes = BTreeMap::new();
    if let Some(cmds) = top.get("commands") {
        let CValue::Arr(cmds) = cmds else { return Err("`commands` must be an array".into()) };
        for (i, c) in cmds.iter().enumerate() {
            let CValue::Obj(m) = c else { return Err(format!("commands[{i}]: not an object")) };
            let (Some(id), Some(major), Some(class)) =
                (get_str(m, "id"), m.get("major").and_then(as_u32), get_str(m, "side_effect"))
            else {
                return Err(format!("commands[{i}]: needs string `id`, integer `major`, string `side_effect`"));
            };
            let class = SideEffect::parse(class).map_err(|_| format!("commands[{i}]: unknown class {class:?}"))?;
            classes.insert((id.to_string(), major), class);
        }
    }
    let mut guides = BTreeMap::new();
    if let Some(gs) = top.get("task_guides") {
        let CValue::Arr(gs) = gs else { return Err("`task_guides` must be an array".into()) };
        for (i, g) in gs.iter().enumerate() {
            let CValue::Obj(m) = g else { return Err(format!("task_guides[{i}]: not an object")) };
            let Some(id) = get_str(m, "id") else { return Err(format!("task_guides[{i}]: `id` must be a string")) };
            let mut steps = Vec::new();
            if let Some(CValue::Arr(ss)) = m.get("steps") {
                for (j, s) in ss.iter().enumerate() {
                    let bad = || format!("task_guides[{i}].steps[{j}]: needs `command` {{id, major}} and `purpose`");
                    let CValue::Obj(sm) = s else { return Err(bad()) };
                    let Some(CValue::Obj(cmd)) = sm.get("command") else { return Err(bad()) };
                    let (Some(cid), Some(major)) = (get_str(cmd, "id"), cmd.get("major").and_then(as_u32)) else {
                        return Err(bad());
                    };
                    steps.push(Step {
                        id: cid.to_string(),
                        major,
                        purpose: get_str(sm, "purpose").unwrap_or("").to_string(),
                    });
                }
            }
            let guide = Guide {
                title: get_str(m, "title").unwrap_or("").to_string(),
                summary: get_str(m, "summary").unwrap_or("").to_string(),
                steps,
            };
            if guides.insert(id.to_string(), guide).is_some() {
                return Err(format!("task_guides: guide {id:?} declared twice"));
            }
        }
    }
    Ok(Manifest { classes, guides })
}

fn is_forbidden(c: SideEffect) -> bool {
    matches!(c, SideEffect::M1 | SideEffect::S1 | SideEffect::A0 | SideEffect::W3)
}

fn forbidden_names(m: &Manifest) -> BTreeMap<String, Forbidden> {
    let mut out = BTreeMap::new();
    for ((id, major), class) in &m.classes {
        if !is_forbidden(*class) {
            continue;
        }
        let wire = format!("{id}@{major}");
        let tool = tool_name(id, *major);
        for name in [id.clone(), wire.clone(), tool.clone()] {
            out.entry(name).or_insert_with(|| Forbidden { label: format!("{wire} ({class}, tool {tool})") });
        }
    }
    out
}

/// Parses a deny list: one entry per line, words are stems; `#` starts a comment line.
/// Returns (line number, lower-case stems) for entries long enough to ever match.
pub fn parse_denylist(text: &str) -> Vec<(usize, Vec<String>)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with('#'))
        .map(|(i, l)| (i + 1, l.split_whitespace().map(norm_word).filter(|w| !w.is_empty()).collect::<Vec<_>>()))
        .filter(|(_, stems)| stems.len() >= MIN_DENY_RUN)
        .collect()
}

struct Allow {
    rule: Rule,
    line: usize,
    file: String,
}

fn parse_allow(text: &str) -> Result<Vec<Allow>, String> {
    let mut out = Vec::new();
    for (i, l) in text.lines().enumerate() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let mut parts = l.splitn(3, char::is_whitespace);
        let (Some(rule), Some(line), Some(file)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!("line {}: expected `<rule> <line> <file>`", i + 1));
        };
        let rule = Rule::parse(rule)
            .filter(|r| *r != Rule::BadInput)
            .ok_or_else(|| format!("line {}: unknown rule {rule:?}", i + 1))?;
        let line = line.parse::<usize>().map_err(|_| format!("line {}: bad line number {line:?}", i + 1))?;
        out.push(Allow { rule, line, file: file.trim().replace('\\', "/") });
    }
    Ok(out)
}

fn allowed(f: &LintFinding, allow: &[Allow]) -> bool {
    allow.iter().any(|a| {
        a.rule == f.rule && a.line == f.line && (f.file == a.file || f.file.ends_with(&format!("/{}", a.file)))
    })
}

// ------------------------------------------------------------------ prose rules

/// Lower-case word with leading and trailing non-alphanumeric characters removed.
fn norm_word(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// Splits Markdown into (1-based line number, text, inside a fenced block). Fence marker lines
/// count as inside. Lines before `first_line` (front matter) are dropped.
fn classify_lines(text: &str, first_line: usize) -> Vec<(usize, &str, bool)> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        if n < first_line {
            continue;
        }
        let line = raw.trim_end_matches('\r');
        let t = line.trim_start();
        let marker = ['`', '~'].into_iter().find_map(|c| {
            let k = t.chars().take_while(|x| *x == c).count();
            (k >= 3).then_some((c, k))
        });
        match (fence, marker) {
            (None, Some(m)) => {
                fence = Some(m);
                out.push((n, line, true));
            }
            (Some((c, k)), Some((mc, mk))) if c == mc && mk >= k && t.trim_matches(c).trim().is_empty() => {
                fence = None;
                out.push((n, line, true));
            }
            (Some(_), _) => out.push((n, line, true)),
            (None, None) => out.push((n, line, false)),
        }
    }
    out
}

/// Number-plus-unit tokens in one line: (token text, unit kind).
pub fn result_like_numbers(line: &str) -> Vec<(String, &'static str)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev_word = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        if !c.is_ascii_digit() || prev_word {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i + 1 < chars.len() && (chars[i] == '.' || chars[i] == ',') && chars[i + 1].is_ascii_digit() {
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
        let num_end = i;
        let mut j = i;
        while j < chars.len() && (chars[j] == ' ' || chars[j] == '\u{a0}') && j - num_end < 2 {
            j += 1;
        }
        for (sym, kind) in UNITS {
            let s: Vec<char> = sym.chars().collect();
            if j + s.len() > chars.len() || chars[j..j + s.len()] != s[..] {
                continue;
            }
            let after = chars.get(j + s.len());
            let alpha = s.iter().all(|x| x.is_alphabetic());
            if alpha && after.is_some_and(|a| a.is_alphanumeric() || *a == '_') {
                continue;
            }
            let rest: String = chars[j + s.len()..].iter().collect::<String>().to_lowercase();
            if not_an_angle(sym, &rest) {
                continue;
            }
            let token: String = chars[start..j + s.len()].iter().collect();
            out.push((token, *kind));
            i = j + s.len();
            break;
        }
    }
    out
}

/// True when a degree symbol or word is a temperature (`°C`, `°F`, `°K`, `degrees Celsius`)
/// or part of "degrees of freedom", not an angle. `rest` is the lower-cased text after the unit.
fn not_an_angle(sym: &str, rest: &str) -> bool {
    let word_is = |text: &str, w: &str| {
        text.strip_prefix(w).is_some_and(|after| !after.chars().next().is_some_and(char::is_alphanumeric))
    };
    match sym {
        "°" => {
            let r = rest.strip_prefix(' ').unwrap_or(rest);
            ["c", "f", "k"].iter().any(|t| word_is(r, *t))
        }
        "deg" | "degree" | "degrees" => {
            let r = rest.trim_start();
            ["of freedom", "celsius", "fahrenheit", "kelvin", "c", "f"].iter().any(|t| word_is(r, *t))
        }
        _ => false,
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '@'
}

/// Identifier-like tokens of a line with their byte offsets (dots trimmed at both ends).
fn ident_tokens(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in line.char_indices().chain(std::iter::once((line.len(), ' '))) {
        match (start, is_ident_char(c)) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                let tok = &line[s..i];
                let lead = tok.len() - tok.trim_start_matches('.').len();
                let tok = tok.trim_matches('.');
                if !tok.is_empty() {
                    out.push((s + lead, tok));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// Lints one prose unit and appends findings. `first_line` skips front matter.
fn lint_prose(file: &str, text: &str, first_line: usize, ctx: &Ctx, out: &mut Vec<LintFinding>) {
    let lines = classify_lines(text, first_line);

    for &(n, line, fenced) in &lines {
        // REQ-001: numbers dressed as results, outside fenced examples.
        if !fenced {
            for (tok, kind) in result_like_numbers(line) {
                out.push(LintFinding::new(
                    file,
                    n,
                    Rule::ResultLikeNumber,
                    format!("`{tok}` reads like a result value ({kind} unit)"),
                ));
            }
        }
        // REQ-002: forbidden commands, fenced or not, unless after the human-request phrase.
        // Each occurrence of the phrase excuses only the first forbidden name after it.
        let lower = line.to_ascii_lowercase();
        let phrases: Vec<usize> = lower.match_indices(HUMAN_REQUEST_PHRASE).map(|(i, _)| i).collect();
        let mut next_phrase = 0;
        let mut excuse = false;
        for (at, tok) in ident_tokens(line) {
            while next_phrase < phrases.len() && phrases[next_phrase] < at {
                excuse = true;
                next_phrase += 1;
            }
            if let Some(fb) = ctx.forbidden.get(tok) {
                if excuse {
                    excuse = false;
                    continue;
                }
                out.push(LintFinding::new(
                    file,
                    n,
                    Rule::ForbiddenInstruction,
                    format!("names {} outside a request for a human", fb.label),
                ));
            }
        }
    }

    // REQ-005a: deny-list runs over prose words outside fences.
    // A fenced block ends a segment, so runs never join across it.
    let mut segments: Vec<Vec<(usize, String)>> = vec![Vec::new()];
    for &(n, l, fenced) in &lines {
        if fenced {
            if segments.last().is_some_and(|s| !s.is_empty()) {
                segments.push(Vec::new());
            }
            continue;
        }
        if let Some(seg) = segments.last_mut() {
            seg.extend(l.split_whitespace().map(|w| (n, norm_word(w))).filter(|(_, w)| !w.is_empty()));
        }
    }
    for (entry_line, stems) in &ctx.deny {
        let hit = segments.iter().find_map(|seg| first_deny_run(seg, stems).map(|start| seg[start].0));
        if let Some(line) = hit {
            out.push(LintFinding::new(
                file,
                line,
                Rule::PossibleStandardsText,
                format!("{MIN_DENY_RUN} or more consecutive words match deny-list entry on line {entry_line}"),
            ));
        }
    }

    // REQ-005b: quotations longer than the limit.
    for (n, count) in long_quotes(&lines) {
        out.push(LintFinding::new(
            file,
            n,
            Rule::PossibleStandardsText,
            format!("quotation of {count} words, at most {MAX_QUOTE_WORDS} allowed"),
        ));
    }
}

/// Start index of the first run of at least [`MIN_DENY_RUN`] words that match consecutive stems.
fn first_deny_run(words: &[(usize, String)], stems: &[String]) -> Option<usize> {
    let mut prev = vec![0usize; stems.len()];
    let mut cur = vec![0usize; stems.len()];
    for (i, (_, w)) in words.iter().enumerate() {
        for (j, s) in stems.iter().enumerate() {
            cur[j] = match (w.starts_with(s.as_str()), j) {
                (false, _) => 0,
                (true, 0) => 1,
                (true, _) => prev[j - 1] + 1,
            };
            if cur[j] >= MIN_DENY_RUN {
                return Some(i + 1 - cur[j]);
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    None
}

/// (start line, word count) of each quotation over the limit: blockquotes (consecutive `>`
/// lines) and text between `"` or `“`/`”` within a paragraph, outside fences.
fn long_quotes(lines: &[(usize, &str, bool)]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    // Blockquotes.
    let mut block: Option<(usize, usize)> = None;
    for &(n, line, fenced) in lines {
        let t = line.trim_start();
        if !fenced && t.starts_with('>') {
            let words = t.trim_start_matches('>').split_whitespace().count();
            block = Some(match block {
                None => (n, words),
                Some((s, w)) => (s, w + words),
            });
        } else if let Some((s, w)) = block.take() {
            if w > MAX_QUOTE_WORDS {
                out.push((s, w));
            }
        }
    }
    if let Some((s, w)) = block {
        if w > MAX_QUOTE_WORDS {
            out.push((s, w));
        }
    }
    // Inline quotations, reset at blank lines and fences.
    let mut open: Option<(usize, char, String)> = None;
    for &(n, line, fenced) in lines {
        if fenced || line.trim().is_empty() {
            open = None;
            continue;
        }
        for c in line.chars() {
            match open.as_mut() {
                None => {
                    if c == '"' {
                        open = Some((n, '"', String::new()));
                    } else if c == '“' {
                        open = Some((n, '”', String::new()));
                    }
                }
                Some((s, close, buf)) => {
                    if c == *close {
                        let w = buf.split_whitespace().count();
                        if w > MAX_QUOTE_WORDS {
                            out.push((*s, w));
                        }
                        open = None;
                    } else {
                        buf.push(c);
                    }
                }
            }
        }
        if let Some((_, _, buf)) = open.as_mut() {
            buf.push(' ');
        }
    }
    out
}

/// First body line of a Skill: after a `---` fenced front matter, or 1 when there is none.
fn body_start(text: &str) -> usize {
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    if lines.next() != Some("---") {
        return 1;
    }
    for (i, l) in lines.enumerate() {
        if l == "---" {
            return i + 3;
        }
    }
    1
}

// ------------------------------------------------------------------ driver

/// Collects `*.md` files under `p`. The path given on the command line may be a symlink; inside
/// a folder, symlinks are skipped (never followed), and nesting deeper than [`MAX_DIR_DEPTH`]
/// is an error, so a link loop cannot recurse without end.
fn collect_md(p: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    collect_md_at(p, 0, out)
}

fn collect_md_at(p: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if p.is_dir() {
        if depth > MAX_DIR_DEPTH {
            return Err(format!("folders nested deeper than {MAX_DIR_DEPTH}"));
        }
        let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
            .map_err(|e| format!("{}: {e}", label(p)))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        for e in entries {
            let Ok(meta) = std::fs::symlink_metadata(&e) else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() || e.extension().is_some_and(|x| x == "md") {
                collect_md_at(&e, depth + 1, out)?;
            }
        }
    } else {
        out.push(p.to_path_buf());
    }
    Ok(())
}

/// Runs the linter over every input. Never panics; unreadable inputs become `bad_input`.
pub fn lint(inputs: &StyleInputs) -> LintReport {
    let mut out = Vec::new();
    let mut units = 0usize;

    let manifest_label = label(&inputs.manifest);
    let manifest = match read_file(&inputs.manifest, fid_canon::MAX_INPUT_BYTES) {
        Ok(Some(b)) => fid_canon::parse_with_limits(&b, fid_canon::MAX_INPUT_BYTES, MAX_DEPTH)
            .map_err(|e| format!("not valid JSON: {e}"))
            .and_then(|v| parse_manifest(&v)),
        Ok(None) => Err("not found".to_string()),
        Err(e) => Err(e),
    };
    let manifest = match manifest {
        Ok(m) => m,
        Err(e) => {
            out.push(LintFinding::new(&manifest_label, 0, Rule::BadInput, e));
            Manifest { classes: BTreeMap::new(), guides: BTreeMap::new() }
        }
    };

    let deny = match read_text(&inputs.denylist, MAX_FILE_BYTES) {
        Ok(Some(t)) => parse_denylist(&t),
        Ok(None) => {
            out.push(LintFinding::new(&label(&inputs.denylist), 0, Rule::BadInput, "not found"));
            Vec::new()
        }
        Err(e) => {
            out.push(LintFinding::new(&label(&inputs.denylist), 0, Rule::BadInput, e));
            Vec::new()
        }
    };
    let ctx = Ctx { forbidden: forbidden_names(&manifest), deny };

    let allow = match &inputs.allow {
        None => Vec::new(),
        Some(p) => match read_text(p, MAX_FILE_BYTES)
            .and_then(|t| t.ok_or_else(|| "not found".to_string()))
            .and_then(|t| parse_allow(&t))
        {
            Ok(a) => a,
            Err(e) => {
                out.push(LintFinding::new(&label(p), 0, Rule::BadInput, e));
                Vec::new()
            }
        },
    };

    // Curated guides and recipes from the catalog.
    let catalog_label = label(&inputs.catalog);
    let rows = match read_file(&inputs.catalog, MAX_FILE_BYTES) {
        Ok(Some(b)) => parse_catalog(&b).unwrap_or_else(|f| {
            out.push(LintFinding::new(&catalog_label, 0, Rule::BadInput, f.detail));
            Vec::new()
        }),
        Ok(None) => {
            out.push(LintFinding::new(&catalog_label, 0, Rule::BadInput, "not found"));
            Vec::new()
        }
        Err(e) => {
            out.push(LintFinding::new(&catalog_label, 0, Rule::BadInput, e));
            Vec::new()
        }
    };
    let guide_ids: BTreeSet<&str> = rows.iter().map(|r| r.guide_id.as_str()).collect();
    let recipe_ids: BTreeSet<&str> = rows.iter().flat_map(|r| r.recipes.iter().map(String::as_str)).collect();

    for gid in &guide_ids {
        let Some(g) = manifest.guides.get(*gid) else { continue };
        let base = format!("{manifest_label}#task_guides/{gid}");
        lint_prose(&format!("{base}/title"), &g.title, 1, &ctx, &mut out);
        lint_prose(&format!("{base}/summary"), &g.summary, 1, &ctx, &mut out);
        units += 2;
        for (k, s) in g.steps.iter().enumerate() {
            lint_prose(&format!("{base}/steps/{k}/purpose"), &s.purpose, 1, &ctx, &mut out);
            units += 1;
        }
        // REQ-003: state-changing guides end with the brief and the promotion request.
        let changing = g.steps.iter().enumerate().find(|(_, s)| {
            matches!(manifest.classes.get(&(s.id.clone(), s.major)), Some(SideEffect::W1 | SideEffect::W2))
        });
        if let Some((k, s)) = changing {
            let n = g.steps.len();
            let is = |st: &Step, want: (&str, u32)| st.id == want.0 && st.major == want.1;
            let closes = n >= 2 && is(&g.steps[n - 2], BRIEF_STEP) && is(&g.steps[n - 1], PROMOTE_STEP);
            if !closes {
                out.push(LintFinding::new(
                    &base,
                    n,
                    Rule::MissingProposalStep,
                    format!(
                        "step {} ({}@{}) changes state, but the guide does not end with {}@{} then {}@{}",
                        k + 1,
                        s.id,
                        s.major,
                        BRIEF_STEP.0,
                        BRIEF_STEP.1,
                        PROMOTE_STEP.0,
                        PROMOTE_STEP.1
                    ),
                ));
            }
        }

        // Rendered Skill of the curated guide.
        let skill = inputs.skills.join(skill_dir_name(gid)).join("SKILL.md");
        let skill_label = label(&skill);
        match read_text(&skill, MAX_FILE_BYTES) {
            Ok(None) => {}
            Err(e) => out.push(LintFinding::new(&skill_label, 0, Rule::BadInput, e)),
            Ok(Some(text)) => {
                units += 1;
                let start = body_start(&text);
                lint_prose(&skill_label, &text, start, &ctx, &mut out);
                // REQ-004: word budget of the body.
                let words: usize = text.lines().skip(start - 1).map(|l| l.split_whitespace().count()).sum();
                if words > WORD_BUDGET {
                    out.push(LintFinding::new(
                        &skill_label,
                        start,
                        Rule::OverBudget,
                        format!("Skill body has {words} words, budget is {WORD_BUDGET}"),
                    ));
                }
            }
        }
    }

    for rid in &recipe_ids {
        let path = inputs.recipes.join(format!("{rid}.json"));
        let rl = label(&path);
        let v = match read_file(&path, MAX_FILE_BYTES) {
            Ok(None) => continue,
            Ok(Some(b)) => fid_canon::parse_with_limits(&b, MAX_FILE_BYTES, MAX_DEPTH),
            Err(e) => {
                out.push(LintFinding::new(&rl, 0, Rule::BadInput, e));
                continue;
            }
        };
        let m = match v {
            Ok(CValue::Obj(m)) => m,
            Ok(_) => {
                out.push(LintFinding::new(&rl, 0, Rule::BadInput, "not a JSON object"));
                continue;
            }
            Err(e) => {
                out.push(LintFinding::new(&rl, 0, Rule::BadInput, format!("not valid JSON: {e}")));
                continue;
            }
        };
        for key in ["title", "prompt"] {
            if let Some(t) = get_str(&m, key) {
                lint_prose(&format!("{rl}#{key}"), t, 1, &ctx, &mut out);
                units += 1;
            }
        }
        if let Some(CValue::Arr(cs)) = m.get("commentary") {
            for (k, c) in cs.iter().enumerate() {
                if let CValue::Str(t) = c {
                    lint_prose(&format!("{rl}#commentary/{k}"), t, 1, &ctx, &mut out);
                    units += 1;
                }
            }
        }
    }

    // Extra Markdown.
    let mut files = Vec::new();
    for p in &inputs.prose {
        if let Err(e) = collect_md(p, &mut files) {
            out.push(LintFinding::new(&label(p), 0, Rule::BadInput, e));
        }
    }
    for f in files {
        let fl = label(&f);
        match read_text(&f, MAX_FILE_BYTES) {
            Ok(Some(t)) => {
                lint_prose(&fl, &t, 1, &ctx, &mut out);
                units += 1;
            }
            Ok(None) => out.push(LintFinding::new(&fl, 0, Rule::BadInput, "not found")),
            Err(e) => out.push(LintFinding::new(&fl, 0, Rule::BadInput, e)),
        }
    }

    out.retain(|f| f.rule == Rule::BadInput || !allowed(f, &allow));
    out.sort();
    out.dedup();
    LintReport { findings: out, units }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        let toks = |s: &str| result_like_numbers(s).into_iter().map(|(t, _)| t).collect::<Vec<_>>();
        assert_eq!(toks("the bore measures 12.004 mm here"), ["12.004 mm"]);
        assert_eq!(toks("tilt 0.5° and 3 % and 5um"), ["0.5°", "3 %", "5um"]);
        assert_eq!(toks("step 2 in the guide, v2.1, sha256, W2, 3 mmol, 4 models"), Vec::<String>::new());
        assert_eq!(toks("12,5 mm"), ["12,5 mm"]);
        assert_eq!(toks("10 m."), ["10 m"]);
        // Temperatures and degrees of freedom are not angles.
        let none = Vec::<String>::new();
        assert_eq!(toks("scale 0.01 °C as K, 20 °C, 68°F, 300 °K"), none);
        assert_eq!(toks("lock 3 degrees of freedom, keep 20 degrees Celsius, 5 deg C"), none);
        assert_eq!(toks("turn 90 degrees, 45° tilt, 30 °Cx"), ["90 degrees", "45°", "30 °"]);
    }

    fn ctx_with(names: &[&str]) -> Ctx {
        let forbidden =
            names.iter().map(|n| (n.to_string(), Forbidden { label: format!("{n} (W3, tool {n})") })).collect();
        Ctx { forbidden, deny: Vec::new() }
    }

    #[test]
    fn human_request_excuses_only_the_next_name() {
        let ctx = ctx_with(&["a_x", "b_y"]);
        let run = |text: &str| {
            let mut out = Vec::new();
            lint_prose("f.md", text, 1, &ctx, &mut out);
            out.len()
        };
        assert_eq!(run("File a request for a human to run `a_x`."), 0);
        assert_eq!(run("File a request for a human to run `a_x`, then call `b_y` yourself."), 1);
        assert_eq!(run("Call `b_y`, then file a request for a human to run `a_x`."), 1);
        assert_eq!(run("A request for a human for `a_x`; another request for a human for `b_y`."), 0);
    }

    #[test]
    fn deny_runs_stop_at_fences() {
        let stems: Vec<String> = "a b c d e f g h i j k l m".split(' ').map(String::from).collect();
        let ctx = Ctx { forbidden: BTreeMap::new(), deny: vec![(1, stems)] };
        let run = |text: &str| {
            let mut out = Vec::new();
            lint_prose("f.md", text, 1, &ctx, &mut out);
            out.len()
        };
        assert_eq!(run("ax bx cx dx ex fx\ngx hx ix jx kx lx"), 1);
        assert_eq!(run("ax bx cx dx ex fx\n```\ncode\n```\ngx hx ix jx kx lx"), 0);
    }

    #[test]
    fn fences() {
        let l = classify_lines("a\n```sh\nx 1 mm\n```\nb\n~~~~\n```\n~~~~\nc", 1);
        let fenced: Vec<bool> = l.iter().map(|x| x.2).collect();
        assert_eq!(fenced, [false, true, true, true, false, true, true, true, false]);
    }

    #[test]
    fn tokens() {
        let t: Vec<&str> = ident_tokens("call `a.b_c@1`. Then x_y.").into_iter().map(|x| x.1).collect();
        assert_eq!(t, ["call", "a.b_c@1", "Then", "x_y"]);
        assert_eq!(tool_name("a.b_c", 1), "a_b_c");
        assert_eq!(tool_name("a.b_c", 2), "a_b_c_v2");
    }

    #[test]
    fn front_matter() {
        assert_eq!(body_start("---\na: b\n---\nbody"), 4);
        assert_eq!(body_start("body"), 1);
        assert_eq!(body_start("---\nunclosed"), 1);
    }

    #[test]
    fn quotes() {
        let q = |s: &str| long_quotes(&classify_lines(s, 1));
        let long = (0..26).map(|_| "w").collect::<Vec<_>>().join(" ");
        let ok = (0..25).map(|_| "w").collect::<Vec<_>>().join(" ");
        assert_eq!(q(&format!("x \"{long}\" y")), [(1, 26)]);
        assert!(q(&format!("x \"{ok}\" y")).is_empty());
        assert_eq!(q(&format!("> {ok}\n> one")), [(1, 26)]);
        assert!(q(&format!("x \"{long}\n\nnever closed\"")).is_empty());
    }

    #[test]
    fn deny_runs() {
        let stems: Vec<String> = "a b c d e f g h i j k l m".split(' ').map(String::from).collect();
        let words = |s: &str| s.split(' ').map(|w| (1, w.to_string())).collect::<Vec<_>>();
        assert_eq!(first_deny_run(&words("zz ax bx cx dx ex fx gx hx ix jx kx lx"), &stems), Some(1));
        assert_eq!(first_deny_run(&words("ax bx cx dx ex fx gx hx ix jx kx zz"), &stems), None);
    }
}
