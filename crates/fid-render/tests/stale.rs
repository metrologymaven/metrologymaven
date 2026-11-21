//! Acceptance criteria AC-1 .. AC-4 of spec leaf VIZ.RENDER.SCENE.STALE, plus its error paths.
//! Expected graphs and batches come from `fixtures/viz/scene-doc-small` (written by the
//! independent Python scripts `make_fixture.py` and `make_stale.py`, which implement the
//! projectors, the line layout and the stale rules separately) or are hand-labelled from the
//! spec's rules; none is produced by running this crate. No GPU is used.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_frame::FrameId;
use fid_geo_types::Aabb;
use fid_kdec::Outcome as KernelOutcome;
use fid_render::scene::idtab::{EntityId, RenderId, ViewOp};
use fid_render::scene::proj::{
    ChangeBatch, DisplayStatus, Document, ElementKind, ElementView, KernelStatus, NodeDraw, NodeKind, NodeSink,
    ProjError, Projector, ProjectorRegistry, SceneError, SceneGraph, StatusCode, StyleToken, MAX_ELEMENTS,
};
use fid_render::scene::stale::{marked_status, next_status, Evidence, StatusStyles, StyleError, StyleResolver};
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

fn entities(name: &str) -> Vec<EntityId> {
    fixture(name).lines().map(uuid).collect()
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

/// Render IDs 5..=40: the 12 characteristics' primary, label and leader nodes (hand-labelled:
/// the 4 geometry elements come first in tree order, one node each).
fn char_rids() -> Vec<RenderId> {
    (5..=40).map(rid).collect()
}

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
    assert_eq!(g.stale_count(), 0);
    g
}

/// The full graph with the alignment change marked (AC-1).
fn marked() -> SceneGraph {
    let mut g = full();
    g.mark_outdated(&entities("stale-outdated.txt")).unwrap();
    g
}

#[test]
fn ac1_alignment_change_marks_all_characteristics_and_labels_stale() {
    let mut g = full();
    let outdated = entities("stale-outdated.txt");
    let out = g.mark_outdated(&outdated).unwrap();
    assert_eq!(out.batch, parse_batch(&fixture("stale-expect-mark-batch.txt")));
    assert_eq!(out.batch.updated, char_rids());
    assert_eq!(out.stale_count, 36);
    assert_eq!(g.stale_count(), 36);
    assert_eq!(g.serialise().unwrap(), fixture("stale-expect-marked.txt"));
    assert_eq!(g.digest().unwrap().as_str(), fixture("stale-expect-marked.digest").trim_end());
    // All 12 characteristic nodes and their labels (and leaders) are stale; geometry is not.
    let d = doc("doc.txt");
    let mut labels = 0;
    for n in g.nodes() {
        let el = d.get(n.identity.entity()).unwrap();
        let is_char = el.kind.as_str() == "characteristic";
        assert_eq!(n.status.is_stale(), is_char, "node {}", n.id);
        if is_char {
            assert_eq!(
                n.status,
                DisplayStatus::Stale { since_digest: n.data_digest, evidence: Evidence::of(&el.status) }
            );
        } else {
            assert_eq!(n.status, el.status);
        }
        labels += usize::from(n.kind == NodeKind::Derived(ViewOp::Label) && n.status.is_stale());
    }
    assert_eq!(labels, 12);
    // Every mark is kept, including the alignment, which has no node.
    assert!(outdated.iter().all(|e| g.is_outdated(*e)));
    assert_eq!(g.outdated().count(), 13);
    // Marking again changes nothing (since digests are kept).
    let again = g.mark_outdated(&outdated).unwrap();
    assert!(again.batch.is_empty());
    assert_eq!(again.stale_count, 36);
    assert_eq!(g.serialise().unwrap(), fixture("stale-expect-marked.txt"));
}

