//! Acceptance criteria AC-1 .. AC-5 of spec leaf VIZ.RENDER.SCENE.PROJ, plus its error paths.
//! Expected values come from `fixtures/viz/scene-doc-small` (written by its independent Python
//! script, which implements the fixture's projectors and the pinned layout separately) or are
//! hand-labelled from the spec's rules; none is produced by running this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_frame::{Frame, FrameId, FrameKind, FrameTree, Transform};
use fid_geo_types::Aabb;
use fid_kdec::{NotDecidableReason, Outcome as KernelOutcome};
use fid_render::scene::idtab::{DerivedId, EntityId, IdError, Identity, RenderId, ViewOp};
use fid_render::scene::proj::{
    derived_digest, ChangeBatch, Diagnostic, DisplayStatus, Document, ElementKind, ElementView, KernelStatus, NodeDraw,
    NodeKind, NodeSink, ProjError, Projector, ProjectorRegistry, SceneError, SceneGraph, StatusCode, StyleToken,
};
use fid_types::Digest;
use std::path::PathBuf;

const LABEL_DZ: f64 = 0.005;

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/viz/scene-doc-small").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn uuid(text: &str) -> EntityId {
    EntityId::parse(text).unwrap_or_else(|_| panic!("bad uuid {text}"))
}

fn rid(n: u32) -> RenderId {
    RenderId::new(n).unwrap()
}

fn style(s: &str) -> StyleToken {
    StyleToken::new(s).unwrap()
}

/// Parses a document file (format in make_fixture.py).
fn parse_doc(text: &str) -> Vec<ElementView> {
    text.lines()
        .map(|line| {
            let f: Vec<&str> = line.split(' ').collect();
            assert_eq!(f.len(), 14, "bad line {line}");
            let n: Vec<f64> = f[5..].iter().map(|x| x.parse().unwrap()).collect();
            ElementView {
                entity: uuid(f[0]),
                kind: ElementKind::new(f[1]).unwrap(),
                frame: FrameId::parse(f[2]).unwrap(),
                status: DisplayStatus::parse(f[3]).unwrap(),
                data_digest: Digest::parse(f[4]).unwrap(),
                origin_m: [n[0], n[1], n[2]],
                bounds_m: Aabb::new([n[3], n[4], n[5]], [n[6], n[7], n[8]]).unwrap(),
            }
        })
        .collect()
}

fn doc(name: &str) -> Document {
    Document::new(parse_doc(&fixture(name))).unwrap()
}

fn dirty() -> Vec<EntityId> {
    fixture("dirty.txt").lines().map(uuid).collect()
}

fn parse_batch(text: &str) -> ChangeBatch {
    let mut b = ChangeBatch::default();
    for line in text.lines() {
        let mut f = line.split(' ');
        let name = f.next().unwrap();
        let ids: Vec<RenderId> = f.map(|x| rid(x.parse().unwrap())).collect();
        match name {
            "added" => b.added = ids,
            "updated" => b.updated = ids,
            "retired" => b.retired = ids,
            _ => panic!("bad batch line {line}"),
        }
    }
    b
}

