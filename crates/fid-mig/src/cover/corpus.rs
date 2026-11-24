//! Migration corpus manifests `mig-corpus@1` and the corpus check run by
//! `cargo xtask mig corpus-check` (spec leaf `MIG.COVER.CORPUS`).
//!
//! A corpus manifest lists the files of one migration format that the producing nodes are run
//! over. The check makes sure every file is admitted in the project dataset catalogue
//! (`datasets.toml`, spec node `ECO.DATA.CAT`) with the same SHA-256, that no `public` entry
//! is a file the project may not redistribute, and that the format is admitted in the
//! incumbent source register (`mig-sources@1`, [`crate::src::reg`]).
//!
//! # Rules pinned here
//!
//! - **Manifest.** `{schema, format_id, name, entries}`; each entry
//!   `{catalogue_id, sha256, format_id, visibility, note?}`. `catalogue_id` follows the
//!   catalogue's ID rule `^[a-z0-9][a-z0-9._-]{2,63}$`; `sha256` is the `sha256:<64 lowercase
//!   hex>` digest form (`fid-types`); format IDs follow the format registry's ID rule
//!   (IO.REG.DETECT `FormatId`, so `native:` formats are refused), and every entry's format
//!   must equal the manifest's, one manifest per format; `visibility` is `public` or
//!   `private`; `name` and `note` are plain text without markup, links or invisible
//!   characters. Unknown keys, wrong types, numbers anywhere and malformed values are
//!   `CORPUS-SCHEMA`; duplicate JSON keys are refused by the parser. A manifest that fails the
//!   schema is not checked further.
//! - **Catalogue.** Read with the `toml` crate (duplicate keys refused). Only the fields this
//!   check needs are read from each `[[dataset]]`: `id`, `sha256` (64 lowercase hex),
//!   `redistribution` (`mirror` or `fetch-only`) and the optional boolean
//!   `contains_customer_data`; a malformed one of those, or a repeated ID, fails the whole
//!   catalogue (`CORPUS-SCHEMA catalogue`, fail closed). The catalogue's own admission rules
//!   (licences, owners, standards text) are `datasets-check` (ECO.DATA.CAT).
//! - **Per entry**, in manifest order: a repeated catalogue ID is `CORPUS-DUP`; an ID absent
//!   from the catalogue is `CORPUS-UNCATALOGUED`; a SHA-256 that differs from the catalogue
//!   entry is `CORPUS-DIGEST`; a `public` entry whose catalogue entry is `fetch-only` or
//!   declares customer data is `CORPUS-VISIBILITY` (REQ-003; public needs `mirror` and no
//!   customer data).
//! - **Source** (REQ-004). The manifest's format is admitted when the register has an entry
//!   with that format whose status is `ADMITTED` or `OPEN_EXPORT_ONLY` and which passes the
//!   register's review, attestation and policy rules (`SRC-REVIEW`, `SRC-ATTEST`,
//!   `SRC-POLICY`); otherwise `CORPUS-SOURCE <format>`. A register that does not load is
//!   `CORPUS-SCHEMA sources`.
//! - **Registered formats** (REQ-001). With a registry manifest, the format must be in its
//!   format catalogue ([`crate::src::reg::parse_manifest`]), else
//!   `CORPUS-SCHEMA <format>`. Without one, only the ID syntax is checked.
//! - **Digest** (REQ-005). SHA-256 of the RFC 8785 canonical JSON (`fid-canon`) of the
//!   manifest as written, with `entries` sorted by catalogue ID. Entry order in the file
//!   does not change it.
//! - **Private run** (REQ-006). `private_run` is true when any entry is `private`.
//! - **Output.** Findings sorted by subject (entry ID, format ID or input name), then code.
//!
//! Determinism D0: `BTreeMap`/`BTreeSet` only, no clock, no environment.

use crate::src::reg::{self, FormatCatalogue, FormatRef, SourceRegister, Status};
use fid_canon::CValue;
use fid_io_reg::FormatId;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::Path;

