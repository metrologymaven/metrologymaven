//! `fid-design status-check --root <design dir> [--json]` (spec node `UX.TOKENS.STATUS`): the
//! structure of the locked status palette `<root>/tokens/status.json`, its set membership, lock
//! flag and guard-threshold fields, the glyph and text cue of every state, and that every
//! `sem.status.<state>` token resolves to exactly the palette's colour in every theme. Colour
//! distances are not gated here; that is `UX.TOKENS.CHECK`. The CIEDE2000 formula and the
//! colour-vision simulation it will use are in [`ciede2000`] and [`cvd`].
//!
//! `tokens/status.json` (`status-palette@1`), an object with exactly these keys:
//! - `format`: `"status-palette@1"`;
//! - `locked`: `true` (REQ-UX.TOKENS.STATUS-005);
//! - `formula`: `"ciede2000"` and `min_delta_e2000`: `20`, the defaults of the runtime guard of
//!   `VIZ.OVERLAY.STATUS` (REQ-UX.TOKENS.STATUS-003);
//! - `sets`: `{ "conforming": ["conforms"], "nonconforming": ["nonconforms",
//!   "out-of-tolerance", "stale"] }` in any order (REQ-UX.TOKENS.STATUS-002). `out-of-tolerance`
//!   stands for the out-of-tolerance region colours of `UX.TOKENS.DATAPAL`; it has no colour here;
//! - `cues`: `{ "<state>": { "glyph": "state.<state>", "text": "icon.state.<state>" } }`, the glyph
//!   ID and accessible-name message ID of `UX.ICONS.CLASS` for every state, so that no state is
//!   ever carried by colour alone (principle P-NOCOLOR);
//! - `themes`: `{ "<theme>": { "<state>": "#rrggbb" } }` for the four themes and six states,
//!   lowercase hex.
//!
//! States are named as their semantic tokens: [`STATES`].
//!
//! Rules (findings, exit 1):
//! - `STATUS-MISSING <theme> <state>`: no colour for a state in a theme (a missing theme gives
//!   one finding per state).
//! - `STATUS-SET <set>`: a set's membership differs from the above or lists a state twice;
//!   `STATUS-SET <name>`: the brand tier (`tokens/brand-tier.json`) lists a `sem.status.*` token.
//! - `STATUS-UNLOCKED tokens/status.json`: `locked` is `false`.
//! - `STATUS-THRESHOLD formula|min_delta_e2000`: a value other than `ciede2000` or `20`.
//! - `STATUS-ALIAS <theme> <state>`: `sem.status.<state>` does not resolve in that theme, or
//!   resolves to another colour; `STATUS-ALIAS <name>`: a `sem.status.*` token that is not a state.
//! - `STATUS-CUE <state>`: a state without its glyph and text cue, or with another state's.
//! - `STATUS-SCHEMA <file, theme or theme state>`: invalid JSON, a missing file, a wrong format ID,
//!   a missing or extra key, a value of the wrong type, an unknown theme or state, a colour that
//!   is not lowercase `#rrggbb`.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable root or limits exceeded (including an
//! unreadable token tree, which the alias rule resolves with [`crate::tokens::check`]).
//! Determinism D0: `BTreeMap` only, no clock, no floating point.

pub mod ciede2000;
pub mod cvd;

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::tokens::THEMES;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-status-check@1";
/// Palette file format ID.
pub const PALETTE_FORMAT: &str = "status-palette@1";
/// The palette file, relative to the design root.
pub const PALETTE_FILE: &str = "tokens/status.json";
/// The six display states, in the spec's order.
pub const STATES: [&str; 6] = ["conforms", "nonconforms", "indeterminate", "not-decidable", "not-evaluated", "stale"];
/// Members of the conforming set.
pub const CONFORMING: [&str; 1] = ["conforms"];
/// Members of the nonconforming set.
pub const NONCONFORMING: [&str; 3] = ["nonconforms", "out-of-tolerance", "stale"];
/// The guard's colour-difference formula ID (REQ-UX.TOKENS.STATUS-003).
pub const FORMULA: &str = "ciede2000";
/// The guard's threshold [unverified threshold, REQ-UX.TOKENS.STATUS-003].
pub const MIN_DELTA_E2000: f64 = 20.0;
/// Top-level keys of the palette file, sorted.
pub const KEYS: [&str; 7] = ["cues", "format", "formula", "locked", "min_delta_e2000", "sets", "themes"];

const MAX_FILE_BYTES: u64 = 1024 * 1024;

