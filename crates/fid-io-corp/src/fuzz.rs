//! Fuzz harness, target generation, coverage check, seed staging and crash minimisation
//! (spec node `IO.CORP.FUZZ`).
//!
//! - **REQ-IO.CORP.FUZZ-001:** Every registered first-party reader has a fuzz target;
//!   [`check_coverage`] fails with [`CorpError::FuzzTargetMissing`] (`fuzz_target_missing`)
//!   naming any reader without one.
//! - **REQ-IO.CORP.FUZZ-002:** CI budgets are pinned as [`FUZZ_MERGE_BUDGET_SECS`] (600 s =
//!   10 CPU-minutes per merge) and [`FUZZ_WEEKLY_BUDGET_SECS`] (28 800 s = 8 CPU-hours weekly).
//! - **REQ-IO.CORP.FUZZ-003:** Any panic, abort, per-input timeout ([`FUZZ_PER_INPUT_TIMEOUT_SECS`]
//!   = 10 s) or memory-cap breach ([`FUZZ_RSS_LIMIT_MIB`] = 2 048 MiB) fails the run and saves
//!   the minimised input + structured `finding.json` for upload.
//! - **REQ-IO.CORP.FUZZ-004:** Every reader runs through `fid-io-reg`'s [`BoundedSource`] and
//!   [`Budget`] with small fuzz limits ([`FUZZ_LIMIT_BYTES`] = 16 MiB, [`FUZZ_LIMIT_DEPTH`] = 64).

use crate::error::CorpError;
use crate::fetch::cache_path;
use crate::manifest::{FormatId, Location, Manifest};
use fid_canon::CValue;
use fid_io_reg::{BoundedSource, Budget, IoError, LimitRequest, Limits, Position, Principal};
use fid_num::rng::{Address, Cursor};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

/// Small per-input byte limit for fuzzing (16 MiB, REQ-IO.CORP.FUZZ-004).
pub const FUZZ_LIMIT_BYTES: u64 = 16 * 1024 * 1024;
/// Per-input element limit for fuzzing (1 000 000 elements).
pub const FUZZ_LIMIT_ELEMENTS: u64 = 1_000_000;
/// Nesting depth limit for fuzzing (depth 64, REQ-IO.CORP.FUZZ-004).
pub const FUZZ_LIMIT_DEPTH: u32 = 64;
/// Tracked allocation cap inside the harness (128 MiB, well below the 2 GiB RSS cap).
pub const FUZZ_LIMIT_ALLOC: u64 = 128 * 1024 * 1024;
/// Wall-time limit inside `IO.REG.LIMITS` (9 s, inside the 10 s external timeout).
pub const FUZZ_LIMIT_WALL_MS: u64 = 9_000;

/// External per-input timeout in seconds (REQ-IO.CORP.FUZZ-003).
pub const FUZZ_PER_INPUT_TIMEOUT_SECS: u64 = 10;
/// External per-target RSS memory cap in MiB (2 GiB, REQ-IO.CORP.FUZZ-003).
pub const FUZZ_RSS_LIMIT_MIB: u64 = 2_048;
/// Per-target CPU budget on merge to main (10 CPU-minutes, REQ-IO.CORP.FUZZ-002).
pub const FUZZ_MERGE_BUDGET_SECS: u64 = 600;
/// Per-target CPU budget on weekly run (8 CPU-hours, REQ-IO.CORP.FUZZ-002).
pub const FUZZ_WEEKLY_BUDGET_SECS: u64 = 28_800;

/// Largest single seed file accepted when staging or loading seeds.
pub const MAX_SEED_BYTES: u64 = FUZZ_LIMIT_BYTES;
/// Maximum number of seed files loaded for one fuzz target.
pub const MAX_SEEDS_PER_TARGET: usize = 65_536;
/// Maximum length of a mutated in-process fuzz input (64 KiB keeps mutations fast while seeds may be up to 16 MiB).
const MAX_MUTATED_BYTES: usize = 64 * 1024;
/// Interesting edge bytes injected during mutation (includes 0x00 and 0xFF).
const INTERESTING_BYTES: [u8; 8] = [0x00, 0x01, 0x0a, 0x20, 0x7f, 0x80, 0xfe, 0xff];

/// The small `IO.REG.LIMITS` override request used by every fuzz target (REQ-IO.CORP.FUZZ-004).
pub fn fuzz_limit_request() -> LimitRequest {
    LimitRequest::default()
        .bytes(FUZZ_LIMIT_BYTES)
        .elements(FUZZ_LIMIT_ELEMENTS)
        .depth(FUZZ_LIMIT_DEPTH)
        .alloc(FUZZ_LIMIT_ALLOC)
        .wall_ms(FUZZ_LIMIT_WALL_MS)
}

/// Resolves the fuzz limits under `Principal::Script` (every limit is `<= Limits::DEFAULT`).
pub fn fuzz_limits() -> Result<Limits, IoError> {
    Limits::resolve(Principal::Script, &fuzz_limit_request())
}

/// Builds a fresh `fid-io-reg` [`Budget`] with the small fuzz limits (REQ-IO.CORP.FUZZ-004).
pub fn fuzz_budget() -> Result<Budget, IoError> {
    Budget::new(Principal::Script, &fuzz_limit_request())
}

/// A first-party format reader driven through the `IO.REG.LIMITS` fuzz harness.
pub trait FuzzReader: Send + Sync {
    /// Registered format ID, e.g. `"mesh.stl"`.
    fn reader_id(&self) -> &str;

    /// Parses arbitrary bytes from `source`, charging bytes, elements, depth and tracked
    /// allocations to `source.budget_mut()`. Must return `Ok(())` or a typed [`IoError`] and
    /// never panic, abort or hang.
    fn read_bounded(&self, source: &mut BoundedSource<&[u8]>) -> Result<(), IoError>;
}

