//! `fid-design lint-literals --root <dir> [--rules <toml>] [--package <dir>] [--allowlist <file>]
//! [--json]` (spec node `UX.SYSTEM.LINT`, REQ-UX.SYSTEM.LINT-001 .. -005). Tooling, nearest
//! class R0; D0 (sorted walk, `BTreeMap`/`BTreeSet` only, sorted findings, linear-time regex,
//! no clock, no network).
//!
//! # Rules (`lint-rules@1` TOML)
//!
//! ```toml
//! format = "lint-rules@1"
//!
//! [[rule]]
//! id = "LIT-COLOR-HEX"
//! kind = "color"
//! globs = ["**/*.css", "**/*.svg"]
//! regex = '(?:^|[^0-9A-Za-z_])(#(?:[0-9A-Fa-f]{8}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{3}))\b'
//! value_group = 1
//! ```
//!
//! Default rules ship for Rust, TOML, JSON, CSS and SVG.
//!
//! # Every literal in a value
//!
//! For `font_size`, `spacing` and `duration` rules the value group may hold a whole declaration
//! value (`margin: 4px 8px`, `transition: opacity 120ms ease, transform 0.2s 50ms`): every
//! number with a unit of the rule's kind in it (`px`/`pt`, or `ms`/`s`) is its own finding, at its
//! own column. A unitless number (`padding: 0 8px` reports only `8px`) and a token name
//! (`duration = "motion.fast"`) are not literals. A value group with no such unit is reported
//! whole when it is one literal by itself (`from_millis(120)`, or `12em` from a custom rule).
//!
//! # No silent pass
//!
//! A root in which no file matches any rule's globs is the finding `LINT-NO-FILES` and exits 2:
//! nothing was checked.
//!
//! # Escape hatches (REQ-002)
//!
//! - Per-line marker `design-allow: <reason>` with non-empty reason suppresses findings on that
//!   line. Empty reason reports `LIT-ALLOW-NOREASON` and does not suppress findings.
//! - Allowlist file (`--allowlist <file>`): one `<path-or-glob>: <reason>` per line (`#` comments
//!   and blank lines skipped). Missing reason reports `LIT-ALLOW-NOREASON`.
//!
//! # Token suggestions (`--package <dir>`, REQ-003)
//!
//! Reads the design-system package with `fid_design_pkg::Package::load`, resolves aliases and
//! deprecations, normalises colours to lowercase `#rrggbb[aa]`, dimensions to `px` and durations
//! to `ms`, and attaches every matching token name in sorted order.
//!
//! Exit codes: 0 clean, 1 findings, 2 unreadable input, bad usage, or no file matched any rule
//! (`LINT-NO-FILES`).

use crate::finding::read_text;
use crate::review::guard::{defects, glob_defect, glob_match, path_defect};
use crate::review::walk;
use fid_canon::CValue;
use fid_design_pkg::Package;
use regex::{Regex, RegexBuilder};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-lint-literals@1";
/// Rule file format ID.
pub const RULES_FORMAT: &str = "lint-rules@1";

/// Diagnostic codes (UX.SYSTEM.LINT Design).
pub const ALLOW_NOREASON: &str = "LIT-ALLOW-NOREASON";
pub const RULES_SCHEMA: &str = "LINT-RULES-SCHEMA";
pub const REGEX_ERR: &str = "LINT-REGEX";
/// No file under the root matches any rule's globs, so nothing was checked (exit 2).
pub const NO_FILES: &str = "LINT-NO-FILES";

/// Largest source file scanned (4 MiB, UX.SYSTEM.LINT Build prompt).
pub const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
/// Largest rule TOML or allowlist file read (1 MiB).
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
/// Maximum compiled regex size and DFA cache size (2 MiB, linear-time bounded).
pub const MAX_REGEX_BYTES: usize = 2 * 1024 * 1024;
/// Most rules in one `lint-rules@1` file.
pub const MAX_RULES: usize = 256;
/// Most allowlist entries in one file.
pub const MAX_ALLOWLIST: usize = 10_000;

/// Built-in default rule sets for Rust, TOML, JSON, CSS and SVG (REQ-UX.SYSTEM.LINT-004).
pub const DEFAULT_RULES_TOML: &str = r#"format = "lint-rules@1"

[[rule]]
id = "LIT-COLOR-RUST"
kind = "color"
globs = ["**/*.rs"]
regex = '\b(from_rgba?(?:_unmultiplied|_premultiplied|_srgba)?\s*\(\s*(?:0[xX][0-9A-Fa-f_]+|[0-9][0-9_.]*)(?:u8|f32|f64)?(?:\s*,\s*(?:0[xX][0-9A-Fa-f_]+|[0-9][0-9_.]*)(?:u8|f32|f64)?){2,3}\s*,?\s*\))'
value_group = 1

[[rule]]
id = "LIT-COLOR-HEX"
kind = "color"
globs = ["**/*.rs", "**/*.toml", "**/*.json", "**/*.css", "**/*.svg"]
regex = '(?:^|[^0-9A-Za-z_])(#(?:[0-9A-Fa-f]{8}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{3}))\b'
value_group = 1