pub const MISSING: &str = "STATUS-MISSING";
pub const SET: &str = "STATUS-SET";
pub const UNLOCKED: &str = "STATUS-UNLOCKED";
pub const THRESHOLD: &str = "STATUS-THRESHOLD";
pub const ALIAS: &str = "STATUS-ALIAS";
pub const CUE: &str = "STATUS-CUE";
pub const SCHEMA: &str = "STATUS-SCHEMA";

/// The glyph ID of a state (`UX.ICONS.CLASS`).
pub fn glyph_id(state: &str) -> String {
    format!("state.{state}")
}

/// The accessible-name message ID of a state's glyph (`UX.ICONS.CLASS`).
pub fn text_id(state: &str) -> String {
    format!("icon.state.{state}")
}

/// Result of `status-check`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Valid colours by theme, then state.
    pub themes: BTreeMap<String, BTreeMap<String, String>>,
    /// `Some` when `locked` is a boolean.
    pub locked: Option<bool>,
    /// Sorted, de-duplicated.
    pub findings: Vec<Finding>,
}

/// Runs the check on `<root>`. `Err` (exit 2): the root, the token tree or the palette cannot be
/// read, or a limit is exceeded.
pub fn check(root: &Path) -> Result<Report, String> {
    let tokens = crate::tokens::check(root)?;
    let mut f = Vec::new();
    let mut r = Report::default();
    if let Some(v) = read_palette(root, &mut f)? {
        parse(&v, &mut r, &mut f);
    }

    // REQ-UX.TOKENS.STATUS-001: sem.status.<state> aliases exactly these values.
    for theme in THEMES {
        let values = tokens.themes.get(theme).map(|t| &t.values);
        let Some(row) = r.themes.get(theme) else {
            continue;
        };
        for (state, hex) in row {
            let name = format!("sem.status.{state}");
            match values.and_then(|v| v.get(&name)) {
                None => f.push(Finding::new(
                    ALIAS,
                    format!("{theme} {state}"),
                    format!("{name} does not resolve in theme {theme}; it must alias {hex}"),
                )),
                Some(v) if v.value != *hex => f.push(Finding::new(
                    ALIAS,
                    format!("{theme} {state}"),
                    format!("{name} resolves to {} ({}); the locked palette says {hex}", v.value, v.prim),
                )),
                Some(_) => {}
            }
        }
    }
    let mut extra = BTreeSet::new();
    for t in tokens.themes.values() {
        for n in t.values.keys() {
            if let Some(s) = n.strip_prefix("sem.status.") {
                if !STATES.contains(&s) {
                    extra.insert(n.clone());
                }
            }
        }
    }
    for n in extra {
        f.push(Finding::new(
            ALIAS,
            &n,
            format!("not a display state; status tokens are sem.status.{{{}}}", STATES.join(",")),
        ));
    }

    // REQ-UX.TOKENS.STATUS-005: the brand tier holds no status token.
    for b in tokens.brand_tier.iter().filter(|b| b.starts_with("sem.status.")) {
        f.push(Finding::new(SET, b, "listed in the brand tier; status tokens are locked and never brand tokens"));
    }

    f.sort();
    f.dedup();
    r.findings = f;
    Ok(r)
}

/// Reads and parses the palette. A missing, non-regular or invalid file is a finding and
/// `Ok(None)`; an I/O failure or an exceeded limit is `Err`.
fn read_palette(root: &Path, f: &mut Vec<Finding>) -> Result<Option<CValue>, String> {
    let p = root.join("tokens").join("status.json");
    match std::fs::symlink_metadata(&p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            f.push(Finding::new(SCHEMA, PALETTE_FILE, "missing"));
            return Ok(None);
        }
        Err(e) => return Err(format!("cannot read {}: {e}", p.display())),
        Ok(m) if !m.is_file() => {
            f.push(Finding::new(SCHEMA, PALETTE_FILE, "not a regular file (symbolic links are not followed)"));
            return Ok(None);
        }
        Ok(_) => {}
    }
    let text = read_text(&p, MAX_FILE_BYTES)?;
    match fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, fid_design_pkg::MAX_JSON_DEPTH) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            f.push(Finding::new(SCHEMA, PALETTE_FILE, format!("invalid JSON: {e}")));
            Ok(None)
        }
    }
}

