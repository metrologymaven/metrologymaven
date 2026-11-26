//! `fid-design pattern-check --root <patterns dir | spec file> --package <design dir>
//! [--manifest <file>] [--journeys <dir>] [--json]` (spec node `UX.PATTERN.SCHEMA`,
//! REQ-UX.PATTERN.SCHEMA-001 .. -004; parent REQ-UX.PATTERN-001). Tooling, nearest class R0;
//! D0 (`BTreeMap`/`BTreeSet` only, sorted walks, sorted findings, no clock, no network).
//!
//! # `pattern-spec@1`
//!
//! A pattern spec is a Markdown file whose first line opens a fenced `toml` block (the front
//! block), closed by a line holding only the closing fence; prose follows. The front block is
//! split exactly as `component-spec@1` is (see [`crate::comp`]). `design/patterns/TEMPLATE.md`
//! is a commented, checked example of the whole format.
//!
//! Front block keys (a closed set; an unknown key is `PAT-SCHEMA`):
//! - `format = "pattern-spec@1"`, `id = "pat.<name>"` (`<name>` is `[a-z][a-z0-9-]*`, unique
//!   across the root), optional `title`.
//! - `trigger`: text, what starts the pattern.
//! - `[[steps]]`: `n` (the steps are numbered 1, 2, 3, .. in file order), `actor` (one of
//!   [`ACTORS`], the journey-map actors of FLOW.PERSONA.MAPS), `components` (the `comp.<name>`
//!   IDs the step uses; `[]` for a step that shows nothing) and `input` (text: the input the step
//!   expects). At least one step.
//! - `states`: the pattern's states, `[a-z][a-z0-9-]*` names, at least one, unique.
//! - `commands`: registry command ID patterns `<name glob>@<major>` such as
//!   `view.select_*@1`; `[]` for a pattern that invokes no command. See [`CommandPattern`].
//! - `[[keyboard]]`: `step` (a declared step number), `keys` (text), optional `note`. The path is
//!   complete when every step whose actor is a person ([`PERSON_ACTORS`]) has an entry.
//! - `cancel` and `failure`: the cancel and failure paths, arrays of text lines.
//! - `[[journeys]]`: `map` (a journey map ID, `[a-z][a-z0-9-]*`) and `steps` (the step numbers
//!   of that map the pattern serves, positive and unique). At least one entry.
//!
//! # Findings
//!
//! Errors (exit 1):
//! - `PAT-SCHEMA <file>`: no front block, invalid TOML, wrong format, a missing or malformed
//!   key, an unknown key, a duplicate id, an empty root; `PAT-SCHEMA <journey file>`: a file of
//!   `--journeys` that is not a well-formed `journey-map@1`.
//! - `PAT-CANCEL-MISSING <file>`, `PAT-FAILURE-MISSING <file>`: the path is missing, empty or
//!   holds no text line (REQ-004).
//! - `PAT-KEYBOARD-MISSING <file>`: `keyboard` is missing or empty, or a person step has no
//!   keyboard entry (REQ-001 "one complete keyboard path", REQ-004).
//! - `PAT-COMPONENT-UNKNOWN <comp id>`: a step names a component that is not a valid
//!   `component-spec@1` file under `<package>/components/` (REQ-002). A value that is not a
//!   `comp.<name>` ID at all is named by its field (`steps[0].components[1]`).
//! - `PAT-JOURNEY-UNKNOWN <map>` or `<map>/<step>`: with `--journeys`, a journey map ID that no
//!   map in the directory has, or a step that map does not have.
//!
//! Warnings (exit 0 when there is no error):
//! - `PAT-COMMAND-UNMATCHED <pattern>`: with `--manifest`, a command pattern that matches no
//!   command of the registry manifest (REQ-003).
//!
//! Finding subjects never carry a raw spec string: a subject is a file name, a field path, or a
//! value already checked against a grammar (component IDs, journey map IDs, command patterns).
//!
//! # Fail closed
//!
//! Without `--manifest` the command patterns are not matched, and without `--journeys` the
//! journey references are not resolved. The report lists those checks under `unchecked`, and its
//! `ok` is true only when there is no error *and* nothing is unchecked; the exit code follows
//! the spec (errors only), so a run without them can exit 0 with `ok: false`.
//!
//! The manifest is CORE.REG.MAN's (`{ "commands": [{ "id", "major", .. }] }`, at most 64 MiB),
//! read as the person view: every command it lists counts (D-035's agent view hides commands
//! from agents, not from the design tooling). A manifest that is not that shape, holds a
//! malformed ID or major, a duplicate `(id, major)` or more than [`MAX_COMMANDS`] commands
//! cannot be checked against and is an error (exit 2).
//!
//! Exit codes: 0 no errors (warnings allowed), 1 errors, 2 unreadable input, malformed manifest
//! or a limit exceeded.

use crate::comp::{is_ident, is_text, split_front};
use crate::finding::{findings_text, findings_value, read_text, Finding};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Front-block format ID.
pub const SPEC_FORMAT: &str = "pattern-spec@1";
/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-pattern-check@1";
/// Component-spec format ID (`UX.COMP.SCHEMA`).
pub const COMPONENT_FORMAT: &str = crate::comp::SPEC_FORMAT;
/// Journey map format ID (FLOW.PERSONA.MAPS).
pub const JOURNEY_FORMAT: &str = "journey-map@1";
/// Component specs, relative to the package root.
pub const COMPONENTS_DIR: &str = "components";

