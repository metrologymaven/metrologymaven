//! Acceptance tests AC-1..AC-6 and REQ-006 for `GEO.FRAME.CMD` (`fid_geo_cmd::frame_cmds`).
//! Uses `include_str!` so every test runs identically on x86-64, ARM64, Windows and `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_canon::{parse, CValue};
use fid_dep::{EdgeIndex, ElementState, ElementStates, RecomputeMode};
use fid_geo_cmd::{
    command_defs, invoke_frame_cli, invoke_frame_mcp, register_element, replay_frame_journal, ActivityEntry, EntityId,
    FrameJournalEntry, FramePrincipal, GeoCmdError, GeoTx, IdOrigin, TreeEntry, APPLY_TRANSFORM, CREATE_FRAME,
    DELETE_FRAME, MCP_CREATE_FRAME, MCP_RESOLVE_TRANSFORM, SET_TRANSFORM,
};
use fid_geo_frame::apply::transform_element;
use fid_geo_frame::tree::{Frame, FrameId, FrameKind, FrameTree};
use fid_geo_frame::units::{AuthoredQuantity, LengthUnit, Unit};
use fid_geo_frame::xform::{Rigid, Similarity, Transform};
use fid_geo_types::{AttributeSet, FrameRef, GeometryElement, PointBuffer, PointSet};
use fid_project::{Bits, Observation, ReplaySource};
use fid_registry::{mcp_projection, tool_name, Projection};
use fid_uuid::Uuid;

const TREE_7_JSON: &str = include_str!("../../../fixtures/geo/frame/tree_7.json");
const POINTSET_SMALL_TXT: &str = include_str!("../../../fixtures/geo/types/pointset_small.txt");

fn hex_f64(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap())
}

fn str_f<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k).unwrap() {
        CValue::Str(s) => s.as_str(),
        _ => panic!("field {k} not string"),
    }
}

fn load_pointset_small_in_world() -> GeometryElement {
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut zs = Vec::new();
    for line in POINTSET_SMALL_TXT.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("p ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            xs.push(hex_f64(parts[1]));
            ys.push(hex_f64(parts[2]));
            zs.push(hex_f64(parts[3]));
        }
    }
    let buf = PointBuffer::new(xs, ys, zs);
    let ps = PointSet::new(FrameRef::from_bytes(FrameId::WORLD.to_bytes()), buf, AttributeSet::default()).unwrap();
    GeometryElement::PointSet(ps)
}

type ExpectedXform = ([[f64; 3]; 3], [f64; 3], f64);

fn load_tree_7() -> (FrameTree, ExpectedXform) {
    let CValue::Obj(doc) = parse(TREE_7_JSON.as_bytes()).unwrap() else { panic!("tree_7 not object") };
    let CValue::Arr(records) = doc.get("records").unwrap() else { panic!("no records") };
    let mut id_by_name: BTreeMap<String, FrameId> = BTreeMap::new();
    let mut frames_raw = Vec::new();
    let mut expected_align1_inst2 = None;

    for rec in records {
        let CValue::Obj(o) = rec else { continue };
        match str_f(o, "rec") {
            "frame" => {
                let id = FrameId::parse(str_f(o, "id")).unwrap();
                let name = str_f(o, "name").to_owned();
                id_by_name.insert(name.clone(), id);
                frames_raw.push((id, name, o.clone()));
            }
            "pair" if str_f(o, "a") == "align_1" && str_f(o, "b") == "inst_2" => {
                let rv: Vec<f64> = str_f(o, "r").split_whitespace().map(hex_f64).collect();
                let tv: Vec<f64> = str_f(o, "t").split_whitespace().map(hex_f64).collect();
                let s = hex_f64(str_f(o, "s"));
                expected_align1_inst2 = Some((
                    [[rv[0], rv[1], rv[2]], [rv[3], rv[4], rv[5]], [rv[6], rv[7], rv[8]]],
                    [tv[0], tv[1], tv[2]],
                    s,
                ));
            }
            _ => {}
        }
    }

    let mut list = Vec::new();
    for (id, name, o) in frames_raw {
        let kind = match str_f(&o, "frame_kind") {
            "world" => FrameKind::World,
            "part" => FrameKind::Part,
            "instrument" => FrameKind::Instrument,
            "alignment" => FrameKind::Alignment,
            _ => FrameKind::User,
        };
        let parent_s = str_f(&o, "parent");
        let parent = if parent_s.is_empty() { None } else { Some(id_by_name[parent_s]) };
        let rv: Vec<f64> = str_f(&o, "r").split_whitespace().map(hex_f64).collect();
        let tv: Vec<f64> = str_f(&o, "t").split_whitespace().map(hex_f64).collect();
        let s = hex_f64(str_f(&o, "s"));
        let rigid =
            Rigid::new([[rv[0], rv[1], rv[2]], [rv[3], rv[4], rv[5]], [rv[6], rv[7], rv[8]]], [tv[0], tv[1], tv[2]])
                .unwrap();
        let to_parent = if str_f(&o, "xkind") == "similarity" {
            Transform::Similarity(Similarity::new(rigid, s).unwrap())
        } else {
            Transform::Rigid(rigid)
        };
        list.push(Frame { id, name, kind, parent, to_parent });
    }
    (FrameTree::from_frames(list).unwrap(), expected_align1_inst2.unwrap())
}

