//! Surface generator framework. Spec node `AUTO.SURF.GEN`.
//!
//! The build-time driver behind `cargo xtask gen-surfaces --manifest <path> --out <dir>`. It
//! projects the registry manifest once ([`fid_surface::project`]), runs every registered
//! [`SurfaceEmitter`] over that projection in a fixed order, and writes what they return.
//! Emitters (CLI tree, MCP tools, Python package, docs pages) belong to their surface nodes;
//! this crate owns only the trait, the "do not edit" header, the per-surface
//! [`INDEX_FILE`] and the byte-level determinism of everything written.
//!
//! Generation runs in two phases. [`render`] is pure: it validates every path, stamps the
//! header, normalises line endings and builds the index entirely in memory, so any error
//! (`emit_failed`, `path_outside_root`, `duplicate_output`) leaves the file system untouched.
//! [`write_outputs`] then stages each surface in a sibling directory and swaps it in.
//!
//! Pinned policies:
//! - **Ownership.** The generator owns exactly `<out>/<surface>/` for each surface of the run,
//!   plus the sibling work directories `<out>/.<surface>.gen-staging` and
//!   `<out>/.<surface>.gen-old`. Nothing else under `<out>` is read, written or removed.
//! - **Stale files.** An owned surface directory is replaced as a whole, so a file the run no
//!   longer produces disappears from it; files outside owned directories are never removed.
//! - **Atomic replacement.** A surface directory is always either the complete previous tree or
//!   the complete new one, never a partial one: the new tree is written and synced in the
//!   staging directory, the old directory is renamed aside, the staging directory is renamed
//!   into place, and only then is the old one deleted. A run interrupted between the two
//!   renames is repaired at the start of the next run (the old tree is restored first).
//! - **Paths.** Emitted paths are relative, `/`-separated, and limited to the portable
//!   characters `A-Z a-z 0-9 . _ -` per segment, so the same tree is written on Windows, Linux
//!   and macOS; duplicates are detected ASCII case-insensitively for the same reason.
//! - **Text.** Every file is UTF-8 without BOM, with LF line endings and a final LF; the header
//!   carries no timestamp, path, host or user name.

use fid_canon::{to_canonical_bytes, CValue};
use fid_surface::projection::MAX_MANIFEST_BYTES;
use fid_surface::{project, ProjError, Projection};
use fid_types::CommandId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Name of the generator, as written in every header.
pub const GENERATOR: &str = "fid-surface-gen";
/// Version of the generator's output layout (header, index, normalisation). Bumped whenever
/// the bytes this crate adds or changes would differ; it is not the crate version, which
/// changes for unrelated reasons.
pub const GENERATOR_VERSION: &str = "1";
/// The words every header starts with (REQ-002).
pub const HEADER_TEXT: &str = "generated, do not edit";
/// Per-surface index file name (REQ-003), written by the generator, never by an emitter.
pub const INDEX_FILE: &str = "surface-index.json";
/// Longest accepted surface name.
pub const MAX_SURFACE_NAME: usize = 32;
/// Longest accepted relative path, in bytes (keeps Windows below its path limits).
pub const MAX_PATH_BYTES: usize = 200;

const STAGING_SUFFIX: &str = ".gen-staging";
const OLD_SUFFIX: &str = ".gen-old";

/// Usage line of `cargo xtask gen-surfaces`.
pub const USAGE: &str = "usage: cargo xtask gen-surfaces --manifest <registry manifest json> --out <dir> \
                         [--test-emitter echo]...";

/// How a header is written in a given file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentSyntax {
    /// A line comment: each header line is `<prefix> <text>`.
    Line(&'static str),
    /// A block comment: each header line is `<open> <text> <close>`.
    Block(&'static str, &'static str),
}

impl CommentSyntax {
    /// `#` (Python, TOML, shell).
    pub const HASH: CommentSyntax = CommentSyntax::Line("#");
    /// `//` (Rust, TypeScript).
    pub const SLASHES: CommentSyntax = CommentSyntax::Line("//");
    /// `<!-- -->` (Markdown, HTML).
    pub const HTML: CommentSyntax = CommentSyntax::Block("<!--", "-->");

    /// Comment tokens must be non-empty printable ASCII without spaces, so a header line is
    /// always exactly one line.
    fn check(self) -> Result<(), String> {
        let ok = |t: &str| !t.is_empty() && t.bytes().all(|b| (0x21..=0x7e).contains(&b));
        let good = match self {
            CommentSyntax::Line(p) => ok(p),
            CommentSyntax::Block(o, c) => ok(o) && ok(c),
        };
        if good {
            Ok(())
        } else {
            Err(format!("comment syntax {self:?} is not non-empty printable ASCII"))
        }
    }