/// Drives any [`FuzzReader`] on arbitrary bytes under the small `IO.REG.LIMITS` fuzz budget
/// (REQ-IO.CORP.FUZZ-004).
pub fn drive_reader(reader: &dyn FuzzReader, data: &[u8]) -> Result<(), IoError> {
    let budget = fuzz_budget()?;
    let mut src = BoundedSource::new(data, budget);
    reader.read_bounded(&mut src)
}

// ------------------------------------------------------------------ built-in STL reader

/// Hardened first-party STL reader (`mesh.stl`, binary and ASCII) driven through `BoundedSource`
/// and `Budget` (zero `unsafe`, zero panics). Used by the `mesh_stl` fuzz target and AC-1 / AC-3.
#[derive(Debug, Default, Clone, Copy)]
pub struct StlFuzzReader;

impl FuzzReader for StlFuzzReader {
    fn reader_id(&self) -> &str {
        "mesh.stl"
    }

    fn read_bounded(&self, source: &mut BoundedSource<&[u8]>) -> Result<(), IoError> {
        let mut bytes = Vec::new();
        source.read_to_end_bounded(&mut bytes)?;
        let budget = source.budget_mut();
        if bytes.is_empty() {
            return Err(IoError::TruncatedFile { at: Position::byte(0) });
        }
        // Distinguish binary vs ASCII STL:
        // If total length >= 84 and equals 84 + 50 * facet_count, parse as binary STL;
        // otherwise if it starts with ASCII "solid" (after optional ASCII whitespace), parse as ASCII STL;
        // otherwise parse as binary STL (which reports truncated_file / trailing_data).
        if bytes.len() >= 84 {
            let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]);
            let expected = 84u64.saturating_add(u64::from(count).saturating_mul(50));
            if expected == bytes.len() as u64 {
                return parse_binary_stl(&bytes, count, budget);
            }
        }
        let trimmed = skip_ascii_ws(&bytes);
        if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case(b"solid") {
            return parse_ascii_stl(&bytes, budget);
        }
        if bytes.len() < 84 {
            return Err(IoError::TruncatedFile { at: Position::byte(bytes.len() as u64) });
        }
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]);
        parse_binary_stl(&bytes, count, budget)
    }
}

fn skip_ascii_ws(mut s: &[u8]) -> &[u8] {
    while let Some((&b, rest)) = s.split_first() {
        if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn parse_binary_stl(bytes: &[u8], count: u32, budget: &mut Budget) -> Result<(), IoError> {
    let expected = 84u64.saturating_add(u64::from(count).saturating_mul(50));
    if (bytes.len() as u64) < expected {
        // Charge the declared facet count before truncation if it exceeds the element limit,
        // so hostile count fields are caught as `limit_exceeded` (REQ-IO.CORP.FUZZ-004).
        if u64::from(count) > budget.limits().elements() {
            budget.charge_elements(u64::from(count), Position::byte(80))?;
        }
        return Err(IoError::TruncatedFile { at: Position::byte(bytes.len() as u64) });
    }
    if (bytes.len() as u64) > expected {
        return Err(IoError::Malformed { at: Position::byte(expected), reason: "trailing_bytes_after_stl_facets" });
    }
    let mut off = 84usize;
    for _ in 0..count {
        let at = Position::byte(off as u64);
        budget.charge_elements(1, at)?;
        budget.check_wall(at)?;
        let Some(rec) = bytes.get(off..off + 50) else {
            return Err(IoError::TruncatedFile { at });
        };
        // Check all 12 IEEE-754 binary32 coordinates (normal + 3 vertices) are finite.
        for f_idx in 0..12usize {
            let fo = f_idx * 4;
            let val = f32::from_le_bytes([rec[fo], rec[fo + 1], rec[fo + 2], rec[fo + 3]]);
            if !val.is_finite() {
                return Err(IoError::Malformed {
                    at: Position::byte((off + fo) as u64),
                    reason: "non_finite_stl_float",
                });
            }
        }
        off += 50;
    }
    Ok(())
}

fn parse_ascii_stl(bytes: &[u8], budget: &mut Budget) -> Result<(), IoError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| IoError::Malformed { at: Position::byte(e.valid_up_to() as u64), reason: "non_utf8_ascii_stl" })?;
    let mut lines = text.lines().enumerate();
    let mut in_solid = false;
    let mut in_facet = false;
    let mut in_loop = false;
    let mut verts_in_loop: u8 = 0;
    let mut offset: u64 = 0;

    for (line_idx, raw_line) in lines.by_ref() {
        let line_no = u32::try_from(line_idx.saturating_add(1)).unwrap_or(u32::MAX);
        let at = Position::line_col(offset, line_no, 1);
        offset = offset.saturating_add(raw_line.len() as u64).saturating_add(1);
        budget.check_wall(at)?;

        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_ascii_whitespace();
        let Some(head) = parts.next() else {
            continue;
        };
        if !in_solid {
            if head.eq_ignore_ascii_case("solid") {
                in_solid = true;
                continue;
            }
            return Err(IoError::Malformed { at, reason: "expected_solid" });
        }
        if head.eq_ignore_ascii_case("endsolid") {
            if in_facet || in_loop {
                return Err(IoError::Malformed { at, reason: "unclosed_facet_at_endsolid" });
            }
            return Ok(());
        }
        if head.eq_ignore_ascii_case("facet") {
            if in_facet {
                return Err(IoError::Malformed { at, reason: "nested_facet" });
            }
            let mut guard = budget.enter(at)?;
            guard.charge_elements(1, at)?;
            let Some(kw) = parts.next() else {
                return Err(IoError::Malformed { at, reason: "missing_normal_keyword" });
            };
            if !kw.eq_ignore_ascii_case("normal") {
                return Err(IoError::Malformed { at, reason: "expected_normal_keyword" });
            }
            parse_three_f64(&mut parts, at)?;
            in_facet = true;
            continue;
        }
        if head.eq_ignore_ascii_case("outer") {
            if !in_facet || in_loop {
                return Err(IoError::Malformed { at, reason: "unexpected_outer_loop" });
            }
            let Some(kw) = parts.next() else {
                return Err(IoError::Malformed { at, reason: "missing_loop_keyword" });
            };
            if !kw.eq_ignore_ascii_case("loop") {
                return Err(IoError::Malformed { at, reason: "expected_loop_keyword" });
            }
            in_loop = true;
            verts_in_loop = 0;
            continue;
        }
        if head.eq_ignore_ascii_case("vertex") {
            if !in_loop || verts_in_loop >= 3 {
                return Err(IoError::Malformed { at, reason: "unexpected_vertex" });
            }
            parse_three_f64(&mut parts, at)?;
            verts_in_loop += 1;
            continue;
        }
        if head.eq_ignore_ascii_case("endloop") {
            if !in_loop || verts_in_loop != 3 {
                return Err(IoError::Malformed { at, reason: "incomplete_vertex_loop" });
            }
            in_loop = false;
            continue;
        }
        if head.eq_ignore_ascii_case("endfacet") {
            if !in_facet || in_loop {
                return Err(IoError::Malformed { at, reason: "unexpected_endfacet" });
            }
            in_facet = false;
            continue;
        }
        return Err(IoError::Malformed { at, reason: "unknown_stl_token" });
    }
    Err(IoError::TruncatedFile { at: Position::byte(bytes.len() as u64) })
}

