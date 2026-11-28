//! ALN.BFIT.ICP.CORR acceptance criteria AC-1 to AC-4 plus REQ-003, REQ-005 and the refusals.
//!
//! Expected values come from outside this crate:
//!
//! - AC-1: `fixtures/aln/bfit/icp/cube_points.json`, written by the independent Python
//!   reference `make_corr_fixtures.py` next to it (exact rational feet, 40-digit decimal
//!   normals and distances, no libm), never from running this crate.
//! - AC-2: GEO.DIST.MESH's `fixtures/geo/dist/shared_vertex_ties/cases.txt` (its oracle
//!   `tools/fixtures/geo_dist.py` lists the lowest tied triangle for each query).
//! - The CAD cube, AC-3, the patches, the open sheet and the refusals are hand-labelled from
//!   geometry and the leaf's rules (a point 2 mm above a face is 2 mm from it; a foot on a
//!   free edge has no pseudo-normal).
//! - AC-4 compares the search with itself on 1, 2 and 8 threads, which is what it states.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::needless_range_loop)]

use core::sync::atomic::AtomicBool;

use fid_align_solve::{
    correspond, CadReference, CorrError, CorrOptions, CorrOutput, CorrReason, Correspondences, Reference,
};
use fid_geo_cad::{Body, BodyBuilder, CurveDef, CurveParams, Frame, Sense, SurfaceDef, SurfaceParams};
use fid_geo_dist::{BodyTarget, MeshTarget, SignTarget, SignUndefined};
use fid_geo_index::PrimitiveBvh;
use fid_geo_types::{FrameRef, Mesh, PointSet};

type V3 = [f64; 3];

const TOL: f64 = 1e-12;

// ------------------------------------------------------------------ a small JSON reader

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) {
        self.ws();
        assert_eq!(self.b[self.i], c, "expected {} at {}", c as char, self.i);
        self.i += 1;
    }

    fn value(&mut self) -> J {
        self.ws();
        match self.b[self.i] {
            b'{' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b[self.i] == b'}' {
                    self.i += 1;
                    return J::Obj(out);
                }
                loop {
                    self.ws();
                    let J::Str(k) = self.value() else { panic!("object key") };
                    assert!(out.iter().all(|(o, _)| *o != k), "duplicate key {k}");
                    self.eat(b':');
                    let v = self.value();
                    out.push((k, v));
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    match c {
                        b',' => continue,
                        b'}' => return J::Obj(out),
                        _ => panic!("bad object at {}", self.i),
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b[self.i] == b']' {
                    self.i += 1;
                    return J::Arr(out);
                }
                loop {
                    out.push(self.value());
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    match c {
                        b',' => continue,
                        b']' => return J::Arr(out),
                        _ => panic!("bad array at {}", self.i),
                    }
                }
            }
            b'"' => {
                self.i += 1;
                let start = self.i;
                while self.b[self.i] != b'"' {
                    assert_ne!(self.b[self.i], b'\\', "escapes are not used by the fixtures");
                    self.i += 1;
                }
                let s = std::str::from_utf8(&self.b[start..self.i]).unwrap().to_string();
                self.i += 1;
                J::Str(s)
            }
            b't' => {
                self.i += 4;
                J::Bool(true)
            }
            b'f' => {
                self.i += 5;
                J::Bool(false)
            }
            b'n' => {
                self.i += 4;
                J::Null
            }
            _ => {
                let start = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.i += 1;
                }
                J::Num(std::str::from_utf8(&self.b[start..self.i]).unwrap().to_string())
            }
        }
    }
}

impl J {
    fn at(&self, key: &str) -> &J {
        match self {
            J::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or_else(|| panic!("missing {key}")),
            _ => panic!("not an object"),
        }
    }

    fn arr(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            _ => panic!("not an array"),
        }
    }

    fn f(&self) -> f64 {
        match self {
            J::Num(s) => s.parse().unwrap(),
            _ => panic!("not a number: {self:?}"),
        }
    }

    fn u(&self) -> u64 {
        match self {
            J::Num(s) => s.parse().unwrap(),
            _ => panic!("not an integer"),
        }
    }

    fn s(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string"),
        }
    }

    fn f3(&self) -> V3 {
        let a = self.arr();
        assert_eq!(a.len(), 3);
        [a[0].f(), a[1].f(), a[2].f()]
    }
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(format!("{}/fixtures/{rel}", fid_testkit::WORKSPACE_ROOT)).unwrap()
}

