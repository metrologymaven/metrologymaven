//! NURBS reference fixtures: spec node GEO.NURBS.REF (AC-1 Rust side, AC-2, AC-3, AC-4).
//!
//! The fixtures under `fixtures/geo/nurbs/` come from the independent oracle
//! `tools/fixtures/nurbs_oracle.py` (exact rational arithmetic with `fractions.Fraction`, and
//! 60-digit `decimal` where square roots or trigonometry are needed); nothing here computes
//! expected values. Every f64 is stored as `{hex: bit pattern, dec: shortest decimal}`.
//!
//! The crate has no evaluator yet (GEO.NURBS.EVAL owns it). Until it lands, [`compare_positions`]
//! is exercised with a small de Boor test double: the correct double must agree within the
//! 1e-12 m position tolerance (REQ-GEO.NURBS.REF-005), and the same double with an injected
//! basis-function off-by-one must fail the comparison (AC-4). GEO.NURBS.EVAL plugs its
//! evaluator into `compare_positions` with the same tolerance.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::OnceLock;

use fid_geo_nurbs::{NurbsCurve, NurbsCurveParts, NurbsSurface, NurbsSurfaceParts};
use fid_testkit::sha256_hex;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/nurbs/");
/// REQ-GEO.NURBS.REF-005 (and REQ-GEO.NURBS-003): position tolerance in metres.
const POS_TOL: f64 = 1e-12;
const SEED: f64 = 40.0;
const ORACLE_FILES: [&str; 3] = ["ref_set_v2.json", "analytic_set.json", "multi_minima.json"];
/// Frozen output of oracle 1.0.0, superseded by `ref_set_v2.json` (non-global sphere-q0121).
const SUPERSEDED: &str = "ref_set_v1.json";

// ---------------------------------------------------------------- minimal JSON reader

#[derive(Debug)]
enum J {
    Null,
    Bool(bool),
    Num(f64),
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
        let mut out = String::new();
        loop {
            let c = self.s[self.i];
            self.i += 1;
            match c {
                b'"' => return out,
                b'\\' => {
                    let e = self.s[self.i];
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let h = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap();
                            out.push(char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap());
                            self.i += 4;
                        }
                        other => panic!("unsupported escape \\{}", other as char),
                    }
                }
                _ => {
                    // one character: an ASCII byte or a lead byte plus its continuation bytes
                    let start = self.i - 1;
                    let mut end = self.i;
                    while end < self.s.len() && (self.s[end] & 0xC0) == 0x80 {
                        end += 1;
                    }
                    out.push_str(std::str::from_utf8(&self.s[start..end]).unwrap());
                    self.i = end;
                }
            }
        }
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
                    self.ws();
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
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[start..self.i]).unwrap();
                J::Num(t.parse().unwrap_or_else(|_| panic!("bad number {t}")))
            }
        }
    }
}

fn parse(bytes: &[u8]) -> J {
    let mut p = P { s: bytes, i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, p.s.len(), "trailing data");
    v
}

