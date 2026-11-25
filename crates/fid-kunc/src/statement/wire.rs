//! Canonical JSON form of a statement (REQ-UNC.UEC.STMT-005), schema
//! `uncertainty_statement@1`.
//!
//! Written with the kernel RFC 8785 writer (`fid_kci::kci::json`), which sorts members by
//! UTF-16 code units and emits no whitespace. Every scalar is a JSON string (kernel
//! convention):
//!
//! - `f64`: its IEEE-754 bit pattern as exactly 16 lowercase hex digits, no prefix;
//! - booleans: `"true"` or `"false"`;
//! - counts: decimal `u32` without leading zeros;
//! - enums: their wire names; digests: `sha256:<64 lowercase hex>`.
//!
//! Absent optional members are omitted. `sources`, `contributors`, `provenance`,
//! `provenance.proposal` and `provenance.rule_sets` are always written (arrays may be
//! empty). Member names: `schema`, `class`, `uec0_reason`, `method`, `unit`, `u_std`,
//! `expanded` `{U, k}`, `interval` `{low, high, p, kind}`, `model` `{id, semver, digest}`,
//! `sources` `[{kind, id, digest, unverified}]` (`unverified` only when `"true"`), `contributors` `[{input_id, sensitivity, u_contrib,
//! variance_share}]`, `lpu_validated_by_mc`, `nonlinearity` `{version, outcome, v1, v2,
//! ratio, probed_inputs, probed_pairs, cross_terms_truncated, reason}`, `samples_digest`
//! (`MCM` only, omitted when absent), `selection_rule`,
//! `estimate_convention`, `provenance` `{rule_sets, principal, principal_kind, proposal,
//! adopted_by, supersedes}`.

use fid_kci::kci::json::{to_canonical as write, Json};
use fid_types::Digest;

use super::{
    Contributor, CoverageInterval, EvidenceClass, Expanded, IntervalKind, Method, ModelRef, NlinVerdict, Nonlinearity,
    PrincipalKind, Provenance, SourceKind, SourceRef, StatementParts, StmtError, Uec0Reason, SCHEMA,
};

const HEX: &[u8; 16] = b"0123456789abcdef";

fn s(v: &str) -> Json {
    Json::Str(v.to_owned())
}

fn hex(x: f64) -> Json {
    let bits = x.to_bits();
    let mut out = String::with_capacity(16);
    for i in (0..16).rev() {
        out.push(char::from(HEX[((bits >> (4 * i)) & 15) as usize]));
    }
    Json::Str(out)
}

fn boolean(b: bool) -> Json {
    s(if b { "true" } else { "false" })
}

type Members = Vec<(String, Json)>;

fn put(m: &mut Members, k: &str, v: Json) {
    m.push((k.to_owned(), v));
}

fn put_opt<T>(m: &mut Members, k: &str, v: Option<T>, f: impl FnOnce(T) -> Json) {
    if let Some(v) = v {
        put(m, k, f(v));
    }
}

