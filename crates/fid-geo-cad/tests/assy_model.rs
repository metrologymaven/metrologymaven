//! GEO.CAD.ASSY.MODEL acceptance tests.
//!
//! Expected IDs come from outside the Rust code: `fixtures/geo/cad/assy/expected.json`,
//! written by the independent stdlib Python reference
//! `fixtures/geo/cad/assy/make_assy_fixtures.py` from the hand-written `two_bolts.json`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;

use fid_geo_cad::{
    AssyModelError, Body, EntityIx, Instance, InstanceEntityId, InstanceId, InstanceIndex, InstanceIx, InstancePath,
    InstanceView, Label, TopologyId,
};
use fid_geo_frame::{Frame, FrameId, FrameKind, FrameTree, Rigid, Transform};
use fid_num::rng::{Address, Cursor};

mod common;

use common::*;

const ID: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn fixture() -> J {
    load("../assy/two_bolts.json")
}

fn expected() -> J {
    load("../assy/expected.json")
}

fn frame_id(s: &str) -> FrameId {
    FrameId::parse(s).unwrap()
}

fn uuid_text(b: [u8; 16]) -> String {
    let h = hex(&b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn id_text(id: &TopologyId) -> String {
    match id {
        TopologyId::Imported(l) => l.as_str().to_string(),
        TopologyId::Derived(_) => id.to_string(),
    }
}

fn path_of(occ: &J) -> InstancePath {
    let comps = occ
        .get("path")
        .arr()
        .iter()
        .map(|c| match c.opt("label") {
            Some(l) => TopologyId::Imported(Label::new(l.str()).unwrap()),
            None => TopologyId::Derived(unhex(c.get("derived").str()).try_into().unwrap()),
        })
        .collect();
    InstancePath::new(comps).unwrap()
}

fn bolt() -> Body {
    builder(fixture().get("body")).build().unwrap()
}

fn instances(fx: &J, body: &Body) -> Vec<Instance> {
    fx.get("occurrences")
        .arr()
        .iter()
        .map(|o| Instance::new(*body.id(), frame_id(o.get("frame").str()), path_of(o)))
        .collect()
}

/// World, the assembly frame, and one frame per occurrence (skipping `skip`).
fn tree(fx: &J, skip: Option<&str>) -> FrameTree {
    let mut t = FrameTree::new();
    let asm = frame_id(fx.get("assembly_frame").str());
    t.insert(Frame {
        id: asm,
        name: "assembly".into(),
        kind: FrameKind::Part,
        parent: Some(FrameId::WORLD),
        to_parent: Transform::IDENTITY,
    })
    .unwrap();
    for o in fx.get("occurrences").arr() {
        if Some(o.get("name").str()) == skip {
            continue;
        }
        t.insert(Frame {
            id: frame_id(o.get("frame").str()),
            name: o.get("name").str().into(),
            kind: FrameKind::Part,
            parent: Some(asm),
            to_parent: Transform::Rigid(Rigid::new(ID, o.get("translation").v3()).unwrap()),
        })
        .unwrap();
    }
    t
}

/// The (kind, definition ID, instance entity ID) triples of one instance in canonical body
/// order (faces, edges, vertices), as the fixture script lists them.
fn rows(index: &InstanceIndex, ix: InstanceIx, body: &Body) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let defs = body
        .faces()
        .iter()
        .map(|x| ("face", x.id()))
        .chain(body.edges().iter().map(|x| ("edge", x.id())))
        .chain(body.vertices().iter().map(|x| ("vertex", x.id())));
    for (kind, tid) in defs {
        let ent = body.lookup(tid).unwrap();
        let iid = index.entity_id(ix, ent).unwrap();
        out.push((kind.to_string(), id_text(tid), iid.to_string()));
    }
    out
}

#[test]
fn ac1_two_bolts_distinct_committed_ids() {
    let fx = fixture();
    let want = expected();
    let body = bolt();
    let per = body.faces().len() + body.edges().len() + body.vertices().len();
    assert_eq!(per, 5 + 6 + 4);
    let index = InstanceIndex::build(instances(&fx, &body), &[&body]).unwrap();
    assert_eq!(index.entity_count(), 2 * per);
    let distinct: BTreeSet<InstanceEntityId> = index.entities().map(|e| e.0).collect();
    assert_eq!(distinct.len(), 2 * per);

    let insts = want.get("instances").arr();
    assert_eq!(insts.len(), 2);
    for (occ, exp) in fx.get("occurrences").arr().iter().zip(insts) {
        assert_eq!(occ.get("name").str(), exp.get("name").str());
        let path = path_of(occ);
        assert_eq!(hex(&path.to_bytes()), exp.get("path_bytes").str());
        let ix = index.find(&path).unwrap();
        let inst = index.instance(ix).unwrap();
        assert_eq!(inst.id().to_string(), exp.get("instance_id").str());
        assert_eq!(inst.id(), InstanceId::derive(&path));
        let got = rows(&index, ix, &body);
        let exp_rows: Vec<(String, String, String)> = exp
            .get("entities")
            .arr()
            .iter()
            .map(|e| (e.get("kind").str().into(), e.get("definition").str().into(), e.get("id").str().into()))
            .collect();
        assert_eq!(got, exp_rows, "{}", occ.get("name").str());
        // The free derivation agrees with the index.
        for f in body.faces() {
            let ent = body.lookup(f.id()).unwrap();
            assert_eq!(index.entity_id(ix, ent).unwrap(), inst.entity_id(f.id()));
            assert_eq!(inst.entity_id(f.id()), InstanceEntityId::derive(&path, f.id()));
        }
    }
    assert_eq!(uuid_text(fid_geo_cad::ASSY_NAMESPACE), want.get("namespace").str());
}

#[test]
fn ac2_input_order_independent() {
    let fx = fixture();
    let body = bolt();
    let fwd = instances(&fx, &body);
    let mut rev = fwd.clone();
    rev.reverse();
    let a = InstanceIndex::build(fwd, &[&body]).unwrap();
    let b = InstanceIndex::build(rev, &[&body]).unwrap();
    for occ in fx.get("occurrences").arr() {
        let p = path_of(occ);
        let (ia, ib) = (a.find(&p).unwrap(), b.find(&p).unwrap());
        assert_eq!(rows(&a, ia, &body), rows(&b, ib, &body));
        assert_eq!(a.instance(ia).unwrap().id(), b.instance(ib).unwrap().id());
    }
    assert_eq!(a, b);
}

#[test]
fn ac3_seeded_round_trips() {
    let body = bolt();
    // 100 placements of the bolt under one sub-assembly, supplied in a scrambled order.
    let asm = TopologyId::Imported(Label::new("#510").unwrap());
    let mut insts: Vec<Instance> = (0..100u32)
        .map(|i| {
            let occ = if i % 2 == 0 {
                TopologyId::Imported(Label::new(&format!("#{}", 7000 + (i * 37) % 100)).unwrap())
            } else {
                let mut b = [0u8; 16];
                b[..4].copy_from_slice(&((i * 37) % 100).to_le_bytes());
                TopologyId::Derived(b)
            };
            let mut fb = [0u8; 16];
            fb[0] = 0xf1;
            fb[12..].copy_from_slice(&i.to_be_bytes());
            Instance::new(*body.id(), FrameId::from_bytes(fb), InstancePath::new(vec![asm.clone(), occ]).unwrap())
        })
        .collect();
    insts.rotate_left(41);
    let index = InstanceIndex::build(insts, &[&body]).unwrap();
    let ents: Vec<EntityIx> = body
        .ids()
        .values()
        .copied()
        .filter(|e| matches!(e, EntityIx::Face(_) | EntityIx::Edge(_) | EntityIx::Vertex(_)))
        .collect();
    assert_eq!(ents.len(), 15);
    assert_eq!(index.entity_count(), 100 * 15);

    let mut rng = Cursor::new(Address { seed: 5, stream: 0 }, 0);
    for _ in 0..10_000 {
        let n = index.instances().len() as u64;
        let ix = index.find(index.instances()[rng.below(n).unwrap() as usize].path()).unwrap();
        let ent = ents[rng.below(ents.len() as u64).unwrap() as usize];
        // Forward then back, and back then forward.
        let id = index.entity_id(ix, ent).unwrap();
        assert_eq!(index.resolve(&id).unwrap(), (ix, ent));
        assert_eq!(index.entity_id(ix, ent).unwrap(), id);
        // A random 16-byte ID is not in the index.
        let mut junk = [0u8; 16];
        junk[..8].copy_from_slice(&rng.next_u64().to_le_bytes());
        junk[8..].copy_from_slice(&rng.next_u64().to_le_bytes());
        assert_eq!(index.resolve(&InstanceEntityId::from_bytes(junk)), Err(AssyModelError::UnknownEntity));
    }
    // Every entry round-trips too.
    for (id, ix, ent) in index.entities() {
        assert_eq!(index.entity_id(ix, ent).unwrap(), id);
        assert_eq!(index.resolve(&id).unwrap(), (ix, ent));
    }
}

#[test]
fn ac4_unknown_frame() {
    let fx = fixture();
    let body = bolt();
    let index = InstanceIndex::build(instances(&fx, &body), &[&body]).unwrap();
    let t = tree(&fx, Some("bolt-2"));
    let p1 = path_of(&fx.get("occurrences").arr()[0]);
    let p2 = path_of(&fx.get("occurrences").arr()[1]);
    let i1 = index.instance(index.find(&p1).unwrap()).unwrap();
    let i2 = index.instance(index.find(&p2).unwrap()).unwrap();
    assert!(InstanceView::new(i1, &body, &t).is_ok());
    let err = InstanceView::new(i2, &body, &t).unwrap_err();
    assert_eq!(err, AssyModelError::UnknownFrame { frame: i2.frame() });
    assert_eq!(err.code(), "UnknownFrame");
}

#[test]
fn view_hands_out_definition_geometry_and_frame() {
    let fx = fixture();
    let body = bolt();
    let index = InstanceIndex::build(instances(&fx, &body), &[&body]).unwrap();
    let t = tree(&fx, None);
    for inst in index.instances() {
        let v = InstanceView::new(inst, &body, &t).unwrap();
        assert_eq!(v.frame(), inst.frame());
        // The very same body, not a copy.
        assert!(std::ptr::eq(v.definition(), &body));
        for (i, f) in body.faces().iter().enumerate() {
            let EntityIx::Face(fx_ix) = body.lookup(f.id()).unwrap() else { panic!() };
            assert_eq!(fx_ix.index(), i);
            assert!(std::ptr::eq(v.face(fx_ix).unwrap(), f));
        }
        for e in body.edges() {
            let EntityIx::Edge(ix) = body.lookup(e.id()).unwrap() else { panic!() };
            assert!(std::ptr::eq(v.edge(ix).unwrap(), e));
        }
        for x in body.vertices() {
            let EntityIx::Vertex(ix) = body.lookup(x.id()).unwrap() else { panic!() };
            assert!(std::ptr::eq(v.vertex(ix).unwrap(), x));
            assert_eq!(v.entity_id(x.id()), inst.entity_id(x.id()));
        }
    }
    // A view over another body is refused.
    let mut script = fx.get("body").clone();
    if let J::Obj(m) = &mut script {
        m.insert("body_id".into(), J::Str("b0170000000000000000000000000002".into()));
    }
    let other_body = builder(&script).build().unwrap();
    let inst = &index.instances()[0];
    assert_eq!(
        InstanceView::new(inst, &other_body, &t).unwrap_err(),
        AssyModelError::UnknownDefinition { body: *inst.definition() }
    );
}

#[test]
fn unknown_entity_lookups() {
    let fx = fixture();
    let body = bolt();
    let index = InstanceIndex::build(instances(&fx, &body), &[&body]).unwrap();
    let ix = index.find(&path_of(&fx.get("occurrences").arr()[0])).unwrap();
    // Shells, loops and coedges have no instance entity ID.
    for (tid, ent) in body.ids() {
        let exposed = matches!(ent, EntityIx::Face(_) | EntityIx::Edge(_) | EntityIx::Vertex(_));
        assert_eq!(index.entity_id(ix, *ent).is_ok(), exposed, "{tid}");
    }
    // Instance indexes are dense in path order; one past the end is unknown.
    let ixs: Vec<usize> = index.instances().iter().map(|i| index.find(i.path()).unwrap().index()).collect();
    assert_eq!(ixs, vec![0, 1]);
    let face = body.lookup(body.faces()[0].id()).unwrap();
    let empty = InstanceIndex::build(Vec::new(), &[]).unwrap();
    assert_eq!(empty.entity_count(), 0);
    assert_eq!(empty.entity_id(ix, face), Err(AssyModelError::UnknownEntity));
    // The ID of a face in a path that is not indexed is unknown.
    let stray = InstancePath::new(vec![TopologyId::Imported(Label::new("#999").unwrap())]).unwrap();
    assert!(index.find(&stray).is_none());
    assert_eq!(
        index.resolve(&InstanceEntityId::derive(&stray, body.faces()[0].id())),
        Err(AssyModelError::UnknownEntity)
    );
}

#[test]
fn build_errors() {
    let fx = fixture();
    let body = bolt();
    let insts = instances(&fx, &body);
    // Same path twice.
    let mut dup = insts.clone();
    dup.push(insts[0].clone());
    match InstanceIndex::build(dup, &[&body]).unwrap_err() {
        AssyModelError::DuplicatePath { path } => assert_eq!(&path, insts[0].path()),
        e => panic!("{e}"),
    }
    // Definition not supplied.
    assert_eq!(
        InstanceIndex::build(insts.clone(), &[]).unwrap_err(),
        AssyModelError::UnknownDefinition { body: *body.id() }
    );
    // The same body twice.
    assert_eq!(
        InstanceIndex::build(insts.clone(), &[&body, &body]).unwrap_err(),
        AssyModelError::DuplicateBody { body: *body.id() }
    );
}