#[test]
fn ac2_recompute_with_new_digests_restores_kernel_statuses() {
    let mut g = marked();
    let re = doc("stale-doc-recomputed.txt");
    let out = g.recomputed(&registry(), &re, &entities("stale-dirty.txt"), &entities("stale-outdated.txt")).unwrap();
    assert!(out.diagnostics.is_empty());
    assert_eq!(out.batch, parse_batch(&fixture("stale-expect-recompute-batch.txt")));
    assert_eq!(out.stale_count, 0);
    assert_eq!(g.stale_count(), 0);
    assert_eq!(g.outdated().count(), 0);
    assert_eq!(g.serialise().unwrap(), fixture("stale-expect-recomputed.txt"));
    assert_eq!(g.digest().unwrap().as_str(), fixture("stale-expect-recomputed.digest").trim_end());
    // Kernel-supplied statuses, copied from the recomputed document.
    for n in g.nodes() {
        assert_eq!(n.status, re.get(n.identity.entity()).unwrap().status);
    }
}

#[test]
fn new_digest_while_still_outdated_stays_stale() {
    // Marks not cleared: the nodes take the delivered data but stay stale since the old digest.
    let mut g = marked();
    let re = doc("stale-doc-recomputed.txt");
    let out = g.recomputed(&registry(), &re, &entities("stale-dirty.txt"), &[]).unwrap();
    assert_eq!(out.batch.updated, char_rids());
    assert_eq!(out.stale_count, 36);
    assert_eq!(g.serialise().unwrap(), fixture("stale-expect-held.txt"));
    // Clearing the marks alone delivers nothing, so nothing leaves Stale ...
    g.clear_outdated(&entities("stale-outdated.txt")).unwrap();
    assert_eq!(g.stale_count(), 36);
    // ... until the projector delivers again.
    let out = g.reproject(&registry(), &re, &entities("stale-dirty.txt")).unwrap();
    assert_eq!(out.batch.updated, char_rids());
    assert_eq!(g.stale_count(), 0);
    assert_eq!(g.serialise().unwrap(), fixture("stale-expect-recomputed.txt"));
}

/// Test styles standing in for VIZ.OVERLAY.STATUS, with switches for broken variants.
#[derive(Default)]
struct Styles {
    stale_as: Option<&'static str>,
    claimed_as_measured: bool,
    stale_ignores_evidence: bool,
}

impl StatusStyles for Styles {
    fn kernel(&self, k: &KernelStatus) -> StyleToken {
        let base = match k.outcome {
            KernelOutcome::Conforms => "status.conforms",
            KernelOutcome::Nonconforms => "status.nonconforms",
            KernelOutcome::Indeterminate => "status.indeterminate",
            KernelOutcome::NotDecidable(_) => "status.not_decidable",
        };
        if Evidence::of_kernel(k).any() && !self.claimed_as_measured {
            style(&format!("{base}.claimed"))
        } else {
            style(base)
        }
    }
    fn stale(&self, e: Evidence) -> StyleToken {
        match self.stale_as {
            Some(s) => style(s),
            None if e.any() && !self.stale_ignores_evidence => style("status.stale.claimed"),
            None => style("status.stale"),
        }
    }
    fn not_evaluated(&self) -> StyleToken {
        style("status.not_evaluated")
    }
    fn error(&self, _code: &StatusCode) -> StyleToken {
        style("error")
    }
}

#[test]
fn ac3_stale_conforming_node_gets_the_stale_style() {
    let g = marked();
    let r = StyleResolver::new(Styles::default()).unwrap();
    // Node 5 is c01 (NONCONFORMS); c05 (CONFORMS, hand-labelled from doc.txt) is 17..19.
    let d = doc("doc.txt");
    let c05 = uuid("00000000-0000-4000-8000-00000000000f");
    assert_eq!(d.get(c05).unwrap().status, DisplayStatus::parse("kernel:CONFORMS:---").unwrap());
    for id in [17, 18, 19] {
        let n = g.get(rid(id)).unwrap();
        assert!(n.status.is_stale());
        assert_eq!(r.resolve_node(n), style("status.stale"));
    }
    // No stale node of the graph resolves to a conforming or nonconforming style.
    for n in g.nodes().filter(|n| n.status.is_stale()) {
        let t = r.resolve_node(n);
        assert!(t.as_str().starts_with("status.stale"), "node {}: {t}", n.id);
    }
    // The claimed c03 (CONFORMS, claimed) keeps a not-measured stale style (D-045).
    let c03 = uuid("00000000-0000-4000-8000-00000000000d");
    assert_eq!(d.get(c03).unwrap().status, DisplayStatus::parse("kernel:CONFORMS:c--").unwrap());
    let n = g.get(g.render_ids(c03)[0]).unwrap();
    assert_eq!(r.resolve_node(n), style("status.stale.claimed"));
    // Unmarked nodes resolve by their kernel status.
    assert_eq!(r.resolve(&DisplayStatus::parse("kernel:CONFORMS:---").unwrap()), style("status.conforms"));
    assert_eq!(r.resolve(&DisplayStatus::parse("kernel:CONFORMS:c--").unwrap()), style("status.conforms.claimed"));
    assert_eq!(r.resolve(&DisplayStatus::NotEvaluated), style("status.not_evaluated"));
}