pub const SCHEMA: &str = "PAT-SCHEMA";
pub const CANCEL_MISSING: &str = "PAT-CANCEL-MISSING";
pub const FAILURE_MISSING: &str = "PAT-FAILURE-MISSING";
pub const KEYBOARD_MISSING: &str = "PAT-KEYBOARD-MISSING";
pub const COMPONENT_UNKNOWN: &str = "PAT-COMPONENT-UNKNOWN";
pub const JOURNEY_UNKNOWN: &str = "PAT-JOURNEY-UNKNOWN";
pub const COMMAND_UNMATCHED: &str = "PAT-COMMAND-UNMATCHED";

/// The `unchecked` entry of a run without `--manifest`.
pub const UNCHECKED_COMMANDS: &str = "commands";
/// The `unchecked` entry of a run without `--journeys`.
pub const UNCHECKED_JOURNEYS: &str = "journeys";

/// Largest spec, component spec or journey map read.
pub const MAX_SPEC_BYTES: u64 = 256 * 1024;
/// Largest journey map read (FLOW.PERSONA.MAPS REQ-004: 1 MiB).
pub const MAX_JOURNEY_BYTES: u64 = 1024 * 1024;
/// Largest registry manifest read (CORE.REG.MAN: 64 MiB).
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
/// Deepest JSON nesting read from a manifest or journey map.
pub const MAX_JSON_DEPTH: usize = 64;
/// Most commands in a manifest (CORE.REG.MAN `too_many_commands`).
pub const MAX_COMMANDS: usize = 50_000;
/// Most files under one root, components directory or journeys directory.
pub const MAX_FILES: usize = 512;
/// Most entries in any one array or table of the front block.
pub const MAX_ITEMS: usize = 256;
/// Most steps in one journey map (FLOW.PERSONA.MAPS REQ-004).
pub const MAX_JOURNEY_STEPS: usize = 500;
/// Longest command pattern, in bytes.
pub const MAX_PATTERN_BYTES: usize = 128;
/// Most `*` in one command pattern.
pub const MAX_STARS: usize = 8;
/// Largest step number accepted anywhere.
pub const MAX_STEP: i64 = 100_000;

/// Step actors: the journey-map actors of FLOW.PERSONA.MAPS. Sorted.
pub const ACTORS: [&str; 5] = ["admin", "agent", "human", "operator", "system"];
/// Actors that are people and so need a keyboard entry. Sorted.
pub const PERSON_ACTORS: [&str; 3] = ["admin", "human", "operator"];

const TOP_KEYS: [&str; 11] =
    ["cancel", "commands", "failure", "format", "id", "journeys", "keyboard", "states", "steps", "title", "trigger"];

/// The inputs of one run.
#[derive(Debug, Clone, Copy)]
pub struct Options<'a> {
    pub root: &'a Path,
    pub package: &'a Path,
    pub manifest: Option<&'a Path>,
    pub journeys: Option<&'a Path>,
}

/// One checked spec file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecSummary {
    /// File name relative to the root.
    pub file: String,
    /// The `id`, when it was valid.
    pub id: Option<String>,
}

/// The result of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub specs: Vec<SpecSummary>,
    /// Component IDs found in the package.
    pub components: Vec<String>,
    /// Commands in the manifest, when one was given.
    pub commands: Option<usize>,
    /// Journey maps read, when `--journeys` was given.
    pub journeys: Option<usize>,
    /// Errors (exit 1).
    pub findings: Vec<Finding>,
    /// Warnings (exit 0).
    pub warnings: Vec<Finding>,
    /// Checks that were not made (sorted).
    pub unchecked: Vec<&'static str>,
}

/// A registry command ID pattern `<name glob>@<major>`.
///
/// The glob uses the command-name alphabet (`[a-z0-9_.]`) plus `*`, which matches any run of
/// name characters (dots included, possibly empty). It starts with a literal lowercase letter or
/// digit, holds at most [`MAX_STARS`] `*` and never two in a row. The major is 1 to 5 decimal
/// digits without a leading zero, at least 1, and is compared exactly. The whole pattern is at
/// most [`MAX_PATTERN_BYTES`] bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CommandPattern {
    glob: String,
    major: u32,
}