impl J {
    fn get(&self, k: &str) -> &J {
        match self {
            J::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing key {k}")),
            _ => panic!("not an object (key {k})"),
        }
    }
    fn has(&self, k: &str) -> bool {
        matches!(self, J::Obj(m) if m.contains_key(k))
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
    fn num(&self) -> f64 {
        match self {
            J::Num(x) => *x,
            other => panic!("not a number: {other:?}"),
        }
    }
    fn usize(&self) -> usize {
        let x = self.num();
        assert!(x >= 0.0 && x.fract() == 0.0, "not a count: {x}");
        x as usize
    }
    fn bool(&self) -> bool {
        match self {
            J::Bool(b) => *b,
            other => panic!("not a bool: {other:?}"),
        }
    }
    fn is_null(&self) -> bool {
        matches!(self, J::Null)
    }
    // f64 fields are {hex, dec}; values are taken from the exact bit pattern.
    fn f(&self) -> f64 {
        bits(self.get("hex"))
    }
    fn fvec(&self) -> Vec<f64> {
        self.get("hex").arr().iter().map(bits).collect()
    }
    fn v3(&self) -> [f64; 3] {
        let v = self.fvec();
        assert_eq!(v.len(), 3);
        [v[0], v[1], v[2]]
    }
    fn pts(&self) -> Vec<[f64; 3]> {
        self.get("hex")
            .arr()
            .iter()
            .map(|p| {
                let c = p.arr();
                assert_eq!(c.len(), 3);
                [bits(&c[0]), bits(&c[1]), bits(&c[2])]
            })
            .collect()
    }
    fn weights(&self) -> Option<Vec<f64>> {
        if self.is_null() {
            None
        } else {
            Some(self.fvec())
        }
    }
    fn tags(&self) -> Vec<&str> {
        self.get("tags").arr().iter().map(J::str).collect()
    }
}

fn bits(j: &J) -> f64 {
    let s = j.str();
    assert_eq!(s.len(), 18, "bit pattern {s}");
    f64::from_bits(u64::from_str_radix(s.strip_prefix("0x").unwrap(), 16).unwrap())
}

fn doc(name: &str) -> &'static J {
    static DOCS: OnceLock<BTreeMap<String, J>> = OnceLock::new();
    let docs = DOCS.get_or_init(|| {
        let mut m = BTreeMap::new();
        for f in ORACLE_FILES.iter().chain([SUPERSEDED, "manifest.json"].iter()) {
            let bytes = std::fs::read(format!("{DIR}{f}")).unwrap();
            m.insert((*f).to_string(), parse(&bytes));
        }
        m
    });
    docs.get(name).unwrap()
}

fn entities(name: &str) -> &'static [J] {
    doc(name).get("entities").arr()
}

// ---------------------------------------------------------------- DATA constructors

enum Built {
    Curve(NurbsCurve),
    Surface(NurbsSurface),
}

fn build(e: &J) -> Built {
    match e.get("kind").str() {
        "curve" => Built::Curve(
            NurbsCurve::new(NurbsCurveParts {
                degree: e.get("degree").usize(),
                knots: e.get("knots").fvec(),
                ctrl: e.get("ctrl").pts(),
                weights: e.get("weights").weights(),
                periodic: e.get("periodic").bool(),
            })
            .unwrap_or_else(|err| panic!("{}: {err}", e.get("id").str())),
        ),
        "surface" => Built::Surface(
            NurbsSurface::new(NurbsSurfaceParts {
                degree_u: e.get("degree_u").usize(),
                degree_v: e.get("degree_v").usize(),
                knots_u: e.get("knots_u").fvec(),
                knots_v: e.get("knots_v").fvec(),
                n_u: e.get("n_u").usize(),
                n_v: e.get("n_v").usize(),
                ctrl: e.get("ctrl").pts(),
                weights: e.get("weights").weights(),
                periodic_u: e.get("periodic_u").bool(),
                periodic_v: e.get("periodic_v").bool(),
            })
            .unwrap_or_else(|err| panic!("{}: {err}", e.get("id").str())),
        ),
        k => panic!("unknown kind {k}"),
    }
}

// ---------------------------------------------------------------- comparison harness

/// A position evaluator under test: curve at `t`, or surface at `(u, v)`.
trait PositionEvaluator {
    fn curve(&self, c: &NurbsCurve, t: f64) -> [f64; 3];
    fn surface(&self, s: &NurbsSurface, u: f64, v: f64) -> [f64; 3];
}

/// Largest position error over every `eval` row of `ref_set_v2` and the number of rows whose
/// error is not within [`POS_TOL`] (a NaN counts as outside).
fn compare_positions(ev: &dyn PositionEvaluator) -> (f64, usize, usize) {
    let mut worst = 0.0_f64;
    let mut fails = 0;
    let mut total = 0;
    for e in entities("ref_set_v2.json") {
        let built = build(e);
        for r in e.get("eval").arr() {
            let want = r.get("p").v3();
            let got = match &built {
                Built::Curve(c) => ev.curve(c, r.get("t").f()),
                Built::Surface(s) => ev.surface(s, r.get("u").f(), r.get("v").f()),
            };
            let err = (got[0] - want[0]).abs().max((got[1] - want[1]).abs()).max((got[2] - want[2]).abs());
            total += 1;
            if err.is_nan() || err > POS_TOL {
                fails += 1;
            }
            if !err.is_nan() && err > worst {
                worst = err;
            }
        }
    }
    (worst, fails, total)
}

