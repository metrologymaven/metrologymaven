//! `fid-design space-check --root <design dir> [--json]` (spec node `UX.TOKENS.SPACE`): the
//! non-colour layout tokens: the 4 px spacing grid and its density modes, the minimum
//! interactive target per theme and density, elevation levels and motion.
//!
//! Inputs below `<root>/tokens/` (each a JSON file, duplicate keys refused, read with
//! `fid_design_pkg::tokens::collect`, so the DTCG syntax rules of `UX.SYSTEM.PKG` apply):
//! - `space.tokens.json`: `prim.space.<n>` literal `dimension` tokens and `sem.space.*`
//!   tokens aliasing them; a semantic token's own `$value` is its `comfortable` binding.
//! - `density/{compact,comfortable}.json` (`fid-design-density@1`):
//!   `{ "format", "density", "bindings": { "<sem.space name>": "{prim.space.<n>}" },
//!   "target_min": { "<theme>": "<n>px" } }`, every theme of [`THEMES`].
//! - `size.tokens.json`: `size.target.min`, a `dimension` whose `$value` is the `light` target of
//!   the `comfortable` mode. The target is square: the value bounds width and height.
//! - `motion.tokens.json`: every group `motion.<group>.<name>` holding `duration` (`duration`),
//!   `easing` (`cubicBezier`, four numbers, x1 and x2 in 0..=1) and `reduced-duration`
//!   (`duration`). The spec's `reduced_duration` is spelled with a hyphen because token names
//!   allow no underscore (`UX.SYSTEM.PKG`).
//! - `elevation.tokens.json`: levels `elev.0` .. `elev.<n-1>`, each a `shadow` token whose
//!   `$extensions."org.metrologymaven.elevation"` names a `surface` and/or `border` cue: a
//!   `sem.surface.*` or `sem.border.*` token of `semantic/color.tokens.json`.
//!
//! Rules (findings, exit 1):
//! - `SPACE-GRID <name>`: a `prim.space.*` token, a `sem.space.*` default or a density binding
//!   whose value is not a whole multiple of 4 px (REQ-001).
//! - `SPACE-DENSITY <mode> <name>`: a mode that does not bind a `sem.space.*` token, binds a name
//!   that is not one, binds something other than a `prim.space.*` alias, or (mode `comfortable`)
//!   differs from the token's own `$value` (REQ-001).
//! - `SIZE-TARGET <theme> <mode>`: a target below 24 px, or below 44 px in theme `floor`, or a
//!   theme without a target; `SIZE-TARGET default` when `size.target.min` is missing or differs
//!   from `light` in `comfortable` (REQ-002).
//! - `MOTION-REDUCED <name>`: a motion token without a valid duration, easing or
//!   reduced-duration, or with a reduced-duration other than 0 ms (REQ-003).
//! - `MOTION-LONG <name>`: a `motion.panel.*` duration above 250 ms (REQ-003).
//! - `ELEV-LEVELS`: more than [`MAX_ELEVATION_LEVELS`] levels, or levels not named `0`..`n-1`
//!   (REQ-004).
//! - `ELEV-NOSHADOW-CUE <name>`: a level without a surface or border cue, with an unknown cue
//!   key, a cue that is not a `sem.surface.*` / `sem.border.*` colour token, or the same cues as
//!   a lower level, so that without shadows it could not be told apart (REQ-004).
//! - `SPACE-SCHEMA <file or name>`: a missing or unreadable file, a wrong format ID, a value of
//!   the wrong `$type`, a malformed shadow, or an unknown key; DTCG syntax findings of the
//!   package reader also land here.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable root or limits exceeded. Determinism D0:
//! fixed files, `BTreeMap` only, no clock, and every comparison is on exact decimals (no
//! floating point but the easing range check on parsed JSON numbers).

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::tokens::THEMES;
use fid_canon::CValue;
use fid_design_pkg::tokens::{collect, parse_alias, Token};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-space-check@1";
/// Density file format ID.
pub const DENSITY_FORMAT: &str = "fid-design-density@1";
/// The two density modes.
pub const DENSITIES: [&str; 2] = ["comfortable", "compact"];
/// The spacing grid, in px.
pub const GRID_PX: u64 = 4;
/// Least target size in every theme and mode, px (WCAG 2.2 SC 2.5.8).
pub const MIN_TARGET_PX: u64 = 24;
/// Least target size in the floor theme, px [unverified].
pub const MIN_FLOOR_TARGET_PX: u64 = 44;
/// Longest panel transition, ms [unverified].
pub const MAX_PANEL_MS: u64 = 250;
/// Most elevation levels.
pub const MAX_ELEVATION_LEVELS: usize = 5;
/// The `$extensions` key of an elevation cue.
pub const ELEVATION_EXT: &str = "org.metrologymaven.elevation";

