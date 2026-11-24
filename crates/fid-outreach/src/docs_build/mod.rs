//! Edition build, page frame and navigation (spec node `OUTR.DOCS.FRAME.NAV`).
//!
//! `fid-outreach docs-build --src <dir> --ref <dir> --release <tag> --out <dir> [--guides <file>]`
//! builds one static release edition under `<out>/<release>/`:
//! - Renders every Markdown source under `<src>/book/`, `<src>/contributor/` (and optional
//!   `<src>/index.md`) with the bounded CommonMark + tables + fenced-code renderer
//!   ([`markdown`]), escaping raw HTML except well-formed `<!-- cap: ... -->` and
//!   `<!-- cap-planned: ... -->` comments.
//! - Wraps every Markdown page and every pre-rendered HTML fragment under `--ref` (placed at
//!   `<out>/<release>/ref/...`) in the shared frame ([`frame`]) with header, release banner,
//!   `<!-- edition-notice -->` placeholder, navigation tree from `<src>/nav.toml`, main content
//!   and footer.
//! - Copies `<src>/assets/` once into `<out>/<release>/assets/`, refusing any non-static file
//!   extension.
//! - Ensures every internal link is relative and ends in a file name so the edition browses
//!   offline from `file://`.
//! - Fails before writing anything if `nav.toml` names a missing source (`NavSourceMissing`), if
//!   `<out>/<release>` already exists and is non-empty (`OutputExists`), or if an asset is outside
//!   the static allowlist (`NonStaticAsset`).
//! - Determinism D0: sorted `BTreeMap` traversal, no timestamps in output, fixed file mode `0644`.

pub mod frame;
pub mod markdown;

use crate::ref_pages::valid_release;
use frame::{FrameParams, FRAME_CSS, FRAME_CSS_PATH};
use markdown::render_markdown;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest single source or asset file read, in bytes (5 MB per source).
pub const MAX_SOURCE_BYTES: usize = 5 * 1024 * 1024;
/// Largest `nav.toml` file read, in bytes.
pub const MAX_NAV_BYTES: usize = 1024 * 1024;
/// Most files accepted in one edition.
pub const MAX_FILES: usize = 100_000;
/// Deepest directory nesting below `--src` or `--ref`.
pub const MAX_DIR_DEPTH: usize = 32;
/// Deepest nesting inside `nav.toml` item children.
pub const MAX_NAV_DEPTH: usize = 16;

/// Allowed static output extensions (REQ-OUTR.DOCS.FRAME.NAV-006).
/// Matches the static-only set of HTML, CSS, image, font, media, JSON and JS data files;
/// no server-side executable extensions are ever written.
pub const STATIC_EXTENSIONS: [&str; 23] = [
    "avif", "css", "gif", "html", "ico", "jpeg", "jpg", "js", "json", "mp3", "mp4", "ogg", "otf", "png", "svg", "ttf",
    "txt", "vtt", "wav", "webm", "webp", "woff", "woff2",
];

/// Command options for [`build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Directory containing `book/`, `contributor/`, `nav.toml` and optional `assets/` / `index.md`.
    pub src: PathBuf,
    /// Directory containing pre-rendered reference HTML fragments (e.g. `cli/...`, `mcp/...`).
    pub ref_dir: PathBuf,
    /// Release tag documented by this edition (e.g. `v0.2.0`).
    pub release: String,
    /// Site output directory; the edition is written to `<out>/<release>/`.
    pub out: PathBuf,
    /// Optional guide registration file (`guides.toml`), passed through to `OUTR.DOCS.FRAME.SLOT`.
    pub guides: Option<PathBuf>,
}

/// Parsed `nav.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavTree {
    pub version: i64,
    pub sections: Vec<NavSection>,
}

/// One top-level section in `nav.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavSection {
    pub title: String,
    pub items: Vec<NavItem>,
}

/// One navigation entry, optionally with nested child entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavItem {
    pub title: String,
    /// Source path as written in `nav.toml` (e.g. `book/units.md` or `ref/cli/demo.read_item@1.html`).
    pub source: String,
    /// Rendered output path relative to `<out>/<release>/` (e.g. `book/units.html`).
    pub output_path: String,
    pub children: Vec<NavItem>,
}

/// Summary returned after a successful build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildReport {
    pub release: String,
    pub edition_dir: PathBuf,
    pub pages_written: usize,
    pub assets_written: usize,
    /// Output paths written under `<out>/<release>/`, in sorted order.
    pub files: Vec<String>,
}

