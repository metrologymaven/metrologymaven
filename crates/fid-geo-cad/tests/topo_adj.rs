//! GEO.CAD.TOPO.ADJ acceptance tests.
//!
//! Expected answers come from outside the Rust code: `fixtures/geo/cad/topo/cube_adjacency.json`
//! and `adjacency_expected.json`, written by the independent stdlib Python reference
//! `fixtures/geo/cad/topo/make_adj_fixtures.py` (literal scans over the canonical entity lists
//! of `make_topo_fixtures.py`). AC-4 checks a seeded 10⁵-face body against a brute-force scan
//! of the arena written here without the index.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use fid_geo_cad::{
    decode, encode, AdjError, Adjacency, Body, BodyBuilder, EntityIx, EntityKind, Frame, Label, PairingFault, Sense,
    SurfaceDef, SurfaceParams, TopoBuildError, TopologyId,
};

const QUERIES: [(&str, EntityKind); 7] = [
    ("faces_of_edge", EntityKind::Edge),
    ("coedges_of_edge", EntityKind::Edge),
    ("edges_of_face", EntityKind::Face),
    ("loops_of_face", EntityKind::Face),
    ("face_neighbours", EntityKind::Face),
    ("faces_of_vertex", EntityKind::Vertex),
    ("edges_of_vertex", EntityKind::Vertex),
];

fn run(adj: &Adjacency<'_>, q: &str, id: &TopologyId) -> Result<Vec<TopologyId>, AdjError> {
    match q {
        "faces_of_edge" => adj.faces_of_edge(id),
        "coedges_of_edge" => adj.coedges_of_edge(id),
        "edges_of_face" => adj.edges_of_face(id),
        "loops_of_face" => adj.loops_of_face(id),
        "face_neighbours" => adj.face_neighbours(id),
        "faces_of_vertex" => adj.faces_of_vertex(id),
        "edges_of_vertex" => adj.edges_of_vertex(id),
        _ => panic!("query {q}"),
    }
}

const OTHER: [&str; 5] = ["cube_unlabelled", "cylinder_capped", "sphere_hemispheres", "cylinder_seam", "bowtie_vertex"];

fn kind(ix: EntityIx) -> EntityKind {
    match ix {
        EntityIx::Shell(_) => EntityKind::Shell,
        EntityIx::Face(_) => EntityKind::Face,
        EntityIx::Loop(_) => EntityKind::Loop,
        EntityIx::Coedge(_) => EntityKind::Coedge,
        EntityIx::Edge(_) => EntityKind::Edge,
        EntityIx::Vertex(_) => EntityKind::Vertex,
    }
}

/// The text form used by the Python reference: a label verbatim, a derived ID hyphenated.
fn id_text(id: &TopologyId) -> String {
    match id {
        TopologyId::Imported(l) => l.as_str().to_string(),
        TopologyId::Derived(_) => id.to_string(),
    }
}

fn texts(ids: &[TopologyId]) -> Vec<String> {
    ids.iter().map(id_text).collect()
}

fn label(s: &str) -> TopologyId {
    TopologyId::Imported(Label::new(s).unwrap())
}

/// IDs of one kind in ascending order.
fn ids_of(body: &Body, k: EntityKind) -> Vec<TopologyId> {
    body.ids().iter().filter(|(_, ix)| kind(**ix) == k).map(|(id, _)| id.clone()).collect()
}

fn assert_sorted_unique(v: &[TopologyId], what: &str) {
    for w in v.windows(2) {
        assert!(w[0] < w[1], "{what}: {} then {}", w[0], w[1]);
    }
}

