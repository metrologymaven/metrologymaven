//! Acceptance criteria of spec node GEO.FRAME.TREE (AC-1 to AC-5) plus requirement and
//! invariant checks. Expected bits and digests come from `fixtures/geo/frame/tree_7.json`,
//! written by the independent Python reference `tools/fixtures/geo_tree_cases.py` (stdlib;
//! bit-exact float model of the pinned resolution order, `hashlib` for the digest layout);
//! none come from this crate. The error-path expectations are hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_frame::tree::{Frame, FrameError, FrameId, FrameKind, FrameTree, MAX_DEPTH, MAX_NAME_BYTES};
use fid_geo_frame::xform::{Matrix3, Rigid, Similarity, Transform, XformError};
use fid_num::rng::{Address, Cursor};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/frame/");

fn records(file: &str) -> Vec<String> {
    let text = std::fs::read_to_string(format!("{ROOT}{file}")).unwrap();
    text.lines().map(str::trim).filter(|l| l.starts_with("{\"")).map(str::to_owned).collect()
}

/// The value of `"key":...` (string values never contain quotes).
fn field<'a>(line: &'a str, key: &str) -> &'a str {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat).unwrap_or_else(|| panic!("{key} missing in {line}")) + pat.len();
    let rest = &line[start..];
    if let Some(r) = rest.strip_prefix('"') {
        &r[..r.find('"').unwrap()]
    } else {
        &rest[..rest.find([',', '}']).unwrap()]
    }
}

fn hexes(s: &str) -> Vec<f64> {
    s.split(' ').map(|h| f64::from_bits(u64::from_str_radix(h, 16).unwrap())).collect()
}

fn matrix(v: &[f64]) -> Matrix3 {
    [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]]
}

fn transform(line: &str, kind_key: &str) -> Transform {
    let r = matrix(&hexes(field(line, "r")));
    let t = hexes(field(line, "t"));
    let rigid = Rigid::new(r, [t[0], t[1], t[2]]).unwrap();
    match field(line, kind_key) {
        "rigid" => Transform::Rigid(rigid),
        "similarity" => Transform::Similarity(Similarity::new(rigid, hexes(field(line, "s"))[0]).unwrap()),
        k => panic!("kind {k}"),
    }
}

fn bits(t: &Transform) -> Vec<u64> {
    let r = t.rotation();
    r.iter().flatten().chain(t.translation().iter()).chain([t.scale()].iter()).map(|v| v.to_bits()).collect()
}

fn kind(s: &str) -> FrameKind {
    match s {
        "world" => FrameKind::World,
        "part" => FrameKind::Part,
        "instrument" => FrameKind::Instrument,
        "alignment" => FrameKind::Alignment,
        "user" => FrameKind::User,
        k => panic!("frame kind {k}"),
    }
}

struct Tree7 {
    tree: FrameTree,
    names: Vec<(String, FrameId)>,
}

impl Tree7 {
    fn id(&self, name: &str) -> FrameId {
        self.names.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("{name}")).1
    }
}

/// Builds tree_7 by inserting the fixture's frames in fixture order.
fn tree7() -> Tree7 {
    let mut tree = FrameTree::new();
    let mut names = Vec::new();
    for line in records("tree_7.json").iter().filter(|l| field(l, "rec") == "frame") {
        let name = field(line, "name").to_owned();
        let id = FrameId::parse(field(line, "id")).unwrap();
        if field(line, "frame_kind") == "world" {
            assert_eq!(id, tree.world());
            assert_eq!(transform(line, "xkind"), Transform::IDENTITY);
        } else {
            let parent = names.iter().find(|(n, _): &&(String, FrameId)| n == field(line, "parent")).unwrap().1;
            tree.insert(Frame {
                id,
                name: name.clone(),
                kind: kind(field(line, "frame_kind")),
                parent: Some(parent),
                to_parent: transform(line, "xkind"),
            })
            .unwrap();
        }
        names.push((name, id));
    }
    assert_eq!(tree.len(), 7);
    Tree7 { tree, names }
}

