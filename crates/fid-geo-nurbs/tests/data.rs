//! Acceptance criteria of spec node GEO.NURBS.DATA (AC-1, AC-2, AC-3).
//!
//! Fixtures come from `fixtures/geo/nurbs/make_data_fixtures.py`, an independent Python
//! validator; every float is stored as its binary64 bit pattern in hex. The kernel crate
//! has no dependencies, so this test carries a minimal JSON reader for those files.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_geo_nurbs::{Dir, KnotVector, NurbsCurve, NurbsCurveParts, NurbsError, NurbsSurface, NurbsSurfaceParts};

const VALID: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/nurbs/valid_set.json");
const INVALID: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/nurbs/invalid_set.json");

// ---------------------------------------------------------------- minimal JSON reader

#[derive(Debug, Clone)]
enum J {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<J>),
    Obj(BTreeMap<String, J>),
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) {
        self.ws();
        assert_eq!(self.s[self.i], c, "expected {} at {}", c as char, self.i);
        self.i += 1;
    }
    fn lit(&mut self, w: &str) {
        assert!(self.s[self.i..].starts_with(w.as_bytes()), "expected {w} at {}", self.i);
        self.i += w.len();
    }
    fn string(&mut self) -> String {
        self.eat(b'"');
        let start = self.i;
        while self.s[self.i] != b'"' {
            assert_ne!(self.s[self.i], b'\\', "escapes not used in these fixtures");
            self.i += 1;
        }
        let out = std::str::from_utf8(&self.s[start..self.i]).unwrap().to_string();
        self.i += 1;
        out
    }
    fn value(&mut self) -> J {
        self.ws();
        match self.s[self.i] {
            b'n' => {
                self.lit("null");
                J::Null
            }
            b't' => {
                self.lit("true");
                J::Bool(true)
            }
            b'f' => {
                self.lit("false");
                J::Bool(false)
            }
            b'"' => J::Str(self.string()),
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.s[self.i] == b']' {
                    self.i += 1;
                    return J::Arr(v);
                }
                loop {
                    v.push(self.value());
                    self.ws();
                    let c = self.s[self.i];
                    self.i += 1;
                    match c {
                        b',' => continue,
                        b']' => return J::Arr(v),
                        _ => panic!("bad array at {}", self.i),
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut m = BTreeMap::new();
                self.ws();
                if self.s[self.i] == b'}' {
                    self.i += 1;
                    return J::Obj(m);
                }
                loop {
                    let k = self.string();
                    self.eat(b':');
                    let v = self.value();
                    m.insert(k, v);
                    self.ws();
                    let c = self.s[self.i];
                    self.i += 1;
                    match c {
                        b',' => continue,
                        b'}' => return J::Obj(m),
                        _ => panic!("bad object at {}", self.i),
                    }
                }
            }
            _ => {
                let start = self.i;
                while self.i < self.s.len() && (self.s[self.i] == b'-' || self.s[self.i].is_ascii_digit()) {
                    self.i += 1;
                }
                J::Int(std::str::from_utf8(&self.s[start..self.i]).unwrap().parse().unwrap())
            }
        }
    }
}

fn load(path: &str) -> Vec<J> {
    let text = std::fs::read_to_string(path).expect("read fixture");
    let mut p = P { s: text.as_bytes(), i: 0 };
    let root = p.value();
    p.ws();
    assert_eq!(p.i, p.s.len(), "trailing data");
    match root.get("cases") {
        J::Arr(v) => v.clone(),
        other => panic!("cases: {other:?}"),
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
            other => panic!("not an array: {other:?}"),
        }
    }
    fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            other => panic!("not a string: {other:?}"),
        }
    }
    fn usize(&self) -> usize {
        match self {
            J::Int(i) => usize::try_from(*i).unwrap(),
            other => panic!("not an integer: {other:?}"),
        }
    }
    fn bool(&self) -> bool {
        match self {
            J::Bool(b) => *b,
            other => panic!("not a bool: {other:?}"),
        }
    }
    fn bits(&self) -> u64 {
        u64::from_str_radix(self.str().strip_prefix("0x").unwrap(), 16).unwrap()
    }
    fn f(&self) -> f64 {
        f64::from_bits(self.bits())
    }
    fn fvec(&self) -> Vec<f64> {
        self.arr().iter().map(J::f).collect()
    }
    fn bitvec(&self) -> Vec<u64> {
        self.arr().iter().map(J::bits).collect()
    }
    fn pts(&self) -> Vec<[f64; 3]> {
        self.arr()
            .iter()
            .map(|p| {
                let c = p.arr();
                assert_eq!(c.len(), 3);
                [c[0].f(), c[1].f(), c[2].f()]
            })
            .collect()
    }
    fn ptbits(&self) -> Vec<[u64; 3]> {
        self.arr()
            .iter()
            .map(|p| {
                let c = p.arr();
                [c[0].bits(), c[1].bits(), c[2].bits()]
            })
            .collect()
    }
    fn weights(&self) -> Option<Vec<f64>> {
        match self {
            J::Null => None,
            j => Some(j.fvec()),
        }
    }
}