/// The schema tag of a corpus manifest.
pub const SCHEMA: &str = "mig-corpus@1";
/// Largest accepted manifest, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
/// Largest accepted dataset catalogue, in bytes (the ECO.DATA.CAT input limit).
pub const MAX_CATALOGUE_BYTES: usize = 1024 * 1024;
/// Largest accepted source register, in bytes.
pub const MAX_SOURCES_BYTES: usize = reg::MAX_REGISTER_BYTES;
/// Deepest accepted JSON nesting in a manifest.
pub const MAX_DEPTH: usize = 4;
/// Most entries in one manifest.
pub const MAX_ENTRIES: usize = 10_000;
/// Longest manifest name, in characters.
pub const MAX_NAME: usize = 64;
/// Longest entry note, in characters.
pub const MAX_NOTE: usize = 1000;

const TOP_KEYS: [&str; 4] = ["entries", "format_id", "name", "schema"];
const ENTRY_REQUIRED: [&str; 4] = ["catalogue_id", "format_id", "sha256", "visibility"];
const ENTRY_OPTIONAL: [&str; 1] = ["note"];

/// Subject of findings about the dataset catalogue as a whole.
pub const CATALOGUE_SUBJECT: &str = "catalogue";
/// Subject of findings about the source register as a whole.
pub const SOURCES_SUBJECT: &str = "sources";
/// Subject of findings about the manifest as a whole.
pub const MANIFEST_SUBJECT: &str = "manifest";

/// Who may see a corpus file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Visibility {
    Public,
    Private,
}

impl Visibility {
    /// Parses the wire form.
    pub fn parse(s: &str) -> Option<Visibility> {
        match s {
            "public" => Some(Visibility::Public),
            "private" => Some(Visibility::Private),
            _ => None,
        }
    }

    /// The wire form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
        }
    }
}

/// One corpus file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusEntry {
    pub catalogue_id: String,
    pub sha256: Digest,
    pub format_id: FormatId,
    pub visibility: Visibility,
    pub note: Option<String>,
}

/// A schema-valid `mig-corpus@1` manifest with its corpus digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusManifest {
    pub format_id: FormatId,
    pub name: String,
    /// In file order.
    pub entries: Vec<CorpusEntry>,
    /// SHA-256 of the canonical JSON with entries sorted by catalogue ID (REQ-005).
    pub digest: Digest,
}

impl CorpusManifest {
    /// Whether any entry is private (REQ-006).
    pub fn private_run(&self) -> bool {
        self.entries.iter().any(|e| e.visibility == Visibility::Private)
    }
}

/// Whether a catalogued file may be mirrored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redistribution {
    Mirror,
    FetchOnly,
}

/// The fields of one catalogue entry this check needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueRecord {
    pub sha256: Digest,
    pub redistribution: Redistribution,
    pub contains_customer_data: bool,
}

/// The dataset catalogue, keyed by dataset ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatasetCatalogue {
    records: BTreeMap<String, CatalogueRecord>,
}

impl DatasetCatalogue {
    /// The record of a dataset ID.
    pub fn get(&self, id: &str) -> Option<&CatalogueRecord> {
        self.records.get(id)
    }

    /// Number of datasets.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the catalogue lists no dataset.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// The stable finding codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    Schema,
    Dup,
    Uncatalogued,
    Digest,
    Visibility,
    Source,
}

impl Code {
    /// The wire form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::Schema => "CORPUS-SCHEMA",
            Code::Dup => "CORPUS-DUP",
            Code::Uncatalogued => "CORPUS-UNCATALOGUED",
            Code::Digest => "CORPUS-DIGEST",
            Code::Visibility => "CORPUS-VISIBILITY",
            Code::Source => "CORPUS-SOURCE",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A corpus-check finding, displayed as `<code> <subject>[ <detail>]`. Ordered by subject,
/// then code, then detail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// The entry's catalogue ID, the format ID, or `manifest`, `catalogue` or `sources`.
    pub subject: String,
    pub code: Code,
    pub detail: String,
}

