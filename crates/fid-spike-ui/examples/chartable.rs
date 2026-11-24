//! `chartable`: the VIZ.SHELL.SPKTABLE prototype. An egui characteristic table of 10^5 rows
//! x 14 columns in an egui + winit + wgpu window: virtual scrolling, two frozen columns,
//! sort and filter computed on a background thread and swapped in, column resize and
//! reorder (pointer and keyboard), in-place editing that stores the typed decimal string
//! exactly, status chips with icon and text, TSV copy, a fly-to stub and AccessKit exposure
//! of the table, its header row and every cell on screen. Throwaway: it adopts nothing.
//!
//! ```text
//! cargo run --release -p fid-spike-ui --example chartable
//! cargo run --release -p fid-spike-ui --example chartable -- --bench <dir> --hardware "<CPU, RAM, OS>"
//!     [--h1] [--font <ttf>] [--screen-reader pass|fail] [--scroll-seconds <s>]
//! ```
//!
//! `--bench` runs the scripted schedule (30 s end-to-end scroll, sort by deviation, typing a
//! 6-character filter) with vsync off, times each frame with a monotonic clock from input to
//! submit, runs the AC-3 exact-decimal check and the AC-4 keyboard sequence on the model and
//! writes `<dir>/REPORT.md` and `<dir>/timings.csv`. `--screen-reader` records the outcome of
//! the manual AC-5 check once it has been done.
//!
//! Keys: arrows, PageUp / PageDown, Home / End (columns), Ctrl+Home / Ctrl+End (rows),
//! Shift+arrows (select), Alt+Up / Alt+Down (sort ascending / descending by the focused
//! column), Alt+Backspace (no sort), Ctrl+F (filter box; Enter keeps the filter, Escape
//! clears it), F2 or Enter (edit nominal or tolerance; typing a character starts an edit
//! too), Enter (commit), Escape (cancel), Ctrl+C (copy as TSV), Ctrl+Shift+Left / Right
//! (move column), Alt+Left / Right (narrow / widen column), Ctrl+Enter (fly-to stub).

use egui_wgpu::wgpu;
use egui_winit::accesskit_winit;
use egui_winit::winit;
use fid_hal_idl::UntrustedText;
use fid_spike_ui::lock;
use fid_spike_ui::report::{Environment, Outcome};
use fid_spike_ui::table::data::{Column, Status, Table};
use fid_spike_ui::table::grid::{compute_view, Action, Dir, Grid, Mode, Query, FROZEN, MAX_FILTER_CHARS};
use fid_spike_ui::table::report::{render, timings_csv, Content, Results};
use fid_spike_ui::table::script::{exact_decimal, parse_keyboard, run_keyboard, Command, Cursor, Phase, Schedule};
use fid_spike_ui::table::FIXTURE_KEYBOARD;
use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