/// Every query on every entity equals the Python reference `exp`.
fn check_adjacency(name: &str, body: &Body, exp: &J) {
    let adj = Adjacency::build(body);
    let want: Vec<String> = exp.get("faces_of_body").arr().iter().map(|x| x.str().to_string()).collect();
    assert_eq!(texts(&adj.faces_of_body()), want, "{name} faces_of_body");
    for (q, k) in QUERIES {
        let table = exp.get(q).obj();
        let ids = ids_of(body, k);
        // The reference lists every entity of the kind, in ascending ID order.
        let mut keys: Vec<&String> = table.keys().collect();
        let mut ours: Vec<String> = texts(&ids);
        keys.sort();
        ours.sort();
        assert_eq!(ours.iter().collect::<Vec<_>>(), keys, "{name} {q} keys");
        for id in &ids {
            let got = run(&adj, q, id).unwrap();
            assert_sorted_unique(&got, q);
            let want: Vec<String> = table[&id_text(id)].arr().iter().map(|x| x.str().to_string()).collect();
            assert_eq!(texts(&got), want, "{name} {q}({id})");
        }
    }
}

// ---------------------------------------------------------------- acceptance

/// AC-1: the labelled cube against `cube_adjacency.json`; 4 neighbours per face, 2 faces per
/// edge.
#[test]
fn ac1_cube_matches_reference() {
    let exp = load("cube_adjacency.json");
    assert_eq!(exp.get("fixture").str(), "cube_labelled");
    let body = build("cube_labelled");
    check_adjacency("cube_labelled", &body, exp.get("adjacency"));
    let adj = Adjacency::build(&body);
    for f in ids_of(&body, EntityKind::Face) {
        let nb = adj.face_neighbours(&f).unwrap();
        assert_eq!(nb.len(), 4);
        assert!(!nb.contains(&f));
    }
    for e in ids_of(&body, EntityKind::Edge) {
        assert_eq!(adj.faces_of_edge(&e).unwrap().len(), 2);
    }
    // Faces #51..#56 are the planes x=0, x=1, y=0, y=1, z=0, z=1: #51 meets all but #52.
    assert_eq!(texts(&adj.face_neighbours(&label("#51")).unwrap()), ["#53", "#54", "#55", "#56"]);
    // Labels sort bytewise: "#10" before "#9".
    assert_eq!(texts(&adj.edges_of_face(&label("#55")).unwrap()), ["#10", "#13", "#14", "#9"]);
}

/// The other fixtures (derived IDs, seams, closed edges, one-coedge boundary edges, a
/// non-manifold vertex, an inner loop) against `adjacency_expected.json`.
#[test]
fn other_fixtures_match_reference() {
    let exp = load("adjacency_expected.json");
    let fixtures = exp.get("fixtures").obj();
    assert_eq!(fixtures.len(), OTHER.len());
    for name in OTHER {
        check_adjacency(name, &build(name), &fixtures[name]);
    }
}

/// AC-2: the seam edge of a one-face cylinder lists that face once, and the face is not its
/// own neighbour (REQ-004).
#[test]
fn ac2_seam_edge_one_face_not_own_neighbour() {
    let body = build("cylinder_seam");
    let adj = Adjacency::build(&body);
    let (seam, side) = (label("#seam"), label("#side"));
    assert_eq!(adj.faces_of_edge(&seam).unwrap(), vec![side.clone()]);
    assert_eq!(adj.coedges_of_edge(&seam).unwrap().len(), 2);
    assert_eq!(adj.face_neighbours(&side).unwrap(), Vec::<TopologyId>::new());
    assert_eq!(adj.edges_of_face(&side).unwrap().len(), 3);

    // Capped: the side face's neighbours are the two caps, not itself.
    let body = build("cylinder_capped");
    let adj = Adjacency::build(&body);
    let faces = ids_of(&body, EntityKind::Face);
    let side = faces.iter().find(|f| adj.edges_of_face(f).unwrap().len() == 3).unwrap();
    let nb = adj.face_neighbours(side).unwrap();
    assert_eq!(nb.len(), 2);
    assert!(!nb.contains(side));
}