impl Finding {
    fn new(code: Code, subject: impl Into<String>, detail: impl Into<String>) -> Finding {
        Finding { subject: subject.into(), code, detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code, self.subject)?;
        if !self.detail.is_empty() {
            write!(f, " {}", self.detail)?;
        }
        Ok(())
    }
}

/// The result of [`check`]: the manifest when it loaded, and every finding, sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusReport {
    pub manifest: Option<CorpusManifest>,
    pub findings: Vec<Finding>,
}

impl CorpusReport {
    /// True when the manifest loaded and there are no findings.
    pub fn is_ok(&self) -> bool {
        self.manifest.is_some() && self.findings.is_empty()
    }

    /// The corpus digest, when the check passed.
    pub fn digest(&self) -> Option<&Digest> {
        self.manifest.as_ref().filter(|_| self.findings.is_empty()).map(|m| &m.digest)
    }

    fn failed(findings: Vec<Finding>) -> CorpusReport {
        CorpusReport { manifest: None, findings: sorted(findings) }
    }
}

fn sorted(mut v: Vec<Finding>) -> Vec<Finding> {
    v.sort();
    v.dedup();
    v
}

// ------------------------------------------------------------------ manifest

/// The dataset catalogue's ID rule: `^[a-z0-9][a-z0-9._-]{2,63}$`.
pub fn is_catalogue_id(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=64).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
}

fn keys_ok(
    o: &BTreeMap<String, CValue>,
    required: &[&str],
    optional: &[&str],
    at: &str,
    out: &mut Vec<Finding>,
) -> bool {
    let mut ok = true;
    for k in required {
        if !o.contains_key(*k) {
            out.push(Finding::new(Code::Schema, at, format!("missing field {k}")));
            ok = false;
        }
    }
    for k in o.keys() {
        if !required.contains(&k.as_str()) && !optional.contains(&k.as_str()) {
            out.push(Finding::new(Code::Schema, at, format!("unknown field {k}")));
            ok = false;
        }
    }
    ok
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Option<&'a str> {
    match o.get(key) {
        Some(CValue::Str(s)) => Some(s),
        _ => None,
    }
}

fn entry(v: &CValue, index: usize, manifest_format: Option<&FormatId>, out: &mut Vec<Finding>) -> Option<CorpusEntry> {
    let at = format!("entries[{index}]");
    let CValue::Obj(o) = v else {
        out.push(Finding::new(Code::Schema, MANIFEST_SUBJECT, format!("{at}: must be an object")));
        return None;
    };
    // Findings about an entry are filed under its catalogue ID when it has a usable one.
    let subject = match str_field(o, "catalogue_id") {
        Some(id) if is_catalogue_id(id) => id.to_string(),
        _ => format!("{MANIFEST_SUBJECT} {at}"),
    };
    if !keys_ok(o, &ENTRY_REQUIRED, &ENTRY_OPTIONAL, &subject, out) {
        return None;
    }
    let before = out.len();
    let mut bad = |what: &str| out.push(Finding::new(Code::Schema, subject.as_str(), what.to_string()));
    let catalogue_id = str_field(o, "catalogue_id").filter(|s| is_catalogue_id(s));
    if catalogue_id.is_none() {
        bad("catalogue_id: must match ^[a-z0-9][a-z0-9._-]{2,63}$");
    }
    let sha256 = str_field(o, "sha256").and_then(|s| Digest::parse(s).ok());
    if sha256.is_none() {
        bad("sha256: must be sha256:<64 lowercase hex>");
    }
    let format_id = str_field(o, "format_id").and_then(|s| FormatId::new(s).ok());
    match (&format_id, manifest_format) {
        (None, _) => bad("format_id: must be a format-registry ID"),
        (Some(f), Some(m)) if f != m => bad("format_id: must equal the manifest format_id"),
        _ => {}
    }
    let visibility = str_field(o, "visibility").and_then(Visibility::parse);
    if visibility.is_none() {
        bad("visibility: must be public or private");
    }
    let note = match o.get("note") {
        None => None,
        Some(CValue::Str(s)) if reg::is_text(s, MAX_NOTE, false) => Some(s.clone()),
        Some(_) => {
            bad("note: must be 1 to 1000 characters of plain text (no markup, links or invisible characters)");
            None
        }
    };
    if out.len() != before {
        return None;
    }
    Some(CorpusEntry {
        catalogue_id: catalogue_id?.to_string(),
        sha256: sha256?,
        format_id: format_id?,
        visibility: visibility?,
        note,
    })
}

