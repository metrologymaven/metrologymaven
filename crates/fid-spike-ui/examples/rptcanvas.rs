//! `rptcanvas`: the VIZ.SHELL.SPKRPT prototype. A WYSIWYG page canvas in egui: 5 A4 (or
//! Letter) pages with 200 blocks (bands, text blocks, tables and one live wgpu 3D frame
//! drawn through egui-wgpu's paint callback), drag and resize with snapping to the grid and
//! to guides, zoom 25 to 400 %, multi-page scroll, text editing with IME composition and a
//! CJK fallback font, undo and redo, keyboard move and resize, and AccessKit exposure of
//! every block on screen. Throwaway: it adopts nothing.
//!
//! ```text
//! cargo run --release -p fid-spike-ui --example rptcanvas
//! cargo run --release -p fid-spike-ui --example rptcanvas -- --bench <dir> --hardware "<CPU, RAM, OS>"
//!     [--h1] [--font <ttf>] [--cjk-font <ttf or ttc>] [--ime pass|fail]
//!     [--screen-reader pass|fail] [--drag-seconds <s>]
//! ```
//!
//! Keys (with a block focused): Tab / Shift+Tab next / previous block, Page Down / Page Up
//! first block of the next / previous page, arrows move one grid step (5 mm), Shift+arrows
//! resize, Enter edits text (Escape ends editing), Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z) undo /
//! redo, Ctrl+Plus / Ctrl+Minus zoom. Ctrl+wheel zooms at the pointer; Alt while dragging
//! turns snapping off.
//!
//! `--bench` runs the scripted schedule (drag a block across page 1 for 10 s, zoom from
//! 25 % to 400 %, type 200 characters) with vsync off, times each frame with a monotonic
//! clock from input to submit, runs the AC-3 keyboard sequence on the model and writes
//! `<dir>/REPORT.md` and `<dir>/timings.csv`. `--ime` and `--screen-reader` record the
//! outcomes of the manual AC-4 and REQ-005 checks once they have been done.

use egui_wgpu::wgpu;
use egui_winit::accesskit_winit;
use egui_winit::winit;
use fid_hal_idl::UntrustedText;
use fid_spike_ui::lock;
use fid_spike_ui::report::{Environment, Outcome};
use fid_spike_ui::rpt::canvas::{Block, Canvas, Kind, Paper, Rect};
use fid_spike_ui::rpt::edit::{Action, Designer, Guides, GRID, ZOOM_MAX, ZOOM_MIN};
use fid_spike_ui::rpt::layout::{Layout, PxRect};
use fid_spike_ui::rpt::report::{render, timings_csv, Content, Results};
use fid_spike_ui::rpt::script::{parse_keyboard, run_keyboard, Command, Cursor, Phase, Schedule};
use fid_spike_ui::rpt::{FIXTURE_CANVAS, FIXTURE_KEYBOARD};
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;
const FONT_CANDIDATES: [&str; 6] = [
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];
const CJK_CANDIDATES: [&str; 7] = [
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    "C:\\Windows\\Fonts\\meiryo.ttc",
    "C:\\Windows\\Fonts\\msgothic.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
];
const LIMITATIONS: [&str; 8] = [
    "Frame time is CPU wall time from input to submit (vsync off); GPU completion is not awaited.",
    "The harness drives the designer model (drag positions, zoom, committed characters), not OS pointer, wheel or keyboard events.",
    "Only blocks on visible pages are laid out and reach AccessKit; off-screen pages are culled.",
    "Text sizes are rounded to 0.5 px while zooming, because egui rasterises glyphs once per size.",
    "Rich text is one font and size per block; the text widget is egui's plain multiline editor.",
    "The 3D frame is a lit-less rotating cube; when the frame is partly off-screen egui-wgpu clamps its viewport, which distorts it.",
    "egui default fonts are not built (licence gate in deny.toml); system fonts are loaded at run time.",
    "No Metal backend (advisory gate on paste via metal); macOS is not measured.",
];

// ---------------------------------------------------------------------------------------
// Arguments and fonts

struct Args {
    bench: Option<PathBuf>,
    hardware: String,
    h1: bool,
    font: Option<PathBuf>,
    cjk_font: Option<PathBuf>,
    ime: Outcome,
    screen_reader: Outcome,
    drag_seconds: f64,
}

const USAGE: &str = "usage: rptcanvas [--bench <dir> --hardware <text>] [--h1] [--font <ttf>] \
                     [--cjk-font <ttf or ttc>] [--ime pass|fail] [--screen-reader pass|fail] \
                     [--drag-seconds <s>]";

