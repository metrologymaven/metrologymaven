//! `fid-design icons-prov --root <design dir> --attribution <file> [--json]` and
//! `fid-design icons-registry --manifest <file> --root <design dir> [--json]` (spec node
//! `UX.ICONS.PROV`, REQ-UX.ICONS.PROV-001 .. -004). Tooling, nearest class R0; D0 (sorted
//! walks, `BTreeMap`/`BTreeSet` only, no clock).
//!
//! # The provenance record of an icon
//!
//! Every `*.svg` under `<root>/icons/` and `<root>/gdt/` has two files:
//!
//! - its `originality@1` declaration (`UX.SYSTEM.REVIEW`), `<svg>.origin.json` beside it or
//!   `review/origin/<svg>.origin.json` (exactly one), checked with
//!   [`crate::review::check_decl`]: it carries the author, the date and the originality
//!   statement;
//! - its provenance file `<svg>.prov.json` (format `icon-provenance@1`), exactly the keys
//!   `format`, `artefact` (the SVG's path below the root), `licence` (an SPDX licence
//!   expression: identifiers or `LicenseRef-*` joined by `AND`, `OR` or `WITH`, no parentheses)
//!   and `third_party`: `false`, or `{ "source": <text>, "licence": <SPDX> }`.
//!
//! A missing declaration or provenance file is `PROV-MISSING`; an invalid one, both declaration
//! locations at once, a provenance file whose SVG does not exist, or a tree that cannot be fully
//! walked is `PROV-SCHEMA`. A `third_party` source must be the text of a Markdown heading
//! (`#` to `######`) of the given `ATTRIBUTION.md`, else `PROV-ATTRIBUTION`; a heading, not
//! any substring, so a short source cannot match by accident.
//!
//! # The icon index
//!
//! `<root>/icons/index.json` (owned by `UX.ICONS.CORESET`) is read when present: an object with
//! an `icons` array of objects. Only `name` (required, unique), `labelless` (optional boolean)
//! and `a11y_id` (optional string) are read here; other keys belong to CORESET. An entry with
//! `labelless = true` and no non-blank `a11y_id` is `PROV-A11Y-MISSING`. A malformed index is
//! `PROV-SCHEMA`. With no index file there are no index entries.
//!
//! # The registry check
//!
//! The manifest is CORE.REG.MAN's (`{ "commands": [{ "id", "major", .. }] }`, at most 32 MiB).
//! A command with an `icon` field whose name is not in the icon index is
//! `ICON-REGISTRY-MISSING <id>@<major> <icon>`. Commands without the field are listed as
//! `no_icon_field` (informational, not a finding) until CORE.REG adds it. A manifest that is not
//! that shape cannot be checked and is an error (exit 2).
//!
//! Untrusted text fails closed through [`crate::review::guard::value_defect`]. Exit codes: 0 no
//! findings, 1 findings, 2 unreadable input or bad usage.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::review::guard::{path_defect, value_defect};
use crate::review::{self, walk};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const MISSING: &str = "PROV-MISSING";
pub const SCHEMA: &str = "PROV-SCHEMA";
pub const ATTRIBUTION: &str = "PROV-ATTRIBUTION";
pub const A11Y_MISSING: &str = "PROV-A11Y-MISSING";
pub const REGISTRY_MISSING: &str = "ICON-REGISTRY-MISSING";

/// Report format IDs.
pub const PROV_REPORT_FORMAT: &str = "fid-design-icons-prov@1";
pub const REGISTRY_REPORT_FORMAT: &str = "fid-design-icons-registry@1";
/// Format ID of a provenance file.
pub const RECORD_FORMAT: &str = "icon-provenance@1";
/// Suffix of a provenance file.
pub const RECORD_SUFFIX: &str = ".prov.json";
/// The directories (below the root) whose SVGs need a record.
pub const ICON_DIRS: [&str; 2] = ["icons", "gdt"];
/// The icon index, below the root.
pub const INDEX_FILE: &str = "icons/index.json";
/// Largest provenance file or declaration read.
pub const MAX_RECORD_BYTES: u64 = 64 * 1024;
/// Largest icon index read.
pub const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;
/// Largest `ATTRIBUTION.md` read.
pub const MAX_ATTRIBUTION_BYTES: u64 = 1024 * 1024;
/// Largest registry manifest read (spec: 32 MiB).
pub const MAX_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;
/// Deepest JSON nesting accepted in any input.
pub const MAX_JSON_DEPTH: usize = 32;
/// Longest SPDX expression accepted.
pub const MAX_SPDX_BYTES: usize = 256;

const RECORD_KEYS: [&str; 4] = ["artefact", "format", "licence", "third_party"];
const THIRD_PARTY_KEYS: [&str; 2] = ["licence", "source"];

