//! The `uncertainty-model/v1` document: canonical writing of checked parts and mapping of a
//! read document onto parts. The member layout is in the module documentation of
//! [`super`].

use fid_kci::kci::json::{to_canonical as write, Json};
use fid_types::Digest;

use super::{
    invalid, is_model_number, CorrelationGroup, Edge, Endpoint, InPort, InputNode, InputSourceKind, ModelParts,
    OperatorNode, OutPort, OutputNode, SchemaError, SourceRef, SCHEMA,
};
use crate::dist::{DistError, Distribution, Params, Quantity, UnitCode};

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

fn obj(members: Vec<(&str, Json)>) -> Json {
    Json::Obj(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn dist_json(d: &Distribution) -> Json {
    let (kind, params): (&str, Vec<(&str, f64)>) = match d.params() {
        Params::Normal { mu, sigma } => ("normal", vec![("mu", mu), ("sigma", sigma)]),
        Params::Rect { a, b } => ("rect", vec![("a", a), ("b", b)]),
        Params::Tri { a, b, c } => ("tri", vec![("a", a), ("b", b), ("c", c)]),
        Params::Arcsine { a, b } => ("arcsine", vec![("a", a), ("b", b)]),
        Params::ScaledT { mu, s, nu } => ("scaled_t", vec![("mu", mu), ("s", s), ("nu", nu)]),
        Params::Trap { a, b, beta } => ("trap", vec![("a", a), ("b", b), ("beta", beta)]),
    };
    obj(vec![("kind", s(kind)), ("params", obj(params.into_iter().map(|(k, v)| (k, hex(v))).collect()))])
}

/// Canonical bytes of checked parts in canonical order.
pub(crate) fn to_canonical(p: &ModelParts) -> Result<Vec<u8>, SchemaError> {
    let inputs = p
        .inputs
        .iter()
        .map(|n| {
            let mut m = vec![
                ("id", s(&n.id)),
                ("quantity_kind", s(&n.quantity_kind)),
                ("unit", s(n.unit.as_str())),
                ("distribution", dist_json(&n.distribution)),
            ];
            if let Some(src) = &n.source {
                m.push((
                    "source",
                    obj(vec![
                        ("kind", s(src.kind.wire())),
                        ("digest", s(src.digest.as_str())),
                        ("author", s(&src.author)),
                    ]),
                ));
            }
            obj(m)
        })
        .collect();
    let operators = p
        .operators
        .iter()
        .map(|n| {
            let ins = n
                .ports_in
                .iter()
                .map(|x| {
                    obj(vec![
                        ("name", s(&x.name)),
                        ("quantity_kind", s(&x.quantity_kind)),
                        ("unit", s(x.unit.as_str())),
                        ("arity", s(&x.arity.to_string())),
                    ])
                })
                .collect();
            let outs = n
                .ports_out
                .iter()
                .map(|x| {
                    obj(vec![
                        ("name", s(&x.name)),
                        ("quantity_kind", s(&x.quantity_kind)),
                        ("unit", s(x.unit.as_str())),
                    ])
                })
                .collect();
            obj(vec![
                ("id", s(&n.id)),
                ("algorithm_id", s(&n.algorithm_id)),
                ("algorithm_version", s(&n.algorithm_version)),
                ("params_digest", s(n.params_digest.as_str())),
                ("ports_in", Json::Arr(ins)),
                ("ports_out", Json::Arr(outs)),
            ])
        })
        .collect();
    let outputs = p
        .outputs
        .iter()
        .map(|n| {
            let mut m = vec![("id", s(&n.id)), ("quantity_kind", s(&n.quantity_kind)), ("unit", s(n.unit.as_str()))];
            if let Some(c) = &n.characteristic {
                m.push(("characteristic", s(c)));
            }
            obj(m)
        })
        .collect();
    let end = |e: &Endpoint| obj(vec![("node", s(&e.node)), ("port", s(&e.port))]);
    let edges = p.edges.iter().map(|e| obj(vec![("from", end(&e.from)), ("to", end(&e.to))])).collect();
    let correlations = p
        .correlations
        .iter()
        .map(|g| {
            obj(vec![
                ("inputs", Json::Arr(g.inputs.iter().map(|x| s(x)).collect())),
                (
                    "matrix",
                    Json::Arr(g.matrix.iter().map(|r| Json::Arr(r.iter().map(|&x| hex(x)).collect())).collect()),
                ),
            ])
        })
        .collect();
    let doc = obj(vec![
        ("schema", s(SCHEMA)),
        ("name", s(&p.name)),
        ("semver", s(&p.semver)),
        ("inputs", Json::Arr(inputs)),
        ("operators", Json::Arr(operators)),
        ("outputs", Json::Arr(outputs)),
        ("edges", Json::Arr(edges)),
        ("correlations", Json::Arr(correlations)),
    ]);
    write(&doc).map_err(|_| invalid("writer"))
}

// ------------------------------------------------------------------ reading

/// An object whose members are taken one by one; leftovers are unknown members.
struct Obj {
    members: Vec<(String, Json)>,
    path: &'static str,
}

impl Obj {
    fn of(j: Json, path: &'static str) -> Result<Obj, SchemaError> {
        match j {
            Json::Obj(members) => Ok(Obj { members, path }),
            _ => Err(invalid(path)),
        }
    }

    fn take(&mut self, key: &str) -> Option<Json> {
        let i = self.members.iter().position(|(k, _)| k == key)?;
        Some(self.members.swap_remove(i).1)
    }

    /// `field` is the full path of the member, reported when it is missing.
    fn need(&mut self, key: &str, field: &'static str) -> Result<Json, SchemaError> {
        self.take(key).ok_or(invalid(field))
    }

    fn finish(self) -> Result<(), SchemaError> {
        if self.members.is_empty() {
            Ok(())
        } else {
            Err(invalid(self.path))
        }
    }
}

fn string(j: Json, field: &'static str) -> Result<String, SchemaError> {
    match j {
        Json::Str(s) => Ok(s),
        _ => Err(invalid(field)),
    }
}

fn array(j: Json, field: &'static str) -> Result<Vec<Json>, SchemaError> {
    match j {
        Json::Arr(a) => Ok(a),
        _ => Err(invalid(field)),
    }
}

/// 16 lowercase hex digits of a finite, non-negative-zero binary64.
fn f64_of(j: Json, field: &'static str) -> Result<f64, SchemaError> {
    let t = string(j, field)?;
    let ok = t.len() == 16 && t.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !ok {
        return Err(invalid(field));
    }
    let x = f64::from_bits(u64::from_str_radix(&t, 16).map_err(|_| invalid(field))?);
    if is_model_number(x) {
        Ok(x)
    } else {
        Err(invalid(field))
    }
}

/// Decimal `1..=u32::MAX` without leading zeros.
fn arity_of(j: Json, field: &'static str) -> Result<u32, SchemaError> {
    let t = string(j, field)?;
    let ok = !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) && !t.starts_with('0');
    if !ok {
        return Err(invalid(field));
    }
    t.parse::<u32>().map_err(|_| invalid(field))
}

fn digest_of(j: Json, field: &'static str) -> Result<Digest, SchemaError> {
    Digest::parse(&string(j, field)?).map_err(|_| invalid(field))
}

fn unit_of(j: Json, field: &'static str) -> Result<UnitCode, SchemaError> {
    UnitCode::parse(&string(j, field)?).ok_or(invalid(field))
}

fn param_field(name: &str) -> &'static str {
    match name {
        "mu" => "inputs.distribution.params.mu",
        "sigma" => "inputs.distribution.params.sigma",
        "a" => "inputs.distribution.params.a",
        "b" => "inputs.distribution.params.b",
        "c" => "inputs.distribution.params.c",
        "s" => "inputs.distribution.params.s",
        "nu" => "inputs.distribution.params.nu",
        "beta" => "inputs.distribution.params.beta",
        _ => "inputs.distribution.params",
    }
}