/// The fixture's geometry projector: one primary node with the element's origin and bounds.
struct Geom(&'static str);

impl Projector for Geom {
    fn kind(&self) -> ElementKind {
        ElementKind::new(self.0).unwrap()
    }
    fn project(&self, el: &ElementView, out: &mut NodeSink<'_>) -> Result<(), ProjError> {
        out.primary(NodeDraw {
            origin_m: el.origin_m,
            bounds_m: el.bounds_m,
            style: style(&format!("geom.{}", self.0)),
        })
    }
}

/// The fixture's characteristic projector: primary node, then a label 5 mm above the origin,
/// then a leader from the origin to the label.
struct Characteristic;

impl Projector for Characteristic {
    fn kind(&self) -> ElementKind {
        ElementKind::new("characteristic").unwrap()
    }
    fn project(&self, el: &ElementView, out: &mut NodeSink<'_>) -> Result<(), ProjError> {
        let o = el.origin_m;
        let p = [o[0], o[1], o[2] + LABEL_DZ];
        out.primary(NodeDraw { origin_m: o, bounds_m: el.bounds_m, style: style("char") })?;
        out.derived(
            ViewOp::Label,
            NodeDraw { origin_m: p, bounds_m: Aabb::new(p, p).unwrap(), style: style("char.label") },
        )?;
        let lo = [0, 1, 2].map(|k| if p[k] < o[k] { p[k] } else { o[k] });
        let hi = [0, 1, 2].map(|k| if p[k] > o[k] { p[k] } else { o[k] });
        out.derived(
            ViewOp::Leader,
            NodeDraw { origin_m: o, bounds_m: Aabb::new(lo, hi).unwrap(), style: style("char.leader") },
        )
    }
}

fn registry() -> ProjectorRegistry {
    let mut r = ProjectorRegistry::new();
    for k in ["part", "mesh", "point_set"] {
        r.register(Box::new(Geom(k))).unwrap();
    }
    r.register(Box::new(Characteristic)).unwrap();
    r
}

fn full() -> SceneGraph {
    let (g, out) = SceneGraph::project(&registry(), &doc("doc.txt")).unwrap();
    assert!(out.diagnostics.is_empty());
    assert_eq!(out.batch.added.len(), g.len());
    g
}

#[test]
fn ac1_full_projection_matches_expectation() {
    let g = full();
    assert_eq!(g.len(), 4 + 12 * 3);
    assert_eq!(g.serialise().unwrap(), fixture("expect-full.txt"));
    assert_eq!(g.digest().unwrap().as_str(), fixture("expect-full.digest").trim_end());
    let labels = g.nodes().filter(|n| n.kind == NodeKind::Derived(ViewOp::Label)).count();
    let leaders = g.nodes().filter(|n| n.kind == NodeKind::Derived(ViewOp::Leader)).count();
    assert_eq!((labels, leaders), (12, 12));
    // REQ-001: every field present; frame and status are the element's.
    let d = doc("doc.txt");
    for n in g.nodes() {
        let el = d.get(n.identity.entity()).unwrap();
        assert_eq!(n.frame, el.frame);
        assert_eq!(n.status, el.status);
        assert_eq!(g.ids().render_id_of(n.identity), Some(n.id));
    }
}

#[test]
fn ac2_dirty_set_updates_only_the_changed_characteristic() {
    let mut g = full();
    let before: Vec<String> = g.serialise().unwrap().lines().map(str::to_owned).collect();
    let out = g.reproject(&registry(), &doc("doc-changed.txt"), &dirty()).unwrap();
    assert!(out.diagnostics.is_empty());
    let expected = parse_batch(&fixture("expect-batch.txt"));
    assert_eq!(out.batch, expected);
    // Hand-labelled: c05 is the 9th element; its primary, label and leader nodes are 17..19.
    let c05 = uuid("00000000-0000-4000-8000-00000000000f");
    assert_eq!(g.render_ids(c05), &[rid(17), rid(18), rid(19)]);
    assert_eq!(out.batch.updated, vec![rid(17), rid(18), rid(19)]);
    assert_eq!(g.serialise().unwrap(), fixture("expect-changed.txt"));
    assert_eq!(g.digest().unwrap().as_str(), fixture("expect-changed.digest").trim_end());
    // Every other node line is byte-identical.
    let after: Vec<String> = g.serialise().unwrap().lines().map(str::to_owned).collect();
    assert_eq!(before.len(), after.len());
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        let id = rid(u32::try_from(i + 1).unwrap());
        assert_eq!(a == b, !out.batch.updated.contains(&id), "node {id}");
    }
    // The derived nodes were regenerated from the new source digest.
    let el = doc("doc-changed.txt").get(c05).unwrap().clone();
    assert_eq!(g.get(rid(18)).unwrap().data_digest, derived_digest(&el.data_digest, ViewOp::Label));
    assert_eq!(g.get(rid(19)).unwrap().data_digest, derived_digest(&el.data_digest, ViewOp::Leader));
}

