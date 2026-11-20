//! JSON form of a rule. Every scalar is a JSON string (kernel convention): the ID without
//! its version, the version as a decimal without leading zeros, enums by their wire names,
//! parameters as canonical decimals, and `standards_ref` as `"none"` or an object
//! `{clause, designator, edition}`. Members are written in RFC 8785 order by the kernel
//! writer.

use fid_kci::kci::json::Json;

use super::id::parse_version;
use super::json::{exact_members, member};
use super::{
    check_id, check_kind, check_params, check_pdf, check_risk, check_version, DecError, DecisionRule, Origin,
    ParamDecimal, PdfSource, RiskReporting, RuleId, RuleKind, RuleParams, RuleParts, StandardsRef,
};

/// The members of a rule object, in validation order.
pub(crate) const RULE_MEMBERS: [&str; 7] =
    ["id", "version", "kind", "params", "standards_ref", "pdf_source", "risk_reporting"];

fn s(v: &str) -> Json {
    Json::Str(v.to_owned())
}

fn invalid(param: &'static str) -> DecError {
    DecError::InvalidRule { param }
}

fn text(v: Option<&Json>) -> Option<&str> {
    match v {
        Some(Json::Str(x)) => Some(x.as_str()),
        _ => None,
    }
}

pub(crate) fn rule_to_json(p: &RuleParts) -> Json {
    let params = match &p.params {
        RuleParams::ConformanceProbability { p_min } => vec![("p_min".to_owned(), s(p_min.as_str()))],
        RuleParams::GuardBand { r: Some(r) } => vec![("r".to_owned(), s(r.as_str()))],
        RuleParams::GuardBand { r: None } | RuleParams::SimpleAcceptance | RuleParams::LabelOnly => Vec::new(),
    };
    let standards = match &p.standards_ref {
        None => s("none"),
        Some(r) => Json::Obj(vec![
            ("designator".to_owned(), s(r.designator())),
            ("edition".to_owned(), s(r.edition())),
            ("clause".to_owned(), s(r.clause())),
        ]),
    };
    Json::Obj(vec![
        ("id".to_owned(), s(&p.id.to_string())),
        ("version".to_owned(), s(&p.version.to_string())),
        ("kind".to_owned(), s(p.params.kind().wire())),
        ("params".to_owned(), Json::Obj(params)),
        ("standards_ref".to_owned(), standards),
        ("pdf_source".to_owned(), s(p.pdf_source.wire())),
        ("risk_reporting".to_owned(), s(p.risk_reporting.wire())),
    ])
}

fn read_params(v: Option<&Json>, kind: RuleKind) -> Result<RuleParams, DecError> {
    let Some(Json::Obj(p)) = v else { return Err(invalid("params")) };
    let allowed: &[&str] = match kind {
        RuleKind::ConformanceProbability => &["p_min"],
        RuleKind::GuardBand => &["r"],
        RuleKind::SimpleAcceptance | RuleKind::LabelOnly => &[],
    };
    // An unknown key, or (in a hand-built value) a repeated one.
    if p.len() > allowed.len() || p.iter().any(|(k, _)| !allowed.contains(&k.as_str())) {
        return Err(invalid("params"));
    }
    let dec = |name: &'static str| -> Result<Option<ParamDecimal>, DecError> {
        match member(p, name) {
            None => Ok(None),
            Some(Json::Str(x)) => ParamDecimal::parse(x).map(Some).ok_or_else(|| invalid(name)),
            Some(_) => Err(invalid(name)),
        }
    };
    Ok(match kind {
        RuleKind::ConformanceProbability => {
            RuleParams::ConformanceProbability { p_min: dec("p_min")?.ok_or_else(|| invalid("p_min"))? }
        }
        RuleKind::GuardBand => RuleParams::GuardBand { r: dec("r")? },
        RuleKind::SimpleAcceptance => RuleParams::SimpleAcceptance,
        RuleKind::LabelOnly => RuleParams::LabelOnly,
    })
}

fn read_standards(v: Option<&Json>) -> Result<Option<StandardsRef>, DecError> {
    match v {
        Some(Json::Str(x)) if x == "none" => Ok(None),
        Some(Json::Obj(m)) if m.len() == 3 => {
            let d = text(member(m, "designator"));
            let e = text(member(m, "edition"));
            let c = text(member(m, "clause"));
            match (d, e, c) {
                (Some(d), Some(e), Some(c)) => StandardsRef::new(d, e, c).map(Some),
                _ => Err(invalid("standards_ref")),
            }
        }
        _ => Err(invalid("standards_ref")),
    }
}

/// Reads a rule object, checking in the pinned order (module documentation of `rule`).
pub(crate) fn rule_from_json(v: &Json, origin: Origin) -> Result<DecisionRule, DecError> {
    let Json::Obj(m) = v else { return Err(DecError::Malformed { detail: "not_an_object" }) };
    exact_members(m, &RULE_MEMBERS)?;
    let id = match text(member(m, "id")) {
        Some(x) => RuleId::parse(x)?,
        None => return Err(DecError::InvalidRuleId),
    };
    check_id(&id, origin)?;
    let version = text(member(m, "version")).and_then(parse_version).ok_or_else(|| invalid("version"))?;
    check_version(version)?;
    let kind = text(member(m, "kind")).and_then(RuleKind::from_wire).ok_or_else(|| invalid("kind"))?;
    check_kind(kind, &id, origin)?;
    let params = read_params(member(m, "params"), kind)?;
    check_params(&params, origin)?;
    let standards_ref = read_standards(member(m, "standards_ref"))?;
    let pdf_source =
        text(member(m, "pdf_source")).and_then(PdfSource::from_wire).ok_or_else(|| invalid("pdf_source"))?;
    check_pdf(kind, pdf_source)?;
    let risk_reporting = text(member(m, "risk_reporting"))
        .and_then(RiskReporting::from_wire)
        .ok_or_else(|| invalid("risk_reporting"))?;
    check_risk(kind, risk_reporting)?;
    DecisionRule::validated(RuleParts { id, version, standards_ref, params, pdf_source, risk_reporting }, origin)
}
