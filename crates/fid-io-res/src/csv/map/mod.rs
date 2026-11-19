//! Versioned result column maps (spec node `IO.RES.CSV.MAP`).
//!
//! A column map (`colmap@1`, JSON) says which column of a result table holds which
//! [`Field`], in which [`Layout`], with which [`Dialect`]. Loading is
//! bounded JSON parse → strict validation → RFC 8785 canonical form (`fid-canon`) →
//! SHA-256. The map's ID, version and digest ([`ColumnMap::identity`]) are what commands
//! journal (REQ-IO.RES.CSV.MAP-001).
//!
//! # `colmap@1`
//!
//! ```json
//! {
//!   "schema": "colmap@1",
//!   "id": "acme.cmm.semicolon",
//!   "version": "1.2.0",
//!   "layout": "long",
//!   "dialect": { "delimiter": "semicolon", "decimal_mark": ",", "header": true,
//!                "quote": "rfc4180", "encoding": "utf-8", "line_ending": "lf" },
//!   "columns": [ { "field": "characteristic_id", "header": "Char", "required": true },
//!                { "field": "actual", "index": 4, "required": true } ]
//! }
//! ```
//!
//! Every key shown is required except a column's `header` and `index` (at least one of
//! them); any other key, at any level, fails with `colmap_invalid{unknown_key}`. Values
//! are matched exactly (case-sensitive, no trimming). Rules, each with its reason code:
//!
//! - `schema` is `colmap@1` (`unsupported_schema`);
//! - `id` is 1 to 128 bytes of `a-z 0-9 . _ -`, starting with a letter or digit, without
//!   `..` (`bad_id`); IDs starting `fid.` are reserved for built-in maps (`reserved_id`);
//! - `version` is `MAJOR.MINOR.PATCH`, decimal without leading zeros (`bad_version`);
//! - `layout` is `long` (one row per result) or `wide` (one row per part, one column
//!   group per characteristic) (`bad_layout`);
//! - `delimiter` is `comma`, `semicolon` or `tab`; `decimal_mark` is `.` or `,`;
//!   `quote` is `rfc4180`; `encoding` is `utf-8`; `line_ending` is `lf` or `crlf`
//!   (`bad_delimiter`, `bad_decimal_mark`, `bad_quote`, `bad_encoding`, `bad_line_ending`);
//!   decimal comma with comma delimiter fails (`decimal_comma_with_comma_delimiter`);
//! - `columns` holds 1 to [`MAX_COLUMNS`] columns (`no_columns`, `too_many_columns`);
//! - `field` is a vocabulary name (`unknown_field`) mapped at most once
//!   (`duplicate_field`); two columns with the same index, or with headers equal
//!   ignoring ASCII case, fail (`duplicate_column`);
//! - `header` is 1 to [`MAX_HEADER_BYTES`] bytes without control characters, without
//!   leading or trailing white space and without U+FEFF (`bad_header`) and needs `dialect.header: true` (`header_without_header_row`);
//!   `index` is an integer 0 to [`MAX_INDEX`] (`bad_index`); a column needs one of them
//!   (`no_locator`); with both, the header row must hold that header at that index when
//!   the table is read ([`Column::locate`], `header_index_mismatch`);
//! - a long map maps `characteristic_id` or `characteristic_name`
//!   (`no_characteristic_key`);
//! - a wide map needs a header row (`wide_needs_header_row`), does not map
//!   `characteristic_id` (the group header carries it: `wide_characteristic_id_column`),
//!   and locates every characteristic-scope column by header only
//!   (`wide_group_needs_header`); no header of a wide map, part or group scope, holds
//!   [`WIDE_SEPARATOR`] (`wide_header_separator`). The group column of field header `h` for characteristic
//!   `c` is headed `c` + [`WIDE_SEPARATOR`] + `h`.
//!
//! Wrong JSON types fail with `wrong_type`, missing keys with `missing_key`, and the
//! JSON layer with `json_syntax`, `invalid_utf8` or `duplicate_key`. User maps are read
//! under `IO.REG.LIMITS` with a [`MAX_MAP_BYTES`] byte cap and a nesting cap of
//! [`MAX_DEPTH`] (`limit_exceeded`); their header texts are [`UntrustedText`].