pub const GRID: &str = "SPACE-GRID";
pub const DENSITY: &str = "SPACE-DENSITY";
pub const TARGET: &str = "SIZE-TARGET";
pub const REDUCED: &str = "MOTION-REDUCED";
pub const LONG: &str = "MOTION-LONG";
pub const LEVELS: &str = "ELEV-LEVELS";
pub const NO_CUE: &str = "ELEV-NOSHADOW-CUE";
pub const SCHEMA: &str = "SPACE-SCHEMA";

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MOTION_PARTS: [&str; 3] = ["duration", "easing", "reduced-duration"];
const SHADOW_KEYS: [&str; 5] = ["blur", "color", "offsetX", "offsetY", "spread"];

/// The outcome of one run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub space_tokens: usize,
    pub motion_tokens: usize,
    pub elevation_levels: usize,
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let num = |n: usize| CValue::Num(n as f64);
        let mut counts = BTreeMap::new();
        counts.insert("elevation_levels".to_string(), num(self.elevation_levels));
        counts.insert("motion_tokens".to_string(), num(self.motion_tokens));
        counts.insert("space_tokens".to_string(), num(self.space_tokens));
        let mut o = BTreeMap::new();
        o.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        o.insert("counts".to_string(), CValue::Obj(counts));
        o.insert("findings".to_string(), findings_value(&self.findings));
        o.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(o)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "fid-design space-check: {} space tokens, {} motion tokens, {} elevation levels\n",
            self.space_tokens, self.motion_tokens, self.elevation_levels
        );
        findings_text(&mut out, &self.findings);
        out
    }
}

// ------------------------------------------------------------------ reading

/// Reads `<root>/tokens/<rel>` as JSON. Missing or invalid: a finding and `None`. Too large or
/// unreadable for another reason than absence: `Err` (exit 2).
fn read_json(root: &Path, rel: &str, f: &mut Vec<Finding>) -> Result<Option<CValue>, String> {
    let path = root.join("tokens").join(rel);
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            f.push(Finding::new(SCHEMA, format!("tokens/{rel}"), "missing file"));
            return Ok(None);
        }
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        Ok(m) if !m.is_file() => {
            f.push(Finding::new(SCHEMA, format!("tokens/{rel}"), "not a regular file"));
            return Ok(None);
        }
        Ok(_) => {}
    }
    let text = read_text(&path, MAX_FILE_BYTES)?;
    match fid_canon::parse(text.as_bytes()) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            f.push(Finding::new(SCHEMA, format!("tokens/{rel}"), format!("not valid JSON: {}", e.code())));
            Ok(None)
        }
    }
}

/// The tokens of one token file, by name; DTCG syntax findings become `SPACE-SCHEMA`.
fn tokens_of(root: &Path, rel: &str, f: &mut Vec<Finding>) -> Result<BTreeMap<String, Token>, String> {
    Ok(tokens_and_raw(root, rel, f)?.0)
}

/// [`tokens_of`] plus the parsed file, for the `$extensions` the package reader does not keep.
fn tokens_and_raw(
    root: &Path,
    rel: &str,
    f: &mut Vec<Finding>,
) -> Result<(BTreeMap<String, Token>, Option<CValue>), String> {
    let mut out = BTreeMap::new();
    let Some(v) = read_json(root, rel, f)? else { return Ok((out, None)) };
    let mut list = Vec::new();
    let mut pkg = Vec::new();
    collect(&format!("tokens/{rel}"), &v, &mut list, &mut pkg);
    for p in pkg {
        f.push(Finding::new(SCHEMA, p.subject.clone(), p.detail.clone()));
    }
    for t in list {
        if out.contains_key(&t.name) {
            f.push(Finding::new(SCHEMA, t.name.clone(), format!("tokens/{rel}: defined twice")));
        } else {
            out.insert(t.name.clone(), t);
        }
    }
    Ok((out, Some(v)))
}