fn parse_json(text: &str, max: u64) -> Result<CValue, String> {
    let v = fid_canon::parse_with_limits(text.as_bytes(), max as usize, MAX_JSON_DEPTH)
        .map_err(|e| format!("not valid JSON: {}", e.code()))?;
    if let Some(d) = value_defect(&v) {
        return Err(format!("text refused at {d}"));
    }
    Ok(v)
}

/// Whether `s` is an SPDX licence expression of the accepted form (see the module docs).
pub fn spdx_ok(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_SPDX_BYTES || s != s.trim() {
        return false;
    }
    let toks: Vec<&str> = s.split(' ').collect();
    let id = |t: &str| {
        let b = t.as_bytes();
        !b.is_empty()
            && b[0].is_ascii_alphanumeric()
            && b.iter().all(|c| c.is_ascii_alphanumeric() || b".+-".contains(c))
            && !matches!(t, "AND" | "OR" | "WITH")
    };
    toks.len() % 2 == 1
        && toks.iter().enumerate().all(|(i, t)| if i % 2 == 0 { id(t) } else { matches!(*t, "AND" | "OR" | "WITH") })
}

fn text_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 1024
}

/// What a provenance file says about third-party origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThirdParty {
    No,
    Yes { source: String, licence: String },
}

/// A parsed provenance file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub licence: String,
    pub third_party: ThirdParty,
}

/// Validates an `icon-provenance@1` file for `artefact`; `Err` names the defect.
pub fn parse_record(text: &str, artefact: &str) -> Result<Record, String> {
    let v = parse_json(text, MAX_RECORD_BYTES)?;
    let CValue::Obj(m) = &v else {
        return Err(format!("{RECORD_FORMAT} must be a JSON object"));
    };
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    if keys != RECORD_KEYS {
        return Err(format!("{RECORD_FORMAT} must have exactly the keys {}", RECORD_KEYS.join(", ")));
    }
    match m.get("format") {
        Some(CValue::Str(f)) if f == RECORD_FORMAT => {}
        _ => return Err(format!("`format` must be \"{RECORD_FORMAT}\"")),
    }
    match m.get("artefact") {
        Some(CValue::Str(a)) if a == artefact => {}
        _ => return Err(format!("`artefact` must be {artefact:?}")),
    }
    let licence = match m.get("licence") {
        Some(CValue::Str(l)) if spdx_ok(l) => l.clone(),
        _ => return Err("`licence` must be an SPDX licence expression".to_string()),
    };
    let third_party = match m.get("third_party") {
        Some(CValue::Bool(false)) => ThirdParty::No,
        Some(CValue::Obj(t)) => {
            let keys: Vec<&str> = t.keys().map(String::as_str).collect();
            if keys != THIRD_PARTY_KEYS {
                return Err(format!("`third_party` must have exactly the keys {}", THIRD_PARTY_KEYS.join(", ")));
            }
            let source = match t.get("source") {
                Some(CValue::Str(s)) if text_ok(s) => s.trim().to_string(),
                _ => return Err("`third_party.source` must be non-blank text".to_string()),
            };
            let licence = match t.get("licence") {
                Some(CValue::Str(l)) if spdx_ok(l) => l.clone(),
                _ => return Err("`third_party.licence` must be an SPDX licence expression".to_string()),
            };
            ThirdParty::Yes { source, licence }
        }
        _ => return Err("`third_party` must be false or {source, licence}".to_string()),
    };
    Ok(Record { licence, third_party })
}

/// The Markdown heading texts of an `ATTRIBUTION.md` (`#` to `######`, then a space).
pub fn attribution_headings(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|l| {
            let hashes = l.len() - l.trim_start_matches('#').len();
            let rest = &l[hashes..];
            ((1..=6).contains(&hashes) && rest.starts_with(' ')).then(|| rest.trim().to_string())
        })
        .filter(|h| !h.is_empty())
        .collect()
}

/// One icon index entry, as far as this check reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub name: String,
    pub labelless: bool,
    pub a11y_id: Option<String>,
}

/// Parses the icon index; `Err` names the defect.
pub fn parse_index(text: &str) -> Result<Vec<IndexEntry>, String> {
    let v = parse_json(text, MAX_INDEX_BYTES)?;
    let CValue::Obj(m) = &v else {
        return Err("the icon index must be a JSON object".to_string());
    };
    let Some(CValue::Arr(icons)) = m.get("icons") else {
        return Err("the icon index must have an `icons` array".to_string());
    };
    let mut out = Vec::new();
    let mut names = BTreeSet::new();
    for (i, e) in icons.iter().enumerate() {
        let at = format!("icons[{i}]");
        let CValue::Obj(e) = e else {
            return Err(format!("{at} is not an object"));
        };
        let name = match e.get("name") {
            Some(CValue::Str(n)) if text_ok(n) && n == n.trim() => n.clone(),
            _ => return Err(format!("{at}: `name` must be non-blank text")),
        };
        if !names.insert(name.clone()) {
            return Err(format!("{at}: name `{name}` is listed twice"));
        }
        let labelless = match e.get("labelless") {
            None => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(format!("{at}: `labelless` must be a boolean")),
        };
        let a11y_id = match e.get("a11y_id") {
            None => None,
            Some(CValue::Str(s)) => (!s.trim().is_empty()).then(|| s.clone()),
            Some(_) => return Err(format!("{at}: `a11y_id` must be a string")),
        };
        out.push(IndexEntry { name, labelless, a11y_id });
    }
    Ok(out)
}

