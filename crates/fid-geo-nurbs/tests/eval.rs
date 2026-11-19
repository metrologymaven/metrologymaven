//! GEO.NURBS.EVAL acceptance tests (AC-1 to AC-5) and error paths.
//!
//! Expected values come from outside the Rust code: `fixtures/geo/nurbs/ref_set_v1.json` (the
//! exact-rational GEO.NURBS.REF oracle), `fixtures/geo/nurbs/eval/digests-v1.txt` (the bit-exact
//! Python model of the order of operations pinned in NODE.md, written by
//! `fixtures/geo/nurbs/eval/make_eval_fixtures.py`), and hand-labelled cases below.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::OnceLock;

use fid_geo_nurbs::{
    Curve, Dir, EvalError, NurbsCurve, NurbsCurveParts, NurbsError, NurbsSurface, NurbsSurfaceParts, Regularity,
    Surface, MAX_ORDER,
};
use fid_testkit::sha256_hex;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/nurbs/");
const POS_TOL: f64 = 1e-12;
const REL_TOL: f64 = 1e-9;

// ---------------------------------------------------------------- minimal JSON reader

enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(BTreeMap<String, J>),
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn string(&mut self) -> String {
        assert_eq!(self.s[self.i], b'"');
        self.i += 1;
        let start = self.i;
        while self.s[self.i] != b'"' {
            assert_ne!(self.s[self.i], b'\\', "escapes are not used by the fixture");
            self.i += 1;
        }
        let out = std::str::from_utf8(&self.s[start..self.i]).unwrap().to_string();
        self.i += 1;
        out
    }
    fn value(&mut self) -> J {
        self.ws();
        let c = self.s[self.i];
        match c {
            b'"' => J::Str(self.string()),
            b'n' | b't' | b'f' => {
                for (w, v) in [("null", J::Null), ("true", J::Bool(true)), ("false", J::Bool(false))] {
                    if self.s[self.i..].starts_with(w.as_bytes()) {
                        self.i += w.len();
                        return v;
                    }
                }
                panic!("bad literal at {}", self.i)
            }
            b'[' | b'{' => {
                self.i += 1;
                let (mut arr, mut obj) = (Vec::new(), BTreeMap::new());
                let close = if c == b'[' { b']' } else { b'}' };
                loop {
                    self.ws();
                    if self.s[self.i] == close {
                        self.i += 1;
                        break;
                    }
                    if c == b'[' {
                        arr.push(self.value());
                    } else {
                        let k = self.string();
                        self.ws();
                        assert_eq!(self.s[self.i], b':');
                        self.i += 1;
                        assert!(obj.insert(k, self.value()).is_none(), "duplicate key");
                    }
                    self.ws();
                    if self.s[self.i] == b',' {
                        self.i += 1;
                    }
                }
                if c == b'[' {
                    J::Arr(arr)
                } else {
                    J::Obj(obj)
                }
            }
            _ => {
                let start = self.i;
                while matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                J::Num(std::str::from_utf8(&self.s[start..self.i]).unwrap().parse().unwrap())
            }
        }
    }
}

impl J {
    fn get(&self, k: &str) -> &J {
        match self {
            J::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing key {k}")),
            _ => panic!("not an object"),
        }
    }
    fn opt(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(m) => m.get(k),
            _ => None,
        }
    }
    fn arr(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            _ => panic!("not an array"),
        }
    }
    fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string"),
        }
    }
    fn usize(&self) -> usize {
        match self {
            J::Num(x) => *x as usize,
            _ => panic!("not a number"),
        }
    }
    fn bool(&self) -> bool {
        matches!(self, J::Bool(true))
    }
    fn is_null(&self) -> bool {
        matches!(self, J::Null)
    }
    /// `{hex, dec}` -> f64 from the bit pattern.
    fn f(&self) -> f64 {
        bits(self.get("hex"))
    }
    fn fvec(&self) -> Vec<f64> {
        self.get("hex").arr().iter().map(bits).collect()
    }
    fn v3(&self) -> [f64; 3] {
        let v = self.fvec();
        [v[0], v[1], v[2]]
    }
    fn pts(&self) -> Vec<[f64; 3]> {
        self.get("hex").arr().iter().map(|p| [bits(&p.arr()[0]), bits(&p.arr()[1]), bits(&p.arr()[2])]).collect()
    }
}

