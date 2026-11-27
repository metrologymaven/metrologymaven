//! The rule-set file format `fid.stats.ruleset/1` and its loader (REQ-RPT.STATS.SPC.RULES-003,
//! -007).
//!
//! A rule-set file is one JSON object:
//!
//! ```json
//! {"id":"western_electric","review":"UNVERIFIED","rules":[{"id":"1","primitive":"BEYOND","z":3},
//!  {"id":"2","k":2,"m":3,"primitive":"K_OF_M_BEYOND","z":2}],"schema":"fid.stats.ruleset/1","version":1}
//! ```
//!
//! The top level has exactly the keys `schema`, `id`, `version`, `review` and `rules`; each rule
//! has exactly `id`, `primitive` and the primitive's parameters: `BEYOND` `z`; `K_OF_M_BEYOND`
//! `k`, `m`, `z`; `RUN_SAME_SIDE`, `TREND` and `ALTERNATING` `len`; `RUN_WITHIN` and
//! `RUN_OUTSIDE` `len`, `z`. `k`, `m`, `len` and `version` are JSON integers (no fraction, no
//! exponent, no sign); `z` is any JSON number.
//!
//! The loader is bounded and fails closed with the first failing check, in this order:
//!
//! 1. more than [`MAX_RULESET_BYTES`] bytes: `too_large`;
//! 2. JSON syntax, read left to right: not UTF-8, a syntax error, a NaN or infinity literal, a
//!    string with a backslash escape or a control character, or bytes after the value:
//!    `malformed`; nesting deeper than [`MAX_DEPTH`]: `too_deep`; an object with a repeated key,
//!    detected when that object closes: `duplicate_key`;
//! 3. the top level: not an object `wrong_type`; a key outside the five (first in text order)
//!    `unknown_key`; a missing key (in the order `schema`, `id`, `version`, `review`, `rules`)
//!    `missing_key`; then `bad_schema`, `bad_id`, `bad_version`, `bad_review` (the review must
//!    be `UNVERIFIED`, `EXPERT_REVIEWED` or `CLAIMED_BY_FILE`; in a loaded file either of the
//!    last two becomes [`ReviewStatus::ClaimedByFile`], and only the shipped sets can carry
//!    [`ReviewStatus::ExpertReviewed`], D-045), `bad_rules`
//!    (not an array), `no_rules`, `too_many_rules`;
//! 4. each rule in order: not an object `bad_rule`; no `primitive` `missing_key`; `primitive`
//!    not a string `bad_rule`; not a known name `unknown_primitive`; a key outside the rule's set
//!    `unknown_key`; a missing key (`id`, then the parameters in the order above)
//!    `missing_key`; `bad_rule_id`; an ID used by an earlier rule `duplicate_rule_id`; then each
//!    parameter in order `bad_parameter` (wrong type, not an integer, or out of the range
//!    [`crate::rules::Primitive`] allows; `m < k` is reported at `m`).

use crate::rules::{
    rule_id_ok, set_id_ok, z_ok, Primitive, Rule, RuleSet, RulesetReason, MAX_RULES, MAX_RULESET_BYTES, MAX_VERSION,
    MAX_WINDOW, RULESET_SCHEMA,
};
use crate::spc::ReviewStatus;

/// The deepest nesting the loader reads (the format itself needs 3).
pub const MAX_DEPTH: usize = 8;

type R<T> = Result<T, RulesetReason>;