/// AC-3 and REQ-002: wrong-kind and unknown IDs are typed errors, never an empty list.
#[test]
fn ac3_wrong_kind_and_unknown() {
    let body = build("cube_labelled");
    let adj = Adjacency::build(&body);
    let edge = label("#9");
    let err = adj.edges_of_face(&edge).unwrap_err();
    assert_eq!(err, AdjError::WrongKind { id: edge.clone(), expected: EntityKind::Face, found: EntityKind::Edge });
    assert_eq!(err.code(), "WrongKind");
    assert_eq!(err.to_string(), "\"#9\" has kind edge, expected face");

    // Every query, given an ID of each other kind (shells, loops and coedges included).
    let shell = ids_of(&body, EntityKind::Shell).remove(0);
    let samples = [shell, label("#51"), label("#45"), label("#21"), label("#9"), label("#1")];
    for (q, k) in QUERIES {
        for id in &samples {
            let found = kind(body.lookup(id).unwrap());
            match run(&adj, q, id) {
                Ok(v) => assert_eq!(found, k, "{q}({id}) answered {v:?}"),
                Err(AdjError::WrongKind { expected, found: got, .. }) => {
                    assert_ne!(found, k, "{q}");
                    assert_eq!((expected, got), (k, found), "{q}");
                }
                Err(e) => panic!("{q}({id}): {e}"),
            }
        }
        for id in [label("#999"), TopologyId::Derived([7; 16])] {
            let err = run(&adj, q, &id).unwrap_err();
            assert_eq!(err, AdjError::UnknownEntity { id: id.clone() }, "{q}");
            assert_eq!(err.code(), "UnknownEntity");
        }
    }
    assert_eq!(adj.faces_of_edge(&label("#999")).unwrap_err().to_string(), "\"#999\" is not an entity of this body");
}

/// Non-manifold cases: an edge with three coedges never becomes a body (the model refuses it,
/// so no adjacency holds one); a vertex where two faces meet only at a point lists both faces,
/// which are not neighbours.
#[test]
fn non_manifold_edge_refused_and_vertex_listed() {
    let err = builder(&load("invalid/sphere_three_coedges.json")).build().unwrap_err();
    assert_eq!(err, TopoBuildError::BadPairing { edge: 0, fault: PairingFault::TooMany });

    let body = build("bowtie_vertex");
    let adj = Adjacency::build(&body);
    let faces = adj.faces_of_body();
    assert_eq!(faces.len(), 2);
    let shared: Vec<TopologyId> =
        ids_of(&body, EntityKind::Vertex).into_iter().filter(|v| adj.faces_of_vertex(v).unwrap().len() == 2).collect();
    assert_eq!(shared.len(), 1);
    assert_eq!(adj.faces_of_vertex(&shared[0]).unwrap(), faces);
    assert_eq!(adj.edges_of_vertex(&shared[0]).unwrap().len(), 4);
    for f in &faces {
        assert!(adj.face_neighbours(f).unwrap().is_empty());
    }
    let loops: Vec<usize> = faces.iter().map(|f| adj.loops_of_face(f).unwrap().len()).collect();
    assert_eq!(loops.iter().copied().collect::<BTreeSet<_>>(), BTreeSet::from([1, 2]));
}

/// The index is derived: equal bodies (built twice, or decoded from the encoding) give equal
/// indexes and answers, and building it leaves the encoding untouched.
#[test]
fn index_is_derived_and_deterministic() {
    for name in ["cube_labelled", "cylinder_capped", "bowtie_vertex"] {
        let a = build(name);
        let bytes = encode(&a);
        let b = decode(&bytes).unwrap();
        let (ia, ib) = (Adjacency::build(&a), Adjacency::build(&b));
        assert_eq!(ia, ib);
        assert_eq!(ia, Adjacency::build(&a));
        for f in ids_of(&a, EntityKind::Face) {
            assert_eq!(ia.face_neighbours(&f), ib.face_neighbours(&f));
        }
        assert_eq!(encode(&a), bytes);
    }
}

