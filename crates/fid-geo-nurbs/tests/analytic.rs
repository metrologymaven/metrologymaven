//! GEO.NURBS.ANALYTIC acceptance tests (AC-1 to AC-4), the extreme-input fuzz set and error
//! paths.
//!
//! Expected values come from outside the Rust code: `fixtures/geo/nurbs/analytic_set.json` (the
//! 60-digit GEO.NURBS.REF oracle), `fixtures/geo/nurbs/analytic/fuzz_set.json` (outcomes
//! decided in exact rational arithmetic or at 60 digits, with per-case tolerances) and
//! `fixtures/geo/nurbs/analytic/digests-v1.txt` (the bit-exact Python model of the order of
//! operations pinned in `src/analytic.rs`), all written by
//! `fixtures/geo/nurbs/analytic/make_analytic_fixtures.py`, and hand-labelled cases below.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::fmt::Write as _;
use std::sync::OnceLock;

use fid_geo_nurbs::analytic::{SIZE_MAX, SIZE_MIN};
use fid_geo_nurbs::{
    curve_geometry, AnalyticCurve, AnalyticError, AnalyticSurface, Circle, Cone, Curve, CurveFoot, Cylinder, Dir,
    Ellipse, EvalError, Line, Placement, PlacementFault, Plane, Regularity, Sphere, Surface, SurfaceFoot, Torus,
    Uniqueness,
};
use fid_testkit::sha256_hex;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/nurbs/");
/// REQ-GEO.NURBS.ANALYTIC-004: positions and feet within 1e-13 m.
const POS_TOL: f64 = 1e-13;

// ---------------------------------------------------------------- minimal JSON reader

enum J {
    Null,
    Bool(bool),
    Num,
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
            assert_ne!(self.s[self.i], b'\\', "escapes are not used by the fixtures");
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
                let _: f64 = std::str::from_utf8(&self.s[start..self.i]).unwrap().parse().unwrap();
                J::Num
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
    fn arr(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            _ => panic!("not an array"),
        }
    }
    fn obj(&self) -> &BTreeMap<String, J> {
        match self {
            J::Obj(m) => m,
            _ => panic!("not an object"),
        }
    }
    fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string"),
        }
    }
    fn bool(&self) -> bool {
        matches!(self, J::Bool(true))
    }
    fn is_null(&self) -> bool {
        matches!(self, J::Null)
    }
    /// `{hex, dec}` (analytic_set) -> f64 from the bit pattern.
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
    /// A bare `"0x..."` string (fuzz_set) -> f64.
    fn x(&self) -> f64 {
        bits(self)
    }
    fn xv(&self) -> Vec<f64> {
        self.arr().iter().map(bits).collect()
    }
    fn x3(&self) -> [f64; 3] {
        let v = self.xv();
        [v[0], v[1], v[2]]
    }
}

fn bits(j: &J) -> f64 {
    f64::from_bits(u64::from_str_radix(j.str().strip_prefix("0x").unwrap(), 16).unwrap())
}

fn load(name: &str) -> J {
    let bytes = std::fs::read(format!("{DIR}{name}")).unwrap();
    Parser { s: &bytes, i: 0 }.value()
}

fn analytic_set() -> &'static [J] {
    static DOC: OnceLock<J> = OnceLock::new();
    DOC.get_or_init(|| load("analytic_set.json")).get("entities").arr()
}

fn fuzz_set() -> &'static [J] {
    static DOC: OnceLock<J> = OnceLock::new();
    DOC.get_or_init(|| load("analytic/fuzz_set.json")).get("cases").arr()
}

// ---------------------------------------------------------------- building

enum Built {
    Curve(AnalyticCurve),
    Surface(AnalyticSurface),
}

fn construct(
    typ: &str,
    o: [f64; 3],
    z: [f64; 3],
    x: [f64; 3],
    p: &BTreeMap<String, f64>,
) -> Result<Built, AnalyticError> {
    let pl = Placement::new(o, z, x)?;
    let g = |k: &str| p[k];
    Ok(match typ {
        "line" => Built::Curve(AnalyticCurve::Line(Line::new(pl))),
        "circle" => Built::Curve(AnalyticCurve::Circle(Circle::new(pl, g("radius"))?)),
        "ellipse" => Built::Curve(AnalyticCurve::Ellipse(Ellipse::new(pl, g("semi_axis_1"), g("semi_axis_2"))?)),
        "plane" => Built::Surface(AnalyticSurface::Plane(Plane::new(pl))),
        "cylinder" => Built::Surface(AnalyticSurface::Cylinder(Cylinder::new(pl, g("radius"))?)),
        "cone" => Built::Surface(AnalyticSurface::Cone(Cone::new(pl, g("radius"), g("semi_angle"))?)),
        "sphere" => Built::Surface(AnalyticSurface::Sphere(Sphere::new(pl, g("radius"))?)),
        "torus" => Built::Surface(AnalyticSurface::Torus(Torus::new(pl, g("major_radius"), g("minor_radius"))?)),
        other => panic!("unknown type {other}"),
    })
}