fn replay_source(seed: u64) -> ReplaySource {
    let mut obs = Vec::new();
    for i in 0..32_u64 {
        let rand = (u128::from(seed.wrapping_add(i.wrapping_mul(0x9e37_79b9))) << 16) | u128::from(i + 1);
        obs.push(Observation::NowMs(1_764_000_000_000 + i * 10));
        obs.push(Observation::RandomBits(Bits::new(74, rand & ((1_u128 << 74) - 1)).unwrap()));
    }
    ReplaySource::new(obs)
}

#[test]
fn ac1_ac2_ac3_ac5_create_apply_outdate_delete_and_exact_replay() {
    let ns = Uuid::from_u128(0x0195_0000_0000_7000_8000_0000_0000_0001);
    let human = FramePrincipal::Human { id: "user:alice".to_owned() };

    let mut frames = FrameTree::new();
    let mut tree: BTreeMap<EntityId, TreeEntry> = BTreeMap::new();
    let mut edges = EdgeIndex::new();
    let mut states = ElementStates::new();
    let mut obs = replay_source(11);
    let mut journal: Vec<FrameJournalEntry> = Vec::new();
    let mut log: Vec<ActivityEntry> = Vec::new();

    // Register pointset_small in world before frame commands.
    let ps_world = load_pointset_small_in_world();
    let ps_id = {
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut obs,
        };
        register_element(&mut tx, ps_world.clone(), IdOrigin::New).unwrap().id
    };

    // AC-1: `fid geo create-frame --name part_a --parent world --translation "10 mm,0 mm,5 mm" --rotation-euler ZYX:0,0,90deg --json`
    {
        let usage = tree.clone();
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut obs,
        };
        let out = invoke_frame_cli(
            &mut frames,
            &mut tx,
            &usage,
            &mut journal,
            &mut log,
            &human,
            &[
                "create-frame",
                "--name",
                "part_a",
                "--parent",
                "world",
                "--translation",
                "10 mm,0 mm,5 mm",
                "--rotation-euler",
                "ZYX:0,0,90deg",
                "--json",
            ],
        )
        .unwrap();
        let CValue::Obj(o) = parse(&out).unwrap() else { panic!() };
        assert_eq!(o.get("name"), Some(&CValue::Str("part_a".to_owned())));
    }
    assert!(frames.find_by_name("part_a").is_some());
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].command, CREATE_FRAME);
    // Verify the three conversion records carry exact si_hex for 0.01, 0 and 0.005 m!
    let expected_hex = [
        AuthoredQuantity::parse("10", Unit::Length(LengthUnit::Mm)).unwrap().si_hex(),
        AuthoredQuantity::parse("0", Unit::Length(LengthUnit::Mm)).unwrap().si_hex(),
        AuthoredQuantity::parse("5", Unit::Length(LengthUnit::Mm)).unwrap().si_hex(),
    ];
    let CValue::Obj(j_args) = &journal[0].args else { panic!() };
    let CValue::Arr(t_recs) = j_args.get("translation").unwrap() else { panic!() };
    assert_eq!(t_recs.len(), 3);
    for (i, rec) in t_recs.iter().enumerate() {
        let CValue::Obj(ro) = rec else { panic!() };
        assert_eq!(ro.get("si_hex"), Some(&CValue::Str(expected_hex[i].clone())));
    }

    // AC-2: `fid geo apply-transform <id> --to part_a --json` -> bit-exact match with GEO.FRAME.APPLY
    let derived_id = {
        let usage = tree.clone();
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut obs,
        };
        let ps_id_s = ps_id.to_string();
        let out = invoke_frame_cli(
            &mut frames,
            &mut tx,
            &usage,
            &mut journal,
            &mut log,
            &human,
            &["apply-transform", &ps_id_s, "--to", "part_a", "--json"],
        )
        .unwrap();
        let CValue::Obj(o) = parse(&out).unwrap() else { panic!() };
        let CValue::Str(id_s) = o.get("id").unwrap() else { panic!() };
        Uuid::parse(id_s).unwrap()
    };
    let part_a_id = frames.find_by_name("part_a").unwrap();
    let direct = transform_element(
        &ps_world,
        &frames.resolve(frames.world(), part_a_id).unwrap(),
        FrameRef::from_bytes(part_a_id.to_bytes()),
    )
    .unwrap();
    let TreeEntry::Geometry(stored) = tree.get(&derived_id).unwrap() else { panic!() };
    assert_eq!(stored.element, direct, "AC-2: transformed element must match GEO.FRAME.APPLY bit-for-bit");

    // AC-3: deleting part_a while referenced by derived_id fails with `frame_in_use` and journals nothing.
    let j_len_before = journal.len();
    {
        let usage = tree.clone();
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut obs,
        };
        let err = invoke_frame_cli(
            &mut frames,
            &mut tx,
            &usage,
            &mut journal,
            &mut log,
            &human,
            &["delete-frame", "part_a", "--json"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "frame_in_use");
        assert!(matches!(err, GeoCmdError::FrameInUse { refs, .. } if refs.contains(&derived_id)));
    }
    assert_eq!(journal.len(), j_len_before);

    // AC-5: exact replay of AC-1 + AC-2 journal reproduces bit-identical entries and digests!
    let mut r_frames = FrameTree::new();
    let mut r_tree: BTreeMap<EntityId, TreeEntry> = BTreeMap::new();
    let mut r_edges = EdgeIndex::new();
    let mut r_states = ElementStates::new();
    let mut r_obs = replay_source(11);
    {
        let mut tx = GeoTx {
            tree: &mut r_tree,
            edges: &mut r_edges,
            states: &mut r_states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut r_obs,
        };
        let r_ps = register_element(&mut tx, ps_world, IdOrigin::New).unwrap().id;
        assert_eq!(r_ps, ps_id);
    }
    {
        let usage = r_tree.clone();
        let mut tx = GeoTx {
            tree: &mut r_tree,
            edges: &mut r_edges,
            states: &mut r_states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut r_obs,
        };
        let replayed = replay_frame_journal(&mut r_frames, &mut tx, &usage, &journal).unwrap();
        assert_eq!(replayed.len(), journal.len());
    }
    assert_eq!(r_frames.digest(), frames.digest());
    assert_eq!(r_tree.get(&derived_id), tree.get(&derived_id));

    // AC-2 part 2: later `geo.set_transform@1` on `part_a` marks derived_id `Outdated`!
    {
        let usage = tree.clone();
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut obs,
        };
        invoke_frame_cli(
            &mut frames,
            &mut tx,
            &usage,
            &mut journal,
            &mut log,
            &human,
            &["set-transform", "part_a", "--translation", "20 mm,0 mm,5 mm", "--json"],
        )
        .unwrap();
    }
    assert_eq!(states.get(&derived_id), Some(ElementState::Outdated));
    assert_eq!(journal.last().unwrap().command, SET_TRANSFORM);

    // REQ-006: agent principal invoking W2 over MCP commits to its agent branch.
    let agent = FramePrincipal::Agent { id: "agent:planner".to_owned(), branch: "agent/planner-1".to_owned() };
    {
        let usage = tree.clone();
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut obs,
        };
        invoke_frame_mcp(
            &mut frames,
            &mut tx,
            &usage,
            &mut journal,
            &mut log,
            &agent,
            MCP_CREATE_FRAME,
            br#"{"name":"temp_frame","parent":"world","translation":[{"text":"1","unit":"mm"},{"text":"0","unit":"mm"},{"text":"0","unit":"mm"}]}"#,
        )
        .unwrap();
    }
    assert_eq!(journal.last().unwrap().branch.as_deref(), Some("agent/planner-1"));

    // Deleting unused `temp_frame` succeeds and appends one `geo.delete_frame@1` entry.
    {
        let usage = tree.clone();
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut obs,
        };
        invoke_frame_cli(
            &mut frames,
            &mut tx,
            &usage,
            &mut journal,
            &mut log,
            &human,
            &["delete-frame", "temp_frame", "--json"],
        )
        .unwrap();
    }
    assert!(frames.find_by_name("temp_frame").is_none());
    assert_eq!(journal.last().unwrap().command, DELETE_FRAME);
}

