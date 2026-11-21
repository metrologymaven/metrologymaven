//! Spec leaf VIZ.RENDER.FRAME.GRAPH: backend order and selection against a scripted fake
//! adapter source (AC-2, AC-3, AC-5), the policy reader on the shipped and recorded policy
//! files, pass registration (AC-4), and one real frame of `fixtures/viz/golden-cube` on the
//! software adapter with ID and depth readback (AC-1). Expected pixels, IDs and depths come
//! from `fixtures/viz/golden-cube/make_fixtures.py` (independent Python reference).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_render::frame::graph::{
    order_or_software, policy_order, policy_order_from_file, resolve_order, select, AdapterChoice, AdapterSource,
    Attempt, Backend, FrameGraph, FrameTargets, GraphError, Host, Pass, PassContext, PassDecl, PassId, PassList,
    PolicyError, Rect, TargetId, WgpuSource, BACKEND_SETTING, COLOUR, DEPTH, ID, ID_FORMAT, REASON_CHOICE_ABSENT,
    REASON_NOT_COMPILED, REASON_NO_ADAPTER, REASON_NO_BACKEND_FOR_TARGET, REASON_NO_CPU_ADAPTER,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(p: &str) -> Vec<u8> {
    std::fs::read(root().join(p)).unwrap()
}

fn host(os: &str, arch: &str) -> Host {
    Host { os: os.into(), arch: arch.into() }
}

/// A scripted adapter source: each backend yields its scripted result; unscripted backends
/// have no adapter. Records every request.
struct Fake {
    script: BTreeMap<Backend, Result<&'static str, &'static str>>,
    calls: std::cell::RefCell<Vec<(Backend, Option<AdapterChoice>)>>,
}

impl Fake {
    fn new(script: &[(Backend, Result<&'static str, &'static str>)]) -> Fake {
        Fake { script: script.iter().cloned().collect(), calls: Default::default() }
    }
}

impl AdapterSource for Fake {
    type Adapter = &'static str;
    fn request(&self, b: Backend, c: Option<&AdapterChoice>) -> Result<&'static str, String> {
        self.calls.borrow_mut().push((b, c.cloned()));
        match self.script.get(&b) {
            Some(Ok(a)) => Ok(a),
            Some(Err(e)) => Err((*e).to_string()),
            None => Err("no adapter".into()),
        }
    }
}

#[test]
fn shipped_policy_gives_each_host_its_row() {
    let bytes = fixture("platform/gpu-policy.toml");
    let b = |s: &str| Backend::parse(s).unwrap();
    assert_eq!(policy_order(&bytes, &host("windows", "x86_64")).unwrap(), [b("dx12"), b("vulkan"), b("software")]);
    assert_eq!(policy_order(&bytes, &host("linux", "x86_64")).unwrap(), [b("vulkan"), b("software")]);
    assert_eq!(policy_order(&bytes, &host("linux", "aarch64")).unwrap(), [b("vulkan"), b("software")]);
    assert_eq!(policy_order(&bytes, &host("macos", "aarch64")).unwrap(), [b("metal"), b("software")]);
    let missing = policy_order(&bytes, &host("macos", "x86_64")).unwrap_err();
    assert_eq!(missing, PolicyError::NoRow { os: "macos".into(), arch: "x86_64".into() });
    // Safe default of PLAT.GPU.POLICY: no row (or no valid file) means software only.
    assert_eq!(order_or_software(Err(missing.clone())), (vec![Backend::Software], Some(missing)));
    let from_file = policy_order_from_file(&root().join("platform/gpu-policy.toml"), &host("linux", "x86_64"));
    assert_eq!(from_file.unwrap(), [Backend::Vulkan, Backend::Software]);
    assert!(matches!(policy_order_from_file(&root().join("no/such.toml"), &Host::current()), Err(PolicyError::Io(_))));
}