fn parse_three_f64<'a>(parts: &mut impl Iterator<Item = &'a str>, at: Position) -> Result<[f64; 3], IoError> {
    let mut vals = [0.0; 3];
    for slot in &mut vals {
        let Some(tok) = parts.next() else {
            return Err(IoError::Malformed { at, reason: "missing_coordinate" });
        };
        let v: f64 = tok.parse().map_err(|_| IoError::Malformed { at, reason: "invalid_float" })?;
        if !v.is_finite() {
            return Err(IoError::Malformed { at, reason: "non_finite_stl_float" });
        }
        *slot = v;
    }
    if parts.next().is_some() {
        return Err(IoError::Malformed { at, reason: "trailing_tokens_on_stl_line" });
    }
    Ok(vals)
}

/// Hardened first-party PLY reader (`mesh.ply`, ASCII and binary headers) driven through
/// `BoundedSource` and `Budget` (zero `unsafe`, zero panics).
#[derive(Debug, Default, Clone, Copy)]
pub struct PlyFuzzReader;

impl FuzzReader for PlyFuzzReader {
    fn reader_id(&self) -> &str {
        "mesh.ply"
    }

    fn read_bounded(&self, source: &mut BoundedSource<&[u8]>) -> Result<(), IoError> {
        let mut bytes = Vec::new();
        source.read_to_end_bounded(&mut bytes)?;
        let budget = source.budget_mut();
        if bytes.is_empty() {
            return Err(IoError::TruncatedFile { at: Position::byte(0) });
        }
        let mut pos = 0usize;
        let first_line =
            next_line_bytes(&bytes, &mut pos).ok_or_else(|| IoError::TruncatedFile { at: Position::byte(0) })?;
        if first_line.trim_ascii() != b"ply" {
            return Err(IoError::Malformed { at: Position::byte(0), reason: "missing_ply_magic" });
        }

        let mut format_ascii = false;
        let mut format_seen = false;
        let mut elements: Vec<u64> = Vec::new();
        let mut header_done = false;
        let mut line_no = 1u32;

        while let Some(raw_line) = next_line_bytes(&bytes, &mut pos) {
            line_no = line_no.saturating_add(1);
            let at = Position::line_col(pos as u64, line_no, 1);
            budget.check_wall(at)?;
            let line = std::str::from_utf8(raw_line)
                .map_err(|_| IoError::Malformed { at, reason: "non_utf8_ply_header" })?
                .trim();
            if line.is_empty() || line.starts_with("comment") || line.starts_with("obj_info") {
                continue;
            }
            if line == "end_header" {
                header_done = true;
                break;
            }
            let mut parts = line.split_ascii_whitespace();
            let Some(head) = parts.next() else {
                continue;
            };
            match head {
                "format" => {
                    if format_seen {
                        return Err(IoError::Malformed { at, reason: "duplicate_ply_format" });
                    }
                    let Some(kind) = parts.next() else {
                        return Err(IoError::Malformed { at, reason: "missing_ply_format_kind" });
                    };
                    let Some(ver) = parts.next() else {
                        return Err(IoError::Malformed { at, reason: "missing_ply_version" });
                    };
                    if ver != "1.0" {
                        return Err(IoError::Malformed { at, reason: "unsupported_ply_version" });
                    }
                    format_ascii = match kind {
                        "ascii" => true,
                        "binary_little_endian" | "binary_big_endian" => false,
                        _ => return Err(IoError::Malformed { at, reason: "unknown_ply_format" }),
                    };
                    format_seen = true;
                }
                "element" => {
                    let Some(_name) = parts.next() else {
                        return Err(IoError::Malformed { at, reason: "missing_element_name" });
                    };
                    let Some(cnt_s) = parts.next() else {
                        return Err(IoError::Malformed { at, reason: "missing_element_count" });
                    };
                    let cnt: u64 =
                        cnt_s.parse().map_err(|_| IoError::Malformed { at, reason: "invalid_element_count" })?;
                    if cnt > budget.limits().elements() {
                        budget.charge_elements(cnt, at)?;
                    }
                    elements.push(cnt);
                }
                "property" => {
                    let _guard = budget.enter(at)?;
                    if parts.next().is_none() {
                        return Err(IoError::Malformed { at, reason: "empty_property_line" });
                    }
                }
                _ => return Err(IoError::Malformed { at, reason: "unknown_ply_header_keyword" }),
            }
        }
        if !format_seen || !header_done {
            return Err(IoError::TruncatedFile { at: Position::byte(pos as u64) });
        }
        if format_ascii {
            for &cnt in &elements {
                for _ in 0..cnt {
                    line_no = line_no.saturating_add(1);
                    let at = Position::line_col(pos as u64, line_no, 1);
                    budget.charge_elements(1, at)?;
                    budget.check_wall(at)?;
                    let raw = next_line_bytes(&bytes, &mut pos)
                        .ok_or_else(|| IoError::TruncatedFile { at: Position::byte(pos as u64) })?;
                    let line = std::str::from_utf8(raw)
                        .map_err(|_| IoError::Malformed { at, reason: "non_utf8_ply_body" })?
                        .trim();
                    if line.is_empty() {
                        return Err(IoError::Malformed { at, reason: "empty_ply_record" });
                    }
                    for tok in line.split_ascii_whitespace() {
                        let v: f64 =
                            tok.parse().map_err(|_| IoError::Malformed { at, reason: "invalid_ply_number" })?;
                        if !v.is_finite() {
                            return Err(IoError::Malformed { at, reason: "non_finite_ply_number" });
                        }
                    }
                }
            }
        } else {
            for &cnt in &elements {
                budget.charge_elements(cnt, Position::byte(pos as u64))?;
            }
        }
        Ok(())
    }
}