/// The corpus digest of a parsed manifest: SHA-256 of the RFC 8785 canonical JSON with
/// `entries` sorted by catalogue ID (REQ-005).
fn corpus_digest(value: &CValue) -> Result<Digest, String> {
    let mut v = value.clone();
    if let CValue::Obj(top) = &mut v {
        if let Some(CValue::Arr(items)) = top.get_mut("entries") {
            let key = |e: &CValue| match e {
                CValue::Obj(o) => str_field(o, "catalogue_id").map(str::to_string),
                _ => None,
            };
            items.sort_by_key(key);
        }
    }
    let canonical = fid_canon::to_canonical_bytes(&v).map_err(|e| format!("no canonical form ({e})"))?;
    Ok(Digest::from_bytes(fid_canon::digest_bytes(&canonical).0))
}

/// Parses and schema-checks a `mig-corpus@1` manifest. On failure every `CORPUS-SCHEMA`
/// finding is returned.
pub fn load(bytes: &[u8]) -> Result<CorpusManifest, Vec<Finding>> {
    let schema = |d: String| vec![Finding::new(Code::Schema, MANIFEST_SUBJECT, d)];
    let value = fid_canon::parse_with_limits(bytes, MAX_MANIFEST_BYTES, MAX_DEPTH)
        .map_err(|e| schema(format!("not valid JSON within limits ({e})")))?;
    let CValue::Obj(top) = &value else { return Err(schema("top level must be an object".into())) };
    let mut out = Vec::new();
    if !keys_ok(top, &TOP_KEYS, &[], MANIFEST_SUBJECT, &mut out) {
        return Err(out);
    }
    if str_field(top, "schema") != Some(SCHEMA) {
        out.push(Finding::new(Code::Schema, MANIFEST_SUBJECT, format!("schema: must be {SCHEMA}")));
    }
    let format_id = str_field(top, "format_id").and_then(|s| FormatId::new(s).ok());
    if format_id.is_none() {
        out.push(Finding::new(Code::Schema, MANIFEST_SUBJECT, "format_id: must be a format-registry ID"));
    }
    let name = str_field(top, "name").filter(|s| reg::is_text(s, MAX_NAME, false));
    if name.is_none() {
        out.push(Finding::new(
            Code::Schema,
            MANIFEST_SUBJECT,
            "name: must be 1 to 64 characters of plain text (no markup, links or invisible characters)",
        ));
    }
    let mut entries = Vec::new();
    match top.get("entries") {
        Some(CValue::Arr(items)) if items.len() <= MAX_ENTRIES => {
            for (i, v) in items.iter().enumerate() {
                if let Some(e) = entry(v, i, format_id.as_ref(), &mut out) {
                    entries.push(e);
                }
            }
        }
        _ => out.push(Finding::new(
            Code::Schema,
            MANIFEST_SUBJECT,
            format!("entries: must be an array of at most {MAX_ENTRIES} entries"),
        )),
    }
    if !out.is_empty() {
        return Err(sorted(out));
    }
    let digest = corpus_digest(&value).map_err(schema)?;
    match (format_id, name) {
        (Some(format_id), Some(name)) => Ok(CorpusManifest { format_id, name: name.to_string(), entries, digest }),
        _ => Err(schema("incomplete manifest".into())),
    }
}