#[test]
fn ac3_two_projections_are_byte_identical() {
    let a = full();
    let b = full();
    assert_eq!(a.serialise().unwrap().into_bytes(), b.serialise().unwrap().into_bytes());
    assert_eq!(a.digest().unwrap(), b.digest().unwrap());
    assert_eq!(a, b);
}

#[test]
fn ac4_unregistered_kind_reports_one_no_projector() {
    let mut els = parse_doc(&fixture("doc.txt"));
    let mut extra = els[1].clone();
    extra.entity = uuid("00000000-0000-4000-8000-0000000000ff");
    extra.kind = ElementKind::new("annotation_note").unwrap();
    els.insert(2, extra.clone());
    let d = Document::new(els).unwrap();
    let (g, out) = SceneGraph::project(&registry(), &d).unwrap();
    assert_eq!(
        out.diagnostics,
        vec![Diagnostic::NoProjector { entity: extra.entity, kind: ElementKind::new("annotation_note").unwrap() }]
    );
    assert_eq!(out.diagnostics[0].code(), "NoProjector");
    assert!(out.diagnostics[0].to_string().contains("annotation_note"));
    // Projection completed: every other element is there, the skipped one has no node.
    assert_eq!(g.len(), 40);
    assert!(g.render_ids(extra.entity).is_empty());
    assert_eq!(g.serialise().unwrap(), fixture("expect-full.txt"));
}

#[test]
fn ac5_reopened_project_regenerates_every_derived_node() {
    let g = full();
    // "Save" the project: the stored state is the document, which has no slot for a derived
    // node. Reopen in a new session: parse it back and project into an empty graph.
    let saved: String = doc("doc.txt")
        .elements()
        .iter()
        .map(|e| {
            let mut s = format!("{} {} {} {} {}", e.entity, e.kind, e.frame, e.status, e.data_digest.as_str());
            for v in e.origin_m.iter().chain(e.bounds_m.min().iter()).chain(e.bounds_m.max().iter()) {
                s.push_str(&format!(" {v:?}"));
            }
            s + "\n"
        })
        .collect();
    let (reopened, _) = SceneGraph::project(&registry(), &Document::new(parse_doc(&saved)).unwrap()).unwrap();
    let derived: Vec<_> = g.nodes().filter(|n| n.is_derived()).collect();
    let again: Vec<_> = reopened.nodes().filter(|n| n.is_derived()).collect();
    assert_eq!(derived.len(), 24);
    assert_eq!(derived, again);
    for n in again {
        let Identity::Derived(DerivedId { source, op }) = n.identity else { panic!() };
        let src = reopened.get(reopened.ids().render_id_of(Identity::Entity(source)).unwrap()).unwrap();
        assert_eq!(n.data_digest, derived_digest(&src.data_digest, op));
    }
}

#[test]
fn derived_digest_layout() {
    // Hand-computed reference: sha256(b"FIDSCN" 0x01 || 32 zero bytes || 0x02).
    let mut msg = b"FIDSCN\x01".to_vec();
    msg.extend([0u8; 32]);
    msg.push(2);
    assert_eq!(derived_digest(&Digest::TOMBSTONE, ViewOp::Label), fid_kci::kci::sha256_digest(&msg));
    assert_ne!(derived_digest(&Digest::TOMBSTONE, ViewOp::Label), derived_digest(&Digest::TOMBSTONE, ViewOp::Leader));
}