/// A parsed JSON value; strings and numbers borrow their text from the input.
enum Val<'a> {
    Obj(Vec<(&'a str, Val<'a>)>),
    Arr(Vec<Val<'a>>),
    Str(&'a str),
    Num(&'a str),
    Bool,
    Null,
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> R<()> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(RulesetReason::Malformed)
        }
    }

    fn word(&mut self, w: &str) -> R<()> {
        if self.b.get(self.i..).is_some_and(|rest| rest.starts_with(w.as_bytes())) {
            self.i += w.len();
            Ok(())
        } else {
            Err(RulesetReason::Malformed)
        }
    }

    fn value(&mut self, depth: usize) -> R<Val<'a>> {
        self.ws();
        match self.peek() {
            Some(b'{') => {
                if depth >= MAX_DEPTH {
                    return Err(RulesetReason::TooDeep);
                }
                self.i += 1;
                let mut members: Vec<(&'a str, Val<'a>)> = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(Val::Obj(members));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    self.eat(b':')?;
                    let v = self.value(depth + 1)?;
                    members.push((k, v));
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err(RulesetReason::Malformed),
                    }
                }
                for (n, (k, _)) in members.iter().enumerate() {
                    if members.iter().take(n).any(|(p, _)| p == k) {
                        return Err(RulesetReason::DuplicateKey);
                    }
                }
                Ok(Val::Obj(members))
            }
            Some(b'[') => {
                if depth >= MAX_DEPTH {
                    return Err(RulesetReason::TooDeep);
                }
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(Val::Arr(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Val::Arr(items));
                        }
                        _ => return Err(RulesetReason::Malformed),
                    }
                }
            }
            Some(b'"') => Ok(Val::Str(self.string()?)),
            Some(b't') => self.word("true").map(|()| Val::Bool),
            Some(b'f') => self.word("false").map(|()| Val::Bool),
            Some(b'n') => self.word("null").map(|()| Val::Null),
            Some(b'-' | b'0'..=b'9') => Ok(Val::Num(self.number()?)),
            _ => Err(RulesetReason::Malformed),
        }
    }

    /// A string without escapes or control characters.
    fn string(&mut self) -> R<&'a str> {
        self.eat(b'"')?;
        let start = self.i;
        loop {
            match self.peek() {
                Some(b'"') => break,
                Some(b'\\') | None => return Err(RulesetReason::Malformed),
                Some(c) if c < 0x20 => return Err(RulesetReason::Malformed),
                Some(_) => self.i += 1,
            }
        }
        let text = self.s.get(start..self.i).ok_or(RulesetReason::Malformed)?;
        self.i += 1;
        Ok(text)
    }

    fn digits(&mut self) -> usize {
        let start = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        self.i - start
    }

    /// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`
    fn number(&mut self) -> R<&'a str> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(RulesetReason::Malformed),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if self.digits() == 0 {
                return Err(RulesetReason::Malformed);
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return Err(RulesetReason::Malformed);
            }
        }
        self.s.get(start..self.i).ok_or(RulesetReason::Malformed)
    }
}

fn get<'v, 'a>(members: &'v [(&'a str, Val<'a>)], key: &str) -> Option<&'v Val<'a>> {
    members.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
}

