//! File handles and approved roots. Spec node `CORE.BUS.POL.FS`.
//!
//! Agents and scripts never pass raw file-system paths (CONSTITUTION §3, D-012). A parameter
//! the command declares `"path_like": true` carries a handle `root:<root_id>/<relative path>`
//! that names a file below an approved import or export root. The roots are an A0 admin
//! setting ([`ROOTS_KEY`], declared here with `fid_store::fid_settings!` and read only through
//! `fid_store::settings::resolve`); nothing in this module adds, widens or changes a root
//! (REQ-006).
//!
//! [`resolve`] turns a parameter value into a [`ResolvedPath`], which only this module can
//! build, so a command crate cannot open an unchecked path. The checks run in a fixed order and
//! every refusal is a `policy` [`BusError`]:
//!
//! 1. **Raw paths** (REQ-001): a value that does not start with `root:` is a raw path. Only a
//!    `human` principal (a GUI session or an upgraded CLI session) may pass one; every other
//!    kind gets `raw_path_refused`. The rule names agent and script; service, device and
//!    system are refused too (fail closed). [`check_raw_paths`] applies the same rule to a
//!    whole parameter object before anything else runs.
//! 2. **Lexical check** ([`FileHandle::parse`], no file-system call): `bad_handle` for a
//!    malformed root ID, an empty relative part or one naming the root itself, more than
//!    [`MAX_REL_BYTES`] bytes, an absolute part, empty segments, `.`, NUL and control
//!    characters, `\`, `:` (drive letters and alternate data streams, so also UNC and `\\?\`
//!    prefixes), the other characters Windows forbids (`<>"|?*`), Unicode lookalikes of
//!    `/ \ : .` and of those characters, format, private-use, unassigned and
//!    default-ignorable code points, blanks other than U+0020, text not in NFC, trailing dots
//!    or spaces, Windows device names (`CON`, `NUL`, `COM1`, `LPT¹`, `CONIN$` ... with any
//!    extension) and 8.3 short-name aliases (`~` followed by a digit). The same rules apply on
//!    every platform, so a handle means the same everywhere. A `..` segment in an otherwise
//!    well-formed handle gives `outside_root` (AC-4); it is never resolved, even when it
//!    would stay inside the root.
//! 3. **Root and effect** (REQ-003): `bad_root` when the command declares `fs_effect: none`,
//!    the root ID is not configured, or an import root is used for `write_root` or an export
//!    root for `read_root`.
//! 4. **Extension** (REQ-004, ASCII case-insensitive): the last `.`-separated part of the file
//!    name, with a non-empty stem, must be in the parameter's declared list (`x-extensions`)
//!    and, when the root has its own list, in that too. `part.stl.exe` has extension `exe`. A
//!    parameter without a list allows nothing (fail closed). Checked on the handle's name and
//!    again on the name the handle resolves to, so a link `a.stl -> b.exe` is refused.
//! 5. **Containment** (REQ-002): the relative path is walked component by component below the
//!    canonical root without following links; each symbolic link or junction is read and
//!    expanded in place, and a target that leaves the root, crosses into another volume
//!    namespace or loops more than [`MAX_LINK_HOPS`] times gives `outside_root`. The resolved
//!    file (read) or folder (write) is then canonicalised by the OS and checked again, on
//!    canonical components, to lie below the canonical root. A file to read with more than one
//!    hard link gives `outside_root` (Unix `st_nlink`, Windows `nNumberOfLinks` from the open
//!    handle): a hard link may share its data with a file outside the root and cannot be told
//!    apart by path.
//! 6. **Existence**: a file to read must exist (`precondition/false`) and be a regular file
//!    (`bad_handle` otherwise). For `write_root` the parent folder must exist
//!    (`precondition/false`) and the target must not (`would_overwrite`, REQ-005); a link of
//!    that name counts as existing and is never followed.
//!
//! **At open.** The checks are repeated when the handler opens the file
//! ([`ResolvedPath::open_read`], [`ResolvedPath::create_new`]); `resolve` itself runs the same
//! open once for a read. Both open through the root folder held open as a capability
//! (`cap_std::fs::Dir`), which refuses any open that would leave it (`openat2(RESOLVE_BENEATH)`
//! on Linux, a checked walk elsewhere), so a folder swapped for an escaping link after
//! resolution makes the open fail with `outside_root` instead of redirecting it. The final
//! component is opened without following links (`O_NOFOLLOW` on Unix,
//! `FILE_FLAG_OPEN_REPARSE_POINT` on Windows, where an opened reparse point is then refused),
//! so the name whose extension was checked is the file that is opened; the opened file is
//! re-checked (regular file, one hard link). `create_new` creates with `O_EXCL` /
//! `CREATE_NEW`, so a file or link that appears between resolution and open is never
//! overwritten (`would_overwrite`).
//!
//! **Residual risk** (documented, not closed here): an intermediate folder swapped after
//! resolution for a link that stays inside the root is still followed by the capability open,
//! so the file opened may lie in another folder of the same root (containment and the
//! extension rule still hold). On Windows, files that are reparse points for other reasons
//! (cloud placeholders, deduplicated files) are refused (fail closed). Crash-safe writes are
//! `CORE.STO`'s job.
//!
//! A human's raw path must be absolute. It follows links (the person chose it), but the
//! extension list and the no-overwrite rule still apply. Determinism D0 given the file-system
//! state, which is an observed input.

use crate::error::{BusError, Detail, InternalReason, PolicyReason, PreconditionReason};
use crate::principal::{forbidden, Principal, PrincipalKind};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::Dir;
use fid_canon::CValue;
use fid_registry::{CommandDef, FsEffect};
use fid_store::settings::{resolve as resolve_settings, Scope, SettingWarning, SettingsRegistry, Sources};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};
use unicode_normalization::is_nfc;

