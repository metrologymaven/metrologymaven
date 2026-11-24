//! The incumbent source register `mig-sources@1`, its review records `mig-review@1`, the
//! sources check and the listing `migrate.list_sources@1` with the generated sources page
//! (spec leaf `MIG.SRC.REG`).
//!
//! The register names every incumbent artefact kind MIG may be asked to read, its access
//! route and admission status, and for non-admitted artefacts the open format to export to.
//! It ships embedded in the build ([`EMBEDDED`], `data/mig/sources.json`), so readers need no
//! file I/O.
//!
//! # Rules pinned here
//!
//! - **File.** `{schema, version, policy, entries}`; each entry
//!   `{id, product, kind, format_id, versions, access_route, status, export_route?, review?}`;
//!   `export_route` is `{format, note}` with `format` an open (registered) format ID or `none`;
//!   `review` is the entry's `mig-review@1` record, inline, so a reference cannot dangle.
//!   The loader checks format IDs for syntax only; [`check`] then requires every non-native
//!   `format_id` and export-route format to be registered in the format catalogue of a
//!   registry manifest ([`parse_manifest`]). The workspace manifest carries no format
//!   catalogue yet, so today every non-native format is refused (fail closed).
//!   Unknown keys, wrong types and malformed values are `SRC-SCHEMA`; duplicate JSON keys are
//!   refused by the parser.
//! - **Text is untrusted.** Every free-text field is printable text without control, line or
//!   paragraph separator, invisible-operator, bidi-mark or bidi-override characters, without
//!   the Markdown and HTML metacharacters `| < > \` * _ [ ] # \\ ~ { } $`, and without `://`,
//!   `www.` or `@`, so the page can be rendered without escaping and never carries markup or a
//!   link from the register.
//! - **Versions.** `<lo>..<hi>`, each end `*` (open) or a dotted numeric version of 1 to 4
//!   components; both ends inclusive, missing components count as 0; `lo <= hi`. So `3..5.2`
//!   holds 5.2 (= 5.2.0.0) but not 5.2.1: write `3..5.3` or `3..*` to include later patches.
//! - **Check order** (the leaf's algorithm): schema, then `SRC-DUP` (same product compared
//!   ASCII-case-insensitively, same kind and format ID, overlapping versions), then
//!   `SRC-REVIEW` and `SRC-ATTEST` per entry in ID order, then `SRC-POLICY`. A register that
//!   fails the schema is not checked further.
//! - **Review** (REQ-003, REQ-005). `ADMITTED` and `OPEN_EXPORT_ONLY` need a review record
//!   with non-empty basis, reviewer, a real `YYYY-MM-DD` date and clean-room statement, whose
//!   `entry` names the entry; under policy `D-033-B` an admitted native format also needs
//!   `counsel_ref`. Every review record, on any entry, needs
//!   `no_copied_text_attestation: true`, else `SRC-ATTEST`.
//! - **Policy** (REQ-004). Under `D-033-A`, an `ADMITTED` entry whose format is `native:<slug>`
//!   is `SRC-POLICY`.
//! - **Listing** (REQ-006, REQ-007). Entries sorted by ID, filtered by kind and status; the
//!   Markdown page is rendered only from the listing and is byte-identical for identical data.
//!
//! Determinism D0: `BTreeMap`/`BTreeSet` only, no clock, no environment.

use crate::ladder::def::SemVer;
use fid_canon::CValue;
use fid_io_reg::FormatId;
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;

/// The schema tag of the register.
pub const SCHEMA: &str = "mig-sources@1";
/// The schema tag of a review record.
pub const REVIEW_SCHEMA: &str = "mig-review@1";
/// The listing command.
pub const LIST_SOURCES: &str = "migrate.list_sources@1";
const NAME: &str = "migrate.list_sources";
const LIFECYCLE_SINCE: &str = "2026-11-21";
/// Largest accepted register file, in bytes.
pub const MAX_REGISTER_BYTES: usize = 1024 * 1024;
/// Deepest accepted nesting.
pub const MAX_DEPTH: usize = 8;
/// Most entries in one register.
pub const MAX_ENTRIES: usize = 1024;
/// Longest ID, product name, reviewer or reference.
pub const MAX_NAME: usize = 64;
/// Longest free-text field (note, basis, statements).
pub const MAX_TEXT: usize = 1000;
/// The production register, embedded at build time. Its content needs human and, per D-033,
/// counsel review before release.
pub const EMBEDDED: &[u8] = include_bytes!("../../../../data/mig/sources.json");

const TOP_KEYS: [&str; 4] = ["entries", "policy", "schema", "version"];
const ENTRY_REQUIRED: [&str; 7] = ["access_route", "format_id", "id", "kind", "product", "status", "versions"];
const ENTRY_OPTIONAL: [&str; 2] = ["export_route", "review"];
const ROUTE_KEYS: [&str; 2] = ["format", "note"];
const REVIEW_KEYS: [&str; 9] = [
    "basis",
    "clean_room_statement",
    "counsel_ref",
    "date",
    "entry",
    "eula_note",
    "no_copied_text_attestation",
    "reviewer",
    "schema",
];
const FORBIDDEN: [char; 15] = ['|', '<', '>', '`', '*', '_', '[', ']', '#', '\\', '~', '{', '}', '$', '\u{7f}'];

