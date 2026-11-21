//! The frame graph (spec leaf `VIZ.RENDER.FRAME.GRAPH`): backend selection and device
//! creation ([`backend`]), pass registration ([`passes`]), the frame's colour, reversed-Z
//! depth and ID targets ([`FrameTargets`]) and their rectangle readback for picking.
//!
//! Side-effect class R0 (it reads the policy file and draws into GPU memory it owns or the
//! caller hands it); determinism class N for the pixels (GPU output), while the backend order
//! and the pass order are deterministic. No UI-toolkit dependency: the frame is drawn into a
//! `wgpu::Texture` the caller supplies or takes ([`FrameTargets::colour`]).
//!
//! - **Targets** (REQ-004): colour (caller's format, or [`COLOUR_FORMAT`]), depth
//!   [`DEPTH_FORMAT`] with reversed Z (cleared to 0.0, nearer is larger, compare `Greater`)
//!   and ID [`ID_FORMAT`] (`Rg32Uint`: red the render ID, green the primitive or point index;
//!   render ID 0 means no drawable). Every frame clears all three before the first pass.
//! - **Device**: the selected adapter's device is created with no optional feature and the
//!   adapter's own limits (tier classification and narrower limits are `VIZ.RENDER.FRAME.TIER`).
//!   A device that cannot be created counts as a failed attempt and the next backend is tried.
//! - **Errors**: wgpu validation errors of a frame or readback are caught with an error scope
//!   and returned as [`GraphError::Gpu`]; a lost device is [`GraphError::DeviceLost`] on the
//!   next call (recovery is `VIZ.RENDER.RECOVER`).
//! - **Readback** (REQ-005): [`FrameGraph::read_id`] and [`FrameGraph::read_depth`] copy a
//!   pixel rectangle of the ID or depth target to a staging buffer and map it. These are the
//!   only GPU reads in the crate; the staging buffers are declared as ID and depth targets in
//!   `shaders/manifest.toml` (`fid-shlint` REQ-002).

pub mod backend;
pub mod passes;

pub use backend::{
    instance_backends, order_or_software, policy_order, policy_order_from_file, resolve_order, select,
    select_excluding, AdapterChoice, AdapterSource, Attempt, Backend, Host, PolicyError, ProbeExclusions, Selected,
    BACKEND_SETTING, REASON_PROBE_FAILED, REASON_PROBE_TIMEOUT,
};
pub use passes::{Pass, PassContext, PassDecl, PassId, PassList, TargetId, COLOUR, DEPTH, FRAME_TARGETS, ID};

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

/// Colour format of targets the graph creates.
pub const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Depth format (reversed Z).
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// ID format: render ID and primitive or point index.
pub const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Uint;
/// Depth clear value: the far plane under reversed Z.
pub const DEPTH_CLEAR: f32 = 0.0;
/// Most pixels one readback may cover (a 4096 x 4096 rectangle).
pub const MAX_READBACK_PIXELS: u64 = 4096 * 4096;