fn next_line_bytes<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    if *pos >= bytes.len() {
        return None;
    }
    let start = *pos;
    let rest = bytes.get(start..)?;
    if let Some(rel) = rest.iter().position(|&b| b == b'\n') {
        *pos = start + rel + 1;
        let line = &rest[..rel];
        Some(line.strip_suffix(b"\r").unwrap_or(line))
    } else {
        *pos = bytes.len();
        Some(rest)
    }
}

/// Built-in first-party fuzz readers bundled in `fid-io-corp`.
pub fn builtin_readers() -> BTreeMap<String, Arc<dyn FuzzReader>> {
    let mut m: BTreeMap<String, Arc<dyn FuzzReader>> = BTreeMap::new();
    let ply: Arc<dyn FuzzReader> = Arc::new(PlyFuzzReader);
    m.insert(ply.reader_id().to_string(), ply);
    let stl: Arc<dyn FuzzReader> = Arc::new(StlFuzzReader);
    m.insert(stl.reader_id().to_string(), stl);
    m
}

// ------------------------------------------------------------------ target naming & generation

/// One generated or checked fuzz target (spec Design: `FuzzTarget { reader_id, seeds_dir }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzTarget {
    pub reader_id: FormatId,
    pub target_name: String,
    pub target_file: PathBuf,
    pub seeds_dir: PathBuf,
}

/// Maps a registry [`FormatId`] (e.g. `mesh.stl`) to its valid Cargo fuzz binary stem (`mesh_stl`).
pub fn target_name(format: &FormatId) -> String {
    format.as_str().chars().map(|c| if c == '.' || c == '-' { '_' } else { c }).collect()
}

/// Renders the deterministic `cargo-fuzz` target source file for `format`.
pub fn render_target_source(format: &FormatId) -> String {
    let id = format.as_str();
    format!(
        concat!(
            "//! Generated cargo-fuzz target for registered reader `{id}` (spec node IO.CORP.FUZZ).\n",
            "//! Drives the reader through `fid-io-reg` with small fuzz limits (16 MiB bytes, depth 64).\n",
            "#![no_main]\n",
            "\n",
            "use libfuzzer_sys::fuzz_target;\n",
            "\n",
            "fuzz_target!(|data: &[u8]| {{\n",
            "    let readers = fid_io_corp::builtin_readers();\n",
            "    if let Some(reader) = readers.get({id:?}) {{\n",
            "        let _ = fid_io_corp::drive_reader(reader.as_ref(), data);\n",
            "    }}\n",
            "}});\n",
        ),
        id = id
    )
}

/// Renders the deterministic standalone `fid-io-fuzz` `Cargo.toml` for `formats`.
pub fn render_fuzz_cargo_toml(formats: &[FormatId]) -> String {
    let mut sorted: Vec<&FormatId> = formats.iter().collect();
    sorted.sort();
    sorted.dedup();

    let mut out = String::from(
        "# Generated cargo-fuzz harness for registered first-party readers (IO.CORP.FUZZ).\n\
         # Not a workspace member: requires nightly + libFuzzer. Run from crates/fid-io-corp:\n\
         #   cargo +nightly fuzz run <target> <seeds_dir> -- -max_total_time=600 -timeout=10 -rss_limit_mb=2048\n\
         [package]\n\
         name = \"fid-io-fuzz\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         publish = false\n\
         license = \"MIT OR Apache-2.0\"\n\
         \n\
         [package.metadata]\n\
         cargo-fuzz = true\n\
         \n\
         [dependencies]\n\
         libfuzzer-sys = \"0.4\"\n\
         fid-io-corp = { path = \"..\" }\n",
    );
    for fmt in sorted {
        let stem = target_name(fmt);
        out.push_str(&format!(
            "\n[[bin]]\n\
             name = \"{stem}\"\n\
             path = \"fuzz_targets/{stem}.rs\"\n\
             test = false\n\
             doc = false\n\
             bench = false\n"
        ));
    }
    out.push_str("\n# Keep this crate out of the parent workspace.\n[workspace]\nmembers = [\".\"]\n");
    out
}

