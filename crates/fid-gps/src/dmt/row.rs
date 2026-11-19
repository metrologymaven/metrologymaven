//! Row types of the DMT schema, their strict parser from a parsed JSON value, and their
//! canonical JSON form.
//!
//! Every set-valued field (editions, modifiers, clauses, variants, ORS cases, limitations,
//! reviewers) must already be strictly ascending and duplicate-free in the source
//! (`malformed_table{not_sorted}`), so the canonical form never reorders anything the author
//! wrote except the rows themselves. No JSON number is accepted anywhere: integers and
//! floating-point parameters are strings (see [`Param`]).

use super::ident::{
    is_label, is_modifier, is_name, is_versioned_id, lower_alnum, AlgId, Dialect, DmtKey, OperatorKind, MAX_NAME_LEN,
};
use super::DmtError;
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Longest free text (`notes`), in Unicode scalar values (REQ-GDT.RES.SCHEMA-002).
pub const MAX_FREE_TEXT_CHARS: usize = 280;
/// Longest edition, designator or clause edition, in bytes.
pub const MAX_LABEL_LEN: usize = 32;
/// Longest clause number, in bytes.
pub const MAX_CLAUSE_LEN: usize = 24;
/// Longest actual-value definition ID or decision-rule hint ID, in bytes.
pub const MAX_ID_LEN: usize = 96;
/// Longest ORS case ID, in bytes.
pub const MAX_ORS_LEN: usize = 96;
/// Longest limitation code, in bytes.
pub const MAX_LIMITATION_LEN: usize = 80;
/// Longest reviewer ID, in bytes.
pub const MAX_REVIEWER_LEN: usize = 64;
/// Longest token parameter, in bytes.
pub const MAX_TOKEN_LEN: usize = 64;
/// Most editions per row.
pub const MAX_EDITIONS: usize = 8;
/// Most clause references per row.
pub const MAX_CLAUSES: usize = 16;
/// Most variants per row.
pub const MAX_VARIANTS: usize = 16;
/// Most operators per chain.
pub const MAX_STEPS: usize = 64;
/// Most parameters per operator.
pub const MAX_PARAMS: usize = 32;
/// Most ORS case IDs per row.
pub const MAX_ORS_CASES: usize = 64;
/// Most limitation codes per row.
pub const MAX_LIMITATIONS: usize = 32;
/// Most reviewers in a sign-off.
pub const MAX_REVIEWERS: usize = 16;

const ROW_FIELDS: [&str; 16] = [
    "actual_value",
    "characteristic",
    "clauses",
    "decision_rule_hint",
    "default_variant",
    "dialect",
    "editions",
    "key",
    "limitations",
    "modifiers",
    "notes",
    "ors_cases",
    "review",
    "status",
    "synthetic",
    "variants",
];
const CLAUSE_FIELDS: [&str; 3] = ["clause", "designator", "edition"];
const CHAIN_FIELDS: [&str; 2] = ["steps", "variant"];
const OPERATOR_FIELDS: [&str; 3] = ["alg", "kind", "params"];

/// A clause reference: designator, edition and clause number only (REQ-002).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClauseRef {
    /// Standard designator, e.g. `asme-y14.5` ([`is_label`]).
    pub designator: String,
    /// Edition, e.g. `2018` ([`is_label`]).
    pub edition: String,
    /// Clause number: `[0-9a-z]+(\.[0-9a-z]+)*`, annex letters in lower case.
    pub clause: String,
}

/// One canonical operator parameter. Written in JSON as a boolean or a string:
/// `"0x"` + 16 lower-case hex digits is an IEEE-754 binary64 bit pattern (finite only);
/// `-?[1-9][0-9]*` or `0` is a signed 64-bit integer; `[a-z][a-z0-9_.-]{0,63}` is a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParamKind {
    /// Boolean.
    Bool,
    /// 64-bit integer.
    Int,
    /// IEEE-754 binary64 bits.
    F64,
    /// Token.
    Token,
}