mod dialect;
mod field;

pub use dialect::{
    decode_utf8, strip_bom, DecimalMark, Delimiter, Dialect, LineEnding, BOM, ENCODING, MAX_DECIMAL_BYTES, MAX_FIELDS,
    QUOTE, REASON_COMMA_COMMA,
};
pub use field::{Field, FieldType, Scope, EVALUATION_BASIS_VALUES, STATUS_VALUES, UEC_VALUES};

use fid_canon::{CValue, CanonError};
use fid_io_reg::{BoundedSource, Budget, IoError, LimitKind, LimitRequest, Position, Principal, UntrustedText};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;

/// The schema tag of this map format.
pub const SCHEMA: &str = "colmap@1";
/// Largest user map file, in bytes (REQ-IO.RES.CSV.MAP-005).
pub const MAX_MAP_BYTES: u64 = 1024 * 1024;
/// Deepest JSON nesting of a map (a map needs 3).
pub const MAX_DEPTH: u32 = 4;
/// Most columns in one map.
pub const MAX_COLUMNS: usize = 1024;
/// Largest column index.
pub const MAX_INDEX: u32 = 65_535;
/// Longest header text, in bytes.
pub const MAX_HEADER_BYTES: usize = 256;
/// Longest map ID, in bytes.
pub const MAX_ID_BYTES: usize = 128;
/// Joins characteristic ID and field header in wide-layout group headers.
pub const WIDE_SEPARATOR: char = ':';
/// Prefix reserved for built-in map IDs.
pub const BUILTIN_PREFIX: &str = "fid.";

/// Built-in long map: one row per result, every field.
pub const LONG_REF: &str = "fid.results.long@1";
/// Built-in wide map: one row per part, one column group per characteristic.
pub const WIDE_REF: &str = "fid.results.wide@1";
/// Every built-in map reference (`id@major`).
pub const BUILTIN_REFS: &[&str] = &[LONG_REF, WIDE_REF];

const LONG_JSON: &str = include_str!("long.json");
const WIDE_JSON: &str = include_str!("wide.json");

/// Errors of map loading. [`ColMapError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColMapError {
    /// `colmap_invalid`: the map breaks a `colmap@1` rule.
    Invalid {
        /// Static reason code (never file content).
        reason: &'static str,
        /// Zero-based position in `columns` of the offending column, if any.
        column: Option<usize>,
    },
    /// A reader-harness error (`limit_exceeded`, `io`, ...).
    Io(IoError),
}

impl ColMapError {
    /// The stable code: `colmap_invalid`, or the [`IoError`] code.
    pub fn code(&self) -> &'static str {
        match self {
            ColMapError::Invalid { .. } => "colmap_invalid",
            ColMapError::Io(e) => e.code(),
        }
    }

    /// The reason of `colmap_invalid`.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            ColMapError::Invalid { reason, .. } => Some(reason),
            ColMapError::Io(_) => None,
        }
    }
}

impl fmt::Display for ColMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColMapError::Invalid { reason, column: Some(c) } => write!(f, "colmap_invalid: {reason} (column {c})"),
            ColMapError::Invalid { reason, column: None } => write!(f, "colmap_invalid: {reason}"),
            ColMapError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ColMapError {}

impl From<IoError> for ColMapError {
    fn from(e: IoError) -> Self {
        ColMapError::Io(e)
    }
}

fn invalid(reason: &'static str) -> ColMapError {
    ColMapError::Invalid { reason, column: None }
}

fn invalid_at(reason: &'static str, column: usize) -> ColMapError {
    ColMapError::Invalid { reason, column: Some(column) }
}

/// Table layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layout {
    /// One row per result.
    Long,
    /// One row per part, one column group per characteristic.
    Wide,
}

impl Layout {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Long => "long",
            Layout::Wide => "wide",
        }
    }
}

/// Semantic version `MAJOR.MINOR.PATCH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Major.
    pub major: u32,
    /// Minor.
    pub minor: u32,
    /// Patch.
    pub patch: u32,
}

