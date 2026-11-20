//! `fid-design palettes-check --root <design dir> [--baseline <digests>] [--json]` (spec node
//! `UX.TOKENS.DATAPAL`): the catalogue of data palettes used for deviation colour maps, the
//! structure of every palette file, the default marker, immutability against a baseline of
//! digests, and the measured perceptual properties of every palette.
//!
//! Inputs below `<root>/tokens/palettes/`:
//! - `catalogue.json` (`palette-catalogue@1`): `{ "format", "default": "<ref>", "palettes":
//!   ["<ref>", ..] }`. A palette reference is `<id>@<major>`, the ID being `pal.<group>.<name>`
//!   (segments `[a-z0-9][a-z0-9-]*`) and the major version a positive integer without leading
//!   zeros. The catalogue holds at least [`REQUIRED`], and the default is [`DEFAULT`].
//! - `<ref>.json`, one per listed palette and nothing else in the directory: `{ "id", "version",
//!   "kind": "stepped"|"continuous", "colors": ["#rrggbb", ..], "under", "over", "centre_index"
//!   (stepped only), "source", "licence", "digest" }`, colours lowercase. `id` and `version` are
//!   the reference's two parts; `pal.step.*` IDs are stepped and `pal.cont.*` continuous;
//!   `digest` is `sha256:` of the RFC 8785 form of `{ "colors", "over", "under" }` ([`digest`]).
//! - `--baseline <file>` (`palette-digests@1`): `{ "format", "digests": { "<ref>": "sha256:.." } }`,
//!   the digests of a released catalogue.
//!
//! Rules (findings, exit 1):
//! - `PAL-SCHEMA <file or ref>`: a missing or invalid file, an unknown or missing key, a value
//!   of the wrong type, a malformed reference, an ID or version that differs from the file name,
//!   a kind that differs from the ID's group, a colour that is not lowercase `#rrggbb`, a `digest`
//!   field that differs from the colours, a listed palette without a file or a file the
//!   catalogue does not list, and a catalogue without one of [`REQUIRED`].
//! - `PAL-DEFAULT tokens/palettes/catalogue.json`: no default, or a default other than
//!   [`DEFAULT`] (REQ-UX.TOKENS.DATAPAL-001).
//! - `PAL-STEPS <ref>`: a stepped palette whose step count is even or outside
//!   [`MIN_STEPS`]..=[`MAX_STEPS`] (REQ-002).
//! - `PAL-CENTRE <ref>`: an odd stepped palette whose `centre_index` is not the middle step, or
//!   whose middle step is not neutral (OKLab chroma above [`MAX_NEUTRAL_CHROMA`]) (REQ-002).
//! - `PAL-SAMPLES <ref>`: a continuous palette with fewer than [`MIN_SAMPLES`] or more than
//!   [`MAX_SAMPLES`] samples (REQ-003).
//! - `PAL-LICENCE <ref>`: an empty source, or a licence other than one of
//!   [`THIRD_PARTY_LICENCES`]; [`PROJECT_LICENCE`] is accepted only for a palette made by this
//!   project, whose source starts with [`ORIGINAL`] (REQ-003, LICENSE.md). The prefix is
//!   cross-checked against the review record: a source starting [`ORIGINAL`] needs a valid
//!   originality declaration `review/origin/tokens/palettes/<ref>.json.origin.json`
//!   ([`crate::review::check_decl`]), and a palette from elsewhere must not have one.
//! - `PAL-IMMUTABLE <ref>`: with `--baseline`, a baseline palette whose colours now give another
//!   digest, or that is no longer in the catalogue (REQ-004).
//!
//! Measured perceptual rules, on every palette whose kind and colours are valid. Lightness is
//! CIELAB L*; colour differences are CIEDE2000 under normal vision and under simulated
//! protanopia, deuteranopia and tritanopia ([`crate::status::cvd`], the smallest of the four):
//! - `PAL-LIGHTNESS <ref>`: lightness not strictly monotonic along each arm of an odd stepped
//!   palette (from the centre outwards), or along a continuous palette (REQ-UX.TOKENS-007).
//! - `PAL-DELTA <ref> <a> <b>`: two colours less than [`MIN_STEP_DELTA_E2000`] apart, for every
//!   pair of adjacent steps, each mirrored pair of an odd stepped palette (step `c - k` against
//!   `c + k`, so the two arms stay apart), `under` against the first colour and the last colour
//!   against `over` (REQ-UX.TOKENS-007, REQ-005). `a` and `b` are step indexes, `under` or `over`.
//! - `PAL-OOT <ref> <theme> under|over`: an out-of-tolerance colour (the palette's `under` and
//!   `over`, the out-of-tolerance region colours of the status palette's nonconforming set) less
//!   than [`crate::status::MIN_DELTA_E2000`] from that theme's `sem.status.conforms`
//!   (REQ-UX.TOKENS-009), or less than [`MIN_OOT_NEUTRAL_DELTA_E2000`] from one of that theme's
//!   neutral status colours ([`NEUTRAL_TOKENS`], as `STATUS-DISTANCE`);
//!   `PAL-OOT <ref> under|over`: an out-of-tolerance colour less than
//!   [`MIN_OOT_INRANGE_DELTA_E2000`] from any in-range colour (step or sample) of any palette in
//!   the catalogue, so that an out-of-range region never looks in range (maintainer decision on
//!   PR 345); `PAL-OOT <theme> <token>` when one of those status tokens does not resolve.
//! - `PAL-CONTRAST <ref> <theme> under|over`: an out-of-tolerance colour below
//!   [`MIN_OOT_CONTRAST`]:1 (WCAG 2.2 contrast, SC 1.4.11) against that theme's
//!   `sem.surface.base`; `PAL-CONTRAST <theme> sem.surface.base` when that token does not resolve.
//! - `PAL-UNIFORM <ref>`: a continuous palette whose CIEDE2000 (normal vision) between samples
//!   `i` and `i + `[`WINDOW`], over every `i`, varies by more than [`MAX_WINDOW_RATIO`] (largest
//!   over smallest), or is zero somewhere: the map is not perceptually uniform.
//! - `PAL-CLAIMED <ref> <theme>`: a colour of the palette (steps or samples, `under`, `over`)
//!   less than [`crate::tokens::MIN_DELTA_E_OK`] in OKLab from a claimed semantic role of that
//!   theme (`sem.*` with a segment starting `claimed`): data colours are measured values, and
//!   claimed values never look measured (UX.TOKENS.COLOR's `TOK-CLAIMED-AS-MEASURED` floor).
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable root, token tree or baseline, or a
//! limit exceeded. Determinism D0: sorted directory walk, `BTreeMap` only, no clock. The
//! floating-point rules use `cbrt`, `powf`, `atan2`, `sin`, `cos`, `exp`, no fused multiply-add;
//! the shipped palettes clear every threshold by a margin far above any last-bit difference
//! between platforms, and the report prints no floating-point value except in a finding's
//! detail, rounded.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::status::ciede2000::lab_of_linear;
use crate::status::cvd::{delta_e2000_under, Vision};
use crate::tokens::oklab::{delta_e_ok, oklab};
use crate::tokens::wcag::{contrast_ratio, linear, parse_hex, Rgb};
use crate::tokens::THEMES;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-palettes-check@1";
/// Catalogue format ID.
pub const CATALOGUE_FORMAT: &str = "palette-catalogue@1";
/// Baseline format ID.
pub const DIGESTS_FORMAT: &str = "palette-digests@1";
/// The palette directory, relative to the design root.
pub const PAL_DIR: &str = "tokens/palettes";
/// The catalogue, relative to the design root.
pub const CATALOGUE_FILE: &str = "tokens/palettes/catalogue.json";
/// Palettes the catalogue must hold (REQ-UX.TOKENS.DATAPAL-001).
pub const REQUIRED: [&str; 4] = ["pal.step.tol@1", "pal.step.cvd@1", "pal.cont.seq-a@1", "pal.cont.seq-cvd@1"];
/// The default deviation palette (REQ-UX.TOKENS.DATAPAL-001).
pub const DEFAULT: &str = "pal.step.cvd@1";
/// Step-count bounds of a stepped palette (REQ-UX.TOKENS.DATAPAL-002).
pub const MIN_STEPS: usize = 7;
pub const MAX_STEPS: usize = 15;
/// Sample-count bounds of a continuous palette (REQ-UX.TOKENS.DATAPAL-003; the upper bound is a
/// size limit of this check).
pub const MIN_SAMPLES: usize = 256;
pub const MAX_SAMPLES: usize = 4096;
/// Largest OKLab chroma of a neutral centre step [unverified].
pub const MAX_NEUTRAL_CHROMA: f64 = 0.02;
/// Least CIEDE2000 between adjacent or mirrored steps under every vision condition
/// [unverified threshold, REQ-UX.TOKENS-007].
pub const MIN_STEP_DELTA_E2000: f64 = 10.0;
/// Least WCAG contrast of an out-of-tolerance colour against the base surface (SC 1.4.11).
pub const MIN_OOT_CONTRAST: f64 = 3.0;
/// Licences accepted for a palette taken from elsewhere (REQ-UX.TOKENS.DATAPAL-003).
pub const THIRD_PARTY_LICENCES: [&str; 2] = ["CC0-1.0", "public-domain"];
/// The licence of data made by this project (LICENSE.md).
pub const PROJECT_LICENCE: &str = "CC-BY-4.0";
/// How the source of a palette made by this project starts.
pub const ORIGINAL: &str = "original: ";
/// The out-of-tolerance reference colour and the surface the colours sit on.
pub const CONFORMS_TOKEN: &str = "sem.status.conforms";
pub const SURFACE_TOKEN: &str = "sem.surface.base";
/// The neutral status colours the out-of-tolerance colours keep away from (UX.TOKENS.STATUS).
pub const NEUTRAL_TOKENS: [&str; 3] =
    ["sem.status.indeterminate", "sem.status.not-decidable", "sem.status.not-evaluated"];
