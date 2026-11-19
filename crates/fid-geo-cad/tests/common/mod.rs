//! Shared by the fid-geo-cad integration tests: a minimal JSON reader for the fixture
//! scripts under `fixtures/geo/cad/topo/` and the script-to-`BodyBuilder` translation.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_geo_cad::{Body, BodyBuilder, Curve2, CurveDef, CurveParams, Frame, Sense, SurfaceDef, SurfaceParams};
use fid_geo_nurbs::{NurbsCurve, NurbsCurveParts};

pub const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/cad/topo/");

// ---------------------------------------------------------------- minimal JSON reader

#[derive(Debug, Clone)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(BTreeMap<String, J>),
}

pub struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    pub fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    pub fn string(&mut self) -> String {
        assert_eq!(self.s[self.i], b'"');
        self.i += 1;
        let mut raw = Vec::new();
        while self.s[self.i] != b'"' {
            if self.s[self.i] == b'\\' {
                // Only `\"` and `\\` occur in the fixtures (quoted labels in messages).
                self.i += 1;
                assert!(matches!(self.s[self.i], b'"' | b'\\'), "unsupported escape");
            }
            raw.push(self.s[self.i]);
            self.i += 1;
        }
        self.i += 1;
        String::from_utf8(raw).unwrap()
    }
    pub fn value(&mut self) -> J {
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
    pub fn get(&self, k: &str) -> &J {
        self.opt(k).unwrap_or_else(|| panic!("missing key {k}"))
    }
    pub fn opt(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(m) => m.get(k),
            _ => panic!("not an object"),
        }
    }
    pub fn arr(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            _ => panic!("not an array"),
        }
    }
    pub fn obj(&self) -> &BTreeMap<String, J> {
        match self {
            J::Obj(m) => m,
            _ => panic!("not an object"),
        }
    }
    pub fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string"),
        }
    }
    pub fn f(&self) -> f64 {
        match self {
            J::Num(x) => *x,
            _ => panic!("not a number"),
        }
    }
    pub fn u(&self) -> usize {
        let x = self.f();
        assert!(x >= 0.0 && x.fract() == 0.0);
        x as usize
    }
    pub fn bool(&self) -> bool {
        match self {
            J::Bool(b) => *b,
            _ => panic!("not a bool"),
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, J::Null)
    }
    pub fn fs(&self) -> Vec<f64> {
        self.arr().iter().map(J::f).collect()
    }
    pub fn v3(&self) -> [f64; 3] {
        let v = self.fs();
        [v[0], v[1], v[2]]
    }
    pub fn v2(&self) -> [f64; 2] {
        let v = self.fs();
        assert_eq!(v.len(), 2);
        [v[0], v[1]]
    }
    pub fn label(&self) -> Option<&str> {
        self.opt("label").map(J::str)
    }
}

pub fn load(rel: &str) -> J {
    let text = std::fs::read(format!("{DIR}{rel}")).unwrap();
    let mut p = Parser { s: &text, i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, text.len(), "trailing bytes in {rel}");
    v
}

pub fn expected() -> J {
    load("expected.json")
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

// ---------------------------------------------------------------- script -> builder

pub fn frame(g: &J) -> Frame {
    Frame { origin: g.get("origin").v3(), axis: g.get("axis").v3(), ref_dir: g.get("ref_dir").v3() }
}

pub fn nurbs_curve(g: &J) -> NurbsCurve {
    let ctrl = g
        .get("ctrl")
        .arr()
        .iter()
        .map(|p| {
            let v = p.fs();
            [v[0], v[1], if v.len() == 3 { v[2] } else { 0.0 }]
        })
        .collect();
    let w = g.get("weights");
    NurbsCurve::new(NurbsCurveParts {
        degree: g.get("degree").u(),
        knots: g.get("knots").fs(),
        ctrl,
        weights: if w.is_null() { None } else { Some(w.fs()) },
        periodic: g.get("periodic").bool(),
    })
    .unwrap()
}

pub fn curve(g: &J) -> Option<CurveDef> {
    if g.is_null() {
        return None;
    }
    Some(match g.get("type").str() {
        "line" => CurveDef::analytic(CurveParams::Line { frame: frame(g) }).unwrap(),
        "circle" => CurveDef::analytic(CurveParams::Circle { frame: frame(g), radius: g.get("radius").f() }).unwrap(),
        "ellipse" => CurveDef::analytic(CurveParams::Ellipse {
            frame: frame(g),
            semi_axis_1: g.get("semi_axis_1").f(),
            semi_axis_2: g.get("semi_axis_2").f(),
        })
        .unwrap(),
        "nurbs" => CurveDef::nurbs(nurbs_curve(g)).unwrap(),
        t => panic!("curve type {t}"),
    })
}

pub fn pcurve(g: &J) -> Option<Curve2> {
    if g.is_null() {
        return None;
    }
    Some(match g.get("type").str() {
        "line2" => Curve2::line(g.get("origin").v2(), g.get("dir").v2()).unwrap(),
        "nurbs" => Curve2::nurbs(nurbs_curve(g)).unwrap(),
        t => panic!("pcurve type {t}"),
    })
}

pub fn surface(g: &J) -> SurfaceDef {
    let fr = frame(g);
    let p = match g.get("type").str() {
        "plane" => SurfaceParams::Plane { frame: fr },
        "cylinder" => SurfaceParams::Cylinder { frame: fr, radius: g.get("radius").f() },
        "cone" => SurfaceParams::Cone { frame: fr, radius: g.get("radius").f(), half_angle: g.get("half_angle").f() },
        "sphere" => SurfaceParams::Sphere { frame: fr, radius: g.get("radius").f() },
        "torus" => SurfaceParams::Torus {
            frame: fr,
            major_radius: g.get("major_radius").f(),
            minor_radius: g.get("minor_radius").f(),
        },
        t => panic!("surface type {t}"),
    };
    SurfaceDef::analytic(p).unwrap()
}

pub fn sense(j: &J) -> Sense {
    match j.str() {
        "same" => Sense::Same,
        "reversed" => Sense::Reversed,
        s => panic!("sense {s}"),
    }
}

pub fn builder(script: &J) -> BodyBuilder {
    let id: [u8; 16] = unhex(script.get("body_id").str()).try_into().unwrap();
    let mut b = BodyBuilder::new(id);
    for v in script.get("vertices").arr() {
        b.add_vertex(v.get("point").v3(), v.label());
    }
    for e in script.get("edges").arr() {
        let r = e.get("range").v2();
        b.add_edge(curve(e.get("curve")), e.get("start").u(), e.get("end").u(), r, e.label());
    }
    for c in script.get("coedges").arr() {
        b.add_coedge(c.get("edge").u(), sense(c.get("sense")), pcurve(c.get("pcurve")), c.label());
    }
    for l in script.get("loops").arr() {
        let cs = l.get("coedges").arr().iter().map(J::u).collect();
        b.add_loop(cs, l.get("outer").bool(), l.label());
    }
    for f in script.get("faces").arr() {
        let ls = f.get("loops").arr().iter().map(J::u).collect();
        b.add_face(surface(f.get("surface")), sense(f.get("sense")), ls, f.label());
    }
    for s in script.get("shells").arr() {
        b.add_shell(s.get("faces").arr().iter().map(J::u).collect(), s.get("closed").bool());
    }
    b
}

pub fn build(name: &str) -> Body {
    builder(&load(&format!("{name}.json"))).build().unwrap()
}