    fn line(self, text: &str) -> String {
        match self {
            CommentSyntax::Line(p) => format!("{p} {text}\n"),
            CommentSyntax::Block(o, c) => format!("{o} {text} {c}\n"),
        }
    }
}

/// One file an emitter wants written, relative to its surface directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    /// Relative, `/`-separated path inside the surface directory.
    pub rel_path: String,
    /// File content without the header; UTF-8, any line endings.
    pub bytes: Vec<u8>,
    /// The commands this file describes; they make up the surface index.
    pub covers: Vec<CommandId>,
}

/// An emitter could not produce its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitError {
    pub detail: String,
}

/// A surface emitter (implemented by `AUTO.CLI.TREE`, `AUTO.MCP.PROJ`, `AUTO.PY.GEN`,
/// `AUTO.SURF.DOCS`). Emitters must be pure functions of the projection.
pub trait SurfaceEmitter {
    /// The surface name, which is also its directory under `--out`: `[a-z][a-z0-9_-]*`.
    fn surface(&self) -> &'static str;
    /// The files of this surface, without headers.
    fn emit(&self, p: &Projection) -> Result<Vec<GeneratedFile>, EmitError>;
    /// The comment syntax of the file at `path` (the emitted relative path).
    fn comment_syntax(&self, path: &str) -> CommentSyntax;
}

/// Why generation failed. Codes are stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenError {
    /// The manifest could not be projected; the code is the projection's.
    Manifest(ProjError),
    /// An emitter failed, returned non-UTF-8 text, a bad comment syntax, or covered a command
    /// absent from the projection.
    EmitFailed { surface: String, detail: String },
    /// A path (or surface name) that is not a plain portable relative path inside the root.
    PathOutsideRoot { surface: String, path: String, detail: &'static str },
    /// Two outputs at the same path (ASCII case-insensitive), a path used as both file and
    /// directory, an emitter writing the index itself, or two emitters of one surface.
    DuplicateOutput { surface: String, path: String },
    /// Reading the manifest or writing the output failed.
    Io { path: String, detail: String },
}

impl GenError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            GenError::Manifest(e) => e.code(),
            GenError::EmitFailed { .. } => "emit_failed",
            GenError::PathOutsideRoot { .. } => "path_outside_root",
            GenError::DuplicateOutput { .. } => "duplicate_output",
            GenError::Io { .. } => "io_error",
        }
    }
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            GenError::Manifest(e) => write!(f, "{e}"),
            GenError::EmitFailed { surface, detail } => write!(f, "{code}: surface {surface}: {detail}"),
            GenError::PathOutsideRoot { surface, path, detail } => {
                write!(f, "{code}: surface {surface}: {path:?}: {detail}")
            }
            GenError::DuplicateOutput { surface, path } => write!(f, "{code}: surface {surface}: {path:?}"),
            GenError::Io { path, detail } => write!(f, "{code}: {path}: {detail}"),
        }
    }
}

impl std::error::Error for GenError {}

/// One surface, fully rendered: relative path to final bytes (index included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceOutput {
    pub surface: String,
    pub files: BTreeMap<String, Vec<u8>>,
    /// Covered commands, in byte order of the command ID, without repeats.
    pub commands: Vec<CommandId>,
}

/// The three header lines (REQ-002): the do-not-edit words, the manifest digest and the
/// generator version.
pub fn header_lines(p: &Projection) -> [String; 3] {
    [
        HEADER_TEXT.to_string(),
        format!("manifest {}", p.manifest_digest.as_str()),
        format!("generator {GENERATOR} {GENERATOR_VERSION}"),
    ]
}

/// The header block for one file in `syntax`.
pub fn header(p: &Projection, syntax: CommentSyntax) -> String {
    header_lines(p).iter().map(|l| syntax.line(l)).collect()
}

/// Checks a relative output path (REQ-006). Returns the reason it is refused.
pub fn check_rel_path(p: &str) -> Result<(), &'static str> {
    if p.is_empty() {
        return Err("empty path");
    }
    if p.len() > MAX_PATH_BYTES {
        return Err("path longer than 200 bytes");
    }
    if p.contains('\\') {
        return Err("backslash separator");
    }
    if p.starts_with('/') {
        return Err("absolute path");
    }
    if p.contains(':') {
        return Err("drive letter or stream separator");
    }
    for seg in p.split('/') {
        if seg.is_empty() {
            return Err("empty segment");
        }
        if seg == "." || seg == ".." {
            return Err("dot segment");
        }
        if !seg.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err("non-portable character");
        }
        if seg.ends_with('.') {
            return Err("trailing dot");
        }
        if is_device_name(seg) {
            return Err("reserved device name");
        }
    }
    Ok(())
}