/// Errors produced by [`build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// `nav.toml` named one or more sources that do not exist (exit 1; nothing written).
    NavSourceMissing { missing: Vec<String> },
    /// `<out>/<release>` already exists and is non-empty (exit 1; nothing written).
    OutputExists { path: String },
    /// An asset under `<src>/assets/` has a disallowed extension (exit 1; nothing written).
    NonStaticAsset { path: String },
    /// A source directory or file cannot be read, is a symlink, exceeds the size bound, or is not UTF-8 (exit 2).
    SourceUnreadable { path: String, reason: String },
    /// `nav.toml` failed to parse or violated the schema (exit 2).
    NavParse { path: String, reason: String },
    /// Release tag is not a safe single path segment (exit 2).
    InvalidRelease { release: String },
    /// Writing an output directory or file failed (exit 2).
    OutputUnwritable { path: String, reason: String },
}

impl BuildError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            BuildError::NavSourceMissing { .. } => "nav_source_missing",
            BuildError::OutputExists { .. } => "output_exists",
            BuildError::NonStaticAsset { .. } => "non_static_asset",
            BuildError::SourceUnreadable { .. } => "source_unreadable",
            BuildError::NavParse { .. } => "nav_parse",
            BuildError::InvalidRelease { .. } => "invalid_release",
            BuildError::OutputUnwritable { .. } => "output_unwritable",
        }
    }

    /// Exit code: 1 for build refusals (`NavSourceMissing`, `OutputExists`, `NonStaticAsset`),
    /// 2 for unreadable/unparsable inputs, bad release tag, or unwritable output.
    pub fn exit_code(&self) -> u8 {
        match self {
            BuildError::NavSourceMissing { .. }
            | BuildError::OutputExists { .. }
            | BuildError::NonStaticAsset { .. } => 1,
            BuildError::SourceUnreadable { .. }
            | BuildError::NavParse { .. }
            | BuildError::InvalidRelease { .. }
            | BuildError::OutputUnwritable { .. } => 2,
        }
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::NavSourceMissing { missing } => {
                write!(f, "nav_source_missing: {}", missing.join(", "))
            }
            BuildError::OutputExists { path } => {
                write!(f, "output_exists {path}: edition directory already exists and is not empty")
            }
            BuildError::NonStaticAsset { path } => {
                write!(f, "non_static_asset {path}: extension is outside the static allowlist")
            }
            BuildError::SourceUnreadable { path, reason } => {
                write!(f, "source_unreadable {path}: {reason}")
            }
            BuildError::NavParse { path, reason } => {
                write!(f, "nav_parse {path}: {reason}")
            }
            BuildError::InvalidRelease { release } => {
                write!(f, "invalid_release {release:?}: release tag must be a safe single path segment")
            }
            BuildError::OutputUnwritable { path, reason } => {
                write!(f, "output_unwritable {path}: {reason}")
            }
        }
    }
}

impl std::error::Error for BuildError {}

/// Returns true if `path` has an extension in [`STATIC_EXTENSIONS`] (ASCII case-insensitive).
pub fn is_static_path(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let Some((_, ext)) = file_name.rsplit_once('.') else {
        return false;
    };
    let lower = ext.to_ascii_lowercase();
    STATIC_EXTENSIONS.contains(&lower.as_str())
}

/// Parses `nav.toml` text into a [`NavTree`].
pub fn parse_nav_toml(text: &str, path_label: &str) -> Result<NavTree, BuildError> {
    let err = |reason: String| BuildError::NavParse { path: path_label.to_string(), reason };
    let val: toml::Value = toml::from_str(text).map_err(|e| err(e.to_string()))?;
    let table = val.as_table().ok_or_else(|| err("top level must be a table".into()))?;
    for key in table.keys() {
        if key != "version" && key != "section" {
            return Err(err(format!("unknown top-level key {key:?}")));
        }
    }
    let version = table
        .get("version")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| err("missing integer field `version`".into()))?;
    if version != 1 {
        return Err(err(format!("unsupported nav.toml version {version} (expected 1)")));
    }
    let sections_val = table
        .get("section")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| err("missing array `[[section]]`".into()))?;
    let mut sections = Vec::with_capacity(sections_val.len());
    for (idx, sec_val) in sections_val.iter().enumerate() {
        let sec_tbl = sec_val.as_table().ok_or_else(|| err(format!("section[{idx}] must be a table")))?;
        for key in sec_tbl.keys() {
            if key != "title" && key != "items" {
                return Err(err(format!("section[{idx}]: unknown key {key:?}")));
            }
        }
        let title = sec_tbl
            .get("title")
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| err(format!("section[{idx}]: missing non-empty string `title`")))?
            .to_string();
        let items_val = sec_tbl
            .get("items")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| err(format!("section[{idx}]: missing array `items`")))?;
        let mut items = Vec::with_capacity(items_val.len());
        for (j, item_val) in items_val.iter().enumerate() {
            items.push(parse_nav_item(item_val, &format!("section[{idx}].items[{j}]"), 0, path_label)?);
        }
        sections.push(NavSection { title, items });
    }
    Ok(NavTree { version, sections })
}

