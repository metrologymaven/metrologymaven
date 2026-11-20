//! `tree10k`: the VIZ.SHELL.SPKTREE prototype. An egui tree of 10 000 items (virtualised
//! rows, expand and collapse, multi-select, rename in place, drag to reorder, filter box,
//! keyboard navigation, AccessKit exposure) beside a wgpu viewport drawing 10^6 points,
//! integrated through egui-wgpu's paint callback. Throwaway: it adopts nothing.
//!
//! ```text
//! cargo run --release -p fid-spike-ui --example tree10k
//! cargo run --release -p fid-spike-ui --example tree10k -- --bench <dir> --hardware "<CPU, RAM, OS>"
//!     [--h1] [--font <ttf>] [--screen-reader pass|fail] [--scroll-seconds <s>]
//! ```
//!
//! `--bench` runs the scripted schedule (expand-all / collapse-all, 30 s scroll, filter
//! typing) with vsync off, times each frame with a monotonic clock from input to submit,
//! runs the AC-3 keyboard sequence and writes `<dir>/REPORT.md` and `<dir>/timings.csv`.
//! `--screen-reader` records the outcome of the manual AC-4 check once it has been done.

use egui_wgpu::wgpu;
use egui_winit::accesskit_winit;
use egui_winit::winit;
use fid_hal_idl::UntrustedText;
use fid_spike_ui::report::{render, timings_csv, Environment, Outcome, Results};
use fid_spike_ui::script::{parse_keyboard, Command, Cursor, Phase, Schedule};
use fid_spike_ui::tree::Tree;
use fid_spike_ui::view::{Action, TreeView};
use fid_spike_ui::{lock, run_keyboard, FIXTURE_KEYBOARD, FIXTURE_TREE};
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

const ROW_H: f32 = 20.0;
const INDENT: f32 = 16.0;
const POINTS: u32 = 1_000_000;
const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;
const FONT_CANDIDATES: [&str; 6] = [
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];
const LIMITATIONS: [&str; 6] = [
    "Frame time is CPU wall time from input to submit (vsync off); GPU completion is not awaited.",
    "The 10^6-point viewport is static geometry redrawn every frame.",
    "Only rows on screen (virtualised) reach AccessKit; position and set size use the full sibling list.",
    "No Tree container role is set; screen readers see tree items without a tree parent [unverified].",
    "egui default fonts are not built (licence gate in deny.toml); a system font is loaded at run time.",
    "No Metal backend (advisory gate on paste via metal); macOS is not measured.",
];

// ---------------------------------------------------------------------------------------
// Arguments

struct Args {
    bench: Option<PathBuf>,
    hardware: String,
    h1: bool,
    font: Option<PathBuf>,
    screen_reader: Outcome,
    scroll_seconds: f64,
}

const USAGE: &str = "usage: tree10k [--bench <dir> --hardware <text>] [--h1] [--font <ttf>] \
                     [--screen-reader pass|fail] [--scroll-seconds <s>]";

fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        bench: None,
        hardware: String::new(),
        h1: false,
        font: None,
        screen_reader: Outcome::NotRun,
        scroll_seconds: 30.0,
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
            "--screen-reader" if v == "pass" => a.screen_reader = Outcome::Pass,
            "--screen-reader" if v == "fail" => a.screen_reader = Outcome::Fail,
            "--scroll-seconds" => match v.parse::<f64>() {
                Ok(s) if s.is_finite() && s > 0.0 && s <= 3600.0 => a.scroll_seconds = s,
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

fn load_fonts(explicit: Option<&Path>) -> Result<egui::FontDefinitions, String> {
    let candidates: Vec<PathBuf> = match explicit {
        Some(p) => vec![p.to_path_buf()],
        None => FONT_CANDIDATES.iter().map(PathBuf::from).collect(),
    };
    for p in candidates {
        let small = std::fs::metadata(&p).map(|m| m.is_file() && m.len() <= MAX_FONT_BYTES).unwrap_or(false);
        if !small {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&p) {
            let mut fonts = egui::FontDefinitions::empty();
            fonts.font_data.insert("ui".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push("ui".to_owned());
            }
            return Ok(fonts);
        }
    }
    Err("no usable font found; pass --font <path to a .ttf>".to_string())
}

// ---------------------------------------------------------------------------------------
// The 10^6-point wgpu viewport

const SHADER: &str = r#"
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(@location(0) p: vec2<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(p, 0.0, 1.0);
    o.color = vec3<f32>(0.5 + 0.5 * p.x, 0.5 + 0.5 * p.y, 0.85);
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color, 1.0);
}
"#;

