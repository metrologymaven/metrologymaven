//! The `fid.cert@1` wire form: writing through `fid-kci`'s kernel canonical JSON writer, and a
//! bounded reader for exactly what that writer emits for a certificate (objects, arrays and
//! strings of printable ASCII without escapes or whitespace). Anything else is
//! `Malformed { detail }`.

use fid_kci::kci::json::{to_canonical, Json};
use fid_types::Digest;

use super::{CertFlags, CertKind, Certificate, KktRecord, RecordedCondition, ALGORITHM, SCHEMA};
use crate::cert::active::Thresholds;
use crate::cert::kkt::{KktScales, KktTolerances};
use crate::cert::recomp::RecompConfig;
use crate::cert::{CertError, RowLabel};

/// Largest serialised certificate read, in bytes.
pub(crate) const MAX_INPUT_BYTES: usize = 1 << 20;
/// Deepest nesting (the certificate needs 3).
const MAX_DEPTH: usize = 4;
/// Most members of one object or items of one array.
const MAX_ITEMS: usize = 4096;

fn bad(detail: &'static str) -> CertError {
    CertError::Malformed { detail }
}

fn s(v: &str) -> Json {
    Json::Str(v.to_owned())
}

fn hex(x: f64) -> Json {
    Json::Str(format!("{:016x}", x.to_bits()))
}

fn hexes(xs: &[f64]) -> Json {
    Json::Arr(xs.iter().map(|&x| hex(x)).collect())
}

fn boolean(b: bool) -> Json {
    s(if b { "true" } else { "false" })
}