impl Version {
    /// Parses `MAJOR.MINOR.PATCH`: decimal digits, no leading zeros, each at most 9 digits.
    pub fn parse(s: &str) -> Option<Version> {
        let mut parts = s.split('.');
        let mut next = || -> Option<u32> {
            let p = parts.next()?;
            let ok = !p.is_empty()
                && p.len() <= 9
                && p.bytes().all(|b| b.is_ascii_digit())
                && !(p.len() > 1 && p.starts_with('0'));
            if ok {
                p.parse().ok()
            } else {
                None
            }
        };
        let v = Version { major: next()?, minor: next()?, patch: next()? };
        if parts.next().is_some() {
            return None;
        }
        Some(v)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Where a map came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Compiled into this crate.
    BuiltIn,
    /// Loaded from a user file.
    User,
}

/// One column of a map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// Header text the column is found by (and written with).
    pub header: Option<UntrustedText>,
    /// Zero-based column index.
    pub index: Option<u32>,
    /// What the column holds.
    pub field: Field,
    /// Whether every row must hold a value in it.
    pub required: bool,
}

impl Column {
    /// Finds this column in a table's header row (long layout, and part-scope columns of
    /// the wide layout; wide group columns are located by the importer). Headers match
    /// exactly.
    ///
    /// - index only: `Some(index)` if the row is that wide, else `None`;
    /// - header only: the position of the first cell equal to the header, else `None`;
    /// - header and index: `Some(index)` when the cell at `index` equals the header,
    ///   otherwise `colmap_invalid{header_index_mismatch}` naming map column `position`.
    ///
    /// `None` means not present; whether that is an error (`required`) is the importer's
    /// rule (`IO.RES.CSV.IN`).
    pub fn locate(&self, header_row: &[&str], position: usize) -> Result<Option<usize>, ColMapError> {
        let is_header = |cell: &str| self.header.as_ref().is_some_and(|h| *h == UntrustedText::new(cell));
        let index = self.index.and_then(|i| usize::try_from(i).ok());
        match (&self.header, index) {
            (None, None) => Ok(None),
            (None, Some(i)) => Ok(Some(i).filter(|i| *i < header_row.len())),
            (Some(_), None) => Ok(header_row.iter().position(|c| is_header(c))),
            (Some(_), Some(i)) => match header_row.get(i) {
                Some(c) if is_header(c) => Ok(Some(i)),
                _ => Err(invalid_at("header_index_mismatch", position)),
            },
        }
    }
}

/// What commands journal about a map (REQ-IO.RES.CSV.MAP-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapIdentity {
    /// Map ID.
    pub id: String,
    /// Map version.
    pub version: Version,
    /// SHA-256 of the RFC 8785 canonical form of the map JSON.
    pub digest: fid_types::Digest,
}

impl MapIdentity {
    /// `{"id": .., "version": "M.m.p", "sha256": "sha256:.."}` for a journal event.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("id".to_owned(), CValue::Str(self.id.clone()));
        m.insert("version".to_owned(), CValue::Str(self.version.to_string()));
        m.insert("sha256".to_owned(), CValue::Str(self.digest.as_str().to_owned()));
        CValue::Obj(m)
    }
}

/// A validated column map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMap {
    id: String,
    version: Version,
    layout: Layout,
    dialect: Dialect,
    columns: Vec<Column>,
    /// Header texts of `columns`, same positions, kept raw for writing tables.
    headers: Vec<Option<String>>,
    digest: fid_types::Digest,
    origin: Origin,
}

