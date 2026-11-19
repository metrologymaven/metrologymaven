//! The built-in catalog `dr-catalog@1` (REQ-UNC.DEC.TYPE-003, -006).
//!
//! Canonical form: `{"catalog":"dr-catalog@1","rules":[...]}` written by the kernel
//! RFC 8785 writer, rules sorted by (ID bytes, version). The built-in catalog is embedded
//! as its canonical bytes ([`BUILTIN_CATALOG_JSON`], identical to
//! `fixtures/unc/dec/type/catalog_v1.json`, which the independent Python reference writes)
//! and is parsed and fully validated on every load; its SHA-256 must equal
//! [`BUILTIN_CATALOG_DIGEST`] or the load fails. Only integer and byte operations are
//! involved, so the digest is the same on every target.

use fid_kci::kci::json::{to_canonical, Json};
use fid_types::Digest;

use super::json::{exact_members, member, read};
use super::wire::{rule_from_json, rule_to_json};
use super::{check_id, check_kind, check_params, DecError, DecisionRule, Origin, RuleId, RuleParams, RuleRef};

/// Catalog tag, the `catalog` member.
pub const CATALOG_TAG: &str = "dr-catalog@1";
/// Largest catalog document, in bytes.
pub const MAX_CATALOG_BYTES: usize = 64 * 1024;
/// Most rules in a catalog.
pub const MAX_RULES: usize = 256;

/// Canonical bytes of the built-in catalog `dr-catalog@1`.
pub const BUILTIN_CATALOG_JSON: &str = concat!(
    r#"{"catalog":"dr-catalog@1","rules":["#,
    r#"{"id":"dr:as-reported-by-source","kind":"label_only","params":{},"pdf_source":"none","#,
    r#""risk_reporting":"none","standards_ref":"none","version":"1"},"#,
    r#"{"id":"dr:guard-band","kind":"guard_band","params":{},"pdf_source":"mc_or_validated_normal","#,
    r#""risk_reporting":"specific","standards_ref":{"clause":"8.3","designator":"JCGM 106","edition":"2012"},"#,
    r#""version":"1"},"#,
    r#"{"id":"dr:iso14253-1:2017/default","kind":"conformance_probability","params":{"p_min":"0.95"},"#,
    r#""pdf_source":"mc_or_validated_normal","risk_reporting":"specific","#,
    r#""standards_ref":{"clause":"5","designator":"ISO 14253-1","edition":"2017"},"version":"1"},"#,
    r#"{"id":"dr:simple-acceptance","kind":"simple_acceptance","params":{},"#,
    r#""pdf_source":"mc_or_validated_normal","risk_reporting":"specific","#,
    r#""standards_ref":{"clause":"8.2","designator":"JCGM 106","edition":"2012"},"version":"1"}"#,
    "]}"
);

/// SHA-256 of [`BUILTIN_CATALOG_JSON`] (`fixtures/unc/dec/type/catalog_v1.sha256`).
pub const BUILTIN_CATALOG_DIGEST: &str = "sha256:10a6d72428e0c70da7f2dc11c90d8a0a28e44d8371fbe433ba152dadb9c07381";

fn bad(detail: &'static str) -> DecError {
    DecError::Malformed { detail }
}

/// A validated catalog of built-in rules with its canonical bytes and digest.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    rules: Vec<DecisionRule>,
    canonical: Vec<u8>,
    digest: Digest,
}

impl Catalog {
    /// Loads the embedded built-in catalog `dr-catalog@1` and checks its digest.
    pub fn builtin() -> Result<Catalog, DecError> {
        let c = Catalog::parse_canonical(BUILTIN_CATALOG_JSON.as_bytes())?;
        if c.digest.as_str() != BUILTIN_CATALOG_DIGEST {
            return Err(bad("builtin_digest"));
        }
        Ok(c)
    }