impl CommandPattern {
    /// Parses a pattern; `Err` says why it is malformed.
    pub fn parse(s: &str) -> Result<CommandPattern, &'static str> {
        if s.len() > MAX_PATTERN_BYTES {
            return Err("longer than 128 bytes");
        }
        let Some((glob, major)) = s.rsplit_once('@') else {
            return Err("has no @<major>");
        };
        let mb = major.as_bytes();
        if mb.is_empty() || mb.len() > 5 || !mb.iter().all(u8::is_ascii_digit) || mb.first() == Some(&b'0') {
            return Err("the major must be 1 to 5 decimal digits without a leading zero");
        }
        let major: u32 = major.parse().map_err(|_| "the major is not a number")?;
        let gb = glob.as_bytes();
        if !gb.first().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
            return Err("the name must start with a lowercase letter or digit");
        }
        if !gb.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'*')) {
            return Err("the name may hold only a-z, 0-9, _, . and *");
        }
        if gb.windows(2).any(|w| w == b"**") {
            return Err("two * in a row");
        }
        if gb.iter().filter(|&&c| c == b'*').count() > MAX_STARS {
            return Err("more than 8 *");
        }
        Ok(CommandPattern { glob: glob.to_string(), major })
    }

    /// The literal text before the first `*`.
    pub fn prefix(&self) -> &str {
        self.glob.split('*').next().unwrap_or_default()
    }

    pub fn major(&self) -> u32 {
        self.major
    }

    /// Whether the pattern matches command `name@major`.
    pub fn matches(&self, name: &str, major: u32) -> bool {
        major == self.major && glob_match(&self.glob, name)
    }
}

/// `*`-only glob match (iterative, no recursion; O(|glob| * |name|) worst case).
pub fn glob_match(glob: &str, name: &str) -> bool {
    let (p, s) = (glob.as_bytes(), name.as_bytes());
    let (mut pi, mut si) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        match (p.get(pi), s.get(si)) {
            (Some(b'*'), _) => {
                star = Some((pi, si));
                pi += 1;
            }
            (Some(a), Some(b)) if a == b => {
                pi += 1;
                si += 1;
            }
            _ => match star {
                Some((sp, ss)) => {
                    pi = sp + 1;
                    si = ss + 1;
                    star = Some((sp, ss + 1));
                }
                None => return false,
            },
        }
    }
    p.get(pi..).is_some_and(|rest| rest.iter().all(|&c| c == b'*'))
}

/// The commands of a registry manifest, as `(major, name)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Commands(BTreeSet<(u32, String)>);

