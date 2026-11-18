//! Reader and comparator for the GEO.DIST.FACE.REF fixtures (`fixtures/geo/dist/face/ref_v1`).
//!
//! Test support only: a small JSON reader (the kernel crate has no dependencies), the
//! fixture model, the 1e-9 m comparison, and a trim-ignoring test double.
#![allow(dead_code)]

use fid_geo_dist::{FACE_REF_FOOT_TOL_M, FACE_TIE_WINDOW_M};

pub const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/dist/face/ref_v1/");

// ------------------------------------------------------------------ JSON

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

static NULL: Json = Json::Null;

impl Json {
    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(v) => v.iter().find(|(k, _)| k == key).map_or(&NULL, |(_, x)| x),
            _ => &NULL,
        }
    }
    pub fn num(&self) -> f64 {
        match self {
            Json::Num(x) => *x,
            other => panic!("expected a number, got {other:?}"),
        }
    }
    pub fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("expected a string, got {other:?}"),
        }
    }
    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            other => panic!("expected an array, got {other:?}"),
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }
    pub fn truthy(&self) -> bool {
        matches!(self, Json::Bool(true))
    }
}

pub fn parse_json(text: &str) -> Json {
    let mut p = Parser { b: text.as_bytes(), i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, p.b.len(), "trailing bytes after JSON value");
    v
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn peek(&self) -> u8 {
        self.b[self.i]
    }
    fn expect(&mut self, c: u8) {
        assert_eq!(self.peek(), c, "JSON: expected '{}' at byte {}", c as char, self.i);
        self.i += 1;
    }
    fn literal(&mut self, word: &str, v: Json) -> Json {
        assert!(self.b[self.i..].starts_with(word.as_bytes()), "JSON: bad literal at byte {}", self.i);
        self.i += word.len();
        v
    }
    fn value(&mut self) -> Json {
        self.ws();
        match self.peek() {
            b'{' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.peek() == b'}' {
                    self.i += 1;
                    return Json::Obj(v);
                }
                loop {
                    self.ws();
                    let k = self.string();
                    self.ws();
                    self.expect(b':');
                    let x = self.value();
                    v.push((k, x));
                    self.ws();
                    if self.peek() == b',' {
                        self.i += 1;
                    } else {
                        self.expect(b'}');
                        return Json::Obj(v);
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.peek() == b']' {
                    self.i += 1;
                    return Json::Arr(v);
                }
                loop {
                    v.push(self.value());
                    self.ws();
                    if self.peek() == b',' {
                        self.i += 1;
                    } else {
                        self.expect(b']');
                        return Json::Arr(v);
                    }
                }
            }
            b'"' => Json::Str(self.string()),
            b'n' => self.literal("null", Json::Null),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            _ => {
                let start = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.i += 1;
                }
                let s = std::str::from_utf8(&self.b[start..self.i]).unwrap();
                Json::Num(s.parse::<f64>().unwrap_or_else(|_| panic!("JSON: bad number {s:?}")))
            }
        }
    }
    fn string(&mut self) -> String {
        self.expect(b'"');
        let mut out = Vec::new();
        loop {
            let c = self.b[self.i];
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).unwrap(),
                b'\\' => {
                    let e = self.b[self.i];
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let h = std::str::from_utf8(&self.b[self.i..self.i + 4]).unwrap();
                            self.i += 4;
                            let ch = char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap();
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => panic!("JSON: bad escape"),
                    }
                }
                _ => out.push(c),
            }
        }
    }
}