/// Least CIEDE2000 between an out-of-tolerance colour and a neutral status colour, under every
/// vision condition (as `STATUS-DISTANCE`).
pub const MIN_OOT_NEUTRAL_DELTA_E2000: f64 = crate::status::MIN_NEUTRAL_DELTA_E2000;
/// Least CIEDE2000 between an out-of-tolerance colour and any in-range colour of any catalogue
/// palette, under every vision condition (maintainer decision on PR 345).
pub const MIN_OOT_INRANGE_DELTA_E2000: f64 = 10.0;
/// Span, in samples, of the perceptual-uniformity windows of a continuous palette.
pub const WINDOW: usize = 16;
/// Largest ratio of the biggest to the smallest window difference of a continuous palette.
pub const MAX_WINDOW_RATIO: f64 = 1.5;

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_PALETTES: usize = 64;
const MAX_DIR_ENTRIES: usize = 256;

pub const SCHEMA: &str = "PAL-SCHEMA";
pub const DEFAULT_CODE: &str = "PAL-DEFAULT";
pub const STEPS: &str = "PAL-STEPS";
pub const CENTRE: &str = "PAL-CENTRE";
pub const SAMPLES: &str = "PAL-SAMPLES";
pub const LICENCE: &str = "PAL-LICENCE";
pub const IMMUTABLE: &str = "PAL-IMMUTABLE";
pub const LIGHTNESS: &str = "PAL-LIGHTNESS";
pub const DELTA: &str = "PAL-DELTA";
pub const OOT: &str = "PAL-OOT";
pub const CONTRAST: &str = "PAL-CONTRAST";
pub const CLAIMED: &str = "PAL-CLAIMED";
pub const UNIFORM: &str = "PAL-UNIFORM";