fn build(e: &J) -> Built {
    let pl = e.get("placement");
    let p: BTreeMap<String, f64> = e.get("params").obj().iter().map(|(k, v)| (k.clone(), v.f())).collect();
    construct(e.get("type").str(), pl.get("origin").v3(), pl.get("axis").v3(), pl.get("ref_dir").v3(), &p).unwrap()
}

fn build_fuzz(c: &J) -> Result<Built, AnalyticError> {
    let pl = c.get("placement");
    let p: BTreeMap<String, f64> = c.get("params").obj().iter().map(|(k, v)| (k.clone(), v.x())).collect();
    construct(c.get("type").str(), pl.get("origin").x3(), pl.get("axis").x3(), pl.get("ref_dir").x3(), &p)
}

/// Inversion as `(params, foot, distance, uniqueness)`.
fn invert(b: &Built, q: [f64; 3]) -> Result<(Vec<f64>, [f64; 3], f64, Uniqueness), AnalyticError> {
    match b {
        Built::Curve(c) => c.invert(q).map(|r: CurveFoot| (vec![r.t], r.foot, r.distance, r.uniqueness)),
        Built::Surface(s) => s.invert(q).map(|r: SurfaceFoot| (vec![r.u, r.v], r.foot, r.distance, r.uniqueness)),
    }
}

fn inf_err(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, |m, x| if x > m { x } else { m })
}

fn inf_norm(a: [f64; 3]) -> f64 {
    inf_err(a, [0.0; 3])
}

fn ang_err(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % TAU;
    if d < TAU - d {
        d
    } else {
        TAU - d
    }
}

fn periodic(typ: &str) -> &'static [bool] {
    match typ {
        "line" => &[false],
        "circle" | "ellipse" => &[true],
        "plane" => &[false, false],
        "torus" => &[true, true],
        _ => &[true, false],
    }
}

// ---------------------------------------------------------------- AC-1

/// AC-1, REQ-001 and REQ-004: every evaluation (position, derivatives, normal, curvatures) and
/// every inversion (foot, distance, parameters, `NonUnique` flag) of all eight types agrees with
/// the 60-digit GEO.NURBS.REF oracle; positions and feet within 1e-13 m.
#[test]
fn ac1_matches_high_precision_reference() {
    let mut worst = 0.0_f64;
    let mut types = std::collections::BTreeSet::new();
    for e in analytic_set() {
        let id = e.get("id").str();
        let typ = e.get("type").str();
        types.insert(typ.to_string());
        let b = build(e);
        for r in e.get("eval").arr() {
            match &b {
                Built::Curve(c) => {
                    let d = c.eval(r.get("t").f(), 2).unwrap();
                    let err = inf_err(d.p, r.get("p").v3());
                    assert!(err <= POS_TOL, "{id}: position off by {err:e}");
                    worst = worst.max(err);
                    for (got, key) in [(d.d1.unwrap(), "d1"), (d.d2.unwrap(), "d2")] {
                        let want = r.get(key).v3();
                        assert!(inf_err(got, want) <= 1e-12 * inf_norm(want).max(1.0), "{id}: {key}");
                    }
                    let g = c.geometry(d.t).unwrap();
                    if r.get("curvature").is_null() {
                        assert_eq!(g.regularity, Regularity::Degenerate, "{id}");
                    } else {
                        let k = r.get("curvature").f();
                        assert!((g.curvature.unwrap() - k).abs() <= 1e-9 * k.abs().max(1.0), "{id}: curvature");
                    }
                }
                Built::Surface(s) => {
                    let d = s.eval(r.get("u").f(), r.get("v").f(), 2).unwrap();
                    let err = inf_err(d.p, r.get("p").v3());
                    assert!(err <= POS_TOL, "{id}: position off by {err:e}");
                    worst = worst.max(err);
                    for (got, key) in [(d.su, "su"), (d.sv, "sv"), (d.suu, "suu"), (d.suv, "suv"), (d.svv, "svv")] {
                        let want = r.get(key).v3();
                        assert!(inf_err(got.unwrap(), want) <= 1e-12 * inf_norm(want).max(1.0), "{id}: {key}");
                    }
                    let k = s.curvature(d.u, d.v).unwrap();
                    if r.get("regularity").str() == "degenerate" {
                        assert!(k.is_none(), "{id}");
                        continue;
                    }
                    let k = k.unwrap();
                    assert!(inf_err(k.normal, r.get("normal").v3()) <= 1e-12, "{id}: normal");
                    assert_eq!(s.normal(d.u, d.v).unwrap(), Some(k.normal));
                    let ks = r.get("k1").f().abs().max(r.get("k2").f().abs()).max(1.0);
                    for (got, key) in [(k.k1, "k1"), (k.k2, "k2"), (k.mean, "mean")] {
                        assert!((got - r.get(key).f()).abs() <= 1e-9 * ks, "{id}: {key}");
                    }
                    assert!((k.gaussian - r.get("gaussian").f()).abs() <= 1e-9 * ks * ks, "{id}: gaussian");
                }
            }
        }
        for r in e.get("invert").arr() {
            let qid = r.get("id").str();
            let (prm, foot, dist, u) = invert(&b, r.get("q").v3()).unwrap();
            let err = inf_err(foot, r.get("foot").v3());
            assert!(err <= POS_TOL, "{qid}: foot off by {err:e}");
            worst = worst.max(err);
            assert!((dist - r.get("distance").f()).abs() <= POS_TOL, "{qid}: distance");
            let nu = r.get("non_unique").bool();
            assert_eq!(u == Uniqueness::NonUnique, nu, "{qid}: uniqueness");
            let want = r.get("params").fvec();
            for ((g, w), per) in prm.iter().zip(&want).zip(periodic(typ)) {
                let e = if *per { ang_err(*g, *w) } else { (g - w).abs() };
                assert!(e <= 1e-12 * w.abs().max(1.0), "{qid}: params {prm:?} vs {want:?}");
                if *per {
                    assert!((0.0..TAU).contains(g) && g.to_bits() != (-0.0_f64).to_bits(), "{qid}: angle range");
                }
            }
        }
    }
    assert_eq!(types.len(), 8);
    eprintln!("AC-1 worst position / foot error {worst:e} m");
}

