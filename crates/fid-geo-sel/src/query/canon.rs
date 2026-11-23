//! Canonicalisation, RFC 8785 canonical JSON encoding and SHA-256 digest for semantic
//! selection queries (spec node `GEO.SEL.QUERY.GRAMMAR`, REQ-004..006).
//!
//! All traversals are iterative (no recursion) and KNP-1 clean (no libm float calls, no
//! `HashMap`/`HashSet`, no `unsafe`, no panicking operations).

use crate::query::ast::{
    format_entity_id, Expr, NearRef, Predicate, QueryDigest, QueryError, SelectionQuery, QUERY_VERSION,
};
use crate::query::parse::parse_query;
use fid_geo_frame::AuthoredQuantity;
use fid_kci::kci::Sha256;
use fid_types::Digest;

/// Normalises `-0.0` to `+0.0` without float arithmetic.
fn plus_zero(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

/// Bitwise absolute value of a finite `f64` (no libm).
fn abs_bits(v: f64) -> f64 {
    f64::from_bits(v.to_bits() & !(1u64 << 63))
}

/// RFC 8785 §3.2.2.2 string serialisation.
fn write_json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{000c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// RFC 8785 §3.2.2.3: ECMAScript `Number::toString` for finite doubles (matches `fid-canon`).
pub fn format_jcs_number(v: f64) -> Result<String, QueryError> {
    if !v.is_finite() {
        return Err(QueryError::BadQuery { pointer: String::new(), reason: "non-finite number".into() });
    }
    if v == 0.0 {
        return Ok("0".to_string());
    }
    let a = abs_bits(v);
    let (shortest, exp) = sci_digits(&format!("{a:e}"))
        .ok_or_else(|| QueryError::BadQuery { pointer: String::new(), reason: "number formatting failed".into() })?;
    let digits = tie_to_even(a, &shortest, exp).unwrap_or(shortest);
    let k = digits.len() as i32;
    let n = exp + 1;
    let mut s = String::new();
    if v < 0.0 {
        s.push('-');
    }
    if k <= n && n <= 21 {
        s.push_str(&digits);
        s.push_str(&"0".repeat((n - k) as usize));
    } else if 0 < n && n <= 21 {
        let (hi, lo) = digits.split_at(n as usize);
        s.push_str(hi);
        s.push('.');
        s.push_str(lo);
    } else if -6 < n && n <= 0 {
        s.push_str("0.");
        s.push_str(&"0".repeat((-n) as usize));
        s.push_str(&digits);
    } else {
        let e = n - 1;
        let (first, rest) = digits.split_at(1);
        s.push_str(first);
        if !rest.is_empty() {
            s.push('.');
            s.push_str(rest);
        }
        s.push('e');
        s.push(if e < 0 { '-' } else { '+' });
        s.push_str(&e.unsigned_abs().to_string());
    }
    Ok(s)
}

fn sci_digits(sci: &str) -> Option<(String, i32)> {
    let (mant, exp) = sci.split_once('e')?;
    let exp: i32 = exp.parse().ok()?;
    Some((mant.chars().filter(|c| *c != '.').collect(), exp))
}

fn tie_to_even(a: f64, digits: &str, exp: i32) -> Option<String> {
    let k = digits.len();
    let last = *digits.as_bytes().last()?;
    if (last - b'0') % 2 == 0 {
        return None;
    }
    let (near, near_exp) = sci_digits(&format!("{a:.k$e}"))?;
    if near_exp != exp || !near.ends_with('5') {
        return None;
    }
    let stem = near.get(..k)?;
    let head = digits.get(..k - 1)?;
    let candidate = if stem == digits {
        if last == b'9' {
            return None;
        }
        format!("{head}{}", char::from(last + 1))
    } else if stem.get(..k - 1) == Some(head) && stem.as_bytes().last() == Some(&(last - 1)) {
        stem.to_string()
    } else {
        return None;
    };
    if candidate.starts_with('0') {
        return None;
    }
    let (full, full_exp) = sci_digits(&format!("{a:.800e}"))?;
    let tail = full.get(k + 1..)?;
    if full_exp != exp || !full.starts_with(&near) || !tail.bytes().all(|b| b == b'0') {
        return None;
    }
    let back: f64 = format!("{candidate}e{}", i64::from(exp) - k as i64 + 1).parse().ok()?;
    if back.to_bits() != a.to_bits() {
        return None;
    }
    Some(candidate.trim_end_matches('0').to_string())
}

fn write_quantity(out: &mut String, q: &AuthoredQuantity) {
    // Keys in RFC 8785 order: "si", "text", "unit" (REQ-005).
    out.push_str("{\"si\":\"0x");
    out.push_str(&q.si_hex());
    out.push_str("\",\"text\":");
    write_json_str(out, q.text());
    out.push_str(",\"unit\":");
    write_json_str(out, q.unit().symbol());
    out.push('}');
}

fn write_length3(out: &mut String, q: &[AuthoredQuantity; 3]) {
    out.push('[');
    write_quantity(out, &q[0]);
    out.push(',');
    write_quantity(out, &q[1]);
    out.push(',');
    write_quantity(out, &q[2]);
    out.push(']');
}

enum Emit<'a> {
    Raw(&'static str),
    Expr(&'a Expr),
}

/// Iteratively serialises an [`Expr`] (assumed already canonical) to RFC 8785 JSON bytes.
fn serialize_expr(expr: &Expr) -> Result<Vec<u8>, QueryError> {
    let mut out = String::new();
    let mut stack = vec![Emit::Expr(expr)];
    while let Some(item) = stack.pop() {
        match item {
            Emit::Raw(s) => out.push_str(s),
            Emit::Expr(e) => match e {
                Expr::And(children) => {
                    out.push_str("{\"and\":[");
                    stack.push(Emit::Raw("]}"));
                    for (i, child) in children.iter().enumerate().rev() {
                        stack.push(Emit::Expr(child));
                        if i > 0 {
                            stack.push(Emit::Raw(","));
                        }
                    }
                }
                Expr::Or(children) => {
                    out.push_str("{\"or\":[");
                    stack.push(Emit::Raw("]}"));
                    for (i, child) in children.iter().enumerate().rev() {
                        stack.push(Emit::Expr(child));
                        if i > 0 {
                            stack.push(Emit::Raw(","));
                        }
                    }
                }
                Expr::Not(inner) => {
                    out.push_str("{\"not\":");
                    stack.push(Emit::Raw("}"));
                    stack.push(Emit::Expr(inner));
                }
                Expr::Pred(pred) => match pred {
                    Predicate::All => out.push_str("{\"all\":{}}"),
                    Predicate::Ids(ids) => {
                        out.push_str("{\"ids\":{\"ids\":[");
                        for (i, id) in ids.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            out.push_str(&id.to_string());
                        }
                        out.push_str("]}}");
                    }
                    Predicate::AttrRange { channel, min, max } => {
                        // Keys in RFC 8785 order: "channel", "max", "min".
                        out.push_str("{\"attr_range\":{\"channel\":");
                        write_json_str(&mut out, channel.as_str());
                        if let Some(hi) = max {
                            out.push_str(",\"max\":");
                            out.push_str(&format_jcs_number(plus_zero(*hi))?);
                        }
                        if let Some(lo) = min {
                            out.push_str(",\"min\":");
                            out.push_str(&format_jcs_number(plus_zero(*lo))?);
                        }
                        out.push_str("}}");
                    }
                    Predicate::QualityFlags { all_of, none_of } => {
                        // Keys in RFC 8785 order: "all_of", "none_of".
                        out.push_str("{\"quality_flags\":{");
                        let mut first = true;
                        if let Some(a) = all_of {
                            out.push_str("\"all_of\":");
                            out.push_str(&a.to_string());
                            first = false;
                        }
                        if let Some(n) = none_of {
                            if !first {
                                out.push(',');
                            }
                            out.push_str("\"none_of\":");
                            out.push_str(&n.to_string());
                        }
                        out.push_str("}}");
                    }
                    Predicate::NormalCone { axis, half_angle } => {
                        // Keys: "axis", "half_angle".
                        out.push_str("{\"normal_cone\":{\"axis\":[");
                        for (i, c) in axis.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            out.push_str(&format_jcs_number(plus_zero(*c))?);
                        }
                        out.push_str("],\"half_angle\":");
                        write_quantity(&mut out, half_angle);
                        out.push_str("}}");
                    }
                    Predicate::InBox { min, max } => {
                        // Keys: "max", "min".
                        out.push_str("{\"in_box\":{\"max\":");
                        write_length3(&mut out, max);
                        out.push_str(",\"min\":");
                        write_length3(&mut out, min);
                        out.push_str("}}");
                    }
                    Predicate::InSphere { centre, radius } => {
                        // Keys: "centre", "radius".
                        out.push_str("{\"in_sphere\":{\"centre\":");
                        write_length3(&mut out, centre);
                        out.push_str(",\"radius\":");
                        write_quantity(&mut out, radius);
                        out.push_str("}}");
                    }
                    Predicate::Near { of, distance } => {
                        // Keys: "distance", "of".
                        out.push_str("{\"near\":{\"distance\":");
                        write_quantity(&mut out, distance);
                        out.push_str(",\"of\":");
                        match of {
                            NearRef::Element(id) => {
                                out.push_str("{\"element\":");
                                write_json_str(&mut out, &format_entity_id(*id));
                                out.push('}');
                            }
                            NearRef::Selection(id) => {
                                out.push_str("{\"selection\":");
                                write_json_str(&mut out, &format_entity_id(*id));
                                out.push('}');
                            }
                            NearRef::Face { body, id } => {
                                // Keys: "body", "id".
                                out.push_str("{\"face\":{\"body\":");
                                write_json_str(&mut out, &format_entity_id(*body));
                                out.push_str(",\"id\":");
                                write_json_str(&mut out, id.as_str());
                                out.push_str("}}");
                            }
                        }
                        out.push_str("}}");
                    }
                    Predicate::FaceIds { body, ids } => {
                        // Keys: "body", "ids".
                        out.push_str("{\"face_ids\":{\"body\":");
                        write_json_str(&mut out, &format_entity_id(*body));
                        out.push_str(",\"ids\":[");
                        for (i, tid) in ids.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            write_json_str(&mut out, tid.as_str());
                        }
                        out.push_str("]}}");
                    }
                    Predicate::InGroup { group } => {
                        out.push_str("{\"in_group\":{\"group\":");
                        write_json_str(&mut out, &format_entity_id(*group));
                        out.push_str("}}");
                    }
                    Predicate::InLayer { layer } => {
                        out.push_str("{\"in_layer\":{\"layer\":");
                        write_json_str(&mut out, &format_entity_id(*layer));
                        out.push_str("}}");
                    }
                    Predicate::InBody { body } => {
                        out.push_str("{\"in_body\":{\"body\":");
                        write_json_str(&mut out, &format_entity_id(*body));
                        out.push_str("}}");
                    }
                    Predicate::TangentGrow { seed, angle } => {
                        // Keys: "angle", "seed".
                        out.push_str("{\"tangent_grow\":{\"angle\":");
                        write_quantity(&mut out, angle);
                        out.push_str(",\"seed\":");
                        stack.push(Emit::Raw("}}"));
                        stack.push(Emit::Expr(seed));
                    }
                },
            },
        }
    }
    Ok(out.into_bytes())
}