fn is_hex(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s.bytes().skip(1).all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn parse(v: &CValue, r: &mut Report, f: &mut Vec<Finding>) {
    let CValue::Obj(m) = v else {
        f.push(Finding::new(SCHEMA, PALETTE_FILE, "must be a JSON object"));
        return;
    };
    if m.get("format") != Some(&CValue::Str(PALETTE_FORMAT.to_string())) {
        f.push(Finding::new(SCHEMA, PALETTE_FILE, format!("format must be {PALETTE_FORMAT:?}")));
        return;
    }
    for k in m.keys().filter(|k| !KEYS.contains(&k.as_str())) {
        f.push(Finding::new(SCHEMA, PALETTE_FILE, format!("unknown key {k:?}")));
    }
    for k in KEYS.iter().filter(|k| !m.contains_key(**k)) {
        f.push(Finding::new(SCHEMA, PALETTE_FILE, format!("missing key {k:?}")));
    }

    match m.get("locked") {
        Some(CValue::Bool(true)) => r.locked = Some(true),
        Some(CValue::Bool(false)) => {
            r.locked = Some(false);
            f.push(Finding::new(UNLOCKED, PALETTE_FILE, "locked is false; the status palette is locked = true"));
        }
        Some(_) => f.push(Finding::new(SCHEMA, PALETTE_FILE, "locked must be a boolean")),
        None => {}
    }
    match m.get("formula") {
        Some(CValue::Str(s)) if s == FORMULA => {}
        Some(CValue::Str(s)) => {
            f.push(Finding::new(THRESHOLD, "formula", format!("{s:?}; the guard's formula is {FORMULA:?}")))
        }
        Some(_) => f.push(Finding::new(SCHEMA, PALETTE_FILE, "formula must be a string")),
        None => {}
    }
    match m.get("min_delta_e2000") {
        Some(CValue::Num(n)) if *n == MIN_DELTA_E2000 => {}
        Some(CValue::Num(n)) => f.push(Finding::new(
            THRESHOLD,
            "min_delta_e2000",
            format!("{n}; the guard's threshold is {MIN_DELTA_E2000}"),
        )),
        Some(_) => f.push(Finding::new(SCHEMA, PALETTE_FILE, "min_delta_e2000 must be a number")),
        None => {}
    }
    if let Some(s) = m.get("sets") {
        parse_sets(s, f);
    }
    if let Some(c) = m.get("cues") {
        parse_cues(c, f);
    }
    if let Some(t) = m.get("themes") {
        parse_themes(t, r, f);
    }
}

fn parse_sets(v: &CValue, f: &mut Vec<Finding>) {
    let shape = "sets must be an object with exactly the keys conforming, nonconforming, each an array of state names";
    let CValue::Obj(m) = v else {
        f.push(Finding::new(SCHEMA, PALETTE_FILE, shape));
        return;
    };
    if m.len() != 2 || !m.contains_key("conforming") || !m.contains_key("nonconforming") {
        f.push(Finding::new(SCHEMA, PALETTE_FILE, shape));
        return;
    }
    for (set, want) in [("conforming", &CONFORMING[..]), ("nonconforming", &NONCONFORMING[..])] {
        let Some(CValue::Arr(items)) = m.get(set) else {
            f.push(Finding::new(SCHEMA, PALETTE_FILE, shape));
            continue;
        };
        let mut got = Vec::new();
        for item in items {
            match item {
                CValue::Str(s) => got.push(s.as_str()),
                _ => f.push(Finding::new(SCHEMA, PALETTE_FILE, format!("sets.{set} holds a non-string"))),
            }
        }
        let uniq: BTreeSet<&str> = got.iter().copied().collect();
        let want_set: BTreeSet<&str> = want.iter().copied().collect();
        if uniq.len() != got.len() {
            f.push(Finding::new(SET, set, "lists a state twice"));
        }
        if uniq != want_set {
            f.push(Finding::new(SET, set, format!("is [{}]; must be exactly [{}]", got.join(", "), want.join(", "))));
        }
    }
}

fn parse_cues(v: &CValue, f: &mut Vec<Finding>) {
    let CValue::Obj(m) = v else {
        f.push(Finding::new(SCHEMA, PALETTE_FILE, "cues must be an object keyed by state"));
        return;
    };
    for k in m.keys().filter(|k| !STATES.contains(&k.as_str())) {
        f.push(Finding::new(SCHEMA, PALETTE_FILE, format!("cues: {k:?} is not a display state")));
    }
    for state in STATES {
        let (glyph, text) = (glyph_id(state), text_id(state));
        let want = format!("must be {{\"glyph\": {glyph:?}, \"text\": {text:?}}}; colour is never the only cue");
        let ok = match m.get(state) {
            Some(CValue::Obj(c)) => {
                c.len() == 2
                    && c.get("glyph") == Some(&CValue::Str(glyph.clone()))
                    && c.get("text") == Some(&CValue::Str(text.clone()))
            }
            _ => false,
        };
        if !ok {
            let what = if m.contains_key(state) { "cue " } else { "missing cue; " };
            f.push(Finding::new(CUE, state, format!("{what}{want}")));
        }
    }
}

fn parse_themes(v: &CValue, r: &mut Report, f: &mut Vec<Finding>) {
    let CValue::Obj(m) = v else {
        f.push(Finding::new(SCHEMA, PALETTE_FILE, "themes must be an object keyed by theme"));
        return;
    };
    for k in m.keys().filter(|k| !THEMES.contains(&k.as_str())) {
        f.push(Finding::new(SCHEMA, k, format!("not a theme; themes are {}", THEMES.join(", "))));
    }
    for theme in THEMES {
        let row = match m.get(theme) {
            Some(CValue::Obj(row)) => row,
            Some(_) => {
                f.push(Finding::new(SCHEMA, theme, "must be an object of \"<state>\": \"#rrggbb\""));
                continue;
            }
            None => {
                for state in STATES {
                    f.push(Finding::new(MISSING, format!("{theme} {state}"), "no colour (theme missing)"));
                }
                continue;
            }
        };
        let mut out = BTreeMap::new();
        for k in row.keys().filter(|k| !STATES.contains(&k.as_str())) {
            f.push(Finding::new(SCHEMA, format!("{theme} {k}"), "not a display state"));
        }
        for state in STATES {
            match row.get(state) {
                Some(CValue::Str(s)) if is_hex(s) => {
                    out.insert(state.to_string(), s.clone());
                }
                Some(_) => {
                    f.push(Finding::new(SCHEMA, format!("{theme} {state}"), "must be a lowercase \"#rrggbb\" string"))
                }
                None => f.push(Finding::new(MISSING, format!("{theme} {state}"), "no colour")),
            }
        }
        r.themes.insert(theme.to_string(), out);
    }
}

// ------------------------------------------------------------------ report

fn num(n: usize) -> CValue {
    CValue::Num(n as f64)
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    /// Number of valid colours.
    pub fn colours(&self) -> usize {
        self.themes.values().map(BTreeMap::len).sum()
    }

    pub fn to_cvalue(&self) -> CValue {
        let themes = self
            .themes
            .iter()
            .map(|(t, row)| {
                (t.clone(), CValue::Obj(row.iter().map(|(s, h)| (s.clone(), CValue::Str(h.clone()))).collect()))
            })
            .collect();
        let strs = |xs: &[&str]| CValue::Arr(xs.iter().map(|x| CValue::Str((*x).to_string())).collect());
        let mut counts = BTreeMap::new();
        counts.insert("states".to_string(), num(STATES.len()));
        counts.insert("themes".to_string(), num(self.themes.len()));
        counts.insert("colours".to_string(), num(self.colours()));
        let mut o = BTreeMap::new();
        o.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        o.insert("states".to_string(), strs(&STATES));
        o.insert("counts".to_string(), CValue::Obj(counts));
        o.insert("themes".to_string(), CValue::Obj(themes));
        o.insert("locked".to_string(), self.locked.map_or(CValue::Null, CValue::Bool));
        o.insert("findings".to_string(), findings_value(&self.findings));
        o.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(o)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "fid-design status-check: {} states x {} themes, {} colours\n",
            STATES.len(),
            self.themes.len(),
            self.colours()
        );
        for t in THEMES {
            if let Some(row) = self.themes.get(t) {
                let cells: Vec<String> =
                    STATES.iter().filter_map(|st| row.get(*st).map(|h| format!("{st} {h}"))).collect();
                s.push_str(&format!("{t}: {}\n", cells.join(", ")));
            }
        }
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_shape() {
        assert!(is_hex("#00637e"));
        for bad in ["#00637E", "00637e", "#00637", "#00637e0", "#0g637e", ""] {
            assert!(!is_hex(bad), "{bad}");
        }
    }

    #[test]
    fn cue_ids() {
        assert_eq!(glyph_id("not-evaluated"), "state.not-evaluated");
        assert_eq!(text_id("stale"), "icon.state.stale");
        assert_eq!(KEYS.iter().copied().collect::<BTreeSet<_>>().len(), KEYS.len());
        let mut sorted = KEYS;
        sorted.sort_unstable();
        assert_eq!(sorted, KEYS);
    }
}
