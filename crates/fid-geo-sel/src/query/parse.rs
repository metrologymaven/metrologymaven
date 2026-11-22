//! Bounded, iterative JSON parser and closed-schema v1 validator for semantic selection
//! queries (spec node `GEO.SEL.QUERY.GRAMMAR`).
//!
//! No recursion on untrusted depth: both JSON decoding and AST construction run on explicit
//! stacks bounded by [`MAX_QUERY_BYTES`], [`MAX_QUERY_DEPTH`], [`MAX_AST_NODES`] and
//! [`MAX_IDS_PER_LIST`]. Every refusal carries either `query_limit`, `bad_quantity`, or
//! `bad_query` with an RFC 6901 JSON pointer to the offending location.

use crate::model::{Domain, EntityId};
use crate::query::ast::{
    parse_entity_id, AttrChannel, Expr, NearRef, Predicate, QueryError, SelectionQuery, TopologyId, MAX_AST_NODES,
    MAX_IDS_PER_LIST, MAX_QUERY_BYTES, MAX_QUERY_DEPTH, QUERY_VERSION,
};
use fid_geo_frame::{AuthoredQuantity, Unit};
use std::collections::BTreeMap;

/// Escapes one RFC 6901 JSON pointer token (`~` -> `~0`, `/` -> `~1`).
fn escape_ptr_token(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    for c in token.chars() {
        match c {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            other => out.push(other),
        }
    }
    out
}

fn join_ptr(base: &str, token: &str) -> String {
    let esc = escape_ptr_token(token);
    let mut out = String::with_capacity(base.len() + 1 + esc.len());
    out.push_str(base);
    out.push('/');
    out.push_str(&esc);
    out
}

fn join_idx(base: &str, idx: usize) -> String {
    format!("{base}/{idx}")
}

fn bad(pointer: impl Into<String>, reason: impl Into<String>) -> QueryError {
    QueryError::BadQuery { pointer: pointer.into(), reason: reason.into() }
}

// ------------------------------------------------------------------ bounded iterative JSON parser

#[derive(Debug, Clone)]
enum JsonNum {
    U64(u64),
    I64(i64),
    F64(f64),
}

impl JsonNum {
    fn as_f64(&self) -> Option<f64> {
        let v = match *self {
            JsonNum::U64(u) => u as f64,
            JsonNum::I64(i) => i as f64,
            JsonNum::F64(f) => f,
        };
        v.is_finite().then_some(v)
    }