impl ColumnMap {
    /// Map ID.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Map version.
    pub fn version(&self) -> Version {
        self.version
    }
    /// `id@major`, the form built-in maps are named by.
    pub fn reference(&self) -> String {
        format!("{}@{}", self.id, self.version.major)
    }
    /// Layout.
    pub fn layout(&self) -> Layout {
        self.layout
    }
    /// Dialect.
    pub fn dialect(&self) -> &Dialect {
        &self.dialect
    }
    /// Columns, in file order.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }
    /// SHA-256 of the RFC 8785 canonical form of the map JSON.
    pub fn digest(&self) -> fid_types::Digest {
        self.digest
    }
    /// Built-in or user map.
    pub fn origin(&self) -> Origin {
        self.origin
    }
    /// ID, version and digest, for journaling.
    pub fn identity(&self) -> MapIdentity {
        MapIdentity { id: self.id.clone(), version: self.version, digest: self.digest }
    }
    /// The header text of the column at `position` in [`ColumnMap::columns`], as written in
    /// a table's header row (`IO.RES.CSV.OUT`). The map validated it: 1 to
    /// [`MAX_HEADER_BYTES`] bytes, no control characters, no padding, no U+FEFF.
    pub fn header_text(&self, position: usize) -> Option<&str> {
        self.headers.get(position).and_then(|h| h.as_deref())
    }
    /// The column holding `field`, if mapped.
    pub fn column(&self, field: Field) -> Option<&Column> {
        self.columns.iter().find(|c| c.field == field)
    }
    /// Whether `field` is mapped.
    pub fn has_field(&self, field: Field) -> bool {
        self.column(field).is_some()
    }

    /// The lossy check (REQ-IO.RES.CSV.MAP-003): every field of
    /// [`Field::LOSSY_CHECK`] this map omits, in that order. Empty means not lossy.
    pub fn lossy(&self) -> Vec<Field> {
        Field::LOSSY_CHECK.iter().copied().filter(|f| !self.has_field(*f)).collect()
    }

    /// Whether [`ColumnMap::lossy`] is non-empty.
    pub fn is_lossy(&self) -> bool {
        !self.lossy().is_empty()
    }
}

/// Reads a user map under `IO.REG.LIMITS` (1 MiB byte cap, REQ-IO.RES.CSV.MAP-005) and
/// loads it with [`load_user_map_bytes`].
pub fn load_user_map<R: Read>(reader: R) -> Result<ColumnMap, ColMapError> {
    let request = LimitRequest::default().bytes(MAX_MAP_BYTES).alloc(4 * MAX_MAP_BYTES).depth(MAX_DEPTH);
    let budget = Budget::new(Principal::Script, &request)?;
    let mut source = BoundedSource::new(reader, budget);
    let mut buf = Vec::new();
    source.read_to_end_bounded(&mut buf)?;
    load_user_map_bytes(&buf)
}

/// Loads a user map from its file bytes. More than [`MAX_MAP_BYTES`] fails with
/// `limit_exceeded{bytes}`; IDs starting `fid.` fail with `reserved_id`.
pub fn load_user_map_bytes(bytes: &[u8]) -> Result<ColumnMap, ColMapError> {
    if bytes.len() as u64 > MAX_MAP_BYTES {
        return Err(ColMapError::Io(IoError::LimitExceeded {
            limit: LimitKind::Bytes,
            value: MAX_MAP_BYTES,
            at: Position::byte(MAX_MAP_BYTES),
        }));
    }
    from_json(bytes, Origin::User)
}

/// The built-in map named `reference` (one of [`BUILTIN_REFS`]); any other name fails
/// with `colmap_invalid{unknown_builtin}`.
pub fn builtin(reference: &str) -> Result<ColumnMap, ColMapError> {
    let json = match reference {
        LONG_REF => LONG_JSON,
        WIDE_REF => WIDE_JSON,
        _ => return Err(invalid("unknown_builtin")),
    };
    let map = from_json(json.as_bytes(), Origin::BuiltIn)?;
    if map.reference() != reference {
        return Err(invalid("unknown_builtin"));
    }
    Ok(map)
}

/// Every built-in map, in [`BUILTIN_REFS`] order.
pub fn builtins() -> Result<Vec<ColumnMap>, ColMapError> {
    BUILTIN_REFS.iter().map(|r| builtin(r)).collect()
}