// ------------------------------------------------------------------ iterative canonicalisation

enum CanonTask<'a> {
    Visit(&'a Expr),
    FinishAnd(usize),
    FinishOr(usize),
    FinishNot,
    FinishTangentGrow(AuthoredQuantity),
}

fn canonicalize_predicate(p: &Predicate) -> Predicate {
    match p {
        Predicate::All => Predicate::All,
        Predicate::Ids(ids) => {
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            sorted.dedup();
            Predicate::Ids(sorted)
        }
        Predicate::AttrRange { channel, min, max } => {
            Predicate::AttrRange { channel: *channel, min: min.map(plus_zero), max: max.map(plus_zero) }
        }
        Predicate::QualityFlags { all_of, none_of } => Predicate::QualityFlags { all_of: *all_of, none_of: *none_of },
        Predicate::NormalCone { axis, half_angle } => Predicate::NormalCone {
            axis: [plus_zero(axis[0]), plus_zero(axis[1]), plus_zero(axis[2])],
            half_angle: half_angle.clone(),
        },
        Predicate::InBox { min, max } => Predicate::InBox { min: min.clone(), max: max.clone() },
        Predicate::InSphere { centre, radius } => {
            Predicate::InSphere { centre: centre.clone(), radius: radius.clone() }
        }
        Predicate::Near { of, distance } => Predicate::Near { of: of.clone(), distance: distance.clone() },
        Predicate::FaceIds { body, ids } => {
            let mut sorted = ids.clone();
            sorted.sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
            sorted.dedup();
            Predicate::FaceIds { body: *body, ids: sorted }
        }
        Predicate::InGroup { group } => Predicate::InGroup { group: *group },
        Predicate::InLayer { layer } => Predicate::InLayer { layer: *layer },
        Predicate::InBody { body } => Predicate::InBody { body: *body },
        Predicate::TangentGrow { seed, angle } => {
            // Handled via CanonTask::FinishTangentGrow.
            Predicate::TangentGrow { seed: seed.clone(), angle: angle.clone() }
        }
    }
}