/// Canonical bytes of already validated parts.
pub(crate) fn to_canonical(p: &StatementParts) -> Result<Vec<u8>, StmtError> {
    let mut m = Members::new();
    put(&mut m, "schema", s(SCHEMA));
    put(&mut m, "class", s(p.class.wire()));
    put_opt(&mut m, "uec0_reason", p.uec0_reason, |r| s(r.wire()));
    put_opt(&mut m, "method", p.method, |x| s(x.wire()));
    put_opt(&mut m, "unit", p.unit.as_deref(), s);
    put_opt(&mut m, "u_std", p.u_std, hex);
    put_opt(&mut m, "expanded", p.expanded, |e| Json::Obj(vec![("U".into(), hex(e.value)), ("k".into(), hex(e.k))]));
    put_opt(&mut m, "interval", p.interval, |i| {
        Json::Obj(vec![
            ("low".into(), hex(i.low)),
            ("high".into(), hex(i.high)),
            ("p".into(), hex(i.p)),
            ("kind".into(), s(i.kind.wire())),
        ])
    });
    put_opt(&mut m, "model", p.model.as_ref(), |x| {
        Json::Obj(vec![
            ("id".into(), s(&x.id)),
            ("semver".into(), s(&x.semver)),
            ("digest".into(), s(x.digest.as_str())),
        ])
    });
    let sources = p
        .sources
        .iter()
        .map(|x| {
            let mut o = Members::new();
            put(&mut o, "kind", s(x.kind.wire()));
            put(&mut o, "id", s(&x.id));
            put(&mut o, "digest", s(x.digest.as_str()));
            if x.unverified {
                put(&mut o, "unverified", boolean(true));
            }
            Json::Obj(o)
        })
        .collect();
    put(&mut m, "sources", Json::Arr(sources));
    let contributors = p
        .contributors
        .iter()
        .map(|c| {
            let mut o = Members::new();
            put(&mut o, "input_id", s(&c.input_id));
            put_opt(&mut o, "sensitivity", c.sensitivity, hex);
            put(&mut o, "u_contrib", hex(c.u_contrib));
            put(&mut o, "variance_share", hex(c.variance_share));
            Json::Obj(o)
        })
        .collect();
    put(&mut m, "contributors", Json::Arr(contributors));
    put_opt(&mut m, "lpu_validated_by_mc", p.lpu_validated_by_mc, boolean);
    put_opt(&mut m, "nonlinearity", p.nonlinearity.as_ref(), |n| {
        let mut o = Members::new();
        put(&mut o, "version", s(&n.version));
        put(&mut o, "outcome", s(n.verdict.wire()));
        put(&mut o, "v1", hex(n.v1));
        put(&mut o, "v2", hex(n.v2));
        put_opt(&mut o, "ratio", n.ratio, hex);
        put(&mut o, "probed_inputs", s(&n.probed_inputs.to_string()));
        put(&mut o, "probed_pairs", s(&n.probed_pairs.to_string()));
        put(&mut o, "cross_terms_truncated", boolean(n.cross_terms_truncated));
        put_opt(&mut o, "reason", n.reason.as_deref(), s);
        Json::Obj(o)
    });
    put_opt(&mut m, "samples_digest", p.samples_digest, |d| s(d.as_str()));
    put_opt(&mut m, "selection_rule", p.selection_rule.as_deref(), s);
    put_opt(&mut m, "estimate_convention", p.estimate_convention.as_deref(), s);
    let pv = &p.provenance;
    let mut o = Members::new();
    put(&mut o, "rule_sets", Json::Arr(pv.rule_sets.iter().map(|r| s(r.as_str())).collect()));
    put_opt(&mut o, "principal", pv.principal.as_deref(), s);
    put_opt(&mut o, "principal_kind", pv.principal_kind, |k| s(k.wire()));
    put(&mut o, "proposal", boolean(pv.proposal));
    put_opt(&mut o, "adopted_by", pv.adopted_by.as_deref(), s);
    put_opt(&mut o, "supersedes", pv.supersedes, |d| s(d.as_str()));
    put(&mut m, "provenance", Json::Obj(o));
    write(&Json::Obj(m)).map_err(|_| StmtError::Malformed { detail: "writer" })
}

// ------------------------------------------------------------------ reading

fn malformed(detail: &'static str) -> StmtError {
    StmtError::Malformed { detail }
}

fn invalid(field: &'static str) -> StmtError {
    StmtError::InvalidField { field }
}

/// An object whose members are taken one by one; leftovers are unknown members.
struct Obj(Vec<(String, Json)>);

impl Obj {
    fn of(j: Json) -> Result<Obj, StmtError> {
        match j {
            Json::Obj(m) => Ok(Obj(m)),
            _ => Err(malformed("wrong_type")),
        }
    }

    fn take(&mut self, key: &str) -> Option<Json> {
        let i = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.swap_remove(i).1)
    }

    fn need(&mut self, key: &str) -> Result<Json, StmtError> {
        self.take(key).ok_or(malformed("missing_member"))
    }

    fn finish(self) -> Result<(), StmtError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(malformed("unknown_member"))
        }
    }
}

fn string(j: Json) -> Result<String, StmtError> {
    match j {
        Json::Str(s) => Ok(s),
        _ => Err(malformed("wrong_type")),
    }
}

fn array(j: Json) -> Result<Vec<Json>, StmtError> {
    match j {
        Json::Arr(a) => Ok(a),
        _ => Err(malformed("wrong_type")),
    }
}