[[rule]]
id = "LIT-COLOR-FUNC"
kind = "color"
globs = ["**/*.css", "**/*.svg", "**/*.toml", "**/*.json"]
regex = '\b((?:rgba?|hsla?)\s*\(\s*[^)]+\))'
value_group = 1

[[rule]]
id = "LIT-FONT-SIZE"
kind = "font_size"
globs = ["**/*.css", "**/*.svg", "**/*.toml", "**/*.json"]
regex = '(?i)\b(?:font[-_]?size|fontSize)\s*[:=]\s*"?([0-9]+(?:\.[0-9]+)?(?:px|pt))"?'
value_group = 1

[[rule]]
id = "LIT-FONT-RUST"
kind = "font_size"
globs = ["**/*.rs"]
regex = '\bfont_size\s*[:=]\s*"?([0-9]+(?:\.[0-9]+)?(?:px|pt))"?'
value_group = 1

[[rule]]
id = "LIT-SPACE-STYLE"
kind = "spacing"
globs = ["**/*.css", "**/*.toml", "**/*.json"]
regex = '(?i)\b(?:padding(?:-(?:top|right|bottom|left))?|margin(?:-(?:top|right|bottom|left))?|(?:row-|column-)?gap|spacing)"?\s*[:=]\s*"?([^;{}"\x27\n]+)'
value_group = 1

[[rule]]
id = "LIT-DURATION-STYLE"
kind = "duration"
globs = ["**/*.css", "**/*.toml", "**/*.json", "**/*.svg"]
regex = '(?i)\b(?:transition(?:-duration|-delay)?|animation(?:-duration|-delay)?|duration)"?\s*[:=]\s*"?([^;{}"\x27\n]+)'
value_group = 1

[[rule]]
id = "LIT-DURATION-RUST"
kind = "duration"
globs = ["**/*.rs"]
regex = '\bDuration::(from_(?:millis|secs|secs_f32|secs_f64)\s*\(\s*[0-9]+(?:\.[0-9]+)?\s*\))'
value_group = 1
"#;

/// Kind of literal detected by a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuleKind {
    Color,
    FontSize,
    Spacing,
    Duration,
}

impl RuleKind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "color" => Some(Self::Color),
            "font_size" => Some(Self::FontSize),
            "spacing" => Some(Self::Spacing),
            "duration" => Some(Self::Duration),
            _ => None,
        }
    }

    fn required_prefix(self) -> &'static str {
        match self {
            Self::Color => "LIT-COLOR-",
            Self::FontSize => "LIT-FONT-",
            Self::Spacing => "LIT-SPACE-",
            Self::Duration => "LIT-DURATION-",
        }
    }

    fn bucket(self) -> Bucket {
        match self {
            Self::Color => Bucket::Color,
            Self::FontSize | Self::Spacing => Bucket::Dimension,
            Self::Duration => Bucket::Duration,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Bucket {
    Color,
    Dimension,
    Duration,
}

/// One compiled `lint-rules@1` rule.
#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub kind: RuleKind,
    pub globs: Vec<String>,
    pub regex: Regex,
    pub value_group: usize,
}

impl Rule {
    fn matches_path(&self, rel: &str) -> bool {
        self.globs.iter().any(|g| glob_match(g, rel))
    }
}

/// One finding emitted by `fid-design lint-literals`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LintFinding {
    pub file: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub code: String,
    pub literal: Option<String>,
    pub suggestions: Vec<String>,
    pub detail: String,
}

impl LintFinding {
    fn schema(code: &str, file: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            line: None,
            column: None,
            code: code.to_string(),
            literal: None,
            suggestions: Vec::new(),
            detail: detail.into(),
        }
    }

    fn allow_noreason(file: impl Into<String>, line: usize, column: usize, detail: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            line: Some(line),
            column: Some(column),
            code: ALLOW_NOREASON.to_string(),
            literal: None,
            suggestions: Vec::new(),
            detail: detail.into(),
        }
    }

    fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("code".to_string(), CValue::Str(self.code.clone()));
        m.insert("column".to_string(), self.column.map_or(CValue::Null, |c| CValue::Num(c as f64)));
        m.insert("detail".to_string(), CValue::Str(self.detail.clone()));
        m.insert("file".to_string(), CValue::Str(self.file.clone()));
        m.insert("line".to_string(), self.line.map_or(CValue::Null, |l| CValue::Num(l as f64)));
        m.insert("literal".to_string(), self.literal.clone().map_or(CValue::Null, CValue::Str));
        m.insert("suggestions".to_string(), CValue::Arr(self.suggestions.iter().cloned().map(CValue::Str).collect()));
        CValue::Obj(m)
    }
}

impl fmt::Display for LintFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "{} {}:{}:{}: {}", self.code, self.file, l, c, self.detail)?,
            (Some(l), None) => write!(f, "{} {}:{}: {}", self.code, self.file, l, self.detail)?,
            _ => write!(f, "{} {}: {}", self.code, self.file, self.detail)?,
        }
        if !self.suggestions.is_empty() {
            write!(f, " (suggest: {})", self.suggestions.join(", "))?;
        }
        Ok(())
    }
}