#[test]
fn reproject_adds_and_retires() {
    let mut g = full();
    let mut els = parse_doc(&fixture("doc.txt"));
    let removed = els.remove(4).entity; // c01, render IDs 5..7
    let mut added = els[0].clone();
    added.entity = uuid("00000000-0000-4000-8000-0000000000aa");
    added.kind = ElementKind::new("characteristic").unwrap();
    els.push(added.clone());
    let d = Document::new(els).unwrap();
    // Dirty set lists a clean entity, a duplicate and an unknown entity too.
    let unknown = uuid("00000000-0000-4000-8000-0000000000bb");
    let clean = uuid("00000000-0000-4000-8000-000000000002");
    let out = g.reproject(&registry(), &d, &[added.entity, removed, removed, unknown, clean]).unwrap();
    assert_eq!(
        out.batch,
        ChangeBatch { added: vec![rid(41), rid(42), rid(43)], updated: vec![], retired: vec![rid(5), rid(6), rid(7)] }
    );
    assert!(g.get(rid(5)).is_none());
    assert!(g.render_ids(removed).is_empty());
    assert_eq!(g.len(), 40);
    // Retired IDs are tombstones and never reused.
    assert!(matches!(g.ids().identity(rid(5)), Ok(fid_render::scene::idtab::Slot::Retired(_))));
    // An empty dirty set changes nothing.
    let out = g.reproject(&registry(), &d, &[]).unwrap();
    assert!(out.batch.is_empty());
}

#[test]
fn kind_change_without_projector_retires_nodes() {
    let mut g = full();
    let mut els = parse_doc(&fixture("doc.txt"));
    els[4].kind = ElementKind::new("unknown_kind").unwrap();
    let e = els[4].entity;
    let out = g.reproject(&registry(), &Document::new(els).unwrap(), &[e]).unwrap();
    assert_eq!(out.batch.retired, vec![rid(5), rid(6), rid(7)]);
    assert_eq!(out.diagnostics.len(), 1);
    assert_eq!(out.diagnostics[0].code(), "NoProjector");
}

/// Projectors that misbehave in each way the sink checks.
struct Bad(&'static str, u8);

impl Projector for Bad {
    fn kind(&self) -> ElementKind {
        ElementKind::new(self.0).unwrap()
    }
    fn project(&self, el: &ElementView, out: &mut NodeSink<'_>) -> Result<(), ProjError> {
        let draw = NodeDraw { origin_m: el.origin_m, bounds_m: el.bounds_m, style: style("x") };
        match self.1 {
            0 => out.primary(NodeDraw { origin_m: [f64::NAN, 0.0, 0.0], ..draw }),
            1 => out.primary(NodeDraw { origin_m: [0.0, f64::INFINITY, 0.0], ..draw }),
            2 => {
                out.primary(draw.clone())?;
                out.primary(draw)
            }
            3 => {
                out.derived(ViewOp::Whisker, draw.clone())?;
                out.derived(ViewOp::Whisker, draw)
            }
            _ => Err(ProjError::Failed { reason: "no tessellation" }),
        }
    }
}

#[test]
fn projector_failure_draws_error_node_and_continues() {
    let cases = [
        (0, ProjError::NonFiniteOrigin),
        (1, ProjError::NonFiniteOrigin),
        (2, ProjError::DuplicatePrimary),
        (3, ProjError::DuplicateDerived(ViewOp::Whisker)),
        (4, ProjError::Failed { reason: "no tessellation" }),
    ];
    for (mode, expected) in cases {
        let mut r = ProjectorRegistry::new();
        r.register(Box::new(Bad("mesh", mode))).unwrap();
        for k in ["part", "point_set"] {
            r.register(Box::new(Geom(k))).unwrap();
        }
        r.register(Box::new(Characteristic)).unwrap();
        let d = doc("doc.txt");
        let (g, out) = SceneGraph::project(&r, &d).unwrap();
        let mesh = uuid("00000000-0000-4000-8000-000000000002");
        assert_eq!(
            out.diagnostics,
            vec![Diagnostic::ProjectorFailed {
                entity: mesh,
                kind: ElementKind::new("mesh").unwrap(),
                error: expected
            }]
        );
        // Exactly one node for the mesh, marked error, never dropped; nothing partial kept.
        assert_eq!(g.render_ids(mesh), &[rid(2)]);
        let n = g.get(rid(2)).unwrap();
        assert_eq!(n.status, d.get(mesh).unwrap().status);
        assert_eq!(n.proj_error, Some(expected));
        assert_eq!(n.style.as_str(), "error");
        assert_eq!(n.data_digest, d.get(mesh).unwrap().data_digest);
        assert_eq!(g.len(), 40);
    }
}

#[test]
fn document_and_registry_errors_are_typed() {
    let els = parse_doc(&fixture("doc.txt"));
    let mut dup = els.clone();
    dup.push(els[3].clone());
    assert_eq!(Document::new(dup), Err(SceneError::DuplicateEntity(els[3].entity)));
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut v = els.clone();
        v[2].origin_m[1] = bad;
        assert_eq!(Document::new(v), Err(SceneError::NonFiniteOrigin(els[2].entity)));
    }
    let mut r = registry();
    assert_eq!(
        r.register(Box::new(Geom("mesh"))),
        Err(SceneError::DuplicateProjector(ElementKind::new("mesh").unwrap()))
    );
    for bad in ["", "Mesh", "9a", "a b", "a-b"] {
        assert_eq!(ElementKind::new(bad), Err(SceneError::BadToken));
        assert_eq!(StyleToken::new(bad), Err(SceneError::BadToken));
    }
    assert_eq!(ElementKind::from_geo(fid_geo_types::ElementKind::PointSet).as_str(), "point_set");
}