fn obj(members: Vec<(&str, Json)>) -> Json {
    Json::Obj(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn condition(c: RecordedCondition) -> Json {
    obj(vec![("hi", hex(c.hi)), ("lo", hex(c.lo)), ("threshold", hex(c.threshold))])
}

/// The certificate as a JSON value; `with_digest` adds the `digest` member.
pub(crate) fn to_json(c: &Certificate, with_digest: bool) -> Json {
    let k = &c.kkt;
    let t = k.tolerances;
    let mut m = vec![
        ("active_set", Json::Arr(c.active_set.iter().map(|l| Json::Str(format!("{}:{}", l.point, l.side))).collect())),
        ("algorithm", s(ALGORITHM)),
        ("cond", hex(c.cond)),
        (
            "flags",
            obj(vec![
                ("local_optimum_only", boolean(c.flags.local_optimum_only)),
                ("tie_broken", boolean(c.flags.tie_broken)),
            ]),
        ),
        ("kind", s(c.kind.as_str())),
        (
            "kkt",
            obj(vec![
                ("complementarity", condition(k.complementarity)),
                ("dual", condition(k.dual)),
                ("dual_thresholds", hexes(&k.dual_thresholds)),
                ("primal", condition(k.primal)),
                (
                    "scales",
                    obj(vec![
                        ("data", hex(k.scales.data)),
                        ("gradient", hex(k.scales.gradient)),
                        ("gradient_lo", hex(k.scales.gradient_lo)),
                    ]),
                ),
                ("stationarity", condition(k.stationarity)),
                ("status", s("certified")),
                (
                    "tolerances",
                    obj(vec![
                        ("rank_tol", hex(t.rank_tol)),
                        ("tol_c", hex(t.tol_c)),
                        ("tol_d", hex(t.tol_d)),
                        ("tol_p", hex(t.tol_p)),
                        ("tol_s", hex(t.tol_s)),
                    ]),
                ),
            ]),
        ),
        ("multiplier_bounds", hexes(&c.multiplier_bounds)),
        ("multipliers", hexes(&c.multipliers)),
        ("param_bounds", hexes(&c.param_bounds)),
        ("params", hexes(&c.params)),
        (
            "recomp",
            obj(vec![
                ("cond_max", hex(c.recomp.cond_max)),
                ("gn_steps", Json::Str(c.recomp.gn_steps.to_string())),
                ("rel_tol", hex(c.recomp.rel_tol)),
            ]),
        ),
        ("schema", s(SCHEMA)),
        ("steps", Json::Str(c.steps.to_string())),
        ("thresholds", obj(vec![("mult_rel", hex(c.thresholds.mult_rel)), ("slack_rel", hex(c.thresholds.slack_rel))])),
        ("value", hex(c.value)),
        ("value_bound", hex(c.value_bound)),
    ];
    if with_digest {
        m.push(("digest", s(c.digest.as_str())));
    }
    obj(m)
}

/// Canonical bytes. The writer fails only on a duplicate key or nesting deeper than 128; the
/// certificate's keys are fixed and unique and its depth is 3, so an error is a defect.
pub(crate) fn canonical(v: &Json) -> Result<Vec<u8>, CertError> {
    to_canonical(v).map_err(|_| CertError::Inconsistent { detail: "canonical_json" })
}

// ------------------------------------------------------------------ reading

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), CertError> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(bad("syntax"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, CertError> {
        match self.peek() {
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'[') | Some(b'{') if depth >= MAX_DEPTH => Err(bad("too_deep")),
            Some(b'[') => self.array(depth + 1),
            Some(b'{') => self.object(depth + 1),
            Some(_) => Err(bad("unsupported_value")),
            None => Err(bad("syntax")),
        }
    }

    fn string(&mut self) -> Result<String, CertError> {
        self.expect(b'"')?;
        let start = self.i;
        loop {
            match self.peek() {
                Some(b'"') => break,
                Some(b'\\') => return Err(bad("escape")),
                Some(0x20..=0x7e) => self.i += 1,
                Some(_) => return Err(bad("not_ascii_text")),
                None => return Err(bad("syntax")),
            }
        }
        let text = self.b.get(start..self.i).ok_or_else(|| bad("syntax"))?;
        self.i += 1;
        std::str::from_utf8(text).map(str::to_owned).map_err(|_| bad("not_ascii_text"))
    }

    fn array(&mut self, depth: usize) -> Result<Json, CertError> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            if items.len() >= MAX_ITEMS {
                return Err(bad("too_many_items"));
            }
            items.push(self.value(depth)?);
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(bad("syntax")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, CertError> {
        self.expect(b'{')?;
        let mut members: Vec<(String, Json)> = Vec::new();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Obj(members));
        }
        loop {
            if members.len() >= MAX_ITEMS {
                return Err(bad("too_many_items"));
            }
            let k = self.string()?;
            self.expect(b':')?;
            let v = self.value(depth)?;
            members.push((k, v));
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    break;
                }
                _ => return Err(bad("syntax")),
            }
        }
        let mut keys: Vec<&str> = members.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort_unstable();
        if keys.windows(2).any(|w| matches!(w, [a, b] if a == b)) {
            return Err(bad("duplicate_key"));
        }
        Ok(Json::Obj(members))
    }
}

/// An object's members, checked to be exactly `keys` (unknown member first, then missing).
fn members<'a>(v: &'a Json, keys: &[&str]) -> Result<Fields<'a>, CertError> {
    let Json::Obj(m) = v else { return Err(bad("not_object")) };
    if m.iter().any(|(k, _)| !keys.contains(&k.as_str())) {
        return Err(bad("unknown_member"));
    }
    if keys.iter().any(|k| !m.iter().any(|(mk, _)| mk == k)) {
        return Err(bad("missing_member"));
    }
    if m.len() != keys.len() {
        return Err(bad("duplicate_key"));
    }
    Ok(Fields(m))
}

struct Fields<'a>(&'a [(String, Json)]);