/// Windows device names, which open a device instead of a file whatever the extension.
fn is_device_name(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or(seg).to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        s if s.len() == 4 && (s.starts_with("COM") || s.starts_with("LPT")) => {
            s.as_bytes().get(3).is_some_and(u8::is_ascii_digit)
        }
        _ => false,
    }
}

fn check_surface_name(s: &str) -> Result<(), &'static str> {
    let b = s.as_bytes();
    let first_ok = b.first().is_some_and(u8::is_ascii_lowercase);
    let rest_ok = b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-'));
    if !first_ok || !rest_ok || b.len() > MAX_SURFACE_NAME {
        return Err("surface name must match [a-z][a-z0-9_-]{0,31}");
    }
    if is_device_name(s) {
        return Err("reserved device name");
    }
    Ok(())
}

/// UTF-8 without BOM, LF endings (CRLF and lone CR become LF), and a final LF unless empty
/// (REQ-004).
pub fn normalise(bytes: &[u8]) -> Result<String, String> {
    let s = std::str::from_utf8(bytes).map_err(|e| format!("not UTF-8 at byte {}", e.valid_up_to()))?;
    let s = s.strip_prefix('\u{feff}').unwrap_or(s);
    let mut out = String::with_capacity(s.len() + 1);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn str_val(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

/// The RFC 8785 bytes of `surface-index.json` (REQ-003). JSON has no comments, so the
/// header travels as the member `_header`, which sorts first; the file therefore starts with
/// `{"_header":"generated, do not edit; manifest sha256:...`. No trailing newline.
pub fn index_bytes(p: &Projection, surface: &str, commands: &[CommandId]) -> Result<Vec<u8>, String> {
    let mut list = BTreeMap::new();
    for c in commands {
        list.insert(c.as_str().to_string(), c.major());
    }
    let entries = list
        .into_iter()
        .map(|(id, major)| {
            let mut o = BTreeMap::new();
            o.insert("command_id".to_string(), CValue::Str(id));
            o.insert("major".to_string(), CValue::Num(f64::from(major)));
            CValue::Obj(o)
        })
        .collect();
    let mut o = BTreeMap::new();
    o.insert("_header".to_string(), CValue::Str(header_lines(p).join("; ")));
    o.insert("commands".to_string(), CValue::Arr(entries));
    o.insert("generator_version".to_string(), str_val(GENERATOR_VERSION));
    o.insert("manifest_digest".to_string(), str_val(p.manifest_digest.as_str()));
    o.insert("surface".to_string(), str_val(surface));
    to_canonical_bytes(&CValue::Obj(o)).map_err(|e| format!("index: {}", e.code()))
}

/// Runs the emitters in the given order and renders every surface in memory (REQ-001 to
/// REQ-004, REQ-006). Touches no file.
pub fn render(p: &Projection, emitters: &[&dyn SurfaceEmitter]) -> Result<Vec<SurfaceOutput>, GenError> {
    let known: BTreeSet<&str> = p.commands.iter().map(|c| c.command_id.as_str()).collect();
    let mut surfaces = BTreeSet::new();
    let mut outputs = Vec::new();
    for e in emitters {
        let surface = e.surface();
        let sname = || surface.to_string();
        check_surface_name(surface).map_err(|detail| GenError::PathOutsideRoot {
            surface: sname(),
            path: sname(),
            detail,
        })?;
        if !surfaces.insert(surface) {
            return Err(GenError::DuplicateOutput { surface: sname(), path: format!("{surface}/") });
        }
        let emitted = e.emit(p).map_err(|er| GenError::EmitFailed { surface: sname(), detail: er.detail })?;
        // ASCII-folded file keys and every directory prefix, to catch case-only duplicates and
        // a path used as both a file and a directory on any file system.
        let mut keys = BTreeSet::new();
        keys.insert(INDEX_FILE.to_string());
        let mut dirs: BTreeMap<String, String> = BTreeMap::new();
        let mut files = BTreeMap::new();
        let mut covered = BTreeSet::new();
        for f in emitted {
            check_rel_path(&f.rel_path).map_err(|detail| GenError::PathOutsideRoot {
                surface: sname(),
                path: f.rel_path.clone(),
                detail,
            })?;
            let folded = f.rel_path.to_ascii_lowercase();
            if !keys.insert(folded.clone()) {
                return Err(GenError::DuplicateOutput { surface: sname(), path: f.rel_path });
            }
            for (i, _) in folded.match_indices('/') {
                dirs.entry(folded[..i].to_string()).or_insert_with(|| f.rel_path.clone());
            }
            let syntax = e.comment_syntax(&f.rel_path);
            syntax.check().map_err(|detail| GenError::EmitFailed { surface: sname(), detail })?;
            let body = normalise(&f.bytes)
                .map_err(|d| GenError::EmitFailed { surface: sname(), detail: format!("{}: {d}", f.rel_path) })?;
            for c in f.covers {
                if !known.contains(c.as_str()) {
                    return Err(GenError::EmitFailed {
                        surface: sname(),
                        detail: format!("{} covers {}, which is not in the manifest", f.rel_path, c.as_str()),
                    });
                }
                covered.insert(c);
            }
            let mut text = header(p, syntax);
            text.push_str(&body);
            files.insert(f.rel_path, text.into_bytes());
        }
        if let Some((_, path)) = dirs.iter().find(|(d, _)| keys.contains(*d)) {
            return Err(GenError::DuplicateOutput { surface: sname(), path: path.clone() });
        }
        let commands: Vec<CommandId> = covered.into_iter().collect();
        let index =
            index_bytes(p, surface, &commands).map_err(|detail| GenError::EmitFailed { surface: sname(), detail })?;
        files.insert(INDEX_FILE.to_string(), index);
        outputs.push(SurfaceOutput { surface: sname(), files, commands });
    }
    Ok(outputs)
}

fn io_err(path: &Path) -> impl Fn(std::io::Error) -> GenError + '_ {
    move |e| GenError::Io { path: path.display().to_string(), detail: e.to_string() }
}

fn exists(path: &Path) -> Result<bool, GenError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_err(path)(e)),
    }
}