/// A parameter value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Param {
    /// Boolean.
    Bool(bool),
    /// Integer.
    Int(i64),
    /// IEEE-754 binary64 bits (never NaN or infinite).
    F64Bits(u64),
    /// Token.
    Token(String),
}

impl Param {
    /// The kind.
    pub fn kind(&self) -> ParamKind {
        match self {
            Param::Bool(_) => ParamKind::Bool,
            Param::Int(_) => ParamKind::Int,
            Param::F64Bits(_) => ParamKind::F64,
            Param::Token(_) => ParamKind::Token,
        }
    }

    /// The float value of an `F64Bits` parameter (a bit cast; no arithmetic).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Param::F64Bits(b) => Some(f64::from_bits(*b)),
            _ => None,
        }
    }

    /// Whether the value obeys the schema: a token within 64 bytes and its grammar, a
    /// finite float. Booleans and integers are always valid.
    pub fn is_valid(&self) -> bool {
        match self {
            Param::Bool(_) | Param::Int(_) => true,
            Param::F64Bits(b) => (b >> 52) & 0x7ff != 0x7ff,
            Param::Token(s) => is_token(s),
        }
    }

    fn parse_str(s: &str) -> Option<Param> {
        let b = s.as_bytes();
        if let Some(hex) = s.strip_prefix("0x") {
            if hex.len() != 16 || !hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
                return None;
            }
            let bits = u64::from_str_radix(hex, 16).ok()?;
            // Exponent all ones is NaN or infinity: refused (NaN is always an error).
            if (bits >> 52) & 0x7ff == 0x7ff {
                return None;
            }
            return Some(Param::F64Bits(bits));
        }
        if b.first().is_some_and(|&c| c == b'-' || c.is_ascii_digit()) {
            let digits = s.strip_prefix('-').unwrap_or(s);
            let d = digits.as_bytes();
            let ok = !d.is_empty()
                && d.len() <= 19
                && d.iter().all(u8::is_ascii_digit)
                && (d[0] != b'0' || (d.len() == 1 && digits.len() == s.len()));
            if !ok {
                return None;
            }
            return s.parse::<i64>().ok().map(Param::Int);
        }
        is_token(s).then(|| Param::Token(s.to_owned()))
    }

    fn to_cvalue(&self) -> CValue {
        match self {
            Param::Bool(v) => CValue::Bool(*v),
            Param::Int(v) => CValue::Str(v.to_string()),
            Param::F64Bits(v) => CValue::Str(format!("0x{v:016x}")),
            Param::Token(v) => CValue::Str(v.clone()),
        }
    }
}

/// One operator of a chain. Fields are private: an `Operator` can only be built through
/// [`Operator::new`], which enforces the parameter rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operator {
    kind: OperatorKind,
    alg: AlgId,
    params: BTreeMap<String, Param>,
}

impl Operator {
    /// Builds an operator. Fails `malformed_table{too_many_params}` past [`MAX_PARAMS`],
    /// `field_too_long{param_name}` / `malformed_table{bad_param_name}` on a bad name, and
    /// `field_too_long{param}` / `malformed_table{bad_param}` on a token over 64 bytes or off
    /// grammar, or a NaN or infinite float.
    pub fn new(kind: OperatorKind, alg: AlgId, params: BTreeMap<String, Param>) -> Result<Operator, DmtError> {
        if params.len() > MAX_PARAMS {
            return Err(DmtError::malformed("too_many_params"));
        }
        for (name, p) in &params {
            if name.len() > MAX_NAME_LEN {
                return Err(DmtError::too_long("param_name"));
            }
            if !is_name(name, MAX_NAME_LEN) {
                return Err(DmtError::malformed("bad_param_name"));
            }
            if let Param::Token(s) = p {
                if s.len() > MAX_TOKEN_LEN {
                    return Err(DmtError::too_long("param"));
                }
            }
            if !p.is_valid() {
                return Err(DmtError::malformed("bad_param"));
            }
        }
        Ok(Operator { kind, alg, params })
    }