fn tree_digest() -> String {
    let recs = records("tree_7.json");
    let line = recs.iter().find(|l| field(l, "rec") == "tree").unwrap();
    field(line, "digest").to_owned()
}

#[test]
fn ac1_all_49_pairs_bit_exact() {
    let t = tree7();
    let mut n = 0;
    for line in records("tree_7.json").iter().filter(|l| field(l, "rec") == "pair") {
        let (a, b) = (field(line, "a"), field(line, "b"));
        let got = t.tree.resolve(t.id(a), t.id(b)).unwrap();
        let want = transform(line, "xkind");
        assert_eq!(got.is_rigid(), want.is_rigid(), "{a} -> {b}");
        assert_eq!(bits(&got), bits(&want), "{a} -> {b}");
        n += 1;
    }
    assert_eq!(n, 49);
}

#[test]
fn ac2_cycle_rejected_with_path_and_digest_unchanged() {
    let mut t = tree7();
    let before = t.tree.digest();
    assert_eq!(before.as_str(), tree_digest());
    let (part_a, align_1) = (t.id("part_a"), t.id("align_1"));
    let err = t.tree.reparent(part_a, align_1, Transform::IDENTITY).unwrap_err();
    assert_eq!(err, FrameError::Cycle { frames: vec![part_a, align_1] });
    assert_eq!(t.tree.digest(), before);
    // A deeper descendant reports the whole would-be cycle, frame first.
    let (align_2, inst_1) = (t.id("align_2"), t.id("inst_1"));
    let err = t.tree.reparent(part_a, inst_1, Transform::IDENTITY).unwrap_err();
    assert_eq!(err, FrameError::Cycle { frames: vec![part_a, inst_1, align_2, align_1] });
    // Self-parent.
    let err = t.tree.reparent(part_a, part_a, Transform::IDENTITY).unwrap_err();
    assert_eq!(err, FrameError::Cycle { frames: vec![part_a] });
    assert_eq!(t.tree.digest(), before);
}

#[test]
fn ac3_remove_with_children_fails() {
    let mut t = tree7();
    let before = t.tree.digest();
    let part_a = t.id("part_a");
    assert_eq!(t.tree.remove(part_a).unwrap_err(), FrameError::HasChildren { id: part_a });
    assert_eq!(t.tree.digest(), before);
    // A leaf can be removed.
    let inst_1 = t.id("inst_1");
    let f = t.tree.remove(inst_1).unwrap();
    assert_eq!(f.id, inst_1);
    assert!(t.tree.get(inst_1).is_none());
    assert_eq!(t.tree.find_by_name("inst_1"), None);
}

#[test]
fn ac4_round_trip_within_1e12() {
    let t = tree7();
    let ids: Vec<FrameId> = t.names.iter().map(|(_, id)| *id).collect();
    let mut pairs = Vec::new();
    for a in &ids {
        for b in &ids {
            let fwd = t.tree.resolve(*a, *b).unwrap();
            let back = t.tree.resolve(*b, *a).unwrap();
            pairs.push((fwd, back));
        }
    }
    let mut cur = Cursor::new(Address { seed: 17, stream: 4 }, 0);
    let mut worst = 0.0f64;
    for k in 0..100_000usize {
        let x = [0, 1, 2].map(|_| cur.uniform_f64() * 200.0 - 100.0);
        let (fwd, back) = &pairs[k % pairs.len()];
        let y = back.apply_point(&fwd.apply_point(&x).unwrap()).unwrap();
        let e = ((y[0] - x[0]).powi(2) + (y[1] - x[1]).powi(2) + (y[2] - x[2]).powi(2)).sqrt();
        if e > worst {
            worst = e;
        }
    }
    assert!(worst <= 1e-12, "worst round trip {worst:e} m");
}