// ---------------------------------------------------------------- AC-2

fn aligned(o: [f64; 3]) -> Placement {
    Placement::new(o, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]).unwrap()
}

/// AC-2 (hand-labelled): a point on the cylinder axis inverts to `u = 0` at radius distance,
/// flagged `NonUnique`; so do the other axis, centre and apex cases of REQ-003.
#[test]
fn ac2_axis_cases_are_canonical_and_flagged() {
    let o = [1.0, -2.0, 0.5];
    let cyl = Cylinder::new(aligned(o), 0.25).unwrap();
    let r = cyl.invert([1.0, -2.0, 1.75]).unwrap();
    assert_eq!((r.u, r.v, r.distance, r.uniqueness), (0.0, 1.25, 0.25, Uniqueness::NonUnique));
    assert_eq!(r.foot, [1.25, -2.0, 1.75]);
    // Off the axis the answer is unique and on the query's azimuth.
    let r = cyl.invert([1.0, -1.0, 0.5]).unwrap();
    assert_eq!((r.u, r.v, r.distance, r.uniqueness), (FRAC_PI_2, 0.0, 0.75, Uniqueness::Unique));

    let circ = Circle::new(aligned(o), 2.0).unwrap();
    let r = circ.invert([1.0, -2.0, -1.0]).unwrap();
    assert_eq!((r.t, r.uniqueness), (0.0, Uniqueness::NonUnique));
    assert!((r.distance - 2.5).abs() <= 1e-15);

    let sph = Sphere::new(aligned(o), 3.0).unwrap();
    let r = sph.invert(o).unwrap();
    assert_eq!((r.u, r.v, r.distance, r.uniqueness), (0.0, 0.0, 3.0, Uniqueness::NonUnique));
    let r = sph.invert([1.0, -2.0, 10.5]).unwrap();
    assert_eq!((r.u, r.v, r.distance, r.uniqueness), (0.0, FRAC_PI_2, 7.0, Uniqueness::Unique));

    let tor = Torus::new(aligned(o), 2.0, 0.5).unwrap();
    let r = tor.invert([1.0, -2.0, 0.5]).unwrap();
    assert_eq!((r.u, r.uniqueness), (0.0, Uniqueness::NonUnique));
    assert_eq!(r.v, PI, "on the axis in the tube plane the foot is the inner equator");
    assert!((r.distance - 1.5).abs() <= 1e-15);
    let r = tor.invert([3.0, -2.0, 0.5]).unwrap(); // the tube-circle centre
    assert_eq!((r.u, r.v, r.distance, r.uniqueness), (0.0, 0.0, 0.5, Uniqueness::NonUnique));

    // Cone with its apex at the origin (r0 = 0): the apex itself is the unique foot; a point
    // on the axis above it is NonUnique; a point in the apex plane ties between the nappes.
    let cone = Cone::new(aligned([0.0; 3]), 0.0, PI / 4.0).unwrap();
    let r = cone.invert([0.0; 3]).unwrap();
    assert_eq!((r.u, r.v, r.distance, r.uniqueness), (0.0, 0.0, 0.0, Uniqueness::Unique));
    let r = cone.invert([0.0, 0.0, 2.0]).unwrap();
    assert_eq!((r.u, r.uniqueness), (0.0, Uniqueness::NonUnique));
    assert!((r.v - 1.0).abs() <= 1e-15 && (r.distance - 2f64.sqrt()).abs() <= 1e-15);
    let r = cone.invert([0.0, 3.0, 0.0]).unwrap();
    assert_eq!((r.u, r.uniqueness), (FRAC_PI_2, Uniqueness::NonUnique));
    // Below the apex the nearer nappe is the mirrored one (u = azimuth + pi).
    let r = cone.invert([1.0, 0.0, -3.0]).unwrap();
    assert_eq!((r.u, r.uniqueness), (PI, Uniqueness::Unique));
    assert!((r.distance - 2f64.sqrt()).abs() <= 1e-15, "{r:?}");

    // Ellipse centre: two feet (0, +-b); the smaller t, pi/2. Tall ellipse: (+-a, 0), t = 0.
    let ell = Ellipse::new(aligned(o), 2.0, 1.0).unwrap();
    let r = ell.invert(o).unwrap();
    assert_eq!((r.t, r.distance, r.uniqueness), (FRAC_PI_2, 1.0, Uniqueness::NonUnique));
    let tall = Ellipse::new(aligned(o), 1.0, 2.0).unwrap();
    let r = tall.invert(o).unwrap();
    assert_eq!((r.t, r.distance, r.uniqueness), (0.0, 1.0, Uniqueness::NonUnique));
    // On the major axis outside the evolute: the vertex, unique.
    let r = ell.invert([4.0, -2.0, 0.5]).unwrap();
    assert_eq!((r.t, r.distance, r.uniqueness), (0.0, 1.0, Uniqueness::Unique));
}