impl Commands {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether any command matches `p` (a range scan over the pattern's literal prefix).
    pub fn any_match(&self, p: &CommandPattern) -> bool {
        let prefix = p.prefix();
        self.0
            .range((p.major, prefix.to_string())..)
            .take_while(|(m, n)| *m == p.major && n.starts_with(prefix))
            .any(|(m, n)| p.matches(n, *m))
    }
}

/// Parses CORE.REG.MAN manifest text. `Err` means it cannot be checked against (exit 2).
pub fn parse_manifest(text: &str) -> Result<Commands, String> {
    let v = fid_canon::parse_with_limits(text.as_bytes(), MAX_MANIFEST_BYTES as usize, MAX_JSON_DEPTH)
        .map_err(|e| format!("not valid JSON ({})", e.code()))?;
    let CValue::Obj(m) = &v else {
        return Err("the manifest must be a JSON object".into());
    };
    let Some(CValue::Arr(commands)) = m.get("commands") else {
        return Err("the manifest has no `commands` array".into());
    };
    if commands.len() > MAX_COMMANDS {
        return Err(format!("the manifest lists more than {MAX_COMMANDS} commands"));
    }
    let mut out = BTreeSet::new();
    for (i, c) in commands.iter().enumerate() {
        let CValue::Obj(c) = c else {
            return Err(format!("commands[{i}] is not an object"));
        };
        let id = match c.get("id") {
            Some(CValue::Str(s)) if s.len() <= 256 && fid_types::CommandName::is_valid(s) => s,
            _ => return Err(format!("commands[{i}]: `id` missing or not a command name")),
        };
        let major = match c.get("major") {
            Some(CValue::Num(n)) if n.fract() == 0.0 && *n >= 1.0 && *n <= 99_999.0 => *n as u32,
            Some(CValue::U64(n)) if (1..=99_999).contains(n) => *n as u32,
            Some(CValue::I64(n)) if (1..=99_999).contains(n) => *n as u32,
            _ => return Err(format!("commands[{i}]: `major` missing or not an integer from 1 to 99999")),
        };
        if !out.insert((major, id.clone())) {
            return Err(format!("commands[{i}]: {id}@{major} is listed twice"));
        }
    }
    Ok(Commands(out))
}

/// A journey map: its step numbers.
type Journeys = BTreeMap<String, BTreeSet<u64>>;

/// The `*.<ext>` regular files directly under `dir`, sorted. A symbolic link or a non-UTF-8
/// name is a finding with `code`; subdirectories are skipped.
fn list_files(
    dir: &Path,
    ext: &str,
    code: &'static str,
    f: &mut Vec<Finding>,
) -> Result<Vec<(String, PathBuf)>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for e in rd {
        let e = e.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let Some(name) = e.file_name().to_str().map(str::to_string) else {
            f.push(Finding::new(code, dir.display().to_string(), "a file name is not valid UTF-8"));
            continue;
        };
        if !name.ends_with(ext) {
            continue;
        }
        let ft = e.file_type().map_err(|err| format!("cannot read {}: {err}", e.path().display()))?;
        if ft.is_symlink() {
            f.push(Finding::new(code, &name, "symbolic links are not followed"));
        } else if ft.is_file() {
            out.push((name, e.path()));
            if out.len() > MAX_FILES {
                return Err(format!("more than {MAX_FILES} files under {}", dir.display()));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The spec files: `root` itself when it is a file, else the `*.md` files directly under it.
fn spec_files(root: &Path, f: &mut Vec<Finding>) -> Result<Vec<(String, PathBuf)>, String> {
    let meta = std::fs::symlink_metadata(root).map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
    if meta.is_file() {
        let name = root
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("--root {} has a non-UTF-8 file name", root.display()))?;
        return Ok(vec![(name.to_string(), root.to_path_buf())]);
    }
    if !meta.is_dir() {
        return Err(format!("--root {} is neither a file nor a directory", root.display()));
    }
    list_files(root, ".md", SCHEMA, f)
}

/// The component IDs of the package: every `components/*.md` whose front block is a
/// `component-spec@1` with a valid `comp.<name>` id. Whether those specs pass `comp-check` is
/// `comp-check`'s job. A package without a `components/` directory has no components.
pub fn load_components(package: &Path) -> Result<BTreeSet<String>, String> {
    let meta =
        std::fs::symlink_metadata(package).map_err(|e| format!("cannot read --package {}: {e}", package.display()))?;
    if !meta.is_dir() {
        return Err(format!("--package {} is not a directory", package.display()));
    }
    let dir = package.join(COMPONENTS_DIR);
    match std::fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
        Ok(m) if !m.is_dir() => return Ok(BTreeSet::new()),
        Ok(_) => {}
    }
    let mut ignored = Vec::new();
    let mut out = BTreeSet::new();
    for (_, path) in list_files(&dir, ".md", SCHEMA, &mut ignored)? {
        let text = read_text(&path, MAX_SPEC_BYTES)?;
        if let Some(id) = component_id(&text) {
            out.insert(id);
        }
    }
    Ok(out)
}

/// The `id` of a `component-spec@1` text, when it has one.
pub fn component_id(text: &str) -> Option<String> {
    let (block, _) = split_front(text).ok()?;
    let doc: toml::Table = block.parse().ok()?;
    if doc.get("format").and_then(toml::Value::as_str) != Some(COMPONENT_FORMAT) {
        return None;
    }
    match doc.get("id") {
        Some(toml::Value::String(s)) if is_comp_id(s) => Some(s.clone()),
        _ => None,
    }
}

fn is_comp_id(s: &str) -> bool {
    s.strip_prefix("comp.").is_some_and(is_ident)
}

fn json_step(v: Option<&CValue>) -> Option<u64> {
    match v {
        Some(CValue::Num(n)) if n.fract() == 0.0 && *n >= 1.0 && *n <= MAX_STEP as f64 => Some(*n as u64),
        Some(CValue::U64(n)) if (1..=MAX_STEP as u64).contains(n) => Some(*n),
        Some(CValue::I64(n)) if (1..=MAX_STEP).contains(n) => Some(*n as u64),
        _ => None,
    }
}

/// Reads the `*.json` journey maps directly under `dir`. A file that is not a well-formed
/// `journey-map@1` (object, `format`, `map_id` ident, `steps` objects with distinct positive `n`)
/// is a `PAT-SCHEMA` finding naming the file, and its map is not known.
pub fn load_journeys(dir: &Path, f: &mut Vec<Finding>) -> Result<Journeys, String> {
    let md = std::fs::symlink_metadata(dir).map_err(|e| format!("cannot read --journeys {}: {e}", dir.display()))?;
    if md.file_type().is_symlink() || !md.is_dir() {
        return Err(format!("--journeys {} must be a plain directory", dir.display()));
    }
    let mut out = Journeys::new();
    for (name, path) in list_files(dir, ".json", SCHEMA, f)? {
        let text = read_text(&path, MAX_JOURNEY_BYTES)?;
        let bad = |why: &str| Finding::new(SCHEMA, &name, format!("journey map: {why}"));
        let v = match fid_canon::parse_with_limits(text.as_bytes(), MAX_JOURNEY_BYTES as usize, MAX_JSON_DEPTH) {
            Ok(v) => v,
            Err(e) => {
                f.push(bad(&format!("not valid JSON ({})", e.code())));
                continue;
            }
        };
        let CValue::Obj(m) = &v else {
            f.push(bad("not a JSON object"));
            continue;
        };
        if m.get("format") != Some(&CValue::Str(JOURNEY_FORMAT.to_string())) {
            f.push(bad(&format!("format must be {JOURNEY_FORMAT:?}")));
            continue;
        }
        let map_id = match m.get("map_id") {
            Some(CValue::Str(s)) if is_ident(s) => s.clone(),
            _ => {
                f.push(bad("map_id must match [a-z][a-z0-9-]*"));
                continue;
            }
        };
        let steps = match m.get("steps") {
            Some(CValue::Arr(a)) if a.len() <= MAX_JOURNEY_STEPS => a,
            _ => {
                f.push(bad(&format!("steps must be an array of at most {MAX_JOURNEY_STEPS} steps")));
                continue;
            }
        };
        let mut ns = BTreeSet::new();
        let mut ok = true;
        for s in steps {
            let n = match s {
                CValue::Obj(o) => json_step(o.get("n")),
                _ => None,
            };
            match n {
                Some(n) if ns.insert(n) => {}
                _ => ok = false,
            }
        }
        if !ok {
            f.push(bad("every step must be an object with a distinct positive integer n"));
            continue;
        }
        if out.contains_key(&map_id) {
            f.push(bad(&format!("map_id {map_id} is also used by another file")));
            continue;
        }
        out.insert(map_id, ns);
    }
    Ok(out)
}

/// What one spec references, for the checks that need other inputs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Refs {
    /// `(component id, field path)`.
    pub components: Vec<(String, String)>,
    /// Command patterns (their text, already valid).
    pub commands: Vec<(String, CommandPattern)>,
    /// `(map, steps)`.
    pub journeys: Vec<(String, Vec<u64>)>,
}

/// One parsed spec: its `id` when valid and what it references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub id: Option<String>,
    pub refs: Refs,
}