macro_rules! wire_enum {
    ($(#[$doc:meta])* $name:ident { $($v:ident => $w:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($v),+ }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$v),+];

            /// The wire form.
            pub const fn as_str(self) -> &'static str {
                match self { $($name::$v => $w),+ }
            }

            /// Parses the exact wire form.
            pub fn parse(s: &str) -> Option<$name> {
                match s { $($w => Some($name::$v),)+ _ => None }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

wire_enum!(
    /// The register policy bound to D-033.
    Policy { D033A => "D-033-A", D033B => "D-033-B" }
);
wire_enum!(
    /// The artefact kind of an entry.
    ArtefactKind { Results => "results", Points => "points", Project => "project", Form => "form", Program => "program" }
);
wire_enum!(
    /// How the artefact can be read.
    AccessRoute { PublicSpec => "PUBLIC_SPEC", OpenExport => "OPEN_EXPORT", NoAccess => "NONE" }
);
wire_enum!(
    /// The admission status of an entry.
    Status {
        Admitted => "ADMITTED",
        OpenExportOnly => "OPEN_EXPORT_ONLY",
        NotAdmitted => "NOT_ADMITTED",
        PendingReview => "PENDING_REVIEW",
    }
);
wire_enum!(
    /// The listing output format.
    OutputFormat { Json => "json", Md => "md" }
);

/// The format of an entry: a format-registry ID (an open, registered format) or
/// `native:<slug>` for an unregistered incumbent-native format.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormatRef {
    Registered(FormatId),
    Native(String),
}

impl FormatRef {
    fn parse(s: &str) -> Option<FormatRef> {
        match s.strip_prefix("native:") {
            Some(slug) => is_slug(slug).then(|| FormatRef::Native(slug.to_string())),
            None => FormatId::new(s).ok().map(FormatRef::Registered),
        }
    }

    /// Whether the format is incumbent-native (not open).
    pub fn is_native(&self) -> bool {
        matches!(self, FormatRef::Native(_))
    }
}

impl fmt::Display for FormatRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatRef::Registered(id) => f.write_str(id.as_str()),
            FormatRef::Native(slug) => write!(f, "native:{slug}"),
        }
    }
}

/// A dotted numeric version (1 to 4 components), compared with missing components as 0.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version([u32; 4]);

impl Version {
    fn parse(s: &str) -> Option<Version> {
        let mut parts = [0u32; 4];
        for (n, p) in s.split('.').enumerate() {
            if n >= 4 || p.is_empty() || p.len() > 9 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            if p.len() > 1 && p.starts_with('0') {
                return None;
            }
            *parts.get_mut(n)? = p.parse().ok()?;
        }
        Some(Version(parts))
    }
}

/// An inclusive version range `<lo>..<hi>`; `None` is an open end (`*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRange {
    lo: Option<Version>,
    hi: Option<Version>,
    text: String,
}

impl VersionRange {
    /// Parses `<lo>..<hi>`; `None` when malformed or `lo > hi`.
    pub fn parse(s: &str) -> Option<VersionRange> {
        let (a, b) = s.split_once("..")?;
        let end = |x: &str| if x == "*" { Some(None) } else { Version::parse(x).map(Some) };
        let (lo, hi) = (end(a)?, end(b)?);
        if let (Some(l), Some(h)) = (&lo, &hi) {
            if l.0 > h.0 {
                return None;
            }
        }
        Some(VersionRange { lo, hi, text: s.to_string() })
    }

    /// Whether the two inclusive ranges share a version.
    pub fn overlaps(&self, other: &VersionRange) -> bool {
        // [a, b] and [c, d] overlap unless b < c or d < a; open ends never bound.
        let below = |hi: &Option<Version>, lo: &Option<Version>| match (hi, lo) {
            (Some(h), Some(l)) => h.0.cmp(&l.0) == Ordering::Less,
            _ => false,
        };
        !(below(&self.hi, &other.lo) || below(&other.hi, &self.lo))
    }

    /// The text as written.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

/// Where to export a non-admitted artefact: an open format ID (or none) and a note in the
/// project's own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRoute {
    /// `None` for `none`.
    pub format: Option<FormatId>,
    pub note: String,
}

/// A `mig-review@1` record. Missing text fields are empty and a missing attestation is
/// `false`; the check reports them (`SRC-REVIEW`, `SRC-ATTEST`), not the loader.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewRecord {
    pub entry: String,
    pub basis: String,
    pub reviewer: String,
    pub date: String,
    pub clean_room_statement: String,
    pub eula_note: String,
    pub counsel_ref: Option<String>,
    pub no_copied_text_attestation: bool,
}

/// One register entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    /// `src.<slug>`.
    pub id: String,
    /// Incumbent product name, for identification only.
    pub product: String,
    pub kind: ArtefactKind,
    pub format: FormatRef,
    pub versions: VersionRange,
    pub access_route: AccessRoute,
    pub status: Status,
    pub export_route: Option<ExportRoute>,
    pub review: Option<ReviewRecord>,
}

/// A loaded, schema-valid `mig-sources@1` register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRegister {
    version: SemVer,
    policy: Policy,
    entries: Vec<SourceEntry>,
    digest: Digest,
}

impl SourceRegister {
    /// The register version.
    pub fn version(&self) -> SemVer {
        self.version
    }

    /// The register policy.
    pub fn policy(&self) -> Policy {
        self.policy
    }

    /// The entries, ascending by ID.
    pub fn entries(&self) -> &[SourceEntry] {
        &self.entries
    }

    /// SHA-256 of the file's RFC 8785 canonical JSON.
    pub fn digest(&self) -> &Digest {
        &self.digest
    }
}

/// A sources-check finding, displayed as one line starting with its code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// `SRC-SCHEMA <detail>`.
    Schema(String),
    /// `SRC-DUP <id> <id>` (REQ-002), the lower ID first.
    Dup { first: String, second: String },
    /// `SRC-REVIEW <id> <detail>` (REQ-003).
    Review { id: String, detail: String },
    /// `SRC-ATTEST <id>` (REQ-005).
    Attest { id: String },
    /// `SRC-POLICY <id> <policy>` (REQ-004).
    Policy { id: String, policy: Policy },
    /// `SRC-VOCAB <id> <rule> <message>`: a vocabulary-lint error in an entry's text (run by
    /// `cargo xtask mig sources-check --vocab-rules`).
    Vocab { id: String, rule: String, message: String },
}