/// Checks that `members` has only `allowed` keys and then every one of them, in that order.
fn keys(members: &[(&str, Val<'_>)], allowed: &[&str]) -> R<()> {
    if members.iter().any(|(k, _)| !allowed.contains(k)) {
        return Err(RulesetReason::UnknownKey);
    }
    if allowed.iter().any(|a| get(members, a).is_none()) {
        return Err(RulesetReason::MissingKey);
    }
    Ok(())
}

/// A JSON integer `0|[1-9][0-9]*` of at most 10 digits.
fn integer(v: Option<&Val<'_>>) -> Option<u64> {
    match v {
        Some(Val::Num(t)) if !t.is_empty() && t.len() <= 10 && t.bytes().all(|c| c.is_ascii_digit()) => {
            if t.len() > 1 && t.starts_with('0') {
                None
            } else {
                t.parse::<u64>().ok()
            }
        }
        _ => None,
    }
}

fn param_int(members: &[(&str, Val<'_>)], key: &str, lo: u32, hi: u32) -> R<u32> {
    match integer(get(members, key)) {
        Some(n) if (u64::from(lo)..=u64::from(hi)).contains(&n) => {
            u32::try_from(n).map_err(|_| RulesetReason::BadParameter)
        }
        _ => Err(RulesetReason::BadParameter),
    }
}

fn param_z(members: &[(&str, Val<'_>)]) -> R<f64> {
    match get(members, "z") {
        Some(Val::Num(t)) => match t.parse::<f64>() {
            Ok(z) if z_ok(z) => Ok(z),
            _ => Err(RulesetReason::BadParameter),
        },
        _ => Err(RulesetReason::BadParameter),
    }
}

/// The parameter keys of a primitive, in check order; `None` for an unknown name.
fn params_of(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "BEYOND" => &["z"],
        "K_OF_M_BEYOND" => &["k", "m", "z"],
        "RUN_SAME_SIDE" | "TREND" | "ALTERNATING" => &["len"],
        "RUN_WITHIN" | "RUN_OUTSIDE" => &["len", "z"],
        _ => return None,
    })
}

fn rule(v: &Val<'_>, earlier: &[Rule]) -> R<Rule> {
    let Val::Obj(members) = v else {
        return Err(RulesetReason::BadRule);
    };
    let name = match get(members, "primitive") {
        None => return Err(RulesetReason::MissingKey),
        Some(Val::Str(s)) => *s,
        Some(_) => return Err(RulesetReason::BadRule),
    };
    let params = params_of(name).ok_or(RulesetReason::UnknownPrimitive)?;
    let mut allowed: Vec<&str> = vec!["id", "primitive"];
    allowed.extend_from_slice(params);
    if members.iter().any(|(k, _)| !allowed.contains(k)) {
        return Err(RulesetReason::UnknownKey);
    }
    if get(members, "id").is_none() || params.iter().any(|p| get(members, p).is_none()) {
        return Err(RulesetReason::MissingKey);
    }
    let id = match get(members, "id") {
        Some(Val::Str(s)) if rule_id_ok(s) => *s,
        _ => return Err(RulesetReason::BadRuleId),
    };
    if earlier.iter().any(|r| r.id() == id) {
        return Err(RulesetReason::DuplicateRuleId);
    }
    let primitive = match name {
        "BEYOND" => Primitive::Beyond { z: param_z(members)? },
        "K_OF_M_BEYOND" => {
            let k = param_int(members, "k", 1, MAX_WINDOW)?;
            let m = param_int(members, "m", k, MAX_WINDOW)?;
            Primitive::KOfMBeyond { k, m, z: param_z(members)? }
        }
        "RUN_SAME_SIDE" => Primitive::RunSameSide { len: param_int(members, "len", 2, MAX_WINDOW)? },
        "TREND" => Primitive::Trend { len: param_int(members, "len", 3, MAX_WINDOW)? },
        "ALTERNATING" => Primitive::Alternating { len: param_int(members, "len", 3, MAX_WINDOW)? },
        "RUN_WITHIN" => {
            let len = param_int(members, "len", 2, MAX_WINDOW)?;
            Primitive::RunWithin { len, z: param_z(members)? }
        }
        "RUN_OUTSIDE" => {
            let len = param_int(members, "len", 2, MAX_WINDOW)?;
            Primitive::RunOutside { len, z: param_z(members)? }
        }
        _ => return Err(RulesetReason::UnknownPrimitive),
    };
    Rule::new(id, primitive).map_err(|_| RulesetReason::BadParameter)
}

/// Where a rule-set file comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    /// A file handed to [`RuleSet::parse`]: any review it claims becomes
    /// [`ReviewStatus::ClaimedByFile`].
    File,
    /// A set embedded in this crate ([`crate::rules::SHIPPED_RULE_SETS`]): the only source of
    /// [`ReviewStatus::ExpertReviewed`].
    Shipped,
}

/// Loads a rule-set file (see the module documentation for the order of checks).
pub(crate) fn parse(bytes: &[u8], origin: Origin) -> R<RuleSet> {
    if bytes.len() > MAX_RULESET_BYTES {
        return Err(RulesetReason::TooLarge);
    }
    let s = core::str::from_utf8(bytes).map_err(|_| RulesetReason::Malformed)?;
    let mut p = Parser { s, b: bytes, i: 0 };
    let root = p.value(0)?;
    p.ws();
    if p.i != bytes.len() {
        return Err(RulesetReason::Malformed);
    }
    let Val::Obj(members) = &root else {
        return Err(RulesetReason::WrongType);
    };
    keys(members, &["schema", "id", "version", "review", "rules"])?;
    match get(members, "schema") {
        Some(Val::Str(s)) if *s == RULESET_SCHEMA => {}
        _ => return Err(RulesetReason::BadSchema),
    }
    let id = match get(members, "id") {
        Some(Val::Str(s)) if set_id_ok(s) => *s,
        _ => return Err(RulesetReason::BadId),
    };
    let version = match integer(get(members, "version")) {
        Some(v) if (1..=u64::from(MAX_VERSION)).contains(&v) => {
            u32::try_from(v).map_err(|_| RulesetReason::BadVersion)?
        }
        _ => return Err(RulesetReason::BadVersion),
    };
    // D-045: a review claimed by a loaded file stays distinguishable from reviewed shipped data.
    let review = match (get(members, "review"), origin) {
        (Some(Val::Str("UNVERIFIED")), _) => ReviewStatus::Unverified,
        (Some(Val::Str("EXPERT_REVIEWED")), Origin::Shipped) => ReviewStatus::ExpertReviewed,
        (Some(Val::Str("EXPERT_REVIEWED" | "CLAIMED_BY_FILE")), _) => ReviewStatus::ClaimedByFile,
        _ => return Err(RulesetReason::BadReview),
    };
    let items = match get(members, "rules") {
        Some(Val::Arr(items)) => items,
        _ => return Err(RulesetReason::BadRules),
    };
    if items.is_empty() {
        return Err(RulesetReason::NoRules);
    }
    if items.len() > MAX_RULES {
        return Err(RulesetReason::TooManyRules);
    }
    let mut rules: Vec<Rule> = Vec::with_capacity(items.len());
    for v in items {
        let r = rule(v, &rules)?;
        rules.push(r);
    }
    RuleSet::build(id, version, review, rules, origin == Origin::Shipped).map_err(|e| match e {
        crate::StatsError::RulesetInvalid { reason } => reason,
        _ => RulesetReason::Malformed,
    })
}

/// The canonical JSON of a set (sorted keys, no whitespace).
pub(crate) fn encode(set: &RuleSet) -> String {
    let mut out = String::new();
    out.push_str("{\"id\":\"");
    out.push_str(set.id());
    out.push_str("\",\"review\":\"");
    out.push_str(set.review().as_str());
    out.push_str("\",\"rules\":[");
    for (n, r) in set.rules().iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":\"");
        out.push_str(r.id());
        out.push('"');
        let p = r.primitive();
        // Keys in sorted order: id, k, len, m, primitive, z.
        if let Primitive::KOfMBeyond { k, .. } = p {
            out.push_str(&format!(",\"k\":{k}"));
        }
        match p {
            Primitive::RunSameSide { len }
            | Primitive::Trend { len }
            | Primitive::Alternating { len }
            | Primitive::RunWithin { len, .. }
            | Primitive::RunOutside { len, .. } => out.push_str(&format!(",\"len\":{len}")),
            Primitive::Beyond { .. } | Primitive::KOfMBeyond { .. } => {}
        }
        if let Primitive::KOfMBeyond { m, .. } = p {
            out.push_str(&format!(",\"m\":{m}"));
        }
        out.push_str(",\"primitive\":\"");
        out.push_str(p.name());
        out.push('"');
        if let Some(z) = p.z() {
            // `Display` of an f64 is the shortest decimal that reads back as the same value and
            // never uses an exponent; `z` is finite and positive, so never `-0`.
            out.push_str(&format!(",\"z\":{z}"));
        }
        out.push('}');
    }
    out.push_str("],\"schema\":\"");
    out.push_str(RULESET_SCHEMA);
    out.push_str(&format!("\",\"version\":{}}}", set.version()));
    out
}