/// Kind of a palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Stepped,
    Continuous,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Stepped => "stepped",
            Kind::Continuous => "continuous",
        }
    }
}

/// What the report says about one listed palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub kind: Option<Kind>,
    pub count: usize,
    /// The digest of the colours as read, `None` when they are not strings.
    pub digest: Option<String>,
}

/// Result of `palettes-check`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The catalogue's default, when it is a string.
    pub default: Option<String>,
    /// Every listed palette whose file was read, by reference.
    pub palettes: BTreeMap<String, Summary>,
    /// Whether a baseline was given.
    pub baseline: bool,
    /// The palettes whose kind and colours are valid, by reference (read by `color-check`).
    pub colours: BTreeMap<String, Colours>,
    /// Sorted, de-duplicated.
    pub findings: Vec<Finding>,
}

/// A palette whose kind and colours are valid: the input of the perceptual rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Colours {
    pub kind: Kind,
    pub colors: Vec<Rgb>,
    pub under: Rgb,
    pub over: Rgb,
}

// ------------------------------------------------------------------ colour helpers

/// CIELAB L* of an opaque sRGB colour.
pub fn lstar(c: Rgb) -> f64 {
    lab_of_linear([linear(c[0]), linear(c[1]), linear(c[2])])[0]
}

/// OKLab chroma `sqrt(a² + b²)`.
pub fn chroma(c: Rgb) -> f64 {
    let [_, a, b] = oklab(c);
    (a * a + b * b).sqrt()
}

/// The smallest CIEDE2000 over the four vision conditions, and the condition that gives it.
pub fn min_delta_e2000(a: Rgb, b: Rgb) -> (f64, Vision) {
    let mut worst = (f64::INFINITY, Vision::Normal);
    for v in Vision::ALL {
        let d = delta_e2000_under(v, a, b);
        if d < worst.0 {
            worst = (d, v);
        }
    }
    worst
}

/// `sha256:` of the RFC 8785 form of `{ "colors", "over", "under" }`.
pub fn digest(colors: &[String], under: &str, over: &str) -> Result<String, fid_canon::CanonError> {
    let mut m = BTreeMap::new();
    m.insert("colors".to_string(), CValue::Arr(colors.iter().map(|c| CValue::Str(c.clone())).collect()));
    m.insert("over".to_string(), CValue::Str(over.to_string()));
    m.insert("under".to_string(), CValue::Str(under.to_string()));
    fid_canon::digest(&CValue::Obj(m)).map(|d| d.to_string())
}