impl Finding {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            Finding::Schema(_) => "SRC-SCHEMA",
            Finding::Dup { .. } => "SRC-DUP",
            Finding::Review { .. } => "SRC-REVIEW",
            Finding::Attest { .. } => "SRC-ATTEST",
            Finding::Policy { .. } => "SRC-POLICY",
            Finding::Vocab { .. } => "SRC-VOCAB",
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Finding::Schema(d) => write!(f, "{code} {d}"),
            Finding::Dup { first, second } => write!(f, "{code} {first} {second}"),
            Finding::Review { id, detail } => write!(f, "{code} {id} {detail}"),
            Finding::Attest { id } => write!(f, "{code} {id}"),
            Finding::Policy { id, policy } => write!(f, "{code} {id} {policy}"),
            Finding::Vocab { id, rule, message } => write!(f, "{code} {id} {rule} {message}"),
        }
    }
}

/// The result of [`check`]: the register when it loaded, and every finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub register: Option<SourceRegister>,
    pub findings: Vec<Finding>,
}

impl CheckReport {
    /// True when the register loaded and there are no findings.
    pub fn is_ok(&self) -> bool {
        self.register.is_some() && self.findings.is_empty()
    }
}

// ------------------------------------------------------------------ loading

fn schema(msg: impl Into<String>) -> Finding {
    Finding::Schema(msg.into())
}

fn is_slug(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn is_entry_id(s: &str) -> bool {
    s.len() <= MAX_NAME && s.strip_prefix("src.").is_some_and(is_slug)
}

/// Whether the text could carry a link or an address: `://`, `www.` (any case) or `@`. The
/// page is public, so the register can never place one on it.
fn has_link(s: &str) -> bool {
    s.contains("://") || s.contains('@') || s.to_ascii_lowercase().contains("www.")
}

fn is_product(s: &str) -> bool {
    let b = s.as_bytes();
    !has_link(s)
        && !b.is_empty()
        && b.len() <= MAX_NAME
        && b[0].is_ascii_alphanumeric()
        && !s.ends_with(' ')
        && !s.contains("  ")
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b' ' | b'.' | b'-' | b'+' | b'&' | b'(' | b')'))
}

/// Free text from the register: printable, no control, line or paragraph separator,
/// invisible-operator, bidi-mark or bidi-override characters, no Markdown/HTML metacharacters,
/// nothing that could carry a link or an address ([`has_link`]), no surrounding whitespace, at
/// most `max` characters. Empty is allowed
/// only when `may_be_empty`.
pub(crate) fn is_text(s: &str, max: usize, may_be_empty: bool) -> bool {
    if s.is_empty() {
        return may_be_empty;
    }
    s.chars().count() <= max
        && s.trim() == s
        && !has_link(s)
        && s.chars().all(|c| {
            !c.is_control()
                && !FORBIDDEN.contains(&c)
                && !matches!(
                    c,
                    '\u{061c}'
                        | '\u{200b}'..='\u{200f}'
                        | '\u{2028}'..='\u{202e}'
                        | '\u{2060}'..='\u{2064}'
                        | '\u{2066}'..='\u{2069}'
                        | '\u{feff}'
                )
        })
}

/// A real calendar date `YYYY-MM-DD` (proleptic Gregorian, years 1900 to 9999).
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let p = s.get(r)?;
        if p.bytes().all(|c| c.is_ascii_digit()) {
            p.parse().ok()
        } else {
            None
        }
    };
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else { return false };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1900..=9999).contains(&y) && (1..=days).contains(&d)
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
            out.push(schema(format!("{at}: missing field {k}")));
            ok = false;
        }
    }
    for k in o.keys() {
        if !required.contains(&k.as_str()) && !optional.contains(&k.as_str()) {
            out.push(schema(format!("{at}: unknown field {k}")));
            ok = false;
        }
    }
    ok
}

fn string<'a>(o: &'a BTreeMap<String, CValue>, key: &str, at: &str, out: &mut Vec<Finding>) -> Option<&'a str> {
    match o.get(key) {
        Some(CValue::Str(s)) => Some(s),
        Some(_) => {
            out.push(schema(format!("{at}.{key}: must be a string")));
            None
        }
        None => None,
    }
}

fn export_route(v: &CValue, at: &str, out: &mut Vec<Finding>) -> Option<ExportRoute> {
    let CValue::Obj(o) = v else {
        out.push(schema(format!("{at}: must be an object")));
        return None;
    };
    if !keys_ok(o, &ROUTE_KEYS, &[], at, out) {
        return None;
    }
    let format = match string(o, "format", at, out)? {
        "none" => None,
        s => match FormatRef::parse(s) {
            Some(FormatRef::Registered(id)) => Some(id),
            Some(FormatRef::Native(_)) => {
                out.push(schema(format!("{at}.format: must be an open format ID or none, not a native format")));
                return None;
            }
            None => {
                out.push(schema(format!("{at}.format: not a format ID")));
                return None;
            }
        },
    };
    let note = string(o, "note", at, out)?;
    if !is_text(note, MAX_TEXT, false) {
        out.push(schema(format!(
            "{at}.note: must be 1 to {MAX_TEXT} characters of plain text (no markup, links or invisible characters)"
        )));
        return None;
    }
    Some(ExportRoute { format, note: note.to_string() })
}