fn sort_and_dedup_operands(operands: Vec<Expr>) -> Result<Vec<Expr>, QueryError> {
    let mut keyed: Vec<(Vec<u8>, Expr)> = Vec::with_capacity(operands.len());
    for op in operands {
        let bytes = serialize_expr(&op)?;
        keyed.push((bytes, op));
    }
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    let mut deduped: Vec<Expr> = Vec::with_capacity(keyed.len());
    let mut last_bytes: Option<Vec<u8>> = None;
    for (bytes, op) in keyed {
        if last_bytes.as_deref() != Some(bytes.as_slice()) {
            last_bytes = Some(bytes);
            deduped.push(op);
        }
    }
    Ok(deduped)
}

/// Canonicalises an [`Expr`] bottom-up without recursion (REQ-004):
/// - flattens nested `and` / `or`;
/// - eliminates double `not`;
/// - sorts and deduplicates `and` / `or` operands by their canonical JSON bytes;
/// - collapses single-operand `and` / `or`;
/// - sorts and deduplicates `ids` and `face_ids`;
/// - normalises `-0.0` to `+0.0`.
pub fn canonicalize_expr(root: &Expr) -> Result<Expr, QueryError> {
    let mut work: Vec<CanonTask<'_>> = vec![CanonTask::Visit(root)];
    let mut out: Vec<Expr> = Vec::new();

    while let Some(task) = work.pop() {
        match task {
            CanonTask::Visit(expr) => match expr {
                Expr::And(items) => {
                    work.push(CanonTask::FinishAnd(items.len()));
                    for item in items.iter().rev() {
                        work.push(CanonTask::Visit(item));
                    }
                }
                Expr::Or(items) => {
                    work.push(CanonTask::FinishOr(items.len()));
                    for item in items.iter().rev() {
                        work.push(CanonTask::Visit(item));
                    }
                }
                Expr::Not(inner) => {
                    work.push(CanonTask::FinishNot);
                    work.push(CanonTask::Visit(inner));
                }
                Expr::Pred(Predicate::TangentGrow { seed, angle }) => {
                    work.push(CanonTask::FinishTangentGrow(angle.clone()));
                    work.push(CanonTask::Visit(seed));
                }
                Expr::Pred(p) => {
                    out.push(Expr::Pred(canonicalize_predicate(p)));
                }
            },
            CanonTask::FinishAnd(n) => {
                let start = out
                    .len()
                    .checked_sub(n)
                    .ok_or_else(|| QueryError::BadQuery { pointer: String::new(), reason: "canon stack".into() })?;
                let raw = out.split_off(start);
                let mut flat: Vec<Expr> = Vec::with_capacity(raw.len());
                for item in raw {
                    match item {
                        Expr::And(sub) => flat.extend(sub),
                        other => flat.push(other),
                    }
                }
                let deduped = sort_and_dedup_operands(flat)?;
                if deduped.len() == 1 {
                    if let Some(only) = deduped.into_iter().next() {
                        out.push(only);
                    }
                } else {
                    out.push(Expr::And(deduped));
                }
            }
            CanonTask::FinishOr(n) => {
                let start = out
                    .len()
                    .checked_sub(n)
                    .ok_or_else(|| QueryError::BadQuery { pointer: String::new(), reason: "canon stack".into() })?;
                let raw = out.split_off(start);
                let mut flat: Vec<Expr> = Vec::with_capacity(raw.len());
                for item in raw {
                    match item {
                        Expr::Or(sub) => flat.extend(sub),
                        other => flat.push(other),
                    }
                }
                let deduped = sort_and_dedup_operands(flat)?;
                if deduped.len() == 1 {
                    if let Some(only) = deduped.into_iter().next() {
                        out.push(only);
                    }
                } else {
                    out.push(Expr::Or(deduped));
                }
            }
            CanonTask::FinishNot => {
                let inner = out
                    .pop()
                    .ok_or_else(|| QueryError::BadQuery { pointer: String::new(), reason: "canon stack".into() })?;
                match inner {
                    Expr::Not(double_inner) => out.push(*double_inner),
                    other => out.push(Expr::Not(Box::new(other))),
                }
            }
            CanonTask::FinishTangentGrow(angle) => {
                let seed = out
                    .pop()
                    .ok_or_else(|| QueryError::BadQuery { pointer: String::new(), reason: "canon stack".into() })?;
                out.push(Expr::Pred(Predicate::TangentGrow { seed: Box::new(seed), angle }));
            }
        }
    }

    out.pop().ok_or_else(|| QueryError::BadQuery { pointer: String::new(), reason: "canon empty".into() })
}

