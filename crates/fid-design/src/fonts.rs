//! `fid-design fonts-check --root <design dir> --allowlist <file> [--json]` (spec node
//! `UX.TOKENS.TYPE`, REQ-UX.TOKENS.TYPE-001 .. -004). Tooling, nearest class R0; D0
//! (`BTreeMap`/`BTreeSet` only, sorted findings, no clock, no network).
//!
//! # Inputs
//!
//! - `<root>/tokens/fonts.json` (`font-list@1`):
//!   `{ "format": "font-list@1", "families": [{ "family", "files", "licence", "source",
//!   "tabular_numerals", "scripts" }] }`.
//! - `<root>/tokens/type.tokens.json`: DTCG token file (`$type: "typography"`) defining the ten
//!   required roles (`type.body`, `type.body.small`, `type.label`, `type.heading.1..3`,
//!   `type.numeric`, `type.numeric.large`, `type.code`, `type.family.display`) across the four
//!   themes (`light`, `dark`, `hc`, `floor`), e.g. `type.body.light` .. `type.body.floor`, each
//!   with `$value`: `{ "fontFamily", "fontSize" ("<n>px"), "fontWeight", "lineHeight",
//!   "fontFeatureSettings"? }`.
//! - `--allowlist <file>`: one SPDX licence expression per line (`#` comments and blank lines
//!   skipped).
//!
//! # Findings (exit 1)
//!
//! - `FONT-LICENCE <family>`: family's SPDX licence is not in the allowlist (REQ-003).
//! - `FONT-TNUM <role>`: a `type.numeric*` role uses a family without `tabular_numerals = true`
//!   or omits `"tnum"` in `fontFeatureSettings` (REQ-002).
//! - `FONT-UNKNOWN <role>`: a role names a `fontFamily` not declared in `tokens/fonts.json`.
//! - `TYPE-SIZE <theme> <role>`: body or label role smaller than 12 px in `light`/`dark`/`hc`
//!   or smaller than 16 px in `floor` (REQ-004).
//! - `TYPE-ROLE-MISSING <role>`: a required role or theme binding is missing (REQ-001).
//! - `FONTS-SCHEMA <subject>`: malformed JSON, wrong format ID, bad field types, or invalid
//!   allowlist.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable input or bad usage.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::icons_prov::spdx_ok;
use crate::review::guard::{defects, path_defect, value_defect};
use crate::review::{self, Kind};
use crate::tokens::THEMES;
use fid_canon::CValue;
use fid_design_pkg::tokens::{collect, Token};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-fonts-check@1";
/// Format ID of `tokens/fonts.json`.
pub const FONT_LIST_FORMAT: &str = "font-list@1";
/// Relative path to `fonts.json` below `--root`.
pub const FONTS_FILE: &str = "tokens/fonts.json";
/// Relative path to `type.tokens.json` below `--root`.
pub const TYPE_TOKENS_FILE: &str = "tokens/type.tokens.json";

/// Finding codes (UX.TOKENS.TYPE Design).
pub const FONT_LICENCE: &str = "FONT-LICENCE";
pub const FONT_TNUM: &str = "FONT-TNUM";
pub const FONT_UNKNOWN: &str = "FONT-UNKNOWN";
pub const TYPE_SIZE: &str = "TYPE-SIZE";
pub const TYPE_ROLE_MISSING: &str = "TYPE-ROLE-MISSING";
pub const SCHEMA: &str = "FONTS-SCHEMA";

/// Minimum body/label font size in `light`, `dark` and `hc` themes (px, REQ-004).
pub const MIN_SIZE_STANDARD_PX: f64 = 12.0;
/// Minimum body/label font size in the `floor` theme (px, REQ-004).
pub const MIN_SIZE_FLOOR_PX: f64 = 16.0;

/// Largest JSON token or font-list file read (4 MiB).
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Largest allowlist file read (64 KiB).
pub const MAX_ALLOWLIST_BYTES: u64 = 64 * 1024;
/// Deepest JSON nesting accepted.
pub const MAX_JSON_DEPTH: usize = 32;
/// Most entries in `families`, `files`, `scripts` or allowlist.
pub const MAX_ITEMS: usize = 10_000;

