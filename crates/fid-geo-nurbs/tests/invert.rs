//! GEO.NURBS.INVERT acceptance tests (AC-1 to AC-5), the fuzz set, hand-labelled cases and
//! error paths.
//!
//! Expected values come from outside the Rust code: the GEO.NURBS.REF fixtures
//! `fixtures/geo/nurbs/ref_set_v1.json` and `multi_minima.json` (dense grid search plus
//! 60-digit Newton), `fixtures/geo/nurbs/invert/fuzz_set.json` (the same oracle on extra hard
//! cases, written by `make_invert_fixtures.py`), closed forms for the circle and the spheres,
//! and `fixtures/geo/nurbs/invert/digests-v1.txt` (the bit-exact Python model of the pinned
//! algorithm, `invert_model.py`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::OnceLock;

use fid_geo_nurbs::{
    invert_curve, invert_surface, InvertError, NurbsCurve, NurbsCurveParts, NurbsSurface, NurbsSurfaceParts, Uniqueness,
};
use fid_testkit::sha256_hex;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/nurbs/");
/// AC-1: feet and distances within 1e-12 m.
const TOL: f64 = 1e-12;
const FILES: [&str; 3] = ["ref_set_v1.json", "multi_minima.json", "invert/fuzz_set.json"];

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
    f64::from_bits(u64::from_str_radix(s.strip_prefix("0x").unwrap(), 16).unwrap())
}

fn load(path: &str) -> J {
    parse(&std::fs::read(format!("{DIR}{path}")).unwrap())
}

fn doc(name: &str) -> &'static J {
    static DOCS: OnceLock<BTreeMap<String, J>> = OnceLock::new();
    let docs = DOCS.get_or_init(|| FILES.iter().map(|f| ((*f).to_string(), load(f))).collect());
    docs.get(name).unwrap()
}

// ---------------------------------------------------------------- entities and results

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
            .unwrap(),
        ),
        _ => Built::Surface(
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
            .unwrap(),
        ),
    }
}

/// One inversion: parameters (`v` is absent for curves), foot, distance, flags, certified.
#[derive(Debug, Clone, PartialEq)]
struct Res {
    params: Vec<f64>,
    foot: [f64; 3],
    distance: f64,
    on_boundary: bool,
    unique: bool,
    certified: bool,
}

fn run(b: &Built, q: [f64; 3]) -> Result<Res, String> {
    match b {
        Built::Curve(c) => {
            let (r, ok) = match invert_curve(c, q) {
                Ok(r) => (r, true),
                Err(InvertError::NonConvergence { best }) => (best, false),
                Err(e) => return Err(e.code().to_string()),
            };
            Ok(Res {
                params: vec![r.t],
                foot: r.foot,
                distance: r.distance,
                on_boundary: r.on_boundary,
                unique: r.uniqueness == Uniqueness::Unique,
                certified: ok,
            })
        }
        Built::Surface(s) => {
            let (r, ok) = match invert_surface(s, q) {
                Ok(r) => (r, true),
                Err(InvertError::NonConvergence { best }) => (best, false),
                Err(e) => return Err(e.code().to_string()),
            };
            Ok(Res {
                params: vec![r.u, r.v],
                foot: r.foot,
                distance: r.distance,
                on_boundary: r.on_boundary,
                unique: r.uniqueness == Uniqueness::Unique,
                certified: ok,
            })
        }
    }
}

