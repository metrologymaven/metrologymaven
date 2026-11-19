//! The governing-spec value types. Every field is private and every constructor applies the
//! loaders' rules, so no value of a public type here can hold what a loader would refuse.
//!
//! ```json
//! { "dialect": "ASME", "standards": ["std:asme:y14.5:2018"],
//!   "defaults_profile": "dp:asme-y14.5-2018@1.0.0#sha256:..", "inherited_from": "plan" }
//! ```

use super::{echo, SpecError};
use crate::chars::dprof::{CharId, ProfileRef};
use crate::dmt_flow::roster::Dialect;
use fid_canon::CValue;
use fid_gps::dmt::ident::is_label;
use std::collections::BTreeMap;
use std::fmt;

/// Most standard designators in one governing spec.
pub const MAX_STANDARDS: usize = 16;
/// Longest designator or edition label, in bytes (the DMT schema's label cap).
pub const MAX_STD_LABEL_LEN: usize = 32;
/// Longest standard reference text, in bytes.
pub const MAX_STD_REF_LEN: usize = 80;
/// The scheme prefix of a standard reference.
pub const STD_PREFIX: &str = "std:";

/// Wire form of a governing spec's dialect: `ASME` or `ISO`, exactly (REQ-001 spelling).
pub fn dialect_wire(d: Dialect) -> &'static str {
    match d {
        Dialect::Asme => "ASME",
        Dialect::Iso => "ISO",
    }
}

/// Parses the wire form `ASME` or `ISO`; any other spelling (including `asme`) is refused.
pub fn parse_dialect_wire(s: &str) -> Option<Dialect> {
    match s {
        "ASME" => Some(Dialect::Asme),
        "ISO" => Some(Dialect::Iso),
        _ => None,
    }
}

/// A standard designator with its edition, `std:<body>:<designator>:<edition>`, for example
/// `std:asme:y14.5:2018` or `std:iso:1101:2017`. Body is `asme` or `iso`; designator and
/// edition are labels of the DMT schema (`fid_gps::dmt::ident::is_label`: 1 to 32 bytes of
/// `[a-z0-9.-]`, alphanumeric at both ends, no two separators in a row). Lower case only, so
/// two references can never differ only in case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StdRef {
    body: Dialect,
    designator: String,
    edition: String,
}

impl StdRef {
    /// Builds a reference from parts (`bad_std_ref` when a label breaks the grammar).
    pub fn new(body: Dialect, designator: &str, edition: &str) -> Result<StdRef, SpecError> {
        if !is_label(designator, MAX_STD_LABEL_LEN) || !is_label(edition, MAX_STD_LABEL_LEN) {
            let text = format!("{STD_PREFIX}{}:{designator}:{edition}", body.as_str());
            return Err(SpecError::BadStdRef { text: echo(&text) });
        }
        Ok(StdRef { body, designator: designator.to_owned(), edition: edition.to_owned() })
    }

    /// Parses `std:<asme|iso>:<designator>:<edition>` (`bad_std_ref` otherwise).
    pub fn parse(s: &str) -> Result<StdRef, SpecError> {
        let bad = || SpecError::BadStdRef { text: echo(s) };
        if s.len() > MAX_STD_REF_LEN {
            return Err(bad());
        }
        let rest = s.strip_prefix(STD_PREFIX).ok_or_else(bad)?;
        let mut parts = rest.split(':');
        let (body, designator, edition) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(b), Some(d), Some(e), None) => (b, d, e),
            _ => return Err(bad()),
        };
        let body = Dialect::parse(body).ok_or_else(bad)?;
        StdRef::new(body, designator, edition).map_err(|_| bad())
    }

    /// The standards body, which must equal the governing spec's dialect.
    pub fn body(&self) -> Dialect {
        self.body
    }

    /// The designator, e.g. `y14.5`.
    pub fn designator(&self) -> &str {
        &self.designator
    }

    /// The edition, e.g. `2018`.
    pub fn edition(&self) -> &str {
        &self.edition
    }

    /// The defaults-profile edition this standard corresponds to: `<designator>-<edition>`.
    pub fn profile_edition(&self) -> String {
        format!("{}-{}", self.designator, self.edition)
    }
}

impl fmt::Display for StdRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{STD_PREFIX}{}:{}:{}", self.body.as_str(), self.designator, self.edition)
    }
}