/// Test double: de Boor's algorithm on homogeneous points (span rule right-continuous, last
/// non-empty span at the domain end). `off_by_one` injects a basis-function bug: the knot index
/// in the blending factor is shifted by one.
struct DeBoorDouble {
    off_by_one: bool,
}

impl DeBoorDouble {
    fn span(knots: &[f64], p: usize, n: usize, t: f64) -> usize {
        if t == knots[n] {
            let mut s = n - 1;
            while knots[s] == knots[s + 1] {
                s -= 1;
            }
            return s;
        }
        let mut s = p;
        while s + 1 < n && knots[s + 1] <= t {
            s += 1;
        }
        s
    }

    fn de_boor(&self, knots: &[f64], p: usize, pw: &[[f64; 4]], t: f64) -> [f64; 4] {
        let n = pw.len();
        let s = Self::span(knots, p, n, t);
        let mut d: Vec<[f64; 4]> = pw[s - p..=s].to_vec();
        let shift = usize::from(self.off_by_one);
        for r in 1..=p {
            for j in (r..=p).rev() {
                let k = j + s - p + shift;
                let a = (t - knots[k]) / (knots[j + 1 + s - r] - knots[k]);
                let b = 1.0 - a;
                let (lo, hi) = (d[j - 1], d[j]);
                d[j] = [b * lo[0] + a * hi[0], b * lo[1] + a * hi[1], b * lo[2] + a * hi[2], b * lo[3] + a * hi[3]];
            }
        }
        d[p]
    }
}

fn homogeneous(ctrl: &[[f64; 3]], weights: Option<&[f64]>) -> Vec<[f64; 4]> {
    ctrl.iter()
        .enumerate()
        .map(|(i, c)| {
            let w = weights.map_or(1.0, |w| w[i]);
            [c[0] * w, c[1] * w, c[2] * w, w]
        })
        .collect()
}