/// Parses an IEEE double written by Python's `float.hex()` (for example `-0x1.8p-3`).
pub fn parse_hex_f64(s: &str) -> f64 {
    let (neg, t) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let t = t.strip_prefix("0x").unwrap_or_else(|| panic!("hex float without 0x: {s}"));
    let (mant, exp) = t.split_once('p').unwrap_or_else(|| panic!("hex float without exponent: {s}"));
    let exp: i32 = exp.parse().unwrap();
    let (ip, fp) = mant.split_once('.').unwrap_or((mant, ""));
    let ip = u64::from_str_radix(ip, 16).unwrap();
    assert!(fp.len() <= 13, "hex float with too many digits: {s}");
    let mut frac = if fp.is_empty() { 0 } else { u64::from_str_radix(fp, 16).unwrap() };
    frac <<= 4 * (13 - fp.len() as u32);
    let bits = if ip == 0 && frac == 0 {
        0
    } else if ip == 1 {
        let be = exp + 1023;
        assert!((1..=2046).contains(&be), "exponent out of range: {s}");
        ((be as u64) << 52) | frac
    } else {
        assert!(ip == 0 && exp == -1022, "not a canonical hex float: {s}");
        frac
    };
    f64::from_bits(bits | if neg { 1 << 63 } else { 0 })
}

// ------------------------------------------------------------------ fixture model

pub type V3 = [f64; 3];

#[derive(Debug, Clone, PartialEq)]
pub enum Feature {
    Interior,
    Edge(String),
    Vertex(String),
}

#[derive(Debug, Clone)]
pub struct Answer {
    pub face: String,
    pub feature: Feature,
    pub foot: V3,
    pub distance: f64,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub id: String,
    pub gen: String,
    pub tags: Vec<String>,
    pub p: V3,
    pub answer: Answer,
    pub alternatives: Vec<Answer>,
    pub edge_distance: Option<f64>,
    pub foot_edge_distance: Option<f64>,
    pub untrimmed_face: String,
    pub untrimmed_foot: V3,
    pub untrimmed_distance: f64,
}

impl Query {
    pub fn has(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

pub struct Fixture {
    pub file: String,
    pub analytic: bool,
    pub body: Json,
    pub queries: Vec<Query>,
}

/// A scalar written as `{hex, dec}`; both spellings must give the same bits.
fn scalar(j: &Json) -> f64 {
    let h = parse_hex_f64(j.get("hex").str());
    let d = j.get("dec").num();
    assert_eq!(h.to_bits(), d.to_bits(), "hex and decimal disagree: {j:?}");
    h
}

fn vector(j: &Json) -> V3 {
    let (h, d) = (j.get("hex").arr(), j.get("dec").arr());
    assert!(h.len() == 3 && d.len() == 3);
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let x = parse_hex_f64(h[k].str());
        assert_eq!(x.to_bits(), d[k].num().to_bits(), "hex and decimal disagree: {j:?}");
        *o = x;
    }
    out
}

fn feature(j: &Json) -> Feature {
    match j.get("kind").str() {
        "interior" => Feature::Interior,
        "edge" => Feature::Edge(j.get("id").str().to_string()),
        "vertex" => Feature::Vertex(j.get("id").str().to_string()),
        k => panic!("unknown feature kind {k}"),
    }
}

fn answer(j: &Json) -> Answer {
    Answer {
        face: j.get("face").str().to_string(),
        feature: feature(j.get("feature")),
        foot: vector(j.get("foot")),
        distance: scalar(j.get("distance")),
    }
}

fn opt_scalar(j: &Json) -> Option<f64> {
    if j.is_null() {
        None
    } else {
        Some(scalar(j))
    }
}

pub fn load(file: &str) -> Fixture {
    let text = std::fs::read_to_string(format!("{FIX}{file}")).unwrap();
    let doc = parse_json(&text);
    assert_eq!(doc.get("format").str(), "fid-face-ref/1");
    let queries = doc
        .get("queries")
        .arr()
        .iter()
        .map(|q| {
            let u = q.get("untrimmed");
            Query {
                id: q.get("id").str().to_string(),
                gen: q.get("gen").str().to_string(),
                tags: q.get("tags").arr().iter().map(|t| t.str().to_string()).collect(),
                p: vector(q.get("p")),
                answer: answer(q),
                alternatives: q.get("alternatives").arr().iter().map(answer).collect(),
                edge_distance: opt_scalar(q.get("edge_distance")),
                foot_edge_distance: opt_scalar(q.get("foot_edge_distance")),
                untrimmed_face: u.get("face").str().to_string(),
                untrimmed_foot: vector(u.get("foot")),
                untrimmed_distance: scalar(u.get("distance")),
            }
        })
        .collect();
    Fixture { file: file.to_string(), analytic: doc.get("analytic").truthy(), body: doc.get("body").clone(), queries }
}

/// Body files listed in the manifest, in manifest order.
pub fn manifest_files() -> Vec<String> {
    let man = parse_json(&std::fs::read_to_string(format!("{FIX}manifest.json")).unwrap());
    man.get("files").arr().iter().map(|f| f.get("path").str().to_string()).collect()
}

// ------------------------------------------------------------------ comparison

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn add_scaled(a: V3, s: f64, b: V3) -> V3 {
    [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2]]
}