/// Whole pixels of `"<n>px"` when `n` is a decimal integer (a fraction of zeros allowed).
fn whole_px(v: &str) -> Option<u64> {
    let n = v.strip_suffix("px")?;
    let (int, frac) = n.split_once('.').unwrap_or((n, ""));
    if int.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b == b'0') {
        return None;
    }
    if int.len() > 1 && int.starts_with('0') {
        return None;
    }
    int.parse().ok()
}

/// `"<n>ms"` compared with `limit` exactly: `Some(true)` when above it, `None` when malformed.
fn ms_above(v: &str, limit: u64) -> Option<bool> {
    let n = v.strip_suffix("ms")?;
    let (int, frac) = n.split_once('.').unwrap_or((n, ""));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) || (n.contains('.') && !digits(frac)) {
        return None;
    }
    let int = int.trim_start_matches('0');
    if int.len() > 18 {
        return Some(true);
    }
    let whole: u64 = if int.is_empty() { 0 } else { int.parse().ok()? };
    Some(whole > limit || (whole == limit && frac.bytes().any(|b| b != b'0')))
}

fn is_zero_ms(v: &str) -> bool {
    ms_above(v, 0) == Some(false)
}

fn str_value(t: &Token) -> Option<&str> {
    match &t.value {
        CValue::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

// ------------------------------------------------------------------ space and density

fn check_space(
    root: &Path,
    space: &BTreeMap<String, Token>,
    r: &mut Report,
    f: &mut Vec<Finding>,
) -> Result<Option<String>, String> {
    let mut light = None;
    let prim: BTreeMap<&str, &Token> =
        space.iter().filter(|(n, _)| n.starts_with("prim.space.")).map(|(n, t)| (n.as_str(), t)).collect();
    let sem: BTreeMap<&str, &Token> =
        space.iter().filter(|(n, _)| n.starts_with("sem.space.")).map(|(n, t)| (n.as_str(), t)).collect();
    r.space_tokens = prim.len() + sem.len();
    for (name, t) in space {
        if !(name.starts_with("prim.space.") || name.starts_with("sem.space.")) {
            f.push(Finding::new(
                SCHEMA,
                name.clone(),
                "tokens/space.tokens.json holds only prim.space.* and sem.space.*",
            ));
        } else if t.ty.as_deref() != Some("dimension") {
            f.push(Finding::new(SCHEMA, name.clone(), "a space token must be a dimension"));
        }
    }
    if sem.is_empty() {
        f.push(Finding::new(SCHEMA, "tokens/space.tokens.json", "no sem.space.* token"));
    }
    // The px value of each primitive; a primitive must be a literal.
    let mut px: BTreeMap<&str, u64> = BTreeMap::new();
    for (name, t) in &prim {
        match str_value(t) {
            Some(v) if t.alias().is_none() => match whole_px(v) {
                Some(n) if n % GRID_PX == 0 => {
                    px.insert(name, n);
                }
                _ => f.push(Finding::new(GRID, *name, format!("{v} is not a whole multiple of {GRID_PX}px"))),
            },
            _ => f.push(Finding::new(SCHEMA, *name, "a prim.space token must be a literal dimension")),
        }
    }
    // A binding must alias a prim.space token; returns the alias target when it does.
    let target = |v: &str| -> Option<String> {
        parse_alias(v).filter(|a| a.starts_with("prim.space.") && prim.contains_key(a)).map(str::to_string)
    };
    for (name, t) in &sem {
        match (str_value(t), t.alias()) {
            (Some(v), Some(_)) if target(v).is_some() => {}
            (Some(v), None) if whole_px(v).is_none_or(|n| n % GRID_PX != 0) => {
                f.push(Finding::new(GRID, *name, format!("{v} is not a whole multiple of {GRID_PX}px")));
            }
            _ => f.push(Finding::new(SCHEMA, *name, "a sem.space token must alias a prim.space token")),
        }
    }
    for mode in DENSITIES {
        let rel = format!("density/{mode}.json");
        let Some(v) = read_json(root, &rel, f)? else { continue };
        let CValue::Obj(o) = &v else {
            f.push(Finding::new(SCHEMA, format!("tokens/{rel}"), "not a JSON object"));
            continue;
        };
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        if keys != ["bindings", "density", "format", "target_min"] {
            f.push(Finding::new(SCHEMA, format!("tokens/{rel}"), "keys must be bindings, density, format, target_min"));
        }
        if o.get("format") != Some(&CValue::Str(DENSITY_FORMAT.to_string()))
            || o.get("density") != Some(&CValue::Str(mode.to_string()))
        {
            f.push(Finding::new(
                SCHEMA,
                format!("tokens/{rel}"),
                format!("format must be {DENSITY_FORMAT}, density {mode}"),
            ));
        }
        let empty = BTreeMap::new();
        let bindings = match o.get("bindings") {
            Some(CValue::Obj(b)) => b,
            _ => &empty,
        };
        for name in sem.keys() {
            if !bindings.contains_key(*name) {
                f.push(Finding::new(DENSITY, format!("{mode} {name}"), "not bound in this mode"));
            }
        }
        for (name, b) in bindings {
            if !sem.contains_key(name.as_str()) {
                f.push(Finding::new(DENSITY, format!("{mode} {name}"), "not a sem.space token"));
                continue;
            }
            let CValue::Str(s) = b else {
                f.push(Finding::new(DENSITY, format!("{mode} {name}"), "a binding must be a {prim.space.<n>} alias"));
                continue;
            };
            // An alias to an off-grid primitive is reported once, at the primitive.
            if target(s).is_none() {
                f.push(Finding::new(DENSITY, format!("{mode} {name}"), "a binding must be a {prim.space.<n>} alias"));
            } else if mode == "comfortable" && sem.get(name.as_str()).copied().and_then(str_value) != Some(s.as_str()) {
                f.push(Finding::new(DENSITY, format!("{mode} {name}"), "differs from the token's own $value"));
            }
        }
        if let Some(l) = check_targets(mode, o.get("target_min"), f) {
            light = Some(l);
        }
    }
    Ok(light)
}

/// Checks one mode's targets; returns the `light` target of the `comfortable` mode.
fn check_targets(mode: &str, v: Option<&CValue>, f: &mut Vec<Finding>) -> Option<String> {
    let mut light = None;
    let empty = BTreeMap::new();
    let targets = match v {
        Some(CValue::Obj(t)) => t,
        _ => &empty,
    };
    for k in targets.keys() {
        if !THEMES.contains(&k.as_str()) {
            f.push(Finding::new(SCHEMA, format!("density/{mode}.json"), "target_min names an unknown theme"));
        }
    }
    for theme in THEMES {
        let min = if theme == "floor" { MIN_FLOOR_TARGET_PX } else { MIN_TARGET_PX };
        let subject = format!("{theme} {mode}");
        match targets.get(theme) {
            Some(CValue::Str(s)) => match whole_px(s) {
                Some(n) if n >= min => {
                    if theme == "light" && mode == "comfortable" {
                        light = Some(s.clone());
                    }
                }
                Some(_) => f.push(Finding::new(TARGET, subject, format!("{s} is below {min}px x {min}px"))),
                None => f.push(Finding::new(TARGET, subject, "the target must be a whole number of px")),
            },
            _ => f.push(Finding::new(TARGET, subject, "no target_min for this theme")),
        }
    }
    light
}

// ------------------------------------------------------------------ motion

fn check_motion(motion: &BTreeMap<String, Token>, r: &mut Report, f: &mut Vec<Finding>) {
    let mut groups: BTreeMap<String, BTreeMap<&str, &Token>> = BTreeMap::new();
    for (name, t) in motion {
        let split = name.rsplit_once('.').filter(|(g, _)| g.starts_with("motion.") && g.matches('.').count() == 2);
        match split {
            Some((g, part)) if MOTION_PARTS.contains(&part) => {
                groups.entry(g.to_string()).or_default().insert(part, t);
            }
            _ => f.push(Finding::new(
                SCHEMA,
                name.clone(),
                "a motion token is motion.<group>.<name>.{duration,easing,reduced-duration}",
            )),
        }
    }
    r.motion_tokens = groups.len();
    for (g, parts) in &groups {
        let duration =
            parts.get("duration").copied().filter(|t| t.ty.as_deref() == Some("duration")).and_then(str_value);
        let reduced =
            parts.get("reduced-duration").copied().filter(|t| t.ty.as_deref() == Some("duration")).and_then(str_value);
        let easing = parts.get("easing").filter(|t| t.ty.as_deref() == Some("cubicBezier")).map(|t| &t.value);
        let mut why = Vec::new();
        if duration.and_then(|d| ms_above(d, 0)).is_none() {
            why.push("no valid duration");
        }
        if !easing.is_some_and(valid_bezier) {
            why.push("no valid cubicBezier easing");
        }
        match reduced {
            Some(v) if is_zero_ms(v) => {}
            Some(_) => why.push("reduced-duration is not 0ms"),
            None => why.push("no reduced-duration"),
        }
        if !why.is_empty() {
            f.push(Finding::new(REDUCED, g.clone(), why.join("; ")));
        }
        if g.starts_with("motion.panel.") {
            if let Some(d) = duration {
                if ms_above(d, MAX_PANEL_MS) == Some(true) {
                    f.push(Finding::new(LONG, g.clone(), format!("{d} is longer than {MAX_PANEL_MS}ms")));
                }
            }
        }
    }
}

fn valid_bezier(v: &CValue) -> bool {
    let CValue::Arr(a) = v else { return false };
    let nums: Vec<f64> = a
        .iter()
        .filter_map(|x| match x {
            CValue::Num(n) if n.is_finite() => Some(*n),
            _ => None,
        })
        .collect();
    nums.len() == 4 && a.len() == 4 && (0.0..=1.0).contains(&nums[0]) && (0.0..=1.0).contains(&nums[2])
}

// ------------------------------------------------------------------ elevation

fn check_elevation(
    elev: &BTreeMap<String, Token>,
    raw: Option<&CValue>,
    colours: &BTreeMap<String, Token>,
    r: &mut Report,
    f: &mut Vec<Finding>,
) {
    let mut levels: BTreeMap<u64, &Token> = BTreeMap::new();
    for (name, t) in elev {
        match name.strip_prefix("elev.").filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) {
            Some(n) if n.len() == 1 || !n.starts_with('0') => {
                if let Ok(k) = n.parse::<u64>() {
                    levels.insert(k, t);
                }
            }
            _ => f.push(Finding::new(SCHEMA, name.clone(), "an elevation token is elev.<level>")),
        }
    }
    r.elevation_levels = levels.len();
    if levels.len() > MAX_ELEVATION_LEVELS {
        f.push(Finding::new(LEVELS, "elev", format!("{} levels, at most {MAX_ELEVATION_LEVELS}", levels.len())));
    }
    if levels.keys().enumerate().any(|(i, k)| *k != i as u64) {
        f.push(Finding::new(LEVELS, "elev", "levels must be numbered 0 to n-1 without gaps"));
    }
    if levels.is_empty() {
        f.push(Finding::new(LEVELS, "elev", "no elevation level"));
    }
    let mut seen: BTreeSet<(Option<String>, Option<String>)> = BTreeSet::new();
    for (k, t) in &levels {
        let name = format!("elev.{k}");
        if t.ty.as_deref() != Some("shadow") || !valid_shadow(&t.value) {
            f.push(Finding::new(
                SCHEMA,
                name.clone(),
                "an elevation level is a shadow {blur,color,offsetX,offsetY,spread}",
            ));
        }
        match cue_of(raw, *k, colours) {
            Err(why) => f.push(Finding::new(NO_CUE, name, why)),
            Ok(cue) => {
                if !seen.insert(cue) {
                    f.push(Finding::new(NO_CUE, name, "the same surface and border cue as a lower level"));
                }
            }
        }
    }
}

fn valid_shadow(v: &CValue) -> bool {
    let CValue::Obj(o) = v else { return false };
    let keys: Vec<&str> = o.keys().map(String::as_str).collect();
    if keys != SHADOW_KEYS {
        return false;
    }
    o.iter().all(|(k, x)| match (k.as_str(), x) {
        ("color", CValue::Str(s)) => {
            let h = s.strip_prefix('#').unwrap_or("");
            (h.len() == 6 || h.len() == 8) && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        ("color", _) => false,
        (_, CValue::Str(s)) => s.strip_suffix("px").is_some_and(|n| {
            let n = n.strip_prefix('-').unwrap_or(n);
            let (i, fr) = n.split_once('.').unwrap_or((n, "0"));
            !i.is_empty()
                && !fr.is_empty()
                && i.bytes().all(|b| b.is_ascii_digit())
                && fr.bytes().all(|b| b.is_ascii_digit())
        }),
        _ => false,
    })
}

type Cue = (Option<String>, Option<String>);

fn member<'v>(v: Option<&'v CValue>, key: &str) -> Option<&'v CValue> {
    match v {
        Some(CValue::Obj(o)) => o.get(key),
        _ => None,
    }
}