#[test]
fn ac5_digest_after_edits_matches_reference() {
    let mut t = tree7();
    assert_eq!(t.tree.digest().as_str(), tree_digest());
    let mut steps = 0;
    for line in records("tree_7.json").iter().filter(|l| field(l, "rec") == "edit") {
        let frame = field(line, "frame");
        match field(line, "op") {
            "set_transform" => t.tree.set_transform(t.id(frame), transform(line, "xkind")).unwrap(),
            "reparent" => {
                t.tree.reparent(t.id(frame), t.id(field(line, "parent")), transform(line, "xkind")).unwrap();
            }
            "insert" => {
                let id = FrameId::parse(field(line, "id")).unwrap();
                t.tree
                    .insert(Frame {
                        id,
                        name: frame.to_owned(),
                        kind: kind(field(line, "frame_kind")),
                        parent: Some(t.id(field(line, "parent"))),
                        to_parent: transform(line, "xkind"),
                    })
                    .unwrap();
                t.names.push((frame.to_owned(), id));
            }
            "remove" => {
                t.tree.remove(t.id(frame)).unwrap();
            }
            op => panic!("op {op}"),
        }
        assert_eq!(t.tree.digest().as_str(), field(line, "digest"), "after {} {frame}", field(line, "op"));
        steps += 1;
    }
    assert_eq!(steps, 5);
}

#[test]
fn resolve_identity_exact_and_unknown_frames() {
    let t = tree7();
    for (_, id) in &t.names {
        assert_eq!(t.tree.resolve(*id, *id).unwrap(), Transform::IDENTITY);
    }
    let ghost = FrameId::from_bytes([0xee; 16]);
    let w = t.tree.world();
    assert_eq!(t.tree.resolve(ghost, w).unwrap_err(), FrameError::UnknownFrame { id: ghost });
    assert_eq!(t.tree.resolve(w, ghost).unwrap_err(), FrameError::UnknownFrame { id: ghost });
    assert_eq!(t.tree.resolve(ghost, ghost).unwrap_err(), FrameError::UnknownFrame { id: ghost });
}

#[test]
fn world_is_single_and_immutable() {
    let mut t = tree7();
    let w = t.tree.world();
    assert_eq!(w, FrameId::WORLD);
    let wf = t.tree.get(w).unwrap();
    assert_eq!((wf.kind, wf.parent, wf.name.as_str()), (FrameKind::World, None, "world"));
    assert_eq!(t.tree.reparent(w, t.id("part_a"), Transform::IDENTITY).unwrap_err(), FrameError::WorldImmutable);
    assert_eq!(t.tree.set_transform(w, Transform::IDENTITY).unwrap_err(), FrameError::WorldImmutable);
    assert_eq!(t.tree.remove(w).unwrap_err(), FrameError::WorldImmutable);
    let id = FrameId::from_bytes([9; 16]);
    let second = Frame { id, name: "w2".into(), kind: FrameKind::World, parent: None, to_parent: Transform::IDENTITY };
    assert_eq!(t.tree.insert(second).unwrap_err(), FrameError::SecondWorld { id });
}

fn user(id: u8, name: &str, parent: FrameId) -> Frame {
    Frame {
        id: FrameId::from_bytes([id; 16]),
        name: name.to_owned(),
        kind: FrameKind::User,
        parent: Some(parent),
        to_parent: Transform::IDENTITY,
    }
}