fn outcome(v: &str) -> Option<Outcome> {
    match v {
        "pass" => Some(Outcome::Pass),
        "fail" => Some(Outcome::Fail),
        _ => None,
    }
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        bench: None,
        hardware: String::new(),
        h1: false,
        font: None,
        cjk_font: None,
        ime: Outcome::NotRun,
        screen_reader: Outcome::NotRun,
        drag_seconds: 10.0,
    };
    while let Some(k) = it.next() {
        if k == "--h1" {
            a.h1 = true;
            continue;
        }
        let v = it.next().ok_or_else(|| USAGE.to_string())?;
        match k.as_str() {
            "--bench" => a.bench = Some(PathBuf::from(v)),
            "--hardware" if v.len() <= 512 => a.hardware = v,
            "--font" => a.font = Some(PathBuf::from(v)),
            "--cjk-font" => a.cjk_font = Some(PathBuf::from(v)),
            "--ime" => a.ime = outcome(&v).ok_or_else(|| USAGE.to_string())?,
            "--screen-reader" => a.screen_reader = outcome(&v).ok_or_else(|| USAGE.to_string())?,
            "--drag-seconds" => match v.parse::<f64>() {
                Ok(s) if s.is_finite() && s > 0.0 && s <= 3600.0 => a.drag_seconds = s,
                _ => return Err(USAGE.to_string()),
            },
            _ => return Err(USAGE.to_string()),
        }
    }
    if a.bench.is_some() && a.hardware.is_empty() {
        return Err(format!("--bench needs --hardware\n{USAGE}"));
    }
    Ok(a)
}

fn read_font(candidates: &[PathBuf]) -> Option<(PathBuf, Vec<u8>)> {
    for p in candidates {
        let small = std::fs::metadata(p).map(|m| m.is_file() && m.len() <= MAX_FONT_BYTES).unwrap_or(false);
        if small {
            if let Ok(bytes) = std::fs::read(p) {
                return Some((p.clone(), bytes));
            }
        }
    }
    None
}

/// The primary font, then a CJK fallback font if one is found; egui looks a glyph up in
/// each font of the family in turn. Returns the fonts and the paths loaded.
fn load_fonts(primary: Option<&Path>, cjk: Option<&Path>) -> Result<(egui::FontDefinitions, Vec<String>), String> {
    let list = |explicit: Option<&Path>, defaults: &[&str]| -> Vec<PathBuf> {
        match explicit {
            Some(p) => vec![p.to_path_buf()],
            None => defaults.iter().map(PathBuf::from).collect(),
        }
    };
    let (path, bytes) = read_font(&list(primary, &FONT_CANDIDATES))
        .ok_or_else(|| "no usable font found; pass --font <path to a .ttf>".to_string())?;
    let mut fonts = egui::FontDefinitions::empty();
    let mut loaded = vec![path.display().to_string()];
    fonts.font_data.insert("ui".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
    let mut names = vec!["ui".to_owned()];
    if let Some((path, bytes)) = read_font(&list(cjk, &CJK_CANDIDATES)) {
        fonts.font_data.insert("cjk".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        names.push("cjk".to_owned());
        loaded.push(path.display().to_string());
    }
    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(fam).or_default().extend(names.iter().cloned());
    }
    Ok((fonts, loaded))
}

// ---------------------------------------------------------------------------------------
// The live 3D frame: a rotating cube drawn by wgpu inside egui's render pass

const SHADER: &str = r#"
struct U {
    mvp: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(@location(0) p: vec3<f32>, @location(1) c: vec3<f32>) -> VOut {
    var o: VOut;
    o.pos = u.mvp * vec4<f32>(p, 1.0);
    o.color = c;
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color, 1.0);
}
"#;

type Mat4 = [[f32; 4]; 4]; // column-major: m[column][row]

fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (c, col) in out.iter_mut().enumerate() {
        for (r, v) in col.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

/// Perspective (right-handed, depth 0..1) times a view 4 units back times two rotations.
fn mvp(angle: f32, aspect: f32) -> Mat4 {
    let (sy, cy) = angle.sin_cos();
    let (sx, cx) = (0.6 * angle).sin_cos();
    let ry = [[cy, 0.0, -sy, 0.0], [0.0, 1.0, 0.0, 0.0], [sy, 0.0, cy, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let rx = [[1.0, 0.0, 0.0, 0.0], [0.0, cx, sx, 0.0], [0.0, -sx, cx, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let view = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, -4.0, 1.0]];
    let (near, far) = (0.1_f32, 100.0_f32);
    let f = 1.0 / (std::f32::consts::FRAC_PI_8).tan();
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
    let proj = [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ];
    mul(&proj, &mul(&view, &mul(&ry, &rx)))
}

/// 36 vertices (position, colour) of a cube with outward counter-clockwise faces, so
/// back-face culling alone draws it correctly without a depth buffer.
fn cube_bytes() -> Vec<u8> {
    let axes: [([f32; 3], [f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.85, 0.35, 0.3]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [0.3, 0.75, 0.4]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.3, 0.45, 0.85]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.85, 0.75, 0.3]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.6, 0.35, 0.8]),
        ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.3, 0.75, 0.8]),
    ];
    let mut out = Vec::with_capacity(36 * 24);
    for (n, u, v, color) in axes {
        let corner = |su: f32, sv: f32| -> [f32; 3] {
            [n[0] + su * u[0] + sv * v[0], n[1] + su * u[1] + sv * v[1], n[2] + su * u[2] + sv * v[2]]
        };
        let (c0, c1, c2, c3) = (corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0));
        for p in [c0, c1, c2, c0, c2, c3] {
            for x in p.iter().chain(color.iter()) {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
    }
    out
}