fn f64_of(j: Json, field: &'static str) -> Result<f64, StmtError> {
    let t = string(j)?;
    let ok = t.len() == 16 && t.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !ok {
        return Err(invalid(field));
    }
    let bits = u64::from_str_radix(&t, 16).map_err(|_| invalid(field))?;
    Ok(f64::from_bits(bits))
}

fn bool_of(j: Json, field: &'static str) -> Result<bool, StmtError> {
    match string(j)?.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid(field)),
    }
}

fn u32_of(j: Json, field: &'static str) -> Result<u32, StmtError> {
    let t = string(j)?;
    let ok = !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) && (t.len() == 1 || !t.starts_with('0'));
    if !ok {
        return Err(invalid(field));
    }
    t.parse::<u32>().map_err(|_| invalid(field))
}

fn digest_of(j: Json, field: &'static str) -> Result<Digest, StmtError> {
    Digest::parse(&string(j)?).map_err(|_| invalid(field))
}

fn wire_of<T>(j: Json, field: &'static str, f: fn(&str) -> Option<T>) -> Result<T, StmtError> {
    f(&string(j)?).ok_or(invalid(field))
}

fn opt<T>(v: Option<Json>, f: impl FnOnce(Json) -> Result<T, StmtError>) -> Result<Option<T>, StmtError> {
    v.map(f).transpose()
}