#[test]
fn insert_errors_leave_tree_unchanged() {
    let mut t = tree7();
    let before = t.tree.digest();
    let part_a = t.id("part_a");
    // Duplicate ID, also when written with upper-case hex.
    let upper = FrameId::parse("5F1E0C3A9B2D4E6F8A7B6C5D4E3F2A10").unwrap();
    assert_eq!(upper, part_a);
    let mut dup = user(1, "fresh", part_a);
    dup.id = upper;
    assert_eq!(t.tree.insert(dup).unwrap_err(), FrameError::DuplicateId { id: part_a });
    // Case-only duplicate name.
    assert_eq!(
        t.tree.insert(user(1, "PART_A", part_a)).unwrap_err(),
        FrameError::DuplicateName { name: "PART_A".into(), existing: part_a }
    );
    // Dangling parent.
    let ghost = FrameId::from_bytes([0xee; 16]);
    assert_eq!(t.tree.insert(user(1, "u", ghost)).unwrap_err(), FrameError::UnknownFrame { id: ghost });
    // Missing parent.
    let mut orphan = user(1, "u", part_a);
    orphan.parent = None;
    assert_eq!(t.tree.insert(orphan).unwrap_err(), FrameError::NoParent { id: FrameId::from_bytes([1; 16]) });
    // Self parent.
    let me = FrameId::from_bytes([1; 16]);
    assert_eq!(t.tree.insert(user(1, "u", me)).unwrap_err(), FrameError::Cycle { frames: vec![me] });
    // Names.
    for bad in ["", "a\u{7}b", &"x".repeat(MAX_NAME_BYTES + 1)] {
        assert_eq!(t.tree.insert(user(1, bad, part_a)).unwrap_err(), FrameError::BadName);
    }
    assert!(t.tree.insert(user(1, &"x".repeat(MAX_NAME_BYTES), part_a)).is_ok());
    t.tree.remove(me).unwrap();
    assert_eq!(t.tree.digest(), before);
}

#[test]
fn bad_ids_rejected() {
    for bad in ["", "00", "g0000000000000000000000000000000", "0000000000000000000000000000000000"] {
        assert_eq!(FrameId::parse(bad).unwrap_err(), FrameError::BadId);
    }
    let id = FrameId::parse("00112233445566778899AaBbCcDdEeFf").unwrap();
    assert_eq!(id.to_string(), "00112233445566778899aabbccddeeff");
}

fn worst(name: &str) -> Transform {
    let recs = records("xform_worst_residual.json");
    let l = recs.iter().find(|l| field(l, "name") == name).unwrap();
    let r = matrix(&hexes(field(l, "r")));
    let t = hexes(field(l, "t"));
    Transform::Rigid(Rigid::new(r, [t[0], t[1], t[2]]).unwrap())
}

/// Composition does not re-check orthonormality (XFORM), so a chain of accepted transforms
/// can drift past the tolerance; the tree re-checks every transform on entry and refuses it
/// with a typed error, leaving the tree unchanged.
#[test]
fn transforms_rechecked_on_entry() {
    let mut t = tree7();
    let before = t.tree.digest();
    let a = t.id("align_2");
    let stretch = worst("stretch_x");
    let drifted = stretch.compose(&stretch).unwrap();
    match t.tree.set_transform(a, drifted).unwrap_err() {
        FrameError::BadTransform { id, error: XformError::NotOrthonormal { residual } } => {
            assert_eq!(id, a);
            assert!(residual > 1e-12);
        }
        e => panic!("{e:?}"),
    }
    assert!(matches!(
        t.tree.insert(Frame { to_parent: drifted, ..user(0x42, "drift", a) }).unwrap_err(),
        FrameError::BadTransform { .. }
    ));
    assert!(matches!(
        t.tree.reparent(t.id("inst_1"), t.id("part_b"), drifted).unwrap_err(),
        FrameError::BadTransform { .. }
    ));
    assert_eq!(t.tree.digest(), before);
    // A long composition of near-exact rotations stays within tolerance and is accepted.
    let mut acc = t.tree.resolve(t.id("inst_1"), t.tree.world()).unwrap();
    let step = t.tree.resolve(t.id("align_1"), t.id("part_a")).unwrap();
    for _ in 0..20 {
        acc = acc.compose(&step).unwrap();
    }
    assert!(t.tree.set_transform(a, acc).is_ok());
}