/// Reads the icon index under `root`: `Ok(None)` when there is none, `Ok(Some(Err))` when it
/// is malformed.
fn read_index(root: &Path) -> Result<Option<Result<Vec<IndexEntry>, String>>, String> {
    match review::kind(root, INDEX_FILE)? {
        review::Kind::Missing => Ok(None),
        review::Kind::File => Ok(Some(parse_index(&read_text(&root.join(INDEX_FILE), MAX_INDEX_BYTES)?))),
        _ => Ok(Some(Err("the icon index is not a regular file".to_string()))),
    }
}

/// Result of `icons-prov`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvReport {
    /// SVGs found, sorted.
    pub icons: Vec<String>,
    /// Index entries read.
    pub index_entries: usize,
    pub findings: Vec<Finding>,
}

/// Runs `icons-prov`. `Err` is an unreadable input (exit 2).
pub fn check_prov(root: &Path, attribution: &Path) -> Result<ProvReport, String> {
    let headings = attribution_headings(&read_text(attribution, MAX_ATTRIBUTION_BYTES)?);
    let mut f = Vec::new();
    let mut files = BTreeSet::new();
    for dir in ICON_DIRS {
        match review::kind(root, dir)? {
            review::Kind::Missing => continue,
            review::Kind::Dir => {}
            _ => {
                f.push(Finding::new(SCHEMA, dir, "not a directory; not read"));
                continue;
            }
        }
        let mut wf = Vec::new();
        for e in walk::walk(&root.join(dir), dir, &mut wf)? {
            if !e.is_dir {
                files.insert(e.rel);
            }
        }
        f.extend(wf.into_iter().map(|w| Finding::new(SCHEMA, w.subject, w.detail)));
    }
    let is_svg = |p: &str| p.to_ascii_lowercase().ends_with(".svg");
    let icons: Vec<String> = files.iter().filter(|p| is_svg(p)).cloned().collect();
    for icon in &icons {
        if let Some(d) = path_defect(icon) {
            f.push(Finding::new(SCHEMA, icon, format!("icon path refused: {d}")));
            continue;
        }
        check_icon(root, icon, &headings, &mut f)?;
    }
    for p in files.iter().filter(|p| p.ends_with(RECORD_SUFFIX)) {
        let svg = &p[..p.len() - RECORD_SUFFIX.len()];
        if !icons.iter().any(|i| i == svg) {
            f.push(Finding::new(SCHEMA, p, "provenance file for an icon that does not exist"));
        }
    }
    let mut index_entries = 0;
    match read_index(root)? {
        None => {}
        Some(Err(e)) => f.push(Finding::new(SCHEMA, INDEX_FILE, e)),
        Some(Ok(entries)) => {
            index_entries = entries.len();
            for e in entries.iter().filter(|e| e.labelless && e.a11y_id.is_none()) {
                f.push(Finding::new(A11Y_MISSING, &e.name, "labelless icon has no `a11y_id`"));
            }
        }
    }
    f.sort();
    f.dedup();
    Ok(ProvReport { icons, index_entries, findings: f })
}

fn check_icon(root: &Path, icon: &str, headings: &BTreeSet<String>, f: &mut Vec<Finding>) -> Result<(), String> {
    let beside = format!("{icon}{}", review::ORIGIN_SUFFIX);
    let central = format!("{}{icon}{}", review::ORIGIN_DIR, review::ORIGIN_SUFFIX);
    let found: Vec<&String> =
        [&beside, &central].into_iter().filter(|p| matches!(review::kind(root, p), Ok(review::Kind::File))).collect();
    match found.as_slice() {
        [] => f.push(Finding::new(MISSING, icon, format!("no originality declaration ({beside} or {central})"))),
        [one] => {
            let text = read_text(&root.join(one.as_str()), MAX_RECORD_BYTES)?;
            if let Err(e) = review::check_decl(&text, icon) {
                f.push(Finding::new(SCHEMA, icon, format!("{one}: {e}")));
            }
        }
        _ => f.push(Finding::new(SCHEMA, icon, format!("declared twice ({beside} and {central})"))),
    }
    let rec = format!("{icon}{RECORD_SUFFIX}");
    match review::kind(root, &rec)? {
        review::Kind::Missing => f.push(Finding::new(MISSING, icon, format!("no provenance file {rec}"))),
        review::Kind::File => {
            let text = read_text(&root.join(&rec), MAX_RECORD_BYTES)?;
            match parse_record(&text, icon) {
                Err(e) => f.push(Finding::new(SCHEMA, icon, format!("{rec}: {e}"))),
                Ok(Record { third_party: ThirdParty::Yes { source, .. }, .. }) if !headings.contains(&source) => {
                    f.push(Finding::new(
                        ATTRIBUTION,
                        icon,
                        format!("third-party source {source:?} is not a heading of the attribution file"),
                    ));
                }
                Ok(_) => {}
            }
        }
        _ => f.push(Finding::new(SCHEMA, icon, format!("{rec} is not a regular file"))),
    }
    Ok(())
}