/// The handle scheme.
pub const HANDLE_PREFIX: &str = "root:";
/// Longest relative part of a handle, in bytes (REQ-006).
pub const MAX_REL_BYTES: usize = 1024;
/// Longest segment of a relative part, in bytes.
pub const MAX_SEGMENT_BYTES: usize = 255;
/// Longest root ID, in bytes.
pub const MAX_ROOT_ID_BYTES: usize = 64;
/// Longest extension, in bytes.
pub const MAX_EXT_BYTES: usize = 16;
/// Most links one resolution may expand; more is treated as a loop (`outside_root`).
pub const MAX_LINK_HOPS: usize = 40;
/// Longest raw path a human may pass, in bytes.
pub const MAX_RAW_PATH_BYTES: usize = 32_767;
/// Most approved roots the setting may hold.
pub const MAX_ROOTS: usize = 256;
/// Most extensions one root or one parameter may list.
pub const MAX_EXTENSIONS: usize = 64;
/// The admin setting that holds the approved roots (CORE.STO.SET, A0, machine scope).
pub const ROOTS_KEY: &str = "bus.fs_roots";
/// The parameter-schema keyword that marks a file parameter (shared with AUTO.SURF.ROOTS and
/// AGENT.REDTEAM.SCRIPT).
pub const PATH_LIKE_KEYWORD: &str = "path_like";
/// The parameter-schema keyword listing a file parameter's allowed extensions.
pub const EXTENSIONS_KEYWORD: &str = "x-extensions";

fid_store::fid_settings! {
    pub fn fs_settings;
    {
        key: "bus.fs_roots",
        owner: "bus",
        schema: r#"{
            "type": "array",
            "maxItems": 256,
            "items": {
                "type": "object",
                "required": ["id", "kind", "path"],
                "properties": {
                    "id": {"type": "string", "minLength": 1, "maxLength": 64},
                    "kind": {"type": "string", "enum": ["import", "export"]},
                    "path": {"type": "string", "minLength": 1, "maxLength": 4096},
                    "extensions": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 64,
                        "uniqueItems": true,
                        "items": {"type": "string", "minLength": 1, "maxLength": 16}
                    }
                }
            }
        }"#,
        default: "[]",
        scopes: [Machine],
        admin: true,
    }
}

// ------------------------------------------------------------------ errors

fn policy(reason: PolicyReason, what: &str) -> BusError {
    BusError::Policy(reason, Detail::new(what))
}

fn bad_handle(what: &str) -> BusError {
    policy(PolicyReason::BadHandle, what)
}

fn outside(what: &str) -> BusError {
    policy(PolicyReason::OutsideRoot, what)
}

fn missing(what: &str) -> BusError {
    BusError::Precondition(PreconditionReason::False, Detail::new(what))
}

fn invariant(what: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(what))
}

fn storage(e: &io::Error) -> BusError {
    BusError::Internal(InternalReason::Storage, Detail::new(&format!("file-system error: {:?}", e.kind())))
}

/// Maps a failed file-system call: a capability escape is `outside_root`, a missing file
/// `precondition/false`, an existing one `would_overwrite`, anything else `internal/storage`.
fn io_refusal(e: &io::Error) -> BusError {
    if is_escape(e) {
        return outside("the path leads outside its approved root");
    }
    if e.kind() == io::ErrorKind::NotFound {
        missing("the file or its folder does not exist")
    } else if e.kind() == io::ErrorKind::AlreadyExists {
        policy(PolicyReason::WouldOverwrite, "the target already exists; write_root never overwrites")
    } else {
        storage(e)
    }
}

/// True for `cap-std`'s refusal of a path that would leave the folder it was opened through
/// (`PermissionDenied`, "a path led outside of the filesystem"), and for Linux's `EXDEV` from
/// `openat2(RESOLVE_BENEATH)`.
fn is_escape(e: &io::Error) -> bool {
    let cap_std_escape =
        e.kind() == io::ErrorKind::PermissionDenied && e.to_string().contains("outside of the filesystem");
    #[cfg(target_os = "linux")]
    let exdev = e.raw_os_error() == Some(libc::EXDEV);
    #[cfg(not(target_os = "linux"))]
    let exdev = false;
    cap_std_escape || exdev
}

// ------------------------------------------------------------------ names

/// A root ID: 1 to [`MAX_ROOT_ID_BYTES`] lowercase ASCII letters, digits, `-` or `_`, starting
/// with a letter or digit. Lowercase only, so two IDs never differ only in case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootId(String);