struct CubeResources {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct CubeCallback {
    angle: f32,
    aspect: f32,
}

impl egui_wgpu::CallbackTrait for CubeCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(r) = resources.get::<CubeResources>() {
            let m = mvp(self.angle, self.aspect);
            let bytes: Vec<u8> = m.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
            queue.write_buffer(&r.uniform, 0, &bytes);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(r) = resources.get::<CubeResources>() {
            render_pass.set_pipeline(&r.pipeline);
            render_pass.set_bind_group(0, &r.bind_group, &[]);
            render_pass.set_vertex_buffer(0, r.vertices.slice(..));
            render_pass.draw(0..36, 0..1);
        }
    }
}

fn install_cube(rs: &egui_wgpu::RenderState) {
    use wgpu::util::DeviceExt as _;
    let device = &rs.device;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("cube"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("cube"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("cube"),
        bind_group_layouts: &[&bind_layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("cube"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
                ],
            }],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: rs.target_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    });
    let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("cube vertices"),
        contents: &cube_bytes(),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cube mvp"),
        size: 64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("cube"),
        layout: &bind_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
    });
    rs.renderer.write().callback_resources.insert(CubeResources { pipeline, vertices, uniform, bind_group });
}

// ---------------------------------------------------------------------------------------
// The canvas

struct DragState {
    id: u32,
    resize: bool,
    pointer: egui::Pos2,
    start: Rect,
}

struct Model {
    designer: Designer,
    edit_buf: String,
    edit_started: bool,
    focus_block: bool,
    reveal: bool,
    drag: Option<DragState>,
    guides: Guides,
    scroll_to: Option<egui::Vec2>,
    viewport: egui::Rect,
}

fn block_id(id: u32) -> egui::Id {
    egui::Id::new(("spkrpt-block", id))
}

fn handle_id(id: u32) -> egui::Id {
    egui::Id::new(("spkrpt-handle", id))
}

/// Applies an action and keeps the editing buffer, egui focus and scroll in step.
fn act(m: &mut Model, a: &Action) {
    let was_editing = m.designer.editing().map(|e| e.0);
    let focus_before = m.designer.focus();
    let zoom_before = m.designer.zoom();
    m.designer.apply(a);
    match m.designer.editing() {
        Some((id, buf)) => {
            if was_editing != Some(id) {
                m.edit_started = true;
            }
            m.edit_buf.clear();
            m.edit_buf.push_str(buf);
        }
        None => {
            if was_editing.is_some() {
                m.focus_block = true;
            }
        }
    }
    if m.designer.focus() != focus_before || m.designer.zoom() != zoom_before {
        m.focus_block = true;
        m.reveal = true;
    }
}

fn keyboard_actions(ctx: &egui::Context) -> Vec<Action> {
    use egui::{Key, Modifiers};
    // Patterns with Shift first: egui ignores Shift in a pattern that does not name it.
    let table = [
        (Modifiers::COMMAND | Modifiers::SHIFT, Key::Z, Action::Redo),
        (Modifiers::COMMAND, Key::Z, Action::Undo),
        (Modifiers::COMMAND, Key::Y, Action::Redo),
        (Modifiers::COMMAND, Key::Plus, Action::ZoomIn),
        (Modifiers::COMMAND, Key::Equals, Action::ZoomIn),
        (Modifiers::COMMAND, Key::Minus, Action::ZoomOut),
        (Modifiers::SHIFT, Key::Tab, Action::BackTab),
        (Modifiers::SHIFT, Key::ArrowLeft, Action::ShiftLeft),
        (Modifiers::SHIFT, Key::ArrowRight, Action::ShiftRight),
        (Modifiers::SHIFT, Key::ArrowUp, Action::ShiftUp),
        (Modifiers::SHIFT, Key::ArrowDown, Action::ShiftDown),
        (Modifiers::NONE, Key::Tab, Action::Tab),
        (Modifiers::NONE, Key::ArrowLeft, Action::Left),
        (Modifiers::NONE, Key::ArrowRight, Action::Right),
        (Modifiers::NONE, Key::ArrowUp, Action::Up),
        (Modifiers::NONE, Key::ArrowDown, Action::Down),
        (Modifiers::NONE, Key::PageDown, Action::PageDown),
        (Modifiers::NONE, Key::PageUp, Action::PageUp),
        (Modifiers::NONE, Key::Enter, Action::Enter),
    ];
    ctx.input_mut(|i| {
        let mut out = Vec::new();
        for (mods, key, action) in table {
            while i.consume_key(mods, key) {
                out.push(action.clone());
            }
        }
        out
    })
}