    fn as_u64(&self) -> Option<u64> {
        match *self {
            JsonNum::U64(u) => Some(u),
            JsonNum::I64(i) if i >= 0 => u64::try_from(i).ok(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
enum JsonVal {
    Null,
    Bool,
    Num(JsonNum),
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(BTreeMap<String, JsonVal>),
}

enum JsonFrame {
    Arr(Vec<JsonVal>),
    Obj { map: BTreeMap<String, JsonVal>, key: Option<String>, ptr: String },
}

struct JsonLexer<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> JsonLexer<'a> {
    fn skip_ws(&mut self) {
        while let Some(&b) = self.src.get(self.pos) {
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.src.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn parse_string(&mut self, ptr: &str) -> Result<String, QueryError> {
        if self.bump() != Some(b'"') {
            return Err(bad(ptr, "expected '\"'"));
        }
        let mut out = String::new();
        while let Some(b) = self.bump() {
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = self.bump().ok_or_else(|| bad(ptr, "unterminated escape"))?;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let code = self.parse_u16_hex(ptr)?;
                            if (0xD800..=0xDBFF).contains(&code) {
                                // High surrogate must be followed by \uXXXX low surrogate.
                                if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                                    return Err(bad(ptr, "unpaired high surrogate"));
                                }
                                let low = self.parse_u16_hex(ptr)?;
                                if !(0xDC00..=0xDFFF).contains(&low) {
                                    return Err(bad(ptr, "invalid low surrogate"));
                                }
                                let combined =
                                    0x10000 + (((u32::from(code) - 0xD800) << 10) | (u32::from(low) - 0xDC00));
                                let ch = char::from_u32(combined).ok_or_else(|| bad(ptr, "invalid unicode scalar"))?;
                                out.push(ch);
                            } else if (0xDC00..=0xDFFF).contains(&code) {
                                return Err(bad(ptr, "unpaired low surrogate"));
                            } else {
                                let ch = char::from_u32(u32::from(code))
                                    .ok_or_else(|| bad(ptr, "invalid unicode scalar"))?;
                                out.push(ch);
                            }
                        }
                        _ => return Err(bad(ptr, "invalid escape sequence")),
                    }
                }
                c if c < 0x20 => return Err(bad(ptr, "unescaped control character in string")),
                c if c < 0x80 => out.push(char::from(c)),
                _ => {
                    // Multi-byte UTF-8 sequence (entire slice already validated as UTF-8).
                    let start = self.pos - 1;
                    while let Some(&next) = self.src.get(self.pos) {
                        if (next & 0xC0) == 0x80 {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    let slice = self.src.get(start..self.pos).ok_or_else(|| bad(ptr, "invalid utf-8"))?;
                    let s = core::str::from_utf8(slice).map_err(|_| bad(ptr, "invalid utf-8"))?;
                    out.push_str(s);
                }
            }
        }
        Err(bad(ptr, "unterminated string"))
    }

    fn parse_u16_hex(&mut self, ptr: &str) -> Result<u16, QueryError> {
        let mut val = 0u16;
        for _ in 0..4 {
            let b = self.bump().ok_or_else(|| bad(ptr, "truncated \\u escape"))?;
            let d = match b {
                b'0'..=b'9' => u16::from(b - b'0'),
                b'a'..=b'f' => u16::from(b - b'a' + 10),
                b'A'..=b'F' => u16::from(b - b'A' + 10),
                _ => return Err(bad(ptr, "invalid hex digit in \\u escape")),
            };
            val = (val << 4) | d;
        }
        Ok(val)
    }

    fn parse_number(&mut self, ptr: &str) -> Result<JsonNum, QueryError> {
        let start = self.pos;
        let neg = if self.peek() == Some(b'-') {
            self.pos += 1;
            true
        } else {
            false
        };
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
            }
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(bad(ptr, "invalid number")),
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(bad(ptr, "expected digit after decimal point"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(bad(ptr, "expected digit in exponent"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let raw = self.src.get(start..self.pos).ok_or_else(|| bad(ptr, "number slice"))?;
        let text = core::str::from_utf8(raw).map_err(|_| bad(ptr, "invalid utf-8"))?;
        if !is_float {
            if !neg {
                if let Ok(u) = text.parse::<u64>() {
                    return Ok(JsonNum::U64(u));
                }
            } else if let Ok(i) = text.parse::<i64>() {
                return Ok(JsonNum::I64(i));
            }
        }
        let f: f64 = text.parse().map_err(|_| bad(ptr, "invalid number"))?;
        if !f.is_finite() {
            return Err(bad(ptr, "non-finite number"));
        }
        Ok(JsonNum::F64(f))
    }

    fn expect_literal(&mut self, lit: &[u8], ptr: &str) -> Result<(), QueryError> {
        let end = self.pos.checked_add(lit.len()).ok_or_else(|| bad(ptr, "unexpected eof"))?;
        if self.src.get(self.pos..end) != Some(lit) {
            return Err(bad(ptr, "invalid literal"));
        }
        self.pos = end;
        Ok(())
    }
}

fn current_ptr(stack: &[JsonFrame]) -> String {
    match stack.last() {
        None => String::new(),
        Some(JsonFrame::Obj { ptr, key: Some(k), .. }) => join_ptr(ptr, k),
        Some(JsonFrame::Obj { ptr, key: None, .. }) => ptr.clone(),
        Some(JsonFrame::Arr(items)) => {
            // Reconstruct path only on error; walk from bottom.
            let mut p = String::new();
            for frame in stack {
                match frame {
                    JsonFrame::Obj { key: Some(k), .. } => p = join_ptr(&p, k),
                    JsonFrame::Obj { key: None, .. } => {}
                    JsonFrame::Arr(v) => p = join_idx(&p, v.len()),
                }
            }
            let _ = items;
            p
        }
    }
}

fn parse_json_bounded(input: &[u8], max_bytes: usize, max_depth: usize) -> Result<JsonVal, QueryError> {
    if input.len() > max_bytes {
        return Err(QueryError::QueryLimit { reason: "input exceeds 64 KiB" });
    }
    if core::str::from_utf8(input).is_err() {
        return Err(bad("", "input is not valid UTF-8"));
    }
    let mut lex = JsonLexer { src: input, pos: 0 };
    let mut stack: Vec<JsonFrame> = Vec::new();
    let mut expect_value = true;
    let mut root: Option<JsonVal> = None;

    loop {
        lex.skip_ws();
        if expect_value {
            let ptr = current_ptr(&stack);
            let b = lex.peek().ok_or_else(|| bad(&ptr, "unexpected end of input"))?;
            let completed: Option<JsonVal> = match b {
                b'{' => {
                    lex.pos += 1;
                    if stack.len() + 1 > max_depth {
                        return Err(QueryError::QueryLimit { reason: "nesting depth exceeds 32" });
                    }
                    lex.skip_ws();
                    if lex.peek() == Some(b'}') {
                        lex.pos += 1;
                        Some(JsonVal::Obj(BTreeMap::new()))
                    } else {
                        let k = lex.parse_string(&ptr)?;
                        lex.skip_ws();
                        if lex.bump() != Some(b':') {
                            return Err(bad(join_ptr(&ptr, &k), "expected ':' after object key"));
                        }
                        stack.push(JsonFrame::Obj { map: BTreeMap::new(), key: Some(k), ptr });
                        expect_value = true;
                        None
                    }
                }
                b'[' => {
                    lex.pos += 1;
                    if stack.len() + 1 > max_depth {
                        return Err(QueryError::QueryLimit { reason: "nesting depth exceeds 32" });
                    }
                    lex.skip_ws();
                    if lex.peek() == Some(b']') {
                        lex.pos += 1;
                        Some(JsonVal::Arr(Vec::new()))
                    } else {
                        stack.push(JsonFrame::Arr(Vec::new()));
                        expect_value = true;
                        None
                    }
                }
                b'"' => Some(JsonVal::Str(lex.parse_string(&ptr)?)),
                b't' => {
                    lex.expect_literal(b"true", &ptr)?;
                    Some(JsonVal::Bool)
                }
                b'f' => {
                    lex.expect_literal(b"false", &ptr)?;
                    Some(JsonVal::Bool)
                }
                b'n' => {
                    lex.expect_literal(b"null", &ptr)?;
                    Some(JsonVal::Null)
                }
                b'-' | b'0'..=b'9' => Some(JsonVal::Num(lex.parse_number(&ptr)?)),
                _ => return Err(bad(&ptr, "unexpected token")),
            };

            if let Some(mut val) = completed {
                loop {
                    match stack.last_mut() {
                        None => {
                            root = Some(val);
                            expect_value = false;
                            break;
                        }
                        Some(JsonFrame::Arr(items)) => {
                            items.push(val);
                            lex.skip_ws();
                            match lex.peek() {
                                Some(b',') => {
                                    lex.pos += 1;
                                    expect_value = true;
                                    break;
                                }
                                Some(b']') => {
                                    lex.pos += 1;
                                    let Some(JsonFrame::Arr(done)) = stack.pop() else {
                                        return Err(bad("", "internal parser state"));
                                    };
                                    val = JsonVal::Arr(done);
                                }
                                _ => return Err(bad(current_ptr(&stack), "expected ',' or ']'")),
                            }
                        }
                        Some(JsonFrame::Obj { map, key, ptr }) => {
                            let Some(k) = key.take() else {
                                return Err(bad(ptr.as_str(), "internal parser key"));
                            };
                            if map.contains_key(&k) {
                                return Err(bad(join_ptr(ptr, &k), "duplicate key"));
                            }
                            map.insert(k, val);
                            lex.skip_ws();
                            match lex.peek() {
                                Some(b',') => {
                                    lex.pos += 1;
                                    lex.skip_ws();
                                    let obj_ptr = ptr.clone();
                                    let next_k = lex.parse_string(&obj_ptr)?;
                                    lex.skip_ws();
                                    if lex.bump() != Some(b':') {
                                        return Err(bad(join_ptr(&obj_ptr, &next_k), "expected ':' after object key"));
                                    }
                                    *key = Some(next_k);
                                    expect_value = true;
                                    break;
                                }
                                Some(b'}') => {
                                    lex.pos += 1;
                                    let Some(JsonFrame::Obj { map: done, .. }) = stack.pop() else {
                                        return Err(bad("", "internal parser state"));
                                    };
                                    val = JsonVal::Obj(done);
                                }
                                _ => return Err(bad(ptr.as_str(), "expected ',' or '}'")),
                            }
                        }
                    }
                }
            }
        } else {
            break;
        }
    }

    lex.skip_ws();
    if lex.pos != lex.src.len() {
        return Err(bad("", "trailing characters after JSON value"));
    }
    root.ok_or_else(|| bad("", "empty JSON input"))
}

// ------------------------------------------------------------------ closed schema v1 validation

fn expect_obj<'a>(v: &'a JsonVal, ptr: &str) -> Result<&'a BTreeMap<String, JsonVal>, QueryError> {
    match v {
        JsonVal::Obj(m) => Ok(m),
        _ => Err(bad(ptr, "expected object")),
    }
}

fn check_allowed_keys(obj: &BTreeMap<String, JsonVal>, allowed: &[&str], ptr: &str) -> Result<(), QueryError> {
    for k in obj.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(bad(join_ptr(ptr, k), "unknown key"));
        }
    }
    Ok(())
}

fn req_field<'a>(obj: &'a BTreeMap<String, JsonVal>, key: &str, ptr: &str) -> Result<&'a JsonVal, QueryError> {
    obj.get(key).ok_or_else(|| bad(join_ptr(ptr, key), "missing required field"))
}

