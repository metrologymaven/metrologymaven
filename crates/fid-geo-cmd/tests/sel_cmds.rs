//! Acceptance and unit tests for `GEO.SEL.CMD` (`spec/geo/sel/cmd/NODE.md`):
//! AC-1..AC-6, REQ-001..REQ-005 (create, CPU lasso/pick resolve without stored view fields,
//! combine algebra, grow/shrink, stale_selection refusal, CORE.DEP outdated propagation,
//! 10^6 inline index limit, exact replay, and manifest tool projections).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_dep::{EdgeIndex, ElementState, ElementStates, RecomputeMode};
use fid_geo_cmd::{
    command_defs, create_selection, invoke_sel_cli, invoke_sel_mcp, parse_index_spec, register_element,
    replace_element, replay_sel_journal, ActivityEntry, CreateSelectionParams, EntityId, FramePrincipal, GeoCmdError,
    GeoTx, IdOrigin, SelJournalEntry, SelectionRegistry, TreeEntry, CREATE_SELECTION, DESCRIBE_SELECTION,
    MAX_INLINE_INDICES, MCP_CREATE_SELECTION, MCP_DESCRIBE_SELECTION,
};
use fid_geo_frame::Rigid;
use fid_geo_sel::{
    resolve_lasso, union_selections, Camera, Domain, EntityId as SelEntityId, Lasso, Projection, ResolveMode,
    Selection, SelectionBody, TriangleRule,
};
use fid_geo_types::{digest as elem_digest, AttributeSet, FrameRef, GeometryElement, Mesh, PointBuffer, PointSet};
use fid_project::{Bits, Observation, ReplaySource};
use fid_registry::{mcp_projection, tool_name, Projection as McpProjection};
use fid_uuid::Uuid;

const POINTSET_SMALL: &str = include_str!("../../../fixtures/geo/types/pointset_small.txt");
const LASSO_CASES: &str = include_str!("../../../fixtures/geo/sel/lasso_cases/cases.txt");

fn unhex16(s: &str) -> [u8; 16] {
    assert_eq!(s.len(), 32);
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn load_pointset_small() -> GeometryElement {
    let mut frame = None;
    let mut buf = PointBuffer::default();
    for line in POINTSET_SMALL.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("frame ") {
            frame = Some(FrameRef::from_bytes(unhex16(rest)));
        } else if let Some(rest) = t.strip_prefix("p ") {
            let p: Vec<&str> = rest.split_whitespace().collect();
            buf.x.push(hexf(p[1]));
            buf.y.push(hexf(p[2]));
            buf.z.push(hexf(p[3]));
        }
    }
    GeometryElement::PointSet(PointSet::new(frame.unwrap(), buf, AttributeSet::default()).unwrap())
}