struct Ctx<'a> {
    file: &'a str,
    f: &'a mut Vec<Finding>,
}

impl Ctx<'_> {
    fn at_file(&mut self, code: &'static str, detail: impl AsRef<str>) {
        let file = self.file.to_string();
        self.f.push(Finding::new(code, file, detail.as_ref().to_string()));
    }

    fn push(&mut self, code: &'static str, subject: &str, detail: impl AsRef<str>) {
        self.f.push(Finding::new(code, subject, format!("{}: {}", self.file, detail.as_ref())));
    }

    fn schema(&mut self, detail: impl AsRef<str>) {
        self.at_file(SCHEMA, detail);
    }

    fn text(&mut self, v: Option<&toml::Value>, at: &str) -> Option<String> {
        match v {
            Some(toml::Value::String(s)) if is_text(s) => Some(s.clone()),
            Some(_) => {
                self.schema(format!(
                    "{at} must be non-blank text of at most 1000 characters without control characters"
                ));
                None
            }
            None => {
                self.schema(format!("missing key `{at}`"));
                None
            }
        }
    }

    fn step_no(&mut self, v: Option<&toml::Value>, at: &str) -> Option<u64> {
        match v {
            Some(toml::Value::Integer(n)) if (1..=MAX_STEP).contains(n) => Some(*n as u64),
            _ => {
                self.schema(format!("{at} must be an integer from 1 to {MAX_STEP}"));
                None
            }
        }
    }

    /// An array of at most [`MAX_ITEMS`] entries.
    fn array<'v>(&mut self, v: &'v toml::Value, at: &str) -> Option<&'v Vec<toml::Value>> {
        match v {
            toml::Value::Array(a) if a.len() <= MAX_ITEMS => Some(a),
            toml::Value::Array(_) => {
                self.schema(format!("{at} has more than {MAX_ITEMS} entries"));
                None
            }
            other => {
                self.schema(format!("{at} must be an array, not {}", other.type_str()));
                None
            }
        }
    }

    /// The tables of an array, each holding only `allowed` keys and every key of `required`.
    fn tables<'v>(
        &mut self,
        v: &'v toml::Value,
        at: &str,
        allowed: &[&str],
        required: &[&str],
    ) -> Vec<&'v toml::Table> {
        let Some(items) = self.array(v, at) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let toml::Value::Table(t) = item else {
                self.schema(format!("{at}[{i}] must be a table"));
                continue;
            };
            let mut ok = true;
            for k in t.keys() {
                if !allowed.contains(&k.as_str()) {
                    self.schema(format!("{at}[{i}]: unknown key `{k}`"));
                    ok = false;
                }
            }
            for k in required {
                if !t.contains_key(*k) {
                    self.schema(format!("{at}[{i}]: missing key `{k}`"));
                    ok = false;
                }
            }
            if ok {
                out.push(t);
            }
        }
        out
    }
}

/// Checks one spec text on its own (everything but the cross-checks against the package,
/// manifest and journeys, which [`check`] makes from the returned [`Refs`]).
pub fn check_text(file: &str, text: &str, f: &mut Vec<Finding>) -> Parsed {
    let mut c = Ctx { file, f };
    let mut out = Parsed::default();
    let block = match split_front(text) {
        Ok((b, _)) => b,
        Err(e) => {
            c.schema(e);
            return out;
        }
    };
    let doc: toml::Table = match block.parse() {
        Ok(t) => t,
        Err(e) => {
            c.schema(format!("the front block is not valid TOML: {}", e.message()));
            return out;
        }
    };
    for k in doc.keys() {
        if !TOP_KEYS.contains(&k.as_str()) {
            c.schema(format!("unknown front-block key `{k}`"));
        }
    }
    if doc.get("format").and_then(toml::Value::as_str) != Some(SPEC_FORMAT) {
        c.schema(format!("format must be {SPEC_FORMAT:?}"));
        return out;
    }
    out.id = match doc.get("id") {
        Some(toml::Value::String(s)) if s.strip_prefix("pat.").is_some_and(is_ident) => Some(s.clone()),
        _ => {
            c.schema("id must be \"pat.<name>\" with <name> matching [a-z][a-z0-9-]*");
            None
        }
    };
    if doc.contains_key("title") {
        c.text(doc.get("title"), "title");
    }
    c.text(doc.get("trigger"), "trigger");
    let persons = check_steps(&mut c, doc.get("steps"), &mut out.refs);
    check_states(&mut c, doc.get("states"));
    check_commands(&mut c, doc.get("commands"), &mut out.refs);
    check_keyboard(&mut c, doc.get("keyboard"), &persons);
    check_path(&mut c, doc.get("cancel"), "cancel", CANCEL_MISSING);
    check_path(&mut c, doc.get("failure"), "failure", FAILURE_MISSING);
    check_journeys(&mut c, doc.get("journeys"), &mut out.refs);
    out
}