// ---------------------------------------------------------------- AC-3

/// AC-3 and REQ-002 (hand-labelled from the spec's rules): invalid constructions are refused
/// with typed errors; boundaries are inclusive exactly as documented.
#[test]
fn ac3_invalid_constructions_are_refused() {
    let pl = aligned([0.0; 3]);
    for a in [0.0, -0.1, FRAC_PI_2, 1.6, PI] {
        assert_eq!(Cone::new(pl, 1.0, a), Err(AnalyticError::BadAngle { value: a }), "half-angle {a}");
    }
    assert!(Cone::new(pl, 1.0, f64::from_bits(FRAC_PI_2.to_bits() - 1)).is_ok());
    assert!(Cone::new(pl, 0.0, 0.5).is_ok());
    assert_eq!(Cone::new(pl, f64::NAN, 0.5), Err(AnalyticError::NotANumber));
    assert_eq!(Cone::new(pl, 1.0, f64::NAN), Err(AnalyticError::NotANumber));
    assert_eq!(Cone::new(pl, -1.0, 0.5).unwrap_err().code(), "BadRadius");
    for (big, small) in [(1.0, 1.0), (1.0, 2.0)] {
        assert_eq!(
            Torus::new(pl, big, small),
            Err(AnalyticError::BadRadius { name: "minor_radius", value: small }),
            "self-intersecting torus {big} {small}"
        );
    }
    assert!(Torus::new(pl, 1.0, f64::from_bits(1.0_f64.to_bits() - 1)).is_ok());
    for r in [0.0, -1.0, 1e-101, 2e100, f64::MIN_POSITIVE, 5e-324] {
        assert_eq!(Sphere::new(pl, r).unwrap_err().code(), "BadRadius", "{r}");
        assert_eq!(Cylinder::new(pl, r).unwrap_err().code(), "BadRadius", "{r}");
        assert_eq!(Circle::new(pl, r).unwrap_err().code(), "BadRadius", "{r}");
        assert_eq!(Ellipse::new(pl, 1.0, r).unwrap_err().code(), "BadRadius", "{r}");
    }
    assert!(Sphere::new(pl, SIZE_MIN).is_ok() && Sphere::new(pl, SIZE_MAX).is_ok());
    assert_eq!(Sphere::new(pl, f64::INFINITY), Err(AnalyticError::NonFinite));

    let bad = |o, z, x| Placement::new(o, z, x).unwrap_err();
    let z = [0.0, 0.0, 1.0];
    let x = [1.0, 0.0, 0.0];
    assert_eq!(
        bad([0.0; 3], [0.0, 0.0, 1.0 + 2e-12], x),
        AnalyticError::BadPlacement { fault: PlacementFault::AxisNotUnit }
    );
    assert_eq!(
        bad([0.0; 3], z, [1.0 - 2e-12, 0.0, 0.0]),
        AnalyticError::BadPlacement { fault: PlacementFault::RefDirNotUnit }
    );
    assert_eq!(
        bad([0.0; 3], z, [1.0, 0.0, 2e-12]),
        AnalyticError::BadPlacement { fault: PlacementFault::NotOrthogonal }
    );
    assert_eq!(bad([0.0; 3], [0.0; 3], x).code(), "BadPlacement");
    assert_eq!(bad([0.0; 3], [0.0, 0.0, f64::MAX], x).code(), "BadPlacement");
    assert_eq!(bad([f64::NAN, 0.0, 0.0], z, x), AnalyticError::NotANumber);
    assert_eq!(bad([0.0, f64::NEG_INFINITY, 0.0], z, x), AnalyticError::NonFinite);
    assert!(Placement::new([0.0; 3], [0.0, 0.0, 1.0 + 0.5e-12], [1.0, 0.0, 0.5e-12]).is_ok());
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

/// The eval line of GEO.NURBS.EVAL's digest format.
fn eval_line(b: &Built, a: &[f64]) -> Result<String, EvalError> {
    let mut out = String::new();
    match b {
        Built::Curve(c) => {
            let d = c.eval(a[0], 2)?;
            let g = curve_geometry(d.d1.unwrap(), d.d2.unwrap())?;
            let reg = if g.regularity == Regularity::Regular { "R" } else { "D" };
            let k = g.curvature.map_or("-".to_string(), hx);
            let (p, d1, d2) = (vx(Some(d.p)), vx(d.d1), vx(d.d2));
            write!(out, "c {} {p} {d1} {d2} {reg} {} {k}", hx(d.t), vx(g.tangent)).unwrap();
        }
        Built::Surface(s) => {
            let d = s.eval(a[0], a[1], 2)?;
            let k = s.curvature(d.u, d.v)?;
            write!(out, "s {} {} {}", hx(d.u), hx(d.v), vx(Some(d.p))).unwrap();
            for x in [d.su, d.sv, d.suu, d.suv, d.svv] {
                write!(out, " {}", vx(x)).unwrap();
            }
            match k {
                None => out.push_str(" D -"),
                Some(k) => write!(
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
    Ok(out)
}

fn invert_line(b: &Built, q: [f64; 3]) -> Result<String, AnalyticError> {
    let (prm, foot, d, u) = invert(b, q)?;
    let p: Vec<String> = prm.iter().map(|x| hx(*x)).collect();
    let u = if u == Uniqueness::NonUnique { "N" } else { "U" };
    Ok(format!("i {} {} {} {u}", p.join(" "), vx(Some(foot)), hx(d)))
}

fn entity_text(e: &J) -> String {
    let b = build(e);
    let mut out = String::new();
    for r in e.get("eval").arr() {
        let a = match &b {
            Built::Curve(_) => vec![r.get("t").f()],
            Built::Surface(_) => vec![r.get("u").f(), r.get("v").f()],
        };
        writeln!(out, "{}", eval_line(&b, &a).unwrap()).unwrap();
    }
    for r in e.get("invert").arr() {
        writeln!(out, "{}", invert_line(&b, r.get("q").v3()).unwrap()).unwrap();
    }
    out
}

fn fuzz_line(c: &J) -> String {
    let kind = c.get("kind").str();
    let b = match build_fuzz(c) {
        Ok(b) => b,
        Err(e) if kind == "construct" => return format!("k {}", e.code()),
        Err(e) => return format!("x {}", e.code()),
    };
    match kind {
        "construct" => "k ok".to_string(),
        "eval" => match eval_line(&b, &c.get("args").xv()) {
            Ok(l) => format!("e {l}"),
            Err(e) => format!("e {}", e.code()),
        },
        _ => invert_line(&b, c.get("q").x3()).unwrap_or_else(|e| format!("i {}", e.code())),
    }
}

/// AC-4 and REQ-005: the evaluation and inversion of every entity of `analytic_set.json`, and
/// the outcome of every fuzz case, are bit-identical to the Python model of the pinned order of
/// operations (per-entity, total and fuzz SHA-256 equal `digests-v1.txt`). CI runs this on
/// x86-64 Linux and Windows, ARM64 macOS and wasm32-wasip1.
#[test]
fn ac4_result_digest_is_pinned() {
    let golden = std::fs::read_to_string(format!("{DIR}analytic/digests-v1.txt")).unwrap();
    let want: BTreeMap<&str, &str> =
        golden.lines().filter(|l| !l.starts_with('#')).map(|l| l.split_once(' ').unwrap()).collect();
    let mut all = String::new();
    let mut bad = Vec::new();
    for e in analytic_set() {
        let id = e.get("id").str();
        let text = entity_text(e);
        let got = sha256_hex(text.as_bytes());
        if got.strip_prefix("sha256:").unwrap_or(&got) != want[id] {
            if bad.is_empty() {
                eprintln!("first differing entity {id}:\n{}", &text[..text.len().min(4000)]);
            }
            bad.push(id.to_string());
        }
        all.push_str(&text);
    }
    assert!(bad.is_empty(), "entities differ from the pinned model: {bad:?}");
    let total = sha256_hex(all.as_bytes());
    assert_eq!(total.strip_prefix("sha256:").unwrap_or(&total), want["all"]);
    let mut fz = String::new();
    for c in fuzz_set() {
        writeln!(fz, "{}", fuzz_line(c)).unwrap();
    }
    let got = sha256_hex(fz.as_bytes());
    assert_eq!(got.strip_prefix("sha256:").unwrap_or(&got), want["fuzz"], "fuzz outcomes differ:\n{fz}");
    assert_eq!(want.len(), analytic_set().len() + 2);
}

// ---------------------------------------------------------------- fuzz set

/// Extreme and singular inputs against the exact / 60-digit reference: every outcome is the
/// decided one (a typed error, or an answer within the case's tolerance); nothing is silently
/// wrong, nothing panics.
#[test]
fn fuzz_extreme_inputs_against_exact_reference() {
    let mut counts = BTreeMap::new();
    for c in fuzz_set() {
        let id = c.get("id").str();
        let kind = c.get("kind").str();
        let expect = c.get("expect").str();
        *counts.entry(format!("{kind} {expect}")).or_insert(0) += 1;
        let built = build_fuzz(c);
        if kind == "construct" {
            match (&built, expect) {
                (Ok(_), "ok") => {}
                (Err(e), _) => {
                    assert_eq!(format!("err:{}", e.code()), expect, "{id}");
                    if let AnalyticError::BadPlacement { fault } = e {
                        assert_eq!(fault.as_str(), c.get("fault").str(), "{id}");
                    }
                }
                (Ok(_), _) => panic!("{id}: constructed, expected {expect}"),
            }
            continue;
        }
        let b = built.unwrap_or_else(|e| panic!("{id}: {e}"));
        if kind == "eval" {
            let a = c.get("args").xv();
            let r = match &b {
                Built::Curve(cv) => cv.eval(a[0], 0).map(|d| d.p),
                Built::Surface(s) => s.eval(a[0], a[1], 0).map(|d| d.p),
            };
            match (r, expect) {
                (Ok(p), "ok") => {
                    let err = inf_err(p, c.get("p").x3());
                    assert!(err <= c.get("tol").x(), "{id}: position off by {err:e}");
                }
                (Ok(p), _) => panic!("{id}: evaluated to {p:?}, expected {expect}"),
                (Err(e), _) => assert_eq!(format!("err:{}", e.code()), expect, "{id}"),
            }
            continue;
        }
        let q = c.get("q").x3();
        let r = invert(&b, q);
        let (prm, foot, d, u) = match (r, expect) {
            (Ok(r), "ok") => r,
            (Ok(r), _) => panic!("{id}: inverted to {r:?}, expected {expect}"),
            (Err(AnalyticError::NonFinite), "ok") if c.get("accept_nonfinite").bool() => continue,
            (Err(e), _) => {
                assert_eq!(format!("err:{}", e.code()), expect, "{id}");
                continue;
            }
        };
        let typ = c.get("type").str();
        let want_u = if c.get("uniqueness").str() == "N" { Uniqueness::NonUnique } else { Uniqueness::Unique };
        assert_eq!(u, want_u, "{id} {typ}: uniqueness");
        let dw = c.get("distance").x();
        assert!((d - dw).abs() <= c.get("tol_d").x(), "{id} {typ}: distance {d:e} vs {dw:e}");
        if !c.get("foot").is_null() {
            let err = inf_err(foot, c.get("foot").x3());
            assert!(err <= c.get("tol_f").x(), "{id} {typ}: foot off by {err:e}");
        }
        if !c.get("canonical").is_null() {
            for (g, w) in prm.iter().zip(c.get("canonical").xv()) {
                assert!((g - w).abs() <= 1e-15 * w.abs().max(1.0), "{id} {typ}: params {prm:?}, canonical {w}");
            }
        }
        assert!(d.is_finite() && d >= 0.0 && foot.iter().all(|x| x.is_finite()), "{id}");
    }
    eprintln!("fuzz outcomes: {counts:?}");
    assert!(fuzz_set().len() >= 600);
}

// ---------------------------------------------------------------- traits, domains, errors

/// REQ-001: all eight types behind the shared traits, generically.
#[test]
fn shared_traits_and_domains() {
    fn probe_c(c: &dyn Curve) -> (f64, f64) {
        c.domain()
    }
    fn probe_s(s: &dyn Surface) -> ((f64, f64), (f64, f64)) {
        s.domain()
    }
    let pl = aligned([0.0; 3]);
    let inf = (f64::NEG_INFINITY, f64::INFINITY);
    let per = (0.0, TAU);
    assert_eq!(probe_c(&Line::new(pl)), inf);
    assert_eq!(probe_c(&Circle::new(pl, 1.0).unwrap()), per);
    assert_eq!(probe_c(&Ellipse::new(pl, 2.0, 1.0).unwrap()), per);
    assert_eq!(probe_s(&Plane::new(pl)), (inf, inf));
    assert_eq!(probe_s(&Cylinder::new(pl, 1.0).unwrap()), (per, inf));
    assert_eq!(probe_s(&Cone::new(pl, 1.0, 0.5).unwrap()), (per, inf));
    assert_eq!(probe_s(&Sphere::new(pl, 1.0).unwrap()), (per, (-FRAC_PI_2, FRAC_PI_2)));
    assert_eq!(probe_s(&Torus::new(pl, 2.0, 1.0).unwrap()), (per, per));
    let s = AnalyticSurface::Sphere(Sphere::new(pl, 2.0).unwrap());
    assert_eq!(s.kind(), "sphere");
    let k = s.curvature(0.3, 0.2).unwrap().unwrap();
    assert!((k.k1 + 0.5).abs() <= 1e-15 && (k.k2 + 0.5).abs() <= 1e-15 && k.d1.is_none());
    let c = AnalyticSurface::Cylinder(Cylinder::new(pl, 0.5).unwrap());
    let k = c.curvature(1.0, 3.0).unwrap().unwrap();
    assert!(k.k1.abs() <= 1e-15 && (k.k2 + 2.0).abs() <= 1e-14, "{k:?}");
    let ci = AnalyticCurve::Circle(Circle::new(pl, 0.25).unwrap());
    assert_eq!(ci.kind(), "circle");
    assert!((ci.geometry(2.0).unwrap().curvature.unwrap() - 4.0).abs() <= 1e-14);
    // A periodic parameter is evaluated as given (no reduction): t and t + 2 pi agree to rounding.
    let a = ci.eval(1.0, 0).unwrap().p;
    let b = ci.eval(1.0 + TAU, 0).unwrap().p;
    assert!(inf_err(a, b) <= 1e-15);
    assert_eq!(ci.eval(1.0 + TAU, 0).unwrap().t, 1.0 + TAU);
    // The cone apex is a degenerate point (no normal, never NaN).
    let cone = Cone::new(pl, 1.0, PI / 4.0).unwrap();
    let apex_v = -1.0 / fid_num::f::tan(PI / 4.0).unwrap();
    assert_eq!(cone.eval(0.7, apex_v, 1).unwrap().regularity, Some(Regularity::Degenerate));
    assert_eq!(cone.normal(0.7, apex_v).unwrap(), None);
}

#[test]
fn parameter_and_order_errors() {
    let pl = aligned([0.0; 3]);
    let s = Sphere::new(pl, 1.0).unwrap();
    assert_eq!(s.eval(f64::NAN, 0.0, 0), Err(EvalError::NotANumber { dir: Dir::U }));
    assert_eq!(s.eval(0.0, f64::NAN, 0), Err(EvalError::NotANumber { dir: Dir::V }));
    assert_eq!(s.eval(0.0, 1.6, 0).unwrap_err().code(), "OutsideDomain");
    assert_eq!(s.eval(f64::INFINITY, 0.0, 0).unwrap_err().code(), "OutsideDomain");
    assert!(s.eval(0.0, -FRAC_PI_2, 2).is_ok());
    assert_eq!(s.eval(0.0, 0.0, 3), Err(EvalError::OrderTooHigh { order: 3 }));
    let l = Line::new(pl);
    assert_eq!(l.eval(f64::NAN, 0), Err(EvalError::NotANumber { dir: Dir::Curve }));
    assert!(l.eval(-1e300, 2).is_ok());
    assert_eq!(l.eval(1.0, 3), Err(EvalError::OrderTooHigh { order: 3 }));
    let d = l.eval(1.0, 1).unwrap();
    assert!(d.d1.is_some() && d.d2.is_none() && d.regularity == Some(Regularity::Regular));
    let d = Plane::new(pl).eval(1.0, 2.0, 0).unwrap();
    assert!(d.su.is_none() && d.regularity.is_none());
    // Overflow is refused, never returned.
    let far = Cylinder::new(aligned([0.0, 0.0, 1.7e308]), 1.0).unwrap();
    assert_eq!(far.eval(0.0, 1e308, 0), Err(EvalError::NonFinite));
    assert_eq!(far.invert([0.0, 0.0, -1.7e308]), Err(AnalyticError::NonFinite));
    assert_eq!(far.invert([0.0, f64::NAN, 0.0]), Err(AnalyticError::NotANumber));
}

/// The ellipse bisection terminates within `MAX_BISECT` for queries across the whole binary64
/// range and every answer satisfies the foot condition (foot on the ellipse, distance equal
/// to |foot - q|); the near-axis case does not lose precision (`w = s + 1` form).
#[test]
fn ellipse_inversion_is_bounded_and_consistent() {
    let ell = Ellipse::new(aligned([0.0; 3]), 3.0, 1.0).unwrap();
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..2000 {
        let e = (next() % 600) as i32 - 300;
        let q = [
            (next() % 2001) as f64 / 1000.0 - 1.0,
            (next() % 2001) as f64 / 1000.0 - 1.0,
            (next() % 2001) as f64 / 1000.0 - 1.0,
        ];
        let s = 10f64.powi(e / 3);
        let q = [q[0] * s, q[1] * s, q[2] * s];
        match ell.invert(q) {
            Ok(r) => {
                let (lx, ly) = (r.foot[0] / 3.0, r.foot[1]);
                assert!(((lx * lx + ly * ly) - 1.0).abs() <= 1e-14, "{q:?} -> {r:?}");
                let e = [r.foot[0] - q[0], r.foot[1] - q[1], r.foot[2] - q[2]];
                let d = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
                assert!((d - r.distance).abs() <= 1e-14 * d.max(1.0), "{q:?}");
            }
            Err(e) => assert_eq!(e, AnalyticError::NonFinite, "{q:?}"),
        }
    }
    // Near the major axis inside the evolute (minor coordinate 1e-12, well above AXIS_REL):
    // the foot is on the query's side and accurate, not the cancellation-prone s + 1 form.
    let r = ell.invert([1.0, 1e-12, 0.0]).unwrap();
    let r2 = ell.invert([1.0, -1e-12, 0.0]).unwrap();
    assert_eq!(r.uniqueness, Uniqueness::Unique);
    assert!(r.foot[1] > 0.9 && r2.foot[1] < -0.9, "{r:?} {r2:?}");
    // Exact feet for x = 1 on the major axis: x_f = a^2 x / (a^2 - b^2) = 9/8, y_f^2 = 1 - (x_f/3)^2.
    let yf = (1.0 - (1.125_f64 / 3.0).powi(2)).sqrt();
    assert!((r.foot[0] - 1.125).abs() <= 1e-12 && (r.foot[1] - yf).abs() <= 1e-12, "{r:?}");
}

#[test]
fn error_text_and_codes() {
    let cases: [(AnalyticError, &str); 5] = [
        (AnalyticError::NotANumber, "NotANumber"),
        (AnalyticError::NonFinite, "NonFinite"),
        (AnalyticError::BadPlacement { fault: PlacementFault::NotOrthogonal }, "BadPlacement"),
        (AnalyticError::BadRadius { name: "radius", value: -1.0 }, "BadRadius"),
        (AnalyticError::BadAngle { value: 2.0 }, "BadAngle"),
    ];
    for (e, code) in cases {
        assert_eq!(e.code(), code);
        assert!(!e.to_string().is_empty());
    }
    assert!(AnalyticError::BadPlacement { fault: PlacementFault::AxisNotUnit }.to_string().contains("AxisNotUnit"));
    let pl = aligned([0.5, 0.25, -1.0]);
    assert_eq!(pl.origin(), [0.5, 0.25, -1.0]);
    assert_eq!((pl.axis(), pl.ref_dir(), pl.y_dir()), ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
    assert_eq!(pl.to_local([1.5, 0.25, 0.0]).unwrap(), [1.0, 0.0, 1.0]);
}