fn hx(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn line(b: &Built, r: &Result<Res, String>) -> String {
    match r {
        Err(code) => format!("x {code}"),
        Ok(r) => {
            let mut parts = vec![if matches!(b, Built::Curve(_)) { "c" } else { "s" }.to_string()];
            parts.extend(r.params.iter().map(|x| hx(*x)));
            parts.extend(r.foot.iter().map(|x| hx(*x)));
            parts.push(hx(r.distance));
            parts.push(if r.on_boundary { "B" } else { "I" }.to_string());
            parts.push(if r.unique { "U" } else { "N" }.to_string());
            parts.push(if r.certified { "ok" } else { "nc" }.to_string());
            parts.join(" ")
        }
    }
}

/// Every inversion of every entity with queries, computed once and shared by the tests.
struct Run {
    file: &'static str,
    entity: &'static J,
    built_kind_curve: bool,
    rows: Vec<(&'static J, Result<Res, String>, String)>,
}

fn runs() -> &'static [Run] {
    static RUNS: OnceLock<Vec<Run>> = OnceLock::new();
    RUNS.get_or_init(|| {
        let mut out = Vec::new();
        for f in FILES {
            for e in doc(f).get("entities").arr() {
                if e.get("invert").arr().is_empty() {
                    continue;
                }
                let b = build(e);
                let rows = e
                    .get("invert")
                    .arr()
                    .iter()
                    .map(|q| {
                        let r = run(&b, q.get("q").v3());
                        let l = line(&b, &r);
                        (q, r, l)
                    })
                    .collect();
                out.push(Run { file: f, entity: e, built_kind_curve: matches!(b, Built::Curve(_)), rows });
            }
        }
        out
    })
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
}

/// Closed-form (distance, foot) for the named sphere and circle entities: the radial projection
/// (sphere radius 0.5 at the origin) and the nearest point of the unit circle in z = 0. These
/// are exact to a few ulps in binary64, far inside the 1e-12 m tolerance.
fn closed_form(entity: &str, q: [f64; 3]) -> Option<(f64, [f64; 3])> {
    match entity {
        "sphere" | "sphere_c" => {
            let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
            if n == 0.0 {
                return Some((0.5, [0.0; 3]));
            }
            Some(((n - 0.5).abs(), [q[0] * 0.5 / n, q[1] * 0.5 / n, q[2] * 0.5 / n]))
        }
        "circle_c" => {
            let rho = q[0].hypot(q[1]);
            let foot = if rho == 0.0 { [1.0, 0.0, 0.0] } else { [q[0] / rho, q[1] / rho, 0.0] };
            Some(((rho - 1.0).hypot(q[2]), foot))
        }
        _ => None,
    }
}

fn param_keys(curve: bool) -> &'static [&'static str] {
    if curve {
        &["t"]
    } else {
        &["u", "v"]
    }
}

/// Foot and distance errors against the reference (or the closed form where it exists and
/// the query is not tied); `None` for the cusp tips (checked by AC-4).
fn errors(run: &Run, q: &J, r: &Res) -> (f64, f64) {
    let id = run.entity.get("id").str();
    if let Some((d, foot)) = closed_form(id, q.get("q").v3()) {
        if !q.tags().contains(&"tie") {
            return (dist3(r.foot, foot), (r.distance - d).abs());
        }
        let d_err = (r.distance - d).abs().max((r.distance - q.get("distance").f()).abs());
        return (dist3(r.foot, q.get("foot").v3()), d_err);
    }
    (dist3(r.foot, q.get("foot").v3()), (r.distance - q.get("distance").f()).abs())
}

/// A reference parameter equals a domain limit (the GEO.NURBS.REF convention; the oracle's own
/// flag follows its candidate's origin and can disagree on tied queries). Closed-form queries
/// that are not tied have no trustworthy oracle parameters (the oracle misses some near-pole
/// minima): none of the fuzz set's radial queries lies on the boundary.
fn expect_on_boundary(run: &Run, q: &J) -> bool {
    let e = run.entity;
    let tie = q.tags().contains(&"tie");
    if !tie && closed_form(e.get("id").str(), q.get("q").v3()).is_some() {
        return false;
    }
    let pairs: Vec<(f64, Vec<f64>)> = if run.built_kind_curve {
        vec![(q.get("t").f(), e.get("domain").fvec())]
    } else {
        vec![(q.get("u").f(), e.get("domain_u").fvec()), (q.get("v").f(), e.get("domain_v").fvec())]
    };
    pairs.iter().any(|(x, d)| *x == d[0] || *x == d[1])
}

fn cusp_tip(run: &Run, q: &J) -> bool {
    run.entity.get("id").str() == "cusp" && q.get("u").f() == 0.5
}

// ---------------------------------------------------------------- acceptance tests

/// AC-1 and REQ-001: all 5 000 ref_set_v1 queries (including those near several local minima)
/// give the global closest point: foot and distance within 1e-12 m of the GEO.NURBS.REF
/// reference, certified (REQ-002) and unique unless tied. sphere-q0121 is a known defect of the
/// REF fixture (its reference is a local, not the global, minimum; the radial projection is
/// 2.7e-6 m closer), so the sphere is checked against its closed form.
#[test]
fn ac1_ref_set_global_closest_points() {
    let mut worst = (0.0_f64, 0.0_f64);
    let mut n = 0;
    let mut bad = Vec::new();
    for run in runs().iter().filter(|r| r.file == "ref_set_v1.json") {
        for (q, r, _) in &run.rows {
            let id = q.get("id").str();
            let r = r.as_ref().unwrap_or_else(|e| panic!("{id}: {e}"));
            let (fe, de) = errors(run, q, r);
            worst = (worst.0.max(fe), worst.1.max(de));
            n += 1;
            if fe > TOL || de > TOL || !r.certified || r.unique == q.tags().contains(&"tie") {
                bad.push(format!("{id}: foot {fe:e}, distance {de:e}, certified {}, unique {}", r.certified, r.unique));
            }
        }
    }
    eprintln!("ref_set_v1: {n} queries, worst foot error {:e} m, worst distance error {:e} m", worst.0, worst.1);
    assert_eq!(n, 5000);
    assert!(bad.is_empty(), "{bad:#?}");
}