/// Returns the canonicalised [`SelectionQuery`] (REQ-004).
pub fn canonicalize(query: &SelectionQuery) -> Result<SelectionQuery, QueryError> {
    Ok(SelectionQuery { domain: query.domain, root: canonicalize_expr(&query.root)? })
}

/// Canonicalises `query` and encodes it to RFC 8785 canonical JSON bytes (REQ-004..006).
pub fn to_canonical_bytes(query: &SelectionQuery) -> Result<Vec<u8>, QueryError> {
    let canon = canonicalize(query)?;
    let where_bytes = serialize_expr(&canon.root)?;
    let where_str = core::str::from_utf8(&where_bytes)
        .map_err(|_| QueryError::BadQuery { pointer: String::new(), reason: "utf-8".into() })?;
    // Top-level keys in RFC 8785 order: "domain", "v", "where".
    let mut out = String::with_capacity(where_str.len() + 48);
    out.push_str("{\"domain\":");
    write_json_str(&mut out, canon.domain.as_str());
    out.push_str(",\"v\":");
    out.push_str(&QUERY_VERSION.to_string());
    out.push_str(",\"where\":");
    out.push_str(where_str);
    out.push('}');
    Ok(out.into_bytes())
}

/// Computes the SHA-256 [`QueryDigest`] (`sha256:<64 hex>`) over already-canonical JSON bytes.
pub fn digest_canonical_bytes(bytes: &[u8]) -> QueryDigest {
    let mut h = Sha256::new();
    h.update(bytes);
    QueryDigest(Digest::from_bytes(h.finish()))
}