struct PointsResources {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
}

struct PointsCallback;

impl egui_wgpu::CallbackTrait for PointsCallback {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(r) = resources.get::<PointsResources>() {
            render_pass.set_pipeline(&r.pipeline);
            render_pass.set_vertex_buffer(0, r.buffer.slice(..));
            render_pass.draw(0..POINTS, 0..1);
        }
    }
}

/// A seeded ring-shaped cloud (SplitMix64, seed 5), as little-endian f32 pairs.
fn point_bytes() -> Vec<u8> {
    let mut s: u64 = 5;
    let mut next = move || {
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let unit = |v: u64| (v >> 40) as f32 / (1u64 << 24) as f32;
    let mut out = Vec::with_capacity(POINTS as usize * 8);
    for _ in 0..POINTS {
        let t = unit(next()) * std::f32::consts::TAU;
        let r = 0.2 + 0.7 * unit(next()).sqrt();
        out.extend_from_slice(&(r * t.cos()).to_le_bytes());
        out.extend_from_slice(&(r * t.sin()).to_le_bytes());
    }
    out
}

fn install_points(rs: &egui_wgpu::RenderState) {
    use wgpu::util::DeviceExt as _;
    let device = &rs.device;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("points"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("points"),
        bind_group_layouts: &[],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("points"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 8,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                }],
            }],
        },
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::PointList, ..Default::default() },
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
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("points"),
        contents: &point_bytes(),
        usage: wgpu::BufferUsages::VERTEX,
    });
    rs.renderer.write().callback_resources.insert(PointsResources { pipeline, buffer });
}

// ---------------------------------------------------------------------------------------
// The tree panel

struct Model {
    tree: Tree,
    view: TreeView,
    filter_buf: String,
    edit_buf: String,
    edit_started: bool,
    drag: Option<u32>,
    scroll_to: Option<f32>,
    visible: std::ops::Range<usize>,
    focus_row: bool,
}

enum RowEvent {
    Toggle(u32),
    Click(u32, bool, bool),
    Rename(u32),
    DragStart(u32),
    Drop(u32),
}

fn row_id(id: u32) -> egui::Id {
    egui::Id::new(("spktree-row", id))
}

fn act(m: &mut Model, a: &Action) {
    if let Action::Filter(t) = a {
        m.filter_buf.clone_from(t);
    }
    let was_editing = m.view.editing().is_some();
    m.view.apply(&mut m.tree, a);
    if !was_editing {
        if let Some((_, seed)) = m.view.editing() {
            m.edit_buf = seed.to_string();
            m.edit_started = true;
        }
    }
}

/// Scrolls so that the focused row is on screen (used after keyboard moves).
fn reveal_focus(m: &mut Model) {
    let Some(k) = m.view.focus().and_then(|f| m.view.row_of(f)) else { return };
    let (start, end) = (m.visible.start, m.visible.end);
    if k < start {
        m.scroll_to = Some(k as f32 * ROW_H);
    } else if k + 1 >= end && end > start {
        let shown = end - start;
        m.scroll_to = Some((k + 2).saturating_sub(shown) as f32 * ROW_H);
    }
}