fn review(v: &CValue, at: &str, out: &mut Vec<Finding>) -> Option<ReviewRecord> {
    let CValue::Obj(o) = v else {
        out.push(schema(format!("{at}: must be an object")));
        return None;
    };
    if !keys_ok(o, &["schema"], &REVIEW_KEYS, at, out) {
        return None;
    }
    let before = out.len();
    if string(o, "schema", at, out) != Some(REVIEW_SCHEMA) && out.len() == before {
        out.push(schema(format!("{at}.schema: must be {REVIEW_SCHEMA}")));
    }
    let mut text = |key: &str, max: usize| -> String {
        match string(o, key, at, out) {
            Some(s) if is_text(s, max, true) => s.to_string(),
            Some(_) => {
                out.push(schema(format!("{at}.{key}: must be at most {max} characters of plain text (no markup, links or invisible characters)")));
                String::new()
            }
            None => String::new(),
        }
    };
    let entry = text("entry", MAX_NAME);
    let basis = text("basis", MAX_TEXT);
    let reviewer = text("reviewer", MAX_NAME);
    let date = text("date", MAX_NAME);
    let clean_room_statement = text("clean_room_statement", MAX_TEXT);
    let eula_note = text("eula_note", MAX_TEXT);
    let counsel_ref = o.contains_key("counsel_ref").then(|| text("counsel_ref", MAX_NAME));
    let no_copied_text_attestation = match o.get("no_copied_text_attestation") {
        None => false,
        Some(CValue::Bool(b)) => *b,
        Some(_) => {
            out.push(schema(format!("{at}.no_copied_text_attestation: must be a boolean")));
            false
        }
    };
    (out.len() == before).then_some(ReviewRecord {
        entry,
        basis,
        reviewer,
        date,
        clean_room_statement,
        eula_note,
        counsel_ref,
        no_copied_text_attestation,
    })
}

fn entry(v: &CValue, index: usize, out: &mut Vec<Finding>) -> Option<SourceEntry> {
    let at = format!("entries[{index}]");
    let CValue::Obj(o) = v else {
        out.push(schema(format!("{at}: must be an object")));
        return None;
    };
    if !keys_ok(o, &ENTRY_REQUIRED, &ENTRY_OPTIONAL, &at, out) {
        return None;
    }
    let before = out.len();
    let mut bad = |what: String| out.push(schema(format!("{at}.{what}")));
    let id = match o.get("id") {
        Some(CValue::Str(s)) if is_entry_id(s) => s.clone(),
        _ => {
            bad("id: must be src.<slug> (lowercase letters, digits, -)".to_string());
            String::new()
        }
    };
    let product = match o.get("product") {
        Some(CValue::Str(s)) if is_product(s) => s.clone(),
        _ => {
            bad(format!("product: must be 1 to {MAX_NAME} ASCII letters, digits, spaces or .-+&()"));
            String::new()
        }
    };
    let kind = match o.get("kind") {
        Some(CValue::Str(s)) => ArtefactKind::parse(s),
        _ => None,
    };
    if kind.is_none() {
        bad("kind: must be results, points, project, form or program".to_string());
    }
    let format = match o.get("format_id") {
        Some(CValue::Str(s)) => FormatRef::parse(s),
        _ => None,
    };
    if format.is_none() {
        bad("format_id: must be a format ID or native:<slug>".to_string());
    }
    let versions = match o.get("versions") {
        Some(CValue::Str(s)) if s.len() <= MAX_NAME => VersionRange::parse(s),
        _ => None,
    };
    if versions.is_none() {
        bad("versions: must be <lo>..<hi> with dotted numeric versions or *, lo <= hi".to_string());
    }
    let access_route = match o.get("access_route") {
        Some(CValue::Str(s)) => AccessRoute::parse(s),
        _ => None,
    };
    if access_route.is_none() {
        bad("access_route: must be PUBLIC_SPEC, OPEN_EXPORT or NONE".to_string());
    }
    let status = match o.get("status") {
        Some(CValue::Str(s)) => Status::parse(s),
        _ => None,
    };
    if status.is_none() {
        bad("status: must be ADMITTED, OPEN_EXPORT_ONLY, NOT_ADMITTED or PENDING_REVIEW".to_string());
    }
    let export_route = o.get("export_route").and_then(|v| export_route(v, &format!("{at}.export_route"), out));
    let review = o.get("review").and_then(|v| review(v, &format!("{at}.review"), out));
    // REQ-001: an export route unless ADMITTED or OPEN_EXPORT_ONLY; status and route agree.
    match (status, access_route) {
        (Some(Status::NotAdmitted | Status::PendingReview), _) if !o.contains_key("export_route") => {
            out.push(schema(format!("{at}.export_route: required unless ADMITTED or OPEN_EXPORT_ONLY")));
        }
        (Some(Status::Admitted), Some(AccessRoute::NoAccess)) => {
            out.push(schema(format!("{at}: an ADMITTED entry needs an access route other than NONE")));
        }
        (Some(Status::OpenExportOnly), Some(r)) if r != AccessRoute::OpenExport => {
            out.push(schema(format!("{at}: an OPEN_EXPORT_ONLY entry needs access route OPEN_EXPORT")));
        }
        _ => {}
    }
    if out.len() != before {
        return None;
    }
    Some(SourceEntry {
        id,
        product,
        kind: kind?,
        format: format?,
        versions: versions?,
        access_route: access_route?,
        status: status?,
        export_route,
        review,
    })
}