fn bits(j: &J) -> f64 {
    f64::from_bits(u64::from_str_radix(j.str().strip_prefix("0x").unwrap(), 16).unwrap())
}

fn ref_set() -> &'static [J] {
    static DOC: OnceLock<J> = OnceLock::new();
    DOC.get_or_init(|| {
        let bytes = std::fs::read(format!("{DIR}ref_set_v1.json")).unwrap();
        let mut p = Parser { s: &bytes, i: 0 };
        p.value()
    })
    .get("entities")
    .arr()
}

enum Built {
    Curve(NurbsCurve),
    Surface(NurbsSurface),
}

fn build(e: &J) -> Built {
    let weights = |j: &J| if j.is_null() { None } else { Some(j.fvec()) };
    if e.get("kind").str() == "curve" {
        Built::Curve(
            NurbsCurve::new(NurbsCurveParts {
                degree: e.get("degree").usize(),
                knots: e.get("knots").fvec(),
                ctrl: e.get("ctrl").pts(),
                weights: weights(e.get("weights")),
                periodic: e.get("periodic").bool(),
            })
            .unwrap(),
        )
    } else {
        Built::Surface(
            NurbsSurface::new(NurbsSurfaceParts {
                degree_u: e.get("degree_u").usize(),
                degree_v: e.get("degree_v").usize(),
                knots_u: e.get("knots_u").fvec(),
                knots_v: e.get("knots_v").fvec(),
                n_u: e.get("n_u").usize(),
                n_v: e.get("n_v").usize(),
                ctrl: e.get("ctrl").pts(),
                weights: weights(e.get("weights")),
                periodic_u: e.get("periodic_u").bool(),
                periodic_v: e.get("periodic_v").bool(),
            })
            .unwrap(),
        )
    }
}

fn entity(id: &str) -> &'static J {
    ref_set().iter().find(|e| e.get("id").str() == id).unwrap()
}

fn inf_err(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).abs().max((a[1] - b[1]).abs()).max((a[2] - b[2]).abs())
}

fn inf_norm(a: [f64; 3]) -> f64 {
    a[0].abs().max(a[1].abs()).max(a[2].abs())
}

/// Derivative tolerance (AC-1): `|got - want|_inf <= 1e-9 * max(1, |want|_inf)`.
fn assert_rel(got: [f64; 3], want: [f64; 3], ctx: &str) {
    let tol = REL_TOL * inf_norm(want).max(1.0);
    assert!(inf_err(got, want) <= tol, "{ctx}: {got:?} vs {want:?}");
}

fn assert_scalar(got: f64, want: f64, scale: f64, ctx: &str) {
    assert!((got - want).abs() <= REL_TOL * scale, "{ctx}: {got:e} vs {want:e}");
}

// ---------------------------------------------------------------- AC-1