fn screen(content_min: egui::Pos2, r: PxRect) -> egui::Rect {
    egui::Rect::from_min_size(content_min + egui::vec2(r.x, r.y), egui::vec2(r.w, r.h))
}

/// Font size in pixels for a text height in dmm, rounded to 0.5 px.
fn font_px(dmm: f32, scale: f32) -> f32 {
    ((dmm * scale * 2.0).round() / 2.0).max(1.0)
}

fn paint_block(painter: &egui::Painter, b: &Block, text: &str, rect: egui::Rect, scale: f32, angle: f32) {
    let ink = egui::Color32::from_gray(30);
    let pad = 10.0 * scale;
    match b.kind {
        Kind::Band => {
            painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(226, 232, 240));
            let g =
                painter.layout(text.to_string(), egui::FontId::proportional(font_px(45.0, scale)), ink, rect.width());
            painter.galley(rect.min + egui::vec2(pad, pad), g, ink);
        }
        Kind::Text => {
            let g = painter.layout(
                text.to_string(),
                egui::FontId::proportional(font_px(35.0, scale)),
                ink,
                (rect.width() - 2.0 * pad).max(1.0),
            );
            painter.galley(rect.min + egui::vec2(pad, pad), g, ink);
        }
        Kind::Table => {
            let (rows, cols) = b.table_size().unwrap_or((1, 1));
            let line = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(120));
            let (cw, rh) = (rect.width() / cols as f32, rect.height() / rows as f32);
            for c in 1..cols {
                painter.vline(rect.min.x + c as f32 * cw, rect.y_range(), line);
            }
            for r in 1..rows {
                painter.hline(rect.x_range(), rect.min.y + r as f32 * rh, line);
            }
            let size = font_px(25.0, scale);
            if size >= 4.0 {
                for r in 0..rows {
                    for c in 0..cols {
                        let at = rect.min + egui::vec2(c as f32 * cw + 3.0, r as f32 * rh + 2.0);
                        let label = format!("R{}C{}", r + 1, c + 1);
                        painter.text(at, egui::Align2::LEFT_TOP, label, egui::FontId::proportional(size), ink);
                    }
                }
            }
            painter.rect_stroke(rect, 0.0, line, egui::StrokeKind::Inside);
        }
        Kind::Frame3d => {
            painter.rect_filled(rect, 0.0, egui::Color32::from_gray(20));
            let aspect = rect.width() / rect.height().max(1.0);
            painter.add(egui_wgpu::Callback::new_paint_callback(rect, CubeCallback { angle, aspect }));
        }
    }
}