/// A malformed policy text and the check its error must pass.
type PolicyCase = (&'static str, fn(&PolicyError) -> bool);

/// Review of #368: every row is checked; a backend listed twice and a row not ending in
/// `software` are schema errors (REQ-PLAT.GPU.POLICY-004), so `order_or_software` falls back.
const MORE_CASES: &[PolicyCase] = &[
    (
        "schema_version = 1
[[row]]
os='linux'
arch='x86_64'
backends=['vulkan','vulkan','software']",
        |e| matches!(e, PolicyError::Schema(_)),
    ),
    (
        "schema_version = 1
[[row]]
os='linux'
arch='x86_64'
backends=['vulkan','gl']",
        |e| matches!(e, PolicyError::Schema(_)),
    ),
    (
        "schema_version = 1
[[row]]
os='linux'
arch='x86_64'
backends=['vulkan','software']
[[row]]
os='windows'
arch='x86_64'
backends=['dx13','software']",
        |e| *e == PolicyError::UnknownBackend("dx13".into()),
    ),
    (
        "schema_version = 1
[[row]]
os='linux'
arch='x86_64'
backends=['vulkan','software']
[[row]]
os='windows'
arch='x86_64'
backends=['dx12']",
        |e| matches!(e, PolicyError::Schema(_)),
    ),
];

#[test]
fn malformed_policies_are_refused() {
    let h = host("linux", "x86_64");
    let cases: &[PolicyCase] = &[
        ("schema_version = 2\n[[row]]\nos='linux'\narch='x86_64'\nbackends=['vulkan']", |e| {
            matches!(e, PolicyError::Schema(_))
        }),
        ("[[row]]\nos='linux'\narch='x86_64'\nbackends=['vulkan']", |e| matches!(e, PolicyError::Schema(_))),
        ("schema_version = 1\nschema_version = 1", |e| matches!(e, PolicyError::Parse(_))),
        ("schema_version = 1\n[[row]]\nos='linux'\narch='x86_64'\nbackends=['vulcan','software']", |e| {
            *e == PolicyError::UnknownBackend("vulcan".into())
        }),
        ("schema_version = 1\n[[row]]\nos='linux'\narch='x86_64'\nbackends=[]", |e| {
            matches!(e, PolicyError::Schema(_))
        }),
        ("schema_version = 1\n[[row]]\nos='linux'\nbackends=['gl']", |e| matches!(e, PolicyError::Schema(_))),
        ("schema_version = 1\n[[row]]\nos='linux'\narch='x86_64'\nbackends=[1]", |e| {
            matches!(e, PolicyError::Schema(_))
        }),
    ];
    for (text, ok) in cases.iter().chain(MORE_CASES) {
        let e = policy_order(text.as_bytes(), &h).unwrap_err();
        assert!(ok(&e), "{text}: {e:?}");
    }
    let dup = policy_order(&fixture("fixtures/viz/frame-graph/policy-duplicate-row.toml"), &h).unwrap_err();
    assert_eq!(dup, PolicyError::DuplicateRow { os: "linux".into(), arch: "x86_64".into() });
    let big = vec![b' '; fid_render::frame::graph::backend::MAX_POLICY_BYTES + 1];
    assert_eq!(policy_order(&big, &h).unwrap_err(), PolicyError::TooLarge);
    assert!(matches!(policy_order(&[0xff, 0xfe], &h).unwrap_err(), PolicyError::Parse(_)));
}

/// AC-2: the recorded policy lists [vulkan, gl]; Vulkan has no adapter, so GL is selected and
/// Vulkan's failure reason is recorded.
#[test]
fn ac2_falls_back_to_gl_and_records_the_vulkan_reason() {
    let policy =
        policy_order(&fixture("fixtures/viz/frame-graph/policy-vulkan-gl.toml"), &host("linux", "x86_64")).unwrap();
    assert_eq!(policy, [Backend::Vulkan, Backend::Gl, Backend::Software]);
    let fake = Fake::new(&[(Backend::Vulkan, Err("vkEnumeratePhysicalDevices: 0 devices")), (Backend::Gl, Ok("gl0"))]);
    let order = resolve_order(&policy, None, None).unwrap();
    let s = select(&fake, &order, None).unwrap();
    assert_eq!((s.backend, s.adapter), (Backend::Gl, "gl0"));
    assert_eq!(s.tried, [Attempt { backend: Backend::Vulkan, reason: "vkEnumeratePhysicalDevices: 0 devices".into() }]);
}

/// AC-3: no adapters at all: `NoAdapter` names every backend tried, in order, with its reason.
#[test]
fn ac3_no_adapter_names_every_backend_tried() {
    let fake = Fake::new(&[(Backend::Dx12, Err("no DXGI adapter")), (Backend::Software, Err("no CPU adapter"))]);
    let order = [Backend::Dx12, Backend::Vulkan, Backend::Software];
    let e = select(&fake, &order, None).unwrap_err();
    let GraphError::NoAdapter { tried } = &e else { panic!("{e:?}") };
    let got: Vec<(Backend, &str)> = tried.iter().map(|a| (a.backend, a.reason.as_str())).collect();
    assert_eq!(
        got,
        [(Backend::Dx12, "no DXGI adapter"), (Backend::Vulkan, "no adapter"), (Backend::Software, "no CPU adapter")]
    );
    let text = e.to_string();
    assert!(text.starts_with("no_adapter"), "{text}");
    for b in ["dx12", "vulkan", "software"] {
        assert!(text.contains(b), "{text}");
    }
    assert_eq!(select(&fake, &[], None).unwrap_err(), GraphError::NoAdapter { tried: vec![] });
}

/// AC-5: `viz.render.backend = gl` makes GL first even when the policy prefers another backend;
/// an injected adapter choice comes before the setting; `auto` keeps the policy order.
#[test]
fn ac5_setting_and_choice_override_the_policy() {
    assert_eq!(BACKEND_SETTING, "viz.render.backend");
    let policy = [Backend::Vulkan, Backend::Software];
    let fake = Fake::new(&[(Backend::Vulkan, Ok("vk0")), (Backend::Gl, Ok("gl0")), (Backend::Dx12, Ok("dx0"))]);
    let order = resolve_order(&policy, Some("gl"), None).unwrap();
    assert_eq!(order, [Backend::Gl, Backend::Vulkan, Backend::Software]);
    assert_eq!(select(&fake, &order, None).unwrap().backend, Backend::Gl);
    assert_eq!(resolve_order(&policy, Some("auto"), None).unwrap(), policy);
    assert_eq!(resolve_order(&policy, None, None).unwrap(), policy);
    assert_eq!(resolve_order(&policy, Some("vulkan"), None).unwrap(), policy);
    assert_eq!(resolve_order(&policy, Some("GL"), None).unwrap_err(), GraphError::BadSetting("GL".into()));
    let choice = AdapterChoice { backend: Backend::Dx12, vendor: Some(0x1414), device: Some(0x8c), name: None };
    let order = resolve_order(&policy, Some("gl"), Some(&choice)).unwrap();
    assert_eq!(order, [Backend::Dx12, Backend::Gl, Backend::Vulkan, Backend::Software]);
    let s = select(&fake, &order, Some(&choice)).unwrap();
    assert_eq!(s.backend, Backend::Dx12);
    // The choice is passed only with its own backend.
    let calls = fake.calls.borrow();
    assert_eq!(calls.last().unwrap(), &(Backend::Dx12, Some(choice.clone())));
    drop(calls);
    let fake = Fake::new(&[(Backend::Vulkan, Ok("vk0"))]);
    let s = select(&fake, &order, Some(&choice)).unwrap();
    assert_eq!(s.backend, Backend::Vulkan);
    assert!(fake.calls.borrow().iter().filter(|(b, _)| *b != Backend::Dx12).all(|(_, c)| c.is_none()));
}

#[test]
fn backend_names_round_trip() {
    for b in Backend::ALL {
        assert_eq!(Backend::parse(b.as_str()), Some(b));
        assert_eq!(b.to_string(), b.as_str());
    }
    assert_eq!(Backend::parse("Vulkan"), None);
    assert_eq!(Backend::Software.wgpu_backends(), wgpu::Backends::all());
}

struct Noop;
impl Pass for Noop {
    fn record(&mut self, _: &mut PassContext<'_>) -> Result<(), String> {
        Ok(())
    }
}

fn decl(id: &'static str, order: u32, inputs: &[&'static str], outputs: &[&'static str]) -> PassDecl {
    PassDecl {
        id: PassId(id),
        order,
        inputs: inputs.iter().map(|s| TargetId(s)).collect(),
        outputs: outputs.iter().map(|s| TargetId(s)).collect(),
        run: Box::new(Noop),
    }
}

/// AC-4 and the registration rules: an input no earlier pass writes is `UnresolvedInput`;
/// producers must have a lower order key; IDs and order keys are unique; passes run in
/// ascending order key whatever the registration order.
#[test]
fn ac4_pass_registration() {
    let mut l = PassList::new();
    let e = l.register(decl("ssao", 20, &["normals"], &["occlusion"])).unwrap_err();
    assert_eq!(e, GraphError::UnresolvedInput { pass: PassId("ssao"), input: TargetId("normals") });
    assert!(e.to_string().starts_with("unresolved_input"), "{e}");
    assert!(l.is_empty(), "a rejected registration changes nothing");
    l.register(decl("points", 10, &[], &[COLOUR.0, DEPTH.0, ID.0, "normals"])).unwrap();
    l.register(decl("ssao", 20, &["normals", "depth"], &["occlusion"])).unwrap();
    // A producer with a higher order key does not count: it runs later.
    l.register(decl("late", 40, &[], &["halo"])).unwrap();
    let e = l.register(decl("glow", 30, &["halo"], &["colour"])).unwrap_err();
    assert_eq!(e, GraphError::UnresolvedInput { pass: PassId("glow"), input: TargetId("halo") });
    // The frame's own targets are always available.
    l.register(decl("overlay", 5, &["colour", "depth", "id"], &["colour"])).unwrap();
    assert_eq!(
        l.register(decl("points", 50, &[], &[])).unwrap_err(),
        GraphError::DuplicatePass { pass: PassId("points") }
    );
    assert_eq!(
        l.register(decl("other", 20, &[], &[])).unwrap_err(),
        GraphError::DuplicateOrder { order: 20, pass: PassId("other"), existing: PassId("ssao") }
    );
    assert_eq!(l.order(), [PassId("overlay"), PassId("points"), PassId("ssao"), PassId("late")]);
    assert_eq!(l.len(), 4);
}

// ------------------------------------------------------------------ AC-1 on a real adapter

struct Tri {
    id: [u32; 2],
    verts: [[f32; 4]; 3],
}

struct Expect {
    px: u32,
    py: u32,
    id: u32,
    prims: [u32; 2],
    depth: f64,
}

struct Cube {
    size: (u32, u32),
    tris: Vec<Tri>,
    expects: Vec<Expect>,
    background: (u32, u32),
}

fn cube() -> Cube {
    let text = String::from_utf8(fixture("fixtures/viz/golden-cube/cube.txt")).unwrap();
    let mut c = Cube { size: (0, 0), tris: vec![], expects: vec![], background: (0, 0) };
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let w: Vec<&str> = line.split(' ').collect();
        let u = |i: usize| w[i].parse::<u32>().unwrap();
        match w[0] {
            "size" => c.size = (u(1), u(2)),
            "tri" => {
                let f: Vec<f32> = w[3..].iter().map(|s| s.parse::<f32>().unwrap()).collect();
                assert_eq!(f.len(), 12);
                let v = |k: usize| [f[4 * k], f[4 * k + 1], f[4 * k + 2], f[4 * k + 3]];
                c.tris.push(Tri { id: [u(1), u(2)], verts: [v(0), v(1), v(2)] });
            }
            "expect" => c.expects.push(Expect {
                px: u(1),
                py: u(2),
                id: u(3),
                prims: [u(4), u(5)],
                depth: w[6].parse().unwrap(),
            }),
            "background" => c.background = (u(1), u(2)),
            other => panic!("unknown line {other}"),
        }
    }
    assert_eq!((c.tris.len(), c.expects.len()), (12, 3));
    c
}