    /// Kind (closed ISO 17450 vocabulary).
    pub fn kind(&self) -> OperatorKind {
        self.kind
    }

    /// Algorithm ID.
    pub fn alg(&self) -> &AlgId {
        &self.alg
    }

    /// Canonical parameters, by name.
    pub fn params(&self) -> &BTreeMap<String, Param> {
        &self.params
    }
}

/// A named operator chain (one variant of a row). An empty chain is allowed (placeholder rows
/// of `GDT.RES.ROWISO.STUBS`). Fields are private, so the cached digest cannot go stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorChain {
    variant: String,
    steps: Vec<Operator>,
    canonical: Vec<u8>,
    digest: Digest,
}

impl OperatorChain {
    /// Builds a chain and its digest. Fails `malformed_table{bad_variant}` on a bad name and
    /// `malformed_table{too_many_steps}` past [`MAX_STEPS`].
    pub fn new(variant: &str, steps: Vec<Operator>) -> Result<OperatorChain, DmtError> {
        if variant.len() > MAX_NAME_LEN {
            return Err(DmtError::too_long("variant"));
        }
        if !is_name(variant, MAX_NAME_LEN) {
            return Err(DmtError::malformed("bad_variant"));
        }
        if steps.len() > MAX_STEPS {
            return Err(DmtError::malformed("too_many_steps"));
        }
        let mut c =
            OperatorChain { variant: variant.to_owned(), steps, canonical: Vec::new(), digest: Digest::TOMBSTONE };
        let (canonical, digest) = canonical_and_digest(&c.to_cvalue())?;
        c.canonical = canonical;
        c.digest = digest;
        Ok(c)
    }

    /// Variant name.
    pub fn variant(&self) -> &str {
        &self.variant
    }

    /// The steps, in execution order.
    pub fn steps(&self) -> &[Operator] {
        &self.steps
    }

    /// The chain as a JSON value: `{steps: [{alg, kind, params}], variant}`.
    pub fn to_cvalue(&self) -> CValue {
        let steps = self
            .steps
            .iter()
            .map(|op| {
                let params = op.params.iter().map(|(k, v)| (k.clone(), v.to_cvalue())).collect();
                obj([
                    ("alg", CValue::Str(op.alg.as_str().to_owned())),
                    ("kind", CValue::Str(op.kind.as_str().to_owned())),
                    ("params", CValue::Obj(params)),
                ])
            })
            .collect();
        obj([("steps", CValue::Arr(steps)), ("variant", CValue::Str(self.variant.clone()))])
    }

    /// RFC 8785 canonical JSON of [`OperatorChain::to_cvalue`].
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    /// The chain digest: SHA-256 of [`OperatorChain::canonical_bytes`] (REQ-004).
    pub fn digest(&self) -> Digest {
        self.digest
    }
}

/// Row status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RowStatus {
    /// `supported`.
    Supported,
    /// `partial`.
    Partial,
    /// `unsupported`.
    Unsupported,
}

impl RowStatus {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            RowStatus::Supported => "supported",
            RowStatus::Partial => "partial",
            RowStatus::Unsupported => "unsupported",
        }
    }

    /// Parses the wire form.
    pub fn parse(s: &str) -> Option<RowStatus> {
        match s {
            "supported" => Some(RowStatus::Supported),
            "partial" => Some(RowStatus::Partial),
            "unsupported" => Some(RowStatus::Unsupported),
            _ => None,
        }
    }
}

/// Review state of a row. Sign-off rules (D-009) are enforced by `GDT.RES.ROWFLOW`; this
/// schema only fixes the shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewState {
    /// `{"state": "draft"}`.
    Draft,
    /// `{"state": "under_review"}`.
    UnderReview,
    /// `{"state": "signed_off", "reviewers": [...]}`: 1 to 16 reviewer IDs, ascending.
    SignedOff {
        /// Reviewer IDs (`[a-z0-9][a-z0-9._-]{0,63}`, the roster grammar).
        reviewers: Vec<String>,
    },
}