fn canvas_ui(ui: &mut egui::Ui, m: &mut Model) {
    let ctx = ui.ctx().clone();
    // Keyboard: only while the focused block holds egui focus and no text edit is open.
    let focused = m.designer.focus().map(block_id);
    let has_focus = focused.is_some_and(|id| ctx.memory(|mem| mem.has_focus(id)));
    if has_focus && m.designer.editing().is_none() {
        for a in keyboard_actions(&ctx) {
            act(m, &a);
        }
    }

    // Ctrl+wheel (or pinch) zoom, anchored at the pointer.
    let zoom_delta = ui.input(|i| i.zoom_delta());
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let canvas_rect = ui.available_rect_before_wrap();
    if zoom_delta != 1.0 {
        if let Some(p) = pointer.filter(|p| canvas_rect.contains(*p)) {
            let z0 = m.designer.zoom();
            m.designer.set_zoom(z0 * zoom_delta);
            let ratio = m.designer.zoom() / z0;
            let under = m.viewport.min + (p - canvas_rect.min);
            let off = under.to_vec2() * ratio - (p - canvas_rect.min);
            m.scroll_to = Some(egui::vec2(off.x.max(0.0), off.y.max(0.0)));
        }
    }

    let layout = Layout::new(m.designer.canvas(), m.designer.zoom());
    let scale = layout.scale();
    if m.reveal {
        m.reveal = false;
        if let Some(b) = m.designer.focused() {
            if let Some(r) = layout.to_px(b.page, b.rect) {
                let inside = m.viewport.contains(egui::pos2(r.x, r.y))
                    && m.viewport
                        .contains(egui::pos2(r.x + r.w.min(m.viewport.width()), r.y + r.h.min(m.viewport.height())));
                if !inside {
                    m.scroll_to = Some(egui::vec2((r.x - 40.0).max(0.0), (r.y - 40.0).max(0.0)));
                }
            }
        }
    }

    let mut area = egui::ScrollArea::both().id_salt("spkrpt-canvas").auto_shrink([false, false]);
    if let Some(off) = m.scroll_to.take() {
        area = area.horizontal_scroll_offset(off.x).vertical_scroll_offset(off.y);
    }
    let angle = ctx.input(|i| i.time) as f32;
    let mut events: Vec<(u32, egui::Response, egui::Response)> = Vec::new();
    let mut edit_done: Vec<Action> = Vec::new();
    let mut seen = egui::Rect::NOTHING;
    area.show_viewport(ui, |ui, viewport| {
        seen = viewport;
        let (w, h) = layout.content_size();
        let (content, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let painter = ui.painter().clone();
        let sel = ui.visuals().selection.stroke;
        let mut live_frame = false;
        for page in layout.visible_pages(viewport.min.y, viewport.max.y) {
            let Some(pr) = layout.page(page) else { continue };
            let prect = screen(content.min, pr);
            painter.rect_filled(prect.translate(egui::vec2(3.0, 3.0)), 0.0, egui::Color32::from_black_alpha(60));
            painter.rect_filled(prect, 0.0, egui::Color32::WHITE);
            // Grid lines every 5 mm on the visible part of the page (hidden below 50 %).
            if m.designer.zoom() >= 50.0 {
                let step = GRID as f32 * scale;
                let vis = prect.intersect(ui.clip_rect());
                let grid = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(238));
                let mut x = prect.min.x + ((vis.min.x - prect.min.x) / step).ceil().max(0.0) * step;
                while x < vis.max.x {
                    painter.vline(x, vis.y_range(), grid);
                    x += step;
                }
                let mut y = prect.min.y + ((vis.min.y - prect.min.y) / step).ceil().max(0.0) * step;
                while y < vis.max.y {
                    painter.hline(vis.x_range(), y, grid);
                    y += step;
                }
            }
            let page_painter = painter.with_clip_rect(prect.intersect(ui.clip_rect()));
            let blocks: Vec<Block> = m.designer.canvas().on_page(page).cloned().collect();
            for b in &blocks {
                let Some(r) = layout.to_px(page, b.rect) else { continue };
                let rect = screen(content.min, r);
                let id = block_id(b.id);
                let resp = ui.interact(rect, id, egui::Sense::click_and_drag());
                let handle_rect = egui::Rect::from_center_size(rect.max, egui::vec2(10.0, 10.0));
                let handle = ui.interact(handle_rect, handle_id(b.id), egui::Sense::drag());
                let editing = m.designer.editing().is_some_and(|e| e.0 == b.id);
                let clip = page_painter.with_clip_rect(rect.intersect(page_painter.clip_rect()));
                if editing {
                    let size = font_px(if b.kind == Kind::Band { 45.0 } else { 35.0 }, scale);
                    let r = ui.put(
                        rect,
                        egui::TextEdit::multiline(&mut m.edit_buf)
                            .font(egui::FontId::proportional(size))
                            .char_limit(fid_spike_ui::rpt::canvas::MAX_TEXT_CHARS),
                    );
                    if m.edit_started {
                        r.request_focus();
                        m.edit_started = false;
                    }
                    if r.changed() {
                        edit_done.push(Action::SetText(m.edit_buf.clone()));
                    }
                    if r.lost_focus() {
                        edit_done.push(Action::Escape);
                    }
                } else {
                    paint_block(&clip, b, &b.content, rect, scale, angle);
                }
                live_frame |= b.kind == Kind::Frame3d;
                if m.designer.focus() == Some(b.id) {
                    if m.focus_block && !editing {
                        resp.request_focus();
                        m.focus_block = false;
                    }
                    ui.memory_mut(|mem| {
                        mem.set_focus_lock_filter(
                            id,
                            egui::EventFilter {
                                tab: true,
                                horizontal_arrows: true,
                                vertical_arrows: true,
                                escape: true,
                            },
                        )
                    });
                    page_painter.rect_stroke(
                        rect,
                        0.0,
                        egui::Stroke::new(2.0_f32, sel.color),
                        egui::StrokeKind::Outside,
                    );
                    page_painter.rect_filled(handle_rect.shrink(1.0), 0.0, sel.color);
                } else if resp.hovered() {
                    page_painter.rect_stroke(
                        rect,
                        0.0,
                        egui::Stroke::new(1.0_f32, sel.color),
                        egui::StrokeKind::Outside,
                    );
                }
                // AccessKit (REQ-VIZ.SHELL.SPKRPT-005): role, and type, name and position.
                if let Some(a) = m.designer.canvas().a11y(b.id) {
                    ctx.accesskit_node_builder(id, |node| {
                        node.set_role(match a.role {
                            "group" => egui::accesskit::Role::Group,
                            "paragraph" => egui::accesskit::Role::Paragraph,
                            "table" => egui::accesskit::Role::Table,
                            _ => egui::accesskit::Role::Image,
                        });
                        node.set_label(a.label.clone());
                    });
                }
                events.push((b.id, resp, handle));
            }
            // Snap guides of the block being dragged.
            let dragged_here =
                m.designer.dragging().and_then(|id| m.designer.canvas().get(id)).is_some_and(|b| b.page == page);
            if dragged_here {
                let guide = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(230, 60, 160));
                if let Some(gx) = m.guides.x {
                    page_painter.vline(prect.min.x + gx as f32 * scale, prect.y_range(), guide);
                }
                if let Some(gy) = m.guides.y {
                    page_painter.hline(prect.x_range(), prect.min.y + gy as f32 * scale, guide);
                }
            }
        }
        if live_frame {
            ctx.request_repaint();
        }
    });
    m.viewport = seen;

    for a in &edit_done {
        act(m, a);
    }
    let alt = ctx.input(|i| i.modifiers.alt);
    for (id, resp, handle) in events {
        if resp.double_clicked() {
            m.designer.select(id);
            act(m, &Action::Enter);
        } else if resp.clicked() {
            m.designer.select(id);
            m.focus_block = true;
        }
        for (r, resize) in [(&resp, false), (&handle, true)] {
            if r.drag_started() && m.designer.begin_drag(id) {
                if let (Some(p), Some(b)) = (r.interact_pointer_pos(), m.designer.canvas().get(id)) {
                    m.drag = Some(DragState { id, resize, pointer: p, start: b.rect });
                    m.focus_block = true;
                }
            }
            if r.dragged() {
                if let (Some(d), Some(p)) =
                    (m.drag.as_ref().filter(|d| d.id == id && d.resize == resize), r.interact_pointer_pos())
                {
                    let delta = (p - d.pointer) / scale;
                    let (dx, dy) = (delta.x.round() as i32, delta.y.round() as i32);
                    if d.resize {
                        m.designer.resize_to(d.start.w.saturating_add(dx), d.start.h.saturating_add(dy), !alt);
                        m.guides = Guides::default();
                    } else {
                        m.guides = m.designer.drag_to(d.start.x.saturating_add(dx), d.start.y.saturating_add(dy), !alt);
                    }
                }
            }
            if r.drag_stopped() && m.drag.as_ref().is_some_and(|d| d.id == id && d.resize == resize) {
                m.designer.end_drag();
                m.drag = None;
                m.guides = Guides::default();
            }
        }
    }
}