#[test]
fn error_codes_display() {
    let w = FrameId::WORLD;
    assert_eq!(
        FrameError::Handedness { id: w, det: -1.0 }.to_string(),
        "handedness 00000000000000000000000000000000 (det -1)"
    );
    assert_eq!(FrameError::HasChildren { id: w }.to_string(), "has_children 00000000000000000000000000000000");
    assert_eq!(FrameError::Cycle { frames: vec![w, w] }.to_string(), format!("cycle [{w}, {w}]"));
}

#[test]
fn depth_is_bounded() {
    let mut tree = FrameTree::new();
    let mut parent = FrameId::WORLD;
    for d in 1..=MAX_DEPTH {
        let mut b = [0u8; 16];
        b[0] = 0x80;
        b[14] = (d >> 8) as u8;
        b[15] = d as u8;
        let id = FrameId::from_bytes(b);
        tree.insert(Frame {
            id,
            name: format!("f{d}"),
            kind: FrameKind::User,
            parent: Some(parent),
            to_parent: Transform::IDENTITY,
        })
        .unwrap();
        parent = id;
    }
    assert_eq!(tree.depth(parent).unwrap(), MAX_DEPTH);
    let deep = FrameId::from_bytes([0x7f; 16]);
    let err = tree
        .insert(Frame {
            id: deep,
            name: "deep".into(),
            kind: FrameKind::User,
            parent: Some(parent),
            to_parent: Transform::IDENTITY,
        })
        .unwrap_err();
    assert_eq!(err, FrameError::TooDeep { id: deep });
    // Reparenting a subtree of height 1 under depth MAX_DEPTH - 1 overflows too.
    let mut b = [0u8; 16];
    b[0] = 0x80;
    b[15] = 1;
    let first = FrameId::from_bytes(b);
    let x = FrameId::from_bytes([0x10; 16]);
    let y = FrameId::from_bytes([0x11; 16]);
    tree.insert(Frame {
        id: x,
        name: "x".into(),
        kind: FrameKind::User,
        parent: Some(FrameId::WORLD),
        to_parent: Transform::IDENTITY,
    })
    .unwrap();
    tree.insert(Frame {
        id: y,
        name: "y".into(),
        kind: FrameKind::User,
        parent: Some(x),
        to_parent: Transform::IDENTITY,
    })
    .unwrap();
    let before = tree.digest();
    assert_eq!(tree.reparent(x, parent, Transform::IDENTITY).unwrap_err(), FrameError::TooDeep { id: x });
    assert_eq!(tree.digest(), before);
    // Resolution across the full depth works and is exact for identities.
    assert_eq!(tree.resolve(parent, first).unwrap(), Transform::IDENTITY);
    assert_eq!(tree.ancestors(parent).unwrap().len(), MAX_DEPTH + 1);
}