fn parse_nav_item(val: &toml::Value, ctx: &str, depth: usize, path_label: &str) -> Result<NavItem, BuildError> {
    let err = |reason: String| BuildError::NavParse { path: path_label.to_string(), reason };
    if depth >= MAX_NAV_DEPTH {
        return Err(err(format!("{ctx}: navigation nesting exceeds depth {MAX_NAV_DEPTH}")));
    }
    let tbl = val.as_table().ok_or_else(|| err(format!("{ctx}: must be a table")))?;
    for key in tbl.keys() {
        if key != "title" && key != "source" && key != "children" {
            return Err(err(format!("{ctx}: unknown key {key:?}")));
        }
    }
    let title = tbl
        .get("title")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err(format!("{ctx}: missing non-empty string `title`")))?
        .to_string();
    let source = tbl
        .get("source")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err(format!("{ctx}: missing non-empty string `source`")))?
        .to_string();
    let output_path = nav_source_to_output(&source).ok_or_else(|| {
        err(format!("{ctx}: invalid source path {source:?} (must be a clean relative .md or .html path)"))
    })?;
    let mut children = Vec::new();
    if let Some(ch_val) = tbl.get("children") {
        let ch_arr = ch_val.as_array().ok_or_else(|| err(format!("{ctx}.children: must be an array")))?;
        for (k, child) in ch_arr.iter().enumerate() {
            children.push(parse_nav_item(child, &format!("{ctx}.children[{k}]"), depth + 1, path_label)?);
        }
    }
    Ok(NavItem { title, source, output_path, children })
}

/// Validates a `nav.toml` source path and maps it to its rendered output path.
fn nav_source_to_output(source: &str) -> Option<String> {
    if source.is_empty()
        || source.starts_with('/')
        || source.ends_with('/')
        || source.contains('\\')
        || source.bytes().any(|b| b < 0x20 || b == 0x7f)
    {
        return None;
    }
    for seg in source.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return None;
        }
    }
    if let Some(stem) = source.strip_suffix(".md") {
        Some(format!("{stem}.html"))
    } else if source.ends_with(".html") {
        Some(source.to_string())
    } else {
        None
    }
}

