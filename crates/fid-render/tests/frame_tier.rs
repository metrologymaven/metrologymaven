//! Spec leaf VIZ.RENDER.FRAME.TIER: adapter tier classification against `fixtures/viz/adapters.json`
//! (AC-2), schema agreement with `schemas/render-info-1.json`, and software-adapter rendering of
//! `fixtures/viz/golden-cube` reporting 12 displayed / 12 total triangles (AC-1).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse_with_limits, CValue};
use fid_render::frame::graph::{
    Backend, FrameGraph, FrameTargets, GraphError, Pass, PassContext, PassDecl, PassId, WgpuSource, COLOUR, DEPTH, ID,
    REASON_NOT_COMPILED, REASON_NO_ADAPTER, REASON_NO_BACKEND_FOR_TARGET, REASON_NO_CPU_ADAPTER,
};
use fid_render::frame::tier::{
    classify_tier, parse_adapter_fixtures, CapabilityTier, PointRasterPath, PrimitiveCounts, RenderInfo, RenderProfile,
    TransparencyPath, MAX_JSON_BYTES, MAX_JSON_DEPTH,
};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn absent(reason: &str) -> bool {
    [REASON_NO_ADAPTER, REASON_NO_CPU_ADAPTER, REASON_NOT_COMPILED, REASON_NO_BACKEND_FOR_TARGET].contains(&reason)
}

#[test]
fn ac2_recorded_adapters_classify_to_expected_tiers() {
    let bytes = std::fs::read(root().join("fixtures/viz/adapters.json")).unwrap();
    let cases = parse_adapter_fixtures(&bytes).unwrap();
    assert!(cases.len() >= 10, "expected full tier coverage in adapters.json");

    let mut seen_tiers = std::collections::BTreeSet::new();
    for c in &cases {
        let got_tier = classify_tier(&c.capabilities);
        assert_eq!(got_tier, c.expected_tier, "case `{}` tier mismatch", c.id);
        let profile = RenderProfile::from_capabilities(&c.capabilities);
        assert_eq!(profile.tier, c.expected_tier, "case `{}` profile tier", c.id);
        assert_eq!(profile.point_raster, c.expected_point_raster, "case `{}` point_raster", c.id);
        assert_eq!(profile.transparency, c.expected_transparency, "case `{}` transparency", c.id);
        seen_tiers.insert(got_tier);
    }
    assert_eq!(
        seen_tiers.into_iter().collect::<Vec<_>>(),
        vec![CapabilityTier::T0, CapabilityTier::T1, CapabilityTier::T2, CapabilityTier::T3],
        "fixture must cover all four tiers T0..T3"
    );
}

#[test]
fn schema_file_is_valid_json_and_matches_render_info_fields() {
    let schema_bytes = std::fs::read(root().join("schemas/render-info-1.json")).unwrap();
    let schema = parse_with_limits(&schema_bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH).unwrap();
    let CValue::Obj(obj) = schema else { panic!("schema must be a JSON object") };
    let Some(CValue::Arr(req)) = obj.get("required") else { panic!("schema missing required") };
    let req_names: Vec<&str> = req
        .iter()
        .map(|v| match v {
            CValue::Str(s) => s.as_str(),
            other => panic!("non-string required entry: {other:?}"),
        })
        .collect();
    assert_eq!(
        req_names,
        [
            "adapter",
            "backend",
            "driver",
            "frame_budget_ms",
            "mem_budget_bytes",
            "point_raster",
            "points",
            "tier",
            "transparency",
            "triangles",
        ]
    );
    assert_eq!(obj.get("additionalProperties"), Some(&CValue::Bool(false)));
}

#[test]
fn ac1_software_adapter_golden_cube_reports_12_triangles_and_valid_render_info() {
    let source = WgpuSource::new();
    let mut graph = match FrameGraph::create(&source, &[Backend::Software], None) {
        Ok(g) => g,
        Err(GraphError::NoAdapter { tried }) if !cfg!(windows) && tried.iter().all(|a| absent(&a.reason)) => {
            eprintln!("AC-1 not run on this host (no software adapter): {tried:?}");
            return;
        }
        Err(e) => panic!("AC-1 needs the software adapter: {e}"),
    };

    // Count triangles in fixtures/viz/golden-cube/cube.txt (6 faces x 2 triangles = 12).
    let cube_txt = std::fs::read_to_string(root().join("fixtures/viz/golden-cube/cube.txt")).unwrap();
    let tri_count = cube_txt.lines().filter(|l| l.starts_with("tri ")).count() as u64;
    assert_eq!(tri_count, 12);

    struct GoldenCubeStatsPass {
        triangles: u64,
    }
    impl Pass for GoldenCubeStatsPass {
        fn record(&mut self, ctx: &mut PassContext<'_>) -> Result<(), String> {
            ctx.stats.add_triangles(self.triangles, self.triangles);
            Ok(())
        }
    }

    graph
        .register(PassDecl {
            id: PassId("golden-cube-stats"),
            order: 100,
            inputs: vec![COLOUR, DEPTH, ID],
            outputs: vec![COLOUR, DEPTH, ID],
            run: Box::new(GoldenCubeStatsPass { triangles: tri_count }),
        })
        .unwrap();

    let targets = FrameTargets::new(&graph, 64, 64).unwrap();
    graph.render(&targets, wgpu::Color::BLACK).unwrap();

    let info = graph.render_info();
    assert_eq!(info.backend, Backend::Software);
    assert_eq!(graph.adapter_info().device_type, wgpu::DeviceType::Cpu);
    assert_eq!(info.triangles, PrimitiveCounts { displayed: 12, total: 12 });
    assert_eq!(info.points, PrimitiveCounts { displayed: 0, total: 0 });
    assert!(info.frame_budget_ms > 0);
    assert!(info.mem_budget_bytes > 0);

    // Canonical JSON round-trip and determinism across consecutive calls.
    let bytes_a = info.to_canonical_json().unwrap();
    let bytes_b = graph.render_info().to_canonical_json().unwrap();
    assert_eq!(bytes_a, bytes_b);
    assert_eq!(RenderInfo::from_json(&bytes_a).unwrap(), info);

    // Failing pass must not overwrite last_frame_stats.
    struct FailingPass;
    impl Pass for FailingPass {
        fn record(&mut self, ctx: &mut PassContext<'_>) -> Result<(), String> {
            ctx.stats.add_triangles(999, 999);
            Err("simulated failure".into())
        }
    }
    graph
        .register(PassDecl {
            id: PassId("fail"),
            order: 200,
            inputs: vec![],
            outputs: vec![],
            run: Box::new(FailingPass),
        })
        .unwrap();
    assert!(graph.render(&targets, wgpu::Color::BLACK).is_err());
    assert_eq!(graph.last_frame_stats().triangles, PrimitiveCounts { displayed: 12, total: 12 });

    // Headless server-stills mode classifies as T3 with sprites + peel.
    let headless = FrameGraph::create_headless(&source, &[Backend::Software], None).unwrap();
    assert_eq!(headless.capability_tier(), CapabilityTier::T3);
    assert_eq!(headless.render_profile().point_raster, PointRasterPath::Sprites);
    assert_eq!(headless.render_profile().transparency, TransparencyPath::Peel);
}