/// `[[steps]]`: returns step number to "the actor is a person" (`None` when the steps are
/// unusable, so the keyboard path is not judged against them).
fn check_steps(c: &mut Ctx<'_>, v: Option<&toml::Value>, refs: &mut Refs) -> Option<BTreeMap<u64, bool>> {
    let Some(v) = v else {
        c.schema("missing key `steps`");
        return None;
    };
    let keys = ["actor", "components", "input", "n"];
    let len = c.array(v, "steps").map(Vec::len)?;
    let items = c.tables(v, "steps", &keys, &keys);
    if len == 0 {
        c.schema("steps must list at least one step");
        return None;
    }
    let mut usable = items.len() == len;
    let mut steps = BTreeMap::new();
    for (i, t) in items.into_iter().enumerate() {
        let at = format!("steps[{i}]");
        match c.step_no(t.get("n"), &format!("{at}.n")) {
            Some(n) if n == i as u64 + 1 => {}
            Some(n) => {
                c.schema(format!("{at}.n is {n}; steps are numbered 1, 2, 3, .. in order"));
                usable = false;
            }
            None => usable = false,
        }
        let person = match t.get("actor") {
            Some(toml::Value::String(a)) if ACTORS.contains(&a.as_str()) => PERSON_ACTORS.contains(&a.as_str()),
            _ => {
                c.schema(format!("{at}.actor must be one of {}", ACTORS.join(", ")));
                usable = false;
                true
            }
        };
        steps.insert(i as u64 + 1, person);
        c.text(t.get("input"), &format!("{at}.input"));
        if let Some(comps) = t.get("components") {
            let at = format!("{at}.components");
            if let Some(items) = c.array(comps, &at) {
                for (j, x) in items.iter().enumerate() {
                    let field = format!("{at}[{j}]");
                    match x {
                        toml::Value::String(s) if is_comp_id(s) => refs.components.push((s.clone(), field)),
                        toml::Value::String(_) => {
                            // The raw string is untrusted: the finding names the field.
                            c.push(COMPONENT_UNKNOWN, &field, "not a comp.<name> component ID");
                        }
                        other => c.schema(format!("{field} must be a string, not {}", other.type_str())),
                    }
                }
            }
        }
    }
    usable.then_some(steps)
}

/// `states`: unique `[a-z][a-z0-9-]*` names, at least one.
fn check_states(c: &mut Ctx<'_>, v: Option<&toml::Value>) {
    let Some(v) = v else {
        c.schema("missing key `states`");
        return;
    };
    let Some(items) = c.array(v, "states") else {
        return;
    };
    if items.is_empty() {
        c.schema("states must list at least one state");
    }
    let mut seen = BTreeSet::new();
    for (i, s) in items.iter().enumerate() {
        match s {
            toml::Value::String(s) if is_ident(s) => {
                if !seen.insert(s.as_str()) {
                    c.schema(format!("states lists `{s}` twice"));
                }
            }
            _ => c.schema(format!("states[{i}] must match [a-z][a-z0-9-]*")),
        }
    }
}

/// `commands`: valid, unique command patterns (`[]` allowed).
fn check_commands(c: &mut Ctx<'_>, v: Option<&toml::Value>, refs: &mut Refs) {
    let Some(v) = v else {
        c.schema("missing key `commands` (commands = [] for a pattern that runs no command)");
        return;
    };
    let Some(items) = c.array(v, "commands") else {
        return;
    };
    let mut seen = BTreeSet::new();
    for (i, s) in items.iter().enumerate() {
        let toml::Value::String(s) = s else {
            c.schema(format!("commands[{i}] must be a string"));
            continue;
        };
        match CommandPattern::parse(s) {
            Ok(p) => {
                if seen.insert(s.clone()) {
                    refs.commands.push((s.clone(), p));
                } else {
                    c.schema(format!("commands lists `{s}` twice"));
                }
            }
            Err(why) => c.schema(format!("commands[{i}] is not a command pattern <name glob>@<major>: {why}")),
        }
    }
}