/// Generates `Cargo.toml`, `.gitignore`, and one `fuzz_targets/<stem>.rs` per format in sorted order.
pub fn generate_targets(
    formats: &[FormatId],
    fuzz_dir: &Path,
    seeds_root: &Path,
) -> Result<Vec<FuzzTarget>, CorpError> {
    let mut sorted: Vec<FormatId> = formats.to_vec();
    sorted.sort();
    sorted.dedup();

    let targets_dir = fuzz_dir.join("fuzz_targets");
    fs::create_dir_all(&targets_dir).map_err(|e| CorpError::Io {
        format: String::new(),
        entry: targets_dir.display().to_string(),
        detail: e.to_string(),
    })?;

    write_regular_file(&fuzz_dir.join("Cargo.toml"), render_fuzz_cargo_toml(&sorted).as_bytes())?;
    write_regular_file(&fuzz_dir.join(".gitignore"), b"target\ncorpus\nartifacts\ncoverage\n")?;

    let mut out = Vec::new();
    for fmt in sorted {
        let stem = target_name(&fmt);
        let target_file = targets_dir.join(format!("{stem}.rs"));
        write_regular_file(&target_file, render_target_source(&fmt).as_bytes())?;
        let seeds_dir = seeds_root.join(&stem);
        out.push(FuzzTarget { reader_id: fmt, target_name: stem, target_file, seeds_dir });
    }
    Ok(out)
}

fn write_regular_file(path: &Path, bytes: &[u8]) -> Result<(), CorpError> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(CorpError::Io {
                format: String::new(),
                entry: path.display().to_string(),
                detail: "refusing to overwrite symbolic link".to_string(),
            });
        }
    }
    let mut f = File::create(path).map_err(|e| CorpError::Io {
        format: String::new(),
        entry: path.display().to_string(),
        detail: e.to_string(),
    })?;
    f.write_all(bytes).map_err(|e| CorpError::Io {
        format: String::new(),
        entry: path.display().to_string(),
        detail: e.to_string(),
    })?;
    Ok(())
}

// ------------------------------------------------------------------ coverage check (REQ-001, AC-2)

/// Result of checking fuzz target coverage against a registry listing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverageReport {
    pub checked: usize,
    pub present: Vec<FormatId>,
    pub missing: Vec<CorpError>,
}

impl CoverageReport {
    pub fn is_ok(&self) -> bool {
        self.missing.is_empty()
    }
}

/// Verifies every registered first-party reader has a non-empty regular target file under
/// `targets_dir` (either `<stem>.rs` or `<format_id>.rs`). Fails with
/// [`CorpError::FuzzTargetMissing`] for every uncovered reader (REQ-IO.CORP.FUZZ-001, AC-2).
pub fn check_coverage(formats: &[FormatId], targets_dir: &Path) -> CoverageReport {
    let mut sorted: Vec<FormatId> = formats.to_vec();
    sorted.sort();
    sorted.dedup();

    let mut report = CoverageReport { checked: sorted.len(), ..CoverageReport::default() };
    for fmt in sorted {
        let stem_path = targets_dir.join(format!("{}.rs", target_name(&fmt)));
        let raw_path = targets_dir.join(format!("{}.rs", fmt.as_str()));
        if is_nonempty_regular(&stem_path) || is_nonempty_regular(&raw_path) {
            report.present.push(fmt);
        } else {
            report.missing.push(CorpError::FuzzTargetMissing { format: fmt.as_str().to_string() });
        }
    }
    report
}

fn is_nonempty_regular(path: &Path) -> bool {
    fs::symlink_metadata(path).map(|m| m.file_type().is_file() && m.len() > 0).unwrap_or(false)
}

// ------------------------------------------------------------------ seed staging

/// Summary of staging manifest seeds (accepted and hostile entries) for fuzz targets.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeedReport {
    pub targets: usize,
    pub seeds_staged: usize,
    pub urls_skipped: usize,
    pub errors: Vec<CorpError>,
}

impl SeedReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Copies manifest seeds (both accepted and hostile entries) into `<seeds_root>/<target_name>/`,
/// named by `<sha256_hex>` and bounded by [`MAX_SEED_BYTES`].
pub fn stage_seeds(
    corpus_dir: &Path,
    manifests: &[Manifest],
    cache_dir: Option<&Path>,
    seeds_root: &Path,
) -> SeedReport {
    let mut report = SeedReport::default();
    let mut seen_targets: BTreeSet<String> = BTreeSet::new();

    for manifest in manifests {
        let fmt_str = manifest.format.as_str();
        let stem = target_name(&manifest.format);
        let dir = seeds_root.join(&stem);
        if let Err(e) = fs::create_dir_all(&dir) {
            report.errors.push(CorpError::Io {
                format: fmt_str.to_string(),
                entry: dir.display().to_string(),
                detail: e.to_string(),
            });
            continue;
        }
        seen_targets.insert(stem);
        let mut staged_for_target = 0usize;

        for entry in &manifest.entries {
            if staged_for_target >= MAX_SEEDS_PER_TARGET {
                break;
            }
            let src_path = match &entry.location {
                Location::Path(rel) => corpus_dir.join(rel),
                Location::Url(_) => match cache_dir {
                    Some(c) => {
                        let p = cache_path(c, &entry.sha256);
                        if !p.is_file() {
                            report.urls_skipped += 1;
                            continue;
                        }
                        p
                    }
                    None => {
                        report.urls_skipped += 1;
                        continue;
                    }
                },
            };
            match read_bounded_regular(&src_path, MAX_SEED_BYTES) {
                Ok(bytes) => {
                    let digest = sha256_bytes(&bytes);
                    if digest != entry.sha256 {
                        report.errors.push(CorpError::DigestMismatch {
                            format: fmt_str.to_string(),
                            entry: entry.location.as_str().to_string(),
                            expected: Box::new(entry.sha256),
                            actual: Box::new(digest),
                        });
                        continue;
                    }
                    let dst = dir.join(digest.hex());
                    if let Err(e) = write_regular_file(&dst, &bytes) {
                        report.errors.push(e);
                        continue;
                    }
                    staged_for_target += 1;
                    report.seeds_staged += 1;
                }
                Err(detail) => {
                    report.errors.push(CorpError::Io {
                        format: fmt_str.to_string(),
                        entry: entry.location.as_str().to_string(),
                        detail,
                    });
                }
            }
        }
    }
    report.targets = seen_targets.len();
    report
}

