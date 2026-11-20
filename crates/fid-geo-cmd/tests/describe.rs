//! Acceptance criteria of spec node GEO.TYPES.REG (AC-1 .. AC-6), plus fail-closed checks.
//!
//! Expected describe bytes, the derived UUIDv5 and the ID of an element imported with a
//! non-UUID source ID come from `fixtures/geo/types/reg/expected.json`, written by the
//! independent stdlib reference `fixtures/geo/types/reg/make_reg_fixtures.py`, which reads
//! the GEO.TYPES fixtures (`pointset_small.txt`, `mesh_cube.txt`, `mesh_cube.facts.json`,
//! `digest_vectors.json`) directly. Nothing expected is produced by this crate.
//!
//! The test project is in memory: a `BTreeMap` element tree, a CORE.DEP edge index and
//! element states, a replayed observation source, a test "Record Journal" holding the
//! canonical bytes of every event registration returns, and an Activity Log.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_dep::{EdgeIndex, EdgesSet, ElementState, ElementStates, RecomputeMode};
use fid_geo_cmd::{
    command_defs, derive_id, describe_element, imported_text_id, invoke_cli, invoke_mcp, register_element,
    replace_element, ActivityEntry, DescribeParams, GeoCmdError, GeoTx, IdOrigin, Registered, Surface, TreeEntry,
    CLI_DESCRIBE, DESCRIBE_ELEMENT, MCP_DESCRIBE,
};
use fid_geo_types::{digest, FrameRef, GeometryElement, Mesh, PointBuffer, PointSet};
use fid_project::{Bits, Observation, ReplaySource};
use fid_registry::{tool_name, Catalogue, CommandDecl};
use fid_types::{CommandId, Digest};
use fid_uuid::Uuid;
use std::collections::BTreeMap;

// ------------------------------------------------------------------ fixtures

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/types/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

fn lines(text: &str) -> Vec<Vec<&str>> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split_whitespace().collect())
        .collect()
}

fn frame_hex(s: &str) -> FrameRef {
    assert_eq!(s.len(), 32);
    let mut b = [0u8; 16];
    for (i, o) in b.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    FrameRef::from_bytes(b)
}

fn f64_hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn pointset_small() -> GeometryElement {
    let text = read("pointset_small.txt");
    let mut frame = None;
    let mut pts = Vec::new();
    for l in lines(&text) {
        match l[0] {
            "frame" => frame = Some(frame_hex(l[1])),
            "p" => pts.push([f64_hex(l[2]), f64_hex(l[3]), f64_hex(l[4])]),
            _ => {}
        }
    }
    GeometryElement::PointSet(PointSet::from_points(frame.unwrap(), &pts).unwrap())
}

fn mesh_cube_parts() -> (FrameRef, Vec<[f64; 3]>, Vec<[u64; 3]>) {
    let text = read("mesh_cube.txt");
    let mut frame = None;
    let (mut v, mut t) = (Vec::new(), Vec::new());
    for l in lines(&text) {
        match l[0] {
            "frame" => frame = Some(frame_hex(l[1])),
            "v" => v.push([l[1].parse().unwrap(), l[2].parse().unwrap(), l[3].parse().unwrap()]),
            "t" => t.push([l[1].parse().unwrap(), l[2].parse().unwrap(), l[3].parse().unwrap()]),
            _ => {}
        }
    }
    (frame.unwrap(), v, t)
}

fn mesh_cube() -> GeometryElement {
    let (frame, v, t) = mesh_cube_parts();
    GeometryElement::Mesh(Mesh::from_points(frame, &v, &t).unwrap())
}

/// The cube with its top two triangles removed: a different, open mesh in the same frame.
fn mesh_open_box() -> GeometryElement {
    let (frame, v, t) = mesh_cube_parts();
    let open: Vec<[u64; 3]> = t.into_iter().filter(|tri| !(tri[0] == 4 && tri[1] >= 5)).collect();
    assert_eq!(open.len(), 10);
    GeometryElement::Mesh(Mesh::from_points(frame, &v, &open).unwrap())
}

struct Expected(BTreeMap<String, CValue>);

impl Expected {
    fn load() -> Expected {
        let CValue::Obj(o) = parse(read("reg/expected.json").as_bytes()).unwrap() else { panic!() };
        Expected(o)
    }