// ---------------------------------------------------------------- AC-4

struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A random open quad mesh: 10⁵ of the 340 × 330 unit cells of a grid (seed 3), in shuffled
/// order with shuffled loop starts; about half of all entities carry labels. Missing cells
/// leave one-coedge boundary edges and vertices where faces meet only at a corner.
fn random_body(seed: u64, faces: usize) -> BodyBuilder {
    const W: usize = 340;
    const H: usize = 330;
    let mut rng = SplitMix(seed);
    let mut cells: Vec<(usize, usize)> = (0..W * H).map(|k| (k % W, k / W)).collect();
    for i in (1..cells.len()).rev() {
        let j = rng.below(i + 1);
        cells.swap(i, j);
    }
    cells.truncate(faces);
    let mut b = BodyBuilder::new([3; 16]);
    let plane = SurfaceDef::analytic(SurfaceParams::Plane {
        frame: Frame { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] },
    })
    .unwrap();
    let mut vertex: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    let mut edge: BTreeMap<((usize, usize), (usize, usize)), usize> = BTreeMap::new();
    let mut face_ix = Vec::with_capacity(faces);
    for (n, &(i, j)) in cells.iter().enumerate() {
        let ring = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
        let start = rng.below(4);
        let mut coedges = Vec::with_capacity(4);
        for k in 0..4 {
            let p = ring[(start + k) % 4];
            let q = ring[(start + k + 1) % 4];
            for v in [p, q] {
                if !vertex.contains_key(&v) {
                    let lab = format!("#v{}", vertex.len());
                    let l = (rng.next() & 1 == 0).then_some(lab.as_str());
                    let ix = b.add_vertex([v.0 as f64, v.1 as f64, 0.0], l);
                    vertex.insert(v, ix);
                }
            }
            let (key, sense) = if p < q { ((p, q), Sense::Same) } else { ((q, p), Sense::Reversed) };
            let e = match edge.get(&key) {
                Some(&e) => e,
                None => {
                    let lab = format!("#e{}", edge.len());
                    let l = (rng.next() & 1 == 0).then_some(lab.as_str());
                    let e = b.add_edge(None, vertex[&key.0], vertex[&key.1], [0.0, 1.0], l);
                    edge.insert(key, e);
                    e
                }
            };
            let lab = format!("#c{n}_{k}");
            let l = (rng.next() & 1 == 0).then_some(lab.as_str());
            coedges.push(b.add_coedge(e, sense, None, l));
        }
        let (ll, fl) = (format!("#l{n}"), format!("#f{n}"));
        let lp = b.add_loop(coedges, true, (rng.next() & 1 == 0).then_some(ll.as_str()));
        face_ix.push(b.add_face(plane.clone(), Sense::Same, vec![lp], (rng.next() & 1 == 0).then_some(fl.as_str())));
    }
    b.add_shell(face_ix, false);
    b
}

