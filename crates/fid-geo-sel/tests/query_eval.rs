//! Acceptance tests AC-1..AC-6 for `GEO.SEL.QUERY.EVAL` (`fid_geo_sel::query::eval`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_sel::model::{Domain, EntityId, IndexSet, SelectionBody};
use fid_geo_sel::query::{
    evaluate, evaluate_selection, parse_query, CadEvaluator, CadPred, EvalContext, EvalError, Expr, NearEvaluator,
    NearPred,
};
use fid_geo_types::{AttributeSet, FrameRef, GeometryElement, Mesh, PointBuffer, PointSet};
use std::f64::consts::PI;

const EVAL_CASES_JSON: &str = include_str!("../../../fixtures/geo/sel/query/eval_cases.json");

struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn next_f64(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) * (1.0 / ((1u64 << 53) as f64))
    }
}

/// Deterministic 10^5-point cloud with intensity, scanner_id, timestamp_ns, quality and unit normals (seed 41, AC-1).
fn build_cloud_attrs_1e5() -> PointSet {
    let n = 100_000usize;
    let mut rng = SplitMix64(41);
    let mut x = Vec::with_capacity(n);
    let mut y = Vec::with_capacity(n);
    let mut z = Vec::with_capacity(n);
    let mut nx = Vec::with_capacity(n);
    let mut ny = Vec::with_capacity(n);
    let mut nz = Vec::with_capacity(n);
    let mut intensity = Vec::with_capacity(n);
    let mut scanner_id = Vec::with_capacity(n);
    let mut timestamp_ns = Vec::with_capacity(n);
    let mut quality = Vec::with_capacity(n);

    for i in 0..n {
        x.push(rng.next_f64() * 2.0 - 1.0);
        y.push(rng.next_f64() * 2.0 - 1.0);
        z.push(rng.next_f64() * 2.0 - 1.0);
        let vx = rng.next_f64() * 2.0 - 1.0;
        let vy = rng.next_f64() * 2.0 - 1.0;
        let vz = rng.next_f64() * 2.0 - 1.0;
        let len = (vx * vx + vy * vy + vz * vz).sqrt().max(1e-9);
        nx.push(vx / len);
        ny.push(vy / len);
        nz.push(vz / len);
        intensity.push(rng.next_f64() as f32);
        scanner_id.push((rng.next_u64() % 4) as u32);
        timestamp_ns.push((i as i64) * 1_000);
        quality.push((rng.next_u64() & 0x0f) as u16);
    }

    let attrs = AttributeSet {
        normal: Some([nx, ny, nz]),
        intensity: Some(intensity),
        scanner_id: Some(scanner_id),
        timestamp_ns: Some(timestamp_ns),
        quality: Some(quality),
        ..AttributeSet::default()
    };
    PointSet::new(FrameRef::from_bytes([0; 16]), PointBuffer::new(x, y, z), attrs).unwrap()
}

/// Deterministic UV-sphere triangle mesh (outward normals, ~20k triangles for fast test runtime while exercising AC-2).
fn build_sphere_mesh(lat_steps: usize, lon_steps: usize) -> Mesh {
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut tris: Vec<[u64; 3]> = Vec::new();

    verts.push([0.0, 0.0, 1.0]); // north pole = 0
    for r in 1..lat_steps {
        let theta = PI * (r as f64) / (lat_steps as f64);
        let st = theta.sin();
        let ct = theta.cos();
        for c in 0..lon_steps {
            let phi = 2.0 * PI * (c as f64) / (lon_steps as f64);
            verts.push([st * phi.cos(), st * phi.sin(), ct]);
        }
    }
    let south_idx = verts.len() as u64;
    verts.push([0.0, 0.0, -1.0]);

    for c in 0..lon_steps {
        let c1 = (c + 1) % lon_steps;
        tris.push([0, 1 + c as u64, 1 + c1 as u64]);
    }
    for r in 0..(lat_steps - 2) {
        let row0 = 1 + r * lon_steps;
        let row1 = 1 + (r + 1) * lon_steps;
        for c in 0..lon_steps {
            let c1 = (c + 1) % lon_steps;
            let a = (row0 + c) as u64;
            let b = (row1 + c) as u64;
            let d = (row1 + c1) as u64;
            let e = (row0 + c1) as u64;
            tris.push([a, b, d]);
            tris.push([a, d, e]);
        }
    }
    let last_row = 1 + (lat_steps - 2) * lon_steps;
    for c in 0..lon_steps {
        let c1 = (c + 1) % lon_steps;
        tris.push([(last_row + c) as u64, south_idx, (last_row + c1) as u64]);
    }

    Mesh::from_points(FrameRef::from_bytes([0; 16]), &verts, &tris).unwrap()
}