/// Loads all regular seed files from `seeds_dir` in sorted filename order, bounded by
/// [`MAX_SEED_BYTES`] and [`MAX_SEEDS_PER_TARGET`]. Returns an empty `Vec` if `seeds_dir` does
/// not exist.
pub fn load_seeds_dir(seeds_dir: &Path) -> Result<Vec<Vec<u8>>, CorpError> {
    if !seeds_dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    let rd = fs::read_dir(seeds_dir).map_err(|e| CorpError::Io {
        format: String::new(),
        entry: seeds_dir.display().to_string(),
        detail: e.to_string(),
    })?;
    for entry in rd {
        let entry = entry.map_err(|e| CorpError::Io {
            format: String::new(),
            entry: seeds_dir.display().to_string(),
            detail: e.to_string(),
        })?;
        names.push(entry.file_name());
    }
    names.sort();

    let mut seeds = Vec::new();
    for name in names.into_iter().take(MAX_SEEDS_PER_TARGET) {
        let path = seeds_dir.join(&name);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.file_type().is_file() {
            continue;
        }
        let bytes = read_bounded_regular(&path, MAX_SEED_BYTES).map_err(|detail| CorpError::Io {
            format: String::new(),
            entry: path.display().to_string(),
            detail,
        })?;
        seeds.push(bytes);
    }
    Ok(seeds)
}

fn read_bounded_regular(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() {
        return Err("not a regular file".to_string());
    }
    if meta.len() > limit {
        return Err(format!("file exceeds {limit} byte limit"));
    }
    let f = File::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    f.take(limit.saturating_add(1)).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > limit {
        return Err(format!("file exceeds {limit} byte limit"));
    }
    Ok(buf)
}

fn sha256_bytes(bytes: &[u8]) -> Digest {
    let mut h = Sha256::new();
    h.update(bytes);
    Digest::from_bytes(h.finalize().into())
}

// ------------------------------------------------------------------ fuzz runner & crash minimiser

/// Failure class of a fuzz finding (REQ-IO.CORP.FUZZ-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FuzzFindingKind {
    Panic,
    Timeout,
    Abort,
    MemoryCap,
}

impl FuzzFindingKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            FuzzFindingKind::Panic => "panic",
            FuzzFindingKind::Timeout => "timeout",
            FuzzFindingKind::Abort => "abort",
            FuzzFindingKind::MemoryCap => "memory_cap",
        }
    }
}

impl fmt::Display for FuzzFindingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One reproducible crash or timeout finding saved for CI artefact upload (REQ-IO.CORP.FUZZ-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzFinding {
    pub reader_id: String,
    pub kind: FuzzFindingKind,
    pub detail: String,
    pub original_len: usize,
    pub minimised_len: usize,
    pub minimised_sha256: Digest,
    pub artifact_path: PathBuf,
}

impl FuzzFinding {
    /// Serialises the finding metadata to canonical JSON (`finding.json`).
    pub fn to_json(&self) -> Result<Vec<u8>, CorpError> {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str("fid-io-fuzz-finding/0".to_string()));
        m.insert("reader_id".to_string(), CValue::Str(self.reader_id.clone()));
        m.insert("kind".to_string(), CValue::Str(self.kind.as_str().to_string()));
        m.insert("detail".to_string(), CValue::Str(self.detail.clone()));
        m.insert("original_len".to_string(), CValue::U64(self.original_len as u64));
        m.insert("minimised_len".to_string(), CValue::U64(self.minimised_len as u64));
        m.insert("minimised_sha256".to_string(), CValue::Str(self.minimised_sha256.as_str().to_string()));
        m.insert("artifact_file".to_string(), CValue::Str(self.artifact_path.to_string_lossy().into_owned()));
        fid_canon::to_canonical_bytes(&CValue::Obj(m))
            .map_err(|e| CorpError::malformed("finding.json", format!("canonical JSON: {e}")))
    }
}

/// Configuration for an in-process fuzz run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzConfig {
    /// Total wall-time budget per target (default: [`FUZZ_MERGE_BUDGET_SECS`] = 600 s).
    pub budget: Duration,
    /// Per-input timeout (default: [`FUZZ_PER_INPUT_TIMEOUT_SECS`] = 10 s).
    pub per_input_timeout: Duration,
    /// Optional execution cap (useful for deterministic regression tests).
    pub max_iterations: Option<u64>,
    /// Deterministic Philox mutation seed.
    pub seed: u64,
    /// Minimise crashing inputs before saving artefacts (REQ-IO.CORP.FUZZ-003).
    pub minimize: bool,
}

impl Default for FuzzConfig {
    fn default() -> Self {
        FuzzConfig {
            budget: Duration::from_secs(FUZZ_MERGE_BUDGET_SECS),
            per_input_timeout: Duration::from_secs(FUZZ_PER_INPUT_TIMEOUT_SECS),
            max_iterations: None,
            seed: 0x6675_7a7a_0000_0001,
            minimize: true,
        }
    }
}