fn project(h: [f64; 4]) -> [f64; 3] {
    [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
}

impl PositionEvaluator for DeBoorDouble {
    fn curve(&self, c: &NurbsCurve, t: f64) -> [f64; 3] {
        let pw = homogeneous(c.ctrl(), c.weights());
        project(self.de_boor(c.knots().as_slice(), c.degree(), &pw, t))
    }

    fn surface(&self, s: &NurbsSurface, u: f64, v: f64) -> [f64; 3] {
        let (n_u, n_v) = s.shape();
        let pw = homogeneous(s.ctrl(), s.weights());
        let rows: Vec<[f64; 4]> =
            pw.chunks(n_v).map(|row| self.de_boor(s.knots_v().as_slice(), s.degree_v(), row, v)).collect();
        assert_eq!(rows.len(), n_u);
        project(self.de_boor(s.knots_u().as_slice(), s.degree_u(), &rows, u))
    }
}

// ---------------------------------------------------------------- tests

/// AC-1 (Rust side): every file matches the SHA-256 in the manifest, oracle outputs carry the
/// manifest's oracle version and seed, and the adopted GEO.NURBS.DATA sets are pinned too.
#[test]
fn ac1_manifest_matches_files() {
    let man = doc("manifest.json");
    assert_eq!(man.get("seed").num(), SEED);
    let version = man.get("oracle_version").str();
    let mut names = Vec::new();
    for f in man.get("files").arr() {
        let path = f.get("path").str();
        names.push(path.to_string());
        let bytes = std::fs::read(format!("{DIR}{path}")).unwrap();
        assert_eq!(sha256_hex(&bytes), f.get("sha256").str(), "{path}: SHA-256 differs from the manifest");
        if f.has("adopted") {
            continue;
        }
        if f.has("superseded") {
            assert_eq!(path, SUPERSEDED, "unexpected superseded file");
            assert_eq!(f.get("superseded_by").str(), "ref_set_v2.json");
            assert_eq!(f.get("oracle_version").str(), "nurbs_oracle 1.0.0");
            continue;
        }
        assert_eq!(f.get("oracle_version").str(), version, "{path}");
        let d = doc(path);
        assert_eq!(d.get("oracle_version").str(), version, "{path}");
        assert_eq!(d.get("seed").num(), SEED, "{path}");
        assert_eq!(d.get("file_seed").num(), f.get("seed").num(), "{path}");
        assert_eq!(d.get("license").str(), "CC-BY-4.0");
    }
    for want in ORACLE_FILES.iter().chain(["valid_set.json", "invalid_set.json", SUPERSEDED].iter()) {
        assert!(names.iter().any(|n| n == want), "{want} missing from the manifest");
    }
}

fn check_hex_dec(j: &J, ctx: &str) {
    match j {
        J::Obj(m) if m.len() == 2 && m.contains_key("hex") && m.contains_key("dec") => {
            fn pair(h: &J, d: &J, ctx: &str) {
                match (h, d) {
                    (J::Arr(a), J::Arr(b)) => {
                        assert_eq!(a.len(), b.len(), "{ctx}: hex/dec shapes differ");
                        a.iter().zip(b).for_each(|(x, y)| pair(x, y, ctx));
                    }
                    (J::Str(_), J::Num(x)) => assert_eq!(bits(h).to_bits(), x.to_bits(), "{ctx}: {h:?} vs {x}"),
                    _ => panic!("{ctx}: malformed f64 pair"),
                }
            }
            pair(&m["hex"], &m["dec"], ctx);
        }
        J::Obj(m) => m.values().for_each(|v| check_hex_dec(v, ctx)),
        J::Arr(v) => v.iter().for_each(|x| check_hex_dec(x, ctx)),
        _ => {}
    }
}

/// Data format: every hex bit pattern and its decimal are the same binary64 value.
#[test]
fn hex_and_decimal_agree() {
    for f in ORACLE_FILES {
        check_hex_dec(doc(f), f);
    }
}

/// Every NURBS entity is valid input for the GEO.NURBS.DATA constructors and reports the
/// domain the oracle used.
#[test]
fn entities_construct_with_matching_domains() {
    for f in ["ref_set_v2.json", "multi_minima.json"] {
        for e in entities(f) {
            let id = e.get("id").str();
            match build(e) {
                Built::Curve(c) => {
                    let d = e.get("domain").fvec();
                    assert_eq!(c.domain(), (d[0], d[1]), "{id}");
                    // DATA calls a curve rational when its weights are not all equal
                    let varied = e.get("weights").weights().is_some_and(|w| w.iter().any(|x| *x != w[0]));
                    assert_eq!(c.is_rational(), varied, "{id}");
                }
                Built::Surface(s) => {
                    let du = e.get("domain_u").fvec();
                    let dv = e.get("domain_v").fvec();
                    assert_eq!(s.domain(), ((du[0], du[1]), (dv[0], dv[1])), "{id}");
                }
            }
        }
    }
}

/// AC-2: the NURBS unit circle's reference points (at least 100 parameters) have radius 1
/// within 1e-15 and lie in z = 0.
#[test]
fn ac2_circle_reference_points_on_unit_circle() {
    let circle = entities("ref_set_v2.json").iter().find(|e| e.get("id").str() == "circle").unwrap();
    let rows = circle.get("eval").arr();
    assert!(rows.len() >= 100, "circle has {} parameters", rows.len());
    for r in rows {
        let p = r.get("p").v3();
        let radius = (p[0] * p[0] + p[1] * p[1]).sqrt();
        assert!((radius - 1.0).abs() <= 1e-15, "t = {}: radius {radius}", r.get("t").f());
        assert_eq!(p[2], 0.0);
    }
}

fn repeated_interior(knots: &[f64], p: usize) -> bool {
    let n = knots.len() - p - 1;
    let (lo, hi) = (knots[p], knots[n]);
    knots.windows(2).any(|w| w[0] == w[1] && w[0] > lo && w[0] < hi)
}

/// AC-3 (Rust side; `tools/fixtures/nurbs_coverage.py` is the script): degrees 1..25 rational
/// and non-rational, repeated interior knots, the named circle, sphere and cylinder, the eight
/// analytic types, tied multi-minima queries and at least 5000 inversion queries.
#[test]
fn ac3_fixture_coverage() {
    let ents = entities("ref_set_v2.json");
    for p in 1..=25 {
        for rational in [false, true] {
            assert!(
                ents.iter().any(|e| e.get("kind").str() == "curve"
                    && e.get("degree").usize() == p
                    && e.get("weights").is_null() != rational),
                "degree {p} rational={rational} missing"
            );
        }
    }
    let curves_rep = ents
        .iter()
        .filter(|e| e.get("kind").str() == "curve")
        .any(|e| repeated_interior(&e.get("knots").fvec(), e.get("degree").usize()));
    let surfaces_rep = ents.iter().filter(|e| e.get("kind").str() == "surface").any(|e| {
        repeated_interior(&e.get("knots_u").fvec(), e.get("degree_u").usize())
            || repeated_interior(&e.get("knots_v").fvec(), e.get("degree_v").usize())
    });
    assert!(curves_rep && surfaces_rep, "repeated interior knots missing");
    for named in ["circle", "sphere", "cylinder"] {
        assert!(ents.iter().any(|e| e.get("id").str() == named), "{named} missing");
    }
    let inversions: usize = ents.iter().map(|e| e.get("invert").arr().len()).sum();
    assert!(inversions >= 5000, "{inversions} inversion queries");
    let mut types: Vec<&str> = entities("analytic_set.json").iter().map(|e| e.get("type").str()).collect();
    types.sort_unstable();
    types.dedup();
    assert_eq!(types, ["circle", "cone", "cylinder", "ellipse", "line", "plane", "sphere", "torus"]);
    let ties =
        entities("multi_minima.json").iter().flat_map(|e| e.get("invert").arr()).filter(|q| q.tags().contains(&"tie"));
    assert!(ties.count() >= 10);
}

/// AC-4: the comparison harness accepts a correct evaluator (the de Boor double, within
/// 1e-12 m everywhere, degrees 1..25) and rejects the same evaluator with an injected
/// basis-function off-by-one: the fixtures detect a real bug.
#[test]
fn ac4_injected_off_by_one_is_caught() {
    let (worst, fails, total) = compare_positions(&DeBoorDouble { off_by_one: false });
    assert!(total >= 1500, "{total} reference positions");
    assert_eq!(fails, 0, "correct double: {fails} of {total} outside {POS_TOL} m (worst {worst:e})");
    let (_, fails, total) = compare_positions(&DeBoorDouble { off_by_one: true });
    assert!(fails > 0, "injected off-by-one went unnoticed");
    assert!(fails * 2 > total, "off-by-one caught only at {fails} of {total} positions");
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1]) + (a[2] - b[2]) * (a[2] - b[2])).sqrt()
}