fn distribution_of(j: Json, unit: UnitCode) -> Result<Distribution, SchemaError> {
    let mut o = Obj::of(j, "inputs.distribution")?;
    let kind = string(o.need("kind", "inputs.distribution.kind")?, "inputs.distribution.kind")?;
    let names: &[&str] = match kind.as_str() {
        "normal" => &["mu", "sigma"],
        "rect" | "arcsine" => &["a", "b"],
        "tri" => &["a", "b", "c"],
        "scaled_t" => &["mu", "s", "nu"],
        "trap" => &["a", "b", "beta"],
        _ => return Err(invalid("inputs.distribution.kind")),
    };
    let mut p = Obj::of(o.need("params", "inputs.distribution.params")?, "inputs.distribution.params")?;
    let mut v = Vec::with_capacity(3);
    for &n in names {
        let field = param_field(n);
        let unit = if matches!(n, "nu" | "beta") { UnitCode::One } else { unit };
        v.push(Quantity::new(f64_of(p.need(n, field)?, field)?, unit));
    }
    p.finish()?;
    o.finish()?;
    let at = |i: usize| v.get(i).copied().ok_or(invalid("inputs.distribution.params"));
    let d = match kind.as_str() {
        "normal" => Distribution::normal(at(0)?, at(1)?),
        "rect" => Distribution::rect(at(0)?, at(1)?),
        "arcsine" => Distribution::arcsine(at(0)?, at(1)?),
        "tri" => Distribution::tri(at(0)?, at(1)?, at(2)?),
        "scaled_t" => Distribution::scaled_t(at(0)?, at(1)?, at(2)?),
        _ => Distribution::trap(at(0)?, at(1)?, at(2)?),
    };
    d.map_err(|e| match e {
        DistError::InvalidParameter { name } => invalid(param_field(name)),
        _ => invalid("inputs.distribution"),
    })
}