/// Builds one static release edition under `<out>/<release>/`.
pub fn build(options: &Options) -> Result<BuildReport, BuildError> {
    if !valid_release(&options.release) {
        return Err(BuildError::InvalidRelease { release: options.release.clone() });
    }
    ensure_real_dir(&options.src)?;
    ensure_real_dir(&options.ref_dir)?;

    // If `--guides` is passed, verify bounded readability here (passed through to OUTR.DOCS.FRAME.SLOT).
    if let Some(guides_path) = &options.guides {
        let _ = read_utf8_bounded(guides_path, MAX_SOURCE_BYTES)?;
    }

    // Check output edition directory before doing any writes.
    let edition_dir = options.out.join(&options.release);
    check_edition_dir_writable(&edition_dir)?;

    // Parse `<src>/nav.toml`.
    let nav_path = options.src.join("nav.toml");
    let nav_text = read_utf8_bounded(&nav_path, MAX_NAV_BYTES)?;
    let nav = parse_nav_toml(&nav_text, &display_path(&nav_path))?;

    // Discover sources in deterministic sorted order.
    let mut md_sources: BTreeMap<String, String> = BTreeMap::new(); // source_key -> markdown text
    let mut ref_sources: BTreeMap<String, String> = BTreeMap::new(); // source_key (`ref/...`) -> html fragment
    let mut asset_files: BTreeMap<String, Vec<u8>> = BTreeMap::new(); // output_path (`assets/...`) -> bytes
    let mut file_count: usize = 0;

    // Optional root `index.md`.
    let root_index = options.src.join("index.md");
    if path_exists_no_follow(&root_index)? {
        file_count = bump_file_count(file_count, &root_index)?;
        let text = read_utf8_bounded(&root_index, MAX_SOURCE_BYTES)?;
        md_sources.insert("index.md".to_string(), text);
    }

    // `book/` and `contributor/` Markdown sources.
    for sub in ["book", "contributor"] {
        let dir = options.src.join(sub);
        if path_exists_no_follow(&dir)? {
            ensure_real_dir(&dir)?;
            let mut found = BTreeMap::new();
            walk_files(&dir, sub, 0, &mut file_count, &mut found)?;
            for (rel, path) in found {
                if rel.ends_with(".md") {
                    let text = read_utf8_bounded(&path, MAX_SOURCE_BYTES)?;
                    md_sources.insert(rel, text);
                }
            }
        }
    }

    // Reference HTML fragments under `--ref`.
    {
        let mut found = BTreeMap::new();
        walk_files(&options.ref_dir, "ref", 0, &mut file_count, &mut found)?;
        for (rel, path) in found {
            if rel.ends_with(".html") {
                let text = read_utf8_bounded(&path, MAX_SOURCE_BYTES)?;
                ref_sources.insert(rel, text);
            }
        }
    }

    // Optional `assets/` static files.
    let assets_dir = options.src.join("assets");
    if path_exists_no_follow(&assets_dir)? {
        ensure_real_dir(&assets_dir)?;
        let mut found = BTreeMap::new();
        walk_files(&assets_dir, "assets", 0, &mut file_count, &mut found)?;
        for (rel, path) in found {
            if !is_static_path(&rel) {
                return Err(BuildError::NonStaticAsset { path: rel });
            }
            let bytes = read_bytes_bounded(&path, MAX_SOURCE_BYTES)?;
            asset_files.insert(rel, bytes);
        }
    }

    // Validate that every source named in `nav.toml` exists.
    let mut all_sources: BTreeSet<&str> = BTreeSet::new();
    for k in md_sources.keys() {
        all_sources.insert(k.as_str());
    }
    for k in ref_sources.keys() {
        all_sources.insert(k.as_str());
    }
    let mut missing: Vec<String> = Vec::new();
    let mut missing_seen: BTreeSet<String> = BTreeSet::new();
    collect_missing_nav(&nav, &all_sources, &mut missing, &mut missing_seen);
    if !missing.is_empty() {
        return Err(BuildError::NavSourceMissing { missing });
    }

    // Collect `output_path -> nav_title` and choose the edition home page (always a file name).
    let mut nav_titles: BTreeMap<String, String> = BTreeMap::new();
    let first_nav_output = collect_nav_titles(&nav, &mut nav_titles);

    // Map all output HTML paths to determine home_page fallback deterministically.
    let mut all_html_outputs: BTreeSet<String> = BTreeSet::new();
    for src_key in md_sources.keys() {
        if let Some(out_path) = nav_source_to_output(src_key) {
            all_html_outputs.insert(out_path);
        }
    }
    for src_key in ref_sources.keys() {
        all_html_outputs.insert(src_key.clone());
    }

    let home_page = if all_html_outputs.contains("index.html") {
        "index.html".to_string()
    } else if let Some(first) = first_nav_output {
        first
    } else if let Some(first) = all_html_outputs.iter().next() {
        first.clone()
    } else {
        "index.html".to_string()
    };

    // Render all pages in memory before writing anything to disk.
    let mut outputs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut pages_written: usize = 0;

    for (src_key, md_text) in &md_sources {
        let Some(output_path) = nav_source_to_output(src_key) else { continue };
        let rendered = render_markdown(md_text, &output_path);
        let title = rendered
            .first_h1
            .as_deref()
            .or(rendered.front_matter_title.as_deref())
            .or_else(|| nav_titles.get(&output_path).map(String::as_str))
            .unwrap_or(&output_path);
        let page_html = frame::render_page(&FrameParams {
            release: &options.release,
            title,
            current_page: &output_path,
            home_page: &home_page,
            nav: &nav,
            body_html: &rendered.html,
        });
        outputs.insert(output_path, page_html.into_bytes());
        pages_written += 1;
    }

    for (output_path, fragment) in &ref_sources {
        let derived_title = derive_ref_title(output_path, fragment, nav_titles.get(output_path).map(String::as_str));
        let page_html = frame::render_page(&FrameParams {
            release: &options.release,
            title: &derived_title,
            current_page: output_path,
            home_page: &home_page,
            nav: &nav,
            body_html: fragment,
        });
        outputs.insert(output_path.clone(), page_html.into_bytes());
        pages_written += 1;
    }

    // Stylesheet and static brand assets.
    outputs.insert(FRAME_CSS_PATH.to_string(), FRAME_CSS.as_bytes().to_vec());
    let assets_written = asset_files.len();
    for (rel, bytes) in asset_files {
        outputs.insert(rel, bytes);
    }

    // Defense-in-depth static allowlist check on every file to be written (REQ-006).
    for rel in outputs.keys() {
        if !is_static_path(rel) {
            return Err(BuildError::NonStaticAsset { path: rel.clone() });
        }
    }

    // Write files deterministically in sorted `BTreeMap` order.
    let mut written_files: Vec<String> = Vec::with_capacity(outputs.len());
    for (rel, bytes) in &outputs {
        let dest = edition_dir.join(rel);
        write_file_fixed_mode(&dest, bytes)?;
        written_files.push(rel.clone());
    }

    Ok(BuildReport {
        release: options.release.clone(),
        edition_dir,
        pages_written,
        assets_written,
        files: written_files,
    })
}