#[test]
fn ac4_random_body_agrees_with_brute_force() {
    let body = random_body(3, 100_000).build().unwrap();
    assert_eq!(body.faces().len(), 100_000);
    let adj = Adjacency::build(&body);
    let reference = scan_all(&body);
    let mut boundary = 0usize;
    for (q, k) in QUERIES {
        let table = &reference[q];
        for (i, want) in table.iter().enumerate() {
            let id = match k {
                EntityKind::Edge => body.edges()[i].id(),
                EntityKind::Face => body.faces()[i].id(),
                _ => body.vertices()[i].id(),
            };
            let got = run(&adj, q, id).unwrap();
            assert_eq!(&got, want, "{q}({id})");
            if q == "faces_of_edge" && got.len() == 1 {
                boundary += 1;
            }
        }
    }
    assert!(boundary > 0);
    assert_eq!(adj.faces_of_body(), ids_of(&body, EntityKind::Face));

    // Literal scans for a seeded sample (no maps at all).
    let mut rng = SplitMix(3);
    let faces = body.faces();
    let edges = body.edges();
    let coedge_face = |c: usize| body.r#loop(body.coedges()[c].r#loop()).unwrap().face().index();
    for _ in 0..16 {
        let e = rng.below(edges.len());
        let mut want: Vec<TopologyId> = (0..body.coedges().len())
            .filter(|&c| body.coedges()[c].edge().index() == e)
            .map(|c| faces[coedge_face(c)].id().clone())
            .collect();
        want.sort();
        want.dedup();
        assert_eq!(adj.faces_of_edge(edges[e].id()).unwrap(), want);

        let f = rng.below(faces.len());
        let mine: Vec<usize> = (0..body.coedges().len()).filter(|&c| coedge_face(c) == f).collect();
        let my_edges: Vec<usize> = mine.iter().map(|&c| body.coedges()[c].edge().index()).collect();
        let mut want: Vec<TopologyId> = (0..body.coedges().len())
            .filter(|&c| my_edges.contains(&body.coedges()[c].edge().index()) && coedge_face(c) != f)
            .map(|c| faces[coedge_face(c)].id().clone())
            .collect();
        want.sort();
        want.dedup();
        assert_eq!(adj.face_neighbours(faces[f].id()).unwrap(), want);

        let v = rng.below(body.vertices().len());
        let mut want: Vec<TopologyId> =
            edges.iter().filter(|x| x.start().index() == v || x.end().index() == v).map(|x| x.id().clone()).collect();
        want.sort();
        want.dedup();
        assert_eq!(adj.edges_of_vertex(body.vertices()[v].id()).unwrap(), want);
    }
}

/// Every query's answer for every argument (by dense index of the argument), by walking
/// faces, loops, coedges and edges of the arena; each list then sorted and deduplicated.
fn scan_all(body: &Body) -> BTreeMap<&'static str, Vec<Vec<TopologyId>>> {
    let (nf, ne, nv) = (body.faces().len(), body.edges().len(), body.vertices().len());
    let mut m: BTreeMap<&'static str, Vec<Vec<TopologyId>>> = BTreeMap::new();
    for (q, k) in QUERIES {
        let n = match k {
            EntityKind::Edge => ne,
            EntityKind::Face => nf,
            _ => nv,
        };
        m.insert(q, vec![Vec::new(); n]);
    }
    let mut edge_faces: Vec<Vec<usize>> = vec![Vec::new(); ne];
    {
        let mut add = |q: &'static str, key: usize, val: &TopologyId| {
            m.get_mut(q).unwrap()[key].push(val.clone());
        };
        for (fi, face) in body.faces().iter().enumerate() {
            for &l in face.loops() {
                let lp = body.r#loop(l).unwrap();
                add("loops_of_face", fi, lp.id());
                for &c in lp.coedges() {
                    let co = body.coedge(c).unwrap();
                    let ei = co.edge().index();
                    let edge = body.edge(co.edge()).unwrap();
                    edge_faces[ei].push(fi);
                    add("coedges_of_edge", ei, co.id());
                    add("faces_of_edge", ei, face.id());
                    add("edges_of_face", fi, edge.id());
                    for v in [edge.start(), edge.end()] {
                        add("faces_of_vertex", v.index(), face.id());
                    }
                }
            }
        }
        for edge in body.edges() {
            for v in [edge.start(), edge.end()] {
                add("edges_of_vertex", v.index(), edge.id());
            }
        }
        for fs in &edge_faces {
            for &a in fs {
                for &b in fs {
                    if a != b {
                        add("face_neighbours", a, body.faces()[b].id());
                    }
                }
            }
        }
    }
    for lists in m.values_mut() {
        for v in lists.iter_mut() {
            v.sort();
            v.dedup();
        }
    }
    m
}