fn first_lasso_case() -> (Camera, Lasso, String, String) {
    for line in LASSO_CASES.lines() {
        let t = line.trim();
        if !t.starts_with("lasso ") {
            continue;
        }
        let sections: Vec<&str> = t.split('|').map(str::trim).collect();
        let cam_toks: Vec<&str> = sections[0].split_whitespace().collect();
        let kind = cam_toks[3];
        let vw: u32 = cam_toks[4].parse().unwrap();
        let vh: u32 = cam_toks[5].parse().unwrap();
        let p0 = hexf(cam_toks[6]);
        let aspect = hexf(cam_toks[7]);
        let near = hexf(cam_toks[8]);
        let far = hexf(cam_toks[9]);
        let r = [
            [hexf(cam_toks[10]), hexf(cam_toks[11]), hexf(cam_toks[12])],
            [hexf(cam_toks[13]), hexf(cam_toks[14]), hexf(cam_toks[15])],
            [hexf(cam_toks[16]), hexf(cam_toks[17]), hexf(cam_toks[18])],
        ];
        let tv = [hexf(cam_toks[19]), hexf(cam_toks[20]), hexf(cam_toks[21])];
        let proj = if kind == "persp" {
            Projection::Perspective { fov_y: p0, aspect, near, far }
        } else {
            Projection::Ortho { height: p0, aspect, near, far }
        };
        let cam = Camera { view: Rigid::new(r, tv).unwrap(), proj, viewport: (vw, vh) };

        let poly_toks: Vec<&str> = sections[1].split_whitespace().collect();
        let n_verts: usize = poly_toks[0].parse().unwrap();
        let mut pixels = Vec::with_capacity(n_verts);
        for k in 0..n_verts {
            pixels.push((hexf(poly_toks[1 + 2 * k]), hexf(poly_toks[2 + 2 * k])));
        }
        let lasso = Lasso::new(pixels.clone()).unwrap();

        let proj_fields = if kind == "persp" {
            format!(
                r#""projection":"perspective","fov_y":"0x{:016x}","aspect":"0x{:016x}","near":"0x{:016x}","far":"0x{:016x}""#,
                p0.to_bits(),
                aspect.to_bits(),
                near.to_bits(),
                far.to_bits()
            )
        } else {
            format!(
                r#""projection":"ortho","height":"0x{:016x}","aspect":"0x{:016x}","near":"0x{:016x}","far":"0x{:016x}""#,
                p0.to_bits(),
                aspect.to_bits(),
                near.to_bits(),
                far.to_bits()
            )
        };
        let cam_json = format!(
            r#"{{{proj_fields},"viewport":[{vw},{vh}],"view_r":[["0x{:016x}","0x{:016x}","0x{:016x}"],["0x{:016x}","0x{:016x}","0x{:016x}"],["0x{:016x}","0x{:016x}","0x{:016x}"]],"view_t":["0x{:016x}","0x{:016x}","0x{:016x}"]}}"#,
            r[0][0].to_bits(),
            r[0][1].to_bits(),
            r[0][2].to_bits(),
            r[1][0].to_bits(),
            r[1][1].to_bits(),
            r[1][2].to_bits(),
            r[2][0].to_bits(),
            r[2][1].to_bits(),
            r[2][2].to_bits(),
            tv[0].to_bits(),
            tv[1].to_bits(),
            tv[2].to_bits(),
        );
        let poly_pts = pixels
            .iter()
            .map(|(x, y)| format!(r#"["0x{:016x}","0x{:016x}"]"#, x.to_bits(), y.to_bits()))
            .collect::<Vec<_>>()
            .join(",");
        let lasso_json = format!(r#"{{"pixels":[{poly_pts}]}}"#);
        return (cam, lasso, cam_json, lasso_json);
    }
    panic!("no lasso case in fixture");
}

fn replay_source(seed: u64) -> ReplaySource {
    let mut obs = Vec::new();
    for i in 1..=32u64 {
        obs.push(Observation::NowMs(1_700_000_000_000 + seed * 100 + i));
        obs.push(Observation::RandomBits(Bits::new(74, u128::from(seed * 1000 + i)).unwrap()));
    }
    ReplaySource::new(obs)
}

fn grid_mesh(frame: FrameRef) -> GeometryElement {
    let mut buf = PointBuffer::default();
    for iy in 0..3 {
        for ix in 0..3 {
            buf.x.push(f64::from(ix) * 0.01);
            buf.y.push(f64::from(iy) * 0.01);
            buf.z.push(0.0);
        }
    }
    let mut tris = Vec::new();
    for iy in 0..2u64 {
        for ix in 0..2u64 {
            let a = iy * 3 + ix;
            let b = a + 1;
            let c = a + 3;
            let d = c + 1;
            tris.push([a, b, c]);
            tris.push([b, d, c]);
        }
    }
    GeometryElement::Mesh(Mesh::new(frame, buf, AttributeSet::default(), tris).unwrap())
}

#[test]
fn ac1_ac2_ac3_ac4_ac5_create_resolve_combine_grow_stale_and_replay() {
    let ps_elem = load_pointset_small();
    let frame = ps_elem.frame();
    let mesh_elem = grid_mesh(frame);

    let ns = Uuid::parse("0190a5b2-7c3d-7000-8000-000000000042").unwrap();
    let mut tree: BTreeMap<EntityId, TreeEntry> = BTreeMap::new();
    let mut edges = EdgeIndex::new();
    let mut states = ElementStates::new();
    let mut obs = replay_source(1);
    let mut sels = SelectionRegistry::new();
    let mut journal: Vec<SelJournalEntry> = Vec::new();
    let mut log: Vec<ActivityEntry> = Vec::new();
    let human = FramePrincipal::Human { id: "user:alice".to_owned() };
    let agent = FramePrincipal::Agent { id: "agent:planner".to_owned(), branch: "agent/plan-1".to_owned() };

    let (ps_id, mesh_id) = {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut obs,
        };
        let r1 = register_element(&mut tx, ps_elem.clone(), IdOrigin::New).unwrap();
        let r2 = register_element(&mut tx, mesh_elem.clone(), IdOrigin::New).unwrap();
        (r1.id, r2.id)
    };
    let ps_str = ps_id.to_string();
    let mesh_str = mesh_id.to_string();

    // AC-1: `fid geo create-selection <id> --indices 1-10,20 --json` -> 11 members, digest matches GEO.SEL.MODEL, +1 journal entry.
    let out_a = {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut obs,
        };
        invoke_sel_cli(
            &mut sels,
            &mut tx,
            &mut journal,
            &mut log,
            &human,
            &["create-selection", &ps_str, "--indices", "1-10,20", "--json"],
        )
        .unwrap()
    };
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].command, CREATE_SELECTION);
    let CValue::Obj(oa) = parse(&out_a).unwrap() else { panic!() };
    assert_eq!(oa.get("count"), Some(&CValue::Num(11.0)));
    assert_eq!(oa.get("ranges_count"), Some(&CValue::Num(2.0)));
    let CValue::Str(id_a_str) = oa.get("id").unwrap() else { panic!() };
    let id_a = Uuid::parse(id_a_str).unwrap();
    let expected_a = Selection::on_element(
        SelEntityId::from_bytes(*ps_id.as_bytes()),
        &ps_elem,
        Domain::Points,
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 20],
    )
    .unwrap();
    assert_eq!(
        oa.get("digest"),
        Some(&CValue::Str(expected_a.content_digest().unwrap().to_digest().as_str().to_owned()))
    );
    assert_eq!(sels.get(&id_a).unwrap().selection, expected_a);

    // AC-2: resolve-selection from fixtures/geo/sel/lasso_cases matches library and stores NO camera or pixel fields.
    let (cam, lasso, cam_json, lasso_json) = first_lasso_case();
    let expected_lasso_set =
        resolve_lasso(&ps_elem, &cam, &lasso, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through).unwrap();
    let expected_lasso_sel = Selection::new(
        SelEntityId::from_bytes(*ps_id.as_bytes()),
        elem_digest(&ps_elem),
        SelectionBody::IndexSet(expected_lasso_set),
    );
    let out_r = {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut obs,
        };
        invoke_sel_cli(
            &mut sels,
            &mut tx,
            &mut journal,
            &mut log,
            &agent,
            &["resolve-selection", &ps_str, "--camera", &cam_json, "--lasso", &lasso_json, "--json"],
        )
        .unwrap()
    };
    assert_eq!(journal.len(), 2);
    assert_eq!(journal[1].branch.as_deref(), Some("agent/plan-1"));
    let CValue::Obj(or_) = parse(&out_r).unwrap() else { panic!() };
    let CValue::Str(id_r_str) = or_.get("id").unwrap() else { panic!() };
    let id_r = Uuid::parse(id_r_str).unwrap();
    let stored_r = sels.get(&id_r).unwrap();
    assert_eq!(stored_r.selection, expected_lasso_sel);
    // Serialised stored selection must contain NO camera or pixel fields (REQ-002, AC-2).
    let ser_bytes = to_canonical_bytes(&stored_r.to_serialised_value().unwrap()).unwrap();
    let ser_str = std::str::from_utf8(&ser_bytes).unwrap();
    assert!(
        !ser_str.contains("camera")
            && !ser_str.contains("pixel")
            && !ser_str.contains("viewport")
            && !ser_str.contains("lasso")
    );
    // Journal args DO record camera & lasso, while journal entry records target_digest & selection_digest (REQ-002).
    let CValue::Obj(ref j_args) = journal[1].args else { panic!() };
    assert!(j_args.contains_key("camera"));
    assert!(j_args.contains_key("lasso"));
    assert_eq!(journal[1].selection_digest, expected_lasso_sel.content_digest().unwrap().to_digest());

    // Create second selection B on ps_id (`5-15`) and combine A union B (AC-3).
    let out_b = {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut obs,
        };
        invoke_sel_cli(
            &mut sels,
            &mut tx,
            &mut journal,
            &mut log,
            &human,
            &["create-selection", &ps_str, "--indices", "5-15", "--json"],
        )
        .unwrap()
    };
    let CValue::Obj(ob) = parse(&out_b).unwrap() else { panic!() };
    let CValue::Str(id_b_str) = ob.get("id").unwrap() else { panic!() };
    let id_b = Uuid::parse(id_b_str).unwrap();
    let snap_a = sels.get(&id_a).unwrap().clone();
    let snap_b = sels.get(&id_b).unwrap().clone();

    let out_u = {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut obs,
        };
        invoke_sel_cli(
            &mut sels,
            &mut tx,
            &mut journal,
            &mut log,
            &human,
            &["combine-selection", "--op", "union", id_a_str, id_b_str, "--json"],
        )
        .unwrap()
    };
    assert_eq!(journal.len(), 4);
    let CValue::Obj(ou) = parse(&out_u).unwrap() else { panic!() };
    let CValue::Str(id_u_str) = ou.get("id").unwrap() else { panic!() };
    let id_u = Uuid::parse(id_u_str).unwrap();
    let expected_u = union_selections(&snap_a.selection, &snap_b.selection).unwrap();
    assert_eq!(sels.get(&id_u).unwrap().selection, expected_u);
    assert_eq!(sels.get(&id_a).unwrap(), &snap_a, "A must remain unchanged");
    assert_eq!(sels.get(&id_b).unwrap(), &snap_b, "B must remain unchanged");

    // Grow mesh triangle selection by 1 edge ring.
    let out_m0 = {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut obs,
        };
        invoke_sel_cli(
            &mut sels,
            &mut tx,
            &mut journal,
            &mut log,
            &human,
            &["create-selection", &mesh_str, "--domain", "triangles", "--indices", "0", "--json"],
        )
        .unwrap()
    };
    let CValue::Obj(om0) = parse(&out_m0).unwrap() else { panic!() };
    let CValue::Str(id_m0_str) = om0.get("id").unwrap() else { panic!() };
    let out_mg = {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut obs,
        };
        invoke_sel_cli(
            &mut sels,
            &mut tx,
            &mut journal,
            &mut log,
            &human,
            &["grow-selection", id_m0_str, "--rings", "1", "--adjacency", "edge", "--json"],
        )
        .unwrap()
    };
    let CValue::Obj(omg) = parse(&out_mg).unwrap() else { panic!() };
    let CValue::Num(grown_count) = omg.get("count").unwrap() else { panic!() };
    assert!(*grown_count > 1.0);
    assert_eq!(journal.len(), 6);

    // R0 describe-selection writes NO Record Journal entry and logs 1 Activity Log entry.
    let j_before_desc = journal.len();
    let desc_bytes = {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut obs,
        };
        invoke_sel_mcp(
            &mut sels,
            &mut tx,
            &mut journal,
            &mut log,
            &human,
            MCP_DESCRIBE_SELECTION,
            format!(r#"{{"selection":"{id_u_str}"}}"#).as_bytes(),
        )
        .unwrap()
    };
    assert_eq!(journal.len(), j_before_desc);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].command, DESCRIBE_SELECTION);
    let CValue::Obj(od) = parse(&desc_bytes).unwrap() else { panic!() };
    assert_eq!(od.get("stale"), Some(&CValue::Bool(false)));
    assert_eq!(od.get("count"), Some(&CValue::Num(16.0)));

    // AC-5: Exact replay of the journal reproduces every entry bit-for-bit IDENTICAL.
    {
        let mut r_tree: BTreeMap<EntityId, TreeEntry> = BTreeMap::new();
        let mut r_edges = EdgeIndex::new();
        let mut r_states = ElementStates::new();
        let mut r_obs = replay_source(1);
        let mut r_sels = SelectionRegistry::new();
        let mut r_tx = GeoTx {
            tree: &mut r_tree,
            edges: &mut r_edges,
            states: &mut r_states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut r_obs,
        };
        let _ = register_element(&mut r_tx, ps_elem.clone(), IdOrigin::New).unwrap();
        let _ = register_element(&mut r_tx, mesh_elem.clone(), IdOrigin::New).unwrap();
        let replayed = replay_sel_journal(&mut r_sels, &mut r_tx, &journal).unwrap();
        assert_eq!(replayed.len(), journal.len());
        for (a, b) in replayed.iter().zip(journal.iter()) {
            assert_eq!(a.to_canonical_bytes().unwrap(), b.to_canonical_bytes().unwrap());
        }
    }

    // AC-4 & REQ-004: Replacing target marks selection outdated in CORE.DEP and combining returns stale_selection without journaling.
    let mut new_buf = PointBuffer::default();
    for i in 0..9u32 {
        new_buf.x.push(f64::from(i) * 0.02 + 1.0);
        new_buf.y.push(f64::from(i) * 0.01);
        new_buf.z.push(0.5);
    }
    let replaced_mesh =
        GeometryElement::Mesh(Mesh::new(frame, new_buf, AttributeSet::default(), vec![[0, 1, 2], [2, 3, 4]]).unwrap());
    let id_m0 = Uuid::parse(id_m0_str).unwrap();
    {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut obs,
        };
        let _ = replace_element(&mut tx, &mesh_id, replaced_mesh, &[]).unwrap();
    }
    assert_eq!(
        states.get(&id_m0),
        Some(ElementState::Outdated),
        "REQ-004: target replacement must mark selection outdated"
    );

    let j_len_before_stale = journal.len();
    let stale_err = {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Manual,
            namespace: ns,
            source: &mut obs,
        };
        invoke_sel_cli(
            &mut sels,
            &mut tx,
            &mut journal,
            &mut log,
            &human,
            &["combine-selection", "--op", "invert", id_m0_str, "--json"],
        )
        .unwrap_err()
    };
    assert_eq!(stale_err.code(), "stale_selection");
    assert_eq!(journal.len(), j_len_before_stale, "AC-4: stale_selection must journal nothing");
}