    /// Builds a catalog from built-in rules: at most [`MAX_RULES`]; every rule passes the
    /// same built-in checks as [`Catalog::parse_canonical`] (template ID, kind implied by
    /// the ID, parameters; so no customer rule and no resolved guard-band instance,
    /// `invalid_rule{id|kind|r|p_min}`); (ID, version) pairs unique (`duplicate_rule`).
    /// Rules are sorted by (ID bytes, version). The bytes written are therefore always
    /// bytes that [`Catalog::parse_canonical`] accepts.
    pub fn new(mut rules: Vec<DecisionRule>) -> Result<Catalog, DecError> {
        if rules.len() > MAX_RULES {
            return Err(bad("too_many_rules"));
        }
        for r in &rules {
            check_id(r.id(), Origin::Builtin)?;
            check_kind(r.kind(), r.id(), Origin::Builtin)?;
            check_params(r.params(), Origin::Builtin)?;
        }
        rules.sort_by_cached_key(|r| (r.id().to_string(), r.version()));
        let dup = rules.windows(2).any(|w| match w {
            [a, b] => a.id() == b.id() && a.version() == b.version(),
            _ => false,
        });
        if dup {
            return Err(DecError::DuplicateRule);
        }
        let doc = Json::Obj(vec![
            ("catalog".to_owned(), Json::Str(CATALOG_TAG.to_owned())),
            ("rules".to_owned(), Json::Arr(rules.iter().map(|r| rule_to_json(&r.to_parts())).collect())),
        ]);
        let canonical = to_canonical(&doc).map_err(|_| bad("writer"))?;
        let digest = fid_kci::kci::sha256_digest(&canonical);
        Ok(Catalog { rules, canonical, digest })
    }

    /// Parses catalog bytes. Order: size, JSON shape (`malformed`), tag
    /// (`malformed{catalog_tag}`), each rule in turn (built-in checks), duplicates
    /// (`duplicate_rule`), then byte equality with the canonical form (`non_canonical`).
    pub fn parse_canonical(input: &[u8]) -> Result<Catalog, DecError> {
        if input.len() > MAX_CATALOG_BYTES {
            return Err(bad("too_large"));
        }
        let Json::Obj(m) = read(input)? else { return Err(bad("not_an_object")) };
        exact_members(&m, &["catalog", "rules"])?;
        match member(&m, "catalog") {
            Some(Json::Str(t)) if t == CATALOG_TAG => {}
            _ => return Err(bad("catalog_tag")),
        }
        let Some(Json::Arr(items)) = member(&m, "rules") else { return Err(bad("rules")) };
        if items.len() > MAX_RULES {
            return Err(bad("too_many_rules"));
        }
        let rules = items.iter().map(|v| rule_from_json(v, Origin::Builtin)).collect::<Result<Vec<_>, _>>()?;
        let catalog = Catalog::new(rules)?;
        if catalog.canonical != input {
            return Err(DecError::NonCanonical);
        }
        Ok(catalog)
    }

    /// The rules, sorted by (ID bytes, version).
    pub fn rules(&self) -> &[DecisionRule] {
        &self.rules
    }

    /// The canonical RFC 8785 bytes.
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical
    }

    /// SHA-256 of [`Self::canonical_json`] (REQ-UNC.DEC.TYPE-006).
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The rule with exactly this ID and version.
    pub fn get(&self, id: &RuleId, version: u32) -> Option<&DecisionRule> {
        self.rules.iter().find(|r| r.id() == id && r.version() == version)
    }

    /// The rule a reference names. A guard-band instance `dr:guard-band(r=x)@v` resolves
    /// to the template `dr:guard-band@v` with `r = x` bound. A customer reference, or one
    /// not in the catalog, is `not_in_catalog` (customer rules are resolved by
    /// `UNC.DEC.DEFAULT`). A hand-built reference outside the grammar is refused first.
    pub fn resolve(&self, rule: &RuleRef) -> Result<DecisionRule, DecError> {
        rule.check()?;
        match &rule.id {
            RuleId::Customer { .. } => Err(DecError::NotInCatalog),
            RuleId::GuardBand { r: Some(r) } => {
                let template = self.get(&RuleId::GuardBand { r: None }, rule.version).ok_or(DecError::NotInCatalog)?;
                let mut parts = template.to_parts();
                parts.id = rule.id.clone();
                parts.params = RuleParams::GuardBand { r: Some(r.clone()) };
                // A bound instance: the customer-side parameter checks apply (r present, r >= 0).
                check_params(&parts.params, Origin::Customer)?;
                Ok(DecisionRule::from_validated_instance(parts))
            }
            id => self.get(id, rule.version).cloned().ok_or(DecError::NotInCatalog),
        }
    }
}