/// AC-2 and REQ-004: on the wavy surface, query points equidistant to the two crests return
/// the lexicographically smallest (u, v) (the reference's choice, smaller than its listed
/// alternative) and are flagged NonUnique; the other queries are unique and within 1e-12 m.
#[test]
fn ac2_ties_choose_smallest_parameters() {
    let run = runs().iter().find(|r| r.entity.get("id").str() == "wavy").unwrap();
    let mut ties = 0;
    for (q, r, _) in &run.rows {
        let id = q.get("id").str();
        let r = r.as_ref().unwrap();
        let (fe, de) = errors(run, q, r);
        assert!(fe <= TOL && de <= TOL && r.certified, "{id}: foot {fe:e}, distance {de:e}");
        if q.tags().contains(&"tie") {
            ties += 1;
            let alt = q.get("alternatives").arr()[0].get("params").fvec();
            let want = [q.get("u").f(), q.get("v").f()];
            // the reference parameters are the 60-digit minimum rounded once; ours agree to
            // the parameter resolution of a 1e-12 m foot
            assert!((r.params[0] - want[0]).abs() <= 1e-11 && (r.params[1] - want[1]).abs() <= 1e-11, "{id}");
            assert!(want[0] < alt[0], "{id}: reference is not the smaller choice");
            assert!(r.params[0] < alt[0] - 1e-3, "{id}: not the lexicographically smaller crest");
            assert!(!r.unique, "{id}: tie not flagged");
        } else {
            assert!(r.unique, "{id}");
        }
    }
    assert!(ties >= 60, "{ties} tied queries");
}

/// AC-3: whenever the closest point lies on a boundary edge, the boundary parameter is exactly
/// the domain limit (bit for bit the reference's), and on_boundary is set exactly then
/// (ref_set_v1 and multi_minima; the fuzz set checks the flag in `fuzz_set_against_oracle`).
#[test]
fn ac3_boundary_parameters_exact() {
    let mut n = 0;
    for run in runs().iter().filter(|r| r.file != "invert/fuzz_set.json") {
        let curve = run.built_kind_curve;
        let doms: Vec<Vec<f64>> = if curve {
            vec![run.entity.get("domain").fvec()]
        } else {
            vec![run.entity.get("domain_u").fvec(), run.entity.get("domain_v").fvec()]
        };
        for (q, r, _) in &run.rows {
            let id = q.get("id").str();
            let Ok(r) = r else { panic!("{id}") };
            assert_eq!(r.on_boundary, q.get("on_boundary").bool(), "{id}");
            for (k, key) in param_keys(curve).iter().enumerate() {
                let want = q.get(key).f();
                if want == doms[k][0] || want == doms[k][1] {
                    n += 1;
                    assert_eq!(r.params[k].to_bits(), want.to_bits(), "{id}: {key}");
                }
            }
        }
    }
    assert!(n >= 500, "{n} boundary parameters");
}

/// AC-4 and REQ-003: at the cusp tip of the synthetic cusp surface (S_u = 0) the orthogonality
/// criterion cannot be met, so NonConvergence is returned with a finite best point, which is
/// the tip within 1e-12 m; every other cusp query converges.
#[test]
fn ac4_cusp_is_non_convergence_with_finite_best() {
    let run = runs().iter().find(|r| r.entity.get("id").str() == "cusp").unwrap();
    let mut tips = 0;
    for (q, r, _) in &run.rows {
        let id = q.get("id").str();
        let r = r.as_ref().unwrap();
        assert!(r.foot.iter().all(|x| x.is_finite()) && r.distance.is_finite(), "{id}");
        let (fe, de) = errors(run, q, r);
        assert!(fe <= TOL && de <= TOL, "{id}: foot {fe:e}, distance {de:e}");
        if cusp_tip(run, q) {
            tips += 1;
            assert!(!r.certified, "{id}: cusp tip certified");
        } else {
            assert!(r.certified, "{id}");
        }
    }
    assert!(tips >= 20, "{tips} cusp-tip queries");
}