fn collect_missing_nav(
    nav: &NavTree,
    all_sources: &BTreeSet<&str>,
    missing: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
) {
    fn visit(items: &[NavItem], all_sources: &BTreeSet<&str>, missing: &mut Vec<String>, seen: &mut BTreeSet<String>) {
        for item in items {
            if !all_sources.contains(item.source.as_str()) && seen.insert(item.source.clone()) {
                missing.push(item.source.clone());
            }
            visit(&item.children, all_sources, missing, seen);
        }
    }
    for sec in &nav.sections {
        visit(&sec.items, all_sources, missing, seen);
    }
}

fn collect_nav_titles(nav: &NavTree, titles: &mut BTreeMap<String, String>) -> Option<String> {
    fn visit(items: &[NavItem], titles: &mut BTreeMap<String, String>, first: &mut Option<String>) {
        for item in items {
            if first.is_none() {
                *first = Some(item.output_path.clone());
            }
            titles.entry(item.output_path.clone()).or_insert_with(|| item.title.clone());
            visit(&item.children, titles, first);
        }
    }
    let mut first = None;
    for sec in &nav.sections {
        visit(&sec.items, titles, &mut first);
    }
    first
}

/// Derives a distinct, readable `<title>` for a reference page fragment.
/// Prefers the `nav.toml` title when present; otherwise extracts the first `<h1>` text and
/// appends the surface label (`CLI`, `MCP`, `Python`) inferred from `ref/<surface>/...`.
fn derive_ref_title(output_path: &str, fragment: &str, nav_title: Option<&str>) -> String {
    if let Some(t) = nav_title {
        if !t.trim().is_empty() {
            return t.to_string();
        }
    }
    let surface_suffix = match output_path.split('/').nth(1) {
        Some("cli") => Some("CLI"),
        Some("mcp") => Some("MCP"),
        Some("python") => Some("Python"),
        _ => None,
    };
    if let Some(h1) = extract_first_h1(fragment) {
        if let Some(sfx) = surface_suffix {
            return format!("{h1} ({sfx})");
        }
        return h1;
    }
    output_path.to_string()
}

fn extract_first_h1(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start_tag = lower.find("<h1")?;
    let after_open = html.get(start_tag + 3..)?;
    let gt = after_open.find('>')?;
    let body_start = start_tag + 3 + gt + 1;
    let close_rel = lower.get(body_start..)?.find("</h1>")?;
    let inner = html.get(body_start..body_start + close_rel)?;
    // Strip tags inside `<h1>` (e.g. `<code>demo.read_item@1</code>`).
    let mut plain = String::with_capacity(inner.len());
    let mut in_tag = false;
    for c in inner.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => plain.push(c),
            _ => {}
        }
    }
    let collapsed = plain
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let trimmed = collapsed.split_whitespace().collect::<Vec<_>>().join(" ");
    (!trimmed.is_empty()).then_some(trimmed)
}

fn display_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn path_exists_no_follow(p: &Path) -> Result<bool, BuildError> {
    match std::fs::symlink_metadata(p) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(BuildError::SourceUnreadable {
                    path: display_path(p),
                    reason: "symbolic links are not allowed".into(),
                });
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(BuildError::SourceUnreadable { path: display_path(p), reason: e.to_string() }),
    }
}

