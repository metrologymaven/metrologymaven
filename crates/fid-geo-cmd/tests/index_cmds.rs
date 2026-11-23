//! Acceptance and unit tests for `GEO.INDEX.CMD` (`spec/geo/index/cmd/NODE.md`):
//! AC-1..AC-5 plus error paths, selection handles, authored length units, primitive BVH
//! builds, and Activity Log / Record Journal separation.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use fid_canon::{parse, CValue};
use fid_dep::{EdgeIndex, ElementStates, RecomputeMode};
use fid_geo_cmd::{
    build_index, command_defs, invoke_index_cli, invoke_index_mcp, query_box, query_knn, query_radius,
    register_element, ActivityEntry, AuthoredLength, BuildIndexParams, BuildKind, EntityId, GeoCmdError, GeoTx,
    IdOrigin, QueryBoxParams, QueryKnnParams, QueryPoints, QueryRadiusParams, SelectionPoints, Surface, TreeEntry,
    BUILD_INDEX, MAX_INLINE_QUERIES, MAX_RESULT_ENTRIES, MCP_BUILD_INDEX, MCP_QUERY_BOX, MCP_QUERY_KNN,
    MCP_QUERY_RADIUS, QUERY_BOX, QUERY_KNN, QUERY_RADIUS,
};
use fid_geo_index::{estimate_bytes, BuildParams, IndexCache, IndexKind, MemoryBudget, PointTree};
use fid_geo_types::{FrameRef, GeometryElement, Mesh, PointCloud, PointSet};
use fid_project::{Bits, Observation, ReplaySource};
use fid_registry::{tool_name, Catalogue, CommandDecl};
use fid_uuid::Uuid;

// ------------------------------------------------------------------ pinned uniform_1e5 generator
// Matches `tools/fixtures/geo_index.py` / `crates/fid-geo-index/tests/kdtree.rs`.

struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

fn uniform_1e5() -> (Vec<[f64; 3]>, Vec<[f64; 3]>) {
    let mut g = SplitMix(21);
    let n = 100_000usize;
    let pts: Vec<[f64; 3]> = (0..n).map(|_| [g.unit() * 0.125, g.unit() * 0.125, g.unit() * 0.125]).collect();
    let mut qs = Vec::new();
    for i in 0..32 {
        if i % 10 == 0 {
            qs.push(pts[(g.next() % n as u64) as usize]);
        } else {
            qs.push(std::array::from_fn(|_| g.unit() * 0.140625 - 0.0078125));
        }
    }
    (pts, qs)
}

struct TestProject {
    tree: BTreeMap<EntityId, TreeEntry>,
    edges: EdgeIndex,
    states: ElementStates,
    namespace: Uuid,
    record_journal: Vec<String>,
    activity_log: Vec<ActivityEntry>,
    cache: IndexCache,
    selections: BTreeMap<EntityId, SelectionPoints>,
}

impl TestProject {
    fn new() -> Self {
        let namespace = Uuid::parse("0190a5b2-7c3d-7000-8000-000000000001").unwrap();
        Self {
            tree: BTreeMap::new(),
            edges: EdgeIndex::new(),
            states: ElementStates::new(),
            namespace,
            record_journal: Vec::new(),
            activity_log: Vec::new(),
            cache: IndexCache::new(MemoryBudget::UNLIMITED),
            selections: BTreeMap::new(),
        }
    }

    fn register(&mut self, elem: GeometryElement, tag: u64) -> EntityId {
        let mut obs = ReplaySource::new(vec![
            Observation::NowMs(1_700_000_000_000 + tag),
            Observation::RandomBits(Bits::new(74, tag as u128 + 1).unwrap()),
        ]);
        let mut tx = GeoTx {
            tree: &mut self.tree,
            edges: &mut self.edges,
            states: &mut self.states,
            mode: RecomputeMode::Manual,
            namespace: self.namespace,
            source: &mut obs,
        };
        let reg = register_element(&mut tx, elem, IdOrigin::New).unwrap();
        self.record_journal.push(format!("registered:{}", reg.id));
        reg.id
    }
}

fn frame_a() -> FrameRef {
    FrameRef::from_bytes(*Uuid::parse("0190a5b2-7c3d-7000-8000-000000000010").unwrap().as_bytes())
}