impl RootId {
    /// Checks a root ID; `None` if it breaks the grammar.
    pub fn new(id: &str) -> Option<RootId> {
        let b = id.as_bytes();
        let low = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
        let ok = !b.is_empty()
            && b.len() <= MAX_ROOT_ID_BYTES
            && b.first().is_some_and(low)
            && b.iter().all(|c| low(c) || *c == b'-' || *c == b'_');
        ok.then(|| RootId(id.to_string()))
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RootId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A file extension: 1 to [`MAX_EXT_BYTES`] ASCII letters or digits, stored lowercase.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ext(String);

impl Ext {
    /// Checks an extension (without the dot) and folds it to lowercase; `None` if it is empty,
    /// too long, or has anything but ASCII letters and digits.
    pub fn new(ext: &str) -> Option<Ext> {
        let ok = !ext.is_empty() && ext.len() <= MAX_EXT_BYTES && ext.bytes().all(|b| b.is_ascii_alphanumeric());
        ok.then(|| Ext(ext.to_ascii_lowercase()))
    }

    /// The lowercase extension text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Ext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The extension of a file name: the part after the last `.`, when the part before it is not
/// empty and the part after it is a valid [`Ext`]. `None` for `noext`, `.stl`, `a.` and
/// `a.ſtl`.
pub fn file_ext(name: &str) -> Option<Ext> {
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    Ext::new(ext)
}

/// Characters refused in a segment on every platform: the ASCII characters Windows forbids in
/// names, and Unicode lookalikes of `/ \ : .` and of those, which a best-fit code-page
/// conversion or a careless reader may turn into the ASCII character.
const LOOKALIKES: &[char] = &[
    '<', '>', '"', '|', '?', '*', // Windows-forbidden ASCII (`/`, `\`, `:` checked separately)
    '\u{a5}', '\u{20a9}', // yen and won signs: the backslash of Japanese and Korean code pages
    '\u{2044}', '\u{2215}', '\u{27cb}', '\u{29f8}', '\u{ff0f}', '\u{2571}', // slash lookalikes
    '\u{2216}', '\u{27cd}', '\u{29f5}', '\u{29f9}', '\u{fe68}', '\u{ff3c}', '\u{2572}', // backslash
    '\u{2236}', '\u{a789}', '\u{fe13}', '\u{fe55}', '\u{ff1a}', '\u{2d0}', '\u{589}', // colon
    '\u{2024}', '\u{fe52}', '\u{ff0e}', '\u{3002}', '\u{ff61}', // full stop
    '\u{ff1c}', '\u{ff1e}', '\u{ff02}', '\u{ff5c}', '\u{ff1f}', '\u{ff0a}', // fullwidth forms
];

/// Windows reserved device names (with any extension, and with trailing spaces before the
/// dot), refused on every platform so the outcome does not depend on the OS.
pub(crate) fn windows_device(c: &str) -> bool {
    let stem = c.split('.').next().unwrap_or(c).trim_end_matches(' ').to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$" => true,
        s => {
            // COM0-9, LPT0-9 and the superscript forms COM¹-³, LPT¹-³ (U+00B9, U+00B2, U+00B3).
            match s.strip_prefix("COM").or_else(|| s.strip_prefix("LPT")) {
                Some(t) => {
                    (t.len() == 1 && t.bytes().all(|b| b.is_ascii_digit()))
                        || ["\u{b9}", "\u{b2}", "\u{b3}"].contains(&t)
                }
                None => false,
            }
        }
    }
}

/// True if `c` names a plain child of its folder on every supported platform: see the module
/// documentation, step 2. `..` is not a plain name.
pub fn plain_segment(c: &str) -> bool {
    if c.is_empty() || c.len() > MAX_SEGMENT_BYTES || c.chars().all(|ch| ch == '.' || ch == ' ') {
        // Empty, `.`, `..` and dot-and-space names Windows may normalise to them.
        return false;
    }
    if c.ends_with('.') || c.ends_with(' ') {
        // Windows strips trailing dots and spaces, so the name would alias another.
        return false;
    }
    for ch in c.chars() {
        if ch == '/' || ch == '\\' || ch == ':' || ch == '\0' || LOOKALIKES.contains(&ch) || forbidden(ch).is_some() {
            return false;
        }
    }
    // 8.3 short names (`PROGRA~1`) alias a long name with a different extension.
    let b = c.as_bytes();
    if b.windows(2).any(|w| w[0] == b'~' && w[1].is_ascii_digit()) {
        return false;
    }
    is_nfc(c) && !windows_device(c)
}

// ------------------------------------------------------------------ handles

/// A lexically checked path relative to a root: one or more [`plain_segment`]s.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(Vec<String>);

impl RelPath {
    /// The segments, in order (never empty).
    pub fn segments(&self) -> &[String] {
        &self.0
    }

    /// The last segment: the file name.
    pub fn file_name(&self) -> &str {
        self.0.last().map(String::as_str).unwrap_or("")
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("/"))
    }
}

/// A parsed handle `root:<root_id>/<relative path>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileHandle {
    /// The approved root.
    pub root: RootId,
    /// The path below it.
    pub rel: RelPath,
}

/// True if `value` is in handle form (starts with `root:`, case-sensitive). Anything else in a
/// `path_like` parameter is a raw path.
pub fn is_handle(value: &str) -> bool {
    value.starts_with(HANDLE_PREFIX)
}

impl FileHandle {
    /// Parses a handle without touching the file system (module documentation, step 2).
    /// Errors: `policy/bad_handle`; `policy/outside_root` for a `..` segment in an otherwise
    /// well-formed handle.
    pub fn parse(text: &str) -> Result<FileHandle, BusError> {
        let rest = text.strip_prefix(HANDLE_PREFIX).ok_or_else(|| bad_handle("not a root: handle"))?;
        let Some((id, rel)) = rest.split_once('/') else {
            return Err(bad_handle("the handle names no file below its root"));
        };
        let root = RootId::new(id).ok_or_else(|| bad_handle("malformed root ID"))?;
        if rel.is_empty() {
            return Err(bad_handle("the handle names the root itself"));
        }
        if rel.len() > MAX_REL_BYTES {
            return Err(bad_handle("the relative part is longer than 1024 bytes"));
        }
        if rel.contains(['\0', '\\', ':']) {
            return Err(bad_handle("the relative part holds NUL, a backslash, a drive letter or a stream marker"));
        }
        if rel.starts_with('/') {
            return Err(bad_handle("the relative part is absolute"));
        }
        let mut parent_ref = false;
        let mut segments = Vec::new();
        for seg in rel.split('/') {
            if seg == ".." {
                parent_ref = true;
            } else if !plain_segment(seg) {
                return Err(bad_handle("a segment of the relative part is not a plain file name"));
            } else {
                segments.push(seg.to_string());
            }
        }
        if parent_ref {
            return Err(outside("`..` segments are never resolved"));
        }
        Ok(FileHandle { root, rel: RelPath(segments) })
    }
}

impl fmt::Display for FileHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{HANDLE_PREFIX}{}/{}", self.root, self.rel)
    }
}

// ------------------------------------------------------------------ roots

/// Which way an approved root may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RootKind {
    /// Read from (`fs_effect: read_root`).
    Import,
    /// Written into (`fs_effect: write_root`).
    Export,
}

impl RootKind {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            RootKind::Import => "import",
            RootKind::Export => "export",
        }
    }
}

/// One approved root, as configured by an administrator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    id: RootId,
    kind: RootKind,
    path: PathBuf,
    extensions: Option<BTreeSet<Ext>>,
}

impl Root {
    /// The root ID.
    pub fn id(&self) -> &RootId {
        &self.id
    }

    /// Import or export.
    pub fn kind(&self) -> RootKind {
        self.kind
    }

    /// The configured absolute path (canonicalised at each resolution).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The root's own extension list, if it has one.
    pub fn extensions(&self) -> Option<&BTreeSet<Ext>> {
        self.extensions.as_ref()
    }
}

/// Why the configured roots could not be read. Fail closed: the caller must then resolve
/// against [`Roots::default`] (no roots), so every handle gets `bad_root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootsError(String);