fn ui(ctx: &egui::Context, m: &mut Model) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button("-").on_hover_text("Zoom out (Ctrl+Minus)").clicked() {
                act(m, &Action::ZoomOut);
            }
            ui.label(format!("{:.0} %", m.designer.zoom()));
            if ui.button("+").on_hover_text("Zoom in (Ctrl+Plus)").clicked() {
                act(m, &Action::ZoomIn);
            }
            let mut paper = m.designer.canvas().pages().first().copied().unwrap_or(Paper::A4);
            let before = paper;
            egui::ComboBox::from_label("Paper").selected_text(paper.name()).show_ui(ui, |ui| {
                ui.selectable_value(&mut paper, Paper::A4, "A4");
                ui.selectable_value(&mut paper, Paper::Letter, "Letter");
            });
            if paper != before {
                m.designer.set_paper(paper);
            }
            if ui.button("Undo").clicked() {
                act(m, &Action::Undo);
            }
            if ui.button("Redo").clicked() {
                act(m, &Action::Redo);
            }
            let status =
                m.designer.focus().and_then(|f| m.designer.canvas().a11y(f)).map_or_else(String::new, |a| a.label);
            ui.label(status);
        });
    });
    egui::CentralPanel::default().show(ctx, |ui| canvas_ui(ui, m));
}

// ---------------------------------------------------------------------------------------
// Window, event loop and benchmark harness

struct Gfx {
    window: Arc<Window>,
    state: egui_winit::State,
    painter: egui_wgpu::winit::Painter,
    ctx: egui::Context,
    adapter: String,
}

struct Bench {
    out: PathBuf,
    schedule: Schedule,
    cursor: Cursor,
    series: BTreeMap<Phase, Vec<f64>>,
    drag_started: Option<Instant>,
}