/// AC-1: every position of `ref_set_v1` within 1e-12 m of the exact oracle, every derivative
/// within 1e-9 relative, and regularity, normals and curvatures matching (degrees 1 to 25,
/// rational and non-rational, repeated knots, domain ends).
#[test]
fn ac1_matches_exact_reference() {
    let mut rows = 0;
    for e in ref_set() {
        let id = e.get("id").str();
        match build(e) {
            Built::Curve(c) => {
                for r in e.get("eval").arr() {
                    let t = r.get("t").f();
                    let ctx = format!("{id} t={t:e}");
                    let d = c.eval(t, 2).unwrap();
                    assert_eq!(d.t, t, "{ctx}");
                    assert!(inf_err(d.p, r.get("p").v3()) <= POS_TOL, "{ctx}: position");
                    assert_rel(d.d1.unwrap(), r.get("d1").v3(), &ctx);
                    assert_rel(d.d2.unwrap(), r.get("d2").v3(), &ctx);
                    let g = c.geometry(t).unwrap();
                    let want = r.get("curvature");
                    assert_eq!(g.curvature.is_none(), want.is_null(), "{ctx}");
                    assert_eq!(d.regularity, Some(g.regularity));
                    if let Some(k) = g.curvature {
                        assert_scalar(k, want.f(), want.f().abs().max(1.0), &ctx);
                        let tan = g.tangent.unwrap();
                        assert!(((tan[0] * tan[0] + tan[1] * tan[1] + tan[2] * tan[2]) - 1.0).abs() <= 1e-15);
                    }
                    rows += 1;
                }
            }
            Built::Surface(s) => {
                for r in e.get("eval").arr() {
                    let (u, v) = (r.get("u").f(), r.get("v").f());
                    let ctx = format!("{id} u={u:e} v={v:e}");
                    let d = s.eval(u, v, 2).unwrap();
                    assert_eq!((d.u, d.v), (u, v), "{ctx}");
                    assert!(inf_err(d.p, r.get("p").v3()) <= POS_TOL, "{ctx}: position");
                    for (got, key) in [(d.su, "su"), (d.sv, "sv"), (d.suu, "suu"), (d.suv, "suv"), (d.svv, "svv")] {
                        assert_rel(got.unwrap(), r.get(key).v3(), &format!("{ctx} {key}"));
                    }
                    let degenerate = r.get("regularity").str() == "degenerate";
                    let want_reg = if degenerate { Regularity::Degenerate } else { Regularity::Regular };
                    assert_eq!(d.regularity, Some(want_reg), "{ctx}");
                    let k = s.curvature(u, v).unwrap();
                    assert_eq!(k.is_none(), degenerate, "{ctx}");
                    assert_eq!(s.normal(u, v).unwrap(), k.map(|k| k.normal), "{ctx}");
                    if let Some(k) = k {
                        assert_rel(k.normal, r.get("normal").v3(), &format!("{ctx} normal"));
                        let scale = r.get("k1").f().abs().max(r.get("k2").f().abs()).max(1.0);
                        assert_scalar(k.k1, r.get("k1").f(), scale, &format!("{ctx} k1"));
                        assert_scalar(k.k2, r.get("k2").f(), scale, &format!("{ctx} k2"));
                        assert_scalar(k.mean, r.get("mean").f(), scale, &format!("{ctx} mean"));
                        assert_scalar(k.gaussian, r.get("gaussian").f(), scale * scale, &format!("{ctx} K"));
                        assert!(k.k1 >= k.k2);
                        if let (Some(d1), Some(d2)) = (k.d1, k.d2) {
                            check_principal_frame(&d, &k, d1, d2, &ctx);
                        }
                    }
                    rows += 1;
                }
            }
        }
    }
    assert!(rows >= 1900, "{rows} rows");
}

/// Self-consistency of the principal frame: unit, orthogonal, tangent, and the normal curvature
/// II(d)/I(d) of each direction is its principal curvature (Euler's theorem).
fn check_principal_frame(
    d: &fid_geo_nurbs::SurfaceDerivs,
    k: &fid_geo_nurbs::Curvature,
    d1: [f64; 3],
    d2: [f64; 3],
    ctx: &str,
) {
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (su, sv) = (d.su.unwrap(), d.sv.unwrap());
    for x in [d1, d2] {
        assert!((dot(x, x) - 1.0).abs() <= 1e-12, "{ctx}: not unit");
        assert!(dot(x, k.normal).abs() <= 1e-12, "{ctx}: not tangent");
    }
    assert!(dot(d1, d2).abs() <= 1e-12, "{ctx}: not orthogonal");
    // tangent direction x = a Su + b Sv: solve the 2x2 Gram system for (a, b)
    let (e, f, g) = (dot(su, su), dot(su, sv), dot(sv, sv));
    let normal_curvature = |x: [f64; 3]| {
        let (p, q) = (dot(x, su), dot(x, sv));
        let det = e * g - f * f;
        let (a, b) = ((g * p - f * q) / det, (e * q - f * p) / det);
        let ii = a * a * dot(d.suu.unwrap(), k.normal)
            + 2.0 * a * b * dot(d.suv.unwrap(), k.normal)
            + b * b * dot(d.svv.unwrap(), k.normal);
        let i = a * a * e + 2.0 * a * b * f + b * b * g;
        ii / i
    };
    let scale = k.k1.abs().max(k.k2.abs()).max(1.0);
    assert!((normal_curvature(d1) - k.k1).abs() <= 1e-6 * scale, "{ctx}: d1 is not the k1 direction");
    assert!((normal_curvature(d2) - k.k2).abs() <= 1e-6 * scale, "{ctx}: d2 is not the k2 direction");
}