fn expect_str<'a>(v: &'a JsonVal, ptr: &str) -> Result<&'a str, QueryError> {
    match v {
        JsonVal::Str(s) => Ok(s),
        _ => Err(bad(ptr, "expected string")),
    }
}

fn expect_uuid(v: &JsonVal, ptr: &str) -> Result<EntityId, QueryError> {
    let s = expect_str(v, ptr)?;
    parse_entity_id(s).ok_or_else(|| bad(ptr, "expected canonical lowercase UUID"))
}

fn expect_topology_id(v: &JsonVal, ptr: &str) -> Result<TopologyId, QueryError> {
    let s = expect_str(v, ptr)?;
    TopologyId::new(s.to_string()).ok_or_else(|| bad(ptr, "topology id must be 1..=256 UTF-8 bytes"))
}

fn expect_f64(v: &JsonVal, ptr: &str) -> Result<f64, QueryError> {
    match v {
        JsonVal::Num(n) => n.as_f64().ok_or_else(|| bad(ptr, "non-finite number")),
        _ => Err(bad(ptr, "expected number")),
    }
}

fn expect_u64(v: &JsonVal, ptr: &str) -> Result<u64, QueryError> {
    match v {
        JsonVal::Num(n) => n.as_u64().ok_or_else(|| bad(ptr, "expected non-negative integer")),
        _ => Err(bad(ptr, "expected non-negative integer")),
    }
}