/// Inversion records are self-consistent: distance = |q - foot|, boundary answers sit exactly on
/// the domain limit, ties carry their alternative, and parameters lie in the domain.
#[test]
fn inversion_records_are_consistent() {
    for f in ["ref_set_v2.json", "multi_minima.json"] {
        for e in entities(f) {
            let surface = e.get("kind").str() == "surface";
            for q in e.get("invert").arr() {
                let id = q.get("id").str();
                let d = q.get("distance").f();
                assert!((dist(q.get("q").v3(), q.get("foot").v3()) - d).abs() <= POS_TOL, "{id}");
                let params: Vec<(f64, Vec<f64>)> = if surface {
                    vec![(q.get("u").f(), e.get("domain_u").fvec()), (q.get("v").f(), e.get("domain_v").fvec())]
                } else {
                    vec![(q.get("t").f(), e.get("domain").fvec())]
                };
                for (x, dom) in &params {
                    assert!((dom[0]..=dom[1]).contains(x), "{id}: parameter outside the domain");
                }
                let on_edge = params.iter().any(|(x, dom)| *x == dom[0] || *x == dom[1]);
                assert_eq!(q.get("on_boundary").bool(), on_edge, "{id}");
                if q.tags().contains(&"tie") {
                    let alt = q.get("alternatives").arr();
                    assert!(!alt.is_empty(), "{id}: tie without alternative");
                    assert!((alt[0].get("distance").f() - d).abs() <= 1e-15, "{id}");
                }
            }
        }
    }
    for e in entities("analytic_set.json") {
        for q in e.get("invert").arr() {
            let d = q.get("distance").f();
            assert!((dist(q.get("q").v3(), q.get("foot").v3()) - d).abs() <= 1e-13, "{}", q.get("id").str());
        }
    }
}