/// Splits `<id>@<major>`; `None` when it breaks the grammar.
pub fn parse_ref(s: &str) -> Option<(&str, u64)> {
    let (id, major) = s.split_once('@')?;
    let segs: Vec<&str> = id.split('.').collect();
    let seg_ok = |g: &str| {
        g.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && g.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    if segs.len() != 3 || segs[0] != "pal" || !segs[1..].iter().all(|g| seg_ok(g)) {
        return None;
    }
    if major.is_empty() || major.len() > 4 || major.starts_with('0') || !major.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((id, major.parse().ok()?))
}

fn is_hex(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s.bytes().skip(1).all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Claimed semantic role: `sem.*` with a segment after the tier starting `claimed` and none
/// starting `measured`.
fn is_claimed(name: &str) -> bool {
    let mut segs = name.split('.');
    if segs.next() != Some("sem") {
        return false;
    }
    let rest: Vec<&str> = segs.collect();
    rest.iter().any(|s| s.starts_with("claimed")) && !rest.iter().any(|s| s.starts_with("measured"))
}

// ------------------------------------------------------------------ check

/// Runs the check on `<root>`. `Err` (exit 2): the root, the token tree, a palette file or the
/// baseline cannot be read, the baseline is invalid, or a limit is exceeded.
pub fn check(root: &Path, baseline: Option<&Path>) -> Result<Report, String> {
    let tokens = crate::tokens::check(root)?;
    let base = match baseline {
        Some(p) => Some(read_baseline(p)?),
        None => None,
    };
    let mut f = Vec::new();
    let mut r = Report { baseline: base.is_some(), ..Report::default() };

    let catalogue = read_catalogue(root, &mut r, &mut f)?;
    let listed = catalogue.clone().unwrap_or_default();
    let files = list_dir(root, &mut f)?;
    let mut colours: BTreeMap<String, Colours> = BTreeMap::new();
    for reference in &listed {
        let name = format!("{reference}.json");
        if !files.contains(&name) {
            f.push(Finding::new(SCHEMA, reference, format!("listed in the catalogue but {PAL_DIR}/{name} is missing")));
            continue;
        }
        let rel = format!("{PAL_DIR}/{name}");
        let text = read_text(&root.join(&rel), MAX_FILE_BYTES)?;
        match fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, fid_design_pkg::MAX_JSON_DEPTH) {
            Ok(v) => {
                let decl = read_decl(root, reference)?;
                if let Some(c) = parse_palette(reference, &v, decl, &mut r, &mut f) {
                    colours.insert(reference.clone(), c);
                }
            }
            Err(e) => f.push(Finding::new(SCHEMA, reference, format!("invalid JSON: {e}"))),
        }
    }
    let listed_files: BTreeSet<String> = listed.iter().map(|x| format!("{x}.json")).collect();
    // Without a readable catalogue nothing is listed, and only the catalogue finding is given.
    let unlisted = files.iter().filter(|n| *n != "catalogue.json" && !listed_files.contains(*n));
    for name in unlisted.filter(|_| catalogue.is_some()) {
        f.push(Finding::new(SCHEMA, format!("{PAL_DIR}/{name}"), "not a palette the catalogue lists"));
    }

    // REQ-UX.TOKENS.DATAPAL-004: colours never change within an ID and major version.
    if let Some(base) = base {
        for (reference, want) in base {
            match r.palettes.get(&reference) {
                None => f.push(Finding::new(
                    IMMUTABLE,
                    &reference,
                    "in the baseline but no longer in the catalogue; old versions stay",
                )),
                Some(s) if s.digest.as_deref() != Some(want.as_str()) => f.push(Finding::new(
                    IMMUTABLE,
                    &reference,
                    format!(
                        "colours give {}, the baseline says {want}; a colour change needs a new major version",
                        s.digest.as_deref().unwrap_or("no digest")
                    ),
                )),
                Some(_) => {}
            }
        }
    }

    perceptual(&colours, &tokens, &mut f);
    f.sort();
    f.dedup();
    r.findings = f;
    r.colours = colours;
    Ok(r)
}

/// The listed references (valid, de-duplicated, in catalogue order). A missing or invalid
/// catalogue is a finding and `None`.
fn read_catalogue(root: &Path, r: &mut Report, f: &mut Vec<Finding>) -> Result<Option<Vec<String>>, String> {
    let p = root.join(CATALOGUE_FILE);
    match std::fs::symlink_metadata(&p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            f.push(Finding::new(SCHEMA, CATALOGUE_FILE, "missing"));
            return Ok(None);
        }
        Err(e) => return Err(format!("cannot read {}: {e}", p.display())),
        Ok(m) if !m.is_file() => {
            f.push(Finding::new(SCHEMA, CATALOGUE_FILE, "not a regular file (symbolic links are not followed)"));
            return Ok(None);
        }
        Ok(_) => {}
    }
    let text = read_text(&p, MAX_FILE_BYTES)?;
    let v = match fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, fid_design_pkg::MAX_JSON_DEPTH)
    {
        Ok(v) => v,
        Err(e) => {
            f.push(Finding::new(SCHEMA, CATALOGUE_FILE, format!("invalid JSON: {e}")));
            return Ok(None);
        }
    };
    let CValue::Obj(m) = &v else {
        f.push(Finding::new(SCHEMA, CATALOGUE_FILE, "must be a JSON object"));
        return Ok(None);
    };
    if m.get("format") != Some(&CValue::Str(CATALOGUE_FORMAT.to_string())) {
        f.push(Finding::new(SCHEMA, CATALOGUE_FILE, format!("format must be {CATALOGUE_FORMAT:?}")));
        return Ok(None);
    }
    for k in m.keys().filter(|k| !["default", "format", "palettes"].contains(&k.as_str())) {
        f.push(Finding::new(SCHEMA, CATALOGUE_FILE, format!("unknown key {k:?}")));
    }
    let mut listed = Vec::new();
    match m.get("palettes") {
        Some(CValue::Arr(items)) if items.len() <= MAX_PALETTES => {
            for item in items {
                match item {
                    CValue::Str(s) if parse_ref(s).is_some() => {
                        if listed.contains(s) {
                            f.push(Finding::new(SCHEMA, CATALOGUE_FILE, format!("{s} is listed twice")));
                        } else {
                            listed.push(s.clone());
                        }
                    }
                    CValue::Str(s) => f.push(Finding::new(
                        SCHEMA,
                        CATALOGUE_FILE,
                        format!("{s:?} is not a palette reference pal.<group>.<name>@<major>"),
                    )),
                    _ => f.push(Finding::new(SCHEMA, CATALOGUE_FILE, "palettes holds a non-string")),
                }
            }
        }
        Some(CValue::Arr(_)) => {
            f.push(Finding::new(SCHEMA, CATALOGUE_FILE, format!("more than {MAX_PALETTES} palettes")))
        }
        _ => f.push(Finding::new(SCHEMA, CATALOGUE_FILE, "palettes must be an array of palette references")),
    }
    for req in REQUIRED.iter().filter(|q| !listed.iter().any(|l| l == *q)) {
        f.push(Finding::new(SCHEMA, CATALOGUE_FILE, format!("required palette {req} is not listed")));
    }
    match m.get("default") {
        Some(CValue::Str(d)) => {
            r.default = Some(d.clone());
            if d != DEFAULT {
                f.push(Finding::new(DEFAULT_CODE, CATALOGUE_FILE, format!("default is {d}; it must be {DEFAULT}")));
            } else if !listed.contains(d) {
                f.push(Finding::new(DEFAULT_CODE, CATALOGUE_FILE, format!("default {d} is not listed")));
            }
        }
        Some(_) => f.push(Finding::new(DEFAULT_CODE, CATALOGUE_FILE, "default must be a palette reference")),
        None => f.push(Finding::new(DEFAULT_CODE, CATALOGUE_FILE, format!("no default; it must be {DEFAULT}"))),
    }
    Ok(Some(listed))
}

/// Names of the regular files in the palette directory, sorted. Anything else is a finding; a
/// missing directory lists nothing (the catalogue finding says why).
fn list_dir(root: &Path, f: &mut Vec<Finding>) -> Result<BTreeSet<String>, String> {
    let dir = root.join(PAL_DIR);
    let mut out = BTreeSet::new();
    match std::fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
        Ok(m) if !m.is_dir() => {
            f.push(Finding::new(SCHEMA, PAL_DIR, "not a directory (symbolic links are not followed)"));
            return Ok(out);
        }
        Ok(_) => {}
    }
    let rd = std::fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut n = 0usize;
    for entry in rd {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        n += 1;
        if n > MAX_DIR_ENTRIES {
            return Err(format!("{} holds more than {MAX_DIR_ENTRIES} entries", dir.display()));
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let md = std::fs::symlink_metadata(entry.path()).map_err(|e| format!("cannot read {name}: {e}"))?;
        if md.is_file() {
            out.insert(name);
        } else {
            f.push(Finding::new(
                SCHEMA,
                format!("{PAL_DIR}/{name}"),
                "not a regular file (symbolic links are not followed)",
            ));
        }
    }
    Ok(out)
}