/// What a projector under test returns for one query.
#[derive(Debug, Clone)]
pub struct Hit {
    pub face: String,
    pub feature: Feature,
    pub foot: V3,
    pub distance: f64,
}

/// True when `hit` matches the reference answer or one of its listed alternatives: same face,
/// foot within 1e-9 m, distance within 1e-9 m, and the same feature (the feature is not
/// compared when the reference foot lies within 1e-9 m of an edge).
pub fn matches(q: &Query, hit: &Hit) -> bool {
    let feature_free = q.foot_edge_distance.is_some_and(|d| d <= FACE_REF_FOOT_TOL_M);
    std::iter::once(&q.answer).chain(q.alternatives.iter()).any(|a| {
        a.face == hit.face
            && norm(sub(a.foot, hit.foot)) <= FACE_REF_FOOT_TOL_M
            && (a.distance - hit.distance).abs() <= FACE_REF_FOOT_TOL_M
            && (feature_free || a.feature == hit.feature)
    })
}

/// IDs of the queries a projector gets wrong.
pub fn compare(fx: &Fixture, projector: &dyn Fn(&Fixture, &Query) -> Hit) -> Vec<String> {
    fx.queries.iter().filter(|q| !matches(q, &projector(fx, q))).map(|q| q.id.clone()).collect()
}

// ------------------------------------------------------------------ analytic surfaces

fn v3(j: &Json) -> V3 {
    let a = j.arr();
    [a[0].num(), a[1].num(), a[2].num()]
}

/// An analytic face surface from the body description (`None` for NURBS).
#[derive(Debug, Clone)]
pub enum Surf {
    Plane { o: V3, n: V3 },
    Cylinder { o: V3, z: V3, r: f64 },
    Cone { o: V3, z: V3, r0: f64, k: f64 },
    Sphere { c: V3, r: f64 },
    Torus { o: V3, z: V3, big: f64, small: f64 },
}

pub fn surface(j: &Json) -> Option<Surf> {
    let pl = j.get("placement");
    let (o, z) = if pl.is_null() { ([0.0; 3], [0.0; 3]) } else { (v3(pl.get("origin")), v3(pl.get("axis"))) };
    Some(match j.get("type").str() {
        "plane" => Surf::Plane { o, n: z },
        "cylinder" => Surf::Cylinder { o, z, r: j.get("radius").num() },
        "cone" => Surf::Cone { o, z, r0: j.get("radius").num(), k: j.get("half_angle").num().tan() },
        "sphere" => Surf::Sphere { c: o, r: j.get("radius").num() },
        "torus" => Surf::Torus { o, z, big: j.get("major_radius").num(), small: j.get("minor_radius").num() },
        _ => return None,
    })
}

/// Axial coordinate, radial distance and radial unit vector of `p` about the axis (o, z).
fn axial(p: V3, o: V3, z: V3) -> (f64, f64, V3) {
    let w = sub(p, o);
    let h = dot(w, z);
    let rad = add_scaled(w, -h, z);
    let rho = norm(rad);
    (h, rho, [rad[0] / rho, rad[1] / rho, rad[2] / rho])
}