fn keyboard_actions(ctx: &egui::Context) -> Vec<Action> {
    use egui::{Key, Modifiers};
    let table = [
        (Modifiers::SHIFT, Key::ArrowDown, Action::ShiftDown),
        (Modifiers::SHIFT, Key::ArrowUp, Action::ShiftUp),
        (Modifiers::COMMAND, Key::ArrowDown, Action::CtrlDown),
        (Modifiers::COMMAND, Key::ArrowUp, Action::CtrlUp),
        (Modifiers::NONE, Key::ArrowDown, Action::Down),
        (Modifiers::NONE, Key::ArrowUp, Action::Up),
        (Modifiers::NONE, Key::ArrowRight, Action::Right),
        (Modifiers::NONE, Key::ArrowLeft, Action::Left),
        (Modifiers::NONE, Key::Home, Action::Home),
        (Modifiers::NONE, Key::End, Action::End),
        (Modifiers::NONE, Key::Space, Action::Space),
        (Modifiers::NONE, Key::F2, Action::F2),
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

fn status_color(status: &str) -> egui::Color32 {
    match status {
        "pass" => egui::Color32::from_rgb(60, 170, 90),
        "warn" => egui::Color32::from_rgb(220, 170, 40),
        "fail" => egui::Color32::from_rgb(210, 60, 60),
        _ => egui::Color32::GRAY,
    }
}

fn tree_ui(ui: &mut egui::Ui, m: &mut Model) {
    let ctx = ui.ctx().clone();
    // Keyboard: only while the focused row holds egui focus and no rename is open.
    let focused_row = m.view.focus().map(row_id);
    let has_focus = focused_row.is_some_and(|id| ctx.memory(|mem| mem.has_focus(id)));
    if has_focus && m.view.editing().is_none() {
        let before = m.view.focus();
        for a in keyboard_actions(&ctx) {
            act(m, &a);
        }
        if m.view.focus() != before {
            reveal_focus(m);
            m.focus_row = true;
        }
    }

    let mut events = Vec::new();
    let mut edit_done: Option<Action> = None;
    let mut area = egui::ScrollArea::vertical().id_salt("spktree-rows").auto_shrink([false, false]);
    if let Some(off) = m.scroll_to.take() {
        area = area.vertical_scroll_offset(off);
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    let row_count = m.view.rows().len();
    let Model { tree, view, edit_buf, edit_started, drag, focus_row, .. } = m;
    let out = area.show_rows(ui, ROW_H, row_count, |ui, range| {
        let pointer_released = ui.input(|i| i.pointer.any_released());
        for k in range.clone() {
            let Some(row) = view.rows().get(k).copied() else { continue };
            let Some(item) = tree.get(row.id) else { continue };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), egui::Sense::hover());
            let id = row_id(row.id);
            let resp = ui.interact(rect, id, egui::Sense::click_and_drag());
            if *focus_row && view.focus() == Some(row.id) {
                resp.request_focus();
                *focus_row = false;
            }
            if view.focus() == Some(row.id) {
                ui.memory_mut(|mem| {
                    mem.set_focus_lock_filter(
                        id,
                        egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: false },
                    )
                });
            }
            let painter = ui.painter_at(rect);
            let sel_fill = ui.visuals().selection.bg_fill;
            let sel_stroke = ui.visuals().selection.stroke;
            let text_color = ui.visuals().text_color();
            if view.selection().contains(&row.id) {
                painter.rect_filled(rect, 0.0, sel_fill);
            }
            if resp.has_focus() {
                painter.rect_stroke(rect, 0.0, sel_stroke, egui::StrokeKind::Inside);
            }
            let x0 = rect.min.x + (row.level.saturating_sub(1)) as f32 * INDENT;
            let has_children = !tree.children(row.id).is_empty();
            if has_children {
                let sign = if view.is_expanded(row.id) || !view.filter().is_empty() { "-" } else { "+" };
                painter.text(
                    egui::pos2(x0 + 4.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    sign,
                    egui::FontId::monospace(13.0),
                    text_color,
                );
            }
            painter.circle_filled(egui::pos2(x0 + INDENT + 5.0, rect.center().y), 4.0, status_color(&item.status));
            let text_min = egui::pos2(x0 + INDENT + 14.0, rect.min.y);
            match view.editing() {
                Some((eid, _)) if eid == row.id => {
                    let edit_rect = egui::Rect::from_min_max(text_min, rect.max);
                    let r = ui.put(edit_rect, egui::TextEdit::singleline(edit_buf).char_limit(256));
                    if *edit_started {
                        r.request_focus();
                        *edit_started = false;
                    }
                    if r.changed() {
                        edit_done = Some(Action::Type(edit_buf.clone()));
                    }
                    if r.lost_focus() {
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        edit_done = Some(if enter { Action::Enter } else { Action::Escape });
                    }
                }
                _ => {
                    painter.text(
                        egui::pos2(text_min.x, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        item.display(),
                        egui::FontId::proportional(14.0),
                        text_color,
                    );
                }
            }
            if drag.is_some() && ui.rect_contains_pointer(rect) {
                painter.hline(rect.x_range(), rect.min.y, sel_stroke);
                if pointer_released {
                    events.push(RowEvent::Drop(row.id));
                }
            }
            if resp.double_clicked() {
                events.push(RowEvent::Rename(row.id));
            } else if resp.clicked() {
                let mods = ui.input(|i| i.modifiers);
                let on_toggle = resp.interact_pointer_pos().is_some_and(|p| p.x >= x0 && p.x < x0 + INDENT);
                if has_children && on_toggle {
                    events.push(RowEvent::Toggle(row.id));
                } else {
                    events.push(RowEvent::Click(row.id, mods.command, mods.shift));
                }
            }
            if resp.drag_started() {
                events.push(RowEvent::DragStart(row.id));
            }
            // AccessKit (REQ-VIZ.SHELL.SPKTREE-004): role, name, level, expanded, selected.
            if let Some(a) = view.a11y(tree, k) {
                ctx.accesskit_node_builder(id, |node| {
                    node.set_role(egui::accesskit::Role::TreeItem);
                    node.set_label(a.name.clone());
                    node.set_level(a.level);
                    if let Some(e) = a.expanded {
                        node.set_expanded(e);
                    }
                    node.set_selected(a.selected);
                    node.set_position_in_set(a.position_in_set);
                    node.set_size_of_set(a.size_of_set);
                });
            }
        }
        range
    });
    m.visible = out.inner;

    if let Some(a) = edit_done {
        let ends = matches!(a, Action::Enter | Action::Escape);
        act(m, &a);
        if ends {
            m.focus_row = true;
        }
    }
    for e in events {
        match e {
            RowEvent::Toggle(id) => m.view.toggle(&m.tree, id),
            RowEvent::Click(id, ctrl, shift) => {
                m.view.click(id, ctrl, shift);
                m.focus_row = true;
            }
            RowEvent::Rename(id) => {
                m.view.click(id, false, false);
                act(m, &Action::F2);
            }
            RowEvent::DragStart(id) => m.drag = Some(id),
            RowEvent::Drop(target) => {
                if let Some(src) = m.drag.take() {
                    if src != target && m.tree.move_before(src, target).is_ok() {
                        m.view.rebuild(&m.tree);
                    }
                }
            }
        }
    }
    if ctx.input(|i| i.pointer.any_released()) {
        m.drag = None;
    }
}

fn ui(ctx: &egui::Context, m: &mut Model) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Filter");
            let r = ui.add(egui::TextEdit::singleline(&mut m.filter_buf).desired_width(240.0).char_limit(256));
            if r.changed() {
                m.view.set_filter(&m.tree, &m.filter_buf);
            }
            if ui.button("Expand all").clicked() {
                act(m, &Action::ExpandAll);
            }
            if ui.button("Collapse all").clicked() {
                act(m, &Action::CollapseAll);
            }
            ui.label(format!("{} of {} items shown", m.view.rows().len(), m.tree.len()));
        });
    });
    egui::SidePanel::left("tree").resizable(true).default_width(440.0).show(ctx, |ui| tree_ui(ui, m));
    egui::CentralPanel::default().show(ctx, |ui| {
        let rect = ui.available_rect_before_wrap();
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, PointsCallback));
        ui.allocate_rect(rect, egui::Sense::hover());
    });
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
    scroll_started: Option<Instant>,
}