fn from_json(bytes: &[u8], origin: Origin) -> Result<ColumnMap, ColMapError> {
    let body = strip_bom(bytes);
    let value =
        fid_canon::parse_with_limits(body, MAX_MAP_BYTES as usize, MAX_DEPTH as usize).map_err(|e| match e {
            // Bytes are checked before parsing, so the parser's limit here is the nesting cap.
            CanonError::InputLimit => ColMapError::Io(IoError::LimitExceeded {
                limit: LimitKind::Depth,
                value: u64::from(MAX_DEPTH),
                at: Position::byte(0),
            }),
            CanonError::DuplicateKey(_) => invalid("duplicate_key"),
            CanonError::InvalidUtf8 => invalid("invalid_utf8"),
            CanonError::Syntax(_) | CanonError::NonFiniteNumber => invalid("json_syntax"),
        })?;
    let canonical = fid_canon::to_canonical_bytes(&value).map_err(|_| invalid("json_syntax"))?;
    let digest = fid_types::Digest::from_bytes(fid_canon::digest_bytes(&canonical).0);

    let top = as_obj(&value, None)?;
    check_keys(top, &["columns", "dialect", "id", "layout", "schema", "version"], &[], None)?;

    if as_str(get(top, "schema", None)?, None)? != SCHEMA {
        return Err(invalid("unsupported_schema"));
    }
    let id = as_str(get(top, "id", None)?, None)?;
    if !valid_id(id) {
        return Err(invalid("bad_id"));
    }
    let reserved = id.starts_with(BUILTIN_PREFIX);
    match origin {
        Origin::User if reserved => return Err(invalid("reserved_id")),
        Origin::BuiltIn if !reserved => return Err(invalid("bad_id")),
        _ => {}
    }
    let version = Version::parse(as_str(get(top, "version", None)?, None)?).ok_or_else(|| invalid("bad_version"))?;
    let layout = match as_str(get(top, "layout", None)?, None)? {
        "long" => Layout::Long,
        "wide" => Layout::Wide,
        _ => return Err(invalid("bad_layout")),
    };
    let dialect = parse_dialect(get(top, "dialect", None)?)?;
    let (columns, headers) = parse_columns(get(top, "columns", None)?, &dialect, layout)?;

    Ok(ColumnMap { id: id.to_owned(), version, layout, dialect, columns, headers, digest, origin })
}

fn parse_dialect(v: &CValue) -> Result<Dialect, ColMapError> {
    let o = as_obj(v, None)?;
    check_keys(o, &["decimal_mark", "delimiter", "encoding", "header", "line_ending", "quote"], &[], None)?;
    let delimiter =
        Delimiter::parse(as_str(get(o, "delimiter", None)?, None)?).ok_or_else(|| invalid("bad_delimiter"))?;
    let decimal_mark =
        DecimalMark::parse(as_str(get(o, "decimal_mark", None)?, None)?).ok_or_else(|| invalid("bad_decimal_mark"))?;
    let header = as_bool(get(o, "header", None)?, None)?;
    if as_str(get(o, "quote", None)?, None)? != QUOTE {
        return Err(invalid("bad_quote"));
    }
    if as_str(get(o, "encoding", None)?, None)? != ENCODING {
        return Err(invalid("bad_encoding"));
    }
    let line_ending =
        LineEnding::parse(as_str(get(o, "line_ending", None)?, None)?).ok_or_else(|| invalid("bad_line_ending"))?;
    Dialect::new(delimiter, decimal_mark, header, line_ending).map_err(invalid)
}

type Columns = (Vec<Column>, Vec<Option<String>>);

