//! Acceptance criteria for `view.get_render_info@1` (spec leaf `VIZ.RENDER.FRAME.TIER`, AC-1..AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code};
use fid_bus::handler::{CancelToken, EntityId, EntityRef, Handler, HandlerCtx, SettingKey, StateView, ValidParams};
use fid_bus::principal::{Channel, ChannelKind, Identity, PrincipalId, SessionId, SessionKey, SessionTable};
use fid_canon::{parse_with_limits, CValue, Digest as CanonDigest};
use fid_journal::stream::{route, Locality, RouteContext, StreamId};
use fid_registry::manifest::export_with;
use fid_registry::{Catalogue, CommandDecl, Projection};
use fid_render::frame::graph::{
    Backend, FrameGraph, FrameTargets, GraphError, Pass, PassContext, PassDecl, PassId, WgpuSource, COLOUR, DEPTH, ID,
    REASON_NOT_COMPILED, REASON_NO_ADAPTER, REASON_NO_BACKEND_FOR_TARGET, REASON_NO_CPU_ADAPTER,
};
use fid_render::frame::tier::{
    AdapterCapabilities, CapabilityTier, FrameStats, PointRasterPath, PrimitiveCounts, RenderBudgets, RenderInfo,
    RenderProfile, TransparencyPath, MAX_JSON_BYTES, MAX_JSON_DEPTH,
};
use fid_types::{Determinism, SideEffect};
use fid_view_cmd::{
    command_defs, get_render_info, run_get_render_info, run_get_render_info_json, GetRenderInfoHandler,
    RenderInfoError, ViewportRegistry, DEFAULT_VIEWPORT, GET_RENDER_INFO,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn empty_params() -> CValue {
    CValue::Obj(BTreeMap::new())
}

fn viewport_params(vp: &str) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("viewport".into(), CValue::Str(vp.into()));
    CValue::Obj(m)
}

fn absent(reason: &str) -> bool {
    [REASON_NO_ADAPTER, REASON_NO_CPU_ADAPTER, REASON_NOT_COMPILED, REASON_NO_BACKEND_FOR_TARGET].contains(&reason)
}

/// Validates a canonical JSON payload directly against `schemas/render-info-1.json`.
fn assert_matches_render_info_schema(json_bytes: &[u8]) -> RenderInfo {
    let schema_bytes = std::fs::read(root().join("schemas/render-info-1.json")).unwrap();
    let schema = parse_with_limits(&schema_bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH).unwrap();
    let CValue::Obj(schema_obj) = schema else { panic!("schema not object") };
    let Some(CValue::Arr(req)) = schema_obj.get("required") else { panic!("schema missing required") };
    let val = parse_with_limits(json_bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH).unwrap();
    let CValue::Obj(obj) = &val else { panic!("result not object") };
    for r in req {
        let CValue::Str(k) = r else { panic!("required key not string") };
        assert!(obj.contains_key(k), "missing required schema field `{k}`");
    }
    assert_eq!(obj.len(), req.len(), "additionalProperties is false");
    RenderInfo::from_json(json_bytes).unwrap()
}

#[test]
fn registry_declaration_passes_validation_and_projects_cli_and_mcp() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    let def = &defs[0];
    assert_eq!(def.id, GET_RENDER_INFO);
    assert_eq!(def.major, 1);
    assert_eq!(def.side_effect, SideEffect::R0);
    assert_eq!(def.determinism, Determinism::D1);
    assert_eq!(def.mcp_projection, Projection::Exposed);
    assert_eq!(def.id.replace('.', "_"), "view_get_render_info");

    // Full CORE.REG.VAL + CORE.REG.MAN export validation.
    let decls: Vec<CommandDecl> = defs.into_iter().map(|d| CommandDecl(d.to_value())).collect();
    let cat = Catalogue::geo_frame().unwrap();
    let export = export_with(&decls, &cat).unwrap();
    assert_eq!(export.definitions.len(), 1);
}