/// Parses and schema-checks a `mig-sources@1` register. On failure every `SRC-SCHEMA`
/// finding is returned.
pub fn load(bytes: &[u8]) -> Result<SourceRegister, Vec<Finding>> {
    let value = fid_canon::parse_with_limits(bytes, MAX_REGISTER_BYTES, MAX_DEPTH)
        .map_err(|e| vec![schema(format!("register: not valid JSON within limits ({e})"))])?;
    let canonical = fid_canon::to_canonical_bytes(&value)
        .map_err(|e| vec![schema(format!("register: no canonical form ({e})"))])?;
    let digest = Digest::from_bytes(fid_canon::digest_bytes(&canonical).0);
    let CValue::Obj(top) = &value else {
        return Err(vec![schema("register: top level must be an object")]);
    };
    let mut out = Vec::new();
    if !keys_ok(top, &TOP_KEYS, &[], "register", &mut out) {
        return Err(out);
    }
    if top.get("schema") != Some(&CValue::Str(SCHEMA.to_string())) {
        out.push(schema(format!("schema: must be {SCHEMA}")));
    }
    let version = match top.get("version") {
        Some(CValue::Str(s)) => SemVer::parse(s),
        _ => None,
    };
    if version.is_none() {
        out.push(schema("version: must be MAJOR.MINOR.PATCH"));
    }
    let policy = match top.get("policy") {
        Some(CValue::Str(s)) => Policy::parse(s),
        _ => None,
    };
    if policy.is_none() {
        out.push(schema("policy: must be D-033-A or D-033-B"));
    }
    let mut entries: Vec<SourceEntry> = Vec::new();
    match top.get("entries") {
        Some(CValue::Arr(a)) if a.len() <= MAX_ENTRIES => {
            let mut ids = BTreeSet::new();
            for (i, v) in a.iter().enumerate() {
                if let Some(e) = entry(v, i, &mut out) {
                    if ids.insert(e.id.clone()) {
                        entries.push(e);
                    } else {
                        out.push(schema(format!("entries[{i}].id: duplicate entry ID {}", e.id)));
                    }
                }
            }
        }
        _ => out.push(schema(format!("entries: must be an array of at most {MAX_ENTRIES} entries"))),
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    match (version, policy) {
        (Some(version), Some(policy)) if out.is_empty() => Ok(SourceRegister { version, policy, entries, digest }),
        _ => Err(out),
    }
}

/// The embedded production register.
pub fn embedded() -> Result<SourceRegister, Vec<Finding>> {
    load(EMBEDDED)
}

// ------------------------------------------------------------------ checks

/// REQ-002: entries with the same product (ASCII-case-insensitive), kind and format and
/// overlapping versions. Pairs in ID order.
pub fn check_duplicates(reg: &SourceRegister) -> Vec<Finding> {
    let mut out = Vec::new();
    let e = reg.entries();
    for (i, a) in e.iter().enumerate() {
        for b in e.iter().skip(i + 1) {
            if a.product.eq_ignore_ascii_case(&b.product)
                && a.kind == b.kind
                && a.format == b.format
                && a.versions.overlaps(&b.versions)
            {
                out.push(Finding::Dup { first: a.id.clone(), second: b.id.clone() });
            }
        }
    }
    out
}

/// REQ-003 and REQ-005: review records and attestations, per entry in ID order.
pub fn check_reviews(reg: &SourceRegister) -> Vec<Finding> {
    let mut out = Vec::new();
    for e in reg.entries() {
        let needs = matches!(e.status, Status::Admitted | Status::OpenExportOnly);
        match &e.review {
            None if needs => out.push(Finding::Review { id: e.id.clone(), detail: "missing review record".into() }),
            None => {}
            Some(r) => {
                if needs {
                    let mut missing: Vec<&str> = Vec::new();
                    if r.entry != e.id {
                        missing.push("entry");
                    }
                    if r.basis.is_empty() {
                        missing.push("basis");
                    }
                    if r.reviewer.is_empty() {
                        missing.push("reviewer");
                    }
                    if !is_date(&r.date) {
                        missing.push("date");
                    }
                    if r.clean_room_statement.is_empty() {
                        missing.push("clean_room_statement");
                    }
                    let counsel = reg.policy() == Policy::D033B && e.format.is_native() && e.status == Status::Admitted;
                    if counsel && r.counsel_ref.as_deref().unwrap_or_default().is_empty() {
                        missing.push("counsel_ref");
                    }
                    if !missing.is_empty() {
                        out.push(Finding::Review {
                            id: e.id.clone(),
                            detail: format!("missing or invalid {}", missing.join(",")),
                        });
                    }
                }
                if !r.no_copied_text_attestation {
                    out.push(Finding::Attest { id: e.id.clone() });
                }
            }
        }
    }
    out
}

/// REQ-004: under `D-033-A`, no native format may be `ADMITTED`.
pub fn check_policy(reg: &SourceRegister) -> Vec<Finding> {
    if reg.policy() != Policy::D033A {
        return Vec::new();
    }
    reg.entries()
        .iter()
        .filter(|e| e.status == Status::Admitted && e.format.is_native())
        .map(|e| Finding::Policy { id: e.id.clone(), policy: reg.policy() })
        .collect()
}

/// The format catalogue of a registry manifest: the IDs of its top-level `formats` array of
/// `{"id": <format ID>, ...}` objects (other fields and keys ignored). The workspace manifest
/// (`CORE.REG.MAN`) carries no format catalogue yet, so for it the set is empty and every
/// non-native format is refused until IO.REG.DETECT publishes one (fail closed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormatCatalogue {
    ids: BTreeSet<FormatId>,
}

impl FormatCatalogue {
    /// Whether the format is registered.
    pub fn contains(&self, id: &FormatId) -> bool {
        self.ids.contains(id)
    }