fn source_of(j: Json) -> Result<SourceRef, SchemaError> {
    let mut o = Obj::of(j, "inputs.source")?;
    let kind = InputSourceKind::from_wire(&string(o.need("kind", "inputs.source.kind")?, "inputs.source.kind")?)
        .ok_or(invalid("inputs.source.kind"))?;
    let digest = digest_of(o.need("digest", "inputs.source.digest")?, "inputs.source.digest")?;
    let author = string(o.need("author", "inputs.source.author")?, "inputs.source.author")?;
    o.finish()?;
    Ok(SourceRef { kind, digest, author })
}

fn input_of(j: Json) -> Result<InputNode, SchemaError> {
    let mut o = Obj::of(j, "inputs")?;
    let id = string(o.need("id", "inputs.id")?, "inputs.id")?;
    let quantity_kind = string(o.need("quantity_kind", "inputs.quantity_kind")?, "inputs.quantity_kind")?;
    let unit = unit_of(o.need("unit", "inputs.unit")?, "inputs.unit")?;
    let distribution = distribution_of(o.need("distribution", "inputs.distribution")?, unit)?;
    let source = o.take("source").map(source_of).transpose()?;
    o.finish()?;
    Ok(InputNode { id, quantity_kind, unit, distribution, source })
}

fn in_port_of(j: Json) -> Result<InPort, SchemaError> {
    let mut o = Obj::of(j, "operators.ports_in")?;
    let name = string(o.need("name", "operators.ports_in.name")?, "operators.ports_in.name")?;
    let quantity_kind =
        string(o.need("quantity_kind", "operators.ports_in.quantity_kind")?, "operators.ports_in.quantity_kind")?;
    let unit = unit_of(o.need("unit", "operators.ports_in.unit")?, "operators.ports_in.unit")?;
    let arity = arity_of(o.need("arity", "operators.ports_in.arity")?, "operators.ports_in.arity")?;
    o.finish()?;
    Ok(InPort { name, quantity_kind, unit, arity })
}

fn out_port_of(j: Json) -> Result<OutPort, SchemaError> {
    let mut o = Obj::of(j, "operators.ports_out")?;
    let name = string(o.need("name", "operators.ports_out.name")?, "operators.ports_out.name")?;
    let quantity_kind =
        string(o.need("quantity_kind", "operators.ports_out.quantity_kind")?, "operators.ports_out.quantity_kind")?;
    let unit = unit_of(o.need("unit", "operators.ports_out.unit")?, "operators.ports_out.unit")?;
    o.finish()?;
    Ok(OutPort { name, quantity_kind, unit })
}

fn operator_of(j: Json) -> Result<OperatorNode, SchemaError> {
    let mut o = Obj::of(j, "operators")?;
    let id = string(o.need("id", "operators.id")?, "operators.id")?;
    let algorithm_id = string(o.need("algorithm_id", "operators.algorithm_id")?, "operators.algorithm_id")?;
    let algorithm_version =
        string(o.need("algorithm_version", "operators.algorithm_version")?, "operators.algorithm_version")?;
    let params_digest = digest_of(o.need("params_digest", "operators.params_digest")?, "operators.params_digest")?;
    let ports_in = array(o.need("ports_in", "operators.ports_in")?, "operators.ports_in")?
        .into_iter()
        .map(in_port_of)
        .collect::<Result<_, _>>()?;
    let ports_out = array(o.need("ports_out", "operators.ports_out")?, "operators.ports_out")?
        .into_iter()
        .map(out_port_of)
        .collect::<Result<_, _>>()?;
    o.finish()?;
    Ok(OperatorNode { id, algorithm_id, algorithm_version, params_digest, ports_in, ports_out })
}