/// A frame graph error.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphError {
    /// No backend yielded an adapter; every attempt in order with its reason (REQ-002).
    NoAdapter { tried: Vec<Attempt> },
    /// A pass reads a target no earlier pass writes (REQ-003).
    UnresolvedInput { pass: PassId, input: TargetId },
    /// A pass with this ID is already registered.
    DuplicatePass { pass: PassId },
    /// Another pass already has this order key.
    DuplicateOrder { order: u32, pass: PassId, existing: PassId },
    /// The setting `viz.render.backend` names no backend.
    BadSetting(String),
    /// The device was lost (forwarded to `VIZ.RENDER.RECOVER`).
    DeviceLost(String),
    /// A wgpu error during target creation, a frame or a readback. An error no error scope
    /// caught (kept by the device's uncaptured-error handler) is returned by whichever of these
    /// calls comes next, so it may belong to an earlier operation.
    Gpu(String),
    /// A pass's `record` failed; nothing of the frame was submitted.
    PassFailed { pass: PassId, reason: String },
    /// A caller-supplied texture cannot be a frame target.
    BadTarget(String),
    /// A readback rectangle is empty, too large or outside the target.
    BadRect { x: u32, y: u32, width: u32, height: u32 },
    /// A readback would need a staging buffer larger than the device allows.
    ReadbackTooLarge { bytes: u64, max: u64 },
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::NoAdapter { tried } => {
                write!(f, "no_adapter: no backend yielded an adapter")?;
                for a in tried {
                    write!(f, "; {}: {}", a.backend, a.reason)?;
                }
                Ok(())
            }
            GraphError::UnresolvedInput { pass, input } => {
                write!(f, "unresolved_input: pass {pass} reads `{input}`, which no earlier pass writes")
            }
            GraphError::DuplicatePass { pass } => write!(f, "duplicate_pass: pass {pass} is already registered"),
            GraphError::DuplicateOrder { order, pass, existing } => {
                write!(f, "duplicate_order: pass {pass} has order key {order}, already used by {existing}")
            }
            GraphError::BadSetting(s) => write!(f, "bad_setting: viz.render.backend = `{s}` is not a backend"),
            GraphError::DeviceLost(m) => write!(f, "device_lost: {m}"),
            GraphError::Gpu(m) => write!(f, "gpu: {m}"),
            GraphError::PassFailed { pass, reason } => write!(f, "pass_failed: {pass}: {reason}"),
            GraphError::BadTarget(m) => write!(f, "bad_target: {m}"),
            GraphError::ReadbackTooLarge { bytes, max } => {
                write!(f, "readback_too_large: {bytes} bytes of staging, the device allows {max}")
            }
            GraphError::BadRect { x, y, width, height } => {
                write!(f, "bad_rect: {width}x{height} at ({x}, {y}) is empty, too large or outside the target")
            }
        }
    }
}

impl std::error::Error for GraphError {}

/// What [`WgpuSource`] hands back for a selected backend.
#[derive(Debug)]
pub struct GpuAdapter {
    pub info: wgpu::AdapterInfo,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Adapters of a real wgpu instance over the backends it was created with.
#[derive(Debug)]
pub struct WgpuSource {
    instance: Option<wgpu::Instance>,
    /// The wgpu backends of `instance` (empty when there is none).
    backends: wgpu::Backends,
    excluded: ProbeExclusions,
}

impl WgpuSource {
    /// An instance over every compiled backend, with nothing excluded. A caller that has a
    /// `PLAT.GPU.DETECT` selection uses [`WgpuSource::for_choice`] instead, so a backend whose
    /// probe hung is not entered again. With no backend compiled for this target, every request
    /// fails with that reason (wgpu would panic on creating the instance).
    pub fn new() -> WgpuSource {
        WgpuSource::for_choice(None, &ProbeExclusions::none())
    }

    /// An instance over only the backends [`instance_backends`] gives (the choice's backend
    /// when a choice is given, never an excluded one), on this thread. When none of them is
    /// compiled in, no instance is created. [`FrameGraph::create`] and [`FrameGraph::start`]
    /// with this source never request an excluded backend. Pass the same `choice` to them.
    pub fn for_choice(choice: Option<&AdapterChoice>, excluded: &ProbeExclusions) -> WgpuSource {
        let backends = instance_backends(choice, excluded) & wgpu::Instance::enabled_backend_features();
        if backends.is_empty() {
            return WgpuSource { instance: None, backends, excluded: excluded.clone() };
        }
        let desc = wgpu::InstanceDescriptor { backends, ..Default::default() };
        WgpuSource { instance: Some(wgpu::Instance::new(&desc)), backends, excluded: excluded.clone() }
    }

    /// The wgpu backends the instance was created with (empty: no instance).
    pub fn backends(&self) -> wgpu::Backends {
        self.backends
    }