/// Outcome of fuzzing one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetOutcome {
    pub reader_id: String,
    pub executions: u64,
    pub finding: Option<FuzzFinding>,
}

/// Aggregated result of running the fuzz suite over a registry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FuzzRunReport {
    pub coverage_errors: Vec<CorpError>,
    pub targets: Vec<TargetOutcome>,
}

impl FuzzRunReport {
    pub fn is_ok(&self) -> bool {
        self.coverage_errors.is_empty() && self.targets.iter().all(|t| t.finding.is_none())
    }

    pub fn findings(&self) -> Vec<&FuzzFinding> {
        self.targets.iter().filter_map(|t| t.finding.as_ref()).collect()
    }
}

/// Minimises a panicking input using greedy delta-debugging (chunk removal followed by single-byte
/// deletion) until no strictly smaller input reproduces the panic (REQ-IO.CORP.FUZZ-003).
pub fn minimize_panic_input(reader: &dyn FuzzReader, failing: &[u8], max_steps: usize) -> Vec<u8> {
    let mut best = failing.to_vec();
    if best.is_empty() {
        return best;
    }
    let mut steps = 0usize;
    let panics = |buf: &[u8]| -> bool { catch_unwind(AssertUnwindSafe(|| drive_reader(reader, buf))).is_err() };

    if !panics(&best) {
        return best;
    }
    // Check empty input first.
    if panics(&[]) {
        return Vec::new();
    }

    // Coarse chunk deletion (halves, quarters, ...).
    let mut chunk = best.len() / 2;
    while chunk >= 1 && steps < max_steps {
        let mut improved = false;
        let mut pos = 0usize;
        while pos < best.len() && steps < max_steps {
            let end = pos.saturating_add(chunk).min(best.len());
            let mut cand = Vec::with_capacity(best.len() - (end - pos));
            cand.extend_from_slice(&best[..pos]);
            cand.extend_from_slice(&best[end..]);
            steps += 1;
            if panics(&cand) {
                best = cand;
                improved = true;
                if best.len() <= 1 {
                    return best;
                }
            } else {
                pos = end;
            }
        }
        if !improved {
            chunk /= 2;
        } else if chunk > best.len() / 2 {
            chunk = best.len() / 2;
        }
    }
    best
}

enum ExecResult {
    Finished,
    Panicked(String),
    TimedOut,
}

fn run_one_input(reader: &Arc<dyn FuzzReader>, data: Vec<u8>, timeout: Duration) -> ExecResult {
    let (tx, rx) = mpsc::sync_channel::<Result<(), String>>(1);
    let r = Arc::clone(reader);
    let _handle = thread::spawn(move || {
        let res = catch_unwind(AssertUnwindSafe(|| {
            let _ = drive_reader(r.as_ref(), &data);
        }));
        let msg = match res {
            Ok(()) => Ok(()),
            Err(payload) => Err(panic_message(payload)),
        };
        let _ = tx.send(msg);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(())) => ExecResult::Finished,
        Ok(Err(msg)) => ExecResult::Panicked(msg),
        Err(mpsc::RecvTimeoutError::Timeout) => ExecResult::TimedOut,
        Err(mpsc::RecvTimeoutError::Disconnected) => ExecResult::Panicked("worker thread aborted".to_string()),
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".to_string()
    }
}

fn mutate_input(base: &[u8], rng: &mut Cursor) -> Vec<u8> {
    let mut buf = if base.len() > MAX_MUTATED_BYTES { base[..MAX_MUTATED_BYTES].to_vec() } else { base.to_vec() };
    let ops = 1 + (rng.below(4).unwrap_or(0) as usize);
    for _ in 0..ops {
        let choice = rng.below(6).unwrap_or(0);
        match choice {
            0 if !buf.is_empty() => {
                // Single bit flip
                let idx = rng.below(buf.len() as u64).unwrap_or(0) as usize;
                let bit = 1u8 << (rng.below(8).unwrap_or(0) as u8);
                if let Some(slot) = buf.get_mut(idx) {
                    *slot ^= bit;
                }
            }
            1 if !buf.is_empty() => {
                // Replace byte with an interesting edge byte (0x00, 0xFF, ...)
                let idx = rng.below(buf.len() as u64).unwrap_or(0) as usize;
                let b = INTERESTING_BYTES[rng.below(INTERESTING_BYTES.len() as u64).unwrap_or(0) as usize];
                if let Some(slot) = buf.get_mut(idx) {
                    *slot = b;
                }
            }
            2 if buf.len() < MAX_MUTATED_BYTES => {
                // Insert a random or interesting byte
                let pos = rng.below((buf.len() + 1) as u64).unwrap_or(0) as usize;
                let b = if rng.below(2).unwrap_or(0) == 0 {
                    INTERESTING_BYTES[rng.below(INTERESTING_BYTES.len() as u64).unwrap_or(0) as usize]
                } else {
                    rng.below(256).unwrap_or(0) as u8
                };
                buf.insert(pos, b);
            }
            3 if !buf.is_empty() => {
                // Delete a byte
                let pos = rng.below(buf.len() as u64).unwrap_or(0) as usize;
                buf.remove(pos);
            }
            4 if !buf.is_empty() => {
                // Truncate
                let new_len = rng.below(buf.len() as u64).unwrap_or(0) as usize;
                buf.truncate(new_len);
            }
            _ => {
                // Overwrite or push random byte
                let b = rng.below(256).unwrap_or(0) as u8;
                if buf.is_empty() || buf.len() < MAX_MUTATED_BYTES {
                    buf.push(b);
                } else {
                    let idx = rng.below(buf.len() as u64).unwrap_or(0) as usize;
                    if let Some(slot) = buf.get_mut(idx) {
                        *slot = b;
                    }
                }
            }
        }
    }
    buf
}