fn read_baseline(p: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = read_text(p, MAX_FILE_BYTES)?;
    let bad = |why: &str| format!("{}: not a {DIGESTS_FORMAT} file: {why}", p.display());
    let v = fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, fid_design_pkg::MAX_JSON_DEPTH)
        .map_err(|e| bad(&format!("invalid JSON: {e}")))?;
    let CValue::Obj(m) = &v else {
        return Err(bad("not an object"));
    };
    if m.len() != 2 || m.get("format") != Some(&CValue::Str(DIGESTS_FORMAT.to_string())) {
        return Err(bad("keys must be exactly format and digests"));
    }
    let Some(CValue::Obj(d)) = m.get("digests") else {
        return Err(bad("digests must be an object"));
    };
    let mut out = BTreeMap::new();
    for (k, v) in d {
        match v {
            CValue::Str(s) if parse_ref(k).is_some() && fid_types::Digest::is_valid(s) => {
                out.insert(k.clone(), s.clone());
            }
            _ => return Err(bad(&format!("entry {k:?} must map a palette reference to a sha256: digest"))),
        }
    }
    Ok(out)
}

/// The originality declaration of a palette file: `None` when there is none, else whether it
/// is valid for that artefact.
fn read_decl(root: &Path, reference: &str) -> Result<Option<Result<(), String>>, String> {
    let artefact = format!("{PAL_DIR}/{reference}.json");
    let rel = format!("{}{artefact}{}", crate::review::ORIGIN_DIR, crate::review::ORIGIN_SUFFIX);
    let p = root.join(&rel);
    match std::fs::symlink_metadata(&p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", p.display())),
        Ok(m) if !m.is_file() => Ok(Some(Err(format!("{rel} is not a regular file")))),
        Ok(_) => {
            let text = read_text(&p, MAX_FILE_BYTES)?;
            Ok(Some(crate::review::check_decl(&text, &artefact).map_err(|e| format!("{rel}: {e}"))))
        }
    }
}

const KEYS_STEPPED: [&str; 10] =
    ["centre_index", "colors", "digest", "id", "kind", "licence", "over", "source", "under", "version"];