/// Parts from a parsed document. Unknown members, wrong JSON types and malformed scalars
/// fail here; the statement invariants are checked afterwards by the constructor.
///
/// Error priority: every error raised here (`malformed{detail}`, `unknown_method`,
/// `invalid_field{field}` for a bad hex, boolean, count, enum value or digest, and
/// `statement_incomplete{class}`) takes priority over the constructor's class, numbers,
/// completeness, text and consistency order. So a document with both an unknown method and a
/// UEC-0 number reports `unknown_method`.
pub(crate) fn from_json(j: Json) -> Result<StatementParts, StmtError> {
    let mut o = Obj::of(j)?;
    if string(o.need("schema")?)? != SCHEMA {
        return Err(malformed("schema"));
    }
    let class = match o.take("class") {
        Some(c) => wire_of(c, "class", EvidenceClass::from_wire)?,
        None => return Err(StmtError::StatementIncomplete { field: "class" }),
    };
    let mut p = StatementParts::new(class);
    p.uec0_reason = opt(o.take("uec0_reason"), |x| wire_of(x, "uec0_reason", Uec0Reason::from_wire))?;
    p.method = opt(o.take("method"), |x| Method::from_wire(&string(x)?).ok_or(StmtError::UnknownMethod))?;
    p.unit = opt(o.take("unit"), string)?;
    p.u_std = opt(o.take("u_std"), |x| f64_of(x, "u_std"))?;
    p.expanded = opt(o.take("expanded"), |x| {
        let mut e = Obj::of(x)?;
        let value = f64_of(e.need("U")?, "expanded.U")?;
        let k = f64_of(e.need("k")?, "expanded.k")?;
        e.finish()?;
        Ok(Expanded { value, k })
    })?;
    p.interval = opt(o.take("interval"), |x| {
        let mut e = Obj::of(x)?;
        let low = f64_of(e.need("low")?, "interval.low")?;
        let high = f64_of(e.need("high")?, "interval.high")?;
        let pp = f64_of(e.need("p")?, "interval.p")?;
        let kind = wire_of(e.need("kind")?, "interval.kind", IntervalKind::from_wire)?;
        e.finish()?;
        Ok(CoverageInterval { low, high, p: pp, kind })
    })?;
    p.model = opt(o.take("model"), |x| {
        let mut e = Obj::of(x)?;
        let id = string(e.need("id")?)?;
        let semver = string(e.need("semver")?)?;
        let digest = digest_of(e.need("digest")?, "model.digest")?;
        e.finish()?;
        Ok(ModelRef { id, semver, digest })
    })?;
    p.sources = array(o.need("sources")?)?
        .into_iter()
        .map(|x| {
            let mut e = Obj::of(x)?;
            let kind = wire_of(e.need("kind")?, "sources.kind", SourceKind::from_wire)?;
            let id = string(e.need("id")?)?;
            let digest = digest_of(e.need("digest")?, "sources.digest")?;
            // Absent means false; a written `"false"` re-serializes without the member and
            // so fails `non_canonical`.
            let unverified = opt(e.take("unverified"), |v| bool_of(v, "sources.unverified"))?.unwrap_or(false);
            e.finish()?;
            Ok(SourceRef { kind, id, digest, unverified })
        })
        .collect::<Result<_, StmtError>>()?;
    p.contributors = array(o.need("contributors")?)?
        .into_iter()
        .map(|x| {
            let mut e = Obj::of(x)?;
            let input_id = string(e.need("input_id")?)?;
            let sensitivity = opt(e.take("sensitivity"), |v| f64_of(v, "contributors.sensitivity"))?;
            let u_contrib = f64_of(e.need("u_contrib")?, "contributors.u_contrib")?;
            let variance_share = f64_of(e.need("variance_share")?, "contributors.variance_share")?;
            e.finish()?;
            Ok(Contributor { input_id, sensitivity, u_contrib, variance_share })
        })
        .collect::<Result<_, StmtError>>()?;
    p.lpu_validated_by_mc = opt(o.take("lpu_validated_by_mc"), |x| bool_of(x, "lpu_validated_by_mc"))?;
    p.nonlinearity = opt(o.take("nonlinearity"), |x| {
        let mut e = Obj::of(x)?;
        let n = Nonlinearity {
            version: string(e.need("version")?)?,
            verdict: wire_of(e.need("outcome")?, "nonlinearity.outcome", NlinVerdict::from_wire)?,
            v1: f64_of(e.need("v1")?, "nonlinearity.v1")?,
            v2: f64_of(e.need("v2")?, "nonlinearity.v2")?,
            ratio: opt(e.take("ratio"), |v| f64_of(v, "nonlinearity.ratio"))?,
            probed_inputs: u32_of(e.need("probed_inputs")?, "nonlinearity.probed_inputs")?,
            probed_pairs: u32_of(e.need("probed_pairs")?, "nonlinearity.probed_pairs")?,
            cross_terms_truncated: bool_of(e.need("cross_terms_truncated")?, "nonlinearity.cross_terms_truncated")?,
            reason: opt(e.take("reason"), string)?,
        };
        e.finish()?;
        Ok(n)
    })?;
    p.samples_digest = opt(o.take("samples_digest"), |x| digest_of(x, "samples_digest"))?;
    p.selection_rule = opt(o.take("selection_rule"), string)?;
    p.estimate_convention = opt(o.take("estimate_convention"), string)?;
    let mut e = Obj::of(o.need("provenance")?)?;
    p.provenance = Provenance {
        rule_sets: array(e.need("rule_sets")?)?.into_iter().map(string).collect::<Result<_, _>>()?,
        principal: opt(e.take("principal"), string)?,
        principal_kind: opt(e.take("principal_kind"), |x| {
            wire_of(x, "provenance.principal_kind", PrincipalKind::from_wire)
        })?,
        proposal: bool_of(e.need("proposal")?, "provenance.proposal")?,
        adopted_by: opt(e.take("adopted_by"), string)?,
        supersedes: opt(e.take("supersedes"), |x| digest_of(x, "provenance.supersedes"))?,
    };
    e.finish()?;
    o.finish()?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_sixteen_lowercase_digits_of_the_bits() {
        assert_eq!(hex(1.0), Json::Str("3ff0000000000000".into()));
        assert_eq!(hex(f64::from_bits(0x000f_a0b0_c0d0_e0f1)), Json::Str("000fa0b0c0d0e0f1".into()));
        assert_eq!(f64_of(Json::Str("3FF0000000000000".into()), "x"), Err(invalid("x")));
        assert_eq!(f64_of(Json::Str("+ff0000000000000".into()), "x"), Err(invalid("x")));
        assert_eq!(f64_of(Json::Str("3ff00000000000000".into()), "x"), Err(invalid("x")));
    }

    #[test]
    fn counts_are_plain_u32() {
        assert_eq!(u32_of(Json::Str("0".into()), "n"), Ok(0));
        assert_eq!(u32_of(Json::Str("4294967295".into()), "n"), Ok(u32::MAX));
        for bad in ["", "01", "+1", "-1", "4294967296", " 1"] {
            assert_eq!(u32_of(Json::Str(bad.into()), "n"), Err(invalid("n")), "{bad:?}");
        }
    }
}