/// The ten required typography roles of REQ-UX.TOKENS.TYPE-001, sorted.
pub const REQUIRED_ROLES: [&str; 10] = [
    "type.body",
    "type.body.small",
    "type.code",
    "type.family.display",
    "type.heading.1",
    "type.heading.2",
    "type.heading.3",
    "type.label",
    "type.numeric",
    "type.numeric.large",
];

const TOP_KEYS: [&str; 2] = ["families", "format"];
const FAMILY_KEYS: [&str; 6] = ["family", "files", "licence", "scripts", "source", "tabular_numerals"];

/// One parsed entry of `tokens/fonts.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFamily {
    pub family: String,
    pub files: Vec<String>,
    pub licence: String,
    pub source: String,
    pub tabular_numerals: bool,
    pub scripts: Vec<String>,
}

/// Result of `fid-design fonts-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub families: usize,
    pub roles: usize,
    pub allowlist: usize,
    pub findings: Vec<Finding>,
}

fn schema(f: &mut Vec<Finding>, subject: impl Into<String>, detail: impl Into<String>) {
    f.push(Finding::new(SCHEMA, subject, detail));
}

fn valid_https_url(s: &str) -> bool {
    if s.is_empty() || s.len() > 1024 || s != s.trim() {
        return false;
    }
    let Some(rest) = s.strip_prefix("https://") else {
        return false;
    };
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return false;
    }
    let Some(authority) = rest.split(['/', '?', '#']).next() else {
        return false;
    };
    !authority.is_empty() && !authority.contains('@') && !authority.starts_with(':')
}

fn valid_script_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s == s.trim()
        && s.bytes().all(|b| b.is_ascii_alphabetic() || matches!(b, b' ' | b'-'))
}

fn valid_opentype_tag(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 4 && b.iter().all(|c| c.is_ascii_alphanumeric())
}

/// Parses a `<positive decimal>px` dimension string.
pub fn parse_px(s: &str) -> Option<f64> {
    let num = s.strip_suffix("px")?;
    if num.is_empty() || num != num.trim() || num.starts_with('+') || num.starts_with('-') {
        return None;
    }
    let (int, frac) = match num.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (num, None),
    };
    if int.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) || (int.len() > 1 && int.starts_with('0')) {
        return None;
    }
    if let Some(fr) = frac {
        if fr.is_empty() || !fr.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let v: f64 = num.parse().ok()?;
    (v.is_finite() && v > 0.0).then_some(v)
}

/// Whether a role is subject to the REQ-004 minimum body/label size rule.
pub fn is_body_or_label(role: &str) -> bool {
    role == "type.body" || role.starts_with("type.body.") || role == "type.label" || role.starts_with("type.label.")
}

/// Whether a role is subject to the REQ-002 tabular-numeral rule.
pub fn is_numeric_role(role: &str) -> bool {
    role == "type.numeric" || role.starts_with("type.numeric.")
}

/// Parses an SPDX allowlist file (one expression per line; blank lines and `#` comments ignored).
pub fn parse_allowlist(text: &str, f: &mut Vec<Finding>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if out.len() >= MAX_ITEMS {
            schema(f, "allowlist", format!("more than {MAX_ITEMS} entries"));
            break;
        }
        if !defects(line).is_empty() {
            schema(f, format!("allowlist:{}", i + 1), "refused untrusted text");
            continue;
        }
        if !spdx_ok(line) {
            schema(f, format!("allowlist:{}", i + 1), format!("not a valid SPDX licence expression: {line:?}"));
            continue;
        }
        out.insert(line.to_string());
    }
    if out.is_empty() {
        schema(f, "allowlist", "allowlist holds no SPDX licence entries");
    }
    out
}