/// One to [`MAX_STANDARDS`] standard references, sorted by (body, designator, edition), with
/// no repeat and no designator listed with two editions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standards(Vec<StdRef>);

impl Standards {
    /// Builds the list. Checks, in order: none (`governing_spec_missing{standards}`), too many
    /// (`bad_request{too_many_standards}`), a repeat (`bad_request{duplicate_standard}`), one
    /// designator with two editions (`bad_request{conflicting_editions}`). The order given
    /// does not matter.
    pub fn new(mut refs: Vec<StdRef>) -> Result<Standards, SpecError> {
        if refs.is_empty() {
            return Err(SpecError::missing("standards"));
        }
        if refs.len() > MAX_STANDARDS {
            return Err(SpecError::bad_request("too_many_standards"));
        }
        refs.sort();
        for w in refs.windows(2) {
            if w[0] == w[1] {
                return Err(SpecError::bad_request("duplicate_standard"));
            }
            if w[0].body == w[1].body && w[0].designator == w[1].designator {
                return Err(SpecError::bad_request("conflicting_editions"));
            }
        }
        Ok(Standards(refs))
    }

    /// The references, sorted.
    pub fn as_slice(&self) -> &[StdRef] {
        &self.0
    }
}

/// Where a characteristic's governing spec came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InheritedFrom {
    /// Set by `gdt.set_governing_spec@1` (and the level of every plan or tenant default).
    Explicit,
    /// Copied from the plan default at creation.
    Plan,
    /// Copied from the tenant default at creation.
    Tenant,
}

impl InheritedFrom {
    /// Wire name: `explicit`, `plan` or `tenant`.
    pub fn as_str(self) -> &'static str {
        match self {
            InheritedFrom::Explicit => "explicit",
            InheritedFrom::Plan => "plan",
            InheritedFrom::Tenant => "tenant",
        }
    }

    /// Parses the wire name.
    pub fn parse(s: &str) -> Option<InheritedFrom> {
        match s {
            "explicit" => Some(InheritedFrom::Explicit),
            "plan" => Some(InheritedFrom::Plan),
            "tenant" => Some(InheritedFrom::Tenant),
            _ => None,
        }
    }
}

/// A plan ID: the characteristic-ID grammar of `GDT.CHAR.DPROF` (1 to 128 bytes, first
/// `[A-Za-z0-9]`, then `[A-Za-z0-9._:-]`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlanId(String);

impl PlanId {
    /// Parses a plan ID.
    pub fn parse(s: &str) -> Option<PlanId> {
        CharId::parse(s).map(|_| PlanId(s.to_owned()))
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A complete governing spec (REQ-GDT.CHAR.SPEC-001): one dialect, one or more standards of
/// that dialect, one defaults profile of that dialect whose edition is
/// `<designator>-<edition>` of one of the standards, and where the value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoverningSpec {
    dialect: Dialect,
    standards: Standards,
    defaults_profile: ProfileRef,
    inherited_from: InheritedFrom,
}

impl GoverningSpec {
    /// Builds a spec. Checks, in order: each standard's body against `dialect`
    /// (`dialect_mismatch{<std>}`, first in sorted order), the profile's dialect
    /// (`dialect_mismatch{<ref>}`), the profile's edition
    /// (`defaults_profile_edition_mismatch{<ref>}`).
    pub fn new(
        dialect: Dialect,
        standards: Standards,
        defaults_profile: ProfileRef,
        inherited_from: InheritedFrom,
    ) -> Result<GoverningSpec, SpecError> {
        if let Some(s) = standards.0.iter().find(|s| s.body != dialect) {
            return Err(SpecError::DialectMismatch { item: s.to_string() });
        }
        if defaults_profile.dialect() != dialect {
            return Err(SpecError::DialectMismatch { item: echo(&defaults_profile.to_string()) });
        }
        let edition = defaults_profile.id.edition.as_str();
        if !standards.0.iter().any(|s| s.profile_edition() == edition) {
            return Err(SpecError::DefaultsProfileEditionMismatch { reference: echo(&defaults_profile.to_string()) });
        }
        Ok(GoverningSpec { dialect, standards, defaults_profile, inherited_from })
    }

    /// The dialect.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// The standards, sorted.
    pub fn standards(&self) -> &[StdRef] {
        self.standards.as_slice()
    }