fn expect_u16(v: &JsonVal, ptr: &str) -> Result<u16, QueryError> {
    let u = expect_u64(v, ptr)?;
    u16::try_from(u).map_err(|_| bad(ptr, "integer out of u16 range (0..=65535)"))
}

#[derive(Clone, Copy)]
enum QtyKind {
    Length,
    Angle,
}

fn expect_quantity(v: &JsonVal, kind: QtyKind, ptr: &str) -> Result<AuthoredQuantity, QueryError> {
    let obj = expect_obj(v, ptr)?;
    check_allowed_keys(obj, &["si", "text", "unit"], ptr)?;
    let text = expect_str(req_field(obj, "text", ptr)?, &join_ptr(ptr, "text"))?;
    let unit_str = expect_str(req_field(obj, "unit", ptr)?, &join_ptr(ptr, "unit"))?;
    let unit = Unit::from_symbol(unit_str).ok_or_else(|| bad(join_ptr(ptr, "unit"), "unknown unit symbol"))?;
    match (kind, unit) {
        (QtyKind::Length, Unit::Length(_)) | (QtyKind::Angle, Unit::Angle(_)) => {}
        (QtyKind::Length, Unit::Angle(_)) => return Err(bad(join_ptr(ptr, "unit"), "expected length unit")),
        (QtyKind::Angle, Unit::Length(_)) => return Err(bad(join_ptr(ptr, "unit"), "expected angle unit")),
    }
    let q = AuthoredQuantity::parse(text, unit)
        .map_err(|error| QueryError::BadQuantity { pointer: ptr.to_string(), error })?;
    // If optional canonical `si` hex bits are present, verify they match the converted SI bits.
    if let Some(si_val) = obj.get("si") {
        let si_ptr = join_ptr(ptr, "si");
        let si_s = expect_str(si_val, &si_ptr)?;
        let expected = format!("0x{}", q.si_hex());
        if si_s != expected {
            return Err(bad(si_ptr, "si bits do not match authored text and unit"));
        }
    }
    Ok(q)
}