// ------------------------------------------------------------------ catalogue

/// Reads the fields this check needs from dataset catalogue text (`datasets.toml`, ECO.DATA.CAT).
/// Fails closed: any malformed needed field or a repeated ID fails the whole catalogue.
pub fn parse_catalogue(text: &str) -> Result<DatasetCatalogue, String> {
    if text.len() > MAX_CATALOGUE_BYTES {
        return Err(format!("larger than {MAX_CATALOGUE_BYTES} bytes"));
    }
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("not TOML: {}", e.message()))?;
    let rows = match table.get("dataset") {
        None => return Ok(DatasetCatalogue::default()),
        Some(toml::Value::Array(rows)) => rows,
        Some(_) => return Err("dataset must be an array of tables".to_string()),
    };
    let mut records = BTreeMap::new();
    for (i, row) in rows.iter().enumerate() {
        let toml::Value::Table(t) = row else { return Err(format!("dataset[{i}]: must be a table")) };
        let s = |key: &str| match t.get(key) {
            Some(toml::Value::String(v)) => Some(v.as_str()),
            _ => None,
        };
        let id =
            s("id").filter(|v| is_catalogue_id(v)).ok_or_else(|| format!("dataset[{i}]: missing or invalid id"))?;
        let sha256 = s("sha256")
            .and_then(|hex| Digest::parse(&format!("sha256:{hex}")).ok())
            .ok_or_else(|| format!("{id}: missing or invalid sha256"))?;
        let redistribution = match s("redistribution") {
            Some("mirror") => Redistribution::Mirror,
            Some("fetch-only") => Redistribution::FetchOnly,
            _ => return Err(format!("{id}: redistribution must be mirror or fetch-only")),
        };
        let contains_customer_data = match t.get("contains_customer_data") {
            None => false,
            Some(toml::Value::Boolean(b)) => *b,
            Some(_) => return Err(format!("{id}: contains_customer_data must be a boolean")),
        };
        let rec = CatalogueRecord { sha256, redistribution, contains_customer_data };
        if records.insert(id.to_string(), rec).is_some() {
            return Err(format!("{id}: id used twice"));
        }
    }
    Ok(DatasetCatalogue { records })
}

// ------------------------------------------------------------------ check

/// Per-entry catalogue checks (REQ-002, REQ-003) and duplicate entries.
pub fn check_entries(manifest: &CorpusManifest, catalogue: &DatasetCatalogue) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for e in &manifest.entries {
        let id = e.catalogue_id.as_str();
        if !seen.insert(id) {
            out.push(Finding::new(Code::Dup, id, "catalogue ID listed twice"));
            continue;
        }
        let Some(rec) = catalogue.get(id) else {
            out.push(Finding::new(Code::Uncatalogued, id, ""));
            continue;
        };
        if rec.sha256 != e.sha256 {
            out.push(Finding::new(
                Code::Digest,
                id,
                format!("manifest {} catalogue {}", e.sha256.as_str(), rec.sha256.as_str()),
            ));
        }
        if e.visibility == Visibility::Public {
            if rec.redistribution == Redistribution::FetchOnly {
                out.push(Finding::new(Code::Visibility, id, "public entry whose catalogue entry is fetch-only"));
            }
            if rec.contains_customer_data {
                out.push(Finding::new(
                    Code::Visibility,
                    id,
                    "public entry whose catalogue entry declares customer data",
                ));
            }
        }
    }
    out
}