/// `[[keyboard]]` (REQ-001, REQ-004): non-empty, and every person step has an entry.
fn check_keyboard(c: &mut Ctx<'_>, v: Option<&toml::Value>, steps: &Option<BTreeMap<u64, bool>>) {
    let Some(v) = v else {
        c.at_file(KEYBOARD_MISSING, "no keyboard path");
        return;
    };
    let Some(len) = c.array(v, "keyboard").map(Vec::len) else {
        return;
    };
    if len == 0 {
        c.at_file(KEYBOARD_MISSING, "the keyboard path is empty");
        return;
    }
    let items = c.tables(v, "keyboard", &["keys", "note", "step"], &["keys", "step"]);
    let mut covered = BTreeSet::new();
    for (i, t) in items.into_iter().enumerate() {
        let at = format!("keyboard[{i}]");
        let n = c.step_no(t.get("step"), &format!("{at}.step"));
        let keys = c.text(t.get("keys"), &format!("{at}.keys"));
        if t.contains_key("note") {
            c.text(t.get("note"), &format!("{at}.note"));
        }
        if let (Some(n), Some(_)) = (n, keys) {
            if let Some(steps) = steps {
                if !steps.contains_key(&n) {
                    c.schema(format!("{at}.step {n} is not a declared step"));
                }
            }
            covered.insert(n);
        }
    }
    if let Some(steps) = steps {
        for (n, person) in steps {
            if *person && !covered.contains(n) {
                c.at_file(KEYBOARD_MISSING, format!("step {n} is a person's step with no keyboard entry"));
            }
        }
    }
}

/// `cancel` or `failure` (REQ-004): an array holding at least one text line.
fn check_path(c: &mut Ctx<'_>, v: Option<&toml::Value>, key: &str, code: &'static str) {
    let Some(v) = v else {
        c.at_file(code, format!("no {key} path"));
        return;
    };
    let Some(items) = c.array(v, key) else {
        c.at_file(code, format!("no usable {key} path"));
        return;
    };
    let mut lines = 0;
    for (i, x) in items.iter().enumerate() {
        match x {
            toml::Value::String(s) if is_text(s) => lines += 1,
            _ => c.schema(format!("{key}[{i}] must be non-blank text without control characters")),
        }
    }
    if lines == 0 {
        c.at_file(code, format!("the {key} path is empty"));
    }
}

/// `[[journeys]]`: at least one `{map, steps}`.
fn check_journeys(c: &mut Ctx<'_>, v: Option<&toml::Value>, refs: &mut Refs) {
    let Some(v) = v else {
        c.schema("missing key `journeys`");
        return;
    };
    let Some(len) = c.array(v, "journeys").map(Vec::len) else {
        return;
    };
    if len == 0 {
        c.schema("journeys must list at least one journey map step");
    }
    let keys = ["map", "steps"];
    let items = c.tables(v, "journeys", &keys, &keys);
    let mut maps = BTreeSet::new();
    for (i, t) in items.into_iter().enumerate() {
        let at = format!("journeys[{i}]");
        let map = match t.get("map") {
            Some(toml::Value::String(s)) if is_ident(s) => Some(s.clone()),
            _ => {
                c.schema(format!("{at}.map must be a journey map ID matching [a-z][a-z0-9-]*"));
                None
            }
        };
        let mut steps = Vec::new();
        let mut seen = BTreeSet::new();
        if let Some(items) = t.get("steps").and_then(|s| c.array(s, &format!("{at}.steps"))) {
            if items.is_empty() {
                c.schema(format!("{at}.steps must list at least one step"));
            }
            for (j, s) in items.iter().enumerate() {
                if let Some(n) = c.step_no(Some(s), &format!("{at}.steps[{j}]")) {
                    if seen.insert(n) {
                        steps.push(n);
                    } else {
                        c.schema(format!("{at}.steps lists {n} twice"));
                    }
                }
            }
        }
        if let Some(map) = map {
            if !maps.insert(map.clone()) {
                c.schema(format!("journeys lists map `{map}` twice"));
            }
            refs.journeys.push((map, steps));
        }
    }
}

/// Runs `pattern-check`. `Err` is an unreadable input, a malformed manifest or a limit (exit 2).
pub fn check(o: &Options<'_>) -> Result<Report, String> {
    let components = load_components(o.package)?;
    let commands = match o.manifest {
        None => None,
        Some(p) => {
            let text = read_text(p, MAX_MANIFEST_BYTES)?;
            Some(parse_manifest(&text).map_err(|e| format!("--manifest {}: {e}", p.display()))?)
        }
    };
    let mut f = Vec::new();
    let journeys = match o.journeys {
        None => None,
        Some(d) => Some(load_journeys(d, &mut f)?),
    };
    let files = spec_files(o.root, &mut f)?;
    if files.is_empty() {
        f.push(Finding::new(SCHEMA, "--root", "no pattern spec (*.md) found"));
    }
    let mut w = Vec::new();
    let mut specs = Vec::new();
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let mut matched: BTreeMap<CommandPattern, bool> = BTreeMap::new();
    for (name, path) in &files {
        let text = read_text(path, MAX_SPEC_BYTES)?;
        let p = check_text(name, &text, &mut f);
        let push = |f: &mut Vec<Finding>, code: &'static str, subject: &str, detail: String| {
            f.push(Finding::new(code, subject, format!("{name}: {detail}")));
        };
        for (id, field) in &p.refs.components {
            if !components.contains(id) {
                push(&mut f, COMPONENT_UNKNOWN, id, format!("{field} is not a component spec in the package"));
            }
        }
        if let Some(cmds) = &commands {
            for (text, pat) in &p.refs.commands {
                let hit = *matched.entry(pat.clone()).or_insert_with(|| cmds.any_match(pat));
                if !hit {
                    push(&mut w, COMMAND_UNMATCHED, text, "matches no command in the registry manifest".into());
                }
            }
        }
        if let Some(js) = &journeys {
            for (map, steps) in &p.refs.journeys {
                match js.get(map) {
                    None => push(&mut f, JOURNEY_UNKNOWN, map, "no journey map has this map_id".into()),
                    Some(have) => {
                        for n in steps {
                            if !have.contains(n) {
                                push(
                                    &mut f,
                                    JOURNEY_UNKNOWN,
                                    &format!("{map}/{n}"),
                                    format!("map {map} has no step {n}"),
                                );
                            }
                        }
                    }
                }
            }
        }
        if let Some(id) = &p.id {
            if let Some(first) = ids.get(id) {
                f.push(Finding::new(SCHEMA, name, format!("id {id} is also used by {first}")));
            } else {
                ids.insert(id.clone(), name.clone());
            }
        }
        specs.push(SpecSummary { file: name.clone(), id: p.id });
    }
    let mut unchecked = Vec::new();
    if commands.is_none() {
        unchecked.push(UNCHECKED_COMMANDS);
    }
    if journeys.is_none() {
        unchecked.push(UNCHECKED_JOURNEYS);
    }
    f.sort();
    f.dedup();
    w.sort();
    w.dedup();
    Ok(Report {
        specs,
        components: components.into_iter().collect(),
        commands: commands.map(|c| c.len()),
        journeys: journeys.map(|j| j.len()),
        findings: f,
        warnings: w,
        unchecked,
    })
}