fn expect_length3(v: &JsonVal, ptr: &str) -> Result<[AuthoredQuantity; 3], QueryError> {
    let JsonVal::Arr(arr) = v else { return Err(bad(ptr, "expected 3-element array of lengths")) };
    if arr.len() != 3 {
        return Err(bad(ptr, "expected 3-element array of lengths"));
    }
    let q0 = expect_quantity(&arr[0], QtyKind::Length, &join_idx(ptr, 0))?;
    let q1 = expect_quantity(&arr[1], QtyKind::Length, &join_idx(ptr, 1))?;
    let q2 = expect_quantity(&arr[2], QtyKind::Length, &join_idx(ptr, 2))?;
    Ok([q0, q1, q2])
}

fn expect_near_ref(v: &JsonVal, ptr: &str) -> Result<NearRef, QueryError> {
    let obj = expect_obj(v, ptr)?;
    if obj.len() != 1 {
        return Err(bad(ptr, "near.of must contain exactly one reference key"));
    }
    let Some((k, val)) = obj.iter().next() else {
        return Err(bad(ptr, "near.of must contain exactly one reference key"));
    };
    let kptr = join_ptr(ptr, k);
    match k.as_str() {
        "element" => Ok(NearRef::Element(expect_uuid(val, &kptr)?)),
        "selection" => Ok(NearRef::Selection(expect_uuid(val, &kptr)?)),
        "face" => {
            let fobj = expect_obj(val, &kptr)?;
            check_allowed_keys(fobj, &["body", "id"], &kptr)?;
            let body = expect_uuid(req_field(fobj, "body", &kptr)?, &join_ptr(&kptr, "body"))?;
            let id = expect_topology_id(req_field(fobj, "id", &kptr)?, &join_ptr(&kptr, "id"))?;
            Ok(NearRef::Face { body, id })
        }
        _ => Err(bad(kptr, "unknown near.of reference kind")),
    }
}