fn work_dirs(out: &Path, surface: &str) -> (PathBuf, PathBuf, PathBuf) {
    (out.join(surface), out.join(format!(".{surface}{STAGING_SUFFIX}")), out.join(format!(".{surface}{OLD_SUFFIX}")))
}

/// Repairs what an interrupted earlier run left: drops a staging directory, and restores the
/// old tree if the swap stopped between its two renames.
fn recover(out: &Path, surface: &str) -> Result<(), GenError> {
    let (target, staging, old) = work_dirs(out, surface);
    if exists(&staging)? {
        fs::remove_dir_all(&staging).map_err(io_err(&staging))?;
    }
    if exists(&old)? {
        if exists(&target)? {
            fs::remove_dir_all(&old).map_err(io_err(&old))?;
        } else {
            fs::rename(&old, &target).map_err(io_err(&old))?;
        }
    }
    Ok(())
}

fn stage(staging: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<(), GenError> {
    fs::create_dir(staging).map_err(io_err(staging))?;
    for (rel, bytes) in files {
        let mut path = staging.to_path_buf();
        for seg in rel.split('/') {
            path.push(seg);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let mut f = fs::File::create_new(&path).map_err(io_err(&path))?;
        f.write_all(bytes).map_err(io_err(&path))?;
        f.sync_all().map_err(io_err(&path))?;
    }
    Ok(())
}

fn swap(out: &Path, surface: &str) -> Result<(), GenError> {
    let (target, staging, old) = work_dirs(out, surface);
    match fs::symlink_metadata(&target) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {
            fs::rename(&target, &old).map_err(io_err(&target))?;
            if let Err(e) = fs::rename(&staging, &target) {
                // Put the previous tree back; if even that fails, the next run's recover() does.
                let _ = fs::rename(&old, &target);
                return Err(io_err(&staging)(e));
            }
            fs::remove_dir_all(&old).map_err(io_err(&old))
        }
        Ok(_) => Err(GenError::Io {
            path: target.display().to_string(),
            detail: "exists and is not a plain directory; refusing to replace it".to_string(),
        }),
        Err(e) if e.kind() == ErrorKind::NotFound => fs::rename(&staging, &target).map_err(io_err(&staging)),
        Err(e) => Err(io_err(&target)(e)),
    }
}

fn drop_staging(out: &Path, outputs: &[SurfaceOutput]) {
    for o in outputs {
        let (_, staging, _) = work_dirs(out, &o.surface);
        // Best effort: a leftover staging directory is removed by the next run's recover().
        let _ = fs::remove_dir_all(staging);
    }
}

/// Writes rendered surfaces under `out` (REQ-001, REQ-005): stages every surface first, then
/// swaps each owned directory in. Files outside the owned directories are left alone.
pub fn write_outputs(out: &Path, outputs: &[SurfaceOutput]) -> Result<(), GenError> {
    fs::create_dir_all(out).map_err(io_err(out))?;
    for o in outputs {
        recover(out, &o.surface)?;
    }
    for o in outputs {
        let (_, staging, _) = work_dirs(out, &o.surface);
        if let Err(e) = stage(&staging, &o.files) {
            drop_staging(out, outputs);
            return Err(e);
        }
    }
    for o in outputs {
        if let Err(e) = swap(out, &o.surface) {
            drop_staging(out, outputs);
            return Err(e);
        }
    }
    Ok(())
}

/// Projects `manifest`, renders every emitter and writes the result under `out`. On any
/// validation error nothing is written (REQ-006).
pub fn generate(manifest: &[u8], emitters: &[&dyn SurfaceEmitter], out: &Path) -> Result<Vec<SurfaceOutput>, GenError> {
    let p = project(manifest).map_err(GenError::Manifest)?;
    let outputs = render(&p, emitters)?;
    write_outputs(out, &outputs)?;
    Ok(outputs)
}

/// The emitter list, in its fixed run order. Each surface node adds its emitter here when it
/// is built; none is built yet.
pub fn registered() -> Vec<Box<dyn SurfaceEmitter>> {
    Vec::new()
}

/// Test emitters selectable with `--test-emitter`, appended after the registered ones.
pub fn test_emitter(name: &str) -> Option<Box<dyn SurfaceEmitter>> {
    match name {
        "echo" => Some(Box::new(EchoEmitter)),
        _ => None,
    }
}

/// The test emitter of the acceptance criteria: surface `echo`, one file per command,
/// `commands/<name>.v<major>.txt`, holding the command ID and its title.
#[derive(Debug, Clone, Copy, Default)]
pub struct EchoEmitter;

impl SurfaceEmitter for EchoEmitter {
    fn surface(&self) -> &'static str {
        "echo"
    }

    fn emit(&self, p: &Projection) -> Result<Vec<GeneratedFile>, EmitError> {
        Ok(p.commands
            .iter()
            .map(|c| GeneratedFile {
                rel_path: format!("commands/{}.v{}.txt", c.command_id.name(), c.major),
                bytes: format!("{}\n{}\n", c.command_id.as_str(), c.help.title).into_bytes(),
                covers: vec![c.command_id.clone()],
            })
            .collect())
    }

    fn comment_syntax(&self, _path: &str) -> CommentSyntax {
        CommentSyntax::HASH
    }
}