// ---------------------------------------------------------------- helpers

enum Built {
    Curve(NurbsCurve),
    Surface(NurbsSurface),
}

fn build(c: &J) -> Result<Built, NurbsError> {
    match c.get("kind").str() {
        "curve" => NurbsCurve::new(NurbsCurveParts {
            degree: c.get("degree").usize(),
            knots: c.get("knots").fvec(),
            ctrl: c.get("ctrl").pts(),
            weights: c.get("weights").weights(),
            periodic: c.get("periodic").bool(),
        })
        .map(Built::Curve),
        "surface" => NurbsSurface::new(NurbsSurfaceParts {
            degree_u: c.get("degree_u").usize(),
            degree_v: c.get("degree_v").usize(),
            knots_u: c.get("knots_u").fvec(),
            knots_v: c.get("knots_v").fvec(),
            n_u: c.get("n_u").usize(),
            n_v: c.get("n_v").usize(),
            ctrl: c.get("ctrl").pts(),
            weights: c.get("weights").weights(),
            periodic_u: c.get("periodic_u").bool(),
            periodic_v: c.get("periodic_v").bool(),
        })
        .map(Built::Surface),
        k => panic!("unknown kind {k}"),
    }
}

fn dir(s: &str) -> Dir {
    match s {
        "curve" => Dir::Curve,
        "u" => Dir::U,
        "v" => Dir::V,
        _ => panic!("unknown dir {s}"),
    }
}

fn expected_error(e: &J) -> NurbsError {
    let n = |k: &str| e.get(k).usize();
    match e.get("variant").str() {
        "BadDegree" => NurbsError::BadDegree { dir: dir(e.get("dir").str()), degree: n("degree") },
        "CountMismatch" => NurbsError::CountMismatch {
            dir: dir(e.get("dir").str()),
            knots: n("knots"),
            ctrl: n("ctrl"),
            degree: n("degree"),
        },
        "BadKnots" => NurbsError::BadKnots { dir: dir(e.get("dir").str()), index: n("index") },
        "NetShape" => NurbsError::NetShape { ctrl: n("ctrl"), n_u: n("n_u"), n_v: n("n_v") },
        "NonFiniteCtrl" => NurbsError::NonFiniteCtrl { index: n("index") },
        "WeightCount" => NurbsError::WeightCount { weights: n("weights"), ctrl: n("ctrl") },
        "BadWeight" => NurbsError::BadWeight { index: n("index") },
        v => panic!("unknown variant {v}"),
    }
}

fn b(x: f64) -> u64 {
    x.to_bits()
}

fn hex(x: f64) -> String {
    format!("0x{:016x}", x.to_bits())
}

// ---------------------------------------------------------------- acceptance criteria