/// Parses a non-recursive predicate (all predicates except `tangent_grow`, which pushes its
/// `seed` onto the explicit expression stack).
fn parse_leaf_predicate(name: &str, v: &JsonVal, ptr: &str) -> Result<Predicate, QueryError> {
    let obj = expect_obj(v, ptr)?;
    match name {
        "all" => {
            check_allowed_keys(obj, &[], ptr)?;
            Ok(Predicate::All)
        }
        "ids" => {
            check_allowed_keys(obj, &["ids"], ptr)?;
            let ids_ptr = join_ptr(ptr, "ids");
            let JsonVal::Arr(arr) = req_field(obj, "ids", ptr)? else {
                return Err(bad(ids_ptr, "expected array of u64 ids"));
            };
            if arr.len() > MAX_IDS_PER_LIST {
                return Err(QueryError::QueryLimit { reason: "id list exceeds 10 000 entries" });
            }
            let mut out = Vec::with_capacity(arr.len());
            for (i, item) in arr.iter().enumerate() {
                out.push(expect_u64(item, &join_idx(&ids_ptr, i))?);
            }
            Ok(Predicate::Ids(out))
        }
        "attr_range" => {
            check_allowed_keys(obj, &["channel", "max", "min"], ptr)?;
            let ch_ptr = join_ptr(ptr, "channel");
            let ch_s = expect_str(req_field(obj, "channel", ptr)?, &ch_ptr)?;
            let channel = AttrChannel::parse(ch_s).ok_or_else(|| bad(ch_ptr, "unknown attr_range channel"))?;
            let min = match obj.get("min") {
                None => None,
                Some(mv) => Some(expect_f64(mv, &join_ptr(ptr, "min"))?),
            };
            let max = match obj.get("max") {
                None => None,
                Some(mv) => Some(expect_f64(mv, &join_ptr(ptr, "max"))?),
            };
            if min.is_none() && max.is_none() {
                return Err(bad(ptr, "attr_range requires at least one of min or max"));
            }
            if let (Some(lo), Some(hi)) = (min, max) {
                if lo > hi {
                    return Err(bad(join_ptr(ptr, "max"), "attr_range max is less than min"));
                }
            }
            Ok(Predicate::AttrRange { channel, min, max })
        }
        "quality_flags" => {
            check_allowed_keys(obj, &["all_of", "none_of"], ptr)?;
            let all_of = match obj.get("all_of") {
                None => None,
                Some(av) => Some(expect_u16(av, &join_ptr(ptr, "all_of"))?),
            };
            let none_of = match obj.get("none_of") {
                None => None,
                Some(nv) => Some(expect_u16(nv, &join_ptr(ptr, "none_of"))?),
            };
            if all_of.is_none() && none_of.is_none() {
                return Err(bad(ptr, "quality_flags requires at least one of all_of or none_of"));
            }
            Ok(Predicate::QualityFlags { all_of, none_of })
        }
        "normal_cone" => {
            check_allowed_keys(obj, &["axis", "half_angle"], ptr)?;
            let axis_ptr = join_ptr(ptr, "axis");
            let JsonVal::Arr(arr) = req_field(obj, "axis", ptr)? else {
                return Err(bad(axis_ptr, "expected 3-element numeric axis array"));
            };
            if arr.len() != 3 {
                return Err(bad(axis_ptr, "expected 3-element numeric axis array"));
            }
            let a0 = expect_f64(&arr[0], &join_idx(&axis_ptr, 0))?;
            let a1 = expect_f64(&arr[1], &join_idx(&axis_ptr, 1))?;
            let a2 = expect_f64(&arr[2], &join_idx(&axis_ptr, 2))?;
            let sq_norm = a0 * a0 + a1 * a1 + a2 * a2;
            if !sq_norm.is_finite() || sq_norm == 0.0 {
                return Err(bad(axis_ptr, "normal_cone axis must be non-zero and finite"));
            }
            let ha_ptr = join_ptr(ptr, "half_angle");
            let half_angle = expect_quantity(req_field(obj, "half_angle", ptr)?, QtyKind::Angle, &ha_ptr)?;
            if half_angle.si() < 0.0 || half_angle.si() > core::f64::consts::PI {
                return Err(bad(ha_ptr, "half_angle must be within [0, pi] radians"));
            }
            Ok(Predicate::NormalCone { axis: [a0, a1, a2], half_angle })
        }
        "in_box" => {
            check_allowed_keys(obj, &["max", "min"], ptr)?;
            let min_ptr = join_ptr(ptr, "min");
            let max_ptr = join_ptr(ptr, "max");
            let min = expect_length3(req_field(obj, "min", ptr)?, &min_ptr)?;
            let max = expect_length3(req_field(obj, "max", ptr)?, &max_ptr)?;
            for i in 0..3 {
                if min[i].si() > max[i].si() {
                    return Err(bad(join_idx(&max_ptr, i), "in_box max component is less than min"));
                }
            }
            Ok(Predicate::InBox { min, max })
        }
        "in_sphere" => {
            check_allowed_keys(obj, &["centre", "radius"], ptr)?;
            let centre = expect_length3(req_field(obj, "centre", ptr)?, &join_ptr(ptr, "centre"))?;
            let rad_ptr = join_ptr(ptr, "radius");
            let radius = expect_quantity(req_field(obj, "radius", ptr)?, QtyKind::Length, &rad_ptr)?;
            if radius.si() < 0.0 {
                return Err(bad(rad_ptr, "in_sphere radius must be non-negative"));
            }
            Ok(Predicate::InSphere { centre, radius })
        }
        "near" => {
            check_allowed_keys(obj, &["distance", "of"], ptr)?;
            let of = expect_near_ref(req_field(obj, "of", ptr)?, &join_ptr(ptr, "of"))?;
            let dist_ptr = join_ptr(ptr, "distance");
            let distance = expect_quantity(req_field(obj, "distance", ptr)?, QtyKind::Length, &dist_ptr)?;
            if distance.si() < 0.0 {
                return Err(bad(dist_ptr, "near distance must be non-negative"));
            }
            Ok(Predicate::Near { of, distance })
        }
        "face_ids" => {
            check_allowed_keys(obj, &["body", "ids"], ptr)?;
            let body = expect_uuid(req_field(obj, "body", ptr)?, &join_ptr(ptr, "body"))?;
            let ids_ptr = join_ptr(ptr, "ids");
            let JsonVal::Arr(arr) = req_field(obj, "ids", ptr)? else {
                return Err(bad(ids_ptr, "expected array of topology ids"));
            };
            if arr.len() > MAX_IDS_PER_LIST {
                return Err(QueryError::QueryLimit { reason: "face_ids list exceeds 10 000 entries" });
            }
            let mut out = Vec::with_capacity(arr.len());
            for (i, item) in arr.iter().enumerate() {
                out.push(expect_topology_id(item, &join_idx(&ids_ptr, i))?);
            }
            Ok(Predicate::FaceIds { body, ids: out })
        }
        "in_group" => {
            check_allowed_keys(obj, &["group"], ptr)?;
            let group = expect_uuid(req_field(obj, "group", ptr)?, &join_ptr(ptr, "group"))?;
            Ok(Predicate::InGroup { group })
        }
        "in_layer" => {
            check_allowed_keys(obj, &["layer"], ptr)?;
            let layer = expect_uuid(req_field(obj, "layer", ptr)?, &join_ptr(ptr, "layer"))?;
            Ok(Predicate::InLayer { layer })
        }
        "in_body" => {
            check_allowed_keys(obj, &["body"], ptr)?;
            let body = expect_uuid(req_field(obj, "body", ptr)?, &join_ptr(ptr, "body"))?;
            Ok(Predicate::InBody { body })
        }
        _ => Err(bad(ptr, "unknown predicate")),
    }
}