impl Report {
    /// 0 with no error (warnings allowed), 1 with errors.
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    /// No error and every check made.
    pub fn ok(&self) -> bool {
        self.findings.is_empty() && self.unchecked.is_empty()
    }

    pub fn to_cvalue(&self) -> CValue {
        let specs = self
            .specs
            .iter()
            .map(|s| {
                let mut m = BTreeMap::new();
                m.insert("file".to_string(), CValue::Str(s.file.clone()));
                m.insert("id".to_string(), s.id.clone().map_or(CValue::Null, CValue::Str));
                CValue::Obj(m)
            })
            .collect();
        let count = |n: Option<usize>| n.map_or(CValue::Null, |n| CValue::Num(n as f64));
        let mut m = BTreeMap::new();
        m.insert("commands".to_string(), count(self.commands));
        m.insert(
            "components".to_string(),
            CValue::Arr(self.components.iter().map(|c| CValue::Str(c.clone())).collect()),
        );
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("journeys".to_string(), count(self.journeys));
        m.insert("ok".to_string(), CValue::Bool(self.ok()));
        m.insert("specs".to_string(), CValue::Arr(specs));
        m.insert(
            "unchecked".to_string(),
            CValue::Arr(self.unchecked.iter().map(|u| CValue::Str((*u).to_string())).collect()),
        );
        m.insert("warnings".to_string(), findings_value(&self.warnings));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("fid-design pattern-check: {} spec(s)\n", self.specs.len());
        for x in &self.specs {
            s.push_str(&format!("  {}: {}\n", x.file, x.id.as_deref().unwrap_or("(no valid id)")));
        }
        for x in &self.warnings {
            s.push_str(&format!("warning: {x}\n"));
        }
        for u in &self.unchecked {
            let why = if *u == UNCHECKED_COMMANDS { "no --manifest" } else { "no --journeys" };
            s.push_str(&format!("not checked: {u} ({why})\n"));
        }
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn closed_lists_are_sorted() {
        assert!(TOP_KEYS.windows(2).all(|w| w[0] < w[1]));
        assert!(ACTORS.windows(2).all(|w| w[0] < w[1]));
        assert!(PERSON_ACTORS.iter().all(|p| ACTORS.contains(p)));
    }

    #[test]
    fn globs() {
        assert!(glob_match("view.select_*", "view.select_one"));
        assert!(glob_match("view.select_*", "view.select_"));
        assert!(!glob_match("view.select_*", "view.selec"));
        assert!(glob_match("a*b*c", "aXbYbZc"));
        assert!(!glob_match("a*b*c", "aXbYbZ"));
        assert!(glob_match("view.*", "view.a.b"));
        assert!(glob_match("x", "x"));
        assert!(!glob_match("x", "xy"));
    }

    #[test]
    fn patterns() {
        let p = CommandPattern::parse("view.select_*@1").unwrap();
        assert_eq!(p.prefix(), "view.select_");
        assert_eq!(p.major(), 1);
        assert!(p.matches("view.select_all", 1));
        assert!(!p.matches("view.select_all", 2));
        for bad in ["view.x", "view.x@0", "view.x@01", "view.x@123456", "*.x@1", "View.x@1", "v**@1", "v.x@a", "@1", ""]
        {
            assert!(CommandPattern::parse(bad).is_err(), "{bad}");
        }
        assert!(CommandPattern::parse(&format!("{}@1", "a".repeat(127))).is_err());
        assert!(CommandPattern::parse("a*b*c*d*e*f*g*h*i*j@1").is_err());
        assert!(CommandPattern::parse("a*b*c*d*e*f*g*h*i@1").is_ok());
    }
}