#[test]
fn frames_are_checked_against_the_frame_tree() {
    let d = doc("doc.txt");
    let mut tree = FrameTree::new();
    let part = FrameId::parse("5c0e0000000000000000000000000001").unwrap();
    let missing = d.check_frames(&tree);
    assert_eq!(missing, Err(SceneError::UnknownFrame { entity: d.elements()[0].entity, frame: part }));
    tree.insert(Frame {
        id: part,
        name: "part".to_owned(),
        kind: FrameKind::Part,
        parent: Some(FrameId::WORLD),
        to_parent: Transform::IDENTITY,
    })
    .unwrap();
    assert_eq!(d.check_frames(&tree), Ok(()));
}

#[test]
fn id_space_exhaustion_leaves_graph_unchanged() {
    let d = doc("doc.txt");
    // 39 IDs: one short of the 40 nodes; the batch is refused before any change.
    let mut g = SceneGraph::with_id_limit(39);
    assert_eq!(g.project_into(&registry(), &d), Err(SceneError::Id(IdError::IdSpaceExhausted)));
    assert!(g.is_empty());
    assert_eq!(g.ids().allocated(), 0);
    let mut g = SceneGraph::with_id_limit(40);
    g.project_into(&registry(), &d).unwrap();
    assert_eq!(g.len(), 40);
    // A full table refuses an added element but keeps every node.
    let mut els = parse_doc(&fixture("doc.txt"));
    let mut extra = els[1].clone();
    extra.entity = uuid("00000000-0000-4000-8000-0000000000cc");
    els.push(extra.clone());
    let before = g.clone();
    assert_eq!(
        g.reproject(&registry(), &Document::new(els).unwrap(), &[extra.entity]),
        Err(SceneError::Id(IdError::IdSpaceExhausted))
    );
    assert_eq!(g, before);
}

#[test]
fn dirty_set_is_bounded() {
    let mut g = full();
    let too_many = vec![uuid("00000000-0000-4000-8000-000000000001"); fid_render::scene::proj::MAX_ELEMENTS + 1];
    assert_eq!(g.reproject(&registry(), &doc("doc.txt"), &too_many), Err(SceneError::TooManyElements));
}