/// Parses `tokens/fonts.json` (`font-list@1`).
pub fn parse_fonts(text: &str, f: &mut Vec<Finding>) -> BTreeMap<String, FontFamily> {
    let mut out = BTreeMap::new();
    let val = match fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, MAX_JSON_DEPTH) {
        Ok(v) => v,
        Err(e) => {
            schema(f, FONTS_FILE, format!("not valid JSON: {}", e.code()));
            return out;
        }
    };
    if let Some(d) = value_defect(&val) {
        schema(f, FONTS_FILE, format!("text refused at {d}"));
        return out;
    }
    let CValue::Obj(m) = &val else {
        schema(f, FONTS_FILE, format!("{FONT_LIST_FORMAT} must be a JSON object"));
        return out;
    };
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    if keys != TOP_KEYS {
        schema(f, FONTS_FILE, format!("{FONT_LIST_FORMAT} must have exactly the keys {}", TOP_KEYS.join(", ")));
    }
    match m.get("format") {
        Some(CValue::Str(fmt)) if fmt == FONT_LIST_FORMAT => {}
        _ => schema(f, FONTS_FILE, format!("`format` must be \"{FONT_LIST_FORMAT}\"")),
    }
    let Some(CValue::Arr(arr)) = m.get("families") else {
        schema(f, FONTS_FILE, "`families` must be an array");
        return out;
    };
    if arr.is_empty() || arr.len() > MAX_ITEMS {
        schema(f, FONTS_FILE, format!("`families` must hold 1 to {MAX_ITEMS} entries"));
        return out;
    }
    for (i, item) in arr.iter().enumerate() {
        let fallback = format!("families[{i}]");
        let CValue::Obj(e) = item else {
            schema(f, fallback, "family entry must be a JSON object");
            continue;
        };
        let ekeys: Vec<&str> = e.keys().map(String::as_str).collect();
        let family = match e.get("family") {
            Some(CValue::Str(s)) if !s.trim().is_empty() && s.len() <= 128 && s == s.trim() => Some(s.clone()),
            _ => None,
        };
        let subj = family.as_deref().unwrap_or(fallback.as_str());
        let mut ok = true;
        if ekeys != FAMILY_KEYS {
            schema(f, subj, format!("family entry must have exactly the keys {}", FAMILY_KEYS.join(", ")));
            ok = false;
        }
        if family.is_none() {
            schema(f, subj, "`family` must be non-blank text");
            ok = false;
        }
        let mut files = Vec::new();
        match e.get("files") {
            Some(CValue::Arr(a)) if !a.is_empty() && a.len() <= MAX_ITEMS => {
                let mut seen = BTreeSet::new();
                for item in a {
                    match item {
                        CValue::Str(p) if path_defect(p).is_none() => {
                            if !seen.insert(p.clone()) {
                                schema(f, subj, format!("`files` repeats `{p}`"));
                                ok = false;
                            } else {
                                files.push(p.clone());
                            }
                        }
                        _ => {
                            schema(f, subj, "`files` entries must be clean relative paths");
                            ok = false;
                        }
                    }
                }
            }
            _ => {
                schema(f, subj, "`files` must be a non-empty array of file names");
                ok = false;
            }
        }
        let licence = match e.get("licence") {
            Some(CValue::Str(l)) if spdx_ok(l) => l.clone(),
            _ => {
                schema(f, subj, "`licence` must be an SPDX licence expression");
                ok = false;
                String::new()
            }
        };
        let source = match e.get("source") {
            Some(CValue::Str(u)) if valid_https_url(u) => u.clone(),
            _ => {
                schema(f, subj, "`source` must be a public https:// URL");
                ok = false;
                String::new()
            }
        };
        let tabular_numerals = match e.get("tabular_numerals") {
            Some(CValue::Bool(b)) => *b,
            _ => {
                schema(f, subj, "`tabular_numerals` must be a boolean");
                ok = false;
                false
            }
        };
        let mut scripts = Vec::new();
        match e.get("scripts") {
            Some(CValue::Arr(a)) if !a.is_empty() && a.len() <= 256 => {
                let mut seen = BTreeSet::new();
                for item in a {
                    match item {
                        CValue::Str(sc) if valid_script_name(sc) => {
                            if !seen.insert(sc.clone()) {
                                schema(f, subj, format!("`scripts` repeats `{sc}`"));
                                ok = false;
                            } else {
                                scripts.push(sc.clone());
                            }
                        }
                        _ => {
                            schema(f, subj, "`scripts` entries must be script names");
                            ok = false;
                        }
                    }
                }
            }
            _ => {
                schema(f, subj, "`scripts` must be a non-empty array of script names");
                ok = false;
            }
        }
        if let Some(fam) = family {
            if out.contains_key(&fam) {
                schema(f, &fam, "family is listed twice");
            } else if ok {
                out.insert(fam.clone(), FontFamily { family: fam, files, licence, source, tabular_numerals, scripts });
            }
        }
    }
    out
}