/// Options for [`check`].
#[derive(Debug, Clone)]
pub struct Options<'a> {
    pub root: &'a Path,
    pub rules: Option<&'a Path>,
    pub package: Option<&'a Path>,
    pub allowlist: Option<&'a Path>,
}

/// Result of `fid-design lint-literals`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub files_scanned: usize,
    pub rules: usize,
    pub findings: Vec<LintFinding>,
}

fn valid_rule_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2
        && b.len() <= 64
        && b[0].is_ascii_uppercase()
        && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'-')
}

/// Parses a `lint-rules@1` TOML rule set. Schema and regex errors are recorded as findings.
pub fn parse_rules(text: &str, subject: &str, f: &mut Vec<LintFinding>) -> Vec<Rule> {
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            f.push(LintFinding::schema(RULES_SCHEMA, subject, format!("not valid TOML: {}", e.message())));
            return Vec::new();
        }
    };
    for k in table.keys() {
        if k != "format" && k != "rule" {
            f.push(LintFinding::schema(RULES_SCHEMA, subject, format!("unknown top-level key `{k}`")));
        }
    }
    match table.get("format") {
        Some(toml::Value::String(s)) if s == RULES_FORMAT => {}
        _ => {
            f.push(LintFinding::schema(RULES_SCHEMA, subject, format!("`format` must be \"{RULES_FORMAT}\"")));
            return Vec::new();
        }
    }
    let Some(toml::Value::Array(arr)) = table.get("rule") else {
        f.push(LintFinding::schema(RULES_SCHEMA, subject, "`rule` must be a non-empty array of [[rule]] tables"));
        return Vec::new();
    };
    if arr.is_empty() || arr.len() > MAX_RULES {
        f.push(LintFinding::schema(RULES_SCHEMA, subject, format!("`rule` must hold 1 to {MAX_RULES} entries")));
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut seen_ids = BTreeSet::new();
    for (i, item) in arr.iter().enumerate() {
        let fallback = format!("{subject}:rule[{i}]");
        let toml::Value::Table(t) = item else {
            f.push(LintFinding::schema(RULES_SCHEMA, fallback, "each [[rule]] entry must be a table"));
            continue;
        };
        let mut ok = true;
        for k in t.keys() {
            if !matches!(k.as_str(), "id" | "kind" | "globs" | "regex" | "value_group") {
                f.push(LintFinding::schema(RULES_SCHEMA, &fallback, format!("unknown key `{k}`")));
                ok = false;
            }
        }
        let id = match t.get("id") {
            Some(toml::Value::String(s)) if valid_rule_id(s) && defects(s).is_empty() => s.clone(),
            _ => {
                f.push(LintFinding::schema(
                    RULES_SCHEMA,
                    &fallback,
                    "`id` must be an uppercase rule ID (e.g. `LIT-COLOR-HEX`)",
                ));
                ok = false;
                String::new()
            }
        };
        let subj = if id.is_empty() { fallback.as_str() } else { id.as_str() };
        let kind = match t.get("kind") {
            Some(toml::Value::String(s)) => match RuleKind::parse(s) {
                Some(k) => Some(k),
                None => {
                    f.push(LintFinding::schema(
                        RULES_SCHEMA,
                        subj,
                        "`kind` must be `color`, `font_size`, `spacing` or `duration`",
                    ));
                    ok = false;
                    None
                }
            },
            _ => {
                f.push(LintFinding::schema(RULES_SCHEMA, subj, "`kind` must be a string"));
                ok = false;
                None
            }
        };
        if let (false, Some(k)) = (id.is_empty(), kind) {
            if !id.starts_with(k.required_prefix()) {
                f.push(LintFinding::schema(
                    RULES_SCHEMA,
                    subj,
                    format!("rule ID `{id}` must start with `{}` for kind", k.required_prefix()),
                ));
                ok = false;
            }
        }
        let mut globs = Vec::new();
        match t.get("globs") {
            Some(toml::Value::Array(gs)) if !gs.is_empty() && gs.len() <= 64 => {
                for g in gs {
                    match g {
                        toml::Value::String(gs_str) if glob_defect(gs_str).is_none() => globs.push(gs_str.clone()),
                        _ => {
                            f.push(LintFinding::schema(RULES_SCHEMA, subj, "`globs` entries must be valid globs"));
                            ok = false;
                        }
                    }
                }
            }
            _ => {
                f.push(LintFinding::schema(RULES_SCHEMA, subj, "`globs` must be a non-empty array of globs"));
                ok = false;
            }
        }
        let value_group = match t.get("value_group") {
            Some(toml::Value::Integer(n)) if *n >= 0 && *n <= 64 => *n as usize,
            _ => {
                f.push(LintFinding::schema(RULES_SCHEMA, subj, "`value_group` must be a non-negative integer"));
                ok = false;
                0
            }
        };
        let regex = match t.get("regex") {
            Some(toml::Value::String(pat)) if !pat.is_empty() && pat.len() <= 4096 => {
                match RegexBuilder::new(pat).size_limit(MAX_REGEX_BYTES).dfa_size_limit(MAX_REGEX_BYTES).build() {
                    Ok(re) => Some(re),
                    Err(e) => {
                        f.push(LintFinding::schema(REGEX_ERR, subj, format!("invalid or oversized regex: {e}")));
                        ok = false;
                        None
                    }
                }
            }
            _ => {
                f.push(LintFinding::schema(RULES_SCHEMA, subj, "`regex` must be a non-empty pattern string"));
                ok = false;
                None
            }
        };
        if let Some(re) = &regex {
            if value_group >= re.captures_len() {
                f.push(LintFinding::schema(
                    RULES_SCHEMA,
                    subj,
                    format!("`value_group` {value_group} exceeds capture count {}", re.captures_len()),
                ));
                ok = false;
            }
        }
        if !id.is_empty() && !seen_ids.insert(id.clone()) {
            f.push(LintFinding::schema(RULES_SCHEMA, &id, "duplicate rule id"));
            ok = false;
        }
        if ok {
            if let (Some(kind), Some(regex)) = (kind, regex) {
                out.push(Rule { id, kind, globs, regex, value_group });
            }
        }
    }
    out
}