/// Order 0 and 1 give bit-for-bit the fields that order 2 gives, and absent fields are absent.
#[test]
fn lower_orders_are_prefixes_of_order_two() {
    for e in ref_set().iter().step_by(5) {
        match build(e) {
            Built::Curve(c) => {
                for r in e.get("eval").arr().iter().step_by(3) {
                    let t = r.get("t").f();
                    let (d0, d1, d2) = (c.eval(t, 0).unwrap(), c.eval(t, 1).unwrap(), c.eval(t, 2).unwrap());
                    assert_eq!((d0.p, d0.d1, d0.d2, d0.regularity), (d2.p, None, None, None));
                    assert_eq!((d1.p, d1.d1, d1.d2, d1.regularity), (d2.p, d2.d1, None, d2.regularity));
                }
            }
            Built::Surface(s) => {
                for r in e.get("eval").arr().iter().step_by(3) {
                    let (u, v) = (r.get("u").f(), r.get("v").f());
                    let (d0, d1, d2) = (s.eval(u, v, 0).unwrap(), s.eval(u, v, 1).unwrap(), s.eval(u, v, 2).unwrap());
                    assert_eq!((d0.p, d0.su, d0.suu, d0.regularity), (d2.p, None, None, None));
                    assert_eq!((d1.p, d1.su, d1.sv, d1.suv, d1.regularity), (d2.p, d2.su, d2.sv, None, d2.regularity));
                }
            }
        }
    }
}

// ---------------------------------------------------------------- AC-2, AC-3

/// AC-2: the NURBS sphere at its poles is degenerate, with finite derivatives, no normal and no
/// curvature (never a NaN normal).
#[test]
fn ac2_sphere_poles_are_degenerate() {
    let e = entity("sphere");
    let Built::Surface(s) = build(e) else { panic!("sphere is a surface") };
    let poles: Vec<&J> =
        e.get("eval").arr().iter().filter(|r| r.opt("tag").is_some_and(|t| t.str() == "pole")).collect();
    assert!(poles.len() >= 2);
    for r in poles {
        let (u, v) = (r.get("u").f(), r.get("v").f());
        let d = s.eval(u, v, 2).unwrap();
        assert_eq!(d.regularity, Some(Regularity::Degenerate));
        for x in [d.p, d.su.unwrap(), d.sv.unwrap(), d.suu.unwrap(), d.suv.unwrap(), d.svv.unwrap()] {
            assert!(x.iter().all(|c| c.is_finite()));
        }
        assert_eq!(s.normal(u, v).unwrap(), None);
        assert_eq!(s.curvature(u, v).unwrap(), None);
    }
}

/// AC-3: the 10 mm NURBS cylinder at its 100 curvature parameters: principal curvatures of
/// magnitude 100 per metre and 0 within 1e-9 relative. Signed against the normal `Su x Sv`,
/// which points outward here, the curved direction is -100 (the GEO.NURBS.REF convention), so
/// `k1 = 0`, `k2 = -100`. The zero-curvature direction is the axis (z).
#[test]
fn ac3_cylinder_principal_curvatures() {
    let e = entity("cylinder");
    let Built::Surface(s) = build(e) else { panic!("cylinder is a surface") };
    let rows: Vec<&J> =
        e.get("eval").arr().iter().filter(|r| r.opt("tag").is_some_and(|t| t.str() == "curvature")).collect();
    assert_eq!(rows.len(), 100);
    for r in rows {
        let (u, v) = (r.get("u").f(), r.get("v").f());
        let k = s.curvature(u, v).unwrap().unwrap();
        assert!(k.k1.abs() <= REL_TOL * 100.0, "u={u}: k1 = {}", k.k1);
        assert!((k.k2 + 100.0).abs() <= REL_TOL * 100.0, "u={u}: k2 = {}", k.k2);
        assert!(k.gaussian.abs() <= REL_TOL * 100.0 * 100.0);
        assert!((k.mean + 50.0).abs() <= REL_TOL * 100.0);
        let (d1, d2) = (k.d1.unwrap(), k.d2.unwrap());
        assert!((d1[2].abs() - 1.0).abs() <= 1e-9, "u={u}: d1 = {d1:?} is not the axis");
        assert!(d2[2].abs() <= 1e-9);
        let p = s.eval(u, v, 0).unwrap().p;
        assert!(((p[0] * p[0] + p[1] * p[1]).sqrt() - 0.01).abs() <= POS_TOL);
    }
}