fn split_role_theme(token_name: &str) -> Option<(&str, &str)> {
    let (role, theme) = token_name.rsplit_once('.')?;
    (THEMES.contains(&theme) && role.starts_with("type.")).then_some((role, theme))
}

struct TypeBinding {
    family: String,
    size_px: f64,
    has_tnum: bool,
}

fn parse_type_value(token: &Token, f: &mut Vec<Finding>) -> Option<TypeBinding> {
    if token.ty.as_deref() != Some("typography") {
        schema(f, &token.name, "`$type` must be \"typography\"");
        return None;
    }
    let CValue::Obj(m) = &token.value else {
        schema(f, &token.name, "`$value` must be a typography object");
        return None;
    };
    let mut ok = true;
    for k in m.keys() {
        if !matches!(k.as_str(), "fontFamily" | "fontSize" | "fontWeight" | "lineHeight" | "fontFeatureSettings") {
            schema(f, &token.name, format!("unknown key `{k}` in typography `$value`"));
            ok = false;
        }
    }
    let family = match m.get("fontFamily") {
        Some(CValue::Str(s)) if !s.trim().is_empty() && s.len() <= 128 && s == s.trim() => s.clone(),
        _ => {
            schema(f, &token.name, "`fontFamily` must be a non-blank family name");
            ok = false;
            String::new()
        }
    };
    let size_px = match m.get("fontSize") {
        Some(CValue::Str(s)) => match parse_px(s) {
            Some(v) => v,
            None => {
                schema(f, &token.name, "`fontSize` must be a positive px dimension (e.g. \"14px\")");
                ok = false;
                0.0
            }
        },
        _ => {
            schema(f, &token.name, "`fontSize` must be a px dimension string");
            ok = false;
            0.0
        }
    };
    match m.get("fontWeight") {
        Some(CValue::Num(w)) if w.is_finite() && w.fract() == 0.0 && *w >= 100.0 && *w <= 900.0 => {}
        _ => {
            schema(f, &token.name, "`fontWeight` must be an integer between 100 and 900");
            ok = false;
        }
    }
    match m.get("lineHeight") {
        Some(CValue::Num(lh)) if lh.is_finite() && *lh >= 1.0 && *lh <= 3.0 => {}
        _ => {
            schema(f, &token.name, "`lineHeight` must be a finite number between 1.0 and 3.0");
            ok = false;
        }
    }
    let mut has_tnum = false;
    if let Some(v) = m.get("fontFeatureSettings") {
        match v {
            CValue::Arr(a) if a.len() <= 32 => {
                let mut seen = BTreeSet::new();
                for item in a {
                    match item {
                        CValue::Str(tag) if valid_opentype_tag(tag) => {
                            if !seen.insert(tag.clone()) {
                                schema(f, &token.name, format!("`fontFeatureSettings` repeats `{tag}`"));
                                ok = false;
                            }
                            if tag == "tnum" {
                                has_tnum = true;
                            }
                        }
                        _ => {
                            schema(f, &token.name, "`fontFeatureSettings` items must be 4-char OpenType tags");
                            ok = false;
                        }
                    }
                }
            }
            _ => {
                schema(f, &token.name, "`fontFeatureSettings` must be an array of OpenType feature tags");
                ok = false;
            }
        }
    }
    ok.then_some(TypeBinding { family, size_px, has_tnum })
}