/// One DMT row (REQ-001). Fields are private and read through getters: a row can only be
/// built by [`row_from_value`], so it always obeys the schema and its cached digest is current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmtRow {
    key: DmtKey,
    dialect: Dialect,
    editions: Vec<String>,
    characteristic: String,
    modifiers: Vec<String>,
    clauses: Vec<ClauseRef>,
    variants: Vec<OperatorChain>,
    default_variant: String,
    actual_value: String,
    decision_rule_hint: String,
    ors_cases: Vec<String>,
    status: RowStatus,
    limitations: Vec<String>,
    review: ReviewState,
    notes: String,
    synthetic: bool,
    canonical: Vec<u8>,
    digest: Digest,
}

impl DmtRow {
    /// The key; dialect, characteristic and modifiers agree with it.
    pub fn key(&self) -> &DmtKey {
        &self.key
    }

    /// Dialect.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Edition set, ascending.
    pub fn editions(&self) -> &[String] {
        &self.editions
    }

    /// Characteristic type.
    pub fn characteristic(&self) -> &str {
        &self.characteristic
    }

    /// Modifier set, ascending (equal to the key's).
    pub fn modifiers(&self) -> &[String] {
        &self.modifiers
    }

    /// Clause references, ascending by (designator, edition, clause).
    pub fn clauses(&self) -> &[ClauseRef] {
        &self.clauses
    }

    /// Variants, ascending by name.
    pub fn variants(&self) -> &[OperatorChain] {
        &self.variants
    }

    /// Name of the default variant.
    pub fn default_variant(&self) -> &str {
        &self.default_variant
    }

    /// Actual-value definition ID.
    pub fn actual_value(&self) -> &str {
        &self.actual_value
    }

    /// Decision-rule hint ID.
    pub fn decision_rule_hint(&self) -> &str {
        &self.decision_rule_hint
    }

    /// ORS case IDs, ascending.
    pub fn ors_cases(&self) -> &[String] {
        &self.ors_cases
    }

    /// Status.
    pub fn status(&self) -> RowStatus {
        self.status
    }

    /// Limitation codes, ascending.
    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }

    /// Review state.
    pub fn review(&self) -> &ReviewState {
        &self.review
    }

    /// The only free-text field (at most 280 characters).
    pub fn notes(&self) -> &str {
        &self.notes
    }

    /// Whether the row is synthetic test data rather than expert content.
    pub fn synthetic(&self) -> bool {
        self.synthetic
    }

    /// The variant with this name.
    pub fn variant(&self, name: &str) -> Option<&OperatorChain> {
        self.variants.iter().find(|c| c.variant == name)
    }

    /// The default variant's chain.
    pub fn default_chain(&self) -> Option<&OperatorChain> {
        self.variant(&self.default_variant)
    }

    /// RFC 8785 canonical JSON of the row.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    /// The row digest: SHA-256 of [`DmtRow::canonical_bytes`] (REQ-004).
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The row as a JSON value (the canonical field set; every field always present).
    pub fn to_cvalue(&self) -> CValue {
        let strs = |v: &[String]| CValue::Arr(v.iter().map(|s| CValue::Str(s.clone())).collect());
        let clauses = self
            .clauses
            .iter()
            .map(|c| {
                obj([
                    ("clause", CValue::Str(c.clause.clone())),
                    ("designator", CValue::Str(c.designator.clone())),
                    ("edition", CValue::Str(c.edition.clone())),
                ])
            })
            .collect();
        let review = match &self.review {
            ReviewState::Draft => obj([("state", CValue::Str("draft".to_owned()))]),
            ReviewState::UnderReview => obj([("state", CValue::Str("under_review".to_owned()))]),
            ReviewState::SignedOff { reviewers } => {
                obj([("reviewers", strs(reviewers)), ("state", CValue::Str("signed_off".to_owned()))])
            }
        };
        obj([
            ("actual_value", CValue::Str(self.actual_value.clone())),
            ("characteristic", CValue::Str(self.characteristic.clone())),
            ("clauses", CValue::Arr(clauses)),
            ("decision_rule_hint", CValue::Str(self.decision_rule_hint.clone())),
            ("default_variant", CValue::Str(self.default_variant.clone())),
            ("dialect", CValue::Str(self.dialect.as_str().to_owned())),
            ("editions", strs(&self.editions)),
            ("key", CValue::Str(self.key.as_str().to_owned())),
            ("limitations", strs(&self.limitations)),
            ("modifiers", strs(&self.modifiers)),
            ("notes", CValue::Str(self.notes.clone())),
            ("ors_cases", strs(&self.ors_cases)),
            ("review", review),
            ("status", CValue::Str(self.status.as_str().to_owned())),
            ("synthetic", CValue::Bool(self.synthetic)),
            ("variants", CValue::Arr(self.variants.iter().map(OperatorChain::to_cvalue).collect())),
        ])
    }
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