// ---------------------------------------------------------------- AC-4

fn hx(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn vx(v: Option<[f64; 3]>) -> String {
    match v {
        None => "-".to_string(),
        Some(v) => format!("{} {} {}", hx(v[0]), hx(v[1]), hx(v[2])),
    }
}

fn domain_line(b: &Built, tag: &str, t: f64, other: f64) -> String {
    let r = match (b, tag) {
        (Built::Curve(c), _) => c.eval(t, 0).map(|d| (d.t, d.p)),
        (Built::Surface(s), "u") => s.eval(t, other, 0).map(|d| (d.u, d.p)),
        (Built::Surface(s), _) => s.eval(other, t, 0).map(|d| (d.v, d.p)),
    };
    match r {
        Ok((tc, p)) => format!("x {tag} {} C {} {}", hx(t), hx(tc), vx(Some(p))),
        Err(EvalError::OutsideDomain { .. }) => format!("x {tag} {} O", hx(t)),
        Err(other) => panic!("unexpected {other}"),
    }
}

/// The digest text of one entity, in the format pinned by `make_eval_fixtures.py`.
fn entity_text(e: &J) -> String {
    let b = build(e);
    let mut out = String::new();
    for r in e.get("eval").arr() {
        match &b {
            Built::Curve(c) => {
                let d = c.eval(r.get("t").f(), 2).unwrap();
                let g = fid_geo_nurbs::curve_geometry(d.d1.unwrap(), d.d2.unwrap()).unwrap();
                let reg = if g.regularity == Regularity::Regular { "R" } else { "D" };
                let k = g.curvature.map_or("-".to_string(), hx);
                let (p, d1, d2) = (vx(Some(d.p)), vx(d.d1), vx(d.d2));
                writeln!(out, "c {} {p} {d1} {d2} {reg} {} {k}", hx(d.t), vx(g.tangent)).unwrap();
            }
            Built::Surface(s) => {
                let d = s.eval(r.get("u").f(), r.get("v").f(), 2).unwrap();
                write!(out, "s {} {} {}", hx(d.u), hx(d.v), vx(Some(d.p))).unwrap();
                for x in [d.su, d.sv, d.suu, d.suv, d.svv] {
                    write!(out, " {}", vx(x)).unwrap();
                }
                match s.curvature(d.u, d.v).unwrap() {
                    None => out.push_str(" D -\n"),
                    Some(k) => writeln!(
                        out,
                        " R {} {} {} {} {} {} {}",
                        vx(Some(k.normal)),
                        hx(k.k1),
                        hx(k.k2),
                        hx(k.gaussian),
                        hx(k.mean),
                        vx(k.d1),
                        vx(k.d2)
                    )
                    .unwrap(),
                }
            }
        }
    }
    match &b {
        Built::Curve(_) => {
            for dc in e.get("domain_checks").arr() {
                writeln!(out, "{}", domain_line(&b, "c", dc.get("t").f(), 0.0)).unwrap();
            }
        }
        Built::Surface(s) => {
            let ((u0, _), (v0, _)) = Surface::domain(s);
            for (tag, other) in [("u", v0), ("v", u0)] {
                for dc in e.get("domain_checks").get(tag).arr() {
                    writeln!(out, "{}", domain_line(&b, tag, dc.get("t").f(), other)).unwrap();
                }
            }
        }
    }
    out
}

/// AC-4: the evaluation of every entity of `ref_set_v1` is bit-identical to the Python model of
/// the pinned order of operations: the per-entity and total SHA-256 equal
/// `digests-v1.txt`. CI runs this on x86-64 Linux and Windows, ARM64 macOS and wasm32-wasip1.
#[test]
fn ac4_result_digest_is_pinned() {
    let golden = std::fs::read_to_string(format!("{DIR}eval/digests-v1.txt")).unwrap();
    let want: BTreeMap<&str, &str> =
        golden.lines().filter(|l| !l.starts_with('#')).map(|l| l.split_once(' ').unwrap()).collect();
    let mut all = String::new();
    let mut bad = Vec::new();
    for e in ref_set() {
        let id = e.get("id").str();
        let text = entity_text(e);
        let got = sha256_hex(text.as_bytes());
        if got.strip_prefix("sha256:").unwrap_or(&got) != want[id] {
            bad.push(id.to_string());
            if bad.len() == 1 {
                eprintln!("first differing entity {id}:\n{}", &text[..text.len().min(4000)]);
            }
        }
        all.push_str(&text);
    }
    assert!(bad.is_empty(), "entities differ from the pinned model: {bad:?}");
    let total = sha256_hex(all.as_bytes());
    assert_eq!(total.strip_prefix("sha256:").unwrap_or(&total), want["all"]);
    assert_eq!(want.len(), ref_set().len() + 1);
}

// ---------------------------------------------------------------- AC-5 and error paths

/// AC-5 and REQ-GEO.NURBS.EVAL-002 on every entity: 1e-6 x width outside an end is
/// `OutsideDomain` (naming the direction), 0.25e-14 x width outside clamps to the end, and the
/// parameter actually used is reported.
#[test]
fn ac5_outside_domain_and_clamp() {
    let mut outside = 0;
    for e in ref_set() {
        let b = build(e);
        let checks: Vec<(&str, &J)> = match &b {
            Built::Curve(_) => e.get("domain_checks").arr().iter().map(|c| ("c", c)).collect(),
            Built::Surface(_) => ["u", "v"]
                .into_iter()
                .flat_map(|tag| e.get("domain_checks").get(tag).arr().iter().map(move |c| (tag, c)))
                .collect(),
        };
        for (tag, dc) in checks {
            let t = dc.get("t").f();
            let (res, dir) = match &b {
                Built::Curve(c) => (c.eval(t, 2).map(|d| d.t), Dir::Curve),
                Built::Surface(s) => {
                    let ((u0, _), (v0, _)) = Surface::domain(s);
                    if tag == "u" {
                        (s.eval(t, v0, 2).map(|d| d.u), Dir::U)
                    } else {
                        (s.eval(u0, t, 2).map(|d| d.v), Dir::V)
                    }
                }
            };
            if dc.get("expect").str() == "outside_domain" {
                match res {
                    Err(EvalError::OutsideDomain { dir: d, t: got, .. }) => {
                        assert_eq!((d, got), (dir, t));
                        outside += 1;
                    }
                    other => panic!("{}: {tag} = {t:e} gave {other:?}", e.get("id").str()),
                }
            } else {
                assert_eq!(res, Ok(dc.get("to").f()), "{}: {tag} = {t:e}", e.get("id").str());
            }
        }
    }
    assert!(outside >= 2 * 62);
}

fn line_curve() -> NurbsCurve {
    NurbsCurve::new(NurbsCurveParts {
        degree: 1,
        knots: vec![2.0, 2.0, 6.0, 6.0],
        ctrl: vec![[0.0, 0.0, 0.0], [4.0, 0.0, 0.0]],
        weights: None,
        periodic: false,
    })
    .unwrap()
}

fn plane_patch() -> NurbsSurface {
    NurbsSurface::new(NurbsSurfaceParts {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        n_u: 2,
        n_v: 2,
        ctrl: vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
        weights: None,
        periodic_u: false,
        periodic_v: false,
    })
    .unwrap()
}

/// AC-5 by hand: u = domain end + 1e-6 x width on a curve and in each surface direction.
#[test]
fn ac5_hand_labelled() {
    let c = line_curve();
    let t = 6.0 + 1e-6 * 4.0;
    assert_eq!(c.eval(t, 0), Err(EvalError::OutsideDomain { dir: Dir::Curve, t, lo: 2.0, hi: 6.0 }));
    assert_eq!(c.eval(2.0 - 1e-6 * 4.0, 1).unwrap_err().code(), "OutsideDomain");
    let s = plane_patch();
    let x = 1.0 + 1e-6;
    assert_eq!(s.eval(x, 0.5, 2), Err(EvalError::OutsideDomain { dir: Dir::U, t: x, lo: 0.0, hi: 1.0 }));
    assert_eq!(s.eval(0.5, x, 2), Err(EvalError::OutsideDomain { dir: Dir::V, t: x, lo: 0.0, hi: 1.0 }));
    assert_eq!(s.normal(-1e-6, 0.5).unwrap_err().code(), "OutsideDomain");
    assert_eq!(s.curvature(0.5, -1e-6).unwrap_err().code(), "OutsideDomain");
    // within the margin: clamped, and reported as clamped
    let d = c.eval(6.0 + 1e-15, 1).unwrap();
    assert_eq!((d.t, d.p, d.d1), (6.0, [4.0, 0.0, 0.0], Some([1.0, 0.0, 0.0])));
    let d = s.eval(1.0 + 5e-15, -5e-15, 0).unwrap();
    assert_eq!((d.u, d.v, d.p), (1.0, 0.0, [1.0, 0.0, 0.0]));
}

#[test]
fn nan_infinite_parameter_and_order_are_refused() {
    let c = line_curve();
    assert_eq!(c.eval(f64::NAN, 0), Err(EvalError::NotANumber { dir: Dir::Curve }));
    assert_eq!(c.eval(f64::INFINITY, 0).unwrap_err().code(), "OutsideDomain");
    assert_eq!(c.eval(f64::NEG_INFINITY, 0).unwrap_err().code(), "OutsideDomain");
    assert_eq!(c.eval(3.0, MAX_ORDER + 1), Err(EvalError::OrderTooHigh { order: 3 }));
    assert_eq!(c.eval(3.0, usize::MAX), Err(EvalError::OrderTooHigh { order: usize::MAX }));
    let s = plane_patch();
    assert_eq!(s.eval(0.5, f64::NAN, 0), Err(EvalError::NotANumber { dir: Dir::V }));
    assert_eq!(s.eval(f64::NAN, f64::NAN, 0), Err(EvalError::NotANumber { dir: Dir::U }));
    assert_eq!(s.eval(0.5, 0.5, 3), Err(EvalError::OrderTooHigh { order: 3 }));
    for err in [EvalError::NonFinite, EvalError::Inconsistent, EvalError::OrderTooHigh { order: 3 }] {
        assert!(!err.to_string().is_empty());
    }
}

/// Overflow is a typed error, never a NaN or infinite result.
#[test]
fn overflow_is_refused_not_returned() {
    let c = NurbsCurve::new(NurbsCurveParts {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        ctrl: vec![[1e308, 0.0, 0.0], [-1e308, 0.0, 0.0]],
        weights: None,
        periodic: false,
    })
    .unwrap();
    assert_eq!(c.eval(0.5, 0).unwrap().p, [0.0, 0.0, 0.0]);
    assert_eq!(c.eval(0.5, 1), Err(EvalError::NonFinite));
    let r = NurbsCurve::new(NurbsCurveParts {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        ctrl: vec![[1e300, 0.0, 0.0], [1e300, 1.0, 0.0]],
        weights: Some(vec![1e10, 1.0]),
        periodic: false,
    })
    .unwrap();
    assert_eq!(r.eval(0.5, 0), Err(EvalError::NonFinite));
}

/// Invalid knot vectors, degrees and weights never reach the evaluator: GEO.NURBS.DATA refuses
/// them with a typed error at construction.
#[test]
fn invalid_input_is_refused_before_evaluation() {
    let parts = |degree: usize, knots: Vec<f64>, weights: Option<Vec<f64>>| NurbsCurveParts {
        degree,
        knots,
        ctrl: vec![[0.0; 3]; 3],
        weights,
        periodic: false,
    };
    let cases: [(NurbsCurveParts, &str); 7] = [
        (parts(2, vec![0.0, 0.0, 0.0, 1.0, 0.5, 1.0], None), "BadKnots"), // decreasing
        (parts(1, vec![0.0, 0.0, 1.0, 1.0], None), "CountMismatch"),
        (parts(1, vec![0.0, 0.5, 0.5, 0.5, 1.0], None), "BadKnots"), // multiplicity above degree
        (parts(0, vec![0.0, 0.5, 1.0], None), "BadDegree"),
        (parts(2, vec![0.0, 0.0, 0.0, f64::NAN, 1.0, 1.0], None), "BadKnots"),
        (parts(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], Some(vec![1.0, 0.0, 1.0])), "BadWeight"),
        (parts(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], Some(vec![1.0, f64::INFINITY, 1.0])), "BadWeight"),
    ];
    for (p, code) in cases {
        let err: NurbsError = NurbsCurve::new(p).unwrap_err();
        assert_eq!(err.code(), code);
    }
}