    /// The backends this source never requests.
    pub fn excluded(&self) -> &ProbeExclusions {
        &self.excluded
    }
}

impl Default for WgpuSource {
    fn default() -> Self {
        WgpuSource::new()
    }
}

/// Reason: this target has no wgpu backend compiled in at all.
pub const REASON_NO_BACKEND_FOR_TARGET: &str = "no wgpu backend is compiled in for this target";
/// Reason: the backend is not compiled into this build.
pub const REASON_NOT_COMPILED: &str = "backend not compiled into this build";
/// Reason: the backend enumerates no adapter.
pub const REASON_NO_ADAPTER: &str = "no adapter";
/// Reason (`software`): no CPU adapter on any compiled backend.
pub const REASON_NO_CPU_ADAPTER: &str = "no CPU adapter";
/// Reason (hardware backend): only CPU adapters, which are tried as `software`.
pub const REASON_ONLY_CPU: &str = "only CPU adapters (software is tried as its own backend)";
/// Reason: the backend is compiled in but not part of this source's instance (it was created
/// for another chosen backend, or every backend it could search is excluded).
pub const REASON_NOT_IN_INSTANCE: &str = "backend not enabled on this instance (created for the chosen backend only)";
/// Reason: an injected adapter choice matches no adapter of its backend.
pub const REASON_CHOICE_ABSENT: &str = "the chosen adapter is not present";

/// Rank of an adapter type for a hardware backend (lower is preferred); CPU adapters are
/// only taken for [`Backend::Software`].
fn hardware_rank(t: wgpu::DeviceType) -> Option<u8> {
    match t {
        wgpu::DeviceType::DiscreteGpu => Some(0),
        wgpu::DeviceType::IntegratedGpu => Some(1),
        wgpu::DeviceType::VirtualGpu => Some(2),
        wgpu::DeviceType::Other => Some(3),
        wgpu::DeviceType::Cpu => None,
    }
}

/// Which of `infos` (one backend's adapters, in enumeration order) a request takes, or the
/// reason none qualifies. `software` takes only CPU adapters, with or without a choice; a
/// hardware backend takes the best of discrete, integrated, virtual, other, and skips CPU
/// adapters unless an injected choice names one (a WARP choice on dx12 is honoured). A
/// choice narrows the candidates by its vendor, device and name. Ties keep enumeration order.
pub fn pick_adapter(
    backend: Backend,
    choice: Option<&AdapterChoice>,
    infos: &[wgpu::AdapterInfo],
) -> Result<usize, String> {
    let mut best: Option<(u8, usize)> = None;
    for (i, info) in infos.iter().enumerate() {
        if choice.is_some_and(|c| !c.matches(info)) {
            continue;
        }
        let rank = match (backend, choice) {
            (Backend::Software, _) => (info.device_type == wgpu::DeviceType::Cpu).then_some(0),
            (_, Some(_)) => Some(hardware_rank(info.device_type).unwrap_or(4)),
            (_, None) => hardware_rank(info.device_type),
        };
        if let Some(r) = rank {
            if best.is_none_or(|(b, _)| r < b) {
                best = Some((r, i));
            }
        }
    }
    best.map(|(_, i)| i).ok_or_else(|| {
        match (backend, choice) {
            (_, Some(_)) => REASON_CHOICE_ABSENT,
            (Backend::Software, None) => REASON_NO_CPU_ADAPTER,
            _ => REASON_ONLY_CPU,
        }
        .to_string()
    })
}

impl AdapterSource for WgpuSource {
    type Adapter = GpuAdapter;