fn read_manifest(path: &Path) -> Result<Vec<u8>, GenError> {
    let f = fs::File::open(path).map_err(io_err(path))?;
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_MANIFEST_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    f.take(limit).read_to_end(&mut bytes).map_err(io_err(path))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(GenError::Io {
            path: path.display().to_string(),
            detail: format!("manifest larger than {MAX_MANIFEST_BYTES} bytes"),
        });
    }
    Ok(bytes)
}

/// `cargo xtask gen-surfaces`. Exit 0: written; 1: generation error (nothing changed unless
/// the error is `io_error` during the swap); 2: usage.
pub fn cli(args: &[String]) -> ExitCode {
    let (mut manifest, mut out) = (None, None);
    let mut emitters = registered();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--manifest", Some(v)) => manifest = Some(PathBuf::from(v)),
            ("--out", Some(v)) => out = Some(PathBuf::from(v)),
            ("--test-emitter", Some(v)) => match test_emitter(v) {
                Some(e) => emitters.push(e),
                None => {
                    eprintln!("gen-surfaces: unknown test emitter {v:?}\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(manifest), Some(out)) = (manifest, out) else {
        eprintln!("gen-surfaces: --manifest and --out are required\n{USAGE}");
        return ExitCode::from(2);
    };
    let refs: Vec<&dyn SurfaceEmitter> = emitters.iter().map(|e| e.as_ref()).collect();
    let result = read_manifest(&manifest).and_then(|bytes| generate(&bytes, &refs, &out));
    match result {
        Ok(outputs) => {
            for o in &outputs {
                println!("gen-surfaces: {}: {} files, {} commands", o.surface, o.files.len(), o.commands.len());
            }
            println!("gen-surfaces: {} surfaces written", outputs.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gen-surfaces: {e}");
            ExitCode::from(1)
        }
    }
}