/// AC-1: all 40 valid cases construct; domains (bit-exact) and rational flags match.
#[test]
fn ac1_valid_set() {
    let cases = load(VALID);
    assert_eq!(cases.len(), 40);
    let (mut curves, mut surfaces, mut rational, mut max_deg) = (0, 0, 0, 0);
    for c in &cases {
        let name = c.get("name").str();
        let exp = c.get("expect");
        let built = build(c).unwrap_or_else(|e| panic!("{name}: {e}"));
        let want_rat = exp.get("rational").bool();
        rational += usize::from(want_rat);
        match built {
            Built::Curve(k) => {
                curves += 1;
                max_deg = max_deg.max(k.degree());
                let d = exp.get("domain").arr();
                let (a, z) = k.domain();
                assert_eq!((b(a), b(z)), (d[0].bits(), d[1].bits()), "{name}: domain");
                assert_eq!(k.is_rational(), want_rat, "{name}: rational");
                assert_eq!(k.periodic(), c.get("periodic").bool(), "{name}");
            }
            Built::Surface(s) => {
                surfaces += 1;
                max_deg = max_deg.max(s.degree_u()).max(s.degree_v());
                let d = exp.get("domain").arr();
                let ((u0, u1), (v0, v1)) = s.domain();
                let du = d[0].arr();
                let dv = d[1].arr();
                assert_eq!((b(u0), b(u1)), (du[0].bits(), du[1].bits()), "{name}: u domain");
                assert_eq!((b(v0), b(v1)), (dv[0].bits(), dv[1].bits()), "{name}: v domain");
                assert_eq!(s.is_rational(), want_rat, "{name}: rational");
                assert_eq!(s.shape(), (c.get("n_u").usize(), c.get("n_v").usize()), "{name}");
                assert_eq!((s.periodic_u(), s.periodic_v()), (c.get("periodic_u").bool(), c.get("periodic_v").bool()));
            }
        }
    }
    assert!(curves > 0 && surfaces > 0, "need curves and surfaces");
    assert!(rational > 0 && rational < cases.len(), "need rational and non-rational cases");
    assert_eq!(max_deg, 25);
}

/// AC-2: each invalid case fails with exactly the expected error and index.
#[test]
fn ac2_invalid_set() {
    let cases = load(INVALID);
    let mut seen = std::collections::BTreeSet::new();
    for c in &cases {
        let name = c.get("name").str();
        let want = expected_error(c.get("error"));
        match build(c) {
            Ok(_) => panic!("{name}: accepted, expected {want:?}"),
            Err(got) => assert_eq!(got, want, "{name}"),
        }
        assert!(!want.to_string().is_empty());
        seen.insert(want.code());
    }
    let mut all = ["BadDegree", "CountMismatch", "BadKnots", "NetShape", "NonFiniteCtrl", "WeightCount", "BadWeight"];
    all.sort_unstable();
    assert_eq!(seen.iter().copied().collect::<Vec<_>>(), all.to_vec(), "one case per error variant");
}

/// AC-3: knots, control points and weights come back bit-identical, both through the
/// accessors and through `into_parts`, and re-serialise to the fixture's hex strings.
#[test]
fn ac3_bit_identical_round_trip() {
    for c in &load(VALID) {
        let name = c.get("name").str();
        let ctrl = c.get("ctrl").ptbits();
        let weights = match c.get("weights") {
            J::Null => None,
            w => Some(w.bitvec()),
        };
        let wbits = |w: Option<&[f64]>| w.map(|w| w.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
        let pbits = |p: &[[f64; 3]]| p.iter().map(|q| [b(q[0]), b(q[1]), b(q[2])]).collect::<Vec<_>>();
        match build(c).unwrap() {
            Built::Curve(k) => {
                let knots = c.get("knots").bitvec();
                assert_eq!(k.knots().as_slice().iter().map(|x| x.to_bits()).collect::<Vec<_>>(), knots, "{name}");
                assert_eq!(pbits(k.ctrl()), ctrl, "{name}");
                assert_eq!(wbits(k.weights()), weights, "{name}");
                let ser: Vec<String> = k.knots().as_slice().iter().map(|&x| hex(x)).collect();
                let orig: Vec<&str> = c.get("knots").arr().iter().map(J::str).collect();
                assert_eq!(ser, orig, "{name}: serialised knots");
                let parts = k.into_parts();
                assert_eq!(parts.degree, c.get("degree").usize());
                assert_eq!(parts.knots.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), knots, "{name}");
                assert_eq!(pbits(&parts.ctrl), ctrl, "{name}");
                assert_eq!(wbits(parts.weights.as_deref()), weights, "{name}");
            }
            Built::Surface(s) => {
                let ku = c.get("knots_u").bitvec();
                let kv = c.get("knots_v").bitvec();
                assert_eq!(s.knots_u().as_slice().iter().map(|x| x.to_bits()).collect::<Vec<_>>(), ku, "{name}");
                assert_eq!(s.knots_v().as_slice().iter().map(|x| x.to_bits()).collect::<Vec<_>>(), kv, "{name}");
                assert_eq!(pbits(s.ctrl()), ctrl, "{name}");
                assert_eq!(wbits(s.weights()), weights, "{name}");
                let (n_u, n_v) = s.shape();
                let last = s.ctrl_at(n_u - 1, n_v - 1).unwrap();
                assert_eq!([b(last[0]), b(last[1]), b(last[2])], ctrl[n_u * n_v - 1], "{name}");
                assert!(s.ctrl_at(n_u, 0).is_none() && s.ctrl_at(0, n_v).is_none());
                let parts = s.into_parts();
                assert_eq!(parts.knots_u.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), ku, "{name}");
                assert_eq!(parts.knots_v.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), kv, "{name}");
                assert_eq!(pbits(&parts.ctrl), ctrl, "{name}");
                assert_eq!(wbits(parts.weights.as_deref()), weights, "{name}");
            }
        }
    }
}