/// Parses an allowlist file (`<path-or-glob>: <reason>` per line; `#` comments and blank lines
/// skipped).
pub fn parse_allowlist(text: &str, subject: &str, f: &mut Vec<LintFinding>) -> Vec<String> {
    let mut globs = Vec::new();
    let mut non_empty = 0usize;
    for (idx, raw) in text.split('\n').enumerate() {
        let line_no = idx + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        non_empty += 1;
        if non_empty > MAX_ALLOWLIST {
            f.push(LintFinding::schema(RULES_SCHEMA, subject, format!("more than {MAX_ALLOWLIST} entries")));
            break;
        }
        let Some((pat, reason)) = line.split_once(':') else {
            f.push(LintFinding::allow_noreason(subject, line_no, 1, "allowlist entry is missing `: <reason>`"));
            continue;
        };
        let pat = pat.trim();
        let reason = reason.trim();
        if let Some(d) = glob_defect(pat) {
            f.push(LintFinding::schema(
                RULES_SCHEMA,
                format!("{subject}:{line_no}"),
                format!("invalid path/glob: {d}"),
            ));
            continue;
        }
        if reason.is_empty() {
            f.push(LintFinding::allow_noreason(subject, line_no, 1, "allowlist entry has an empty reason"));
            continue;
        }
        if !defects(reason).is_empty() {
            f.push(LintFinding::allow_noreason(subject, line_no, 1, "allowlist reason refused (untrusted text)"));
            continue;
        }
        globs.push(pat.to_string());
    }
    if non_empty == 0 {
        f.push(LintFinding::schema(RULES_SCHEMA, subject, "allowlist file holds no entries"));
    }
    globs
}

fn format_number_unit(v: f64, unit: &str) -> Option<String> {
    if !v.is_finite() {
        return None;
    }
    // Round to 4 decimal places for deterministic comparison across pt->px and s->ms.
    let rounded = (v * 10_000.0).round() / 10_000.0;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    let s = format!("{rounded:.4}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    Some(format!("{trimmed}{unit}"))
}

fn parse_u8_lit(s: &str) -> Option<u8> {
    let t = s.trim().trim_end_matches("u8").trim_end_matches('_');
    let clean: String = t.chars().filter(|c| *c != '_').collect();
    if let Some(hex) = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16).ok()
    } else {
        clean.parse::<u8>().ok()
    }
}

fn parse_alpha_byte(s: &str) -> Option<u8> {
    let t = s.trim();
    if let Some(pct) = t.strip_suffix('%') {
        let p: f64 = pct.trim().parse().ok()?;
        if !(0.0..=100.0).contains(&p) {
            return None;
        }
        return Some(((p / 100.0) * 255.0).round() as u8);
    }
    let a: f64 = t.parse().ok()?;
    if !(0.0..=1.0).contains(&a) {
        return None;
    }
    Some((a * 255.0).round() as u8)
}

fn hsl_to_rgb(h_deg: f64, s_pct: f64, l_pct: f64) -> Option<(u8, u8, u8)> {
    if !h_deg.is_finite() || !(0.0..=100.0).contains(&s_pct) || !(0.0..=100.0).contains(&l_pct) {
        return None;
    }
    let h = ((h_deg % 360.0) + 360.0) % 360.0;
    let s = s_pct / 100.0;
    let l = l_pct / 100.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let to_u8 = |v: f64| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Some((to_u8(r1), to_u8(g1), to_u8(b1)))
}