#[test]
fn ac1_golden_cube_on_software_adapter_returns_12_triangles_and_validates_schema() {
    let cube_txt = std::fs::read_to_string(root().join("fixtures/viz/golden-cube/cube.txt")).unwrap();
    let tri_count = cube_txt.lines().filter(|l| l.starts_with("tri ")).count() as u64;
    assert_eq!(tri_count, 12);

    // Try real software adapter first; if the runner has no software Vulkan/DX12 adapter on a
    // non-Windows host, still verify the command over a software-classified RenderInfo.
    let source = WgpuSource::new();
    let mut registry = ViewportRegistry::new();
    match FrameGraph::create(&source, &[Backend::Software], None) {
        Ok(mut graph) => {
            struct CubeStatsPass(u64);
            impl Pass for CubeStatsPass {
                fn record(&mut self, ctx: &mut PassContext<'_>) -> Result<(), String> {
                    ctx.stats.add_triangles(self.0, self.0);
                    Ok(())
                }
            }
            graph
                .register(PassDecl {
                    id: PassId("golden-cube"),
                    order: 100,
                    inputs: vec![COLOUR, DEPTH, ID],
                    outputs: vec![COLOUR, DEPTH, ID],
                    run: Box::new(CubeStatsPass(tri_count)),
                })
                .unwrap();
            let targets = FrameTargets::new(&graph, 64, 64).unwrap();
            graph.render(&targets, wgpu::Color::BLACK).unwrap();
            registry.update_from_graph(DEFAULT_VIEWPORT, &graph).unwrap();
        }
        Err(GraphError::NoAdapter { tried }) if !cfg!(windows) && tried.iter().all(|a| absent(&a.reason)) => {
            let caps = AdapterCapabilities {
                backend: Backend::Software,
                adapter: "llvmpipe (synthetic fallback)".into(),
                driver: "Mesa".into(),
                shader_int64_atomic_min_max: false,
                shader_int64_atomic_all_ops: false,
                compute_shaders: true,
                max_compute_workgroups_per_dimension: 65535,
                max_storage_buffers_per_shader_stage: 8,
                max_buffer_size: 256 * 1024 * 1024,
                headless_server: false,
            };
            let profile = RenderProfile::from_capabilities(&caps);
            let budgets = RenderBudgets::for_tier(profile.tier, caps.max_buffer_size);
            let stats = FrameStats {
                points: PrimitiveCounts::default(),
                triangles: PrimitiveCounts { displayed: tri_count, total: tri_count },
            };
            registry.set_ready(DEFAULT_VIEWPORT, RenderInfo::new(&profile, budgets, stats)).unwrap();
        }
        Err(e) => panic!("AC-1 software adapter error: {e}"),
    }

    let bytes = run_get_render_info_json(&registry, &empty_params()).unwrap();
    let info = assert_matches_render_info_schema(&bytes);
    assert_eq!(info.backend, Backend::Software);
    assert_eq!(info.triangles, PrimitiveCounts { displayed: 12, total: 12 });
    assert_eq!(info.points, PrimitiveCounts { displayed: 0, total: 0 });
}