/// The cue of level `k`, read from the file's `elev.<k>.$extensions` (the package reader does
/// not keep `$extensions` in a `Token`).
fn cue_of(raw: Option<&CValue>, k: u64, colours: &BTreeMap<String, Token>) -> Result<Cue, String> {
    let level = member(member(raw, "elev"), &k.to_string());
    let ext = member(member(level, "$extensions"), ELEVATION_EXT);
    let Some(CValue::Obj(cue)) = ext else {
        return Err(format!("no $extensions.{ELEVATION_EXT} cue"));
    };
    if cue.keys().any(|k| k != "surface" && k != "border") {
        return Err("a cue has only surface and border".to_string());
    }
    let pick = |key: &str, prefix: &str| -> Result<Option<String>, String> {
        match cue.get(key) {
            None => Ok(None),
            Some(CValue::Str(n)) if n.starts_with(prefix) && colours.contains_key(n) => Ok(Some(n.clone())),
            Some(_) => Err(format!("{key} must name a {prefix}* colour token")),
        }
    };
    let surface = pick("surface", "sem.surface.")?;
    let border = pick("border", "sem.border.")?;
    if surface.is_none() && border.is_none() {
        return Err("no surface or border cue".to_string());
    }
    Ok((surface, border))
}

// ------------------------------------------------------------------ entry