    /// Number of registered formats.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether no format is registered.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// Largest accepted registry manifest, in bytes (the `fid-canon` input limit).
pub const MAX_MANIFEST_BYTES: usize = fid_canon::MAX_INPUT_BYTES;

/// Reads the format catalogue of a registry manifest (bounded parse). `SRC-SCHEMA` when the
/// manifest is not an object or `formats` is malformed.
pub fn parse_manifest(bytes: &[u8]) -> Result<FormatCatalogue, Finding> {
    let value = fid_canon::parse_with_limits(bytes, MAX_MANIFEST_BYTES, fid_canon::MAX_DEPTH)
        .map_err(|e| schema(format!("manifest: not valid JSON within limits ({e})")))?;
    let CValue::Obj(top) = &value else { return Err(schema("manifest: must be an object")) };
    let mut ids = BTreeSet::new();
    match top.get("formats") {
        None => {}
        Some(CValue::Arr(items)) => {
            for (i, item) in items.iter().enumerate() {
                let id = match item {
                    CValue::Obj(o) => match o.get("id") {
                        Some(CValue::Str(s)) => FormatId::new(s).ok(),
                        _ => None,
                    },
                    _ => None,
                };
                match id {
                    Some(id) => {
                        ids.insert(id);
                    }
                    None => return Err(schema(format!("manifest.formats[{i}]: needs an id that is a format ID"))),
                }
            }
        }
        Some(_) => return Err(schema("manifest.formats: must be an array")),
    }
    Ok(FormatCatalogue { ids })
}

/// "Open format" is checked against the format registry: every non-native `format_id` and
/// every export-route format must be registered in the manifest's format catalogue, else
/// `SRC-SCHEMA <entry>.<field>`.
pub fn check_registered(reg: &SourceRegister, formats: &FormatCatalogue) -> Vec<Finding> {
    let mut out = Vec::new();
    for e in reg.entries() {
        if let FormatRef::Registered(id) = &e.format {
            if !formats.contains(id) {
                out.push(schema(format!("{}.format_id: {id} is not a registered format", e.id)));
            }
        }
        if let Some(ExportRoute { format: Some(id), .. }) = &e.export_route {
            if !formats.contains(id) {
                out.push(schema(format!("{}.export_route.format: {id} is not a registered format", e.id)));
            }
        }
    }
    out
}

/// The whole sources check (`cargo xtask mig sources-check`, without the vocabulary lint):
/// schema, registered formats (against `manifest`), duplicates, reviews and attestations,
/// policy, in that order. A register that fails the schema or names an unregistered format
/// is not checked further.
pub fn check(bytes: &[u8], manifest: &[u8]) -> CheckReport {
    let reg = match load(bytes) {
        Ok(r) => r,
        Err(findings) => return CheckReport { register: None, findings },
    };
    let formats = match parse_manifest(manifest) {
        Ok(f) => f,
        Err(f) => return CheckReport { register: None, findings: vec![f] },
    };
    let unregistered = check_registered(&reg, &formats);
    if !unregistered.is_empty() {
        return CheckReport { register: None, findings: unregistered };
    }
    let mut findings = check_duplicates(&reg);
    findings.extend(check_reviews(&reg));
    findings.extend(check_policy(&reg));
    CheckReport { register: Some(reg), findings }
}

/// The free-text fields of each entry for the vocabulary lint: `(entry ID, field, text)`,
/// entries in ID order, fields in a fixed order.
pub fn texts(reg: &SourceRegister) -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    for e in reg.entries() {
        out.push((e.id.clone(), "product", e.product.clone()));
        if let Some(r) = &e.export_route {
            out.push((e.id.clone(), "export_route.note", r.note.clone()));
        }
        if let Some(r) = &e.review {
            for (field, text) in [
                ("review.basis", &r.basis),
                ("review.clean_room_statement", &r.clean_room_statement),
                ("review.eula_note", &r.eula_note),
            ] {
                if !text.is_empty() {
                    out.push((e.id.clone(), field, text.clone()));
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------ listing

/// Parameters of `migrate.list_sources@1`: `{ kind?, status?, format? = json }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListSources {
    pub kind: Option<ArtefactKind>,
    pub status: Option<Status>,
    pub format: Option<OutputFormat>,
}

/// Why `migrate.list_sources@1` failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListError {
    /// The parameters are malformed (`bad_params`).
    BadParams(String),
    /// The register does not load; carries the findings.
    Register(Vec<Finding>),
}

impl ListError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ListError::BadParams(_) => "bad_params",
            ListError::Register(_) => "register_invalid",
        }
    }
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListError::BadParams(m) => write!(f, "bad_params: {m}"),
            ListError::Register(fs) => {
                write!(f, "register_invalid: {} findings", fs.len())?;
                for x in fs.iter().take(3) {
                    write!(f, "; {x}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ListError {}

impl ListSources {
    /// Reads the parameter object; fails closed on unknown keys and bad values.
    pub fn from_value(v: &CValue) -> Result<ListSources, ListError> {
        let CValue::Obj(o) = v else { return Err(ListError::BadParams("params is not an object".into())) };
        if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "kind" | "status" | "format")) {
            return Err(ListError::BadParams(format!("unknown parameter {k}")));
        }
        fn get<T>(
            o: &BTreeMap<String, CValue>,
            key: &str,
            parse: fn(&str) -> Option<T>,
        ) -> Result<Option<T>, ListError> {
            match o.get(key) {
                None => Ok(None),
                Some(CValue::Str(s)) => parse(s).map(Some).ok_or_else(|| ListError::BadParams(format!("bad {key}"))),
                Some(_) => Err(ListError::BadParams(format!("{key} is not a string"))),
            }
        }
        Ok(ListSources {
            kind: get(o, "kind", ArtefactKind::parse)?,
            status: get(o, "status", Status::parse)?,
            format: get(o, "format", OutputFormat::parse)?,
        })
    }
}

/// One listed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedEntry {
    pub id: String,
    pub product: String,
    pub kind: ArtefactKind,
    pub format_id: String,
    pub versions: String,
    pub access_route: AccessRoute,
    pub status: Status,
    pub export_route: Option<ExportRoute>,
}

/// The result of `migrate.list_sources@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceList {
    pub register_version: SemVer,
    pub register_digest: Digest,
    pub policy: Policy,
    pub kind: Option<ArtefactKind>,
    pub status: Option<Status>,
    pub entries: Vec<ListedEntry>,
}