fn save_finding(
    reader_id: &str,
    kind: FuzzFindingKind,
    detail: String,
    original_len: usize,
    minimised: &[u8],
    artifacts_dir: &Path,
) -> Result<FuzzFinding, CorpError> {
    let stem: String = reader_id.chars().map(|c| if c == '.' || c == '-' { '_' } else { c }).collect();
    let dir = artifacts_dir.join(&stem);
    fs::create_dir_all(&dir).map_err(|e| CorpError::Io {
        format: reader_id.to_string(),
        entry: dir.display().to_string(),
        detail: e.to_string(),
    })?;
    let digest = sha256_bytes(minimised);
    let prefix = match kind {
        FuzzFindingKind::Timeout => "timeout",
        FuzzFindingKind::Panic | FuzzFindingKind::Abort | FuzzFindingKind::MemoryCap => "crash",
    };
    let artifact_path = dir.join(format!("{prefix}-{}", digest.hex()));
    write_regular_file(&artifact_path, minimised)?;
    let finding = FuzzFinding {
        reader_id: reader_id.to_string(),
        kind,
        detail,
        original_len,
        minimised_len: minimised.len(),
        minimised_sha256: digest,
        artifact_path,
    };
    let json = finding.to_json()?;
    write_regular_file(&dir.join("finding.json"), &json)?;
    Ok(finding)
}

/// Fuzzes one `reader` starting from `seeds` (plus edge inputs `[]` and single interesting bytes),
/// enforcing `config.per_input_timeout` on every input and `config.budget` overall. On panic or
/// timeout, minimises the input (for panics) and writes the reproducible artefact under
/// `artifacts_dir`.
pub fn run_fuzz_target(
    reader: Arc<dyn FuzzReader>,
    seeds: &[Vec<u8>],
    artifacts_dir: &Path,
    config: &FuzzConfig,
) -> Result<TargetOutcome, CorpError> {
    let reader_id = reader.reader_id().to_string();
    let start = Instant::now();
    let mut executions = 0u64;

    // Initial deterministic queue: empty input, manifest seeds, then single edge bytes.
    let mut initial: Vec<Vec<u8>> = Vec::with_capacity(seeds.len() + INTERESTING_BYTES.len() + 1);
    initial.push(Vec::new());
    for s in seeds {
        initial.push(s.clone());
    }
    for &b in &INTERESTING_BYTES {
        initial.push(vec![b]);
    }

    let mut corpus: Vec<Vec<u8>> = if seeds.is_empty() { vec![Vec::new()] } else { seeds.to_vec() };
    let mut stream_id = 0u64;
    for b in reader_id.as_bytes() {
        stream_id = stream_id.wrapping_mul(131).wrapping_add(u64::from(*b));
    }
    let mut rng = Cursor::new(Address { seed: config.seed, stream: stream_id }, 0);

    let mut initial_iter = initial.into_iter();
    loop {
        if let Some(cap) = config.max_iterations {
            if executions >= cap {
                break;
            }
        }
        if start.elapsed() >= config.budget {
            break;
        }
        let input = match initial_iter.next() {
            Some(v) => v,
            None => {
                let idx = rng.below(corpus.len() as u64).unwrap_or(0) as usize;
                let base = corpus.get(idx).map(Vec::as_slice).unwrap_or(&[]);
                mutate_input(base, &mut rng)
            }
        };
        executions = executions.saturating_add(1);

        let orig_len = input.len();
        match run_one_input(&reader, input.clone(), config.per_input_timeout) {
            ExecResult::Finished => {
                if corpus.len() < 128 && executions % 16 == 0 && !input.is_empty() {
                    corpus.push(input);
                }
            }
            ExecResult::Panicked(msg) => {
                let minimised =
                    if config.minimize { minimize_panic_input(reader.as_ref(), &input, 4_096) } else { input };
                let finding =
                    save_finding(&reader_id, FuzzFindingKind::Panic, msg, orig_len, &minimised, artifacts_dir)?;
                return Ok(TargetOutcome { reader_id, executions, finding: Some(finding) });
            }
            ExecResult::TimedOut => {
                let msg = format!("input exceeded per-input timeout of {} ms", config.per_input_timeout.as_millis());
                let finding = save_finding(&reader_id, FuzzFindingKind::Timeout, msg, orig_len, &input, artifacts_dir)?;
                return Ok(TargetOutcome { reader_id, executions, finding: Some(finding) });
            }
        }
    }

    Ok(TargetOutcome { reader_id, executions, finding: None })
}

/// Checks target coverage for every registered format in `formats`, then fuzzes every covered
/// reader whose implementation is registered in `readers`.
pub fn run_fuzz_suite(
    formats: &[FormatId],
    readers: &BTreeMap<String, Arc<dyn FuzzReader>>,
    seeds_root: &Path,
    artifacts_dir: &Path,
    targets_dir: &Path,
    config: &FuzzConfig,
) -> Result<FuzzRunReport, CorpError> {
    let cov = check_coverage(formats, targets_dir);
    let mut report = FuzzRunReport { coverage_errors: cov.missing, targets: Vec::new() };
    if !report.coverage_errors.is_empty() {
        return Ok(report);
    }

    for fmt in cov.present {
        let Some(reader) = readers.get(fmt.as_str()) else {
            report.coverage_errors.push(CorpError::FuzzTargetMissing { format: fmt.as_str().to_string() });
            continue;
        };
        let seeds_dir = seeds_root.join(target_name(&fmt));
        let seeds = load_seeds_dir(&seeds_dir)?;
        let outcome = run_fuzz_target(Arc::clone(reader), &seeds, artifacts_dir, config)?;
        report.targets.push(outcome);
    }
    Ok(report)
}