    fn at(&self, path: &[&str]) -> &CValue {
        let mut v = self.0.get(path[0]).unwrap();
        for k in &path[1..] {
            let CValue::Obj(o) = v else { panic!("{path:?}") };
            v = o.get(*k).unwrap();
        }
        v
    }

    fn str(&self, path: &[&str]) -> String {
        match self.at(path) {
            CValue::Str(s) => s.clone(),
            other => panic!("{path:?}: {other:?}"),
        }
    }

    fn uuid(&self, path: &[&str]) -> Uuid {
        Uuid::parse(&self.str(path)).unwrap()
    }
}

// ------------------------------------------------------------------ test project

struct Project {
    tree: BTreeMap<Uuid, TreeEntry>,
    edges: EdgeIndex,
    states: ElementStates,
    namespace: Uuid,
    source: ReplaySource,
    journal: Vec<Vec<u8>>,
    activity: Vec<ActivityEntry>,
}

impl Project {
    fn new(exp: &Expected) -> Project {
        Project::with_observations(exp, Vec::new())
    }

    fn with_observations(exp: &Expected, recorded: Vec<Observation>) -> Project {
        Project {
            tree: BTreeMap::new(),
            edges: EdgeIndex::new(),
            states: ElementStates::new(),
            namespace: exp.uuid(&["namespace"]),
            source: ReplaySource::new(recorded),
            journal: Vec::new(),
            activity: Vec::new(),
        }
    }

    fn tx(&mut self) -> GeoTx<'_> {
        GeoTx {
            tree: &mut self.tree,
            edges: &mut self.edges,
            states: &mut self.states,
            mode: RecomputeMode::Manual,
            namespace: self.namespace,
            source: &mut self.source,
        }
    }

    fn register(&mut self, element: GeometryElement, origin: IdOrigin) -> Result<Registered, GeoCmdError> {
        let r = register_element(&mut self.tx(), element, origin)?;
        self.journal.push(to_canonical_bytes(&r.edges.to_value()).unwrap());
        Ok(r)
    }

    fn cli(&mut self, id: &str) -> Result<Vec<u8>, GeoCmdError> {
        invoke_cli(&self.tree, &mut self.activity, &[CLI_DESCRIBE, id, "--json"])
    }

    fn mcp(&mut self, id: &str) -> Result<Vec<u8>, GeoCmdError> {
        let params = format!("{{\"element\":\"{id}\"}}");
        invoke_mcp(&self.tree, &mut self.activity, MCP_DESCRIBE, params.as_bytes())
    }

    /// Everything registration may change, for "changed nothing" checks.
    fn snapshot(&self) -> (BTreeMap<Uuid, TreeEntry>, CValue, Vec<(Uuid, ElementState)>, usize) {
        let states = self.states.iter().map(|(k, v)| (*k, *v)).collect();
        (self.tree.clone(), self.edges.to_value(), states, self.journal.len())
    }
}

/// A project with `pointset_small` and `mesh_cube` imported under the fixture IDs.
fn with_fixtures(exp: &Expected) -> Project {
    let mut p = Project::new(exp);
    let a = p.register(pointset_small(), IdOrigin::Imported(exp.str(&["ids", "pointset_small"]))).unwrap();
    let b = p.register(mesh_cube(), IdOrigin::Imported(exp.str(&["ids", "mesh_cube"]))).unwrap();
    assert_eq!(a.id, exp.uuid(&["ids", "pointset_small"]));
    assert_eq!(b.id, exp.uuid(&["ids", "mesh_cube"]));
    p
}

fn derived_origin(exp: &Expected) -> IdOrigin {
    let CValue::Arr(given) = exp.at(&["derived", "inputs_given"]) else { panic!() };
    let inputs = given
        .iter()
        .map(|pair| {
            let CValue::Arr(pair) = pair else { panic!() };
            let (CValue::Str(id), CValue::Str(d)) = (&pair[0], &pair[1]) else { panic!() };
            (Uuid::parse(id).unwrap(), Digest::parse(d).unwrap())
        })
        .collect();
    IdOrigin::Derived { command: CommandId::parse(&exp.str(&["derived", "command"])).unwrap(), inputs }
}

fn derived_element() -> GeometryElement {
    let GeometryElement::Mesh(m) = mesh_cube() else { panic!() };
    GeometryElement::PointSet(m.vertices_to_point_set())
}