/// REQ-006: every entry sorted by ID, filtered by kind and status.
pub fn list_sources(reg: &SourceRegister, params: &ListSources) -> SourceList {
    let entries = reg
        .entries()
        .iter()
        .filter(|e| params.kind.is_none_or(|k| k == e.kind) && params.status.is_none_or(|s| s == e.status))
        .map(|e| ListedEntry {
            id: e.id.clone(),
            product: e.product.clone(),
            kind: e.kind,
            format_id: e.format.to_string(),
            versions: e.versions.as_str().to_string(),
            access_route: e.access_route,
            status: e.status,
            export_route: e.export_route.clone(),
        })
        .collect();
    SourceList {
        register_version: reg.version(),
        register_digest: *reg.digest(),
        policy: reg.policy(),
        kind: params.kind,
        status: params.status,
        entries,
    }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn route_format(r: &ExportRoute) -> &str {
    r.format.as_ref().map_or("none", FormatId::as_str)
}

impl SourceList {
    /// The JSON result: `{register_version, register_digest, policy, filter, entries}`.
    pub fn to_value(&self) -> CValue {
        let mut filter = BTreeMap::new();
        if let Some(k) = self.kind {
            filter.insert("kind".to_string(), s(k.as_str()));
        }
        if let Some(st) = self.status {
            filter.insert("status".to_string(), s(st.as_str()));
        }
        let entries = self
            .entries
            .iter()
            .map(|e| {
                let mut o = BTreeMap::new();
                o.insert("id".to_string(), s(&e.id));
                o.insert("product".to_string(), s(&e.product));
                o.insert("kind".to_string(), s(e.kind.as_str()));
                o.insert("format_id".to_string(), s(&e.format_id));
                o.insert("versions".to_string(), s(&e.versions));
                o.insert("access_route".to_string(), s(e.access_route.as_str()));
                o.insert("status".to_string(), s(e.status.as_str()));
                if let Some(r) = &e.export_route {
                    o.insert("export_route".to_string(), obj([("format", s(route_format(r))), ("note", s(&r.note))]));
                }
                CValue::Obj(o)
            })
            .collect();
        obj([
            ("register_version", s(&self.register_version.to_string())),
            ("register_digest", s(self.register_digest.as_str())),
            ("policy", s(self.policy.as_str())),
            ("filter", CValue::Obj(filter)),
            ("entries", CValue::Arr(entries)),
        ])
    }

    /// REQ-007: the public sources page, rendered only from this listing. Every text field
    /// was checked free of Markdown and HTML metacharacters at load, so nothing is escaped.
    pub fn render_md(&self) -> String {
        let mut p = String::new();
        let _ = writeln!(
            p,
            "<!-- Generated by fid migrate list-sources ({LIST_SOURCES}) from {SCHEMA} version {} ({}). Do not edit. -->",
            self.register_version,
            self.register_digest.as_str()
        );
        let _ = writeln!(p, "# Incumbent sources");
        let _ = writeln!(p);
        let _ = writeln!(
            p,
            "Generated by `fid migrate list-sources --format md` from register {SCHEMA} version {}, digest `{}`. Do not edit.",
            self.register_version,
            self.register_digest.as_str()
        );
        let _ = writeln!(p);
        let _ = writeln!(p, "Policy: {}.", self.policy);
        if self.kind.is_some() || self.status.is_some() {
            let k = self.kind.map_or("any", ArtefactKind::as_str);
            let st = self.status.map_or("any", Status::as_str);
            let _ = writeln!(p, "Filter: kind {k}, status {st}.");
        }
        let _ = writeln!(p);
        if self.entries.is_empty() {
            let _ = writeln!(p, "No entries.");
            return p;
        }
        let _ = writeln!(p, "| ID | Product | Artefact | Format | Versions | Access route | Status | Export to |");
        let _ = writeln!(p, "|---|---|---|---|---|---|---|---|");
        for e in &self.entries {
            let export = e.export_route.as_ref().map_or("-", route_format);
            let _ = writeln!(
                p,
                "| `{}` | {} | {} | `{}` | `{}` | {} | {} | {} |",
                e.id,
                e.product,
                e.kind,
                e.format_id,
                e.versions,
                e.access_route,
                e.status,
                if export == "-" || export == "none" { export.to_string() } else { format!("`{export}`") }
            );
        }
        let routes: Vec<&ListedEntry> = self.entries.iter().filter(|e| e.export_route.is_some()).collect();
        if !routes.is_empty() {
            let _ = writeln!(p);
            let _ = writeln!(p, "## Export routes");
            let _ = writeln!(p);
            for e in routes {
                if let Some(r) = &e.export_route {
                    let _ = writeln!(p, "- `{}`: {}", e.id, r.note);
                }
            }
        }
        p
    }
}

/// Runs `migrate.list_sources@1` against register bytes: the JSON result, plus the rendered
/// page under `page` when `format` is `md`.
pub fn run_list_sources(register: &[u8], params: &CValue) -> Result<CValue, ListError> {
    let params = ListSources::from_value(params)?;
    let reg = load(register).map_err(ListError::Register)?;
    let list = list_sources(&reg, &params);
    let mut v = list.to_value();
    if params.format == Some(OutputFormat::Md) {
        if let CValue::Obj(o) = &mut v {
            o.insert("page".to_string(), CValue::Str(list.render_md()));
        }
    }
    Ok(v)
}

// ------------------------------------------------------------------ registry definition

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|x| s(x)).collect())
}

fn enum_prop(values: &[&str], description: &str) -> CValue {
    obj([("type", s("string")), ("enum", strs(values)), ("description", s(description))])
}

fn str_prop(description: &str) -> CValue {
    obj([("type", s("string")), ("description", s(description))])
}