/// AC-5 and REQ-006: the results (parameters, foot, distance and flags, bit for bit) of
/// ref_set_v1, multi_minima and the fuzz set equal the bit-exact Python model of the pinned
/// algorithm (per-entity, per-file and total SHA-256 in digests-v1.txt). CI runs this on
/// x86-64 Linux and Windows, ARM64 macOS and wasm32-wasip1.
#[test]
fn ac5_result_digest_is_pinned() {
    let golden = std::fs::read_to_string(format!("{DIR}invert/digests-v1.txt")).unwrap();
    let want: BTreeMap<&str, &str> =
        golden.lines().filter(|l| !l.starts_with('#')).map(|l| l.split_once(' ').unwrap()).collect();
    let mut all = String::new();
    let mut files: BTreeMap<&str, String> = BTreeMap::new();
    let mut bad = Vec::new();
    for run in runs() {
        let text: String = run.rows.iter().map(|(_, _, l)| format!("{l}\n")).collect();
        let key = format!("{}:{}", run.file, run.entity.get("id").str());
        let got = sha256_hex(text.as_bytes());
        if got.strip_prefix("sha256:").unwrap_or(&got) != want[key.as_str()] {
            if bad.is_empty() {
                eprintln!("first differing entity {key}:\n{}", &text[..text.len().min(3000)]);
            }
            bad.push(key);
        }
        files.entry(run.file).or_default().push_str(&text);
        all.push_str(&text);
    }
    assert!(bad.is_empty(), "entities differ from the pinned model: {bad:?}");
    for (f, text) in &files {
        let got = sha256_hex(text.as_bytes());
        assert_eq!(got.strip_prefix("sha256:").unwrap_or(&got), want[f], "{f}");
    }
    let got = sha256_hex(all.as_bytes());
    assert_eq!(got.strip_prefix("sha256:").unwrap_or(&got), want["all"]);
}

/// Fuzz set (fixtures/geo/nurbs/invert/fuzz_set.json): curvature centres and axes (every point
/// closest: NonUnique, smallest parameters), sphere poles, kinks and creases, unclamped and
/// periodic knot vectors, degrees up to 25, near ties and random rational curves and surfaces,
/// against the GEO.NURBS.REF dense-search oracle and closed forms. Reports the worst error.
#[test]
fn fuzz_set_against_oracle() {
    let mut worst = (0.0_f64, 0.0_f64);
    let mut n = 0;
    let mut bad = Vec::new();
    for run in runs().iter().filter(|r| r.file == "invert/fuzz_set.json") {
        for (q, r, _) in &run.rows {
            let id = q.get("id").str();
            let r = r.as_ref().unwrap_or_else(|e| panic!("{id}: {e}"));
            let (fe, de) = errors(run, q, r);
            worst = (worst.0.max(fe), worst.1.max(de));
            n += 1;
            let tie = q.tags().contains(&"tie");
            if fe > TOL || de > TOL || !r.certified || r.unique == tie || r.on_boundary != expect_on_boundary(run, q) {
                bad.push(format!("{id}: foot {fe:e}, distance {de:e}, certified {}, unique {}", r.certified, r.unique));
            }
            if tie {
                let keys = param_keys(run.built_kind_curve);
                let want: Vec<f64> = keys.iter().map(|k| q.get(k).f()).collect();
                if r.params != want {
                    bad.push(format!("{id}: tie parameters {:?}, reference {want:?}", r.params));
                }
            }
        }
    }
    eprintln!("fuzz set: {n} queries, worst foot error {:e} m, worst distance error {:e} m", worst.0, worst.1);
    assert!(n >= 1000, "{n} fuzz queries");
    assert!(bad.is_empty(), "{bad:#?}");
}

// ---------------------------------------------------------------- hand-labelled cases

fn line_curve() -> NurbsCurve {
    NurbsCurve::new(NurbsCurveParts {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        ctrl: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        weights: None,
        periodic: false,
    })
    .unwrap()
}