impl<'a> Fields<'a> {
    fn get(&self, key: &str) -> Result<&'a Json, CertError> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v).ok_or_else(|| bad("missing_member"))
    }
    fn str(&self, key: &str) -> Result<&'a str, CertError> {
        match self.get(key)? {
            Json::Str(v) => Ok(v),
            _ => Err(bad("not_string")),
        }
    }
    fn f(&self, key: &str) -> Result<f64, CertError> {
        float(self.str(key)?)
    }
    fn fs(&self, key: &str) -> Result<Vec<f64>, CertError> {
        array(self.get(key)?)?.iter().map(|v| float(text(v)?)).collect()
    }
}

fn text(v: &Json) -> Result<&str, CertError> {
    match v {
        Json::Str(s) => Ok(s),
        _ => Err(bad("not_string")),
    }
}

fn array(v: &Json) -> Result<&[Json], CertError> {
    match v {
        Json::Arr(a) => Ok(a),
        _ => Err(bad("not_array")),
    }
}

/// Exactly 16 lowercase hex digits: the IEEE-754 bits.
fn float(s: &str) -> Result<f64, CertError> {
    let ok = s.len() == 16 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
    if !ok {
        return Err(bad("float"));
    }
    u64::from_str_radix(s, 16).map(f64::from_bits).map_err(|_| bad("float"))
}

/// A canonical decimal (no sign, no leading zero) that fits `T`.
fn decimal<T: core::str::FromStr>(s: &str) -> Result<T, CertError> {
    let ok = !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if !ok {
        return Err(bad("integer"));
    }
    s.parse().map_err(|_| bad("integer"))
}

fn boolean_from(s: &str) -> Result<bool, CertError> {
    match s {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(bad("boolean")),
    }
}

fn label(s: &str) -> Result<RowLabel, CertError> {
    let (p, sd) = s.split_once(':').ok_or_else(|| bad("label"))?;
    Ok(RowLabel { point: decimal(p)?, side: decimal(sd)? })
}

fn cond_from(v: &Json) -> Result<RecordedCondition, CertError> {
    let f = members(v, &["hi", "lo", "threshold"])?;
    Ok(RecordedCondition { lo: f.f("lo")?, hi: f.f("hi")?, threshold: f.f("threshold")? })
}

const TOP_KEYS: [&str; 17] = [
    "active_set",
    "algorithm",
    "cond",
    "digest",
    "flags",
    "kind",
    "kkt",
    "multiplier_bounds",
    "multipliers",
    "param_bounds",
    "params",
    "recomp",
    "schema",
    "steps",
    "thresholds",
    "value",
    "value_bound",
];