enum ExprTask<'a> {
    Parse { val: &'a JsonVal, ptr: String },
    BuildAnd(usize),
    BuildOr(usize),
    BuildNot,
    BuildTangentGrow(AuthoredQuantity),
}

fn parse_expr_iterative(root_val: &JsonVal, root_ptr: &str) -> Result<Expr, QueryError> {
    let mut work: Vec<ExprTask<'_>> = vec![ExprTask::Parse { val: root_val, ptr: root_ptr.to_string() }];
    let mut built: Vec<Expr> = Vec::new();
    let mut node_count = 0usize;

    while let Some(task) = work.pop() {
        match task {
            ExprTask::Parse { val, ptr } => {
                node_count += 1;
                if node_count > MAX_AST_NODES {
                    return Err(QueryError::QueryLimit { reason: "AST node count exceeds 1 024" });
                }
                let obj = expect_obj(val, &ptr)?;
                if obj.len() != 1 {
                    return Err(bad(ptr, "expression object must have exactly one operator or predicate key"));
                }
                let Some((key, inner)) = obj.iter().next() else {
                    return Err(bad(ptr, "expression object must have exactly one key"));
                };
                let kptr = join_ptr(&ptr, key);
                match key.as_str() {
                    "and" => {
                        let JsonVal::Arr(arr) = inner else {
                            return Err(bad(kptr, "'and' operand must be an array"));
                        };
                        if arr.is_empty() {
                            return Err(bad(kptr, "'and' array must not be empty"));
                        }
                        work.push(ExprTask::BuildAnd(arr.len()));
                        for (i, item) in arr.iter().enumerate().rev() {
                            work.push(ExprTask::Parse { val: item, ptr: join_idx(&kptr, i) });
                        }
                    }
                    "or" => {
                        let JsonVal::Arr(arr) = inner else {
                            return Err(bad(kptr, "'or' operand must be an array"));
                        };
                        if arr.is_empty() {
                            return Err(bad(kptr, "'or' array must not be empty"));
                        }
                        work.push(ExprTask::BuildOr(arr.len()));
                        for (i, item) in arr.iter().enumerate().rev() {
                            work.push(ExprTask::Parse { val: item, ptr: join_idx(&kptr, i) });
                        }
                    }
                    "not" => {
                        work.push(ExprTask::BuildNot);
                        work.push(ExprTask::Parse { val: inner, ptr: kptr });
                    }
                    "tangent_grow" => {
                        let tg_obj = expect_obj(inner, &kptr)?;
                        check_allowed_keys(tg_obj, &["angle", "seed"], &kptr)?;
                        let ang_ptr = join_ptr(&kptr, "angle");
                        let angle = expect_quantity(req_field(tg_obj, "angle", &kptr)?, QtyKind::Angle, &ang_ptr)?;
                        if angle.si() < 0.0 || angle.si() > core::f64::consts::PI {
                            return Err(bad(ang_ptr, "tangent_grow angle must be within [0, pi] radians"));
                        }
                        let seed = req_field(tg_obj, "seed", &kptr)?;
                        work.push(ExprTask::BuildTangentGrow(angle));
                        work.push(ExprTask::Parse { val: seed, ptr: join_ptr(&kptr, "seed") });
                    }
                    other => {
                        let pred = parse_leaf_predicate(other, inner, &kptr)?;
                        built.push(Expr::Pred(pred));
                    }
                }
            }
            ExprTask::BuildAnd(n) => {
                let start = built.len().checked_sub(n).ok_or_else(|| bad("", "internal AST stack"))?;
                let items = built.split_off(start);
                built.push(Expr::And(items));
            }
            ExprTask::BuildOr(n) => {
                let start = built.len().checked_sub(n).ok_or_else(|| bad("", "internal AST stack"))?;
                let items = built.split_off(start);
                built.push(Expr::Or(items));
            }
            ExprTask::BuildNot => {
                let inner = built.pop().ok_or_else(|| bad("", "internal AST stack"))?;
                built.push(Expr::Not(Box::new(inner)));
            }
            ExprTask::BuildTangentGrow(angle) => {
                let seed = built.pop().ok_or_else(|| bad("", "internal AST stack"))?;
                built.push(Expr::Pred(Predicate::TangentGrow { seed: Box::new(seed), angle }));
            }
        }
    }

    if built.len() != 1 {
        return Err(bad("", "internal AST result"));
    }
    built.pop().ok_or_else(|| bad("", "internal AST result"))
}