const SHADER: &str = include_str!("../../../fixtures/viz/golden-cube/cube.wgsl");

/// The golden cube's draw pass (test data, not product code).
struct CubePass {
    vertices: Vec<u8>,
    count: u32,
}

impl Pass for CubePass {
    fn record(&mut self, ctx: &mut PassContext<'_>) -> Result<(), String> {
        let device = ctx.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("golden cube"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let attrs = [
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32x2, offset: 16, shader_location: 1 },
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("golden cube"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: ctx.targets.colour_format(),
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState { format: ID_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                ],
            }),
            multiview: None,
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("golden cube vertices"),
            size: self.vertices.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        ctx.queue.write_buffer(&buffer, 0, &self.vertices);
        let load = |view| wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
        };
        let mut rp = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("golden cube"),
            color_attachments: &[Some(load(ctx.targets.colour_view())), Some(load(ctx.targets.id_view()))],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: ctx.targets.depth_view(),
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_pipeline(&pipeline);
        rp.set_vertex_buffer(0, buffer.slice(..));
        rp.draw(0..self.count, 0..1);
        Ok(())
    }
}

fn vertex_bytes(c: &Cube) -> Vec<u8> {
    let mut out = Vec::new();
    for t in &c.tris {
        for v in &t.verts {
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
            out.extend_from_slice(&t.id[0].to_le_bytes());
            out.extend_from_slice(&t.id[1].to_le_bytes());
        }
    }
    out
}