/// Decodes a serialised certificate (no consistency, digest or canonical-bytes check here).
pub(crate) fn from_bytes(input: &[u8]) -> Result<Certificate, CertError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(bad("too_large"));
    }
    let mut r = Reader { b: input, i: 0 };
    let v = r.value(0)?;
    if r.i != input.len() {
        return Err(bad("trailing_data"));
    }
    let top = members(&v, &TOP_KEYS)?;
    if top.str("schema")? != SCHEMA {
        return Err(bad("schema"));
    }
    if top.str("algorithm")? != ALGORITHM {
        return Err(bad("algorithm"));
    }
    let kind = CertKind::from_wire(top.str("kind")?).ok_or_else(|| bad("kind"))?;
    let active_set = array(top.get("active_set")?)?.iter().map(|v| label(text(v)?)).collect::<Result<_, _>>()?;
    let flags = members(top.get("flags")?, &["local_optimum_only", "tie_broken"])?;
    let flags = CertFlags {
        tie_broken: boolean_from(flags.str("tie_broken")?)?,
        local_optimum_only: boolean_from(flags.str("local_optimum_only")?)?,
    };
    let kkt = members(
        top.get("kkt")?,
        &["complementarity", "dual", "dual_thresholds", "primal", "scales", "stationarity", "status", "tolerances"],
    )?;
    if kkt.str("status")? != "certified" {
        return Err(bad("status"));
    }
    let sc = members(kkt.get("scales")?, &["data", "gradient", "gradient_lo"])?;
    let tl = members(kkt.get("tolerances")?, &["rank_tol", "tol_c", "tol_d", "tol_p", "tol_s"])?;
    let kkt = KktRecord {
        primal: cond_from(kkt.get("primal")?)?,
        dual: cond_from(kkt.get("dual")?)?,
        stationarity: cond_from(kkt.get("stationarity")?)?,
        complementarity: cond_from(kkt.get("complementarity")?)?,
        dual_thresholds: kkt.fs("dual_thresholds")?,
        tolerances: KktTolerances {
            tol_p: tl.f("tol_p")?,
            tol_d: tl.f("tol_d")?,
            tol_s: tl.f("tol_s")?,
            tol_c: tl.f("tol_c")?,
            rank_tol: tl.f("rank_tol")?,
        },
        scales: KktScales { data: sc.f("data")?, gradient_lo: sc.f("gradient_lo")?, gradient: sc.f("gradient")? },
    };
    let rc = members(top.get("recomp")?, &["cond_max", "gn_steps", "rel_tol"])?;
    let recomp = RecompConfig {
        rel_tol: rc.f("rel_tol")?,
        cond_max: rc.f("cond_max")?,
        gn_steps: decimal(rc.str("gn_steps")?)?,
    };
    let th = members(top.get("thresholds")?, &["mult_rel", "slack_rel"])?;
    let thresholds = Thresholds { slack_rel: th.f("slack_rel")?, mult_rel: th.f("mult_rel")? };
    let digest = Digest::parse(top.str("digest")?).map_err(|_| bad("digest"))?;
    Ok(Certificate {
        kind,
        active_set,
        multipliers: top.fs("multipliers")?,
        multiplier_bounds: top.fs("multiplier_bounds")?,
        kkt,
        thresholds,
        recomp,
        cond: top.f("cond")?,
        steps: decimal(top.str("steps")?)?,
        flags,
        params: top.fs("params")?,
        param_bounds: top.fs("param_bounds")?,
        value: top.f("value")?,
        value_bound: top.f("value_bound")?,
        digest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail(input: &str) -> Option<&'static str> {
        match from_bytes(input.as_bytes()) {
            Err(CertError::Malformed { detail }) => Some(detail),
            _ => None,
        }
    }

    #[test]
    fn refuses_what_the_writer_never_emits() {
        assert_eq!(detail(r#"{"a":1}"#), Some("unsupported_value"));
        assert_eq!(detail(r#"{"a":"\n"}"#), Some("escape"));
        assert_eq!(detail("{\"a\":\"\u{e9}\"}"), Some("not_ascii_text"));
        assert_eq!(detail(r#"{"a":"x","a":"y"}"#), Some("duplicate_key"));
        assert_eq!(detail(r#"{"a":"x"} "#), Some("trailing_data"));
        assert_eq!(detail(r#"{ "a":"x"}"#), Some("syntax"));
        assert_eq!(detail(r#"{"a":"x",}"#), Some("syntax"));
        assert_eq!(detail(r#"[[[[["x"]]]]]"#), Some("too_deep"));
        assert_eq!(detail(""), Some("syntax"));
        assert_eq!(detail(r#"{"a":"x"}"#), Some("unknown_member"));
        assert_eq!(detail("[]"), Some("not_object"));
    }

    #[test]
    fn scalar_decoders_are_strict() {
        assert_eq!(float("3ff0000000000000"), Ok(1.0));
        assert!(float("3FF0000000000000").is_err());
        assert!(float("3ff000000000000").is_err());
        assert!(float("+ff0000000000000").is_err());
        assert_eq!(decimal::<u32>("0"), Ok(0));
        assert_eq!(decimal::<u32>("17"), Ok(17));
        assert!(decimal::<u32>("017").is_err());
        assert!(decimal::<u32>("+1").is_err());
        assert!(decimal::<u8>("256").is_err());
        assert_eq!(label("4:1"), Ok(RowLabel { point: 4, side: 1 }));
        assert!(label("4").is_err());
        assert!(boolean_from("True").is_err());
    }
}