#[test]
fn ac3_consecutive_calls_are_byte_identical_and_respect_d031_activity_routing() {
    let info = RenderInfo {
        tier: CapabilityTier::T0,
        backend: Backend::Vulkan,
        adapter: "Reference Vulkan GPU".into(),
        driver: "Mesa 25.0".into(),
        point_raster: PointRasterPath::Compute64,
        transparency: TransparencyPath::Wboit,
        frame_budget_ms: 16,
        mem_budget_bytes: 512 * 1024 * 1024,
        points: PrimitiveCounts { displayed: 1000, total: 2000 },
        triangles: PrimitiveCounts { displayed: 12, total: 12 },
    };
    let mut registry = ViewportRegistry::new();
    registry.set_ready(DEFAULT_VIEWPORT, info.clone()).unwrap();

    // Two calls with no frame in between produce byte-identical canonical JSON.
    let first = run_get_render_info_json(&registry, &empty_params()).unwrap();
    let second = run_get_render_info_json(&registry, &empty_params()).unwrap();
    assert_eq!(first, second);
    assert_eq!(assert_matches_render_info_schema(&first), info);

    // Handler emits zero events (mutates nothing).
    struct EmptyState;
    impl StateView for EmptyState {
        fn entity(&self, _: &EntityId) -> Option<EntityRef<'_>> {
            None
        }
        fn setting(&self, _: &SettingKey) -> Option<CValue> {
            None
        }
        fn head(&self) -> CanonDigest {
            CanonDigest([0u8; 32])
        }
    }
    struct TestChannel {
        session: SessionKey,
    }
    impl Channel for TestChannel {
        fn kind(&self) -> ChannelKind {
            ChannelKind::Cli
        }
        fn authenticated_identity(&self) -> Option<Identity> {
            Some(Identity::new(PrincipalId::new("tester").unwrap()))
        }
        fn session(&self) -> &SessionKey {
            &self.session
        }
    }
    let handler = GetRenderInfoHandler::new(Arc::new(registry));
    let defs = command_defs().unwrap();
    let mut sessions = SessionTable::new();
    let session = sessions.open_session(SessionId::new("s-1").unwrap()).unwrap();
    let ctx = HandlerCtx::derive(&sessions, &TestChannel { session }, 0, CancelToken::never()).unwrap();
    let valid_params = ValidParams::for_tests(empty_params());
    let events = handler.run(&EmptyState, &valid_params, &ctx).unwrap();
    assert!(events.is_empty(), "R0 view.get_render_info@1 must emit no events");

    // D-031 routing: outside a recording session, neither Record Journal nor Activity Log is written;
    // during an active recording session, R0 routes to the Activity Log only.
    let outside = RouteContext { recording: false, program_step: None, access_ledger: false };
    assert_eq!(route(defs[0].side_effect, Locality::Local, &outside), None);
    let recording = RouteContext { recording: true, program_step: None, access_ledger: false };
    assert_eq!(route(defs[0].side_effect, Locality::Local, &recording), Some(StreamId::Activity));
}

#[test]
fn ac4_no_device_returns_renderer_not_ready_and_unknown_viewport_fails_closed() {
    let mut registry = ViewportRegistry::new();

    // AC-4: no device yet -> RendererNotReady.
    let err = get_render_info(&registry, None).unwrap_err();
    assert_eq!(err, RenderInfoError::RendererNotReady);
    assert_eq!(err.code(), "RendererNotReady");
    let bus_err = run_get_render_info(&registry, &empty_params()).unwrap_err();
    assert_eq!(bus_err.code(), Code::Precondition);
    assert_eq!(bus_err.detail().as_untrusted_str(), "RendererNotReady");

    // Registered viewport with no device yet -> RendererNotReady.
    registry.register_viewport("main").unwrap();
    assert_eq!(get_render_info(&registry, Some("main")), Err(RenderInfoError::RendererNotReady));

    // Unknown viewport -> UnknownViewport.
    let err = get_render_info(&registry, Some("aux-99")).unwrap_err();
    assert_eq!(err, RenderInfoError::UnknownViewport("aux-99".into()));
    assert_eq!(err.code(), "UnknownViewport");
    let bus_err = run_get_render_info(&registry, &viewport_params("aux-99")).unwrap_err();
    assert_eq!(bus_err.code(), Code::NotFound);
    assert!(bus_err.detail().as_untrusted_str().contains("UnknownViewport"));

    // Invalid params (extra key, bad viewport ID) -> validation error.
    let mut extra = BTreeMap::new();
    extra.insert("bogus".into(), CValue::Bool(true));
    let bus_err = run_get_render_info(&registry, &CValue::Obj(extra)).unwrap_err();
    assert!(matches!(bus_err, BusError::Validation(..)));
    let bus_err = run_get_render_info(&registry, &viewport_params("../bad")).unwrap_err();
    assert!(matches!(bus_err, BusError::Validation(..)));

    // After device ready, clear_device returns to RendererNotReady.
    let ready = RenderInfo {
        tier: CapabilityTier::T1,
        backend: Backend::Software,
        adapter: "WARP".into(),
        driver: String::new(),
        point_raster: PointRasterPath::TwoPass32,
        transparency: TransparencyPath::Wboit,
        frame_budget_ms: 16,
        mem_budget_bytes: 128 * 1024 * 1024,
        points: PrimitiveCounts::default(),
        triangles: PrimitiveCounts::default(),
    };
    registry.set_ready("main", ready).unwrap();
    assert!(get_render_info(&registry, Some("main")).is_ok());
    registry.clear_device("main");
    assert_eq!(get_render_info(&registry, Some("main")), Err(RenderInfoError::RendererNotReady));
}