fn load(rel: &str) -> J {
    let text = read(rel);
    let mut p = P { b: &text, i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, text.len(), "trailing bytes in {rel}");
    v
}

// ------------------------------------------------------------------ helpers

fn frame() -> FrameRef {
    FrameRef::from_bytes([7; 16])
}

fn mesh_of(verts: &[V3], tris: &[[u64; 3]]) -> (Mesh, PrimitiveBvh) {
    let mesh = Mesh::from_points(frame(), verts, tris).unwrap();
    let payload: Vec<u64> = (0..tris.len() as u64).collect();
    let bvh = PrimitiveBvh::build(&mesh, &payload, 1).unwrap();
    (mesh, bvh)
}

struct Cube {
    verts: Vec<V3>,
    tris: Vec<[u64; 3]>,
    cases: Vec<Case>,
}

struct Case {
    group: String,
    p: V3,
    foot: V3,
    normal: V3,
    dist: f64,
    entity: u64,
}

fn cube_fixture() -> Cube {
    let j = load("aln/bfit/icp/cube_points.json");
    let verts = j.at("vertices").arr().iter().map(J::f3).collect();
    let tris = j
        .at("triangles")
        .arr()
        .iter()
        .map(|t| {
            let a = t.arr();
            [a[0].u(), a[1].u(), a[2].u()]
        })
        .collect();
    let cases = j
        .at("cases")
        .arr()
        .iter()
        .map(|c| Case {
            group: c.at("group").s().to_string(),
            p: c.at("p").f3(),
            foot: c.at("foot").f3(),
            normal: c.at("normal").f3(),
            dist: c.at("dist").f(),
            entity: c.at("entity").u(),
        })
        .collect();
    Cube { verts, tris, cases }
}

fn points(ps: &[V3]) -> PointSet {
    PointSet::from_points(frame(), ps).unwrap()
}

fn run(ps: &[V3], r: Reference<'_, '_, '_>, opts: &CorrOptions<'_>) -> CorrOutput {
    correspond(&points(ps), r, opts, &AtomicBool::new(false)).unwrap()
}

fn close3(a: V3, b: V3, tol: f64) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() <= tol)
}

fn dot(a: V3, b: V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

/// Every field of every entry as bits, for byte identity.
fn encode(c: &Correspondences) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..c.foot.len() {
        for x in c.foot[i].iter().chain(c.normal[i].iter()).chain([c.dist[i]].iter()) {
            out.extend_from_slice(&x.to_bits().to_le_bytes());
        }
        out.extend_from_slice(&c.entity[i].to_le_bytes());
        out.push(u8::from(c.valid[i]));
        out.extend_from_slice(c.reason[i].code().as_bytes());
        if let Some(r) = c.reason[i].undefined_reason() {
            out.extend_from_slice(r.code().as_bytes());
        }
        out.push(b';');
    }
    out
}

fn check_consistent(out: &CorrOutput) {
    let c = out.correspondences();
    let s = out.summary();
    let n = c.foot.len();
    for v in [c.normal.len(), c.entity.len(), c.dist.len(), c.valid.len(), c.reason.len()] {
        assert_eq!(v, n);
    }
    assert_eq!(s.points, n as u64);
    assert_eq!(s.hits + s.normal_undefined + s.beyond_max_distance + s.no_hit, s.points);
    let hits = c.reason.iter().filter(|r| **r == CorrReason::Hit).count() as u64;
    assert_eq!(hits, s.hits);
    for i in 0..n {
        assert_eq!(c.valid[i], c.reason[i] == CorrReason::Hit, "point {i}");
        if c.valid[i] {
            assert!((dot(c.normal[i], c.normal[i]) - 1.0).abs() <= TOL, "point {i}: {:?}", c.normal[i]);
        }
    }
}

// ------------------------------------------------------------------ the CAD cube