fn obj_of(bytes: &[u8]) -> BTreeMap<String, CValue> {
    let CValue::Obj(o) = parse(bytes).unwrap() else { panic!() };
    o
}

// ------------------------------------------------------------------ AC-1, AC-2

#[test]
fn ac1_point_set_is_described_from_the_fixture() {
    let exp = Expected::load();
    let mut p = with_fixtures(&exp);
    let id = exp.str(&["ids", "pointset_small"]);
    let cli = p.cli(&id).unwrap();
    assert_eq!(String::from_utf8(cli.clone()).unwrap(), exp.str(&["describe", "pointset_small"]));
    let o = obj_of(&cli);
    assert_eq!(o.get("kind"), Some(&CValue::Str("PointSet".into())));
    assert_eq!(o.get("points"), Some(&CValue::Num(1000.0)));
    // Both surfaces return identical bytes, and each call is one Activity Log line.
    assert_eq!(p.mcp(&id).unwrap(), cli);
    assert_eq!(
        p.activity,
        [
            ActivityEntry { command: DESCRIBE_ELEMENT, surface: Surface::Cli, outcome: "ok" },
            ActivityEntry { command: DESCRIBE_ELEMENT, surface: Surface::Mcp, outcome: "ok" },
        ]
    );
}

#[test]
fn ac2_mesh_cube_is_closed_with_twelve_triangles() {
    let exp = Expected::load();
    let mut p = with_fixtures(&exp);
    let out = p.cli(&exp.str(&["ids", "mesh_cube"])).unwrap();
    assert_eq!(String::from_utf8(out.clone()).unwrap(), exp.str(&["describe", "mesh_cube"]));
    let o = obj_of(&out);
    assert_eq!(o.get("triangles"), Some(&CValue::Num(12.0)));
    let Some(CValue::Obj(t)) = o.get("topology") else { panic!() };
    assert_eq!(t.get("closed"), Some(&CValue::Bool(true)));
}

#[test]
fn describe_reads_cached_facts_through_the_typed_entry_point() {
    let exp = Expected::load();
    let p = with_fixtures(&exp);
    let v = describe_element(&p.tree, &DescribeParams { element: exp.uuid(&["ids", "mesh_cube"]) }).unwrap();
    assert_eq!(String::from_utf8(to_canonical_bytes(&v).unwrap()).unwrap(), exp.str(&["describe", "mesh_cube"]));
}

// ------------------------------------------------------------------ AC-3 and REQ-005

#[test]
fn ac3_unknown_id_fails_not_found_and_writes_only_the_activity_log() {
    let exp = Expected::load();
    let mut p = with_fixtures(&exp);
    let before = p.snapshot();
    let unknown = "0190a5b2-7c3d-7000-8000-0000000000ff";
    let err = p.cli(unknown).unwrap_err();
    assert_eq!(err.code(), "not_found");
    assert_eq!(err, GeoCmdError::NotFound(Uuid::parse(unknown).unwrap()));
    assert_eq!(p.snapshot(), before, "Record Journal and project unchanged");
    assert_eq!(p.activity, [ActivityEntry { command: DESCRIBE_ELEMENT, surface: Surface::Cli, outcome: "not_found" }]);
}

#[test]
fn a_non_geometric_element_fails_not_geometry() {
    let exp = Expected::load();
    let mut p = with_fixtures(&exp);
    let feature = Uuid::parse("0190a5b2-7c3d-7000-8000-0000000000aa").unwrap();
    p.tree.insert(feature, TreeEntry::Other { kind: "feature".into() });
    let before = p.snapshot();
    assert_eq!(p.mcp(&feature.to_string()).unwrap_err(), GeoCmdError::NotGeometry(feature));
    assert_eq!(p.snapshot(), before);
    assert_eq!(p.activity.len(), 1);
    assert_eq!(p.activity[0].outcome, "not_geometry");
}