// ---------------------------------------------------------------- hand-labelled geometry

/// Quadratic Bezier (0,0,0), (1,2,0), (2,0,0) at t = 1/2, worked by hand:
/// C = (1, 1, 0), C' = (2, 0, 0), C'' = (0, -8, 0), tangent (1, 0, 0), curvature 16 / 8 = 2.
#[test]
fn quadratic_bezier_by_hand() {
    let c = NurbsCurve::new(NurbsCurveParts {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        ctrl: vec![[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]],
        weights: None,
        periodic: false,
    })
    .unwrap();
    let d = c.eval(0.5, 2).unwrap();
    assert_eq!((d.p, d.d1, d.d2), ([1.0, 1.0, 0.0], Some([2.0, 0.0, 0.0]), Some([0.0, -8.0, 0.0])));
    let g = c.geometry(0.5).unwrap();
    assert_eq!((g.regularity, g.tangent, g.curvature), (Regularity::Regular, Some([1.0, 0.0, 0.0]), Some(2.0)));
}

/// Rational quadratic quarter circle (1,0), (1,1), (0,1), weights 1, sqrt(2)/2, 1: every point
/// at radius 1 and curvature 1.
#[test]
fn rational_quarter_circle_by_hand() {
    let c = NurbsCurve::new(NurbsCurveParts {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        ctrl: vec![[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
        weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
        periodic: false,
    })
    .unwrap();
    let mid = c.eval(0.5, 0).unwrap().p;
    assert!((mid[0] - std::f64::consts::FRAC_1_SQRT_2).abs() <= 1e-15 && mid[0] == mid[1]);
    for i in 0..=20 {
        let t = f64::from(i) / 20.0;
        let p = c.eval(t, 0).unwrap().p;
        assert!(((p[0] * p[0] + p[1] * p[1]).sqrt() - 1.0).abs() <= 1e-15, "t = {t}");
        let k = c.geometry(t).unwrap().curvature.unwrap();
        assert!((k - 1.0).abs() <= 1e-12, "t = {t}: curvature {k}");
    }
}

/// Bilinear unit patch: S(u, v) = (u, v, 0); flat and umbilic, so no principal directions.
#[test]
fn plane_patch_by_hand_through_the_traits() {
    fn probe(s: &dyn Surface) -> (fid_geo_nurbs::SurfaceDerivs, Option<fid_geo_nurbs::Curvature>) {
        (s.eval(0.25, 0.75, 2).unwrap(), s.curvature(0.25, 0.75).unwrap())
    }
    let (d, k) = probe(&plane_patch());
    assert_eq!((d.p, d.su, d.sv), ([0.25, 0.75, 0.0], Some([1.0, 0.0, 0.0]), Some([0.0, 1.0, 0.0])));
    assert_eq!((d.suu, d.suv, d.svv), (Some([0.0; 3]), Some([0.0; 3]), Some([0.0; 3])));
    let k = k.unwrap();
    assert_eq!((k.normal, k.k1, k.k2, k.gaussian, k.mean), ([0.0, 0.0, 1.0], 0.0, 0.0, 0.0, 0.0));
    assert_eq!((k.d1, k.d2), (None, None));
}

/// A curve with a zero first derivative (coincident control points) is degenerate: no tangent,
/// no curvature, no NaN.
#[test]
fn stationary_curve_point_is_degenerate() {
    let c = NurbsCurve::new(NurbsCurveParts {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        ctrl: vec![[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]],
        weights: None,
        periodic: false,
    })
    .unwrap();
    let d = c.eval(0.3, 2).unwrap();
    assert_eq!(d.regularity, Some(Regularity::Degenerate));
    let g = c.geometry(0.3).unwrap();
    assert_eq!((g.regularity, g.tangent, g.curvature), (Regularity::Degenerate, None, None));
}

/// Hand-labelled report text for the documentation of errors.
#[test]
fn error_text_and_codes() {
    let e = EvalError::OutsideDomain { dir: Dir::U, t: 2.0, lo: 0.0, hi: 1.0 };
    let mut s = String::new();
    write!(s, "{e}").unwrap();
    assert_eq!(s, "u: parameter 2e0 outside the domain [0e0, 1e0]");
    assert_eq!(EvalError::NotANumber { dir: Dir::Curve }.to_string(), "curve: parameter is NaN");
    assert_eq!(EvalError::NonFinite.code(), "NonFinite");
}