/// The unit cube of `fixtures/geo/cad/topo/cube_labelled.json` (same vertices, line edges,
/// coedge senses, loops and plane faces), without labels.
fn cad_cube() -> Body {
    const EDGES: [(usize, usize, V3, V3, V3); 12] = [
        (0, 1, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (2, 3, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (4, 5, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (6, 7, [0.0, 1.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (0, 2, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        (1, 3, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        (4, 6, [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        (5, 7, [1.0, 0.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        (0, 4, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        (1, 5, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        (2, 6, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        (3, 7, [1.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    ];
    const COEDGES: [(usize, bool); 24] = [
        (8, true),
        (6, true),
        (10, false),
        (4, false),
        (5, true),
        (11, true),
        (7, false),
        (9, false),
        (0, true),
        (9, true),
        (2, false),
        (8, false),
        (10, true),
        (3, true),
        (11, false),
        (1, false),
        (4, true),
        (1, true),
        (5, false),
        (0, false),
        (2, true),
        (7, true),
        (3, false),
        (6, false),
    ];
    const FACES: [(V3, V3, V3); 6] = [
        ([0.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    ];
    let mut b = BodyBuilder::new([0x43; 16]);
    for v in 0..8u32 {
        b.add_vertex([f64::from(v & 1), f64::from((v >> 1) & 1), f64::from((v >> 2) & 1)], None);
    }
    for (s, e, origin, dir, perp) in EDGES {
        // A line runs along its frame's reference direction (GEO.NURBS.ANALYTIC).
        let c = CurveDef::analytic(CurveParams::Line { frame: Frame { origin, axis: perp, ref_dir: dir } }).unwrap();
        b.add_edge(Some(c), s, e, [0.0, 1.0], None);
    }
    for (e, same) in COEDGES {
        b.add_coedge(e, if same { Sense::Same } else { Sense::Reversed }, None, None);
    }
    let mut faces = Vec::new();
    for (k, (origin, axis, ref_dir)) in FACES.into_iter().enumerate() {
        let l = b.add_loop((4 * k..4 * k + 4).collect(), true, None);
        let s = SurfaceDef::analytic(SurfaceParams::Plane { frame: Frame { origin, axis, ref_dir } }).unwrap();
        faces.push(b.add_face(s, Sense::Same, vec![l], None));
    }
    b.add_shell(faces, true);
    b.build().unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_cube_points_mesh() {
    let cube = cube_fixture();
    assert_eq!(cube.cases.len(), 126);
    let (mesh, bvh) = mesh_of(&cube.verts, &cube.tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let ps: Vec<V3> = cube.cases.iter().map(|c| c.p).collect();
    let out = run(&ps, Reference::Mesh(&st), &CorrOptions::default());
    check_consistent(&out);
    assert_eq!(out.summary().hits, 126);
    assert!(out.notes().is_empty());
    let c = out.correspondences();
    for (i, e) in cube.cases.iter().enumerate() {
        assert_eq!(c.reason[i], CorrReason::Hit, "case {i} ({})", e.group);
        assert!(close3(c.foot[i], e.foot, TOL), "case {i} ({}): foot {:?} vs {:?}", e.group, c.foot[i], e.foot);
        assert!(
            close3(c.normal[i], e.normal, TOL),
            "case {i} ({}): normal {:?} vs {:?}",
            e.group,
            c.normal[i],
            e.normal
        );
        assert!((c.dist[i] - e.dist).abs() <= TOL, "case {i} ({}): dist {} vs {}", e.group, c.dist[i], e.dist);
        assert_eq!(c.entity[i], e.entity, "case {i} ({}): lowest containing triangle", e.group);
    }
}

/// REQ-005 for CAD faces: the outward surface normal of the hit face at the foot. On a face
/// (and the inside and diagonal groups) that is the fixture's normal; at a cube edge or corner
/// it is the outward axis of whichever incident face wins GEO's tie rule, one of the axes the
/// fixture's pseudo-normal combines (hand-labelled).
#[test]
fn ac1_cube_points_cad_surface_normal() {
    let cube = cube_fixture();
    let body = cad_cube();
    let bt = BodyTarget::new(&body, frame()).unwrap();
    let cad = CadReference::new(bt.set()).unwrap();
    let ps: Vec<V3> = cube.cases.iter().map(|c| c.p).collect();
    let out = run(&ps, Reference::Cad(&cad), &CorrOptions::default());
    check_consistent(&out);
    assert_eq!(out.summary().hits, 126);
    assert!(out.notes().is_empty());
    let c = out.correspondences();
    for (i, e) in cube.cases.iter().enumerate() {
        assert_eq!(c.reason[i], CorrReason::Hit, "case {i}");
        assert!(close3(c.foot[i], e.foot, 1e-9), "case {i} ({}): foot {:?} vs {:?}", e.group, c.foot[i], e.foot);
        assert!((c.dist[i] - e.dist).abs() <= 1e-9, "case {i} ({})", e.group);
        let n = c.normal[i];
        match e.group.as_str() {
            "face" | "inside" | "diagonal" => assert!(close3(n, e.normal, TOL), "case {i}: {n:?}"),
            _ => {
                let k = (0..3).find(|&k| n[k].abs() > 0.5).unwrap();
                let axis = [0, 1, 2].map(|m| if m == k { n[k].signum() } else { 0.0 });
                assert!(close3(n, axis, TOL), "case {i}: {n:?} is not an axis");
                assert!(e.normal[k] * n[k] > 0.0, "case {i}: {n:?} is not incident at {:?}", e.foot);
            }
        }
        let f = body.faces()[c.entity[i] as usize].surface().as_analytic();
        assert!(f.is_some(), "case {i}: entity {} is a face of the body", c.entity[i]);
    }
}

// ------------------------------------------------------------------ AC-2

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

#[test]
fn ac2_shared_vertex_ties_lowest_index() {
    let text = String::from_utf8(read("geo/dist/shared_vertex_ties/cases.txt")).unwrap();
    let (mut verts, mut tris, mut qs) = (Vec::new(), Vec::new(), Vec::new());
    for ln in text.lines() {
        let f: Vec<&str> = ln.split(' ').collect();
        match f[0] {
            "v" => verts.push([hexf(f[1]), hexf(f[2]), hexf(f[3])]),
            "t" => tris.push([f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap()]),
            "q" => qs.push(f.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
            _ => {}
        }
    }
    let (mesh, bvh) = mesh_of(&verts, &tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let ps: Vec<V3> = qs.iter().map(|f| [hexf(&f[1]), hexf(&f[2]), hexf(&f[3])]).collect();
    let out = run(&ps, Reference::Mesh(&st), &CorrOptions { threads: 4, ..CorrOptions::default() });
    check_consistent(&out);
    let c = out.correspondences();
    let mut ties = 0;
    for (i, f) in qs.iter().enumerate() {
        // A tie at a fan's boundary vertex has no pseudo-normal: the entity is still reported.
        assert!(
            matches!(c.reason[i], CorrReason::Hit | CorrReason::NormalUndefined(_)),
            "query {i}: {:?}",
            c.reason[i]
        );
        assert_eq!(c.entity[i].to_string(), f[4], "query {i}: lowest tied triangle");
        let foot = [hexf(&f[6]), hexf(&f[7]), hexf(&f[8])];
        assert_eq!(c.foot[i].map(f64::to_bits), foot.map(f64::to_bits), "query {i}");
        assert!((c.dist[i] * c.dist[i] - hexf(&f[9])).abs() <= 1e-12, "query {i}");
        if f[10] != "1" {
            ties += 1;
        }
    }
    assert!(ties >= 20, "{ties}");
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_beyond_max_distance() {
    let cube = cube_fixture();
    let (mesh, bvh) = mesh_of(&cube.verts, &cube.tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let body = cad_cube();
    let bt = BodyTarget::new(&body, frame()).unwrap();
    let cad = CadReference::new(bt.set()).unwrap();
    let ps = [[0.5, 0.5, 1.002], [0.5, 0.5, 1.0005], [0.5, 0.5, 0.9995]];
    let opts = CorrOptions { max_distance: Some(0.001), ..CorrOptions::default() };
    for r in [Reference::Mesh(&st), Reference::Cad(&cad)] {
        let out = run(&ps, r, &opts);
        check_consistent(&out);
        let c = out.correspondences();
        assert_eq!(c.reason[0], CorrReason::BeyondMaxDistance);
        assert_eq!(c.reason[0].code(), "beyond_max_distance");
        assert!(!c.valid[0]);
        assert_eq!((c.foot[0], c.normal[0], c.dist[0], c.entity[0]), ([0.0; 3], [0.0; 3], 0.0, 0));
        for i in [1, 2] {
            assert_eq!(c.reason[i], CorrReason::Hit);
            assert!((c.dist[i] - 0.0005).abs() <= TOL);
            assert!(close3(c.normal[i], [0.0, 0.0, 1.0], TOL));
            assert!(close3(c.foot[i], [0.5, 0.5, 1.0], TOL));
        }
        let s = out.summary();
        assert_eq!((s.points, s.hits, s.beyond_max_distance, s.normal_undefined, s.no_hit), (3, 2, 1, 0, 0));
    }
}

// ------------------------------------------------------------------ AC-4

struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [lo, hi) on a 2^-53 grid.
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * ((self.next() >> 11) as f64 / (1u64 << 53) as f64)
    }
}

fn random_points(n: usize, seed: u64) -> Vec<V3> {
    let mut r = SplitMix(seed);
    (0..n).map(|_| [r.uniform(-0.5, 1.5), r.uniform(-0.5, 1.5), r.uniform(-0.5, 1.5)]).collect()
}

#[test]
fn ac4_thread_count_does_not_change_bytes() {
    let cube = cube_fixture();
    let (mesh, bvh) = mesh_of(&cube.verts, &cube.tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let ps = random_points(100_000, 3);
    for max_distance in [None, Some(0.25)] {
        let mut first: Option<Vec<u8>> = None;
        for threads in [1, 2, 8] {
            let out = run(&ps, Reference::Mesh(&st), &CorrOptions { max_distance, threads, ..CorrOptions::default() });
            check_consistent(&out);
            if max_distance.is_some() {
                assert!(out.summary().beyond_max_distance > 0 && out.summary().hits > 0);
            }
            let bytes = encode(out.correspondences());
            match &first {
                None => first = Some(bytes),
                Some(b) => assert!(b == &bytes, "{threads} threads differ from 1 thread"),
            }
        }
    }
}

#[test]
fn ac4_cad_thread_count_does_not_change_bytes() {
    let body = cad_cube();
    let bt = BodyTarget::new(&body, frame()).unwrap();
    let cad = CadReference::new(bt.set()).unwrap();
    let ps = random_points(3_000, 3);
    let mut first: Option<Vec<u8>> = None;
    for threads in [1, 2, 8] {
        let out = run(&ps, Reference::Cad(&cad), &CorrOptions { threads, ..CorrOptions::default() });
        check_consistent(&out);
        let bytes = encode(out.correspondences());
        match &first {
            None => first = Some(bytes),
            Some(b) => assert!(b == &bytes, "{threads} threads differ from 1 thread"),
        }
    }
}

// ------------------------------------------------------------------ REQ-003 patches

#[test]
fn req003_patches_restrict_the_search() {
    let cube = cube_fixture();
    let (mesh, bvh) = mesh_of(&cube.verts, &cube.tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    // Triangles 10 and 11 are the top face z = 1 (the fixture's last face).
    for t in [10usize, 11] {
        assert!(cube.tris[t].iter().all(|&v| cube.verts[v as usize][2] == 1.0));
    }
    let top = [10u64, 11];
    let ps = [[0.5, 0.25, -0.1]];
    let out = run(&ps, Reference::Mesh(&st), &CorrOptions { patches: Some(&top), ..CorrOptions::default() });
    let c = out.correspondences();
    assert_eq!(c.reason[0], CorrReason::Hit);
    assert!(close3(c.foot[0], [0.5, 0.25, 1.0], TOL));
    assert!((c.dist[0] - 1.1).abs() <= TOL);
    assert!(close3(c.normal[0], [0.0, 0.0, 1.0], TOL));
    assert!(top.contains(&c.entity[0]));
    // Without patches the bottom face is nearest.
    let all = run(&ps, Reference::Mesh(&st), &CorrOptions::default());
    assert!(close3(all.correspondences().foot[0], [0.5, 0.25, 0.0], TOL));
    assert!(close3(all.correspondences().normal[0], [0.0, 0.0, -1.0], TOL));
}

#[test]
fn req003_cad_patches_are_face_indexes() {
    let body = cad_cube();
    let bt = BodyTarget::new(&body, frame()).unwrap();
    let cad = CadReference::new(bt.set()).unwrap();
    // The face whose plane is z = 1, found by geometry: the face a point above it hits.
    let top = run(&[[0.5, 0.5, 1.5]], Reference::Cad(&cad), &CorrOptions::default()).correspondences().entity[0];
    let ps = [[0.5, 0.25, -0.1]];
    let out = run(&ps, Reference::Cad(&cad), &CorrOptions { patches: Some(&[top]), ..CorrOptions::default() });
    let c = out.correspondences();
    assert_eq!((c.reason[0], c.entity[0]), (CorrReason::Hit, top));
    assert!(close3(c.foot[0], [0.5, 0.25, 1.0], 1e-9));
    assert!(close3(c.normal[0], [0.0, 0.0, 1.0], TOL));
}

// ------------------------------------------------------------------ REQ-005 undefined normals

#[test]
fn open_sheet_boundary_has_no_normal() {
    // The unit square z = 0 as two triangles, normal +z; its outer edges are free.
    let verts = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
    let tris = [[0, 1, 2], [0, 2, 3]];
    let (mesh, bvh) = mesh_of(&verts, &tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let ps = [[0.25, 0.75, 0.5], [0.5, -0.25, 0.5], [1.5, 1.5, 0.0], [0.5, 0.5, -0.5]];
    let out = run(&ps, Reference::Mesh(&st), &CorrOptions::default());
    check_consistent(&out);
    let c = out.correspondences();
    assert_eq!(c.reason[0], CorrReason::Hit);
    assert!(close3(c.normal[0], [0.0, 0.0, 1.0], TOL));
    // A foot on the free edge y = 0, and at the free corner (1, 1).
    for (i, foot, dist) in [(1, [0.5, 0.0, 0.0], 0.3125_f64.sqrt()), (2, [1.0, 1.0, 0.0], 0.5_f64.sqrt())] {
        assert_eq!(c.reason[i], CorrReason::NormalUndefined(SignUndefined::Boundary), "point {i}");
        assert_eq!(c.reason[i].code(), "normal_undefined");
        assert!(!c.valid[i]);
        assert!(close3(c.foot[i], foot, TOL), "point {i}: the foot is still reported");
        assert!((c.dist[i] - dist).abs() <= TOL);
        assert_eq!(c.normal[i], [0.0; 3]);
    }
    // Below the sheet: the face normal still points +z (the sheet's orientation).
    assert_eq!(c.reason[3], CorrReason::Hit);
    assert!(close3(c.normal[3], [0.0, 0.0, 1.0], TOL));
    let s = out.summary();
    assert_eq!((s.hits, s.normal_undefined), (2, 2));
}

// ------------------------------------------------------------------ refusals

#[test]
fn refusals_fail_closed() {
    let cube = cube_fixture();
    let (mesh, bvh) = mesh_of(&cube.verts, &cube.tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let r = Reference::Mesh(&st);
    let ok = points(&[[0.5, 0.5, 2.0]]);
    let go = AtomicBool::new(false);
    let call = |ps: &PointSet, o: CorrOptions<'_>| correspond(ps, r, &o, &go).unwrap_err();
    let d = CorrOptions::default();

    let other = PointSet::from_points(FrameRef::from_bytes([8; 16]), &[[0.5, 0.5, 2.0]]).unwrap();
    assert!(matches!(call(&other, d), CorrError::FrameMismatch(_)));
    assert_eq!(call(&ok, CorrOptions { threads: 0, ..d }).code(), "invalid_threads");
    assert_eq!(call(&ok, CorrOptions { threads: 257, ..d }).code(), "invalid_threads");
    for m in [f64::NAN, f64::INFINITY, -1.0, -f64::MIN_POSITIVE] {
        assert_eq!(call(&ok, CorrOptions { max_distance: Some(m), ..d }), CorrError::InvalidMaxDistance, "{m}");
    }
    assert_eq!(call(&ok, CorrOptions { patches: Some(&[]), ..d }), CorrError::EmptyPatches);
    assert_eq!(call(&ok, CorrOptions { patches: Some(&[3, 2]), ..d }), CorrError::InvalidPatches { position: 1 });
    assert_eq!(call(&ok, CorrOptions { patches: Some(&[2, 2]), ..d }), CorrError::InvalidPatches { position: 1 });
    assert_eq!(call(&ok, CorrOptions { patches: Some(&[12]), ..d }), CorrError::InvalidPatches { position: 0 });
    assert_eq!(call(&points(&[]), d), CorrError::NoPoints);
    let far = points(&[[0.0, 0.0, 0.0], [0.0, f64::from_bits((1023 + 251) << 52), 0.0]]);
    assert_eq!(call(&far, d), CorrError::QueryOutOfRange { index: 1 });
    // Cancelled before the first block: no results.
    let stop = AtomicBool::new(true);
    assert_eq!(correspond(&ok, r, &d, &stop).unwrap_err(), CorrError::Cancelled);
    // Zero distance is a valid limit.
    let on = run(&[[0.5, 0.5, 1.0]], r, &CorrOptions { max_distance: Some(0.0), ..d });
    assert_eq!(on.correspondences().reason[0], CorrReason::Hit);
}