/// Normalises a colour literal to lowercase `#rrggbb` or `#rrggbbaa`.
pub fn normalise_color(raw: &str) -> Option<String> {
    let t = raw.trim();
    if let Some(hex) = t.strip_prefix('#') {
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let low = hex.to_ascii_lowercase();
        return match low.len() {
            3 => {
                let mut out = String::with_capacity(7);
                out.push('#');
                for ch in low.chars() {
                    out.push(ch);
                    out.push(ch);
                }
                Some(out)
            }
            6 | 8 => Some(format!("#{low}")),
            _ => None,
        };
    }
    let (fname, rest) = t.split_once('(')?;
    let args_str = rest.strip_suffix(')')?;
    let fname_low = fname.trim().to_ascii_lowercase();

    if fname_low.starts_with("from_rgb") {
        let parts: Vec<&str> = args_str.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        return match parts.as_slice() {
            [r, g, b] => Some(format!("#{:02x}{:02x}{:02x}", parse_u8_lit(r)?, parse_u8_lit(g)?, parse_u8_lit(b)?)),
            [r, g, b, a] => Some(format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                parse_u8_lit(r)?,
                parse_u8_lit(g)?,
                parse_u8_lit(b)?,
                parse_u8_lit(a)?
            )),
            _ => None,
        };
    }

    if fname_low == "rgb" || fname_low == "rgba" {
        let parts: Vec<&str> = args_str.split([',', '/', ' ']).map(str::trim).filter(|s| !s.is_empty()).collect();
        return match parts.as_slice() {
            [r, g, b] => Some(format!("#{:02x}{:02x}{:02x}", parse_u8_lit(r)?, parse_u8_lit(g)?, parse_u8_lit(b)?)),
            [r, g, b, a] => Some(format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                parse_u8_lit(r)?,
                parse_u8_lit(g)?,
                parse_u8_lit(b)?,
                parse_alpha_byte(a)?
            )),
            _ => None,
        };
    }

    if fname_low == "hsl" || fname_low == "hsla" {
        let parts: Vec<&str> = args_str.split([',', '/', ' ']).map(str::trim).filter(|s| !s.is_empty()).collect();
        let parse_deg = |s: &str| s.trim_end_matches("deg").parse::<f64>().ok();
        let parse_pct = |s: &str| s.strip_suffix('%')?.parse::<f64>().ok();
        return match parts.as_slice() {
            [h, s, l] => {
                let (r, g, b) = hsl_to_rgb(parse_deg(h)?, parse_pct(s)?, parse_pct(l)?)?;
                Some(format!("#{r:02x}{g:02x}{b:02x}"))
            }
            [h, s, l, a] => {
                let (r, g, b) = hsl_to_rgb(parse_deg(h)?, parse_pct(s)?, parse_pct(l)?)?;
                Some(format!("#{r:02x}{g:02x}{b:02x}{:02x}", parse_alpha_byte(a)?))
            }
            _ => None,
        };
    }
    None
}

/// Normalises a dimension literal (`px` or `pt`) to canonical `px` (`1 pt = 4/3 px`).
pub fn normalise_dimension(raw: &str) -> Option<String> {
    let t = raw.trim().to_ascii_lowercase();
    if let Some(n) = t.strip_suffix("px") {
        let v: f64 = n.trim().parse().ok()?;
        format_number_unit(v, "px")
    } else if let Some(n) = t.strip_suffix("pt") {
        let v: f64 = n.trim().parse().ok()?;
        format_number_unit(v * (96.0 / 72.0), "px")
    } else {
        None
    }
}

/// Normalises a duration literal (`ms`, `s`, or Rust `from_millis`/`from_secs*`) to canonical `ms`.
pub fn normalise_duration(raw: &str) -> Option<String> {
    let t = raw.trim();
    if let Some((fname, rest)) = t.split_once('(') {
        let arg = rest.strip_suffix(')')?.trim();
        let v: f64 = arg.parse().ok()?;
        return match fname.trim() {
            "from_millis" => format_number_unit(v, "ms"),
            "from_secs" | "from_secs_f32" | "from_secs_f64" => format_number_unit(v * 1000.0, "ms"),
            _ => None,
        };
    }
    let low = t.to_ascii_lowercase();
    if let Some(n) = low.strip_suffix("ms") {
        let v: f64 = n.trim().parse().ok()?;
        format_number_unit(v, "ms")
    } else if let Some(n) = low.strip_suffix('s') {
        let v: f64 = n.trim().parse().ok()?;
        format_number_unit(v * 1000.0, "ms")
    } else {
        None
    }
}

/// Normalises a literal according to its rule kind.
pub fn normalise_literal(kind: RuleKind, raw: &str) -> Option<String> {
    match kind {
        RuleKind::Color => normalise_color(raw),
        RuleKind::FontSize | RuleKind::Spacing => normalise_dimension(raw),
        RuleKind::Duration => normalise_duration(raw),
    }
}

type Suggestions = BTreeMap<(Bucket, String), BTreeSet<String>>;