/// Spot checks of the named surfaces against their closed-form geometry: the sphere poles are
/// degenerate (no normal), and the 10 mm NURBS cylinder has curvatures 0 and -100 per metre
/// (signed against the outward normal; GEO.NURBS.EVAL AC-2, AC-3).
#[test]
fn named_surfaces_regularity_and_curvature() {
    let ents = entities("ref_set_v2.json");
    let sphere = ents.iter().find(|e| e.get("id").str() == "sphere").unwrap();
    let poles: Vec<&J> =
        sphere.get("eval").arr().iter().filter(|r| r.has("tag") && r.get("tag").str() == "pole").collect();
    assert!(!poles.is_empty());
    for r in poles {
        assert_eq!(r.get("regularity").str(), "degenerate");
        assert!(r.get("normal").is_null());
        let p = r.get("p").v3();
        assert!((p[2].abs() - 0.5).abs() <= POS_TOL);
    }
    let cyl = ents.iter().find(|e| e.get("id").str() == "cylinder").unwrap();
    let rows = cyl.get("eval").arr();
    assert!(rows.len() >= 100);
    for r in rows {
        assert_eq!(r.get("regularity").str(), "regular");
        assert!(r.get("k1").f().abs() <= 1e-7);
        assert!((r.get("k2").f() + 100.0).abs() <= 1e-7);
    }
}

/// GEO.NURBS.EVAL plugged into the harness: the real evaluator (order 0) is within the 1e-12 m
/// position tolerance at every reference position.
struct RealEvaluator;

impl PositionEvaluator for RealEvaluator {
    fn curve(&self, c: &NurbsCurve, t: f64) -> [f64; 3] {
        fid_geo_nurbs::Curve::eval(c, t, 0).unwrap().p
    }

    fn surface(&self, s: &NurbsSurface, u: f64, v: f64) -> [f64; 3] {
        fid_geo_nurbs::Surface::eval(s, u, v, 0).unwrap().p
    }
}

#[test]
fn real_evaluator_passes_the_reference_harness() {
    let (worst, fails, total) = compare_positions(&RealEvaluator);
    assert!(total >= 1500, "{total} reference positions");
    assert_eq!(fails, 0, "{fails} of {total} outside {POS_TOL} m (worst {worst:e})");
}

/// ref_set_v2 supersedes ref_set_v1 only in its inversion rows: geometry, `eval` and
/// `domain_checks` rows are identical, so consumers of the v1 evaluation rows (GEO.NURBS.EVAL)
/// stay valid.
#[test]
fn v2_keeps_v1_geometry_and_evaluation_rows() {
    let v1 = doc(SUPERSEDED).get("entities").arr();
    let v2 = entities("ref_set_v2.json");
    assert_eq!(v1.len(), v2.len());
    for (a, b) in v1.iter().zip(v2) {
        let id = a.get("id").str();
        assert_eq!(id, b.get("id").str());
        let (J::Obj(ma), J::Obj(mb)) = (a, b) else { panic!("{id}: not an object") };
        for (k, va) in ma {
            if k == "invert" {
                assert_eq!(va.arr().len(), b.get("invert").arr().len(), "{id}");
                continue;
            }
            assert_eq!(format!("{va:?}"), format!("{:?}", mb[k]), "{id}: {k} differs between v1 and v2");
        }
    }
}

/// The v2 sphere references are the radial projection: distance | |q| - 0.5 | (the case that
/// v1 got wrong, sphere-q0121, included).
#[test]
fn v2_sphere_references_match_the_closed_form() {
    let sphere = entities("ref_set_v2.json").iter().find(|e| e.get("id").str() == "sphere").unwrap();
    for q in sphere.get("invert").arr() {
        let p = q.get("q").v3();
        let want = ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 0.5).abs();
        let got = q.get("distance").f();
        assert!((got - want).abs() <= 2e-15, "{}: {got} vs closed form {want}", q.get("id").str());
    }
}