fn kinds() -> Vec<&'static str> {
    ArtefactKind::ALL.iter().map(|k| k.as_str()).collect()
}

fn statuses() -> Vec<&'static str> {
    Status::ALL.iter().map(|k| k.as_str()).collect()
}

fn params_schema() -> CValue {
    obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", CValue::Arr(Vec::new())),
        (
            "properties",
            obj([
                ("kind", enum_prop(&kinds(), "Only entries of this artefact kind.")),
                ("status", enum_prop(&statuses(), "Only entries with this admission status.")),
                (
                    "format",
                    obj([
                        ("type", s("string")),
                        ("enum", strs(&["json", "md"])),
                        ("default", s("json")),
                        ("description", s("json: the listing only; md: also the rendered public sources page.")),
                    ]),
                ),
            ]),
        ),
    ])
}

fn result_schema() -> CValue {
    let route = obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["format", "note"])),
        (
            "properties",
            obj([
                ("format", str_prop("The open format ID to export to, or none.")),
                ("note", str_prop("How to export, in the project's own words.")),
            ]),
        ),
    ]);
    let entry = obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["access_route", "format_id", "id", "kind", "product", "status", "versions"])),
        (
            "properties",
            obj([
                ("id", str_prop("Entry ID, src.<slug>.")),
                ("product", str_prop("Incumbent product name, for identification only.")),
                ("kind", enum_prop(&kinds(), "Artefact kind.")),
                ("format_id", str_prop("Format-registry ID, or native:<slug> for an unregistered native format.")),
                ("versions", str_prop("Inclusive version range <lo>..<hi>; * is open.")),
                ("access_route", enum_prop(&["PUBLIC_SPEC", "OPEN_EXPORT", "NONE"], "How the artefact can be read.")),
                ("status", enum_prop(&statuses(), "Admission status.")),
                ("export_route", route),
            ]),
        ),
    ]);
    obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["entries", "filter", "policy", "register_digest", "register_version"])),
        (
            "properties",
            obj([
                ("register_version", str_prop("Version of the source register.")),
                ("register_digest", str_prop("SHA-256 of the register's canonical JSON.")),
                ("policy", enum_prop(&["D-033-A", "D-033-B"], "Register policy bound to D-033.")),
                (
                    "filter",
                    obj([
                        ("type", s("object")),
                        ("additionalProperties", CValue::Bool(false)),
                        (
                            "properties",
                            obj([
                                ("kind", enum_prop(&kinds(), "The kind filter applied.")),
                                ("status", enum_prop(&statuses(), "The status filter applied.")),
                            ]),
                        ),
                    ]),
                ),
                ("entries", obj([("type", s("array")), ("items", entry), ("description", s("Entries sorted by ID."))])),
                ("page", str_prop("The rendered sources page (format md only).")),
            ]),
        ),
    ])
}

/// The registry definition of `migrate.list_sources@1` (R0, D0), built through
/// [`CommandDef::from_value`] so every field rule of the registry applies.
pub fn command_def() -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("id", s(NAME)),
        ("major", CValue::Num(1.0)),
        ("title", s("List incumbent sources")),
        (
            "description",
            s("Lists the incumbent source register sorted by entry ID, with status, access route and export route, \
               optionally filtered by artefact kind or status; format md also renders the public sources page."),
        ),
        ("params_schema", params_schema()),
        ("result_schema", result_schema()),
        ("side_effect", s("R0")),
        ("mcp_projection", s("exposed")),
        ("determinism", s("D0")),
        ("idempotency", s("natural")),
        ("fs_effect", s("none")),
        ("undo", s("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", s("active")), ("since", s(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", s("none")),
        ("web", s("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}

/// Every command this crate declares (for `fid-registry-build`).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(vec![command_def()?])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        let r = |s: &str| VersionRange::parse(s).unwrap();
        assert!(r("1..2").overlaps(&r("2..3")));
        assert!(!r("1..2").overlaps(&r("2.1..3")));
        assert!(r("*..*").overlaps(&r("5..5")));
        assert!(r("2019..*").overlaps(&r("*..2019")));
        assert!(!r("2019..*").overlaps(&r("*..2018.9")));
        assert!(r("1.0..1").overlaps(&r("1..1.0.0.0")));
        for bad in ["", "1", "1...2", "2..1", "01..2", "1..2.3.4.5.6", "a..b", "1.-1..2", "1..", "..2", "1. ..2"] {
            assert!(VersionRange::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn links_and_invisible_characters() {
        for bad in [
            "See https://example.org for details.",
            "Ask a@b",
            "Visit WWW.example.org",
            "a\u{061c}b",
            "a\u{2028}b",
            "a\u{2029}b",
            "a\u{2060}b",
            "a\u{2064}b",
        ] {
            assert!(!is_text(bad, 100, false), "{bad:?}");
        }
        assert!(is_text("Mail someone at example dot org.", 100, false));
        assert!(!is_product("www.DemoCMM") && !is_product("Www.Demo"));
    }

    #[test]
    fn dates_and_text() {
        for ok in ["2026-02-28", "2024-02-29", "2000-02-29", "1999-12-31"] {
            assert!(is_date(ok), "{ok}");
        }
        for bad in ["2026-02-29", "1900-02-29", "2026-13-01", "2026-00-10", "2026-1-01", "26-01-01", "2026-04-31"] {
            assert!(!is_date(bad), "{bad}");
        }
        assert!(is_text("Export to QIF results, then import.", 100, false));
        for bad in ["", " x", "x ", "a|b", "<b>", "a\u{202e}b", "a\nb", "**x**", "[x](y)"] {
            assert!(!is_text(bad, 100, false), "{bad:?}");
        }
        assert!(is_product("DemoCMM 2 (Pro)"));
        assert!(!is_product("Demo  CMM") && !is_product(" Demo") && !is_product("Démo"));
    }
}