    fn request(&self, backend: Backend, choice: Option<&AdapterChoice>) -> Result<GpuAdapter, String> {
        let compiled = wgpu::Instance::enabled_backend_features();
        if compiled.is_empty() {
            return Err(REASON_NO_BACKEND_FOR_TARGET.into());
        }
        let wanted = backend.wgpu_backends();
        if !compiled.intersects(wanted) {
            return Err(REASON_NOT_COMPILED.into());
        }
        if let Some(reason) = self.excluded.reason(backend) {
            return Err(reason.into());
        }
        // Only the instance's backends (for `software`: every one not excluded).
        let wanted = wanted & self.backends;
        let Some(instance) = self.instance.as_ref().filter(|_| !wanted.is_empty()) else {
            return Err(REASON_NOT_IN_INSTANCE.into());
        };
        let adapters = instance.enumerate_adapters(wanted);
        if adapters.is_empty() {
            return Err(REASON_NO_ADAPTER.into());
        }
        let infos: Vec<wgpu::AdapterInfo> = adapters.iter().map(wgpu::Adapter::get_info).collect();
        let pick = pick_adapter(backend, choice, &infos)?;
        let Some(adapter) = adapters.into_iter().nth(pick) else {
            return Err(REASON_NO_ADAPTER.into());
        };
        let desc = wgpu::DeviceDescriptor {
            label: Some("fid-render frame graph"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&desc)).map_err(|e| format!("device creation failed: {e}"))?;
        Ok(GpuAdapter { info: adapter.get_info(), device, queue })
    }
}

/// The frame's colour, depth and ID targets, all of one size.
#[derive(Debug)]
pub struct FrameTargets {
    width: u32,
    height: u32,
    colour: wgpu::Texture,
    colour_view: wgpu::TextureView,
    depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    id: wgpu::Texture,
    id_view: wgpu::TextureView,
}

fn target(device: &wgpu::Device, label: &str, w: u32, h: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

impl FrameTargets {
    /// Targets of `width` x `height` with a new colour texture of [`COLOUR_FORMAT`].
    pub fn new(graph: &FrameGraph, width: u32, height: u32) -> Result<FrameTargets, GraphError> {
        check_size(graph, width, height)?;
        graph.begin_scopes()?;
        let colour = target(&graph.device, "frame colour", width, height, COLOUR_FORMAT);
        let t = FrameTargets::around(graph, colour);
        graph.end_scopes()?;
        Ok(t)
    }

    /// Targets around a colour texture the caller owns. It must be a single-sample 2D texture
    /// with `RENDER_ATTACHMENT` usage.
    pub fn with_colour(graph: &FrameGraph, colour: wgpu::Texture) -> Result<FrameTargets, GraphError> {
        if !colour.usage().contains(wgpu::TextureUsages::RENDER_ATTACHMENT) {
            return Err(GraphError::BadTarget("the colour texture lacks RENDER_ATTACHMENT usage".into()));
        }
        if colour.dimension() != wgpu::TextureDimension::D2 || colour.sample_count() != 1 {
            return Err(GraphError::BadTarget("the colour texture is not a single-sample 2D texture".into()));
        }
        if colour.depth_or_array_layers() != 1 || colour.mip_level_count() != 1 {
            return Err(GraphError::BadTarget("the colour texture has layers or mip levels".into()));
        }
        check_size(graph, colour.width(), colour.height())?;
        graph.begin_scopes()?;
        let t = FrameTargets::around(graph, colour);
        graph.end_scopes()?;
        Ok(t)
    }

    fn around(graph: &FrameGraph, colour: wgpu::Texture) -> FrameTargets {
        let (width, height) = (colour.width(), colour.height());
        let depth = target(&graph.device, "frame depth", width, height, DEPTH_FORMAT);
        let id = target(&graph.device, "frame id", width, height, ID_FORMAT);
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        FrameTargets {
            width,
            height,
            colour_view: view(&colour),
            depth_view: view(&depth),
            id_view: view(&id),
            colour,
            depth,
            id,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The colour texture (the frame's output to the caller).
    pub fn colour(&self) -> &wgpu::Texture {
        &self.colour
    }

    pub fn colour_format(&self) -> wgpu::TextureFormat {
        self.colour.format()
    }

    pub fn colour_view(&self) -> &wgpu::TextureView {
        &self.colour_view
    }

    pub fn depth(&self) -> &wgpu::Texture {
        &self.depth
    }

    pub fn depth_view(&self) -> &wgpu::TextureView {
        &self.depth_view
    }

    pub fn id(&self) -> &wgpu::Texture {
        &self.id
    }

    pub fn id_view(&self) -> &wgpu::TextureView {
        &self.id_view
    }
}

fn check_size(graph: &FrameGraph, width: u32, height: u32) -> Result<(), GraphError> {
    let max = graph.device.limits().max_texture_dimension_2d;
    if width == 0 || height == 0 || width > max || height > max {
        return Err(GraphError::BadTarget(format!("size {width}x{height} outside 1..={max}")));
    }
    Ok(())
}

/// A pixel rectangle of the frame targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The GPU entry point: the device of the selected backend and the registered passes.
#[derive(Debug)]
pub struct FrameGraph {
    device: wgpu::Device,
    queue: wgpu::Queue,
    backend: Backend,
    info: wgpu::AdapterInfo,
    tried: Vec<Attempt>,
    passes: PassList,
    lost: Arc<Mutex<Option<String>>>,
    lost_flag: Arc<AtomicBool>,
    uncaptured: Arc<Mutex<Option<String>>>,
    policy_warning: Option<PolicyError>,
}

impl FrameGraph {
    /// Selects a backend from `source` in `order` (see [`resolve_order`]) and creates its
    /// device. The source's excluded backends are skipped ([`select_excluding`]).
    pub fn create(
        source: &WgpuSource,
        order: &[Backend],
        choice: Option<&AdapterChoice>,
    ) -> Result<FrameGraph, GraphError> {
        let s = select_excluding(source, order, choice, source.excluded())?;
        Ok(FrameGraph::from_selected(s))
    }

    /// Start-up as the spec describes it: the policy order (or `[software]` when the policy gave
    /// none, keeping its error as [`FrameGraph::policy_warning`]), overridden by the setting
    /// `viz.render.backend` and an injected choice ([`resolve_order`]), then [`select`].
    pub fn start(
        source: &WgpuSource,
        policy: Result<Vec<Backend>, PolicyError>,
        setting: Option<&str>,
        choice: Option<&AdapterChoice>,
    ) -> Result<FrameGraph, GraphError> {
        let (order, warning) = order_or_software(policy);
        let order = resolve_order(&order, setting, choice)?;
        let mut g = FrameGraph::create(source, &order, choice)?;
        g.policy_warning = warning;
        Ok(g)
    }

    /// Why the policy gave no order, when [`FrameGraph::start`] fell back to `[software]`; to
    /// be reported as a warning.
    pub fn policy_warning(&self) -> Option<&PolicyError> {
        self.policy_warning.as_ref()
    }

    fn from_selected(s: Selected<GpuAdapter>) -> FrameGraph {
        let lost = Arc::new(Mutex::new(None));
        let lost_flag = Arc::new(AtomicBool::new(false));
        let (l, f) = (Arc::clone(&lost), Arc::clone(&lost_flag));
        s.adapter.device.set_device_lost_callback(move |reason, msg| {
            if let Ok(mut g) = l.lock() {
                *g = Some(format!("{reason:?}: {msg}"));
            }
            f.store(true, Ordering::SeqCst);
        });
        // wgpu's default handler panics on an error no scope catches; keep it as a result
        // instead (CONSTITUTION §8), returned as `Gpu` by the next call.
        let uncaptured = Arc::new(Mutex::new(None));
        let u = Arc::clone(&uncaptured);
        s.adapter.device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            if let Ok(mut g) = u.lock() {
                if g.is_none() {
                    *g = Some(e.to_string());
                }
            }
        }));
        FrameGraph {
            device: s.adapter.device,
            queue: s.adapter.queue,
            backend: s.backend,
            info: s.adapter.info,
            tried: s.tried,
            passes: PassList::new(),
            lost,
            lost_flag,
            uncaptured,
            policy_warning: None,
        }
    }

    /// The backend in use.
    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// The adapter in use.
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }

    /// The backends tried before the one in use, with their reasons.
    pub fn tried(&self) -> &[Attempt] {
        &self.tried
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Registers a pass ([`PassList::register`]).
    pub fn register(&mut self, decl: PassDecl) -> Result<(), GraphError> {
        self.passes.register(decl)
    }

    /// The registered pass IDs in execution order.
    pub fn pass_order(&self) -> Vec<PassId> {
        self.passes.order()
    }

    fn check_lost(&self) -> Result<(), GraphError> {
        if self.lost_flag.load(Ordering::SeqCst) {
            let msg = self.lost.lock().ok().and_then(|g| g.clone()).unwrap_or_else(|| "device lost".into());
            return Err(GraphError::DeviceLost(msg));
        }
        Ok(())
    }

    /// Opens the validation and out-of-memory error scopes around a GPU operation.
    fn begin_scopes(&self) -> Result<(), GraphError> {
        self.check_lost()?;
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        Ok(())
    }

    /// Closes the scopes of [`FrameGraph::begin_scopes`]: a caught error, a stored uncaptured
    /// error or a lost device is the result.
    fn end_scopes(&self) -> Result<(), GraphError> {
        let oom = pollster::block_on(self.device.pop_error_scope());
        let validation = pollster::block_on(self.device.pop_error_scope());
        self.check_lost()?;
        if let Some(e) = validation.or(oom) {
            return Err(GraphError::Gpu(e.to_string()));
        }
        let stored = self.uncaptured.lock().ok().and_then(|mut g| g.take());
        match stored {
            Some(e) => Err(GraphError::Gpu(e)),
            None => Ok(()),
        }
    }

    fn scoped<T>(&self, f: impl FnOnce() -> Result<T, GraphError>) -> Result<T, GraphError> {
        self.begin_scopes()?;
        let r = f();
        self.end_scopes()?;
        r
    }

    /// Renders one frame into `targets`: clears colour to `clear`, depth to [`DEPTH_CLEAR`]
    /// and ID to 0, then runs every pass in ascending order key and submits.
    pub fn render(&mut self, targets: &FrameTargets, clear: wgpu::Color) -> Result<(), GraphError> {
        self.begin_scopes()?;
        let r = self.encode_frame(targets, clear);
        self.end_scopes()?;
        r
    }

    fn encode_frame(&mut self, targets: &FrameTargets, clear: wgpu::Color) -> Result<(), GraphError> {
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame graph") });
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame clear"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: targets.colour_view(),
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: targets.id_view(),
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: targets.depth_view(),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(DEPTH_CLEAR),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        for p in self.passes.iter_mut() {
            let mut ctx = PassContext { device: &self.device, queue: &self.queue, encoder: &mut encoder, targets };
            p.run.record(&mut ctx).map_err(|reason| GraphError::PassFailed { pass: p.id, reason })?;
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// The ID target over `rect`, row by row: `[render ID, primitive or point index]` per
    /// pixel, render ID 0 for no drawable.
    pub fn read_id(&self, targets: &FrameTargets, rect: Rect) -> Result<Vec<[u32; 2]>, GraphError> {
        self.scoped(|| {
            let (layout, size) = readback_layout(targets, rect, 8)?;
            let id_readback = self.staging(size, "id readback");
            self.copy_out(targets.id(), wgpu::TextureAspect::All, rect, &id_readback, layout);
            let (tx, rx) = mpsc::channel();
            id_readback.map_async(wgpu::MapMode::Read, .., move |r| {
                let _ = tx.send(r);
            });
            let bytes = self.finish_map(&id_readback, rx)?;
            Ok(rows(&bytes, rect, layout, 8)
                .map(|px| {
                    [u32::from_le_bytes([px[0], px[1], px[2], px[3]]), u32::from_le_bytes([px[4], px[5], px[6], px[7]])]
                })
                .collect())
        })
    }

    /// The depth target over `rect`, row by row (reversed Z: 0.0 is the far plane or no
    /// drawable). WebGPU allows only whole-subresource copies of depth textures, so the whole
    /// depth target is copied to a staging buffer and `rect` is cropped on the CPU
    /// ([`crop_rows`]); a target whose staging buffer would exceed the device's
    /// `max_buffer_size` is [`GraphError::ReadbackTooLarge`].
    pub fn read_depth(&self, targets: &FrameTargets, rect: Rect) -> Result<Vec<f32>, GraphError> {
        self.scoped(|| {
            // Checks `rect` against the target (the layout itself is not used).
            readback_layout(targets, rect, 4)?;
            let full = Rect { x: 0, y: 0, width: targets.width(), height: targets.height() };
            let bad = GraphError::BadRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
            let bytes_per_row = padded_bytes_per_row(full.width, 4).ok_or_else(|| bad.clone())?;
            let size = u64::from(bytes_per_row) * u64::from(full.height);
            let max = self.device.limits().max_buffer_size;
            if size > max {
                return Err(GraphError::ReadbackTooLarge { bytes: size, max });
            }
            let depth_readback = self.staging(size, "depth readback");
            self.copy_out(
                targets.depth(),
                wgpu::TextureAspect::DepthOnly,
                full,
                &depth_readback,
                Layout { bytes_per_row },
            );
            let (tx, rx) = mpsc::channel();
            depth_readback.map_async(wgpu::MapMode::Read, .., move |r| {
                let _ = tx.send(r);
            });
            let bytes = self.finish_map(&depth_readback, rx)?;
            let px = crop_rows(&bytes, bytes_per_row, rect, 4).ok_or(bad)?;
            Ok(px.iter().map(|p| f32::from_le_bytes([p[0], p[1], p[2], p[3]])).collect())
        })
    }

    fn staging(&self, size: u64, label: &str) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        })
    }

    fn copy_out(
        &self,
        texture: &wgpu::Texture,
        aspect: wgpu::TextureAspect,
        rect: Rect,
        buffer: &wgpu::Buffer,
        layout: Layout,
    ) {
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame readback") });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: rect.x, y: rect.y, z: 0 },
                aspect,
            },
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(layout.bytes_per_row),
                    rows_per_image: Some(rect.height),
                },
            },
            wgpu::Extent3d { width: rect.width, height: rect.height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
    }

    fn finish_map(
        &self,
        buffer: &wgpu::Buffer,
        rx: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    ) -> Result<Vec<u8>, GraphError> {
        self.device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
            .map_err(|e| GraphError::Gpu(e.to_string()))?;
        match rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(GraphError::Gpu(e.to_string())),
            Err(_) => return Err(GraphError::Gpu("readback mapping was never completed".into())),
        }
        let bytes = buffer.get_mapped_range(..).to_vec();
        buffer.unmap();
        Ok(bytes)
    }
}