struct App {
    args: Args,
    model: Model,
    gfx: Option<Gfx>,
    bench: Option<Bench>,
    proxy: EventLoopProxy<accesskit_winit::Event>,
    failure: Option<String>,
}

fn init(el: &ActiveEventLoop, args: &Args, proxy: EventLoopProxy<accesskit_winit::Event>) -> Result<Gfx, String> {
    let attrs = Window::default_attributes()
        .with_title("fid-spike-ui tree10k (VIZ.SHELL.SPKTREE)")
        .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 900.0))
        .with_visible(false);
    let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
    let ctx = egui::Context::default();
    ctx.set_fonts(load_fonts(args.font.as_deref())?);
    let present_mode = if args.bench.is_some() { wgpu::PresentMode::AutoNoVsync } else { wgpu::PresentMode::AutoVsync };
    let config = egui_wgpu::WgpuConfiguration { present_mode, ..Default::default() };
    let options = egui_wgpu::RendererOptions { msaa_samples: 1, ..Default::default() };
    let mut painter = pollster::block_on(egui_wgpu::winit::Painter::new(ctx.clone(), config, false, options));
    pollster::block_on(painter.set_window(egui::ViewportId::ROOT, Some(window.clone()))).map_err(|e| e.to_string())?;
    let rs = painter.render_state().ok_or_else(|| "no wgpu render state".to_string())?;
    let info = rs.adapter.get_info();
    let adapter = format!("{} ({:?}, {} {})", info.name, info.backend, info.driver, info.driver_info);
    install_points(&rs);
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
    Ok(Gfx { window, state, painter, ctx, adapter })
}