impl fmt::Display for RootsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "approved roots setting {ROOTS_KEY}: {}", self.0)
    }
}

impl std::error::Error for RootsError {}

/// A read-only snapshot of the approved roots. There is no API to add or change a root; a
/// snapshot is built only from the setting's value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roots {
    roots: BTreeMap<RootId, Root>,
}

impl Roots {
    /// Reads the value of [`ROOTS_KEY`]: an array of `{id, kind, path, extensions?}`. The whole
    /// value is refused on any error: unknown field, malformed or duplicate ID, unknown kind, a
    /// path that is not absolute or holds NUL, a malformed, empty or duplicate extension list
    /// (compared case-insensitively), or more than [`MAX_ROOTS`] roots.
    ///
    /// Test support only: production code reads roots through [`Roots::from_settings`], so a
    /// root never comes from anywhere but the admin setting.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn from_value(v: &CValue) -> Result<Roots, RootsError> {
        Roots::parse_value(v)
    }

    fn parse_value(v: &CValue) -> Result<Roots, RootsError> {
        let err = |s: &str| Err(RootsError(s.to_string()));
        let CValue::Arr(items) = v else { return err("must be an array") };
        if items.len() > MAX_ROOTS {
            return err("too many roots");
        }
        let mut roots = BTreeMap::new();
        for (i, item) in items.iter().enumerate() {
            let CValue::Obj(o) = item else { return err(&format!("root {i} must be an object")) };
            if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "id" | "kind" | "path" | "extensions")) {
                let k: String = k.chars().take(32).filter(|c| c.is_ascii_graphic()).collect();
                return err(&format!("root {i} has unknown field {k:?}"));
            }
            let id = match o.get("id") {
                Some(CValue::Str(s)) => RootId::new(s),
                _ => None,
            };
            let Some(id) = id else { return err(&format!("root {i}: id is missing or malformed")) };
            let kind = match o.get("kind") {
                Some(CValue::Str(s)) if s == "import" => RootKind::Import,
                Some(CValue::Str(s)) if s == "export" => RootKind::Export,
                _ => return err(&format!("root {id}: kind must be import or export")),
            };
            let path = match o.get("path") {
                Some(CValue::Str(s)) if !s.contains('\0') && Path::new(s).is_absolute() => PathBuf::from(s),
                _ => return err(&format!("root {id}: path must be an absolute path")),
            };
            let extensions = match o.get("extensions") {
                None => None,
                Some(v) => Some(ext_list(v).ok_or_else(|| RootsError(format!("root {id}: bad extension list")))?),
            };
            if roots.contains_key(&id) {
                return err(&format!("duplicate root id {id}"));
            }
            roots.insert(id.clone(), Root { id, kind, path, extensions });
        }
        Ok(Roots { roots })
    }

    /// Reads [`ROOTS_KEY`] through the settings layer (CORE.STO.SET, read only), with the
    /// warnings of the read. The declaration is this module's own ([`fs_settings`]: admin,
    /// machine scope only), never one supplied by the caller, so a value stored at user or
    /// project scope is ignored (with a `scope_not_allowed` warning). Fails if the value is
    /// malformed (see the parser's rules: unknown field, malformed or duplicate ID, unknown
    /// kind, a path that is not absolute or holds NUL, a malformed, empty or duplicate
    /// extension list, more than [`MAX_ROOTS`] roots).
    pub fn from_settings(sources: &Sources<'_>) -> Result<(Roots, Vec<SettingWarning>), RootsError> {
        let registry = SettingsRegistry::build(&fs_settings())
            .map_err(|_| RootsError("the built-in declaration does not build".into()))?;
        match registry.get(ROOTS_KEY) {
            Some(d) if d.admin() && !d.allows(Scope::User) && !d.allows(Scope::Project) => {}
            _ => return Err(RootsError("the built-in declaration is not an admin machine setting".into())),
        }
        let (values, warnings) = resolve_settings(&registry, sources, |d| d.key() == ROOTS_KEY);
        let value = values.into_iter().find(|r| r.key == ROOTS_KEY).ok_or_else(|| RootsError("not resolved".into()))?;
        Ok((Roots::parse_value(&value.value)?, warnings))
    }

    /// The root with this ID.
    pub fn get(&self, id: &RootId) -> Option<&Root> {
        self.roots.get(id)
    }

    /// Every root, by ID.
    pub fn iter(&self) -> impl Iterator<Item = &Root> {
        self.roots.values()
    }

    /// Number of roots.
    pub fn len(&self) -> usize {
        self.roots.len()
    }

    /// True if no root is configured.
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}

/// A non-empty list of distinct extensions (case-insensitive), at most [`MAX_EXTENSIONS`].
fn ext_list(v: &CValue) -> Option<BTreeSet<Ext>> {
    let CValue::Arr(items) = v else { return None };
    if items.is_empty() || items.len() > MAX_EXTENSIONS {
        return None;
    }
    let mut out = BTreeSet::new();
    for item in items {
        let CValue::Str(s) = item else { return None };
        if !out.insert(Ext::new(s)?) {
            return None;
        }
    }
    Some(out)
}

// ------------------------------------------------------------------ parameters

/// How a command uses its file parameter, from its `fs_effect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Access {
    /// `read_root`: read an existing file from an import root.
    Read,
    /// `write_root`: create a new file in an export root.
    Write,
}

impl Access {
    /// The access a command's `fs_effect` grants; `None` for `none`.
    pub fn from_fs_effect(effect: FsEffect) -> Option<Access> {
        match effect {
            FsEffect::None => None,
            FsEffect::ReadRoot => Some(Access::Read),
            FsEffect::WriteRoot => Some(Access::Write),
        }
    }

    fn root_kind(self) -> RootKind {
        match self {
            Access::Read => RootKind::Import,
            Access::Write => RootKind::Export,
        }
    }
}

/// Number of `"path_like": true` keywords anywhere in `v`.
fn count_path_like(v: &CValue) -> usize {
    match v {
        CValue::Obj(o) => {
            let here = usize::from(o.get(PATH_LIKE_KEYWORD) == Some(&CValue::Bool(true)));
            here + o.values().map(count_path_like).sum::<usize>()
        }
        CValue::Arr(a) => a.iter().map(count_path_like).sum(),
        _ => 0,
    }
}