fn parse_domain(v: &JsonVal, ptr: &str) -> Result<Domain, QueryError> {
    match expect_str(v, ptr)? {
        "points" => Ok(Domain::Points),
        "triangles" => Ok(Domain::Triangles),
        "faces" => Ok(Domain::Faces),
        "edges" => Ok(Domain::Edges),
        _ => Err(bad(ptr, "unknown domain (expected points, triangles, faces or edges)")),
    }
}

/// Parses and validates raw JSON bytes into a typed [`SelectionQuery`] (schema v1).
///
/// Enforces all REQ-GEO.SEL.QUERY.GRAMMAR-001..003 limits and closed-schema rules without
/// recursion on untrusted input depth.
pub fn parse_query(input: &[u8]) -> Result<SelectionQuery, QueryError> {
    let val = parse_json_bounded(input, MAX_QUERY_BYTES, MAX_QUERY_DEPTH)?;
    let top = expect_obj(&val, "")?;
    check_allowed_keys(top, &["domain", "v", "where"], "")?;
    let v_val = req_field(top, "v", "")?;
    let version = expect_u64(v_val, "/v")?;
    if version != QUERY_VERSION {
        return Err(bad("/v", "unsupported query version (expected 1)"));
    }
    let domain = parse_domain(req_field(top, "domain", "")?, "/domain")?;
    let root = parse_expr_iterative(req_field(top, "where", "")?, "/where")?;
    Ok(SelectionQuery { domain, root })
}

/// Parses the opaque bytes stored inside [`crate::model::SelectionQuery`].
pub fn parse_opaque_query(opaque: &crate::model::SelectionQuery) -> Result<SelectionQuery, QueryError> {
    parse_query(opaque.as_bytes())
}