/// REQ-004: the manifest's format needs a register entry with status `ADMITTED` or
/// `OPEN_EXPORT_ONLY` that passes the register's review, attestation and policy rules.
pub fn check_source(format: &FormatId, register: &SourceRegister) -> Vec<Finding> {
    let mut refused: BTreeSet<String> = BTreeSet::new();
    for f in reg::check_reviews(register).into_iter().chain(reg::check_policy(register)) {
        match f {
            reg::Finding::Review { id, .. } | reg::Finding::Attest { id } | reg::Finding::Policy { id, .. } => {
                refused.insert(id);
            }
            _ => {}
        }
    }
    let wanted = FormatRef::Registered(format.clone());
    let matching: Vec<&reg::SourceEntry> = register.entries().iter().filter(|e| e.format == wanted).collect();
    let admitted = matching
        .iter()
        .any(|e| matches!(e.status, Status::Admitted | Status::OpenExportOnly) && !refused.contains(&e.id));
    if admitted {
        return Vec::new();
    }
    let detail = if matching.is_empty() {
        "no source register entry".to_string()
    } else {
        let list: Vec<String> = matching
            .iter()
            .map(|e| {
                let why = if refused.contains(&e.id) { " fails review, attestation or policy" } else { "" };
                format!("{} {}{why}", e.id, e.status)
            })
            .collect();
        format!("not admitted: {}", list.join(", "))
    };
    vec![Finding::new(Code::Source, format.as_str(), detail)]
}

/// The whole corpus check: the manifest, the dataset catalogue text, the source register and,
/// optionally, a registry manifest whose format catalogue the format must be in.
pub fn check(manifest: &[u8], catalogue: &str, sources: &[u8], registry_manifest: Option<&[u8]>) -> CorpusReport {
    let mut findings = Vec::new();
    let loaded = load(manifest).map_err(|f| findings.extend(f)).ok();
    let cat =
        parse_catalogue(catalogue).map_err(|e| findings.push(Finding::new(Code::Schema, CATALOGUE_SUBJECT, e))).ok();
    let register = reg::load(sources)
        .map_err(|fs| {
            for f in fs.iter().take(3) {
                findings.push(Finding::new(Code::Schema, SOURCES_SUBJECT, f.to_string()));
            }
        })
        .ok();
    let formats: Option<FormatCatalogue> = match registry_manifest.map(reg::parse_manifest) {
        None => None,
        Some(Ok(f)) => Some(f),
        Some(Err(f)) => {
            findings.push(Finding::new(Code::Schema, "registry-manifest", f.to_string()));
            None
        }
    };
    let (Some(m), Some(cat), Some(register)) = (loaded, cat, register) else {
        return CorpusReport::failed(findings);
    };
    if !findings.is_empty() {
        return CorpusReport::failed(findings);
    }
    if let Some(formats) = &formats {
        if !formats.contains(&m.format_id) {
            findings.push(Finding::new(Code::Schema, m.format_id.as_str(), "not a registered format"));
            return CorpusReport::failed(findings);
        }
    }
    findings.extend(check_entries(&m, &cat));
    findings.extend(check_source(&m.format_id, &register));
    CorpusReport { manifest: Some(m), findings: sorted(findings) }
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = Vec::new();
    // One byte over the limit is enough for the parsers to report the input as too large.
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    f.take(cap).read_to_end(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(buf)
}

/// [`check`] over files. `Err` only when a file cannot be read; a catalogue that is not
/// UTF-8 is a `CORPUS-SCHEMA catalogue` finding.
pub fn check_paths(
    manifest: &Path,
    catalogue: &Path,
    sources: &Path,
    registry_manifest: Option<&Path>,
) -> Result<CorpusReport, String> {
    let m = read_bounded(manifest, MAX_MANIFEST_BYTES)?;
    let c = read_bounded(catalogue, MAX_CATALOGUE_BYTES)?;
    let s = read_bounded(sources, MAX_SOURCES_BYTES)?;
    let r = match registry_manifest {
        Some(p) => Some(read_bounded(p, reg::MAX_MANIFEST_BYTES)?),
        None => None,
    };
    let c = match String::from_utf8(c) {
        Ok(text) => text,
        Err(_) => {
            return Ok(CorpusReport::failed(vec![Finding::new(Code::Schema, CATALOGUE_SUBJECT, "not UTF-8")]));
        }
    };
    Ok(check(&m, &c, &s, r.as_deref()))
}