pub(crate) fn canonical_and_digest(v: &CValue) -> Result<(Vec<u8>, Digest), DmtError> {
    let bytes = fid_canon::to_canonical_bytes(v).map_err(|e| DmtError::malformed(e.code()))?;
    let digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
    Ok((bytes, digest))
}

// ------------------------------------------------------------------ parsing helpers

type Obj = BTreeMap<String, CValue>;

pub(crate) fn as_obj<'a>(v: &'a CValue, fields: &[&str], required: &[&str]) -> Result<&'a Obj, DmtError> {
    let o = match v {
        CValue::Obj(o) => o,
        _ => return Err(DmtError::malformed("wrong_type")),
    };
    if o.keys().any(|k| !fields.contains(&k.as_str())) {
        return Err(DmtError::malformed("unknown_field"));
    }
    if required.iter().any(|k| !o.contains_key(*k)) {
        return Err(DmtError::malformed("missing_field"));
    }
    Ok(o)
}

/// A string field: present, a string, at most `max` bytes (`field_too_long{field}`), then
/// `ok` (else `malformed_table{bad_<field>}` via `bad`).
fn field<'a>(
    o: &'a Obj,
    name: &'static str,
    max: usize,
    ok: impl Fn(&str) -> bool,
    bad: &'static str,
) -> Result<&'a str, DmtError> {
    let s = str_of(o.get(name))?;
    check_str(s, name, max, &ok, bad)?;
    Ok(s)
}

fn str_of(v: Option<&CValue>) -> Result<&str, DmtError> {
    match v {
        Some(CValue::Str(s)) => Ok(s),
        Some(_) => Err(DmtError::malformed("wrong_type")),
        None => Err(DmtError::malformed("missing_field")),
    }
}

fn check_str(
    s: &str,
    name: &'static str,
    max: usize,
    ok: &impl Fn(&str) -> bool,
    bad: &'static str,
) -> Result<(), DmtError> {
    if s.len() > max {
        return Err(DmtError::too_long(name));
    }
    if !ok(s) {
        return Err(DmtError::malformed(bad));
    }
    Ok(())
}

fn arr_of<'a>(
    o: &'a Obj,
    name: &str,
    min: usize,
    max: usize,
    count_reason: &'static str,
) -> Result<&'a [CValue], DmtError> {
    let a = match o.get(name) {
        Some(CValue::Arr(a)) => a,
        Some(_) => return Err(DmtError::malformed("wrong_type")),
        None => return Err(DmtError::malformed("missing_field")),
    };
    if a.len() < min || a.len() > max {
        return Err(DmtError::malformed(count_reason));
    }
    Ok(a)
}

/// A string list field: element checks as [`field`], then strictly ascending by bytes.
fn str_list(
    o: &Obj,
    name: &'static str,
    (min, max_items): (usize, usize),
    max_len: usize,
    ok: impl Fn(&str) -> bool,
    bad: &'static str,
) -> Result<Vec<String>, DmtError> {
    let a = arr_of(o, name, min, max_items, "bad_count")?;
    let mut out: Vec<String> = Vec::with_capacity(a.len());
    for v in a {
        let s = str_of(Some(v))?;
        check_str(s, name, max_len, &ok, bad)?;
        out.push(s.to_owned());
    }
    ascending(&out, |s| s.as_bytes())?;
    Ok(out)
}