impl Surf {
    /// Closest point on the untrimmed (infinite) surface.
    pub fn closest(&self, p: V3) -> V3 {
        match *self {
            Surf::Plane { o, n } => add_scaled(p, -dot(sub(p, o), n), n),
            Surf::Cylinder { o, z, r } => {
                let (h, _, u) = axial(p, o, z);
                add_scaled(add_scaled(o, h, z), r, u)
            }
            Surf::Cone { o, z, r0, k } => {
                // The double cone meets the meridian plane in two lines through the apex.
                let (h, rho, u) = axial(p, o, z);
                let (mut best_d, mut best) = (f64::INFINITY, p);
                for sgn in [1.0, -1.0] {
                    let (a0, d0) = (sgn * r0, sgn * k);
                    let t = ((rho - a0) * d0 + h) / (d0 * d0 + 1.0);
                    let foot = add_scaled(add_scaled(o, t, z), a0 + t * d0, u);
                    let d = norm(sub(p, foot));
                    if d < best_d {
                        (best_d, best) = (d, foot);
                    }
                }
                best
            }
            Surf::Sphere { c, r } => {
                let w = sub(p, c);
                add_scaled(c, r / norm(w), w)
            }
            Surf::Torus { o, z, big, small } => {
                let (_, _, u) = axial(p, o, z);
                let c = add_scaled(o, big, u);
                let w = sub(p, c);
                add_scaled(c, small / norm(w), w)
            }
        }
    }

    /// Distance of `x` from the surface, from its implicit form.
    pub fn residual(&self, x: V3) -> f64 {
        match *self {
            Surf::Plane { o, n } => dot(sub(x, o), n).abs(),
            Surf::Cylinder { o, z, r } => (axial(x, o, z).1 - r).abs(),
            Surf::Cone { o, z, r0, k } => {
                let (h, rho, _) = axial(x, o, z);
                (rho - (r0 + h * k)).abs() / (1.0 + k * k).sqrt()
            }
            Surf::Sphere { c, r } => (norm(sub(x, c)) - r).abs(),
            Surf::Torus { o, z, big, small } => {
                let (h, rho, _) = axial(x, o, z);
                (((rho - big) * (rho - big) + h * h).sqrt() - small).abs()
            }
        }
    }
}

/// Face labels and analytic surfaces of a body, in face-ID order (labels bytewise).
pub fn faces(body: &Json) -> Vec<(String, Option<Surf>)> {
    let mut v: Vec<(String, Option<Surf>)> =
        body.get("faces").arr().iter().map(|f| (f.get("label").str().to_string(), surface(f.get("surface")))).collect();
    v.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    v
}

/// AC-3 test double: projects onto every face's untrimmed surface and ignores all trimming.
/// Analytic bodies are projected here in closed form; for NURBS bodies the double returns the
/// fixture's recorded untrimmed (domain-patch) answer.
pub fn trim_ignoring(fx: &Fixture, q: &Query) -> Hit {
    if !fx.analytic {
        return Hit {
            face: q.untrimmed_face.clone(),
            feature: Feature::Interior,
            foot: q.untrimmed_foot,
            distance: q.untrimmed_distance,
        };
    }
    let mut cands: Vec<(String, V3, f64)> = Vec::new();
    for (label, s) in faces(&fx.body) {
        let s = s.unwrap_or_else(|| panic!("{}: analytic body with a non-analytic face", fx.file));
        let foot = s.closest(q.p);
        cands.push((label, foot, norm(sub(q.p, foot))));
    }
    let mut dmin = f64::INFINITY;
    for c in &cands {
        if c.2 < dmin {
            dmin = c.2;
        }
    }
    let (face, foot, distance) = cands.into_iter().find(|c| c.2 <= dmin + FACE_TIE_WINDOW_M).unwrap();
    Hit { face, feature: Feature::Interior, foot, distance }
}

/// Test double that returns the reference answer itself.
pub fn reference(_: &Fixture, q: &Query) -> Hit {
    Hit {
        face: q.answer.face.clone(),
        feature: q.answer.feature.clone(),
        foot: q.answer.foot,
        distance: q.answer.distance,
    }
}