/// Refuses a command whose parameter schema declares `path_like` anywhere but on a top-level
/// property: nested file parameters are not supported, so they could not be checked
/// (`internal/invariant_violated`, fail closed).
fn no_nested_path_like(def: &CommandDef) -> Result<(), BusError> {
    if count_path_like(&def.params_schema) == path_like_params(def).len() {
        Ok(())
    } else {
        Err(invariant("the command declares a nested path_like parameter, which the bus cannot check"))
    }
}

/// The top-level parameters of `def` declared `"path_like": true`, in key order. Nested
/// `path_like` declarations are refused by [`check_raw_paths`] and [`resolve`].
pub fn path_like_params(def: &CommandDef) -> Vec<String> {
    let Some(CValue::Obj(props)) = (match &def.params_schema {
        CValue::Obj(o) => o.get("properties"),
        _ => None,
    }) else {
        return Vec::new();
    };
    props
        .iter()
        .filter(|(_, p)| matches!(p, CValue::Obj(o) if o.get(PATH_LIKE_KEYWORD) == Some(&CValue::Bool(true))))
        .map(|(k, _)| k.clone())
        .collect()
}

/// The declared extension list of the `path_like` parameter `param` of `def`. A parameter that
/// is not declared `path_like` is a caller bug (`internal/invariant_violated`); so is a
/// malformed list. A missing list is empty: nothing is allowed.
fn param_extensions(def: &CommandDef, param: &str) -> Result<BTreeSet<Ext>, BusError> {
    let prop = match &def.params_schema {
        CValue::Obj(o) => match o.get("properties") {
            Some(CValue::Obj(props)) => props.get(param),
            _ => None,
        },
        _ => None,
    };
    let Some(CValue::Obj(prop)) = prop else {
        return Err(invariant("the parameter is not declared by the command"));
    };
    if prop.get(PATH_LIKE_KEYWORD) != Some(&CValue::Bool(true)) {
        return Err(invariant("the parameter is not declared path_like"));
    }
    match prop.get(EXTENSIONS_KEYWORD) {
        None => Ok(BTreeSet::new()),
        Some(v) => ext_list(v).ok_or_else(|| invariant("the parameter's x-extensions list is malformed")),
    }
}

/// Whether a principal of this kind may pass a raw path: `human` only (D-012). One arm per
/// kind, no wildcard.
fn may_pass_raw(kind: PrincipalKind) -> bool {
    match kind {
        PrincipalKind::Human => true,
        PrincipalKind::Agent
        | PrincipalKind::Script
        | PrincipalKind::Service
        | PrincipalKind::Device
        | PrincipalKind::System => false,
    }
}

fn raw_refused(kind: PrincipalKind) -> BusError {
    policy(
        PolicyReason::RawPathRefused,
        &format!("a {} principal must pass a root: handle, not a raw path", kind.as_str()),
    )
}

/// REQ-001 for a whole call: every `path_like` parameter present in `params` must hold a string,
/// and, for any principal but `human`, a handle. Commits nothing; run it before anything else.
/// Errors: `policy/raw_path_refused`, `policy/bad_handle` (a non-string value);
/// `internal/invariant_violated` for a nested `path_like` declaration.
pub fn check_raw_paths(principal: &Principal, def: &CommandDef, params: &CValue) -> Result<(), BusError> {
    no_nested_path_like(def)?;
    let CValue::Obj(values) = params else { return Ok(()) };
    for name in path_like_params(def) {
        match values.get(&name) {
            None => {}
            Some(CValue::Str(s)) => {
                if !is_handle(s) && !may_pass_raw(principal.kind()) {
                    return Err(raw_refused(principal.kind()));
                }
            }
            Some(_) => return Err(bad_handle("a path_like parameter must be a string")),
        }
    }
    Ok(())
}

fn check_extension(name: &str, allowed: &BTreeSet<Ext>, root: Option<&Root>) -> Result<(), BusError> {
    let ok = match file_ext(name) {
        Some(ext) => allowed.contains(&ext) && root.and_then(Root::extensions).map_or(true, |r| r.contains(&ext)),
        None => false,
    };
    if ok {
        Ok(())
    } else {
        Err(policy(PolicyReason::ExtensionRefused, "the file's extension is not allowed for this command or root"))
    }
}

// ------------------------------------------------------------------ resolution

/// A file location that passed every check of this module. Only this module builds one; open it
/// with [`ResolvedPath::open_read`] or [`ResolvedPath::create_new`], which repeat the checks.
#[derive(Debug)]
pub struct ResolvedPath {
    /// The canonical root folder (or, for a human's raw path, the canonical parent folder),
    /// held open as a capability.
    dir: Dir,
    /// The link-free path below `dir`.
    rel: PathBuf,
    root: Option<RootId>,
    access: Access,
}

impl ResolvedPath {
    /// The approved root; `None` for a human's raw path.
    pub fn root(&self) -> Option<&RootId> {
        self.root.as_ref()
    }

    /// The link-free path below the root (the file name alone for a human's raw path).
    pub fn rel_path(&self) -> &Path {
        &self.rel
    }

    /// Read or write.
    pub fn access(&self) -> Access {
        self.access
    }

    /// Opens the resolved file for reading through the held root folder, without following a
    /// link at the final component. Fails with `outside_root` if a component now leads outside
    /// the root, the file itself has become a link or other reparse point, or it has more than
    /// one hard link; `bad_handle` if it is no longer a regular file; `precondition/false` if
    /// it is gone. Only for `read_root`.
    pub fn open_read(&self) -> Result<File, BusError> {
        if self.access != Access::Read {
            return Err(invariant("open_read on a write_root path"));
        }
        open_checked(&self.dir, &self.rel, self.root.is_some())
    }