fn ascending<T, K: Ord + ?Sized>(items: &[T], key: impl Fn(&T) -> &K) -> Result<(), DmtError> {
    if items.windows(2).all(|w| key(&w[0]) < key(&w[1])) {
        Ok(())
    } else {
        Err(DmtError::malformed("not_sorted"))
    }
}

fn is_token(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_TOKEN_LEN
        && b[0].is_ascii_lowercase()
        && b.iter().all(|&c| lower_alnum(c) || matches!(c, b'_' | b'.' | b'-'))
}

fn is_clause(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(|p| !p.is_empty() && p.bytes().all(lower_alnum))
}

fn is_ors(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && lower_alnum(b[0])
        && b.iter().all(|&c| lower_alnum(c) || matches!(c, b'.' | b'_' | b':' | b'/' | b'-'))
}

fn is_limitation(s: &str) -> bool {
    let (code, arg) = match s.split_once(':') {
        Some((c, a)) => (c, Some(a)),
        None => (s, None),
    };
    is_name(code, 48)
        && arg.map_or(true, |a| !a.is_empty() && a.len() <= 31 && a.bytes().all(|c| lower_alnum(c) || c == b'_'))
}

/// The roster's reviewer ID grammar (`GDT.RES.ROWFLOW.ROSTER`): `[a-z0-9][a-z0-9._-]{0,63}`.
pub fn is_reviewer_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_REVIEWER_LEN
        && lower_alnum(b[0])
        && b.iter().all(|&c| lower_alnum(c) || matches!(c, b'.' | b'_' | b'-'))
}

fn is_free_text(s: &str) -> bool {
    s.chars().all(|c| !c.is_control())
}

fn parse_operator(v: &CValue) -> Result<Operator, DmtError> {
    let o = as_obj(v, &OPERATOR_FIELDS, &OPERATOR_FIELDS)?;
    let kind_text = str_of(o.get("kind"))?;
    let kind = OperatorKind::parse(kind_text)?;
    let alg_text = str_of(o.get("alg"))?;
    let alg = AlgId::parse(alg_text)?;
    let raw = match o.get("params") {
        Some(CValue::Obj(p)) => p,
        _ => return Err(DmtError::malformed("wrong_type")),
    };
    if raw.len() > MAX_PARAMS {
        return Err(DmtError::malformed("too_many_params"));
    }
    let mut params = BTreeMap::new();
    for (name, val) in raw {
        if name.len() > MAX_NAME_LEN {
            return Err(DmtError::too_long("param_name"));
        }
        if !is_name(name, MAX_NAME_LEN) {
            return Err(DmtError::malformed("bad_param_name"));
        }
        let p = match val {
            CValue::Bool(b) => Param::Bool(*b),
            CValue::Str(s) => {
                if s.len() > MAX_TOKEN_LEN {
                    return Err(DmtError::too_long("param"));
                }
                Param::parse_str(s).ok_or_else(|| DmtError::malformed("bad_param"))?
            }
            _ => return Err(DmtError::malformed("bad_param")),
        };
        params.insert(name.clone(), p);
    }
    Operator::new(kind, alg, params)
}

fn parse_chain(v: &CValue) -> Result<OperatorChain, DmtError> {
    let o = as_obj(v, &CHAIN_FIELDS, &CHAIN_FIELDS)?;
    let name = str_of(o.get("variant"))?;
    if name.len() > MAX_NAME_LEN {
        return Err(DmtError::too_long("variant"));
    }
    if !is_name(name, MAX_NAME_LEN) {
        return Err(DmtError::malformed("bad_variant"));
    }
    let steps = arr_of(o, "steps", 0, MAX_STEPS, "too_many_steps")?;
    let steps = steps.iter().map(parse_operator).collect::<Result<Vec<_>, _>>()?;
    OperatorChain::new(name, steps)
}