/// Runs the check over `<root>/tokens/`.
pub fn check(root: &Path) -> Result<Report, String> {
    let dir = root.join("tokens");
    if !std::fs::symlink_metadata(&dir).map(|m| m.is_dir()).unwrap_or(false) {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let mut f = Vec::new();
    let mut r = Report::default();
    let space = tokens_of(root, "space.tokens.json", &mut f)?;
    let light = check_space(root, &space, &mut r, &mut f)?;
    let size = tokens_of(root, "size.tokens.json", &mut f)?;
    for name in size.keys().filter(|n| n.as_str() != "size.target.min") {
        f.push(Finding::new(SCHEMA, name.clone(), "tokens/size.tokens.json holds only size.target.min"));
    }
    let default = size.get("size.target.min").filter(|t| t.ty.as_deref() == Some("dimension")).and_then(str_value);
    match (default, &light) {
        (Some(d), Some(l)) if d == l => {}
        (Some(_), Some(_)) => {
            f.push(Finding::new(TARGET, "default", "size.target.min differs from light in comfortable density"))
        }
        (None, _) => f.push(Finding::new(TARGET, "default", "size.target.min is missing or not a dimension")),
        (Some(_), None) => {}
    }
    let motion = tokens_of(root, "motion.tokens.json", &mut f)?;
    check_motion(&motion, &mut r, &mut f);
    let (elev, raw) = tokens_and_raw(root, "elevation.tokens.json", &mut f)?;
    let colours = tokens_of(root, "semantic/color.tokens.json", &mut f)?;
    check_elevation(&elev, raw.as_ref(), &colours, &mut r, &mut f);
    f.sort();
    f.dedup();
    r.findings = f;
    Ok(r)
}