    /// Creates the resolved file for writing through the held root folder. It must not exist:
    /// the create is exclusive (`O_EXCL` / `CREATE_NEW`), so a file or link that appeared after
    /// resolution gives `would_overwrite` and is left untouched. A folder swapped for an
    /// escaping link gives `outside_root`. Only for `write_root`.
    pub fn create_new(&self) -> Result<File, BusError> {
        if self.access != Access::Write {
            return Err(invariant("create_new on a read_root path"));
        }
        let mut opts = cap_std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        OpenOptionsFollowExt::follow(&mut opts, FollowSymlinks::No);
        self.dir.open_with(&self.rel, &opts).map(cap_std::fs::File::into_std).map_err(|e| {
            // Whatever the error, something now at the target means the create would overwrite.
            if self.dir.symlink_metadata(&self.rel).is_ok() && !is_escape(&e) {
                policy(PolicyReason::WouldOverwrite, "the target already exists; write_root never overwrites")
            } else {
                io_refusal(&e)
            }
        })
    }
}

/// Opens `rel` below `dir` for reading: a regular file, not followed if it is a link
/// (`O_NOFOLLOW` on Unix; `FILE_FLAG_OPEN_REPARSE_POINT` on Windows, then refused if the opened
/// file is a reparse point), so the name whose extension was checked is the file opened. With
/// `root_bound`, a file with more than one hard link is refused (`outside_root`).
pub(crate) fn open_checked(dir: &Dir, rel: &Path, root_bound: bool) -> Result<File, BusError> {
    let meta = dir.symlink_metadata(rel).map_err(|e| io_refusal(&e))?;
    if meta.file_type().is_symlink() {
        return Err(outside("the file is a link"));
    }
    if !meta.is_file() {
        return Err(bad_handle("the handle does not name a regular file"));
    }
    let mut opts = cap_std::fs::OpenOptions::new();
    opts.read(true);
    OpenOptionsFollowExt::follow(&mut opts, FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        // Never block on a FIFO swapped in after the check; never adopt a terminal.
        opts.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
    }
    let file = match dir.open_with(rel, &opts) {
        Ok(f) => f.into_std(),
        Err(e) if is_link_refusal(&e) => return Err(outside("the file became a link")),
        Err(e) => return Err(io_refusal(&e)),
    };
    let meta = file.metadata().map_err(|e| storage(&e))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(outside("the opened file is not a plain regular file"));
    }
    let links = handle_info(&file)?;
    if links.reparse_point {
        return Err(outside("the opened file is a reparse point"));
    }
    if root_bound && links.count > 1 {
        return Err(outside("the file has more than one hard link"));
    }
    Ok(file)
}

/// `ELOOP` (or FreeBSD/NetBSD's `EMLINK`) from an `O_NOFOLLOW` open of a link.
fn is_link_refusal(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(e.raw_os_error(), Some(c) if c == libc::ELOOP || c == libc::EMLINK)
    }
    #[cfg(not(unix))]
    {
        let _ = e;
        false
    }
}

/// What the open handle says about links.
struct HandleInfo {
    /// The hard-link count.
    count: u64,
    /// The file is a reparse point (Windows); never on Unix.
    reparse_point: bool,
}

#[cfg(unix)]
fn handle_info(file: &File) -> Result<HandleInfo, BusError> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata().map_err(|e| storage(&e))?;
    Ok(HandleInfo { count: meta.nlink(), reparse_point: false })
}

/// `FILE_ATTRIBUTE_REPARSE_POINT`.
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u64 = 0x400;

#[cfg(windows)]
fn handle_info(file: &File) -> Result<HandleInfo, BusError> {
    let info = winx::winapi_util::file::information(file).map_err(|e| storage(&e))?;
    Ok(HandleInfo {
        count: info.number_of_links(),
        reparse_point: info.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0,
    })
}

/// Other platforms: no link count is available, so fail closed.
#[cfg(not(any(unix, windows)))]
fn handle_info(_file: &File) -> Result<HandleInfo, BusError> {
    Err(invariant("file handles are not supported on this platform"))
}

/// Resolves the value of the `path_like` parameter `param` of the command `def` for
/// `principal` against the approved `roots` (module documentation, steps 1 to 6).
///
/// Errors: `policy` with reason `raw_path_refused`, `bad_handle`, `bad_root`, `outside_root`,
/// `extension_refused` or `would_overwrite`; `precondition/false` for a missing file or folder;
/// `internal/storage` for other file-system faults; `internal/invariant_violated` if `param`
/// is not a `path_like` parameter of `def`.
pub fn resolve(
    principal: &Principal,
    def: &CommandDef,
    param: &str,
    value: &str,
    roots: &Roots,
) -> Result<ResolvedPath, BusError> {
    no_nested_path_like(def)?;
    let allowed = param_extensions(def, param)?;
    let kind = principal.kind();
    let access = Access::from_fs_effect(def.fs_effect);
    if !is_handle(value) {
        if !may_pass_raw(kind) {
            return Err(raw_refused(kind));
        }
        let access = access.ok_or_else(|| policy(PolicyReason::BadRoot, "the command declares no file effect"))?;
        return resolve_raw(access, value, &allowed);
    }
    let handle = FileHandle::parse(value)?;
    let access = access.ok_or_else(|| policy(PolicyReason::BadRoot, "the command declares no file effect"))?;
    let root = roots.get(&handle.root).ok_or_else(|| policy(PolicyReason::BadRoot, "the root is not configured"))?;
    if root.kind != access.root_kind() {
        return Err(policy(PolicyReason::BadRoot, &format!("root {} is an {} root", root.id, root.kind.as_str())));
    }
    check_extension(handle.rel.file_name(), &allowed, Some(root))?;

    let canon = std::fs::canonicalize(&root.path).map_err(|_| missing("the approved root is not available"))?;
    let root_key = path_key(&canon).ok_or_else(|| missing("the approved root is not an absolute local path"))?;
    let dir = Dir::open_ambient_dir(&canon, cap_std::ambient_authority()).map_err(|e| storage(&e))?;
    let segs = handle.rel.segments();
    match access {
        Access::Read => {
            let comps = walk(&canon, &root_key, segs)?;
            let name = comps.last().ok_or_else(|| bad_handle("the handle resolves to the root itself"))?;
            check_extension(name.to_str().unwrap_or(""), &allowed, Some(root))?;
            let rel: PathBuf = comps.iter().collect();
            // Second, independent check: the OS's canonical path of the file.
            let full = std::fs::canonicalize(canon.join(&rel)).map_err(|e| io_refusal(&e))?;
            if !strictly_below(&full, &root_key) {
                return Err(outside("the file resolves outside its approved root"));
            }
            // The same checks the handler's open repeats: regular file, not a link or reparse
            // point, one hard link.
            drop(open_checked(&dir, &rel, true)?);
            Ok(ResolvedPath { dir, rel, root: Some(handle.root), access })
        }
        Access::Write => {
            let (last, folders) = segs.split_last().ok_or_else(|| bad_handle("the handle names the root itself"))?;
            let comps = walk(&canon, &root_key, folders)?;
            let mut rel: PathBuf = comps.iter().collect();
            let parent = std::fs::canonicalize(canon.join(&rel)).map_err(|e| io_refusal(&e))?;
            if !at_or_below(&parent, &root_key) {
                return Err(outside("the target folder resolves outside its approved root"));
            }
            if !std::fs::metadata(&parent).map_err(|e| io_refusal(&e))?.is_dir() {
                return Err(missing("the target folder does not exist"));
            }
            rel.push(last);
            match dir.symlink_metadata(&rel) {
                Ok(_) => {
                    Err(policy(PolicyReason::WouldOverwrite, "the target already exists; write_root never overwrites"))
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    Ok(ResolvedPath { dir, rel, root: Some(handle.root), access })
                }
                Err(e) => Err(io_refusal(&e)),
            }
        }
    }
}