fn bilinear_plane() -> NurbsSurface {
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

/// Hand-labelled: orthogonal projection onto a straight segment and a flat square, interior
/// and beyond the ends (boundary parameter exactly at the limit).
#[test]
fn hand_labelled_line_and_plane() {
    let c = line_curve();
    let r = invert_curve(&c, [0.5, 1.0, 0.0]).unwrap();
    assert_eq!((r.t, r.foot, r.distance, r.on_boundary), (0.25, [0.5, 0.0, 0.0], 1.0, false));
    assert_eq!(r.uniqueness, Uniqueness::Unique);
    let r = invert_curve(&c, [3.0, 0.0, 4.0]).unwrap();
    assert_eq!((r.t, r.foot, r.on_boundary), (1.0, [2.0, 0.0, 0.0], true));
    assert!((r.distance - 17.0_f64.sqrt()).abs() <= 1e-15);
    let s = bilinear_plane();
    let r = invert_surface(&s, [0.25, 0.75, -2.0]).unwrap();
    assert_eq!((r.u, r.v, r.foot, r.distance, r.on_boundary), (0.25, 0.75, [0.25, 0.75, 0.0], 2.0, false));
    let r = invert_surface(&s, [-1.0, 0.5, 0.0]).unwrap();
    assert_eq!((r.u, r.v, r.on_boundary, r.distance), (0.0, 0.5, true, 1.0));
    // the centre of a square's circumscribed sphere above it: four tied corners? no: the
    // point straight above the centre projects to the centre, unique
    let r = invert_surface(&s, [0.5, 0.5, 3.0]).unwrap();
    assert_eq!((r.u, r.v, r.uniqueness), (0.5, 0.5, Uniqueness::Unique));
}

/// Error paths: a NaN or infinite query coordinate is refused with a typed error.
#[test]
fn non_finite_query_is_refused() {
    let c = line_curve();
    let s = bilinear_plane();
    for q in [[f64::NAN, 0.0, 0.0], [0.0, f64::INFINITY, 0.0], [0.0, 0.0, f64::NEG_INFINITY]] {
        assert_eq!(invert_curve(&c, q), Err(InvertError::NonFiniteQuery));
        assert_eq!(invert_surface(&s, q), Err(InvertError::NonFiniteQuery));
    }
}

/// Extreme coordinates: no panic, and either a finite answer or a typed error (never NaN).
#[test]
fn extreme_coordinates_never_panic() {
    let s = bilinear_plane();
    for q in [[1e300, 0.0, 0.0], [0.0, -1e300, 1e300], [1e-300, 1e-300, 1e-300], [0.5, 0.5, 1e-320]] {
        match invert_surface(&s, q) {
            Ok(r) => assert!(r.distance.is_finite() && r.u.is_finite() && r.v.is_finite(), "{q:?}"),
            Err(InvertError::NonConvergence { best }) => assert!(best.distance.is_finite(), "{q:?}"),
            Err(e) => assert!(matches!(e, InvertError::NonFinite), "{q:?}: {e}"),
        }
    }
    let big = NurbsCurve::new(NurbsCurveParts {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        ctrl: vec![[-1e300, 0.0, 0.0], [0.0, 1e300, 0.0], [1e300, 0.0, 0.0]],
        weights: None,
        periodic: false,
    })
    .unwrap();
    match invert_curve(&big, [0.0, 0.0, 0.0]) {
        Ok(r) => assert!(r.distance.is_finite()),
        Err(InvertError::NonConvergence { best }) => assert!(best.distance.is_finite()),
        Err(e) => assert!(matches!(e, InvertError::NonFinite), "{e}"),
    }
}

/// D0: repeated calls give bit-identical results (no hidden state).
#[test]
fn repeated_calls_are_identical() {
    let rr = runs().iter().find(|r| r.entity.get("id").str() == "s0504n").unwrap();
    let b = build(rr.entity);
    for (q, r, _) in rr.rows.iter().take(40) {
        assert_eq!(&run(&b, q.get("q").v3()), r);
    }
}

/// KNP-1 and CONSTITUTION §8 by inspection of the sources: no fused multiply-add, no
/// `f64::min`/`max`, no hash containers, no `unsafe`, no `unwrap`/`expect`/`panic!` outside
/// the unit tests.
#[test]
fn knp1_source_scan() {
    for f in ["invert.rs", "bezier.rs"] {
        let src = std::fs::read_to_string(format!("{}/src/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let code: String = src
            .split("#[cfg(test)]")
            .next()
            .unwrap()
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .map(|l| format!("{l}\n"))
            .collect();
        let body = code.as_str();
        for bad in ["mul_add", ".min(", ".max(", "HashMap", "HashSet", "unsafe", ".unwrap()", ".expect(", "panic!"] {
            assert!(!body.contains(bad), "{f} contains {bad}");
        }
    }
}