/// Structural rules of one palette file. Returns the colours when the kind and every colour
/// are valid.
fn parse_palette(
    reference: &str,
    v: &CValue,
    decl: Option<Result<(), String>>,
    r: &mut Report,
    f: &mut Vec<Finding>,
) -> Option<Colours> {
    let schema = |f: &mut Vec<Finding>, d: String| f.push(Finding::new(SCHEMA, reference, d));
    let CValue::Obj(m) = v else {
        schema(f, "must be a JSON object".to_string());
        return None;
    };
    let (id, major) = parse_ref(reference)?;
    let kind = match m.get("kind") {
        Some(CValue::Str(k)) if k == "stepped" => Some(Kind::Stepped),
        Some(CValue::Str(k)) if k == "continuous" => Some(Kind::Continuous),
        _ => {
            schema(f, "kind must be \"stepped\" or \"continuous\"".to_string());
            None
        }
    };
    let allowed = |k: &str| KEYS_STEPPED.contains(&k) && (k != "centre_index" || kind != Some(Kind::Continuous));
    for k in m.keys().filter(|k| !allowed(k)) {
        schema(f, format!("unknown key {k:?}"));
    }
    for k in KEYS_STEPPED.iter().filter(|k| !m.contains_key(**k)) {
        if *k != "centre_index" || kind == Some(Kind::Stepped) {
            schema(f, format!("missing key {k:?}"));
        }
    }
    if m.get("id") != Some(&CValue::Str(id.to_string())) || m.get("version") != Some(&CValue::Num(major as f64)) {
        schema(f, format!("id and version must be {id:?} and {major}, as in the file name"));
    }
    let group = if id.starts_with("pal.step.") {
        Some(Kind::Stepped)
    } else if id.starts_with("pal.cont.") {
        Some(Kind::Continuous)
    } else {
        None
    };
    if kind.is_some() && kind != group {
        schema(f, "kind must be stepped for pal.step.* and continuous for pal.cont.*".to_string());
    }

    // Colours, and the digest over them as written.
    let strings = match m.get("colors") {
        Some(CValue::Arr(items)) if items.len() <= MAX_SAMPLES * 4 => {
            let s: Option<Vec<String>> =
                items.iter().map(|x| if let CValue::Str(s) = x { Some(s.clone()) } else { None }).collect();
            if s.is_none() {
                schema(f, "colors must be an array of \"#rrggbb\" strings".to_string());
            }
            s
        }
        _ => {
            schema(f, "colors must be an array of \"#rrggbb\" strings".to_string());
            None
        }
    };
    let side = |k: &str| match m.get(k) {
        Some(CValue::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let (under, over) = (side("under"), side("over"));
    let computed = match (&strings, &under, &over) {
        (Some(c), Some(u), Some(o)) => digest(c, u, o).ok(),
        _ => None,
    };
    match m.get("digest") {
        Some(CValue::Str(d)) if Some(d) == computed.as_ref() => {}
        Some(CValue::Str(d)) => schema(
            f,
            format!("digest {d} does not match the colours ({})", computed.as_deref().unwrap_or("not computable")),
        ),
        Some(_) => schema(f, "digest must be a sha256: string".to_string()),
        None => {}
    }
    let mut colours_ok = strings.is_some();
    let mut rgb = Vec::new();
    for s in strings.iter().flatten() {
        match parse_hex(s) {
            Some(c) if is_hex(s) => rgb.push(c),
            _ => colours_ok = false,
        }
    }
    let mut ends = [[0u8; 3]; 2];
    for (i, (k, val)) in [("under", &under), ("over", &over)].into_iter().enumerate() {
        match val.as_deref().filter(|s| is_hex(s)).and_then(parse_hex) {
            Some(c) => ends[i] = c,
            None => {
                colours_ok = false;
                if m.contains_key(k) {
                    schema(f, format!("{k} must be a lowercase \"#rrggbb\" string"));
                }
            }
        }
    }
    if strings.is_some() && rgb.len() != strings.as_ref().map_or(0, Vec::len) {
        schema(f, "every colour must be a lowercase \"#rrggbb\" string".to_string());
    }
    let n = strings.as_ref().map_or(0, Vec::len);
    r.palettes.insert(reference.to_string(), Summary { kind, count: n, digest: computed });

    // Counts and centre (REQ-002, REQ-003).
    match kind {
        Some(Kind::Stepped) => {
            if strings.is_some() && (n % 2 == 0 || !(MIN_STEPS..=MAX_STEPS).contains(&n)) {
                f.push(Finding::new(
                    STEPS,
                    reference,
                    format!("{n} steps; a stepped palette has an odd number of steps from {MIN_STEPS} to {MAX_STEPS}"),
                ));
            }
            match m.get("centre_index") {
                Some(CValue::Num(c)) if c.fract() == 0.0 && *c >= 0.0 && *c < n as f64 => {
                    if n % 2 == 1 && *c != ((n - 1) / 2) as f64 {
                        f.push(Finding::new(
                            CENTRE,
                            reference,
                            format!("centre_index is {c}; the middle of {n} steps is {}", (n - 1) / 2),
                        ));
                    }
                }
                Some(_) => schema(f, "centre_index must be the index of a step".to_string()),
                None => {}
            }
        }
        Some(Kind::Continuous) if strings.is_some() && !(MIN_SAMPLES..=MAX_SAMPLES).contains(&n) => {
            f.push(Finding::new(
                SAMPLES,
                reference,
                format!("{n} samples; a continuous palette has {MIN_SAMPLES} to {MAX_SAMPLES}"),
            ));
        }
        _ => {}
    }

    // Source and licence (REQ-003).
    match (m.get("source"), m.get("licence")) {
        (Some(CValue::Str(src)), Some(CValue::Str(lic))) => {
            let ok = !src.trim().is_empty()
                && (THIRD_PARTY_LICENCES.contains(&lic.as_str())
                    || (lic == PROJECT_LICENCE && src.starts_with(ORIGINAL) && src.len() > ORIGINAL.len()));
            if !ok {
                f.push(Finding::new(
                    LICENCE,
                    reference,
                    format!(
                        "licence {lic:?}: a palette from elsewhere must be {} with its source recorded; {PROJECT_LICENCE} is for palettes made here (source starting {ORIGINAL:?})",
                        THIRD_PARTY_LICENCES.join(" or ")
                    ),
                ));
            }
            // The `original:` claim and the review record must agree.
            let original = src.starts_with(ORIGINAL);
            match (&decl, original) {
                (None, true) => f.push(Finding::new(
                    LICENCE,
                    reference,
                    format!(
                        "source says {ORIGINAL:?} but there is no originality declaration {}{PAL_DIR}/{reference}.json{}",
                        crate::review::ORIGIN_DIR,
                        crate::review::ORIGIN_SUFFIX
                    ),
                )),
                (Some(Err(e)), _) => f.push(Finding::new(LICENCE, reference, format!("invalid originality declaration: {e}"))),
                (Some(Ok(())), false) => f.push(Finding::new(
                    LICENCE,
                    reference,
                    format!("an originality declaration exists but the source does not start {ORIGINAL:?}"),
                )),
                _ => {}
            }
        }
        (s, l) => {
            if s.is_some_and(|x| !matches!(x, CValue::Str(_))) || l.is_some_and(|x| !matches!(x, CValue::Str(_))) {
                schema(f, "source and licence must be strings".to_string());
            }
        }
    }

    match (kind, colours_ok && n > 0) {
        (Some(kind), true) => Some(Colours { kind, colors: rgb, under: ends[0], over: ends[1] }),
        _ => None,
    }
}

// ------------------------------------------------------------------ perceptual rules

fn label(i: Option<usize>, side: &str) -> String {
    i.map_or_else(|| side.to_string(), |i| i.to_string())
}

fn monotonic(ls: &[f64]) -> bool {
    let up = ls.windows(2).all(|w| w[1] > w[0]);
    let down = ls.windows(2).all(|w| w[1] < w[0]);
    up || down
}

/// The pairs `PAL-DELTA` compares, as `(a, b)` with `None` for `under` (as `a`) or `over` (as `b`).
pub fn delta_pairs(kind: Kind, n: usize) -> Vec<(Option<usize>, Option<usize>)> {
    let mut out = vec![(None, Some(0)), (Some(n.saturating_sub(1)), None)];
    if kind == Kind::Stepped {
        out.extend((0..n.saturating_sub(1)).map(|i| (Some(i), Some(i + 1))));
        if n % 2 == 1 {
            let c = (n - 1) / 2;
            out.extend((1..=c).map(|k| (Some(c - k), Some(c + k))));
        }
    }
    out
}

/// What the out-of-tolerance and claimed rules read in one theme.
struct ThemeEnv {
    conforms: Option<Rgb>,
    base: Option<Rgb>,
    neutral: Vec<(&'static str, Rgb)>,
    claimed: Vec<(String, Rgb)>,
}

/// CIEDE2000 (normal vision) between samples `i` and `i + WINDOW`, for every `i`.
pub fn window_differences(colors: &[Rgb]) -> Vec<f64> {
    let labs: Vec<[f64; 3]> =
        colors.iter().map(|c| lab_of_linear([linear(c[0]), linear(c[1]), linear(c[2])])).collect();
    labs.iter().zip(labs.iter().skip(WINDOW)).map(|(a, b)| crate::status::ciede2000::delta_e2000(*a, *b)).collect()
}

fn perceptual(pals: &BTreeMap<String, Colours>, tokens: &crate::tokens::Report, f: &mut Vec<Finding>) {
    // Per theme: CONFORMS, the base surface and the claimed roles.
    let mut env: BTreeMap<&str, ThemeEnv> = BTreeMap::new();
    for theme in THEMES {
        let values = tokens.themes.get(theme).map(|t| &t.values);
        let get = |n: &str| values.and_then(|v| v.get(n)).and_then(|x| parse_hex(&x.value));
        let conforms = get(CONFORMS_TOKEN);
        let base = get(SURFACE_TOKEN);
        let mut neutral = Vec::new();
        for n in std::iter::once(CONFORMS_TOKEN).chain(NEUTRAL_TOKENS) {
            match get(n) {
                Some(c) if n != CONFORMS_TOKEN => neutral.push((n, c)),
                Some(_) => {}
                None => f.push(Finding::new(
                    OOT,
                    format!("{theme} {n}"),
                    "does not resolve to an opaque colour; the out-of-tolerance distance cannot be verified",
                )),
            }
        }
        if base.is_none() {
            f.push(Finding::new(
                CONTRAST,
                format!("{theme} {SURFACE_TOKEN}"),
                "does not resolve to an opaque colour; the out-of-tolerance contrast cannot be verified",
            ));
        }
        let claimed = values
            .map(|v| {
                v.iter()
                    .filter(|(k, _)| is_claimed(k))
                    .filter_map(|(k, x)| parse_hex(&x.value).map(|c| (k.clone(), c)))
                    .collect()
            })
            .unwrap_or_default();
        env.insert(theme, ThemeEnv { conforms, base, neutral, claimed });
    }
    let inrange: Vec<(&str, usize, Rgb)> =
        pals.iter().flat_map(|(r, p)| p.colors.iter().enumerate().map(move |(i, c)| (r.as_str(), i, *c))).collect();

    for (reference, p) in pals {
        let n = p.colors.len();
        let col = |i: Option<usize>, end: Rgb| i.and_then(|i| p.colors.get(i).copied()).unwrap_or(end);

        // Lightness (REQ-UX.TOKENS-007) and the neutral centre (REQ-002).
        match p.kind {
            Kind::Stepped if n % 2 == 1 => {
                let c = (n - 1) / 2;
                let ls: Vec<f64> = p.colors.iter().map(|x| lstar(*x)).collect();
                let lower: Vec<f64> = ls[..=c].iter().rev().copied().collect();
                if !monotonic(&lower) || !monotonic(&ls[c..]) {
                    f.push(Finding::new(
                        LIGHTNESS,
                        reference,
                        "CIELAB L* is not strictly monotonic along each arm from the centre outwards",
                    ));
                }
                let ch = chroma(p.colors[c]);
                if ch > MAX_NEUTRAL_CHROMA {
                    f.push(Finding::new(
                        CENTRE,
                        reference,
                        format!(
                            "centre step has OKLab chroma {ch:.3}; a neutral centre has at most {MAX_NEUTRAL_CHROMA}"
                        ),
                    ));
                }
            }
            Kind::Continuous => {
                let ls: Vec<f64> = p.colors.iter().map(|x| lstar(*x)).collect();
                if !monotonic(&ls) {
                    f.push(Finding::new(LIGHTNESS, reference, "CIELAB L* is not strictly monotonic along the samples"));
                }
                let w = window_differences(&p.colors);
                let lo = w.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = w.iter().copied().fold(0.0, f64::max);
                if !w.is_empty() && (lo <= 0.0 || hi / lo > MAX_WINDOW_RATIO) {
                    f.push(Finding::new(
                        UNIFORM,
                        reference,
                        format!(
                            "CIEDE2000 over {WINDOW}-sample windows ranges from {lo:.2} to {hi:.2}; the largest may be at most {MAX_WINDOW_RATIO} times the smallest"
                        ),
                    ));
                }
            }
            Kind::Stepped => {}
        }

        // Step differences under the four vision conditions (REQ-UX.TOKENS-007).
        for (a, b) in delta_pairs(p.kind, n) {
            let (d, v) = min_delta_e2000(col(a, p.under), col(b, p.over));
            if d < MIN_STEP_DELTA_E2000 {
                f.push(Finding::new(
                    DELTA,
                    format!("{reference} {} {}", label(a, "under"), label(b, "over")),
                    format!(
                        "CIEDE2000 {d:.2} under {}; at least {MIN_STEP_DELTA_E2000} under every vision condition",
                        v.as_str()
                    ),
                ));
            }
        }

        // Out-of-range colours never look in range, in any catalogue palette.
        for (side, c) in [("under", p.under), ("over", p.over)] {
            let mut worst: Option<(f64, Vision, &str, usize)> = None;
            for (r, i, x) in &inrange {
                let (d, v) = min_delta_e2000(c, *x);
                if worst.map_or(true, |w| d < w.0) {
                    worst = Some((d, v, r, *i));
                }
            }
            if let Some((d, v, r, i)) = worst.filter(|w| w.0 < MIN_OOT_INRANGE_DELTA_E2000) {
                f.push(Finding::new(
                    OOT,
                    format!("{reference} {side}"),
                    format!(
                        "CIEDE2000 {d:.2} from colour {i} of {r} under {}; at least {MIN_OOT_INRANGE_DELTA_E2000} from every in-range colour",
                        v.as_str()
                    ),
                ));
            }
        }

        for (theme, env) in &env {
            let (conforms, base, claimed) = (&env.conforms, &env.base, &env.claimed);
            for (side, c) in [("under", p.under), ("over", p.over)] {
                for (n, x) in &env.neutral {
                    let (d, v) = min_delta_e2000(c, *x);
                    if d < MIN_OOT_NEUTRAL_DELTA_E2000 {
                        f.push(Finding::new(
                            OOT,
                            format!("{reference} {theme} {side}"),
                            format!(
                                "CIEDE2000 {d:.2} from {n} under {}; at least {MIN_OOT_NEUTRAL_DELTA_E2000}",
                                v.as_str()
                            ),
                        ));
                    }
                }
                // REQ-UX.TOKENS-009: out-of-tolerance colours keep away from CONFORMS.
                if let Some(cf) = conforms {
                    let (d, v) = min_delta_e2000(c, *cf);
                    if d < crate::status::MIN_DELTA_E2000 {
                        f.push(Finding::new(
                            OOT,
                            format!("{reference} {theme} {side}"),
                            format!(
                                "CIEDE2000 {d:.2} from {CONFORMS_TOKEN} under {}; at least {} under every vision condition",
                                v.as_str(),
                                crate::status::MIN_DELTA_E2000
                            ),
                        ));
                    }
                }
                // WCAG 2.2 SC 1.4.11 on the base surface.
                if let Some(b) = base {
                    let ratio = contrast_ratio(c, *b);
                    if ratio < MIN_OOT_CONTRAST {
                        f.push(Finding::new(
                            CONTRAST,
                            format!("{reference} {theme} {side}"),
                            format!("contrast {ratio:.2}:1 on {SURFACE_TOKEN}; at least {MIN_OOT_CONTRAST}:1"),
                        ));
                    }
                }
            }
            // Claimed values never look measured.
            let mut worst: Option<(f64, &str)> = None;
            for x in p.colors.iter().chain([&p.under, &p.over]) {
                for (name, c) in claimed {
                    let d = delta_e_ok(*x, *c);
                    if worst.map_or(true, |w| d < w.0) {
                        worst = Some((d, name));
                    }
                }
            }
            if let Some((d, name)) = worst.filter(|w| w.0 < crate::tokens::MIN_DELTA_E_OK) {
                f.push(Finding::new(
                    CLAIMED,
                    format!("{reference} {theme}"),
                    format!("a colour lies {d:.3} in OKLab from {name}; at least {:.2}", crate::tokens::MIN_DELTA_E_OK),
                ));
            }
        }
    }
}

// ------------------------------------------------------------------ report

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let pals = self
            .palettes
            .iter()
            .map(|(k, s)| {
                let mut o = BTreeMap::new();
                o.insert("ref".to_string(), CValue::Str(k.clone()));
                o.insert("kind".to_string(), s.kind.map_or(CValue::Null, |x| CValue::Str(x.as_str().to_string())));
                o.insert("count".to_string(), CValue::Num(s.count as f64));
                o.insert("digest".to_string(), s.digest.clone().map_or(CValue::Null, CValue::Str));
                o.insert("default".to_string(), CValue::Bool(self.default.as_deref() == Some(k.as_str())));
                CValue::Obj(o)
            })
            .collect();
        let mut counts = BTreeMap::new();
        counts.insert("palettes".to_string(), CValue::Num(self.palettes.len() as f64));
        let mut o = BTreeMap::new();
        o.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        o.insert("default".to_string(), self.default.clone().map_or(CValue::Null, CValue::Str));
        o.insert("palettes".to_string(), CValue::Arr(pals));
        o.insert("counts".to_string(), CValue::Obj(counts));
        o.insert("baseline".to_string(), CValue::Bool(self.baseline));
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
            "fid-design palettes-check: {} palettes, default {}\n",
            self.palettes.len(),
            self.default.as_deref().unwrap_or("(none)")
        );
        for (k, p) in &self.palettes {
            s.push_str(&format!(
                "{k}: {} {}, {}\n",
                p.kind.map_or("?", Kind::as_str),
                p.count,
                p.digest.as_deref().unwrap_or("no digest")
            ));
        }
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references() {
        assert_eq!(parse_ref("pal.step.cvd@1"), Some(("pal.step.cvd", 1)));
        assert_eq!(parse_ref("pal.cont.my-map@12"), Some(("pal.cont.my-map", 12)));
        for bad in [
            "pal.step.cvd",
            "pal.step.cvd@0",
            "pal.step.cvd@01",
            "pal.step.cvd@1.0",
            "pal.step@1",
            "pal.step.cvd.x@1",
            "pal.Step.cvd@1",
            "pal.step.-x@1",
            "tok.step.cvd@1",
            "pal.step.cvd@12345",
            "",
        ] {
            assert_eq!(parse_ref(bad), None, "{bad}");
        }
        for r in REQUIRED {
            assert!(parse_ref(r).is_some(), "{r}");
        }
    }

    #[test]
    fn claimed_names() {
        assert!(is_claimed("sem.text.value.claimed"));
        assert!(is_claimed("sem.surface.claimed-bg"));
        assert!(!is_claimed("comp.badge.claimed"));
        assert!(!is_claimed("sem.text.value.measured"));
        assert!(!is_claimed("sem.text.claimed.measured"));
    }

    #[test]
    fn pairs_of_a_nine_step_palette() {
        let p = delta_pairs(Kind::Stepped, 9);
        assert_eq!(p.len(), 2 + 8 + 4);
        assert!(p.contains(&(Some(3), Some(5))) && p.contains(&(Some(0), Some(8))));
        assert_eq!(delta_pairs(Kind::Continuous, 256), vec![(None, Some(0)), (Some(255), None)]);
        assert_eq!(delta_pairs(Kind::Stepped, 8).len(), 2 + 7);
    }

    #[test]
    fn lightness_and_chroma_anchors() {
        assert!((lstar([255, 255, 255]) - 100.0).abs() < 1e-3);
        assert!(lstar([0, 0, 0]).abs() < 1e-9);
        assert!(chroma([128, 128, 128]) < 1e-6);
        assert!(monotonic(&[1.0, 2.0, 3.0]) && monotonic(&[3.0, 2.0]) && !monotonic(&[1.0, 1.0]));
    }
}