/// A human's raw path: absolute, no NUL; links are followed, the extension list and the
/// no-overwrite rule apply.
fn resolve_raw(access: Access, value: &str, allowed: &BTreeSet<Ext>) -> Result<ResolvedPath, BusError> {
    if value.len() > MAX_RAW_PATH_BYTES || value.contains('\0') {
        return Err(bad_handle("the raw path is too long or holds NUL"));
    }
    let p = Path::new(value);
    if !p.is_absolute() {
        return Err(bad_handle("a raw path must be absolute"));
    }
    let name = p.file_name().and_then(OsStr::to_str).ok_or_else(|| bad_handle("the raw path names no file"))?;
    check_extension(name, allowed, None)?;
    match access {
        Access::Read => {
            let full = std::fs::canonicalize(p).map_err(|e| io_refusal(&e))?;
            if !std::fs::metadata(&full).map_err(|e| io_refusal(&e))?.is_file() {
                return Err(bad_handle("the raw path does not name a regular file"));
            }
            let name = full.file_name().ok_or_else(|| bad_handle("the raw path names no file"))?;
            check_extension(name.to_str().unwrap_or(""), allowed, None)?;
            let parent = full.parent().ok_or_else(|| bad_handle("the raw path names no file"))?;
            let dir = Dir::open_ambient_dir(parent, cap_std::ambient_authority()).map_err(|e| io_refusal(&e))?;
            Ok(ResolvedPath { dir, rel: PathBuf::from(name), root: None, access })
        }
        Access::Write => {
            let parent = p.parent().ok_or_else(|| bad_handle("the raw path names no file"))?;
            let parent = std::fs::canonicalize(parent).map_err(|e| io_refusal(&e))?;
            let dir = Dir::open_ambient_dir(&parent, cap_std::ambient_authority()).map_err(|e| io_refusal(&e))?;
            match dir.symlink_metadata(name) {
                Ok(_) => {
                    Err(policy(PolicyReason::WouldOverwrite, "the target already exists; write_root never overwrites"))
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    Ok(ResolvedPath { dir, rel: PathBuf::from(name), root: None, access })
                }
                Err(e) => Err(io_refusal(&e)),
            }
        }
    }
}

/// Walks `rel` below `root` (canonical, with comparison key `root_key`) without following
/// links, expanding each link in place and refusing any whose target leaves the root. Returns
/// the link-free components below the root.
fn walk(root: &Path, root_key: &[KeyPart], rel: &[String]) -> Result<Vec<OsString>, BusError> {
    let mut done: Vec<OsString> = Vec::new();
    // Leading components of `done` that were seen on disk; below a missing folder nothing
    // exists, so nothing more is looked up.
    let mut seen = 0usize;
    let mut todo: VecDeque<OsString> = rel.iter().map(OsString::from).collect();
    let mut hops = 0usize;
    while let Some(c) = todo.pop_front() {
        if c.as_os_str() == OsStr::new("..") {
            // Only link targets contribute `..`; every component in `done` is a real folder
            // (links were expanded), so `..` is its parent.
            if done.pop().is_none() {
                return Err(outside("a link leads above its approved root"));
            }
            seen = seen.min(done.len());
            continue;
        }
        if done.len() > seen {
            done.push(c);
            continue;
        }
        let mut p = root.to_path_buf();
        p.extend(done.iter());
        p.push(&c);
        let meta = match std::fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                done.push(c);
                continue;
            }
            Err(e) => return Err(storage(&e)),
        };
        if !meta.file_type().is_symlink() {
            done.push(c);
            seen = done.len();
            continue;
        }
        hops += 1;
        if hops > MAX_LINK_HOPS {
            return Err(outside("too many links (a loop?)"));
        }
        let target = std::fs::read_link(&p).map_err(|e| storage(&e))?;
        let (from_root, comps) = link_components(&target, root_key)?;
        if from_root {
            done.clear();
            seen = 0;
        }
        for x in comps.into_iter().rev() {
            todo.push_front(x);
        }
    }
    Ok(done)
}

/// One element of an absolute path's comparison key: the head (drive, UNC share or `/`) or a
/// name.
#[derive(Debug, Clone)]
enum KeyPart {
    Head(String),
    Name(OsString),
}