// ---------------------------------------------------------------- hand-labelled extras

/// A stand-alone knot vector applies the same rules and reports multiplicities.
#[test]
fn knot_vector_standalone() {
    let kv = KnotVector::new(vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0], 2).unwrap();
    assert_eq!(kv.n_ctrl(), 5);
    assert_eq!(kv.domain(), (0.0, 1.0));
    assert_eq!(kv.multiplicities(), vec![(0.0, 3), (0.5, 2), (1.0, 3)]);
    assert_eq!(kv.clone().into_vec().len(), 8);
    assert_eq!(
        KnotVector::new(vec![0.0, 1.0], 1),
        Err(NurbsError::CountMismatch { dir: Dir::Curve, knots: 2, ctrl: 0, degree: 1 })
    );
    assert_eq!(
        KnotVector::new(vec![], 3),
        Err(NurbsError::CountMismatch { dir: Dir::Curve, knots: 0, ctrl: 0, degree: 3 })
    );
    assert_eq!(KnotVector::new(vec![0.0; 4], 0), Err(NurbsError::BadDegree { dir: Dir::Curve, degree: 0 }));
    assert_eq!(KnotVector::new(vec![0.0, 0.0, 1.0, 1.0], 1).unwrap().degree(), 1);
    assert_eq!(KnotVector::new(vec![0.0, 0.0, 0.0, 0.0], 1), Err(NurbsError::BadKnots { dir: Dir::Curve, index: 2 }));
}

/// Hostile sizes never panic: huge degrees and a net whose n_u * n_v overflows.
#[test]
fn hostile_sizes_do_not_panic() {
    let c = NurbsCurve::new(NurbsCurveParts {
        degree: usize::MAX,
        knots: vec![0.0, 1.0],
        ctrl: vec![[0.0; 3]],
        weights: None,
        periodic: false,
    });
    assert_eq!(c, Err(NurbsError::BadDegree { dir: Dir::Curve, degree: usize::MAX }));
    let s = NurbsSurface::new(NurbsSurfaceParts {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        n_u: 2,
        n_v: 2,
        ctrl: vec![],
        weights: None,
        periodic_u: false,
        periodic_v: false,
    });
    assert_eq!(s, Err(NurbsError::NetShape { ctrl: 0, n_u: 2, n_v: 2 }));
    let s = NurbsSurface::new(NurbsSurfaceParts {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![0.0; 3],
        knots_v: vec![0.0; 4],
        n_u: usize::MAX,
        n_v: 2,
        ctrl: vec![],
        weights: None,
        periodic_u: false,
        periodic_v: false,
    });
    assert_eq!(s, Err(NurbsError::CountMismatch { dir: Dir::U, knots: 3, ctrl: usize::MAX, degree: 1 }));
}