#[derive(Debug, Clone, Copy)]
struct Layout {
    bytes_per_row: u32,
}

/// Checks `rect` against the targets and returns the aligned row layout and buffer size.
fn readback_layout(targets: &FrameTargets, rect: Rect, texel: u32) -> Result<(Layout, u64), GraphError> {
    let bad = GraphError::BadRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    let inside = rect.x.checked_add(rect.width).is_some_and(|e| e <= targets.width())
        && rect.y.checked_add(rect.height).is_some_and(|e| e <= targets.height());
    if rect.width == 0 || rect.height == 0 || !inside {
        return Err(bad);
    }
    if u64::from(rect.width) * u64::from(rect.height) > MAX_READBACK_PIXELS {
        return Err(bad);
    }
    let bytes_per_row = padded_bytes_per_row(rect.width, texel).ok_or(bad)?;
    Ok((Layout { bytes_per_row }, u64::from(bytes_per_row) * u64::from(rect.height)))
}

/// Bytes per row of a `width`-texel row of `texel` bytes, padded to wgpu's copy alignment
/// (`COPY_BYTES_PER_ROW_ALIGNMENT`, 256); `None` on overflow.
pub fn padded_bytes_per_row(width: u32, texel: u32) -> Option<u32> {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    width.checked_mul(texel)?.div_ceil(align).checked_mul(align)
}