/// Runs `fid-design fonts-check` against `<root>` and `<allowlist>`.
/// `Err` is an unreadable input or bad directory (exit 2).
pub fn check(root: &Path, allowlist_path: &Path) -> Result<Report, String> {
    let root_md = std::fs::metadata(root).map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
    if !root_md.is_dir() {
        return Err(format!("--root {} is not a directory", root.display()));
    }
    let allow_text = read_text(allowlist_path, MAX_ALLOWLIST_BYTES)?;

    let fonts_text = match review::kind(root, FONTS_FILE)? {
        Kind::File => read_text(&root.join(FONTS_FILE), MAX_FILE_BYTES)?,
        Kind::Missing => return Err(format!("cannot read {}: file does not exist", root.join(FONTS_FILE).display())),
        Kind::Dir | Kind::Other => return Err(format!("{} is not a regular file", root.join(FONTS_FILE).display())),
    };
    let type_text = match review::kind(root, TYPE_TOKENS_FILE)? {
        Kind::File => read_text(&root.join(TYPE_TOKENS_FILE), MAX_FILE_BYTES)?,
        Kind::Missing => {
            return Err(format!("cannot read {}: file does not exist", root.join(TYPE_TOKENS_FILE).display()))
        }
        Kind::Dir | Kind::Other => {
            return Err(format!("{} is not a regular file", root.join(TYPE_TOKENS_FILE).display()))
        }
    };

    let mut f = Vec::new();
    let allowlist = parse_allowlist(&allow_text, &mut f);
    let families = parse_fonts(&fonts_text, &mut f);

    // REQ-UX.TOKENS.TYPE-003: every family licence must be in the allowlist.
    if !allowlist.is_empty() {
        for fam in families.values() {
            if !allowlist.contains(&fam.licence) {
                f.push(Finding::new(
                    FONT_LICENCE,
                    &fam.family,
                    format!("licence `{}` is not in the allowlist", fam.licence),
                ));
            }
        }
    }

    // Parse tokens/type.tokens.json using fid-design-pkg's DTCG collector.
    let mut roles: BTreeMap<String, BTreeMap<String, TypeBinding>> = BTreeMap::new();
    match fid_canon::parse_with_limits(type_text.as_bytes(), MAX_FILE_BYTES as usize, MAX_JSON_DEPTH) {
        Err(e) => schema(&mut f, TYPE_TOKENS_FILE, format!("not valid JSON: {}", e.code())),
        Ok(val) => {
            if let Some(d) = value_defect(&val) {
                schema(&mut f, TYPE_TOKENS_FILE, format!("text refused at {d}"));
            } else {
                let mut tokens = Vec::new();
                let mut pkg_findings = Vec::new();
                collect(TYPE_TOKENS_FILE, &val, &mut tokens, &mut pkg_findings);
                for pf in pkg_findings {
                    schema(&mut f, pf.subject, pf.detail);
                }
                for tok in &tokens {
                    let Some((role, theme)) = split_role_theme(&tok.name) else {
                        schema(
                            &mut f,
                            &tok.name,
                            "typography token must be named `type.<role>.<theme>` with theme in light, dark, hc, floor",
                        );
                        continue;
                    };
                    if let Some(binding) = parse_type_value(tok, &mut f) {
                        let by_theme = roles.entry(role.to_string()).or_default();
                        if by_theme.contains_key(theme) {
                            schema(&mut f, &tok.name, "duplicate theme binding");
                        } else {
                            by_theme.insert(theme.to_string(), binding);
                        }
                    }
                }
            }
        }
    }

    // REQ-UX.TOKENS.TYPE-001: all 10 required roles present in all 4 themes.
    for req_role in REQUIRED_ROLES {
        match roles.get(req_role) {
            None => f.push(Finding::new(
                TYPE_ROLE_MISSING,
                req_role,
                "required typography role is missing from tokens/type.tokens.json",
            )),
            Some(by_theme) => {
                for theme in THEMES {
                    if !by_theme.contains_key(theme) {
                        f.push(Finding::new(
                            TYPE_ROLE_MISSING,
                            req_role,
                            format!("missing `{theme}` theme binding for `{req_role}`"),
                        ));
                    }
                }
            }
        }
    }

    // Check family resolution, tabular numerals (REQ-002), and minimum sizes (REQ-004).
    for (role, by_theme) in &roles {
        let mut unknown_fams: BTreeSet<String> = BTreeSet::new();
        let mut missing_tnum_family: BTreeSet<String> = BTreeSet::new();
        let mut missing_tnum_feature: BTreeSet<String> = BTreeSet::new();

        for (theme, b) in by_theme {
            match families.get(&b.family) {
                None => {
                    unknown_fams.insert(b.family.clone());
                }
                Some(fam) => {
                    if is_numeric_role(role) && !fam.tabular_numerals {
                        missing_tnum_family.insert(b.family.clone());
                    }
                }
            }
            if is_numeric_role(role) && !b.has_tnum {
                missing_tnum_feature.insert(theme.clone());
            }
            if is_body_or_label(role) {
                let min_px = if theme == "floor" { MIN_SIZE_FLOOR_PX } else { MIN_SIZE_STANDARD_PX };
                if b.size_px < min_px {
                    f.push(Finding::new(
                        TYPE_SIZE,
                        format!("{theme} {role}"),
                        format!("size {}px is below minimum {min_px}px for theme `{theme}`", b.size_px),
                    ));
                }
            }
        }

        for fam in unknown_fams {
            f.push(Finding::new(FONT_UNKNOWN, role, format!("font family {fam:?} is not declared in {FONTS_FILE}")));
        }
        for fam in missing_tnum_family {
            f.push(Finding::new(
                FONT_TNUM,
                role,
                format!("numeric role `{role}` uses family {fam:?} without `tabular_numerals = true`"),
            ));
        }
        if !missing_tnum_feature.is_empty() {
            let themes: Vec<&str> = missing_tnum_feature.iter().map(String::as_str).collect();
            f.push(Finding::new(
                FONT_TNUM,
                role,
                format!(
                    "numeric role `{role}` does not set `\"tnum\"` in `fontFeatureSettings` (themes: {})",
                    themes.join(", ")
                ),
            ));
        }
    }

    f.sort();
    f.dedup();
    Ok(Report { families: families.len(), roles: roles.len(), allowlist: allowlist.len(), findings: f })
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("families".to_string(), CValue::Num(self.families as f64));
        m.insert("roles".to_string(), CValue::Num(self.roles as f64));
        m.insert("allowlist".to_string(), CValue::Num(self.allowlist as f64));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "fid-design fonts-check: {} family/families, {} role(s), {} allowlisted licence(s)\n",
            self.families, self.roles, self.allowlist
        );
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn px_and_role_classifiers() {
        assert_eq!(parse_px("14px"), Some(14.0));
        assert_eq!(parse_px("13.5px"), Some(13.5));
        assert_eq!(parse_px("0px"), None);
        assert_eq!(parse_px("-12px"), None);
        assert_eq!(parse_px("14pt"), None);
        assert_eq!(parse_px("014px"), None);
        assert_eq!(parse_px("14"), None);

        assert!(is_body_or_label("type.body"));
        assert!(is_body_or_label("type.body.small"));
        assert!(is_body_or_label("type.label"));
        assert!(!is_body_or_label("type.heading.1"));
        assert!(!is_body_or_label("type.code"));

        assert!(is_numeric_role("type.numeric"));
        assert!(is_numeric_role("type.numeric.large"));
        assert!(!is_numeric_role("type.body"));
    }

    #[test]
    fn allowlist_parsing() {
        let mut f = Vec::new();
        let a = parse_allowlist("# Comment\nOFL-1.1\nApache-2.0\n\nMIT\n", &mut f);
        assert!(f.is_empty(), "{f:?}");
        assert_eq!(a.len(), 3);
        assert!(a.contains("OFL-1.1"));

        let mut f2 = Vec::new();
        let empty = parse_allowlist("# only comments\n", &mut f2);
        assert!(empty.is_empty());
        assert_eq!(f2.len(), 1);
        assert_eq!(f2[0].code, SCHEMA);
    }
}