#[test]
fn ac3_error_path_resolver_refuses_stale_styled_as_decided() {
    for (s, outcome) in
        [("status.conforms", KernelOutcome::Conforms), ("status.nonconforms", KernelOutcome::Nonconforms)]
    {
        let bad = Styles { stale_as: Some(s), ..Styles::default() };
        assert_eq!(
            StyleResolver::new(bad).unwrap_err(),
            StyleError::StaleLooksDecided { evidence: Evidence::MEASURED, outcome }
        );
    }
    // The claimed conforming style is a conforming style too.
    let bad = Styles { stale_as: Some("status.conforms.claimed"), ..Styles::default() };
    assert_eq!(StyleResolver::new(bad).unwrap_err().code(), "StaleLooksDecided");
}

#[test]
fn resolver_refuses_claimed_values_that_look_measured() {
    let bad = Styles { claimed_as_measured: true, ..Styles::default() };
    let err = StyleResolver::new(bad).unwrap_err();
    assert_eq!(err.code(), "ClaimedLooksMeasured");
    let bad = Styles { stale_ignores_evidence: true, ..Styles::default() };
    assert_eq!(StyleResolver::new(bad).unwrap_err().code(), "StaleLooksMeasured");
}

#[test]
fn ac4_same_digest_with_mark_cleared_leaves_stale() {
    let mut g = marked();
    let d = doc("doc.txt");
    let dirty = entities("stale-dirty.txt");
    // Same digest, mark still set: delivered, but stays stale, and nothing changes.
    let out = g.recomputed(&registry(), &d, &dirty, &[]).unwrap();
    assert!(out.batch.is_empty());
    assert_eq!(out.stale_count, 36);
    // Same digest, mark cleared: leaves Stale, back to the full projection.
    let out = g.recomputed(&registry(), &d, &dirty, &entities("stale-outdated.txt")).unwrap();
    assert_eq!(out.batch.updated, char_rids());
    assert!(out.batch.added.is_empty() && out.batch.retired.is_empty());
    assert_eq!(out.stale_count, 0);
    assert_eq!(g.serialise().unwrap(), fixture("expect-full.txt"));
    assert_eq!(g.digest().unwrap().as_str(), fixture("expect-full.digest").trim_end());
}

#[test]
fn failed_recompute_keeps_the_marks() {
    let mut g = marked();
    let before = g.clone();
    let too_many = vec![uuid("00000000-0000-4000-8000-0000000000aa"); MAX_ELEMENTS + 1];
    let err = g.recomputed(&registry(), &doc("doc.txt"), &too_many, &entities("stale-outdated.txt"));
    assert_eq!(err, Err(SceneError::TooManyElements));
    assert_eq!(g, before);
    assert_eq!(g.mark_outdated(&too_many), Err(SceneError::TooManyElements));
    assert_eq!(g.clear_outdated(&too_many), Err(SceneError::TooManyElements));
    assert_eq!(g, before);
}

#[test]
fn a_document_may_not_supply_stale() {
    let mut els = parse_doc(&fixture("doc.txt"));
    let e = els[4].entity;
    els[4].status = DisplayStatus::Stale { since_digest: els[4].data_digest, evidence: Evidence::MEASURED };
    assert_eq!(Document::new(els), Err(SceneError::StaleInDocument(e)));
}