fn build_suggestions(pkg: &Package) -> Suggestions {
    let mut map: Suggestions = BTreeMap::new();
    for name in pkg.tokens.keys() {
        let Ok(res) = pkg.resolve(name) else {
            continue;
        };
        let ty = res.ty.as_deref();
        match (ty, &res.value) {
            (Some("color"), CValue::Str(s)) => {
                if let Some(norm) = normalise_color(s) {
                    map.entry((Bucket::Color, norm)).or_default().insert(name.clone());
                }
            }
            (Some("dimension"), CValue::Str(s)) => {
                if let Some(norm) = normalise_dimension(s) {
                    map.entry((Bucket::Dimension, norm)).or_default().insert(name.clone());
                }
            }
            (Some("duration"), CValue::Str(s)) => {
                if let Some(norm) = normalise_duration(s) {
                    map.entry((Bucket::Duration, norm)).or_default().insert(name.clone());
                }
            }
            (Some("typography"), CValue::Obj(obj)) => {
                if let Some(CValue::Str(sz)) = obj.get("fontSize") {
                    if let Some(norm) = normalise_dimension(sz) {
                        map.entry((Bucket::Dimension, norm)).or_default().insert(name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    map
}

enum AllowMarker {
    None,
    Valid,
    Empty(usize),
    Refused(usize),
}

fn check_allow_marker(line: &str) -> AllowMarker {
    let Some(byte_idx) = line.find("design-allow:") else {
        return AllowMarker::None;
    };
    let col = line.get(..byte_idx).map_or(1, |pre| pre.chars().count() + 1);
    let mut after = line.get(byte_idx + "design-allow:".len()..).unwrap_or("").trim();
    loop {
        if let Some(stripped) = after.strip_suffix("*/") {
            after = stripped.trim();
        } else if let Some(stripped) = after.strip_suffix("-->") {
            after = stripped.trim();
        } else {
            break;
        }
    }
    if after.is_empty() {
        AllowMarker::Empty(col)
    } else if !defects(after).is_empty() {
        AllowMarker::Refused(col)
    } else {
        AllowMarker::Valid
    }
}

/// Units that make a number a literal of `kind` (longest first), or none for colours.
fn kind_units(kind: RuleKind) -> &'static [&'static str] {
    match kind {
        RuleKind::Color => &[],
        RuleKind::FontSize | RuleKind::Spacing => &["px", "pt"],
        RuleKind::Duration => &["ms", "s"],
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Byte ranges of every `<number><unit>` word in `value` whose unit belongs to `kind`
/// (`12px`, `.5s`, `0.12s`, `120ms`; ASCII case-insensitive units). A number glued to a word or
/// a dot before it (`fade2s`, `space.2`) or a word after it (`8pxx`) is not one.
fn unit_literals(kind: RuleKind, value: &str) -> Vec<(usize, usize)> {
    let units = kind_units(kind);
    let b = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let starts_number = b[i].is_ascii_digit() || (b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit));
        let glued = i > 0 && (is_word_byte(b[i - 1]) || b[i - 1] == b'.');
        if !starts_number || glued {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j < b.len() && b[j] == b'.' && b.get(j + 1).is_some_and(u8::is_ascii_digit) {
            j += 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
        }
        let unit = units.iter().find(|u| {
            b.get(j..j + u.len()).is_some_and(|w| w.eq_ignore_ascii_case(u.as_bytes()))
                && !b.get(j + u.len()).copied().is_some_and(is_word_byte)
        });
        if let Some(u) = unit {
            out.push((i, j + u.len()));
            i = j + u.len();
        } else {
            i = j.max(i + 1);
        }
    }
    out
}

/// The literals a rule match reports: for colours the value group itself; for the other kinds
/// every unit literal inside it ([`unit_literals`]), or the whole group when it is one literal
/// by itself (a Rust `Duration::from_millis(120)` argument list, or a single number-and-unit word
/// of another unit). Returns `(byte offset in line, literal)`.
fn literals_in<'a>(kind: RuleKind, m: &regex::Match<'a>) -> Vec<(usize, &'a str)> {
    let text = m.as_str();
    if kind == RuleKind::Color {
        return vec![(m.start(), text)];
    }
    let found: Vec<(usize, &'a str)> =
        unit_literals(kind, text).into_iter().filter_map(|(a, z)| text.get(a..z).map(|l| (m.start() + a, l))).collect();
    if !found.is_empty() {
        return found;
    }
    let t = text.trim();
    let single_word = !t.is_empty() && !t.contains(|c: char| c.is_whitespace() || c == ',');
    let number_first = t.starts_with(|c: char| c.is_ascii_digit() || c == '.');
    let unit_last = t.ends_with(|c: char| c.is_ascii_alphabetic());
    // The group is itself one recognised literal (`from_millis(120)`), or one number-and-unit
    // word of a unit the kind does not list (`12em`, from a custom rule).
    if normalise_literal(kind, t).is_some() || (single_word && number_first && unit_last) {
        let lead = text.len() - text.trim_start().len();
        vec![(m.start() + lead, t)]
    } else {
        Vec::new()
    }
}

fn scan_text(rel: &str, text: &str, rules: &[&Rule], suggestions: &Suggestions, f: &mut Vec<LintFinding>) {
    for (idx, raw) in text.split('\n').enumerate() {
        let line_no = idx + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        match check_allow_marker(line) {
            AllowMarker::Valid => continue,
            AllowMarker::Empty(col) => {
                f.push(LintFinding::allow_noreason(rel, line_no, col, "`design-allow:` requires a non-empty reason"));
            }
            AllowMarker::Refused(col) => {
                f.push(LintFinding::allow_noreason(
                    rel,
                    line_no,
                    col,
                    "`design-allow:` reason refused (untrusted text)",
                ));
            }
            AllowMarker::None => {}
        }
        for rule in rules {
            for caps in rule.regex.captures_iter(line) {
                let Some(m) = caps.get(rule.value_group) else {
                    continue;
                };
                for (start, lit) in literals_in(rule.kind, &m) {
                    let col = line.get(..start).map_or(1, |pre| pre.chars().count() + 1);
                    let suggs = normalise_literal(rule.kind, lit)
                        .and_then(|norm| suggestions.get(&(rule.kind.bucket(), norm)))
                        .map(|set| set.iter().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    f.push(LintFinding {
                        file: rel.to_string(),
                        line: Some(line_no),
                        column: Some(col),
                        code: rule.id.clone(),
                        literal: Some(lit.to_string()),
                        suggestions: suggs,
                        detail: format!("literal {lit:?} does not resolve to a design token"),
                    });
                }
            }
        }
    }
}

/// Runs `fid-design lint-literals`. `Err` is an unreadable input, oversized file or bad package
/// (exit 2).
pub fn check(o: &Options<'_>) -> Result<Report, String> {
    let root_md =
        std::fs::symlink_metadata(o.root).map_err(|e| format!("cannot read --root {}: {e}", o.root.display()))?;
    if root_md.file_type().is_symlink() {
        return Err(format!("--root {} is a symbolic link; not followed", o.root.display()));
    }
    if !root_md.is_dir() && !root_md.is_file() {
        return Err(format!("--root {} is not a regular file or directory", o.root.display()));
    }

    let mut f = Vec::new();
    let (rules_text, rules_subj) = match o.rules {
        Some(p) => (read_text(p, MAX_CONFIG_BYTES)?, p.display().to_string()),
        None => (DEFAULT_RULES_TOML.to_string(), "default-rules".to_string()),
    };
    let rules = parse_rules(&rules_text, &rules_subj, &mut f);

    let allow_globs = match o.allowlist {
        Some(p) => {
            let text = read_text(p, MAX_CONFIG_BYTES)?;
            parse_allowlist(&text, "allowlist", &mut f)
        }
        None => Vec::new(),
    };

    let suggestions = match o.package {
        Some(pkg_dir) => {
            let pkg =
                Package::load(pkg_dir).map_err(|e| format!("cannot load --package {}: {e}", pkg_dir.display()))?;
            build_suggestions(&pkg)
        }
        None => BTreeMap::new(),
    };

    let mut files_scanned = 0usize;
    // Files whose path matches some rule's globs, allowlisted or not: zero means the root holds
    // nothing the rules apply to (`LINT-NO-FILES`).
    let mut files_matched = 0usize;
    if root_md.is_file() {
        let name = o
            .root
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("--root {} has a non-UTF-8 file name", o.root.display()))?
            .to_string();
        if rules.iter().any(|r| r.matches_path(&name)) {
            files_matched += 1;
        }
        if !allow_globs.iter().any(|g| glob_match(g, &name)) {
            let active: Vec<&Rule> = rules.iter().filter(|r| r.matches_path(&name)).collect();
            if !active.is_empty() {
                let text = read_text(o.root, MAX_SOURCE_BYTES)?;
                files_scanned += 1;
                scan_text(&name, &text, &active, &suggestions, &mut f);
            }
        }
    } else {
        let mut wf = Vec::new();
        let entries = walk::walk(o.root, "", &mut wf)?;
        for w in wf {
            f.push(LintFinding::schema(RULES_SCHEMA, w.subject, w.detail));
        }
        for e in entries {
            if e.is_dir {
                continue;
            }
            if let Some(d) = path_defect(&e.rel) {
                f.push(LintFinding::schema(RULES_SCHEMA, &e.rel, format!("path refused: {d}")));
                continue;
            }
            if rules.iter().any(|r| r.matches_path(&e.rel)) {
                files_matched += 1;
            }
            if allow_globs.iter().any(|g| glob_match(g, &e.rel)) {
                continue;
            }
            let active: Vec<&Rule> = rules.iter().filter(|r| r.matches_path(&e.rel)).collect();
            if active.is_empty() {
                continue;
            }
            let text = read_text(&e.path, MAX_SOURCE_BYTES)?;
            files_scanned += 1;
            scan_text(&e.rel, &text, &active, &suggestions, &mut f);
        }
    }

    // A broken rule set is already reported (and is why nothing matched); otherwise an empty
    // match is a finding rather than a silent pass.
    let rules_broken = f.iter().any(|x| x.code == RULES_SCHEMA || x.code == REGEX_ERR);
    if files_matched == 0 && !rules_broken {
        f.push(LintFinding::schema(
            NO_FILES,
            ".",
            format!("no file under --root matches any of the {} rule(s); nothing was checked", rules.len()),
        ));
    }

    f.sort();
    f.dedup();
    Ok(Report { files_scanned, rules: rules.len(), findings: f })
}

impl Report {
    /// 0 clean, 1 findings, 2 when no file matched any rule (`LINT-NO-FILES`).
    pub fn exit_code(&self) -> u8 {
        if self.findings.iter().any(|x| x.code == NO_FILES) {
            2
        } else {
            u8::from(!self.findings.is_empty())
        }
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("files_scanned".to_string(), CValue::Num(self.files_scanned as f64));
        m.insert("rules".to_string(), CValue::Num(self.rules as f64));
        m.insert("findings".to_string(), CValue::Arr(self.findings.iter().map(LintFinding::to_cvalue).collect()));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s =
            format!("fid-design lint-literals: {} file(s) scanned, {} rule(s)\n", self.files_scanned, self.rules);
        for x in &self.findings {
            s.push_str(&x.to_string());
            s.push('\n');
        }
        if self.findings.is_empty() {
            s.push_str("ok: no findings\n");
        } else {
            s.push_str(&format!("{} finding(s)\n", self.findings.len()));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rules_compile_cleanly() {
        let mut f = Vec::new();
        let rules = parse_rules(DEFAULT_RULES_TOML, "default", &mut f);
        assert!(f.is_empty(), "{f:?}");
        assert_eq!(rules.len(), 8);
    }

    #[test]
    fn normalisers() {
        assert_eq!(normalise_color("#F0a"), Some("#ff00aa".to_string()));
        assert_eq!(normalise_color("#1F6FEB"), Some("#1f6feb".to_string()));
        assert_eq!(normalise_color("from_rgb(0x1f, 0x6f, 0xeb)"), Some("#1f6feb".to_string()));
        assert_eq!(normalise_color("from_rgba_unmultiplied(31, 111, 235, 255)"), Some("#1f6febff".to_string()));
        assert_eq!(normalise_color("rgb(31, 111, 235)"), Some("#1f6feb".to_string()));
        assert_eq!(normalise_color("rgba(31, 111, 235, 0.5)"), Some("#1f6feb80".to_string()));
        assert_eq!(normalise_color("hsl(0, 100%, 50%)"), Some("#ff0000".to_string()));

        assert_eq!(normalise_dimension("8px"), Some("8px".to_string()));
        assert_eq!(normalise_dimension("12pt"), Some("16px".to_string()));
        assert_eq!(normalise_dimension("6pt"), Some("8px".to_string()));

        assert_eq!(normalise_duration("120ms"), Some("120ms".to_string()));
        assert_eq!(normalise_duration("0.12s"), Some("120ms".to_string()));
        assert_eq!(normalise_duration("from_millis(120)"), Some("120ms".to_string()));
        assert_eq!(normalise_duration("from_secs_f32(0.12)"), Some("120ms".to_string()));
    }

    fn lits(kind: RuleKind, v: &str) -> Vec<String> {
        unit_literals(kind, v).into_iter().map(|(a, z)| v[a..z].to_string()).collect()
    }

    #[test]
    fn unit_literals_find_every_value() {
        assert_eq!(lits(RuleKind::Spacing, "0 8px"), ["8px"]);
        assert_eq!(lits(RuleKind::Spacing, "4px 8px"), ["4px", "8px"]);
        assert_eq!(lits(RuleKind::Spacing, "4px 8PX 2.5pt .5px"), ["4px", "8PX", "2.5pt", ".5px"]);
        assert_eq!(lits(RuleKind::Duration, "opacity 120ms ease"), ["120ms"]);
        assert_eq!(lits(RuleKind::Duration, "spin 1s linear infinite"), ["1s"]);
        assert_eq!(
            lits(RuleKind::Duration, "opacity 120ms ease, transform 0.2s ease-out 50ms"),
            ["120ms", "0.2s", "50ms"]
        );
        assert_eq!(lits(RuleKind::Duration, ".5s"), [".5s"]);
        // Not literals: unitless, glued to a word or a dot, followed by a word, other units.
        assert!(lits(RuleKind::Duration, "fade2s 3 motion.fast").is_empty());
        assert!(lits(RuleKind::Spacing, "space.2 8pxx 2em").is_empty());
        assert!(lits(RuleKind::Duration, "8px").is_empty());
        assert!(lits(RuleKind::Spacing, "120ms").is_empty());
    }

    #[test]
    fn adjacent_hex_colours_are_all_found() {
        let mut f = Vec::new();
        let rules = parse_rules(DEFAULT_RULES_TOML, "default", &mut f);
        let hex = rules.iter().find(|r| r.id == "LIT-COLOR-HEX").map(|r| r.regex.clone());
        let found: Vec<String> = hex
            .map(|re| {
                re.captures_iter("border-color: #1f5fbf #2e8540 #fff,#000;")
                    .filter_map(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(found, ["#1f5fbf", "#2e8540", "#fff", "#000"]);
    }
}