/// The head and the following components of an absolute path, or `None` if `p` is not
/// absolute or lies in a device or volume namespace. `.` is dropped; `..` is kept as a name.
fn path_key(p: &Path) -> Option<Vec<KeyPart>> {
    let mut parts = Vec::new();
    let mut it = p.components();
    match it.next()? {
        Component::RootDir => parts.push(KeyPart::Head("/".to_string())),
        Component::Prefix(pre) => {
            let head = match pre.kind() {
                Prefix::Disk(d) | Prefix::VerbatimDisk(d) => format!("{}:", char::from(d).to_ascii_uppercase()),
                Prefix::UNC(s, sh) | Prefix::VerbatimUNC(s, sh) => {
                    format!("\\\\{}\\{}", s.to_string_lossy(), sh.to_string_lossy())
                }
                // Device and other verbatim namespaces (`\\.\`, `\\?\Volume{...}`) are never
                // inside a root.
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => return None,
            };
            if it.next() != Some(Component::RootDir) {
                return None;
            }
            parts.push(KeyPart::Head(head));
        }
        Component::CurDir | Component::ParentDir | Component::Normal(_) => return None,
    }
    for c in it {
        match c {
            Component::Normal(n) => parts.push(KeyPart::Name(n.to_os_string())),
            Component::ParentDir => parts.push(KeyPart::Name(OsString::from(".."))),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    Some(parts)
}

/// Name equality for the containment check: on Windows ASCII letters compare without case
/// and every other code point must match exactly; elsewhere names must match exactly. Any
/// fold the file system does beyond that (Unicode case, a case-insensitive macOS volume, 8.3
/// names) can only make an inside target look outside, which fails closed.
fn same_part(a: &KeyPart, b: &KeyPart) -> bool {
    match (a, b) {
        (KeyPart::Head(x), KeyPart::Head(y)) => x.eq_ignore_ascii_case(y),
        (KeyPart::Name(x), KeyPart::Name(y)) => {
            if cfg!(windows) {
                x.as_encoded_bytes().eq_ignore_ascii_case(y.as_encoded_bytes())
            } else {
                x == y
            }
        }
        (KeyPart::Head(_), KeyPart::Name(_)) | (KeyPart::Name(_), KeyPart::Head(_)) => false,
    }
}

fn has_prefix(key: &[KeyPart], root_key: &[KeyPart]) -> bool {
    key.len() >= root_key.len()
        && root_key.iter().zip(key.iter()).all(|(a, b)| same_part(a, b))
        && key.iter().skip(root_key.len()).all(|k| match k {
            KeyPart::Name(n) => n.as_os_str() != OsStr::new(".."),
            KeyPart::Head(_) => false,
        })
}

/// True if the canonical path `p` is the root or below it.
fn at_or_below(p: &Path, root_key: &[KeyPart]) -> bool {
    path_key(p).is_some_and(|k| has_prefix(&k, root_key))
}

/// True if the canonical path `p` is strictly below the root.
fn strictly_below(p: &Path, root_key: &[KeyPart]) -> bool {
    path_key(p).is_some_and(|k| k.len() > root_key.len() && has_prefix(&k, root_key))
}

/// The components a link target contributes to the walk, and whether they start at the root
/// (absolute target) or at the link's folder (relative target). An absolute target must start
/// with the canonical root's own components; otherwise `outside_root`. Every name a target
/// contributes must be a [`plain_segment`] (or `..`), so a link cannot smuggle in a stream
/// marker or device name.
fn link_components(target: &Path, root_key: &[KeyPart]) -> Result<(bool, Vec<OsString>), BusError> {
    let refuse = || outside("a link leads outside its approved root");
    let check = |n: &OsStr| -> Result<OsString, BusError> {
        match n.to_str() {
            Some(s) if plain_segment(s) => Ok(n.to_os_string()),
            _ => Err(outside("a link target holds a name that is not a plain file name")),
        }
    };
    let first = target.components().next();
    let absolute = matches!(first, Some(Component::Prefix(_)) | Some(Component::RootDir));
    if !absolute {
        let mut out = Vec::new();
        for c in target.components() {
            match c {
                Component::Normal(n) => out.push(check(n)?),
                Component::ParentDir => out.push(OsString::from("..")),
                Component::CurDir => {}
                Component::Prefix(_) | Component::RootDir => return Err(refuse()),
            }
        }
        return Ok((false, out));
    }
    let key = path_key(target).ok_or_else(refuse)?;
    if key.len() < root_key.len() || !root_key.iter().zip(key.iter()).all(|(a, b)| same_part(a, b)) {
        return Err(refuse());
    }
    let mut out = Vec::new();
    for part in key.into_iter().skip(root_key.len()) {
        match part {
            KeyPart::Name(n) if n.as_os_str() == OsStr::new("..") => out.push(n),
            KeyPart::Name(n) => out.push(check(&n)?),
            KeyPart::Head(_) => return Err(refuse()),
        }
    }
    Ok((true, out))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn device_names() {
        for c in ["CON", "nul", "Com1", "lpt9.txt", "aux.tar.gz", "CONIN$", "con .txt", "COM\u{b9}", "clock$"] {
            assert!(windows_device(c), "{c}");
            assert!(!plain_segment(c), "{c}");
        }
        for c in ["console", "com", "lpt10", "nullx", "a", "COM\u{b4}"] {
            assert!(!windows_device(c), "{c}");
        }
    }

    #[test]
    fn names_fold_ascii_only() {
        let n = |s: &str| KeyPart::Name(OsString::from(s));
        assert!(!same_part(&n("\u{212a}"), &n("k")));
        assert!(!same_part(&n("\u{c4}"), &n("\u{e4}")));
        assert_eq!(same_part(&n("ABC"), &n("abc")), cfg!(windows));
    }

    #[test]
    fn extensions() {
        assert_eq!(file_ext("a.STL").map(|e| e.0), Some("stl".into()));
        assert_eq!(file_ext("a.tar.gz").map(|e| e.0), Some("gz".into()));
        for none in ["noext", ".stl", "a.", "a.\u{17f}tl", "a.st-l"] {
            assert!(file_ext(none).is_none(), "{none}");
        }
    }

    #[test]
    fn link_targets_are_checked() {
        let root = if cfg!(windows) { Path::new("C:\\r") } else { Path::new("/r") };
        let key = path_key(root).unwrap();
        assert!(link_components(Path::new("a/b"), &key).is_ok());
        assert!(link_components(Path::new("a/CON"), &key).is_err());
        let out = if cfg!(windows) { Path::new("C:\\other") } else { Path::new("/other") };
        assert!(link_components(out, &key).is_err());
    }
}