impl ProvReport {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(PROV_REPORT_FORMAT.to_string()));
        m.insert("icons".to_string(), CValue::Arr(self.icons.iter().map(|p| CValue::Str(p.clone())).collect()));
        m.insert("index_entries".to_string(), CValue::U64(self.index_entries as u64));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s =
            format!("fid-design icons-prov: {} icon(s), {} index entries\n", self.icons.len(), self.index_entries);
        findings_text(&mut s, &self.findings);
        s
    }
}

/// Result of `icons-registry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryReport {
    /// Commands (`<id>@<major>`) that name an icon.
    pub with_icon: usize,
    /// Commands without an `icon` field (informational), sorted.
    pub no_icon_field: Vec<String>,
    pub findings: Vec<Finding>,
}

/// Runs `icons-registry`. `Err` is an unreadable or malformed manifest, or a malformed index
/// (exit 2): the check cannot be made.
pub fn check_registry(manifest: &Path, root: &Path) -> Result<RegistryReport, String> {
    let names: BTreeSet<String> = match read_index(root)? {
        None => BTreeSet::new(),
        Some(Err(e)) => return Err(format!("{INDEX_FILE}: {e}")),
        Some(Ok(entries)) => entries.into_iter().map(|e| e.name).collect(),
    };
    let text = read_text(manifest, MAX_MANIFEST_BYTES)?;
    let v = parse_json(&text, MAX_MANIFEST_BYTES).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let bad = |why: String| format!("{}: {why}", manifest.display());
    let CValue::Obj(m) = &v else {
        return Err(bad("the manifest must be a JSON object".into()));
    };
    let Some(CValue::Arr(commands)) = m.get("commands") else {
        return Err(bad("the manifest has no `commands` array".into()));
    };
    let mut f = Vec::new();
    let mut no_icon = BTreeSet::new();
    let mut with_icon = 0;
    for (i, c) in commands.iter().enumerate() {
        let CValue::Obj(c) = c else {
            return Err(bad(format!("commands[{i}] is not an object")));
        };
        let id = match c.get("id") {
            Some(CValue::Str(s)) if text_ok(s) => s,
            _ => return Err(bad(format!("commands[{i}]: `id` missing or not text"))),
        };
        let major = match c.get("major") {
            // JSON numbers parse as `Num`; a major is a whole number 1..=u32::MAX.
            Some(CValue::Num(n)) if n.fract() == 0.0 && *n >= 1.0 && *n <= f64::from(u32::MAX) => *n as u64,
            _ => return Err(bad(format!("commands[{i}]: `major` missing or not a positive integer"))),
        };
        let cmd = format!("{id}@{major}");
        match c.get("icon") {
            None => {
                no_icon.insert(cmd);
            }
            Some(CValue::Str(icon)) if text_ok(icon) => {
                with_icon += 1;
                if !names.contains(icon) {
                    f.push(Finding::new(REGISTRY_MISSING, format!("{cmd} {icon}"), "icon not in the icon index"));
                }
            }
            Some(_) => return Err(bad(format!("commands[{i}] ({cmd}): `icon` must be non-blank text"))),
        }
    }
    f.sort();
    f.dedup();
    Ok(RegistryReport { with_icon, no_icon_field: no_icon.into_iter().collect(), findings: f })
}

impl RegistryReport {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(REGISTRY_REPORT_FORMAT.to_string()));
        m.insert("with_icon".to_string(), CValue::U64(self.with_icon as u64));
        m.insert(
            "no_icon_field".to_string(),
            CValue::Arr(self.no_icon_field.iter().map(|c| CValue::Str(c.clone())).collect()),
        );
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
            "fid-design icons-registry: {} command(s) name an icon; no_icon_field: {}\n",
            self.with_icon,
            if self.no_icon_field.is_empty() { "none".to_string() } else { self.no_icon_field.join(", ") }
        );
        findings_text(&mut s, &self.findings);
        s
    }
}