impl App {
    fn finish(&mut self, el: &ActiveEventLoop) {
        el.exit();
        let Some(b) = self.bench.take() else { return };
        let mut tree = match Tree::parse(FIXTURE_TREE) {
            Ok(t) => t,
            Err(e) => {
                self.failure = Some(e.to_string());
                return;
            }
        };
        let keyboard = parse_keyboard(FIXTURE_KEYBOARD).ok().map(|steps| {
            let ok = run_keyboard(&mut tree, &steps);
            (ok.iter().filter(|x| **x).count(), ok.len())
        });
        let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
        let lock_text = std::fs::metadata(&lock_path)
            .ok()
            .filter(|m| m.len() <= lock::MAX_LOCK_BYTES as u64)
            .and_then(|_| std::fs::read_to_string(&lock_path).ok())
            .unwrap_or_default();
        let adapter = self.gfx.as_ref().map_or_else(|| "-".to_string(), |g| g.adapter.clone());
        let results = Results {
            env: Environment {
                hardware: UntrustedText::new(&self.args.hardware),
                adapter: UntrustedText::new(&adapter),
                h1: self.args.h1,
                egui: lock::version_text(&lock_text, "egui"),
                wgpu: lock::version_text(&lock_text, "wgpu"),
                winit: lock::version_text(&lock_text, "winit"),
            },
            series: b.series,
            keyboard,
            screen_reader: self.args.screen_reader,
            limitations: LIMITATIONS.iter().map(|s| s.to_string()).collect(),
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
            let scrolled = b.scroll_started.map_or(0.0, |t| t.elapsed().as_secs_f64());
            match b.schedule.next(&mut b.cursor, self.model.view.rows().len(), scrolled) {
                Command::Idle => {}
                Command::Act(phase, a) => {
                    act(&mut self.model, &a);
                    record = (phase != Phase::Warmup).then_some(phase);
                }
                Command::ScrollTo(top) => {
                    b.scroll_started.get_or_insert_with(Instant::now);
                    self.model.scroll_to = Some(top as f32 * ROW_H);
                    record = Some(Phase::Scroll);
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
            [0.06, 0.06, 0.08, 1.0],
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
            Ok(g) => self.gfx = Some(g),
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
    let tree = match Tree::parse(FIXTURE_TREE) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("fixture: {e}");
            return ExitCode::from(1);
        }
    };
    let view = TreeView::new(&tree);
    let event_loop = match EventLoop::<accesskit_winit::Event>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("event loop: {e}");
            return ExitCode::from(1);
        }
    };
    let bench = args.bench.clone().map(|out| Bench {
        out,
        schedule: Schedule { scroll_seconds: args.scroll_seconds, ..Schedule::default() },
        cursor: Cursor::default(),
        series: BTreeMap::new(),
        scroll_started: None,
    });
    let mut app = App {
        args,
        model: Model {
            tree,
            view,
            filter_buf: String::new(),
            edit_buf: String::new(),
            edit_started: false,
            drag: None,
            scroll_to: None,
            visible: 0..0,
            focus_row: true,
        },
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