fn parse_clause(v: &CValue) -> Result<ClauseRef, DmtError> {
    let o = as_obj(v, &CLAUSE_FIELDS, &CLAUSE_FIELDS)?;
    let designator = field(o, "designator", MAX_LABEL_LEN, |s| is_label(s, MAX_LABEL_LEN), "bad_designator")?;
    let edition = field(o, "edition", MAX_LABEL_LEN, |s| is_label(s, MAX_LABEL_LEN), "bad_edition")?;
    let clause = field(o, "clause", MAX_CLAUSE_LEN, is_clause, "bad_clause")?;
    Ok(ClauseRef { designator: designator.to_owned(), edition: edition.to_owned(), clause: clause.to_owned() })
}

fn parse_review(v: &CValue) -> Result<ReviewState, DmtError> {
    let o = as_obj(v, &["reviewers", "state"], &["state"])?;
    match str_of(o.get("state"))? {
        "draft" | "under_review" if o.contains_key("reviewers") => Err(DmtError::malformed("unknown_field")),
        "draft" => Ok(ReviewState::Draft),
        "under_review" => Ok(ReviewState::UnderReview),
        "signed_off" => {
            let reviewers =
                str_list(o, "reviewers", (1, MAX_REVIEWERS), MAX_REVIEWER_LEN, is_reviewer_id, "bad_reviewer")?;
            Ok(ReviewState::SignedOff { reviewers })
        }
        _ => Err(DmtError::malformed("bad_review_state")),
    }
}