/// The texels of `rect`, row by row, cut from `bytes`: a whole image copied with rows of
/// `bytes_per_row` bytes (padding included) and texels of `texel` bytes. `None` when `rect`
/// is empty, or reaches past a row's bytes or past the copied rows (padding is never told
/// apart from texels here: the caller checks `rect` against the image first).
pub fn crop_rows(bytes: &[u8], bytes_per_row: u32, rect: Rect, texel: u32) -> Option<Vec<&[u8]>> {
    let (bpr, t) = (bytes_per_row as usize, texel as usize);
    if rect.width == 0 || rect.height == 0 || t == 0 || bpr == 0 {
        return None;
    }
    let x0 = (rect.x as usize).checked_mul(t)?;
    let x1 = (rect.x as usize).checked_add(rect.width as usize)?.checked_mul(t)?;
    if x1 > bpr {
        return None;
    }
    let mut out = Vec::with_capacity(rect.width as usize * rect.height as usize);
    for row in rect.y as usize..(rect.y as usize).checked_add(rect.height as usize)? {
        let start = row.checked_mul(bpr)?;
        let line = bytes.get(start.checked_add(x0)?..start.checked_add(x1)?)?;
        out.extend(line.chunks_exact(t));
    }
    Some(out)
}

/// The texels of `rect` in `bytes` (rows padded to `layout.bytes_per_row`), row by row.
fn rows<'a>(bytes: &'a [u8], rect: Rect, layout: Layout, texel: u32) -> impl Iterator<Item = &'a [u8]> + 'a {
    let (bpr, texel) = (layout.bytes_per_row as usize, texel as usize);
    let row_len = rect.width as usize * texel;
    bytes
        .chunks(bpr)
        .take(rect.height as usize)
        .flat_map(move |row| row.get(..row_len).unwrap_or(&[]).chunks_exact(texel))
}