/// AC-1: one frame of the golden cube on the software adapter. Each visible face's centre
/// pixel holds its render ID and one of its two primitive indices, with the reversed-Z depth
/// of the reference; the background pixel holds 0 and depth 0.0. Runs wherever a CPU adapter
/// exists (WARP on Windows); it must exist on Windows. Elsewhere it returns early only on
/// `NoAdapter` whose every reason says no adapter exists or nothing is compiled in; any other
/// error fails the test (review of #368).
#[test]
fn ac1_golden_cube_ids_and_depth() {
    let source = WgpuSource::new();
    // Start-up through the policy path: a policy with no row for the host keeps its error as
    // the warning and falls back to `[software]`.
    let no_row = Err(PolicyError::NoRow { os: "plan9".into(), arch: "mips".into() });
    let mut graph = match FrameGraph::start(&source, no_row.clone(), None, None) {
        Ok(g) => g,
        Err(GraphError::NoAdapter { tried }) if !cfg!(windows) && tried.iter().all(|a| absent(&a.reason)) => {
            eprintln!("AC-1 not run on this host (no software adapter): {tried:?}");
            return;
        }
        Err(e) => panic!("AC-1 needs the software adapter (WARP on Windows): {e}"),
    };
    assert_eq!(graph.policy_warning(), no_row.as_ref().err());
    eprintln!("AC-1 adapter: {:?}", graph.adapter_info());
    assert_eq!(graph.backend(), Backend::Software);
    assert_eq!(graph.adapter_info().device_type, wgpu::DeviceType::Cpu);
    let c = cube();
    graph
        .register(PassDecl {
            id: PassId("golden-cube"),
            order: 100,
            inputs: vec![COLOUR, DEPTH, ID],
            outputs: vec![COLOUR, DEPTH, ID],
            run: Box::new(CubePass { vertices: vertex_bytes(&c), count: 3 * c.tris.len() as u32 }),
        })
        .unwrap();
    let targets = FrameTargets::new(&graph, c.size.0, c.size.1).unwrap();
    graph.render(&targets, wgpu::Color::BLACK).unwrap();
    for e in &c.expects {
        let r = Rect { x: e.px, y: e.py, width: 1, height: 1 };
        let id = graph.read_id(&targets, r).unwrap();
        assert_eq!(id.len(), 1);
        assert_eq!(id[0][0], e.id, "render ID at ({}, {})", e.px, e.py);
        assert!(e.prims.contains(&id[0][1]), "primitive {} at ({}, {}), want {:?}", id[0][1], e.px, e.py, e.prims);
        let d = graph.read_depth(&targets, r).unwrap();
        let rel = (f64::from(d[0]) - e.depth).abs() / e.depth;
        assert!(rel < 1e-4, "depth {} at ({}, {}), reference {}", d[0], e.px, e.py, e.depth);
    }
    let (bx, by) = c.background;
    let bg = Rect { x: bx, y: by, width: 1, height: 1 };
    assert_eq!(graph.read_id(&targets, bg).unwrap(), [[0, 0]]);
    assert_eq!(graph.read_depth(&targets, bg).unwrap(), [0.0]);
    // A whole-frame read: every face's ID appears, nothing else but 0.
    let all = graph.read_id(&targets, Rect { x: 0, y: 0, width: c.size.0, height: c.size.1 }).unwrap();
    assert_eq!(all.len(), (c.size.0 * c.size.1) as usize);
    let mut seen: Vec<u32> = all.iter().map(|p| p[0]).collect();
    seen.sort_unstable();
    seen.dedup();
    let mut want: Vec<u32> = c.expects.iter().map(|e| e.id).collect();
    want.push(0);
    want.sort_unstable();
    assert_eq!(seen, want, "only the visible faces and the background");
    // Rectangles outside the target are refused.
    let out = Rect { x: c.size.0 - 1, y: 0, width: 2, height: 1 };
    assert!(matches!(graph.read_id(&targets, out), Err(GraphError::BadRect { .. })));
    assert!(matches!(
        graph.read_depth(&targets, Rect { x: 0, y: 0, width: 0, height: 1 }),
        Err(GraphError::BadRect { .. })
    ));
    // A failing pass stops the frame.
    struct Fails;
    impl Pass for Fails {
        fn record(&mut self, _: &mut PassContext<'_>) -> Result<(), String> {
            Err("boom".into())
        }
    }
    graph
        .register(PassDecl { id: PassId("fails"), order: 200, inputs: vec![], outputs: vec![], run: Box::new(Fails) })
        .unwrap();
    assert_eq!(
        graph.render(&targets, wgpu::Color::BLACK).unwrap_err(),
        GraphError::PassFailed { pass: PassId("fails"), reason: "boom".into() }
    );
    // A caller-owned colour texture without RENDER_ATTACHMENT is refused.
    let plain = graph.device().create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    assert!(matches!(FrameTargets::with_colour(&graph, plain), Err(GraphError::BadTarget(_))));
    // A wgpu validation error inside a pass is a `Gpu` result, never wgpu's panicking default
    // handler (review of #368); the graph keeps working afterwards.
    struct Invalid;
    impl Pass for Invalid {
        fn record(&mut self, ctx: &mut PassContext<'_>) -> Result<(), String> {
            let _ = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("invalid on purpose"),
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE,
                mapped_at_creation: false,
            });
            Ok(())
        }
    }
    let mut g2 = FrameGraph::create(&source, &[Backend::Software], None).unwrap();
    g2.register(PassDecl { id: PassId("invalid"), order: 1, inputs: vec![], outputs: vec![], run: Box::new(Invalid) })
        .unwrap();
    let t2 = FrameTargets::new(&g2, 8, 8).unwrap();
    assert!(matches!(g2.render(&t2, wgpu::Color::BLACK), Err(GraphError::Gpu(_))));
    assert_eq!(g2.read_id(&t2, Rect { x: 0, y: 0, width: 1, height: 1 }).unwrap(), [[0, 0]]);
    // An injected choice of the software adapter on its own backend is honoured although it is
    // a CPU adapter (review of #368): without the choice that backend skips CPU adapters.
    let info = graph.adapter_info().clone();
    let own = match info.backend {
        wgpu::Backend::Dx12 => Backend::Dx12,
        wgpu::Backend::Vulkan => Backend::Vulkan,
        other => panic!("software adapter on unexpected backend {other:?}"),
    };
    let choice = AdapterChoice { backend: own, vendor: Some(info.vendor), device: Some(info.device), name: None };
    let chosen = FrameGraph::create(&source, &[own], Some(&choice)).unwrap();
    assert_eq!(chosen.backend(), own);
    assert_eq!(chosen.adapter_info().device_type, wgpu::DeviceType::Cpu);
    assert_eq!((chosen.adapter_info().vendor, chosen.adapter_info().device), (info.vendor, info.device));
    let wrong = AdapterChoice {
        backend: own,
        vendor: Some(info.vendor),
        device: Some(info.device),
        name: Some("no such adapter".into()),
    };
    let e = FrameGraph::create(&source, &[own], Some(&wrong)).unwrap_err();
    assert_eq!(e, GraphError::NoAdapter { tried: vec![Attempt { backend: own, reason: REASON_CHOICE_ABSENT.into() }] });
}

/// Reasons that mean no adapter exists on this host or build (the only ones AC-1 may skip on).
fn absent(reason: &str) -> bool {
    [REASON_NO_ADAPTER, REASON_NO_CPU_ADAPTER, REASON_NOT_COMPILED, REASON_NO_BACKEND_FOR_TARGET].contains(&reason)
}