#[test]
fn req005_limit_and_ac6_manifest_tools() {
    // REQ-005: > 10^6 inline indices fails with too_many_indices.
    let err = parse_index_spec("0-1000000").unwrap_err();
    assert_eq!(err.code(), "too_many_indices");
    assert!(matches!(err, GeoCmdError::TooManyIndices { max, .. } if max == MAX_INLINE_INDICES));

    // Also verify create_selection rejects Vec of length MAX_INLINE_INDICES + 1 without journaling.
    let ps = load_pointset_small();
    let ns = Uuid::from_u128(77);
    let mut tree: BTreeMap<EntityId, TreeEntry> = BTreeMap::new();
    let mut edges = EdgeIndex::new();
    let mut states = ElementStates::new();
    let mut obs = replay_source(2);
    let mut sels = SelectionRegistry::new();
    let mut journal = Vec::new();
    let human = FramePrincipal::Human { id: "user:bob".to_owned() };
    let mut tx = GeoTx {
        tree: &mut tree,
        edges: &mut edges,
        states: &mut states,
        mode: RecomputeMode::Manual,
        namespace: ns,
        source: &mut obs,
    };
    let pid = register_element(&mut tx, ps, IdOrigin::New).unwrap().id;
    let over = create_selection(
        &mut sels,
        &mut tx,
        &mut journal,
        &human,
        &CreateSelectionParams { target: pid, domain: Some(Domain::Points), indices: vec![0; MAX_INLINE_INDICES + 1] },
    )
    .unwrap_err();
    assert_eq!(over.code(), "too_many_indices");
    assert!(journal.is_empty());

    // MCP create_selection matches schema & commits one entry.
    let mut log = Vec::new();
    let mcp_out = invoke_sel_mcp(
        &mut sels,
        &mut tx,
        &mut journal,
        &mut log,
        &human,
        MCP_CREATE_SELECTION,
        format!(r#"{{"target":"{pid}","indices":[0,2,4]}}"#).as_bytes(),
    )
    .unwrap();
    let CValue::Obj(mo) = parse(&mcp_out).unwrap() else { panic!() };
    assert_eq!(mo.get("count"), Some(&CValue::Num(3.0)));
    assert_eq!(journal.len(), 1);

    // AC-6: four W2 tools and R0 geo_describe_selection are registered with exact names and exposed projection.
    let defs = command_defs().unwrap();
    let expected = [
        ("geo.create_selection", "geo_create_selection", "W2"),
        ("geo.resolve_selection", "geo_resolve_selection", "W2"),
        ("geo.combine_selection", "geo_combine_selection", "W2"),
        ("geo.grow_selection", "geo_grow_selection", "W2"),
        ("geo.describe_selection", "geo_describe_selection", "R0"),
    ];
    for (id, mcp, se) in expected {
        let d = defs.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("missing {id}"));
        assert_eq!(d.side_effect.as_str(), se);
        assert_eq!(d.determinism.as_str(), "D0");
        assert_eq!(mcp_projection(d), McpProjection::Exposed);
        assert_eq!(tool_name(&d.id, d.major), mcp);
    }
}