fn frame_b() -> FrameRef {
    FrameRef::from_bytes(*Uuid::parse("0190a5b2-7c3d-7000-8000-000000000020").unwrap().as_bytes())
}

fn write_temp_points(name: &str, content: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fid-geo-index-cmd-{n}"));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

/// AC-1: Given a project containing `uniform_1e5`, running
/// `fid geo query-knn <id> --k 16 --points q.json --json` returns output identical to the
/// library `PointTree::knn` result and leaves the Record Journal length unchanged.
#[test]
fn ac1_query_knn_cli_equals_library_and_record_journal_unchanged() {
    let (pts, qs) = uniform_1e5();
    let cloud = PointCloud::from_points(frame_a(), &pts).unwrap();
    let expected_tree = PointTree::build(cloud.buffer(), 1).unwrap();
    let expected_hits: Vec<Vec<(u64, f64)>> = qs
        .iter()
        .map(|&q| {
            expected_tree.knn(cloud.buffer(), q, 16).unwrap().into_iter().map(|n| (n.index, n.dist2.sqrt())).collect()
        })
        .collect();

    let mut proj = TestProject::new();
    let elem_id = proj.register(GeometryElement::PointCloud(cloud), 1);
    let journal_before = proj.record_journal.len();

    // Write q.json with matching frame.
    let pts_json = qs.iter().map(|p| format!("[{:?},{:?},{:?}]", p[0], p[1], p[2])).collect::<Vec<_>>().join(",");
    let q_file_content = format!(r#"{{"frame":"{}","points":[{pts_json}]}}"#, Uuid::from_bytes(frame_a().to_bytes()));
    let q_path = write_temp_points("ac1_q.json", &q_file_content);
    let q_path_str = q_path.to_string_lossy().to_string();
    let elem_str = elem_id.to_string();

    let out_bytes = invoke_index_cli(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &mut proj.activity_log,
        &["query-knn", &elem_str, "--k", "16", "--points", &q_path_str, "--json"],
    )
    .expect("cli query-knn");
    let _ = fs::remove_file(&q_path);

    // Record Journal length is unchanged; Activity Log recorded one R1 entry.
    assert_eq!(proj.record_journal.len(), journal_before);
    assert_eq!(proj.activity_log.len(), 1);
    assert_eq!(proj.activity_log[0], ActivityEntry { command: QUERY_KNN, surface: Surface::Cli, outcome: "ok" });

    // CLI output equals library result bit-for-bit.
    let CValue::Obj(root) = parse(&out_bytes).unwrap() else { panic!("object") };
    assert_eq!(root.get("truncated"), Some(&CValue::Bool(false)));
    let Some(CValue::Arr(results)) = root.get("results") else { panic!("results array") };
    assert_eq!(results.len(), expected_hits.len());
    for (got_list, exp_list) in results.iter().zip(expected_hits.iter()) {
        let CValue::Arr(items) = got_list else { panic!("hit list") };
        assert_eq!(items.len(), exp_list.len());
        for (item, &(exp_idx, exp_dist)) in items.iter().zip(exp_list.iter()) {
            let CValue::Obj(o) = item else { panic!("hit obj") };
            assert_eq!(o.get("index"), Some(&CValue::Num(exp_idx as f64)));
            let Some(CValue::Num(d)) = o.get("distance_m") else { panic!("distance_m") };
            assert_eq!(d.to_bits(), exp_dist.to_bits());
        }
    }

    // MCP produces identical canonical bytes for the same parameters.
    let mcp_params = format!(
        r#"{{"element":"{elem_str}","frame":"{}","k":16,"points":[{pts_json}]}}"#,
        Uuid::from_bytes(frame_a().to_bytes())
    );
    let mcp_bytes = invoke_index_mcp(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &mut proj.activity_log,
        MCP_QUERY_KNN,
        mcp_params.as_bytes(),
    )
    .expect("mcp query-knn");
    assert_eq!(mcp_bytes, out_bytes);
    assert_eq!(proj.record_journal.len(), journal_before);
}

/// AC-2: Given radius queries returning 1.2 × 10^6 entries, exactly 10^6 entries are
/// returned in query order with `truncated: true`.
#[test]
fn ac2_radius_query_truncates_at_exactly_one_million_entries() {
    // 1 200 points in a tight cluster; 1 000 queries each match all 1 200 points => 1.2M entries.
    let pts: Vec<[f64; 3]> = (0..1_200).map(|i| [i as f64 * 1e-6, 0.0, 0.0]).collect();
    let ps = PointSet::from_points(frame_a(), &pts).unwrap();
    let mut proj = TestProject::new();
    let elem_id = proj.register(GeometryElement::PointSet(ps), 2);
    let journal_before = proj.record_journal.len();

    let queries: Vec<[f64; 3]> = vec![[0.0, 0.0, 0.0]; 1_000];
    let res = query_radius(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &QueryRadiusParams {
            element: elem_id,
            query: QueryPoints::Inline { points: queries, frame: Some(frame_a()) },
            radius: AuthoredLength::parse_str("10mm").unwrap(),
        },
    )
    .expect("radius query");

    let total: usize = res.results.iter().map(Vec::len).sum();
    assert_eq!(total, MAX_RESULT_ENTRIES);
    assert!(res.truncated);
    // 1_000_000 / 1_200 = 833 full lists (999_600) + 1 partial list of 400 entries.
    assert_eq!(res.results.len(), 834);
    assert_eq!(res.results[0].len(), 1_200);
    assert_eq!(res.results[833].len(), 400);
    assert_eq!(res.results[833][0].index, 0);
    assert_eq!(res.results[833][399].index, 399);
    assert_eq!(proj.record_journal.len(), journal_before);
}

/// AC-3 (error path): Given a budget smaller than the index estimate, `fid geo build-index`
/// fails with `budget_exceeded` and reports the needed bytes without allocating or caching.
#[test]
fn ac3_build_index_budget_exceeded_reports_needed_bytes() {
    let (pts, _) = uniform_1e5();
    let cloud = PointCloud::from_points(frame_a(), &pts).unwrap();
    let mut proj = TestProject::new();
    let elem_id = proj.register(GeometryElement::PointCloud(cloud), 3);
    let expected_needed = estimate_bytes(IndexKind::KdTree, 100_000, BuildParams::kdtree());
    let tight_budget = expected_needed / 2;
    assert!(expected_needed > tight_budget && tight_budget > 0);

    let elem_str = elem_id.to_string();
    let budget_str = tight_budget.to_string();
    let err = invoke_index_cli(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &mut proj.activity_log,
        &["build-index", &elem_str, "--kind", "point", "--budget-bytes", &budget_str, "--json"],
    )
    .expect_err("must fail with budget_exceeded");

    assert_eq!(err, GeoCmdError::BudgetExceeded { needed: expected_needed, budget: tight_budget });
    assert_eq!(err.code(), "budget_exceeded");
    assert!(err.to_string().contains(&expected_needed.to_string()));
    assert_eq!(proj.cache.len(), 0, "no index allocated or cached on budget failure");
    assert_eq!(proj.activity_log.last().unwrap().outcome, "budget_exceeded");

    // Also verify a 1 MiB (1_048_576 B) budget fails on a 300k-point index (> 1 MiB).
    let one_mib: u64 = 1024 * 1024;
    let big_pts: Vec<[f64; 3]> = (0..300_000).map(|i| [i as f64 * 1e-6, 0.0, 0.0]).collect();
    let big_cloud = PointCloud::from_points(frame_a(), &big_pts).unwrap();
    let big_id = proj.register(GeometryElement::PointCloud(big_cloud), 30);
    let big_needed = estimate_bytes(IndexKind::KdTree, 300_000, BuildParams::kdtree());
    assert!(big_needed > one_mib);
    let big_str = big_id.to_string();
    let err_1m = invoke_index_cli(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &mut proj.activity_log,
        &["build-index", &big_str, "--kind", "point", "--budget-bytes", "1048576", "--json"],
    )
    .expect_err("1 MiB budget must fail on 300k-point index");
    assert_eq!(err_1m, GeoCmdError::BudgetExceeded { needed: big_needed, budget: one_mib });

    // With sufficient budget, build succeeds and a second call hits cache.
    let ok = build_index(
        &proj.tree,
        &mut proj.cache,
        &BuildIndexParams { element: elem_id, kind: BuildKind::Point, budget_bytes: Some(expected_needed) },
    )
    .expect("sufficient budget succeeds");
    assert!(!ok.hit);
    assert!(ok.bytes <= expected_needed);

    let hit = build_index(
        &proj.tree,
        &mut proj.cache,
        &BuildIndexParams { element: elem_id, kind: BuildKind::Point, budget_bytes: Some(expected_needed) },
    )
    .expect("second call hits cache");
    assert!(hit.hit);
    assert_eq!(hit.bytes, ok.bytes);
}

/// AC-4 (error path): Given query points tagged with another frame, querying fails with
/// `frame_mismatch` (for both inline points, selection handles, and box queries).
#[test]
fn ac4_frame_mismatch_fails_closed() {
    let ps = PointSet::from_points(frame_a(), &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]).unwrap();
    let mut proj = TestProject::new();
    let elem_id = proj.register(GeometryElement::PointSet(ps), 4);

    // Inline frame mismatch on kNN.
    let err = query_knn(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &QueryKnnParams {
            element: elem_id,
            query: QueryPoints::Inline { points: vec![[0.0, 0.0, 0.0]], frame: Some(frame_b()) },
            k: 1,
        },
    )
    .expect_err("frame mismatch");
    assert_eq!(err, GeoCmdError::FrameMismatch { expected: frame_a(), found: frame_b() });
    assert_eq!(err.code(), "frame_mismatch");

    // Selection handle frame mismatch on radius query.
    let sel_id = Uuid::parse("0190a5b2-7c3d-7000-8000-000000000099").unwrap();
    proj.selections.insert(sel_id, SelectionPoints { frame: frame_b(), points: vec![[0.0, 0.0, 0.0]] });
    let err = query_radius(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &QueryRadiusParams {
            element: elem_id,
            query: QueryPoints::Selection(sel_id),
            radius: AuthoredLength::parse_str("1m").unwrap(),
        },
    )
    .expect_err("selection frame mismatch");
    assert_eq!(err.code(), "frame_mismatch");

    // Box query frame mismatch.
    let err = query_box(
        &proj.tree,
        &mut proj.cache,
        &QueryBoxParams { element: elem_id, min: [0.0, 0.0, 0.0], max: [1.0, 1.0, 1.0], frame: Some(frame_b()) },
    )
    .expect_err("box frame mismatch");
    assert_eq!(err.code(), "frame_mismatch");
}

/// AC-5: The generated manifest lists all four MCP tools as exposed (`R1`, `D0`, `cli` + `mcp`)
/// and every params/result schema passes `fid_registry::check_schema`.
#[test]
fn ac5_generated_manifest_lists_four_exposed_mcp_tools() {
    let defs = command_defs().expect("command_defs");
    let geo = Catalogue::geo_frame().unwrap();
    for d in &defs {
        assert_eq!(fid_registry::check_schema(&d.params_schema, &geo), vec![], "params schema of {}", d.id);
        assert_eq!(fid_registry::check_schema(&d.result_schema, &geo), vec![], "result schema of {}", d.id);
    }

    let decls: Vec<CommandDecl> = defs.iter().map(CommandDecl::from_def).collect();
    let export = fid_registry::export_with(&decls, &geo).expect("export manifest");
    let CValue::Obj(top) = parse(&export.bytes).unwrap() else { panic!() };
    let Some(CValue::Arr(commands)) = top.get("commands") else { panic!() };

    let expected = [
        ("geo.build_index", BUILD_INDEX, MCP_BUILD_INDEX),
        ("geo.query_knn", QUERY_KNN, MCP_QUERY_KNN),
        ("geo.query_radius", QUERY_RADIUS, MCP_QUERY_RADIUS),
        ("geo.query_box", QUERY_BOX, MCP_QUERY_BOX),
    ];

    for (id, full_id, mcp_name) in expected {
        assert_eq!(format!("{id}@1"), full_id);
        assert_eq!(tool_name(id, 1), mcp_name);
        let found = commands
            .iter()
            .find_map(|c| match c {
                CValue::Obj(o) if o.get("id") == Some(&CValue::Str(id.into())) => Some(o),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing command {id} in manifest"));
        assert_eq!(found.get("major"), Some(&CValue::Num(1.0)));
        assert_eq!(found.get("side_effect"), Some(&CValue::Str("R1".into())));
        assert_eq!(found.get("determinism"), Some(&CValue::Str("D0".into())));
        assert_eq!(found.get("mcp_projection"), Some(&CValue::Str("exposed".into())));
        let Some(CValue::Arr(exp)) = found.get("exposure") else { panic!() };
        assert!(exp.contains(&CValue::Str("cli".into())));
        assert!(exp.contains(&CValue::Str("mcp".into())));
    }
}

/// REQ-004: More than 10 000 inline query points fails with `too_many_queries`.
#[test]
fn inline_points_over_limit_fail_with_too_many_queries() {
    let ps = PointSet::from_points(frame_a(), &[[0.0, 0.0, 0.0]]).unwrap();
    let mut proj = TestProject::new();
    let elem_id = proj.register(GeometryElement::PointSet(ps), 5);

    let err = query_knn(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &QueryKnnParams {
            element: elem_id,
            query: QueryPoints::Inline { points: vec![[0.0, 0.0, 0.0]; MAX_INLINE_QUERIES + 1], frame: None },
            k: 1,
        },
    )
    .expect_err("too many queries");
    assert_eq!(err, GeoCmdError::TooManyQueries { count: MAX_INLINE_QUERIES + 1, max: MAX_INLINE_QUERIES });
    assert_eq!(err.code(), "too_many_queries");
}

/// Box query, authored length units (`mm`, `um`, `in`), selection handle, and primitive BVH build.
#[test]
fn box_query_selection_handle_authored_units_and_primitive_bvh() {
    let verts = vec![[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.0, 0.01, 0.0], [0.05, 0.05, 0.05]];
    let tris = vec![[0u64, 1, 2]];
    let mesh = Mesh::from_points(frame_a(), &verts, &tris).unwrap();
    let mut proj = TestProject::new();
    let elem_id = proj.register(GeometryElement::Mesh(mesh), 6);

    // Primitive BVH build succeeds for Mesh.
    let bvh_res = build_index(
        &proj.tree,
        &mut proj.cache,
        &BuildIndexParams { element: elem_id, kind: BuildKind::Primitive, budget_bytes: None },
    )
    .expect("primitive bvh build");
    assert_eq!(bvh_res.kind, BuildKind::Primitive);
    assert!(!bvh_res.hit);

    // Radius query using authored unit `12mm` (0.012 m) via selection handle finds vertices 0, 1, 2.
    let sel_id = Uuid::parse("0190a5b2-7c3d-7000-8000-000000000055").unwrap();
    proj.selections.insert(sel_id, SelectionPoints { frame: frame_a(), points: vec![[0.0, 0.0, 0.0]] });
    let r_res = query_radius(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &QueryRadiusParams {
            element: elem_id,
            query: QueryPoints::Selection(sel_id),
            radius: AuthoredLength::parse_str("12mm").unwrap(),
        },
    )
    .expect("radius query via selection");
    assert!(!r_res.truncated);
    let idxs: Vec<u64> = r_res.results[0].iter().map(|h| h.index).collect();
    assert_eq!(idxs, vec![0, 1, 2]);

    // Box query CLI & MCP parity.
    let elem_str = elem_id.to_string();
    let cli_box = invoke_index_cli(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &mut proj.activity_log,
        &["query-box", &elem_str, "--min", "0,0,0", "--max", "0.02,0.02,0.02", "--json"],
    )
    .expect("cli query-box");
    let mcp_box = invoke_index_mcp(
        &proj.tree,
        &proj.selections,
        &mut proj.cache,
        &mut proj.activity_log,
        MCP_QUERY_BOX,
        format!(r#"{{"element":"{elem_str}","max":[0.02,0.02,0.02],"min":[0,0,0]}}"#).as_bytes(),
    )
    .expect("mcp query-box");
    assert_eq!(cli_box, mcp_box);
    let CValue::Obj(root) = parse(&cli_box).unwrap() else { panic!() };
    let Some(CValue::Arr(outer)) = root.get("results") else { panic!() };
    let CValue::Arr(hits) = &outer[0] else { panic!() };
    assert_eq!(hits.len(), 3);
}