fn parse_columns(v: &CValue, dialect: &Dialect, layout: Layout) -> Result<Columns, ColMapError> {
    let items = match v {
        CValue::Arr(a) => a,
        _ => return Err(invalid("wrong_type")),
    };
    if items.is_empty() {
        return Err(invalid("no_columns"));
    }
    if items.len() > MAX_COLUMNS {
        return Err(invalid("too_many_columns"));
    }
    if layout == Layout::Wide && !dialect.header() {
        return Err(invalid("wide_needs_header_row"));
    }
    let mut out: Vec<Column> = Vec::with_capacity(items.len());
    // ASCII-lowercased headers of `out`, for the case-insensitive duplicate check.
    let mut folded: Vec<Option<String>> = Vec::with_capacity(items.len());
    let mut headers: Vec<Option<String>> = Vec::with_capacity(items.len());
    for (n, item) in items.iter().enumerate() {
        let at = Some(n);
        let o = as_obj(item, at)?;
        check_keys(o, &["field", "required"], &["header", "index"], at)?;
        let field = Field::parse(as_str(get(o, "field", at)?, at)?).ok_or_else(|| invalid_at("unknown_field", n))?;
        let required = as_bool(get(o, "required", at)?, at)?;
        let header = match o.get("header") {
            None => None,
            Some(h) => {
                let h = as_str(h, at)?;
                let padded = h.trim() != h || h.contains('\u{feff}');
                if h.is_empty() || h.len() > MAX_HEADER_BYTES || h.chars().any(char::is_control) || padded {
                    return Err(invalid_at("bad_header", n));
                }
                Some(h)
            }
        };
        let index = match o.get("index") {
            None => None,
            Some(CValue::Num(x)) => Some(as_index(*x).ok_or_else(|| invalid_at("bad_index", n))?),
            Some(_) => return Err(invalid_at("wrong_type", n)),
        };
        if header.is_none() && index.is_none() {
            return Err(invalid_at("no_locator", n));
        }
        if header.is_some() && !dialect.header() {
            return Err(invalid_at("header_without_header_row", n));
        }
        if layout == Layout::Wide {
            if field == Field::CharacteristicId {
                return Err(invalid_at("wide_characteristic_id_column", n));
            }
            if field.wide_scope() == Scope::Characteristic && (header.is_none() || index.is_some()) {
                return Err(invalid_at("wide_group_needs_header", n));
            }
            // Part-scope headers too, so no header of a wide table looks like a group header.
            if header.is_some_and(|h| h.contains(WIDE_SEPARATOR)) {
                return Err(invalid_at("wide_header_separator", n));
            }
        }
        let fold = header.map(str::to_ascii_lowercase);
        for (prev, prev_fold) in out.iter().zip(&folded) {
            if prev.field == field {
                return Err(invalid_at("duplicate_field", n));
            }
            let same_header = matches!((prev_fold, &fold), (Some(a), Some(b)) if a == b);
            let same_index = matches!((prev.index, index), (Some(a), Some(b)) if a == b);
            if same_header || same_index {
                return Err(invalid_at("duplicate_column", n));
            }
        }
        out.push(Column { header: header.map(UntrustedText::new), index, field, required });
        headers.push(header.map(str::to_owned));
        folded.push(fold);
    }
    if layout == Layout::Long
        && !out.iter().any(|c| matches!(c.field, Field::CharacteristicId | Field::CharacteristicName))
    {
        return Err(invalid("no_characteristic_key"));
    }
    Ok((out, headers))
}

fn as_index(x: f64) -> Option<u32> {
    let integral = matches!(x.fract().classify(), std::num::FpCategory::Zero);
    if integral && (0.0..=f64::from(MAX_INDEX)).contains(&x) {
        Some(x as u32)
    } else {
        None
    }
}

fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    let first_ok = b.first().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    first_ok
        && b.len() <= MAX_ID_BYTES
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
        && !id.contains("..")
}

type Obj = BTreeMap<String, CValue>;

fn as_obj(v: &CValue, column: Option<usize>) -> Result<&Obj, ColMapError> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(ColMapError::Invalid { reason: "wrong_type", column }),
    }
}

fn as_str(v: &CValue, column: Option<usize>) -> Result<&str, ColMapError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(ColMapError::Invalid { reason: "wrong_type", column }),
    }
}

fn as_bool(v: &CValue, column: Option<usize>) -> Result<bool, ColMapError> {
    match v {
        CValue::Bool(b) => Ok(*b),
        _ => Err(ColMapError::Invalid { reason: "wrong_type", column }),
    }
}

fn get<'a>(o: &'a Obj, key: &str, column: Option<usize>) -> Result<&'a CValue, ColMapError> {
    o.get(key).ok_or(ColMapError::Invalid { reason: "missing_key", column })
}

/// Unknown keys first (strictness), then missing required keys.
fn check_keys(o: &Obj, required: &[&str], optional: &[&str], column: Option<usize>) -> Result<(), ColMapError> {
    if o.keys().any(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str())) {
        return Err(ColMapError::Invalid { reason: "unknown_key", column });
    }
    if required.iter().any(|k| !o.contains_key(*k)) {
        return Err(ColMapError::Invalid { reason: "missing_key", column });
    }
    Ok(())
}