/// Canonicalises `query` and returns its [`QueryDigest`] (`sha256:<64 lowercase hex>`).
pub fn query_digest(query: &SelectionQuery) -> Result<QueryDigest, QueryError> {
    let bytes = to_canonical_bytes(query)?;
    Ok(digest_canonical_bytes(&bytes))
}

/// Parses raw JSON bytes, canonicalises the query, and returns `(canonical_query, canonical_bytes, digest)`.
pub fn parse_and_canonicalize(input: &[u8]) -> Result<(SelectionQuery, Vec<u8>, QueryDigest), QueryError> {
    let parsed = parse_query(input)?;
    let canon = canonicalize(&parsed)?;
    let bytes = to_canonical_bytes(&canon)?;
    let digest = digest_canonical_bytes(&bytes);
    Ok((canon, bytes, digest))
}

/// Encodes a [`SelectionQuery`] into the opaque [`crate::model::SelectionQuery`] wrapper
/// stored on [`crate::model::Selection`].
pub fn to_opaque_query(query: &SelectionQuery) -> Result<crate::model::SelectionQuery, QueryError> {
    let bytes = to_canonical_bytes(query)?;
    crate::model::SelectionQuery::from_bytes(bytes)
        .map_err(|_| QueryError::QueryLimit { reason: "canonical query > 64 KiB" })
}