    /// The defaults-profile reference.
    pub fn defaults_profile(&self) -> &ProfileRef {
        &self.defaults_profile
    }

    /// Where the value came from.
    pub fn inherited_from(&self) -> InheritedFrom {
        self.inherited_from
    }

    /// The same spec with another origin (the copy made by inheritance).
    pub fn with_inherited_from(&self, from: InheritedFrom) -> GoverningSpec {
        GoverningSpec { inherited_from: from, ..self.clone() }
    }

    /// The same spec naming another defaults profile, re-checked (the re-point path).
    pub(crate) fn with_profile(&self, r: &ProfileRef) -> Result<GoverningSpec, SpecError> {
        GoverningSpec::new(self.dialect, self.standards.clone(), r.clone(), self.inherited_from)
    }

    /// Whether both name the same rulebook (dialect, standards, profile), whatever their origin.
    pub fn same_rules(&self, other: &GoverningSpec) -> bool {
        self.dialect == other.dialect
            && self.standards == other.standards
            && self.defaults_profile == other.defaults_profile
    }

    /// The spec as a canonical-JSON object.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("dialect".to_owned(), CValue::Str(dialect_wire(self.dialect).to_owned()));
        m.insert(
            "standards".to_owned(),
            CValue::Arr(self.standards.0.iter().map(|s| CValue::Str(s.to_string())).collect()),
        );
        m.insert("defaults_profile".to_owned(), CValue::Str(self.defaults_profile.to_string()));
        m.insert("inherited_from".to_owned(), CValue::Str(self.inherited_from.as_str().to_owned()));
        CValue::Obj(m)
    }

    /// Reads a spec object strictly. `structural` builds the error for a value that is not an
    /// object, has a field outside the four, or has a field of the wrong JSON type. Then, in
    /// order: the first missing field (`governing_spec_missing{field}`), the dialect
    /// (`bad_request{bad_dialect}`), the standards (each `bad_std_ref`, then
    /// [`Standards::new`]), the profile (`bad_profile_ref`), the origin
    /// (`structural("bad_inherited_from")`), then [`GoverningSpec::new`].
    pub fn from_cvalue(v: &CValue, structural: fn(&'static str) -> SpecError) -> Result<GoverningSpec, SpecError> {
        const FIELDS: [&str; 4] = ["dialect", "standards", "defaults_profile", "inherited_from"];
        let o = match v {
            CValue::Obj(o) => o,
            _ => return Err(structural("spec_not_object")),
        };
        if o.keys().any(|k| !FIELDS.contains(&k.as_str())) {
            return Err(structural("spec_unknown_field"));
        }
        let text = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(Some(s.as_str())),
            None => Ok(None),
            Some(_) => Err(structural("spec_wrong_type")),
        };
        let dialect = text("dialect")?;
        let standards = match o.get("standards") {
            None => None,
            Some(CValue::Arr(a)) => Some(a),
            Some(_) => return Err(structural("spec_wrong_type")),
        };
        let profile = text("defaults_profile")?;
        let from = text("inherited_from")?;
        let dialect = dialect.ok_or_else(|| SpecError::missing("dialect"))?;
        let standards = standards.ok_or_else(|| SpecError::missing("standards"))?;
        let profile = profile.ok_or_else(|| SpecError::missing("defaults_profile"))?;
        let from = from.ok_or_else(|| SpecError::missing("inherited_from"))?;
        let dialect = parse_dialect_wire(dialect).ok_or_else(|| SpecError::bad_request("bad_dialect"))?;
        if standards.len() > MAX_STANDARDS {
            return Err(SpecError::bad_request("too_many_standards"));
        }
        let mut refs = Vec::with_capacity(standards.len());
        for s in standards {
            match s {
                CValue::Str(t) => refs.push(StdRef::parse(t)?),
                _ => return Err(structural("spec_wrong_type")),
            }
        }
        let standards = Standards::new(refs)?;
        let profile = ProfileRef::parse(profile).ok_or(SpecError::BadProfileRef)?;
        let from = InheritedFrom::parse(from).ok_or_else(|| structural("bad_inherited_from"))?;
        GoverningSpec::new(dialect, standards, profile, from)
    }
}

/// The optional spec as a canonical-JSON value (`null` when absent).
pub(crate) fn opt_cvalue(s: Option<&GoverningSpec>) -> CValue {
    s.map_or(CValue::Null, GoverningSpec::to_cvalue)
}