#[test]
fn stale_status_text_round_trips_and_is_strict() {
    let digest = "sha256:5adc8ad4583f09c2662e954928577442a45c90f2292d626f7ae353b9802a6acb";
    for (flags, e) in [
        ("---", Evidence::MEASURED),
        ("c--", Evidence { claimed: true, ..Evidence::MEASURED }),
        ("-du", Evidence { declared_input: true, uncharacterized_input: true, ..Evidence::MEASURED }),
    ] {
        let text = format!("stale:{flags}:{digest}");
        let s = DisplayStatus::parse(&text).unwrap();
        assert_eq!(s, DisplayStatus::Stale { since_digest: Digest::parse(digest).unwrap(), evidence: e });
        assert_eq!(s.to_string(), text);
    }
    for bad in [
        "stale:",
        "stale:---",
        "stale:---:",
        "stale:--:",
        "stale:cdu-:sha256:00",
        "stale:dcu:sha256:5adc8ad4583f09c2662e954928577442a45c90f2292d626f7ae353b9802a6acb",
        "stale:---:sha256:5ADC8AD4583F09C2662E954928577442A45C90F2292D626F7AE353B9802A6ACB",
        "Stale:---:sha256:5adc8ad4583f09c2662e954928577442a45c90f2292d626f7ae353b9802a6acb",
    ] {
        assert_eq!(DisplayStatus::parse(bad), Err(SceneError::BadStatus), "{bad}");
    }
}

#[test]
fn next_status_table() {
    // Pure function, hand-labelled from the table in its docs.
    let g = full();
    let fresh = g.get(rid(17)).unwrap().clone(); // c05, CONFORMS, not stale
    let stale = {
        let mut n = fresh.clone();
        n.status = marked_status(&fresh);
        n
    };
    let since = fresh.data_digest;
    let other = Digest::from_bytes([7; 32]);
    let delivered = DisplayStatus::parse("kernel:NONCONFORMS:--u").unwrap();
    let ev = Evidence { uncharacterized_input: true, ..Evidence::MEASURED };
    // Not marked: the delivered status, whatever the digest or current node.
    for cur in [None, Some(&fresh), Some(&stale)] {
        for dig in [since, other] {
            assert_eq!(next_status(&delivered, dig, cur, false), delivered);
        }
    }
    // Marked: stale since the recorded digest, the current digest, or the delivered one.
    let want = |d: Digest| DisplayStatus::Stale { since_digest: d, evidence: ev };
    assert_eq!(next_status(&delivered, other, Some(&stale), true), want(since));
    assert_eq!(next_status(&delivered, other, Some(&fresh), true), want(since));
    assert_eq!(next_status(&delivered, other, None, true), want(other));
    // marked_status keeps an existing stale status and the evidence facts.
    assert_eq!(marked_status(&stale), stale.status);
    assert_eq!(stale.status, DisplayStatus::Stale { since_digest: since, evidence: Evidence::MEASURED });
    let mut err = fresh.clone();
    err.status = DisplayStatus::Error(StatusCode::new("eval_failed").unwrap());
    assert_eq!(marked_status(&err), DisplayStatus::Stale { since_digest: since, evidence: Evidence::MEASURED });
}

#[test]
fn node_added_for_an_outdated_element_starts_stale() {
    // ps2 (entity 4) is marked outdated, then its projector starts emitting a label.
    struct Labelled;
    impl Projector for Labelled {
        fn kind(&self) -> ElementKind {
            ElementKind::new("point_set").unwrap()
        }
        fn project(&self, el: &ElementView, out: &mut NodeSink<'_>) -> Result<(), ProjError> {
            let draw = NodeDraw { origin_m: el.origin_m, bounds_m: el.bounds_m, style: style("geom.point_set") };
            out.primary(draw.clone())?;
            out.derived(ViewOp::Label, draw)
        }
    }
    let mut r = ProjectorRegistry::new();
    for k in ["part", "mesh"] {
        r.register(Box::new(Geom(k))).unwrap();
    }
    r.register(Box::new(Labelled)).unwrap();
    r.register(Box::new(Characteristic)).unwrap();
    let mut g = full();
    let ps2 = uuid("00000000-0000-4000-8000-000000000004");
    let out = g.mark_outdated(&[ps2]).unwrap();
    assert_eq!(out.batch.updated, vec![rid(4)]);
    let out = g.reproject(&r, &doc("doc.txt"), &[ps2]).unwrap();
    assert_eq!(out.batch.added, vec![rid(41)]);
    assert!(out.batch.updated.is_empty());
    assert!(g.get(rid(41)).unwrap().status.is_stale());
    assert_eq!(g.stale_count(), 2);
}