#[test]
fn malformed_calls_are_refused_and_logged_once_each() {
    let exp = Expected::load();
    let mut p = with_fixtures(&exp);
    let id_text = exp.str(&["ids", "mesh_cube"]);
    let id = id_text.as_str();
    let upper = id_text.to_uppercase();
    let t = &p.tree;
    let log = &mut p.activity;
    let cases: Vec<(Result<Vec<u8>, GeoCmdError>, &str)> = vec![
        (invoke_cli(t, log, &[CLI_DESCRIBE]), "bad_params"),
        (invoke_cli(t, log, &[CLI_DESCRIBE, id, id]), "bad_params"),
        (invoke_cli(t, log, &[CLI_DESCRIBE, id, "--json", "--json"]), "bad_params"),
        (invoke_cli(t, log, &[CLI_DESCRIBE, id, "--all"]), "bad_params"),
        (invoke_cli(t, log, &[CLI_DESCRIBE, upper.as_str()]), "bad_params"),
        (invoke_cli(t, log, &[CLI_DESCRIBE, "00000000-0000-0000-0000-000000000000"]), "bad_params"),
        (invoke_cli(t, log, &["describe-mesh", id]), "unknown_command"),
        (invoke_cli(t, log, &[]), "bad_params"),
        (invoke_mcp(t, log, "geo_describe", b"{}"), "unknown_command"),
        (invoke_mcp(t, log, MCP_DESCRIBE, b"{}"), "bad_params"),
        (invoke_mcp(t, log, MCP_DESCRIBE, b"[]"), "bad_params"),
        (invoke_mcp(t, log, MCP_DESCRIBE, b"{\"element\":1}"), "bad_params"),
        (invoke_mcp(t, log, MCP_DESCRIBE, format!("{{\"element\":\"{id}\",\"x\":1}}").as_bytes()), "bad_params"),
        (
            invoke_mcp(t, log, MCP_DESCRIBE, format!("{{\"element\":\"{id}\",\"element\":\"{id}\"}}").as_bytes()),
            "bad_params",
        ),
        (invoke_mcp(t, log, MCP_DESCRIBE, &vec![b' '; 8192]), "input_limit"),
    ];
    let n = cases.len();
    for (i, (got, code)) in cases.into_iter().enumerate() {
        assert_eq!(got.unwrap_err().code(), code, "case {i}");
    }
    assert_eq!(p.activity.len(), n);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_a_derived_element_gets_the_same_uuidv5_every_time() {
    let exp = Expected::load();
    let want = exp.uuid(&["derived", "id"]);
    let mut ids = Vec::new();
    for _ in 0..2 {
        let mut p = with_fixtures(&exp);
        let r = p.register(derived_element(), derived_origin(&exp)).unwrap();
        assert_eq!(r.id.version(), 5);
        // The new element depends on both inputs, and is current.
        let mut inputs = vec![exp.uuid(&["ids", "pointset_small"]), exp.uuid(&["ids", "mesh_cube"])];
        inputs.sort();
        assert_eq!(r.edges, EdgesSet::new(r.id, inputs).unwrap());
        assert_eq!(p.states.get(&r.id), Some(ElementState::Current));
        ids.push(r.id);
    }
    assert_eq!(ids, [want, want]);
    // The pure function agrees, and the input order does not matter.
    let IdOrigin::Derived { command, mut inputs } = derived_origin(&exp) else { panic!() };
    assert_eq!(derive_id(exp.uuid(&["namespace"]), &command, &inputs).unwrap(), want);
    inputs.reverse();
    assert_eq!(derive_id(exp.uuid(&["namespace"]), &command, &inputs).unwrap(), want);
}

#[test]
fn ac4_imported_source_ids_are_kept() {
    let exp = Expected::load();
    let mut p = Project::new(&exp);
    // A non-UUID source ID: kept verbatim; the entity ID is fixed by it.
    let text = exp.str(&["imported_text", "source_id"]);
    assert_eq!(text, "step:#123");
    let r = p.register(mesh_cube(), IdOrigin::Imported(text.clone())).unwrap();
    assert_eq!(r.id, exp.uuid(&["imported_text", "id"]));
    assert_eq!(imported_text_id(exp.uuid(&["namespace"]), &text).unwrap(), r.id);
    let o = obj_of(&p.cli(&r.id.to_string()).unwrap());
    assert_eq!(o.get("source_id"), Some(&CValue::Str(text.clone())));
    // Importing it again is a collision, and changes nothing.
    let before = p.snapshot();
    assert_eq!(p.register(mesh_cube(), IdOrigin::Imported(text)).unwrap_err(), GeoCmdError::IdCollision(r.id));
    assert_eq!(p.snapshot(), before);
    // A UUID source ID is the entity ID itself (AC-1 and AC-2 rely on this too).
    let id = exp.str(&["ids", "pointset_small"]);
    let r = p.register(pointset_small(), IdOrigin::Imported(id.clone())).unwrap();
    assert_eq!(r.id.to_string(), id);
    assert_eq!(r.edges, EdgesSet::new(r.id, Vec::new()).unwrap());
}

#[test]
fn registering_the_same_derived_element_twice_in_one_project_collides() {
    let exp = Expected::load();
    let mut p = with_fixtures(&exp);
    let r = p.register(derived_element(), derived_origin(&exp)).unwrap();
    let before = p.snapshot();
    assert_eq!(p.register(derived_element(), derived_origin(&exp)).unwrap_err(), GeoCmdError::IdCollision(r.id));
    assert_eq!(p.snapshot(), before);
}

#[test]
fn derivation_inputs_are_checked_before_anything_changes() {
    let exp = Expected::load();
    let mut p = with_fixtures(&exp);
    let before = p.snapshot();
    let command = CommandId::parse("geo.test_vertices_to_points@1").unwrap();
    let a = exp.uuid(&["ids", "pointset_small"]);
    let d = Digest::parse(&format!("sha256:{}", "0".repeat(64))).unwrap();
    let dup = IdOrigin::Derived { command: command.clone(), inputs: vec![(a, d), (a, d)] };
    assert_eq!(p.register(derived_element(), dup).unwrap_err(), GeoCmdError::DuplicateInput(a));
    let ghost = Uuid::parse("0190a5b2-7c3d-7000-8000-0000000000ee").unwrap();
    let missing = IdOrigin::Derived { command, inputs: vec![(ghost, d)] };
    let e = p.register(derived_element(), missing).unwrap_err();
    assert_eq!(e.code(), "not_found");
    assert!(matches!(e, GeoCmdError::Edge(fid_dep::EdgeError::DanglingInput { input, .. }) if input == ghost), "{e:?}");
    for bad in ["", "a\nb"] {
        assert_eq!(p.register(mesh_cube(), IdOrigin::Imported(bad.into())).unwrap_err().code(), "bad_source_id");
    }
    let long = "x".repeat(257);
    assert_eq!(p.register(mesh_cube(), IdOrigin::Imported(long)).unwrap_err().code(), "bad_source_id");
    assert_eq!(p.snapshot(), before);
}

#[test]
fn a_new_element_gets_a_uuidv7_from_the_recorded_source() {
    let exp = Expected::load();
    let recorded = vec![
        Observation::NowMs(0x0190_a5b2_7c3d),
        Observation::RandomBits(Bits::new(74, 0x2_0000_0000_0000_0042).unwrap()),
    ];
    let mut ids = Vec::new();
    for _ in 0..2 {
        let mut p = Project::with_observations(&exp, recorded.clone());
        let r = p.register(mesh_cube(), IdOrigin::New).unwrap();
        assert_eq!(r.id.version(), 7);
        let o = obj_of(&p.cli(&r.id.to_string()).unwrap());
        assert_eq!(o.get("source_id"), Some(&CValue::Null));
        ids.push(r.id);
    }
    // Replay of the same observations rebuilds the same ID.
    assert_eq!(ids[0], ids[1]);
    assert!(ids[0].to_string().starts_with("0190a5b2-7c3d-7"));
    // With nothing recorded, replay is exhausted and nothing is registered.
    let mut p = Project::new(&exp);
    let before = p.snapshot();
    assert_eq!(p.register(mesh_cube(), IdOrigin::New).unwrap_err().code(), "replay_exhausted");
    assert_eq!(p.snapshot(), before);
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_replacement_keeps_the_id_updates_the_digest_and_outdates_dependents() {
    let exp = Expected::load();
    let mut p = with_fixtures(&exp);
    let mesh = exp.uuid(&["ids", "mesh_cube"]);
    let derived = p.register(derived_element(), derived_origin(&exp)).unwrap().id;
    // The mock part of the dependency graph: a non-geometric feature computed from the mesh.
    let feature = Uuid::parse("0190a5b2-7c3d-7000-8000-0000000000aa").unwrap();
    p.edges.apply(&[EdgesSet::new(feature, vec![mesh]).unwrap()]).unwrap();
    p.tree.insert(feature, TreeEntry::Other { kind: "feature".into() });
    p.states.insert(feature, ElementState::Current);

    let old = obj_of(&p.cli(&mesh.to_string()).unwrap());
    let new_mesh = mesh_open_box();
    let new_digest = digest(&new_mesh);
    let r = replace_element(&mut p.tx(), &mesh, new_mesh).unwrap();
    assert_eq!(r.id, mesh);
    assert_eq!(r.digest_after, new_digest);
    assert_ne!(r.digest_before, r.digest_after);
    let described = obj_of(exp.str(&["describe", "mesh_cube"]).as_bytes());
    assert_eq!(described.get("digest"), Some(&CValue::Str(Digest::from(r.digest_before).as_str().to_string())));

    let now = obj_of(&p.cli(&mesh.to_string()).unwrap());
    assert_eq!(now.get("id"), old.get("id"));
    assert_eq!(now.get("source_id"), old.get("source_id"));
    assert_eq!(now.get("digest"), Some(&CValue::Str(Digest::from(new_digest).as_str().to_string())));
    assert_eq!(now.get("triangles"), Some(&CValue::Num(10.0)));
    let Some(CValue::Obj(t)) = now.get("topology") else { panic!() };
    assert_eq!(t.get("closed"), Some(&CValue::Bool(false)));

    // CORE.DEP marks both dependents outdated, in the same transaction.
    let mut marked = vec![derived, feature];
    marked.sort();
    assert_eq!(r.propagation.marking.marked(), marked.as_slice());
    assert!(r.propagation.event.is_some());
    assert_eq!(r.propagation.submission, None, "manual mode submits nothing");
    assert_eq!(p.states.get(&derived), Some(ElementState::Outdated));
    assert_eq!(p.states.get(&feature), Some(ElementState::Outdated));
    assert_eq!(p.states.get(&mesh), Some(ElementState::Current));
    assert_eq!(p.states.get(&exp.uuid(&["ids", "pointset_small"])), Some(ElementState::Current));
}

#[test]
fn replacement_refusals_change_nothing() {
    let exp = Expected::load();
    let mut p = with_fixtures(&exp);
    let mesh = exp.uuid(&["ids", "mesh_cube"]);
    let feature = Uuid::parse("0190a5b2-7c3d-7000-8000-0000000000aa").unwrap();
    p.tree.insert(feature, TreeEntry::Other { kind: "feature".into() });
    let ghost = Uuid::parse("0190a5b2-7c3d-7000-8000-0000000000ee").unwrap();
    let before = p.snapshot();
    let pts = PointSet::new(FrameRef::from_bytes([1; 16]), PointBuffer::from_points(&[[0.0; 3]]), Default::default());
    let as_points = GeometryElement::PointSet(pts.unwrap());
    let e = replace_element(&mut p.tx(), &mesh, as_points).unwrap_err();
    assert_eq!(e.code(), "kind_mismatch");
    assert_eq!(replace_element(&mut p.tx(), &ghost, mesh_cube()).unwrap_err(), GeoCmdError::NotFound(ghost));
    assert_eq!(replace_element(&mut p.tx(), &feature, mesh_cube()).unwrap_err(), GeoCmdError::NotGeometry(feature));
    assert_eq!(p.snapshot(), before);
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_the_tool_is_in_the_manifest_with_projection_exposed() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    let decls: Vec<CommandDecl> = defs.iter().map(CommandDecl::from_def).collect();
    let export = fid_registry::export_with(&decls, &Catalogue::geo_frame().unwrap()).unwrap();
    let top = obj_of(&export.bytes);
    let Some(CValue::Arr(commands)) = top.get("commands") else { panic!() };
    assert_eq!(commands.len(), 1);
    let CValue::Obj(c) = &commands[0] else { panic!() };
    assert_eq!(c.get("id"), Some(&CValue::Str("geo.describe_element".into())));
    assert_eq!(c.get("major"), Some(&CValue::Num(1.0)));
    assert_eq!(format!("geo.describe_element@{}", 1), DESCRIBE_ELEMENT);
    assert_eq!(tool_name("geo.describe_element", 1), MCP_DESCRIBE);
    assert_eq!(c.get("mcp_projection"), Some(&CValue::Str("exposed".into())));
    assert_eq!(c.get("side_effect"), Some(&CValue::Str("R0".into())));
    assert_eq!(c.get("determinism"), Some(&CValue::Str("D0".into())));
    let Some(CValue::Arr(exposure)) = c.get("exposure") else { panic!() };
    assert!(exposure.contains(&CValue::Str("mcp".into())) && exposure.contains(&CValue::Str("cli".into())));
}