#[test]
fn large_scene_reprojects_only_dirty_nodes() {
    // 30 000 point sets; 100 change. Only their 100 nodes are touched.
    let base = parse_doc(&fixture("doc.txt"))[2].clone();
    let make = |changed: bool| {
        let els: Vec<ElementView> = (0..30_000u128)
            .map(|i| {
                let mut e = base.clone();
                e.entity = EntityId::from_u128(0x1000_0000 + i);
                if changed && i % 300 == 7 {
                    e.status = DisplayStatus::Error(StatusCode::new("eval_failed").unwrap());
                }
                e
            })
            .collect();
        Document::new(els).unwrap()
    };
    let r = registry();
    let (mut g, _) = SceneGraph::project(&r, &make(false)).unwrap();
    let dirty: Vec<EntityId> =
        (0..30_000u128).filter(|i| i % 300 == 7).map(|i| EntityId::from_u128(0x1000_0000 + i)).collect();
    assert_eq!(dirty.len(), 100);
    let out = g.reproject(&r, &make(true), &dirty).unwrap();
    assert_eq!(out.batch.updated.len(), 100);
    assert!(out.batch.added.is_empty() && out.batch.retired.is_empty());
    assert!(out.batch.updated.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn kernel_statuses_round_trip_unchanged() {
    // Hand-labelled from the kdec outcomes: nothing is lost or upgraded.
    let cases = [
        (
            "kernel:INDETERMINATE:---",
            KernelStatus {
                outcome: KernelOutcome::Indeterminate,
                claimed: false,
                declared_input: false,
                uncharacterized_input: false,
            },
        ),
        (
            "kernel:NOT_DECIDABLE(u_missing):---",
            KernelStatus {
                outcome: KernelOutcome::NotDecidable(NotDecidableReason::UMissing),
                claimed: false,
                declared_input: false,
                uncharacterized_input: false,
            },
        ),
        (
            "kernel:NOT_DECIDABLE(pdf_unavailable):-du",
            KernelStatus {
                outcome: KernelOutcome::NotDecidable(NotDecidableReason::PdfUnavailable),
                claimed: false,
                declared_input: true,
                uncharacterized_input: true,
            },
        ),
        (
            "kernel:CONFORMS:c--",
            KernelStatus {
                outcome: KernelOutcome::Conforms,
                claimed: true,
                declared_input: false,
                uncharacterized_input: false,
            },
        ),
        (
            "kernel:NONCONFORMS:cdu",
            KernelStatus {
                outcome: KernelOutcome::Nonconforms,
                claimed: true,
                declared_input: true,
                uncharacterized_input: true,
            },
        ),
    ];
    let base = parse_doc(&fixture("doc.txt"))[4].clone(); // a characteristic
    for (text, kernel) in cases {
        let status = DisplayStatus::Kernel(kernel);
        assert_eq!(DisplayStatus::parse(text), Ok(status.clone()), "{text}");
        assert_eq!(status.to_string(), text);
        // Through projection: every node of the element carries the status verbatim, and the
        // serialised line parses back to the same status.
        let mut el = base.clone();
        el.status = status.clone();
        let (g, _) = SceneGraph::project(&registry(), &Document::new(vec![el]).unwrap()).unwrap();
        assert_eq!(g.len(), 3);
        for n in g.nodes() {
            assert_eq!(n.status, status);
            let line = n.to_string();
            let field = line.split(' ').nth(14).unwrap();
            assert_eq!(DisplayStatus::parse(field), Ok(status.clone()));
        }
    }
    for text in ["not_evaluated", "error:eval_failed"] {
        assert_eq!(DisplayStatus::parse(text).unwrap().to_string(), text);
    }
}

#[test]
fn error_node_keeps_the_kernel_status() {
    // Every characteristic projector fails; c01 is NONCONFORMS and must stay visible as such.
    let mut r = ProjectorRegistry::new();
    for k in ["part", "mesh", "point_set"] {
        r.register(Box::new(Geom(k))).unwrap();
    }
    r.register(Box::new(Bad("characteristic", 4))).unwrap();
    let d = doc("doc.txt");
    let (g, out) = SceneGraph::project(&r, &d).unwrap();
    assert_eq!(out.diagnostics.len(), 12);
    let c01 = uuid("00000000-0000-4000-8000-00000000000b");
    let n = g.get(rid(5)).unwrap();
    assert_eq!(n.identity, Identity::Entity(c01));
    assert_eq!(n.status, DisplayStatus::parse("kernel:NONCONFORMS:---").unwrap());
    assert_eq!(n.proj_error, Some(ProjError::Failed { reason: "no tessellation" }));
    assert_eq!(n.style.as_str(), "error");
    assert!(n.to_string().contains(" kernel:NONCONFORMS:--- ProjectorFailed sha256:"));
    for n in g.nodes().filter(|n| n.proj_error.is_some()) {
        assert_eq!(n.status, d.get(n.identity.entity()).unwrap().status);
    }
}

/// A small seeded generator (64-bit LCG, high bits), so the property test needs no dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next()).unwrap() % n
    }
}