struct App {
    args: Args,
    model: Model,
    fonts: Vec<String>,
    gfx: Option<Gfx>,
    bench: Option<Bench>,
    proxy: EventLoopProxy<accesskit_winit::Event>,
    failure: Option<String>,
}

fn init(
    el: &ActiveEventLoop,
    args: &Args,
    proxy: EventLoopProxy<accesskit_winit::Event>,
) -> Result<(Gfx, Vec<String>), String> {
    let attrs = Window::default_attributes()
        .with_title("fid-spike-ui rptcanvas (VIZ.SHELL.SPKRPT)")
        .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 1000.0))
        .with_visible(false);
    let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
    let ctx = egui::Context::default();
    let (fonts, loaded) = load_fonts(args.font.as_deref(), args.cjk_font.as_deref())?;
    ctx.set_fonts(fonts);
    // Ctrl+Plus / Ctrl+Minus zoom the canvas, not egui's UI scale.
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    let present_mode = if args.bench.is_some() { wgpu::PresentMode::AutoNoVsync } else { wgpu::PresentMode::AutoVsync };
    let config = egui_wgpu::WgpuConfiguration { present_mode, ..Default::default() };
    let options = egui_wgpu::RendererOptions { msaa_samples: 1, ..Default::default() };
    let mut painter = pollster::block_on(egui_wgpu::winit::Painter::new(ctx.clone(), config, false, options));
    pollster::block_on(painter.set_window(egui::ViewportId::ROOT, Some(window.clone()))).map_err(|e| e.to_string())?;
    let rs = painter.render_state().ok_or_else(|| "no wgpu render state".to_string())?;
    let info = rs.adapter.get_info();
    let adapter = format!("{} ({:?}, {} {})", info.name, info.backend, info.driver, info.driver_info);
    install_cube(&rs);
    let mut state = egui_winit::State::new(
        ctx.clone(),
        egui::ViewportId::ROOT,
        &*window,
        Some(window.scale_factor() as f32),
        None,
        painter.max_texture_side(),
    );
    state.init_accesskit(el, &window, proxy);
    window.set_visible(true);
    window.request_redraw();
    Ok((Gfx { window, state, painter, ctx, adapter }, loaded))
}

impl App {
    fn finish(&mut self, el: &ActiveEventLoop) {
        el.exit();
        let Some(b) = self.bench.take() else { return };
        let canvas = match Canvas::parse(FIXTURE_CANVAS) {
            Ok(c) => c,
            Err(e) => {
                self.failure = Some(e.to_string());
                return;
            }
        };
        let keyboard = parse_keyboard(FIXTURE_KEYBOARD).ok().map(|steps| {
            let ok = run_keyboard(&canvas, &steps);
            (ok.iter().filter(|x| **x).count(), ok.len())
        });
        let shown = self.model.designer.canvas();
        let content = Content {
            pages: shown.pages().len(),
            blocks: shown.blocks().len(),
            tables: shown.count(Kind::Table),
            frames: shown.count(Kind::Frame3d),
        };
        let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
        let lock_text = std::fs::metadata(&lock_path)
            .ok()
            .filter(|m| m.len() <= lock::MAX_LOCK_BYTES as u64)
            .and_then(|_| std::fs::read_to_string(&lock_path).ok())
            .unwrap_or_default();
        let adapter = self.gfx.as_ref().map_or_else(|| "-".to_string(), |g| g.adapter.clone());
        let mut limitations: Vec<String> = LIMITATIONS.iter().map(|s| s.to_string()).collect();
        if self.fonts.len() < 2 {
            limitations.push("No CJK fallback font was found; CJK text renders as missing glyphs.".to_string());
        }
        let results = Results {
            env: Environment {
                hardware: UntrustedText::new(&self.args.hardware),
                adapter: UntrustedText::new(&adapter),
                h1: self.args.h1,
                egui: lock::version_text(&lock_text, "egui"),
                wgpu: lock::version_text(&lock_text, "wgpu"),
                winit: lock::version_text(&lock_text, "winit"),
            },
            fonts: self.fonts.clone(),
            content: Some(content),
            series: b.series,
            keyboard,
            ime: self.args.ime,
            screen_reader: self.args.screen_reader,
            limitations,
        };
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(&b.out)?;
            std::fs::write(b.out.join("REPORT.md"), render(&results))?;
            std::fs::write(b.out.join("timings.csv"), timings_csv(&results.series))
        };
        match write() {
            Ok(()) => println!("wrote {}", b.out.join("REPORT.md").display()),
            Err(e) => self.failure = Some(format!("{}: {e}", b.out.display())),
        }
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            return;
        }
        let t0 = Instant::now();
        let mut record = None;
        let mut done = false;
        if let Some(b) = self.bench.as_mut() {
            let dragged = b.drag_started.map_or(0.0, |t| t.elapsed().as_secs_f64());
            let m = &mut self.model;
            match b.schedule.next(&mut b.cursor, dragged) {
                Command::Idle => {}
                Command::DragTo(id, x, y) => {
                    if b.drag_started.is_none() {
                        b.drag_started = Some(Instant::now());
                        m.scroll_to = Some(egui::Vec2::ZERO);
                    }
                    if m.designer.dragging() != Some(id) {
                        m.designer.begin_drag(id);
                    }
                    m.guides = m.designer.drag_to(x, y, true);
                    record = Some(Phase::Drag);
                }
                Command::DragEnd => {
                    m.designer.end_drag();
                    m.guides = Guides::default();
                }
                Command::Zoom(phase, z) => {
                    m.designer.set_zoom(z.clamp(ZOOM_MIN, ZOOM_MAX));
                    record = (phase != Phase::Warmup).then_some(phase);
                }
                Command::Edit(id) => {
                    m.designer.select(id);
                    act(m, &Action::Enter);
                    m.reveal = true;
                }
                Command::Act(phase, a) => {
                    act(m, &a);
                    record = (phase != Phase::Warmup).then_some(phase);
                }
                Command::Done => done = true,
            }
        }
        if done {
            self.finish(el);
            return;
        }
        let Some(g) = self.gfx.as_mut() else { return };
        let raw = g.state.take_egui_input(&g.window);
        let model = &mut self.model;
        let out = g.ctx.run(raw, |ctx| ui(ctx, model));
        let delay = out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay);
        g.state.handle_platform_output(&g.window, out.platform_output);
        let prims = g.ctx.tessellate(out.shapes, out.pixels_per_point);
        g.painter.paint_and_update_textures(
            egui::ViewportId::ROOT,
            out.pixels_per_point,
            [0.42, 0.44, 0.48, 1.0],
            &prims,
            &out.textures_delta,
            Vec::new(),
        );
        if let (Some(phase), Some(b)) = (record, self.bench.as_mut()) {
            b.series.entry(phase).or_default().push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        if self.bench.is_some() || delay == Some(Duration::ZERO) {
            g.window.request_redraw();
        } else if let Some(d) = delay.filter(|d| *d < Duration::from_secs(3600)) {
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + d));
        }
    }
}