fn output_of(j: Json) -> Result<OutputNode, SchemaError> {
    let mut o = Obj::of(j, "outputs")?;
    let id = string(o.need("id", "outputs.id")?, "outputs.id")?;
    let quantity_kind = string(o.need("quantity_kind", "outputs.quantity_kind")?, "outputs.quantity_kind")?;
    let unit = unit_of(o.need("unit", "outputs.unit")?, "outputs.unit")?;
    let characteristic = o.take("characteristic").map(|c| string(c, "outputs.characteristic")).transpose()?;
    o.finish()?;
    Ok(OutputNode { id, quantity_kind, unit, characteristic })
}

fn endpoint_of(j: Json, path: &'static str, node: &'static str, port: &'static str) -> Result<Endpoint, SchemaError> {
    let mut o = Obj::of(j, path)?;
    let n = string(o.need("node", node)?, node)?;
    let p = string(o.need("port", port)?, port)?;
    o.finish()?;
    Ok(Endpoint { node: n, port: p })
}

fn edge_of(j: Json) -> Result<Edge, SchemaError> {
    let mut o = Obj::of(j, "edges")?;
    let from = endpoint_of(o.need("from", "edges.from")?, "edges.from", "edges.from.node", "edges.from.port")?;
    let to = endpoint_of(o.need("to", "edges.to")?, "edges.to", "edges.to.node", "edges.to.port")?;
    o.finish()?;
    Ok(Edge { from, to })
}

fn group_of(j: Json) -> Result<CorrelationGroup, SchemaError> {
    let mut o = Obj::of(j, "correlations")?;
    let inputs = array(o.need("inputs", "correlations.inputs")?, "correlations.inputs")?
        .into_iter()
        .map(|x| string(x, "correlations.inputs"))
        .collect::<Result<_, _>>()?;
    let matrix = array(o.need("matrix", "correlations.matrix")?, "correlations.matrix")?
        .into_iter()
        .map(|r| {
            array(r, "correlations.matrix")?
                .into_iter()
                .map(|x| f64_of(x, "correlations.matrix"))
                .collect::<Result<Vec<f64>, _>>()
        })
        .collect::<Result<_, _>>()?;
    o.finish()?;
    Ok(CorrelationGroup { inputs, matrix })
}

fn list<T>(
    o: &mut Obj,
    key: &str,
    field: &'static str,
    f: fn(Json) -> Result<T, SchemaError>,
) -> Result<Vec<T>, SchemaError> {
    array(o.need(key, field)?, field)?.into_iter().map(f).collect()
}

/// Parts from a read document, in document order (the caller sorts and checks them).
pub(crate) fn from_json(j: Json) -> Result<ModelParts, SchemaError> {
    let mut o = Obj::of(j, "model")?;
    if string(o.need("schema", "schema")?, "schema")? != SCHEMA {
        return Err(invalid("schema"));
    }
    let name = string(o.need("name", "name")?, "name")?;
    let semver = string(o.need("semver", "semver")?, "semver")?;
    let inputs = list(&mut o, "inputs", "inputs", input_of)?;
    let operators = list(&mut o, "operators", "operators", operator_of)?;
    let outputs = list(&mut o, "outputs", "outputs", output_of)?;
    let edges = list(&mut o, "edges", "edges", edge_of)?;
    let correlations = list(&mut o, "correlations", "correlations", group_of)?;
    o.finish()?;
    Ok(ModelParts { name, semver, inputs, operators, outputs, edges, correlations })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_bit_patterns_with_one_spelling() {
        assert_eq!(hex(1.0), Json::Str("3ff0000000000000".into()));
        assert_eq!(f64_of(Json::Str("3ff0000000000000".into()), "x"), Ok(1.0));
        for bad in ["3FF0000000000000", "3ff000000000000", "3ff00000000000000", "8000000000000000", "7ff8000000000000"]
        {
            assert_eq!(f64_of(Json::Str(bad.into()), "x"), Err(invalid("x")), "{bad}");
        }
        assert_eq!(f64_of(Json::Str("7ff0000000000000".into()), "x"), Err(invalid("x")));
    }

    #[test]
    fn arity_is_a_positive_plain_u32() {
        assert_eq!(arity_of(Json::Str("1".into()), "a"), Ok(1));
        assert_eq!(arity_of(Json::Str("4294967295".into()), "a"), Ok(u32::MAX));
        for bad in ["", "0", "01", "+1", "-1", "4294967296", " 1"] {
            assert_eq!(arity_of(Json::Str(bad.into()), "a"), Err(invalid("a")), "{bad:?}");
        }
    }
}