fn ensure_real_dir(p: &Path) -> Result<(), BuildError> {
    let meta = std::fs::symlink_metadata(p)
        .map_err(|e| BuildError::SourceUnreadable { path: display_path(p), reason: e.to_string() })?;
    if meta.file_type().is_symlink() {
        return Err(BuildError::SourceUnreadable {
            path: display_path(p),
            reason: "symbolic links are not allowed".into(),
        });
    }
    if !meta.is_dir() {
        return Err(BuildError::SourceUnreadable { path: display_path(p), reason: "not a directory".into() });
    }
    Ok(())
}

fn check_edition_dir_writable(edition_dir: &Path) -> Result<(), BuildError> {
    match std::fs::symlink_metadata(edition_dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(BuildError::OutputUnwritable { path: display_path(edition_dir), reason: e.to_string() }),
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(BuildError::OutputExists { path: display_path(edition_dir) });
            }
            let mut entries = std::fs::read_dir(edition_dir)
                .map_err(|e| BuildError::OutputUnwritable { path: display_path(edition_dir), reason: e.to_string() })?;
            if entries.next().is_some() {
                return Err(BuildError::OutputExists { path: display_path(edition_dir) });
            }
            Ok(())
        }
    }
}

fn bump_file_count(count: usize, p: &Path) -> Result<usize, BuildError> {
    let next = count.saturating_add(1);
    if next > MAX_FILES {
        return Err(BuildError::SourceUnreadable {
            path: display_path(p),
            reason: format!("more than {MAX_FILES} files"),
        });
    }
    Ok(next)
}

fn walk_files(
    dir: &Path,
    prefix: &str,
    depth: usize,
    file_count: &mut usize,
    out: &mut BTreeMap<String, PathBuf>,
) -> Result<(), BuildError> {
    if depth > MAX_DIR_DEPTH {
        return Err(BuildError::SourceUnreadable {
            path: display_path(dir),
            reason: format!("directory nesting exceeds depth {MAX_DIR_DEPTH}"),
        });
    }
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir)
        .map_err(|e| BuildError::SourceUnreadable { path: display_path(dir), reason: e.to_string() })?
    {
        let entry =
            entry.map_err(|e| BuildError::SourceUnreadable { path: display_path(dir), reason: e.to_string() })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        entries.push((name, entry.path()));
    }
    entries.sort();
    for (name, path) in entries {
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| BuildError::SourceUnreadable { path: display_path(&path), reason: e.to_string() })?;
        if meta.file_type().is_symlink() {
            return Err(BuildError::SourceUnreadable {
                path: display_path(&path),
                reason: "symbolic links are not allowed".into(),
            });
        }
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if meta.is_dir() {
            walk_files(&path, &rel, depth + 1, file_count, out)?;
        } else if meta.is_file() {
            *file_count = bump_file_count(*file_count, &path)?;
            out.insert(rel, path);
        }
    }
    Ok(())
}

fn read_bytes_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>, BuildError> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| BuildError::SourceUnreadable { path: display_path(path), reason: e.to_string() })?;
    if meta.file_type().is_symlink() {
        return Err(BuildError::SourceUnreadable {
            path: display_path(path),
            reason: "symbolic links are not allowed".into(),
        });
    }
    if !meta.is_file() {
        return Err(BuildError::SourceUnreadable { path: display_path(path), reason: "not a regular file".into() });
    }
    if meta.len() > max_bytes as u64 {
        return Err(BuildError::SourceUnreadable {
            path: display_path(path),
            reason: format!("file exceeds {max_bytes} byte limit"),
        });
    }
    let file = std::fs::File::open(path)
        .map_err(|e| BuildError::SourceUnreadable { path: display_path(path), reason: e.to_string() })?;
    let mut buf = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| BuildError::SourceUnreadable { path: display_path(path), reason: e.to_string() })?;
    if buf.len() > max_bytes {
        return Err(BuildError::SourceUnreadable {
            path: display_path(path),
            reason: format!("file exceeds {max_bytes} byte limit"),
        });
    }
    Ok(buf)
}

fn read_utf8_bounded(path: &Path, max_bytes: usize) -> Result<String, BuildError> {
    let bytes = read_bytes_bounded(path, max_bytes)?;
    String::from_utf8(bytes)
        .map_err(|e| BuildError::SourceUnreadable { path: display_path(path), reason: format!("invalid UTF-8: {e}") })
}

fn write_file_fixed_mode(path: &Path, bytes: &[u8]) -> Result<(), BuildError> {
    let err = |e: std::io::Error| BuildError::OutputUnwritable { path: display_path(path), reason: e.to_string() };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(err)?;
    }
    std::fs::write(path, bytes).map_err(err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).map_err(err)?;
    }
    Ok(())
}