/// AC-1 & AC-6: 10^5 points (seed 41), every query in `eval_cases.json` matches an independent
/// brute-force reference and produces deterministic digests.
#[test]
fn ac1_ac6_point_queries_1e5_match_brute_force_and_stable_digests() {
    assert!(EVAL_CASES_JSON.contains(r#""seed": 41"#));
    let ps = build_cloud_attrs_1e5();
    let elem = GeometryElement::PointSet(ps.clone());
    let ctx = EvalContext::new(&elem);
    let n = ps.len() as usize;

    // 1. all
    let q_all = parse_query(br#"{"domain":"points","v":1,"where":{"all":{}}}"#).unwrap();
    let r_all = evaluate(&q_all, &ctx).unwrap();
    assert_eq!(r_all.len(), 100_000);

    // 2. ids
    let q_ids = parse_query(br#"{"domain":"points","v":1,"where":{"ids":{"ids":[0,42,99999,12345]}}}"#).unwrap();
    let r_ids = evaluate(&q_ids, &ctx).unwrap();
    assert_eq!(r_ids.ranges(), &[(0, 1), (42, 43), (12345, 12346), (99999, 100000)]);

    // 3. intensity & scanner_id
    let q_attr = parse_query(
        br#"{"domain":"points","v":1,"where":{"and":[{"attr_range":{"channel":"intensity","min":0.25,"max":0.75}},{"attr_range":{"channel":"scanner_id","min":1,"max":2}}]}}"#,
    )
    .unwrap();
    let r_attr = evaluate(&q_attr, &ctx).unwrap();
    let bf_attr: Vec<u64> = (0..n)
        .filter(|&i| {
            let inten = f64::from(ps.attributes().intensity.as_ref().unwrap()[i]);
            let sid = f64::from(ps.attributes().scanner_id.as_ref().unwrap()[i]);
            (0.25..=0.75).contains(&inten) && (1.0..=2.0).contains(&sid)
        })
        .map(|i| i as u64)
        .collect();
    assert_eq!(r_attr, IndexSet::from_indices(Domain::Points, &bf_attr, ps.len()).unwrap());

    // 4. quality_flags & normal_cone
    let q_qn = parse_query(
        br#"{"domain":"points","v":1,"where":{"and":[{"quality_flags":{"all_of":1,"none_of":4}},{"normal_cone":{"axis":[0,0,1],"half_angle":{"text":"30","unit":"deg"}}}]}}"#,
    )
    .unwrap();
    let r_qn = evaluate(&q_qn, &ctx).unwrap();
    let cos30 = fid_num::f::cos(30.0_f64.to_radians()).unwrap();
    let bf_qn: Vec<u64> = (0..n)
        .filter(|&i| {
            let q = ps.attributes().quality.as_ref().unwrap()[i];
            let nz = ps.attributes().normal.as_ref().unwrap()[2][i];
            (q & 1) == 1 && (q & 4) == 0 && nz >= cos30
        })
        .map(|i| i as u64)
        .collect();
    assert_eq!(r_qn, IndexSet::from_indices(Domain::Points, &bf_qn, ps.len()).unwrap());

    // 5. (in_box OR in_sphere) AND NOT (timestamp_ns >= 50_000_000)
    let q_comb = parse_query(
        br#"{"domain":"points","v":1,"where":{"and":[{"or":[{"in_box":{"min":[{"text":"-0.2","unit":"m"},{"text":"-0.2","unit":"m"},{"text":"0","unit":"m"}],"max":[{"text":"0.2","unit":"m"},{"text":"0.2","unit":"m"},{"text":"0.5","unit":"m"}]}},{"in_sphere":{"centre":[{"text":"0.4","unit":"m"},{"text":"0.4","unit":"m"},{"text":"0.4","unit":"m"}],"radius":{"text":"0.25","unit":"m"}}}]},{"not":{"attr_range":{"channel":"timestamp_ns","min":50000000}}}]}}"#,
    )
    .unwrap();
    let r_comb = evaluate(&q_comb, &ctx).unwrap();
    let bf_comb: Vec<u64> = (0..n)
        .filter(|&i| {
            let p = ps.point(i as u64).unwrap();
            let in_box = (-0.2..=0.2).contains(&p[0]) && (-0.2..=0.2).contains(&p[1]) && (0.0..=0.5).contains(&p[2]);
            let dx = p[0] - 0.4;
            let dy = p[1] - 0.4;
            let dz = p[2] - 0.4;
            let in_sph = dx * dx + dy * dy + dz * dz <= 0.25 * 0.25;
            let ts = ps.attributes().timestamp_ns.as_ref().unwrap()[i];
            (in_box || in_sph) && ts < 50_000_000
        })
        .map(|i| i as u64)
        .collect();
    assert_eq!(r_comb, IndexSet::from_indices(Domain::Points, &bf_comb, ps.len()).unwrap());

    // AC-6 / REQ-001: evaluate_selection binds IndexSet to element digest and is repeatable.
    let target = EntityId::from_bytes([1; 16]);
    let sel1 = evaluate_selection(&q_comb, target, &ctx).unwrap();
    let sel2 = evaluate_selection(&q_comb, target, &ctx).unwrap();
    assert_eq!(sel1, sel2);
    let SelectionBody::IndexSet(s1) = sel1.body() else { panic!() };
    assert_eq!(s1.digest(sel1.target_digest()), r_comb.digest(sel1.target_digest()));
}

/// AC-2: Triangle predicates (`normal_cone`, `in_box`, `in_sphere`) on a sphere mesh match
/// independent brute force over triangle centroids and normals.
#[test]
fn ac2_triangle_queries_match_brute_force() {
    let mesh = build_sphere_mesh(50, 100);
    let elem = GeometryElement::Mesh(mesh.clone());
    let ctx = EvalContext::new(&elem);

    let q_cap = parse_query(
        br#"{"domain":"triangles","v":1,"where":{"and":[{"normal_cone":{"axis":[0,0,1],"half_angle":{"text":"45","unit":"deg"}}},{"in_box":{"min":[{"text":"-0.8","unit":"m"},{"text":"-0.8","unit":"m"},{"text":"0.5","unit":"m"}],"max":[{"text":"0.8","unit":"m"},{"text":"0.8","unit":"m"},{"text":"1.1","unit":"m"}]}}]}}"#,
    )
    .unwrap();
    let r_cap = evaluate(&q_cap, &ctx).unwrap();
    let cos45 = fid_num::f::cos(45.0_f64.to_radians()).unwrap();
    let bf_cap: Vec<u64> = mesh
        .triangles()
        .enumerate()
        .filter_map(|(ti, [i0, i1, i2])| {
            let v0 = mesh.vertex(i0).unwrap();
            let v1 = mesh.vertex(i1).unwrap();
            let v2 = mesh.vertex(i2).unwrap();
            let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
            let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
            let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let nz = n[2] / len;
            let c = [(v0[0] + v1[0] + v2[0]) / 3.0, (v0[1] + v1[1] + v2[1]) / 3.0, (v0[2] + v1[2] + v2[2]) / 3.0];
            if nz >= cos45
                && (-0.8..=0.8).contains(&c[0])
                && (-0.8..=0.8).contains(&c[1])
                && (0.5..=1.1).contains(&c[2])
            {
                Some(ti as u64)
            } else {
                None
            }
        })
        .collect();
    assert!(!bf_cap.is_empty());
    assert_eq!(r_cap, IndexSet::from_indices(Domain::Triangles, &bf_cap, mesh.triangle_count()).unwrap());
}

/// AC-3 & AC-4: Missing attribute channel -> `unresolved_reference`; out-of-range id ->
/// `unresolved_reference`; `in_group` / `near` without evaluator -> `unsupported_predicate`;
/// `attr_range` on triangles -> `domain_mismatch`; pluggable `NearEvaluator` & `CadEvaluator` dispatch.
#[test]
fn ac3_ac4_error_paths_and_pluggable_evaluators() {
    let bare = PointSet::from_points(FrameRef::from_bytes([0; 16]), &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]).unwrap();
    let elem = GeometryElement::PointSet(bare);
    let ctx = EvalContext::new(&elem);

    // AC-3: missing intensity -> unresolved_reference { channel: Some("intensity") }
    let q_inten =
        parse_query(br#"{"domain":"points","v":1,"where":{"attr_range":{"channel":"intensity","min":0.1}}}"#).unwrap();
    let err = evaluate(&q_inten, &ctx).unwrap_err();
    assert_eq!(err.code(), "unresolved_reference");
    assert!(matches!(err, EvalError::UnresolvedReference { channel: Some("intensity"), .. }));

    // Out-of-range index -> unresolved_reference { index: Some(9) }
    let q_oob = parse_query(br#"{"domain":"points","v":1,"where":{"ids":{"ids":[9]}}}"#).unwrap();
    let err_oob = evaluate(&q_oob, &ctx).unwrap_err();
    assert_eq!(err_oob.code(), "unresolved_reference");
    assert!(matches!(err_oob, EvalError::UnresolvedReference { index: Some(9), .. }));

    // AC-4: in_group without CadEvaluator -> unsupported_predicate
    let q_grp = parse_query(
        br#"{"domain":"points","v":1,"where":{"in_group":{"group":"00000000-0000-7000-8000-000000000001"}}}"#,
    )
    .unwrap();
    assert_eq!(evaluate(&q_grp, &ctx).unwrap_err(), EvalError::UnsupportedPredicate { predicate: "in_group" });

    // AC-4: attr_range on triangles -> domain_mismatch
    let mesh = build_sphere_mesh(4, 6);
    let mesh_elem = GeometryElement::Mesh(mesh);
    let mesh_ctx = EvalContext::new(&mesh_elem);
    let q_tri_attr =
        parse_query(br#"{"domain":"triangles","v":1,"where":{"attr_range":{"channel":"intensity","min":0}}}"#).unwrap();
    assert_eq!(
        evaluate(&q_tri_attr, &mesh_ctx).unwrap_err(),
        EvalError::DomainMismatch { predicate: "attr_range", domain: Domain::Triangles }
    );

    // Pluggable NearEvaluator and CadEvaluator (with seed callback on tangent_grow)
    struct MockNear;
    impl NearEvaluator for MockNear {
        fn near(&self, _e: &GeometryElement, domain: Domain, _p: &NearPred<'_>) -> Result<IndexSet, EvalError> {
            Ok(IndexSet::from_indices(domain, &[1], 2)?)
        }
    }
    struct MockCad;
    impl CadEvaluator for MockCad {
        fn eval(
            &self,
            _e: &GeometryElement,
            _d: Domain,
            pred: &CadPred<'_>,
            eval_seed: &dyn Fn(&Expr) -> Result<IndexSet, EvalError>,
        ) -> Result<IndexSet, EvalError> {
            match pred {
                CadPred::TangentGrow { seed, .. } => eval_seed(seed),
                _ => Ok(IndexSet::empty(Domain::Points)),
            }
        }
    }
    let plugged = EvalContext { element: &elem, near: Some(&MockNear), cad: Some(&MockCad) };
    let q_near = parse_query(
        br#"{"domain":"points","v":1,"where":{"near":{"distance":{"text":"1","unit":"mm"},"of":{"element":"00000000-0000-7000-8000-000000000001"}}}}"#,
    )
    .unwrap();
    assert_eq!(evaluate(&q_near, &plugged).unwrap().ranges(), &[(1, 2)]);

    let q_tg = parse_query(
        br#"{"domain":"points","v":1,"where":{"tangent_grow":{"angle":{"text":"5","unit":"deg"},"seed":{"ids":{"ids":[0]}}}}}"#,
    )
    .unwrap();
    assert_eq!(evaluate(&q_tg, &plugged).unwrap().ranges(), &[(0, 1)]);
}

/// AC-5: Point exactly on a box face, point exactly on a sphere boundary, and normal exactly at
/// the half-angle are all included.
#[test]
fn ac5_inclusive_box_sphere_and_normal_cone_boundaries() {
    let half_angle = PI / 3.0; // 60 deg -> cos = 0.5 exact in IEEE-754!
    let sin_ha = fid_num::f::sin(half_angle).unwrap();
    let cos_ha = fid_num::f::cos(half_angle).unwrap();
    let attrs = AttributeSet {
        normal: Some([vec![sin_ha, 1.0], vec![0.0, 0.0], vec![cos_ha, 0.0]]),
        ..AttributeSet::default()
    };
    let ps = PointSet::new(
        FrameRef::from_bytes([0; 16]),
        PointBuffer::from_points(&[[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]),
        attrs,
    )
    .unwrap();
    let elem = GeometryElement::PointSet(ps);
    let ctx = EvalContext::new(&elem);

    // Point 0 is at x=1.0, exactly on max face of box [0,1]x[0,1]x[0,1]
    let q_box = parse_query(
        br#"{"domain":"points","v":1,"where":{"in_box":{"min":[{"text":"0","unit":"m"},{"text":"0","unit":"m"},{"text":"0","unit":"m"}],"max":[{"text":"1","unit":"m"},{"text":"1","unit":"m"},{"text":"1","unit":"m"}]}}}"#,
    )
    .unwrap();
    assert_eq!(evaluate(&q_box, &ctx).unwrap().ranges(), &[(0, 1)]);

    // Point 0 is at distance 1.0, exactly on sphere radius 1.0 around origin
    let q_sph = parse_query(
        br#"{"domain":"points","v":1,"where":{"in_sphere":{"centre":[{"text":"0","unit":"m"},{"text":"0","unit":"m"},{"text":"0","unit":"m"}],"radius":{"text":"1","unit":"m"}}}}"#,
    )
    .unwrap();
    assert_eq!(evaluate(&q_sph, &ctx).unwrap().ranges(), &[(0, 1)]);

    // Point 0 has normal at exact 60 deg half-angle from +Z
    let q_cone = parse_query(
        br#"{"domain":"points","v":1,"where":{"normal_cone":{"axis":[0,0,1],"half_angle":{"text":"60","unit":"deg"}}}}"#,
    )
    .unwrap();
    assert_eq!(evaluate(&q_cone, &ctx).unwrap().ranges(), &[(0, 1)]);
}