impl ApplicationHandler<accesskit_winit::Event> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        match init(el, &self.args, self.proxy.clone()) {
            Ok((g, fonts)) => {
                self.gfx = Some(g);
                self.fonts = fonts;
            }
            Err(e) => {
                self.failure = Some(e);
                el.exit();
            }
        }
    }

    fn new_events(&mut self, _el: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            if let Some(g) = &self.gfx {
                g.window.request_redraw();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = self.gfx.as_mut() else { return };
        let response = g.state.on_window_event(&g.window, &event);
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
                    g.painter.on_window_resized(egui::ViewportId::ROOT, w, h);
                }
            }
            WindowEvent::RedrawRequested => {
                self.frame(el);
                return;
            }
            _ => {}
        }
        if response.repaint {
            g.window.request_redraw();
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, event: accesskit_winit::Event) {
        let Some(g) = self.gfx.as_mut() else { return };
        match event.window_event {
            accesskit_winit::WindowEvent::InitialTreeRequested => g.ctx.enable_accesskit(),
            accesskit_winit::WindowEvent::ActionRequested(request) => g.state.on_accesskit_action_request(request),
            _ => g.ctx.disable_accesskit(),
        }
        g.window.request_redraw();
    }
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let canvas = match Canvas::parse(FIXTURE_CANVAS) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fixture: {e}");
            return ExitCode::from(1);
        }
    };
    let bench = match args.bench.clone() {
        Some(out) => match Schedule::for_canvas(&canvas, args.drag_seconds) {
            Some(schedule) => {
                Some(Bench { out, schedule, cursor: Cursor::default(), series: BTreeMap::new(), drag_started: None })
            }
            None => {
                eprintln!("fixture: page 1 needs two text blocks for the benchmark");
                return ExitCode::from(1);
            }
        },
        None => None,
    };
    let event_loop = match EventLoop::<accesskit_winit::Event>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("event loop: {e}");
            return ExitCode::from(1);
        }
    };
    let mut app = App {
        args,
        model: Model {
            designer: Designer::new(canvas),
            edit_buf: String::new(),
            edit_started: false,
            focus_block: true,
            reveal: false,
            drag: None,
            guides: Guides::default(),
            scroll_to: None,
            viewport: egui::Rect::NOTHING,
        },
        fonts: Vec::new(),
        gfx: None,
        bench,
        proxy: event_loop.create_proxy(),
        failure: None,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("event loop: {e}");
        return ExitCode::from(1);
    }
    match app.failure {
        Some(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
        None => ExitCode::SUCCESS,
    }
}