/// The graph's content keyed by identity, without render IDs (a fresh session numbers nodes
/// differently).
fn content(g: &SceneGraph) -> std::collections::BTreeMap<Identity, String> {
    g.nodes().map(|n| (n.identity, n.to_string().split_once(' ').unwrap().1.to_owned())).collect()
}

#[test]
fn incremental_reprojection_equals_full_rebuild() {
    let statuses = [
        "not_evaluated",
        "error:eval_failed",
        "kernel:CONFORMS:---",
        "kernel:CONFORMS:c--",
        "kernel:NONCONFORMS:--u",
        "kernel:INDETERMINATE:-d-",
        "kernel:NOT_DECIDABLE(rule_missing):---",
    ];
    let kinds = ["part", "mesh", "point_set", "characteristic", "note", "broken"];
    let mut r = registry();
    r.register(Box::new(Bad("broken", 4))).unwrap();
    let template = parse_doc(&fixture("doc.txt"));
    for seed in 0..300u64 {
        let mut rng = Lcg(seed);
        let mut els = template.clone();
        let (mut g, _) = SceneGraph::project(&r, &Document::new(els.clone()).unwrap()).unwrap();
        let mut fresh = 0x9000u128;
        for _step in 0..12 {
            let mut dirty: Vec<EntityId> = Vec::new();
            for _ in 0..1 + rng.below(4) {
                let op = rng.below(7);
                if els.is_empty() || op == 0 {
                    // Insert a new element of a random kind at a random position.
                    let mut e = template[rng.below(template.len())].clone();
                    fresh += 1;
                    e.entity = EntityId::from_u128(fresh);
                    e.kind = ElementKind::new(kinds[rng.below(kinds.len())]).unwrap();
                    let at = rng.below(els.len() + 1);
                    dirty.push(e.entity);
                    els.insert(at, e);
                    continue;
                }
                let i = rng.below(els.len());
                dirty.push(els[i].entity);
                match op {
                    1 => {
                        els.remove(i);
                    }
                    2 => els[i].status = DisplayStatus::parse(statuses[rng.below(statuses.len())]).unwrap(),
                    3 => els[i].data_digest = fid_kci::kci::sha256_digest(&rng.next().to_le_bytes()),
                    4 => els[i].origin_m[2] += 0.001 * (rng.below(5) as f64),
                    5 => els[i].kind = ElementKind::new(kinds[rng.below(kinds.len())]).unwrap(),
                    _ => {} // dirty but unchanged
                }
            }
            // Extra clean entities, a duplicate and an unknown entity in the dirty set.
            for _ in 0..rng.below(3) {
                if !els.is_empty() {
                    dirty.push(els[rng.below(els.len())].entity);
                }
            }
            if let Some(&first) = dirty.first() {
                dirty.push(first);
            }
            dirty.push(EntityId::from_u128(0xdead_0000 + u128::from(seed)));
            let d = Document::new(els.clone()).unwrap();
            let out = g.reproject(&r, &d, &dirty).unwrap();
            let (rebuilt, _) = SceneGraph::project(&r, &d).unwrap();
            assert_eq!(content(&g), content(&rebuilt), "seed {seed}");
            // The batch is ascending and consistent with the table.
            for list in [&out.batch.added, &out.batch.updated, &out.batch.retired] {
                assert!(list.windows(2).all(|w| w[0] < w[1]), "seed {seed}");
            }
            for id in out.batch.added.iter().chain(&out.batch.updated) {
                assert!(g.get(*id).is_some(), "seed {seed}");
            }
            for id in &out.batch.retired {
                assert!(g.get(*id).is_none(), "seed {seed}");
            }
            for n in g.nodes() {
                assert_eq!(g.ids().render_id_of(n.identity), Some(n.id), "seed {seed}");
            }
        }
    }
}