#[test]
fn from_frames_checks_every_invariant() {
    let t = tree7();
    let frames: Vec<Frame> = t.tree.frames().cloned().collect();
    // Any order rebuilds the same tree.
    let mut rev = frames.clone();
    rev.reverse();
    let rebuilt = FrameTree::from_frames(rev).unwrap();
    assert_eq!(rebuilt.digest(), t.tree.digest());
    assert_eq!(rebuilt, t.tree);

    let part_a = t.id("part_a");
    let align_1 = t.id("align_1");
    let with = |edit: &dyn Fn(&mut Vec<Frame>)| {
        let mut v = frames.clone();
        edit(&mut v);
        FrameTree::from_frames(v).unwrap_err()
    };
    // Dangling parent.
    let ghost = FrameId::from_bytes([0xee; 16]);
    assert_eq!(
        with(&|v| v.iter_mut().find(|f| f.id == part_a).unwrap().parent = Some(ghost)),
        FrameError::UnknownFrame { id: ghost }
    );
    // Cycle part_a <-> align_1 (detached from the world). The walk in ID order reaches it
    // from align_2 (ID 3c..), so the cycle is reported from align_1, its first frame reached.
    assert_eq!(
        with(&|v| v.iter_mut().find(|f| f.id == part_a).unwrap().parent = Some(align_1)),
        FrameError::Cycle { frames: vec![align_1, part_a] }
    );
    // Duplicate ID and case-only duplicate name.
    assert_eq!(
        with(&|v| {
            let mut d = v[1].clone();
            d.name = "other".into();
            v.push(d)
        }),
        FrameError::DuplicateId { id: frames[1].id }
    );
    assert_eq!(
        with(&|v| v.push(user(0x42, "Part_A", part_a))),
        FrameError::DuplicateName { name: "Part_A".into(), existing: part_a }
    );
    // World count and world shape.
    assert_eq!(with(&|v| v.retain(|f| f.kind != FrameKind::World)), FrameError::WorldCount { found: 0 });
    assert_eq!(
        with(&|v| v.push(Frame {
            id: FrameId::from_bytes([0x42; 16]),
            name: "world2".into(),
            kind: FrameKind::World,
            parent: None,
            to_parent: Transform::IDENTITY
        })),
        FrameError::WorldCount { found: 2 }
    );
    assert_eq!(
        with(&|v| v.iter_mut().find(|f| f.kind == FrameKind::World).unwrap().parent = Some(part_a)),
        FrameError::BadWorld
    );
    assert_eq!(
        with(&|v| v.iter_mut().find(|f| f.id == part_a).unwrap().parent = None),
        FrameError::NoParent { id: part_a }
    );
}

#[test]
fn path_bound_holds_on_worst_residual_edges() {
    // Rotations at the largest residual XFORM accepts (inputs of xform_worst_residual.json).
    let mut tree = FrameTree::new();
    let spec = [
        (0x11u8, "w1", 0u8, "stretch_x"),
        (0x12, "w2", 0x11, "shear_xy"),
        (0x13, "w3", 0x12, "rotated_stretch"),
        (0x14, "w4", 0x13, "stretch_x_translated"),
        (0x15, "w5", 0, "rotated_stretch"),
        (0x16, "w6", 0x15, "stretch_x_translated"),
    ];
    let mut ids = vec![FrameId::WORLD];
    for (id, name, parent, src) in spec {
        let mut b = [0u8; 16];
        b[15] = id;
        let mut p = [0u8; 16];
        p[15] = parent;
        tree.insert(Frame {
            id: FrameId::from_bytes(b),
            name: name.into(),
            kind: FrameKind::User,
            parent: Some(FrameId::from_bytes(p)),
            to_parent: worst(src),
        })
        .unwrap();
        ids.push(FrameId::from_bytes(b));
    }
    let mut cur = Cursor::new(Address { seed: 17, stream: 5 }, 0);
    for a in &ids {
        for b in &ids {
            let fwd = tree.resolve(*a, *b).unwrap();
            let back = tree.resolve(*b, *a).unwrap();
            for _ in 0..200 {
                let x = [0, 1, 2].map(|_| cur.uniform_f64() * 200.0 - 100.0);
                let y = back.apply_point(&fwd.apply_point(&x).unwrap()).unwrap();
                let e = ((y[0] - x[0]).powi(2) + (y[1] - x[1]).powi(2) + (y[2] - x[2]).powi(2)).sqrt();
                let bound = tree.path_round_trip_bound(*a, *b, &x).unwrap().unwrap();
                assert!(e <= bound, "{a} -> {b}: {e:e} > {bound:e}");
            }
        }
    }
    // A similarity edge has no stated bound.
    let t = tree7();
    assert_eq!(t.tree.path_round_trip_bound(t.id("inst_2"), t.tree.world(), &[1.0, 2.0, 3.0]).unwrap(), None);
    assert_eq!(t.tree.path_round_trip_bound(t.id("inst_1"), t.id("inst_1"), &[1.0, 2.0, 3.0]).unwrap(), Some(0.0));
}