const HEADER_H: f32 = 26.0;
const ROW_H: f32 = 22.0;
const BAR: f32 = 12.0;
const PAD: f32 = 5.0;
const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;
const FONT_CANDIDATES: [&str; 6] = [
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];
const LIMITATIONS: [&str; 9] = [
    "Frame time is CPU wall time from input to submit (vsync off); GPU completion is not awaited.",
    "The harness drives the model (scroll offset, sort request, filter characters), not OS input events.",
    "Sort and filter run on a background thread: the sort and filter series time the UI thread while the result is pending and for 10 frames after it is shown, not the sort itself.",
    "Copy builds the TSV but does not reach the OS clipboard: egui-winit's clipboard feature is off (clipboard-win is BSL-1.0, outside deny.toml's allow list). The text is shown in the in-app clipboard panel.",
    "Only rows on screen (virtualised) reach AccessKit; row and column indices use the full view. Whether screen readers associate headers through AccessKit is [unverified]; cell labels also carry both headers and the status.",
    "Edits do not re-sort or re-filter the view until the next sort or filter change.",
    "The fly-to action is a stub: it records the row and shows a message; this spike has no 3D view.",
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

const USAGE: &str = "usage: chartable [--bench <dir> --hardware <text>] [--h1] [--font <ttf>] \
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
// The table model and its background sorter

/// A sort or filter being computed on a background thread.
struct Worker {
    query: Query,
    rx: mpsc::Receiver<Vec<u32>>,
}

enum HeaderDrag {
    Resize { col: Column, start_w: i32, start_x: f32 },
    Move { col: Column },
}

struct Model {
    table: Arc<Table>,
    grid: Grid,
    worker: Option<Worker>,
    /// First visible row, fractional (virtual scrolling).
    scroll_top: f64,
    /// Horizontal offset of the unfrozen columns in logical pixels.
    scroll_x: f32,
    visible_rows: usize,
    /// Scroll so that the focused cell is on screen (after keyboard moves).
    reveal: bool,
    /// Give egui focus to the focused cell (for the screen reader).
    want_focus: bool,
    filter_buf: String,
    filter_focus: bool,
    edit_buf: String,
    edit_focus: bool,
    clipboard: Option<String>,
    pending_copy: Option<String>,
    last_fly_to: Option<u32>,
    message: String,
    drag: Option<HeaderDrag>,
    /// Cell widget ids drawn in the last frame.
    cells: HashMap<egui::Id, (u32, Column)>,
}

impl Model {
    fn new(table: Table) -> Model {
        let grid = Grid::new(&table);
        Model {
            table: Arc::new(table),
            grid,
            worker: None,
            scroll_top: 0.0,
            scroll_x: 0.0,
            visible_rows: 20,
            reveal: false,
            want_focus: true,
            filter_buf: String::new(),
            filter_focus: false,
            edit_buf: String::new(),
            edit_focus: false,
            clipboard: None,
            pending_copy: None,
            last_fly_to: None,
            message: String::new(),
            drag: None,
            cells: HashMap::new(),
        }
    }

    /// Whether a sort or filter is still being computed or not yet shown.
    fn busy(&self) -> bool {
        self.grid.is_stale() || self.worker.is_some()
    }

    /// Installs a finished background result and starts the next one if the request moved
    /// on meanwhile.
    fn poll_worker(&mut self) {
        if let Some(w) = &self.worker {
            match w.rx.try_recv() {
                Ok(view) => {
                    let q = w.query.clone();
                    self.worker = None;
                    if self.grid.install(&q, view) {
                        self.reveal = true;
                        self.want_focus = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.worker = None,
            }
        }
        if self.worker.is_none() && self.grid.is_stale() {
            let query = self.grid.query().clone();
            let (tx, rx) = mpsc::channel();
            let table = Arc::clone(&self.table);
            let q = query.clone();
            let spawned = std::thread::Builder::new().name("chartable-view".into()).spawn(move || {
                let _ = tx.send(compute_view(&table, &q));
            });
            match spawned {
                Ok(_) => self.worker = Some(Worker { query, rx }),
                Err(_) => {
                    // No thread: compute on the UI thread.
                    self.grid.refresh(&self.table);
                    self.reveal = true;
                }
            }
        }
    }
}

fn act(m: &mut Model, a: &Action) {
    let before = (m.grid.focus_row(), m.grid.focus_col(), m.grid.mode());
    let out = m.grid.apply(&m.table, a);
    if let Some(row) = out.edit {
        if let Some(slot) = Arc::make_mut(&mut m.table).get_mut(row.id) {
            *slot = row;
        }
    }
    if let Some(text) = m.grid.take_clipboard() {
        m.message = format!("Copied {} bytes as TSV (in-app clipboard).", text.len());
        m.pending_copy = Some(text.clone());
        m.clipboard = Some(text);
    }
    if m.grid.fly_to() != m.last_fly_to {
        m.last_fly_to = m.grid.fly_to();
        if let Some(row) = m.last_fly_to.and_then(|id| m.table.get(id)) {
            m.message = format!("Fly to {} (balloon {}): stub, no 3D view in this spike.", row.feature, row.balloon);
        }
    }
    let mode = m.grid.mode();
    if mode == Mode::Edit {
        // Keys typed in the same frame as the one that opened the editor reach the grid's
        // buffer before the editor exists; keep the editor's text in step.
        m.edit_buf = m.grid.buffer().to_string();
        if before.2 != Mode::Edit || *a == Action::Enter {
            m.edit_focus = true;
        }
    }
    if let Some(e) = m.grid.error() {
        m.message = format!("Not stored: {e}.");
    }
    if mode == Mode::Filter && before.2 != Mode::Filter {
        m.filter_focus = true;
    }
    if mode == Mode::Grid && before.2 != Mode::Grid {
        m.want_focus = true;
    }
    m.filter_buf.clone_from(&m.grid.query().filter);
    if (m.grid.focus_row(), m.grid.focus_col()) != (before.0, before.1) {
        m.reveal = true;
        m.want_focus = true;
    }
}

fn map_key(key: egui::Key, m: egui::Modifiers) -> Option<Action> {
    use egui::Key as K;
    let ctrl = m.ctrl || m.command || m.mac_cmd;
    let mods = (ctrl, m.shift, m.alt);
    const NONE: (bool, bool, bool) = (false, false, false);
    const SHIFT: (bool, bool, bool) = (false, true, false);
    const CTRL: (bool, bool, bool) = (true, false, false);
    const CTRL_SHIFT: (bool, bool, bool) = (true, true, false);
    const ALT: (bool, bool, bool) = (false, false, true);
    let a = match (key, mods) {
        (K::ArrowUp, NONE) => Action::Up,
        (K::ArrowUp, SHIFT) => Action::ShiftUp,
        (K::ArrowUp, ALT) => Action::SortAsc,
        (K::ArrowDown, NONE) => Action::Down,
        (K::ArrowDown, SHIFT) => Action::ShiftDown,
        (K::ArrowDown, ALT) => Action::SortDesc,
        (K::ArrowLeft, NONE) => Action::Left,
        (K::ArrowLeft, SHIFT) => Action::ShiftLeft,
        (K::ArrowLeft, CTRL_SHIFT) => Action::MoveLeft,
        (K::ArrowLeft, ALT) => Action::Narrow,
        (K::ArrowRight, NONE) => Action::Right,
        (K::ArrowRight, SHIFT) => Action::ShiftRight,
        (K::ArrowRight, CTRL_SHIFT) => Action::MoveRight,
        (K::ArrowRight, ALT) => Action::Widen,
        (K::PageUp, NONE) => Action::PageUp,
        (K::PageDown, NONE) => Action::PageDown,
        (K::Home, NONE) => Action::Home,
        (K::Home, CTRL) => Action::CtrlHome,
        (K::End, NONE) => Action::End,
        (K::End, CTRL) => Action::CtrlEnd,
        (K::F, CTRL) => Action::FocusFilter,
        (K::F2, NONE) => Action::F2,
        (K::Enter, NONE) => Action::Enter,
        (K::Enter, CTRL) => Action::FlyTo,
        (K::Escape, NONE) => Action::Escape,
        (K::Backspace, NONE) => Action::Backspace,
        (K::Backspace, ALT) => Action::SortClear,
        _ => return None,
    };
    Some(a)
}

/// Takes the grid's keys out of this frame's input (only called in grid mode).
fn grid_keys(ctx: &egui::Context) -> Vec<Action> {
    ctx.input_mut(|i| {
        let mut out = Vec::new();
        i.events.retain(|e| match e {
            egui::Event::Copy => {
                out.push(Action::Copy);
                false
            }
            egui::Event::Text(t) => {
                if !t.is_empty() && !t.chars().any(char::is_control) {
                    out.push(Action::Type(t.clone()));
                }
                false
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => match map_key(*key, *modifiers) {
                Some(a) => {
                    out.push(a);
                    false
                }
                None => true,
            },
            _ => true,
        });
        out
    })
}

fn status_color(s: Status) -> egui::Color32 {
    match s {
        Status::Pass => egui::Color32::from_rgb(46, 140, 80),
        Status::Warn => egui::Color32::from_rgb(190, 140, 20),
        Status::Fail => egui::Color32::from_rgb(200, 55, 55),
        Status::NotMeasured => egui::Color32::from_rgb(110, 110, 120),
    }
}

/// A status chip: coloured pill, an icon drawn as shapes (no icon font needed) and the word.
fn chip(p: &egui::Painter, cell: egui::Rect, s: Status) {
    let pill = egui::Rect::from_min_max(
        egui::pos2(cell.min.x + 3.0, cell.min.y + 3.0),
        egui::pos2(cell.max.x - 3.0, cell.max.y - 3.0),
    );
    let fill = status_color(s);
    p.rect_filled(pill, 8.0, fill);
    let c = egui::pos2(pill.min.x + 10.0, pill.center().y);
    let white = egui::Color32::WHITE;
    let stroke = egui::Stroke::new(1.8_f32, white);
    match s.icon() {
        "check" => {
            p.line_segment([c + egui::vec2(-4.0, 0.0), c + egui::vec2(-1.0, 3.0)], stroke);
            p.line_segment([c + egui::vec2(-1.0, 3.0), c + egui::vec2(4.0, -3.5)], stroke);
        }
        "triangle" => {
            let pts = vec![c + egui::vec2(0.0, -5.0), c + egui::vec2(5.0, 4.0), c + egui::vec2(-5.0, 4.0)];
            p.add(egui::Shape::convex_polygon(pts, white, egui::Stroke::NONE));
        }
        "cross" => {
            p.line_segment([c + egui::vec2(-3.5, -3.5), c + egui::vec2(3.5, 3.5)], stroke);
            p.line_segment([c + egui::vec2(-3.5, 3.5), c + egui::vec2(3.5, -3.5)], stroke);
        }
        _ => {
            p.line_segment([c + egui::vec2(-4.0, 0.0), c + egui::vec2(4.0, 0.0)], stroke);
        }
    }
    p.text(
        egui::pos2(pill.min.x + 20.0, pill.center().y),
        egui::Align2::LEFT_CENTER,
        s.word(),
        egui::FontId::proportional(12.5),
        white,
    );
}

fn sort_arrow(p: &egui::Painter, at: egui::Pos2, dir: Dir, color: egui::Color32) {
    let pts = match dir {
        Dir::Asc => vec![at + egui::vec2(0.0, -4.0), at + egui::vec2(4.0, 3.0), at + egui::vec2(-4.0, 3.0)],
        Dir::Desc => vec![at + egui::vec2(-4.0, -3.0), at + egui::vec2(4.0, -3.0), at + egui::vec2(0.0, 4.0)],
    };
    p.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

/// A plain scrollbar: returns the new value when the track is clicked or the thumb dragged.
fn scrollbar(
    ui: &egui::Ui,
    id: egui::Id,
    track: egui::Rect,
    vertical: bool,
    value: f64,
    max: f64,
    shown: f64,
) -> Option<f64> {
    if track.width() <= 0.0 || track.height() <= 0.0 {
        return None;
    }
    let len = if vertical { track.height() } else { track.width() } as f64;
    let total = max + shown;
    let thumb = if total > 0.0 { (len * shown / total).clamp(16.0_f64.min(len), len) } else { len };
    let free = (len - thumb).max(0.0);
    let at = if max > 0.0 { free * value / max } else { 0.0 };
    let rect = if vertical {
        egui::Rect::from_min_size(
            egui::pos2(track.min.x, track.min.y + at as f32),
            egui::vec2(track.width(), thumb as f32),
        )
    } else {
        egui::Rect::from_min_size(
            egui::pos2(track.min.x + at as f32, track.min.y),
            egui::vec2(thumb as f32, track.height()),
        )
    };
    let p = ui.painter_at(track);
    p.rect_filled(track, 0.0, ui.visuals().extreme_bg_color);
    p.rect_filled(rect.shrink(2.0), 4.0, ui.visuals().widgets.inactive.bg_fill);
    let resp = ui.interact(track, id, egui::Sense::click_and_drag());
    let pos = resp.interact_pointer_pos()?;
    if !(resp.dragged() || resp.clicked()) || free <= 0.0 {
        return None;
    }
    let off = if vertical { pos.y - track.min.y } else { pos.x - track.min.x } as f64 - thumb / 2.0;
    Some((off / free).clamp(0.0, 1.0) * max)
}

enum CellEvent {
    Click(u32, Column, bool),
    Edit(u32, Column),
    Focused(u32, Column),
}

fn table_ui(ui: &mut egui::Ui, m: &mut Model) {
    let ctx = ui.ctx().clone();
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, egui::Sense::hover());
    let header = egui::Rect::from_min_size(full.min, egui::vec2(full.width() - BAR, HEADER_H));
    let body =
        egui::Rect::from_min_max(egui::pos2(full.min.x, header.max.y), egui::pos2(full.max.x - BAR, full.max.y - BAR));
    let order: Vec<Column> = m.grid.order().to_vec();
    let widths: Vec<f32> = order.iter().map(|c| m.grid.width(*c) as f32).collect();
    let frozen_w: f32 = widths.iter().take(FROZEN).sum();
    let unfrozen_w: f32 = widths.iter().skip(FROZEN).sum();
    let region_x0 = body.min.x + frozen_w;
    let region_w = (body.max.x - region_x0).max(0.0);
    let n = m.grid.view().len();
    m.visible_rows = ((body.height() / ROW_H).floor() as usize).max(1);
    m.grid.page_rows = m.visible_rows;
    let max_top = n.saturating_sub(m.visible_rows) as f64;
    let max_x = (unfrozen_w - region_w).max(0.0);

    // Wheel scrolling (Shift+wheel scrolls sideways).
    if ui.rect_contains_pointer(full) {
        let d = ui.input(|i| i.smooth_scroll_delta);
        m.scroll_top -= f64::from(d.y / ROW_H);
        m.scroll_x -= d.x;
    }
    // Keep the focused cell on screen after keyboard moves.
    if m.reveal {
        m.reveal = false;
        if let Some(p) = m.grid.focus_pos() {
            let p = p as f64;
            if p < m.scroll_top {
                m.scroll_top = p;
            } else if p + 1.0 > m.scroll_top + m.visible_rows as f64 {
                m.scroll_top = p + 1.0 - m.visible_rows as f64;
            }
        }
        let cp = m.grid.column_pos(m.grid.focus_col());
        if cp >= FROZEN {
            let x: f32 = widths.iter().take(cp).skip(FROZEN).sum();
            let w = widths.get(cp).copied().unwrap_or(0.0);
            if x < m.scroll_x {
                m.scroll_x = x;
            } else if x + w > m.scroll_x + region_w {
                m.scroll_x = x + w - region_w;
            }
        }
    }
    m.scroll_top = m.scroll_top.clamp(0.0, max_top);
    m.scroll_x = m.scroll_x.clamp(0.0, max_x);

    // Column x positions (frozen ones fixed, the rest shifted by the horizontal scroll).
    let mut xs = Vec::with_capacity(order.len());
    let (mut fx, mut ux) = (body.min.x, region_x0 - m.scroll_x);
    for (k, w) in widths.iter().enumerate() {
        if k < FROZEN {
            xs.push(fx);
            fx += w;
        } else {
            xs.push(ux);
            ux += w;
        }
    }
    let frozen_clip = egui::Rect::from_min_max(egui::pos2(full.min.x, full.min.y), egui::pos2(region_x0, body.max.y));
    let region_clip = egui::Rect::from_min_max(egui::pos2(region_x0, full.min.y), egui::pos2(body.max.x, body.max.y));
    let clip_of = |k: usize| if k < FROZEN { frozen_clip } else { region_clip };

    let mut tui = ui.new_child(egui::UiBuilder::new().id_salt("chartable-table").max_rect(full));
    let (row_count, col_count) = (n + 1, order.len());
    ctx.accesskit_node_builder(tui.unique_id(), |node| {
        node.set_role(egui::accesskit::Role::Table);
        node.set_label("Characteristic table");
        node.set_row_count(row_count);
        node.set_column_count(col_count);
    });
    let visuals = ui.visuals().clone();
    let text_color = visuals.text_color();
    let line = visuals.widgets.noninteractive.bg_stroke;

    // Header row: sort by click, resize at the right edge, reorder by dragging.
    let hui = tui.new_child(egui::UiBuilder::new().id_salt("chartable-header").max_rect(header));
    ctx.accesskit_node_builder(hui.unique_id(), |node| {
        node.set_role(egui::accesskit::Role::Row);
        node.set_row_index(0);
    });
    let shown_sort = m.grid.shown_query().sort;
    let pointer = ctx.pointer_interact_pos();
    for (k, c) in order.iter().enumerate() {
        let (x, w) = (xs.get(k).copied().unwrap_or(0.0), widths.get(k).copied().unwrap_or(0.0));
        let rect = egui::Rect::from_min_size(egui::pos2(x, header.min.y), egui::vec2(w, HEADER_H));
        let vis = rect.intersect(clip_of(k));
        if vis.width() <= 0.0 {
            continue;
        }
        let p = hui.painter_at(vis);
        p.rect_filled(rect, 0.0, visuals.faint_bg_color);
        p.line_segment([rect.right_top(), rect.right_bottom()], line);
        p.text(
            egui::pos2(rect.min.x + PAD, rect.center().y),
            egui::Align2::LEFT_CENTER,
            c.title(),
            egui::FontId::proportional(13.5),
            text_color,
        );
        if let Some((_, dir)) = shown_sort.filter(|(sc, _)| sc == c) {
            sort_arrow(&p, egui::pos2(rect.max.x - 12.0, rect.center().y), dir, text_color);
        }
        let hid = egui::Id::new(("chartable-head", c.key()));
        let resp = hui.interact(vis, hid, egui::Sense::click_and_drag());
        let label = m.grid.header_label(*c);
        ctx.accesskit_node_builder(hid, |node| {
            node.set_role(egui::accesskit::Role::ColumnHeader);
            node.set_label(label);
            node.set_column_index(k);
        });
        let edge = egui::Rect::from_min_max(
            egui::pos2(rect.max.x - 4.0, rect.min.y),
            egui::pos2(rect.max.x + 4.0, rect.max.y),
        );
        let eresp = hui
            .interact(edge.intersect(clip_of(k)), hid.with("resize"), egui::Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if eresp.drag_started() {
            if let Some(px) = pointer {
                m.drag = Some(HeaderDrag::Resize { col: *c, start_w: m.grid.width(*c), start_x: px.x });
            }
        } else if resp.drag_started() {
            m.drag = Some(HeaderDrag::Move { col: *c });
        } else if resp.clicked() {
            let dir = if shown_sort == Some((*c, Dir::Asc)) { Dir::Desc } else { Dir::Asc };
            m.grid.set_sort(Some((*c, dir)));
        }
    }
    match (&m.drag, pointer) {
        (Some(HeaderDrag::Resize { col, start_w, start_x }), Some(px)) => {
            let w = *start_w + (px.x - *start_x).round() as i32;
            m.grid.set_width(*col, w);
        }
        (Some(HeaderDrag::Move { col }), Some(px)) if ctx.input(|i| i.pointer.any_released()) => {
            let target = (0..order.len()).find(|k| {
                let x = xs.get(*k).copied().unwrap_or(0.0);
                let w = widths.get(*k).copied().unwrap_or(0.0);
                px.x >= x && px.x < x + w && clip_of(*k).contains(px)
            });
            if let Some(t) = target {
                m.grid.move_column_to(*col, t);
            }
        }
        _ => {}
    }
    if ctx.input(|i| i.pointer.any_released()) {
        m.drag = None;
    }

    // Body rows: only the rows on screen exist (virtualisation).
    let first = m.scroll_top.floor() as usize;
    let last = (first + m.visible_rows + 1).min(n);
    let frac = (m.scroll_top - first as f64) as f32;
    let focus = (m.grid.focus_row(), m.grid.focus_col());
    let editing = m.grid.mode() == Mode::Edit;
    // The selection once per frame (finding the focused row's position scans the view).
    let sel = m.grid.selection();
    let in_sel =
        |k: usize, p: usize| sel.is_some_and(|((r0, r1), (c0, c1))| (r0..=r1).contains(&k) && (c0..=c1).contains(&p));
    let mut events = Vec::new();
    let mut edit_rect = None;
    let mut cells = HashMap::new();
    let body_clip_painter = tui.painter_at(body);
    for k in first..last {
        let Some(id) = m.grid.view().get(k).copied() else { continue };
        let Some(row) = m.table.get(id) else { continue };
        let y = body.min.y + ((k - first) as f32 - frac) * ROW_H;
        let row_rect = egui::Rect::from_min_size(egui::pos2(body.min.x, y), egui::vec2(body.width(), ROW_H));
        let rui =
            tui.new_child(egui::UiBuilder::new().id_salt(("chartable-row", id)).max_rect(row_rect.intersect(body)));
        ctx.accesskit_node_builder(rui.unique_id(), |node| {
            node.set_role(egui::accesskit::Role::Row);
            node.set_row_index(k + 1);
        });
        if k % 2 == 1 {
            body_clip_painter.rect_filled(row_rect, 0.0, visuals.faint_bg_color);
        }
        for (p, c) in order.iter().enumerate() {
            let (x, w) = (xs.get(p).copied().unwrap_or(0.0), widths.get(p).copied().unwrap_or(0.0));
            let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, ROW_H));
            let vis = cell.intersect(clip_of(p)).intersect(body);
            if vis.width() <= 0.0 || vis.height() <= 0.0 {
                continue;
            }
            let painter = rui.painter_at(vis);
            let is_focus = focus == (Some(id), *c);
            if in_sel(k, p) {
                painter.rect_filled(cell, 0.0, visuals.selection.bg_fill);
            }
            painter.line_segment([cell.right_top(), cell.right_bottom()], line);
            if *c == Column::Status {
                chip(&painter, cell, row.status);
            } else if !(is_focus && editing) {
                let text = row.text(*c);
                let (pos, align) = if c.numeric() || matches!(c, Column::Id | Column::Balloon) {
                    (egui::pos2(cell.max.x - PAD, cell.center().y), egui::Align2::RIGHT_CENTER)
                } else {
                    (egui::pos2(cell.min.x + PAD, cell.center().y), egui::Align2::LEFT_CENTER)
                };
                painter.text(pos, align, text, egui::FontId::proportional(13.5), text_color);
            }
            let cid = egui::Id::new(("chartable-cell", id, c.key()));
            cells.insert(cid, (id, *c));
            let resp = rui.interact(vis, cid, egui::Sense::click());
            if is_focus {
                painter.rect_stroke(cell.shrink(1.0), 0.0, visuals.selection.stroke, egui::StrokeKind::Inside);
                if editing {
                    edit_rect = Some(vis);
                } else if m.want_focus {
                    resp.request_focus();
                    m.want_focus = false;
                }
                ctx.memory_mut(|mem| {
                    mem.set_focus_lock_filter(
                        cid,
                        egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true },
                    )
                });
            } else if resp.has_focus() {
                events.push(CellEvent::Focused(id, *c));
            }
            if resp.double_clicked() {
                events.push(CellEvent::Edit(id, *c));
            } else if resp.clicked() {
                events.push(CellEvent::Click(id, *c, ui.input(|i| i.modifiers.shift)));
            }
            // AccessKit (REQ-VIZ.SHELL.SPKTABLE-004): cell role, indices, headers in the
            // label, value and status; frozen cells are row headers.
            if let Some(a) = m.grid.a11y(&m.table, k, *c) {
                let selected = in_sel(k, p);
                ctx.accesskit_node_builder(cid, |node| {
                    node.set_role(if a.row_header_cell {
                        egui::accesskit::Role::RowHeader
                    } else {
                        egui::accesskit::Role::Cell
                    });
                    node.set_row_index(a.row_index + 1);
                    node.set_column_index(a.column_index);
                    node.set_label(a.label());
                    node.set_description(a.description());
                    node.set_value(a.value.clone());
                    node.set_selected(selected);
                });
            }
        }
    }
    m.cells = cells;
    // Frozen-region edge.
    body_clip_painter.line_segment(
        [egui::pos2(region_x0, body.min.y), egui::pos2(region_x0, body.max.y)],
        egui::Stroke::new(2.0_f32, line.color),
    );

    // The cell editor: egui's single-line editor over the focused cell (IME included).
    if let Some(r) = edit_rect {
        let resp = tui.put(
            r,
            egui::TextEdit::singleline(&mut m.edit_buf)
                .id(egui::Id::new("chartable-edit"))
                .char_limit(fid_spike_ui::table::dec::MAX_DEC_CHARS),
        );
        if m.edit_focus {
            resp.request_focus();
            m.edit_focus = false;
        }
        if resp.changed() {
            m.grid.set_buffer(&m.edit_buf);
        }
        if resp.lost_focus() {
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            act(m, if esc { &Action::Escape } else { &Action::Enter });
        }
    } else if editing {
        // The edited cell is scrolled away: focus the editor again once it is back.
        m.edit_focus = true;
    }

    // Scrollbars.
    let vtrack = egui::Rect::from_min_max(egui::pos2(body.max.x, body.min.y), egui::pos2(full.max.x, body.max.y));
    if let Some(v) =
        scrollbar(&tui, egui::Id::new("chartable-vbar"), vtrack, true, m.scroll_top, max_top, m.visible_rows as f64)
    {
        m.scroll_top = v;
    }
    let htrack = egui::Rect::from_min_max(egui::pos2(region_x0, body.max.y), egui::pos2(body.max.x, full.max.y));
    if let Some(v) = scrollbar(
        &tui,
        egui::Id::new("chartable-hbar"),
        htrack,
        false,
        f64::from(m.scroll_x),
        f64::from(max_x),
        f64::from(region_w),
    ) {
        m.scroll_x = v as f32;
    }

    for e in events {
        match e {
            CellEvent::Click(id, c, shift) => {
                m.grid.click(id, c, shift);
                m.want_focus = true;
            }
            CellEvent::Edit(id, c) => {
                m.grid.click(id, c, false);
                act(m, &Action::F2);
            }
            CellEvent::Focused(id, c) => {
                // Tab (or the screen reader) moved egui focus to another cell.
                m.grid.click(id, c, false);
            }
        }
    }
}

fn ui(ctx: &egui::Context, m: &mut Model) {
    // Keyboard: in grid mode, unless a widget other than a cell holds egui focus.
    if m.grid.mode() == Mode::Grid {
        let focused = ctx.memory(|mem| mem.focused());
        let on_grid = focused.is_none_or(|id| m.cells.contains_key(&id));
        if on_grid {
            for a in grid_keys(ctx) {
                act(m, &a);
            }
        }
    }
    if let Some(text) = m.pending_copy.take() {
        ctx.copy_text(text);
    }

    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Filter");
            let r = ui.add(
                egui::TextEdit::singleline(&mut m.filter_buf)
                    .id(egui::Id::new("chartable-filter"))
                    .hint_text("Ctrl+F")
                    .desired_width(240.0)
                    .char_limit(MAX_FILTER_CHARS),
            );
            if m.filter_focus {
                r.request_focus();
                m.filter_focus = false;
            }
            if r.gained_focus() && m.grid.mode() == Mode::Grid {
                act(m, &Action::FocusFilter);
            }
            if r.changed() {
                m.grid.set_filter(&m.filter_buf);
            }
            if r.lost_focus() && m.grid.mode() == Mode::Filter {
                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                act(m, if esc { &Action::Escape } else { &Action::Enter });
            }
            let total = m.table.len();
            let sort = match m.grid.shown_query().sort {
                Some((c, Dir::Asc)) => format!(", sorted by {} ascending", c.title()),
                Some((c, Dir::Desc)) => format!(", sorted by {} descending", c.title()),
                None => String::new(),
            };
            ui.label(format!("{} of {} rows{}", m.grid.view().len(), total, sort));
            if m.busy() {
                ui.label("computing in the background...");
            }
        });
    });
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.label(UntrustedText::new(&m.message).as_display());
        if let Some(c) = &m.clipboard {
            egui::CollapsingHeader::new(format!("In-app clipboard ({} bytes)", c.len())).show(ui, |ui| {
                let shown: String = c.lines().take(20).map(|l| format!("{l}\n")).collect();
                ui.monospace(UntrustedText::new(&shown).as_display());
            });
        }
    });
    egui::CentralPanel::default().show(ctx, |ui| table_ui(ui, m));
    if m.busy() {
        ctx.request_repaint();
    }
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
    scroll_seconds: Option<f64>,
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
        .with_title("fid-spike-ui chartable (VIZ.SHELL.SPKTABLE)")
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
        // AC-3 and AC-4 run on a fresh fixture table, not the one the harness sorted.
        let fixture = Table::fixture();
        let keyboard = parse_keyboard(FIXTURE_KEYBOARD).ok().map(|steps| {
            let got = run_keyboard(&fixture, &steps);
            let ok = steps.iter().zip(&got).filter(|(s, g)| &s.expect == *g).count();
            (ok, steps.len())
        });
        let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
        let lock_text = std::fs::metadata(&lock_path)
            .ok()
            .filter(|m| m.len() <= lock::MAX_LOCK_BYTES as u64)
            .and_then(|_| std::fs::read_to_string(&lock_path).ok())
            .unwrap_or_default();
        let adapter = self.gfx.as_ref().map_or_else(|| "-".to_string(), |g| g.adapter.clone());
        let t = &self.model.table;
        let results = Results {
            env: Environment {
                hardware: UntrustedText::new(&self.args.hardware),
                adapter: UntrustedText::new(&adapter),
                h1: self.args.h1,
                egui: lock::version_text(&lock_text, "egui"),
                wgpu: lock::version_text(&lock_text, "wgpu"),
                winit: lock::version_text(&lock_text, "winit"),
            },
            content: Some(Content {
                rows: t.len(),
                columns: self.model.grid.order().len(),
                frozen: FROZEN,
                chips: t.rows().iter().all(|r| !r.status.icon().is_empty() && !r.status.word().is_empty()),
            }),
            series: b.series,
            scroll_seconds: b.scroll_seconds,
            exact: exact_decimal(&fixture),
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
        self.model.poll_worker();
        let mut record = None;
        let mut done = false;
        if let Some(b) = self.bench.as_mut() {
            let scrolled = b.scroll_started.map_or(0.0, |t| t.elapsed().as_secs_f64());
            let busy = self.model.busy();
            let m = &mut self.model;
            match b.schedule.next(&mut b.cursor, scrolled, m.grid.view().len(), m.visible_rows, busy) {
                Command::Idle => {}
                Command::ScrollTo(top) => {
                    let started = *b.scroll_started.get_or_insert_with(Instant::now);
                    b.scroll_seconds = Some(started.elapsed().as_secs_f64());
                    m.scroll_top = top;
                    record = Some(Phase::Scroll);
                }
                Command::Sort(phase, c, dir) => {
                    m.grid.set_sort(Some((c, dir)));
                    m.poll_worker();
                    record = Some(phase);
                }
                Command::Act(phase, a) => {
                    act(m, &a);
                    m.poll_worker();
                    record = (phase != Phase::Warmup).then_some(phase);
                }
                Command::Await(phase) => record = Some(phase),
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
    let table = Table::fixture();
    if table.is_empty() {
        eprintln!("fixture: table not generated");
        return ExitCode::from(1);
    }
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
        scroll_seconds: None,
    });
    let mut app =
        App { args, model: Model::new(table), gfx: None, bench, proxy: event_loop.create_proxy(), failure: None };
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