/// Parses one row from a parsed JSON value. Checks run in this pinned order, and the first
/// failure is returned: field set (`unknown_field`, `missing_field`); `key` (`bad_dmt_key`);
/// `dialect`, `characteristic`, `modifiers` and their agreement with the key
/// (`key_mismatch`); `editions`; `clauses`; `variants` in array order (per operator: `kind`,
/// `alg`, `params`), then their order; `default_variant`; `actual_value`;
/// `decision_rule_hint`; `ors_cases`; `status`; `limitations`; `review`; `notes`;
/// `synthetic`. Every string is length-checked (`field_too_long`) before its grammar.
pub fn row_from_value(v: &CValue) -> Result<DmtRow, DmtError> {
    let o = as_obj(v, &ROW_FIELDS, &ROW_FIELDS)?;
    let key_text = str_of(o.get("key"))?;
    let key = DmtKey::parse(key_text)?;
    let dialect = Dialect::parse(str_of(o.get("dialect"))?).ok_or_else(|| DmtError::malformed("bad_dialect"))?;
    let characteristic =
        field(o, "characteristic", MAX_NAME_LEN, |s| is_name(s, MAX_NAME_LEN), "bad_characteristic")?.to_owned();
    let modifiers = str_list(
        o,
        "modifiers",
        (0, super::ident::MAX_MODIFIERS),
        super::ident::MAX_MODIFIER_LEN,
        is_modifier,
        "bad_modifier",
    )?;
    if dialect != key.dialect() || characteristic != key.characteristic() || modifiers.as_slice() != key.modifiers() {
        return Err(DmtError::malformed("key_mismatch"));
    }
    let editions =
        str_list(o, "editions", (1, MAX_EDITIONS), MAX_LABEL_LEN, |s| is_label(s, MAX_LABEL_LEN), "bad_edition")?;
    let clauses =
        arr_of(o, "clauses", 1, MAX_CLAUSES, "bad_count")?.iter().map(parse_clause).collect::<Result<Vec<_>, _>>()?;
    ascending(&clauses, |c| c)?;
    let variants =
        arr_of(o, "variants", 1, MAX_VARIANTS, "bad_count")?.iter().map(parse_chain).collect::<Result<Vec<_>, _>>()?;
    ascending(&variants, |c| c.variant.as_bytes())?;
    let default_variant = field(o, "default_variant", MAX_NAME_LEN, |s| is_name(s, MAX_NAME_LEN), "bad_variant")?;
    if !variants.iter().any(|c| c.variant == default_variant) {
        return Err(DmtError::malformed("unknown_default_variant"));
    }
    let actual_value = field(o, "actual_value", MAX_ID_LEN, |s| is_versioned_id(s, MAX_ID_LEN), "bad_actual_value")?;
    let decision_rule_hint =
        field(o, "decision_rule_hint", MAX_ID_LEN, |s| is_versioned_id(s, MAX_ID_LEN), "bad_decision_rule_hint")?;
    let ors_cases = str_list(o, "ors_cases", (0, MAX_ORS_CASES), MAX_ORS_LEN, is_ors, "bad_ors_case")?;
    let status = RowStatus::parse(str_of(o.get("status"))?).ok_or_else(|| DmtError::malformed("bad_status"))?;
    let limitations =
        str_list(o, "limitations", (0, MAX_LIMITATIONS), MAX_LIMITATION_LEN, is_limitation, "bad_limitation")?;
    let review = parse_review(o.get("review").unwrap_or(&CValue::Null))?;
    let notes = str_of(o.get("notes"))?;
    if notes.chars().count() > MAX_FREE_TEXT_CHARS {
        return Err(DmtError::too_long("notes"));
    }
    if !is_free_text(notes) {
        return Err(DmtError::malformed("bad_notes"));
    }
    let synthetic = match o.get("synthetic") {
        Some(CValue::Bool(b)) => *b,
        _ => return Err(DmtError::malformed("wrong_type")),
    };
    let mut row = DmtRow {
        key,
        dialect,
        editions,
        characteristic,
        modifiers,
        clauses,
        variants,
        default_variant: default_variant.to_owned(),
        actual_value: actual_value.to_owned(),
        decision_rule_hint: decision_rule_hint.to_owned(),
        ors_cases,
        status,
        limitations,
        review,
        notes: notes.to_owned(),
        synthetic,
        canonical: Vec::new(),
        digest: Digest::TOMBSTONE,
    };
    let (canonical, digest) = canonical_and_digest(&row.to_cvalue())?;
    row.canonical = canonical;
    row.digest = digest;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params() {
        assert_eq!(Param::parse_str("0x3ff0000000000000"), Some(Param::F64Bits(0x3ff0_0000_0000_0000)));
        assert_eq!(Param::parse_str("0x8000000000000000"), Some(Param::F64Bits(1 << 63)));
        assert_eq!(Param::parse_str("0x7ff8000000000000"), None);
        assert_eq!(Param::parse_str("0x7ff0000000000000"), None);
        assert_eq!(Param::parse_str("0x3FF0000000000000"), None);
        assert_eq!(Param::parse_str("0x3ff000000000000"), None);
        assert_eq!(Param::parse_str("0"), Some(Param::Int(0)));
        assert_eq!(Param::parse_str("-12"), Some(Param::Int(-12)));
        assert_eq!(Param::parse_str("-0"), None);
        assert_eq!(Param::parse_str("007"), None);
        assert_eq!(Param::parse_str("1.5"), None);
        assert_eq!(Param::parse_str("9223372036854775808"), None);
        assert_eq!(Param::parse_str("-9223372036854775808"), Some(Param::Int(i64::MIN)));
        assert_eq!(Param::parse_str("gauss"), Some(Param::Token("gauss".to_owned())));
        assert_eq!(Param::parse_str("Gauss"), None);
        assert_eq!(Param::parse_str(""), None);
    }

    #[test]
    fn small_grammars() {
        assert!(is_clause("7.2.1") && is_clause("a.3") && !is_clause("7..2") && !is_clause("A.3"));
        assert!(is_limitation("bestfit_disallowed:axis") && !is_limitation("x:") && !is_limitation("Bad"));
        assert!(is_reviewer_id("rev-1.a") && !is_reviewer_id("Rev") && !is_reviewer_id("-r"));
        assert!(is_ors("ors:gdt/flatness.001") && !is_ors("ORS:1"));
        assert!(is_free_text("plain words") && !is_free_text("tab\there"));
    }
}