#[test]
fn ac4_ac6_resolve_tree7_convert_units_and_manifest_tools() {
    let (mut frames, (exp_r, exp_t, exp_s)) = load_tree_7();
    let mut tree: BTreeMap<EntityId, TreeEntry> = BTreeMap::new();
    let mut edges = EdgeIndex::new();
    let mut states = ElementStates::new();
    let mut obs = replay_source(7);
    let mut journal: Vec<FrameJournalEntry> = Vec::new();
    let mut log: Vec<ActivityEntry> = Vec::new();
    let human = FramePrincipal::Human { id: "user:bob".to_owned() };
    let ns = Uuid::from_u128(9);

    // AC-4: `fid geo resolve-transform --from align_1 --to inst_2 --json` matches tree_7 fixture & writes 0 journal entries.
    let out = {
        let usage = tree.clone();
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut obs,
        };
        invoke_frame_cli(
            &mut frames,
            &mut tx,
            &usage,
            &mut journal,
            &mut log,
            &human,
            &["resolve-transform", "--from", "align_1", "--to", "inst_2", "--json"],
        )
        .unwrap()
    };
    assert_eq!(journal.len(), 0, "R1 resolve-transform must write no Record Journal entry");
    assert_eq!(log.len(), 1);
    let CValue::Obj(o) = parse(&out).unwrap() else { panic!() };
    let CValue::Str(scale_s) = o.get("scale").unwrap() else { panic!() };
    let scale: f64 = scale_s.parse().unwrap();
    assert_eq!(scale.to_bits(), exp_s.to_bits());
    let CValue::Arr(mat_hex) = o.get("matrix_hex").unwrap() else { panic!() };
    for i in 0..3 {
        let CValue::Arr(row) = &mat_hex[i] else { panic!() };
        for j in 0..3 {
            let CValue::Str(hs) = &row[j] else { panic!() };
            let got = hex_f64(hs);
            let want = exp_r[i][j] * exp_s;
            assert!((got - want).abs() <= 1e-15, "row {i} col {j}: got {got} want {want}");
        }
        let CValue::Str(ts) = &row[3] else { panic!() };
        assert_eq!(hex_f64(ts).to_bits(), exp_t[i].to_bits());
    }

    // MCP resolve_transform matches CLI bytes bit-for-bit.
    let mcp_out = {
        let usage = tree.clone();
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut obs,
        };
        invoke_frame_mcp(
            &mut frames,
            &mut tx,
            &usage,
            &mut journal,
            &mut log,
            &human,
            MCP_RESOLVE_TRANSFORM,
            br#"{"from":"align_1","to":"inst_2"}"#,
        )
        .unwrap()
    };
    assert_eq!(out, mcp_out);
    assert_eq!(journal.len(), 0);

    // R1 convert-units writes no Record Journal entry.
    let conv_out = {
        let usage = tree.clone();
        let mut tx = GeoTx {
            tree: &mut tree,
            edges: &mut edges,
            states: &mut states,
            mode: RecomputeMode::Auto,
            namespace: ns,
            source: &mut obs,
        };
        invoke_frame_cli(
            &mut frames,
            &mut tx,
            &usage,
            &mut journal,
            &mut log,
            &human,
            &["convert-units", "--text", "25.4", "--unit", "mm", "--to-unit", "in", "--digits", "4", "--json"],
        )
        .unwrap()
    };
    assert_eq!(journal.len(), 0);
    let CValue::Obj(co) = parse(&conv_out).unwrap() else { panic!() };
    assert_eq!(co.get("formatted"), Some(&CValue::Str("1.0000".to_owned())));

    // AC-6: all six commands are registered with exact tool names and `exposed` projection.
    let defs = command_defs().unwrap();
    let expected = [
        ("geo.create_frame", "geo_create_frame", "W2"),
        ("geo.set_transform", "geo_set_transform", "W2"),
        ("geo.delete_frame", "geo_delete_frame", "W2"),
        ("geo.apply_transform", "geo_apply_transform", "W2"),
        ("geo.resolve_transform", "geo_resolve_transform", "R1"),
        ("geo.convert_units", "geo_convert_units", "R1"),
    ];
    for (id, mcp, se) in expected {
        let d = defs.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("missing {id}"));
        assert_eq!(d.side_effect.as_str(), se);
        assert_eq!(d.determinism.as_str(), "D0");
        assert_eq!(mcp_projection(d), Projection::Exposed);
        assert_eq!(tool_name(&d.id, d.major), mcp);
    }
    assert_eq!(APPLY_TRANSFORM, "geo.apply_transform@1");
}
